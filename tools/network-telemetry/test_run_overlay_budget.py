"""The granted-Codex-turn budget (live/control/budgets.yaml's
`codex-run-overlay-added-seconds`), checked over recorded telemetry of its
workload.

Each test runs the budget's check step — `onebudgetspec check --id` over the
file, which runs `run_overlay_budget.py` exactly as scripts/e2e-control.sh's
run-overlay phase does — over a fixture of the workload's `codex-turn.json`
(live/control/fixtures/). `run-overlay-measured.json` is the record the
installed codex-cli 0.159.2 produced on this change's host. The gate never
checks the budget against real telemetry; the live-control leg does.
"""

from __future__ import annotations

import json
import os
import subprocess
import unittest
from pathlib import Path

from run_overlay_telemetry import (
    BATCHES,
    OverlayTimings,
    Telemetry,
    TurnTiming,
    run_concurrently,
)
from support import ROOT, scratch

CONTROL = ROOT / "live" / "control"
FIXTURES = CONTROL / "fixtures"


def check(telemetry: Path) -> subprocess.CompletedProcess[str]:
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
            "--id",
            "codex-run-overlay-added-seconds",
        ],
        cwd=CONTROL,
        env={
            **{k: v for k, v in os.environ.items() if k != "ONEBUDGETSPEC_RESULT"},
            "OH_E2E_RUN_OVERLAY_FILE": str(telemetry),
        },
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )


class RunOverlayAddedSecondsBudget(unittest.TestCase):
    def test_the_measured_workload_is_within_and_reports_every_figure(self) -> None:
        run = check(FIXTURES / "run-overlay-measured.json")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("within", run.stdout)
        for named in (
            "ungranted slowest 1:",
            "added for 1 granted turn",
            "for 20 concurrent",
            "overlay build",
            "server start",
            "teardown",
            "load average",
            "codex-cli 0.159.2",
        ):
            self.assertIn(named, run.stdout)

    def test_exactly_five_added_seconds_is_within(self) -> None:
        run = check(FIXTURES / "run-overlay-at-threshold.json")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("actual 5 seconds, budget 5 seconds", run.stdout)

    def test_more_than_five_is_over(self) -> None:
        run = check(FIXTURES / "run-overlay-over.json")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertRegex(run.stdout, r" over; ")

    def test_incomplete_telemetry_is_an_error_naming_what_is_missing(self) -> None:
        tmp = scratch(self)
        for fixture, named in (
            (tmp / "absent.json", "no run-overlay telemetry at"),
            (FIXTURES / "run-overlay-not-linux.json", "the budget is measured on linux"),
            (FIXTURES / "run-overlay-short-batch.json", "the granted batch of 20 holds 19 turns"),
            (FIXTURES / "run-overlay-no-overlay.json", "carries no overlay_timings"),
            (FIXTURES / "run-overlay-zero-seconds.json", "not a positive duration"),
            (FIXTURES / "run-overlay-disagreeing-seconds.json", "disagree with its start"),
            (FIXTURES / "run-overlay-no-version.json", "the record names no codex version"),
            (FIXTURES / "run-overlay-not-json.json", "is not JSON"),
            (FIXTURES / "run-overlay-not-an-object.json", "holds no JSON object"),
        ):
            with self.subTest(fixture=fixture.name):
                run = check(fixture)
                self.assertNotEqual(run.returncode, 0, run.stdout)
                self.assertIn("error", run.stdout)
                self.assertIn(named, run.stdout + run.stderr)


class RunOverlayTelemetryContract(unittest.TestCase):
    """The writer's record is what the reader reads: one shape, held by both
    sides through `run_overlay_telemetry`."""

    def test_the_collector_times_each_process_to_its_own_exit(self) -> None:
        # A 1 s process collected first must not lengthen a 0.1 s one beside it.
        slow, fast = run_concurrently(
            [["sleep", "1"], ["sleep", "0.1"]], dict(os.environ), timeout=30
        )
        self.assertGreaterEqual(slow.ended - slow.started, 0.9)
        self.assertLess(fast.ended - fast.started, 0.6)
        self.assertEqual((slow.returncode, fast.returncode), (0, 0))

    def test_a_record_the_writer_builds_is_one_the_budget_reads(self) -> None:
        timed = run_concurrently([["sleep", "0.05"]], dict(os.environ), timeout=30)[0]

        def turn(extra: float, overlay: bool) -> TurnTiming:
            return TurnTiming(
                timed.ended - timed.started + extra,
                timed.started,
                timed.ended + extra,
                timed.started + 0.01,
                OverlayTimings(120, 90) if overlay else None,
            )

        telemetry = Telemetry(
            "codex-cli 0.159.2",
            "linux",
            (1.0, 1.0, 1.0),
            {size: [turn(0.0, False) for _ in range(size)] for size in BATCHES},
            {size: [turn(0.25, True) for _ in range(size)] for size in BATCHES},
        )
        path = scratch(self) / "codex-turn.json"
        path.write_text(json.dumps(telemetry.to_json()), encoding="utf-8")
        run = check(path)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("actual 0.25 seconds", run.stdout)


if __name__ == "__main__":
    unittest.main()
