# Harness subscription headroom

This is the usage-reporting reference behind `oneharness usage`: what each
harness adapter can report about subscription headroom, and the observation each
answer rests on. Every `HarnessSpec.usage` tier and every probe is derived from
what is recorded here.

Observations were made on 2026-07-29. **“Observed”** means the command was
actually run and its (redacted) output is quoted; **“documented”** quotes an
official source; **“inferred”** is reasoning, always labelled. Values, account
identifiers, tokens, and percentages are omitted — field names, JSON structure,
units, and `null`-versus-non-null distinctions are preserved, because those are
what an implementer needs.

Versions observed: `claude` **2.1.220**, `codex-cli` **0.145.0**,
`cursor-agent` **2026.07.23-e383d2b**, `copilot` **1.0.75**, `opencode`
**1.18.5**, `goose` **1.44.0**, `qwen` **0.21.0**, `crush` **v0.87.0**.

Two properties govern everything below.

**Every probe is free.** No probe sends a user message or completes a model
turn, which is what makes `oneharness usage` usable as a pre-flight check rather
than a thing that costs what it measures. Claude's zero-turn property was
observed directly (`num_turns: 0`, `total_cost_usd: 0`, `model_usage: {}`);
codex's and Copilot's read account state without a session at all. A change to
any invocation here has to preserve that.

**An absent figure is never a zero.** `oneharness usage` distinguishes three
states — real headroom, an affirmative “no headroom to report” with a reason,
and “nothing was learned” — and none of the latter two can reach a percentage.
Rendering either as `0% used` is the one way this report can be actively
harmful to someone deciding whether they have room to start a run.

## Support tiers

| Tier | Harness | What `usage` reports | `HarnessSpec.usage` |
|---|---|---|---|
| **Headroom** | `claude-code` | Per-window percent used, reset, binding flag, plan | `Probed(ClaudeGetUsage)` |
| **Headroom** | `codex` | Per-bucket percent used, reported window length, reset, plan | `Probed(CodexAppServer)` |
| **Headroom** | `copilot` | Entitlement/used/remaining in AI credits, unlimited flag, monthly reset, plan | `Probed(CopilotUserEndpoint)` |
| **Plan tier only** | `cursor` | `subscriptionTier`, plus an affirmative “no non-interactive reader” | `Probed(CursorAbout)` |
| **No plan quota** | `opencode`, `goose` | An affirmative “no first-party plan quota exists” | `NoPlanQuota` |
| **No reader** | `qwen`, `crush` | An affirmative “a quota exists, but no non-interactive reader does” | `NoHeadroomReader` |

The last two rows are deliberately separate answers, not one “unsupported”
bucket. OpenCode Zen and Goose have no quantity to report — Zen is
pay-as-you-go and Goose ships no first-party plan — while Crush's Hyper credits
and Qwen's Coding Plan weekly quota are real quotas that no CLI surface exposes.
The first is settled; the second could change with an upstream release.

## The three supported probes

### `claude-code` — the `get_usage` control request

**Observed.** Spawn the CLI in stream-json input/output mode with an empty tool
set, write exactly one control request, read the matching control response, then
terminate. No user message is ever sent, which is what keeps it free.

```console
$ printf '{"type":"control_request","request_id":"oneharness-usage-1","request":{"subtype":"get_usage"}}' |
    CLAUDE_CONFIG_DIR=<isolated-claude-home> \
    claude -p --input-format stream-json --output-format stream-json --verbose --tools ''
{"type":"control_response","response":{"subtype":"success","request_id":"oneharness-usage-1","response":{
  "session":{"total_cost_usd":0,"total_api_duration_ms":0,"model_usage":{}},
  "subscription_type":"<PLAN:enum>",
  "rate_limits_available":true,
  "rate_limits":{
    "five_hour":{"utilization":<USED_PERCENT:int>,"resets_at":"<RESET_AT:iso8601>", ...},
    "seven_day":{"utilization":<USED_PERCENT:int>,"resets_at":"<RESET_AT:iso8601>", ...},
    "seven_day_opus":null,"tangelo":null,"amber_ladder":null, ...,
    "limits":[{"kind":"session","percent":<USED_PERCENT:int>,"is_active":<BOOL>,"scope":null},
              {"kind":"weekly_all", ...},
              {"kind":"weekly_scoped","scope":{"model":{"display_name":"<MODEL>"}}, ...}]}}}}
```

