"""The granted-Codex-turn budget's one command: it reads the run-overlay
workload's telemetry and reports the seconds a run-level allowlist adds.

    run_overlay_budget.py                 the figure, as onebudgetspec reads it
    run_overlay_budget.py --file PATH     read PATH instead

It measures nothing itself. scripts/e2e-control.sh's `run-overlay` phase runs
`live/control/run_allowlist_live.py --budget-telemetry` on its Linux leg, which
drives `oneharness run --control` Codex turns against a scripted model
endpoint — ungranted ones on the warm pooled app-server, then granted ones on
their own overlay homes — and writes
`target/e2e-telemetry/run-overlay/codex-turn.json` (or
`$OH_E2E_RUN_OVERLAY_FILE`), in the shape
`tools/network-telemetry/run_overlay_telemetry.py` declares and decodes for
both.

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
import os
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# The record's one shape, owned by the network-telemetry project this suite
# depends on (live/control/project.json).
sys.path.insert(0, str(ROOT / "tools" / "network-telemetry"))

from run_overlay_telemetry import BATCHES, Incomplete, TurnTiming, decode  # noqa: E402

BUDGETED = 2
"""The batch whose added seconds the budget holds: this host's peak of 2."""


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


def slowest(turns: list[TurnTiming]) -> TurnTiming:
    return max(turns, key=lambda turn: turn.seconds)


def phases(turn: TurnTiming) -> str:
    """The granted turn's overlay build, server start, turn and teardown."""
    build = turn.overlay.build_ms / 1000 if turn.overlay else 0.0
    teardown = turn.overlay.teardown_ms / 1000 if turn.overlay else 0.0
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
    record = decode(raw)
    added = {
        size: slowest(record.granted[size]).seconds - slowest(record.ungranted[size]).seconds
        for size in BATCHES
    }
    load = ", ".join(f"{value:.2f}" for value in record.load_average)
    ungranted = ", ".join(
        f"{size}: {slowest(record.ungranted[size]).seconds:.3f}s" for size in BATCHES
    )
    detail = (
        f"ungranted slowest {ungranted}; "
        f"added for 1 granted turn {added[1]:.3f}s, for 20 concurrent {added[20]:.3f}s; "
        f"slowest of {BUDGETED} granted: {phases(slowest(record.granted[BUDGETED]))}; "
        f"load average {load}; {record.codex_version}"
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
