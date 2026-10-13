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
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import NamedTuple, NewType, Optional, Union

ROOT = Path(__file__).resolve().parents[4]
MEASURED = "netns-pasta"
SUITE = "codex"
# The caps the calibration report is stated against: budgets.yaml's thresholds,
# held equal to them by tools/network-telemetry's tests.
LAUNCH_CAP = 2_500_000
SUITE_CAP = 30_000_000

LaunchId = NewType("LaunchId", str)
# A launch kind as the shim derives it: `exec`, `app-server`,
# `shared-app-server` or `local`, else the Codex subcommand itself — an open
# set, since a Codex release can add subcommands the record must still count.
LaunchKind = NewType("LaunchKind", str)


@dataclass(frozen=True)
class Start:
    """A launch began: the shim's `start` line."""

    launch_id: LaunchId
    phase: str
    kind: LaunchKind
    argv: list[str]


@dataclass(frozen=True)
class Measurement:
    """A launch's measurement line; bytes are None when absent or not counts."""

    launch_id: LaunchId
    method: object
    reason: Optional[str]
    rx: Optional[int]
    tx: Optional[int]


@dataclass(frozen=True)
class Expectation:
    """A phase must produce a measured launch of this kind."""

    phase: str
    kind: LaunchKind


Line = Union[Start, Measurement, Expectation]


@dataclass(frozen=True)
class Launch:
    launch_id: LaunchId
    phase: str
    kind: LaunchKind
    argv: list[str]
    rx: int
    tx: int


class Figure(NamedTuple):
    """What one budget reports: its value and the detail onebudgetspec shows."""

    value: float
    detail: str


class Incomplete(Exception):
    """The record cannot yield a figure; the message names every fault."""


def telemetry_file() -> Path:
    directory = os.environ.get("OH_E2E_TELEMETRY_DIR")
    base = Path(directory) if directory else ROOT / "target" / "e2e-telemetry" / "network"
    return base / "codex.jsonl"


def byte_field(record: dict[str, object], key: str) -> Optional[int]:
    value = record.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        return None
    return value


def text_field(record: dict[str, object], key: str, where: str) -> str:
    value = record.get(key)
    if not isinstance(value, str) or not value:
        raise Incomplete(f"{where} has no {key} string")
    return value


# llmlint: ignore-block[structural_pattern_matching] This file is linted and type-checked at the uv workspace's Python 3.9 floor (ruff target py39, ty python-version 3.9), where `match` does not parse; the record shapes and the Line union are told apart by explicit checks instead.
def parse(number: int, record: object) -> Line:
    """One telemetry line as the recorder writes it, or Incomplete saying why not."""
    where = f"line {number}"
    if not isinstance(record, dict):
        raise Incomplete(f"{where} is not a JSON object")
    if record.get("expected") is True:
        if record.get("suite") != SUITE:
            raise Incomplete(
                f"{where} expects a launch of suite {record.get('suite')!r}, not {SUITE!r}"
            )
        return Expectation(
            text_field(record, "phase", where), LaunchKind(text_field(record, "kind", where))
        )
    if "argv" in record:
        launch_id = LaunchId(text_field(record, "launch_id", f"{where}, a start line,"))
        where = f"{where} (launch {launch_id})"
        for key, want in (("suite", SUITE), ("harness", SUITE)):
            if record.get(key) != want:
                raise Incomplete(
                    f"{where} names {key} {record.get(key)!r}, not {want!r}: it is not a live-codex launch"
                )
        argv = record["argv"]
        if not isinstance(argv, list) or not all(isinstance(word, str) for word in argv):
            raise Incomplete(f"{where} has an argv that is not a list of strings")
        kind = LaunchKind(text_field(record, "kind", where))
        return Start(launch_id, text_field(record, "phase", where), kind, argv)
    launch_id = LaunchId(
        text_field(record, "launch_id", f"{where}, neither a start nor an expected line,")
    )
    reason = record.get("reason")
    return Measurement(
        launch_id,
        record.get("method"),
        reason if isinstance(reason, str) else None,
        byte_field(record, "rx_bytes"),
        byte_field(record, "tx_bytes"),
    )


