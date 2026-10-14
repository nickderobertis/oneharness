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
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
LEASES = 5


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


def telemetry_file() -> Path:
    override = os.environ.get("OH_E2E_REUSE_FILE", "")
    if override != "":
        return Path(override)
    return ROOT / "target" / "e2e-telemetry" / "app-server-reuse" / "reuse.json"


def reading(readings: Any, name: str, faults: list[str]) -> tuple[int, list[int]] | None:
    entry = readings.get(name) if isinstance(readings, dict) else None
    if not isinstance(entry, dict):
        faults.append(f"the {name} reading is missing")
        return None
    rss = entry.get("rss_bytes")
    if not isinstance(rss, int) or isinstance(rss, bool) or rss <= 0:
        faults.append(f"the {name} reading's rss_bytes is {rss!r}, not a positive byte count")
        return None
    pids = entry.get("pids")
    if not isinstance(pids, list) or not pids or not all(isinstance(p, int) for p in pids):
        faults.append(f"the {name} reading names no pids it summed")
        return None
    return rss, [int(pid) for pid in pids]


def figure(path: Path) -> tuple[float, str]:
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
    leases = record.get("leases")
    leases = leases if isinstance(leases, list) else []
    released = record.get("lease_count")
    if not isinstance(released, int) or released < LEASES or len(leases) < LEASES:
        faults.append(
            f"{released!r} leases were released ({len(leases)} recorded); the workload is {LEASES}"
        )
    pids = sorted({lease.get("server_pid") for lease in leases if isinstance(lease, dict)}, key=str)
    if len(pids) > 1:
        faults.append(f"the leases used {len(pids)} server pids {pids}; the workload is one server")
    readings = record.get("readings")
    first = reading(readings, "after_first", faults)
    last = reading(readings, "after_last", faults)
    if faults or first is None or last is None:
        raise Incomplete("; ".join(faults))
    ratio = last[0] / first[0]
    if not math.isfinite(ratio):
        raise Incomplete(f"the ratio {last[0]} / {first[0]} is not finite")
    detail = (
        f"after the last of {released} leases: {last[0]} B over pids {last[1]}; "
        f"after the first: {first[0]} B over pids {first[1]}; "
        f"server pid {pids[0] if pids else None}; {record.get('codex_version')}"
    )
    return ratio, detail


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--file", type=Path, help="read this telemetry file instead")
    args = parser.parse_args(argv)
    try:
        value, detail = figure(args.file or telemetry_file())
    except Incomplete as error:
        print(f"app_server_rss_budget: {error}", file=sys.stderr)
        return 1
    result = os.environ.get("ONEBUDGETSPEC_RESULT", "")
    if result != "":
        Path(result).write_text(json.dumps({"value": value, "detail": detail}), encoding="utf-8")
    else:
        print(f"{value:.6f}\n{detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
