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
global mutable state. Later variant live runs exercised two separately
provisioned subscription directories concurrently and confirmed both served
their selected runs while an ambient API key was present but explicitly masked
from each child.

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
source; `--settings` selects a settings file for one invocation. These token
levers were not exercised because no separate OAuth token was available. The
installed CLI's `--bare` help says it ignores OAuth/keychain and accepts only
`ANTHROPIC_API_KEY` or `apiKeyHelper` (plus third-party cloud credentials).

**Live proof and evidence.** The successful isolated-home probe used a
pre-provisioned directory (the copy step is omitted because it handles a secret):

```console
$ CLAUDE_CONFIG_DIR=<isolated-claude-home> claude auth status
{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "email": "[REDACTED]",
  "orgId": "[REDACTED]",
  "orgName": "[REDACTED]",
  "subscriptionType": "max"
}
$ printf 'Reply exactly AUTH_PROBE_OK' |
    CLAUDE_CONFIG_DIR=<isolated-claude-home> \
    claude -p --input-format text --output-format json --model haiku --tools ''
{"type":"result","is_error":false,"result":"AUTH_PROBE_OK","total_cost_usd":0.046732,"usage":{"input_tokens":10,"output_tokens":72}, ...}
```

The status output is **pre-run auth-mode/identity evidence** tied to that
directory. The completed result proves the same selected directory successfully
served a real turn, but it contains no account identifier. Its nonzero
`total_cost_usd` means cost is **not** a reliable API-key versus subscription
discriminator in this version.

The later first-class variant suite completed the exact-marker contract through
two stored Max subscriptions and one API-key identity. Each subscription run
combined its independently selected `CLAUDE_CONFIG_DIR`, `authMethod:
"claude.ai"` preflight, an exact marker response, and one-hour cache creation.
The API-key run used an empty config directory, an isolated
`ANTHROPIC_API_KEY`, an exact marker response, and five-minute cache creation.
These signals distinguish the identity axes without exposing an account
identifier. Both subscription runs reported `ambient_api_key=present` and
`child_api_key=masked`, live-proving that variant masking prevents ambient
API-key precedence from hijacking subscription auth.

The precedence probe explicitly retained the stored login while setting an
invalid per-process key:

```console
$ printf 'Reply exactly AUTH_PROBE_OK' |
    ANTHROPIC_API_KEY=<invalid-api-key> \
    claude -p --input-format text --output-format json --model haiku --tools ''
⚠ claude.ai connectors are disabled because ANTHROPIC_API_KEY or another auth source is set and takes precedence over your claude.ai login · Unset it to load your organization's connectors
{"type":"result","is_error":true,"api_error_status":401,"result":"Invalid API key · Fix external API key", ...}
```

This completed failed turn is concrete evidence that the env key, rather than
the stored subscription, was selected. The empty-home probe was:

```console
$ printf 'Reply exactly AUTH_PROBE_OK' |
    CLAUDE_CONFIG_DIR=<empty-claude-home> \
    claude -p --input-format text --output-format json --model haiku --tools ''
{"type":"result","is_error":true,"api_error_status":null,"result":"Not logged in · Please run /login", ...}
```

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
that environment variable automatically. A later live run established that
`CODEX_API_KEY` is the direct per-process API-key selector: with an empty
`CODEX_HOME`, it completed and echoed the marker while `OPENAI_API_KEY` alone
under the same condition failed with `401 Unauthorized: Missing bearer or basic
authentication in header`. oneharness therefore maps externally sourced OpenAI
API-key material to `CODEX_API_KEY` for only the selected child.

Piping a key to `CODEX_HOME=<isolated> codex login --with-api-key` also writes
the selected home and makes subsequent runs use it, but that mutation is not
needed for variants and must not happen during dispatch. `--profile` and `-c`
select configuration, but no tested config key selected a different credential
inside one home. The installed help only says `--profile` layers a named
configuration profile; its storage shape/path was not probed, so this document
makes no stronger claim. `CODEX_ACCESS_TOKEN` is not a direct run-time lever
either; the CLI exposes `login --with-access-token`. No second ChatGPT account
was available, so Codex's proven identity axis is ChatGPT subscription versus
API key, not subscription A versus B.

**Live proof and evidence.** The successful isolated-home probe used a
pre-provisioned directory (again omitting the secret-handling copy step):

