//! The usage domain driven from *outside* the crate, the way a consumer reaches
//! it: `oneharness-core` is published so sibling tools (and this repo's own
//! `usage` probe, landing next) can depend on the engine, so its public API is a
//! real boundary — not an implementation detail.
//!
//! These tests therefore start from what a probe actually captures — a whole
//! JSONL stream, a whole JSON-RPC exchange, an HTTP response body — rather than
//! a pre-unwrapped payload, and they finish at the JSON a downstream consumer
//! reads. Anything a consumer needs that is not `pub`, and any envelope step a
//! consumer would have to reinvent, fails here.

use oneharness_core::domain::usage::{
    claude_control_response, normalize_timestamp, parse_claude_get_usage, parse_codex_rate_limits,
    parse_copilot_user, parse_cursor_about, AuthMode, IdentitySelector, ParsedUsage,
    UnavailableReason, UnknownReason, UsageAvailability, UsageIdentity, UsageReport, UsedPercent,
    WindowUsage, SCHEMA_VERSION,
};
use serde_json::Value;

/// What `claude -p --input-format stream-json --output-format stream-json`
/// writes to stdout when a probe sends one `get_usage` control request: the
/// payload arrives on one line among several, so a consumer must find it.
const CLAUDE_STREAM: &str = r#"{"type":"system","subtype":"init","apiKeySource":"none","claude_code_version":"2.1.220"}
{"type":"control_response","response":{"subtype":"success","request_id":"req_1","response":{"session":{"total_cost_usd":0,"total_api_duration_ms":0,"total_duration_ms":607,"total_lines_added":0,"total_lines_removed":0,"model_usage":{}},"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":42,"resets_at":"2026-07-29T18:30:00.123456+00:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null},"seven_day":{"utilization":61,"resets_at":"2026-08-02T09:00:00.000000-04:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null},"seven_day_opus":null,"tangelo":null,"amber_ladder":null,"limits":[{"kind":"session","group":"session","percent":42,"severity":"normal","resets_at":"2026-07-29T18:30:00.123456+00:00","scope":null,"is_active":false},{"kind":"weekly_all","group":"weekly","percent":61,"severity":"normal","resets_at":"2026-08-02T13:00:00.000000+00:00","scope":null,"is_active":true},{"kind":"weekly_scoped","group":"weekly","percent":17,"severity":"normal","resets_at":"2026-08-02T13:00:00.000000+00:00","scope":{"model":{"id":null,"display_name":"Opus 5"},"surface":null},"is_active":false}],"member_dashboard_available":true},"behaviors":null}}}"#;

/// What a probe reads back from `codex app-server --stdio`: the `initialize`
/// reply first, then the rate-limit reply, so a consumer must match on id.
const CODEX_EXCHANGE: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex-cli/0.145.0"}}
{"jsonrpc":"2.0","id":2,"result":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":31,"windowDurationMins":10080,"resetsAt":1785000000},"secondary":null,"credits":{"hasCredits":true,"unlimited":false,"balance":"12.34"},"individualLimit":null,"spendControlReached":false,"planType":"pro","rateLimitReachedType":null},"rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,"primary":{"usedPercent":31,"windowDurationMins":10080,"resetsAt":1785000000},"secondary":null,"credits":null,"individualLimit":null,"spendControlReached":null,"planType":"pro","rateLimitReachedType":null},"limit_model_x":{"limitId":"limit_model_x","limitName":"GPT-5.3 Codex","primary":{"usedPercent":12,"windowDurationMins":10080,"resetsAt":1785000000},"secondary":null,"credits":null,"individualLimit":null,"spendControlReached":null,"planType":"pro","rateLimitReachedType":null}},"rateLimitResetCredits":{"availableCount":0,"credits":[]}}}"#;

/// The `GET /copilot_internal/user` response body.
const COPILOT_BODY: &str = r#"{"copilot_plan":"individual","access_type_sku":"monthly_subscriber_quota","quota_reset_date":"2026-08-01","quota_reset_date_utc":"2026-08-01T00:00:00.000Z","token_based_billing":true,"quota_snapshots":{"chat":{"unlimited":true,"percent_remaining":100.0,"has_quota":true,"entitlement":0,"remaining":0,"credits_used":0,"overage_permitted":false,"quota_reset_at":0},"premium_interactions":{"unlimited":false,"percent_remaining":0.0,"has_quota":false,"entitlement":1500,"credits_used":13518,"remaining":-12019,"overage_permitted":false,"quota_reset_at":0}}}"#;

