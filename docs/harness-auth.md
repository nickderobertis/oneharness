# Harness authentication identity

This is the auth-routing reference for the adapters in
[`domain::harness::REGISTRY`](../crates/oneharness-core/src/domain/harness.rs).
It records observations made on 2026-07-25. “Observed”
means the real CLI completed a model call (or rejected a deliberately invalid
credential); “documented, unverified” means the CLI was not installed or no
credential/account was available. Values, account identifiers, request IDs, and
session IDs are omitted.

Whether observed output classifies as `auth` is evaluated against the canonical
recognizer in
[`signals.rs`](../crates/oneharness-core/src/domain/signals.rs#L295-L344);
this document deliberately does not duplicate its evolving literal list.
The classifications below apply that exact recognizer to the observed text.

“Safe” means selection itself is per-process. A login command that rewrites the
selected directory is still unsafe if two processes mutate that same directory.

## `claude-code`

**Observed levers and precedence.** The stored Claude.ai login lives in
`$CLAUDE_CONFIG_DIR/.credentials.json` (default `~/.claude/.credentials.json`);
`claude auth login/logout` mutates it. Setting `CLAUDE_CONFIG_DIR` to a directory
containing a copied credential produced `authMethod: "claude.ai"` and
`subscriptionType: "max"`, then completed a real call. Thus the directory is a
per-process identity selector, while login/logout against a shared directory is
global mutable state. A second subscription account was unavailable, so
cross-account routing was not exercised.

`ANTHROPIC_API_KEY` is per-process and beats the stored subscription login. With
both present, the CLI warned:

```text
ANTHROPIC_API_KEY or another auth source is set and takes precedence over your claude.ai login
```

The deliberately invalid key then produced JSON with `api_error_status: 401`
and `result: "Invalid API key · Fix external API key"`. oneharness classifies
that as `auth`. An empty `CLAUDE_CONFIG_DIR` produced
`"Not logged in · Please run /login"`; that also classifies as `auth`.

The current [Claude environment reference](https://code.claude.com/docs/en/env-vars)
also defines per-process `ANTHROPIC_AUTH_TOKEN` (Bearer header),
`CLAUDE_CODE_OAUTH_TOKEN` (ahead of keychain credentials), and
`CLAUDE_CODE_OAUTH_REFRESH_TOKEN` plus `CLAUDE_CODE_OAUTH_SCOPES`.
`settings.json` may set these under `env`, and `apiKeyHelper` is another settings
source; `--settings` selects a settings file for one invocation. These were not
exercised because no second token/account was available. The installed CLI's
`--bare` help says it ignores OAuth/keychain and accepts only
`ANTHROPIC_API_KEY` or `apiKeyHelper` (plus third-party cloud credentials).

**Completed-run evidence.** The stored-Max run:

```console
printf 'Reply exactly AUTH_PROBE_OK' |
  claude -p --input-format text --output-format json --model haiku --tools ''
```

returned `result: "AUTH_PROBE_OK"`, nonzero input/output usage, and
`total_cost_usd: 0.046732`. Cost is therefore **not** a reliable API-key versus
subscription discriminator in this version. `claude auth status` is the concrete
pre-run identity evidence (`authMethod`, `subscriptionType`, and account fields);
the completed result contains no account identifier. The invalid-key precedence
probe was the same command with `ANTHROPIC_API_KEY=<invalid>`.

## `codex`

**Observed levers and precedence.** Credentials live in
`$CODEX_HOME/auth.json` (default `~/.codex/auth.json`). `codex login`,
`login --with-api-key`, `login --with-access-token`, and `logout` mutate that
file. `CODEX_HOME` is per-process: a directory containing copied `auth.json` and
`config.toml` reported `Logged in using ChatGPT` and completed a real call.
Use a distinct, pre-provisioned home for each concurrent identity; never run
login/logout concurrently against one home.

In 0.145.0, an inline invalid `OPENAI_API_KEY` did **not** override the stored
ChatGPT login: the run still completed. An empty `CODEX_HOME` also did not consume
that environment variable automatically. Conversely, piping a key to
`CODEX_HOME=<isolated> codex login --with-api-key` wrote the selected home and
the subsequent run used it. Profiles (`--profile`, loading
`$CODEX_HOME/<name>.config.toml`) and `-c` select configuration/model/provider,
but no tested config key selected a different credential inside one home.
`CODEX_ACCESS_TOKEN` is not a direct run-time lever either; the CLI exposes
`login --with-access-token`. No second ChatGPT account or valid API key was
available, so successful cross-account precedence could not be exercised.

**Completed-run evidence.**

```console
printf 'Reply exactly AUTH_PROBE_OK' |
  codex --ask-for-approval never exec --json --sandbox read-only -
```

returned an `agent_message` containing `AUTH_PROBE_OK`, followed by
`turn.completed` usage (`input_tokens`, `cached_input_tokens`,
`output_tokens`, and `reasoning_output_tokens`). The event stream has no account,
plan, cost, or billing-mode field. `codex login status` (`Logged in using
ChatGPT`) is the concrete pre-run auth-mode evidence; there is no observed
completed-run identity proof beyond success under an isolated `CODEX_HOME`.

An empty home failed with:

```text
401 Unauthorized: Missing bearer or basic authentication in header
```

An isolated home provisioned with an invalid API key failed with:

```text
401 Unauthorized: Incorrect API key provided: [REDACTED]
```

Both classify as `auth`.

## `opencode`

**Unverified:** not installed; no provider account/key was available. Official
[CLI documentation](https://dev.opencode.ai/docs/cli/) says `opencode auth login`
stores credentials in `~/.local/share/opencode/auth.json`. Provider API-key
environment variables depend on the provider. The documented
`OPENCODE_CONFIG_DIR` changes the config directory, but documentation does not
say it relocates the data-directory `auth.json`; do not treat it as an identity
selector without a live probe. Config files (`opencode.json[c]`) select provider
and model, not necessarily credentials.

Precedence between env keys and stored credentials, concurrent isolation,
completed-run identity evidence, invalid-auth text, and oneharness
classification remain unknown.

## `goose`

**Unverified:** not installed; no provider account/key was available. The
official [environment reference](https://github.com/block/goose/blob/main/documentation/docs/guides/environment-variables.md)
documents per-process `GOOSE_PROVIDER`, `GOOSE_MODEL`,
`GOOSE_PROVIDER__API_KEY`, provider-specific API-key variables, and
`GOOSE_PATH_ROOT` (root for data/config/state). `goose configure` stores provider
configuration and credentials in the OS keyring, falling back to file storage.
Provider/model choose the backend identity namespace; the key chooses the
account.

Use per-process key env for variants. An isolated `GOOSE_PATH_ROOT` is the
documented candidate for stored identities; shared keyring/config mutation is
unsafe concurrently. Precedence, completed-run evidence, exact invalid-auth
text, and classification were not observed.

## `qwen`

**Unverified:** not installed; no Coding Plan or API-key account was available.
The official [auth reference](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/)
documents `--openai-api-key` as highest priority, then process env, the first
discovered `.env`, then `settings.json` `env`. Provider definitions select an
`envKey`; common keys include `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
`GEMINI_API_KEY`, and `BAILIAN_CODING_PLAN_API_KEY`. `OPENAI_BASE_URL`,
provider/model config, `security.auth.selectedType`, and `model.name` select the
service/model. OAuth/Coding Plan login is interactive and stored under Qwen's
user state.

CLI/env keys are per-process. Project `.env`, user `~/.qwen/.env`,
`~/.qwen/settings.json`, and interactive login are mutable shared state and
unsafe account switches during concurrent runs. No documented home override was
confirmed. Completed-run identity evidence, exact invalid-auth text, and
classification remain unknown.

## `crush`

**Unverified:** not installed; no provider account/key was available. The
registry installs `@charmland/crush`, but current public documentation did not
provide a sufficiently authoritative, version-matched list of auth variables,
credential paths, or precedence. Provider config is expected to choose provider,
model, and credential source, but that is an inference and must not become a
variant contract.

All five details—levers, precedence, concurrency safety, completed-run evidence,
and invalid-auth text/classification—require a live probe.

## `copilot`

**Unverified:** not installed; no GitHub/Copilot token was available. GitHub's
[authentication reference](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli)
documents this precedence:
`COPILOT_GITHUB_TOKEN` > `GH_TOKEN` > `GITHUB_TOKEN` > stored OAuth in the OS
keychain (or plaintext `$COPILOT_HOME/config.json`) > `gh auth token`.
`COPILOT_PROVIDER_API_KEY` plus provider base/model settings is BYOK and routes
model requests regardless of GitHub login.

Token/BYOK env and `COPILOT_HOME` are per-process; a shared OS-keychain login,
`copilot login/logout`, plaintext config, and `gh` login are mutable global
state. Exact completed-run account/plan evidence, invalid-auth output, and
oneharness classification were not observed.

## `cursor`

**Unverified:** not installed; no Cursor account/key was available. Cursor's
[authentication reference](https://docs.cursor.com/en/cli/reference/authentication)
documents `CURSOR_API_KEY`, `--api-key`, and browser `cursor-agent login`, which
stores credentials locally; `status` displays account and endpoint information.
The CLI flag is the most explicit selector, but its precedence over env/stored
login was not observed.

The flag and env are per-process. Browser login/logout and its shared credential
store are global mutable state; no documented home override was confirmed.
Completed-run identity fields, exact invalid-auth output, and oneharness
classification remain unknown.

## Recommendation for oneharness variants

The minimal variant execution surface is:

- an explicit environment map (API/OAuth tokens and provider-specific keys);
- a config/home-directory environment map (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  and verified equivalents);
- extra argv (notably Cursor `--api-key` and Qwen's key flag);
- `bin`, `model`, and `reasoning`, because provider wrappers and account
  entitlements can differ.

Before spawning, strip ambient auth variables not declared by the variant; an
ambient key can silently outrank the requested stored login (observed for
Claude, documented for Copilot and Qwen). Prefer ephemeral env/argv. For
subscription identities, provision one immutable credential directory per
variant and point each process at it.

Unsafe for two concurrent variants: any login/logout/configure command targeting
the same credential directory, Claude/Codex shared default homes, Copilot's
shared keychain or `gh` fallback, Goose's shared keyring, Qwen shared
settings/`.env`/OAuth state, Cursor's shared browser-login store, and OpenCode's
shared data-directory `auth.json`. `OPENCODE_CONFIG_DIR` must not be offered as
auth isolation until a live probe proves it also isolates the data directory.
