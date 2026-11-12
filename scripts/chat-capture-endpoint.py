#!/usr/bin/env python3
"""A local model endpoint that records what a harness sends.

    chat-capture-endpoint.py DIR

Serves OpenAI's `POST .../chat/completions` and Anthropic's `POST .../messages`
on 127.0.0.1, at a port it picks and writes to DIR/port. Every request's
message text is appended to DIR/requests.txt under a `--- request N ---` line,
with CRLF folded to LF, so a caller can grep it for what the harness put in the
model's context. A Messages request's `system` is also appended to
DIR/system.jsonl verbatim, as one `{"system": [TEXT, ...]}` line, so a caller
can hold it to exactly what it gave the harness, line breaks and all. Each
reply is the first `ONEHARNESS-LIVE-<digits>` marker in the request's last user
message (or `no marker`), as one assistant message, streamed when the request
asks for it.

It is the fixture of `oh_context_free_enforce` (scripts/e2e-lib.sh), which
points the real qwen at it through OPENAI_BASE_URL, and of
`oh_multiline_system_enforce`, which points the real claude at it through
ANTHROPIC_BASE_URL, so no model call is made or billed.
"""

# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The reply shapes below are the minimum of OpenAI's chat-completions response qwen needs to finish a turn, and of Anthropic's Messages stream claude needs, and the real CLIs are their drift alarms: `oh_context_free_enforce` fails if qwen stops accepting them and `oh_multiline_system_enforce` if claude does, and the deterministic gate cannot run either.
# llmlint: ignore-file[structural_pattern_matching] The repository's Python floor is 3.9 (python/oneharness-sdk/pyproject.toml), which has no `match` statement.

from __future__ import annotations

import http.server
import json
import re
import socketserver
import sys
import threading
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Optional

MARKER = re.compile(r"ONEHARNESS-LIVE-[0-9]+")
# Far above any prompt a live phase sends; a larger body is refused unread.
MAX_BODY = 16 * 1024 * 1024


class BadRequest(ValueError):
    """A request this endpoint cannot record, with the reason sent back."""


@dataclass(frozen=True)
class Message:
    role: str
    text: str

    @staticmethod
    def parse(raw: object) -> Message:
        if not isinstance(raw, dict):
            raise BadRequest("a message is not an object")
        role = raw.get("role")
        if not isinstance(role, str):
            raise BadRequest("a message has no string role")
        return Message(role, content_text(raw.get("content")))


def content_text(content: object) -> str:
    """A message's text: its string content, or its text parts joined."""
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            part["text"] for part in content if isinstance(part, dict) and isinstance(part.get("text"), str)
        )
    raise BadRequest("a message's content is neither text nor a list of parts")


@dataclass(frozen=True)
class ChatRequest:
    messages: list[Message]
    stream: bool

    @staticmethod
    def parse(body: bytes) -> ChatRequest:
        try:
            raw = json.loads(body)
        except ValueError as error:
            raise BadRequest(f"the body is not JSON: {error}") from error
        if not isinstance(raw, dict) or not isinstance(raw.get("messages"), list):
            raise BadRequest("the request has no messages list")
        stream = raw.get("stream", False)
        if not isinstance(stream, bool):
            raise BadRequest("stream must be true or false")
        return ChatRequest([Message.parse(m) for m in raw["messages"]], stream)

    def reply(self) -> str:
        for message in reversed(self.messages):
            if message.role == "user":
                found = MARKER.search(message.text)
                return found.group(0) if found else "no marker"
        return "no marker"


def system_texts(system: object) -> list[str]:
    """A Messages request's `system`: its string, or its text blocks in order."""
    if system is None:
        return []
    if isinstance(system, str):
        return [system]
    if isinstance(system, list):
        return [
            block["text"]
            for block in system
            if isinstance(block, dict) and isinstance(block.get("text"), str)
        ]
    raise BadRequest("system is neither text nor a list of blocks")


@dataclass(frozen=True)
class MessagesRequest:
    """Anthropic's Messages request, as far as this endpoint reads it."""

    system: list[str]
    chat: ChatRequest

    @staticmethod
    def parse(body: bytes) -> MessagesRequest:
        chat = ChatRequest.parse(body)
        return MessagesRequest(system_texts(json.loads(body).get("system")), chat)

    def as_chat(self) -> ChatRequest:
        """The whole context as chat messages, `system` first, for requests.txt."""
        system = [Message("system", text) for text in self.system]
        return ChatRequest(system + self.chat.messages, self.chat.stream)


@dataclass(frozen=True)
class Usage:
    prompt_tokens: int = 1
    completion_tokens: int = 1
    total_tokens: int = 2


@dataclass(frozen=True)
class Delta:
    role: Optional[str] = None
    content: Optional[str] = None


@dataclass(frozen=True)
class AssistantMessage:
    content: str
    role: str = "assistant"


@dataclass(frozen=True)
class Choice:
    message: AssistantMessage
    index: int = 0
    finish_reason: str = "stop"


@dataclass(frozen=True)
class ChunkChoice:
    delta: Delta
    finish_reason: Optional[str]
    index: int = 0


@dataclass(frozen=True)
class Completion:
    choices: list[Choice]
    usage: Usage = field(default_factory=Usage)
    id: str = "capture"
    object: str = "chat.completion"
    created: int = 0
    model: str = "capture"


@dataclass(frozen=True)
class Chunk:
    choices: list[ChunkChoice]
    usage: Optional[Usage] = None
    id: str = "capture"
    object: str = "chat.completion.chunk"
    created: int = 0
    model: str = "capture"


