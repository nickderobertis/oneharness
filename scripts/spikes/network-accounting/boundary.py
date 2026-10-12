#!/usr/bin/env python3
"""Where netmeasure.py's TAP counters sit against host-side traffic (zero paid turns).

    boundary.py [--down BYTES] [--up BYTES] [--loss PCT]

An owned transfer: a client inside netmeasure's namespace opens ONE HTTPS
keep-alive connection to speed.cloudflare.com, downloads --down bytes, uploads
--up bytes, then holds the connection open 2 s so the host side can be sampled
before it closes. netmeasure runs it twice — clean, and with `netem loss PCT`
on the namespace side of the TAP — with --host-sockets polling the host-side
tcp_info of pasta's socket to that peer (bytes, real segment counts,
retransmissions). The host NIC's own counters are read too, but they carry the
whole host's traffic, so they only bound the comparison from above.
Prints one JSON object per run.
"""

import argparse
import json
import socket
import ssl
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
HOST = "speed.cloudflare.com"


def client(down: int, up: int) -> None:
    raw = socket.create_connection((HOST, 443))
    conn = ssl.create_default_context().wrap_socket(raw, server_hostname=HOST)

    def response() -> int:
        head = b""
        while b"\r\n\r\n" not in head:
            head += conn.recv(1)
        length = int([l for l in head.split(b"\r\n") if l.lower().startswith(b"content-length:")][0]
                     .split(b":")[1])
        got = 0
        while got < length:
            got += len(conn.recv(min(65536, length - got)))
        return got

    conn.sendall(f"GET /__down?bytes={down} HTTP/1.1\r\nHost: {HOST}\r\n\r\n".encode())
    body_down = response()
    conn.sendall(f"POST /__up HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: {up}\r\n"
                 f"Content-Type: application/octet-stream\r\n\r\n".encode())
    payload = bytes(65536)
    sent = 0
    while sent < up:
        piece = payload[: min(65536, up - sent)]
        conn.sendall(piece)
        sent += len(piece)
    response()
    time.sleep(2)  # keep the host-side socket alive for a final sample
    conn.close()
    print(json.dumps({"body_received": body_down, "body_sent": sent}), file=sys.stderr)


def nic_counters() -> dict:
    iface = next(l.split()[4] for l in subprocess.run(["ip", "route"], capture_output=True, text=True)
                 .stdout.splitlines() if l.startswith("default"))
    base = Path("/sys/class/net") / iface / "statistics"
    return {"iface": iface, **{k: int((base / k).read_text()) for k in
                               ("rx_bytes", "tx_bytes", "rx_packets", "tx_packets")}}


def run(down: int, up: int, loss: float) -> dict:
    peers = sorted({a[4][0] for a in socket.getaddrinfo(HOST, 443, socket.AF_INET)})
    argv = [str(HERE / "netmeasure.py"), "--label", f"boundary loss={loss}",
            "--host-sockets", ",".join(peers)]
    if loss:
        argv += ["--netem", f"loss {loss}%"]
    argv += ["--", sys.executable, str(Path(__file__).resolve()), "--client",
             "--down", str(down), "--up", str(up)]
    nic0 = nic_counters()
    done = subprocess.run(argv, capture_output=True, text=True)
    nic1 = nic_counters()
    report = json.loads(done.stdout.strip().splitlines()[-1])
    bodies = json.loads([l for l in done.stderr.splitlines() if l.startswith('{"body')][-1])
    host = report["host_sockets"]
    sock = host["sockets"]
    return {
        "loss_pct_namespace_side": loss,
        "body_down": down, "body_up": up, **bodies,
        "tap_received": report["received_bytes"], "tap_sent": report["sent_bytes"],
        "tap_rx_frames": report["rx_packets"], "tap_tx_frames": report["tx_packets"],
        "tap_avg_rx_frame": round(report["received_bytes"] / max(1, report["rx_packets"])),
        "host_sockets_seen": host["sockets_seen"],
        "host_payload_received": host["bytes_received"], "host_payload_sent": host["bytes_sent"],
        "host_segs_in": host["segs_in"], "host_segs_out": host["segs_out"],
        "host_bytes_retrans": host["bytes_retrans"], "host_retrans_total": host["retrans_total"],
        "host_mss": [s.get("mss") for s in sock], "host_timestamps": [s.get("timestamps") for s in sock],
        "host_wire_estimate_received": host["wire_estimate_received"],
        "host_wire_estimate_sent": host["wire_estimate_sent"],
        "tap_minus_host_payload_received": report["received_bytes"] - host["bytes_received"],
        "tap_minus_host_payload_sent": report["sent_bytes"] - host["bytes_sent"],
        "tap_over_host_wire_estimate_received": round(report["received_bytes"] / max(1, host["wire_estimate_received"]), 4),
        "tap_over_host_wire_estimate_sent": round(report["sent_bytes"] / max(1, host["wire_estimate_sent"]), 4),
        "host_nic_whole_host_delta": {"iface": nic0["iface"],
                                      "rx_bytes": nic1["rx_bytes"] - nic0["rx_bytes"],
                                      "tx_bytes": nic1["tx_bytes"] - nic0["tx_bytes"]},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--down", type=int, default=20_000_000)
    parser.add_argument("--up", type=int, default=5_000_000)
    parser.add_argument("--loss", type=float, default=3.0)
    parser.add_argument("--client", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.client:
        client(args.down, args.up)
        return 0
    for loss in (0.0, args.loss):
        print(json.dumps(run(args.down, args.up, loss), indent=1), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
