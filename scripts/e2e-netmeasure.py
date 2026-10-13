#!/usr/bin/env python3
"""Run one live-tier launch in a measuring network namespace and record its bytes.

    e2e-netmeasure.py --line-file FILE --launch-id ID [--pending PATH] -- ARGV...

The Linux half of the network recorder (`scripts/e2e-netshim.sh` writes the
launch's `start` line, then execs this). ARGV runs inside a fresh unprivileged
user + network + mount + PID namespace whose only interface is a `pasta` TAP
device; when the launch's whole process tree has exited (every descendant,
detached and double-forked included, inherits the namespace and is reaped by
its init), one measurement line is appended to FILE:

    {"launch_id", "method": "netns-pasta", "rx_bytes", "tx_bytes",
     "payload_rx_estimate", "payload_tx_estimate",
     "namespace_tcp_retrans_segs", "seconds", "exit"}

`rx_bytes`/`tx_bytes` are the TAP interface's `/proc/net/dev` deltas: bytes at
the namespace boundary — payload plus per-frame Ethernet/IP/TCP headers — not
host-NIC wire bytes (pasta terminates the namespace's TCP and re-originates it
from host sockets). The payload estimates subtract 54 B per frame.

Where the namespace cannot be built (no pasta on PATH, unprivileged user
namespaces refused) the launch runs directly instead and the line is
`{"launch_id", "method": "unavailable", "reason", "seconds", "exit"}`.

Either way stdin, stdout and stderr are the launch's own and the exit status is
the launch's (128+N for a launch ended by signal N). A supervisor in its own
session owns the namespace and writes the line, so a caller that signals this
process — or SIGKILLs its whole process group, as oneharness does 100 ms after
a SIGTERM — still gets exactly one line: the supervisor sees this process gone,
stops the namespace and records what it measured.

Stopping the namespace, at the deadline (`OH_E2E_NET_DEADLINE` seconds after
the launch starts, default 1800, 0 for none) or because this process was
signalled, sends SIGTERM to every process in it and SIGKILL 5 s later.

`OH_E2E_NET_OBSERVER`, a JSON argv, is the self-check's seam: that command is
started inside the namespace before the counters are first read, with the
interface names as its last argument, and sent SIGTERM after they are read the
last time; the result's counter-read timestamps go to `OH_E2E_NET_WINDOWS`.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import math
import os
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import traceback
from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path
from typing import NamedTuple, NewType, Optional

METHOD = "netns-pasta"
# Ethernet 14 + IPv4 20 + TCP 20: what one TAP frame carries beyond its payload.
FRAME_HEADERS = 54
KILL_GRACE = 5.0
DEFAULT_DEADLINE = 1800.0
CLONE_NEWNS = 0x00020000
CLONE_NEWPID = 0x20000000
# How long after the tree ends the counters are read, so the closing FIN/ACK
# frames of its last connections are counted with it.
SETTLE = 0.2
NAMESPACE_ADVICE = (
    "install passt (`sudo apt-get install -y passt`, whose AppArmor profile permits pasta's "
    "namespaces) or allow unprivileged user namespaces "
    "(`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`)"
)


# The id the shim gave the launch, joining its measurement line to its `start`.
LaunchId = NewType("LaunchId", str)


class Counters(NamedTuple):
    """One interface's — or the namespace's summed — `/proc/net/dev` counters."""

    rx_bytes: int
    rx_frames: int
    tx_bytes: int
    tx_frames: int

    def __sub__(self, earlier: object) -> Counters:
        if not isinstance(earlier, Counters):
            return NotImplemented
        return Counters(*(now - then for now, then in zip(self, earlier)))


class Snapshot(NamedTuple):
    """The namespace's summed counters, between the two wall-clock instants of the read."""

    before_ns: int
    counters: Counters
    after_ns: int