def load(path: Path) -> list[Launch]:
    """Every launch of a complete record, or Incomplete naming what is wrong."""
    if not path.is_file():
        raise Incomplete(
            f"no telemetry at {path}: run `just live-codex` (scripts/e2e-codex.sh records it), "
            "or point OH_E2E_TELEMETRY_DIR at the directory holding codex.jsonl"
        )
    starts: dict[LaunchId, Start] = {}
    measurements: dict[LaunchId, list[Measurement]] = {}
    expected: list[Expectation] = []
    faults: list[str] = []
    for number, text in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not text.strip():
            continue
        try:
            line = parse(number, json.loads(text))
        except json.JSONDecodeError as error:
            faults.append(f"line {number} is not JSON ({error.msg})")
            continue
        except Incomplete as fault:
            faults.append(str(fault))
            continue
        if isinstance(line, Expectation):
            expected.append(line)
        elif isinstance(line, Start):
            if line.launch_id in starts:
                faults.append(
                    f"launch {line.launch_id} has two start lines (duplicate launch id, line {number})"
                )
            else:
                starts[line.launch_id] = line
        else:
            measurements.setdefault(line.launch_id, []).append(line)
    launches: list[Launch] = []
    for launch_id, start in starts.items():
        where = f"launch {launch_id} (phase {start.phase}, kind {start.kind})"
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
        if line.method != MEASURED:
            reason = f": {line.reason}" if line.reason else ""
            faults.append(f"{where} was not measured (method {line.method!r}{reason})")
            continue
        if line.rx is None or line.tx is None:
            missing = [
                key
                for key, value in (("rx_bytes", line.rx), ("tx_bytes", line.tx))
                if value is None
            ]
            faults.append(f"{where} has no non-negative integer {' or '.join(missing)}")
            continue
        launches.append(Launch(launch_id, start.phase, start.kind, start.argv, line.rx, line.tx))
    for launch_id in measurements:
        if launch_id not in starts:
            faults.append(f"a measurement line names launch {launch_id}, which has no start line")
    measured = {(launch.phase, launch.kind) for launch in launches}
    for want in expected:
        if (want.phase, want.kind) not in measured:
            faults.append(f"phase {want.phase} expected a measured {want.kind} launch and has none")
    if not starts and not faults:
        faults.append(f"{path} records no launch at all")
    if faults:
        raise Incomplete(
            f"the record in {path} is incomplete:\n"
            + "\n".join(f"  - {fault}" for fault in faults)
            + "\nrerun `just live-codex` on a Linux host with pasta and unprivileged user namespaces "
            "(live/AGENTS.md); a launch named here went unmeasured or was recorded wrongly"
        )
    return launches


# llmlint: ignore-end[structural_pattern_matching]


def describe(launch: Launch) -> str:
    head = " ".join(launch.argv)[:120]
    return f"launch {launch.launch_id} (phase {launch.phase}, kind {launch.kind}: codex {head})"


def kinds(launches: list[Launch]) -> dict[LaunchKind, list[Launch]]:
    grouped: dict[LaunchKind, list[Launch]] = {}
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


def largest_launch(launches: list[Launch]) -> Figure:
    top = max(launches, key=lambda x: x.rx)
    detail = [
        f"largest: {describe(top)} received {top.rx} B (sent {top.tx} B; sent bytes are recorded, not gated)",
        *per_kind_lines(launches, largest=True),
    ]
    return Figure(float(top.rx), "\n".join(detail))


def usage_probe(launches: list[Launch]) -> Figure:
    probes = [x for x in launches if x.phase == "usage-probe" and x.kind == "app-server"]
    if not probes:
        raise Incomplete(
            "phase usage-probe has no measured app-server launch: the record holds no usage probe; "
            "check that scripts/e2e-codex.sh still runs oh_usage_enforce codex, then rerun `just live-codex`"
        )
    top = max(probes, key=lambda x: x.rx)
    detail = [
        f"largest: {describe(top)} received {top.rx} B",
        f"usage probes: {len(probes)}",
        *(f"{x.launch_id}: received {x.rx} B, sent {x.tx} B" for x in probes),
    ]
    return Figure(float(top.rx), "\n".join(detail))


