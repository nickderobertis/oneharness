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
use oneharness_core::io::server_pool::{
    pid_alive, LeaseHolder, Pid, DEAD_LEASE_STOP_DEADLINE, DEAD_LEASE_SWEEP_INTERVAL,
};
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

/// Whether `pid` names a running process, by the pool's own reading — a
/// zombie counts as gone on every platform, as it does to the pool.
fn alive(pid: u32) -> bool {
    Pid::new(pid).is_ok_and(pid_alive)
}

/// Read `pipe` to its end on a thread of its own; the receiver gets the bytes
/// once the last writer has closed it.
fn read_to_end(
    mut pipe: impl std::io::Read + Send + 'static,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (sender, ends) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    ends
}

/// Wait up to `limit` for `child` to exit; past it, kill it and fail the
/// journey with `label`, so a dispatch that never ends fails here by name.
fn exits_within(child: &mut Child, limit: Duration, label: &str) -> std::process::ExitStatus {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            sigkill(child.id());
            let _ = child.wait();
            panic!("{label}: still running after {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
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
    /// The `oneharness` every dispatch runs, when not the built one.
    bin: Option<PathBuf>,
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
        // Then every process this world started is stopped and gone before
        // the journey returns: its servers, their commands, and the sweepers
        // beside them, which would otherwise run on after the scratch
        // directory is removed until their next sweep noticed.
        // A sweeper by the identity it recorded, so a pid recycled since it
        // ended is never signalled.
        let sweepers = std::fs::read_dir(&servers)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("sweeper.json")).ok())
            .filter_map(|text| serde_json::from_str::<LeaseHolder>(&text).ok())
            .filter(LeaseHolder::is_live)
            .map(|holder| u64::from(holder.pid.get()));
        let commands = self
            .states()
            .into_iter()
            .flat_map(|state| state["commands"].as_array().cloned().unwrap_or_default())
            .filter_map(|command| command["pid"].as_u64());
        let started: Vec<u32> = self
            .server_pids()
            .into_iter()
            .chain(commands.map(|pid| pid as u32))
            .chain(sweepers.map(|pid| pid as u32))
            .filter(|pid| alive(*pid))
            .collect();
        for pid in &started {
            sigkill(*pid);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while started.iter().any(|pid| alive(*pid)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
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
            bin: None,
        }
    }

    fn with_bin(mut self, bin: PathBuf) -> Self {
        self.bin = Some(bin);
        self
    }

    fn oneharness(&self) -> PathBuf {
        self.bin.clone().unwrap_or_else(oneharness_bin)
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
        let mut cmd = Command::new(self.oneharness());
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
        let mut cmd = Command::new(self.oneharness());
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

    /// [`Self::finish`], failing the journey if the run has not ended within
    /// `limit` — after killing it, so a run that never ends is a failure that
    /// says so, not a journey that waits on it forever.
    fn finish_within(mut child: Child, limit: Duration) -> (Output, Value) {
        let deadline = Instant::now() + limit;
        while child.try_wait().expect("poll oneharness").is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().expect("oneharness killed");
                panic!(
                    "the run had not ended after {limit:?}\n--- stderr ---\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Self::finish(child)
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

/// `ask` (an interrupt against a turn the server already holds live), asked
/// again while its answer is `no_active_turn`: the server marks a turn live a
/// moment before the dispatch has read the reply naming it, and until then
/// that answer is the true one. Answers the first other answer, or the last
/// at a 20 s bound.
fn once_the_turn_is_known<T>(mut ask: impl FnMut() -> T, answer: impl Fn(&T) -> &Value) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let asked = ask();
        if answer(&asked)["reason"] != "no_active_turn" || Instant::now() >= deadline {
            return asked;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control` through the real pool; only codex is the repository's subprocess double, held live so two dispatches can be shown sharing (or not sharing) one server, which the gate cannot do with real Codex. Real Codex's sharing is held by scripts/e2e-control.sh's pooled-env phase.
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
    // Held live while the same release runs again, so the server it started
    // is certainly still up: reuse is what is asserted, not that a second run
    // beat the linger.
    let release = pool.path("ver-b-release");
    let held = pool.spawn(pool.command(
        "ver-b",
        &["--bin", &release_a],
        &[("MOCK_CODEX_HOLD_UNTIL", &release.display().to_string())],
    ));
    wait_for("ver-b's turn to be live", Duration::from_secs(30), || {
        pool.server_pids()
            .into_iter()
            .any(|pid| pool.live_turns(pid) == 1)
    });
    // The same release again: the server it already has.
    let (out, again) = pool.run(pool.command("ver-c", &["--bin", &release_a], &[]));
    succeeded(&out, &again);
    std::fs::write(&release, "").unwrap();
    let (out, after) = Pool::finish(held);
    succeeded(&out, &after);
    assert_ne!(
        reported_server(&before),
        reported_server(&after),
        "a new release reused the old release's server"
    );
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
// llmlint: ignore-end[e2e_not_mocked]

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
        // Set by an instrumented mock's own profiling runtime inside its
        // process (the coverage run), never passed in by oneharness.
        || name.starts_with("__LLVM_PROFILE_")
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

/// A dispatch whose `oneharness` executable is replaced while it runs — as a
/// build of the same target does under a running suite — still starts the
/// pool's sweeper, so its death is found and its commands stopped with no other
/// dispatch alive. On Linux the running image's path then reads
/// `… (deleted)`, which names no file to start the sweeper from.
#[test]
fn a_sigkilled_pooled_dispatch_is_stopped_after_its_executable_was_replaced() {
    use std::os::unix::fs::PermissionsExt;
    let pool = Pool::new("pool-kill-rebuilt");
    let copy = pool.path("oneharness");
    std::fs::copy(oneharness_bin(), &copy).unwrap();
    // The version probe runs before the lease, so the copy is replaced (a new
    // file renamed over it, as a linker's output replaces the old one) after
    // the dispatch started and before it starts the sweeper.
    let codex = pool.path("codex");
    std::fs::write(
        &codex,
        format!(
            "#!/bin/sh\ncase \"$1\" in --version) cp '{copy}' '{copy}.new' && mv -f '{copy}.new' '{copy}' || exit 1 ;; esac\nexec '{mock}' \"$@\"\n",
            copy = copy.display(),
            mock = mock_bin().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    let pool = pool.with_bin(copy);
    let beat = pool.file("heartbeat");
    let never = pool.file("never-released");
    let mut victim = pool.spawn(pool.command(
        "victim",
        &[
            "--mode",
            "bypass",
            "--bin",
            &format!("codex={}", codex.display()),
        ],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    let server = pool.await_live_turns(1);
    wait_for("the heartbeat to start", Duration::from_secs(10), || {
        lines(&beat) >= 3
    });
    let command_pid = command_running(&pool, server, &beat)["pid"]
        .as_u64()
        .unwrap() as u32;
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
    wait_for(
        "the server to stop once no lease is live",
        Duration::from_millis(LINGER_MS) + bound + Duration::from_secs(5),
        || !alive(server),
    );
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

/// A release the server refuses, answers with a status that is no release, or
/// leaves unanswered past its reply deadline (5 s), is said on stderr naming
/// the thread and why, and bounded: the
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
        (
            "odd-status",
            "CODEX_MOCK_FAULT",
            "unsubscribe-odd-status",
            "it answered thread/unsubscribe with status \"pending\", not unsubscribed",
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

        let answer = once_the_turn_is_known(
            || {
                if through_the_socket {
                    control_request(&pool.socket("target"), INTERRUPT_LINE)
                } else {
                    interrupt(&pool, "target").0
                }
            },
            |answer| answer,
        );
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

    // Ended, still releasing: nothing is sent. The release is held long enough
    // that both asks reach the run on a loaded host; the 2 s bound below is
    // what proves they did not wait on the server.
    let ending = pool.spawn(pool.command(
        "ending",
        &[],
        &[
            ("MOCK_CODEX_UNSUBSCRIBE_DELAY_MS", "10000"),
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
    let not_found = once_the_turn_is_known(|| interrupt(&pool, "lost-thread").0, |a| a);
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
    let stopped = once_the_turn_is_known(|| interrupt(&pool, "own-child").0, |a| a);
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
    let (answer, took) = once_the_turn_is_known(|| interrupt(&pool, "silent"), |(a, _)| a);
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
        let (answer, served) =
            once_the_turn_is_known(|| redirect(&pool, session, &message), |(a, _)| a);
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

/// Each way a pooled dispatch can fail before its turn is named on its result,
/// within its bound: a `--version` probe that fails or never answers (the
/// probe's 15 s), and a server that stays alive but never listens (the
/// dispatch's own `--timeout`). A linger override that is not whole
/// milliseconds is warned about and the default linger used instead.
#[test]
fn a_pooled_dispatch_names_each_failure_before_its_turn_within_its_bound() {
    let pool = Pool::new("pool-before-turn");
    /// A session, the knob and value that stage its failure, its extra
    /// arguments, what its error says, and the bound it returns within.
    type Case<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a str, Duration);
    let cases: [Case; 3] = [
        (
            "version-fails",
            "CODEX_MOCK_VERSION_EXIT",
            "3",
            &[],
            "--version` failed (exit status: 3)",
            Duration::from_secs(10),
        ),
        (
            "version-hangs",
            "CODEX_MOCK_VERSION_DELAY_MS",
            "30000",
            &[],
            "--version` did not answer within 15s",
            Duration::from_secs(30),
        ),
        (
            "never-listens",
            "CODEX_MOCK_FAULT",
            "never-listen",
            &["--timeout", "3"],
            "did not accept a connection on ",
            Duration::from_secs(15),
        ),
    ];
    for (session, knob, value, args, said, bound) in cases {
        let started = Instant::now();
        let (_, report) = pool.run(pool.command(session, args, &[(knob, value)]));
        let took = started.elapsed();
        let ran = result(&report);
        assert_eq!(ran["status"], "spawn-error", "{session}: {ran}");
        assert!(
            ran["error"].as_str().is_some_and(|e| e.contains(said)),
            "{session}: {ran}"
        );
        assert!(
            took < bound,
            "{session}: returned after {took:?}, past {bound:?}"
        );
    }

    let (output, report) = pool.run(pool.command(
        "bad-linger",
        &[],
        &[("ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS", "soon")],
    ));
    succeeded(&output, &report);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "oneharness: warning: ignoring ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS=\"soon\": not whole milliseconds; the server lingers the default 60s"
        ),
        "{stderr}"
    );
    let records: Vec<Value> = std::fs::read_dir(pool.path("state/oneharness/servers"))
        .unwrap()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("server.json")).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    assert!(
        records.iter().any(|record| record["linger_ms"] == 60_000),
        "no server started under the default linger: {records:?}"
    );
}

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control` through the real pool; only codex is the repository's subprocess double, because real Codex cannot be made to exit before listening with a chosen stderr (whitespace, non-UTF-8, an unopenable log) on demand. Real Codex's own early-exit reason reached this report on macOS CI (run 38942062567).
/// The stderr an early exit quotes is only what the server legibly said: a
/// stderr of whitespace alone is reported as nothing said, one that is not
/// UTF-8 is quoted with each undecodable byte — the character the 2 KiB cut
/// lands inside included — replaced rather than refused, and a log the launch
/// could not keep is reported as nothing said rather than failing the report.
#[test]
fn an_early_exit_quotes_only_the_stderr_it_can_read() {
    let pool = Pool::new("pool-early-exit-legible");
    let early_exit = |id: &str, fault: &str| {
        let (_, report) = pool.run(pool.command(id, &[], &[("CODEX_MOCK_FAULT", fault)]));
        let ran = result(&report).clone();
        assert_eq!(ran["status"], "spawn-error", "{ran}");
        ran["error"].as_str().unwrap_or_default().to_string()
    };

    let error = early_exit("blank", "exit-blank");
    assert!(
        error.contains("it ended with exit status: 5, and wrote nothing to stderr"),
        "{error}"
    );

    let error = early_exit("non-utf8", "exit-non-utf8");
    let (_, tail) = error
        .split_once("it ended with exit status: 9, its stderr ending: ")
        .unwrap_or_else(|| panic!("no exit status and stderr in: {error}"));
    let cut = format!("\u{2026}\u{fffd}{}", "\u{e9}".repeat(1006));
    assert!(tail.starts_with(&cut), "{tail}");
    assert!(
        tail.ends_with("\u{e9}Error: mock codex: bad byte \u{fffd} here"),
        "{tail}"
    );

    // The entry the last launch made keeps its log; a dangling link in its
    // place is never followed, by the launch or by the report.
    let entries: Vec<PathBuf> = std::fs::read_dir(pool.path("state/oneharness/servers"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("server.log").exists())
        .filter(|path| {
            std::fs::read(path.join("server.log")).is_ok_and(|bytes| bytes.contains(&0xff))
        })
        .collect();
    let [entry] = entries.as_slice() else {
        panic!("expected one pool entry with the non-UTF-8 log, found {entries:?}");
    };
    let log = entry.join("server.log");
    // llmlint: ignore-block[tests_mirror_real_usage] No oneharness interface puts a link in a pool entry's log's place; an outside actor doing so is the condition, so the journey stages it and asserts through the dispatch's report.
    std::fs::remove_file(&log).unwrap();
    std::os::unix::fs::symlink(entry.join("nowhere"), &log).unwrap();
    // llmlint: ignore-end[tests_mirror_real_usage]
    let error = early_exit("unreadable-log", "exit-non-utf8");
    assert!(
        error.contains("it ended with exit status: 9, and wrote nothing to stderr"),
        "{error}"
    );
    assert!(!entry.join("nowhere").exists(), "the link was followed");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control` through the real pool; only codex is the repository's subprocess double, because real Codex cannot be made to exit before listening with a known status and stderr on demand. Real Codex's own early-exit reason reached this report on macOS CI (run 38942062567).
