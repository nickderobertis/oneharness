#!/usr/bin/env bash
# Drift gate: scripts/explore_control.py's capability tables vs. the registry.
#
# The probe restates the harness->mechanism contract in `PROBES` and the
# no-control-surface set in `NO_SURFACE`, because it stands each path up itself
# and cannot read a mechanism it has not implemented. That duplication is the
# hazard: the probe is what keeps the control matrix from decaying into stale
# documentation, so a probe that has itself gone stale — a newly declared
# harness it never exercises, or a mechanism id renamed under it — retires the
# alarm silently and the matrix rots exactly where nobody is looking.
#
# `oneharness list` publishes each harness's declared `control`, so the registry
# stays the single source and this check fails when the tables disagree with it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe="$root/scripts/explore_control.py"

bin="${ONEHARNESS_BIN:-$root/target/debug/oneharness}"
if [ ! -x "$bin" ]; then
    echo "check-control-probes: skipped ($bin is not built; run 'just build')"
    exit 0
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "check-control-probes: skipped (python3 is not installed; the probe it checks needs it)"
    exit 0
fi

ONEHARNESS_NO_CONFIG=1 "$bin" list >"${TMPDIR:-/tmp}/oh-control-registry.$$.json"
trap 'rm -f "${TMPDIR:-/tmp}/oh-control-registry.$$.json"' EXIT

# Compare in Python: it can import the probe's own tables rather than re-parsing
# them out of the source, so the check reads the values the probe actually runs.
python3 - "$probe" "${TMPDIR:-/tmp}/oh-control-registry.$$.json" <<'PY'
import importlib.util
import json
import sys

probe_path, registry_path = sys.argv[1], sys.argv[2]

spec = importlib.util.spec_from_file_location("explore_control", probe_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

with open(registry_path, encoding="utf-8") as handle:
    listing = json.load(handle)
harnesses = listing["harnesses"] if isinstance(listing, dict) else listing

declared = {h["id"]: h.get("control") for h in harnesses}
probed = {hid: mechanism for hid, (_bin, mechanism, _fn) in module.PROBES.items()}
no_surface = set(module.NO_SURFACE)

problems = []

for hid, mechanism in sorted(declared.items()):
    if mechanism is None:
        if hid in probed:
            problems.append(
                f"`{hid}` declares no control mechanism, but explore_control.py PROBES still "
                f"probes it as `{probed[hid]}`. Drop it from PROBES, or add it to NO_SURFACE "
                f"if it was probed and found to have no headless surface."
            )
        elif hid not in no_surface:
            problems.append(
                f"`{hid}` declares no control mechanism and explore_control.py accounts for it "
                f"nowhere. Add it to NO_SURFACE with the reason the probe found no surface."
            )
        continue
    if hid in no_surface:
        problems.append(
            f"`{hid}` declares control `{mechanism}`, but explore_control.py lists it in "
            f"NO_SURFACE as having none. Move it to PROBES with a probe for that mechanism."
        )
    elif hid not in probed:
        problems.append(
            f"`{hid}` declares control `{mechanism}`, but explore_control.py has no probe for "
            f"it. Add a PROBES entry so the drift alarm covers it."
        )
    elif probed[hid] != mechanism:
        problems.append(
            f"`{hid}` declares control `{mechanism}`, but explore_control.py PROBES calls it "
            f"`{probed[hid]}`. Use the registry's spelling."
        )

for hid in sorted(set(probed) | no_surface):
    if hid not in declared:
        problems.append(
            f"explore_control.py mentions `{hid}`, which is not a harness in the registry. "
            f"Remove it, or use the registry's id."
        )

if problems:
    print("check-control-probes: explore_control.py has drifted from the registry:", file=sys.stderr)
    for problem in problems:
        print(f"  - {problem}", file=sys.stderr)
    print(
        "  The registry (crates/oneharness-core/src/domain/harness.rs) is the source; "
        "`oneharness list` publishes it.",
        file=sys.stderr,
    )
    sys.exit(1)
PY

echo "check-control-probes: ok"
