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
    claude_control_response, parse_claude_get_usage, parse_codex_rate_limits, parse_copilot_user,
    AuthMode, IdentitySelector, UnavailableReason, UsageAvailability, UsageIdentity, UsageReport,
    WindowUsage, SCHEMA_VERSION,
};
use serde_json::Value;

/// What `claude -p --input-format stream-json --output-format stream-json`
/// writes to stdout when a probe sends one `get_usage` control request: the
/// payload arrives on one line among several, so a consumer must find it.
const CLAUDE_STREAM: &str = r#"{"type":"system","subtype":"init","apiKeySource":"none","claude_code_version":"2.1.220"}
{"type":"control_response","response":{"subtype":"success","request_id":"req_1","response":{"session":{"total_cost_usd":0,"total_api_duration_ms":0,"total_duration_ms":607,"total_lines_added":0,"total_lines_removed":0,"model_usage":{}},"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":42,"resets_at":"2026-07-29T18:30:00.123456+00:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null},"seven_day":{"utilization":61,"resets_at":"2026-08-02T09:00:00.000000-04:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null},"seven_day_opus":null,"tangelo":null,"amber_ladder":null,"limits":[{"kind":"session","group":"session","percent":42,"severity":"normal","resets_at":"2026-07-29T18:30:00.123456+00:00","scope":null,"is_active":false},{"kind":"weekly_all","group":"weekly","percent":61,"severity":"normal","resets_at":"2026-08-02T13:00:00.000000+00:00","scope":null,"is_active":true}],"member_dashboard_available":true},"behaviors":null}}}"#;

/// What a probe reads back from `codex app-server --stdio`: the `initialize`
/// reply first, then the rate-limit reply, so a consumer must match on id.
const CODEX_EXCHANGE: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex-cli/0.145.0"}}
{"jsonrpc":"2.0","id":2,"result":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":31,"windowDurationMins":10080,"resetsAt":1785000000},"secondary":null,"credits":{"hasCredits":true,"unlimited":false,"balance":"12.34"},"individualLimit":null,"spendControlReached":false,"planType":"pro","rateLimitReachedType":null},"rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,"primary":{"usedPercent":31,"windowDurationMins":10080,"resetsAt":1785000000},"secondary":null,"credits":null,"individualLimit":null,"spendControlReached":null,"planType":"pro","rateLimitReachedType":null}},"rateLimitResetCredits":{"availableCount":0,"credits":[]}}}"#;

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
    assert_eq!(
        parsed.availability.windows().len(),
        1,
        "the null secondary and the single-bucket mirror must not add windows"
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
