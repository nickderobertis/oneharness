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

## What this binary is

- A thin CLI over a registry of **harness adapters**. Each adapter is data: a
  canonical id, a default binary name, an install hint, an output format, and two
  pure functions — build the argv, and best-effort extract the final text.
- `run` spawns the selected harnesses **in parallel**, each as a subprocess with
  a timeout, and emits one JSON report. `list` and `detect` describe and probe
  the registry. All three emit JSON to stdout by design.

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
- Validate all external / IO inputs (args, stdin, env, subprocess output) at the
  boundary. Keep the artifact portable across Linux, macOS, and Windows.
- Do not commit secrets, credentials, PII, or customer data.

## Adding or changing a harness

A new harness is a new entry in the registry (`src/domain/harness.rs`) plus its
`build_argv`/`extract` functions — no changes to `run`, the runner, or the report
shape. When you add one:

- Add a `--print-command` assertion in `tests/cli.rs` pinning its exact argv
  (this is the deterministic, network-free proof the adapter is correct).
- Update the harness table in `README.md`.
- Source the real invocation from a known-good driver (the allowlister
  `scripts/e2e-*.sh` are the reference) rather than guessing flags.

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
- A live cross-harness smoke against real CLIs is intentionally **out** of `just
  check` and CI: it needs installed binaries, auth, and network. Keep it opt-in.
- A user-visible change ships with a test that fails without it.

## Releasing

- Versioning is by hand; CI builds the artifacts. Bump `version` in `Cargo.toml`,
  move the `CHANGELOG.md` `[Unreleased]` entries under the new `vX.Y.Z`, land it on
  the default branch, then push a matching `vX.Y.Z` tag. The tag triggers
  `release.yml`, which gates on the test suite and publishes checksummed
  cross-platform binaries. Never publish by editing a release by hand mid-flight.
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
