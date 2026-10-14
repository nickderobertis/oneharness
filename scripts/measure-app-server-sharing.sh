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
import base64, hashlib, json, os, shutil, socket, struct, subprocess, sys, tempfile, time

codex = sys.argv[1]
# Every socket operation's bound: a server that stops answering fails the
# measurement naming the step instead of hanging it.
SOCKET_TIMEOUT = 120
# The most a WebSocket upgrade answer may be before it is refused.
HANDSHAKE_LIMIT = 16 * 1024
RERUN = "rerun, and check the server still starts (`codex app-server --help`)"


def tree(root):
    """`root` and every descendant of it, from /proc. A process that exits
    between the listing and its read is skipped; any other read failure
    could hide a live descendant, so it stops the measurement."""
    try:
        names = os.listdir("/proc")
    except OSError as error:
        stop(f"cannot list /proc: {error}; run it on Linux with /proc mounted and readable")
    parents = {}
    for name in names:
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat") as stat:
                parents[int(name)] = int(stat.read().rsplit(")", 1)[1].split()[1])
        except (FileNotFoundError, ProcessLookupError):
            continue
        except OSError as error:
            stop(f"cannot read /proc/{name}/stat: {error}; run it as the user that owns the server")
    found, index = [root], 0
    while index < len(found):
        found += [pid for pid, ppid in parents.items() if ppid == found[index]]
        index += 1
    return sorted(found)


def rss(pid):
    """The resident bytes of `pid`: 0 once it has exited (or for a zombie, which
    reports no VmRSS and holds none); any other read failure would undercount
    the server, so it stops the measurement."""
    try:
        with open(f"/proc/{pid}/status") as status:
            for line in status:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) * 1024
    except (FileNotFoundError, ProcessLookupError):
        return 0
    except OSError as error:
        stop(f"cannot read /proc/{pid}/status of a process in the server's tree: {error}; run it as the user that owns the server")
    return 0


def reading(pid):
    pids = tree(pid)
    return {"rss_bytes": sum(rss(p) for p in pids), "pids": pids}


def stop(why):
    raise SystemExit(f"measure-app-server-sharing: {why}")


GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


