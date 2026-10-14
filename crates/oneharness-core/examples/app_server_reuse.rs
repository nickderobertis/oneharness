//! The shared Codex app-server's sustained-reuse workload, entered through
//! oneharness-core's own pool: `live/control/app-server-reuse.sh` runs it.
//!
//! ```text
//! app_server_reuse --codex BIN --out PATH [--leases N] [--hold-ms MS]
//!                  [--settle-ms MS] [--cwd DIR]
//! ```
//!
//! It takes N sequential leases on one pooled server through the same
//! [`PooledSession`] a `--control` dispatch's lease, connection and release go
//! through: each lease `initialize`s, starts a thread (`thread/start`), runs
//! one `thread/shellCommand` of `true` on it, resumes it once
//! (`thread/resume`), holds it `--hold-ms`, and releases it — never a
//! `turn/start`, so no model turn is spent. The shell turn is there because
//! codex-cli 0.159.2 writes a thread's rollout only after a turn and refuses to
//! resume a thread without one; a shell turn makes no model call. After the first and the last
//! release — once the server no longer holds the threads released so far
//! (`thread/loaded/list`, polled for at most `--settle-ms`: codex-cli 0.159.2
//! unloads a released thread only 60 s after its last subscriber leaves) — it
//! reads the server's whole-tree resident memory (the pid in the
//! pool entry's `server.json` and every descendant, summed `VmRSS` from
//! `/proc`), and writes the readings to `--out` as the JSON
//! `live/control/app_server_rss_budget.py` reads. A released thread still
//! loaded at the bound fails the workload, naming it beside both readings.

use std::path::{Path, PathBuf};
use std::time::Duration;

use oneharness_core::io::app_server::{serve_sweeper_if_requested, PooledDispatch, PooledSession};
use serde_json::{json, Value};

struct Args {
    codex: String,
    out: PathBuf,
    leases: usize,
    hold: Duration,
    settle: Duration,
    cwd: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut codex = None;
    let mut out = None;
    let mut leases = 5;
    let mut hold = Duration::from_millis(1000);
    let mut settle = Duration::from_millis(90_000);
    let mut cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        let mut value = || raw.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--codex" => codex = Some(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--leases" => leases = value()?.parse().map_err(|e| format!("--leases: {e}"))?,
            "--hold-ms" => {
                hold =
                    Duration::from_millis(value()?.parse().map_err(|e| format!("--hold-ms: {e}"))?)
            }
            "--settle-ms" => {
                settle = Duration::from_millis(
                    value()?.parse().map_err(|e| format!("--settle-ms: {e}"))?,
                )
            }
            "--cwd" => cwd = PathBuf::from(value()?),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if leases == 0 {
        return Err("--leases must be at least 1".to_string());
    }
    Ok(Args {
        codex: codex.ok_or("--codex is required")?,
        out: out.ok_or("--out is required")?,
        leases,
        hold,
        settle,
        cwd: std::fs::canonicalize(&cwd).map_err(|e| format!("--cwd {}: {e}", cwd.display()))?,
    })
}

/// `pid` and every descendant of it, as the OS lists them now.
fn process_tree(root: u32) -> Vec<u32> {
    let parents: Vec<(u32, u32)> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let ppid = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            Some((pid, ppid))
        })
        .collect();
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        tree.extend(
            parents
                .iter()
                .filter(|(_, ppid)| *ppid == parent)
                .map(|(pid, _)| *pid),
        );
        index += 1;
    }
    tree.sort_unstable();
    tree
}

/// The resident memory `pid` holds, in bytes; 0 when it cannot be read.
fn rss(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("VmRSS:"))
                .and_then(|kb| kb.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        })
        .map_or(0, |kb| kb * 1024)
}

/// The whole-tree reading of the server `pid`.
fn reading(pid: u32) -> Value {
    let pids = process_tree(pid);
    let bytes: u64 = pids.iter().map(|pid| rss(*pid)).sum();
    json!({"rss_bytes": bytes, "pids": pids})
}

