#!/usr/bin/env python3
"""Break one launch's network bytes down by destination, rootless, on Linux.

    codex-network-breakdown.py [--label L] [--env K=V]... [--timeout S]
                               [--trace] [--rust-log FILTER] [--trace-out PATH]
                               [--launch-stdout PATH] [--measurement-out PATH]
                               -- ARGV...
    codex-network-breakdown.py report MEASUREMENT.json [--trace TRACE.log]

Measure mode runs ARGV (a `codex` launch, or a `oneharness` invocation that
starts one) inside a fresh unprivileged user, network, mount and PID namespace
whose only route out is a `pasta` tap interface, with `HTTPS_PROXY` and
`HTTP_PROXY` pointed at a byte-counting CONNECT proxy that runs inside that
same namespace. It prints one JSON document, on one line, on stdout:

  * `received_bytes` / `sent_bytes`: the namespace's TAP interface counters
    over the launch's whole process tree (detached children included) — every
    byte the launch exchanged with the outside, as namespace-side frames
    (payload plus a small framing overhead; pasta terminates TCP, so these are
    not host-wire bytes);
  * `tunnels`: one entry per proxied connection, in the order they opened —
    its `destination` (`host:port`), `host`, `port`, `received_bytes` (from
    the destination to the launch) and `sent_bytes` (the other way) as relayed
    payload, `opened_at` / `closed_at` (UNIX seconds), `error` (why the relay
    ended abnormally, else null), and `paths`: the request path Codex's own
    trace names for the connection it opened that tunnel for (empty when the
    trace names none, or no trace was taken);
  * `refused`: each connection the proxy could not open, with the error;
  * `tunnel_received_bytes` / `tunnel_sent_bytes` and `unaccounted_*`: the
    tunnels' totals, and what the namespace counted beyond them (framing, DNS,
    and anything that went around the proxy); `timed_out`, when `--timeout`
    ended the launch.

`--trace` sets `RUST_LOG` (`--rust-log` to change it) for the launch and reads
its stderr as Codex's trace — plus, when the launch is `oneharness run` and its
stdout is the JSON report, the Codex stderr each result carries, since
oneharness captures it; `--trace-out` keeps that trace. Without `--trace` the
launch's stderr passes through. The launch's stdout goes to this script's
stderr, or to `--launch-stdout`. `--measurement-out` keeps the raw record
(counters and tunnels) that report mode reads.

Report mode assembles the same document from a recorded measurement and a
trace, so the attribution can be checked against committed fixtures with no
launch.

pasta comes from `CODEX_NETWORK_BREAKDOWN_PASTA`, else PATH, else a copy this
script fetched before into `$XDG_CACHE_HOME/oneharness-network-breakdown`,
else is fetched there without root (`apt-get download passt` + `dpkg -x`) —
unless `CODEX_NETWORK_BREAKDOWN_NO_FETCH=1`. Exit status: 0 measured (whatever
the launch's own exit), 2 a usage error, 3 when no measurement could be taken;
every failure is reported on stderr with what to do next.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import math
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
from dataclasses import asdict, dataclass, field
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
ENV_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class Unusable(Exception):
    """An input or host this script cannot use, said with what to do about it."""


@dataclass(frozen=True)
class Url:
    host: str
    port: int
    path: str


@dataclass(frozen=True)
class TraceLine:
    """One tracing line: UTC UNIX seconds, the span chain before the target
    (`name{fields}:` repeated, "" when none), and the rest."""

    seconds: float
    spans: str
    message: str


@dataclass(frozen=True)
class Connection:
    """A connection Codex's trace says it starts, and the request path the
    trace names for it, if any."""

    seconds: float
    host: str
    port: int
    path: str | None


@dataclass
class Tunnel:
    host: str
    port: int
    opened_at: float
    received_bytes: int = 0
    sent_bytes: int = 0
    closed_at: float | None = None
    error: str | None = None


@dataclass(frozen=True)
class Refusal:
    destination: str
    at: float
    error: str


def field_of(record: object, key: str, kinds: tuple, where: str) -> object:
    """`record[key]`, refused unless `record` is an object carrying it as one
    of `kinds` (a bool is never taken for a number)."""
    if not isinstance(record, dict) or key not in record:
        raise Unusable(f"{where} has no `{key}`")
    value = record[key]
    if (isinstance(value, bool) and bool not in kinds) or not isinstance(value, kinds):
        raise Unusable(f"{where}'s `{key}` is {value!r}, not {kinds[0].__name__}")
    return value


@dataclass
class Measurement:
    """The raw record measure mode collects and report mode reads."""

    label: str
    argv: list[str]
    launch_exit: int | None
    launch_seconds: float | None
    timed_out: bool
    received_bytes: int
    sent_bytes: int
    tunnels: list[Tunnel]
    refused: list[Refusal] = field(default_factory=list)

    @staticmethod
    def from_json(data: object, source: str) -> Measurement:
        """A recorded measurement, refused at the first field that is wrong."""
        number = (int, float)
        tunnels = []
        for index, raw in enumerate(field_of(data, "tunnels", (list,), source)):
            where = f"{source}: tunnel {index}"
            tunnel = Tunnel(
                host=field_of(raw, "host", (str,), where),
                port=field_of(raw, "port", (int,), where),
                opened_at=field_of(raw, "opened_at", number, where),
                received_bytes=field_of(raw, "received_bytes", (int,), where),
                sent_bytes=field_of(raw, "sent_bytes", (int,), where),
                closed_at=field_of(raw, "closed_at", number, where),
                error=raw.get("error") if isinstance(raw.get("error"), str) else None,
            )
            if not 0 < tunnel.port < 65536 or min(tunnel.received_bytes, tunnel.sent_bytes) < 0:
                raise Unusable(f"{where} has an impossible port or byte count")
            tunnels.append(tunnel)
        refused = []
        for index, raw in enumerate(data.get("refused", [])):
            where = f"{source}: refusal {index}"
            refused.append(
                Refusal(
                    destination=field_of(raw, "destination", (str,), where),
                    at=field_of(raw, "at", number, where),
                    error=field_of(raw, "error", (str,), where),
                )
            )
        exit_code = data.get("launch_exit")
        seconds = data.get("launch_seconds")
        return Measurement(
            label=str(data.get("label", "")),
            argv=[str(arg) for arg in data.get("argv", [])],
            launch_exit=exit_code if isinstance(exit_code, int) else None,
            launch_seconds=seconds if isinstance(seconds, number) else None,
            timed_out=data.get("timed_out") is True,
            received_bytes=field_of(data, "received_bytes", (int,), source),
            sent_bytes=field_of(data, "sent_bytes", (int,), source),
            tunnels=tunnels,
            refused=refused,
        )


def parse_url(url: str) -> Url | None:
    """An http(s)/ws(s) URL's host, port (443 for https/wss, else 80 when
    unstated) and path (`/` when unstated)."""
    match = URL.match(url)
    if not match:
        return None
    default = 443 if match.group(1) in ("https", "wss") else 80
    port = int(match.group(3)) if match.group(3) else default
    return Url(match.group(2), port, match.group(4) or "/")


def trace_lines(trace_text: str) -> list[TraceLine]:
    """Each tracing line of `trace_text`; colour codes are ignored and lines
    that do not start with tracing's RFC 3339 timestamp are dropped."""
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
        spans_text = spans.group(0) if spans else ""
        lines.append(TraceLine(seconds, spans_text, rest[len(spans_text):]))
    return lines


