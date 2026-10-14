"""The shared Codex app-server memory budget (live/control/budgets.yaml), checked
over recorded telemetry of its workload.

Each test runs the budget's check step — `onebudgetspec check` over the file,
which runs `app_server_rss_budget.py` exactly as scripts/e2e-control.sh does —
over a fixture of the sustained-reuse workload's `reuse.json`
(live/control/fixtures/). The gate never checks the budget against real
telemetry; the live-control leg does.
"""

from __future__ import annotations

import os
import subprocess
import unittest
from pathlib import Path

from support import ROOT, scratch

CONTROL = ROOT / "live" / "control"
FIXTURES = CONTROL / "fixtures"


def check(reuse: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "uv",
            "run",
            "--project",
            str(ROOT / "python"),
            "--frozen",
            "--no-sync",
            "onebudgetspec",
            "check",
            "budgets.yaml",
        ],
        cwd=CONTROL,
        env={
            **{k: v for k, v in os.environ.items() if k != "ONEBUDGETSPEC_RESULT"},
            "OH_E2E_REUSE_FILE": str(reuse),
        },
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )


class RssGrowthBudget(unittest.TestCase):
    def test_exactly_one_and_a_half_times_the_first_reading_is_within(self) -> None:
        run = check(FIXTURES / "reuse-at-threshold.json")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("actual 1.5 ratio, budget 1.5 ratio", run.stdout)
        self.assertIn("within", run.stdout)

    def test_one_byte_more_is_over_and_names_both_readings(self) -> None:
        run = check(FIXTURES / "reuse-one-byte-over.json")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn("— over", run.stdout)
        self.assertIn("300000001 B", run.stdout)
        self.assertIn("200000000 B", run.stdout)

    def test_incomplete_telemetry_is_an_error_naming_what_is_missing(self) -> None:
        tmp = scratch(self)
        for fixture, named in (
            (tmp / "absent.json", "no reuse telemetry at"),
            (FIXTURES / "reuse-not-linux.json", "measured on linux only"),
            (FIXTURES / "reuse-four-leases.json", "4 leases were released"),
            (FIXTURES / "reuse-two-servers.json", "2 server pids"),
            (FIXTURES / "reuse-missing-reading.json", "the after_first reading is missing"),
            (FIXTURES / "reuse-zero-reading.json", "after_last reading's rss_bytes is 0"),
        ):
            with self.subTest(fixture=fixture.name):
                run = check(fixture)
                self.assertNotEqual(run.returncode, 0, run.stdout)
                self.assertIn("error", run.stdout)
                self.assertIn(named, run.stdout + run.stderr)


if __name__ == "__main__":
    unittest.main()
