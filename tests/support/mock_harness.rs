// llmlint: ignore-file[comments_earn_their_place] Contradictory rules leave no arrangement that passes both: run 20260730T015003Z-03f66 rejected usage prose duplicated from `docs/harness-usage.md`, and run 20260730T022546Z-ea300 rejected the requested deferral to that document.
//! The deterministic harness responder shared by the shipped CLI and test fixture.
//!
//! A fake harness binary the e2e tests drive via a `--bin` override, so the
//! spawn / capture / parallel / parse path is exercised hermetically and
//! cross-platform — no real CLI, no network. The responder ships as
//! `oneharness mock-harness`; the separate fixture binary remains feature-gated.
//!
//! Behavior is scripted entirely through environment variables:
//!   MOCK_STDOUT     bytes written to stdout (default: a JSON `result` doc)
//!   MOCK_STDERR     bytes written to stderr
//!   MOCK_EXIT       process exit code (default: 0)
//!   MOCK_EXIT_<n>   with MOCK_ATTEMPT_FILE, process exit code for attempt n
//!   MOCK_SLEEP_MS   milliseconds to sleep before exiting (to force a timeout)
//!   MOCK_ARGV_FILE  if set, the received argv (one per line) is written here
//!   MOCK_ECHO_PWD   if set, write `PWD=<the inherited $PWD>` to stdout and exit
//!                   (used to assert that --cwd keeps $PWD consistent)
//!   MOCK_ECHO_ENV   if set to a variable NAME, write `NAME=<inherited value>`
//!                   to stdout and exit (used to assert per-harness env injection)
//!   MOCK_CAT_FILE   if set to a path, write that file's current contents to
//!                   stdout and exit — proving what a config file contained
//!                   WHILE the harness ran (the ephemeral mock-hook install,
//!                   restored afterwards, is only observable from inside).
//!   MOCK_CAT_ARG_AFTER  if set to a flag (e.g. `--settings`), find it in the
//!                   received argv and write the contents of the file named by
//!                   the NEXT argument to stdout, then exit — proving an
//!                   argv-delivered temp file existed and what it carried.
//!   MOCK_REPLY_AFTER_LINES  if set to N, read N newline-terminated request
//!                   lines from stdin, then answer with MOCK_STDOUT and exit.
//!                   With MOCK_ATTEMPT_FILE, the invocation is counted and
//!                   MOCK_STDOUT_<n> is preferred for attempt n exactly as on
//!                   the default path — so a `usage` probe that asks again can
//!                   be answered differently the second time.
//!   MOCK_REQUEST_FILE  with MOCK_REPLY_AFTER_LINES, the requests read from stdin
//!                   are written here, one per line, and reading continues for a
//!                   short grace past the Nth so a line the caller should NOT
//!                   have sent is recorded too. Answering after N lines cannot
//!                   prove WHICH lines arrived; the `usage` probes' zero-turn
//!                   property is a claim about exactly that, so a test has to
//!                   read the bodies back.
//!   MOCK_REPLY_DELAY_MS  with MOCK_REPLY_AFTER_LINES, act like a server whose
//!                   reply is asynchronous: answer this many ms after the last
//!                   request, and — because such a server shuts down on EOF with
//!                   that reply still in flight — exit WITHOUT answering if stdin
//!                   closes first. This is the codex `app-server` behavior, and
//!                   the only way a test can tell a caller that holds stdin open
//!                   for its answer from one that closes the pipe behind it.
//!                   A delay above `MAX_REPLY_DELAY_MS` is refused.
//!   MOCK_ECHO_STDIN if set, read ALL of stdin and write it verbatim to stdout,
//!                   then exit — proving a prompt delivered on the child's stdin
//!                   (the large-prompt escape hatch) actually arrived, and with
//!                   what content.
//!   MOCK_ATTEMPT_FILE  if set, a counter file: each invocation reads the prior
//!                   count, increments it, writes it back, and (when
//!                   MOCK_STDOUT_<n> is set for that 1-based attempt) emits that
//!                   instead of MOCK_STDOUT — used to script the structured-output
//!                   retry loop, where attempt 1 is invalid and a later one valid.
//!   MOCK_STREAM_DELAY_MS  if set, emit MOCK_STDOUT one line at a time, flushing
//!                   and sleeping this many ms between lines (so a streaming
//!                   consumer sees events arrive over time). On completion,
//!                   append `COMPLETE\n` to MOCK_LOG_FILE; on a failed write
//!                   (reader gone / killed mid-stream) exit WITHOUT it, so a test
//!                   can prove an early teardown by the sentinel's absence.
//!   MOCK_STREAM_CHUNK_BYTES  if set, emit MOCK_STDOUT in fixed-size byte chunks,
//!                   flushing each chunk and pausing by MOCK_STREAM_DELAY_MS
//!                   between chunks. This models providers that split one JSONL
//!                   record across multiple pipe reads.
//!   MOCK_PRESERVE_STDOUT  if `1`, bypass session-field normalization so a test
//!                   can preserve provider whitespace and framing exactly.
//!   MOCK_FAIL_IF_MODEL  if set to a model name, fail (exit 1) with a
//!                   `model not found` stderr when the received argv carries
//!                   `--model <that name>`; otherwise run normally. Lets a test
//!                   make one model in a fan-out unusable so a fallback run falls
//!                   through it to the next (the `model_not_found` classification).
//!                   MOCK_FAIL_STDERR overrides the emitted stderr (so the same
//!                   hook can simulate a rate limit instead of a missing model).
//!   MOCK_TURN_LOG   if set to a path, act like a *control-capable* harness whose
//!                   stdin is a message stream: emit MOCK_STDOUT immediately (the
//!                   in-turn transcript), then keep reading stdin, appending each
//!                   received line to the log verbatim. A line containing
//!                   `control_request` ends the turn — the log gains an
//!                   `INTERRUPTED` line and MOCK_TURN_RESULT is emitted, then a
//!                   `TURN_ENDED` line. The stream stays open afterwards, so a
//!                   message arriving next opens ANOTHER turn (which is how an
//!                   interrupt's redirection is delivered); EOF ends the process.
//!                   This is the hermetic stand-in for Claude Code's
//!                   `-p --input-format stream-json` control channel: the turn is
//!   MOCK_TURN_RESULT_DELAY_MS  with MOCK_TURN_LOG, wait this many milliseconds
//!                   after receiving a turn message before emitting its result.
//!                   observably in flight (the prompt frame appears in the log),
//!                   so a test can drive a *separate* `oneharness interrupt`
//!                   process against the live run.
//!   MOCK_TURN_RESULT  with MOCK_TURN_LOG, the terminal document emitted when the
//!                   turn ends (default: a stream-json `result` document).
//!   MOCK_TURN_HOLD  with MOCK_TURN_LOG, keep the turn running after the prompt
//!                   instead of completing it, so a test can interrupt a turn
//!                   that is genuinely still in flight.
//!   MOCK_TURN_ONCE  with MOCK_TURN_LOG, exit as soon as the first turn ends
//!                   instead of reading on — the harness that is GONE by the
//!                   time a committed redirection would be written to it, so a
//!                   test can drive the run's recovery from that write failing.
//!   MOCK_ACP_LOG    if set to a path, act like an **ACP JSON-RPC server** on
//!                   stdio (the shape `copilot --acp` / `goose acp` speak):
//!                   answer `initialize` and `session/new`, hold `session/prompt`
//!                   open, ask one `session/request_permission`, and end the turn
//!                   only when a `session/cancel` NOTIFICATION arrives — reporting
//!                   `stopReason: "end_turn"`, exactly as the real harnesses do
//!                   after a genuine cancellation. Every received line is appended
//!                   to the log, so a test can assert the client answered the
//!                   permission request (without which a real turn never starts)
//!                   and that cancel carried no `id`.
//!   MOCK_ACP_COMPLETE_TURN  with MOCK_ACP_LOG, end the turn on its own once the
//!                   client has answered the permission request, instead of
//!                   waiting for a `session/cancel`.
//!   MOCK_CODEX_APP_SERVER_LOG  if set to a path, act like **`codex app-server`**
//!                   on stdio: answer `initialize`, `thread/start` and
//!                   `thread/resume` (which rejoins the thread the client named,
//!                   echoing its id back exactly as the real server does), answer
//!                   `turn/start` with an in-progress turn (which is NOT the end
//!                   of the turn), and end it on `turn/completed` only once a
//!                   `turn/interrupt` naming the thread and turn arrives. Every
//!                   received line is appended to the log.
//!   (`app-server --listen unix://PATH`)  act like the shared, multi-client
//!                   app-server a pooled dispatch runs on, whatever else is set;
//!                   its knobs are listed on `listen_server` below.
//!   MOCK_CODEX_PLUGINS_LOG     if set to a path, a launch as `exec` or
//!                              `app-server` resolves Codex's `plugins` feature
//!                              from its argv the way Codex does (on unless
//!                              named; `-c features.plugins=<bool>` last-wins,
//!                              then any `--enable plugins`, then any
//!                              `--disable plugins`) and appends one JSON
//!                              line: `subcommand`, `plugins`, `catalog_fetched`
//!                              and `loaded` — the plugins installed under
//!                              `$CODEX_HOME/plugins/` (one directory each)
//!                              that it loaded, none when the feature is off
//!                              (loading them needs `CODEX_HOME` set). Any
//!                              other launch, `--version` among them, is not
//!                              recorded.
//!                              It then carries on as it otherwise would.
//!   MOCK_CODEX_PLUGINS_LIST_FAULT  with MOCK_CODEX_PLUGINS_LOG, if set, an
//!                              entry of `$CODEX_HOME/plugins/` fails to read
//!                              after the directory opened, so that refusal
//!                              can be driven end to end.
//!   MOCK_CODEX_VERSION         if set, `--version` (alone) answers
//!                              `codex-cli <it>` and the catalog fetch below
//!                              stamps its cache with it. Unset, `--version`
//!                              answers MOCK_STDOUT (else the default document),
//!                              which names no Codex version. Either way a
//!                              `--version` run does nothing else — no hold, no
//!                              attempt count, no log line — beyond writing
//!                              MOCK_ARGV_FILE and exiting with MOCK_EXIT, since
//!                              oneharness asks it before every Codex launch.
//!   MOCK_CODEX_VERSION_DELAY_MS  how long `--version` takes to answer.
//!   MOCK_CODEX_VERSION_EXIT    the exit code `--version` answers with (the
//!                              launch itself keeps MOCK_EXIT).
//!   MOCK_CODEX_CACHE_CORRUPT   with MOCK_CODEX_CATALOG_LOG, a fetch writes a
//!                              cache Codex would not: `json` (not JSON), `time`
//!                              (an unreadable fetched_at), `empty` (no models)
//!                              or `models` (a model that is no object).
//!   MOCK_CODEX_CATALOG_LOG     if set to a path, a launch as `exec` or
//!                              `app-server` acts like Codex about its model
//!                              catalog and appends one JSON line: `subcommand`,
//!                              `pid`, `started_ms`, `codex_home`, `catalog`
//!                              (the `-c model_catalog_json=` path, last one
//!                              winning, else null), `fetch`, `apps` (whether
//!                              it connects to ChatGPT's apps server: Codex's
//!                              `apps` feature, resolved like `plugins`) and
//!                              `argv`. With a catalog
//!                              it reads that file (`catalog_models`, its model
//!                              count) and fetches nothing (`fetch` null); a
//!                              missing or empty catalog fails the launch, as it
//!                              fails Codex. Without one it fetches: `fetch` is
//!                              `ok` (with `fetched_ms`) after it writes
//!                              `$CODEX_HOME/models_cache.json` (Codex's
//!                              default `~/.codex` when CODEX_HOME is unset)
//!                              stamped with MOCK_CODEX_CACHE_VERSION, else
//!                              MOCK_CODEX_VERSION, else the fault's outcome.
//!                              It then carries on as it otherwise would.
//!   MOCK_CODEX_CATALOG_FETCH_MS  with MOCK_CODEX_CATALOG_LOG, how long a fetch
//!                              takes before it writes the cache.
//!   MOCK_CODEX_CATALOG_THEN_EXIT  with MOCK_CODEX_CATALOG_LOG, an `app-server`
//!                              launch that fetched exits 0 right after,
//!                              instead of serving its stdin.
//!   MOCK_CODEX_CATALOG_FAULT   with MOCK_CODEX_CATALOG_LOG, `fail` makes every
//!                              fetch fail (`fetch: "failed"`, no cache written);
//!                              `hang` makes it never answer (`fetch: "hung"`,
//!                              no cache written), and an `app-server` launch
//!                              then waits for its stdin to close, as a server
//!                              whose fetch is still in flight does; `exit`
//!                              fails it like `fail`, and an `app-server`
//!                              launch then exits 1 at once, as a Codex
//!                              server that cannot start does.
//!   MOCK_CODEX_ECHO_PROMPT     with MOCK_CODEX_APP_SERVER_LOG, the turn's
//!                              agent message is the text of the `turn/start`
//!                              input instead of a fixed "still working".
//!   MOCK_CODEX_COMPLETE_TURN   with MOCK_CODEX_APP_SERVER_LOG, end the turn on
//!                              its own (`turn/completed`) instead of holding it
//!                              open until an interrupt arrives.
//!   MOCK_CODEX_OVERLOAD_ATTEMPTS with MOCK_CODEX_APP_SERVER_LOG and
//!                              MOCK_ATTEMPT_FILE, fail this many app-server
//!                              invocations with Codex's terminal
//!                              `serverOverloaded` error before completing.
//!   MOCK_CODEX_MODEL           with MOCK_CODEX_APP_SERVER_LOG, the model the
//!                              `thread/start` / `thread/resume` response names
//!                              (`ThreadStartResponse.model`, a required string
//!                              on the real server) INSTEAD of echoing the one
//!                              the request asked for — a server that runs the
//!                              thread under something else. Unset, the response
//!                              echoes the requested model, or names
//!                              `mock-codex-default` when the request named none,
//!                              exactly as the real server answers its own default.
//!   MOCK_CODEX_OMIT_MODEL      with MOCK_CODEX_APP_SERVER_LOG, answer
//!                              `thread/start` / `thread/resume` with NO `model`
//!                              field at all — a server that states nothing
//!                              about the model, which the client must take as
//!                              no claim rather than a wrong one.
//!   MOCK_CODEX_TOOL_STARTED_ONLY  with MOCK_CODEX_TOOL_EVENTS, emit only the
//!                              captured `item/started` tool notification.
//!   MOCK_CODEX_DIE_ON_INTERRUPT  with MOCK_CODEX_APP_SERVER_LOG, acknowledge the
//!                   interrupt and then EXIT without the `turn/completed` that
//!                   ends the turn, appending `DIED_AFTER_INTERRUPT`. The
//!                   conversation is gone holding a redirection it committed, so
//!                   a test can drive the run's report of that loss.
//!   MOCK_HTTP_CONTROL_LOG  if set to a path, act like an **opencode-shaped HTTP
//!                   control server** on the `--port` from its own argv: create a
//!                   session, block the turn on a permission request, and abort it
//!                   on the session's interrupt route. Every request line (and its
//!                   body) is appended to the log.
//!                   The `redirected-turn` fault instead models opencode's real
//!                   redirection shape, measured live: the prompt request is
//!                   HELD OPEN for the whole turn and answered with a refusal
//!                   when the interrupt aborts it, the aborted turn ends
//!                   SILENTLY (no text, no idle — the stream just stops), and
//!                   the session then takes a second prompt and runs it. A
//!                   driver that reads the aborted turn's refusal as the run's
//!                   outcome, or that waits for an end-of-turn event before
//!                   delivering the redirection, loses the message.
//!   MOCK_HTTP_CONTROL_FAULT  with MOCK_HTTP_CONTROL_LOG, break the server the way
//!                   a real one breaks, so the run's user-visible failure can be
//!                   asserted: `never-ready` exits before binding at all (nothing
//!                   ever answers), appending one `LAUNCHED never-ready` line per
//!                   launch so a relaunch is countable, `first-candidate-dies`
//!                   does the same for the first CANDIDATE's two launches (its
//!                   own plus the relaunch it is owed) and then comes up healthy,
//!                   so a chain falls through one pooled candidate onto the
//!                   next, `lose-port` dies on its
//!                   first launch the way a server that lost its reserved port
//!                   does and comes up healthy on the relaunch, `silent-server`
//!                   BINDS its port and stays alive but answers nothing (the
//!                   other half of "not ready": a process that is there and
//!                   quiet, which must be reported against the window and never
//!                   relaunched),
//!                   `refuse-session` answers the session-create
//!                   route `503`, and `no-session-id` answers it `200` with a body
//!                   naming no id, and `foreign-permission` asks permission for a
//!                   session this run does not own, and `close-stream` stops
//!                   the event stream mid-turn, `refuse-prompt` rejects prompt
//!                   submission, and `hang-prompt` never answers that request,
//!                   talking mid-turn with no end-of-turn event, and
//!                   `redirect-interrupt` answers the interrupt route `302`
//!                   without aborting anything. Each exits once
//!                   it has served
//!                   its fault, so nothing is left behind for the pool to reclaim.
//!                   An unrecognized value is refused rather than run as no fault.
//!   MOCK_LOG_FILE   if set, an append-only run log: each invocation appends `S\n`
//!                   when it starts (before MOCK_SLEEP_MS) and `E\n` when it ends
//!                   (after the sleep). With a sleep that exceeds spawn latency,
//!                   the interleaving reveals scheduling: concurrent calls all
//!                   write `S` before the first `E`; a barrier (one call, then the
//!                   rest) shows `S E` before any further `S`. Used to pin the
//!                   batch `speed` vs `min-tokens` wave ordering. Small single-byte
//!                   lines + O_APPEND keep cross-process writes from interleaving.
//!   MOCK_NATIVE_GRANDCHILD_MS  Unix/Windows: act like a launcher which starts a
//!                   long-lived native child. The child emits MOCK_STDOUT /
//!                   MOCK_STDERR, then ticks for this many milliseconds while
//!                   inheriting both output pipes; MOCK_TICK_FILE receives one
//!                   byte per tick when set. The launcher waits for it. On Unix
//!                   the child ignores TERM; on Windows it is a second copy of
//!                   this native binary, whose lifetime is independent of its
//!                   direct parent unless the Job Object contains it. This
//!                   reproduces the process-tree timeout boundary of npm shims.
//!   MOCK_HOLD_FILE  if set to a path, block at startup until that file names
//!                   this process's pid on a line of its own. A supervising
//!                   caller asks the OS about the child while the run still
//!                   holds it, and macOS answers nothing about a process that
//!                   has already exited (`getpgid` refuses a zombie), so a test
//!                   that needs the child demonstrably alive across the
//!                   hand-over releases it from that hook instead of sleeping
//!                   long enough to hope. Bounded, so a caller that never
//!                   releases fails its own assertion rather than hanging.