@dataclass(frozen=True)
class Measured:
    """The line of a launch the namespace measured."""

    launch_id: LaunchId
    rx_bytes: int
    tx_bytes: int
    payload_rx_estimate: int
    payload_tx_estimate: int
    namespace_tcp_retrans_segs: int
    seconds: float
    exit: Optional[int]
    method: str = METHOD


@dataclass(frozen=True)
class Unavailable:
    """The line of a launch that ran without the namespace, and why."""

    launch_id: LaunchId
    reason: str
    seconds: float
    exit: Optional[int]
    method: str = "unavailable"


class Ending(Enum):
    """How the supervisor's work ended, as it tells the shim."""

    EXITED = "exited"  # the launch ran in the namespace; `code` is its status
    UNLAUNCHED = "unlaunched"  # the namespace never ran it; `reason` says why


@dataclass(frozen=True)
class Outcome:
    ending: Ending
    code: int = 0
    reason: str = ""

    def encode(self) -> bytes:
        return json.dumps(
            {"ending": self.ending.value, "code": self.code, "reason": self.reason}
        ).encode()

    @staticmethod
    def decode(message: bytes) -> Optional[Outcome]:
        """The supervisor's report, or None when it ended without a whole one."""
        try:
            fields = json.loads(message)
        except ValueError:
            return None
        if not isinstance(fields, dict):
            return None
        try:
            ending = Ending(fields.get("ending"))
        except ValueError:
            return None
        code, reason = fields.get("code"), fields.get("reason")
        if isinstance(code, bool) or not isinstance(code, int) or not isinstance(reason, str):
            return None
        return Outcome(ending, code, reason)


@dataclass(frozen=True)
class Window:
    """The wall-clock instants just before and just after one counter read."""

    before_ns: int
    after_ns: int

    @staticmethod
    def read(fields: object) -> Window:
        """A window as to_json wrote it, refusing anything else with ValueError."""
        if not isinstance(fields, dict):
            raise ValueError(f"a counter-read window is not an object: {fields!r}")
        before, after = fields.get("before_ns"), fields.get("after_ns")
        if (
            not isinstance(before, int)
            or not isinstance(after, int)
            or isinstance(before, bool)
            or isinstance(after, bool)
        ):
            raise ValueError(f"a counter-read window is not two nanosecond stamps: {fields!r}")
        if not 0 < before <= after:
            raise ValueError(f"a counter-read window ends before it begins: {fields!r}")
        return Window(before, after)


@dataclass(frozen=True)
class NamespaceResult:
    """What the namespace's init hands the supervisor in `result.json`."""

    moved: Counters
    namespace_tcp_retrans_segs: int
    seconds: float
    exit: Optional[int]
    start: Window
    end: Window

    def to_json(self) -> str:
        return json.dumps(
            {
                "moved": list(self.moved),
                "namespace_tcp_retrans_segs": self.namespace_tcp_retrans_segs,
                "seconds": self.seconds,
                "exit": self.exit,
                "start": asdict(self.start),
                "end": asdict(self.end),
            }
        )

    @staticmethod
    def from_json(text: str) -> NamespaceResult:
        """Read back what to_json wrote, refusing anything else with ValueError."""
        fields = json.loads(text)
        keys = ("moved", "namespace_tcp_retrans_segs", "seconds", "exit", "start", "end")
        if not isinstance(fields, dict) or not all(key in fields for key in keys):
            raise ValueError(f"the result is not an object with {', '.join(keys)}: {text[:200]!r}")
        moved, retrans = fields["moved"], fields["namespace_tcp_retrans_segs"]
        seconds, code = fields["seconds"], fields["exit"]
        counts = [*moved, retrans] if isinstance(moved, list) else []
        if len(counts) != 5 or not all(
            isinstance(n, int) and not isinstance(n, bool) and n >= 0 for n in counts
        ):
            raise ValueError(
                f"counters are not four non-negative integers and a retransmit count: {moved!r}"
            )
        if (
            isinstance(seconds, bool)
            or not isinstance(seconds, (int, float))
            or not math.isfinite(seconds)
            or seconds < 0
        ):
            raise ValueError(f"seconds is not a finite non-negative number: {seconds!r}")
        if code is not None and (isinstance(code, bool) or not isinstance(code, int)):
            raise ValueError(f"exit is not an integer or null: {code!r}")
        start, end = Window.read(fields["start"]), Window.read(fields["end"])
        if start.after_ns > end.before_ns:
            raise ValueError("the last counter read began before the first one ended")
        return NamespaceResult(Counters(*moved), retrans, float(seconds), code, start, end)

    def line(self, launch_id: LaunchId) -> Measured:
        return Measured(
            launch_id,
            self.moved.rx_bytes,
            self.moved.tx_bytes,
            self.moved.rx_bytes - FRAME_HEADERS * self.moved.rx_frames,
            self.moved.tx_bytes - FRAME_HEADERS * self.moved.tx_frames,
            self.namespace_tcp_retrans_segs,
            self.seconds,
            self.exit,
        )


