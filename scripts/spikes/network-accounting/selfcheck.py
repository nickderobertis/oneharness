#!/usr/bin/env python3
"""Loopback self-check of netmeasure.py, run through that same command.

    selfcheck.py [--down BYTES] [--up BYTES] [--unrelated BYTES]

A raw-TCP server on the host's 127.0.0.1 serves a generated payload and accepts
an upload. The measured launch is `sh -c 'python3 selfcheck.py --client ...'`:
its CHILD process downloads --down bytes and uploads --up bytes, reaching the
host loopback through pasta's gateway mapping. Meanwhile an unrelated host
process, outside the launch's namespace, downloads --unrelated bytes from the
same server across the whole launch. The report sets the recorded figures
beside both payload sizes, accounts the difference to per-frame header bytes,
and shows the unrelated transfer is absent.

What this validates and what it does not: attribution (only the launch's tree,
never the unrelated transfer) and payload exactness of the TAP counters. The
frames are pasta's TAP frames — GSO-sized up to the TAP's 65520 MTU (5 MB in
~104 frames), not physical packets — and the server leg is host loopback, so
this says nothing about host-wire bytes; boundary.py compares against those.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHUNK = 64 * 1024
# Per-frame header bytes on the TAP device (namespace-side framing, not the
# host wire): Ethernet 14 + IPv4 20 + TCP 20.
# pasta terminates TCP itself and negotiates no timestamp option, so established
# segments carry no options; what is left over is the SYNs' option bytes.
FRAME_HEADERS = 14 + 20 + 20


def recv_exact(conn: socket.socket, n: int) -> int:
    got = 0
    while got < n:
        data = conn.recv(min(CHUNK, n - got))
        if not data:
            break
        got += len(data)
    return got


def recv_line(conn: socket.socket) -> bytes:
    line = b""
    while not line.endswith(b"\n"):
        data = conn.recv(1)
        if not data:
            break
        line += data
    return line


def serve(conn: socket.socket) -> None:
    """`GET n [pace]` sends n bytes (optionally paced over `pace` seconds); `PUT n` reads n."""
    with conn:
        verb, *rest = recv_line(conn).split()
        n = int(rest[0])
        if verb == b"GET":
            pace = float(rest[1]) if len(rest) > 1 else 0.0
            block = os.urandom(CHUNK)
            sent, steps = 0, max(1, n // CHUNK)
            while sent < n:
                piece = block[: min(CHUNK, n - sent)]
                conn.sendall(piece)
                sent += len(piece)
                if pace:
                    time.sleep(pace / steps)
        else:
            got = recv_exact(conn, n)
            conn.sendall(f"OK {got}\n".encode())


def server() -> socket.socket:
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()

    def accept() -> None:
        while True:
            conn, _ = listener.accept()
            threading.Thread(target=serve, args=(conn,), daemon=True).start()

    threading.Thread(target=accept, daemon=True).start()
    return listener


def default_gateway() -> str:
    for line in Path("/proc/net/route").read_text().splitlines()[1:]:
        fields = line.split()
        if fields[1] == "00000000":
            return socket.inet_ntoa(int(fields[2], 16).to_bytes(4, "little"))
    raise SystemExit("no default route in this namespace")


def client(port: int, down: int, up: int) -> None:
    """The launch's child: download `down` bytes, then upload `up` bytes."""
    host = default_gateway()  # pasta maps the gateway address to the host's loopback
    with socket.create_connection((host, port)) as conn:
        conn.sendall(f"GET {down}\n".encode())
        got = recv_exact(conn, down)
    payload = os.urandom(up)
    with socket.create_connection((host, port)) as conn:
        conn.sendall(f"PUT {up}\n".encode() + payload)
        reply = recv_line(conn).decode().strip()
    print(json.dumps({"client_received": got, "client_sent": up, "reply": reply,
                      "host": host}), file=sys.stderr)


def unrelated(port: int, size: int, seconds: float, out: dict) -> None:
    out["start"] = time.time()
    with socket.create_connection(("127.0.0.1", port)) as conn:
        conn.sendall(f"GET {size} {seconds}\n".encode())
        out["received"] = recv_exact(conn, size)
    out["end"] = time.time()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--down", type=int, default=5_000_000)
    parser.add_argument("--up", type=int, default=3_000_000)
    parser.add_argument("--unrelated", type=int, default=7_777_777)
    parser.add_argument("--client", type=int, metavar="PORT", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.client:
        client(args.client, args.down, args.up)
        return 0

    port = server().getsockname()[1]
    side: dict = {}
    other = threading.Thread(target=unrelated, args=(port, args.unrelated, 4.0, side))
    other.start()
    time.sleep(0.5)  # the unrelated transfer is under way before the launch starts
    launch = ["sh", "-c", f'exec "$0" "$1" --client {port} --down {args.down} --up {args.up}',
              sys.executable, str(Path(__file__).resolve())]
    measured = subprocess.run([str(HERE / "netmeasure.py"), "--label", "selfcheck", "--", *launch],
                              capture_output=True, text=True, check=True)
    other.join()
    sys.stderr.write(measured.stderr)
    report = json.loads(measured.stdout.strip().splitlines()[-1])
    launch_start, launch_end = report["started"], report["started"] + report["tree_seconds"]
    rx, tx = report["received_bytes"], report["sent_bytes"]
    rx_headers, tx_headers = report["rx_packets"] * FRAME_HEADERS, report["tx_packets"] * FRAME_HEADERS
    command_bytes_tx = len(f"GET {args.down}\n") + len(f"PUT {args.up}\n")
    reply_bytes_rx = len(f"OK {args.up}\n")
    verdict = {
        "method": report["method"], "boundary": report["boundary"],
        "received_bytes": rx, "download_payload": args.down,
        "received_over_payload": rx - args.down,
        "rx_packets": report["rx_packets"],
        "rx_header_bytes_accounted": rx_headers, "rx_app_protocol_bytes": reply_bytes_rx,
        "rx_unexplained": rx - args.down - reply_bytes_rx - rx_headers,
        "sent_bytes": tx, "upload_payload": args.up,
        "sent_over_payload": tx - args.up,
        "tx_packets": report["tx_packets"],
        "tx_header_bytes_accounted": tx_headers, "tx_app_protocol_bytes": command_bytes_tx,
        "tx_unexplained": tx - args.up - command_bytes_tx - tx_headers,
        "unrelated_payload": args.unrelated, "unrelated_received": side.get("received"),
        "unrelated_overlapped_launch": side["start"] < launch_start and side["end"] > launch_end,
        "unrelated_absent": rx < args.down + args.unrelated / 2 and tx < args.up + args.unrelated / 2,
    }
    ok = (args.down <= rx < args.down * 1.05 and args.up <= tx < args.up * 1.05
          and verdict["unrelated_overlapped_launch"] and verdict["unrelated_absent"]
          and side.get("received") == args.unrelated)
    verdict["pass"] = ok
    print(json.dumps(verdict, indent=2))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
