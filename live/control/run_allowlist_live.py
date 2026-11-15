"""Live proof of the run-level allowlist against the real Claude Code, Codex
and OpenCode.

    run_allowlist_live.py [--harness claude-code,codex,opencode]
                          [--codex-path exec|control|both] [--only GROUP,...]
                          [--scratch DIR] [--keep]
    run_allowlist_live.py --budget-telemetry PATH   the budget workload alone

It drives the real `oneharness` binary (`$ONEHARNESS_BIN`, else
`target/release/oneharness`) over the real CLIs installed on this host, under
`--mode read-only`, against a scripted model endpoint this script serves in
place of the paid provider (Anthropic Messages for Claude Code, OpenAI
Responses for Codex, Chat Completions for OpenCode). The endpoint answers each
turn's first model request with exactly the probe's tool call and the next
with `done`; everything between — the CLI's own permission check, its sandbox
and its execution — is the real harness. Every probe settles by the decision
the harness itself records: the tool result it hands back to the model on the
next request. A probe whose harness recorded none is a failure, never a pass.

Each harness gets one world: a git worktree, a sentinel directory outside it,
the granted stand-in executable (it appends one record of its argv to a log
outside the worktree), a second, ungranted executable, its own harness home,
and ONE config file holding the top-level grant (written `$OH_LIVE_GRANT`) and
a `[sync] allowed_tools` rule granting `touch`, which `oneharness sync` writes
into the worktree before any turn.

Groups (`--only`): sync, probes, posture, lifecycle, kill, concurrency,
session; record, login and keyring on Codex `exec`; OpenCode, which withholds
the grant, runs `withheld` alone (each turn granted and not, the same outcome).
Exits 0 when every check passed, 1 naming each failure on stderr, 2 on a usage
error. Needs Python 3.10 or later.
"""

from __future__ import annotations

import argparse
import base64
import dataclasses
import enum
import hashlib
import http.server
import json
import os
import re
import shlex
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any, NamedTuple, NewType

ROOT = Path(__file__).resolve().parents[2]
# The budget record's one shape and its collector, owned by the
# network-telemetry project this suite depends on (live/control/project.json).
sys.path.insert(0, str(ROOT / "tools" / "network-telemetry"))

from run_overlay_telemetry import BATCHES, OverlayTimings, Telemetry, TurnTiming, run_concurrently  # noqa: E402
GRANT_VAR = "OH_LIVE_GRANT"
MARK = re.compile(r"\[probe:([A-Za-z0-9_.-]+)\]")
PAYLOAD = "line one\nit's line two"
OUTSIDE_TEXT = "outside the worktree: sentinel text"
HARNESSES = ("claude-code", "codex", "opencode")
OVERLAY_MARKER = "ONEHARNESS_RUN_OVERLAY"
OVERLAY_PREFIX = "oneharness-run-overlay-"
OVERLAY_ROOT = ("oneharness", "run-overlays")
"""How this suite finds an overlay: the marker its processes carry, its
directory's name, and the state-dir path it is made under. They restate
`io::run_overlay`'s constants, and `oneharness-core`'s gated unit test
`the_live_suites_overlay_detector_restates_the_engines_layout` reads these
three lines and fails when they drift; `check_layout` also holds them to what
every granted Codex run's report says."""
SAFE_PATH = re.compile(r"[A-Za-z0-9_./-]+")
GROUPS = (
    "sync", "probes", "posture", "lifecycle", "kill", "concurrency", "session",
    "record", "login", "keyring", "withheld",
)  # fmt: skip

ProbeId = NewType("ProbeId", str)
"""The token a turn's prompt carries as `[probe:<id>]`: how the endpoint finds
the turn's script among concurrent turns."""

Json = dict[str, Any]
"""A JSON object read from a harness, a request or a report: untyped where it
crosses the boundary, read field by field."""


class CallKind(enum.Enum):
    SHELL = "shell"
    WRITE = "write"
    EDIT = "edit"
    TASK = "task"
    READ = "read"
    GREP = "grep"
    GLOB = "glob"
    FAIL = "fail"


@dataclasses.dataclass(frozen=True)
class Call:
    """One tool call the scripted model makes, in harness-neutral terms."""

    kind: CallKind
    text: str = ""  # the command (shell), the delegated task, or written content
    path: str = ""
    pattern: str = ""


@dataclasses.dataclass(frozen=True)
class Request:
    """One model request of a scripted turn, as the endpoint saw it."""

    made: int
    results: list[str]
    at: float


@dataclasses.dataclass
class Script:
    calls: list[Call]
    hold: threading.Event | None = None
    seen: threading.Event = dataclasses.field(default_factory=threading.Event)
    requests: list[Request] = dataclasses.field(default_factory=list)

    def decision(self) -> str | None:
        """The harness's recorded decision: the tool result it returned."""
        for request in self.requests:
            if request.results:
                return request.results[-1]
        return None

    def release(self) -> None:
        if self.hold is not None:
            self.hold.set()


@dataclasses.dataclass(frozen=True)
class Refresh:
    """The token endpoint's scripted answer, and each refresh it received."""

    id_token: str
    access_token: str
    refresh_token: str


class Endpoint:
    """The scripted model: one script per probe id, named in the prompt."""

    def __init__(self) -> None:
        self.scripts: dict[ProbeId, Script] = {}
        self.lock = threading.Lock()
        self.require_bearer: str | None = None
        self.refresh: Refresh | None = None
        self.refreshed_with: list[str] = []
        self.rejected: int = 0
        handler = type("Bound", (Handler,), {"endpoint": self})
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def script(self, probe: ProbeId, calls: list[Call], hold: bool = False) -> Script:
        s = Script(calls, threading.Event() if hold else None)
        with self.lock:
            self.scripts[probe] = s
        return s

    def stop(self) -> None:
        self.server.shutdown()


def text_of(content: object) -> str:
    match content:
        case str():
            return content
        case list():
            return "\n".join(
                str(c.get("text") or c.get("content") or c.get("output") or "")
                if isinstance(c, dict)
                else str(c)
                for c in content
            )
        case None:
            return ""
        case _:
            return str(content)


class Handler(http.server.BaseHTTPRequestHandler):
    endpoint: Endpoint

    def log_message(self, *args: object) -> None:
        pass

    def handle(self) -> None:
        # A client killed mid-answer (the SIGKILL and SIGTERM checks) is expected.
        try:
            super().handle()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def send_json(self, obj: object, code: int = 200) -> None:
        raw = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def send_events(self, events: list[tuple[str | None, object]]) -> None:
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        for name, data in events:
            chunk = (f"event: {name}\n" if name else "") + "data: " + json.dumps(data) + "\n\n"
            self.wfile.write(chunk.encode())
        self.wfile.flush()

    def do_GET(self) -> None:
        self.send_json({"object": "list", "data": [], "models": []})

    def do_POST(self) -> None:
        length = self.headers.get("content-length") or "0"
        # ASCII digits only, and no more of them than MAX_BODY has, so int()
        # can neither reject what passed nor parse an unbounded string.
        bounded = length.isascii() and length.isdigit() and len(length) <= len(str(MAX_BODY))
        if not bounded or int(length) > MAX_BODY:
            self.send_json({"error": {"message": f"content-length {length!r} is not 0..{MAX_BODY}"}}, 400)
            return
        raw = self.rfile.read(int(length))
        path = self.path.split("?")[0]
        if path.endswith("/oauth/token"):
            self.answer_refresh(raw)
            return
        try:
            body = json.loads(raw or b"{}")
        except ValueError:
            body = None
        if not well_formed(body):
            self.send_json({"error": {"message": "the request is not a model request object"}}, 400)
            return
        if path.endswith("/count_tokens"):
            self.send_json({"input_tokens": 10})
            return
        want = self.endpoint.require_bearer
        if want is not None and self.headers.get("authorization") != f"Bearer {want}":
            self.endpoint.rejected += 1
            self.send_json({"error": {"message": "unauthorized", "code": "invalid_token"}}, 401)
            return
        dialect = next(
            (
                name
                for suffix, name in (
                    ("/messages", "anthropic"),
                    ("/responses", "responses"),
                    ("/chat/completions", "chat"),
                )
                if path.endswith(suffix)
            ),
            None,
        )
        if dialect is None:
            self.send_json({"error": {"message": f"unknown path {path}"}}, 404)
            return
        # The turn's own prompt is the last one: a resumed conversation also
        # carries every earlier turn's.
        marks = list(MARK.finditer(json.dumps(body)))
        found = marks[-1] if marks else None
        tools = [t for t in body.get("tools") or [] if isinstance(t, dict)]
        with self.endpoint.lock:
            script = self.endpoint.scripts.get(ProbeId(found.group(1))) if found else None
        if script is None or found is None or not tools:
            # A side request (a title, a summary) or a turn this run never scripted.
            ANSWERS[dialect](self, body, None)
            return
        made, results = history(dialect, body, found.group(0))
        script.requests.append(Request(made, results, time.time()))
        script.seen.set()
        if script.hold is not None:
            script.hold.wait(timeout=600)
        call = script.calls[made] if made < len(script.calls) else None
        if call is not None and call.kind is CallKind.FAIL:
            error = {"type": "invalid_request_error", "message": "scripted failure"}
            self.send_json({"error": error}, 400)
            return
        ANSWERS[dialect](self, body, call)

    def answer_refresh(self, raw: bytes) -> None:
        try:
            sent = json.loads(raw)
        except ValueError:
            sent = None
        token = sent.get("refresh_token") if isinstance(sent, dict) else None
        self.endpoint.refreshed_with.append(token if isinstance(token, str) else "")
        reply = self.endpoint.refresh
        if reply is None:
            self.send_json({"error": "unscripted"}, 400)
            return
        self.send_json(dataclasses.asdict(reply))