/// A shared server that exits before it listens fails the dispatch with the
/// server's own account of why — its exit status and the end of its stderr —
/// since nobody else reads a detached server's stderr. The log it is read from
/// is the server's alone, started afresh with each launch.
#[test]
fn a_server_that_exits_before_listening_fails_the_dispatch_with_its_own_reason() {
    let pool = Pool::new("pool-early-exit");
    let (_, report) =
        pool.run(pool.command("early-exit", &[], &[("CODEX_MOCK_FAULT", "exit-at-start")]));
    let ran = result(&report);
    assert_eq!(ran["status"], "spawn-error", "{ran}");
    let error = ran["error"].as_str().unwrap_or_default();
    let entries: Vec<PathBuf> = std::fs::read_dir(pool.path("state/oneharness/servers"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    let [entry] = entries.as_slice() else {
        panic!("expected one pool entry, found {entries:?}");
    };
    let socket = entry.join("sock").join("app-server.sock");
    let last = format!(
        "mock codex: warming up 199\nError: mock codex refuses to listen on {}",
        socket.display()
    );
    for said in [
        "exited before accepting a connection on ",
        "it ended with exit status: 7, its stderr ending: …",
        &last,
    ] {
        assert!(error.contains(said), "no `{said}` in: {error}");
    }
    // Only the end of a long stderr is quoted: its last 2 KiB, marked as cut.
    let (_, tail) = error.split_once("its stderr ending: ").unwrap();
    assert!(
        tail.len() <= 2048 + "…".len(),
        "{} bytes: {tail}",
        tail.len()
    );
    assert!(tail.ends_with(&last), "{tail}");
    assert!(!tail.contains("warming up 000"), "{tail}");
    // Started twice — a server that dies at start is retried once — and the
    // log holds only the last launch's stderr, all of it.
    let log = entry.join("server.log");
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(text.matches("warming up 000").count(), 1, "{text}");
    assert!(text.trim_end().ends_with(&last), "{text}");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&log).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{}: {mode:o}", log.display());

    // A log left readable by others is made private again by the next launch.
    // llmlint: ignore[tests_mirror_real_usage] No oneharness interface loosens a pool entry's log; an outside chmod is the condition, so the journey makes it and reads the mode the next launch leaves.
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (_, report) = pool.run(pool.command(
        "early-exit-again",
        &[],
        &[("CODEX_MOCK_FAULT", "exit-at-start")],
    ));
    assert_eq!(result(&report)["status"], "spawn-error", "{report}");
    let mode = std::fs::metadata(&log).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{}: {mode:o}", log.display());

    // A FIFO in the log's place is neither written to nor read from — either
    // would wait for a peer that never comes — so the launch reports the exit
    // with no stderr to quote, at once.
    // llmlint: ignore-block[tests_mirror_real_usage] No oneharness interface puts a FIFO in a pool entry; an outside actor doing so is the condition, so the journey stages it and asserts through the dispatch's report.
    std::fs::remove_file(&log).unwrap();
    let fifo = std::ffi::CString::new(log.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path and a plain mode.
    assert_eq!(
        unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) },
        0,
        "{}",
        log.display()
    );
    // llmlint: ignore-end[tests_mirror_real_usage]
    let started = Instant::now();
    let (_, report) =
        pool.run(pool.command("fifo-log", &[], &[("CODEX_MOCK_FAULT", "exit-at-start")]));
    let ran = result(&report);
    assert_eq!(ran["status"], "spawn-error", "{ran}");
    assert!(
        ran["error"]
            .as_str()
            .unwrap_or_default()
            .contains("it ended with exit status: 7, and wrote nothing to stderr"),
        "{ran}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );

    let (_, report) =
        pool.run(pool.command("silent-exit", &[], &[("CODEX_MOCK_FAULT", "exit-silently")]));
    let ran = result(&report);
    assert_eq!(ran["status"], "spawn-error", "{ran}");
    let error = ran["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("it ended with exit status: 3, and wrote nothing to stderr"),
        "{error}"
    );

    // A log that cannot be opened costs only the reason, never the launch:
    // the server starts with its stderr discarded and the turn completes. The
    // fault is part of the pool key, so a serving launch has its own entry,
    // made by a first turn whose server is let go before its log is replaced.
    let entries = || -> Vec<PathBuf> {
        std::fs::read_dir(pool.path("state/oneharness/servers"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect()
    };
    let before = entries();
    let (out, report) = pool.run(pool.command("serving", &[], &[]));
    succeeded(&out, &report);
    let first = reported_server(&report);
    let made: Vec<PathBuf> = entries()
        .into_iter()
        .filter(|path| !before.contains(path))
        .collect();
    let [serving] = made.as_slice() else {
        panic!("expected one new pool entry, found {made:?}");
    };
    wait_for(
        "the serving turn's server to stop after its linger",
        Duration::from_millis(LINGER_MS) + DEAD_LEASE_SWEEP_INTERVAL * 2 + Duration::from_secs(5),
        || !alive(first),
    );
    let log = serving.join("server.log");
    // llmlint: ignore-block[tests_mirror_real_usage] No oneharness interface makes a pool entry's log unopenable: the condition is the filesystem refusing it (a full or read-only state dir, a path taken by something else), so the journey stages exactly that and asserts only through what a caller sees, the turn completing on a fresh server.
    std::fs::remove_file(&log).unwrap();
    std::fs::create_dir(&log).unwrap();
    // llmlint: ignore-end[tests_mirror_real_usage]
    let (out, report) = pool.run(pool.command("no-log", &[], &[]));
    succeeded(&out, &report);
    let second = reported_server(&report);
    assert_ne!(second, first, "the stopped server served again");
    assert!(log.is_dir(), "{}", log.display());

    // A link where the log belongs is never written through: the file it
    // names keeps its content, and the launch goes on without a log.
    wait_for(
        "the no-log turn's server to stop after its linger",
        Duration::from_millis(LINGER_MS) + DEAD_LEASE_SWEEP_INTERVAL * 2 + Duration::from_secs(5),
        || !alive(second),
    );
    let elsewhere = pool.path("elsewhere.txt");
    std::fs::write(&elsewhere, "not the server's").unwrap();
    // llmlint: ignore-block[tests_mirror_real_usage] As above: a link left in the pool entry is an outside actor's doing, which no oneharness interface makes, so the journey stages it and asserts through the turn and the file the link names.
    std::fs::remove_dir(&log).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &log).unwrap();
    // llmlint: ignore-end[tests_mirror_real_usage]
    let (out, report) = pool.run(pool.command("linked-log", &[], &[]));
    succeeded(&out, &report);
    assert_ne!(
        reported_server(&report),
        second,
        "the stopped server served again"
    );
    assert_eq!(
        std::fs::read_to_string(&elsewhere).unwrap(),
        "not the server's"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control` through the real pool and a real directory; only codex is the repository's subprocess double, which refuses a loose socket directory exactly as codex-cli 0.163 does, because the gate cannot run real Codex. The live `oh_codex_rendezvous_check` holds the double's contract to real Codex.
/// Codex refuses to start when its socket rendezvous directory is not this
/// user's at mode 0700 — macOS CI met one another tool had made at 0755, and
/// every shared server exited at once. The launch tightens one that is ours,
/// so the turn completes; anything else it leaves for Codex to refuse, and the
/// dispatch says why. Run against a directory of the journey's own, never the
/// host's `/tmp/codex-daemon-<uid>`, which every Codex on it shares.
#[test]
fn a_shared_server_starts_once_its_loose_socket_directory_is_made_private() {
    use std::os::unix::fs::PermissionsExt;
    let pool = Pool::new("pool-daemon-dir");
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let daemon = pool.path("daemon");
    std::fs::create_dir(&daemon).unwrap();
    std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o755)).unwrap();
    let at = |dir: &Path| {
        let dir = dir.display().to_string();
        vec![
            (
                "ONEHARNESS_INTERNAL_CODEX_DAEMON_DIR".to_string(),
                dir.clone(),
            ),
            ("CODEX_MOCK_DAEMON_DIR".to_string(), dir),
        ]
    };
    let envs = at(&daemon);
    let envs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (output, report) = pool.run(pool.command("loose", &[], &envs));
    succeeded(&output, &report);
    assert_eq!(mode(&daemon), 0o700);
    // The server bound under that directory, not the host's.
    let socket = std::fs::read_dir(pool.path("state/oneharness/servers"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path().join("sock").join("app-server.sock"))
        .find(|socket| std::fs::symlink_metadata(socket).is_ok())
        .expect("the shared server's socket link");
    assert!(
        std::fs::read_link(&socket).unwrap().starts_with(&daemon),
        "{}",
        socket.display()
    );

    // Not a directory at all: left alone, and Codex's refusal is the reason.
    let file = pool.path("daemon-file");
    std::fs::write(&file, "").unwrap();
    let envs = at(&file);
    let envs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (_, report) = pool.run(pool.command("not-a-dir", &[], &envs));
    let ran = result(&report);
    assert_eq!(ran["status"], "spawn-error", "{ran}");
    let error = ran["error"].as_str().unwrap_or_default();
    assert!(
        error.contains(
            "it ended with exit status: 1, its stderr ending: Error: app-server socket directory must be a user-owned directory with mode 0700"
        ),
        "{error}"
    );
    assert!(std::fs::metadata(&file).unwrap().is_file());

    // A link in its place: not followed, so the directory it names keeps its
    // mode, and Codex's refusal is the reason.
    let target = pool.path("daemon-target");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = pool.path("daemon-link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let envs = at(&link);
    let envs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (_, report) = pool.run(pool.command("linked-dir", &[], &envs));
    let ran = result(&report);
    assert_eq!(ran["status"], "spawn-error", "{ran}");
    assert!(
        ran["error"].as_str().unwrap_or_default().contains(
            "Error: app-server socket directory must be a user-owned directory with mode 0700"
        ),
        "{ran}"
    );
    assert_eq!(mode(&target), 0o755);
}
// llmlint: ignore-end[e2e_not_mocked]

/// A server that refuses `initialize` fails a pooled session's open naming the
/// refusal — here through the reuse workload, which opens its leases that way.
#[test]
fn a_refused_initialize_fails_the_pooled_session_naming_it() {
    let pool = Pool::new("pool-refused-init");
    let output = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "1000")
        .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_FAULT", "refuse-initialize")
        .env(
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
            LINGER_MS.to_string(),
        )
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{said}");
    assert!(
        said.contains("the app-server's initialize failed: it refused initialize: ")
            && said.contains("the mock refuses initialize"),
        "{said}"
    );
}

/// A process that never hosts the sweeper — a library caller of
/// oneharness-core, as this test process is — starts no sweeper: its released
/// server outlives the linger and the sweep interval, and is reclaimed by the
/// next pool operation instead — a later lease, which replaces it, and a sweep.
#[test]
fn without_a_hosted_sweeper_a_lingering_server_is_reclaimed_by_the_next_pool_operation() {
    use oneharness_core::io::app_server::{PooledDispatch, PooledSession};
    use oneharness_core::io::server_pool;
    let pool = Pool::new("pool-unhosted");
    let linger = Duration::from_millis(300);
    // Each nextest test is its own process, so this environment is this
    // journey's alone.
    std::env::set_var("XDG_STATE_HOME", pool.path("state"));
    std::env::set_var("CODEX_HOME", pool.path("codex-home"));
    std::env::set_var("CODEX_MOCK_DIR", pool.path("mock"));
    std::env::set_var(
        "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
        linger.as_millis().to_string(),
    );
    let cwd = pool.worktree("work");
    let argv = vec![mock_bin().display().to_string(), "app-server".to_string()];
    let plan = || PooledDispatch::plan(&argv, &[], &[], &cwd, false).unwrap();

    let first = PooledSession::open(plan()).unwrap();
    let first_pid = first.record().pid.get();
    let entry = first.entry().to_path_buf();
    first.release();
    std::thread::sleep(linger + DEAD_LEASE_SWEEP_INTERVAL * 2 + Duration::from_millis(500));
    assert!(
        alive(first_pid),
        "something stopped the server with no sweeper hosted"
    );
    assert!(
        !entry.join("sweeper.json").exists(),
        "a sweeper was recorded for a process that hosts none"
    );

    let second = PooledSession::open(plan()).unwrap();
    let second_pid = second.record().pid.get();
    assert_ne!(
        second_pid, first_pid,
        "the lingered-out server was leased again"
    );
    wait_for(
        "the lingered-out server to stop",
        Duration::from_secs(15),
        || !alive(first_pid),
    );
    second.release();
    std::thread::sleep(linger + Duration::from_millis(200));
    let root = server_pool::resolve_root(None).unwrap();
    server_pool::sweep(&root, linger).unwrap();
    wait_for("the swept server to stop", Duration::from_secs(15), || {
        !alive(second_pid)
    });
    assert!(!entry.join("server.json").exists());
}

/// This journey's process as a library caller of the shared pool: its pool
/// root, Codex home, mock records and linger, set in the environment that
/// `PooledSession` reads (each nextest test is its own process, so it is this
/// journey's alone), and the mock's `app-server` launch.
fn as_library_caller(pool: &Pool) -> Vec<String> {
    std::env::set_var("XDG_STATE_HOME", pool.path("state"));
    std::env::set_var("CODEX_HOME", pool.path("codex-home"));
    std::env::set_var("CODEX_MOCK_DIR", pool.path("mock"));
    std::env::set_var(
        "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
        LINGER_MS.to_string(),
    );
    vec![mock_bin().display().to_string(), "app-server".to_string()]
}

/// Stop the servers a library caller's released sessions left lingering: it
/// hosts no sweeper, so the next pool operation is what reclaims them — here,
/// a sweep once their linger has passed.
fn reclaim_lingering_servers() {
    use oneharness_core::io::server_pool;
    std::thread::sleep(Duration::from_millis(LINGER_MS + 200));
    let root = server_pool::resolve_root(None).unwrap();
    server_pool::sweep(&root, Duration::from_millis(LINGER_MS)).unwrap();
}

/// A library caller's pooled session — oneharness-core's `PooledSession`, the
/// lease the reuse workload takes — runs a thread's whole life on the shared
/// server: it opens the thread, runs a shell turn on it, reopens it, finds it
/// loaded across every page of the listing, and once the session is released
/// the server has let it go. A session with no thread yet has no shell turn
/// to run.
#[test]
fn a_library_pooled_session_runs_a_thread_through_its_whole_life() {
    use oneharness_core::io::app_server::{PooledDispatch, PooledSession};
    use oneharness_core::io::server_pool::ThreadId;
    let pool = Pool::new("pool-session-life");
    let argv = as_library_caller(&pool);
    let cwd = pool.worktree("work");
    // Pages of one: the listing pages through every loaded thread.
    let env = [("CODEX_MOCK_LOADED_PAGE".to_string(), "1".to_string())];
    let plan = || PooledDispatch::plan(&argv, &env, &[], &cwd, false).unwrap();

    let mut keeper = PooledSession::open(plan()).unwrap();
    keeper.open_thread(&cwd, None).unwrap();
    let mut session = PooledSession::open(plan()).unwrap();
    assert_eq!(
        session.record().pid,
        keeper.record().pid,
        "the two sessions leased two servers"
    );
    assert_eq!(session.entry(), keeper.entry());
    assert_eq!(
        session.shell_turn("true").unwrap_err(),
        "the session has no thread to run a shell turn on"
    );
    let started = session.open_thread(&cwd, None).unwrap();
    let thread = started
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .and_then(ThreadId::new)
        .unwrap_or_else(|| panic!("thread/start named no thread: {started}"));
    session.shell_turn("true").unwrap();
    let resumed = session.open_thread(&cwd, Some(&thread)).unwrap();
    assert_eq!(
        resumed["result"]["thread"]["id"],
        thread.as_str(),
        "{resumed}"
    );
    let loaded = session
        .loaded_threads(Instant::now() + Duration::from_secs(10))
        .unwrap();
    assert_eq!(loaded.len(), 2, "both sessions' threads, over two pages");
    assert!(loaded.contains(&thread), "{loaded:?}");
    let server = session.record().pid.get();
    session.release();

    // The mock records a release after answering it.
    wait_for(
        "the server to record the session's thread unsubscribed",
        Duration::from_secs(10),
        || {
            pool.state(server)["releases"]
                .as_array()
                .is_some_and(|releases| {
                    releases.iter().any(|release| {
                        release["thread"] == thread.as_str() && release["status"] == "unsubscribed"
                    })
                })
        },
    );
    let still = keeper
        .loaded_threads(Instant::now() + Duration::from_secs(10))
        .unwrap();
    assert!(
        !still.contains(&thread),
        "the server still holds the released thread: {still:?}"
    );
    assert_eq!(still.len(), 1, "the keeper's own thread is still loaded");
    keeper.release();
    reclaim_lingering_servers();
    assert!(!alive(server), "the swept server is still running");
}

/// Each way the shared server can fail a library caller's pooled session —
/// in its `initialize`, its thread's open, a shell turn or the loaded-thread
/// listing — fails the call that met it, naming what the server did; a shell
/// turn that completes before its reply still completes. Each case runs on a
/// server of its own (the fault is in its launch environment, so in its pool
/// key), side by side, two of them waiting out a 30 s bound.
#[test]
fn a_library_pooled_session_names_each_way_the_server_fails_it() {
    use oneharness_core::io::app_server::{PooledDispatch, PooledSession};
    let pool = Pool::new("pool-session-faults");
    let argv = as_library_caller(&pool);
    let cwd = pool.worktree("work");
    let session = move |fault: &str, listing: Duration| -> Option<String> {
        let env = [("CODEX_MOCK_FAULT".to_string(), fault.to_string())];
        let plan = PooledDispatch::plan(&argv, &env, &[], &cwd, false).unwrap();
        let mut session = match PooledSession::open(plan) {
            Ok(session) => session,
            Err(err) => return Some(err),
        };
        let failed = session
            .open_thread(&cwd, None)
            .and_then(|_| session.shell_turn("true"))
            .and_then(|()| session.loaded_threads(Instant::now() + listing))
            .err();
        session.release();
        failed
    };
    let cases: [(&str, Option<&str>); 15] = [
        (
            "refuse-initialize",
            Some("the app-server's initialize failed: it refused initialize: "),
        ),
        ("refuse-thread-start", Some("thread/start was refused: ")),
        (
            "thread-start-no-thread",
            Some("thread/start named no thread: "),
        ),
        (
            "thread-start-bad-id",
            Some("thread/start named an unusable thread id \"\""),
        ),
        (
            "refuse-shell",
            Some("the app-server's thread/shellCommand failed: it refused thread/shellCommand: "),
        ),
        (
            "silent-shell",
            Some("the app-server did not answer thread/shellCommand"),
        ),
        (
            "shell-no-completion",
            Some("the shell turn did not complete within 30s"),
        ),
        (
            "shell-hangup",
            Some("thread/shellCommand: the shared codex app-server closed the connection"),
        ),
        (
            "fail-shell",
            Some("the shell turn ended failed, not completed"),
        ),
        ("shell-completes-first", None),
        ("refuse-loaded", Some("thread/loaded/list was refused: ")),
        (
            "loaded-bad-id",
            Some("thread/loaded/list named a thread 7 that is no id"),
        ),
        (
            "loaded-cursor-number",
            Some("thread/loaded/list answered a cursor 2 that is no string"),
        ),
        (
            "loaded-cycle",
            Some("thread/loaded/list answered the cursor \"0\" a second time"),
        ),
        // Stopped before a page's request or after its answer, as the clock
        // falls; both name the deadline.
        ("loaded-endless", Some("thread/loaded/list ")),
    ];
    let session = std::sync::Arc::new(session);
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(fault, said)| {
            let session = std::sync::Arc::clone(&session);
            let listing = if fault == "loaded-endless" {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(10)
            };
            std::thread::spawn(move || (fault, said, session(fault, listing)))
        })
        .collect();
    for run in runs {
        let (fault, said, failed) = run.join().unwrap();
        match (said, failed) {
            (None, None) => {}
            (Some(said), Some(failed)) => {
                assert!(failed.contains(said), "{fault}: no `{said}` in: {failed}");
                if fault == "loaded-endless" {
                    assert!(
                        failed.contains("deadline"),
                        "{fault}: the deadline went unnamed: {failed}"
                    );
                }
            }
            (said, failed) => panic!("{fault}: expected {said:?}, the session met {failed:?}"),
        }
    }
    reclaim_lingering_servers();
}

/// Whether this process's child `pid` has exited, leaving it unreaped for
/// whoever owns its handle.
fn exited_unreaped(pid: u32) -> bool {
    // SAFETY: a zeroed siginfo_t is a valid out-parameter for waitid.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: plain integers and a live out-parameter; WNOWAIT reaps nothing.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    // SAFETY: si_pid is valid to read once waitid filled the struct.
    waited == 0 && unsafe { info.si_pid() } == pid as libc::pid_t
}

/// A shared server that dies under a library caller's live session is found
/// dead by the next session's lease, which stops the commands the dead
/// server ran for the live session — and not one another server runs for
/// another dispatch — before it starts a fresh server.
#[test]
fn a_library_pooled_session_s_commands_are_stopped_when_its_server_is_found_dead() {
    use oneharness_core::io::app_server::{PooledDispatch, PooledSession};
    let pool = Pool::new("pool-session-dead-server");
    let argv = as_library_caller(&pool);
    let cwd = pool.worktree("work");
    let plan =
        |env: &[(String, String)]| PooledDispatch::plan(&argv, env, &[], &cwd, false).unwrap();
    // A command the server runs for the session's thread that outlives the
    // shell turn starting it, as a backgrounded one does; its pid lands in
    // `pid_file`.
    let run_in_background = |session: &mut PooledSession, pid_file: &str| -> u32 {
        session.open_thread(&cwd, None).unwrap();
        session
            .shell_turn(&format!(
                "sleep 60 >/dev/null 2>&1 & echo $! > '{pid_file}'"
            ))
            .unwrap();
        let mut pid = None;
        wait_for("the command to start", Duration::from_secs(10), || {
            pid = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|text| text.trim().parse().ok());
            pid.is_some()
        });
        pid.unwrap()
    };

    let mut keeper = PooledSession::open(plan(&[])).unwrap();
    let orphaned = run_in_background(&mut keeper, &pool.file("orphaned.pid"));
    // A CODEX_ variable keys the other dispatch to a server of its own.
    let mut neighbour =
        PooledSession::open(plan(&[("CODEX_OH_NEIGHBOUR".into(), "1".into())])).unwrap();
    let dead = keeper.record().pid.get();
    assert_ne!(
        neighbour.record().pid.get(),
        dead,
        "the neighbour shares the server"
    );
    let neighbouring = run_in_background(&mut neighbour, &pool.file("neighbour.pid"));
    sigkill(dead);
    wait_for("the server to exit", Duration::from_secs(10), || {
        exited_unreaped(dead)
    });
    assert!(alive(orphaned), "the command died with its server");

    let session = PooledSession::open(plan(&[])).unwrap();
    assert_ne!(
        session.record().pid.get(),
        dead,
        "the dead server was leased again"
    );
    wait_for(
        "the dead server's command for the live session to stop",
        DEAD_LEASE_STOP_DEADLINE + Duration::from_secs(5),
        || !alive(orphaned),
    );
    assert!(
        alive(neighbouring),
        "another dispatch's command was stopped"
    );
    keeper.release();
    let servers = [session.record().pid.get(), neighbour.record().pid.get()];
    session.release();
    neighbour.release();
    assert!(
        !alive(neighbouring),
        "a released session's command still runs"
    );
    reclaim_lingering_servers();
    for server in servers {
        assert!(!alive(server), "the swept server {server} is still running");
    }
}

/// Frames for a dispatch's thread that arrive before the reply naming the
/// thread are held and replayed once the dispatch knows it, so they reach its
/// report; a frame for another thread held the same way never does.
#[test]
fn frames_before_the_thread_is_named_are_replayed_only_for_the_dispatch_s_own_thread() {
    let pool = Pool::new("pool-early").with_env("CODEX_MOCK_FAULT", "early-frames");
    let (output, report) = pool.run(pool.command("early", &[], &[]));
    succeeded(&output, &report);
    let thread = thread_of(&report);
    let stdout = result(&report)["stdout"].as_str().unwrap().to_string();
    let early: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|frame| frame["params"]["early"].is_string())
        .collect();
    assert_eq!(early.len(), 1, "{stdout}");
    assert_eq!(early[0]["params"]["early"], "own", "{stdout}");
    assert_eq!(early[0]["params"]["threadId"], thread.as_str(), "{stdout}");
    assert!(!stdout.contains("mock-thread-foreign"), "{stdout}");

    // Past the buffer's bound, the earliest frames are kept and the rest
    // dropped — the dispatch neither grows without bound nor fails.
    use oneharness_core::io::app_server::PRE_THREAD_BUFFER;
    let flood = (PRE_THREAD_BUFFER + 5).to_string();
    let (output, report) = pool.run(pool.command(
        "flooded",
        &[],
        &[("CODEX_MOCK_EARLY_FRAMES", flood.as_str())],
    ));
    succeeded(&output, &report);
    let kept: Vec<u64> = result(&report)["stdout"]
        .as_str()
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|frame| frame["params"]["early"] == "own")
        .map(|frame| frame["params"]["n"].as_u64().unwrap())
        .collect();
    assert_eq!(kept, (0..PRE_THREAD_BUFFER as u64).collect::<Vec<_>>());
}

/// The one pool entry `pool` holds, and the pid its sweeper recorded.
fn entry_and_sweeper(pool: &Pool) -> (PathBuf, u32) {
    let entries: Vec<PathBuf> = std::fs::read_dir(pool.path("state/oneharness/servers"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let sweeper: Value = serde_json::from_str(
        &std::fs::read_to_string(entries[0].join("sweeper.json")).expect("a recorded sweeper"),
    )
    .unwrap();
    (entries[0].clone(), sweeper["pid"].as_u64().unwrap() as u32)
}

/// A pool entry removed from under a live server leaves its sweeper the only
/// one who knows the server: it stops it, by the marker it last saw, and ends.
#[test]
fn a_server_whose_pool_entry_is_removed_is_stopped_by_its_sweeper() {
    let pool = Pool::new("pool-entry-gone");
    let (output, report) = pool.run(pool.command(
        "orphaned",
        &[],
        &[("ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS", "60000")],
    ));
    succeeded(&output, &report);
    let server = reported_server(&report);
    let (entry, sweeper) = entry_and_sweeper(&pool);
    assert!(alive(server) && alive(sweeper));
    // llmlint: ignore[tests_mirror_real_usage] No oneharness interface removes a pool entry: the condition this path handles is an outside actor deleting the state directory under a live server — a user clearing ~/.local/state, a tmp reaper — so the journey stages exactly that act, and asserts only through what a user sees (the processes).
    std::fs::remove_dir_all(&entry).unwrap();
    use oneharness_core::io::server_pool::SERVER_STOP_DEADLINE;
    let bound = DEAD_LEASE_SWEEP_INTERVAL * 2 + SERVER_STOP_DEADLINE + Duration::from_secs(5);
    wait_for("the orphaned server to stop", bound, || !alive(server));
    wait_for("its sweeper to end", bound, || !alive(sweeper));
}

/// A server replaced under a live sweeper — its predecessor died and the next
/// dispatch started another — is adopted by that sweeper rather than given a
/// second one, and reclaimed by it once its linger passes.
#[test]
fn a_sweeper_adopts_the_server_that_replaced_its_own_and_reclaims_it() {
    let pool = Pool::new("pool-adopt");
    let linger = [("ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS", "3000")];
    let (output, report) = pool.run(pool.command("first", &[], &linger));
    succeeded(&output, &report);
    let first = reported_server(&report);
    let (entry, sweeper) = entry_and_sweeper(&pool);
    sigkill(first);
    wait_for("the first server to die", Duration::from_secs(10), || {
        !alive(first)
    });
    let (output, report) = pool.run(pool.command("second", &[], &linger));
    succeeded(&output, &report);
    let second = reported_server(&report);
    assert_ne!(second, first);
    assert_eq!(
        entry_and_sweeper(&pool),
        (entry, sweeper),
        "the replacement got a sweeper of its own"
    );
    assert!(alive(sweeper));
    let bound = Duration::from_secs(3) + DEAD_LEASE_SWEEP_INTERVAL * 3 + Duration::from_secs(10);
    wait_for("the adopted server to be reclaimed", bound, || {
        !alive(second)
    });
    wait_for("the sweeper to end", bound, || !alive(sweeper));
}

/// A caller reading a dispatch's output to its end is never held by the shared
/// server the dispatch started. The dispatch here also holds its standard
/// output and error at descriptors 3 and 4, not close-on-exec — as a launcher
/// that passed them on leaves them, and as a pipe another thread had just made
/// is on macOS, where `std` marks one close-on-exec only after creating it. The
/// server and the sweeper it starts outlive the dispatch, and must hold
/// neither: both pipes end with the dispatch, while they run on. Then each
/// stops on its own, and nothing this journey started is left running.
#[test]
fn a_shared_server_holds_none_of_its_starting_dispatch_s_output_open() {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let linger = Duration::from_secs(5);
    let pool = Pool::new("pool-output-held");
    let mut cmd = pool.command(
        "held-output",
        &[],
        &[(
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
            &linger.as_millis().to_string(),
        )],
    );
    // SAFETY: the hook runs in the forked child before exec, after its
    // standard streams are in place, and calls only `dup2`, which is
    // async-signal-safe; the copies it makes are not close-on-exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::dup2(1, 3) == -1 || libc::dup2(2, 4) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut dispatch = pool.spawn(cmd);
    let ended = |mut pipe: Box<dyn Read + Send>| {
        let (sender, ends) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = sender.send(bytes);
        });
        ends
    };
    let stdout = ended(Box::new(dispatch.stdout.take().unwrap()));
    let stderr = ended(Box::new(dispatch.stderr.take().unwrap()));
    let status = dispatch.wait().unwrap();
    let bound = Duration::from_secs(2);
    let stdout = stdout
        .recv_timeout(bound)
        .expect("the dispatch's standard output is held open past its exit");
    let stderr = stderr
        .recv_timeout(bound)
        .expect("the dispatch's standard error is held open past its exit");
    let report: Value = serde_json::from_slice(&stdout).unwrap_or_else(|err| {
        panic!(
            "no JSON report ({err}): {}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )
    });
    let output = Output {
        status,
        stdout,
        stderr,
    };
    succeeded(&output, &report);
    let server = reported_server(&report);
    let (_, sweeper) = entry_and_sweeper(&pool);
    assert!(
        alive(server) && alive(sweeper),
        "the shared server and its sweeper did not outlive the dispatch, so nothing was proven"
    );
    let stop = linger + DEAD_LEASE_SWEEP_INTERVAL * 3 + Duration::from_secs(10);
    wait_for("the lingered-out server to stop", stop, || !alive(server));
    wait_for("its sweeper to end", stop, || !alive(sweeper));
    let left: Vec<u32> = std::iter::once(dispatch.id())
        .chain(pool.server_pids())
        .filter(|pid| alive(*pid))
        .collect();
    assert!(left.is_empty(), "still running: {left:?}");
}

/// The contract a caller of the shared server can rely on when it ends without
/// warning. A dispatch holding its standard output and error at descriptors 3
/// and 4 as well — not close-on-exec, as a launcher that passed them on leaves
/// them — starts the shared server and its sweeper, runs a command on it, and
/// is SIGKILLed mid-turn. Its caller's pipes reach end-of-file with it: the
/// server and the sweeper hold neither. The sweeper then finds the dead lease
/// and stops the command, the server lingers out and is stopped, the sweeper
/// ends, and no process this journey started is left running.
#[test]
fn a_killed_caller_s_output_ends_with_it_and_nothing_it_started_outlives_the_journey() {
    use std::os::unix::process::CommandExt;
    let pool = Pool::new("pool-killed-caller");
    let beat = pool.file("heartbeat");
    let never = pool.file("never");
    let mut cmd = pool.command(
        "killed-caller",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    );
    // SAFETY: the hook runs in the forked child before exec, after its
    // standard streams are in place, and calls only `dup2`, which is
    // async-signal-safe; the copies it makes are not close-on-exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::dup2(1, 3) == -1 || libc::dup2(2, 4) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut caller = pool.spawn(cmd);
    let stdout = read_to_end(caller.stdout.take().unwrap());
    let stderr = read_to_end(caller.stderr.take().unwrap());
    let server = pool.await_live_turns(1);
    wait_for("the heartbeat to start", Duration::from_secs(10), || {
        lines(&beat) >= 3
    });
    let command = command_running(&pool, server, &beat)["pid"]
        .as_u64()
        .unwrap() as u32;
    let (_, sweeper) = entry_and_sweeper(&pool);

    sigkill(caller.id());
    caller.wait().unwrap();
    let bound = Duration::from_secs(2);
    stdout
        .recv_timeout(bound)
        .expect("the killed caller's standard output is held open past its end");
    stderr
        .recv_timeout(bound)
        .expect("the killed caller's standard error is held open past its end");
    assert!(
        alive(server) && alive(sweeper),
        "the shared server and its sweeper did not outlive the caller, so nothing was proven"
    );

    let retire = DEAD_LEASE_SWEEP_INTERVAL + DEAD_LEASE_STOP_DEADLINE;
    wait_for("the dead caller's command to stop", retire * 3, || {
        !alive(command)
    });
    let stop = Duration::from_millis(LINGER_MS) + retire + Duration::from_secs(10);
    wait_for("the lingered-out server to stop", stop, || !alive(server));
    wait_for("its sweeper to end", stop, || !alive(sweeper));
    let left: Vec<u32> = [caller.id(), server, sweeper, command]
        .into_iter()
        .chain(pool.server_pids())
        .filter(|pid| alive(*pid))
        .collect();
    assert!(left.is_empty(), "still running: {left:?}");
}

/// A caller killed while a dispatch is still asking Codex its version —
/// before any server exists — leaves nothing behind either. Two probes run
/// first: the shared model catalog's, and the shared server's own (the first
/// once the catalog is off). Each case stalls the first and says which it
/// stalled by where it runs: the server's probe runs in the pool's root. The
/// stalled probe never answers and has started a descendant; the dispatch
/// holds its standard output and error at descriptors 3 and 4 as well, not
/// close-on-exec. Once it is SIGKILLed nothing it left is there to end the
/// probe by its deadline, so the probe must not need it: the caller's pipes
/// reach end-of-file, and the probe, its descendant and everything in the
/// probe's process group end.
#[test]
fn a_caller_killed_mid_version_probe_leaves_no_probe_and_its_output_ends() {
    let pool = Pool::new("pool-killed-probe");
    let servers = pool.path("state/oneharness/servers");
    for (case, catalog_off) in [("catalog", false), ("server", true)] {
        let stalled = pool.file(&format!("{case}-version-stall"));
        let envs = [("CODEX_MOCK_VERSION_STALL", stalled.as_str())];
        let cmd = if catalog_off {
            pool.command_with_config(
                case,
                "[harness.codex]\nshared_model_catalog = false\n",
                &[],
                &envs,
            )
        } else {
            pool.command(case, &[], &envs)
        };
        let cwd = kill_the_caller_mid_probe(&pool, cmd, Path::new(&stalled), case);
        // The pool's root exists once the server's probe is asked, not before.
        let in_pool = std::fs::canonicalize(&servers)
            .is_ok_and(|root| std::fs::canonicalize(&cwd).is_ok_and(|cwd| cwd == root));
        assert_eq!(
            in_pool, catalog_off,
            "{case}: stalled the probe running in {cwd}, not the {case}'s"
        );
    }
}

/// A caller killed while the shared model catalog's refresh helper — a
/// `codex app-server --stdio` the dispatch starts before any shared server —
/// is wedged leaves nothing behind either. The helper never fetches and never
/// exits, its stdin closed or not, and has started a descendant; the dispatch
/// holds its output at descriptors 3 and 4 as well, not close-on-exec. Once
/// the caller is SIGKILLed its refresh deadline is gone with it, so the
/// helper must not need it: the caller's pipes reach end-of-file, and the
/// helper, its descendant and everything in its process group end.
#[test]
fn a_caller_killed_mid_catalog_refresh_leaves_no_helper_and_its_output_ends() {
    let pool = catalog_pool("pool-killed-refresh");
    let stalled = pool.file("refresh-stall");
    let cmd = pool.command(
        "refresh",
        &[],
        &[("CODEX_MOCK_CATALOG_STALL", stalled.as_str())],
    );
    kill_the_caller_mid_probe(&pool, cmd, Path::new(&stalled), "refresh");
    assert_eq!(
        pool.server_pids(),
        Vec::<u32>::new(),
        "the caller was killed before any shared server started"
    );
}

/// Run `cmd` — a dispatch whose first `--version` (or catalog refresh)
/// stalls, writing to `stalled` — with its output also at descriptors 3 and
/// 4, SIGKILL it once the launch stalls, and hold everything it left to the
/// bound. Returns the directory the stalled launch ran in.
fn kill_the_caller_mid_probe(pool: &Pool, mut cmd: Command, stalled: &Path, case: &str) -> String {
    use std::os::unix::process::CommandExt;

    /// The caller and the probe's processes, SIGKILLed on the way out should
    /// the journey fail with them still running: pids this journey captured,
    /// never more.
    struct Captured(Vec<u32>);
    impl Drop for Captured {
        fn drop(&mut self) {
            for pid in &self.0 {
                if alive(*pid) {
                    sigkill(*pid);
                }
            }
        }
    }

    // SAFETY: the hook runs in the forked child before exec, after its
    // standard streams are in place, and calls only `dup2`, which is
    // async-signal-safe; the copies it makes are not close-on-exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::dup2(1, 3) == -1 || libc::dup2(2, 4) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut caller = pool.spawn(cmd);
    let mut captured = Captured(vec![caller.id()]);
    let stdout = read_to_end(caller.stdout.take().unwrap());
    let stderr = read_to_end(caller.stderr.take().unwrap());
    wait_for(
        &format!("{case}: its first launch to stall"),
        Duration::from_secs(10),
        || stalled.exists(),
    );
    let written = std::fs::read_to_string(stalled).unwrap();
    let lines: Vec<&str> = written.lines().collect();
    let [probe, descendant, cwd] = lines[..] else {
        panic!("{case}: the stalled launch wrote {written:?}, not two pids and a directory");
    };
    let (probe, descendant): (u32, u32) = (probe.parse().unwrap(), descendant.parse().unwrap());
    captured.0.extend([probe, descendant]);
    // SAFETY: `getpgid` only reads the process table.
    let group = unsafe { libc::getpgid(probe as libc::pid_t) };
    assert!(group > 0, "{case}: {}", std::io::Error::last_os_error());

    sigkill(caller.id());
    caller.wait().unwrap();
    let bound = Duration::from_secs(5);
    stdout.recv_timeout(bound).unwrap_or_else(|_| {
        panic!("{case}: the killed caller's standard output is held open past its end")
    });
    stderr.recv_timeout(bound).unwrap_or_else(|_| {
        panic!("{case}: the killed caller's standard error is held open past its end")
    });
    // An exited member nobody has reaped yet still answers for its group, so
    // the group is given the same bound to be collected.
    wait_for(
        &format!("{case}: the probe's process group to end"),
        bound,
        || {
            // SAFETY: signal 0 to a group delivers nothing.
            !alive(probe) && !alive(descendant) && unsafe { libc::kill(-group, 0) } == -1
        },
    );
    drop(captured);
    cwd.to_string()
}

// llmlint: ignore-block[e2e_not_mocked] The reuse workload and its pooled server run for real through oneharness-core's pool; only codex is the repository's subprocess double, because real Codex cannot be made to end a shell turn before replying on demand. Real Codex's shell turn is driven by scripts/e2e-control.sh's pooled-reuse phase.
/// A shell turn that completes before the server replies to the request that
/// ran it still completes: the reuse workload runs to its end.
#[test]
fn a_shell_turn_completed_before_its_reply_still_completes() {
    let pool = Pool::new("pool-shell-first");
    let output = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "5000")
        .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_FAULT", "shell-completes-first")
        .env(
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
            LINGER_MS.to_string(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reuse: Value =
        serde_json::from_str(&std::fs::read_to_string(pool.path("reuse.json")).unwrap()).unwrap();
    assert_eq!(reuse["lease_count"], 5);
}
// llmlint: ignore-end[e2e_not_mocked]

/// Each way a pooled session's thread open, shell turn or loaded-thread
/// listing can fail fails the reuse workload naming it, writing no telemetry:
/// a refused, threadless or unusable `thread/start`; a refused, unanswered,
/// never-completing, failed or hung-up shell turn; and a refused listing, one
/// naming a thread that is no id, or a cursor that is no string. The cases run
/// side by side, two of them waiting out a 30 s bound.
#[test]
fn the_reuse_workload_names_each_shell_turn_and_listing_failure() {
    let cases = [
        ("refuse-thread-start", "thread/start was refused: "),
        ("thread-start-no-thread", "thread/start named no thread: "),
        (
            "thread-start-bad-id",
            "thread/start named an unusable thread id \"\"",
        ),
        (
            "refuse-shell",
            "the app-server's thread/shellCommand failed: it refused thread/shellCommand: ",
        ),
        (
            "silent-shell",
            "the app-server did not answer thread/shellCommand",
        ),
        (
            "shell-no-completion",
            "the shell turn did not complete within 30s",
        ),
        (
            "shell-hangup",
            "thread/shellCommand: the shared codex app-server closed the connection",
        ),
        ("fail-shell", "the shell turn ended failed, not completed"),
        ("refuse-loaded", "thread/loaded/list was refused: "),
        (
            "loaded-bad-id",
            "thread/loaded/list named a thread 7 that is no id",
        ),
        (
            "loaded-cursor-number",
            "thread/loaded/list answered a cursor 2 that is no string",
        ),
    ];
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(fault, said)| {
            std::thread::spawn(move || {
                let pool = Pool::new(&format!("pool-reuse-{fault}"));
                let output = Command::new("bash")
                    .arg(repo_root().join("live/control/app-server-reuse.sh"))
                    .env("OH_REUSE_CODEX", mock_bin())
                    .env("OH_REUSE_SETTLE_MS", "1000")
                    .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
                    .env("XDG_STATE_HOME", pool.path("state"))
                    .env("CODEX_MOCK_DIR", pool.path("mock"))
                    .env("CODEX_MOCK_FAULT", fault)
                    .env(
                        "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
                        LINGER_MS.to_string(),
                    )
                    .output()
                    .unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                let wrote = pool.path("reuse.json").exists();
                (fault, said, output.status.success(), stderr, wrote)
            })
        })
        .collect();
    for run in runs {
        let (fault, said, ok, stderr, wrote) = run.join().unwrap();
        assert!(!ok, "{fault}: {stderr}");
        assert!(stderr.contains(said), "{fault}: no `{said}` in: {stderr}");
        assert!(!wrote, "{fault}: telemetry was written");
    }
}

/// A variant's `app_server` beats its harness's: under `[harness.codex]
/// app_server = "per-dispatch"`, the variant that says `shared` runs its turn
/// on the pool while the harness itself keeps its own child.
#[test]
fn a_variant_s_app_server_setting_beats_its_harness_s() {
    let pool = Pool::new("pool-variant");
    let config = "[harness.codex]\n\
                  app_server = \"per-dispatch\"\n\
                  [harness.codex.variant.pooled]\n\
                  app_server = \"shared\"\n";
    let log = pool.file("own-child.log");
    let (output, own) = pool.run(pool.command_with_config(
        "own",
        config,
        &[],
        &[("MOCK_CODEX_APP_SERVER_LOG", &log)],
    ));
    succeeded(&output, &own);
    assert!(own["control"]["app_server"].is_null(), "{own}");
    assert!(
        pool.server_pids().is_empty(),
        "the harness's own run used the pool"
    );
    let (output, pooled) =
        pool.run(pool.command_with_config("pooled", config, &["--harness", "codex:pooled"], &[]));
    succeeded(&output, &pooled);
    let server = reported_server(&pooled);
    assert_eq!(pool.server_pids().len(), 1);
    assert!(
        pool.state(server)["turns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn["thread"] == thread_of(&pooled).as_str()),
        "the variant's turn did not run on the shared server"
    );
}

/// The sweeper hook refuses an entry that is not a pool entry under the pool's
/// root — it reconciles and stops servers — exiting 2 and naming why, and
/// touches nothing there.
#[test]
fn the_sweeper_refuses_a_path_that_is_no_pool_entry() {
    let pool = Pool::new("pool-sweeper-path");
    let stranger = pool.path("not-a-pool/codex-app-server-0123");
    std::fs::create_dir_all(&stranger).unwrap();
    std::fs::write(stranger.join("keep"), "x").unwrap();
    std::fs::create_dir_all(pool.path("state/oneharness/servers")).unwrap();
    // Named like an entry, under the root, and leading out of the pool.
    let link = pool.path("state/oneharness/servers/codex-app-server-4567");
    std::os::unix::fs::symlink(&stranger, &link).unwrap();
    for (entry, why) in [
        (stranger.clone(), "it is not a directory under "),
        (link.clone(), "it is not a directory under "),
        (
            pool.path("state/oneharness/servers/../.."),
            "its name is no pool key",
        ),
    ] {
        let output = Command::new(oneharness_bin())
            .env("ONEHARNESS_INTERNAL_APP_SERVER_SWEEPER", &entry)
            .env("XDG_STATE_HOME", pool.path("state"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert!(
            stderr.contains("is not a shared app-server pool entry: ") && stderr.contains(why),
            "{stderr}"
        );
    }
    assert!(stranger.join("keep").exists());
}

/// Under the parallel opt-in, a zero-work overload on a pooled turn repeats it
/// on the shared server too — the one controlled candidate drives itself — and
/// recovers within `server_overloaded_max_retries`.
#[test]
fn a_pooled_overload_retries_on_the_shared_server_under_the_parallel_opt_in() {
    let pool = Pool::new("pool-overload-parallel");
    let count = pool.file("attempts");
    let (output, report) = pool.run(pool.command_with_config(
        "parallel",
        "run_mode = \"parallel\"\nserver_overloaded_max_retries = 1\n",
        &[],
        &[
            ("MOCK_CODEX_OVERLOAD_ATTEMPTS", "1"),
            ("MOCK_ATTEMPT_FILE", &count),
        ],
    ));
    succeeded(&output, &report);
    assert!(report["fallback"].is_null(), "{report}");
    let ran = result(&report);
    assert!(ran["failure_kind"].is_null(), "{ran}");
    assert_eq!(std::fs::read_to_string(&count).unwrap(), "2");
    let statuses: Vec<Value> = pool.state(reported_server(&report))["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].clone())
        .collect();
    assert_eq!(statuses, vec![json!("failed"), json!("completed")]);
}

/// Frames whose thread field is no thread id — a number, a null — belong to no
/// dispatch: none of them reaches the run's output or its answer.
#[test]
fn frames_with_a_malformed_thread_field_reach_no_dispatch() {
    let pool = Pool::new("pool-malformed").with_env("CODEX_MOCK_FAULT", "malformed-thread-frames");
    let (output, report) = pool.run(pool.command("guarded", &["--events"], &[]));
    succeeded(&output, &report);
    let ran = result(&report);
    let seen = ran.to_string();
    assert!(!seen.contains("SMUGGLED"), "{seen}");
    assert_eq!(ran["text"], "work for guarded", "{ran}");
}

/// A variable whose value is not UTF-8 cannot travel in a thread's JSON
/// policy: the pooled dispatch says so, naming it, and runs without it.
#[test]
fn a_variable_that_is_not_utf8_is_named_and_left_out_of_a_pooled_thread() {
    use std::os::unix::ffi::OsStringExt;
    let pool = Pool::new("pool-not-utf8");
    let mut cmd = pool.command(
        "bytes",
        &["--mode", "bypass", "--events"],
        &[("MOCK_CODEX_RUN_COMMAND", "env")],
    );
    cmd.env(
        "OH_NOT_UTF8",
        std::ffi::OsString::from_vec(vec![0x66, 0xff, 0x6f]),
    );
    let (output, report) = pool.run(cmd);
    succeeded(&output, &report);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the variable OH_NOT_UTF8 is not UTF-8, so it cannot travel to a shared codex app-server's thread"),
        "{stderr}"
    );
    let env = command_output(&report);
    assert!(
        env.lines().any(|line| line.starts_with("PATH=")),
        "the command ran no env: {env}"
    );
    assert!(
        !env.lines().any(|line| line.starts_with("OH_NOT_UTF8=")),
        "{env}"
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

/// A client of the shared server that connects, upgrades, and then never
/// reads again — a dispatch frozen mid-turn, or anything else dialling the
/// socket — must cost every other dispatch nothing. The server broadcasts each
/// thread's frames to every connection, so this one's buffer fills: a server
/// writing to it under its one lock would stall every other dispatch for as
/// long as it stays connected. The mock bounds each write and ends a
/// connection that stops reading; the dispatch held live across the stall and
/// one started into it both finish, the stalled connection reads its end, and
/// every process the journey captured — both dispatches, the server and its
/// sweeper — is gone once the server lingers out.
#[test]
fn a_client_that_stops_reading_stalls_no_other_dispatch_and_is_ended() {
    use std::io::Read;
    let pool = Pool::new("pool-stalled-reader").with_env("CODEX_MOCK_BROADCAST", "1");
    let release = pool.file("release");
    let mut live = pool.spawn(pool.command(
        "live",
        &[],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &release),
            ("MOCK_CODEX_TICK_MS", "10"),
        ],
    ));
    // Read as they arrive: a report carrying hundreds of ticks outgrows a
    // pipe nobody is reading.
    let live_out = read_to_end(live.stdout.take().unwrap());
    let live_err = read_to_end(live.stderr.take().unwrap());
    let server = pool.await_live_turns(1);
    let (_, sweeper) = entry_and_sweeper(&pool);
    let listen = pool.mock_servers()[0]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find_map(|arg| arg.strip_prefix("unix://"))
        .map(PathBuf::from)
        .expect("the server's --listen path");

    let mut stalled =
        UnixStream::connect(oneharness_core::io::websocket::dial_path(&listen)).unwrap();
    // Bounded now, while the server still holds its end: on Darwin
    // `setsockopt` on a socket whose peer closed fails with EINVAL, and the
    // server is expected to end this connection before it is read again.
    stalled
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stalled
        .write_all(
            b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        )
        .unwrap();
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while !answer.ends_with(b"\r\n\r\n") {
        assert_eq!(
            stalled.read(&mut byte).unwrap(),
            1,
            "the upgrade went unanswered"
        );
        answer.push(byte[0]);
    }
    assert!(answer.starts_with(b"HTTP/1.1 101"), "{answer:?}");
    // Every frame the server sends the stalled connection, as the mock
    // records it. A server that writes under its lock stops sending once the
    // buffer is full — a few hundred ticks on Linux, far fewer on macOS — and
    // never reaches this many; one that does not stall goes on queueing.
    // The connection registered after the live dispatch's own.
    let stalled_conn = || {
        let conns: Vec<u64> = pool.state(server)["connections"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        (conns.len() == 2)
            .then(|| conns.into_iter().max())
            .flatten()
    };
    wait_for(
        "the stalled connection to register",
        Duration::from_secs(10),
        || stalled_conn().is_some_and(|conn| !pool.sent(server, conn).is_empty()),
    );
    let conn = stalled_conn().unwrap();
    let flooded = Instant::now() + Duration::from_secs(20);
    while pool.sent(server, conn).len() < 1000 && Instant::now() < flooded {
        std::thread::sleep(Duration::from_millis(100));
    }

    let mut joining = pool.spawn(pool.command("joining", &[], &[]));
    let captured = [live.id(), joining.id(), server, sweeper];
    let joining_out = read_to_end(joining.stdout.take().unwrap());
    let joining_err = read_to_end(joining.stderr.take().unwrap());
    let bound = Duration::from_secs(40);
    let joined = exits_within(
        &mut joining,
        bound,
        "a dispatch started while another client stopped reading",
    );
    std::fs::write(&release, b"").unwrap();
    let lived = exits_within(
        &mut live,
        bound,
        "a dispatch held live while another client stopped reading",
    );
    for (status, out, err, name) in [
        (joined, joining_out, joining_err, "joining"),
        (lived, live_out, live_err, "live"),
    ] {
        let output = Output {
            status,
            stdout: out
                .recv_timeout(bound)
                .expect("the dispatch's output ended"),
            stderr: err
                .recv_timeout(bound)
                .expect("the dispatch's errors ended"),
        };
        let report: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|err| panic!("{name}: no JSON report ({err})"));
        succeeded(&output, &report);
        assert_eq!(reported_server(&report), server, "{name}");
    }

    // The server ended the connection that stopped reading: what it sent
    // before the bound, then the end.
    let mut rest = Vec::new();
    stalled
        .read_to_end(&mut rest)
        .expect("the stalled connection was never ended");

    let gone =
        Duration::from_millis(LINGER_MS) + DEAD_LEASE_SWEEP_INTERVAL * 3 + Duration::from_secs(15);
    wait_for("the lingered-out server to stop", gone, || !alive(server));
    wait_for("its sweeper to end", gone, || !alive(sweeper));
    let left: Vec<u32> = captured
        .into_iter()
        .chain(pool.server_pids())
        .filter(|pid| alive(*pid))
        .collect();
    assert!(left.is_empty(), "still running: {left:?}");
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

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control --session` through the real pool; only codex is the repository's subprocess double, held live so both creations share one server, which the gate cannot do with real Codex. Real Codex's continued controlled session is held by scripts/e2e-control.sh's `oh_control_session_enforce` phase.
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
    // Each thread is created under the other dispatch's settings, both held
    // live together so they are certainly on one server: one run after the
    // other passed only if the second beat the linger, which on macOS under
    // coverage it did not.
    let command = "env; pwd";
    let created = pool.file("created");
    let making_a = spawn_with(
        &pool,
        &b("res-a"),
        &[
            ("PER_DISPATCH", "creator-of-a"),
            ("MOCK_CODEX_RUN_COMMAND", command),
            ("MOCK_CODEX_HOLD_UNTIL", &created),
        ],
    );
    pool.await_live_turns(1);
    let making_b = spawn_with(
        &pool,
        &a("res-b"),
        &[
            ("PER_DISPATCH", "creator-of-b"),
            ("MOCK_CODEX_RUN_COMMAND", command),
            ("MOCK_CODEX_HOLD_UNTIL", &created),
        ],
    );
    pool.await_live_turns(2);
    std::fs::write(&created, b"").unwrap();
    let (out, made_a) = Pool::finish(making_a);
    succeeded(&out, &made_a);
    let (out, made_b) = Pool::finish(making_b);
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
// llmlint: ignore-end[e2e_not_mocked]

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

    let live_answer = once_the_turn_is_known(|| interrupt(&pool, "live").0, |a| a);
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

/// The repository root, for the journeys that run its scripts.
fn repo_root() -> PathBuf {
    std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap()
}

/// `pid` and every descendant of it, as the OS lists them now (`/proc`).
#[cfg(target_os = "linux")]
fn os_process_tree(root: u32) -> Vec<u64> {
    descendants(root, &linux_parents())
}

/// Every process's (pid, parent pid), from `/proc`.
#[cfg(target_os = "linux")]
fn linux_parents() -> Vec<(u64, u64)> {
    std::fs::read_dir("/proc")
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
        .collect()
}

/// `pid` and every descendant of it, as the OS lists them now: libproc's
/// listing of every pid and each one's BSD info, not the `ps` listing the
/// workload under test reads.
#[cfg(target_os = "macos")]
fn os_process_tree(root: u32) -> Vec<u64> {
    let mut pids = vec![0 as libc::c_int; 1 << 16];
    let bytes = libc::c_int::try_from(std::mem::size_of_val(pids.as_slice())).unwrap();
    // SAFETY: `pids` is writable for `bytes` bytes.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    assert!(listed > 0, "{}", std::io::Error::last_os_error());
    pids.truncate(usize::try_from(listed).unwrap());
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).unwrap();
    let parents: Vec<(u64, u64)> = pids
        .into_iter()
        .filter_map(|pid| {
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
            // SAFETY: `info` is writable for `size` bytes, the size of the
            // structure PROC_PIDTBSDINFO fills.
            let filled = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            // SAFETY: the kernel filled all `size` bytes of the structure.
            (filled == size).then(|| {
                (
                    u64::try_from(pid).unwrap(),
                    u64::from(unsafe { info.assume_init() }.pbi_ppid),
                )
            })
        })
        .collect();
    descendants(root, &parents)
}

/// `root` and every pid descending from it through `parents` (pid, parent
/// pid), sorted.
fn descendants(root: u32, parents: &[(u64, u64)]) -> Vec<u64> {
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
/// A measured codex behind a launcher — the native server its child — leaves
/// neither process behind once the measurement ends.
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_stops_a_launcher_and_its_child() {
    let pool = Pool::new("pool-measure-shim");
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
        .env("OH_MEASURE_CODEX", mock_bin())
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_SHIM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let measured: Value = serde_json::from_slice(&output.stdout).unwrap();
    let launcher = measured["server_pid"].as_u64().unwrap() as u32;
    let child = pool.the_server();
    assert_ne!(
        child, launcher,
        "the measured server was not behind a launcher"
    );
    for pid in [launcher, child] {
        wait_for(
            "the measured processes to stop",
            Duration::from_secs(5),
            || !alive(pid),
        );
    }
}

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

/// A stand-in `codex` for the sharing measurement's own WebSocket client: it
/// answers `--version`, and as `app-server --listen` it misbehaves as
/// `FAKE_MODE` says — in the upgrade, or with one bad frame once the client
/// has sent its first message.
#[cfg(target_os = "linux")]
const MISBEHAVING_CODEX: &str = r#"#!/usr/bin/env python3
import base64, hashlib, os, socket, struct, sys, time

args = sys.argv[1:]
if args == ["--version"]:
    print("codex-cli 0.0.0-fake")
    sys.exit(0)
path = args[args.index("--listen") + 1][len("unix://"):]
mode = os.environ["FAKE_MODE"]
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(path)
server.listen(4)
conn, _ = server.accept()
request = b""
while not request.endswith(b"\r\n\r\n"):
    request += conn.recv(1)
key = [l.split(b":", 1)[1].strip() for l in request.split(b"\r\n") if l.lower().startswith(b"sec-websocket-key")][0]
accept = base64.b64encode(hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
if mode == "close-upgrade":
    conn.close()
    sys.exit(0)
if mode == "bad-accept":
    accept = b"wrong"
padding = b"X-Padding: " + b"p" * 20000 + b"\r\n" if mode == "huge-upgrade" else b""
conn.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n" + padding + b"\r\n")
if mode not in ("bad-accept", "huge-upgrade"):
    conn.recv(65536)
    bad = {
        "reserved-bits": b"\xc1\x01x",
        "masked": b"\x81\x81\x00\x00\x00\x00x",
        "bad-opcode": b"\x83\x01x",
        "nonminimal": b"\x81\x7e\x00\x05hello",
        "fragmented-control": b"\x09\x00",
        "oversized": b"\x81\x7f" + struct.pack(">Q", 65 * 1024 * 1024),
        "bad-close": b"\x88\x02\x03\xed",
        "interleaved": b"\x01\x01{\x81\x01}",
        "not-json": b"\x81\x03abc",
        "utf16": b"\x81" + bytes([len("{}".encode("utf-16"))]) + "{}".encode("utf-16"),
    }[mode]
    conn.sendall(bad)
conn.settimeout(30)
try:
    while conn.recv(65536):
        pass
except OSError:
    pass
"#;

/// The sharing measurement's own WebSocket client refuses each way a server can
/// break the protocol — in the upgrade or in a frame — naming it, printing no
/// figure, and leaving no scratch behind.
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_refuses_each_protocol_violation_naming_it() {
    use std::os::unix::fs::PermissionsExt;
    let pool = Pool::new("pool-measure-protocol");
    let fake = pool.path("codex");
    std::fs::write(&fake, MISBEHAVING_CODEX).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let frame = "a frame a WebSocket server may not";
    let limit = "past 64 MiB or a fragmented control frame";
    for (mode, said) in [
        (
            "bad-accept",
            "refused or mis-answered the WebSocket upgrade",
        ),
        ("huge-upgrade", "upgrade answer passed 16384 bytes"),
        ("close-upgrade", "closed the WebSocket handshake"),
        ("reserved-bits", frame),
        ("masked", frame),
        ("bad-opcode", frame),
        ("nonminimal", "not in its shortest encoding"),
        ("fragmented-control", limit),
        ("oversized", limit),
        ("bad-close", "malformed close frame"),
        ("interleaved", "interleaved two messages' frames"),
        ("not-json", "not UTF-8 JSON"),
        ("utf16", "not UTF-8 JSON"),
    ] {
        let tmp = pool.path(&format!("{mode}-tmp"));
        std::fs::create_dir_all(&tmp).unwrap();
        let output = Command::new("bash")
            .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
            .env("OH_MEASURE_CODEX", &fake)
            .env("OH_MEASURE_SOCKET_TIMEOUT", "5")
            .env("TMPDIR", &tmp)
            .env("FAKE_MODE", mode)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{mode}: {stderr}");
        assert!(output.stdout.is_empty(), "{mode}: a figure was printed");
        assert!(stderr.contains(said), "{mode}: no `{said}` in: {stderr}");
        let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(left.is_empty(), "{mode}: scratch left behind: {left:?}");
    }
}

/// A server that pings the measurement and handles nothing until it hears a
/// pong is answered, so the measurement still completes.
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_answers_a_ping_and_completes() {
    let pool = Pool::new("pool-measure-ping");
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
        .env("OH_MEASURE_CODEX", mock_bin())
        .env("OH_MEASURE_SOCKET_TIMEOUT", "10")
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_FAULT", "ping-pong")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let measured: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(measured["threads"].as_array().unwrap().len(), 2);
}

/// Each way the sharing measurement can fail is named with its next step, no
/// figure is printed, and the measurement's scratch directory is gone all the
/// same: a server that exits before it listens (its exit, the last connection
/// error and its stderr), one that never answers the WebSocket upgrade (within
/// the socket bound), and one that refuses `thread/start` (its error).
#[cfg(target_os = "linux")]
#[test]
fn the_sharing_measurement_names_each_failure_and_leaves_no_scratch() {
    let pool = Pool::new("pool-measure-fail");
    let cases: [(&str, (&str, String), &[&str]); 3] = [
        (
            "never-listened",
            ("CODEX_MOCK_EXIT_FIRST", pool.file("exited")),
            &[
                "measure-app-server-sharing: the server never accepted a connection on ",
                "(last attempt: [Errno 2] No such file or directory; exit 1); its stderr:",
                "check `codex login status`",
            ],
        ),
        (
            "stalled-upgrade",
            ("CODEX_MOCK_FAULT", "stall-upgrade".to_string()),
            &["measure-app-server-sharing: the WebSocket upgrade failed: timed out; rerun"],
        ),
        (
            "refused-thread",
            ("CODEX_MOCK_FAULT", "refuse-thread-start".to_string()),
            &[
                "measure-app-server-sharing: thread/start was refused: ",
                "the mock refuses thread/start",
                "check `codex login status`",
            ],
        ),
    ];
    for (name, (knob, value), said_all) in cases {
        let tmp = pool.path(&format!("{name}-tmp"));
        std::fs::create_dir_all(&tmp).unwrap();
        let output = Command::new("bash")
            .arg(repo_root().join("scripts/measure-app-server-sharing.sh"))
            .env("OH_MEASURE_CODEX", mock_bin())
            .env("OH_MEASURE_SOCKET_TIMEOUT", "2")
            .env("TMPDIR", &tmp)
            .env("CODEX_MOCK_DIR", pool.path("mock"))
            .env(knob, &value)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{name}: {said}");
        assert!(
            output.stdout.is_empty(),
            "{name}: a figure was printed: {said}"
        );
        for part in said_all {
            assert!(said.contains(part), "{name}: no `{part}` in: {said}");
        }
        let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(left.is_empty(), "{name}: scratch left behind: {left:?}");
    }
}

// llmlint: ignore-block[e2e_not_mocked] The reuse workload and its pooled server run for real through oneharness-core's pool; only codex is the repository's subprocess double, because the gate cannot run real Codex and needs its frames and process tree recorded to assert on. Real Codex under this workload is scripts/e2e-control.sh's pooled-reuse phase.
/// live/control/app-server-reuse.sh runs its workload through the pool: each
/// of the leases ran `initialize`, `thread/start`, a `thread/shellCommand` and
/// `thread/resume` of its own thread and no `turn/start`, all on one server;
/// and each reading's pids are exactly the server's process tree — launcher,
/// server and grandchild — as the OS lists it, its bytes counting at least the
/// server's own footprint, which is no root's. A 1 ms linger cannot split the
/// leases across servers: the workload's own idle lease holds the server from
/// the first lease to the last.
#[test]
fn the_reuse_workload_runs_through_the_pool_and_reads_the_whole_server_tree() {
    const SERVER_BYTES: u64 = 64 * 1024 * 1024;
    let pool = Pool::new("pool-reuse");
    let out = pool.path("reuse.json");
    let output = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "5000")
        .env("OH_E2E_REUSE_FILE", &out)
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_SHIM", "1")
        .env("CODEX_MOCK_HELPER", "1")
        .env("CODEX_MOCK_SERVER_BYTES", SERVER_BYTES.to_string())
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
        // The root is the launcher; the footprint is its child's.
        let bytes = reuse["readings"][name]["rss_bytes"].as_u64().unwrap();
        assert!(
            bytes >= SERVER_BYTES,
            "{name}: {bytes} B leaves out the server's own {SERVER_BYTES} B"
        );
    }
    let native = pool.the_server();
    assert!(tree.contains(&u64::from(native)));
    for lease in leases {
        let thread = lease["thread"].as_str().unwrap();
        let no_model_turns = pool.state(native)["turns"].as_array().unwrap().is_empty();
        assert!(no_model_turns, "a lease ran a model turn");
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
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The workload and its pooled server run for real; only `ps` is replaced, by a script or by its absence, because a real `ps` cannot be made to fail, list a malformed row or omit a running server on demand. The real listing is read end to end by the_reuse_workload_runs_through_the_pool_and_reads_the_whole_server_tree, which holds it to the OS's own process tree.
/// Each way the reuse workload's process listing can fail fails the workload
/// naming it, writing no telemetry: a `ps` it cannot run, one that fails, and
/// one listing a row it cannot read, a row of the wrong shape, a pid or an
/// rss out of range, no server at all, or a server tree whose rows are each
/// in range but sum past it. `ps` is a double first on PATH (wrapping the real
/// one for the sum), or, to be unrunnable, absent from a PATH of only the
/// tools the run needs. The server runs behind a launcher with a helper, so
/// its tree is three processes.
#[test]
fn the_reuse_workload_names_each_process_listing_failure() {
    let real_ps = Command::new("sh")
        .args(["-c", "command -v ps"])
        .output()
        .unwrap();
    let real_ps = String::from_utf8(real_ps.stdout)
        .unwrap()
        .trim()
        .to_string();
    let cases: [(&str, &str, &str); 8] = [
        ("unrunnable", "", "cannot run `ps`: "),
        (
            "failing",
            "echo 'ps: no listing here' >&2; exit 1",
            "`ps -A -o pid=,ppid=,rss=` failed (exit status: 1): ps: no listing here",
        ),
        (
            "unreadable",
            "echo '12 x 3'",
            "`ps` listed an unreadable row \"12 x 3\"",
        ),
        (
            "short",
            "echo '12 1'",
            "`ps` listed \"12 1\", not a pid, parent and rss",
        ),
        (
            "pid-range",
            "echo '99999999999 1 3'",
            "`ps` listed a pid out of range: \"99999999999 1 3\"",
        ),
        (
            "rss-range",
            "echo '12 1 18014398509481984'",
            "`ps` listed an rss out of range: \"12 1 18014398509481984\"",
        ),
        ("serverless", "echo '1 0 10'", ") is not running"),
        (
            // The largest rss whose bytes fit: any two of them overflow.
            "sum-range",
            "\"$OH_REAL_PS\" \"$@\" | awk '{ print $1, $2, \"18014398509481983\" }'",
            "the server tree's rss over pids [",
        ),
    ];
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, body, said)| {
            let real_ps = real_ps.clone();
            std::thread::spawn(move || {
                let pool = Pool::new(&format!("pool-reuse-ps-{case}"));
                let bin = pool.path("ps-double");
                std::fs::create_dir_all(&bin).unwrap();
                let path = if body.is_empty() {
                    // PATH lookup skips an entry it cannot run, so no `ps`
                    // anywhere on it: only the tools the run itself calls.
                    for tool in [
                        "bash", "sh", "cargo", "node", "mktemp", "rm", "dirname", "cp", "sleep",
                    ] {
                        let found = Command::new("sh")
                            .args(["-c", &format!("command -v {tool}")])
                            .output()
                            .unwrap();
                        let found = String::from_utf8(found.stdout).unwrap();
                        std::os::unix::fs::symlink(found.trim(), bin.join(tool)).unwrap();
                    }
                    bin.display().to_string()
                } else {
                    let ps = bin.join("ps");
                    std::fs::write(&ps, format!("#!/bin/sh\n{body}\n")).unwrap();
                    std::fs::set_permissions(
                        &ps,
                        std::os::unix::fs::PermissionsExt::from_mode(0o755),
                    )
                    .unwrap();
                    format!(
                        "{}:{}",
                        bin.display(),
                        std::env::var("PATH").unwrap_or_default()
                    )
                };
                let output = Command::new("bash")
                    .arg(repo_root().join("live/control/app-server-reuse.sh"))
                    .env("PATH", path)
                    .env("OH_REUSE_CODEX", mock_bin())
                    .env("OH_REAL_PS", &real_ps)
                    .env("CODEX_MOCK_SHIM", "1")
                    .env("CODEX_MOCK_HELPER", "1")
                    .env("OH_REUSE_LEASES", "1")
                    .env("OH_REUSE_SETTLE_MS", "1000")
                    .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
                    .env("XDG_STATE_HOME", pool.path("state"))
                    .env("CODEX_MOCK_DIR", pool.path("mock"))
                    .env(
                        "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
                        LINGER_MS.to_string(),
                    )
                    .output()
                    .unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                let wrote = pool.path("reuse.json").exists();
                (case, said, output.status.success(), stderr, wrote)
            })
        })
        .collect();
    for run in runs {
        let (case, said, ok, stderr, wrote) = run.join().unwrap();
        assert!(!ok, "{case}: {stderr}");
        assert!(stderr.contains(said), "{case}: no `{said}` in: {stderr}");
        assert!(!wrote, "{case}: telemetry was written");
    }
}

// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] Only `cargo` is replaced, by a script first on PATH, because a real build cannot be made to name no executable, one that is gone, unreadable JSON, or a target directory whose path needs JSON escapes without staging a checkout at such a path; the real build's artifact message is read end to end by every other reuse-workload test, which runs the example from wherever the calling build's target directory put it.
/// The reuse workload runs the example cargo's artifact message names, read
/// as JSON: a path spelled with escapes runs the file it names, and a build
/// whose message names no executable for it, one that is not there, or is no
/// JSON at all, fails the workload saying so, before any server starts.
#[test]
fn the_reuse_workload_runs_the_example_its_build_names_and_refuses_one_it_cannot() {
    let artifact = |executable: &str| {
        format!(
            "{{\"reason\":\"compiler-artifact\",\"target\":{{\"kind\":[\"example\"],\
             \"name\":\"app_server_reuse\"}},\"executable\":{executable}}}"
        )
    };
    // `after`, when not empty, is a command the double runs half a second
    // after it has answered, as another build re-linking the example would.
    let run = |case: &str, message: &str, after: &str| {
        let pool = Pool::new(&format!("pool-reuse-cargo-{case}"));
        let bin = pool.path("cargo-double");
        std::fs::create_dir_all(&bin).unwrap();
        let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
        let cargo = bin.join("cargo");
        std::fs::write(
            &cargo,
            "#!/bin/sh\ncat \"$OH_CARGO_MESSAGE\"\n\
             [ -z \"$OH_CARGO_AFTER\" ] || (sleep 0.5; sh -c \"$OH_CARGO_AFTER\") >/dev/null 2>&1 &\n",
        )
        .unwrap();
        std::fs::set_permissions(&cargo, mode).unwrap();
        let message_file = pool.path("cargo-message");
        std::fs::write(&message_file, format!("{message}\n")).unwrap();
        let output = Command::new("bash")
            .arg(repo_root().join("live/control/app-server-reuse.sh"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("OH_CARGO_MESSAGE", &message_file)
            .env("OH_CARGO_AFTER", after)
            .env("OH_REUSE_CODEX", mock_bin())
            .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
            .env("XDG_STATE_HOME", pool.path("state"))
            .env("CODEX_MOCK_DIR", pool.path("mock"))
            .output()
            .unwrap();
        assert!(
            !pool.path("reuse.json").exists(),
            "{case}: telemetry was written"
        );
        assert!(
            pool.server_pids().is_empty(),
            "{case}: a server started without the example"
        );
        (pool, output)
    };

    // A target directory whose path JSON must escape: a quote and a backslash.
    let pool = Pool::new("pool-reuse-cargo-escaped-home");
    let dir = pool.path("odd \"target\\dir");
    std::fs::create_dir_all(&dir).unwrap();
    let example = dir.join("app_server_reuse");
    let ran = pool.path("example-ran");
    std::fs::write(
        &example,
        format!(
            "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\nexit 3\n",
            ran.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &example,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let escaped = serde_json::to_string(&example.display().to_string()).unwrap();
    assert!(
        escaped.contains("\\\"") && escaped.contains("\\\\"),
        "{escaped}"
    );
    let (_scratch, output) = run("escaped", &artifact(&escaped), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "escaped: {stderr}");
    assert!(
        stderr.contains("app-server-reuse: the workload failed"),
        "{stderr}"
    );
    let args = std::fs::read_to_string(&ran).expect("the escaped path's example did not run");
    assert!(args.starts_with("--codex "), "{args}");

    // Missing for a moment when the build answers, as while another build
    // re-links it, then back: the workload waits for it and runs it.
    let relinked = pool.path("relinked");
    std::fs::create_dir_all(&relinked).unwrap();
    let later = relinked.join("app_server_reuse");
    let after = format!("cp '{}' '{}'", example.display(), later.display());
    std::fs::remove_file(&ran).unwrap();
    let named = serde_json::to_string(&later.display().to_string()).unwrap();
    let (_scratch, output) = run("relinked", &artifact(&named), &after);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "relinked: {stderr}");
    assert!(
        stderr.contains("app-server-reuse: the workload failed"),
        "relinked: {stderr}"
    );
    assert!(ran.exists(), "relinked: the example did not run: {stderr}");

    // There but not readable, so it cannot be copied: the copy's own error,
    // not a missing executable, is the reason.
    let locked = pool.path("locked");
    std::fs::create_dir_all(&locked).unwrap();
    let unreadable = locked.join("app_server_reuse");
    std::fs::copy(&example, &unreadable).unwrap();
    std::fs::set_permissions(
        &unreadable,
        std::os::unix::fs::PermissionsExt::from_mode(0o311),
    )
    .unwrap();
    let named = serde_json::to_string(&unreadable.display().to_string()).unwrap();
    let (_scratch, output) = run("uncopyable", &artifact(&named), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "uncopyable: {stderr}");
    assert!(
        stderr.contains(&format!(
            "cannot copy the app_server_reuse example {}",
            unreadable.display()
        )) && stderr.contains("Permission denied"),
        "uncopyable: {stderr}"
    );

    let cases = [
        (
            "unnamed",
            artifact("null"),
            "cargo built the app_server_reuse example but its output named no executable for it",
        ),
        (
            "absent",
            artifact("\"/nonexistent/app_server_reuse\""),
            "cargo named /nonexistent/app_server_reuse as the app_server_reuse example, but no executable is there",
        ),
        (
            "unreadable",
            "{\"reason\":\"compiler-artifact\",".to_string(),
            "cargo's artifact message for the app_server_reuse example is not JSON node can read",
        ),
    ];
    for (case, message, said) in cases {
        let (_scratch, output) = run(case, &message, "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{case}: {stderr}");
        assert!(stderr.contains(said), "{case}: no `{said}` in: {stderr}");
    }
}

// llmlint: ignore-end[e2e_not_mocked]

/// A server whose `thread/loaded/list` pages never end — each naming a cursor
/// already answered — fails the reuse workload at once, naming the repeated
/// cursor, rather than paging until the listing's deadline.
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

/// The live tools refuse a bad setting before they run anything, exiting 2
/// and naming the setting, its value and the fix: the reuse workload's settle
/// bound and lease count, and the sharing measurement's socket bound.
#[test]
fn the_live_tools_refuse_a_bad_setting_before_running_anything() {
    let pool = Pool::new("pool-live-settings");
    let reuse = repo_root().join("live/control/app-server-reuse.sh");
    let measure = repo_root().join("scripts/measure-app-server-sharing.sh");
    for (script, name, value, said) in [
        (
            &reuse,
            "OH_REUSE_SETTLE_MS",
            "soon",
            "OH_REUSE_SETTLE_MS must be whole milliseconds below 1000000, got 'soon'",
        ),
        (&reuse, "OH_REUSE_SETTLE_MS", "1000000", "got '1000000'"),
        (
            &reuse,
            "OH_REUSE_LEASES",
            "0",
            "OH_REUSE_LEASES must be a whole number from 1 to 99, got '0'",
        ),
        (&reuse, "OH_REUSE_LEASES", "100", "got '100'"),
        (&reuse, "OH_REUSE_LEASES", "five", "got 'five'"),
        (
            &measure,
            "OH_MEASURE_SOCKET_TIMEOUT",
            "0",
            "OH_MEASURE_SOCKET_TIMEOUT is '0'; set it to a whole number of seconds from 1 to 3600",
        ),
        (
            &measure,
            "OH_MEASURE_SOCKET_TIMEOUT",
            "99999999999999999999",
            "OH_MEASURE_SOCKET_TIMEOUT is '99999999999999999999'",
        ),
        (
            &measure,
            "OH_MEASURE_SOCKET_TIMEOUT",
            "slow",
            "OH_MEASURE_SOCKET_TIMEOUT is 'slow'",
        ),
    ] {
        let output = Command::new("bash")
            .arg(script)
            .env("OH_REUSE_CODEX", mock_bin())
            .env("OH_MEASURE_CODEX", mock_bin())
            .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
            .env("CODEX_MOCK_DIR", pool.path("mock"))
            .env(name, value)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{name}={value}: {stderr}");
        assert!(stderr.contains(said), "{name}={value}: {stderr}");
        assert!(
            pool.mock_servers().is_empty(),
            "{name}={value}: a server was started"
        );
    }
}

// llmlint: ignore-block[e2e_not_mocked] The control suite's pooled phases run for real — scripts/e2e-control.sh, the built oneharness and oneharness-core's pool; only codex is the repository's subprocess double, installed first on PATH, because the gate cannot run real Codex or make it leak, retain or keep growing on demand. The same phases against real Codex are scripts/e2e-control.sh's live run.
/// scripts/e2e-control.sh's shared app-server phases, `codex` the mock on PATH.
/// The suite runs its reuse phase on Linux alone, where its memory budget is
/// measured, and says it skipped it elsewhere.
struct ControlSuite {
    pool: Pool,
}

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
            .env("OH_REUSE_SETTLE_MS", "5000")
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
// llmlint: ignore-end[e2e_not_mocked]

/// A server whose `thread/loaded/list` keeps naming fresh cursors fails the
/// reuse workload once its listing runs past the settle bound (and its grace),
/// naming that, instead of paging forever.
#[test]
fn the_reuse_workload_refuses_a_loaded_list_still_paging_at_its_deadline() {
    let pool = Pool::new("pool-reuse-endless");
    let mut child = Command::new("bash")
        .arg(repo_root().join("live/control/app-server-reuse.sh"))
        .env("OH_REUSE_CODEX", mock_bin())
        .env("OH_REUSE_SETTLE_MS", "1000")
        .env("OH_E2E_REUSE_FILE", pool.path("reuse.json"))
        .env("XDG_STATE_HOME", pool.path("state"))
        .env("CODEX_MOCK_DIR", pool.path("mock"))
        .env("CODEX_MOCK_FAULT", "loaded-endless")
        .env(
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS",
            LINGER_MS.to_string(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
    // Stopped before a page's request or after its answer, as the clock falls.
    assert!(
        said.contains("thread/loaded/list was still paging at the deadline")
            || said.contains("thread/loaded/list answered past the deadline"),
        "{said}"
    );
}

/// scripts/e2e-control.sh's pooled-kill bound restates the pool's own sweep
/// interval and stop deadline, and is held to them here.
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

// llmlint: ignore-block[e2e_not_mocked] The control suite's reuse phase runs for real — scripts/e2e-control.sh, the workload and oneharness-core's pool; only codex is the repository's subprocess double, because real Codex cannot be made to keep a released thread for 30 minutes, or forever, on demand within the gate. The same phase against real codex-cli 0.163.0 is scripts/e2e-control.sh's live pooled-reuse run.
/// The suite's reuse phase runs the workload and the memory budget's check,
/// against a mock that, like codex-cli 0.163.0, keeps a released thread
/// loaded for 30 minutes unless its launch shortens that: it passes when the
/// pool releases each thread — its server launched with a 1 s unload delay —
/// and fails naming both readings, and the threads still loaded at the settle
/// bound, when the mock ignores the release and keeps a large allocation per
/// thread it keeps.
#[cfg(target_os = "linux")]
#[test]
fn the_control_suite_s_reuse_phase_fails_naming_both_readings_when_threads_are_retained() {
    let codex_0_163 = ("CODEX_MOCK_UNLOAD_DELAY_SECS", "1800");
    let suite = ControlSuite::new("pool-suite-rss");
    let bytes = (100 * 1024 * 1024).to_string();
    let kept = suite.run(&[codex_0_163, ("CODEX_MOCK_THREAD_BYTES", &bytes)]);
    let said = String::from_utf8_lossy(&kept.stderr);
    assert!(kept.status.success(), "{said}");
    assert!(said.contains("within"), "{said}");
    // The server that held the workload's threads was launched with the
    // short unload delay; the pooled-env and pooled-kill servers keep the
    // default.
    let reuse: Value =
        serde_json::from_str(&std::fs::read_to_string(suite.pool.path("reuse.json")).unwrap())
            .unwrap();
    let thread = reuse["leases"][0]["thread"].as_str().unwrap();
    let servers = suite.pool.mock_servers();
    let held = servers
        .iter()
        .find(|record| {
            let pid = record["pid"].as_u64().unwrap() as u32;
            suite.pool.state(pid)["releases"]
                .as_array()
                .is_some_and(|releases| releases.iter().any(|r| r["thread"] == thread))
        })
        .unwrap_or_else(|| panic!("no mock server held {thread}: {servers:?}"));
    let argv: Vec<&str> = held["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["-c", "thread_unload_delay_secs=1"]),
        "the workload's server keeps Codex's own unload delay: {argv:?}"
    );
    let suite = ControlSuite::new("pool-suite-rss-leak");
    let retained = suite.run(&[
        codex_0_163,
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
// llmlint: ignore-end[e2e_not_mocked]

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

// llmlint: ignore-block[e2e_not_mocked] This drives the real built `oneharness run --control` through the real pool; only codex is the repository's subprocess double, held mid-turn so a timeout or cancel lands on demand, which real Codex cannot be made to do deterministically. Real Codex's killed pooled dispatch is held by scripts/e2e-control.sh's pooled-kill phase.
/// A pooled turn past its `--timeout` is `timeout`, and one whose run is
/// cancelled (SIGTERM to oneharness) is `cancelled`; either way its live turn
/// is interrupted on the server, its commands stopped and its thread released,
/// rather than left running on a server nobody reads.
///
/// The timed dispatch runs once the cancelled one's turn is beating, on the
/// server that one started: its deadline is about a turn cut short, and a
/// cold server's start — slow under coverage on macOS — would otherwise spend
/// it before any thread opened.
#[test]
fn a_pooled_turn_cut_short_by_its_timeout_or_a_cancel_leaves_nothing_behind() {
    let pool = Pool::new("pool-cut");
    let never = pool.file("never");
    let cancel_beat = pool.file("cancel-beat");
    let child = pool.spawn(pool.command(
        "cancelled",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", &heartbeat(&cancel_beat)),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    wait_for("the heartbeat to start", Duration::from_secs(20), || {
        lines(&cancel_beat) >= 3
    });

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
    let server = reported_server(&report);
    left_nothing_behind(&pool, server, &report);

    // SAFETY: plain integers; the pid is the run this journey started.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let (_, report) = Pool::finish(child);
    assert_eq!(result(&report)["status"], "cancelled", "{report}");
    assert_eq!(reported_server(&report), server, "{report}");
    left_nothing_behind(&pool, server, &report);
}
// llmlint: ignore-end[e2e_not_mocked]

/// A streaming consumer that stops reading — its end of `run --stream`'s
/// stdout closed — ends a pooled run as it ends one on a dispatch's own child,
/// and the live turn it leaves is interrupted on the shared server and its
/// thread released, rather than left billing on a server nobody reads.
#[test]
fn a_streaming_consumer_that_stops_reading_leaves_no_live_pooled_turn() {
    use std::io::BufRead;
    let pool = Pool::new("pool-stream-stop");
    let never = pool.file("never");
    let mut child = pool.spawn(pool.command(
        "streamed",
        &["--stream"],
        &[
            ("MOCK_CODEX_HOLD_UNTIL", &never),
            ("MOCK_CODEX_TICK_MS", "100"),
        ],
    ));
    let server = pool.await_live_turns(1);
    {
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        assert!(!first.is_empty(), "the run streamed nothing");
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the run kept going after its consumer left"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let turns = pool.state(server)["turns"].as_array().unwrap().clone();
    assert_eq!(turns.len(), 1, "{turns:?}");
    assert_eq!(turns[0]["status"], "interrupted", "{turns:?}");
    let thread = turns[0]["thread"].as_str().unwrap().to_string();
    wait_for("the thread to be released", Duration::from_secs(10), || {
        !holds_thread(&pool, server, &thread)
    });
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
/// Only Linux refuses that write; Darwin drops it, which the journey after
/// this one plays on every platform.
#[test]
fn a_write_refused_under_a_live_turn_is_app_server_lost() {
    let pool = Pool::new("pool-write-lost");
    let never = pool.file("never");
    let mut child = pool.spawn(pool.command(
        "write-lost",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", "true"),
            ("MOCK_CODEX_REQUEST_APPROVAL", "1"),
            ("MOCK_CODEX_SHUT_READ_ON_APPROVAL", "1"),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    exits_within(
        &mut child,
        Duration::from_secs(30),
        "the dispatch whose connection refused its write",
    );
    let (output, report) = Pool::finish(child);
    let lost = result(&report);
    assert_eq!(
        lost["status"],
        "nonzero",
        "{lost}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
    if cfg!(target_os = "linux") {
        assert!(
            lost["error"]
                .as_str()
                .unwrap_or_default()
                .contains("refused a write"),
            "the loss was not read off the refused write: {lost}"
        );
    }
}

/// A shared server killed while a command it started holds the server's end
/// of the dispatch's connection is still lost under the turn: the dispatch
/// reads no end-of-file — the command keeps the connection open — so it learns
/// of the loss from the server itself, then stops the command, and nothing is
/// left running. On macOS a command holds a connection this way whenever the
/// server started it while another thread was accepting one;
/// `inheritable-connections` stages that on every platform.
#[test]
fn a_shared_server_killed_while_its_command_holds_the_connection_is_lost_within_a_bound() {
    let pool = Pool::new("pool-held-conn").with_env("CODEX_MOCK_FAULT", "inheritable-connections");
    let beat = pool.file("heartbeat");
    let never = pool.file("never");
    let mut dispatch = pool.spawn(pool.command(
        "held-conn",
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
    let command = command_running(&pool, server, &beat)["pid"]
        .as_u64()
        .unwrap() as u32;
    sigkill(server);
    let stdout = read_to_end(dispatch.stdout.take().unwrap());
    let stderr = read_to_end(dispatch.stderr.take().unwrap());
    let status = exits_within(
        &mut dispatch,
        DEAD_LEASE_STOP_DEADLINE * 3,
        "the dispatch whose server died under a command holding its connection",
    );
    let bound = Duration::from_secs(2);
    let output = Output {
        status,
        stdout: stdout
            .recv_timeout(bound)
            .expect("the dispatch's output ended"),
        stderr: stderr
            .recv_timeout(bound)
            .expect("the dispatch's errors ended"),
    };
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "no JSON report ({err}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let lost = result(&report);
    assert_eq!(lost["status"], "nonzero", "{lost}");
    assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
    assert!(
        lost["error"]
            .as_str()
            .is_some_and(|error| error.contains(&format!("(pid {server}) exited"))),
        "the loss was not read off the server's own exit: {lost}"
    );
    wait_for("the command holding the connection to stop", bound, || {
        !alive(command)
    });
}

/// The same answer onto a connection the server stopped reading, where the
/// platform does not refuse it: a Unix socket whose reader shut its read side
/// takes a write and drops it on Darwin (XNU marks the receive buffer
/// `SB_DROP`), where Linux fails it with `EPIPE`. Played on every platform
/// (MOCK_CODEX_SHUT_READ_DISCARDS), as macOS CI met it: the answer goes nowhere
/// and nothing says so, while the turn is held and the run has no timeout. The
/// server ends the connection it stopped reading, so the run still ends
/// `app_server_lost`, within a bound, rather than waiting on a turn that
/// cannot end — which it did while the server held that connection open.
#[test]
fn a_write_dropped_under_a_live_turn_still_ends_the_run_app_server_lost() {
    let pool = Pool::new("pool-write-dropped");
    let never = pool.file("never");
    let child = pool.spawn(pool.command(
        "write-dropped",
        &["--mode", "bypass"],
        &[
            ("MOCK_CODEX_RUN_COMMAND", "true"),
            ("MOCK_CODEX_REQUEST_APPROVAL", "1"),
            ("MOCK_CODEX_SHUT_READ_ON_APPROVAL", "1"),
            ("MOCK_CODEX_SHUT_READ_DISCARDS", "1"),
            ("MOCK_CODEX_HOLD_UNTIL", &never),
        ],
    ));
    let (output, report) = Pool::finish_within(child, Duration::from_secs(30));
    let lost = result(&report);
    assert_eq!(
        lost["status"],
        "nonzero",
        "{lost}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Dropped and then read as the connection's end, or refused outright
    // when the server's end of the connection came first: both are the
    // server lost under the turn.
    assert_eq!(lost["failure_kind"], "app_server_lost", "{lost}");
    let approvals = &pool.state(pool.the_server())["approvals"];
    assert_eq!(
        approvals[0]["answers"],
        json!([]),
        "the server read an answer it should have dropped: {approvals}"
    );
}
