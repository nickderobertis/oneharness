#!/usr/bin/env python3
"""Hermetic tests of `scripts/codex-network-breakdown.py`.

    python3 -I tools/network-breakdown/test_codex_network_breakdown.py

The reporting tests read a measurement and a Codex `RUST_LOG` trace recorded
from a real Codex launch and committed beside this file, so they need no
launch; neither do the tests of how the script finds pasta and what it says
when it cannot measure. The measuring tests run the script around
`transfer_double.py`, which moves payloads of known sizes through the proxy
variables to loopback servers standing for destinations. They need Linux with
unprivileged user namespaces and pasta; where the namespace cannot be built
they say so and skip, unless `CODEX_NETWORK_BREAKDOWN_REQUIRED=1` makes that a
failure.
"""

from __future__ import annotations

import json
import os
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = ROOT / "scripts" / "codex-network-breakdown.py"
DOUBLE = HERE / "transfer_double.py"
MEASUREMENT = HERE / "recorded-measurement.json"
TRACE = HERE / "recorded-codex-trace.log"
# Generated from oneharness's own report types; the SDK contract tests hold it
# to them, so it is the producer's word on the run report's shape.
SCHEMAS = (
    ROOT / "python" / "oneharness-sdk" / "src" / "oneharness_sdk" / "_generated" / "schemas.json"
)
# A launch still running after this fails the test rather than hanging it.
HANG_GUARD_SECONDS = 120


def run_script(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=HANG_GUARD_SECONDS,
        env=env,
        check=False,
    )


# The script's document is JSON this test reads by key, and those keys' shapes
# are exactly what its assertions check, so it is held as `Any` rather than
# modelled twice.
def breakdown(*args: str) -> dict[str, Any]:
    """The script's document, which must be one line on stdout."""
    run = run_script(*args)
    if run.returncode != 0:
        raise AssertionError(f"exit {run.returncode}\n{run.stdout}\n{run.stderr}")
    lines = run.stdout.splitlines()
    if len(lines) != 1:
        raise AssertionError(f"the document must be one line, got {len(lines)}:\n{run.stdout}")
    return json.loads(lines[0])


