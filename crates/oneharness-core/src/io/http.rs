//! The socket half of an HTTP-submitted turn.
//!
//! A tiny HTTP/1.1 client rather than a dependency: `oneharness-core` depends
//! only on serde/toml/thiserror/which/wait-timeout by design (see AGENTS.md),
//! and what these two servers need is one request/response and one
//! server-sent-events stream over loopback TCP or a unix socket. All framing
//! and every route lives in [`crate::domain::http`]; this module only moves
//! bytes.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::domain::control::ServerAddress;
#[cfg(test)]
use crate::domain::http::Method;
use crate::domain::http::{parse_head, ChunkedDecoder, HttpRequest, SseAccumulator};

/// A server's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    /// Whether the server accepted the request. A control route that answers
    /// `4xx`/`5xx` has not done what was asked, however readable its body.
    #[must_use]
    pub fn ok(&self) -> bool {
        (200..400).contains(&self.status)
    }
}

/// A connection to one control server, dialed fresh per request (both servers
/// answer `Connection: close`, and a pooled socket buys nothing at this rate).
#[derive(Debug, Clone)]
pub struct HttpClient {
    address: ServerAddress,
    timeout: Duration,
}

impl HttpClient {
    #[must_use]
    pub fn new(address: ServerAddress, timeout: Duration) -> Self {
        HttpClient { address, timeout }
    }

    /// Send `request` and read the whole answer.
    ///
    /// The answer ends where its own framing says it does — the terminating
    /// chunk, or `Content-Length` bytes of body — and only falls back to reading
    /// until EOF when the server framed it neither way. Waiting for the close
    /// instead is what a `Connection: close` request seems to promise, and
    /// opencode does not keep that promise on every route: it answers in full
    /// and leaves the socket open, so a reader that waits for EOF times out and
    /// reports a complete answer as cut short. A read that fails — or a close
    /// that arrives — *before* the declared framing is satisfied really is a
    /// truncated answer, and a truncated body parsed as JSON is a wrong answer
    /// rather than a missing one.
    pub fn send(&self, request: &HttpRequest) -> io::Result<HttpResponse> {
        let mut stream = self.dial(self.timeout)?;
        write_request(&mut stream, request, &self.address)?;
        let mut pending = Vec::new();
        let mut head: Option<crate::domain::http::ResponseHead> = None;
        let mut chunks = ChunkedDecoder::default();
        let mut body: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            // Completeness is decided BEFORE the next blocking read, so a whole
            // answer never costs a read timeout.
            if let Some(head) = head {
                if head.chunked && chunks.is_complete() {
                    break;
                }
                if let Some(length) = head.content_length {
                    if !head.chunked && body.len() >= length {
                        body.truncate(length);
                        break;
                    }
                }
            }
            let read = stream.read(&mut buffer).map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!("the control server's answer was cut short: {err}"),
                )
            })?;
            if read == 0 {
                // The close. A server that framed its answer and then hung up
                // before delivering it cut the answer short just as surely as a
                // failing read did, and half a JSON document is a wrong answer
                // rather than a missing one. Only an answer the server framed
                // neither way legitimately ends here.
                if head.is_some_and(|head| head.chunked || head.content_length.is_some()) {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the control server's answer was cut short: it closed before the body \
                         it declared had arrived",
                    ));
                }
                break;
            }
            let bytes = &buffer[..read];
            match head {
                Some(head) => absorb(bytes, head.chunked, &mut chunks, &mut body),
                None => {
                    pending.extend_from_slice(bytes);
                    if let Some(parsed) = parse_head(&pending) {
                        let rest = pending.split_off(parsed.body_at);
                        absorb(&rest, parsed.chunked, &mut chunks, &mut body);
                        head = Some(parsed);
                    }
                }
            }
        }
        let Some(head) = head else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the control server answered something that is not an HTTP response",
            ));
        };
        Ok(HttpResponse {
            status: head.status,
            body: String::from_utf8_lossy(&body).to_string(),
        })
    }

    /// Open `request` as a server-sent-events stream, read one payload at a
    /// time. The stream's own timeout bounds a server that goes quiet.
    pub fn open_stream(&self, request: &HttpRequest, timeout: Duration) -> io::Result<EventStream> {
        let mut stream = self.dial(timeout)?;
        write_request(&mut stream, request, &self.address)?;
        Ok(EventStream {
            stream,
            status: None,
            head_seen: false,
            chunked: false,
            pending: Vec::new(),
            chunks: ChunkedDecoder::default(),
            sse: SseAccumulator::default(),
            ready: Vec::new(),
        })
    }

    fn dial(&self, timeout: Duration) -> io::Result<Socket> {
        match &self.address {
            ServerAddress::Tcp { port } => {
                let stream = TcpStream::connect(("127.0.0.1", port.get()))?;
                stream.set_read_timeout(Some(timeout))?;
                stream.set_write_timeout(Some(timeout))?;
                Ok(Socket::Tcp(stream))
            }
            #[cfg(unix)]
            ServerAddress::UnixSocket { path } => {
                let stream = std::os::unix::net::UnixStream::connect(path.as_path())?;
                stream.set_read_timeout(Some(timeout))?;
                stream.set_write_timeout(Some(timeout))?;
                Ok(Socket::Unix(stream))
            }
            #[cfg(not(unix))]
            ServerAddress::UnixSocket { .. } => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unix-socket control servers are not available on this platform",
            )),
            ServerAddress::Stdio => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a stdio server has no address to dial",
            )),
        }
    }
}

