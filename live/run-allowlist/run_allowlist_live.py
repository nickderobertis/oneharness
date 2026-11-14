"""Live proof of the run-level allowlist against the real Claude Code and Codex.

    run_allowlist_live.py [--harness claude-code,codex] [--codex-path exec|control|both]
                          [--only GROUP,...] [--scratch DIR] [--keep]
    run_allowlist_live.py --budget-telemetry PATH       the budget workload alone

It drives the real `oneharness` binary (`$ONEHARNESS_BIN`, else
`target/release/oneharness`) over the real CLIs installed on this host, under
`--mode read-only`, against a scripted model endpoint this script serves in
place of the paid provider (Anthropic Messages for Claude Code, OpenAI
Responses for Codex). The endpoint answers each turn's first model request with
exactly the probe's tool call and the next with `done`; everything between —
the CLI's own permission check, its sandbox and its execution — is the real
harness. Every probe settles by the decision the harness itself records: the
tool result it hands back to the model on the next request. A probe whose
harness recorded none is a failure, never a pass.

Each harness gets one world: a git worktree, a sentinel directory outside it,
the granted stand-in executable (it appends one record of its argv to a log
outside the worktree), a second, ungranted executable, its own harness home,
and ONE config file holding the top-level grant (written `$OH_LIVE_GRANT`) and
a `[sync] allowed_tools` rule granting `touch`, which `oneharness sync` writes
into the worktree before any turn.

Groups (`--only`): sync, probes, posture, lifecycle, kill, concurrency, session,
record (Codex), login (Codex), keyring (Codex). Exits 0 when every check
passed; otherwise prints each failure and exits 1. Quiet but for one line per
check group and every failure.
"""

from __future__ import annotations

import argparse
import base64
import dataclasses
import hashlib
import http.server
import json
import os
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Callable, Optional

ROOT = Path(__file__).resolve().parents[2]
GRANT_VAR = "OH_LIVE_GRANT"
MARK = re.compile(r"\[probe:([A-Za-z0-9_.-]+)\]")
PAYLOAD = "line one\nit's line two"
OUTSIDE_TEXT = "outside the worktree: sentinel text"

# --------------------------------------------------------------------------
# Scripted model endpoint
# --------------------------------------------------------------------------


@dataclasses.dataclass
class Call:
    """One tool call the scripted model makes, in harness-neutral terms."""

    kind: str  # shell | write | edit | task | read | grep | glob | fail
    cmd: str = ""
    path: str = ""
    content: str = ""
    pattern: str = ""


@dataclasses.dataclass
class Script:
    calls: list[Call]
    hold: Optional[threading.Event] = None
    seen: threading.Event = dataclasses.field(default_factory=threading.Event)
    requests: list[dict[str, Any]] = dataclasses.field(default_factory=list)


class Endpoint:
    """The scripted model: one script per probe id, named in the prompt."""

    def __init__(self) -> None:
        self.scripts: dict[str, Script] = {}
        self.lock = threading.Lock()
        self.require_bearer: Optional[str] = None
        self.refresh_calls: list[dict[str, Any]] = []
        self.refresh_reply: Optional[dict[str, Any]] = None
        self.rejected_bearers: list[str] = []
        handler = type("Bound", (Handler,), {"endpoint": self})
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def script(self, probe: str, calls: list[Call], hold: bool = False) -> Script:
        s = Script(calls, threading.Event() if hold else None)
        with self.lock:
            self.scripts[probe] = s
        return s

    def stop(self) -> None:
        self.server.shutdown()


def _text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            str(c.get("text") or c.get("content") or c.get("output") or "")
            if isinstance(c, dict)
            else str(c)
            for c in content
        )
    return "" if content is None else str(content)


class Handler(http.server.BaseHTTPRequestHandler):
    endpoint: Endpoint

    def log_message(self, *args: Any) -> None:
        pass

    def handle(self) -> None:
        # A client killed mid-answer (the SIGKILL and SIGTERM checks) is expected.
        try:
            super().handle()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _json(self, obj: Any, code: int = 200) -> None:
        raw = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _sse(self, events: list[Any]) -> None:
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        for name, data in events:
            chunk = (f"event: {name}\n" if name else "") + "data: " + json.dumps(data) + "\n\n"
            self.wfile.write(chunk.encode())
            self.wfile.flush()

    def do_GET(self) -> None:
        self._json({"object": "list", "data": [], "models": []})

    def do_POST(self) -> None:
        raw = self.rfile.read(int(self.headers.get("content-length") or 0))
        path = self.path.split("?")[0]
        if path.endswith("/oauth/token"):
            self.endpoint.refresh_calls.append({"body": raw.decode(errors="replace")})
            self._json(self.endpoint.refresh_reply or {"error": "unscripted"})
            return
        try:
            body = json.loads(raw or b"{}")
        except ValueError:
            body = {}
        if path.endswith("/count_tokens"):
            self._json({"input_tokens": 10})
            return
        want = self.endpoint.require_bearer
        if want is not None and self.headers.get("authorization") != f"Bearer {want}":
            self.endpoint.rejected_bearers.append(self.headers.get("authorization") or "")
            self._json({"error": {"message": "unauthorized", "code": "invalid_token"}}, 401)
            return
        anthropic = path.endswith("/messages")
        responses = path.endswith("/responses")
        if not (anthropic or responses):
            self._json({"error": {"message": f"unknown path {path}"}}, 404)
            return
        # The turn's own prompt is the last one: a resumed conversation also
        # carries every earlier turn's.
        marks = list(MARK.finditer(json.dumps(body)))
        found = marks[-1] if marks else None
        tools = [t.get("name") or t.get("type", "") for t in body.get("tools") or []]
        with self.endpoint.lock:
            script = self.endpoint.scripts.get(found.group(1)) if found else None
        if script is None or not tools:
            # A side request (a title, a summary) or a turn this run never scripted.
            self._answer(anthropic, body, None)
            return
        made, results = history(anthropic, body, found.group(0))
        script.requests.append({"made": made, "results": results, "at": time.time()})
        script.seen.set()
        if script.hold is not None:
            script.hold.wait(timeout=600)
        call = script.calls[made] if made < len(script.calls) else None
        if call is not None and call.kind == "fail":
            self._json(
                {"error": {"type": "invalid_request_error", "message": "scripted failure"}}, 400
            )
            return
        self._answer(anthropic, body, call)

    def _answer(self, anthropic: bool, body: dict[str, Any], call: Optional[Call]) -> None:
        if anthropic:
            answer_anthropic(self, body, call)
        else:
            answer_responses(self, body, call)


