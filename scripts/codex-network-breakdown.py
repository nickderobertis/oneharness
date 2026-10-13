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
launch's stderr passes through. The launch's stdout is kept in
`--launch-stdout`, else in scratch, and only a launch that exits non-zero or
times out has its last lines shown on stderr. `--measurement-out` keeps the raw
record (counters and tunnels) that report mode reads.

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
import contextlib
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
from enum import Enum
from pathlib import Path
from typing import cast

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
# The most a proxy request's head may hold before it is refused.
MAX_REQUEST_HEAD = 65536
# How long, after a signal ends pasta, the script waits for the namespace to
# finish recording the measurement it took.
RECORD_AFTER_SIGNAL_SECONDS = 5.0
# How much of a failed launch's stdout is shown when it was not kept.
LAUNCH_STDOUT_TAIL_LINES = 20
CONNECT_AFTER_SECONDS = 5.0
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Codex's tracing spelling has no schema to reconcile against short of running Codex: the committed fixture (tools/network-breakdown/recorded-codex-trace.log, from codex-cli 0.159.2, the version the README's figures name) pins what these read, and a spelling that moves degrades attribution to empty paths, never to wrong byte counts. Re-measuring against live Codex — the README's invocation, which the `oneharness-live-network-budget` plan node re-runs — is where that surfaces.
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TIMESTAMP = re.compile(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?Z\s+[A-Z]+\s+")
SPANS = re.compile(r"^(?:[\w.]+\{[^{}]*\}:)+")
CONNECT = re.compile(
    r"(?:starting new connection|connecting to websocket): ((?:https?|wss?)://[^\s,]+)"
)
COMPLETED_URL = re.compile(r"\burl=((?:https?|wss?)://[^\s,]+)")
SPAN_PATH = re.compile(r'api\.path="?([^"\s}]+)')
# The authority ends at a path, query, fragment, separator or the end: a URL
# whose port is not digits (`host:x`) is no URL this reads.
URL = re.compile(r"^(https?|wss?)://([A-Za-z0-9.-]+)(?::(\d{1,5}))?(?=$|[/?#\s,])(/[^\s\"'?#]*)?")
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
ENV_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
# A destination a proxy request may name: a hostname, an IPv4 address, or an
# IPv6 address without its brackets.
HOST = re.compile(r"[A-Za-z0-9.-]{1,253}|[0-9A-Fa-f:.]{2,45}")


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


class Counter(Enum):
    """Which of a tunnel's byte counts a relay direction adds to."""

    RECEIVED = "received_bytes"
    SENT = "sent_bytes"


@dataclass(frozen=True)
class Refusal:
    destination: str
    at: float
    error: str


RECORD_ADVICE = "re-record it with `codex-network-breakdown.py --measurement-out PATH -- ARGV...`"


def as_object(value: object, where: str) -> dict[str, object]:
    """`value` as a JSON object, refused with where it was expected."""
    if not isinstance(value, dict):
        raise Unusable(f"{where} is not a JSON object; {RECORD_ADVICE}")
    return cast("dict[str, object]", value)


def as_list(value: object, where: str) -> list[object]:
    if not isinstance(value, list):
        raise Unusable(f"{where} is not a JSON array; {RECORD_ADVICE}")
    return cast("list[object]", value)


def count_field(record: dict[str, object], key: str, where: str) -> int:
    """A byte count or port: an integer (never a bool) that is not negative."""
    value = record.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise Unusable(f"{where}'s `{key}` is {value!r}, not a count; {RECORD_ADVICE}")
    return value


def time_field(record: dict[str, object], key: str, where: str) -> float:
    """A UNIX time or duration: a finite, non-negative number."""
    value = record.get(key)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise Unusable(f"{where}'s `{key}` is {value!r}, not a time; {RECORD_ADVICE}")
    try:
        seconds = float(value)
    except OverflowError:
        raise Unusable(f"{where}'s `{key}` is too large to be a time; {RECORD_ADVICE}") from None
    if not math.isfinite(seconds) or seconds < 0:
        raise Unusable(f"{where}'s `{key}` is {value!r}, not a finite time; {RECORD_ADVICE}")
    return float(value)


def text_field(record: dict[str, object], key: str, where: str) -> str:
    value = record.get(key)
    if not isinstance(value, str):
        raise Unusable(f"{where}'s `{key}` is {value!r}, not text; {RECORD_ADVICE}")
    return value


def optional_text(record: dict[str, object], key: str, where: str) -> str | None:
    """`text_field`, or None when the field is absent or null."""
    return None if record.get(key) is None else text_field(record, key, where)


def optional_time(record: dict[str, object], key: str, where: str) -> float | None:
    """`time_field`, or None when the field is absent or null."""
    return None if record.get(key) is None else time_field(record, key, where)


def tunnel_from(value: object, where: str) -> Tunnel:
    record = as_object(value, where)
    tunnel = Tunnel(
        host=text_field(record, "host", where),
        port=count_field(record, "port", where),
        opened_at=time_field(record, "opened_at", where),
        received_bytes=count_field(record, "received_bytes", where),
        sent_bytes=count_field(record, "sent_bytes", where),
        closed_at=time_field(record, "closed_at", where),
        error=optional_text(record, "error", where),
    )
    if not 0 < tunnel.port < 65536:
        raise Unusable(f"{where}'s `port` {tunnel.port} is no port; {RECORD_ADVICE}")
    if tunnel.closed_at is not None and tunnel.closed_at < tunnel.opened_at:
        raise Unusable(f"{where} closed before it opened; {RECORD_ADVICE}")
    return tunnel


def refusal_from(value: object, where: str) -> Refusal:
    record = as_object(value, where)
    return Refusal(
        destination=text_field(record, "destination", where),
        at=time_field(record, "at", where),
        error=text_field(record, "error", where),
    )


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
        record = as_object(data, source)
        argv = as_list(record.get("argv", []), f"{source}'s `argv`")
        if not all(isinstance(arg, str) for arg in argv):
            raise Unusable(f"{source}'s `argv` holds a non-text argument; {RECORD_ADVICE}")
        launch_exit = record.get("launch_exit")
        if launch_exit is not None and (
            isinstance(launch_exit, bool) or not isinstance(launch_exit, int)
        ):
            raise Unusable(
                f"{source}'s `launch_exit` is {launch_exit!r}, not an exit code; {RECORD_ADVICE}"
            )
        timed_out = record.get("timed_out", False)
        if not isinstance(timed_out, bool):
            raise Unusable(
                f"{source}'s `timed_out` is {timed_out!r}, not true or false; {RECORD_ADVICE}"
            )
        label = record.get("label", "")
        if not isinstance(label, str):
            raise Unusable(f"{source}'s `label` is {label!r}, not text; {RECORD_ADVICE}")
        return Measurement(
            label=label,
            argv=[str(arg) for arg in argv],
            launch_exit=launch_exit,
            launch_seconds=optional_time(record, "launch_seconds", source),
            timed_out=timed_out,
            received_bytes=count_field(record, "received_bytes", source),
            sent_bytes=count_field(record, "sent_bytes", source),
            tunnels=[
                tunnel_from(tunnel, f"{source}: tunnel {index}")
                for index, tunnel in enumerate(
                    as_list(record.get("tunnels"), f"{source}'s `tunnels`")
                )
            ],
            refused=[
                refusal_from(refusal, f"{source}: refusal {index}")
                for index, refusal in enumerate(
                    as_list(record.get("refused", []), f"{source}'s `refused`")
                )
            ],
        )


def parse_url(url: str) -> Url | None:
    """An http(s)/ws(s) URL's host, port (443 for https/wss, else 80 when
    unstated) and path (`/` when unstated)."""
    match = URL.match(url)
    if not match:
        return None
    default = 443 if match.group(1) in ("https", "wss") else 80
    port = int(match.group(3)) if match.group(3) else default
    if not 0 < port < 65536:
        return None
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
        try:
            whole = datetime.strptime(stamp.group(1), "%Y-%m-%dT%H:%M:%S")
        except ValueError:
            continue  # shaped like a timestamp, but no date that exists
        # The trace is UTC; a naive `timestamp()` would read it as local time.
        seconds = (whole - datetime(1970, 1, 1)).total_seconds()
        if stamp.group(2):
            seconds += float("0" + stamp.group(2))
        rest = line[stamp.end() :]
        spans = SPANS.match(rest)
        spans_text = spans.group(0) if spans else ""
        lines.append(TraceLine(seconds, spans_text, rest[len(spans_text) :]))
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
        for later in lines[index + 1 :]:
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
    result after another; "" when the text is not such a report. The fields
    read (`results[].stderr`) are held to the generated run report schema by
    the test's RunReportShape."""
    try:
        document: object = json.loads(stdout_text)
    except ValueError:
        return ""
    if not isinstance(document, dict):
        return ""
    results = cast("dict[str, object]", document).get("results")
    if not isinstance(results, list):
        return ""
    carried = []
    for result in cast("list[object]", results):
        if isinstance(result, dict):
            stderr = cast("dict[str, object]", result).get("stderr")
            if isinstance(stderr, str):
                carried.append(stderr)
    return "\n".join(carried)


def report(measurement: Measurement, trace_text: str | None) -> dict[str, object]:
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
            except OSError as error:
                print(
                    f"codex-network-breakdown: warning: the proxy stopped accepting connections "
                    f"({type(error).__name__}: {error}); any opened after this are missing from "
                    "`tunnels` — rerun the measurement",
                    file=sys.stderr,
                )
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
        """Serve one proxy connection; one whose handshake fails is recorded
        as refused, with the error, rather than lost with its thread."""
        try:
            self._serve(client)
        except OSError as error:
            with self.lock:
                self.refused.append(
                    Refusal("?", time.time(), f"handshake: {type(error).__name__}: {error}")
                )
            client.close()

    def _serve(self, client: socket.socket) -> None:
        head = b""
        while b"\r\n\r\n" not in head:
            data = client.recv(4096)
            if not data:
                self._refuse(
                    client, "400 Bad Request", "?", "closed before its request was complete"
                )
                return
            head += data
            if len(head) > MAX_REQUEST_HEAD:
                self._refuse(
                    client,
                    "431 Request Header Fields Too Large",
                    "?",
                    f"request head over {MAX_REQUEST_HEAD} bytes",
                )
                return
        request = head.split(b"\r\n", 1)[0].decode("latin-1")
        # llmlint: ignore-block[structural_pattern_matching] The script's ruff and ty settings are the Python SDK's, which target Python 3.9 (tools/network-breakdown/project.json says why they are shared); `match` is a syntax error there, so the request line is dispatched with length and prefix checks.
        parts = request.split(" ")
        if len(parts) != 3:
            self._refuse(client, "400 Bad Request", "?", f"malformed request line {request!r}")
            return
        method, target, _ = parts
        connect = method == "CONNECT"
        if connect:
            host, _, port = target.rpartition(":")
        elif target.startswith("http://"):
            hostport = target[len("http://") :].split("/", 1)[0]
            host, _, port = hostport.partition(":")
            port = port or "80"
        else:
            self._refuse(client, "400 Bad Request", target, f"not a proxy request: {request!r}")
            return
        # llmlint: ignore-end[structural_pattern_matching]
        host = host.strip("[]")
        if not HOST.fullmatch(host) or not is_port(port):
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
        if connect:
            client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            forwarded = rest
        else:
            forwarded = head
        if forwarded:
            upstream.sendall(forwarded)
            self._count(tunnel, Counter.SENT, len(forwarded))
        down = threading.Thread(
            target=self._pump, args=(upstream, client, tunnel, Counter.RECEIVED), daemon=True
        )
        down.start()
        self._pump(client, upstream, tunnel, Counter.SENT)
        down.join()
        with self.lock:
            tunnel.closed_at = time.time()
        client.close()
        upstream.close()

    def _count(self, tunnel: Tunnel, counter: Counter, n: int) -> None:
        with self.lock:
            if counter is Counter.RECEIVED:
                tunnel.received_bytes += n
            else:
                tunnel.sent_bytes += n

    def _pump(
        self, src: socket.socket, dst: socket.socket, tunnel: Tunnel, counter: Counter
    ) -> None:
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
                self._count(tunnel, counter, len(data))
        except OSError as error:
            # A reset is how many TLS peers end a connection; it is recorded
            # rather than swallowed, so a relay that broke is visible.
            with self.lock:
                if tunnel.error is None:
                    tunnel.error = f"{counter.value}: {type(error).__name__}: {error}"
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
                Tunnel(
                    host=tunnel.host,
                    port=tunnel.port,
                    opened_at=tunnel.opened_at,
                    received_bytes=tunnel.received_bytes,
                    sent_bytes=tunnel.sent_bytes,
                    closed_at=now if tunnel.closed_at is None else tunnel.closed_at,
                    error=tunnel.error,
                )
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
        received=sum(
            after.received - start.get(name, zero).received for name, after in end.items()
        ),
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
    servers = [
        words[1]
        for words in (line.split() for line in current.splitlines())
        if len(words) >= 2 and words[0] == "nameserver"
    ]
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
            "the launch sees the host's process list. The byte counts stand; if the launch "
            "inspects processes, rerun on a host that lets an unprivileged namespace mount proc",
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
    try:
        stdout = open(args.launch_stdout, "wb")
        stderr = open(args.trace_file, "wb") if args.trace else None
    except OSError as error:
        raise Unusable(
            f"cannot open the launch's output file: {error}; point --launch-stdout or "
            "--trace-out at a writable path, then rerun"
        ) from None
    start = read_counters()
    t0 = time.monotonic()
    try:
        launch = subprocess.Popen(args.argv, stdout=stdout, stderr=stderr, env=env)
    except OSError as error:
        raise Unusable(
            f"cannot start the launch `{args.argv[0]}`: {error}; check the command after `--` "
            "is installed and on PATH inside the namespace"
        ) from None
    launch_exit: int | None = None
    launch_seconds: float | None = None
    timeout = float(args.timeout)
    deadline: float | None = t0 + timeout if timeout > 0 else None
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
            if deadline is not None and now > deadline and not timed_out:
                timed_out = True
                os.kill(-1, signal.SIGTERM)
                deadline = now + 5
            elif timed_out and deadline is not None and now > deadline:
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
        launch_seconds=round(
            time.monotonic() - t0 if launch_seconds is None else launch_seconds, 3
        ),
        timed_out=timed_out,
        received_bytes=delta.received,
        sent_bytes=delta.sent,
        tunnels=tunnels,
        refused=refused,
    )
    # Written whole and then renamed: the script outside may be waiting for
    # this file, and must never read half of it.
    partial = Path(args.result + ".partial")
    write_text(partial, json.dumps(asdict(measurement)))
    os.replace(partial, args.result)
    return 0