def suite_total(launches: list[Launch]) -> Figure:
    total = sum(x.rx for x in launches)
    detail = [
        f"suite total: {total} B received over {len(launches)} launches (sent {sum(x.tx for x in launches)} B)",
        *per_kind_lines(launches, largest=False),
    ]
    return Figure(float(total), "\n".join(detail))


def model_turn(launch: Launch) -> bool:
    return launch.kind == "exec" or (
        launch.kind == "app-server" and launch.phase == "controlled-turn"
    )


@dataclass(frozen=True)
class Spread:
    median: float
    largest: int
    total: int


@dataclass(frozen=True)
class KindFigures:
    launches: int
    received: Spread
    sent: Spread


@dataclass(frozen=True)
class Condition:
    value: float
    cap: int
    holds: bool


@dataclass(frozen=True)
class LargestLaunch:
    launch_id: LaunchId
    phase: str
    kind: LaunchKind
    received: int


@dataclass(frozen=True)
class ModelTurns:
    launches: int
    median_received: Optional[float]


@dataclass(frozen=True)
class EvidenceReport:
    """One pass's calibration figures: the largest launch and the suite total
    against the caps, and whether each of the three conditions the caps are
    checked against holds for this pass — the largest launch x1.5 within the
    launch cap, the total x1.2 within the suite cap, and a doubled pass over
    the suite cap."""

    method: str
    boundary: str
    launches: int
    largest_launch: LargestLaunch
    per_kind: dict[str, KindFigures]
    model_turns: ModelTurns
    suite_total_received: int
    calibration: dict[str, Condition]


def spread(values: list[int]) -> Spread:
    return Spread(statistics.median(values), max(values), sum(values))


def evidence_report(launches: list[Launch]) -> EvidenceReport:
    top = max(launches, key=lambda x: x.rx)
    total = sum(x.rx for x in launches)
    turns = [x.rx for x in launches if model_turn(x)]
    return EvidenceReport(
        method=MEASURED,
        boundary="bytes at each launch's network-namespace boundary (pasta TAP), not host-NIC wire bytes",
        launches=len(launches),
        largest_launch=LargestLaunch(top.launch_id, top.phase, top.kind, top.rx),
        per_kind={
            kind: KindFigures(
                len(group), spread([x.rx for x in group]), spread([x.tx for x in group])
            )
            for kind, group in kinds(launches).items()
        },
        model_turns=ModelTurns(len(turns), statistics.median(turns) if turns else None),
        suite_total_received=total,
        calibration={
            "largest_x1_5_within_launch_cap": Condition(
                top.rx * 1.5, LAUNCH_CAP, top.rx * 1.5 <= LAUNCH_CAP
            ),
            "suite_total_x1_2_within_suite_cap": Condition(
                total * 1.2, SUITE_CAP, total * 1.2 <= SUITE_CAP
            ),
            "doubled_pass_exceeds_suite_cap": Condition(
                total * 2, SUITE_CAP, total * 2 > SUITE_CAP
            ),
        },
    )


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
            print(json.dumps(asdict(evidence_report(launches)), indent=2))
            return 0
        if args.kind:
            figure = usage_probe(launches)
        elif args.total:
            figure = suite_total(launches)
        else:
            figure = largest_launch(launches)
    except Incomplete as error:
        print(f"network_budget: {error}", file=sys.stderr)
        return 1
    # The script's own environment (its PEP 723 header) has the SDK; the gate's
    # Python 3.9 workspace, which only lints and type-checks this file, does not.
    from onebudgetspec_sdk import report  # ty: ignore[unresolved-import]

    # Inside `onebudgetspec check` the result file carries it; run by hand, say it.
    if not report(figure.value, figure.detail):
        print(f"{figure.value:.0f} B\n{figure.detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