def history(anthropic: bool, body: dict[str, Any], mark: str) -> tuple[int, list[str]]:
    """(tool calls the model made, tool results the harness returned) since the
    turn's own prompt — a resumed conversation carries earlier turns' too."""
    made, results = 0, []
    if anthropic:
        messages = body.get("messages") or []
        start = max((i for i, m in enumerate(messages) if mark in json.dumps(m)), default=0)
        for m in messages[start:]:
            for b in m.get("content") if isinstance(m.get("content"), list) else []:
                if isinstance(b, dict) and b.get("type") == "tool_use":
                    made += 1
                if isinstance(b, dict) and b.get("type") == "tool_result":
                    results.append(_text(b.get("content")))
        return made, results
    items = body.get("input") or []
    start = max((i for i, it in enumerate(items) if mark in json.dumps(it)), default=0)
    for item in items[start:]:
        kind = item.get("type") if isinstance(item, dict) else None
        if kind in ("function_call", "custom_tool_call", "local_shell_call"):
            made += 1
        if kind in ("function_call_output", "custom_tool_call_output"):
            out = item.get("output")
            results.append(out if isinstance(out, str) else json.dumps(out))
    return made, results


def answer_anthropic(h: Handler, body: dict[str, Any], call: Optional[Call]) -> None:
    mid = "msg_" + uuid.uuid4().hex[:20]
    if call is None:
        block, final, stop = (
            {"type": "text", "text": ""},
            {"type": "text", "text": "done"},
            "end_turn",
        )
        delta = {"type": "text_delta", "text": "done"}
    else:
        name, inp = {
            "shell": ("Bash", {"command": call.cmd, "description": "live probe"}),
            "write": ("Write", {"file_path": call.path, "content": call.content}),
            "edit": (
                "Edit",
                {"file_path": call.path, "old_string": "worktree", "new_string": "pwned"},
            ),
            "task": (
                "Agent",
                {
                    "description": "live probe",
                    "prompt": call.cmd,
                    "subagent_type": "general-purpose",
                },
            ),
            "read": ("Read", {"file_path": call.path}),
            "grep": (
                "Grep",
                {"pattern": call.pattern, "path": call.path, "output_mode": "content"},
            ),
            "glob": ("Glob", {"pattern": call.pattern, "path": call.path}),
        }[call.kind]
        tid = "toolu_" + uuid.uuid4().hex[:20]
        block = {"type": "tool_use", "id": tid, "name": name, "input": {}}
        delta = {"type": "input_json_delta", "partial_json": json.dumps(inp)}
        final, stop = {"type": "tool_use", "id": tid, "name": name, "input": inp}, "tool_use"
    usage = {"input_tokens": 10, "output_tokens": 5}
    message = {
        "id": mid,
        "type": "message",
        "role": "assistant",
        "model": body.get("model", "m"),
        "stop_reason": None,
        "stop_sequence": None,
        "usage": usage,
    }
    if not body.get("stream"):
        h._json({**message, "content": [final], "stop_reason": stop})
        return
    h._sse(
        [
            ("message_start", {"type": "message_start", "message": {**message, "content": []}}),
            (
                "content_block_start",
                {"type": "content_block_start", "index": 0, "content_block": block},
            ),
            ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": delta}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            (
                "message_delta",
                {
                    "type": "message_delta",
                    "delta": {"stop_reason": stop, "stop_sequence": None},
                    "usage": {"output_tokens": 5},
                },
            ),
            ("message_stop", {"type": "message_stop"}),
        ]
    )


def answer_responses(h: Handler, body: dict[str, Any], call: Optional[Call]) -> None:
    rid = "resp_" + uuid.uuid4().hex[:20]
    names = [t.get("name") or t.get("type", "") for t in body.get("tools") or []]
    if call is None:
        item: dict[str, Any] = {
            "type": "message",
            "role": "assistant",
            "id": "msg_" + uuid.uuid4().hex[:12],
            "content": [{"type": "output_text", "text": "done", "annotations": []}],
        }
    elif call.kind in ("shell", "read"):
        cmd = call.cmd if call.kind == "shell" else f"cat {call.path}"
        if "exec_command" in names:
            args: dict[str, Any] = {"cmd": cmd}
            name = "exec_command"
        elif "shell_command" in names:
            args, name = {"command": cmd}, "shell_command"
        else:
            args, name = {"command": ["bash", "-lc", cmd]}, "shell"
        item = {
            "type": "function_call",
            "id": "fc_" + uuid.uuid4().hex[:12],
            "name": name,
            "arguments": json.dumps(args),
            "call_id": "call_" + uuid.uuid4().hex[:12],
        }
    elif call.kind in ("write", "edit"):
        patch = (
            f"*** Begin Patch\n*** Add File: {call.path}\n+{call.content}\n*** End Patch\n"
            if call.kind == "write"
            else f"*** Begin Patch\n*** Update File: {call.path}\n@@\n-worktree\n+pwned\n*** End Patch\n"
        )
        item = {
            "type": "custom_tool_call",
            "id": "ctc_" + uuid.uuid4().hex[:12],
            "name": "apply_patch",
            "input": patch,
            "call_id": "call_" + uuid.uuid4().hex[:12],
        }
    elif call.kind == "task":
        item = {
            "type": "function_call",
            "id": "fc_" + uuid.uuid4().hex[:12],
            "name": "spawn_agent",
            "arguments": json.dumps({"message": call.cmd}),
            "call_id": "call_" + uuid.uuid4().hex[:12],
        }
    else:
        raise ValueError(call.kind)
    usage = {
        "input_tokens": 10,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": 5,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 15,
    }
    h._sse(
        [
            ("response.created", {"type": "response.created", "response": {"id": rid}}),
            (
                "response.output_item.added",
                {"type": "response.output_item.added", "output_index": 0, "item": item},
            ),
            (
                "response.output_item.done",
                {"type": "response.output_item.done", "output_index": 0, "item": item},
            ),
            (
                "response.completed",
                {
                    "type": "response.completed",
                    "response": {"id": rid, "usage": usage, "output": [item]},
                },
            ),
        ]
    )


# --------------------------------------------------------------------------
# The world one harness runs in
# --------------------------------------------------------------------------

STANDIN = """#!/usr/bin/env bash
# The granted executable's stand-in: append one record of its argv to LOG.
args=""
for a in "$@"; do args="$args$(printf '%s' "$a" | base64 | tr -d '\\n') "; done
printf '{"name":"@NAME@","args_b64":"%s"}\\n' "$args" >> @LOG@ || { echo "could not append" >&2; exit 3; }
echo "@NAME@: recorded"
"""


