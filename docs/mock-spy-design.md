# Mock/spy for tool calls, shell commands, and file operations — design notes

Status: **research → v1 implemented** (2026-07-06). Shipped so far:
`oneharness mock <id>` (`domain::mock` + `src/commands/mock.rs`) with the
deny + input-rewrite verbs, the JSONL spy log
(`--spy-file`/`ONEHARNESS_SPY_FILE`), the `mock_rewrite` registry capability
(claude-code + codex `claude-nested`, crush `crush-flat`, cursor
`cursor-permission`, opencode via the plugin shim's `updated_input` args merge
— all verified live by `oh_mock_enforce` and/or the `explore-hooks` probe,
2026-07-06; codex requires the run to opt in via `-c features.hooks=true
--dangerously-bypass-hook-trust`, and cursor's `preToolUse` was added to its
hook binding for this). Live-refuted and deliberately absent: qwen's
documented `updatedInput` (hook fired, verdict emitted, original ran, 3 OSes),
copilot's headless hooks entirely (zero events under `-p`), and claude's
PostToolUse `updatedToolOutput` result replacement (fired, ignored). The
probe ran and its findings are folded in below. Still open, in build order:
result replacement (`replace` — start with opencode's probe-verified
after-hook mutation), `run --mock-rules` ephemeral wiring, hook-sourced
`events` for the transcript-less harnesses, and the `mocks` report block.

Consumer: the cross-harness skill-testing
framework (`nickderobertis/skilltest`), which drives real harnesses through
`oneharness run` and needs to (a) **spy** — observe every tool call a harness
made — and (b) **mock** — substitute canned results for selected tool calls —
in real headless runs, delivered **per-run** (no permanent config mutation).
Whole-harness mocking (the `--bin` test fixture) is explicitly out of scope:
the point is real harness behavior with individual calls intercepted.

Facts below are tagged by provenance:

- **LIVE** — verified in this repo (e2e suite, `explore-events` probe).
- **DOC** — current official docs / source of the harness (July 2026), not yet
  reproduced here.
- **UNVERIFIED** — plausible but unconfirmed; must be settled by the
  `explore-hooks` probe (below) before any registry data depends on it.

Per this repo's standing rule, nothing DOC/UNVERIFIED becomes registry data
until it is probe- or e2e-confirmed ("sourced from real output, never guessed").

## What already exists

