![oneharness](docs/assets/oneharness-banner.png)

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
  "schema_version": "…", // the contract version this binary emits
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
      "work": null,
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

| id | CLI | default binary | auth identity axis | `model` | `system` | `reasoning` | bypass mode requested | synced config file | allow / deny | hooks | output format | `--resume` (continue / fork) | `usage` headroom |
|----|-----|----------------|--------------------|:-------:|----------|-------------|-----------------------|--------------------|:------------:|:-----:|:-------------:|:---------:|-------------|
| `claude-code` | Claude Code | `claude` | `CLAUDE_CONFIG_DIR`, `ANTHROPIC_API_KEY` (live-proven) | ✓ | native flag | `--effort` | `--permission-mode bypassPermissions` | `.claude/settings.json` | ✓ / ✓ | ✓ | ✓ | `--resume` + `--fork-session` | `headroom` (`get_usage`) |
| `codex` | OpenAI Codex CLI | `codex` | `CODEX_HOME`, `CODEX_API_KEY` (live-proven) | ✓ | prepended | `model_reasoning_effort` | `--dangerously-bypass-approvals-and-sandbox` | — | — | — | ✓ | `exec resume <id>` (linear) | `headroom` (app-server) |
| `opencode` | OpenCode | `opencode` | `ANTHROPIC_API_KEY` (live-proven); stored auth (mapped, unproven) | ✓ | prepended | config only | `--dangerously-skip-permissions` | `opencode.json` | via `settings` | — | ✓ | `--session` + `--fork` | no plan quota |
| `goose` | Goose | `goose` | `GOOSE_PROVIDER` + `OPENAI_API_KEY` (live-proven); stored auth (mapped, unproven) | — | native flag | — | (runs unattended) | — | — | — | — | `--resume --name` (linear)¹ | no plan quota |
| `qwen` | Qwen Code | `qwen` | `OPENAI_API_KEY` + base URL (live-proven); OAuth/Coding Plan (mapped, unproven) | ✓ | prepended | config only | `--yolo` | `.qwen/settings.json` | ✓ / ✓ (interactive) | — | ✓ | `--resume` (linear) | no reader |
| `crush` | Crush | `crush` | `ANTHROPIC_API_KEY` (live-proven); stored login (mapped, unproven) | ✓ | prepended | config only | `run -q` (non-interactive) | `crush.json` | ✓ / ✓ | — | — | `--session` (linear) | no reader |
| `copilot` | GitHub Copilot CLI | `copilot` | token/BYOK/stored login (mapped, unproven; no usable host quota) | ✓ | prepended | `--reasoning-effort` | `--allow-all-tools --allow-all-paths --no-ask-user` | — | — | — | — | `--resume` (linear)¹ | `headroom` (GitHub API) |
| `cursor` | Cursor CLI | `cursor-agent` | API key/browser login (mapped, unproven; credentials absent) | ✓ | prepended | `--model 'M-<effort>'` | `--force` (`--trust` under `--no-bypass`) | `.cursor/cli.json` | ✓ / ✓ | — | ✓ | `--resume` (linear) | plan tier only |

