#!/usr/bin/env python3
"""Break one launch's network bytes down by destination, rootless, on Linux.

    codex-network-breakdown.py [--label L] [--env K=V]... [--timeout S]
                               [--trace] [--trace-out PATH] [--launch-stdout PATH]
                               -- ARGV...
    codex-network-breakdown.py report MEASUREMENT.json [--trace TRACE.log]

Measure mode runs ARGV (a `codex` launch, or a `oneharness` invocation that
starts one) inside a fresh unprivileged user, network, mount and PID namespace
whose only route out is a `pasta` tap interface, with `HTTPS_PROXY` and
`HTTP_PROXY` pointed at a byte-counting CONNECT proxy that runs inside that
same namespace. It prints one JSON document on stdout:

  * `received_bytes` / `sent_bytes`: the namespace's TAP interface counters
    over the launch's whole process tree (detached children included) — every
    byte the launch exchanged with the outside, as namespace-side frames
    (payload plus a small framing overhead; pasta terminates TCP, so these are
    not host-wire bytes);
  * `tunnels`: one entry per proxied connection, in the order they opened —
    its `destination` (`host:port`), `host`, `port`, `received_bytes` (from
    the destination to the launch) and `sent_bytes` (the other way) as relayed
    payload, `opened_at` / `closed_at` (UNIX seconds), and `paths`: the request
    paths Codex's own trace names for that host while the tunnel was open
    (empty when the trace names none, or no trace was taken);
  * `tunnel_received_bytes` / `tunnel_sent_bytes` and `unaccounted_*`: the
    tunnels' totals, and what the namespace counted beyond them (framing, DNS,
    and anything that went around the proxy).

`--trace` sets `RUST_LOG` (`--rust-log` to change it) for the launch and reads
its stderr as Codex's trace — plus, when the launch is `oneharness run` and its
stdout is the JSON report, the Codex stderr each result carries, since
oneharness captures it; `--trace-out` keeps that trace. Without `--trace` the
launch's stderr passes through. The launch's stdout goes to this script's
stderr, or to `--launch-stdout`. `--measurement-out` keeps the raw record
(counters and tunnels) that report mode reads.

Report mode assembles the same document from a recorded measurement (the
namespace counters and tunnel records measure mode collects) and a trace, so
the attribution can be checked against committed fixtures with no launch.

pasta comes from `CODEX_NETWORK_BREAKDOWN_PASTA`, else PATH, else is fetched
without root (`apt-get download passt` + `dpkg -x`) into
`$XDG_CACHE_HOME/oneharness-network-breakdown`. Exit status: 0 measured
(whatever the launch's own exit), 3 when no measurement could be taken.
"""

import argparse
import ctypes
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime
from pathlib import Path

