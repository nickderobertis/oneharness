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
import os
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Optional

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


def append_line(path: str, record: dict[str, object]) -> None:
    """Append one JSON line in a single write, so concurrent launches never interleave."""
    data = (json.dumps(record, separators=(",", ":")) + "\n").encode()
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
    if value < 0:
        sys.exit(
            f"e2e-netmeasure: OH_E2E_NET_DEADLINE must be a non-negative number of seconds, "
            f"got {text!r}; unset it for the {DEFAULT_DEADLINE:.0f} s default"
        )
    return value


def remove_quietly(path: Optional[str]) -> None:
    if path is not None:
        try:
            os.unlink(path)
        except OSError:
            pass


# --- the namespace's side -------------------------------------------------


def read_counters() -> dict[str, tuple[int, int, int, int]]:
    """Every non-loopback interface's byte and frame counters in this network namespace."""
    counters: dict[str, tuple[int, int, int, int]] = {}
    for line in Path("/proc/net/dev").read_text().splitlines()[2:]:
        name, _, rest = line.partition(":")
        name = name.strip()
        if name == "lo":
            continue
        fields = [int(x) for x in rest.split()]
        counters[name] = (fields[0], fields[1], fields[8], fields[9])
    return counters


def tcp_retrans() -> int:
    rows = [
        r.split() for r in Path("/proc/net/snmp").read_text().splitlines() if r.startswith("Tcp:")
    ]
    return int(dict(zip(rows[0][1:], rows[1][1:]))["RetransSegs"])


def timed_counters() -> tuple[int, dict[str, tuple[int, int, int, int]], int]:
    """Counters with the wall-clock instants just before and just after the read."""
    before = time.time_ns()
    counters = read_counters()
    return before, counters, time.time_ns()


def private_resolver() -> None:
    """Give the mount namespace a resolv.conf whose nameservers are reachable off-host.

    A host resolving through systemd-resolved's 127.0.0.53 stub has, inside the
    namespace, only its own empty loopback there; the upstream list is the same
    configuration without the stub."""
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    current = Path("/etc/resolv.conf").read_text()
    servers = [line.split()[1] for line in current.splitlines() if line.startswith("nameserver")]
    upstream = Path("/run/systemd/resolve/resolv.conf")
    if servers and all(s.startswith("127.") for s in servers) and upstream.exists():
        subprocess.run(["mount", "--bind", str(upstream), "/etc/resolv.conf"], check=True)


