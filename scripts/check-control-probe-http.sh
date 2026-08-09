#!/usr/bin/env bash
# Boundary self-test for scripts/explore_control.py's HTTP reader.
#
# The probe is what sources a `ControlShape` from real behavior, so its verdicts
# become declared capabilities. That makes its reader a trust boundary: an
# answer it accepts as complete when it is not — a read that timed out, a first
# line that is not a status line, a head that merely carries the word `chunked`
# — turns into a LIVE/REFUTED claim about bytes it never finished reading.
#
# Driven against real sockets rather than a stub, because the failure modes are
# arrival-shaped: bytes that stop coming, and framing that never terminates.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe="$root/scripts/explore_control.py"

if ! command -v python3 >/dev/null 2>&1; then
    echo "check-control-probe-http: skipped (python3 is not installed; the probe it checks needs it)"
    exit 0
fi

python3 - "$probe" <<'PY'
import importlib.util
import io
import socket
import sys
import threading

probe_path = sys.argv[1]
spec = importlib.util.spec_from_file_location("explore_control", probe_path)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

failures = []


def check(name, ok, detail=""):
    if not ok:
        failures.append(f"{name}: {detail}")


def serve_once(answer: bytes, hold: bool = False):
    """A one-connection server answering `answer` verbatim.

    `hold` keeps the socket open afterwards, which is how a real server that
    ignores `Connection: close` (opencode does, on some routes) looks to a
    reader waiting for an ending.
    """
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = listener.getsockname()[1]

    def serve():
        conn, _ = listener.accept()
        try:
            conn.recv(65536)
            conn.sendall(answer)
            if hold:
                # Never closes; only the client hanging up releases this.
                try:
                    conn.settimeout(20)
                    while conn.recv(65536):
                        pass
                except OSError:
                    pass
        finally:
            conn.close()
            listener.close()

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    return ("127.0.0.1", port), thread


def request(answer, hold=False, timeout=3.0):
    address, thread = serve_once(answer, hold)
    try:
        return probe.http_request(address, None, "GET", "/api/app", timeout=timeout)
    finally:
        thread.join(timeout=5)


# Each case below catches broadly on purpose: the reader under test is allowed
# to refuse in more than one way, so an unexpected exception type is a result to
# report through `check` rather than a traceback that ends the sweep and hides
# every case after it.
#
# A whole, well-framed answer still reads — the positive control, without which
# every refusal below could hold for the wrong reason.
try:
    status, body = request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"id\":\"ses_01\"}", hold=True
    )
    check("framed answer", (status, body) == (200, '{"id":"ses_01"}'), f"{status} {body!r}")
except Exception as err:
    check("framed answer", False, f"{type(err).__name__}: {err}")

# So does a chunked one, which is what crush answers with.
try:
    status, body = request(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n",
        hold=True,
    )
    check("chunked answer", (status, body) == (200, '{"a":1}'), f"{status} {body!r}")
except Exception as err:
    check("chunked answer", False, f"{type(err).__name__}: {err}")

# A body cut short of its own declaration is refused, not returned as half a
# document for `json.loads` to make a verdict out of.
try:
    request(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"id\":\"ses")
    check("truncated body", False, "a short body was accepted as whole")
except ValueError:
    pass
except Exception as err:
    check("truncated body", False, f"{type(err).__name__}: {err}")

# A read that never completes is a timeout, not an answer. Held open with no
# ending in sight, this is the shape a wedged server takes.
try:
    request(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n{}", hold=True, timeout=1.0)
    check("timed-out read", False, "a timed-out read was accepted as an answer")
except socket.timeout:
    pass
except Exception as err:
    check("timed-out read", False, f"{type(err).__name__}: {err}")

# A first line that is not a status line is not a `0` status either: promoting
# it would let something that is not a response decide the verdict.
for label, answer in [
    ("not http", b"NOT-HTTP 200 fine\r\nContent-Length: 0\r\n\r\n"),
    ("no status", b"HTTP/1.1\r\nContent-Length: 0\r\n\r\n"),
    ("short code", b"HTTP/1.1 20 OK\r\nContent-Length: 0\r\n\r\n"),
]:
    try:
        request(answer)
        check(label, False, "a non-status line was read as a status")
    except ValueError:
        pass
    except Exception as err:
        check(label, False, f"{type(err).__name__}: {err}")

# A head that merely CARRIES the word must not frame the body as chunked: the
# de-chunker would then eat the answer and hand back whatever survived.
try:
    status, body = request(
        b"HTTP/1.1 200 OK\r\nX-Upstream: transfer-encoding: chunked\r\n"
        b"Content-Length: 7\r\n\r\n{\"a\":1}",
        hold=True,
    )
    check("chunked lookalike", (status, body) == (200, '{"a":1}'), f"{status} {body!r}")
except Exception as err:
    check("chunked lookalike", False, f"{type(err).__name__}: {err}")

# Two simultaneous body framings are ambiguous external input, even when both
# happen to describe the bytes in this fixture consistently.
try:
    request(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 7\r\n\r\n"
        b"7\r\n{\"a\":1}\r\n0\r\n\r\n"
    )
    check("ambiguous framing", False, "chunked plus Content-Length was accepted")
except ValueError:
    pass
except Exception as err:
    check("ambiguous framing", False, f"{type(err).__name__}: {err}")

# Chunked framing that never terminates is refused rather than handed back as
# whatever chunks happened to arrive.
try:
    request(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n")
    check("unterminated chunks", False, "an unterminated chunked body was accepted")
except ValueError:
    pass
except Exception as err:
    check("unterminated chunks", False, f"{type(err).__name__}: {err}")

# A JSON-RPC frame off a child's stdout is bounded the same way: the newline
# that ends one is the child's to send, so a line with none in it must not be
# held. The over-long frame is dropped whole and the reader resyncs, rather than
# handing half a document to `json.loads`.
limit = 64
stream = io.StringIO('{"a":1}\n' + "x" * (limit * 3) + '\n{"b":2}\n')
frames = list(probe.bounded_lines(stream, limit))
check("bounded frames", frames == ['{"a":1}\n', '{"b":2}\n'], f"{frames!r}")
# A last line with no newline is still a frame, as long as it is within bounds.
check(
    "unterminated tail",
    list(probe.bounded_lines(io.StringIO('{"a":1}'), limit)) == ['{"a":1}'],
)

if failures:
    for failure in failures:
        print(f"check-control-probe-http: {failure}", file=sys.stderr)
    print(
        "check-control-probe-http: each line above is <check>: <what the reader did>. "
        "Fix the boundary in scripts/explore_control.py — http_request (how much it "
        "reads and when it stops), parse_response_head (status line, Content-Length, "
        "Transfer-Encoding), dechunk (whether the body terminated), or bounded_lines "
        "(per-frame ceiling) — then rerun: bash scripts/check-control-probe-http.sh",
        file=sys.stderr,
    )
    sys.exit(1)
PY

echo "check-control-probe-http: ok"
