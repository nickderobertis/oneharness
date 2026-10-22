"""The scratch guard's own regression: cleanup a failing test cannot skip."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest import mock

from . import package_e2e
from . import scratch as scratch_module
from .scratch import PREFIX, RUN_ENV, RUN_MARKER, control_scratch, scratch

# Makes and removes scratch directories through both makers in a loop, then
# leaves one behind as a real leak would, for the observer below to watch from
# another process the way a sibling run's leak gate does.
CHURN = textwrap.dedent(
    """
    import sys, unittest
    from contextlib import ExitStack
    from test import package_e2e
    from test.scratch import scratch

    case = unittest.TestCase()
    for cycle in range(int(sys.argv[1])):
        scratch(case, f"churn-{cycle}")
        case.doCleanups()
        with ExitStack() as stack:
            package_e2e.scratch_dir(stack, f"churn-e2e-{cycle}")
    scratch(unittest.TestCase(), "leaked")
    """
)


def entries(root: Path) -> list[str]:
    """What ``root`` holds besides the leak gate's marker for this run, if any."""
    return sorted(name for name in os.listdir(root) if name != RUN_MARKER)


class ScratchTests(unittest.TestCase):
    """Prove the teardown runs for a case that fails, not only for one that passes."""

    def test_a_failing_case_still_gives_back_its_scratch_directory(self) -> None:
        """Run a real failing case through unittest and inspect what it left.

        The teardown that matters runs after a test body has already failed, so
        nothing inside a passing test can watch it happen. This is the regression
        guard for the shape that leaked one directory per case, every run, onto
        the host.
        """
        taken: list[Path] = []

        class Failing(unittest.TestCase):
            def runTest(self) -> None:  # unittest's own spelling
                taken.append(scratch(self, "cleanup-probe"))
                self.fail("the failing test this stands in for")

        result = Failing().run()

        self.assertIsNotNone(result)
        assert result is not None
        self.assertFalse(result.wasSuccessful(), "the case must really have failed")
        self.assertEqual(len(taken), 1)
        self.assertFalse(
            taken[0].exists(), f"a failing case left its scratch directory: {taken[0]}"
        )

    def test_scratch_names_carry_the_prefix_the_leak_gate_sweeps_for(self) -> None:
        """`scripts/check-temp-leaks.sh` sweeps for `io::scratch::PREFIX`.

        These names have to start with it or the sweep passes while the
        directories pile up. `scripts/check-scratch-prefixes.sh` holds the two in
        step across the language boundary; this asserts the names really use it.
        """
        directory = scratch(self, "prefix-probe")
        self.assertTrue(directory.is_dir())
        self.assertTrue(directory.name.startswith(PREFIX), directory.name)

    def test_scratch_names_end_in_the_id_of_the_process_that_made_them(self) -> None:
        """The leak gate reads that suffix to leave another checkout's live
        directory out of its verdict; a name without it counts as this run's leak."""
        directory = scratch(self, "pid-probe")
        self.assertTrue(directory.name.endswith(f"-{os.getpid()}"), directory.name)

    def test_every_scratch_directory_made_under_the_leak_gate_names_its_run(self) -> None:
        """`scripts/check-temp-leaks.sh` reads that marker to tell this run's
        scratch from a sibling run's without reading this process's environment,
        which it cannot do on Windows. Each maker here writes it, including the
        package journey's, which makes its directory on its own."""
        with (
            mock.patch.dict(os.environ, {RUN_ENV: "4242.python-probe"}),
            ExitStack() as stack,
        ):
            made = [
                scratch(self, "marker-probe"),
                control_scratch(self, "mark"),
                package_e2e.scratch_dir(stack, "marker-probe"),
            ]
            for directory in made:
                self.assertEqual(
                    (directory / RUN_MARKER).read_text(encoding="utf-8"), "4242.python-probe"
                )

    def test_a_scratch_directory_made_with_no_gate_watching_carries_no_marker(self) -> None:
        with mock.patch.dict(os.environ), ExitStack() as stack:
            os.environ.pop(RUN_ENV, None)
            made = [
                scratch(self, "unmarked-probe"),
                control_scratch(self, "unmk"),
                package_e2e.scratch_dir(stack, "unmarked-probe"),
            ]
            for directory in made:
                self.assertFalse((directory / RUN_MARKER).exists(), directory)

    def test_a_sibling_gate_never_sees_a_scratch_directory_without_its_marker(self) -> None:
        """The gate on one project sweeps the same temp dir while a sibling
        makes and removes its scratch. On Windows it cannot read that process's
        environment, so any swept directory it found unmarked counted as its own
        leak. So another process churns here while this one lists the root
        throughout, and every swept name must hold the churner's marker while it
        exists."""
        root = scratch(self, "observed")
        churn_run = "4242.python-churn"
        environment = {**os.environ, "TMPDIR": str(root), "TEMP": str(root), "TMP": str(root)}
        environment[RUN_ENV] = churn_run
        churner = subprocess.Popen(
            [sys.executable, "-c", CHURN, "300"],
            cwd=Path(__file__).resolve().parents[1],
            env=environment,
        )
        seen = 0
        unmarked: list[str] = []
        while churner.poll() is None:
            for name in os.listdir(root):
                if not name.startswith(PREFIX):
                    continue
                seen += 1
                directory = root / name
                try:
                    marker = (directory / RUN_MARKER).read_text(encoding="utf-8")
                except OSError:
                    marker = None
                # Each cycle's name is its own, so a directory still there after a
                # failed read is the one that was read.
                if marker != churn_run and directory.exists():
                    unmarked.append(name)
        self.assertEqual(churner.returncode, 0)
        self.assertGreater(seen, 0, "the observer never overlapped a scratch directory")
        self.assertEqual(unmarked, [])
        # A directory that really is left behind is still the churner's leak: it
        # stands under the swept name, marked for its run, with nothing staged or
        # retired beside it.
        [leaked] = entries(root)
        self.assertTrue(leaked.startswith(f"{PREFIX}leaked-"), leaked)
        self.assertEqual((root / leaked / RUN_MARKER).read_text(encoding="utf-8"), churn_run)

    def test_a_scratch_directory_that_cannot_be_marked_leaves_nothing_behind(self) -> None:
        """The marker is written under a name the gate does not sweep, so a
        failure once that staging exists would leave a directory no gate could
        ever report. A run token holding a lone surrogate cannot be written as
        UTF-8, so the marker write really fails after staging exists."""
        root = scratch(self, "unmarkable")
        for make in (
            lambda: scratch_module.make_marked(f"{PREFIX}unmarked-", str(root)),
            lambda: package_e2e.make_marked(f"{PREFIX}unmarked-"),
        ):
            with (
                mock.patch.dict(os.environ, {RUN_ENV: "4242.\udcff"}),
                mock.patch.object(tempfile, "tempdir", str(root)),
                self.assertRaises(UnicodeEncodeError),
            ):
                make()
            self.assertEqual(entries(root), [])

    def test_a_scratch_directory_already_gone_is_given_back_without_complaint(self) -> None:
        """Removal renames a directory off the sweep first, and falls back to
        removing it in place when that rename fails, as it does on Windows while
        a handle is open inside, or here, where nothing is left."""
        for retire in (scratch_module.retire, package_e2e.retire):
            directory = Path(tempfile.mkdtemp(prefix=f"{PREFIX}already-gone-"))
            directory.rmdir()
            retire(directory)
            self.assertFalse(directory.exists())

    @unittest.skipIf(sys.platform == "win32" or os.geteuid() == 0, "needs POSIX permissions")
    def test_a_scratch_directory_that_cannot_be_removed_stays_where_the_gate_sees_it(
        self,
    ) -> None:
        """A removal that fails must not hide the directory under a name the gate
        never sweeps: it goes back to its own name, so the gate reports it."""
        for retire in (scratch_module.retire, package_e2e.retire):
            root = scratch(self, "unremovable")
            directory = root / f"{PREFIX}stuck"
            (directory / "locked").mkdir(parents=True)
            (directory / "locked" / "file").write_text("held", encoding="utf-8")
            (directory / "locked").chmod(0o500)
            self.addCleanup((directory / "locked").chmod, 0o700)
            retire(directory)
            self.assertEqual(entries(root), [directory.name])

    def test_a_control_store_is_rooted_at_tmp_rather_than_the_temp_dir(self) -> None:
        """Its path becomes a socket address, and macOS's per-user temp dir alone
        spends about half of `sun_path` — the interrupt test's refusal there. The
        budget itself is the CLI's to enforce, which that test drives for real."""
        store = control_scratch(self, "interrupt")
        self.assertTrue(store.is_dir())
        # Reads the root control_scratch() chooses and creates nothing there.
        tmp = os.path.realpath("/tmp")  # noqa: S108 - an assertion's expected path, not a write
        expected = tempfile.gettempdir() if sys.platform == "win32" else tmp
        self.assertEqual(str(store.parent), expected)


if __name__ == "__main__":
    unittest.main()
