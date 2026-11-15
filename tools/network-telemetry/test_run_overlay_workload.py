"""The run-overlay budget's workload writer (`run_allowlist_live.py
--budget-telemetry`, which scripts/e2e-control.sh's run-overlay phase runs)
times only the turn the budget describes.

Each test runs the writer for real, with its own scripted model endpoint,
over budget_turn_double.py standing in for the installed `codex` and the
`oneharness` binary: the double's turns follow the endpoint's script, and
BUDGET_DOUBLE_FAULT bends one side of them. A whole workload of complete
turns is recorded; a turn that exits non-zero, reports anything but `ok`,
returns no result for its call, runs the granted command other than exactly
once on an overlay, or runs it ungranted, stops the writer naming that, and
nothing is recorded. The live-control leg runs the same writer over the real
codex and oneharness.
"""

from __future__ import annotations

import json
import os
import subprocess
import unittest

from run_overlay_telemetry import BATCHES, decode
from support import HERE, ROOT, WINDOWS, scratch

WRITER = ROOT / "live" / "control" / "run_allowlist_live.py"
DOUBLE = HERE / "budget_turn_double.py"


@unittest.skipIf(WINDOWS, "the workload's turns run under bash, on the live-control leg's Linux")
class RunOverlayWorkloadWriter(unittest.TestCase):
    def workload(self, fault: str) -> tuple[subprocess.CompletedProcess[str], object]:
        root = scratch(self)
        bin_dir = root / "bin"
        bin_dir.mkdir()
        for name in ("codex", "oneharness"):
            (bin_dir / name).symlink_to(DOUBLE)
        out = root / "telemetry" / "codex-turn.json"
        world = root / "world"
        done = subprocess.run(
            # The writer's own interpreter floor, as scripts/e2e-control.sh runs it.
            [
                "uv",
                "run",
                "--no-project",
                "--python",
                ">=3.10",
                "python",
                str(WRITER),
                "--budget-telemetry",
                str(out),
                "--scratch",
                str(world),
            ],
            env={
                **os.environ,
                "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                "ONEHARNESS_BIN": str(bin_dir / "oneharness"),
                "BUDGET_DOUBLE_FAULT": fault,
            },
            capture_output=True,
            text=True,
            timeout=600,
            check=False,
        )
        return done, (json.loads(out.read_text()) if out.exists() else None)

    def test_a_workload_of_complete_turns_is_recorded(self) -> None:
        done, record = self.workload("")
        self.assertEqual(done.returncode, 0, done.stderr)
        telemetry = decode(record)
        for size in BATCHES:
            self.assertEqual(len(telemetry.granted[size]), size)
            self.assertEqual(len(telemetry.ungranted[size]), size)
            self.assertTrue(all(t.overlay is not None for t in telemetry.granted[size]))
            self.assertTrue(all(t.overlay is None for t in telemetry.ungranted[size]))

    def test_a_turn_that_is_not_the_budget_s_stops_the_writer_recording_nothing(self) -> None:
        for fault, says in (
            ("exit", "a granted budget turn exited 1"),
            ("status", "a granted budget turn ended nonzero"),
            ("no-decision", "a granted budget turn recorded no decision"),
            ("not-run", "a granted budget turn ran the granted command 0 time(s), on an overlay"),
            (
                "no-overlay",
                "a granted budget turn ran the granted command 1 time(s), on no overlay",
            ),
            (
                "ungranted-ran",
                "an ungranted budget turn ran the granted command 1 time(s) without the grant",
            ),
        ):
            with self.subTest(fault=fault):
                done, record = self.workload(fault)
                self.assertEqual(done.returncode, 1, done.stderr)
                self.assertIn(f"the budget workload stopped: {says}", done.stderr)
                self.assertIsNone(record)


if __name__ == "__main__":
    unittest.main()
