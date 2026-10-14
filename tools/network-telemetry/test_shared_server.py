"""A shared Codex app-server in the live tier's network telemetry.

A pooled `codex app-server --listen` serves every concurrent dispatch with its
key, so it is one launch over its whole life — startup to linger — with one
`start` line of kind `shared-app-server` and one measurement line, and no
dispatch's line counting its bytes. The suites wait for every such server to
exit before their budget check, since its line lands only after its linger.

The real oneharness binary drives the real shim, recorder and budget command;
the mock harness stands in for Codex and the tests' loopback server for the
internet.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
import unittest
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, NewType

from support import (
    BASH,
    MOCK,
    NETSHIM,
    ONEHARNESS,
    ROOT,
    Record,
    budget,
    lines,
    measurements,
    need_builds,
    need_recorder,
    scratch,
    server,
    starts,
    suite_env,
    write_lines,
)

PAYLOAD = 3_000_000

Pid = NewType("Pid", int)
"""A server process id a dispatch's report names."""


def shim_for(tmp: Path, real: Path, file: Path) -> Path:
    """A `codex` measuring shim, written as oh_telemetry_setup writes one."""
    shim = tmp / "shims" / "codex"
    shim.write_text(
        "#!/usr/bin/env bash\n# oneharness-network-shim suite=codex\n"
        f'exec bash "{NETSHIM}" --suite codex --harness codex --real "{real}" '
        f'--self "{shim}" --file "{file}" -- "$@"\n'
    )
    shim.chmod(0o755)
    return shim


@dataclass(frozen=True)
class MockState:
    """What one mock app-server's state file says, as far as these tests read it."""

    live_turns: int

    @staticmethod
    def read(path: Path) -> MockState | None:
        """The state at `path`, or None while it is mid-write or unreadable."""
        try:
            loaded = json.loads(path.read_text())["loaded"]
        except (OSError, ValueError, KeyError):
            return None
        if not isinstance(loaded, dict):
            return None
        return MockState(sum(1 for t in loaded.values() if t.get("active_turn")))


@dataclass(frozen=True)
class DispatchReport:
    """One pooled dispatch's run report, as far as these tests read it."""

    server_pid: Pid

    @staticmethod
    def parse(stdout: bytes) -> DispatchReport:
        return DispatchReport(Pid(int(json.loads(stdout)["control"]["app_server"]["pid"])))


def live_turns(mock_dir: Path) -> int:
    states = [MockState.read(path) for path in mock_dir.glob("state-*.json")]
    return sum(state.live_turns for state in states if state is not None)


def wait_for(label: str, limit: float, condition: Callable[[], bool]) -> None:
    deadline = time.monotonic() + limit
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(0.05)
    raise AssertionError(f"timed out after {limit} s waiting for {label}")