METHOD = "netns-pasta-ifcounters+connect-proxy"
BOUNDARY = (
    "namespace-side TAP frames (pasta-terminated TCP): payload + namespace framing, "
    "not host wire; tunnels are relayed payload"
)
DEFAULT_RUST_LOG = "info,codex_http_client=debug,reqwest::connect=debug,codex_api=debug"
CLONE_NEWNS = 0x00020000
CLONE_NEWPID = 0x20000000
# A connection Codex's trace says it is starting is matched to the first tunnel
# to that host that opened this soon before (clock skew between the log line
# and the proxy's accept) or after it.
CONNECT_BEFORE_SECONDS = 0.5
CONNECT_AFTER_SECONDS = 5.0
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TIMESTAMP = re.compile(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?Z\s+[A-Z]+\s+")
SPANS = re.compile(r"^(?:[\w.]+\{[^{}]*\}:)+")
CONNECT = re.compile(
    r"(?:starting new connection|connecting to websocket): ((?:https?|wss?)://[^\s,]+)"
)
COMPLETED_URL = re.compile(r"\burl=((?:https?|wss?)://[^\s,]+)")
SPAN_PATH = re.compile(r'api\.path="?([^"\s}]+)')
URL = re.compile(r"^(https?|wss?)://([A-Za-z0-9.-]+)(?::(\d+))?(/[^\s\"'?#]*)?")


# --- the report: pure, shared by both modes --------------------------------


def parse_url(url: str):
    """`(host, port, path)` of an http(s)/ws(s) URL; the port defaults by
    scheme (443 for https/wss, else 80) and the path to `/`."""
    match = URL.match(url)
    if not match:
        return None
    default = 443 if match.group(1) in ("https", "wss") else 80
    port = int(match.group(3)) if match.group(3) else default
    return match.group(2), port, match.group(4) or "/"


def trace_lines(trace_text: str) -> list:
    """Each tracing line as `(seconds, spans, message)`: the UTC timestamp in
    UNIX seconds, the span chain before the target (`name{fields}:` repeated,
    "" when none), and the rest. Colour codes are ignored; other lines drop."""
    lines = []
    for raw in trace_text.splitlines():
        line = ANSI.sub("", raw)
        stamp = TIMESTAMP.match(line)
        if not stamp:
            continue
        whole = datetime.strptime(stamp.group(1), "%Y-%m-%dT%H:%M:%S")
        # The trace is UTC; a naive `timestamp()` would read it as local time.
        seconds = (whole - datetime(1970, 1, 1)).total_seconds()
        if stamp.group(2):
            seconds += float("0" + stamp.group(2))
        rest = line[stamp.end():]
        spans = SPANS.match(rest)
        spans = spans.group(0) if spans else ""
        lines.append((seconds, spans, rest[len(spans):]))
    return lines


def trace_connections(trace_text: str) -> list:
    """Every connection Codex's trace says it starts: `(seconds, host, port,
    path)`, where `path` is the request the trace names for it, or None.

    The path is, in order of preference: the URL a later line in the same
    span chain reports completing (`url=…`, e.g. `Request completed`) — for a
    connection started outside any span, the next such line outside any span,
    unless another span-less connection to that host starts first; the URL
    being connected to, when it has a path beyond `/` (a websocket's
    endpoint); the span's own `api.path`."""
    lines = trace_lines(trace_text)
    connections = []
    for index, (seconds, spans, message) in enumerate(lines):
        connect = CONNECT.search(message)
        target = parse_url(connect.group(1)) if connect else None
        if not target:
            continue
        host, port, url_path = target
        path = None
        for _, later_spans, later in lines[index + 1:]:
            if later_spans != spans:
                continue
            again = CONNECT.search(later)
            if not spans and again and (parse_url(again.group(1)) or ("",))[0] == host:
                break
            completed = COMPLETED_URL.search(later)
            parsed = parse_url(completed.group(1)) if completed else None
            if parsed and parsed[0] == host:
                path = parsed[2]
                break
        if path is None and url_path != "/":
            path = url_path
        named = SPAN_PATH.search(spans)
        if path is None and named:
            path = "/" + named.group(1).lstrip("/")
        connections.append((seconds, host, port, path))
    return connections


def attribute(tunnels: list, connections: list) -> None:
    """Give each tunnel the path of the connection Codex started it for.

    Connections are matched, in the order the trace starts them, to the
    first not-yet-matched tunnel to the same host and port that opened from
    CONNECT_BEFORE_SECONDS before to CONNECT_AFTER_SECONDS after. A tunnel
    whose connection the trace names no path for, or that no connection line
    matches, gets `paths: []`."""
    for tunnel in tunnels:
        tunnel["paths"] = []
    matched = set()
    for seconds, host, port, path in sorted(connections, key=lambda c: c[0]):
        for index, tunnel in enumerate(tunnels):
            if (
                index not in matched
                and tunnel["host"] == host
                and tunnel["port"] == port
                and seconds - CONNECT_BEFORE_SECONDS
                <= tunnel["opened_at"]
                <= seconds + CONNECT_AFTER_SECONDS
            ):
                matched.add(index)
                if path is not None:
                    tunnel["paths"].append(path)
                break


def oneharness_stderr(stdout_text: str) -> str:
    """The Codex stderr a `oneharness run --format json` report carries, one
    result after another; "" when the text is not such a report."""
    try:
        document = json.loads(stdout_text)
    except ValueError:
        return ""
    results = document.get("results") if isinstance(document, dict) else None
    return "\n".join(
        result["stderr"]
        for result in results or []
        if isinstance(result, dict) and isinstance(result.get("stderr"), str)
    )


def report(measurement: dict, trace_text) -> dict:
    """The printed document for one recorded measurement and optional trace."""
    tunnels = [
        {
            "destination": f"{tunnel['host']}:{tunnel['port']}",
            "host": tunnel["host"],
            "port": tunnel["port"],
            "received_bytes": tunnel["received_bytes"],
            "sent_bytes": tunnel["sent_bytes"],
            "opened_at": tunnel["opened_at"],
            "closed_at": tunnel["closed_at"],
        }
        for tunnel in sorted(measurement["tunnels"], key=lambda tunnel: tunnel["opened_at"])
    ]
    attribute(tunnels, trace_connections(trace_text) if trace_text is not None else [])
    received = sum(tunnel["received_bytes"] for tunnel in tunnels)
    sent = sum(tunnel["sent_bytes"] for tunnel in tunnels)
    return {
        "label": measurement.get("label", ""),
        "method": METHOD,
        "boundary": BOUNDARY,
        "argv": measurement.get("argv", []),
        "launch_exit": measurement.get("launch_exit"),
        "launch_seconds": measurement.get("launch_seconds"),
        "traced": trace_text is not None,
        "received_bytes": measurement["received_bytes"],
        "sent_bytes": measurement["sent_bytes"],
        "tunnel_received_bytes": received,
        "tunnel_sent_bytes": sent,
        "unaccounted_received_bytes": measurement["received_bytes"] - received,
        "unaccounted_sent_bytes": measurement["sent_bytes"] - sent,
        "tunnels": tunnels,
    }


# --- the counting proxy, run inside the namespace ---------------------------


class CountingProxy:
    """An HTTP CONNECT (and absolute-URI plain HTTP) proxy that records, per
    connection, the destination, the payload relayed each way, and when it
    opened and closed."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.tunnels = []
        self.listener = socket.socket()
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(256)
        self.port = self.listener.getsockname()[1]
        threading.Thread(target=self._accept, daemon=True).start()

    def _accept(self) -> None:
        while True:
            try:
                client, _ = self.listener.accept()
            except OSError:
                return
            threading.Thread(target=self._handle, args=(client,), daemon=True).start()

    def _handle(self, client: socket.socket) -> None:
        head = b""
        while b"\r\n\r\n" not in head:
            data = client.recv(4096)
            if not data:
                client.close()
                return
            head += data
        method, target, _ = head.split(b"\r\n", 1)[0].decode("latin-1").split(" ", 2)
        rest = head.split(b"\r\n\r\n", 1)[1]
        if method == "CONNECT":
            host, port = target.rsplit(":", 1)
        else:
            hostport = target.split("/", 3)[2]
            host, _, port = hostport.partition(":")
            port = port or "80"
        tunnel = {
            "host": host.strip("[]"),
            "port": int(port),
            "received_bytes": 0,
            "sent_bytes": 0,
            "opened_at": time.time(),
            "closed_at": None,
        }
        try:
            upstream = socket.create_connection((tunnel["host"], tunnel["port"]), timeout=30)
            upstream.settimeout(None)
        except OSError:
            client.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            client.close()
            return
        with self.lock:
            self.tunnels.append(tunnel)
        if method == "CONNECT":
            client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            if rest:
                upstream.sendall(rest)
                self._count(tunnel, "sent_bytes", len(rest))
        else:
            upstream.sendall(head)
            self._count(tunnel, "sent_bytes", len(head))
        down = threading.Thread(
            target=self._pump, args=(upstream, client, tunnel, "received_bytes"), daemon=True
        )
        down.start()
        self._pump(client, upstream, tunnel, "sent_bytes")
        down.join()
        with self.lock:
            tunnel["closed_at"] = time.time()
        client.close()
        upstream.close()

    def _count(self, tunnel: dict, field: str, n: int) -> None:
        with self.lock:
            tunnel[field] += n

    def _pump(self, src: socket.socket, dst: socket.socket, tunnel: dict, field: str) -> None:
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
                self._count(tunnel, field, len(data))
        except OSError:
            pass
        finally:
            try:
                dst.shutdown(socket.SHUT_WR)
            except OSError:
                pass

    def snapshot(self) -> list:
        """Every tunnel so far; one still open is closed at this moment."""
        now = time.time()
        with self.lock:
            return [
                dict(tunnel, closed_at=tunnel["closed_at"] or now) for tunnel in self.tunnels
            ]


# --- measure mode -----------------------------------------------------------


def read_counters() -> dict:
    """Every non-loopback interface's byte counters in this namespace."""
    counters = {}
    for line in Path("/proc/net/dev").read_text().splitlines()[2:]:
        name, _, rest = line.partition(":")
        if name.strip() == "lo":
            continue
        fields = [int(x) for x in rest.split()]
        counters[name.strip()] = (fields[0], fields[8])
    return counters


def counter_delta(start: dict, end: dict) -> tuple:
    received = sum(rx - start.get(name, (0, 0))[0] for name, (rx, _) in end.items())
    sent = sum(tx - start.get(name, (0, 0))[1] for name, (_, tx) in end.items())
    return received, sent


def private_resolver() -> None:
    """Give the mount namespace a resolver reachable from inside it.

    A host resolving through systemd-resolved's 127.0.0.53 stub has, inside the
    namespace, only an empty loopback there; the upstream list is the same
    configuration minus the stub."""
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    current = Path("/etc/resolv.conf").read_text()
    servers = [line.split()[1] for line in current.splitlines() if line.startswith("nameserver")]
    upstream = Path("/run/systemd/resolve/resolv.conf")
    if servers and all(server.startswith("127.") for server in servers) and upstream.exists():
        subprocess.run(["mount", "--bind", str(upstream), "/etc/resolv.conf"], check=True)


def init(args: argparse.Namespace) -> int:
    """Inside pasta's user and network namespace: become PID 1 of a new PID
    namespace, run the proxy and the launch, and record the measurement."""
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.unshare(CLONE_NEWNS | CLONE_NEWPID) != 0:
        raise OSError(ctypes.get_errno(), "unshare(CLONE_NEWNS|CLONE_NEWPID)")
    child = os.fork()
    if child:
        _, status = os.waitpid(child, 0)
        return os.waitstatus_to_exitcode(status)

    private_resolver()
    subprocess.run(["mount", "-t", "proc", "proc", "/proc"], check=False)
    subprocess.run(["ip", "link", "set", "lo", "up"], check=False, capture_output=True)
    signal.signal(signal.SIGTERM, lambda *_: os.kill(-1, signal.SIGKILL))
    proxy = CountingProxy()
    address = f"http://127.0.0.1:{proxy.port}"
    env = dict(os.environ)
    for name in ("HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"):
        env[name] = address
    for name in ("NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy"):
        env.pop(name, None)
    if args.trace:
        env["RUST_LOG"] = args.rust_log
    stdout = open(args.launch_stdout, "wb") if args.launch_stdout else sys.stderr.buffer
    stderr = open(args.trace_file, "wb") if args.trace else None
    start = read_counters()
    t0 = time.monotonic()
    launch = subprocess.Popen(args.argv, stdout=stdout, stderr=stderr, env=env)
    launch_exit = None
    launch_seconds = None
    deadline = t0 + args.timeout if args.timeout > 0 else None
    timed_out = False
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break  # nothing left in the namespace but this init and its threads
        if pid == launch.pid:
            launch_exit = os.waitstatus_to_exitcode(status)
            launch_seconds = time.monotonic() - t0
            launch.returncode = launch_exit
        if pid == 0:
            now = time.monotonic()
            if deadline and now > deadline and not timed_out:
                timed_out = True
                os.kill(-1, signal.SIGTERM)
                deadline = now + 5
            elif timed_out and now > deadline:
                os.kill(-1, signal.SIGKILL)
                deadline = now + 3600
            time.sleep(0.02)
    # Let the proxy see the last FINs before the counters are read.
    time.sleep(0.2)
    received, sent = counter_delta(start, read_counters())
    measurement = {
        "label": args.label,
        "argv": args.argv,
        "launch_exit": launch_exit,
        "launch_seconds": round(launch_seconds or time.monotonic() - t0, 3),
        "timed_out": timed_out,
        "received_bytes": received,
        "sent_bytes": sent,
        "tunnels": proxy.snapshot(),
    }
    Path(args.result).write_text(json.dumps(measurement))
    return 0


def find_pasta() -> str:
    explicit = os.environ.get("CODEX_NETWORK_BREAKDOWN_PASTA")
    if explicit:
        return explicit
    on_path = shutil.which("pasta")
    if on_path:
        return on_path
    cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    tools = cache / "oneharness-network-breakdown"
    pasta = tools / "root" / "usr" / "bin" / "pasta"
    if not pasta.exists():
        debs = tools / "debs"
        debs.mkdir(parents=True, exist_ok=True)
        subprocess.run(["apt-get", "download", "passt"], cwd=debs, check=True, capture_output=True)
        for deb in debs.glob("*.deb"):
            subprocess.run(["dpkg", "-x", str(deb), str(tools / "root")], check=True)
    return str(pasta)


def measure(args: argparse.Namespace) -> int:
    env = dict(os.environ)
    for pair in args.env:
        key, sep, value = pair.partition("=")
        if not sep:
            sys.exit(f"codex-network-breakdown: --env wants K=V, got {pair!r}")
        env[key] = value
    try:
        pasta = find_pasta()
    except (OSError, subprocess.CalledProcessError) as error:
        print(json.dumps({"label": args.label, "method": METHOD,
                          "error": f"no pasta to build the namespace with: {error}"}))
        return 3
    scratch = tempfile.mkdtemp(prefix="codex-network-breakdown-")
    try:
        result = os.path.join(scratch, "measurement.json")
        trace_file = args.trace_out or os.path.join(scratch, "trace.log")
        inner = [sys.executable, "-I", str(Path(__file__).resolve()), "--_init", result,
                 "--label", args.label, "--timeout", str(args.timeout)]
        if args.trace:
            inner += ["--trace", "--rust-log", args.rust_log, "--_trace-file",
                      os.path.abspath(trace_file)]
        launch_stdout = args.launch_stdout or (
            os.path.join(scratch, "launch.stdout") if args.trace else None
        )
        if launch_stdout:
            inner += ["--launch-stdout", os.path.abspath(launch_stdout)]
        command = [pasta, "--config-net", "--quiet", "-4",
                   "-t", "none", "-u", "none", "-T", "none", "-U", "none",
                   "--", *inner, "--", *args.argv]
        run = subprocess.Popen(command, env=env, start_new_session=True, stdout=sys.stderr)
        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(signum, lambda *_: os.killpg(run.pid, signal.SIGTERM))
        run.wait()
        if not os.path.exists(result):
            print(json.dumps({"label": args.label, "method": METHOD,
                              "error": f"no measurement (pasta exited {run.returncode})"}))
            return 3
        measurement = json.loads(Path(result).read_text())
        if args.measurement_out:
            Path(args.measurement_out).write_text(json.dumps(measurement, indent=2) + "\n")
        trace_text = None
        if args.trace:
            stdout_text = Path(launch_stdout).read_text(errors="replace")
            if not args.launch_stdout:
                sys.stderr.write(stdout_text)
            trace_text = Path(trace_file).read_text(errors="replace")
            carried = oneharness_stderr(stdout_text)
            if carried:
                trace_text += "\n" + carried
                Path(trace_file).write_text(trace_text)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    print(json.dumps(report(measurement, trace_text), indent=2))
    return 0


def main(argv: list) -> int:
    if argv[:1] == ["report"]:
        parser = argparse.ArgumentParser(prog="codex-network-breakdown.py report")
        parser.add_argument("measurement")
        parser.add_argument("--trace")
        args = parser.parse_args(argv[1:])
        measurement = json.loads(Path(args.measurement).read_text())
        trace_text = Path(args.trace).read_text(errors="replace") if args.trace else None
        print(json.dumps(report(measurement, trace_text), indent=2))
        return 0
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--label", default="")
    parser.add_argument("--env", action="append", default=[], metavar="K=V")
    parser.add_argument("--timeout", type=float, default=0,
                        help="after S seconds SIGTERM every process left, SIGKILL 5 s later")
    parser.add_argument("--trace", action="store_true",
                        help="set RUST_LOG for the launch and attribute request paths from it")
    parser.add_argument("--rust-log", default=DEFAULT_RUST_LOG)
    parser.add_argument("--trace-out", help="keep the launch's trace here")
    parser.add_argument("--launch-stdout")
    parser.add_argument("--measurement-out", help="keep the raw measurement report mode reads")
    parser.add_argument("--_init", dest="result", help=argparse.SUPPRESS)
    parser.add_argument("--_trace-file", dest="trace_file", help=argparse.SUPPRESS)
    parser.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.argv[:1] == ["--"]:
        args.argv = args.argv[1:]
    if not args.argv:
        parser.error("give the launch's argv after --")
    if args.result:
        return init(args)
    return measure(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
