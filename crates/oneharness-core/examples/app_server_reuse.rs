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
//! `turn/start`, so no model turn is spent. One idle lease of its own (no
//! thread) is held from before the first lease to after the last, so the
//! server cannot linger out in a gap between two leases and every lease lands
//! on the one server the readings measure. The shell turn is there because
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

// llmlint: ignore-block[changed_behavior_has_e2e] A /proc that cannot be listed, a stat or status file this user cannot read, or a malformed VmRSS cannot be staged by an unprivileged test on a live kernel; each such path now fails the reading with the file and the OS error rather than undercounting, and the readable path — every descendant summed, an exited one skipped — is proven end to end by the_reuse_workload_runs_through_the_pool_and_reads_the_whole_server_tree, whose pids must equal the OS's own process tree.
/// `pid` and every descendant of it, as the OS lists them now. A process that
/// exits between the listing and its read is skipped; any other failure to
/// read `/proc` could hide a live descendant, so it fails the reading.
fn process_tree(root: u32) -> Result<Vec<u32>, String> {
    let listing = std::fs::read_dir("/proc").map_err(|e| format!("cannot list /proc: {e}"))?;
    let mut parents: Vec<(u32, u32)> = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|e| format!("cannot list /proc: {e}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot read /proc/{pid}/stat: {e}")),
        };
        let ppid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse().ok())
            .ok_or_else(|| format!("/proc/{pid}/stat names no parent: {stat:?}"))?;
        parents.push((pid, ppid));
    }
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
    Ok(tree)
}

/// The resident memory `pid` holds, in bytes: 0 once it has exited, or for a
/// zombie (which reports no `VmRSS` and holds none); any other failure would
/// undercount the server, so it fails the reading.
fn rss(pid: u32) -> Result<u64, String> {
    let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(status) => status,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("cannot read /proc/{pid}/status: {e}")),
    };
    let Some(kb) = status.lines().find_map(|line| line.strip_prefix("VmRSS:")) else {
        return Ok(0);
    };
    kb.trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .map(|kb| kb * 1024)
        .map_err(|e| format!("/proc/{pid}/status has an unreadable VmRSS {kb:?}: {e}"))
}

// llmlint: ignore-end[changed_behavior_has_e2e]

/// The whole-tree reading of the server `pid`.
fn reading(pid: u32) -> Result<Value, String> {
    let pids = process_tree(pid)?;
    let mut bytes = 0u64;
    for pid in &pids {
        bytes += rss(*pid)?;
    }
    Ok(json!({"rss_bytes": bytes, "pids": pids}))
}

fn version(codex: &str) -> Result<String, String> {
    let out = std::process::Command::new(codex)
        .arg("--version")
        .output()
        .map_err(|e| format!("cannot run `{codex} --version`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`{codex} --version` failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// How far past the settle bound a listing begun before it may run.
const LISTING_GRACE: Duration = Duration::from_secs(10);

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
        // The last poll starts at the bound at the latest, and its listing
        // must finish within LISTING_GRACE of it.
        let loaded = observer.loaded_threads(until + LISTING_GRACE)?;
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
    let keeper = PooledSession::open(
        PooledDispatch::plan(&argv, &[], &[], &args.cwd, false)
            .ok_or("the launch is not a codex app-server launch")?,
    )?;
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
            let taken = reading(pid)?;
            if lease == 1 {
                after_first = taken.clone();
            }
            if lease == args.leases {
                after_last = taken;
            }
        }
    }
    keeper.release();
    Ok(json!({
        "retained": retained,
        "codex_version": version(&args.codex)?,
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