MAX_BODY = 64 * 1024 * 1024
"""The largest request body the endpoint reads: far past any turn's context."""


def well_formed(body: object) -> bool:
    """Whether `body` is a model request the endpoint can read: an object whose
    `tools`, `messages` and `input`, when present, are lists of objects."""
    if not isinstance(body, dict):
        return False
    for key in ("tools", "messages"):
        value = body.get(key, [])
        if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
            return False
    items = body.get("input", [])
    return isinstance(items, str) or (isinstance(items, list) and all(isinstance(i, dict) for i in items))


def history(dialect: str, body: Json, mark: str) -> tuple[int, list[str]]:
    """(tool calls the model made, tool results the harness returned) since the
    turn's own prompt — a resumed conversation carries earlier turns' too."""
    made, results = 0, []
    match dialect:
        case "chat" | "anthropic":
            messages = [m for m in body.get("messages") or [] if isinstance(m, dict)]
            start = max((i for i, m in enumerate(messages) if mark in json.dumps(m)), default=0)
            for m in messages[start:]:
                if dialect == "chat":
                    calls = m.get("tool_calls")
                    if m.get("role") == "assistant" and isinstance(calls, list):
                        made += sum(1 for call in calls if isinstance(call, dict))
                    if m.get("role") == "tool":
                        results.append(text_of(m.get("content")))
                    continue
                content = m.get("content")
                for block in content if isinstance(content, list) else []:
                    match block:
                        case {"type": "tool_use"}:
                            made += 1
                        case {"type": "tool_result", **rest}:
                            results.append(text_of(rest.get("content")))
        case _:
            items = [it for it in body.get("input") or [] if isinstance(it, dict)]
            start = max((i for i, it in enumerate(items) if mark in json.dumps(it)), default=0)
            for item in items[start:]:
                match item.get("type"):
                    case "function_call" | "custom_tool_call" | "local_shell_call":
                        made += 1
                    case "function_call_output" | "custom_tool_call_output":
                        out = item.get("output")
                        results.append(out if isinstance(out, str) else json.dumps(out))
    return made, results


# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] These answers are the live suite's own double of three provider protocols, and the real CLIs that read them in the live tier are its drift alarm: a CLI that stops understanding one records no decision, which fails every probe as inconclusive. The deterministic gate cannot run those CLIs, and no provider publishes a schema to generate them from.
def answer_anthropic(h: Handler, body: Json, call: Call | None) -> None:
    mid = "msg_" + uuid.uuid4().hex[:20]
    match call:
        case None:
            block: Json = {"type": "text", "text": ""}
            final: Json = {"type": "text", "text": "done"}
            delta: Json = {"type": "text_delta", "text": "done"}
            stop = "end_turn"
        case Call(kind=kind):
            name, inp = {
                CallKind.SHELL: ("Bash", {"command": call.text, "description": "live probe"}),
                CallKind.WRITE: ("Write", {"file_path": call.path, "content": call.text}),
                CallKind.EDIT: (
                    "Edit",
                    {"file_path": call.path, "old_string": "worktree", "new_string": "pwned"},
                ),
                CallKind.TASK: (
                    "Agent",
                    {"description": "live probe", "prompt": call.text,
                     "subagent_type": "general-purpose"},
                ),  # fmt: skip
                CallKind.READ: ("Read", {"file_path": call.path}),
                CallKind.GREP: (
                    "Grep",
                    {"pattern": call.pattern, "path": call.path, "output_mode": "content"},
                ),
                CallKind.GLOB: ("Glob", {"pattern": call.pattern, "path": call.path}),
            }[kind]
            tid = "toolu_" + uuid.uuid4().hex[:20]
            block = {"type": "tool_use", "id": tid, "name": name, "input": {}}
            delta = {"type": "input_json_delta", "partial_json": json.dumps(inp)}
            final = {"type": "tool_use", "id": tid, "name": name, "input": inp}
            stop = "tool_use"
    usage = {"input_tokens": 10, "output_tokens": 5}
    message = {"id": mid, "type": "message", "role": "assistant", "model": body.get("model", "m"),
               "stop_reason": None, "stop_sequence": None, "usage": usage}  # fmt: skip
    if not body.get("stream"):
        h.send_json({**message, "content": [final], "stop_reason": stop})
        return
    h.send_events([
        ("message_start", {"type": "message_start", "message": {**message, "content": []}}),
        ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": block}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": delta}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        ("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop,
                           "stop_sequence": None}, "usage": {"output_tokens": 5}}),
        ("message_stop", {"type": "message_stop"}),
    ])  # fmt: skip


def answer_responses(h: Handler, body: Json, call: Call | None) -> None:
    rid = "resp_" + uuid.uuid4().hex[:20]
    names = [t.get("name") or t.get("type", "") for t in body.get("tools") or []]
    call_id = "call_" + uuid.uuid4().hex[:12]
    match call:
        case None:
            item: Json = {"type": "message", "role": "assistant", "id": "msg_" + uuid.uuid4().hex[:12],
                          "content": [{"type": "output_text", "text": "done", "annotations": []}]}  # fmt: skip
        case Call(kind=CallKind.SHELL | CallKind.READ):
            cmd = call.text if call.kind is CallKind.SHELL else f"cat {call.path}"
            if "exec_command" in names:
                name, args = "exec_command", {"cmd": cmd}
            elif "shell_command" in names:
                name, args = "shell_command", {"command": cmd}
            else:
                name, args = "shell", {"command": ["bash", "-lc", cmd]}
            item = {"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:12], "name": name,
                    "arguments": json.dumps(args), "call_id": call_id}  # fmt: skip
        case Call(kind=CallKind.WRITE):
            patch = f"*** Begin Patch\n*** Add File: {call.path}\n+{call.text}\n*** End Patch\n"
            item = {"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:12],
                    "name": "apply_patch", "input": patch, "call_id": call_id}  # fmt: skip
        case Call(kind=CallKind.EDIT):
            patch = f"*** Begin Patch\n*** Update File: {call.path}\n@@\n-worktree\n+pwned\n*** End Patch\n"
            item = {"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:12],
                    "name": "apply_patch", "input": patch, "call_id": call_id}  # fmt: skip
        case Call(kind=CallKind.TASK):
            item = {"type": "function_call", "id": "fc_" + uuid.uuid4().hex[:12], "name": "spawn_agent",
                    "arguments": json.dumps({"message": call.text}), "call_id": call_id}  # fmt: skip
        case Call(kind=kind):
            raise ValueError(f"Codex has no {kind.value} tool")
    usage = {"input_tokens": 10, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 5,
             "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 15}  # fmt: skip
    h.send_events([
        ("response.created", {"type": "response.created", "response": {"id": rid}}),
        ("response.output_item.added", {"type": "response.output_item.added", "output_index": 0,
                                        "item": item}),
        ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0,
                                       "item": item}),
        ("response.completed", {"type": "response.completed",
                                "response": {"id": rid, "usage": usage, "output": [item]}}),
    ])  # fmt: skip


