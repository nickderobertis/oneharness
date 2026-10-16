"""The network recorder, driven as a live suite drives it: through the real shim.

The measuring cases need Linux, pasta and unprivileged user namespaces; where
those are absent they skip saying why, unless OH_NET_NO_SKIP=1 (CI's Linux leg)
makes that a failure. The fallback and refusal cases run everywhere.
"""

from __future__ import annotations

import dataclasses
import importlib.util
import json
import os
import signal
import subprocess
import sys
import threading
import time
import unittest
from pathlib import Path
from typing import Literal, NamedTuple

from support import (
    BASH,
    NETMEASURE,
    NETSHIM,
    OBSERVER,
    WINDOWS,
    Record,
    lines,
    measurements,
    need_recorder,
    path_without,
    recorder_unavailable,
    run_shim,
    scratch,
    server,
    starts,
    transfer_argv,
)


def unavailable_fields() -> set[str]:
    """The fields of the recorder's own unmeasured line (`Unavailable` in
    scripts/e2e-netmeasure.py), which every unmeasured line the shim writes
    itself must carry exactly."""
    spec = importlib.util.spec_from_file_location("netmeasure_fields", NETMEASURE)
    assert spec is not None and spec.loader is not None
    netmeasure = importlib.util.module_from_spec(spec)
    # Its dataclasses resolve their annotations through sys.modules.
    sys.modules["netmeasure_fields"] = netmeasure
    try:
        spec.loader.exec_module(netmeasure)
        return {field.name for field in dataclasses.fields(netmeasure.Unavailable)}
    finally:
        sys.modules.pop("netmeasure_fields", None)


DOWN = 5_000_000
UP = 3_000_000
UNRELATED = 7_777_777


class Frame(NamedTuple):
    """One frame the observer captured (observer.py's Frame, read back)."""

    stamp_ns: int
    direction: Literal["rx", "tx"]
    length: int


def frame_sums(frames: list[Frame], direction: str, low: int, high: int) -> int:
    return sum(f.length for f in frames if f.direction == direction and low <= f.stamp_ns <= high)