class ReportFromRecordedFixtures(unittest.TestCase):
    """The printed document, assembled from a recorded measurement and trace."""

    def test_each_tunnel_carries_its_destination_bytes_times_and_traced_paths(self) -> None:
        recorded = json.loads(MEASUREMENT.read_text())
        report = breakdown("report", str(MEASUREMENT), "--trace", str(TRACE))
        self.assertTrue(report["traced"])
        self.assertEqual(report["received_bytes"], recorded["received_bytes"])
        self.assertEqual(report["sent_bytes"], recorded["sent_bytes"])
        self.assertEqual(len(report["tunnels"]), len(recorded["tunnels"]))
        for tunnel in report["tunnels"]:
            self.assertEqual(tunnel["destination"], f"{tunnel['host']}:{tunnel['port']}")
            for name in ("received_bytes", "sent_bytes", "opened_at", "closed_at"):
                self.assertIsInstance(tunnel[name], (int, float), name)
            self.assertLessEqual(tunnel["opened_at"], tunnel["closed_at"])
            self.assertIsInstance(tunnel["paths"], list)
        self.assertEqual(
            report["tunnel_received_bytes"] + report["unaccounted_received_bytes"],
            report["received_bytes"],
        )
        expected = json.loads((HERE / "expected-paths.json").read_text())
        self.assertEqual(
            [[tunnel["destination"], tunnel["paths"]] for tunnel in report["tunnels"]],
            expected,
        )
        # The trace names nothing for at least one tunnel, which gets no path.
        self.assertIn([], [tunnel["paths"] for tunnel in report["tunnels"]])
        self.assertTrue(any(tunnel["paths"] for tunnel in report["tunnels"]))

    def attributed(
        self, connections: list[str], opened: list[float], url: str = "https://chatgpt.com/"
    ) -> list[list[str]]:
        """The paths report mode gives tunnels to chatgpt.com:443 opened at
        `opened` (seconds after 2026-10-13T00:00:00Z), under a trace of
        `connections` (each `<seconds> <span chain>`) to `url`."""
        epoch = 1_791_849_600.0  # 2026-10-13T00:00:00Z
        trace = "\n".join(
            f"2026-10-13T00:00:{float(at):09.6f}Z DEBUG {spans}reqwest::connect: "
            f"starting new connection: {url}"
            for at, spans in (line.split(" ", 1) for line in connections)
        )
        measurement = {
            "received_bytes": 100,
            "sent_bytes": 100,
            "tunnels": [
                {
                    "host": "chatgpt.com",
                    "port": 443,
                    "opened_at": epoch + at,
                    "closed_at": epoch + at + 1,
                    "received_bytes": 10,
                    "sent_bytes": 10,
                }
                for at in opened
            ],
        }
        with tempfile.TemporaryDirectory() as scratch:
            (Path(scratch) / "m.json").write_text(json.dumps(measurement))
            (Path(scratch) / "t.log").write_text(trace)
            report = breakdown(
                "report", str(Path(scratch) / "m.json"), "--trace", str(Path(scratch) / "t.log")
            )
        return [tunnel["paths"] for tunnel in report["tunnels"]]

    def test_a_span_api_path_names_a_connection_no_url_names(self) -> None:
        span = 'list_models{refresh_strategy=online}:endpoint_session.execute_with{api.path="models"}: '
        self.assertEqual(self.attributed([f"1.0 {span}"], [1.01]), [["/models"]])

    def test_a_connection_outside_a_tunnels_window_names_nothing(self) -> None:
        span = 'list_models{api.path="models"}: '
        self.assertEqual(self.attributed([f"1.0 {span}"], [10.0]), [[]])
        self.assertEqual(self.attributed([f"10.0 {span}"], [1.0]), [[]])

    def test_a_url_whose_port_is_not_a_port_names_nothing(self) -> None:
        span = 'list_models{api.path="models"}: '
        for url in ("https://chatgpt.com:invalid/", f"https://chatgpt.com:{'4' * 5000}/"):
            self.assertEqual(self.attributed([f"1.0 {span}"], [1.01], url=url), [[]], url[:40])

    def test_one_connection_names_one_tunnel_however_many_could_match(self) -> None:
        span = 'list_models{api.path="models"}: '
        self.assertEqual(self.attributed([f"1.0 {span}"], [1.1, 1.2]), [["/models"], []])

    def test_unreadable_inputs_are_refused_saying_what_to_do(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            missing = str(Path(scratch) / "missing.json")
            not_json = Path(scratch) / "not.json"
            not_json.write_text("{")
            for args, said in (
                (["report", missing], f"cannot read {missing}"),
                (["report", str(MEASUREMENT), "--trace", missing], f"cannot read {missing}"),
                (["report", str(not_json)], "is not JSON"),
            ):
                run = run_script(*args)
                self.assertEqual(run.returncode, 3, (args, run))
                self.assertEqual(run.stdout, "")
                self.assertIn(said, run.stderr, args)
                self.assertTrue(
                    "check the path" in run.stderr or "--measurement-out" in run.stderr,
                    run.stderr,
                )

    def test_without_a_trace_no_tunnel_has_a_path(self) -> None:
        report = breakdown("report", str(MEASUREMENT))
        self.assertFalse(report["traced"])
        self.assertTrue(report["tunnels"])
        self.assertEqual({len(tunnel["paths"]) for tunnel in report["tunnels"]}, {0})

    def test_a_recorded_measurement_that_is_wrong_is_refused_saying_how_to_fix_it(self) -> None:
        recorded = json.loads(MEASUREMENT.read_text())
        cases = [
            ("tunnels", 0, "sent_bytes", None, "tunnel 0's `sent_bytes` is None"),
            ("tunnels", 0, "closed_at", 0.0, "tunnel 0 closed before it opened"),
            ("tunnels", 1, "opened_at", float("inf"), "not a finite time"),
            (None, None, "received_bytes", -1, "`received_bytes` is -1, not a count"),
            (None, None, "refused", 1, "`refused` is not a JSON array"),
            ("tunnels", 0, "opened_at", 10**400, "too large to be a time"),
            ("tunnels", 0, "opened_at", "soon", "is 'soon', not a time"),
            ("tunnels", 0, "opened_at", True, "is True, not a time"),
            ("tunnels", 0, "host", 7, "`host` is 7, not text"),
            ("tunnels", 0, "error", 7, "`error` is 7, not text"),
            ("tunnels", 0, "port", 0, "`port` 0 is no port"),
            ("tunnels", 0, "port", 70000, "`port` 70000 is no port"),
            (None, None, "tunnels", ["x"], "tunnel 0 is not a JSON object"),
            (None, None, "refused", ["x"], "refusal 0 is not a JSON object"),
            (
                None,
                None,
                "refused",
                [{"destination": 1, "at": 1.0, "error": "e"}],
                "`destination` is 1, not text",
            ),
            (None, None, "argv", ["codex", 1], "holds a non-text argument"),
            (None, None, "launch_exit", True, "`launch_exit` is True, not an exit code"),
            (None, None, "launch_exit", 1.5, "`launch_exit` is 1.5, not an exit code"),
            (None, None, "timed_out", "yes", "`timed_out` is 'yes', not true or false"),
            (None, None, "label", 3, "`label` is 3, not text"),
        ]
        for listed, index, key, value, said in cases:
            broken = json.loads(json.dumps(recorded))
            target = broken[listed][index] if listed is not None else broken
            target[key] = value
            with tempfile.TemporaryDirectory() as scratch:
                path = Path(scratch) / "broken.json"
                path.write_text(json.dumps(broken))
                run = run_script("report", str(path))
            self.assertEqual(run.returncode, 3, (key, run))
            self.assertEqual(run.stdout, "")
            self.assertIn(said, run.stderr, key)
            self.assertIn("--measurement-out", run.stderr, key)
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "array.json"
            path.write_text("[]")
            run = run_script("report", str(path))
        self.assertEqual(run.returncode, 3, run)
        self.assertIn("is not a JSON object", run.stderr)


def without_pasta_env(cache: Path, **extra: str) -> dict[str, str]:
    """An environment with no pasta to find but what the test provides."""
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("CODEX_NETWORK_BREAKDOWN_")
    }
    env.update(PATH=str(cache / "empty-path"), XDG_CACHE_HOME=str(cache), **extra)
    return env


