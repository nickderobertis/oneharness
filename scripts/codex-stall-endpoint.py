#!/usr/bin/env python3
"""A local OpenAI Responses endpoint that can stall one response, for real Codex.

    codex-stall-endpoint.py DIR

Serves the two transports Codex's built-in `openai` provider uses — the
Responses websocket (`GET /v1/responses`, upgraded) and, as its fallback, SSE
(`POST /v1/responses`) — on 127.0.0.1, at a port it picks and writes to
DIR/port. Every response says one fixed assistant message and completes, unless
DIR/mode reads `stall-next` when a model request arrives: that one response
sends `response.created` and then nothing, and the mode returns to serving, so
the request Codex retries once it notices the stall is answered. Each request is
appended to DIR/requests.log as `<epoch seconds> <transport> <warmup|request>
<mode> <bytes>`.

It is the fixture of `oh_resume_stall_enforce` (scripts/e2e-lib.sh), which
points an isolated Codex at it through `openai_base_url` — the built-in
provider, never a custom one — so no model call is made or billed.
"""

# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The events below are the minimum of OpenAI's Responses stream Codex needs to finish a turn (`response.created`, `response.output_item.done`, `response.completed`), and real Codex is their drift alarm: `oh_resume_stall_enforce` fails if codex-cli stops accepting them, and the deterministic gate cannot run codex.
# llmlint: ignore-file[structural_pattern_matching] The repository's Python floor is 3.9 (python/oneharness-sdk/pyproject.toml), which has no `match` statement.

from __future__ import annotations

import base64
import binascii
import enum
import hashlib
import http.server
import json
import socket
import struct
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Callable, NamedTuple, TypedDict, TypeVar

if TYPE_CHECKING:
    from typing_extensions import override
else:
    # Run by a bare interpreter (`python3 -I`), which may have no
    # typing_extensions; the decorator only informs the type checker.
    _Method = TypeVar("_Method", bound=Callable[..., object])

    def override(method: _Method) -> _Method:
        return method


ANSWER = "OH-STALL-RECOVERED"
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
# The largest request this endpoint reads. Codex's requests here are under
# 100 KB; anything past this is not one of them.
MAX_BODY = 1 << 24
# How long a stalled response is held. Longer than any bound a phase puts on a
# turn, so only Codex's own idle timeout can end it.
STALL_SECONDS = 3600


class Transport(enum.Enum):
    WEBSOCKET = "websocket"
    SSE = "sse"


class Mode(enum.Enum):
    SERVE = "serve"
    STALL = "stall"


class Opcode(enum.IntEnum):
    TEXT = 0x1
    CLOSE = 0x8
    PING = 0x9
    PONG = 0xA


@dataclass(frozen=True)
class Decision:
    number: int
    mode: Mode


class Frame(NamedTuple):
    opcode: int
    payload: bytes


class Content(TypedDict):
    type: str
    text: str


class Message(TypedDict):
    type: str
    role: str
    id: str
    content: list[Content]


class Usage(TypedDict):
    input_tokens: int
    input_tokens_details: dict[str, int]
    output_tokens: int
    output_tokens_details: dict[str, int]
    total_tokens: int


class Response(TypedDict, total=False):
    id: str
    usage: Usage


class Event(TypedDict, total=False):
    type: str
    response: Response
    output_index: int
    item: Message


class State:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.lock = threading.Lock()
        self.count = 0

    def decide(
        self, transport: Transport, request: dict[str, object], size: int
    ) -> Decision | None:
        """This request's number and whether it stalls, consuming a `stall-next`.

        A websocket warmup (`"generate": false`, which Codex sends before each
        turn's request on that transport) is always served: stalling it tests
        nothing, because Codex abandons a warmup and carries on.

        None, said on stderr, when the mode or the request log cannot be read
        or written: a request this endpoint cannot account for is not served.
        """
        warmup = request.get("generate") is False
        with self.lock:
            self.count += 1
            mode_file = self.root / "mode"
            log_file = self.root / "requests.log"
            try:
                armed = mode_file.exists() and mode_file.read_text().strip() == "stall-next"
                mode = Mode.SERVE
                if armed and not warmup:
                    mode_file.write_text("serve\n")
                    mode = Mode.STALL
                with log_file.open("a") as log:
                    kind = "warmup" if warmup else "request"
                    log.write(f"{time.time():.3f} {transport.value} {kind} {mode.value} {size}\n")
            except OSError as err:
                print(
                    f"codex-stall-endpoint: cannot use {mode_file} or {log_file} ({err}); "
                    f"check that {self.root} is writable and has space",
                    file=sys.stderr,
                )
                return None
            return Decision(self.count, mode)


def parse_request(body: bytes) -> dict[str, object] | None:
    """A Responses request: a JSON object, or None for anything else."""
    try:
        request = json.loads(body)
    except ValueError:
        return None
    if not isinstance(request, dict):
        return None
    fields: dict[str, object] = {}
    for key, value in request.items():
        fields[str(key)] = value
    return fields


def created(number: int) -> Event:
    return {"type": "response.created", "response": {"id": f"resp_{number}"}}


def completion(number: int) -> list[Event]:
    """The events after `response.created` that complete one response."""
    item: Message = {
        "type": "message",
        "role": "assistant",
        "id": f"msg_{number}",
        "content": [{"type": "output_text", "text": ANSWER}],
    }
    usage: Usage = {
        "input_tokens": 5,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": 1,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 6,
    }
    return [
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {"type": "response.completed", "response": {"id": f"resp_{number}", "usage": usage}},
    ]


def read_exactly(conn: socket.socket, n: int) -> bytes | None:
    data = b""
    while len(data) < n:
        chunk = conn.recv(n - len(data))
        if not chunk:
            return None
        data += chunk
    return data


