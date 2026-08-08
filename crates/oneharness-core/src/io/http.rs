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
    pub fn send(&self, request: &HttpRequest) -> io::Result<HttpResponse> {
        let mut stream = self.dial(self.timeout)?;
        write_request(&mut stream, request, &self.address)?;
        let mut raw = Vec::new();
        // The server closes the connection at the end of the body, so a read to
        // EOF is the whole answer; a timeout keeps a wedged server from
        // hanging the run instead of failing it.
        let _ = stream.read_to_end(&mut raw);
        let Some(head) = parse_head(&raw) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the control server answered something that is not an HTTP response",
            ));
        };
        let body = &raw[head.body_at..];
        let body = if head.chunked {
            ChunkedDecoder::default().feed(body)
        } else {
            body.to_vec()
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

/// A live event stream, yielding one `data:` payload at a time.
pub struct EventStream {
    stream: Socket,
    head_seen: bool,
    chunked: bool,
    pending: Vec<u8>,
    chunks: ChunkedDecoder,
    sse: SseAccumulator,
    ready: Vec<String>,
}

impl EventStream {
    /// The next event payload, or `None` once the server closed the stream (or
    /// went quiet past the timeout, which for a control stream is the same
    /// thing: there is nothing more to read).
    pub fn next_event(&mut self) -> Option<String> {
        loop {
            if !self.ready.is_empty() {
                return Some(self.ready.remove(0));
            }
            let mut buffer = [0u8; 8192];
            let read = self.stream.read(&mut buffer).ok()?;
            if read == 0 {
                return None;
            }
            let bytes = &buffer[..read];
            if !self.head_seen {
                self.pending.extend_from_slice(bytes);
                let Some(head) = parse_head(&self.pending) else {
                    continue;
                };
                self.head_seen = true;
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
    let body = request.body.clone().unwrap_or_default();
    let head = format!(
        "{} {} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream, application/json\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request.method,
        request.path,
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
                    length = value.trim().parse().unwrap_or(0);
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
            .send(&HttpRequest {
                method: "POST",
                path: "/api/session".to_string(),
                body: Some("{}".to_string()),
            })
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

    #[test]
    fn a_refusal_is_data_the_caller_can_act_on_rather_than_an_error() {
        let (port, server) = serve_once(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 21\r\n\r\n404 page not found\r\n\r\n",
        );
        let response = client(port)
            .send(&HttpRequest {
                method: "POST",
                path: "/v1/workspaces/x/agent/sessions".to_string(),
                body: None,
            })
            .unwrap();
        assert_eq!(response.status, 404);
        assert!(!response.ok());
        assert!(response.body.contains("404 page not found"));
        let _ = server.join();
    }

    #[test]
    fn an_answer_that_is_not_http_is_an_error_rather_than_an_empty_body() {
        let (port, server) = serve_once(b"this is not a response");
        let err = client(port)
            .send(&HttpRequest {
                method: "GET",
                path: "/api/app".to_string(),
                body: None,
            })
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
                &HttpRequest {
                    method: "GET",
                    path: "/api/event".to_string(),
                    body: None,
                },
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            stream.next_event().as_deref(),
            Some(r#"{"type":"session.idle"}"#)
        );
        assert_eq!(stream.next_event().as_deref(), Some(r#"{"a":true}"#));
        assert_eq!(stream.next_event(), None);
        let _ = server.join();
    }

    #[test]
    fn a_stdio_server_has_no_address_to_dial() {
        // The pairing is a type error waiting to happen otherwise: a stdio
        // mechanism's "address" is its pipes, which no HTTP client can reach.
        let err = HttpClient::new(ServerAddress::Stdio, Duration::from_secs(1))
            .send(&HttpRequest {
                method: "GET",
                path: "/".to_string(),
                body: None,
            })
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
