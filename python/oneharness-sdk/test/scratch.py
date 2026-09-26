"""Scratch directories that the test framework removes, however a test ends.

:meth:`unittest.TestCase.addCleanup` is what makes it failure-safe: it runs after
a test that errored or failed exactly as it does after one that passed, which a
``finally`` written out at each call site has to earn again every time.
"""

from __future__ import annotations

import os
import shutil
import sys
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

    ``tag`` distinguishes one case's directory from another's. The name ends in
    this process's id, which is how ``scripts/check-temp-leaks.sh`` tells another
    checkout's live directory from one this run left behind.
    """
    return _scratch_under(case, tag, None)


def control_scratch(case: unittest.TestCase, tag: str) -> Path:
    """Return a scratch session store, whose path becomes a control socket address.

    The socket lives at ``<store>/control/<name>.sock``. Rooted at the canonical
    ``/tmp`` on unix rather than the temp dir, as the Rust suite's
    ``control_store_root`` is, because that address has a ``sun_path`` budget of
    104 bytes on macOS and its per-user ``$TMPDIR`` spends 49 of them before the
    store's own name begins — a name ending in the pid then leaves the socket no
    room. Windows has no ``sun_path``, so the temp dir serves there.
    """
    # S108: the shared root is the point, and mkdtemp still makes an unguessable
    # owner-only directory under it.
    root = None if sys.platform == "win32" else os.path.realpath("/tmp")  # noqa: S108
    return _scratch_under(case, tag, root)


def _scratch_under(case: unittest.TestCase, tag: str, root: str | None) -> Path:
    directory = Path(tempfile.mkdtemp(prefix=f"{PREFIX}{tag}-", suffix=f"-{os.getpid()}", dir=root))
    case.addCleanup(shutil.rmtree, directory, ignore_errors=True)
    return directory
