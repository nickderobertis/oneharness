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
import shlex
import shutil
import signal
import subprocess
import sys
import time
import unittest
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import NamedTuple

from support import (
    BASH,
    ROOT,
    WINDOWS,
    LaunchId,
    check_step,
    lines,
    measurements,
    need_builds,
    need_recorder,
    path_without,
    recorder_unavailable,
    run_shim,
    run_suite,
    scratch,
    starts,
    suite_env,
    transfer_argv,
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


# llmlint: ignore-block[shared_internals_are_a_project_not_a_reach_in] scripts/e2e-netmeasure.py is this project's own code under test, not a sibling's internals: network-telemetry declares it as an input and formats, lints and type-checks it in its own targets, and its existing tests load it by path the same way (test_recorder's netmeasure_module). The hang guard must be derived from the bounds the recorder itself enforces, so it reads them from the recorder rather than restating them.
def recorder_bounds() -> ModuleType:
    """The recorder's own module, loaded once from its script, for the bounds
    it enforces on itself."""
    name = "netmeasure_bounds"
    if name not in sys.modules:
        spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / "e2e-netmeasure.py")
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        # Its dataclasses resolve their annotations through sys.modules.
        sys.modules[name] = module
        spec.loader.exec_module(module)
    return sys.modules[name]


# llmlint: ignore-end[shared_internals_are_a_project_not_a_reach_in]


def recorder_lifetime() -> float:
    """The longest a stopped launch's recorder can take to write its line, by
    the recorder's own constants: every inbound-TCP trial timing out, then the
    stopped tree's kill grace — or a direct launch's grace and reaping — and
    the counters' settle."""
    bounds = recorder_bounds()
    return float(
        len(bounds.INBOUND_TCP) * bounds.TRIAL_TIMEOUT
        + bounds.KILL_GRACE
        + bounds.REAP_WAIT
        + bounds.SETTLE
    )


# The one fixed bound on waiting for a stopped suite's launches to be
# recorded: a hang guard against a recorder that never exits. Twice the
# recorder's own maximum, so a loaded runner's scheduling cannot make it
# reject a launch the recorder can still record.
HANG_GUARD = 2 * recorder_lifetime()


# A pid as a marker spells it: ASCII decimal, positive — 0 would name a
# process group, not a recorder — and within pid_t.
PID = re.compile(r"[1-9][0-9]{0,9}")
PID_MAX = 2**31 - 1


def owners(marker: Path) -> list[int] | None:
    """The pids a pending marker names as able to still record its launch, or
    None while that is not known: the marker is still being written, is no
    longer there to read, or names anything but pids."""
    try:
        words = marker.read_text().split()
    except OSError:
        return None
    pids = [int(word) for word in words if PID.fullmatch(word) and int(word) <= PID_MAX]
    return pids if pids and len(pids) == len(words) else None