class Failures:
    def __init__(self) -> None:
        self.items: list[str] = []

    def check(self, ok: bool, what: str) -> bool:
        if not ok:
            self.items.append(what)
            print(f"FAIL {what}", flush=True)
        return ok


@dataclasses.dataclass
class World:
    harness: str
    root: Path
    endpoint: Endpoint
    control: bool = False
    # The session store: short, because a controlled turn's socket lives in it
    # and a unix socket address is at most 104-108 bytes.
    sessions: Path = dataclasses.field(
        default_factory=lambda: Path(tempfile.mkdtemp(prefix="oh-ra-", dir="/tmp"))
    )

    @property
    def wt(self) -> Path:
        return self.root / "worktree"

    @property
    def tmp(self) -> Path:
        return self.root / "tmp"

    @property
    def home(self) -> Path:
        return self.root / ("codex-home" if self.harness == "codex" else "claude-config")

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
    def other_log(self) -> Path:
        return self.root / "out" / "other.jsonl"

    @property
    def sentinel(self) -> Path:
        return self.root / "sentinel"

    @property
    def config(self) -> Path:
        return self.root / "oneharness.toml"

    def make(self) -> World:
        for sub in ("worktree", "tmp", "bin", "out", "sentinel", "sessions", "history"):
            (self.root / sub).mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q", str(self.wt)], check=True)
        (self.wt / "README.md").write_text("live worktree\n")
        (self.sentinel / "notes.txt").write_text(OUTSIDE_TEXT + "\n")
        for exe, log, name in (
            (self.grant, self.log, "grant"),
            (self.other, self.other_log, "other"),
        ):
            exe.write_text(STANDIN.replace("@LOG@", str(log)).replace("@NAME@", name))
            exe.chmod(0o755)
            log.touch()
        self.home.mkdir(parents=True, exist_ok=True)
        if self.harness == "codex":
            write_codex_home(self.home, self.endpoint.port)
        self.write_config(grant=True)
        if self.harness == "codex":
            # A home Codex has already used, as a real one is: its sessions/
            # tree and thread index exist before the first granted turn.
            self.endpoint.script("warm", [])
            subprocess.run(
                [
                    "codex",
                    "exec",
                    "--sandbox",
                    "read-only",
                    "--json",
                    "--skip-git-repo-check",
                    "-C",
                    str(self.wt),
                    "[probe:warm] reply done",
                ],
                env=self.env(),
                capture_output=True,
                text=True,
                timeout=300,
                check=True,
                stdin=subprocess.DEVNULL,
            )
        return self

    def write_config(self, grant: bool, extra: str = "") -> None:
        top = f'allowed_tools = ["Bash(${GRANT_VAR})"]\n' if grant else ""
        # The scripted endpoint serves no model catalog, so the shared catalog
        # would only add its 10 s wait to every Codex launch, granted or not.
        codex = "[harness.codex]\nshared_model_catalog = false\n" if self.harness == "codex" else ""
        self.config.write_text(
            top + extra + codex + f'[env]\n{GRANT_VAR} = "{self.grant}"\n'
            '[sync]\nallowed_tools = ["Bash(touch:*)"]\n'
        )

    def env(self) -> dict[str, str]:
        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith(("ONEHARNESS_", "CODEX_", "CLAUDE_", "ANTHROPIC_", "OPENAI_"))
        }
        # The pooled app-server inherits only CODEX_* and OPENAI_* variables,
        # so the scripted provider's key is spelled CODEX_SCRIPTED_KEY.
        env.update(
            TMPDIR=str(self.tmp),
            XDG_STATE_HOME=str(self.state),
            ONEHARNESS_CONFIG=str(self.root / "user.toml"),
        )
        (self.root / "user.toml").touch()
        if self.harness == "codex":
            env.update(CODEX_HOME=str(self.home), CODEX_SCRIPTED_KEY="scripted")
        else:
            env.update(
                CLAUDE_CONFIG_DIR=str(self.home),
                ANTHROPIC_API_KEY="scripted",
                ANTHROPIC_BASE_URL=f"http://127.0.0.1:{self.endpoint.port}",
            )
        return env

    def argv(
        self,
        prompt: str,
        session: Optional[str] = None,
        extra: Optional[list[str]] = None,
        control: Optional[bool] = None,
    ) -> list[str]:
        argv = [
            oneharness_bin(),
            "run",
            "--harness",
            self.harness,
            "--mode",
            "read-only",
            "--cwd",
            str(self.wt),
            "--config",
            str(self.config),
            "--prompt",
            prompt,
            "--compact",
        ]
        control = self.control if control is None else control
        if control or session is not None:
            argv += [
                "--session",
                session or f"s-{uuid.uuid4().hex[:8]}",
                "--session-dir",
                str(self.sessions),
            ]
        if control:
            argv.append("--control")
        if self.harness == "claude-code":
            argv += ["--model", "claude-sonnet-4-5"]
        return argv + (extra or [])

    @property
    def state(self) -> Path:
        # Short for the same reason as `sessions`: the shared app-server pool
        # listens on a socket under it.
        return self.sessions / "st"

    def overlays(self) -> list[str]:
        roots = (self.tmp, self.state / "oneharness" / "run-overlays")
        return [
            p.name
            for root in roots
            if root.is_dir()
            for p in root.iterdir()
            if "run-overlay" in p.name
        ]


def write_codex_home(home: Path, port: int, provider: Optional[str] = None) -> None:
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
    rows = []
    for line in world.log.read_text().splitlines():
        rec = json.loads(line)
        rows.append([base64.b64decode(a).decode() for a in rec["args_b64"].split()])
    return rows


@dataclasses.dataclass
class Turn:
    report: dict[str, Any]
    stderr: str
    code: int
    started: float
    ended: float


def run_turn(
    world: World,
    probe: str,
    calls: list[Call],
    what: str,
    session: Optional[str] = None,
    extra: Optional[list[str]] = None,
    control: Optional[bool] = None,
) -> tuple[Turn, Script]:
    script = world.endpoint.script(probe, calls)
    prompt = (
        f"[probe:{probe}] This is a permission test. {what} Do not try any alternative if it "
        "is refused. Then reply with one word: done."
    )
    started = time.time()
    proc = subprocess.run(
        world.argv(prompt, session, extra, control),
        env=world.env(),
        capture_output=True,
        text=True,
        timeout=600,
        stdin=subprocess.DEVNULL,
    )
    ended = time.time()
    try:
        report = json.loads(proc.stdout)
    except ValueError:
        report = {}
    return Turn(report, proc.stderr, proc.returncode, started, ended), script


