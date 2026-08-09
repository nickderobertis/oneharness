#!/usr/bin/env python3
"""Per-harness turn-control probe: does an out-of-band interrupt actually stop work?

Driven by ``scripts/explore-control.sh``. Each probe stands up the harness's own
control path *directly* (bypassing oneharness), drives a real turn that creates
one file per step, interrupts it mid-flight, and reports whether the file count
froze. The filesystem is the evidence on purpose: several harnesses report a
normal ``end_turn`` after a real cancellation, so a probe that trusted the
harness's own stop reason would report success for the wrong reason.

This is the counterpart of ``explore-hooks.sh``/``explore-events.sh`` for turn
control: it is how a ``ControlShape`` gets *sourced from real behavior* instead
of guessed, and the drift alarm that stops the capability matrix in ``README.md``
from decaying into stale documentation.

Not part of any gate. Verdicts:

* ``LIVE``     — the turn was interrupted and work stopped.
* ``REFUTED``  — the interrupt was accepted (or rejected) and work kept going.
* ``BLOCKED``  — the harness could not be driven far enough to judge (missing
  binary, auth/quota refusal, protocol error). Never reported as support.
"""

# llmlint: ignore-file[tool_output_is_signal] This script's OUTPUT IS its product:
# it is an investigative probe, not a gate step, and the per-harness heading,
# per-harness verdict, and summary table are the findings a reader runs it to
# obtain. Silencing them on success would leave it with nothing to report.

from __future__ import annotations

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from typing import Any, Callable, Dict, List, Optional, Tuple

# Enough steps that the turn cannot finish before the probe interrupts it.
STEPS = 60
# How long work must stay frozen after the interrupt to count as stopped.
FREEZE_SECONDS = 15
# How long to wait for the agent to get going before giving up on the attempt.
WARMUP_TIMEOUT = 180
# Hard ceiling per harness. A probe drives someone else's server over someone
# else's protocol, so any of them can wedge; a wedged probe must report BLOCKED
# rather than hang the investigation it exists to speed up.
PROBE_TIMEOUT = 420

PROMPT = (
    "You are a non-interactive test fixture in a scratch directory. Using your "
    f"shell tool, create {STEPS} files named step-001.txt through step-060.txt in "
    "the current directory, ONE PER TOOL CALL, sleeping 1 second between each "
    "(for example: sleep 1 && touch step-001.txt). Do not use a loop and do not "
    "create them in one command - make a separate tool call for every file. "
    "Start now and keep going."
)


class Verdict:
    LIVE = "LIVE"
    REFUTED = "REFUTED"
    BLOCKED = "BLOCKED"


def log(message: str) -> None:
    print(message, file=sys.stderr, flush=True)


def step_count(work: str) -> int:
    try:
        return len([n for n in os.listdir(work) if n.startswith("step-")])
    except OSError:
        return 0


