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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::domain::control::{AbsolutePath, DialAddress, RedirectInput};
use crate::domain::http::{
    self, ClientId, HttpShape, OpencodeModel, PermissionAsk, PermissionDecision, ResourceId,
    TurnAddress, TurnEvent, TurnOpening,
};
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
    #[must_use]
    pub fn shape(&self) -> HttpShape {
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

/// How a turn ended — the one decision `status` and `error` are both read off.
///
/// A sum type rather than the pair, because the pair has states that are not
/// outcomes: an `ok` carrying a failure reason, or a failure carrying none. A
/// timeout keeps whatever was noticed on the way, since its status is
/// authoritative but a reason is still worth reporting.
#[derive(Debug, Clone)]
enum TurnEnd {
    /// The turn ran to its own end.
    Completed,
    /// It outlasted the budget its caller set.
    TimedOut(Option<String>),
    /// Nothing ran: the server, the session or the event subscription could not
    /// be opened.
    CouldNotStart(String),
    /// The turn began and did not get through: the server refused to carry it,
    /// stopped talking mid-flight, or framed a stream the client cannot read.
    /// Named for that outcome rather than for a refusal, which is only one of
    /// the ways to reach it.
    DidNotFinish(String),
}

impl TurnEnd {
    fn status(&self) -> Status {
        match self {
            TurnEnd::Completed => Status::Ok,
            TurnEnd::TimedOut(_) => Status::Timeout,
            TurnEnd::CouldNotStart(_) => Status::SpawnError,
            TurnEnd::DidNotFinish(_) => Status::Nonzero,
        }
    }

    fn error(&self) -> Option<&str> {
        match self {
            TurnEnd::Completed => None,
            TurnEnd::TimedOut(why) => why.as_deref(),
            TurnEnd::CouldNotStart(why) | TurnEnd::DidNotFinish(why) => Some(why),
        }
    }
}

/// What one HTTP-submitted turn produced.
///
/// Built only through the constructors below, which take the ending as one
/// value and stamp the whole timing themselves. A turn is only described once
/// it is over, so "finished at nothing, for 40ms" has nowhere to come from
/// either.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    end: TurnEnd,
    /// The assistant's answer, when the stream carried one. Never fabricated,
    /// and never the empty string standing in for one.
    text: Option<String>,
    /// Every event payload observed, newline-joined — the run's `stdout`, so a
    /// consumer needing certainty can parse exactly what oneharness saw.
    transcript: String,
    /// The invocation boundaries this turn was observed between, in the same
    /// shape a spawned run reports them — and in the same type, so text that is
    /// not a millisecond-precision UTC instant cannot reach a measurement.
    started_at: RunInstant,
    finished_at: RunInstant,
    duration_ms: u128,
}

impl TurnOutcome {
    /// A finished turn: how it ended, what it said, and what was seen on the
    /// wire. The end is one value, so the status and the reason are two views
    /// of the same decision rather than two fields that can disagree.
    fn ended(
        end: TurnEnd,
        text: &str,
        transcript: String,
        started_at: RunInstant,
        elapsed: Duration,
    ) -> Self {
        let text = text.trim();
        TurnOutcome {
            end,
            text: (!text.is_empty()).then(|| text.to_string()),
            transcript,
            started_at,
            finished_at: utc_now(),
            duration_ms: elapsed.as_millis(),
        }
    }