# llmlint: ignore-block[changed_behavior_has_e2e] A real fetch reaches Debian's package archive, which the gate never contacts: the network-breakdown target sets CODEX_NETWORK_BREAKDOWN_NO_FETCH=1. Its test drives this path end to end with stand-in `apt-get` and `dpkg` that take the same argv and leave the same file layout, and the real fetch is what provisioned pasta for the README's measured figures on this repository's host.
def fetch_step(argv: list[str], cwd: Path | None, what: str) -> None:
    """One step of fetching pasta; a failure names the step's own error."""
    advice = (
        "install passt (`apt install passt`) or set CODEX_NETWORK_BREAKDOWN_PASTA to a pasta binary"
    )
    try:
        done = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    except OSError as error:
        raise Unusable(f"cannot {what}: `{argv[0]}` could not run ({error}); {advice}") from None
    if done.returncode != 0:
        raise Unusable(
            f"cannot {what}: `{' '.join(argv)}` exited {done.returncode}: "
            f"{done.stderr.strip() or 'no output'}; {advice}"
        )


def pasta_tools() -> Path:
    """Where a fetched pasta is unpacked, under the user's cache directory."""
    cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    return cache / "oneharness-network-breakdown"


def find_pasta() -> str | None:
    """The pasta already on this host: the explicit override, then PATH, then
    one an earlier run fetched. Never fetches."""
    explicit = os.environ.get("CODEX_NETWORK_BREAKDOWN_PASTA", "")
    if explicit != "":
        return explicit
    on_path = shutil.which("pasta")
    if on_path is not None:
        return on_path
    pasta = pasta_tools() / "root" / "usr" / "bin" / "pasta"
    return str(pasta) if pasta.exists() else None