def read_frame(conn: socket.socket) -> Frame | None:
    """One client frame, unmasked, or None once the peer is gone or broke the
    protocol: a client frame must be masked (RFC 6455 5.1), set no reserved
    bit, and be no larger than MAX_BODY. Codex sends each request as one
    final frame, so a fragment (FIN unset) is refused rather than reassembled,
    as is a control frame over 125 bytes (RFC 6455 5.5)."""
    head = read_exactly(conn, 2)
    if head is None or not head[1] & 0x80 or head[0] & 0x70 or not head[0] & 0x80:
        return None
    opcode, length = head[0] & 0x0F, head[1] & 0x7F
    if opcode & 0x8 and length > 125:
        return None
    if length in (126, 127):
        raw = read_exactly(conn, 2 if length == 126 else 8)
        if raw is None:
            return None
        (length,) = struct.unpack("!H" if len(raw) == 2 else "!Q", raw)
    if length > MAX_BODY:
        return None
    mask = read_exactly(conn, 4)
    payload = read_exactly(conn, length)
    if mask is None or payload is None:
        return None
    return Frame(opcode, bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))


def send_frame(conn: socket.socket, opcode: Opcode, payload: bytes) -> None:
    length = len(payload)
    if length < 126:
        head = struct.pack("!BB", 0x80 | opcode, length)
    elif length < 1 << 16:
        head = struct.pack("!BBH", 0x80 | opcode, 126, length)
    else:
        head = struct.pack("!BBQ", 0x80 | opcode, 127, length)
    conn.sendall(head + payload)


def websocket_accept(key: str | None) -> str | None:
    """The handshake's accept value, or None for a key that is not 16 bytes of
    base64 (RFC 6455 4.1)."""
    if key is None:
        return None
    try:
        if len(base64.b64decode(key, validate=True)) != 16:
            return None
    except (binascii.Error, ValueError):
        return None
    # RFC 6455 fixes SHA-1 for the handshake; it protects nothing here.
    digest = hashlib.sha1((key + WS_GUID).encode(), usedforsecurity=False).digest()
    return base64.b64encode(digest).decode()


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    state: State

    @override
    def log_message(self, format: str, *args: object) -> None:
        """Routine access lines stay out of the log a failing phase prints."""

    def refuse(self, status: int, why: str) -> None:
        body = why.encode()
        self.send_response(status)
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def do_GET(self) -> None:
        if self.headers.get("upgrade", "").lower() != "websocket":
            body = json.dumps({"models": []}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        accept = websocket_accept(self.headers.get("sec-websocket-key"))
        if accept is None:
            self.refuse(400, "a websocket upgrade needs a Sec-WebSocket-Key of 16 base64 bytes")
            return
        self.send_response(101)
        self.send_header("upgrade", "websocket")
        self.send_header("connection", "Upgrade")
        self.send_header("sec-websocket-accept", accept)
        self.end_headers()
        self.wfile.flush()
        conn = self.connection
        while True:
            frame = read_frame(conn)
            if frame is None or frame.opcode == Opcode.CLOSE:
                break
            if frame.opcode == Opcode.PING:
                send_frame(conn, Opcode.PONG, frame.payload)
                continue
            if frame.opcode != Opcode.TEXT:
                continue
            request = parse_request(frame.payload)
            if request is None:
                send_frame(conn, Opcode.CLOSE, struct.pack("!H", 1007))
                break
            decision = self.state.decide(Transport.WEBSOCKET, request, len(frame.payload))
            if decision is None:
                send_frame(conn, Opcode.CLOSE, struct.pack("!H", 1011))
                break
            send_frame(conn, Opcode.TEXT, json.dumps(created(decision.number)).encode())
            if decision.mode is Mode.STALL:
                time.sleep(STALL_SECONDS)
                break
            for event in completion(decision.number):
                send_frame(conn, Opcode.TEXT, json.dumps(event).encode())
        self.close_connection = True

    def do_POST(self) -> None:
        try:
            length = int(self.headers.get("content-length", "0"))
        except ValueError:
            length = -1
        if not 0 <= length <= MAX_BODY:
            self.refuse(400, f"Content-Length must be a whole number from 0 to {MAX_BODY}")
            return
        body = self.rfile.read(length)
        request = parse_request(body)
        if request is None:
            self.refuse(400, "a Responses request is a JSON object")
            return
        decision = self.state.decide(Transport.SSE, request, len(body))
        if decision is None:
            self.refuse(500, "the endpoint cannot record this request; its stderr says why")
            return
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("connection", "close")
        self.end_headers()

        def emit(event: Event) -> None:
            data = f"event: {event['type']}\ndata: {json.dumps(event)}\n\n"
            self.wfile.write(data.encode())
            self.wfile.flush()

        emit(created(decision.number))
        if decision.mode is Mode.STALL:
            time.sleep(STALL_SECONDS)
        else:
            for event in completion(decision.number):
                emit(event)
        self.close_connection = True


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: codex-stall-endpoint.py DIR", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    if not root.is_dir():
        print(f"codex-stall-endpoint: {root} is not a directory; create it first", file=sys.stderr)
        return 2
    Handler.state = State(root)
    try:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    except OSError as err:
        print(
            f"codex-stall-endpoint: cannot listen on 127.0.0.1 ({err}); "
            "check that loopback is up and this process may bind a port",
            file=sys.stderr,
        )
        return 1
    server.daemon_threads = True
    port_file = root / "port"
    try:
        port_file.write_text(f"{server.server_address[1]}\n")
    except OSError as err:
        print(
            f"codex-stall-endpoint: cannot write {port_file} ({err}); "
            f"check that {root} is writable and has space",
            file=sys.stderr,
        )
        return 1
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