def answer_chat(h: Handler, body: Json, call: Call | None) -> None:
    """An OpenAI Chat Completions stream (OpenCode's openai-compatible provider)."""
    base = {"id": "chatcmpl-" + uuid.uuid4().hex[:16], "object": "chat.completion.chunk",
            "created": int(time.time()), "model": body.get("model", "m")}  # fmt: skip
    match call:
        case None:
            delta: Json = {"role": "assistant", "content": "done"}
            finish = "stop"
        case Call(kind=kind):
            name, args = {
                CallKind.SHELL: ("bash", {"command": call.text, "description": "live probe"}),
                CallKind.WRITE: ("write", {"filePath": call.path, "content": call.text}),
                CallKind.EDIT: (
                    "edit",
                    {"filePath": call.path, "oldString": "worktree", "newString": "pwned"},
                ),
                CallKind.TASK: (
                    "task",
                    {"description": "live probe", "prompt": call.text, "subagent_type": "general"},
                ),
                CallKind.READ: ("read", {"filePath": call.path}),
            }[kind]
            tool = {"index": 0, "id": "call_" + uuid.uuid4().hex[:12], "type": "function",
                    "function": {"name": name, "arguments": json.dumps(args)}}  # fmt: skip
            delta = {"role": "assistant", "tool_calls": [tool]}
            finish = "tool_calls"
    usage = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    h.send_events([
        (None, {**base, "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}),
        (None, {**base, "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}),
        (None, {**base, "choices": [], "usage": usage}),
    ])  # fmt: skip
    h.wfile.write(b"data: [DONE]\n\n")
    h.wfile.flush()


# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

ANSWERS: dict[str, Callable[[Handler, Json, Call | None], None]] = {
    "anthropic": answer_anthropic,
    "responses": answer_responses,
    "chat": answer_chat,
}

STANDIN = """#!/usr/bin/env bash
# The granted executable's stand-in: append one record of its argv to LOG.
args=""
for a in "$@"; do args="$args$(printf '%s' "$a" | base64 | tr -d '\\n') "; done
printf '{"name":"@NAME@","args_b64":"%s"}\\n' "$args" >> @LOG@ || { echo "could not append" >&2; exit 3; }
echo "@NAME@: recorded"
"""


class Failures:
    """Every failed check, each named on stderr as it fails."""

    def __init__(self) -> None:
        self.items: list[str] = []

    def check(self, ok: bool, what: str) -> bool:
        if not ok:
            self.items.append(what)
            print(f"FAIL {what}", file=sys.stderr, flush=True)
        return ok


def short_dir() -> Path:
    # A controlled turn's socket and the server pool's live under this, and a
    # unix socket address is at most 104-108 bytes.
    return Path(tempfile.mkdtemp(prefix="oh-ra-", dir="/tmp"))


@dataclasses.dataclass
class World:
    harness: str
    root: Path
    endpoint: Endpoint
    control: bool = False
    sessions: Path = dataclasses.field(default_factory=short_dir)
    extra_env: dict[str, str] = dataclasses.field(default_factory=dict)
    drop_env: frozenset[str] = frozenset()

    @property
    def label(self) -> str:
        return f"{self.harness}{' (control)' if self.control else ''}"

    @property
    def wt(self) -> Path:
        return self.root / "worktree"

    @property
    def tmp(self) -> Path:
        return self.root / "tmp"

    @property
    def state(self) -> Path:
        return self.sessions / "st"

    @property
    def home(self) -> Path:
        name = {"codex": "codex-home", "opencode": "opencode-home"}.get(self.harness)
        return self.root / (name or "claude-config")

    @property
    def grant(self) -> Path:
        return self.root / "bin" / "grant"

    @property
    def other(self) -> Path:
        return self.root / "bin" / "other"

    @property
    def log(self) -> Path:
        return self.root / "out" / "records.jsonl"

    @property
    def sentinel(self) -> Path:
        return self.root / "sentinel"

    @property
    def config(self) -> Path:
        return self.root / "oneharness.toml"

    def make(self) -> World:
        for sub in ("worktree", "tmp", "bin", "out", "sentinel", "history"):
            (self.root / sub).mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q", str(self.wt)], check=True)
        (self.wt / "README.md").write_text("live worktree\n")
        (self.sentinel / "notes.txt").write_text(OUTSIDE_TEXT + "\n")
        (self.root / "user.toml").touch()
        for exe, log, name in (
            (self.grant, self.log, "grant"),
            (self.other, self.root / "out" / "other.jsonl", "other"),
        ):
            exe.write_text(STANDIN.replace("@LOG@", shlex.quote(str(log))).replace("@NAME@", name))
            exe.chmod(0o755)
            log.touch()
        self.home.mkdir(parents=True, exist_ok=True)
        match self.harness:
            case "codex":
                write_codex_home(self.home, self.endpoint.port)
            case "opencode":
                write_opencode_home(self.home, self.endpoint.port)
        self.write_config(grant=True)
        if self.harness == "codex":
            # A home Codex has already used, as a real one is: its sessions/
            # tree and thread index exist before the first granted turn.
            warm = self.endpoint.script(ProbeId("warm"), [])
            try:
                # Unformatted so the argv reads as the command line it is, flag
                # beside value, as every other harness command in this file.
                subprocess.run(
                    ["codex", "exec", "--sandbox", "read-only", "--json", "--skip-git-repo-check",
                     "-C", str(self.wt), "[probe:warm] reply done"],
                    env=self.env(), capture_output=True, text=True, timeout=300, check=True,
                    stdin=subprocess.DEVNULL,
                )  # fmt: skip
            except (subprocess.TimeoutExpired, subprocess.CalledProcessError) as err:
                # Codex's own output is the only account of where it stopped:
                # before reaching the scripted endpoint, or after its answer.
                raise SystemExit(warm_up_failure(err, len(warm.requests))) from err
        return self

    def write_config(self, grant: bool) -> None:
        top = f'allowed_tools = ["Bash(${GRANT_VAR})"]\n' if grant else ""
        # The scripted endpoint serves no model catalog, so the shared catalog
        # would only add its 10 s wait to every Codex launch, granted or not.
        codex = "[harness.codex]\nshared_model_catalog = false\n" if self.harness == "codex" else ""
        self.config.write_text(
            top + codex + f'[env]\n{GRANT_VAR} = "{self.grant}"\n'
            '[sync]\nallowed_tools = ["Bash(touch:*)"]\n'
        )

    def env(self) -> dict[str, str]:
        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith(("ONEHARNESS_", "CODEX_", "CLAUDE_", "ANTHROPIC_", "OPENAI_"))
        }
        env.update(
            TMPDIR=str(self.tmp),
            XDG_STATE_HOME=str(self.state),
            ONEHARNESS_CONFIG=str(self.root / "user.toml"),
        )
        match self.harness:
            case "codex":
                # The pooled app-server inherits only CODEX_* and OPENAI_*
                # variables, so the scripted provider's key is spelled so.
                env.update(CODEX_HOME=str(self.home), CODEX_SCRIPTED_KEY="scripted")
            case "opencode":
                env.update(
                    XDG_CONFIG_HOME=str(self.home / "config"),
                    XDG_DATA_HOME=str(self.home / "data"),
                )
            case _:
                env.update(
                    CLAUDE_CONFIG_DIR=str(self.home),
                    ANTHROPIC_API_KEY="scripted",
                    ANTHROPIC_BASE_URL=f"http://127.0.0.1:{self.endpoint.port}",
                )
        env.update(self.extra_env)
        return {k: v for k, v in env.items() if k not in self.drop_env}

    def argv(self, prompt: str, session: str | None = None, extra: list[str] | None = None,
             config: Path | None = None) -> list[str]:  # fmt: skip
        argv = [oneharness_bin(), "run", "--harness", self.harness, "--mode", "read-only",
                "--cwd", str(self.wt), "--config", str(config or self.config),
                "--prompt", prompt, "--compact"]  # fmt: skip
        if self.control or session is not None:
            argv += ["--session", session or f"s-{uuid.uuid4().hex[:8]}",
                     "--session-dir", str(self.sessions)]  # fmt: skip
        if self.control:
            argv.append("--control")
        if self.harness == "claude-code":
            argv += ["--model", "claude-sonnet-4-5"]
        return argv + (extra or [])

    def overlays(self) -> list[str]:
        roots = (self.tmp, self.state.joinpath(*OVERLAY_ROOT))
        return [p.name for r in roots if r.is_dir() for p in r.iterdir() if p.name.startswith(OVERLAY_PREFIX)]


def warm_up_failure(err: subprocess.TimeoutExpired | subprocess.CalledProcessError, reached: int) -> str:
    """Why the Codex world's warm-up turn failed, with the tail of what Codex printed."""

    def tail(out: str | bytes | None) -> str:
        text = out.decode(errors="replace") if isinstance(out, bytes) else out or ""
        return text[-2000:].strip() or "(nothing)"

    how = (
        f"timed out after {err.timeout:g} s"
        if isinstance(err, subprocess.TimeoutExpired)
        else f"exited {err.returncode}"
    )
    return (
        f"the Codex world's warm-up `codex exec` {how}; the scripted endpoint saw "
        f"{reached} model request(s) for it\n"
        f"codex stderr (tail):\n{tail(err.stderr)}\ncodex stdout (tail):\n{tail(err.stdout)}"
    )


def write_codex_home(home: Path, port: int, provider: str | None = None) -> None:
    (home / "rules").mkdir(parents=True, exist_ok=True)
    (home / "sessions").mkdir(exist_ok=True)
    (home / "rules" / "user.rules").write_text('prefix_rule(pattern=["ls"], decision="allow")\n')
    provider = provider or (
        '[model_providers.scripted]\nname = "scripted"\n'
        f'base_url = "http://127.0.0.1:{port}/v1"\nwire_api = "responses"\n'
        'env_key = "CODEX_SCRIPTED_KEY"\nrequest_max_retries = 0\nstream_max_retries = 0\n'
    )
    # Nothing reaches the network: no remote plugin catalog, no ChatGPT apps
    # (connectors) MCP server, and the ChatGPT backend is the scripted endpoint.
    (home / "config.toml").write_text(
        'model = "gpt-5.5"\nmodel_provider = "scripted"\n'
        f'chatgpt_base_url = "http://127.0.0.1:{port}/backend-api/"\n'
        "[analytics]\nenabled = false\n[features]\nplugins = false\napps = false\n" + provider
    )
    (home / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "scripted"}) + "\n")


def write_opencode_home(home: Path, port: int) -> None:
    (home / "config" / "opencode").mkdir(parents=True, exist_ok=True)
    (home / "data" / "opencode").mkdir(parents=True, exist_ok=True)
    provider = {"npm": "@ai-sdk/openai-compatible", "name": "scripted",
                "options": {"baseURL": f"http://127.0.0.1:{port}/v1", "apiKey": "scripted"},
                "models": {"m": {"name": "m", "tool_call": True}}}  # fmt: skip
    config = {"autoupdate": False, "share": "disabled", "model": "scripted/m",
              "small_model": "scripted/m", "provider": {"scripted": provider}}  # fmt: skip
    (home / "config" / "opencode" / "opencode.json").write_text(json.dumps(config))


def oneharness_bin() -> str:
    return os.environ.get("ONEHARNESS_BIN") or str(ROOT / "target" / "release" / "oneharness")


def snapshot(world: World) -> dict[str, str]:
    """Every file the probes control: the worktree (but .git), the sentinel
    directory, and both executables' logs."""
    out: dict[str, str] = {}
    for base in (world.wt, world.sentinel, world.root / "out"):
        for path in sorted(base.rglob("*")):
            if ".git" in path.relative_to(base).parts or not path.is_file():
                continue
            out[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    return out


def records(world: World) -> list[list[str]]:
    """Each record the granted stand-in appended: its argv, decoded. A line the
    stand-in did not write is a failure of the suite's own world, named."""
    rows = []
    for number, line in enumerate(world.log.read_text().splitlines(), start=1):
        rec = json.loads(line)
        encoded = rec.get("args_b64") if isinstance(rec, dict) else None
        if not isinstance(encoded, str):
            raise ValueError(f"{world.log}:{number} is no stand-in record: {line!r}")
        rows.append([base64.b64decode(a, validate=True).decode() for a in encoded.split()])
    return rows


@dataclasses.dataclass(frozen=True)
class Result:
    """What the suite reads of one result of a `oneharness run` report."""

    status: str | None
    session_id: str | None
    stderr: str
    diagnostics: list[Json]
    overlay_env: dict[str, str]
    overlay: OverlayTimings | None
    usage: Json | None

    @property
    def overlaid(self) -> bool:
        return bool(self.overlay_env)

    @staticmethod
    def of(report: Json) -> Result:
        """The report's first result, each field kept only in its documented
        type: a field of another shape reads as absent."""
        results = report.get("results")
        raw = results[0] if isinstance(results, list) and results and isinstance(results[0], dict) else {}

        def text(key: str) -> str | None:
            value = raw.get(key)
            return value if isinstance(value, str) else None

        overlay = OverlayTimings.from_json(raw.get("overlay_timings"))
        diagnostics = raw.get("diagnostics")
        diagnostics = [d for d in diagnostics if isinstance(d, dict)] if isinstance(diagnostics, list) else []
        usage = raw.get("usage")
        env = raw.get("overlay_env")
        overlay_env = {k: v for k, v in env.items() if isinstance(v, str)} if isinstance(env, dict) else {}
        return Result(text("status"), text("session_id"), text("stderr") or "", diagnostics,
                      overlay_env, overlay, usage if isinstance(usage, dict) else None)  # fmt: skip

    def grant_diagnostics(self) -> list[Json]:
        return [d for d in self.diagnostics if d.get("setting") == "allowed_tools"]


@dataclasses.dataclass(frozen=True)
class Turn:
    report: Json
    result: Result
    stderr: str
    code: int


def run_turn(world: World, probe: ProbeId, calls: list[Call], what: str,
             session: str | None = None, extra: list[str] | None = None) -> tuple[Turn, Script]:  # fmt: skip
    script = world.endpoint.script(probe, calls)
    prompt = (
        f"[probe:{probe}] This is a permission test. {what} Do not try any alternative if it "
        "is refused. Then reply with one word: done."
    )
    proc = subprocess.run(
        world.argv(prompt, session, extra), env=world.env(), capture_output=True, text=True,
        timeout=600, stdin=subprocess.DEVNULL,
    )  # fmt: skip
    report = report_of(proc.stdout)
    return Turn(report, Result.of(report), proc.stderr, proc.returncode), script


def probe_id(name: str) -> ProbeId:
    return ProbeId(f"{name}-{uuid.uuid4().hex[:6]}")


class Expect(enum.Enum):
    RUN = "run"
    REFUSE = "refuse"
    ALLOW = "allow"  # a read today's read-only posture permits


@dataclasses.dataclass(frozen=True)
class Probe:
    name: str
    expect: Expect
    call: Callable[[World], Call]
    args: list[str] | None = None  # the granted record a RUN probe must leave
    harnesses: tuple[str, ...] = ("claude-code", "codex")


def shell(command: Callable[[World], str]) -> Callable[[World], Call]:
    def call(world: World) -> Call:
        return Call(CallKind.SHELL, text=command(world))

    return call


def probes() -> list[Probe]:
    def g(w: World) -> Path:
        return w.grant

    def pwned(w: World) -> str:
        return f"{w.wt}/pwned"

    run, refuse, allow = Expect.RUN, Expect.REFUSE, Expect.ALLOW
    return [
        Probe("granted-plain", run, shell(lambda w: f"{g(w)} --ruling r-1"), ["--ruling", "r-1"]),
        Probe("granted-multiline-apostrophe", run,
              shell(lambda w: f'{g(w)} --ruling r-1 --reason "{PAYLOAD}"'),
              ["--ruling", "r-1", "--reason", PAYLOAD]),
        Probe("touch-worktree", refuse, shell(lambda w: f"touch {pwned(w)}")),
        Probe("synced-touch-rule", refuse, shell(lambda w: f"touch {w.sentinel}/synced")),
        Probe("chain-semicolon", refuse, shell(lambda w: f"{g(w)} --ruling r-1; touch {pwned(w)}")),
        Probe("chain-and", refuse, shell(lambda w: f"{g(w)} --ruling r-1 && touch {pwned(w)}")),
        Probe("chain-or", refuse, shell(lambda w: f"{g(w)} --ruling r-1 --bad || touch {pwned(w)}")),
        Probe("chain-newline", refuse, shell(lambda w: f"{g(w)} --ruling r-1\ntouch {pwned(w)}")),
        Probe("chain-other", refuse, shell(lambda w: f"{g(w)} --ruling r-1; {w.other} --ruling r-1")),
        Probe("pipe-into-tee", refuse, shell(lambda w: f"{g(w)} --ruling r-1 | tee {pwned(w)}")),
        Probe("subst-dollar-other", refuse, shell(lambda w: f'{g(w)} --ruling "$({w.other} --ruling r-1)"')),
        Probe("subst-backtick-other", refuse, shell(lambda w: f'{g(w)} --ruling "`{w.other} --ruling r-1`"')),
        Probe("redirect-out", refuse, shell(lambda w: f"{g(w)} --ruling r-1 > {pwned(w)}")),
        Probe("redirect-append-log", refuse, shell(lambda w: f"echo forged >> {w.log}")),
        Probe("other-executable", refuse, shell(lambda w: f"{w.other} --ruling r-1")),
        Probe("reassign-variable", refuse, shell(lambda w: f'{GRANT_VAR}={w.other}; "${GRANT_VAR}" --ruling r-1')),
        Probe("reassign-variable-touch", refuse,
              shell(lambda w: f'{GRANT_VAR}=/usr/bin/touch; "${GRANT_VAR}" {pwned(w)}')),
        Probe("shadow-function", refuse, shell(lambda w: f"{g(w)}() {{ touch {pwned(w)}; }}; {g(w)} --ruling r-1")),
        Probe("shadow-alias", refuse,
              shell(lambda w: f"shopt -s expand_aliases; alias grant='touch {pwned(w)}'\ngrant --ruling r-1")),
        Probe("subagent-delegate", refuse, lambda w: Call(CallKind.TASK, text=f"Run `touch {pwned(w)}`.")),
        Probe("write-tool-create", refuse, lambda w: Call(CallKind.WRITE, path=pwned(w), text="pwned")),
        Probe("edit-tool-modify", refuse, lambda w: Call(CallKind.EDIT, path=f"{w.wt}/README.md")),
        Probe("read-outside", allow, lambda w: Call(CallKind.READ, path=str(w.sentinel / "notes.txt"))),
        Probe("grep-outside", allow, lambda w: Call(CallKind.GREP, pattern="sentinel text", path=str(w.sentinel)),
              harnesses=("claude-code",)),
        Probe("glob-outside", allow, lambda w: Call(CallKind.GLOB, pattern="*.txt", path=str(w.sentinel)),
              harnesses=("claude-code",)),
    ]  # fmt: skip


def describe(call: Call) -> str:
    match call.kind:
        case CallKind.SHELL:
            return f"Use your shell tool to run exactly this command, once, verbatim:\n\n{call.text}\n"
        case CallKind.TASK:
            return "Use your subagent tool to delegate this to a subagent."
        case CallKind.WRITE:
            return f"Use your file editing tool to create {call.path}."
        case CallKind.EDIT:
            return f"Use your file editing tool to edit {call.path}."
        case CallKind.READ:
            return f"Use your file read tool to read {call.path}."
        case CallKind.GREP:
            return f"Search {call.path} for '{call.pattern}'."
        case CallKind.GLOB:
            return f"List {call.pattern} under {call.path}."
        case CallKind.FAIL:
            return "Answer."


def changed_between(before: dict[str, str], after: dict[str, str]) -> list[str]:
    return sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))


