"""What the network recorder's and budgets' hermetic tests share.

Every test drives the real thing a live run uses — scripts/e2e-netshim.sh,
scripts/e2e-netmeasure.py, scripts/e2e-lib.sh's helpers, the suites
themselves, live/pr-matrix/harness/codex/network_budget.py and the
`onebudgetspec check` step — with a double standing in only for the paid
harness and a loopback server for the internet.
"""

from __future__ import annotations

import json
import math
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from typing import Any, Literal, NamedTuple

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPTS = ROOT / "scripts"
NETSHIM = SCRIPTS / "e2e-netshim.sh"
NETMEASURE = SCRIPTS / "e2e-netmeasure.py"
LIB = SCRIPTS / "e2e-lib.sh"
CHECK_STEP = SCRIPTS / "e2e-network-budget.sh"
CODEX_DIR = ROOT / "live" / "pr-matrix" / "harness" / "codex"
BUDGET = CODEX_DIR / "network_budget.py"
BUDGETS = CODEX_DIR / "budgets.yaml"
DOUBLE = HERE / "harness_double.sh"
TRANSFER = HERE / "transfer.py"
OBSERVER = HERE / "observer.py"
WINDOWS = os.name == "nt" or sys.platform == "cygwin"
EXE = ".exe" if WINDOWS else ""
ONEHARNESS = ROOT / "target" / "debug" / f"oneharness{EXE}"
MOCK = ROOT / "target" / "debug" / f"oneharness-mock-harness{EXE}"
BASH = shutil.which("bash") or "bash"
CHUNK = 64 * 1024
# One telemetry line, as JSON decodes it. Deliberately untyped: the tests write
# lines the recorder never would (negative or string bytes, missing fields) to
# prove the budget command refuses them, which a typed model could not express.
Record = dict[str, Any]


def scratch(test: unittest.TestCase) -> Path:
    """A directory the test owns and gives back however it ends."""
    path = Path(tempfile.mkdtemp(prefix="oh-net-test-"))
    test.addCleanup(shutil.rmtree, path, True)
    return path


def lines(path: Path) -> list[Record]:
    if not path.exists():
        return []
    return [
        json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()
    ]


def starts(records: list[Record]) -> list[Record]:
    return [r for r in records if "argv" in r]


def measurements(records: list[Record], launch_id: str) -> list[Record]:
    return [r for r in records if r.get("launch_id") == launch_id and "argv" not in r]


def write_lines(path: Path, records: list[Record]) -> None:
    path.write_text("".join(json.dumps(r) + "\n" for r in records), encoding="utf-8")


_UNMEASURABLE: list[str] = []


def recorder_unavailable() -> str:
    """Why the recorder cannot measure on this host, or "" when it can (cached)."""
    if not _UNMEASURABLE:
        if not sys.platform.startswith("linux"):
            _UNMEASURABLE.append(f"the recorder measures on Linux only, and this is {sys.platform}")
        else:
            with tempfile.TemporaryDirectory(prefix="oh-net-probe-") as probe:
                out = Path(probe) / "probe.jsonl"
                subprocess.run(
                    [
                        sys.executable,
                        str(NETMEASURE),
                        "--line-file",
                        str(out),
                        "--launch-id",
                        "probe",
                        "--",
                        "true",
                    ],
                    capture_output=True,
                    check=False,
                )
                got = lines(out)
                method = got[0].get("method") if got else None
                _UNMEASURABLE.append(
                    ""
                    if method == "netns-pasta"
                    else (got[0].get("reason", "") if got else "no line written")
                )
    return _UNMEASURABLE[0]


def need_recorder(test: unittest.TestCase) -> None:
    """Skip a measuring test where the recorder cannot measure, saying why —
    a failure instead when OH_NET_NO_SKIP=1 (CI's Linux leg)."""
    why = recorder_unavailable()
    if why:
        if os.environ.get("OH_NET_NO_SKIP") == "1" and sys.platform.startswith("linux"):
            test.fail(f"the network recorder cannot measure here and OH_NET_NO_SKIP=1: {why}")
        test.skipTest(why)


def need_builds(test: unittest.TestCase) -> None:
    for binary in (ONEHARNESS, MOCK):
        if not binary.exists():
            test.fail(
                f"{binary} is missing; build it with 'just build' and 'just build-mock-harness'"
            )


class Request(NamedTuple):
    verb: Literal["GET", "PUT"]
    size: int
    pace: float

    @staticmethod
    def parse(line: bytes) -> Request | None:
        """`GET n [pace]` or `PUT n`, n a non-negative integer and pace a finite,
        non-negative number of seconds; None for anything else."""
        words = line.decode(errors="replace").split()
        if (
            not words
            or words[0] not in ("GET", "PUT")
            or len(words) > (3 if words[0] == "GET" else 2)
        ):
            return None
        try:
            size = int(words[1]) if len(words) > 1 else -1
            pace = float(words[2]) if len(words) > 2 else 0.0
        except ValueError:
            return None
        if size < 0 or not math.isfinite(pace) or pace < 0:
            return None
        return Request("GET" if words[0] == "GET" else "PUT", size, pace)


