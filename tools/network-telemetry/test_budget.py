"""live-codex's network budgets, over lines the recorder itself wrote.

Fixtures are recorded by the real writers — scripts/e2e-lib.sh's `_oh_expect`
and the shim around a do-nothing double — and then copied with their byte
fields set explicitly, so a threshold is tested at its exact value without
equating a payload to TAP traffic. The budgets are checked by the step `just
live-codex` runs (scripts/e2e-network-budget.sh → `onebudgetspec check`).
They hold on Linux alone, where the recorder measures, so every case here
needs it.
"""

from __future__ import annotations

import copy
import json
import math
import os
import re
import subprocess
import unittest
from pathlib import Path
from typing import Callable, NamedTuple, Optional

from support import (
    BASH,
    BUDGETS,
    LIB,
    NETSHIM,
    ROOT,
    WINDOWS,
    LaunchId,
    Record,
    budget,
    check_step,
    lines,
    measurements,
    need_builds,
    need_recorder,
    run_suite,
    scratch,
    server,
    starts,
    suite_env,
    write_lines,
)

EXEC = ["exec", "-c", "features.plugins=false", "--json", "a one-line prompt"]
RESUME = ["exec", "resume", "-c", "sandbox_mode=read-only", "--json", "th", "a one-line prompt"]
APP_SERVER = ["app-server", "-c", "features.plugins=false", "--stdio"]
SHARED = ["app-server", "--listen", "unix:///tmp/oh.sock"]
VERSION = ["--version"]
FEATURES = ["features", "list", "-c", "features.hooks=true"]
EXECPOLICY = ["execpolicy", "check", "--rules", "r", "rm"]
SANDBOX = ["sandbox", "-c", "sandbox_mode=read-only", "--", "bash"]


class Phase(NamedTuple):
    """One phase of a recorded fixture: the launch kind it must produce (None
    for a phase that expects nothing) and each launch's argv."""

    name: str
    expects: Optional[str]
    launches: list[list[str]]


Plan = list[Phase]

# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] This is the budget workload as the task's acceptance criteria state it (50 default-config launches), not a restatement of what the suite launches today: the recorder now also sees oneharness's `--version` probe before every launch (97 in the committed pass), and the thresholds are tested at their exact values over this stated workload. What the suite really launches is held by the committed evidence and its recomputation (class Evidence).
# The budget workload as the task's acceptance criteria state it: 50
# default-config launches — 22 model turns, 1 usage probe, 26 network-free
# launches and 1 controlled turn.
WORKLOAD: Plan = [
    Phase("run", "exec", [EXEC, VERSION]),
    Phase("controlled-turn", "app-server", [APP_SERVER, VERSION]),
    Phase("usage-probe", "app-server", [APP_SERVER, VERSION]),
    Phase(
        "codex-rules-enforce", "exec", [EXEC, EXEC, EXECPOLICY, EXECPOLICY, EXECPOLICY, EXECPOLICY]
    ),
    Phase("long-prompt-enforce", "exec", [["exec", "--json", "-"], VERSION]),
    Phase("mode-enforce", "exec", [EXEC, EXEC, SANDBOX, SANDBOX]),
    Phase("resume-mode-enforce", "exec", [EXEC, RESUME] * 4 + [SANDBOX] * 4),
    Phase("codex-feature-flag-order", None, [FEATURES] * 8),
    Phase("mock-enforce", "exec", [EXEC, EXEC]),
    Phase("events-assert", "exec", [EXEC, EXEC, VERSION]),
    Phase("stream-assert", "exec", [EXEC, EXEC, VERSION]),
    Phase("reasoning-enforce", "exec", [EXEC, EXEC, VERSION, VERSION]),
]
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
# Several launches of every kind, for the detail's per-kind figures.
KINDS: Plan = [
    Phase("run", "exec", [EXEC, RESUME, EXEC]),
    Phase("controlled-turn", "app-server", [APP_SERVER]),
    Phase("usage-probe", "app-server", [APP_SERVER, APP_SERVER]),
    Phase("shared-server", "shared-app-server", [SHARED, SHARED]),
    Phase("rules", None, [VERSION, FEATURES, EXECPOLICY]),
]