def append_line(path: str, record: Measured | Unavailable) -> None:
    """Append one JSON line in a single write, so concurrent launches never interleave."""
    data = (json.dumps(asdict(record), separators=(",", ":")) + "\n").encode()
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
    try:
        os.write(fd, data)
    finally:
        os.close(fd)


def exit_code(status: int) -> int:
    """A wait status as a shell reports it: the exit code, or 128+N for signal N."""
    code = os.waitstatus_to_exitcode(status)
    return 128 - code if code < 0 else code


def deadline_seconds() -> float:
    text = os.environ.get("OH_E2E_NET_DEADLINE", "")
    if not text:
        return DEFAULT_DEADLINE
    try:
        value = float(text)
    except ValueError:
        value = -1.0
    if not math.isfinite(value) or value < 0:
        sys.exit(
            f"e2e-netmeasure: OH_E2E_NET_DEADLINE must be a finite, non-negative number of seconds, "
            f"got {text!r}; unset it for the {DEFAULT_DEADLINE:.0f} s default, or 0 for none"
        )
    return value


def observer_argv() -> Optional[list[str]]:
    """The self-check's observer command (`OH_E2E_NET_OBSERVER`), validated."""
    text = os.environ.get("OH_E2E_NET_OBSERVER")
    if text is None:
        return None
    try:
        decoded: object = json.loads(text)
    except ValueError:
        decoded = None
    words: list[str] = (
        [str(w) for w in decoded if isinstance(w, str) and w] if isinstance(decoded, list) else []
    )
    if not words or not isinstance(decoded, list) or len(words) != len(decoded):
        sys.exit(
            f"e2e-netmeasure: OH_E2E_NET_OBSERVER must be a JSON array of non-empty strings "
            f"(an argv), got {text!r}; unset it outside the recorder's self-check"
        )
    return words


def remove_pending(path: Optional[str]) -> None:
    """Remove the launch's pending marker once its line is in; a marker that
    cannot be removed is reported, since the budget step would wait on it."""
    if path is None:
        return
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    except OSError as error:
        print(
            f"e2e-netmeasure: could not remove the pending marker {path} ({error}); the budget "
            f"step will wait its bound for it — delete it by hand (rm -f '{path}')",
            file=sys.stderr,
        )


def remove_scratch(scratch: Path) -> None:
    """Remove the supervisor's scratch directory, naming any part left behind."""

    def report(_function: object, path: str, error: tuple[object, BaseException, object]) -> None:
        print(
            f"e2e-netmeasure: could not remove {path} ({error[1]}); delete {scratch} by hand (rm -rf '{scratch}')",
            file=sys.stderr,
        )

    shutil.rmtree(scratch, onerror=report)


def read_counters() -> dict[str, Counters]:
    """Every non-loopback interface's byte and frame counters in this network namespace."""
    counters: dict[str, Counters] = {}
    for line in Path("/proc/net/dev").read_text().splitlines()[2:]:
        name, _, rest = line.partition(":")
        name = name.strip()
        if name == "lo":
            continue
        fields = [int(x) for x in rest.split()]
        counters[name] = Counters(fields[0], fields[1], fields[8], fields[9])
    return counters