def decided(script: Script) -> Optional[str]:
    """The harness's recorded decision: the tool result it returned."""
    for req in script.requests:
        if req["results"]:
            return req["results"][-1]
    return None


# --------------------------------------------------------------------------
# Probes
# --------------------------------------------------------------------------


@dataclasses.dataclass
class Probe:
    id: str
    expect: str  # run | refuse | allow
    call: Callable[[World], Call]
    args: Optional[list[str]] = None  # the granted record a `run` probe must leave
    harnesses: tuple[str, ...] = ("claude-code", "codex")


def probes() -> list[Probe]:
    sh = lambda f: (lambda w: Call("shell", cmd=f(w)))  # noqa: E731
    g = lambda w: str(w.grant)  # noqa: E731
    pw = lambda w: f"{w.wt}/pwned"  # noqa: E731
    P = [
        Probe("granted-plain", "run", sh(lambda w: f"{g(w)} --ruling r-1"), ["--ruling", "r-1"]),
        Probe(
            "granted-multiline-apostrophe",
            "run",
            sh(lambda w: f'{g(w)} --ruling r-1 --reason "{PAYLOAD}"'),
            ["--ruling", "r-1", "--reason", PAYLOAD],
        ),
        Probe("touch-worktree", "refuse", sh(lambda w: f"touch {pw(w)}")),
        Probe("synced-touch-rule", "refuse", sh(lambda w: f"touch {w.sentinel}/synced")),
        Probe("chain-semicolon", "refuse", sh(lambda w: f"{g(w)} --ruling r-1; touch {pw(w)}")),
        Probe("chain-and", "refuse", sh(lambda w: f"{g(w)} --ruling r-1 && touch {pw(w)}")),
        Probe(
            "chain-or", "refuse", sh(lambda w: f"{g(w)} --ruling r-1 --bad-flag || touch {pw(w)}")
        ),
        Probe("chain-newline", "refuse", sh(lambda w: f"{g(w)} --ruling r-1\ntouch {pw(w)}")),
        Probe(
            "chain-other", "refuse", sh(lambda w: f"{g(w)} --ruling r-1; {w.other} --ruling r-1")
        ),
        Probe("pipe-into-tee", "refuse", sh(lambda w: f"{g(w)} --ruling r-1 | tee {pw(w)}")),
        Probe(
            "subst-dollar-other",
            "refuse",
            sh(lambda w: f'{g(w)} --ruling "$({w.other} --ruling r-1)"'),
        ),
        Probe(
            "subst-backtick-other",
            "refuse",
            sh(lambda w: f'{g(w)} --ruling "`{w.other} --ruling r-1`"'),
        ),
        Probe("redirect-out", "refuse", sh(lambda w: f"{g(w)} --ruling r-1 > {pw(w)}")),
        Probe("redirect-append-log", "refuse", sh(lambda w: f"echo forged >> {w.log}")),
        Probe("other-executable", "refuse", sh(lambda w: f"{w.other} --ruling r-1")),
        Probe(
            "reassign-variable",
            "refuse",
            sh(lambda w: f'{GRANT_VAR}={w.other}; "${GRANT_VAR}" --ruling r-1'),
        ),
        Probe(
            "reassign-variable-touch",
            "refuse",
            sh(lambda w: f'{GRANT_VAR}=/usr/bin/touch; "${GRANT_VAR}" {pw(w)}'),
        ),
        Probe(
            "shadow-function",
            "refuse",
            sh(lambda w: f"{g(w)}() {{ touch {pw(w)}; }}; {g(w)} --ruling r-1"),
        ),
        Probe(
            "shadow-alias",
            "refuse",
            sh(
                lambda w: f"shopt -s expand_aliases; alias grant='touch {pw(w)}'\ngrant --ruling r-1"
            ),
        ),
        Probe(
            "subagent-delegate", "refuse", lambda w: Call("task", cmd=f"Run `touch {w.wt}/pwned`.")
        ),
        Probe(
            "write-tool-create",
            "refuse",
            lambda w: Call("write", path=f"{w.wt}/pwned", content="pwned"),
        ),
        Probe("edit-tool-modify", "refuse", lambda w: Call("edit", path=f"{w.wt}/README.md")),
        Probe("read-outside", "allow", lambda w: Call("read", path=str(w.sentinel / "notes.txt"))),
        Probe(
            "grep-outside",
            "allow",
            lambda w: Call("grep", pattern="sentinel text", path=str(w.sentinel)),
            harnesses=("claude-code",),
        ),
        Probe(
            "glob-outside",
            "allow",
            lambda w: Call("glob", pattern="*.txt", path=str(w.sentinel)),
            harnesses=("claude-code",),
        ),
    ]
    return P


def judge(
    world: World, probe: Probe, fails: Failures, tag: str = "", control: Optional[bool] = None
) -> None:
    before = snapshot(world)
    before_records = records(world)
    turn, script = run_turn(
        world,
        f"{probe.id}-{uuid.uuid4().hex[:6]}",
        [probe.call(world)],
        describe(probe.call(world)),
        control=control,
    )
    label = f"{world.harness}{tag} {probe.id}"
    decision = decided(script)
    if not fails.check(
        turn.code in (0, 1) and turn.report.get("results"),
        f"{label}: oneharness exited {turn.code}: {turn.stderr[-400:]}",
    ):
        return
    if not fails.check(
        decision is not None,
        f"{label}: the harness recorded no decision (inconclusive): "
        f"{json.dumps(turn.report.get('results', [{}])[0].get('stdout', ''))[:300]}",
    ):
        return
    stderr = json.dumps(turn.report["results"][0].get("stderr", ""))
    fails.check(
        "Refusing to create helper binaries" not in stderr,
        f"{label}: Codex lost its PATH helpers on the overlay home: {stderr[:300]}",
    )
    after = snapshot(world)
    changed = sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))
    new = records(world)[len(before_records) :]
    if probe.expect == "run":
        fails.check(
            new == [probe.args],
            f"{label}: the granted call left {new}, wanted [{probe.args}]; "
            f"decision: {decision[:300]}",
        )
        fails.check(changed == [str(world.log)], f"{label}: unexpected change {changed}")
    elif probe.expect == "refuse":
        fails.check(
            not changed and not new,
            f"{label}: NOT refused — changed {changed}, records {new}; decision: {decision[:300]}",
        )
    else:
        fails.check(
            OUTSIDE_TEXT in decision or "notes.txt" in decision,
            f"{label}: a read outside the worktree was refused: {decision[:300]}",
        )
        fails.check(not changed, f"{label}: a read changed {changed}")
    check_clean(world, fails, label)