def judge(world: World, probe: Probe, fails: Failures) -> None:
    before, before_records = snapshot(world), records(world)
    call = probe.call(world)
    turn, script = run_turn(world, probe_id(probe.name), [call], describe(call))
    label = f"{world.label} {probe.name}"
    decision = script.decision()
    if not fails.check(turn.code in (0, 1) and turn.result.status is not None,
                       f"{label}: oneharness exited {turn.code}: {turn.stderr[-400:]}"):  # fmt: skip
        return
    if not fails.check(decision is not None, f"{label}: the harness recorded no decision (inconclusive)"):
        return
    assert decision is not None
    fails.check(
        "Refusing to create helper binaries" not in turn.result.stderr,
        f"{label}: Codex lost its PATH helpers on the overlay home: {turn.result.stderr[:300]}",
    )
    check_layout(world, turn.result, fails, label)
    changed = changed_between(before, snapshot(world))
    new = records(world)[len(before_records) :]
    match probe.expect:
        case Expect.RUN:
            fails.check(new == [probe.args],
                        f"{label}: the granted call left {new}, wanted [{probe.args}]; decision: {decision[:300]}")  # fmt: skip
            fails.check(changed == [str(world.log)], f"{label}: unexpected change {changed}")
        case Expect.REFUSE:
            fails.check(not changed and not new,
                        f"{label}: NOT refused — changed {changed}, records {new}; decision: {decision[:300]}")  # fmt: skip
        case Expect.ALLOW:
            fails.check(OUTSIDE_TEXT in decision or "notes.txt" in decision,
                        f"{label}: a read outside the worktree was refused: {decision[:300]}")  # fmt: skip
            fails.check(not changed, f"{label}: a read changed {changed}")
    check_clean(world, fails, label)