def fetch_pasta() -> str:
    """Download and unpack Debian's passt package without root, into the cache
    `pasta_tools` names, unless CODEX_NETWORK_BREAKDOWN_NO_FETCH=1 forbids it."""
    tools = pasta_tools()
    pasta = tools / "root" / "usr" / "bin" / "pasta"
    if os.environ.get("CODEX_NETWORK_BREAKDOWN_NO_FETCH") == "1":
        raise Unusable(
            f"no pasta on PATH or in {tools}, and CODEX_NETWORK_BREAKDOWN_NO_FETCH=1 forbids "
            "fetching it; install passt (`apt install passt`), set CODEX_NETWORK_BREAKDOWN_PASTA, "
            "or run once without CODEX_NETWORK_BREAKDOWN_NO_FETCH to fetch it"
        )
    debs = tools / "debs"
    try:
        debs.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise Unusable(
            f"cannot create {debs} to fetch pasta into: {error}; point XDG_CACHE_HOME at a "
            "writable directory, or set CODEX_NETWORK_BREAKDOWN_PASTA"
        ) from None
    fetch_step(["apt-get", "download", "passt"], debs, "fetch pasta")
    for deb in debs.glob("*.deb"):
        fetch_step(["dpkg", "-x", str(deb), str(tools / "root")], None, f"unpack {deb}")
    if not pasta.exists():
        raise Unusable(f"the fetched passt package has no {pasta}; delete {tools} and rerun")
    return str(pasta)


