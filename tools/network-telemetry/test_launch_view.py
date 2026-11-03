"""What a harness launched through the recorder sees, and what its run reports.

The recorder sits between oneharness and every harness a live suite launches,
so it must hand the launch its argv as oneharness built it and leave the run's
stderr the harness's own. Each case drives the real `oneharness` CLI at a
two-line shim like the one oh_telemetry_setup writes, around the real
scripts/e2e-netshim.sh (and, on Linux, scripts/e2e-netmeasure.py); the
mock harness plays the harness, and a double plays only what this host is not
— Git Bash's argument rewriting, or a pasta its AppArmor refuses.
"""

from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
import unittest
from pathlib import Path

from support import (
    BASH,
    MOCK,
    NETSHIM,
    ONEHARNESS,
    WINDOWS,
    lines,
    measurements,
    need_builds,
    scratch,
    starts,
)


def shim(tmp: Path, harness: str, exe: str, real: Path) -> Path:
    """The shim oh_telemetry_setup writes for `exe`, wrapping `real`."""
    directory = tmp / "shims"
    directory.mkdir(exist_ok=True)
    path = directory / exe
    args = [
        "--suite",
        exe,
        "--harness",
        harness,
        "--real",
        str(real),
        "--self",
        str(path),
        "--file",
        str(tmp / f"{exe}.jsonl"),
    ]
    path.write_text(
        "#!/usr/bin/env bash\n"
        f"exec {shlex.quote(BASH)} {shlex.quote(str(NETSHIM))} "
        + " ".join(shlex.quote(a) for a in args)
        + ' -- "$@"\n'
    )
    path.chmod(0o755)
    return path


def oneharness(args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    base = {k: v for k, v in os.environ.items() if not k.startswith("ONEHARNESS_")}
    return subprocess.run(
        [str(ONEHARNESS), *args, "--format", "json"],
        capture_output=True,
        text=True,
        env={**base, "ONEHARNESS_NO_CONFIG": "1", **env},
        timeout=120,
        check=False,
    )


@unittest.skipIf(WINDOWS, "the Windows leg is played on unix by its `uname`; live-claude runs the real one")
class WindowsLeg(unittest.TestCase):
    """On a Windows leg the shim launches the harness from Git Bash, whose MSYS
    runtime rewrites any argument that looks like a POSIX path when it hands
    argv to a native program — unless told not to."""

    def test_a_compaction_through_a_windows_legs_shim_reaches_claude_as_its_slash_command(
        self,
    ) -> None:
        need_builds(self)
        tmp = scratch(self)
        host = tmp / "windows-host"
        host.mkdir()
        (host / "uname").write_text("#!/usr/bin/env bash\necho MINGW64_NT-10.0-20348\n")
        (host / "uname").chmod(0o755)
        # The MSYS runtime between Git Bash and the native claude: an argument
        # starting with one `/` becomes a path under Git's install root, as
        # live-claude's Windows leg received `-p C:/Program Files/Git/compact`.
        argv_log = tmp / "argv.jsonl"
        native = tmp / "native-claude"
        native.write_text(
            "#!/usr/bin/env bash\n"
            'if [ -z "${MSYS_NO_PATHCONV:-}" ] && [ "${MSYS2_ARG_CONV_EXCL:-}" != "*" ]; then\n'
            "  converted=()\n"
            '  for arg in "$@"; do\n'
            '    case "$arg" in /[!/]*) arg="C:/Program Files/Git$arg" ;; esac\n'
            '    converted+=("$arg")\n'
            "  done\n"
            '  set -- "${converted[@]}"\n'
            "fi\n"
            f"printf '%s\\0' \"$@\" | jq -Rsc 'split(\"\\u0000\")[:-1]' >>{shlex.quote(str(argv_log))}\n"
            f'exec {shlex.quote(str(MOCK))} "$@"\n'
        )
        native.chmod(0o755)
        claude = shim(tmp, "claude-code", "claude", native)
        for sub in ("proj", "store", "home"):
            (tmp / sub).mkdir()
        common = [
            "--session",
            "watch",
            "--session-dir",
            str(tmp / "store"),
            "--cwd",
            str(tmp / "proj"),
            "--bin",
            f"claude-code={claude}",
            "--env",
            f"CLAUDE_CONFIG_DIR={tmp / 'home'}",
            "--env",
            "MOCK_CLAUDE_SESSIONS=1",
        ]
        env = {"PATH": f"{host}{os.pathsep}{os.environ['PATH']}", "TMPDIR": str(tmp)}
        for prompt in ("alpha", "beta"):
            turn = oneharness(
                ["run", "--harness", "claude-code", "--mode", "bypass", "--prompt", prompt, *common],
                env,
            )
            self.assertEqual(turn.returncode, 0, turn.stdout + turn.stderr)

        compact = oneharness(["compact", "--harness", "claude-code", *common], env)
        outcome = json.loads(compact.stdout)
        self.assertEqual(outcome["status"], "completed", compact.stdout + compact.stderr)
        handed = [json.loads(line) for line in argv_log.read_text().splitlines()]
        self.assertIn("/compact", handed[-1], handed[-1])
        records = lines(tmp / "claude.jsonl")
        self.assertEqual(len(starts(records)), 3, records)
        for start in starts(records):
            (line,) = measurements(records, start["launch_id"])
            self.assertIn("this leg is MINGW64_NT", line["reason"])


@unittest.skipUnless(sys.platform.startswith("linux"), "the recorder tries a namespace on Linux only")
class RefusedNamespace(unittest.TestCase):
    """Where AppArmor refuses pasta its user namespace (ubuntu-24.04 without
    the sysctl the codex leg sets), the recorder runs the harness directly and
    records why; the run's stderr is still the harness's alone."""

    def test_a_run_after_a_refused_namespace_reports_only_the_harnesss_own_stderr(self) -> None:
        need_builds(self)
        tmp = scratch(self)
        refused = tmp / "refused-pasta"
        refused.mkdir()
        # The words the qwen leg's pasta printed before the run went on unmeasured.
        (refused / "pasta").write_text(
            "#!/usr/bin/env bash\n"
            "echo \"Couldn't write to /proc/self/uid_map: Operation not permitted\" >&2\n"
            "echo \"Couldn't configure user mappings\" >&2\n"
            "echo 'clone: Operation not permitted' >&2\n"
            "exit 1\n"
        )
        (refused / "pasta").chmod(0o755)
        qwen = shim(tmp, "qwen", "qwen", MOCK)
        own = "qwen: the provider answered 429 Too Many Requests"
        for exit_code, stderr in (("0", ""), ("3", own)):
            with self.subTest(exit=exit_code):
                run = oneharness(
                    [
                        "run",
                        "--harness",
                        "qwen",
                        "--bin",
                        f"qwen={qwen}",
                        "--prompt",
                        "echo the marker",
                        "--env",
                        "MOCK_STDOUT=I'm ready to assist you!",
                        "--env",
                        f"MOCK_STDERR={stderr}",
                        "--env",
                        f"MOCK_EXIT={exit_code}",
                    ],
                    {"PATH": f"{refused}{os.pathsep}{os.environ['PATH']}", "TMPDIR": str(tmp)},
                )
                (result,) = json.loads(run.stdout)["results"]
                self.assertEqual(result["stderr"].strip(), stderr, result)
        records = lines(tmp / "qwen.jsonl")
        for start in starts(records):
            (line,) = measurements(records, start["launch_id"])
            self.assertEqual(line["method"], "unavailable", line)
            self.assertIn("Couldn't write to /proc/self/uid_map", line["reason"])


if __name__ == "__main__":
    unittest.main()
