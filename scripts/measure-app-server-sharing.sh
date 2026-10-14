#!/usr/bin/env bash
# What sharing one Codex app-server buys a second dispatch, measured against a
# real `codex app-server --listen`: start one, open one thread on it, then a
# second on a fresh connection — as a first and a second pooled dispatch would,
# with no model turn — and print one compact JSON object with, after each thread, the
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
  echo "measure-app-server-sharing: needs /proc to read resident memory; run it on Linux with /proc mounted" >&2
  exit 1
}
command -v python3 >/dev/null 2>&1 || {
  echo "measure-app-server-sharing: needs python3 for its WebSocket client; install python3 or put it on PATH, then rerun" >&2
  exit 1
}
exec python3 -I - "$codex" <<'PY'
import base64, hashlib, json, os, socket, struct, subprocess, sys, tempfile, time

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


def stop(why):
    raise SystemExit(f"measure-app-server-sharing: {why}")


GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


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
                stop("the server closed the WebSocket handshake; check that the executable is a codex that supports `app-server --listen`")
            head += byte
        lines = head.decode(errors="replace").split("\r\n")
        headers = {k.strip().lower(): v.strip() for k, _, v in (l.partition(":") for l in lines[1:] if ":" in l)}
        accept = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()
        if " 101 " not in lines[0] or headers.get("upgrade", "").lower() != "websocket" \
                or "upgrade" not in headers.get("connection", "").lower() \
                or headers.get("sec-websocket-accept") != accept:
            stop(f"the server refused or mis-answered the WebSocket upgrade ({lines[0]!r}); check that the executable is a codex that supports `app-server --listen`")
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
                stop("the server closed the connection mid-measurement; rerun, and check the server still starts (`codex app-server --help`)")
            data += chunk
        return data

    def message(self):
        data, started = b"", False
        while True:
            first, second = self.exact(2)
            opcode = first & 0x0F
            if first & 0x70 or second & 0x80 or opcode not in (0x0, 0x1, 0x8, 0x9, 0xA):
                stop("the server sent a frame a WebSocket server may not (reserved bits, masking or opcode); rerun, and report it if it recurs")
            n = second & 0x7F
            if n == 126:
                n = struct.unpack(">H", self.exact(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self.exact(8))[0]
            if n > 64 * 1024 * 1024 or (opcode >= 0x8 and (n > 125 or not first & 0x80)):
                stop("the server sent an oversized or fragmented frame; rerun, and report it if it recurs")
            body = self.exact(n)
            if opcode == 0x8:
                stop("the server closed the connection mid-measurement; rerun, and check the server still starts (`codex app-server --help`)")
            if opcode in (0x9, 0xA):
                continue
            if (opcode == 0x1) == started:
                stop("the server interleaved two messages' frames; rerun, and report it if it recurs")
            started = True
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
                if "error" in frame or "result" not in frame:
                    stop(f"{method} was refused: {frame.get('error')}; check `codex login status` and the codex version")
                return frame["result"]

    def ready_thread(self):
        self.request("initialize", {"clientInfo": {"name": "oneharness-measure", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        return self.request("thread/start", {"cwd": work, "approvalPolicy": "never"})["thread"]["id"]


probe = subprocess.run([codex, "--version"], capture_output=True, text=True)
if probe.returncode != 0:
    stop(f"`{codex} --version` failed ({probe.returncode}): {probe.stderr.strip()}; check the executable")
version = probe.stdout.strip()
errors = open(os.path.join(work, "server.err"), "w+")
launched = time.monotonic()
server = subprocess.Popen(
    [codex, "app-server", "-c", "features.plugins=false", "-c", "features.apps=false",
     "-c", "analytics.enabled=false", "--listen", f"unix://{path}"],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=errors, cwd=work,
)
try:
    deadline = time.monotonic() + 60
    first = None
    while first is None:
        try:
            first = Connection()
        except OSError:
            if server.poll() is not None or time.monotonic() > deadline:
                errors.seek(0)
                stop(f"the server never accepted a connection (exit {server.poll()}): {errors.read()[-500:].strip()}; check `codex login status` and that this codex supports `app-server --listen`")
            time.sleep(0.02)
    threads = []
    first.ready_thread()
    threads.append({"thread": 1, "ready_ms": round((time.monotonic() - launched) * 1000), **reading(server.pid)})
    started = time.monotonic()
    second = Connection()
    second.ready_thread()
    threads.append({"thread": 2, "ready_ms": round((time.monotonic() - started) * 1000), **reading(server.pid)})
    print(json.dumps({"codex_version": version, "platform": sys.platform, "server_pid": server.pid,
                      "threads": threads}, separators=(",", ":")))
finally:
    server.terminate()
    try:
        server.wait(10)
    except subprocess.TimeoutExpired:
        server.kill()
        server.wait()
    errors.close()
    for leftover in os.listdir(work):
        try:
            os.remove(os.path.join(work, leftover))
        except OSError:
            pass
    os.rmdir(work)
PY
