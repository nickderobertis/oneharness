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

ROOT = Path(__file__).resolve().parents[2]
LEASES = 5


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


@dataclass(frozen=True)
class Reading:
    """One whole-tree resident memory reading of the server."""

    rss_bytes: int
    pids: list[int]


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


def reading(readings: object, name: str, server: int | None, faults: list[str]) -> Reading | None:
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
    summed = [pid for pid in pids if pid is not None]
    if server is not None and server not in summed:
        faults.append(f"the {name} reading's pids {summed} leave out the server pid {server}")
        return None
    return Reading(rss, summed)


def server_pid(record: dict[str, object], faults: list[str]) -> int | None:
    """The one server pid every lease ran on, or None with the faults named."""
    leases = record.get("leases")
    leases = leases if isinstance(leases, list) else []
    released = record.get("lease_count")
    if not isinstance(released, int) or released < LEASES or released != len(leases):
        faults.append(
            f"{released!r} leases were released ({len(leases)} recorded); the workload is {LEASES}"
        )
    pids: set[int] = set()
    for index, lease in enumerate(leases, start=1):
        pid = positive_int(lease.get("server_pid")) if isinstance(lease, dict) else None
        if pid is None:
            faults.append(f"lease {index} names no positive server pid: {lease!r}")
        else:
            pids.add(pid)
    if len(pids) > 1:
        faults.append(
            f"the leases used {len(pids)} server pids {sorted(pids)}; the workload is one server"
        )
    return next(iter(pids)) if len(pids) == 1 else None


def figure(path: Path) -> Figure:
    if not path.is_file():
        raise Incomplete(
            f"no reuse telemetry at {path}; run live/control/app-server-reuse.sh first"
        )
    try:
        record = json.loads(path.read_text(encoding="utf-8"))
    except ValueError as error:
        raise Incomplete(f"{path} is not JSON: {error}") from error
    if not isinstance(record, dict):
        raise Incomplete(f"{path} holds no JSON object")
    faults: list[str] = []
    platform = record.get("platform")
    if platform != "linux":
        faults.append(f"the workload ran on {platform!r}; the budget is measured on linux only")
    retained = record.get("retained", [])
    if not isinstance(retained, list):
        faults.append(f"the retained-thread list is {retained!r}, not a list")
    elif retained:
        faults.append(f"released threads {retained} were still loaded when the readings were taken")
    server = server_pid(record, faults)
    readings = record.get("readings")
    first = reading(readings, "after_first", server, faults)
    last = reading(readings, "after_last", server, faults)
    if faults or first is None or last is None:
        raise Incomplete("; ".join(faults))
    ratio = last.rss_bytes / first.rss_bytes
    if not math.isfinite(ratio):
        raise Incomplete(f"the ratio {last.rss_bytes} / {first.rss_bytes} is not finite")
    detail = (
        f"after the last of {record['lease_count']} leases: {last.rss_bytes} B over pids {last.pids}; "
        f"after the first: {first.rss_bytes} B over pids {first.pids}; "
        f"server pid {server}; {record.get('codex_version')}"
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