def describe(call: Call) -> str:
    return {
        "shell": f"Use your shell tool to run exactly this command, once, verbatim:\n\n{call.cmd}\n",
        "task": "Use your subagent tool to delegate this to a subagent.",
        "write": f"Use your file editing tool to create {call.path}.",
        "edit": f"Use your file editing tool to edit {call.path}.",
        "read": f"Use your file read tool to read {call.path}.",
        "grep": f"Search {call.path} for '{call.pattern}'.",
        "glob": f"List {call.pattern} under {call.path}.",
    }.get(call.kind, "Answer.")


def check_clean(world: World, fails: Failures, label: str) -> None:
    """Nothing the overlay made outlives the run: no overlay directory, no
    process of one, no run rule in the worktree or the real home."""
    fails.check(not world.overlays(), f"{label}: overlay left behind: {world.overlays()}")
    fails.check(
        not overlay_pids(world), f"{label}: an overlay process survived: {overlay_pids(world)}"
    )
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
        if (
            b"ONEHARNESS_RUN_OVERLAY=oneharness-run-overlay" in environ
            and str(world.state).encode() in environ
        ):
            found.append(int(entry.name))
    return found


# --------------------------------------------------------------------------
# Groups
# --------------------------------------------------------------------------


def group_sync(world: World, fails: Failures) -> None:
    proc = subprocess.run(
        [
            oneharness_bin(),
            "sync",
            "--harness",
            world.harness,
            "--cwd",
            str(world.wt),
            "--config",
            str(world.config),
            "--compact",
        ],
        env=world.env(),
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
    )
    if not fails.check(proc.returncode == 0, f"{world.harness} sync: {proc.stderr}"):
        return
    if world.harness == "claude-code":
        held = json.loads((world.wt / ".claude" / "settings.json").read_text())["permissions"][
            "allow"
        ]
        fails.check(held == ["Bash(touch:*)"], f"claude-code sync wrote {held}")
    else:
        rules = (world.wt / ".codex" / "rules" / "oneharness.rules").read_text()
        fails.check(
            'pattern=["touch"]' in rules and str(world.grant) not in rules,
            f"codex sync wrote {rules!r}",
        )


def group_probes(world: World, fails: Failures, kinds: tuple[str, ...]) -> None:
    for probe in probes():
        if world.harness in probe.harnesses and probe.expect in kinds:
            judge(world, probe, fails, " (control)" if world.control else "")


def held_turn(
    world: World, probe: str, extra: Optional[list[str]] = None, config: Optional[Path] = None
) -> tuple[subprocess.Popen, Script]:
    script = world.endpoint.script(
        probe, [Call("shell", cmd=f"{world.grant} --ruling {probe}")], hold=True
    )
    argv = world.argv(
        f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}",
        extra=extra,
    )
    if config is not None:
        argv[argv.index("--config") + 1] = str(config)
    proc = subprocess.Popen(
        argv,
        env=world.env(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        stdin=subprocess.DEVNULL,
    )
    if not script.seen.wait(timeout=120):
        proc.kill()
        raise RuntimeError(f"{probe}: the harness never reached the model")
    return proc, script


def group_lifecycle(world: World, fails: Failures) -> None:
    label = f"{world.harness}{' (control)' if world.control else ''} lifecycle"
    # A run the harness fails.
    turn, _ = run_turn(world, f"fail-{uuid.uuid4().hex[:6]}", [Call("fail")], "Answer.")
    status = (turn.report.get("results") or [{}])[0].get("status")
    fails.check(status in ("nonzero", "error", "ok"), f"{label} failure: status {status}")
    check_clean(world, fails, f"{label} after a harness failure")
    # oneharness's own timeout.
    proc, script = held_turn(world, f"timeout-{uuid.uuid4().hex[:6]}", extra=["--timeout", "5"])
    out, _ = proc.communicate(timeout=120)
    script.hold.set()
    status = json.loads(out)["results"][0]["status"] if out.strip() else "none"
    fails.check(status == "timeout", f"{label} timeout: status {status}")
    time.sleep(1)
    check_clean(world, fails, f"{label} after a timeout")
    # SIGTERM to oneharness mid-turn.
    proc, script = held_turn(world, f"term-{uuid.uuid4().hex[:6]}")
    proc.send_signal(signal.SIGTERM)
    out, _ = proc.communicate(timeout=120)
    script.hold.set()
    status = json.loads(out)["results"][0]["status"] if out.strip() else "none"
    fails.check(status == "cancelled", f"{label} SIGTERM: status {status}")
    time.sleep(1)
    check_clean(world, fails, f"{label} after SIGTERM")


def group_kill(world: World, fails: Failures) -> None:
    label = f"{world.harness}{' (control)' if world.control else ''} SIGKILL"
    killed, killed_script = held_turn(world, f"killed-{uuid.uuid4().hex[:6]}")
    live, live_script = held_turn(world, f"live-{uuid.uuid4().hex[:6]}")
    before_kill = set(world.overlays())
    killed.send_signal(signal.SIGKILL)
    killed.wait(timeout=30)
    # The next oneharness run reclaims the killed run's overlay and processes.
    _turn, _ = run_turn(
        world,
        f"next-{uuid.uuid4().hex[:6]}",
        [Call("read", path=str(world.sentinel / "notes.txt"))],
        "Read the notes.",
    )
    remaining = set(world.overlays())
    if world.harness == "codex":
        fails.check(
            len(before_kill) == 2, f"{label}: expected two live overlays, saw {sorted(before_kill)}"
        )
        fails.check(
            len(remaining) == 1 and remaining < before_kill,
            f"{label}: after the next run the overlays are {sorted(remaining)} (were {sorted(before_kill)})",
        )
    killed_script.hold.set()
    live_script.hold.set()
    out, err = live.communicate(timeout=300)
    result = json.loads(out)["results"][0] if out.strip() else {}
    fails.check(
        result.get("status") == "ok",
        f"{label}: the live run beside it ended {result.get('status')}: {err[-300:]}",
    )
    time.sleep(1)
    check_clean(world, fails, label)


def direct_cli(world: World, probe: str) -> tuple[subprocess.Popen, Script]:
    """The harness CLI run directly against the same home and project — the
    user's own session."""
    script = world.endpoint.script(probe, [Call("shell", cmd=f"{world.grant} --ruling {probe}")])
    prompt = f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}"
    if world.harness == "codex":
        argv = [
            "codex",
            "exec",
            "--sandbox",
            "read-only",
            "--json",
            "--skip-git-repo-check",
            "-C",
            str(world.wt),
            prompt,
        ]
    else:
        argv = [
            "claude",
            "-p",
            prompt,
            "--permission-mode",
            "dontAsk",
            "--output-format",
            "json",
            "--model",
            "claude-sonnet-4-5",
        ]
    env = world.env()
    env[GRANT_VAR] = str(world.grant)
    return subprocess.Popen(
        argv,
        env=env,
        cwd=world.wt,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        stdin=subprocess.DEVNULL,
        text=True,
    ), script