def wait_for(predicate: Callable[[], bool], timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(0.5)
    return False


def judge(work: str, interrupt: Callable[[], None]) -> Tuple[str, str]:
    """Wait for real work, interrupt, then require the count to freeze."""
    if not wait_for(lambda: step_count(work) >= 2, WARMUP_TIMEOUT):
        return Verdict.BLOCKED, "the agent never produced two steps, so nothing could be interrupted"
    before = step_count(work)
    log(f"  interrupting after {before} steps")
    interrupt()
    time.sleep(3)
    frozen = step_count(work)
    time.sleep(FREEZE_SECONDS)
    after = step_count(work)
    if after != frozen:
        return (
            Verdict.REFUTED,
            f"work continued: {frozen} -> {after} step files in the {FREEZE_SECONDS}s after the interrupt",
        )
    return Verdict.LIVE, f"work froze at {after} step files for {FREEZE_SECONDS}s after the interrupt"


class JsonRpc:
    """Minimal newline-delimited JSON-RPC client over a child's stdio."""

    def __init__(self, process: subprocess.Popen):
        self.process = process
        self.next_id = 1
        self.responses: Dict[int, dict] = {}
        self.notifications: List[dict] = []
        self.server_requests: List[dict] = []
        self.lock = threading.Lock()
        self.on_server_request: Optional[Callable[[dict], Optional[Any]]] = None
        threading.Thread(target=self._read_loop, daemon=True).start()

    def _read_loop(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            # A server's stdout is external input: a valid JSON scalar, or an
            # object whose `id` cannot key a dict, would crash this reader thread
            # and leave every later request waiting on a response that can no
            # longer arrive. Neither is a JSON-RPC message, so neither is one.
            if not isinstance(message, dict):
                continue
            message_id = message.get("id")
            if message_id is not None and not isinstance(message_id, (str, int)):
                continue
            with self.lock:
                if message_id is not None and ("result" in message or "error" in message):
                    self.responses[message_id] = message
                elif message_id is not None and "method" in message:
                    self.server_requests.append(message)
                    handler = self.on_server_request
                else:
                    self.notifications.append(message)
                    handler = None
            if message_id is not None and "method" in message and self.on_server_request:
                reply = self.on_server_request(message)
                if reply is not None:
                    self.send_response(message_id, reply)

    def _write(self, payload: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(payload) + "\n")
        self.process.stdin.flush()

    def notify(self, method: str, params: Optional[dict] = None) -> None:
        self._write({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def send_response(self, request_id: Any, result: Any) -> None:
        self._write({"jsonrpc": "2.0", "id": request_id, "result": result})

    def request(self, method: str, params: Optional[dict] = None, timeout: float = 120.0) -> dict:
        request_id = self.next_id
        self.next_id += 1
        self._write({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}})
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                if request_id in self.responses:
                    return self.responses.pop(request_id)
            if self.process.poll() is not None:
                raise RuntimeError(f"{method}: the server exited (code {self.process.returncode})")
            time.sleep(0.05)
        raise TimeoutError(f"{method}: no response within {timeout}s")

    def find_notification(self, method: str) -> Optional[dict]:
        with self.lock:
            for message in self.notifications:
                if message.get("method") == method:
                    return message
        return None


def http_request(
    address: Tuple[str, Any],
    unix_path: Optional[str],
    method: str,
    path: str,
    body: Optional[dict] = None,
    timeout: float = 60.0,
) -> Tuple[int, str]:
    """One HTTP/1.1 request over TCP or a unix socket, no dependencies."""
    if unix_path:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        sock.connect(unix_path)
        host = "localhost"
    else:
        sock = socket.create_connection(address, timeout=timeout)
        host = f"{address[0]}:{address[1]}"
    payload = json.dumps(body).encode() if body is not None else b""
    head = (
        f"{method} {path} HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        "Connection: close\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(payload)}\r\n\r\n"
    ).encode()
    sock.sendall(head + payload)
    chunks = []
    try:
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
    except socket.timeout:
        pass
    finally:
        sock.close()
    raw = b"".join(chunks)
    head, _, body = raw.partition(b"\r\n\r\n")
    header = head.decode("utf-8", "replace")
    status = 0
    first = header.split("\r\n", 1)[0].split(" ")
    if len(first) > 1 and first[1].isdigit():
        status = int(first[1])
    if "chunked" in header.lower():
        body = dechunk(body)
    return status, body.decode("utf-8", "replace")


def dechunk(body: bytes) -> bytes:
    """Reassemble a `Transfer-Encoding: chunked` body.

    Not optional: crush's server answers chunked, and a probe that fed the raw
    framing to `json.loads` reported a decode error where the harness had in
    fact answered correctly.
    """
    out = bytearray()
    while True:
        line, _, rest = body.partition(b"\r\n")
        try:
            size = int(line.split(b";")[0].strip() or b"0", 16)
        except ValueError:
            return bytes(out) or body
        if size == 0:
            return bytes(out)
        out += rest[:size]
        body = rest[size:].lstrip(b"\r\n")


def path_segment(value: str, what: str) -> str:
    """A harness-supplied id, checked before it becomes part of a request path.

    The ids come from another program's JSON, so they are external input: one
    carrying `/` or `..` would silently retarget the request at a route the
    probe never meant to call, and the resulting verdict would describe
    something else entirely.
    """
    if not value or len(value) > 128 or not all(
        c.isalnum() or c in "-_." for c in value
    ):
        raise ValueError(f"{what} `{value}` is not a usable path segment")
    # A segment of nothing but dots (`.`, `..`) carries no `/` and so clears the
    # character check above, yet a server still resolves it as a traversal —
    # which is the retargeting this function exists to refuse.
    if not value.strip("."):
        raise ValueError(f"{what} `{value}` traverses rather than names a path segment")
    return value


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def scratch() -> str:
    work = tempfile.mkdtemp(prefix="oh-control-probe-")
    subprocess.run(["git", "init", "-q", work], check=False)
    return work


def terminate(process: Optional[subprocess.Popen]) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()




def probe_claude(bin_name: str) -> Tuple[str, str]:
    """Control rides the run process's own stdin (`-p --input-format stream-json`)."""
    work = scratch()
    argv = [
        bin_name,
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-mode",
        # llmlint: ignore[least_privilege_grants] Claude Code offers no
        # per-directory sandbox to scope this to, and its narrower headless
        # postures deny the very shell calls the probe measures — which would make
        # every verdict vacuous. Blast radius is one fresh mktemp scratch dir.
        "bypassPermissions",  # llmlint: ignore[least_privilege_grants] see above
    ]
    model = os.environ.get("CLAUDE_E2E_MODEL")
    if model:
        argv += ["--model", model]
    process = subprocess.Popen(
        argv, cwd=work, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
    )
    try:
        assert process.stdin is not None
        process.stdin.write(
            json.dumps(
                {"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": PROMPT}]}}
            )
            + "\n"
        )
        process.stdin.flush()

        def interrupt() -> None:
            assert process.stdin is not None
            process.stdin.write(
                json.dumps(
                    {
                        "type": "control_request",
                        "request_id": "probe-1",
                        "request": {"subtype": "interrupt"},
                    }
                )
                + "\n"
            )
            process.stdin.flush()

        return judge(work, interrupt)
    finally:
        terminate(process)
        shutil.rmtree(work, ignore_errors=True)




