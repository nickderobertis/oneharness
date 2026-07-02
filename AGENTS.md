# AGENTS

`oneharness` is a Rust CLI that drives many agentic coding harnesses
(Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot CLI, Cursor)
through one non-interactive interface and returns one stable JSON shape. Its
consumers are programs, not humans: e2e suites that exercise a feature against
every real harness, and (later) a cross-harness skill-testing framework that uses
this CLI as its driver.

It is a two-crate Cargo workspace: **`oneharness-core`**
(`crates/oneharness-core`) is the reusable engine — the pure `domain` layer and
the `io` boundary, including the harness registry, hook rendering/installation,
config layering, and the sync merge — depending only on serde/toml/thiserror/
which/wait-timeout (never `clap`). The root **`oneharness`** crate is the thin
binary: the `clap` surface (`src/cli.rs`) and per-verb orchestration
(`src/commands/`) over the core. The split exists so sibling tools (e.g.
`nickderobertis/allowlister`) can depend on the engine — most of all
`io::hooks::install`, which writes a normalized hook into any harness's native
config at either project or user-global scope — as a lean git dependency without
pulling the CLI.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`). Edit
> `AGENTS.md` only; the two must never drift.

## Two standing goals on every task

The user drives product features and their request is the priority — but carry
two goals into *every* task. When either is the lowest-error path to what the
user asked, fold it into the same task without asking first; surface the rest as
follow-ups (see "After the main task").

1. **Engineer the context for next time.** Make the next agent (and you) see
   more for less: realistic end-to-end tests that exercise what consumers
   actually observe — especially when a bug existing tests missed surfaces (the
   suite is this repo's only QA loop, see "Tests are context engineering") —
   scripts and skills that automate repetitive steps and shrink their output to
   signal, and terse `AGENTS.md` notes capturing what the code doesn't make
   obvious.
2. **Engineer the codebase and environment.** Be the engineer the user isn't:
   prioritize the technical initiatives that keep the codebase clean,
   maintainable, and repeatable, and keep setup automated and consistent
   (`just bootstrap` from a clean clone). Strict quality gates plus local/CI
   parity make results repeatable (here, `just check` on the pinned toolchain).
   A clean base and a reproducible environment are usually how the user's
   feature ships with a low error rate.

## Command surface

Use the `just` recipes; do not hand-roll equivalents.

- `just bootstrap` — set up from a clean clone (toolchain components + fetch).
- `just check` — full gate: format check, clippy (`-D warnings`), tests, line
  coverage (hard-gated at 95%), build, smoke. Must pass before any commit or PR.
- `just test` / `just lint` / `just format` — individual gate steps.
- `just coverage` — run the workspace suite under `cargo llvm-cov` and fail below
  95% line coverage (the `COVERAGE_MIN` gate, also part of `just check` and CI).
  `just coverage-html` writes a browsable report to find uncovered lines.
- `just upgrade` — update dependencies, then re-run `just check`.
- `just deps-check` — advisory/license audit (`cargo deny`); separate from the
  core gate because it needs a network-fetched advisory DB.
- `just smoke` — hermetic end-to-end smoke of the built binary (part of `just
  check` and CI). `just smoke-live` is the opt-in variant that hits installed,
  authenticated harnesses with real model calls — never in the gate or CI.

## What this binary is

- A thin CLI over a registry of **harness adapters**. Each adapter is data: a
  canonical id, a default binary name, an install hint, an output format, and two
  pure functions — build the argv, and best-effort extract the final text.
- `run` spawns the selected harnesses **in parallel**, each as a subprocess with
  a timeout, and emits one JSON report. A **same-prefix batch** is the dual shape:
  pass more than one prompt (`--prompt`/`--prompt-file` are repeatable, each one
  whole prompt) and `run` fans **one** harness over the N prompts instead —
  single-harness by nature (a provider cache prefix is per harness/model/tools),
  and not itself a `--resume`/`--fork` continuation (those are a usage error with a
  batch). Pure scheduling lives in `domain::batch` (the `BatchStrategy` waves:
  `speed` = one concurrent wave; `min-tokens` = a one-call warm-up wave then the
  fanned-out rest, with a barrier between). `min-tokens` *reduces tokens* only via a
  **cache-reusing fork** (`HarnessSpec.fork_reuses_cache`, a capability beyond
  `supports_fork`): a static `--system` is NOT reused across separate harness
  processes (Claude Code re-creates a user-supplied `--append-system-prompt` every
  `claude -p`; only its own global prefix gets cross-process cache reads — verified
  live, three experiments). So on a harness whose fork reuses the cache (today
  **claude-code only**) the command layer's `run_fork_batch` runs the warm-up
  (prompt[0], establishing a session that carries `--system`), reads its
  `session_id`, then rewrites the fan-out jobs to `--resume <sid> --fork` (dropping
  `--system`, inherited from the session) so they reuse the warmed prefix.
  **OpenCode is fork-*capable* but `fork_reuses_cache: false`** — its `--fork`
  re-sends the branched conversation cold (fan-out reads no cache, re-writes the
  whole prefix — measured live, so forking it would *raise* tokens). Without a
  cache-reusing fork, `min-tokens` only orders the calls (a stderr warning — no
  reuse). `run_in_waves` covers `speed`/order-only; `run_fork_batch` the fork path.
  Only spawning is I/O, so the warm-then-fan ordering is unit-testable against the
  mock (`MOCK_LOG_FILE` records start/end interleaving; a mock `session_id` drives
  the fork-argv test). Each result carries its own `prompt`; the report's `batch`
  block carries `strategy`/`prompt_count`/`forked`. `--batch-strategy` is a
  per-invocation orchestration knob, so — unlike most `run` flags — it deliberately
  has no config/`ONEHARNESS_*` layer. The live drift alarm that the mode *reduces
  tokens* is `oh_batch_fork_enforce` in `e2e-claude.sh` (the fork fan-out reads the
  warmed prefix and writes less than the warm-up), tied to the `usage` cache counts.
  It is claude-only because claude-code is the only `fork_reuses_cache` harness;
  `e2e-opencode.sh` deliberately omits it (its fork doesn't reuse — see above).
  `list` and `detect` describe and probe
  the registry; `config` shows the effective layered configuration with each
  value's source; `sync` merges the unified policy settings (allow/deny rules,
  hooks, raw `settings` tables) into each harness's **own** config file — project
  by default, or the user-global location under `--global` (hooks only) — so the
  policy also applies without oneharness in the loop. Those five emit JSON to
  stdout by design. `gate <id>` is the odd one out: the runtime pre-tool gate an
  installed `[[hooks]]` hook invokes, reading a harness's hook event on stdin and
  emitting its native deny verdict on stdout (pure shapes in `domain::gate`). It
  exists to prove a synced hook is *honored* end to end (the per-harness live
  e2e drives a real harness through it), not to be a policy engine — that is the
  sibling `allowlister`'s role, which consumes the `install` library.
- **Structured output** (`run --schema <file>`): constrain each harness's final
  answer to a JSON Schema, validate it (the `jsonschema` crate, pinned
  `default-features = false` so it stays offline), and re-prompt on failure up to
  `--schema-max-retries` (default 2). Two deliveries, per `HarnessSpec.native_schema`:
  *native* where the CLI has a schema flag (only Claude Code's `--json-schema`
  today, value read from `structured_output`), *prompt-based* for the rest (the
  schema is appended to the prompt, the value recovered from the answer text).
  oneharness validates either way, so a native flag the harness ignores is still
  caught. The validate/retry loop lives in the runner as `run_jobs_with` (a pure
  domain closure decides re-runs; the runner owns spawning), so it stays parallel
  across harnesses. Pure logic — schema compile/validate, JSON extraction,
  instruction text, the shared `check` used by both the loop and the report — is
  in `domain::structured`. Like every normalized signal, the structured value is
  **never fabricated**: no extractable JSON is "invalid", not a guess. Codex's
  native `--output-schema` is deliberately *not* wired yet (file-based + ignored
  once tools run, https://github.com/openai/codex/issues/15451); adding it is one
  registry line plus a `build_argv` arm (the pointer comments at the codex
  registry entry and `structured::NativeSchema` say exactly what to change). The
  *per-feature* live e2e (`scripts/e2e-schema.sh` / `just live-schema` /
  `e2e-schema.yml`, helper `oh_schema_enforce`) drives real claude-code through
  `--schema` and asserts a schema-valid round-trip — the drift alarm for the
  native `--json-schema` flag the hermetic suite can only mock. That live check
  is Linux/macOS-only: a JSON Schema is quote-heavy and npm `.cmd` shims mangle
  quote-containing argv via cmd.exe `%*` on Windows (so structured output is
  unreliable against a `.cmd`-shim harness there — documented in the README; the
  hermetic `check` job still covers the Windows argv/validation path). The
  structured-output prompt additions stay **single-line** by convention; this is
  not a spawn constraint — `io::runner` now spawns a multi-line argument against
  a `.cmd`-shim harness by bypassing the shim (`domain::shim::parse_cmd_shim`
  rewrites it to the shim's real target: `node <cli.js>`, or the wrapped `.exe`
  directly, as for claude-code whose bin is `bin/claude.exe`) — but the cmd.exe `%*`
  quote-mangling above is a *separate* limitation the bypass does not touch
  (a quote-heavy schema is single-line, so it never triggers the bypass).

## How this repo was composed

Assembled with the `create-repo` skill from one **product shape** (CLI), one
**language** (Rust), and the **CI** cross-cutting reference. What the skill's
catalog offered but this repo deliberately leaves out — recorded so the choices
aren't re-litigated each session:

- **No `rust-cli` intersection reference** — none exists yet. The overlap it
  would cover (snapshot-testing a compiled binary, cross-platform release
  artifacts) is handled inline by the `--print-command` argv assertions and the
  `release.yml` matrix.
- **No `cargo-dist`** — `release.yml`'s native build matrix already ships
  checksummed cross-platform binaries; cargo-dist's generated pipeline isn't
  worth replacing it. (`release-plz` *is* now used for conventional-commit
  versioning — see *Releasing* — driven directly with a `RELEASE_PLZ_TOKEN` PAT,
  reversing the earlier "hand-versioned only" choice.)
- **crates.io publish** (*now enabled* — reversing the earlier "distribute the
  binary via GitHub Releases / `cargo install --git` only" choice). release-plz
  runs `cargo publish` on release for **both** crates in dependency order
  (`oneharness-core` then the `oneharness` binary), so a release ships via
  crates.io *and* the GitHub Release binaries *and* `git`. Publishing the binary
  forces the engine onto crates.io too: a path dependency must resolve to a
  registry version at publish time, so `Cargo.toml` pins
  `oneharness-core = { path = ..., version = "x.y.z" }` (release-plz keeps that
  `version` in step). `oneharness-core` is git-tagged in its own
  `oneharness-core-v{{ version }}` namespace (NOT the binary's `v{{ version }}` —
  a shared scheme collides, and release-plz then mistakes an existing `vX.Y.Z`
  for the engine being already published and skips its `cargo publish`; see
  *Releasing*) and keeps `git_release_enable = false`, so only the binary gets a
  `vX.Y.Z` tag *and* GitHub Release — one `vX.Y.Z` tag still means one thing.
  Needs a `CARGO_REGISTRY_TOKEN` secret alongside the `RELEASE_PLZ_TOKEN` PAT
  (see *Releasing*).
- **PyPI wheels** (*now enabled*, mirroring `nickderobertis/llmlint`). `pyproject.toml`
  uses maturin's `bindings = "bin"` (the ruff/uv pattern) to wrap the prebuilt
  `oneharness` binary in per-platform wheels, so `pip install oneharness-cli` is a
  seconds-fast binary install where PyPI is reachable but github.com may be
  blocked. The PyPI distribution is **`oneharness-cli`** (the bare name was
  unavailable); the console command it installs is still `oneharness`. The wheel
  version is `dynamic` — maturin reads it from `Cargo.toml`, so release-plz stays
  the single version driver (never hand-set a version in `pyproject.toml`).
  `release.yml`'s `build-wheels` job runs on every release (so a packaging break
  surfaces even while publishing is off); `publish-pypi` uses keyless **Trusted
  Publishing** (OIDC, environment `pypi`, no token secret) and stays dormant until
  the `PYPI_PUBLISH` repo variable is `true` and the PyPI project registers this
  repo's `release.yml` as its Trusted Publisher; `verify-pypi` then proves the
  published version is `pip install`-able.
- **Sigstore release signing + mirror-safe `install.sh`** (*now enabled*,
  mirroring llmlint). `release.yml`'s `upload` job signs each archive with a
  keyless [Sigstore](https://www.sigstore.dev/) build-provenance attestation
  (`actions/attest-build-provenance@v2`, OIDC `id-token` — no secret) and
  publishes the `.sigstore.json` bundle beside the archive. `scripts/install.sh`
  verifies the downloaded archive against a trust root **independent of the
  mirror it came from**, in order: (1) the Sigstore bundle, verified OFFLINE by
  `cosign` → `sigstore` (python) → `gh` (whichever is installed), pinned to this
  repo's `release.yml` signer identity + SLSA-provenance predicate; (2) a SHA-256
  checksum from canonical GitHub — and it **refuses** a checksum that shares the
  mirror's origin (a mirror vouching for its own download is no trust root),
  aborting instead. Never re-introduce a "trust the mirror's own checksum" escape
  hatch. The `verify-attestation` release job runs the exact `cosign`/`sigstore`
  commands `install.sh` uses against the real published bundle — the drift alarm
  for the signing identity/flags. The install path is proven hermetically by
  `scripts/install-e2e.sh` (run in `just check`/CI via `smoke.sh`): independent
  checksum installs, tampered mirror rejected, mirror-origin checksum refused, and
  a stubbed `cosign`/`sigstore`/`gh` proves the Sigstore gate (pass installs, fail
  aborts) without a live signature.
- **No pre-commit/lefthook or direnv** — template baggage. The gate is `just
  check` plus CI on the standard Cargo workspace layout (root binary crate +
  `crates/oneharness-core` library).
- **`.tool-versions` pins `just` only** (read by asdf/mise) so a clean clone can
  resolve the command runner; the Rust toolchain stays on rustup, not asdf.
- **Shell scripts are linted with shellcheck** — an external tool, handled like
  `cargo-deny`: CI installs it and `just lint-sh` (part of `just check`) enforces
  it; install it locally (`apt-get`/`brew install shellcheck`) to run the full
  gate.
- **Coverage is enforced at the skill default, 95% lines** (`COVERAGE_MIN` in the
  justfile), via `cargo llvm-cov` — an external tool, handled like `cargo-deny`
  and shellcheck: CI installs it (`cargo-llvm-cov` + the `llvm-tools-preview`
  rustup component, added by `just bootstrap`) and `just coverage` (part of `just
  check`) fails the gate below the bar. It measures the whole workspace
  (`--workspace`), so the `oneharness-core` engine is gated alongside the binary.
  The threshold is line coverage, not region/branch: the hermetic mock-harness
  suite drives whole user journeys (high-leverage line coverage), and a few
  I/O-failure arms in `crates/oneharness-core/src/io/runner.rs` (spawn/wait
  errors) and `io/config.rs` are intentionally left ungated rather than faked with
  brittle environment manipulation. Measured coverage sits above 95% lines; keep
  new behavior covered rather than lowering `COVERAGE_MIN`. Coverage is a
  platform-independent property of the suite, so it is enforced on Linux/macOS and
  skipped on Windows, where llvm-cov does not attribute the integration tests'
  subprocess-spawned binary coverage (a tooling limitation — the binary reads ~0%
  there). The functional gate still runs on all three platforms; only the coverage
  *measurement* is Linux/macOS.

## Invariants (non-negotiable)

- **The domain layer is pure.** `crates/oneharness-core/src/domain/` builds
  argv, parses output, and shapes the report with no process / filesystem / env
  / clock I/O. All I/O lives in `crates/oneharness-core/src/io/` (spawning, PATH
  resolution, version probing, config/hook file writes) and the binary's
  `src/commands/`. Never hide I/O in a helper that looks pure.
- **Output is a contract.** The JSON report on stdout carries a `schema_version`;
  it is the interface consumers depend on. Add fields; do not repurpose or remove
  them without bumping the version. Diagnostics go to stderr, never stdout.
- **Best-effort `text`, guaranteed envelope.** The execution envelope (command,
  exit code, stdout, stderr, duration, status) is guaranteed and identical across
  harnesses. The normalized `text` field is a convenience whose method is recorded
  in `text_source`; it is `null` when extraction is not possible. Never fabricate
  it — consumers needing certainty parse `stdout`.
- **Never panic on a harness's behavior.** A missing binary is `skipped`, a
  non-zero exit is `nonzero`, a hang is `timeout` — all are data in the report,
  not a crash. Only true usage/config errors abort with a non-zero process exit.
- **Approval mode is explicit; the default is `default`, not bypass.** The
  normalized spectrum lives in `domain::mode` (`read-only` < `plan` < `default` <
  `edit` < `auto` < `bypass`; `read-only` is no-mutation enforcement without the
  plan workflow, `plan` adds it). The built-in default is `default` — each
  harness's normal posture mapped to its cleanest *non-interactive* variant
  (Claude's `dontAsk` deny-and-continue, Goose's fail-closed `approve`, Copilot's
  auto-deny), so it neither hangs nor blanket-approves; `bypass` ("allow
  everything") is the opt-in (`--mode bypass` / `--bypass`). Each harness
  declares which modes it can express and whether each is headless-`clean` or
  would `hangs` (`HarnessSpec.modes` / `ModeSpec`). The command layer refuses an
  *unsupported* mode before spawning (no command to build — a loud usage error,
  never a silent downgrade); a *supported-but-`hangs`* mode is warned about on
  stderr and still run, with the per-harness `--timeout` as the backstop (a hang
  becomes a `timeout` result, per "never panic on a harness's behavior"), and
  `--permit-prompts` silences the warning. The per-harness `read-only`/`plan`
  mapping is drift-alarmed live by `oh_mode_enforce` (writes blocked under
  read-only, allowed under bypass).
- **Config is layered and loud.** Defaults come from `oneharness.toml` files —
  user level (`$ONEHARNESS_CONFIG` or the platform config dir) under project
  level (discovered upward from `--cwd`/cwd) under the `ONEHARNESS_<FIELD>`
  environment overrides under CLI flags; `[harness.<id>]` beats top-level within
  a file. The env overrides are not handled per-command: `domain::config::from_env`
  parses them into a `FileConfig` layer (pure — it takes a getter closure;
  `io::config` passes `std::env::var`) appended after the files in `load_layers`,
  so they flow through `run`/`detect`/`sync` *and* `config`'s provenance (source
  `"environment"`) for free, and CLI-beats-config already makes CLI beat env.
  Keep the trio in sync: a new top-level field with a `run` flag wants a
  matching `ONEHARNESS_<FIELD>` arm in `from_env` (sync-policy fields,
  `[env]`, and `[harness.<id>]` deliberately have none). Unknown fields, bad
  values, or unknown harness ids are usage errors (exit 2), never ignored.
  Parsing/merging is pure (`crates/oneharness-core/src/domain/config.rs`);
  discovery/reading is I/O (`crates/oneharness-core/src/io/config.rs`). Anything
  that must be hermetic (tests, `smoke.sh`, the e2e scripts) sets
  `ONEHARNESS_NO_CONFIG=1`, which disables the env overrides too, so the
  machine's real config — files *or* `ONEHARNESS_*` — can never reshape an
  assertion. The `tests/cli.rs` config helper also strips ambient overrides;
  keep that property when adding tests or scripts.
- Validate all external / IO inputs (args, stdin, env, subprocess output) at the
  boundary. Keep the artifact portable across Linux, macOS, and Windows.
- Do not commit secrets, credentials, PII, or customer data.

## Adding or changing a harness

A new harness is a new entry in the registry
(`crates/oneharness-core/src/domain/harness.rs`) plus its `build_argv`/`extract`
functions — no changes to `run`, the runner, or the report
shape. When you add one:

- Add a `--print-command` assertion in `tests/cli.rs` pinning its exact argv
  (this is the deterministic, network-free proof the adapter is correct).
- Declare its `modes` (`HarnessSpec.modes`): one `ModeSpec` per
  [`PermissionMode`] the CLI can express, each tagged `clean` or `hangs`
  headless, sourced from that CLI's docs/behavior — never guessed. Every harness
  lists `bypass` and `default`. Map each in `build_argv` (or, when the mode is an
  environment variable like Goose's `GOOSE_MODE`, in `ModeSpec.env`), and prefer
  the cleanest non-interactive variant for `default` (e.g. a deny-and-continue,
  not an interactive prompt). A *behavioral* mode a harness can't express
  natively can be synthesized from **enforcement + an instruction**: set
  `ModeSpec.instruction` (prepended to the prompt by the command layer) and pair
  it with the enforcement `build_argv`/`env` provides — this is how Codex's
  `plan` works (read-only sandbox + a plan instruction). Only do this when the
  enforcement half exists (a plan instruction without read-only enforcement
  wouldn't stop the agent acting). Pin the mode→flag mapping with a `build_argv`
  assertion, and update the *Approval modes* table in `README.md`. For each
  no-mutation mode the harness supports (`read-only`, `plan`), add an
  `oh_mode_enforce <id> <mode>` phase to its `e2e-<id>.sh` (a write blocked under
  `--mode <mode>`, allowed under `--mode bypass`) — the live proof the mapping is
  honored and its drift alarm. If the harness's `edit` auto-approves a write that
  its non-edit posture would deny (copilot, qwen), add `oh_edit_enforce <id>` —
  under `--mode edit` a file-tool edit must succeed, the live proof the edit
  mapping is honored. Its "gate shell" half is NOT asserted live: it isn't
  reliably testable, because a model told to write via shell routes around any
  gate through whatever path the harness still allows (copilot auto-approves
  `echo`, opencode delegates to a `task` subagent, qwen's auto-edit ran it) — that
  half stays argv/env-pinned in the `domain::harness` tests. A mode delivered by
  environment (Goose's `GOOSE_MODE`, OpenCode's `OPENCODE_CONFIG_CONTENT`) is
  pinned hermetically via the mock harness's `MOCK_ECHO_ENV` instead. (`auto`
  likewise has no live drift-alarm: a deterministic cross-harness check would
  hinge on the classifier's model-dependent safe/risky split — it stays
  unit-pinned.)
- Update the harness table in `README.md` — including its config-support
  columns (`model`, `system`, bypass, allow/deny rules, hooks, output format,
  `--resume`), which document how each unified setting reaches (or doesn't
  reach) the harness — and the `supports_*` capability fields in the registry.
  Policy settings (`allowed_tools`/`denied_tools`/`hooks`/`settings`) are
  delivered by `oneharness sync` into the harness's own config file (the
  `SyncSpec` in the registry — file path + key paths, sourced from that CLI's
  docs, never guessed). They follow the loud-absence rule: no mapping means a
  parse error for `[harness.<id>]` fields and an `unmapped` entry in the sync
  report (plus a stderr warning) for top-level ones — never a silent drop. The
  sync merge is non-destructive by contract: unrelated keys untouched, lists
  unioned (idempotent re-sync), unparseable files refused and left intact,
  writes atomic. Keep those properties test-pinned when touching it.
- Declare `supports_resume` / `supports_fork` and map them in `build_argv`,
  sourced from that CLI's headless docs (never guessed). *Resume* is the
  continuation flag (`--resume`, `--session`, or a subcommand like Codex's `exec
  resume <id>`); all current harnesses support it, so `supports_resume` is a
  drift-alarm for a future one that doesn't — when false, the command layer
  rejects `--resume` rather than silently starting fresh. *Fork* (`run --resume
  <id> --fork`) branches a new session from the resumed one and is rare — only
  Claude Code (`--fork-session`) and OpenCode (`--fork`) express it headlessly;
  the rest resume linearly, and `--fork` is a loud usage error for them (`fork`
  implies `resume`, clap-enforced). Pin each mapping with a `build_argv`/`--print-
  command` assertion, and remember the session-id round-trip: if the harness emits
  an id headlessly, teach `signals::extract_session` its field (Codex's
  `thread_id`); if it emits none (Goose, Copilot), the continuation handle is
  caller-supplied (a `--name` / minted UUID) and `session_id` stays `null` — never
  fabricate one. Update the `--resume` column in `README.md`. Also declare
  `fork_reuses_cache` (implies `supports_fork`): true only if a forked run reuses
  the parent session's prompt-cache prefix, which is what makes a `min-tokens`
  batch save tokens — gate, **measured** by `oh_batch_fork_enforce` not guessed
  (true for claude-code; false for opencode, whose fork re-sends the prefix cold).
  When true, add the `oh_batch_fork_enforce <id>` live phase and update the batch
  support matrix in `README.md`.
- Set its `native_schema` only if the CLI has a real schema flag, sourced from
  that CLI's docs (never guessed) — and pin the injected argv with a
  `--print-command`/`build_argv` assertion. `None` is the right default: the
  prompt-based structured-output path already works for every harness, and
  oneharness validates the result regardless. If a harness reports its conforming
  value somewhere other than the answer text, extend `structured::extract_value`.
- Give the harness its `global_hook` (the user-global hook location, for `sync
  --global` / `install` at `Scope::Global`) and its `gate_deny` (how it expresses
  a pre-tool deny when it runs `oneharness gate <id>`). Both are registry data
  sourced from the allowlister adapters, never guessed; both are loud when absent
  (a missing `gate_deny` makes `oneharness gate <id>` a usage error). Pin the new
  deny shape with a `--print`-style assertion in `domain::gate`/`tests/cli.rs`.
- Source the real invocation from a known-good driver — the
  `nickderobertis/allowlister` repo's `run_agent()` / `e2e-*.sh` drivers are the
  reference — rather than guessing flags. (`scripts/smoke.sh --live` here is the
  fast way to confirm a real invocation actually works once installed.)
- Add the per-harness live counterpart: a `scripts/e2e-<id>.sh` (source
  `e2e-lib.sh`; declare its auth env and any model/provider knobs), a `live-<id>`
  just recipe, a `.github/workflows/e2e-<id>.yml` gated like the others (a
  `fail-fast: false` matrix over `ubuntu-latest`/`macos-latest`/`windows-latest`
  with `defaults.run.shell: bash`, so the bash scripts run under Git Bash on
  Windows; any `curl | bash` installer needs a PowerShell branch for the Windows
  leg), and — if it needs a secret not already synced — an entry in
  `gh-secrets.json`. If the
  harness has a `SyncSpec`, also add the `oh_sync_enforce` phases (allow rule
  executes under `--no-bypass`, deny rule doesn't): that live check is the only
  proof the synced file is *honored* and the drift alarm for its format. Unless
  the harness can't load a hook through `oneharness run` (Codex's `codex exec`
  ignores hooks) or needs bespoke trust scaffolding (Copilot), also add the
  `oh_hook_enforce <id> [scope]` phase — it syncs a `oneharness gate <id>` hook
  and proves the real CLI blocks a marked command and runs an unmarked one, the
  honoring proof + drift alarm for the *hook* install (use `global` scope for a
  harness, like Qwen, that only fires user-scoped hooks headlessly). If the
  harness reports provider prompt-cache counts in its usage (today only Claude
  Code and OpenCode — see `extract_usage` in `domain::signals` and the README
  `usage` support matrix), also add the `oh_cache_assert <id>` phase: a second run
  within the cache TTL must surface `cache_read_tokens > 0` — the live drift alarm
  that cache-token extraction still matches the real output shape.

## Scripts and output are context

- Recipes are quiet on success — a line or nothing. On failure they preserve the
  exact error (paths, line/cols, rule names, exit codes) and suggest the next
  action. Treat all command output as context the next agent must read.

## Tests are context engineering

- Tests are how you and future agents see this system behave; invest in them.
- **Coverage is a hard gate at 95% lines** (`just coverage`, run inside `just
  check` and CI, measuring the whole workspace). A user-visible change ships with
  a test that fails without it, and the coverage number keeps a behavior the tests
  never execute from slipping in unseen. Find the gaps with `just coverage-html`;
  raise the tests, never lower `COVERAGE_MIN`. (Rationale and tooling notes: *How
  this repo was composed*.)
- The execution path is proven **hermetically** by a mock harness binary
  (`tests/support/mock_harness.rs`, built behind the `mock-harness` feature) that
  oneharness drives via a `--bin` override — no network, no real CLI, fully
  deterministic and cross-platform. Command construction is proven by
  `--print-command` argv assertions covering every harness.
- An end-to-end smoke of the *built* binary (`scripts/smoke.sh`, via `just
  smoke`) is part of `just check` and CI: it drives the real artifact through
  `list`/`detect`/`--print-command` plus one mock spawn, fully hermetically —
  proving the shipped binary, not just the test-compiled crate.
- A live cross-harness smoke against real CLIs (`just smoke-live`) is
  deliberately **out** of `just check` and CI: it needs installed binaries, auth,
  and network and makes real model calls. It stays opt-in and skips cleanly when
  no harness is installed.
- The allowlister-style **per-harness** live suite (`scripts/e2e-<id>.sh`,
  `just live-<id>` / `live-all`, `.github/workflows/e2e-<id>.yml`) is the granular
  counterpart to `smoke-live`: each check drives ONE real harness with its own
  model/provider config and asserts the marker round-trips (status ok + marker
  surfaced), so CI gets a per-harness pass/fail. Also out of the core gate; the
  workflows are gated to the canonical repo and non-fork PRs. Auth comes from the
  `gh-secrets.json` manifest (Bitwarden secure notes → `.env` + GitHub Actions
  secrets via `just secrets-sync`); values never enter the repo and `.env` /
  `.gh-secrets-state.json` are gitignored.
- A user-visible change ships with a test that fails without it.

## Releasing

- Releases are automated from conventional commits by **release-plz**
  (`release-plz.toml` + `.github/workflows/release-plz.yml`), mirroring
  nickderobertis/allowlister. Land conventional commits on `main` (`feat` →
  minor, `fix`/`perf` → patch, `!`/`BREAKING` → major; `docs`/`test`/`chore`/`ci`
  do not release — so commit subjects are load-bearing for both the bump and the
  generated `CHANGELOG.md`). release-plz opens a `release vX.Y.Z` PR that bumps
  `Cargo.toml`/`Cargo.lock` and writes the changelog section, auto-merges it once
  the required checks are green, then — in one `release-plz release` run —
  `cargo publish`es both crates in dependency order (`oneharness-core`, then the
  `oneharness` binary), tags `vX.Y.Z`, and cuts the GitHub Release. That Release
  fires `release.yml`, which re-gates on the tests, attaches the checksummed
  cross-platform binaries + their Sigstore `.sigstore.json` bundles, and builds &
  publishes the PyPI wheels. So a release lands four ways: **PyPI**
  (`pip install oneharness-cli`), **crates.io**, the GitHub Release binaries, and
  `cargo install --git` (see the PyPI-wheels and Sigstore bullets under *How this
  repo was composed*). Only the binary gets a
  `vX.Y.Z` tag + GitHub Release; `oneharness-core` is published and tagged in its
  own `oneharness-core-v{{ version }}` namespace (with `git_release_enable =
  false`, so no GitHub Release) — a distinct namespace is required, else its
  version can collide with a historical binary `vX.Y.Z` tag and release-plz skips
  the engine's `cargo publish` (this exact collision — core 0.3.0 vs the binary's
  old `v0.3.0` — broke the first automated release). See the crates.io bullet
  under *How this repo was composed*.
- **Requires two secrets.** The automation runs only once BOTH repo secrets
  exist; the `guard` job no-ops cleanly (no partial release) until then.
  `RELEASE_PLZ_TOKEN` is a PAT (classic or fine-grained, `contents: write` +
  `pull-requests: write`): a tag/Release made with the default `GITHUB_TOKEN`
  would not retrigger `release.yml`, so the binaries would never build.
  `CARGO_REGISTRY_TOKEN` is a crates.io API token for `cargo publish`; without it
  the release fails before publishing anything, so the guard requires it up
  front. `CARGO_REGISTRY_TOKEN` is synced from Bitwarden via the
  `gh-secrets.json` manifest (`just secrets-sync`); `RELEASE_PLZ_TOKEN` is a
  GitHub PAT set by hand (a PAT can't live in the harness-auth manifest's
  Bitwarden flow). The crate version and `CHANGELOG.md` are managed by
  release-plz — do not hand-bump them.
- **Manual fallback.** Creating a GitHub Release by hand (the UI, or
  `gh release create vX.Y.Z`) fires the same `release: published` event and builds
  the binaries — use it only if the automation is wedged. It does NOT publish to
  crates.io (only `release-plz release` does); run `cargo publish` by hand if a
  crates.io version is missing. Never publish by editing a release mid-flight.
- The JSON `schema_version` is independent of the crate version: bump it only when
  the report shape changes incompatibly, and document it in the changelog.

## Keeping the allowlist current

- The agent command allowlist lives in `.claude/settings.json`; the tool enforces
  it. When a new routine command joins the normal build/test workflow, add it to
  the allowlist (kept narrow) instead of re-approving it each session.

## After the main task: refine and hand off

After the requested task, propose only materially-helpful follow-ups (scripts,
`AGENTS.md` constraints, shared skills, tests/fixtures), each with its likely
impact. Skip busywork; if nothing helps, say so.