def executable(path: Path, text: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)
    return path


class RunReportShape(unittest.TestCase):
    """The run report fields the script reads a launch's stderr from, against
    the generated schema of the report oneharness actually prints."""

    def test_the_run_report_carries_each_results_stderr_as_a_string(self) -> None:
        schema = json.loads(SCHEMAS.read_text())["run_report"]
        self.assertIn("results", schema["required"])
        results = schema["properties"]["results"]
        self.assertEqual(results["type"], "array")
        result = schema["$defs"][results["items"]["$ref"].rsplit("/", 1)[-1]]
        self.assertIn("stderr", result["required"])
        self.assertEqual(result["properties"]["stderr"]["type"], "string")


class RefusalsWithoutANamespace(unittest.TestCase):
    """How the script finds pasta, and what it says when it cannot measure: on
    stderr, with a next action, exit 3; and its usage errors, exit 2."""

    def test_a_pasta_that_cannot_start_says_how_to_get_one(self) -> None:
        env = dict(os.environ, CODEX_NETWORK_BREAKDOWN_PASTA="/nonexistent/oneharness-no-pasta")
        run = run_script("--", "true", env=env)
        self.assertEqual(run.returncode, 3, run)
        self.assertEqual(run.stdout, "")
        self.assertIn("cannot start pasta (/nonexistent/oneharness-no-pasta)", run.stderr)
        self.assertIn("apt install passt", run.stderr)

    def test_a_pasta_that_records_nothing_says_how_to_find_out_why(self) -> None:
        # Python rejects pasta's options and exits, as a pasta that cannot
        # build the namespace does, before anything is recorded.
        env = dict(os.environ, CODEX_NETWORK_BREAKDOWN_PASTA=sys.executable)
        run = run_script("--", "true", env=env)
        self.assertEqual(run.returncode, 3, run)
        self.assertEqual(run.stdout, "")
        self.assertIn("no measurement: pasta exited 2", run.stderr)
        self.assertIn("apparmor_restrict_unprivileged_userns", run.stderr)

    def test_with_fetching_forbidden_a_missing_pasta_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            env = without_pasta_env(Path(scratch), CODEX_NETWORK_BREAKDOWN_NO_FETCH="1")
            run = run_script("--", "true", env=env)
        self.assertEqual(run.returncode, 3, run)
        self.assertIn("CODEX_NETWORK_BREAKDOWN_NO_FETCH=1 forbids fetching it", run.stderr)

    @unittest.skipUnless(os.name == "posix", "the stand-in tools are shell scripts")
    def test_pasta_is_taken_from_path_then_from_a_cached_fetch(self) -> None:
        # Each stand-in is Python under pasta's name, so the attempt fails the
        # same recognizable way, naming the pasta it started.
        stand_in = f'#!/bin/sh\nexec {sys.executable} "$@"\n'
        with tempfile.TemporaryDirectory() as scratch:
            cache = Path(scratch)
            cached = executable(
                cache / "oneharness-network-breakdown" / "root" / "usr" / "bin" / "pasta",
                stand_in,
            )
            run = run_script("--", "true", env=without_pasta_env(cache))
            self.assertIn(f"{cached} --config-net", run.stderr)
            on_path = executable(cache / "empty-path" / "pasta", stand_in)
            run = run_script("--", "true", env=without_pasta_env(cache))
            self.assertIn(f"{on_path} --config-net", run.stderr)
        self.assertEqual(run.returncode, 3, run)

    @unittest.skipUnless(os.name == "posix", "the stand-in tools are shell scripts")
    def test_a_fetch_runs_apt_get_and_dpkg_and_says_which_step_failed(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            cache = Path(scratch)
            tools = cache / "empty-path"
            executable(
                tools / "apt-get",
                "#!/bin/sh\necho 'E: no network' >&2\nexit 100\n",
            )
            run = run_script("--", "true", env=without_pasta_env(cache))
            self.assertEqual(run.returncode, 3, run)
            self.assertIn("`apt-get download passt` exited 100: E: no network", run.stderr)
            # PATH holds only these stand-ins, so they use the shell's builtins
            # and absolute paths.
            executable(tools / "apt-get", "#!/bin/sh\n: > passt_0_fake.deb\n")
            executable(
                tools / "dpkg",
                '#!/bin/sh\n/bin/mkdir -p "$3/usr/bin" && '
                f'printf \'#!/bin/sh\\nexec {sys.executable} "$@"\\n\' > "$3/usr/bin/pasta" && '
                '/bin/chmod +x "$3/usr/bin/pasta"\n',
            )
            run = run_script("--", "true", env=without_pasta_env(cache))
            fetched = cache / "oneharness-network-breakdown" / "root" / "usr" / "bin" / "pasta"
            self.assertTrue(fetched.exists(), run)
            self.assertIn(f"{fetched} --config-net", run.stderr)

    def test_usage_errors_are_refused_before_anything_runs(self) -> None:
        for args, said in (
            (["--timeout", "-1", "--", "true"], "seconds >= 0"),
            (["--timeout", "nan", "--", "true"], "seconds >= 0"),
            (["--env", "1BAD=x", "--", "true"], "NAME=VALUE"),
            (["--env", "BAD\n=x", "--", "true"], "NAME=VALUE"),
            (["--timeout", "\u00b2", "--", "true"], "seconds >= 0"),
            ([], "argv after --"),
        ):
            run = run_script(*args)
            self.assertEqual(run.returncode, 2, (args, run))
            self.assertIn(said, run.stderr, args)


def answer(conn: socket.socket) -> None:
    """`GET n` answers n bytes; `PUT n` reads n bytes and answers `OK`; a
    plain-HTTP `GET http://host:port/n` answers n bytes after its head."""
    with conn:
        line = b""
        while not line.endswith(b"\n"):
            data = conn.recv(1)
            if not data:
                return
            line += data
        words = line.split()
        if words[:1] == [b"GET"] and len(words) == 3 and words[1].startswith(b"http://"):
            head = line
            while not head.endswith(b"\r\n\r\n"):
                data = conn.recv(1)
                if not data:
                    return
                head += data
            words = [b"GET", words[1].rsplit(b"/", 1)[1]]
        if len(words) != 2 or words[0] not in (b"GET", b"PUT") or not words[1].isdigit():
            return
        size = int(words[1])
        if words[0] == b"GET":
            block = os.urandom(65536)
            sent = 0
            while sent < size:
                piece = block[: min(len(block), size - sent)]
                conn.sendall(piece)
                sent += len(piece)
        else:
            got = 0
            while got < size:
                data = conn.recv(65536)
                if not data:
                    return
                got += len(data)
            conn.sendall(b"OK\n")


def loopback_server() -> int:
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()

    def serve() -> None:
        while True:
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=answer, args=(conn,), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()
    return int(listener.getsockname()[1])


def resetting_server() -> int:
    """A loopback server that answers a request line with a few bytes and then
    resets the connection, as a peer that drops mid-transfer does."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()

    def serve() -> None:
        while True:
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            conn.recv(64)
            conn.sendall(b"partial")
            time.sleep(0.2)
            # SO_LINGER with a zero timeout makes close() send a reset.
            conn.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
            conn.close()

    threading.Thread(target=serve, daemon=True).start()
    return int(listener.getsockname()[1])


def closed_port() -> int:
    """A loopback port nothing listens on."""
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def namespace_unavailable() -> str:
    """Why the measuring namespace cannot be built here, or "" when it can."""
    if not sys.platform.startswith("linux"):
        return f"the measuring namespace is Linux-only, and this is {sys.platform}"
    run = run_script("--label", "probe", "--", "true")
    if run.returncode != 0:
        return f"the measuring namespace could not be built: {run.stderr.strip()}"
    return ""


class Side(Enum):
    """Which byte count a transfer's payload moves through."""

    RECEIVED = "received_bytes"
    SENT = "sent_bytes"


@dataclass(frozen=True)
class Expected:
    port: int
    side: Side
    payload: int


class MeasureAroundATransferDouble(unittest.TestCase):
    """Each tunnel's bytes against payloads of known sizes."""

    DOWN_FIRST = 3_000_000
    DOWN_SECOND = 1_500_000
    UP = 2_000_000

    def need_namespace(self) -> None:
        why = namespace_unavailable()
        if why:
            if os.environ.get("CODEX_NETWORK_BREAKDOWN_REQUIRED") == "1":
                self.fail(why)
            self.skipTest(why)

    def assert_payload(self, tunnel: dict[str, Any], expected: Expected) -> None:
        self.assertEqual(tunnel["port"], expected.port, tunnel)
        self.assertEqual(tunnel["destination"], f"{tunnel['host']}:{expected.port}", tunnel)
        side = expected.side.value
        other = Side.SENT.value if expected.side is Side.RECEIVED else Side.RECEIVED.value
        self.assertGreaterEqual(tunnel[side], expected.payload, tunnel)
        self.assertLessEqual(tunnel[side], expected.payload * 1.01, tunnel)
        self.assertLessEqual(tunnel[other], expected.payload * 0.01, tunnel)
        self.assertLess(tunnel["opened_at"], tunnel["closed_at"], tunnel)
        self.assertIsNone(tunnel["error"], tunnel)

    def test_each_tunnel_carries_its_destination_and_payload_and_accounts_for_the_namespace(
        self,
    ) -> None:
        self.need_namespace()
        first, second = loopback_server(), loopback_server()
        report = breakdown(
            "--label",
            "transfer-double",
            "--",
            sys.executable,
            "-I",
            str(DOUBLE),
            f"{first}:down:{self.DOWN_FIRST}",
            f"{second}:down:{self.DOWN_SECOND}",
            f"{second}:up:{self.UP}",
        )
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertFalse(report["timed_out"])
        tunnels = report["tunnels"]
        self.assertEqual(len(tunnels), 3, tunnels)
        expected = [
            Expected(first, Side.RECEIVED, self.DOWN_FIRST),
            Expected(second, Side.RECEIVED, self.DOWN_SECOND),
            Expected(second, Side.SENT, self.UP),
        ]
        for tunnel, want in zip(tunnels, expected):
            self.assert_payload(tunnel, want)
        self.assertEqual(len({tunnel["destination"] for tunnel in tunnels}), 2, tunnels)
        for side in ("received", "sent"):
            namespace = report[f"{side}_bytes"]
            accounted = report[f"tunnel_{side}_bytes"]
            self.assertGreaterEqual(accounted, 0.9 * namespace, report)
            self.assertLessEqual(accounted, namespace, report)

    def test_a_plain_http_request_through_the_proxy_is_a_tunnel_too(self) -> None:
        self.need_namespace()
        port = loopback_server()
        report = breakdown(
            "--", sys.executable, "-I", str(DOUBLE), f"{port}:http:{self.DOWN_SECOND}"
        )
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(len(report["tunnels"]), 1, report)
        self.assert_payload(report["tunnels"][0], Expected(port, Side.RECEIVED, self.DOWN_SECOND))

    def test_a_connection_the_proxy_cannot_open_is_reported_with_its_error(self) -> None:
        self.need_namespace()
        port = closed_port()
        report = breakdown("--", sys.executable, "-I", str(DOUBLE), "--loopback", f"{port}:down:1")
        self.assertNotEqual(report["launch_exit"], 0, report)
        self.assertEqual(report["tunnels"], [])
        self.assertEqual(len(report["refused"]), 1, report)
        refusal = report["refused"][0]
        self.assertTrue(refusal["destination"].endswith(f":{port}"), refusal)
        self.assertIn("ConnectionRefusedError", refusal["error"])

    def test_malformed_proxy_requests_are_refused_and_reported(self) -> None:
        self.need_namespace()
        client = (
            "import os, socket\n"
            "from urllib.parse import urlparse\n"
            "proxy = urlparse(os.environ['HTTPS_PROXY'])\n"
            "for head in (b'NONSENSE\\r\\n\\r\\n', b'GET /relative HTTP/1.1\\r\\n\\r\\n',\n"
            "             b'CONNECT example.invalid:99999 HTTP/1.1\\r\\n\\r\\n',\n"
            "             b'CONNECT bad_host!:443 HTTP/1.1\\r\\n\\r\\n'):\n"
            "    with socket.create_connection((proxy.hostname, proxy.port)) as conn:\n"
            "        conn.sendall(head)\n"
            "        assert conn.recv(64).startswith(b'HTTP/1.1 400'), head\n"
        )
        report = breakdown("--", sys.executable, "-I", "-c", client)
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(report["tunnels"], [])
        errors = [refusal["error"] for refusal in report["refused"]]
        self.assertEqual(len(errors), 4, report)
        self.assertIn("malformed request line", errors[0])
        self.assertIn("not a proxy request", errors[1])
        self.assertIn("no host and port", errors[2])
        self.assertIn("no host and port in 'bad_host!:443'", errors[3])

    def test_a_request_that_never_completes_is_refused_and_reported(self) -> None:
        self.need_namespace()
        client = (
            "import os, socket, struct\n"
            "from urllib.parse import urlparse\n"
            "proxy = urlparse(os.environ['HTTPS_PROXY'])\n"
            "def conn():\n"
            "    return socket.create_connection((proxy.hostname, proxy.port))\n"
            "with conn() as c:\n"
            "    c.sendall(b'CONNECT example.invalid:443 HTTP/1.1\\r\\n')\n"
            "    c.shutdown(socket.SHUT_WR)\n"
            "    c.recv(64)\n"
            "with conn() as c:\n"
            "    c.sendall(b'X' * 70000)\n"
            "    assert c.recv(64).startswith(b'HTTP/1.1 431')\n"
            "c = conn()\n"
            "c.sendall(b'CONNECT example.invalid:443 HTTP/1.1\\r\\n')\n"
            "c.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack('ii', 1, 0))\n"
            "c.close()\n"
        )
        report = breakdown("--", sys.executable, "-I", "-c", client)
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(report["tunnels"], [])
        errors = sorted(refusal["error"] for refusal in report["refused"])
        self.assertEqual(len(errors), 3, report)
        self.assertTrue(any("closed before its request was complete" in e for e in errors), errors)
        self.assertTrue(any("request head over 65536 bytes" in e for e in errors), errors)
        self.assertTrue(
            any(e.startswith("handshake: ConnectionResetError") for e in errors), errors
        )

    def test_env_values_reach_the_measured_launch(self) -> None:
        self.need_namespace()
        report = breakdown(
            "--env",
            "BREAKDOWN_PROBE=reached",
            "--",
            sys.executable,
            "-I",
            "-c",
            "import os, sys; sys.exit(0 if os.environ.get('BREAKDOWN_PROBE') == 'reached' else 7)",
        )
        self.assertEqual(report["launch_exit"], 0, report)

    def test_ambient_proxy_settings_cannot_route_around_the_counting_proxy(self) -> None:
        self.need_namespace()
        port = loopback_server()
        ambient = dict(
            os.environ,
            NO_PROXY="*",
            no_proxy="*",
            ALL_PROXY="http://127.0.0.1:1",
            https_proxy="http://127.0.0.1:1",
        )
        check = (
            "import os, sys\n"
            "proxies = {os.environ[name] for name in "
            "('HTTPS_PROXY', 'HTTP_PROXY', 'https_proxy', 'http_proxy')}\n"
            "bypass = [n for n in ('NO_PROXY', 'no_proxy', 'ALL_PROXY', 'all_proxy') if n in os.environ]\n"
            "sys.exit(0 if len(proxies) == 1 and not bypass else 7)\n"
        )
        run = run_script("--", sys.executable, "-I", "-c", check, env=ambient)
        self.assertEqual(json.loads(run.stdout)["launch_exit"], 0, run)
        run = run_script(
            "--", sys.executable, "-I", str(DOUBLE), f"{port}:down:{self.DOWN_SECOND}", env=ambient
        )
        report = json.loads(run.stdout)
        self.assertEqual(report["launch_exit"], 0, run)
        self.assert_payload(report["tunnels"][0], Expected(port, Side.RECEIVED, self.DOWN_SECOND))

    def test_a_signal_to_the_script_ends_the_whole_measured_tree(self) -> None:
        self.need_namespace()
        with tempfile.TemporaryDirectory() as scratch:
            ticks = Path(scratch) / "ticks"
            # The launch leaves a detached grandchild ticking into a file, then
            # waits: a signal to the script must end both.
            grandchild = (
                "import time\n"
                f"with open({str(ticks)!r}, 'a') as out:\n"
                "    while True:\n"
                "        out.write('.'); out.flush(); time.sleep(0.1)\n"
            )
            launch = (
                "import subprocess, sys, time\n"
                f"subprocess.Popen([sys.executable, '-I', '-c', {grandchild!r}], "
                "start_new_session=True)\n"
                "time.sleep(60)\n"
            )
            script = subprocess.Popen(
                [sys.executable, "-I", str(SCRIPT), "--", sys.executable, "-I", "-c", launch],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            deadline = time.monotonic() + 20
            while not (ticks.exists() and ticks.stat().st_size > 3):
                self.assertLess(time.monotonic(), deadline, "the grandchild never ticked")
                time.sleep(0.1)
            script.send_signal(signal.SIGTERM)
            stdout, _ = script.communicate(timeout=20)
            settled = ticks.stat().st_size
            time.sleep(0.5)
            self.assertEqual(ticks.stat().st_size, settled, "the grandchild outlived the script")
        self.assertEqual(script.returncode, 0, stdout)
        self.assertIn("received_bytes", stdout)

    def test_a_launchs_stdout_is_quiet_on_success_and_its_tail_shown_on_failure(self) -> None:
        self.need_namespace()
        chatter = "import sys\nfor n in range(50): print(f'line {n}')\nsys.exit(int(sys.argv[1]))\n"
        run = run_script("--", sys.executable, "-I", "-c", chatter, "0")
        self.assertEqual(json.loads(run.stdout)["launch_exit"], 0, run)
        self.assertNotIn("line ", run.stderr)
        run = run_script("--", sys.executable, "-I", "-c", chatter, "4")
        self.assertEqual(run.returncode, 0, run)
        self.assertEqual(json.loads(run.stdout)["launch_exit"], 4, run)
        self.assertIn("the launch exited 4", run.stderr)
        self.assertIn("--launch-stdout PATH", run.stderr)
        self.assertIn("line 30\n", run.stderr)
        self.assertIn("line 49", run.stderr)
        self.assertNotIn("line 29\n", run.stderr)
        with tempfile.TemporaryDirectory() as scratch:
            kept = Path(scratch) / "stdout"
            run = run_script(
                "--launch-stdout", str(kept), "--", sys.executable, "-I", "-c", chatter, "4"
            )
            self.assertEqual(run.returncode, 0, run)
            self.assertNotIn("line ", run.stderr)
            self.assertEqual(len(kept.read_text().splitlines()), 50)

    def test_a_launch_that_cannot_start_is_refused_saying_what_to_do(self) -> None:
        self.need_namespace()
        run = run_script("--", "/nonexistent/oneharness-no-launch")
        self.assertEqual(run.returncode, 3, run)
        self.assertEqual(run.stdout, "")
        self.assertIn("cannot start the launch `/nonexistent/oneharness-no-launch`", run.stderr)

    def test_an_unwritable_output_path_is_refused_saying_what_to_do(self) -> None:
        self.need_namespace()
        with tempfile.TemporaryDirectory() as scratch:
            run = run_script("--measurement-out", scratch, "--", "true")
        self.assertEqual(run.returncode, 3, run)
        self.assertEqual(run.stdout, "")
        self.assertIn(f"cannot write {scratch}", run.stderr)
        self.assertIn("writable path", run.stderr)

    def test_traffic_around_the_proxy_is_counted_as_unaccounted(self) -> None:
        self.need_namespace()
        port = loopback_server()
        report = breakdown(
            "--", sys.executable, "-I", str(DOUBLE), f"{port}:direct:{self.DOWN_SECOND}"
        )
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(report["tunnels"], [])
        self.assertGreaterEqual(report["received_bytes"], self.DOWN_SECOND)
        self.assertGreaterEqual(report["unaccounted_received_bytes"], self.DOWN_SECOND)

    def test_an_oversized_port_in_a_proxy_request_is_refused(self) -> None:
        self.need_namespace()
        client = (
            "import os, socket\n"
            "from urllib.parse import urlparse\n"
            "proxy = urlparse(os.environ['HTTPS_PROXY'])\n"
            "with socket.create_connection((proxy.hostname, proxy.port)) as conn:\n"
            "    conn.sendall(b'CONNECT example.invalid:' + b'4' * 5000 + b' HTTP/1.1\\r\\n\\r\\n')\n"
            "    assert conn.recv(64).startswith(b'HTTP/1.1 400')\n"
        )
        report = breakdown("--", sys.executable, "-I", "-c", client)
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(len(report["refused"]), 1, report)
        self.assertIn("no host and port", report["refused"][0]["error"])

    def test_a_relay_the_destination_breaks_reports_its_error(self) -> None:
        self.need_namespace()
        port = resetting_server()
        report = breakdown("--", sys.executable, "-I", str(DOUBLE), f"{port}:down:1000000")
        self.assertNotEqual(report["launch_exit"], 0, report)
        self.assertEqual(len(report["tunnels"]), 1, report)
        tunnel = report["tunnels"][0]
        self.assertEqual(tunnel["received_bytes"], len(b"partial"), tunnel)
        self.assertIn("ConnectionResetError", tunnel["error"] or "", tunnel)

    def test_a_detached_child_is_measured_after_its_parent_exits(self) -> None:
        self.need_namespace()
        port = loopback_server()
        report = breakdown(
            "--", sys.executable, "-I", str(DOUBLE), "--detach", f"{port}:down:{self.DOWN_SECOND}"
        )
        self.assertEqual(report["launch_exit"], 0, report)
        self.assertEqual(len(report["tunnels"]), 1, report)
        self.assert_payload(report["tunnels"][0], Expected(port, Side.RECEIVED, self.DOWN_SECOND))
        self.assertGreaterEqual(report["received_bytes"], self.DOWN_SECOND)

    def test_a_timeout_ends_the_launch_and_says_so(self) -> None:
        self.need_namespace()
        started = time.monotonic()
        report = breakdown(
            "--timeout", "1", "--", sys.executable, "-I", "-c", "import time; time.sleep(60)"
        )
        self.assertTrue(report["timed_out"], report)
        self.assertLess(time.monotonic() - started, 5)

    def test_a_launch_ignoring_sigterm_is_killed_after_the_grace(self) -> None:
        self.need_namespace()
        started = time.monotonic()
        report = breakdown(
            "--timeout",
            "1",
            "--",
            sys.executable,
            "-I",
            "-c",
            "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)",
        )
        elapsed = time.monotonic() - started
        self.assertTrue(report["timed_out"], report)
        self.assertGreaterEqual(elapsed, 6, "SIGKILL comes 5 s after SIGTERM")
        self.assertLess(elapsed, 15)

    def test_a_traced_launch_names_paths_from_its_stderr_and_a_carried_report(self) -> None:
        self.need_namespace()
        first, second = loopback_server(), loopback_server()
        with tempfile.TemporaryDirectory() as scratch:
            out = Path(scratch)
            report = breakdown(
                "--trace",
                "--trace-out",
                str(out / "trace.log"),
                "--launch-stdout",
                str(out / "stdout.json"),
                "--measurement-out",
                str(out / "measurement.json"),
                "--",
                sys.executable,
                "-I",
                str(DOUBLE),
                f"{first}:down:{self.DOWN_SECOND}:stderr",
                f"{second}:up:{self.UP}:report",
            )
            trace = (out / "trace.log").read_text()
            carried = json.loads((out / "stdout.json").read_text())
            recorded = json.loads((out / "measurement.json").read_text())
        self.assertTrue(report["traced"])
        self.assertEqual(
            [tunnel["paths"] for tunnel in report["tunnels"]],
            [["/transfer/down"], ["/transfer/up"]],
        )
        # One line came from the launch's stderr, one from the report it
        # printed on stdout, and the kept trace holds both.
        self.assertEqual(trace.count("starting new connection"), 2, trace)
        self.assertIn("/transfer/up", carried["results"][0]["stderr"])
        self.assertEqual(len(recorded["tunnels"]), 2)
        self.assertEqual(recorded["received_bytes"], report["received_bytes"])


if __name__ == "__main__":
    unittest.main()
