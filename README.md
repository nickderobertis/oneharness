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
  "bypass_permissions": true,
  "dry_run": false,
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
      "stdout": "{\"type\":\"result\",\"result\":\"pong\"…}",
      "stderr": "",
      "error": null
    },
    { "harness": "codex", "available": false, "status": "skipped", "error": "`codex` not found on PATH; harness skipped. Install it: npm install -g @openai/codex", "…": "…" }
  ]
}
```

## Supported harnesses

| id | CLI | default binary | bypass mode requested |
|----|-----|----------------|-----------------------|
| `claude-code` | Claude Code | `claude` | `--permission-mode bypassPermissions` |
| `codex` | OpenAI Codex CLI | `codex` | `--sandbox danger-full-access -a never` |
| `opencode` | OpenCode | `opencode` | `--dangerously-skip-permissions` |
| `goose` | Goose | `goose` | (runs unattended) |
| `qwen` | Qwen Code | `qwen` | `--yolo` |
| `crush` | Crush | `crush` | `run -q` (non-interactive) |
| `copilot` | GitHub Copilot CLI | `copilot` | `--allow-all-tools --allow-all-paths --no-ask-user` |
| `cursor` | Cursor CLI | `cursor-agent` | `--force` |

`oneharness list` prints this registry as JSON, including the exact command each
adapter builds.

## Install

```console
cargo install --path .        # from a clone
# or build the release binary
just build-release            # -> target/release/oneharness
```

Requires a stable Rust toolchain and [`just`](https://github.com/casey/just).

## Usage

Three subcommands, all emitting JSON to **stdout**; diagnostics go to **stderr**.

```console
oneharness list                                   # describe the registry
oneharness detect --all                           # which harnesses are installed (+ versions)
oneharness run --all --prompt "…"                 # run everywhere, in parallel
oneharness run --harness claude-code,codex --prompt-file task.md
oneharness run --all --print-command --prompt "…" # dry run: show commands, run nothing
```

Useful `run` flags:

- `--all` / `--harness <id,…>` / `--exclude <id,…>` — selection.
- `--prompt <text>` or `--prompt-file <path|->` — the prompt (file or stdin).
- `--model <m>` — passed to each harness that supports a model flag.
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
- `--no-bypass` — do **not** request bypass mode (see the safety note below).
- `--require-available` — treat a not-installed harness as a failure.
- `--bin <id>=<path>` — override a harness binary (also via `ONEHARNESS_BIN_<ID>`).
- `--compact` — single-line JSON.

### Exit codes

- `0` — every selected harness was `ok` or `skipped` (or it was a dry run).
- `1` — at least one harness `nonzero`/`timeout`/`spawn-error`ed (or, under
  `--require-available`, was missing).
- `2` — usage/configuration error (bad args, unknown harness, no prompt).

### The result envelope vs. `text`

The execution envelope — `command`, `exit_code`, `duration_ms`, `status`,
`stdout`, `stderr` — is **guaranteed and identical** across harnesses. The
normalized `text` is a **best-effort** convenience whose extraction method is
recorded in `text_source` (e.g. `json:result`, `raw`, `stream-json:result`); it
is `null` when oneharness can't confidently extract a final message. Consumers
that need certainty should parse `stdout` themselves.

### Safety note: bypass by default

A headless agent run hangs waiting for a human to approve tool calls, so `run`
requests each harness's "don't prompt / allow everything" mode by default. That
is the right default for automation but means the agent can take real actions —
run it against throwaway sandboxes (see `--cwd`), or pass `--no-bypass` to leave
each harness's normal permission flow intact.

## Why it exists

[`nickderobertis/allowlister`](https://github.com/nickderobertis/allowlister)
verifies its policy engine against **every** real agent CLI. Each check had its
own bash `run_agent()` — Claude wants `-p … --permission-mode bypassPermissions
--output-format stream-json`, OpenCode wants `run --dangerously-skip-permissions
--format json`, Codex wants `exec --sandbox danger-full-access`, and so on — plus
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
just check       # full gate: fmt-check, clippy -D warnings, tests, build
just test        # tests only
just run -- list # run the CLI through cargo
```

Tests are hermetic: the subprocess path is exercised against a mock harness
fixture (no network, no real CLI), and every adapter's command construction is
pinned with `--print-command` assertions. See `AGENTS.md` and `tests/AGENTS.md`.

## License

MIT — see [LICENSE](LICENSE).