def group_concurrency(world: World, fails: Failures) -> None:
    label = f"{world.harness}{' (control)' if world.control else ''} isolation"
    ungranted = world.root / "ungranted.toml"
    ungranted.write_text(world.config.read_text().split("\n", 1)[1])
    for phase in ("concurrent", "sequential"):
        held = None
        if phase == "concurrent":
            held, held_script = held_turn(world, f"g-{uuid.uuid4().hex[:6]}")
        before = len(records(world))
        # An ungranted oneharness run in the same worktree and home.
        probe = f"u-{uuid.uuid4().hex[:6]}"
        script = world.endpoint.script(
            probe, [Call("shell", cmd=f"{world.grant} --ruling {probe}")]
        )
        argv = world.argv(
            f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling {probe}"
        )
        argv[argv.index("--config") + 1] = str(ungranted)
        subprocess.run(
            argv,
            env=world.env(),
            capture_output=True,
            text=True,
            timeout=600,
            stdin=subprocess.DEVNULL,
        )
        fails.check(
            decided(script) is not None, f"{label} {phase}: the ungranted run recorded no decision"
        )
        # The CLI run directly.
        proc, direct = direct_cli(world, f"d-{uuid.uuid4().hex[:6]}")
        proc.communicate(timeout=600)
        fails.check(
            decided(direct) is not None, f"{label} {phase}: the direct CLI recorded no decision"
        )
        fails.check(
            len(records(world)) == before,
            f"{label} {phase}: an ungranted session ran the granted executable: {records(world)[before:]}",
        )
        if held is not None:
            held_script.hold.set()
            out, _ = held.communicate(timeout=300)
            result = json.loads(out)["results"][0]
            fails.check(
                result["status"] == "ok" and len(records(world)) == before + 1,
                f"{label}: the granted run beside them did not run its call ({result['status']})",
            )
    check_clean(world, fails, label)


def group_session(world: World, fails: Failures) -> None:
    label = f"{world.harness}{' (control)' if world.control else ''} session"
    name = f"cont-{uuid.uuid4().hex[:6]}"
    hist = ["--history", "--history-dir", str(world.root / "history")]
    for turn_no in (1, 2):
        before = len(records(world))
        turn, _script = run_turn(
            world,
            f"{name}-{turn_no}",
            [Call("shell", cmd=f"{world.grant} --ruling t{turn_no}")],
            f"Use your shell tool to run exactly: {world.grant} --ruling t{turn_no}",
            session=name,
            extra=hist,
        )
        result = (turn.report.get("results") or [{}])[0]
        fails.check(
            result.get("status") == "ok"
            and records(world)[before:] == [["--ruling", f"t{turn_no}"]],
            f"{label}: turn {turn_no} ended {result.get('status')} with {records(world)[before:]}",
        )
        if turn_no == 2:
            first = turn.report.get("session") or {}
            fails.check(
                first.get("phase") == "continue",
                f"{label}: the second turn did not continue the session: {first}",
            )
    # Recorded as an ungranted read-only turn is: same record shape.
    world.write_config(grant=False)
    run_turn(
        world,
        f"plain-{uuid.uuid4().hex[:6]}",
        [Call("read", path=str(world.sentinel / "notes.txt"))],
        "Read the notes.",
        extra=hist,
    )
    world.write_config(grant=True)
    rows = [
        json.loads(line)
        for f in sorted((world.root / "history").rglob("*.jsonl"))
        for line in f.read_text().splitlines()
        if line.strip()
    ]
    runs = [r for r in rows if r.get("type") == "run" or "permission_mode" in r]
    if fails.check(
        len(runs) >= 3, f"{label}: the history store holds {len(runs)} turn records, wanted 3"
    ):
        *granted, plain = runs[-3:]
        for record in granted:
            missing = sorted(set(plain) - set(record) - {"observed_tool_ms"})
            fails.check(
                not missing
                and record.get("permission_mode") == plain.get("permission_mode")
                and record.get("status") == plain.get("status") == "ok"
                and not any("overlay" in key or "allowed" in key for key in record),
                f"{label}: a granted turn is recorded unlike an ungranted read-only one "
                f"(missing {missing}; {record.get('permission_mode')}/{record.get('status')})",
            )