use std::io::Write;

/// How long the request reader keeps listening for a line the caller should not
/// have sent, when a request log is being written and no reply delay sets its
/// own window. A caller writes its whole exchange up front, so an extra line is
/// already in the pipe; this only has to outlast the scheduling of one read.
const EXTRA_REQUEST_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// The largest reply delay `MOCK_REPLY_DELAY_MS` accepts. A scripted delay only
/// has to outlast a probe's own timeout, so this cap sits far past any real use,
/// while any larger value is a typo that would otherwise overflow the `Instant`
/// deadline the delay becomes.
const MAX_REPLY_DELAY_MS: u64 = 600_000;

/// Per-attempt scripting: with MOCK_ATTEMPT_FILE set, read the prior count,
/// increment it, write it back, and return this invocation's 1-based attempt
/// number — so a test can answer the same caller differently on its second
/// spawn. `None` when the knob is unset.
fn scripted_attempt() -> Option<u32> {
    std::env::var("MOCK_ATTEMPT_FILE").ok().map(|path| {
        let prior = std::fs::read_to_string(&path)
            .ok()
            .map(|text| {
                text.trim()
                    .parse::<u32>()
                    .expect("MOCK_ATTEMPT_FILE must contain an unsigned integer")
            })
            .unwrap_or(0);
        let attempt = prior + 1;
        let _ = std::fs::write(path, attempt.to_string());
        attempt
    })
}

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Real Codex's precedence can only be read from an installed Codex, which the deterministic gate does not have; this task places that reconciliation in the live codex suite (`oh_codex_feature_flag_order` in scripts/e2e-lib.sh asks real codex every row of tests/fixtures/codex-feature-flag-order.json), and in the gate this model is held to that same one-source matrix by its unit test below.
/// Codex's `plugins` feature as an argv resolves it, by the precedence real
/// codex applies: on when nothing names it; every `-c features.plugins=<bool>`
/// in argv order, the last one winning; then any `--enable plugins`; then any
/// `--disable plugins` — both toggles land after every `-c`, wherever they sit,
/// and `--disable` beats `--enable`. Only those three spellings are modelled;
/// `tests/fixtures/codex-feature-flag-order.json` holds the measured rows this
/// is tested against and `oh_codex_feature_flag_order` (`scripts/e2e-lib.sh`)
/// asks real codex.
fn codex_plugins_enabled(argv: &[String]) -> bool {
    codex_feature_enabled(argv, "plugins")
}

/// Any default-on Codex feature (`plugins`, `apps`) as an argv resolves it, by
/// the precedence [`codex_plugins_enabled`] states — the same for every
/// feature toggle, as `codex features list` shows for `apps` too.
fn codex_feature_enabled(argv: &[String], feature: &str) -> bool {
    let set = format!("features.{feature}=");
    let mut enabled = true;
    let (mut enable, mut disable) = (false, false);
    for pair in argv.windows(2) {
        match (pair[0].as_str(), pair[1].as_str()) {
            ("-c", value) if value.strip_prefix(set.as_str()) == Some("true") => enabled = true,
            ("-c", value) if value.strip_prefix(set.as_str()) == Some("false") => enabled = false,
            ("--enable", name) if name == feature => enable = true,
            ("--disable", name) if name == feature => disable = true,
            _ => {}
        }
    }
    (enabled || enable) && !disable
}
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] `catalog_fetched` follows the feature because spike-network-accounting measured exactly that on every launch path (6.9–32.2 MB a turn with plugins on, 0.74–1.33 MB with `--disable plugins`), and the live tier's `codex-launch-network-bytes` budget, which the `oneharness-live-network-budget` plan node registers, is the drift gate against real Codex; this double is not a second source of that fact. Codex's own plugin storage is not modelled.
/// `MOCK_CODEX_PLUGINS_LOG`: what a Codex process started with `argv` did about
/// plugins — whether it fetched the remote catalog, and which plugins installed
/// under its `CODEX_HOME` it loaded — appended as one JSON line. Only a launch
/// of a subcommand that starts Codex (`exec`, `app-server`) is recorded.
fn record_codex_plugins(path: &str, argv: &[String]) {
    let Some(subcommand) = argv
        .first()
        .filter(|first| matches!(first.as_str(), "exec" | "app-server"))
    else {
        return;
    };
    let enabled = codex_plugins_enabled(argv);
    let loaded = if enabled {
        installed_codex_plugins()
    } else {
        Vec::new()
    };
    let line = serde_json::json!({
        "subcommand": subcommand,
        "plugins": enabled,
        "catalog_fetched": enabled,
        "loaded": loaded,
    });
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("MOCK_CODEX_PLUGINS_LOG could not be opened");
    writeln!(log, "{line}").expect("MOCK_CODEX_PLUGINS_LOG could not be written");
}
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] This layout is not a claim about where real Codex keeps plugins: it is this double's own fixture convention, and its one other reader is the e2e helper that installs a plugin under it (`launch_codex` in `crates/oneharness-e2e/tests/cli.rs`), whose journeys fail if the two disagree.
/// The plugins installed under `$CODEX_HOME/plugins/`, one directory each and
/// sorted; any other file there is not a plugin. No `plugins/` directory is no
/// plugins installed. A launch that loads plugins with no `CODEX_HOME` would
/// read the real `~/.codex`, and an unreadable directory or entry would read as
/// none installed, so each stops the mock rather than record a launch nobody
/// set up.
fn installed_codex_plugins() -> Vec<String> {
    let Some(home) = std::env::var_os("CODEX_HOME") else {
        refuse("MOCK_CODEX_PLUGINS_LOG loads plugins from CODEX_HOME, which is unset");
    };
    // Listing and reading the entries is one fallible step, so a failure part
    // way through the listing takes the same refusal as one opening it.
    let listed =
        std::fs::read_dir(std::path::Path::new(&home).join("plugins")).and_then(|entries| {
            // MOCK_CODEX_PLUGINS_LIST_FAULT: one entry after the real ones fails to
            // read, the fault no real directory can be made to raise on demand.
            let fault = mock_var_os("MOCK_CODEX_PLUGINS_LIST_FAULT")
                .map(|_| Err(std::io::Error::other("injected listing fault")));
            entries.chain(fault).collect::<std::io::Result<Vec<_>>>()
        });
    let entries = match listed {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => refuse(&format!("$CODEX_HOME/plugins could not be read: {err}")),
    };
    // An entry whose type cannot be read is refused rather than read as no
    // plugin, the way `Path::is_dir` would.
    let mut loaded: Vec<String> = entries
        .into_iter()
        .filter(|entry| match std::fs::metadata(entry.path()) {
            Ok(metadata) => metadata.is_dir(),
            // Named by its file name alone: the full path carries the scratch
            // directory's pid, which a `401`/`403` in it reads as an auth refusal.
            Err(err) => refuse(&format!(
                "$CODEX_HOME/plugins entry `{}` could not be read: {err}",
                entry.file_name().to_string_lossy()
            )),
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    loaded.sort();
    loaded
}
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

/// Stop the mock the way a panic would — its message on stderr, exit 101 —
/// but without a panic's `file:line`, whose digits shift with every edit to
/// this file and, as a `401` or `403`, read as an auth refusal.
fn refuse(message: &str) -> ! {
    let _ = writeln!(std::io::stderr(), "mock harness: {message}");
    std::process::exit(101);
}

/// `--version`, answered with no other effect: `codex-cli <MOCK_CODEX_VERSION>`
/// when that is set, else MOCK_STDOUT (else the default document).
fn answer_version(argv: &[String]) -> ! {
    if let Some(ms) = millis_knob("MOCK_CODEX_VERSION_DELAY_MS") {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Ok(path) = std::env::var("MOCK_ARGV_FILE") {
        let _ = std::fs::write(path, argv.join("\n"));
    }
    let answer = mock_var("MOCK_CODEX_VERSION").map_or_else(
        |_| std::env::var("MOCK_STDOUT").unwrap_or_else(|_| "{\"result\":\"mock ok\"}".into()),
        |version| format!("codex-cli {version}\n"),
    );
    // CODEX_MOCK_FAULT=vary-version: a different release on every ask, so no
    // two dispatches can share a server.
    let answer = if std::env::var("CODEX_MOCK_FAULT").as_deref() == Ok("vary-version") {
        format!("{} build-{}\n", answer.trim_end(), std::process::id())
    } else {
        answer
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(answer.as_bytes());
    let _ = out.flush();
    let code = mock_var("MOCK_CODEX_VERSION_EXIT").or_else(|_| std::env::var("MOCK_EXIT"));
    std::process::exit(match code {
        Err(_) => 0,
        Ok(code) => code.parse().unwrap_or_else(|_| {
            refuse(&format!(
                "`--version`'s exit code must be a number, got `{code}`"
            ))
        }),
    });
}

/// A milliseconds knob: unset is `None`, and anything but a whole number stops
/// the mock, so a mistyped knob cannot quietly drive a different scenario.
fn millis_knob(name: &str) -> Option<u64> {
    let text = mock_var(name).ok()?;
    match text.parse() {
        Ok(ms) => Some(ms),
        Err(_) => refuse(&format!("{name} must be whole milliseconds, got `{text}`")),
    }
}

/// The last `-c model_catalog_json=<value>` in `argv`, decoded the way Codex
/// reads a `-c` value: as TOML (through the same `toml` parser oneharness
/// writes it with, so a literal `'…'`, a basic `"…"` and every escape decode as
/// they would in Codex), else as the raw text, since Codex reads a `-c` value
/// that is no TOML as a string. A quoted value that is no TOML string stops the
/// mock rather than standing for a path with its quotes in it.
fn codex_model_catalog_arg(argv: &[String]) -> Option<String> {
    let value = argv
        .windows(2)
        .filter(|pair| pair[0] == "-c")
        .filter_map(|pair| pair[1].strip_prefix("model_catalog_json="))
        .next_back()?;
    let parsed = toml::from_str::<toml::Table>(&format!("value = {value}"))
        .ok()
        .and_then(|table| table.get("value")?.as_str().map(str::to_string));
    if let Some(path) = parsed {
        return Some(path);
    }
    if value.starts_with(['"', '\'']) {
        refuse(&format!(
            "model_catalog_json `{value}` is quoted but no TOML string"
        ));
    }
    Some(value.to_string())
}

fn epoch_ms() -> u64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is past the epoch");
    u64::try_from(since.as_millis()).expect("milliseconds fit")
}

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] The cache's shape (`fetched_at`, `client_version`, `models`) is real Codex's `models_cache.json` as read on this repository's live host; its one other reader is `domain::model_catalog::read_codex_cache`, whose unit tests pin the same fields, and the refresh journeys fail if the two disagree. The fetch itself stands in for Codex's request to its models endpoint, which the live network breakdown (`scripts/codex-network-breakdown.py`) measures.
/// MOCK_CODEX_CATALOG_LOG: what a Codex process started with `argv` did about
/// its model catalog, appended as one JSON line. Only a launch of a subcommand
/// that starts Codex (`exec`, `app-server`) is recorded.
fn codex_model_catalog(path: &str, argv: &[String]) {
    let Some(subcommand) = argv
        .first()
        .filter(|first| matches!(first.as_str(), "exec" | "app-server"))
    else {
        return;
    };
    let started_ms = epoch_ms();
    // Codex's own default when CODEX_HOME is unset: `.codex` in the user's home.
    let home = std::env::var("CODEX_HOME").unwrap_or_else(|_| {
        let user = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .expect("MOCK_CODEX_CATALOG_LOG needs CODEX_HOME or a home directory for ~/.codex");
        std::path::Path::new(&user)
            .join(".codex")
            .display()
            .to_string()
    });
    let catalog = codex_model_catalog_arg(argv);
    let mut line = serde_json::json!({
        "subcommand": subcommand,
        "pid": std::process::id(),
        "started_ms": started_ms,
        "codex_home": home,
        "catalog": catalog,
        "fetch": null,
        "apps": codex_feature_enabled(argv, "apps"),
        "argv": argv,
    });
    let fault = mock_var("MOCK_CODEX_CATALOG_FAULT").unwrap_or_default();
    if !matches!(fault.as_str(), "" | "fail" | "hang" | "exit") {
        refuse(&format!(
            "MOCK_CODEX_CATALOG_FAULT must be `fail`, `hang` or `exit`, got `{fault}`"
        ));
    }
    if let Some(catalog) = &catalog {
        let models = std::fs::read_to_string(catalog)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value["models"].as_array().map(Vec::len))
            .filter(|count| *count > 0);
        let Some(models) = models else {
            let _ = writeln!(
                std::io::stderr(),
                "mock codex: model_catalog_json `{catalog}` is missing or has no models"
            );
            std::process::exit(1);
        };
        line["catalog_models"] = models.into();
    } else {
        if let Some(ms) = millis_knob("MOCK_CODEX_CATALOG_FETCH_MS") {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        line["fetch"] = match fault.as_str() {
            "fail" | "exit" => "failed".into(),
            "hang" => "hung".into(),
            _ => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("the clock is past the epoch");
                let fetched_at = format!(
                    "{}.{:09}Z",
                    oneharness_core::domain::history::format_rfc3339(
                        i64::try_from(now.as_secs()).expect("seconds fit")
                    )
                    .trim_end_matches('Z'),
                    now.subsec_nanos()
                );
                let version = mock_var("MOCK_CODEX_CACHE_VERSION")
                    .or_else(|_| mock_var("MOCK_CODEX_VERSION"))
                    .unwrap_or_else(|_| "0.0.0".into());
                let mut cache = serde_json::json!({
                    "fetched_at": fetched_at,
                    "etag": "W/\"mock\"",
                    "client_version": version,
                    "models": [
                        {"slug": "mock-model-a", "display_name": "Mock A"},
                        {"slug": "mock-model-b", "display_name": "Mock B"},
                    ],
                });
                let corrupt = mock_var("MOCK_CODEX_CACHE_CORRUPT").unwrap_or_default();
                match corrupt.as_str() {
                    "" | "json" => {}
                    "time" => cache["fetched_at"] = "not a time".into(),
                    "empty" => cache["models"] = serde_json::json!([]),
                    "models" => cache["models"] = serde_json::json!([null]),
                    other => refuse(&format!(
                        "MOCK_CODEX_CACHE_CORRUPT must be `json`, `time`, `empty` or `models`, got `{other}`"
                    )),
                }
                let text = if corrupt == "json" {
                    "{\"models\":".to_string()
                } else {
                    cache.to_string()
                };
                let cache_path = std::path::Path::new(&home).join("models_cache.json");
                let temp = cache_path.with_extension(format!("{}.tmp", std::process::id()));
                std::fs::write(&temp, text)
                    .and_then(|()| std::fs::rename(&temp, &cache_path))
                    .expect("the mock's models_cache.json could not be written");
                line["fetched_ms"] = u64::try_from(now.as_millis())
                    .expect("milliseconds fit")
                    .into();
                "ok".into()
            }
        };
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("MOCK_CODEX_CATALOG_LOG could not be opened");
    // One write per line, so launches appending at once never interleave.
    log.write_all(format!("{line}\n").as_bytes())
        .expect("MOCK_CODEX_CATALOG_LOG could not be written");
    drop(log);
    if fault == "exit" && catalog.is_none() && subcommand == "app-server" {
        std::process::exit(1);
    }
    if mock_var_os("MOCK_CODEX_CATALOG_THEN_EXIT").is_some()
        && catalog.is_none()
        && subcommand == "app-server"
    {
        std::process::exit(0);
    }
    if fault == "hang" && catalog.is_none() && subcommand == "app-server" {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        std::process::exit(0);
    }
}
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

/// The stdout scripted for this invocation: MOCK_STDOUT_<attempt> when the
/// attempt is counted and that knob is set, else MOCK_STDOUT, else `default`.
fn scripted_stdout(attempt: Option<u32>, default: &str) -> String {
    attempt
        .and_then(|n| std::env::var(format!("MOCK_STDOUT_{n}")).ok())
        .or_else(|| std::env::var("MOCK_STDOUT").ok())
        .unwrap_or_else(|| default.to_string())
}

/// Read `wanted` newline-terminated request lines from stdin, then keep
/// listening for a window, and report whether stdin reached EOF.
///
/// Reading on a thread is what makes both of those observable at once: a caller
/// may hold stdin open for as long as it waits for an answer, so a blocking read
/// past the last request would never return, and yet the *absence* of an extra
/// line and the arrival of EOF are each something a test has to be able to see.
/// The window is `delay` when a reply delay was asked for (the caller's EOF has
/// to be noticed inside it) and otherwise a short grace when the requests are
/// being logged.
fn read_requests(wanted: usize, delay: u64, recording: bool) -> (Vec<String>, bool) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        loop {
            let mut line = String::new();
            match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if sender
                        .send(line.trim_end_matches(['\r', '\n']).to_string())
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    });

    let mut requests = Vec::new();
    let mut saw_eof = false;
    while requests.len() < wanted && !saw_eof {
        match receiver.recv() {
            Ok(line) => requests.push(line),
            Err(_) => saw_eof = true,
        }
    }

    let window = if delay > 0 {
        std::time::Duration::from_millis(delay)
    } else if recording {
        EXTRA_REQUEST_GRACE
    } else {
        std::time::Duration::ZERO
    };
    let deadline = std::time::Instant::now() + window;
    while !saw_eof {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        match receiver.recv_timeout(remaining) {
            Ok(line) => requests.push(line),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => saw_eof = true,
        }
    }
    (requests, saw_eof)
}