class OneLaunchOverItsLife(unittest.TestCase):
    def test_a_shared_server_is_one_measured_launch_whose_bytes_no_dispatch_counts(self) -> None:
        need_builds(self)
        need_recorder(self)
        tmp = scratch(self)
        serving = server(self)
        telemetry = tmp / "telemetry"
        telemetry.mkdir()
        file = telemetry / "codex.jsonl"
        shim = shim_for(tmp, MOCK, file)
        release = tmp / "release"
        env = {
            **{k: v for k, v in os.environ.items() if not k.startswith("ONEHARNESS_")},
            "ONEHARNESS_NO_CONFIG": "1",
            "XDG_STATE_HOME": str(tmp / "state"),
            "CODEX_HOME": str(tmp / "codex-home"),
            "CODEX_MOCK_DIR": str(tmp / "mock"),
            "CODEX_MOCK_STARTUP_FETCH": f"{serving.port} {PAYLOAD}",
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS": "1000",
            "MOCK_CODEX_HOLD_UNTIL": str(release),
        }
        (tmp / "codex-home").mkdir()

        def dispatch(session: str) -> subprocess.Popen[bytes]:
            return subprocess.Popen(
                [
                    str(ONEHARNESS),
                    "run",
                    "--harness",
                    "codex",
                    "--control",
                    "--session",
                    session,
                    "--session-dir",
                    str(tmp / "s"),
                    "--prompt",
                    "hello",
                    "--mode",
                    "bypass",
                    "--bin",
                    f"codex={shim}",
                    "--format",
                    "json",
                ],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        first = dispatch("one")
        wait_for("the first turn", 60, lambda: live_turns(tmp / "mock") == 1)
        second = dispatch("two")
        wait_for("both turns live on the server", 60, lambda: live_turns(tmp / "mock") == 2)
        release.touch()
        reports: list[DispatchReport] = []
        for child in (first, second):
            out, err = child.communicate(timeout=120)
            self.assertEqual(child.returncode, 0, err.decode())
            reports.append(DispatchReport.parse(out))
        pids = {report.server_pid for report in reports}
        self.assertEqual(len(pids), 1, "the two dispatches did not share one server")

        def server_starts() -> list[Record]:
            return [s for s in starts(lines(file)) if s["kind"] == "shared-app-server"]

        wait_for(
            "the server's measurement line",
            30,
            lambda: len(server_starts()) == 1
            and len(measurements(lines(file), server_starts()[0]["launch_id"])) == 1,
        )
        records = lines(file)
        (launch,) = server_starts()
        (line,) = measurements(records, launch["launch_id"])
        self.assertEqual(line["method"], "netns-pasta", line)
        self.assertGreaterEqual(line["rx_bytes"], PAYLOAD, "the server's download was not counted")
        self.assertLess(line["rx_bytes"], 2 * PAYLOAD, "the server's download was counted twice")
        for other in starts(records):
            if other["launch_id"] == launch["launch_id"]:
                continue
            for quiet in measurements(records, other["launch_id"]):
                self.assertLess(quiet.get("rx_bytes", 0), PAYLOAD // 2, other)

        # The budget command counts it as one Codex launch …
        counted = budget(telemetry_dir=telemetry)
        self.assertEqual(counted.returncode, 0, counted.stdout + counted.stderr)
        self.assertEqual(counted.stdout.splitlines()[0], f"{line['rx_bytes']} B")
        self.assertIn("shared-app-server: 1 launch,", counted.stdout)
        # … and a record without its line names it.
        withheld = telemetry.parent / "withheld"
        withheld.mkdir()
        write_lines(withheld / "codex.jsonl", [r for r in records if r is not line])
        refused = budget(telemetry_dir=withheld)
        self.assertNotEqual(refused.returncode, 0, refused.stdout)
        self.assertIn(
            f"launch {launch['launch_id']} (phase unphased, kind shared-app-server) "
            "has no measurement line",
            refused.stdout + refused.stderr,
        )


class SuitesWaitForTheirServers(unittest.TestCase):
    def test_the_wait_returns_once_every_server_is_gone_and_measured(self) -> None:
        need_builds(self)
        need_recorder(self)
        tmp = scratch(self)
        (tmp / "home2").mkdir()
        script = r"""
oh_telemetry_setup codex
export ONEHARNESS_NO_CONFIG=1
dispatch() {
  "$ONEHARNESS_BIN" run --harness codex --control --session "$1" --session-dir "$PWD/s" \
    --prompt hello --mode bypass --format json >"$1.json"
}
ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS=200 dispatch quick
# The quick server lingers out (its linger and one sweep) before the other starts.
sleep 3
CODEX_HOME="$PWD/home2" ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS=4000 dispatch lingering
cp "$OH_E2E_TELEMETRY_FILE" before-the-wait.jsonl
started="$(date +%s%N)"
oh_wait_pooled_servers
echo "waited $(( ($(date +%s%N) - started) / 1000000 ))"
"""
        env = suite_env(tmp, "codex")
        run = subprocess.run(
            [
                BASH,
                "-c",
                f'set -euo pipefail; source "$1"; {script}',
                "_",
                str(ROOT / "scripts" / "e2e-lib.sh"),
            ],
            capture_output=True,
            text=True,
            cwd=tmp,
            env=env,
            timeout=240,
            check=False,
        )
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        file = tmp / "telemetry" / "codex.jsonl"
        records = lines(file)
        servers = [s for s in starts(records) if s["kind"] == "shared-app-server"]
        self.assertEqual(len(servers), 2, records)
        for launch in servers:
            (line,) = measurements(records, launch["launch_id"])
            self.assertEqual(line["method"], "netns-pasta", line)
            socket = launch["argv"][launch["argv"].index("--listen") + 1].removeprefix("unix://")
            entry = Path(socket).parent.parent
            self.assertFalse((entry / "server.json").exists(), f"{entry} still runs a server")
        waited = int(run.stdout.strip().splitlines()[-1].split()[1])
        self.assertGreater(waited, 2000, "the wait returned before the lingering server left")

        # Without the wait, the lingering server's launch has no line yet.
        before = lines(tmp / "before-the-wait.jsonl")
        missing = [
            s
            for s in starts(before)
            if s["kind"] == "shared-app-server" and not measurements(before, s["launch_id"])
        ]
        self.assertEqual(len(missing), 1, before)
        early = tmp / "early"
        early.mkdir()
        shutil.copy(tmp / "before-the-wait.jsonl", early / "codex.jsonl")
        refused = budget(telemetry_dir=early)
        self.assertNotEqual(refused.returncode, 0, refused.stdout)
        self.assertIn(
            f"launch {missing[0]['launch_id']} (phase unphased, kind shared-app-server) has no measurement line",
            refused.stdout + refused.stderr,
        )
        # With it, the check counts both servers.
        counted = budget("--total", telemetry_dir=tmp / "telemetry")
        self.assertEqual(counted.returncode, 0, counted.stdout + counted.stderr)
        self.assertIn("shared-app-server: 2 launches,", counted.stdout)


class TheWaitFailsLoudly(unittest.TestCase):
    """oh_wait_pooled_servers refuses what it cannot trust, and gives up past
    its bound, naming why each time."""

    def wait(
        self, tmp: Path, records: list[Record], limit: str
    ) -> subprocess.CompletedProcess[str]:
        file = tmp / "telemetry.jsonl"
        write_lines(file, records)
        env = {
            **suite_env(tmp),
            "OH_TEST_TELEMETRY": str(file),
            "OH_E2E_POOL_WAIT": limit,
        }
        return subprocess.run(
            [
                BASH,
                "-c",
                'set -euo pipefail; source "$1"; OH_E2E_TELEMETRY_FILE="$OH_TEST_TELEMETRY"; oh_wait_pooled_servers',
                "_",
                str(ROOT / "scripts" / "e2e-lib.sh"),
            ],
            capture_output=True,
            text=True,
            cwd=tmp,
            env=env,
            timeout=60,
            check=False,
        )

    def test_a_bad_bound_damaged_record_or_lingering_server_fails_the_wait(self) -> None:
        tmp = scratch(self)
        entry = tmp / "state" / "oneharness" / "servers" / "codex-app-server-0123"
        (entry / "sock").mkdir(parents=True)
        (entry / "server.json").write_text("{}")
        socket = f"unix://{entry / 'sock' / 'app-server.sock'}"

        def launch(launch_id: str, address: str) -> Record:
            return {
                "kind": "shared-app-server",
                "launch_id": launch_id,
                "argv": ["codex", "app-server", "--listen", address],
            }

        for name, records, limit, said in (
            (
                "bad bound",
                [],
                "soon",
                "OH_E2E_POOL_WAIT must be a whole number of seconds, got 'soon'",
            ),
            ("escaping id", [launch("../escape", socket)], "5", "launch id '../escape'"),
            (
                "relative socket",
                [launch("codex-1-ab", "unix://sock/app.sock")],
                "5",
                "is not an absolute unix:// path",
            ),
            (
                "lingering",
                [launch("codex-1-ab", socket)],
                "1",
                "still running or unmeasured after 1s",
            ),
        ):
            with self.subTest(name):
                started = time.monotonic()
                run = self.wait(tmp, records, limit)
                self.assertNotEqual(run.returncode, 0, run.stdout + run.stderr)
                self.assertIn(said, run.stderr)
                if name == "lingering":
                    self.assertGreaterEqual(time.monotonic() - started, 1.0)


if __name__ == "__main__":
    unittest.main()