/// Add `bytes` to a response body, de-chunking on the way in when the server
/// framed it that way. Shared by the one-shot reader and nothing else; the
/// event stream keeps its own accumulator because it also splits SSE payloads.
fn absorb(bytes: &[u8], chunked: bool, chunks: &mut ChunkedDecoder, body: &mut Vec<u8>) {
    if chunked {
        let decoded = chunks.feed(bytes);
        body.extend_from_slice(&decoded);
    } else {
        body.extend_from_slice(bytes);
    }
}

/// A live event stream, yielding one `data:` payload at a time.
pub struct EventStream {
    stream: Socket,
    /// The status the stream answered with, once its head arrived. A server
    /// that refused the subscription (`404`, `401`) still sends a body, and
    /// reading that body as events would report a turn that never streamed.
    status: Option<u16>,
    head_seen: bool,
    chunked: bool,
    pending: Vec<u8>,
    chunks: ChunkedDecoder,
    sse: SseAccumulator,
    ready: Vec<String>,
}

/// What one poll of an event stream found.
///
/// Quiet and closed are deliberately different answers: a turn that is thinking
/// produces no events for many seconds, and treating that as the end of the
/// stream would end the run while the agent was still working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamPoll {
    Event(String),
    /// Nothing arrived within the read timeout; the stream is still open.
    Idle,
    /// The server answered the subscription with a non-success status, so its
    /// body is an error document rather than a stream of events.
    Refused(u16),
    /// The server closed the stream.
    Closed,
}

impl EventStream {
    /// The next event payload, or why there is none yet.
    pub fn poll(&mut self) -> StreamPoll {
        loop {
            if !self.ready.is_empty() {
                return StreamPoll::Event(self.ready.remove(0));
            }
            let mut buffer = [0u8; 8192];
            let read = match self.stream.read(&mut buffer) {
                Ok(read) => read,
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return StreamPoll::Idle
                }
                Err(_) => return StreamPoll::Closed,
            };
            if read == 0 {
                return StreamPoll::Closed;
            }
            let bytes = &buffer[..read];
            if !self.head_seen {
                self.pending.extend_from_slice(bytes);
                let Some(head) = parse_head(&self.pending) else {
                    continue;
                };
                self.head_seen = true;
                self.status = Some(head.status);
                if !(200..300).contains(&head.status) {
                    return StreamPoll::Refused(head.status);
                }
                self.chunked = head.chunked;
                let body: Vec<u8> = self.pending.split_off(head.body_at);
                self.pending.clear();
                self.absorb(&body);
                continue;
            }
            self.absorb(bytes);
        }
    }

    fn absorb(&mut self, bytes: &[u8]) {
        let decoded = if self.chunked {
            self.chunks.feed(bytes)
        } else {
            bytes.to_vec()
        };
        self.ready.extend(self.sse.feed(&decoded));
    }
}

