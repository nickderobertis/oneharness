// llmlint: ignore-block[comments_earn_their_place] The two properties stated here — honest fleet coverage, and no probe failure taking the command down — are what the engine sweep below preserves; deferring them to `docs/harness-usage.md` is what `no_redundant_instruction_pointers` forbids.
//! `oneharness usage` — report each harness identity's remaining subscription
//! headroom, without spending a model turn.
//!
//! The command is the pre-flight counterpart to `run`: it answers "do I have
//! room to start this?" before a long orchestrated job rather than after one
//! fails on quota. Two properties shape everything here.
//!
//! **It covers the fleet, honestly.** All eight harnesses appear. Three report
//! real headroom, one reports a plan tier, and four say affirmatively that they
//! cannot — with which kind of cannot. A `usage` command that quietly covered
//! three of eight would undermine the premise that one command works across
//! every harness, and one that rendered an absent figure as `0%` would actively
//! mislead someone deciding whether to start a run.
//!
//! **Nothing here can take a harness down.** A missing binary, an
//! unauthenticated harness, a malformed payload, and a probe timeout are all
//! data in the report. Only a genuine usage error (an unknown harness id, an
//! undeclared variant, a bad config) exits non-zero.
// llmlint: ignore-end[comments_earn_their_place]
//!
//! Both properties belong to the sweep, which is a library call
//! ([`oneharness_core::io::usage::report`]) — so a Rust consumer checking
//! provider health gets the same [`UsageReport`] without spawning anything.
//! This module is the shell that renders it.

use std::time::Duration;

use crate::cli::{UsageArgs, UsageFormat};
use crate::commands::{print_json, print_text};
use oneharness_core::domain::usage::{
    AuthMode, QuotaCounters, UnavailableReason, UnknownReason, UsageAvailability, UsageIdentity,
    UsageReport, UsageWindow, WindowUsage,
};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::usage::{self as usage_io, UsageRequest};

pub fn run(args: &UsageArgs) -> Result<i32, OneharnessError> {
    let report = usage_io::report(&UsageRequest {
        all: args.all,
        harness: args.harness.clone(),
        exclude: args.exclude.clone(),
        bin: args.bin.clone(),
        cwd: args.cwd.clone(),
        timeout: Some(Duration::from_secs(args.timeout)),
        config: args.config.clone(),
        no_config: args.no_config,
    })?;

    match args.format {
        UsageFormat::Json => print_json(&report, args.compact)?,
        UsageFormat::Text => print_text(&render_text(&report))?,
    }
    Ok(0)
}

/// Render the report for a human. The rule the JSON contract encodes carries
/// over verbatim: an absent figure is *never* drawn as a percentage. An
/// unavailable identity prints why, an unknown one prints what went wrong, and
/// an unlimited quota prints "unlimited" rather than a full bar.
fn render_text(report: &UsageReport) -> String {
    let mut out = format!("usage as of {}\n", report.observed_at);
    for identity in &report.identities {
        out.push('\n');
        out.push_str(&render_identity(identity));
    }
    out
}

fn render_identity(identity: &UsageIdentity) -> String {
    let plan = identity
        .plan
        .as_deref()
        .map_or_else(String::new, |plan| format!(" · plan {plan}"));
    let variant = identity
        .variant
        .as_ref()
        .map_or_else(String::new, |variant| format!(":{variant}"));
    let mut out = format!(
        "{}{variant} [{}]{plan} · auth {}\n",
        identity.harness,
        identity.selector.key(),
        auth_label(identity.auth_mode)
    );
    match &identity.availability {
        UsageAvailability::Available { windows } => {
            for window in windows.as_slice() {
                out.push_str(&render_window(window));
            }
        }
        UsageAvailability::Unavailable { reason } => {
            out.push_str(&format!(
                "  no headroom to report: {}\n",
                unavailable_label(*reason)
            ));
        }
        UsageAvailability::Unknown { reason } => {
            out.push_str(&format!("  unknown: {}\n", unknown_label(reason)));
        }
    }
    out
}

fn render_window(window: &UsageWindow) -> String {
    let label = window
        .label
        .as_deref()
        .filter(|label| *label != window.id)
        .map_or_else(String::new, |label| format!(" ({label})"));
    let binding = match window.is_binding {
        Some(true) => " ← binding",
        _ => "",
    };
    let resets = window
        .resets_at
        .as_ref()
        .map_or_else(String::new, |at| format!(" · resets {at}"));
    let usage = match &window.usage {
        // Rounded for reading, never for deciding: the JSON carries the exact
        // figure, and an overage above 100% is shown as reported.
        WindowUsage::Metered {
            used_percent,
            counters,
        } => format!(
            "{:.0}% used{}",
            used_percent.get(),
            counters.as_ref().map_or_else(String::new, render_counters)
        ),
        WindowUsage::Unlimited => "unlimited".to_string(),
    };
    format!("  {}{label}: {usage}{resets}{binding}\n", window.id)
}