def trace_connections(trace_text: str) -> list[Connection]:
    """Every connection Codex's trace says it starts.

    Its path is, in order of preference: the URL a later line in the same span
    chain reports completing (`url=…`, e.g. `Request completed`) — for a
    connection started outside any span, the next such line outside any span,
    unless another span-less connection to that host starts first; the URL
    being connected to, when it has a path beyond `/` (a websocket's
    endpoint); the span's own `api.path`."""
    lines = trace_lines(trace_text)
    connections = []
    for index, line in enumerate(lines):
        connect = CONNECT.search(line.message)
        target = parse_url(connect.group(1)) if connect else None
        if target is None:
            continue
        path = None
        for later in lines[index + 1:]:
            if later.spans != line.spans:
                continue
            again = CONNECT.search(later.message)
            again_url = parse_url(again.group(1)) if again else None
            if not line.spans and again_url is not None and again_url.host == target.host:
                break
            completed = COMPLETED_URL.search(later.message)
            completed_url = parse_url(completed.group(1)) if completed else None
            if completed_url is not None and completed_url.host == target.host:
                path = completed_url.path
                break
        if path is None and target.path != "/":
            path = target.path
        named = SPAN_PATH.search(line.spans)
        if path is None and named:
            path = "/" + named.group(1).lstrip("/")
        connections.append(Connection(line.seconds, target.host, target.port, path))
    return connections