class MeasuredLaunch(unittest.TestCase):
    def test_counts_exactly_what_crossed_the_namespace_boundary_against_an_independent_capture(
        self,
    ) -> None:
        need_recorder(self)
        tmp = scratch(self)
        serving = server(self)
        heartbeat = tmp / "heartbeat"
        unrelated: dict[str, int] = {}

        # llmlint: ignore-block[async_typed_clients_at_boundaries] An unrelated host process moving opaque bytes from the tests' own loopback server, which the recorder must not count; no service API exists for a typed client to model.
        def unrelated_download() -> None:
            # A host process outside the launch, moving more than the launch
            # does across the whole of it, through the same server.
            import socket

            with socket.create_connection(("127.0.0.1", serving.port)) as conn:
                conn.sendall(f"GET {UNRELATED} 4.0\n".encode())
                got = 0
                while got < UNRELATED:
                    data = conn.recv(65536)
                    if not data:
                        break
                    got += len(data)
            unrelated["received"] = got

        # llmlint: ignore-end[async_typed_clients_at_boundaries]

        other = threading.Thread(target=unrelated_download)
        other.start()
        time.sleep(0.3)
        capture, windows = tmp / "capture.json", tmp / "windows.json"
        run = run_shim(
            tmp,
            transfer_argv(serving.port, DOWN, UP, "--detach", "--heartbeat", str(heartbeat)),
            ["exec", "--json", "probe"],
            env={
                "OH_E2E_PHASE": "self-check",
                "OH_E2E_NET_OBSERVER": json.dumps([sys.executable, str(OBSERVER), str(capture)]),
                "OH_E2E_NET_WINDOWS": str(windows),
            },
        )
        other.join()
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(unrelated.get("received"), UNRELATED)
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        self.assertEqual(
            (start["phase"], start["kind"], start["harness"], start["suite"]),
            ("self-check", "exec", "codex", "codex"),
        )
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "netns-pasta", line)
        # The detached child moved its payloads after the launch's own process
        # had exited, and they are in the figure.
        beats = heartbeat.read_text().splitlines()
        self.assertGreater(len(beats), 2)
        observed = json.loads(capture.read_text())
        frames = [Frame(f["stamp_ns"], f["direction"], f["length"]) for f in observed["frames"]]
        window = json.loads(windows.read_text())
        start, end = window["start"], window["end"]
        self.assertEqual(
            observed["drops"], 0, "the capture dropped frames, so it cannot vouch for the counters"
        )
        self.assertEqual(observed["truncated"], 0)
        self.assertLess(
            observed["ready_ns"],
            start["before_ns"],
            "the capture began after the first counter read",
        )
        self.assertGreater(
            observed["stopped_ns"],
            end["after_ns"],
            "the capture ended before the last counter read",
        )
        for direction, recorded, payload in (
            ("rx", line["rx_bytes"], DOWN),
            ("tx", line["tx_bytes"], UP),
        ):
            outer = frame_sums(frames, direction, start["before_ns"], end["after_ns"])
            inner = frame_sums(frames, direction, start["after_ns"], end["before_ns"])
            self.assertLessEqual(
                inner, recorded, f"{direction}: the counters missed captured frames"
            )
            self.assertLessEqual(
                recorded, outer, f"{direction}: the counters exceed every captured frame"
            )
            # Headers and control frames are in it: payload-only accounting
            # would sit below even the inner bound.
            self.assertGreater(
                inner, payload, f"{direction}: capture holds no more than the payload"
            )
        self.assertLess(
            line["rx_bytes"], DOWN + UNRELATED // 2, "the unrelated transfer was counted"
        )
        self.assertLess(line["tx_bytes"], UP + UNRELATED // 2)
        # The payload estimates are the counters less 54 B per captured frame,
        # and nothing was retransmitted on the namespace's lossless link.
        for direction, total, estimate in (
            ("rx", "rx_bytes", "payload_rx_estimate"),
            ("tx", "tx_bytes", "payload_tx_estimate"),
        ):
            counted = [
                f
                for f in frames
                if f.direction == direction and start["before_ns"] <= f.stamp_ns <= end["after_ns"]
            ]
            inner = [f for f in counted if start["after_ns"] <= f.stamp_ns <= end["before_ns"]]
            if len(inner) == len(counted):
                self.assertEqual(line[estimate], line[total] - 54 * len(counted), direction)
        self.assertGreaterEqual(line["payload_rx_estimate"], DOWN)
        self.assertGreaterEqual(line["payload_tx_estimate"], UP)
        self.assertEqual(line["namespace_tcp_retrans_segs"], 0)
        self.assertFalse(list((tmp / ".pending").glob("*")), "a pending marker outlived its line")

    def test_the_launch_resolves_through_the_hosts_upstream_resolvers(self) -> None:
        # Inside the namespace the host's 127.0.0.53 stub is the namespace's
        # own empty loopback; the recorder binds the upstream list over it.
        need_recorder(self)
        tmp = scratch(self)
        reader = tmp / "resolv-reader"
        reader.write_text("#!/usr/bin/env bash\ncat /etc/resolv.conf\n")
        reader.chmod(0o755)
        run = run_shim(tmp, [str(reader)], ["exec", "x"])
        self.assertEqual(run.returncode, 0, run.stderr)
        inside = [
            w.split()[1] for w in run.stdout.decode().splitlines() if w.startswith("nameserver")
        ]
        host = [
            w.split()[1]
            for w in Path("/etc/resolv.conf").read_text().splitlines()
            if w.startswith("nameserver")
        ]
        upstream = Path("/run/systemd/resolve/resolv.conf")
        self.assertTrue(inside, "the namespace has no nameserver")
        if host and all(server.startswith("127.") for server in host) and upstream.exists():
            self.assertFalse([s for s in inside if s.startswith("127.")], inside)
            want = [
                w.split()[1]
                for w in upstream.read_text().splitlines()
                if w.startswith("nameserver")
            ]
            self.assertEqual(inside, want)
        else:
            self.assertEqual(inside, host)

    def test_binary_stdin_and_stdout_and_the_exit_status_pass_through(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        blob = bytes(range(256)) * 512 + b"\x00\r\n\xff"
        run = run_shim(
            tmp, transfer_argv(0, 0, 0, "--echo", "--exit", "7"), ["exec", "-"], stdin=blob
        )
        self.assertEqual(run.returncode, 7, run.stderr)
        self.assertEqual(run.stdout, blob)
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual((line["method"], line["exit"]), ("netns-pasta", 7))

    def test_a_detached_descendant_is_stopped_at_the_deadline_and_recorded_once(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        heartbeat = tmp / "heartbeat"
        deadline = 2
        started = time.monotonic()
        run = run_shim(
            tmp,
            transfer_argv(
                0, 0, 0, "--detach", "--stubborn", "--linger", "600", "--heartbeat", str(heartbeat)
            ),
            ["exec", "stay"],
            env={"OH_E2E_NET_DEADLINE": str(deadline)},
        )
        took = time.monotonic() - started
        self.assertEqual(run.returncode, 0, run.stderr)
        # SIGTERM at the deadline is ignored; SIGKILL 5 s later is not.
        self.assertLess(took, deadline + 5 + 3, "the shim outlived its deadline and the kill grace")
        self.assertGreater(took, deadline + 4, "the stubborn child was not given its grace")
        size = heartbeat.stat().st_size
        time.sleep(0.5)
        self.assertEqual(heartbeat.stat().st_size, size, "the detached child is still alive")
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "netns-pasta")
        self.assertGreaterEqual(line["seconds"], deadline)

    def test_a_shim_its_caller_kills_with_its_process_group_still_records_once(self) -> None:
        # oneharness ends a launch it stops by SIGTERM to the process group
        # and SIGKILL 100 ms later; the line must survive the shim's death.
        need_recorder(self)
        tmp = scratch(self)
        heartbeat = tmp / "heartbeat"
        real = tmp / "real"
        real.write_text(
            f"#!/usr/bin/env bash\nexec {' '.join(transfer_argv(0, 0, 0, '--linger', '600', '--heartbeat', str(heartbeat)))}\n"
        )
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        shim = subprocess.Popen(
            [
                BASH,
                str(NETSHIM),
                "--suite",
                "codex",
                "--harness",
                "codex",
                "--real",
                str(real),
                "--self",
                str(tmp / "shims" / "codex"),
                "--file",
                str(file),
                "--",
                "app-server",
                "--stdio",
            ],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        for _ in range(100):
            if heartbeat.exists():
                break
            time.sleep(0.1)
        os.killpg(shim.pid, signal.SIGTERM)
        time.sleep(0.1)
        try:
            os.killpg(shim.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        shim.wait()
        for _ in range(100):
            records = lines(file)
            if len(records) == 2:
                break
            time.sleep(0.1)
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "netns-pasta", line)
        size = heartbeat.stat().st_size
        time.sleep(0.5)
        self.assertEqual(heartbeat.stat().st_size, size, "the launch outlived the stop")
        time.sleep(0.5)
        self.assertEqual(len(lines(file)), 2, "the launch was recorded more than once")


class EarlyStop(unittest.TestCase):
    """A stop can reach a launch's process group at any moment of the
    recorder's own start — while the shim encodes the start line, while the
    recorder's interpreter starts, before its namespace exists — as a suite
    stopped mid-phase does. Whenever it lands, a launch that wrote a start line
    gets exactly one measurement line and leaves no pending marker: one the
    stop reached is recorded as it ends, one stopped while the recorder still
    defers stops runs its course and is recorded then, and one stopped before
    its start line records nothing at all."""

    # Each launch in a process group of its own (bash job control), stopped
    # by SIGTERM to that group from bash's own `kill`: what a suite's runner
    # does, and on every platform the shim runs on — Git Bash delivers it to
    # its own processes on Windows.
    DRIVER = """set -m
shim=$1 real=$2 self=$3 file=$4
shift 4
pids=()
for delay in "$@"; do
  "$BASH" "$shim" --suite codex --harness codex --real "$real" --self "$self" \\
    --file "$file" -- exec x </dev/null >/dev/null 2>&1 &
  pid=$!
  sleep "$delay"
  kill -TERM -- "-$pid" 2>/dev/null || true
  pids+=("$pid")
done
for pid in "${pids[@]}"; do
  wait "$pid" || true
done
"""

    def test_a_stop_at_any_moment_of_the_recorders_start_leaves_each_start_line_one_line(
        self,
    ) -> None:
        tmp = scratch(self)
        real = tmp / "real"
        # A launch that outlives every stop but ends on its own: one whose
        # stop the recorder dropped runs to here and is recorded then.
        real.write_text("#!/usr/bin/env bash\nexec sleep 3\n")
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        # 0 to 145 ms: from the shim's first line to well past the recorder's
        # interpreter start, staggered across the launches.
        delays = [f"{step * 0.005:.3f}" for step in range(30)]
        driver = subprocess.run(
            [
                BASH,
                "-c",
                self.DRIVER,
                "driver",
                str(NETSHIM),
                str(real),
                str(tmp / "shims" / "codex"),
                str(file),
                *delays,
            ],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
        self.assertEqual(driver.returncode, 0, driver.stderr)
        pending = tmp / ".pending"
        deadline = time.monotonic() + 60
        while any(pending.glob("*")) and time.monotonic() < deadline:
            time.sleep(0.1)
        self.assertEqual(list(pending.glob("*")), [], "a stopped launch's marker outlived it")
        records = lines(file)
        for start in starts(records):
            (line,) = measurements(records, start["launch_id"])
            if line["method"] == "unavailable":
                self.assertEqual(set(line), unavailable_fields(), line)

    @unittest.skipIf(WINDOWS, "SIGKILL to a process group is POSIX")
    def test_a_caller_that_kills_its_launch_s_group_at_any_moment_leaves_each_start_line_one_line(
        self,
    ) -> None:
        # oneharness stops a launch by SIGTERM to its process group and
        # SIGKILL 100 ms later. Nothing in that group can write a line after
        # the SIGKILL, so whatever writes a start line must be out of its
        # reach first — on a loaded host the recorder's own start alone
        # outlasts the 100 ms. Each leg the recorder takes is driven: the
        # measuring one (where this host can measure), the one without pasta,
        # and the macOS one.
        legs = {
            "no-pasta": {"PATH": path_without(scratch(self), "pasta")},
            "macos": {"OH_E2E_OS": "Darwin"},
        }
        if sys.platform.startswith("linux") and not recorder_unavailable():
            legs["measured"] = {}
        for leg, env in legs.items():
            with self.subTest(leg=leg):
                tmp = scratch(self)
                real = tmp / "real"
                real.write_text("#!/usr/bin/env bash\nexec sleep 30\n")
                real.chmod(0o755)
                file = tmp / "codex.jsonl"
                # 0 to 290 ms before the SIGTERM: from the shim's first line
                # to well past the recorder's interpreter start.
                delays = [f"{step * 0.01:.2f}" for step in range(30)]
                driver = subprocess.run(
                    [
                        BASH,
                        "-c",
                        self.DRIVER.replace(
                            'kill -TERM -- "-$pid" 2>/dev/null || true',
                            'kill -TERM -- "-$pid" 2>/dev/null || true\n'
                            '  sleep 0.1\n  kill -KILL -- "-$pid" 2>/dev/null || true',
                        ),
                        "driver",
                        str(NETSHIM),
                        str(real),
                        str(tmp / "shims" / "codex"),
                        str(file),
                        *delays,
                    ],
                    stdin=subprocess.DEVNULL,
                    capture_output=True,
                    text=True,
                    timeout=120,
                    check=False,
                    env={**os.environ, **env},
                )
                self.assertEqual(driver.returncode, 0, driver.stderr)
                pending = tmp / ".pending"
                deadline = time.monotonic() + 60
                while any(pending.glob("*")) and time.monotonic() < deadline:
                    time.sleep(0.1)
                records = lines(file)
                lost = [
                    start["launch_id"]
                    for start in starts(records)
                    if len(measurements(records, start["launch_id"])) != 1
                ]
                self.assertEqual(lost, [], "a killed launch's start line has no one line")
                self.assertEqual(
                    list(pending.glob("*")), [], "a killed launch's marker outlived it"
                )


class Concurrency(unittest.TestCase):
    def test_launches_recorded_at_once_each_get_one_whole_line(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        serving = server(self)
        file = tmp / "codex.jsonl"
        real = tmp / "real"
        real.write_text(
            "#!/usr/bin/env bash\nexec "
            + " ".join(transfer_argv(serving.port, 300_000, 100_000))
            + "\n"
        )
        real.chmod(0o755)
        shims = [
            subprocess.Popen(
                [
                    BASH,
                    str(NETSHIM),
                    "--suite",
                    "codex",
                    "--harness",
                    "codex",
                    "--real",
                    str(real),
                    "--self",
                    str(tmp / "shims" / "codex"),
                    "--file",
                    str(file),
                    "--",
                    "exec",
                    str(n),
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            for n in range(8)
        ]
        self.assertEqual([shim.wait(timeout=120) for shim in shims], [0] * 8)
        records = lines(file)  # every line parses: none was interleaved
        launched = starts(records)
        self.assertEqual(len(launched), 8)
        self.assertEqual(len(records), 16)
        for launch in launched:
            (line,) = measurements(records, launch["launch_id"])
            self.assertGreater(line["rx_bytes"], 300_000)


class Cleanup(unittest.TestCase):
    def test_a_marker_or_scratch_the_recorder_cannot_remove_is_named(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        file = tmp / "codex.jsonl"
        own_tmp = tmp / "own-tmp"
        own_tmp.mkdir()
        real = tmp / "locking-double"
        real.write_text(f'#!/usr/bin/env bash\nchmod a-w "{tmp / ".pending"}" "{own_tmp}"\n')
        real.chmod(0o755)
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env={"TMPDIR": str(own_tmp)})
        (tmp / ".pending").chmod(0o755)
        own_tmp.chmod(0o755)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn(b"could not remove the pending marker", run.stderr)
        self.assertIn(b"by hand (rm -rf", run.stderr)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual(line["method"], "netns-pasta", "a cleanup failure cost the measurement")


class SupervisorLoss(unittest.TestCase):
    def test_a_supervisor_that_dies_still_leaves_the_launch_one_line(self) -> None:
        # The measuring supervisor is the line's writer; if it is lost, the
        # shim records that the launch went unmeasured and clears its marker.
        need_recorder(self)
        tmp = scratch(self)
        heartbeat = tmp / "heartbeat"
        real = tmp / "real"
        real.write_text(
            "#!/usr/bin/env bash\nexec "
            + " ".join(transfer_argv(0, 0, 0, "--linger", "3", "--heartbeat", str(heartbeat)))
            + "\n"
        )
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        shim = subprocess.Popen(
            [
                BASH,
                str(NETSHIM),
                "--suite",
                "codex",
                "--harness",
                "codex",
                "--real",
                str(real),
                "--self",
                str(tmp / "shims" / "codex"),
                "--file",
                str(file),
                "--",
                "exec",
                "x",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ, "TMPDIR": str(tmp)},
        )
        for _ in range(100):
            if heartbeat.exists():
                break
            time.sleep(0.1)
        # The supervisor: the shim's child that leads its own session.
        supervisors = []
        for stat in Path("/proc").glob("[0-9]*/stat"):
            try:
                fields = stat.read_text().rsplit(")", 1)[1].split()
            except OSError:
                continue
            pid = int(stat.parent.name)
            if int(fields[1]) == shim.pid and int(fields[3]) == pid:
                supervisors.append(pid)
        self.assertEqual(len(supervisors), 1, supervisors)
        os.kill(supervisors[0], signal.SIGKILL)
        self.assertEqual(shim.wait(timeout=30), 137)
        self.assertTrue(file.exists())
        records = lines(file)
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring supervisor ended without reporting", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")), "the pending marker outlived the line")
        time.sleep(3.5)  # the orphaned launch finishes its linger and the namespace goes

    def test_a_supervisor_that_fails_after_the_launch_records_it_unmeasured(self) -> None:
        # Any failure in the supervisor once the launch has run (here: the
        # self-check's windows file cannot be written) is recorded, not lost.
        need_recorder(self)
        tmp = scratch(self)
        (tmp / "a-directory").mkdir()
        run = run_shim(
            tmp,
            transfer_argv(0, 0, 0, "--exit", "3"),
            ["exec", "x"],
            env={"OH_E2E_NET_WINDOWS": str(tmp / "a-directory"), "TMPDIR": str(tmp)},
        )
        self.assertEqual(run.returncode, 137, run.stderr)
        self.assertIn(b"Traceback", run.stderr)
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring supervisor failed (IsADirectoryError", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")))


class Deadline(unittest.TestCase):
    def test_a_zero_deadline_leaves_the_launch_running_and_one_second_ends_it(self) -> None:
        need_recorder(self)
        for deadline, ran_out in (("0", False), ("1", True)):
            with self.subTest(deadline=deadline):
                tmp = scratch(self)
                run = run_shim(
                    tmp,
                    transfer_argv(0, 0, 0, "--linger", "3"),
                    ["exec", "x"],
                    env={"OH_E2E_NET_DEADLINE": deadline},
                )
                (start,) = starts(lines(tmp / "codex.jsonl"))
                (line,) = measurements(lines(tmp / "codex.jsonl"), start["launch_id"])
                if ran_out:
                    self.assertEqual(run.returncode, 128 + signal.SIGTERM, run.stderr)
                    self.assertLess(line["seconds"], 2.5)
                else:
                    self.assertEqual(run.returncode, 0, run.stderr)
                    self.assertGreaterEqual(line["seconds"], 3)


class Arguments(unittest.TestCase):
    @unittest.skipIf(WINDOWS, "the legs are named through OH_E2E_OS on unix")
    def test_a_long_argument_is_recorded_cut_and_passed_whole(self) -> None:
        tmp = scratch(self)
        got = tmp / "got"
        real = tmp / "argv-double"
        real.write_text(f'#!/usr/bin/env bash\nprintf "%s" "$2" >"{got}"\n')
        real.chmod(0o755)
        prompt = "p" * 199 + "Q" + "r" * 300
        run_shim(tmp, [str(real)], ["exec", prompt], env={"OH_E2E_OS": "Darwin"})
        self.assertEqual(got.read_text(), prompt, "the harness did not get the whole argument")
        (start,) = starts(lines(tmp / "codex.jsonl"))
        self.assertEqual(start["argv"], ["exec", "p" * 199 + "Q" + "…"])


class MalformedResult(unittest.TestCase):
    @unittest.skipUnless(
        sys.platform.startswith("linux"), "the recorder's namespace path is Linux's"
    )
    def test_a_malformed_namespace_result_is_recorded_as_unmeasured(self) -> None:
        # A stand-in pasta runs the launch itself and leaves a result the
        # supervisor must refuse, the way a namespace that wrote garbage would.
        tmp = scratch(self)
        fake = tmp / "fake-pasta-bin"
        fake.mkdir()
        (fake / "pasta").write_text(
            "#!/usr/bin/env bash\n"
            'while [ "$1" != --_init ]; do shift; done\n'
            'scratch="$2"; shift 3\n'
            ': >"$scratch/launched"\n'
            '"$@"\n'
            'printf \'{"moved": "garbage"}\' >"$scratch/result.json"\n'
        )
        (fake / "pasta").chmod(0o755)
        ran = tmp / "ran"
        real = tmp / "real"
        real.write_text(f'#!/usr/bin/env bash\ntouch "{ran}"\n')
        real.chmod(0o755)
        env = {"PATH": f"{fake}{os.pathsep}{os.environ['PATH']}", "TMPDIR": str(tmp)}
        run = run_shim(tmp, [str(real)], ["exec", "x"], env=env)
        self.assertEqual(run.returncode, 137, run.stderr)
        self.assertTrue(ran.exists())
        (start,) = starts(lines(tmp / "codex.jsonl"))
        (line,) = measurements(lines(tmp / "codex.jsonl"), start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring supervisor failed (ValueError", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")))
        self.assertEqual(list(tmp.glob("oh-netmeasure-*")), [])


class Unstarted(unittest.TestCase):
    def test_an_observer_that_never_reports_ready_falls_back_to_running_the_launch_once(
        self,
    ) -> None:
        # The self-check's observer exits without its `ready`: the namespace
        # never starts the launch, so it runs directly — once — unmeasured.
        need_recorder(self)
        tmp = scratch(self)
        ran = tmp / "ran"
        real = tmp / "counting-double"
        real.write_text(f'#!/usr/bin/env bash\necho run >>"{ran}"\n')
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        run = run_shim(
            tmp,
            [str(real)],
            ["exec", "x"],
            file=file,
            env={"OH_E2E_NET_OBSERVER": '["false"]', "TMPDIR": str(tmp)},
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn(b"without reporting ready", run.stderr)
        self.assertEqual(ran.read_text().splitlines(), ["run"], "the launch ran other than once")
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring namespace could not start", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")))
        self.assertEqual(
            sorted(p.name for p in tmp.glob("oh-netmeasure-*")),
            [],
            "the supervisor left its scratch",
        )

    @unittest.skipUnless(
        sys.platform.startswith("linux"), "the recorder's namespace path is Linux's"
    )
    def test_a_pasta_that_cannot_be_spawned_falls_back_to_running_the_launch_once(self) -> None:
        tmp = scratch(self)
        unspawnable = tmp / "unspawnable-pasta"
        unspawnable.mkdir()
        (unspawnable / "pasta").write_text("#!/nonexistent/interpreter\n")
        (unspawnable / "pasta").chmod(0o755)
        ran = tmp / "ran"
        real = tmp / "counting-double"
        real.write_text(f'#!/usr/bin/env bash\necho run >>"{ran}"\n')
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        env = {"PATH": f"{unspawnable}{os.pathsep}{os.environ['PATH']}", "TMPDIR": str(tmp)}
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(ran.read_text().splitlines(), ["run"], "the launch ran other than once")
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("cannot start pasta", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")))
        self.assertEqual(list(tmp.glob("oh-netmeasure-*")), [])

    def test_a_harness_that_cannot_start_inside_the_namespace_is_recorded_unmeasured(self) -> None:
        # The namespace is up and has begun the launch when the harness turns
        # out not to start: no measurement exists, and the line says so.
        need_recorder(self)
        tmp = scratch(self)
        real = tmp / "broken-interpreter"
        real.write_text("#!/nonexistent/interpreter\n")
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env={"TMPDIR": str(tmp)})
        self.assertEqual(run.returncode, 137, run.stderr)
        self.assertIn(b"interpreter", run.stderr)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring namespace ended without a measurement", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")))
        self.assertEqual(list(tmp.glob("oh-netmeasure-*")), [])

    @unittest.skipUnless(sys.platform.startswith("linux"), "a Linux host without pasta")
    def test_a_direct_launch_that_cannot_start_is_recorded_as_127(self) -> None:
        tmp = scratch(self)
        real = tmp / "broken-interpreter"
        real.write_text("#!/nonexistent/interpreter\n")
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        no_pasta = path_without(tmp, "pasta")
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env={"PATH": no_pasta})
        self.assertEqual(run.returncode, 127, run.stderr)
        self.assertIn(b"cannot start", run.stderr)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual((line["method"], line["exit"]), ("unavailable", 127))
        self.assertFalse(list((tmp / ".pending").glob("*")))


