#!/usr/bin/env python3
"""Throwaway spike: how concurrent Codex dispatches can share one app-server.

Settles nickderobertis/oneharness#1442 part 2 against a real `codex app-server`.
Never landed; stdlib only so it runs anywhere Codex does.

    python3 scripts/spikes/shared-app-server/spike.py <scenario> [--work DIR]

Scenarios (question in brackets):
    transport      [1] --listen unix:// lifecycle vs `daemon` / `proxy`         (free)
    isolation-free [2,3] per-thread fields + env via thread/shellCommand       (free)
    isolation      [2,3] two clients, concurrent real turns running `env`      (2 paid)
    killed-shell   [4] SIGKILL a client mid thread/shellCommand heartbeat      (free)
    killed-client  [4] SIGKILL a client mid-turn running a heartbeat command   (2 paid)
    interrupt      [5] turn/interrupt: active / none / unknown / after restart (1 paid)
    blast          [6] kill the server under two live turns                    (2 paid)
    savings        [7] time to first turn + bytes, warm shared vs fresh spawn  (2 paid)
    pool-key       [8] which settings a thread can override vs process-fixed   (free)

Prints one JSON object of what it observed on stdout; diagnostics on stderr.
Every paid turn is appended to <work>/paid-turns.jsonl.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import queue
import re
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path

CODEX = os.environ.get("SPIKE_CODEX", "codex")
T0 = time.monotonic()


def now() -> float:
    return round(time.monotonic() - T0, 3)


def log(*a):
    print(f"[{now():8.3f}]", *a, file=sys.stderr, flush=True)


# --------------------------------------------------------------------------
# WebSocket over a unix socket (the app-server's unix:// transport), stdlib.
# --------------------------------------------------------------------------


class WS:
    def __init__(self, path: str, timeout: float = 10):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        # sun_path is 108 bytes; for a longer --listen path Codex binds a short
        # target under /tmp/codex-daemon-<uid>/ and leaves a symlink at PATH.
        self.sock.connect(os.path.realpath(path) if len(path.encode()) > 100 else path)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            (
                "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
                f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                "Sec-WebSocket-Version: 13\r\n\r\n"
            ).encode()
        )
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = self.sock.recv(1)
            if not chunk:
                raise ConnectionError("closed during websocket handshake")
            head += chunk
        if b" 101 " not in head.split(b"\r\n")[0]:
            raise ConnectionError(f"handshake refused: {head!r}")
        self.sock.settimeout(None)
        self.lock = threading.Lock()

    def _recv_exact(self, n: int) -> bytes:
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise EOFError("eof")
            buf += chunk
        return buf

    def send_frame(self, opcode: int, payload: bytes):
        mask = os.urandom(4)
        n = len(payload)
        if n < 126:
            hdr = struct.pack("!BB", 0x80 | opcode, 0x80 | n)
        elif n < 65536:
            hdr = struct.pack("!BBH", 0x80 | opcode, 0x80 | 126, n)
        else:
            hdr = struct.pack("!BBQ", 0x80 | opcode, 0x80 | 127, n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        with self.lock:
            self.sock.sendall(hdr + mask + masked)

    def send_text(self, text: str):
        self.send_frame(0x1, text.encode())

    def recv_message(self):
        """('text', str) | ('close', (code, reason)); raises EOFError/OSError."""
        parts = []
        while True:
            b0, b1 = self._recv_exact(2)
            opcode = b0 & 0x0F
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack("!H", self._recv_exact(2))[0]
            elif n == 127:
                n = struct.unpack("!Q", self._recv_exact(8))[0]
            if b1 & 0x80:
                mk = self._recv_exact(4)
                data = bytes(b ^ mk[i % 4] for i, b in enumerate(self._recv_exact(n)))
            else:
                data = self._recv_exact(n)
            if opcode == 0x9:
                self.send_frame(0xA, data)
                continue
            if opcode == 0xA:
                continue
            if opcode == 0x8:
                code = struct.unpack("!H", data[:2])[0] if len(data) >= 2 else None
                return ("close", (code, data[2:].decode(errors="replace")))
            parts.append(data)
            if b0 & 0x80:
                return ("text", b"".join(parts).decode())

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


class Client:
    """One JSON-RPC connection; records every frame with a timestamp."""

    def __init__(self, sock_path: str, name: str):
        self.name = name
        self.ws = WS(sock_path)
        self.next_id = 1
        self.pending: dict[int, queue.Queue] = {}
        self.events: list[tuple[float, dict]] = []
        self.cond = threading.Condition()
        self.ended: dict | None = None  # how the connection ended
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            while True:
                kind, val = self.ws.recv_message()
                if kind == "close":
                    self._end({"how": "close_frame", "code": val[0], "reason": val[1]})
                    return
                msg = json.loads(val)
                with self.cond:
                    self.events.append((now(), msg))
                    self.cond.notify_all()
                if "id" in msg and ("result" in msg or "error" in msg):
                    q = self.pending.pop(msg["id"], None)
                    if q:
                        q.put(msg)
                elif "id" in msg and "method" in msg:
                    # A server request (approval, user input): refuse, never hang.
                    self.send({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "spike client declines"}})
        except EOFError:
            self._end({"how": "eof"})
        except OSError as e:
            self._end({"how": "oserror", "error": f"{type(e).__name__}: {e}"})

    def _end(self, how: dict):
        how["at"] = now()
        with self.cond:
            self.ended = how
            self.cond.notify_all()
        for q in list(self.pending.values()):
            q.put({"_connection_ended": how})

    def send(self, obj: dict):
        self.ws.send_text(json.dumps(obj))

    def request(self, method: str, params=None, timeout: float = 60) -> dict:
        rid = self.next_id
        self.next_id += 1
        q: queue.Queue = queue.Queue()
        self.pending[rid] = q
        sent = now()
        try:
            self.send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params if params is not None else {}})
        except OSError as e:
            self.pending.pop(rid, None)
            return {"_send_failed": f"{type(e).__name__}: {e}", "sent_at": sent}
        if self.ended:
            self.pending.pop(rid, None)
            return {"_connection_ended": self.ended, "sent_at": sent}
        try:
            msg = q.get(timeout=timeout)
        except queue.Empty:
            return {"_timeout": timeout, "sent_at": sent}
        msg = dict(msg)
        msg["_latency_s"] = round(now() - sent, 3)
        return msg

    def notify(self, method: str, params=None):
        self.send({"jsonrpc": "2.0", "method": method, "params": params if params is not None else {}})

    def initialize(self) -> dict:
        r = self.request("initialize", {"clientInfo": {"name": f"spike-{self.name}", "title": "spike", "version": "0"}})
        self.notify("initialized")
        return r

    def wait_for(self, pred, timeout: float, since: int = 0):
        deadline = time.monotonic() + timeout
        with self.cond:
            while True:
                for t, m in self.events[since:]:
                    if pred(m):
                        return t, m
                if self.ended and pred({"_ended": self.ended}):
                    return self.ended["at"], {"_ended": self.ended}
                left = deadline - time.monotonic()
                if left <= 0:
                    return None
                self.cond.wait(left)

    def notes(self, method: str, thread: str | None = None):
        out = []
        for t, m in list(self.events):
            if m.get("method") == method and (thread is None or (m.get("params") or {}).get("threadId") == thread):
                out.append((t, m))
        return out

    def close(self):
        self.ws.close()


def is_note(method, thread=None):
    def pred(m):
        return m.get("method") == method and (thread is None or (m.get("params") or {}).get("threadId") == thread)

    return pred


def turn_completed(thread):
    def pred(m):
        return "_ended" in m or (m.get("method") == "turn/completed" and m["params"].get("threadId") == thread)

    return pred


# --------------------------------------------------------------------------
# Server process + network sampling
# --------------------------------------------------------------------------


class Server:
    def __init__(self, work: Path, name: str, args=(), env_extra=None, env_drop=(), sock: str | None = None):
        self.sock = sock or str(work / f"{name}.sock")
        self.log_path = work / f"{name}.server.log"
        env = dict(os.environ)
        for k in env_drop:
            env.pop(k, None)
        env.update(env_extra or {})
        self.argv = [CODEX, "app-server", "--listen", f"unix://{self.sock}", *args]
        self.launched_at = now()
        self.proc = subprocess.Popen(
            self.argv, env=env, stdin=subprocess.DEVNULL, stdout=open(self.log_path, "wb"),
            stderr=subprocess.STDOUT, start_new_session=True,
        )
        self.pid = self.proc.pid  # what oneharness would record: the spawned `codex`
        self.ready_at = None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"server exited {self.proc.returncode}: {self.log_path.read_text()[-2000:]}")
            try:
                WS(self.sock, timeout=2).close()
                self.ready_at = now()
                break
            except OSError:
                time.sleep(0.02)
        else:
            raise RuntimeError("server socket never accepted")
        self.real_pid = real_server_pid(self.pid)
        log(f"server {name} pid={self.pid} real={self.real_pid} ready in {self.ready_at - self.launched_at:.3f}s")

    def alive(self) -> bool:
        return self.proc.poll() is None and pid_alive(self.real_pid)

    def stop(self, sig=signal.SIGTERM, wait=10) -> dict:
        if not self.alive():
            return {"already_exited": self.proc.returncode}
        t = now()
        self.proc.send_signal(sig)
        try:
            self.proc.wait(wait)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            return {"signal": sig.name, "exited_after_s": None, "needed_sigkill": True}
        out = {"signal": sig.name, "exited_after_s": round(now() - t, 3), "returncode": self.proc.returncode}
        if self.real_pid != self.pid:
            time.sleep(0.5)
            out["real_server_outlived_launcher"] = pid_alive(self.real_pid)
            if out["real_server_outlived_launcher"]:
                os.kill(self.real_pid, signal.SIGKILL)  # the server this spike started, by its pid
        return out


class NetSampler:
    """Polls `ss -tinp` for TCP sockets owned by pid; keeps each connection's
    max bytes. A lower bound (a connection opened and closed between samples
    is missed) — the fallback the task allows when spike-network-accounting's
    branch is absent."""

    def __init__(self, pid: int, interval=0.1):
        self.pid = pid
        self.conns: dict[tuple, dict] = {}
        self.stop_ev = threading.Event()
        self.t = threading.Thread(target=self._run, daemon=True)
        self.interval = interval
        self.t.start()

    def _run(self):
        pat = re.compile(rf"pid={self.pid},")
        while not self.stop_ev.is_set():
            try:
                out = subprocess.run(["ss", "-tinpH"], capture_output=True, text=True, timeout=5).stdout
            except Exception:
                out = ""
            lines = out.splitlines()
            for i, line in enumerate(lines):
                if not pat.search(line):
                    continue
                cols = line.split()
                local, peer = cols[3], cols[4]
                info = lines[i + 1] if i + 1 < len(lines) else ""
                rx = re.search(r"bytes_received:(\d+)", info)
                tx = re.search(r"bytes_acked:(\d+)", info)
                key = (local, peer)
                c = self.conns.setdefault(key, {"first_seen": now(), "rx": 0, "tx": 0})
                c["last_seen"] = now()
                c["rx"] = max(c["rx"], int(rx.group(1)) if rx else 0)
                c["tx"] = max(c["tx"], int(tx.group(1)) if tx else 0)
            self.stop_ev.wait(self.interval)

    def snapshot(self):
        return {k: dict(v) for k, v in self.conns.items()}

    def stop(self):
        self.stop_ev.set()
        self.t.join()

    @staticmethod
    def delta(before: dict, after: dict) -> dict:
        rx = tx = 0
        new = 0
        peers = set()
        for k, v in after.items():
            b = before.get(k)
            if b is None:
                new += 1
                peers.add(k[1].rsplit(":", 1)[0])
            rx += v["rx"] - (b["rx"] if b else 0)
            tx += v["tx"] - (b["tx"] if b else 0)
        return {"rx_bytes": rx, "tx_bytes": tx, "new_connections": new, "new_peers": sorted(peers)}


# --------------------------------------------------------------------------
# Helpers
# --------------------------------------------------------------------------

PAID = {"n": 0}


def record_paid(work: Path, scenario: str, label: str, prompt: str, outcome: dict):
    PAID["n"] += 1
    with open(work / "paid-turns.jsonl", "a") as f:
        f.write(json.dumps({"scenario": scenario, "label": label, "prompt": prompt, "outcome": outcome, "at": time.strftime("%Y-%m-%dT%H:%M:%S")}) + "\n")


def thread_start(c: Client, cwd: str, **extra) -> dict:
    params = {"cwd": cwd, "approvalPolicy": "never", "sandbox": "danger-full-access", "ephemeral": True}
    params.update(extra)
    return c.request("thread/start", params)


def turn_start(c: Client, thread: str, cwd: str, prompt: str, **extra) -> dict:
    params = {
        "threadId": thread, "cwd": cwd, "input": [{"type": "text", "text": prompt}],
        "approvalPolicy": "never", "sandboxPolicy": {"type": "dangerFullAccess"}, "effort": "low",
    }
    params.update(extra)
    return c.request("turn/start", params)


def tid(resp: dict) -> str | None:
    return ((resp.get("result") or {}).get("thread") or {}).get("id")


def summarize(resp: dict) -> dict:
    """A response trimmed to what the report needs."""
    if "error" in resp:
        return {"error": resp["error"], "latency_s": resp.get("_latency_s")}
    if "result" in resp:
        r = resp["result"]
        if isinstance(r, dict) and "thread" in r:
            r = {k: v for k, v in r.items() if k not in ("thread", "instructionSources")}
        return {"result": r, "latency_s": resp.get("_latency_s")}
    return resp


def command_outputs(c: Client, thread: str) -> list[dict]:
    out = []
    for t, m in c.notes("item/completed", thread):
        item = m["params"].get("item", {})
        if item.get("type") == "commandExecution":
            out.append({"at": t, "command": item.get("command"), "exitCode": item.get("exitCode"),
                        "status": item.get("status"), "source": item.get("source"),
                        "output": (item.get("aggregatedOutput") or "")[-3000:]})
    return out


def agent_text(c: Client, thread: str) -> str:
    parts = []
    for _, m in c.notes("item/completed", thread):
        item = m["params"].get("item", {})
        if item.get("type") == "agentMessage":
            parts.append(item.get("text") or "")
    return "\n".join(parts)


def env_probe_cmd(tag: str) -> str:
    return (f"echo TAG={tag}; echo PWD=$(pwd); echo RUN=${{ONEPIPELINE_RUN_ID-<unset>}}; "
            "echo SRV=${SPIKE_SERVER_ONLY-<unset>}; echo POL=${SPIKE_POLICY_VAR-<unset>}; echo PPID=$PPID")


def parse_kv(text: str) -> dict:
    return dict(re.findall(r"^(TAG|PWD|RUN|SRV|POL|PPID)=(.*)$", text or "", re.M))


def shell(c: Client, thread: str, cmd: str, timeout=30) -> dict:
    """thread/shellCommand and wait for its commandExecution item."""
    since = len(c.events)
    r = c.request("thread/shellCommand", {"threadId": thread, "command": cmd})
    if "error" in r:
        return {"rpc": summarize(r)}
    done = c.wait_for(lambda m: m.get("method") == "item/completed" and m["params"].get("threadId") == thread
                      and m["params"].get("item", {}).get("type") == "commandExecution", timeout, since)
    if not done:
        return {"rpc": summarize(r), "item": None}
    item = done[1]["params"]["item"]
    return {"rpc": summarize(r), "exitCode": item.get("exitCode"), "output": item.get("aggregatedOutput"),
            "parsed": parse_kv(item.get("aggregatedOutput"))}


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    try:
        st = Path(f"/proc/{pid}/stat").read_text()
        return st.split(")")[-1].split()[0] != "Z"
    except OSError:
        return False


def proc_parent(pid: int):
    try:
        return int(Path(f"/proc/{pid}/stat").read_text().split(")")[-1].split()[1])
    except OSError:
        return None


def marker_pids(marker: str) -> list[int]:
    """Processes whose environment carries `SPIKE_LEASE_MARKER=<marker>`."""
    out = []
    needle = f"SPIKE_LEASE_MARKER={marker}".encode()
    for d in Path("/proc").iterdir():
        if d.name.isdigit():
            try:
                if needle in (d / "environ").read_bytes().split(b"\0"):
                    out.append(int(d.name))
            except OSError:
                pass
    return sorted(out)


def real_server_pid(pid: int) -> int:
    """`codex` on PATH may be the npm Node shim, which spawns the native binary
    as a child; the app-server is that child."""
    for _ in range(50):
        try:
            kids = Path(f"/proc/{pid}/task/{pid}/children").read_text().split()
        except OSError:
            return pid
        for k in kids:
            try:
                if b"app-server" in Path(f"/proc/{k}/cmdline").read_bytes():
                    return int(k)
            except OSError:
                pass
        if b"node" not in Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")[0]:
            return pid
        time.sleep(0.05)
    return pid


def hb_script(wd: Path, seconds: int) -> Path:
    s = wd / "hb.sh"
    s.write_text(
        "#!/bin/bash\n"
        f"echo $$ > {wd}/hb.pid\n"
        f"for i in $(seq 1 {seconds * 10}); do date +%s.%N >> {wd}/hb.log; sleep 0.1; done\n"
        f"echo finished >> {wd}/hb.log\n"
    )
    s.chmod(0o755)
    return s


def hb_lines(wd: Path):
    try:
        lines = (wd / "hb.log").read_text().splitlines()
    except OSError:
        return 0, None
    stamps = [float(x) for x in lines if re.fullmatch(r"\d+\.\d+", x)]
    return len(lines), (stamps[-1] if stamps else None)


# --------------------------------------------------------------------------
# Scenarios
# --------------------------------------------------------------------------


def sc_transport(work: Path, a) -> dict:
    """Q1. Own `--listen unix://` server vs Codex's `daemon` and `proxy`."""
    res: dict = {"listen": {}, "daemon": {}, "proxy": {}}
    # --- own server: lifecycle facts the lease pool depends on
    s = Server(work, "own", ["--disable", "plugins"])
    st = os.lstat(s.sock)
    res["listen"]["socket_is_symlink"] = os.path.islink(s.sock)
    res["listen"]["socket_target"] = os.readlink(s.sock) if os.path.islink(s.sock) else None
    res["listen"]["ready_s"] = round(s.ready_at - s.launched_at, 3)
    c1 = Client(s.sock, "a")
    c1.initialize()
    c1.close()
    time.sleep(2)
    res["listen"]["alive_after_last_client_disconnects_2s"] = s.alive()
    # second server on the same path while the first runs
    try:
        s2 = Server(work, "own-dup", ["--disable", "plugins"], sock=s.sock)
        time.sleep(3)
        res["listen"]["second_server_same_path"] = {"launched": True, "second_alive_after_3s": s2.alive(),
                                                     "second_returncode": s2.proc.poll(), "first_alive": s.alive(),
                                                     "second_log": s2.log_path.read_text()[-400:]}
        s2.stop()
    except Exception as e:  # noqa: BLE001
        res["listen"]["second_server_same_path"] = {"started": False, "error": str(e)[-600:]}
    res["listen"]["first_alive_after_dup_attempt"] = s.alive()
    target = res["listen"]["socket_target"]
    res["listen"]["launcher_is_node_shim"] = s.real_pid != s.pid
    res["listen"]["stop_sigterm"] = s.stop(signal.SIGTERM)
    res["listen"]["socket_path_left_after_sigterm"] = os.path.lexists(s.sock)
    res["listen"]["socket_target_left_after_sigterm"] = bool(target) and os.path.exists(target)
    s3 = Server(work, "own-kill", ["--disable", "plugins"])
    tgt3 = os.readlink(s3.sock) if os.path.islink(s3.sock) else s3.sock
    res["listen"]["stop_sigkill"] = s3.stop(signal.SIGKILL)
    res["listen"]["socket_path_left_after_sigkill"] = os.path.lexists(s3.sock)
    res["listen"]["socket_target_left_after_sigkill"] = os.path.exists(tgt3)
    try:
        WS(s3.sock, timeout=2)
        res["listen"]["connect_to_stale_socket"] = "connected?!"
    except OSError as e:
        res["listen"]["connect_to_stale_socket"] = f"{type(e).__name__}: {e}"
    try:
        s4 = Server(work, "own-restart", ["--disable", "plugins"], sock=s3.sock)
        res["listen"]["restart_on_stale_path"] = {"started": True, "ready_s": round(s4.ready_at - s4.launched_at, 3)}
        s4.stop()
    except Exception as e:  # noqa: BLE001
        res["listen"]["restart_on_stale_path"] = {"started": False, "error": str(e)[-600:]}

    # --- Codex's own daemon, under two scratch CODEX_HOMEs (no credentials needed)
    homes = []
    for n in ("h1", "h2"):
        h = work / f"daemon-{n}"
        h.mkdir(exist_ok=True)
        (h / "config.toml").write_text("[features]\nplugins = false\n")
        homes.append(h)

    def daemon(h, *args, timeout=120):
        r = subprocess.run([CODEX, "app-server", "daemon", *args], env={**os.environ, "CODEX_HOME": str(h)},
                           capture_output=True, text=True, timeout=timeout)
        try:
            body = json.loads(r.stdout.strip().splitlines()[-1])
        except Exception:  # noqa: BLE001
            body = r.stdout.strip()[-500:]
        return {"rc": r.returncode, "out": body, "err": r.stderr.strip()[-400:]}

    def daemon_procs():
        out = subprocess.run(["ps", "-eo", "pid,ppid,sid,args"], capture_output=True, text=True).stdout
        return [ln.strip() for ln in out.splitlines() if str(work) in ln and "daemon" in ln or ("managed-daemon" in ln and str(work) in ln)]

    d = res["daemon"]
    t = time.monotonic()
    d["start_h1"] = daemon(homes[0], "start", "--disable", "plugins", "-c", 'model="gpt-5.5"')
    d["start_h1_s"] = round(time.monotonic() - t, 2)
    d["start_h2"] = daemon(homes[1], "start")
    d["restart_h1_with_other_args"] = daemon(homes[0], "start", "-c", 'model="gpt-5.6-luna"', "--enable", "plugins")
    d["processes"] = daemon_procs()
    d["package_copy_bytes"] = sum(f.stat().st_size for f in (homes[0] / "packages").rglob("*") if f.is_file()) if (homes[0] / "packages").exists() else None
    d["updater_log"] = (homes[0] / "app-server-daemon" / "daemon-updater.stderr.log").read_text()[-300:] if (homes[0] / "app-server-daemon" / "daemon-updater.stderr.log").exists() else None
    d["auto_update_version_file"] = (homes[0] / "packages/app-server-daemon/auto-update-version").read_text().strip() if (homes[0] / "packages/app-server-daemon/auto-update-version").exists() else None
    # did the daemon apply the -c model the first start passed? read it back over its socket
    sockp = d["start_h1"]["out"].get("socketPath") if isinstance(d["start_h1"]["out"], dict) else None
    if sockp:
        try:
            c = Client(sockp, "daemon")
            c.initialize()
            cfg = c.request("config/read", {})
            conf = (cfg.get("result") or {}).get("config") or {}
            d["h1_effective_model"] = conf.get("model")
            d["h1_effective_features_plugins"] = (conf.get("features") or {}).get("plugins")
            c.close()
        except Exception as e:  # noqa: BLE001
            d["h1_socket_error"] = str(e)
    # proxy: stdio bytes <-> the daemon's socket; does it frame JSON lines?
    if sockp:
        p = subprocess.Popen([CODEX, "app-server", "proxy", "--sock", sockp], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=open(work / "proxy.err", "w"), text=True, start_new_session=True,
                             env={**os.environ, "CODEX_HOME": str(homes[0])})
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "proxy", "title": "p", "version": "0"}}}) + "\n")
        p.stdin.flush()
        line = {"v": None}

        def rd():
            line["v"] = p.stdout.readline()

        th = threading.Thread(target=rd, daemon=True)
        th.start()
        th.join(15)
        res["proxy"]["first_line"] = (line["v"] or "")[:300]
        os.killpg(p.pid, signal.SIGKILL)  # the proxy's own process group, started above
        p.wait()
        res["proxy"]["stderr"] = (work / "proxy.err").read_text()[-300:]
    d["stop_h1"] = daemon(homes[0], "stop")
    d["stop_h2"] = daemon(homes[1], "stop")
    time.sleep(1)
    d["processes_after_stop"] = daemon_procs()
    for h in homes:
        shutil.rmtree(h, ignore_errors=True)
    return res


