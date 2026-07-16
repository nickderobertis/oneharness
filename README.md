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
  "fork": false,
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
      "usage": { "input_tokens": 1234, "output_tokens": 8, "cache_read_tokens": 7, "cache_write_tokens": null, "cost_usd": 0.0095 },
      "usage_source": "json",
      "session_id": "0f3c…",
      "events": null,
      "events_source": null,
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

| id | CLI | default binary | `model` | `system` | `reasoning` | bypass mode requested | synced config file | allow / deny | hooks | output format | `--resume` (continue / fork) |
|----|-----|----------------|:-------:|----------|-------------|-----------------------|--------------------|:------------:|:-----:|:-------------:|:---------:|
| `claude-code` | Claude Code | `claude` | ✓ | native flag | `--effort` | `--permission-mode bypassPermissions` | `.claude/settings.json` | ✓ / ✓ | ✓ | ✓ | `--resume` + `--fork-session` |
| `codex` | OpenAI Codex CLI | `codex` | ✓ | prepended | `model_reasoning_effort` | `--dangerously-bypass-approvals-and-sandbox` | — | — | — | — | `exec resume <id>` (linear) |
| `opencode` | OpenCode | `opencode` | ✓ | prepended | config only | `--dangerously-skip-permissions` | `opencode.json` | via `settings` | — | ✓ | `--session` + `--fork` |
| `goose` | Goose | `goose` | — | native flag | — | (runs unattended) | — | — | — | — | `--resume --name` (linear)¹ |
| `qwen` | Qwen Code | `qwen` | ✓ | prepended | config only | `--yolo` | `.qwen/settings.json` | ✓ / ✓ (interactive) | — | — | `--resume` (linear) |
| `crush` | Crush | `crush` | ✓ | prepended | config only | `run -q` (non-interactive) | `crush.json` | ✓ / ✓ | — | — | `--session` (linear) |
| `copilot` | GitHub Copilot CLI | `copilot` | ✓ | prepended | `--reasoning-effort` | `--allow-all-tools --allow-all-paths --no-ask-user` | — | — | — | — | `--resume` (linear)¹ |
| `cursor` | Cursor CLI | `cursor-agent` | ✓ | prepended | `--model 'M-<effort>'` | `--force` (`--trust` under `--no-bypass`) | `.cursor/cli.json` | ✓ / ✓ | — | ✓ | `--resume` (linear) |

The `--resume` column shows each harness's headless continuation flag and whether
it can **fork** (`run --resume <id> --fork`: branch a new session from the resumed
one, leaving the original — and its cached prefix — untouched). Only Claude Code
(`--fork-session`) and OpenCode (`--fork`) fork headlessly; the rest *resume
linearly* (append in place), and `--fork` is a usage error for them, never a
silent linear resume. ¹ Goose and Copilot emit no session id to stdout headlessly,
so their continuation handle is **caller-supplied** (a `--name`, or a minted UUID
respectively) and reused on the next run — `session_id` stays `null` for them
(nothing to extract); every other harness reports an id oneharness captures.

Above `--resume` sits the higher-level **`--session <name>`** (see the *Session
handle* section): a stable, caller-owned name oneharness maps to the harness's
native session id in a small store, so a consumer threads **one name** across
turns instead of extracting and re-passing the id itself. It is supported exactly
for the harnesses that expose a session id headlessly — `claude-code`, `opencode`,
`codex`, `cursor`, `qwen` (`session_capable: true` in `oneharness list`); for the
rest (which have no id to bind a name to) `--session` is a loud usage error.

- **`model`** — ✓ means the harness takes a model flag. Goose selects its model
  from its own provider config, so `model` is intentionally not mapped for it.
- **`system`** — "native flag" means the system prompt maps to a real flag
  (Claude Code's `--append-system-prompt`, Goose's `--system`); "prepended"
  means the harness has no such flag, so the text is prepended to the prompt —
  it always reaches the model, never silently dropped.
