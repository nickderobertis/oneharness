//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use oneharness_core::domain::batch::{self, BatchStrategy};
use oneharness_core::domain::harness::{self, BuildCtx, HarnessSpec};
use oneharness_core::domain::mock::{self, MockDelivery};
use oneharness_core::domain::mode::{ModeHeadless, PermissionMode};
use oneharness_core::domain::report::{
    BatchReport, Capture, OutputFormat, RunReport, RunResult, SessionReport, Status, SCHEMA_VERSION,
};
use oneharness_core::domain::session::{self, SessionPlan, SessionRecord};
use oneharness_core::domain::signals::Usage;
use oneharness_core::domain::structured::{self, Schema};
use oneharness_core::domain::{events, normalize, signals};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::detect::{self, BinOverrides};
use oneharness_core::io::history::{self, HistoryWriter};
use oneharness_core::io::hooks::{self as hooks_io, HookSnapshot, Scope};
use oneharness_core::io::runner::{self, Job, Outcome};
use oneharness_core::io::session as session_io;
use std::path::PathBuf;

/// Exit codes (clap uses 2 for argument errors).
const EXIT_OK: i32 = 0;
const EXIT_FAILURE: i32 = 1;

pub fn run(args: &RunArgs) -> Result<i32, OneharnessError> {
    // Project config is discovered from where the harnesses will run (--cwd,
    // else the current directory): the project being operated on is the one
    // whose config should apply.
    let project_start = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_start)?;
    let cfg = &loaded.config;

    // stdin can be consumed only once total, so `--prompt-file -` and
    // `--system-file -` cannot both read it (repeated `--prompt-file -` is caught
    // in resolve_prompts). Validated before any read so it never blocks on stdin.
    if args.system_file.as_deref() == Some("-") && args.prompt_file.iter().any(|p| p == "-") {
        return Err(OneharnessError::MultipleStdinConsumers { count: 2 });
    }

    let prompts = resolve_prompts(args)?;
    // A batch run is "one harness over N prompts that share a cacheable prefix"
    // (the same --system/model). It is signalled simply by more than one prompt;
    // a single prompt keeps the ordinary "one prompt across the selected
    // harnesses" behavior.
    let batch_run = prompts.len() > 1;
    let batch_strategy = args.batch_strategy.unwrap_or(BatchStrategy::Speed);
    // Structured output: an optional compiled schema and the per-harness retry
    // budget. Compiling here (once) turns an invalid schema into a loud usage
    // error before any harness is spawned.
    let schema = load_schema(args, cfg, &project_start)?;
    let max_retries = args
        .schema_max_retries
        .or(cfg.schema_max_retries)
        .unwrap_or(2);
    // A CLI selection (--all / --harness) replaces the config selection
    // entirely; config `exclude` still applies unless --exclude is given.
    let (all, include) = if args.all || !args.harness.is_empty() {
        (args.all, args.harness.clone())
    } else {
        (
            cfg.all.unwrap_or(false),
            cfg.harnesses.clone().unwrap_or_default(),
        )
    };
    let exclude = if args.exclude.is_empty() {
        cfg.exclude.clone().unwrap_or_default()
    } else {
        args.exclude.clone()
    };
    let specs = select_specs(all, &include, &exclude)?;
    // A batch (multi-prompt) run is single-harness by nature — a provider cache
    // prefix is per harness/model/tools — and is a fresh fan-out, not a session
    // continuation. Refuse both before anything spawns (loud usage errors).
    if batch_run {
        validate_batch(&specs, args.resume.is_some() || args.fork)?;
    }
    let resume = args.resume.as_deref();
    validate_resume(resume, &specs)?;
    // `--fork` (clap-guaranteed to imply `--resume`) branches a new session
    // instead of appending; refused before spawning for a harness that can't fork.
    validate_fork(args.fork, &specs)?;
    // `--session <name>`: resolve the uniform handle to the harness's native
    // token (via the session store) before building argv. Validates single-harness
    // + capability + no-batch loudly; on a continue it yields the token to resume
    // with, reusing the harness's verified `--resume` mapping. `None` when the flag
    // was not passed.
    let session_wiring = setup_session(args, &specs, batch_run, &project_start)?;
    let session_resume: Option<String> = session_wiring
        .as_ref()
        .and_then(|w| w.plan.resume_token.clone());
    // `--stream` emits events incrementally for a *single* harness/prompt; the
    // validate/retry loop and the batch fan-out both need the whole output at
    // once, so they are mutually exclusive. Refused loudly before spawning.
    validate_stream(args.stream, &specs, batch_run, schema.is_some())?;
    // Resolve the approval mode (CLI --mode > --bypass/--no-bypass > config
    // `mode` > config `bypass` > the built-in default, which is `default`). A
    // mode a selected harness *cannot express* is refused here (a command can't
    // be built); a mode that *might block on a prompt* is warned about but still
    // run, with the per-harness `--timeout` as the backstop (a hang becomes a
    // `timeout` result, never an infinite stall).
    let mode = resolve_mode(args, cfg);
    validate_modes(mode, &specs)?;
    if !args.permit_prompts {
        for id in hang_prone(mode, &specs) {
            eprintln!(
                "oneharness: warning: `--mode {}` may block on an interactive approval prompt for \
                 harness `{id}` headlessly; relying on --timeout. Sync allow-rules (and pass \
                 --permit-prompts to silence this), or use --mode bypass / read-only.",
                mode.as_str()
            );
        }
    }
    let config_bins: std::collections::HashMap<String, String> = cfg
        .harness
        .iter()
        .filter_map(|(id, h)| h.bin.clone().map(|bin| (id.clone(), bin)))
        .collect();
    let overrides = BinOverrides::parse(&args.bin)?.with_config_bins(config_bins);
    // One-shot mock/spy wiring (`--mock-rules` / `--spy-file`): validate the
    // ruleset and every selected harness's capability loudly, then deliver the
    // hook ephemerally — on the argv where the harness supports it, else via a
    // snapshotted project-scope install restored after the run.
    let mock_wiring = setup_mock(args, &specs, &project_start, &overrides)?;
    let cli_env = parse_env(&args.env)?;
    let model = args.model.as_deref().or(cfg.model.as_deref());
    // The effective system prompt comes from `--system` xor `--system-file` (the
    // argv-limit escape hatch, mirroring `--prompt-file`), then config `system`.
    let system_text: Option<String> = resolve_system(args)?.or_else(|| cfg.system.clone());
    let system = system_text.as_deref();
    let timeout = args.timeout.or(cfg.timeout).unwrap_or(120);
    let require_available = args.require_available || cfg.require_available.unwrap_or(false);

    // History (opt-in): --history/--no-history beats config `history`; the
    // directory is --history-dir, else config `history_dir`, else the platform
    // default. Never recorded under --print-command (nothing runs). Best-effort —
    // a store that cannot be opened warns and disables history for the run, so a
    // history problem never takes the results down (see "never panic on a
    // harness's behavior"). The absolute path is echoed in the report as the
    // programmatic handle a consumer reads the session back with.
    let history_writer = open_history_writer(args, cfg, &project_start, &prompts);
    let history_file = history_writer.as_ref().map(|w| {
        std::path::absolute(w.path())
            .unwrap_or_else(|_| w.path().to_path_buf())
            .display()
            .to_string()
    });

    // The (harness, prompt) units to run. An ordinary run is each selected
    // harness against the one prompt; a batch run is the single selected harness
    // against each prompt (so `results` carries one entry per prompt, in order).
    let units: Vec<(&'static HarnessSpec, &str)> = if batch_run {
        prompts.iter().map(|p| (specs[0], p.as_str())).collect()
    } else {
        specs.iter().map(|s| (*s, prompts[0].as_str())).collect()
    };

    // Build a plan entry for every unit; queue jobs only for the ones that are
    // available and actually being executed. `job_plans` parallels `jobs` and
    // retains what the structured-output retry loop needs to rebuild a unit's
    // argv with a feedback prompt.
    let mut plan: Vec<Plan> = Vec::with_capacity(units.len());
    let mut jobs: Vec<Job> = Vec::new();
    let mut job_plans: Vec<HarnessPlan> = Vec::new();

    for (spec, unit_prompt) in &units {
        let spec = *spec;
        // On a batch run each result records the prompt it ran (they differ);
        // on an ordinary run the single top-level `prompt` covers them all.
        let result_prompt = batch_run.then(|| unit_prompt.to_string());
        let resolved = detect::resolve(spec, &overrides);
        // Explicit format (CLI or config) always wins. Otherwise, when the caller
        // asked for events/streaming, upgrade to the harness's events-capable
        // format so a tool transcript is actually emitted (e.g. Claude Code's
        // default `json` → `stream-json`); harnesses whose default already
        // carries a transcript declare `events_format: None` and stay put.
        let explicit_format = args.output_format.or(cfg.output_format);
        let want_events = args.events || args.stream;
        let chosen_format = explicit_format.unwrap_or_else(|| {
            if want_events {
                spec.events_format.unwrap_or(spec.output_format)
            } else {
                spec.output_format
            }
        });
        // A native-schema harness must receive its schema as JSON; force the
        // format so the conforming value lands where we read it (Claude Code's
        // `structured_output`, which needs `--output-format json`).
        let native = schema.is_some() && spec.native_schema.is_some();
        let output_format = if native {
            OutputFormat::Json
        } else {
            chosen_format
        };
        let mut extra = cfg.args_for(spec.id).to_vec();
        extra.extend(args.passthrough.iter().cloned());
        if let Some(wiring) = &mock_wiring {
            extra.extend(wiring.extra_args_for(spec.id));
        }
        let harness_plan = HarnessPlan {
            spec,
            bin: resolved.bin.clone(),
            // A CLI --model beats config; within config, the harness's own
            // [harness.<id>] model beats the top-level one.
            model: args
                .model
                .as_deref()
                .or_else(|| cfg.model_for(spec.id))
                .map(str::to_string),
            system: system.map(str::to_string),
            // A `--session` continue supplies the native token to resume with,
            // reusing the harness's verified `--resume` mapping; a create (or no
            // session) leaves it to the explicit `--resume` value (they are
            // mutually exclusive, so at most one is `Some`).
            resume: session_resume
                .clone()
                .or_else(|| resume.map(str::to_string)),
            fork: args.fork,
            mode,
            output_format,
            native,
            base_prompt: unit_prompt.to_string(),
            extra,
        };
        let command = harness_plan.argv(schema.as_ref(), None);

        if args.print_command {
            plan.push(Plan::Ready(Box::new(planned_result(
                spec,
                &resolved.bin,
                resolved.available,
                command,
                output_format,
                result_prompt,
            ))));
        } else if !resolved.available {
            plan.push(Plan::Ready(Box::new(skipped_result(
                spec,
                &resolved.bin,
                command,
                output_format,
                result_prompt,
            ))));
        } else {
            let job_index = jobs.len();
            // Env layers, applied in order (the runner is last-write-wins):
            // the harness's declared defaults, then any env that delivers the
            // approval mode (Goose's GOOSE_MODE), then config ([env], then
            // [harness.<id>.env]), then the explicit `--env`, which always wins.
            let mut job_env: Vec<(String, String)> = spec
                .default_env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            if let Some(ms) = spec.mode(mode) {
                job_env.extend(ms.env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            }
            job_env.extend(cfg.env_for(spec.id));
            job_env.extend(cli_env.iter().cloned());
            jobs.push(Job {
                argv: command.clone(),
                cwd: args.cwd.clone(),
                env: job_env,
                timeout: Duration::from_secs(timeout),
            });
            plan.push(Plan::Pending {
                spec,
                bin: resolved.bin,
                output_format,
                job_index,
                prompt: result_prompt,
            });
            job_plans.push(harness_plan);
        }
    }

    // Streaming path: a single harness, emitting normalized events to stdout as
    // they arrive, then a final report line — so a consumer can short-circuit the
    // moment it sees a disallowed action. `--print-command` still just prints the
    // planned command (nothing executes), so it falls through to the normal path.
    if args.stream && !args.print_command {
        let result = match plan.into_iter().next().expect("stream: one unit") {
            // The harness was unavailable/skipped — nothing to stream; emit only
            // the terminal report line so the shape is still complete.
            Plan::Ready(result) => *result,
            Plan::Pending {
                spec,
                bin,
                output_format,
                job_index,
                prompt,
            } => stream_one_harness(&jobs[job_index], spec, &bin, output_format, prompt),
        };
        record_history(
            &history_writer,
            mode,
            model,
            &prompts[0],
            std::slice::from_ref(&result),
        );
        // The run is over: put the workspace back before anything else can fail.
        let mock_report = mock_wiring.map(MockWiring::finish);
        // Persist the captured session token (if `--session` was in play) and
        // build its report block, before `result` is moved into the report.
        let session_report = finalize_session(
            session_wiring,
            std::slice::from_ref(&result),
            args.print_command,
        );
        if let Some(dir) = &args.output_dir {
            write_output_dir(dir, std::slice::from_ref(&result))?;
        }
        let exit = exit_code(std::slice::from_ref(&result), require_available);
        let report = build_report(
            vec![result],
            &prompts,
            model,
            args,
            mode,
            schema.as_ref(),
            max_retries,
            None,
            loaded.files.clone(),
            mock_report,
            history_file,
            session_report,
        );
        // The event lines were already written during the run; the report is the
        // terminal `{"type":"result", ...}` line of the same NDJSON stream.
        emit_stream_result(&report)?;
        return Ok(exit);
    }

    // Schedule and run the jobs. An ordinary run (and a batch under `speed`) is a
    // single concurrent wave; a batch under `min-tokens` runs a one-call warm-up
    // before the fanned-out rest, with a barrier between them.
    let max_parallel = args
        .max_parallel
        .or(cfg.max_parallel)
        .unwrap_or(jobs.len().max(1));
    // Fork-based `min-tokens`: when a batch's single harness can fork, the warm-up
    // (prompt[0]) establishes a session and the fan-out branches forks of it, so
    // each reuses the warmed cached prefix — the realizable token saving on these
    // CLIs (a static --system is re-created per process, so plain warm-then-fan
    // saves nothing). It needs the warm-up's *runtime* session id, so it cannot
    // run under --print-command (nothing executes, no session).
    let fork_batch = batch_run
        && batch_strategy == BatchStrategy::MinTokens
        && specs[0].fork_reuses_cache
        && !args.print_command
        && !jobs.is_empty();
    // `min-tokens` reduces tokens only when the harness has a *cache-reusing* fork
    // (the warm-up writes the shared prefix, the forked fan-out reads it). When it
    // does not — no fork at all, or a fork that re-sends the prefix cold, like
    // OpenCode — `min-tokens` can only order the calls; say so rather than imply a
    // saving the harness can't deliver.
    if batch_run
        && batch_strategy == BatchStrategy::MinTokens
        && !specs[0].fork_reuses_cache
        && !args.print_command
    {
        eprintln!(
            "oneharness: warning: `--batch-strategy min-tokens` cannot reduce tokens on `{}` \
             (no cache-reusing fork available); it only orders the calls. Token savings need a \
             harness whose fork reuses the prompt cache (see `fork_reuses_cache` in \
             `oneharness list`).",
            specs[0].id
        );
    }
    let mut forked = false;
    let outcomes = if fork_batch {
        let o = run_fork_batch(
            &mut jobs,
            &mut job_plans,
            schema.as_ref(),
            max_retries,
            max_parallel,
        );
        // The fan-out actually forked iff the warm-up exposed a session to branch
        // (run_fork_batch sets the fan-out plans' `resume` only when it did).
        forked = job_plans.len() > 1 && job_plans[1].resume.is_some();
        o
    } else {
        let waves = if batch_run {
            batch::waves(batch_strategy, jobs.len())
        } else if jobs.is_empty() {
            Vec::new()
        } else {
            vec![(0..jobs.len()).collect()]
        };
        run_in_waves(
            &jobs,
            &job_plans,
            schema.as_ref(),
            max_retries,
            max_parallel,
            &waves,
        )
    };

    // Every job is done (or nothing ran under --print-command, where mock
    // wiring is refused by clap): put the workspace back before anything else
    // can fail, so a later I/O error never leaves the ephemeral hook behind.
    let mock_report = mock_wiring.map(MockWiring::finish);

    let results: Vec<RunResult> = plan
        .into_iter()
        .map(|entry| match entry {
            Plan::Ready(result) => *result,
            Plan::Pending {
                spec,
                bin,
                output_format,
                job_index,
                prompt,
            } => {
                let outcome = &outcomes[job_index];
                // The argv actually run (fork-batch rewrites the fan-out jobs to
                // resume+fork the warmed session, so read it back from the job).
                let command = jobs[job_index].argv.clone();
                executed_result(
                    spec,
                    bin,
                    command,
                    output_format,
                    &outcome.capture,
                    schema.as_ref(),
                    outcome.attempts,
                    prompt,
                )
            }
        })
        .collect();

    record_history(&history_writer, mode, model, &prompts[0], &results);
    // Persist the captured session token (if `--session` was in play) and build
    // its report block. A session run is single-harness, so `results` holds one.
    let session_report = finalize_session(session_wiring, &results, args.print_command);

    if let Some(dir) = &args.output_dir {
        write_output_dir(dir, &results)?;
    }

    let exit = exit_code(&results, require_available);

    let report = build_report(
        results,
        &prompts,
        model,
        args,
        mode,
        schema.as_ref(),
        max_retries,
        batch_run.then_some(BatchReport {
            strategy: batch_strategy,
            prompt_count: prompts.len(),
            forked,
        }),
        loaded.files,
        mock_report,
        history_file,
        session_report,
    );
    print_json(&report, args.compact)?;

    if exit != EXIT_OK {
        let failed = report
            .results
            .iter()
            .filter(|r| is_failure(r.status, r.available, require_available, r.schema_valid))
            .count();
        eprintln!(
            "oneharness: {failed}/{} harness run(s) did not succeed (see results[].status and results[].error)",
            report.results.len()
        );
    }
    Ok(exit)
}