def attribute(tunnels: list[Tunnel], connections: list[Connection]) -> list[list[str]]:
    """Each tunnel's paths: those of the connections Codex started it for.

    Connections are matched, in the order the trace starts them, to the first
    not-yet-matched tunnel to the same host and port that opened from
    CONNECT_BEFORE_SECONDS before to CONNECT_AFTER_SECONDS after. A tunnel
    whose connection the trace names no path for, or that no connection line
    matches, gets no paths."""
    paths: list[list[str]] = [[] for _ in tunnels]
    matched: set[int] = set()
    for connection in sorted(connections, key=lambda c: c.seconds):
        for index, tunnel in enumerate(tunnels):
            if (
                index not in matched
                and tunnel.host == connection.host
                and tunnel.port == connection.port
                and connection.seconds - CONNECT_BEFORE_SECONDS
                <= tunnel.opened_at
                <= connection.seconds + CONNECT_AFTER_SECONDS
            ):
                matched.add(index)
                if connection.path is not None:
                    paths[index].append(connection.path)
                break
    return paths


def oneharness_stderr(stdout_text: str) -> str:
    """The Codex stderr a `oneharness run --format json` report carries, one
    result after another; "" when the text is not such a report."""
    try:
        document = json.loads(stdout_text)
    except ValueError:
        return ""
    results = document.get("results") if isinstance(document, dict) else None
    if not isinstance(results, list):
        return ""
    return "\n".join(
        result["stderr"]
        for result in results
        if isinstance(result, dict) and isinstance(result.get("stderr"), str)
    )


def report(measurement: Measurement, trace_text: str | None) -> dict:
    """The printed document for one recorded measurement and optional trace."""
    tunnels = sorted(measurement.tunnels, key=lambda tunnel: tunnel.opened_at)
    paths = attribute(tunnels, trace_connections(trace_text) if trace_text is not None else [])
    received = sum(tunnel.received_bytes for tunnel in tunnels)
    sent = sum(tunnel.sent_bytes for tunnel in tunnels)
    return {
        "label": measurement.label,
        "method": METHOD,
        "boundary": BOUNDARY,
        "argv": measurement.argv,
        "launch_exit": measurement.launch_exit,
        "launch_seconds": measurement.launch_seconds,
        "timed_out": measurement.timed_out,
        "traced": trace_text is not None,
        "received_bytes": measurement.received_bytes,
        "sent_bytes": measurement.sent_bytes,
        "tunnel_received_bytes": received,
        "tunnel_sent_bytes": sent,
        "unaccounted_received_bytes": measurement.received_bytes - received,
        "unaccounted_sent_bytes": measurement.sent_bytes - sent,
        "tunnels": [
            {
                "destination": f"{tunnel.host}:{tunnel.port}",
                "host": tunnel.host,
                "port": tunnel.port,
                "received_bytes": tunnel.received_bytes,
                "sent_bytes": tunnel.sent_bytes,
                "opened_at": tunnel.opened_at,
                "closed_at": tunnel.closed_at,
                "error": tunnel.error,
                "paths": tunnel_paths,
            }
            for tunnel, tunnel_paths in zip(tunnels, paths)
        ],
        "refused": [asdict(refusal) for refusal in measurement.refused],
    }


