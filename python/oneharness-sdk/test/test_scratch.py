"""The scratch guard's own regression: cleanup a failing test cannot skip."""

from __future__ import annotations

import os
import sys
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

    def test_a_control_store_leaves_its_socket_inside_the_tightest_budget(self) -> None:
        """The CLI refuses a socket address past `sun_path` before it answers.

        macOS allows 103 bytes before the NUL. Charged at the longest address the
        CLI builds there — ``/private/tmp`` plus the 12-hex digest it shortens a
        session name to — so an overrun fails on Linux too, not only in macOS CI.
        """
        store = control_scratch(self, "interrupt")
        self.assertTrue(store.is_dir())
        if sys.platform == "win32":
            return
        self.assertEqual(str(store.parent), os.path.realpath("/tmp"))
        address = f"/private/tmp/{store.name}/control/{'0' * 12}.sock"
        self.assertLessEqual(len(address), 103, address)


if __name__ == "__main__":
    unittest.main()
