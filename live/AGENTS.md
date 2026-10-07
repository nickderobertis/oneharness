# AGENTS (live suites)

Subtree rules for the `live-*` projects: the paid, credential-gated suites that
drive real harnesses. None declares a gate target, so neither tier ever runs one;
each runs from its own `.github/workflows/e2e-*.yml` (and `just live-<id>`). Root `AGENTS.md` still applies.

- The allowlister-style **per-harness** live suite (`scripts/e2e-<id>.sh`,
  `just live-<id>` / `live-all`, `.github/workflows/e2e-<id>.yml`) is the granular
  counterpart to `smoke-live`: each check drives ONE real harness with its own
  model/provider config and asserts the marker round-trips (status ok + marker
  surfaced), so CI gets a per-harness pass/fail. Also out of the core gate; the
  workflows are gated to the canonical repo and non-fork PRs. Auth comes from the
  `gh-secrets.json` manifest (Bitwarden secure notes → `.env` + GitHub Actions
  secrets via `just secrets-sync`); values never enter the repo and `.env` /
  `.gh-secrets-state.json` are gitignored.
- **Live e2e in CI — when it fires, and how to check ONE harness/platform.** The
  live workflows (`e2e-<id>.yml` + `e2e-schema.yml`) run on **`pull_request` and
  `workflow_dispatch` only — never `push: main`**. The on-main run was dropped as
  redundant paid model calls: the release-plz `release vX.Y.Z` PR re-runs the
  suite as the pre-release gate, so main is already covered by the last PR before
  a release. The **PR matrix is deliberately slim** — only **claude-code and codex
  run cross-platform** (ubuntu/macos/windows); every other harness (and the schema
  feature) runs **Linux-only** on PRs. Cross-platform coverage for the rest is
  **on demand**, not automatic. To check a single harness and/or platform, DO NOT
  push a commit — that re-runs the whole PR suite. Instead dispatch the one
  workflow with its `os` input (`all`, or a single `ubuntu-latest` /
  `macos-latest` / `windows-latest`; `default` = the PR matrix): e.g.
  `gh workflow run e2e-goose.yml -f os=windows-latest` (or the GitHub MCP
  `actions_run_trigger` with `workflow=e2e-goose.yml`, `inputs={os: windows-latest}`).
  schema's dispatch offers only ubuntu/macos (its native `--json-schema` argv is
  unreliable through the Windows `.cmd` shim). When adding a harness, keep this
  slim-PR + on-demand-dispatch shape (copy an existing `e2e-<id>.yml` matrix
  block); put a new harness in the Linux-only PR set unless it exercises a
  platform-specific spawn path (like the `.cmd`-shim bypass) worth pinning on
  every PR. GitHub Actions can't centralize the per-workflow dispatch options or
  matrix, so this contract is duplicated across the `e2e-*.yml` files by
  necessity; `scripts/check-e2e-matrix.sh` (in `ci-contracts`' `test`, so in `just
  check` and CI) is its **drift gate** — it holds the one canonical spelling of
  the contract and fails if any workflow diverges (no `push` trigger;
  claude/codex cross-platform, the rest Linux-only on PR). Add a new harness to
  its `CROSS_PLATFORM`/`LINUX_ONLY` list when you wire its workflow.
- A live cross-harness smoke against real CLIs (`just smoke-live`) is
  deliberately **out** of `just check` and CI: it needs installed binaries, auth,
  and network and makes real model calls. It stays opt-in and skips cleanly when
  no harness is installed.
- `just live-control` — the per-feature live turn-control suite: interrupt a real
  multi-step turn on every control-capable harness, prove the work stopped, then
  interrupt again with `--input` and prove the redirected work ran. Slow by
  nature (two 15s freeze windows per harness), so it is opt-in locally and
  outside the gate and the shared per-PR e2e matrix — but NOT outside CI: goose,
  opencode and crush authenticate from provider keys a developer box generally
  does not carry, so `e2e-control.yml` (which supplies all of them) is the only
  place those three are ever proven. It runs on a `pull_request` whose paths
  touch the control feature's own sources, and on demand; a PR touching anything
  else does not run a minute of it. **macOS is on the daily `schedule`, never on
  the pull request**: this feature has broken there three times in ways Linux
  cannot show (`/tmp`→`/private/tmp`, the shorter `sun_path` budget, a
  refusal-reason mismatch), and a second 26-minute leg per control PR is not
  what that is worth. A scheduled failure opens (or comments on) an issue,
  because a schedule has no PR to turn red. In CI `OH_E2E_NO_SKIP` makes a
  harness that drops out for want of a credential RED — without it the suite
  reports success having proven nothing for whichever harnesses went
  unauthenticated. Two absences are NOT red, since no credential fixes
  either: a **provider refusal** (`_oh_provider_refusal` / `not_run`) and a
  declared **known gap** (`known_gap`). Both are still SAID every run — an
  absence dropped from the verdict reads as coverage. A refusal is recognized
  from the provider's own WORDS on every path a CLI states them (`text`,
  `error`, `stderr`, frames), because no *status* does; it is never retried, and
  a *rate limit* is deliberately not one. The control × mode grid is mostly NOT its job: the policy each
  mode sends with and without `--control` is pinned per harness as a unit
  assertion (`domain::control`'s `control_mode_parity`), since a live phase per
  mode would multiply an already-26-minute suite to prove a value. The one live
  phase it does own is `oh_control_mode_enforce` — a controlled turn under the
  gating `--mode default` must END — because whether a harness HONORS the policy
  it was handed is the half a value cannot show, and because a bypass-only suite
  no longer exercises the ACP permission answer at all (copilot's controlled
  launch now carries allow-all, so it stops asking). On opencode it is the known
  gap `known_gap` reports.
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
  the harness can't load a hook through a plain `oneharness run` (Codex loads
  hooks only when the run opts in via `-c features.hooks=true
  --dangerously-bypass-hook-trust` — probe-verified, covered live by its mock
  phase; Copilot's hooks were probe-REFUTED headlessly, zero events), also add the
  `oh_hook_enforce <id> [scope]` phase — it syncs a `oneharness gate <id>` hook
  and proves the real CLI blocks a marked command and runs an unmarked one, the
  honoring proof + drift alarm for the *hook* install (use `global` scope for a
  harness, like Qwen, that only fires user-scoped hooks headlessly). If the
  harness reports provider prompt-cache counts in its usage (today only Claude
  Code and OpenCode — see `extract_usage` in `domain::signals` and the README
  `usage` support matrix), also add the `oh_cache_assert <id>` phase: a second run
  within the cache TTL must surface `cache_read_tokens > 0` — the live drift alarm
  that cache-token extraction still matches the real output shape.