/// Assemble the top-level [`RunReport`] from the finished results and the shared
/// run metadata. Extracted so the normal and streaming paths emit an identical
/// envelope shape (the streaming path passes `batch: None`).
#[allow(clippy::too_many_arguments)]
/// What `--mock-rules`/`--spy-file` wired up before spawning, and how to undo
/// it. Built by [`setup_mock`]; consumed by [`MockWiring::finish`] the moment
/// every job is done — the restore must run on the same code path as the run
/// itself, so a later failure (report I/O) can never leave the ephemeral hook
/// installed in the workspace.
struct MockWiring {
    /// Per-harness argv additions: Claude Code's `--settings <tempfile>`,
    /// Codex's hook-engine opt-in flags.
    extra_args: std::collections::HashMap<&'static str, Vec<String>>,
    /// Byte snapshots of every project config file the installs touched
    /// (existing files restored verbatim, created ones deleted).
    snapshot: HookSnapshot,
    /// Temp settings files to delete afterwards (the `SettingsFlag` delivery).
    temp_files: Vec<std::path::PathBuf>,
    /// What the report records.
    rules: Option<serde_json::Value>,
    spy_file: Option<String>,
}

/// The report-facing remainder of a finished [`MockWiring`].
struct MockReport {
    rules: Option<serde_json::Value>,
    spy_file: Option<String>,
}

