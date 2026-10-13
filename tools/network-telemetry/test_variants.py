"""The opted-in Codex launch: e2e-variants.sh's `codex:plugins` phase.

Driven through scripts/e2e-variants.sh itself — beside e2e-variants-test.sh's
journeys, which hold the rest of that suite — with the mock codex harness
behind the real measuring shim standing for `codex`. The double records the
argv it was launched with, the sleep its turn ran, and the mock's plugins
record of each launch as the evidence the phase reads in Codex's log's place.
The phase's assertions are then held to that run's own evidence, changed one
way per case, and Codex's log extraction to a log database of Codex's shape.
"""

from __future__ import annotations

import json
import os
import re
import sqlite3
import subprocess
import unittest
from pathlib import Path
from typing import NamedTuple

from support import (
    BASH,
    LIB,
    MOCK,
    WINDOWS,
    Record,
    lines,
    measurements,
    need_builds,
    need_recorder,
    run_suite,
    scratch,
    starts,
    suite_env,
    write_lines,
)

PHASE = "codex-plugins-opt-in"
PROVIDER_DOUBLE = (
    Path(__file__).resolve().parents[2] / "scripts" / "e2e-variants-provider-double.sh"
)


class Journey(NamedTuple):
    tmp: Path
    run: subprocess.CompletedProcess[str]
    telemetry: list[Record]
    launch: Record
    evidence: Path
    marker: str


