//! Recovering a `crush run` reply that crush's stdout cut short, from crush's own
//! session store. Pure: this builds the commands that read the store and parses
//! what they print; `io::crush_store` spawns them.
//!
//! `crush run` prints the reply as it streams, and stops printing when its agent
//! finishes, even if deltas are still queued (crush v0.102.0's
//! `RunNonInteractive` `select`s between both at random). The store still holds
//! the whole turn, so a reply whose stdout is a strict prefix of what the store
//! holds is read from the store. Crush has no JSON output on `run`, so the store
//! is reached through `crush session list --json` and `crush session show <id>
//! --json`, which crush documents for agents to read. Both read the `sessions`
//! and `messages` tables of `<data-dir>/crush.db`. The shapes read here are
//! captured from crush v0.87.0 in `tests/fixtures/crush-0.87.0-*`, and
//! `just live-crush` fails when a real run cannot be read back.

use crate::domain::usage::epoch_millis_from_rfc3339;
use serde::{Deserialize, Deserializer};

/// `text_source` when `text` was read from crush's session store.
pub const RECOVERED_SOURCE: &str = "crush-session";
/// `text_source` when `text` is stdout because the store could not be read.
pub const UNAVAILABLE_SOURCE: &str = "raw:crush-session-unavailable";
/// `text_source` when `text` is stdout because the run's session could not be
/// told apart in the store.
pub const UNIDENTIFIED_SOURCE: &str = "raw:crush-session-unidentified";

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] These flag spellings and their meaning (`-s/--session` names the session, `-C/--continue` the most recent one, `-D/--data-dir` and `-c/--cwd` choose the store; `-H/--host` takes a value but `crush session` opens the store from those two alone, per `sessionSetup` in `internal/cmd/session.go` at both releases) are `crush run --help`'s in v0.87.0 and crush's `internal/cmd/run.go` and `root.go` at v0.102.0; crush publishes no machine-readable flag list to generate them from. Drift fails safe: a flag crush adds later is skipped as a positional, and a session or store crush stops naming this way leaves the run's session unidentified or its store unreadable, so `text` stays stdout's and `text_source` says so — never another run's reply, because the store's reply is taken only when stdout is its strict prefix. `just live-crush` (ci.yml's `check` job, on every pull request that changes crates/**) reads a real run back through these argvs.
/// `crush run` flags that take a value, so the parser skips the value too.
const VALUED_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "--small-model",
    "--reasoning-effort",
    "-s",
    "--session",
    "-D",
    "--data-dir",
    "-c",
    "--cwd",
    "-H",
    "--host",
];

/// A flag that chooses which store a `crush` command opens. `crush session`
/// takes each of them, so the store read is the one the run wrote to. (`-H`
/// picks a server, not a store: `crush session` never reads it.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFlag {
    /// The flag as the run spelled it (`-D` or `--data-dir`, …).
    flag: &'static str,
    value: String,
}

impl StoreFlag {
    fn of(flag: &str, value: String) -> Option<Self> {
        ["-D", "--data-dir", "-c", "--cwd"]
            .into_iter()
            .find(|known| *known == flag)
            .map(|flag| Self { flag, value })
    }
}

/// Which session a `crush run` wrote its turn to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionChoice {
    /// A new session, which the run names nowhere it prints.
    #[default]
    Created,
    /// The session `--session` named.
    Named(SessionId),
    /// `--continue`: the most recent session, which the run names nowhere.
    Continued,
}

/// What a `crush run` argv says about the store it wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStore {
    pub session: SessionChoice,
    flags: Vec<StoreFlag>,
}

