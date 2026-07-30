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

use std::path::PathBuf;
use std::time::Duration;

use crate::cli::{UsageArgs, UsageFormat};
use crate::commands::{
    dedupe_exact_ids, print_json, print_text, select_specs, variant_environment,
};
use oneharness_core::domain::usage::{
    AuthMode, QuotaCounters, UnavailableReason, UnknownReason, UsageAvailability, UsageIdentity,
    UsageReport, UsageWindow, WindowUsage,
};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::detect::{self, BinOverrides};
use oneharness_core::io::usage::{self as usage_io, EnvView, UsageProbeRequest};

/// Probes run concurrently: identity selection is per-process for every harness
/// here, so nothing is shared between them. Bounded so a `--all` sweep on a
/// small machine does not start eight subprocesses at once.
const MAX_PARALLEL_PROBES: usize = 4;

pub fn run(args: &UsageArgs) -> Result<i32, OneharnessError> {
    // Probing defaults to every harness; only naming one with `--harness`
    // narrows the sweep. `--exclude` alone therefore means "everything but
    // these" rather than an empty selection.
    let all = args.all || args.harness.is_empty();
    let specs = select_specs(all, &args.harness, &args.exclude)?;
    let selected_ids = dedupe_exact_ids(&args.harness);

    let project_start = args.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_start)?;
    // A variant that was never declared is a usage error, not an identity
    // silently collapsed onto the base harness's credentials.
    for id in args.harness.iter().chain(&args.exclude) {
        if let Some((base, variant)) = id.split_once(':') {
            if loaded.config.variant_for(id).is_none() {
                return Err(OneharnessError::UnknownHarnessVariant {
                    id: id.clone(),
                    base: base.to_string(),
                    variant: variant.to_string(),
                });
            }
        }
    }
    let overrides = BinOverrides::parse(&args.bin)?.with_config_bins(config_bins(&loaded.config));
    let timeout = Duration::from_secs(args.timeout);

    // One plan per selected identity, built before any probe runs so a config
    // fault fails loudly up front rather than half way through a sweep.
    let mut plans = Vec::with_capacity(specs.len());
    for (index, spec) in specs.iter().enumerate() {
        let id = selected_ids
            .get(index)
            .map_or_else(|| spec.id.to_string(), Clone::clone);
        let env = variant_environment(&loaded.config, &id, &project_start)?;
        let env_remove = loaded
            .config
            .variant_for(&id)
            .map_or_else(Vec::new, |variant| variant.unset_env.clone());
        let resolved = detect::resolve_named(spec, &id, &overrides);
        plans.push(Plan {
            id,
            base: spec.id,
            support: spec.usage,
            bin: resolved.bin,
            available: resolved.available,
            env,
            env_remove,
            cwd: args.cwd.clone(),
            timeout,
        });
    }

    let identities = probe_all(&plans);
    let report = UsageReport::new(now_rfc3339(), identities);

    match args.format {
        UsageFormat::Json => print_json(&report, args.compact)?,
        UsageFormat::Text => print_text(&render_text(&report))?,
    }
    Ok(0)
}

/// Everything one identity's probe needs, resolved from config and the CLI.
struct Plan {
    /// The composed selector (`claude-code:work`), which is how a consumer joins
    /// this entry back to the `run` invocation it describes.
    id: String,
    /// The registry id, which is what the report's `harness` field carries.
    base: &'static str,
    support: oneharness_core::domain::usage::UsageSupport,
    bin: String,
    available: bool,
    env: Vec<(String, String)>,
    env_remove: Vec<String>,
    cwd: Option<PathBuf>,
    timeout: Duration,
}

impl Plan {
    /// The variant half of the composed selector, so two subscriptions of one
    /// harness stay separately attributed in the report.
    fn variant(&self) -> Option<String> {
        self.id
            .split_once(':')
            .map(|(_, variant)| variant.to_string())
    }

    /// Probe this identity, or record why no probe ran. Never fails: every
    /// outcome — including a missing binary — is a normalized identity.
    fn probe(&self) -> UsageIdentity {
        fault_inject_probe_panic(&self.id);
        self.probe_inner().with_variant(self.variant())
    }

    fn probe_inner(&self) -> UsageIdentity {
        let env = EnvView::new(&self.env, &self.env_remove);
        let Some(probe) = self.support.probe() else {
            // An affirmative "no headroom to report", stated rather than
            // omitted. `expect` is sound: a support tier is either probed or
            // carries a reason, and this is the not-probed half.
            let reason = self
                .support
                .unprobed_reason()
                .expect("a tier with no probe carries a reason");
            return UsageIdentity::new(
                self.base,
                usage_io::selector_for(None, &env),
                unavailable(reason),
            );
        };
        if probe.spawns_harness() && !self.available {
            return UsageIdentity::new(
                self.base,
                usage_io::selector_for(Some(probe), &env),
                oneharness_core::domain::usage::ParsedUsage::unknown(
                    UnknownReason::binary_missing(&self.bin),
                ),
            );
        }
        let probed = usage_io::probe(&UsageProbeRequest {
            probe,
            bin: self.bin.clone(),
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            env_remove: self.env_remove.clone(),
            timeout: self.timeout,
        });
        UsageIdentity::new(self.base, probed.selector, probed.parsed)
    }