class Unwritable(unittest.TestCase):
    def test_a_measured_launch_whose_line_cannot_be_written_fails_the_shim(self) -> None:
        # The launch makes the telemetry file read-only; the supervisor, on the
        # host, cannot append its line, says which file and why, and the shim
        # exits 125 rather than pass the launch off as recorded.
        need_recorder(self)
        tmp = scratch(self)
        file = tmp / "codex.jsonl"
        real = tmp / "locking-double"
        real.write_text(f'#!/usr/bin/env bash\nchmod a-w "{file}"\nexit 4\n')
        real.chmod(0o755)
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env={"TMPDIR": str(tmp)})
        file.chmod(0o644)
        self.assertEqual(run.returncode, 125, run.stderr)
        self.assertIn(f"measurement line to {file}".encode(), run.stderr)
        (start,) = starts(lines(file))
        self.assertEqual(measurements(lines(file), start["launch_id"]), [])
        self.assertFalse(
            list((tmp / ".pending").glob("*")), "the pending marker outlived the failure"
        )

    def test_a_start_line_or_marker_that_cannot_be_written_refuses_the_launch(self) -> None:
        # The recorder writes the marker, then the start line, before the
        # launch — the measuring recorder on Linux, the shim's own fallback
        # elsewhere; where either cannot be written it says which and why, and
        # neither runs the launch nor leaves a marker the budget would wait on.
        for blocked, said in (("line", "cannot append to"), ("marker", "cannot create")):
            with self.subTest(blocked=blocked):
                tmp = scratch(self)
                ran = tmp / "ran"
                real = tmp / "real"
                real.write_text(f'#!/usr/bin/env bash\ntouch "{ran}"\n')
                real.chmod(0o755)
                file = tmp / "codex.jsonl"
                # A directory where the line file goes, or a file where the
                # markers' directory goes.
                if blocked == "line":
                    file.mkdir()
                else:
                    (tmp / ".pending").write_text("")
                run = run_shim(tmp, [str(real)], ["exec", "x"], file=file)
                self.assertNotEqual(run.returncode, 0, run.stderr)
                self.assertIn(said.encode(), run.stderr)
                self.assertFalse(ran.exists(), "the launch ran with nothing recorded for it")
                if blocked == "line":
                    self.assertFalse(
                        list((tmp / ".pending").glob("*")), "the marker outlived the refusal"
                    )
                else:
                    self.assertFalse(file.exists(), "a start line was written with no marker")

    def test_a_refused_start_line_whose_marker_cannot_be_removed_says_how_to_recover(
        self,
    ) -> None:
        # The line cannot be appended and the marker it already made cannot be
        # removed (an `rm` first on PATH that fails, so the shim's own
        # recording, with no python3 to hand it to): the refusal names the
        # marker the budget step would wait on, and how to clear it.
        tmp = scratch(self)
        ran = tmp / "ran"
        real = tmp / "real"
        real.write_text(f'#!/usr/bin/env bash\ntouch "{ran}"\n')
        real.chmod(0o755)
        failing = tmp / "failing-rm"
        failing.mkdir()
        (failing / "rm").write_text("#!/usr/bin/env bash\nexit 1\n")
        (failing / "rm").chmod(0o755)
        file = tmp / "codex.jsonl"
        file.mkdir()
        run = run_shim(
            tmp,
            [str(real)],
            ["exec", "x"],
            file=file,
            env={"PATH": f"{failing}{os.pathsep}{path_without(tmp, 'python3')}"},
        )
        self.assertEqual(run.returncode, 126, run.stderr)
        (marker,) = (tmp / ".pending").glob("*")
        self.assertIn(f"nor remove its pending marker {marker}".encode(), run.stderr)
        self.assertIn(b"remove the marker, then rerun", run.stderr)
        self.assertFalse(ran.exists(), "the launch ran with nothing recorded for it")