/// Read the store-selecting flags off a `crush run` argv (`argv[0]` is the
/// binary). An argv that is not a `run` names nothing.
///
/// # Errors
/// [`StoreMiss::Unreadable`] when `--session` names no valid session id.
pub fn run_store(argv: &[String]) -> Result<RunStore, StoreMiss> {
    let mut store = RunStore::default();
    if argv.get(1).map(String::as_str) != Some("run") {
        return Ok(store);
    }
    let mut args = argv[2..].iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with('-') => (flag, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        if matches!(flag, "-C" | "--continue") {
            store.session = SessionChoice::Continued;
            continue;
        }
        if !VALUED_FLAGS.contains(&flag) {
            continue;
        }
        let Some(value) = inline.or_else(|| args.next().cloned()) else {
            break;
        };
        if matches!(flag, "-s" | "--session") {
            store.session = SessionChoice::Named(SessionId::new(&value)?);
        } else if let Some(store_flag) = StoreFlag::of(flag, value) {
            store.flags.push(store_flag);
        }
    }
    Ok(store)
}

// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

fn with_store_flags(mut argv: Vec<String>, store: &RunStore) -> Vec<String> {
    for StoreFlag { flag, value } in &store.flags {
        argv.push((*flag).to_string());
        argv.push(value.clone());
    }
    argv
}

/// `crush session list --json`, against the run's store.
#[must_use]
pub fn list_argv(bin: &str, store: &RunStore) -> Vec<String> {
    let argv = ["session", "list", "--json"].map(String::from);
    with_store_flags([bin.to_string()].into_iter().chain(argv).collect(), store)
}

/// `crush session show <id> --json`, against the run's store.
#[must_use]
pub fn show_argv(bin: &str, session: &SessionId, store: &RunStore) -> Vec<String> {
    let argv = vec![
        bin.to_string(),
        "session".into(),
        "show".into(),
        session.0.clone(),
        "--json".into(),
    ];
    with_store_flags(argv, store)
}

/// A session id fit to pass to `crush session show`: non-empty, and not
/// readable as a flag or carrying control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    /// # Errors
    /// [`StoreMiss::Unreadable`] naming the id when it is not one.
    pub fn new(id: &str) -> Result<Self, StoreMiss> {
        if id.is_empty() || id.starts_with('-') || id.chars().any(char::is_control) {
            return Err(StoreMiss::Unreadable(format!(
                "`{}` is not a crush session id",
                id.escape_debug()
            )));
        }
        Ok(Self(id.to_string()))
    }
}

// llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Crush publishes no schema for `session list|show --json` to generate these from. They read the fields of crush v0.87.0's real output, captured byte for byte in tests/fixtures/crush-0.87.0-session-*.json and unit-tested below, and the producer is gated live: ci.yml's `check` job runs `just live-crush` against the real installed crush on every pull request that changes crates/**, and scripts/e2e-crush.sh fails it when the run cannot be read back from the store.
/// One row of `crush session list --json`.
#[derive(Debug, Deserialize)]
struct ListedSession {
    #[serde(deserialize_with = "session_id")]
    uuid: SessionId,
    /// Crush stores `created_at` in whole seconds and prints it as RFC 3339.
    #[serde(deserialize_with = "epoch_millis")]
    created: i64,
}

fn session_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SessionId, D::Error> {
    let text = String::deserialize(deserializer)?;
    SessionId::new(&text).map_err(|_| {
        serde::de::Error::custom(format!(
            "`{}` is not a crush session id",
            text.escape_debug()
        ))
    })
}

fn epoch_millis<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    let text = String::deserialize(deserializer)?;
    epoch_millis_from_rfc3339(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not an RFC 3339 instant")))
}

/// Why the run's session could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreMiss {
    /// The store's answer was not the documented JSON.
    Unreadable(String),
    /// The store holds no single session this run alone can have created.
    Unidentified(String),
}