    /// What this identity reports when its probe panicked outright. Nothing was
    /// learned, which is exactly `unknown` — and saying so keeps the rest of the
    /// report readable.
    fn crashed(&self) -> UsageIdentity {
        UsageIdentity::new(
            self.base,
            usage_io::selector_for(
                self.support.probe(),
                &EnvView::new(&self.env, &self.env_remove),
            ),
            oneharness_core::domain::usage::ParsedUsage::unknown(UnknownReason::ProbeFailed {
                message: "the probe stopped unexpectedly".to_string(),
            }),
        )
        .with_variant(self.variant())
    }
}

/// Comma-separated harness ids whose probe must panic outright, read by
/// [`fault_inject_probe_panic`].
///
/// A panicking probe is a *bug*, not an input: no payload, timeout, or missing
/// binary produces one, so [`probe_all`]'s containment — itself the fix for a
/// probe that took a whole report down — has nothing that can drive it through
/// the CLI a consumer runs. This injects one, and is compiled only into the
/// `mock-harness` test build, exactly like the mock harness fixture binary, so
/// it cannot exist in a shipped `oneharness`.
#[cfg(feature = "mock-harness")]
const PANIC_PROBE_ENV: &str = "MOCK_PANIC_PROBE";

#[cfg(feature = "mock-harness")]
fn fault_inject_probe_panic(id: &str) {
    let faulted = std::env::var(PANIC_PROBE_ENV).unwrap_or_default();
    assert!(
        !faulted.split(',').any(|name| name == id),
        "fault-injected probe panic for `{id}`"
    );
}

#[cfg(not(feature = "mock-harness"))]
fn fault_inject_probe_panic(_id: &str) {}

fn unavailable(reason: UnavailableReason) -> oneharness_core::domain::usage::ParsedUsage {
    oneharness_core::domain::usage::ParsedUsage {
        auth_mode: AuthMode::Unknown,
        plan: None,
        availability: UsageAvailability::Unavailable { reason },
    }
}

/// Run every plan's probe concurrently, preserving selection order.
///
/// Each probe is joined individually, so one that panics becomes *that
/// identity's* `unknown` instead of taking the whole report down with it. A
/// report is the deliverable here; losing seven readings because the eighth
/// harness misbehaved is the failure mode this command exists to avoid.
fn probe_all(plans: &[Plan]) -> Vec<UsageIdentity> {
    let mut identities = Vec::with_capacity(plans.len());
    // Bounded concurrency without a work queue: the selection is at most the
    // registry's size, so a chunk at a time is both simpler and enough.
    for chunk in plans.chunks(MAX_PARALLEL_PROBES.max(1)) {
        std::thread::scope(|scope| {
            let running: Vec<_> = chunk
                .iter()
                .map(|plan| scope.spawn(|| plan.probe()))
                .collect();
            for (plan, handle) in chunk.iter().zip(running) {
                identities.push(handle.join().unwrap_or_else(|_| plan.crashed()));
            }
        });
    }
    identities
}

fn config_bins(
    config: &oneharness_core::domain::config::FileConfig,
) -> std::collections::HashMap<String, String> {
    let mut bins: std::collections::HashMap<String, String> = config
        .harness
        .iter()
        .filter_map(|(id, harness)| harness.bin.clone().map(|bin| (id.clone(), bin)))
        .collect();
    for (base, harness) in &config.harness {
        for name in harness.variant.keys() {
            let id = format!("{base}:{name}");
            if let Some(bin) = config.bin_for(&id) {
                bins.insert(id, bin.to_string());
            }
        }
    }
    bins
}

/// The single clock read for the whole report — the io layer's job, so the
/// domain stays pure and every identity is stamped with one observation time.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    oneharness_core::domain::history::format_rfc3339(secs)
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
        .as_deref()
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
        .as_deref()
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
    use oneharness_core::domain::usage::{
        IdentitySelector, QuotaAmount, UsedPercent, WindowDuration, Windows,
    };

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
            "2026-07-29T12:00:00Z".to_string(),
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
        binding.resets_at = Some("2026-08-02T13:00:00Z".to_string());
        let unlimited = UsageWindow {
            usage: WindowUsage::Unlimited,
            ..metered("chat", 0.0)
        };
        let report = UsageReport::new(
            "2026-07-29T12:00:00Z".to_string(),
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
            "2026-07-29T12:00:00Z".to_string(),
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
}