fn lines(stream: &str) -> Vec<Value> {
    stream
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The envelope walk a consumer performs on Claude's stream.
fn claude_payload(stream: &str) -> Value {
    lines(stream)
        .iter()
        .find_map(|line| claude_control_response(line).cloned())
        .expect("the stream carries a successful get_usage response")
}

/// The envelope walk a consumer performs on codex's exchange.
fn codex_response(exchange: &str, id: u64) -> Value {
    lines(exchange)
        .into_iter()
        .find(|line| line.get("id").and_then(Value::as_u64) == Some(id))
        .expect("the exchange carries that response id")
}

fn used_percent(availability: &UsageAvailability, id: &str) -> f64 {
    let window = availability
        .windows()
        .iter()
        .find(|window| window.id == id)
        .unwrap_or_else(|| panic!("no window {id}"));
    match window.usage {
        WindowUsage::Metered { used_percent, .. } => used_percent.get(),
        WindowUsage::Unlimited => panic!("{id} is unlimited"),
    }
}

#[test]
fn a_consumer_reads_claude_headroom_from_a_captured_stream() {
    let parsed = parse_claude_get_usage(&claude_payload(CLAUDE_STREAM));

    assert_eq!(parsed.auth_mode, AuthMode::Subscription);
    assert_eq!(parsed.plan.as_deref(), Some("max"));
    assert_eq!(used_percent(&parsed.availability, "five_hour"), 42.0);
    assert_eq!(used_percent(&parsed.availability, "seven_day"), 61.0);

    let seven_day = parsed
        .availability
        .windows()
        .iter()
        .find(|window| window.id == "seven_day")
        .expect("the weekly window");
    assert_eq!(
        seven_day.resets_at.as_deref(),
        Some("2026-08-02T13:00:00Z"),
        "the -04:00 offset the harness sent must arrive as absolute UTC"
    );
    assert_eq!(seven_day.duration.seconds(), Some(604_800));
    assert_eq!(seven_day.is_binding, Some(true));

    assert!(
        !parsed.availability.windows().iter().any(|window| [
            "seven_day_opus",
            "tangelo",
            "amber_ladder"
        ]
        .contains(&window.id.as_str())),
        "null windows must not reach a consumer at all"
    );

    // The per-model weekly limit has no named `rate_limits` key of its own, so
    // the flat `limits[]` array is the only place a consumer can find it.
    let scoped = parsed
        .availability
        .windows()
        .iter()
        .find(|window| window.id == "weekly_scoped/Opus 5")
        .expect("the model-scoped weekly window");
    assert_eq!(
        used_percent(&parsed.availability, "weekly_scoped/Opus 5"),
        17.0
    );
    assert_eq!(scoped.scope.as_deref(), Some("Opus 5"));
    assert_eq!(scoped.duration.seconds(), Some(604_800));
}

#[test]
fn a_consumer_reads_codex_headroom_from_a_captured_app_server_exchange() {
    let parsed = parse_codex_rate_limits(&codex_response(CODEX_EXCHANGE, 2));

    assert_eq!(parsed.auth_mode, AuthMode::Subscription);
    assert_eq!(
        parsed.plan.as_deref(),
        Some("pro"),
        "codex's plan vocabulary reaches the consumer verbatim, unmerged with Claude's"
    );
    assert_eq!(used_percent(&parsed.availability, "codex/primary"), 31.0);

    let primary = &parsed.availability.windows()[0];
    assert_eq!(
        primary.resets_at.as_deref(),
        Some("2026-07-25T17:20:00Z"),
        "epoch seconds must arrive as absolute UTC, like every other harness"
    );
    assert_eq!(primary.duration.seconds(), Some(604_800));

    // The exchange carries the main bucket, a per-model bucket, and the
    // top-level single-bucket mirror of the first. A consumer sees two windows:
    // one per bucket, with the mirror deduplicated and the null secondaries
    // omitted rather than reported at 0% used.
    assert_eq!(
        parsed
            .availability
            .windows()
            .iter()
            .map(|window| window.id.as_str())
            .collect::<Vec<_>>(),
        vec!["codex/primary", "limit_model_x/primary"]
    );
    let per_model = &parsed.availability.windows()[1];
    assert_eq!(
        per_model.label.as_deref(),
        Some("GPT-5.3 Codex"),
        "codex labels its per-model bucket, and that name must reach the consumer"
    );
    assert_eq!(
        used_percent(&parsed.availability, "limit_model_x/primary"),
        12.0
    );
}

#[test]
fn a_consumer_reads_copilot_headroom_from_the_response_body() {
    let body: Value = serde_json::from_str(COPILOT_BODY).expect("a JSON body");
    let parsed = parse_copilot_user(&body);

    assert_eq!(parsed.plan.as_deref(), Some("individual"));
    assert_eq!(
        used_percent(&parsed.availability, "premium_interactions"),
        100.0
    );

    let chat = parsed
        .availability
        .windows()
        .iter()
        .find(|window| window.id == "chat")
        .expect("the chat quota");
    assert_eq!(
        chat.usage,
        WindowUsage::Unlimited,
        "an unlimited quota must reach a consumer with no counters to misread as full headroom"
    );

    let premium = parsed
        .availability
        .windows()
        .iter()
        .find(|window| window.id == "premium_interactions")
        .expect("the premium quota");
    let WindowUsage::Metered {
        counters: Some(counters),
        ..
    } = &premium.usage
    else {
        panic!("the metered quota carries counters");
    };
    assert!(
        counters.blocked(),
        "a consumer must be able to see exhausted-and-blocked without parsing prose"
    );
}

#[test]
fn an_api_key_identity_offers_a_consumer_no_percentage_to_render() {
    let codex = parse_codex_rate_limits(&serde_json::json!({
        "id": 4,
        "error": {"code": -32600, "message": "chatgpt authentication required to read rate limits"}
    }));
    assert_eq!(codex.auth_mode, AuthMode::ApiKey);
    assert_eq!(
        codex.availability,
        UsageAvailability::Unavailable {
            reason: UnavailableReason::ApiKeyAuth
        }
    );

    let claude = parse_claude_get_usage(&serde_json::json!({
        "subscription_type": null, "rate_limits_available": false,
        "rate_limits": null, "behaviors": null
    }));
    assert_eq!(claude.auth_mode, AuthMode::ApiKey);
    assert_eq!(
        claude.availability,
        UsageAvailability::Unavailable {
            reason: UnavailableReason::ApiKeyAuth
        }
    );

    for parsed in [&codex, &claude] {
        assert!(parsed.availability.windows().is_empty());
    }
}

/// The degradation paths, which matter more than the happy ones: a consumer
/// deciding whether to launch a long run must never read "0% used" out of a
/// probe that simply did not work.
#[test]
fn a_degraded_probe_reaches_a_consumer_as_unknown_or_unavailable_never_as_headroom() {
    // A wedged app-server: a reply carrying neither a result nor an error.
    let wedged = parse_codex_rate_limits(&serde_json::json!({"jsonrpc": "2.0", "id": 2}));
    assert!(matches!(
        wedged.availability,
        UsageAvailability::Unknown { .. }
    ));

    // A failure whose message this version does not recognize stays unknown
    // rather than becoming an assumed absence of headroom.
    let surprising = parse_codex_rate_limits(&serde_json::json!({
        "id": 2, "error": {"code": -32603, "message": "internal error"}
    }));
    assert!(matches!(
        surprising.availability,
        UsageAvailability::Unknown { .. }
    ));

    // No stored credential is a *different* answer from API-key auth, and both
    // are affirmative rather than unknown.
    let logged_out = parse_codex_rate_limits(&serde_json::json!({
        "id": 2,
        "error": {"code": -32600, "message": "codex account authentication required to read rate limits"}
    }));
    assert_eq!(
        logged_out.availability,
        UsageAvailability::Unavailable {
            reason: UnavailableReason::NotLoggedIn
        }
    );

    // A Copilot body with no quota block at all. The endpoint is undocumented
    // internal, so a vanished quota surface is drift — nothing was learned —
    // rather than an affirmative "this account has no quota window".
    let bodiless = parse_copilot_user(&serde_json::json!({"copilot_plan": "individual"}));
    assert!(matches!(
        bodiless.availability,
        UsageAvailability::Unknown {
            reason: UnknownReason::ProbeFailed { .. }
        }
    ));

    // A harness that reports a reset instant a consumer cannot trust: the
    // window still reports its usage, but with no reset rather than a guessed
    // one. (`2026-08-01T00:00:00` has no offset, so its instant is ambiguous.)
    assert_eq!(normalize_timestamp("2026-08-01T00:00:00"), None);
    let unparseable_reset = parse_copilot_user(&serde_json::json!({
        "copilot_plan": "individual",
        "quota_reset_date_utc": "2026-08-01T00:00:00",
        "token_based_billing": true,
        "quota_snapshots": {"premium_interactions": {
            "unlimited": false, "percent_remaining": 40.0, "has_quota": true,
            "entitlement": 1500, "credits_used": 900, "remaining": 600,
            "overage_permitted": true
        }}
    }));
    let window = &unparseable_reset.availability.windows()[0];
    assert_eq!(window.resets_at, None);
    assert!(matches!(window.usage, WindowUsage::Metered { .. }));

    // Not one of these degraded answers exposes a percentage a renderer could
    // draw as an empty bar.
    for parsed in [&wedged, &surprising, &logged_out, &bodiless] {
        assert!(
            parsed.availability.windows().is_empty(),
            "a degraded probe must offer no window at all"
        );
        let json = serde_json::to_string(&UsageIdentity::new(
            "codex",
            IdentitySelector::Ambient,
            parsed.clone(),
        ))
        .expect("the identity serializes");
        assert!(
            !json.contains("used_percent"),
            "no percentage may reach the wire for a degraded probe: {json}"
        );
    }
}

#[test]
fn the_report_json_is_what_a_downstream_consumer_reads() {
    let report = UsageReport::new(
        "2026-07-29T12:00:00Z".to_string(),
        vec![
            UsageIdentity::new(
                "claude-code",
                IdentitySelector::EnvPath {
                    env: "CLAUDE_CONFIG_DIR".to_string(),
                    path: "/home/u/.claude".to_string(),
                },
                parse_claude_get_usage(&claude_payload(CLAUDE_STREAM)),
            ),
            UsageIdentity::new(
                "codex",
                IdentitySelector::EnvPath {
                    env: "CODEX_HOME".to_string(),
                    path: "/home/u/.codex".to_string(),
                },
                parse_codex_rate_limits(&codex_response(CODEX_EXCHANGE, 2)),
            ),
            UsageIdentity::new(
                "copilot",
                IdentitySelector::EnvSecret {
                    env: "GH_TOKEN".to_string(),
                },
                parse_copilot_user(&serde_json::from_str(COPILOT_BODY).expect("a JSON body")),
            ),
        ],
    );

    // Read it back as a consumer in any language would: through the JSON.
    let json: Value =
        serde_json::from_str(&serde_json::to_string(&report).expect("the report serializes"))
            .expect("valid JSON");

    assert_eq!(json["schema_version"], SCHEMA_VERSION);
    assert_eq!(json["observed_at"], "2026-07-29T12:00:00Z");

    let claude = &json["identities"][0];
    assert_eq!(claude["harness"], "claude-code");
    assert_eq!(claude["selector"]["kind"], "env_path");
    assert_eq!(claude["plan"], "max");
    assert_eq!(claude["availability"]["state"], "available");
    let five_hour = &claude["availability"]["windows"][0];
    assert_eq!(five_hour["id"], "five_hour");
    assert_eq!(five_hour["usage"]["kind"], "metered");
    assert_eq!(five_hour["usage"]["used_percent"], 42.0);
    assert_eq!(five_hour["window_seconds_source"], "inferred_from_id");
    assert_eq!(five_hour["window_seconds"], 18_000);

    let codex_primary = &json["identities"][1]["availability"]["windows"][0];
    assert_eq!(
        codex_primary["window_seconds_source"], "reported",
        "a consumer must be able to tell a stated window length from a derived one"
    );

    let copilot = &json["identities"][2];
    assert_eq!(copilot["plan"], "individual");
    assert_eq!(
        copilot["selector"].get("path"),
        None,
        "a secret-valued selector must never carry its credential into the report"
    );
    let chat = &copilot["availability"]["windows"][0];
    assert_eq!(chat["id"], "chat");
    assert_eq!(chat["usage"]["kind"], "unlimited");
    assert_eq!(
        chat["usage"].get("used_percent"),
        None,
        "an unlimited quota has no percentage a consumer could draw as a full bar"
    );
    assert_eq!(
        chat["window_seconds_source"], "unknown",
        "a calendar month reports no fixed length rather than a default one"
    );

    let round_tripped: UsageReport =
        serde_json::from_value(json).expect("the contract deserializes back");
    assert_eq!(round_tripped, report);
}

/// The bytes a consumer of the v0.1 contract sees.
const GOLDEN: &str = include_str!("../../../tests/fixtures/usage-report-v01.json");

/// Every shape the v0.1 contract can take, in one report: a plan window whose
/// length is derived from its key, one whose length the harness stated, one
/// whose length cannot be established at all, a model-scoped window, an
/// unlimited quota, a metered quota with counters, an affirmative unavailable,
/// and an unknown. If a shape is not here, the golden does not pin it.
fn golden_report() -> UsageReport {
    let mut claude = claude_payload(CLAUDE_STREAM);
    // A codename key carrying real data. Every one observed so far was null,
    // but the key set is open by contract, and this is the forward-compatible
    // shape a consumer must handle: an opaque window with no derivable
    // duration, no label, no reset, no scope, and no binding flag — i.e. the
    // window that exercises every omission at once.
    claude["rate_limits"]["nimbus_quill"] = serde_json::json!({
        "utilization": 4,
        "resets_at": null,
        "limit_dollars": null, "used_dollars": null, "remaining_dollars": null
    });

    UsageReport::new(
        "2026-07-29T12:00:00Z".to_string(),
        vec![
            UsageIdentity::new(
                "claude-code",
                IdentitySelector::EnvPath {
                    env: "CLAUDE_CONFIG_DIR".to_string(),
                    path: "/home/u/.claude".to_string(),
                },
                parse_claude_get_usage(&claude),
            ),
            UsageIdentity::new(
                "codex",
                IdentitySelector::EnvPath {
                    env: "CODEX_HOME".to_string(),
                    path: "/home/u/.codex".to_string(),
                },
                parse_codex_rate_limits(&codex_response(CODEX_EXCHANGE, 2)),
            ),
            UsageIdentity::new(
                "copilot",
                IdentitySelector::EnvSecret {
                    env: "GH_TOKEN".to_string(),
                },
                parse_copilot_user(&serde_json::from_str(COPILOT_BODY).expect("a JSON body")),
            ),
            UsageIdentity::new(
                "codex",
                IdentitySelector::EnvPath {
                    env: "CODEX_HOME".to_string(),
                    path: "/home/u/.codex-apikey".to_string(),
                },
                parse_codex_rate_limits(&serde_json::json!({
                    "id": 4,
                    "error": {
                        "code": -32600,
                        "message": "chatgpt authentication required to read rate limits"
                    }
                })),
            ),
            // A second subscription of the same harness, selected by a named
            // variant: the entry a consumer must be able to tell apart from the
            // first even though both are `claude-code`.
            UsageIdentity::new(
                "claude-code",
                IdentitySelector::EnvPath {
                    env: "CLAUDE_CONFIG_DIR".to_string(),
                    path: "/home/u/.claude-work".to_string(),
                },
                ParsedUsage::unknown(UnknownReason::ProbeFailed {
                    message: "claude-code's `get_usage` control request did not answer within 60s"
                        .to_string(),
                }),
            )
            .with_variant(Some("work".to_string())),
            // Plan tier only: a real plan name with an affirmative "no reader".
            UsageIdentity::new(
                "cursor",
                IdentitySelector::Ambient,
                parse_cursor_about(&serde_json::json!({"subscriptionTier": "Team"})),
            ),
            // The two affirmative "cannot report headroom" answers, which a
            // consumer must be able to tell apart from each other and from
            // `unknown`.
            UsageIdentity::new(
                "opencode",
                IdentitySelector::Ambient,
                unavailable(UnavailableReason::NoPlanQuota),
            ),
            UsageIdentity::new(
                "qwen",
                IdentitySelector::Ambient,
                unavailable(UnavailableReason::NoHeadroomReader),
            ),
            // A harness that is simply not installed here.
            UsageIdentity::new(
                "crush",
                IdentitySelector::Ambient,
                ParsedUsage::unknown(UnknownReason::BinaryMissing {
                    bin: "crush".to_string(),
                }),
            ),
            UsageIdentity::new(
                "goose",
                IdentitySelector::Ambient,
                ParsedUsage::unknown(UnknownReason::Unprobed),
            ),
        ],
    )
}

/// An affirmative "no headroom to report", as the command layer builds it.
fn unavailable(reason: UnavailableReason) -> ParsedUsage {
    ParsedUsage {
        auth_mode: AuthMode::Unknown,
        plan: None,
        availability: UsageAvailability::Unavailable { reason },
    }
}

#[test]
fn the_v0_1_report_serializes_to_the_checked_in_golden() {
    let actual = serde_json::to_string(&golden_report()).expect("the report serializes");

    assert_eq!(
        actual.as_bytes(),
        GOLDEN.trim_end().as_bytes(),
        "the v0.1 usage contract changed; if that is deliberate, bump SCHEMA_VERSION \
         and update tests/fixtures/usage-report-v01.json in the same change"
    );
}

#[test]
fn the_v0_1_golden_deserializes_and_round_trips_through_the_public_api() {
    let parsed: UsageReport =
        serde_json::from_str(GOLDEN).expect("the golden deserializes with the current types");

    assert_eq!(parsed, golden_report());
    assert_eq!(
        serde_json::to_string(&parsed).expect("re-serializes"),
        GOLDEN.trim_end(),
        "reading the contract and writing it back must be byte-stable"
    );

    // An absent optional deserializes as absent, not as some default value.
    let opaque = parsed.identities[0]
        .availability
        .windows()
        .iter()
        .find(|window| window.id == "nimbus_quill")
        .expect("the opaque codename window");
    assert_eq!(opaque.label, None);
    assert_eq!(opaque.resets_at, None);
    assert_eq!(opaque.scope, None);
    assert_eq!(opaque.is_binding, None);
    assert_eq!(opaque.duration.seconds(), None);
    assert_eq!(
        opaque.usage,
        WindowUsage::Metered {
            used_percent: UsedPercent::new(4.0).expect("a valid percentage"),
            counters: None
        }
    );
    assert_eq!(parsed.identities[3].plan, None);
}

/// The omissions are the point: a consumer distinguishes "the harness did not
/// report this" from a value, and a JSON `null` is neither.
#[test]
fn absent_optionals_are_omitted_from_the_wire_rather_than_written_as_null() {
    let json: Value = serde_json::from_str(GOLDEN).expect("valid JSON");

    assert!(
        !GOLDEN.contains(":null"),
        "no field may reach the wire as null: {GOLDEN}"
    );

    let opaque = json["identities"][0]["availability"]["windows"]
        .as_array()
        .expect("windows")
        .iter()
        .find(|window| window["id"] == "nimbus_quill")
        .expect("the opaque codename window");
    for absent in [
        "label",
        "resets_at",
        "scope",
        "is_binding",
        "window_seconds",
    ] {
        assert_eq!(
            opaque.get(absent),
            None,
            "{absent} must be absent, not null, on a window that has none"
        );
    }
    assert_eq!(
        opaque["window_seconds_source"], "unknown",
        "the source discriminator stays, so a consumer can tell \
         'no length reported' from 'length not yet read'"
    );
    assert_eq!(
        opaque["usage"].get("counters"),
        None,
        "a window with no counter set omits it rather than writing null"
    );

    // The shapes that *do* carry these fields still carry them.
    let scoped = json["identities"][0]["availability"]["windows"]
        .as_array()
        .expect("windows")
        .iter()
        .find(|window| window["id"] == "weekly_scoped/Opus 5")
        .expect("the model-scoped window");
    assert_eq!(scoped["scope"], "Opus 5");
    assert_eq!(scoped["is_binding"], false);
    assert_eq!(scoped["label"], "Opus 5");

    assert_eq!(
        json["identities"][3].get("plan"),
        None,
        "an API-key identity has no plan, and says so by omission"
    );
    assert_eq!(json["identities"][0]["plan"], "max");
}

/// Rewrites `tests/fixtures/usage-report-v01.json` from [`golden_report`], then
/// asserts the file it left behind is one a consumer can actually read back.
/// Ignored by default — run with `--ignored` after a deliberate contract change,
/// then read the diff before committing it. Regenerating without checking the
/// result would let a broken serialization author its own passing golden.
#[test]
#[ignore = "regenerates the checked-in golden; run deliberately"]
fn regenerating_the_golden_leaves_a_readable_contract() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/usage-report-v01.json"
    );
    let json = serde_json::to_string(&golden_report()).expect("the report serializes");
    std::fs::write(path, format!("{json}\n")).expect("the fixture is writable");

    let written = std::fs::read_to_string(path).expect("the fixture reads back");
    let parsed: UsageReport =
        serde_json::from_str(&written).expect("the regenerated golden deserializes");
    assert_eq!(parsed, golden_report(), "the golden must round-trip");
    assert_eq!(
        parsed.schema_version, SCHEMA_VERSION,
        "a regenerated golden that no longer matches SCHEMA_VERSION means a bump was missed"
    );
}