/// The session a fresh `crush run` created: the one top-level session the
/// store lists as created no earlier than the second the run was planned in.
/// `crush run` creates its session before it prints anything and names it
/// nowhere it prints, so creation time is the one mark the run leaves. Two
/// such sessions (another run in the same store meanwhile) leave this run's
/// unknown, never guessed.
///
/// # Errors
/// [`StoreMiss::Unreadable`] when `list_json` is not the session list;
/// [`StoreMiss::Unidentified`] when no single session qualifies.
pub fn created_session(list_json: &str, since_ms: i64) -> Result<SessionId, StoreMiss> {
    let listed: Vec<ListedSession> = serde_json::from_str(list_json.trim())
        .map_err(|err| StoreMiss::Unreadable(format!("`crush session list --json`: {err}")))?;
    let since_second = since_ms.div_euclid(1_000) * 1_000;
    let mut created = listed
        .into_iter()
        .filter(|session| session.created >= since_second);
    match (created.next(), created.next()) {
        (Some(session), None) => Ok(session.uuid),
        (None, _) => Err(StoreMiss::Unidentified(
            "the store lists no session created since the run began".to_string(),
        )),
        (Some(_), Some(_)) => Err(StoreMiss::Unidentified(
            "the store lists more than one session created since the run began".to_string(),
        )),
    }
}

/// `crush session show <id> --json`, as far as the reply needs it.
#[derive(Debug, Deserialize)]
struct ShownSession {
    messages: Vec<ShownMessage>,
}

#[derive(Debug, Deserialize)]
struct ShownMessage {
    role: Role,
    #[serde(default)]
    parts: Vec<ShownPart>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Role {
    User,
    Assistant,
    /// `tool`, `system`, or a role a later crush adds.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ShownPart {
    Text {
        text: String,
    },
    /// Reasoning, tool calls and results, finish markers, images, …
    #[serde(other)]
    Other,
}

/// The reply of the session's last turn, as `crush run` prints it: the text of
/// every assistant message after the last user message, in order, each with
/// the leading newline and indentation crush drops before printing a message.
///
/// # Errors
/// [`StoreMiss::Unreadable`] when `show_json` is not a shown session.
pub fn turn_reply(show_json: &str) -> Result<String, StoreMiss> {
    let shown: ShownSession = serde_json::from_str(show_json.trim())
        .map_err(|err| StoreMiss::Unreadable(format!("`crush session show --json`: {err}")))?;
    let turn_start = shown
        .messages
        .iter()
        .rposition(|message| message.role == Role::User)
        .map_or(0, |user| user + 1);
    Ok(shown.messages[turn_start..]
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| {
            let text: String = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    ShownPart::Text { text } => Some(text.as_str()),
                    ShownPart::Other => None,
                })
                .collect();
            let text = text.strip_prefix('\n').unwrap_or(&text);
            text.trim_start_matches([' ', '\t']).to_string()
        })
        .collect())
}
// llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

