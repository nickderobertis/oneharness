# AGENTS (shell-coverage)

Subtree rules for the shell line-coverage floor.

<!-- llmlint: ignore-block[agents_md_durable_and_terse] The create-repo bash reference admits a shell floor below 95% only with its reason recorded in AGENTS.md, and this repository's baseline task requires the measurement the approved floor rests on beside it: the figures are the floor's standing justification, which a future raise is compared against, not a session log. -->
- **The floor is 54% lines** (`SHELL_COVERAGE_MIN`, default 54, in
  `scripts/shell-coverage.sh`), below the 95% default as the create-repo bash
  reference allows with a recorded reason, and still enforced: `coverage` fails
  below it. Its basis: a merged kcov 43 measurement of 56.19% over every shell
  script under `scripts/` and `.githooks/` (9662 lines, 48 test steps, Linux).
  Raise floor and basis together, from a fresh `just coverage` run, as tests
  land; never lower it.
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
- **Why it is below 95.** The denominator is every script, so what the gate
  never runs counts at zero: the paid `live-*` suites and `explore-*` probes
  (and most of their shared `e2e-lib.sh`), `install.sh` (POSIX `sh` run by
  dash, which kcov's bash engine cannot trace — rerouting it through bash would
  test the wrong shell), the Rust test and coverage drivers (`cargo-test.sh`,
  `rust-coverage.sh`; tracing under cargo would reach every bash a Rust test
  spawns), and `shell-trace-env.sh`, which turns tracing on and so cannot trace
  itself. Test scripts are in the set too, and their failure arms run only
  when a test fails.
- **Linux only.** kcov ships no binaries, so its pin is a checksummed source
  build (`scripts/shell-tools.sh`) made on Linux alone; macOS and Windows run
  the same shell tests uninstrumented (`scripts/shell-test.sh` runs plain
  `bash` there) and `coverage` prints its skip line.
- **One merge over every step.** Each shell test step is
  `bash scripts/shell-test.sh <project> scripts/<name>.sh [args...]` in some
  project's `test`, which declares `target/coverage/shell/<project>` as an
  output. `coverage` depends on every such `test` (`check-nx-graph.mjs`
  refuses a missing one), reads the steps off the project definitions, refuses
  one that left no report, and merges them with a zero baseline of every
  script, so a new untested script lowers the rate rather than vanishing.
  Every such `test` lists nx.json's `shellTestRunner` named input: every file
  `shell-test.sh` reads, transitively (`check-nx-graph.mjs` derives that set
  from its code and refuses a gap), so a runner change never replays a stale
  report.
- **Tracing must not change what a test sees.** `shell-test.sh` replaces kcov's
  `BASH_ENV` helper with `scripts/shell-trace-env.sh`, which traces only where
  kcov's descriptor is still open and keeps `PS4` valid under `bash -u -c`;
  kcov's own helper put trace lines on stderr and broke `set -u` recipes
  (`check-shell-coverage.sh`, in `shell-toolchain`, holds both).
