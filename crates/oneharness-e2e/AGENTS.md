# AGENTS (oneharness-e2e)

Subtree rules for the binary e2e journeys (`tests/cli.rs`), which drive the built
`oneharness` and `oneharness-mock-harness` executables as subprocesses. Root `AGENTS.md` still applies.

- An end-to-end smoke of the *built* binary (`scripts/smoke.sh`, via `just
  smoke`) is part of `just check` and CI: it drives the real artifact through
  `list`/`detect`/`--print-command` plus one mock spawn, fully hermetically —
  proving the shipped binary, not just the test-compiled crate.
- **Pin command construction with `--print-command`.** Every harness adapter has
  an argv assertion in `cli.rs`; that dry-run path is the deterministic proof and
  needs no binary at all. Add one when you add a harness.
- **Assert the contract, not the prose.** Parse the JSON and assert on fields
  (`status`, `exit_code`, `text`, `text_source`); never grep human stderr except
  when the test is specifically about a usage-error message.
- **A printed-path assertion canonicalizes BOTH sides** — `resolved(actual)`
  against `resolved(expected)` (`cli.rs`'s helper), since on macOS a temp path
  the test spelled and the one oneharness prints differ. The exception is an
  assertion pinning a path echoed as the caller spelled it (the `config_files`
  chain), which stays raw on both sides.
- **The suite reads the binaries beside it.** Cargo sets `CARGO_BIN_EXE_*` only
  for a package's own binaries, and this crate has none: its `test` target selects
  `oneharness` and `oneharness-mock-harness` beside it (`scripts/cargo-test.sh
  --with`), so one cargo invocation builds all three into the same profile
  directory under the same instrumentation, and `workspace_bin` reads them from
  there. Paths into the repository go through `repo_root()`, since a test runs
  with this crate's directory as its working directory.
- `test` also replays the journeys with `$TMPDIR` reached through a symlink
  (`scripts/with-symlinked-tmp.sh`, Linux only), the spelling macOS gives every
  temp path, and runs the smoke through `scripts/check-smoke-env.sh`.