/// The two socket families a control server binds. An enum rather than a boxed
/// trait object so the read timeout each one needs stays on the concrete type.
enum Socket {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Socket::Tcp(stream) => stream.read(buf),
            #[cfg(unix)]
            Socket::Unix(stream) => stream.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Socket::Tcp(stream) => stream.write(buf),
            #[cfg(unix)]
            Socket::Unix(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Socket::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Socket::Unix(stream) => stream.flush(),
        }
    }
}

fn write_request(
    stream: &mut Socket,
    request: &HttpRequest,
    address: &ServerAddress,
) -> io::Result<()> {
    let host = match address {
        ServerAddress::Tcp { port } => format!("127.0.0.1:{}", port.get()),
        _ => "localhost".to_string(),
    };
    let body = request.body().unwrap_or_default().to_string();
    let head = format!(
        "{} {} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream, application/json\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request.method().as_str(),
        request.path(),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::control::Port;
    use std::io::BufRead;
    use std::net::TcpListener;

    /// A one-connection HTTP server that answers `answer` verbatim, so the
    /// client is exercised against real socket reads rather than a fake.
    fn serve_once(answer: &'static [u8]) -> (Port, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = Port::new(listener.local_addr().unwrap().port()).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = io::BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    // A declared length this double cannot read is a framing
                    // error, not an empty body: reading none of a body that is
                    // there would hand the test a truncated request.
                    length = value
                        .trim()
                        .parse()
                        .unwrap_or_else(|_| panic!("unreadable Content-Length: {value}"));
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            request.push_str(&String::from_utf8_lossy(&body));
            socket.write_all(answer).unwrap();
            socket.flush().unwrap();
            request
        });
        (port, handle)
    }

    fn client(port: Port) -> HttpClient {
        HttpClient::new(ServerAddress::Tcp { port }, Duration::from_secs(5))
    }

    #[test]
    fn a_chunked_answer_reaches_the_caller_as_its_body() {
        // The framing both control servers use: without de-chunking this is a
        // JSON decode error on an answer the server got right.
        let (port, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n{\"id\":\"ses_01\"}\r\n\r\n0\r\n\r\n",
        );
        let response = client(port)
            .send(&HttpRequest::for_test(
                Method::Post,
                "/api/session",
                Some("{}"),
            ))
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body.trim(), r#"{"id":"ses_01"}"#);
        assert!(response.ok());

        // The request the server actually received: method, path and body.
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /api/session HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("Content-Length: 2\r\n"), "{request}");
        assert!(request.ends_with("{}"), "{request}");
    }

    /// A server that answers in full and then *keeps the connection open*,
    /// ignoring the `Connection: close` the request asked for — which is what
    /// `opencode serve` does on the readiness route. It holds the socket until
    /// the CLIENT closes it, so a reader waiting for EOF waits out its whole
    /// timeout and cannot pass by luck.
    fn serve_once_without_closing(answer: &'static [u8]) -> (Port, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = Port::new(listener.local_addr().unwrap().port()).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = io::BufReader::new(socket.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            socket.write_all(answer).unwrap();
            socket.flush().unwrap();
            // Never closes; only the client hanging up releases this.
            let mut drain = Vec::new();
            let _ = reader.read_to_end(&mut drain);
        });
        (port, handle)
    }

    #[test]
    fn a_complete_answer_is_returned_without_waiting_for_the_server_to_close() {
        // Content-Length framing: the whole body is here, so the answer is
        // whole — whatever the server then does with the socket.
        let (port, server) = serve_once_without_closing(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"id\":\"ses_01\"}",
        );
        let started = std::time::Instant::now();
        let response = client(port)
            .send(&HttpRequest::for_test(Method::Get, "/api/app", None))
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"id":"ses_01"}"#);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the answer was framed as complete, so reading it must not cost a read timeout"
        );
        server.join().unwrap();

        // The same for chunked framing, which ends at its terminating chunk.
        let (port, server) = serve_once_without_closing(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n",
        );
        let started = std::time::Instant::now();
        let response = client(port)
            .send(&HttpRequest::for_test(Method::Post, "/api/session", None))
            .unwrap();
        assert_eq!(response.body, r#"{"a":1}"#);
        assert!(started.elapsed() < Duration::from_secs(5));
        server.join().unwrap();
    }

    #[test]
    fn a_refusal_is_data_the_caller_can_act_on_rather_than_an_error() {
        let (port, server) = serve_once(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 21\r\n\r\n404 page not found\r\n\r\n",
        );
        let response = client(port)
            .send(&HttpRequest::for_test(
                Method::Post,
                "/v1/workspaces/x/agent/sessions",
                None,
            ))
            .unwrap();
        assert_eq!(response.status, 404);
        assert!(!response.ok());
        assert!(response.body.contains("404 page not found"));
        let _ = server.join();
    }

    #[test]
    fn an_answer_the_server_closed_before_finishing_is_an_error_rather_than_half_a_body() {
        // A control server that dies mid-answer (or a proxy that drops it)
        // leaves a body its own head says is incomplete. Returning it would
        // hand the caller half a JSON document to parse — a wrong answer,
        // where the error is a missing one the caller can retry.
        let (port, server) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"id\":\"ses");
        let err = client(port)
            .send(&HttpRequest::for_test(Method::Get, "/api/app", None))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        assert!(err.to_string().contains("cut short"), "{err}");
        let _ = server.join();

        // The same for chunked framing, whose terminating chunk never arrives.
        let (port, server) =
            serve_once(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n");
        let err = client(port)
            .send(&HttpRequest::for_test(Method::Post, "/api/session", None))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let _ = server.join();

        // An answer the server framed NEITHER way still ends at the close:
        // that is the one shape whose only end-of-body signal is the close.
        let (port, server) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}");
        let response = client(port)
            .send(&HttpRequest::for_test(Method::Get, "/api/app", None))
            .unwrap();
        assert_eq!(response.body, r#"{"a":1}"#);
        let _ = server.join();
    }

    #[test]
    fn an_answer_that_is_not_http_is_an_error_rather_than_an_empty_body() {
        let (port, server) = serve_once(b"this is not a response");
        let err = client(port)
            .send(&HttpRequest::for_test(Method::Get, "/api/app", None))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let _ = server.join();
    }

    #[test]
    fn an_event_stream_yields_each_payload_as_it_arrives_and_ends_at_close() {
        let (port, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n\
              1f\r\ndata: {\"type\":\"session.idle\"}\n\r\n\
              13\r\ndata: {\"a\":true}\n\r\n0\r\n\r\n",
        );
        let mut stream = client(port)
            .open_stream(
                &HttpRequest::for_test(Method::Get, "/api/event", None),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            stream.poll(),
            StreamPoll::Event(r#"{"type":"session.idle"}"#.to_string())
        );
        assert_eq!(
            stream.poll(),
            StreamPoll::Event(r#"{"a":true}"#.to_string())
        );
        assert_eq!(stream.poll(), StreamPoll::Closed);
        let _ = server.join();
    }

    #[test]
    fn a_stdio_server_has_no_address_to_dial() {
        // The pairing is a type error waiting to happen otherwise: a stdio
        // mechanism's "address" is its pipes, which no HTTP client can reach.
        let err = HttpClient::new(ServerAddress::Stdio, Duration::from_secs(1))
            .send(&HttpRequest::for_test(Method::Get, "/", None))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