/// Return the value immediately following `flag`, matching the simple
/// flag/value shapes emitted by every adapter that selects an output format.
fn arg_value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|arg| arg == flag)
        .and_then(|index| argv.get(index + 1))
        .map(String::as_str)
}

/// Remove top-level session handles from a JSON document or JSONL transcript.
/// The fake harness is shared by every adapter, so its scripted stdout must not
/// expose an id in an argv mode where the real harness would omit it.
fn without_session_fields(stdout: &str, fields: &[&str]) -> String {
    fn remove(value: &mut serde_json::Value, fields: &[&str]) {
        match value {
            serde_json::Value::Object(object) => {
                for field in fields {
                    object.remove(*field);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    remove(value, fields);
                }
            }
            _ => {}
        }
    }

    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        remove(&mut value, fields);
        return serde_json::to_string(&value).unwrap_or_else(|_| stdout.to_string());
    }

    stdout
        .lines()
        .map(|line| {
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                return line.to_string();
            };
            remove(&mut value, fields);
            serde_json::to_string(&value).unwrap_or_else(|_| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Make scripted session output obey the real harness's format contract. In
/// particular, Codex emits `thread_id` only under `exec --json`, and Qwen emits
/// `session_id` only in its JSON output modes. The remaining adapters are kept
/// honest here too so future session tests cannot accidentally prove a text-only
/// invocation captured an id that the real CLI would never print.
fn session_faithful_stdout(argv: &[String], stdout: String) -> String {
    let is_codex = argv.first().is_some_and(|arg| arg == "exec");
    if is_codex {
        return if argv.iter().any(|arg| arg == "--json") {
            without_session_fields(&stdout, &["session_id", "sessionID"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let format = arg_value_after(argv, "--output-format");
    let is_qwen = argv
        .iter()
        .any(|arg| arg == "--approval-mode" || arg == "--yolo");
    if is_qwen {
        return if matches!(format, Some("json" | "stream-json")) {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let is_claude = argv.iter().any(|arg| arg == "--permission-mode");
    if is_claude {
        return if matches!(format, Some("json" | "stream-json")) {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let is_cursor = argv.first().is_some_and(|arg| arg == "-p")
        && argv.iter().any(|arg| arg == "--force" || arg == "--trust")
        && argv.iter().any(|arg| arg == "--output-format");
    if is_cursor {
        return if format == Some("stream-json") {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let opencode_format = arg_value_after(argv, "--format");
    let is_opencode = argv.first().is_some_and(|arg| arg == "run") && opencode_format.is_some();
    if is_opencode {
        return if opencode_format == Some("json") {
            without_session_fields(&stdout, &["session_id", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let cannot_emit_session = (argv.first().is_some_and(|arg| arg == "run")
        && argv
            .iter()
            .any(|arg| arg == "--with-builtin" || arg == "-q"))
        || argv.iter().any(|arg| arg == "--no-ask-user");
    if cannot_emit_session {
        return without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"]);
    }

    stdout
}

#[cfg(windows)]
fn run_native_descendant() -> ! {
    let run_for = std::env::var("MOCK_NATIVE_GRANDCHILD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let mut tick_file = std::env::var("MOCK_TICK_FILE").ok().and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    });
    let mut tick = || {
        if let Some(file) = tick_file.as_mut() {
            let _ = file.write_all(b"x");
            let _ = file.flush();
        }
    };

    // Write the durable witness before the first stream line. A streaming
    // consumer can stop on that line and still prove this descendant existed.
    tick();
    if let Ok(text) = std::env::var("MOCK_STDOUT") {
        let _ = std::io::stdout().write_all(text.as_bytes());
        let _ = std::io::stdout().flush();
    }
    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = std::io::stderr().write_all(text.as_bytes());
        let _ = std::io::stderr().flush();
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(run_for);
    while std::time::Instant::now() < deadline {
        tick();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::process::exit(0);
}

/// How the mock control server is asked to break, parsed at the boundary.
///
/// An enum rather than the raw string: an unrecognized value read as "no fault"
/// would turn a misspelled knob into a passing happy-path run, which is the one
/// outcome a fault test must never quietly produce.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HttpControlFault {
    None,
    NeverReady,
    FirstCandidateDies,
    LosePort,
    SilentServer,
    RefuseSession,
    NoSessionId,
    ForeignPermission,
    CloseStream,
    RefusePrompt,
    HangPrompt,
    SlowTurn,
    RefuseEvents,
    UnreadableEvents,
    RedirectInterrupt,
    RedirectedTurn,
    RefuseRedirect,
}

impl HttpControlFault {
    fn from_env() -> Self {
        match std::env::var("MOCK_HTTP_CONTROL_FAULT")
            .unwrap_or_default()
            .as_str()
        {
            "" => HttpControlFault::None,
            "never-ready" => HttpControlFault::NeverReady,
            "first-candidate-dies" => HttpControlFault::FirstCandidateDies,
            "lose-port" => HttpControlFault::LosePort,
            "silent-server" => HttpControlFault::SilentServer,
            "refuse-session" => HttpControlFault::RefuseSession,
            "no-session-id" => HttpControlFault::NoSessionId,
            "foreign-permission" => HttpControlFault::ForeignPermission,
            "close-stream" => HttpControlFault::CloseStream,
            "refuse-prompt" => HttpControlFault::RefusePrompt,
            "hang-prompt" => HttpControlFault::HangPrompt,
            "slow-turn" => HttpControlFault::SlowTurn,
            "refuse-events" => HttpControlFault::RefuseEvents,
            "unreadable-events" => HttpControlFault::UnreadableEvents,
            "redirect-interrupt" => HttpControlFault::RedirectInterrupt,
            "redirected-turn" => HttpControlFault::RedirectedTurn,
            "refuse-redirect" => HttpControlFault::RefuseRedirect,
            other => panic!("mock harness: MOCK_HTTP_CONTROL_FAULT names no fault: `{other}`"),
        }
    }
}

/// The largest request body the mock control server will reserve room for. A
/// control request is a small JSON object; the bound exists because the number
/// that sizes the allocation arrives on the socket.
const MAX_REQUEST_BODY: usize = 1 << 20;

/// The largest request line or header line it will hold. Both end at a newline
/// the *peer* sends, so without a bound the peer chooses how much this fixture
/// accumulates before it has validated anything at all.
const MAX_HEAD_LINE: usize = 16 * 1024;

/// Read one head line into `line`, or `None` when it outgrew [`MAX_HEAD_LINE`]
/// before ending. `Some(0)` is the end of the stream.
fn read_bounded_line<R: std::io::BufRead>(reader: &mut R, line: &mut String) -> Option<usize> {
    use std::io::Read;
    match std::io::BufRead::read_line(&mut reader.take(MAX_HEAD_LINE as u64), line) {
        Ok(read) if read == MAX_HEAD_LINE && !line.ends_with('\n') => None,
        Ok(read) => Some(read),
        Err(_) => Some(0),
    }
}

/// Record that a control server launch happened and which fault it was serving,
/// so a test can count the launches a dispatch made. The line is all a fault
/// that exits before binding ever gets to say.
fn note_launch(log_path: &str, fault: &str) {
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(log, "LAUNCHED {fault}");
    }
}

/// Act like an opencode-shaped HTTP control server: the third execution model,
/// where the turn is submitted to a server rather than to a CLI.
///
/// Reproduces the two behaviors that decide whether the model works at all: the
/// server blocks on a permission request until the client answers it, and it
/// announces `session.idle` BEFORE the prompt is admitted as well as after the
/// turn ends — so a driver that reads the first one ends the run having done
/// nothing. The port comes off the argv the pool launched it with, exactly as
/// `opencode serve --port N` takes it.
fn run_http_control_server(log_path: &str) -> ! {
    use std::io::Read;
    let args: Vec<String> = std::env::args().collect();
    let fault = HttpControlFault::from_env();
    // A server the pool started and that then never listens — the shape a real
    // one takes when it dies on startup. Exiting before binding is what makes
    // the bring-up, not a route, the thing under test. The launch is logged
    // first, and it is the only thing this fault ever writes: a dispatch that
    // relaunches a server which died leaves one line per attempt, so the log
    // counts the attempts.
    if fault == HttpControlFault::NeverReady {
        note_launch(log_path, "never-ready");
        std::process::exit(0);
    }
    // A server that lost its reserved port to whoever asked the kernel next.
    // The port is reserved by binding and letting go, so the loser dies on
    // `EADDRINUSE` through no fault of its own — and only the FIRST launch
    // does: the relaunch it is owed comes up healthy, which is the whole
    // behavior under test. The log is what tells the two launches apart, since
    // nothing else survives the exit.
    // A server that will not come up for the FIRST candidate of a chain and does
    // for the one behind it. A candidate is owed one relaunch, so it spends the
    // first two launches; the third is the next candidate's and serves normally.
    // Counted out of the shared log because the pool launches these, and a
    // per-candidate environment does not reach a process the pool starts.
    let fault = if fault == HttpControlFault::FirstCandidateDies {
        let launched = std::fs::read_to_string(log_path)
            .unwrap_or_default()
            .lines()
            .filter(|line| *line == "LAUNCHED first-candidate-dies")
            .count();
        if launched < 2 {
            note_launch(log_path, "first-candidate-dies");
            std::process::exit(0);
        }
        HttpControlFault::None
    } else {
        fault
    };
    let fault = if fault == HttpControlFault::LosePort {
        if std::fs::read_to_string(log_path)
            .unwrap_or_default()
            .contains("LAUNCHED lose-port")
        {
            HttpControlFault::None
        } else {
            note_launch(log_path, "lose-port");
            std::process::exit(1);
        }
    } else {
        fault
    };
    // The pool dials the port it put on the argv, so an absent or unreadable
    // one is refused rather than defaulted: binding `0` would listen on some
    // port nobody is going to connect to, which reads as a server that never
    // came up rather than as a launch that was wrong.
    // `0` is refused with the rest: it parses as a `u16` but names no address —
    // the kernel picks one, and the dispatch goes on dialing the port that is
    // written down. Same rule the real [`crate::domain::control::Port`] holds.
    let port: u16 = match args.windows(2).find(|w| w[0] == "--port") {
        Some(pair) => pair[1]
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .unwrap_or_else(|| panic!("mock harness: `--port {}` is not a dialable port", pair[1])),
        None => panic!("mock harness: the control server was launched with no --port"),
    };
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .expect("the mock control server could not bind its port");
    // A server that is there and says nothing. It holds the port for the whole
    // readiness window — so the address is demonstrably ITS own, not a
    // stranger's — accepts each dial and closes it without answering, then
    // exits once the caller has stopped dialing so nothing is left for the pool
    // to reclaim. Both waits are bounded; neither polls open-endedly.
    if fault == HttpControlFault::SilentServer {
        note_launch(log_path, "silent-server");
        listener
            .set_nonblocking(true)
            .expect("the mock control server could not poll its listener");
        let started = std::time::Instant::now();
        let mut last_seen = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(30)
            && last_seen.elapsed() < std::time::Duration::from_millis(1500)
        {
            match listener.accept() {
                // Closed without a byte of answer: a definite, immediate error
                // for the dialer every time, so the window is the only clock.
                Ok((socket, _)) => {
                    last_seen = std::time::Instant::now();
                    drop(socket);
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        std::process::exit(0);
    }
    let log = std::sync::Arc::new(std::sync::Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("the mock control server could not open its log"),
    ));
    // Shared turn state: whether the prompt was admitted and whether the turn
    // has been aborted. The event stream reads both.
    let admitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let aborted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // How many prompts this session has taken. A redirection makes a SECOND
    // one, which is the whole thing the redirect fault exercises.
    let prompts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for connection in listener.incoming() {
        let Ok(mut socket) = connection else { continue };
        let log = std::sync::Arc::clone(&log);
        let admitted = std::sync::Arc::clone(&admitted);
        let aborted = std::sync::Arc::clone(&aborted);
        let prompts = std::sync::Arc::clone(&prompts);
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(socket.try_clone().expect("clone"));
            let mut request_line = String::new();
            match read_bounded_line(&mut reader, &mut request_line) {
                Some(0) | None => {
                    // Nothing, or more of a request line than this fixture will
                    // hold before it has even seen the end of it.
                    if request_line.len() >= MAX_HEAD_LINE {
                        reply_bad_request(&mut socket);
                    }
                    return;
                }
                Some(_) => {}
            }
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                match read_bounded_line(&mut reader, &mut header) {
                    Some(0) => break,
                    // A header line with no ending in it is the same hazard as
                    // an endless request line, and refused the same way.
                    None => {
                        reply_bad_request(&mut socket);
                        return;
                    }
                    Some(_) => {}
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    // A length that cannot be read is not a zero-length body:
                    // reading none of a body that is there leaves the next
                    // request's bytes in the stream.
                    let Ok(declared) = value.trim().parse::<usize>() else {
                        reply_bad_request(&mut socket);
                        return;
                    };
                    // The declaration is a number off a socket, and it is about
                    // to size an allocation. A control request is a small JSON
                    // object, so anything past the bound is refused rather than
                    // reserved — otherwise the peer, not this fixture, decides
                    // how much memory it commits and how long it then blocks
                    // waiting for a body that is never coming.
                    if declared > MAX_REQUEST_BODY {
                        reply_bad_request(&mut socket);
                        return;
                    }
                    length = declared;
                }
                if header == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let line = request_line.trim().to_string();
            {
                let mut file = log.lock().unwrap_or_else(|e| e.into_inner());
                let _ = writeln!(file, "{line} {}", String::from_utf8_lossy(&body));
                let _ = file.flush();
            }
            let reply = |socket: &mut std::net::TcpStream, status: &str, body: &str| {
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.flush();
            };
            if line.starts_with("GET /api/event") {
                if fault == HttpControlFault::RefuseEvents {
                    reply(
                        &mut socket,
                        "503 Service Unavailable",
                        "{\"error\":\"events unavailable\"}",
                    );
                    exit_shortly();
                    return;
                }
                if fault == HttpControlFault::UnreadableEvents {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 7\r\nContent-Length: 9\r\n\r\ndata: {{}}\n"
                    );
                    let _ = socket.flush();
                    exit_shortly();
                    return;
                }
                // The stream a driver follows. The idle BEFORE admission is the
                // trap: acting on it ends the run before any work happens.
                let send = |socket: &mut std::net::TcpStream, payload: &str| {
                    let _ = write!(socket, "data: {payload}\n\n");
                    let _ = socket.flush();
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
                );
                send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                let mut asked = false;
                loop {
                    if admitted.load(std::sync::atomic::Ordering::SeqCst) && !asked {
                        asked = true;
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.prompt.admitted\",\"data\":{}}",
                        );
                        // A server that stops talking mid-turn: the stream ends
                        // with no end-of-turn event on it at all.
                        if fault == HttpControlFault::CloseStream {
                            exit_shortly();
                            return;
                        }
                        // The shared stream carries every session's asks, so a
                        // faulted server asks about one this run does not own.
                        // Nothing may answer it — and the turn then ends on its
                        // own, rather than leaving the run to its timeout.
                        if fault == HttpControlFault::ForeignPermission {
                            send(
                                &mut socket,
                                "{\"type\":\"permission.requested\",\"data\":{\"id\":\"per_1\",\"sessionID\":\"ses_intruder\"}}",
                            );
                            std::thread::sleep(std::time::Duration::from_millis(400));
                            send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                            exit_shortly();
                            return;
                        }
                        send(
                            &mut socket,
                            "{\"type\":\"permission.requested\",\"data\":{\"id\":\"per_1\",\"sessionID\":\"ses_mock\"}}",
                        );
                        if fault == HttpControlFault::SlowTurn {
                            std::thread::sleep(std::time::Duration::from_millis(1500));
                            send(
                                &mut socket,
                                "{\"type\":\"session.next.text.ended\",\"data\":{\"text\":\"slow controlled answer\"}}",
                            );
                            send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                            exit_shortly();
                            return;
                        }
                    }
                    if aborted.load(std::sync::atomic::Ordering::SeqCst) {
                        // A refused redirection ends the run through the
                        // submission, not the stream: the stream stays silent
                        // exactly as opencode's does after an abort.
                        if fault == HttpControlFault::RefuseRedirect {
                            std::thread::sleep(std::time::Duration::from_secs(30));
                            return;
                        }
                        if fault != HttpControlFault::RedirectedTurn {
                            send(
                                &mut socket,
                                "{\"type\":\"session.next.text.ended\",\"data\":{\"text\":\"stopped\"}}",
                            );
                            send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                            return;
                        }
                        // Opencode's real shape, measured live: an aborted turn
                        // ends SILENTLY. No text, no idle — the stream just
                        // stops until something else happens on the session. A
                        // driver holding a redirection for an end-of-turn event
                        // would wait here until its timeout, so the served
                        // interrupt has to be what releases it.
                        while prompts.load(std::sync::atomic::Ordering::SeqCst) < 2 {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.prompt.admitted\",\"data\":{}}",
                        );
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.text.ended\",\"data\":{\"text\":\"redirected\"}}",
                        );
                        send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                        exit_shortly();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            if line.starts_with("POST /api/session/") && line.contains("/interrupt") {
                // A route that answers "ask elsewhere" has not aborted
                // anything: the turn is deliberately left running, so a client
                // that read a redirect as acceptance would report a stop that
                // never happened.
                if fault == HttpControlFault::RedirectInterrupt {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 302 Found\r\nLocation: /v2/session/ses_mock/interrupt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.flush();
                    exit_shortly();
                    return;
                }
                aborted.store(true, std::sync::atomic::Ordering::SeqCst);
                reply(&mut socket, "204 No Content", "");
                // The redirect fault has a second turn to serve, so the server
                // stays up until the event stream has carried it.
                if !matches!(
                    fault,
                    HttpControlFault::RedirectedTurn | HttpControlFault::RefuseRedirect
                ) {
                    exit_shortly();
                }
            } else if line.starts_with("POST /api/session/") && line.contains("/prompt") {
                match fault {
                    HttpControlFault::RefusePrompt => {
                        reply(
                            &mut socket,
                            "503 Service Unavailable",
                            "{\"error\":\"model unavailable\"}",
                        );
                        exit_shortly();
                    }
                    HttpControlFault::HangPrompt => {
                        // Keep the request blocked past the run's one-second
                        // budget, then let the fixture process exit cleanly so
                        // its coverage profile is complete.
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            std::process::exit(0);
                        });
                        std::thread::sleep(std::time::Duration::from_secs(120));
                    }
                    // The redirected prompt is the one refused: the first
                    // turn runs normally so there is something to interrupt,
                    // and the SECOND submission — the redirection — is what the
                    // server will not take. A run that swallowed that would
                    // report success having done none of the redirected work.
                    HttpControlFault::RefuseRedirect => {
                        let seq = prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        if seq == 1 {
                            admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                            while !aborted.load(std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(25));
                            }
                            reply(
                                &mut socket,
                                "409 Conflict",
                                "{\"error\":\"the turn was aborted\"}",
                            );
                        } else {
                            reply(
                                &mut socket,
                                "503 Service Unavailable",
                                "{\"error\":\"model unavailable\"}",
                            );
                            exit_shortly();
                        }
                    }
                    HttpControlFault::RedirectedTurn => {
                        let seq = prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                        if seq == 1 {
                            // Opencode holds the prompt request open for the
                            // whole turn and answers it with a REFUSAL when the
                            // turn is aborted. A driver that reads that refusal
                            // as the run's outcome stops before the redirection
                            // it accepted can become the next turn — the message
                            // lost by another route.
                            while !aborted.load(std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(25));
                            }
                            reply(
                                &mut socket,
                                "409 Conflict",
                                "{\"error\":\"the turn was aborted\"}",
                            );
                        } else {
                            reply(&mut socket, "200 OK", "{\"data\":{\"admittedSeq\":2}}");
                        }
                    }
                    _ => {
                        prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                        reply(&mut socket, "200 OK", "{\"data\":{\"admittedSeq\":1}}");
                    }
                }
            } else if line.starts_with("POST /api/session/") && line.contains("/permission/") {
                let mut file = log.lock().unwrap_or_else(|e| e.into_inner());
                let _ = writeln!(file, "PERMISSION_ANSWERED");
                let _ = file.flush();
                reply(&mut socket, "200 OK", "{}");
            } else if line.starts_with("POST /api/session") {
                // The two ways session creation fails against a server that IS
                // up: it refuses, or it answers something with no id in it.
                // Either way the run has no turn to drive, and the server then
                // exits rather than lingering as a pooled orphan — a process
                // reclaimed with SIGTERM leaves a truncated coverage profile.
                match fault {
                    HttpControlFault::RefuseSession => {
                        reply(
                            &mut socket,
                            "503 Service Unavailable",
                            "{\"error\":\"no provider configured\"}",
                        );
                        exit_shortly();
                    }
                    HttpControlFault::NoSessionId => {
                        reply(&mut socket, "200 OK", "{\"data\":{\"kind\":\"session\"}}");
                        exit_shortly();
                    }
                    _ => reply(&mut socket, "200 OK", "{\"data\":{\"id\":\"ses_mock\"}}"),
                }
            } else {
                reply(&mut socket, "200 OK", "{}");
            }
        });
    }
    std::process::exit(0);
}

/// Refuse a request whose framing this fixture could not read. Answering keeps
/// the client from waiting out a timeout on a connection nobody will write to.
fn reply_bad_request(socket: &mut std::net::TcpStream) {
    let _ = write!(
        socket,
        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = socket.flush();
}

/// Shut the mock control server down once it has served what it was standing up
/// for, rather than waiting to be reclaimed: the pool stops a server with
/// SIGTERM, whose default disposition skips the at-exit handlers — including the
/// one that writes this binary's coverage profile, leaving a truncated file that
/// fails the whole coverage merge. The delay lets the answer just written (and,
/// for an aborted turn, its final events) reach the client first.
fn exit_shortly() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::process::exit(0);
    });
}

/// Act like `codex app-server`: the JSON-RPC protocol a codex turn is driven
/// over, including the fact that decides whether a controlled run works at all.
///
/// `turn/start` answers IMMEDIATELY with the new turn's id and
/// `status:"inProgress"` — a client that reads that response as the end of the
/// turn finishes in under half a second having done nothing. The turn ends only
/// on the `turn/completed` notification, which this emits once the interrupt
/// naming both the thread and the turn arrives.
fn run_codex_app_server(log_path: &str) -> ! {
    use serde_json::{json, Value};
    let attempt = scripted_attempt();
    let overload_attempts = mock_var("MOCK_CODEX_OVERLOAD_ATTEMPTS")
        .ok()
        .map(|text| {
            text.parse::<u32>()
                .expect("MOCK_CODEX_OVERLOAD_ATTEMPTS must be an unsigned integer")
        })
        .unwrap_or(0);
    // Opened once, up front: a log path that cannot be written is a fixture
    // that answers correctly while recording nothing, which reads as a client
    // that never sent the frames the test is looking for.
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("the mock app-server could not open its log");
    let mut append = |line: &str| {
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
    };
    let mut out = std::io::stdout();
    let mut send = |value: &Value| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };

    for line in std::io::BufRead::lines(std::io::stdin().lock()) {
        let Ok(line) = line else { break };
        append(&line);
        // A real frame or nothing: the fixture answers what it can parse rather
        // than scanning the text for fields that may not be there.
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => send(&json!({"jsonrpc": "2.0", "id": id, "result": {}})),
            Some("thread/start") => send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": open_response("mock-codex-thread", &message),
            })),
            // Rejoining the thread the client named, exactly as the real
            // app-server does: `ThreadResumeResponse` carries the same required
            // `thread` field `ThreadStartResponse` does, so the client reads the
            // conversation's id off one arm either way. Echoing the REQUESTED id
            // is what makes a resumed turn distinguishable from a fresh one in
            // the report — a fixture that minted a new id would hide exactly the
            // bug this exists to catch.
            Some("thread/resume") => {
                // `threadId` is the one REQUIRED field of `ThreadResumeParams`,
                // so a request without a readable one is refused rather than
                // answered with an empty conversation — which would let a client
                // that named no thread pass for one that resumed the right one.
                let requested = message
                    .get("params")
                    .and_then(|params| params.get("threadId"))
                    .and_then(Value::as_str)
                    .filter(|thread| !thread.is_empty());
                let Some(requested) = requested else {
                    append("RESUME_UNADDRESSED");
                    send(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "thread/resume requires a threadId"},
                    }));
                    continue;
                };
                send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": open_response(requested, &message),
                }));
            }
            Some("turn/start") => {
                send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"turn": {"id": "mock-codex-turn", "status": "inProgress"}},
                }));
                if attempt.is_some_and(|attempt| attempt <= overload_attempts) {
                    if mock_var_os("MOCK_CODEX_OVERLOAD_AFTER_TOOL").is_some() {
                        for frame in
                            include_str!("../fixtures/codex-app-server-command-execution.jsonl")
                                .lines()
                        {
                            send(
                                &serde_json::from_str(frame)
                                    .expect("captured app-server frame is JSON"),
                            );
                        }
                    }
                    send(&json!({
                        "jsonrpc": "2.0",
                        "method": "error",
                        "params": {
                            "error": {
                                "message": "The server is overloaded",
                                "codexErrorInfo": "serverOverloaded"
                            },
                            "willRetry": false
                        }
                    }));
                    send(&json!({
                        "jsonrpc": "2.0",
                        "method": "turn/completed",
                        "params": {
                            "turn": {
                                "id": "mock-codex-turn",
                                "status": "failed",
                                "error": {
                                    "message": "The server is overloaded",
                                    "codexErrorInfo": "serverOverloaded"
                                }
                            }
                        }
                    }));
                    continue;
                }
                // The reply: a fixed "still working", or the turn's own input
                // text when MOCK_CODEX_ECHO_PROMPT asks for a turn that
                // answers what it was sent, as a model echoing a marker does.
                let reply = if mock_var_os("MOCK_CODEX_ECHO_PROMPT").is_some() {
                    message
                        .pointer("/params/input")
                        .and_then(Value::as_array)
                        .map(|input| {
                            input
                                .iter()
                                .filter_map(|item| item.get("text").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                } else {
                    "still working".to_string()
                };
                send(&json!({
                    "jsonrpc": "2.0",
                    "method": "item/agentMessage/delta",
                    "params": {"itemId": "item_1", "delta": reply},
                }));
                send(&json!({
                    "method": "item/completed",
                    "params": {"item": {"type": "agentMessage", "id": "item_1", "text": reply}},
                }));
                if mock_var_os("MOCK_CODEX_TOOL_EVENTS").is_some() {
                    let started_only = mock_var_os("MOCK_CODEX_TOOL_STARTED_ONLY").is_some();
                    for (index, frame) in
                        include_str!("../fixtures/codex-app-server-command-execution.jsonl")
                            .lines()
                            .enumerate()
                    {
                        if started_only && index > 0 {
                            break;
                        }
                        let mut frame: Value =
                            serde_json::from_str(frame).expect("captured app-server frame is JSON");
                        if mock_var_os("MOCK_CODEX_NON_STRING_COMMAND").is_some() {
                            frame["params"]["item"]["command"] = json!({"unexpected": "shape"});
                        }
                        send(&frame);
                    }
                }
                // A turn that ends on its own, for the exchanges that are about
                // what the client SENT rather than about interrupting it. The
                // default fixture holds the turn open until an interrupt
                // arrives, which a run nobody interrupts would wait out.
                if mock_var_os("MOCK_CODEX_COMPLETE_TURN").is_some() {
                    send(&json!({
                        "jsonrpc": "2.0",
                        "method": "turn/completed",
                        "params": {"turnId": "mock-codex-turn"},
                    }));
                }
            }
            // Only an interrupt that names BOTH coordinates stops the turn:
            // the real app-server takes `{threadId, turnId}`, and a fixture
            // that ends on the method alone would pass a client that addressed
            // no particular turn.
            Some("turn/interrupt") => {
                let params = message.get("params").unwrap_or(&Value::Null);
                let names = |key: &str, expected: &str| {
                    params.get(key).and_then(Value::as_str) == Some(expected)
                };
                if !names("threadId", "mock-codex-thread") || !names("turnId", "mock-codex-turn") {
                    append("INTERRUPT_MISADDRESSED");
                    send(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "invalid interrupt coordinates"},
                    }));
                    continue;
                }
                send(&json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                // A server that accepts the abort and then dies before saying
                // the turn ended. The client is left holding a redirection it
                // committed to a conversation that will never open its next
                // turn, which is the loss the run has to report.
                if mock_var_os("MOCK_CODEX_DIE_ON_INTERRUPT").is_some() {
                    append("DIED_AFTER_INTERRUPT");
                    std::process::exit(0);
                }
                send(&json!({
                    "jsonrpc": "2.0",
                    "method": "turn/completed",
                    "params": {"turnId": "mock-codex-turn"},
                }));
            }
            _ => {}
        }
    }
    std::process::exit(0);
}