The `usage` column shows how much subscription headroom
[`oneharness usage`](#subscription-headroom-oneharness-usage) can report for that
harness. Three expose real remaining-quota windows and one exposes a plan tier;
the other four cannot, and the column distinguishes *why*: **no plan quota**
means the quantity does not exist (OpenCode Zen is pay-as-you-go, Goose has no
first-party plan), while **no reader** means a real quota exists that the CLI
exposes no non-interactive way to read (Crush's Hyper credits, Qwen's weekly
Coding Plan quota).

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
rest (which have no id to bind a name to) `--session` is a loud usage error. A
named session automatically selects that harness's session-id-bearing output
format unless you explicitly set `--output-format`/config `output_format`; an
explicit format remains authoritative only when it can emit the id, otherwise the
run fails with a usage error before spawning.

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
  setting only changes how `text` is extracted. Codex defaults to its `--json`
  stream so plain runs capture `thread_id`; Qwen remains text by default and is
  upgraded to `stream-json` automatically when `--session` needs its id.
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
# typed Python API (includes the exact matching CLI package)
pip install oneharness-sdk
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
Linux, macOS, and Windows, and `cargo install --git`. The PyPI and npm CLI packages
both wrap the **prebuilt** binary — no Rust toolchain, no compile — carrying the
platform-specific binary in a per-platform artifact (a wheel; an
`@oneharness/cli-<platform>-<arch>` optional dependency) that the package manager
selects for your OS and CPU. Building from source requires a stable Rust
toolchain and [`just`](https://github.com/casey/just).
The same release publishes matching `@oneharness/sdk` and `oneharness-sdk`
language clients; each pins its packaged CLI dependency to that exact version.

Applications can use `@oneharness/sdk` (Node 20+) or `oneharness-sdk` / the
`oneharness_sdk` import (Python 3.9+) for the same complete surface: `run`,
streaming run, registry `list`/`detect`, and history lookup/list/watch. Both
streaming methods are async iterators; every JSONL envelope is validated before
it is yielded, and breaking or cancelling an iterator terminates the subprocess.
Both SDK distributions are stamped from the root Cargo version and depend on the
exact matching `oneharness-cli` package.

The SDK declarations, input contracts, and runtime validation schemas are
generated from one Rust JSON Schema bundle and drift-checked on every gate run.
Outputs preserve unknown fields for additive forward compatibility; inputs are
strict, so unknown option names and misspellings fail before a subprocess starts.
`HistoryStreamEnvelope` has no independent schema version: its event variant is
an opt-in additive capability behind history watch's `events` option. Existing
callers that omit that option continue to receive only `record` envelopes, while
the nested event/run lines remain governed by the current history schema;
readers also accept prior event-sourced versions.
Missing history records, sessions, and watch cursors raise a typed
`HistoryNotFoundError`. See the [Node SDK guide](npm/oneharness-sdk/README.md) and
[Python SDK guide](python/oneharness-sdk/README.md).

Both clients cover every verb this CLI exposes, and both build their command
lines from the same declared manifest rather than naming flags of their own, so
no consumer has to drop to raw argv for anything. A Rust consumer does not need
the subprocess at all: `oneharness-core` returns each verb's report directly.
Every gate run re-derives all of that from the capability manifest and from each
client's own source, so none of it can quietly stop being true.

History distinguishes provider timing from tool intervals observed by
oneharness. `model_ms` and `tool_ms` remain reserved for harnesses with explicit
provider lifecycle boundaries. Anthropic-envelope harnesses instead emit
`observed_tool_ms`, and each timed tool call carries
`timing_source: "stdout_observed"`; `model_ms` stays absent. These intervals run
from the pipe-read observation of a `tool_use` JSON record to its matching
`tool_result`, not from provider or harness-internal timestamps. They can include
error from JSON-line buffering, CLI flush timing, OS pipe and reader scheduling,
and delay between the model selecting a tool and the CLI emitting the record.
If no start boundary is observed, the fields are omitted—unknown is never
reported as zero.

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

`list`/`detect`/`config`/`sync`/`run`/`usage` emit JSON to **stdout**
(diagnostics go to **stderr**), `gate` speaks a harness's hook protocol on
stdin/stdout, and `init` scaffolds a starter config with a plain confirmation
line.

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
oneharness usage                                  # how much subscription headroom is left (costs no model turn)
oneharness usage --format text                    # …the same, for humans
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
  error. Without an explicit format, oneharness selects the harness's
  session-id-bearing format automatically; an explicitly pinned incompatible
  `--output-format`/config `output_format` is a usage error instead of a silent
  empty store. The higher-level counterpart to `--resume`; mutually exclusive
  with `--resume`/`--fork`/`--all` and with a batch. Under `--control` the flag
  also **names the control channel**, and whether it still continues a
  conversation is the mechanism's question: a driven turn builds no argv, so one
  without a resume request (`opencode-http`, `acp-cancel`, `crush-http`) refuses
  a continue rather than starting over in silence. See
  [Session handle](#session-handle).
- `--session-dir <dir>` — directory the `--session` store lives in (default:
  `<platform state dir>/oneharness/sessions`). Like `--resume`/`--fork`, a
  per-invocation knob with no config/env layer; mainly for isolating the store in
  tests and scripts.
- `--output-format <text|json|stream-json>` — override the format requested from
  each harness (default: per-harness); affects the emitted flag and how `text` is
  extracted. With `--session`, the explicit choice must be one of that harness's
  session-id-bearing formats or the run is rejected before spawning.
- `--schema <path>` / `--schema-max-retries <n>` — **structured output**:
  constrain each harness's final answer to a JSON Schema, validate it, and
  re-prompt on failure. See [Structured output](#structured-output) below.
- `--output-dir <dir>` — also write each harness's raw stdout/stderr to
  `<dir>/<harness>.stdout` and `<dir>/<harness>.stderr` (read transcripts from
  files without a JSON parser).
- `-- <args…>` — extra arguments appended verbatim to each harness command (for
  single-harness runs, since flags differ per harness).
- `--timeout <secs>` — optional per-harness timeout; by default a run has no
  deadline. `0` remains an explicit synonym for that default. With a positive
  limit, a hang becomes a `timeout` result that names
  the harness and enforced deadline, not a provider failure. oneharness owns the launcher's whole
  process tree, terminates descendants too, and bounds final pipe draining, so a
  native child cannot survive an npm/Node wrapper or hold the report open.
- `--cwd <dir>` / `--env KEY=VALUE` — run each harness in a directory / with extra
  env (useful for sandboxed e2e).
- `--max-parallel <n>` — cap concurrency (default: all selected at once).
- `--mode <read-only|plan|default|edit|auto|bypass>` — the approval mode
  requested from each harness (default `default`; see *Approval modes* below). A
  mode a selected harness **can't express** is a loud usage error before anything
  spawns; one that **may block on a prompt** headlessly is warned about and run,
  with a 120-second approval-wait safety deadline when no timeout was selected.
  Choosing `--timeout 0` explicitly removes that safety valve, makes any such
  approval wait unbounded, and produces a strengthened warning.
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

A `[harness.<id>.variant.<name>]` section is an opt-in named preset selected
everywhere as `<id>:<name>`. `--all` selects base harnesses only; variants never
silently join an all-run. Names match `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`.
Variants accept the same model/bin/args/env/reasoning and sync fields as their
base section. Precedence is built-ins → top level → base harness → variant →
CLI. Each report result retains the base `harness`, and also records `variant`
and the composed `harness_id`.

Credential values stay outside committed config. Within a variant child,
ordinary top-level/base environment is applied first, then `env_file`, variant
`env`, and `env_from` indirection; CLI `--env` follows, and `unset_env` masking
is final. `env_file` is a `KEY=VALUE` file (relative to the project unless
absolute). `env_from = { ANTHROPIC_API_KEY = "ANTHROPIC_API_KEY_WORK" }` maps a
differently named parent variable without changing the parent. `unset_env`
removes even an ambient canonical key, which is required for subscription
variants. These operations affect only the spawned child, so variants run
concurrently without credential leakage.

An `env_from` value that is an **absolute path** names the identity's home
directory (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, …). When that directory is **not on
disk**, nothing has been provisioned there, so oneharness does not run the
harness: the result is `status: "skipped"` with `available: true` and
`failure_kind: "auth"` — the same classification an *empty* home directory earns
from the harness itself, and one a [fallback chain](#fallback-mode-first-that-runs-wins)
routes around. So a second account can sit in a committed chain before anyone
logs into it, costing nothing and leaving no config directory behind for it.
Authenticate the path to activate the candidate. Only absolute values are checked
(a credential is never probed on disk), and only `env_from` — a committed `env`
path is the config author's own declaration, where a typo should stay loud.

Variants share the base harness's one native project config file. Selecting two
variants with conflicting sync fields is therefore a usage error, not
last-writer-wins.

```toml
# oneharness.toml — every field optional; shown with its CLI counterpart
harnesses = ["claude-code", "codex"]  # --harness (or `all = true` for --all)
exclude = ["cursor"]            # --exclude (applies to an `all` selection)
model = "gpt-5"                 # --model (single model, per harness)
models = ["opus", "sonnet"]     # repeated --model: fan out over the model axis
system = "Be terse."            # --system
bypass = true                   # legacy --bypass toggle (opt-in; default false)
mode = "default"                # --mode; beats `bypass` (default: "default")
# timeout = 300                # --timeout, in seconds; omitted or 0 means none
output_format = "json"          # --output-format
stream = false                  # --stream / --no-stream (incremental events)
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
allowed_tools = ["Bash(git status --short)", "Bash(git diff --check)"]
env = { ANTHROPIC_LOG = "debug" }

[harness.claude-code.variant.work]
env_file = "/home/me/.config/oneharness/work.env"
env_from = { ANTHROPIC_API_KEY = "ANTHROPIC_API_KEY_WORK" }

[harness.claude-code.variant.subscription]
env = { CLAUDE_CONFIG_DIR = "/home/me/.claude-work" }
unset_env = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]

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
- `1` — at least one harness `nonzero`/`timeout`/`spawn-error`ed or was
  `cancelled` (or, under `--require-available`, was missing; or, under
  `--schema`, never produced a schema-conforming answer).
- `2` — usage/configuration error (bad args, unknown harness, no prompt, an
  unreadable or invalid `--schema` file).

Under [`--run-mode fallback`](#fallback-mode-first-that-runs-wins) the rule is
different: `0` when the harness that ran succeeded, `1` when it ran but failed
**or** when no candidate could run at all — the fallen-through candidates never
count against the run.

### Cancelling a run (Ctrl-C, SIGINT/SIGTERM)

A harness launcher runs in its **own process group** (Unix) / Job Object
(Windows), so killing `oneharness` outright would not take the harness with it —
and an agent left running keeps working and billing. `oneharness run` therefore
handles `SIGINT`/`SIGTERM` itself while harnesses are in flight:

1. every running harness — **including one that has printed nothing at all** —
   and its whole descendant tree is terminated, the same teardown a `--timeout`
   performs;
2. any harnesses still queued are left unspawned;
3. the ordinary JSON report is still written to stdout, with each cut-short
   result as `"status": "cancelled"` (and an `error` saying so). Whatever output
   the harness had already produced is still normalized into `text`/`usage`/
   `events`, exactly as it is for a timeout;
4. the process exits `1`.

`cancelled` is distinct from `timeout` (the harness was given its whole deadline
and exceeded it): nothing was exceeded, the run was stopped. A **second**
`SIGINT`/`SIGTERM` exits immediately with `130` without waiting for teardown.

Library consumers get the same guarantee without touching process signals:
`oneharness_core::io::cancel::CancelToken` is passed as `RunControls::cancel` to
`io::run::run` (see [Driving a run from Rust](#driving-a-run-from-rust-no-oneharness-process)),
or directly to `runner::run_job_cancellable`, `run_job_streaming_cancellable`, or
`run_jobs_with_cancel`, and cancelling it tears the tree down through the same
path. `io::cancel::install_signal_cancel()` is the opt-in that wires the host's
signals to it (the CLI calls it for `run`).

### The result envelope vs. the normalized signals

The execution envelope — `command`, `exit_code`, `duration_ms`, `status`,
`stdout`, `stderr` — is **guaranteed and identical** across harnesses.

Alongside it, oneharness lifts a few **best-effort** signals out of each
harness's bespoke stdout so consumers don't have to parse it per harness. Each is
`null`/empty when it can't be found, is **never fabricated**, and (where there's
more than one possible method) records how it was found. This also applies to
bytes captured before a `timeout` or a cancellation: the result stays
`timeout`/`cancelled`, while complete records can still populate `text`, `usage`,
`session_id`, and `events` (a truncated final JSONL record is ignored rather than
invalidating earlier ones):

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
- `telemetry` — the measured execution telemetry for the run, `null` when nothing
  was measured (never estimated). Internally tagged by `source`, so a consumer
  switches on one value:
  - `provider_measured` — the harness's transcript carried a complete provider
    trace: `{ started_at, finished_at, model_ms, tool_ms,
    time_to_first_token_ms }`, the model/tool split of the run's wall clock.
  - `stdout_observed` — `{ tool_ms }`, the union of tool intervals observed at
    the stdout pipe, for a harness whose transcript has no provider trace to
    split. Not provider-measured, and with no model-latency counterpart.
  - `partial_invocation` — `{ started_at }` only: a trace-capable run that failed
    before its trace completed. When it began is measured; a split read out of a
    transcript that stopped mid-turn would not be.

  The same numbers are frozen into the history record; the field is on the result
  so a consumer reading a run it just made never has to re-open the history file
  for them.
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
  output carries no machine-readable trace — a plain-text harness (Goose, Qwen,
  Crush, Copilot), or Claude Code's single-document `json` result, which
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
  | `codex` | `exec --json` (default; `--events` needs no upgrade) | `json:codex-items` |
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
  runs one harness at a time (no batch, no `--schema`); `--stream` implies the
  `--events` format selection. In the default `parallel` mode that means exactly
  one selected harness — several would interleave their streams on one stdout —
  but a whole [`--run-mode fallback`](#fallback-mode-first-that-runs-wins) chain
  is allowed, because only the candidate that runs ever publishes events.
  Streaming is also settable declaratively as `stream = true` in config (or
  `ONEHARNESS_STREAM`), so a consumer that always reads events does not have to
  inject the flag per invocation; `--stream` / `--no-stream` beat it either way.
  A config that asks for streaming on a selection that cannot stream raises the
  same loud usage error the flag does — turn it off for that call with
  `--no-stream`.
- `failure_kind` / `failure_kind_source` — on a non-zero run, a coarse reason
  (`auth`, `rate_limit`, `model_not_found`, `quota`, `session_not_found`,
  `untrusted_directory`, `input_too_large`) so a
  caller can tell a retryable condition from a broken request. `session_not_found`
  is the harness refusing to continue a session its identity has never seen — a
  resumed token belongs to exactly one identity's session store, so it is a
  provisioning miss like `auth`, not a task failure. `untrusted_directory` and
  `input_too_large` are **precondition** refusals: the harness's own check failed
  before it made the request at all (Codex declining a directory it does not
  trust, or an input past the provider's character cap), so no model was called
  and no tokens were spent. When the provider states the cause in machine-readable
  terms — Codex's `{"input_error_code":"input_too_large","max_chars":…,
  "actual_chars":…}` — that object is quoted **verbatim** into the result's
  `error` and into the fallback block's `detail`, so a caller can shard against
  the real cap instead of re-parsing raw stdout. This is **distinct from `status`**,
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

  **A run that completed and did the work carries no `failure_kind`.** A refusal
  classification says the harness or its provider would not run the task, and a
  process that exited `0` having recorded a tool call or billed tokens is that
  claim's own refutation — so the envelope wins and the classification is not
  stamped. Either kind of evidence is enough on its own: a harness that reported
  no accounting at all and ran a tool did the task just as decisively. It is read one record at a time out of a transcript, while the
  envelope is what the harness said about the whole run: a 94-minute Claude Code
  turn that exited 0 having billed $12.11 published `failure_kind: "rate_limit"`,
  read off an intermediate `is_error` record it had already retried past, and the
  supervisor consuming that field killed the finished dispatch and threw the
  completed work away. `tool_deferred` is unaffected, being the one kind that is
  not a refusal: it describes what a run that *did* complete produced, so a clean,
  billed exit is its shape rather than a contradiction of it.
- `work` — what a run that failed with **nothing classified** has to show for
  itself: `"done"` where the harness recorded a tool call or billed usage,
  `"none"` where nothing says it got that far. `null` on every other run (a
  success raises no such question, and a classified failure has already answered
  it). It is the reading the fallback verdict itself consults, published rather
  than left to be re-derived: without it a candidate that never got started — a
  launcher shim that could not resolve the binary, say, exiting in 60ms — looks
  exactly like one that tried the task and failed it, and a chain that stopped at
  the first reads like a chain that stopped at the second. Publishing it changes
  no verdict: a candidate reading `"none"` still stops a chain (see
  [fallback mode](#fallback-mode-first-that-runs-wins)), and one reading `"done"`
  still never falls through it.

Coverage is keyed off each harness's documented output shape — Claude Code's
`result` JSON, OpenCode's JSONL (`text` parts for the answer, `step_finish` for
usage), Cursor's `stream-json` — and widens as more shapes are sourced; an absent
signal is the honest answer, not an error. Consumers that need certainty should
parse `stdout` themselves.

#### Streaming events

The CLI already emits the normalized events incrementally with `run --stream`,
using the Rust-owned `RunStreamEnvelope` contract described above. Non-streaming
runs still return the same events at the end in `RunReport.results[].events`. It
composes with [`--run-mode fallback`](#fallback-mode-first-that-runs-wins), so a
consumer that needs a chain surviving a subscription limit does not have to give
up watching the turn.

The Node and Python SDKs expose this contract as `runStream` / `run_stream` async
iterators. A behavioral consumer such as `skilltest` can **short-circuit the
moment it observes bad behavior** (a forbidden `rm -rf`, an out-of-scope network
call), killing the run instead of paying for a full turn before judging it.

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
A named session is **bound to one identity** — the variant-qualified harness id
(`claude-code:alternate`), because each [variant](#configuration) points its
harness at its own home directory and therefore keeps a disjoint session
namespace, so a token minted under one is meaningless under another. Reusing the
name on a different harness *or a sibling variant* is a loud error. It cannot
combine with `--resume`/`--fork`/`--all` or a batch, and is
supported only for harnesses that expose a session id headlessly (`session_capable`
in `oneharness list`: `claude-code`, `opencode`, `codex`, `cursor`, `qwen`) — for
the rest, `--session` is a usage error rather than a silent fresh start. Session
ids are format-dependent: with no explicit format oneharness selects the
harness's preferred session-bearing format (notably Qwen `stream-json`; Codex now
defaults to `--json` for every run). An explicit `--output-format` or config
`output_format` still wins only when that format can emit the id; pairing
`--session` with an incompatible format such as `text` is a usage error before
the harness runs, never a warning after a lost capture. In the
default **parallel** run mode it is single-harness; under
[`--run-mode fallback`](#fallback-mode-first-that-runs-wins) it is allowed on a
multi-harness chain and binds to the **anchor** — the candidate the stored record
already belongs to when it is still in the chain, else the first session-capable
one in priority order. The token is applied to the anchor's argv only, so no other
candidate is ever handed an id its identity cannot resolve. When the anchor falls
through (its quota is spent, or it can no longer resolve the token) the next
candidate does the turn *fresh* and the handle follows it: the store rebinds to
whoever ran, with a warning on stderr, because a native token cannot move between
identities. Continuity costs one conversation; the dispatch keeps going.

**Under `--control` the mechanism decides, not the output format.** A control
mechanism that *drives the turn* over its own protocol
([Turn control](#turn-control-interrupt-a-running-turn)) negotiates the prompt,
model, working directory and approvals on the wire and builds no argv at all, so
the harness's `--resume` mapping is never reached and the only way to continue
one conversation is the protocol's own resume request. One driven mechanism has
one — `codex-app-server`'s `thread/resume` — and `claude-control-request` needs
none, because it rides the harness's ordinary `-p` run whose `--resume` argv
carries the handle exactly as it does without `--control`. On the mechanisms
without one (`opencode-http`, `acp-cancel`, `crush-http`) a **create** still runs — a new
conversation is what was asked for — and says on stderr that this handle will not
continue; the next `--control --session <name>` turn on it is then a **loud usage
error** naming the mechanism, never a silent fresh start. Note the sharp edge
this closes for `opencode`, whose ordinary run *does* carry a session id: the
same name that continues fine without `--control` cannot be continued with it.

Records written before oneharness bound sessions to the variant-qualified id
(store `schema_version` `0.1`) name only the base harness, so which identity minted
their token is unrecoverable. Such a record starts a **fresh** session rather than
resuming a guessed identity — once, on the next run of that name.
This is the substrate a multi-turn driver (e.g. a simulated-user / skill-testing
framework) builds on: thread one handle, get faithful state, read `events` for what
the agent *did*.

### Turn control (interrupt a running turn)

A dispatched turn can run for many minutes. Without a control channel the only
way to redirect one that has gone the wrong way is to kill the dispatch — which
throws away the whole turn *and* its session. `--control` gives a supervisor a
lever that keeps the session:

```bash
# The dispatch. Opens <session-dir>/control/triage.sock for the run's lifetime.
oneharness run --harness claude-code --control --session triage \
  --prompt "Refactor the parser." &

# A SEPARATE process, whenever the supervisor decides to redirect it.
oneharness interrupt --session triage

# The turn aborted, the session survived: the next turn continues with full context.
oneharness run --harness claude-code --session triage --prompt "Stop there — do X instead."
```

Those last two steps are one operation with `--input`: the turn stops **and**
the message becomes the next turn, on the same live dispatch.

```bash
oneharness interrupt --session triage --input "Stop there — do X instead."
```

The answer says the run took the message (see the frames below). *Atomic* here
means **committed with the abort, delivered at the turn boundary**: every one of
these protocols drops a message sent into a turn already in flight (Claude Code
silently discards a mid-turn `user` frame; codex and ACP refuse a second turn on
a busy thread; both HTTP servers queue against the turn they are still running),
so a supervisor doing this by hand has to stop, wait, and then send — and each of
those waits is a window where the turn is dead and the message is nobody's.
Instead the run takes ownership in the same request that aborts the turn: parked
before the abort goes out, handed back if the abort fails, and written by the run
itself the moment the turn ends. So the two answers a supervisor can read are
"stopped, and your message is mine to deliver" and "nothing happened, and your
message is still yours" — never a third where both were lost. The redirected turn
runs on the same session in the same posture, working directory and model, and
the run's report covers both turns.

A redirection is spliced into another program's protocol frame, so it has to be a
message rather than bytes: `--input` is refused as a **usage error** (exit 2,
nothing sent) when it is blank, past the bound the refusal itself names, or
carrying characters that are not message text.

`interrupt` being a separate process is the whole point, and is why the channel
is a socket rather than a flag. The protocol is one newline-terminated JSON
request per connection, one response, connection closed:

<!-- Every frame below is reconciled against the type that encodes it by
     `the_readme_documents_the_control_protocol_frames_in_force` (tests/cli.rs),
     so this block cannot drift from `domain::control` unnoticed. -->
```jsonc
→ {"v":2,"verb":"interrupt"}
→ {"v":2,"verb":"interrupt","input":"<redirection>"}
← {"v":2,"ok":true,"mechanism":"<shape-id>"}
← {"v":2,"ok":true,"mechanism":"<shape-id>","redirected":true}
← {"v":2,"ok":false,"error":"<msg>","reason":"unsupported"|"no_active_turn"|"not_running"}
```

`interrupt` is the only verb; `v` is what leaves room to add one later (some
harnesses can also *steer* a turn without ending it, which is deliberately out of
scope). The optional fields above are omitted when absent, so a plain stop gains
no field a supervisor did not ask for — `v` is the only thing that changed about
it. And `v` itself is checked strictly in both directions, so an older
`oneharness interrupt` against a newer run (or the reverse) is told which side
speaks what rather than having a field it does not understand silently dropped. The three refusal reasons are distinct
because a supervisor reacts differently to each: `unsupported` is permanent for
the harness, `not_running` means the dispatch is gone, `no_active_turn` means the
run is alive but between turns. `interrupt` exits 0 when the request was served
and 1 when it was refused.

Requirements, all loud usage errors before anything spawns — a control lever that
silently is not there is worse than none:

- **`--session <NAME>` is required.** The socket is addressed by the caller-owned
  handle; oneharness never infers one, because a run nobody can name is a run
  nobody can interrupt. That handle is also a *conversation*, and a mechanism
  that drives the turn over its own protocol can only continue one by asking the
  protocol to — so **continuing a stored handle over a mechanism with no resume
  request is refused**, naming the mechanism. Creating one on such a mechanism
  still runs, and says on stderr that it will not continue. See
  [Session handle](#session-handle).
- **Exactly one live turn.** In the default `parallel` run mode that means
  exactly one harness, which must declare a control mechanism (`control` in
  `oneharness list`). A `--run-mode fallback` chain starts its candidates one at
  a time — it reaches candidate N+1 only because candidate N has finished — so it
  is already one turn and is accepted whatever its length, **whatever mechanisms
  its candidates use**. Every candidate must declare one, because any of them can
  end up serving and a supervisor told the lever exists must never find the
  candidate serving them has none; they do *not* have to declare the same one.
  The mechanism is **late-bound**: the socket address is the run's for its whole
  lifetime, and what sits behind it is whichever candidate is serving — bound as
  that candidate takes the turn, released when the turn ends, and re-bound by the
  next one if the chain falls through. So a chain may mix `claude-code`'s stdin
  frame, `codex`'s app-server and a **server-submitted** mechanism
  (`opencode-http`, `crush-http`) freely; falling through a pooled-server turn
  releases its lease before the next candidate leases anything, so there is never
  a second live turn.

  An interrupt is therefore answered by whatever is bound when it arrives, and
  the answer names *that* mechanism. In the window where nothing is bound —
  before the first candidate opens a turn, across a fall-through, after the last
  one ends — it is `no_active_turn`. That is deliberate: queuing the abort for
  whichever candidate binds next would land a supervisor's stop on a turn they
  never saw start, and reaching back to the candidate that just finished would
  write at a mechanism nobody is on. The run's `control.mechanism` reports the
  one that served (before any turn opens, the one the chain starts on).
- **Unix only** (the socket has no Windows equivalent in `std`). Checked last, so
  a request that is *also* wrong in a platform-independent way — an unsupported
  harness, mode, or output format — is refused with that reason instead, which is
  the one that survives changing machines. `--print-command` is exempt: it opens
  no socket and spawns nothing, and the argv it prints is the same everywhere.
- **No `--mode edit` on OpenCode's controlled turn.** OpenCode carries `edit`
  in its own config environment (`OPENCODE_CONFIG_CONTENT`), which belongs to the
  pooled `opencode serve` process a controlled turn is submitted to — not to the
  turn — so the mode cannot travel with it. It is refused before anything spawns
  rather than run under whatever policy that server already had. Use `--mode
  default` (deny and continue) or `--mode bypass`. Every other harness supports
  under `--control` whatever it supports without it, at the same policy.
- **No `--stream` on a server-submitted mechanism** (`opencode-http`,
  `crush-http`): those turns never spawn the harness CLI, so there is no stdout
  to publish line by line — and accepting the flag would silently select the
  ordinary run, whose interrupt does not reach the turn. The report still
  carries the whole event transcript as the result's `stdout`. On a chain this
  is refused if *any* candidate declares one: `--stream` is a promise about the
  run's stdout made before a candidate is chosen, and discovering mid-chain that
  the serving one cannot keep it is the silent downgrade the flag prevents.

The socket is created mode `0600` under a `0700` directory and removed when the
run exits. A dispatch killed with `SIGKILL` cannot run its cleanup, so a stale
socket file can survive; a client then gets `ECONNREFUSED` and reports
`not_running`, the same answer as a missing file. The run report gains a
`control` block recording the socket, the mechanism, and every request served, so
a consumer can tell an interrupted turn from one that simply ended.

**Without `--control` nothing changes**: no socket, no extra process, and a
byte-identical argv.

#### Approval modes under control

`--control` is a lever on a turn, not a policy. **For every harness that declares
a mechanism and every `--mode` that harness supports, a controlled run is under
exactly the policy the same mode gives without `--control`** — pinned cell by
cell as a unit assertion (`domain::control`'s `control_mode_parity`), which reads
both postures out of the real code rather than out of a second table.

That is an invariant with a history. `--control --mode bypass` once asked codex's
app-server for a `workspaceWrite` sandbox where `codex exec` under the same mode
asks for none at all, so the controlled run was *more* restricted than the
uncontrolled one — and on a host without unprivileged user namespaces every shell
call failed before running, so the turn did no work whatsoever.

It holds because the control path delivers the harness's **own** mapping wherever
one exists, rather than deriving a second policy for the protocol:

| Harness | How the mode reaches a controlled turn |
| --- | --- |
| Claude Code | Its control frame rides the ordinary `-p` run, so the mode's flags are the ordinary ones, byte for byte |
| Copilot | Its permission flags are top-level options that sit beside `--acp`, so the ACP launch carries the same `--allow-tool`/`--deny-tool`/`--mode` arguments `-p` gets |
| Goose | `GOOSE_MODE` is injected into the ACP child exactly as into an ordinary run — the control child *is* an ordinary job |
| OpenCode | The wire posture, for every mode the wire can carry. A mode whose only mapping is `OPENCODE_CONFIG_CONTENT` (its `edit`) is **a known gap**, refused as a usage error — see below |
| Codex | The app-server negotiates the sandbox per turn, so its `SandboxPolicy` is asserted equal to the sandbox `codex exec` selects for that mode |
| Crush | `crush run` cannot gate at all, so its `default` acts without asking — and a controlled turn declares the same `yolo` posture rather than gating what the CLI would not |

Where a harness answers permission requests on the wire, the answer is the
harness's own posture for that mode (`ModeSpec::posture`), not the
normalized spectrum's — which is what lets crush's ungated `default` be expressed
instead of refused.

**One known gap, named rather than hidden.** A mode delivered *only* through the
harness's own config environment cannot reach a turn submitted to a pooled
server. Handing that environment to the server was tried and reverted: it made
the approval mode a component of the pool key, and the controlled `--mode
default` turn it was meant to prove ended in `status=timeout` on OpenCode across
four consecutive CI cycles. So OpenCode's `edit` is a loud usage error under
`--control`, the grid carries the cell as
`known-gap:mode-env-not-delivered-to-a-pooled-server`, and `scripts/e2e-control.sh`
reports OpenCode's mode phase as a known gap instead of running it. A cell
dropped from the grid would read as coverage; a named one reads as the hole it
is. For every other harness that suite proves the delivered policy is *honored*,
by driving a controlled turn under the gating `--mode default` and requiring it
to end.

#### Control support matrix

Every entry is sourced from a real interrupt against the real CLI —
`scripts/explore-control.sh <id>` stands up each harness's control path, drives a
multi-step turn, interrupts it, and reports whether work actually *stopped*
(measured on the filesystem, because several harnesses report a normal
`end_turn` after a genuine cancellation). `LIVE` means an interrupt through
**oneharness** was proven end to end by `scripts/e2e-control.sh`; a mechanism
that is only probe-verified is **not** declared in the registry, so
`oneharness interrupt` can never report success on a path nobody exercised.

`--session` continues under `--control`? is the mechanism's own question, not the
harness's: a driven turn builds no argv, so a mechanism with no resume request
cannot reopen a conversation and a continue over it is refused
([Session handle](#session-handle)).

| Harness | `control` | Mechanism | Continues a `--session`? | Status |
| --- | --- | --- | --- | --- |
| Claude Code | `claude-control-request` | A `control_request` frame on the run's own stdin (`-p --input-format stream-json`) | ✓ (the ordinary `-p` run's `--resume` argv) | **LIVE** through oneharness |
| Codex | `codex-app-server` | `turn/interrupt {threadId,turnId}` over the `codex app-server` JSON-RPC stdio protocol | ✓ `thread/resume` | **LIVE** through oneharness |
| Copilot | `acp-cancel` | The ACP `session/cancel` **notification** over `copilot --acp` | — (loud usage error) | **LIVE** through oneharness |
| Goose | `acp-cancel` | The same ACP `session/cancel` **notification** over `goose acp` | — (loud usage error) | **LIVE** through oneharness |
| OpenCode | `opencode-http` | `POST /api/session/{id}/interrupt` against a pooled `opencode serve` | — (loud usage error) | **LIVE** through oneharness |
| Crush | `crush-http` | `POST /v1/workspaces/{id}/agent/sessions/{sid}/cancel` against a pooled `crush server` | — (loud usage error) | **LIVE** through oneharness |
| Cursor | — | none | n/a | cursor-agent exposes no headless control surface |
| Qwen | — | none | n/a | qwen exposes no headless control surface |

Every declared mechanism carries a redirection, so `--input` is never refused for
being unsupported: each one delivers it through **the same frame or route that
opened the turn in the first place**, on a session that already exists (the
per-mechanism spelling is on `ControlShape` and its dialogue/HTTP route
functions, which are what actually send it). There is no mechanism that can abort
a turn but not open the next one, so there is no harness that has to emulate this
with a racy stop-then-send — and `scripts/e2e-control.sh` proves the redirected
work is really done, per harness, on the filesystem.

What differs between them is *when* the run knows the aborted turn is over, and
it is measured rather than assumed. Most announce it — Claude Code's `result`,
codex's `turn/completed`, ACP's prompt response, crush's `run_complete` — so the
redirection rides that. **Opencode announces nothing**: an aborted turn's event
stream simply stops, with no `session.idle`, so a run waiting for one waits out
its whole timeout. Its interrupt route is synchronous, so there the served
interrupt *is* the ending and the redirection goes out as soon as the abort
lands. Either way the message is the run's from the moment the interrupt is
accepted, and it is never submitted before the abort it rides has landed.

**Crush needs a provider its server can actually call.** It resolves one from the
ambient environment, and no single variable selects it, so a host carrying AWS
selectors and no `ANTHROPIC_API_KEY` falls through to Bedrock — where a role
without `bedrock:InvokeModelWithResponseStream` answers `403 Forbidden`, the
agent never runs a tool, and there is no work whose freeze could be measured.
That is an unusable *credential*, not an absent mechanism: with
`ANTHROPIC_API_KEY` present crush picks Anthropic even alongside an `AWS_PROFILE`,
and the cancel route then freezes the turn. `oh_control_enforce crush` is what
holds that proof.

**Copilot's phase asks copilot, not the environment.** `copilot login` stores its
token in the OS credential store, so a host with no `GH_TOKEN`/`GITHUB_TOKEN`
runs copilot perfectly well — and a token check retires the phase on exactly such
a host, which is how copilot's control path came to ship with no live alarm at
all. The gate is a zero-turn ACP `session/new` (`oh_copilot_login_ready`): a
session that opens is a usable login, `Authentication required` is not, and
neither spends AI credits. Copilot also blocks until the client answers
`session/request_permission`, so the step files the freeze assertion counts exist
only because oneharness answered it — the permission path is proven by the same
run rather than assumed.

A declared mechanism means oneharness **drives the turn** over that protocol
rather than through the harness's ordinary headless run: it spawns
`codex app-server` / `copilot --acp` / `goose acp` as the run's own child,
negotiates the thread or session, sends the prompt, and holds that same stdin
open so the interrupt reaches the live turn. Model, working directory, sandbox
and approvals are negotiated on the wire, so they leave the argv entirely —
which is also why Copilot and Goose can take `--session` under `--control` even
though none of their ordinary output formats carries a session id. That handle
**names the channel**; whether it also *continues a conversation* is the
mechanism's own question, answered in the table above and enforced before
anything spawns.

**OpenCode and Crush turns are submitted to their servers, not to their CLIs.**
This is the third execution model, and the only one their interrupt reaches.
Interrupting an ordinary `opencode run` was REFUTED both ways it can be pointed
at a server: `run --port <n>` binds nothing (the port never appears in `ss
-ltn`), and `run --attach http://…` leaves the attached server's
`/api/session/active` **empty while the run is creating files** — so the
interrupt answers `2xx` while the work continues (measured: 3 → 9 step files in
the 15s after a "successful" interrupt). Crush's `run` has no attach flag at
all. So under `--control` oneharness never spawns either CLI: it leases the
harness's server from the pool, creates a session on it, follows its event
stream, and answers what the server blocks on. The recorded `command` is
therefore the *server's* launch argv, and the run's `stdout` is the event
transcript oneharness actually saw.

Four things that path has to get right, each learned from a run that got it
wrong:

- **Both servers block on a permission decision**, exactly as ACP does. Crush
  emits `permission_request` and waits; opencode emits `permission.*`. A
  permissive run tells crush once (`permissions/skip`) and answers opencode's
  per request (`…/permission/{id}/reply`). Without an answer the agent never
  runs a single tool.
- **Opencode announces `session.idle` before the prompt is admitted**, not only
  after the turn ends — and its `/wait` route returns immediately in the same
  window. A driver that treats either as the end of the turn finishes every run
  in under a second having done nothing. The end of the turn is idle *after*
  `session.next.prompt.admitted`.
- **The working directory is per turn on both** (opencode's
  `location.directory`, crush's workspace `path`), which is what lets one server
  be shared across dispatches in different projects without the cwd widening the
  pool key.
- **Opencode's model is per turn too, and the session is the only place it takes
  one.** `POST /api/session` accepts `{"providerID": …, "id": …}` (both required
  by the server's own `/doc` schema), and the session it answers with runs its
  steps on exactly that. The server's config is NOT that place: `opencode serve`
  loads a `model` from `OPENCODE_CONFIG_CONTENT` and echoes it back on `/config`
  while creating sessions on its own choice regardless (measured against 1.18.5).
  A session opened without a model therefore runs on whatever the server picked —
  live that was `wandb/nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B` on one host
  and `ling-3.0-tiny-free` in CI, both answering `Provider request failed with
  HTTP 401`. Only the first `/` splits the id: opencode's own model ids contain
  slashes, so a `--model` naming no provider is refused rather than guessed at.
- **Crush's routes are not where they look.** A session is created on the
  *workspace* (`POST /v1/workspaces/{id}/sessions`; `/agent/sessions` answers a
  bare `404 page not found`) and the prompt goes to `POST
  /v1/workspaces/{id}/agent` with the session in the **body**
  (`/agent/sessions/{sid}` is GET-only and answers `405`), returning `202` with
  the turn running in the background.

**Goose takes its provider and model from the environment, in ACP too.**
`goose acp` resolves `GOOSE_PROVIDER` / `GOOSE_MODEL` (plus the matching
provider key) exactly as an ordinary `goose run` does, and neither travels on
the argv or the wire — so with none set, `session/new` fails `-32603 Internal
error: Failed to resolve provider` and the run never reaches a turn to
interrupt. `scripts/e2e-control.sh` exports them for the goose phase like
`e2e-goose.sh` does.

Notes worth keeping, all from the probe rather than documentation:

- Claude Code **silently drops** a plain user message written mid-turn, so the
  `control_request` frame is the only mechanism that works.
- The ACP client **must answer `session/request_permission`** — goose and copilot
  block indefinitely and never begin work otherwise, which is easy to mistake for
  a slow or broken harness — and cancel must be sent as a **notification** (with
  an `id`, goose answers `-32601 Method not found`).
- **Opencode does not honor `Connection: close`** on every route — it answers in
  full and leaves the socket open. A client that reads until EOF therefore times
  out on an answer that already arrived, and reports the server as unreachable:
  `--control` failed readiness against a server that was up and answering `200`.
  The answer ends where its own framing says it does (the terminating chunk, or
  `Content-Length` bytes), and only a server that framed it neither way is read
  to EOF.
- Crush's `client_id` is a self-assigned UUID that travels in the request **body**
  when creating a workspace but as a **query parameter** on every other route; a
  mismatch yields a bare `{"message":"invalid client_id"}`. Its prompt POST
  returns `202` immediately, so control is fire-and-forget against the event
  stream rather than request-scoped, and its server answers
  `Transfer-Encoding: chunked` (an HTTP client that skips de-chunking reports a
  JSON decode error where the harness in fact answered correctly).
- Codex must **not** be driven through `codex app-server daemon`: it requires a
  managed standalone install this project does not use and self-updates from a
  fixed path, which conflicts with pinning an exact version.
- Codex answers `turn/start` **immediately** with the new turn's id and
  `status: "inProgress"`. That response is the acknowledgement, not the end of
  the turn — reading it as terminal ends every controlled run in under half a
  second, before the agent does anything. `turn/completed` is the end.

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
| Did the task's work (a tool call, or billed tokens/cost) | ⛔ stop — it ran, whatever its record says |
| Not installed (`skipped`) | ✅ fall through — `not-installed` |
| Installed, but the variant's [`env_from`](#configuration) home directory is absent (`skipped`, `auth`) | ✅ fall through — `auth` |
| Resolved but unspawnable (`spawn-error`) | ✅ fall through — `spawn-error` |
| Ran, exited non-zero, classified `auth`, no work done | ✅ fall through — `auth` |
| Ran, exited non-zero, classified `quota` (no credit), no work done | ✅ fall through — `quota` |
| Ran, exited non-zero, classified `rate_limit`, no work done | ✅ fall through — `rate-limit` |
| Refused a resume it cannot resolve, classified `session_not_found`, no work done | ✅ fall through — `session-not-found` |
| Refused the directory it was pointed at, classified `untrusted_directory`, no work done | ✅ fall through — `untrusted-directory` |
| Refused the input as too large, classified `input_too_large`, no work done | ✅ fall through — `input-too-large` |
| Ran and succeeded (`ok`) | ⛔ stop — this is the answer |
| Ran and failed the task (`nonzero`, incl. `model_not_found`) | ⛔ stop¹ |
| Timed out (`timeout`) — a slow but genuine run | ⛔ stop |
| Never produced a schema-conforming answer (`--schema`) | ⛔ stop (the harness ran) |

¹ **Exception — a model list.** When the run is fanning out over several models
(repeated `--model` / config `models`; see [Multiple models](#multiple-models-fan-out-over-the-model-axis)),
a per-model rejection means "try the next model", so `model_not_found` (fall
through — `model-not-found`) *does* fall through. With a single model it still
stops the chain, as above — an unknown model is a configuration mistake the user
should see rather than one oneharness silently routes around.

> **Behavior change.** A `rate_limit` used to fall through only under a model
> list, and to stop the chain otherwise. It now falls through on any chain, with
> reason `rate-limit`. A rate limit is a property of whoever is being billed, not
> of the model: one rate-limited identity ended a dispatch that four further
> identities could have served. The zero-work rule below is unchanged and still
> bounds it — a `429` that spent tokens describes a run, and still stops.

**An unresolvable resume falls through too.** A native session token lives in one
identity's session store — each `claude-code` variant points the CLI at its own
`CLAUDE_CONFIG_DIR`, so their namespaces are disjoint — and a harness handed a
token it has never seen refuses in about a second with no output and no usage
(`No conversation found with session ID …`, `no rollout found for thread id …`,
`Session not found`, `No saved session found with title …`). That is a
provisioning miss, not a task failure: the task is untouched and the next
candidate can still do it, so it classifies as `session_not_found` and falls
through. Left unclassified it read as a real failure and stopped the chain at the
one identity that could never serve it.

A **subscription/usage-limit** rejection classifies as `quota` and falls through
whichever way its CLI reports it: Claude Code's session or weekly limit, and
Codex's usage limit — the latter arriving as a `turn.failed` event on stdout
after the turn started, so it falls through on a clean exit too. A turn driven
under [`--control`](#turn-control-interrupt-a-running-turn) is read as well,
where the same refusal arrives over the `codex app-server` protocol instead: an
`error` notification and a `turn/completed` frame reporting `status: "failed"`,
both carrying `codexErrorInfo: "usageLimitExceeded"`. Only the limit does; an
ordinary failed turn is a real run and stops the chain, whichever transport it
came over.

That holds however the harness dresses the rejection up. Claude Code reports a
session limit in a terminal record that may say `subtype: "success"` with no
`is_error`, declaring the failure through `terminal_reason: "api_error"` and an
`api_error_status` of `429` instead — and the limit message still wins over that
embedded `429`, so the rejection reads as `quota` (fall through) rather than the
transient `rate_limit` (stop).

It also holds however the harness **words** the rejection, because the wording is
not what the classification rests on. A record declaring `api_error_status: 429`
whose own accounting shows it did nothing reads as `quota` whatever prose it
carries: a `429` is the provider saying *this identity may not run right now*,
which is exactly the condition the next candidate can serve, and a rejection that
did no work has nothing to lose by trying it. The limit phrases (`hit your
<session|weekly|…> limit`, `usage limit reached`) remain a fast path for the
surfaces with no record to read — a bare limit line on stderr, or a limit
reported without a status code. Only `429` gets this treatment: a zero-work `500`
is a provider fault the next candidate would hit too, and `401`/`403` already
have their own `auth` fall-through.

> **Behavior change.** A zero-work `429` *without* a recognized limit message
> used to stay a plain `rate_limit` and stop the chain. It now reads as `quota`
> and falls through. The two failure modes are not symmetric: an unrecognized
> phrase kills the run outright, while reading a transient `429` as quota merely
> hands the task to the next candidate — which is what the chain is configured
> for. A `429` that spent tokens is unaffected and still stops (below).

**A limit — or a `429` — only falls through when the run did no work.** `quota`
means the candidate could not run the task *at all*, so the discriminator is the
harness's own accounting, not the error text: a record reporting any non-zero
token count, any non-zero cost, or a non-empty `modelUsage` map describes a run
that got somewhere, and falling through it would burn the next candidate's quota
re-running work already paid for. So the same limit message with tokens spent is
an ordinary run — with the embedded `429` it lands as `rate_limit`, without one it
stays unclassified, and either way the chain stops. A limit with **no** accounting
at all (a bare `You've hit your session limit` line on stderr) still falls
through: absent accounting is not evidence of work.

> **Behavior change.** A Claude session/weekly limit that arrived *mid-run* used
> to fall through as `quota`. It now stops the chain. Only zero-work rejections
> fall through. This affects the limit signature specifically — the generic
> `insufficient_quota` / `credit balance` vocabulary means the account is out of
> money rather than out of session, and is unchanged.

> **Behavior change.** That zero-work discriminator is now applied to *every*
> fall-through reason, not just the Claude limit signature: any candidate whose
> result shows a tool call or billed usage stops the chain. A generic `auth`
> classification scanned out of a transcript, or a Codex `turn.failed` usage
> limit, no longer falls through once the candidate has done work. Rejections
> that did no work — the overwhelming majority, and the ones a fallback chain
> exists for — are unaffected.

The report gains a `fallback` block, `{ "ran", "fell_through": [{ "harness",
"reason", "detail" }], "stopped_without_work" }`: `ran` is the harness that
executed (or `null` when every
candidate failed to start), and `results` holds only the harnesses **attempted**
— the fallen-through ones in priority order, then the one that ran. Priority
order is the `--harness` / config `harnesses` order (registry order under
`--all`). Each fallen-through entry's `detail` is that candidate's own account of
why it could not run — the provider's machine-readable refusal verbatim when it
named one, else the result's `error` text, and `null` when it said nothing — so a
supervisor reading only this block never has to re-derive the cause from stdout.
`stopped_without_work` is the other half of that: `true` when the candidate the
chain stopped at failed with nothing classified *and* nothing to show it did any
of the task (its result's [`work`](#the-result-envelope-vs-the-normalized-signals) read `"none"`). The chain still
stops there — re-running a task that may genuinely have failed for free would
burn the next identity's quota on the same failure — but a reader is told which
kind of stop it was, instead of seeing untried candidates and no reason. Under
`--print-command` nothing executes, so the block is `null` and
every candidate's command is printed in priority order.

**The command must be valid for the whole set.** Every listed harness is
validated up front, so a flag no candidate could honor (an approval `--mode` a
listed harness can't express, an unsupported `--mock-rules` action, …) is a loud
usage error **before anything spawns** — even for a harness that is never
reached. This keeps people and agents writing commands that work for every
harness the fallback config supports, not just the one that happens to run.

Fallback is single-outcome by nature, so it refuses a [batch](#batch-runs-same-prefix-prompt-caching)
run and the low-level `--resume` / `--fork` continuations (each pins one specific
harness's native id) as loud usage errors. The higher-level
[`--session`](#session-handle) handle **is** allowed: it binds to the anchor (the
first session-capable harness in the chain), so a named conversation degrades
gracefully across the same priority set. Exit code: `0` when the harness that ran
succeeded, `1` when it ran but failed **or** when no candidate could run at all.

**Work evidence decides, so streaming changes nothing.** Before any of the
reasons above are consulted, a candidate whose result carries **evidence it did
the task's work** — a recorded tool call, or usage accounting with a non-zero
token count or dollar cost — is treated as having run, whatever its terminal
record then said. This is the "work done, not error text" rule the Claude
session-limit classifier already applied, lifted to the whole verdict so it also
covers the surfaces with no accounting of their own (a generic `401` scanned out
of a transcript, Codex's `turn.failed` usage limit). Falling through a candidate
that worked would burn the next one's quota re-running what already happened. A
rejection that did *no* work — the zero-token 429, bad credentials, a missing
binary — still falls through exactly as before.

**Streaming a fallback chain.** [`--stream`](#streaming-events) is allowed — over
harnesses and over [models](#multiple-models-fan-out-over-the-model-axis) alike —
and is how a supervising process watches a long turn while keeping the chain that
survives a 429. The candidates run one at a time, so nothing interleaves, and the
verdict comes from the same rule over the same normalized result — **a streamed
chain and a buffered chain always select the same candidate**, which the suite
pins end to end across zero-work rejections, a missing binary, a candidate that
worked before being rejected, a real task failure, and a timeout. Publishing is
safe under that rule because a candidate that publishes an event has a tool event
in its result, which is work evidence, so it cannot then be discarded; a candidate
that *does* fall through has published nothing a consumer could act on. Its whole
transcript is still in `results` — withheld from the live stream, not discarded.

```console
# Watch the turn while keeping the fallback chain:
oneharness run --run-mode fallback --harness claude-code,codex --prompt "Fix the failing test" --stream
```

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
`--resume` / `--fork` / `--session`. [`--stream`](#streaming-events) is refused
only in `parallel` mode, where the fan-out really is several concurrent results
whose event streams would interleave; under `--run-mode fallback` the pairs are a
priority chain with one outcome, so the chain streams like a harness chain does.

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
If a run times out after emitting parseable records, their normalized signals are
preserved here just as they are in the report; the record's status remains
`timeout`.

**A run that failed is recorded too.** A run killed at launch or cut short
mid-turn has no complete provider trace *because* it failed, and history keeps it
anyway rather than fabricating one — a history that could only hold successes
would hide exactly the runs an operator has to see. Its timing is whichever of
two honest shapes applies: **partial**, the invocation start the runner itself
watched, with no model/tool split (a split read out of a transcript that stopped
mid-turn is not a measurement); or **absent**, for a harness that was never
spawned at all. Such a record carries `status`, `exit_code`, the classified
`failure_kind`, the measured `duration_ms`, whatever partial transcript the
failure left (`events`, `usage`, any salvaged `text`), and `error`: the harness's own account of the failure as
captured on its stderr, or oneharness's own message when it generated one (a
spawn failure, a timeout, a binary that is not installed). `error` is trimmed,
bounded to 2048 characters (Unicode code points, like every other bounded string
here), omitted entirely when there is nothing to report, and never written for a
run that succeeded — so a working harness's stderr chatter stays out of history.
It is the one place a record quotes the process's own bytes, and it is never
taken from stdout, so it can never stand in for provider output the run did not
produce. It and partial timing both arrived in history schema **v1.3**;
a record's `schema_version` names the oldest reader that can understand it, so a
record carrying either declares `1.3` while a provider-measured success still declares `1.1`.
The same rule versions the enums a reader has to know: a `cancelled` run declares
**v1.4**, one classified `session_not_found` declares **v1.5**, and one classified
`untrusted_directory` or `input_too_large` declares **v1.6**. A record whose run
failed with **nothing classified** also carries `work` — the same `"done"` /
`"none"` reading the report publishes, and the only thing that tells a candidate
which never got started apart from one that ran the task and lost — and declares
**v1.7** for it.

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

Records can carry validated task-graph labels. Labels merge by key with the same
precedence (CLI > environment > project file > user file), so a nearer layer can
replace one key without discarding the others:

```toml
history_labels = { graph = "release", owner = "platform" }
```

```bash
ONEHARNESS_HISTORY_LABELS='graph=release,owner=ci' oneharness run … \
  --history-label owner=agent --history-label task=verify
```

Keys are 1–64 ASCII letters/digits/`.`/`_`/`-` and must start alphanumeric;
values are non-empty, at most 256 characters (Unicode code points, so a
multibyte value is bounded by what you can read, not by its encoded size), and
contain no control characters — every character Unicode calls `Cc`, which is C0,
DEL, and C1. The CLI and the language SDKs enforce this one contract identically.
Malformed config, environment, and CLI values are rejected before a run starts.

`--no-history` (or `history = false` in a nearer layer) turns it back off. Nothing
is written under `--print-command` (nothing runs).

**Layout.** `<history_dir>/<project-slug>/<session>.jsonl` — one file per
`oneharness run` invocation ("session"), partitioned by a slug of the project
directory. `history_dir` defaults to `<platform state dir>/oneharness/history`
(`$XDG_STATE_HOME` or `~/.local/state` on Linux, `~/.local/state` on macOS,
`%LOCALAPPDATA%` on Windows); set it with `--history-dir`, `history_dir`, or
`ONEHARNESS_HISTORY_DIR`. Relative project/cwd paths are canonicalized before
the record and slug are written, so later `list`/`show` lookups resolve the same
project even when the original run used `..`.

Each v0.2 record has a time-ordered UUIDv7 `history_id`, which is both an exact
lookup key and a watch cursor; empty `labels` are omitted. Readers continue to
accept v0.1 records, assigning deterministic UUIDv5 IDs and empty labels so the
same legacy line always migrates to the same identity. A `history_id` is
canonical hyphenated UUID text (`8-4-4-4-12` hex, either case) carrying the
RFC 4122 variant and a defined version; the unhyphenated, braced, and
`urn:uuid:` spellings are not the contract and are refused, as they are by the
SDKs' schema.

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
oneharness history show <session-id-or-name> [--last] [--all] # a session's records
oneharness history show <history-id>                          # one exact record
oneharness history watch [--label key=value] [--after <history-id>] --format jsonl
oneharness history clear [--all-projects] [--yes]            # dry-run unless --yes
```

`show` resolves its argument against a session **id or name** (name is
non-unique — the newest match wins, or `--all` shows every match); a UUID
`history_id` instead performs an exact record lookup across projects. `watch`
first emits matching records after its optional cursor, then follows the locked,
append-only `.index.jsonl` without rescanning the history tree. Reconciliation
on startup adds missing session records, ignores removed sessions, and truncates
a partial final index line left by an interrupted writer. Reusing the last
emitted `history_id` with `--after` resumes without duplication; repeated
`--label` filters are ANDed. `clear` reports
what it *would* remove and deletes nothing until `--yes`, so it is safe to run
non-interactively first.

### Subscription headroom (`oneharness usage`)

`oneharness usage` reports how much plan quota each harness identity has left,
**without any harness taking a model turn** — so it is the pre-flight check to
run *before* launching a long job, rather than the thing you learn after one
fails on quota.

```console
$ oneharness usage --harness claude-code,copilot,goose --format text
usage as of 2026-07-29T16:41:13Z

claude-code [CLAUDE_CONFIG_DIR=/home/u/.claude] · plan max · auth subscription
  five_hour: 42% used · resets 2026-07-29T18:30:00Z
  seven_day: 61% used · resets 2026-08-02T13:00:00Z ← binding
  weekly_scoped/Opus 5: 17% used · resets 2026-08-02T13:00:00Z

copilot [GH_TOKEN=<secret>] · plan individual · auth subscription
  chat: unlimited · resets 2026-08-01T00:00:00Z
  premium_interactions: 100% used (13518 of 1500 AI credits used, -12019 left · exhausted and blocked) · resets 2026-08-01T00:00:00Z

goose [ambient] · auth unknown
  no headroom to report: no first-party plan quota exists to report
```

JSON on stdout is the contract (`--format text` is the view above); it carries
its own `schema_version`, independent of the run report's.

Three things it will not do:

- **It never invents a number.** An identity with no readable headroom carries
  no percentage *at all* — the JSON has no field to hold one — so nothing can
  render as “0% used / plenty of room”. An unlimited quota reports `unlimited`
  rather than a full bar, and a window the harness reported as `null` (“not
  applicable to this plan”) is omitted rather than zero-filled.
- **It never takes a harness down.** A missing binary, an unauthenticated
  harness, a malformed payload, and a probe timeout are all reported as data;
  only a genuine usage error (an unknown id, an undeclared variant) exits
  non-zero.
- **It never authenticates anything.** Every probe reads existing credentials.
  In particular the Cursor probe reads a plan tier only from a **pre-existing**
  login and masks `CURSOR_API_KEY` from its child: Cursor's API-key path is not a
  per-process selector but a *login* that exchanges the key for tokens and writes
  them to the shared credential store, which has been observed overwriting a real
  user login. Absence of a login is reported, never resolved by authenticating.

Useful flags: `--all` / `--harness <id,…>` / `--exclude <id,…>` (selection,
defaulting to every harness — `--exclude` drops ids from that sweep and is
refused alongside `--harness`, which already names the selection),
`--format <json|text>`, `--compact`,
`--timeout <secs>` (per probe, default 60), plus the usual `--bin`, `--cwd`,
`--config`, and `--no-config`.

**Per-identity attribution.** A composed id selects a *distinct identity* using
the same variant machinery `run` uses, so two subscriptions of one harness are
reported separately — each entry carrying its variant name and the credential
directory that selected it (never the credential):

```console
$ oneharness usage --harness claude-code:work,claude-code:personal --compact
```

**Copilot needs a GitHub token, and nothing else.** It is read out of band from
GitHub's API, so it answers even where the Copilot CLI is not installed. The
token comes from `COPILOT_GITHUB_TOKEN`, then `GH_TOKEN`, then `GITHUB_TOKEN`
(Copilot's own documented precedence); with none of them set, the result is
`unknown` naming the variables rather than a claim about headroom. The probe
shells out to `curl` (the token rides its stdin config, never the argv), and
`ONEHARNESS_COPILOT_API_BASE` points it at a GitHub Enterprise host — an HTTPS
one, since the request carries the token: a plaintext `http://` base is refused
as a named probe failure unless it names a loopback host.

**Both upstream payloads are experimental**, so the drift guards are explicit:
codex's contract is snapshotted from `codex app-server generate-json-schema` and
diffed in `just check`, while Claude — which publishes no schema — is guarded by
asserting on `rate_limits_available` and the expected `limits[].kind` values, so
a shape change degrades to `unknown` instead of to zero.

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
headlessly (a `hangs` tag) is warned about on stderr but still run. When the
caller omitted a general timeout, this case alone receives a 120-second
approval-wait safety deadline, so an unattended prompt cannot silently stall
forever. An explicit `--timeout 0` removes that backstop and strengthens
the warning to say the wait is unbounded; `--permit-prompts` silences it once
allow-rules are synced so the prompt never fires.

| `--mode` | claude-code | codex | opencode | goose | qwen | crush | copilot | cursor |
|------------|:-----------:|:-----:|:--------:|:-----:|:----:|:-----:|:-------:|:------:|
| `read-only`| ✓ᵈ | ✓ˢ | ✓ᵖ | — | ✓ᵖ | — | ✓ᵈ | ✓ |
| `plan`     | ✓ | ✓ⁱ | ✓ | — | ✓ | — | ✓ | ✓ |
| `default`  | ✓ | ✓ | ✓ | ✓ | ✓ | ✓¹ | ✓ | ⚠ |
| `edit`     | ✓ | — | ✓ᵉ | — | ✓ | — | ✓ | — |
| `auto`     | ✓ | ✓ | — | ✓ | ✓ | — | — | — |
| `bypass`   | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

✓ supported & clean headless · ⚠ supported but may block on a prompt headlessly
(warns + runs; an omitted timeout gets the 120-second approval-wait safety
deadline, while `--timeout 0` opts out and `--permit-prompts` silences the warning) ·
— unsupported (refused). `read-only` is **enforced** where marked — ˢ Codex's
read-only sandbox (OS-enforced), ᵈ tool rules (Claude's `--tools Read Grep Glob
WebFetch WebSearch`, which narrows the built-in set to the tools that only read;
Copilot's `--deny-tool shell/write` — deny beats allow) — and ᵖ behavioral where
its only mechanism is the plan agent (OpenCode
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

### Driving a run from Rust (no `oneharness` process)

A Rust consumer does not have to spawn the CLI and parse its stdout: the whole
run is a call on the engine crate, and it **returns** the report.

```rust
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{run, EventSink, RunControls, RunRequest, SinkStep};

let request = RunRequest {
    harness: vec!["claude-code".to_string()],
    prompt: vec!["summarize this repo".to_string()],
    timeout: Some(300),
    ..RunRequest::default()
};

let cancel = CancelToken::new();          // hand a clone to your supervisor
let outcome = run(&request, RunControls { cancel: cancel.clone(), ..Default::default() })?;
println!("{}", outcome.report.results[0].text.as_deref().unwrap_or(""));
# Ok::<(), oneharness_core::errors::OneharnessError>(())
```

`RunRequest` is the `oneharness run` flag surface as plain data (everything
optional; the same `oneharness.toml` / `ONEHARNESS_*` layering applies unless you
set `no_config`). `RunOutcome` carries the `RunReport`, the exit code the CLI
would have returned, whether the run streamed, and the one-line failure summary
the CLI prints to stderr. `Err` means the *request* was refused before anything
spawned — a harness's own failure is always a `RunResult`, never an error.

Four things the CLI does for itself, which an in-process caller now chooses
(three on `RunControls`, the fourth through `run_supervised`):

- **Where events go.** Set `stream: Some(true)` and pass an `EventSink`; its `event`
  method is called as each normalized event arrives, and returning
  `SinkStep::Stop` short-circuits the turn (the CLI's own sink is the one that
  writes the NDJSON protocol to stdout — nothing inside the engine does).
- **How it is cancelled.** Each harness leads its own process group / Job Object,
  so no signal you send your own group reaches one; `RunControls::cancel` is the
  handle that does, tearing the whole tree down and still returning a report with
  `"status": "cancelled"`. Cancel and then *wait for the call to return* — killing
  your own process instead orphans a live, billing harness.
- **Who else owns the harness processes.** `run_supervised(&request, controls,
  Some(&supervisor))` takes a `ProcessSupervisor`, whose `spawning(&mut Command)`
  / `spawned(&Child)` hooks put each harness child into the process group (POSIX)
  or job object (Windows) **you** supervise — the grouping the subprocess hop
  used to provide, without which your activity watchdog cannot see the harness
  subtree as one unit and your own kill does not reap it. Observing hooks change
  nothing else: oneharness still tears the whole descendant tree down. A
  `spawning` hook that re-parents the child's process group takes that tree over
  — oneharness will not signal a group it did not create, since yours may hold
  your own processes, so it then confines its teardown to the direct child. (Its
  own entry point rather than a `RunControls` field: that struct is exhaustively
  constructible, so a field would break every literal already written for a
  capability that is otherwise purely additive.)
- **Whose signals apply.** `RunControls::signal_cancel` is off by default, so the
  engine never takes over your `SIGINT`/`SIGTERM` disposition; the CLI sets it.

Warnings (a history file that could not be opened, a mode that may block) go to
**your** stderr, exactly as they did from the CLI.

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

Released CLI and SDK consumers can use that same deterministic provider seam;
see [Testing patterns](docs/testing-patterns.md) for the stable `MOCK_*`
contract and CLI, Python, and Node examples.

## Live end-to-end testing

Live variant coverage selects Claude subscription identities with isolated
`CLAUDE_CONFIG_DIR` values and selects API identities for Claude, Codex,
OpenCode, Qwen, and Crush with child-only key injection. A local Goose probe
separately proved its API-key axis and provider/model banner. The suite also masks
ambient keys for subscription runs and live-proves OpenCode child isolation.
Every phase requires both the marker assertion and provider-specific identity
evidence.
Set `OH_E2E_EVIDENCE_FILE` to an external file path when a sanitized
command/assertion transcript is needed; successful runs keep stdout concise and
append the identity evidence to that file.

The extended-adapter CI matrix deliberately omits Qwen 0.21.0 on macOS: its
tool-enabled run exited successfully without the required exact marker (the
same version can treat a marker request as a file-write task), so that platform
cannot satisfy the live contract. It also omits the OpenCode
isolation phase on Windows because the probe's temporary Bash wrapper is not a
native Windows executable and oneharness correctly reports it unavailable.
Linux runs every extended phase; macOS still runs OpenCode and Crush, while
Windows still runs Qwen and Crush. These are matrix selections, not runtime
skips, so `OH_E2E_NO_SKIP=1` remains strict for every selected adapter.

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

`just live-variants` does not globally export keys. It prefers an explicitly
set phase variable, otherwise reads `KEY=VALUE` material from
`$OH_LIVE_AUTH_FILE` (default
`~/.config/oneharness/live-auth.env`, required mode 0600), and passes the value
only to the child phase that needs it. The justfile deliberately has no
`set dotenv-load`: gh-secrets' repo `.env` destination is not automatically
loaded. Likewise, `scripts/e2e-lib.sh`'s `need_env` reads only the current
process environment.

The Codex ChatGPT-subscription phase is local-only and guarded by
`OH_E2E_CODEX_SUBSCRIPTION=1`; interactive `codex login` cannot run in CI. CI
therefore excludes that phase by configuration rather than runtime-skipping
under `OH_E2E_NO_SKIP=1`.

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