def _isolation_threads(c1: Client, c2: Client, work: Path):
    wa, wb = work / "wd-a", work / "wd-b"
    wa.mkdir(exist_ok=True)
    wb.mkdir(exist_ok=True)
    ta = thread_start(c1, str(wa), model="gpt-5.5", baseInstructions="You are spike thread A. Be terse.",
                      config={"shell_environment_policy": {"set": {"ONEPIPELINE_RUN_ID": "run-A"}}})
    tb = thread_start(c2, str(wb), model="gpt-5.6-luna", sandbox="workspace-write", approvalPolicy="on-request",
                      baseInstructions="You are spike thread B. Be terse.",
                      config={"shell_environment_policy.set": {"ONEPIPELINE_RUN_ID": "run-B"}})
    return wa, wb, ta, tb


SERVER_ENV = {"SPIKE_SERVER_ONLY": "from-server-env", "ONEPIPELINE_RUN_ID": "run-SERVER"}


def sc_isolation_free(work: Path, a) -> dict:
    """Q2/Q3 without a model: thread/start overrides + thread/shellCommand env."""
    s = Server(work, "iso-free", ["--disable", "plugins"], env_extra=SERVER_ENV)
    c1, c2 = Client(s.sock, "A"), Client(s.sock, "B")
    c1.initialize(), c2.initialize()
    res: dict = {"server_env": SERVER_ENV}
    wa, wb, ta, tb = _isolation_threads(c1, c2, work)
    res["thread_start_A"] = summarize(ta)
    res["thread_start_B"] = summarize(tb)
    res["shell_A"] = shell(c1, tid(ta), env_probe_cmd("A"))
    res["shell_B"] = shell(c2, tid(tb), env_probe_cmd("B"))
    # other connection's thread: can B's connection drive A's thread?
    res["shell_on_other_connections_thread"] = shell(c2, tid(ta), "echo cross", timeout=10)
    # variants of the per-thread config map
    variants = {
        "nested_set": {"shell_environment_policy": {"set": {"SPIKE_POLICY_VAR": "v-nested"}}},
        "dotted_set_key": {"shell_environment_policy.set.SPIKE_POLICY_VAR": "v-dotted"},
        "inherit_none": {"shell_environment_policy": {"inherit": "none", "set": {"SPIKE_POLICY_VAR": "v-none"}}},
        "exclude_server_var": {"shell_environment_policy": {"exclude": ["SPIKE_SERVER_ONLY"], "set": {"SPIKE_POLICY_VAR": "v-excl"}}},
        "unknown_key": {"spike_bogus_key": 1},
        "bad_type": {"shell_environment_policy": {"set": "not-a-table"}},
        "feature_key": {"features": {"plugins": True}},
    }
    res["variants"] = {}
    for name, cfg in variants.items():
        t = thread_start(c1, str(wa), config=cfg)
        v = {"thread_start": summarize(t)}
        if tid(t):
            v["shell"] = shell(c1, tid(t), env_probe_cmd(name))
        res["variants"][name] = v
    time.sleep(1)
    for tag, c, own in (("A", c1, {tid(ta)} | {tid(t) for t in []}), ("B", c2, {tid(tb)})):
        res[f"connection_{tag}_frames_for_threads_it_did_not_start"] = sorted(
            {f"{m.get('method')}:{'A-thread' if (m.get('params') or {}).get('threadId') == tid(ta) else 'B-thread' if (m.get('params') or {}).get('threadId') == tid(tb) else 'variant-thread'}"
             for _, m in c.events if (m.get("params") or {}).get("threadId") not in (None, *own)})
    res["server_alive"] = s.alive()
    c1.close(), c2.close()
    s.stop()
    return res