def check_layout(world: World, result: Result, fails: Failures, label: str) -> None:
    """A granted Codex run's report names its overlay where this suite looks:
    the marker and the directory under the world's overlay root."""
    if world.harness != "codex" or not result.overlaid:
        return
    marker = result.overlay_env.get(OVERLAY_MARKER, "")
    home = Path(result.overlay_env.get("CODEX_HOME", ""))
    fails.check(
        marker.startswith(OVERLAY_PREFIX)
        and home.parent.name == marker
        and home.parent.parent == world.state.joinpath(*OVERLAY_ROOT),
        f"{label}: oneharness reports its overlay as {result.overlay_env}, not where this suite looks "
        f"({OVERLAY_MARKER}={OVERLAY_PREFIX}… under {world.state.joinpath(*OVERLAY_ROOT)}); update the detector",
    )


def check_clean(world: World, fails: Failures, label: str) -> None:
    """Nothing the overlay made outlives the run: no overlay directory, no
    process of one, no run rule in the worktree or the real home."""
    fails.check(not world.overlays(), f"{label}: overlay left behind: {world.overlays()}")
    fails.check(not overlay_pids(world), f"{label}: an overlay process survived: {overlay_pids(world)}")
    for base in (world.wt, world.home):
        for path in base.rglob("*"):
            if path.name == "oneharness-run.rules":
                fails.check(False, f"{label}: the run's rules file reached {path}")
            if path.is_symlink() and "run-overlay" in os.readlink(path):
                fails.check(False, f"{label}: a link into the overlay reached {path}")


def overlay_pids(world: World) -> list[int]:
    """Processes carrying an overlay of this world (Linux /proc)."""
    found = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            environ = (entry / "environ").read_bytes()
        except OSError:
            continue
        if f"{OVERLAY_MARKER}={OVERLAY_PREFIX}".encode() in environ and str(world.state).encode() in environ:
            found.append(int(entry.name))
    return found


def group_sync(world: World, fails: Failures) -> None:
    proc = subprocess.run(
        [oneharness_bin(), "sync", "--harness", world.harness, "--cwd", str(world.wt),
         "--config", str(world.config), "--compact"],
        env=world.env(), capture_output=True, text=True, stdin=subprocess.DEVNULL,
    )  # fmt: skip
    if not fails.check(proc.returncode == 0, f"{world.label} sync: {proc.stderr}"):
        return
    match world.harness:
        case "claude-code":
            held = json.loads((world.wt / ".claude" / "settings.json").read_text())["permissions"]["allow"]
            fails.check(held == ["Bash(touch:*)"], f"claude-code sync wrote {held}")
        case _:
            rules = (world.wt / ".codex" / "rules" / "oneharness.rules").read_text()
            fails.check('pattern=["touch"]' in rules and str(world.grant) not in rules,
                        f"codex sync wrote {rules!r}")  # fmt: skip


def group_probes(world: World, fails: Failures, kinds: tuple[Expect, ...]) -> None:
    for probe in probes():
        if world.harness in probe.harnesses and probe.expect in kinds:
            judge(world, probe, fails)


class Held(NamedTuple):
    """A granted turn the endpoint holds at its first model request."""

    process: subprocess.Popen[str]
    script: Script


