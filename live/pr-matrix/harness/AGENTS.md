# AGENTS (live per-harness suites)

Rules for the eight `live-<harness>` suites beneath this directory, one per
registry harness.

<!-- llmlint: ignore-file[agents_md_durable_and_terse] Everything below this lead was moved verbatim from live/AGENTS.md (and before that the root AGENTS.md) under this change's instruction to move — not rewrite or trim — the text that governs only the per-harness suites; only the connecting clause naming where a split-off rule now lives is new. The per-harness and per-platform detail is the original's; a durability pass over it is a change of its own. -->

- The allowlister-style **per-harness** live suite (`scripts/e2e-<id>.sh`, `just
  live-<id>` / `live-all`, `.github/workflows/e2e-<id>.yml`) is the granular
  counterpart to `smoke-live`: each check drives ONE real harness with its own
  model/provider config and asserts the marker round-trips (status ok + marker
  surfaced), so CI gets a per-harness pass/fail.
- `scripts/check-e2e-matrix.sh` holds the PR-matrix contract. When adding a harness, keep this slim-PR +
  on-demand-dispatch shape (copy an existing `e2e-<id>.yml` matrix block); put a
  new harness in the Linux-only PR set unless it exercises a platform-specific
  spawn path (like the `.cmd`-shim bypass) worth pinning on every PR. Add a new
  harness to its `CROSS_PLATFORM`/`LINUX_ONLY` list when you wire its workflow.
- Add the per-harness live counterpart: a `scripts/e2e-<id>.sh` (source
  `e2e-lib.sh`; declare its auth env and any model/provider knobs), a
  `live-<id>` just recipe, a `.github/workflows/e2e-<id>.yml` gated like the
  others (a `fail-fast: false` matrix over
  `ubuntu-latest`/`macos-latest`/`windows-latest` with `defaults.run.shell:
  bash`, so the bash scripts run under Git Bash on Windows; any `curl | bash`
  installer needs a PowerShell branch for the Windows leg), and — if it needs a
  secret not already synced — an entry in `gh-secrets.json`. If the harness has
  a `SyncSpec`, also add the `oh_sync_enforce` phases (allow rule executes under
  `--no-bypass`, deny rule doesn't): that live check is the only proof the
  synced file is *honored* and the drift alarm for its format. Unless the
  harness can't load a hook through a plain `oneharness run`, also add the
  `oh_hook_enforce <id> [scope]` phase — it syncs a `oneharness gate <id>` hook
  and proves the real CLI blocks a marked command and runs an unmarked one, the
  honoring proof + drift alarm for the *hook* install (use `global` scope for a
  harness that only fires user-scoped hooks headlessly). If the harness reports
  provider prompt-cache counts in its usage, also add the `oh_cache_assert <id>`
  phase: a second run within the cache TTL must surface `cache_read_tokens > 0`
  — the live drift alarm that cache-token extraction still matches the real
  output shape.
