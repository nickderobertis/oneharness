# oneharness

One CLI across many agentic coding harnesses. `oneharness` drives **Claude Code,
Codex, OpenCode, Goose, Qwen Code, Crush, GitHub Copilot CLI, and Cursor** through
a single non-interactive interface, runs them **in parallel**, and returns **one
stable JSON shape** built for programmatic consumers.

It exists to make cross-harness automation boring: instead of hand-rolling a
`run_agent()` for each tool — different flags, different output, different
"don't prompt me" switch, different skip-if-not-installed dance — you call one
command and read one JSON document.

```console
$ oneharness run --all --prompt "Reply with the single word: pong" --model haiku
```

```jsonc
{
  "schema_version": "0.1",
  "oneharness_version": "0.1.0",
  "prompt": "Reply with the single word: pong",
  "model": "haiku",
  "resume": null,
  "bypass_permissions": true,
  "dry_run": false,
  "config_files": ["/home/me/.config/oneharness/config.toml"],
  "results": [
    {
      "harness": "claude-code",
      "bin": "claude",
      "available": true,
      "status": "ok",
      "exit_code": 0,
      "duration_ms": 1840,
      "command": ["claude", "-p", "Reply with…", "--permission-mode", "bypassPermissions", "--output-format", "json"],
      "output_format": "json",
      "text": "pong",
      "text_source": "json:result",
      "usage": { "input_tokens": 1234, "output_tokens": 8, "cost_usd": 0.0095 },
      "usage_source": "json",
      "session_id": "0f3c…",
      "failure_kind": null,
      "failure_kind_source": null,
      "stdout": "{\"type\":\"result\",\"result\":\"pong\"…}",
      "stderr": "",
      "error": null
    },
    { "harness": "codex", "available": false, "status": "skipped", "error": "`codex` not found on PATH; harness skipped. Install it: npm install -g @openai/codex", "…": "…" }
  ]
}
```

## Supported harnesses

The table doubles as the **config support matrix**: each column after the
binary is a unified setting (CLI flag and/or `oneharness.toml` field) and shows
how — or whether — it reaches that harness.

| id | CLI | default binary | `model` | `system` | bypass mode requested | synced config file | allow / deny | hooks | output format | `--resume` |
|----|-----|----------------|:-------:|----------|-----------------------|--------------------|:------------:|:-----:|:-------------:|:---------:|
| `claude-code` | Claude Code | `claude` | ✓ | native flag | `--permission-mode bypassPermissions` | `.claude/settings.json` | ✓ / ✓ | ✓ | ✓ | `--resume` |
| `codex` | OpenAI Codex CLI | `codex` | ✓ | prepended | `--dangerously-bypass-approvals-and-sandbox` | — | — | — | — | — |
| `opencode` | OpenCode | `opencode` | ✓ | prepended | `--dangerously-skip-permissions` | `opencode.json` | via `settings` | — | ✓ | `--session` |
| `goose` | Goose | `goose` | — | native flag | (runs unattended) | — | — | — | — | — |
| `qwen` | Qwen Code | `qwen` | ✓ | prepended | `--yolo` | `.qwen/settings.json` | ✓ / ✓ (interactive) | — | — | — |
| `crush` | Crush | `crush` | ✓ | prepended | `run -q` (non-interactive) | `crush.json` | ✓ / ✓ | — | — | — |
| `copilot` | GitHub Copilot CLI | `copilot` | ✓ | prepended | `--allow-all-tools --allow-all-paths --no-ask-user` | — | — | — | — | — |
| `cursor` | Cursor CLI | `cursor-agent` | ✓ | prepended | `--force` (`--trust` under `--no-bypass`) | `.cursor/cli.json` | ✓ / ✓ | — | ✓ | `--resume` |

- **`model`** — ✓ means the harness takes a model flag. Goose selects its model
  from its own provider config, so `model` is intentionally not mapped for it.
- **`system`** — "native flag" means the system prompt maps to a real flag
  (Claude Code's `--append-system-prompt`, Goose's `--system`); "prepended"
  means the harness has no such flag, so the text is prepended to the prompt —
  it always reaches the model, never silently dropped.
- **synced config file** — the project-scoped file `oneharness sync` merges the
  unified settings into. Because the policy lands in each harness's *own*
  config, it also governs the tools when used directly — oneharness is not in
  the loop at run time. Codex and Goose read only user-global config, and
  Copilot takes permission rules only as flags (deliverable via
  `[harness.copilot] args`), so they have no sync target.
