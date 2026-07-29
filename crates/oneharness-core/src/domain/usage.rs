// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] Each payload's gate lives with it: codex's schema is snapshotted and diffed (`scripts/check-codex-usage-schema.sh`), Claude's is `claude_usage_drift`, and Copilot's undocumented endpoint degrades to `Unknown`. See `docs/harness-usage.md`.
//! Normalized subscription **headroom**: the shape of a `oneharness usage`
//! report and one parser per harness payload. Pure — every parser takes an
//! already-captured payload and returns normalized values, and the observation
//! timestamp is passed in, never read from a clock. The probes that spawn a
//! harness (Claude Code's `get_usage` control request, codex's app-server
//! `account/rateLimits/read`, Copilot's `/copilot_internal/user` GET) live
//! outside this module.
//!
//! The report is its own output contract with its own [`SCHEMA_VERSION`],
//! independent of the run report's and the history's.
//!
//! Three normalizations exist because the harnesses disagree on every
//! representational detail, and the disagreements are silent unless made
//! explicit:
//!
//! - **Percent polarity.** [`WindowUsage::Metered::used_percent`] is *always*
//!   percent-**used**. Claude's `utilization` and codex's `usedPercent` are
//!   already percent-used; Copilot's `percent_remaining` and codex's
//!   `individualLimit.remainingPercent` are percent-**remaining** and are
//!   converted when promoted.
//! - **Reset units.** [`UsageWindow::resets_at`] is *always* absolute RFC 3339
//!   UTC. Codex reports epoch seconds, Claude an ISO 8601 instant with a numeric
//!   offset, Copilot an already-UTC RFC 3339 string.
//! - **Window length.** Codex reports `windowDurationMins`; Claude reports
//!   nothing and the length must be derived from the key name — and *cannot* be
//!   for the codename keys the payload also carries. [`WindowDuration`] keeps
//!   that asymmetry in the data instead of papering over it with a default.
//!
//! Two states are deliberately unrepresentable rather than merely discouraged,
//! because rendering either as "0% used / plenty of headroom" is the one way
//! this report can be actively harmful:
//!
//! - An **unavailable** identity has no percentage at all: windows live only
//!   inside [`UsageAvailability::Available`], which cannot be empty.
//! - An **unlimited** quota has no counters at all: [`WindowUsage::Unlimited`]
//!   carries none, so Copilot's `unlimited: true` snapshots (which report
//!   `entitlement: 0` / `remaining: 0` / `percent_remaining: 100.0`, meaningless
//!   as counters) can never render as a full bar.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::history::{civil_from_epoch, format_rfc3339};

/// Bumped when the usage report shape changes in a way a consumer must notice.
/// Independent of [`crate::domain::report::SCHEMA_VERSION`] and
/// [`crate::domain::history::SCHEMA_VERSION`] — this is its own contract.
pub const SCHEMA_VERSION: &str = "0.1";

/// One usage report: every probed identity, stamped with a single observation
/// time supplied by the caller (this module reads no clock).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    /// The report shape version ([`SCHEMA_VERSION`]).
    pub schema_version: String,
    // llmlint: ignore[invalid_states_unrepresentable] RFC 3339 instants are plain strings across every serialized contract in this crate (`report`, `history`, `session`); a bespoke timestamp type here would diverge from all of them, and the value is minted by `io`'s single clock read rather than parsed from an external payload.
    /// RFC 3339 UTC instant the identities were observed, minted by the io layer.
    pub observed_at: String,
    pub identities: Vec<UsageIdentity>,
}

impl UsageReport {
    /// Assemble a report at `observed_at` (RFC 3339 UTC, supplied by the caller).
    #[must_use]
    pub fn new(observed_at: String, identities: Vec<UsageIdentity>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            observed_at,
            identities,
        }
    }
}

/// One harness identity's headroom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageIdentity {
    // llmlint: ignore[invalid_states_unrepresentable] `RunResult::harness` and `HistoryRunRecord::harness` are the established representation for a registry id on the wire, and matching them is what lets a consumer join a usage identity to a run; a newtype here would make this one contract spell it differently from every sibling.
    /// Canonical harness id, matching a [`crate::domain::harness`] registry id.
    pub harness: String,
    /// The named variant this identity came from, when it was selected by a
    /// composed id (`claude-code:work`). Absent for a bare harness id, matching
    /// [`crate::domain::report::RunResult::variant`] — so a consumer joins a
    /// usage identity to the runs it describes on the same pair of fields.
    ///
    /// Two subscriptions of one harness therefore stay distinguishable even when
    /// their [`IdentitySelector`]s do not distinguish them (a variant that
    /// selects an identity by credential rather than by directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// How this identity was selected — never the credential itself.
    pub selector: IdentitySelector,
    pub auth_mode: AuthMode,
    /// The plan as the harness spells it, **verbatim**: Claude's `max` and
    /// codex's `pro` are different vocabularies and are never unified into one
    /// enum. Absent when the harness reports no plan (an API-key session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub availability: UsageAvailability,
}

impl UsageIdentity {
    /// Attach a parsed payload to the identity it was probed for.
    #[must_use]
    pub fn new(harness: &str, selector: IdentitySelector, parsed: ParsedUsage) -> Self {
        Self {
            harness: harness.to_string(),
            variant: None,
            selector,
            auth_mode: parsed.auth_mode,
            plan: parsed.plan,
            availability: parsed.availability,
        }
    }

    /// Attribute this identity to the named variant that selected it.
    #[must_use]
    pub fn with_variant(mut self, variant: Option<String>) -> Self {
        self.variant = variant;
        self
    }
}

/// How one identity is selected for a probe. A secret-valued selector records
/// only the variable's *name*, so a credential can never reach the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentitySelector {
    /// An environment variable naming a credential *directory* — Claude Code's
    /// `CLAUDE_CONFIG_DIR`, codex's `CODEX_HOME`. The path is not a secret.
    EnvPath { env: String, path: String },
    /// An environment variable carrying a credential — Copilot's GitHub token.
    /// The value is deliberately absent.
    EnvSecret { env: String },
    /// The harness's ambient credential store, with nothing overridden.
    Ambient,
}

impl IdentitySelector {
    /// A stable, human-readable key for this identity, safe to print.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::EnvPath { env, path } => format!("{env}={path}"),
            Self::EnvSecret { env } => format!("{env}=<secret>"),
            Self::Ambient => "ambient".to_string(),
        }
    }
}

/// How the identity authenticates, which is what decides whether plan headroom
/// exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// A first-party subscription (Claude.ai, ChatGPT, a GitHub Copilot seat).
    Subscription,
    /// An API key. Both claude-code and codex affirmatively report no plan
    /// headroom in this mode — see [`UnavailableReason::ApiKeyAuth`].
    ApiKey,
    /// Not established by the probe.
    Unknown,
}

/// Tri-state headroom availability.
///
/// The three states are genuinely different answers and are never collapsed:
/// `available` carries real data, `unavailable` is an affirmative "this identity
/// has no plan headroom to report" **with a reason**, and `unknown` means only
/// that nothing was learned. There is no percentage reachable from either
/// non-available state, so neither can be rendered as `0%` used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UsageAvailability {
    /// Real headroom data: one entry per non-null window, never empty.
    Available { windows: Windows },
    /// The harness answered and affirmatively reported no plan headroom.
    Unavailable { reason: UnavailableReason },
    /// Nothing was learned — the identity was not probed, or the probe failed.
    Unknown { reason: UnknownReason },
}

impl UsageAvailability {
    /// Available when at least one window was read, else an affirmative
    /// [`UnavailableReason::NoWindowsReported`]. Never an empty `available`,
    /// which a renderer could mistake for full headroom.
    #[must_use]
    pub fn from_windows(windows: Vec<UsageWindow>) -> Self {
        match Windows::new(windows) {
            Some(windows) => Self::Available { windows },
            None => Self::Unavailable {
                reason: UnavailableReason::NoWindowsReported,
            },
        }
    }

    /// The windows, or an empty slice when not available. This is the only way
    /// to reach a percentage, so an unavailable or unknown identity has none.
    #[must_use]
    pub fn windows(&self) -> &[UsageWindow] {
        match self {
            Self::Available { windows } => windows.as_slice(),
            Self::Unavailable { .. } | Self::Unknown { .. } => &[],
        }
    }
}

/// Why an identity affirmatively has no plan headroom. Distinct from
/// [`UnknownReason`]: these are answers, not absences of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// API-key auth. Verified for both claude-code (`rate_limits_available:
    /// false`, `subscription_type: null`) and codex (`chatgpt authentication
    /// required to read rate limits`): the harness exposes no plan headroom in
    /// this mode. This is a finding, never a guess — and it is *not*
    /// [`UsageAvailability::Unknown`].
    ApiKeyAuth,
    /// No stored credential for the selected identity — codex's `codex account
    /// authentication required to read rate limits`, a different code branch
    /// from the API-key one above.
    NotLoggedIn,
    /// The harness answered, but carried no readable window at all.
    NoWindowsReported,
    /// The harness has **no first-party plan quota** for headroom to exist in:
    /// OpenCode Zen is pay-as-you-go with nothing that resets, and Goose ships
    /// no first-party inference plan at all. Nothing is missing here — the
    /// quantity itself is undefined, which is why this is not
    /// [`Self::NoHeadroomReader`].
    NoPlanQuota,
    /// A plan quota exists but the harness exposes **no non-interactive reader**
    /// for it: Cursor's dollar pools reach only its interactive TUI, Crush's
    /// Hyper credits have no balance command or API, and Qwen's Coding Plan
    /// weekly quota has no reader of any kind. A future CLI release could add
    /// one, which is exactly what separates this from [`Self::NoPlanQuota`].
    NoHeadroomReader,
}

/// Why nothing is known. Reserved **strictly** for unprobed or probe-failed —
/// API-key auth is [`UnavailableReason::ApiKeyAuth`], not this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnknownReason {
    /// No probe was run for this identity.
    Unprobed,
    /// A probe ran and failed, carrying the harness's own message.
    ProbeFailed { message: String },
    /// The harness's binary is not installed, so its probe could not run. The
    /// harness may well have headroom; this identity simply has no reader on
    /// this machine. Mirrors a run's `skipped` status: data, never a crash.
    BinaryMissing { bin: String },
}

/// A non-empty list of windows. The non-emptiness is the invariant that keeps
/// "available" from ever meaning "no data" — construct it with [`Windows::new`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<UsageWindow>", into = "Vec<UsageWindow>")]
pub struct Windows(Vec<UsageWindow>);

impl Windows {
    /// `Some` iff `windows` is non-empty.
    #[must_use]
    pub fn new(windows: Vec<UsageWindow>) -> Option<Self> {
        (!windows.is_empty()).then_some(Self(windows))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[UsageWindow] {
        &self.0
    }
}

impl TryFrom<Vec<UsageWindow>> for Windows {
    type Error = &'static str;