/// The reply a run reports: the store's when stdout carried only its start.
/// Anything else — the same text, or a store reply stdout does not begin —
/// keeps stdout, which is what the harness itself printed.
#[must_use]
pub fn recovered_reply(stdout_text: &str, store_reply: &str) -> Option<String> {
    let printed = stdout_text.trim();
    let stored = store_reply.trim();
    (stored.len() > printed.len() && stored.starts_with(printed)).then(|| stored.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = include_str!("../../../../tests/fixtures/crush-0.87.0-session-show.json");
    const LIST: &str = include_str!("../../../../tests/fixtures/crush-0.87.0-session-list.json");
    const STDOUT: &str = include_str!("../../../../tests/fixtures/crush-0.87.0-run-stdout.txt");

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn run_store_reads_the_session_and_store_flags_in_either_spelling() {
        let store = run_store(&argv(&[
            "crush",
            "run",
            "-q",
            "--session=abc",
            "-m",
            "sonnet",
            "-D",
            "/data",
            "--cwd=/work",
            "hello",
        ]))
        .unwrap();
        let id = SessionId::new("abc").unwrap();
        assert_eq!(store.session, SessionChoice::Named(id.clone()));
        assert_eq!(
            show_argv("crush", &id, &store),
            argv(&["crush", "session", "show", "abc", "--json", "-D", "/data", "--cwd", "/work"])
        );
        assert_eq!(
            list_argv("crush", &store),
            argv(&["crush", "session", "list", "--json", "-D", "/data", "--cwd", "/work"])
        );
    }

    #[test]
    fn run_store_names_nothing_for_another_verb_or_past_a_separator() {
        assert_eq!(
            run_store(&argv(&["crush", "session", "-s", "x"])),
            Ok(RunStore::default())
        );
        let store = run_store(&argv(&["crush", "run", "-C", "--", "-s", "x"])).unwrap();
        assert_eq!(store.session, SessionChoice::Continued);
        // A valued flag with nothing after it ends the parse rather than panicking.
        assert_eq!(
            run_store(&argv(&["crush", "run", "-s"])).unwrap().session,
            SessionChoice::Created
        );
        assert!(matches!(
            run_store(&argv(&["crush", "run", "--session="])),
            Err(StoreMiss::Unreadable(_))
        ));
    }

    #[test]
    fn created_session_is_the_one_session_created_since_the_run_began() {
        // The capture's one session was created at 2026-11-22T23:10:09Z; a run
        // planned half a second into that second still finds it, because crush
        // records the second without its fraction.
        let created = epoch_millis_from_rfc3339("2026-11-22T23:10:09Z").unwrap();
        assert_eq!(
            created_session(LIST, created + 500),
            SessionId::new("eb0eee6a-6bb0-4ee0-a505-6b882d77fb71")
        );
        assert!(matches!(
            created_session(LIST, created + 1_000),
            Err(StoreMiss::Unidentified(why)) if why.contains("no session")
        ));
        let two = r#"[{"uuid":"a","created":"2026-11-22T23:10:09Z"},
                      {"uuid":"b","created":"2026-11-22T23:10:10Z"}]"#;
        assert!(matches!(
            created_session(two, created),
            Err(StoreMiss::Unidentified(why)) if why.contains("more than one")
        ));
        assert!(matches!(
            created_session("not json", created),
            Err(StoreMiss::Unreadable(_))
        ));
        // A malformed creation time is an unreadable list, not a session left out.
        assert!(matches!(
            created_session(r#"[{"uuid":"a","created":"yesterday"}]"#, created),
            Err(StoreMiss::Unreadable(why)) if why.contains("yesterday")
        ));
        assert!(matches!(
            created_session(r#"[{"uuid":"--help","created":"2026-11-22T23:10:09Z"}]"#, created),
            Err(StoreMiss::Unreadable(why)) if why.contains("--help")
        ));
    }

    #[test]
    fn turn_reply_is_what_crush_printed_for_the_captured_turn() {
        // The capture's turn is two assistant messages around a tool call, each
        // stored with leading whitespace crush did not print.
        assert_eq!(turn_reply(SHOW), Ok(STDOUT.trim().to_string()));
        assert!(matches!(turn_reply("{}"), Err(StoreMiss::Unreadable(_))));
    }

    #[test]
    fn turn_reply_reads_only_the_last_turn() {
        let show = r#"{"messages":[
            {"role":"user","parts":[{"type":"text","text":"first"}]},
            {"role":"assistant","parts":[{"type":"text","text":"earlier turn"}]},
            {"role":"user","parts":[{"type":"text","text":"second"}]},
            {"role":"assistant","parts":[{"type":"reasoning","thinking":"hm"},{"type":"text","text":"\n  Later."}]}
        ]}"#;
        assert_eq!(turn_reply(show), Ok("Later.".to_string()));
    }

    #[test]
    fn recovered_reply_takes_the_store_only_past_a_printed_prefix() {
        assert_eq!(
            recovered_reply("MARK-3", "MARK-3-alpha\n"),
            Some("MARK-3-alpha".to_string())
        );
        assert_eq!(recovered_reply("", "whole"), Some("whole".to_string()));
        assert_eq!(recovered_reply("MARK-3-alpha\n", "MARK-3-alpha"), None);
        assert_eq!(recovered_reply("other", "MARK-3-alpha"), None);
    }
}