- **allow / deny** — whether `allowed_tools` / `denied_tools` lists have a
  place in that file, in each harness's own rule syntax: Claude Code, Qwen, and
  Cursor use `permissions.allow` / `permissions.deny`. Qwen's rules govern its
  *interactive* approval flow only — live testing showed its headless mode
  never auto-approves from settings (only the `-y` CLI flag executes
  approval-gated tools), so synced qwen rules protect regular usage, not
  headless runs. Crush uses
  `permissions.allowed_tools`, with deny mapped to `options.disabled_tools`
  (the tool is hidden entirely — its strongest deny). OpenCode's `permission`
  is a policy map, not a list, so the lists are rejected for it — express it
  with `[harness.opencode.settings]` instead. A rule aimed at a harness with no
  mapping is a parse error (per-harness fields) or reported `unmapped` (top
  level) — never silently dropped.
- **hooks** — Claude Code's `hooks` table in `.claude/settings.json`. Other
  harnesses keep hooks in places oneharness doesn't manage yet (Copilot's
  `.github/hooks/`, Cursor's `hooks.json`, OpenCode's JS plugins).
- **output format** — ✓ means the harness takes a format flag the
  `output_format` setting maps onto; a `—` harness emits plain text and the
  setting only changes how `text` is extracted.
- **`--resume`** — the flag each adapter maps `run --resume <session>` onto
  (Claude Code, OpenCode, and Cursor today); a `—` means the harness has no
  headless continuation flag, so `--resume` is rejected for it rather than
  silently starting fresh.

The remaining unified settings — `timeout`, `env`, `bin`, per-harness `args`,
`cwd`, selection — are enforced by oneharness itself at run time, so they work
for **every** harness. `oneharness list` prints this registry as JSON,
including each adapter's exact command, its `sync_file`, and `supports_resume`
/ `supports_allowed_tools` / `supports_denied_tools` / `supports_hooks`
capability flags.

## Install

```console
# from a published release tag (reproducible)
cargo install --git https://github.com/nickderobertis/oneharness --tag v0.1.0 --locked
# or from a clone
cargo install --path .
# or just build the release binary
just build-release            # -> target/release/oneharness
```

