"""Every live suite reaches the recorder, and the legs select what they should.

Each suite runs its own setup and its first phase through its own helper —
OH_E2E_PHASE_LIMIT=1 stops it as its second phase starts, if the first passed
against the double at all — with the double
standing for every harness executable, so nothing real or paid is launched.
"""

from __future__ import annotations

import importlib.util
import itertools
import json
import os
import re
import signal
import subprocess
import sys
import time
import unittest
from dataclasses import dataclass
from pathlib import Path
from typing import NamedTuple

from support import (
    BASH,
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


class Suite(NamedTuple):
    """A live suite as these tests drive it."""

    executable: str  # what its first launch runs
    harness: str  # that launch's harness id
    auth: dict[str, str]  # the credentials its preflight asks for


SUITES = {
    "claude": Suite("claude", "claude-code", {"ANTHROPIC_API_KEY": "test-material"}),
    "copilot": Suite("copilot", "copilot", {"COPILOT_GITHUB_TOKEN": "test-material"}),
    "crush": Suite("crush", "crush", {"ANTHROPIC_API_KEY": "test-material"}),
    "cursor": Suite("cursor-agent", "cursor", {"CURSOR_API_KEY": "test-material"}),
    "goose": Suite("goose", "goose", {"OPENAI_API_KEY": "test-material"}),
    "opencode": Suite("opencode", "opencode", {"ANTHROPIC_API_KEY": "test-material"}),
    "qwen": Suite("qwen", "qwen", {"OPENAI_API_KEY": "test-material"}),
    # The first controllable harness with credentials runs first; codex's
    # double speaks the app-server its control phase drives.
    "control": Suite("codex", "codex", {"CODEX_E2E_AUTH": "1"}),
    "schema": Suite("claude", "claude-code", {"ANTHROPIC_API_KEY": "test-material"}),
    "variants": Suite(
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
            [BASH, str(ROOT / "scripts" / f"e2e-{suite}.sh")],
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
        for suite, (_exe, harness, auth) in SUITES.items():  # Suite unpacks in field order
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
                # A stale shim directory the setup cannot rewrite, holding a
                # foreign executable where the shim goes.
                stale = tmp / "telemetry" / ".shims" / suite
                stale.mkdir(parents=True)
                (stale / ".oh-network-shims").write_text("")
                (stale / exe).write_text("#!/usr/bin/env bash\necho 'not a shim'\n")
                (stale / exe).chmod(0o755)
                stale.chmod(0o555)
                self.addCleanup(stale.chmod, 0o755)
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


@unittest.skipIf(WINDOWS, "the doubles are bash scripts")
class Setup(unittest.TestCase):
    """oh_telemetry_setup itself, driven as a suite drives it."""

    def setup_and(
        self,
        tmp: Path,
        script: str,
        *,
        path_first: Path | None = None,
        telemetry_dir: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        env = suite_env(tmp, "codex")
        if path_first is not None:
            env["PATH"] = f"{path_first}{os.pathsep}{env['PATH']}"
        if telemetry_dir is not None:
            env["OH_E2E_TELEMETRY_DIR"] = telemetry_dir
        return subprocess.run(
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
            check=False,
        )

    def test_a_windows_form_telemetry_directory_is_taken_as_that_directory(self) -> None:
        # Unix reads C:\... as relative, so the shims land under the cwd here;
        # what matters is that the shim records to and names the directory as
        # given, never prefixed with the cwd. (Its colon splits PATH on unix,
        # so the PATH check that follows is Windows's own, not this test's.)
        need_builds(self)
        tmp = scratch(self)
        windows_form = r"C:\oh-telemetry"
        self.setup_and(tmp, "oh_telemetry_setup codex", telemetry_dir=windows_form)
        shim = (tmp / windows_form / ".shims" / "codex" / "codex").read_text()
        quoted = windows_form.replace("\\", "\\\\")
        self.assertIn(f"--file {quoted}/codex.jsonl ", shim)
        self.assertIn(f"--self {quoted}/.shims/codex/codex ", shim)
        self.assertNotIn(
            str(tmp), shim.split("--self", 1)[1], "the directory was joined onto the cwd"
        )

    def test_a_suite_started_under_another_suites_shims_wraps_the_real_executable(self) -> None:
        need_builds(self)
        tmp = scratch(self)
        lib = ROOT / "scripts" / "e2e-lib.sh"
        run = self.setup_and(
            tmp,
            "oh_telemetry_setup outer; OH_E2E_SUITE= OH_E2E_TELEMETRY_FILE= "
            f"bash -c 'set -euo pipefail; source \"{lib}\"; oh_telemetry_setup inner; codex --version >/dev/null'",
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        inner = lines(tmp / "telemetry" / "inner.jsonl")
        (launch,) = starts(inner)
        self.assertEqual(len(measurements(inner, launch["launch_id"])), 1)
        self.assertEqual(
            lines(tmp / "telemetry" / "outer.jsonl"),
            [],
            "the outer shim recorded the inner launch too",
        )
        shim = (tmp / "telemetry" / ".shims" / "inner" / "codex").read_text()
        self.assertIn(
            str(tmp / "double-bin" / "codex"),
            shim,
            "the inner shim wraps the outer one, not the real codex",
        )

    def test_a_stale_shim_of_the_same_suite_is_refused_when_it_cannot_be_rewritten(self) -> None:
        # The last run's shim (same suite, same marker) wraps an executable
        # that has since moved; this run cannot rewrite it, and must not take
        # the old one for its own.
        need_builds(self)
        tmp = scratch(self)
        self.assertEqual(self.setup_and(tmp, "oh_telemetry_setup again").returncode, 0)
        moved = tmp / "moved-bin"
        moved.mkdir()
        (moved / "codex").write_text("#!/usr/bin/env bash\nexit 0\n")
        (moved / "codex").chmod(0o755)
        shims = tmp / "telemetry" / ".shims" / "again"
        shims.chmod(0o555)
        self.addCleanup(shims.chmod, 0o755)
        run = self.setup_and(tmp, "oh_telemetry_setup again", path_first=moved)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn(
            f"PATH resolves codex to '{shims / 'codex'}', not this suite's measuring shim",
            run.stderr,
        )

    def test_windows_gets_a_cmd_adapter_and_refuses_one_that_bypasses_the_shim(self) -> None:
        # Git Bash's cygpath is stood in for by one that marks the Windows form,
        # so the adapter oh_telemetry_setup writes there can be read here.
        need_builds(self)
        tmp = scratch(self)
        stand_in = tmp / "cygpath-bin"
        stand_in.mkdir()
        (stand_in / "cygpath").write_text('#!/usr/bin/env bash\nprintf "W:%s" "${@: -1}"\n')
        (stand_in / "cygpath").chmod(0o755)
        run = self.setup_and(tmp, "oh_telemetry_setup winleg", path_first=stand_in)
        self.assertEqual(run.returncode, 0, run.stderr)
        shims = tmp / "telemetry" / ".shims" / "winleg"
        adapter = (shims / "codex.cmd").read_bytes()
        bash = subprocess.run(
            [BASH, "-c", "command -v bash"], capture_output=True, text=True
        ).stdout.strip()
        self.assertEqual(adapter, f'@"W:{bash}" "W:{shims / "codex"}" %*\r\n'.encode())
        stale = self.setup_and(
            tmp,
            'oh_telemetry_setup winleg; printf "@other %%*\\r\\n" >"$OH_E2E_TELEMETRY_DIR/.shims/winleg/codex.cmd"; _oh_shims_first',
            path_first=stand_in,
        )
        self.assertNotEqual(stale.returncode, 0)
        self.assertIn("codex.cmd does not launch this suite's measuring shim", stale.stderr)


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
                BASH,
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
            (evidence["catalog_request"], evidence["plugin_load_ran"]), (False, False), evidence
        )

    def test_the_direct_launch_defaults_are_the_ones_oneharness_applies(self) -> None:
        # _OH_CODEX_DEFAULTS restates oneharness's own Codex defaults; held to
        # the argv oneharness builds, so the two cannot drift apart.
        need_builds(self)
        planned = json.loads(
            subprocess.run(
                [
                    str(ROOT / "target" / "debug" / "oneharness"),
                    "run",
                    "--harness",
                    "codex",
                    "--prompt",
                    "x",
                    "--print-command",
                    "--compact",
                ],
                capture_output=True,
                text=True,
                env={**os.environ, "ONEHARNESS_NO_CONFIG": "1"},
                check=True,
            ).stdout
        )
        argv = json.dumps(planned)
        library = (ROOT / "scripts" / "e2e-lib.sh").read_text()
        declared = re.search(r"^_OH_CODEX_DEFAULTS=\((.*)\)$", library, re.M)
        assert declared is not None
        words = declared.group(1).split()
        self.assertEqual(words[::2], ["-c"] * 3)
        for setting in words[1::2]:
            self.assertIn(
                f'"-c", "{setting}"',
                argv.replace('","', '", "'),
                f"oneharness no longer applies {setting}",
            )


LEGS = ("Linux", "macOS", "Windows")


@dataclass(frozen=True)
class Step:
    """One workflow step, as much of it as leg selection reads."""

    name: str
    condition: str  # its `if:`, "" when it always runs
    run: str
    text: str  # the whole step, env block included


def steps(workflow: Path, job: str) -> list[Step]:
    """The job's steps in order."""
    text = workflow.read_text()
    body = re.search(rf"^  {job}:\n(.*?)(?=^  \S|\Z)", text, re.M | re.S)
    assert body is not None, f"{workflow} has no job {job}"

    def field(chunk: str, key: str) -> str:
        match = re.search(rf"^(?:{key}|        {key}): (.*)$", chunk, re.M)
        if match is None:
            return ""
        value = str(match.group(1)).strip()
        if value not in ("|", ">"):
            return value
        # A block scalar: its body is the lines indented past the key.
        block = chunk[match.end() :].splitlines()[1:]
        lines = itertools.takewhile(
            lambda line: line.startswith(" " * 10) or not line.strip(), block
        )
        return "\n".join(line.strip() for line in lines if line.strip())

    chunks = re.split(r"^      - ", body.group(1).split("    steps:\n", 1)[1], flags=re.M)[1:]
    return [Step(field(c, "name"), field(c, "if"), field(c, "run"), c) for c in chunks]


def selected(step: Step, runner_os: str) -> bool:
    """Whether a step runs on `runner_os`: its `runner.os` comparisons decide;
    any other condition (an earlier step's output) is taken as met."""
    return all(other == runner_os for other in re.findall(r"runner\.os == '(\w+)'", step.condition))


def named(steps_run: list[Step], fragment: str) -> int:
    for index, step in enumerate(steps_run):
        if fragment in step.name or fragment in step.run:
            return index
    return -1


@unittest.skipIf(WINDOWS, "the suites are bash scripts")
class Settings(unittest.TestCase):
    """Every setting the recorder, the suites and the budget step read is
    refused, saying what it accepts, before anything runs on a bad value."""

    def test_a_bad_phase_limit_stops_the_suite_before_its_first_launch(self) -> None:
        need_builds(self)
        for value in ("x", "-1", "1000", "99999999999999999999", "1.5"):
            with self.subTest(value=value):
                tmp = scratch(self)
                run = run_suite("codex", suite_env(tmp, "codex", OH_E2E_PHASE_LIMIT=value))
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(
                    "OH_E2E_PHASE_LIMIT must be a whole number of phases from 0 to 999", run.stderr
                )
                self.assertEqual(starts(lines(tmp / "telemetry" / "codex.jsonl")), [])

    def test_a_bad_pending_wait_is_refused_before_the_check(self) -> None:
        for value in ("x", "08", "99999", "-1"):
            with self.subTest(value=value):
                tmp = scratch(self)
                run = subprocess.run(
                    [BASH, str(ROOT / "scripts" / "e2e-network-budget.sh"), "codex"],
                    capture_output=True,
                    text=True,
                    env={
                        **os.environ,
                        "OH_E2E_TELEMETRY_DIR": str(tmp),
                        "OH_E2E_OS": "Linux",
                        "OH_E2E_PENDING_WAIT": value,
                    },
                    check=False,
                )
                self.assertEqual(run.returncode, 2)
                self.assertIn("OH_E2E_PENDING_WAIT must be a whole number of seconds", run.stderr)
                self.assertNotIn("budget codex", run.stdout)

    def run_budget_step(
        self, tmp: Path, *, path_first: Path | None = None
    ) -> subprocess.CompletedProcess[str]:
        path = f"{path_first}{os.pathsep}{os.environ['PATH']}" if path_first else os.environ["PATH"]
        return subprocess.run(
            [BASH, str(ROOT / "scripts" / "e2e-network-budget.sh"), "codex"],
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "OH_E2E_TELEMETRY_DIR": str(tmp),
                "OH_E2E_OS": "Linux",
                "PATH": path,
            },
            check=False,
        )

    def stand_in(self, tmp: Path, name: str, script: str) -> Path:
        directory = tmp / f"{name}-stand-in"
        directory.mkdir()
        (directory / name).write_text(f"#!/usr/bin/env bash\n{script}\n")
        (directory / name).chmod(0o755)
        return directory

    def test_the_budget_step_refuses_a_pending_path_it_cannot_list(self) -> None:
        tmp = scratch(self)
        (tmp / ".pending").write_text("not a directory")
        run = self.run_budget_step(tmp)
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertIn(
            f"cannot read {tmp / '.pending'} to wait for launches still being measured", run.stderr
        )
        self.assertNotIn("budget codex", run.stdout)
        listed = scratch(self)
        (listed / ".pending").mkdir()
        (listed / ".pending" / "codex-1-abc").write_text("")
        run = self.run_budget_step(
            listed, path_first=self.stand_in(listed, "find", "echo 'find: failed' >&2; exit 1")
        )
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertIn(f"cannot list {listed / '.pending'}", run.stderr)
        self.assertNotIn("budget codex", run.stdout)

    def test_the_budget_step_stops_when_its_tools_cannot_be_installed(self) -> None:
        tmp = scratch(self)
        ran = tmp / "checker-ran"
        uv = self.stand_in(
            tmp, "uv", f'[ "$1" = sync ] && {{ echo "uv: no network" >&2; exit 2; }}\ntouch "{ran}"'
        )
        run = self.run_budget_step(tmp, path_first=uv)
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertIn("could not install the uv workspace's dev tools (onebudgetspec)", run.stderr)
        self.assertFalse(ran.exists(), "the budget checker ran without its tools")

    def test_a_bad_deadline_or_observer_is_refused_before_the_launch(self) -> None:
        for name, value, said in (
            ("OH_E2E_NET_DEADLINE", "-5", "must be a finite, non-negative number of seconds"),
            ("OH_E2E_NET_DEADLINE", "nan", "must be a finite, non-negative number of seconds"),
            ("OH_E2E_NET_DEADLINE", "soon", "must be a finite, non-negative number of seconds"),
            ("OH_E2E_NET_OBSERVER", '"python3"', "must be a JSON array of non-empty strings"),
            ("OH_E2E_NET_OBSERVER", '["python3", 2]', "must be a JSON array of non-empty strings"),
            ("OH_E2E_NET_OBSERVER", "not json", "must be a JSON array of non-empty strings"),
        ):
            with self.subTest(setting=name, value=value):
                tmp = scratch(self)
                ran = tmp / "ran"
                run = subprocess.run(
                    [
                        sys.executable,
                        str(ROOT / "scripts" / "e2e-netmeasure.py"),
                        "--line-file",
                        str(tmp / "t.jsonl"),
                        "--launch-id",
                        "x",
                        "--",
                        "touch",
                        str(ran),
                    ],
                    capture_output=True,
                    text=True,
                    env={**os.environ, name: value},
                    check=False,
                )
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(said, run.stderr)
                self.assertFalse(ran.exists(), "the launch ran on a bad setting")
                self.assertFalse((tmp / "t.jsonl").exists())

    def test_a_malformed_namespace_result_never_becomes_a_measurement(self) -> None:
        spec = importlib.util.spec_from_file_location(
            "netmeasure", ROOT / "scripts" / "e2e-netmeasure.py"
        )
        assert spec is not None and spec.loader is not None
        netmeasure = importlib.util.module_from_spec(spec)
        # Its dataclasses resolve their annotations through sys.modules.
        sys.modules["netmeasure"] = netmeasure
        self.addCleanup(sys.modules.pop, "netmeasure", None)
        spec.loader.exec_module(netmeasure)
        good = {
            "moved": [10, 1, 5, 1],
            "namespace_tcp_retrans_segs": 0,
            "seconds": 1.5,
            "exit": 0,
            "start": {"before_ns": 1, "after_ns": 2},
            "end": {"before_ns": 3, "after_ns": 4},
        }
        self.assertEqual(
            netmeasure.NamespaceResult.from_json(json.dumps(good)).line("x").rx_bytes, 10
        )
        for name, change in (
            ("a negative count", {"moved": [-1, 1, 5, 1]}),
            ("a boolean count", {"moved": [True, 1, 5, 1]}),
            ("three counters", {"moved": [10, 1, 5]}),
            ("a boolean duration", {"seconds": True}),
            ("an infinite duration", {"seconds": float("inf")}),
            ("a fractional exit", {"exit": 1.5}),
            ("a window that ends before it begins", {"start": {"before_ns": 5, "after_ns": 2}}),
            ("windows out of order", {"start": {"before_ns": 6, "after_ns": 7}}),
            ("a window that is not an object", {"end": [3, 4]}),
        ):
            with self.subTest(result=name):
                with self.assertRaises(ValueError):
                    netmeasure.NamespaceResult.from_json(json.dumps({**good, **change}))


class Legs(unittest.TestCase):
    PREPARE = "Permit unprivileged namespaces for pasta"

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
                        # The image's passt profile alone does not admit
                        # pasta's namespace: the restriction itself is lifted,
                        # then the pasta the recorder resolves is proven to
                        # start one.
                        lift = "sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0"
                        probe = "pasta --config-net --quiet -- true"
                        self.assertIn(lift, run[prepare].run)
                        self.assertIn(probe, run[prepare].run)
                        self.assertLess(run[prepare].run.index(lift), run[prepare].run.index(probe))
                    else:
                        self.assertEqual(
                            prepare, -1, f"{runner_os} has no AppArmor userns restriction to lift"
                        )
        gate = [
            s for s in steps(ROOT / ".github/workflows/ci.yml", "check") if "just check" in s.run
        ]
        self.assertEqual(len(gate), 2)
        for step in gate:
            self.assertIn("OH_NET_NO_SKIP: ${{ runner.os == 'Linux' && '1' || '' }}", step.text)
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

    def test_the_budget_step_waits_for_a_launch_still_being_measured(self) -> None:
        # A namespace still winding down holds its pending marker until its
        # line is in; the step waits for it — and only up to its bound.
        tmp = scratch(self)
        pending = tmp / ".pending"
        pending.mkdir()
        (pending / "codex-1-abc").write_text("")
        remover = subprocess.Popen([BASH, "-c", f'sleep 1.5; rm -f "{pending}/codex-1-abc"'])
        started = time.monotonic()
        run = check_step(tmp)
        remover.wait()
        self.assertGreaterEqual(
            time.monotonic() - started, 1.5, "the step did not wait for the pending launch"
        )
        self.assertNotIn("still measuring", run.stderr)
        (pending / "codex-2-def").write_text("")
        started = time.monotonic()
        run = subprocess.run(
            [BASH, str(ROOT / "scripts" / "e2e-network-budget.sh"), "codex"],
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "OH_E2E_TELEMETRY_DIR": str(tmp),
                "OH_E2E_OS": "Linux",
                "OH_E2E_PENDING_WAIT": "1",
            },
            check=False,
        )
        self.assertLess(time.monotonic() - started, 60)
        self.assertIn("still measuring after 1s: codex-2-def", run.stderr)
        self.assertNotEqual(run.returncode, 0, "a check over no telemetry passed")

    def test_the_budget_step_reads_a_windows_form_directory_as_that_directory(self) -> None:
        # Git Bash hands the step a drive-letter path; joined onto the working
        # directory it named D:\repo\C:\Users\... and waited on nothing.
        tmp = scratch(self)
        windows_form = r"C:\oh-net-test-absent\telemetry"
        run = subprocess.run(
            [BASH, str(ROOT / "scripts" / "e2e-network-budget.sh"), "codex"],
            capture_output=True,
            text=True,
            cwd=tmp,
            env={**os.environ, "OH_E2E_TELEMETRY_DIR": windows_form, "OH_E2E_OS": "Linux"},
            timeout=600,
            check=False,
        )
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn(f"no telemetry at {windows_form}", run.stdout)
        self.assertNotIn(tmp.name, run.stdout + run.stderr, "the directory was joined onto the cwd")

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