def record(tmp: Path, plan: Plan) -> list[Record]:
    """The lines the real writers produce for `plan`, every launch measured."""
    true = tmp / "true-double"
    true.write_text("#!/usr/bin/env bash\nexit 0\n")
    true.chmod(0o755)
    file = tmp / "recorded.jsonl"
    script = [
        'set -euo pipefail; source "$1"; export OH_E2E_SUITE=codex OH_E2E_TELEMETRY_FILE="$2"; : >"$2"'
    ]
    for phase in plan:
        script.append(f"export OH_E2E_PHASE={phase.name}")
        if phase.expects is not None:
            script.append(f"_oh_expect codex {phase.expects}")
        for argv in phase.launches:
            words = " ".join(f"'{w}'" for w in argv)
            script.append(
                f'bash "$3" --suite codex --harness codex --real "$4" --self "$5" --file "$2" -- {words} </dev/null'
            )
    run = subprocess.run(
        [
            BASH,
            "-c",
            "\n".join(script),
            "_",
            str(LIB),
            str(file),
            str(NETSHIM),
            str(true),
            str(tmp / "shim"),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        raise AssertionError(f"recording the fixture failed: {run.stderr}")
    return lines(file)


def with_bytes(records: list[Record], received: Callable[[Record], int]) -> list[Record]:
    """A copy whose measurement lines carry `received(start)` bytes (sent: a third)."""
    by_id = {s["launch_id"]: s for s in starts(records)}
    out = copy.deepcopy(records)
    for line in out:
        start = by_id.get(line.get("launch_id")) if "argv" not in line else None
        if start is not None:
            line["rx_bytes"] = received(start)
            line["tx_bytes"] = line["rx_bytes"] // 3
    return out


# llmlint: ignore-block[structural_pattern_matching] The tests run at the uv workspace's Python 3.9 floor, where `match` does not parse.
def plausible(start: Record) -> int:
    """Each launch's share of a plausible pass before adjust_total scales its
    turns to the total a case wants: the probe at 400,000 B, the controlled
    turn at 900,000 B."""
    if start["phase"] == "usage-probe" and start["kind"] == "app-server":
        return 400_000
    if start["kind"] == "app-server":
        return 900_000
    return 850_000 if start["kind"] == "exec" else 0


# llmlint: ignore-end[structural_pattern_matching]


def adjust_total(records: list[Record], total: int) -> list[Record]:
    """Spread the difference to `total` over the exec launches, the remainder
    on the first, so no one launch nears its own cap."""
    out = copy.deepcopy(records)
    now = sum(m["rx_bytes"] for m in out if "argv" not in m and "rx_bytes" in m)
    turns = [s["launch_id"] for s in starts(out) if s["kind"] == "exec"]
    share, remainder = divmod(total - now, len(turns))
    for line in out:
        if line.get("launch_id") in turns and "argv" not in line:
            line["rx_bytes"] += share + (remainder if line["launch_id"] == turns[0] else 0)
    return out


def set_rx(records: list[Record], launch_id: LaunchId, rx: int) -> list[Record]:
    out = copy.deepcopy(records)
    for line in out:
        if line.get("launch_id") == launch_id and "argv" not in line:
            line["rx_bytes"] = rx
    return out


def verdicts(output: str) -> dict[str, str]:
    """budget id -> within | over | error, from the check's text report."""
    return dict(re.findall(r"^budget ([a-z0-9-]+): .*?(within|over|error)", output, re.M))


# Each fixture is recorded once per run and copied for every case.
_RECORDED: dict[str, list[Record]] = {}


class BudgetCase(unittest.TestCase):
    def recorded(self, name: str, plan: Plan) -> list[Record]:
        need_recorder(self)
        if name not in _RECORDED:
            _RECORDED[name] = record(scratch(self), plan)
        return copy.deepcopy(_RECORDED[name])

    def check(self, records: list[Record]) -> subprocess.CompletedProcess[str]:
        tmp = scratch(self)
        write_lines(tmp / "codex.jsonl", records)
        return check_step(tmp)


_CAPS: dict[str, int] = {}


def caps() -> dict[str, int]:
    """Each budget's threshold, as budgets.yaml registers it (read once)."""
    if not _CAPS:
        listed = subprocess.run(
            [
                "uv",
                "run",
                "--project",
                str(ROOT / "python"),
                "--frozen",
                "--no-sync",
                "onebudgetspec",
                "list",
                "--json",
                str(BUDGETS),
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        _CAPS.update({b["id"]: int(b["threshold"]) for b in json.loads(listed.stdout)["budgets"]})
    return _CAPS


class Thresholds(BudgetCase):
    """Each cap at its exact value, read from budgets.yaml, every other budget
    kept within its own so the one tested is isolated."""

    def base(self, total: int) -> list[Record]:
        records = with_bytes(self.recorded("workload", WORKLOAD), plausible)
        return adjust_total(records, total)

    @property
    def launch_cap(self) -> int:
        return caps()["codex-launch-network-bytes"]

    @property
    def suite_cap(self) -> int:
        return caps()["codex-live-suite-network-bytes"]

    def test_the_fixture_is_the_budget_workload_completely_recorded(self) -> None:
        records = self.recorded("workload", WORKLOAD)
        launches = starts(records)
        self.assertEqual(len(launches), 50)
        kinds = [s["kind"] for s in launches]
        self.assertEqual(kinds.count("exec"), 22)
        self.assertEqual(kinds.count("local"), 26)
        self.assertEqual(
            [s["phase"] for s in launches if s["kind"] == "app-server"],
            ["controlled-turn", "usage-probe"],
        )
        for start in launches:
            (line,) = measurements(records, start["launch_id"])
            self.assertEqual(line["method"], "netns-pasta")

    def test_the_largest_launch_passes_at_the_cap_and_fails_one_byte_over_naming_it(self) -> None:
        self.assertEqual(self.launch_cap, 2_500_000)
        base = self.base(self.suite_cap // 2)
        target = [s for s in starts(base) if s["kind"] == "exec"][5]["launch_id"]
        run = self.check(set_rx(base, target, 2_500_000))
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertEqual(set(verdicts(run.stdout).values()), {"within"}, run.stdout)
        run = self.check(set_rx(base, target, 2_500_001))
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(
            verdicts(run.stdout),
            {
                "codex-launch-network-bytes": "over",
                "codex-usage-probe-network-bytes": "within",
                "codex-live-suite-network-bytes": "within",
            },
            run.stdout,
        )
        self.assertIn(f"largest: launch {target}", run.stdout)
        self.assertIn("received 2500001 B", run.stdout)

    def test_the_suite_total_passes_at_its_cap_and_fails_one_byte_over_with_its_breakdown(
        self,
    ) -> None:
        base = self.base(self.suite_cap // 2)
        for total, verdict in (
            (self.suite_cap, "within"),
            (self.suite_cap + 1, "over"),
            (self.suite_cap * 4 // 5, "within"),
        ):
            with self.subTest(total=total):
                run = self.check(adjust_total(base, total))
                got = verdicts(run.stdout)
                self.assertEqual(got["codex-live-suite-network-bytes"], verdict, run.stdout)
                self.assertEqual(got["codex-launch-network-bytes"], "within", run.stdout)
                self.assertEqual(got["codex-usage-probe-network-bytes"], "within", run.stdout)
                self.assertEqual(run.returncode == 0, verdict == "within", run.stdout + run.stderr)
                self.assertIn(f"suite total: {total} B received over 50 launches", run.stdout)
                self.assertRegex(
                    run.stdout, r"exec: 22 launches, total received \d+ B, total sent \d+ B"
                )
                self.assertRegex(run.stdout, r"local: 26 launches, total received 0 B")
                self.assertIn(
                    f"suite cap: planned 30000000 B, recalibrated to {self.suite_cap} B",
                    run.stdout,
                )
                self.assertIn(f"launch cap: {self.launch_cap} B kept", run.stdout)

    def test_doubling_a_plausible_pass_fails_the_suite_total_alone(self) -> None:
        # A pass at 80% of the cap — the cap is the measured pass x1.2,
        # rounded up — doubled across the board.
        plausible_total = self.suite_cap * 4 // 5
        base = self.base(plausible_total)
        by_id = {m["launch_id"]: m["rx_bytes"] for m in base if "rx_bytes" in m}
        doubled = with_bytes(base, lambda s: 2 * by_id[s["launch_id"]])
        run = self.check(doubled)
        self.assertNotEqual(run.returncode, 0)
        got = verdicts(run.stdout)
        self.assertEqual(got["codex-live-suite-network-bytes"], "over", run.stdout)
        self.assertEqual(got["codex-launch-network-bytes"], "within", run.stdout)
        self.assertEqual(got["codex-usage-probe-network-bytes"], "within", run.stdout)
        self.assertIn(f"suite total: {2 * plausible_total} B received over 50 launches", run.stdout)
        self.assertRegex(run.stdout, r"app-server: 2 launches, total received 2600000 B")

    def test_the_usage_probe_is_held_to_under_one_megabyte(self) -> None:
        self.assertEqual(caps()["codex-usage-probe-network-bytes"], 999_999)
        base = self.base(self.suite_cap // 2)
        probe = next(
            s["launch_id"]
            for s in starts(base)
            if s["phase"] == "usage-probe" and s["kind"] == "app-server"
        )
        for rx, verdict in (
            (1_200_000, "over"),
            (1_000_000, "over"),
            (999_999, "within"),
            (600_000, "within"),
        ):
            with self.subTest(rx=rx):
                run = self.check(set_rx(base, probe, rx))
                got = verdicts(run.stdout)
                self.assertEqual(got["codex-usage-probe-network-bytes"], verdict, run.stdout)
                # 1,200,000 B is under the per-launch cap, which still passes.
                self.assertEqual(got["codex-launch-network-bytes"], "within", run.stdout)
                self.assertEqual(got["codex-live-suite-network-bytes"], "within", run.stdout)
                if verdict == "over":
                    self.assertIn(
                        f"largest: launch {probe} (phase usage-probe, kind app-server", run.stdout
                    )

    def test_a_run_with_no_measured_usage_probe_fails_naming_the_phase(self) -> None:
        base = self.base(self.suite_cap // 2)
        probe = next(
            s["launch_id"]
            for s in starts(base)
            if s["phase"] == "usage-probe" and s["kind"] == "app-server"
        )
        without = [
            r
            for r in base
            if r.get("launch_id") != probe
            and not (r.get("expected") and r["phase"] == "usage-probe")
        ]
        run = self.check(without)
        self.assertNotEqual(run.returncode, 0)
        got = verdicts(run.stdout)
        self.assertEqual(got["codex-usage-probe-network-bytes"], "error", run.stdout)
        self.assertEqual(got["codex-launch-network-bytes"], "within", run.stdout)
        self.assertIn("phase usage-probe has no measured app-server launch", run.stdout)


class Detail(BudgetCase):
    def figures(self) -> list[Record]:
        sizes = iter(range(1, 100))
        return with_bytes(
            self.recorded("kinds", KINDS), lambda s: 10_000 * next(sizes) + len(s["kind"])
        )

    def test_each_mode_reports_its_figure_and_the_per_kind_detail_the_lines_hold(self) -> None:
        records = self.figures()
        tmp = scratch(self)
        write_lines(tmp / "codex.jsonl", records)
        launches = {s["launch_id"]: s for s in starts(records)}
        rx = {
            m["launch_id"]: (m["rx_bytes"], m["tx_bytes"])
            for m in records
            if "argv" not in m and "launch_id" in m
        }
        top = max(rx, key=lambda i: rx[i][0])
        run = budget(telemetry_dir=tmp)
        self.assertEqual(run.returncode, 0, run.stderr)
        value, *detail = run.stdout.splitlines()
        self.assertEqual(value, f"{rx[top][0]} B")
        self.assertIn(
            f"largest: launch {top} (phase {launches[top]['phase']}, kind {launches[top]['kind']}",
            detail[0],
        )
        for kind in ("app-server", "exec", "local", "shared-app-server"):
            ids = [i for i, s in launches.items() if s["kind"] == kind]
            want = (
                f"{kind}: {len(ids)} launch{'es' if len(ids) != 1 else ''}, "
                f"largest received {max(rx[i][0] for i in ids)} B, largest sent {max(rx[i][1] for i in ids)} B, "
                f"total received {sum(rx[i][0] for i in ids)} B, total sent {sum(rx[i][1] for i in ids)} B"
            )
            self.assertIn(want, detail)
        probes = [
            i
            for i, s in launches.items()
            if s["phase"] == "usage-probe" and s["kind"] == "app-server"
        ]
        run = budget("--kind", "usage-probe", telemetry_dir=tmp)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout.splitlines()[0], f"{max(rx[i][0] for i in probes)} B")
        self.assertIn("usage probes: 2", run.stdout)
        for probe in probes:
            self.assertIn(f"{probe}: received {rx[probe][0]} B, sent {rx[probe][1]} B", run.stdout)
        run = budget("--total", telemetry_dir=tmp)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout.splitlines()[0], f"{sum(v[0] for v in rx.values())} B")

    def test_every_mode_refuses_an_incomplete_record_naming_what_is_at_fault(self) -> None:
        base = self.figures()
        first = starts(base)[0]["launch_id"]

        def measurement_of(records: list[Record], launch_id: LaunchId) -> Record:
            return next(r for r in records if r.get("launch_id") == launch_id and "argv" not in r)

        def mutate(change) -> list[Record]:
            out = copy.deepcopy(base)
            change(out)
            return out

        cases = {
            "a duplicated launch id": (
                mutate(
                    lambda r: r.insert(
                        1, next(s for s in r if s.get("launch_id") == first and "argv" in s)
                    )
                ),
                f"launch {first} has two start lines",
            ),
            "negative received bytes": (
                mutate(lambda r: measurement_of(r, first).update(rx_bytes=-1)),
                "no non-negative integer rx_bytes",
            ),
            "nonnumeric sent bytes": (
                mutate(lambda r: measurement_of(r, first).update(tx_bytes="12")),
                "no non-negative integer tx_bytes",
            ),
            "a start with no measurement line": (
                mutate(lambda r: r.remove(measurement_of(r, first))),
                f"launch {first} (phase run, kind exec) has no measurement line",
            ),
            "a start with two measurement lines": (
                mutate(lambda r: r.append(dict(measurement_of(r, first)))),
                f"launch {first} (phase run, kind exec) has 2 measurement lines",
            ),
            "an unavailable measurement": (
                mutate(
                    lambda r: r.__setitem__(
                        r.index(measurement_of(r, first)),
                        {
                            "launch_id": first,
                            "method": "unavailable",
                            "reason": "no pasta on PATH",
                            "seconds": 1,
                            "exit": 0,
                        },
                    )
                ),
                f"launch {first} (phase run, kind exec) was not measured (method 'unavailable': no pasta on PATH)",
            ),
            "a received-less measurement": (
                mutate(lambda r: measurement_of(r, first).pop("rx_bytes")),
                "no non-negative integer rx_bytes",
            ),
            "a sent-less measurement": (
                mutate(lambda r: measurement_of(r, first).pop("tx_bytes")),
                "no non-negative integer tx_bytes",
            ),
            "a measurement with no start": (
                mutate(
                    lambda r: r.append({**measurement_of(r, first), "launch_id": "codex-orphan"})
                ),
                "names launch codex-orphan, which has no start line",
            ),
            "a boolean received count": (
                mutate(lambda r: measurement_of(r, first).update(rx_bytes=True)),
                "no non-negative integer rx_bytes",
            ),
            "a fractional sent count": (
                mutate(lambda r: measurement_of(r, first).update(tx_bytes=12.5)),
                "no non-negative integer tx_bytes",
            ),
            "a start of another suite": (
                mutate(
                    lambda r: next(
                        x for x in r if x.get("launch_id") == first and "argv" in x
                    ).update(suite="claude")
                ),
                f"(launch {first}) names suite 'claude', not 'codex'",
            ),
            "a start of another harness": (
                mutate(
                    lambda r: next(
                        x for x in r if x.get("launch_id") == first and "argv" in x
                    ).update(harness="claude-code")
                ),
                f"(launch {first}) names harness 'claude-code', not 'codex'",
            ),
            "a start with no phase": (
                mutate(
                    lambda r: next(x for x in r if x.get("launch_id") == first and "argv" in x).pop(
                        "phase"
                    )
                ),
                f"(launch {first}) has no phase string",
            ),
            "a start whose argv is not strings": (
                mutate(
                    lambda r: next(
                        x for x in r if x.get("launch_id") == first and "argv" in x
                    ).update(argv=[1, 2])
                ),
                f"(launch {first}) has an argv that is not a list of strings",
            ),
            "an expectation of another suite": (
                mutate(
                    lambda r: r.append(
                        {"suite": "claude", "phase": "run", "kind": "exec", "expected": True}
                    )
                ),
                "expects a launch of suite 'claude', not 'codex'",
            ),
            "an expected phase with no measured launch": (
                mutate(
                    lambda r: r.append(
                        {"suite": "codex", "phase": "ghost", "kind": "exec", "expected": True}
                    )
                ),
                "phase ghost expected a measured exec launch and has none",
            ),
        }
        for mode in ([], ["--kind", "usage-probe"], ["--total"]):
            tmp = scratch(self)
            run = budget(*mode, telemetry_dir=tmp)
            with self.subTest(mode=mode, case="a missing file"):
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(f"no telemetry at {tmp / 'codex.jsonl'}", run.stderr)
            for case, (records, named) in cases.items():
                with self.subTest(mode=mode, case=case):
                    write_lines(tmp / "codex.jsonl", records)
                    run = budget(*mode, telemetry_dir=tmp)
                    self.assertNotEqual(run.returncode, 0, run.stdout)
                    self.assertIn(named, run.stderr)
            for case, text, named in (
                ("a line that is not JSON", "{not json\n", "line 1 is not JSON"),
                ("a line that is not an object", "[1, 2]\n", "line 1 is not a JSON object"),
                ("an empty record", "\n\n", "records no launch at all"),
            ):
                with self.subTest(mode=mode, case=case):
                    (tmp / "codex.jsonl").write_text(text)
                    run = budget(*mode, telemetry_dir=tmp)
                    self.assertNotEqual(run.returncode, 0, run.stdout)
                    self.assertIn(named, run.stderr)


class SmallCannotCarryLarge(BudgetCase):
    """One measured small launch never stands in for an unmeasured large one."""

    def test_the_check_fails_whenever_the_catalog_sized_launch_is_not_completely_measured(
        self,
    ) -> None:
        need_recorder(self)
        tmp = scratch(self)
        serving = server(self)
        file = tmp / "codex.jsonl"
        script = tmp / "fetch-double"
        script.write_text(
            "#!/usr/bin/env bash\n"
            f'exec python3 "{ROOT}/tools/network-telemetry/transfer.py" client {serving.port} "$OH_TEST_DOWN" 0\n'
        )
        script.chmod(0o755)
        plan = [
            ("run", "exec", 3_000, EXEC),
            ("catalog", "exec", 1_500_000, EXEC),
            ("usage-probe", "app-server", 3_000, APP_SERVER),
        ]
        for phase, kind, down, argv in plan:
            subprocess.run(
                [
                    BASH,
                    "-c",
                    'source "$1"; export OH_E2E_SUITE=codex OH_E2E_TELEMETRY_FILE="$2" OH_E2E_PHASE="$3"; _oh_expect codex "$4"',
                    "_",
                    str(LIB),
                    str(file),
                    phase,
                    kind,
                ],
                check=True,
            )
            subprocess.run(
                [
                    BASH,
                    str(NETSHIM),
                    "--suite",
                    "codex",
                    "--harness",
                    "codex",
                    "--real",
                    str(script),
                    "--self",
                    str(tmp / "shim"),
                    "--file",
                    str(file),
                    "--",
                    *argv,
                ],
                env={**os.environ, "OH_E2E_PHASE": phase, "OH_TEST_DOWN": str(down)},
                stdin=subprocess.DEVNULL,
                capture_output=True,
                check=True,
            )
        records = lines(file)
        small, large, _probe = (s["launch_id"] for s in starts(records))
        (small_line,) = measurements(records, small)
        (large_line,) = measurements(records, large)
        self.assertGreater(small_line["rx_bytes"], 3_000)
        self.assertLess(small_line["rx_bytes"], 20_000)
        self.assertGreater(large_line["rx_bytes"], 1_500_000)
        run = self.check(records)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn(f"largest: launch {large}", run.stdout)
        without = [r for r in records if r is not large_line]
        cases = {
            "no measurement line": without,
            "an unavailable line": [
                *without,
                {
                    "launch_id": large,
                    "method": "unavailable",
                    "reason": "no pasta on PATH",
                    "seconds": 2,
                    "exit": 0,
                },
            ],
            "an incomplete line": [
                *without,
                {k: v for k, v in large_line.items() if k != "rx_bytes"},
            ],
            "a duplicate of the small launch's line": [*without, dict(small_line)],
        }
        for case, mutated in cases.items():
            with self.subTest(case=case):
                run = self.check(mutated)
                self.assertNotEqual(run.returncode, 0, run.stdout)
                self.assertEqual(set(verdicts(run.stdout).values()), {"error"}, run.stdout)
                if case.startswith("a duplicate"):
                    self.assertIn(
                        f"launch {small} (phase run, kind exec) has 2 measurement lines", run.stdout
                    )
                self.assertIn(f"launch {large} (phase catalog, kind exec)", run.stdout)


class CodexSuite(BudgetCase):
    """scripts/e2e-codex.sh itself, `codex` the mock codex harness behind the shim."""

    def run_codex(self, tmp: Path, **extra: str) -> subprocess.CompletedProcess[str]:
        need_builds(self)
        return run_suite("codex", suite_env(tmp, "codex", OH_E2E_PHASE_LIMIT="3", **extra))

    def test_its_three_budget_phases_each_record_an_expectation_a_start_and_one_measurement(
        self,
    ) -> None:
        need_recorder(self)
        tmp = scratch(self)
        run = self.run_codex(tmp)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("STOP: OH_E2E_PHASE_LIMIT=3 reached", run.stderr)
        records = lines(tmp / "telemetry" / "codex.jsonl")
        for phase, kind in (
            ("run", "exec"),
            ("controlled-turn", "app-server"),
            ("usage-probe", "app-server"),
        ):
            with self.subTest(phase=phase):
                self.assertIn(
                    {"suite": "codex", "phase": phase, "kind": kind, "expected": True}, records
                )
                (launch,) = [
                    s for s in starts(records) if s["phase"] == phase and s["kind"] == kind
                ]
                self.assertEqual((launch["suite"], launch["harness"]), ("codex", "codex"))
                (line,) = measurements(records, launch["launch_id"])
                self.assertEqual(line["method"], "netns-pasta")
        run = check_step(tmp / "telemetry")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_the_usage_probe_phase_carries_its_launchs_own_download(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        serving = server(self)
        run = self.run_codex(
            tmp, OH_TEST_TRANSFER=f"{serving.port} 800000 0", OH_TEST_TRANSFER_PHASE="usage-probe"
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        records = lines(tmp / "telemetry" / "codex.jsonl")
        (probe,) = [
            s for s in starts(records) if s["phase"] == "usage-probe" and s["kind"] == "app-server"
        ]
        (line,) = measurements(records, probe["launch_id"])
        self.assertGreater(line["rx_bytes"], 800_000)
        for other in starts(records):
            if other is not probe:
                (quiet,) = measurements(records, other["launch_id"])
                self.assertLess(quiet["rx_bytes"], 100_000, other)
        selected = budget("--kind", "usage-probe", telemetry_dir=tmp / "telemetry")
        self.assertEqual(selected.returncode, 0, selected.stderr)
        self.assertEqual(selected.stdout.splitlines()[0], f"{line['rx_bytes']} B")
        self.assertIn(
            f"largest: launch {probe['launch_id']} (phase usage-probe, kind app-server",
            selected.stdout,
        )

    def test_a_phase_whose_launch_the_shim_could_not_measure_fails_the_check_step(self) -> None:
        need_recorder(self)
        tmp = scratch(self)
        # A pasta that cannot build the namespace for the controlled turn only:
        # that launch runs unmeasured, as on a host whose namespaces fail.
        broken = tmp / "pasta-bin"
        broken.mkdir()
        real = subprocess.run(
            ["bash", "-c", "command -v pasta"], capture_output=True, text=True
        ).stdout.strip()
        (broken / "pasta").write_text(
            '#!/usr/bin/env bash\n[ "${OH_E2E_PHASE:-}" != controlled-turn ] || { echo "pasta: refused" >&2; exit 1; }\n'
            f'exec "{real}" "$@"\n'
        )
        (broken / "pasta").chmod(0o755)
        env = suite_env(tmp, "codex", OH_E2E_PHASE_LIMIT="3")
        env["PATH"] = (
            f"{env['PATH'].split(os.pathsep)[0]}{os.pathsep}{broken}{os.pathsep}{env['PATH']}"
        )
        run = run_suite("codex", env)
        self.assertEqual(run.returncode, 0, run.stderr)
        records = lines(tmp / "telemetry" / "codex.jsonl")
        (turn,) = [
            s
            for s in starts(records)
            if s["phase"] == "controlled-turn" and s["kind"] == "app-server"
        ]
        (line,) = measurements(records, turn["launch_id"])
        self.assertEqual(line["method"], "unavailable")
        run = check_step(tmp / "telemetry")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn(
            f"launch {turn['launch_id']} (phase controlled-turn, kind app-server) was not measured",
            run.stdout,
        )

    def test_the_suite_fails_when_codex_on_path_is_not_its_shim(self) -> None:
        tmp = scratch(self)
        # A stale shim directory the setup cannot rewrite, holding a foreign
        # `codex` where the shim goes.
        stale = tmp / "telemetry" / ".shims" / "codex"
        stale.mkdir(parents=True)
        (stale / ".oh-network-shims").write_text("")
        (stale / "codex").write_text("#!/usr/bin/env bash\necho 'not a shim'\n")
        (stale / "codex").chmod(0o755)
        stale.chmod(0o555)
        self.addCleanup(stale.chmod, 0o755)
        run = self.run_codex(tmp)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn(
            f"PATH resolves codex to '{stale / 'codex'}', not this suite's measuring shim",
            run.stderr,
        )


class Evidence(unittest.TestCase):
    """The committed finished-tree pass: its telemetry is a complete record
    under the recorder-to-command contract, and its report is exactly what the
    committed network_budget.py computes from it."""

    EVIDENCE = ROOT / "live/pr-matrix/harness/codex/evidence"

    def test_the_committed_report_is_what_the_committed_telemetry_yields(self) -> None:
        telemetry, report = (
            self.EVIDENCE / "live-codex.jsonl",
            self.EVIDENCE / "live-codex-report.json",
        )
        self.assertTrue(
            telemetry.is_file() and report.is_file(), "the live-codex pass's evidence is missing"
        )
        if WINDOWS:
            self.skipTest("recomputed on the unix legs; the budgets are Linux's")
        recomputed = budget(
            "--evidence-report", "--file", str(telemetry), telemetry_dir=self.EVIDENCE
        )
        self.assertEqual(recomputed.returncode, 0, recomputed.stderr)
        committed = json.loads(report.read_text())
        self.assertEqual(json.loads(recomputed.stdout), committed)
        records = lines(telemetry)
        received = {m["launch_id"]: m["rx_bytes"] for m in records if "rx_bytes" in m}
        self.assertEqual(committed["launches"], len(starts(records)))
        self.assertEqual(committed["largest_launch"]["received"], max(received.values()))
        self.assertEqual(committed["suite_total_received"], sum(received.values()))
        calibration = committed["calibration"]
        # The caps the report states its conditions against are budgets.yaml's.
        listed = subprocess.run(
            [
                "uv",
                "run",
                "--project",
                str(ROOT / "python"),
                "--frozen",
                "--no-sync",
                "onebudgetspec",
                "list",
                "--json",
                str(BUDGETS),
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        thresholds = {b["id"]: b["threshold"] for b in json.loads(listed.stdout)["budgets"]}
        self.assertEqual(
            calibration["largest_x1_5_within_launch_cap"]["cap"],
            thresholds["codex-launch-network-bytes"],
        )
        self.assertEqual(
            calibration["suite_total_x1_2_within_suite_cap"]["cap"],
            thresholds["codex-live-suite-network-bytes"],
        )
        self.assertEqual(
            calibration["doubled_pass_exceeds_suite_cap"]["cap"],
            thresholds["codex-live-suite-network-bytes"],
        )
        launch_cap = int(thresholds["codex-launch-network-bytes"])
        suite_cap = int(thresholds["codex-live-suite-network-bytes"])
        total = sum(received.values())
        self.assertEqual(
            calibration["largest_x1_5_within_launch_cap"]["holds"],
            max(received.values()) * 1.5 <= launch_cap,
        )
        self.assertEqual(
            calibration["suite_total_x1_2_within_suite_cap"]["holds"], total * 1.2 <= suite_cap
        )
        self.assertEqual(
            calibration["doubled_pass_exceeds_suite_cap"]["holds"], total * 2 > suite_cap
        )
        # The suite cap is this evidence's total x1.2, rounded up to the next
        # 0.5 MB: re-measuring onto another step means moving the cap with it.
        self.assertEqual(suite_cap, math.ceil(total * 1.2 / 500_000) * 500_000)
        largest = max(received.values())
        self.assertEqual(
            committed["caps"],
            {
                "planned_suite_cap": 30_000_000,
                "suite_cap": suite_cap,
                "suite_total_received": total,
                "suite_total_x1_2": total * 1.2,
                "suite_total_x1_2_rounded_up": suite_cap,
                "recalibration_matches_suite_cap": True,
                "launch_cap": launch_cap,
                "largest_received": largest,
                "largest_x1_5": largest * 1.5,
                "launch_cap_kept_above_largest_x1_5": largest * 1.5 <= launch_cap,
                "statement": [
                    f"suite cap: planned 30000000 B, recalibrated to {suite_cap} B = the committed "
                    f"pass's suite total {total} B x1.2 = {total * 1.2:.1f} B, rounded up to the "
                    f"next 500000 B: {suite_cap} B",
                    f"launch cap: {launch_cap} B kept, beside the committed pass's largest launch "
                    f"{largest} B x1.5 = {largest * 1.5:.1f} B",
                ],
            },
        )
        description = " ".join(BUDGETS.read_text().split())
        for figure in (
            "30,000,000 B",
            f"{total:,} B x1.2 = {total * 1.2:,.0f} B",
            f"{suite_cap:,} B",
            f"{largest:,} B x1.5 = {largest * 3 // 2:,} B",
            f"{launch_cap:,} B",
        ):
            self.assertIn(figure, description)


class CapBasisOfAnotherPass(BudgetCase):
    def test_a_pass_the_caps_were_not_set_from_reports_both_conditions_false(self) -> None:
        records = with_bytes(
            self.recorded("kinds", KINDS),
            lambda s: 2_000_000 if s["kind"] == "exec" else 0,
        )
        tmp = scratch(self)
        file = tmp / "pass.jsonl"
        write_lines(file, records)
        run = budget("--evidence-report", "--file", str(file), telemetry_dir=tmp)
        self.assertEqual(run.returncode, 0, run.stderr)
        got = json.loads(run.stdout)["caps"]
        self.assertEqual(got["suite_total_received"], 6_000_000)
        self.assertEqual(got["suite_total_x1_2_rounded_up"], 7_500_000)
        self.assertFalse(got["recalibration_matches_suite_cap"])
        self.assertEqual(got["largest_x1_5"], 3_000_000)
        self.assertFalse(got["launch_cap_kept_above_largest_x1_5"])
        self.assertIn(
            "pass's suite total 6000000 B x1.2 = 7200000.0 B, rounded up to the next 500000 B: "
            "7500000 B",
            got["statement"][0],
        )


class OneVersion(unittest.TestCase):
    def test_the_budget_command_and_the_check_pin_one_onebudgetspec(self) -> None:
        header = re.search(
            r'onebudgetspec-sdk==([0-9.]+)"',
            (ROOT / "live/pr-matrix/harness/codex/network_budget.py").read_text(),
        )
        lock = re.search(
            r'name = "onebudgetspec-cli"\nversion = "([0-9.]+)"',
            (ROOT / "python/uv.lock").read_text(),
        )
        assert header is not None and lock is not None
        self.assertEqual(header.group(1), lock.group(1))


if __name__ == "__main__":
    unittest.main()
