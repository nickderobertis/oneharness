/* Generated from oneharness-core. Do not edit. */

/**
 * The answer to one `oneharness interrupt`: either the abort was served, or it was refused with a reason.
 */
export type InterruptResponse =
  | {
      mechanism: ControlShape;
      ok: true;
      /**
       * Whether the request's redirection was committed with the abort. Omitted rather than sent as `false`, so a plain interrupt's answer gains no field the supervisor did not ask for.
       */
      redirected?: boolean | undefined;
      v: 2;
      [k: string]: unknown;
    }
  | {
      error: string;
      ok: false;
      reason: ControlReason;
      v: 2;
      [k: string]: unknown;
    };
/**
 * How a harness accepts an out-of-band interrupt for an in-flight turn.
 *
 * Registry data on [`crate::domain::harness::HarnessSpec::control`], sourced
 * from a live interrupt against the real CLI — never guessed. `None` there
 * means `oneharness interrupt` is a loud usage error for the harness, never a
 * silent no-op: a supervisor that is told "ok" while the turn keeps running is
 * worse off than one told the lever does not exist.
 */
export type ControlShape =
  "claude-control-request" | "codex-app-server" | "opencode-http" | "acp-cancel" | "crush-http";
/**
 * Why a control request could not be served. Distinct reasons because a
 * supervisor reacts differently to each: `unsupported` is permanent for the
 * harness, `not_running` means the dispatch is gone, `no_active_turn` means
 * the run is alive but between turns.
 */
export type ControlReason = "unsupported" | "no_active_turn" | "not_running";