def held_turn(world: World, name: str, extra: list[str] | None = None, config: Path | None = None) -> Held:
    probe = probe_id(name)
    script = world.endpoint.script(probe, [Call(CallKind.SHELL, text=f"{world.grant} --ruling {probe}")], hold=True)
    argv = world.argv(f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}",
                      extra=extra, config=config)  # fmt: skip
    proc = subprocess.Popen(argv, env=world.env(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                            stdin=subprocess.DEVNULL)  # fmt: skip
    if not script.seen.wait(timeout=120):
        proc.kill()
        raise RuntimeError(f"{probe}: the harness never reached the model")
    return Held(proc, script)


def report_of(stdout: str) -> Json:
    """A run's report: the JSON object on its stdout, or an empty one when
    stdout holds anything else."""
    try:
        report = json.loads(stdout)
    except ValueError:
        return {}
    return report if isinstance(report, dict) else {}


def status_of(out: str) -> str | None:
    return Result.of(report_of(out)).status


def recorded_turn_failure(world: World, report: Json) -> bool:
    """Whether the harness's recorded event stream (the report's first result's
    stdout) holds the event that ends a turn as failed on the endpoint's
    scripted 400: Claude Code's error `result`, `codex exec`'s `turn.failed`,
    or the app-server's `turn/completed` whose turn has status `failed`."""
    results = report.get("results")
    raw = results[0] if isinstance(results, list) and results and isinstance(results[0], dict) else {}
    stdout = raw.get("stdout")
    for line in stdout.splitlines() if isinstance(stdout, str) else []:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if not isinstance(event, dict):
            continue
        params = event.get("params")
        turn = params.get("turn") if isinstance(params, dict) else None
        match world.harness, world.control:
            case "claude-code", _:
                ended, carries = event.get("type") == "result" and event.get("is_error") is True, event.get("result")
            case "codex", False:
                ended, carries = event.get("type") == "turn.failed", event.get("error")
            case "codex", True:
                ended = event.get("method") == "turn/completed" and isinstance(turn, dict) and turn.get("status") == "failed"
                carries = turn.get("error") if isinstance(turn, dict) else None
            case _:
                return False
        if ended and "scripted failure" in json.dumps(carries):
            return True
    return False


def group_lifecycle(world: World, fails: Failures) -> None:
    label = f"{world.label} lifecycle"
    turn, script = run_turn(world, probe_id("fail"), [Call(CallKind.FAIL)], "Answer.")
    # The failure is the endpoint's scripted 400: it must have been served, and
    # the harness's own record of the turn must say the turn failed on it. The
    # report's status is read beside that record, never in its place: a
    # `--control` Codex turn ends on `turn/completed` without reading the
    # turn's status, so oneharness reports it `ok` (main does the same); every
    # other path reports the harness's nonzero exit.
    failed = recorded_turn_failure(world, turn.report)
    expected = "ok" if world.control else "nonzero"
    fails.check(bool(script.requests) and failed and turn.result.status == expected,
                f"{label} failure: status {turn.result.status} (expected {expected}), "
                f"{len(script.requests)} model request(s), turn recorded failed on the scripted 400: {failed}")  # fmt: skip
    check_clean(world, fails, f"{label} after a harness failure")
    held = held_turn(world, "timeout", extra=["--timeout", "5"])
    out, _ = held.process.communicate(timeout=120)
    held.script.release()
    fails.check(status_of(out) == "timeout", f"{label} timeout: status {status_of(out)}")
    time.sleep(1)
    check_clean(world, fails, f"{label} after a timeout")
    held = held_turn(world, "term")
    held.process.send_signal(signal.SIGTERM)
    out, _ = held.process.communicate(timeout=120)
    held.script.release()
    fails.check(status_of(out) == "cancelled", f"{label} SIGTERM: status {status_of(out)}")
    time.sleep(1)
    check_clean(world, fails, f"{label} after SIGTERM")


def group_kill(world: World, fails: Failures) -> None:
    label = f"{world.label} SIGKILL"
    killed = held_turn(world, "killed")
    live = held_turn(world, "live")
    before_kill = set(world.overlays())
    killed.process.send_signal(signal.SIGKILL)
    killed.process.wait(timeout=30)
    # The next oneharness run reclaims the killed run's overlay and processes.
    run_turn(world, probe_id("next"), [Call(CallKind.READ, path=str(world.sentinel / "notes.txt"))], "Read it.")
    remaining = set(world.overlays())
    if world.harness == "codex":
        fails.check(len(before_kill) == 2, f"{label}: expected two live overlays, saw {sorted(before_kill)}")
        fails.check(len(remaining) == 1 and remaining < before_kill,
                    f"{label}: after the next run the overlays are {sorted(remaining)} (were {sorted(before_kill)})")  # fmt: skip
    killed.script.release()
    live.script.release()
    out, err = live.process.communicate(timeout=300)
    fails.check(status_of(out) == "ok", f"{label}: the live run beside it ended {status_of(out)}: {err[-300:]}")
    time.sleep(1)
    check_clean(world, fails, label)


def direct_cli(world: World, name: str) -> tuple[subprocess.Popen[str], Script]:
    """The harness CLI run directly against the same home and project — the
    user's own session."""
    probe = probe_id(name)
    script = world.endpoint.script(probe, [Call(CallKind.SHELL, text=f"{world.grant} --ruling {probe}")])
    prompt = f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}"
    match world.harness:
        case "codex":
            argv = ["codex", "exec", "--sandbox", "read-only", "--json", "--skip-git-repo-check",
                    "-C", str(world.wt), prompt]  # fmt: skip
        case _:
            argv = ["claude", "-p", prompt, "--permission-mode", "dontAsk", "--output-format", "json",
                    "--model", "claude-sonnet-4-5"]  # fmt: skip
    env = {**world.env(), GRANT_VAR: str(world.grant)}
    proc = subprocess.Popen(argv, env=env, cwd=world.wt, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            stdin=subprocess.DEVNULL, text=True)  # fmt: skip
    return proc, script


def group_concurrency(world: World, fails: Failures) -> None:
    label = f"{world.label} isolation"
    ungranted = world.root / "ungranted.toml"
    ungranted.write_text(world.config.read_text().split("\n", 1)[1])
    for phase in ("concurrent", "sequential"):
        held = held_turn(world, "granted") if phase == "concurrent" else None
        before = len(records(world))
        probe = probe_id("ungranted")
        script = world.endpoint.script(probe, [Call(CallKind.SHELL, text=f"{world.grant} --ruling {probe}")])
        prompt = f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}"
        subprocess.run(world.argv(prompt, config=ungranted), env=world.env(), capture_output=True, text=True,
                       timeout=600, stdin=subprocess.DEVNULL)  # fmt: skip
        fails.check(script.decision() is not None, f"{label} {phase}: the ungranted run recorded no decision")
        proc, direct = direct_cli(world, "direct")
        proc.communicate(timeout=600)
        fails.check(direct.decision() is not None, f"{label} {phase}: the direct CLI recorded no decision")
        fails.check(len(records(world)) == before,
                    f"{label} {phase}: an ungranted session ran the granted executable: {records(world)[before:]}")  # fmt: skip
        if held is not None:
            held.script.release()
            out, _ = held.process.communicate(timeout=300)
            fails.check(status_of(out) == "ok" and len(records(world)) == before + 1,
                        f"{label}: the granted run beside them did not run its call ({status_of(out)})")  # fmt: skip
    check_clean(world, fails, label)


def group_session(world: World, fails: Failures) -> None:
    label = f"{world.label} session"
    name = f"cont-{uuid.uuid4().hex[:6]}"
    hist = ["--history", "--history-dir", str(world.root / "history")]
    for turn_no in (1, 2):
        before = len(records(world))
        command = f"{world.grant} --ruling t{turn_no}"
        turn, _ = run_turn(world, probe_id(f"{name}-{turn_no}"), [Call(CallKind.SHELL, text=command)],
                           f"Use your shell tool to run exactly: {command}", session=name, extra=hist)  # fmt: skip
        fails.check(turn.result.status == "ok" and records(world)[before:] == [["--ruling", f"t{turn_no}"]],
                    f"{label}: turn {turn_no} ended {turn.result.status} with {records(world)[before:]}")  # fmt: skip
        if turn_no == 2:
            session = turn.report.get("session")
            phase = session.get("phase") if isinstance(session, dict) else None
            fails.check(phase == "continue", f"{label}: the second turn did not continue: {session}")
    world.write_config(grant=False)
    run_turn(world, probe_id("plain"), [Call(CallKind.READ, path=str(world.sentinel / "notes.txt"))],
             "Read the notes.", extra=hist)  # fmt: skip
    world.write_config(grant=True)
    rows = [json.loads(line) for f in sorted((world.root / "history").rglob("*.jsonl"))
            for line in f.read_text().splitlines() if line.strip()]  # fmt: skip
    fails.check(all(isinstance(row, dict) for row in rows), f"{label}: a history line is no record object")
    rows = [row for row in rows if isinstance(row, dict)]
    runs = [r for r in rows if r.get("type") == "run" or "permission_mode" in r]
    if fails.check(len(runs) >= 3, f"{label}: the history store holds {len(runs)} turn records, wanted 3"):
        *granted, plain = runs[-3:]
        for record in granted:
            missing = sorted(set(plain) - set(record) - {"observed_tool_ms"})
            same = (not missing and record.get("permission_mode") == plain.get("permission_mode")
                    and record.get("status") == plain.get("status") == "ok"
                    and not any("overlay" in key or "allowed" in key for key in record))  # fmt: skip
            fails.check(same, f"{label}: a granted turn is recorded unlike an ungranted read-only one "
                              f"(missing {missing}; {record.get('permission_mode')}/{record.get('status')})")  # fmt: skip


