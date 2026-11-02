//! The responder's stand-ins for a harness that keeps its own session store,
//! so a journey can compact a session and see what the next turn is handed.
//!
//!   MOCK_CLAUDE_SESSIONS  act like Claude Code keeping transcripts under
//!                   `$CLAUDE_CONFIG_DIR/projects/oneharness-mock/<id>.jsonl`:
//!                   `-p <prompt>` starts one, `--resume <id>` appends to it,
//!                   and `-p /compact --resume <id> --fork-session` writes a
//!                   compacted FORK and replays the captured compaction
//!                   (`tests/fixtures/claude-compact-fork.jsonl`) under the
//!                   fork's id. Every turn answers `context: [...]`, the
//!                   prompts the conversation it was handed carries (a
//!                   compacted one starts from its summary).
//!   MOCK_CODEX_SESSIONS   the same for codex under
//!                   `$CODEX_HOME` (else `$HOME/.codex`)
//!                   `/sessions/2026/11/02/rollout-…-<id>.jsonl`:
//!                   `exec [resume <id>] … <prompt>` appends the turn and the
//!                   captured per-call `token_count` events
//!                   (`tests/fixtures/codex-rollout-token-counts.jsonl`), and a
//!                   stdio `app-server` answers `thread/fork` and
//!                   `thread/compact/start` as the captured exchange does.
//!   MOCK_COMPACT_LOG      with MOCK_CODEX_SESSIONS, every request the stdio
//!                   `app-server` receives is appended here, one per line.
//!   MOCK_COMPACT_FAIL     with either, the compaction fails as that harness
//!                   does: `refuse` (claude's `compact_result: failed`, codex's
//!                   error answer), `exit` (the process dies), `nofork` (claude
//!                   names no forked session), or for codex `fail-turn` (the
//!                   turn ends `failed`), `noresult` (`thread/fork` answers no
//!                   result), `samefork` (it names the original thread) or
//!                   `exit-after` (it completes, then exits non-zero).
//!   MOCK_COMPACT_NOISE    with MOCK_CODEX_SESSIONS, the compaction is surrounded
//!                   by another thread's turn and another turn of the fork.
//!   MOCK_COMPACT_USAGE    with MOCK_CODEX_SESSIONS, the fork's token totals grow
//!                   by 60 input and 9 output tokens over the compaction.
//!   MOCK_COMPACT_DELAY_MS with either, a compaction pauses this long midway
//!                   and then completes.
//!   MOCK_COMPACT_BEGUN    with either, a compaction writes part of the fork,
//!                   writes its own pid to this path, and then holds until it is
//!                   killed — the compaction a caller cancels mid-rewrite.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const CLAUDE_COMPACT: &str = include_str!("../fixtures/claude-compact-fork.jsonl");
const CLAUDE_COMPACT_ID: &str = "9e02ec47-3a9e-4737-a89a-ce9ed05abf43";
const CODEX_TOKEN_COUNTS: &str = include_str!("../fixtures/codex-rollout-token-counts.jsonl");
const CODEX_FORK_COMPACT: &str = include_str!("../fixtures/codex-app-server-fork-compact.jsonl");
const CODEX_FORK_ID: &str = "01a19db5-ae1c-77d2-ad64-b3e9aa7b861e";
/// The captured compaction turn's id.
const CODEX_COMPACT_TURN: &str = "01a19db5-aef1-7f73-a78a-d9ccb96b5511";

/// Answer as a session-keeping harness when one of the knobs is set.
pub fn run_if_requested(argv: &[String]) {
    if std::env::var_os("MOCK_CLAUDE_SESSIONS").is_some() && argv.iter().any(|a| a == "-p") {
        claude(argv);
    }
    if std::env::var_os("MOCK_CODEX_SESSIONS").is_some() {
        match argv.first().map(String::as_str) {
            Some("exec") => codex_exec(argv),
            Some("app-server") => codex_app_server(),
            _ => {}
        }
    }
}

fn fresh_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("mock-{nanos:x}-{}", std::process::id())
}

fn value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

fn append(path: &Path, lines: &[String]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("mock session dir");
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("mock session file");
    for line in lines {
        writeln!(file, "{line}").expect("mock session write");
    }
}

