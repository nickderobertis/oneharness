"""The shared Codex app-server memory budget's one command: it reads the
sustained-reuse workload's telemetry and reports one ratio.

    app_server_rss_budget.py                 the ratio, as onebudgetspec reads it
    app_server_rss_budget.py --file PATH     read PATH instead

It measures nothing itself. `live/control/app-server-reuse.sh` (run by
scripts/e2e-control.sh on its Linux leg) has the pool start one server and
serve sequential leases, and writes
`target/e2e-telemetry/app-server-reuse/reuse.json` (or `$OH_E2E_REUSE_FILE`):

    {"codex_version", "platform", "lease_count",
     "leases": [{"lease", "server_pid", "thread"}, ...],
     "readings": {"after_first": {"rss_bytes", "pids"},
                  "after_last":  {"rss_bytes", "pids"}}}

The figure is the server's whole-tree resident memory after the last lease is
released over the same reading after the first. It exists only for a complete
record of the workload: Linux, at least five leases released, every lease on
one server pid, and both readings present and positive. Anything else fails,
naming what is missing, and onebudgetspec reports the budget as an error.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import NewType

ROOT = Path(__file__).resolve().parents[2]
LEASES = 5

Pid = NewType("Pid", int)
"""A process id the workload recorded: always positive."""


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


@dataclass(frozen=True)
class Reading:
    """One whole-tree resident memory reading of the server."""

    rss_bytes: int
    pids: list[Pid]


@dataclass(frozen=True)
class Lease:
    """One lease the workload took and released."""

    lease: int
    server_pid: Pid
    thread: str


@dataclass(frozen=True)
class ReuseRecord:
    """The workload's whole record, decoded: every field the figure reads."""

    platform: str
    codex_version: str
    leases: list[Lease]
    server: Pid
    after_first: Reading
    after_last: Reading


@dataclass(frozen=True)
class Figure:
    """What the budget reports: the ratio, and the detail onebudgetspec shows."""

    value: float
    detail: str


def telemetry_file() -> Path:
    override = os.environ.get("OH_E2E_REUSE_FILE", "")
    if override != "":
        return Path(override)
    return ROOT / "target" / "e2e-telemetry" / "app-server-reuse" / "reuse.json"


def positive_int(value: object) -> int | None:
    """`value` as a positive integer, or None (a bool is no count)."""
    if isinstance(value, int) and not isinstance(value, bool) and value > 0:
        return value
    return None


def decode_reading(
    readings: object, name: str, server: Pid | None, faults: list[str]
) -> Reading | None:
    entry = readings.get(name) if isinstance(readings, dict) else None
    if not isinstance(entry, dict):
        faults.append(f"the {name} reading is missing")
        return None
    rss = positive_int(entry.get("rss_bytes"))
    if rss is None:
        faults.append(
            f"the {name} reading's rss_bytes is {entry.get('rss_bytes')!r}, not a positive byte count"
        )
        return None
    raw = entry.get("pids")
    pids = [positive_int(pid) for pid in raw] if isinstance(raw, list) else []
    if not pids or any(pid is None for pid in pids):
        faults.append(f"the {name} reading's pids {raw!r} are not the positive pids it summed")
        return None
    summed = [Pid(pid) for pid in pids if pid is not None]
    if server is not None and server not in summed:
        faults.append(f"the {name} reading's pids {summed} leave out the server pid {server}")
        return None
    return Reading(rss, summed)


def decode_leases(record: dict[str, object], faults: list[str]) -> list[Lease]:
    """Every lease the record lists, each with its positive server pid."""
    raw = record.get("leases")
    entries = raw if isinstance(raw, list) else []
    released = record.get("lease_count")
    if not isinstance(released, int) or released < LEASES or released != len(entries):
        faults.append(
            f"{released!r} leases were released ({len(entries)} recorded); the workload is {LEASES}"
        )
    leases: list[Lease] = []
    for index, entry in enumerate(entries, start=1):
        pid = positive_int(entry.get("server_pid")) if isinstance(entry, dict) else None
        if pid is None or not isinstance(entry, dict):
            faults.append(f"lease {index} names no positive server pid: {entry!r}")
            continue
        number = entry.get("lease")
        thread = entry.get("thread")
        leases.append(
            Lease(
                number if isinstance(number, int) else index,
                Pid(pid),
                thread if isinstance(thread, str) else "",
            )
        )
    return leases


def one_server(leases: list[Lease], faults: list[str]) -> Pid | None:
    """The one server pid every lease ran on, or None with the fault named."""
    pids = {lease.server_pid for lease in leases}
    if len(pids) > 1:
        faults.append(
            f"the leases used {len(pids)} server pids {sorted(pids)}; the workload is one server"
        )
    return next(iter(pids)) if len(pids) == 1 else None


def decode(record: dict[str, object]) -> ReuseRecord:
    """The record as a ReuseRecord, or Incomplete naming every fault."""
    faults: list[str] = []
    platform = record.get("platform")
    if platform != "linux":
        faults.append(f"the workload ran on {platform!r}; the budget is measured on linux only")
    retained = record.get("retained", [])
    if not isinstance(retained, list):
        faults.append(f"the retained-thread list is {retained!r}, not a list")
    elif retained:
        faults.append(f"released threads {retained} were still loaded when the readings were taken")
    leases = decode_leases(record, faults)
    server = one_server(leases, faults)
    readings = record.get("readings")
    first = decode_reading(readings, "after_first", server, faults)
    last = decode_reading(readings, "after_last", server, faults)
    if faults or server is None or first is None or last is None:
        raise Incomplete("; ".join(faults))
    version = record.get("codex_version")
    return ReuseRecord(
        "linux", version if isinstance(version, str) else "", leases, server, first, last
    )


def figure(path: Path) -> Figure:
    if not path.is_file():
        raise Incomplete(
            f"no reuse telemetry at {path}; run live/control/app-server-reuse.sh first"
        )
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except ValueError as error:
        raise Incomplete(f"{path} is not JSON: {error}") from error
    if not isinstance(raw, dict):
        raise Incomplete(f"{path} holds no JSON object")
    record = decode(raw)
    first, last = record.after_first, record.after_last
    ratio = last.rss_bytes / first.rss_bytes
    if not math.isfinite(ratio):
        raise Incomplete(f"the ratio {last.rss_bytes} / {first.rss_bytes} is not finite")
    detail = (
        f"after the last of {len(record.leases)} leases: {last.rss_bytes} B over pids {last.pids}; "
        f"after the first: {first.rss_bytes} B over pids {first.pids}; "
        f"server pid {record.server}; {record.codex_version}"
    )
    return Figure(ratio, detail)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--file", type=Path, help="read this telemetry file instead")
    args = parser.parse_args(argv)
    try:
        measured = figure(args.file or telemetry_file())
    except Incomplete as error:
        print(f"app_server_rss_budget: {error}", file=sys.stderr)
        return 1
    result = os.environ.get("ONEBUDGETSPEC_RESULT", "")
    if result != "":
        Path(result).write_text(
            json.dumps({"value": measured.value, "detail": measured.detail}), encoding="utf-8"
        )
    else:
        print(f"{measured.value:.6f}\n{measured.detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
