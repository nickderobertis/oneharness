#!/usr/bin/env python3
"""The gate never checks the network budgets against real telemetry.

    check_gate_selection.py

Run by the network-telemetry project's `lint`, apart from its measuring tests:
it reads every project definition and the justfile, so it is selected by any
project's change, and costs a second. It holds that the only target anywhere
that runs the budget check (`scripts/e2e-network-budget.sh`, `onebudgetspec
check`, or `just live-codex`, which runs both) is the paid `live-codex:live`,
which no gate tier runs (`just check` excludes `live-*`), and that the recipe
runs the suite and then the check. Quiet on success; each violation is named.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RECIPE = [
    'ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-codex.sh',
    "bash scripts/e2e-network-budget.sh codex",
]


def violations() -> list[str]:
    found = []
    for project in sorted(ROOT.glob("**/project.json")):
        if {"node_modules", "target", ".nx"} & set(project.parts):
            continue
        definition = json.loads(project.read_text())
        for target, spec in definition.get("targets", {}).items():
            options = spec.get("options", {})
            commands = json.dumps(options.get("commands", options.get("command", "")))
            runs_check = any(
                word in commands
                for word in ("e2e-network-budget", "onebudgetspec check", "live-codex")
            )
            if runs_check and (definition.get("name"), target) != ("live-codex", "live"):
                found.append(
                    f"{project.relative_to(ROOT)}: target {target} runs the network budget check"
                )
    justfile = (ROOT / "justfile").read_text()
    check = re.search(r"^check tier=.*?(?=^\S)", justfile, re.M | re.S)
    if check is None or "--exclude='live-*,explore-*'" not in check.group(0):
        found.append(
            "justfile: `check` no longer excludes the live-* projects, so the gate would run live-codex"
        )
    recipe = re.search(r"^live-codex:.*?(?=^\S|\Z)", justfile, re.M | re.S)
    lines = (
        [line.strip() for line in recipe.group(0).splitlines()[1:] if line.strip()]
        if recipe
        else []
    )
    if lines != RECIPE:
        found.append(
            f"justfile: the live-codex recipe is {lines}, not the suite then its budget check {RECIPE}"
        )
    return found


def main() -> int:
    found = violations()
    for violation in found:
        print(f"check-gate-selection: {violation}", file=sys.stderr)
    if found:
        print(
            "check-gate-selection: the network budgets read live telemetry, which only `just live-codex` "
            "records; keep their check there (live/AGENTS.md)",
            file=sys.stderr,
        )
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