/// How a scripted compaction fails (`MOCK_COMPACT_FAIL`): `refuse`, `exit`,
/// and per harness `nofork` (claude-code) or `fail-turn` (codex).
fn compact_fault() -> Option<String> {
    std::env::var("MOCK_COMPACT_FAIL").ok()
}

/// Hold a compaction mid-rewrite when the journey asked to cancel it there,
/// or slow it down (`MOCK_COMPACT_DELAY_MS`) when it asked for a slow one.
fn begun() {
    if let Some(ms) = std::env::var("MOCK_COMPACT_DELAY_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Ok(path) = std::env::var("MOCK_COMPACT_BEGUN") {
        std::fs::write(&path, std::process::id().to_string()).expect("begun marker");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}

fn say(lines: &[String]) -> ! {
    let mut out = std::io::stdout();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    std::process::exit(0);
}

fn claude_store() -> PathBuf {
    PathBuf::from(
        std::env::var("CLAUDE_CONFIG_DIR").expect("MOCK_CLAUDE_SESSIONS needs CLAUDE_CONFIG_DIR"),
    )
    .join("projects")
    .join("oneharness-mock")
}

/// The prompts a transcript hands its next turn: from the last compact
/// boundary's summary on, else all of them.
fn claude_context(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut context = Vec::new();
    for value in text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
    {
        if value["subtype"] == "compact_boundary" {
            context.clear();
        }
        if value["type"] == "user" {
            if let Some(content) = value["message"]["content"].as_str() {
                context.push(content.to_string());
            }
        }
    }
    context
}

fn claude(argv: &[String]) -> ! {
    let prompt = value_after(argv, "-p").unwrap_or_default().to_string();
    let store = claude_store();
    let resumed = value_after(argv, "--resume").map(str::to_string);
    if prompt == "/compact" {
        let original = resumed.expect("/compact resumes a session");
        match compact_fault().as_deref() {
            Some("refuse") => say(&[serde_json::json!({
                "type": "system", "subtype": "status", "status": null,
                "compact_result": "failed", "compact_error": "Not enough messages to compact.",
                "session_id": original,
            })
            .to_string()]),
            Some("exit") => {
                eprintln!("mock claude: compaction crashed");
                std::process::exit(1);
            }
            // The captured stream, but under the ORIGINAL session's id: a
            // compaction that names no fork.
            Some("nofork") => say(&CLAUDE_COMPACT
                .lines()
                .map(|l| l.replace(CLAUDE_COMPACT_ID, &original))
                .collect::<Vec<_>>()),
            _ => {}
        }
        let from = store.join(format!("{original}.jsonl"));
        // Without `--fork-session` Claude Code compacts the original
        // transcript in place, appending to it (as captured).
        let fork = if argv.iter().any(|a| a == "--fork-session") {
            fresh_id()
        } else {
            original.clone()
        };
        let to = store.join(format!("{fork}.jsonl"));
        let summary = format!("summary of: {}", claude_context(&from).join(", "));
        append(
            &to,
            &[
                serde_json::json!({"type":"system","subtype":"compact_boundary","session_id":fork})
                    .to_string(),
            ],
        );
        begun();
        append(
            &to,
            &[serde_json::json!({"type":"user","message":{"role":"user","content":summary},"session_id":fork}).to_string()],
        );
        let lines: Vec<String> = CLAUDE_COMPACT
            .lines()
            .map(|l| l.replace(CLAUDE_COMPACT_ID, &fork))
            .collect();
        say(&lines);
    }
    let (id, path) = match resumed {
        Some(id) => {
            let path = store.join(format!("{id}.jsonl"));
            if !path.is_file() {
                eprintln!("No conversation found with session ID: {id}");
                std::process::exit(1);
            }
            (id, path)
        }
        None => {
            let id = fresh_id();
            let path = store.join(format!("{id}.jsonl"));
            (id, path)
        }
    };
    append(
        &path,
        &[serde_json::json!({"type":"user","message":{"role":"user","content":prompt},"session_id":id}).to_string()],
    );
    let text = format!("context: [{}]", claude_context(&path).join(", "));
    let result = serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": text, "session_id": id,
        "usage": {"input_tokens": 3, "output_tokens": 2},
    });
    say(&[result.to_string()]);
}