# llmlint: ignore-block[async_typed_clients_at_boundaries] This is a measuring relay, not a client of any service's API: it copies opaque TLS bytes between the launch and whatever it asked for and counts them, so there is no request or response shape for a typed client to model, and one OS thread per direction of a handful of tunnels is the simplest relay that cannot reorder or buffer what it counts.
class CountingProxy:
    """An HTTP CONNECT (and absolute-URI plain HTTP) proxy that records, per
    connection, the destination, the payload relayed each way, when it opened
    and closed, and why it ended if not cleanly; and every connection it could
    not open, with the error."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.tunnels: list[Tunnel] = []
        self.refused: list[Refusal] = []
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

    def _refuse(self, client: socket.socket, status: str, destination: str, error: str) -> None:
        with self.lock:
            self.refused.append(Refusal(destination, time.time(), error))
        try:
            client.sendall(f"HTTP/1.1 {status}\r\n\r\n{error}\n".encode())
        except OSError:
            pass
        client.close()

    def _handle(self, client: socket.socket) -> None:
        head = b""
        while b"\r\n\r\n" not in head:
            data = client.recv(4096)
            if not data or len(head) > 65536:
                client.close()
                return
            head += data
        request = head.split(b"\r\n", 1)[0].decode("latin-1")
        parts = request.split(" ")
        if len(parts) != 3:
            self._refuse(client, "400 Bad Request", "?", f"malformed request line {request!r}")
            return
        method, target, _ = parts
        if method == "CONNECT":
            host, _, port = target.rpartition(":")
        elif target.startswith("http://"):
            hostport = target[len("http://"):].split("/", 1)[0]
            host, _, port = hostport.partition(":")
            port = port or "80"
        else:
            self._refuse(client, "400 Bad Request", target, f"not a proxy request: {request!r}")
            return
        host = host.strip("[]")
        if not host or not port.isdigit() or not 0 < int(port) < 65536:
            self._refuse(client, "400 Bad Request", target, f"no host and port in {target!r}")
            return
        destination = f"{host}:{port}"
        try:
            upstream = socket.create_connection((host, int(port)), timeout=30)
            upstream.settimeout(None)
        except OSError as error:
            self._refuse(client, "502 Bad Gateway", destination, f"{type(error).__name__}: {error}")
            return
        tunnel = Tunnel(host=host, port=int(port), opened_at=time.time())
        with self.lock:
            self.tunnels.append(tunnel)
        rest = head.split(b"\r\n\r\n", 1)[1]
        if method == "CONNECT":
            client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            forwarded = rest
        else:
            forwarded = head
        if forwarded:
            upstream.sendall(forwarded)
            self._count(tunnel, "sent_bytes", len(forwarded))
        down = threading.Thread(
            target=self._pump, args=(upstream, client, tunnel, "received_bytes"), daemon=True
        )
        down.start()
        self._pump(client, upstream, tunnel, "sent_bytes")
        down.join()
        with self.lock:
            tunnel.closed_at = time.time()
        client.close()
        upstream.close()

    def _count(self, tunnel: Tunnel, direction: str, n: int) -> None:
        with self.lock:
            setattr(tunnel, direction, getattr(tunnel, direction) + n)

    def _pump(self, src: socket.socket, dst: socket.socket, tunnel: Tunnel, direction: str) -> None:
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
                self._count(tunnel, direction, len(data))
        except OSError as error:
            # A reset is how many TLS peers end a connection; it is recorded
            # rather than swallowed, so a relay that broke is visible.
            with self.lock:
                tunnel.error = tunnel.error or f"{direction}: {type(error).__name__}: {error}"
        finally:
            try:
                dst.shutdown(socket.SHUT_WR)
            except OSError:
                pass

    def snapshot(self) -> tuple[list[Tunnel], list[Refusal]]:
        """Every tunnel and refusal so far; a tunnel still open is closed at
        this moment."""
        now = time.time()
        with self.lock:
            tunnels = [
                Tunnel(**{**asdict(tunnel), "closed_at": tunnel.closed_at or now})
                for tunnel in self.tunnels
            ]
            return tunnels, list(self.refused)
# llmlint: ignore-end[async_typed_clients_at_boundaries]


@dataclass(frozen=True)
class Counters:
    received: int
    sent: int


def read_counters() -> dict[str, Counters]:
    """Every non-loopback interface's byte counters in this namespace."""
    counters = {}
    for line in Path("/proc/net/dev").read_text().splitlines()[2:]:
        name, _, rest = line.partition(":")
        if name.strip() == "lo":
            continue
        fields = [int(x) for x in rest.split()]
        counters[name.strip()] = Counters(received=fields[0], sent=fields[8])
    return counters


def counter_delta(start: dict[str, Counters], end: dict[str, Counters]) -> Counters:
    zero = Counters(0, 0)
    return Counters(
        received=sum(after.received - start.get(name, zero).received for name, after in end.items()),
        sent=sum(after.sent - start.get(name, zero).sent for name, after in end.items()),
    )


def run_or_explain(argv: list[str], what: str) -> None:
    """Run a namespace setup step; a failure stops the measurement with the
    step's own error, since a half-built namespace would measure something
    else."""
    try:
        done = subprocess.run(argv, capture_output=True, text=True)
    except OSError as error:
        raise Unusable(f"{what} failed: `{argv[0]}` could not run ({error})") from None
    if done.returncode != 0:
        raise Unusable(
            f"{what} failed (`{' '.join(argv)}` exited {done.returncode}: "
            f"{done.stderr.strip() or 'no output'}); the namespace cannot be set up on this host"
        )


