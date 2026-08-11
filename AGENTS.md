# AGENTS

`oneharness` is a Rust CLI that drives many agentic coding harnesses
(Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot CLI, Cursor)
through one non-interactive interface and returns one stable JSON shape. Its
consumers are programs, not humans: e2e suites that exercise a feature against
every real harness, and (later) a cross-harness skill-testing framework that uses
this CLI as its driver.

It is a two-crate Cargo workspace: **`oneharness-core`**
(`crates/oneharness-core`) is the reusable engine — the pure `domain` layer and
the `io` boundary, including the harness registry, hook rendering/installation,
config layering, and the sync merge — depending only on serde/toml/thiserror/
which/wait-timeout (never `clap`). The root **`oneharness`** crate is the thin
binary: the `clap` surface (`src/cli.rs`) and per-verb orchestration
(`src/commands/`) over the core. The split exists so sibling tools (e.g.
`nickderobertis/allowlister`) can depend on the engine — most of all
`io::hooks::install`, which writes a normalized hook into any harness's native
config at either project or user-global scope — as a lean git dependency without
pulling the CLI.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`). Edit
> `AGENTS.md` only; the two must never drift.

## Two standing goals on every task

The user drives product features and their request is the priority — but carry
two goals into *every* task. When either is the lowest-error path to what the
user asked, fold it into the same task without asking first; surface the rest as
follow-ups (see "After the main task").

1. **Engineer the context for next time.** Make the next agent (and you) see
   more for less: realistic end-to-end tests that exercise what consumers
   actually observe — especially when a bug existing tests missed surfaces (the
   suite is this repo's only QA loop, see "Tests are context engineering") —
   scripts and skills that automate repetitive steps and shrink their output to
   signal, and terse `AGENTS.md` notes capturing what the code doesn't make
   obvious.
2. **Engineer the codebase and environment.** Be the engineer the user isn't:
   prioritize the technical initiatives that keep the codebase clean,
   maintainable, and repeatable, and keep setup automated and consistent
   (`just bootstrap` from a clean clone). Strict quality gates plus local/CI
   parity make results repeatable (here, `just check` on the pinned toolchain).
   A clean base and a reproducible environment are usually how the user's
   feature ships with a low error rate.

## Stack and composition

- **Product shape:** CLI plus Rust, Node, and Python libraries (`shapes/cli.md`,
  `shapes/library.md`, `intersections/rust-cli.md`).
- **Language(s):** Rust, TypeScript, and Python; Bash is limited to setup/live e2e.
- **References composed:** `base.md`, `shapes/cli.md`, `shapes/library.md`,
  `languages/rust.md`, `languages/typescript.md`, `intersections/rust-cli.md`,
  `ci.md`, `llmlint.md`, `releasing.md`, `monorepo.md`.
- **Cross-cutting:** `ci.md`, `releasing.md`, and `monorepo.md`; root `just`
  delegates to Cargo/Bun without Nx because this small two-package graph is static.
- **Excluded, and why:** web-app/React/Next.js/asdf-plugin/skills-repo guidance
  do not apply; release artifacts are handled by the existing Cargo/GitHub
  Release workflow rather than a separate frontend or plugin distribution.

## Command surface

Use the `just` recipes; do not hand-roll equivalents.

- `just bootstrap` — set up from a clean clone (toolchain, llmlint, dependencies,
  and the committed pre-push hook).
- `just check` — full gate: format check, clippy (`-D warnings`), tests, line
  coverage (hard-gated at 95%), build, smoke. Must pass before any commit or PR.
- `just gate` — pre-push superset: `check`, dependency/license audit, llmlint
  validation, and its merge-base diff judge (skipped locally without Codex/key).
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
- `just test` / `just lint` / `just format` — individual gate steps.
- `just coverage` — run the workspace suite under `cargo llvm-cov` and fail below
  95% line coverage (the `COVERAGE_MIN` gate, also part of `just check` and CI).
  `just coverage-html` writes a browsable report to find uncovered lines.
- `just upgrade` — update dependencies, then re-run `just check`.
- `just deps-check` — advisory/license audit (`cargo deny`); separate from the
  core gate because it needs a network-fetched advisory DB.
- `just smoke` — hermetic end-to-end smoke of the built binary (part of `just
  check` and CI). `just smoke-live` is the opt-in variant that hits installed,
  authenticated harnesses with real model calls — never in the gate or CI.
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
  unauthenticated. Two absences are NOT red, because no credential fixes either:
  a **provider refusal** (`_oh_provider_refusal` / `not_run`) and a declared
  **known gap** (`known_gap` in `e2e-control.sh`). Both are still SAID every run
  — an absence dropped from the verdict is indistinguishable from coverage. A
  refusal is recognized from the provider's own WORDS, on every path a CLI states
  them (`text`, `error`, `stderr`, frames), because no *status* does: a driven
  copilot turn is a clean `ok` that did nothing and its `-p` run exits 1 with the
  message on stderr alone. It is never retried (a quota does not refill inside a
  suite); a *rate limit* is deliberately not one, since that turn may run on the
  next attempt. The control × mode grid is mostly NOT its job: the policy each
  mode sends with and without `--control` is pinned per harness as a unit
  assertion (`domain::control`'s `control_mode_parity`), since a live phase per
  mode would multiply an already-26-minute suite to prove a value. The one live
  phase it does own is `oh_control_mode_enforce` — a controlled turn under the
  gating `--mode default` must END — because whether a harness HONORS the policy
  it was handed is the half a value cannot show, and because a bypass-only suite
  no longer exercises the ACP permission answer at all (copilot's controlled
  launch now carries allow-all, so it stops asking). It is a known gap on
  opencode — see the `--control` notes below.
- `just sdk-check` / `just python-sdk-check` — generated-contract drift, strict
  language lint/type/test coverage, and packed-artifact subprocess e2e for the
  Node and Python SDKs. The Python gate runs on the oldest supported Python 3.9.

## What this binary is

- A thin CLI over a registry of **harness adapters**. Each adapter is data: a
  canonical id, a default binary name, an install hint, an output format, and two
  pure functions — build the argv, and best-effort extract the final text.
- `run` spawns the selected harnesses **in parallel**, each as a subprocess with
  a timeout, and emits one JSON report. `io::process` owns each launcher's whole
  tree (Unix process group; Windows kill-on-close Job Object assigned while the
  child is suspended), applies a brief TERM→KILL grace on Unix, reaps, and bounds
  pipe drain; both buffered and streaming runner paths must go through it so an
  npm wrapper's native child cannot survive or hold inherited pipes open. It also
  owns `resolve_program`, so every spawn — runner and `usage` probe alike — takes
  a bare registry name through the PATHEXT-aware lookup that finds Windows's
  `codex.cmd`; a site that skips it reports an installed harness as
  `program not found`. **Cancellation** goes through that same
  `Finish::Terminate` teardown: `io::cancel` holds a caller-owned `CancelToken`
  plus a process-wide flag raised by `install_signal_cancel` (SIGINT/SIGTERM;
  Windows console-control), which the CLI installs for `run` *after* any stdin
  read. Because the launcher leads its own process group, a signal that killed
  oneharness would orphan a live, billing harness — so both runner loops bound
  their wait/pipe-read by `CANCEL_POLL_SLICE` and re-check the flag. That bound is
  the whole mechanism for a **silent** harness: it emits no line, so `on_line`
  (and therefore `StreamStep::Stop`) is unreachable, and a plain wait to the
  deadline would hold the run for the entire timeout after the caller gave up.
  A cancelled run is `Status::Cancelled` — its own value, never `timeout` (nothing
  was exceeded) and never the streaming consumer-`Stop`'s `ok` — with its captured
  bytes still normalized; queued jobs report cancelled without spawning. Timeout
  status is authoritative, but `commands::run::executed_result` still normalizes
  any complete captured records into text/usage/session/events (skipping a
  truncated JSONL tail), which history then preserves. A **same-prefix batch** is
  the dual shape:
  pass more than one prompt (`--prompt`/`--prompt-file` are repeatable, each one
  whole prompt) and `run` fans **one** harness over the N prompts instead —
  single-harness by nature (a provider cache prefix is per harness/model/tools),
  and not itself a `--resume`/`--fork` continuation (those are a usage error with a
  batch). Pure scheduling lives in `domain::batch` (the `BatchStrategy` waves:
  `speed` = one concurrent wave; `min-tokens` = a one-call warm-up wave then the
  fanned-out rest, with a barrier between). `min-tokens` *reduces tokens* only via a
  **cache-reusing fork** (`HarnessSpec.fork_reuses_cache`, a capability beyond
  `supports_fork`): a static `--system` is NOT reused across separate harness
  processes (Claude Code re-creates a user-supplied `--append-system-prompt` every
  `claude -p`; only its own global prefix gets cross-process cache reads — verified
  live, three experiments). So on a harness whose fork reuses the cache (today
  **claude-code only**) the command layer's `run_fork_batch` runs the warm-up
  (prompt[0], establishing a session that carries `--system`), reads its
  `session_id`, then rewrites the fan-out jobs to `--resume <sid> --fork` (dropping
  `--system`, inherited from the session) so they reuse the warmed prefix.
  **OpenCode is fork-*capable* but `fork_reuses_cache: false`** — its `--fork`
  re-sends the branched conversation cold (fan-out reads no cache, re-writes the
  whole prefix — measured live, so forking it would *raise* tokens). Without a
  cache-reusing fork, `min-tokens` only orders the calls (a stderr warning — no
  reuse). `run_in_waves` covers `speed`/order-only; `run_fork_batch` the fork path.
  Only spawning is I/O, so the warm-then-fan ordering is unit-testable against the
  mock (`MOCK_LOG_FILE` records start/end interleaving; a mock `session_id` drives
  the fork-argv test). Each result carries its own `prompt`; the report's `batch`
  block carries `strategy`/`prompt_count`/`forked`. `--batch-strategy` is a
  per-invocation orchestration knob, so — unlike most `run` flags — it deliberately
  has no config/`ONEHARNESS_*` layer. The live drift alarm that the mode *reduces
  tokens* is `oh_batch_fork_enforce` in `e2e-claude.sh` (the fork fan-out reads the
  warmed prefix and writes less than the warm-up), tied to the `usage` cache counts.
  It is claude-only because claude-code is the only `fork_reuses_cache` harness;
  `e2e-opencode.sh` deliberately omits it (its fork doesn't reuse — see above).
  `list` and `detect` describe and probe
  the registry; `config` shows the effective layered configuration with each
  value's source; `sync` merges the unified policy settings (allow/deny rules,
  hooks, raw `settings` tables) into each harness's **own** config file — project
  by default, or the user-global location under `--global` (hooks only) — so the
  policy also applies without oneharness in the loop. Those five emit JSON to
  stdout by design. `history` (opt-in via `run --history` / `history` config /
  `ONEHARNESS_HISTORY`, off by default) streams a **standardized cross-harness**
  run history — one normalized record per harness run (the report's signals, no
  raw stdout/stderr) — to `<history_dir>/<project-slug>/<session>.jsonl` (one file
  per run; `history_dir` defaults to the platform state dir). It is its own output
  v0.2 contract with its own `domain::history::SCHEMA_VERSION` (independent of
  the report's): each record has a UUIDv7 `history_id` and validated `labels`
  (`history_labels` / `ONEHARNESS_HISTORY_LABELS` / repeated `--history-label`,
  merged by key with CLI > env > project > user precedence). v0.1 remains
  readable with a deterministic UUIDv5 id and empty labels. A validated input the
  SDKs also validate must be stated so the **Rust runtime check and the hand-written
  `JsonSchema` accept the same values** — the schema is the SDK validators' only
  source, so a gap there ships as an SDK that refuses what the CLI takes. Three
  traps, all live in `domain::history`: bound lengths in **characters** (code
  points — the only unit `maxLength` expresses; never bytes); spell a character
  allow-list as a **forbidden unanchored `not` search**, never an anchored
  `^…$` (Python's `re` `$` also matches before a trailing newline, so `"v\n"`
  passes the Python SDK); and keep `char::is_control` (Cc = C0 + DEL + **C1**) and
  its pattern in step. `HistoryId` accepts only canonical hyphenated text with the
  RFC 4122 variant and a defined version — `Uuid::parse_str` is laxer than the
  pattern promises. The shared `tests/fixtures/sdk-contract-matrix.json` is where
  such a rule gets pinned across Rust/Node/Python at once. The record shape +
  slug + name + timestamp formatting are pure (`domain::history`); the clock
  reads that mint the session id/timestamps and all file writes/reads are I/O
  (`io::history`). The writer is **best-effort** — a store that can't be opened or
  a record that can't be written warns on stderr and disables history for the run,
  never taking the results down (like the mock restore). The session `name` is
  oneharness-derived (a slug of the first prompt, or `--history-name`), NOT from
  the harness — headless harnesses expose only an opaque `session_id` (already
  captured per record), never a readable title; don't fabricate one. The report
  echoes the session file as `history_file` (the programmatic handle). The
  `oneharness history list/show/watch/clear` verb views/manages the store: JSON on
  stdout by default (the contract), `--format text` for humans; `show` resolves a
  record UUID exactly before its back-compatible session id/name lookup; `watch`
  emits typed JSONL envelopes with label filters and `--after` cursor resume.
  Its process-locked append-only `.index.jsonl` is reconciled once on startup
  (including partial-tail recovery), then followed by byte offset without repeated
  tree scans. `clear` is a dry run until `--yes`. History paths are canonicalized
  before writing so `cwd=..` remains discoverable.
  <!-- llmlint: ignore-block[agents_md_durable_and_terse, no_redundant_instruction_pointers, comments_earn_their_place] Stating these load-bearing constraints here and deferring them to `docs/harness-usage.md` are the only two arrangements, and one rule in this list forbids each; they stay stated, with the pointer intact. `comments_earn_their_place` is listed because the span covers these directive lines too. -->
  `usage` is the pre-flight verb: subscription headroom per identity, on its own
  output contract, parsers pure (`domain::usage`) and probes I/O (`io::usage`).
  Four constraints are load-bearing; every observed payload and exchange behind
  them lives in `docs/harness-usage.md`. A probe must be **zero-turn** — no user
  message, no completed turn — because a pre-flight check that spends quota
  defeats itself. Each parser carries its own drift guard and degrades an
  unrecognized shape to *unknown*, because a confident wrong headroom number is
  silent where a crash is loud. A probe whose answer is **asynchronous** must
  hold the child's stdin open until that answer lands (`StdinAfterRequests`):
  codex's app-server drops an in-flight reply on EOF, which reported a readable
  45%-used window as unreadable for a whole release, and only the live
  `oh_usage_enforce` phase can catch it — a mock that answers inline cannot.
  And the Cursor probe must keep masking
  `CURSOR_API_KEY` from its child: passing it authenticates rather than selects,
  a hazard any future Cursor dispatch also hits.
  <!-- llmlint: ignore-end[agents_md_durable_and_terse, no_redundant_instruction_pointers, comments_earn_their_place] -->
  `run --control` requires `--session` and exactly one harness; both violations
  are loud usage errors. For every control-capable harness and every
  `PermissionMode` that harness supports, a controlled run must be under exactly
  the policy the same mode gives without `--control` (the codex
  `bypass`→`workspaceWrite` bug was one cell of that grid). The way to keep it
  true is to DELIVER the harness's own mapping into the controlled launch rather
  than re-derive a posture for the protocol: copilot's permission flags ride the
  `--acp` argv beside it, goose's `GOOSE_MODE` already rides the control child's
  job env. Only where nothing can be delivered is a posture answered on the wire,
  and then it is the harness's own (`ModeSpec::posture`) rather than the
  spectrum's — which is why crush's ungated `default` is unattended under control
  too. A mode whose ONLY delivery is the harness's own config environment cannot
  reach a turn submitted to a pooled server — that environment belongs to the
  server process, which this dispatch may not have started — so opencode's `edit`
  is a **loud usage error** under `--control` rather than a turn under whatever
  policy that server already had, and the approval mode stays out of the pool key.
  That is this feature's one **known gap**, and it is NAMED in both places a
  reader looks: its own grid cell
  (`known-gap:mode-env-not-delivered-to-a-pooled-server`) and a phase
  `e2e-control.sh` reports rather than runs. A cell dropped from either reads as
  coverage. Adding a harness or a mode means adding its cell to `control_mode_parity`. Declare `ControlShape` only after a live interrupt
  through oneharness. Stdin control keeps the child stdin open, then closes it
  on `is_turn_terminal`. Dialogue control owns its JSON-RPC child per dispatch:
  codex ends on `turn/completed`, not the `turn/start` response, and ACP must
  answer `session/request_permission`. Dialogue-derived session ids are usable
  only under `--control` (`session_capable_under`). HTTP control submits turns
  through the pooled server, not the harness CLI: permission requests must be
  answered; opencode is terminal only on idle after admission; and cwd — plus
  opencode's MODEL, which its session-create route takes as a required
  provider+id pair — stays a per-turn value. A per-turn setting the wire has no
  place for is refused, never dropped: an opencode session opened without a
  model runs on whatever the server picks, and live that was a free model
  answering 401 on every turn. Its own config does not decide that — `opencode
  serve` loads a `model` from `OPENCODE_CONFIG_CONTENT` and creates sessions on
  another one anyway. Pool keys exclude all per-turn and per-thread settings.
  Readiness is a question about the PROCESS oneharness launched, never about who
  answers at its address: a TCP port is reserved by binding and letting go, so
  between the reservation and the launch it belongs to whoever asks the kernel
  next, and a run that took any answer could be driven against a stranger's
  server (which is how a hermetic control test read `timeout` at random). So a
  server that EXITED during bring-up is said so at once and relaunched once at a
  fresh address; one that is merely SILENT is reported against the window and
  never relaunched.
  `interrupt --input` carries a **redirection** with the abort. Atomic means
  *committed with the abort, delivered at the turn boundary*, never written
  alongside it: every mechanism drops or queues a message sent into a live turn,
  so the run parks it before the abort goes out, hands it back on any failure,
  and opens the next turn itself — through the same frame/route that opened the
  first one, which is why no declared mechanism has to refuse `--input`. So every
  backend must keep its turn (and stdin) OPEN while a redirection is pending; a
  mechanism whose terminal signal ends the run unconditionally would drop it.
  *When* the run learns the aborted turn ended is per mechanism and measured:
  most announce it, but **opencode announces nothing** — its stream just stops,
  so there the served interrupt is the ending and the message goes out as soon as
  the abort lands (`HttpShape::abort_ends_turn_silently`). Interrupting also
  makes the aborted turn's OWN submission fail (opencode answers its held-open
  prompt request with a refusal), and that refusal is not the run's outcome.
  `gate <id>` is the odd one out: the runtime pre-tool gate an
  installed `[[hooks]]` hook invokes, reading a harness's hook event on stdin and
  emitting its native deny verdict on stdout (pure shapes in `domain::gate`). It
  exists to prove a synced hook is *honored* end to end (the per-harness live
  e2e drives a real harness through it), not to be a policy engine — that is the
  sibling `allowlister`'s role, which consumes the `install` library. `mock
  <id>` is its read-write sibling for behavioral test suites (the `skilltest`
  consumer; design in `docs/mock-spy-design.md`): the same hook loop, driven by
  a `--rules` JSON ruleset — rules match on the tool name (`tool`/`tool_regex`),
  the raw event (`event_contains`/`event_regex`), and per-field `input`
  predicates (`equals`/`contains`/`regex` over `tool_input`), all ANDed and
  loud-validated (regexes are the linear-time `regex` crate, compiled at parse
  time) — that can *deny*, *rewrite the tool's input*, or *stub* a shell call
  (declare only the output; oneharness compiles it to a safely-quoted printf
  rewrite — nothing user-authored executes) and appends every
  observed event to a `--spy-file`/`ONEHARNESS_SPY_FILE` JSONL spy log, which
  preserves the *original* pre-rewrite call (the transcript `events` show only
  post-rewrite reality). Decision/verdicts are pure in `domain::mock`; the
  rewrite shape is per-harness registry data (`mock_rewrite`, all verified live
  by `oh_mock_enforce` and/or the `explore-hooks` probe: claude-code and codex
  `claude-nested` — codex's hooks engine needs the run to opt in via a `-c
  features.hooks=true --dangerously-bypass-hook-trust` passthrough — crush
  `crush-flat`, cursor `cursor-permission` (its `preToolUse` event, wired into
  the hook binding for this), opencode via the plugin shim's args merge;
  absent — a loud usage error — for goose, whose protocol can't rewrite, for
  copilot, whose hooks were probe-REFUTED headlessly (zero events under `-p`),
  and for qwen, whose documented `updatedInput` was live-REFUTED — hook fired,
  verdict emitted, original command still ran on all three OSes. Claude's
  documented PostToolUse `updatedToolOutput` replacement was also
  probe-refuted — fired, ignored — so there is no `replace` verb yet; opencode
  after-hook replacement is probe-verified and is where `replace` starts).
  `run --mock-rules <file>` / `run --spy-file <file>` is the single-flag
  ephemeral delivery: per-run argv for claude-code (`--settings` temp file,
  zero mutation), a snapshot-and-restore project-scope install for the rest
  (layers onto existing config via the non-destructive merge; created files
  deleted, created dirs pruned — `io::hooks::HookSnapshot`), codex's opt-in
  flags auto-appended (`MockDelivery` in the registry); qwen/copilot are
  refused loudly (no headless-capable delivery). `oh_mock_enforce` is the live
  drift alarm for both the verdict shape and the ephemeral delivery (it drives
  `run --mock-rules` and asserts zero residue), and it retries once when the
  spy log is empty (an agent refusal — the hook never fired — is flakiness,
  not verdict drift).
- **Structured output** (`run --schema <file>`): constrain each harness's final
  answer to a JSON Schema, validate it (the `jsonschema` crate, pinned
  `default-features = false` so it stays offline), and re-prompt on failure up to
  `--schema-max-retries` (default 2). Two deliveries, per `HarnessSpec.native_schema`:
  *native* where the CLI has a schema flag (only Claude Code's `--json-schema`
  today, value read from `structured_output`), *prompt-based* for the rest (the
  schema is appended to the prompt, the value recovered from the answer text).
  oneharness validates either way, so a native flag the harness ignores is still
  caught. The validate/retry loop lives in the runner as `run_jobs_with` (a pure
  domain closure decides re-runs; the runner owns spawning), so it stays parallel
  across harnesses. Pure logic — schema compile/validate, JSON extraction,
  instruction text, the shared `check` used by both the loop and the report — is
  in `domain::structured`. Like every normalized signal, the structured value is
  **never fabricated**: no extractable JSON is "invalid", not a guess. Codex's
  native `--output-schema` is deliberately *not* wired yet (file-based + ignored
  once tools run, https://github.com/openai/codex/issues/15451); adding it is one
  registry line plus a `build_argv` arm (the pointer comments at the codex
  registry entry and `structured::NativeSchema` say exactly what to change). The
  *per-feature* live e2e (`scripts/e2e-schema.sh` / `just live-schema` /
  `e2e-schema.yml`, helper `oh_schema_enforce`) drives real claude-code through
  `--schema` and asserts a schema-valid round-trip — the drift alarm for the
  native `--json-schema` flag the hermetic suite can only mock. That live check
  is Linux/macOS-only: a JSON Schema is quote-heavy and npm `.cmd` shims mangle
  quote-containing argv via cmd.exe `%*` on Windows (so structured output is
  unreliable against a `.cmd`-shim harness there — documented in the README; the
  hermetic `check` job still covers the Windows argv/validation path). The
  structured-output prompt additions stay **single-line** by convention; this is
  not a spawn constraint — `io::runner` now spawns a multi-line argument against
  a `.cmd`-shim harness by bypassing the shim (`domain::shim::parse_cmd_shim`
  rewrites it to the shim's real target: `node <cli.js>`, or the wrapped `.exe`
  directly, as for claude-code whose bin is `bin/claude.exe`) — but the cmd.exe `%*`
  quote-mangling above is a *separate* limitation the bypass does not touch
  (a quote-heavy schema is single-line, so it never triggers the bypass).

## How this repo was composed

Assembled with the `create-repo` skill from one **product shape** (CLI), one
**language** (Rust), and the **CI** cross-cutting reference. What the skill's
catalog offered but this repo deliberately leaves out — recorded so the choices
aren't re-litigated each session:

- **No `rust-cli` intersection reference** — none exists yet. The overlap it
  would cover (snapshot-testing a compiled binary, cross-platform release
  artifacts) is handled inline by the `--print-command` argv assertions and the
  `release.yml` matrix.
- **No `cargo-dist`** — `release.yml`'s native build matrix already ships
  checksummed cross-platform binaries; cargo-dist's generated pipeline isn't
  worth replacing it. (`release-plz` *is* now used for conventional-commit
  versioning — see *Releasing* — driven directly with a `RELEASE_PLZ_TOKEN` PAT,
  reversing the earlier "hand-versioned only" choice.)
- **crates.io publish** (*now enabled* — reversing the earlier "distribute the
  binary via GitHub Releases / `cargo install --git` only" choice). The tag's
  `release.yml` runs idempotent `cargo publish` for **both** crates in dependency order
  (`oneharness-core` then the `oneharness` binary), so a release ships via
  crates.io *and* the GitHub Release binaries *and* `git`. Publishing the binary
  forces the engine onto crates.io too: a path dependency must resolve to a
  registry version at publish time, so `Cargo.toml` pins
  `oneharness-core = { path = ..., version = "x.y.z" }` (release-plz keeps that
  `version` in step). `oneharness-core` is git-tagged in its own
  `oneharness-core-v{{ version }}` namespace (NOT the binary's `v{{ version }}` —
  a shared scheme collides, and release-plz then mistakes an existing `vX.Y.Z`
  for the engine being already published and skips its `cargo publish`; see
  *Releasing*) and keeps `git_release_enable = false`, so only the binary gets a
  `vX.Y.Z` tag *and* GitHub Release — one `vX.Y.Z` tag still means one thing.
  Needs a `CARGO_REGISTRY_TOKEN` secret alongside the `RELEASE_PLZ_TOKEN` PAT
  (see *Releasing*).
- **PyPI wheels** (*now enabled*, mirroring `nickderobertis/llmlint`). `pyproject.toml`
  uses maturin's `bindings = "bin"` (the ruff/uv pattern) to wrap the prebuilt
  `oneharness` binary in per-platform wheels, so `pip install oneharness-cli` is a
  seconds-fast binary install where PyPI is reachable but github.com may be
  blocked. The PyPI distribution is **`oneharness-cli`** (the bare name was
  unavailable); the console command it installs is still `oneharness`. The wheel
  version is `dynamic` — maturin reads it from `Cargo.toml`, so release-plz stays
  the single version driver (never hand-set a version in `pyproject.toml`).
  `release.yml`'s `build-wheels` job runs on every release (so a packaging break
  surfaces even while publishing is off); `publish-pypi` uses keyless **Trusted
  Publishing** (OIDC, no token secret and **no GitHub Actions environment** — the
  Trusted Publisher is registered without one, so the job must not declare
  `environment:` or the OIDC claim won't match) and stays dormant until the
  `PYPI_PUBLISH` repo variable is `true` and the PyPI project registers this
  repo's `release.yml` as its Trusted Publisher; `verify-pypi` then proves the
  published version is `pip install`-able.
  The typed Python client is a separate pure-Python **`oneharness-sdk`**
  distribution (imported as `oneharness_sdk`, Python 3.9+). Its checked-in
  schemas and types are generated from `sdk_schema::bundle`; runtime inputs are
  strict while output validation preserves additive fields. `scripts/python-sdk-pack.mjs`
  stamps both its package version and exact `oneharness-cli==X.Y.Z` dependency
  from the root `Cargo.toml`, keeping Rust/CLI/Node/Python releases aligned. The
  release workflow builds wheel + sdist on every release, then publishes through
  the already-registered PyPI Trusted Publisher in an environment-free,
  `id-token: write` job only after `oneharness-cli` publishes; no PyPI token is
  stored. `verify-python-sdk` installs the real release and drives `list()`
  through the packaged CLI dependency.
- **npm packages** (*now enabled*, the direct analogue of the PyPI wheels — a
  fifth install path). The npm distribution is **`oneharness-cli`** too (same
  bare-name reasoning), and the command it installs is still `oneharness`.
  `npm/oneharness/` is the committed **launcher** package: its `bin/oneharness.js`
  shim resolves and execs the prebuilt binary, which is carried in a per-platform
  package `@oneharness/cli-<platform>-<arch>` declared as an **optional
  dependency** (with `os`/`cpu` set) so npm installs only the one matching the
  host — the same "carry the native binary, no compile" pattern as
  esbuild/@biomejs and the exact npm mirror of maturin's per-platform wheels.
  `scripts/npm-build.mjs` assembles both shapes: `platform` wraps a target's
  binary in its package; `launcher` stamps the version into the launcher's own
  version *and* every optionalDependency (so they stay in lockstep). The version
  comes from `Cargo.toml` by default (release-plz stays the single version driver,
  like the wheels' `dynamic` version) — never hand-set it in a committed
  `package.json` (the committed versions are the `0.0.0-managed` placeholder,
  replaced at publish). Keep the three platform lists in lockstep: the release
  matrix's Rust targets, `TARGETS` in `npm-build.mjs`, `PACKAGES` in
  `bin/oneharness.js`, and the `optionalDependencies` in the launcher manifest.
  `release.yml`'s `build-npm` job runs on every release (packaging-break alarm,
  like `build-wheels`); `publish-npm` publishes the platform packages first then
  the launcher, authenticating with an **npm token** (the `NPM_TOKEN` secret — an
  automation/granular-access token with publish rights to `oneharness-cli` and the
  `@oneharness` scope, wired through `NODE_AUTH_TOKEN`), and stays dormant until
  the `NPM_PUBLISH` repo variable is `true`; `verify-npm` then proves the
  published version is `npm install -g`-able. (Token, not Trusted Publishing — a
  deliberate choice, unlike PyPI's keyless OIDC.) The launcher's resolve-and-exec
  logic is drift-alarmed hermetically by `scripts/npm-e2e.sh` (assemble the host
  package from the built binary, stage it under the launcher exactly as npm's
  optional-dependency resolution would, run the shim end to end), which
  `scripts/smoke.sh` runs inside `just check`/CI whenever Node is present
  (Node-gated like an external tool — GitHub runners ship Node, a node-less clone
  skips with a notice). `just npm-e2e` runs it standalone.
- **Sigstore release signing + mirror-safe `install.sh`** (*now enabled*,
  mirroring llmlint). `release.yml`'s `upload` job signs each archive with a
  keyless [Sigstore](https://www.sigstore.dev/) build-provenance attestation
  (`actions/attest-build-provenance@v2`, OIDC `id-token` — no secret) and
  publishes the `.sigstore.json` bundle beside the archive. `scripts/install.sh`
  verifies the downloaded archive against a trust root **independent of the
  mirror it came from**, in order: (1) the Sigstore bundle, verified OFFLINE by
  `cosign` → `sigstore` (python) → `gh` (whichever is installed), pinned to this
  repo's `release.yml` signer identity + SLSA-provenance predicate; (2) a SHA-256
  checksum from canonical GitHub — and it **refuses** a checksum that shares the
  mirror's origin (a mirror vouching for its own download is no trust root),
  aborting instead. Never re-introduce a "trust the mirror's own checksum" escape
  hatch. The `verify-attestation` release job runs the exact `cosign`/`sigstore`
  commands `install.sh` uses against the real published bundle — the drift alarm
  for the signing identity/flags. The install path is proven hermetically by
  `scripts/install-e2e.sh` (run in `just check`/CI via `smoke.sh`): independent
  checksum installs, tampered mirror rejected, mirror-origin checksum refused, and
  a stubbed `cosign`/`sigstore`/`gh` proves the Sigstore gate (pass installs, fail
  aborts) without a live signature.
- **No pre-commit/lefthook or direnv** — template baggage. The gate is `just
  check` plus CI on the standard Cargo workspace layout (root binary crate +
  `crates/oneharness-core` library).
- **`.tool-versions` pins `just` only** (read by asdf/mise) so a clean clone can
  resolve the command runner; the Rust toolchain stays on rustup, not asdf.
- **Shell scripts are linted with shellcheck** — an external tool, handled like
  `cargo-deny`: CI installs it and `just lint-sh` (part of `just check`) enforces
  it; install it locally (`apt-get`/`brew install shellcheck`) to run the full
  gate.
- **Coverage is enforced at the skill default, 95% lines** (`COVERAGE_MIN` in the
  justfile), via `cargo llvm-cov` — an external tool, handled like `cargo-deny`
  and shellcheck: CI installs it (`cargo-llvm-cov` + the `llvm-tools-preview`
  rustup component, added by `just bootstrap`) and `just coverage` (part of `just
  check`) fails the gate below the bar. It measures the whole workspace
  (`--workspace`), so the `oneharness-core` engine is gated alongside the binary.
  The threshold is line coverage, not region/branch: the hermetic mock-harness
  suite drives whole user journeys (high-leverage line coverage), and a few
  I/O-failure arms in `crates/oneharness-core/src/io/runner.rs` (spawn/wait
  errors) and `io/config.rs` are intentionally left ungated rather than faked with
  brittle environment manipulation. Measured coverage sits above 95% lines; keep
  new behavior covered rather than lowering `COVERAGE_MIN`. Coverage is a
  platform-independent property of the suite, so it is enforced on Linux/macOS and
  skipped on Windows, where llvm-cov does not attribute the integration tests'
  subprocess-spawned binary coverage (a tooling limitation — the binary reads ~0%
  there). The functional gate still runs on all three platforms; only the coverage
  *measurement* is Linux/macOS.

## Invariants (non-negotiable)

- **The domain layer is pure.** `crates/oneharness-core/src/domain/` builds
  argv, parses output, and shapes the report with no process / filesystem / env
  / clock I/O. All I/O lives in `crates/oneharness-core/src/io/` (spawning, PATH
  resolution, version probing, config/hook file writes) and the binary's
  `src/commands/`. Never hide I/O in a helper that looks pure.
- **Output is a contract.** The JSON report on stdout carries a `schema_version`;
  it is the interface consumers depend on. Add fields; do not repurpose or remove
  them without bumping the version. Diagnostics go to stderr, never stdout. A new
  *value* in an existing enum (a `Status` variant) is a bump too: a consumer that
  matches exhaustively learns of it only from the version. On the history side
  that means a new version constant, since a record's `schema_version` is the
  oldest reader that can understand it — and `history::versions_from(minimum)`
  is how every version-gated field/value states its legal range, so a bump can
  never silently narrow an older one. State the gate in **both** the runtime
  reader (`HistoryRecord::complete`) and `sdk_schema`, and pin it in
  `tests/fixtures/sdk-contract-matrix.json`; the matrix test is what catches the
  two disagreeing.
- **Best-effort `text`, guaranteed envelope.** The execution envelope (command,
  exit code, stdout, stderr, duration, status) is guaranteed and identical across
  harnesses. The normalized `text` field is a convenience whose method is recorded
  in `text_source`; it is `null` when extraction is not possible. Never fabricate
  it — consumers needing certainty parse `stdout`. A timeout does not discard
  output already captured: normalize complete records best-effort while keeping
  status `timeout`; ignore a truncated final JSONL record.
- **Never panic on a harness's behavior.** A missing binary is `skipped`, a
  non-zero exit is `nonzero`, a hang is `timeout` — all are data in the report,
  not a crash. Only true usage/config errors abort with a non-zero process exit.
- **Approval mode is explicit; the default is `default`, not bypass.** The
  normalized spectrum lives in `domain::mode` (`read-only` < `plan` < `default` <
  `edit` < `auto` < `bypass`; `read-only` is no-mutation enforcement without the
  plan workflow, `plan` adds it). The built-in default is `default` — each
  harness's normal posture mapped to its cleanest *non-interactive* variant
  (Claude's `dontAsk` deny-and-continue, Goose's fail-closed `approve`, Copilot's
  auto-deny), so it neither hangs nor blanket-approves; `bypass` ("allow
  everything") is the opt-in (`--mode bypass` / `--bypass`). Each harness
  declares which modes it can express and whether each is headless-`clean` or
  would `hangs` (`HarnessSpec.modes` / `ModeSpec`). The command layer refuses an
  *unsupported* mode before spawning (no command to build — a loud usage error,
  never a silent downgrade); a *supported-but-`hangs`* mode is warned about on
  stderr and still run, with the per-harness `--timeout` as the backstop (a hang
  becomes a `timeout` result, per "never panic on a harness's behavior"), and
  `--permit-prompts` silences the warning. The per-harness `read-only`/`plan`
  mapping is drift-alarmed live by `oh_mode_enforce` (writes blocked under
  read-only, allowed under bypass). A no-mutation mode whose mechanism
  enumerates TOOLS must name the ones the run may use, never the ones it may
  not: claude's `--disallowedTools Bash Edit Write NotebookEdit` was complete
  until 2.1.220 put `Task` in the built-in set, and an agent with no `Bash`
  delegated the write to a subagent the deny rules did not reach. The
  equality grid cannot catch that (both paths fail open together), so the floor
  is its own assertion — `control_mode_parity`'s
  `a_no_mutation_mode_withholds_the_capability_to_write`, over the whole
  registry.
- **Config is layered and loud.** Defaults come from `oneharness.toml` files —
  user level (`$ONEHARNESS_CONFIG` or the platform config dir) under project
  level (discovered upward from `--cwd`/cwd) under the `ONEHARNESS_<FIELD>`
  environment overrides under CLI flags; `[harness.<id>]` beats top-level within
  a file. The env overrides are not handled per-command: `domain::config::from_env`
  parses them into a `FileConfig` layer (pure — it takes a getter closure;
  `io::config` passes `std::env::var`) appended after the files in `load_layers`,
  so they flow through `run`/`detect`/`sync` *and* `config`'s provenance (source
  `"environment"`) for free, and CLI-beats-config already makes CLI beat env.
  Keep the trio in sync: a new top-level field with a `run` flag wants a
  matching `ONEHARNESS_<FIELD>` arm in `from_env` (sync-policy fields,
  `[env]`, and `[harness.<id>]` deliberately have none). Unknown fields, bad
  values, or unknown harness ids are usage errors (exit 2), never ignored.
  Parsing/merging is pure (`crates/oneharness-core/src/domain/config.rs`);
  discovery/reading is I/O (`crates/oneharness-core/src/io/config.rs`). Anything
  that must be hermetic (tests, `smoke.sh`, the e2e scripts) sets
  `ONEHARNESS_NO_CONFIG=1`, which disables the env overrides too, so the
  machine's real config — files *or* `ONEHARNESS_*` — can never reshape an
  assertion. The `tests/cli.rs` config helper also strips ambient overrides;
  keep that property when adding tests or scripts.
- Validate all external / IO inputs (args, stdin, env, subprocess output) at the
  boundary. Keep the artifact portable across Linux, macOS, and Windows.
- Do not commit secrets, credentials, PII, or customer data.

## Adding or changing a harness

A new harness is a new entry in the registry
(`crates/oneharness-core/src/domain/harness.rs`) plus its `build_argv`/`extract`
functions — no changes to `run`, the runner, or the report
shape. When you add one:

- Add a `--print-command` assertion in `tests/cli.rs` pinning its exact argv
  (this is the deterministic, network-free proof the adapter is correct).
- Declare its `modes` (`HarnessSpec.modes`): one `ModeSpec` per
  [`PermissionMode`] the CLI can express, each tagged `clean` or `hangs`
  headless, sourced from that CLI's docs/behavior — never guessed. Every harness
  lists `bypass` and `default`. Map each in `build_argv` (or, when the mode is an
  environment variable like Goose's `GOOSE_MODE`, in `ModeSpec.env`), and prefer
  the cleanest non-interactive variant for `default` (e.g. a deny-and-continue,
  not an interactive prompt). A *behavioral* mode a harness can't express
  natively can be synthesized from **enforcement + an instruction**: set
  `ModeSpec.instruction` (prepended to the prompt by the command layer) and pair
  it with the enforcement `build_argv`/`env` provides — this is how Codex's
  `plan` works (read-only sandbox + a plan instruction). Only do this when the
  enforcement half exists (a plan instruction without read-only enforcement
  wouldn't stop the agent acting). Pin the mode→flag mapping with a `build_argv`
  assertion, and update the *Approval modes* table in `README.md`. For each
  no-mutation mode the harness supports (`read-only`, `plan`), add an
  `oh_mode_enforce <id> <mode>` phase to its `e2e-<id>.sh` (a write blocked under
  `--mode <mode>`, allowed under `--mode bypass`) — the live proof the mapping is
  honored and its drift alarm. If the harness's `edit` auto-approves a write that
  its non-edit posture would deny (copilot, qwen), add `oh_edit_enforce <id>` —
  under `--mode edit` a file-tool edit must succeed, the live proof the edit
  mapping is honored. Its "gate shell" half is NOT asserted live: it isn't
  reliably testable, because a model told to write via shell routes around any
  gate through whatever path the harness still allows (copilot auto-approves
  `echo`, opencode delegates to a `task` subagent, qwen's auto-edit ran it) — that
  half stays argv/env-pinned in the `domain::harness` tests. A mode delivered by
  environment (Goose's `GOOSE_MODE`, OpenCode's `OPENCODE_CONFIG_CONTENT`) is
  pinned hermetically via the mock harness's `MOCK_ECHO_ENV` instead. (`auto`
  likewise has no live drift-alarm: a deterministic cross-harness check would
  hinge on the classifier's model-dependent safe/risky split — it stays
  unit-pinned.)
- Update the harness table in `README.md` — including its config-support
  columns (`model`, `system`, bypass, allow/deny rules, hooks, output format,
  `--resume`), which document how each unified setting reaches (or doesn't
  reach) the harness — and the `supports_*` capability fields in the registry.
  Policy settings (`allowed_tools`/`denied_tools`/`hooks`/`settings`) are
  delivered by `oneharness sync` into the harness's own config file (the
  `SyncSpec` in the registry — file path + key paths, sourced from that CLI's
  docs, never guessed). They follow the loud-absence rule: no mapping means a
  parse error for `[harness.<id>]` fields and an `unmapped` entry in the sync
  report (plus a stderr warning) for top-level ones — never a silent drop. The
  sync merge is non-destructive by contract: unrelated keys untouched, lists
  unioned (idempotent re-sync), unparseable files refused and left intact,
  writes atomic. Keep those properties test-pinned when touching it.
- Declare `supports_resume` / `supports_fork` and map them in `build_argv`,
  sourced from that CLI's headless docs (never guessed). *Resume* is the
  continuation flag (`--resume`, `--session`, or a subcommand like Codex's `exec
  resume <id>`); all current harnesses support it, so `supports_resume` is a
  drift-alarm for a future one that doesn't — when false, the command layer
  rejects `--resume` rather than silently starting fresh. *Fork* (`run --resume
  <id> --fork`) branches a new session from the resumed one and is rare — only
  Claude Code (`--fork-session`) and OpenCode (`--fork`) express it headlessly;
  the rest resume linearly, and `--fork` is a loud usage error for them (`fork`
  implies `resume`, clap-enforced). Pin each mapping with a `build_argv`/`--print-
  command` assertion, and remember the session-id round-trip: if the harness emits
  an id headlessly, teach `signals::extract_session` its field (Codex's
  `thread_id`); if it emits none (Goose, Copilot), the continuation handle is
  caller-supplied (a `--name` / minted UUID) and `session_id` stays `null` — never
  fabricate one. Update the `--resume` column in `README.md`. Also declare
  `session_formats` (every non-empty list implies `supports_resume`): the exact
  output formats that emit the native id, preferred automatic format first; an
  empty list means incapable. `oneharness list` derives `session_capable` from
  this list, so capability can never drift from the transport. The non-empty
  harnesses are exactly the `extract_session` sources — claude-code, opencode,
  codex, cursor, qwen — which is what lets the uniform
  `run --session <name>` handle map a caller-owned name to the harness's native
  token in the session store (`domain::session` decides create-vs-continue,
  `io::session` persists `<state>/oneharness/sessions/<slug>/<name>.json`; the
  command layer feeds a continue's token through the *existing verified* `--resume`
  mapping, so `--session` needs **no** new argv arm). With no explicit output
  format the command layer selects the first `session_formats` entry; an explicit
  CLI/config format still wins only if it appears in the list, otherwise it is a
  loud usage error before spawning. When the list is empty, `--session` is a loud
  usage error (no id to bind a name to) — never a silent fresh start. It is
  single-harness, refuses batch/`--resume`/`--fork`/`--all`, and echoes a `session`
  block `{name, phase, token, store_file}` in the report. The record binds to the
  **variant-qualified** id: a native token is scoped to one identity's session
  store (each variant is its own `env_from` home), and a base id cannot say which
  identity minted it. So `harness_conflict` compares the whole id, a legacy `0.1`
  record starts fresh rather than guessing, the token is captured from — and
  rebound to — the candidate that actually *ran*, and the fallback anchor prefers
  the identity the record already belongs to. A resume no identity can resolve is
  the `session_not_found` kind, which falls through beside `auth`/`quota`; its
  phrasings in `domain::signals` are captures from real CLIs (cursor's is
  deliberately missing, never guessed).
  <!-- llmlint: ignore-block[no_redundant_instruction_pointers] `README.md` is not in the agent-loaded instruction set (only this file is), so naming the sections that go stale is the instruction, not a redirect to one; dropping it is how the two documents drift. -->
  Update the *Session
  handle* section + `session_capable`/`--session` mentions in `README.md`.
  <!-- llmlint: ignore-end[no_redundant_instruction_pointers] -->
  Also
  declare `fork_reuses_cache` (implies `supports_fork`): true only if a forked run
  reuses
  the parent session's prompt-cache prefix, which is what makes a `min-tokens`
  batch save tokens — gate, **measured** by `oh_batch_fork_enforce` not guessed
  (true for claude-code; false for opencode, whose fork re-sends the prefix cold).
  When true, add the `oh_batch_fork_enforce <id>` live phase and update the batch
  support matrix in `README.md`.
- Set its `native_schema` only if the CLI has a real schema flag, sourced from
  that CLI's docs (never guessed) — and pin the injected argv with a
  `--print-command`/`build_argv` assertion. `None` is the right default: the
  prompt-based structured-output path already works for every harness, and
  oneharness validates the result regardless. If a harness reports its conforming
  value somewhere other than the answer text, extend `structured::extract_value`.
- Set its `reasoning` (`ReasoningDelivery`) only if the CLI takes a
  reasoning/thinking-effort setting **on the argv** headlessly, sourced from that
  CLI's docs (never guessed). Three shapes: `Flag("--effort")` for a dedicated
  flag (claude-code, copilot's `--reasoning-effort`), `ConfigKv("model_reasoning_effort")`
  for a `-c key=value` override (codex), or `ModelSuffix` when effort is a
  `-<tier>` suffix baked into the **model id** (cursor's `claude-opus-4-8` +
  `high` → `--model claude-opus-4-8-high`; cursor-agent rejects a bracketed
  `model[effort=…]` — verified live). The value is an **opaque string** the caller
  picks for their model and oneharness forwards verbatim (reasoning effort is a
  provider/model capability with no shared spelling — OpenAI's `reasoning_effort`
  enum vs. Anthropic's thinking-token budget — so it is per-harness delivery, not
  a normalized spectrum; an effort the model rejects surfaces as that harness's
  own `nonzero`, never a guess). `build_argv` is untouched: the command layer
  renders the delivery — appending `ReasoningDelivery::args` to the harness's
  override args (alongside config `args`/passthrough) for the flag/`-c` shapes, or
  decorating the resolved `--model` value via `ReasoningDelivery::model_suffix` for
  the model-suffix shape (which therefore needs a model — `ReasoningNeedsModel`, a
  loud usage error, when none is set; the recorded result `model` stays the plain
  id). It refuses (`ReasoningUnsupported`, a loud usage error) any selected harness
  with `reasoning: None` that has an effective `--reasoning`/config value — never a
  silent drop. `None` is the honest default (opencode/qwen/crush express effort
  only through their own config file — the `sync`-path follow-up; goose has no
  headless knob at all). Wired today: claude-code (`--effort`), codex
  (`-c model_reasoning_effort=`), copilot (`--reasoning-effort`), cursor
  (`ModelSuffix`). Pin the rendered argv with a `--print-command` assertion, add
  the `reasoning`/`supports_reasoning` column to the README matrix, resolve it per
  harness (`[harness.<id>] reasoning`, next to `model`, since effort values are
  provider-specific), and add the `oh_reasoning_enforce <id> <effort>` live phase —
  a real `--reasoning` run must complete cleanly (a bogus effort is best-effort
  evidence of honoring). That live phase matters most for copilot (a history of
  headless features silently not firing — its hooks were probe-refuted) and cursor
  (a forum report says cursor-agent may reject the very bracket syntax its `--help`
  advertises — the phase fails loudly if so). The config/env/CLI trio gained
  `reasoning` / `ONEHARNESS_REASONING` / `--reasoning`.
- Declare its `large_input` (`LargeInput`): how a **large** prompt/system reaches
  the harness without inlining it into the argv (past the OS ceiling → `E2BIG`;
  issue #1115). Three fields, all sourced from the CLI's headless docs, never
  guessed: `prompt_stdin` (the harness reads the user prompt from stdin — add a
  `c.prompt_stdin` arm to `build_argv` that omits the positional and adds any
  stdin-selecting flags, e.g. Claude's `--input-format text`, Goose's `-i -`);
  `system_rides_prompt` (for a harness with no system flag, whose `--system` is
  already prepended to the prompt — so the combined text rides the same stdin);
  and `system_file_flag` (a CLI flag that reads the system prompt from a file,
  Claude's `--append-system-prompt-file`). The command layer materializes/pipes
  only when a value clears the 64 KiB `LARGE_INPUT_THRESHOLD` (small prompts keep
  the byte-identical inline argv, so `--print-command` is unchanged); `build_argv`
  reads the `BuildCtx::system_file`/`prompt_stdin` fields. Pin the stdin/file arms
  with a `build_argv` assertion, add the harness to the README large-prompt
  matrix, and add the `oh_long_prompt_enforce <id>` live phase (a >128 KiB
  prompt+system must round-trip and stay out of `.command`) — the drift alarm that
  the CLI still reads the off-argv input. `LargeInput::NONE` (inline only) is the
  honest default until a stdin/file route is *verified* from a real invocation —
  a large value then stays inline and the command layer warns loudly rather than
  risking a silent E2BIG. All eight harnesses are wired today (cursor's
  stdin-only-prompt path was closed-source, so it was **probe-verified** via
  `scripts/explore-cursor-stdin.sh` + the dispatch-only `explore-cursor-stdin.yml`
  before wiring — the pattern to reuse for the next uncertain CLI).
  <!-- llmlint: ignore-block[no_redundant_instruction_pointers, agents_md_durable_and_terse, comments_earn_their_place] This bullet can state the two rules an adapter author must satisfy or defer them to `docs/harness-usage.md`, and one rule in this list forbids each; it keeps the minimum, with the pointer intact. `comments_earn_their_place` is listed because the span covers these directive lines too. -->
- Declare its `usage` (`UsageSupport`). Every harness must report an honest tier:
  one that cannot report headroom says *which kind* of cannot (no plan quota at
  all, versus a quota with no non-interactive reader), never a `0%` and never an
  omission. A probing tier requires a zero-turn probe sourced from a real
  capture; a probe that sends a user message or completes a turn is disqualified.
  <!-- llmlint: ignore-end[no_redundant_instruction_pointers, agents_md_durable_and_terse, comments_earn_their_place] -->
<!-- llmlint: ignore-block[no_redundant_instruction_pointers] The capability matrix is per-harness data that lives in `README.md` (like the mode, resume, and events tables above); naming the file an adapter author must edit is the instruction, not a deferral of one. -->
- Declare `control` ([`ControlShape`]) only after `scripts/explore-control.sh
  <id>` and `oh_control_enforce <id>` prove a filesystem-level interrupt through
  oneharness; `None` is the default. A new shape must also source how a
  redirection reaches it (the frame/route that opens a turn on the session it
  just aborted) and add `oh_control_redirect_enforce <id>` — the live proof the
  redirected turn actually runs. Keep the probe tables, registry, live suite,
  and README matrix aligned. A sidecar also declares `server` ([`ServerSpec`]).
  Its pool key excludes per-turn and per-thread settings; membership is a lease
  naming a live process identity, never a counter or a bare pid.
<!-- llmlint: ignore-end[no_redundant_instruction_pointers] -->
- Give the harness its `global_hook` (the user-global hook location, for `sync
  --global` / `install` at `Scope::Global`) and its `gate_deny` (how it expresses
  a pre-tool deny when it runs `oneharness gate <id>`). Both are registry data
  sourced from the allowlister adapters, never guessed; both are loud when absent
  (a missing `gate_deny` makes `oneharness gate <id>` a usage error). Pin the new
  deny shape with a `--print`-style assertion in `domain::gate`/`tests/cli.rs`.
  Likewise declare `mock_rewrite` (how `oneharness mock <id>` expresses an
  input-rewrite verdict) ONLY once verified — doc-source the shape, pin it in
  `domain::mock` + the registry test, and add the `oh_mock_enforce <id> [scope]
  [run-args…]` live phase (the rewritten command runs, the original doesn't,
  the spy log keeps the original event; forward any opt-in flags the harness's
  hooks engine needs, as codex's phase does); leave it `None` (a loud usage
  error) until the `explore-hooks` probe proves the CLI honors it headlessly.
- Source the real invocation from a known-good driver — the
  `nickderobertis/allowlister` repo's `run_agent()` / `e2e-*.sh` drivers are the
  reference — rather than guessing flags. (`scripts/smoke.sh --live` here is the
  fast way to confirm a real invocation actually works once installed.)
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
- If the harness's oneharness output format carries a machine-readable **tool
  transcript** (OpenCode's `tool` parts, or the Anthropic content-block stream a
  `stream-json` harness emits — see `extract_events` in `domain::events` and the
  README `events` docs), the normalized `events` array works for free once the
  shape is recognized. A harness with a *new* transcript shape needs a recognizer
  arm in `extract_events` (sourced from a real transcript, never guessed) plus a
  unit test; then add the `oh_events_assert <id> <source> [run-args…]` live phase
  — a tool-using turn must surface at least one `tool_call` event — the honoring
  proof + drift alarm. Events need a **transcript-carrying output format**, which
  `--events`/`--stream` selects per harness via `HarnessSpec.events_format`
  (must not break text extraction — verified live). **Never guess a shape: source
  it from a real transcript** — the `scripts/explore-events.sh` + dispatch-only
  `explore-events.yml` probe dumps every harness's live output to CI logs (run it
  from the Actions tab), which is how the current four recognizers were written;
  re-run it when adding a harness. Coverage today
  (all sourced, all e2e drift-alarmed): opencode (`json`, default),
  cursor (`stream-json`, default, its own `type:"tool_call"` shape), claude-code
  (`--events`→`stream-json`, Anthropic content blocks), codex (`exec --json` by
  default, `command_execution` items), qwen (`--events`→`stream-json`, content
  blocks). Goose/crush/copilot emit only decorative TUI text headlessly (probe-
  confirmed), so `events` stays `null` — correct, not a gap. Forward `--events`
  (or `--stream`) to `oh_events_assert`/`oh_stream_assert` as a run-arg for a
  harness whose transcript needs the upgraded format. Streaming
  (`run --stream`, `io::runner::run_job_streaming` + `events::events_from_value`)
  emits events incrementally so a consumer can short-circuit on bad behavior;
  its lines are the typed Rust `RunStreamEnvelope` contract and
  `oh_stream_assert` is its live proof. It is single-unit only in `parallel` —
  one harness, one model (interleaving); a **fallback chain streams** over both
  axes, since its (harness, model) candidates run in turn. So the streamed
  history attribution is per plan entry, not per selected harness (a model
  fan-out repeats a harness). The constraint is narrower than "one harness":
  stdout must never be committed
  to a candidate the chain then discards. So a fall-through is decided by
  `fallback::RunWork` first — a candidate whose result carries a tool call or
  billed usage (`signals::Usage::reports_billed_work`, the one definition
  `record_reports_work` also classifies a raw record with) ran the task and never
  falls through, whatever its terminal record then says.
  Both drivers read that evidence from the same normalized result, so **streamed
  and buffered chains always select the same candidate**; there is no
  streaming-only rule, and a published line is never retracted (a consumer acts
  on what it reads). `sdk_schema::bundle` is the single Rust
  generation source for that envelope, `HistoryStreamEnvelope`, and the shared
  SDK contracts.

## Scripts and output are context

- Recipes are quiet on success — a line or nothing. On failure they preserve the
  exact error (paths, line/cols, rule names, exit codes) and suggest the next
  action. Treat all command output as context the next agent must read.

## Tests are context engineering

- Tests are how you and future agents see this system behave; invest in them.
- **Coverage is a hard gate at 95% lines** (`just coverage`, run inside `just
  check` and CI, measuring the whole workspace). A user-visible change ships with
  a test that fails without it, and the coverage number keeps a behavior the tests
  never execute from slipping in unseen. Find the gaps with `just coverage-html`;
  raise the tests, never lower `COVERAGE_MIN`. (Rationale and tooling notes: *How
  this repo was composed*.)
- The execution path is proven **hermetically** by a mock harness binary
  (`tests/support/mock_harness.rs`, built behind the `mock-harness` feature) that
  oneharness drives via a `--bin` override — no network, no real CLI, fully
  deterministic and cross-platform. Command construction is proven by
  `--print-command` argv assertions covering every harness.
- An end-to-end smoke of the *built* binary (`scripts/smoke.sh`, via `just
  smoke`) is part of `just check` and CI: it drives the real artifact through
  `list`/`detect`/`--print-command` plus one mock spawn, fully hermetically —
  proving the shipped binary, not just the test-compiled crate.
- A live cross-harness smoke against real CLIs (`just smoke-live`) is
  deliberately **out** of `just check` and CI: it needs installed binaries, auth,
  and network and makes real model calls. It stays opt-in and skips cleanly when
  no harness is installed.
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
  necessity; `scripts/check-e2e-matrix.sh` (the `lint-workflows` step in `just
  check` and CI) is its **drift gate** — it holds the one canonical spelling of
  the contract and fails if any workflow diverges (no `push` trigger;
  claude/codex cross-platform, the rest Linux-only on PR). Add a new harness to
  its `CROSS_PLATFORM`/`LINUX_ONLY` list when you wire its workflow.
- A user-visible change ships with a test that fails without it.

## Releasing

- Releases are automated from conventional commits by **release-plz**
  (`release-plz.toml` + `.github/workflows/release-plz.yml`), mirroring
  nickderobertis/allowlister. Land conventional commits on `main` (`feat` →
  minor, `fix`/`perf` → patch, `!`/`BREAKING` → major; `docs`/`test`/`chore`/`ci`
  do not release — so commit subjects are load-bearing for both the bump and the
  generated `CHANGELOG.md`). release-plz opens a `release vX.Y.Z` PR that bumps
  `Cargo.toml`/`Cargo.lock` and writes the changelog section, auto-merges it once
  the required checks are green, then `release-plz release` tags `vX.Y.Z` and
  cuts the GitHub Release. That Release fires `release.yml`, which re-runs the
  complete gate, idempotently publishes both crates in dependency order
  (`oneharness-core`, then `oneharness`), attaches the checksummed cross-platform
  binaries + their Sigstore `.sigstore.json` bundles, and builds/publishes the
  PyPI wheels and npm packages.
  So a release lands five ways: **PyPI** (`pip install oneharness-cli`), **npm**
  (`npm install -g oneharness-cli`), **crates.io**, the GitHub Release binaries,
  and `cargo install --git` (see the PyPI-wheels, npm-packages, and Sigstore
  bullets under *How this repo was composed*). Only the binary gets a
  `vX.Y.Z` tag + GitHub Release; `oneharness-core` is published and tagged in its
  own `oneharness-core-v{{ version }}` namespace (with `git_release_enable =
  false`, so no GitHub Release) — a distinct namespace is required, else its
  version can collide with a historical binary `vX.Y.Z` tag and release-plz skips
  the engine's `cargo publish` (this exact collision — core 0.3.0 vs the binary's
  old `v0.3.0` — broke the first automated release). See the crates.io bullet
  under *How this repo was composed*.
- **Requires two secrets.** The automation runs only once BOTH repo secrets
  exist; the `guard` job no-ops cleanly (no partial release) until then.
  `RELEASE_PLZ_TOKEN` is a PAT (classic or fine-grained, `contents: write` +
  `pull-requests: write`): a tag/Release made with the default `GITHUB_TOKEN`
  would not retrigger `release.yml`, so the binaries would never build.
  `CARGO_REGISTRY_TOKEN` is a crates.io API token used by the downstream release
  workflow; the guard requires it before creating a GitHub Release.
  `CARGO_REGISTRY_TOKEN` is synced from Bitwarden via the
  `gh-secrets.json` manifest (`just secrets-sync`); `RELEASE_PLZ_TOKEN` is a
  GitHub PAT set by hand (a PAT can't live in the harness-auth manifest's
  Bitwarden flow). The crate version and `CHANGELOG.md` are managed by
  release-plz — do not hand-bump them.
- **Manual fallback.** Creating a GitHub Release by hand (the UI, or
  `gh release create vX.Y.Z`) fires the same `release: published` event and builds
  every distribution, including the validated idempotent crates.io job — use it
  only if the automation is wedged. Never publish by editing a release mid-flight.
- The JSON `schema_version` is independent of the crate version: bump it only when
  the report shape changes incompatibly, and document it in the changelog.

## Keeping the allowlist current

- The agent command allowlist lives in `.claude/settings.json`; the tool enforces
  it. When a new routine command joins the normal build/test workflow, add it to
  the allowlist (kept narrow) instead of re-approving it each session.

## After the main task: refine and hand off

After the requested task, propose only materially-helpful follow-ups (scripts,
`AGENTS.md` constraints, shared skills, tests/fixtures), each with its likely
impact. Skip busywork; if nothing helps, say so.