    fn try_from(windows: Vec<UsageWindow>) -> Result<Self, Self::Error> {
        Self::new(windows).ok_or("an available identity must carry at least one window")
    }
}

impl From<Windows> for Vec<UsageWindow> {
    fn from(windows: Windows) -> Self {
        windows.0
    }
}

/// One rate-limit window. Emitted only for a window the harness actually
/// reported: a null window (Claude's `seven_day_opus: null`, codex's `secondary:
/// null`) means "not applicable to this plan", **not** "0% used", so it is
/// omitted rather than zero-filled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// The harness-native window identifier, verbatim where the harness names
    /// one (`five_hour`, `tangelo`, `chat`) and `<limitId>/<slot>` where codex's
    /// two-level buckets are flattened.
    pub id: String,
    /// A human label the harness supplied (codex's `limitName`), when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub usage: WindowUsage,
    /// The window's length, paired with where that length came from.
    #[serde(flatten)]
    pub duration: WindowDuration,
    // llmlint: ignore[invalid_states_unrepresentable] The RFC 3339 invariant is enforced at the only boundary that can violate it: every value here comes from `normalize_timestamp`/`format_rfc3339`, which reject an unparseable or offset-less instant into `None` rather than storing it, and the string spelling matches every sibling timestamp contract.
    /// When the window resets, always absolute RFC 3339 UTC. Absent when the
    /// harness reported no reset, or one that could not be normalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// The model display name this window is scoped to, when it is scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Whether this is the limit currently binding, when the harness says so
    /// (Claude's `limits[].is_active`). Absent when it does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_binding: Option<bool>,
}

/// What a window's consumption looks like. An unlimited quota carries no
/// counters, so it can never be rendered as a metered bar at 0% used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WindowUsage {
    Metered {
        /// **Always percent-used**, whatever polarity the source used, and
        /// validated at the boundary — see [`UsedPercent`].
        used_percent: UsedPercent,
        /// The raw counters behind the percentage, when the harness reported
        /// every one of them. Never partially fabricated, and absent rather than
        /// null when the payload did not carry the full set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counters: Option<QuotaCounters>,
    },
    /// The harness reported this quota as unlimited. No counter is read: an
    /// unlimited Copilot snapshot reports `entitlement: 0` / `remaining: 0`
    /// alongside `percent_remaining: 100.0`, which are meaningless as counters.
    Unlimited,
}

/// A validated percent-**used** figure, whatever polarity the harness reported.
///
/// Validated at the boundary rather than trusted: a harness's payload is
/// external input, and a `NaN`, an infinity, or a negative percentage would
/// render as a nonsense bar. A value *above* 100 is accepted and preserved
/// rather than clamped — it means the harness reported consumption past its own
/// ceiling (an overage), which is precisely when a consumer needs the figure.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct UsedPercent(f64);

impl UsedPercent {
    /// `Some` iff `value` is finite and non-negative.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for UsedPercent {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("a used percentage must be finite and non-negative")
    }
}

impl From<UsedPercent> for f64 {
    fn from(percent: UsedPercent) -> Self {
        percent.0
    }
}

/// The raw counters behind a metered window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaCounters {
    pub entitlement: i64,
    pub used: i64,
    /// The server's own remaining figure, taken as authoritative rather than
    /// recomputed from `entitlement - used`: Copilot's observed values disagree
    /// by about 1 and the server's figure wins.
    pub remaining: i64,
    /// Whether any quota remains on this plan.
    pub has_quota: bool,
    /// Whether spending past the entitlement is permitted. `has_quota: false`
    /// *and* `overage_permitted: false` together are the machine-readable
    /// "exhausted and blocked" signal — see [`QuotaCounters::blocked`].
    pub overage_permitted: bool,
    pub unit: QuotaUnit,
}

impl QuotaCounters {
    /// Exhausted **and** blocked from spending further: the joint-false state of
    /// `has_quota` and `overage_permitted`.
    #[must_use]
    pub fn blocked(&self) -> bool {
        !self.has_quota && !self.overage_permitted
    }
}

/// What the counters count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    /// GitHub AI credits, $0.01 each — Copilot's unit when `token_based_billing`
    /// is true.
    AiCredits,
    /// The harness did not say what its counters count.
    Unspecified,
}

/// A window's length together with the provenance of that length. The
/// discriminator is the point: codex *reports* a duration, Claude's must be
/// *inferred* from the key name, and for Claude's codename keys it simply
/// cannot be derived. Serializes flat as `window_seconds_source` plus (except
/// when unknown) `window_seconds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "window_seconds_source", rename_all = "snake_case")]
pub enum WindowDuration {
    /// The harness stated the length (codex's `windowDurationMins`).
    Reported { window_seconds: u64 },
    /// Derived from the harness's own window key (Claude's `five_hour`).
    InferredFromId { window_seconds: u64 },
    /// Not derivable — an opaque key, or a calendar window with no fixed length.
    Unknown,
}

impl WindowDuration {
    /// The length in seconds, or `None` when it could not be established.
    #[must_use]
    pub fn seconds(&self) -> Option<u64> {
        match *self {
            Self::Reported { window_seconds } | Self::InferredFromId { window_seconds } => {
                Some(window_seconds)
            }
            Self::Unknown => None,
        }
    }

    /// A duration the harness reported, in minutes (codex). A missing or
    /// negative value stays [`WindowDuration::Unknown`] rather than becoming 0.
    fn from_reported_minutes(minutes: Option<i64>) -> Self {
        match minutes {
            Some(minutes) if minutes > 0 => Self::Reported {
                window_seconds: (minutes as u64).saturating_mul(60),
            },
            _ => Self::Unknown,
        }
    }
}

/// One parsed payload: everything a probe learns about a single identity, ready
/// to be attached to it with [`UsageIdentity::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUsage {
    pub auth_mode: AuthMode,
    pub plan: Option<String>,
    pub availability: UsageAvailability,
}

impl ParsedUsage {
    /// An identity nothing is known about — no probe ran, or one failed.
    #[must_use]
    pub fn unknown(reason: UnknownReason) -> Self {
        Self {
            auth_mode: AuthMode::Unknown,
            plan: None,
            availability: UsageAvailability::Unknown { reason },
        }
    }
}

/// What `oneharness usage` can learn about a harness. Registry data
/// ([`crate::domain::harness::HarnessSpec::usage`]) sourced from
/// `docs/harness-usage.md`, never guessed.
///
/// The two non-probing variants are the point of the enum: five of the eight
/// harnesses cannot report headroom, and *which kind* of cannot they are is a
/// real distinction — one has no quota, the other has no reader. Collapsing them
/// (or omitting those harnesses) would make `oneharness usage` quietly cover
/// three of eight while claiming to cover the fleet.
///
/// *How much* a probed harness reports is a property of the probe
/// ([`UsageProbe::reports`]), not a separate choice made here: pairing a tier
/// with a probe would let the registry claim headroom from a probe that reads
/// only a plan name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageSupport {
    /// Read by a zero-turn probe, which decides how much it can report.
    Probed(UsageProbe),
    /// No first-party plan quota exists to report ([`UnavailableReason::NoPlanQuota`]).
    NoPlanQuota,
    /// A plan quota exists with no non-interactive reader
    /// ([`UnavailableReason::NoHeadroomReader`]).
    NoHeadroomReader,
}

impl UsageSupport {
    /// The probe to run for this harness, or `None` when nothing is readable.
    #[must_use]
    pub fn probe(&self) -> Option<UsageProbe> {
        match *self {
            Self::Probed(probe) => Some(probe),
            Self::NoPlanQuota | Self::NoHeadroomReader => None,
        }
    }

    /// The affirmative "no headroom to report" verdict for a non-probing
    /// harness, or `None` when this harness is probed instead.
    #[must_use]
    pub fn unprobed_reason(&self) -> Option<UnavailableReason> {
        match *self {
            Self::NoPlanQuota => Some(UnavailableReason::NoPlanQuota),
            Self::NoHeadroomReader => Some(UnavailableReason::NoHeadroomReader),
            Self::Probed(_) => None,
        }
    }
}

/// How much a probe can report — a property of what the harness exposes, so it
/// cannot disagree with the probe that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageReporting {
    /// Real remaining-headroom windows.
    Headroom,
    /// The plan tier only; the harness publishes no non-interactive headroom
    /// reader, so a successful read is still an affirmative
    /// [`UnavailableReason::NoHeadroomReader`].
    PlanTier,
}

/// One zero-turn probe. Every variant is chosen because it completes **without
/// the harness taking a model turn** — no user message is sent and no turn is
/// completed — which is what makes `oneharness usage` usable as a pre-flight
/// check rather than a thing that costs what it measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageProbe {
    /// Claude Code driven in stream-json input/output mode with an empty tool
    /// set, sent exactly one `get_usage` control request and read for the
    /// matching control response. No user message is ever written, which is what
    /// keeps it free (observed: `num_turns: 0`, `total_cost_usd: 0`).
    ClaudeGetUsage,
    /// `codex app-server --stdio`, driven `initialize` → `initialized` →
    /// `account/rateLimits/read` over JSON-RPC.
    CodexAppServer,
    /// An authenticated `GET /copilot_internal/user` against the GitHub API —
    /// out of band from the Copilot CLI entirely, so it needs no Copilot binary
    /// and answers before a run rather than after a turn is spent. The
    /// run-embedded JSONL quota surface is deliberately not used: oneharness
    /// drives Copilot in text mode, so it is unreachable as wired.
    CopilotUserEndpoint,
    /// `cursor-agent about --format json`, read for `subscriptionTier` only, and
    /// only from a **pre-existing** login. Cursor's `--api-key`/`CURSOR_API_KEY`
    /// path is not a per-process selector — it performs a token exchange and
    /// persists credentials to the shared store, observed clobbering a real user
    /// login — so the probe must never take it.
    CursorAbout,
}

impl UsageProbe {
    /// Whether this probe spawns the harness's own binary (and so needs it
    /// installed). Copilot's is an out-of-band HTTP GET whose entire credential
    /// requirement is a GitHub token, so it answers with no Copilot CLI present.
    #[must_use]
    pub fn spawns_harness(&self) -> bool {
        !matches!(self, Self::CopilotUserEndpoint)
    }

    /// How much this probe can report. Cursor's is the lone
    /// [`UsageReporting::PlanTier`]: its dollar pools reach only the interactive
    /// TUI, so `about` yields a plan name and nothing more.
    #[must_use]
    pub fn reports(&self) -> UsageReporting {
        match self {
            Self::ClaudeGetUsage | Self::CodexAppServer | Self::CopilotUserEndpoint => {
                UsageReporting::Headroom
            }
            Self::CursorAbout => UsageReporting::PlanTier,
        }
    }
}