def private_resolver() -> None:
    """Give the mount namespace a resolver reachable from inside it.

    A host resolving through systemd-resolved's 127.0.0.53 stub has, inside the
    namespace, only an empty loopback there; the upstream list is the same
    configuration minus the stub."""
    run_or_explain(["mount", "--make-rprivate", "/"], "making the mount namespace private")
    current = Path("/etc/resolv.conf").read_text()
    servers = [line.split()[1] for line in current.splitlines() if line.startswith("nameserver")]
    upstream = Path("/run/systemd/resolve/resolv.conf")
    if servers and all(server.startswith("127.") for server in servers) and upstream.exists():
        run_or_explain(
            ["mount", "--bind", str(upstream), "/etc/resolv.conf"],
            "pointing the namespace at a resolver",
        )


def loopback_up() -> None:
    """The proxy listens on the namespace's loopback, so it must be up."""
    flags = Path("/sys/class/net/lo/flags")
    if flags.exists() and int(flags.read_text(), 16) & 0x1:
        return
    run_or_explain(["ip", "link", "set", "lo", "up"], "bringing the namespace's loopback up")


def init(args: argparse.Namespace) -> int:
    """Inside pasta's user and network namespace: become PID 1 of a new PID
    namespace, run the proxy and the launch, and record the measurement."""
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.unshare(CLONE_NEWNS | CLONE_NEWPID) != 0:
        raise Unusable(
            f"unshare(CLONE_NEWNS|CLONE_NEWPID) failed: {os.strerror(ctypes.get_errno())}; this "
            "host does not allow the mount and PID namespaces the measurement needs"
        )
    child = os.fork()
    if child:
        _, status = os.waitpid(child, 0)
        return os.waitstatus_to_exitcode(status)

    private_resolver()
    loopback_up()
    proc = subprocess.run(["mount", "-t", "proc", "proc", "/proc"], capture_output=True, text=True)
    if proc.returncode != 0:
        # Only process listings are affected: the counters are read per network
        # namespace, so the measurement stands, and the reader is told.
        print(
            f"codex-network-breakdown: warning: /proc was not remounted ({proc.stderr.strip()}); "
            "the launch sees the host's process list",
            file=sys.stderr,
        )
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
            elif timed_out and deadline and now > deadline:
                os.kill(-1, signal.SIGKILL)
                deadline = now + 3600
            time.sleep(0.02)
    # Let the proxy see the last FINs before the counters are read.
    time.sleep(0.2)
    delta = counter_delta(start, read_counters())
    tunnels, refused = proxy.snapshot()
    measurement = Measurement(
        label=args.label,
        argv=args.argv,
        launch_exit=launch_exit,
        launch_seconds=round(launch_seconds or time.monotonic() - t0, 3),
        timed_out=timed_out,
        received_bytes=delta.received,
        sent_bytes=delta.sent,
        tunnels=tunnels,
        refused=refused,
    )
    Path(args.result).write_text(json.dumps(asdict(measurement)))
    return 0


def fetch_step(argv: list[str], cwd: Path | None, what: str) -> None:
    """One step of fetching pasta; a failure names the step's own error."""
    advice = "install passt (`apt install passt`) or set CODEX_NETWORK_BREAKDOWN_PASTA to a pasta binary"
    try:
        done = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    except OSError as error:
        raise Unusable(f"cannot {what}: `{argv[0]}` could not run ({error}); {advice}") from None
    if done.returncode != 0:
        raise Unusable(
            f"cannot {what}: `{' '.join(argv)}` exited {done.returncode}: "
            f"{done.stderr.strip() or 'no output'}; {advice}"
        )


def find_pasta() -> str:
    """The pasta to build the namespace with, fetching it without root only
    when nothing else provides it and fetching is allowed."""
    explicit = os.environ.get("CODEX_NETWORK_BREAKDOWN_PASTA")
    if explicit:
        return explicit
    on_path = shutil.which("pasta")
    if on_path:
        return on_path
    cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    tools = cache / "oneharness-network-breakdown"
    pasta = tools / "root" / "usr" / "bin" / "pasta"
    if pasta.exists():
        return str(pasta)
    if os.environ.get("CODEX_NETWORK_BREAKDOWN_NO_FETCH") == "1":
        raise Unusable(
            f"no pasta on PATH or in {tools}, and CODEX_NETWORK_BREAKDOWN_NO_FETCH=1 forbids "
            "fetching it; install passt (`apt install passt`), set CODEX_NETWORK_BREAKDOWN_PASTA, "
            "or run once without CODEX_NETWORK_BREAKDOWN_NO_FETCH to fetch it"
        )
    debs = tools / "debs"
    debs.mkdir(parents=True, exist_ok=True)
    fetch_step(["apt-get", "download", "passt"], debs, "fetch pasta")
    for deb in debs.glob("*.deb"):
        fetch_step(["dpkg", "-x", str(deb), str(tools / "root")], None, f"unpack {deb}")
    if not pasta.exists():
        raise Unusable(f"the fetched passt package has no {pasta}; delete {tools} and rerun")
    return str(pasta)