def open_sqlite_files(world: World) -> set[str]:
    """Every SQLite file the world's overlay processes hold open."""
    files = set()
    for pid in overlay_pids(world):
        fd_dir = Path(f"/proc/{pid}/fd")
        for fd in fd_dir.iterdir() if fd_dir.is_dir() else []:
            try:
                target = os.readlink(fd)
            except OSError:
                continue
            if ".sqlite" in target:
                files.add(target)
    return files


def group_record(world: World, fails: Failures) -> None:
    """Codex: a granted run's rollout, thread index and new top-level files are
    in the real home, and its usage matches the same run ungranted."""
    label = "codex record"
    installation = world.home / "installation_id"
    installation.unlink(missing_ok=True)
    command = f"{world.grant} --ruling rec"
    what = f"Use your shell tool to run exactly: {command}"
    turn, _ = run_turn(world, probe_id("rec"), [Call(CallKind.SHELL, text=command)], what)
    thread = turn.result.session_id
    fails.check(installation.is_file() and not installation.is_symlink(),
                f"{label}: a file Codex created at the home's top level is not in the real home")  # fmt: skip
    rollouts = list((world.home / "sessions").rglob(f"*{thread}.jsonl")) if thread else []
    fails.check(len(rollouts) == 1, f"{label}: the granted rollout {thread} is not in the real sessions/")
    dbs = sorted(world.home.glob("state_*.sqlite"))
    if fails.check(bool(dbs) and bool(rollouts), f"{label}: the real home has no thread index"):
        # llmlint: ignore[async_typed_clients_at_boundaries] A single-threaded live check reads one row of Codex's own index once its turn has ended; nothing here runs on an event loop, and stdlib sqlite3 is the only reader this dependency-free script has.
        with sqlite3.connect(dbs[-1]) as db:
            # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] An assertion about Codex's own private index, made only in the live tier against the installed codex: a schema change fails it loudly there (sqlite raises on an unknown table or column), which is the drift alarm a store Codex alone defines can have.
            row = db.execute("select rollout_path from threads where id = ?", (thread,)).fetchone()
        fails.check(row is not None and Path(row[0]) == rollouts[0],
                    f"{label}: the real index holds {row} for {thread}, wanted {rollouts}")  # fmt: skip
    world.write_config(grant=False)
    plain, _ = run_turn(world, probe_id("rec-plain"), [Call(CallKind.SHELL, text=command)], what)
    world.write_config(grant=True)
    # While a granted turn runs, every SQLite file its Codex holds open — each
    # store and its -wal/-shm companions — is the real home's (SQLite resolves
    # the overlay's links), but for the overlay's own thread index.
    held = held_turn(world, "rec-held")
    open_files = open_sqlite_files(world)
    outside = [t for t in open_files if not t.startswith(f"{world.home}/")]
    fails.check(bool(open_files), f"{label}: no SQLite file open in the granted turn's Codex")
    fails.check(all("run-overlay" in t and Path(t).name.startswith("state_") for t in outside),
                f"{label}: a store other than the overlay's own index is open outside the real home: {sorted(outside)}")  # fmt: skip
    fails.check(any(t.endswith("-wal") and t.startswith(str(world.home)) for t in open_files),
                f"{label}: no -wal companion open in the real home: {sorted(open_files)}")  # fmt: skip
    held.script.release()
    held.process.communicate(timeout=300)
    fails.check(not any(world.state.rglob("*.sqlite*")) and not any(world.tmp.rglob("*.sqlite*")),
                f"{label}: an index file outlived the overlay")  # fmt: skip
    fails.check(turn.result.usage == plain.result.usage,
                f"{label}: usage {turn.result.usage} differs from the ungranted run's {plain.result.usage}")  # fmt: skip
    world.endpoint.script(ProbeId("resume-direct"), [])
    proc = subprocess.run(
        ["codex", "exec", "resume", "-c", "sandbox_mode=read-only", "--json", "--skip-git-repo-check",
         thread or "", "[probe:resume-direct] again"],
        env=world.env(), cwd=world.wt, capture_output=True, text=True, timeout=300, stdin=subprocess.DEVNULL,
    )  # fmt: skip
    fails.check(proc.returncode == 0, f"{label}: codex exec resume of the granted thread failed: "
                                      f"{proc.stderr.strip().splitlines()[-1:]}")  # fmt: skip


def jwt(claims: Json) -> str:
    def encode(obj: Json) -> str:
        return base64.urlsafe_b64encode(json.dumps(obj).encode()).rstrip(b"=").decode()

    return f"{encode({'alg': 'none', 'typ': 'JWT'})}.{encode(claims)}.c2ln"


def group_login(world: World, fails: Failures) -> None:
    """Codex: a granted run that refreshes an expired file-stored ChatGPT login
    leaves the real home's auth.json holding exactly the issued tokens."""
    label = "codex login"
    now = int(time.time())
    auth = {"https://api.openai.com/auth": {"chatgpt_account_id": "acct-live", "chatgpt_plan_type": "pro",
                                            "chatgpt_user_id": "user-live"},
            "email": "live@example.invalid"}  # fmt: skip
    issued = Refresh(jwt({"exp": now + 86400, **auth}), jwt({"exp": now + 86400, "n": 1, **auth}), "rt-new")
    write_codex_home(world.home, world.endpoint.port, provider=(
        '[model_providers.scripted]\nname = "scripted"\n'
        f'base_url = "http://127.0.0.1:{world.endpoint.port}/v1"\nwire_api = "responses"\n'
        'requires_openai_auth = true\nrequest_max_retries = 0\nstream_max_retries = 0\n'))  # fmt: skip
    (world.home / "auth.json").write_text(json.dumps({
        "auth_mode": "chatgpt", "OPENAI_API_KEY": None,
        "tokens": {"id_token": jwt({"exp": now - 3600, **auth}), "access_token": jwt({"exp": now - 3600, **auth}),
                   "refresh_token": "rt-old", "account_id": "acct-live"},
        "last_refresh": "2020-01-01T00:00:00Z"}) + "\n")  # fmt: skip
    world.endpoint.refresh = issued
    world.endpoint.require_bearer = issued.access_token
    world.extra_env = {"CODEX_REFRESH_TOKEN_URL_OVERRIDE": f"http://127.0.0.1:{world.endpoint.port}/oauth/token"}
    world.drop_env = frozenset({"CODEX_SCRIPTED_KEY"})
    try:
        command = f"{world.grant} --ruling login"
        turn, _ = run_turn(world, probe_id("login"), [Call(CallKind.SHELL, text=command)],
                           f"Use your shell tool to run exactly: {command}")  # fmt: skip
        stored = json.loads((world.home / "auth.json").read_text())
        stored = stored if isinstance(stored, dict) else {}
        fails.check(len(world.endpoint.refreshed_with) >= 1, f"{label}: the granted run never refreshed "
                                                             f"({turn.stderr[-300:]})")  # fmt: skip
        fails.check(not (world.home / "auth.json").is_symlink(), f"{label}: the real auth.json became a link")
        tokens = stored.get("tokens")
        tokens = tokens if isinstance(tokens, dict) else {}
        fails.check(tokens.get("access_token") == issued.access_token and tokens.get("refresh_token") == "rt-new",
                    f"{label}: the real home's auth.json does not hold the issued tokens")  # fmt: skip
        refreshes = len(world.endpoint.refreshed_with)
        world.write_config(grant=False)
        _, plain = run_turn(world, probe_id("login-plain"), [], "Reply done.")
        world.write_config(grant=True)
        fails.check(bool(plain.requests) and len(world.endpoint.refreshed_with) == refreshes,
                    f"{label}: the later ungranted run was not accepted with the new token without a refresh "
                    f"(refreshes {refreshes} -> {len(world.endpoint.refreshed_with)}; "
                    f"rejected {world.endpoint.rejected})")  # fmt: skip
    finally:
        world.extra_env = {}
        world.drop_env = frozenset()
        world.endpoint.require_bearer = None
        write_codex_home(world.home, world.endpoint.port)