def tcp_retrans() -> int:
    rows = [
        r.split() for r in Path("/proc/net/snmp").read_text().splitlines() if r.startswith("Tcp:")
    ]
    return int(dict(zip(rows[0][1:], rows[1][1:]))["RetransSegs"])


def snapshot() -> Snapshot:
    before = time.time_ns()
    counters = read_counters()
    after = time.time_ns()
    summed = (
        Counters(*(sum(column) for column in zip(*counters.values())))
        if counters
        else Counters(0, 0, 0, 0)
    )
    return Snapshot(before, summed, after)


def private_resolver() -> None:
    """Give the mount namespace a resolv.conf whose nameservers are reachable off-host.

    A host resolving through systemd-resolved's 127.0.0.53 stub has, inside the
    namespace, only its own empty loopback there; the upstream list is the same
    configuration without the stub."""
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    current = Path("/etc/resolv.conf").read_text()
    words = [line.split() for line in current.splitlines()]
    servers = [w[1] for w in words if len(w) >= 2 and w[0] == "nameserver"]
    upstream = Path("/run/systemd/resolve/resolv.conf")
    if servers and all(s.startswith("127.") for s in servers) and upstream.exists():
        subprocess.run(["mount", "--bind", str(upstream), "/etc/resolv.conf"], check=True)


def namespace_init(scratch: Path, deadline: float, argv: list[str]) -> int:
    """Inside pasta's user and network namespace: run the launch as PID 1's child.

    Writes `launched` just before the launch starts and `result.json` once the
    whole tree has exited; the supervisor asks for a stop by creating `stop`."""
    observer = None
    observing = observer_argv()
    if observing is not None:
        # Outside the launch's PID namespace, so it is not part of the tree the
        # init waits for, but inside the network namespace it observes.
        observer = subprocess.Popen(
            [*observing, ",".join(sorted(read_counters()))],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
        )
        if observer.stdout is None or observer.stdout.readline().strip() != b"ready":
            observer.kill()
            sys.exit(
                f"e2e-netmeasure: the observer {observing[0]} exited {observer.wait()} without "
                "reporting ready; run it alone inside `pasta --config-net -- ...` to see its error"
            )
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.unshare(CLONE_NEWNS | CLONE_NEWPID) != 0:
        sys.exit(
            f"e2e-netmeasure: unshare(CLONE_NEWNS|CLONE_NEWPID): "
            f"{os.strerror(ctypes.get_errno())}; {NAMESPACE_ADVICE}"
        )
    child = os.fork()
    if child:
        _, status = os.waitpid(child, 0)
        if observer is not None:
            time.sleep(SETTLE)
            observer.terminate()
            observer.wait()
        return exit_code(status)

    # PID 1 of the launch's PID namespace from here on.
    private_resolver()
    subprocess.run(["mount", "-t", "proc", "proc", "/proc"], check=True)
    stop_requested = False

    def request_stop(*_: object) -> None:
        nonlocal stop_requested
        stop_requested = True

    signal.signal(signal.SIGTERM, request_stop)
    start = snapshot()
    retrans_start = tcp_retrans()
    t0 = time.monotonic()
    (scratch / "launched").touch()
    launch = subprocess.Popen(argv)
    launch_exit: Optional[int] = None
    stop_at = t0 + deadline if deadline > 0 else None
    kill_at: Optional[float] = None
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break  # nothing left in the namespace: the tree is over
        if pid == launch.pid:
            launch_exit = exit_code(status)
            launch.returncode = launch_exit
        if pid:
            continue
        now = time.monotonic()
        if kill_at is None and (
            stop_requested
            or (scratch / "stop").exists()
            or (stop_at is not None and now >= stop_at)
        ):
            os.kill(-1, signal.SIGTERM)
            kill_at = now + KILL_GRACE
        elif kill_at is not None and now >= kill_at:
            os.kill(-1, signal.SIGKILL)
            kill_at = now + 3600
        time.sleep(0.02)
    seconds = time.monotonic() - t0
    time.sleep(SETTLE)
    end = snapshot()
    result = NamespaceResult(
        end.counters - start.counters,
        tcp_retrans() - retrans_start,
        round(seconds, 3),
        launch_exit,
        Window(start.before_ns, start.after_ns),
        Window(end.before_ns, end.after_ns),
    )
    partial = scratch / "result.json.partial"
    partial.write_text(result.to_json())
    os.replace(partial, scratch / "result.json")
    return 0


