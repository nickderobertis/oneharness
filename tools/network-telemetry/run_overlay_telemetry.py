"""The run-overlay budget's telemetry: one source for the record its writer
(`run_allowlist_live.py --budget-telemetry`) emits and its reader
(`run_overlay_budget.py`) decodes, and the collector that times each turn.

    {"codex_version", "platform", "load_average": [1m, 5m, 15m],
     "ungranted": {"1": [turn], "2": [turn, turn], "20": [20 turns]},
     "granted":   {"1": [...], "2": [...], "20": [...]}}
    turn: {"seconds", "started", "ended", "first_model_request",
           "overlay_timings": {"build_ms", "teardown_ms"} | null}

Kept to the Python 3.9 floor its reader shares (tools/network-telemetry).
"""

from __future__ import annotations

import math
import subprocess
import threading
import time
from dataclasses import dataclass
from typing import Optional

BATCHES = (1, 2, 20)
"""The batch sizes of the workload: 1 turn alone, 2 at once, 20 at once."""

SIDES = ("ungranted", "granted")

AGREEMENT = 0.002
"""How far a turn's seconds may differ from its end minus its start: the
rounding of three figures each kept to the millisecond."""


@dataclass(frozen=True)
class OverlayTimings:
    """What one granted turn's overlay cost, as oneharness reported it: the
    report's `overlay_timings` (`domain::report::OverlayTimings`), whose field
    names test_run_overlay_budget.py holds equal to the generated report
    schema's."""

    build_ms: int
    teardown_ms: int

    @staticmethod
    def from_json(value: object) -> Optional[OverlayTimings]:
        """`value` as overlay timings: an object of non-negative millisecond
        counts under exactly these field names, else None."""
        if not isinstance(value, dict):
            return None
        build, teardown = count(value.get("build_ms")), count(value.get("teardown_ms"))
        return None if build is None or teardown is None else OverlayTimings(build, teardown)


@dataclass(frozen=True)
class TurnTiming:
    """One `oneharness run --control` Codex turn, timed from outside."""

    seconds: float
    started: float
    ended: float
    first_model_request: Optional[float]
    overlay: Optional[OverlayTimings]

    def to_json(self) -> dict[str, object]:
        return {
            "seconds": self.seconds,
            "started": self.started,
            "ended": self.ended,
            "first_model_request": self.first_model_request,
            "overlay_timings": None
            if self.overlay is None
            else {"build_ms": self.overlay.build_ms, "teardown_ms": self.overlay.teardown_ms},
        }


@dataclass(frozen=True)
class Telemetry:
    """The workload's whole record."""

    codex_version: str
    platform: str
    load_average: tuple[float, ...]
    ungranted: dict[int, list[TurnTiming]]
    granted: dict[int, list[TurnTiming]]

    def to_json(self) -> dict[str, object]:
        def side(batches: dict[int, list[TurnTiming]]) -> dict[str, object]:
            return {
                str(size): [turn.to_json() for turn in turns] for size, turns in batches.items()
            }

        return {
            "codex_version": self.codex_version,
            "platform": self.platform,
            "load_average": list(self.load_average),
            "ungranted": side(self.ungranted),
            "granted": side(self.granted),
        }


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


def number(value: object) -> Optional[float]:
    """`value` as a finite number, or None (a bool is no number, and neither is
    an integer too large for a float)."""
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return None
    try:
        converted = float(value)
    except OverflowError:
        return None
    return converted if math.isfinite(converted) else None


MAX_COUNT = 10**12
"""The largest millisecond count a turn may report: about 31 years, so every
accepted count converts to a float exactly."""


def count(value: object) -> Optional[int]:
    """`value` as a non-negative integer no larger than MAX_COUNT, or None."""
    if isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= MAX_COUNT:
        return value
    return None


def decode_turn(
    entry: object, where: str, granted: bool, faults: list[str]
) -> Optional[TurnTiming]:
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
    if abs(seconds - (ended - started)) > AGREEMENT:
        faults.append(
            f"{where}'s seconds {seconds!r} disagree with its start {started!r} and end {ended!r}"
        )
        return None
    timings = entry.get("overlay_timings")
    overlay = None
    if granted:
        if not isinstance(timings, dict):
            faults.append(f"{where} carries no overlay_timings, so it ran on no overlay")
            return None
        overlay = OverlayTimings.from_json(timings)
        if overlay is None:
            faults.append(f"{where}'s overlay_timings {timings!r} are not two millisecond counts")
            return None
    raw_first = entry.get("first_model_request")
    first = number(raw_first)
    if raw_first is not None and (first is None or not started <= first <= ended):
        faults.append(
            f"{where}'s first model request {raw_first!r} is not a moment between its start "
            f"{started!r} and end {ended!r}"
        )
        return None
    return TurnTiming(seconds, started, ended, first, overlay)


def decode_side(
    record: dict[str, object], side: str, faults: list[str]
) -> dict[int, list[TurnTiming]]:
    raw = record.get(side)
    if not isinstance(raw, dict):
        faults.append(f"the {side} batches are {raw!r}, not an object")
        return {}
    batches: dict[int, list[TurnTiming]] = {}
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
        decoded = [turn for turn in turns if turn is not None]
        if len(decoded) == size:
            batches[size] = decoded
    return batches


def decode(record: object) -> Telemetry:
    """The record as Telemetry, or Incomplete naming every fault."""
    if not isinstance(record, dict):
        raise Incomplete("the telemetry holds no JSON object")
    faults: list[str] = []
    platform = record.get("platform")
    if platform != "linux":
        faults.append(f"the workload ran on {platform!r}; the budget is measured on linux")
    version = record.get("codex_version")
    if not isinstance(version, str) or version == "":
        faults.append(f"the record names no codex version: {version!r}")
    raw_load = record.get("load_average")
    load = tuple(number(value) for value in raw_load) if isinstance(raw_load, list) else ()
    readings = tuple(value for value in load if value is not None and value >= 0)
    if len(readings) != 3 or len(load) != 3:
        faults.append(f"the load average {raw_load!r} is not three non-negative readings")
    ungranted = decode_side(record, "ungranted", faults)
    granted = decode_side(record, "granted", faults)
    if faults or not isinstance(version, str):
        raise Incomplete("; ".join(faults))
    return Telemetry(version, "linux", readings, ungranted, granted)


@dataclass(frozen=True)
class Finished:
    """One process the collector ran: its own start and exit, and its output."""

    started: float
    ended: float
    returncode: int
    stdout: str
    stderr: str


def run_concurrently(argvs: list[list[str]], env: dict[str, str], timeout: float) -> list[Finished]:
    """Start every argv at once and time each from its own start to its own
    exit: one waiter thread per process, so a slow process collected first
    never lengthens a fast one's figure."""
    finished: list[Optional[Finished]] = [None] * len(argvs)

    def wait(index: int, started: float, process: subprocess.Popen[str]) -> None:
        try:
            out, err = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            out, err = process.communicate()
        finished[index] = Finished(started, time.time(), process.returncode, out, err)

    waiters = []
    for index, argv in enumerate(argvs):
        started = time.time()
        process = subprocess.Popen(  # noqa: S603 — each argv is the workload's own launch.
            argv,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        waiter = threading.Thread(target=wait, args=(index, started, process))
        waiter.start()
        waiters.append(waiter)
    for waiter in waiters:
        waiter.join()
    return [done for done in finished if done is not None]