Field semantics, quoted from schema text embedded in the shipped binary:
`subscription_type` is the *“Claude.ai subscription type ('pro', 'max', 'team',
'enterprise') or null for API key / 3P provider sessions”*; `utilization` is
*“Percentage of the window used, 0-100”* — **percent-used, not remaining**.

Three parsing rules follow from the payload rather than from taste:

- **A null window is omitted, never zero-filled.** Eleven of thirteen window
  keys were `null`; `seven_day_opus: null` means “not applicable to this plan”,
  not “0% used”.
- **Window length comes from the key name and is sometimes underivable.**
  Several keys (`tangelo`, `iguana_necktie`, `nimbus_quill`, `cinder_cove`,
  `amber_ladder`) are codenames with no discoverable duration. The report keeps
  that asymmetry in the data (`window_seconds_source`) rather than guessing.
- **`extra_usage` is not a plan window.** It carries a `utilization` of its own
  but is a monthly credit axis, a different measurement.

**Cheaper auth-mode-only probe (observed):** `claude auth status` returns
`{"loggedIn":true,"authMethod":"api_key","apiProvider":"firstParty",
"apiKeySource":"ANTHROPIC_API_KEY"}` under an API key — with no `email`,
`orgId`, or `subscriptionType` key at all — versus `authMethod: "claude.ai"`
plus all three under a subscription. `oneharness usage` uses `get_usage`
instead, because it needs `rate_limits_available` as well.

**Contract status: explicitly experimental.** The SDK surface is literally named
`usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET()`, and no schema is
published to diff against. The guard is therefore assertion-based
(`claude_usage_drift`) and runs **inside** `parse_claude_get_usage`, so there is
no unguarded way to parse the payload. It degrades to **unknown** when the
payload omits `subscription_type` or carries it as neither a plan string nor
null, when it no longer carries `rate_limits_available`, or when it carries that
flag as `true` with no recognizable window surface (none of the expected
`limits[].kind` values `session`/`weekly_all`/`weekly_scoped`, and no window key
with a numeric `utilization`). Each of those is a branch the parser decides by a
field's *absence* — an absent discriminator means API-key auth, an absent flag
means no headroom — so a rename would otherwise publish “no headroom” as fact
for every user at once.

### `codex` — the app-server `account/rateLimits/read`

**Observed.** Spawn `codex app-server --stdio`; send `initialize`, the
`initialized` notification, then the read; match the reply by JSON-RPC id.

```console
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"oneharness", ...}}}
→ {"jsonrpc":"2.0","method":"initialized","params":{}}
→ {"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read","params":null}
← {"id":2,"result":{"rateLimits":{ …single-bucket mirror… },
     "rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,
       "primary":{"usedPercent":<USED_PERCENT:int>,
                  "windowDurationMins":<WINDOW_MINUTES:int>,
                  "resetsAt":<EPOCH_SECONDS>},
       "secondary":null,"planType":"<PLAN:enum>", ...}, …}}}
```

Calling-convention detail worth preserving: `account/rateLimits/read` takes
`params: null`, while its sibling `account/read` *requires* `params: {}` and
returns `-32600 "Invalid request: missing field \`params\`"` for `null`.

`rateLimitsByLimitId` is preferred over the top-level `rateLimits`, which the
generated schema documents as a *“Backward-compatible single-bucket view;
mirrors the historical payload”* — reading both would double-count. Buckets
flatten to `<limitId>/primary` and `<limitId>/secondary`; a `null` secondary
means one window on this plan, not a second at 0%.