def group_record(world: World, fails: Failures) -> None:
    """Codex: a granted run's rollout, thread index and new top-level files are
    in the real home, and its usage matches the same run ungranted."""
    label = "codex record"
    installation = world.home / "installation_id"
    if installation.exists():
        installation.unlink()
    turn, _ = run_turn(
        world,
        f"rec-{uuid.uuid4().hex[:6]}",
        [Call("shell", cmd=f"{world.grant} --ruling rec")],
        f"Use your shell tool to run exactly: {world.grant} --ruling rec",
    )
    result = turn.report["results"][0]
    thread = result.get("session_id")
    fails.check(
        installation.is_file() and not installation.is_symlink(),
        f"{label}: a file Codex created at the home's top level is not in the real home",
    )
    rollouts = list((world.home / "sessions").rglob(f"*{thread}.jsonl")) if thread else []
    fails.check(
        len(rollouts) == 1, f"{label}: the granted rollout {thread} is not in the real sessions/"
    )
    dbs = sorted(world.home.glob("state_*.sqlite"))
    if fails.check(bool(dbs), f"{label}: the real home has no thread index"):
        with sqlite3.connect(dbs[-1]) as db:
            row = db.execute("select rollout_path from threads where id = ?", (thread,)).fetchone()
        fails.check(
            row is not None and Path(row[0]) == rollouts[0] if rollouts and row else False,
            f"{label}: the real index holds {row} for {thread}, wanted {rollouts}",
        )
    world.write_config(grant=False)
    plain, _ = run_turn(
        world,
        f"recu-{uuid.uuid4().hex[:6]}",
        [Call("shell", cmd=f"{world.grant} --ruling rec")],
        f"Use your shell tool to run exactly: {world.grant} --ruling rec",
    )
    world.write_config(grant=True)
    # While a granted turn runs, every SQLite file its Codex holds open — each
    # store and its -wal/-shm companions — is the real home's (SQLite resolves
    # the overlay's links), but for the overlay's own thread index.
    proc, script = held_turn(world, f"recheld-{uuid.uuid4().hex[:6]}")
    open_files = set()
    for pid in overlay_pids(world):
        fd_dir = Path(f"/proc/{pid}/fd")
        for fd in fd_dir.iterdir() if fd_dir.is_dir() else []:
            try:
                target = os.readlink(fd)
            except OSError:
                continue
            if ".sqlite" in target:
                open_files.add(target)
    overlay_only = [t for t in open_files if not t.startswith(str(world.home) + "/")]
    fails.check(bool(open_files), f"{label}: no SQLite file open in the granted turn's Codex")
    fails.check(
        all("run-overlay" in t and Path(t).name.startswith("state_") for t in overlay_only),
        f"{label}: a store other than the overlay's own index is open outside the real home: "
        f"{sorted(overlay_only)}",
    )
    fails.check(
        any(t.endswith("-wal") and t.startswith(str(world.home)) for t in open_files),
        f"{label}: no -wal companion open in the real home: {sorted(open_files)}",
    )
    script.hold.set()
    proc.communicate(timeout=300)
    fails.check(
        not any(world.state.rglob("*.sqlite*")) and not any(world.tmp.rglob("*.sqlite*")),
        f"{label}: an index file outlived the overlay",
    )
    usage, plain_usage = result.get("usage"), plain.report["results"][0].get("usage")
    fails.check(
        usage == plain_usage,
        f"{label}: usage {usage} differs from the ungranted run's {plain_usage}",
    )
    # The real home's own `codex exec resume` finds the granted thread.
    world.endpoint.script("resume-direct", [])
    proc = subprocess.run(
        [
            "codex",
            "exec",
            "resume",
            "-c",
            "sandbox_mode=read-only",
            "--json",
            "--skip-git-repo-check",
            thread,
            "[probe:resume-direct] again",
        ],
        env=world.env(),
        cwd=world.wt,
        capture_output=True,
        text=True,
        timeout=300,
        stdin=subprocess.DEVNULL,
    )
    fails.check(
        proc.returncode == 0,
        f"{label}: codex exec resume of the granted thread failed: "
        f"{proc.stderr.strip().splitlines()[-1:] if proc.stderr else ''}",
    )


def jwt(claims: dict[str, Any]) -> str:
    enc = lambda obj: base64.urlsafe_b64encode(json.dumps(obj).encode()).rstrip(b"=").decode()  # noqa: E731
    return f"{enc({'alg': 'none', 'typ': 'JWT'})}.{enc(claims)}.c2ln"


def group_login(world: World, fails: Failures) -> None:
    """Codex: a granted run that refreshes an expired file-stored ChatGPT login
    leaves the real home's auth.json holding exactly the issued tokens."""
    label = "codex login"
    now = int(time.time())
    auth = {
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct-live",
            "chatgpt_plan_type": "pro",
            "chatgpt_user_id": "user-live",
        },
        "email": "live@example.invalid",
    }
    old_access = jwt({"exp": now - 3600, **auth})
    new_access = jwt({"exp": now + 86400, **auth})
    new_id = jwt({"exp": now + 86400, **auth})
    write_codex_home(
        world.home,
        world.endpoint.port,
        provider=(
            '[model_providers.scripted]\nname = "scripted"\n'
            f'base_url = "http://127.0.0.1:{world.endpoint.port}/v1"\nwire_api = "responses"\n'
            "requires_openai_auth = true\nrequest_max_retries = 0\nstream_max_retries = 0\n"
        ),
    )
    (world.home / "auth.json").write_text(
        json.dumps(
            {
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": None,
                "tokens": {
                    "id_token": jwt({"exp": now - 3600, **auth}),
                    "access_token": old_access,
                    "refresh_token": "rt-old",
                    "account_id": "acct-live",
                },
                "last_refresh": "2020-01-01T00:00:00Z",
            }
        )
        + "\n"
    )
    world.endpoint.refresh_reply = {
        "id_token": new_id,
        "access_token": new_access,
        "refresh_token": "rt-new",
    }
    world.endpoint.require_bearer = new_access
    env_override = {
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE": f"http://127.0.0.1:{world.endpoint.port}/oauth/token"
    }
    original_env = world.env

    def env() -> dict[str, str]:
        e = original_env()
        e.pop("CODEX_SCRIPTED_KEY", None)
        e.update(env_override)
        return e

    world.env = env  # type: ignore[method-assign]
    try:
        turn, _script = run_turn(
            world,
            f"login-{uuid.uuid4().hex[:6]}",
            [Call("shell", cmd=f"{world.grant} --ruling login")],
            f"Use your shell tool to run exactly: {world.grant} --ruling login",
        )
        stored = json.loads((world.home / "auth.json").read_text())
        fails.check(
            len(world.endpoint.refresh_calls) >= 1,
            f"{label}: the granted run never refreshed ({turn.stderr[-300:]})",
        )
        fails.check(
            not (world.home / "auth.json").is_symlink(),
            f"{label}: the real auth.json became a link",
        )
        tokens = stored.get("tokens", {})
        fails.check(
            tokens.get("access_token") == new_access and tokens.get("refresh_token") == "rt-new",
            f"{label}: the real home's auth.json holds {tokens.get('refresh_token')!r} / an access token "
            f"{'equal' if tokens.get('access_token') == new_access else 'other than'} the issued one",
        )
        refreshes = len(world.endpoint.refresh_calls)
        world.write_config(grant=False)
        _plain, plain_script = run_turn(world, f"loginu-{uuid.uuid4().hex[:6]}", [], "Reply done.")
        world.write_config(grant=True)
        fails.check(
            decided_or_answered(plain_script) and len(world.endpoint.refresh_calls) == refreshes,
            f"{label}: the later ungranted run was not accepted with the new token without a refresh "
            f"(refreshes {refreshes} -> {len(world.endpoint.refresh_calls)}; rejected "
            f"{len(world.endpoint.rejected_bearers)})",
        )
    finally:
        world.env = original_env  # type: ignore[method-assign]
        world.endpoint.require_bearer = None
        write_codex_home(world.home, world.endpoint.port)