def probe_codex(bin_name: str) -> Tuple[str, str]:
    """`turn/interrupt` over the `codex app-server` JSON-RPC stdio protocol."""
    work = scratch()
    process = subprocess.Popen(
        [bin_name, "app-server"],
        cwd=work,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        rpc = JsonRpc(process)
        # Approvals arrive as server->client requests; answer them or the turn
        # blocks forever waiting on a client that is not listening.
        # The probe's prompt only ever runs shell commands, so only command
        # execution is approved — a patch or file-change approval would be a
        # grant nothing in this probe needs. An unrecognized or unneeded method
        # is still ANSWERED (silence stalls the turn), just not granted.
        approvals = {
            "execCommandApproval",
            "item/commandExecution/requestApproval",
        }
        rpc.on_server_request = lambda message: (
            {"decision": "approved"} if message.get("method") in approvals else {}
        )
        rpc.request(
            "initialize",
            {"clientInfo": {"name": "oneharness-control-probe", "title": "probe", "version": "0"}},
        )
        rpc.notify("initialized", {})
        started = rpc.request("thread/start", {"cwd": work, "approvalPolicy": "never"})
        if "error" in started:
            return Verdict.BLOCKED, f"thread/start failed: {started['error']}"
        thread_id = (started["result"].get("thread") or {}).get("id")
        if not thread_id:
            return Verdict.BLOCKED, f"thread/start returned no thread id: {started['result']}"
        turn: Dict[str, Any] = {}

        def start_turn() -> None:
            try:
                turn["response"] = rpc.request(
                    "turn/start",
                    {
                        "threadId": thread_id,
                        "input": [{"type": "text", "text": PROMPT}],
                        "approvalPolicy": "never",
                        # The narrowest policy that still lets the probe write
                        # its step files: writes confined to the scratch
                        # workspace, no network.
                        "sandboxPolicy": {
                            "type": "workspaceWrite",
                            "writableRoots": [work],
                            "networkAccess": False,
                        },
                        "cwd": work,
                    },
                    timeout=600,
                )
            except Exception as err:  # noqa: BLE001 - reported as BLOCKED below
                turn["error"] = err

        threading.Thread(target=start_turn, daemon=True).start()

        # The turn id arrives on the event stream; the interrupt needs it.
        def turn_id() -> Optional[str]:
            message = rpc.find_notification("turn/started")
            if not message:
                return None
            params = message.get("params") or {}
            return (params.get("turn") or {}).get("id")

        if not wait_for(lambda: turn_id() is not None, 120):
            return Verdict.BLOCKED, "no turn/started notification carried a turn id"

        def interrupt() -> None:
            rpc.request("turn/interrupt", {"threadId": thread_id, "turnId": turn_id()}, timeout=60)

        return judge(work, interrupt)
    except Exception as err:  # noqa: BLE001 - a probe never crashes the suite
        return Verdict.BLOCKED, f"{type(err).__name__}: {err}"
    finally:
        terminate(process)
        shutil.rmtree(work, ignore_errors=True)




def probe_opencode(bin_name: str) -> Tuple[str, str]:
    """`POST /api/session/{id}/interrupt` against `opencode serve`."""
    work = scratch()
    port = free_port()
    server = subprocess.Popen(
        [bin_name, "serve", "--port", str(port)],
        cwd=work,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        address = ("127.0.0.1", port)

        def up() -> bool:
            try:
                return http_request(address, None, "GET", "/api/app", timeout=5)[0] > 0
            except OSError:
                return False

        if not wait_for(up, 60):
            return Verdict.BLOCKED, f"opencode serve did not answer on port {port}"
        # The session names the directory it works in; without it the turn runs
        # wherever the server was started and the verdict watches the wrong tree.
        status, text = http_request(
            address, None, "POST", "/api/session", {"location": {"directory": work}}
        )
        if status >= 400:
            return Verdict.BLOCKED, f"session create failed ({status}): {text[:200]}"
        payload = json.loads(text)
        session = payload.get("data", payload)
        session_id = session.get("id")
        if not session_id:
            return Verdict.BLOCKED, f"session create returned no id: {text[:200]}"
        session_id = path_segment(session_id, "opencode session id")

        def prompt() -> None:
            # `/prompt` with a `{"prompt":{"text":…}}` body — `/message` is not a
            # route this server has, and an unmatched path falls through to the
            # web UI, answering `200` with HTML while nothing runs.
            try:
                http_request(
                    address,
                    None,
                    "POST",
                    f"/api/session/{session_id}/prompt",
                    {"prompt": {"text": PROMPT}},
                    timeout=600,
                )
            except OSError:
                pass

        threading.Thread(target=prompt, daemon=True).start()

        def interrupt() -> None:
            code, body = http_request(address, None, "POST", f"/api/session/{session_id}/interrupt", {})
            log(f"  interrupt responded {code}: {body[-200:]}")

        return judge(work, interrupt)
    except Exception as err:  # noqa: BLE001 - a probe reports a protocol fault as BLOCKED rather than crashing the sweep
        return Verdict.BLOCKED, f"{type(err).__name__}: {err}"
    finally:
        terminate(server)
        shutil.rmtree(work, ignore_errors=True)




def probe_crush(bin_name: str) -> Tuple[str, str]:
    """`POST /v1/workspaces/{id}/agent/sessions/{sid}/cancel` against `crush server`.

    Three details that are easy to get wrong: the ``client_id`` is a self-assigned
    UUID that travels in the *body* when creating a workspace but as a *query
    parameter* on every other route (a mismatch yields a bare
    ``{"message":"invalid client_id"}``), the prompt POST returns 202
    immediately with the turn running in the background, and the server blocks on
    a permission decision — so a probe that never posts ``permissions/skip``
    watches an agent that is waiting rather than working, and reports BLOCKED for
    its own omission.
    """
    work = scratch()
    sock_path = os.path.join(tempfile.mkdtemp(prefix="oh-crush-"), "crush.sock")
    server = subprocess.Popen(
        [bin_name, "server", "-H", f"unix://{sock_path}"],
        cwd=work,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    client_id = str(uuid.uuid4())
    try:
        if not wait_for(lambda: os.path.exists(sock_path), 60):
            return Verdict.BLOCKED, f"crush server never bound {sock_path}"

        def call(method: str, path: str, body: Optional[dict] = None, timeout: float = 60.0):
            joiner = "&" if "?" in path else "?"
            return http_request(
                ("", 0), sock_path, method, f"{path}{joiner}client_id={client_id}", body, timeout
            )

        status, text = http_request(
            ("", 0),
            sock_path,
            "POST",
            "/v1/workspaces",
            {"client_id": client_id, "path": work},
        )
        if status >= 400:
            return Verdict.BLOCKED, f"workspace create failed ({status}): {text[:200]}"
        workspace = json.loads(text)
        workspace_id = workspace.get("id") or workspace.get("workspace", {}).get("id")
        if not workspace_id:
            return Verdict.BLOCKED, f"workspace create returned no id: {text[:200]}"
        workspace_id = path_segment(workspace_id, "crush workspace id")

        # Sessions are created on the WORKSPACE (`/sessions`); everything under
        # `/agent` addresses an existing one. Posting the create to
        # `/agent/sessions` answers a bare `404 page not found`.
        status, text = call("POST", f"/v1/workspaces/{workspace_id}/sessions", {"title": "probe"})
        if status >= 400:
            return Verdict.BLOCKED, f"session create failed ({status}): {text[:200]}"
        session_id = json.loads(text).get("id")
        if not session_id:
            return Verdict.BLOCKED, f"session create returned no id: {text[:200]}"
        session_id = path_segment(session_id, "crush session id")

        # `permissions/skip` is crush's `--yolo`, and it is what oneharness posts
        # for a permissive run. Without it the agent asks and waits, so the
        # freeze the verdict measures would be a permission prompt rather than an
        # interrupt.
        status, text = call("POST", f"/v1/workspaces/{workspace_id}/permissions/skip", {"skip": True})
        if status >= 400:
            return Verdict.BLOCKED, f"permissions/skip failed ({status}): {text[:200]}"

        # The prompt goes to the workspace's agent with the session named in the
        # BODY — `/agent/sessions/{sid}` is a GET-only resource, and posting
        # there answers `405 Method Not Allowed`. A missing `prompt`/`session_id`
        # is a 500 naming the field, which is how both were pinned.
        status, text = call(
            "POST",
            f"/v1/workspaces/{workspace_id}/agent",
            {"prompt": PROMPT, "session_id": session_id},
        )
        if status >= 400:
            return Verdict.BLOCKED, f"prompt failed ({status}): {text[:200]}"

        def interrupt() -> None:
            code, body = call(
                "POST", f"/v1/workspaces/{workspace_id}/agent/sessions/{session_id}/cancel", {}
            )
            log(f"  cancel responded {code}: {body[-200:]}")

        return judge(work, interrupt)
    except Exception as err:  # noqa: BLE001 - a probe reports a protocol fault as BLOCKED rather than crashing the sweep
        return Verdict.BLOCKED, f"{type(err).__name__}: {err}"
    finally:
        terminate(server)
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(os.path.dirname(sock_path), ignore_errors=True)




def probe_acp(bin_name: str, launch: List[str]) -> Tuple[str, str]:
    """The ACP `session/cancel` NOTIFICATION (no `id`) over a JSON-RPC stdio server.

    Two things the client must handle or the probe proves nothing: it MUST answer
    ``session/request_permission`` (both harnesses block indefinitely and never
    begin work otherwise), and it must not trust the reported stop reason — both
    report ``end_turn`` after a real cancellation.
    """
    work = scratch()
    process = subprocess.Popen(
        [bin_name] + launch,
        cwd=work,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        rpc = JsonRpc(process)

        def answer(message: dict) -> Optional[Any]:
            if message.get("method") != "session/request_permission":
                return None
            # llmlint: ignore[least_privilege_grants] The option set is defined by
            # the harness, not the client: ACP offers no way to narrow a grant, and
            # declining is exactly the failure mode this probe must not create (a
            # refused turn produces no work, so the freeze assertion proves nothing).
            options = (message.get("params") or {}).get("options") or []
            # llmlint: ignore[least_privilege_grants] ACP lets a client accept or
            # decline the harness's own options; there is no narrower grant to
            # choose, and declining is the one outcome that makes the probe prove
            # nothing (no work runs, so nothing can be observed to stop).
            allow = next(
                (o for o in options if "allow" in str(o.get("kind", "")).lower()),
                options[0] if options else None,
            )
            if allow is None:
                return {"outcome": {"outcome": "cancelled"}}
            return {"outcome": {"outcome": "selected", "optionId": allow.get("optionId")}}

        rpc.on_server_request = answer
        rpc.request(
            "initialize",
            {
                "protocolVersion": 1,
                "clientCapabilities": {"fs": {"readTextFile": False, "writeTextFile": False}},
            },
        )
        created = rpc.request("session/new", {"cwd": work, "mcpServers": []})
        if "error" in created:
            return Verdict.BLOCKED, f"session/new failed: {created['error']}"
        session_id = created["result"].get("sessionId")
        if not session_id:
            return Verdict.BLOCKED, f"session/new returned no sessionId: {created['result']}"

        def prompt() -> None:
            try:
                rpc.request(
                    "session/prompt",
                    {"sessionId": session_id, "prompt": [{"type": "text", "text": PROMPT}]},
                    timeout=600,
                )
            except Exception:  # noqa: BLE001 - the prompt thread's fate is irrelevant; the filesystem is the verdict
                pass

        threading.Thread(target=prompt, daemon=True).start()

        def interrupt() -> None:
            # A NOTIFICATION, not a request: sent with an `id`, goose answers
            # `-32601 Method not found`.
            rpc.notify("session/cancel", {"sessionId": session_id})

        return judge(work, interrupt)
    except Exception as err:  # noqa: BLE001 - a probe reports a protocol fault as BLOCKED rather than crashing the sweep
        return Verdict.BLOCKED, f"{type(err).__name__}: {err}"
    finally:
        terminate(process)
        shutil.rmtree(work, ignore_errors=True)


PROBES: Dict[str, Tuple[str, str, Callable[[str], Tuple[str, str]]]] = {
    "claude-code": ("claude", "claude-control-request", probe_claude),
    "codex": ("codex", "codex-app-server", probe_codex),
    "opencode": ("opencode", "opencode-http", probe_opencode),
    "crush": ("crush", "crush-http", probe_crush),
    "goose": ("goose", "acp-cancel", lambda b: probe_acp(b, ["acp"])),
    "copilot": ("copilot", "acp-cancel", lambda b: probe_acp(b, ["--acp"])),
}

# Probed and found to have no headless control surface at all. Reported rather
# than omitted, so the matrix says *why* they are absent.
NO_SURFACE = {
    "cursor": "cursor-agent exposes no headless control channel (probed)",
    "qwen": "qwen exposes no headless control channel (probed)",
}


def with_deadline(seconds: int, body: Callable[[], Tuple[str, str]]) -> Tuple[str, str]:
    """Run `body`, converting a wedged probe into a BLOCKED verdict."""

    def expire(_signum: int, _frame: Any) -> None:
        raise TimeoutError(f"probe exceeded {seconds}s")

    previous = signal.signal(signal.SIGALRM, expire)
    signal.alarm(seconds)
    try:
        return body()
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, previous)


def main(argv: List[str]) -> int:
    if len(argv) != 2:
        print("usage: explore_control.py <harness-id|all>", file=sys.stderr)
        return 2
    target = argv[1]
    ids = sorted(PROBES) + sorted(NO_SURFACE) if target == "all" else [target]

    results: List[Tuple[str, str, str, str]] = []
    for harness in ids:
        if harness in NO_SURFACE:
            results.append((harness, "-", Verdict.BLOCKED, NO_SURFACE[harness]))
            continue
        if harness not in PROBES:
            print(f"unknown harness `{harness}`", file=sys.stderr)
            return 2
        bin_name, mechanism, probe = PROBES[harness]
        bin_name = os.environ.get(f"OH_PROBE_BIN_{harness.replace('-', '_').upper()}", bin_name)
        log(f"\n========== {harness} ({mechanism}) ==========")
        if shutil.which(bin_name) is None:
            results.append((harness, mechanism, Verdict.BLOCKED, f"`{bin_name}` is not installed"))
            continue
        try:
            verdict, detail = with_deadline(PROBE_TIMEOUT, lambda: probe(bin_name))
        except Exception as err:  # noqa: BLE001 - any protocol fault is data (BLOCKED), never a crash
            verdict, detail = Verdict.BLOCKED, f"{type(err).__name__}: {err}"
        log(f"  {verdict}: {detail}")
        results.append((harness, mechanism, verdict, detail))

    print("\n=== turn-control probe verdicts ===")
    for harness, mechanism, verdict, detail in results:
        print(f"{harness:<12} {mechanism:<24} {verdict:<8} {detail}")
    # A probe reports; it never fails a build. BLOCKED is data, not an error.
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