# llmlint: ignore-end[changed_behavior_has_e2e]


def is_port(text: str) -> bool:
    """Whether `text` is a TCP port: one to five ASCII digits naming 1 to 65535."""
    return 0 < len(text) <= 5 and text.isascii() and text.isdigit() and 0 < int(text) < 65536


def env_pair(text: str) -> tuple[str, str]:
    """One `--env NAME=VALUE`, with NAME a variable name and no NUL in VALUE."""
    key, sep, value = text.partition("=")
    if not sep or not ENV_NAME.fullmatch(key) or "\0" in value:
        raise argparse.ArgumentTypeError(
            f"wants NAME=VALUE with NAME a variable name and no NUL in VALUE, got {text!r}"
        )
    return key, value


def measure(args: argparse.Namespace) -> int:
    env = dict(os.environ)
    env.update(args.env)
    pasta = find_pasta() or fetch_pasta()
    try:
        scratch = tempfile.mkdtemp(prefix="codex-network-breakdown-")
    except OSError as error:
        raise Unusable(
            f"cannot create a scratch directory: {error}; point TMPDIR at a writable directory"
        ) from None
    try:
        result = os.path.join(scratch, "measurement.json")
        trace_file = args.trace_out or os.path.join(scratch, "trace.log")
        launch_stdout = args.launch_stdout or os.path.join(scratch, "launch.stdout")
        inner = [
            sys.executable,
            "-I",
            str(Path(__file__).resolve()),
            "--_init",
            result,
            "--label",
            args.label,
            "--timeout",
            str(args.timeout),
        ]
        if args.trace:
            inner += [
                "--trace",
                "--rust-log",
                args.rust_log,
                "--_trace-file",
                os.path.abspath(trace_file),
            ]
        inner += ["--launch-stdout", os.path.abspath(launch_stdout)]
        command = [
            pasta,
            "--config-net",
            "--quiet",
            "-4",
            "-t",
            "none",
            "-u",
            "none",
            "-T",
            "none",
            "-U",
            "none",
            "--",
            *inner,
            "--",
            *args.argv,
        ]
        try:
            run = subprocess.Popen(command, env=env, start_new_session=True, stdout=sys.stderr)
        except OSError as error:
            raise Unusable(
                f"cannot start pasta ({pasta}): {error}; install passt (`apt install passt`) or "
                "point CODEX_NETWORK_BREAKDOWN_PASTA at a pasta binary"
            ) from None
        interrupted: list[int] = []

        def forward(signum: int, _frame: object) -> None:
            interrupted.append(signum)
            # pasta may have exited between the signal and this handler; the
            # wait below then returns at once, and nothing is left to end.
            with contextlib.suppress(ProcessLookupError):
                os.killpg(run.pid, signal.SIGTERM)

        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(signum, forward)
        run.wait()
        # A forwarded signal ends pasta at once, while the namespace's init is
        # still ending the launch's tree and recording what it measured.
        record_by = time.monotonic() + RECORD_AFTER_SIGNAL_SECONDS
        while interrupted and not os.path.exists(result) and time.monotonic() < record_by:
            time.sleep(0.05)
        if not os.path.exists(result):
            raise Unusable(
                f"no measurement: pasta exited {run.returncode} before the namespace recorded "
                "one (its own error, if any, is above). Rerun it by hand to see why: "
                f"{pasta} --config-net --quiet -4 -- true. On a host that restricts unprivileged "
                "user namespaces (Ubuntu 24.04's AppArmor), allow them: "
                "`sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0`"
            )
        measurement = Measurement.from_json(json.loads(read_text(Path(result))), result)
        if args.measurement_out is not None:
            write_text(Path(args.measurement_out), json.dumps(asdict(measurement), indent=2) + "\n")
        stdout_text = read_text(Path(launch_stdout))
        if measurement.launch_exit != 0 and args.launch_stdout is None:
            tail = stdout_text.splitlines()[-LAUNCH_STDOUT_TAIL_LINES:]
            print(
                f"codex-network-breakdown: the launch exited {measurement.launch_exit} "
                f"(none: timed out); the last {len(tail)} line(s) of its stdout follow. "
                "The measurement stands; rerun with --launch-stdout PATH to keep all of it",
                *tail,
                sep="\n",
                file=sys.stderr,
            )
        trace_text = None
        if args.trace:
            trace_text = read_text(Path(trace_file))
            carried = oneharness_stderr(stdout_text)
            if carried:
                trace_text += "\n" + carried
                write_text(Path(trace_file), trace_text)
    finally:
        remove_scratch(Path(scratch))
    print(json.dumps(report(measurement, trace_text)))
    return 0


