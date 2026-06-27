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
for **every** harness — as does `--schema` ([structured output](#structured-output),
prompt-based where a harness has no native schema flag). `oneharness list` prints
this registry as JSON, including each adapter's exact command, its `sync_file`,
and `supports_resume` / `supports_native_schema` / `supports_allowed_tools` /
`supports_denied_tools` / `supports_hooks` capability flags.

## Install

```console
# latest prebuilt release for your platform
curl -fsSL https://raw.githubusercontent.com/nickderobertis/oneharness/main/scripts/install.sh | sh
# or pin a release tag / install directory
curl -fsSL https://raw.githubusercontent.com/nickderobertis/oneharness/main/scripts/install.sh \
  | sh -s -- --version v0.1.0 --to ~/.local/bin
# or build from a published release tag
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
The installer honors `ONEHARNESS_VERSION`, `ONEHARNESS_INSTALL_DIR`, and
`GITHUB_TOKEN` (for higher GitHub API rate limits when resolving the latest
release), and refuses to install an archive whose checksum does not match.

## Usage

Six subcommands; `list`/`detect`/`config`/`sync`/`run` emit JSON to **stdout**
(diagnostics go to **stderr**), and `gate` speaks a harness's hook protocol on
stdin/stdout.

```console
oneharness list                                   # describe the registry
oneharness detect --all                           # which harnesses are installed (+ versions)
oneharness config                                 # effective layered config + where each value came from
oneharness sync                                   # merge the unified settings into each harness's own config file
oneharness sync --global                          # install [[hooks]] into the user-global config instead of the project
oneharness run --all --prompt "…"                 # run everywhere, in parallel
oneharness run --harness claude-code,codex --prompt-file task.md
oneharness run --all --print-command --prompt "…" # dry run: show commands, run nothing
oneharness gate claude-code --deny-if-contains X  # the pre-tool gate an installed hook invokes (reads stdin)
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
- `--schema <path>` / `--schema-max-retries <n>` — **structured output**:
  constrain each harness's final answer to a JSON Schema, validate it, and
  re-prompt on failure. See [Structured output](#structured-output) below.
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
- `--mode <read-only|plan|default|edit|auto|bypass>` — the approval mode
  requested from each harness (default `default`; see *Approval modes* below). A
  mode a selected harness **can't express** is a loud usage error before anything
  spawns; one that **may block on a prompt** headlessly is warned about and run,
  with `--timeout` as the backstop.
- `--no-bypass` / `--bypass` — shorthands for `--mode default` / `--mode bypass`;
  `--bypass` forces bypass on over a config's `mode` / `bypass`.
- `--permit-prompts` — silence the "may block on a prompt" warning for the chosen
  mode (use once allow-rules are synced so the prompt never fires).
- `--require-available` — treat a not-installed harness as a failure.
- `--bin <id>=<path>` — override a harness binary (also via `ONEHARNESS_BIN_<ID>`).
- `--config <path>` / `--no-config` — load exactly one config file / ignore all
  config files (see below).
- `--compact` — single-line JSON.

### Configuration

Most `run` flags have a persistent counterpart in **`oneharness.toml`**, so a
project (or a user) states its defaults once instead of repeating flags. Several
sources layer per field, lowest precedence first:

1. **Built-in defaults.**
2. **User-level** — `~/.config/oneharness/config.toml` (honoring
   `$XDG_CONFIG_HOME`; `%APPDATA%\oneharness\config.toml` on Windows), or the
   file named by `$ONEHARNESS_CONFIG`.
3. **Project-level** — the nearest `oneharness.toml` (or `.oneharness.toml`),
   discovered by walking up from the directory the harnesses run in (`--cwd`,
   else the current directory).
4. **Environment overrides** — `ONEHARNESS_<FIELD>` variables (see below); beat
   every config file.
5. **CLI flags** — always win.

Every top-level field with a `run` flag also has a standard
**`ONEHARNESS_<FIELD>`** environment override, the field name upper-snake-cased
so the env var, config key, and flag stay in sync (`model` → `ONEHARNESS_MODEL`,
`schema_max_retries` → `ONEHARNESS_SCHEMA_MAX_RETRIES`). List fields are
comma-separated like their repeatable flags (`ONEHARNESS_HARNESSES=claude-code,codex`),
booleans take `true`/`false` (or `1`/`0`), and an empty value counts as unset. A
malformed value (bad boolean/integer/format, unknown harness id) is the same
loud usage error a file would raise. The sync-policy fields (`allowed_tools`,
`denied_tools`, `hooks`, `settings`), the `[env]` table, and the
`[harness.<id>]` overrides have no env form by design.

```console
ONEHARNESS_MODEL=gpt-5 ONEHARNESS_TIMEOUT=300 oneharness run --harness codex --prompt hi
ONEHARNESS_HARNESSES=claude-code,codex oneharness run --prompt hi   # selection from the env
```

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
bypass = true                   # legacy --bypass toggle (opt-in; default false)
mode = "default"                # --mode; beats `bypass` (default: "default")
timeout = 120                   # --timeout, in seconds
output_format = "json"          # --output-format
schema_file = "person.json"     # --schema (structured output; relative to project)
schema_max_retries = 2          # --schema-max-retries (default 2)
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
oneharness sync --global         # install [[hooks]] into the user-global config (~ / $XDG_CONFIG_HOME)
```

By default `sync` writes the **project** config files. `--global` instead
installs the normalized `[[hooks]]` into each harness's **user-global** location
(`~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.copilot/hooks/…`,
`$XDG_CONFIG_HOME/crush/crush.json`, `$XDG_CONFIG_HOME/opencode/plugin/…`, etc.),
so the gate applies to every project. Permission rules and raw `settings` are
project-scoped only, so configuring them under `--global` is a loud usage error
rather than a silent half-write.

#### The runtime gate (`oneharness gate`)

A normalized `[[hooks]]` entry's `command` is what each harness runs before a
tool call. **`oneharness gate <id>`** is a ready-made such command: it reads the
harness's pre-tool hook event on **stdin**, and — when the event matches
`--deny-if-contains <substr>` — emits that harness's native *deny* verdict on
**stdout** (otherwise nothing, so the call proceeds). It always exits 0, so a
gate never blocks a call on its own error. The per-harness deny shapes are
sourced from each CLI's hook protocol. The decision is a deliberately trivial
substring match: `gate` exists to prove a synced hook is honored end to end (the
live e2e drives a real harness through it), not to be a policy engine — that is
[allowlister](https://github.com/nickderobertis/allowlister)'s role, which
consumes `oneharness-core`'s installer as a library.

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

To opt out: `--config <path>` loads exactly that file and skips discovery (the
`ONEHARNESS_<FIELD>` overrides still apply on top); `--no-config` (or
`ONEHARNESS_NO_CONFIG=1` for wrappers and hermetic test suites) ignores every
config file **and** the env overrides, leaving only flags and defaults. `detect`
honors the configured `bin`s too, so it probes the same binaries `run` would
invoke.

**`oneharness config`** is the debugging surface for the layering: it prints
the effective configuration with every value's provenance — the config file
path that supplied it, `"environment"` for an `ONEHARNESS_*` override, or
`"default"` for a built-in — plus per-key attribution
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
  `--require-available`, was missing; or, under `--schema`, never produced a
  schema-conforming answer).
- `2` — usage/configuration error (bad args, unknown harness, no prompt, an
  unreadable or invalid `--schema` file).

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

### Structured output

`run --schema <path>` constrains each harness's final answer to a [JSON
Schema](https://json-schema.org/) and validates it, so a programmatic consumer
gets a checked JSON value instead of prose to parse. The schema is delivered two
ways, chosen per harness:

- **Native** where the CLI supports it — Claude Code's `--json-schema` (with
  `--output-format json`), which returns the conforming value in its result
  document's `structured_output` field. `supports_native_schema` in `oneharness
  list` flags these.
- **Prompt-based** for every other harness — the schema is appended to the
  prompt as an instruction to emit only a conforming JSON value, which oneharness
  then recovers from the final text (unwrapping a ```` ```json ```` fence or an
  object embedded in prose).

Either way oneharness **validates the result itself** (with the
[`jsonschema`](https://crates.io/crates/jsonschema) crate), so a native flag the
harness ignores is still caught. On a validation failure it re-prompts the
harness with the prior answer and the exact errors, up to `--schema-max-retries`
times (default 2 — so at most `1 + N` invocations per harness). The loop runs
**per harness, in parallel**, so a `--schema` run across many harnesses is still
concurrent.

> Codex CLI also has a native `--output-schema`, but it takes a schema *file*
> and is [reportedly ignored once the agent uses tools](https://github.com/openai/codex/issues/15451),
> so oneharness uses the more reliable prompt-based path for it today. The
> registry's `native_schema` hook makes adding more native deliveries a
> one-line, well-tested change.

Each result gains four fields (all `null` when no `--schema` was given):

- `structured` — the JSON value extracted from the answer and validated. Carries
  the **last-attempted** value even when it failed, so you can see what the
  harness produced; `null` only when no JSON could be extracted at all (never
  fabricated).
- `schema_valid` — `true`/`false` for the final attempt. A `false` here makes the
  run a failure (exit `1`), so you can gate on "did I actually get conforming
  output".
- `schema_attempts` — how many times the harness was invoked under the loop
  (`1 + retries`).
- `schema_error` — the validation errors from the final attempt, joined for
  display; `null` when valid.

The top-level report echoes the applied `schema` and `schema_max_retries`. Both
the schema path and the retry budget are also configurable
(`schema_file` / `schema_max_retries` in `oneharness.toml`).

```console
oneharness run --harness claude-code --prompt "extract the person from auth.py" \
  --schema person.json --compact | jq '.results[0].structured'
```

**Windows note.** A JSON Schema is quote-heavy, and a harness installed as an npm
`.cmd` shim receives its arguments through cmd.exe's `%*` forwarding, which
mangles quote-containing arguments. So on Windows the native `--json-schema`
delivery (and a schema appended to the prompt) may not reach a `.cmd`-shim
harness intact — structured output is most reliable on Linux/macOS, or on Windows
against a real `.exe` harness. oneharness's own argv construction and validation
are exercised on Windows by the hermetic test suite regardless.

### Safety note: bypass by default

A headless agent run hangs waiting for a human to approve tool calls. `run`'s
default mode (`default`) maps each harness to its cleanest *non-interactive*
variant — deny-and-continue, fail-closed, or auto-deny — so it neither hangs nor
blanket-approves; an agent in `default` mode can read and answer but is denied
the tools it would otherwise prompt for. To let it take real actions, pass
`--mode bypass` (or `--bypass`) — the "allow everything" mode — ideally against a
throwaway sandbox (see `--cwd`). `--mode` (below) selects any other point on the
spectrum.

### Approval modes

Every harness has its own approval vocabulary (Claude Code's `--permission-mode`,
Codex's `--sandbox`, Qwen's `--approval-mode`, Goose's `GOOSE_MODE`, …).
`--mode <m>` is oneharness's single spectrum across all of them, from least to
most autonomy:

- **`read-only`** — no mutations; the agent may read but not edit files or run
  commands. *No* plan workflow — it just does whatever read-only work the task
  allows. Mapped to each harness's strongest per-run no-mutation enforcement.
- **`plan`** — like `read-only`, but additionally engages the harness's native
  *plan* workflow (research the task, write a plan, don't act).
- **`default`** — the harness's ask flow, mapped to its cleanest non-interactive
  variant.
- **`edit`** — auto-approve edits, gate commands.
- **`auto`** — auto-approve what the harness deems safe.
- **`bypass`** — approve everything (the default).

The default when nothing is passed is **`default`**. Each mode is mapped to the
harness's own mechanism; `oneharness list` shows the per-harness `modes` (each
tagged `clean` or `hangs`), and the report echoes `permission_mode`. A harness
that **can't express** a requested mode is a loud usage error *before* anything
spawns (there's no command to build). A mode that **may block on a prompt**
headlessly (a `hangs` tag) is warned about on stderr but still run, with the
`--timeout` as the backstop (a real hang becomes a `timeout` result, never an
infinite stall); `--permit-prompts` silences that warning once allow-rules are
synced so the prompt never fires.

| `--mode` | claude-code | codex | opencode | goose | qwen | crush | copilot | cursor |
|------------|:-----------:|:-----:|:--------:|:-----:|:----:|:-----:|:-------:|:------:|
| `read-only`| ✓ᵈ | ✓ˢ | ✓ᵖ | — | ✓ᵖ | — | ✓ᵈ | ✓ |
| `plan`     | ✓ | — | ✓ | — | ✓ | — | ✓ | ✓ |
| `default`  | ✓ | ✓ | ✓ | ✓ | ✓ | ✓¹ | ✓ | ⚠ |
| `edit`     | ✓ | — | ✓ᵉ | — | ✓ | — | ✓ | — |
| `auto`     | ✓ | ✓ | — | ✓ | ✓ | — | — | — |
| `bypass`   | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

✓ supported & clean headless · ⚠ supported but may block on a prompt headlessly
(warns + runs; `--timeout` backstops, `--permit-prompts` silences the warning) ·
— unsupported (refused). `read-only` is **enforced** where marked — ˢ Codex's
read-only sandbox (OS-enforced), ᵈ deny rules (Claude's `--disallowedTools Bash
Edit Write NotebookEdit`, Copilot's `--deny-tool shell/write` — deny beats
allow) — and ᵖ behavioral where its only mechanism is the plan agent (OpenCode
`--agent plan`, Qwen `--approval-mode plan`, so `read-only` and `plan` coincide
there). Cursor's `read-only` is native `--mode ask`. Codex/Goose have no plan
workflow (use `read-only`); Goose's only no-mutation option (`chat`) disables
reads too, so it offers neither plan nor read-only; Crush's `run` can't gate, so
it supports only `default`/`bypass` (¹ it auto-approves the whole session, so the
two are identical). Only **Cursor's `default`** can still block on a prompt (no
fail-fast deny) — every other harness's `default` is clean: it maps to that
harness's cleanest non-interactive variant — Claude Code's `dontAsk`
(deny-and-continue), Codex's read-only exec, Goose's fail-closed `approve`,
Copilot's auto-deny, and OpenCode/Qwen auto-*reject* gated tools and continue
rather than hang. Modes ride the argv except: Goose carries the whole spectrum in
`GOOSE_MODE`, and OpenCode's `edit` (ᵉ) rides the inline-config env var
`OPENCODE_CONFIG_CONTENT` (its per-tool `permission` map has no argv flag).
Copilot's `edit` is a composed `--allow-tool write --allow-tool read` list (shell
omitted → auto-denied); `edit`/`auto` for Cursor remain a `permission` config
concern (`oneharness sync`), not `--mode`.

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
`scripts/smoke.sh`, an end-to-end smoke of the *built* binary, including a
local-release installer check that drives `scripts/install.sh` without network.
To exercise the real harnesses you have installed, run `just smoke-live` — it
makes real model calls, skips any harness that isn't installed, and is
intentionally never part of the gate or CI. See `AGENTS.md` and
`tests/AGENTS.md`.

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

The live check also proves **hook enforcement** the same way: it syncs a
`[[hooks]]` entry whose command is `oneharness gate <id>` into the harness's own
config, then drives the real CLI under bypass (so the hook is the sole decider)
through a marked command (the gate must block it) and an unmarked one (the gate
must let it run). For **Qwen** the gate is synced with `--global` — Qwen only
fires user-scoped hooks headlessly — which also exercises `sync --global` live.
Two harnesses are excluded by design: **Codex** (`oneharness run` drives `codex
exec`, which does not load hooks) and **Copilot** (its project hooks sit behind a
trusted-folder + prompt-mode setup that belongs in allowlister's adapter e2e);
both keep their hermetic install coverage.

Alongside the per-harness checks there is a **per-feature** one for structured
output: `scripts/e2e-schema.sh` (`just live-schema`) drives the real Claude Code
CLI through `oneharness run --schema` and asserts a schema-**valid** round-trip —
it plants a marker, asks for a conforming JSON object carrying it, and checks
`schema_valid == true` with the marker in `.structured`. claude-code is chosen
because it is the one with *native* delivery (`--json-schema` →
`structured_output`); this is the live drift alarm for that flag and field, which
the hermetic suite can only mock. (The portable prompt-based path is harness-
agnostic; any per-harness script can add a live leg by calling
`oh_schema_enforce <id>`.)

```console
just live-claude     # one harness (installs the release binary, runs the live check)
just live-schema     # the structured-output feature (drives claude-code via --schema)
just live-all        # every harness + feature in sequence; skips pass, only real failures fail
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
The structured-output feature has its own (`e2e-schema.yml`), reusing the Claude
auth secret. Locally a missing CLI or auth is a clean **skip**, but those
workflows set `OH_E2E_NO_SKIP=1`, which turns any skip into a hard **failure**:
in CI the harness is installed and auth verified up front, so a skip there can
only mean detection/install/spawn silently broke (classically an unresolved npm
`.cmd` shim on Windows) and the job would otherwise go green having run zero
model calls. A genuine per-platform gap is therefore expressed as a matrix
exclude or an `if`-guarded phase, never a runtime skip.
Every workflow runs a `fail-fast: false` matrix across **Linux, macOS, and
Windows** (`ubuntu-latest`, `macos-latest`, `windows-latest`), so the adapter
argv, JSON contract, and sync/hook enforcement are proven on each platform
independently — the scripts run under bash everywhere (Git Bash on Windows),
and the two `curl | bash` installers (cursor, goose) use their PowerShell
equivalents on Windows. A per-harness model can be overridden with
`<HARNESS>_E2E_MODEL` (e.g. `CLAUDE_E2E_MODEL`, `OPENCODE_E2E_MODEL`).

The one per-platform gap is **cursor hook enforcement on Windows**: cursor-agent
builds its hook command as a PowerShell wrapper but executes it through bash
(Git Bash on `PATH`), so the wrapper dies on a syntax error and cursor blocks
every command. This is an [acknowledged cursor-agent bug][cursor-shell-bug] with
no shell flag, config field, or env lever (`$SHELL` and `$COMSPEC` are ignored;
the only workaround is WSL), so that single phase is skipped on `windows-latest`.
Cursor's echo and sync enforcement still run on Windows, and hook enforcement is
still proven on Linux and macOS. Every other harness's hook enforcement runs on
all three platforms.

[cursor-shell-bug]: https://forum.cursor.com/t/agent-cli-on-windows-no-way-to-configure-shell-hardcoded-to-powershell-no-shell-flag-or-config-option/151858

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