- **Spy via output transcripts — shipped** (#1097): `events`/`events_source` on
  every run result normalize tool calls (`{kind, name, input, output, index}`)
  from the harness's own stdout; `--events` selects a transcript-carrying
  format per harness (`HarnessSpec.events_format`); `--stream` delivers events
  incrementally with a consumer short-circuit. LIVE coverage: opencode, cursor,
  claude-code, codex, qwen. goose/crush/copilot emit no headless transcript
  (probe-confirmed), so their `events` stay `null`.
- **Per-tool-call interception — the hook → `oneharness gate <id>` loop**:
  `sync` installs a pre-tool hook into each harness's native config;
  at runtime the harness pipes `{tool_name, tool_input, cwd, session_id}` to
  the gate on stdin and the gate can emit the harness's native **deny**
  verdict (`domain::gate::DenyShape`). Deny-only today. LIVE for claude-code,
  opencode, goose, crush, cursor (project scope) and qwen (user scope); codex
  loads hooks only when the run opts in via `-c features.hooks=true
  --dangerously-bypass-hook-trust` (probe-verified — its mock phase passes
  them); copilot's hooks never fire headlessly (probe-refuted).
- **Ephemeral delivery — proven in the e2e lib**: project-scope hooks
  installed into a `mktemp -d` passed as `--cwd` (die with the sandbox);
  user-scope hooks into a fake `HOME`/`XDG_CONFIG_HOME` passed to both `sync
  --global` and the run via `--env` (`oh_hook_enforce`'s global path). LIVE.
  Per-run env injection is likewise proven (`OPENCODE_CONFIG_CONTENT`,
  `GOOSE_MODE` — highest-precedence inline config with no file touched).

## The gap

Nothing intercepts-and-alters: hooks can only deny, `events` only observes.
The mock feature is: extend the gate seam from deny-only to the verdicts each
harness's hook protocol actually supports, delivered ephemerally per run.

## Mock primitives, by universality

1. **Deny with a model-visible message** — all 8 harnesses (LIVE for the six
   hook-enforced ones). Degraded mock: the model reads the message as tool
   feedback, but the call "failed".
2. **Input rewrite** (pre-tool hook rewrites the tool's arguments) — LIVE on
   5 of 8 (claude-code, codex, opencode, crush, cursor). The workhorse:
   rewrite `git push …` → `printf '%s' '<canned>'` and the model sees exactly
   the fabricated output while the real command never runs; rewrite a read's
   path to a fixture to mock file reads (claude Read redirect LIVE — though
   the model can notice the returned content naming a different file).
   Refuted for qwen (docs say yes, CLI ignores it) and unreachable for
   copilot (hooks never fire); goose has no such verdict.
3. **Result replacement** (the model sees a substituted result) — LIVE for
   opencode only (mutate the `after` hook's output object; the probe surfaced
   the substituted answer). Claude's documented PostToolUse
   `updatedToolOutput` was probe-REFUTED (fired, ignored, v2.1.201) and
   copilot's `modifiedResult` is unreachable (hooks never fire). So the
   composed "rewrite-to-no-op + replace" true mock currently exists only on
   opencode; everywhere else the true mock IS the rewrite-to-stub. OpenCode
   alone also has a genuine no-execution mock: a same-named custom tool
   overrides a built-in (DOC), plus `tools:{bash:false}`.
4. **MCP substitution** (mock MCP server + disable built-ins) — all 8 to
   varying degrees; the only pre-execution fabrication that works everywhere,
   but it changes the tool surface the skill sees. Escape hatch, not default.

**File writes**: prefer sandboxing over interception — let writes really land
in the throwaway `--cwd` and *spy* on them (post-tool events / diff the
sandbox). More faithful to real behavior, zero per-harness support needed.

## Capability matrix

All cells below are settled — the probe ran on PR #1099 (2026-07-06, run
28813236792) and every prior DOC/UNVERIFIED tag was replaced with a LIVE
verdict:

| harness | pre-hook fires headless | payload → spy | rewrite input | replace result | ephemeral delivery |
|---|---|---|---|---|---|
| claude-code | LIVE (project file; `--settings <file>` per-run also LIVE via the probe) | LIVE — Pre + Post, `tool_response` object | `updatedInput` LIVE for Bash **and Read** (file-read redirect verified; model can notice a content/filename mismatch) | **REFUTED live**: PostToolUse `updatedToolOutput` fired but was ignored (v2.1.201) | `--settings <file>` per-run LIVE; temp-cwd LIVE (trust handled by e2e lib) |
| codex | LIVE (v0.142.5, project `.codex/hooks.json`) — **only with `-c features.hooks=true --dangerously-bypass-hook-trust` on the invocation**; the `projects.<dir>.trust_level="trusted"` config route loads no hooks | LIVE — Pre + Post; claude-compatible payload + `turn_id`/`model`; Post `tool_response` is a plain string | `updatedInput` LIVE (claude-nested shape honored) | untested | temp-cwd `.codex/hooks.json` + the two flags LIVE; `CODEX_HOME` (carries auth — seed it) |
| opencode | LIVE (JS plugin) | LIVE — before + after (after carries the result; note: hook sees the PRE-replace output) | mutate `before` `output.args` LIVE | mutate `after` `output.output` **LIVE (probe)** — the substituted result surfaced in the model's answer | `OPENCODE_CONFIG_CONTENT` LIVE; `OPENCODE_CONFIG_DIR` DOC; temp-cwd plugin LIVE |
| goose | LIVE (plugin dir) | LIVE — but **PostToolUse carries no result** (byte-identical fields to Pre; observe-only AND result-blind) | none (probe: claude-style verdict ignored, as expected) | none | temp-cwd `.agents/plugins` LIVE; `GOOSE_PATH_ROOT` DOC |
| qwen | LIVE (user scope; both `QWEN_HOME` and fake-HOME redirects verified) | LIVE — Pre + Post `tool_response.llmContent`; payload carries `timestamp` + dual `tool_use_id`/`tool_call_id` | **REFUTED live twice** (`updatedInput` per docs, ignored under `--yolo` — original ran) | none (context-inject only) | `QWEN_HOME` LIVE; fake-HOME LIVE (auth is env-delivered, so redirect is safe) |
| crush | LIVE (project file; pre only) | LIVE — pre only, **no post event** | `updated_input` LIVE (shallow-merge) | none | temp-cwd `.crush.json` LIVE |
| copilot | **REFUTED live**: repo `.github/hooks/*.json` produced ZERO events across every `-p` experiment, despite real shell tool use and its docs demonstrating `-p` hooks | none (nothing ever fired) | unreachable (hook never fires) | unreachable | n/a until its hooks load at all headlessly |
| cursor | LIVE — **four events per shell call** (`preToolUse`, `beforeShellExecution`, `afterShellExecution`, `postToolUse`); payloads include `user_email`/`cursor_version` | LIVE — post carries `tool_output` (a JSON string) | `updated_input` on `preToolUse` LIVE (snake_case; pre event shows the original, downstream events the rewrite; the model was not told) | MCP tools only (`updated_mcp_tool_output`, DOC) | temp-cwd `.cursor/hooks.json` LIVE (known Windows hook bug applies) |

## Proposed oneharness additions

1. **`oneharness mock <id>`** — the gate's read-write sibling. Reads the hook
   event on stdin, matches a ruleset (JSON file: tool category/name +
   command/path pattern → verb `deny(msg)` | `rewrite(patch)` |
   `replace(output)` | `allow`), renders the harness's **native verdict**.
   Pure decision/render in a new `domain::mock` (mirroring `domain::gate`);
   thin stdin/stdout wrapper in `src/commands/`. Registry grows per-harness
   verdict shapes (a richer sibling of `gate_deny`), each probe-sourced and
   loud when absent — requesting a verb a harness can't express is a usage
   error, never a silent downgrade.
2. **Spy log.** Every `mock`/`gate` invocation appends `{event, verdict}` as a
   JSONL line to a path from env. Two views matter and differ: transcript
   `events` show post-rewrite reality (the stub that ran); the hook log shows
   the **original intent** plus the verdict applied. skilltest needs both to
   assert "agent attempted X and received the mocked response".
3. **Ephemeral wiring inside `run`** (e.g. `run --mock-rules <file>`): pick
   the harness's delivery (per-run flag / temp-cwd project install /
   redirected-home env), install, inject env, clean up — encapsulating what
   `oh_hook_enforce` does in bash. Registry gains an `EphemeralDelivery`
   declaration per harness. Note: claude's `--settings` route relaxes the
   current "policy never rides the argv" test pin — deliberately, for this
   flag only.
4. **Hook-sourced events** for the transcript-less harnesses (goose, crush,
   copilot): surface spy-log entries through the same `ActionEvent` shape with
   an honest `events_source` (e.g. `hook:pretooluse`), filling the `null`
   cells of the events matrix without touching the recognizers.
5. **Report surface**: a `mocks` block per result (rules matched / applied /
   unmatched) — added fields, `schema_version` intact.

## Verification plan

- **`scripts/explore-hooks.sh` + `explore-hooks.yml`** (dispatch-only, this
  branch): per harness, install a hook ephemerally, drive a real turn, dump
  the received payloads and whether rewrite/replace verdicts were honored.
  Settles every UNVERIFIED cell above; mirrors `explore-events.sh`.
- Once shapes are probe-sourced: hermetic pins (mock harness echoes a hook
  event through `oneharness mock`; `domain::mock` unit tests) + an
  **`oh_mock_enforce <id>`** live phase per harness (agent runs a marked
  command; assert the canned output surfaced — in `events`, not just `text`)
  as the drift alarm, like `oh_hook_enforce`/`oh_events_assert`.

## Build order

1. Run the `explore-hooks` probe; update the matrix (and the stale
   codex/copilot notes in AGENTS.md/README/e2e) from its logs.
2. `domain::mock` + `oneharness mock <id>` + registry verdict shapes, starting
   with claude-code and opencode (richest verbs), rewrite-only for
   codex/qwen/crush/cursor, deny-only for goose.
3. Ephemeral delivery integrated into `run` (`--mock-rules`), reusing the e2e
   sandbox/fake-home mechanics.
4. Hook-sourced events for goose/crush/copilot; `mocks` report block.
5. skilltest consumes: `events` + `--stream` for assertions/short-circuit, the
   ruleset + `mocks` block for stubbing.
