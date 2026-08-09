//! Driving one turn against a control server over HTTP.
//!
//! The third execution model. A turn is neither the harness's headless run (its
//! interrupt does not reach it — live-REFUTED for both harnesses) nor a
//! JSON-RPC conversation on a child's stdin: oneharness opens a session on the
//! server, follows its event stream, submits the prompt, and answers whatever
//! the server blocks on until the turn ends. The interrupt is then one more
//! request against the same session, which is what lets a *separate* process
//! reach the live turn.
//!
//! Every route and payload is pure ([`crate::domain::http`]); this owns only
//! sockets, threads and the clock.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::domain::control::ServerAddress;
use crate::domain::http::{
    self, ClientId, HttpShape, PermissionAsk, PermissionDecision, ResourceId, TurnAddress,
    TurnEvent,
};
use crate::domain::mode::PermissionMode;
use crate::domain::report::{Capture, RunInstant, Status};
use crate::io::http::{HttpClient, StreamPoll};

/// How long one control request may take before the turn is called broken.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a quiet event stream is read before looping to re-check the
/// deadline. Short enough to notice a finished turn promptly, long enough not
/// to spin.
const POLL_SLICE: Duration = Duration::from_secs(2);

/// A turn in flight on a control server, addressable from another thread.
///
/// Cloneable on purpose: the socket thread serving `oneharness interrupt` holds
/// one while the run's own thread follows the event stream.
#[derive(Debug, Clone)]
pub struct HttpTurn {
    client: HttpClient,
    address: TurnAddress,
}

impl HttpTurn {
    /// The protocol this turn is driven over, read off the coordinates it is
    /// addressed by so the two can never disagree.
    fn shape(&self) -> HttpShape {
        self.address.shape()
    }

    /// Abort the live turn. `Ok(())` means the server accepted the request;
    /// every other answer is reported rather than swallowed, because a
    /// supervisor told "ok" while the turn keeps running is worse off than one
    /// told it failed.
    pub fn interrupt(&self) -> io::Result<()> {
        let request = http::interrupt_request(&self.address);
        let response = self.client.send(&request)?;
        if response.ok() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "the control server refused the interrupt ({}): {}",
                response.status,
                response.body.trim()
            )))
        }
    }

    /// The harness's own session id for this turn, which the report echoes and
    /// a later run resumes from.
    #[must_use]
    pub fn session_id(&self) -> &str {
        self.address.session().as_str()
    }
}

/// What one HTTP-submitted turn produced.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub status: Status,
    /// The assistant's answer, when the stream carried one. Never fabricated.
    pub text: Option<String>,
    /// Every event payload observed, newline-joined — the run's `stdout`, so a
    /// consumer needing certainty can parse exactly what oneharness saw.
    pub transcript: String,
    pub error: Option<String>,
    /// The invocation boundaries this turn was observed between, in the same
    /// shape a spawned run reports them — and in the same type, so text that is
    /// not a millisecond-precision UTC instant cannot reach a measurement.
    pub started_at: RunInstant,
    pub finished_at: Option<RunInstant>,
    pub duration_ms: Option<u128>,
}

impl TurnOutcome {
    /// The execution envelope for a turn that had no subprocess of its own.
    ///
    /// The event transcript stands in for `stdout`: there was no child to
    /// capture, and a consumer needing certainty should still be able to parse
    /// exactly what oneharness saw on the wire.
    #[must_use]
    pub fn to_capture(&self) -> Capture {
        Capture {
            status: self.status,
            exit_code: None,
            duration_ms: self.duration_ms,
            stdout: self.transcript.clone(),
            stderr: String::new(),
            error: self.error.clone(),
            started_at: self.started_at.as_str().to_string(),
            finished_at: self.finished_at.as_ref().map(|at| at.as_str().to_string()),
            stdout_observations: Vec::new(),
        }
    }

    /// The outcome of a turn that could not be opened at all: no server, no
    /// session, nothing ran. Reported as data, exactly like a harness that
    /// could not be spawned.
    #[must_use]
    pub fn failed(error: String) -> Self {
        let now = utc_now();
        TurnOutcome {
            status: Status::SpawnError,
            text: None,
            transcript: String::new(),
            error: Some(error),
            started_at: now.clone(),
            finished_at: Some(now),
            duration_ms: Some(0),
        }
    }
}

