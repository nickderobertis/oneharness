# AGENTS

`oneharness` is a single Rust CLI that drives many agentic coding harnesses
(Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot CLI, Cursor)
through one non-interactive interface and returns one stable JSON shape. Its
consumers are programs, not humans: e2e suites that exercise a feature against
every real harness, and (later) a cross-harness skill-testing framework that uses
this CLI as its driver.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`). Edit
> `AGENTS.md` only; the two must never drift.

## Command surface

Use the `just` recipes; do not hand-roll equivalents.

- `just bootstrap` — set up from a clean clone (toolchain components + fetch).
- `just check` — full gate: format check, clippy (`-D warnings`), tests, build.
  Must pass before any commit or PR.
- `just test` / `just lint` / `just format` — individual gate steps.
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
  the registry. All three emit JSON to stdout by design.

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
- **No crates.io publish** — this is an end-user binary, distributed via GitHub
  Releases and `cargo install --git`, not a library dependency.
- **No pre-commit/lefthook, direnv, or `src/`-style layout** — template baggage
  that doesn't fit a single-crate Rust CLI. The gate is `just check` plus CI, on
  the standard Cargo layout.
- **`.tool-versions` pins `just` only** (read by asdf/mise) so a clean clone can
  resolve the command runner; the Rust toolchain stays on rustup, not asdf.
- **Shell scripts are linted with shellcheck** — an external tool, handled like
  `cargo-deny`: CI installs it and `just lint-sh` (part of `just check`) enforces
  it; install it locally (`apt-get`/`brew install shellcheck`) to run the full
  gate.

## Invariants (non-negotiable)

- **The domain layer is pure.** `src/domain/` builds argv, parses output, and
  shapes the report with no process / filesystem / env / clock I/O. All I/O lives
  in `src/io/` (spawning, PATH resolution, version probing) and `src/commands/`.
  Never hide I/O in a helper that looks pure.
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
  (exit 2), never ignored. Parsing/merging is pure (`src/domain/config.rs`);
  discovery/reading is I/O (`src/io/config.rs`). Anything that must be hermetic
  (tests, `smoke.sh`, the e2e scripts) sets `ONEHARNESS_NO_CONFIG=1` so the
  machine's real config can never reshape an assertion — keep that property
  when adding tests or scripts.
- Validate all external / IO inputs (args, stdin, env, subprocess output) at the
  boundary. Keep the artifact portable across Linux, macOS, and Windows.
- Do not commit secrets, credentials, PII, or customer data.

## Adding or changing a harness

A new harness is a new entry in the registry (`src/domain/harness.rs`) plus its
`build_argv`/`extract` functions — no changes to `run`, the runner, or the report
shape. When you add one:

- Add a `--print-command` assertion in `tests/cli.rs` pinning its exact argv
  (this is the deterministic, network-free proof the adapter is correct).
- Update the harness table in `README.md` — including its config-support
  columns (`model`, `system`, bypass, output format, `--resume`), which document
  how each unified setting reaches (or doesn't reach) the harness.
- Source the real invocation from a known-good driver — the
  `nickderobertis/allowlister` repo's `run_agent()` / `e2e-*.sh` drivers are the
  reference — rather than guessing flags. (`scripts/smoke.sh --live` here is the
  fast way to confirm a real invocation actually works once installed.)
- Add the per-harness live counterpart: a `scripts/e2e-<id>.sh` (source
  `e2e-lib.sh`; declare its auth env and any model/provider knobs), a `live-<id>`
  just recipe, a `.github/workflows/e2e-<id>.yml` gated like the others, and — if
  it needs a secret not already synced — an entry in `gh-secrets.json`.

## Scripts and output are context

- Recipes are quiet on success — a line or nothing. On failure they preserve the
  exact error (paths, line/cols, rule names, exit codes) and suggest the next
  action. Treat all command output as context the next agent must read.

## Tests are context engineering

- Tests are how you and future agents see this system behave; invest in them.
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