class Connection:
    """One WebSocket connection to the server at `path`. Constructing it raises
    OSError only while the socket cannot be connected (the caller retries);
    every later failure stops the measurement naming its step."""

    def __init__(self, path):
        target = os.readlink(path) if os.path.islink(path) else path
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(SOCKET_TIMEOUT)
        try:
            self.sock.connect(target)
        except OSError:
            self.sock.close()
            raise
        try:
            self.handshake()
        except OSError as error:
            stop(f"the WebSocket upgrade failed: {error}; {RERUN}")
        self.next_id = 1

    def handshake(self):
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            f"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode()
        )
        head = b""
        while not head.endswith(b"\r\n\r\n"):
            if len(head) > HANDSHAKE_LIMIT:
                stop(f"the server's WebSocket upgrade answer passed {HANDSHAKE_LIMIT} bytes; check that the executable is a codex that supports `app-server --listen`")
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

    def send(self, value):
        payload = json.dumps(value).encode()
        mask = os.urandom(4)
        n = len(payload)
        header = bytes([0x81]) + (
            bytes([0x80 | n]) if n < 126
            else bytes([0x80 | 126]) + struct.pack(">H", n) if n < 65536
            else bytes([0x80 | 127]) + struct.pack(">Q", n)
        )
        try:
            self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))
        except OSError as error:
            stop(f"sending {value.get('method', 'a frame')} to the server failed: {error}; {RERUN}")

    def exact(self, n):
        data = b""
        while len(data) < n:
            try:
                chunk = self.sock.recv(n - len(data))
            except OSError as error:
                stop(f"reading from the server failed (no answer within {SOCKET_TIMEOUT} s, or the connection broke): {error}; {RERUN}")
            if not chunk:
                stop(f"the server closed the connection mid-measurement; {RERUN}")
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
                stop(f"the server closed the connection mid-measurement; {RERUN}")
            if opcode in (0x9, 0xA):
                continue
            if (opcode == 0x1) == started:
                stop("the server interleaved two messages' frames; rerun, and report it if it recurs")
            started = True
            data += body
            if first & 0x80:
                try:
                    return json.loads(data)
                except ValueError as error:
                    stop(f"the server sent a message that is not JSON ({error}); rerun, and report it if it recurs")

    def request(self, method, params):
        ident = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        while True:
            frame = self.message()
            if isinstance(frame, dict) and frame.get("id") == ident and "method" not in frame:
                if "error" in frame or not isinstance(frame.get("result"), dict):
                    stop(f"{method} was refused: {frame.get('error')}; check `codex login status` and the codex version")
                return frame["result"]

    def ready_thread(self, cwd):
        self.request("initialize", {"clientInfo": {"name": "oneharness-measure", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        thread = self.request("thread/start", {"cwd": cwd, "approvalPolicy": "never"}).get("thread")
        if not isinstance(thread, dict) or not isinstance(thread.get("id"), str):
            stop(f"thread/start named no thread: {thread!r}; check the codex version")
        return thread["id"]


def measure(work, errors):
    path = os.path.join(work, "app-server.sock")
    launched = time.monotonic()
    try:
        server = subprocess.Popen(
            [codex, "app-server", "-c", "features.plugins=false", "-c", "features.apps=false",
             "-c", "analytics.enabled=false", "--listen", f"unix://{path}"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=errors, cwd=work,
        )
    except OSError as error:
        stop(f"cannot start `{codex} app-server`: {error}; check the executable")
    try:
        deadline = time.monotonic() + 60
        first = None
        while first is None:
            try:
                first = Connection(path)
            except OSError as error:
                if server.poll() is not None or time.monotonic() > deadline:
                    errors.seek(0)
                    said = errors.read().strip() or "(nothing on stderr)"
                    stop(f"the server never accepted a connection on {path} (last attempt: {error}; exit {server.poll()}); its stderr:\n{said}\ncheck `codex login status` and that this codex supports `app-server --listen`")
                time.sleep(0.02)
        threads = []
        first.ready_thread(work)
        threads.append({"thread": 1, "ready_ms": round((time.monotonic() - launched) * 1000), **reading(server.pid)})
        started = time.monotonic()
        try:
            second = Connection(path)
        except OSError as error:
            stop(f"the server accepted no second connection: {error}; {RERUN}")
        second.ready_thread(work)
        threads.append({"thread": 2, "ready_ms": round((time.monotonic() - started) * 1000), **reading(server.pid)})
        return server.pid, threads
    finally:
        server.terminate()
        try:
            server.wait(10)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()


def main():
    try:
        probe = subprocess.run([codex, "--version"], capture_output=True, text=True)
    except OSError as error:
        stop(f"cannot run `{codex} --version`: {error}; check the executable")
    if probe.returncode != 0:
        stop(f"`{codex} --version` failed ({probe.returncode}): {probe.stderr.strip()}; check the executable")
    try:
        work = tempfile.mkdtemp(prefix="oh-measure-")
    except OSError as error:
        stop(f"cannot create a scratch directory under {tempfile.gettempdir()}: {error}; check that the host temp dir is writable")
    try:
        try:
            errors = open(os.path.join(work, "server.err"), "w+")
        except OSError as error:
            stop(f"cannot create the server's stderr file in {work}: {error}; check that the host temp dir is writable")
        with errors:
            pid, threads = measure(work, errors)
    finally:
        failed = []
        shutil.rmtree(work, onerror=lambda _f, p, info: failed.append(f"{p}: {info[1]}"))
        if failed:
            print(f"measure-app-server-sharing: could not remove its scratch directory {work} ({'; '.join(failed)}); delete it by hand", file=sys.stderr)
    print(json.dumps({"codex_version": probe.stdout.strip(), "platform": sys.platform, "server_pid": pid,
                      "threads": threads}, separators=(",", ":")))


main()
PY
