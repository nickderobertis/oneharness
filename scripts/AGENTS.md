# AGENTS (scripts)

Subtree rules for the repository's tooling scripts.

- **Shell scripts are formatted with shfmt and linted with shellcheck**, the
  versions `.shell-tool-versions` pins, through `scripts/shell-check.sh` (this
  project's `format` and `lint`, part of `just check`). The style is the flags in `shell-check.sh` (`-i 2 -ci`);
  `just format` writes it. `just bootstrap` installs the pinned tools
  (`scripts/shell-tools.sh`); the targets run only those, never a PATH copy.
- **This project owns the scripts; it does not run them.** Every script belongs
  here by path, so a change to one always selects this `lint`. Which project
  RUNS it is declared by that project's target inputs: the drift gates belong to
  `ci-contracts`, `release-tooling`, `e2e-support`, `workspace` and
  `workspace-integration`, the per-crate
  test runner and coverage merge to the Rust projects and `rust-coverage`, the
  installer to `install-surface`, the npm build to `npm-launcher`, and the live
  and exploration drivers to the `live-*` and `explore-*` projects. A new script
  is listed in the inputs of the target that runs it, and a drift script runs
  through `scripts/with-portable-sed.sh` — in a `test`, under
  `scripts/shell-test.sh <project>` so the shell coverage floor reads it
  (`check-workflows-portable-test.sh` holds every project definition to both).
- **The live drivers run on macOS too** (`check (macos-latest)` runs them, and
  the release gate reads it). Bound a command with `oh_deadline`
  (`e2e-lib.sh`), never bare `timeout`: macOS ships none. Make a phase that opens
  a control or app-server socket under `oh_socket_sandbox`: macOS's resolved
  `$TMPDIR` is too long for a socket address. Give `mktemp -d` an explicit
  template, because macOS's bare `mktemp -d` ignores `$TMPDIR`.
- **A live echo turn proves oneharness, not the checkout's context.** A
  harness that loads context files from its cwd upward, its global home, or its
  memory (qwen: every `QWEN.md`/`AGENTS.md` to the git root, plus auto-memory)
  must not run a marker turn from the checkout: this repo's rules swamp a small
  model's reply. Start it through `oh_echo_without_context` (`e2e-lib.sh`),
  which launches from an empty sandbox outside any checkout with every context
  home pointed into it; `check-qwen-context-free.sh` holds it to that.
- **An echo probe asks for the marker alone and runs once.** `oh_prompt` names
  the test suite and requests exactly the marker, with no reassurance about
  trust or secrets (models read that as injection); `oh_assert_echoed` checks
  the marker with no retry. `check-echo-probe.sh` holds the request, the single
  launch, and both verdicts.
- **A live positive control retries only a turn that never acted.** A phase
  whose control needs the model to run a tool may retry when the report shows
  no such tool call, never when the call ran and the outcome was wrong; and it
  calls `oh_run` in the phase's own shell, not inside `$(...)`, or `oh_dump`
  shows the previous phase's report. `check-opencode-session-control.sh` holds
  OpenCode's session_id control to both.