```console
$ CODEX_HOME=<isolated-codex-home> codex login status
Logged in using ChatGPT
$ printf 'Reply exactly AUTH_PROBE_OK' |
    CODEX_HOME=<isolated-codex-home> \
    codex --ask-for-approval never exec --json --sandbox read-only -
{"type":"item.completed","item":{"type":"agent_message","text":"AUTH_PROBE_OK", ...}}
{"type":"turn.completed","usage":{"input_tokens":24011,"cached_input_tokens":13056,"cache_write_input_tokens":0,"output_tokens":8,"reasoning_output_tokens":0}}
```

`login status` is **pre-run auth-mode evidence**, not evidence embedded in the
turn. The completed event stream proves the selected home served the turn but
has no account, plan, cost, or billing-mode field; therefore it cannot identify
which ChatGPT account owned that home.

The guarded, local-only variant phase later repeated that proof against the
host login: `codex login status` reported `Logged in using ChatGPT`, API-key
variables were masked, and the run returned the exact marker. A separate empty
`CODEX_HOME` plus per-process `CODEX_API_KEY` run also returned the marker.
Together the selected home/key and preflight status are the available positive
subscription-versus-API identity evidence; completed JSONL alone cannot
distinguish those billing modes.

The environment-precedence probe retained the stored ChatGPT login and set an
invalid API-key variable:

```console
$ printf 'Reply exactly AUTH_PROBE_OK' |
    OPENAI_API_KEY=<invalid-api-key> \
    codex --ask-for-approval never exec --json --sandbox read-only -
{"type":"item.completed","item":{"type":"agent_message","text":"AUTH_PROBE_OK", ...}}
{"type":"turn.completed","usage":{...}}
```

Success is the observed evidence that 0.145.0 did not select that env value over
the stored ChatGPT credential.

An empty-home probe failed with:

```console
$ printf 'Reply exactly AUTH_PROBE_OK' |
    CODEX_HOME=<empty-codex-home> \
    codex --ask-for-approval never exec --json --sandbox read-only -
{"type":"error","message":"... 401 Unauthorized: Missing bearer or basic authentication in header ..."}
{"type":"turn.failed","error":{"message":"... 401 Unauthorized: Missing bearer or basic authentication in header ..."}}
```

The invalid stored-API-key probe both selected the isolated home during login
and then ran from it:

```console
$ printf '<invalid-api-key>' |
    CODEX_HOME=<invalid-key-codex-home> codex login --with-api-key
Reading API key from stdin...
Successfully logged in
$ printf 'Reply exactly AUTH_PROBE_OK' |
    CODEX_HOME=<invalid-key-codex-home> \
    codex --ask-for-approval never exec --json --sandbox read-only -
{"type":"error","message":"... 401 Unauthorized: Incorrect API key provided: [REDACTED] ..."}
{"type":"turn.failed","error":{"message":"... 401 Unauthorized: Incorrect API key provided: [REDACTED] ..."}}
```

Both classify as `auth`.

A deliberately invalid direct `CODEX_API_KEY` also produced `401 Unauthorized`
and is classified as `auth`, allowing fallback to the next variant without
mutating a credential file.

## `opencode`

**Live-proven:** a named variant injected only `ANTHROPIC_API_KEY`, selected
`anthropic/claude-haiku-4-5`, and completed the exact-marker contract. OpenCode's
JSONL ended in a billed `step_finish` (`cost > 0`), tying the selected Anthropic
provider/model and child-only key to a real request:

```text
ASSERT opencode:apikey: status=ok marker=exact harness_id=opencode:apikey
IDENTITY opencode:apikey: provider=anthropic model=claude-haiku-4-5 api_key=present ambient_openai=masked completed_step_cost>0
```

The probe deliberately placed `OPENAI_API_KEY` in oneharness's ambient
environment while the variant declared it in `unset_env`. A wrapper immediately
around the real OpenCode binary observed the selected Anthropic key present and
the ambient OpenAI key absent, then `exec`ed OpenCode. This live-proves both
child-only injection and masking without printing either value.

**Mapped, unproven:** `opencode auth login` stores credentials in
`~/.local/share/opencode/auth.json`. `OPENCODE_CONFIG_DIR` changes configuration,
but has not been shown to relocate that data-directory credential file; it is
not a stored-identity selector. Stored-auth precedence, invalid-auth text, and
classification remain unproven. No auth axis is known unsupported.

## `goose`

