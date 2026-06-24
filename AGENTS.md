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
  a timeout, and emits one JSON report. `list` and `detect` describe and probe
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
  rewrites it to the underlying `node <cli.js>`) — but the cmd.exe `%*`
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
- **No crates.io publish** — the binary is distributed via GitHub Releases and
  `cargo install --git`; the `oneharness-core` library is consumed by sibling
  tools as a git dependency, not from crates.io. release-plz versions/tags only
  the binary (`oneharness-core` is `release = false`).
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
- **Bypass-by-default is deliberate.** Headless agent runs hang waiting for
  approval, so `run` requests each harness's "don't prompt" mode by default. This
  is documented in `--help`; `--no-bypass` opts out. Keep this explicit, never
  silent.
- **Config is layered and loud.** Defaults come from `oneharness.toml` files —
  user level (`$ONEHARNESS_CONFIG` or the platform config dir) under project
  level (discovered upward from `--cwd`/cwd) under CLI flags; `[harness.<id>]`
  beats top-level within a file. Unknown fields or harness ids are usage errors
  (exit 2), never ignored. Parsing/merging is pure
  (`crates/oneharness-core/src/domain/config.rs`); discovery/reading is I/O
  (`crates/oneharness-core/src/io/config.rs`). Anything that must be hermetic
  (tests, `smoke.sh`, the e2e scripts) sets `ONEHARNESS_NO_CONFIG=1` so the
  machine's real config can never reshape an assertion — keep that property
  when adding tests or scripts.
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
  harness, like Qwen, that only fires user-scoped hooks headlessly).

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
  the required checks are green, then tags `vX.Y.Z` and cuts the GitHub Release.
  That Release fires `release.yml`, which re-gates on the tests and attaches the
  checksummed cross-platform binaries.
- **Requires a PAT.** The automation runs only once a `RELEASE_PLZ_TOKEN` repo
  secret exists (a classic or fine-grained PAT with `contents: write` +
  `pull-requests: write`). A tag/Release made with the default `GITHUB_TOKEN`
  would not retrigger `release.yml`, so the binaries would never build; until the
  secret is set, the workflow's `guard` job no-ops cleanly. The crate version and
  `CHANGELOG.md` are managed by release-plz — do not hand-bump them.
- **Manual fallback.** Creating a GitHub Release by hand (the UI, or
  `gh release create vX.Y.Z`) fires the same `release: published` event and builds
  the binaries — use it only if the automation is wedged. Never publish by editing
  a release by hand mid-flight.
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