def without_nulls(value: object) -> object:
    """A serialized model with its unset optional fields left out."""
    if isinstance(value, dict):
        return {k: without_nulls(v) for k, v in value.items() if v is not None or k == "finish_reason"}
    if isinstance(value, list):
        return [without_nulls(v) for v in value]
    return value


def encode(model: Completion | Chunk) -> bytes:
    return json.dumps(without_nulls(asdict(model))).encode()


MESSAGE_USAGE = {"input_tokens": 1, "output_tokens": 1}


def anthropic_message(text: str) -> dict[str, object]:
    """A whole Messages response carrying one text block."""
    return {
        "id": "msg_capture",
        "type": "message",
        "role": "assistant",
        "model": "capture",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "stop_sequence": None,
        "usage": MESSAGE_USAGE,
    }


def anthropic_events(text: str) -> list[dict[str, object]]:
    """The Messages stream of that response, event by event."""
    start = {**anthropic_message(""), "content": [], "stop_reason": None}
    return [
        {"type": "message_start", "message": start},
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}},
        {"type": "content_block_stop", "index": 0},
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": None},
            "usage": {"output_tokens": 1},
        },
        {"type": "message_stop"},
    ]


class Recorder:
    def __init__(self, directory: Path) -> None:
        self.log = directory / "requests.txt"
        self.systems = directory / "system.jsonl"
        self.count = 0
        self.lock = threading.Lock()

    def record(self, request: ChatRequest, system: Optional[list[str]] = None) -> None:
        text = "\n".join(m.text for m in request.messages).replace("\r\n", "\n")
        with self.lock:
            self.count += 1
            self.append(self.log, f"--- request {self.count} ---\n{text}\n")
            if system is not None:
                self.append(self.systems, json.dumps({"system": system}) + "\n")

    @staticmethod
    def append(path: Path, text: str) -> None:
        try:
            with path.open("a", encoding="utf-8", newline="\n") as out:
                out.write(text)
        except OSError as error:
            print(
                f"chat-capture-endpoint: could not append to {path}: {error}; "
                "check that its directory is writable and has free space",
                file=sys.stderr,
            )
            raise


def handler(recorder: Recorder) -> type[http.server.BaseHTTPRequestHandler]:
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args: object) -> None:
            return

        def do_POST(self) -> None:
            route = self.path.split("?", 1)[0].rstrip("/")
            messages = route.endswith("/messages")
            if not messages and not route.endswith("/chat/completions"):
                self.send_error(404, "only chat/completions and messages are served")
                return
            try:
                length = int(self.headers.get("Content-Length") or "0")
            except ValueError:
                self.send_error(400, "Content-Length is not a whole number")
                return
            if not 0 <= length <= MAX_BODY:
                self.send_error(413, f"Content-Length must be 0 to {MAX_BODY} bytes")
                return
            body = self.rfile.read(length)
            try:
                if messages:
                    parsed = MessagesRequest.parse(body)
                    request, system = parsed.as_chat(), parsed.system
                else:
                    request, system = ChatRequest.parse(body), None
            except BadRequest as error:
                self.send_error(400, str(error))
                return
            try:
                recorder.record(request, system)
            except OSError:
                self.send_error(500, f"could not record the request in {recorder.log}")
                return
            if messages:
                self.anthropic(request.reply(), request.stream)
            elif request.stream:
                self.stream(request.reply())
            else:
                self.whole(request.reply())

        def anthropic(self, text: str, stream: bool) -> None:
            if not stream:
                body = json.dumps(anthropic_message(text)).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for event in anthropic_events(text):
                self.wfile.write(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n".encode())
            self.close_connection = True

        def whole(self, text: str) -> None:
            body = encode(Completion([Choice(AssistantMessage(text))]))
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def stream(self, text: str) -> None:
            chunks = [
                Chunk([ChunkChoice(Delta(role="assistant", content=text), None)]),
                Chunk([ChunkChoice(Delta(), "stop")], usage=Usage()),
            ]
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for chunk in chunks:
                self.wfile.write(b"data: " + encode(chunk) + b"\n\n")
            self.wfile.write(b"data: [DONE]\n\n")
            self.close_connection = True

    return Handler


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    """An HTTP server that binds without resolving its own address's name.

    `HTTPServer.server_bind` names the server with `socket.getfqdn`, a reverse
    lookup of 127.0.0.1 that macOS's resolver can hold for longer than the
    phase waits for the port file, so the endpoint never reported its port
    there. Nothing here reads the name, so the bound address stands in for it.
    """

    daemon_threads = True

    def server_bind(self) -> None:
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name = str(host)
        self.server_port = int(port)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: chat-capture-endpoint.py DIR", file=sys.stderr)
        return 2
    directory = Path(argv[1])
    if not directory.is_dir():
        print(
            f"chat-capture-endpoint: {directory} is not a directory; create it first",
            file=sys.stderr,
        )
        return 2
    try:
        server = Server(("127.0.0.1", 0), handler(Recorder(directory)))
    except OSError as error:
        print(
            f"chat-capture-endpoint: could not listen on 127.0.0.1: {error}; "
            "check that the loopback interface is up and this process may bind a port",
            file=sys.stderr,
        )
        return 1
    staged = directory / "port.tmp"
    try:
        staged.write_text(str(server.server_address[1]), encoding="ascii")
        staged.replace(directory / "port")
    except OSError as error:
        print(
            f"chat-capture-endpoint: could not write {directory / 'port'}: {error}; "
            "check that the directory is writable and has free space",
            file=sys.stderr,
        )
        return 1
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