def env_pair(text: str) -> tuple[str, str]:
    """One `--env NAME=VALUE`, with NAME a variable name and no NUL in VALUE."""
    key, sep, value = text.partition("=")
    if not sep or not ENV_NAME.match(key) or "\0" in value:
        raise argparse.ArgumentTypeError(
            f"wants NAME=VALUE with NAME a variable name and no NUL in VALUE, got {text!r}"
        )
    return key, value


def measure(args: argparse.Namespace) -> int:
    env = dict(os.environ)
    env.update(args.env)
    pasta = find_pasta()
    scratch = tempfile.mkdtemp(prefix="codex-network-breakdown-")
    try:
        result = os.path.join(scratch, "measurement.json")
        trace_file = args.trace_out or os.path.join(scratch, "trace.log")
        launch_stdout = args.launch_stdout or (
            os.path.join(scratch, "launch.stdout") if args.trace else None
        )
        inner = [sys.executable, "-I", str(Path(__file__).resolve()), "--_init", result,
                 "--label", args.label, "--timeout", str(args.timeout)]
        if args.trace:
            inner += ["--trace", "--rust-log", args.rust_log, "--_trace-file",
                      os.path.abspath(trace_file)]
        if launch_stdout:
            inner += ["--launch-stdout", os.path.abspath(launch_stdout)]
        command = [pasta, "--config-net", "--quiet", "-4",
                   "-t", "none", "-u", "none", "-T", "none", "-U", "none",
                   "--", *inner, "--", *args.argv]
        try:
            run = subprocess.Popen(command, env=env, start_new_session=True, stdout=sys.stderr)
        except OSError as error:
            raise Unusable(
                f"cannot start pasta ({pasta}): {error}; install passt (`apt install passt`) or "
                "point CODEX_NETWORK_BREAKDOWN_PASTA at a pasta binary"
            ) from None
        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(signum, lambda *_: os.killpg(run.pid, signal.SIGTERM))
        run.wait()
        if not os.path.exists(result):
            raise Unusable(
                f"no measurement: pasta exited {run.returncode} before the namespace recorded "
                "one (its own error, if any, is above). Rerun it by hand to see why: "
                f"{pasta} --config-net --quiet -4 -- true. On a host that restricts unprivileged "
                "user namespaces (Ubuntu 24.04's AppArmor), allow them: "
                "`sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0`"
            )
        measurement = Measurement.from_json(json.loads(Path(result).read_text()), result)
        if args.measurement_out:
            Path(args.measurement_out).write_text(json.dumps(asdict(measurement), indent=2) + "\n")
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
    print(json.dumps(report(measurement, trace_text)))
    return 0


def report_mode(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="codex-network-breakdown.py report")
    parser.add_argument("measurement")
    parser.add_argument("--trace")
    args = parser.parse_args(argv)
    try:
        data = json.loads(Path(args.measurement).read_text())
    except (OSError, ValueError) as error:
        raise Unusable(f"cannot read the measurement {args.measurement}: {error}") from None
    measurement = Measurement.from_json(data, args.measurement)
    try:
        trace_text = Path(args.trace).read_text(errors="replace") if args.trace else None
    except OSError as error:
        raise Unusable(f"cannot read the trace {args.trace}: {error}") from None
    print(json.dumps(report(measurement, trace_text)))
    return 0


def timeout_seconds(text: str) -> float:
    seconds = float(text)
    if not math.isfinite(seconds) or seconds < 0:
        raise argparse.ArgumentTypeError(f"wants a finite number of seconds >= 0, got {text}")
    return seconds


def main(argv: list[str]) -> int:
    if argv[:1] == ["report"]:
        return report_mode(argv[1:])
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--label", default="")
    parser.add_argument("--env", action="append", default=[], metavar="K=V", type=env_pair)
    parser.add_argument("--timeout", type=timeout_seconds, default=0.0,
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
    try:
        sys.exit(main(sys.argv[1:]))
    except Unusable as problem:
        print(f"codex-network-breakdown: {problem}", file=sys.stderr)
        sys.exit(3)
