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

import dataclasses
import json
import os
import re
import subprocess
import unittest
from pathlib import Path

from run_overlay_telemetry import (
    ALLOWED_TOOLS_SETTING,
    BATCHES,
    RESULT_FIELDS,
    OverlayTimings,
    ReportResult,
    Telemetry,
    TurnTiming,
    number,
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
            (FIXTURES / "run-overlay-malformed-load.json", "is not three non-negative readings"),
            (
                FIXTURES / "run-overlay-request-outside-turn.json",
                "granted batch 1 turn 1's first model request",
            ),
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

    def test_a_process_past_the_timeout_is_killed_and_the_rest_still_count(self) -> None:
        stuck, quick = run_concurrently(
            [["sleep", "30"], ["sleep", "0.1"]], dict(os.environ), timeout=1
        )
        self.assertNotEqual(stuck.returncode, 0, "the stuck process was not killed")
        self.assertLess(stuck.ended - stuck.started, 10)
        self.assertEqual(quick.returncode, 0)
        self.assertLess(quick.ended - quick.started, 0.9)

    def test_overlay_timings_name_the_fields_the_report_schema_declares(self) -> None:
        # The drift gate between `domain::report::OverlayTimings` and the
        # Python that reads it: the schema is generated from the Rust type.
        schemas = json.loads(
            (
                ROOT
                / "python"
                / "oneharness-sdk"
                / "src"
                / "oneharness_sdk"
                / "_generated"
                / "schemas.json"
            ).read_text(encoding="utf-8")
        )
        found = []

        def walk(node: object) -> None:
            # llmlint: ignore[structural_pattern_matching] This test shares the Python SDK's 3.9 floor (tools/network-telemetry/ruff.toml extends it), where `match` is a syntax error; the isinstance/elif pair is the 3.9 spelling of the two shapes.
            if isinstance(node, dict):
                timings = node.get("OverlayTimings")
                if isinstance(timings, dict) and "properties" in timings:
                    found.append(timings)
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)

        walk(schemas)
        self.assertTrue(found, "the generated report schema declares no OverlayTimings")
        fields = {field.name for field in dataclasses.fields(OverlayTimings)}
        for timings in found:
            self.assertEqual(set(timings["properties"]), fields)
            self.assertEqual(set(timings.get("required", [])), fields)

    def test_the_report_fields_the_suites_read_are_the_report_schema_s(self) -> None:
        # The drift gate between `RunResult` (generated from the Rust report
        # type) and RESULT_FIELDS, the one table the live suites read a
        # result through: every field it names is one the schema declares,
        # kept in exactly the JSON types the schema gives it.
        defs = json.loads(
            (
                ROOT
                / "python"
                / "oneharness-sdk"
                / "src"
                / "oneharness_sdk"
                / "_generated"
                / "schemas.json"
            ).read_text(encoding="utf-8")
        )["run_report"]["$defs"]

        def types(node: dict[str, object]) -> set[object]:
            ref = node.get("$ref")
            if isinstance(ref, str):
                return types(defs[ref.rsplit("/", 1)[-1]])
            alternatives = node.get("anyOf", node.get("oneOf"))
            if isinstance(alternatives, list):
                return set().union(*(types(alt) for alt in alternatives))
            declared = node.get("type")
            return set(declared) if isinstance(declared, list) else {declared}

        result = defs["RunResult"]["properties"]
        self.assertEqual(
            {field.name for field in dataclasses.fields(ReportResult)}, set(RESULT_FIELDS)
        )
        for name, kept in RESULT_FIELDS.items():
            with self.subTest(field=name):
                self.assertIn(name, result, f"the report schema's RunResult declares no `{name}`")
                self.assertEqual(types(result[name]), set(kept))
        settings = {alt.get("const") for alt in defs["Setting"]["oneOf"]}
        self.assertIn(ALLOWED_TOOLS_SETTING, settings)

    def test_a_report_result_keeps_each_field_only_in_its_declared_type(self) -> None:
        read = ReportResult.from_report(
            {
                "results": [
                    {
                        "status": "ok",
                        "session_id": None,
                        "stderr": 7,
                        "diagnostics": [{"setting": ALLOWED_TOOLS_SETTING}, "not one"],
                        "overlay_env": {"CODEX_HOME": "/o/codex-home", "odd": 1},
                        "overlay_timings": {"build_ms": 3, "teardown_ms": 4},
                        "usage": [],
                    }
                ]
            }
        )
        self.assertEqual(read.status, "ok")
        self.assertIsNone(read.session_id)
        self.assertEqual(read.stderr, "")
        self.assertEqual(read.diagnostics, [{"setting": ALLOWED_TOOLS_SETTING}])
        self.assertEqual(read.overlay_env, {"CODEX_HOME": "/o/codex-home"})
        self.assertEqual(read.overlay_timings, OverlayTimings(3, 4))
        self.assertIsNone(read.usage)
        for report in ({}, {"results": []}, {"results": ["x"]}, []):
            with self.subTest(report=report):
                self.assertEqual(
                    ReportResult.from_report(report),
                    ReportResult(None, None, "", [], {}, None, None),
                )

    def test_the_live_suite_s_overlay_detector_restates_the_engine_s_layout(self) -> None:
        # live/control/run_allowlist_live.py finds overlays (and their
        # processes) by restating io::run_overlay's marker, directory prefix
        # and root path; it runs only in the paid live tier, so this is what
        # catches the two drifting. Both are read as text: the suite's
        # interpreter floor (3.10) is above this one's.
        engine = (ROOT / "crates" / "oneharness-core" / "src" / "io").joinpath
        overlay = engine("run_overlay.rs").read_text(encoding="utf-8")
        scratch = engine("scratch.rs").read_text(encoding="utf-8")

        def constant(source: str, name: str) -> str:
            found = re.search(rf'^(?:pub )?const {name}: &str = "([^"]*)";$', source, re.M)
            if found is None:
                self.fail(f"io::run_overlay no longer declares `{name}` as a string")
            return str(found.group(1))

        components = re.search(
            r"^const ROOT_COMPONENTS: \[&str; \d+\] = \[([^\]]*)\];$", overlay, re.M
        )
        if components is None:
            self.fail("io::run_overlay no longer declares ROOT_COMPONENTS")
        root = ", ".join(part.strip() for part in components.group(1).split(","))
        suite = (CONTROL / "run_allowlist_live.py").read_text(encoding="utf-8").splitlines()
        for line in (
            f'OVERLAY_MARKER = "{constant(overlay, "MARKER_ENV")}"',
            f'OVERLAY_PREFIX = "{constant(scratch, "PREFIX")}{constant(overlay, "TAG")}"',
            f"OVERLAY_ROOT = ({root})",
        ):
            self.assertTrue(
                line in suite,
                f"live/control/run_allowlist_live.py no longer has `{line}`: "
                "update its overlay detector to the engine's layout",
            )

    def test_a_number_too_large_for_a_float_is_no_figure(self) -> None:
        self.assertIsNone(number(10**400))
        self.assertIsNone(OverlayTimings.from_json({"build_ms": 10**400, "teardown_ms": 1}))
        self.assertIsNone(number(True))
        self.assertIsNone(number(float("nan")))
        self.assertEqual(number(3), 3.0)

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