class LostResult(unittest.TestCase):
    def test_a_namespace_that_ran_the_launch_but_left_no_result_records_it_unmeasured(self) -> None:
        # The launch plants a directory where the namespace's init writes its
        # result (the scratch path is in the init's argv), so the init cannot
        # write it: the launch ran, and the supervisor records that it went
        # unmeasured. (A chmod would not do: the launch is root in its user
        # namespace.)
        need_recorder(self)
        tmp = scratch(self)
        own_tmp = tmp / "own-tmp"
        own_tmp.mkdir()
        real = tmp / "result-blocking-double"
        real.write_text(
            "#!/usr/bin/env bash\n"
            "scratch=\"$(tr '\\0' '\\n' </proc/1/cmdline | sed -n '/^--_init$/{n;p;q}')\"\n"
            'mkdir "$scratch/result.json.partial"\n'
        )
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env={"TMPDIR": str(own_tmp)})
        self.assertEqual(run.returncode, 137, run.stderr)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertIn("the measuring namespace ended without a measurement", line["reason"])
        self.assertFalse(list((tmp / ".pending").glob("*")), "the pending marker outlived the line")
        self.assertEqual(list(own_tmp.iterdir()), [], "the supervisor left its scratch behind")


class Fallback(unittest.TestCase):
    """Where the method cannot run, the launch still runs, unmeasured and said so."""

    def assert_unavailable(
        self, tmp: Path, run: subprocess.CompletedProcess[bytes], blob: bytes, reason: str
    ) -> None:
        self.assertEqual(run.returncode, 5, run.stderr)
        self.assertEqual(run.stdout, blob)
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        self.assertEqual(set(line), unavailable_fields(), line)
        self.assertIn(reason, line["reason"])
        self.assertEqual(line["exit"], 5)
        for field in ("rx_bytes", "tx_bytes", "payload_rx_estimate", "payload_tx_estimate"):
            self.assertNotIn(field, line)

    def echo(self, tmp: Path) -> Path:
        real = tmp / "echo-double"
        real.write_text("#!/usr/bin/env bash\ncat\nexit 5\n")
        real.chmod(0o755)
        return real

    @unittest.skipIf(WINDOWS, "the legs' platforms are named through OH_E2E_OS on unix")
    def test_macos_and_windows_legs_run_the_launch_unmeasured(self) -> None:
        for leg in ("Darwin", "MINGW64_NT-10.0-20348"):
            with self.subTest(leg=leg):
                tmp = scratch(self)
                blob = os.urandom(70_000) + b"\x00\r\n"
                run = run_shim(
                    tmp, [str(self.echo(tmp))], ["exec", "-"], stdin=blob, env={"OH_E2E_OS": leg}
                )
                self.assert_unavailable(tmp, run, blob, f"this leg is {leg}")

    @unittest.skipIf(WINDOWS, "the legs' platforms are named through OH_E2E_OS on unix")
    def test_a_native_binary_is_wrapped_under_a_utf8_locale(self) -> None:
        # A native harness (opencode's Mach-O on macOS) starts with bytes no
        # UTF-8 decodes, which BSD tr refuses under a UTF-8 locale; the
        # self-wrap check must still read them and run the launch.
        tmp = scratch(self)
        real = tmp / "native-double"
        real.write_bytes(b"#!/usr/bin/env bash\n# \xcf\xfa\xed\xfe\x80\xff\ncat\nexit 5\n")
        real.chmod(0o755)
        blob = os.urandom(4_096)
        run = run_shim(
            tmp,
            [str(real)],
            ["run", "hi"],
            harness="opencode",
            stdin=blob,
            env={"OH_E2E_OS": "Darwin", "LC_ALL": "en_US.UTF-8"},
        )
        self.assert_unavailable(tmp, run, blob, "this leg is Darwin")

    @unittest.skipUnless(sys.platform.startswith("linux"), "a Linux host without pasta")
    def test_a_linux_host_without_pasta_runs_the_launch_unmeasured(self) -> None:
        tmp = scratch(self)
        bare = tmp / "bare-path"
        bare.mkdir()
        for tool in (
            "bash",
            "jq",
            "python3",
            "cat",
            "od",
            "tr",
            "dirname",
            "uname",
            "date",
            "head",
            "grep",
            "mkdir",
            "rm",
        ):
            found = subprocess.run(
                ["bash", "-c", f"command -v {tool}"], capture_output=True, text=True
            ).stdout.strip()
            if found:
                (bare / tool).symlink_to(found)
        blob = os.urandom(70_000)
        run = run_shim(
            tmp, [str(self.echo(tmp))], ["exec", "-"], stdin=blob, env={"PATH": str(bare)}
        )
        self.assert_unavailable(tmp, run, blob, "no pasta on PATH")

    @unittest.skipIf(WINDOWS, "the legs are named through OH_E2E_OS on unix")
    def test_a_signal_to_the_fallbacks_process_group_ends_the_launch_and_is_recorded(self) -> None:
        # The macOS/Windows fallback runs the launch in the shim's own process
        # group; oneharness's SIGTERM to that group ends it, and the shim
        # still records it.
        tmp = scratch(self)
        heartbeat = tmp / "heartbeat"
        real = tmp / "real"
        real.write_text(
            "#!/usr/bin/env bash\nexec "
            + " ".join(transfer_argv(0, 0, 0, "--linger", "60", "--heartbeat", str(heartbeat)))
            + "\n"
        )
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        shim = subprocess.Popen(
            [
                BASH,
                str(NETSHIM),
                "--suite",
                "codex",
                "--harness",
                "codex",
                "--real",
                str(real),
                "--self",
                str(tmp / "shims" / "codex"),
                "--file",
                str(file),
                "--",
                "exec",
                "x",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ, "OH_E2E_OS": "Darwin"},
            start_new_session=True,
        )
        for _ in range(100):
            if heartbeat.exists():
                break
            time.sleep(0.1)
        os.killpg(shim.pid, signal.SIGTERM)
        self.assertEqual(shim.wait(timeout=30), 128 + signal.SIGTERM)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual((line["method"], line["exit"]), ("unavailable", 128 + signal.SIGTERM))
        size = heartbeat.stat().st_size
        time.sleep(0.5)
        self.assertEqual(heartbeat.stat().st_size, size, "the launch outlived the signal")

    @unittest.skipUnless(sys.platform.startswith("linux"), "a Linux host without pasta")
    def test_a_signal_to_an_unmeasured_launch_reaches_it_and_is_recorded(self) -> None:
        tmp = scratch(self)
        heartbeat = tmp / "heartbeat"
        real = tmp / "real"
        real.write_text(
            "#!/usr/bin/env bash\nexec "
            + " ".join(transfer_argv(0, 0, 0, "--linger", "60", "--heartbeat", str(heartbeat)))
            + "\n"
        )
        real.chmod(0o755)
        file = tmp / "codex.jsonl"
        no_pasta = path_without(tmp, "pasta")
        shim = subprocess.Popen(
            [
                BASH,
                str(NETSHIM),
                "--suite",
                "codex",
                "--harness",
                "codex",
                "--real",
                str(real),
                "--self",
                str(tmp / "shims" / "codex"),
                "--file",
                str(file),
                "--",
                "exec",
                "x",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ, "PATH": no_pasta},
        )
        for _ in range(100):
            if heartbeat.exists():
                break
            time.sleep(0.1)
        shim.send_signal(signal.SIGTERM)
        self.assertEqual(shim.wait(timeout=30), 128 + signal.SIGTERM)
        (start,) = starts(lines(file))
        (line,) = measurements(lines(file), start["launch_id"])
        self.assertEqual((line["method"], line["exit"]), ("unavailable", 128 + signal.SIGTERM))
        size = heartbeat.stat().st_size
        time.sleep(0.5)
        self.assertEqual(heartbeat.stat().st_size, size, "the launch outlived the signal")

    @unittest.skipUnless(sys.platform.startswith("linux"), "a Linux host without python3")
    def test_a_linux_host_without_python3_runs_the_launch_unmeasured(self) -> None:
        tmp = scratch(self)
        bare = tmp / "bare-path"
        bare.mkdir()
        for tool in (
            "bash",
            "jq",
            "cat",
            "od",
            "tr",
            "dirname",
            "uname",
            "date",
            "mkdir",
            "rm",
            "head",
            "grep",
            "pasta",
        ):
            found = subprocess.run(
                ["bash", "-c", f"command -v {tool}"], capture_output=True, text=True
            ).stdout.strip()
            if found:
                (bare / tool).symlink_to(found)
        blob = os.urandom(70_000)
        run = run_shim(
            tmp, [str(self.echo(tmp))], ["exec", "-"], stdin=blob, env={"PATH": str(bare)}
        )
        self.assert_unavailable(tmp, run, blob, "no python3 on PATH")

    @unittest.skipIf(WINDOWS, "the legs are named through OH_E2E_OS on unix")
    def test_a_launch_whose_line_cannot_be_written_fails_the_shim(self) -> None:
        # The launch ran, but an unrecorded launch must not pass for a
        # recorded one: the recorder exits 125, saying why — the measurer
        # where python3 runs it, the shim itself where none does.
        for leg, said in (
            ("measurer", b"cannot append launch "),
            ("no-python3", b"exited 4, but its measurement line could not be written"),
        ):
            with self.subTest(leg=leg):
                tmp = scratch(self)
                env = {"OH_E2E_OS": "Darwin"}
                if leg == "no-python3":
                    env["PATH"] = path_without(tmp, "python3")
                file = tmp / "codex.jsonl"
                real = tmp / "locking-double"
                real.write_text(f'#!/usr/bin/env bash\nchmod a-w "{file}"\nexit 4\n')
                real.chmod(0o755)
                run = run_shim(tmp, [str(real)], ["exec", "x"], file=file, env=env)
                file.chmod(0o644)
                self.assertEqual(run.returncode, 125, run.stderr)
                self.assertIn(said, run.stderr)
                (start,) = starts(lines(file))
                self.assertEqual(measurements(lines(file), start["launch_id"]), [])

    def test_this_platform_records_every_launch_once(self) -> None:
        # Whatever this host is, a launch through the shim gets one start and
        # one measurement line, measured or not.
        tmp = scratch(self)
        run = run_shim(tmp, [str(self.echo(tmp))], ["--version"], stdin=b"x")
        self.assertEqual(run.returncode, 5, run.stderr)
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        self.assertEqual(start["kind"], "local")
        self.assertEqual(len(measurements(records, start["launch_id"])), 1)