Each tagged release also publishes prebuilt, checksummed binaries for Linux,
macOS, and Windows on its [GitHub Releases](https://github.com/nickderobertis/oneharness/releases)
page. Building from source requires a stable Rust toolchain and
[`just`](https://github.com/casey/just).

## Usage

Five subcommands, all emitting JSON to **stdout**; diagnostics go to **stderr**.

```console
oneharness list                                   # describe the registry
oneharness detect --all                           # which harnesses are installed (+ versions)
oneharness config                                 # effective layered config + where each value came from
oneharness sync                                   # merge the unified settings into each harness's own config file
oneharness run --all --prompt "…"                 # run everywhere, in parallel
oneharness run --harness claude-code,codex --prompt-file task.md
oneharness run --all --print-command --prompt "…" # dry run: show commands, run nothing
```

Useful `run` flags:

- `--all` / `--harness <id,…>` / `--exclude <id,…>` — selection.
- `--prompt <text>` or `--prompt-file <path|->` — the prompt (file or stdin).
- `--model <m>` — passed to each harness that supports a model flag.
- `--system <text>` — portable system prompt for **every** harness: mapped to a
  native flag where one exists (Claude Code's `--append-system-prompt`, Goose's
  `--system`), and prepended to the prompt otherwise, so the instructions always
  reach the model.
- `--resume <session>` — continue a prior session, sending the prompt as its next
  turn. **Single-harness only** (a session belongs to one harness) and only for
  harnesses that support it (`supports_resume` in `oneharness list`); other
  selections are a usage error rather than a silent fresh session. The continued
  `session_id` is surfaced on each result (see below).
- `--output-format <text|json|stream-json>` — override the format requested from
  each harness (default: per-harness); affects the emitted flag and how `text` is
  extracted.
- `--output-dir <dir>` — also write each harness's raw stdout/stderr to
  `<dir>/<harness>.stdout` and `<dir>/<harness>.stderr` (read transcripts from
  files without a JSON parser).
- `-- <args…>` — extra arguments appended verbatim to each harness command (for
  single-harness runs, since flags differ per harness).
- `--timeout <secs>` — per-harness timeout (default 120); a hang becomes a
  `timeout` result, not a stuck process.
- `--cwd <dir>` / `--env KEY=VALUE` — run each harness in a directory / with extra
  env (useful for sandboxed e2e).
- `--max-parallel <n>` — cap concurrency (default: all selected at once).
- `--no-bypass` — do **not** request bypass mode (see the safety note below);
  `--bypass` forces it back on over a config's `bypass = false`.
- `--require-available` — treat a not-installed harness as a failure.
- `--bin <id>=<path>` — override a harness binary (also via `ONEHARNESS_BIN_<ID>`).
- `--config <path>` / `--no-config` — load exactly one config file / ignore all
  config files (see below).
- `--compact` — single-line JSON.

### Configuration

Most `run` flags have a persistent counterpart in **`oneharness.toml`**, so a
project (or a user) states its defaults once instead of repeating flags. Two
levels exist and layer per field, lowest precedence first:

1. **Built-in defaults.**
2. **User-level** — `~/.config/oneharness/config.toml` (honoring
   `$XDG_CONFIG_HOME`; `%APPDATA%\oneharness\config.toml` on Windows), or the
   file named by `$ONEHARNESS_CONFIG`.
3. **Project-level** — the nearest `oneharness.toml` (or `.oneharness.toml`),
   discovered by walking up from the directory the harnesses run in (`--cwd`,
   else the current directory).
4. **CLI flags** — always win.

Within one file, a `[harness.<id>]` value beats the top-level value for that
harness. Every field is optional, and an unknown field or harness id is a loud
usage error (exit 2), never silently ignored. The run report's `config_files`
array records exactly which files shaped a run.

```toml
# oneharness.toml — every field optional; shown with its CLI counterpart
harnesses = ["claude-code", "codex"]  # --harness (or `all = true` for --all)
exclude = ["cursor"]            # --exclude (applies to an `all` selection)
model = "gpt-5"                 # --model
system = "Be terse."            # --system
bypass = true                   # `false` ≙ --no-bypass (default true)
timeout = 120                   # --timeout, in seconds
output_format = "json"          # --output-format
max_parallel = 4                # --max-parallel
require_available = false       # --require-available
allowed_tools = ["Bash(git log:*)"]  # synced into each harness's config file
denied_tools = ["Bash(rm:*)"]        # (see `oneharness sync` below)

# A normalized pre-tool hook, fanned across every synced harness and rendered
# into each one's native shape (a shared config file, a dedicated hooks file,
# or a plugin). `{harness}` is replaced with the harness id. Unlike the
# verbatim `[harness.<id>.hooks]` table below, this reaches ALL harnesses.
[[hooks]]
command = "mygate hook {harness}"   # required; {harness} → claude-code, codex, …
matcher = "Bash"                    # optional tool-name matcher (harness dialect)
timeout = 10                        # optional; honored where the schema has one
# plugin_name = "mygate"            # optional identity for plugin/Copilot files
# harnesses = ["claude-code"]       # optional; default = every synced harness

[env]                           # --env, for every harness
RUST_LOG = "warn"

[harness.claude-code]           # per-harness: beats the top level for this id
model = "claude-sonnet-4-5"     # each harness can name its own model
bin = "/opt/claude"             # like --bin (the flag and ONEHARNESS_BIN_* win)
args = ["--max-turns", "6"]     # extra argv appended for this harness only
allowed_tools = ["Bash(git:*)", "Read"]  # this harness's rule syntax
env = { ANTHROPIC_LOG = "debug" }

# Lifecycle hooks, in the harness's own hooks schema, synced into its config
# file (Claude Code's .claude/settings.json `hooks` key) uninterpreted.
[harness.claude-code.hooks]
PreToolUse = [{ matcher = "Bash", hooks = [{ type = "command", command = "./validate.sh" }] }]

# Raw settings merged verbatim into a harness's config file — the escape
# hatch for shapes the unified fields don't model, like OpenCode's
# permission policy map.
[harness.opencode.settings.permission]
edit = "deny"
bash = { "git *" = "allow" }
```

### Syncing harness configs

`allowed_tools`, `denied_tools`, `hooks`, `settings`, and the top-level
`[[hooks]]` are **sync settings**: instead of being passed on each invocation,
**`oneharness sync`** merges them into each harness's *own* project config file
(the *synced config file* column in the matrix). That makes oneharness a
config-sync dev tool: state the policy once in `oneharness.toml`, run `sync`,
and it governs Claude Code, Cursor, Qwen, crush, and OpenCode even when they're
used directly — oneharness is not needed at run time.

Hooks come in two forms. A `[harness.<id>.hooks]` table is written *verbatim*
in that harness's own hooks schema, so it only reaches harnesses whose hooks
live in the config file oneharness already syncs (Claude Code). A top-level
`[[hooks]]` entry is **normalized**: oneharness renders it into each harness's
native shape and delivers it the right way for that harness — merged into a
shared file (Claude Code, Qwen, crush), written to a dedicated hooks file
(Codex, Cursor, Copilot), or installed as a plugin (Goose's manifest +
`hooks.json`, OpenCode's JS shim). One `[[hooks]]` entry therefore installs the
same gate into **all eight** harnesses. The per-harness install appears under a
`hooks` array in each entry of the `sync` JSON report.

```console
oneharness sync                  # write/merge the harness config files in this project
oneharness sync --check          # CI mode: exit 1 (writing nothing) if out of sync
oneharness sync --harness claude-code --cwd ~/proj
```

The merge is deliberately conservative:

- **Unrelated keys are never touched** — objects merge per key, and only the
  keys oneharness manages are written.
- **Lists union** — existing entries keep their order and place; missing ones
  are appended. Re-syncing is therefore idempotent (`sync` adds and updates,
  it never removes — delete by hand or edit the harness file directly).
- **Scalars oneharness manages take the config's value** — the unified config
  is the source of truth for the keys you declared, and only those.
- **Unparseable files are refused, untouched** — a JSONC file with comments,
  say, fails loudly rather than being rewritten without them. Writes are
  atomic (temp file + rename), and an existing higher-precedence variant
  (crush's `.crush.json`) is merged into rather than shadowed.
- **Nothing is dropped silently** — a setting with no mapping for a harness is
  a parse error (per-harness fields) or surfaced as `unmapped` in the JSON
  report plus a stderr warning (top-level fields).

To opt out: `--config <path>` loads exactly that file and skips discovery;
`--no-config` (or `ONEHARNESS_NO_CONFIG=1` for wrappers and hermetic test
suites) ignores every config file. `detect` honors the configured `bin`s too,
so it probes the same binaries `run` would invoke.

**`oneharness config`** is the debugging surface for the layering: it prints
the effective configuration with every value's provenance — the config file
path that supplied it, or `"default"` for a built-in — plus per-key attribution
for `[env]` and per-field attribution for each `[harness.<id>]` section. It
takes the same `--cwd`, `--config`, and `--no-config` as `run`, so it shows
exactly what a run from that directory would load:

```console
$ oneharness config --cwd ~/proj | jq '{config_files, model, timeout}'
{
  "config_files": ["/home/me/.config/oneharness/config.toml", "/home/me/proj/oneharness.toml"],
  "model": { "value": "gpt-5", "source": "/home/me/proj/oneharness.toml" },
  "timeout": { "value": 30, "source": "/home/me/.config/oneharness/config.toml" }
}
```

Which settings can reach which harness is the support table above: `model`,
`system`, bypass, and output format are per-harness capabilities; `timeout`,
`env`, `bin`, and `args` are enforced by oneharness and work everywhere.

### Exit codes

- `0` — every selected harness was `ok` or `skipped` (or it was a dry run).
- `1` — at least one harness `nonzero`/`timeout`/`spawn-error`ed (or, under
  `--require-available`, was missing).
- `2` — usage/configuration error (bad args, unknown harness, no prompt).

### The result envelope vs. the normalized signals

The execution envelope — `command`, `exit_code`, `duration_ms`, `status`,
`stdout`, `stderr` — is **guaranteed and identical** across harnesses.

Alongside it, oneharness lifts a few **best-effort** signals out of each
harness's bespoke stdout so consumers don't have to parse it per harness. Each is
`null`/empty when it can't be found, is **never fabricated**, and (where there's
more than one possible method) records how it was found:

- `text` / `text_source` — the final assistant message, normalized to one clean
  string across harnesses (`json:result` for Claude Code's terminal event,
  `json:opencode-parts` for OpenCode's JSONL text parts, `stream-json:result` for
  Cursor, `raw` for a plain-text harness, …). **`text` is a convenience, not a
  guarantee: it is `null` whenever extraction isn't possible, and `text_source`
  is then `null` too.** A consumer that needs certainty reads the guaranteed
  `stdout` — when `text` is `null`, `stdout` is the fallback that always carries
  the harness's real output.
- `usage` / `usage_source` — `{ input_tokens, output_tokens, cost_usd }`, each
  field independently `null` when the harness doesn't report it (cost is commonly
  absent on subscription auth). The `usage` object is always present so the shape
  is stable for cross-harness cost/latency tables. `usage_source` records the
  method: `json` for a harness that reports a whole-run total in one event (Claude
  Code), `json:summed-steps` for one that reports per-step usage that oneharness
  sums (OpenCode). Cursor does not emit token usage today, so its `usage` stays
  `null`.
- `session_id` — the handle a harness exposes for continuation, read from either
  the snake_case `session_id` (Claude Code, Cursor) or camelCase `sessionID`
  (OpenCode); feed it back via `run --resume <session>` (single-harness, supported
  harnesses only) to drive a faithful multi-turn against the real agent.
- `failure_kind` / `failure_kind_source` — on a non-zero run, a coarse reason
  (`auth`, `rate_limit`, `model_not_found`, `quota`) so a caller can tell a
  retryable condition from a broken request. This is **distinct from `status`**,
  which only records oneharness's relationship to the process.

Coverage is keyed off each harness's documented output shape — Claude Code's
`result` JSON, OpenCode's JSONL (`text` parts for the answer, `step_finish` for
usage), Cursor's `stream-json` — and widens as more shapes are sourced; an absent
signal is the honest answer, not an error. Consumers that need certainty should
parse `stdout` themselves.

### Safety note: bypass by default

A headless agent run hangs waiting for a human to approve tool calls, so `run`
requests each harness's "don't prompt / allow everything" mode by default. That
is the right default for automation but means the agent can take real actions —
run it against throwaway sandboxes (see `--cwd`), or pass `--no-bypass` to leave
each harness's normal permission flow intact.

Relatedly, a harness can carry a small **default environment** so headless runs
stay clean — e.g. oneharness sets `QWEN_CODE_SUPPRESS_YOLO_WARNING=1` for Qwen
Code so its `--yolo`/no-sandbox startup warning doesn't litter `stderr`. These
defaults are per-harness data in the registry, and an explicit `--env KEY=VALUE`
always overrides them.

## Why it exists

[`nickderobertis/allowlister`](https://github.com/nickderobertis/allowlister)
verifies its policy engine against **every** real agent CLI. Each check had its
own bash `run_agent()` — Claude wants `-p … --permission-mode bypassPermissions
--output-format stream-json`, OpenCode wants `run --dangerously-skip-permissions
--format json`, Codex wants `exec --dangerously-bypass-approvals-and-sandbox`, and
so on — plus
its own timeout, output capture, and skip-if-missing logic.

`oneharness` collapses that to one call per check:

```bash
# before: ~40 lines of harness-specific bash per agent
# after:
result="$(oneharness run --harness claude-code \
  --prompt "$prompt" --cwd "$proj" --timeout 150 --compact)"
status="$(jq -r '.results[0].status' <<<"$result")"
```

The same uniform interface is the intended driver for a future **cross-harness
skill-testing framework**: set up a sandbox, fire one prompt at every harness via
`oneharness run --all`, and assert on the JSON.

## Development

```console
just bootstrap   # toolchain components + fetch (works from a clean clone)
just check       # full gate: fmt-check, clippy -D warnings, shellcheck, tests, build, smoke
just test        # tests only
just smoke       # hermetic end-to-end smoke of the built binary
just run -- list # run the CLI through cargo
```

The gate uses [`just`](https://github.com/casey/just) (pinned in `.tool-versions`
for asdf/mise users) and [`shellcheck`](https://github.com/koalaman/shellcheck)
for the shell scripts; CI installs both, so install `shellcheck`
(`apt-get`/`brew install shellcheck`) to run the full gate locally.

Tests are hermetic: the subprocess path is exercised against a mock harness
fixture (no network, no real CLI), and every adapter's command construction is
pinned with `--print-command` assertions. `just check` also runs
`scripts/smoke.sh`, an end-to-end smoke of the *built* binary. To exercise the
real harnesses you have installed, run `just smoke-live` — it makes real model
calls, skips any harness that isn't installed, and is intentionally never part
of the gate or CI. See `AGENTS.md` and `tests/AGENTS.md`.

## Live end-to-end testing

`just smoke-live` is the quick "does any installed harness work" check. The
**per-harness** suite is the allowlister-style counterpart: each
`scripts/e2e-<harness>.sh` drives one *real* harness through `oneharness` with
that provider's model/auth and asserts the JSON contract end to end — it plants
a high-entropy marker, asks the harness (via `oneharness run`) to echo exactly
that marker, and asserts `status == ok`, `exit_code == 0`, and that the marker
surfaced. So a pass means the model genuinely ran, not just that the process
exited. A missing CLI or missing auth is a **skip**, never a failure.

For the sync-capable harnesses (Claude Code, OpenCode, Qwen, Crush, Cursor)
the live check also proves **sync enforcement** end to end: it syncs an
allow + deny policy into the harness's own config file, then drives the real
CLI with `--no-bypass` — the allowed `touch` must execute (the positive
control) and the denied one must not. This is the only tier that can prove a
synced file is *honored*, not merely written; it doubles as the drift alarm
for the encoded config formats.

```console
just live-claude     # one harness (builds the release binary, runs the live check)
just live-all        # every harness in sequence; skips pass, only real failures fail
```

Each harness needs its CLI installed and that provider's auth in the environment:

| harness | install | auth env var(s) |
|---------|---------|-----------------|
| `claude-code` | `npm i -g @anthropic-ai/claude-code` | `CLAUDE_CODE_OAUTH_TOKEN` (or `ANTHROPIC_API_KEY`) |
| `codex` | `npm i -g @openai/codex` | `OPENAI_API_KEY` |
| `opencode` | `npm i -g opencode-ai` | `ANTHROPIC_API_KEY` (or `OPENAI_API_KEY`) |
| `goose` | [installer](https://block.github.io/goose/docs/getting-started/installation) | `OPENAI_API_KEY` + `GOOSE_PROVIDER`/`GOOSE_MODEL` |
| `qwen` | `npm i -g @qwen-code/qwen-code` | `OPENAI_API_KEY` (+ optional `OPENAI_BASE_URL`) |
| `crush` | `npm i -g @charmland/crush` | `ANTHROPIC_API_KEY` (or `OPENAI_API_KEY`) |
| `copilot` | `npm i -g @github/copilot` | `COPILOT_GITHUB_TOKEN` |
| `cursor` | [installer](https://docs.cursor.com/en/cli/overview) | `CURSOR_API_KEY` |

Per-harness CI workflows (`.github/workflows/e2e-*.yml`) run the same checks,
each gated to the canonical repo and non-fork PRs so secrets are never exposed.
A per-harness model can be overridden with `<HARNESS>_E2E_MODEL` (e.g.
`CLAUDE_E2E_MODEL`, `OPENCODE_E2E_MODEL`).

### Secrets

The auth above is managed with [`gh-secrets`](https://github.com/nickderobertis/github-secrets):
[`gh-secrets.json`](gh-secrets.json) is a committed manifest that pulls each
secret from Bitwarden (secure notes) and pushes it to two destinations — a local
`.env` (for `just live-*`) and the repo's GitHub Actions secrets (for the
workflows). `COPILOT_GITHUB_TOKEN` is sourced from the `GH_TOKEN` vault item.

```console
just secrets-sync    # gh-secrets manifest sync: Bitwarden -> .env + GitHub Actions
```

The manifest names *which* secrets go *where*; the values never touch the repo.
`.env` and the sync-state file are gitignored.

## Releasing

Releases are versioned by hand and built by CI. To cut one:

```console
# 1. bump the version + changelog, commit, and land on the default branch
#    (edit Cargo.toml `version`, move CHANGELOG [Unreleased] to the new version)
# 2. tag the release commit and push the tag
git tag v0.2.0 && git push origin v0.2.0
```

Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`, which runs the
gate, creates the GitHub Release with generated notes, and attaches archived,
sha256-checksummed binaries for Linux, macOS, and Windows. There is no
crates.io publish step.

## License

MIT — see [LICENSE](LICENSE).
