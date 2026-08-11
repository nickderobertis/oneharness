/* Generated from oneharness-core. Do not edit. */

/**
 * How the identity authenticates, which is what decides whether plan headroom
 * exists at all.
 */
export type AuthMode = "subscription" | "api_key" | "unknown";
/**
 * Tri-state headroom availability.
 *
 * The three states are genuinely different answers and are never collapsed:
 * `available` carries real data, `unavailable` is an affirmative "this identity
 * has no plan headroom to report" **with a reason**, and `unknown` means only
 * that nothing was learned. There is no percentage reachable from either
 * non-available state, so neither can be rendered as `0%` used.
 */
export type UsageAvailability =
  | {
      state: "available";
      windows: Windows;
      [k: string]: unknown;
    }
  | {
      reason: UnavailableReason;
      state: "unavailable";
      [k: string]: unknown;
    }
  | {
      reason: UnknownReason;
      state: "unknown";
      [k: string]: unknown;
    };
/**
 * One rate-limit window. Emitted only for a window the harness actually
 * reported: a null window (Claude's `seven_day_opus: null`, codex's `secondary:
 * null`) means "not applicable to this plan", **not** "0% used", so it is
 * omitted rather than zero-filled.
 */
export type UsageWindow = {
  /**
   * The harness-native window identifier, verbatim where the harness names
   * one (`five_hour`, `tangelo`, `chat`) and `<limitId>/<slot>` where codex's
   * two-level buckets are flattened.
   */
  id: string;
  /**
   * Whether this is the limit currently binding, when the harness says so
   * (Claude's `limits[].is_active`). Absent when it does not.
   */
  is_binding?: boolean | null | undefined;
  /**
   * A human label the harness supplied (codex's `limitName`), when it did.
   */
  label?: string | null | undefined;
  /**
   * When the window resets, always absolute RFC 3339 UTC — a [`UtcInstant`],
   * so no other text is representable here. Absent when the harness reported
   * no reset, or one that could not be normalized.
   */
  resets_at?: UtcInstant | null | undefined;
  /**
   * The model display name this window is scoped to, when it is scoped.
   */
  scope?: string | null | undefined;
  usage: WindowUsage;
  [k: string]: unknown;
} & UsageWindow1;
/**
 * An RFC 3339 timestamp in UTC (`Z`), in oneharness's canonical spelling.
 */
export type UtcInstant = string;
/**
 * What a window's consumption looks like. An unlimited quota carries no
 * counters, so it can never be rendered as a metered bar at 0% used.
 */
export type WindowUsage =
  | {
      /**
       * The raw counters behind the percentage, when the harness reported
       * every one of them. Never partially fabricated, and absent rather than
       * null when the payload did not carry the full set.
       */
      counters?: QuotaCounters | null | undefined;
      kind: "metered";
      /**
       * **Always percent-used**, whatever polarity the source used, and
       * validated at the boundary — see [`UsedPercent`].
       */
      used_percent: number;
      [k: string]: unknown;
    }
  | {
      kind: "unlimited";
      [k: string]: unknown;
    };
/**
 * A counter that cannot legitimately be negative. An entitlement is a ceiling
 * and a consumption is an amount spent; neither has a meaning below zero, so a
 * negative one is a payload that failed to parse rather than an account state,
 * and it is rejected at the boundary like [`UsedPercent`].
 *
 * Deliberately *not* used for [`QuotaCounters::remaining`], which is genuinely
 * negative for an account past its ceiling — the deficit is the signal there,
 * so constraining it would discard a real over-consumption reading.
 */
export type QuotaAmount = number;
/**
 * What the counters count.
 */
export type QuotaUnit = "ai_credits" | "unspecified";
export type UsageWindow1 =
  | {
      window_seconds: number;
      window_seconds_source: "reported";
      [k: string]: unknown;
    }
  | {
      window_seconds: number;
      window_seconds_source: "inferred_from_id";
      [k: string]: unknown;
    }
  | {
      window_seconds_source: "unknown";
      [k: string]: unknown;
    };
/**
 * A non-empty list of windows. The non-emptiness is the invariant that keeps
 * "available" from ever meaning "no data" — construct it with [`Windows::new`].
 */