fn utc_now() -> RunInstant {
    RunInstant::from_epoch_millis(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
}

/// Open a turn on `address`: create the session (and, for crush, the workspace
/// it hangs off), and tell a permissive server once that it need not ask.
pub fn open(
    shape: HttpShape,
    server: ServerAddress,
    cwd: &str,
    mode: PermissionMode,
    client_id: &str,
) -> io::Result<HttpTurn> {
    let client = HttpClient::new(server, REQUEST_TIMEOUT);
    let decision = http::permits_action(mode);

    // One arm per protocol, because the coordinates a turn is addressed by are
    // per protocol: opencode's open request already IS its session, while
    // crush's is a workspace the session then hangs off, named by a client
    // identity opencode has no notion of.
    let address = match shape {
        HttpShape::Opencode => TurnAddress::Opencode {
            session: created_id(
                &expect_ok(
                    &client,
                    &http::open_request(shape, cwd, None, decision),
                    "open the control session",
                )?
                .body,
                "answered no usable id",
            )?,
        },
        HttpShape::Crush => {
            let identity = ClientId::new(client_id)
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
            let opened = expect_ok(
                &client,
                &http::open_request(shape, cwd, Some(&identity), decision),
                "open the control session",
            )?;
            let workspace = created_id(&opened.body, "answered no usable id")?;
            let created = expect_ok(
                &client,
                &http::session_request(&workspace, &identity),
                "create the control session",
            )?;
            TurnAddress::Crush {
                session: created_id(&created.body, "created no usable session")?,
                workspace,
                client: identity,
            }
        }
    };

    if let Some(request) = http::skip_permissions_request(&address, decision) {
        expect_ok(&client, &request, "set the permission posture")?;
    }

    Ok(HttpTurn { client, address })
}

/// The id a create response named, or the loud failure of a server that named
/// none. Never guessed: an unrecognized answer fails the turn rather than
/// addressing an id nobody returned.
fn created_id(body: &str, what: &str) -> io::Result<ResourceId> {
    http::parse_id(body).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the control server {what}: {}", body.trim()),
        )
    })
}

/// Submit `prompt` and follow the turn to its end, answering every permission
/// the server blocks on.
///
/// A turn that ends because it was interrupted is still a completed run here:
/// oneharness records the interrupt from its own side (the socket that served
/// it), never from what the harness says about how the turn stopped.
pub fn run(turn: &HttpTurn, prompt: &str, mode: PermissionMode, timeout: Duration) -> TurnOutcome {
    let decision = http::permits_action(mode);
    let started = Instant::now();
    let started_at = utc_now();
    let deadline = started + timeout;
    let transcript = Arc::new(Mutex::new(Vec::<String>::new()));
    let text = Arc::new(Mutex::new(String::new()));
    let finished = Arc::new(AtomicBool::new(false));

    // The stream is opened BEFORE the prompt: a turn that finishes quickly
    // would otherwise end before anything was listening, and the run would wait
    // out its whole timeout for an event that already happened.
    let stream = turn
        .client
        .open_stream(&http::event_stream_request(&turn.address), POLL_SLICE);
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            return TurnOutcome {
                status: Status::SpawnError,
                text: None,
                transcript: String::new(),
                error: Some(format!(
                    "could not follow the control server's events: {err}"
                )),
                started_at,
                finished_at: Some(utc_now()),
                duration_ms: Some(started.elapsed().as_millis()),
            }
        }
    };

    let submit_error = Arc::new(Mutex::new(None::<String>));
    let submitter = {
        let turn = turn.clone();
        let prompt = prompt.to_string();
        let finished = Arc::clone(&finished);
        let submit_error = Arc::clone(&submit_error);
        std::thread::spawn(move || {
            let request = http::prompt_request(&turn.address, &prompt);
            // A prompt the server would not take means there is no turn to
            // follow, so the reader is released rather than left waiting out the
            // whole timeout for events that can never arrive.
            let refusal = match turn.client.send(&request) {
                Ok(response) if response.ok() => None,
                Ok(response) => Some(format!(
                    "the control server refused the prompt ({}): {}",
                    response.status,
                    response.body.trim()
                )),
                Err(err) => Some(format!("could not submit the prompt: {err}")),
            };
            if let Some(refusal) = refusal {
                *submit_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(refusal);
                finished.store(true, Ordering::SeqCst);
            }
        })
    };

    let mut timed_out = false;
    // Whether the stream ended before the turn did. Buffered events are handed
    // over before a close is ever reported, so this is only true of a server
    // that really did stop mid-turn.
    let mut closed_early = false;
    // The turn is only over once it has begun: see `TurnEvent::Started`.
    let mut in_flight = false;
    let mut ended = false;
    while !ended {
        if finished.load(Ordering::SeqCst) {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        match stream.poll() {
            StreamPoll::Event(payload) => {
                transcript
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(payload.clone());
                match http::classify_event(turn.shape(), &payload) {
                    TurnEvent::PermissionRequest(ask) => answer(turn, &ask, decision),
                    TurnEvent::Text(chunk) => {
                        text.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push_str(&chunk);
                    }
                    TurnEvent::Started => in_flight = true,
                    TurnEvent::Finished => {
                        if in_flight {
                            ended = true;
                        }
                    }
                    TurnEvent::Ignored => {}
                }
            }
            StreamPoll::Idle => {}
            // A refused subscription is not a quiet turn: nothing will ever
            // arrive, so the run reports why rather than waiting out its
            // timeout on a stream that is an error document.
            StreamPoll::Refused(status) => {
                *submit_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!(
                    "the control server refused the event subscription ({status})"
                ));
                break;
            }
            // The server closed the stream: nothing more will arrive, so the
            // only remaining end-of-turn signal is the submitting thread's.
            StreamPoll::Closed => {
                closed_early = true;
                break;
            }
        }
    }
    let _ = submitter.join();

    // A stream that ended before the turn did is not a turn that ended: the
    // server stopped talking mid-flight. Reported rather than passed off as a
    // clean finish, which would hand a supervisor an `ok` for work that was
    // cut short — and, unlike a timeout or a refusal, leaves nothing else in
    // the envelope to notice it by.
    let error = submit_error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .or_else(|| {
            (closed_early && !ended).then(|| {
                "the control server closed the event stream before the turn ended".to_string()
            })
        });
    let status = if timed_out {
        Status::Timeout
    } else if error.is_some() {
        Status::Nonzero
    } else {
        Status::Ok
    };
    let text = text
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .trim()
        .to_string();
    let transcript = transcript
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .join("\n");
    TurnOutcome {
        status,
        text: (!text.is_empty()).then_some(text),
        transcript,
        error,
        started_at,
        finished_at: Some(utc_now()),
        duration_ms: Some(started.elapsed().as_millis()),
    }
}