def supervise(args: argparse.Namespace, pasta: str, scratch: Path, life: int) -> Outcome:
    """In its own session: run pasta, stop the namespace if the shim goes, write the line."""
    t0 = time.monotonic()
    inner = [sys.executable, os.path.abspath(__file__), "--_init", str(scratch), "--", *args.argv]
    no_ports = ["-t", "none", "-u", "none", "-T", "none", "-U", "none"]
    try:
        pasta_run = subprocess.Popen(
            [pasta, "--config-net", "--quiet", "-4", *no_ports, "--", *inner],
            stderr=subprocess.PIPE,
        )
    except OSError as error:
        return Outcome(
            Ending.UNLAUNCHED, reason=f"cannot start pasta ({pasta}): {error}; {NAMESPACE_ADVICE}"
        )
    if pasta_run.stderr is None:  # stderr=PIPE always gives one; this narrows the type
        raise RuntimeError("pasta started without its stderr pipe")
    stderr_tail = b""
    errors = pasta_run.stderr.fileno()
    watched = [life, errors]
    # Until pasta's stderr closes — the namespace's last process has exited —
    # relay it, and watch the shim: its end of `life` closing is the cue to stop.
    while errors in watched:
        ready, _, _ = select.select(watched, [], [])
        for stream in ready:
            chunk = os.read(stream, 65536)
            if stream == life:
                if not chunk:
                    (scratch / "stop").touch()
                    watched.remove(life)
            elif chunk:
                os.write(2, chunk)
                stderr_tail = (stderr_tail + chunk)[-2000:]
            else:
                watched.remove(stream)
    pasta_code = pasta_run.wait()
    result_path = scratch / "result.json"
    if result_path.exists():
        # An unreadable result raises here, into the supervisor's one failure
        # path (in measure), which records the launch as unmeasured, saying why.
        result = NamespaceResult.from_json(result_path.read_text())
        windows = os.environ.get("OH_E2E_NET_WINDOWS")
        if windows is not None:
            Path(windows).write_text(
                json.dumps({"start": asdict(result.start), "end": asdict(result.end)})
            )
        append_line(args.line_file, result.line(args.launch_id))
        return Outcome(Ending.EXITED, code=result.exit if result.exit is not None else 137)
    said = stderr_tail.decode(errors="replace").strip().replace("\n", " | ")[-500:] or "no message"
    if not (scratch / "launched").exists():
        return Outcome(
            Ending.UNLAUNCHED,
            reason=f"the measuring namespace could not start (pasta exit {pasta_code}: {said}); {NAMESPACE_ADVICE}",
        )
    append_line(
        args.line_file,
        Unavailable(
            args.launch_id,
            f"the measuring namespace ended without a measurement (pasta exit {pasta_code}: {said}); "
            "rerun the launch, and if it recurs run scripts/e2e-netmeasure.py by hand around it",
            round(time.monotonic() - t0, 3),
            None,
        ),
    )
    return Outcome(Ending.EXITED, code=137)


