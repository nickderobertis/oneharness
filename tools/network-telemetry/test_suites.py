"""Every live suite reaches the recorder, and the legs select what they should.

Each suite runs its own setup and its first phase through its own helper —
OH_E2E_PHASE_LIMIT=1 stops it as its second phase starts, if the first passed
against the double at all — with the double
standing for every harness executable, so nothing real or paid is launched.
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import time
import unittest
from pathlib import Path

from support import (
    ROOT,
    WINDOWS,
    check_step,
    lines,
    measurements,
    need_builds,
    recorder_unavailable,
    run_shim,
    run_suite,
    scratch,
    starts,
    suite_env,
)

EXECUTABLES = ("claude", "codex", "copilot", "crush", "cursor-agent", "goose", "opencode", "qwen")
# suite -> (the executable its first launch runs, its harness id, its auth env)
SUITES = {
    "claude": ("claude", "claude-code", {"ANTHROPIC_API_KEY": "test-material"}),
    "copilot": ("copilot", "copilot", {"COPILOT_GITHUB_TOKEN": "test-material"}),
    "crush": ("crush", "crush", {"ANTHROPIC_API_KEY": "test-material"}),
    "cursor": ("cursor-agent", "cursor", {"CURSOR_API_KEY": "test-material"}),
    "goose": ("goose", "goose", {"OPENAI_API_KEY": "test-material"}),
    "opencode": ("opencode", "opencode", {"ANTHROPIC_API_KEY": "test-material"}),
    "qwen": ("qwen", "qwen", {"OPENAI_API_KEY": "test-material"}),
    # The first controllable harness with credentials runs first; codex's
    # double speaks the app-server its control phase drives.
    "control": ("codex", "codex", {"CODEX_E2E_AUTH": "1"}),
    "schema": ("claude", "claude-code", {"ANTHROPIC_API_KEY": "test-material"}),
    "variants": (
        "claude",
        "claude-code",
        {
            "ANTHROPIC_API_KEY": "test-material",
            "OPENAI_API_KEY": "test-material",
            "OH_E2E_VARIANTS_API_ONLY": "1",
            "OH_E2E_VARIANTS_CORE_ONLY": "1",
        },
    ),
}


def run_until_recorded(suite: str, env: dict[str, str], harness: str, limit: float = 120) -> str:
    """Run a suite until it has recorded a measured launch of `harness`, or
    exits; then stop it — by its own process group, as a runner would — and
    return its stderr. A first phase the double cannot satisfy may otherwise
    wait out minutes of live-tier timeouts nobody here is testing."""
    telemetry = Path(env["OH_E2E_TELEMETRY_DIR"]) / f"{suite}.jsonl"
    err = Path(env["TMPDIR"]) / "suite.err"
    with err.open("wb") as sink:
        process = subprocess.Popen(
            ["bash", str(ROOT / "scripts" / f"e2e-{suite}.sh")],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=sink,
            env=env,
            start_new_session=True,
        )
        started = time.monotonic()
        while process.poll() is None and time.monotonic() - started < limit:
            records = lines(telemetry)
            if any(
                s["harness"] == harness and measurements(records, s["launch_id"])
                for s in starts(records)
            ):
                time.sleep(1)
                break
            time.sleep(0.2)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
    # A launch the stop cut short is still recorded, by the recorder's own
    # supervisor; give it the namespace's kill grace to finish.
    for _ in range(80):
        if not any(Path(env["OH_E2E_TELEMETRY_DIR"], ".pending").glob("*")):
            break
        time.sleep(0.1)
    return err.read_text(errors="replace")


@unittest.skipIf(
    WINDOWS, "the suites' doubles are bash scripts; Windows runs the recorder's fallback tests"
)
class EverySuiteRecords(unittest.TestCase):
    def test_each_suite_records_its_first_launch_through_its_own_shim(self) -> None:
        need_builds(self)
        for suite, (_exe, harness, auth) in SUITES.items():
            with self.subTest(suite=suite):
                tmp = scratch(self)
                stderr = run_until_recorded(
                    suite, suite_env(tmp, *EXECUTABLES, OH_E2E_PHASE_LIMIT="1", **auth), harness
                )
                records = lines(tmp / "telemetry" / f"{suite}.jsonl")
                launched = starts(records)
                self.assertTrue(launched, f"{suite} recorded no launch: {stderr[-2000:]}")
                first = next((s for s in launched if s["harness"] == harness), None)
                self.assertIsNotNone(first, f"{suite} launched no {harness}: {launched}")
                assert first is not None
                self.assertEqual(first["suite"], suite)
                self.assertNotIn(first["phase"], ("", "unphased", "main"), first)
                self.assertIn(first["kind"], ("turn", "local", "exec", "app-server"), first)
                # Its first phase may well fail against a double; what is held
                # is that every launch it made was recorded, once.
                for launch in launched:
                    self.assertEqual(len(measurements(records, launch["launch_id"])), 1, launch)

    def test_each_suite_fails_when_its_harness_on_path_is_not_its_shim(self) -> None:
        need_builds(self)
        for suite, (exe, _harness, auth) in SUITES.items():
            with self.subTest(suite=suite):
                tmp = scratch(self)
                stale = tmp / "telemetry" / ".shims" / suite
                stale.mkdir(parents=True)
                (stale / exe).write_text("#!/usr/bin/env bash\necho 'not a shim'\n")
                (stale / exe).chmod(0o555)
                run = run_suite(suite, suite_env(tmp, *EXECUTABLES, OH_E2E_PHASE_LIMIT="1", **auth))
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(
                    f"PATH resolves {exe} to '{stale / exe}', not this suite's measuring shim",
                    run.stderr,
                )
                self.assertEqual(starts(lines(tmp / "telemetry" / f"{suite}.jsonl")), [])

    def test_the_registry_executables_are_the_ones_the_suites_shim(self) -> None:
        need_builds(self)
        listed = json.loads(
            subprocess.run(
                [str(ROOT / "target" / "debug" / "oneharness"), "list", "--compact"],
                capture_output=True,
                text=True,
                env={**os.environ, "ONEHARNESS_NO_CONFIG": "1"},
                check=True,
            ).stdout
        )
        registry = sorted(f"{h['default_bin']}:{h['id']}" for h in listed["harnesses"])
        library = (ROOT / "scripts" / "e2e-lib.sh").read_text()
        declared = re.search(r"OH_HARNESS_EXECUTABLES=\(([^)]*)\)", library)
        assert declared is not None
        self.assertEqual(sorted(declared.group(1).split()), registry)


@unittest.skipIf(WINDOWS, "the double is a bash script")
class DirectCodexLaunches(unittest.TestCase):
    """The suite's own `codex exec` launches (the synced-rules phase) start Codex
    with the defaults oneharness applies, so they fetch no plugin catalog."""

    def test_a_direct_codex_launch_runs_with_plugins_apps_and_analytics_off(self) -> None:
        need_builds(self)
        tmp = scratch(self)
        env = suite_env(tmp, "codex")
        env.update(
            OH_TEST_ARGV_LOG=str(tmp / "argv.jsonl"),
            OH_TEST_PLUGINS_EVIDENCE=str(tmp / "plugins.jsonl"),
        )
        project, home = tmp / "project", tmp / "codex-home"
        project.mkdir()
        (home / "plugins" / "fixture-plugin").mkdir(parents=True)
        run = subprocess.run(
            [
                "bash",
                "-c",
                'set -euo pipefail; source "$1"; _oh_codex_direct "$2" "$3" "Reply exactly ONEHARNESS-LIVE-7"',
                "_",
                str(ROOT / "scripts" / "e2e-lib.sh"),
                str(project),
                str(home),
            ],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        (launch,) = [json.loads(line) for line in (tmp / "argv.jsonl").read_text().splitlines()]
        argv = launch["argv"]
        for flag in ("features.plugins=false", "features.apps=false", "analytics.enabled=false"):
            self.assertIn(["-c", flag], [argv[i : i + 2] for i in range(len(argv) - 1)], argv)
        (evidence,) = [
            json.loads(line) for line in (tmp / "plugins.jsonl").read_text().splitlines()
        ]
        self.assertEqual(
            (evidence["catalog_request"], evidence["plugins_loaded"]), (False, False), evidence
        )


# --- the legs ---------------------------------------------------------------

LEGS = {"Linux": "ubuntu-latest", "macOS": "macos-latest", "Windows": "windows-latest"}


def steps(workflow: Path, job: str) -> list[dict[str, str]]:
    """The job's steps in order, each as its `name`/`if`/`run`/`uses` and env text."""
    text = workflow.read_text()
    body = re.search(rf"^  {job}:\n(.*?)(?=^  \S|\Z)", text, re.M | re.S)
    assert body is not None, f"{workflow} has no job {job}"
    found = []
    for chunk in re.split(r"^      - ", body.group(1).split("    steps:\n", 1)[1], flags=re.M)[1:]:
        step: dict[str, str] = {"text": chunk}
        for key in ("name", "if", "run", "uses"):
            match = re.search(rf"^(?:{key}|        {key}): (.*)$", chunk, re.M)
            if match:
                step[key] = match.group(1).strip()
        found.append(step)
    return found


