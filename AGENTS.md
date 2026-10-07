# AGENTS

`oneharness` is a Rust CLI that drives many agentic coding harnesses
(Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot CLI, Cursor)
through one non-interactive interface and returns one stable JSON shape. Its
consumers are programs, not humans: e2e suites that exercise a feature against
every real harness, and (later) a cross-harness skill-testing framework that uses
this CLI as its driver.

Two crates publish: **`oneharness-core`**
(`crates/oneharness-core`) is the reusable engine — the pure `domain` layer and
the `io` boundary, including the harness registry, hook rendering/installation,
config layering, and the sync merge — depending only on serde/toml/thiserror/
which/wait-timeout (never `clap`). The root **`oneharness`** crate is the thin
binary: the `clap` surface (`src/cli.rs`) and per-verb orchestration
(`src/commands/`) over the core. The split exists so sibling tools (e.g.
`nickderobertis/allowlister`) can depend on the engine — most of all
`io::hooks::install`, which writes a normalized hook into any harness's native
config at either project or user-global scope — as a lean git dependency without
pulling the CLI. Everything else in the Cargo workspace is `publish = false` test
or contract code (see *Project graph*).

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

<!-- llmlint: ignore-block[agents_md_durable_and_terse] The composition record is mandated content — the create-repo baseline's record of the references composed, the projects in the graph and each exclusion with its reason — so its project list is the decision of record, held to the graph by scripts/check-nx-graph.mjs. The block brackets the whole section from outside because the baseline checker refuses any angle-bracket text inside it. -->
## Stack and composition

- **Product shape:** CLI plus Rust, Node, and Python libraries (`shapes/cli.md`,
  `shapes/library.md`, `intersections/rust-cli.md`).
- **Language(s):** Rust, TypeScript, Python, and Bash. Bash is gate code, not
  only setup: `scripts/` holds the smoke, install and live e2e drivers and the
  drift checks (and their tests) the tooling projects run.
- **References composed:** `base.md`, `project-graph.md`, `shapes/cli.md`,
  `shapes/library.md`, `languages/rust.md`, `languages/typescript.md`,
  `languages/python.md`, `languages/bash.md`, `intersections/rust-cli.md`,
  `ci.md`, `llmlint.md`, `releasing.md`.
- **Cross-cutting:** `project-graph.md`, `ci.md` and `releasing.md`: an Nx
  project graph runs every target (*Project graph* below); release-plz drives
  versions; the full sweep sits at release-prep (*Releasing*).
- **Projects in the graph:** `oneharness-core`, `oneharness`,
  `oneharness-integration`, `oneharness-mock-harness`, `oneharness-e2e`,
  `history-compat`, `mock-responder`, `harness-captures`, `sdk-contract`,
  `sdk-conformance`, `node-sdk`, `python-sdk`, `npm-launcher`,
  `install-surface`, `release-tooling`, `ci-contracts`, `e2e-support`,
  `workspace`, `rust-coverage`, `scripts`, the paid `live-*` suites (claude,
  codex, opencode, goose, qwen, crush, copilot, cursor, schema, control,
  variants) and the dispatch-only `explore-*` probes (control, cursor-stdin,
  events, hooks).
- **Excluded, and why:** web-app/React/Next.js/asdf-plugin/skills-repo guidance
  do not apply; release artifacts are handled by the existing Cargo/GitHub
  Release workflow rather than a separate frontend or plugin distribution.
  **No `cargo-dist`** — `release.yml`'s native build matrix already ships
  checksummed cross-platform binaries; cargo-dist's generated pipeline isn't
  worth replacing it. **No pre-commit/lefthook or direnv** — template baggage;
  the gate is `just check` plus CI, and the committed `pre-push` hook runs
  `just gate`. **No shared remote Nx cache**: the computation cache is local to
  each clone (`.nx/cache`, gitignored) and CI starts cold.
- **Recorded decisions:** both crates publish to crates.io, in dependency order,
  with `oneharness-core` tagged in its own `oneharness-core-v{{ version }}`
  namespace (*Releasing* says why). `.tool-versions` pins `just` only (read by
  asdf/mise) so a clean clone can resolve the command runner; the Rust toolchain
  stays on rustup, not asdf. Rust line coverage is measured on Linux/macOS and
  skipped on Windows, where llvm-cov does not attribute the integration tests'
  subprocess-spawned binary coverage (a tooling limitation — the binary reads ~0%
  there); the functional gate still runs on all three platforms.

## Project graph
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->

