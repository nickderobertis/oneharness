//! `oneharness interrupt --session <NAME>`: abort a running turn from outside.
//!
//! A separate process from the `run --control` it addresses — the reason turn
//! control is a socket at all. It resolves the same `<session-dir>/control/
//! <NAME>.sock` the run opened, sends one request frame, and prints the run's
//! answer verbatim so a supervisor reads exactly what the run decided.
//!
//! Two refusals are answered here rather than over the wire, because they are
//! knowable without a live run: a harness with no control mechanism
//! (`unsupported`, read from the session store's harness binding) and a socket
//! nobody is listening on (`not_running`).

use oneharness_core::domain::control::{
    socket_path, ControlReason, ControlRequest, ControlResponse, RedirectInput,
};
use oneharness_core::domain::harness;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::control;
use oneharness_core::io::session as session_io;

use crate::cli::InterruptArgs;

pub fn run(args: &InterruptArgs) -> Result<i32, OneharnessError> {
    // Validated before anything is resolved: a redirection that cannot be
    // carried is the supervisor's mistake, and it should hear about it while the
    // turn it meant to redirect is still running.
    let request = match args.input.as_deref() {
        Some(input) => ControlRequest::redirect(
            RedirectInput::new(input)
                .map_err(|reason| OneharnessError::ControlInputInvalid { reason })?,
        ),
        None => ControlRequest::interrupt(),
    };
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // A `--session-dir` that cannot be spelled as UTF-8 is refused rather than
    // dropped: silently falling back to the default store would report
    // `not_running` for a run that is very much running.
    let configured = match args.session_dir.as_deref() {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| OneharnessError::SessionDirInvalid {
                    path: path.display().to_string(),
                })?,
        ),
        None => None,
    };
    let dir = session_io::resolve_dir(configured).ok_or(OneharnessError::ControlNoSessionDir)?;

    let response = refuse_unsupported(&dir, &cwd, &args.session)
        .unwrap_or_else(|| control::send(&socket_path(&dir, &args.session), &request));

    // Through the shared writer, not `println!`: a supervisor piping this into
    // `head`, or dying between the interrupt and reading its answer, closes
    // stdout — and `println!` panics on that instead of reporting it. The same
    // writer every other JSON-emitting command uses, so the answer frame fails
    // the way a report does.
    crate::commands::print_json(&response, args.compact)?;
    Ok(i32::from(!response.is_ok()))
}

/// The `unsupported` refusal, when the store says this session is bound to a
/// harness with no control mechanism. `None` means "ask the run": either the
/// session is unknown here (so the socket is the only authority) or its harness
/// is control-capable.
///
/// Answering from the store matters because it is the one refusal a supervisor
/// can get *before* wasting a dispatch: a harness that can never be interrupted
/// says so whether or not a run happens to be alive.
///
/// The store read is validated at the boundary by the record's own type: a
/// `harness` that is not a well-formed
/// [`oneharness_core::domain::harness::HarnessIdentity`] fails to deserialize and
/// [`session_io::read`] reports no record, so the only value reaching the
/// registry here is one parsing already proved names a harness.
///
/// Deliberately NOT filtered through
/// [`oneharness_core::domain::session::resumable`], unlike the `run` path. That
/// filter drops a record whose *token* this build will not resume;
/// control capability is a property of the adapter, which every store version
/// records faithfully — a `0.1` record's bare `"cursor"` answers "can this ever be
/// interrupted?" exactly as well as a qualified one. Dropping it would make a
/// supervisor spend a dispatch to learn what the store already knew.
fn refuse_unsupported(
    dir: &std::path::Path,
    cwd: &std::path::Path,
    name: &str,
) -> Option<ControlResponse> {
    let record = session_io::read(&session_io::session_path(dir, cwd, name))?;
    // The record's identity is variant-qualified (`claude-code:alternate`), and
    // control is a property of the adapter, so read the registry entry off the
    // identity rather than looking the whole composed id up as a bare harness id.
    let spec = record.harness.spec();
    if spec.control.is_some() {
        return None;
    }
    Some(ControlResponse::refused(
        format!(
            "harness `{}` has no out-of-band turn control (control-capable: {})",
            spec.id,
            control_capable_ids()
        ),
        ControlReason::Unsupported,
    ))
}

/// The comma-joined ids of every control-capable harness, for the diagnostic.
fn control_capable_ids() -> String {
    let ids: Vec<&str> = harness::all()
        .iter()
        .filter(|spec| spec.control.is_some())
        .map(|spec| spec.id)
        .collect();
    if ids.is_empty() {
        "none".to_string()
    } else {
        ids.join(", ")
    }
}
