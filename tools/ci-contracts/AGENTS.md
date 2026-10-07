# AGENTS (ci-contracts)

Subtree rules for the CI and local-gate contracts: the workflow drift gates, the
PR-title lint, CI's tier selection (`scripts/ci-gate-tier.sh`), the setup-just
action, and the pre-push/llmlint gate. Root `AGENTS.md` still applies.

- `just gate`'s llmlint judge (`scripts/local-llmlint-gate.sh`):
  The judge is non-deterministic, so its greens are recorded and **replayed**:
  one workspace content plus one resolved base commit plus one judge config is
  judged exactly once, and `pre-push` replays what the working tree's own gate
  already cleared rather than re-rolling a verdict it can lose. Any of the three
  moving re-judges; only a green is ever recorded, so a finding always asks
  again. `ONEHARNESS_LLMLINT_REJUDGE=1 just gate` forces a fresh roll that
  neither reads nor records a verdict. The judge half of that key is read with
  `LLMLINT_ONEHARNESS_BIN` cleared: llmlint renders that override into `llmlint
  config`, but it names the executable dispatching the call rather than what the
  judge asks, and reading it gave every environment a key of its own — which is
  why a publication that wraps oneharness re-rolled the green the working tree's
  own gate had just recorded. A stored verdict replays only as a complete record
  naming the key and base commit it was recorded for; anything less judges again.
- **CI picks the gate tier, the recipe runs it.** `scripts/ci-gate-tier.sh`
  reads the event: release-plz's release pull request (head branch
  `release-plz-*`) runs the full sweep, every other pull request and push to
  main the affected tier against an explicit base, a dispatch whichever tier it
  names. The `check` job's step names are a contract `scripts/ci-verdict.sh`
  reads (`Full sweep (just check all)`); `check-ci-gate-tier.sh` drives the
  script with synthetic events and holds ci.yml's wiring, its required context
  names and its unconditioned jobs in place.
