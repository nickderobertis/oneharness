#!/usr/bin/env python3
"""Measure one launch's real network bytes, rootless, on Linux.

    netmeasure.py [--label L] [--env K=V]... [--timeout S] [--launch-stdout PATH] -- ARGV...

Runs ARGV with the given environment inside a fresh unprivileged user, network,
mount and PID namespace whose only route out is a `pasta` tap interface, and
prints one JSON line on stdout: the bytes the namespace's interface received and
sent while the launch's whole process tree lived, and the method.

Why that attributes bytes to exactly one launch's process tree:
  * every process the launch starts inherits the network namespace — a
    `setsid`/double-forked daemon included — and nothing else on the host is in
    it, so the interface counters are that tree's packets and nobody else's;
  * the PID namespace makes this script's init its reaper: a detached child
    reparents to it rather than to the host's init, so the measurement ends only
    when the last process of the tree has exited (the "whole life" of a
    long-lived server the launch left behind), and a deadline kills them all.
The counters are the kernel's per-interface `/proc/net/dev` figures: Ethernet
frames on the namespace's tap device, i.e. payload plus TCP/UDP, IP and
Ethernet headers, handshakes, ACKs and DNS — bytes on the wire, never a log.

The launch's stdout goes to stderr (or --launch-stdout) so stdout carries only
the report. Exit status: 0 measured (whatever the launch's own exit), 3 when no
measurement could be taken.
"""

import argparse
import ctypes
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
METHOD = "netns-pasta-ifcounters"
CLONE_NEWNS = 0x00020000
CLONE_NEWPID = 0x20000000


def read_counters() -> dict:
    """Every non-loopback interface's counters from this namespace's /proc/net/dev."""
    counters = {}
    for line in Path("/proc/net/dev").read_text().splitlines()[2:]:
        name, _, rest = line.partition(":")
        name = name.strip()
        if name == "lo":
            continue
        fields = [int(x) for x in rest.split()]
        counters[name] = {
            "rx_bytes": fields[0],
            "rx_packets": fields[1],
            "tx_bytes": fields[8],
            "tx_packets": fields[9],
        }
    return counters


def delta(start: dict, end: dict) -> dict:
    total = {"rx_bytes": 0, "rx_packets": 0, "tx_bytes": 0, "tx_packets": 0}
    for name, after in end.items():
        before = start.get(name, {k: 0 for k in total})
        for key in total:
            total[key] += after[key] - before[key]
    return total


def private_resolver() -> str:
    """Point the mount namespace's resolv.conf at a resolver reachable off-host.

    The host resolves through systemd-resolved on 127.0.0.53, which inside the
    namespace is its own empty loopback; the upstream list is the same resolver
    configuration minus the stub."""
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    current = Path("/etc/resolv.conf").read_text()
    servers = [l.split()[1] for l in current.splitlines() if l.startswith("nameserver")]
    if servers and all(s.startswith("127.") for s in servers):
        upstream = Path("/run/systemd/resolve/resolv.conf")
        if upstream.exists():
            subprocess.run(["mount", "--bind", str(upstream), "/etc/resolv.conf"], check=True)
            return str(upstream)
    return "/etc/resolv.conf"


def init(args: argparse.Namespace) -> int:
    """Runs inside pasta's user+net namespace; becomes PID 1 of a new PID namespace."""
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.unshare(CLONE_NEWNS | CLONE_NEWPID) != 0:
        raise OSError(ctypes.get_errno(), "unshare(CLONE_NEWNS|CLONE_NEWPID)")
    child = os.fork()
    if child:
        _, status = os.waitpid(child, 0)
        return os.waitstatus_to_exitcode(status)

    # PID 1 of the launch's PID namespace from here on.
    resolver = private_resolver()
    proc_mounted = subprocess.run(["mount", "-t", "proc", "proc", "/proc"]).returncode == 0
    signal.signal(signal.SIGTERM, lambda *_: os.kill(-1, signal.SIGKILL))
    stdout = open(args.launch_stdout, "wb") if args.launch_stdout else sys.stderr.buffer
    start = read_counters()
    t0 = time.monotonic()
    launch = subprocess.Popen(args.argv, stdout=stdout, start_new_session=False)
    launch_exit = None
    launch_seconds = None
    timed_out = False
    deadline = t0 + args.timeout if args.timeout > 0 else None
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break  # nothing left in the namespace: the tree is over
        if pid == launch.pid:
            launch_exit = os.waitstatus_to_exitcode(status)
            launch_seconds = time.monotonic() - t0
            launch.returncode = launch_exit
        if pid == 0:
            if deadline and time.monotonic() > deadline and not timed_out:
                timed_out = True
                os.kill(-1, signal.SIGKILL)
            time.sleep(0.02)
    tree_seconds = time.monotonic() - t0
    end = read_counters()
    result = {
        "launch_exit": launch_exit,
        "launch_seconds": round(launch_seconds or tree_seconds, 3),
        "tree_seconds": round(tree_seconds, 3),
        "outlived_launch": round(tree_seconds - (launch_seconds or tree_seconds), 3),
        "timed_out": timed_out,
        "interfaces": sorted(end),
        "resolver": resolver,
        "proc_remounted": proc_mounted,
        **delta(start, end),
    }
    Path(args.result).write_text(json.dumps(result))
    return 0


def find_pasta() -> str:
    explicit = os.environ.get("NETMEASURE_PASTA")
    if explicit:
        return explicit
    on_path = shutil.which("pasta")
    if on_path:
        return on_path
    fetched = subprocess.run(
        [str(HERE / "fetch-pasta.sh")], check=True, capture_output=True, text=True
    )
    return fetched.stdout.strip()


def main(argv: list) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--label", default="")
    parser.add_argument("--env", action="append", default=[], metavar="K=V")
    parser.add_argument("--timeout", type=float, default=0, help="kill the tree after S seconds")
    parser.add_argument("--launch-stdout")
    parser.add_argument("--_init", dest="result", help=argparse.SUPPRESS)
    parser.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.argv[:1] == ["--"]:
        args.argv = args.argv[1:]
    if not args.argv:
        parser.error("give the launch's argv after --")
    if args.result:
        return init(args)

    env = dict(os.environ)
    for pair in args.env:
        key, sep, value = pair.partition("=")
        if not sep:
            parser.error(f"--env wants K=V, got {pair!r}")
        env[key] = value
    pasta = find_pasta()
    scratch = tempfile.mkdtemp(prefix="netmeasure-")
    try:
        result_path = os.path.join(scratch, "result.json")
        inner = [sys.executable, str(Path(__file__).resolve()), "--_init", result_path,
                 "--timeout", str(args.timeout)]
        if args.launch_stdout:
            inner += ["--launch-stdout", os.path.abspath(args.launch_stdout)]
        command = [pasta, "--config-net", "--quiet", "-4",
                   "-t", "none", "-u", "none", "-T", "none", "-U", "none",
                   "--", *inner, "--", *args.argv]
        t0 = time.time()
        pasta_run = subprocess.run(command, env=env, stdout=sys.stderr)
        if not os.path.exists(result_path):
            print(json.dumps({"label": args.label, "method": METHOD, "error":
                              f"no measurement (pasta exit {pasta_run.returncode})"}))
            return 3
        result = json.loads(Path(result_path).read_text())
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    report = {"label": args.label, "method": METHOD, "started": round(t0, 3),
              "received_bytes": result["rx_bytes"], "sent_bytes": result["tx_bytes"],
              **result, "argv": args.argv}
    print(json.dumps(report), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