/// Answer one permission ask. A failure to answer is warned about rather than
/// fatal: the turn then stalls until its timeout, which the envelope reports —
/// and a panic here would take the run's results with it.
fn answer(turn: &HttpTurn, ask: &PermissionAsk, decision: PermissionDecision) {
    let Some(request) = http::permission_reply_request(&turn.address, ask, decision) else {
        // The ask belongs to another session on this shared server, so it is
        // that run's decision to make. Said out loud because it is otherwise
        // indistinguishable from a turn the server simply never asked about.
        eprintln!(
            "oneharness: warning: ignored a permission request for another session ({})",
            ask.session
                .as_ref()
                .map_or("unnamed", |session| session.as_str())
        );
        return;
    };
    match turn.client.send(&request) {
        Ok(response) if response.ok() => {}
        Ok(response) => eprintln!(
            "oneharness: warning: the control server refused a permission answer ({}): {}",
            response.status,
            response.body.trim()
        ),
        Err(err) => eprintln!(
            "oneharness: warning: could not answer the control server's permission request: {err}"
        ),
    }
}

/// A control-server request whose failure means the turn cannot start, so it is
/// an error rather than a warning.
fn expect_ok(
    client: &HttpClient,
    request: &crate::domain::http::HttpRequest,
    what: &str,
) -> io::Result<crate::io::http::HttpResponse> {
    let response = client.send(request)?;
    if response.ok() {
        Ok(response)
    } else {
        Err(io::Error::other(format!(
            "could not {what} ({} {} answered {}): {}",
            request.method().as_str(),
            request.path(),
            response.status,
            response.body.trim()
        )))
    }
}

/// Wait until the server at `address` answers, or say it never did.
///
/// A launched server is not a reachable one: opencode takes seconds to bind,
/// and crush's socket file appears before it accepts. Every request after this
/// would otherwise fail as "connection refused" and read like a broken
/// mechanism rather than a slow start.
pub fn await_ready(shape: HttpShape, address: &ServerAddress, within: Duration) -> io::Result<()> {
    let client = HttpClient::new(address.clone(), Duration::from_secs(5));
    let request = http::readiness_request(shape);
    let deadline = Instant::now() + within;
    let mut last = String::from("it never accepted a connection");
    while Instant::now() < deadline {
        match client.send(&request) {
            // Any HTTP answer proves it is listening; a 404 from a server that
            // moved this route still means "up".
            Ok(_) => return Ok(()),
            Err(err) => last = err.to_string(),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "the control server did not answer within {}s: {last}",
            within.as_secs()
        ),
    ))
}

/// A client identity for a server that requires one (crush).
///
/// Derived from `material` (the caller's own key — the harness id today) and
/// this dispatch's pid rather than randomly, so it is reproducible within a run
/// and distinct across concurrent ones.
#[must_use]
pub fn client_id(material: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in material.bytes().chain(std::process::id().to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // The shape crush accepts: a UUID-looking, hyphenated hex identity.
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        hash >> 32,
        (hash >> 16) & 0xffff,
        hash & 0xfff,
        (hash >> 12) & 0xfff,
        hash & 0xffff_ffff_ffff
    )
}

/// Whether `id` is one this driver would accept back from a server, for callers
/// validating a resumed session token.
#[must_use]
pub fn is_usable_id(id: &str) -> bool {
    ResourceId::new(id).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_id_is_stable_within_a_run_and_shaped_like_a_uuid() {
        let first = client_id("triage");
        assert_eq!(first, client_id("triage"));
        assert_ne!(first, client_id("other"));
        let parts: Vec<&str> = first.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "{first}"
        );
        assert!(first.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn an_id_that_could_retarget_a_request_is_not_usable() {
        assert!(is_usable_id("ses_01d2"));
        assert!(!is_usable_id("../elsewhere"));
        assert!(!is_usable_id(""));
    }
}