class LoopbackServer:
    """`GET n [pace]` sends n generated bytes; `PUT n` reads n and answers `OK n`."""

    def __init__(self) -> None:
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.port = self.listener.getsockname()[1]
        threading.Thread(target=self._accept, daemon=True).start()

    def _accept(self) -> None:
        while True:
            try:
                conn, _ = self.listener.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(conn,), daemon=True).start()

    @staticmethod
    def _serve(conn: socket.socket) -> None:
        with conn:
            line = b""
            while not line.endswith(b"\n"):
                byte = conn.recv(1)
                if not byte:
                    return
                line += byte
            request = Request.parse(line)
            if request is None:
                conn.sendall(b"ERR expected GET n [pace] or PUT n\n")
                return
            n, pace = request.size, request.pace
            if request.verb == "GET":
                block = os.urandom(CHUNK)
                sent, steps = 0, max(1, n // CHUNK)
                while sent < n:
                    piece = block[: min(CHUNK, n - sent)]
                    conn.sendall(piece)
                    sent += len(piece)
                    if pace:
                        time.sleep(pace / steps)
            else:
                got = 0
                while got < n:
                    data = conn.recv(min(CHUNK, n - got))
                    if not data:
                        break
                    got += len(data)
                conn.sendall(f"OK {got}\n".encode())

    def close(self) -> None:
        self.listener.close()


def server(test: unittest.TestCase) -> LoopbackServer:
    serving = LoopbackServer()
    test.addCleanup(serving.close)
    return serving


def run_shim(
    tmp: Path,
    real: list[str],
    args: list[str],
    *,
    file: Path | None = None,
    harness: str = "codex",
    suite: str = "codex",
    env: dict[str, str] | None = None,
    stdin: bytes = b"",
    timeout: float = 120,
) -> subprocess.CompletedProcess[bytes]:
    """One launch through the real shim. `real` is the executable it wraps;
    an argv of more than one word is written to a script first."""
    executable = Path(real[0])
    if len(real) > 1:
        executable = tmp / f"real-{len(list(tmp.glob('real-*')))}"
        executable.write_text(
            "#!/usr/bin/env bash\nexec " + " ".join(f"'{w}'" for w in real) + ' "$@"\n'
        )
        executable.chmod(0o755)
    file = file or tmp / f"{suite}.jsonl"
    command = [
        BASH,
        str(NETSHIM),
        "--suite",
        suite,
        "--harness",
        harness,
        "--real",
        str(executable),
        "--self",
        str(tmp / "shims" / harness),
        "--file",
        str(file),
        "--",
        *args,
    ]
    return subprocess.run(
        command,
        input=stdin,
        capture_output=True,
        env={**os.environ, **(env or {})},
        timeout=timeout,
        check=False,
    )


def transfer_argv(port: int, down: int, up: int, *flags: str) -> list[str]:
    return [sys.executable, str(TRANSFER), "client", str(port), str(down), str(up), *flags]


def double_bin(tmp: Path, *tools: str) -> Path:
    """A PATH directory where each named harness executable is the double."""
    directory = tmp / "double-bin"
    directory.mkdir(exist_ok=True)
    for tool in tools:
        path = directory / tool
        path.write_text(f'#!/usr/bin/env bash\nexec bash "{DOUBLE}" --double-tool={tool} "$@"\n')
        path.chmod(0o755)
    return directory


def suite_env(tmp: Path, *tools: str, **extra: str) -> dict[str, str]:
    """The environment a suite runs a double under: the double first on PATH,
    the built binaries, telemetry and every home in the test's own scratch."""
    home = tmp / "home"
    (home / ".codex").mkdir(parents=True, exist_ok=True)
    (home / ".codex" / "auth.json").write_text("{}")
    live = tmp / "live-tmp"
    live.mkdir(exist_ok=True)
    env = {
        **os.environ,
        "PATH": f"{double_bin(tmp, *tools)}{os.pathsep}{os.environ['PATH']}",
        "HOME": str(home),
        "CODEX_HOME": str(home / ".codex"),
        "TMPDIR": str(live),
        "ONEHARNESS_BIN": str(ONEHARNESS),
        "OH_TEST_MOCK": str(MOCK),
        "OH_E2E_PROVIDER_DOUBLE": str(MOCK),
        "OH_E2E_TELEMETRY_DIR": str(tmp / "telemetry"),
        "OH_TIMEOUT": "60",
        "XDG_STATE_HOME": str(tmp / "state"),
        "XDG_CONFIG_HOME": str(tmp / "config"),
    }
    for name in (
        "OH_E2E_NO_SKIP",
        "OH_E2E_PHASE",
        "OH_E2E_PHASE_LIMIT",
        "OH_E2E_OS",
        "ONEHARNESS_CONFIG",
    ):
        env.pop(name, None)
    env.update(extra)
    return env


def run_suite(
    name: str, env: dict[str, str], timeout: float = 600
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [BASH, str(SCRIPTS / f"e2e-{name}.sh")],
        capture_output=True,
        text=True,
        env=env,
        timeout=timeout,
        check=False,
    )


def check_step(telemetry_dir: Path, os_name: str = "Linux") -> subprocess.CompletedProcess[str]:
    """The live-codex recipe's budget step, as `just live-codex` runs it."""
    return subprocess.run(
        [BASH, str(CHECK_STEP), "codex"],
        capture_output=True,
        text=True,
        env={**os.environ, "OH_E2E_TELEMETRY_DIR": str(telemetry_dir), "OH_E2E_OS": os_name},
        timeout=600,
        check=False,
    )


def budget(*args: str, telemetry_dir: Path) -> subprocess.CompletedProcess[str]:
    """The budgets' command, run as budgets.yaml runs it, outside a check."""
    return subprocess.run(
        ["uv", "run", "--quiet", "--locked", "--script", str(BUDGET), *args],
        capture_output=True,
        text=True,
        cwd=CODEX_DIR,
        env={
            **{k: v for k, v in os.environ.items() if k != "ONEBUDGETSPEC_RESULT"},
            "OH_E2E_TELEMETRY_DIR": str(telemetry_dir),
        },
        timeout=300,
        check=False,
    )
