//! `oneharness history` — view and manage the standardized run history that
//! `run --history` streams to disk. `list`/`show` print JSON to stdout by default
//! (the programmatic contract every other subcommand upholds) and offer an opt-in
//! `--format text` human view; `clear` deletes sessions (dry-run unless `--yes`).

use std::path::{Path, PathBuf};

use crate::cli::{
    HistoryClearArgs, HistoryCommand, HistoryFormat, HistoryListArgs, HistoryMigrateArgs,
    HistoryShowArgs, HistoryWatchArgs, HistoryWatchFormat,
};
use crate::commands::print_json;
use oneharness_core::domain::history::{self, HistoryId, HistoryRecord, HistoryStreamEnvelope};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::history as history_io;
use oneharness_core::io::history::SessionSummary;

/// Exit codes (clap uses 2 for argument errors).
const EXIT_OK: i32 = 0;
const EXIT_NOT_FOUND: i32 = 1;

pub fn run(args: &crate::cli::HistoryArgs) -> Result<i32, OneharnessError> {
    match &args.command {
        HistoryCommand::List(a) => list(a),
        HistoryCommand::Show(a) => show(a),
        HistoryCommand::Watch(a) => watch(a),
        HistoryCommand::Clear(a) => clear(a),
        HistoryCommand::Migrate(a) => migrate(a),
    }
}

fn migrate(args: &HistoryMigrateArgs) -> Result<i32, OneharnessError> {
    let dir = resolve_dir(
        args.history_dir.as_deref(),
        args.config.as_deref(),
        args.no_config,
    )?;
    let files = history_io::migrate(&dir)?;
    let report = serde_json::json!({
        "files": files,
        "files_processed": files.len(),
    });
    print_json(&report, args.compact)?;
    Ok(EXIT_OK)
}

fn watch(args: &HistoryWatchArgs) -> Result<i32, OneharnessError> {
    use std::time::Duration;

    let dir = resolve_dir(
        args.history_dir.as_deref(),
        args.config.as_deref(),
        args.no_config,
    )?;
    let after = args
        .after
        .as_deref()
        .map(str::parse::<HistoryId>)
        .transpose()
        .map_err(|_| OneharnessError::HistoryCursorInvalid {
            value: args.after.clone().unwrap_or_default(),
        })?;
    let labels = history::parse_labels(args.label.iter().map(String::as_str))
        .map_err(OneharnessError::HistoryLabelInvalid)?;
    let slug = project_slug(args.all_projects, args.project.as_deref());
    let mut watcher = history_io::HistoryWatcher::open(&dir, after, labels, slug, args.events)?;

    match args.format {
        HistoryWatchFormat::Jsonl => loop {
            if args.events && !write_watch_events(&watcher.drain_events())? {
                return Ok(EXIT_OK);
            }
            let records: Vec<_> = watcher
                .drain_available()
                .into_iter()
                .filter(|record| {
                    args.variant
                        .as_ref()
                        .is_none_or(|variant| record.variant.as_deref() == Some(variant.as_str()))
                })
                .collect();
            if !write_watch_records(&records)? {
                return Ok(EXIT_OK);
            }
            std::thread::sleep(Duration::from_millis(100));
            let records: Vec<_> = watcher
                .poll()?
                .into_iter()
                .filter(|record| {
                    args.variant
                        .as_ref()
                        .is_none_or(|variant| record.variant.as_deref() == Some(variant.as_str()))
                })
                .collect();
            if args.events && !write_watch_events(&watcher.drain_events())? {
                return Ok(EXIT_OK);
            }
            for record in records {
                if !write_watch_records(&[record])? {
                    return Ok(EXIT_OK);
                }
            }
        },
    }
}

fn write_watch_events(
    events: &[oneharness_core::domain::history::HistoryEventLine],
) -> Result<bool, OneharnessError> {
    write_watch_envelopes(
        events
            .iter()
            .cloned()
            .map(|line| HistoryStreamEnvelope::Event { line }),
    )
}

fn write_watch_records(records: &[HistoryRecord]) -> Result<bool, OneharnessError> {
    write_watch_envelopes(
        records
            .iter()
            .cloned()
            .map(|record| HistoryStreamEnvelope::Record { record }),
    )
}

fn write_watch_envelopes(
    envelopes: impl IntoIterator<Item = HistoryStreamEnvelope>,
) -> Result<bool, OneharnessError> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for envelope in envelopes {
        let line = serde_json::to_string(&envelope)?;
        if let Err(error) = writeln!(out, "{line}") {
            return if error.kind() == std::io::ErrorKind::BrokenPipe {
                Ok(false)
            } else {
                Err(OneharnessError::HistoryIo {
                    path: "stdout".to_string(),
                    source: error,
                })
            };
        }
    }
    if let Err(error) = out.flush() {
        return if error.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(false)
        } else {
            Err(OneharnessError::HistoryIo {
                path: "stdout".to_string(),
                source: error,
            })
        };
    }
    Ok(true)
}