def selected(step: dict[str, str], runner_os: str) -> bool:
    """Whether a step runs on `runner_os`: its `runner.os` comparisons decide;
    any other condition (an earlier step's output) is taken as met."""
    return all(
        other == runner_os for other in re.findall(r"runner\.os == '(\w+)'", step.get("if", ""))
    )


def named(steps_run: list[dict[str, str]], fragment: str) -> int:
    for index, step in enumerate(steps_run):
        if fragment in step.get("name", "") or fragment in step.get("run", ""):
            return index
    return -1


class Legs(unittest.TestCase):
    PREPARE = "Permit pasta's unprivileged namespaces"

    def test_only_the_linux_leg_prepares_pasta_and_does_so_before_measuring(self) -> None:
        for workflow, job, measured in (
            (ROOT / ".github/workflows/e2e-codex.yml", "live", "just live-codex"),
            (ROOT / ".github/workflows/ci.yml", "check", "just check"),
        ):
            for runner_os in LEGS:
                with self.subTest(workflow=workflow.name, leg=runner_os):
                    run = [s for s in steps(workflow, job) if selected(s, runner_os)]
                    prepare, measure = named(run, self.PREPARE), named(run, measured)
                    self.assertGreaterEqual(
                        measure, 0, f"{workflow.name} never runs {measured} on {runner_os}"
                    )
                    if runner_os == "Linux":
                        self.assertGreaterEqual(prepare, 0)
                        self.assertLess(prepare, measure, "pasta is prepared after it is needed")
                        self.assertIn("apt-get install -y -q passt", run[prepare]["run"])
                    else:
                        self.assertEqual(
                            prepare, -1, f"{runner_os} has no AppArmor userns restriction to lift"
                        )
        gate = [
            s
            for s in steps(ROOT / ".github/workflows/ci.yml", "check")
            if "just check" in s.get("run", "")
        ]
        self.assertEqual(len(gate), 2)
        for step in gate:
            self.assertIn("OH_NET_NO_SKIP: ${{ runner.os == 'Linux' && '1' || '' }}", step["text"])
        self.assertLess(
            named(steps(ROOT / ".github/workflows/ci.yml", "check"), self.PREPARE),
            named(steps(ROOT / ".github/workflows/ci.yml", "check"), "Bootstrap"),
        )

    def test_only_the_linux_leg_selects_the_budget_check(self) -> None:
        for leg, runs_check in (
            ("Linux", True),
            ("Darwin", False),
            ("MINGW64_NT-10.0-20348", False),
        ):
            with self.subTest(leg=leg):
                tmp = scratch(self)
                run = check_step(tmp / "never-written", os_name=leg)
                if runs_check:
                    self.assertNotEqual(run.returncode, 0, run.stdout)
                    self.assertIn("no telemetry at", run.stdout)
                else:
                    self.assertEqual(run.returncode, 0, run.stderr)
                    self.assertIn(f"not checked on {leg}", run.stdout)
                    self.assertNotIn("budget codex", run.stdout)

    @unittest.skipIf(WINDOWS, "the legs are named through OH_E2E_OS on unix")
    def test_the_unsupported_legs_select_the_fallback_launch(self) -> None:
        for leg in ("Darwin", "MINGW64_NT-10.0-20348", "Linux"):
            with self.subTest(leg=leg):
                if leg == "Linux" and recorder_unavailable():
                    self.skipTest(recorder_unavailable())
                tmp = scratch(self)
                true = tmp / "true-double"
                true.write_text("#!/usr/bin/env bash\nexit 0\n")
                true.chmod(0o755)
                run_shim(tmp, [str(true)], ["exec", "x"], env={"OH_E2E_OS": leg})
                records = lines(tmp / "codex.jsonl")
                (line,) = measurements(records, starts(records)[0]["launch_id"])
                self.assertEqual(line["method"], "netns-pasta" if leg == "Linux" else "unavailable")


if __name__ == "__main__":
    unittest.main()