def decided_or_answered(script: Script) -> bool:
    return bool(script.requests)


def group_keyring(world: World, fails: Failures) -> None:
    label = "codex keyring"
    config = world.home / "config.toml"
    original = config.read_text()
    config.write_text('cli_auth_credentials_store = "keyring"\n' + original)
    try:
        before = len(records(world))
        turn, _ = run_turn(
            world,
            f"kr-{uuid.uuid4().hex[:6]}",
            [Call("shell", cmd=f"{world.grant} --ruling kr")],
            f"Use your shell tool to run exactly: {world.grant} --ruling kr",
        )
        result = turn.report["results"][0]
        diags = [d for d in result.get("diagnostics", []) if d["setting"] == "allowed_tools"]
        fails.check(
            len(diags) == 1 and "keyring" in diags[0]["message"], f"{label}: diagnostics {diags}"
        )
        fails.check(
            "overlay_env" not in result, f"{label}: the withheld run still ran on an overlay"
        )
        fails.check(
            len(records(world)) == before, f"{label}: the withheld run executed the granted command"
        )
    finally:
        config.write_text(original)


# --------------------------------------------------------------------------
# The budget workload (Codex, --control)
# --------------------------------------------------------------------------


def budget_workload(world: World, path: Path) -> None:
    """Ungranted controlled turns on a warm pooled server, then 1, 2 and 20
    granted ones, each timed from oneharness's start to its exit."""

    def batch(n: int, grant: bool) -> list[dict[str, Any]]:
        world.write_config(grant=grant)
        procs = []
        for _ in range(n):
            probe = f"b-{uuid.uuid4().hex[:8]}"
            script = world.endpoint.script(
                probe, [Call("shell", cmd=f"{world.grant} --ruling 'budget'")]
            )
            argv = world.argv(
                f"[probe:{probe}] Use your shell tool to run exactly: {world.grant} --ruling 'budget'"
            )
            procs.append(
                (
                    probe,
                    script,
                    time.time(),
                    subprocess.Popen(
                        argv,
                        env=world.env(),
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        text=True,
                        stdin=subprocess.DEVNULL,
                    ),
                )
            )
        turns = []
        for probe, script, started, proc in procs:
            out, err = proc.communicate(timeout=900)
            ended = time.time()
            report = json.loads(out) if out.strip() else {}
            result = (report.get("results") or [{}])[0]
            if result.get("status") != "ok":
                raise RuntimeError(
                    f"budget turn {probe} ended {result.get('status')}: {err[-400:]}"
                )
            first = script.requests[0]["at"] if script.requests else None
            turns.append(
                {
                    "seconds": ended - started,
                    "started": started,
                    "ended": ended,
                    "first_model_request": first,
                    "overlay_timings": result.get("overlay_timings"),
                }
            )
        return turns

    batch(1, grant=False)  # warm the pooled server with one earlier lease
    ungranted = {str(n): batch(n, grant=False) for n in (1, 2, 20)}
    granted = {str(n): batch(n, grant=True) for n in (1, 2, 20)}
    version = subprocess.run(["codex", "--version"], capture_output=True, text=True).stdout.strip()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "codex_version": version,
                "platform": sys.platform,
                "load_average": list(os.getloadavg()),
                "ungranted": ungranted,
                "granted": granted,
            },
            indent=1,
        )
        + "\n"
    )


# --------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--harness", default="claude-code,codex")
    parser.add_argument("--codex-path", default="exec", choices=("exec", "control", "both"))
    parser.add_argument("--only", default="")
    parser.add_argument("--scratch", type=Path)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--budget-telemetry", type=Path)
    args = parser.parse_args(argv)
    if not Path(oneharness_bin()).is_file():
        print(
            f"run_allowlist_live: no oneharness binary at {oneharness_bin()}; run 'just build-release' "
            "or set ONEHARNESS_BIN",
            file=sys.stderr,
        )
        return 2
    base = args.scratch or Path(
        tempfile.mkdtemp(
            prefix="oh-run-allowlist-", dir=os.environ.get("ONEPIPELINE_NODE_SCRATCH_DIR")
        )
    )
    endpoint = Endpoint()
    fails = Failures()
    only = set(filter(None, args.only.split(",")))
    want = lambda g: not only or g in only  # noqa: E731
    try:
        if args.budget_telemetry is not None:
            world = World("codex", base / "budget", endpoint, control=True).make()
            # One state database, as a real home keeps: an ungranted turn builds it.
            budget_workload(world, args.budget_telemetry)
            return 0
        worlds: list[World] = []
        for harness in filter(None, args.harness.split(",")):
            paths = (
                [False]
                if harness != "codex"
                else {"exec": [False], "control": [True], "both": [False, True]}[args.codex_path]
            )
            for control in paths:
                tag = f"{harness}{'-control' if control else ''}"
                worlds.append(World(harness, base / tag, endpoint, control=control).make())
        for world in worlds:
            name = f"{world.harness}{' (control)' if world.control else ''}"
            started = time.time()
            count = len(fails.items)
            groups = [
                ("sync", lambda w: group_sync(w, fails)),
                ("probes", lambda w: group_probes(w, fails, ("run", "refuse"))),
                ("posture", lambda w: group_probes(w, fails, ("allow",))),
                ("lifecycle", lambda w: group_lifecycle(w, fails)),
                ("kill", lambda w: group_kill(w, fails)),
                ("concurrency", lambda w: group_concurrency(w, fails)),
                ("session", lambda w: group_session(w, fails)),
            ]
            if world.harness == "codex" and not world.control:
                groups += [
                    ("record", lambda w: group_record(w, fails)),
                    ("login", lambda w: group_login(w, fails)),
                    ("keyring", lambda w: group_keyring(w, fails)),
                ]
            for group, run in groups:
                if want(group):
                    try:
                        run(world)
                    except Exception as error:
                        fails.check(False, f"{name} {group}: crashed: {error!r}")
            print(
                f"{name}: {len(fails.items) - count} failure(s) in {time.time() - started:.0f}s",
                flush=True,
            )
    finally:
        endpoint.stop()
        for world in locals().get("worlds", []):
            shutil.rmtree(world.sessions, ignore_errors=True)
        if not args.keep and args.scratch is None:
            shutil.rmtree(base, ignore_errors=True)
    if fails.items:
        print(f"run_allowlist_live: {len(fails.items)} failure(s)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
