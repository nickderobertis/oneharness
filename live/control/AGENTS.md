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
  does not carry, so `e2e-control.yml` and the consolidated Mac CI job supply
  the credentials and are the
  places those three are proven. The Linux workflow runs for control-source
  changes and on demand, with no schedule: CI's one Mac check already runs the
  same proof on trusted ordinary PRs, so a nightly run would only re-spend paid
  calls on a result nobody reads. In CI `OH_E2E_NO_SKIP` makes a
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

## The run-level allowlist's live proof

`live/control/run_allowlist_live.py` (Python 3.10+), with the budget's
telemetry shape and collector in `tools/network-telemetry/run_overlay_telemetry.py`
(a declared dependency) and its reader in `run_overlay_budget.py`.

- **It is the drift alarm for every harness that applies a run-level
  `allowed_tools`** (Claude Code and Codex; the rest withhold it). Each turn is
  the real CLI against a scripted model endpoint the script serves, so its
  permission check, sandbox and execution are real and no model quota is spent.
  A probe settles only by the tool result the harness hands back; a turn that
  recorded none is a failure, never a pass.
- **Its Codex `--control` half runs from `scripts/e2e-control.sh`'s
  `run-overlay` phase** (`--codex-path control`), which also runs the
  `--budget-telemetry` workload behind `live/control/budgets.yaml`'s
  `codex-run-overlay-added-seconds`. `just live-run-allowlist` runs Claude Code,
  Codex `exec` and OpenCode.
- **No Codex world may reach the network.** A ChatGPT login without
  `features.plugins = false`, `features.apps = false` and a local
  `chatgpt_base_url` fetched the remote plugin catalog and the connectors MCP
  server, was refused there, and refreshed in a loop: `write_codex_home` pins
  all three. The pooled app-server inherits only `CODEX_*`/`OPENAI_*`
  variables, hence `CODEX_SCRIPTED_KEY`.
- **Paths stay short.** A controlled turn's socket and the server pool's live
  under the session dir and `$XDG_STATE_HOME`, both kept under a short `/tmp`
  directory per world: a unix socket address is at most 104-108 bytes.
- **Direct `codex` calls get `stdin=DEVNULL`**, or `codex exec` waits on the
  caller's stdin ("Reading additional input from stdin").
