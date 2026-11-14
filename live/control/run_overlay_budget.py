"""The granted-Codex-turn budget's one command: it reads the run-overlay
workload's telemetry and reports the seconds a run-level allowlist adds.

    run_overlay_budget.py                 the figure, as onebudgetspec reads it
    run_overlay_budget.py --file PATH     read PATH instead

It measures nothing itself. scripts/e2e-control.sh's `run-overlay` phase runs
`live/run-allowlist/run_allowlist_live.py --budget-telemetry` on its Linux leg,
which drives `oneharness run --control` Codex turns against a scripted model
endpoint — ungranted ones on the warm pooled app-server, then granted ones on
their own overlay homes — and writes
`target/e2e-telemetry/run-overlay/codex-turn.json` (or
`$OH_E2E_RUN_OVERLAY_FILE`):

    {"codex_version", "platform", "load_average": [1m, 5m, 15m],
     "ungranted": {"1": [turn], "2": [turn, turn], "20": [20 turns]},
     "granted":   {"1": [...], "2": [...], "20": [...]}}
    turn: {"seconds", "started", "ended", "first_model_request",
           "overlay_timings": {"build_ms", "teardown_ms"} | null}

The figure is the slowest of the 2 concurrent granted turns' seconds, from
its oneharness process's start to its exit, minus the slowest of the 2
concurrent ungranted turns'. It exists only for a complete record: Linux,
every batch holding exactly its count of turns, each with positive seconds,
and every granted turn with the overlay timings it ran through. Anything else
fails, naming what is missing, and onebudgetspec reports the budget as an
error.
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
BATCHES = (1, 2, 20)
BUDGETED = 2


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


@dataclass(frozen=True)
class Turn:
    """One `oneharness run --control` Codex turn, timed from outside."""

    seconds: float
    started: float
    ended: float
    first_model_request: float | None
    build_ms: int | None
    teardown_ms: int | None


@dataclass(frozen=True)
class Figure:
    """What the budget reports: the seconds, and the detail onebudgetspec shows."""

    value: float
    detail: str


def telemetry_file() -> Path:
    override = os.environ.get("OH_E2E_RUN_OVERLAY_FILE", "")
    if override != "":
        return Path(override)
    return ROOT / "target" / "e2e-telemetry" / "run-overlay" / "codex-turn.json"


def number(value: object) -> float | None:
    """`value` as a finite number, or None (a bool is no number)."""
    if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value):
        return float(value)
    return None


def count(value: object) -> int | None:
    """`value` as a non-negative integer, or None."""
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return None


def decode_turn(entry: object, where: str, granted: bool, faults: list[str]) -> Turn | None:
    if not isinstance(entry, dict):
        faults.append(f"{where} is {entry!r}, not a turn record")
        return None
    seconds = number(entry.get("seconds"))
    started = number(entry.get("started"))
    ended = number(entry.get("ended"))
    if seconds is None or seconds <= 0:
        faults.append(f"{where}'s seconds are {entry.get('seconds')!r}, not a positive duration")
        return None
    if started is None or ended is None or ended < started:
        faults.append(f"{where} names no start and end it ran between")
        return None
    first = number(entry.get("first_model_request"))
    timings = entry.get("overlay_timings")
    build = teardown = None
    if granted:
        if not isinstance(timings, dict):
            faults.append(f"{where} carries no overlay_timings, so it ran on no overlay")
            return None
        build, teardown = count(timings.get("build_ms")), count(timings.get("teardown_ms"))
        if build is None or teardown is None:
            faults.append(f"{where}'s overlay_timings {timings!r} are not two millisecond counts")
            return None
    return Turn(seconds, started, ended, first, build, teardown)


def decode_side(record: dict[str, object], side: str, faults: list[str]) -> dict[int, list[Turn]]:
    raw = record.get(side)
    if not isinstance(raw, dict):
        faults.append(f"the {side} batches are {raw!r}, not an object")
        return {}
    batches: dict[int, list[Turn]] = {}
    for size in BATCHES:
        entries = raw.get(str(size))
        if not isinstance(entries, list) or len(entries) != size:
            got = len(entries) if isinstance(entries, list) else entries
            faults.append(f"the {side} batch of {size} holds {got!r} turns")
            continue
        turns = [
            decode_turn(entry, f"{side} batch {size} turn {index}", side == "granted", faults)
            for index, entry in enumerate(entries, start=1)
        ]
        if all(turn is not None for turn in turns):
            batches[size] = [turn for turn in turns if turn is not None]
    return batches


def slowest(turns: list[Turn]) -> Turn:
    return max(turns, key=lambda turn: turn.seconds)


def phases(turn: Turn) -> str:
    """The granted turn's overlay build, server start, turn and teardown."""
    build = (turn.build_ms or 0) / 1000
    teardown = (turn.teardown_ms or 0) / 1000
    if turn.first_model_request is None:
        return f"overlay build {build:.3f}s, teardown {teardown:.3f}s (no model request recorded)"
    start = max(turn.first_model_request - turn.started - build, 0.0)
    body = max(turn.ended - teardown - turn.first_model_request, 0.0)
    return (
        f"overlay build {build:.3f}s, server start {start:.3f}s, turn {body:.3f}s, "
        f"teardown {teardown:.3f}s"
    )


def figure(path: Path) -> Figure:
    if not path.is_file():
        raise Incomplete(
            f"no run-overlay telemetry at {path}; run scripts/e2e-control.sh's run-overlay phase"
        )
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except ValueError as error:
        raise Incomplete(f"{path} is not JSON: {error}") from error
    if not isinstance(raw, dict):
        raise Incomplete(f"{path} holds no JSON object")
    faults: list[str] = []
    if raw.get("platform") != "linux":
        faults.append(
            f"the workload ran on {raw.get('platform')!r}; the budget is measured on linux"
        )
    version = raw.get("codex_version")
    if not isinstance(version, str) or version == "":
        faults.append(f"the record names no codex version: {version!r}")
    ungranted = decode_side(raw, "ungranted", faults)
    granted = decode_side(raw, "granted", faults)
    if faults:
        raise Incomplete("; ".join(faults))
    added = {
        size: slowest(granted[size]).seconds - slowest(ungranted[size]).seconds for size in BATCHES
    }
    load = raw.get("load_average")
    load_text = (
        ", ".join(f"{value:.2f}" for value in load)
        if isinstance(load, list) and all(number(v) is not None for v in load)
        else "unrecorded"
    )
    detail = (
        f"ungranted slowest {', '.join(f'{size}: {slowest(ungranted[size]).seconds:.3f}s' for size in BATCHES)}; "
        f"added for 1 granted turn {added[1]:.3f}s, for 20 concurrent {added[20]:.3f}s; "
        f"slowest of {BUDGETED} granted: {phases(slowest(granted[BUDGETED]))}; "
        f"load average {load_text}; {version}"
    )
    return Figure(added[BUDGETED], detail)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--file", type=Path, help="read this telemetry file instead")
    args = parser.parse_args(argv)
    try:
        measured = figure(args.file or telemetry_file())
    except Incomplete as error:
        print(f"run_overlay_budget: {error}", file=sys.stderr)
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
