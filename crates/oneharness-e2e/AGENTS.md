# AGENTS (oneharness-e2e)

Subtree rules for the binary e2e journeys (`tests/cli.rs`), which drive the built
`oneharness` and `oneharness-mock-harness` executables as subprocesses.

- An end-to-end smoke of the *built* binary (`scripts/smoke.sh`, via `just
  smoke`) is part of `just check` and CI: it drives the real artifact through
  `list`/`detect`/`--print-command` plus one mock spawn, fully hermetically —
  proving the shipped binary, not just the test-compiled crate.
- A live cross-harness smoke against real CLIs (`just smoke-live`) is
  deliberately **out** of `just check` and CI: it needs installed binaries, auth,
  and network and makes real model calls. It stays opt-in and skips cleanly when
  no harness is installed.
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
- **A fixture's socket behaviour is a platform's; say which.** A Unix socket
  whose reader shut its read side drops a peer's writes on Darwin (XNU marks the
  buffer `SB_DROP`) where Linux refuses them with `EPIPE`, so a mock that waits
  for that refusal waits forever on macOS. A mock that stops reading a
  connection must close it; `MOCK_CODEX_SHUT_READ_DISCARDS` plays Darwin's
  behaviour on any platform, so Linux runs the macOS case too.
- **Every app-server journey is bounded twice.** Its own waits are bounded
  (`wait_for`, `Pool::finish_within`) so it fails naming what it waited on; and
  `.config/nextest.toml` ends any journey at 180 s as a `TIMEOUT` naming it,
  identically on every platform. Raise that bound only when the slowest passing
  journey, run instrumented, nears it;
  `scripts/app-server-hang-bound-test.sh` (Linux, in this crate's `test`)
  proves it ends a hung journey and leaves none of its processes running.
- **Gate a journey off macOS only for a facility macOS lacks, and name it.**
  The Rust coverage floor is measured on macOS too, from one run; a journey
  gated to Linux takes every line only it reaches out of that measurement (the
  reuse-workload journeys once cost `io/app_server.rs` 150 lines there). Here
  that facility is `/proc` (a workload that runs to its end reads the server's
  memory from it); its failure paths and `PooledSession` itself run everywhere
  (`a_library_pooled_session_*`).