def run_lib(*command: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    """One scripts/e2e-lib.sh helper, called as the suite calls it."""
    return subprocess.run(
        [BASH, "-c", 'set -euo pipefail; source "$1"; shift; "$@"', "_", str(LIB), *command],
        capture_output=True,
        text=True,
        env={**os.environ, **(env or {})},
        check=False,
    )


@unittest.skipIf(WINDOWS, "the double is a bash script; Windows runs the recorder's fallback tests")
class OptedInLaunch(unittest.TestCase):
    def journey(self, *, plugins_off_home: bool = False) -> Journey:
        need_builds(self)
        need_recorder(self)
        tmp = scratch(self)
        env = suite_env(
            tmp,
            "claude",
            "codex",
            ANTHROPIC_API_KEY="test-material",
            OPENAI_API_KEY="test-material",
            OH_E2E_VARIANTS_API_ONLY="1",
            OH_E2E_VARIANTS_CORE_ONLY="1",
            OH_E2E_CODEX_SUBSCRIPTION="1",
            OH_E2E_PLUGINS_SLEEP="2",
            OH_E2E_PLUGINS_EVIDENCE=str(tmp / "plugins-evidence.jsonl"),
            OH_TEST_PLUGINS_EVIDENCE=str(tmp / "plugins-evidence.jsonl"),
            OH_TEST_ARGV_LOG=str(tmp / "argv.jsonl"),
            OH_TEST_RAN_LOG=str(tmp / "ran.log"),
            OH_E2E_PROVIDER_DOUBLE=str(MOCK),
        )
        # Claude's turns are the variants suite's own provider double's, which
        # answers in the shape that suite's Claude checks read.
        claude = Path(env["PATH"].split(os.pathsep)[0]) / "claude"
        claude.write_text(
            f'#!/usr/bin/env bash\nexec bash "{PROVIDER_DOUBLE}" --provider-tool=claude "$@"\n'
        )
        codex_home = Path(env["HOME"]) / ".codex"
        (codex_home / "plugins" / "fixture-plugin").mkdir(parents=True)
        if plugins_off_home:
            (codex_home / "config.toml").write_text("[features]\nplugins = false\n")
        run = run_suite("variants", env)
        telemetry = lines(tmp / "telemetry" / "variants.jsonl")
        opted = [s for s in starts(telemetry) if s["phase"] == PHASE and s["kind"] == "exec"]
        launch = opted[0] if opted else {}
        found = re.search(r"OH_VARIANT_\d+_\d+_pl", " ".join(launch.get("argv", [])))
        return Journey(
            tmp,
            run,
            telemetry,
            launch,
            tmp / "plugins-evidence.jsonl",
            found.group(0) if found else "",
        )

    def test_the_opted_in_launch_runs_with_plugins_on_measured_outside_live_codex(self) -> None:
        journey = self.journey()
        self.assertEqual(journey.run.returncode, 0, journey.run.stderr[-3000:])
        opted = [
            s for s in starts(journey.telemetry) if s["phase"] == PHASE and s["kind"] == "exec"
        ]
        self.assertEqual(len(opted), 1, opted)
        (line,) = measurements(journey.telemetry, journey.launch["launch_id"])
        self.assertEqual(line["method"], "netns-pasta")
        self.assertGreaterEqual(
            line["seconds"], 2, "the recorder measured the launch shorter than its sleep"
        )
        self.assertFalse(
            (journey.tmp / "telemetry" / "codex.jsonl").exists(), "it wrote to live-codex's file"
        )
        launched = [
            json.loads(row)
            for row in (journey.tmp / "argv.jsonl").read_text().splitlines()
            if journey.marker in row and '"exec"' in row
        ]
        self.assertEqual(len(launched), 1, launched)
        argv = launched[0]["argv"]
        pairs = [argv[i : i + 2] for i in range(len(argv) - 1)]
        self.assertIn(
            ["-c", 'model_reasoning_effort="low"'],
            pairs,
            "the variant's own args did not reach codex",
        )
        self.assertNotIn(["-c", "features.plugins=false"], pairs)
        self.assertNotIn(["--disable", "plugins"], pairs)
        self.assertEqual((journey.tmp / "ran.log").read_text().splitlines(), ["sleep 2"])

    def test_a_codex_home_that_disables_plugins_fails_the_phase_naming_its_config(self) -> None:
        journey = self.journey(plugins_off_home=True)
        self.assertNotEqual(journey.run.returncode, 0)
        config = journey.tmp / "home" / ".codex" / "config.toml"
        self.assertIn(f"{config} disables Codex plugins", journey.run.stderr)

    def test_the_phase_assertions_hold_only_for_the_launchs_own_complete_evidence(self) -> None:
        journey = self.journey()
        self.assertEqual(journey.run.returncode, 0, journey.run.stderr[-3000:])
        evidence = lines(journey.evidence)
        mine = [r for r in evidence if journey.marker in r["text"]]
        others = [r for r in evidence if journey.marker not in r["text"]]
        self.assertTrue(mine and others, evidence)

        def evidence_case(records: list[Record]) -> subprocess.CompletedProcess[str]:
            path = journey.tmp / "case-evidence.jsonl"
            write_lines(path, records)
            return run_lib("oh_codex_plugins_evidence_assert", journey.marker, str(path))

        self.assertEqual(evidence_case(evidence).returncode, 0)
        for name, records, said in (
            (
                "no catalog request",
                [*others, *({**r, "catalog_request": False} for r in mine)],
                "never requested the plugin catalog",
            ),
            (
                "no plugin load",
                [*others, *({**r, "plugin_load_ran": False} for r in mine)],
                "never ran its plugin-load step",
            ),
            (
                "only another launch's",
                [*others, *({**r, "text": "another launch"} for r in mine)],
                "belongs to the opted-in launch",
            ),
            ("the launch's own absent", others, "belongs to the opted-in launch"),
            ("none at all", [], "no Codex plugins evidence"),
        ):
            with self.subTest(evidence=name):
                run = evidence_case(records)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(said, run.stderr)

        launch_id = journey.launch["launch_id"]
        (measured,) = measurements(journey.telemetry, launch_id)
        rest = [r for r in journey.telemetry if r.get("launch_id") != launch_id]

        def lines_case(records: list[Record]) -> subprocess.CompletedProcess[str]:
            path = journey.tmp / "case-telemetry.jsonl"
            write_lines(path, records)
            return run_lib(
                "oh_launch_measured_assert",
                str(path),
                PHASE,
                "exec",
                "2",
                env={"OH_E2E_OS": "Linux"},
            )

        self.assertEqual(lines_case([*rest, journey.launch, measured]).returncode, 0)
        second = {**journey.launch, "launch_id": "variants-2-second"}
        for name, records, said in (
            ("no start", [*rest, measured], f"has no exec launch for phase {PHASE}"),
            (
                "two starts",
                [*rest, journey.launch, measured, second],
                f"more than one exec launch for phase {PHASE}",
            ),
        ):
            with self.subTest(starts=name):
                run = lines_case(records)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(said, run.stderr)

        def argv_case(argv: list[str]) -> subprocess.CompletedProcess[str]:
            return run_lib("oh_codex_opted_in_argv_assert", json.dumps(argv))

        self.assertEqual(argv_case(journey.launch["argv"]).returncode, 0)
        own_args = ["-c", 'model_reasoning_effort="low"']
        without_args = [w for w in journey.launch["argv"] if w not in own_args]
        for name, argv, said in (
            ("without the variant's args", without_args, "lacks the variant's own args"),
            (
                "with -c features.plugins=false",
                [*journey.launch["argv"], "-c", "features.plugins=false"],
                "still carried a plugins-off flag",
            ),
            (
                "with --disable plugins",
                ["--disable", "plugins", *journey.launch["argv"]],
                "still carried a plugins-off flag",
            ),
        ):
            with self.subTest(argv=name):
                run = argv_case(argv)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(said, run.stderr)
        unavailable = {
            "launch_id": launch_id,
            "method": "unavailable",
            "reason": "no pasta on PATH",
            "seconds": 3,
            "exit": 0,
        }
        for name, records, said in (
            ("missing", [*rest, journey.launch], "has no measurement line"),
            ("unavailable", [*rest, journey.launch, unavailable], "was not measured"),
            (
                "incomplete",
                [*rest, journey.launch, {k: v for k, v in measured.items() if k != "tx_bytes"}],
                "incomplete measurement line",
            ),
            (
                "negative",
                [*rest, journey.launch, {**measured, "rx_bytes": -5}],
                "incomplete measurement line",
            ),
            (
                "duplicate",
                [*rest, journey.launch, measured, measured],
                "more than one measurement line",
            ),
            (
                "short",
                [*rest, journey.launch, {**measured, "seconds": 0.5}],
                "under the 2 s its sustained shell command takes",
            ),
        ):
            with self.subTest(lines=name):
                run = lines_case(records)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(said, run.stderr)


class PluginsSleep(unittest.TestCase):
    @unittest.skipIf(WINDOWS, "the suite is a bash script")
    def test_a_bad_sleep_is_refused_before_the_opted_in_launch(self) -> None:
        need_builds(self)
        for value in ("0", "601", "18446744073709551617", "08", "abc"):
            with self.subTest(value=value):
                tmp = scratch(self)
                env = suite_env(
                    tmp,
                    "claude",
                    "codex",
                    ANTHROPIC_API_KEY="test-material",
                    OPENAI_API_KEY="test-material",
                    OH_E2E_VARIANTS_API_ONLY="1",
                    OH_E2E_VARIANTS_CORE_ONLY="1",
                    OH_E2E_CODEX_SUBSCRIPTION="1",
                    OH_E2E_PLUGINS_SLEEP=value,
                    OH_E2E_PROVIDER_DOUBLE=str(MOCK),
                )
                claude = Path(env["PATH"].split(os.pathsep)[0]) / "claude"
                claude.write_text(
                    f'#!/usr/bin/env bash\nexec bash "{PROVIDER_DOUBLE}" --provider-tool=claude "$@"\n'
                )
                run = run_suite("variants", env)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(
                    "OH_E2E_PLUGINS_SLEEP must be a whole number of seconds from 1 to 600",
                    run.stderr,
                )
                opted = [
                    s
                    for s in starts(lines(tmp / "telemetry" / "variants.jsonl"))
                    if s["phase"] == PHASE and s["kind"] == "exec"
                ]
                self.assertEqual(opted, [], "the opted-in launch ran on a bad sleep")


class CodexHomePlugins(unittest.TestCase):
    """oh_codex_home_allows_plugins over every top-level spelling of the setting."""

    def test_each_spelling_that_turns_plugins_off_is_refused_naming_the_file(self) -> None:
        cases = {
            "[features]\nplugins = false\n": False,
            '[ features ] # trailing\n"plugins"=false # comment\n': False,
            "features.plugins = false\n": False,
            "features = { apps = true, plugins = false }\n": False,
            "[features]\nplugins = true\n": True,
            "[profiles.fast.features]\nplugins = false\n": True,
            "[features]\n# plugins = false\n": True,
            "[features]\napps = false\n[other]\nplugins = false\n": True,
            'model = "gpt"\n': True,
        }
        for text, allowed in cases.items():
            with self.subTest(config=text):
                home = scratch(self)
                (home / "config.toml").write_text(text)
                run = run_lib("oh_codex_home_allows_plugins", str(home))
                self.assertEqual(run.returncode == 0, allowed, run.stderr)
                if not allowed:
                    self.assertIn(f"{home / 'config.toml'} disables Codex plugins", run.stderr)

    def test_a_config_that_is_not_toml_is_refused_naming_the_file(self) -> None:
        home = scratch(self)
        (home / "config.toml").write_text("[features\nplugins = false\n")
        run = run_lib("oh_codex_home_allows_plugins", str(home))
        self.assertNotEqual(run.returncode, 0)
        self.assertIn(f"could not read {home / 'config.toml'} as TOML", run.stderr)


class CodexLogEvidence(unittest.TestCase):
    """The live run reads Codex's own log: rows of `logs_2.sqlite` in Codex's
    schema, attributed to the opted-in launch's process by its marker or its
    thread, and its catalog request and plugin bundle sync from their lines."""

    def test_codex_log_rows_become_one_record_per_process(self) -> None:
        tmp = scratch(self)
        home = tmp / "codex-home"
        home.mkdir()
        db = sqlite3.connect(home / "logs_2.sqlite")
        db.execute(
            "create table logs (id integer primary key autoincrement, ts integer not null, ts_nanos integer not null,"
            " level text not null, target text not null, feedback_log_body text, module_path text, file text,"
            " line integer, thread_id text, process_uuid text, estimated_bytes integer not null default 0)"
        )
        rows = [
            # The opted-in launch's process: named by its thread, then by its prompt.
            (
                2000,
                "th-mine",
                "pid:7:aaa",
                "session_loop{thread_id=th-mine}: Submission ... OH_VARIANT_1_2_pl ...",
            ),
            (
                2001,
                None,
                "pid:7:aaa",
                'fetching remote plugin catalog operation="plugins.remote_catalog.list"',
            ),
            (
                2002,
                None,
                "pid:7:aaa",
                "completed remote installed plugin bundle sync materialized_remote_plugins=[]",
            ),
            # Another launch that also fetched the catalog.
            (
                2001,
                "th-other",
                "pid:9:bbb",
                'fetching remote plugin catalog operation="plugins.remote_catalog.list"',
            ),
            # A row from before the launch began is not read.
            (1000, None, "pid:3:old", "completed remote installed plugin bundle sync"),
        ]
        for ts, thread, process, body in rows:
            db.execute(
                "insert into logs (ts, ts_nanos, level, target, feedback_log_body, thread_id, process_uuid)"
                " values (?, 0, 'INFO', 'codex', ?, ?, ?)",
                (ts, body, thread, process),
            )
        db.commit()
        db.close()
        run = run_lib("oh_codex_log_evidence", str(home), "OH_VARIANT_1_2_pl", "th-mine", "1500")
        self.assertEqual(run.returncode, 0, run.stderr)
        records = sorted(
            (json.loads(line) for line in run.stdout.splitlines()), key=lambda r: r["process"]
        )
        self.assertEqual(
            records,
            [
                {
                    "process": "pid:7:aaa",
                    "text": "OH_VARIANT_1_2_pl",
                    "catalog_request": True,
                    "plugin_load_ran": True,
                },
                {
                    "process": "pid:9:bbb",
                    "text": "",
                    "catalog_request": True,
                    "plugin_load_ran": False,
                },
            ],
        )
        evidence = tmp / "evidence.jsonl"
        evidence.write_text(run.stdout)
        self.assertEqual(
            run_lib(
                "oh_codex_plugins_evidence_assert", "OH_VARIANT_1_2_pl", str(evidence)
            ).returncode,
            0,
        )

    def test_an_unreadable_codex_log_is_refused(self) -> None:
        tmp = scratch(self)
        run = run_lib("oh_codex_log_evidence", str(tmp), "OH_VARIANT_1_2_pl", "", "0")
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("logs_2.sqlite", run.stderr)


if __name__ == "__main__":
    unittest.main()