Codex is the best-instrumented of the three: `windowDurationMins` means a window
length is never hardcoded, and `limitId`/`limitName` means new buckets arrive
self-labelled.

**Two authentication errors, deliberately not collapsed (both observed, and both
exist as distinct single-occurrence literals in the codex binary):**

| Condition | Error message | Reported as |
|---|---|---|
| Stored API-key login | `chatgpt authentication required to read rate limits` | `unavailable` / `api_key_auth` |
| No stored credential | `codex account authentication required to read rate limits` | `unavailable` / `not_logged_in` |

The gate is **auth mode**, not billing state or key validity: a valid, stored,
metered API key still gets the first error. Any *other* error message is a probe
failure (`unknown`), never an assumed absence of headroom.

**Contract status: experimental transport, but drift is detectable and exact.**
`codex app-server generate-json-schema` emits the contract from the installed
binary. `tests/fixtures/codex-rate-limits.schema.json` is that snapshot, diffed
by `scripts/check-codex-usage-schema.sh` inside `just check` (skipping cleanly
where codex is not installed), and asserted field-by-field against what the
parser reads by the hermetic Rust suite.

### `copilot` — `GET /copilot_internal/user`, out of band

**Observed** (read-only GET with a pre-existing token; identifiers redacted):

```console
$ gh api /copilot_internal/user
{ "copilot_plan": "<PLAN:string>", "access_type_sku": "monthly_subscriber_quota",
  "quota_reset_date": "<DATE:YYYY-MM-DD>", "quota_reset_date_utc": "<RESET_AT:rfc3339>",
  "token_based_billing": true,
  "quota_snapshots": {
    "chat":        { "unlimited": true, "percent_remaining": 100.0, "has_quota": true,
                     "entitlement": 0, "remaining": 0, "credits_used": 0, ... },
    "completions": { ...identical, unlimited: true... },
    "premium_interactions": {
      "unlimited": false, "percent_remaining": <PERCENT:float>, "has_quota": <BOOL>,
      "entitlement": <CREDITS:int>, "credits_used": <CREDITS:int>, "remaining": <CREDITS:int>,
      "overage_permitted": <BOOL>, "overage_entitlement": <CREDITS:int>, ... } } }
```

**Credential: a GitHub bearer token, and nothing else.** The CLI builds
``Bearer ${t}`` from the precedence GitHub documents — `COPILOT_GITHUB_TOKEN` >
`GH_TOKEN` > `GITHUB_TOKEN` > stored OAuth. oneharness reads the first three;
the stored OAuth login lives in the OS keyring, which it cannot read, so an
identity with none of the three is reported as **unknown** naming the variables
to set — not as an absence of headroom. Because the token is the entire
requirement, this probe answers **with no Copilot CLI installed at all**, and it
answers *before* a run rather than after a turn is spent.

Guardrails, both from the payload:

- **Gate on `unlimited` before reading any counter.** `chat` and `completions`
  report `unlimited: true` alongside `entitlement: 0` / `remaining: 0` /
  `percent_remaining: 100.0` — meaningless as counters, and a false full bar if
  read. Because the gate decides whether any counter means anything, an
  `unlimited` that is present but *not a boolean* is reported as drift rather
  than defaulting to `false`: defaulting would publish a snapshot that failed to
  parse as an affirmative metered reading.
- **A negative `entitlement` or `credits_used` drops the counter set.** Neither
  has a meaning below zero, so an unreadable one is treated like a missing one
  (the percentage still stands) rather than clamped into a plausible figure.
  `remaining` stays signed — an account past its ceiling reports a real deficit.
- **`percent_remaining` is the inverse polarity** of the normalized field and is
  converted to percent-used. `remaining` is taken as the server reports it
  rather than recomputed from `entitlement − credits_used`: the observed values
  disagree by about 1, and the server's figure wins.

