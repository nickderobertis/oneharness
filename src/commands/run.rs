//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use oneharness_core::domain::batch::{self, BatchStrategy};
use oneharness_core::domain::harness::{self, BuildCtx, HarnessSpec};
use oneharness_core::domain::mode::{ModeHeadless, PermissionMode};
use oneharness_core::domain::report::{
    BatchReport, Capture, OutputFormat, RunReport, RunResult, Status, SCHEMA_VERSION,
};
use oneharness_core::domain::signals::Usage;
use oneharness_core::domain::structured::{self, Schema};
use oneharness_core::domain::{events, normalize, signals};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::detect::{self, BinOverrides};
use oneharness_core::io::runner::{self, Job, Outcome};

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
    let cli_env = parse_env(&args.env)?;
    let model = args.model.as_deref().or(cfg.model.as_deref());
    let system = args.system.as_deref().or(cfg.system.as_deref());
    let timeout = args.timeout.or(cfg.timeout).unwrap_or(120);
    let require_available = args.require_available || cfg.require_available.unwrap_or(false);

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
        let chosen_format = args
            .output_format
            .or(cfg.output_format)
            .unwrap_or(spec.output_format);
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
            resume: resume.map(str::to_string),
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

    if let Some(dir) = &args.output_dir {
        write_output_dir(dir, &results)?;
    }

    let exit = exit_code(&results, require_available);

    let report = RunReport {
        schema_version: SCHEMA_VERSION,
        oneharness_version: env!("CARGO_PKG_VERSION"),
        // On a batch run the per-result `prompt` is authoritative; the top-level
        // field repeats the first prompt for back-compat (it is always present).
        prompt: prompts[0].clone(),
        // The effective top-level model (CLI, else config); a per-harness
        // config model is visible in that result's `command`.
        model: model.map(str::to_string),
        resume: args.resume.clone(),
        fork: args.fork,
        permission_mode: mode,
        bypass_permissions: mode.is_bypass(),
        dry_run: args.print_command,
        schema: schema.as_ref().map(|s| s.as_value().clone()),
        schema_max_retries: schema.as_ref().map(|_| max_retries),
        batch: batch_run.then_some(BatchReport {
            strategy: batch_strategy,
            prompt_count: prompts.len(),
            forked,
        }),
        config_files: loaded.files,
        results,
    };
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
            supports_resume: false,
            supports_fork: false,
            fork_reuses_cache: false,
            sync: None,
            hooks: None,
            global_hook: None,
            gate_deny: None,
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