def sc_isolation(work: Path, a) -> dict:
    """Q2/Q3 with the model: two clients, concurrent turns each running env."""
    s = Server(work, "iso", ["--disable", "plugins"], env_extra=SERVER_ENV)
    c1, c2 = Client(s.sock, "A"), Client(s.sock, "B")
    c1.initialize(), c2.initialize()
    res: dict = {"server_env": SERVER_ENV}
    wa, wb, ta, tb = _isolation_threads(c1, c2, work)
    res["thread_start_A"] = summarize(ta)
    res["thread_start_B"] = summarize(tb)
    prompt = "Run this exact shell command once, then reply with only its output: {}"
    pa = prompt.format(env_probe_cmd("A"))
    pb = prompt.format(env_probe_cmd("B"))
    ra = turn_start(c1, tid(ta), str(wa), pa)
    rb = turn_start(c2, tid(tb), str(wb), pb)
    res["turn_start_A"] = summarize(ra)
    res["turn_start_B"] = summarize(rb)
    da = c1.wait_for(turn_completed(tid(ta)), 240)
    db = c2.wait_for(turn_completed(tid(tb)), 240)
    for tag, c, t, d, p in (("A", c1, tid(ta), da, pa), ("B", c2, tid(tb), db, pb)):
        cmds = command_outputs(c, t)
        out = {
            "completed": d[1]["params"]["turn"]["status"] if d and "params" in d[1] else d,
            "commands": cmds,
            "parsed": [parse_kv(x["output"]) for x in cmds],
            "agent": agent_text(c, t)[-800:],
            "model_rerouted": [m["params"] for _, m in c.notes("model/rerouted", t)],
            "foreign_thread_frames": sum(1 for _, m in c.events if (m.get("params") or {}).get("threadId") not in (None, t)),
            "foreign_thread_frame_methods": sorted({m.get("method") for _, m in c.events if (m.get("params") or {}).get("threadId") not in (None, t)}),
            "frames_without_thread": sorted({m.get("method") for _, m in c.events if "method" in m and "threadId" not in (m.get("params") or {})}),
        }
        res[f"turn_{tag}"] = out
        record_paid(work, "isolation", tag, p, {"status": out["completed"]})
    c1.close(), c2.close()
    s.stop()
    return res