def stops() -> tuple[signal.Signals, ...]:
    """The stops the shim and the measurer defer while the launch starts,
    read when a test runs: SIGHUP is POSIX-only, and this module is imported
    on Windows too."""
    return (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)


@unittest.skipUnless(sys.platform.startswith("linux"), "the measurer the shim execs runs on Linux")
class StoppedWhileStarting(unittest.TestCase):
    """A suite stopped by its caller signals the whole group; one stop that
    lands after the shim's start line but before the measurer's handlers are
    in place must not leave that start without a line. A python3 first on
    PATH holds the shim in that window long enough to aim at."""

    def python(self) -> str:
        return subprocess.run(
            ["bash", "-c", "command -v python3"], capture_output=True, text=True, check=True
        ).stdout.strip()

    def start(self, tmp: Path, held: Path, real: Path, path: str) -> subprocess.Popen[bytes]:
        shim = subprocess.Popen(
            [
                BASH,
                str(NETSHIM),
                "--suite",
                "codex",
                "--harness",
                "codex",
                "--real",
                str(real),
                "--self",
                str(tmp / "shims" / "codex"),
                "--file",
                str(tmp / "codex.jsonl"),
                "--",
                "exec",
                "x",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ, "PATH": f"{held.parent}{os.pathsep}{path}"},
            start_new_session=True,
        )

        def reap() -> None:
            if shim.poll() is None:
                os.killpg(shim.pid, signal.SIGKILL)
                shim.wait()

        self.addCleanup(reap)
        return shim

    def stop_when_reached(
        self, shim: subprocess.Popen[bytes], reached: Path, stop: int = signal.SIGTERM
    ) -> None:
        for _ in range(300):
            if reached.exists():
                break
            time.sleep(0.05)
        self.assertTrue(reached.exists(), "the shim never reached its measurer")
        os.killpg(shim.pid, stop)

    def only_line(self, tmp: Path) -> Record:
        records = lines(tmp / "codex.jsonl")
        (start,) = starts(records)
        (line,) = measurements(records, start["launch_id"])
        self.assertEqual(list((tmp / ".pending").glob("*")), [], "a pending marker was left")
        return line

    def lingering(self, tmp: Path) -> Path:
        real = tmp / "real"
        real.write_text(
            "#!/usr/bin/env bash\nexec "
            + " ".join(transfer_argv(0, 0, 0, "--linger", "600"))
            + "\n"
        )
        real.chmod(0o755)
        return real

    def blocking_python(self, tmp: Path, stop: int) -> tuple[Path, Path]:
        # Holds `stop` blocked until the test's stop is pending, then execs
        # the real interpreter, so that stop reaches the measurer already
        # pending at its first instruction.
        reached = tmp / "reached"
        held = tmp / "held-python" / "python3"
        held.parent.mkdir()
        held.write_text(
            f"#!{self.python()} -I\n"
            "import os, signal, sys, time\n"
            f"signal.pthread_sigmask(signal.SIG_BLOCK, {{{int(stop)}}})\n"
            f"open({str(reached)!r}, 'w').close()\n"
            f"while {int(stop)} not in signal.sigpending():\n"
            "    time.sleep(0.01)\n"
            f"os.execv({self.python()!r}, [{self.python()!r}, *sys.argv[1:]])\n"
        )
        held.chmod(0o755)
        return held, reached

    def test_a_stop_before_the_measurer_is_ready_still_leaves_the_launch_one_line(self) -> None:
        tmp = scratch(self)
        reached = tmp / "reached"
        held = tmp / "slow-python" / "python3"
        held.parent.mkdir()
        held.write_text(
            f'#!/usr/bin/env bash\n: >"{reached}"\nsleep 1\nexec "{self.python()}" "$@"\n'
        )
        held.chmod(0o755)
        real = tmp / "echo-double"
        real.write_text("#!/usr/bin/env bash\ncat\nexit 5\n")
        real.chmod(0o755)
        shim = self.start(tmp, held, real, os.environ["PATH"])
        self.stop_when_reached(shim, reached)
        # Dropped, the stop leaves the launch to run its course and be recorded.
        self.assertEqual(shim.wait(timeout=60), 5)
        self.assertEqual(self.only_line(tmp)["exit"], 5)

    def test_a_stop_pending_as_a_direct_launch_starts_reaches_it_and_is_recorded(self) -> None:
        for stop in stops():
            with self.subTest(stop=stop.name):
                tmp = scratch(self)
                held, reached = self.blocking_python(tmp, stop)
                shim = self.start(tmp, held, self.lingering(tmp), path_without(tmp, "pasta"))
                self.stop_when_reached(shim, reached, stop)
                # The stop itself reaches the launch, which ends by it.
                self.assertEqual(shim.wait(timeout=60), 128 + stop)
                line = self.only_line(tmp)
                self.assertEqual((line["method"], line["exit"]), ("unavailable", 128 + stop))

    def test_a_stop_pending_as_a_measured_launch_starts_stops_its_namespace_and_is_recorded(
        self,
    ) -> None:
        need_recorder(self)
        for stop in stops():
            with self.subTest(stop=stop.name):
                tmp = scratch(self)
                held, reached = self.blocking_python(tmp, stop)
                shim = self.start(tmp, held, self.lingering(tmp), os.environ["PATH"])
                self.stop_when_reached(shim, reached, stop)
                # Any stop stops the namespace, whose init ends the launch by SIGTERM.
                self.assertEqual(shim.wait(timeout=60), 128 + signal.SIGTERM)
                line = self.only_line(tmp)
                self.assertEqual(
                    (line["method"], line["exit"]), ("netns-pasta", 128 + signal.SIGTERM)
                )


