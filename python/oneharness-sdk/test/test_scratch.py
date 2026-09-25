"""The scratch guard's own regression: cleanup a failing test cannot skip."""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

from .scratch import PREFIX, control_scratch, scratch


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

    def test_a_control_store_is_rooted_at_tmp_rather_than_the_temp_dir(self) -> None:
        """Its path becomes a socket address, and macOS's per-user temp dir alone
        spends about half of `sun_path` — the interrupt test's refusal there. The
        budget itself is the CLI's to enforce, which that test drives for real."""
        store = control_scratch(self, "interrupt")
        self.assertTrue(store.is_dir())
        # S108: asserts the root control_scratch() chooses, and creates nothing there.
        tmp = os.path.realpath("/tmp")  # noqa: S108
        expected = tempfile.gettempdir() if sys.platform == "win32" else tmp
        self.assertEqual(str(store.parent), expected)


if __name__ == "__main__":
    unittest.main()