/// codex's own generated contract for `account/rateLimits/read`, snapshotted
/// from the installed CLI. `scripts/check-codex-usage-schema.sh` diffs this
/// against a fresh generation when codex is installed; the test below is the
/// hermetic half, and it is the one that fails when a *field the parser reads*
/// disappears rather than merely when the file changes.
const CODEX_SCHEMA: &str = include_str!("../../../tests/fixtures/codex-rate-limits.schema.json");

/// Does `schema` define `property` on `definition` (or at the top level when
/// `definition` is `None`)?
fn declares(schema: &Value, definition: Option<&str>, property: &str) -> bool {
    let object = match definition {
        Some(name) => &schema["definitions"][name],
        None => schema,
    };
    object["properties"].get(property).is_some()
}

#[test]
fn codex_schema_snapshot_still_declares_every_field_the_parser_reads() {
    // The app-server is experimental, so this is the drift alarm for the exact
    // set of names `parse_codex_rate_limits` walks. A rename upstream would
    // otherwise turn a real reading into "no windows reported" — a confident
    // answer built from a shape that no longer exists.
    let schema: Value = serde_json::from_str(CODEX_SCHEMA).expect("the snapshot is JSON");

    for property in ["rateLimits", "rateLimitsByLimitId"] {
        assert!(
            declares(&schema, None, property),
            "the response no longer declares `{property}`"
        );
    }
    for property in ["limitId", "limitName", "primary", "secondary", "planType"] {
        assert!(
            declares(&schema, Some("RateLimitSnapshot"), property),
            "RateLimitSnapshot no longer declares `{property}`"
        );
    }
    for property in ["usedPercent", "windowDurationMins", "resetsAt"] {
        assert!(
            declares(&schema, Some("RateLimitWindow"), property),
            "RateLimitWindow no longer declares `{property}`"
        );
    }

    // `usedPercent` is the one required window field, which is why a window
    // without a usable percentage is dropped rather than defaulted to zero.
    assert_eq!(
        schema["definitions"]["RateLimitWindow"]["required"],
        serde_json::json!(["usedPercent"])
    );
    // The plan vocabulary is kept verbatim rather than mapped onto Claude's, so
    // the enum only has to remain a string — but it must remain present.
    assert!(
        schema["definitions"]["PlanType"]["enum"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == "pro")),
        "PlanType is no longer the enum the `plan` field is read from"
    );
}

#[test]
fn a_payload_shaped_like_the_snapshot_parses_into_headroom() {
    // Ties the two halves together: the fields asserted above are the ones an
    // actual reading flows through, so the snapshot cannot drift into being a
    // check on names nothing consumes.
    let parsed = parse_codex_rate_limits(&codex_response(CODEX_EXCHANGE, 2));

    assert!(matches!(
        parsed.availability,
        UsageAvailability::Available { .. }
    ));
    assert_eq!(parsed.plan.as_deref(), Some("pro"));
    assert_eq!(used_percent(&parsed.availability, "codex/primary"), 31.0);
}
