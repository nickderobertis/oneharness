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
//! and `messages` tables of `<data-dir>/crush.db`.

use crate::domain::usage::epoch_millis_from_rfc3339;
use serde::Deserialize;

/// `text_source` when `text` was read from crush's session store.
pub const RECOVERED_SOURCE: &str = "crush-session";
/// `text_source` when `text` is stdout because the store could not be read.
pub const UNAVAILABLE_SOURCE: &str = "raw:crush-session-unavailable";
/// `text_source` when `text` is stdout because the run's session could not be
/// told apart in the store.
pub const UNIDENTIFIED_SOURCE: &str = "raw:crush-session-unidentified";

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

/// The flags that choose which store a `crush` command opens. `crush session`
/// takes each of them, so the store is the one the run wrote to.
const STORE_FLAGS: &[&str] = &["-D", "--data-dir", "-c", "--cwd", "-H", "--host"];

/// What a `crush run` argv says about the store it wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStore {
    /// The session the run continued (`--session`), when it named one.
    pub session: Option<String>,
    /// `--continue`: the run appended to whichever session was most recent,
    /// which nothing it printed names.
    pub continued: bool,
    /// The store-selecting flags, as `flag value` pairs, to pass on unchanged.
    pub flags: Vec<String>,
}

/// Read the store-selecting flags off a `crush run` argv (`argv[0]` is the
/// binary). An argv that is not a `run` names nothing.
#[must_use]
pub fn run_store(argv: &[String]) -> RunStore {
    let mut store = RunStore::default();
    if argv.get(1).map(String::as_str) != Some("run") {
        return store;
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
            store.continued = true;
            continue;
        }
        if !VALUED_FLAGS.contains(&flag) {
            continue;
        }
        let Some(value) = inline.or_else(|| args.next().cloned()) else {
            break;
        };
        if matches!(flag, "-s" | "--session") {
            store.session = Some(value);
        } else if STORE_FLAGS.contains(&flag) {
            store.flags.push(flag.to_string());
            store.flags.push(value);
        }
    }
    store
}

/// `crush session list --json`, against the run's store.
#[must_use]
pub fn list_argv(bin: &str, store: &RunStore) -> Vec<String> {
    let mut argv = vec![
        bin.to_string(),
        "session".into(),
        "list".into(),
        "--json".into(),
    ];
    argv.extend(store.flags.iter().cloned());
    argv
}

/// `crush session show <id> --json`, against the run's store.
#[must_use]
pub fn show_argv(bin: &str, session: &str, store: &RunStore) -> Vec<String> {
    let mut argv = vec![
        bin.to_string(),
        "session".into(),
        "show".into(),
        session.to_string(),
        "--json".into(),
    ];
    argv.extend(store.flags.iter().cloned());
    argv
}

/// One row of `crush session list --json`.
#[derive(Debug, Deserialize)]
struct ListedSession {
    uuid: String,
    /// RFC 3339 to the second: crush stores `created_at` in whole seconds.
    created: String,
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
pub fn created_session(list_json: &str, since_ms: i64) -> Result<String, StoreMiss> {
    let listed: Vec<ListedSession> = serde_json::from_str(list_json.trim())
        .map_err(|err| StoreMiss::Unreadable(format!("`crush session list --json`: {err}")))?;
    let since_second = since_ms.div_euclid(1_000) * 1_000;
    let mut created = listed.into_iter().filter(|session| {
        epoch_millis_from_rfc3339(&session.created).is_some_and(|at| at >= since_second)
    });
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
    role: String,
    #[serde(default)]
    parts: Vec<ShownPart>,
}

#[derive(Debug, Deserialize)]
struct ShownPart {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
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
        .rposition(|message| message.role == "user")
        .map_or(0, |user| user + 1);
    Ok(shown.messages[turn_start..]
        .iter()
        .filter(|message| message.role == "assistant")
        .map(|message| {
            let text: String = message
                .parts
                .iter()
                .filter(|part| part.kind == "text")
                .map(|part| part.text.as_str())
                .collect();
            let text = text.strip_prefix('\n').unwrap_or(&text);
            text.trim_start_matches([' ', '\t']).to_string()
        })
        .collect())
}

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
        ]));
        assert_eq!(store.session.as_deref(), Some("abc"));
        assert!(!store.continued);
        assert_eq!(store.flags, argv(&["-D", "/data", "--cwd", "/work"]));
        assert_eq!(
            show_argv("crush", "abc", &store),
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
            RunStore::default()
        );
        let store = run_store(&argv(&["crush", "run", "-C", "--", "-s", "x"]));
        assert!(store.continued);
        assert_eq!(store.session, None);
        // A valued flag with nothing after it ends the parse rather than panicking.
        assert_eq!(run_store(&argv(&["crush", "run", "-s"])).session, None);
    }

    #[test]
    fn created_session_is_the_one_session_created_since_the_run_began() {
        let list = r#"[
            {"id":"h1","uuid":"new","title":"t","created":"2026-11-22T16:26:49-06:00","modified":"x"},
            {"id":"h2","uuid":"old","title":"t","created":"2026-11-22T16:26:48-06:00","modified":"x"}
        ]"#;
        // 2026-11-22T22:26:49.500Z: the run began half a second into the new
        // session's creation second, which crush records without its fraction.
        let since = epoch_millis_from_rfc3339("2026-11-22T22:26:49.500Z").unwrap();
        assert_eq!(created_session(list, since), Ok("new".to_string()));
        assert!(matches!(
            created_session(list, since - 1_000),
            Err(StoreMiss::Unidentified(why)) if why.contains("more than one")
        ));
        assert!(matches!(
            created_session(list, since + 1_000),
            Err(StoreMiss::Unidentified(why)) if why.contains("no session")
        ));
        assert!(matches!(
            created_session("not json", since),
            Err(StoreMiss::Unreadable(_))
        ));
    }

    #[test]
    fn turn_reply_joins_the_last_turns_assistant_text_as_crush_prints_it() {
        let show = r#"{"meta":{},"messages":[
            {"role":"user","parts":[{"type":"text","text":"first"}]},
            {"role":"assistant","parts":[{"type":"text","text":"earlier turn"}]},
            {"role":"user","parts":[{"type":"text","text":"second"}]},
            {"role":"assistant","parts":[{"type":"reasoning","thinking":"hm"},{"type":"text","text":"\n  Looking."},{"type":"tool_call","name":"ls"}]},
            {"role":"tool","parts":[{"type":"tool_result","content":"a"}]},
            {"role":"assistant","parts":[{"type":"text","text":" Done."},{"type":"finish","reason":"end_turn"}]}
        ]}"#;
        assert_eq!(turn_reply(show), Ok("Looking.Done.".to_string()));
        assert!(matches!(turn_reply("{}"), Err(StoreMiss::Unreadable(_))));
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
