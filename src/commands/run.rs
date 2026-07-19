//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use oneharness_core::domain::batch::{self, BatchStrategy};
use oneharness_core::domain::fallback::{self, RunMode};
use oneharness_core::domain::harness::{self, BuildCtx, HarnessSpec};
use oneharness_core::domain::mock::{self, MockDelivery};
use oneharness_core::domain::mode::{ModeHeadless, PermissionMode};
use oneharness_core::domain::report::{
    BatchReport, Capture, FallThrough, FallbackReport, OutputFormat, RunReport, RunResult,
    SessionReport, Status, SCHEMA_VERSION,
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
use oneharness_core::io::runner::{self, Job, NextRun, Outcome};
use oneharness_core::io::session as session_io;
use std::path::PathBuf;

/// Exit codes (clap uses 2 for argument errors).
const EXIT_OK: i32 = 0;
const EXIT_FAILURE: i32 = 1;

/// Byte length past which a prompt or system prompt is delivered off the argv
/// (temp file / stdin) for a harness that supports it, instead of inline — so a
/// large value never trips the OS argument ceiling (`E2BIG`: Linux caps a single
/// argv string at 128 KiB, macOS/Windows cap the whole argv+env). 64 KiB is well
/// under every ceiling (leaving headroom for the rest of the argv and env) yet far
/// above any ordinary prompt, so the common case keeps its byte-identical inline
/// argv and only genuinely-large prompts switch delivery. See `LargeInput` and
/// issue #1115.
const LARGE_INPUT_THRESHOLD: usize = 64 * 1024;

/// Temp files holding off-argv prompt/system text for the duration of a run,
/// removed on drop — so every early return (stream path, an I/O error, normal
/// completion) cleans them up, like the mock hook's snapshot-and-restore. Writes
/// are best-effort; a removal failure is ignored (a leftover temp file is
/// harmless and the OS reclaims its temp dir).
#[derive(Default)]
struct TempPromptFiles(Vec<PathBuf>);

impl Drop for TempPromptFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl TempPromptFiles {
    /// Write `contents` to a fresh temp file (labelled by harness id + a unique
    /// index) and return its path, registering it for cleanup. The path is
    /// process- and index-unique so concurrent units never collide.
    fn write(
        &mut self,
        id: &str,
        index: usize,
        contents: &str,
    ) -> Result<PathBuf, OneharnessError> {
        let path = std::env::temp_dir().join(format!(
            "oneharness-input-{id}-{}-{index}.txt",
            std::process::id()
        ));
        std::fs::write(&path, contents).map_err(|source| OneharnessError::PromptFile {
            path: path.display().to_string(),
            source,
        })?;
        self.0.push(path.clone());
        Ok(path)
    }
}

/// Decide and apply off-argv delivery for a unit whose prompt or system prompt is
/// large enough to risk the argv ceiling. Mutates `plan` in place (setting
/// `system_file` / `prompt_stdin`, which the structured-output retry then reuses)
/// and, for a system prompt on a harness with a system-file flag, writes it to a
/// temp file. Small prompts return with `plan` untouched (byte-identical inline
/// argv). When a large value cannot be moved off the argv for the harness (no
/// stdin/file route), it stays inline and a warning names the risk rather than
/// silently letting the spawn fail later.
fn plan_large_input(
    plan: &mut HarnessPlan,
    spec: &HarnessSpec,
    system: Option<&str>,
    index: usize,
    temp_files: &mut TempPromptFiles,
) -> Result<(), OneharnessError> {
    let prompt_large = plan.base_prompt.len() > LARGE_INPUT_THRESHOLD;
    let system_large = system.is_some_and(|s| s.len() > LARGE_INPUT_THRESHOLD);
    if !prompt_large && !system_large {
        return Ok(());
    }
    let li = &spec.large_input;
    // System prompt via a file flag (Claude Code): materialize it and let
    // build_argv emit the flag instead of the inline value.
    let use_system_file = system_large && li.system_file_flag.is_some();
    if use_system_file {
        let path = temp_files.write(spec.id, index, system.unwrap_or_default())?;
        plan.system_file = Some(path.display().to_string());
    }
    // User prompt (and, for a system-rides-prompt harness, the system with it) via
    // stdin. Also triggered by a large *system* alone on such a harness, since its
    // system rides the same stream.
    let use_stdin = li.prompt_stdin && (prompt_large || (li.system_rides_prompt && system_large));
    if use_stdin {
        plan.prompt_stdin = true;
    }
    // Loud when a large value is stuck on the argv anyway (e.g. Goose's inline
    // `--system`, or any harness oneharness has not wired for off-argv input).
    if prompt_large && !use_stdin {
        eprintln!(
            "oneharness: warning: the prompt for harness `{}` is {} KiB and cannot be delivered \
             off the argv (no stdin/file route wired) — it stays inline and may exceed the OS \
             argument limit (E2BIG) at spawn.",
            spec.id,
            plan.base_prompt.len() / 1024
        );
    }
    let system_off_argv = use_system_file || (use_stdin && li.system_rides_prompt);
    if system_large && !system_off_argv {
        eprintln!(
            "oneharness: warning: the --system prompt for harness `{}` is {} KiB and cannot be \
             delivered off the argv (no system file flag; its system does not ride the prompt) — \
             it stays inline and may exceed the OS argument limit (E2BIG) at spawn.",
            spec.id,
            system.map_or(0, str::len) / 1024
        );
    }
    Ok(())
}

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
    // The model axis. An explicit list — repeated `--model` (CLI beats config)
    // or config `models` — turns the run into a **model fan-out**: the harness ×
    // model cross-product in parallel, or the (harness, model) priority chain in
    // fallback. Absent, each harness resolves its own single model (`model_for`),
    // exactly as before. A one-element list is not a fan-out (it behaves like the
    // historical single `--model`), so `multi_model` gates on more than one.
    let explicit_models: Option<Vec<String>> = if !args.model.is_empty() {
        Some(args.model.clone())
    } else {
        cfg.models.clone()
    };
    let multi_model = explicit_models.as_ref().is_some_and(|m| m.len() > 1);
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
    // `--run-mode` (CLI beats config; default `parallel`). Fallback runs the
    // selected harnesses in priority order, stopping at the first that runs and
    // falling through only harnesses that cannot run at all. It is single-outcome
    // by nature, so it refuses the multi-prompt / continuation shapes up front —
    // but the *whole candidate set* still flows through every capability validator
    // below, so a flag unsupported by ANY listed harness fails fast even though
    // only one harness will run (the command must be valid for the whole set).
    let run_mode = args.run_mode.or(cfg.run_mode).unwrap_or(RunMode::Parallel);
    let fallback_mode = run_mode == RunMode::Fallback;
    if fallback_mode {
        validate_fallback(batch_run, args)?;
    }
    // A model fan-out multiplies the run into several (harness, model) units, so —
    // like a batch — it refuses every single-unit shape up front (loud usage
    // errors). It is *compatible* with fallback: the model list is exactly the
    // fallback chain there.
    if multi_model {
        validate_multi_model(batch_run, args)?;
    }
    // In fallback the run order is the priority chain: the caller's `--harness` /
    // config order (registry order under `--all`), not the registry order
    // `select_specs` returns. Parallel keeps registry order.
    let specs = if fallback_mode {
        fallback_order(specs, &include)
    } else {
        specs
    };
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
    // token (via the session store) before building argv. Validates capability +
    // no-batch loudly; in parallel it is single-harness, in fallback it binds to
    // the anchor (the first session-capable harness in the chain). On a continue it
    // yields the token to resume with, reusing the harness's verified `--resume`
    // mapping. `None` when the flag was not passed.
    let session_wiring = setup_session(args, &specs, batch_run, fallback_mode, &project_start)?;
    let session_resume: Option<String> = session_wiring
        .as_ref()
        .and_then(|w| w.plan.resume_token.clone());
    // The harness the session is bound to (the anchor in fallback, the single
    // harness in parallel). The resume token is applied ONLY to this harness's
    // argv below, never to a different fallback candidate that happens to win.
    let session_anchor: Option<&'static str> = session_wiring.as_ref().map(|w| w.harness);
    // An explicitly selected format keeps its authority, but a named session can
    // use it only when that harness actually emits an id in the format. Refuse a
    // lossy pin before spawning instead of accepting `--session` and silently
    // leaving the store empty. With no explicit format, the plan loop below
    // selects the anchor harness's preferred session-bearing format.
    let explicit_format = args.output_format.or(cfg.output_format);
    validate_session_output_format(session_anchor, explicit_format)?;
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
    // A reasoning/effort setting for a harness that has no headless argv surface
    // for it is refused here (no way to deliver it) — a loud usage error rather
    // than a silent drop, mirroring an unsupported mode.
    validate_reasoning(args, cfg, &specs)?;
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
    // The effective top-level model for the report/history: the first fan-out
    // model when a list was given, else the single configured/CLI model. Each
    // result's own `model` (set per unit below) is authoritative on a fan-out.
    let top_model: Option<String> = explicit_models
        .as_ref()
        .and_then(|l| l.first().cloned())
        .or_else(|| cfg.model.clone());
    let model = top_model.as_deref();
    // Present on the report only for a real fan-out (more than one model), the
    // signal a consumer keys on to read each result's own `model`.
    let report_models: Option<Vec<String>> = if multi_model {
        explicit_models.clone()
    } else {
        None
    };
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
    let history_writer = open_history_writer(args, cfg, &project_start, &prompts)?;
    let history_file = history_writer.as_ref().map(|w| {
        std::path::absolute(w.path())
            .unwrap_or_else(|_| w.path().to_path_buf())
            .display()
            .to_string()
    });

    // The (harness, model, prompt) units to run:
    //  - a **batch** run is the single selected harness against each prompt, one
    //    model (batch refuses a fan-out), so `results` is one entry per prompt;
    //  - a **model fan-out** is the harness × model cross-product (harness-major,
    //    model-minor), one prompt, so each harness repeats once per model;
    //  - an ordinary run is each selected harness against the one prompt, with its
    //    own resolved model (`model_for`) — the historical behavior.
    // In fallback mode `specs` is already in priority order, so this ordering is
    // the fallback chain.
    let units: Vec<(&'static HarnessSpec, Option<String>, &str)> = if batch_run {
        let m = explicit_models
            .as_ref()
            .and_then(|l| l.first().cloned())
            .or_else(|| cfg.model_for(specs[0].id).map(str::to_string));
        prompts
            .iter()
            .map(|p| (specs[0], m.clone(), p.as_str()))
            .collect()
    } else if let Some(list) = &explicit_models {
        let mut units = Vec::with_capacity(specs.len() * list.len());
        for spec in &specs {
            for m in list {
                units.push((*spec, Some(m.clone()), prompts[0].as_str()));
            }
        }
        units
    } else {
        specs
            .iter()
            .map(|s| {
                (
                    *s,
                    cfg.model_for(s.id).map(str::to_string),
                    prompts[0].as_str(),
                )
            })
            .collect()
    };

    // Build a plan entry for every unit; queue jobs only for the ones that are
    // available and actually being executed. `job_plans` parallels `jobs` and
    // retains what the structured-output retry loop needs to rebuild a unit's
    // argv with a feedback prompt.
    let mut plan: Vec<Plan> = Vec::with_capacity(units.len());
    let mut jobs: Vec<Job> = Vec::new();
    let mut job_plans: Vec<HarnessPlan> = Vec::new();
    // Temp files backing off-argv system prompts, cleaned up on drop (covers every
    // return path below). Never populated under --print-command (nothing spawns).
    let mut temp_files = TempPromptFiles::default();

    for (spec, unit_model, unit_prompt) in &units {
        let spec = *spec;
        // On a batch run each result records the prompt it ran (they differ);
        // on an ordinary run the single top-level `prompt` covers them all.
        let result_prompt = batch_run.then(|| unit_prompt.to_string());
        let resolved = detect::resolve(spec, &overrides);
        // Explicit format (CLI or config) always wins (and was validated above
        // when a named session is in play). Otherwise events/streaming selects the
        // harness's transcript-bearing format; absent that, the named-session
        // anchor selects its id-bearing format. Ordinary runs keep the default.
        let want_events = args.events || args.stream;
        let chosen_format = explicit_format.unwrap_or_else(|| {
            if want_events {
                spec.events_format.unwrap_or(spec.output_format)
            } else if session_anchor == Some(spec.id) {
                spec.session_format()
                    .expect("setup_session selected only a harness with a session-bearing format")
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
        // Reasoning / thinking effort is delivered in the harness's native shape,
        // resolved per harness so effort can sit next to each harness's own model
        // (CLI `--reasoning` beats config). Append-style deliveries (Claude's
        // `--effort`, Codex's `-c model_reasoning_effort=`, Copilot's
        // `--reasoning-effort`) lead `extra` so a raw `--` passthrough can still
        // override them; a model-suffix delivery (Cursor's `model[effort=…]`)
        // instead decorates the resolved model, so it needs one. A harness with no
        // reasoning surface was already refused by `validate_reasoning`, so
        // `spec.reasoning` is `Some` whenever a value is.
        let mut extra: Vec<String> = Vec::new();
        let mut plan_model = unit_model.clone();
        if let Some(value) = args
            .reasoning
            .as_deref()
            .or_else(|| cfg.reasoning_for(spec.id))
        {
            let delivery = spec.reasoning.expect(
                "validate_reasoning refused a reasoning value for a harness without delivery",
            );
            if let Some(suffix) = delivery.model_suffix(value) {
                match plan_model.as_mut() {
                    Some(m) => m.push_str(&suffix),
                    None => {
                        return Err(OneharnessError::ReasoningNeedsModel {
                            id: spec.id.to_string(),
                        });
                    }
                }
            } else {
                extra.extend(delivery.args(value));
            }
        }
        extra.extend(cfg.args_for(spec.id).iter().cloned());
        extra.extend(args.passthrough.iter().cloned());
        if let Some(wiring) = &mock_wiring {
            extra.extend(wiring.extra_args_for(spec.id));
        }
        let mut harness_plan = HarnessPlan {
            spec,
            bin: resolved.bin.clone(),
            // The model the argv carries: the unit's resolved model (a fan-out
            // model, the batch's single model, or the harness's own `model_for`
            // — per-harness `[harness.<id>]` beating the top-level), decorated
            // with a reasoning suffix above for a model-suffix harness (cursor).
            // The *recorded* model (in the result) stays the plain `unit_model`.
            model: plan_model,
            system: system.map(str::to_string),
            // A `--session` continue supplies the native token to resume with,
            // reusing the harness's verified `--resume` mapping; a create (or no
            // session) leaves it to the explicit `--resume` value (they are
            // mutually exclusive, so at most one is `Some`). The session token is
            // scoped to the anchor harness: in fallback the chain holds several
            // harnesses, but a native token belongs to exactly one of them, so a
            // *different* candidate that ends up winning must never be handed it
            // (it would resume the wrong harness with a foreign id). In parallel
            // the anchor is the only harness, so this filter is a no-op there.
            resume: session_resume
                .clone()
                .filter(|_| session_anchor == Some(spec.id))
                .or_else(|| resume.map(str::to_string)),
            fork: args.fork,
            mode,
            output_format,
            native,
            base_prompt: unit_prompt.to_string(),
            extra,
            system_file: None,
            prompt_stdin: false,
        };

        if args.print_command {
            // --print-command never spawns, so nothing is materialized off-argv:
            // the printed command is the deterministic inline form (large prompts
            // that would actually run via file/stdin are shown inline).
            plan.push(Plan::Ready(Box::new(planned_result(
                spec,
                &resolved.bin,
                resolved.available,
                harness_plan.build(schema.as_ref(), None).argv,
                output_format,
                result_prompt,
                unit_model.clone(),
            ))));
        } else if !resolved.available {
            plan.push(Plan::Ready(Box::new(skipped_result(
                spec,
                &resolved.bin,
                harness_plan.build(schema.as_ref(), None).argv,
                output_format,
                result_prompt,
                unit_model.clone(),
            ))));
        } else {
            let job_index = jobs.len();
            // Large prompt / system: deliver it off the argv (temp file / stdin)
            // where the harness supports it, so it never trips the OS argv ceiling.
            // Mutates `harness_plan` (so the structured-output retry rebuilds the
            // same delivery) and may write a temp file for the system prompt.
            plan_large_input(&mut harness_plan, spec, system, job_index, &mut temp_files)?;
            let built = harness_plan.build(schema.as_ref(), None);
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
                argv: built.argv,
                cwd: args.cwd.clone(),
                env: job_env,
                timeout: Duration::from_secs(timeout),
                stdin: built.stdin,
            });
            plan.push(Plan::Pending {
                spec,
                bin: resolved.bin,
                output_format,
                job_index,
                prompt: result_prompt,
                model: unit_model.clone(),
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
                model,
            } => stream_one_harness(&jobs[job_index], spec, &bin, output_format, prompt, model),
        };
        record_history(
            &history_writer,
            mode,
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
            report_models.clone(),
            args,
            mode,
            schema.as_ref(),
            max_retries,
            None,
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

    // Schedule and run the jobs. Parallel mode runs every job at once (an
    // ordinary run and a batch under `speed`), or a batch's warm-then-fan waves
    // under `min-tokens`. Fallback mode instead drives the harnesses one at a
    // time in priority order, stopping at the first that actually runs — so it
    // never uses the wave scheduler. `--print-command` never executes, so it
    // always takes the parallel branch (which emits the planned rows).
    let mut forked = false;
    let (results, fallback_report): (Vec<RunResult>, Option<FallbackReport>) = if fallback_mode
        && !args.print_command
    {
        // Sequential fallback: run the priority chain until one harness runs.
        // The workspace-restoring mock finish happens below, after every spawn
        // this branch does is complete.
        let (results, fb) = run_fallback(
            plan,
            &jobs,
            &job_plans,
            schema.as_ref(),
            max_retries,
            multi_model,
        );
        (results, Some(fb))
    } else {
        let max_parallel = args
            .max_parallel
            .or(cfg.max_parallel)
            .unwrap_or(jobs.len().max(1));
        // Fork-based `min-tokens`: when a batch's single harness can fork, the
        // warm-up (prompt[0]) establishes a session and the fan-out branches
        // forks of it, so each reuses the warmed cached prefix — the realizable
        // token saving on these CLIs (a static --system is re-created per
        // process, so plain warm-then-fan saves nothing). It needs the warm-up's
        // *runtime* session id, so it cannot run under --print-command.
        let fork_batch = batch_run
            && batch_strategy == BatchStrategy::MinTokens
            && specs[0].fork_reuses_cache
            && !args.print_command
            && !jobs.is_empty();
        // `min-tokens` reduces tokens only when the harness has a *cache-reusing*
        // fork (the warm-up writes the shared prefix, the forked fan-out reads
        // it). When it does not — no fork at all, or a fork that re-sends the
        // prefix cold, like OpenCode — `min-tokens` can only order the calls; say
        // so rather than imply a saving the harness can't deliver.
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
        let outcomes = if fork_batch {
            let o = run_fork_batch(
                &mut jobs,
                &mut job_plans,
                schema.as_ref(),
                max_retries,
                max_parallel,
            );
            // The fan-out actually forked iff the warm-up exposed a session to
            // branch (run_fork_batch sets the fan-out plans' `resume` only then).
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
                    model,
                } => {
                    let outcome = &outcomes[job_index];
                    // The argv actually run (fork-batch rewrites the fan-out jobs
                    // to resume+fork the warmed session, so read it back).
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
                        model,
                    )
                }
            })
            .collect();
        (results, None)
    };

    // Every job is done (or nothing ran under --print-command, where mock
    // wiring is refused by clap): put the workspace back before anything else
    // can fail, so a later I/O error never leaves the ephemeral hook behind.
    let mock_report = mock_wiring.map(MockWiring::finish);

    record_history(&history_writer, mode, &prompts[0], &results);
    // Persist the captured session token (if `--session` was in play) and build
    // its report block. A session run is single-harness, so `results` holds one.
    let session_report = finalize_session(session_wiring, &results, args.print_command);

    if let Some(dir) = &args.output_dir {
        write_output_dir(dir, &results)?;
    }

    // Fallback has its own success rule: a hard failure only when NO candidate
    // could run (nothing executed), else the outcome of the one harness that ran
    // — the fallen-through candidates never count against it. Parallel mode keeps
    // the "any harness failed" rule.
    let exit = match &fallback_report {
        Some(fb) => fallback_exit(&results, fb.ran.is_some()),
        None => exit_code(&results, require_available),
    };

    let report = build_report(
        results,
        &prompts,
        model,
        report_models,
        args,
        mode,
        schema.as_ref(),
        max_retries,
        batch_run.then_some(BatchReport {
            strategy: batch_strategy,
            prompt_count: prompts.len(),
            forked,
        }),
        fallback_report,
        loaded.files,
        mock_report,
        history_file,
        session_report,
    );
    print_json(&report, args.compact)?;

    if exit != EXIT_OK {
        match &report.fallback {
            // Fallback where nothing could run: every candidate failed to start.
            Some(fb) if fb.ran.is_none() => {
                let chain = fb
                    .fell_through
                    .iter()
                    .map(|f| format!("{} [{}]", f.harness, f.reason))
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!(
                    "oneharness: no selected harness could be run — all {} fallback candidate(s) \
                     failed to start ({chain}); nothing executed",
                    fb.fell_through.len()
                );
            }
            // Fallback where a harness ran but its task failed.
            Some(fb) => eprintln!(
                "oneharness: fallback harness `{}` ran but did not succeed (see results[].status \
                 and results[].error)",
                fb.ran.as_deref().unwrap_or_default()
            ),
            None => {
                let failed = report
                    .results
                    .iter()
                    .filter(|r| {
                        is_failure(
                            r.status,
                            r.available,
                            require_available,
                            r.schema_valid,
                            r.failure_kind,
                        )
                    })
                    .count();
                eprintln!(
                    "oneharness: {failed}/{} harness run(s) did not succeed (see results[].status and results[].error)",
                    report.results.len()
                );
            }
        }
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
    let rules_str = rules_abs.as_deref().map(embed).transpose()?;
    let spy_str = spy_abs.as_deref().map(embed).transpose()?;

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
    models: Option<Vec<String>>,
    args: &RunArgs,
    mode: PermissionMode,
    schema: Option<&Schema>,
    max_retries: u32,
    batch: Option<BatchReport>,
    fallback: Option<FallbackReport>,
    config_files: Vec<String>,
    mock: Option<MockReport>,
    history_file: Option<String>,
    session: Option<SessionReport>,
) -> RunReport {
    RunReport {
        schema_version: SCHEMA_VERSION.to_string(),
        oneharness_version: env!("CARGO_PKG_VERSION").to_string(),
        // On a batch run the per-result `prompt` is authoritative; the top-level
        // field repeats the first prompt for back-compat (it is always present).
        prompt: prompts[0].clone(),
        // The effective top-level model (first fan-out model, else the single
        // CLI/config model); each result's own `model` is authoritative.
        model: model.map(str::to_string),
        // The model fan-out list, present only on a multi-model run.
        models,
        resume: args.resume.clone(),
        fork: args.fork,
        session,
        permission_mode: mode,
        bypass_permissions: mode.is_bypass(),
        dry_run: args.print_command,
        schema: schema.map(|s| s.as_value().clone()),
        schema_max_retries: schema.map(|_| max_retries),
        batch,
        fallback,
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

/// The comma-joined ids of every session-capable harness, for the "supported:"
/// hint on a `--session` capability error.
fn session_capable_ids() -> String {
    harness::all()
        .iter()
        .filter(|s| s.session_capable())
        .map(|s| s.id)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate and resolve a `--session <name>` request against the store, or
/// `Ok(None)` when the flag was not passed. Loud usage errors up front (nothing
/// spawns): a batch run, a harness that exposes no session id (`session_capable`),
/// an unresolvable store directory, or a name already bound to a different harness.
/// On success the returned plan says whether to create fresh or continue a stored
/// token.
///
/// Which harness the session binds to depends on the run mode:
///
/// * **Parallel** — the run is single-harness by contract, so more than one
///   selected harness makes a single session name ambiguous
///   ([`OneharnessError::SessionMultipleHarnesses`]).
/// * **Fallback** — the whole priority chain is selected, but exactly ONE harness
///   ultimately runs (fallback stops at the first that runs), so `--session` is
///   allowed. It binds to the *anchor*: the first session-capable harness in the
///   chain. Fallback deterministically settles on the same harness across repeated
///   runs given stable availability, so the anchor is the harness the session
///   naturally lives on. A chain with no session-capable harness at all cannot
///   carry a named handle ([`OneharnessError::SessionUnsupported`]).
///
/// The anchor's native token is applied to the anchor's argv *only* (see the job
/// loop's `session_anchor` filter) and captured from the anchor's own result only
/// (see [`finalize_session`]), so a transient fall-through to a different harness
/// never resumes it with the anchor's token.
fn setup_session(
    args: &RunArgs,
    specs: &[&'static HarnessSpec],
    batch_run: bool,
    fallback_mode: bool,
    project: &std::path::Path,
) -> Result<Option<SessionWiring>, OneharnessError> {
    let Some(name) = args.session.as_deref() else {
        return Ok(None);
    };
    if batch_run {
        return Err(OneharnessError::SessionBatch);
    }
    let spec = if fallback_mode {
        // Bind to the first session-capable harness in the priority chain; a chain
        // with none cannot carry a named handle (list the whole selection in the
        // error, since no single harness is the offender).
        specs
            .iter()
            .copied()
            .find(|s| s.session_capable())
            .ok_or_else(|| OneharnessError::SessionUnsupported {
                id: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
                supported: session_capable_ids(),
            })?
    } else {
        if specs.len() != 1 {
            return Err(OneharnessError::SessionMultipleHarnesses {
                count: specs.len(),
                selected: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
            });
        }
        let spec = specs[0];
        if !spec.session_capable() {
            return Err(OneharnessError::SessionUnsupported {
                id: spec.id.to_string(),
                supported: session_capable_ids(),
            });
        }
        spec
    };
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

/// Refuse a caller-pinned output format that cannot carry the named session's
/// native id. `None` means the normal automatic selection remains in force.
/// Only the session anchor is checked: in fallback mode the other candidates do
/// not own this named session and must retain their ordinary format behavior.
fn validate_session_output_format(
    session_anchor: Option<&str>,
    explicit_format: Option<OutputFormat>,
) -> Result<(), OneharnessError> {
    let (Some(id), Some(format)) = (session_anchor, explicit_format) else {
        return Ok(());
    };
    let spec = harness::by_id(id).expect("session anchor came from the harness registry");
    if spec.format_carries_session(format) {
        return Ok(());
    }
    Err(OneharnessError::SessionOutputFormat {
        id: id.to_string(),
        format: format.as_str().to_string(),
        supported: spec
            .session_formats
            .iter()
            .map(|format| format.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    })
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
    // Capture the token from the *anchor* harness's own result — the harness the
    // session is bound to (`wiring.harness`). In parallel single-harness mode the
    // anchor is the only result, so this is exactly `results.first()`. In fallback
    // `results` lists every *attempted* harness in priority order (fell-through
    // candidates first), so `results.first()` would be a candidate that ran
    // nothing and exposed no id; find the anchor's result instead. If the anchor
    // never ran (it fell through, or a non-session-capable harness earlier in the
    // chain won), there is no new token: nothing is captured, the existing stored
    // token is preserved, and the no-id warning below fires — never a wrong token.
    let captured = results
        .iter()
        .find(|r| r.harness == wiring.harness)
        .and_then(|r| r.session_id.clone());
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
) -> Result<Option<HistoryWriter>, OneharnessError> {
    let cli_labels = oneharness_core::domain::history::parse_labels(
        args.history_label.iter().map(String::as_str),
    )
    .map_err(OneharnessError::HistoryLabelInvalid)?;
    let mut labels = cfg.history_labels.clone().unwrap_or_default();
    labels.extend(&cli_labels);
    if args.print_command {
        return Ok(None);
    }
    let enabled = if args.history {
        true
    } else if args.no_history {
        false
    } else {
        cfg.history.unwrap_or(false)
    };
    if !enabled {
        return Ok(None);
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
        return Ok(None);
    };
    let name = args.history_name.clone().unwrap_or_else(|| {
        oneharness_core::domain::history::session_name(
            prompts.first().map(String::as_str).unwrap_or(""),
        )
    });
    match HistoryWriter::open(&dir, project_start, &name, labels) {
        Ok(writer) => Ok(Some(writer)),
        Err(err) => {
            eprintln!(
                "oneharness: warning: could not open a history file under `{}`: {err}; \
                 skipping history for this run",
                dir.display()
            );
            Ok(None)
        }
    }
}

/// Append each finished result to the session's history file, if history is on.
/// Best-effort per record: a write failure warns and moves on (the run's stdout
/// report is authoritative; history is a side channel). Each record carries the
/// result's own `model`, so a model fan-out records the model each harness ran.
fn record_history(
    writer: &Option<HistoryWriter>,
    mode: PermissionMode,
    run_prompt: &str,
    results: &[RunResult],
) {
    let Some(writer) = writer else { return };
    for r in results {
        if let Err(err) = writer.append(mode, r.model.as_deref(), run_prompt, r) {
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
    model: Option<String>,
) -> RunResult {
    use oneharness_core::domain::report::RunStreamEnvelope;
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
            let envelope = RunStreamEnvelope::Event { event: ev.clone() };
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
        model,
    )
}

/// Write the terminal `{"type":"result","report":<RunReport>}` line that closes a
/// streaming run — the same envelope a non-streaming run emits, so a consumer
/// that ignored the incremental events still gets the full report. A broken pipe
/// (the consumer already short-circuited and left) is not an error.
fn emit_stream_result(report: &RunReport) -> Result<(), OneharnessError> {
    use oneharness_core::domain::report::RunStreamEnvelope;
    use std::io::Write;
    let line = serde_json::to_string(&RunStreamEnvelope::Result {
        report: report.clone(),
    })?;
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
                plan.system_file = None; // ...so drop any off-argv system delivery too
            }
            for (i, job) in jobs.iter_mut().enumerate().skip(1) {
                let built = job_plans[i].build(schema, None);
                job.argv = built.argv;
                job.stdin = built.stdin;
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

/// Refuse the run shapes fallback mode cannot express, before anything spawns.
/// Fallback drives several harnesses in priority order for one prompt, stopping
/// at the first that runs — so a multi-prompt batch, the explicit `--resume` /
/// `--fork` continuations (each pins one *specific* harness's native id), and
/// `--stream` are loud usage errors here. `--session` is *not* refused: the
/// higher-level named handle binds to the anchor (the first session-capable
/// harness in the chain), which fallback settles on under stable availability —
/// see [`setup_session`], which does the capability check for it. The *capability*
/// validation of the requested features against every listed harness stays in the
/// shared validators (`validate_modes`, `setup_mock`, …), which run over all specs
/// regardless of mode — so a flag no candidate could honor still fails fast even
/// though only one harness will run.
fn validate_fallback(batch_run: bool, args: &RunArgs) -> Result<(), OneharnessError> {
    let conflict = |with, why| Err(OneharnessError::FallbackConflict { with, why });
    if batch_run {
        return conflict(
            "a batch run (more than one prompt)",
            "fallback tries harnesses in order for one prompt; a batch fans one harness over many prompts",
        );
    }
    if args.resume.is_some() || args.fork {
        return conflict(
            "--resume/--fork",
            "a resumed session belongs to one specific harness, so it cannot fall through to another (use --session, which binds to the fallback anchor)",
        );
    }
    if args.stream {
        return conflict(
            "--stream",
            "streaming drives a single harness incrementally; a fallback chain may run several in turn",
        );
    }
    Ok(())
}

/// Refuse the run shapes a model fan-out cannot express, before anything spawns.
/// Fanning over models multiplies the run into several (harness, model) units, so
/// every single-unit shape is a loud usage error: a batch (its shared cache prefix
/// is per harness/model, so it cannot also vary the model), and each single-harness
/// continuation — `--resume` / `--fork` / `--session` (bound to one model context)
/// and `--stream` (one incremental output). `--run-mode fallback` is deliberately
/// *not* refused: the model list is exactly the fallback chain there.
fn validate_multi_model(batch_run: bool, args: &RunArgs) -> Result<(), OneharnessError> {
    let conflict = |with, why| Err(OneharnessError::MultiModelConflict { with, why });
    if batch_run {
        return conflict(
            "a batch run (more than one prompt)",
            "a batch shares one cacheable prefix, which is per harness/model — fan out over models or over prompts, not both",
        );
    }
    if args.resume.is_some() || args.fork {
        return conflict(
            "--resume/--fork",
            "a resumed session is tied to one model, so it cannot fan out over several",
        );
    }
    if args.session.is_some() {
        return conflict(
            "--session",
            "a named session is tied to one model, so it cannot fan out over several",
        );
    }
    if args.stream {
        return conflict(
            "--stream",
            "streaming emits one incremental output; a model fan-out produces several results",
        );
    }
    Ok(())
}

/// Order the selected specs into the fallback priority chain. When the caller
/// named harnesses (`--harness` / config `harnesses`), that order *is* the
/// priority; under `--all` (no explicit list) the registry order `select_specs`
/// already returns is the priority. Every id in `include` is present in `specs`
/// (both are validated the same way) and duplicates collapse to first mention.
fn fallback_order(
    specs: Vec<&'static HarnessSpec>,
    include: &[String],
) -> Vec<&'static HarnessSpec> {
    if include.is_empty() {
        return specs;
    }
    let mut ordered: Vec<&'static HarnessSpec> = Vec::with_capacity(specs.len());
    for id in include {
        if let Some(spec) = specs.iter().find(|s| s.id == id.as_str()) {
            if !ordered.iter().any(|o| o.id == spec.id) {
                ordered.push(*spec);
            }
        }
    }
    ordered
}

/// Run a single harness job under the structured-output retry loop — the
/// one-harness analogue of a [`run_in_waves`] wave of size one — returning its
/// outcome. Used by the fallback driver, which spawns harnesses one at a time.
fn run_one_job(
    job: &Job,
    plan: &HarnessPlan,
    schema: Option<&Schema>,
    max_retries: u32,
) -> Outcome {
    let jobs = std::slice::from_ref(job);
    let outs = match schema {
        Some(sch) => runner::run_jobs_with(jobs, 1, |_, attempt, capture| {
            retry_decision(plan, sch, attempt, max_retries, capture)
        }),
        None => runner::run_jobs_with(jobs, 1, |_, _, _| None),
    };
    outs.into_iter().next().expect("one job, one outcome")
}

/// Drive the selected harnesses in priority order, stopping at the first that
/// actually runs the task (success OR real failure). A harness that could not run
/// at all — not installed, unspawnable, or rejected before doing any work (auth /
/// quota) — is fallen through to the next. Returns one result per *attempted*
/// harness (the fallen-through ones in order, then the one that ran), plus the
/// fallback report; candidates after the one that ran are never spawned and do
/// not appear. `plan`/`jobs`/`job_plans` are the same structures the parallel
/// path builds — a `Ready` entry is an already-resolved (here, always `Skipped`)
/// row, a `Pending` entry carries a job to spawn.
fn run_fallback(
    plan: Vec<Plan>,
    jobs: &[Job],
    job_plans: &[HarnessPlan],
    schema: Option<&Schema>,
    max_retries: u32,
    multi_model: bool,
) -> (Vec<RunResult>, FallbackReport) {
    let mut results: Vec<RunResult> = Vec::new();
    let mut fell_through: Vec<FallThrough> = Vec::new();
    let mut ran: Option<String> = None;
    for entry in plan {
        let result = match entry {
            Plan::Ready(result) => *result,
            Plan::Pending {
                spec,
                bin,
                output_format,
                job_index,
                prompt,
                model,
            } => {
                let outcome =
                    run_one_job(&jobs[job_index], &job_plans[job_index], schema, max_retries);
                let command = jobs[job_index].argv.clone();
                executed_result(
                    spec,
                    bin,
                    command,
                    output_format,
                    &outcome.capture,
                    schema,
                    outcome.attempts,
                    prompt,
                    model,
                )
            }
        };
        match fallback::startup_failure_reason(result.status, result.failure_kind, multi_model) {
            // Could not run — record why and try the next candidate.
            Some(reason) => {
                fell_through.push(FallThrough {
                    harness: result.harness.clone(),
                    reason: reason.to_string(),
                });
                results.push(result);
            }
            // Actually ran (well or badly) — this is the answer; stop here.
            None => {
                ran = Some(result.harness.clone());
                results.push(result);
                break;
            }
        }
    }
    (results, FallbackReport { ran, fell_through })
}

/// Exit code for a fallback run: a hard failure when no candidate could run at
/// all (nothing executed), otherwise the success of the one harness that ran —
/// which is always the last result. A real task failure or timeout there is a
/// failure; the fallen-through candidates never count against it, which is the
/// whole point of the mode. `require_available` does not apply (a missing harness
/// is the expected, tolerated case in fallback).
fn fallback_exit(results: &[RunResult], ran: bool) -> i32 {
    if !ran {
        return EXIT_FAILURE;
    }
    let last = results.last().expect("a harness ran, so there is a result");
    if is_failure(
        last.status,
        last.available,
        false,
        last.schema_valid,
        last.failure_kind,
    ) {
        EXIT_FAILURE
    } else {
        EXIT_OK
    }
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
        /// The model this unit ran with (fan-out / per-harness); `None` when the
        /// harness used its own default.
        model: Option<String>,
    },
}

fn planned_result(
    spec: &HarnessSpec,
    bin: &str,
    available: bool,
    command: Vec<String>,
    output_format: OutputFormat,
    prompt: Option<String>,
    model: Option<String>,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available,
        status: Status::Planned,
        prompt,
        model,
        exit_code: None,
        duration_ms: None,
        telemetry: None,
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
    model: Option<String>,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available: false,
        status: Status::Skipped,
        prompt,
        model,
        exit_code: None,
        duration_ms: None,
        telemetry: None,
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
    model: Option<String>,
) -> RunResult {
    // A timeout does not invalidate bytes already captured from the process.
    // Normalize them best-effort exactly like an exited run. SpawnError retains
    // its existing null-signal semantics because a failed wait cannot establish
    // that its captured output is complete or trustworthy.
    let normalize_capture = matches!(
        capture.status,
        Status::Ok | Status::Nonzero | Status::Timeout
    );
    let extracted = normalize_capture
        .then(|| normalize::extract(&capture.stdout, output_format))
        .flatten();
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
    let usage_reading = normalize_capture
        .then(|| signals::extract_usage(&capture.stdout))
        .flatten();
    let (mut usage, usage_source) = match usage_reading {
        Some(r) => (r.usage, Some(r.source)),
        None => (Usage::default(), None),
    };
    signals::apply_model_price(&mut usage, model.as_deref());
    let session_id = normalize_capture
        .then(|| signals::extract_session(&capture.stdout))
        .flatten();
    let events_reading = normalize_capture
        .then(|| events::extract_events(&capture.stdout, output_format))
        .flatten();
    let (mut normalized_events, events_source) = match events_reading {
        Some(r) => (Some(r.events), Some(r.source)),
        None => (None, None),
    };
    let timing = normalized_events.as_mut().map(|normalized_events| {
        events::apply_observed_timing(
            normalized_events,
            &capture.stdout_observations,
            capture.status,
            capture.duration_ms,
        )
    });
    let trace_is_complete = spec.events_format.unwrap_or(spec.output_format) == output_format;
    let telemetry = Some(oneharness_core::domain::report::ExecutionTelemetry {
        started_at: capture.started_at.clone(),
        finished_at: capture.finished_at.clone(),
        model_ms: timing
            .as_ref()
            .and_then(|timing| timing.model_ms)
            .or_else(|| {
                capture
                    .stdout_observations
                    .last()
                    .map(|observation| observation.offset_ms)
            }),
        tool_ms: timing
            .as_ref()
            .and_then(|timing| timing.tool_ms)
            .or_else(|| trace_is_complete.then_some(0)),
        time_to_first_token_ms: timing.and_then(|timing| timing.time_to_first_token_ms),
    });
    // A deferred-tool dead-end: the harness completed cleanly (exit 0) but only
    // *deferred* a builtin tool call instead of running it (Claude Code bridge
    // deployments — issue #1114). It exits 0, so it is not caught by the non-zero
    // classification below; detect it from the output shape and give it a distinct
    // `tool_deferred` kind + an actionable `error`, rather than letting it look
    // like an empty/invalid answer. Checked for any run that produced output.
    let deferred = match capture.status {
        Status::Ok | Status::Nonzero => signals::detect_deferred_tool(&capture.stdout),
        _ => None,
    };
    // Classify only an actual non-zero run: timeouts/spawn failures already carry
    // a oneharness-generated `error`, and `status` explains them. A detected
    // deferral is more specific and actionable, so it wins over a coarse match.
    let failure = match (&deferred, capture.status) {
        (Some(_), _) => Some(signals::FailureReading {
            kind: signals::FailureKind::ToolDeferred,
            source: "stdout".to_string(),
        }),
        (None, Status::Nonzero) => signals::classify_failure(&capture.stdout, &capture.stderr),
        (None, _) => None,
    };
    let (failure_kind, failure_kind_source) = match failure {
        Some(f) => (Some(f.kind), Some(f.source)),
        None => (None, None),
    };
    // A deferral produced no answer, so surface an actionable `error` in place of
    // the harness's (absent) one — even though the process exited 0.
    let error = match &deferred {
        Some(d) => Some(deferred_tool_error(spec.id, d.tool.as_deref())),
        None => capture.error.clone(),
    };
    RunResult {
        harness: spec.id.to_string(),
        bin,
        available: true,
        status: capture.status,
        prompt,
        model,
        exit_code: capture.exit_code,
        duration_ms: capture.duration_ms,
        telemetry,
        command,
        output_format,
        text,
        text_source,
        usage,
        usage_source,
        session_id,
        events: normalized_events,
        events_source,
        structured,
        schema_valid,
        schema_attempts,
        schema_error,
        failure_kind,
        failure_kind_source,
        stdout: capture.stdout.clone(),
        stderr: capture.stderr.clone(),
        error,
    }
}

/// The actionable `error` for a deferred-tool dead-end (issue #1114): the harness
/// deferred `tool` (when named) instead of executing it, so the run produced
/// nothing. Names the cause (a bridged/managed deployment) and the way out.
fn deferred_tool_error(harness_id: &str, tool: Option<&str>) -> String {
    let what = match tool {
        Some(name) => format!("a `{name}` tool call"),
        None => "a builtin tool call".to_string(),
    };
    format!(
        "harness `{harness_id}` deferred {what} instead of executing it, so the run produced no \
         result — you appear to be in a bridged/managed deployment where builtin tools are \
         deferred (empty `tengu_non_deferrable_builtins`). Tool-using runs need a deployment that \
         executes tools inline: run from a standalone environment/CI, or select a harness that \
         executes tools inline."
    )
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
    /// Path to a temp file holding the system prompt, when it is large enough to
    /// deliver off the argv on a harness with a system-file flag (Claude Code).
    /// `None` keeps the system inline. Set by the command layer before spawning.
    system_file: Option<String>,
    /// When true, deliver the user prompt on the child's stdin (a large prompt on
    /// a stdin-capable harness): `build` omits the positional and returns the
    /// assembled prompt as [`BuiltCommand::stdin`]. `false` keeps it on the argv.
    prompt_stdin: bool,
}

/// The result of building one attempt: the argv to spawn and, when the prompt is
/// delivered off the argv, the bytes to pipe to stdin.
struct BuiltCommand {
    argv: Vec<String>,
    stdin: Option<String>,
}

impl HarnessPlan {
    /// Build the argv (and any stdin payload) for one attempt. `schema` drives
    /// structured output: non-native harnesses get the schema instruction appended
    /// to the prompt, native ones get it on the flag. `feedback` (the prior answer
    /// + validation errors) is appended on a retry so the model can correct itself.
    ///
    /// When `prompt_stdin` is set, the assembled prompt is returned as
    /// [`BuiltCommand::stdin`] instead of riding the argv (the adapter omits the
    /// positional), with the system prompt folded in for a harness whose system
    /// rides the prompt ([`LargeInput::system_rides_prompt`]) — so the bytes the
    /// model sees are identical to the inline path.
    fn build(&self, schema: Option<&Schema>, feedback: Option<(&str, &[String])>) -> BuiltCommand {
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
        // When the prompt rides stdin, assemble the exact payload the adapter would
        // have inlined: for a harness whose system rides the prompt, prepend the
        // system (mirroring `prompt_with_system`); otherwise the system is carried
        // separately (Claude's file flag, Goose's inline `--system`).
        let stdin = if self.prompt_stdin {
            Some(if self.spec.large_input.system_rides_prompt {
                harness::prompt_with_system_text(self.system.as_deref(), &prompt)
            } else {
                prompt.clone()
            })
        } else {
            None
        };
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
            system_file: self.system_file.as_deref(),
            prompt_stdin: self.prompt_stdin,
        };
        let mut argv = (self.spec.build_argv)(&ctx);
        argv.extend(self.extra.iter().cloned());
        BuiltCommand { argv, stdin }
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
) -> Option<NextRun> {
    // Only a run that produced output can be validated; a timeout / spawn error
    // is not a validation failure and re-running it would just burn the budget.
    if !matches!(capture.status, Status::Ok | Status::Nonzero) {
        return None;
    }
    // A deferred-tool dead-end (issue #1114) is deterministic — the deployment
    // will defer again on every retry — so re-prompting only burns real model
    // calls without ever producing a value. Stop immediately; the result carries
    // the `tool_deferred` classification either way.
    if signals::detect_deferred_tool(&capture.stdout).is_some() {
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
    // raw stdout) so the correction prompt is grounded in its own output. The
    // rebuild reuses the unit's delivery (a stdin prompt re-prompts via stdin).
    let previous = match &answer {
        Some(text) if !text.is_empty() => text.clone(),
        _ => capture.stdout.trim().to_string(),
    };
    let built = plan.build(Some(schema), Some((&previous, &check.errors)));
    Some(NextRun {
        argv: built.argv,
        stdin: built.stdin,
    })
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
/// spawned, when — under `--require-available` — it was skipped as missing, when
/// a structured-output run never produced a schema-conforming answer (a run you
/// asked for JSON from and didn't get is a failure, regardless of exit code), or
/// when it dead-ended by deferring a tool call (`tool_deferred`) — a clean exit
/// that nonetheless did no useful work (issue #1114).
fn is_failure(
    status: Status,
    available: bool,
    require_available: bool,
    schema_valid: Option<bool>,
    failure_kind: Option<signals::FailureKind>,
) -> bool {
    if schema_valid == Some(false) {
        return true;
    }
    if failure_kind == Some(signals::FailureKind::ToolDeferred) {
        return true;
    }
    match status {
        Status::Nonzero | Status::Timeout | Status::SpawnError => true,
        Status::Skipped => require_available && !available,
        Status::Ok | Status::Planned => false,
    }
}

fn exit_code(results: &[RunResult], require_available: bool) -> i32 {
    let failed = results.iter().any(|r| {
        is_failure(
            r.status,
            r.available,
            require_available,
            r.schema_valid,
            r.failure_kind,
        )
    });
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

/// Refuse — before anything spawns — a reasoning/effort setting for a selected
/// harness that has no headless argv surface for it ([`HarnessSpec::reasoning`]
/// is `None`): there is nothing to deliver it through, so it is a loud usage
/// error ([`OneharnessError::ReasoningUnsupported`]) rather than a silent drop.
/// Resolves the effective value per harness exactly as the run does (CLI
/// `--reasoning` beats config `[harness.<id>]`/top-level `reasoning`), so a
/// value scoped to a capable harness never trips one that isn't selected for it.
fn validate_reasoning(
    args: &RunArgs,
    cfg: &oneharness_core::domain::config::FileConfig,
    specs: &[&'static HarnessSpec],
) -> Result<(), OneharnessError> {
    for spec in specs {
        let set = args.reasoning.is_some() || cfg.reasoning_for(spec.id).is_some();
        if set && spec.reasoning.is_none() {
            return Err(OneharnessError::ReasoningUnsupported {
                id: spec.id.to_string(),
                supported: harness::all()
                    .iter()
                    .filter(|h| h.reasoning.is_some())
                    .map(|h| h.id)
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
            model: None,
            exit_code: None,
            duration_ms: None,
            telemetry: None,
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
            system_file: None,
            prompt_stdin: false,
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
            started_at: "2026-01-01T00:00:00.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:00.001Z".to_string()),
            stdout_observations: Vec::new(),
        }
    }

    #[test]
    fn plan_large_input_warns_and_stays_inline_when_unwired() {
        // A harness with LargeInput::NONE (a future/unverified adapter) has no
        // off-argv route, so a large prompt AND system both stay inline —
        // plan_large_input changes nothing and warns on both (the honest guard
        // against a silent E2BIG). Every real harness is wired, so this synthetic
        // no-capability spec is the only witness for the warning branches.
        let big = "x".repeat(LARGE_INPUT_THRESHOLD + 1);
        let mut plan = crush_plan();
        plan.spec = unsupported_spec();
        plan.base_prompt = big.clone();
        let spec = plan.spec;
        let mut temps = TempPromptFiles::default();
        plan_large_input(&mut plan, spec, Some(&big), 0, &mut temps).unwrap();
        assert!(!plan.prompt_stdin, "no stdin route → prompt stays inline");
        assert!(
            plan.system_file.is_none(),
            "no file route → system stays inline"
        );
        assert!(temps.0.is_empty(), "nothing materialized");
    }

    #[test]
    fn plan_large_input_is_a_noop_for_small_prompts() {
        // Below the threshold, nothing moves off the argv even on a wired harness.
        let mut plan = crush_plan(); // crush is wired (prompt_stdin)
        let spec = plan.spec;
        let mut temps = TempPromptFiles::default();
        plan_large_input(&mut plan, spec, Some("small"), 0, &mut temps).unwrap();
        assert!(!plan.prompt_stdin);
        assert!(temps.0.is_empty());
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
        let argv = plan.build(Some(&schema), None).argv;
        assert!(argv.iter().all(|a| !a.contains('\n')), "{argv:?}");
        // ... and a retry with a multi-line prior answer + errors.
        let argv = plan
            .build(
                Some(&schema),
                Some((
                    "multi\nline\r\nanswer",
                    &["e1".to_string(), "e2".to_string()],
                )),
            )
            .argv;
        assert!(
            argv.iter().all(|a| !a.contains('\n') && !a.contains('\r')),
            "{argv:?}"
        );
        // Native (claude) retry: schema rides the flag, feedback rides the prompt.
        let mut native = crush_plan();
        native.spec = harness::by_id("claude-code").unwrap();
        native.native = true;
        native.output_format = OutputFormat::Json;
        let argv = native
            .build(Some(&schema), Some(("multi\nline", &["e".to_string()])))
            .argv;
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
        assert!(next.argv.iter().any(|a| a.contains("did not conform")));
        // Budget spent → stop even though still invalid.
        assert!(retry_decision(&plan, &schema, 3, 2, &capture(Status::Ok, "{}")).is_none());
        // A timeout is not a validation failure, so it is never retried.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Timeout, "")).is_none());
        // No extractable answer falls back to the raw stdout in the feedback.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, "  ")).is_some());
        // A deferred-tool dead-end is deterministic, so it is never retried even
        // with budget left (issue #1114) — re-prompting would only burn calls.
        let deferred = capture(
            Status::Ok,
            r#"{"stop_reason":"tool_deferred","result":"","deferred_tool_use":{"name":"Read"}}"#,
        );
        assert!(retry_decision(&plan, &schema, 1, 2, &deferred).is_none());
    }

    #[test]
    fn deferred_tool_error_names_the_tool_or_stays_generic() {
        // Both arms of the actionable message: a named tool is quoted; an unnamed
        // deferral falls back to "a builtin tool call" (never a fabricated name).
        let named = deferred_tool_error("claude-code", Some("Read"));
        assert!(named.contains("`Read`"), "{named}");
        assert!(named.contains("inline"), "actionable: {named}");
        let generic = deferred_tool_error("claude-code", None);
        assert!(generic.contains("a builtin tool call"), "{generic}");
        assert!(!generic.contains("``"), "no empty backtick pair: {generic}");
    }

    #[test]
    fn is_failure_treats_tool_deferred_as_failure_on_a_clean_exit() {
        // A tool_deferred run exits 0 (Status::Ok), which is normally a success;
        // the typed signal is what makes it fail (so exit_code / fallback_exit see
        // the dead-end). Without the signal the same ok run is a success.
        assert!(is_failure(
            Status::Ok,
            true,
            false,
            None,
            Some(signals::FailureKind::ToolDeferred)
        ));
        assert!(!is_failure(
            Status::Ok,
            true,
            false,
            None,
            Some(signals::FailureKind::Auth)
        ));
        assert!(!is_failure(Status::Ok, true, false, None, None));
    }

    #[test]
    fn executed_result_classifies_a_deferred_dead_end() {
        // A clean (exit 0) capture that only deferred a tool becomes a
        // tool_deferred result with an actionable error — not an empty answer.
        let spec = harness::by_id("claude-code").unwrap();
        let cap = capture(
            Status::Ok,
            r#"{"type":"result","stop_reason":"tool_deferred","result":"",
               "deferred_tool_use":{"name":"Read"}}"#,
        );
        let r = executed_result(
            spec,
            "claude".into(),
            vec!["claude".into()],
            OutputFormat::Json,
            &cap,
            None,
            1,
            None,
            None,
        );
        assert_eq!(r.status, Status::Ok);
        assert_eq!(r.failure_kind, Some(signals::FailureKind::ToolDeferred));
        assert_eq!(r.failure_kind_source.as_deref(), Some("stdout"));
        assert!(r.error.as_deref().unwrap().contains("`Read`"));
    }

    #[test]
    fn executed_result_normalizes_a_partial_timeout_transcript() {
        // A timeout is still a timeout, but every complete JSONL record captured
        // before the deadline remains usable. The truncated tail is ignored by
        // each best-effort extractor rather than invalidating earlier evidence.
        let transcript = concat!(
            "{\"type\":\"text\",\"sessionID\":\"ses-timeout\",\"part\":",
            "{\"type\":\"text\",\"text\":\"partial answer\"}}\n",
            "{\"type\":\"tool_use\",\"sessionID\":\"ses-timeout\",\"part\":",
            "{\"type\":\"tool\",\"tool\":\"bash\",\"state\":",
            "{\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\"}}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses-timeout\",\"part\":",
            "{\"cost\":0.01,\"tokens\":{\"input\":12,\"output\":3,",
            "\"cache\":{\"read\":9,\"write\":4}}}}\n",
            "{\"type\":\"task_complete\",\"text\":\"emitted before exit\"}\n",
            "{\"type\":\"incomplete\"",
        );
        let cap = capture(Status::Timeout, transcript);
        let r = executed_result(
            harness::by_id("opencode").unwrap(),
            "opencode".into(),
            vec!["opencode".into()],
            OutputFormat::Json,
            &cap,
            None,
            1,
            None,
            None,
        );

        assert_eq!(r.status, Status::Timeout);
        assert_eq!(r.text.as_deref(), Some("partial answer"));
        assert_eq!(r.text_source.as_deref(), Some("json:opencode-parts"));
        assert_eq!(r.usage.input_tokens, Some(12));
        assert_eq!(r.usage.output_tokens, Some(3));
        assert_eq!(r.usage.cache_read_tokens, Some(9));
        assert_eq!(r.usage.cache_write_tokens, Some(4));
        assert_eq!(r.usage.cost_usd, Some(0.01));
        assert_eq!(r.session_id.as_deref(), Some("ses-timeout"));
        let events = r.events.expect("the complete tool event survives");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_deref(), Some("bash"));
    }

    #[test]
    fn executed_spawn_error_keeps_null_signals_even_with_output() {
        let transcript = concat!(
            "{\"type\":\"text\",\"sessionID\":\"ses-untrusted\",\"part\":",
            "{\"type\":\"text\",\"text\":\"untrusted answer\"}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses-untrusted\",\"part\":",
            "{\"cost\":0.01,\"tokens\":{\"input\":12,\"output\":3}}}\n",
        );
        let cap = capture(Status::SpawnError, transcript);
        let r = executed_result(
            harness::by_id("opencode").unwrap(),
            "opencode".into(),
            vec!["opencode".into()],
            OutputFormat::Json,
            &cap,
            None,
            1,
            None,
            None,
        );
        assert!(r.text.is_none());
        assert_eq!(r.usage, Usage::default());
        assert!(r.session_id.is_none());
        assert!(r.events.is_none());
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
            Some("sonnet".into()),
        );
        assert_eq!(r.schema_valid, Some(true));
        assert_eq!(r.schema_attempts, Some(1));
        assert!(r.structured.is_some());
        // The per-result prompt and model are carried through verbatim.
        assert_eq!(r.prompt.as_deref(), Some("the batch prompt"));
        assert_eq!(r.model.as_deref(), Some("sonnet"));
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
            None,
        );
        assert!(r.schema_valid.is_none());
        assert_eq!(r.schema_attempts, Some(1));
        // No prompt / model recorded on an ordinary (non-batch, no-model) result.
        assert!(r.prompt.is_none());
        assert!(r.model.is_none());
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
            session_formats: &[],
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
            large_input: oneharness_core::domain::harness::LargeInput::NONE,
            modes: &[],
            reasoning: None,
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

    /// A `RunArgs` carrying a `--session <name>` request pointed at an isolated,
    /// non-existent store directory. `resolve_dir` returns the path verbatim and
    /// the (absent) record reads back as `None`, so `setup_session` resolves a
    /// fresh *create* without touching a real store — enough to assert which
    /// harness the session anchors to.
    fn session_args(name: &str) -> RunArgs {
        let mut a = run_args();
        a.session = Some(name.to_string());
        a.session_dir = Some(std::env::temp_dir().join("oh-unit-no-such-session-store"));
        a
    }

    #[test]
    fn setup_session_in_fallback_anchors_to_first_session_capable() {
        // A fallback chain with more than one harness is ACCEPTED (unlike parallel)
        // and the session binds to the first *session-capable* harness in priority
        // order — here `codex`, skipping the non-capable `goose` ahead of it.
        let goose = harness::by_id("goose").unwrap();
        let codex = harness::by_id("codex").unwrap();
        let claude = harness::by_id("claude-code").unwrap();
        let wiring = setup_session(
            &session_args("greet"),
            &[goose, codex, claude],
            false,
            true,
            std::path::Path::new("/proj"),
        )
        .expect("fallback + multi-harness --session is allowed")
        .expect("a wiring is returned when --session is set");
        assert_eq!(wiring.harness, "codex");
        // A fresh store means a create plan (no token to resume).
        assert_eq!(wiring.plan.phase, session::SessionPhase::Create);
        assert!(wiring.plan.resume_token.is_none());
    }

    #[test]
    fn setup_session_parallel_still_rejects_multiple_harnesses() {
        // Parallel mode is single-harness by contract: more than one selected
        // harness makes a single session name ambiguous, exactly as before.
        let claude = harness::by_id("claude-code").unwrap();
        let codex = harness::by_id("codex").unwrap();
        assert!(matches!(
            setup_session(
                &session_args("x"),
                &[claude, codex],
                false,
                false,
                std::path::Path::new("/proj"),
            ),
            Err(OneharnessError::SessionMultipleHarnesses { count: 2, .. })
        ));
    }

    #[test]
    fn setup_session_fallback_with_no_session_capable_harness_rejects() {
        // A fallback chain where NO harness exposes a session id headlessly cannot
        // carry a named handle — a loud SessionUnsupported, never a silent fresh
        // start on whichever harness happens to win.
        let goose = harness::by_id("goose").unwrap();
        let crush = harness::by_id("crush").unwrap();
        assert!(matches!(
            setup_session(
                &session_args("x"),
                &[goose, crush],
                false,
                true,
                std::path::Path::new("/proj"),
            ),
            Err(OneharnessError::SessionUnsupported { .. })
        ));
    }
}