def _killed(work: Path, a, via: str) -> dict:
    wk, wb = work / "wd-kill", work / "wd-other"
    for w in (wk, wb):
        shutil.rmtree(w, ignore_errors=True)
        w.mkdir()
    hb_script(wk, 90)
    s = Server(work, f"kill-{via}", ["--disable", "plugins"])
    res: dict = {"via": via, "server_pid": s.pid}
    info = work / "victim.json"
    info.unlink(missing_ok=True)
    marker = str(uuid.uuid4())
    res["lease_marker"] = marker
    victim = subprocess.Popen([sys.executable, __file__, "_victim", "--work", str(work), "--sock", s.sock, "--via", via, "--wd", str(wk), "--marker", marker],
                              stderr=open(work / "victim.err", "w"))
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline and hb_lines(wk)[0] < 10:
        if victim.poll() is not None:
            raise RuntimeError(f"victim exited early: {(work / 'victim.err').read_text()[-2000:]}")
        time.sleep(0.1)
    if hb_lines(wk)[0] < 10:
        victim.kill()
        res["error"] = "heartbeat never started"
        res["victim_err"] = (work / "victim.err").read_text()[-3000:]
        s.stop()
        return res
    vi = json.loads(info.read_text())
    res["victim"] = vi
    hb_pid = int((wk / "hb.pid").read_text())
    res["heartbeat_pid"] = hb_pid
    res["heartbeat_parent_chain"] = []
    p = hb_pid
    for _ in range(6):
        p = proc_parent(p)
        if not p:
            break
        try:
            cmd = Path(f"/proc/{p}/cmdline").read_bytes().replace(b"\0", b" ").decode()[:120]
        except OSError:
            cmd = None
        res["heartbeat_parent_chain"].append({"pid": p, "cmd": cmd})
        if p == s.pid:
            break
    res["heartbeat_pgid_sid"] = {"pgid": os.getpgid(hb_pid), "sid": os.getsid(hb_pid),
                                 "server_pgid": os.getpgid(s.real_pid), "own_group": os.getpgid(hb_pid) != os.getpgid(s.real_pid)}
    res["marker_pids_while_running"] = marker_pids(marker)
    # the other dispatch: its own connection and live turn
    other = Client(s.sock, "other")
    other.initialize()
    to = thread_start(other, str(wb))
    other_prompt = "Run `sleep 8; echo OTHER-OK` once and reply with only its output."
    if via == "turn" and a.no_other:
        pass
    elif via == "turn":
        turn_start(other, tid(to), str(wb), other_prompt)
    else:
        other.request("thread/shellCommand", {"threadId": tid(to), "command": "sleep 8; echo OTHER-OK"})
    time.sleep(0.5)
    # SIGKILL the victim client mid-turn
    killed_at_wall = time.time()
    killed_at = now()
    victim.kill()
    victim.wait()
    res["killed_at_s"] = killed_at
    observer = Client(s.sock, "observer")
    observer.initialize()

    def turn_status():
        r = observer.request("thread/read", {"threadId": vi["threadId"], "includeTurns": True}, timeout=10)
        th = (r.get("result") or {}).get("thread") or {}
        turns = th.get("turns") or []
        return {"thread_status": th.get("status"), "last_turn": ({"id": turns[-1].get("id"), "status": turns[-1].get("status")} if turns else None),
                "error": r.get("error")}

    samples = []
    observe_dead = a.observe_dead
    end = time.monotonic() + observe_dead
    while time.monotonic() < end:
        n, last = hb_lines(wk)
        samples.append({"t": round(now() - killed_at, 2), "hb_lines": n, "hb_pid_alive": pid_alive(hb_pid), **turn_status()})
        time.sleep(1)
    res["after_kill_samples"] = samples
    n_before = samples[0]["hb_lines"]
    n_after = samples[-1]["hb_lines"]
    res["heartbeat_kept_growing_after_kill"] = n_after > n_before + 5
    res["heartbeat_lines_gained_in_observation"] = n_after - n_before
    # attach the observer to the orphaned thread, as a reaper would, then interrupt
    rs = observer.request("thread/resume", {"threadId": vi["threadId"], "excludeTurns": True})
    res["observer_thread_resume"] = summarize(rs)
    res["observer_turn_completed_before_interrupt"] = bool(observer.notes("turn/completed", vi["threadId"]))
    turn_id = vi.get("turnId")
    since_int = len(observer.events)
    t_int = now()
    int_wall = time.time()
    ir = observer.request("turn/interrupt", {"threadId": vi["threadId"], "turnId": turn_id or "none"})
    res["interrupt_response"] = summarize(ir)
    res["interrupt_sent_s_after_kill"] = round(t_int - killed_at, 3)
    turn_end = None
    last_growth_wall = None
    prev = hb_lines(wk)[0]
    samples2 = []
    end = time.monotonic() + 30
    while time.monotonic() < end:
        st = turn_status()
        n, last = hb_lines(wk)
        alive = pid_alive(hb_pid)
        samples2.append({"t": round(now() - t_int, 2), "hb_lines": n, "hb_pid_alive": alive, **st})
        if turn_end is None and st["last_turn"] and st["last_turn"]["status"] not in ("inProgress", "in_progress"):
            turn_end = round(now() - t_int, 3)
        if n > prev:
            last_growth_wall = last
        prev = n
        if turn_end is not None and not alive and len(samples2) > 3:
            break
        time.sleep(0.25)
    res["after_interrupt_samples"] = samples2[::2]
    tc = observer.wait_for(is_note("turn/completed", vi["threadId"]), 1, since_int)
    res["turn_completed_notification_s_after_interrupt"] = round(tc[0] - t_int, 3) if tc else None
    res["turn_completed_notification_status"] = tc[1]["params"]["turn"].get("status") if tc else None
    n, last = hb_lines(wk)
    res["turn_end_s_after_interrupt"] = turn_end
    res["turn_end_s_after_kill"] = round(turn_end + (t_int - killed_at), 3) if turn_end is not None else None
    res["heartbeat_last_line_s_after_interrupt"] = round(last - int_wall, 3) if last else None
    res["heartbeat_last_line_s_after_kill"] = round(last - killed_at_wall, 3) if last else None
    res["heartbeat_pid_alive_after_interrupt"] = pid_alive(hb_pid)
    if pid_alive(hb_pid):
        time.sleep(5)
        res["heartbeat_pid_alive_5s_later"] = pid_alive(hb_pid)
        res["heartbeat_lines_still_growing"] = hb_lines(wk)[0] > n
    if pid_alive(hb_pid) and a.reap:
        # does Codex reap the thread's processes when the thread is unloaded?
        res["unsubscribe"] = summarize(observer.request("thread/unsubscribe", {"threadId": vi["threadId"]}))
        time.sleep(5)
        res["heartbeat_pid_alive_5s_after_unsubscribe"] = pid_alive(hb_pid)
        res["thread_closed_seen"] = bool(observer.notes("thread/closed", vi["threadId"]))
        if pid_alive(hb_pid):
            pids = marker_pids(marker)
            res["marker_pids_after_interrupt"] = pids
            t_r = time.time()
            for p in pids:  # processes carrying this spike's own per-thread marker
                try:
                    os.kill(p, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            time.sleep(2)
            n2, last2 = hb_lines(wk)
            res["marker_reap"] = {"heartbeat_alive": pid_alive(hb_pid), "last_line_s_after_reap": round(last2 - t_r, 3) if last2 else None,
                                  "marker_pids_left": marker_pids(marker)}
    # the other client's turn
    if via == "turn" and a.no_other:
        record_paid(work, f"killed-{via}", "victim", vi.get("prompt"), {"interrupted": res["interrupt_response"]})
    elif via == "turn":
        d = other.wait_for(turn_completed(tid(to)), 120)
        res["other_turn"] = {"status": d[1]["params"]["turn"]["status"] if d and "params" in d[1] else d,
                             "commands": command_outputs(other, tid(to)), "agent": agent_text(other, tid(to))[-300:]}
        record_paid(work, f"killed-{via}", "other", other_prompt, res["other_turn"]["status"])
        record_paid(work, f"killed-{via}", "victim", vi.get("prompt"), {"interrupted": res["interrupt_response"]})
    else:
        d = other.wait_for(lambda m: m.get("method") == "item/completed" and m["params"].get("item", {}).get("type") == "commandExecution", 60)
        res["other_shell"] = {"output": d[1]["params"]["item"].get("aggregatedOutput") if d else None,
                              "exitCode": d[1]["params"]["item"].get("exitCode") if d else None}
    # what stops a surviving command: the server's exit?
    still = pid_alive(hb_pid)
    res["server_stop"] = s.stop()
    time.sleep(1)
    res["heartbeat_pid_alive_after_server_stop"] = pid_alive(hb_pid) if still else "already-gone"
    if pid_alive(hb_pid):
        os.kill(hb_pid, signal.SIGKILL)  # our own spike's command, pid read from its own pidfile
    observer.close(), other.close()
    return res


def sc_killed_shell(work, a):
    return _killed(work, a, "shell")


def sc_killed_client(work, a):
    return _killed(work, a, "turn")


def victim_main(a):
    """Child process: the dispatch that gets SIGKILLed mid-turn."""
    work, wd = Path(a.work), Path(a.wd)
    c = Client(a.sock, "victim")
    c.initialize()
    t = thread_start(c, str(wd), ephemeral=False,  # so an observer can thread/read its turns
                     config={"shell_environment_policy": {"set": {"SPIKE_LEASE_MARKER": a.marker}}})
    th = tid(t)
    info = {"threadId": th, "pid": os.getpid()}
    if a.via == "turn":
        prompt = f"Run `bash {wd}/hb.sh` in the foreground and wait until it finishes, then reply DONE."
        info["prompt"] = prompt
        r = turn_start(c, th, str(wd), prompt)
        info["turnId"] = ((r.get("result") or {}).get("turn") or {}).get("id")
    else:
        r = c.request("thread/shellCommand", {"threadId": th, "command": f"bash {wd}/hb.sh"})
        info["shell_rpc"] = summarize(r)
        st = c.wait_for(is_note("turn/started", th), 10)
        info["turnId"] = st[1]["params"]["turn"]["id"] if st else None
    (work / "victim.json").write_text(json.dumps(info))
    time.sleep(600)


def sc_interrupt(work: Path, a) -> dict:
    """Q5: what turn/interrupt answers in each situation."""
    wd = work / "wd-int"
    wd.mkdir(exist_ok=True)
    sock = str(work / "int.sock")
    s = Server(work, "int", ["--disable", "plugins"], sock=sock)
    c = Client(s.sock, "D")
    c.initialize()
    res: dict = {}
    t = thread_start(c, str(wd))
    th = tid(t)
    res["no_turn_ever_bogus_turnId"] = summarize(c.request("turn/interrupt", {"threadId": th, "turnId": "bogus"}))
    res["unknown_thread"] = summarize(c.request("turn/interrupt", {"threadId": str(uuid.uuid4()), "turnId": "bogus"}))
    res["malformed_thread_id"] = summarize(c.request("turn/interrupt", {"threadId": "not-a-uuid", "turnId": "x"}))
    # a shellCommand: is it a turn, and does interrupt stop it?
    sr = c.request("thread/shellCommand", {"threadId": th, "command": "sleep 30"})
    st = c.wait_for(is_note("turn/started", th), 10)
    sh_turn = st[1]["params"]["turn"]["id"] if st else None
    res["shell_command_is_a_turn"] = sh_turn is not None
    time.sleep(1)
    if sh_turn:
        ti = now()
        res["shell_turn_interrupt"] = summarize(c.request("turn/interrupt", {"threadId": th, "turnId": sh_turn}))
        d = c.wait_for(lambda m: m.get("method") == "turn/completed" and m["params"]["turn"]["id"] == sh_turn, 20)
        res["shell_turn_completed"] = {"status": d[1]["params"]["turn"]["status"], "s_after_interrupt": round(d[0] - ti, 3)} if d else None
    # active model turn on our own connection
    prompt = "Run `sleep 30` and then reply with the word done."
    r = turn_start(c, th, str(wd), prompt)
    turn_id = ((r.get("result") or {}).get("turn") or {}).get("id")
    c.wait_for(lambda m: m.get("method") == "item/started" and m["params"].get("item", {}).get("type") == "commandExecution", 60)
    ti = now()
    res["active_turn_interrupt"] = summarize(c.request("turn/interrupt", {"threadId": th, "turnId": turn_id}))
    d = c.wait_for(lambda m: m.get("method") == "turn/completed" and m["params"]["turn"]["id"] == turn_id, 30)
    res["active_turn_completed"] = {"status": d[1]["params"]["turn"]["status"], "s_after_interrupt": round(d[0] - ti, 3)} if d else None
    record_paid(work, "interrupt", "active", prompt, res["active_turn_completed"])
    res["active_turn_interrupt_mismatched_turnId"] = None
    res["after_turn_ended_same_turnId"] = summarize(c.request("turn/interrupt", {"threadId": th, "turnId": turn_id}))
    res["no_active_turn_bogus_turnId"] = summarize(c.request("turn/interrupt", {"threadId": th, "turnId": "bogus"}))
    res["other_connection_interrupt_idle_thread"] = None
    o = Client(s.sock, "O")
    o.initialize()
    res["other_connection_interrupt_idle_thread"] = summarize(o.request("turn/interrupt", {"threadId": th, "turnId": turn_id}))
    o.close()
    # restart: kill and relaunch on the same path
    res["server_stop"] = s.stop()
    res["old_connection_after_server_exit"] = {"ended": c.ended,
                                               "request": summarize(c.request("turn/interrupt", {"threadId": th, "turnId": turn_id}, timeout=5))}
    s2 = Server(work, "int2", ["--disable", "plugins"], sock=sock)
    c2 = Client(s2.sock, "D2")
    c2.initialize()
    res["after_restart_new_connection"] = summarize(c2.request("turn/interrupt", {"threadId": th, "turnId": turn_id}))
    res["after_restart_thread_read"] = summarize(c2.request("thread/read", {"threadId": th}))
    c2.close()
    s2.stop()
    try:
        WS(sock, timeout=2)
        res["no_server_connect"] = "connected?!"
    except OSError as e:
        res["no_server_connect"] = f"{type(e).__name__}: {e}"
    return res


def sc_blast(work: Path, a) -> dict:
    """Q6: kill the shared server under two live turns."""
    sig = signal.SIGKILL if a.signal == "KILL" else signal.SIGTERM
    s = Server(work, "blast", ["--disable", "plugins"])
    res: dict = {"signal": sig.name, "via": a.via}
    clients = []
    for n in ("A", "B"):
        wd = work / f"wd-blast-{n}"
        shutil.rmtree(wd, ignore_errors=True)
        wd.mkdir()
        c = Client(s.sock, n)
        c.initialize()
        t = thread_start(c, str(wd))
        th = tid(t)
        prompt = f"Run `sleep 30; echo {n}` in the foreground once and reply with only its output."
        if a.via == "turn":
            turn_start(c, th, str(wd), prompt)
        else:
            c.request("thread/shellCommand", {"threadId": th, "command": "echo $$ > cmd.pid; sleep 30"})
        clients.append((n, c, th, wd, prompt))
    for n, c, th, wd, _ in clients:
        c.wait_for(lambda m: m.get("method") == "item/started" and m["params"].get("item", {}).get("type") == "commandExecution", 90)
    time.sleep(1)
    cmd_pids = {}
    for n, c, th, wd, _ in clients:
        try:
            cmd_pids[n] = int((wd / "cmd.pid").read_text())
        except OSError:
            pass
    k = now()
    os.kill(s.real_pid, sig)
    for n, c, th, wd, prompt in clients:
        before = len(c.events)
        got = c.wait_for(lambda m: "_ended" in m, 20)
        frames = [m for _, m in c.events[before:]]
        res[n] = {"ended": c.ended, "s_after_kill": round(got[0] - k, 3) if got else None,
                  "frames_after_kill": [{k2: v for k2, v in f.items() if k2 != "params"} | {"params_keys": list((f.get("params") or {}).keys())} for f in frames][-5:],
                  "turn_completed_seen": any(f.get("method") == "turn/completed" for f in frames),
                  "send_after": summarize(c.request("thread/read", {"threadId": th}, timeout=5))}
        if a.via == "turn":
            record_paid(work, "blast", n, prompt, {"server_killed": sig.name})
    s.proc.wait(15)
    res["server_returncode"] = s.proc.returncode
    time.sleep(0.5)
    res["command_pids_alive_after_server_death"] = {n: pid_alive(p) for n, p in cmd_pids.items()}
    for p in cmd_pids.values():
        if pid_alive(p):
            os.kill(p, signal.SIGKILL)  # our own spike's sleep, pid from its own pidfile
    try:
        WS(s.sock, timeout=2)
        res["reconnect"] = "connected?!"
    except OSError as e:
        res["reconnect"] = f"{type(e).__name__}: {e}"
    return res


def sc_savings(work: Path, a) -> dict:
    """Q7: time to first turn and bytes, fresh spawn vs warm shared server."""
    wd = work / "wd-save"
    wd.mkdir(exist_ok=True)
    res: dict = {"method": "ss -tinp sampling of the server pid every 100 ms (lower bound; no spike-network-accounting branch)"}
    prompt = "Reply with the single word OK."

    def dispatch(server: Server, sampler: NetSampler, label: str, t_start: float, paid: bool):
        before = sampler.snapshot()
        c = Client(server.sock, label)
        c.initialize()
        t_init = now()
        t = thread_start(c, str(wd))
        t_thread = now()
        out = {"connect_init_s": round(t_init - t_start, 3), "thread_ready_s": round(t_thread - t_start, 3)}
        if paid:
            turn_start(c, tid(t), str(wd), prompt)
            st = c.wait_for(is_note("turn/started", tid(t)), 60)
            fd = c.wait_for(is_note("item/agentMessage/delta", tid(t)), 120)
            dn = c.wait_for(turn_completed(tid(t)), 120)
            out["turn_started_s"] = round(st[0] - t_start, 3) if st else None
            out["first_token_s"] = round(fd[0] - t_start, 3) if fd else None
            out["turn_completed_s"] = round(dn[0] - t_start, 3) if dn else None
            out["text"] = agent_text(c, tid(t))[:50]
            record_paid(work, "savings", label, prompt, {"status": dn[1]["params"]["turn"]["status"] if dn and "params" in dn[1] else None})
        time.sleep(1.5)
        out["network"] = NetSampler.delta(before, sampler.snapshot())
        c.close()
        return out

    # unpaid: startup to thread-ready, three of each
    res["unpaid_fresh"] = []
    res["unpaid_warm"] = []
    for i in range(3):
        t0 = now()
        s = Server(work, f"fresh-{i}", ["--disable", "plugins"])
        smp = NetSampler(s.real_pid)
        res["unpaid_fresh"].append(dispatch(s, smp, f"fresh{i}", t0, False))
        res["unpaid_warm"].append(dispatch(s, smp, f"warm{i}", now(), False))
        res["unpaid_fresh"][-1]["server_lifetime_network"] = NetSampler.delta({}, smp.snapshot())
        smp.stop()
        s.stop()
    # paid: one fresh spawn's first turn, then a second dispatch on the warm server
    t0 = now()
    s = Server(work, "paid", ["--disable", "plugins"])
    smp = NetSampler(s.real_pid)
    res["paid_fresh"] = dispatch(s, smp, "fresh", t0, True)
    time.sleep(2)
    res["paid_warm"] = dispatch(s, smp, "warm", now(), True)
    res["paid_server_lifetime_network"] = NetSampler.delta({}, smp.snapshot())
    res["paid_connections"] = {f"{k[0]}->{k[1]}": v for k, v in smp.snapshot().items()}
    smp.stop()
    s.stop()
    return res


def sc_pool_key(work: Path, a) -> dict:
    """Q8: which settings a thread can override and which only the process sets."""
    res: dict = {}
    wd = work / "wd-key"
    wd.mkdir(exist_ok=True)
    s = Server(work, "key", ["--disable", "plugins", "-c", 'shell_environment_policy.set.SPIKE_POLICY_VAR="from-process-c"'],
               env_extra={"SPIKE_SERVER_ONLY": "from-server-env"})
    smp = NetSampler(s.real_pid)
    c = Client(s.sock, "K")
    c.initialize()
    cfg = c.request("config/read", {"cwd": str(wd)})
    conf = (cfg.get("result") or {}).get("config") or {}
    res["process_config_read"] = {"features.plugins": (conf.get("features") or {}).get("plugins"),
                                  "shell_environment_policy": conf.get("shell_environment_policy")}
    t = thread_start(c, str(wd))
    res["process_c_reaches_shell"] = shell(c, tid(t), env_probe_cmd("procC"))
    t2 = thread_start(c, str(wd), config={"shell_environment_policy": {"set": {"SPIKE_POLICY_VAR": "from-thread-config"}}})
    res["thread_config_vs_process_c"] = shell(c, tid(t2), env_probe_cmd("threadC"))
    # features.plugins=true per thread on a --disable plugins server: does the catalog download?
    before = smp.snapshot()
    t3 = thread_start(c, str(wd), config={"features": {"plugins": True}})
    res["thread_plugins_true_thread_start"] = summarize(t3)
    pl = c.request("plugin/list", {"cwds": [str(wd)]}, timeout=40)
    res["plugin_list_after_thread_plugins_true"] = summarize(pl) if "error" in pl else {"ok": True, "keys": list((pl.get("result") or {}).keys())}
    time.sleep(15)
    res["thread_plugins_true_network"] = NetSampler.delta(before, smp.snapshot())
    ef = c.request("experimentalFeature/list", {})
    res["experimental_features"] = [(f.get("name"), f.get("enabled"), f.get("stage")) for f in ((ef.get("result") or {}).get("data") or [])][:60]
    c.close()
    smp.stop()
    s.stop()
    # same server, CODEX_HOME is the process's: a thread cannot select another home
    s2 = Server(work, "key2", ["--disable", "plugins"])
    c = Client(s2.sock, "K2")
    c.initialize()
    acct = c.request("account/read", {})
    res["account_read_keys"] = list((acct.get("result") or {}).keys()) if "result" in acct else summarize(acct)
    t4 = thread_start(c, str(wd), config={"model_provider": "openai", "sqlite_home": str(work)})
    res["thread_config_process_scoped_keys"] = summarize(t4)
    c.close()
    s2.stop()
    res["version"] = subprocess.run([CODEX, "--version"], capture_output=True, text=True).stdout.strip()
    return res


SCENARIOS = {
    "transport": sc_transport,
    "isolation-free": sc_isolation_free,
    "isolation": sc_isolation,
    "killed-shell": sc_killed_shell,
    "killed-client": sc_killed_client,
    "interrupt": sc_interrupt,
    "blast": sc_blast,
    "savings": sc_savings,
    "pool-key": sc_pool_key,
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenario", choices=[*SCENARIOS, "_victim"])
    ap.add_argument("--work", help="scratch dir (default: $ONEPIPELINE_NODE_SCRATCH_DIR/spike or a temp dir)")
    ap.add_argument("--observe-dead", type=float, default=15.0, help="killed-*: seconds watched after SIGKILL before interrupting")
    ap.add_argument("--signal", choices=["KILL", "TERM"], default="KILL", help="blast: signal sent to the server")
    ap.add_argument("--via", choices=["turn", "shell"], default="turn", help="blast: live work is a model turn or a shellCommand")
    ap.add_argument("--no-other", action="store_true", help="killed-client: skip the second client's paid turn")
    ap.add_argument("--reap", action="store_true", help="killed-*: after the interrupt, try thread/unsubscribe then a marker-env reap")
    ap.add_argument("--marker", default="")
    ap.add_argument("--sock")
    ap.add_argument("--wd")
    a = ap.parse_args()
    if a.scenario == "_victim":
        return victim_main(a)
    base = a.work or (os.path.join(os.environ["ONEPIPELINE_NODE_SCRATCH_DIR"], "spike") if os.environ.get("ONEPIPELINE_NODE_SCRATCH_DIR") else tempfile.mkdtemp(prefix="spike-sas-"))
    work = Path(base)
    work.mkdir(parents=True, exist_ok=True)
    started = time.strftime("%Y-%m-%dT%H:%M:%S")
    try:
        obs = SCENARIOS[a.scenario](work, a)
        err = None
    except Exception as e:  # noqa: BLE001
        import traceback

        obs, err = None, traceback.format_exc()
    print(json.dumps({"scenario": a.scenario, "codex": subprocess.run([CODEX, "--version"], capture_output=True, text=True).stdout.strip(),
                      "started": started, "elapsed_s": now(), "paid_turns": PAID["n"], "observed": obs, "error": err}, indent=2, default=str))
    return 1 if err else 0


if __name__ == "__main__":
    sys.exit(main())
