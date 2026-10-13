#!/usr/bin/env python3
"""The launch `test_codex_network_breakdown.py` measures in Codex's place.

    transfer_double.py [--detach] TRANSFER...
    TRANSFER = PORT:DIRECTION:BYTES[:TRACE]

Run inside the measuring namespace, it does what a Codex launch does with the
network as far as the measurement can tell: it finds its proxy only through
`HTTPS_PROXY` / `HTTP_PROXY`, and reaches the host's loopback (pasta's gateway
address stands for it inside the namespace) through one tunnel per transfer,
moving exactly BYTES with the test servers' protocol. DIRECTION is `down`
(CONNECT, then `GET n`), `up` (CONNECT, then `PUT n` and the bytes) or `http`
(a plain-HTTP absolute-URI `GET` through the proxy). TRACE, when given, is
where the double logs the connection the way Codex's trace does: `stderr`, or
`report` — a `oneharness run`-shaped JSON report on stdout whose result carries
the line as its stderr. `--detach` hands every transfer to a detached child
and exits at once, so only a measurement that follows the whole process tree
sees them.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

DIRECTIONS = ("down", "up", "http")
TRACES = ("", "stderr", "report")


@dataclass(frozen=True)
class Proxy:
    host: str
    port: int


@dataclass(frozen=True)
class Transfer:
    port: int
    direction: str
    size: int
    trace: str

    @staticmethod
    def parse(text: str) -> Transfer:
        parts = text.split(":")
        if len(parts) not in (3, 4):
            raise SystemExit(f"transfer_double: {text!r} is not PORT:DIRECTION:BYTES[:TRACE]")
        port, direction, size = parts[:3]
        trace = parts[3] if len(parts) == 4 else ""
        if not port.isdigit() or not 0 < int(port) < 65536:
            raise SystemExit(f"transfer_double: {port!r} is not a port")
        if direction not in DIRECTIONS or trace not in TRACES:
            raise SystemExit(f"transfer_double: {text!r} names an unknown direction or trace")
        if not size.isdigit():
            raise SystemExit(f"transfer_double: {size!r} is not a byte count")
        return Transfer(int(port), direction, int(size), trace)


def proxy_from(name: str) -> Proxy:
    url = urlparse(os.environ.get(name, ""))
    if url.scheme != "http" or not url.hostname or not url.port:
        raise SystemExit(f"transfer_double: {name} is not an http://host:port proxy: {url.geturl()!r}")
    return Proxy(url.hostname, url.port)


def default_gateway() -> str:
    for line in Path("/proc/net/route").read_text().splitlines()[1:]:
        fields = line.split()
        if fields[1] == "00000000":
            return socket.inet_ntoa(int(fields[2], 16).to_bytes(4, "little"))
    raise SystemExit("transfer_double: no default route in this namespace")


def trace_line(host: str, transfer: Transfer) -> str:
    """The line Codex's trace logs as it starts a connection."""
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
    return (
        f"{now} DEBUG reqwest::connect: starting new connection: "
        f"http://{host}:{transfer.port}/transfer/{transfer.direction}"
    )


def receive(conn: socket.socket, size: int) -> None:
    got = 0
    while got < size:
        data = conn.recv(65536)
        if not data:
            raise SystemExit(f"transfer_double: got {got} of {size} bytes")
        got += len(data)


def connect_tunnel(proxy: Proxy, host: str, port: int) -> socket.socket:
    conn = socket.create_connection((proxy.host, proxy.port))
    conn.sendall(f"CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n".encode())
    reply = b""
    while b"\r\n\r\n" not in reply:
        data = conn.recv(4096)
        if not data:
            raise SystemExit("transfer_double: the proxy closed the tunnel")
        reply += data
    if b" 200 " not in reply.split(b"\r\n", 1)[0]:
        raise SystemExit(f"transfer_double: the proxy refused: {reply!r}")
    return conn


def run(transfer: Transfer, host: str) -> None:
    if transfer.direction == "http":
        proxy = proxy_from("HTTP_PROXY")
        with socket.create_connection((proxy.host, proxy.port)) as conn:
            conn.sendall(
                f"GET http://{host}:{transfer.port}/{transfer.size} HTTP/1.1\r\n"
                f"Host: {host}:{transfer.port}\r\n\r\n".encode()
            )
            receive(conn, transfer.size)
        return
    with connect_tunnel(proxy_from("HTTPS_PROXY"), host, transfer.port) as conn:
        if transfer.direction == "down":
            conn.sendall(f"GET {transfer.size}\n".encode())
            receive(conn, transfer.size)
        else:
            conn.sendall(f"PUT {transfer.size}\n".encode() + os.urandom(transfer.size))
            if conn.recv(16).strip() != b"OK":
                raise SystemExit("transfer_double: the upload was not acknowledged")


def main(argv: list[str]) -> int:
    if argv[:1] == ["--detach"]:
        subprocess.Popen(
            [sys.executable, "-I", __file__, *argv[1:]],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
        )
        return 0
    transfers = [Transfer.parse(text) for text in argv]
    host = default_gateway()
    carried = []
    for transfer in transfers:
        if transfer.trace == "stderr":
            print(trace_line(host, transfer), file=sys.stderr, flush=True)
        elif transfer.trace == "report":
            carried.append(trace_line(host, transfer))
        run(transfer, host)
    if carried:
        print(json.dumps({"results": [{"stderr": "\n".join(carried)}]}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
