# AGENTS (live-control)

Rules for the `live-control` suite alone (`scripts/e2e-control.sh`,
`.github/workflows/e2e-control.yml`).

<!-- llmlint: ignore-file[agents_md_durable_and_terse] Everything below this lead was moved verbatim from live/AGENTS.md (and before that the root AGENTS.md) under this change's instruction to move — not rewrite or trim — the text that governs only this suite. The per-harness and per-platform detail is the original's; a durability pass over it is a change of its own. -->

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