Nx runs every target; each ecosystem resolves through its own one workspace and
lockfile — Cargo (`Cargo.lock`), one Bun workspace (root `package.json` +
`bun.lock`: the Node SDK, the npm launcher and Nx itself), and one uv workspace
rooted at `python/` (`python/uv.lock`, resolving for Python 3.9; rooted there
because the root `pyproject.toml` is maturin's `oneharness-cli` wrapper, which a
uv workspace at the root would build). Every project declares the targets that
apply to it from one set — `format`, `lint`, `typecheck`, `build`, `test`,
`e2e` (an SDK's packaged-artifact journey, never part of its `test`) and the
repo-level `coverage` — and each target calls its own language's tool.

- **Ownership is by path; selection is by input.** Each file belongs to the
  project whose `project.json` is nearest above it. A target's declared
  `{workspaceRoot}/…` inputs ALSO select its project when one changes, which is
  how a drift gate under `scripts/` is re-run by the tooling project that runs
  it (and by nothing else). So a target's inputs must list every file it reads:
  a missing one is a cached pass that hides a failure.
- **No project is rooted at the repository root**: one there would own every
  file no other project owns, and every document or script edit would rebuild
  everything that depends on it. The binary crate's project is rooted at `src/`
  for that reason, while its `Cargo.toml` stays at the root.
- **Boundaries are tags**, enforced over the graph Nx itself computes by
  `scripts/check-nx-graph.mjs` (in `workspace:lint`): each project carries one
  `type:*` tag, `tools/workspace/boundaries.json` lists what each type may
  depend on, nothing may depend on a live, exploration, e2e or other leaf
  project, and the `type:contract` SDK contract depends on no consumer. Nx does
  not read Cargo manifests, so each Rust project restates its crate edges as
  `implicitDependencies` and the same check reconciles them with `cargo
  metadata`.
- **Tiers.** `just check` is the affected tier: every gate target of each
  project the diff can reach since an explicit base (`scripts/nx-base.sh`:
  `NX_BASE` — a plain ref name or a commit SHA, nothing else — or the merge
  base with `origin/main`). `just check all` is the full sweep: every project,
  with the cache skipped so no replayed result stands in for a clean run. The
  paid `live-*` suites and the dispatch-only `explore-*` probes declare no gate
  target, so neither tier ever runs them; each runs from its own workflow.
- **Caching** is on for every deterministic target, keyed by its declared
  inputs, replaying its declared outputs (a Rust `test` replays its merged
  profile, `target/coverage/<record>.profdata`; a `build` its binary). Uncached: `format-write`
  (it writes the tree), `workspace:lint` (the graph it checks is no single
  file), the targets that reach the network (`supply-chain`, `package-crates`,
  `semver-check`, `probe-live`) and the paid or dispatch-only `live`/`explore`
  ones.
- **Rules live with their project:** each project's directory has a nested
  `AGENTS.md` for what governs only it; this file keeps what is repo-wide.

## Command surface

Use the `just` recipes; do not hand-roll equivalents.

- `just bootstrap` — set up from a clean clone (toolchain, llmlint, dependencies,
  and the committed pre-push hook).
- `just check` — the gate's affected tier; `just check all` — the full sweep
  (*Project graph*). Must pass before any commit or PR.
- `just gate` — pre-push superset: `check`, dependency/license audit, crate
  packaging, published-API compatibility, llmlint validation, and its merge-base
  diff judge (skipped locally without Codex/key).
- `just test` / `just lint` / `just fmt-check` / `just coverage` — one slice of
  the gate's targets, affected by default or `all`; `just format` writes. Every
  suite runs under `scripts/check-temp-leaks.sh`, which fails a run that
  abandoned scratch space. `just coverage-html` writes a browsable report to find
  uncovered lines. `just sdk-check`, `just python-sdk-check`, `just e2e` and `just
  lint-workflows` run one project's (or the drift gates') targets alone.
