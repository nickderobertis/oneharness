#!/usr/bin/env python3
"""Hermetic tests of `scripts/codex-network-breakdown.py`.

    python3 -I tools/e2e-support/codex-network-breakdown/test_codex_network_breakdown.py

The reporting tests read a measurement and a Codex `RUST_LOG` trace recorded
from a real Codex launch and committed beside this file, so they need no
launch. The measuring test runs the script around `transfer_double.py`, which
moves payloads of known sizes through the proxy variables to two loopback
servers standing for two destinations. It needs Linux with unprivileged user
namespaces and pasta; where the namespace cannot be built it says so and
skips, unless `CODEX_NETWORK_BREAKDOWN_REQUIRED=1` makes that a failure.
"""

import json
import os
import socket
import subprocess
import sys
import threading
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SCRIPT = ROOT / "scripts" / "codex-network-breakdown.py"
MEASUREMENT = HERE / "recorded-measurement.json"
TRACE = HERE / "recorded-codex-trace.log"
# A launch still running after this fails the test rather than hanging it.
HANG_GUARD_SECONDS = 120


def breakdown(*args: str) -> dict:
    run = subprocess.run(
        [sys.executable, "-I", str(SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=HANG_GUARD_SECONDS,
    )
    if run.returncode != 0:
        raise AssertionError(f"exit {run.returncode}\n{run.stdout}\n{run.stderr}")
    return json.loads(run.stdout)


class ReportFromRecordedFixtures(unittest.TestCase):
    """The printed document, assembled from a recorded measurement and trace."""

    def test_each_tunnel_carries_its_destination_bytes_times_and_traced_paths(self):
        recorded = json.loads(MEASUREMENT.read_text())
        report = breakdown("report", str(MEASUREMENT), "--trace", str(TRACE))
        self.assertTrue(report["traced"])
        self.assertEqual(report["received_bytes"], recorded["received_bytes"])
        self.assertEqual(report["sent_bytes"], recorded["sent_bytes"])
        self.assertEqual(len(report["tunnels"]), len(recorded["tunnels"]))
        for tunnel in report["tunnels"]:
            self.assertEqual(tunnel["destination"], f"{tunnel['host']}:{tunnel['port']}")
            for field in ("received_bytes", "sent_bytes", "opened_at", "closed_at"):
                self.assertIsInstance(tunnel[field], (int, float), field)
            self.assertLessEqual(tunnel["opened_at"], tunnel["closed_at"])
            self.assertIsInstance(tunnel["paths"], list)
        self.assertEqual(
            report["tunnel_received_bytes"] + report["unaccounted_received_bytes"],
            report["received_bytes"],
        )
        expected = json.loads((HERE / "expected-paths.json").read_text())
        self.assertEqual(
            [[tunnel["destination"], tunnel["paths"]] for tunnel in report["tunnels"]],
            expected,
        )
        # The trace names nothing for at least one tunnel, which gets no path.
        self.assertIn([], [tunnel["paths"] for tunnel in report["tunnels"]])
        self.assertTrue(any(tunnel["paths"] for tunnel in report["tunnels"]))

    def test_without_a_trace_no_tunnel_has_a_path(self):
        report = breakdown("report", str(MEASUREMENT))
        self.assertFalse(report["traced"])
        self.assertTrue(report["tunnels"])
        self.assertEqual({len(tunnel["paths"]) for tunnel in report["tunnels"]}, {0})


def serve(listener: socket.socket) -> None:
    """`GET n` answers n bytes; `PUT n` reads n bytes and answers `OK`."""
    while True:
        try:
            conn, _ = listener.accept()
        except OSError:
            return
        threading.Thread(target=answer, args=(conn,), daemon=True).start()


def answer(conn: socket.socket) -> None:
    with conn:
        line = b""
        while not line.endswith(b"\n"):
            data = conn.recv(1)
            if not data:
                return
            line += data
        verb, size = line.split()
        size = int(size)
        if verb == b"GET":
            block = os.urandom(65536)
            sent = 0
            while sent < size:
                piece = block[: min(len(block), size - sent)]
                conn.sendall(piece)
                sent += len(piece)
        else:
            got = 0
            while got < size:
                data = conn.recv(65536)
                if not data:
                    return
                got += len(data)
            conn.sendall(b"OK\n")


def loopback_server() -> int:
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    threading.Thread(target=serve, args=(listener,), daemon=True).start()
    return listener.getsockname()[1]


def namespace_unavailable() -> str:
    """Why the measuring namespace cannot be built here, or "" when it can."""
    if not sys.platform.startswith("linux"):
        return f"the measuring namespace is Linux-only, and this is {sys.platform}"
    run = subprocess.run(
        [sys.executable, "-I", str(SCRIPT), "--label", "probe", "--", "true"],
        capture_output=True,
        text=True,
        timeout=HANG_GUARD_SECONDS,
    )
    if run.returncode != 0:
        return f"the measuring namespace could not be built: {run.stdout.strip()} {run.stderr.strip()}"
    return ""


class MeasureAroundATransferDouble(unittest.TestCase):
    """Each tunnel's bytes against payloads of known sizes."""

    DOWN_FIRST = 3_000_000
    DOWN_SECOND = 1_500_000
    UP = 2_000_000

    def setUp(self):
        why = namespace_unavailable()
        if why:
            if os.environ.get("CODEX_NETWORK_BREAKDOWN_REQUIRED") == "1":
                self.fail(why)
            self.skipTest(why)

    def test_each_tunnel_carries_its_destination_and_payload_and_the_tunnels_account_for_the_namespace(self):
        first, second = loopback_server(), loopback_server()
        report = breakdown(
            "--label",
            "transfer-double",
            "--",
            sys.executable,
            "-I",
            str(HERE / "transfer_double.py"),
            f"{first}:down:{self.DOWN_FIRST}",
            f"{second}:down:{self.DOWN_SECOND}",
            f"{second}:up:{self.UP}",
        )
        self.assertEqual(report["launch_exit"], 0, report)
        tunnels = report["tunnels"]
        self.assertEqual(len(tunnels), 3, tunnels)
        payloads = [
            (first, "received_bytes", self.DOWN_FIRST),
            (second, "received_bytes", self.DOWN_SECOND),
            (second, "sent_bytes", self.UP),
        ]
        for tunnel, (port, direction, payload) in zip(tunnels, payloads):
            self.assertEqual(tunnel["port"], port, tunnel)
            self.assertEqual(tunnel["destination"], f"{tunnel['host']}:{port}", tunnel)
            self.assertGreaterEqual(tunnel[direction], payload, tunnel)
            self.assertLessEqual(tunnel[direction], payload * 1.01, tunnel)
            other = "sent_bytes" if direction == "received_bytes" else "received_bytes"
            self.assertLessEqual(tunnel[other], payload * 0.01, tunnel)
            self.assertLess(tunnel["opened_at"], tunnel["closed_at"], tunnel)
        self.assertEqual(len({tunnel["destination"] for tunnel in tunnels}), 2, tunnels)
        for side in ("received", "sent"):
            namespace = report[f"{side}_bytes"]
            accounted = report[f"tunnel_{side}_bytes"]
            self.assertGreaterEqual(accounted, 0.9 * namespace, report)
            self.assertLessEqual(accounted, namespace, report)


if __name__ == "__main__":
    unittest.main()
