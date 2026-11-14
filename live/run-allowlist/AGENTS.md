# AGENTS (live-run-allowlist)

Rules for the run-level allowlist's live proof alone
(`live/run-allowlist/run_allowlist_live.py`, `just live-run-allowlist`).

- **It is the drift alarm for every harness that applies a run-level
  `allowed_tools`** (Claude Code and Codex; the rest withhold it). Each turn is
  the real CLI against a scripted model endpoint the script serves, so its
  permission check, sandbox and execution are real and no model quota is spent.
  A probe settles only by the tool result the harness hands back; a turn that
  recorded none is a failure, never a pass.
- **Its Codex `--control` half runs from `scripts/e2e-control.sh`'s
  `run-overlay` phase** (`--codex-path control`), which also runs the
  `--budget-telemetry` workload behind `live/control/budgets.yaml`'s
  `codex-run-overlay-added-seconds`. This suite's own recipe runs Claude Code
  and Codex `exec`.
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
