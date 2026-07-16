"""External CLI fixture for malformed and additive Python SDK boundaries."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import Any


def forward_with_additions() -> int:
    """Drive the real CLI and add compatible future fields to its output."""
    binary = os.environ["PYTHON_SDK_REAL_CLI"]
    completed = subprocess.run(
        [binary, *sys.argv[1:]],
        check=False,
        capture_output=True,
        env=os.environ,
    )
    sys.stderr.buffer.write(completed.stderr)
    for line in completed.stdout.splitlines():
        value: dict[str, Any] = json.loads(line)
        value["future_output_field"] = {"preserved": True}
        if value.get("harnesses"):
            value["harnesses"][0]["future_harness_field"] = 7
        if value.get("results"):
            value["results"][0]["future_result_field"] = 7
        sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    return completed.returncode


def main() -> int:
    """Select one deterministic fixture behavior."""
    mode = os.environ.get("PYTHON_SDK_FIXTURE_MODE")
    if mode == "additive":
        return forward_with_additions()
    if mode == "process-error":
        sys.stderr.write("fixture process failed\n")
        return 3
    if mode == "invalid-json":
        sys.stdout.write("{broken")
        return 0
    if mode == "invalid-json-nonzero":
        sys.stdout.write("{broken")
        sys.stderr.write("fixture invalid response\n")
        return 3
    if mode == "invalid-json-stream":
        sys.stdout.write("\nnot-json\n")
        return 0
    outputs = {
        "run": '{"schema_version":"1","results":[{"usage":{"input_tokens":"many"}}]}',
        "run-stream": '{"type":"event","event":{}}\n',
        "history": '[{"schema_version":"1","usage":{"input_tokens":"many"}}]',
        "history-watch": '{"type":"record","record":{}}\n',
        "history-list": '[{"id":42}]',
        "list": '{"schema_version":"1","harnesses":[{"id":42}]}',
        "detect": '{"schema_version":"1","detected":[{"id":42}]}',
    }
    if mode not in outputs:
        sys.stderr.write(f"unknown PYTHON_SDK_FIXTURE_MODE: {mode!r}\n")
        return 2
    sys.stdout.write(outputs[mode])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
