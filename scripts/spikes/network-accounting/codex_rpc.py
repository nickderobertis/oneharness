#!/usr/bin/env python3
"""Drive `codex app-server` over JSON-RPC for one measured launch.

    codex_rpc.py usage [--sock PATH] [--linger S] [-- CODEX_FLAGS...]
    codex_rpc.py turn  [--sock PATH] [--prompt TEXT] [-- CODEX_FLAGS...]
    codex_rpc.py exec  [--prompt TEXT] [-- CODEX_FLAGS...]

`usage`: initialize, initialized, account/rateLimits/read, then close stdin.
`turn`: initialize, initialized, thread/start, one turn/start, wait for
turn/completed, then close stdin. `exec`: one `codex exec --json` turn.
For `turn` and `exec` the summary says whether the launch's process logged
`fetching remote plugin catalog` to ~/.codex/logs_2.sqlite. CODEX_FLAGS (e.g. `--disable plugins`,
`-c features.apps=false`) go before the subcommand. With --sock the client
connects to an already running `codex app-server --listen unix://PATH`
instead of spawning `codex app-server --stdio`; that socket speaks JSON-RPC
inside WebSocket text frames (it answers an HTTP Upgrade with 101), so the
client below frames it itself — `codex app-server proxy` only pipes raw bytes.
Prints one JSON summary line on stderr (thread id, whether each step succeeded).
"""

import argparse
import base64
import json
import os
import socket
import sqlite3
import struct
import subprocess
import sys
import tempfile
import threading
import time

PROMPT = "Reply with the single word: ok"


CATALOG = "fetching remote plugin catalog"


def catalog_logged(pid, t0, t1):
    """Did the launch's codex process log the catalog fetch while it ran?

    The log DB is shared by every codex on the host and an exec's lines carry no
    thread id, but each row's process_uuid is `pid:<pid as codex sees it>:<uuid>`.
    `codex` on PATH is a Node launcher that spawns the native binary, so the
    logging pid is a descendant of `pid`; inside netmeasure's fresh PID namespace
    the tree's pids are allocated upward from it and nothing else is there, so
    pids [pid, pid + 64) in the run's time window are this launch's."""
    db = os.path.join(os.environ.get("CODEX_HOME", os.path.expanduser("~/.codex")), "logs_2.sqlite")
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            "select process_uuid, feedback_log_body like ? from logs where ts between ? and ?",
            (f"%{CATALOG}%", int(t0) - 1, int(t1) + 1)).fetchall()
    finally:
        conn.close()
    mine = [(u, hit) for u, hit in rows
            if u and u.startswith("pid:") and pid <= int(u.split(":")[1]) < pid + 64]
    return {"log_processes": sorted({u for u, _ in mine}), "log_rows": len(mine),
            "catalog_log_lines": sum(hit for _, hit in mine)}


def run_exec(codex, flags, prompt):
    work = tempfile.mkdtemp(prefix="netmeasure-exec-")
    argv = [codex, *flags, "exec", "--skip-git-repo-check", "--json", prompt]
    t0 = time.time()
    proc = subprocess.Popen(argv, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, text=True)
    stdout, _ = proc.communicate()
    summary = {"mode": "exec", "argv": argv, "exit": proc.returncode}
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "thread.started":
            summary["thread_id"] = event.get("thread_id")
        if event.get("type") == "item.completed" and event.get("item", {}).get("type") == "agent_message":
            summary["reply"] = event["item"].get("text")
    summary.update(catalog_logged(proc.pid, t0, time.time()))
    os.rmdir(work) if not os.listdir(work) else None
    return summary


