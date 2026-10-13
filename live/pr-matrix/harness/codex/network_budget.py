# /// script
# requires-python = ">=3.11"
# dependencies = ["onebudgetspec-sdk==0.1.4"]
# ///
"""The live-codex network budgets' one command: it reads what the recorder wrote.

    network_budget.py                     largest bytes any Codex launch received
    network_budget.py --kind usage-probe  largest bytes any usage probe received
    network_budget.py --total             bytes the whole pass received
    network_budget.py --evidence-report   the calibration report of one pass (JSON)

It measures nothing itself. `just live-codex` runs scripts/e2e-codex.sh, whose
measuring shim (scripts/e2e-netshim.sh) and helpers (scripts/e2e-lib.sh) append
three kinds of line to `codex.jsonl` in `$OH_E2E_TELEMETRY_DIR` (default
`target/e2e-telemetry/network/` at the repository root), or `--file`:

    {"suite", "phase", "harness", "kind", "launch_id", "argv"}     a launch started
    {"launch_id", "method", "rx_bytes", "tx_bytes", ...}           it was measured
    {"suite", "phase", "kind", "expected": true}                   a phase must launch `kind`

The figures are bytes at each launch's own network-namespace boundary (pasta
TAP counters over its whole process tree), not host-NIC wire bytes. They exist
only for a complete record: every `start` line has exactly one measurement line
whose method measured (`netns-pasta`) with non-negative integer `rx_bytes` and
`tx_bytes`, every measurement line has a `start`, and every `expected` phase has
a measured launch of its kind — so one measured small launch can never stand in
for an unmeasured large one. Anything else fails every mode, naming each launch
or phase at fault, and onebudgetspec reports the budget as an error.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
MEASURED = "netns-pasta"
LAUNCH_CAP = 2_500_000
SUITE_CAP = 30_000_000


@dataclass(frozen=True)
class Launch:
    launch_id: str
    phase: str
    kind: str
    argv: list[str]
    rx: int
    tx: int


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


def telemetry_file() -> Path:
    directory = os.environ.get("OH_E2E_TELEMETRY_DIR")
    base = Path(directory) if directory else ROOT / "target" / "e2e-telemetry" / "network"
    return base / "codex.jsonl"


def byte_field(record: dict, key: str) -> int | None:
    value = record.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        return None
    return value


def load(path: Path) -> list[Launch]:
    """Every launch of a complete record, or Incomplete naming what is wrong."""
    if not path.is_file():
        raise Incomplete(
            f"no telemetry at {path}: run `just live-codex` (scripts/e2e-codex.sh records it), "
            "or point OH_E2E_TELEMETRY_DIR at the directory holding codex.jsonl"
        )
    starts: dict[str, dict] = {}
    measurements: dict[str, list[dict]] = {}
    expected: list[dict] = []
    faults: list[str] = []
    for number, text in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not text.strip():
            continue
        try:
            record = json.loads(text)
        except json.JSONDecodeError as error:
            faults.append(f"line {number} is not JSON ({error.msg})")
            continue
        if not isinstance(record, dict):
            faults.append(f"line {number} is not a JSON object")
        elif record.get("expected") is True:
            expected.append(record)
        elif "argv" in record:
            launch_id = record.get("launch_id")
            if not isinstance(launch_id, str) or not launch_id:
                faults.append(f"line {number} is a start line with no launch_id")
            elif launch_id in starts:
                faults.append(
                    f"launch {launch_id} has two start lines (duplicate launch id, line {number})"
                )
            else:
                starts[launch_id] = record
        elif isinstance(record.get("launch_id"), str):
            measurements.setdefault(record["launch_id"], []).append(record)
        else:
            faults.append(f"line {number} is neither a start, a measurement nor an expected line")
    launches: list[Launch] = []
    for launch_id, start in starts.items():
        where = f"launch {launch_id} (phase {start.get('phase')}, kind {start.get('kind')})"
        lines = measurements.get(launch_id, [])
        if not lines:
            faults.append(f"{where} has no measurement line")
            continue
        if len(lines) > 1:
            faults.append(
                f"{where} has {len(lines)} measurement lines; each launch is measured once"
            )
            continue
        line = lines[0]
        if line.get("method") != MEASURED:
            reason = f": {line['reason']}" if line.get("reason") else ""
            faults.append(f"{where} was not measured (method {line.get('method')!r}{reason})")
            continue
        rx, tx = byte_field(line, "rx_bytes"), byte_field(line, "tx_bytes")
        missing = [key for key, value in (("rx_bytes", rx), ("tx_bytes", tx)) if value is None]
        if rx is None or tx is None:
            faults.append(f"{where} has no non-negative integer {' or '.join(missing)}")
            continue
        argv = start.get("argv")
        launches.append(
            Launch(
                launch_id,
                str(start.get("phase")),
                str(start.get("kind")),
                [str(a) for a in argv] if isinstance(argv, list) else [],
                rx,
                tx,
            )
        )
    for launch_id in measurements:
        if launch_id not in starts:
            faults.append(f"a measurement line names launch {launch_id}, which has no start line")
    measured = {(launch.phase, launch.kind) for launch in launches}
    for want in expected:
        if (want.get("phase"), want.get("kind")) not in measured:
            faults.append(
                f"phase {want.get('phase')} expected a measured {want.get('kind')} launch and has none"
            )
    if not starts and not faults:
        faults.append(f"{path} records no launch at all")
    if faults:
        raise Incomplete(
            f"the record in {path} is incomplete:\n" + "\n".join(f"  - {fault}" for fault in faults)
        )
    return launches


def describe(launch: Launch) -> str:
    head = " ".join(launch.argv)[:120]
    return f"launch {launch.launch_id} (phase {launch.phase}, kind {launch.kind}: codex {head})"


def kinds(launches: list[Launch]) -> dict[str, list[Launch]]:
    grouped: dict[str, list[Launch]] = {}
    for launch in launches:
        grouped.setdefault(launch.kind, []).append(launch)
    return dict(sorted(grouped.items()))


def per_kind_lines(launches: list[Launch], largest: bool) -> list[str]:
    lines = []
    for kind, group in kinds(launches).items():
        parts = [f"{kind}: {len(group)} launch{'es' if len(group) != 1 else ''}"]
        if largest:
            parts.append(f"largest received {max(x.rx for x in group)} B")
            parts.append(f"largest sent {max(x.tx for x in group)} B")
        parts.append(f"total received {sum(x.rx for x in group)} B")
        parts.append(f"total sent {sum(x.tx for x in group)} B")
        lines.append(", ".join(parts))
    return lines


def largest_launch(launches: list[Launch]) -> tuple[float, str]:
    top = max(launches, key=lambda x: x.rx)
    detail = [
        f"largest: {describe(top)} received {top.rx} B (sent {top.tx} B; sent bytes are recorded, not gated)",
        *per_kind_lines(launches, largest=True),
    ]
    return float(top.rx), "\n".join(detail)


def usage_probe(launches: list[Launch]) -> tuple[float, str]:
    probes = [x for x in launches if x.phase == "usage-probe" and x.kind == "app-server"]
    if not probes:
        raise Incomplete(
            "phase usage-probe has no measured app-server launch: the record holds no usage probe"
        )
    top = max(probes, key=lambda x: x.rx)
    detail = [
        f"largest: {describe(top)} received {top.rx} B",
        f"usage probes: {len(probes)}",
        *(f"{x.launch_id}: received {x.rx} B, sent {x.tx} B" for x in probes),
    ]
    return float(top.rx), "\n".join(detail)


def suite_total(launches: list[Launch]) -> tuple[float, str]:
    total = sum(x.rx for x in launches)
    detail = [
        f"suite total: {total} B received over {len(launches)} launches (sent {sum(x.tx for x in launches)} B)",
        *per_kind_lines(launches, largest=False),
    ]
    return float(total), "\n".join(detail)


def model_turn(launch: Launch) -> bool:
    return launch.kind == "exec" or (
        launch.kind == "app-server" and launch.phase == "controlled-turn"
    )


def evidence_report(launches: list[Launch]) -> dict:
    """One pass's calibration figures, as the plan's ruling (d) states them."""
    top = max(launches, key=lambda x: x.rx)
    total = sum(x.rx for x in launches)
    turns = [x.rx for x in launches if model_turn(x)]
    per_kind = {}
    for kind, group in kinds(launches).items():
        per_kind[kind] = {
            "launches": len(group),
            "received": {
                "median": statistics.median(x.rx for x in group),
                "largest": max(x.rx for x in group),
                "total": sum(x.rx for x in group),
            },
            "sent": {
                "median": statistics.median(x.tx for x in group),
                "largest": max(x.tx for x in group),
                "total": sum(x.tx for x in group),
            },
        }
    return {
        "method": MEASURED,
        "boundary": "bytes at each launch's network-namespace boundary (pasta TAP), not host-NIC wire bytes",
        "launches": len(launches),
        "largest_launch": {
            "launch_id": top.launch_id,
            "phase": top.phase,
            "kind": top.kind,
            "received": top.rx,
        },
        "per_kind": per_kind,
        "model_turns": {
            "launches": len(turns),
            "median_received": statistics.median(turns) if turns else None,
        },
        "suite_total_received": total,
        "calibration": {
            "largest_x1_5_within_launch_cap": {
                "value": top.rx * 1.5,
                "cap": LAUNCH_CAP,
                "holds": top.rx * 1.5 <= LAUNCH_CAP,
            },
            "suite_total_x1_2_within_suite_cap": {
                "value": total * 1.2,
                "cap": SUITE_CAP,
                "holds": total * 1.2 <= SUITE_CAP,
            },
            "doubled_pass_exceeds_suite_cap": {
                "value": total * 2,
                "cap": SUITE_CAP,
                "holds": total * 2 > SUITE_CAP,
            },
        },
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--kind", choices=["usage-probe"], help="the largest launch of this phase alone"
    )
    mode.add_argument("--total", action="store_true", help="the pass's total received bytes")
    mode.add_argument(
        "--evidence-report", action="store_true", help="print the calibration report as JSON"
    )
    parser.add_argument("--file", type=Path, help="read this telemetry file instead of codex.jsonl")
    args = parser.parse_args(argv)
    path = args.file or telemetry_file()
    try:
        launches = load(path)
        if args.evidence_report:
            print(json.dumps(evidence_report(launches), indent=2))
            return 0
        if args.kind:
            value, detail = usage_probe(launches)
        elif args.total:
            value, detail = suite_total(launches)
        else:
            value, detail = largest_launch(launches)
    except Incomplete as error:
        print(f"network_budget: {error}", file=sys.stderr)
        return 1
    # The script's own environment (its PEP 723 header) has the SDK; the gate's
    # Python 3.9 workspace, which only lints and type-checks this file, does not.
    from onebudgetspec_sdk import report  # ty: ignore[unresolved-import]

    # Inside `onebudgetspec check` the result file carries it; run by hand, say it.
    if not report(value, detail):
        print(f"{value:.0f} B\n{detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
