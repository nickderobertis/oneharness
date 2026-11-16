#!/usr/bin/env python3
"""A scripted `codex` and `oneharness` for test_run_overlay_workload.py,
standing in for both behind live/control/run_allowlist_live.py's budget
workload, so its writer runs for real against its own scripted model endpoint.

Installed as `codex` it answers `--version` and the world's warm-up `exec`
(one model request, then exit 0). Installed as `oneharness`, a `run` reads
the endpoint from $CODEX_HOME/config.toml, sends the turn's prompt as one
Responses request, runs the shell call it is answered with (granted turns
only, as a read-only sandbox refuses it ungranted), returns its output as the
call's result, and prints a report: `ok`, with overlay timings when the run's
config carries a top-level `allowed_tools`.

BUDGET_DOUBLE_FAULT bends one side away from that turn:
  exit           a granted turn exits 1 having reported ok
  status         a granted turn reports `nonzero`
  no-decision    a granted turn never returns its call's result
  not-run        a granted turn never runs the granted command
  no-overlay     a granted turn reports no overlay
  ungranted-ran  an ungranted turn runs the granted command

Kept to the Python 3.9 floor of the suite that runs it.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path
from typing import NoReturn, Optional


def fail(message: str) -> NoReturn:
    print(
        f"budget_turn_double: {message}; check the test's world, then rerun test_run_overlay_workload.py",
        file=sys.stderr,
    )
    sys.exit(2)


def endpoint() -> str:
    home = os.environ.get("CODEX_HOME") or fail("CODEX_HOME is unset")
    found = re.search(r'base_url = "([^"]+)"', Path(str(home), "config.toml").read_text())
    if found is None:
        fail("no base_url in the Codex home's config.toml")
    return str(found.group(1))


# llmlint: ignore-block[async_typed_clients_at_boundaries] This double stands in for a CLI talking to the test's own loopback model endpoint: a turn's two requests are sequential by protocol (the second carries the first answer's call), so there is nothing to overlap, and it runs on whatever bare python3 is on PATH, outside the uv environment, so a typed async client is a dependency it cannot have. Each streamed event is checked for shape before it is used.
def respond(items: list[dict[str, object]]) -> list[dict[str, object]]:
    """The output items the endpoint streams back for one request."""
    body = {
        "model": "m",
        "stream": True,
        "input": items,
        "tools": [{"type": "function", "name": "exec_command"}],
    }
    request = urllib.request.Request(  # noqa: S310 — the test's own loopback endpoint.
        endpoint() + "/responses",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=60) as answer:  # noqa: S310
        events = [
            json.loads(line[6:])
            for line in answer.read().decode().splitlines()
            if line.startswith("data: ")
        ]
    # Only a finished output item that is an object counts; anything else in
    # the stream is not a call this double could act on.
    return [
        e["item"]
        for e in events
        if isinstance(e, dict)
        and e.get("type") == "response.output_item.done"
        and isinstance(e.get("item"), dict)
    ]


# llmlint: ignore-end[async_typed_clients_at_boundaries]


def command_of(arguments: object) -> Optional[str]:
    """The `cmd` of an exec_command call's JSON arguments, else None."""
    try:
        decoded = json.loads(arguments) if isinstance(arguments, str) else None
    except ValueError:
        return None
    command = decoded.get("cmd") if isinstance(decoded, dict) else None
    return command if isinstance(command, str) else None


def turn(prompt: str, grant: bool, fault: str) -> None:
    items: list[dict[str, object]] = [
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": prompt}]}
    ]
    calls = [item for item in respond(items) if item.get("type") == "function_call"]
    if not calls:
        return
    call = calls[0]
    call_id, command = call.get("call_id"), command_of(call.get("arguments"))
    if not isinstance(call_id, str) or command is None:
        fail(
            f"the endpoint's call is not an exec_command call with a call_id and a string cmd: {call}"
        )
    output = "sandbox: denied"
    if (grant and fault != "not-run") or (not grant and fault == "ungranted-ran"):
        bash = shutil.which("bash") or fail("bash is not on PATH")
        done = subprocess.run([bash, "-c", command], capture_output=True, text=True, check=False)  # noqa: S603 — the scripted model's own call.
        output = done.stdout + done.stderr
    if grant and fault == "no-decision":
        return
    respond(
        [
            *items,
            call,
            {"type": "function_call_output", "call_id": call_id, "output": output},
        ]
    )


def value_of(argv: list[str], flag: str) -> str:
    return argv[argv.index(flag) + 1] if flag in argv else fail(f"no {flag} in {argv}")


def main(argv: list[str]) -> int:
    fault = os.environ.get("BUDGET_DOUBLE_FAULT", "")
    # llmlint: ignore[structural_pattern_matching] This module shares the Python SDK's 3.9 floor (tools/network-telemetry/ruff.toml extends it), where `match` is a syntax error; the if chain on the leading argument is the 3.9 spelling of those alternatives.
    if argv[:1] == ["--version"]:
        print("codex-cli 0.0.0-double")
        return 0
    if argv[:1] == ["exec"]:
        turn(argv[-1], grant=False, fault="")
        return 0
    if argv[:1] != ["run"]:
        fail(f"unexpected launch {argv}")
    grant = Path(value_of(argv, "--config")).read_text().startswith("allowed_tools")
    turn(value_of(argv, "--prompt"), grant, fault)
    result: dict[str, object] = {"status": "nonzero" if grant and fault == "status" else "ok"}
    if grant and fault != "no-overlay":
        result["overlay_timings"] = {"build_ms": 3, "teardown_ms": 2}
        result["overlay_env"] = {"CODEX_HOME": "/overlay/codex-home"}
    print(json.dumps({"results": [result]}))
    return 1 if grant and fault == "exit" else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