class Rpc:
    def __init__(self, argv):
        self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, text=True)
        self.next_id = 0
        self.messages = []
        self.cond = threading.Condition()
        self.eof = False
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if "method" in message and "id" in message:  # server -> client request
                self._send({"id": message["id"], "result": {"decision": "denied"}})
            with self.cond:
                self.messages.append(message)
                self.cond.notify_all()
        with self.cond:  # the server exited: nothing more will arrive
            self.eof = True
            self.cond.notify_all()

    def _send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def wait(self, predicate, timeout):
        deadline = time.monotonic() + timeout
        with self.cond:
            while True:
                for message in self.messages:
                    if predicate(message):
                        return message
                left = deadline - time.monotonic()
                if left <= 0 or self.eof:
                    return None
                self.cond.wait(left)

    def request(self, method, params, timeout=120):
        self.next_id += 1
        rid = self.next_id
        self._send({"id": rid, "method": method, "params": params})
        return self.wait(lambda m: m.get("id") == rid and "method" not in m, timeout)

    def notify(self, method, params):
        self._send({"method": method, "params": params})

    def close(self, timeout=60):
        self.proc.stdin.close()
        try:
            return self.proc.wait(timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            return self.proc.wait()


class WsRpc(Rpc):
    """The same JSON-RPC client over a WebSocket on a unix socket."""

    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.connect(path)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall((f"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
                           f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                           f"Sec-WebSocket-Version: 13\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            head += self.sock.recv(1)
        if b" 101 " not in head.split(b"\r\n", 1)[0]:
            raise RuntimeError(f"websocket upgrade refused: {head!r}")
        self.wlock = threading.Lock()
        self.proc = None
        self.next_id = 0
        self.messages = []
        self.cond = threading.Condition()
        self.eof = False
        threading.Thread(target=self._read, daemon=True).start()

    def _exact(self, n):
        data = b""
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    def _frame(self, opcode, payload):
        mask = os.urandom(4)
        n = len(payload)
        if n < 126:
            header = struct.pack("!BB", 0x80 | opcode, 0x80 | n)
        elif n < 65536:
            header = struct.pack("!BBH", 0x80 | opcode, 0x80 | 126, n)
        else:
            header = struct.pack("!BBQ", 0x80 | opcode, 0x80 | 127, n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        with self.wlock:
            self.sock.sendall(header + mask + masked)

    def _send(self, message):
        self._frame(0x1, json.dumps(message).encode())

    def _read(self):
        buffer = b""
        try:
            while True:
                b0, b1 = self._exact(2)
                n = b1 & 0x7F
                if n == 126:
                    n = struct.unpack("!H", self._exact(2))[0]
                elif n == 127:
                    n = struct.unpack("!Q", self._exact(8))[0]
                payload = self._exact(n)
                opcode = b0 & 0x0F
                if opcode == 0x8:
                    break
                if opcode == 0x9:
                    self._frame(0xA, payload)
                    continue
                buffer += payload
                if not b0 & 0x80:
                    continue
                message, buffer = json.loads(buffer), b""
                if "method" in message and "id" in message:
                    self._send({"id": message["id"], "result": {"decision": "denied"}})
                with self.cond:
                    self.messages.append(message)
                    self.cond.notify_all()
        except (EOFError, OSError):
            pass
        with self.cond:
            self.eof = True
            self.cond.notify_all()

    def close(self, timeout=60):
        try:
            self._frame(0x8, b"")
        except OSError:
            pass
        self.sock.close()
        return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["usage", "turn", "exec"])
    parser.add_argument("--sock")
    parser.add_argument("--prompt", default=PROMPT)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--linger", type=float, default=0,
                        help="keep the server alive S seconds before closing stdin")
    argv = sys.argv[1:]
    split = argv.index("--") if "--" in argv else len(argv)
    args = parser.parse_args(argv[:split])
    flags = argv[split + 1:]
    codex = os.environ.get("CODEX_BIN", "codex")
    if args.mode == "exec":
        print(json.dumps(run_exec(codex, flags, args.prompt)), file=sys.stderr)
        return 0
    t0 = time.time()
    if args.sock:
        argv = ["unix", args.sock]
        rpc = WsRpc(args.sock)
    else:
        argv = [codex, *flags, "app-server", "--stdio"]
        rpc = Rpc(argv)
    summary = {"mode": args.mode, "argv": argv}
    init = rpc.request("initialize", {"clientInfo": {"name": "netmeasure", "title": "netmeasure",
                                                     "version": "0"}})
    summary["initialize"] = bool(init and "result" in init)
    rpc.notify("initialized", {})
    if args.mode == "usage":
        limits = rpc.request("account/rateLimits/read", {})
        summary["rateLimits"] = bool(limits and "result" in limits)
    else:
        work = tempfile.mkdtemp(prefix="netmeasure-turn-")
        started = rpc.request("thread/start", {"cwd": work, "approvalPolicy": "never"})
        thread_id = ((started or {}).get("result") or {}).get("thread", {}).get("id")
        summary["thread_id"] = thread_id
        if thread_id:
            turn = rpc.request("turn/start", {
                "threadId": thread_id, "input": [{"type": "text", "text": args.prompt}],
                "approvalPolicy": "never", "cwd": work,
                "sandboxPolicy": {"type": "readOnly"}}, timeout=args.timeout)
            summary["turn_start"] = bool(turn and "result" in turn)
            done = rpc.wait(lambda m: m.get("method") == "turn/completed", args.timeout)
            summary["turn_completed"] = (done or {}).get("params", {}).get("turn", {}).get("status")
            text = [m["params"]["item"].get("text") for m in rpc.messages
                    if m.get("method") == "item/completed"
                    and m.get("params", {}).get("item", {}).get("type") == "agentMessage"]
            summary["reply"] = text[-1] if text else None
        os.rmdir(work) if not os.listdir(work) else None
    time.sleep(args.linger)
    summary["exit"] = rpc.close()
    if not args.sock:
        summary.update(catalog_logged(rpc.proc.pid, t0, time.time()))
    print(json.dumps(summary), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
