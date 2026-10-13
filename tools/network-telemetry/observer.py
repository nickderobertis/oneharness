#!/usr/bin/env python3
"""An independent packet observer on the measuring namespace's TAP interface.

    observer.py OUT IFACE[,IFACE...]

The recorder's self-check runs this through `OH_E2E_NET_OBSERVER`, so
scripts/e2e-netmeasure.py starts it inside the launch's network namespace —
where the unprivileged user namespace grants CAP_NET_RAW, with no host root —
before it first reads the interface counters. It opens one AF_PACKET socket
(every protocol, every frame in either direction), prints `ready`, and records
each frame on the named interfaces until SIGTERM: its kernel receive timestamp,
its direction and its whole length, Ethernet header and control traffic
included. On SIGTERM it reads the socket's PACKET_STATISTICS and writes OUT:

    {"ready_ns", "stopped_ns", "drops", "packets_seen", "truncated", "frames"}

A frame is "tx" when the namespace sent it (PACKET_OUTGOING) and "rx" otherwise,
the directions the TAP's tx_bytes/rx_bytes counters count. Each frame is written
as {"stamp_ns", "direction", "length"}.
"""

from __future__ import annotations

import json
import signal
import socket
import struct
import sys
import time
from dataclasses import asdict, dataclass, field
from typing import Literal

ETH_P_ALL = 0x0003
SOL_PACKET = 263
PACKET_STATISTICS = 6
PACKET_OUTGOING = 4
SO_TIMESTAMPNS = 35
BUFFER = 1 << 18


@dataclass(frozen=True)
class Frame:
    stamp_ns: int
    direction: Literal["rx", "tx"]
    length: int


@dataclass
class Capture:
    ready_ns: int
    stopped_ns: int = 0
    drops: int = 0
    packets_seen: int = 0
    truncated: int = 0
    frames: list[Frame] = field(default_factory=list)


def main() -> int:
    if len(sys.argv) != 3 or not [name for name in sys.argv[2].split(",") if name]:
        print(
            "usage: observer.py OUT IFACE[,IFACE...] (e2e-netmeasure.py appends the interfaces)",
            file=sys.stderr,
        )
        return 2
    out, names = sys.argv[1], [name for name in sys.argv[2].split(",") if name]
    sock = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1 << 24)
    sock.setsockopt(socket.SOL_SOCKET, SO_TIMESTAMPNS, 1)
    stopping = False

    def stop(*_: object) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, stop)
    sock.settimeout(0.05)
    capture = Capture(ready_ns=time.time_ns())
    print("ready", flush=True)
    while not stopping:
        try:
            data, ancillary, flags, address = sock.recvmsg(BUFFER, 64)
        except socket.timeout:
            continue
        except InterruptedError:
            continue
        # An AF_PACKET address is (interface, protocol, packet type, ...).
        if address[0] not in names:
            continue
        stamp = time.time_ns()
        for level, kind, payload in ancillary:
            if level == socket.SOL_SOCKET and kind == SO_TIMESTAMPNS:
                seconds, nanoseconds = struct.unpack("qq", payload[:16])
                stamp = seconds * 1_000_000_000 + nanoseconds
        if flags & socket.MSG_TRUNC:
            capture.truncated += 1
        capture.frames.append(
            Frame(stamp, "tx" if address[2] == PACKET_OUTGOING else "rx", len(data))
        )
    capture.stopped_ns = time.time_ns()
    # tp_packets, tp_drops: reading them also resets them, once, here.
    capture.packets_seen, capture.drops = struct.unpack(
        "II", sock.getsockopt(SOL_PACKET, PACKET_STATISTICS, 8)
    )
    with open(out, "w") as handle:
        json.dump(asdict(capture), handle)
    return 0


if __name__ == "__main__":
    sys.exit(main())
