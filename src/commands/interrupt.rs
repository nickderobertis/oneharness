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
    socket_path, ControlReason, ControlRequest, ControlResponse,
};
use oneharness_core::domain::harness;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::control;
use oneharness_core::io::session as session_io;

use crate::cli::InterruptArgs;

pub fn run(args: &InterruptArgs) -> Result<i32, OneharnessError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = session_io::resolve_dir(args.session_dir.as_deref().and_then(|p| p.to_str()))
        .ok_or(OneharnessError::ControlNoSessionDir)?;

    let response = refuse_unsupported(&dir, &cwd, &args.session)
        .unwrap_or_else(|| control::send(&socket_path(&dir, &args.session), interrupt()));

    let text = if args.compact {
        serde_json::to_string(&response)?
    } else {
        serde_json::to_string_pretty(&response)?
    };
    println!("{text}");
    Ok(i32::from(!response.ok))
}

fn interrupt() -> ControlRequest {
    ControlRequest::interrupt()
}

/// The `unsupported` refusal, when the store says this session is bound to a
/// harness with no control mechanism. `None` means "ask the run": either the
/// session is unknown here (so the socket is the only authority) or its harness
/// is control-capable.
///
/// Answering from the store matters because it is the one refusal a supervisor
/// can get *before* wasting a dispatch: a harness that can never be interrupted
/// says so whether or not a run happens to be alive.
fn refuse_unsupported(
    dir: &std::path::Path,
    cwd: &std::path::Path,
    name: &str,
) -> Option<ControlResponse> {
    let record = session_io::read(&session_io::session_path(dir, cwd, name))?;
    let spec = harness::by_id(&record.harness)?;
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