impl MockWiring {
    fn extra_args_for(&self, id: &str) -> Vec<String> {
        self.extra_args.get(id).cloned().unwrap_or_default()
    }

    /// Undo everything: restore the snapshotted config files and delete the
    /// temp settings files. Best-effort with a stderr warning per failure — a
    /// restore problem must never take the run's results down with it.
    fn finish(self) -> MockReport {
        for (path, err) in self.snapshot.restore() {
            eprintln!(
                "oneharness: warning: could not restore `{}` after the mocked run: {err}",
                path.display()
            );
        }
        for path in &self.temp_files {
            if let Err(err) = std::fs::remove_file(path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "oneharness: warning: could not remove temp settings `{}`: {err}",
                        path.display()
                    );
                }
            }
        }
        MockReport {
            rules: self.rules,
            spy_file: self.spy_file,
        }
    }
}

/// Validate and deliver the one-shot mock hook for every selected harness.
/// `None` when neither `--mock-rules` nor `--spy-file` was given. Everything
/// refusable is refused here, before any file is touched or process spawned:
/// a harness with no one-shot delivery (qwen, copilot), a rule action a
/// harness cannot express, an unreadable/invalid ruleset, or a path that
/// cannot be embedded into a hook command. Only then are the hooks delivered —
/// on the argv where possible, else installed at project scope in the working
/// directory (layering onto existing config via the non-destructive merge)
/// with every touched file snapshotted for [`MockWiring::finish`].
fn setup_mock(
    args: &RunArgs,
    specs: &[&'static HarnessSpec],
    project_dir: &std::path::Path,
    overrides: &BinOverrides,
) -> Result<Option<MockWiring>, OneharnessError> {
    if args.mock_rules.is_none() && args.spy_file.is_none() {
        return Ok(None);
    }

    // Parse + validate the ruleset (loud), and keep the raw value for the report.
    let mut rules_value = None;
    let mut parsed_rules = None;
    if let Some(path) = &args.mock_rules {
        let text =
            std::fs::read_to_string(path).map_err(|source| OneharnessError::MockRulesFile {
                path: path.display().to_string(),
                source,
            })?;
        let rules =
            mock::parse_rules(&text).map_err(|message| OneharnessError::MockRulesInvalid {
                path: path.display().to_string(),
                message,
            })?;
        rules_value = serde_json::from_str(&text).ok();
        parsed_rules = Some(rules);
    }

    // Every selected harness must be able to take the hook AND express every
    // rule action — refused before anything is delivered anywhere.
    for spec in specs {
        if spec.mock_delivery.is_none() {
            return Err(OneharnessError::MockDeliveryUnsupported {
                id: spec.id.to_string(),
                reason: match spec.id {
                    "qwen" => {
                        "its hooks fire only at user scope headlessly (project hooks sit behind \
                         folder trust) — sync a [[hooks]] mock with --global into a redirected \
                         HOME instead (see the README)"
                    }
                    _ => {
                        "its hooks never fire in a headless run (probe-refuted), so no delivery \
                          could make the CLI honor them"
                    }
                },
            });
        }
        if let Some(rules) = &parsed_rules {
            if let Some(action) = mock::unsupported_action(rules, spec.gate_deny, spec.mock_rewrite)
            {
                return Err(OneharnessError::MockActionUnsupported {
                    id: spec.id.to_string(),
                    action,
                });
            }
        }
    }

    // The hook command embeds this binary and the (absolutized) paths — the
    // hook runs from the harness's own cwd, and some harnesses tokenize the
    // command on whitespace, so space-bearing paths are refused loudly.
    let exe = std::env::current_exe().map_err(|err| OneharnessError::MockSetup {
        message: format!("could not resolve the oneharness binary path: {err}"),
    })?;
    let rules_abs = args
        .mock_rules
        .as_deref()
        .map(std::path::absolute)
        .transpose()
        .map_err(|err| OneharnessError::MockSetup {
            message: format!("could not absolutize --mock-rules: {err}"),
        })?;
    let spy_abs = args
        .spy_file
        .as_deref()
        .map(std::path::absolute)
        .transpose()
        .map_err(|err| OneharnessError::MockSetup {
            message: format!("could not absolutize --spy-file: {err}"),
        })?;
    let embed = |p: &std::path::Path| -> Result<String, OneharnessError> {
        // Forward slashes work everywhere Windows paths are consumed here, and
        // keep the string safe for JSON/TOML/shim embedding.
        let text = p.display().to_string().replace('\\', "/");
        if text.chars().any(char::is_whitespace) {
            return Err(OneharnessError::MockPathWhitespace { path: text });
        }
        Ok(text)
    };
    let exe = embed(&exe)?;
    let rules_str = rules_abs.as_deref().map(&embed).transpose()?;
    let spy_str = spy_abs.as_deref().map(&embed).transpose()?;

    let mut wiring = MockWiring {
        extra_args: std::collections::HashMap::new(),
        snapshot: HookSnapshot::default(),
        temp_files: Vec::new(),
        rules: rules_value,
        spy_file: spy_str.clone(),
    };

    for spec in specs {
        // A missing binary is a skipped result; do not touch the workspace for it.
        if !detect::resolve(spec, overrides).available {
            continue;
        }
        let command = mock::hook_command(&exe, spec.id, rules_str.as_deref(), spy_str.as_deref());
        match spec.mock_delivery.expect("validated above") {
            MockDelivery::SettingsFlag { flag } => {
                let path = std::env::temp_dir().join(format!(
                    "oneharness-mock-{}-{}.json",
                    spec.id,
                    std::process::id()
                ));
                std::fs::write(&path, mock::settings_hooks_json(&command)).map_err(|err| {
                    OneharnessError::MockSetup {
                        message: format!(
                            "could not write temp settings `{}`: {err}",
                            path.display()
                        ),
                    }
                })?;
                let path_str = embed(&path)?;
                wiring.temp_files.push(path);
                wiring
                    .extra_args
                    .insert(spec.id, vec![flag.to_string(), path_str]);
            }
            MockDelivery::ProjectHooks { extra_args } => {
                let hook = oneharness_core::domain::hooks::HookSpec {
                    plugin_name: Some("oneharness-mock".into()),
                    ..oneharness_core::domain::hooks::HookSpec::command(&command)
                };
                // Plan first (check mode) to learn which files the install will
                // touch, snapshot exactly those, then install for real. Captured
                // per spec so one harness's install is never re-captured as
                // another's pre-existing state.
                let planned = hooks_io::install(Scope::Project(project_dir), spec, &hook, true)?;
                let paths: Vec<std::path::PathBuf> = planned.into_iter().map(|w| w.path).collect();
                wiring.snapshot.extend(HookSnapshot::capture(&paths));
                hooks_io::install(Scope::Project(project_dir), spec, &hook, false)?;
                if !extra_args.is_empty() {
                    wiring
                        .extra_args
                        .insert(spec.id, extra_args.iter().map(|a| a.to_string()).collect());
                }
            }
        }
    }
    Ok(Some(wiring))
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    results: Vec<RunResult>,
    prompts: &[String],
    model: Option<&str>,
    args: &RunArgs,
    mode: PermissionMode,
    schema: Option<&Schema>,
    max_retries: u32,
    batch: Option<BatchReport>,
    config_files: Vec<String>,
    mock: Option<MockReport>,
    history_file: Option<String>,
    session: Option<SessionReport>,
) -> RunReport {
    RunReport {
        schema_version: SCHEMA_VERSION,
        oneharness_version: env!("CARGO_PKG_VERSION"),
        // On a batch run the per-result `prompt` is authoritative; the top-level
        // field repeats the first prompt for back-compat (it is always present).
        prompt: prompts[0].clone(),
        // The effective top-level model (CLI, else config); a per-harness config
        // model is visible in that result's `command`.
        model: model.map(str::to_string),
        resume: args.resume.clone(),
        fork: args.fork,
        session,
        permission_mode: mode,
        bypass_permissions: mode.is_bypass(),
        dry_run: args.print_command,
        schema: schema.map(|s| s.as_value().clone()),
        schema_max_retries: schema.map(|_| max_retries),
        batch,
        mock_rules: mock.as_ref().and_then(|m| m.rules.clone()),
        spy_file: mock.and_then(|m| m.spy_file),
        history_file,
        config_files,
        results,
    }
}

/// The resolved `--session` context, carried from validation to finalization:
/// which named session, on which harness/project, where its store file lives,
/// what it already held, and the create-vs-continue plan.
struct SessionWiring {
    name: String,
    harness: &'static str,
    project: PathBuf,
    path: PathBuf,
    existing: Option<SessionRecord>,
    plan: SessionPlan,
}

/// Validate and resolve a `--session <name>` request against the store, or
/// `Ok(None)` when the flag was not passed. Loud usage errors up front (nothing
/// spawns): a batch run, more than one harness, a harness that exposes no session
/// id (`session_capable`), an unresolvable store directory, or a name already
/// bound to a different harness. On success the returned plan says whether to
/// create fresh or continue a stored token.
fn setup_session(
    args: &RunArgs,
    specs: &[&'static HarnessSpec],
    batch_run: bool,
    project: &std::path::Path,
) -> Result<Option<SessionWiring>, OneharnessError> {
    let Some(name) = args.session.as_deref() else {
        return Ok(None);
    };
    if batch_run {
        return Err(OneharnessError::SessionBatch);
    }
    if specs.len() != 1 {
        return Err(OneharnessError::SessionMultipleHarnesses {
            count: specs.len(),
            selected: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
        });
    }
    let spec = specs[0];
    if !spec.session_capable {
        return Err(OneharnessError::SessionUnsupported {
            id: spec.id.to_string(),
            supported: harness::all()
                .iter()
                .filter(|s| s.session_capable)
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    let dir = session_io::resolve_dir(args.session_dir.as_deref().and_then(|p| p.to_str()))
        .ok_or(OneharnessError::SessionNoStore)?;
    let path = session_io::session_path(&dir, project, name);
    let existing = session_io::read(&path);
    if let Some(was) = session::harness_conflict(existing.as_ref(), spec.id) {
        return Err(OneharnessError::SessionHarnessConflict {
            name: name.to_string(),
            was: was.to_string(),
            now: spec.id.to_string(),
        });
    }
    let plan = SessionPlan::decide(existing.as_ref());
    Ok(Some(SessionWiring {
        name: name.to_string(),
        harness: spec.id,
        project: project.to_path_buf(),
        path,
        existing,
        plan,
    }))
}

/// Persist the session token this run captured (best-effort) and build the
/// report block. On a create the token is the result's extracted `session_id`;
/// on a continue it is re-affirmed (a harness may rotate it). Under
/// `--print-command` nothing ran, so nothing is written and the block echoes the
/// stored token (or null on a fresh create). A create run that exposed no session
/// id warns — the handle cannot be continued — rather than storing an empty one.
fn finalize_session(
    wiring: Option<SessionWiring>,
    results: &[RunResult],
    dry_run: bool,
) -> Option<SessionReport> {
    let wiring = wiring?;
    let captured = results.first().and_then(|r| r.session_id.clone());
    if !dry_run {
        match &captured {
            Some(token) => {
                if let Err(err) = session_io::write(
                    &wiring.path,
                    &wiring.project,
                    wiring.harness,
                    &wiring.name,
                    token,
                    wiring.existing.as_ref(),
                ) {
                    eprintln!(
                        "oneharness: warning: could not write session store `{}`: {err}",
                        wiring.path.display()
                    );
                }
            }
            None => eprintln!(
                "oneharness: warning: harness `{}` exposed no session id, so `--session {}` \
                 cannot be continued (nothing was stored)",
                wiring.harness, wiring.name
            ),
        }
    }
    // Report the fresh capture if any, else the token we resumed with.
    let token = captured.or_else(|| wiring.plan.resume_token.clone());
    let store_file = std::path::absolute(&wiring.path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| wiring.path.display().to_string());
    Some(SessionReport {
        // Echo the sanitized handle — exactly what the store keyed the file on.
        name: oneharness_core::domain::history::sanitize_name(&wiring.name),
        phase: wiring.plan.phase,
        token,
        store_file: Some(store_file),
    })
}

/// Open the history session writer for this run, or `None` when history is off,
/// under `--print-command`, or the store cannot be opened. Best-effort: every
/// failure warns on stderr and disables history rather than aborting the run.
fn open_history_writer(
    args: &RunArgs,
    cfg: &oneharness_core::domain::config::FileConfig,
    project_start: &std::path::Path,
    prompts: &[String],
) -> Option<HistoryWriter> {
    if args.print_command {
        return None;
    }
    let enabled = if args.history {
        true
    } else if args.no_history {
        false
    } else {
        cfg.history.unwrap_or(false)
    };
    if !enabled {
        return None;
    }
    let configured = args
        .history_dir
        .as_deref()
        .map(|p| p.display().to_string())
        .or_else(|| cfg.history_dir.clone());
    let Some(dir) = history::resolve_dir(configured.as_deref()) else {
        eprintln!(
            "oneharness: warning: history is enabled but no history directory could be resolved \
             (pass --history-dir, set `history_dir`, or ONEHARNESS_HISTORY_DIR); \
             skipping history for this run"
        );
        return None;
    };
    let name = args.history_name.clone().unwrap_or_else(|| {
        oneharness_core::domain::history::session_name(
            prompts.first().map(String::as_str).unwrap_or(""),
        )
    });
    match HistoryWriter::open(&dir, project_start, &name) {
        Ok(writer) => Some(writer),
        Err(err) => {
            eprintln!(
                "oneharness: warning: could not open a history file under `{}`: {err}; \
                 skipping history for this run",
                dir.display()
            );
            None
        }
    }
}

/// Append each finished result to the session's history file, if history is on.
/// Best-effort per record: a write failure warns and moves on (the run's stdout
/// report is authoritative; history is a side channel).
fn record_history(
    writer: &Option<HistoryWriter>,
    mode: PermissionMode,
    model: Option<&str>,
    run_prompt: &str,
    results: &[RunResult],
) {
    let Some(writer) = writer else { return };
    for r in results {
        if let Err(err) = writer.append(mode, model, run_prompt, r) {
            eprintln!(
                "oneharness: warning: could not write history record for `{}`: {err}",
                r.harness
            );
        }
    }
}

/// Run one harness with streaming: feed each stdout line through the event
/// extractor as it arrives and write any new normalized events to stdout as
/// NDJSON (`{"type":"event","event":{…}}`), then return the same [`RunResult`] a
/// batch run would produce (from the accumulated output). A write failure to the
/// consumer (it closed the stream — short-circuiting on what it saw) tells the
/// runner to stop and tear down the child. `schema` is always `None` here
/// (`--stream` and `--schema` are mutually exclusive, enforced up front).
fn stream_one_harness(
    job: &Job,
    spec: &'static HarnessSpec,
    bin: &str,
    output_format: OutputFormat,
    prompt: Option<String>,
) -> RunResult {
    use oneharness_core::io::runner::StreamStep;
    use serde_json::Value;
    use std::io::Write;

    let mut next_index = 0usize;
    let capture = runner::run_job_streaming(job, |line| {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return StreamStep::Continue;
        };
        let evs = events::events_from_value(&value, next_index);
        if evs.is_empty() {
            return StreamStep::Continue;
        }
        next_index += evs.len();
        let mut out = std::io::stdout().lock();
        for ev in &evs {
            let envelope = serde_json::json!({ "type": "event", "event": ev });
            // A broken pipe (consumer closed the stream) is the short-circuit
            // signal: stop reading and tear the child down.
            if serde_json::to_string(&envelope)
                .map_err(|_| ())
                .and_then(|s| writeln!(out, "{s}").map_err(|_| ()))
                .is_err()
            {
                return StreamStep::Stop;
            }
        }
        if out.flush().is_err() {
            return StreamStep::Stop;
        }
        StreamStep::Continue
    });
    executed_result(
        spec,
        bin.to_string(),
        job.argv.clone(),
        output_format,
        &capture,
        None,
        1,
        prompt,
    )
}

/// Write the terminal `{"type":"result","report":<RunReport>}` line that closes a
/// streaming run — the same envelope a non-streaming run emits, so a consumer
/// that ignored the incremental events still gets the full report. A broken pipe
/// (the consumer already short-circuited and left) is not an error.
fn emit_stream_result(report: &RunReport) -> Result<(), OneharnessError> {
    use std::io::Write;
    let line = serde_json::to_string(&serde_json::json!({ "type": "result", "report": report }))?;
    // A broken pipe (the consumer already short-circuited and left) is expected,
    // not an error; any other write failure on the terminal line is non-fatal.
    let _ = writeln!(std::io::stdout(), "{line}");
    Ok(())
}

/// Refuse `--stream` combined with anything it cannot serve: more than one
/// harness, a batch (multi-prompt) run, or structured output — each needs the
/// whole output at once, which streaming does not provide. A loud usage error
/// before anything spawns.
fn validate_stream(
    stream: bool,
    specs: &[&'static HarnessSpec],
    batch_run: bool,
    has_schema: bool,
) -> Result<(), OneharnessError> {
    if !stream {
        return Ok(());
    }
    if specs.len() > 1 {
        return Err(OneharnessError::StreamInvalid(
            "--stream runs a single harness; select exactly one with --harness <id> (a \
             multi-harness stream would interleave unrelated event streams on one stdout)"
                .to_string(),
        ));
    }
    if batch_run {
        return Err(OneharnessError::StreamInvalid(
            "--stream is incompatible with a batch (multiple prompts): the batch fan-out needs \
             each run's whole output. Stream one prompt at a time instead."
                .to_string(),
        ));
    }
    if has_schema {
        return Err(OneharnessError::StreamInvalid(
            "--stream is incompatible with --schema: structured-output validation and its retry \
             loop need the complete answer, not an incremental stream."
                .to_string(),
        ));
    }
    Ok(())
}

/// Run `jobs` wave by wave, preserving global order in the returned outcomes.
/// Each wave is a set of job indices run concurrently (up to `max_parallel`),
/// with a barrier between waves — so a `min-tokens` batch's warm-up call fully
/// completes (writing the shared cache prefix) before the readers fan out. The
/// structured-output retry closure is rebound per wave so its local index maps
/// back to the right `job_plans` entry. Every index in `0..jobs.len()` must
/// appear in exactly one wave (guaranteed by `batch::waves` and the single-wave
/// fallback), so every outcome slot is filled exactly once.
fn run_in_waves(
    jobs: &[Job],
    job_plans: &[HarnessPlan],
    schema: Option<&Schema>,
    max_retries: u32,
    max_parallel: usize,
    waves: &[Vec<usize>],
) -> Vec<Outcome> {
    let mut slots: Vec<Option<Outcome>> = (0..jobs.len()).map(|_| None).collect();
    for wave in waves {
        let wave_jobs: Vec<Job> = wave.iter().map(|&i| jobs[i].clone()).collect();
        let outs = match schema {
            // Structured output: after each run, validate and (if it failed and
            // retries remain) re-run with a feedback prompt. The closure is pure
            // domain validation; the runner owns the spawning.
            Some(sch) => runner::run_jobs_with(&wave_jobs, max_parallel, |k, attempt, capture| {
                retry_decision(&job_plans[wave[k]], sch, attempt, max_retries, capture)
            }),
            None => runner::run_jobs_with(&wave_jobs, max_parallel, |_, _, _| None),
        };
        for (k, out) in outs.into_iter().enumerate() {
            slots[wave[k]] = Some(out);
        }
    }
    slots
        .into_iter()
        .map(|o| o.expect("every job is scheduled into exactly one wave"))
        .collect()
}

/// Execute a fork-based `min-tokens` batch (job 0 is the warm-up, jobs 1.. the
/// fan-out). Runs the warm-up to establish a session, then — if it exposed a
/// session id — rewrites each fan-out job to `--resume <sid> --fork` (dropping
/// `--system`, which the session already carries) so the fan-out reuses the
/// warmed cached prefix, and runs them concurrently. Falls back to plain
/// independent fan-out (a warning, no reuse) when no session id is exposed.
/// Mutates `jobs`/`job_plans` in place (the caller reads back the rewritten argv
/// for the report, and checks `job_plans[1].resume` to learn whether it forked).
/// Returns outcomes in job order. The structured-output retry closure is rebound
/// per wave so its local index maps to the right `job_plans` entry.
fn run_fork_batch(
    jobs: &mut [Job],
    job_plans: &mut [HarnessPlan],
    schema: Option<&Schema>,
    max_retries: u32,
    max_parallel: usize,
) -> Vec<Outcome> {
    let n = jobs.len();
    // Warm-up: job 0 alone (its own wave).
    let warm = runner::run_jobs_with(&jobs[0..1], 1, |_, attempt, capture| {
        schema.and_then(|s| retry_decision(&job_plans[0], s, attempt, max_retries, capture))
    })
    .into_iter()
    .next()
    .expect("one warm-up outcome");
    if n == 1 {
        return vec![warm];
    }
    // Branch the fan-out from the warmed session, when one was exposed. A
    // fork-capable harness (claude-code, opencode) emits a session id headlessly;
    // if absent, leave the fan-out as independent runs so the batch still
    // produces results (no cache reuse).
    match signals::extract_session(&warm.capture.stdout) {
        Some(sid) => {
            for plan in job_plans.iter_mut().skip(1) {
                plan.resume = Some(sid.clone());
                plan.fork = true;
                plan.system = None; // the session already carries --system
            }
            for (i, job) in jobs.iter_mut().enumerate().skip(1) {
                job.argv = job_plans[i].argv(schema, None);
            }
        }
        None => eprintln!(
            "oneharness: warning: the batch warm-up exposed no session id to fork; \
             the fan-out runs independently (no cache reuse)."
        ),
    }
    // Fan-out: jobs 1..n concurrently (local index k → job 1 + k).
    let fan = runner::run_jobs_with(&jobs[1..n], max_parallel, |k, attempt, capture| {
        schema.and_then(|s| retry_decision(&job_plans[1 + k], s, attempt, max_retries, capture))
    });
    let mut outcomes = Vec::with_capacity(n);
    outcomes.push(warm);
    outcomes.extend(fan);
    outcomes
}

/// A planned harness: either fully resolved (skipped/planned) or awaiting a job.
/// `Ready` is boxed because `RunResult` is far larger than `Pending`'s fields.
enum Plan {
    Ready(Box<RunResult>),
    Pending {
        spec: &'static HarnessSpec,
        bin: String,
        output_format: OutputFormat,
        job_index: usize,
        /// The per-result prompt on a batch run; `None` otherwise.
        prompt: Option<String>,
    },
}

fn planned_result(
    spec: &HarnessSpec,
    bin: &str,
    available: bool,
    command: Vec<String>,
    output_format: OutputFormat,
    prompt: Option<String>,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available,
        status: Status::Planned,
        prompt,
        exit_code: None,
        duration_ms: None,
        command,
        output_format,
        text: None,
        text_source: None,
        usage: Usage::default(),
        usage_source: None,
        session_id: None,
        events: None,
        events_source: None,
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
        failure_kind: None,
        failure_kind_source: None,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
    }
}

fn skipped_result(
    spec: &HarnessSpec,
    bin: &str,
    command: Vec<String>,
    output_format: OutputFormat,
    prompt: Option<String>,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available: false,
        status: Status::Skipped,
        prompt,
        exit_code: None,
        duration_ms: None,
        command,
        output_format,
        text: None,
        text_source: None,
        usage: Usage::default(),
        usage_source: None,
        session_id: None,
        events: None,
        events_source: None,
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
        failure_kind: None,
        failure_kind_source: None,
        stdout: String::new(),
        stderr: String::new(),
        error: Some(format!(
            "`{bin}` not found on PATH; harness skipped. Install it: {}",
            spec.install_hint
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn executed_result(
    spec: &HarnessSpec,
    bin: String,
    command: Vec<String>,
    output_format: OutputFormat,
    capture: &Capture,
    schema: Option<&Schema>,
    attempts: u32,
    prompt: Option<String>,
) -> RunResult {
    // Best-effort signals are extracted only from a run that actually produced
    // output (ok or non-zero), never fabricated for a timeout/spawn failure.
    let extracted = match capture.status {
        Status::Ok | Status::Nonzero => normalize::extract(&capture.stdout, output_format),
        _ => None,
    };
    let (text, text_source) = match &extracted {
        Some(e) => (Some(e.text.clone()), Some(e.source.clone())),
        None => (None, None),
    };
    // Structured output: re-derive the validated value for the final capture so
    // the report and the retry loop share one source of truth. Null fields when
    // no schema was requested, or when the run produced nothing to validate.
    let (structured, schema_valid, schema_attempts, schema_error) = match schema {
        Some(sch) if matches!(capture.status, Status::Ok | Status::Nonzero) => {
            let answer = extracted.as_ref().map(|e| e.text.as_str()).unwrap_or("");
            let check = structured::check(sch, spec.native_schema, answer, &capture.stdout);
            if check.is_valid() {
                (check.value, Some(true), Some(attempts), None)
            } else {
                (
                    check.value,
                    Some(false),
                    Some(attempts),
                    Some(check.errors.join("; ")),
                )
            }
        }
        // Ran under a schema but timed out / could not be spawned: nothing to
        // validate, but the attempt count is still meaningful.
        Some(_) => (None, None, Some(attempts), None),
        None => (None, None, None, None),
    };
    let usage_reading = match capture.status {
        Status::Ok | Status::Nonzero => signals::extract_usage(&capture.stdout),
        _ => None,
    };
    let (usage, usage_source) = match usage_reading {
        Some(r) => (r.usage, Some(r.source)),
        None => (Usage::default(), None),
    };
    let session_id = match capture.status {
        Status::Ok | Status::Nonzero => signals::extract_session(&capture.stdout),
        _ => None,
    };
    let events_reading = match capture.status {
        Status::Ok | Status::Nonzero => events::extract_events(&capture.stdout, output_format),
        _ => None,
    };
    let (events, events_source) = match events_reading {
        Some(r) => (Some(r.events), Some(r.source)),
        None => (None, None),
    };
    // Classify only an actual non-zero run: timeouts/spawn failures already carry
    // a oneharness-generated `error`, and `status` explains them.
    let failure = match capture.status {
        Status::Nonzero => signals::classify_failure(&capture.stdout, &capture.stderr),
        _ => None,
    };
    let (failure_kind, failure_kind_source) = match failure {
        Some(f) => (Some(f.kind), Some(f.source)),
        None => (None, None),
    };
    RunResult {
        harness: spec.id.to_string(),
        bin,
        available: true,
        status: capture.status,
        prompt,
        exit_code: capture.exit_code,
        duration_ms: capture.duration_ms,
        command,
        output_format,
        text,
        text_source,
        usage,
        usage_source,
        session_id,
        events,
        events_source,
        structured,
        schema_valid,
        schema_attempts,
        schema_error,
        failure_kind,
        failure_kind_source,
        stdout: capture.stdout.clone(),
        stderr: capture.stderr.clone(),
        error: capture.error.clone(),
    }
}

/// Everything needed to (re)build one harness's argv, retained so the
/// structured-output loop can re-run it with a feedback prompt. Holds owned
/// data because the retry closure runs on the runner's worker threads.
struct HarnessPlan {
    spec: &'static HarnessSpec,
    bin: String,
    model: Option<String>,
    system: Option<String>,
    resume: Option<String>,
    fork: bool,
    mode: PermissionMode,
    output_format: OutputFormat,
    /// The harness takes the schema through a native flag (so the prompt is left
    /// alone); otherwise the schema instruction is appended to the prompt.
    native: bool,
    base_prompt: String,
    /// Config `args` + CLI passthrough, appended verbatim after the built argv.
    extra: Vec<String>,
}

impl HarnessPlan {
    /// Build the argv for one attempt. `schema` drives structured output:
    /// non-native harnesses get the schema instruction appended to the prompt,
    /// native ones get it on the flag. `feedback` (the prior answer + validation
    /// errors) is appended on a retry so the model can correct itself.
    fn argv(&self, schema: Option<&Schema>, feedback: Option<(&str, &[String])>) -> Vec<String> {
        let mut prompt = self.base_prompt.clone();
        // A mode that synthesizes a behavioral posture from an instruction
        // (Codex's `plan`) prepends it so it frames the task. Single-line +
        // space-joined, matching the structured-output convention, so the prompt
        // argument stays newline-free for a `.cmd`-shim harness on Windows.
        if let Some(instruction) = self.spec.mode(self.mode).and_then(|m| m.instruction) {
            prompt = format!("{instruction} {prompt}");
        }
        if let Some(sch) = schema {
            // Join with a space, not a newline: the structured-output additions
            // must keep the prompt argument newline-free so it can still be passed
            // to a `.bat`/`.cmd` harness shim on Windows (Rust's std rejects an
            // argument with `\n`/`\r` there — see `structured::prompt_instruction`).
            if !self.native {
                prompt.push(' ');
                prompt.push_str(&structured::prompt_instruction(sch.as_text()));
            }
            if let Some((previous, errors)) = feedback {
                prompt.push(' ');
                prompt.push_str(&structured::retry_instruction(
                    sch.as_text(),
                    previous,
                    errors,
                ));
            }
        }
        let ctx = BuildCtx {
            bin: &self.bin,
            prompt: &prompt,
            model: self.model.as_deref(),
            system: self.system.as_deref(),
            resume: self.resume.as_deref(),
            fork: self.fork,
            mode: self.mode,
            output_format: self.output_format,
            schema: if self.native {
                schema.map(Schema::as_text)
            } else {
                None
            },
        };
        let mut argv = (self.spec.build_argv)(&ctx);
        argv.extend(self.extra.iter().cloned());
        argv
    }
}

/// Decide whether to re-run one harness under the structured-output loop. Returns
/// the next attempt's argv when the response failed validation and retries
/// remain, else `None`. `attempt` is the number of runs completed so far.
fn retry_decision(
    plan: &HarnessPlan,
    schema: &Schema,
    attempt: u32,
    max_retries: u32,
    capture: &Capture,
) -> Option<Vec<String>> {
    // Only a run that produced output can be validated; a timeout / spawn error
    // is not a validation failure and re-running it would just burn the budget.
    if !matches!(capture.status, Status::Ok | Status::Nonzero) {
        return None;
    }
    let answer = normalize::extract(&capture.stdout, plan.output_format).map(|e| e.text);
    let check = structured::check(
        schema,
        plan.spec.native_schema,
        answer.as_deref().unwrap_or(""),
        &capture.stdout,
    );
    if check.is_valid() || attempt > max_retries {
        return None;
    }
    // Feed back what the harness actually said (its extracted answer, else the
    // raw stdout) so the correction prompt is grounded in its own output.
    let previous = match &answer {
        Some(text) if !text.is_empty() => text.clone(),
        _ => capture.stdout.trim().to_string(),
    };
    Some(plan.argv(Some(schema), Some((&previous, &check.errors))))
}

/// Load and compile the structured-output schema, if one was requested. A
/// `--schema` path is relative to the process's working directory; a config
/// `schema_file` is relative to the project directory (where config was
/// discovered), mirroring how each source is written.
fn load_schema(
    args: &RunArgs,
    cfg: &oneharness_core::domain::config::FileConfig,
    project_start: &std::path::Path,
) -> Result<Option<Schema>, OneharnessError> {
    let path = if let Some(p) = &args.schema {
        p.clone()
    } else if let Some(rel) = &cfg.schema_file {
        project_start.join(rel)
    } else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|source| OneharnessError::SchemaFile {
        path: path.display().to_string(),
        source,
    })?;
    Schema::compile(&text)
        .map(Some)
        .map_err(OneharnessError::Schema)
}

/// A harness "failed" when it ran and did not exit cleanly, when it could not be
/// spawned, when — under `--require-available` — it was skipped as missing, or
/// when a structured-output run never produced a schema-conforming answer (a
/// run you asked for JSON from and didn't get is a failure, regardless of exit
/// code).
fn is_failure(
    status: Status,
    available: bool,
    require_available: bool,
    schema_valid: Option<bool>,
) -> bool {
    if schema_valid == Some(false) {
        return true;
    }
    match status {
        Status::Nonzero | Status::Timeout | Status::SpawnError => true,
        Status::Skipped => require_available && !available,
        Status::Ok | Status::Planned => false,
    }
}

fn exit_code(results: &[RunResult], require_available: bool) -> i32 {
    let failed = results
        .iter()
        .any(|r| is_failure(r.status, r.available, require_available, r.schema_valid));
    if failed {
        EXIT_FAILURE
    } else {
        EXIT_OK
    }
}

/// Resolve the prompt list, in order: every `--prompt` value, then every
/// `--prompt-file` (each file read whole as one prompt; `-` reads stdin once).
/// More than one prompt makes this a batch run. Empty is a usage error.
fn resolve_prompts(args: &RunArgs) -> Result<Vec<String>, OneharnessError> {
    let mut prompts: Vec<String> = args.prompt.clone();
    // stdin can be consumed only once; reading it twice would block/return empty.
    let stdin_count = args.prompt_file.iter().filter(|p| *p == "-").count();
    if stdin_count > 1 {
        return Err(OneharnessError::MultipleStdinPrompts { count: stdin_count });
    }
    for path in &args.prompt_file {
        if path == "-" {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|source| {
                OneharnessError::PromptFile {
                    path: "<stdin>".to_string(),
                    source,
                }
            })?;
            prompts.push(buf);
        } else {
            let text =
                std::fs::read_to_string(path).map_err(|source| OneharnessError::PromptFile {
                    path: path.clone(),
                    source,
                })?;
            prompts.push(text);
        }
    }
    if prompts.is_empty() {
        return Err(OneharnessError::NoPrompt);
    }
    Ok(prompts)
}

/// Resolve the effective system prompt from the mutually-exclusive `--system`
/// (inline argv) and `--system-file` (file, or `-` for stdin). `--system-file` is
/// the argv-limit escape hatch mirroring `--prompt-file`: a system prompt too
/// large for a single argv string trips `E2BIG` at spawn, so it is read from a
/// file instead. Returns `None` when neither flag is set, so the caller's config
/// `system` fallback applies. The `-`/stdin collision with `--prompt-file -` is
/// guarded before any read, so this never double-consumes stdin.
fn resolve_system(args: &RunArgs) -> Result<Option<String>, OneharnessError> {
    if let Some(text) = &args.system {
        return Ok(Some(text.clone()));
    }
    let Some(path) = &args.system_file else {
        return Ok(None);
    };
    let text = if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|source| {
            OneharnessError::SystemFile {
                path: "<stdin>".to_string(),
                source,
            }
        })?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|source| OneharnessError::SystemFile {
            path: path.clone(),
            source,
        })?
    };
    Ok(Some(text))
}

/// A batch run (more than one prompt) fans **one** harness over the prompts so
/// they share a provider cache prefix, which is per harness/model/tools — so it
/// requires exactly one selected harness — and is a fresh fan-out rather than a
/// session continuation, so it cannot combine with `--resume`/`--fork`. Both are
/// usage errors caught before anything spawns.
fn validate_batch(
    specs: &[&'static HarnessSpec],
    resume_or_fork: bool,
) -> Result<(), OneharnessError> {
    if specs.len() != 1 {
        return Err(OneharnessError::BatchMultipleHarnesses {
            count: specs.len(),
            selected: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
        });
    }
    if resume_or_fork {
        return Err(OneharnessError::BatchResume);
    }
    Ok(())
}

/// Write each result's raw stdout/stderr to `<dir>/<harness>.{stdout,stderr}`.
/// Lets consumers (e.g. allowlister's e2e scripts) read the transcript from files
/// without a JSON parser, preserving their existing `$stream`/`$stream.err`
/// contract.
fn write_output_dir(dir: &std::path::Path, results: &[RunResult]) -> Result<(), OneharnessError> {
    std::fs::create_dir_all(dir).map_err(|source| OneharnessError::OutputDir {
        path: dir.display().to_string(),
        source,
    })?;
    // On a batch run every result is the same harness, so a bare `<harness>`
    // stem would overwrite itself. Disambiguate with the index whenever a harness
    // id repeats; unique ids (the ordinary `--all` case) keep their plain name.
    let counts = results.iter().fold(
        std::collections::HashMap::<&str, usize>::new(),
        |mut m, r| {
            *m.entry(r.harness.as_str()).or_default() += 1;
            m
        },
    );
    for (index, result) in results.iter().enumerate() {
        let stem = if counts.get(result.harness.as_str()).copied().unwrap_or(0) > 1 {
            format!("{}-{index}", result.harness)
        } else {
            result.harness.clone()
        };
        for (suffix, contents) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
            let path = dir.join(format!("{stem}.{suffix}"));
            std::fs::write(&path, contents).map_err(|source| OneharnessError::OutputDir {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    Ok(())
}

/// Resume is stateful and single-session: it requires exactly one selected
/// harness, and that harness must support continuation. Both are usage errors,
/// caught before any process is spawned (`--all` is already excluded by clap).
fn validate_resume(
    resume: Option<&str>,
    specs: &[&'static HarnessSpec],
) -> Result<(), OneharnessError> {
    if resume.is_none() {
        return Ok(());
    }
    if specs.len() != 1 {
        return Err(OneharnessError::ResumeMultipleHarnesses {
            count: specs.len(),
            selected: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
        });
    }
    let spec = specs[0];
    if !spec.supports_resume {
        return Err(OneharnessError::ResumeUnsupported {
            id: spec.id.to_string(),
            supported: harness::all()
                .iter()
                .filter(|s| s.supports_resume)
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(())
}

/// `--fork` branches a new session from the resumed one instead of appending. It
/// implies `--resume` (clap-enforced), so the single-harness constraint is already
/// guaranteed by [`validate_resume`]; the only extra check is that the selected
/// harness can actually fork — otherwise it is a usage error, never a silent
/// linear resume. Caught before any process is spawned.
fn validate_fork(fork: bool, specs: &[&'static HarnessSpec]) -> Result<(), OneharnessError> {
    if !fork {
        return Ok(());
    }
    // clap guarantees `--fork` implies `--resume`, and `validate_resume` already
    // proved exactly one harness; guard defensively all the same.
    let Some(spec) = specs.first() else {
        return Ok(());
    };
    if !spec.supports_fork {
        return Err(OneharnessError::ForkUnsupported {
            id: spec.id.to_string(),
            supported: harness::all()
                .iter()
                .filter(|s| s.supports_fork)
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(())
}

/// Resolve the effective approval mode. Precedence, highest first: CLI `--mode`,
/// then the CLI `--bypass` / `--no-bypass` shorthands, then config `mode`, then
/// the legacy config `bypass` boolean, then the built-in default — `default`
/// (the harness's normal permission posture, mapped to its cleanest
/// non-interactive variant), *not* bypass: bypass is the opt-in.
fn resolve_mode(
    args: &RunArgs,
    cfg: &oneharness_core::domain::config::FileConfig,
) -> PermissionMode {
    if let Some(m) = args.mode {
        m
    } else if args.bypass {
        PermissionMode::Bypass
    } else if args.no_bypass {
        PermissionMode::Default
    } else if let Some(m) = cfg.mode {
        m
    } else {
        // Legacy `bypass = true/false` maps to bypass/default; unset → default.
        cfg.bypass
            .map(PermissionMode::from_bypass)
            .unwrap_or(PermissionMode::Default)
    }
}

/// Refuse — before any process is spawned — a mode a selected harness *cannot
/// express*: there is no command to build for it, so it is a loud usage error
/// ([`OneharnessError::ModeUnsupported`]) rather than a silent downgrade. A mode
/// that is supported but might *block on a prompt* headlessly is not refused
/// here — see [`hang_prone`] (it is warned about and run, with `--timeout` as
/// the backstop). Reports the first offending harness, mirroring `validate_resume`.
fn validate_modes(
    mode: PermissionMode,
    specs: &[&'static HarnessSpec],
) -> Result<(), OneharnessError> {
    for spec in specs {
        if spec.mode(mode).is_none() {
            return Err(OneharnessError::ModeUnsupported {
                id: spec.id.to_string(),
                mode: mode.as_str().to_string(),
                supported: spec
                    .modes
                    .iter()
                    .map(|m| m.mode.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
    }
    Ok(())
}

/// The selected harnesses for which `mode` is supported but would block on an
/// interactive approval prompt headlessly (`ModeHeadless::Hangs`). The caller
/// warns about each (unless `--permit-prompts`) but still runs them — the
/// per-harness `--timeout` turns any real hang into a `timeout` result.
fn hang_prone(mode: PermissionMode, specs: &[&'static HarnessSpec]) -> Vec<&'static str> {
    specs
        .iter()
        .filter(|spec| {
            spec.mode(mode)
                .is_some_and(|m| m.headless == ModeHeadless::Hangs)
        })
        .map(|spec| spec.id)
        .collect()
}

fn parse_env(values: &[String]) -> Result<Vec<(String, String)>, OneharnessError> {
    values
        .iter()
        .map(|v| {
            v.split_once('=')
                .filter(|(k, _)| !k.is_empty())
                .map(|(k, val)| (k.to_string(), val.to_string()))
                .ok_or_else(|| OneharnessError::BadEnv(v.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(status: Status, available: bool) -> RunResult {
        RunResult {
            harness: "x".into(),
            bin: "x".into(),
            available,
            status,
            prompt: None,
            exit_code: None,
            duration_ms: None,
            command: vec![],
            output_format: OutputFormat::Text,
            text: None,
            text_source: None,
            usage: Usage::default(),
            usage_source: None,
            session_id: None,
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    fn crush_plan() -> HarnessPlan {
        HarnessPlan {
            spec: harness::by_id("crush").unwrap(),
            bin: "crush".into(),
            model: None,
            system: None,
            resume: None,
            fork: false,
            mode: PermissionMode::Bypass,
            output_format: OutputFormat::Text,
            native: false,
            base_prompt: "p".into(),
            extra: Vec::new(),
        }
    }

    fn capture(status: Status, stdout: &str) -> Capture {
        Capture {
            status,
            exit_code: None,
            duration_ms: Some(1),
            stdout: stdout.into(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn schema_argv_prompt_stays_newline_free_including_on_retry() {
        // Windows passes the prompt as one argv element to a `.cmd` harness shim,
        // which Rust's std refuses if it contains `\n`/`\r`. The structured-output
        // additions (instruction + retry feedback, even with a multi-line prior
        // answer) must never introduce one.
        let schema = Schema::compile(r#"{"type":"object"}"#).unwrap();
        // Prompt-based (non-native): first attempt appends the instruction.
        let plan = crush_plan();
        let argv = plan.argv(Some(&schema), None);
        assert!(argv.iter().all(|a| !a.contains('\n')), "{argv:?}");
        // ... and a retry with a multi-line prior answer + errors.
        let argv = plan.argv(
            Some(&schema),
            Some((
                "multi\nline\r\nanswer",
                &["e1".to_string(), "e2".to_string()],
            )),
        );
        assert!(
            argv.iter().all(|a| !a.contains('\n') && !a.contains('\r')),
            "{argv:?}"
        );
        // Native (claude) retry: schema rides the flag, feedback rides the prompt.
        let mut native = crush_plan();
        native.spec = harness::by_id("claude-code").unwrap();
        native.native = true;
        native.output_format = OutputFormat::Json;
        let argv = native.argv(Some(&schema), Some(("multi\nline", &["e".to_string()])));
        assert!(argv.iter().all(|a| !a.contains('\n')), "{argv:?}");
    }

    #[test]
    fn retry_decision_covers_valid_invalid_exhausted_and_timeout() {
        let schema = Schema::compile(
            r#"{"type":"object","required":["a"],"properties":{"a":{"type":"integer"}}}"#,
        )
        .unwrap();
        let plan = crush_plan();
        // Conforming → stop.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, r#"{"a":1}"#)).is_none());
        // Non-conforming with budget left → re-run with a feedback prompt.
        let next = retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, r#"{"a":"x"}"#))
            .expect("should retry");
        assert!(next.iter().any(|a| a.contains("did not conform")));
        // Budget spent → stop even though still invalid.
        assert!(retry_decision(&plan, &schema, 3, 2, &capture(Status::Ok, "{}")).is_none());
        // A timeout is not a validation failure, so it is never retried.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Timeout, "")).is_none());
        // No extractable answer falls back to the raw stdout in the feedback.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, "  ")).is_some());
    }

    #[test]
    fn executed_result_fills_schema_fields_per_status() {
        let schema = Schema::compile(r#"{"type":"object","required":["a"]}"#).unwrap();
        let spec = harness::by_id("crush").unwrap();
        // A conforming ok run: valid, with the value and attempt count.
        let ok = capture(Status::Ok, r#"{"a":1}"#);
        let r = executed_result(
            spec,
            "crush".into(),
            vec!["crush".into()],
            OutputFormat::Text,
            &ok,
            Some(&schema),
            1,
            Some("the batch prompt".into()),
        );
        assert_eq!(r.schema_valid, Some(true));
        assert_eq!(r.schema_attempts, Some(1));
        assert!(r.structured.is_some());
        // The per-result prompt is carried through verbatim (batch runs).
        assert_eq!(r.prompt.as_deref(), Some("the batch prompt"));
        // Timed out under a schema: nothing to validate, attempts still recorded.
        let to = capture(Status::Timeout, "");
        let r = executed_result(
            spec,
            "crush".into(),
            vec![],
            OutputFormat::Text,
            &to,
            Some(&schema),
            1,
            None,
        );
        assert!(r.schema_valid.is_none());
        assert_eq!(r.schema_attempts, Some(1));
        // No prompt recorded on an ordinary (non-batch) result.
        assert!(r.prompt.is_none());
        // No schema requested: every structured field is null.
        let r = executed_result(
            spec,
            "crush".into(),
            vec![],
            OutputFormat::Text,
            &ok,
            None,
            1,
            None,
        );
        assert!(r.schema_valid.is_none());
        assert!(r.schema_attempts.is_none());
        assert!(r.structured.is_none());
    }

    #[test]
    fn schema_invalid_result_is_a_failure_even_when_status_ok() {
        // A structured-output run that exited cleanly but never conformed is a
        // failure (non-zero exit), so a consumer can gate on it.
        let mut r = result(Status::Ok, true);
        r.schema_valid = Some(false);
        assert_eq!(exit_code(std::slice::from_ref(&r), false), EXIT_FAILURE);
        // A conforming (or schema-less) ok run still passes.
        r.schema_valid = Some(true);
        assert_eq!(exit_code(std::slice::from_ref(&r), false), EXIT_OK);
    }

    #[test]
    fn ok_and_skipped_pass_by_default() {
        let results = vec![result(Status::Ok, true), result(Status::Skipped, false)];
        assert_eq!(exit_code(&results, false), EXIT_OK);
    }

    #[test]
    fn nonzero_fails() {
        let results = vec![result(Status::Ok, true), result(Status::Nonzero, true)];
        assert_eq!(exit_code(&results, false), EXIT_FAILURE);
    }

    #[test]
    fn skipped_fails_under_require_available() {
        let results = vec![result(Status::Skipped, false)];
        assert_eq!(exit_code(&results, true), EXIT_FAILURE);
        assert_eq!(exit_code(&results, false), EXIT_OK);
    }

    #[test]
    fn validate_resume_ok_for_single_supported_harness() {
        let claude = harness::by_id("claude-code").unwrap();
        assert!(validate_resume(Some("sid"), &[claude]).is_ok());
        // No --resume: anything goes.
        assert!(validate_resume(None, &[claude, claude]).is_ok());
    }

    #[test]
    fn validate_resume_rejects_multiple_harnesses() {
        let claude = harness::by_id("claude-code").unwrap();
        assert!(matches!(
            validate_resume(Some("sid"), &[claude, claude]),
            Err(OneharnessError::ResumeMultipleHarnesses { count: 2, .. })
        ));
    }

    /// A no-op argv builder, for the synthetic spec below.
    fn noop_argv(_: &BuildCtx) -> Vec<String> {
        Vec::new()
    }

    /// A synthetic harness that supports neither resume nor fork, to exercise the
    /// capability guards. Every real harness now supports resume (and two support
    /// fork), so the rejection branches have no real-spec witness; this stands in
    /// without weakening the guard. Promoted to `'static` (all-const fields).
    fn unsupported_spec() -> &'static HarnessSpec {
        &HarnessSpec {
            id: "test-unsupported",
            display: "Test Unsupported",
            default_bin: "test-unsupported",
            install_hint: "",
            output_format: OutputFormat::Text,
            events_format: None,
            supports_resume: false,
            session_capable: false,
            supports_fork: false,
            fork_reuses_cache: false,
            sync: None,
            hooks: None,
            global_hook: None,
            gate_deny: None,
            mock_rewrite: None,
            mock_delivery: None,
            default_env: &[],
            native_schema: None,
            modes: &[],
            build_argv: noop_argv,
        }
    }

    #[test]
    fn validate_resume_rejects_unsupported_harness() {
        assert!(matches!(
            validate_resume(Some("sid"), &[unsupported_spec()]),
            Err(OneharnessError::ResumeUnsupported { .. })
        ));
    }

    #[test]
    fn validate_fork_ok_for_capable_harness_and_noop_without_fork() {
        let claude = harness::by_id("claude-code").unwrap();
        assert!(validate_fork(true, &[claude]).is_ok());
        // Without --fork, any selection passes (the flag wasn't requested).
        let codex = harness::by_id("codex").unwrap();
        assert!(validate_fork(false, &[codex]).is_ok());
        // Defensive empty-selection guard (clap prevents this in practice).
        assert!(validate_fork(true, &[]).is_ok());
    }

    #[test]
    fn validate_fork_rejects_resume_only_harness() {
        // Codex supports --resume but resumes linearly (no fork).
        let codex = harness::by_id("codex").unwrap();
        assert!(matches!(
            validate_fork(true, &[codex]),
            Err(OneharnessError::ForkUnsupported { .. })
        ));
    }

    fn run_args() -> RunArgs {
        // A minimal RunArgs with only the mode-relevant flags set; the rest are
        // their clap defaults. Built via the parser so it stays in sync.
        use clap::Parser;
        let cli = crate::cli::Cli::parse_from(["oneharness", "run", "--prompt", "hi"]);
        match cli.command {
            crate::cli::Command::Run(args) => *args,
            _ => unreachable!(),
        }
    }

    fn cfg_with(
        mode: Option<PermissionMode>,
        bypass: Option<bool>,
    ) -> oneharness_core::domain::config::FileConfig {
        oneharness_core::domain::config::FileConfig {
            mode,
            bypass,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_mode_precedence() {
        let empty = cfg_with(None, None);
        // Built-in default is `default`, not bypass.
        assert_eq!(resolve_mode(&run_args(), &empty), PermissionMode::Default);
        // Legacy config bypass=true → bypass; bypass=false → default.
        assert_eq!(
            resolve_mode(&run_args(), &cfg_with(None, Some(true))),
            PermissionMode::Bypass
        );
        assert_eq!(
            resolve_mode(&run_args(), &cfg_with(None, Some(false))),
            PermissionMode::Default
        );
        // config mode beats config bypass.
        assert_eq!(
            resolve_mode(
                &run_args(),
                &cfg_with(Some(PermissionMode::Plan), Some(true))
            ),
            PermissionMode::Plan
        );
        // CLI --mode beats config; --bypass / --no-bypass are the shorthands.
        let mut a = run_args();
        a.mode = Some(PermissionMode::Edit);
        assert_eq!(
            resolve_mode(&a, &cfg_with(Some(PermissionMode::Plan), None)),
            PermissionMode::Edit
        );
        let mut a = run_args();
        a.no_bypass = true;
        assert_eq!(
            resolve_mode(&a, &cfg_with(Some(PermissionMode::Plan), None)),
            PermissionMode::Default
        );
        let mut a = run_args();
        a.bypass = true;
        assert_eq!(
            resolve_mode(&a, &cfg_with(None, Some(false))),
            PermissionMode::Bypass
        );
    }

    #[test]
    fn validate_modes_refuses_only_unsupported() {
        let crush = harness::by_id("crush").unwrap();
        let cursor = harness::by_id("cursor").unwrap();
        let claude = harness::by_id("claude-code").unwrap();
        // crush has no plan mode → unsupported (hard error, no command to build).
        assert!(matches!(
            validate_modes(PermissionMode::Plan, &[crush]),
            Err(OneharnessError::ModeUnsupported { .. })
        ));
        // A supported-but-hang-prone mode is NOT refused — it runs (with a
        // warning + timeout backstop). cursor `default` is hang-prone but valid.
        assert!(validate_modes(PermissionMode::Default, &[cursor]).is_ok());
        assert!(validate_modes(PermissionMode::Plan, &[claude]).is_ok());
        assert!(validate_modes(PermissionMode::Bypass, &[crush, cursor, claude]).is_ok());
    }

    #[test]
    fn hang_prone_lists_only_supported_hang_modes() {
        let cursor = harness::by_id("cursor").unwrap();
        let claude = harness::by_id("claude-code").unwrap();
        let crush = harness::by_id("crush").unwrap();
        // cursor `default` hangs; claude `default` is clean; crush doesn't support
        // plan at all (so it's not "hang-prone", it's unsupported → not listed).
        assert_eq!(hang_prone(PermissionMode::Default, &[cursor]), ["cursor"]);
        assert!(hang_prone(PermissionMode::Default, &[claude]).is_empty());
        assert!(hang_prone(PermissionMode::Plan, &[crush]).is_empty());
        assert!(hang_prone(PermissionMode::Bypass, &[cursor, claude]).is_empty());
    }

    #[test]
    fn parse_env_rejects_malformed() {
        assert!(parse_env(&["KEY=val".into()]).is_ok());
        assert!(parse_env(&["noeq".into()]).is_err());
        assert!(parse_env(&["=val".into()]).is_err());
    }

    #[test]
    fn parse_env_allows_empty_value_and_equals_in_value() {
        let parsed = parse_env(&["A=".into(), "B=x=y".into()]).unwrap();
        assert_eq!(
            parsed,
            vec![("A".into(), "".into()), ("B".into(), "x=y".into())]
        );
    }
}
