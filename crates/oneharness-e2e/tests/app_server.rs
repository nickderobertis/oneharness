//! Journeys of the shared Codex app-server: the real `oneharness` binary
//! driving `--control` Codex dispatches onto one pooled `codex app-server
//! --listen unix://`, played by the shipped mock harness's multi-client
//! app-server (`tests/support/mock_harness.rs`, `listen_server`), which
//! imitates real Codex as `spike-shared-app-server-report` observed it.
//!
//! Every journey here reads two independent accounts and holds them to each
//! other: what each run reports (`control.app_server`, its result) and what
//! the mock itself recorded (`CODEX_MOCK_DIR`: its own pid and environment,
//! every frame per connection, every turn's effective context, every command,
//! approval, interrupt and release).
//!
//! Unix only, like every controlled-run journey: `--control` is refused on
//! Windows, so no Windows dispatch reaches the shared server.
#![cfg(unix)]

use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use oneharness_core::io::marked_processes::{find_marked, ProcessMarker};
use oneharness_core::io::scratch::ScratchDir;
use oneharness_core::io::server_pool::{DEAD_LEASE_STOP_DEADLINE, DEAD_LEASE_SWEEP_INTERVAL};
use serde_json::{json, Value};

/// The shared server's linger every journey runs under (milliseconds).
const LINGER_MS: u64 = 1000;

/// A binary of another workspace crate, read from the profile directory this
/// test executable was built into, as `tests/cli.rs` reads it.
fn workspace_bin(name: &str) -> PathBuf {
    let test_exe = std::env::current_exe().expect("test executable path");
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("test executable under <target>/<profile>/deps");
    let path = profile_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is not built; run this suite through its Nx target (`just e2e`)",
        path.display()
    );
    path
}

fn oneharness_bin() -> PathBuf {
    workspace_bin("oneharness")
}

fn mock_bin() -> PathBuf {
    workspace_bin("oneharness-mock-harness")
}

/// Wait up to `limit` for `condition`, failing the journey with `label`.
fn wait_for(label: &str, limit: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out after {limit:?} waiting for {label}");
}

/// Whether `pid` names a live process (a zombie counts as gone).
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or_else(
        // Not Linux: ask the kernel; a reaped process answers ESRCH.
        // SAFETY: plain integers; signal 0 delivers nothing.
        |_| unsafe { libc::kill(pid as libc::pid_t, 0) == 0 },
        |stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .is_some_and(|state| state != "Z")
        },
    )
}

fn sigkill(pid: u32) {
    // SAFETY: plain integers; the pid is a process this journey started.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

/// One hermetic world for pooled dispatches: its own pool root
/// (`XDG_STATE_HOME`), session store, mock records and Codex home.
struct Pool {
    dir: ScratchDir,
    extra: Vec<(String, String)>,
}

impl Drop for Pool {
    fn drop(&mut self) {
        // Every server a dispatch here started stops on its own once its
        // linger passes; a journey that failed half way gets them stopped.
        let servers = self.dir.join("state").join("oneharness").join("servers");
        let deadline = Instant::now() + Duration::from_secs(15);
        let live = || {
            std::fs::read_dir(&servers)
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|e| e.path().join("server.json").is_file())
                })
                .unwrap_or(false)
        };
        while live() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        for record in self.mock_servers() {
            if let Some(pid) = record["pid"].as_u64() {
                if alive(pid as u32) {
                    sigkill(pid as u32);
                }
            }
        }
        for state in self.states() {
            for command in state["commands"].as_array().into_iter().flatten() {
                if let Some(pid) = command["pid"].as_u64() {
                    if alive(pid as u32) {
                        sigkill(pid as u32);
                    }
                }
            }
        }
    }
}

impl Pool {
    fn new(tag: &str) -> Self {
        // Under /tmp: the session store's control socket is an address, and
        // macOS's `$TMPDIR` spends most of `sun_path` before a name.
        let dir = ScratchDir::under(Path::new("/tmp"), tag).unwrap();
        for sub in ["state", "mock", "sess", "codex-home"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Pool {
            dir,
            extra: Vec::new(),
        }
    }

    /// The same world with `name=value` in every dispatch's environment.
    fn with_env(mut self, name: &str, value: &str) -> Self {
        self.extra.push((name.to_string(), value.to_string()));
        self
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// A file path inside the world, as a string.
    fn file(&self, relative: &str) -> String {
        self.path(relative).display().to_string()
    }

    /// A worktree of its own, created.
    fn worktree(&self, name: &str) -> PathBuf {
        let dir = self.path(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(dir).unwrap()
    }

    /// `oneharness run --harness <harness> --control` for session `session`,
    /// ready to spawn: no config file, this world's state, sessions and mock
    /// records, then `args`, then `envs` over everything.
    fn command(&self, session: &str, args: &[&str], envs: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(oneharness_bin());
        cmd.env("ONEHARNESS_NO_CONFIG", "1");
        self.base(&mut cmd);
        Self::completes_unless_held(&mut cmd, envs);
        cmd.args(self.run_args(session, args));
        for (key, value) in envs {
            cmd.env(key, value);
        }
        cmd
    }

    /// [`Self::command`] under the config file `toml` (written into the world)
    /// instead of none.
    fn command_with_config(
        &self,
        session: &str,
        toml: &str,
        args: &[&str],
        envs: &[(&str, &str)],
    ) -> Command {
        let config = self.path(&format!("{session}.toml"));
        std::fs::write(&config, toml).unwrap();
        let mut cmd = Command::new(oneharness_bin());
        cmd.env_remove("ONEHARNESS_NO_CONFIG");
        for (key, _) in std::env::vars() {
            if key.starts_with("ONEHARNESS_") {
                cmd.env_remove(key);
            }
        }
        self.base(&mut cmd);
        Self::completes_unless_held(&mut cmd, envs);
        let mut run = vec![
            "run".to_string(),
            "--config".to_string(),
            config.display().to_string(),
        ];
        run.extend(self.run_args(session, args).into_iter().skip(1));
        cmd.args(run);
        for (key, value) in envs {
            cmd.env(key, value);
        }
        cmd
    }

    /// A turn ends on its own unless the journey holds it
    /// (`MOCK_CODEX_HOLD_UNTIL`, a file it creates when it wants the turn over).
    fn completes_unless_held(cmd: &mut Command, envs: &[(&str, &str)]) {
        if !envs.iter().any(|(key, _)| *key == "MOCK_CODEX_HOLD_UNTIL") {
            cmd.env("MOCK_CODEX_COMPLETE_TURN", "1");
        }
    }

    fn base(&self, cmd: &mut Command) {
        cmd.env("XDG_STATE_HOME", self.path("state"))
            .env("LOCALAPPDATA", self.path("state"))
            .env("CODEX_HOME", self.path("codex-home"))
            .env("CODEX_MOCK_DIR", self.path("mock"))
            .env(
                "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
                LINGER_MS.to_string(),
            )
            .env("MOCK_CODEX_ECHO_PROMPT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.extra {
            cmd.env(key, value);
        }
    }

    fn run_args(&self, session: &str, args: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = [
            "run",
            "--control",
            "--session",
            session,
            "--session-dir",
            &self.file("sess"),
            "--format",
            "json",
            "--env",
            &format!(
                "LLVM_PROFILE_FILE={}",
                std::env::temp_dir()
                    .join("oneharness-killed-mock-%p.profraw")
                    .display()
            ),
        ]
        .iter()
        .map(|arg| (*arg).to_string())
        .collect();
        if !args.iter().any(|arg| *arg == "--harness") {
            out.extend(["--harness".to_string(), "codex".to_string()]);
        }
        if !args.iter().any(|arg| *arg == "--bin") {
            out.extend([
                "--bin".to_string(),
                format!("codex={}", mock_bin().display()),
            ]);
        }
        if !args.iter().any(|arg| *arg == "--prompt") {
            out.extend(["--prompt".to_string(), format!("work for {session}")]);
        }
        out.extend(args.iter().map(|arg| (*arg).to_string()));
        out
    }

    fn spawn(&self, mut cmd: Command) -> Child {
        cmd.spawn().expect("spawn oneharness")
    }

    /// Run `cmd` to its end and read its report.
    fn finish(child: Child) -> (Output, Value) {
        let output = child.wait_with_output().expect("oneharness finished");
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "no JSON report ({err}): {}\n--- stderr ---\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, report)
    }

    fn run(&self, cmd: Command) -> (Output, Value) {
        Self::finish(self.spawn(cmd))
    }

    /// The control socket a session listens on.
    fn socket(&self, session: &str) -> PathBuf {
        self.path("sess")
            .join("control")
            .join(format!("{session}.sock"))
    }

    /// Every server the mock started, as it recorded itself.
    fn mock_servers(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(self.path("mock"))
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        name.starts_with("server-") && name.ends_with(".json")
                    })
                    .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                    .filter_map(|text| serde_json::from_str(&text).ok())
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|record: &Value| record["pid"].as_u64());
        out
    }

    /// The pids of every server the mock started.
    fn server_pids(&self) -> Vec<u32> {
        self.mock_servers()
            .iter()
            .filter_map(|record| record["pid"].as_u64().map(|pid| pid as u32))
            .collect()
    }

    /// The one server the mock started.
    fn the_server(&self) -> u32 {
        let pids = self.server_pids();
        assert_eq!(pids.len(), 1, "exactly one server: {pids:?}");
        pids[0]
    }

    fn state(&self, pid: u32) -> Value {
        let path = self.path("mock").join(format!("state-{pid}.json"));
        serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("the mock wrote no state at {}: {err}", path.display())
            }),
        )
        .unwrap()
    }

    fn states(&self) -> Vec<Value> {
        self.server_pids()
            .into_iter()
            .filter_map(|pid| {
                std::fs::read_to_string(self.path("mock").join(format!("state-{pid}.json")))
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
            })
            .collect()
    }

    /// Every frame connection `conn` of server `pid` received, in order.
    fn received(&self, pid: u32, conn: u64) -> Vec<Value> {
        let path = self.path("mock").join(format!("conn-{pid}-{conn}.jsonl"));
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|line| line.get("in").cloned())
            .collect()
    }

    /// Every frame connection `conn` of server `pid` was sent, in order.
    fn sent(&self, pid: u32, conn: u64) -> Vec<Value> {
        let path = self.path("mock").join(format!("conn-{pid}-{conn}.jsonl"));
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|line| line.get("out").cloned())
            .collect()
    }

    /// How many turns server `pid` holds live right now.
    fn live_turns(&self, pid: u32) -> usize {
        std::fs::read_to_string(self.path("mock").join(format!("state-{pid}.json")))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map_or(0, |state| {
                state["loaded"].as_object().map_or(0, |threads| {
                    threads
                        .values()
                        .filter(|t| !t["active_turn"].is_null())
                        .count()
                })
            })
    }

    /// Wait until the mock has started exactly one server holding `n` live
    /// turns, and answer its pid.
    fn await_live_turns(&self, n: usize) -> u32 {
        wait_for(
            &format!("{n} live turns on one server"),
            Duration::from_secs(30),
            || self.server_pids().len() == 1 && self.live_turns(self.server_pids()[0]) == n,
        );
        self.the_server()
    }

    /// The turn record server `pid` kept for `thread`.
    fn turn_of(&self, pid: u32, thread: &str) -> Value {
        self.state(pid)["turns"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|turn| turn["thread"] == thread)
            .cloned()
            .unwrap_or_else(|| panic!("no turn on {thread}: {}", self.state(pid)))
    }

    /// The connection server `pid` served `thread` over.
    fn conn_of(&self, pid: u32, thread: &str) -> u64 {
        self.turn_of(pid, thread)["conn"].as_u64().unwrap()
    }
}

/// The shared server a run's report says its turn ran on.
fn reported_server(report: &Value) -> u32 {
    report["control"]["app_server"]["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("no shared server in the report: {}", report["control"]))
        as u32
}

fn result(report: &Value) -> &Value {
    &report["results"][0]
}

fn thread_of(report: &Value) -> String {
    result(report)["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no thread in {}", result(report)))
        .to_string()
}