fn render_counters(counters: &QuotaCounters) -> String {
    let unit = match counters.unit {
        oneharness_core::domain::usage::QuotaUnit::AiCredits => " AI credits",
        oneharness_core::domain::usage::QuotaUnit::Unspecified => "",
    };
    let blocked = if counters.blocked() {
        " · exhausted and blocked"
    } else {
        ""
    };
    format!(
        " ({} of {}{unit} used, {} left{blocked})",
        counters.used.get(),
        counters.entitlement.get(),
        counters.remaining
    )
}

fn auth_label(mode: AuthMode) -> &'static str {
    match mode {
        AuthMode::Subscription => "subscription",
        AuthMode::ApiKey => "api key",
        AuthMode::Unknown => "unknown",
    }
}

/// Wording for an affirmative "no headroom". Deliberately scoped to what was
/// established — "this CLI exposes no plan headroom", not "no signal exists".
fn unavailable_label(reason: UnavailableReason) -> &'static str {
    match reason {
        UnavailableReason::ApiKeyAuth => {
            "API-key auth — this harness exposes no plan headroom in this mode"
        }
        UnavailableReason::NotLoggedIn => "no stored credential for this identity",
        UnavailableReason::NoWindowsReported => "the harness reported no rate-limit window",
        UnavailableReason::NoPlanQuota => "no first-party plan quota exists to report",
        UnavailableReason::NoHeadroomReader => {
            "a plan quota exists, but this CLI exposes no non-interactive reader for it"
        }
    }
}

