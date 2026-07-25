# Harness authentication identity

This reference records the identity selectors observed on 2026-07-25. Values
and account identifiers are intentionally omitted. “Per-process” means two
children can safely use different identities concurrently; login commands that
rewrite a shared credential file are not per-process.

## Live-proven adapters

### Claude Code 2.1.220

- `CLAUDE_CONFIG_DIR` selects the directory containing stored Claude login
  state. It is per-process; `claude auth login/logout` mutates that directory.
- `ANTHROPIC_API_KEY` selects API-key billing per-process and takes precedence
  over stored subscription login. `CLAUDE_CODE_OAUTH_TOKEN` similarly supplies
  OAuth auth. `apiKeyHelper` in `--settings` is another documented API-key
  source, but was not used because it executes a helper.
- `claude auth status --json` reports `authMethod`, provider, and subscription
  type before a run. Completed JSON output does not expose an account id. In the
  probes, subscription runs showed the first-party provider and one-hour cache
  creation; API-key auth showed five-minute cache creation and inference
  geography. Together with the independently selected config directory and
  preflight status, this is positive identity-axis evidence.
- Two stored Max subscriptions and one API key each completed
  `claude -p "Reply exactly <marker>" --output-format json`, returning the exact
  marker. Subscription commands set `CLAUDE_CONFIG_DIR` and removed
  `ANTHROPIC_API_KEY`/`CLAUDE_CODE_OAUTH_TOKEN`; the API command used an empty
  config directory and set only `ANTHROPIC_API_KEY`.
- An invalid key produces HTTP `401` authentication text, which the existing
  signal classifier recognizes as `auth`.

### Codex CLI 0.145.0

- `CODEX_HOME` selects `config.toml` and stored `auth.json`; it is per-process.
  `codex login` mutates the selected home, so switching a shared home by logging
  in is global mutable state.
- `CODEX_API_KEY` is the live-proven per-process API-key selector. Despite older
  ecosystem convention, `OPENAI_API_KEY` alone with an empty `CODEX_HOME` was
  not honored by this build: the run failed `401 Unauthorized: Missing bearer
  or basic authentication in header`. `CODEX_API_KEY` under the same empty home
  completed and echoed the marker.
- With `OPENAI_API_KEY` removed, the host `CODEX_HOME` completed through its
  ChatGPT subscription; `codex login status` reported `Logged in using ChatGPT`.
  With an empty home plus `CODEX_API_KEY`, the API-key run completed. JSONL
  completion records contain the marker and usage but no stable account id, so
  the selected isolated home/key plus login-status preflight is the available
  positive identity-axis evidence.
- A deliberately invalid `CODEX_API_KEY` produces `401 Unauthorized`; the
  existing classifier recognizes this as `auth`.
- `-c` and `--profile` select configuration/model behavior, not a distinct
  credential independently of `CODEX_HOME`.

## Other registry adapters

These CLIs were not installed on the probe host, so the mappings below remain
unverified and must not be represented as live-proven:

| harness | mapped selectors | state/evidence |
|---|---|---|
| OpenCode | provider API-key env vars; XDG/config home and stored auth | Unverified: CLI unavailable |
| Goose | provider API-key env vars plus `GOOSE_PROVIDER`/`GOOSE_MODEL`; config home | Unverified: CLI unavailable |
| Qwen Code | `OPENAI_API_KEY`, optional `OPENAI_BASE_URL`; Qwen config home | Unverified: CLI unavailable |
| Crush | provider API-key env vars; config home/stored auth | Unverified: CLI unavailable |
| Copilot CLI | `COPILOT_GITHUB_TOKEN`; stored GitHub login | Unverified: CLI unavailable |
| Cursor CLI | `CURSOR_API_KEY`; stored Cursor login | Unverified: CLI unavailable |

Environment selectors are expected to be per-process. Stored-login files are
global mutable state unless each child receives a distinct config/home
directory. None of the unavailable adapters has completed-run identity evidence
or a live-observed invalid-auth string yet.

## Design consequence

Variants must only select credentials through the child environment: set a
key, map a differently named parent variable to the canonical child variable,
load a mode-0600 external env file, remove ambient auth variables, or select an
already-prepared isolated config home. oneharness must never log in, rewrite a
credential store, copy a secret into project config, or mutate its own parent
environment.
