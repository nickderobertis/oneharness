"""scripts/e2e-net-readings.sh, the live workflows' network-recorder summary,
read over telemetry files the real shim wrote."""

from __future__ import annotations

import json
import re
import subprocess
import unittest
from pathlib import Path

from support import (
    BASH,
    ROOT,
    SCRIPTS,
    WINDOWS,
    lines,
    need_recorder,
    path_without,
    run_shim,
    scratch,
)

READINGS = SCRIPTS / "e2e-net-readings.sh"
# The live workflows that summarize their suite's readings, by suite.
WORKFLOWS = ("claude", "opencode", "schema")


def readings(file: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [BASH, str(READINGS), str(file)],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )


@unittest.skipIf(WINDOWS, "the live workflows summarize readings on their Linux leg only")
class Readings(unittest.TestCase):
    def harness(self, tmp: Path) -> str:
        """A harness double that exits 0."""
        double = tmp / "harness"
        double.write_text("#!/usr/bin/env bash\nexit 0\n")
        double.chmod(0o755)
        return str(double)

    def unmeasured(self, tmp: Path, file: Path, count: int) -> None:
        """COUNT launches the shim runs directly, on a host with no pasta."""
        env = {"PATH": path_without(tmp, "pasta")}
        harness = self.harness(tmp)
        for _ in range(count):
            run = run_shim(tmp, [harness], ["exec", "x"], file=file, env=env)
            self.assertEqual(run.returncode, 0, run.stderr)

    def test_measured_and_unmeasured_launches_are_counted_by_method_and_reason(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        file = tmp / "claude.jsonl"
        for _ in range(2):
            run = run_shim(tmp, [self.harness(tmp)], ["exec", "x"], file=file)
            self.assertEqual(run.returncode, 0, run.stderr)
        self.unmeasured(tmp, file, 1)
        (reason,) = [r["reason"] for r in lines(file) if r.get("method") == "unavailable"]
        run = readings(file)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(
            run.stdout.splitlines(),
            [f"network recorder: 2 netns-pasta; 1 unavailable: {reason}"],
        )

    def test_an_unmeasured_launch_is_read_with_the_reason_its_line_gives(self) -> None:
        tmp = scratch(self)
        file = tmp / "schema.jsonl"
        self.unmeasured(tmp, file, 2)
        records = lines(file)
        # Each launch's start line is not a reading; only its measurement is.
        self.assertEqual(len(records), 4, records)
        (reason,) = {r["reason"] for r in records if r.get("method") == "unavailable"}
        self.assertIn("pasta", reason)
        run = readings(file)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout.splitlines(), [f"network recorder: 2 unavailable: {reason}"])

    def test_a_suite_that_recorded_no_launch_says_so_and_passes(self) -> None:
        tmp = scratch(self)
        run = readings(tmp / "opencode.jsonl")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(
            run.stdout.strip(),
            f"network recorder: no telemetry file at {tmp / 'opencode.jsonl'}; "
            "the suite recorded no launch",
        )

    def test_start_lines_without_a_measurement_say_none_was_measured(self) -> None:
        tmp = scratch(self)
        file = tmp / "claude.jsonl"
        file.write_text(json.dumps({"launch_id": "a", "argv": ["exec"]}) + "\n")
        run = readings(file)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(
            run.stdout.strip(), "network recorder: none of its launches has a measurement line"
        )

    def test_a_line_that_is_not_json_fails_naming_the_file(self) -> None:
        tmp = scratch(self)
        file = tmp / "claude.jsonl"
        file.write_text('{"launch_id": "a", "method": "netns-pasta"}\n{"cut\n')
        run = readings(file)
        self.assertEqual(run.returncode, 1, run.stdout)
        self.assertIn(f"e2e-net-readings: cannot read {file} (", run.stderr)
        self.assertIn("inspect it for another writer", run.stderr)
        self.assertEqual(run.stdout, "")

    def test_a_reason_s_control_characters_cannot_forge_another_reading(self) -> None:
        tmp = scratch(self)
        file = tmp / "claude.jsonl"
        record = {"launch_id": "a", "method": "unavailable", "reason": "no pasta\n9 netns-pasta\t"}
        file.write_text(json.dumps(record) + "\n")
        run = readings(file)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(
            run.stdout.splitlines(), ["network recorder: 1 unavailable: no pasta 9 netns-pasta "]
        )

    def test_a_method_the_recorder_never_writes_or_a_reason_not_a_string_fails(self) -> None:
        for record, said in (
            ({"launch_id": "a", "method": 7}, "method is neither netns-pasta nor unavailable"),
            (
                {"launch_id": "a", "method": "netns-pasta\n9 unavailable"},
                "method is neither netns-pasta nor unavailable",
            ),
            (
                {"launch_id": "a", "method": "unavailable", "reason": ["no", "pasta"]},
                "reason is not a string",
            ),
            (
                {"launch_id": "a", "method": "unavailable", "reason": False},
                "reason is not a string",
            ),
        ):
            with self.subTest(record=record):
                tmp = scratch(self)
                file = tmp / "claude.jsonl"
                file.write_text(json.dumps(record) + "\n")
                run = readings(file)
                self.assertEqual(run.returncode, 1, run.stdout)
                self.assertIn(f"a measurement line whose {said}", run.stderr)
                self.assertEqual(run.stdout, "")

    def test_each_live_workflow_summarizes_its_own_suite_s_file_after_it_runs(self) -> None:
        for suite in WORKFLOWS:
            with self.subTest(suite=suite):
                text = (ROOT / ".github" / "workflows" / f"e2e-{suite}.yml").read_text()
                step = re.search(r"- name: Network recorder readings\n((?:        .*\n)+)", text)
                self.assertIsNotNone(step, f"e2e-{suite}.yml has no readings step")
                assert step is not None
                body = step.group(1)
                self.assertIn("if: always() && runner.os == 'Linux'", body)
                self.assertIn(
                    f"run: scripts/e2e-net-readings.sh target/e2e-telemetry/network/{suite}.jsonl",
                    body,
                )
