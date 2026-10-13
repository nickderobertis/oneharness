#!/usr/bin/env python3
"""The launch `test_codex_network_breakdown.py` measures in Codex's place.

    transfer_double.py [--detach] [--loopback] TRANSFER...
    TRANSFER = PORT:DIRECTION:BYTES[:TRACE]

Run inside the measuring namespace, it does what a Codex launch does with the
network as far as the measurement can tell: it finds its proxy only through
`HTTPS_PROXY` / `HTTP_PROXY`, and reaches the host's loopback (pasta's gateway
address stands for it inside the namespace) through one tunnel per transfer,
moving exactly BYTES with the test servers' protocol. DIRECTION is `down`
(CONNECT, then `GET n`), `up` (CONNECT, then `PUT n` and the bytes), `http`
(a plain-HTTP absolute-URI `GET` through the proxy) or `direct` (`GET n`
straight to the destination, around the proxy, as a launch that ignores the
proxy variables would). TRACE, when given, is
where the double logs the connection the way Codex's trace does: `stderr`, or
`report` — a `oneharness run`-shaped JSON report on stdout whose result carries
the line as its stderr. `--detach` hands every transfer to a detached child
and exits at once, so only a measurement that follows the whole process tree
sees them. `--loopback` asks for the namespace's own loopback instead of the
host's, where a port nothing listens on refuses at once.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Callable
from urllib.parse import urlparse


class Direction(Enum):
    DOWN = "down"
    UP = "up"
    HTTP = "http"
    DIRECT = "direct"


class TraceTo(Enum):
    NOWHERE = ""
    STDERR = "stderr"
    REPORT = "report"


@dataclass(frozen=True)
class Proxy:
    host: str
    port: int


@dataclass(frozen=True)
class Transfer:
    port: int
    direction: Direction
    size: int
    trace: TraceTo

    @staticmethod
    def parse(text: str) -> Transfer:
        parts = text.split(":")
        if len(parts) not in (3, 4):
            raise SystemExit(f"transfer_double: {text!r} is not PORT:DIRECTION:BYTES[:TRACE]")
        port, direction, size = parts[:3]
        trace = parts[3] if len(parts) == 4 else ""
        if (
            not (0 < len(port) <= 5 and port.isascii() and port.isdigit())
            or not 0 < int(port) < 65536
        ):
            raise SystemExit(f"transfer_double: {port!r} is not a port")
        if not (0 < len(size) <= 12 and size.isascii() and size.isdigit()):
            raise SystemExit(f"transfer_double: {size!r} is not a byte count")
        try:
            return Transfer(int(port), Direction(direction), int(size), TraceTo(trace))
        except ValueError:
            raise SystemExit(
                f"transfer_double: {text!r} names an unknown direction or trace"
            ) from None


def proxy_from(name: str) -> Proxy:
    text = os.environ.get(name, "")
    refusal = f"transfer_double: {name} is not an http://host:port proxy: {text!r}"
    try:
        url = urlparse(text)
        port = url.port
    except ValueError:
        raise SystemExit(refusal) from None
    # urlparse reads `http://host:0` as port 0, which names no destination.
    if url.scheme != "http" or url.hostname is None or port is None or port == 0:
        raise SystemExit(refusal)
    return Proxy(url.hostname, port)


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
        f"http://{host}:{transfer.port}/transfer/{transfer.direction.value}"
    )


def receive(conn: socket.socket, size: int) -> None:
    got = 0
    while got < size:
        data = conn.recv(65536)
        if not data:
            raise SystemExit(f"transfer_double: got {got} of {size} bytes")
        got += len(data)


# llmlint: ignore-block[async_typed_clients_at_boundaries] This double stands in for Codex at the one boundary the measurement observes — raw bytes through an HTTP proxy — so it speaks the proxy protocol by hand on purpose: a typed client would hide the CONNECT exchange and the byte counts the test asserts. It moves one transfer at a time inside a test, with nothing to overlap.
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


def download(transfer: Transfer, host: str) -> None:
    with connect_tunnel(proxy_from("HTTPS_PROXY"), host, transfer.port) as conn:
        conn.sendall(f"GET {transfer.size}\n".encode())
        receive(conn, transfer.size)


def upload(transfer: Transfer, host: str) -> None:
    with connect_tunnel(proxy_from("HTTPS_PROXY"), host, transfer.port) as conn:
        conn.sendall(f"PUT {transfer.size}\n".encode() + os.urandom(transfer.size))
        if conn.recv(16).strip() != b"OK":
            raise SystemExit("transfer_double: the upload was not acknowledged")


def plain_http(transfer: Transfer, host: str) -> None:
    proxy = proxy_from("HTTP_PROXY")
    with socket.create_connection((proxy.host, proxy.port)) as conn:
        conn.sendall(
            f"GET http://{host}:{transfer.port}/{transfer.size} HTTP/1.1\r\n"
            f"Host: {host}:{transfer.port}\r\n\r\n".encode()
        )
        receive(conn, transfer.size)


def direct(transfer: Transfer, host: str) -> None:
    with socket.create_connection((host, transfer.port)) as conn:
        conn.sendall(f"GET {transfer.size}\n".encode())
        receive(conn, transfer.size)


# llmlint: ignore-end[async_typed_clients_at_boundaries]

TRANSFERS: dict[Direction, Callable[[Transfer, str], None]] = {
    Direction.DOWN: download,
    Direction.UP: upload,
    Direction.HTTP: plain_http,
    Direction.DIRECT: direct,
}


def main(argv: list[str]) -> int:
    if argv[:1] == ["--detach"]:
        subprocess.Popen(
            [sys.executable, "-I", __file__, *argv[1:]],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
        )
        return 0
    loopback = argv[:1] == ["--loopback"]
    transfers = [Transfer.parse(text) for text in (argv[1:] if loopback else argv)]
    host = "127.0.0.1" if loopback else default_gateway()
    carried: list[str] = []
    traced: dict[TraceTo, Callable[[str], None]] = {
        TraceTo.NOWHERE: lambda _line: None,
        TraceTo.STDERR: lambda line: print(line, file=sys.stderr, flush=True),
        TraceTo.REPORT: carried.append,
    }
    for transfer in transfers:
        traced[transfer.trace](trace_line(host, transfer))
        TRANSFERS[transfer.direction](transfer, host)
    if carried:
        print(json.dumps({"results": [{"stderr": "\n".join(carried)}]}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
