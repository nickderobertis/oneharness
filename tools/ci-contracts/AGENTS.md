# AGENTS (ci-contracts)

Subtree rules for the CI and local-gate contracts: the workflow drift gates, the
PR-title lint, CI's tier selection (`scripts/ci-gate-tier.sh`), the setup-just
action, and the manual llmlint gate.

<!-- llmlint: ignore-block[agents_md_durable_and_terse] Moved verbatim from the root AGENTS.md under this change's instruction to move — not rewrite or trim — the text that governs this project (standalone trimming of AGENTS.md is outside its scope); only references to where its checks now run were updated. Its account of the release jobs is where the constraints it states apply; a durability pass over it is a change of its own. -->
- `just gate`'s llmlint judge (`scripts/local-llmlint-gate.sh`):
  The judge is non-deterministic, so its greens are recorded and **replayed**:
  one workspace content plus one resolved base commit plus one judge config is
  judged exactly once, and subsequent manual runs replay the green verdict. Any of the three
  moving re-judges; only a green is ever recorded, so a finding always asks
  again. `ONEHARNESS_LLMLINT_REJUDGE=1 just gate` forces a fresh roll that
  neither reads nor records a verdict. The judge half of that key is read with
  `LLMLINT_ONEHARNESS_BIN` cleared: llmlint renders that override into `llmlint
  config`, but it names the executable dispatching the call rather than what the
  judge asks, and reading it gave every environment a key of its own — which is
  why a publication that wraps oneharness re-rolled the green the working tree's
  own gate had just recorded. A stored verdict replays only as a complete record
  naming the key and base commit it was recorded for; anything less judges again.
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
- **CI picks the gate tier, the recipe runs it.** `scripts/ci-gate-tier.sh`
  reads the event: release-plz's release pull request (head branch
  `release-plz-*`) runs the full sweep, every other pull request and push to
  main the affected tier against an explicit base, a dispatch whichever tier it
  names. The `check` job's step names are a contract `scripts/ci-verdict.sh`
  reads (`Full sweep (just check all)`); `check-ci-gate-tier.sh` drives the
  script with synthetic events and holds ci.yml's wiring, its required context
  names and its unconditioned jobs in place.
- **Required checks install harness CLIs at a pin.** Every harness CLI ci.yml
  installs is pinned in `.harness-cli-versions` (npm `pkg@x.y.z`; Goose via its
  installer's `GOOSE_VERSION` and tagged script); `check-harness-cli-pins.sh`
  holds ci.yml to it. The paid `e2e-*`/`explore-*` workflows gate no merge and
  stay on latest on purpose, so they still catch upstream drift. Cursor's
  installer takes no version and stays unpinned. Move a pin to a version a
  green `e2e-<harness>` run already used.