/// Where codex keeps its sessions: `CODEX_HOME`, else `$HOME/.codex`.
fn codex_store() -> PathBuf {
    std::env::var("CODEX_HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .expect("MOCK_CODEX_SESSIONS needs CODEX_HOME or HOME")
        .join("sessions")
        .join("2026")
        .join("11")
        .join("02")
}

fn rollout(thread: &str) -> PathBuf {
    codex_store().join(format!("rollout-2026-11-02T00-00-00-{thread}.jsonl"))
}

/// The prompts a rollout hands its next turn: from the last `compacted`
/// item's summary on, else all of them.
fn codex_context(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut context = Vec::new();
    for value in text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
    {
        if value["type"] == "compacted" {
            context = vec![value["payload"]["message"]
                .as_str()
                .unwrap_or("")
                .to_string()];
        }
        if value["type"] == "response_item" && value["payload"]["role"] == "user" {
            if let Some(text) = value["payload"]["content"][0]["text"].as_str() {
                context.push(text.to_string());
            }
        }
    }
    context
}

fn codex_exec(argv: &[String]) -> ! {
    let resuming = argv.get(1).map(String::as_str) == Some("resume");
    // The positionals, past every flag and the value a value-taking flag owns:
    // `[<id>,] <prompt>`, wherever oneharness put its own `-c` settings.
    let mut positionals = Vec::new();
    let mut rest = argv.iter().skip(if resuming { 2 } else { 1 });
    while let Some(arg) = rest.next() {
        if matches!(
            arg.as_str(),
            "-c" | "--config" | "-m" | "--model" | "-s" | "--sandbox"
        ) {
            rest.next();
        } else if !arg.starts_with('-') {
            positionals.push(arg.clone());
        }
    }
    let prompt = positionals.last().cloned().unwrap_or_default();
    let resumed = resuming.then(|| positionals.first().cloned()).flatten();
    let thread = match &resumed {
        Some(id) => {
            if !rollout(id).is_file() {
                eprintln!("thread not found: {id}");
                std::process::exit(1);
            }
            id.clone()
        }
        None => fresh_id(),
    };
    let path = rollout(&thread);
    // Each turn begins with codex's `task_started`, as a real rollout does.
    let mut lines = vec![
        serde_json::json!({"type": "event_msg", "payload": {"type": "task_started"}}).to_string(),
        serde_json::json!({
        "type": "response_item",
        "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": prompt}]},
    })
    .to_string()];
    lines.extend(CODEX_TOKEN_COUNTS.lines().map(str::to_string));
    append(&path, &lines);
    let text = format!("context: [{}]", codex_context(&path).join(", "));
    say(&[
        serde_json::json!({"type":"thread.started","thread_id":thread}).to_string(),
        serde_json::json!({"type":"turn.started"}).to_string(),
        serde_json::json!({"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":text}}).to_string(),
        serde_json::json!({"type":"turn.completed","usage":{"input_tokens":39287,"cached_input_tokens":37120,"output_tokens":64}}).to_string(),
    ]);
}

/// The captured fork compaction's server lines, retargeted at `fork`.
fn recorded(fork: &str, keep: impl Fn(&serde_json::Value) -> bool) -> Vec<String> {
    CODEX_FORK_COMPACT
        .lines()
        .filter(|l| serde_json::from_str::<serde_json::Value>(l).is_ok_and(|v| keep(&v)))
        .map(|l| l.replace(CODEX_FORK_ID, fork))
        .collect()
}