export type Windows = UsageWindow[];
/**
 * Why an identity affirmatively has no plan headroom. Distinct from
 * [`UnknownReason`]: these are answers, not absences of one.
 */
export type UnavailableReason =
  "api_key_auth" | "not_logged_in" | "no_windows_reported" | "no_plan_quota" | "no_headroom_reader";
/**
 * Why nothing is known. Reserved **strictly** for unprobed or probe-failed —
 * API-key auth is [`UnavailableReason::ApiKeyAuth`], not this.
 */
export type UnknownReason =
  | {
      kind: "unprobed";
      [k: string]: unknown;
    }
  | {
      kind: "probe_failed";
      message: string;
      [k: string]: unknown;
    }
  | {
      bin: string;
      kind: "binary_missing";
      [k: string]: unknown;
    };

/**
 * One usage report: every probed identity, stamped with a single observation
 * time supplied by the caller (this module reads no clock).
 *
 * Deserializing is a consumer boundary — `oneharness-core` is published for
 * sibling tools — so the envelope is validated on the way in rather than
 * trusted: see [`UsageReportWire`].
 */
export interface UsageReport {
  identities: UsageIdentity[];
  /**
   * An RFC 3339 timestamp in UTC (`Z`), in oneharness's canonical spelling.
   */
  observed_at: string;
  /**
   * The shape version, as a type with one value — see [`SchemaVersion`].
   */
  schema_version: null;
  [k: string]: unknown;
}
/**
 * One harness identity's headroom.
 *
 * Deserializing is a consumer boundary like the envelope's, so every string an
 * identity carries is flattened through [`without_control_chars`] on the way in
 * — see [`UsageIdentityWire`].
 */
export interface UsageIdentity {
  auth_mode: AuthMode;
  availability: UsageAvailability;
  /**
   * Canonical harness id, matching a [`crate::domain::harness`] registry id.
   */
  harness: string;
  /**
   * The plan as the harness spells it, **verbatim**: Claude's `max` and
   * codex's `pro` are different vocabularies and are never unified into one
   * enum. Absent when the harness reports no plan (an API-key session).
   */
  plan?: string | null | undefined;
  /**
   * How this identity was selected — never the credential itself.
   */
  selector:
    | {
        env: string;
        kind: "env_path";
        path: string;
        [k: string]: unknown;
      }
    | {
        env: string;
        kind: "env_secret";
        [k: string]: unknown;
      }
    | {
        kind: "ambient";
        [k: string]: unknown;
      };
  /**
   * The named variant this identity came from, when it was selected by a
   * composed id (`claude-code:work`). Absent for a bare harness id, matching
   * [`crate::domain::report::RunResult::variant`] — so a consumer joins a
   * usage identity to the runs it describes on the same pair of fields.
   *
   * Two subscriptions of one harness therefore stay distinguishable even when
   * their [`IdentitySelector`]s do not distinguish them (a variant that
   * selects an identity by credential rather than by directory).
   *
   * A [`VariantName`], so the field cannot be *set* to a name no config could
   * have declared — the same enforcement [`UsageReport::observed_at`] gets
   * from [`UtcInstant`]. It serializes transparently, so the wire keeps the
   * plain string [`crate::domain::report::RunResult::variant`] carries.
   */
  variant?: string | null | undefined;
  [k: string]: unknown;
}
/**
 * The raw counters behind a metered window.
 */
export interface QuotaCounters {
  entitlement: QuotaAmount;
  /**
   * Whether any quota remains on this plan.
   */
  has_quota: boolean;
  /**
   * Whether spending past the entitlement is permitted. `has_quota: false`
   * *and* `overage_permitted: false` together are the machine-readable
   * "exhausted and blocked" signal — see [`QuotaCounters::blocked`].
   */
  overage_permitted: boolean;
  /**
   * The server's own remaining figure, taken as authoritative rather than
   * recomputed from `entitlement - used`: Copilot's observed values disagree
   * by about 1 and the server's figure wins. Signed, and the one counter here
   * that is: an account past its ceiling reports a real deficit.
   */
  remaining: number;
  unit: QuotaUnit;
  used: QuotaAmount;
  [k: string]: unknown;
}
