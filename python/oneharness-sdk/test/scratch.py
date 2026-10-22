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

#: The variable ``scripts/check-temp-leaks.sh`` hands every process of its run,
#: naming that run, and the file each scratch directory here carries it in.
#:
#: Spelled as ``oneharness_core::io::scratch`` spells them, which is where the
#: gate reads them; ``scripts/check-scratch-prefixes.sh`` holds the two in step.
#: The marker is how the gate tells another live run's scratch from this run's
#: without reading this process's environment, which Windows does not expose.
RUN_ENV = "OH_TEMP_LEAKS_RUN"
RUN_MARKER = ".oh-temp-leaks-run"


def scratch(case: unittest.TestCase, tag: str) -> Path:
    """Return a private directory for ``case``, removed when that case ends.

    ``tag`` distinguishes one case's directory from another's. Under the leak
    gate it carries that run's ``RUN_MARKER``; the name also ends in this
    process's id, which is how ``scripts/check-temp-leaks.sh`` tells another
    checkout's live directory from one this run left behind without a marker.
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
    # The shared root is the point: mkdtemp still makes an unguessable,
    # owner-only directory under it.
    root = None if sys.platform == "win32" else os.path.realpath("/tmp")  # noqa: S108 - see above
    return _scratch_under(case, tag, root)


def _scratch_under(case: unittest.TestCase, tag: str, root: str | None) -> Path:
    directory = make_marked(f"{PREFIX}{tag}-", root)
    case.addCleanup(retire, directory)
    return directory


def make_marked(prefix: str, root: str | None) -> Path:
    """Make a directory named ``prefix``, a random part and this process's id.

    Under the leak gate it is made and marked under a name the gate does not
    sweep, then renamed into place. A sibling run's gate sweeping the same temp
    dir therefore never sees it unmarked. An unmarked directory would fall back
    to this process's environment, which Windows cannot read, so the gate
    would count it as that run's own leak.
    """
    token = os.environ.get(RUN_ENV, "")
    if not token:
        return Path(tempfile.mkdtemp(prefix=prefix, suffix=f"-{os.getpid()}", dir=root))
    staged = Path(tempfile.mkdtemp(prefix=f".{prefix}", suffix=f"-{os.getpid()}.new", dir=root))
    directory = staged.with_name(staged.name[1 : -len(".new")])
    try:
        (staged / RUN_MARKER).write_text(token, encoding="utf-8")
        staged.rename(directory)
    except BaseException:
        shutil.rmtree(staged, ignore_errors=True)
        raise
    return directory


def retire(directory: Path) -> None:
    """Remove ``directory``, first renaming it off the leak gate's sweep.

    A directory half-removed under its own name has lost its marker but is
    still visible to a sibling run's gate. If the rename fails (Windows refuses
    it while a handle is open inside), the directory is removed in place. One
    that cannot be removed goes back to its swept name, so the gate still
    reports it.
    """
    gone = directory.with_name(f".{directory.name}.gone")
    try:
        directory.rename(gone)
    except OSError:
        shutil.rmtree(directory, ignore_errors=True)
        return
    shutil.rmtree(gone, ignore_errors=True)
    if gone.exists():
        gone.rename(directory)