def group_keyring(world: World, fails: Failures) -> None:
    label = "codex keyring"
    config = world.home / "config.toml"
    original = config.read_text()
    config.write_text('cli_auth_credentials_store = "keyring"\n' + original)
    try:
        before = len(records(world))
        command = f"{world.grant} --ruling kr"
        turn, _ = run_turn(world, probe_id("kr"), [Call(CallKind.SHELL, text=command)],
                           f"Use your shell tool to run exactly: {command}")  # fmt: skip
        diags = turn.result.grant_diagnostics()
        fails.check(len(diags) == 1 and "keyring" in str(diags[0].get("message")), f"{label}: diagnostics {diags}")
        fails.check(not turn.result.overlaid, f"{label}: the withheld run still ran on an overlay")
        fails.check(len(records(world)) == before, f"{label}: the withheld run executed the granted command")
    finally:
        config.write_text(original)


class Outcome(NamedTuple):
    """What one turn changed: the files, and the granted executable's records."""

    changed: list[str]
    records: list[list[str]]


def group_withheld(world: World, fails: Failures) -> None:
    """A harness that withholds the grant (OpenCode) runs exactly as it does
    with none, on its real CLI, and its report says so."""
    label = f"{world.harness} withheld"
    for probe in probes():
        if probe.name not in ("granted-plain", "touch-worktree", "redirect-out"):
            continue
        outcomes = []
        for grant in (True, False):
            world.write_config(grant=grant)
            before, before_records = snapshot(world), records(world)
            call = probe.call(world)
            turn, script = run_turn(world, probe_id(probe.name), [call], describe(call))
            changed = changed_between(before, snapshot(world))
            outcomes.append(Outcome([c.replace(str(world.wt), "") for c in changed],
                                    records(world)[len(before_records) :]))  # fmt: skip
            fails.check(script.decision() is not None, f"{label} {probe.name}: no decision recorded")
            fails.check(len(turn.result.grant_diagnostics()) == (1 if grant else 0),
                        f"{label} {probe.name}: grant={grant} diagnostics {turn.result.grant_diagnostics()}")  # fmt: skip
            fails.check(not turn.result.overlaid, f"{label} {probe.name}: a withheld run used an overlay")
            # Undo the turn's effects so both runs start from the same state.
            for path in changed:
                if Path(path).parent == world.wt:
                    Path(path).unlink(missing_ok=True)
            kept = world.log.read_text().splitlines()[: len(before_records)]
            world.log.write_text("".join(line + "\n" for line in kept))
        fails.check(outcomes[0] == outcomes[1],
                    f"{label} {probe.name}: the withheld run did {outcomes[0]}, the run without a grant {outcomes[1]}")  # fmt: skip
    world.write_config(grant=True)


def budget_turns(world: World, size: int, grant: bool) -> list[TurnTiming]:
    """`size` controlled turns started at once, each timed from its oneharness
    process's start to its own exit."""
    world.write_config(grant=grant)
    scripts, argvs = [], []
    for _ in range(size):
        probe = probe_id("budget")
        command = f"{world.grant} --ruling 'budget'"
        scripts.append(world.endpoint.script(probe, [Call(CallKind.SHELL, text=command)]))
        argvs.append(world.argv(f"[probe:{probe}] Use your shell tool to run exactly: {command}"))
    turns = []
    for script, done in zip(scripts, run_concurrently(argvs, world.env(), timeout=900)):
        result = Result.of(report_of(done.stdout))
        if result.status != "ok":
            raise RuntimeError(f"a budget turn ended {result.status}: {done.stderr[-400:]}")
        first = script.requests[0].at if script.requests else None
        turns.append(TurnTiming(done.ended - done.started, done.started, done.ended, first, result.overlay))
    return turns


def budget_workload(world: World, path: Path) -> None:
    """Ungranted controlled turns on a warm pooled server, then 1, 2 and 20
    granted ones."""
    budget_turns(world, 1, grant=False)  # warm the pooled server with one earlier lease
    ungranted = {size: budget_turns(world, size, grant=False) for size in BATCHES}
    granted = {size: budget_turns(world, size, grant=True) for size in BATCHES}
    version = subprocess.run(["codex", "--version"], capture_output=True, text=True, check=False).stdout.strip()
    telemetry = Telemetry(version, sys.platform, os.getloadavg(), ungranted, granted)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(telemetry.to_json(), indent=1) + "\n")


def groups_for(world: World, fails: Failures) -> list[tuple[str, Callable[[World], None]]]:
    def run(groups: dict[str, Callable[[World], None]]) -> list[tuple[str, Callable[[World], None]]]:
        return list(groups.items())

    match world.harness, world.control:
        case "opencode", _:
            return run({"withheld": lambda w: group_withheld(w, fails)})
        case harness, control:
            groups = {
                "sync": lambda w: group_sync(w, fails),
                "probes": lambda w: group_probes(w, fails, (Expect.RUN, Expect.REFUSE)),
                "posture": lambda w: group_probes(w, fails, (Expect.ALLOW,)),
                "lifecycle": lambda w: group_lifecycle(w, fails),
                "kill": lambda w: group_kill(w, fails),
                "concurrency": lambda w: group_concurrency(w, fails),
                "session": lambda w: group_session(w, fails),
            }
            if harness == "codex" and not control:
                groups |= {
                    "record": lambda w: group_record(w, fails),
                    "login": lambda w: group_login(w, fails),
                    "keyring": lambda w: group_keyring(w, fails),
                }
            return run(groups)


def usage_error(message: str) -> int:
    print(f"run_allowlist_live: {message}", file=sys.stderr)
    return 2


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--harness", default=",".join(HARNESSES))
    parser.add_argument("--codex-path", default="exec", choices=("exec", "control", "both"))
    parser.add_argument("--only", default="")
    parser.add_argument("--scratch", type=Path)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--budget-telemetry", type=Path)
    args = parser.parse_args(argv)
    harnesses = [h for h in args.harness.split(",") if h]
    only = {g for g in args.only.split(",") if g}
    if not harnesses or any(h not in HARNESSES for h in harnesses):
        return usage_error(f"--harness names {args.harness!r}; each must be one of {', '.join(HARNESSES)}")
    if any(g not in GROUPS for g in only):
        return usage_error(f"--only names {args.only!r}; each must be one of {', '.join(GROUPS)}")
    if not Path(oneharness_bin()).is_file():
        return usage_error(f"no oneharness binary at {oneharness_bin()}; run 'just build-release' or set ONEHARNESS_BIN")
    # Resolved once and used as resolved: the spelling validated is the one
    # every command is built from.
    root = (args.scratch or Path(os.environ.get("ONEPIPELINE_NODE_SCRATCH_DIR") or tempfile.gettempdir())).resolve()
    if not SAFE_PATH.fullmatch(str(root)):
        # Every probe types paths under the scratch root into a real shell, and
        # a grant matches only an executable's path typed exactly as written —
        # quoting one would change what the harness checks.
        return usage_error(f"the scratch root {root} must be a path of letters, digits, `_`, `.`, `-` and `/` only")
    base = root if args.scratch else Path(tempfile.mkdtemp(prefix="oh-run-allowlist-", dir=root))
    endpoint = Endpoint()
    fails = Failures()
    worlds: list[World] = []
    try:
        if args.budget_telemetry is not None:
            world = World("codex", base / "budget", endpoint, control=True).make()
            worlds.append(world)
            budget_workload(world, args.budget_telemetry)
            return 0
        for harness in harnesses:
            paths = {"exec": [False], "control": [True], "both": [False, True]}[args.codex_path]
            for control in paths if harness == "codex" else [False]:
                tag = f"{harness}{'-control' if control else ''}"
                worlds.append(World(harness, base / tag, endpoint, control=control).make())
        ran = 0
        for world in worlds:
            started, count = time.time(), len(fails.items)
            for group, run in groups_for(world, fails):
                if only and group not in only:
                    continue
                ran += 1
                try:
                    run(world)
                except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
                    fails.check(False, f"{world.label} {group}: crashed: {error!r}")
            if len(fails.items) > count:
                print(f"{world.label}: {len(fails.items) - count} failure(s) in {time.time() - started:.0f}s",
                      file=sys.stderr, flush=True)  # fmt: skip
        if ran == 0:
            return usage_error(f"--only {args.only!r} selects no group any selected harness runs")
    finally:
        endpoint.stop()
        for world in worlds:
            shutil.rmtree(world.sessions, ignore_errors=True)
        if not args.keep and args.scratch is None:
            shutil.rmtree(base, ignore_errors=True)
    if fails.items:
        print(f"run_allowlist_live: {len(fails.items)} failure(s)", file=sys.stderr)
        return 1
    print(f"run_allowlist_live: ok ({', '.join(w.label for w in worlds)})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
