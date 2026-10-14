#!/usr/bin/env bash
# What sharing one Codex app-server buys a second dispatch, measured against a
# real `codex app-server --listen`: start one, open one thread on it, then a
# second on a fresh connection — as a first and a second pooled dispatch would,
# with no model turn — and print one JSON object with, after each thread, the
# server's whole-tree resident memory (its pid and every descendant, summed
# VmRSS from /proc) and the time to that ready thread (the first from the
# server's launch, the second from its own connection).
#
#   scripts/measure-app-server-sharing.sh
#
# OH_MEASURE_CODEX names the Codex executable (default: `codex` on PATH). The
# server is launched as a default pooled dispatch keys it (plugins, apps and
# analytics off) and stopped before this exits. Linux only: the reading is
# /proc's. The README's pooling section quotes a run of this.
set -euo pipefail

codex="${OH_MEASURE_CODEX:-codex}"
command -v "$codex" >/dev/null 2>&1 || {
  echo "measure-app-server-sharing: no Codex executable '$codex'; install codex or set OH_MEASURE_CODEX" >&2
  exit 1
}
[ -r /proc/self/status ] || {
  echo "measure-app-server-sharing: needs Linux's /proc to read resident memory" >&2
  exit 1
}
command -v python3 >/dev/null 2>&1 || {
  echo "measure-app-server-sharing: needs python3 for its WebSocket client" >&2
  exit 1
}
exec python3 -I - "$codex" <<'PY'
import base64, json, os, socket, struct, subprocess, sys, tempfile, time

codex = sys.argv[1]
work = tempfile.mkdtemp(prefix="oh-measure-")
path = os.path.join(work, "app-server.sock")


def tree(root):
    parents = {}
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            stat = open(f"/proc/{name}/stat").read()
        except OSError:
            continue
        parents[int(name)] = int(stat.rsplit(")", 1)[1].split()[1])
    found, index = [root], 0
    while index < len(found):
        found += [pid for pid, ppid in parents.items() if ppid == found[index]]
        index += 1
    return sorted(found)


def rss(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) * 1024
    except OSError:
        pass
    return 0


def reading(pid):
    pids = tree(pid)
    return {"rss_bytes": sum(rss(p) for p in pids), "pids": pids}


class Connection:
    def __init__(self):
        target = os.readlink(path) if os.path.islink(path) else path
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(target)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            f"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode()
        )
        head = b""
        while not head.endswith(b"\r\n\r\n"):
            byte = self.sock.recv(1)
            if not byte:
                raise SystemExit("measure-app-server-sharing: the server closed the WebSocket handshake")
            head += byte
        if b" 101 " not in head.split(b"\r\n", 1)[0]:
            raise SystemExit(f"measure-app-server-sharing: the server refused the upgrade: {head[:80]!r}")
        self.next_id = 1

    def send(self, value):
        payload = json.dumps(value).encode()
        mask = os.urandom(4)
        n = len(payload)
        header = bytes([0x81]) + (
            bytes([0x80 | n]) if n < 126
            else bytes([0x80 | 126]) + struct.pack(">H", n) if n < 65536
            else bytes([0x80 | 127]) + struct.pack(">Q", n)
        )
        self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def exact(self, n):
        data = b""
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise SystemExit("measure-app-server-sharing: the server closed the connection")
            data += chunk
        return data

    def message(self):
        data = b""
        while True:
            first, second = self.exact(2)
            n = second & 0x7F
            if n == 126:
                n = struct.unpack(">H", self.exact(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self.exact(8))[0]
            body = self.exact(n)
            opcode = first & 0x0F
            if opcode == 0x8:
                raise SystemExit("measure-app-server-sharing: the server closed the connection")
            if opcode in (0x9, 0xA):
                continue
            data += body
            if first & 0x80:
                return json.loads(data)

    def request(self, method, params):
        ident = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        while True:
            frame = self.message()
            if frame.get("id") == ident and "method" not in frame:
                if "error" in frame:
                    raise SystemExit(f"measure-app-server-sharing: {method} was refused: {frame['error']}")
                return frame["result"]

    def ready_thread(self):
        self.request("initialize", {"clientInfo": {"name": "oneharness-measure", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        return self.request("thread/start", {"cwd": work, "approvalPolicy": "never"})["thread"]["id"]


version = subprocess.run([codex, "--version"], capture_output=True, text=True).stdout.strip()
launched = time.monotonic()
server = subprocess.Popen(
    [codex, "app-server", "-c", "features.plugins=false", "-c", "features.apps=false",
     "-c", "analytics.enabled=false", "--listen", f"unix://{path}"],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=work,
)
try:
    deadline = time.monotonic() + 60
    first = None
    while first is None:
        try:
            first = Connection()
        except OSError:
            if server.poll() is not None or time.monotonic() > deadline:
                raise SystemExit("measure-app-server-sharing: the server never accepted a connection")
            time.sleep(0.02)
    threads = []
    first.ready_thread()
    threads.append({"thread": 1, "ready_ms": round((time.monotonic() - launched) * 1000), **reading(server.pid)})
    started = time.monotonic()
    second = Connection()
    second.ready_thread()
    threads.append({"thread": 2, "ready_ms": round((time.monotonic() - started) * 1000), **reading(server.pid)})
    print(json.dumps({"codex_version": version, "platform": sys.platform, "server_pid": server.pid,
                      "threads": threads}, indent=2))
finally:
    server.terminate()
    try:
        server.wait(10)
    except subprocess.TimeoutExpired:
        server.kill()
        server.wait()
    for leftover in os.listdir(work):
        try:
            os.remove(os.path.join(work, leftover))
        except OSError:
            pass
    os.rmdir(work)
PY