def run_direct(args: argparse.Namespace, reason: str) -> int:
    """Run the launch without the namespace and record why it was not measured."""
    t0 = time.monotonic()
    try:
        launch = subprocess.Popen(args.argv)
    except OSError as error:
        append_line(args.line_file, Unavailable(args.launch_id, reason, 0.0, 127))
        remove_pending(args.pending)
        print(
            f"e2e-netshim: cannot start {args.argv[0]}: {error}; check that the harness is still "
            "installed there (the shim resolved it at setup), then rerun the suite to resolve it again",
            file=sys.stderr,
        )
        return 127
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(signum, lambda n, _f: launch.send_signal(n))
    code = launch.wait()
    code = 128 - code if code < 0 else code
    append_line(
        args.line_file, Unavailable(args.launch_id, reason, round(time.monotonic() - t0, 3), code)
    )
    remove_pending(args.pending)
    return code


def measure(args: argparse.Namespace) -> int:
    pasta = shutil.which("pasta")
    if pasta is None:
        return run_direct(args, f"no pasta on PATH; {NAMESPACE_ADVICE}")
    scratch = Path(tempfile.mkdtemp(prefix="oh-netmeasure-"))
    life_r, life_w = os.pipe()
    done_r, done_w = os.pipe()
    supervisor = os.fork()
    if supervisor == 0:
        os.setsid()
        os.close(life_w)
        os.close(done_r)
        try:
            outcome = supervise(args, pasta, scratch, life_r)
        except Exception as error:
            # The shim still gets an answer and the launch a line: run it
            # directly if it never started, else record why it went unmeasured.
            traceback.print_exc()
            reason = (
                f"the measuring supervisor failed ({error!r}; its traceback is on stderr); rerun "
                "the launch, and if it recurs report that traceback against scripts/e2e-netmeasure.py"
            )
            if (scratch / "launched").exists():
                append_line(args.line_file, Unavailable(args.launch_id, reason, 0.0, None))
                outcome = Outcome(Ending.EXITED, code=137)
            else:
                outcome = Outcome(Ending.UNLAUNCHED, reason=reason)
        try:
            if outcome.ending is Ending.EXITED:
                remove_pending(args.pending)
            try:
                os.write(done_w, outcome.encode())
            except BrokenPipeError:
                pass  # the shim is gone — signalled away — and the line already recorded
        finally:
            remove_scratch(scratch)
            os._exit(0)
    os.close(life_r)
    os.close(done_w)

    def let_go(*_: object) -> None:
        # Closing the pipe is the supervisor's cue to stop the namespace; this
        # process keeps waiting for the launch's exit status.
        try:
            os.close(life_w)
        except OSError:
            pass

    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(signum, let_go)
    message = b""
    while True:
        try:
            chunk = os.read(done_r, 4096)
        except InterruptedError:
            continue
        if not chunk:
            break
        message += chunk
    os.waitpid(supervisor, 0)
    outcome = Outcome.decode(message)
    # llmlint: ignore[structural_pattern_matching] Linted and type-checked at the uv workspace's Python 3.9 floor, where `match` does not parse; the outcome is a closed Enum told apart by identity.
    if outcome is None:
        append_line(
            args.line_file,
            Unavailable(
                args.launch_id,
                "the measuring supervisor ended without reporting, so this launch's bytes are "
                "unknown; rerun it, and if it recurs run scripts/e2e-netmeasure.py by hand around it",
                0.0,
                None,
            ),
        )
        remove_pending(args.pending)
        return 137
    if outcome.ending is Ending.UNLAUNCHED:
        return run_direct(args, outcome.reason)
    return outcome.code


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--line-file")
    parser.add_argument("--launch-id")
    parser.add_argument("--pending", help="a marker file to remove once the line is written")
    parser.add_argument("--_init", dest="init", help=argparse.SUPPRESS)
    parser.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.argv[:1] == ["--"]:
        args.argv = args.argv[1:]
    if not args.argv:
        parser.error("give the launch's argv after --")
    if args.init:
        return namespace_init(Path(args.init), deadline_seconds(), args.argv)
    if not args.line_file or not args.launch_id:
        parser.error("--line-file and --launch-id are required")
    # Refuse a bad value before anything runs, not inside the namespace.
    deadline_seconds()
    observer_argv()
    return measure(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