def namespace_init(scratch: Path, deadline: float, argv: list[str]) -> int:
    """Inside pasta's user and network namespace: run the launch as PID 1's child.

    Writes `launched` just before the launch starts and `result.json` once the
    whole tree has exited; the supervisor asks for a stop by creating `stop`."""
    observer = None
    observer_argv = os.environ.get("OH_E2E_NET_OBSERVER")
    if observer_argv is not None:
        # Outside the launch's PID namespace, so it is not part of the tree the
        # init waits for, but inside the network namespace it observes.
        observer = subprocess.Popen(
            [*json.loads(observer_argv), ",".join(sorted(read_counters()))],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
        )
        if observer.stdout is None or observer.stdout.readline().strip() != b"ready":
            observer.kill()
            sys.exit("e2e-netmeasure: the observer did not report ready")
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.unshare(CLONE_NEWNS | CLONE_NEWPID) != 0:
        sys.exit(
            f"e2e-netmeasure: unshare(CLONE_NEWNS|CLONE_NEWPID): {os.strerror(ctypes.get_errno())}"
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
    start = timed_counters()
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
    end = timed_counters()
    rx = sum(c[0] for c in end[1].values()) - sum(c[0] for c in start[1].values())
    rx_frames = sum(c[1] for c in end[1].values()) - sum(c[1] for c in start[1].values())
    tx = sum(c[2] for c in end[1].values()) - sum(c[2] for c in start[1].values())
    tx_frames = sum(c[3] for c in end[1].values()) - sum(c[3] for c in start[1].values())
    result = {
        "rx_bytes": rx,
        "tx_bytes": tx,
        "payload_rx_estimate": rx - FRAME_HEADERS * rx_frames,
        "payload_tx_estimate": tx - FRAME_HEADERS * tx_frames,
        "namespace_tcp_retrans_segs": tcp_retrans() - retrans_start,
        "seconds": round(seconds, 3),
        "exit": launch_exit,
        "windows": {"start": [start[0], start[2]], "end": [end[0], end[2]]},
    }
    partial = scratch / "result.json.partial"
    partial.write_text(json.dumps(result))
    os.replace(partial, scratch / "result.json")
    return 0


# --- the host's side --------------------------------------------------------


def supervise(args: argparse.Namespace, pasta: str, scratch: Path, life: int, done: int) -> None:
    """In its own session: run pasta, stop the namespace if the shim goes, write the line.

    Tells the shim on `done` either `exit N` or `unlaunched REASON` (the
    namespace never ran the launch, so the shim runs it directly)."""
    t0 = time.monotonic()
    inner = [sys.executable, os.path.abspath(__file__), "--_init", str(scratch), "--", *args.argv]
    try:
        pasta_run = subprocess.Popen(
            [
                pasta,
                "--config-net",
                "--quiet",
                "-4",
                "-t",
                "none",
                "-u",
                "none",
                "-T",
                "none",
                "-U",
                "none",
                "--",
                *inner,
            ],
            stderr=subprocess.PIPE,
        )
    except OSError as error:
        os.write(
            done, f"unlaunched cannot start pasta ({pasta}): {error}; {NAMESPACE_ADVICE}".encode()
        )
        return
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
        result = json.loads(result_path.read_text())
        windows = os.environ.get("OH_E2E_NET_WINDOWS")
        if windows is not None:
            Path(windows).write_text(json.dumps(result.pop("windows")))
        else:
            result.pop("windows")
        append_line(args.line_file, {"launch_id": args.launch_id, "method": METHOD, **result})
        os.write(done, f"exit {result['exit'] if result['exit'] is not None else 137}".encode())
        return
    reason = stderr_tail.decode(errors="replace").strip().replace("\n", " | ")[-500:]
    if not (scratch / "launched").exists():
        os.write(
            done,
            f"unlaunched the measuring namespace could not start (pasta exit {pasta_code}: "
            f"{reason or 'no message'}); {NAMESPACE_ADVICE}".encode(),
        )
        return
    append_line(
        args.line_file,
        {
            "launch_id": args.launch_id,
            "method": "unavailable",
            "reason": f"the measuring namespace ended without a measurement (pasta exit {pasta_code}: {reason or 'no message'})",
            "seconds": round(time.monotonic() - t0, 3),
            "exit": None,
        },
    )
    os.write(done, b"exit 137")


def run_direct(args: argparse.Namespace, reason: str) -> int:
    """Run the launch without the namespace and record why it was not measured."""
    t0 = time.monotonic()
    try:
        launch = subprocess.Popen(args.argv)
    except OSError as error:
        append_line(
            args.line_file,
            {
                "launch_id": args.launch_id,
                "method": "unavailable",
                "reason": reason,
                "seconds": 0.0,
                "exit": 127,
            },
        )
        remove_quietly(args.pending)
        print(f"e2e-netshim: cannot start {args.argv[0]}: {error}", file=sys.stderr)
        return 127
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(signum, lambda n, _f: launch.send_signal(n))
    code = launch.wait()
    code = 128 - code if code < 0 else code
    append_line(
        args.line_file,
        {
            "launch_id": args.launch_id,
            "method": "unavailable",
            "reason": reason,
            "seconds": round(time.monotonic() - t0, 3),
            "exit": code,
        },
    )
    remove_quietly(args.pending)
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
            supervise(args, pasta, scratch, life_r, done_w)
            remove_quietly(args.pending)
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
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
    verb, _, rest = message.decode(errors="replace").partition(" ")
    if verb == "exit":
        return int(rest)
    if verb == "unlaunched":
        return run_direct(args, rest)
    append_line(
        args.line_file,
        {
            "launch_id": args.launch_id,
            "method": "unavailable",
            "reason": "the measuring supervisor ended without reporting",
            "seconds": 0.0,
            "exit": None,
        },
    )
    return 137


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
    deadline_seconds()  # refuse a bad value before anything runs
    return measure(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