/// Claude's window lengths, derived from its own key names. The key set is
/// **open** — the observed payload also carries codenames (`tangelo`,
/// `iguana_necktie`, `nimbus_quill`, `cinder_cove`, `amber_ladder`) with no
/// derivable duration — so a key absent from this table becomes
/// [`WindowDuration::Unknown`], never a guessed length. Extend the table only
/// from a key whose window length is actually known.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] Claude Code publishes no schema for these key names, so there is nothing to generate from; the table is drift-*safe* instead — an unknown key degrades to `Unknown`, never a wrong duration.
const CLAUDE_WINDOW_SECONDS: &[(&str, u64)] = &[
    ("five_hour", 5 * 3_600),
    ("seven_day", 7 * 86_400),
    ("seven_day_cowork", 7 * 86_400),
    ("seven_day_oauth_apps", 7 * 86_400),
    ("seven_day_omelette", 7 * 86_400),
    ("seven_day_opus", 7 * 86_400),
    ("seven_day_overage_included", 7 * 86_400),
    ("seven_day_sonnet", 7 * 86_400),
];

/// Keys inside `rate_limits` that are *not* plan windows. `extra_usage` is
/// excluded by name because it carries a `utilization` field of its own (a
/// monthly credit axis, not a plan window) and would otherwise be mistaken for
/// one.
const CLAUDE_NON_WINDOW_KEYS: &[&str] = &[
    "extra_usage",
    "limits",
    "member_dashboard_available",
    "model_scoped",
    "spend",
];

/// Which named window each `limits[].kind` describes, so the array's `is_active`
/// flag can be attached to the window it belongs to. `weekly_scoped` maps to no
/// named key — it is emitted as its own window (see [`parse_claude_get_usage`]).
const CLAUDE_LIMIT_KIND_KEYS: &[(&str, &str)] =
    &[("session", "five_hour"), ("weekly_all", "seven_day")];

/// The seconds in Claude's `weekly_scoped` window: its `group` is literally
/// `weekly`, which is where the length comes from.
const CLAUDE_WEEKLY_SECONDS: u64 = 7 * 86_400;

/// Unwrap the `get_usage` payload from one `control_response` line, so a caller
/// reading Claude's JSONL stream does not have to know the envelope shape.
/// `None` for any other line.
#[must_use]
pub fn claude_control_response(line: &Value) -> Option<&Value> {
    if line.get("type").and_then(Value::as_str) != Some("control_response") {
        return None;
    }
    let response = line.get("response")?;
    if response.get("subtype").and_then(Value::as_str) != Some("success") {
        return None;
    }
    response.get("response")
}

/// The contract-drift guard for Claude's `get_usage` payload: `Some(reason)`
/// when the payload no longer looks like the shape [`parse_claude_get_usage`]
/// was written against, so a caller can degrade to
/// [`UsageAvailability::Unknown`] instead of publishing a confident answer.
///
/// This exists because Claude's structured usage surface is explicitly
/// experimental — the SDK method is literally named
/// `usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET()` — and, unlike
/// codex's app-server, it publishes no schema to snapshot and diff. The failure
/// mode to prevent is a renamed field silently becoming "0% used / plenty of
/// headroom", so every check here targets a branch whose *absent* input would
/// otherwise read as a confident negative:
///
/// - `rate_limits_available` is the branch the parser takes on absence
///   (`unwrap_or(false)`), so its disappearance would read as "no headroom
///   reported" for every user at once.
/// - Under `rate_limits_available: true` the payload must still carry a
///   recognizable window surface: the `limits[]` array with at least one
///   expected [`CLAUDE_LIMIT_KIND_KEYS`]-style `kind`, or a named window key
///   with a numeric `utilization`. Neither present means the shape moved.
///
/// A *new* key or a new `kind` alongside a recognized one is not drift: the key
/// set is open by contract and unknown keys already degrade to an opaque window.
#[must_use]
pub fn claude_usage_drift(payload: &Value) -> Option<String> {
    let Some(available) = payload.get("rate_limits_available") else {
        return Some("the payload carries no `rate_limits_available` field".to_string());
    };
    let Some(available) = available.as_bool() else {
        return Some("`rate_limits_available` is not a boolean".to_string());
    };
    if !available {
        // An affirmative "no plan headroom" — the API-key answer, and the one
        // shape that needs no window surface at all.
        return None;
    }
    let Some(rate_limits) = payload.get("rate_limits").filter(|value| value.is_object()) else {
        return Some(
            "`rate_limits_available` is true but `rate_limits` is not an object".to_string(),
        );
    };

    let known_kind = rate_limits
        .get("limits")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(is_known_claude_limit_kind)
            })
        });
    let metered_window = rate_limits
        .as_object()
        .into_iter()
        .flatten()
        .any(|(key, value)| {
            !CLAUDE_NON_WINDOW_KEYS.contains(&key.as_str())
                && value.get("utilization").and_then(Value::as_f64).is_some()
        });
    if known_kind || metered_window {
        return None;
    }
    Some(format!(
        "`rate_limits_available` is true but no window surface was recognized: \
         `limits[]` carries none of the expected kinds ({}) and no window key carries a \
         numeric `utilization`",
        CLAUDE_LIMIT_KINDS.join(", ")
    ))
}

/// Every `limits[].kind` the observed payload carries. `weekly_scoped` maps to
/// no named window key (see [`CLAUDE_LIMIT_KIND_KEYS`]) but is still an expected
/// member, so the drift guard recognizes it.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] These kinds ARE the drift gate: `get_usage` has no published schema, so the guard asserts on the observed kinds and degrades to `Unknown` — never to zero.
const CLAUDE_LIMIT_KINDS: &[&str] = &["session", "weekly_all", "weekly_scoped"];

fn is_known_claude_limit_kind(kind: &str) -> bool {
    CLAUDE_LIMIT_KINDS.contains(&kind)
}