- **`reasoning`** — reasoning / thinking effort (`--reasoning <effort>` /
  `ONEHARNESS_REASONING` / config `reasoning`, and per harness next to its
  `model`), an opaque string forwarded verbatim in each harness's native shape:
  Claude Code's `--effort` (low/medium/high/max/auto), Codex's `-c
  model_reasoning_effort=` (minimal…xhigh), Copilot's `--reasoning-effort`
  (low/medium/high), and Cursor's model-id tier suffix (`--model claude-opus-4-8`
  + `--reasoning high` → `--model claude-opus-4-8-high`; cursor-agent bakes the
  effort into the model name and rejects a bracketed `[effort=…]`, so Cursor needs
  a base model whose family accepts the tier suffix, and a loud usage error when
  no model is set). Effort is a provider/model capability with no shared spelling, so
  oneharness does not interpret the value — an effort the model rejects surfaces
  as that harness's own error, never a guess. The remaining harnesses set effort
  only in their own config file (OpenCode, Qwen, Crush) or expose no headless
  knob at all (Goose), so `--reasoning` is a **loud usage error** for those rather
  than a silent drop; scope the setting per harness when a selection mixes capable
  and incapable ones. (`supports_reasoning` in `oneharness list`; each capable
  harness's delivery is drift-alarmed live by `oh_reasoning_enforce`.)
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
  (every harness supports headless continuation). The cell also shows whether the
  harness can **fork** (`--fork`): Claude Code and OpenCode branch a new session
  from the resumed one; the rest *resume linearly* (append in place), so `--fork`
  is a usage error for them, never a silent linear resume.

The remaining unified settings — `timeout`, `env`, `bin`, per-harness `args`,
`cwd`, selection — are enforced by oneharness itself at run time, so they work
for **every** harness — as does `--schema` ([structured output](#structured-output),
prompt-based where a harness has no native schema flag). `oneharness list` prints
this registry as JSON, including each adapter's exact command, its `sync_file`,
and `supports_resume` / `supports_fork` / `supports_native_schema` /
`supports_reasoning` / `supports_allowed_tools` / `supports_denied_tools` /
`supports_hooks` capability flags.

## Install

```console
# from PyPI (per-platform wheel wrapping the prebuilt binary — no Rust toolchain)
pip install oneharness-cli          # installs the `oneharness` command
# or from npm (per-platform package wrapping the prebuilt binary — same promise)
npm install -g oneharness-cli       # also installs the `oneharness` command
# typed Node API (includes the matching CLI package)
npm install @oneharness/sdk
# or the latest prebuilt release for your platform via the install script
curl -fsSL https://raw.githubusercontent.com/nickderobertis/oneharness/main/scripts/install.sh | sh
# or pin a release tag / install directory
curl -fsSL https://raw.githubusercontent.com/nickderobertis/oneharness/main/scripts/install.sh \
  | sh -s -- --version v0.1.0 --to ~/.local/bin
# or from crates.io / a published release tag
cargo install oneharness --locked
cargo install --git https://github.com/nickderobertis/oneharness --tag v0.1.0 --locked
# or from a clone
cargo install --path .
# or just build the release binary
just build-release            # -> target/release/oneharness
```

A tagged release ships five ways: **PyPI** wheels (`pip install oneharness-cli`,
the distribution is `oneharness-cli`, the command is `oneharness`), **npm**
per-platform packages (`npm install -g oneharness-cli`, same distribution name,
same command), **crates.io** (`cargo install oneharness`), prebuilt checksummed
binaries on its
[GitHub Releases](https://github.com/nickderobertis/oneharness/releases) page for
Linux, macOS, and Windows, and `cargo install --git`. The PyPI and npm packages
both wrap the **prebuilt** binary — no Rust toolchain, no compile — carrying the
platform-specific binary in a per-platform artifact (a wheel; an
`@oneharness/cli-<platform>-<arch>` optional dependency) that the package manager
selects for your OS and CPU. Building from source requires a stable Rust
toolchain and [`just`](https://github.com/casey/just).

Node applications can use `@oneharness/sdk` for typed `run`, registry
`list`/`detect`, continuation (`resume` or named `session`), and standardized
history lookup/listing. Its TypeScript declarations and named Zod runtime schemas
are generated from the Rust JSON Schema metadata and drift-checked by `just
check`; output schemas preserve unknown fields for additive forward compatibility,
while the input schemas `RunOptionsSchema` and `HistoryListOptionsSchema` reject
unknown option names before the SDK reads an option. See
[`npm/oneharness-sdk/README.md`](npm/oneharness-sdk/README.md).
The install script honors `ONEHARNESS_VERSION`, `ONEHARNESS_INSTALL_DIR`,
`ONEHARNESS_RELEASE_BASE_URL`/`--base-url`, `ONEHARNESS_CHECKSUM_BASE_URL`, and
`GITHUB_TOKEN` (for higher GitHub API rate limits when resolving the latest
release).

### Supply-chain verification

The install script never trusts a mirror to attest its own download. It verifies
every archive against a trust root **independent of where it was downloaded**,
and aborts if nothing independent can vouch for it. Two roots, tried in order:

1. **Sigstore build-provenance attestation (preferred).** Each release ships a
   keyless [Sigstore](https://www.sigstore.dev/) bundle beside the archive
   (`oneharness-<tag>-<target>.sigstore.json`), logged to the public Rekor
   transparency log and bound to this repo's release workflow's OIDC identity —
   no signing key or secret. When a verifier is present —
   [`cosign`](https://github.com/sigstore/cosign),
   [`sigstore`](https://pypi.org/project/sigstore/) (`pip install sigstore`), or
   [`gh`](https://cli.github.com/) — the installer verifies the archive against
   the bundle **offline**. The trusted digest comes from the signed attestation
   itself (no checksum file is consulted), so a mirror cannot forge it, and it
   works behind a mirror that can't reach github.com. Where github.com is
   unreachable a verifier is one registry install away (`pip install sigstore`,
   `npm i -g @sigstore/cli`, or `go install …/cosign@latest`).
2. **SHA-256 checksum from canonical GitHub (fallback, only when no verifier is
   installed).** The `.sha256` is fetched from github.com, never from the mirror.
   A checksum that shares the mirror's origin is no trust root at all — the mirror
   would just serve a matching tampered checksum — so the installer **refuses**
   it and tells you to install a verifier, rather than trust the mirror to vouch
   for its own download.

Serve the archive from a mirror with `ONEHARNESS_RELEASE_BASE_URL` (or
`--base-url`) — for a network that can reach a mirror but not github.com, ship
the `.sigstore.json` bundle on the mirror too and install a verifier, and the
whole flow works offline. `ONEHARNESS_CHECKSUM_BASE_URL` points the checksum
fallback at a specific independent root. You can also verify any archive out of
band:

```console
cosign verify-blob-attestation --new-bundle-format \
  --bundle oneharness-v0.1.0-x86_64-unknown-linux-gnu.sigstore.json \
  --type https://slsa.dev/provenance/v1 \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/nickderobertis/oneharness/\.github/workflows/release\.yml@' \
  oneharness-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
# or, more simply:
gh attestation verify oneharness-v0.1.0-x86_64-unknown-linux-gnu.tar.gz \
  --repo nickderobertis/oneharness
```

Every release runs a `verify-attestation` CI job that installs real `cosign` and
`sigstore-python` and runs these exact commands against the just-published
bundle, so a drift in the signing identity or flags reddens the release instead
of silently degrading installs to the checksum fallback.

## Usage

`list`/`detect`/`config`/`sync`/`run` emit JSON to **stdout** (diagnostics go to
**stderr**), `gate` speaks a harness's hook protocol on stdin/stdout, and `init`
scaffolds a starter config with a plain confirmation line.

```console
oneharness init                                   # scaffold a starter oneharness.toml (refuses to overwrite; --force to replace)
oneharness init oneharness.judge.toml             # scaffold under a specific name
oneharness list                                   # describe the registry
oneharness detect --all                           # which harnesses are installed (+ versions)
oneharness config                                 # effective layered config + where each value came from
oneharness sync                                   # merge the unified settings into each harness's own config file
oneharness sync --global                          # install [[hooks]] into the user-global config instead of the project
oneharness run --all --prompt "…"                 # run everywhere, in parallel
oneharness run --harness claude-code,codex --prompt-file task.md
oneharness run --harness claude-code --system "$(cat ctx.md)" \
  --prompt "Q1" --prompt "Q2" --prompt "Q3" --batch-strategy min-tokens  # batch: one harness, N prompts, shared cache prefix
oneharness run --all --print-command --prompt "…" # dry run: show commands, run nothing
oneharness gate claude-code --deny-if-contains X  # the pre-tool gate an installed hook invokes (reads stdin)
```

Useful `run` flags:

- `--all` / `--harness <id,…>` / `--exclude <id,…>` — selection.
- `--prompt <text>` or `--prompt-file <path|->` — the prompt (file or stdin). Both
  are **repeatable**; passing more than one prompt switches to a [batch
  run](#batch-runs-same-prefix-prompt-caching) (one harness, N prompts). Each
  `--prompt-file` is read whole as one prompt (not split per line); `-` (stdin)
  may appear once. Combined order is every `--prompt`, then every `--prompt-file`.
  A **large** prompt is delivered to the harness off the argv (piped to its stdin)
  so it never trips the OS argument limit at the harness spawn either — see
  [Large prompts](#large-prompts-off-argv-delivery).
- `--batch-strategy <speed|min-tokens>` — for a batch run, how the calls are
  scheduled to exploit the shared prefix cache (`speed`, the default, or
  `min-tokens`); see [batch runs](#batch-runs-same-prefix-prompt-caching). No
  effect on a single-prompt run.
- `--run-mode <parallel|fallback>` — how the selected harnesses are run
  (`parallel`, the default, or `fallback`); also `run_mode` in config /
  `ONEHARNESS_RUN_MODE`. See [Fallback mode](#fallback-mode-first-that-runs-wins).
- `--model <m>` — passed to each harness that supports a model flag. **Repeatable**:
  pass it more than once (or set config `models` / `ONEHARNESS_MODELS`) to fan out
  over several models — see [Multiple models](#multiple-models-fan-out-over-the-model-axis).
  A CLI value overrides config `model`/`models`.
- `--system <text>` or `--system-file <path|->` — portable system prompt for
  **every** harness: mapped to a native flag where one exists (Claude Code's
  `--append-system-prompt`, Goose's `--system`), and prepended to the prompt
  otherwise, so the instructions always reach the model. The two are **two
  spellings of one input** and mutually exclusive; config `system` is the fallback
  for both. Use `--system-file` (the file-based counterpart to `--prompt-file`,
  `-` for stdin) for a system prompt too large to pass as an argv string — a big
  `--system` value can trip the OS single-argument limit and fail at spawn with
  `Argument list too long` (E2BIG) before any harness runs. Only one input may
  read stdin, so `--system-file -` cannot combine with `--prompt-file -`. However
  the system prompt reaches oneharness, a **large** one is also delivered to the
  harness off the argv (a temp file on Claude Code, folded into the stdin prompt
  elsewhere), so it clears the harness-spawn ceiling too — see
  [Large prompts](#large-prompts-off-argv-delivery).
- `--resume <session>` — continue a prior session, sending the prompt as its next
  turn. **Single-harness only** (a session belongs to one harness); every harness
  supports it, but multi-harness selections are still a usage error. The continued
  `session_id` is surfaced on each result (see below). Harnesses that emit no id
  headlessly (Goose, Copilot) take a **caller-supplied** handle you reuse across
  runs (a `--name`, or a minted UUID).
- `--fork` — with `--resume`, branch a **new** session from the resumed one
  instead of appending to it, leaving the original (and its cached prefix)
  untouched — so one expensive initial prompt can seed many independent follow-ups
  that each reuse the cached prefix. Only Claude Code (`--fork-session`) and
  OpenCode (`--fork`) fork headlessly (`supports_fork` in `oneharness list`);
  requesting it for any other harness is a usage error, never a silent linear
  resume. Requires `--resume`.
- `--session <name>` — continue (or start) a named conversation by a stable,
  caller-owned handle: oneharness maps `<name>` to the harness's native session id
  in a small store, so you thread **one name** across turns instead of extracting
  and re-passing the id yourself. The first `--session <name>` run starts fresh and
  captures the id; later runs with the same name resume it. **Single-harness** in
  the default parallel mode; under `--run-mode fallback` it binds to the first
  session-capable harness in the chain. Only for harnesses that expose a session id
  headlessly (`session_capable` in `oneharness list`) — others are a loud usage
  error. The higher-level counterpart to `--resume`; mutually exclusive with
  `--resume`/`--fork`/`--all` and with a batch. See
  [Session handle](#session-handle).
- `--session-dir <dir>` — directory the `--session` store lives in (default:
  `<platform state dir>/oneharness/sessions`). Like `--resume`/`--fork`, a
  per-invocation knob with no config/env layer; mainly for isolating the store in
  tests and scripts.
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
project (or a user) states its defaults once instead of repeating flags.
**`oneharness init`** scaffolds a commented starter `oneharness.toml` (a
fallback-mode chain) to edit from; it refuses to overwrite an existing file
unless `--force`, and takes an explicit path (`oneharness init oneharness.judge.toml`)
to scaffold a differently named config. Several sources layer per field, lowest
precedence first:

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
comma-separated like their repeatable flags (`ONEHARNESS_HARNESSES=claude-code,codex`,
`ONEHARNESS_MODELS=opus,sonnet` for the model fan-out),
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
model = "gpt-5"                 # --model (single model, per harness)
models = ["opus", "sonnet"]     # repeated --model: fan out over the model axis
system = "Be terse."            # --system
bypass = true                   # legacy --bypass toggle (opt-in; default false)
mode = "default"                # --mode; beats `bypass` (default: "default")
timeout = 120                   # --timeout, in seconds
output_format = "json"          # --output-format
schema_file = "person.json"     # --schema (structured output; relative to project)
schema_max_retries = 2          # --schema-max-retries (default 2)
max_parallel = 4                # --max-parallel
run_mode = "parallel"           # --run-mode ("parallel" or "fallback")
require_available = false       # --require-available
history = false                 # --history / --no-history (opt-in run history)
history_dir = "~/logs/oh"       # --history-dir (default: platform state dir)
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

#### The mock/spy responder (`oneharness mock`)

**`oneharness mock <id>`** is the gate's read-write sibling, for behavioral
test suites (the [skilltest](https://github.com/nickderobertis/skilltest)
consumer — see `docs/mock-spy-design.md`): the same stdin/stdout hook loop,
but driven by a `--rules <file>` JSON ruleset that can **intercept** tool calls,
not just deny them. Every observed event is also appended to a `--spy-file`
JSONL log (or `$ONEHARNESS_SPY_FILE`) — the **spy** channel, which records the
*original* tool call even when a rewrite substituted its input (the transcript
`events` can only show post-rewrite reality).

```jsonc
// rules.json — first matching rule wins; no match = allow through (spy-only)
{
  "rules": [
    {
      // all listed criteria must hold (AND). `tool_regex` spans a harness's
      // tool-name casing; `input` matches specific argument fields.
      "match": {
        "tool_regex": "^(?i)bash$",
        "input": { "command": { "regex": "git\\s+push" } }
      },
      "action": { "deny": { "message": "pushes are mocked in this test" } }
    },
    {
      // fake a shell result by declaring ONLY the output: oneharness generates
      // a safely-quoted printf stub itself, so no user-authored command — and
      // nothing real — executes; the model receives this text (+ trailing
      // newline) as the tool's genuine result. `exit_code` fakes a failure.
      "match": { "input": { "command": { "contains": "git status" } } },
      "action": { "stub": { "output": "nothing to commit, working tree clean" } }
    },
    {
      // the general rewrite: substitute any input fields — here redirecting a
      // file read to a fixture (shell stubs are better written with `stub`)
      "match": { "tool": "Read", "input": { "file_path": { "equals": "/etc/prod.yaml" } } },
      "action": { "rewrite": { "input": { "file_path": "/tmp/ws/fixtures/config.yaml" } } }
    }
  ]
}
```

**Matching** — a rule's `match` combines any of these criteria (all present
ones must hold): `tool` (case-insensitive exact tool name) or `tool_regex`;
`event_contains` (substring of the raw hook event — the portable, harness-
agnostic option) or `event_regex`; and `input`, a map from an argument name
(`command`, `file_path`, …) to a predicate — `equals`, `contains`, or `regex`
— so you can match on the *specific* tool input rather than the whole event. An
absent input field fails the rule (never fabricated); a non-string argument is
compared against its compact JSON, so a predicate can still target an array or
object. Regexes are RE2 (linear-time — a caller-supplied pattern can't hang the
responder), unanchored (use `^…$` for exact); an invalid pattern, an empty
needle, or a match with no criteria is a loud usage error before anything runs.

**Actions**: `deny` (the model reads the message as the tool's failure),
`stub` (declare a shell call's output — compiled to a safe printf rewrite, so
it needs the same `mock_rewrite` capability), and `rewrite` (substitute raw
input fields — the primitive under `stub`, and the way to mock file reads).
The model never perceives the substitution: its own tool call (the original)
is already in its context when the hook fires, and what it receives back is
just the result — keep canned output *plausible for what was asked*, since a
self-inconsistent result (a fixture whose content names a different file) is
the one thing a model has been observed to notice.

Per-harness capability — `deny` works wherever the gate does; `rewrite` (and
therefore `stub`, which compiles to one) needs the harness's `mock_rewrite`
shape (see `supports_mock_deny` / `mock_rewrite` in `oneharness list`; a rule
using an action the harness can't express is a loud usage error, never a silent
allow):

| harness | deny | input rewrite (`mock_rewrite`) |
| --- | --- | --- |
| `claude-code` | ✅ | ✅ `claude-nested` (PreToolUse `updatedInput`; also honored for `Read` — file-read mocking) — verified live |
| `codex` | ✅ | ✅ `claude-nested` — verified live, but its hooks engine needs the run to opt in: pass `-c features.hooks=true --dangerously-bypass-hook-trust` (via `--` passthrough or `[harness.codex] args`); the `trust_level` config route loads no hooks |
| `crush` | ✅ | ✅ `crush-flat` (`updated_input`, shallow-merged) — verified live |
| `opencode` | ✅ | ✅ `opencode-shim` (the synced plugin merges the args) — verified live |
| `cursor` | ✅ | ✅ `cursor-permission` (`preToolUse` `updated_input`) — verified live on Linux/macOS (its Windows hook bug applies to mocks too) |
| `qwen` | ✅ | ❌ its documented `updatedInput` is **not honored live** (hook fires, verdict emitted, original ran — measured on all three OSes); deny-only until re-sourced |
| `copilot` | ✅* | ❌ probe-refuted: its repo hooks produced **zero events** headlessly (`-p`), so neither verb can fire through `oneharness run` today |
| `goose` | ✅ | ❌ its hook protocol has no rewrite verdict |

**The single-flag path — `run --mock-rules` (and/or `--spy-file`)** delivers
the hook **for one invocation**, works in an *original* workspace whose
existing config keeps applying (the mock is layered on top), and leaves no
trace afterwards:

```console
$ oneharness run --harness claude-code --cwd ~/proj \
    --mock-rules rules.json --spy-file spy.jsonl --prompt "…"
```

Per-harness delivery (all live-verified; see `mock_delivery` in the registry):
Claude Code takes the hook **on the argv** via a per-run `--settings` temp
file — zero workspace mutation, project/user settings untouched and still in
effect. The others get a **project-scope install through the non-destructive
merge** (existing hooks and unrelated keys preserved), with every touched file
snapshotted before and restored byte-identically after the run — files the
install created are deleted, directories it created are pruned. Codex's
hook-engine opt-in flags (`-c features.hooks=true
--dangerously-bypass-hook-trust`) are appended to its argv automatically.
`--spy-file` alone (no ruleset) installs a pure observer. A selected harness
whose hooks can't fire this way is refused loudly before anything is touched:
qwen (user-scope-only hooks — use the `sync --global` + redirected-HOME
pattern instead) and copilot (hooks never fire headlessly). The report records
`mock_rules` and `spy_file` so a mocked run is distinguishable from a clean
one. One caveat: the restore runs on the normal exit path, so a hard kill
(SIGKILL) mid-run can leave the hook installed — re-running any oneharness
mock/sync in that workspace, or `git checkout`, puts it back; prefer throwaway
workspaces for suites.

**The standing-policy path** — a `[[hooks]]` entry synced into the harness's
own config — remains available for policies that should persist (and is how
qwen's user-scope delivery works):

```toml
[[hooks]]
command = "oneharness mock {harness} --rules /tmp/ws/rules.json --spy-file /tmp/ws/spy.jsonl"
```

The rewrite path AND the ephemeral delivery are drift-alarmed live per harness
by the `oh_mock_enforce` e2e phases (driven through `run --mock-rules`: the
substituted command must run, the original must not, the spy log must keep the
original event, and the workspace must carry no trace of the hook afterwards).

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

Under [`--run-mode fallback`](#fallback-mode-first-that-runs-wins) the rule is
different: `0` when the harness that ran succeeded, `1` when it ran but failed
**or** when no candidate could run at all — the fallen-through candidates never
count against the run.

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
- `usage` / `usage_source` — `{ input_tokens, output_tokens, cache_read_tokens,
  cache_write_tokens, cost_usd }`, each field independently `null` when the harness
  doesn't report it (cost is commonly absent on subscription auth). The `usage`
  object is always present so the shape is stable for cross-harness cost/latency
  tables. `usage_source` records the method: `json` for a harness that reports a
  whole-run total in one event (Claude Code), `json:summed-steps` for one that
  reports per-step usage that oneharness sums (OpenCode). The two cache fields
  surface **provider-side prompt-cache** counts — `cache_read_tokens` is prefix
  tokens served cheaply from cache, `cache_write_tokens` is tokens written to it
  (a.k.a. cache creation) — so a consumer can confirm a repeated/forked run
  actually hit the cache. Cache-reporting support today (the rest leave both cache
  fields `null` — never `0` as a guess):

  | harness | cache fields | source field(s) |
  |---------|:------------:|-----------------|
  | `claude-code` | ✓ read + write | `usage.cache_read_input_tokens` / `usage.cache_creation_input_tokens` |
  | `opencode` | ✓ read + write | summed `part.tokens.cache.{read,write}` |
  | all others | — | (no cache counts emitted; `cursor` emits no usage at all) |

  Each supported harness has a live drift alarm (`oh_cache_assert` in its
  `scripts/e2e-<id>.sh`): a second run within the cache TTL must surface
  `cache_read_tokens > 0`, proving the extraction matches the real output shape.
- `session_id` — the handle a harness exposes for continuation, read from the
  snake_case `session_id` (Claude Code, Cursor, Qwen), camelCase `sessionID`
  (OpenCode), or Codex's `thread_id`; feed it back via `run --resume <session>`
  (single-harness) to drive a faithful multi-turn against the real agent, or add
  `--fork` (Claude Code / OpenCode) to branch independent follow-ups off one cached
  prefix. `null` for a harness that emits no id headlessly (Goose, Copilot) — their
  handle is caller-supplied, never scraped (see the support matrix).
- `session` — the uniform `--session` handle in play (else `null`): `{name, phase
  (create|continue), token, store_file}`. Lets a consumer thread one stable name
  across turns while oneharness maps it to the harness's `session_id` above. See
  [Session handle](#session-handle).
- `events` / `events_source` — a **normalized array of tool-call / action
  events** the harness took, in order, so a consumer can assert on *behavior*
  (`ran bash with a command matching /…/`, `edited exactly config.yaml`, `used ≤
  3 tool calls`), not just the final `text`. Each entry is `{ kind, name, input,
  output, index }`: `kind` is `tool_call` or `tool_result`, `name` is the
  normalized tool name (`null` for a result), `input` is the structured,
  tool-shaped arguments (so a consumer reads the command string / file path
  without re-parsing), `output` is the observation when exposed, and `index` is
  the position in the run. `events` is `null` (never `[]`) when the harness's
  output carries no machine-readable trace — a plain-text harness (Codex, Goose,
  Qwen, Crush, Copilot), or Claude Code's single-document `json` result, which
  omits the intermediate transcript — with `events_source` then also `null`, so a
  consumer tells "harness doesn't expose it" from "no tools were used." Like
  `text`, it is best-effort and never fabricated; consumers needing certainty
  parse `stdout`.

  Events require the harness to run in a format that carries a **tool
  transcript**. Some emit one in the format oneharness already uses; the rest
  need a richer format, which **`--events`** (and `--stream`) selects
  automatically per harness (`HarnessSpec.events_format`) without you knowing the
  quirk — and without breaking text extraction. Every shape below was **sourced
  from a real transcript** captured from the live CLI (never guessed) and is
  drift-alarmed by a per-harness live e2e (`oh_events_assert`):

  | harness | events via | `events_source` |
  |---------|------------|-----------------|
  | `opencode` | `json` (default) | `json:opencode-parts` |
  | `cursor` | `stream-json` (default) | `stream-json:cursor-tool-calls` |
  | `claude-code` | `--events` → `stream-json` (adds the required `--verbose`) | `stream-json:content-blocks` |
  | `codex` | `--events` → `exec --json` | `json:codex-items` |
  | `qwen` | `--events` → `--output-format stream-json` | `stream-json:content-blocks` |
  | `goose`, `crush`, `copilot` | no machine-readable transcript headlessly | — (null) |

  Four recognizers cover these, each harness-agnostic: OpenCode `tool` parts, the
  Anthropic content-block shape (Claude Code + Qwen), Cursor's `tool_call`
  events, and Codex's `command_execution` items. `goose`, `crush`, and `copilot`
  emit only decorative TUI text headlessly (confirmed by probing the live CLIs),
  so `events` stays `null` for them — the honest answer, not a gap.

  **Streaming** (`oneharness run --stream <one harness>`) emits each event as an
  NDJSON `{"type":"event","event":{…}}` line the instant it is observed, then a
  terminal `{"type":"result","report":{…}}` line with the full envelope. A
  consumer can **short-circuit** the moment it sees a disallowed action by
  closing the stream — oneharness's next write fails (broken pipe) and it tears
  the harness down, so a bad turn is cut off instead of paid for in full. Stream
  runs a single harness (no batch, no `--schema`); `--stream` implies the
  `--events` format selection.
- `failure_kind` / `failure_kind_source` — on a non-zero run, a coarse reason
  (`auth`, `rate_limit`, `model_not_found`, `quota`) so a caller can tell a
  retryable condition from a broken request. This is **distinct from `status`**,
  which only records oneharness's relationship to the process. One kind,
  `tool_deferred`, is reported even on a `status: ok` run: the harness exited
  cleanly but only **deferred** a builtin tool call (`Read`, `Bash`, …) instead
  of executing it, so it produced no result. This happens in **bridged/managed
  Claude Code deployments** (where `tengu_non_deferrable_builtins` is empty and
  every builtin is deferred) — a tool-using run there silently dead-ends. When
  detected, oneharness sets `failure_kind: "tool_deferred"`, writes an actionable
  `error` naming the tool, and fails the run (non-zero exit) instead of letting it
  look like an empty or schema-invalid answer. **Agentic / tool-using runs against
  `claude-code` require a deployment that executes tools inline** (a standalone
  environment or CI); a deferring deployment is unsupported for them, and there is
  no consumer-side flag to force inline execution.

Coverage is keyed off each harness's documented output shape — Claude Code's
`result` JSON, OpenCode's JSONL (`text` parts for the answer, `step_finish` for
usage), Cursor's `stream-json` — and widens as more shapes are sourced; an absent
signal is the honest answer, not an error. Consumers that need certainty should
parse `stdout` themselves.

#### Streaming events (direction)

Today `events` is delivered **at the end**, in the final report — oneharness
spawns each harness, waits for it to exit, then normalizes the whole transcript
at once. The `events` shape above is deliberately the same one a **streaming**
mode will emit incrementally: the extractor is line-oriented and pure, so the
same normalized `{ kind, name, input, output, index }` events can be surfaced as
soon as each is observed rather than only after the run completes.

The motivating consumer is behavioral skill-testing (`skilltest`): with a live
event stream, a language test can **short-circuit the moment it observes bad
behavior** (a forbidden `rm -rf`, an out-of-scope network call) — killing the run
instead of paying for a full turn before judging it. Realizing that end to end is
a staged effort beyond this field: a streaming `run` path in the runner
(incremental read + early-terminate on the consumer's signal, single-harness and
non-batch), then threading the stream through the language **SDKs** and the
skilltest **plugin**. This section is the anchor for that work; the normalized
event shape is the stable contract it will build on.

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

### Session handle

Driving a faithful multi-turn conversation against a real agent means continuing
the *same* harness session each turn. The low-level way is `--resume <id>`: run
once, read `session_id` from the report, pass it back next turn. That works, but
the caller carries per-harness bookkeeping — extract the id, thread it, and know
which harnesses even emit one.

`--session <name>` removes that. You pick a stable name and pass it every turn;
oneharness maps the name to the harness's native session id in a small store
(`<state dir>/oneharness/sessions/<project-slug>/<name>.json`, or `--session-dir`),
so:

```bash
# Turn 1 — starts fresh, captures the harness's session id under the name "triage".
oneharness run --harness claude-code --session triage --prompt "Investigate the flaky test."
# Turn 2 — same name resumes the same session; you never touched the native id.
oneharness run --harness claude-code --session triage --prompt "Now propose a fix."
```

The report's `session` block echoes `{name, phase, token, store_file}` — `phase`
is `create` on the first run and `continue` after, `token` is the bound native id.
A named session is **bound to one harness** (reusing the name on another is a loud
error) and cannot combine with `--resume`/`--fork`/`--all` or a batch, and is
supported only for harnesses that expose a session id headlessly (`session_capable`
in `oneharness list`: `claude-code`, `opencode`, `codex`, `cursor`, `qwen`) — for
the rest, `--session` is a usage error rather than a silent fresh start. In the
default **parallel** run mode it is single-harness; under
[`--run-mode fallback`](#fallback-mode-first-that-runs-wins) it is allowed on a
multi-harness chain and binds to the **anchor** — the first session-capable harness
in the priority order, which fallback deterministically settles on given stable
availability (the token is applied to, and captured from, that harness only, so a
transient fall-through to a different harness never resumes it with a foreign id).
This is the substrate a multi-turn driver (e.g. a simulated-user / skill-testing
framework) builds on: thread one handle, get faithful state, read `events` for what
the agent *did*.

### Fallback mode (first that runs wins)

By default `run` drives every selected harness in **parallel** and reports them
all. `--run-mode fallback` (or `run_mode = "fallback"` in config) instead runs
them in **priority order** and stops at the **first harness that actually runs
the task** — falling through only the candidates that *cannot run at all*. This
is graceful degradation across a set of harnesses a repo declares it supports:
list a few, and whichever one a given contributor (or CI runner) has installed
and authenticated is the one that runs.

```console
# Try claude-code first; if it isn't set up, fall through to codex, then opencode.
oneharness run --run-mode fallback --harness claude-code,codex,opencode \
  --prompt "Explain the failing test" --compact | jq '.fallback, .results[].status'
```

**What falls through vs. what stops.** The distinction is deliberate: a *setup*
problem tries the next harness; a *real run* — success or failure — stops the
chain, so a long, genuine run can never be mistaken for "try the next one".

| Outcome | Fallback? |
| --- | --- |
| Not installed (`skipped`) | ✅ fall through — `not-installed` |
| Resolved but unspawnable (`spawn-error`) | ✅ fall through — `spawn-error` |
| Ran, exited non-zero, classified `auth` | ✅ fall through — `auth` |
| Ran, exited non-zero, classified `quota` (no credit) | ✅ fall through — `quota` |
| Ran and succeeded (`ok`) | ⛔ stop — this is the answer |
| Ran and failed the task (`nonzero`, incl. `rate_limit` / `model_not_found`) | ⛔ stop¹ |
| Timed out (`timeout`) — a slow but genuine run | ⛔ stop |
| Never produced a schema-conforming answer (`--schema`) | ⛔ stop (the harness ran) |

¹ **Exception — a model list.** When the run is fanning out over several models
(repeated `--model` / config `models`; see [Multiple models](#multiple-models-fan-out-over-the-model-axis)),
a per-model rejection means "try the next model", so `model_not_found` (fall
through — `model-not-found`) and `rate_limit` (fall through — `rate-limit`) *do*
fall through. With a single model both still stop the chain, as above.

The report gains a `fallback` block, `{ "ran", "fell_through": [{ "harness",
"reason" }] }`: `ran` is the harness that executed (or `null` when every
candidate failed to start), and `results` holds only the harnesses **attempted**
— the fallen-through ones in priority order, then the one that ran. Priority
order is the `--harness` / config `harnesses` order (registry order under
`--all`). Under `--print-command` nothing executes, so the block is `null` and
every candidate's command is printed in priority order.

**The command must be valid for the whole set.** Every listed harness is
validated up front, so a flag no candidate could honor (an approval `--mode` a
listed harness can't express, an unsupported `--mock-rules` action, …) is a loud
usage error **before anything spawns** — even for a harness that is never
reached. This keeps people and agents writing commands that work for every
harness the fallback config supports, not just the one that happens to run.

Fallback is single-outcome by nature, so it refuses a [batch](#batch-runs-same-prefix-prompt-caching)
run, the low-level `--resume` / `--fork` continuations (each pins one specific
harness's native id), and `--stream` as loud usage errors. The higher-level
[`--session`](#session-handle) handle **is** allowed: it binds to the anchor (the
first session-capable harness in the chain), so a named conversation degrades
gracefully across the same priority set. Exit code: `0` when the harness that ran
succeeded, `1` when it ran but failed **or** when no candidate could run at all.

### Multiple models (fan out over the model axis)

By default a run uses one model per harness — a single `--model` (or config
`model`, overridable per harness with `[harness.<id>].model`). Pass `--model` **more
than once** (or set config `models = [...]` / `ONEHARNESS_MODELS`) and `run` fans
out over the **model axis**, and it composes with the two run modes exactly as you
would expect:

- **`parallel` (the default) — the harness × model cross-product.** Every selected
  harness runs once per model, all concurrently, and `results` holds one entry per
  `(harness, model)` pair (harness-major, then model-minor). One harness × three
  models is three runs; `--all` × two models is every harness twice.

  ```console
  # Compare two models across two harnesses — 4 runs in parallel:
  oneharness run --harness claude-code,codex --model opus --model sonnet \
    --prompt "Explain this diff" --compact | jq '.results[] | {harness, model, status}'
  ```

- **`fallback` — the (harness, model) priority chain.** The same cross-product
  becomes the fallback order (harness-major, model-minor); the run stops at the
  first pair that actually runs. Here a **per-model rejection falls through**: an
  unavailable model (`model_not_found` → `model-not-found`) or an over-limit one
  (`rate_limit` → `rate-limit`) tries the next model, exactly as a missing harness
  tries the next harness — graceful degradation across models. (With a single
  model those still stop the chain — see the [fallback table](#fallback-mode-first-that-runs-wins).)

  ```console
  # Prefer opus; if it's unavailable or rate-limited, fall through to sonnet:
  oneharness run --run-mode fallback --harness claude-code --model opus --model sonnet \
    --prompt "Explain this diff" --compact | jq '.fallback'
  ```

Each result carries its own `model` (the value put on the harness's model flag,
also visible in `command`), and the report gains a `models` list — the presence of
which is the signal a consumer keys on to read each result's `model`. The top-level
`model` is the first of the list. A one-element list is **not** a fan-out (it
behaves like a single `--model`). Because a fan-out multiplies the run into several
units, more than one model is a loud usage error with a [batch](#batch-runs-same-prefix-prompt-caching)
(its cache prefix is per harness/model) and with the single-unit continuations
`--resume` / `--fork` / `--session` / `--stream`.

### Batch runs (same-prefix prompt caching)

A common workload is **many prompts that share a prefix** — the same `--system`
context (a spec, a big reference doc, few-shot examples) with a different question
each time. Pass more than one prompt and `run` switches to a **batch**: it drives
**one** harness over each prompt and returns one report with a result per prompt
(in order), each tagged with its own `prompt`. The top-level report gains a
`batch` block (`{ "strategy", "prompt_count", "forked" }`); `results[].prompt` is
authoritative, and the top-level `prompt` repeats the first for back-compat.

```console
# 3 questions over one shared context, warming it once then forking:
oneharness run --harness claude-code --system "$(cat reference.md)" \
  --prompt "Summarize section 2" \
  --prompt "List the open questions" \
  --prompt "What changed since v1?" \
  --batch-strategy min-tokens --compact | jq '.batch, .results[].usage'
```

Two strategies:

- **`speed`** — **the default** — fire all prompts at once for minimum wall-clock.
  Every call is independent; this optimizes latency, not tokens. It is the default
  precisely because the token-saving alternative only helps one harness today (see
  the support matrix below) and never *hurts* — `speed` is the safe choice for any
  harness.
- **`min-tokens`** — minimize redundant token spend on the shared prefix. On a
  harness whose fork **reuses the cache** (today Claude Code only; see the matrix
  below) it runs the first prompt as a warm-up that establishes a session carrying
  the shared `--system`, then **forks that session** for the remaining prompts, so
  each fanned-out call *reuses* the warmed cached prefix instead of re-sending it.
  The report sets `batch.forked: true`, and the fanned-out results report
  `usage.cache_read_tokens > 0` with a lower `cache_write_tokens` than the warm-up.
  oneharness never claims a saving it can't measure — read the counts. On every
  other harness `min-tokens` falls back to order-only (no saving) with a stderr
  warning, so it is never worse than `speed`.

Why fork rather than just repeating `--system`: provider prompt caching keys on
the harness's byte-exact request prefix, but these CLIs inject per-invocation
content (Claude Code, for instance, re-creates a user-supplied
`--append-system-prompt` on every separate `claude -p` process — only its *own*
global prefix gets cross-process cache reads). So a static `--system` repeated
across processes is **not** reused; the reliable cross-call reuse is a warmed
**session**, which is exactly what `--fork` branches from (see
[`--fork`](#usage)). `min-tokens` operationalizes that.

**Support matrix — where `min-tokens` reduces tokens.** The saving needs a
*cache-reusing fork* (`fork_reuses_cache` in `oneharness list`), which today is
**Claude Code only**:

| harness | token reduction | status |
| --- | --- | --- |
| **claude-code** | yes — warm-then-fork, cache reuse | ✅ **confirmed** (live-proven by `oh_batch_fork_enforce`; the underlying provider caching is itself best-effort — see *Caveats*) |
| opencode | no — its `--fork` re-sends the prefix cold (forking would *raise* tokens), so oneharness keeps it order-only | ⚠️ **known not to help** (measured live) |
| codex, goose, qwen, crush, copilot, cursor | no — no cache-reusing fork, and no cache-count reporting to even measure one | ⛔ **order-only** (no saving) |

So exactly one harness is confirmed to save tokens; every other harness runs
`min-tokens` as a plain scheduler (results are correct, just no token reduction)
and oneharness prints a stderr warning rather than implying a saving. Two findings
shape this (both measured live, not assumed):

- **A static `--system` is not reused across separate harness processes.** Even on
  Claude Code (a *native* `--system` harness) a repeated `--append-system-prompt`
  is re-created on every `claude -p` — only the harness's *own* global prefix gets
  cross-process cache reads. The other five non-Goose harnesses merely *prepend*
  `--system` (no cacheable breakpoint), and the six non-fork harnesses report no
  cache counts at all (so a saving couldn't even be observed). So a system-prompt
  approach saves nothing on them.
- **Only a *cache-reusing* fork helps.** Claude Code's `--fork-session` branches
  from the warmed session and reuses its cached prefix (the fan-out reads it and
  writes little). OpenCode's `--fork` instead re-sends the branched conversation
  cold (the fan-out reads no cache and re-writes the whole prefix — so forking it
  would *raise* tokens), so oneharness leaves OpenCode's `min-tokens` order-only.

On every order-only harness `min-tokens` just orders the calls, and oneharness
says so on stderr rather than implying a saving.

**Caveats.** A batch is **single-harness** by nature (a session/cache prefix is
per harness/model/tools) — selecting more than one harness (or `--all`), or
combining with `--resume`/`--fork`, is a usage error. The token saving needs a
harness with a **cache-reusing fork** (`fork_reuses_cache` in `oneharness list` —
today Claude Code only); on any other harness `min-tokens` only *orders* the calls
(no reuse) and oneharness says so on stderr. Note that where it does fork, this
changes the fan-out's semantics: because the fan-out branches from the warm-up's
turn, the later prompts share the first prompt's context (the fork model — "one
initial prompt seeds independent follow-ups"), rather than being fully independent
questions. Caching itself is best-effort and provider-side (a ~5-min TTL refreshed
on hit, a minimum prefix length, a byte-identical prefix), so the reuse only lands
when the warmed session's prefix clears the minimum and the fan-out runs within
its TTL. Use `speed` when you want N strictly-independent answers with no shared
context.

### Large prompts (off-argv delivery)

Passing a prompt or system prompt as a command-line argument is bounded by the
OS: Linux caps a single argv string at 128 KiB (`MAX_ARG_STRLEN`), and macOS /
Windows cap the whole argv+env. A prompt past that limit fails the spawn with
`Argument list too long` (E2BIG). `--prompt-file` / `--system-file` clear the
*caller → oneharness* hop (the value arrives in a file, not on oneharness's own
argv); oneharness then clears the *oneharness → harness* hop too, delivering a
large prompt (or system prompt) to the harness **off its argv** rather than
re-inlining it (issue #1115).

The switch is automatic and size-gated at **64 KiB**: below it, the argv is
byte-identical to before (so `--print-command` and small runs are unchanged);
above it, oneharness routes the value off-argv where the harness's CLI supports
it. The user/system text the model sees is identical either way. Delivery per
harness (sourced from each CLI's headless docs, drift-alarmed by the live
`oh_long_prompt_enforce` e2e phase):

| Harness | Large user prompt | Large system prompt |
| --- | --- | --- |
| claude-code | stdin (`-p --input-format text`) | temp file (`--append-system-prompt-file`) |
| codex | stdin (`codex exec -`) | folded into the stdin prompt¹ |
| opencode | stdin (piped, positional omitted) | folded into the stdin prompt¹ |
| qwen | stdin (piped, `-p` omitted) | folded into the stdin prompt¹ |
| crush | stdin (piped, positional omitted) | folded into the stdin prompt¹ |
| copilot | stdin (piped, `-p` omitted) | folded into the stdin prompt¹ |
| cursor | stdin (`-p`, positional omitted) | folded into the stdin prompt¹ |
| goose | stdin (`-i -`) | **inline only** — no off-argv route² |

¹ These CLIs have no system-prompt flag, so oneharness already prepends `--system`
to the prompt; the combined text rides the same stdin stream. (Cursor's
stdin-only-prompt behavior was verified live — see `scripts/explore-cursor-stdin.sh`.)
² Goose's `--system` takes inline text with no file/stdin route, so a >128 KiB
*system* prompt for Goose still risks E2BIG — oneharness warns on stderr rather
than failing silently. Its large *user* prompt is delivered via `-i -`.
`oneharness list` exposes `supports_prompt_stdin` / `supports_system_file` per
harness.

### Run history

Every harness keeps its own session history in its own place and shape (Claude
Code under `~/.claude/projects/`, Codex under its sessions dir, …). `run
--history` records a **standardized, cross-harness** history instead: one
normalized record per harness run — the same signals the JSON report carries
(`harness`, `prompt`, `model`, `status`, `usage`, `session_id`, `events`, `text`),
and *only* those (no raw stdout/stderr) — streamed to disk as the run finalizes.

It is **off by default** and opt-in three ways, layered like every other setting
(CLI > env > project file > user file):

```bash
oneharness run --harness claude-code --prompt "…" --history          # this run
```
```toml
# ~/.config/oneharness/config.toml — the per-user opt-in: on for all your
# projects, without committing anything to any project's own config.
history = true
history_dir = "~/logs/oneharness"   # optional; default below
```
```bash
ONEHARNESS_HISTORY=1 ONEHARNESS_HISTORY_DIR=/data/oh oneharness run …  # env
```

`--no-history` (or `history = false` in a nearer layer) turns it back off. Nothing
is written under `--print-command` (nothing runs).

**Layout.** `<history_dir>/<project-slug>/<session>.jsonl` — one file per
`oneharness run` invocation ("session"), partitioned by a slug of the project
directory. `history_dir` defaults to `<platform state dir>/oneharness/history`
(`$XDG_STATE_HOME` or `~/.local/state` on Linux, `~/.local/state` on macOS,
`%LOCALAPPDATA%` on Windows); set it with `--history-dir`, `history_dir`, or
`ONEHARNESS_HISTORY_DIR`.

**Session name.** Each session has a human-meaningful `name` shown next to its
`id`. Harnesses don't expose a readable title headlessly (only an opaque
`session_id`, which oneharness already records per run), so the name is derived
from the session's first prompt — or set explicitly with `--history-name <NAME>`.

**Programmatic handoff.** The run report echoes the session file as
`history_file` (absolute), so a consumer captures it and reads the session back
later. The `oneharness history` verb views and manages the store — JSON on stdout
by default (the programmatic contract), `--format text` for a human view:

```bash
oneharness history list [--project <dir> | --all-projects]   # sessions, newest first
oneharness history show <id-or-name> [--last] [--all]        # a session's records
oneharness history clear [--all-projects] [--yes]            # dry-run unless --yes
```

`show` resolves its argument against a session **id or name** (name is
non-unique — the newest match wins, or `--all` shows every match). `clear` reports
what it *would* remove and deletes nothing until `--yes`, so it is safe to run
non-interactively first.

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
| `plan`     | ✓ | ✓ⁱ | ✓ | — | ✓ | — | ✓ | ✓ |
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
there). Cursor's `read-only` is native `--mode ask`. Codex has no *native* plan
mode in `exec`, so `plan` (ⁱ) is synthesized — the read-only sandbox enforces
no-mutation and a plan instruction is prepended to the prompt, reproducing
Codex's own interactive Plan mode (= read-only sandbox + a plan template). Goose
has no plan workflow and its only no-mutation option (`chat`) disables reads too,
so it offers neither plan nor read-only (a plan *instruction* alone can't help —
it has no read-only *enforcement* to stop the agent acting); Crush's `run` can't
gate, so
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
Two harnesses are excluded by design: **Codex** (`codex exec` loads hooks only
when the invocation opts in with `-c features.hooks=true
--dangerously-bypass-hook-trust` — probe-verified; the `oh_mock_enforce codex`
phase passes those flags and is the live proof its hooks load, so the plain
gate phase stays omitted) and **Copilot** (its project hooks sit behind a
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

Releases are automated from [conventional commits](https://www.conventionalcommits.org)
by [release-plz](https://release-plz.dev) — do not hand-bump the version or
`CHANGELOG.md`. Land commits on `main` (`feat` → minor, `fix`/`perf` → patch,
`!`/`BREAKING` → major; `docs`/`test`/`chore`/`ci` do not release), and
release-plz opens a `release vX.Y.Z` PR that bumps `Cargo.toml`/`Cargo.lock` and
writes the changelog. That PR auto-merges once the gate is green, then:

1. release-plz tags `vX.Y.Z` and cuts the GitHub Release;
2. that Release fires `.github/workflows/release.yml`, which re-runs the complete
   gate, publishes both Cargo crates idempotently in dependency order
   (`oneharness-core` first, then the `oneharness` binary that depends on it),
   attaches archived, sha256-checksummed binaries for Linux, macOS, and Windows,
   signs each archive with a keyless Sigstore build-provenance attestation and
   publishes its `.sigstore.json` bundle (see
   [Supply-chain verification](#supply-chain-verification)), builds per-platform
   PyPI wheels with maturin and publishes them to
   [PyPI](https://pypi.org/project/oneharness-cli/) via Trusted Publishing, and
   builds the per-platform npm packages and publishes them to
   [npm](https://www.npmjs.com/package/oneharness-cli).

So each release ships five ways: [PyPI](https://pypi.org/project/oneharness-cli/)
(`pip install oneharness-cli`), [npm](https://www.npmjs.com/package/oneharness-cli)
(`npm install -g oneharness-cli`), crates.io (`cargo install oneharness`), the
GitHub Release binaries, and `cargo install --git`. Only the binary gets a
`vX.Y.Z` tag and GitHub Release; `oneharness-core` is published to crates.io and
tagged in its own `oneharness-core-v*` namespace (no GitHub Release) so its
version never collides with the binary's `vX.Y.Z` tags.

PyPI publishing is keyless [Trusted Publishing](https://docs.pypi.org/trusted-publishers/)
(OIDC — no token secret), and stays dormant until the `PYPI_PUBLISH` repo
variable is set to `true`; the wheels still build on every release so a packaging
break surfaces early. Activating it requires the PyPI project `oneharness-cli` to
register this repo's `release.yml` as a Trusted Publisher (no GitHub Actions
environment).

npm packaging mirrors the wheels: maturin's `bindings = "bin"` wraps the prebuilt
binary in per-platform wheels; `scripts/npm-build.mjs` wraps the same binary in
per-platform npm packages (`@oneharness/cli-<platform>-<arch>`), pulled in as
optional dependencies of the `oneharness-cli` launcher so npm installs only the
one matching the host. npm publishing stays dormant until the `NPM_PUBLISH` repo
variable is `true` (the packages still build on every release, so a break
surfaces early) and authenticates with an npm token in the `NPM_TOKEN` secret (an
automation or granular-access token with publish rights to `oneharness-cli` and
the `@oneharness` scope).

Two repo secrets gate the automation (the workflow no-ops until both are set):
`RELEASE_PLZ_TOKEN` (a PAT with `contents: write` + `pull-requests: write`) and
`CARGO_REGISTRY_TOKEN` (a crates.io API token). Creating a GitHub Release by hand
(`gh release create vX.Y.Z`) is the supported fallback if the automation is
wedged; it triggers the same idempotent registry publication and artifact jobs.

## License

MIT — see [LICENSE](LICENSE).