def running(pid: int) -> bool:
    """Whether `pid` is still running; a zombie has already exited."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except FileNotFoundError:
        # Gone between the two looks where there is a procfs; without one
        # (macOS), kill's answer stands.
        return not Path("/proc/self/stat").exists()
    except OSError:
        return True
    return stat.rsplit(")", 1)[-1].split()[:1] not in (["Z"], ["X"])


def wait_until_recorded(telemetry: Path, suite: str) -> None:
    """Return once every launch the stopped suite started is recorded — its
    pending marker gone — however long the recorder takes. Fail at once, naming
    them, on the launches whose markers outlived every recorder process they
    name, which can no longer record them, whatever other launches are still
    being recorded; and at HANG_GUARD on a recorder that has not exited."""
    pending = telemetry / ".pending"
    deadline = time.monotonic() + HANG_GUARD
    while True:
        markers = sorted(pending.glob("*"))
        if not markers:
            return
        named = {marker.name: owners(marker) for marker in markers}
        # Looked for again once its owners are seen gone: a recorder removes
        # the marker before it exits, so one still there now is lost.
        lost = {
            launch: pids
            for launch, pids in named.items()
            if pids is not None
            and not any(running(pid) for pid in pids)
            and (pending / launch).exists()
        }
        if lost:
            # Each lost launch's start line, so the failure says which launch
            # (phase, kind, argv) went unrecorded, not only its id.
            launched = [
                s for s in starts(lines(telemetry / f"{suite}.jsonl")) if s["launch_id"] in lost
            ]
            raise AssertionError(
                f"{suite}: launch(es) {', '.join(lost)} never recorded: every recorder process "
                f"their pending markers name ({lost}) has exited with the marker still there; "
                f"their start lines: {launched}"
            )
        if time.monotonic() >= deadline:
            raise AssertionError(
                f"{suite}: launch(es) {', '.join(named)} still pending {HANG_GUARD:.0f} s after "
                f"the suite stopped, over the recorder's own {recorder_lifetime():.0f} s maximum "
                f"(the pids each marker names: {named})"
            )
        time.sleep(0.1)


def run_until_recorded(suite: str, env: dict[str, str], harness: str, limit: float = 120) -> str:
    """Run a suite until it has recorded a measured launch of `harness`, or
    exits; then stop it — by its own process group, as a runner would — wait
    until the recorder has recorded every launch it started
    (`wait_until_recorded`), and return its stderr. A first phase the double
    cannot satisfy may otherwise wait out minutes of live-tier timeouts nobody
    here is testing."""
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
    # supervisor, in its own session, once its namespace has wound down.
    wait_until_recorded(telemetry.parent, suite)
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

    def test_a_cursor_turn_that_leaves_its_worker_running_passes_the_echo_phase(self) -> None:
        # cursor-agent 2026.11.x exits a few seconds into a turn but leaves its
        # `worker-server` running for later invocations. The measuring
        # namespace once waited for that service too, so the shim outlived the
        # turn by the namespace's deadline and every live cursor run timed out
        # with its answer already printed. The turn must end with the CLI.
        need_builds(self)
        need_recorder(self)
        tmp = scratch(self)
        resident = tmp / "resident"
        timeout = 30
        env = suite_env(
            tmp,
            *EXECUTABLES,
            OH_E2E_PHASE_LIMIT="1",
            OH_TEST_RESIDENT=str(resident),
            OH_TIMEOUT=str(timeout),
            **SUITES["cursor"].auth,
        )
        started = time.monotonic()
        run = run_suite("cursor", env, timeout=timeout * 3)
        took = time.monotonic() - started
        self.assertEqual(run.returncode, 0, run.stderr[-3000:])
        self.assertIn("ok: cursor ran and exited cleanly (status=ok, exit_code=0", run.stderr)
        self.assertIn("confirmed: marker surfaced", run.stderr)
        self.assertTrue(resident.exists(), "the double left no resident service running")
        # The service got the measurer's grace after the CLI exited, and no more.
        self.assertLess(took, timeout, "the turn waited on the service the CLI left running")
        size = resident.stat().st_size
        time.sleep(0.5)
        self.assertEqual(resident.stat().st_size, size, "the resident service outlived the turn")
        records = lines(tmp / "telemetry" / "cursor.jsonl")
        (turn,) = [s for s in starts(records) if s["kind"] == "turn"]
        (line,) = measurements(records, turn["launch_id"])
        self.assertEqual((line["method"], line["exit"]), ("netns-pasta", 0), line)

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


# How long the stand-in pasta holds a stopped suite's later namespaces open
# before letting their recorder finish: past the 8 s the helper once allowed
# and the budget step's 30 s OH_E2E_PENDING_WAIT alike, so no fixed wait of
# that order passes it.
HOLD = 35


class StoppedSuite(NamedTuple):
    """The control suite set up to run behind the stand-in pasta."""

    telemetry: Path  # its OH_E2E_TELEMETRY_DIR
    env: dict[str, str]


class Failure(NamedTuple):
    """The helper's failure, as a test reads it."""

    said: str
    took: float  # seconds from the suite's start to the failure


# What the stand-in pasta does with a namespace: start it as pasta itself
# would, or SIGKILL its measurer and supervisor before it starts, so nothing
# is left that can write its line or clear its marker.
THROUGH = 'exec "$real" "$@"'
LOSE = 'kill -KILL "$(ps -o ppid= -p "$PPID" | tr -d " ")" "$PPID"; exit 1'


