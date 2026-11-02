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
//!                   `$CODEX_HOME/sessions/2026/11/02/rollout-…-<id>.jsonl`:
//!                   `exec [resume <id>] … <prompt>` appends the turn and the
//!                   captured per-call `token_count` events
//!                   (`tests/fixtures/codex-rollout-token-counts.jsonl`), and a
//!                   stdio `app-server` answers `thread/fork` and
//!                   `thread/compact/start` as the captured exchange does.
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

/// Hold a compaction mid-rewrite when the journey asked to cancel it there.
fn begun() {
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
        let from = store.join(format!("{original}.jsonl"));
        let fork = fresh_id();
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

fn codex_store() -> PathBuf {
    PathBuf::from(std::env::var("CODEX_HOME").expect("MOCK_CODEX_SESSIONS needs CODEX_HOME"))
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
    let mut lines = vec![serde_json::json!({
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
                let fork = fresh_id();
                std::fs::copy(&from, rollout(&fork)).expect("mock fork copy");
                send(&[serde_json::json!({"id": id, "result": {"thread": {"id": fork, "forkedFromId": original}}}).to_string()]);
            }
            Some("thread/compact/start") => {
                let fork = request["params"]["threadId"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let path = rollout(&fork);
                let summary = format!("summary of: {}", codex_context(&path).join(", "));
                send(&[serde_json::json!({"id": id, "result": {}}).to_string()]);
                send(&recorded(&fork, |v| {
                    matches!(v["method"].as_str(), Some("turn/started" | "item/started"))
                }));
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
                send(&recorded(&fork, |v| {
                    matches!(
                        v["method"].as_str(),
                        Some("thread/tokenUsage/updated" | "item/completed" | "turn/completed")
                    ) && v["params"]["turn"]["id"] != "01a19db2-d12d-7391-9e3c-b345b40db491"
                }));
            }
            _ => {}
        }
    }
    std::process::exit(0);
}