class Refusal(unittest.TestCase):
    def test_the_shim_refuses_to_wrap_itself_or_another_shim(self) -> None:
        tmp = scratch(self)
        own = tmp / "shims" / "codex"
        own.parent.mkdir(parents=True, exist_ok=True)
        own.write_text("#!/usr/bin/env bash\n# oneharness-network-shim suite=codex\nexit 0\n")
        own.chmod(0o755)
        other = tmp / "other-shim"
        other.write_text(own.read_text())
        other.chmod(0o755)
        for real in (own, other):
            with self.subTest(real=real.name):
                run = run_shim(tmp, [str(real)], ["exec", "x"])
                self.assertEqual(run.returncode, 126)
                self.assertIn(b"refusing to wrap", run.stderr)
                self.assertEqual(lines(tmp / "codex.jsonl"), [], "a refused launch was recorded")


class KindCase(NamedTuple):
    harness: str
    argv: list[str]
    kind: str


class Kinds(unittest.TestCase):
    """The launch kind each argv is recorded under (on the fallback, which is quick)."""

    CASES = tuple(
        KindCase(*case)
        for case in (
            ("codex", ["exec", "--json", "hi"], "exec"),
            ("codex", ["exec", "resume", "-c", "sandbox_mode=read-only", "th", "hi"], "exec"),
            ("codex", ["-c", "features.hooks=true", "exec", "hi"], "exec"),
            ("codex", ["app-server", "--disable", "plugins", "--stdio"], "app-server"),
            ("codex", ["app-server", "--listen", "unix:///tmp/s"], "shared-app-server"),
            ("codex", ["app-server", "--listen=unix:///tmp/s"], "shared-app-server"),
            ("codex", ["--version"], "local"),
            ("codex", ["features", "list", "-c", "features.hooks=true"], "local"),
            ("codex", ["execpolicy", "check", "--rules", "r", "rm"], "local"),
            ("codex", ["sandbox", "-c", "sandbox_mode=read-only", "--", "bash"], "local"),
            ("codex", ["login", "status"], "local"),
            ("codex", ["mcp", "list"], "mcp"),
            ("claude-code", ["-p", "hi", "--output-format", "json"], "turn"),
            ("claude-code", ["--version"], "local"),
            ("claude-code", ["auth", "status", "--json"], "local"),
        )
    )

    @unittest.skipIf(WINDOWS, "the fallback leg is named through OH_E2E_OS on unix")
    def test_each_argv_is_recorded_under_its_launch_kind(self) -> None:
        tmp = scratch(self)
        true = tmp / "true-double"
        true.write_text("#!/usr/bin/env bash\nexit 0\n")
        true.chmod(0o755)
        for case in self.CASES:
            run_shim(tmp, [str(true)], case.argv, harness=case.harness, env={"OH_E2E_OS": "Darwin"})
        recorded = [
            KindCase(s["harness"], s["argv"], s["kind"]) for s in starts(lines(tmp / "codex.jsonl"))
        ]
        self.assertEqual(recorded, list(self.CASES))


if __name__ == "__main__":
    unittest.main()
