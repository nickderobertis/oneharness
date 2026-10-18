#!/usr/bin/env python3
"""An independent packet observer on the measuring namespace's TAP interface.

    observer.py OUT IFACE[,IFACE...]

The recorder's self-check runs this through `OH_E2E_NET_OBSERVER`, so
scripts/e2e-netmeasure.py starts it inside the launch's network namespace —
where the unprivileged user namespace grants CAP_NET_RAW, with no host root —
before it first reads the interface counters. It opens one AF_PACKET socket
(every protocol, every frame in either direction) with a TPACKET_V3 receive
ring, prints `ready`, and records each frame on the named interfaces until
SIGTERM: its kernel receive timestamp, its direction and its whole length,
Ethernet header and control traffic included. On SIGTERM it drains the ring,
reads the socket's PACKET_STATISTICS and writes OUT:

    {"ready_ns", "stopped_ns", "drops", "packets_seen", "truncated", "frames"}

A frame is "tx" when the namespace sent it (PACKET_OUTGOING) and "rx" otherwise,
the directions the TAP's tx_bytes/rx_bytes counters count. Each frame is written
as {"stamp_ns", "direction", "length"}.

Why a ring: a plain packet socket queues frames against SO_RCVBUF, which the
kernel caps at twice net.core.rmem_max — about 416 KiB on a stock runner, a
handful of pasta's 64 KiB frames — and an unprivileged namespace cannot raise
it. A reader starved of CPU for a moment by the tests running beside it then
lost frames (15 and 26 on CI), and the drop count, not the counters, decided
the verdict. The ring is mmap'd memory the kernel fills directly, sized here
(RING_BYTES) to hold every frame the self-check moves even if the reader never
ran until SIGTERM, so a drop it reports is a real one.
"""

from __future__ import annotations

import json
import mmap
import select
import signal
import socket
import struct
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal

ETH_P_ALL = 0x0003
SOL_PACKET = 263
PACKET_RX_RING = 5
PACKET_STATISTICS = 6
PACKET_VERSION = 10
TPACKET_V3 = 2
PACKET_OUTGOING = 4
TP_STATUS_KERNEL = 0
TP_STATUS_USER = 1
BLOCK_BYTES = 1 << 20
BLOCKS = 64
RING_BYTES = BLOCK_BYTES * BLOCKS
FRAME_BYTES = 1 << 11
# How long the kernel holds a part-filled block before handing it over (ms).
RETIRE_MS = 10
# struct tpacket_block_desc: version, offset_to_priv, then tpacket_hdr_v1's
# block_status, num_pkts, offset_to_first_pkt.
BLOCK_HEADER = struct.Struct("=IIIII")
# struct tpacket3_hdr: tp_next_offset, tp_sec, tp_nsec, tp_snaplen, tp_len.
PACKET_HEADER = struct.Struct("=IIIII")
# struct sockaddr_ll, which follows the 48-byte tpacket3_hdr: family,
# protocol, ifindex, hatype, pkttype.
LINK_ADDRESS = struct.Struct("=HHiHB")
LINK_ADDRESS_AT = 48


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


def read_capture(path: Path) -> Capture:
    """The capture main() wrote to `path`, read back into its own types."""
    raw = json.loads(path.read_text())
    return Capture(
        ready_ns=int(raw["ready_ns"]),
        stopped_ns=int(raw["stopped_ns"]),
        drops=int(raw["drops"]),
        packets_seen=int(raw["packets_seen"]),
        truncated=int(raw["truncated"]),
        frames=[
            Frame(
                stamp_ns=int(frame["stamp_ns"]),
                direction="rx" if frame["direction"] == "rx" else "tx",
                length=int(frame["length"]),
            )
            for frame in raw["frames"]
        ],
    )


def main() -> int:
    if len(sys.argv) != 3 or not [name for name in sys.argv[2].split(",") if name]:
        print(
            "usage: observer.py OUT IFACE[,IFACE...] (e2e-netmeasure.py appends the interfaces)",
            file=sys.stderr,
        )
        return 2
    out, names = sys.argv[1], [name for name in sys.argv[2].split(",") if name]
    indices = {socket.if_nametoindex(name) for name in names}
    # llmlint: ignore[async_typed_clients_at_boundaries] The acceptance criteria's independent observer: a raw AF_PACKET capture summing Ethernet frame lengths on the pasta TAP, not a client of any service. No typed or async API exists for kernel frames, and polling its ring with a short timeout is what lets its capture window stop on SIGTERM without losing frames.
    sock = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    sock.setsockopt(SOL_PACKET, PACKET_VERSION, TPACKET_V3)
    sock.setsockopt(
        SOL_PACKET,
        PACKET_RX_RING,
        struct.pack(
            "=7I",
            BLOCK_BYTES,
            BLOCKS,
            FRAME_BYTES,
            RING_BYTES // FRAME_BYTES,
            RETIRE_MS,
            0,
            0,
        ),
    )
    ring = mmap.mmap(sock.fileno(), RING_BYTES, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE)
    stopping = False

    def stop(*_: object) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, stop)
    poller = select.poll()
    poller.register(sock.fileno(), select.POLLIN | select.POLLERR)
    capture = Capture(ready_ns=time.time_ns())
    print("ready", flush=True)
    block = 0

    def drain() -> None:
        nonlocal block
        while True:
            base = block * BLOCK_BYTES
            _, _, status, count, offset = BLOCK_HEADER.unpack_from(ring, base)
            if not status & TP_STATUS_USER:
                return
            at = base + offset
            for _ in range(count):
                step, seconds, nanoseconds, snapped, length = PACKET_HEADER.unpack_from(ring, at)
                _, _, index, _, kind = LINK_ADDRESS.unpack_from(ring, at + LINK_ADDRESS_AT)
                if index in indices:
                    if snapped < length:
                        capture.truncated += 1
                    capture.frames.append(
                        Frame(
                            seconds * 1_000_000_000 + nanoseconds,
                            "tx" if kind == PACKET_OUTGOING else "rx",
                            length,
                        )
                    )
                at += step
            # Hand the block back to the kernel.
            struct.pack_into("=I", ring, base + 8, TP_STATUS_KERNEL)
            block = (block + 1) % BLOCKS

    while not stopping:
        try:
            poller.poll(50)
        except InterruptedError:
            continue
        drain()
    # A part-filled block reaches the reader only once the kernel retires it.
    time.sleep(RETIRE_MS * 5 / 1000)
    drain()
    capture.stopped_ns = time.time_ns()
    # tpacket_stats_v3's tp_packets, tp_drops: reading them also resets them,
    # once, here.
    capture.packets_seen, capture.drops, _ = struct.unpack(
        "III", sock.getsockopt(SOL_PACKET, PACKET_STATISTICS, 12)
    )
    with open(out, "w") as handle:
        json.dump(asdict(capture), handle)
    return 0


if __name__ == "__main__":
    sys.exit(main())