/// Parse Claude Code's `get_usage` payload (the inner `response` object — see
/// [`claude_control_response`]).
///
/// `subscription_type` is the plan, verbatim, and its absence is the auth-mode
/// discriminator: the CLI documents it as null "for API key / 3P provider
/// sessions". `rate_limits_available` is the boolean to branch on, and under
/// API-key auth it is affirmatively `false` — an
/// [`UnavailableReason::ApiKeyAuth`], never an unknown.
///
/// Windows come from the open `rate_limits` key set (one per non-null key), with
/// `limits[].is_active` attached to the window each entry describes and each
/// `weekly_scoped` entry emitted as its own model-scoped window (it has no named
/// key of its own).
#[must_use]
pub fn parse_claude_get_usage(payload: &Value) -> ParsedUsage {
    let plan = payload
        .get("subscription_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    // Documented by the CLI's own schema text: null for API-key / third-party
    // provider sessions, non-null for a Claude.ai subscription.
    let auth_mode = if plan.is_some() {
        AuthMode::Subscription
    } else {
        AuthMode::ApiKey
    };

    let available = payload
        .get("rate_limits_available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let rate_limits = payload.get("rate_limits").filter(|value| value.is_object());

    let availability = match (available, rate_limits) {
        (true, Some(rate_limits)) => UsageAvailability::from_windows(claude_windows(rate_limits)),
        _ => UsageAvailability::Unavailable {
            reason: match auth_mode {
                AuthMode::ApiKey => UnavailableReason::ApiKeyAuth,
                _ => UnavailableReason::NoWindowsReported,
            },
        },
    };

    ParsedUsage {
        auth_mode,
        plan,
        availability,
    }
}

fn claude_windows(rate_limits: &Value) -> Vec<UsageWindow> {
    let limits = claude_limits(rate_limits);
    let mut windows: Vec<UsageWindow> = Vec::new();

    for (key, value) in rate_limits.as_object().into_iter().flatten() {
        if CLAUDE_NON_WINDOW_KEYS.contains(&key.as_str()) {
            continue;
        }
        // A null key means "not applicable to this plan" — omitted, never
        // zero-filled. Anything without a usable `utilization` is not a window.
        let Some(used_percent) = value
            .get("utilization")
            .and_then(Value::as_f64)
            .and_then(UsedPercent::new)
        else {
            continue;
        };
        let duration = CLAUDE_WINDOW_SECONDS
            .iter()
            .find(|(name, _)| *name == key)
            .map_or(WindowDuration::Unknown, |(_, seconds)| {
                WindowDuration::InferredFromId {
                    window_seconds: *seconds,
                }
            });
        let is_binding = CLAUDE_LIMIT_KIND_KEYS
            .iter()
            .find(|(_, named)| *named == key)
            .and_then(|(kind, _)| limits.iter().find(|limit| limit.kind == *kind))
            .and_then(|limit| limit.is_active);

        windows.push(UsageWindow {
            id: key.clone(),
            label: None,
            usage: WindowUsage::Metered {
                used_percent,
                counters: None,
            },
            duration,
            resets_at: value
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(normalize_timestamp),
            scope: None,
            is_binding,
        });
    }

    // Model-scoped weekly limits have no named `rate_limits` key of their own,
    // so the flat array is the only place they appear.
    for limit in limits.iter().filter(|limit| limit.kind == "weekly_scoped") {
        let Some(used_percent) = limit.percent.and_then(UsedPercent::new) else {
            continue;
        };
        let id = match &limit.scope {
            Some(scope) => format!("weekly_scoped/{scope}"),
            None => "weekly_scoped".to_string(),
        };
        windows.push(UsageWindow {
            id,
            label: limit.scope.clone(),
            usage: WindowUsage::Metered {
                used_percent,
                counters: None,
            },
            duration: WindowDuration::InferredFromId {
                window_seconds: CLAUDE_WEEKLY_SECONDS,
            },
            resets_at: limit.resets_at.as_deref().and_then(normalize_timestamp),
            scope: limit.scope.clone(),
            is_binding: limit.is_active,
        });
    }

    windows
}

/// One entry of Claude's flat `limits[]` array.
struct ClaudeLimit {
    kind: String,
    percent: Option<f64>,
    resets_at: Option<String>,
    scope: Option<String>,
    is_active: Option<bool>,
}

fn claude_limits(rate_limits: &Value) -> Vec<ClaudeLimit> {
    rate_limits
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some(ClaudeLimit {
                        kind: entry.get("kind").and_then(Value::as_str)?.to_string(),
                        percent: entry.get("percent").and_then(Value::as_f64),
                        resets_at: entry
                            .get("resets_at")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        scope: entry
                            .pointer("/scope/model/display_name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        is_active: entry.get("is_active").and_then(Value::as_bool),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// codex's API-key branch: ChatGPT auth is required to read rate limits, so an
/// API-key session affirmatively has no plan headroom.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] A literal in the codex binary that appears in no emitted schema, so there is nothing to generate from; drift degrades safely — an unrecognized message becomes a probe failure, never an assumed absence of headroom.
const CODEX_API_KEY_ERROR: &str = "chatgpt authentication required to read rate limits";
/// codex's no-stored-credential branch. A genuinely separate code path from
/// [`CODEX_API_KEY_ERROR`] (both strings exist once each in the codex binary),
/// so collapsing the two to "error" would throw away a real distinction.
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] Same as [`CODEX_API_KEY_ERROR`]: an unschematized binary literal, degrading safely to a probe failure rather than an assumed absence of headroom.
const CODEX_NOT_LOGGED_IN_ERROR: &str = "codex account authentication required to read rate limits";

/// Parse codex's app-server `account/rateLimits/read` response — the whole
/// JSON-RPC response object, either `{"result": …}` or `{"error": …}`.
///
/// Buckets come from `rateLimitsByLimitId`, keyed by `limitId`, in preference to
/// the top-level `rateLimits`, which the generated schema documents as a
/// "backward-compatible single-bucket view" mirroring one of them. Each bucket's
/// `primary` and optional `secondary` windows flatten to `<limitId>/<slot>` ids
/// that preserve both levels without inventing a matching hierarchy on the
/// Claude side.
#[must_use]
pub fn parse_codex_rate_limits(response: &Value) -> ParsedUsage {
    if let Some(message) = response
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::trim)
    {
        return codex_error(message);
    }

    // The result must be an *object* before anything is concluded from it: a
    // success verdict reached from a payload with no readable shape would be an
    // affirmative "subscription, no windows" built on nothing.
    let Some(result) = response.get("result").filter(|value| value.is_object()) else {
        return ParsedUsage::unknown(UnknownReason::ProbeFailed {
            message: "response carried neither a result object nor an error".to_string(),
        });
    };

    let by_limit_id = result
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|buckets| !buckets.is_empty());
    let mirror = result.get("rateLimits");

    let mut windows = Vec::new();
    let mut plan = None;
    match by_limit_id {
        Some(buckets) => {
            for (limit_id, bucket) in buckets {
                let id = bucket
                    .get("limitId")
                    .and_then(Value::as_str)
                    .unwrap_or(limit_id.as_str());
                windows.extend(codex_bucket_windows(Some(id), bucket));
                plan = plan.or_else(|| codex_plan(bucket));
            }
            // The mirror is the authoritative plan when the buckets omit one.
            plan = mirror.and_then(codex_plan).or(plan);
        }
        None => {
            if let Some(bucket) = mirror {
                let id = bucket.get("limitId").and_then(Value::as_str);
                windows.extend(codex_bucket_windows(id, bucket));
                plan = codex_plan(bucket);
            }
        }
    }

    ParsedUsage {
        // Rate limits read only under ChatGPT auth — an API-key session errors
        // out above, so a successful read is a subscription by construction.
        auth_mode: AuthMode::Subscription,
        plan,
        availability: UsageAvailability::from_windows(windows),
    }
}

fn codex_error(message: &str) -> ParsedUsage {
    let (auth_mode, reason) = match message {
        CODEX_API_KEY_ERROR => (AuthMode::ApiKey, UnavailableReason::ApiKeyAuth),
        CODEX_NOT_LOGGED_IN_ERROR => (AuthMode::Unknown, UnavailableReason::NotLoggedIn),
        // Any other failure is genuinely unknown — never an assumed absence.
        other => {
            return ParsedUsage::unknown(UnknownReason::ProbeFailed {
                message: other.to_string(),
            })
        }
    };
    ParsedUsage {
        auth_mode,
        plan: None,
        availability: UsageAvailability::Unavailable { reason },
    }
}

fn codex_plan(bucket: &Value) -> Option<String> {
    bucket
        .get("planType")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn codex_bucket_windows(limit_id: Option<&str>, bucket: &Value) -> Vec<UsageWindow> {
    let label = bucket
        .get("limitName")
        .and_then(Value::as_str)
        .map(str::to_string);
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|slot| {
            // A null `secondary` means this plan surfaced one window, not a
            // second window at 0% used.
            let window = bucket.get(slot).filter(|value| value.is_object())?;
            let used_percent = window
                .get("usedPercent")
                .and_then(Value::as_f64)
                .and_then(UsedPercent::new)?;
            Some(UsageWindow {
                id: match limit_id {
                    Some(limit_id) => format!("{limit_id}/{slot}"),
                    None => slot.to_string(),
                },
                label: label.clone(),
                usage: WindowUsage::Metered {
                    used_percent,
                    counters: None,
                },
                duration: WindowDuration::from_reported_minutes(
                    window.get("windowDurationMins").and_then(Value::as_i64),
                ),
                resets_at: window
                    .get("resetsAt")
                    .and_then(Value::as_i64)
                    .map(format_rfc3339),
                scope: None,
                is_binding: None,
            })
        })
        .collect()
}

/// Parse Copilot's `/copilot_internal/user` body.
///
/// Each `quota_snapshots.<id>` entry is gated on `unlimited` **before** any
/// counter is read: an unlimited snapshot reports `entitlement: 0`,
/// `remaining: 0` and `percent_remaining: 100.0`, which are meaningless as
/// counters and would render as a false full bar. `percent_remaining` is
/// percent-*remaining* and is converted to percent-used. `remaining` is taken as
/// the server reports it rather than recomputed from `entitlement -
/// credits_used` (the observed values disagree by about 1, and the server's
/// figure wins).
///
/// The reset is a calendar month, which has no fixed length in seconds, so every
/// window's duration is [`WindowDuration::Unknown`] — the honest answer, not a
/// 30-day default.
#[must_use]
pub fn parse_copilot_user(body: &Value) -> ParsedUsage {
    let plan = body
        .get("copilot_plan")
        .and_then(Value::as_str)
        .map(str::to_string);
    let resets_at = body
        .get("quota_reset_date_utc")
        .and_then(Value::as_str)
        .and_then(normalize_timestamp);
    // AI credits are the unit only when the account is on token-based billing.
    let unit = if body
        .get("token_based_billing")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        QuotaUnit::AiCredits
    } else {
        QuotaUnit::Unspecified
    };

    let windows = body
        .get("quota_snapshots")
        .and_then(Value::as_object)
        .map(|snapshots| {
            snapshots
                .iter()
                .filter_map(|(id, snapshot)| copilot_window(id, snapshot, resets_at.clone(), unit))
                .collect()
        })
        .unwrap_or_default();

    ParsedUsage {
        auth_mode: AuthMode::Subscription,
        plan,
        availability: UsageAvailability::from_windows(windows),
    }
}

fn copilot_window(
    id: &str,
    snapshot: &Value,
    resets_at: Option<String>,
    unit: QuotaUnit,
) -> Option<UsageWindow> {
    let unlimited = snapshot
        .get("unlimited")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let usage = if unlimited {
        WindowUsage::Unlimited
    } else {
        // percent_remaining is the inverse polarity of the normalized field, so
        // an out-of-range one converts to a negative used percentage and is
        // rejected by the validating constructor rather than rendered.
        let remaining_percent = snapshot.get("percent_remaining").and_then(Value::as_f64)?;
        WindowUsage::Metered {
            used_percent: UsedPercent::new(100.0 - remaining_percent)?,
            counters: copilot_counters(snapshot, unit),
        }
    };
    Some(UsageWindow {
        id: id.to_string(),
        label: None,
        usage,
        // A calendar month is not a fixed number of seconds.
        duration: WindowDuration::Unknown,
        resets_at,
        scope: None,
        is_binding: None,
    })
}

fn copilot_counters(snapshot: &Value, unit: QuotaUnit) -> Option<QuotaCounters> {
    // All five or none: a partial payload yields the percentage alone rather
    // than a counter set with fabricated members.
    Some(QuotaCounters {
        entitlement: snapshot.get("entitlement").and_then(Value::as_i64)?,
        used: snapshot.get("credits_used").and_then(Value::as_i64)?,
        remaining: snapshot.get("remaining").and_then(Value::as_i64)?,
        has_quota: snapshot.get("has_quota").and_then(Value::as_bool)?,
        overage_permitted: snapshot.get("overage_permitted").and_then(Value::as_bool)?,
        unit,
    })
}

/// How much of a failing HTTP body is quoted back in a probe-failure message.
/// The endpoint returns a server error document (never a credential), but an
/// unbounded body would swamp the report.
const COPILOT_ERROR_BODY_CHARS: usize = 200;

/// Turn one `/copilot_internal/user` HTTP response into a parsed identity.
///
/// `401` is the token being absent, expired, or rejected — no usable credential,
/// which is [`UnavailableReason::NotLoggedIn`] rather than an unknown. Every
/// other non-200, and a 200 whose body is not JSON, stays
/// [`UnknownReason::ProbeFailed`]: the endpoint is undocumented internal, so an
/// unrecognized answer must degrade to "nothing learned", never to zero used.
#[must_use]
pub fn parse_copilot_http(status: u16, body: &str) -> ParsedUsage {
    if status == 401 {
        return ParsedUsage {
            auth_mode: AuthMode::Unknown,
            plan: None,
            availability: UsageAvailability::Unavailable {
                reason: UnavailableReason::NotLoggedIn,
            },
        };
    }
    if status != 200 {
        return ParsedUsage::unknown(UnknownReason::ProbeFailed {
            message: format!(
                "GET /copilot_internal/user returned HTTP {status}: {}",
                snippet(body)
            ),
        });
    }
    match serde_json::from_str::<Value>(body) {
        Ok(value) if value.is_object() => parse_copilot_user(&value),
        _ => ParsedUsage::unknown(UnknownReason::ProbeFailed {
            message: format!(
                "GET /copilot_internal/user returned a body that is not a JSON object: {}",
                snippet(body)
            ),
        }),
    }
}

/// A single-line, character-bounded excerpt of an external payload, for a
/// diagnostic message. Bounded in **characters** so a multi-byte body can never
/// be split mid-code-point.
fn snippet(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    match flat.char_indices().nth(COPILOT_ERROR_BODY_CHARS) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat.to_string(),
    }
}

/// Parse `cursor-agent about --format json`.
///
/// Cursor is a **plan-tier-only** harness: `subscriptionTier` is the sole
/// plan-level field the non-interactive surface carries, and its dollar pools
/// reach only the interactive TUI — so a successful read is still an
/// affirmative [`UnavailableReason::NoHeadroomReader`], never a percentage.
///
/// The tier is a display name (`Team`), not a lowercase enum like Claude's `max`
/// or codex's `pro`, and is kept verbatim like every other harness's plan.
///
/// A null tier means no **stored** login: the CLI populates it only when both an
/// access and a refresh token are on disk, and an API key does not satisfy that
/// gate. So null is [`UnavailableReason::NotLoggedIn`] — and the probe reports
/// it rather than resolving it, because Cursor's API-key path is a login that
/// overwrites the shared credential store.
#[must_use]
pub fn parse_cursor_about(payload: &Value) -> ParsedUsage {
    let plan = payload
        .get("subscriptionTier")
        .and_then(Value::as_str)
        .filter(|tier| !tier.trim().is_empty())
        .map(str::to_string);
    match plan {
        Some(plan) => ParsedUsage {
            auth_mode: AuthMode::Subscription,
            plan: Some(plan),
            availability: UsageAvailability::Unavailable {
                reason: UnavailableReason::NoHeadroomReader,
            },
        },
        None => ParsedUsage {
            auth_mode: AuthMode::Unknown,
            plan: None,
            availability: UsageAvailability::Unavailable {
                reason: UnavailableReason::NotLoggedIn,
            },
        },
    }
}

/// Normalize an RFC 3339 / ISO 8601 instant to absolute RFC 3339 **UTC**,
/// whatever offset and sub-second precision it arrived with. `None` when the
/// text is not a complete instant, including when it carries no offset at all —
/// a local-time string is ambiguous, and guessing a zone would be fabrication.
#[must_use]
pub fn normalize_timestamp(text: &str) -> Option<String> {
    epoch_from_rfc3339(text).map(format_rfc3339)
}

/// Seconds since the UNIX epoch for an RFC 3339 instant. Sub-second precision is
/// parsed but truncated: window resets are minute-scale.
fn epoch_from_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    // Every separator is checked, not just the date/time one: reading digits at
    // fixed offsets out of unvalidated text would otherwise accept shapes like
    // `2026x08x01T00.00.00Z` as a well-formed instant.
    if !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year_text = text.get(0..4)?;
    if !year_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year: i64 = year_text.parse().ok()?;
    let month: u32 = two_digits(text, 5)?;
    let day: u32 = two_digits(text, 8)?;
    let hour: u32 = two_digits(text, 11)?;
    let minute: u32 = two_digits(text, 14)?;
    let second: u32 = two_digits(text, 17)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // A leap second (`:60`) is a real RFC 3339 value; clamping it to :59 keeps
    // the instant within the minute rather than rejecting the whole timestamp.
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut rest = text.get(19..)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = fraction.get(digits..)?;
    }
    let offset_secs = parse_offset(rest)?;

    let days = days_from_civil(year, month, day);
    // `days_from_civil` is total: it happily rolls a date that does not exist
    // (2026-02-29) forward into the next month. Round-tripping the day back
    // through the inverse rejects it instead of silently reporting the wrong
    // instant.
    let (round_year, round_month, round_day, ..) = civil_from_epoch(days * 86_400);
    if (round_year, round_month, round_day) != (year, month, day) {
        return None;
    }

    Some(
        days * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second.min(59))
            - offset_secs,
    )
}

