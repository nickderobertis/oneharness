"""The scratch guard's own regression: cleanup a failing test cannot skip."""

from __future__ import annotations

import unittest
from pathlib import Path

from .scratch import PREFIX, scratch


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
            def runTest(self) -> None:  # noqa: N802 - unittest's own spelling
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


if __name__ == "__main__":
    unittest.main()
