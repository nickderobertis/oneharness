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
use serde_json::Value;

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

// ---------------------------------------------------------------------------
// One server per pool key, its lifetime the leases'
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// The pool key: what separates servers, and what does not
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Each dispatch's environment reaches only its own commands
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// A lease's end: its commands stopped, its thread released
// ---------------------------------------------------------------------------

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
