#!/usr/bin/env python3
"""A local OpenAI chat-completions endpoint that records what a harness sends.

    chat-capture-endpoint.py DIR

Serves `POST .../chat/completions` on 127.0.0.1, at a port it picks and writes
to DIR/port. Every request's message text is appended to DIR/requests.txt under
a `--- request N ---` line, with CRLF folded to LF, so a caller can grep it for
what the harness put in the model's context. Each reply is the first
`ONEHARNESS-LIVE-<digits>` marker in the request's last user message (or
`no marker`), as one assistant message, streamed when the request asks for it.

It is the fixture of `oh_context_free_enforce` (scripts/e2e-lib.sh), which
points the real qwen at it through OPENAI_BASE_URL, so no model call is made or
billed.
"""

# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The reply shapes below are the minimum of OpenAI's chat-completions response qwen needs to finish a turn, and real qwen is their drift alarm: `oh_context_free_enforce` fails if qwen stops accepting them, and the deterministic gate cannot run qwen.

from __future__ import annotations

import http.server
import json
import re
import socketserver
import sys
import threading
from pathlib import Path

MARKER = re.compile(r"ONEHARNESS-LIVE-[0-9]+")


def message_text(content: object) -> str:
    """The text of one message's content: a string, or a list of text parts."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            part["text"]
            for part in content
            if isinstance(part, dict) and isinstance(part.get("text"), str)
        )
    return ""


def reply_for(messages: list[object]) -> str:
    for message in reversed(messages):
        if isinstance(message, dict) and message.get("role") == "user":
            found = MARKER.search(message_text(message.get("content")))
            return found.group(0) if found else "no marker"
    return "no marker"


class Recorder:
    def __init__(self, directory: Path) -> None:
        self.log = directory / "requests.txt"
        self.count = 0
        self.lock = threading.Lock()

    def record(self, messages: list[object]) -> None:
        texts = [
            message_text(m.get("content")) for m in messages if isinstance(m, dict)
        ]
        with self.lock:
            self.count += 1
            with self.log.open("a", encoding="utf-8", newline="\n") as out:
                out.write(f"--- request {self.count} ---\n")
                out.write("\n".join(texts).replace("\r\n", "\n") + "\n")


def handler(recorder: Recorder) -> type[http.server.BaseHTTPRequestHandler]:
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: object) -> None:  # noqa: A002
            return

        def do_POST(self) -> None:
            if not self.path.rstrip("/").endswith("/chat/completions"):
                self.send_error(404, "only chat/completions is served")
                return
            try:
                length = int(self.headers.get("Content-Length") or 0)
                request = json.loads(self.rfile.read(length) or b"{}")
            except (ValueError, OSError) as error:
                self.send_error(400, f"unreadable request: {error}")
                return
            messages = request.get("messages") if isinstance(request, dict) else None
            if not isinstance(messages, list):
                self.send_error(400, "the request has no messages list")
                return
            recorder.record(messages)
            text = reply_for(messages)
            if request.get("stream"):
                self.stream(text)
            else:
                self.whole(text)

        def whole(self, text: str) -> None:
            body = json.dumps(
                {
                    "id": "capture",
                    "object": "chat.completion",
                    "created": 0,
                    "model": "capture",
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": text},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                }
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def stream(self, text: str) -> None:
            chunk = {"id": "capture", "object": "chat.completion.chunk", "created": 0, "model": "capture"}
            events = [
                {**chunk, "choices": [{"index": 0, "delta": {"role": "assistant", "content": text}, "finish_reason": None}]},
                {
                    **chunk,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                },
            ]
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for event in events:
                self.wfile.write(b"data: " + json.dumps(event).encode() + b"\n\n")
            self.wfile.write(b"data: [DONE]\n\n")
            self.close_connection = True

    return Handler


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: chat-capture-endpoint.py DIR", file=sys.stderr)
        return 2
    directory = Path(argv[1])
    if not directory.is_dir():
        print(f"chat-capture-endpoint: {directory} is not a directory; create it first", file=sys.stderr)
        return 2
    server = Server(("127.0.0.1", 0), handler(Recorder(directory)))
    port = directory / "port.tmp"
    port.write_text(str(server.server_address[1]), encoding="ascii")
    port.replace(directory / "port")
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