fn succeeded(output: &Output, report: &Value) {
    assert!(
        output.status.success() && result(report)["status"] == "ok",
        "the run failed: {}\n--- stderr ---\n{}",
        result(report),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The command output a run's normalized tool events carry.
fn command_output(report: &Value) -> String {
    result(report)["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["kind"] == "tool_call")
        .filter_map(|event| event["output"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every frame a run's stdout carried, as JSON.
fn stdout_frames(report: &Value) -> Vec<Value> {
    result(report)["stdout"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Send one raw request line over a run's control socket and read the answer.
fn control_request(socket: &Path, line: &str) -> Value {
    let mut stream = UnixStream::connect(socket).expect("connect the control socket");
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    let mut reader = std::io::BufReader::new(stream);
    let mut answer = String::new();
    reader.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap_or_else(|err| panic!("{err}: {answer}"))
}

/// `oneharness interrupt` for `session` in `pool`, with how long it took.
fn interrupt(pool: &Pool, session: &str) -> (Value, Duration) {
    let started = Instant::now();
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "interrupt",
            "--session",
            session,
            "--session-dir",
            &pool.file("sess"),
            "--format",
            "json",
        ])
        .output()
        .expect("run oneharness interrupt");
    let answer = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "{err}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (answer, started.elapsed())
}

/// `oneharness interrupt --input <message>` against `session`: the answer frame
/// and whether the process exited 0.
fn redirect(pool: &Pool, session: &str, message: &str) -> (Value, bool) {
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "interrupt",
            "--session",
            session,
            "--session-dir",
            &pool.file("sess"),
            "--input",
            message,
            "--format",
            "json",
        ])
        .output()
        .expect("run oneharness interrupt");
    let answer = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "{err}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (answer, output.status.success())
}

/// Two concurrent dispatches with one pool key run their turns on ONE
/// app-server, which starts with the first lease and exits once the last has
/// ended and the linger has passed.
#[test]
fn two_concurrent_dispatches_with_one_key_share_one_server_for_the_life_of_their_leases() {
    let pool = Pool::new("pool-share");
    let release = pool.file("release");
    let started = Instant::now();
    let first = pool.spawn(pool.command("share-a", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
    let server = pool.await_live_turns(1);
    let record = &pool.mock_servers()[0];
    assert!(alive(server));
    let second = pool.spawn(pool.command("share-b", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
    pool.await_live_turns(2);
    std::fs::write(&release, b"").unwrap();
    let (out_a, report_a) = Pool::finish(first);
    let (out_b, report_b) = Pool::finish(second);
    succeeded(&out_a, &report_a);
    succeeded(&out_b, &report_b);
    let ended = Instant::now();

    // Both runs say they ran on the server the mock says it is.
    assert_eq!(reported_server(&report_a), server);
    assert_eq!(reported_server(&report_b), server);
    assert_eq!(
        report_a["control"]["app_server"]["pool_key"],
        report_b["control"]["app_server"]["pool_key"]
    );
    assert_eq!(
        pool.server_pids(),
        [server],
        "one server for both dispatches"
    );
    // Each ran its own thread, over its own connection.
    let (thread_a, thread_b) = (thread_of(&report_a), thread_of(&report_b));
    assert_ne!(thread_a, thread_b);
    assert_ne!(
        pool.conn_of(server, &thread_a),
        pool.conn_of(server, &thread_b)
    );
    // The result records the server's own launch: oneharness's argv, plus the
    // socket only its owner can enter.
    let argv: Vec<&str> = result(&report_a)["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(argv[1], "app-server");
    assert_eq!(argv[argv.len() - 2], "--listen");
    let socket = argv[argv.len() - 1].strip_prefix("unix://").unwrap();
    assert_eq!(record["argv"].as_array().unwrap().len(), argv.len() - 1);
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = Path::new(socket).parent().unwrap();
        assert_eq!(
            std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
            0o700,
            "the socket's directory is its owner's alone"
        );
    }

    // The last lease ended; the server outlives it by the linger, then stops.
    assert!(alive(server), "the server stopped before its linger passed");
    wait_for(
        "the server to stop after the linger",
        Duration::from_millis(LINGER_MS) + DEAD_LEASE_SWEEP_INTERVAL * 2 + Duration::from_secs(5),
        || !alive(server),
    );
    let lived = ended.elapsed();
    assert!(
        lived >= Duration::from_millis(LINGER_MS) - Duration::from_millis(200),
        "the server stopped {lived:?} after the last lease, inside its {LINGER_MS} ms linger"
    );
    assert!(started.elapsed() > lived);
}

/// A server stopped after its linger leaves no address behind: a successor's
/// first dial waits for the successor's own bind, even while the stopped
/// server's listener is still closing — staged by a launcher whose serving
/// child carries no marker and holds its listener open after the stop.
#[test]
fn a_successor_server_is_never_dialled_through_its_stopped_predecessor_s_socket() {
    let pool = Pool::new("pool-late")
        .with_env("CODEX_MOCK_SHIM", "1")
        .with_env("CODEX_MOCK_FAULT", "late-listener");
    let (out, report) = pool.run(pool.command("first", &[], &[]));
    succeeded(&out, &report);
    let first = reported_server(&report);
    wait_for(
        "the first server to stop after its linger",
        Duration::from_millis(LINGER_MS) + DEAD_LEASE_SWEEP_INTERVAL * 2 + Duration::from_secs(5),
        || !alive(first),
    );
    let (out, report) = pool.run(pool.command("second", &[], &[]));
    succeeded(&out, &report);
    assert_ne!(
        reported_server(&report),
        first,
        "the stopped server served again"
    );
}

/// Run two dispatches one after the other — the second while the first's
/// server is still lingering — and answer the servers each ran on.
fn servers_of(pool: &Pool, first: Command, second: Command) -> (u32, u32) {
    let (out_a, report_a) = pool.run(first);
    succeeded(&out_a, &report_a);
    let (out_b, report_b) = pool.run(second);
    succeeded(&out_b, &report_b);
    (reported_server(&report_a), reported_server(&report_b))
}

/// A launcher standing in for one Codex release at its own path: it answers
/// `--version` from `version_file` (re-read on every ask) and otherwise runs
/// the mock.
fn codex_release(pool: &Pool, name: &str, version_file: &Path) -> String {
    let path = pool.path(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo \"codex-cli $(cat '{}')\"; exit 0; fi\nexec '{}' \"$@\"\n",
            version_file.display(),
            mock_bin().display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

/// Dispatches differing in the executable's path, its reported version, a
/// variable Codex reads (`CODEX_HOME`), a process-level argument, or
/// `plugins` run on separate servers; dispatches differing only in a variable
/// outside the server's environment (an `env_from` such as `XDG_RUNTIME_DIR`)
/// share one.
#[test]
fn the_pool_key_separates_what_the_server_fixes_at_start_and_nothing_else() {
    let pool = Pool::new("pool-key");
    let version = pool.path("version");
    std::fs::write(&version, "1.0.0").unwrap();
    let release_a = format!("codex={}", codex_release(&pool, "codex-a", &version));
    let release_b = format!("codex={}", codex_release(&pool, "codex-b", &version));

    // The executable's path.
    let (a, b) = servers_of(
        &pool,
        pool.command("path-a", &["--bin", &release_a], &[]),
        pool.command("path-b", &["--bin", &release_b], &[]),
    );
    assert_ne!(a, b, "two executables shared a server");

    // Its reported version, at one path: a release upgraded in place.
    let (out, before) = pool.run(pool.command("ver-a", &["--bin", &release_a], &[]));
    succeeded(&out, &before);
    std::fs::write(&version, "1.1.0").unwrap();
    let (out, after) = pool.run(pool.command("ver-b", &["--bin", &release_a], &[]));
    succeeded(&out, &after);
    assert_ne!(
        reported_server(&before),
        reported_server(&after),
        "a new release reused the old release's server"
    );
    // The same release again: the server it already has.
    let (out, again) = pool.run(pool.command("ver-c", &["--bin", &release_a], &[]));
    succeeded(&out, &again);
    assert_eq!(reported_server(&again), reported_server(&after));

    // A variable Codex reads for itself.
    let other_home = pool.path("other-codex-home");
    std::fs::create_dir_all(&other_home).unwrap();
    let other_home = other_home.display().to_string();
    let (a, b) = servers_of(
        &pool,
        pool.command("home-a", &[], &[]),
        pool.command("home-b", &[], &[("CODEX_HOME", &other_home)]),
    );
    assert_ne!(a, b, "two Codex homes shared a server");

    // A process-level argument.
    let (a, b) = servers_of(
        &pool,
        pool.command_with_config("arg-a", "", &[], &[]),
        pool.command_with_config(
            "arg-b",
            "[harness.codex]\nargs = [\"-c\", \"model_reasoning_summary=detailed\"]\n",
            &[],
            &[],
        ),
    );
    assert_ne!(a, b, "two process-level argument sets shared a server");

    // The resolved `plugins`.
    let (a, b) = servers_of(
        &pool,
        pool.command_with_config("plug-a", "", &[], &[]),
        pool.command_with_config("plug-b", "[harness.codex]\nplugins = true\n", &[], &[]),
    );
    assert_ne!(a, b, "plugins on and off shared a server");

    // A variable outside the server's environment, each dispatch's own
    // through `env_from`: one server.
    let runtime = "[harness.codex.variant.a]\nenv_from = { XDG_RUNTIME_DIR = \"RUNTIME_A\" }\n\
                   [harness.codex.variant.b]\nenv_from = { XDG_RUNTIME_DIR = \"RUNTIME_B\" }\n";
    // Absolute paths name an identity's home, which must exist.
    let (run_a, run_b) = (pool.worktree("run-a"), pool.worktree("run-b"));
    let (run_a, run_b) = (run_a.display().to_string(), run_b.display().to_string());
    let envs = [("RUNTIME_A", run_a.as_str()), ("RUNTIME_B", run_b.as_str())];
    let (a, b) = servers_of(
        &pool,
        pool.command_with_config("rt-a", runtime, &["--harness", "codex:a"], &envs),
        pool.command_with_config("rt-b", runtime, &["--harness", "codex:b"], &envs),
    );
    assert_eq!(a, b, "two XDG_RUNTIME_DIR values started two servers");
}

/// The variables a shared server may be launched with.
fn server_may_hold(name: &str) -> bool {
    use oneharness_core::domain::config::{
        APP_SERVER_BASE_ENV_UNIX, APP_SERVER_BASE_ENV_UNIX_PREFIX, CODEX_SERVER_ENV,
        CODEX_SERVER_ENV_PREFIX,
    };
    APP_SERVER_BASE_ENV_UNIX.contains(&name)
        || name.starts_with(APP_SERVER_BASE_ENV_UNIX_PREFIX)
        || CODEX_SERVER_ENV.contains(&name)
        || name.starts_with(CODEX_SERVER_ENV_PREFIX)
        || name == "ONEHARNESS_APP_SERVER_MARKER"
}

/// Two concurrent pooled dispatches with different values of one variable
/// each see their own in a command the server runs for their thread; a
/// variable only the starting dispatch has, and one the second removes
/// through `unset_env`, are absent from the second's command; and the server's
/// own environment holds only the base variables, Codex's own and its marker.
#[test]
fn each_pooled_dispatch_s_commands_see_exactly_its_own_environment() {
    let pool = Pool::new("pool-env");
    let release = pool.file("release");
    let lean = "[harness.codex.variant.lean]\nunset_env = [\"IN_BOTH_UNLESS_UNSET\"]\n";
    let first = pool.spawn(pool.command(
        "env-a",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", "env"),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
            ("PER_DISPATCH", "alpha"),
            ("ONLY_IN_THE_FIRST", "first"),
            ("IN_BOTH_UNLESS_UNSET", "kept"),
        ],
    ));
    let server = pool.await_live_turns(1);
    let second = pool.spawn(pool.command_with_config(
        "env-b",
        lean,
        &["--harness", "codex:lean", "--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", "env"),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
            ("PER_DISPATCH", "beta"),
            ("IN_BOTH_UNLESS_UNSET", "kept"),
        ],
    ));
    pool.await_live_turns(2);
    std::fs::write(&release, b"").unwrap();
    let (out_a, report_a) = Pool::finish(first);
    let (out_b, report_b) = Pool::finish(second);
    succeeded(&out_a, &report_a);
    succeeded(&out_b, &report_b);
    assert_eq!(reported_server(&report_a), server);
    assert_eq!(
        reported_server(&report_b),
        server,
        "the two dispatches did not share"
    );

    let env_a = command_output(&report_a);
    let env_b = command_output(&report_b);
    let has = |env: &str, pair: &str| env.lines().any(|line| line == pair);
    let names = |env: &str, name: &str| {
        env.lines()
            .any(|line| line.starts_with(&format!("{name}=")))
    };
    assert!(
        has(&env_a, "PER_DISPATCH=alpha") && !has(&env_a, "PER_DISPATCH=beta"),
        "{env_a}"
    );
    assert!(
        has(&env_b, "PER_DISPATCH=beta") && !has(&env_b, "PER_DISPATCH=alpha"),
        "{env_b}"
    );
    assert!(has(&env_a, "ONLY_IN_THE_FIRST=first"), "{env_a}");
    assert!(
        !names(&env_b, "ONLY_IN_THE_FIRST"),
        "the first dispatch's variable reached the second: {env_b}"
    );
    assert!(has(&env_a, "IN_BOTH_UNLESS_UNSET=kept"), "{env_a}");
    assert!(
        !names(&env_b, "IN_BOTH_UNLESS_UNSET"),
        "a variable the second unset reached it: {env_b}"
    );
    // Each command carries its own dispatch's marker, never the server's.
    let thread_a = thread_of(&report_a);
    let thread_b = thread_of(&report_b);
    let marker_a = pool.turn_of(server, &thread_a)["marker"]
        .as_str()
        .unwrap()
        .to_string();
    let marker_b = pool.turn_of(server, &thread_b)["marker"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(marker_a, marker_b);
    assert!(
        has(&env_a, &format!("ONEHARNESS_DISPATCH_MARKER={marker_a}")),
        "{env_a}"
    );
    assert!(
        has(&env_b, &format!("ONEHARNESS_DISPATCH_MARKER={marker_b}")),
        "{env_b}"
    );
    assert!(!names(&env_a, "ONEHARNESS_APP_SERVER_MARKER"), "{env_a}");

    // The server's own environment: the base variables, Codex's, its marker.
    let server_env = pool.mock_servers()[0]["env"].as_object().unwrap().clone();
    let stray: Vec<&String> = server_env
        .keys()
        .filter(|name| !server_may_hold(name))
        .collect();
    assert!(stray.is_empty(), "the server was launched with {stray:?}");
    assert!(server_env.contains_key("ONEHARNESS_APP_SERVER_MARKER"));
    for dispatch_only in [
        "PER_DISPATCH",
        "ONLY_IN_THE_FIRST",
        "MOCK_CODEX_RUN_COMMAND",
    ] {
        assert!(
            !server_env.contains_key(dispatch_only),
            "{dispatch_only} reached the server"
        );
    }
}

/// A command appending a line to `file` every 100 ms, forever.
fn heartbeat(file: &str) -> String {
    format!("while :; do echo beat >> '{file}'; sleep 0.1; done")
}

fn lines(path: &str) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

/// The command server `pid` ran whose text mentions `needle`.
fn command_running(pool: &Pool, pid: u32, needle: &str) -> Value {
    pool.state(pid)["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["command"]
                .as_str()
                .is_some_and(|text| text.contains(needle))
        })
        .cloned()
        .unwrap_or_else(|| panic!("no command mentioning {needle}: {}", pool.state(pid)))
}

/// Whether server `pid` still holds anything for `thread`: loaded, a
/// subscription, or per-thread state.
fn holds_thread(pool: &Pool, pid: u32, thread: &str) -> bool {
    !pool.state(pid)["loaded"][thread].is_null()
}

/// SIGKILL a pooled dispatch whose thread is running a heartbeat command (one
/// the server keeps running after `turn/interrupt`, as real Codex does), and
/// show the pool stopping it within the sweep interval plus the stop deadline:
/// the turn ended as interrupted, the command's process gone, no further
/// heartbeat, and the thread released — while `neighbour` (if any) keeps its
/// thread and finishes. Then the server stops once no lease is live.
fn a_killed_dispatch_is_stopped_and_released(pool: &Pool, with_neighbour: bool) {
    let beat = pool.file("heartbeat");
    let never = pool.file("never-released");
    let release = pool.file("release");
    let mut victim = pool.spawn(pool.command(
        "victim",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    let server = pool.await_live_turns(1);
    wait_for("the heartbeat to start", Duration::from_secs(10), || {
        lines(&beat) >= 3
    });
    let neighbour = with_neighbour.then(|| {
        let child =
            pool.spawn(pool.command("neighbour", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
        pool.await_live_turns(2);
        child
    });
    let command = command_running(pool, server, &beat);
    let command_pid = command["pid"].as_u64().unwrap() as u32;
    let thread = command["thread"].as_str().unwrap().to_string();
    assert!(alive(command_pid));

    sigkill(victim.id());
    victim.wait().unwrap();
    let killed = Instant::now();
    let bound = DEAD_LEASE_SWEEP_INTERVAL + DEAD_LEASE_STOP_DEADLINE;
    // A hang guard, well past the bound the assertion below holds it to.
    wait_for("the dead dispatch's command to stop", bound * 3, || {
        !alive(command_pid)
    });
    let stopped = killed.elapsed();
    assert!(
        stopped <= bound + Duration::from_millis(1500),
        "the command outlived its dispatch by {stopped:?}, past the {bound:?} sweep + stop bound"
    );
    assert_eq!(
        pool.turn_of(server, &thread)["status"],
        "interrupted",
        "the dead dispatch's turn was not interrupted"
    );
    let beats = lines(&beat);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        lines(&beat),
        beats,
        "the heartbeat kept beating after the stop"
    );
    wait_for(
        "the dead dispatch's thread to be released",
        Duration::from_secs(10),
        || !holds_thread(pool, server, &thread),
    );
    assert!(
        pool.state(server)["releases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["thread"] == thread.as_str() && r["status"] == "unsubscribed"),
        "{}",
        pool.state(server)
    );

    if let Some(neighbour) = neighbour {
        let live: Vec<String> = pool.state(server)["loaded"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            live.len(),
            1,
            "the neighbour's thread must stay loaded: {live:?}"
        );
        std::fs::write(&release, b"").unwrap();
        let (output, report) = Pool::finish(neighbour);
        succeeded(&output, &report);
        assert_eq!(reported_server(&report), server);
        assert_eq!(thread_of(&report), live[0]);
    }
    wait_for(
        "the server to stop once no lease is live",
        Duration::from_millis(LINGER_MS) + bound + Duration::from_secs(5),
        || !alive(server),
    );
}

#[test]
fn a_sigkilled_pooled_dispatch_is_stopped_and_released_beside_a_live_neighbour() {
    a_killed_dispatch_is_stopped_and_released(&Pool::new("pool-kill"), true);
}

#[test]
fn a_sigkilled_pooled_dispatch_is_stopped_and_released_when_it_was_the_only_one() {
    a_killed_dispatch_is_stopped_and_released(&Pool::new("pool-kill-alone"), false);
}

/// A dispatch that ends normally while a command it started still runs leaves
/// no process carrying its marker once its run returns; its thread is
/// released, and a later dispatch resuming that thread runs its turn on it.
#[test]
fn a_finished_dispatch_leaves_no_marked_process_and_its_released_thread_resumes() {
    let pool = Pool::new("pool-end");
    let beat = pool.file("background");
    let (output, report) = pool.run(pool.command(
        "ended",
        &["--mode", "bypass"],
        &[(
            "MOCK_CODEX_RUN_COMMAND",
            &format!("({}) >/dev/null 2>&1 & echo started", heartbeat(&beat)),
        )],
    ));
    succeeded(&output, &report);
    let server = reported_server(&report);
    let thread = thread_of(&report);
    let marker = pool.turn_of(server, &thread)["marker"]
        .as_str()
        .unwrap()
        .to_string();
    let marked = find_marked(&ProcessMarker {
        name: "ONEHARNESS_DISPATCH_MARKER".to_string(),
        value: marker,
    })
    .unwrap();
    assert!(
        marked.is_empty(),
        "the run returned with {marked:?} still carrying its marker"
    );
    assert!(
        !holds_thread(&pool, server, &thread),
        "{}",
        pool.state(server)
    );

    // The same session continues that thread: reloaded, and its turn run.
    let (output, resumed) = pool.run(pool.command("ended", &[], &[]));
    succeeded(&output, &resumed);
    assert_eq!(thread_of(&resumed), thread);
    let resumed_on = reported_server(&resumed);
    let opened: Vec<Value> = pool.state(resumed_on)["turns"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["thread"] == thread.as_str())
        .cloned()
        .collect();
    assert!(
        !opened.is_empty(),
        "the resumed thread ran no turn: {}",
        pool.state(resumed_on)
    );
}

/// The lease files every pool entry in `pool` still holds.
fn lease_files(pool: &Pool) -> Vec<PathBuf> {
    std::fs::read_dir(pool.path("state/oneharness/servers"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            std::fs::read_dir(entry.path().join("leases"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|lease| lease.path())
        .collect()
}

/// A release the server refuses, or leaves unanswered past its reply deadline
/// (5 s), is said on stderr naming the thread and why, and bounded: the
/// dispatch still succeeds, returns within the deadline, and leaves no lease —
/// while a refused thread stays loaded on the server, as the warning says.
#[test]
fn a_refused_or_unanswered_release_is_said_bounded_and_leaves_no_lease() {
    let pool = Pool::new("pool-release-fail");
    for (session, knob, value, said) in [
        (
            "refused",
            "MOCK_CODEX_UNSUBSCRIBE_REFUSE",
            "1",
            "it refused thread/unsubscribe: ",
        ),
        (
            "silent",
            "MOCK_CODEX_UNSUBSCRIBE_DELAY_MS",
            "30000",
            "no answer to thread/unsubscribe within 5s",
        ),
    ] {
        let started = Instant::now();
        let (output, report) = pool.run(pool.command(session, &[], &[(knob, value)]));
        let took = started.elapsed();
        succeeded(&output, &report);
        let thread = thread_of(&report);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!(
                "the shared codex app-server did not release thread {thread}: {said}"
            )),
            "{session}: {stderr}"
        );
        assert!(
            took < Duration::from_secs(5 + 10),
            "{session}: the dispatch took {took:?} past the 5 s release deadline"
        );
        assert_eq!(lease_files(&pool), Vec::<PathBuf>::new(), "{session}");
        if session == "refused" {
            assert!(
                holds_thread(&pool, reported_server(&report), &thread),
                "the refused release unloaded the thread anyway"
            );
        }
    }
}

/// Codex's Node launcher runs the native server as its child, and a SIGKILLed
/// launcher leaves that child serving. Stopping such a server — at its linger,
/// or once the pool finds its launcher dead — leaves neither process behind.
#[test]
fn a_server_behind_a_launcher_shim_is_stopped_whole() {
    for kill_the_launcher in [false, true] {
        let pool = Pool::new(if kill_the_launcher {
            "pool-shim-kill"
        } else {
            "pool-shim"
        })
        .with_env("CODEX_MOCK_SHIM", "1");
        let release = pool.file("release");
        let child = pool.spawn(pool.command("shim", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
        let native = pool.await_live_turns(1);
        let entries = pool.path("state/oneharness/servers");
        let record: Value = std::fs::read_dir(&entries)
            .unwrap()
            .flatten()
            .find_map(|e| std::fs::read_to_string(e.path().join("server.json")).ok())
            .map(|text| serde_json::from_str(&text).unwrap())
            .unwrap();
        let launcher = record["pid"].as_u64().unwrap() as u32;
        assert_ne!(
            launcher, native,
            "the shim did not run the server as its child"
        );
        std::fs::write(&release, b"").unwrap();
        let (output, report) = Pool::finish(child);
        succeeded(&output, &report);
        assert_eq!(reported_server(&report), launcher);
        if kill_the_launcher {
            sigkill(launcher);
            std::thread::sleep(Duration::from_millis(200));
            assert!(alive(native), "the native server died with its launcher");
        }
        wait_for(
            "both the launcher and the native server to stop",
            Duration::from_millis(LINGER_MS)
                + DEAD_LEASE_SWEEP_INTERVAL * 2
                + Duration::from_secs(12),
            || !alive(launcher) && !alive(native),
        );
    }
}

const INTERRUPT_LINE: &str = r#"{"v":2,"verb":"interrupt"}"#;

/// The interrupts server `pid` received, as `(connection, thread)`.
fn interrupts_received(pool: &Pool, pid: u32) -> Vec<(u64, String)> {
    pool.state(pid)["interrupts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["conn"].as_u64().unwrap(),
                i["thread"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// The thread a live session's dispatch opened, read off the server: the
/// thread of the one live turn served over a connection no other session holds.
fn live_thread(pool: &Pool, pid: u32, besides: &[String]) -> String {
    pool.state(pid)["loaded"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(id, t)| !t["active_turn"].is_null() && !besides.contains(id))
        .map(|(id, _)| id.clone())
        .expect("a live turn")
}

/// With two pooled turns live, `oneharness interrupt` against one dispatch,
/// and an interrupt through another's `--control` socket, each deliver
/// `turn/interrupt` for that dispatch's thread over its own connection while
/// the other turn runs on to completion.
#[test]
fn an_interrupt_reaches_only_its_own_dispatch_s_turn_over_its_own_connection() {
    for through_the_socket in [false, true] {
        let pool = Pool::new(if through_the_socket {
            "pool-int-raw"
        } else {
            "pool-int-cli"
        });
        let never = pool.file("never");
        let release = pool.file("release");
        let target = pool.spawn(pool.command("target", &[], &[("MOCK_CODEX_HOLD_UNTIL", &never)]));
        let server = pool.await_live_turns(1);
        let target_thread = live_thread(&pool, server, &[]);
        let other = pool.spawn(pool.command("other", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
        pool.await_live_turns(2);
        let other_thread = live_thread(&pool, server, std::slice::from_ref(&target_thread));

        let answer = if through_the_socket {
            control_request(&pool.socket("target"), INTERRUPT_LINE)
        } else {
            interrupt(&pool, "target").0
        };
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(answer["mechanism"], "codex-app-server", "{answer}");
        let (output, report) = Pool::finish(target);
        assert!(output.status.success(), "{report}");
        assert_eq!(report["control"]["interrupts"][0]["outcome"], "served");
        assert_eq!(
            interrupts_received(&pool, server),
            [(pool.conn_of(server, &target_thread), target_thread.clone())],
            "the interrupt must address only its own dispatch's thread, over its own connection"
        );
        assert_eq!(
            pool.turn_of(server, &target_thread)["status"],
            "interrupted"
        );
        // The other turn is still running, and finishes on its own.
        assert!(!pool.state(server)["loaded"][&other_thread]["active_turn"].is_null());
        std::fs::write(&release, b"").unwrap();
        let (output, report) = Pool::finish(other);
        succeeded(&output, &report);
        assert_eq!(pool.turn_of(server, &other_thread)["status"], "completed");
    }
}

/// After its turn ended — the dispatch still alive, releasing its thread — an
/// interrupt answers `no_active_turn` from oneharness's own record and sends
/// nothing to a server that would never answer one; once the dispatch is gone
/// it is `not_running`; a server that says `thread not found` is `not_running`
/// too; a frame this build cannot parse is `unsupported`, exactly as for a
/// dispatch's own child; and an interrupt the server never answers returns
/// within `INTERRUPT_REPLY_DEADLINE`.
#[test]
fn a_pooled_interrupt_answers_from_its_own_record_and_never_waits_unbounded() {
    use oneharness_core::io::server_pool::INTERRUPT_REPLY_DEADLINE;
    let pool = Pool::new("pool-int-edges");

    // Ended, still releasing: nothing is sent.
    let ending = pool.spawn(pool.command(
        "ending",
        &[],
        &[
            ("MOCK_CODEX_UNSUBSCRIBE_DELAY_MS", "4000"),
            ("MOCK_CODEX_INTERRUPT_SILENT", "1"),
        ],
    ));
    wait_for("the turn to end", Duration::from_secs(20), || {
        pool.server_pids().len() == 1
            && pool.state(pool.server_pids()[0])["turns"]
                .as_array()
                .is_some_and(|turns| turns.iter().any(|t| t["status"] == "completed"))
    });
    let server = pool.the_server();
    let started = Instant::now();
    let by_cli = interrupt(&pool, "ending").0;
    let by_socket = control_request(&pool.socket("ending"), INTERRUPT_LINE);
    for answer in [&by_cli, &by_socket] {
        assert_eq!(answer["reason"], "no_active_turn", "{answer}");
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "an answer waited on the server"
    );
    let (output, report) = Pool::finish(ending);
    succeeded(&output, &report);
    assert!(
        interrupts_received(&pool, server).is_empty(),
        "an interrupt reached the server"
    );

    // Gone.
    let gone = interrupt(&pool, "ending").0;
    assert_eq!(gone["reason"], "not_running", "{gone}");

    // The server no longer has the thread.
    let never = pool.file("never");
    let lost = pool.spawn(pool.command(
        "lost-thread",
        &[],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &never),
            ("MOCK_CODEX_INTERRUPT_THREAD_NOT_FOUND", "1"),
        ],
    ));
    wait_for("the turn to start", Duration::from_secs(20), || {
        pool.server_pids()
            .iter()
            .any(|pid| pool.live_turns(*pid) == 1)
    });
    let not_found = interrupt(&pool, "lost-thread").0;
    assert_eq!(not_found["reason"], "not_running", "{not_found}");
    assert!(
        not_found["error"]
            .as_str()
            .unwrap()
            .contains("thread not found"),
        "{not_found}"
    );

    // A frame this build cannot parse.
    let unparsable = control_request(
        &pool.socket("lost-thread"),
        r#"{"v":99,"verb":"interrupt"}"#,
    );
    assert_eq!(unparsable["reason"], "unsupported", "{unparsable}");
    sigkill(lost.id());
    let _ = lost.wait_with_output();

    // The same frame against a dispatch's own child.
    let log = pool.file("own-child.log");
    let own = pool.spawn(pool.command_with_config(
        "own-child",
        "[harness.codex]\napp_server = \"per-dispatch\"\n",
        &[],
        &[
            ("MOCK_CODEX_APP_SERVER_LOG", &log),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    wait_for("the own child's turn", Duration::from_secs(20), || {
        std::fs::read_to_string(&log).is_ok_and(|text| text.contains("turn/start"))
    });
    let own_unparsable =
        control_request(&pool.socket("own-child"), r#"{"v":99,"verb":"interrupt"}"#);
    assert_eq!(own_unparsable["reason"], unparsable["reason"]);
    assert_eq!(own_unparsable["error"], unparsable["error"]);
    let stopped = interrupt(&pool, "own-child").0;
    assert_eq!(stopped["ok"], true, "{stopped}");
    let _ = own.wait_with_output();

    // A server that never answers the interrupt. The killed dispatch's turn
    // is retired first, so the live turn waited for below is this one's.
    wait_for(
        "the killed dispatch's turn to be retired",
        Duration::from_secs(20),
        || {
            pool.server_pids()
                .iter()
                .all(|pid| !alive(*pid) || pool.live_turns(*pid) == 0)
        },
    );
    let silent = pool.spawn(pool.command(
        "silent",
        &[],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &never),
            ("MOCK_CODEX_INTERRUPT_SILENT", "1"),
        ],
    ));
    wait_for("the silent turn", Duration::from_secs(20), || {
        pool.socket("silent").exists()
            && pool
                .server_pids()
                .iter()
                .any(|pid| alive(*pid) && pool.live_turns(*pid) >= 1)
    });
    let (answer, took) = interrupt(&pool, "silent");
    assert_eq!(answer["reason"], "not_running", "{answer}");
    assert!(
        took >= INTERRUPT_REPLY_DEADLINE - Duration::from_millis(500)
            && took <= INTERRUPT_REPLY_DEADLINE + Duration::from_secs(2),
        "the unanswered interrupt returned after {took:?}, not at the {INTERRUPT_REPLY_DEADLINE:?} deadline"
    );
    sigkill(silent.id());
    let _ = silent.wait_with_output();
}

/// Spawn a dispatch of the fallback chain `config` names — its own
/// candidates, so the default `--harness` is dropped — under `envs`.
fn chain_dispatch(
    pool: &Pool,
    session: &str,
    config: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Child {
    let cmd = pool.command_with_config(session, config, args, envs);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut fresh = Command::new(cmd.get_program());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => fresh.env(key, value),
            None => fresh.env_remove(key),
        };
    }
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == "--harness" {
            skip = true;
            continue;
        }
        fresh.arg(arg);
    }
    fresh
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    pool.spawn(fresh)
}

/// Killing the shared server under two live turns makes both runs report
/// `app_server_lost` — retryable, so a fallback chain moves to its next
/// candidate (here a variant on its own app-server child) — never a turn
/// failure; the history record carries the kind at its schema version.
#[test]
fn a_shared_server_killed_under_two_live_turns_is_app_server_lost_and_falls_through() {
    let pool = Pool::new("pool-lost");
    let never = pool.file("never");
    let alt_log = pool.file("alt.log");
    let config = format!(
        "harnesses = [\"codex\", \"codex:alt\"]\n\
         [harness.codex.variant.alt]\n\
         app_server = \"per-dispatch\"\n\
         env = {{ MOCK_CODEX_APP_SERVER_LOG = \"{alt_log}\", MOCK_CODEX_COMPLETE_TURN = \"1\" }}\n"
    );
    let history = pool.file("history");
    let spawn = |session: &str| {
        chain_dispatch(
            &pool,
            session,
            &config,
            &["--history", "--history-dir", &history],
            &[("MOCK_CODEX_HOLD_UNTIL", &never)],
        )
    };
    let first = spawn("lost-a");
    let server = pool.await_live_turns(1);
    let second = spawn("lost-b");
    pool.await_live_turns(2);
    sigkill(server);
    for child in [first, second] {
        let (output, report) = Pool::finish(child);
        assert!(output.status.success(), "{report}");
        let lost = &report["results"][0];
        assert_eq!(lost["harness_id"], "codex", "{report}");
        assert_eq!(lost["status"], "nonzero", "{lost}");
        assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
        assert_eq!(
            report["fallback"]["fell_through"][0]["reason"], "app-server-lost",
            "{report}"
        );
        assert_eq!(report["fallback"]["ran"], "codex:alt", "{report}");
        let ran = report["results"].as_array().unwrap().last().unwrap();
        assert_eq!(ran["status"], "ok", "{ran}");
        let file = report["history_file"].as_str().expect("a history file");
        let record = std::fs::read_to_string(file)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|line| line["type"] == "run" && line["failure_kind"] == "app_server_lost")
            .unwrap_or_else(|| panic!("no app_server_lost record in {file}"));
        assert_eq!(record["schema_version"], "1.10", "{record}");
    }
}

/// An interrupt carrying a message on a pooled turn stops the turn and makes
/// the message the thread's next turn, in one operation the answer vouches for
/// (`redirected`); when the shared server refuses the interrupt, the run takes
/// nothing — the answer is `not_running`, no second turn opens, and the turn
/// it could not stop finishes as it would have.
#[test]
fn a_pooled_interrupt_with_a_message_redirects_the_turn_or_takes_nothing() {
    let pool = Pool::new("pool-redirect");
    for (session, refuse) in [("steered", false), ("unsteered", true)] {
        let release = pool.file(&format!("{session}-release"));
        let mut envs = vec![("MOCK_CODEX_HOLD_UNTIL", release.as_str())];
        if refuse {
            envs.push(("MOCK_CODEX_INTERRUPT_THREAD_NOT_FOUND", "1"));
        }
        let child = pool.spawn(pool.command(session, &[], &envs));
        wait_for("the turn to start", Duration::from_secs(30), || {
            pool.server_pids()
                .iter()
                .any(|pid| pool.live_turns(*pid) == 1)
        });
        let message = format!("stop and do the right thing for {session}");
        let (answer, served) = redirect(&pool, session, &message);
        std::fs::write(&release, "").unwrap();
        let (output, report) = Pool::finish(child);
        succeeded(&output, &report);
        let server = reported_server(&report);
        let thread = thread_of(&report);
        let turns: Vec<Value> = pool.state(server)["turns"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["thread"] == thread.as_str())
            .cloned()
            .collect();
        let interrupts = report["control"]["interrupts"].as_array().unwrap();
        if refuse {
            assert!(!served, "{answer}");
            assert_eq!(answer["reason"], "not_running", "{answer}");
            assert!(
                answer["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("thread not found")),
                "{answer}"
            );
            assert_eq!(
                turns.len(),
                1,
                "a refused redirect opened a turn: {turns:?}"
            );
            assert_ne!(interrupts[0]["redirected"], true, "{report}");
        } else {
            assert!(served, "{answer}");
            assert_eq!(answer["redirected"], true, "{answer}");
            assert_eq!(interrupts[0]["outcome"], "served", "{report}");
            assert_eq!(interrupts[0]["redirected"], true, "{report}");
            assert_eq!(turns.len(), 2, "{turns:?}");
            assert_eq!(pool.turn_of(server, &thread)["turn"], turns[1]["turn"]);
            assert!(
                turns[1]["input"].to_string().contains(&message),
                "the replacement turn did not carry the message: {}",
                turns[1]
            );
        }
    }
}

/// A zero-work overload on a pooled turn repeats the turn on the shared
/// server — a fresh lease, the same server — and recovers within
/// `server_overloaded_max_retries`; past it the run is `server_overloaded`,
/// and a chain falls through to its next candidate.
#[test]
fn a_pooled_overload_retries_on_the_shared_server_and_falls_through_once_exhausted() {
    let pool = Pool::new("pool-overload");
    let recovered_count = pool.file("recovered-attempts");
    let (output, report) = pool.run(pool.command_with_config(
        "recovered",
        "server_overloaded_max_retries = 1\n",
        &[],
        &[
            ("MOCK_CODEX_OVERLOAD_ATTEMPTS", "1"),
            ("MOCK_ATTEMPT_FILE", &recovered_count),
        ],
    ));
    succeeded(&output, &report);
    let ran = result(&report);
    assert!(ran["failure_kind"].is_null(), "{ran}");
    assert_eq!(ran["text"], "work for recovered", "{ran}");
    assert_eq!(std::fs::read_to_string(&recovered_count).unwrap(), "2");
    let server = reported_server(&report);
    let statuses: Vec<Value> = pool.state(server)["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].clone())
        .collect();
    assert_eq!(
        statuses,
        vec![json!("failed"), json!("completed")],
        "both attempts ran on the one shared server"
    );

    let exhausted_count = pool.file("exhausted-attempts");
    let alt_log = pool.file("alt.log");
    let config = format!(
        "harnesses = [\"codex\", \"codex:alt\"]\n\
         server_overloaded_max_retries = 1\n\
         [harness.codex.variant.alt]\n\
         app_server = \"per-dispatch\"\n\
         env = {{ MOCK_CODEX_APP_SERVER_LOG = \"{alt_log}\", MOCK_CODEX_COMPLETE_TURN = \"1\" }}\n"
    );
    let (_, report) = Pool::finish(chain_dispatch(
        &pool,
        "exhausted",
        &config,
        &[],
        &[
            ("MOCK_CODEX_OVERLOAD_ATTEMPTS", "2"),
            ("MOCK_ATTEMPT_FILE", &exhausted_count),
        ],
    ));
    let results = report["results"].as_array().unwrap();
    assert_eq!(results[0]["harness_id"], "codex", "{report}");
    assert_eq!(results[0]["failure_kind"], "server_overloaded", "{report}");
    assert_eq!(report["fallback"]["ran"], "codex:alt", "{report}");
    assert_eq!(results.last().unwrap()["status"], "ok", "{report}");
}

/// A pooled turn's frames are observed as they arrive, as a dispatch's own
/// child's stdout is, so a tool the turn runs reads the same on a shared server
/// as on the dispatch's own app-server child — in the report and in the
/// history's event lines: the same tool call, with the same timing fields.
#[test]
fn a_pooled_turn_s_tool_reads_as_it_does_on_the_dispatch_s_own_child() {
    let pool = Pool::new("pool-timing");
    let timing = |event: &Value| -> Vec<Value> {
        [
            "name",
            "tool_call_id",
            "status",
            "started_at",
            "finished_at",
            "duration_ms",
            "timing_source",
        ]
        .iter()
        .map(|field| event[*field].clone())
        .collect()
    };
    let mut seen = Vec::new();
    for per_dispatch in [false, true] {
        let session = if per_dispatch { "own-child" } else { "pooled" };
        let history = pool.file(&format!("{session}-history"));
        let args = ["--events", "--history", "--history-dir", history.as_str()];
        let log = pool.file(&format!("{session}-app-server.log"));
        let envs = [
            ("MOCK_CODEX_TOOL_EVENTS", "1"),
            // The own child acts as an app-server only with a log to keep.
            ("MOCK_CODEX_APP_SERVER_LOG", log.as_str()),
        ];
        let cmd = if per_dispatch {
            pool.command_with_config(
                session,
                "[harness.codex]\napp_server = \"per-dispatch\"\n",
                &args,
                &envs,
            )
        } else {
            pool.command(session, &args, &envs)
        };
        let (output, report) = pool.run(cmd);
        succeeded(&output, &report);
        assert_eq!(
            report["control"]["app_server"].is_null(),
            per_dispatch,
            "{session}: {report}"
        );
        let call = result(&report)["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "tool_call")
            .cloned()
            .unwrap_or_else(|| panic!("{session}: no tool call in {report}"));
        let file = report["history_file"].as_str().expect("a history file");
        let written = std::fs::read_to_string(file).unwrap();
        let in_history = written
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|line| line["type"] == "event" && line["event"]["kind"] == "tool_call")
            .unwrap_or_else(|| panic!("{session}: no tool call event in {written}"));
        assert_eq!(
            timing(&in_history["event"]),
            timing(&call),
            "{session}: {written}"
        );
        seen.push(timing(&call));
    }
    assert_eq!(
        seen[0], seen[1],
        "the shared server and the own child disagree"
    );
}

/// A shared server that accepts the connection and never answers the
/// WebSocket upgrade fails the dispatch within the handshake's deadline,
/// naming it, rather than holding the run past its own bounds.
#[test]
fn a_server_that_never_answers_the_upgrade_fails_the_dispatch_at_the_handshake_deadline() {
    use oneharness_core::io::websocket::HANDSHAKE_DEADLINE;
    let pool = Pool::new("pool-stall").with_env("CODEX_MOCK_FAULT", "stall-upgrade");
    let started = Instant::now();
    let (_, report) = pool.run(pool.command("stalled", &[], &[]));
    let took = started.elapsed();
    let ran = result(&report);
    assert_ne!(ran["status"], "ok", "{ran}");
    assert!(
        ran["error"]
            .as_str()
            .is_some_and(|e| e.contains("did not answer the WebSocket upgrade within 10s")),
        "{ran}"
    );
    assert!(
        took >= HANDSHAKE_DEADLINE && took < HANDSHAKE_DEADLINE + Duration::from_secs(15),
        "the dispatch returned after {took:?}, not at the {HANDSHAKE_DEADLINE:?} handshake deadline"
    );
}

/// A shared server lost after its turn already ran a tool is still
/// `app_server_lost`, but the chain stops there: the tool event the run saw is
/// kept in its report and the next candidate never runs, since re-running the
/// task elsewhere would repeat work that happened.
#[test]
fn a_shared_server_lost_after_its_turn_ran_a_tool_keeps_the_work_and_stops_the_chain() {
    let pool = Pool::new("pool-lost-work");
    let never = pool.file("never");
    let alt_log = pool.file("alt.log");
    let config = format!(
        "harnesses = [\"codex\", \"codex:alt\"]\n\
         [harness.codex.variant.alt]\n\
         app_server = \"per-dispatch\"\n\
         env = {{ MOCK_CODEX_APP_SERVER_LOG = \"{alt_log}\", MOCK_CODEX_COMPLETE_TURN = \"1\" }}\n"
    );
    let child = chain_dispatch(
        &pool,
        "lost-work",
        &config,
        &["--events"],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &never),
            ("MOCK_CODEX_TOOL_EVENTS", "1"),
        ],
    );
    let server = pool.await_live_turns(1);
    sigkill(server);
    let (_, report) = Pool::finish(child);
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "the chain moved on after work: {report}");
    let lost = &results[0];
    assert_eq!(lost["harness_id"], "codex", "{lost}");
    assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
    assert!(
        lost["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|e| e["kind"] == "tool_call")),
        "the tool event the turn ran was not kept: {lost}"
    );
    assert!(
        report["fallback"]["fell_through"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{report}"
    );
    assert!(
        !Path::new(&alt_log).exists(),
        "the next candidate ran after the lost turn's work"
    );
}

/// One dispatch's thread-level settings.
struct Settings<'a> {
    session: &'a str,
    cwd: &'a Path,
    model: &'a str,
    mode: &'a str,
    sandbox: &'a str,
    prompt: &'a str,
    system: &'a str,
}

impl Settings<'_> {
    fn args(&self) -> Vec<String> {
        [
            "--cwd",
            &self.cwd.display().to_string(),
            "--model",
            self.model,
            "--mode",
            self.mode,
            "--prompt",
            self.prompt,
            "--system",
            self.system,
        ]
        .iter()
        .map(|a| (*a).to_string())
        .collect()
    }
}

fn spawn_with(pool: &Pool, settings: &Settings, envs: &[(&str, &str)]) -> Child {
    let args = settings.args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    pool.spawn(pool.command(settings.session, &args, envs))
}

/// The first frame of `method` connection `conn` of server `pid` received.
fn first_received(pool: &Pool, pid: u32, conn: u64, method: &str) -> Value {
    pool.received(pid, conn)
        .into_iter()
        .find(|frame| frame["method"] == method)
        .unwrap_or_else(|| panic!("connection {conn} sent no {method}"))
}

/// Every frame of `method` connection `conn` of server `pid` received.
fn all_received(pool: &Pool, pid: u32, conn: u64, method: &str) -> Vec<Value> {
    pool.received(pid, conn)
        .into_iter()
        .filter(|frame| frame["method"] == method)
        .collect()
}

/// Hold a run's output to its own thread: every frame names its thread or no
/// thread at all, and its answer is its own prompt's echo.
fn output_is_its_own(report: &Value, prompt: &str, other_prompt: &str) {
    let thread = thread_of(report);
    for frame in stdout_frames(report) {
        let named = frame["params"]["threadId"]
            .as_str()
            .or_else(|| frame["params"]["thread"]["id"].as_str());
        assert!(
            named.is_none() || named == Some(thread.as_str()),
            "a frame of {named:?} reached the run of {thread}: {frame}"
        );
    }
    let text = result(report)["text"].as_str().unwrap_or_default();
    assert!(
        text.contains(prompt) && !text.contains(other_prompt),
        "{text}"
    );
}

/// Two concurrent dispatches on one server, both turns live, with different
/// values for every thread-level setting: each sends `thread/start` and every
/// `turn/start` on its own connection with its own values; each command runs
/// in its own worktree under its own sandbox; a server-initiated approval
/// request for one thread arrives only on, and is answered only over, the
/// connection of the dispatch that owns it; and each run's output holds only
/// its own agent text and items.
#[test]
fn every_thread_level_setting_travels_on_its_own_dispatch_s_connection() {
    let pool = Pool::new("pool-thread");
    let (wt_a, wt_b) = (pool.worktree("wt-a"), pool.worktree("wt-b"));
    let release = pool.file("release");
    let a = Settings {
        session: "thread-a",
        cwd: &wt_a,
        model: "model-alpha",
        mode: "bypass",
        sandbox: "dangerFullAccess",
        prompt: "task alpha",
        system: "instruction alpha",
    };
    let b = Settings {
        session: "thread-b",
        cwd: &wt_b,
        model: "model-beta",
        mode: "default",
        sandbox: "readOnly",
        prompt: "task beta",
        system: "instruction beta",
    };
    let first = spawn_with(
        &pool,
        &a,
        &[
            ("MOCK_CODEX_RUN_COMMAND", "touch landed-alpha && pwd"),
            ("MOCK_CODEX_REQUEST_APPROVAL", "1"),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
        ],
    );
    let server = pool.await_live_turns(1);
    let second = spawn_with(
        &pool,
        &b,
        &[
            ("MOCK_CODEX_RUN_COMMAND", "touch landed-beta && pwd"),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
        ],
    );
    pool.await_live_turns(2);
    std::fs::write(&release, b"").unwrap();
    let (out_a, report_a) = Pool::finish(first);
    let (out_b, report_b) = Pool::finish(second);
    succeeded(&out_a, &report_a);
    succeeded(&out_b, &report_b);
    assert_eq!(reported_server(&report_a), server);
    assert_eq!(reported_server(&report_b), server);

    for (settings, report, other) in [(&a, &report_a, &b), (&b, &report_b, &a)] {
        let thread = thread_of(report);
        let conn = pool.conn_of(server, &thread);
        let cwd = settings.cwd.display().to_string();
        let start = first_received(&pool, server, conn, "thread/start");
        assert_eq!(start["params"]["cwd"], cwd.as_str(), "{start}");
        assert_eq!(start["params"]["model"], settings.model, "{start}");
        assert_eq!(start["params"]["approvalPolicy"], "never", "{start}");
        let turns = all_received(&pool, server, conn, "turn/start");
        assert!(!turns.is_empty());
        for turn in &turns {
            assert_eq!(turn["params"]["threadId"], thread.as_str(), "{turn}");
            assert_eq!(turn["params"]["cwd"], cwd.as_str(), "{turn}");
            assert_eq!(turn["params"]["model"], settings.model, "{turn}");
            assert_eq!(turn["params"]["approvalPolicy"], "never", "{turn}");
            assert_eq!(
                turn["params"]["sandboxPolicy"]["type"], settings.sandbox,
                "{turn}"
            );
        }
        // The turn's effective context, as the server ran it.
        let effective = pool.turn_of(server, &thread);
        assert_eq!(effective["cwd"], cwd.as_str());
        assert_eq!(effective["model"], settings.model);
        assert_eq!(effective["sandbox"], settings.sandbox);
        let input = effective["input"].as_str().unwrap();
        assert!(
            input.contains(settings.prompt) && input.contains(settings.system),
            "{input}"
        );
        assert!(
            !input.contains(other.prompt) && !input.contains(other.system),
            "{input}"
        );
        output_is_its_own(report, settings.prompt, other.prompt);
    }

    // Each command ran in its own worktree, under its own sandbox.
    assert!(
        wt_a.join("landed-alpha").exists(),
        "the full-access write did not land"
    );
    assert!(
        !wt_b.join("landed-alpha").exists(),
        "a command ran in the other worktree"
    );
    assert!(!wt_b.join("landed-beta").exists() && !wt_a.join("landed-beta").exists());
    let thread_b = thread_of(&report_b);
    let refused = pool.state(server)["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["thread"] == thread_b.as_str())
        .cloned()
        .unwrap();
    assert_eq!(refused["refused"], "read-only sandbox", "{refused}");
    assert_eq!(refused["cwd"], wt_b.display().to_string().as_str());

    // The approval request: on its owner's connection only, answered by it alone.
    let thread_a = thread_of(&report_a);
    let (conn_a, conn_b) = (
        pool.conn_of(server, &thread_a),
        pool.conn_of(server, &thread_b),
    );
    let approvals = pool.state(server)["approvals"].as_array().unwrap().clone();
    assert_eq!(approvals.len(), 1, "{approvals:?}");
    assert_eq!(approvals[0]["thread"], thread_a.as_str());
    let answers = approvals[0]["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["conn"], conn_a);
    assert_eq!(answers[0]["decision"], "approved");
    assert!(
        !pool
            .sent(server, conn_b)
            .iter()
            .any(|frame| frame["method"] == "item/commandExecution/requestApproval"),
        "the other dispatch was asked to approve"
    );
}

/// Each dispatch resumes its own earlier thread — each created under the
/// OTHER dispatch's settings, so any value left to the thread would be wrong —
/// and each resumed thread and its turn run under the resuming dispatch's
/// directory, model, sandbox, environment and marker: the turn's own values in
/// force over the ones its thread was started with.
#[test]
fn a_resumed_thread_runs_under_the_resuming_dispatch_s_settings() {
    let pool = Pool::new("pool-resume");
    let (wt_a, wt_b) = (pool.worktree("wt-a"), pool.worktree("wt-b"));
    // Each session works through a path of its own — a `current` link, as a
    // deployment repoints one — which names the store its handle lives in.
    // Pointed at the other worktree while the thread is created and at its
    // own when the session continues, so the thread starts in one directory
    // and its resumed turn runs in another, through the public interface.
    let (link_a, link_b) = (pool.path("current-a"), pool.path("current-b"));
    let point = |link: &Path, at: &Path| {
        let _ = std::fs::remove_file(link);
        std::os::unix::fs::symlink(at, link).unwrap();
    };
    point(&link_a, &wt_b);
    point(&link_b, &wt_a);
    let a = |session| Settings {
        session,
        cwd: if session == "res-a" { &link_a } else { &link_b },
        model: "model-alpha",
        mode: "bypass",
        sandbox: "dangerFullAccess",
        prompt: "task alpha",
        system: "instruction alpha",
    };
    let b = |session| Settings {
        session,
        cwd: if session == "res-a" { &link_a } else { &link_b },
        model: "model-beta",
        mode: "auto",
        sandbox: "workspaceWrite",
        prompt: "task beta",
        system: "instruction beta",
    };
    // Each thread is created under the other dispatch's settings.
    let command = "env; pwd";
    let (out, made_a) = Pool::finish(spawn_with(
        &pool,
        &b("res-a"),
        &[
            ("PER_DISPATCH", "creator-of-a"),
            ("MOCK_CODEX_RUN_COMMAND", command),
        ],
    ));
    succeeded(&out, &made_a);
    let (out, made_b) = Pool::finish(spawn_with(
        &pool,
        &a("res-b"),
        &[
            ("PER_DISPATCH", "creator-of-b"),
            ("MOCK_CODEX_RUN_COMMAND", command),
        ],
    ));
    succeeded(&out, &made_b);
    let created_on = reported_server(&made_a);
    let created_marker = |report: &Value| {
        pool.turn_of(created_on, &thread_of(report))["marker"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (creator_marker_a, creator_marker_b) = (created_marker(&made_a), created_marker(&made_b));
    assert_eq!(
        pool.turn_of(created_on, &thread_of(&made_a))["cwd"],
        wt_b.display().to_string().as_str(),
        "res-a's thread was not created in the other dispatch's worktree"
    );
    point(&link_a, &wt_a);
    point(&link_b, &wt_b);

    let release = pool.file("release");
    let first = spawn_with(
        &pool,
        &a("res-a"),
        &[
            ("PER_DISPATCH", "alpha"),
            ("MOCK_CODEX_RUN_COMMAND", command),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
        ],
    );
    let server = pool.await_live_turns(1);
    let second = spawn_with(
        &pool,
        &b("res-b"),
        &[
            ("PER_DISPATCH", "beta"),
            ("MOCK_CODEX_RUN_COMMAND", command),
            ("MOCK_CODEX_HOLD_UNTIL", &release),
        ],
    );
    pool.await_live_turns(2);
    std::fs::write(&release, b"").unwrap();
    let (out_a, report_a) = Pool::finish(first);
    let (out_b, report_b) = Pool::finish(second);
    succeeded(&out_a, &report_a);
    succeeded(&out_b, &report_b);

    for (settings, report, made, value, creator_marker) in [
        (a("res-a"), &report_a, &made_a, "alpha", &creator_marker_a),
        (b("res-b"), &report_b, &made_b, "beta", &creator_marker_b),
    ] {
        let thread = thread_of(report);
        assert_eq!(
            thread,
            thread_of(made),
            "the session did not continue its thread"
        );
        let conn = pool.conn_of(server, &thread);
        let resume = first_received(&pool, server, conn, "thread/resume");
        assert_eq!(resume["params"]["threadId"], thread.as_str());
        assert_eq!(
            resume["params"]["config"]["shell_environment_policy"]["set"]["PER_DISPATCH"],
            value
        );
        let effective = pool.turn_of(server, &thread);
        let cwd = std::fs::canonicalize(settings.cwd)
            .unwrap()
            .display()
            .to_string();
        assert_eq!(effective["cwd"], cwd.as_str(), "{effective}");
        assert_eq!(effective["model"], settings.model, "{effective}");
        assert_eq!(effective["sandbox"], settings.sandbox, "{effective}");
        let marker = effective["marker"].as_str().unwrap();
        assert_ne!(
            marker,
            creator_marker.as_str(),
            "the resumed thread kept its creator's marker"
        );
        let out = command_output(report);
        assert!(
            out.lines().any(|l| l == format!("PER_DISPATCH={value}")),
            "{out}"
        );
        assert!(
            out.lines()
                .any(|l| l == format!("ONEHARNESS_DISPATCH_MARKER={marker}")),
            "{out}"
        );
        assert!(
            out.lines().any(|l| l == cwd),
            "the command did not run in {cwd}: {out}"
        );
    }
}

/// With the server broadcasting every thread's frames to every connection —
/// status changes, turn starts and ends, deltas and approval requests — each
/// dispatch prints only its own thread's events and text; the other's
/// `turn/completed` ends neither turn; the other's `turn/started` does not
/// make an interrupt against an idle dispatch send anything; an interrupt
/// against a live dispatch still reaches its own turn after the other's
/// completed; and neither answers the other's approval request.
#[test]
fn a_broadcasting_server_s_other_thread_frames_are_never_a_dispatch_s_own() {
    let pool = Pool::new("pool-broadcast").with_env("CODEX_MOCK_BROADCAST", "1");
    let never = pool.file("never");
    let release = pool.file("release");
    let opened = pool.file("open-idle");
    let live = pool.spawn(pool.command(
        "live",
        &["--mode", "bypass", "--prompt", "live prompt"],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &never),
            ("MOCK_CODEX_REQUEST_APPROVAL", "1"),
            ("MOCK_CODEX_RUN_COMMAND", "echo live-ran"),
        ],
    ));
    let server = pool.await_live_turns(1);
    let live_thread = live_thread(&pool, server, &[]);
    // A dispatch held idle before its turn, connected through everything below.
    let idle = pool.spawn(pool.command(
        "idle",
        &["--prompt", "idle prompt"],
        &[("MOCK_CODEX_HOLD_OPEN_UNTIL", &opened)],
    ));
    wait_for(
        "the idle dispatch to connect",
        Duration::from_secs(20),
        || {
            pool.socket("idle").exists()
                && pool.state(server)["connections"].as_array().unwrap().len() == 2
        },
    );
    let finishing = pool.spawn(pool.command(
        "finishing",
        &["--prompt", "finishing prompt"],
        &[("MOCK_CODEX_HOLD_UNTIL", &release)],
    ));
    pool.await_live_turns(2);
    // The other dispatch's `turn/started` has reached the idle one.
    let idle_answer = interrupt(&pool, "idle").0;
    assert_eq!(idle_answer["reason"], "no_active_turn", "{idle_answer}");

    std::fs::write(&release, b"").unwrap();
    let (out, finished) = Pool::finish(finishing);
    succeeded(&out, &finished);
    output_is_its_own(&finished, "finishing prompt", "live prompt");
    // Its `turn/completed` went everywhere and ended nothing else.
    std::thread::sleep(Duration::from_millis(300));
    assert!(!pool.state(server)["loaded"][&live_thread]["active_turn"].is_null());

    let live_answer = interrupt(&pool, "live").0;
    assert_eq!(live_answer["ok"], true, "{live_answer}");
    let (out, live_report) = Pool::finish(live);
    assert!(out.status.success(), "{live_report}");
    output_is_its_own(&live_report, "live prompt", "finishing prompt");
    let live_conn = pool.conn_of(server, &live_thread);
    assert_eq!(
        interrupts_received(&pool, server),
        [(live_conn, live_thread.clone())]
    );

    std::fs::write(&opened, b"").unwrap();
    let (out, idle_report) = Pool::finish(idle);
    succeeded(&out, &idle_report);
    output_is_its_own(&idle_report, "idle prompt", "live prompt");

    // The approval request went to every connection; only its owner answered.
    let approvals = pool.state(server)["approvals"].as_array().unwrap().clone();
    assert_eq!(approvals.len(), 1, "{approvals:?}");
    let answers = approvals[0]["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 1, "another dispatch answered: {answers:?}");
    assert_eq!(answers[0]["conn"], live_conn);
}

/// Every launch the mock codex recorded about its model catalog, in order.
fn catalog_records(pool: &Pool) -> Vec<Value> {
    std::fs::read_to_string(pool.path("catalog.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn is_refresh(record: &Value) -> bool {
    record["argv"]
        .as_array()
        .is_some_and(|argv| argv.iter().any(|a| a == "--stdio"))
}

fn server_launches(pool: &Pool) -> Vec<Value> {
    catalog_records(pool)
        .into_iter()
        .filter(|r| {
            r["argv"]
                .as_array()
                .is_some_and(|argv| argv.iter().any(|a| a == "--listen"))
        })
        .collect()
}

/// A world whose mock codex reports a release and keeps a catalog log, both
/// under the `CODEX_MOCK_*` spellings a scrubbed server environment keeps.
fn catalog_pool(tag: &str) -> Pool {
    let pool = Pool::new(tag).with_env("CODEX_MOCK_VERSION", "1.2.3");
    let log = pool.file("catalog.jsonl");
    pool.with_env("CODEX_MOCK_CATALOG_LOG", &log)
}

/// Pooling keeps the startup-fetch settings: a catalog missing when the first
/// dispatch arrives is refreshed exactly once, before the server starts, and
/// the server starts with `-c analytics.enabled=false` and `-c
/// model_catalog_json=<that file>` right after its plugins and apps settings;
/// it fetches no catalog over its whole life across both dispatches' turns.
#[test]
fn a_pooled_server_starts_with_analytics_off_and_the_shared_catalog_and_fetches_none() {
    let pool = catalog_pool("pool-catalog");
    let release = pool.file("release");
    let first = pool.spawn(pool.command("cat-a", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
    let server = pool.await_live_turns(1);
    let second = pool.spawn(pool.command("cat-b", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
    pool.await_live_turns(2);
    std::fs::write(&release, b"").unwrap();
    for child in [first, second] {
        let (output, report) = Pool::finish(child);
        succeeded(&output, &report);
        assert_eq!(reported_server(&report), server);
    }
    let records = catalog_records(&pool);
    let refreshes: Vec<&Value> = records.iter().filter(|r| is_refresh(r)).collect();
    assert_eq!(
        refreshes.len(),
        1,
        "one refresh for the missing catalog: {records:?}"
    );
    let launches = server_launches(&pool);
    assert_eq!(launches.len(), 1, "{launches:?}");
    let launch = &launches[0];
    assert!(
        launch["started_ms"].as_u64() > refreshes[0]["started_ms"].as_u64(),
        "the server started before its catalog was refreshed"
    );
    let argv: Vec<&str> = launch["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        argv[..8],
        [
            "app-server",
            "-c",
            "features.plugins=false",
            "-c",
            "features.apps=false",
            "-c",
            "analytics.enabled=false",
            "-c"
        ],
        "{argv:?}"
    );
    assert!(argv[8].starts_with("model_catalog_json="), "{argv:?}");
    let catalog_dir = pool.path("state/oneharness/codex-model-catalog");
    let catalog = std::fs::read_dir(&catalog_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().is_some_and(|e| e == "json")
                && !p.to_string_lossy().ends_with(".failure.json")
        })
        .unwrap();
    assert_eq!(
        std::fs::canonicalize(launch["catalog"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&catalog).unwrap()
    );
    let fetches = records
        .iter()
        .filter(|r| !is_refresh(r) && !r["fetch"].is_null())
        .count();
    assert_eq!(fetches, 0, "a server launch fetched a catalog: {records:?}");
    wait_for("the server to stop", Duration::from_secs(10), || {
        !alive(server)
    });
    assert_eq!(
        catalog_records(&pool)
            .iter()
            .filter(|r| !is_refresh(r) && !r["fetch"].is_null())
            .count(),
        0,
        "the server fetched a catalog over its life"
    );
}

/// A catalog refreshed while a server lives leaves that server's argv as it
/// was — the dispatch that refreshed it joins the running server — and the
/// next server the pool starts reads the refreshed file.
#[test]
fn a_catalog_refreshed_under_a_live_server_reaches_the_next_server_not_the_running_one() {
    let pool = catalog_pool("pool-catalog-stale");
    let config = "[harness.codex]\nmodel_catalog_refresh_seconds = 1\n";
    let release = pool.file("release");
    let first = pool.spawn(pool.command_with_config(
        "stale-a",
        config,
        &[],
        &[("MOCK_CODEX_HOLD_UNTIL", &release)],
    ));
    let server = pool.await_live_turns(1);
    let catalog = server_launches(&pool)[0]["catalog"]
        .as_str()
        .unwrap()
        .to_string();
    let written = std::fs::metadata(&catalog).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(1300));
    let (output, joined) = pool.run(pool.command_with_config("stale-b", config, &[], &[]));
    succeeded(&output, &joined);
    assert_eq!(
        reported_server(&joined),
        server,
        "the refreshing dispatch did not join the live server"
    );
    assert!(
        std::fs::metadata(&catalog).unwrap().modified().unwrap() > written,
        "the stale catalog was not refreshed"
    );
    assert_eq!(
        server_launches(&pool).len(),
        1,
        "the running server was restarted"
    );
    std::fs::write(&release, b"").unwrap();
    let (output, report) = Pool::finish(first);
    succeeded(&output, &report);
    wait_for("the server to stop", Duration::from_secs(10), || {
        !alive(server)
    });
    let refreshed = std::fs::metadata(&catalog).unwrap().modified().unwrap();
    let (output, next) = pool.run(pool.command_with_config("stale-c", config, &[], &[]));
    succeeded(&output, &next);
    assert_ne!(reported_server(&next), server);
    let launches = server_launches(&pool);
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(launches[1]["catalog"], catalog.as_str());
    assert_eq!(
        launches[0]["argv"], launches[1]["argv"],
        "the catalog path moved"
    );
    let read_at = launches[1]["started_ms"].as_u64().unwrap();
    let refreshed_ms = refreshed
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(
        read_at >= refreshed_ms,
        "the next server started before the refreshed file"
    );
}

/// A dispatch resolving `shared_model_catalog = false` or `analytics = true`
/// runs on a server of its own, whose argv lacks that flag.
#[test]
fn a_dispatch_without_a_startup_fetch_setting_gets_a_server_of_its_own() {
    for (config, missing) in [
        (
            "[harness.codex]\nshared_model_catalog = false\n",
            "model_catalog_json=",
        ),
        (
            "[harness.codex]\nanalytics = true\n",
            "analytics.enabled=false",
        ),
    ] {
        let pool = catalog_pool("pool-catalog-own");
        let release = pool.file("release");
        let defaults =
            pool.spawn(pool.command("own-a", &[], &[("MOCK_CODEX_HOLD_UNTIL", &release)]));
        let shared = pool.await_live_turns(1);
        let (output, own) = pool.run(pool.command_with_config("own-b", config, &[], &[]));
        succeeded(&output, &own);
        assert_ne!(reported_server(&own), shared, "{config}");
        let argv = result(&own)["command"].as_array().unwrap().clone();
        assert!(
            !argv
                .iter()
                .any(|a| a.as_str().is_some_and(|a| a.starts_with(missing))),
            "{config}: {argv:?}"
        );
        std::fs::write(&release, b"").unwrap();
        let (output, report) = Pool::finish(defaults);
        succeeded(&output, &report);
    }
}

/// The repository root, for the Linux-only journeys that run its scripts.
#[cfg(target_os = "linux")]
fn repo_root() -> PathBuf {
    std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap()
}

/// `pid` and every descendant of it, as the OS lists them now (`/proc`).
#[cfg(target_os = "linux")]
fn os_process_tree(root: u32) -> Vec<u64> {
    let parents: Vec<(u64, u64)> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u64>().ok())
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
    let mut tree = vec![u64::from(root)];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        tree.extend(
            parents
                .iter()
                .filter(|(_, p)| *p == parent)
                .map(|(pid, _)| *pid),
        );
        index += 1;
    }
    tree.sort_unstable();
    tree
}

/// scripts/measure-app-server-sharing.sh's figures measure what was exercised:
/// each thread's resident memory against the server's own reading of itself,
/// and each time to a ready thread against the delay the mock was told to
/// answer that thread's `thread/start` after.
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_reports_what_the_server_it_ran_actually_did() {
    let pool = Pool::new("pool-measure");
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
        .env("OH_MEASURE_CODEX", mock_bin())
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_THREAD_BYTES", (64 * 1024 * 1024).to_string())
        .env("CODEX_MOCK_OPEN_DELAYS_MS", "400,900")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let measured: Value = serde_json::from_slice(&output.stdout).unwrap();
    let server = measured["server_pid"].as_u64().unwrap() as u32;
    let own = pool.state(server)["open_rss"].as_array().unwrap().clone();
    let threads = measured["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 2);
    assert_eq!(own.len(), 2, "{own:?}");
    for (index, (thread, delay)) in threads.iter().zip([400u64, 900]).enumerate() {
        let measured_rss = thread["rss_bytes"].as_f64().unwrap();
        let server_rss = own[index].as_f64().unwrap();
        assert!(
            (measured_rss - server_rss).abs() <= server_rss * 0.05,
            "thread {}: measured {measured_rss} B, the server read {server_rss} B of itself",
            index + 1
        );
        assert_eq!(thread["pids"], serde_json::json!([server]));
        let ready = thread["ready_ms"].as_u64().unwrap();
        assert!(
            ready >= delay && ready < delay + 3000,
            "thread {}: ready after {ready} ms against a {delay} ms answer",
            index + 1
        );
    }
    let grown =
        threads[1]["rss_bytes"].as_u64().unwrap() - threads[0]["rss_bytes"].as_u64().unwrap();
    assert!(
        grown >= 60 * 1024 * 1024,
        "the second thread's 64 MiB did not show: {grown} B"
    );
}

/// A server that exits before it listens fails the sharing measurement naming
/// that, its exit and its stderr, with the next step — and the measurement's
/// scratch directory is gone all the same.
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_names_a_server_that_never_listened_and_leaves_no_scratch() {
    let pool = Pool::new("pool-measure-fail");
    let tmp = pool.path("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
        .env("OH_MEASURE_CODEX", mock_bin())
        .env("TMPDIR", &tmp)
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_EXIT_FIRST", pool.path("exited"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{said}");
    assert!(output.stdout.is_empty(), "a figure was printed: {said}");
    assert!(
        said.contains("measure-app-server-sharing: the server never accepted a connection on ")
            && said.contains(
                "(last attempt: [Errno 2] No such file or directory; exit 1); its stderr:"
            )
            && said.contains("check `codex login status`"),
        "{said}"
    );
    let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().flatten().collect();
    assert!(left.is_empty(), "scratch left behind: {left:?}");
}

/// live/control/app-server-reuse.sh runs its workload through the pool: each
/// of the leases ran `initialize`, `thread/start`, a `thread/shellCommand` and
/// `thread/resume` of its own thread and no `turn/start`, all on one server;
/// and each reading's pids are exactly the server's process tree — launcher,
/// server and grandchild — as the OS lists it. A 1 ms linger cannot split the
/// leases across servers: the workload's own idle lease holds the server from
/// the first lease to the last.
#[cfg(target_os = "linux")]
#[test]
fn the_reuse_workload_runs_through_the_pool_and_reads_the_whole_server_tree() {
    let pool = Pool::new("pool-reuse");
    let out = pool.path("reuse.json");
    let output = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "2000")
        .env("OH_E2E_REUSE_FILE", &out)
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_SHIM", "1")
        .env("CODEX_MOCK_HELPER", "1")
        .env("ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reuse: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(reuse["lease_count"], 5);
    let leases = reuse["leases"].as_array().unwrap();
    let launcher = leases[0]["server_pid"].as_u64().unwrap() as u32;
    assert!(
        leases.iter().all(|l| l["server_pid"] == launcher),
        "{leases:?}"
    );
    // The server is still lingering: its tree, as the OS lists it, is what
    // both readings summed.
    let tree = os_process_tree(launcher);
    assert_eq!(tree.len(), 3, "launcher, server and its helper: {tree:?}");
    for name in ["after_first", "after_last"] {
        let pids: Vec<u64> = reuse["readings"][name]["pids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_u64().unwrap())
            .collect();
        assert_eq!(pids, tree, "{name} did not sum the server's whole tree");
        assert!(reuse["readings"][name]["rss_bytes"].as_u64().unwrap() > 0);
    }
    let native = pool.the_server();
    assert!(tree.contains(&u64::from(native)));
    for lease in leases {
        let thread = lease["thread"].as_str().unwrap();
        let conn = pool.state(native)["turns"].as_array().unwrap().is_empty();
        assert!(conn, "a lease ran a model turn");
        let opened = (1..=10u64)
            .find(|conn| {
                pool.sent(native, *conn).iter().any(|frame| {
                    frame["result"]["thread"]["id"] == thread && frame["id"].is_number()
                })
            })
            .unwrap_or_else(|| panic!("no connection opened {thread}"));
        let methods: Vec<String> = pool
            .received(native, opened)
            .iter()
            .filter_map(|frame| frame["method"].as_str().map(str::to_string))
            .collect();
        for method in [
            "initialize",
            "thread/start",
            "thread/shellCommand",
            "thread/resume",
        ] {
            assert!(
                methods.iter().any(|m| m == method),
                "lease {lease}: no {method} in {methods:?}"
            );
        }
        assert!(
            !methods.iter().any(|m| m == "turn/start"),
            "lease {lease} ran turn/start"
        );
        let resumed = pool
            .received(native, opened)
            .into_iter()
            .find(|frame| frame["method"] == "thread/resume")
            .unwrap();
        assert_eq!(resumed["params"]["threadId"], thread);
    }
}

/// A server whose `thread/loaded/list` pages never end — each naming a cursor
/// already answered — fails the reuse workload, naming the repeated cursor,
/// rather than paging forever (the settle bound is checked between polls, not
/// within one).
#[cfg(target_os = "linux")]
#[test]
fn the_reuse_workload_refuses_a_loaded_list_that_repeats_its_cursor() {
    let pool = Pool::new("pool-reuse-cycle");
    let mut child = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "60000")
        .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_FAULT", "loaded-cycle")
        .env(
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
            LINGER_MS.to_string(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Building the example is most of the run; a reader that never checks
    // progress pages forever, so the deadline only has to outlast the build.
    let deadline = Instant::now() + Duration::from_secs(300);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the reuse workload was still paging thread/loaded/list after 300 s");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{said}");
    assert!(
        said.contains("thread/loaded/list answered the cursor \"0\" a second time"),
        "{said}"
    );
}

/// scripts/e2e-control.sh's shared app-server phases, `codex` the mock on PATH.
/// Linux-only, as its journeys are: the reuse phase reads `/proc`.
#[cfg(target_os = "linux")]
struct ControlSuite {
    pool: Pool,
}

#[cfg(target_os = "linux")]
impl ControlSuite {
    fn new(tag: &str) -> Self {
        let pool = Pool::new(tag);
        let bin = pool.path("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(mock_bin(), bin.join("codex")).unwrap();
        ControlSuite { pool }
    }

    fn run(&self, envs: &[(&str, &str)]) -> Output {
        let pool = &self.pool;
        let path = format!(
            "{}:{}",
            pool.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new("bash");
        cmd.arg(repo_root().join("scripts/e2e-control.sh"))
            .env_remove("ONEHARNESS_NO_CONFIG")
            .env("PATH", path)
            .env("ONEHARNESS_BIN", oneharness_bin())
            .env("CODEX_E2E_AUTH", "1")
            .env("OH_CONTROL_ONLY", "pooled")
            .env("XDG_STATE_HOME", pool.path("state"))
            .env("CODEX_HOME", pool.path("codex-home"))
            .env("CODEX_MOCK_DIR", pool.path("mock"))
            .env("ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS", "1000")
            .env("OH_REUSE_SETTLE_MS", "2000")
            .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
            .env("OH_E2E_TELEMETRY_DIR", pool.path("telemetry"))
            .env("MOCK_CODEX_RUN_COMMAND", "eval \"$OH_POOLED_COMMAND\"")
            .env("MOCK_CODEX_COMPLETE_TURN", "1")
            // A resident footprint the size of a real server's, so the memory
            // ratio reads retention rather than a small process's noise.
            .env("CODEX_MOCK_SERVER_BYTES", (200 * 1024 * 1024).to_string());
        for var in [
            "OH_E2E_NO_SKIP",
            "OH_E2E_PHASE",
            "OH_E2E_PHASE_LIMIT",
            "ONEHARNESS_CONFIG",
        ] {
            cmd.env_remove(var);
        }
        for (key, value) in envs {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

/// scripts/e2e-control.sh's pooled-kill bound restates the pool's own sweep
/// interval and stop deadline, and is held to them here.
#[cfg(target_os = "linux")]
#[test]
fn the_control_suite_s_kill_bound_is_the_pool_s_own_deadlines() {
    let script = std::fs::read_to_string(repo_root().join("scripts/e2e-control.sh")).unwrap();
    let stated = |name: &str| -> u64 {
        let line = script
            .lines()
            .find(|line| line.trim_start().starts_with("local sweep_s="))
            .expect("the pooled-kill phase states its deadlines");
        line.split_whitespace()
            .find_map(|word| word.strip_prefix(&format!("{name}=")))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("no numeric {name} in `{line}`"))
    };
    assert_eq!(stated("sweep_s"), DEAD_LEASE_SWEEP_INTERVAL.as_secs());
    assert_eq!(stated("stop_s"), DEAD_LEASE_STOP_DEADLINE.as_secs());
    assert!(script.contains("bound=$((sweep_s + stop_s))"));
}

/// The suite's pooled phases pass against a mock that keeps each dispatch's
/// environment to its own commands, on one server, out of its rollouts, and
/// stops a killed dispatch's heartbeat; and each fails, naming its phase and
/// assertion, when the mock leaks one dispatch's value into the other's
/// command or into a rollout, runs the two on two servers, or leaves the
/// heartbeat running.
#[cfg(target_os = "linux")]
#[test]
fn the_control_suite_s_pooled_phases_pass_and_fail_for_the_right_reason() {
    let suite = ControlSuite::new("pool-suite");
    let passed = suite.run(&[]);
    let said = String::from_utf8_lossy(&passed.stderr);
    assert!(passed.status.success(), "{said}");
    for line in [
        "pooled-env",
        "pooled-kill",
        "pooled-reuse",
        "each pooled dispatch saw only its own variable",
    ] {
        assert!(said.contains(line), "{line} missing from: {said}");
    }
    for (fault, phase, assertion) in [
        ("leak-env", "pooled-env", "saw dispatch"),
        ("rollout-env", "pooled-env", "reached a thread rollout"),
        ("vary-version", "pooled-env", "ran on two servers"),
        ("escape-marker", "pooled-kill", "kept beating"),
    ] {
        let suite = ControlSuite::new("pool-suite-fault");
        let failed = suite.run(&[("CODEX_MOCK_FAULT", fault)]);
        let said = String::from_utf8_lossy(&failed.stderr);
        assert!(
            !failed.status.success(),
            "{fault}: the suite passed: {said}"
        );
        assert!(
            said.contains(&format!("FAIL: {phase}: ")) && said.contains(assertion),
            "{fault}: expected `FAIL: {phase}: … {assertion}` in: {said}"
        );
    }
}

/// The suite's reuse phase runs the workload and the memory budget's check:
/// it passes when the pool releases each thread, and fails naming both
/// readings — and the threads still loaded at the settle bound — when the mock
/// ignores the release and keeps a large allocation per thread it keeps.
#[cfg(target_os = "linux")]
#[test]
fn the_control_suite_s_reuse_phase_fails_naming_both_readings_when_threads_are_retained() {
    let suite = ControlSuite::new("pool-suite-rss");
    let bytes = (100 * 1024 * 1024).to_string();
    let kept = suite.run(&[("CODEX_MOCK_THREAD_BYTES", &bytes)]);
    let said = String::from_utf8_lossy(&kept.stderr);
    assert!(kept.status.success(), "{said}");
    assert!(said.contains("within"), "{said}");
    let suite = ControlSuite::new("pool-suite-rss-leak");
    let retained = suite.run(&[
        ("CODEX_MOCK_THREAD_BYTES", &bytes),
        ("CODEX_MOCK_IGNORE_UNSUBSCRIBE", "1"),
        // Pages of two: a retained thread past the first page is named too.
        ("CODEX_MOCK_LOADED_PAGE", "2"),
    ]);
    let said =
        String::from_utf8_lossy(&retained.stderr) + String::from_utf8_lossy(&retained.stdout);
    assert!(!retained.status.success(), "{said}");
    assert!(said.contains("FAIL: pooled-reuse: "), "{said}");
    assert!(
        said.contains("still loaded"),
        "the retained threads went unnamed: {said}"
    );
    let reuse: Value =
        serde_json::from_str(&std::fs::read_to_string(suite.pool.path("reuse.json")).unwrap())
            .unwrap();
    let retained = reuse["retained"].as_array().unwrap();
    assert_eq!(
        retained.len(),
        5,
        "every retained thread is named, over every page: {retained:?}"
    );
    assert!(
        said.contains("after the last of 5 leases: ") && said.contains("after the first: "),
        "{said}"
    );
}

/// What a pooled dispatch cut short leaves behind: its live turn interrupted
/// on the server, no process carrying its marker, and its thread released.
fn left_nothing_behind(pool: &Pool, server: u32, report: &Value) {
    let thread = thread_of(report);
    assert_eq!(
        pool.turn_of(server, &thread)["status"],
        "interrupted",
        "{}",
        pool.state(server)
    );
    let marker = pool.turn_of(server, &thread)["marker"]
        .as_str()
        .unwrap()
        .to_string();
    let marked = find_marked(&ProcessMarker {
        name: "ONEHARNESS_DISPATCH_MARKER".to_string(),
        value: marker,
    })
    .unwrap();
    assert!(
        marked.is_empty(),
        "{marked:?} still carry the dispatch's marker"
    );
    assert!(
        !holds_thread(pool, server, &thread),
        "{}",
        pool.state(server)
    );
}

/// A pooled turn past its `--timeout` is `timeout`, and one whose run is
/// cancelled (SIGTERM to oneharness) is `cancelled`; either way its live turn
/// is interrupted on the server, its commands stopped and its thread released,
/// rather than left running on a server nobody reads.
#[test]
fn a_pooled_turn_cut_short_by_its_timeout_or_a_cancel_leaves_nothing_behind() {
    let pool = Pool::new("pool-cut");
    let never = pool.file("never");
    let beat = pool.file("timeout-beat");
    let (output, report) = pool.run(pool.command(
        "timed",
        &["--mode", "bypass", "--timeout", "2"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    assert_eq!(
        result(&report)["status"],
        "timeout",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    left_nothing_behind(&pool, reported_server(&report), &report);

    let beat = pool.file("cancel-beat");
    let child = pool.spawn(pool.command(
        "cancelled",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    wait_for("the heartbeat to start", Duration::from_secs(20), || {
        lines(&beat) >= 3
    });
    // SAFETY: plain integers; the pid is the run this journey started.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let (_, report) = Pool::finish(child);
    assert_eq!(result(&report)["status"], "cancelled", "{report}");
    left_nothing_behind(&pool, reported_server(&report), &report);
}

/// A server that exits before it accepts a connection is started once more;
/// the dispatch runs on the second.
#[test]
fn a_server_that_dies_at_start_is_started_once_more() {
    let pool = Pool::new("pool-restart");
    let first = pool.file("first-launch");
    let pool = pool.with_env("CODEX_MOCK_EXIT_FIRST", &first);
    let (output, report) = pool.run(pool.command("restart", &[], &[]));
    succeeded(&output, &report);
    assert!(Path::new(&first).exists(), "the first launch never ran");
    assert_eq!(
        reported_server(&report),
        pool.the_server(),
        "the dispatch did not run on the relaunch"
    );
}

/// A connection that refuses the dispatch's write while its turn runs — here,
/// the answer to an approval request onto a connection the server stopped
/// reading — is the shared server lost under the turn: `app_server_lost`.
#[test]
fn a_write_refused_under_a_live_turn_is_app_server_lost() {
    let pool = Pool::new("pool-write-lost");
    let never = pool.file("never");
    let (output, report) = pool.run(pool.command(
        "write-lost",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", "true"),
            ("MOCK_CODEX_REQUEST_APPROVAL", "1"),
            ("MOCK_CODEX_SHUT_READ_ON_APPROVAL", "1"),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    let lost = result(&report);
    assert_eq!(
        lost["status"],
        "nonzero",
        "{lost}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
    assert!(
        lost["error"]
            .as_str()
            .unwrap_or_default()
            .contains("refused a write"),
        "the loss was not read off the refused write: {lost}"
    );
}
