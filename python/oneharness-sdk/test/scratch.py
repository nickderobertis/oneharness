"""Scratch directories that the test framework removes, however a test ends.

The shape this replaces took a :func:`tempfile.mkdtemp` and never gave it back,
so every run of this suite left one directory per case on the host for good.
Enough of them fill a root filesystem and take every program on it down.

:meth:`unittest.TestCase.addCleanup` is what makes it failure-safe: it runs after
a test that errored or failed exactly as it does after one that passed, which a
``finally`` written out at each call site has to earn again every time.
"""

from __future__ import annotations

import shutil
import tempfile
import unittest
from pathlib import Path

#: The prefix every scratch directory here carries.
#:
#: It must begin with ``oneharness_core::io::scratch::PREFIX``, which is what
#: ``scripts/check-temp-leaks.sh`` sweeps for; ``scripts/check-scratch-prefixes.sh``
#: holds the two in step, because a prefix that drifted out of the sweep would
#: leave the gate silently passing.
PREFIX = "oneharness-python-"


def scratch(case: unittest.TestCase, tag: str) -> Path:
    """Return a private directory for ``case``, removed when that case ends.

    ``tag`` distinguishes one case's directory from another's.
    """
    directory = Path(tempfile.mkdtemp(prefix=f"{PREFIX}{tag}-"))
    case.addCleanup(shutil.rmtree, directory, ignore_errors=True)
    return directory