fn version(codex: &str) -> String {
    std::process::Command::new(codex)
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Wait until the server holds none of `released` loaded, polling through a
/// short-lived lease of its own (released again before the caller reads), for
/// at most `bound`. Answers the released threads still loaded at the bound.
fn settle(
    dispatch: PooledDispatch,
    released: &[String],
    bound: Duration,
) -> Result<Vec<String>, String> {
    let mut observer = PooledSession::open(dispatch)?;
    let until = std::time::Instant::now() + bound;
    let still = loop {
        let loaded = observer.loaded_threads()?;
        let still: Vec<String> = released
            .iter()
            .filter(|t| loaded.contains(t))
            .cloned()
            .collect();
        if still.is_empty() || std::time::Instant::now() >= until {
            break still;
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    observer.release();
    Ok(still)
}

fn run(args: &Args) -> Result<Value, String> {
    // The launch a default controlled dispatch would key its server by.
    let argv: Vec<String> = [
        args.codex.as_str(),
        "app-server",
        "-c",
        "features.plugins=false",
        "-c",
        "features.apps=false",
        "-c",
        "analytics.enabled=false",
    ]
    .iter()
    .map(|arg| (*arg).to_string())
    .collect();
    let mut leases = Vec::new();
    let mut after_first = Value::Null;
    let mut after_last = Value::Null;
    let mut released: Vec<String> = Vec::new();
    let mut retained: Vec<String> = Vec::new();
    for lease in 1..=args.leases {
        let dispatch = PooledDispatch::plan(&argv, &[], &[], &args.cwd, false)
            .ok_or("the launch is not a codex app-server launch")?;
        let mut session = PooledSession::open(dispatch)?;
        let pid = session.record().pid.get();
        let started = session.open_thread(&args.cwd, None)?;
        let thread = started
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
            .ok_or("thread/start named no thread")?
            .to_string();
        // codex-cli 0.159.2 writes a thread's rollout only after a turn and
        // refuses to resume a thread without one; a shell turn running `true`
        // makes no model call and gives the thread its rollout.
        session.shell_turn("true")?;
        session.open_thread(&args.cwd, Some(&thread))?;
        std::thread::sleep(args.hold);
        session.release();
        released.push(thread.clone());
        leases.push(json!({"lease": lease, "server_pid": pid, "thread": thread}));
        if lease == 1 || lease == args.leases {
            let observe = PooledDispatch::plan(&argv, &[], &[], &args.cwd, false)
                .ok_or("the launch is not a codex app-server launch")?;
            for still in settle(observe, &released, args.settle)? {
                if !retained.contains(&still) {
                    retained.push(still);
                }
            }
            let taken = reading(pid);
            if lease == 1 {
                after_first = taken.clone();
            }
            if lease == args.leases {
                after_last = taken;
            }
        }
        eprintln!(
            "app_server_reuse: lease {lease} of {} on server {pid} released",
            args.leases
        );
    }
    Ok(json!({
        "retained": retained,
        "codex_version": version(&args.codex),
        "platform": std::env::consts::OS,
        "lease_count": leases.len(),
        "leases": leases,
        "readings": {"after_first": after_first, "after_last": after_last},
    }))
}

fn write(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, format!("{value:#}\n")).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() {
    serve_sweeper_if_requested();
    let outcome = args().and_then(|args| {
        let value = run(&args)?;
        write(&args.out, &value)?;
        // Retention is its own failure, not a ratio that happens to read high:
        // a released thread the server still holds at the bound is named,
        // beside both readings.
        match value["retained"].as_array().filter(|threads| !threads.is_empty()) {
            Some(threads) => Err(format!(
                "released thread(s) {threads:?} still loaded {}s after their release; after the last of {} leases: {} B over pids {}; after the first: {} B over pids {}",
                args.settle.as_secs(),
                value["lease_count"],
                value["readings"]["after_last"]["rss_bytes"],
                value["readings"]["after_last"]["pids"],
                value["readings"]["after_first"]["rss_bytes"],
                value["readings"]["after_first"]["pids"],
            )),
            None => Ok(()),
        }
    });
    if let Err(error) = outcome {
        eprintln!("app_server_reuse: {error}");
        std::process::exit(1);
    }
}
