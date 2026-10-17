"""The independent observer the recorder's self-check is judged against.

Its verdict must not depend on how much CPU the observer gets: the self-check
reads `drops == 0` as "the capture can vouch for the counters", so a reader the
tests beside it starve must still see every frame. Driven in an unprivileged
user and network namespace, on loopback, with the observer frozen (SIGSTOP)
for the whole transfer — the worst a loaded runner can do to it. Needs Linux
and unprivileged user namespaces; skips saying why elsewhere, unless
OH_NET_NO_SKIP=1 (CI's Linux leg) makes that a failure.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import threading
import unittest

from support import OBSERVER, scratch

PAYLOAD = 8_000_000


def namespace_unavailable() -> str:
    if not sys.platform.startswith("linux"):
        return "the observer captures with AF_PACKET, which only Linux has"
    for tool in ("unshare", "ip"):
        if shutil.which(tool) is None:
            return f"`{tool}` is not on PATH"
    probe = subprocess.run(["unshare", "-Urn", "true"], capture_output=True, text=True)
    if probe.returncode != 0:
        return f"unprivileged user namespaces are unavailable: {probe.stderr.strip()}"
    return ""


def drive(observer: str, out: str) -> None:
    """Inside the namespace: start the observer on loopback, freeze it, move
    PAYLOAD bytes over TCP, then thaw and stop it so it writes `out`."""
    subprocess.run(["ip", "link", "set", "lo", "up"], check=True)
    watcher = subprocess.Popen([sys.executable, "-I", observer, out, "lo"], stdout=subprocess.PIPE)
    assert watcher.stdout is not None
    if watcher.stdout.readline().strip() != b"ready":
        sys.exit(f"the observer exited {watcher.wait()} without reporting ready")
    os.kill(watcher.pid, signal.SIGSTOP)
    listener = socket.create_server(("127.0.0.1", 0))
    received = 0

    def receive() -> None:
        nonlocal received
        conn, _ = listener.accept()
        with conn:
            while data := conn.recv(1 << 16):
                received += len(data)

    reader = threading.Thread(target=receive)
    reader.start()
    with socket.create_connection(listener.getsockname()) as sender:
        sender.sendall(b"x" * PAYLOAD)
    reader.join()
    listener.close()
    if received != PAYLOAD:
        sys.exit(f"the transfer moved {received} of {PAYLOAD} bytes")
    os.kill(watcher.pid, signal.SIGCONT)
    watcher.terminate()
    watcher.wait()


class Observer(unittest.TestCase):
    def test_a_reader_frozen_for_the_whole_transfer_still_drops_no_frame(self) -> None:
        why = namespace_unavailable()
        if why:
            if os.environ.get("OH_NET_NO_SKIP") == "1" and sys.platform.startswith("linux"):
                self.fail(f"the observer cannot be driven here and OH_NET_NO_SKIP=1: {why}")
            self.skipTest(why)
        tmp = scratch(self)
        out = tmp / "capture.json"
        run = subprocess.run(
            ["unshare", "-Urn", sys.executable, __file__, str(OBSERVER), str(out)],
            capture_output=True,
            text=True,
            timeout=120,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        captured = json.loads(out.read_text())
        self.assertEqual(captured["drops"], 0, "a reader that fell behind lost frames")
        self.assertEqual(captured["truncated"], 0)
        # Loopback shows every frame twice (sent, then received), headers in.
        seen = sum(frame["length"] for frame in captured["frames"])
        self.assertGreater(seen, 2 * PAYLOAD, "the capture holds less than the transfer")
        self.assertEqual(captured["packets_seen"], len(captured["frames"]))


if __name__ == "__main__":
    drive(sys.argv[1], sys.argv[2])