- `just upgrade` — update every ecosystem's one lockfile, then run `just check
  all`.
- `just deps-check` — advisory/license audit (`cargo deny` + `cargo machete`, the
  repo-level `workspace:supply-chain` target); separate from the core gate
  because it needs a network-fetched advisory DB.
- `just semver-check` — refuse an API break the release-driving subject does not
  declare. `release-plz.toml`'s `semver_check` is the later, separate half: it
  settles the VERSION at release time and would raise the bump on a subject that
  announced nothing, leaving the changelog silent about the break.
  A `cargo semver-checks` finding is NOT the verdict: the tree always carries
  the last published version, so every breaking change reads as "requires new
  major" — what decides is whether the subject release-plz will read declares it
  (the PR title via `PR_TITLE` on a pull request, the commits elsewhere) in one
  of the types that actually release. Outside `check` like `deps-check`, and the
  one command needing a newer toolchain than `rust-toolchain.toml` pins. Absent
  tooling skips locally and is RED in CI (`OH_SEMVER_NO_SKIP=1`).
- `just package-crates` — package both crates as Cargo verifies them at publish
  time; deliberately NOT in `check` (`tools/release-tooling/AGENTS.md`).
- `just smoke` — hermetic end-to-end smoke of the built binary (the
  `oneharness-e2e` project's `test` runs it). `just smoke-live` is the opt-in
  variant that hits installed, authenticated harnesses with real model calls —
  never in the gate or CI.
- `just release-probe-live` — opt-in network proof of `scripts/release-probe.sh`
  (`tools/release-tooling/AGENTS.md`).

## Invariants (non-negotiable)

- **The domain layer is pure.** `crates/oneharness-core/src/domain/` builds
  argv, parses output, and shapes the report with no process / filesystem / env
  / clock I/O. All I/O lives in `crates/oneharness-core/src/io/` (spawning, PATH
  resolution, version probing, config/hook file writes) and the binary's
  `src/commands/`. Never hide I/O in a helper that looks pure.
- **Output is a contract.** The JSON report on stdout carries a `schema_version`;
  it is the interface consumers depend on. Add fields; do not repurpose or remove
  them without bumping the version. Diagnostics go to stderr, never stdout. A new
  *value* in an existing enum (a `Status` variant) is a bump too: a consumer that
  matches exhaustively learns of it only from the version. On the history side
  that means a new version constant, since a record's `schema_version` is the
  oldest reader that can understand it — and `history::versions_from(minimum)`
  is how every version-gated field/value states its legal range, so a bump can
  never silently narrow an older one. State the gate in **both** the runtime
  reader (`HistoryRecord::complete`) and `sdk_schema`, and pin it in
  `tests/fixtures/sdk-contract-matrix.json`; the matrix test is what catches the
  two disagreeing.
- **Best-effort `text`, guaranteed envelope.** The execution envelope (command,
  exit code, stdout, stderr, duration, status) is guaranteed and identical across
  harnesses. The normalized `text` field is a convenience whose method is recorded
  in `text_source`; it is `null` when extraction is not possible. Never fabricate
  it — consumers needing certainty parse `stdout`. A timeout does not discard
  output already captured: normalize complete records best-effort while keeping
  status `timeout`; ignore a truncated final JSONL record.
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
  stderr and still run. Ordinary omitted-timeout runs have no deadline, but this
  case alone gets a 120-second approval-wait safety deadline; explicit
  `--timeout 0` removes it. `--permit-prompts` silences the warning. The
  per-harness `read-only`/`plan`
  mapping is drift-alarmed live by `oh_mode_enforce` (writes blocked under
  read-only, allowed under bypass). A no-mutation mode whose mechanism
  enumerates TOOLS must name the ones the run may use, never the ones it may
  not: claude's `--disallowedTools Bash Edit Write NotebookEdit` was complete
  until 2.1.220 put `Task` in the built-in set, and an agent with no `Bash`
  delegated the write to a subagent the deny rules did not reach. The
  equality grid cannot catch that (both paths fail open together), so the floor
  is its own assertion — `control_mode_parity`'s
  `a_no_mutation_mode_withholds_the_capability_to_write`, over the whole
  registry.
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
  assertion. The `crates/oneharness-e2e/tests/cli.rs` config helper also strips ambient overrides;
  keep that property when adding tests or scripts.
- Validate all external / IO inputs (args, stdin, env, subprocess output) at the
  boundary. Keep the artifact portable across Linux, macOS, and Windows.
- Do not commit secrets, credentials, PII, or customer data.

## Scripts and output are context

- Recipes are quiet on success — a line or nothing. On failure they preserve the
  exact error (paths, line/cols, rule names, exit codes) and suggest the next
  action. Treat all command output as context the next agent must read.

## Tests are context engineering

- Tests are how you and future agents see this system behave; invest in them.
- **Coverage is a hard gate**: 95% lines for the Rust crates, over the union of
  every crate's instrumented `test` run (the `rust-coverage` project); 95% for
  the Node SDK (`coverageThreshold = 0.95`) and 95% branch-inclusive for the
  Python SDK (`fail_under = 95`), each in its own `test`. A user-visible change
  ships with a test that fails without it, and the coverage number keeps a
  behavior the tests never execute from slipping in unseen. Find the gaps with
  `just coverage-html`; raise the tests, never lower a floor.
- The execution path is proven **hermetically** by a mock harness binary (the
  `oneharness-mock-harness` crate, around the responder in
  `tests/support/mock_harness.rs`) that oneharness drives via a `--bin` override
  — no network, no real CLI, fully deterministic and cross-platform. Command
  construction is proven by `--print-command` argv assertions covering every
  harness.
- **Scratch space is owned, never left.** A test directory under the host temp
  dir comes from its suite's own scratch guard, never a hand-rolled `mkdir`, so
  a test that fails gives it back like one that passes.
- A user-visible change ships with a test that fails without it.

## Releasing

- Releases are automated from conventional commits by **release-plz**
  (`release-plz.toml` + `.github/workflows/release-plz.yml`), mirroring
  nickderobertis/allowlister. Land conventional commits on `main` (`feat` →
  minor, `fix`/`perf` → patch, `!`/`BREAKING` → major; `docs`/`test`/`chore`/`ci`
  do not release — so commit subjects are load-bearing for both the bump and the
  generated `CHANGELOG.md`). release-plz opens a `release vX.Y.Z` PR that bumps
  `Cargo.toml`/`Cargo.lock` and writes the changelog section, auto-merges it once
  the required checks are green, then `release-plz release` tags `vX.Y.Z` and
  cuts the GitHub Release. **Where the broader tier runs, and why:** release-plz
  batches merges behind its release PR, so the tree that ships is one no merge
  job swept. The full sweep (`just check all`, `scripts/ci-gate-tier.sh`)
  therefore runs on the release PR, and merge-to-main and every other PR run
  the affected tier. `release.yml` reads that sweep's verdict
  (`scripts/ci-verdict.sh`) rather than sweeping the same tree again — the
  merged release PR's run when its head carried exactly the tagged tree, else a
  `workflow_dispatch` of `ci.yml` with `tier=all` on the release tag (the
  manual fallback): a failed check job refuses release; only a complete check
  matrix whose every job ran the full sweep successfully skips the release
  gate; once CI ends, a non-Ubuntu check job without a successful sweep refuses
  release, and an Ubuntu one alone runs the sweep on the Ubuntu release runner.
  An in-flight matrix is awaited to a bound.
  The release publishes both crates in dependency order
  (`oneharness-core`, then `oneharness`), attaches the checksummed cross-platform
  binaries + their Sigstore `.sigstore.json` bundles, and builds/publishes the
  PyPI wheels and npm packages. The bump is not the commit subject's word alone:
  `semver_check = true` reads `oneharness-core`'s actual API surface, so a
  breaking change spelled `fix` cannot ship as a patch.
  So a release lands five ways: **PyPI** (`pip install oneharness-cli`), **npm**
  (`npm install -g oneharness-cli`), **crates.io**, the GitHub Release binaries,
  and `cargo install --git` (`tools/release-tooling/AGENTS.md`,
  `npm/oneharness/AGENTS.md`, `tools/install-surface/AGENTS.md`). Only the
  binary gets a
  `vX.Y.Z` tag + GitHub Release; `oneharness-core` is published and tagged in its
  own `oneharness-core-v{{ version }}` namespace (with `git_release_enable =
  false`, so no GitHub Release) — a distinct namespace is required, else its
  version can collide with a historical binary `vX.Y.Z` tag and release-plz skips
  the engine's `cargo publish` (this exact collision — core 0.3.0 vs the binary's
  old `v0.3.0` — broke the first automated release). Publishing the binary forces
  the engine onto crates.io too: a path dependency must resolve to a registry
  version at publish time, so `Cargo.toml` pins
  `oneharness-core = { path = ..., version = "x.y.z" }` (release-plz keeps that
  `version` in step).
- **Requires two secrets.** The automation runs only once BOTH repo secrets
  exist; the `guard` job no-ops cleanly (no partial release) until then.
  `RELEASE_PLZ_TOKEN` is a PAT (classic or fine-grained, `contents: write` +
  `pull-requests: write`): a tag/Release made with the default `GITHUB_TOKEN`
  would not retrigger `release.yml`, so the binaries would never build.
  `CARGO_REGISTRY_TOKEN` is a crates.io API token used by the downstream release
  workflow; the guard requires it before creating a GitHub Release.
  `CARGO_REGISTRY_TOKEN` is synced from Bitwarden via the
  `gh-secrets.json` manifest (`just secrets-sync`); `RELEASE_PLZ_TOKEN` is a
  GitHub PAT set by hand (a PAT can't live in the harness-auth manifest's
  Bitwarden flow). The crate version and `CHANGELOG.md` are managed by
  release-plz — do not hand-bump them.
- **Manual fallback.** Creating a GitHub Release by hand (the UI, or
  `gh release create vX.Y.Z`) fires the same `release: published` event and builds
  every distribution, including the validated idempotent crates.io job — use it
  only if the automation is wedged, and sweep the tag first (`gh workflow run
  ci.yml --ref vX.Y.Z -f tier=all`), since no release pull request swept that
  tree. Never publish by editing a release mid-flight.
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