/// The instant's offset from UTC, in seconds. `Z` is zero; a bare instant with
/// no offset is rejected by returning `None`.
fn parse_offset(text: &str) -> Option<i64> {
    if matches!(text, "Z" | "z") {
        return Some(0);
    }
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    // Both `+hh:mm` and the compact `+hhmm` appear in the wild.
    let (hours, minutes) = match rest.len() {
        5 if rest.as_bytes()[2] == b':' => (two_digits(rest, 0)?, two_digits(rest, 3)?),
        4 => (two_digits(rest, 0)?, two_digits(rest, 2)?),
        2 => (two_digits(rest, 0)?, 0),
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (i64::from(hours) * 3_600 + i64::from(minutes) * 60))
}

/// Exactly two ASCII digits at `at`. The digit check is not redundant with the
/// parse: `"+1"` parses as `1`, so a signed pair would otherwise slip through a
/// field that must be two literal digits.
fn two_digits(text: &str, at: usize) -> Option<u32> {
    let pair = text.get(at..at + 2)?;
    pair.bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| pair.parse().ok())
        .flatten()
}

/// Days since 1970-01-01 for a civil (proleptic Gregorian) date — Howard
/// Hinnant's `days_from_civil`, the exact inverse of
/// [`crate::domain::history::civil_from_epoch`]'s date half, so a reset
/// timestamp round-trips through [`format_rfc3339`] with no date library.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400; // [0, 399]
    let mp = i64::from(if month > 2 { month - 3 } else { month + 9 }); // [0, 11]
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn used_percent(window: &UsageWindow) -> f64 {
        match window.usage {
            WindowUsage::Metered { used_percent, .. } => used_percent.get(),
            WindowUsage::Unlimited => panic!("{} is unlimited, not metered", window.id),
        }
    }

    fn metered(used_percent: f64) -> WindowUsage {
        WindowUsage::Metered {
            used_percent: UsedPercent::new(used_percent).expect("a valid percentage"),
            counters: None,
        }
    }

    fn window<'a>(parsed: &'a ParsedUsage, id: &str) -> &'a UsageWindow {
        parsed
            .availability
            .windows()
            .iter()
            .find(|window| window.id == id)
            .unwrap_or_else(|| panic!("no window {id}"))
    }

    fn ids(parsed: &ParsedUsage) -> Vec<&str> {
        parsed
            .availability
            .windows()
            .iter()
            .map(|window| window.id.as_str())
            .collect()
    }

    /// The observed subscription payload: two non-null plan windows, eleven
    /// null ones (five of them codenames), and the flat `limits[]` array.
    fn claude_subscription_payload() -> Value {
        json!({
            "session": {"total_cost_usd": 0, "model_usage": {}},
            "subscription_type": "max",
            "rate_limits_available": true,
            "rate_limits": {
                "five_hour": {
                    "utilization": 42,
                    "resets_at": "2026-07-29T18:30:00.123456+00:00",
                    "limit_dollars": null, "used_dollars": null, "remaining_dollars": null
                },
                "seven_day": {
                    "utilization": 61,
                    "resets_at": "2026-08-02T09:00:00.000000-04:00",
                    "limit_dollars": null, "used_dollars": null, "remaining_dollars": null
                },
                "seven_day_oauth_apps": null,
                "seven_day_opus": null,
                "seven_day_sonnet": null,
                "seven_day_cowork": null,
                "seven_day_omelette": null,
                "tangelo": null,
                "iguana_necktie": null,
                "omelette_promotional": null,
                "nimbus_quill": null,
                "cinder_cove": null,
                "amber_ladder": null,
                "extra_usage": {
                    "is_enabled": true, "monthly_limit": 5000, "used_credits": 1200,
                    "utilization": 24.0, "currency": "USD", "decimal_places": 2,
                    "disabled_reason": "out_of_credits", "user_disabled": false,
                    "spend_limit_reached": false, "credits_ever_enabled": true,
                    "daily": null, "weekly": null
                },
                "limits": [
                    {"kind": "session", "group": "session", "percent": 42,
                     "severity": "normal", "resets_at": "2026-07-29T18:30:00.123456+00:00",
                     "scope": null, "is_active": false},
                    {"kind": "weekly_all", "group": "weekly", "percent": 61,
                     "severity": "normal", "resets_at": "2026-08-02T13:00:00.000000+00:00",
                     "scope": null, "is_active": true},
                    {"kind": "weekly_scoped", "group": "weekly", "percent": 17,
                     "severity": "normal", "resets_at": "2026-08-02T13:00:00.000000+00:00",
                     "scope": {"model": {"id": null, "display_name": "Opus 5"}, "surface": null},
                     "is_active": false}
                ],
                "member_dashboard_available": true,
                "model_scoped": [
                    {"display_name": "Opus 5", "utilization": 17,
                     "resets_at": "2026-08-02T13:00:00.000000+00:00"}
                ]
            },
            "behaviors": {"day": {"request_count": 3, "session_count": 1}}
        })
    }

    #[test]
    fn claude_plan_is_kept_verbatim_and_windows_are_percent_used() {
        let parsed = parse_claude_get_usage(&claude_subscription_payload());

        assert_eq!(parsed.auth_mode, AuthMode::Subscription);
        assert_eq!(parsed.plan.as_deref(), Some("max"));
        let five_hour = window(&parsed, "five_hour");
        assert_eq!(used_percent(five_hour), 42.0);
        assert_eq!(
            five_hour.duration,
            WindowDuration::InferredFromId {
                window_seconds: 18_000
            }
        );
        assert_eq!(five_hour.resets_at.as_deref(), Some("2026-07-29T18:30:00Z"));
        // A -04:00 offset must land four hours later in UTC.
        assert_eq!(
            window(&parsed, "seven_day").resets_at.as_deref(),
            Some("2026-08-02T13:00:00Z")
        );
    }

    #[test]
    fn claude_null_windows_are_omitted_never_zero_filled() {
        let parsed = parse_claude_get_usage(&claude_subscription_payload());

        assert_eq!(
            ids(&parsed),
            vec!["five_hour", "seven_day", "weekly_scoped/Opus 5"],
            "eleven null keys must be absent, not present at 0%"
        );
        for null_key in ["seven_day_opus", "tangelo", "amber_ladder"] {
            assert!(
                !ids(&parsed).contains(&null_key),
                "{null_key} was null and must not be emitted"
            );
        }
    }

    #[test]
    fn claude_unknown_codename_key_round_trips_as_an_opaque_window() {
        let mut payload = claude_subscription_payload();
        payload["rate_limits"]["tangelo"] = json!({
            "utilization": 7,
            "resets_at": "2026-07-30T00:00:00.000000+00:00",
            "limit_dollars": null, "used_dollars": null, "remaining_dollars": null
        });

        let parsed = parse_claude_get_usage(&payload);

        let tangelo = window(&parsed, "tangelo");
        assert_eq!(used_percent(tangelo), 7.0);
        assert_eq!(
            tangelo.duration,
            WindowDuration::Unknown,
            "a codename key implies no duration and must never have one guessed"
        );
        assert_eq!(tangelo.duration.seconds(), None);
        assert_eq!(tangelo.resets_at.as_deref(), Some("2026-07-30T00:00:00Z"));
        assert_eq!(
            window(&parsed, "five_hour").duration.seconds(),
            Some(18_000),
            "a derivable key still reports its length"
        );
    }

    #[test]
    fn claude_extra_usage_is_not_mistaken_for_a_window() {
        let parsed = parse_claude_get_usage(&claude_subscription_payload());

        assert!(
            !ids(&parsed).contains(&"extra_usage"),
            "extra_usage carries its own `utilization` but is a monthly credit axis"
        );
    }

    #[test]
    fn claude_limits_array_marks_the_binding_window_and_scoped_one() {
        let parsed = parse_claude_get_usage(&claude_subscription_payload());

        assert_eq!(window(&parsed, "five_hour").is_binding, Some(false));
        assert_eq!(window(&parsed, "seven_day").is_binding, Some(true));

        let scoped = window(&parsed, "weekly_scoped/Opus 5");
        assert_eq!(used_percent(scoped), 17.0);
        assert_eq!(scoped.scope.as_deref(), Some("Opus 5"));
        assert_eq!(
            scoped.duration,
            WindowDuration::InferredFromId {
                window_seconds: 604_800
            }
        );
    }

    #[test]
    fn claude_scoped_limit_without_a_model_or_percent_degrades_honestly() {
        let mut payload = claude_subscription_payload();
        payload["rate_limits"]["limits"] = json!([
            {"kind": "weekly_scoped", "group": "weekly", "percent": 17,
             "severity": "normal", "resets_at": null, "scope": null, "is_active": false},
            {"kind": "weekly_scoped", "group": "weekly", "percent": null,
             "severity": "normal", "resets_at": null,
             "scope": {"model": {"id": null, "display_name": "Opus 5"}, "surface": null},
             "is_active": false},
            {"kind": "not-a-known-kind"}
        ]);

        let parsed = parse_claude_get_usage(&payload);

        assert_eq!(
            ids(&parsed),
            vec!["five_hour", "seven_day", "weekly_scoped"],
            "an unscoped entry keeps the bare kind; one with no percent is dropped"
        );
        let scoped = window(&parsed, "weekly_scoped");
        assert_eq!(scoped.scope, None);
        assert_eq!(scoped.resets_at, None);
        assert_eq!(
            window(&parsed, "five_hour").is_binding,
            None,
            "no session entry means no binding flag, never a fabricated false"
        );
    }

    #[test]
    fn claude_api_key_response_is_unavailable_not_unknown() {
        let payload = json!({
            "session": {"total_cost_usd": 0, "model_usage": {}},
            "subscription_type": null,
            "rate_limits_available": false,
            "rate_limits": null,
            "behaviors": null
        });

        let parsed = parse_claude_get_usage(&payload);

        assert_eq!(parsed.auth_mode, AuthMode::ApiKey);
        assert_eq!(parsed.plan, None);
        assert_eq!(
            parsed.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::ApiKeyAuth
            },
            "API-key auth is a verified finding, never `unknown`"
        );
        assert!(parsed.availability.windows().is_empty());
    }

    #[test]
    fn claude_subscription_without_rate_limits_is_not_an_api_key_verdict() {
        let payload = json!({
            "subscription_type": "team",
            "rate_limits_available": false,
            "rate_limits": null
        });

        let parsed = parse_claude_get_usage(&payload);

        assert_eq!(parsed.auth_mode, AuthMode::Subscription);
        assert_eq!(
            parsed.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NoWindowsReported
            }
        );
    }

    #[test]
    fn claude_control_response_unwraps_only_a_successful_usage_line() {
        let payload = claude_subscription_payload();
        let line = json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "req_1", "response": payload}
        });
        assert_eq!(
            claude_control_response(&line)
                .and_then(|payload| payload.get("subscription_type"))
                .and_then(Value::as_str),
            Some("max")
        );

        assert!(claude_control_response(&json!({"type": "assistant"})).is_none());
        assert!(
            claude_control_response(&json!({
                "type": "control_response",
                "response": {"subtype": "error", "error": "boom", "response": payload}
            }))
            .is_none(),
            "an error subtype carries no usable payload even when a body is present"
        );
    }

    fn codex_snapshot(limit_id: &str, limit_name: Value, used: i64, secondary: Value) -> Value {
        json!({
            "limitId": limit_id,
            "limitName": limit_name,
            "primary": {"usedPercent": used, "windowDurationMins": 10_080, "resetsAt": 1_785_000_000},
            "secondary": secondary,
            "credits": {"hasCredits": true, "unlimited": false, "balance": "12.34"},
            "individualLimit": null,
            "spendControlReached": false,
            "planType": "pro",
            "rateLimitReachedType": null
        })
    }

    fn codex_result() -> Value {
        json!({
            "id": 2,
            "result": {
                "rateLimits": codex_snapshot("codex", Value::Null, 31, Value::Null),
                "rateLimitsByLimitId": {
                    "codex": codex_snapshot("codex", Value::Null, 31, Value::Null),
                    "limit_model_x": codex_snapshot("limit_model_x", json!("GPT-5.3 Codex"), 12, Value::Null)
                },
                "rateLimitResetCredits": {"availableCount": 0, "credits": []}
            }
        })
    }

    #[test]
    fn codex_prefers_by_limit_id_and_reports_its_window_duration() {
        let parsed = parse_codex_rate_limits(&codex_result());

        assert_eq!(parsed.auth_mode, AuthMode::Subscription);
        assert_eq!(
            parsed.plan.as_deref(),
            Some("pro"),
            "codex's plan vocabulary stays verbatim alongside Claude's"
        );
        assert_eq!(
            ids(&parsed),
            vec!["codex/primary", "limit_model_x/primary"],
            "the top-level rateLimits mirror must not duplicate the bucket"
        );
        let primary = window(&parsed, "codex/primary");
        assert_eq!(used_percent(primary), 31.0);
        assert_eq!(
            primary.duration,
            WindowDuration::Reported {
                window_seconds: 604_800
            },
            "codex states its window length; it is never inferred"
        );
        assert_eq!(
            window(&parsed, "limit_model_x/primary").label.as_deref(),
            Some("GPT-5.3 Codex")
        );
    }

    #[test]
    fn codex_null_secondary_emits_no_second_window() {
        let parsed = parse_codex_rate_limits(&codex_result());

        assert!(
            !ids(&parsed).iter().any(|id| id.ends_with("/secondary")),
            "a null secondary means one window on this plan, not a second at 0%"
        );
    }

    #[test]
    fn codex_secondary_window_is_emitted_when_present() {
        let mut response = codex_result();
        response["result"]["rateLimitsByLimitId"]["codex"]["secondary"] =
            json!({"usedPercent": 88, "windowDurationMins": null, "resetsAt": null});

        let parsed = parse_codex_rate_limits(&response);

        let secondary = window(&parsed, "codex/secondary");
        assert_eq!(used_percent(secondary), 88.0);
        assert_eq!(
            secondary.duration,
            WindowDuration::Unknown,
            "a null windowDurationMins is unknown, not zero"
        );
        assert_eq!(secondary.resets_at, None);
    }

    #[test]
    fn codex_epoch_reset_becomes_absolute_utc() {
        let parsed = parse_codex_rate_limits(&codex_result());

        assert_eq!(
            window(&parsed, "codex/primary").resets_at.as_deref(),
            Some("2026-07-25T17:20:00Z")
        );
    }

    #[test]
    fn codex_falls_back_to_the_single_bucket_mirror() {
        let response = json!({
            "id": 2,
            "result": {"rateLimits": codex_snapshot("codex", Value::Null, 55, Value::Null)}
        });

        let parsed = parse_codex_rate_limits(&response);

        assert_eq!(ids(&parsed), vec!["codex/primary"]);
        assert_eq!(used_percent(window(&parsed, "codex/primary")), 55.0);
    }

    #[test]
    fn codex_mirror_without_a_limit_id_keeps_the_bare_slot_name() {
        let response = json!({
            "id": 2,
            "result": {"rateLimits": {
                "primary": {"usedPercent": 9, "windowDurationMins": 300, "resetsAt": null},
                "secondary": null,
                "planType": "plus"
            }}
        });

        let parsed = parse_codex_rate_limits(&response);

        assert_eq!(
            ids(&parsed),
            vec!["primary"],
            "with no limitId to prefix, the slot name stands alone rather than an invented id"
        );
        assert_eq!(parsed.plan.as_deref(), Some("plus"));
        assert_eq!(
            window(&parsed, "primary").duration,
            WindowDuration::Reported {
                window_seconds: 18_000
            }
        );
    }

    #[test]
    fn codex_api_key_error_is_distinct_from_the_not_logged_in_error() {
        let api_key = parse_codex_rate_limits(&json!({
            "id": 4,
            "error": {"code": -32600, "message": CODEX_API_KEY_ERROR}
        }));
        assert_eq!(api_key.auth_mode, AuthMode::ApiKey);
        assert_eq!(
            api_key.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::ApiKeyAuth
            }
        );

        let not_logged_in = parse_codex_rate_limits(&json!({
            "id": 4,
            "error": {"code": -32600, "message": CODEX_NOT_LOGGED_IN_ERROR}
        }));
        assert_eq!(not_logged_in.auth_mode, AuthMode::Unknown);
        assert_eq!(
            not_logged_in.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NotLoggedIn
            },
            "no stored credential is a different branch from API-key auth"
        );

        assert_ne!(api_key.availability, not_logged_in.availability);
    }

    #[test]
    fn codex_unrecognized_error_is_unknown_not_unavailable() {
        let parsed = parse_codex_rate_limits(&json!({
            "id": 4,
            "error": {"code": -32603, "message": "internal error"}
        }));

        assert_eq!(
            parsed.availability,
            UsageAvailability::Unknown {
                reason: UnknownReason::ProbeFailed {
                    message: "internal error".to_string()
                }
            }
        );
    }

    #[test]
    fn codex_response_without_a_result_object_or_error_is_a_probe_failure() {
        // A missing result, and a result that is not an object: neither carries a
        // shape to conclude from, so neither may reach a verdict about the plan.
        for response in [
            json!({"id": 4}),
            json!({"id": 4, "result": null}),
            json!({"id": 4, "result": "ok"}),
            json!({"id": 4, "result": []}),
        ] {
            let parsed = parse_codex_rate_limits(&response);

            assert!(
                matches!(
                    parsed.availability,
                    UsageAvailability::Unknown {
                        reason: UnknownReason::ProbeFailed { .. }
                    }
                ),
                "{response} must be nothing learned, not an answer"
            );
            assert_eq!(
                parsed.auth_mode,
                AuthMode::Unknown,
                "{response} establishes no auth mode either"
            );
        }
    }

    /// The observed body: two unlimited snapshots and one exhausted metered one.
    fn copilot_body() -> Value {
        json!({
            "copilot_plan": "individual",
            "access_type_sku": "monthly_subscriber_quota",
            "quota_reset_date": "2026-08-01",
            "quota_reset_date_utc": "2026-08-01T00:00:00.000Z",
            "token_based_billing": true,
            "quota_snapshots": {
                "chat": {
                    "unlimited": true, "percent_remaining": 100.0, "has_quota": true,
                    "entitlement": 0, "remaining": 0, "credits_used": 0,
                    "overage_permitted": false, "quota_reset_at": 0
                },
                "completions": {
                    "unlimited": true, "percent_remaining": 100.0, "has_quota": true,
                    "entitlement": 0, "remaining": 0, "credits_used": 0,
                    "overage_permitted": false, "quota_reset_at": 0
                },
                "premium_interactions": {
                    "unlimited": false, "percent_remaining": 0.0, "has_quota": false,
                    "entitlement": 1500, "credits_used": 13518, "remaining": -12019,
                    "quota_remaining": -12018.1, "overage_count": 12016,
                    "overage_permitted": false, "overage_entitlement": 20000,
                    "timestamp_utc": "2026-07-29T12:54:35.200Z", "quota_reset_at": 0
                }
            }
        })
    }

    #[test]
    fn copilot_unlimited_snapshots_carry_no_counters() {
        let parsed = parse_copilot_user(&copilot_body());

        assert_eq!(parsed.plan.as_deref(), Some("individual"));
        for id in ["chat", "completions"] {
            assert_eq!(
                window(&parsed, id).usage,
                WindowUsage::Unlimited,
                "{id} is unlimited: its 0/0/100.0 counters must never be read"
            );
        }
    }

    #[test]
    fn copilot_exhausted_snapshot_is_percent_used_and_blocked() {
        let parsed = parse_copilot_user(&copilot_body());

        let premium = window(&parsed, "premium_interactions");
        assert_eq!(
            used_percent(premium),
            100.0,
            "percent_remaining 0.0 is 100% used, not 0%"
        );
        let WindowUsage::Metered {
            counters: Some(counters),
            ..
        } = &premium.usage
        else {
            panic!("premium_interactions must carry counters");
        };
        assert_eq!(counters.entitlement, 1500);
        assert_eq!(counters.used, 13518);
        assert_eq!(
            counters.remaining, -12019,
            "the server's own remaining figure wins over entitlement - used"
        );
        assert_eq!(counters.unit, QuotaUnit::AiCredits);
        assert!(
            counters.blocked(),
            "has_quota and overage_permitted both false is exhausted-and-blocked"
        );
    }

    #[test]
    fn copilot_healthy_snapshot_is_not_blocked_and_converts_polarity() {
        let mut body = copilot_body();
        body["quota_snapshots"]["premium_interactions"] = json!({
            "unlimited": false, "percent_remaining": 75.0, "has_quota": true,
            "entitlement": 1500, "credits_used": 375, "remaining": 1125,
            "overage_permitted": true, "quota_reset_at": 0
        });

        let parsed = parse_copilot_user(&body);

        let premium = window(&parsed, "premium_interactions");
        assert_eq!(used_percent(premium), 25.0);
        let WindowUsage::Metered {
            counters: Some(counters),
            ..
        } = &premium.usage
        else {
            panic!("counters expected");
        };
        assert!(!counters.blocked());
    }

    #[test]
    fn copilot_reset_normalizes_and_month_length_stays_unknown() {
        let parsed = parse_copilot_user(&copilot_body());

        let chat = window(&parsed, "chat");
        assert_eq!(chat.resets_at.as_deref(), Some("2026-08-01T00:00:00Z"));
        assert_eq!(
            chat.duration,
            WindowDuration::Unknown,
            "a calendar month has no fixed length in seconds"
        );
    }

    #[test]
    fn copilot_partial_snapshot_keeps_the_percentage_without_fabricated_counters() {
        let body = json!({
            "copilot_plan": "individual",
            "token_based_billing": false,
            "quota_snapshots": {
                "premium_interactions": {"unlimited": false, "percent_remaining": 40.0}
            }
        });

        let parsed = parse_copilot_user(&body);

        assert_eq!(
            window(&parsed, "premium_interactions").usage,
            metered(60.0),
            "no counter field at all still yields the percentage"
        );

        // Every single missing member drops the whole counter set: a default
        // for any one of the five would be a fabricated figure.
        for missing in [
            "entitlement",
            "credits_used",
            "remaining",
            "has_quota",
            "overage_permitted",
        ] {
            let mut partial = copilot_body();
            partial["quota_snapshots"]["premium_interactions"]
                .as_object_mut()
                .unwrap()
                .remove(missing);

            let parsed = parse_copilot_user(&partial);

            assert_eq!(
                window(&parsed, "premium_interactions").usage,
                metered(100.0),
                "a snapshot missing {missing} must carry no counters at all"
            );
        }
    }

    #[test]
    fn an_unusable_percentage_drops_its_window_rather_than_rendering_nonsense() {
        assert_eq!(UsedPercent::new(0.0).map(UsedPercent::get), Some(0.0));
        assert_eq!(UsedPercent::new(100.0).map(UsedPercent::get), Some(100.0));
        assert_eq!(
            UsedPercent::new(140.0).map(UsedPercent::get),
            Some(140.0),
            "a harness reporting consumption past its ceiling is real overage, not an error"
        );
        assert_eq!(UsedPercent::new(-1.0), None);
        assert_eq!(UsedPercent::new(f64::NAN), None);
        assert_eq!(UsedPercent::new(f64::INFINITY), None);
        assert!(serde_json::from_str::<UsedPercent>("-1").is_err());

        // Each parser drops a window whose percentage cannot be believed.
        let mut claude = claude_subscription_payload();
        claude["rate_limits"]["five_hour"]["utilization"] = json!(-3);
        assert_eq!(
            ids(&parse_claude_get_usage(&claude)),
            vec!["seven_day", "weekly_scoped/Opus 5"]
        );

        let mut codex = codex_result();
        codex["result"]["rateLimitsByLimitId"]["codex"]["primary"]["usedPercent"] = json!(-5);
        assert_eq!(
            ids(&parse_codex_rate_limits(&codex)),
            vec!["limit_model_x/primary"]
        );

        let mut copilot = copilot_body();
        copilot["quota_snapshots"]["premium_interactions"]["percent_remaining"] = json!(140.0);
        assert_eq!(
            ids(&parse_copilot_user(&copilot)),
            vec!["chat", "completions"],
            "a percent_remaining above 100 inverts to a negative used percentage"
        );
    }

    #[test]
    fn copilot_without_snapshots_is_unavailable() {
        let parsed = parse_copilot_user(&json!({"copilot_plan": "individual"}));

        assert_eq!(
            parsed.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NoWindowsReported
            }
        );
    }

    #[test]
    fn an_unavailable_identity_carries_no_percentage_at_all() {
        let parsed = parse_codex_rate_limits(&json!({
            "id": 4,
            "error": {"code": -32600, "message": CODEX_API_KEY_ERROR}
        }));
        let identity = UsageIdentity::new(
            "codex",
            IdentitySelector::EnvPath {
                env: "CODEX_HOME".to_string(),
                path: "/home/u/.codex".to_string(),
            },
            parsed,
        );

        let text = serde_json::to_string(&UsageReport::new(
            "2026-07-29T12:00:00Z".to_string(),
            vec![identity],
        ))
        .unwrap();

        assert!(text.contains(r#""state":"unavailable""#));
        assert!(text.contains(r#""reason":"api_key_auth""#));
        assert!(
            !text.contains("used_percent"),
            "an unavailable identity must have no percentage to render as 0%"
        );
        assert!(!text.contains("windows"));
    }

    #[test]
    fn an_available_identity_cannot_be_built_with_no_windows() {
        assert!(Windows::new(Vec::new()).is_none());
        assert_eq!(
            UsageAvailability::from_windows(Vec::new()),
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NoWindowsReported
            }
        );
        let empty: Result<Windows, _> = serde_json::from_str("[]");
        assert!(
            empty.is_err(),
            "an empty available window list must not deserialize either"
        );
    }

    #[test]
    fn a_report_round_trips_through_json() {
        let identities = vec![
            UsageIdentity::new(
                "claude-code",
                IdentitySelector::EnvPath {
                    env: "CLAUDE_CONFIG_DIR".to_string(),
                    path: "/home/u/.claude".to_string(),
                },
                parse_claude_get_usage(&claude_subscription_payload()),
            ),
            UsageIdentity::new(
                "codex",
                IdentitySelector::Ambient,
                parse_codex_rate_limits(&codex_result()),
            ),
            UsageIdentity::new(
                "copilot",
                IdentitySelector::EnvSecret {
                    env: "GH_TOKEN".to_string(),
                },
                parse_copilot_user(&copilot_body()),
            ),
            UsageIdentity::new(
                "goose",
                IdentitySelector::Ambient,
                ParsedUsage::unknown(UnknownReason::Unprobed),
            ),
        ];
        let report = UsageReport::new("2026-07-29T12:00:00Z".to_string(), identities);

        let text = serde_json::to_string(&report).unwrap();
        let parsed: UsageReport = serde_json::from_str(&text).unwrap();

        assert_eq!(parsed, report);
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert!(text.contains(r#""window_seconds_source":"inferred_from_id""#));
        assert!(text.contains(r#""window_seconds_source":"reported""#));
        assert!(text.contains(r#""window_seconds_source":"unknown""#));
        assert!(
            !text.contains("GH_TOKEN\",\"value"),
            "a secret selector records the variable name only"
        );
    }

    #[test]
    fn selector_keys_never_carry_a_credential() {
        assert_eq!(
            IdentitySelector::EnvPath {
                env: "CODEX_HOME".to_string(),
                path: "/home/u/.codex".to_string()
            }
            .key(),
            "CODEX_HOME=/home/u/.codex"
        );
        assert_eq!(
            IdentitySelector::EnvSecret {
                env: "GH_TOKEN".to_string()
            }
            .key(),
            "GH_TOKEN=<secret>"
        );
        assert_eq!(IdentitySelector::Ambient.key(), "ambient");
    }

    #[test]
    fn timestamps_normalize_to_absolute_utc() {
        assert_eq!(
            normalize_timestamp("2026-08-01T00:00:00.000Z").as_deref(),
            Some("2026-08-01T00:00:00Z")
        );
        assert_eq!(
            normalize_timestamp("2026-08-02T09:00:00.000000-04:00").as_deref(),
            Some("2026-08-02T13:00:00Z")
        );
        assert_eq!(
            normalize_timestamp("2026-08-02T09:00:00+0530").as_deref(),
            Some("2026-08-02T03:30:00Z")
        );
        assert_eq!(
            normalize_timestamp("2026-08-02T09:00:00-07").as_deref(),
            Some("2026-08-02T16:00:00Z"),
            "an hours-only offset is a valid RFC 3339 shape"
        );
        assert_eq!(
            normalize_timestamp("2028-02-29T23:59:59Z").as_deref(),
            Some("2028-02-29T23:59:59Z"),
            "a real leap day must survive"
        );
        assert_eq!(
            normalize_timestamp("1970-01-01T00:00:00Z").as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
    }

    #[test]
    fn an_instant_without_an_offset_is_rejected_rather_than_guessed() {
        assert_eq!(normalize_timestamp("2026-08-01T00:00:00"), None);
        assert_eq!(normalize_timestamp("2026-08-01"), None);
        assert_eq!(normalize_timestamp(""), None);
        assert_eq!(normalize_timestamp("not a timestamp at all"), None);
        assert_eq!(normalize_timestamp("2026-13-01T00:00:00Z"), None);
        assert_eq!(normalize_timestamp("2026-08-01T25:00:00Z"), None);
        assert_eq!(normalize_timestamp("2026-08-01T00:00:00.Z"), None);
        for malformed_separator in [
            "2026x08x01T00:00:00Z", // date separators are not checked by offset alone
            "2026-08-01T00.00.00Z", // nor are the time separators
            "2026-08-01X00:00:00Z", // nor the date/time separator
            "20x6-08-01T00:00:00Z", // a non-digit year
            "+026-08-01T00:00:00Z", // a signed year parses as a number unless digits are required
            "-026-08-01T00:00:00Z", // and a negative one is no RFC 3339 instant either
            "2026-+8-01T00:00:00Z", // `+8` parses as 8 unless digits are required
        ] {
            assert_eq!(
                normalize_timestamp(malformed_separator),
                None,
                "{malformed_separator} is not a well-formed instant"
            );
        }
        for malformed_offset in [
            "2026-08-01T00:00:0005:00", // offset-shaped but unsigned: direction unknown
            "2026-08-01T00:00:00 05:00", // no sign at all
            "2026-08-01T00:00:00+5:00", // not two digits
            "2026-08-01T00:00:00+05:0", // truncated
            "2026-08-01T00:00:00+050",  // neither `+hh`, `+hhmm`, nor `+hh:mm`
            "2026-08-01T00:00:00+24:00", // hours out of range
            "2026-08-01T00:00:00+05:60", // minutes out of range
        ] {
            assert_eq!(
                normalize_timestamp(malformed_offset),
                None,
                "{malformed_offset} is not a usable offset"
            );
        }
        assert_eq!(
            normalize_timestamp("2026-02-29T00:00:00Z"),
            None,
            "2026 is not a leap year: the date must be rejected, not rolled into March"
        );
    }

    #[test]
    fn claude_drift_guard_passes_the_observed_payload_and_its_api_key_shape() {
        assert_eq!(claude_usage_drift(&claude_subscription_payload()), None);

        let api_key = json!({
            "subscription_type": null,
            "rate_limits_available": false,
            "rate_limits": null
        });
        assert_eq!(
            claude_usage_drift(&api_key),
            None,
            "an affirmative `false` needs no window surface — it is the API-key answer"
        );
    }

    #[test]
    fn claude_drift_guard_catches_a_vanished_availability_flag() {
        // The parser reads this flag with `unwrap_or(false)`, so a rename would
        // silently turn every subscription into "no headroom reported". The
        // guard is what makes that read as `unknown` instead.
        let mut payload = claude_subscription_payload();
        payload
            .as_object_mut()
            .expect("an object")
            .remove("rate_limits_available");

        let drift = claude_usage_drift(&payload).expect("a renamed flag is drift");
        assert!(drift.contains("rate_limits_available"), "{drift}");

        payload["rate_limits_available"] = json!("yes");
        assert!(claude_usage_drift(&payload).is_some(), "a non-boolean too");
    }

    #[test]
    fn claude_drift_guard_catches_a_window_surface_that_moved() {
        let mut payload = claude_subscription_payload();
        // Every window key renamed and every `limits[].kind` unrecognized: the
        // parser would find nothing and report an affirmative "no windows".
        payload["rate_limits"] = json!({
            "renamed_five_hour": {"pct_used": 42},
            "limits": [{"kind": "brand_new_kind", "percent": 42}]
        });

        let drift = claude_usage_drift(&payload).expect("a moved window surface is drift");
        assert!(drift.contains("session"), "{drift}");
        assert_eq!(
            parse_claude_get_usage(&payload).availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NoWindowsReported
            },
            "which is exactly the confident answer the guard exists to suppress"
        );
    }

    #[test]
    fn claude_drift_guard_accepts_additive_change_around_a_recognized_surface() {
        // The key set is open by contract: a new codename or a new `kind` next to
        // a recognized one is normal evolution, not drift.
        let mut payload = claude_subscription_payload();
        payload["rate_limits"]["brand_new_codename"] = json!({"utilization": 3});
        payload["rate_limits"]["limits"] = json!([
            {"kind": "brand_new_kind", "percent": 1},
            {"kind": "session", "percent": 42, "is_active": true}
        ]);
        assert_eq!(claude_usage_drift(&payload), None);

        // A recognized window key alone also suffices — the `limits[]` array is
        // not the only surface.
        let no_limits = json!({
            "rate_limits_available": true,
            "rate_limits": {"five_hour": {"utilization": 12}}
        });
        assert_eq!(claude_usage_drift(&no_limits), None);
    }

    #[test]
    fn claude_drift_guard_catches_rate_limits_that_stopped_being_an_object() {
        let payload = json!({"rate_limits_available": true, "rate_limits": []});

        assert!(claude_usage_drift(&payload).is_some());
    }

    #[test]
    fn cursor_reports_a_plan_tier_and_affirmatively_no_headroom_reader() {
        let about = json!({
            "cliVersion": "2026.07.23-e383d2b",
            "model": "Auto",
            "subscriptionTier": "Team",
            "userEmail": "someone@example.com"
        });

        let parsed = parse_cursor_about(&about);

        assert_eq!(parsed.auth_mode, AuthMode::Subscription);
        assert_eq!(
            parsed.plan.as_deref(),
            Some("Team"),
            "Cursor's tier is a display name, kept verbatim like every other plan"
        );
        assert_eq!(
            parsed.availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NoHeadroomReader
            },
            "the dollar pools exist but reach only the interactive TUI"
        );
        assert!(parsed.availability.windows().is_empty());
    }

    #[test]
    fn cursor_without_a_stored_login_is_not_logged_in_rather_than_unknown() {
        for tier in [json!(null), json!(""), json!("   ")] {
            let parsed = parse_cursor_about(&json!({
                "cliVersion": "2026.07.23-e383d2b",
                "subscriptionTier": tier,
                "userEmail": null
            }));

            assert_eq!(
                parsed.availability,
                UsageAvailability::Unavailable {
                    reason: UnavailableReason::NotLoggedIn
                },
                "a null tier means no stored token pair — reported, never resolved by logging in"
            );
            assert_eq!(parsed.plan, None);
        }
    }

    #[test]
    fn copilot_http_200_parses_and_401_is_not_logged_in() {
        let body = copilot_body().to_string();
        let parsed = parse_copilot_http(200, &body);
        assert_eq!(parsed.plan.as_deref(), Some("individual"));
        assert_eq!(used_percent(window(&parsed, "premium_interactions")), 100.0);

        assert_eq!(
            parse_copilot_http(401, "{\"message\":\"Bad credentials\"}").availability,
            UsageAvailability::Unavailable {
                reason: UnavailableReason::NotLoggedIn
            }
        );
    }

    #[test]
    fn copilot_http_failures_degrade_to_unknown_never_to_zero_used() {
        let server_error = parse_copilot_http(503, "upstream unavailable");
        let UsageAvailability::Unknown {
            reason: UnknownReason::ProbeFailed { message },
        } = &server_error.availability
        else {
            panic!("a 503 is nothing learned, not an answer");
        };
        assert!(message.contains("503"), "{message}");
        assert!(message.contains("upstream unavailable"), "{message}");

        let not_json = parse_copilot_http(200, "<html>proxy login</html>");
        assert!(matches!(
            not_json.availability,
            UsageAvailability::Unknown {
                reason: UnknownReason::ProbeFailed { .. }
            }
        ));
        assert!(
            not_json.availability.windows().is_empty(),
            "no percentage is reachable from a failed probe"
        );
    }

    #[test]
    fn a_failing_body_is_quoted_back_bounded_and_on_one_line() {
        let long = format!("é{}", "x".repeat(500));

        let parsed = parse_copilot_http(500, &format!("first\nsecond {long}"));
        let UsageAvailability::Unknown {
            reason: UnknownReason::ProbeFailed { message },
        } = &parsed.availability
        else {
            panic!("expected a probe failure");
        };

        assert!(!message.contains('\n'), "a report line stays one line");
        assert!(message.ends_with('…'), "and is bounded: {message}");
        assert!(
            message.chars().count() < COPILOT_ERROR_BODY_CHARS + 100,
            "an unbounded body would swamp the report"
        );
    }

    #[test]
    fn usage_support_maps_each_tier_to_a_probe_or_an_affirmative_reason() {
        assert_eq!(
            UsageSupport::Probed(UsageProbe::ClaudeGetUsage).probe(),
            Some(UsageProbe::ClaudeGetUsage)
        );
        assert_eq!(
            UsageSupport::Probed(UsageProbe::ClaudeGetUsage).unprobed_reason(),
            None
        );
        assert_eq!(
            UsageSupport::Probed(UsageProbe::CursorAbout).probe(),
            Some(UsageProbe::CursorAbout)
        );
        // How much a probe reports belongs to the probe, so the registry cannot
        // claim headroom from one that only reads a plan name.
        assert_eq!(
            UsageProbe::CursorAbout.reports(),
            UsageReporting::PlanTier,
            "`about` yields a plan tier and nothing more"
        );
        for probe in [
            UsageProbe::ClaudeGetUsage,
            UsageProbe::CodexAppServer,
            UsageProbe::CopilotUserEndpoint,
        ] {
            assert_eq!(probe.reports(), UsageReporting::Headroom);
        }

        assert_eq!(UsageSupport::NoPlanQuota.probe(), None);
        assert_eq!(
            UsageSupport::NoPlanQuota.unprobed_reason(),
            Some(UnavailableReason::NoPlanQuota)
        );
        assert_eq!(
            UsageSupport::NoHeadroomReader.unprobed_reason(),
            Some(UnavailableReason::NoHeadroomReader),
            "a quota that exists with no reader is a different answer from no quota at all"
        );

        assert!(
            !UsageProbe::CopilotUserEndpoint.spawns_harness(),
            "the Copilot probe is out of band: it needs no Copilot binary"
        );
        for probe in [
            UsageProbe::ClaudeGetUsage,
            UsageProbe::CodexAppServer,
            UsageProbe::CursorAbout,
        ] {
            assert!(probe.spawns_harness());
        }
    }

    #[test]
    fn days_from_civil_inverts_the_history_epoch_helper() {
        for secs in [0_i64, 1_785_000_000, -86_400, 4_102_444_800] {
            let text = format_rfc3339(secs);
            assert_eq!(
                epoch_from_rfc3339(&text),
                Some(secs),
                "{text} must round-trip"
            );
        }
    }
}