/// Resolve the effective history directory for a view/manage command: the
/// explicit `--history-dir`, else config `history_dir` (layered like every other
/// field), else the platform default. Errors loudly when none can be resolved so
/// a consumer never silently reads the wrong (or no) store.
fn resolve_dir(
    history_dir: Option<&Path>,
    config: Option<&Path>,
    no_config: bool,
) -> Result<PathBuf, OneharnessError> {
    let configured = match history_dir {
        Some(p) => Some(p.display().to_string()),
        None => {
            // Discover config from the current directory, mirroring `config`.
            let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let loaded = config_io::load(config, no_config, &start)?;
            loaded.config.history_dir.clone()
        }
    };
    history_io::resolve_dir(configured.as_deref()).ok_or(OneharnessError::HistoryNoDir)
}

/// The project slug filter for a scoped command: `None` when `--all-projects`,
/// else the slug of `--project` (or the current directory).
fn project_slug(all_projects: bool, project: Option<&Path>) -> Option<String> {
    if all_projects {
        return None;
    }
    let dir = match project {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let canonical = std::fs::canonicalize(&dir).unwrap_or(dir);
    Some(history::project_slug(&canonical.display().to_string()))
}

fn list(args: &HistoryListArgs) -> Result<i32, OneharnessError> {
    let dir = resolve_dir(
        args.history_dir.as_deref(),
        args.config.as_deref(),
        args.no_config,
    )?;
    let slug = project_slug(args.all_projects, args.project.as_deref());
    let mut sessions = history_io::list_sessions(&dir, slug.as_deref())?;
    if let Some(variant) = &args.variant {
        let suffix = format!(":{variant}");
        sessions.retain(|session| {
            session
                .harnesses
                .iter()
                .any(|harness| harness.ends_with(&suffix))
        });
    }
    match args.format {
        HistoryFormat::Json => print_json(&sessions, args.compact)?,
        HistoryFormat::Text => print!("{}", render_list_text(&sessions)),
    }
    Ok(EXIT_OK)
}

fn show(args: &HistoryShowArgs) -> Result<i32, OneharnessError> {
    let dir = resolve_dir(
        args.history_dir.as_deref(),
        args.config.as_deref(),
        args.no_config,
    )?;
    // A UUID is an exact record lookup, independent of session names and project
    // scoping. Preserve the existing id-or-name session lookup for every other
    // spelling.
    if !args.last {
        let needle = args.session.as_deref().unwrap_or_default();
        if let Ok(id) = needle.parse::<HistoryId>() {
            match history_io::find_record_by_id(&dir, id) {
                Ok(record) => {
                    return render_records(args.format, args.compact, &[record]);
                }
                Err(OneharnessError::HistoryNotFound { .. }) => {
                    eprintln!("oneharness: history record `{id}` was not found");
                    return Ok(EXIT_NOT_FOUND);
                }
                Err(error) => return Err(error),
            }
        }
    }

    let slug = project_slug(args.all_projects, args.project.as_deref());
    let sessions = history_io::list_sessions(&dir, slug.as_deref())?;

    // Which session file(s) to read: --last is the newest in scope; otherwise
    // resolve the id-or-name needle (newest match, or every match with --all).
    let chosen: Vec<&SessionSummary> = if args.last {
        sessions.first().into_iter().collect()
    } else {
        // `session` is required by clap unless --last, so it is present here.
        let needle = args.session.as_deref().unwrap_or_default();
        let matched = history_io::match_sessions(&sessions, needle);
        if args.all {
            matched
        } else {
            matched.into_iter().take(1).collect()
        }
    };

    if chosen.is_empty() && !args.last {
        let needle = args.session.as_deref().unwrap_or_default();
        if let Some(path) = history_io::find_session_path(&dir, slug.as_deref(), needle)? {
            return render_record_values(
                args.format,
                args.compact,
                &history_io::read_session_display(&path)?,
            );
        }
    }
    if chosen.is_empty() {
        let scope = if args.last { "any" } else { "matching" };
        eprintln!(
            "oneharness: no {scope} history session found under `{}`",
            dir.display()
        );
        return Ok(EXIT_NOT_FOUND);
    }

    // Read the chosen sessions' records (newest first, already ordered).
    let mut records = Vec::new();
    for s in &chosen {
        records.extend(history_io::read_session_display(Path::new(&s.path))?);
    }
    render_record_values(args.format, args.compact, &records)
}

fn render_records(
    format: HistoryFormat,
    compact: bool,
    records: &[HistoryRecord],
) -> Result<i32, OneharnessError> {
    match format {
        HistoryFormat::Json => print_json(&records.to_vec(), compact)?,
        HistoryFormat::Text => {
            let values = records
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            print!("{}", render_show_text(&values));
        }
    }
    Ok(EXIT_OK)
}

fn render_record_values(
    format: HistoryFormat,
    compact: bool,
    records: &[serde_json::Value],
) -> Result<i32, OneharnessError> {
    match format {
        HistoryFormat::Json => print_json(&records, compact)?,
        HistoryFormat::Text => print!("{}", render_show_text(records)),
    }
    Ok(EXIT_OK)
}

fn clear(args: &HistoryClearArgs) -> Result<i32, OneharnessError> {
    let dir = resolve_dir(
        args.history_dir.as_deref(),
        args.config.as_deref(),
        args.no_config,
    )?;
    let slug = project_slug(args.all_projects, args.project.as_deref());

    if args.yes {
        let removed = history_io::remove_sessions(&dir, slug.as_deref())?;
        let report = serde_json::json!({
            "removed": removed.len(),
            "files": removed,
            "dry_run": false,
        });
        print_json(&report, args.compact)?;
    } else {
        // Dry run: report what *would* be removed, delete nothing.
        let sessions = history_io::list_sessions(&dir, slug.as_deref())?;
        let files: Vec<String> = sessions.iter().map(|s| s.path.clone()).collect();
        let report = serde_json::json!({
            "would_remove": files.len(),
            "files": files,
            "dry_run": true,
            "hint": "re-run with --yes to delete",
        });
        print_json(&report, args.compact)?;
    }
    Ok(EXIT_OK)
}

/// A compact human table for `history list --format text`.
fn render_list_text(sessions: &[SessionSummary]) -> String {
    if sessions.is_empty() {
        return "no history sessions\n".to_string();
    }
    let mut out = String::new();
    for s in sessions {
        out.push_str(&format!(
            "{started}  {name}  ({records} run{plural}, {harnesses})\n  id: {id}\n  project: {project}\n",
            started = if s.started.is_empty() { "?" } else { &s.started },
            name = s.name,
            records = s.record_count,
            plural = if s.record_count == 1 { "" } else { "s" },
            harnesses = if s.harnesses.is_empty() {
                "-".to_string()
            } else {
                s.harnesses.join(", ")
            },
            id = s.id,
            project = s.project,
        ));
    }
    out
}

/// A readable dump for `history show --format text`: one block per record.
fn render_show_text(records: &[serde_json::Value]) -> String {
    if records.is_empty() {
        return "no records\n".to_string();
    }
    let mut out = String::new();
    for r in records {
        let get = |key: &str| r.get(key).and_then(|value| value.as_str()).unwrap_or("");
        let status = get("status");
        out.push_str(&format!(
            "{ts}  [{harness}] {status}\n",
            ts = get("timestamp"),
            harness = get("harness"),
        ));
        let prompt = get("prompt");
        if !prompt.is_empty() {
            out.push_str(&format!("  prompt: {}\n", first_line(prompt)));
        }
        if let Some(text) = r.get("text").and_then(|value| value.as_str()) {
            out.push_str(&format!("  text: {}\n", first_line(text)));
        }
        out.push('\n');
    }
    out
}

/// The first line of a string, so a multi-line prompt/answer stays one row.
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, name: &str, started: &str, harnesses: &[&str]) -> SessionSummary {
        SessionSummary {
            id: id.to_string(),
            name: name.to_string(),
            labels: Default::default(),
            project: "/p".to_string(),
            started: started.to_string(),
            record_count: harnesses.len(),
            harnesses: harnesses.iter().map(|s| s.to_string()).collect(),
            path: format!("/h/{id}.jsonl"),
        }
    }

    #[test]
    fn project_slug_filter_honors_all_projects() {
        assert_eq!(project_slug(true, None), None);
        assert_eq!(
            project_slug(false, Some(Path::new("/home/user/proj"))),
            Some("home-user-proj".to_string())
        );
    }

    #[test]
    fn list_text_renders_rows_or_empty() {
        assert_eq!(render_list_text(&[]), "no history sessions\n");
        let text = render_list_text(&[summary(
            "fix-bug-20260101T000000Z-1",
            "fix-bug",
            "2026-01-01T00:00:00Z",
            &["claude-code", "codex"],
        )]);
        assert!(text.contains("fix-bug"));
        assert!(text.contains("2 runs"));
        assert!(text.contains("claude-code, codex"));
        assert!(text.contains("id: fix-bug-20260101T000000Z-1"));
    }

    #[test]
    fn show_text_renders_first_lines() {
        let records = vec![serde_json::json!({
            "timestamp": "2026-01-01T00:00:00Z",
            "harness": "codex",
            "status": "ok",
            "prompt": "line one\nline two",
            "text": "answer\nmore",
        })];
        let text = render_show_text(&records);
        assert!(text.contains("[codex] ok"));
        assert!(text.contains("prompt: line one"));
        assert!(!text.contains("line two"));
        assert!(text.contains("text: answer"));
    }

    #[test]
    fn show_text_empty_is_labeled() {
        assert_eq!(render_show_text(&[]), "no records\n");
    }
}