`has_quota: false` together with `overage_permitted: false` is the
machine-readable “exhausted and blocked” state — the same condition
`harness-auth.md` records only as the rejection string *“You've reached your
additional usage limit for your plan”*.

**The run-embedded JSONL quota surface is deliberately not used.** Copilot's
shipped `session-events.schema.json` does define an `AssistantUsageEvent` with
`quotaSnapshots`, but oneharness invokes Copilot in text mode
(`OutputFormat::Text`, and `argv_copilot` never emits `--output-format`), so it
is unreachable as wired — and it would only yield data *after* a turn is spent.

**Contract status: undocumented internal.** `/copilot_internal/user` is an
`_internal` path whose own shipped client already ignores a field the server
sends (`credits_used` is absent from the CLI's response allowlist), and the
typed alternative (`account.getQuota`, reachable only via `--acp`/the bundled
SDK) is marked `stability: "experimental"`. Every non-200 other than 401, and
any 200 whose body is not a JSON object, degrades to **unknown** with the status
quoted back.

## `cursor` — plan tier only, and a credential hazard

**Observed.** `cursor-agent about --format json` is the whole non-interactive
surface:

```console
$ cursor-agent about --format json
{ "cliVersion": "<VERSION>", "model": "Auto", "subscriptionTier": "<TIER:display-name>",
  "osPlatform": "linux", "userEmail": "<REDACTED>", ... }
```

`subscriptionTier` is a **display name** (`Team`), title-cased — not a lowercase
enum like Claude's `max` or codex's `pro` — and, like every other plan, it is
kept verbatim rather than unified. It is populated only when both an access and
a refresh token are stored; a `null` tier is therefore reported as
`not_logged_in`. That answer rests on the field being contracted as a string or
null, so a tier of any other type is drift — reading it as logged-out would
state a fact about someone's account from a document that no longer says it.

**Dollar headroom exists and is unreachable.** `getCurrentPeriodUsage` returns
`billing_cycle_start`/`billing_cycle_end` plus plan and spend-limit usage in
integer cents, but its **only callsite is the interactive Ink TUI** — zero
non-interactive or run-output callsites, and no `/usage` slash command in the
non-TUI path. Hence the `NoHeadroomReader` tier rather than a partial reading.

### ⚠️ The Cursor credential-clobber hazard

**Observed, and worth recording regardless of this command — any future Cursor
dispatch hits it.** `cursor-agent --api-key <key>` is **not a per-process
selector; it is a login**. Running it (in a context expecting a read-only model
list) silently performed a token exchange and persisted credentials:

```console
$ stat -c '%y %n' ~/.config/cursor ~/.config/cursor/auth.json
2026-07-29 08:03:21  /home/nick/.config/cursor                # CREATED by the run
2026-07-29 08:03:22  /home/nick/.config/cursor/auth.json      # CREATED by the run
```

`~/.config/cursor` did not exist beforehand. The bundle shows why — the
API-key path calls `loginWithApiKey` and then `setAuthentication(accessToken,
refreshToken, apiKey)`, which writes to disk. On a host with a real Cursor login
that overwrites it.

Consequences, both implemented:

1. **The `usage` probe reads the tier only from a pre-existing login** and never
   passes `--api-key`. If no stored login exists it reports `not_logged_in`
   rather than resolving it by authenticating.
2. **The probe masks `CURSOR_API_KEY` from its child** (`CURSOR_LOGIN_ENVS`).
   `about` ignores the variable today, so this is a guard against a future
   release that starts honoring it, not a workaround for present behavior.

For any *other* Cursor invocation that must use an API key, the working
isolation is `XDG_CONFIG_HOME=<scratch>` (auth store) plus
`CURSOR_CONFIG_DIR=<scratch>` (preferences) — verified: auth landed in the
throwaway directory and `~/.config/cursor` stayed absent.

## The harnesses with nothing to report

Each of these was probed for its **command surface**; the negatives are stated
with the checks made.

### `opencode` — no plan quota (`NoPlanQuota`)

OpenCode Zen is pay-as-you-go: *“You are charged per request and you can add
credits to your account”* (documented), with auto-reload. **Nothing resets**, so
“remaining usage against a reset interval” is not a defined quantity. `opencode
--help` has no balance command, and `opencode stats` (observed) reports local
spend-to-date — `$0.36 spent` implies nothing about what remains.

*Negative finding, stated with what was checked:* the binary does contain
`getCredits()` hitting `${origin}/v1/credits`. On inspection that belongs to the
bundled Vercel AI Gateway provider SDK, and **no CLI subcommand reaches it**.

### `goose` — no plan quota (`NoPlanQuota`)

Goose is Block's open-source agent with **no first-party inference plan**; it
routes to whichever provider `GOOSE_PROVIDER` selects. `goose --help` (observed)
lists no usage, stats, or quota command, and the docs document none. Its
`RateLimitExceeded` strings are retry/backoff paths for failed requests.

*Relevant for design:* Goose can route through a **GitHub Copilot** subscription,
and that credential class is the same GitHub bearer token — so that headroom is
readable under the `copilot` entry rather than through Goose.

### `crush` — a quota with no reader (`NoHeadroomReader`)

Charm Hyper is *“subscription-based, with a free tier”* (README), and its
economics are documented: *“Everyone gets 100 hypercredits, refreshing
monthly”*, *“1 Hypercredit is currently 5¢”*. So a quota with a reset window
genuinely exists. Three independent negative checks:

1. Hyper's FAQ does not address a balance API or CLI at all.
2. `strings` over the stripped Go binary returns **zero** matches for `quota`,
   `entitlement`, `subscription`, `credits remaining`, or any `x-ratelimit-*`.
3. Its Copilot integration touches `/copilot_internal/v2/token` (the device-code
   exchange) and **not** `/copilot_internal/user`.

`crush stats` (observed) is local SQLite spend-to-date, not headroom.

### `qwen` — a quota with no reader (`NoHeadroomReader`)

The Alibaba Cloud Coding Plan is a genuine first-party subscription with a
documented **weekly quota** (*“Coding Plan (for individual developers · weekly
quota included)”*) whose size and anchor day are unpublished. Nothing is
readable — not even the active auth mode:

- **Observed:** the read-only status probe was *removed* — `qwen auth --help`
  answers `Configure authentication (removed)`. `qwen --help` lists no usage,
  stats, or quota command.
- **Bundle:** grepping `codingPlan|coding_plan` across all `*.js` yields only a
  provider binding (`CODING_PLAN_ENV_KEY`, base URLs) and **no quota accessor**.
- Every `quota`-matching string is exhaustion handling (`QUOTA_EXCEEDED_ERR`,
  `isQuotaExceeded(providerCode)`), not headroom.

*Do not build for Qwen's OAuth free tier* — discontinued 2026-04-15; the
`qwen-oauth` strings in the bundle are residue.

## Four distinct reset semantics

The reset field cannot be a boolean or a fixed period. Across the fleet:

| Harness | Window | Machine-readable? | How `usage` reports it |
|---|---|---|---|
| `claude-code` | Rolling 5-hour and 7-day, plus model-scoped weekly | Yes — `resets_at`, ISO 8601 with a numeric offset | Normalized to absolute RFC 3339 **UTC**; length inferred from the key name, `unknown` for a codename key |
| `codex` | Reported per bucket (`windowDurationMins`), observed weekly-length | Yes — `resetsAt`, **epoch seconds** | Converted to RFC 3339 UTC; length `reported`, never inferred |
| `copilot` | **Calendar month**, the 1st | Yes — `quota_reset_date_utc` (RFC 3339 UTC); `quota_reset_at` is present but unpopulated (`0`) — do not depend on it | Reset kept; length **`unknown`**, because a calendar month is not a fixed number of seconds |
| `cursor` | **Billing cycle**, not a calendar month | Only via undocumented internal RPC. `GetCurrentPeriodUsageResponse.billing_cycle_end` is epoch **milliseconds** (settled: the reducer feeds it to the same formatter as the self-documenting `endDateEpochMillis`); `billing_cycle_start` has zero callsites and stays undetermined | Not reported — no non-interactive reader |
| `qwen` | **Weekly** | No — size and anchor unpublished | Not reported |
| `crush` | Monthly refresh; purchased bundles **never expire** | No | Not reported |
| `opencode` | **None** — pure pay-as-you-go balance | No | Not reported |

## Per-identity attribution

**Observed for `claude-code`, across two identities.** The same zero-turn
`get_usage` request, run twice against two independently provisioned
`CLAUDE_CONFIG_DIR` copies, returned different data. Values withheld; the
comparison is the evidence: the weekly windows reset on **different calendar
days**, and the credits/spend sub-objects were populated on one identity and
entirely `null` on the other. Neither is explicable by caching or a shared
account.

**Inferred for `codex`**, from the selection mechanism rather than two accounts:
`account/rateLimits/read` against a `CODEX_HOME` containing a copied `auth.json`
returned live data, while the same request against an empty `CODEX_HOME`
returned the `codex account authentication required` error — so the read
resolves against the selected home and nothing ambient. Only one ChatGPT account
was available, so Codex's proven identity axis remains ChatGPT subscription
versus API key rather than subscription A versus B.

**How `usage` selects an identity.** It reuses `variant_environment`, the same
helper `run` uses, so an identity is selected by the machinery that already
points a run at a subscription:

```console
$ oneharness usage --harness claude-code:work,claude-code:personal
```

Each entry carries the variant name and the selector that chose it
(`CLAUDE_CONFIG_DIR=<path>` for Claude, `CODEX_HOME=<path>` for codex, the
*name* of the token variable for Copilot). A credential value never reaches the
report: `IdentitySelector::EnvSecret` records only the variable's name.

## API-key identities

Settled by observation for both harnesses that could carry plan headroom, so the
report states it rather than hedging:

- **claude-code, valid API key.** `get_usage` returns `subscription_type: null`,
  `rate_limits_available: false`, `rate_limits: null`, `behaviors: null` —
  byte-identical to the invalid-key probe, so the nulls are an **auth-mode
  property, not a key-validity artifact**. A complete paid-turn stream was
  scanned exhaustively (79 distinct keys, walked recursively): **no**
  `rate_limit_event`, and zero occurrences of any `rate_limit`/`quota` key.
- **codex, stored API-key login.** `account/rateLimits/read` fails with the
  ChatGPT-specific error above; no metered-key rate limits are exposed.

So an API-key identity is `unavailable` / `api_key_auth` — an affirmative
finding — **not** `unknown`. The wording stays scoped to what was established:
*“this harness exposes no plan headroom in this mode”*, not *“no signal
exists”*. Anthropic does document `anthropic-ratelimit-*` headers on Messages
API responses; the finding is that Claude Code does not surface them to a
caller, which is what constrains oneharness. Its ceiling in this mode is
billing, which lives in the vendors' Admin APIs — a different credential
answering a different question.

## Endpoints deliberately not called

Both harnesses' underlying HTTP endpoints were located and **not** called:
Claude Code's `/api/oauth/usage` (plus `/api/rate-limits`,
`/api/claude_cli_profile`, `/api/oauth/profile`,
`/api/claude_code/policy_limits`) and codex's `/wham/usage` /
`/api/codex/usage` family. They are undocumented internal paths; calling them
directly with real credentials would be reverse-engineering, and it would give
up the CLI's own token refresh, retry, and caching. Drive each harness through
its own CLI instead — which is what the probes above do.

Copilot's `/copilot_internal/user` is the one exception, and it is one on
purpose: oneharness invokes Copilot in text mode, so the CLI exposes no path to
this data at all, and the alternative would be spending a turn to learn whether
a turn was affordable.