    /// The assistant's answer, when the turn produced one.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// The execution envelope for a turn that had no subprocess of its own.
    ///
    /// The event transcript stands in for `stdout`: there was no child to
    /// capture, and a consumer needing certainty should still be able to parse
    /// exactly what oneharness saw on the wire.
    #[must_use]
    pub fn to_capture(&self) -> Capture {
        Capture {
            status: self.end.status(),
            exit_code: None,
            duration_ms: Some(self.duration_ms),
            stdout: self.transcript.clone(),
            stderr: String::new(),
            error: self.end.error().map(str::to_string),
            started_at: self.started_at.as_str().to_string(),
            finished_at: Some(self.finished_at.as_str().to_string()),
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
            end: TurnEnd::CouldNotStart(error),
            text: None,
            transcript: String::new(),
            started_at: now.clone(),
            finished_at: now,
            duration_ms: 0,
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
///
/// `model` is opencode's alone: it names the model the SESSION runs on, which
/// is where a controlled opencode turn's model has to be said (see
/// [`OpencodeModel`]). Crush takes its own from the server it was started with.
pub fn open(
    shape: HttpShape,
    server: DialAddress,
    cwd: &AbsolutePath,
    decision: PermissionDecision,
    client_id: &str,
    model: Option<&OpencodeModel>,
) -> io::Result<HttpTurn> {
    let client = HttpClient::new(server, REQUEST_TIMEOUT);

    // One arm per protocol, because the coordinates a turn is addressed by are
    // per protocol: opencode's open request already IS its session, while
    // crush's is a workspace the session then hangs off, named by a client
    // identity opencode has no notion of.
    let address = match shape {
        HttpShape::Opencode => TurnAddress::Opencode {
            session: created_id(
                &expect_ok(
                    &client,
                    &http::open_request(&TurnOpening::Opencode { cwd, model }),
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
                &http::open_request(&TurnOpening::Crush {
                    cwd,
                    client: &identity,
                    decision,
                }),
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
///
/// `take_redirect` is asked once each time a turn ends, and hands over the
/// message an interrupt committed. Submitting it *here* rather than from the
/// socket thread is what makes the redirection atomic: both servers queue a
/// prompt behind the turn they are still running, so a message sent alongside
/// the abort would land in the turn being cancelled. The interrupt takes
/// ownership of it instead, and this loop delivers it to a session that is
/// demonstrably idle.
pub fn run(
    turn: &HttpTurn,
    prompt: &str,
    decision: PermissionDecision,
    timeout: Duration,
    take_redirect: &dyn Fn() -> Option<RedirectInput>,
) -> TurnOutcome {
    let started = Instant::now();
    let started_at = utc_now();
    let deadline = started + timeout;
    let transcript = Arc::new(Mutex::new(Vec::<String>::new()));
    let text = Arc::new(Mutex::new(String::new()));
    // llmlint: ignore[names_match_behavior] Accurate finding, deferred by the planner rather than fixed here: this flag is raised only when the submitter gives up (the prompt was refused or could not be sent), never when the turn ends — that is `ended`, off `TurnEvent::Finished`. Renaming it touches four sites in a path this change does not otherwise enter, and this branch is closing two named cross-platform test failures.
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
            return TurnOutcome::ended(
                TurnEnd::CouldNotStart(format!(
                    "could not follow the control server's events: {err}"
                )),
                "",
                String::new(),
                started_at,
                started.elapsed(),
            )
        }
    };

    // Which submission a recorded refusal belongs to, and which one is current.
    //
    // Numbered rather than a single slot, because interrupting a turn makes its
    // own prompt request fail: opencode holds that request open for the whole
    // turn and answers it with a refusal when the turn is aborted. That refusal
    // describes the turn the supervisor deliberately stopped — treating it as
    // the run's outcome ends the run before the redirection it committed can
    // become the next turn, which is the message being lost by another route.
    let gave_up = Arc::new(Mutex::new(None::<(usize, String)>));
    let current = Arc::new(AtomicUsize::new(0));
    // Every prompt this turn submits — its own, and any redirection an
    // interrupt hands over — goes out on its own thread. Opencode holds the
    // prompt request open for the whole turn, so submitting from the reader
    // would stop it following the very stream that says when to stop.
    let spawn_submit = |prompt: String| {
        let turn = turn.clone();
        let finished = Arc::clone(&finished);
        let gave_up = Arc::clone(&gave_up);
        let mine = current.load(Ordering::SeqCst);
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
                *gave_up.lock().unwrap_or_else(|e| e.into_inner()) = Some((mine, refusal));
                finished.store(true, Ordering::SeqCst);
            }
        })
    };
    // The refusal that belongs to the submission still in flight, if any.
    let current_failure = || {
        gave_up
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|(who, _)| *who == current.load(Ordering::SeqCst))
            .map(|(_, why)| why.clone())
    };
    let mut submitters = vec![spawn_submit(prompt.to_string())];

    let mut timed_out = false;
    // Whether the stream ended before the turn did. Buffered events are handed
    // over before a close is ever reported, so this is only true of a server
    // that really did stop mid-turn.
    let mut closed_early = false;
    // The turn is only over once it has begun: see `TurnEvent::Started`.
    let mut in_flight = false;
    let mut ended = false;
    // A failure of the subscription itself rather than of any one submission.
    let mut stream_error: Option<String> = None;
    // Take over a committed redirection and make it the next turn. `true` when
    // one was submitted, which is the loop's signal to keep reading rather than
    // end. Both callers reach this from a *turn* ending — the harness saying so
    // on the stream, or its prompt request coming back — because those are the
    // two ways this run learns the session will accept a prompt again.
    let redirect_next = |submitters: &mut Vec<std::thread::JoinHandle<()>>| {
        let Some(redirect) = take_redirect() else {
            return false;
        };
        // The new submission is current, so the aborted one's refusal is no
        // longer the run's outcome — and a `finished` it raised is no longer
        // this run's reason to stop.
        current.fetch_add(1, Ordering::SeqCst);
        finished.store(false, Ordering::SeqCst);
        submitters.push(spawn_submit(redirect.as_str().to_string()));
        true
    };

    // Opencode's aborted turn ends with NO event at all — the stream simply
    // stops — so a redirection waiting for one would never be delivered and the
    // run would sit out its whole timeout. There the served interrupt is the
    // ending, and the message becomes deliverable the moment that abort lands,
    // which is what this poll picks up. Crush announces its cancellation, so
    // nothing is deliverable early and this never fires for it.
    let abort_is_silent = turn.shape().abort_ends_turn_silently();
    while !ended {
        if abort_is_silent {
            redirect_next(&mut submitters);
        }
        if finished.load(Ordering::SeqCst) {
            // A submission gave up. When that was the turn an interrupt
            // aborted, its redirection is what happens next; otherwise there is
            // genuinely nothing more to follow.
            if !redirect_next(&mut submitters) {
                break;
            }
            in_flight = false;
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
                            // The turn is over, so the session will accept a
                            // prompt again — the first moment a redirection an
                            // interrupt committed can actually become a turn.
                            // Until its own `Started` arrives, the same idle
                            // this arm read must not end the run again.
                            if redirect_next(&mut submitters) {
                                in_flight = false;
                            } else {
                                ended = true;
                            }
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
                stream_error = Some(format!(
                    "the control server refused the event subscription ({status})"
                ));
                break;
            }
            // Nor is a head the client cannot frame a body from: no payload
            // will ever be read off this stream, so the run says why instead of
            // waiting out its timeout on bytes it refuses to interpret.
            StreamPoll::Unreadable(why) => {
                stream_error = Some(format!(
                    "the control server's event subscription cannot be read: {why}"
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
    // A peer can hold a prompt request open longer than the run's budget. Once
    // that budget expires, joining the request threads would turn a timeout into
    // an additional REQUEST_TIMEOUT wait. Dropping the handles detaches the
    // workers; releasing the dispatch's server lease tears down its socket.
    if !timed_out {
        for submitter in submitters {
            let _ = submitter.join();
        }
    }

    // A stream that ended before the turn did is not a turn that ended: the
    // server stopped talking mid-flight. Reported rather than passed off as a
    // clean finish, which would hand a supervisor an `ok` for work that was
    // cut short — and, unlike a timeout or a refusal, leaves nothing else in
    // the envelope to notice it by.
    let error = stream_error.or_else(current_failure).or_else(|| {
        (closed_early && !ended)
            .then(|| "the control server closed the event stream before the turn ended".to_string())
    });
    // A timeout's status stays authoritative — nothing else in the envelope
    // says the budget was exceeded — while still carrying whatever was noticed
    // on the way, which is the only place that reason survives.
    let end = match (timed_out, error) {
        (true, why) => TurnEnd::TimedOut(why),
        (false, Some(why)) => TurnEnd::DidNotFinish(why),
        (false, None) => TurnEnd::Completed,
    };
    let text = text.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let transcript = transcript
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .join("\n");
    TurnOutcome::ended(end, &text, transcript, started_at, started.elapsed())
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

/// Why a launched control server never became reachable.
///
/// Two outcomes rather than one, because they are two different facts and only
/// one of them is about the address. A server that is GONE is a fact about the
/// process oneharness itself started; a server that is SILENT is a verdict a
/// clock rendered at an address. Keeping them apart is what lets the caller
/// relaunch the first without ever re-rolling the second.
#[derive(Debug, Clone)]
pub enum NotReady {
    /// The process oneharness launched is no longer running: it exited before
    /// it ever answered. Worth one relaunch — a fresh address included, since
    /// losing the one it was given is among the reasons a server dies at once.
    Exited(String),
    /// It is still running and simply never answered within the window.
    Silent(String),
}

impl NotReady {
    /// Whether the launched server is gone, as opposed to merely quiet.
    #[must_use]
    pub fn exited(&self) -> bool {
        matches!(self, NotReady::Exited(_))
    }
}

impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotReady::Exited(why) | NotReady::Silent(why) => f.write_str(why),
        }
    }
}

/// Wait until the server at `address` answers, or say why it never did.
///
/// A launched server is not a reachable one: opencode takes seconds to bind,
/// and crush's socket file appears before it accepts. Every request after this
/// would otherwise fail as "connection refused" and read like a broken
/// mechanism rather than a slow start.
///
/// `still_running` is asked before every dial and again after one that
/// answered, because an address is not an identity. A TCP port is reserved by
/// binding and letting go, so between the reservation and the launch it belongs
/// to whoever asks next: a server that lost that race dies on `EADDRINUSE`, and
/// the stranger now listening there would otherwise be dialed, admitted and
/// driven as though it were ours. Asking about the process turns "it is not
/// there" into something the launch knows rather than something the window
/// guesses — and asking again after an answer keeps a stranger's `200` from
/// standing in for a server that is already gone.
/// Wait for the launched server process to become ready, distinguishing an
/// exited child from an address that merely remains silent.
pub fn await_ready(
    shape: HttpShape,
    address: &DialAddress,
    within: Duration,
    still_running: &dyn Fn() -> bool,
) -> Result<(), NotReady> {
    let client = HttpClient::new(address.clone(), Duration::from_secs(5));
    let request = http::readiness_request(shape);
    let deadline = Instant::now() + within;
    let mut last = String::from("it never accepted a connection");
    loop {
        if !still_running() {
            return Err(NotReady::Exited(format!(
                "the control server exited before it answered: {last}"
            )));
        }
        match client.send(&request) {
            // Any HTTP answer proves it is listening; a 404 from a server that
            // moved this route still means "up".
            Ok(_) if still_running() => return Ok(()),
            Ok(_) => {
                return Err(NotReady::Exited(
                    "the control server exited before it answered: something else answered at its address".to_string(),
                ))
            }
            Err(err) => last = err.to_string(),
        }
        if Instant::now() >= deadline {
            return Err(NotReady::Silent(format!(
                "the control server did not answer within {}s: {last}",
                within.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
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

    /// An address nothing can ever answer at: a unix socket path inside a
    /// directory this test alone names, which no other process can bind.
    /// Deliberately not a TCP port — one picked by binding and letting go is a
    /// port anybody may take next, and a stranger answering there is exactly
    /// what these two outcomes exist to tell apart.
    #[cfg(unix)]
    fn unserved_address(tag: &str) -> DialAddress {
        let path = std::env::temp_dir().join(format!("oh-ready-{tag}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        DialAddress::UnixSocket {
            path: crate::domain::control::AbsolutePath::new(path).expect("an absolute path"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_server_still_running_but_silent_is_reported_against_the_window() {
        let failure = await_ready(
            HttpShape::Opencode,
            &unserved_address("silent"),
            Duration::from_secs(1),
            &|| true,
        )
        .expect_err("nothing is listening there");
        assert!(!failure.exited(), "{failure}");
        assert!(
            failure.to_string().contains("did not answer within 1s"),
            "{failure}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_launched_server_that_is_gone_is_reported_without_waiting_the_window_out() {
        // A ninety-second window and no wait at all: the answer is a fact about
        // the process, so there is nothing to wait for once it is gone.
        let started = Instant::now();
        let failure = await_ready(
            HttpShape::Opencode,
            &unserved_address("gone"),
            Duration::from_secs(90),
            &|| false,
        )
        .expect_err("the server it launched is gone");
        assert!(failure.exited(), "{failure}");
        assert!(
            failure
                .to_string()
                .contains("the control server exited before it answered"),
            "{failure}"
        );
        assert!(started.elapsed() < Duration::from_secs(5), "{failure}");
    }

    #[cfg(unix)]
    #[test]
    fn an_answer_from_an_address_whose_launched_server_is_gone_is_not_readiness() {
        // Somebody is listening; it is just not the server this dispatch
        // started. Taking the `200` would submit the turn to a stranger.
        let address = unserved_address("stranger");
        let DialAddress::UnixSocket { path } = &address else {
            unreachable!("the fixture builds a unix socket address")
        };
        let listener =
            std::os::unix::net::UnixListener::bind(path.as_path()).expect("a bound fixture socket");
        let serving = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{BufRead, Write};
                // Read the request head off the wire before answering, or the
                // client's own write is what fails and no answer is ever seen.
                let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 0) {
                    if line.trim().is_empty() {
                        break;
                    }
                    line.clear();
                }
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
        // Alive for the check before the dial, gone by the one after it: the
        // order in which a server that dies mid-bring-up is really observed.
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let failure = await_ready(
            HttpShape::Opencode,
            &address,
            Duration::from_secs(90),
            &|| checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0,
        )
        .expect_err("the launched server is gone whoever answered");
        assert!(failure.exited(), "{failure}");
        assert!(
            failure.to_string().contains("something else answered"),
            "{failure}"
        );
        let _ = serving.join();
        let _ = std::fs::remove_file(path.as_path());
    }

    #[test]
    fn an_id_that_could_retarget_a_request_is_not_usable() {
        assert!(is_usable_id("ses_01d2"));
        assert!(!is_usable_id("../elsewhere"));
        assert!(!is_usable_id(""));
    }
}