fn unknown_label(reason: &UnknownReason) -> String {
    match reason {
        UnknownReason::Unprobed => "not probed".to_string(),
        UnknownReason::ProbeFailed { message } => message.clone(),
        UnknownReason::BinaryMissing { bin } => {
            format!("`{bin}` is not installed, so nothing could be probed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oneharness_core::domain::usage::UtcInstant;
    use oneharness_core::domain::usage::{
        IdentitySelector, QuotaAmount, UsedPercent, WindowDuration, Windows,
    };

    /// The single observation instant the reports below are stamped with.
    fn observed_at() -> UtcInstant {
        "2026-07-29T12:00:00Z"
            .parse()
            .expect("a canonical RFC 3339 UTC instant")
    }

    fn identity(harness: &str, availability: UsageAvailability) -> UsageIdentity {
        UsageIdentity {
            harness: harness.to_string(),
            variant: None,
            selector: IdentitySelector::Ambient,
            auth_mode: AuthMode::Unknown,
            plan: None,
            availability,
        }
    }

    fn metered(id: &str, used: f64) -> UsageWindow {
        UsageWindow {
            id: id.to_string(),
            label: None,
            usage: WindowUsage::Metered {
                used_percent: UsedPercent::new(used).expect("a valid percentage"),
                counters: None,
            },
            duration: WindowDuration::Unknown,
            resets_at: None,
            scope: None,
            is_binding: None,
        }
    }

    #[test]
    fn the_text_view_never_renders_an_absent_figure_as_a_percentage() {
        let report = UsageReport::new(
            observed_at(),
            vec![
                identity(
                    "opencode",
                    UsageAvailability::Unavailable {
                        reason: UnavailableReason::NoPlanQuota,
                    },
                ),
                identity(
                    "qwen",
                    UsageAvailability::Unavailable {
                        reason: UnavailableReason::NoHeadroomReader,
                    },
                ),
                identity(
                    "codex",
                    UsageAvailability::Unknown {
                        reason: UnknownReason::BinaryMissing {
                            bin: "codex".to_string(),
                        },
                    },
                ),
            ],
        );

        let text = render_text(&report);

        assert!(
            !text.contains('%'),
            "no identity here has a percentage:\n{text}"
        );
        assert!(text.contains("no first-party plan quota exists"), "{text}");
        assert!(
            text.contains("no non-interactive reader"),
            "a quota with no reader is a different sentence:\n{text}"
        );
        assert!(text.contains("`codex` is not installed"), "{text}");
    }

    #[test]
    fn the_text_view_marks_the_binding_window_and_an_unlimited_quota() {
        let mut binding = metered("seven_day", 61.4);
        binding.is_binding = Some(true);
        binding.resets_at = Some(
            "2026-08-02T13:00:00Z"
                .parse()
                .expect("a canonical RFC 3339 UTC instant"),
        );
        let unlimited = UsageWindow {
            usage: WindowUsage::Unlimited,
            ..metered("chat", 0.0)
        };
        let report = UsageReport::new(
            observed_at(),
            vec![identity(
                "claude-code",
                UsageAvailability::Available {
                    windows: Windows::new(vec![binding, unlimited]).expect("non-empty"),
                },
            )],
        );

        let text = render_text(&report);

        assert!(text.contains("seven_day: 61% used"), "{text}");
        assert!(text.contains("resets 2026-08-02T13:00:00Z"), "{text}");
        assert!(text.contains("← binding"), "{text}");
        assert!(
            text.contains("chat: unlimited"),
            "an unlimited quota has no counters to draw as a full bar:\n{text}"
        );
    }

    #[test]
    fn an_exhausted_metered_window_reads_as_exhausted_not_as_a_number_alone() {
        let mut window = metered("premium_interactions", 100.0);
        window.usage = WindowUsage::Metered {
            used_percent: UsedPercent::new(100.0).expect("valid"),
            counters: Some(QuotaCounters {
                entitlement: QuotaAmount::new(1500).expect("non-negative"),
                used: QuotaAmount::new(13518).expect("non-negative"),
                remaining: -12019,
                has_quota: false,
                overage_permitted: false,
                unit: oneharness_core::domain::usage::QuotaUnit::AiCredits,
            }),
        };
        let report = UsageReport::new(
            observed_at(),
            vec![identity(
                "copilot",
                UsageAvailability::Available {
                    windows: Windows::new(vec![window]).expect("non-empty"),
                },
            )],
        );

        let text = render_text(&report);

        assert!(text.contains("13518 of 1500 AI credits used"), "{text}");
        assert!(text.contains("exhausted and blocked"), "{text}");
    }

    /// A report read back from JSON never passed the probe parsers that flatten
    /// their own strings, and `oneharness-core` is published — so a consumer can
    /// be handed one written by anything. Every string it carries reaches this
    /// text view, which is what a human reads to decide whether they have
    /// headroom, so an escape sequence in printable identity details or a failure
    /// message must not survive deserialization to move the cursor or recolour
    /// the report. Attribution fields use their stricter semantic validators.
    #[test]
    fn escape_sequences_in_a_deserialized_report_never_reach_the_text_view() {
        let json = serde_json::json!({
            "schema_version": "0.1",
            "observed_at": "2026-07-29T12:00:00Z",
            "identities": [
                {
                    "harness": "claude-code",
                    "variant": "work",
                    "selector": {
                        "kind": "env_path",
                        "env": "CLAUDE_CONFIG_DIR",
                        "path": "/home/u/.claude\r/x",
                    },
                    "auth_mode": "subscription",
                    "plan": "ma\u{9b}31mx",
                    "availability": {
                        "state": "available",
                        "windows": [{
                            "id": "five\u{1b}[A_hour",
                            "label": "5\u{1b}[1m hours",
                            "usage": {"kind": "metered", "used_percent": 42.0},
                            "window_seconds_source": "inferred_from_id",
                            "window_seconds": 18000,
                            // The one string here that carries no escape,
                            // because it cannot: `resets_at` is a `UtcInstant`,
                            // so text with a control character in it is refused
                            // at the boundary rather than flattened after it.
                            "resets_at": "2026-07-29T18:30:00Z",
                            "scope": "Opus\u{1b}[0m 5",
                        }],
                    },
                },
                {
                    "harness": "codex",
                    "selector": {"kind": "env_secret", "env": "GH_TOKEN"},
                    "auth_mode": "unknown",
                    "availability": {
                        "state": "unknown",
                        "reason": {
                            "kind": "probe_failed",
                            "message": "the probe failed\u{1b}[1;31m and said so\u{8}",
                        },
                    },
                },
                {
                    "harness": "crush",
                    "selector": {"kind": "ambient"},
                    "auth_mode": "unknown",
                    "availability": {
                        "state": "unknown",
                        "reason": {"kind": "binary_missing", "bin": "cru\u{1b}[7msh"},
                    },
                },
            ],
        })
        .to_string();
        let report: UsageReport =
            serde_json::from_str(&json).expect("the report deserializes with the current types");

        let text = render_text(&report);

        // Newlines are the renderer's own; nothing else may be a control byte.
        let smuggled: Vec<char> = text
            .chars()
            .filter(|c| c.is_control() && *c != '\n')
            .collect();
        assert!(
            smuggled.is_empty(),
            "a deserialized report smuggled {smuggled:?} into the text view:\n{text:?}"
        );
        // The readable content survives: sanitizing flattens, never discards.
        for readable in [
            "claude-code",
            ":work",
            "CLAUDE_CONFIG_DIR=/home/u/.claude",
            "plan ma",
            "31mx",
            "five",
            "_hour",
            "5",
            " hours",
            "42% used",
            "resets 2026-07-29T18:30:00Z",
            "GH_TOKEN=<secret>",
            "the probe failed",
            "and said so",
            "`cru",
            "sh` is not installed",
        ] {
            assert!(
                text.contains(readable),
                "`{readable}` must survive sanitization:\n{text}"
            );
        }
    }
}