fn codex_app_server() -> ! {
    let mut out = std::io::stdout();
    let mut send = |lines: &[String]| {
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
        let _ = out.flush();
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Ok(log) = std::env::var("MOCK_COMPACT_LOG") {
            append(Path::new(&log), std::slice::from_ref(&line));
        }
        let id = request["id"].clone();
        match request["method"].as_str() {
            Some("initialize") => {
                send(&[serde_json::json!({"id": id, "result": {"userAgent": "mock"}}).to_string()]);
            }
            Some("thread/fork") => {
                let original = request["params"]["threadId"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let from = rollout(&original);
                if !from.is_file() {
                    send(&[serde_json::json!({"id": id, "error": {"code": -32600, "message": format!("thread not found: {original}")}}).to_string()]);
                    continue;
                }
                match compact_fault().as_deref() {
                    Some("noresult") => {
                        send(&[serde_json::json!({"id": id}).to_string()]);
                        continue;
                    }
                    Some("samefork") => {
                        send(&[
                            serde_json::json!({"id": id, "result": {"thread": {"id": original}}})
                                .to_string(),
                        ]);
                        continue;
                    }
                    _ => {}
                }
                let fork = fresh_id();
                std::fs::copy(&from, rollout(&fork)).expect("mock fork copy");
                send(&[serde_json::json!({"id": id, "result": {"thread": {"id": fork, "forkedFromId": original}}}).to_string()]);
            }
            Some("thread/compact/start") if compact_fault().as_deref() == Some("refuse") => {
                send(&[serde_json::json!({"id": id, "error": {"code": -32600, "message": "compaction unavailable"}}).to_string()]);
            }
            Some("thread/compact/start") if compact_fault().as_deref() == Some("exit") => {
                std::process::exit(1);
            }
            Some("thread/compact/start") if compact_fault().as_deref() == Some("fail-turn") => {
                let fork = request["params"]["threadId"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                send(&[serde_json::json!({"id": id, "result": {}}).to_string()]);
                send(&recorded(&fork, |v| v["method"] == "turn/started"));
                send(&[serde_json::json!({"method": "turn/completed", "params": {
                    "threadId": fork,
                    "turn": {"id": "01a19db5-aef1-7f73-a78a-d9ccb96b5511", "status": "failed",
                             "error": {"message": "the compaction request failed"}},
                }})
                .to_string()]);
            }
            Some("thread/compact/start") => {
                let fork = request["params"]["threadId"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let path = rollout(&fork);
                let summary = format!("summary of: {}", codex_context(&path).join(", "));
                send(&[serde_json::json!({"id": id, "result": {}}).to_string()]);
                let usage = |input: u64, output: u64, turn: &str| {
                    serde_json::json!({"method": "thread/tokenUsage/updated", "params": {
                        "threadId": fork, "turnId": turn,
                        "tokenUsage": {"total": {"inputTokens": input, "outputTokens": output,
                            "cachedInputTokens": 0, "cacheWriteInputTokens": 0, "totalTokens": input + output}},
                    }})
                    .to_string()
                };
                if std::env::var_os("MOCK_COMPACT_USAGE").is_some() {
                    send(&[usage(100, 10, "earlier-turn")]);
                }
                if std::env::var_os("MOCK_COMPACT_NOISE").is_some() {
                    // Another thread's turn, starting and completing first.
                    send(&[
                        serde_json::json!({"method": "turn/started", "params": {"threadId": "someone-else", "turn": {"id": "x"}}}).to_string(),
                        serde_json::json!({"method": "turn/completed", "params": {"threadId": "someone-else", "turn": {"id": "x", "status": "failed"}}}).to_string(),
                    ]);
                }
                send(&recorded(&fork, |v| {
                    matches!(v["method"].as_str(), Some("turn/started" | "item/started"))
                }));
                if std::env::var_os("MOCK_COMPACT_NOISE").is_some() {
                    // A different turn of the fork ending: not the compaction.
                    send(&[serde_json::json!({"method": "turn/completed", "params": {"threadId": fork, "turn": {"id": "not-the-compaction", "status": "failed"}}}).to_string()]);
                }
                if std::env::var_os("MOCK_COMPACT_USAGE").is_some() {
                    send(&[usage(160, 19, CODEX_COMPACT_TURN)]);
                }
                append(
                    &path,
                    &[serde_json::json!({"type":"event_msg","payload":{"type":"context_compaction_started"}}).to_string()],
                );
                begun();
                append(
                    &path,
                    &[
                        serde_json::json!({"type":"compacted","payload":{"message":summary}})
                            .to_string(),
                    ],
                );
                // With scripted usage, its series replaces the recorded one: a
                // server reports one consistent set of totals.
                let scripted_usage = std::env::var_os("MOCK_COMPACT_USAGE").is_some();
                send(&recorded(&fork, |v| {
                    let method = v["method"].as_str();
                    (matches!(method, Some("item/completed" | "turn/completed"))
                        || (method == Some("thread/tokenUsage/updated") && !scripted_usage))
                        && v["params"]["turn"]["id"] != "01a19db2-d12d-7391-9e3c-b345b40db491"
                }));
                if compact_fault().as_deref() == Some("exit-after") {
                    std::process::exit(1);
                }
            }
            _ => {}
        }
    }
    std::process::exit(0);
}
