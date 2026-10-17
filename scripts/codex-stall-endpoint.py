#!/usr/bin/env python3
"""A local OpenAI Responses endpoint that can stall one response, for real Codex.

    codex-stall-endpoint.py DIR

Serves the two transports Codex's built-in `openai` provider uses — the
Responses websocket (`GET /v1/responses`, upgraded) and, as its fallback, SSE
(`POST /v1/responses`) — on 127.0.0.1, at a port it picks and writes to
DIR/port. Every response says one fixed assistant message and completes, unless
DIR/mode reads `stall-next` when a request arrives: that one response sends
`response.created` and then nothing, and the mode returns to serving, so the
request Codex retries once it notices the stall is answered. Each request is
appended to DIR/requests.log as `<epoch seconds> <transport> <warmup|request>
<mode> <bytes>`.

It is the fixture of `oh_resume_stall_enforce` (scripts/e2e-lib.sh), which
points an isolated Codex at it through `openai_base_url` — the built-in
provider, never a custom one — so no model call is made or billed.
"""

from __future__ import annotations

import base64
import hashlib
import http.server
import json
import socket
import struct
import sys
import threading
import time
from pathlib import Path

ANSWER = "OH-STALL-RECOVERED"
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
# How long a stalled response is held. Longer than any bound a phase puts on a
# turn, so only Codex's own idle timeout can end it.
STALL_SECONDS = 3600


class State:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.lock = threading.Lock()
        self.count = 0

    def take_mode(self, transport: str, body: bytes) -> tuple[int, str]:
        """This request's number and whether it stalls, consuming a `stall-next`.

        A websocket warmup (`"generate": false`, which Codex sends before each
        turn's request on that transport) is always served: stalling it tests
        nothing, because Codex abandons a warmup and carries on.
        """
        try:
            request = json.loads(body)
        except ValueError:
            request = {}
        warmup = isinstance(request, dict) and request.get("generate") is False
        with self.lock:
            self.count += 1
            mode_file = self.root / "mode"
            mode = mode_file.read_text().strip() if mode_file.exists() else "serve"
            if mode == "stall-next" and not warmup:
                mode_file.write_text("serve\n")
                mode = "stall"
            else:
                mode = "serve"
            with (self.root / "requests.log").open("a") as log:
                kind = "warmup" if warmup else "request"
                log.write(f"{time.time():.3f} {transport} {kind} {mode} {len(body)}\n")
            return self.count, mode


def events(number: int) -> list[dict[str, object]]:
    """The events after `response.created` that complete one response."""
    item = {
        "type": "message",
        "role": "assistant",
        "id": f"msg_{number}",
        "content": [{"type": "output_text", "text": ANSWER}],
    }
    usage = {
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


def created(number: int) -> dict[str, object]:
    return {"type": "response.created", "response": {"id": f"resp_{number}"}}


def read_frame(conn: socket.socket) -> tuple[int, bytes] | None:
    """One client frame: (opcode, unmasked payload), or None once the peer is gone."""

    def exactly(n: int) -> bytes | None:
        data = b""
        while len(data) < n:
            chunk = conn.recv(n - len(data))
            if not chunk:
                return None
            data += chunk
        return data

    head = exactly(2)
    if head is None:
        return None
    opcode, length = head[0] & 0x0F, head[1] & 0x7F
    if length == 126:
        raw = exactly(2)
        if raw is None:
            return None
        (length,) = struct.unpack("!H", raw)
    elif length == 127:
        raw = exactly(8)
        if raw is None:
            return None
        (length,) = struct.unpack("!Q", raw)
    mask = exactly(4) if head[1] & 0x80 else b"\0\0\0\0"
    payload = exactly(length)
    if mask is None or payload is None:
        return None
    return opcode, bytes(b ^ mask[i % 4] for i, b in enumerate(payload))


def send_frame(conn: socket.socket, opcode: int, payload: bytes) -> None:
    length = len(payload)
    if length < 126:
        head = struct.pack("!BB", 0x80 | opcode, length)
    elif length < 1 << 16:
        head = struct.pack("!BBH", 0x80 | opcode, 126, length)
    else:
        head = struct.pack("!BBQ", 0x80 | opcode, 127, length)
    conn.sendall(head + payload)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    state: State

    def do_GET(self) -> None:
        if self.headers.get("upgrade", "").lower() != "websocket":
            body = json.dumps({"models": []}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        accept = base64.b64encode(
            # RFC 6455 fixes SHA-1 for the handshake; it protects nothing here.
            hashlib.sha1(
                (self.headers["sec-websocket-key"] + WS_GUID).encode(), usedforsecurity=False
            ).digest()
        ).decode()
        self.send_response(101)
        self.send_header("upgrade", "websocket")
        self.send_header("connection", "Upgrade")
        self.send_header("sec-websocket-accept", accept)
        self.end_headers()
        self.wfile.flush()
        conn = self.connection
        while True:
            frame = read_frame(conn)
            if frame is None:
                break
            opcode, payload = frame
            if opcode == 0x8:
                break
            if opcode == 0x9:
                send_frame(conn, 0xA, payload)
                continue
            if opcode != 0x1:
                continue
            number, mode = self.state.take_mode("websocket", payload)
            send_frame(conn, 0x1, json.dumps(created(number)).encode())
            if mode == "stall":
                time.sleep(STALL_SECONDS)
                break
            for event in events(number):
                send_frame(conn, 0x1, json.dumps(event).encode())
        self.close_connection = True

    def do_POST(self) -> None:
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        number, mode = self.state.take_mode("sse", body)
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("connection", "close")
        self.end_headers()

        def emit(event: dict[str, object]) -> None:
            data = f"event: {event['type']}\ndata: {json.dumps(event)}\n\n"
            self.wfile.write(data.encode())
            self.wfile.flush()

        emit(created(number))
        if mode == "stall":
            time.sleep(STALL_SECONDS)
        else:
            for event in events(number):
                emit(event)
        self.close_connection = True


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: codex-stall-endpoint.py DIR", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    Handler.state = State(root)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    (root / "port").write_text(f"{server.server_address[1]}\n")
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
