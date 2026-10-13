#!/usr/bin/env python3
"""The launch `test_codex_network_breakdown.py` measures in Codex's place.

    transfer_double.py PORT:down:BYTES ... PORT:up:BYTES ...

Run inside the measuring namespace, it does what a Codex launch does with the
network as far as the measurement can tell: it finds its proxy only through
`HTTPS_PROXY`, opens one CONNECT tunnel per transfer to the host's loopback
(pasta's gateway address stands for it inside the namespace), and downloads
or uploads exactly BYTES over it with the test servers' one-line protocol
(`GET n` answers n bytes; `PUT n` reads n bytes and answers `OK`).
"""

import os
import socket
import sys
from pathlib import Path
from urllib.parse import urlparse


def default_gateway() -> str:
    for line in Path("/proc/net/route").read_text().splitlines()[1:]:
        fields = line.split()
        if fields[1] == "00000000":
            return socket.inet_ntoa(int(fields[2], 16).to_bytes(4, "little"))
    raise SystemExit("transfer_double: no default route in this namespace")


def tunnel(proxy: tuple, host: str, port: int) -> socket.socket:
    conn = socket.create_connection(proxy)
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


def main() -> int:
    proxy_url = urlparse(os.environ["HTTPS_PROXY"])
    proxy = (proxy_url.hostname, proxy_url.port)
    host = default_gateway()
    for transfer in sys.argv[1:]:
        port, direction, size = transfer.split(":")
        size = int(size)
        with tunnel(proxy, host, int(port)) as conn:
            if direction == "down":
                conn.sendall(f"GET {size}\n".encode())
                got = 0
                while got < size:
                    data = conn.recv(65536)
                    if not data:
                        raise SystemExit(f"transfer_double: got {got} of {size} bytes")
                    got += len(data)
            else:
                conn.sendall(f"PUT {size}\n".encode() + os.urandom(size))
                if conn.recv(16).strip() != b"OK":
                    raise SystemExit("transfer_double: the upload was not acknowledged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