@unittest.skipIf(
    WINDOWS, "the suites' doubles are bash scripts; Windows runs the recorder's fallback tests"
)
class StoppedSuiteRecording(unittest.TestCase):
    """The control suite stopped while its recorder still owes launches their
    lines, through the real shim, recorder and pasta: a stand-in pasta in front
    of the real one does `first` with the first namespace and `later` with
    every one after it — among them the pooled `--version` probe started
    beside the first, still in flight when the suite is stopped."""

    def stopped_with(self, later: str, first: str = THROUGH) -> StoppedSuite:
        need_builds(self)
        need_recorder(self)
        real = shutil.which("pasta")
        assert real is not None  # need_recorder measured through it
        tmp = scratch(self)
        stand_in = tmp / "pasta-bin"
        stand_in.mkdir()
        claimed = shlex.quote(str(tmp / "first-namespace"))
        (stand_in / "pasta").write_text(
            "#!/usr/bin/env bash\n"
            f"real={shlex.quote(real)}\n"
            "# The recorder's inbound-TCP trials run `:`; they are pasta's own.\n"
            f'if [ "${{*: -1}}" = : ]; then {THROUGH}; fi\n'
            f"if mkdir {claimed} 2>/dev/null; then {first}; fi\n"
            f"{later}\n"
        )
        (stand_in / "pasta").chmod(0o755)
        env = suite_env(tmp, *EXECUTABLES, OH_E2E_PHASE_LIMIT="1", **SUITES["control"].auth)
        env["PATH"] = f"{stand_in}{os.pathsep}{env['PATH']}"
        return StoppedSuite(tmp / "telemetry", env)

    def test_a_launch_recorded_long_after_the_stop_is_waited_for(self) -> None:
        stopped = self.stopped_with(f'"$real" "$@"; code=$?; sleep {HOLD}; exit "$code"')
        began = time.monotonic()
        run_until_recorded("control", stopped.env, "codex")
        took = time.monotonic() - began
        records = lines(stopped.telemetry / "control.jsonl")
        launched = starts(records)
        self.assertGreater(len(launched), 1, records)
        for launch in launched:
            self.assertEqual(len(measurements(records, launch["launch_id"])), 1, launch)
        # A namespace was held past both fixed waits, and still recorded.
        self.assertGreater(took, HOLD, "no namespace was held: the stand-in tested nothing")

    def failure(self, stopped: StoppedSuite) -> Failure:
        """The helper's failure on `stopped`, and how long it took to come."""
        began = time.monotonic()
        with self.assertRaises(AssertionError) as caught:
            run_until_recorded("control", stopped.env, "codex")
        return Failure(str(caught.exception), time.monotonic() - began)

    def assert_names_only_lost_launches(self, stopped: StoppedSuite, said: str) -> list[LaunchId]:
        """The launch ids `said` names as never recorded, each one unrecorded
        with every recorder process its marker names gone."""
        self.assertIn("never recorded", said)
        named = [
            LaunchId(str(found))
            for found in sorted(set(re.findall(r"control-[0-9]+-[0-9a-f]{12}", said)))
        ]
        self.assertTrue(named, said)
        records = lines(stopped.telemetry / "control.jsonl")
        for launch_id in named:
            self.assertEqual(measurements(records, launch_id), [], launch_id)
            # Its start line is quoted, so the failure says which launch it was.
            self.assertIn(f"'launch_id': '{launch_id}'", said)
            pids = owners(stopped.telemetry / ".pending" / launch_id)
            self.assertTrue(pids, launch_id)
            assert pids is not None
            self.assertFalse(any(running(pid) for pid in pids), (launch_id, pids))
        return named

    def test_a_launch_whose_recorder_exits_without_its_line_fails_naming_it(self) -> None:
        stopped = self.stopped_with(LOSE)
        failed = self.failure(stopped)
        self.assert_names_only_lost_launches(stopped, failed.said)
        # Failed on its recorder's exit, not on the hang guard.
        self.assertLess(failed.took, HANG_GUARD, failed.said)

    def test_a_lost_launch_fails_while_another_is_still_being_recorded(self) -> None:
        # The two pooled probes start together: the first held open, its
        # supervisor recorded, until the test releases it; the other lost.
        tmp = scratch(self)
        held, release = tmp / "held-supervisor", tmp / "release"
        stopped = self.stopped_with(
            LOSE,
            first=(
                f'echo "$PPID" > {shlex.quote(str(held))}; "$real" "$@"; code=$?; '
                f"for _ in $(seq {HOLD * 10}); do [ -e {shlex.quote(str(release))} ] && break; "
                'sleep 0.1; done; exit "$code"'
            ),
        )
        self.addCleanup(release.touch)
        failed = self.failure(stopped)
        supervisor = int(held.read_text())
        # The failure came while the held launch's recorder was still at work.
        self.assertTrue(running(supervisor), failed.said)
        self.assertLess(failed.took, HOLD, failed.said)
        named = self.assert_names_only_lost_launches(stopped, failed.said)
        markers = (stopped.telemetry / ".pending").glob("*")
        recording = [LaunchId(m.name) for m in markers if supervisor in (owners(m) or [])]
        self.assertEqual(len(recording), 1, recording)
        self.assertNotIn(recording[0], named)
        # Released, it is still recorded: its marker goes with its line.
        release.touch()
        deadline = time.monotonic() + HANG_GUARD
        while running(supervisor) and time.monotonic() < deadline:
            time.sleep(0.1)
        self.assertFalse(running(supervisor), "the held recorder never finished")
        records = lines(stopped.telemetry / "control.jsonl")
        self.assertEqual(len(measurements(records, recording[0])), 1, records)