/// The `result` of a `thread/start` / `thread/resume` answer: the thread under
/// `thread_id`, plus the model the server says it runs under unless
/// `MOCK_CODEX_OMIT_MODEL` withholds the claim altogether.
fn open_response(thread_id: &str, open_request: &serde_json::Value) -> serde_json::Value {
    let mut result = serde_json::json!({"thread": {"id": thread_id}});
    if mock_var_os("MOCK_CODEX_OMIT_MODEL").is_none() {
        result["model"] = serde_json::Value::String(served_model(open_request));
    }
    result
}

/// The model the mock app-server says a thread runs under: `MOCK_CODEX_MODEL`
/// when set (a server that runs something other than what was asked), else the
/// model the open request named, else a default — the real `ThreadStartResponse`
/// always carries one, and a fixture that omitted it would pass a client that
/// never read it.
fn served_model(open_request: &serde_json::Value) -> String {
    mock_var("MOCK_CODEX_MODEL").ok().unwrap_or_else(|| {
        open_request
            .get("params")
            .and_then(|params| params.get("model"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("mock-codex-default")
            .to_string()
    })
}

/// Act like an ACP server: the protocol `copilot --acp` and `goose acp` speak,
/// including the two behaviors that decide whether a turn works at all or
/// silently never starts — a mandatory `session/request_permission`, and a
/// cancel that is a notification rather than a request.
fn run_acp_server(log_path: &str) -> ! {
    let append = |line: &str| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };
    let mut out = std::io::stdout();
    let mut send = |value: &str| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };

    let mut prompt_id: Option<String> = None;
    let mut asked_permission = false;
    for line in std::io::BufRead::lines(std::io::stdin().lock()) {
        let Ok(line) = line else { break };
        append(&line);
        // A real frame or nothing: a fixture that scanned the text for its
        // fields would answer something that was never a JSON-RPC message.
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let id = message
            .get("id")
            .filter(|id| id.is_number())
            .map(ToString::to_string);
        let method = message
            .get("method")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        match method.as_deref() {
            Some("initialize") => send(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"protocolVersion\":1}}}}",
                id.unwrap_or_else(|| "1".to_string())
            )),
            Some("session/new") => send(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"sessionId\":\"mock-acp-session\"}}}}",
                id.unwrap_or_else(|| "2".to_string())
            )),
            Some("session/prompt") => {
                prompt_id = id.clone();
                // The turn does not begin until the client answers this.
                asked_permission = true;
                send(
                    "{\"jsonrpc\":\"2.0\",\"id\":9001,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"mock-acp-session\",\"options\":[{\"optionId\":\"allow\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"kind\":\"reject_once\"}]}}",
                );
            }
            // A cancel carries no `id`. The turn then ends reporting a NORMAL
            // stop reason — the lie oneharness must not read as the truth.
            Some("session/cancel") => {
                send("{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"mock-acp-session\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Info: Operation cancelled by user\"}}}}");
                if let Some(prompt_id) = prompt_id.take() {
                    send(&format!(
                        "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
                    ));
                }
                break;
            }
            _ => {
                // A permission answer arrives as a plain response; record it so
                // a test can assert the client actually sent one.
                if method.is_none() && id.is_some() && asked_permission {
                    append("PERMISSION_ANSWERED");
                    // A turn that ends on its own, for the exchanges that are
                    // about what the client SENT rather than about interrupting
                    // it. The default fixture ends only on a cancel, which a run
                    // nobody interrupts would wait out.
                    if std::env::var_os("MOCK_ACP_COMPLETE_TURN").is_some() {
                        if let Some(prompt_id) = prompt_id.take() {
                            send(&format!(
                                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }
    std::process::exit(0);
}

/// Act like a harness whose turn is driven over an open stdin message stream and
/// can be aborted out of band. Emits the in-turn transcript, then blocks reading
/// stdin — so the turn is genuinely still running while a separate process sends
/// the interrupt — and ends on either a control frame or EOF.
fn run_controlled_turn(log_path: &str) -> ! {
    let append = |line: &str| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };
    let mut out = std::io::stdout();
    if let Ok(stdout) = std::env::var("MOCK_STDOUT") {
        if !stdout.is_empty() {
            let _ = writeln!(out, "{}", stdout.trim_end_matches('\n'));
            let _ = out.flush();
        }
    }
    append("TURN_STARTED");

    // A turn that completes on its own ends as soon as it has the prompt (the
    // ordinary case); MOCK_TURN_HOLD keeps it running so a test can interrupt a
    // genuinely in-flight turn.
    let hold = std::env::var_os("MOCK_TURN_HOLD").is_some();
    let mut lines = std::io::BufRead::lines(std::io::stdin().lock());
    // Turns, not one turn: the stream stays open after a turn ends, so a
    // message arriving afterwards opens the next one. That is what an interrupt
    // carrying a redirection depends on, and modelling it here is what lets a
    // hermetic test see the redirected turn actually run.
    let mut index = 0usize;
    loop {
        let mut interrupted = false;
        let mut ended = false;
        for line in lines.by_ref() {
            let Ok(line) = line else { break };
            append(&line);
            // A control frame, not a line that merely mentions one: a prompt
            // whose text says `control_request` would otherwise end the turn
            // nobody asked to stop.
            let control = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .is_some_and(|frame| {
                    frame.get("type").and_then(serde_json::Value::as_str) == Some("control_request")
                });
            if control {
                interrupted = true;
                ended = true;
                append("INTERRUPTED");
                break;
            }
            // Only the run's own turn is held open; a redirected one ends as
            // soon as it has its message, so a test does not have to interrupt
            // twice to watch a run finish.
            if !hold || index > 0 {
                ended = true;
                break;
            }
        }
        // The stream closed rather than a turn ending: there is nothing further
        // to answer, and emitting another document would invent a turn.
        if !ended {
            break;
        }
        if let Ok(delay) = std::env::var("MOCK_TURN_RESULT_DELAY_MS") {
            let delay = match delay.parse::<u64>() {
                Ok(delay) if delay <= MAX_REPLY_DELAY_MS => delay,
                _ => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "mock harness: MOCK_TURN_RESULT_DELAY_MS must be milliseconds from 0 through {MAX_REPLY_DELAY_MS}"
                    );
                    std::process::exit(2);
                }
            };
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        let once = std::env::var_os("MOCK_TURN_ONCE").is_some();
        let result = std::env::var("MOCK_TURN_RESULT").unwrap_or_else(|_| {
            format!(
                r#"{{"type":"result","subtype":"{}","session_id":"mock-session","result":"mock turn"}}"#,
                if interrupted {
                    "error_during_execution"
                } else {
                    "success"
                }
            )
        });
        let _ = writeln!(out, "{result}");
        let _ = out.flush();
        append("TURN_ENDED");
        if once {
            break;
        }
        index += 1;
    }
    std::process::exit(0);
}

/// Block until `path` names this process's pid on a line of its own — the
/// release a [`MOCK_HOLD_FILE`](self) caller writes once it has finished asking
/// the OS about this child. It returns only once that release has arrived;
/// waiting past `HOLD_MAX` ends the process instead, so a caller that never
/// releases gets its own assertion back, loudly, rather than a hung suite.
fn wait_until_released(path: &str) {
    const HOLD_MAX: std::time::Duration = std::time::Duration::from_secs(30);
    // The release arrives in this file, so being able to open it is the whole
    // precondition — established by opening it, not inferred from the shape of
    // the path, which cannot tell a writable directory from an unwritable one.
    // A hold nothing can ever release would otherwise spend the entire bound
    // waiting and then hand its caller a child it never held.
    if let Err(error) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            std::io::stderr(),
            "mock harness: MOCK_HOLD_FILE '{path}' cannot be opened for a release to arrive in: {error}"
        );
        std::process::exit(2);
    }
    let me = std::process::id().to_string();
    let deadline = std::time::Instant::now() + HOLD_MAX;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if text.lines().any(|line| line.trim() == me) {
                return;
            }
        }
        if std::time::Instant::now() >= deadline {
            // Running on regardless would be the very thing the hold exists to
            // stop: a caller that never released this child would get one it
            // had not held, and an assertion about it that means nothing. So
            // the bound ends the process loudly instead — it is there to stop a
            // misuse hanging the suite, not to let one through.
            let _ = writeln!(
                std::io::stderr(),
                "mock harness: MOCK_HOLD_FILE {path} never named pid {me} within {HOLD_MAX:?}"
            );
            std::process::exit(2);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

pub fn run() -> ! {
    #[cfg(windows)]
    if std::env::var_os("ONEHARNESS_MOCK_NATIVE_DESCENDANT").is_some() {
        run_native_descendant();
    }

    // The long-lived helper a shared app-server starts (CODEX_MOCK_HELPER):
    // it only lives, until something stops it.
    if std::env::var_os("ONEHARNESS_MOCK_APP_SERVER_HELPER").is_some() {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv == ["--version"] {
        answer_version(&argv);
    }

    if let Ok(path) = std::env::var("MOCK_HOLD_FILE") {
        wait_until_released(&path);
    }

    if let Ok(path) = std::env::var("MOCK_ARGV_FILE") {
        let _ = std::fs::write(path, argv.join("\n"));
    }

    if let Ok(path) = mock_var("MOCK_CODEX_PLUGINS_LOG") {
        record_codex_plugins(&path, &argv);
    }

    if let Ok(path) = mock_var("MOCK_CODEX_CATALOG_LOG") {
        codex_model_catalog(&path, &argv);
    }

    // `codex app-server --listen unix://PATH`: the shared server a pooled
    // dispatch's turn runs on.
    #[cfg(unix)]
    if argv.first().is_some_and(|first| first == "app-server")
        && argv.iter().any(|arg| arg == "--listen")
    {
        listen_server::run(&argv);
    }

    // Append one line to the run log (when configured), opening fresh each time so
    // concurrent invocations share the file via the OS's append semantics.
    let log_line = |line: &str| {
        if let Ok(path) = std::env::var("MOCK_LOG_FILE") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    };

    #[cfg(unix)]
    if let Ok(ms) = std::env::var("MOCK_NATIVE_GRANDCHILD_MS") {
        let count = ms
            .parse::<u64>()
            .ok()
            .map(|value| (value / 50).max(1))
            .unwrap_or(1);
        // The shell receives transcript bytes through inherited environment, not
        // interpolation, so arbitrary JSON remains data. Ignoring TERM forces
        // oneharness to exercise the group-wide KILL fallback after its grace.
        let script = r#"
            trap '' TERM
            printf '%s' "${MOCK_STDOUT:-}"
            printf '%s' "${MOCK_STDERR:-}" >&2
            i=0
            while [ "$i" -lt "$1" ]; do
                if [ -n "${MOCK_TICK_FILE:-}" ]; then
                    printf x >> "$MOCK_TICK_FILE"
                fi
                i=$((i + 1))
                sleep 0.05
            done
        "#;
        let status = std::process::Command::new("sh")
            .args(["-c", script, "oneharness-native-child", &count.to_string()])
            .status();
        std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(1));
    }

    #[cfg(windows)]
    if std::env::var_os("MOCK_NATIVE_GRANDCHILD_MS").is_some() {
        // Windows does not tie a process's lifetime to its parent. This native
        // child would therefore survive a direct launcher kill and retain the
        // inherited stdout/stderr handles; the runner's Job Object must own it.
        let status = std::env::current_exe()
            .and_then(|executable| {
                std::process::Command::new(executable)
                    .env("ONEHARNESS_MOCK_NATIVE_DESCENDANT", "1")
                    .status()
            })
            .ok();
        std::process::exit(status.and_then(|value| value.code()).unwrap_or(1));
    }

    if std::env::var_os("MOCK_ECHO_PWD").is_some() {
        let pwd = std::env::var("PWD").unwrap_or_default();
        let _ = write!(std::io::stdout(), "PWD={pwd}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(name) = std::env::var("MOCK_ECHO_ENV") {
        let value = std::env::var(&name).unwrap_or_default();
        let _ = write!(std::io::stdout(), "{name}={value}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(path) = std::env::var("MOCK_CAT_FILE") {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = write!(std::io::stdout(), "{contents}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    // An early stdin EOF still answers, so a probe that writes fewer lines than
    // expected sees a real response rather than a hang.
    if let Ok(path) = std::env::var("MOCK_TURN_LOG") {
        run_controlled_turn(&path);
    }

    if let Ok(path) = mock_var("MOCK_CODEX_APP_SERVER_LOG") {
        run_codex_app_server(&path);
    }
    if let Ok(path) = std::env::var("MOCK_ACP_LOG") {
        run_acp_server(&path);
    }

    if let Ok(path) = std::env::var("MOCK_HTTP_CONTROL_LOG") {
        run_http_control_server(&path);
    }

    if let Ok(count) = std::env::var("MOCK_REPLY_AFTER_LINES") {
        // Loud on a malformed value: silently reading it as 1 would let a
        // typo'd test drive the wrong exchange and still report success.
        let Ok(wanted) = count.parse::<usize>() else {
            let _ = write!(
                std::io::stderr(),
                "mock harness: MOCK_REPLY_AFTER_LINES must be a whole number of stdin lines, got `{count}`"
            );
            let _ = std::io::stderr().flush();
            std::process::exit(2);
        };
        // Bounded, not merely numeric: the delay becomes an `Instant` deadline
        // further down, and `Instant + Duration` panics rather than saturates
        // once the sum leaves the platform clock's range. Refusing the value
        // here makes a mistyped knob one legible message on every platform,
        // instead of a crash on some and a wait no run outlasts on the rest.
        let raw_delay = std::env::var("MOCK_REPLY_DELAY_MS").unwrap_or_else(|_| "0".to_string());
        let delay = match raw_delay.parse::<u64>() {
            Ok(delay) if delay <= MAX_REPLY_DELAY_MS => delay,
            _ => {
                let _ = write!(
                    std::io::stderr(),
                    "mock harness: MOCK_REPLY_DELAY_MS must be a whole number of milliseconds \
                     from 0 to {MAX_REPLY_DELAY_MS} ({} minutes), got `{raw_delay}`",
                    MAX_REPLY_DELAY_MS / 60_000
                );
                let _ = std::io::stderr().flush();
                std::process::exit(2);
            }
        };
        let record = std::env::var("MOCK_REQUEST_FILE").ok();
        let (requests, saw_eof) = read_requests(wanted, delay, record.is_some());
        if let Some(path) = record {
            let _ = std::fs::write(path, requests.join("\n"));
        }
        // A delayed answer models a server whose reply is asynchronous, and such
        // a server shuts down on EOF with that reply still in flight. Exiting
        // unanswered is the whole point of the mode: a caller that closed stdin
        // behind its request gets silence, exactly as the real one gives it.
        if delay > 0 && saw_eof {
            std::process::exit(0);
        }
        if let Ok(text) = std::env::var("MOCK_STDERR") {
            let _ = write!(std::io::stderr(), "{text}");
            let _ = std::io::stderr().flush();
        }
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            scripted_stdout(scripted_attempt(), "")
        );
        let _ = std::io::stdout().flush();
        std::process::exit(
            std::env::var("MOCK_EXIT")
                .ok()
                .and_then(|code| code.parse().ok())
                .unwrap_or(0),
        );
    }

    if std::env::var_os("MOCK_ECHO_STDIN").is_some() {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
        let _ = std::io::stdout().write_all(&buf);
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(flag) = std::env::var("MOCK_CAT_ARG_AFTER") {
        let path = argv
            .iter()
            .position(|a| *a == flag)
            .and_then(|i| argv.get(i + 1));
        let contents = path
            .map(|p| std::fs::read_to_string(p).unwrap_or_default())
            .unwrap_or_default();
        let _ = write!(std::io::stdout(), "{contents}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    // Per-model failure: if the received argv selects the doomed model, fail with
    // a classifiable `model not found` stderr so a fallback fan-out falls through
    // it to the next model. Any other model runs normally.
    if let Ok(bad) = std::env::var("MOCK_FAIL_IF_MODEL") {
        let requested = argv
            .iter()
            .position(|a| a == "--model")
            .and_then(|i| argv.get(i + 1));
        if requested.map(String::as_str) == Some(bad.as_str()) {
            let msg = std::env::var("MOCK_FAIL_STDERR")
                .unwrap_or_else(|_| format!("error: model not found: {bad}"));
            let _ = write!(std::io::stderr(), "{msg}");
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }
    }

    log_line("S\n");
    if let Ok(ms) = std::env::var("MOCK_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    log_line("E\n");

    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = write!(std::io::stderr(), "{text}");
    }

    // Per-attempt scripting, so a test can make the first response invalid and
    // a later one valid to exercise the structured-output retry loop.
    let attempt = scripted_attempt();
    let stdout = scripted_stdout(attempt, "{\"result\":\"mock ok\"}");
    let stdout = if std::env::var("MOCK_PRESERVE_STDOUT").as_deref() == Ok("1") {
        stdout
    } else {
        session_faithful_stdout(&argv, stdout)
    };

    if let Ok(chunk_bytes) = std::env::var("MOCK_STREAM_CHUNK_BYTES") {
        let chunk_bytes = chunk_bytes.parse::<usize>().unwrap_or(0);
        assert!(chunk_bytes > 0, "MOCK_STREAM_CHUNK_BYTES must be positive");
        let delay = std::env::var("MOCK_STREAM_DELAY_MS")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .expect("MOCK_STREAM_DELAY_MS must be an unsigned integer")
            })
            .unwrap_or(0);
        let mut out = std::io::stdout();
        for (index, chunk) in stdout.as_bytes().chunks(chunk_bytes).enumerate() {
            if index > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            if out.write_all(chunk).is_err() || out.flush().is_err() {
                std::process::exit(0);
            }
        }
        log_line("COMPLETE\n");
        std::process::exit(0);
    }

    // Incremental streaming mode: with MOCK_STREAM_DELAY_MS set, emit MOCK_STDOUT
    // one line at a time, flushing and sleeping between lines — so a streaming
    // consumer sees events arrive over time and can short-circuit mid-stream. On
    // the FIRST write that fails (the reader — oneharness — was killed or stopped
    // reading, or oneharness forwarded an event to a consumer that closed the
    // pipe and then killed this child), we exit *without* writing the completion
    // sentinel, which is how a test proves the child was torn down early. When the
    // stream runs to completion, "COMPLETE\n" is appended to MOCK_LOG_FILE.
    if let Ok(delay) = std::env::var("MOCK_STREAM_DELAY_MS") {
        let delay = delay.parse::<u64>().unwrap_or(0);
        let mut out = std::io::stdout();
        for (i, line) in stdout.lines().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                std::process::exit(0); // reader gone — do not signal completion
            }
        }
        log_line("COMPLETE\n");
        std::process::exit(0);
    }

    let _ = write!(std::io::stdout(), "{stdout}");
    let _ = std::io::stdout().flush();

    let code = attempt
        .and_then(|n| std::env::var(format!("MOCK_EXIT_{n}")).ok())
        .or_else(|| std::env::var("MOCK_EXIT").ok())
        .map(|text| {
            text.parse::<i32>()
                .expect("MOCK_EXIT[_<attempt>] must be an integer")
        })
        .unwrap_or(0);
    std::process::exit(code);
}

/// `MOCK_CODEX_*` knobs also answer to a `CODEX_MOCK_*` spelling (the
/// `MOCK_CODEX_` prefix replaced): a shared app-server is launched with a
/// scrubbed environment that keeps `CODEX_*` variables and drops the rest, so a
/// knob a pooled server must see at launch travels under that spelling.
fn mock_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name).or_else(|err| match name.strip_prefix("MOCK_CODEX_") {
        Some(rest) => std::env::var(format!("CODEX_MOCK_{rest}")),
        None => Err(err),
    })
}

/// [`mock_var`] for a presence knob.
fn mock_var_os(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).or_else(|| {
        name.strip_prefix("MOCK_CODEX_")
            .and_then(|rest| std::env::var_os(format!("CODEX_MOCK_{rest}")))
    })
}

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] The mock stands in for real Codex, which the deterministic gate cannot run; each behaviour it imitates is one spike-shared-app-server observed of codex-cli 0.159.2 (or this node's own no-turn probes: the 60 s unload grace, the refused resume of a never-turned thread), and the live phases scripts/e2e-control.sh runs against real Codex (pooled-env, pooled-kill, pooled-reuse) are the drift alarm that fails when the two part.
/// Act like `codex app-server --listen unix://PATH`: the shared,
/// multi-client app-server a pooled dispatch's turn runs on, imitating real
/// Codex as `spike-shared-app-server-report` observed it. Unix only, like the
/// controlled-run journeys that drive it.
///
/// What it imitates:
/// - a WebSocket endpoint on the socket; a path too long for `sun_path` bound
///   as a short target under `/tmp/codex-daemon-<uid>/` with a symlink left at
///   the path; a second server on a live path exits 1;
/// - per-thread settings, and commands run under the thread's
///   `shell_environment_policy` (`set` over the server's own environment, minus
///   `exclude`) in their own process group, outliving `turn/interrupt`;
/// - `thread/status/changed` broadcast to every connection;
/// - a finished thread kept loaded until its last subscriber sends
///   `thread/unsubscribe`, then unloaded with `thread/closed`;
/// - no answer to an interrupt naming a turn that already ended.
///
/// Its per-thread knobs are the dispatch's own `MOCK_*` variables, read off
/// the thread's `shell_environment_policy.set`; its server-wide knobs are
/// `CODEX_MOCK_*` variables of its own environment:
///
///   CODEX_MOCK_DIR        records: `server-<pid>.json` (pid, argv, env),
///                         `conn-<pid>-<n>.jsonl` (every frame in and out per
///                         connection) and `state-<pid>.json` (loaded threads,
///                         turns with their effective context, commands,
///                         approvals, interrupts, catalog fetches).
///   CODEX_MOCK_SHIM       act like Codex's Node launcher: run the server as a
///                         child and wait for it (a SIGKILLed launcher leaves the
///                         child serving).
///   CODEX_MOCK_HELPER     the server starts a long-lived helper child.
///   CODEX_MOCK_BROADCAST  send every thread's frames to every connection.
///   CODEX_MOCK_IGNORE_UNSUBSCRIBE  answer `thread/unsubscribe` but keep the
///                         thread loaded.
///   CODEX_MOCK_SERVER_BYTES  allocate and touch this many bytes for the
///                         server's life, its own resident footprint.
///   CODEX_MOCK_THREAD_BYTES  allocate and touch this many bytes per loaded
///                         thread, freed when it unloads.
///   CODEX_MOCK_FAULT      `leak-env` runs every command under the first opened
///                         thread's environment; `rollout-env` writes thread
///                         environments into rollout files; `escape-marker`
///                         drops the dispatch marker from commands;
///                         `late-listener` (with CODEX_MOCK_SHIM) has the serving
///                         child drop the server marker and, once its launcher
///                         is gone, keep its listener open for 3 s accepting
///                         nothing, as an exiting server's socket outlives the
///                         environment a marker scan reads.
///   CODEX_MOCK_EXIT_FIRST  a path: the first launch creates it and exits before
///                         listening.
///   CODEX_MOCK_LOADED_PAGE  answer `thread/loaded/list` in pages of this many.
///   CODEX_MOCK_OPEN_DELAYS_MS  comma-separated: the n-th `thread/start` is
///                         answered after the n-th delay.
///   CODEX_MOCK_STARTUP_FETCH  `PORT BYTES`: download that many bytes from the
///                         network tests' loopback server once at startup.
///
/// Per-thread knobs: MOCK_CODEX_APP_SERVER_LOG (raw frames this connection
/// sent), MOCK_CODEX_COMPLETE_TURN, MOCK_CODEX_HOLD_UNTIL (end the turn once
/// this file exists, so a test holds two turns live at once),
/// MOCK_CODEX_HOLD_OPEN_UNTIL (answer `thread/start` once this file exists),
/// MOCK_CODEX_SHUT_READ_ON_APPROVAL (stop reading the connection as its
/// approval request goes out),
/// MOCK_CODEX_UNSUBSCRIBE_DELAY_MS, MOCK_CODEX_UNSUBSCRIBE_REFUSE (answer
/// `thread/unsubscribe` with an error), MOCK_CODEX_ECHO_PROMPT,
/// MOCK_CODEX_TOOL_EVENTS, MOCK_CODEX_MODEL, MOCK_CODEX_RUN_COMMAND (a `sh -c`
/// command each turn runs), MOCK_CODEX_REQUEST_APPROVAL (ask for approval of
/// that command first), MOCK_CODEX_INTERRUPT_SILENT (never answer
/// `turn/interrupt`), MOCK_CODEX_INTERRUPT_THREAD_NOT_FOUND (answer it with
/// `thread not found`).
#[cfg(unix)]
mod listen_server {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::io::{BufRead, Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    type Shared = Arc<Mutex<Server>>;

    struct Conn {
        writer: Arc<Mutex<UnixStream>>,
        record: Option<std::fs::File>,
        /// The dispatch's own raw frame log, once its thread names one, and
        /// the frames received before it did.
        log: Option<std::fs::File>,
        unlogged: Vec<String>,
        /// Kept open after its reads stopped, so a client's write onto it is
        /// refused rather than its read seeing the end first.
        held: bool,
    }

    #[derive(Clone, Default)]
    struct Settings {
        cwd: String,
        model: Option<String>,
        approval: Option<String>,
        sandbox: Option<String>,
        set: BTreeMap<String, String>,
        exclude: Vec<String>,
    }

    struct Thread {
        settings: Settings,
        subscribers: BTreeSet<u64>,
        active_turn: Option<String>,
        turns: u64,
        commands_running: usize,
        memory: Vec<u8>,
        approval_pending: Option<u64>,
    }

    impl Thread {
        fn knob(&self, name: &str) -> Option<&str> {
            self.settings.set.get(name).map(String::as_str)
        }
    }

    struct Server {
        pid: u32,
        dir: Option<PathBuf>,
        broadcast: bool,
        ignore_unsubscribe: bool,
        thread_bytes: usize,
        fault: String,
        conns: HashMap<u64, Conn>,
        next_conn: u64,
        threads: HashMap<String, Thread>,
        known: HashMap<String, Settings>,
        next_thread: u64,
        next_request: u64,
        first_opened: Option<String>,
        turns: Vec<Value>,
        commands: Vec<Value>,
        approvals: Vec<Value>,
        interrupts: Vec<Value>,
        releases: Vec<Value>,
        /// The server's own resident memory after each thread it opened.
        open_rss: Vec<u64>,
    }

    /// This process's resident memory, in bytes, as /proc says.
    fn own_rss() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("VmRSS:"))
                    .and_then(|kb| kb.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            })
            .map_or(0, |kb| kb * 1024)
    }

    pub fn run(argv: &[String]) -> ! {
        if std::env::var_os("CODEX_MOCK_SHIM").is_some()
            && std::env::var_os("ONEHARNESS_MOCK_SHIM_CHILD").is_none()
        {
            run_shim(argv);
        }
        let path = argv
            .windows(2)
            .find(|pair| pair[0] == "--listen")
            .and_then(|pair| pair[1].strip_prefix("unix://"))
            .map(PathBuf::from)
            .unwrap_or_else(|| super::refuse("--listen must name a unix:// path"));
        // CODEX_MOCK_EXIT_FIRST: the first launch to find this file absent
        // creates it and exits before listening, as a server that dies at
        // start does; every later launch serves.
        if let Ok(first) = std::env::var("CODEX_MOCK_EXIT_FIRST") {
            if std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&first)
                .is_ok()
            {
                std::process::exit(1);
            }
        }
        let listener = bind(&path);
        // CODEX_MOCK_SERVER_BYTES: a resident footprint of the server's own,
        // touched and held for its life, as a real server has one.
        let _footprint: Vec<u8> = vec![
            1u8;
            std::env::var("CODEX_MOCK_SERVER_BYTES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        ];
        if std::env::var_os("CODEX_MOCK_HELPER").is_some() {
            if let Ok(exe) = std::env::current_exe() {
                let helper = std::process::Command::new(exe)
                    .env("ONEHARNESS_MOCK_APP_SERVER_HELPER", "1")
                    .stdin(std::process::Stdio::null())
                    .spawn();
                if let Ok(mut helper) = helper {
                    std::thread::spawn(move || {
                        let _ = helper.wait();
                    });
                }
            }
        }
        if let Ok(url) = std::env::var("CODEX_MOCK_STARTUP_FETCH") {
            startup_fetch(&url);
        }
        let dir = std::env::var_os("CODEX_MOCK_DIR").map(PathBuf::from);
        let pid = std::process::id();
        if let Some(dir) = &dir {
            let _ = std::fs::create_dir_all(dir);
        }
        let server: Shared = Arc::new(Mutex::new(Server {
            pid,
            dir: dir.clone(),
            broadcast: std::env::var_os("CODEX_MOCK_BROADCAST").is_some(),
            ignore_unsubscribe: std::env::var_os("CODEX_MOCK_IGNORE_UNSUBSCRIBE").is_some(),
            thread_bytes: std::env::var("CODEX_MOCK_THREAD_BYTES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            fault: std::env::var("CODEX_MOCK_FAULT").unwrap_or_default(),
            conns: HashMap::new(),
            next_conn: 1,
            threads: HashMap::new(),
            known: HashMap::new(),
            next_thread: 1,
            next_request: 1 << 50,
            first_opened: None,
            turns: Vec::new(),
            commands: Vec::new(),
            approvals: Vec::new(),
            interrupts: Vec::new(),
            releases: Vec::new(),
            open_rss: Vec::new(),
        }));
        lock(&server).save();
        // The server's record after its first state: a reader that finds the
        // server always finds its state too.
        if let Some(dir) = &dir {
            let env: BTreeMap<String, String> = std::env::vars().collect();
            let _ = std::fs::write(
                dir.join(format!("server-{pid}.json")),
                json!({"pid": pid, "argv": argv, "env": env}).to_string(),
            );
        }
        let orphaned = late_listener();
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if orphaned.load(std::sync::atomic::Ordering::SeqCst) {
                held.push(stream);
                continue;
            }
            let server = Arc::clone(&server);
            std::thread::spawn(move || serve(&server, stream));
        }
        std::process::exit(0);
    }

    fn lock(server: &Shared) -> std::sync::MutexGuard<'_, Server> {
        server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// CODEX_MOCK_FAULT=late-listener, in the launcher's serving child: a flag
    /// raised once the launcher is gone, after which the process exits 3 s
    /// later; until then its listener stays open and accepts into nothing.
    fn late_listener() -> Arc<std::sync::atomic::AtomicBool> {
        let orphaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if std::env::var_os("ONEHARNESS_MOCK_SHIM_CHILD").is_none()
            || std::env::var("CODEX_MOCK_FAULT").as_deref() != Ok("late-listener")
        {
            return orphaned;
        }
        let launcher = std::os::unix::process::parent_id();
        let flag = Arc::clone(&orphaned);
        std::thread::spawn(move || loop {
            if std::os::unix::process::parent_id() != launcher {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_secs(3));
                std::process::exit(0);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        });
        orphaned
    }

    /// The launcher: the server as a child, waited on. Nothing forwards a
    /// SIGKILL, so a killed launcher leaves the child serving.
    fn run_shim(argv: &[String]) -> ! {
        let exe = std::env::current_exe().unwrap_or_else(|_| super::refuse("no executable"));
        let mut child = std::process::Command::new(exe);
        child.args(argv).env("ONEHARNESS_MOCK_SHIM_CHILD", "1");
        if std::env::var("CODEX_MOCK_FAULT").as_deref() == Ok("late-listener") {
            child.env_remove(oneharness_core::domain::app_server::SERVER_MARKER_VAR);
        }
        let status = child.status();
        std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(1));
    }

    /// Bind the socket the way Codex does: a path too long for `sun_path` gets
    /// a short target under `/tmp/codex-daemon-<uid>/` and a symlink at the
    /// path. A path a live server answers on is refused; a stale one replaced.
    fn bind(path: &Path) -> UnixListener {
        if UnixStream::connect(super::super_dial(path)).is_ok() {
            let _ = writeln!(
                std::io::stderr(),
                "app-server control socket is already in use"
            );
            std::process::exit(1);
        }
        let too_long = path.as_os_str().len() >= 100;
        let target = if too_long {
            use std::os::unix::fs::MetadataExt;
            let uid = std::fs::metadata(path.parent().unwrap_or(Path::new("/")))
                .map(|meta| meta.uid())
                .unwrap_or(0);
            let dir = PathBuf::from(format!("/tmp/codex-daemon-{uid}"));
            let _ = std::fs::create_dir_all(&dir);
            let digest = path
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
                });
            dir.join(format!("{digest:016x}"))
        } else {
            path.to_path_buf()
        };
        let _ = std::fs::remove_file(&target);
        if too_long {
            let _ = std::fs::remove_file(path);
        }
        let listener = UnixListener::bind(&target).unwrap_or_else(|err| {
            super::refuse(&format!("cannot bind {}: {err}", target.display()))
        });
        if too_long {
            if let Err(err) = std::os::unix::fs::symlink(&target, path) {
                super::refuse(&format!("cannot link {}: {err}", path.display()));
            }
        }
        listener
    }

    /// Download `BYTES` generated bytes from the network tests' loopback
    /// server on `PORT` (`GET <BYTES>`), as a server's startup fetch: reached
    /// at the default route's gateway, which inside the recorder's namespace
    /// stands for the host's loopback, else at 127.0.0.1.
    fn startup_fetch(spec: &str) {
        let mut words = spec.split_whitespace();
        let (Some(port), Some(bytes)) = (words.next(), words.next()) else {
            super::refuse("CODEX_MOCK_STARTUP_FETCH must be `PORT BYTES`");
        };
        let host = std::fs::read_to_string("/proc/net/route")
            .ok()
            .and_then(|routes| {
                routes.lines().skip(1).find_map(|line| {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    (fields.get(1) == Some(&"00000000"))
                        .then(|| u32::from_str_radix(fields.get(2)?, 16).ok())
                        .flatten()
                })
            })
            .map_or_else(
                || "127.0.0.1".to_string(),
                |gateway| std::net::Ipv4Addr::from(gateway.to_le_bytes()).to_string(),
            );
        let Ok(mut stream) = std::net::TcpStream::connect(format!("{host}:{port}")) else {
            super::refuse(&format!(
                "CODEX_MOCK_STARTUP_FETCH could not reach {host}:{port}"
            ));
        };
        let _ = writeln!(stream, "GET {bytes}");
        let mut sink = Vec::new();
        let _ = stream.read_to_end(&mut sink);
    }

    impl Server {
        fn save(&self) {
            let Some(dir) = &self.dir else { return };
            let threads: BTreeMap<&String, Value> = self
                .threads
                .iter()
                .map(|(id, thread)| {
                    (
                        id,
                        json!({
                            "subscribers": thread.subscribers,
                            "active_turn": thread.active_turn,
                            "memory_bytes": thread.memory.len(),
                            "commands_running": thread.commands_running,
                        }),
                    )
                })
                .collect();
            let state = json!({
                "pid": self.pid,
                "connections": self.conns.keys().collect::<Vec<_>>(),
                "loaded": threads,
                "known": self.known.keys().collect::<Vec<_>>(),
                "turns": self.turns,
                "commands": self.commands,
                "approvals": self.approvals,
                "interrupts": self.interrupts,
                "releases": self.releases,
                "open_rss": self.open_rss,
            });
            let tmp = dir.join(format!("state-{}.json.tmp", self.pid));
            if std::fs::write(&tmp, state.to_string()).is_ok() {
                let _ = std::fs::rename(tmp, dir.join(format!("state-{}.json", self.pid)));
            }
        }

        fn record(&mut self, conn: u64, direction: &str, frame: &Value) {
            if let Some(file) = self.conns.get_mut(&conn).and_then(|c| c.record.as_mut()) {
                let _ = writeln!(file, "{}", json!({direction: frame}));
            }
        }

        fn send(&mut self, conn: u64, frame: &Value) {
            self.record(conn, "out", frame);
            if let Some(c) = self.conns.get(&conn) {
                let mut writer = c.writer.lock().unwrap_or_else(|p| p.into_inner());
                let _ = write_frame(&mut writer, frame.to_string().as_bytes());
            }
        }

        /// Send a thread's frame to its subscribers, or to every connection
        /// when broadcasting.
        fn emit(&mut self, thread: &str, frame: &Value) {
            let targets: Vec<u64> = if self.broadcast {
                self.conns.keys().copied().collect()
            } else {
                self.threads
                    .get(thread)
                    .map(|t| t.subscribers.iter().copied().collect())
                    .unwrap_or_default()
            };
            for conn in targets {
                self.send(conn, frame);
            }
        }

        /// `thread/status/changed`, which real Codex sends every connection.
        fn status(&mut self, thread: &str, status: &str) {
            let frame = json!({"jsonrpc": "2.0", "method": "thread/status/changed",
                "params": {"threadId": thread, "status": {"type": status}}});
            let conns: Vec<u64> = self.conns.keys().copied().collect();
            for conn in conns {
                self.send(conn, &frame);
            }
        }

        fn complete_turn(&mut self, thread_id: &str, status: &str) {
            let Some(thread) = self.threads.get_mut(thread_id) else {
                return;
            };
            let Some(turn) = thread.active_turn.take() else {
                return;
            };
            if let Some(record) = self
                .turns
                .iter_mut()
                .rev()
                .find(|t| t["turn"] == json!(turn))
            {
                record["status"] = json!(status);
            }
            self.emit(
                thread_id,
                &json!({"jsonrpc": "2.0", "method": "turn/completed",
                    "params": {"threadId": thread_id, "turn": {"id": turn, "status": status}}}),
            );
            self.status(thread_id, "idle");
            self.save();
        }

        /// End the thread's turn once nothing holds it: no command running, no
        /// approval waiting, and either MOCK_CODEX_COMPLETE_TURN or the file
        /// MOCK_CODEX_HOLD_UNTIL names present.
        fn maybe_complete(&mut self, thread_id: &str) {
            let Some(thread) = self.threads.get(thread_id) else {
                return;
            };
            let released = thread.knob("MOCK_CODEX_COMPLETE_TURN").is_some()
                || thread
                    .knob("MOCK_CODEX_HOLD_UNTIL")
                    .is_some_and(|path| Path::new(path).exists());
            if thread.active_turn.is_some()
                && thread.commands_running == 0
                && thread.approval_pending.is_none()
                && released
            {
                self.complete_turn(thread_id, "completed");
            }
        }

        fn rollout(&self, thread: &str, line: &Value) {
            let Ok(home) = std::env::var("CODEX_HOME") else {
                return;
            };
            let dir = Path::new(&home).join("sessions");
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(format!("rollout-{thread}.jsonl")))
            {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    fn write_frame(stream: &mut UnixStream, payload: &[u8]) -> std::io::Result<()> {
        let mut frame = vec![0x81u8];
        let len = payload.len();
        if len < 126 {
            frame.push(len as u8);
        } else if len <= usize::from(u16::MAX) {
            frame.push(126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        stream.write_all(&frame)
    }

    /// One client message, unmasked; `None` once the client is gone.
    fn read_message(stream: &mut impl Read) -> Option<Vec<u8>> {
        let mut message = Vec::new();
        loop {
            let mut head = [0u8; 2];
            stream.read_exact(&mut head).ok()?;
            let len = match head[1] & 0x7f {
                126 => {
                    let mut ext = [0u8; 2];
                    stream.read_exact(&mut ext).ok()?;
                    u64::from(u16::from_be_bytes(ext))
                }
                127 => {
                    let mut ext = [0u8; 8];
                    stream.read_exact(&mut ext).ok()?;
                    u64::from_be_bytes(ext)
                }
                short => u64::from(short),
            };
            let mut mask = [0u8; 4];
            if head[1] & 0x80 != 0 {
                stream.read_exact(&mut mask).ok()?;
            }
            let mut payload = vec![0u8; usize::try_from(len).ok()?];
            stream.read_exact(&mut payload).ok()?;
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
            match head[0] & 0x0f {
                0x8 => return None,
                0x9 | 0xA => continue,
                _ => message.extend_from_slice(&payload),
            }
            if head[0] & 0x80 != 0 {
                return Some(message);
            }
        }
    }

    fn serve(server: &Shared, stream: UnixStream) {
        let Ok(writer) = stream.try_clone() else {
            return;
        };
        let mut reader = std::io::BufReader::new(stream);
        let mut key = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("sec-websocket-key") {
                    key = value.trim().to_string();
                }
            }
        }
        let writer = Arc::new(Mutex::new(writer));
        {
            let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
            let answer = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
                oneharness_core::io::websocket::accept_key(&key)
            );
            if w.write_all(answer.as_bytes()).is_err() {
                return;
            }
        }
        let conn = {
            let mut s = lock(server);
            let id = s.next_conn;
            s.next_conn += 1;
            let record = s.dir.as_ref().and_then(|dir| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(format!("conn-{}-{id}.jsonl", s.pid)))
                    .ok()
            });
            s.conns.insert(
                id,
                Conn {
                    writer: Arc::clone(&writer),
                    record,
                    log: None,
                    unlogged: Vec::new(),
                    held: false,
                },
            );
            s.save();
            id
        };
        while let Some(bytes) = read_message(&mut reader) {
            let text = String::from_utf8_lossy(&bytes).to_string();
            let Ok(frame) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            hold(server, &frame);
            handle(server, conn, &text, &frame);
        }
        if lock(server).conns.get(&conn).is_some_and(|c| c.held) {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        // The client is gone. Like Codex, its threads stay loaded and their
        // turns and commands keep running; only its subscriptions go.
        let mut s = lock(server);
        s.conns.remove(&conn);
        for thread in s.threads.values_mut() {
            thread.subscribers.remove(&conn);
        }
        s.save();
    }

    /// Keep a request waiting, outside the server's lock, where a knob says
    /// to: `thread/start` until MOCK_CODEX_HOLD_OPEN_UNTIL's file exists (a
    /// dispatch held idle, before its turn), `thread/unsubscribe` for
    /// MOCK_CODEX_UNSUBSCRIBE_DELAY_MS (a dispatch whose turn has ended still
    /// releasing its thread).
    fn hold(server: &Shared, frame: &Value) {
        let set_knob = |name: &str| {
            frame
                .pointer(&format!(
                    "/params/config/shell_environment_policy/set/{name}"
                ))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        match frame.get("method").and_then(Value::as_str) {
            Some("thread/start") => {
                // CODEX_MOCK_OPEN_DELAYS_MS: the n-th thread/start this server
                // answers waits the n-th delay, so a measurement's time to a
                // ready thread has a known floor.
                static OPENS: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let nth = OPENS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(ms) =
                    std::env::var("CODEX_MOCK_OPEN_DELAYS_MS")
                        .ok()
                        .and_then(|delays| {
                            delays
                                .split(',')
                                .nth(nth)
                                .and_then(|ms| ms.trim().parse::<u64>().ok())
                        })
                {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                if let Some(path) = set_knob("MOCK_CODEX_HOLD_OPEN_UNTIL") {
                    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
                    while !Path::new(&path).exists() && std::time::Instant::now() < until {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
            Some("thread/unsubscribe") => {
                let thread = frame
                    .pointer("/params/threadId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let delay = lock(server)
                    .threads
                    .get(thread)
                    .and_then(|t| t.knob("MOCK_CODEX_UNSUBSCRIBE_DELAY_MS"))
                    .and_then(|ms| ms.parse::<u64>().ok());
                if let Some(ms) = delay {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
            }
            _ => {}
        }
    }

    fn settings_from(params: &Value, base: Option<&Settings>) -> Settings {
        let mut settings = base.cloned().unwrap_or_default();
        if let Some(cwd) = params.get("cwd").and_then(Value::as_str) {
            settings.cwd = cwd.to_string();
        }
        if let Some(model) = params.get("model").and_then(Value::as_str) {
            settings.model = Some(model.to_string());
        }
        if let Some(approval) = params.get("approvalPolicy").and_then(Value::as_str) {
            settings.approval = Some(approval.to_string());
        }
        if let Some(sandbox) = params.get("sandbox").and_then(Value::as_str) {
            settings.sandbox = Some(sandbox.to_string());
        }
        if let Some(policy) = params.pointer("/config/shell_environment_policy") {
            settings.set = policy
                .get("set")
                .and_then(Value::as_object)
                .map(|set| {
                    set.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            settings.exclude = policy
                .get("exclude")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(|n| n.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
        }
        settings
    }

    fn open_thread(s: &mut Server, conn: u64, id: &str, mut settings: Settings, resumed: bool) {
        // A `MOCK_CODEX_*` knob the server itself was launched with (a wrapper
        // exporting it, as the network tests' harness double does) is every
        // thread's default; the thread's own value wins.
        for (name, value) in std::env::vars() {
            if name.starts_with("MOCK_CODEX_") && name != "MOCK_CODEX_APP_SERVER_LOG" {
                settings.set.entry(name).or_insert(value);
            }
        }
        let memory = vec![1u8; s.thread_bytes];
        let thread = s.threads.entry(id.to_string()).or_insert(Thread {
            settings: settings.clone(),
            subscribers: BTreeSet::new(),
            active_turn: None,
            turns: 0,
            commands_running: 0,
            memory,
            approval_pending: None,
        });
        thread.settings = settings.clone();
        thread.subscribers.insert(conn);
        s.known.insert(id.to_string(), settings.clone());
        if s.first_opened.is_none() {
            s.first_opened = Some(id.to_string());
        }
        if let Some(dir) = &s.dir {
            let _ = std::fs::create_dir_all(dir.join("threads"));
            let _ = std::fs::write(
                dir.join("threads").join(id),
                json!({"cwd": settings.cwd, "model": settings.model}).to_string(),
            );
        }
        let mut meta = json!({"type": "session_meta", "id": id, "cwd": settings.cwd,
            "model": settings.model, "resumed": resumed});
        if s.fault == "rollout-env" {
            meta["env"] = json!(settings.set);
        }
        s.rollout(id, &meta);
        if let Some(path) = settings.set.get("MOCK_CODEX_APP_SERVER_LOG") {
            if let Some(c) = s.conns.get_mut(&conn) {
                if c.log.is_none() {
                    c.log = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .ok();
                    if let Some(log) = c.log.as_mut() {
                        for line in c.unlogged.drain(..) {
                            let _ = writeln!(log, "{line}");
                        }
                    }
                }
            }
        }
    }

    /// The thread's id as a stored conversation would know it: across server
    /// restarts, a thread the pool started earlier is resumable.
    fn stored(s: &Server, id: &str) -> Option<Settings> {
        if let Some(settings) = s.known.get(id) {
            return Some(settings.clone());
        }
        let text = std::fs::read_to_string(s.dir.as_ref()?.join("threads").join(id)).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        Some(Settings {
            cwd: value.get("cwd")?.as_str()?.to_string(),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            ..Settings::default()
        })
    }

    fn served_model(params: &Value, settings: &Settings) -> String {
        settings
            .set
            .get("MOCK_CODEX_MODEL")
            .cloned()
            .or_else(|| {
                params
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "mock-codex-default".to_string())
    }

    fn handle(server: &Shared, conn: u64, text: &str, frame: &Value) {
        let mut s = lock(server);
        s.record(conn, "in", frame);
        if let Some(c) = s.conns.get_mut(&conn) {
            match c.log.as_mut() {
                Some(log) => {
                    let _ = writeln!(log, "{text}");
                }
                None => c.unlogged.push(text.to_string()),
            }
        }
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        let params = frame.get("params").cloned().unwrap_or(Value::Null);
        let reply = |s: &mut Server, result: Value| {
            s.send(conn, &json!({"jsonrpc": "2.0", "id": id, "result": result}));
        };
        let refuse = |s: &mut Server, message: String| {
            s.send(
                conn,
                &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32600, "message": message}}),
            );
        };
        let Some(method) = frame.get("method").and_then(Value::as_str) else {
            // A client's answer to a request of ours: an approval decision.
            if let Some(answered) = frame.get("id").and_then(Value::as_u64) {
                answer_approval(server, &mut s, conn, answered, frame);
            }
            return;
        };
        match method {
            "initialize" => reply(&mut s, json!({"userAgent": "oneharness-mock-app-server"})),
            "thread/start" => {
                let thread = format!("mock-thread-{}-{}", s.pid, s.next_thread);
                s.next_thread += 1;
                let settings = settings_from(&params, None);
                let model = served_model(&params, &settings);
                open_thread(&mut s, conn, &thread, settings, false);
                let rss = own_rss();
                s.open_rss.push(rss);
                s.save();
                reply(&mut s, json!({"thread": {"id": thread}, "model": model}));
                s.emit(
                    &thread,
                    &json!({"jsonrpc": "2.0", "method": "thread/started", "params": {"thread": {"id": thread}}}),
                );
                s.status(&thread, "idle");
                s.save();
            }
            "thread/resume" => {
                let Some(thread) = params.get("threadId").and_then(Value::as_str) else {
                    return refuse(&mut s, "thread/resume requires a threadId".to_string());
                };
                let base = match s.threads.get(thread) {
                    Some(loaded) => Some(loaded.settings.clone()),
                    None => stored(&s, thread),
                };
                let Some(base) = base else {
                    return refuse(&mut s, format!("thread not found: {thread}"));
                };
                // A resume carries the dispatch's environment; the thread keeps
                // the directory, model, sandbox and approval policy it was
                // started under, so a turn's own values are what is in force.
                let mut settings = base.clone();
                let resumed = settings_from(&params, Some(&base));
                let model = served_model(&params, &resumed);
                settings.set = resumed.set;
                settings.exclude = resumed.exclude;
                open_thread(&mut s, conn, thread, settings, true);
                reply(&mut s, json!({"thread": {"id": thread}, "model": model}));
                s.save();
            }
            "thread/unsubscribe" => {
                let thread = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                // MOCK_CODEX_UNSUBSCRIBE_REFUSE: refuse the release; the
                // thread stays loaded and subscribed.
                if s.threads
                    .get(&thread)
                    .and_then(|t| t.knob("MOCK_CODEX_UNSUBSCRIBE_REFUSE"))
                    .is_some()
                {
                    return refuse(&mut s, format!("the mock refuses to release {thread}"));
                }
                let ignore = s.ignore_unsubscribe;
                let outcome = match s.threads.get_mut(&thread) {
                    None => "notLoaded",
                    Some(loaded) => {
                        if loaded.subscribers.remove(&conn) {
                            "unsubscribed"
                        } else {
                            "notSubscribed"
                        }
                    }
                };
                let empty = s
                    .threads
                    .get(&thread)
                    .is_some_and(|t| t.subscribers.is_empty());
                s.releases
                    .push(json!({"thread": thread, "conn": conn, "status": outcome}));
                reply(&mut s, json!({"status": outcome}));
                if outcome == "unsubscribed" && empty && !ignore {
                    s.threads.remove(&thread);
                    s.send(
                        conn,
                        &json!({"jsonrpc": "2.0", "method": "thread/closed", "params": {"threadId": thread}}),
                    );
                }
                s.save();
            }
            // A shell turn: no model call, ended as soon as it starts.
            "thread/shellCommand" => {
                let thread = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !s.threads.contains_key(&thread) {
                    return refuse(&mut s, format!("thread not found: {thread}"));
                }
                reply(&mut s, json!({}));
                let turn = format!("{thread}-shell");
                s.emit(
                    &thread,
                    &json!({"jsonrpc": "2.0", "method": "turn/started",
                        "params": {"threadId": thread, "turn": {"id": turn}}}),
                );
                s.emit(
                    &thread,
                    &json!({"jsonrpc": "2.0", "method": "turn/completed",
                        "params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}}),
                );
            }
            "thread/loaded/list" => {
                // CODEX_MOCK_LOADED_PAGE: answer in pages of this many, each
                // naming the next by its index.
                let mut data: Vec<String> = s.threads.keys().cloned().collect();
                data.sort();
                let page = std::env::var("CODEX_MOCK_LOADED_PAGE")
                    .ok()
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(usize::MAX);
                let from = params
                    .get("cursor")
                    .and_then(Value::as_str)
                    .and_then(|c| c.parse::<usize>().ok())
                    .unwrap_or(0);
                let to = from.saturating_add(page).min(data.len());
                let next = (to < data.len()).then(|| to.to_string());
                reply(
                    &mut s,
                    json!({"data": data.get(from..to).unwrap_or_default(), "nextCursor": next}),
                );
            }
            "thread/read" => {
                let thread = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if s.threads.contains_key(thread) {
                    reply(&mut s, json!({"thread": {"id": thread}}));
                } else {
                    refuse(&mut s, format!("thread not loaded: {thread}"));
                }
            }
            "turn/start" => start_turn(server, &mut s, conn, &id, &params),
            "turn/interrupt" => {
                let thread = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let turn = params
                    .get("turnId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                s.interrupts
                    .push(json!({"conn": conn, "thread": thread, "turn": turn}));
                s.save();
                let Some(loaded) = s.threads.get(&thread) else {
                    return refuse(&mut s, format!("thread not found: {thread}"));
                };
                if loaded.knob("MOCK_CODEX_INTERRUPT_SILENT").is_some() {
                    return;
                }
                if loaded
                    .knob("MOCK_CODEX_INTERRUPT_THREAD_NOT_FOUND")
                    .is_some()
                {
                    return refuse(&mut s, format!("thread not found: {thread}"));
                }
                match loaded.active_turn.clone() {
                    None if loaded.turns == 0 => {
                        refuse(&mut s, "no active turn to interrupt".to_string())
                    }
                    // Real Codex answers an interrupt naming the turn that
                    // already ended with nothing at all.
                    None => {}
                    Some(active) if active != turn => refuse(
                        &mut s,
                        format!("expected active turn id {turn} but found {active}"),
                    ),
                    Some(_) => {
                        reply(&mut s, json!({}));
                        // The turn ends; the commands it started do not.
                        s.complete_turn(&thread, "interrupted");
                    }
                }
            }
            _ => {
                if !id.is_null() {
                    reply(&mut s, json!({}));
                }
            }
        }
    }

    fn start_turn(server: &Shared, s: &mut Server, conn: u64, id: &Value, params: &Value) {
        let thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(thread) = s.threads.get_mut(&thread_id) else {
            s.send(
                conn,
                &json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32600, "message": format!("thread not found: {thread_id}")}}),
            );
            return;
        };
        thread.turns += 1;
        let turn = format!("{thread_id}-turn-{}", thread.turns);
        thread.active_turn = Some(turn.clone());
        let settings = thread.settings.clone();
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map_or(settings.cwd.clone(), str::to_string);
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(settings.model.clone());
        let approval = params
            .get("approvalPolicy")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(settings.approval.clone());
        let sandbox = params
            .pointer("/sandboxPolicy/type")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(settings.sandbox.clone());
        let input: String = params
            .get("input")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let marker = settings.set.get("ONEHARNESS_DISPATCH_MARKER").cloned();
        s.turns.push(
            json!({"conn": conn, "thread": thread_id, "turn": turn, "cwd": cwd,
            "model": model, "approval": approval, "sandbox": sandbox, "input": input,
            "marker": marker}),
        );
        s.send(conn, &json!({"jsonrpc": "2.0", "id": id, "result": {"turn": {"id": turn, "status": "inProgress"}}}));
        s.emit(
            &thread_id,
            &json!({"jsonrpc": "2.0", "method": "turn/started",
            "params": {"threadId": thread_id, "turn": {"id": turn}}}),
        );
        s.status(&thread_id, "active");
        let mut context =
            json!({"type": "turn_context", "cwd": cwd, "model": model, "sandbox": sandbox});
        if s.fault == "rollout-env" {
            context["env"] = json!(settings.set);
        }
        s.rollout(&thread_id, &context);
        let knob = |name: &str| settings.set.get(name).cloned();
        let reply_text = if knob("MOCK_CODEX_ECHO_PROMPT").is_some() {
            input.clone()
        } else {
            "still working".to_string()
        };
        let item = format!("{turn}-message");
        s.emit(&thread_id, &json!({"jsonrpc": "2.0", "method": "item/agentMessage/delta",
            "params": {"threadId": thread_id, "turnId": turn, "itemId": item, "delta": reply_text}}));
        s.emit(&thread_id, &json!({"jsonrpc": "2.0", "method": "item/completed",
            "params": {"threadId": thread_id, "turnId": turn, "item": {"type": "agentMessage", "id": item, "text": reply_text}}}));
        if knob("MOCK_CODEX_TOOL_EVENTS").is_some() {
            for line in include_str!("../fixtures/codex-app-server-command-execution.jsonl").lines()
            {
                let mut frame: Value =
                    serde_json::from_str(line).expect("captured app-server frame is JSON");
                frame["params"]["threadId"] = json!(thread_id);
                s.emit(&thread_id, &frame);
            }
        }
        if let Some(path) = knob("MOCK_CODEX_HOLD_UNTIL") {
            let server = Arc::clone(server);
            let (thread_id, turn) = (thread_id.clone(), turn.clone());
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let mut s = lock(&server);
                let live = s
                    .threads
                    .get(&thread_id)
                    .is_some_and(|t| t.active_turn.as_deref() == Some(turn.as_str()));
                if !live {
                    return;
                }
                if Path::new(&path).exists() {
                    s.maybe_complete(&thread_id);
                    s.save();
                    return;
                }
            });
        }
        let command = knob("MOCK_CODEX_RUN_COMMAND");
        if let Some(command) = command.clone() {
            if knob("MOCK_CODEX_REQUEST_APPROVAL").is_some() {
                let request = s.next_request;
                s.next_request += 1;
                if let Some(thread) = s.threads.get_mut(&thread_id) {
                    thread.approval_pending = Some(request);
                }
                s.approvals.push(
                    json!({"request": request, "thread": thread_id, "owner": conn, "answers": []}),
                );
                // MOCK_CODEX_SHUT_READ_ON_APPROVAL: stop reading this
                // connection before the request goes out, so the client's
                // answer is always a write onto a connection that is gone.
                if knob("MOCK_CODEX_SHUT_READ_ON_APPROVAL").is_some() {
                    if let Some(c) = s.conns.get_mut(&conn) {
                        c.held = true;
                        let _ = c
                            .writer
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .shutdown(std::net::Shutdown::Read);
                    }
                }
                s.emit(&thread_id, &json!({"jsonrpc": "2.0", "id": request,
                    "method": "item/commandExecution/requestApproval",
                    "params": {"threadId": thread_id, "turnId": turn, "itemId": format!("{turn}-command"), "command": command}}));
                s.save();
                return;
            }
            run_command(
                server,
                s,
                &thread_id,
                &turn,
                &command,
                &cwd,
                sandbox.as_deref(),
            );
        }
        s.maybe_complete(&thread_id);
        s.save();
    }

    fn answer_approval(server: &Shared, s: &mut Server, conn: u64, request: u64, frame: &Value) {
        let decision = frame
            .pointer("/result/decision")
            .cloned()
            .unwrap_or(Value::Null);
        let Some(index) = s
            .approvals
            .iter()
            .position(|a| a["request"] == json!(request))
        else {
            return;
        };
        if let Some(answers) = s.approvals[index]["answers"].as_array_mut() {
            answers.push(json!({"conn": conn, "decision": decision}));
        }
        let thread_id = s.approvals[index]["thread"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let owner_answered = s.approvals[index]["owner"] == json!(conn);
        let pending = s.threads.get(&thread_id).and_then(|t| t.approval_pending);
        if owner_answered && pending == Some(request) {
            if let Some(thread) = s.threads.get_mut(&thread_id) {
                thread.approval_pending = None;
            }
            let (turn, settings) = match s.threads.get(&thread_id) {
                Some(t) => (t.active_turn.clone(), t.settings.clone()),
                None => (None, Settings::default()),
            };
            let approved = decision == json!("approved");
            if let (Some(turn), Some(command), true) = (
                turn,
                settings.set.get("MOCK_CODEX_RUN_COMMAND").cloned(),
                approved,
            ) {
                let cwd = s
                    .turns
                    .iter()
                    .rev()
                    .find(|t| t["turn"] == json!(turn))
                    .and_then(|t| t["cwd"].as_str().map(str::to_string))
                    .unwrap_or(settings.cwd.clone());
                let sandbox = settings.sandbox.clone();
                run_command(
                    server,
                    s,
                    &thread_id,
                    &turn,
                    &command,
                    &cwd,
                    sandbox.as_deref(),
                );
            }
            s.maybe_complete(&thread_id);
        }
        s.save();
    }

    /// Run a turn's command as Codex's unified exec does: a child of the
    /// server in its own process group, under the thread's environment, in
    /// the turn's directory. Under a read-only sandbox it is refused instead.
    fn run_command(
        server: &Shared,
        s: &mut Server,
        thread_id: &str,
        turn: &str,
        command: &str,
        cwd: &str,
        sandbox: Option<&str>,
    ) {
        let item = format!("{turn}-command");
        s.emit(thread_id, &json!({"jsonrpc": "2.0", "method": "item/started",
            "params": {"threadId": thread_id, "turnId": turn,
                "item": {"type": "commandExecution", "id": item, "command": command, "cwd": cwd, "status": "inProgress"}}}));
        if sandbox == Some("readOnly") {
            s.commands.push(json!({"thread": thread_id, "turn": turn, "command": command, "cwd": cwd, "refused": "read-only sandbox"}));
            s.emit(
                thread_id,
                &json!({"jsonrpc": "2.0", "method": "item/completed",
                "params": {"threadId": thread_id, "turnId": turn,
                    "item": {"type": "commandExecution", "id": item, "command": command, "cwd": cwd,
                        "status": "failed", "exitCode": 1,
                        "aggregatedOutput": "sh: cannot write: Read-only file system\n"}}}),
            );
            return;
        }
        // leak-env: every command under the first thread this server opened,
        // whichever dispatch's thread it is running for.
        let settings = match (s.fault.as_str(), s.first_opened.clone()) {
            ("leak-env", Some(first)) => s.known.get(&first).cloned().unwrap_or_default(),
            _ => s
                .threads
                .get(thread_id)
                .map(|t| t.settings.clone())
                .unwrap_or_default(),
        };
        let mut env: BTreeMap<String, String> = std::env::vars().collect();
        env.retain(|name, _| !settings.exclude.iter().any(|excluded| excluded == name));
        env.extend(settings.set.clone());
        if s.fault == "escape-marker" {
            env.remove("ONEHARNESS_DISPATCH_MARKER");
        }
        use std::os::unix::process::CommandExt;
        let child = std::process::Command::new("sh")
            .args(["-c", command])
            .env_clear()
            .envs(&env)
            .current_dir(cwd)
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                s.commands.push(json!({"thread": thread_id, "turn": turn, "command": command, "cwd": cwd, "error": err.to_string()}));
                return;
            }
        };
        s.commands.push(json!({"thread": thread_id, "turn": turn, "command": command, "cwd": cwd, "pid": child.id()}));
        if let Some(thread) = s.threads.get_mut(thread_id) {
            thread.commands_running += 1;
        }
        let server = Arc::clone(server);
        let (thread_id, turn, command, cwd) = (
            thread_id.to_string(),
            turn.to_string(),
            command.to_string(),
            cwd.to_string(),
        );
        std::thread::spawn(move || {
            let mut output = String::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut output);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut output);
            }
            let code = child.wait().ok().and_then(|status| status.code());
            let mut s = lock(&server);
            if let Some(thread) = s.threads.get_mut(&thread_id) {
                thread.commands_running = thread.commands_running.saturating_sub(1);
            }
            s.emit(&thread_id, &json!({"jsonrpc": "2.0", "method": "item/completed",
                "params": {"threadId": thread_id, "turnId": turn,
                    "item": {"type": "commandExecution", "id": format!("{turn}-command"), "command": command, "cwd": cwd,
                        "status": if code == Some(0) { "completed" } else { "failed" },
                        "exitCode": code, "aggregatedOutput": output}}}));
            s.maybe_complete(&thread_id);
            s.save();
        });
    }
}

// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

/// The path a client connects to for a server told to listen on `path`: the
/// symlink's target when Codex left one.
#[cfg(unix)]
fn super_dial(path: &std::path::Path) -> std::path::PathBuf {
    match std::fs::read_link(path) {
        Ok(target) => target,
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::{codex_feature_enabled, codex_model_catalog_arg, codex_plugins_enabled};

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn a_catalog_path_decodes_back_from_the_toml_oneharness_writes_it_as() {
        // Encoded as `domain::harness::codex_model_catalog_setting` encodes it:
        // the path as a TOML string, whatever it holds.
        for path in [
            "/state/oneharness/codex-model-catalog/0f.json",
            r"C:\Users\a b\AppData\Local\oneharness\0f.json",
            "/tmp/it's \"quoted\"\tand\u{e9}\u{1f600}.json",
            "/tmp/a line\nbreak.json",
        ] {
            let setting = format!(
                "model_catalog_json={}",
                toml::Value::String(path.to_string())
            );
            let args = argv(&["exec", "-c", &setting, "go"]);
            assert_eq!(codex_model_catalog_arg(&args).as_deref(), Some(path));
        }
        // A literal string, a value that is no TOML (raw text, as Codex reads
        // it), the last setting winning, and none at all.
        for (args, want) in [
            (
                &["exec", "-c", "model_catalog_json='C:\\x\\y.json'"][..],
                Some(r"C:\x\y.json"),
            ),
            (
                &["exec", "-c", "model_catalog_json=/a b/c.json"],
                Some("/a b/c.json"),
            ),
            (
                &[
                    "exec",
                    "-c",
                    "model_catalog_json=\"/a\"",
                    "-c",
                    "model_catalog_json=\"/b\"",
                ],
                Some("/b"),
            ),
            (&["exec", "-c", "analytics.enabled=false"], None),
        ] {
            assert_eq!(
                codex_model_catalog_arg(&argv(args)).as_deref(),
                want,
                "{args:?}"
            );
        }
    }

    #[test]
    fn codex_plugins_resolve_by_codexs_measured_precedence() {
        // The rows `oh_codex_feature_flag_order` asks real codex, from the one
        // matrix both read.
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/codex-feature-flag-order.json"))
                .expect("the codex plugins matrix is JSON");
        let rows = matrix["rows"].as_array().expect("rows");
        assert!(rows.len() >= 12, "{rows:?}");
        for row in rows {
            let mut args = argv(&["exec"]);
            args.extend(
                row["args"]
                    .as_array()
                    .expect("args")
                    .iter()
                    .map(|arg| arg.as_str().expect("an arg is text").to_string()),
            );
            let want = row["plugins"].as_bool().expect("plugins is a bool");
            assert_eq!(codex_plugins_enabled(&args), want, "{args:?}");
        }
        // Other features, and other spellings, leave it alone.
        for (args, want) in [
            (
                &["exec", "--disable", "apps", "-c", "features.hooks=false"][..],
                true,
            ),
            (
                &["exec", "--disable", "plugins", "--enable", "hooks"],
                false,
            ),
        ] {
            assert_eq!(codex_plugins_enabled(&argv(args)), want, "{args:?}");
        }
    }

    #[test]
    fn codex_apps_resolve_by_the_same_precedence() {
        // `codex features list` gives `apps` the plugins rows' answers with the
        // feature name swapped (measured on codex-cli 0.159.2), and neither
        // feature's toggles move the other.
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/codex-feature-flag-order.json"))
                .expect("the codex plugins matrix is JSON");
        for row in matrix["rows"].as_array().expect("rows") {
            let mut args = argv(&["exec"]);
            args.extend(row["args"].as_array().expect("args").iter().map(|arg| {
                arg.as_str()
                    .expect("an arg is text")
                    .replace("plugins", "apps")
            }));
            let want = row["plugins"].as_bool().expect("plugins is a bool");
            assert_eq!(codex_feature_enabled(&args, "apps"), want, "{args:?}");
            assert!(codex_plugins_enabled(&args), "{args:?}");
        }
    }
}