**Live-proven:** a named variant selected `GOOSE_PROVIDER=openai`,
`GOOSE_MODEL=gpt-4o-mini`, injected `OPENAI_API_KEY`, and used an isolated
`GOOSE_PATH_ROOT`. The real CLI completed the marker and identified the provider
and model in its session banner:

```text
ASSERT goose:apikey: status=ok marker=exact harness_id=goose:apikey
IDENTITY goose:apikey: session_banner='openai gpt-4o-mini' isolated_path_root=yes
```

These env selectors are per-process and safe for concurrent API-key variants.

**Mapped, unproven:** Goose also supports `GOOSE_PROVIDER__API_KEY` and other
provider-specific keys. `goose configure` stores credentials in the OS keyring
with file fallback. Although `GOOSE_PATH_ROOT` was used successfully with env
auth, stored credentials were not provisioned under two roots, so stored-login
isolation and precedence remain unproven. Exact invalid-auth text and
classification are also unproven. No auth axis is known unsupported.

## `qwen`

**Live-proven:** a named variant injected `OPENAI_API_KEY`, selected
`OPENAI_BASE_URL=https://api.openai.com/v1` and `gpt-4o-mini`, and completed the
exact marker from an isolated `HOME`:

```text
ASSERT qwen:apikey: status=ok marker=exact harness_id=qwen:apikey
IDENTITY qwen:apikey: provider=openai base_url=api.openai.com model=gpt-4o-mini isolated_home=yes
```

The isolated home mattered: the host's existing Qwen state changed prompt
behavior, while the fresh home honored the marker. API-key/base/model env and
the home selector are therefore live-proven per-process variant inputs.

**Mapped, unproven:** the documented `--openai-api-key` precedence over env,
discovered `.env`, and settings was not re-probed. Other provider `envKey`
values and interactive OAuth/Coding Plan state remain unproven. Shared project
`.env`, `~/.qwen` settings, and interactive login remain unsafe concurrent
switches. Exact invalid-auth text and classification are unproven. No auth axis
is known unsupported.

## `crush`

**Live-proven:** a named variant injected `ANTHROPIC_API_KEY`, selected Crush's
fully-qualified `anthropic/claude-haiku-4-5-20251001` model, used an isolated
`HOME`, and completed the exact marker:

```text
ASSERT crush:apikey: status=ok marker=exact harness_id=crush:apikey
IDENTITY crush:apikey: provider=anthropic model=claude-haiku-4-5-20251001 isolated_home=yes
```

The provider-qualified model is significant: the unqualified model name was
rejected by Crush 0.87.0. The env key and home are safe per-process selectors.

**Mapped, unproven:** Crush exposes stored platform login and `--data-dir`, but
neither stored-login isolation nor credential precedence was exercised. Other
provider keys, exact invalid-auth text, and classification remain unproven. No
auth axis is known unsupported.

## `copilot`

**Mapped, unproven:** GitHub's authentication reference documents this precedence:
`COPILOT_GITHUB_TOKEN` > `GH_TOKEN` > `GITHUB_TOKEN` > stored OAuth in the OS
keychain (or plaintext `$COPILOT_HOME/config.json`) > `gh auth token`.
`COPILOT_PROVIDER_API_KEY` plus provider base/model settings is BYOK and routes
model requests regardless of GitHub login.

Token/BYOK env and `COPILOT_HOME` are per-process; a shared OS-keychain login,
`copilot login/logout`, plaintext config, and `gh` login are mutable global
state.

The installed CLI found the host's `gh` login, but the attempted model call was
rejected with `You've reached your additional usage limit for your plan`.
Therefore the adapter is not live-proven: this host specifically lacks available
Copilot request quota, and has no `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, or
`GITHUB_TOKEN` environment credential. BYOK was not attempted because no
`COPILOT_PROVIDER_API_KEY` exists. No auth axis is known unsupported.

## `cursor`

**Mapped, unproven:** Cursor's authentication reference documents
`CURSOR_API_KEY`, `--api-key`, and browser `cursor-agent login`, which
stores credentials locally; `status` displays account and endpoint information.
The CLI flag is the most explicit selector, but its precedence over env/stored
login was not observed.

The flag and env are per-process. Browser login/logout and its shared credential
store are global mutable state; no documented home override was confirmed.
The installed CLI reported `Not logged in`; this host specifically lacks both a
`CURSOR_API_KEY` and stored browser credential, so no model call could be made.
Completed-run identity fields, exact invalid-auth output, and classification
remain unproven. No auth axis is known unsupported.

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