def read_text(path: Path) -> str:
    """A file this script reads, or why not and what to do about it."""
    try:
        return path.read_text(errors="replace")
    except OSError as error:
        raise Unusable(
            f"cannot read {path}: {error}; check the path exists and is readable, then rerun"
        ) from None


def write_text(path: Path, text: str) -> None:
    """A file this script writes, or why not and what to do about it."""
    try:
        path.write_text(text)
    except OSError as error:
        raise Unusable(
            f"cannot write {path}: {error}; point the option at a writable path, then rerun"
        ) from None


def remove_scratch(scratch: Path) -> None:
    """Remove this run's scratch directory, saying so when part of it stays."""

    def left_behind(_function: object, path: str, error: object) -> None:
        print(
            f"codex-network-breakdown: warning: could not remove {path} ({error}); delete "
            f"{scratch} by hand",
            file=sys.stderr,
        )

    shutil.rmtree(scratch, onerror=left_behind)


def report_mode(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="codex-network-breakdown.py report")
    parser.add_argument("measurement")
    parser.add_argument("--trace")
    args = parser.parse_args(argv)
    text = read_text(Path(args.measurement))
    try:
        data: object = json.loads(text)
    except ValueError as error:
        raise Unusable(f"{args.measurement} is not JSON ({error}); {RECORD_ADVICE}") from None
    measurement = Measurement.from_json(data, args.measurement)
    trace_text = read_text(Path(args.trace)) if args.trace is not None else None
    print(json.dumps(report(measurement, trace_text)))
    return 0


def timeout_seconds(text: str) -> float:
    try:
        seconds = float(text)
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"wants a finite number of seconds >= 0, got {text!r}"
        ) from None
    if not math.isfinite(seconds) or seconds < 0:
        raise argparse.ArgumentTypeError(f"wants a finite number of seconds >= 0, got {text}")
    return seconds


def main(argv: list[str]) -> int:
    if argv[:1] == ["report"]:
        return report_mode(argv[1:])
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--label", default="")
    parser.add_argument("--env", action="append", default=[], metavar="K=V", type=env_pair)
    parser.add_argument(
        "--timeout",
        type=timeout_seconds,
        default=0.0,
        help="after S seconds SIGTERM every process left, SIGKILL 5 s later",
    )
    parser.add_argument(
        "--trace",
        action="store_true",
        help="set RUST_LOG for the launch and attribute request paths from it",
    )
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