class HangGuard(unittest.TestCase):
    def test_the_hang_guard_outlasts_the_recorder_s_own_longest_recording(self) -> None:
        bounds = recorder_bounds()
        floor = len(bounds.INBOUND_TCP) * bounds.TRIAL_TIMEOUT + bounds.KILL_GRACE
        self.assertGreater(HANG_GUARD, floor + bounds.REAP_WAIT + bounds.SETTLE)
        self.assertGreater(HANG_GUARD, HOLD)


class MarkerSeen(NamedTuple):
    """A pending marker's owners as its launch read them, beside its parents."""

    owners: list[int] | None
    parent: int  # the process that started the launch
    grandparent: int  # that process's parent


@unittest.skipIf(WINDOWS, "the launch reads its parents with ps, and the recorder forks")
class PendingMarkerOwners(unittest.TestCase):
    """A launch's pending marker names the processes that can still record it,
    as the launch itself sees them while it runs, through the real shim."""

    def marker_and_parents(self, env: dict[str, str]) -> MarkerSeen:
        tmp = scratch(self)
        real = tmp / "real-codex"
        real.write_text(
            "#!/usr/bin/env bash\n"
            f"cp {shlex.quote(str(tmp))}/.pending/* {shlex.quote(str(tmp / 'marker'))}\n"
            f'echo "$PPID" > {shlex.quote(str(tmp / "parent"))}\n'
            f'ps -o ppid= -p "$PPID" | tr -d " " > {shlex.quote(str(tmp / "grandparent"))}\n'
        )
        real.chmod(0o755)
        run = run_shim(tmp, [str(real)], ["exec", "x"], env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertFalse(list((tmp / ".pending").glob("*")), "the marker outlived the line")
        return MarkerSeen(
            owners(tmp / "marker"),
            int((tmp / "parent").read_text()),
            int((tmp / "grandparent").read_text()),
        )

    def test_the_recorder_s_marker_names_its_supervisor_then_its_measurer(self) -> None:
        # Unmeasured, so the launch is the supervisor's own child on any host.
        seen = self.marker_and_parents({"OH_E2E_OS": "Darwin"})
        # The supervisor started the launch; the measurer forked the supervisor.
        self.assertEqual(seen.owners, [seen.parent, seen.grandparent])

    def test_the_shim_s_fallback_marker_names_the_shell_that_records_the_launch(self) -> None:
        tmp = scratch(self)
        seen = self.marker_and_parents({"PATH": path_without(tmp, "python3")})
        self.assertEqual(seen.owners, [seen.parent])


def setup_and(
    tmp: Path,
    script: str,
    *,
    path_first: Path | None = None,
    telemetry_dir: str | None = None,
    extra: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """oh_telemetry_setup's library sourced in the test's scratch, then `script`."""
    env = {**suite_env(tmp, "codex"), **(extra or {})}
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


def plain_codex(directory: Path) -> None:
    """A `codex` that exits 0, standing for the harness the setup shims."""
    directory.mkdir(exist_ok=True)
    (directory / "codex").write_text("#!/usr/bin/env bash\nexit 0\n")
    (directory / "codex").chmod(0o755)


class WindowsFormDirectory(unittest.TestCase):
    """A drive-letter OH_E2E_TELEMETRY_DIR is that directory, never joined onto
    the cwd, and its shims still go first on Git Bash's PATH."""

    def assert_recorded_in(self, run: subprocess.CompletedProcess[str], directory: Path) -> None:
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertTrue((directory / ".shims" / "codex" / "codex").is_file(), run.stderr)
        records = lines(directory / "codex.jsonl")
        (start,) = starts(records)
        self.assertEqual(start["argv"], ["--version"])
        self.assertEqual(len(measurements(records, start["launch_id"])), 1)

    @unittest.skipUnless(WINDOWS, "Git Bash's own cygpath and drive letters")
    def test_on_windows_setup_puts_its_shims_first_and_records_in_that_directory(self) -> None:
        tmp = scratch(self)
        plain_codex(tmp / "real-bin")
        directory = tmp / "telemetry-win"
        run = setup_and(
            tmp,
            "oh_telemetry_setup codex; codex --version",
            path_first=tmp / "real-bin",
            telemetry_dir=str(directory),
        )
        self.assertRegex(str(directory), r"^[A-Za-z]:\\")
        self.assert_recorded_in(run, directory)

    @unittest.skipIf(WINDOWS, "names the Windows form through a stand-in cygpath on unix")
    def test_setup_converts_a_drive_letter_directory_through_cygpath(self) -> None:
        # The stand-in maps drive C: into the scratch, as Git Bash's maps it to /c.
        tmp = scratch(self)
        win_bin = tmp / "win-bin"
        plain_codex(win_bin)
        (win_bin / "cygpath").write_text(
            "#!/usr/bin/env bash\n"
            'if [ "$1" = -u ]; then p="${2#[A-Za-z]:}"; '
            f'printf "%s\\n" "{tmp}/c-drive${{p//\\\\//}}"; '
            'else printf "W:%s" "${@: -1}"; fi\n'
        )
        (win_bin / "cygpath").chmod(0o755)
        run = setup_and(
            tmp,
            "oh_telemetry_setup codex; codex --version",
            path_first=win_bin,
            telemetry_dir=r"C:\oh-telemetry",
        )
        self.assert_recorded_in(run, tmp / "c-drive" / "oh-telemetry")

    @unittest.skipIf(WINDOWS, "names the Windows form through a stand-in cygpath on unix")
    def test_a_drive_letter_directory_cygpath_cannot_convert_stops_the_setup(self) -> None:
        for name, body in (("fails", "exit 1"), ("prints nothing", "exit 0")):
            with self.subTest(cygpath=name):
                tmp = scratch(self)
                win_bin = tmp / "win-bin"
                plain_codex(win_bin)
                (win_bin / "cygpath").write_text(f"#!/usr/bin/env bash\n{body}\n")
                (win_bin / "cygpath").chmod(0o755)
                run = setup_and(
                    tmp,
                    "oh_telemetry_setup codex; codex --version",
                    path_first=win_bin,
                    telemetry_dir=r"C:\oh-telemetry",
                )
                self.assertNotEqual(run.returncode, 0, run.stderr)
                self.assertIn(
                    r"cannot convert the telemetry directory C:\oh-telemetry to a Git Bash path",
                    run.stderr,
                )
                self.assertFalse((tmp / "C:\\oh-telemetry").exists(), "setup went on regardless")

    @unittest.skipIf(WINDOWS, "spells the Windows form without Git Bash on unix")
    def test_without_cygpath_the_directory_keeps_its_spelling(self) -> None:
        # Unix reads C:\... as relative, so the shims land under the cwd (the
        # scratch) here; what matters is that the shim records to and names the
        # directory as given, never prefixed with the cwd. Its colon splits
        # PATH on unix, so the PATH check that follows is Windows's own.
        tmp = scratch(self)
        windows_form = r"C:\oh-telemetry"
        setup_and(tmp, "oh_telemetry_setup codex", telemetry_dir=windows_form)
        shim = (tmp / windows_form / ".shims" / "codex" / "codex").read_text()
        quoted = windows_form.replace("\\", "\\\\")
        self.assertIn(f"--file {quoted}/codex.jsonl ", shim)
        self.assertIn(f"--self {quoted}/.shims/codex/codex ", shim)
        self.assertNotIn(
            str(tmp), shim.split("--self", 1)[1], "the directory was joined onto the cwd"
        )


class GraceCase(NamedTuple):
    """One launch the shared-server grace test runs under a suite's setup."""

    suite: str  # the suite whose telemetry file records the launch
    setup: str  # the oh_telemetry_setup line that suite sources
    argv: list[str]  # the codex arguments the launch runs
    keeps_leftovers: bool  # whether the measurer leaves its detached command running


@unittest.skipIf(WINDOWS, "the doubles are bash scripts")
class Setup(unittest.TestCase):
    """oh_telemetry_setup itself, driven as a suite drives it."""

    def test_a_suite_started_under_another_suites_shims_wraps_the_real_executable(self) -> None:
        need_builds(self)
        tmp = scratch(self)
        lib = ROOT / "scripts" / "e2e-lib.sh"
        run = setup_and(
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

    def test_only_the_control_suite_s_shared_servers_keep_what_they_leave_running(self) -> None:
        # pooled-kill proves the pool itself stops a killed dispatch's
        # commands; a command a shared server's stop left running is the
        # defect it exists to catch. The measurer's descendant grace (#1603)
        # reaped such a command 5 s after the server exited, so on a measuring
        # runner the escape-marker fault passed. The control suite's own setup
        # exempts its shared-server launches; its other launches, and every
        # other suite's shared servers, keep the grace.
        #
        # Each case reads the recorder's own record — the grace its init held
        # the launch to (`descendant_grace`) and why it stopped the tree
        # (`stopped`) — never how long anything took: a loaded runner
        # stretches the setup, the shim and every interpreter's start around
        # the grace (11.9 s against a 1 s grace on one CI run), but cannot
        # change either. No case has a deadline, so no stop but the grace can
        # fire, and the double's launch exits only once its leftover is
        # beating. A kept leftover is given no grace and ends by itself,
        # unstopped, whenever that is. A reaped one is given the grace and
        # stopped for it; its own lifetime is only the bound on a broken
        # tree's hang, and the grace runs from the launch's exit, after the
        # leftover has started, so no setup or interpreter start counts
        # against it.
        need_builds(self)
        need_recorder(self)
        control = re.search(
            r"^oh_telemetry_setup control\b.*$",
            (ROOT / "scripts" / "e2e-control.sh").read_text(),
            re.MULTILINE,
        )
        assert control is not None, "scripts/e2e-control.sh no longer sets up its recorder"
        grace, hang_bound = 1, 60
        server = ["app-server", "--listen", "unix:///nonexistent/app.sock"]
        for case in (
            GraceCase("control", control.group(0), server, keeps_leftovers=True),
            GraceCase("control", control.group(0), ["exec", "stay"], keeps_leftovers=False),
            GraceCase("codex", "oh_telemetry_setup codex", server, keeps_leftovers=False),
        ):
            suite, setup, args = case.suite, case.setup, case.argv
            with self.subTest(suite=suite, launch=args[0]):
                tmp = scratch(self)
                heartbeat = tmp / "heartbeat"
                # A codex whose launch exits as soon as it has left a
                # detached command beating, which lingers until something
                # stops it or, at the latest, until it ends by itself.
                real = tmp / "real-bin"
                real.mkdir()
                linger = 2 if case.keeps_leftovers else hang_bound
                words = transfer_argv(0, 0, 0, "--detach", "--linger", str(linger))
                (real / "codex").write_text(
                    f'#!/usr/bin/env bash\nexec {shlex.join(words)} --heartbeat "$OH_TEST_BEAT"\n'
                )
                (real / "codex").chmod(0o755)
                run = setup_and(
                    tmp,
                    f"{setup}; codex {shlex.join(args)}",
                    path_first=real,
                    extra={
                        "OH_TEST_BEAT": str(heartbeat),
                        "OH_E2E_NET_DESCENDANT_GRACE": str(grace),
                        # No deadline: nothing but the grace can stop a leftover.
                        "OH_E2E_NET_DEADLINE": "0",
                    },
                )
                self.assertEqual(run.returncode, 0, run.stderr)
                self.assertTrue(heartbeat.read_text(), "the detached command never ran")
                size = heartbeat.stat().st_size
                time.sleep(0.5)
                self.assertEqual(heartbeat.stat().st_size, size, "the command is still alive")
                records = lines(tmp / "telemetry" / f"{suite}.jsonl")
                (start,) = starts(records)
                (line,) = measurements(records, start["launch_id"])
                self.assertEqual(line["method"], "netns-pasta", line)
                if case.keeps_leftovers:
                    self.assertNotIn("descendant_grace", line, "the server was given a grace")
                    self.assertNotIn("stopped", line, "the server's command was reaped")
                else:
                    self.assertEqual(
                        line.get("descendant_grace"), grace, "the launch kept no grace"
                    )
                    self.assertEqual(
                        line.get("stopped"), "descendant-grace", "its command was not reaped"
                    )

    def test_a_shared_server_grace_that_is_no_number_stops_the_setup(self) -> None:
        need_builds(self)
        for given in ("--shared-server-grace soon", "--shared-server-grace", "--grace 0"):
            with self.subTest(given=given):
                tmp = scratch(self)
                run = setup_and(tmp, f"oh_telemetry_setup control {given}; codex --version")
                self.assertNotEqual(run.returncode, 0)
                self.assertIn("oh_telemetry_setup control: ", run.stderr)
                self.assertFalse((tmp / "telemetry" / "control.jsonl").exists(), run.stderr)

    def test_a_stale_shim_of_the_same_suite_is_refused_when_it_cannot_be_rewritten(self) -> None:
        # The last run's shim (same suite, same marker) wraps an executable
        # that has since moved; this run cannot rewrite it, and must not take
        # the old one for its own.
        need_builds(self)
        tmp = scratch(self)
        self.assertEqual(setup_and(tmp, "oh_telemetry_setup again").returncode, 0)
        moved = tmp / "moved-bin"
        moved.mkdir()
        (moved / "codex").write_text("#!/usr/bin/env bash\nexit 0\n")
        (moved / "codex").chmod(0o755)
        shims = tmp / "telemetry" / ".shims" / "again"
        shims.chmod(0o555)
        self.addCleanup(shims.chmod, 0o755)
        run = setup_and(tmp, "oh_telemetry_setup again", path_first=moved)
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
        run = setup_and(tmp, "oh_telemetry_setup winleg", path_first=stand_in)
        self.assertEqual(run.returncode, 0, run.stderr)
        shims = tmp / "telemetry" / ".shims" / "winleg"
        adapter = (shims / "codex.cmd").read_bytes()
        bash = subprocess.run(
            [BASH, "-c", "command -v bash"], capture_output=True, text=True
        ).stdout.strip()
        self.assertEqual(adapter, f'@"W:{bash}" "W:{shims / "codex"}" %*\r\n'.encode())
        stale = setup_and(
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

    def test_a_bad_deadline_grace_or_observer_is_refused_before_the_launch(self) -> None:
        for name, value, said in (
            ("OH_E2E_NET_DEADLINE", "-5", "must be a finite, non-negative number of seconds"),
            ("OH_E2E_NET_DEADLINE", "nan", "must be a finite, non-negative number of seconds"),
            ("OH_E2E_NET_DEADLINE", "soon", "must be a finite, non-negative number of seconds"),
            (
                "OH_E2E_NET_DESCENDANT_GRACE",
                "-1",
                "must be a finite, non-negative number of seconds",
            ),
            (
                "OH_E2E_NET_DESCENDANT_GRACE",
                "inf",
                "must be a finite, non-negative number of seconds",
            ),
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

    def netmeasure(self) -> ModuleType:
        """The recorder's own module, loaded from its script."""
        spec = importlib.util.spec_from_file_location(
            "netmeasure", ROOT / "scripts" / "e2e-netmeasure.py"
        )
        assert spec is not None and spec.loader is not None
        netmeasure = importlib.util.module_from_spec(spec)
        # Its dataclasses resolve their annotations through sys.modules.
        sys.modules["netmeasure"] = netmeasure
        self.addCleanup(sys.modules.pop, "netmeasure", None)
        spec.loader.exec_module(netmeasure)
        return netmeasure

    def test_a_malformed_namespace_result_never_becomes_a_measurement(self) -> None:
        netmeasure = self.netmeasure()
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
        self.assertIsNone(
            netmeasure.NamespaceResult.from_json(json.dumps(good)).line("x").descendant_grace
        )
        reaped = netmeasure.NamespaceResult.from_json(
            json.dumps({**good, "stopped": "descendant-grace", "descendant_grace": 1.5})
        )
        self.assertEqual(reaped.line("x").stopped, "descendant-grace")
        self.assertEqual(reaped.line("x").descendant_grace, 1.5)
        self.assertEqual(netmeasure.NamespaceResult.from_json(reaped.to_json()), reaped)
        for name, change in (
            ("a negative count", {"moved": [-1, 1, 5, 1]}),
            ("a boolean count", {"moved": [True, 1, 5, 1]}),
            ("three counters", {"moved": [10, 1, 5]}),
            ("a boolean duration", {"seconds": True}),
            ("an infinite duration", {"seconds": float("inf")}),
            ("a fractional exit", {"exit": 1.5}),
            ("an exit that would wrap to success", {"exit": 256}),
            ("a negative exit", {"exit": -1}),
            ("a window that ends before it begins", {"start": {"before_ns": 5, "after_ns": 2}}),
            ("windows out of order", {"start": {"before_ns": 6, "after_ns": 7}}),
            ("a window that is not an object", {"end": [3, 4]}),
            ("a stop the init never makes", {"stopped": "tired"}),
            ("a negative grace", {"descendant_grace": -1}),
            ("a grace that is no number", {"descendant_grace": "5"}),
        ):
            with self.subTest(result=name):
                with self.assertRaises(ValueError):
                    netmeasure.NamespaceResult.from_json(json.dumps({**good, **change}))

    def test_a_supervisor_report_with_an_out_of_range_exit_is_no_report(self) -> None:
        netmeasure = self.netmeasure()
        report = {"ending": "exited", "reason": ""}
        for code in (0, 137, 255):
            with self.subTest(code=code):
                outcome = netmeasure.Outcome.decode(json.dumps({**report, "code": code}).encode())
                self.assertIsNotNone(outcome)
                self.assertEqual(outcome.code, code)
        for code in (256, -1, True, "0"):
            with self.subTest(code=code):
                self.assertIsNone(
                    netmeasure.Outcome.decode(json.dumps({**report, "code": code}).encode())
                )


# What the signal module offers on Windows; any other name read at import time
# breaks test discovery on the Windows leg before a skip can apply.
WINDOWS_SIGNALS = ("SIGABRT", "SIGBREAK", "SIGFPE", "SIGILL", "SIGINT", "SIGSEGV", "SIGTERM")
IMPORT_WITHOUT_POSIX_APIS = """
import ctypes, importlib.util, os, pathlib, signal, sys
# Load ctypes for the host before simulating Git Bash's Windows platform.
sys.platform = "cygwin"
if hasattr(os, "geteuid"):
    delattr(os, "geteuid")
for name in [n for n in dir(signal) if n.startswith("SIG") and not n.startswith("SIG_")]:
    if name not in sys.argv[1].split(","):
        delattr(signal, name)
for name in ("pthread_sigmask", "sigpending", "alarm", "pause"):
    if hasattr(signal, name):
        delattr(signal, name)
for path in sys.argv[2:]:
    path = pathlib.Path(path)
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location(path.stem.replace("-", "_"), path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
"""


class ImportsOnWindows(unittest.TestCase):
    def test_every_test_module_and_the_measurer_import_without_posix_only_apis(self) -> None:
        here = Path(__file__).resolve().parent
        modules = [*sorted(here.glob("test_*.py")), ROOT / "scripts" / "e2e-netmeasure.py"]
        run = subprocess.run(
            [
                sys.executable,
                "-I",
                "-c",
                IMPORT_WITHOUT_POSIX_APIS,
                ",".join(WINDOWS_SIGNALS),
                *map(str, modules),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(run.returncode, 0, run.stderr)


class Legs(unittest.TestCase):
    PREPARE = "Permit unprivileged namespaces for pasta"
    PREFLIGHT = "Prove the network recorder measures in pasta's namespace"

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
                        # That probe passed on 26.04 while its confined pasta
                        # refused the recorder's python: the recorder itself
                        # must measure a launch before anything relies on it.
                        preflight = named(run, self.PREFLIGHT)
                        self.assertGreater(
                            preflight, prepare, "the recorder is never proven, or before the lift"
                        )
                        self.assertLess(
                            preflight, measure, "the recorder is proven after it is needed"
                        )
                        self.assertEqual(
                            run[preflight].run.strip(), "bash scripts/network-recorder-preflight.sh"
                        )
                    else:
                        self.assertEqual(
                            prepare, -1, f"{runner_os} has no AppArmor userns restriction to lift"
                        )
                        self.assertEqual(
                            named(run, self.PREFLIGHT), -1, f"{runner_os} measures nothing"
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
        cwd = tmp / "elsewhere"
        cwd.mkdir()
        # Unix has no drive letters, so there the spelling stands relative to
        # the scratch cwd; Windows names an absent directory inside the scratch.
        windows_form = str(tmp / "absent" / "telemetry") if WINDOWS else r"C:\absent\telemetry"
        run = subprocess.run(
            [BASH, str(ROOT / "scripts" / "e2e-network-budget.sh"), "codex"],
            capture_output=True,
            text=True,
            cwd=cwd,
            env={**os.environ, "OH_E2E_TELEMETRY_DIR": windows_form, "OH_E2E_OS": "Linux"},
            timeout=600,
            check=False,
        )
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn(f"no telemetry at {windows_form}", run.stdout)
        self.assertNotIn(str(cwd), run.stdout + run.stderr, "the directory was joined onto the cwd")

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
