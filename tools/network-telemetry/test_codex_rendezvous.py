"""scripts/e2e-codex-rendezvous.sh, the launch wrapper e2e-codex.sh puts in
front of codex: run for real, with codex_double.sh standing in for Codex.

The suite-level journeys (test_budget.CodexSuite) drive it under the real pool;
these hold each of its own paths — usage, the launches it hands on untouched,
the evidence it records and publishes, and each way that can fail.
"""

from __future__ import annotations

import json
import os
import shlex
import signal
import subprocess
import time
import unittest
from pathlib import Path
from typing import TypedDict, cast

from support import BASH, HERE, LIB, ONEHARNESS, SCRIPTS, WINDOWS, need_builds, scratch


class Evidence(TypedDict):
    """One line of the wrapper's evidence, as it writes it."""

    listen: str
    euid: str
    linked: bool
    target: str
    bound: str
    mode: str


WRAPPER = SCRIPTS / "e2e-codex-rendezvous.sh"
FAKE = HERE / "codex_double.sh"
LISTEN_BASE = "app-server.sock"


@unittest.skipIf(
    WINDOWS, "the wrapper fronts a shared server, which e2e-codex.sh runs on the unix legs"
)
class Wrapper(unittest.TestCase):
    def prepare(self) -> None:
        """The scratch layout every test starts from: a pool entry's socket
        directory, an empty evidence file, and where the double reports."""
        self.tmp = scratch(self)
        self.sock = self.tmp / "entry" / "sock"
        self.sock.mkdir(parents=True)
        self.listen = self.sock / LISTEN_BASE
        self.evidence = self.tmp / "rendezvous.jsonl"
        self.evidence.write_text("")
        self.argv = self.tmp / "argv"
        self.pid = self.tmp / "pid"
        self.socket_dir = self.tmp / "daemon"

    def env(self, fake: str, path: str | None = None) -> dict[str, str]:
        return {
            **os.environ,
            "PATH": path or os.environ["PATH"],
            "FAKE": fake,
            "FAKE_ARGV": str(self.argv),
            "FAKE_PID": str(self.pid),
            "FAKE_SOCKDIR": str(self.socket_dir),
            "FAKE_PUBLISHED": str(self.listen),
        }

    def wrapper(self, *args: str, evidence: Path | None = None) -> list[str]:
        return [
            BASH,
            str(WRAPPER),
            "--real",
            str(FAKE),
            "--evidence",
            str(evidence or self.evidence),
            "--",
            *args,
        ]

    def shared(self, listen: Path | None = None) -> tuple[str, ...]:
        return ("app-server", "--listen", f"unix://{listen or self.listen}")

    def run_wrapper(
        self, fake: str, *args: str, path: str | None = None, evidence: Path | None = None
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            self.wrapper(*args, evidence=evidence),
            env=self.env(fake, path),
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )

    def serve(self, fake: str, *args: str) -> subprocess.Popen[str]:
        """Start a shared server through the wrapper and return once it has
        published the --listen path; the test stops it."""
        process = subprocess.Popen(
            self.wrapper(*(args or self.shared())),
            env=self.env(fake),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(lambda: process.poll() is not None or process.kill())
        deadline = time.monotonic() + 30
        while not self.listen.is_symlink():
            self.assertIsNone(process.poll(), "the wrapper exited before publishing")
            self.assertLess(time.monotonic(), deadline, "the wrapper never published")
            time.sleep(0.02)
        return process

    def stop(self, process: subprocess.Popen[str]) -> tuple[int, str]:
        process.send_signal(signal.SIGTERM)
        _, stderr = process.communicate(timeout=30)
        return process.wait(), stderr

    def child_gone(self) -> bool:
        pid = int(self.pid.read_text())
        for _ in range(100):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return True
            time.sleep(0.05)
        return False

    def records(self) -> list[Evidence]:
        # Unchecked here: the shape is what the tests assert, the first of
        # them every field of the one record it expects.
        return [cast(Evidence, json.loads(line)) for line in self.evidence.read_text().splitlines()]

    def failing(self, tool: str) -> str:
        """A PATH whose `tool` fails, saying so."""
        bin_dir = self.tmp / f"failing-{tool}"
        bin_dir.mkdir()
        (bin_dir / tool).write_text(f"#!/usr/bin/env bash\necho '{tool}: refused' >&2\nexit 1\n")
        (bin_dir / tool).chmod(0o755)
        return f"{bin_dir}{os.pathsep}{os.environ['PATH']}"

    def test_its_usage_errors_exit_125_naming_the_option(self) -> None:
        self.prepare()
        cases = {
            ("--real",): "--real needs a value",
            ("--bogus",): "unknown option '--bogus'",
            ("--real", str(FAKE), "--"): "--real and --evidence are both required",
        }
        for argv, said in cases.items():
            with self.subTest(argv=argv):
                run = subprocess.run(
                    [BASH, str(WRAPPER), *argv], capture_output=True, text=True, timeout=30
                )
                self.assertEqual(run.returncode, 125, run.stderr)
                self.assertIn(f"e2e-codex-rendezvous: {said}", run.stderr)

    def test_a_launch_the_pool_did_not_make_reaches_codex_as_given(self) -> None:
        self.prepare()
        cases = {
            "an exec turn": ("exec", "--json", "hello"),
            "a tcp listener": ("app-server", "--listen", "ws://127.0.0.1:1"),
            "a relative path": ("app-server", "--listen", "unix://sock/app-server.sock"),
            "a trailing slash": ("app-server", "--listen", f"unix://{self.sock}/"),
            "a missing directory": ("app-server", "--listen", f"unix://{self.tmp}/gone/s.sock"),
            "a directory": ("app-server", "--listen", f"unix://{self.sock}"),
        }
        for case, argv in cases.items():
            with self.subTest(case=case):
                run = self.run_wrapper("args", *argv)
                self.assertEqual(run.returncode, 0, run.stderr)
                self.assertEqual(self.argv.read_text().splitlines(), list(argv))
                self.assertEqual(self.evidence.read_text(), "")

    def test_its_own_directory_comes_off_path_and_nothing_else_does(self) -> None:
        self.prepare()
        mine = str(self.tmp / "wrapper")
        record = self.tmp / "record"
        bash_dir = os.path.dirname(BASH)
        cases = {
            "first": (f"{mine}:{bash_dir}", bash_dir),
            "repeated, beside an empty entry": (f"{bash_dir}::{mine}:/x:{mine}", f"{bash_dir}::/x"),
            "a longer name it prefixes": (f"{mine}x:{bash_dir}", f"{mine}x:{bash_dir}"),
        }
        for case, (given, kept) in cases.items():
            with self.subTest(case=case):
                record.write_text("")
                run = subprocess.run(
                    [
                        BASH,
                        str(WRAPPER),
                        "--real",
                        str(FAKE),
                        "--evidence",
                        str(self.evidence),
                        "--drop-path",
                        mine,
                        "--",
                        "exec",
                        "hi",
                    ],
                    env={**self.env("record", given), "FAKE_RECORD": str(record)},
                    capture_output=True,
                    text=True,
                    timeout=60,
                    check=False,
                )
                self.assertEqual(run.returncode, 0, run.stderr)
                self.assertIn(f"env PATH={kept}\n", record.read_text())

    def test_a_shared_server_is_recorded_before_its_listen_path_is_published(self) -> None:
        self.prepare()
        process = self.serve("link")
        private = self.sock / f".{LISTEN_BASE[1:]}"
        argv = self.argv.read_text().splitlines()
        self.assertEqual(argv, ["app-server", "--listen", f"unix://{private}"])
        self.assertEqual(len(str(private)), len(str(self.listen)))
        (record,) = self.records()
        self.assertEqual(
            record,
            {
                "listen": str(self.listen),
                "euid": str(os.geteuid()),
                "linked": True,
                "target": str(self.socket_dir / "s"),
                "bound": str(self.socket_dir.resolve()),
                "mode": "700",
            },
        )
        self.assertEqual(os.readlink(self.listen), str(self.socket_dir / "s"))
        # A stop reaches the wrapper; it passes it on and goes with Codex,
        # taking the private path along.
        code, stderr = self.stop(process)
        self.assertEqual(code, 0, stderr)
        self.assertTrue(self.child_gone())
        self.assertFalse(private.is_symlink())

    def test_the_recorded_mode_is_the_socket_directorys_mode_as_stat_prints_it(self) -> None:
        # Setuid, setgid and sticky lead as a fourth digit, whether `ls` shows
        # them over an execute bit (4700's s, 2750's s, 1777's t) or without
        # one (2700's S, 1700's T), so none reads as plain 700.
        cases = {
            0o700: "700",
            0o755: "755",
            0o750: "750",
            0o1700: "1700",
            0o2700: "2700",
            0o4700: "4700",
            0o6700: "6700",
            0o2740: "2740",
            0o2750: "2750",
            0o1777: "1777",
        }
        for bits, recorded in cases.items():
            with self.subTest(mode=oct(bits)):
                self.prepare()
                self.socket_dir.mkdir()
                self.socket_dir.chmod(bits)
                self.assertEqual(self.socket_dir.stat().st_mode & 0o7777, bits)
                process = self.serve("link")
                (record,) = self.records()
                self.assertEqual(record["mode"], recorded)
                self.assertEqual(record["mode"], f"{bits:o}", "not as `stat -c %a` prints it")
                self.assertEqual(self.stop(process)[0], 0)

    def test_a_listen_name_that_starts_with_a_dot_gets_a_distinct_private_twin(self) -> None:
        self.prepare()
        self.listen = self.sock / ".hidden.sock"
        process = self.serve("link", *self.shared(self.listen))
        self.assertEqual(self.argv.read_text().splitlines()[-1], f"unix://{self.sock}/,hidden.sock")
        self.assertEqual(self.stop(process)[0], 0)

    def test_a_relative_link_is_resolved_from_its_directory_and_republished_as_it_is(self) -> None:
        self.prepare()
        process = self.serve("rel")
        (record,) = self.records()
        self.assertEqual(record["target"], "../rel/s")
        self.assertEqual(record["bound"], str((self.sock.parent / "rel").resolve()))
        self.assertEqual(os.readlink(self.listen), "../rel/s")
        self.assertTrue(self.listen.exists(), "the published link does not reach the socket")
        self.assertEqual(self.stop(process)[0], 0)

    def test_a_server_that_binds_its_path_itself_is_recorded_unlinked(self) -> None:
        self.prepare()
        process = self.serve("direct")
        private = self.sock / f".{LISTEN_BASE[1:]}"
        (record,) = self.records()
        self.assertEqual(
            (record["linked"], record["target"], record["bound"], record["mode"]),
            (False, str(private), "", ""),
        )
        self.assertEqual(os.readlink(self.listen), str(private))
        self.assertEqual(self.stop(process)[0], 0)

    def test_a_server_that_exits_before_binding_keeps_its_status_and_records_nothing(self) -> None:
        self.prepare()
        run = self.run_wrapper("exit:7", *self.shared())
        self.assertEqual(run.returncode, 7, run.stderr)
        self.assertEqual(self.evidence.read_text(), "")
        self.assertFalse(self.listen.is_symlink())

    def test_a_stale_private_path_it_cannot_clear_fails_before_codex_starts(self) -> None:
        self.prepare()
        stale = self.sock / f".{LISTEN_BASE[1:]}"
        stale.mkdir()
        (stale / "x").write_text("")
        run = self.run_wrapper("link", *self.shared())
        self.assertEqual(run.returncode, 125, run.stderr)
        self.assertIn(f"cannot clear the stale private path {stale}", run.stderr)
        self.assertFalse(self.argv.exists(), "codex started anyway")

    def test_each_way_recording_fails_exits_125_and_stops_codex(self) -> None:
        self.prepare()
        missing = self.tmp / "gone" / "rendezvous.jsonl"
        cases: dict[str, tuple[str | None, Path | None, str]] = {
            "the euid": (self.failing("id"), None, "cannot read the euid this server runs as"),
            "the link": (self.failing("readlink"), None, "cannot read the link Codex left at"),
            "the mode": (self.failing("ls"), None, "cannot read the mode of"),
            "the encoding": (self.failing("jq"), None, "cannot encode the rendezvous evidence"),
            "the append": (None, missing, "cannot append the rendezvous evidence to"),
        }
        for case, (path, evidence, said) in cases.items():
            with self.subTest(failing=case):
                for stale in (self.listen, self.sock / f".{LISTEN_BASE[1:]}"):
                    stale.unlink(missing_ok=True)
                run = self.run_wrapper("link", *self.shared(), path=path, evidence=evidence)
                self.assertEqual(run.returncode, 125, run.stderr)
                self.assertIn(said, run.stderr)
                self.assertTrue(self.child_gone(), "the wrapper left codex serving")
                self.assertFalse(self.listen.is_symlink(), "it published without evidence")

    def test_a_failed_publication_rename_stops_codex_after_creating_the_link(self) -> None:
        self.prepare()
        run = self.run_wrapper("link", *self.shared(), path=self.failing("mv"))
        self.assertEqual(run.returncode, 125, run.stderr)
        self.assertIn(f"cannot link {self.listen} to the socket at", run.stderr)
        self.assertIn("mv: refused", run.stderr)
        self.assertEqual(len(self.records()), 1)
        self.assertEqual(len(list(self.sock.glob("*.partial"))), 1)
        self.assertTrue(next(self.sock.glob("*.partial")).is_symlink())
        self.assertFalse(self.listen.is_symlink())
        self.assertTrue(self.child_gone())

    @unittest.skipIf(not WINDOWS and os.geteuid() == 0, "root enters a directory of any mode")
    def test_a_socket_directory_it_cannot_enter_exits_125_and_stops_codex(self) -> None:
        self.prepare()
        self.socket_dir.mkdir(mode=0o000)
        self.addCleanup(self.socket_dir.chmod, 0o700)
        run = self.run_wrapper("dangling", *self.shared())
        self.assertEqual(run.returncode, 125, run.stderr)
        self.assertIn(f"cannot enter {self.socket_dir}", run.stderr)
        self.assertTrue(self.child_gone())

    @unittest.skipIf(not WINDOWS and os.geteuid() == 0, "root writes a directory of any mode")
    def test_a_listen_path_it_cannot_publish_exits_125_and_stops_codex(self) -> None:
        self.prepare()
        # A `jq` that makes the --listen directory read-only as the evidence
        # is encoded, the last step before publishing.
        bin_dir = self.tmp / "locking-jq"
        bin_dir.mkdir()
        real = subprocess.run(
            ["bash", "-c", "command -v jq"], capture_output=True, text=True, check=True
        ).stdout.strip()
        (bin_dir / "jq").write_text(
            f"#!/usr/bin/env bash\nchmod 555 {shlex.quote(str(self.sock))}\n"
            f'exec {shlex.quote(real)} "$@"\n'
        )
        (bin_dir / "jq").chmod(0o755)
        self.addCleanup(self.sock.chmod, 0o755)
        run = self.run_wrapper(
            "link", *self.shared(), path=f"{bin_dir}{os.pathsep}{os.environ['PATH']}"
        )
        self.assertEqual(run.returncode, 125, run.stderr)
        self.assertIn(f"cannot link {self.listen} to the socket at", run.stderr)
        self.assertTrue(self.child_gone())
        self.assertEqual(len(self.records()), 1)

    @unittest.skipIf(not WINDOWS and os.geteuid() == 0, "root writes a directory of any mode")
    def test_a_private_path_it_cannot_remove_is_named_and_codexs_status_kept(self) -> None:
        self.prepare()
        self.addCleanup(self.sock.chmod, 0o755)
        run = self.run_wrapper("lock-after", *self.shared())
        self.assertEqual(run.returncode, 3, run.stderr)
        self.assertIn(
            f"could not remove {self.sock}/.{LISTEN_BASE[1:]}; delete it by hand", run.stderr
        )


# The two launches the live Codex failures were in, made exactly as the suite
# makes them: the denied-tools journey's codex started directly in its trusted
# scratch project with the synced rules file, and the shell-rewrite journey's
# `oneharness run --mock-rules`, whose codex reads the hooks it installs.
LAUNCHES = r"""
source "$LIB"
[ "$1" = bare ] || oh_codex_rendezvous_setup
printf 'codex resolves to %s\n' "$(command -v codex)"
_oh_codex_direct "$PROJ" "$CODEX_HOME_DIR" "mkdir $DENIED" --dangerously-bypass-approvals-and-sandbox
"$ONEHARNESS_BIN" run --harness codex --mode bypass --cwd "$SANDBOX" --mock-rules "$RULES" \
  --prompt 'touch OHMOCKORIG1' --compact >/dev/null
"""


@unittest.skipIf(WINDOWS, "e2e-codex.sh fronts codex with the wrapper on the unix legs alone")
class Transparent(unittest.TestCase):
    """What a codex launch the wrapper fronts sees, against the same launch
    without it: one recording double behind both."""

    def prepare(self) -> None:
        """Both journeys' scratch: the recording double first on PATH as
        `codex`, the rules journey's trusted project with its synced rules,
        and the mock journey's sandbox and ruleset."""
        need_builds(self)
        self.tmp = scratch(self)
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        (self.bin / "codex").symlink_to(FAKE)
        self.record = self.tmp / "record"
        self.proj = self.tmp / "rules" / "project"
        self.proj.mkdir(parents=True)
        self.home = self.tmp / "rules" / "codex-home"
        self.home.mkdir()
        self.sandbox = self.tmp / "mock"
        self.sandbox.mkdir()
        self.rules = self.tmp / "mock-rules.json"
        self.rules.write_text(
            '{"rules":[{"match":{"input":{"command":{"regex":"OHMOCKORIG1"}}},'
            '"action":{"rewrite":{"input":{"command":"true"}}}}]}'
        )
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}",
            "TMPDIR": str(self.tmp),
            "ONEHARNESS_NO_CONFIG": "1",
            "ONEHARNESS_BIN": str(ONEHARNESS),
            "LIB": str(LIB),
            "PROJ": str(self.proj),
            "CODEX_HOME_DIR": str(self.home),
            "DENIED": "rules-deny-1",
            "SANDBOX": str(self.sandbox),
            "RULES": str(self.rules),
            "FAKE": "record",
            "FAKE_RECORD": str(self.record),
            "FAKE_ARGV": str(self.tmp / "argv"),
            "FAKE_PID": str(self.tmp / "pid"),
        }
        # The journey's own setup: its trust config, and the rules `sync` writes.
        trust = self.bash(
            'source "$LIB"; _oh_codex_trust_config "$PROJ" "$CODEX_HOME_DIR/config.toml"'
        )
        self.assertEqual(trust.returncode, 0, trust.stderr)
        policy = self.tmp / "oneharness.toml"
        policy.write_text('[harness.codex]\ndenied_tools = ["Bash(mkdir rules-deny-1:*)"]\n')
        sync = subprocess.run(
            [
                str(ONEHARNESS),
                "sync",
                "--harness",
                "codex",
                "--cwd",
                str(self.proj),
                "--config",
                str(policy),
                "--compact",
            ],
            env={**self.env, "ONEHARNESS_NO_CONFIG": ""},
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(sync.returncode, 0, sync.stderr)

    def bash(self, script: str, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [BASH, "-c", f"set -euo pipefail\n{script}", "launches", *args],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )

    def launches(self, wrap: bool) -> tuple[str, str]:
        """Each codex launch's record, and where `codex` resolved."""
        self.record.write_text("")
        run = self.bash(LAUNCHES, "wrapped" if wrap else "bare")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        return self.record.read_text(), run.stdout

    def test_each_launch_sees_what_it_would_without_the_wrapper(self) -> None:
        self.prepare()
        bare, bare_resolved = self.launches(wrap=False)
        wrapped, wrapped_resolved = self.launches(wrap=True)
        self.assertIn(f"codex resolves to {self.bin}/codex", bare_resolved)
        self.assertRegex(wrapped_resolved, r"codex resolves to .*/oh-codex-rendezvous\.[^/]+/codex")
        # Every launch, the version probe included, recorded the same
        # executable, argv, cwd, environment and files.
        self.assertEqual(wrapped, bare)
        direct, *rest = bare.split("---\n")
        self.assertIn(f"cwd {self.proj}\n", direct)
        self.assertIn(
            'prefix_rule(pattern=["mkdir", "rules-deny-1"], decision="forbidden")', direct
        )
        self.assertIn(f'[projects."{self.proj}"]', direct)
        mock = "".join(rest)
        self.assertIn("arg --dangerously-bypass-hook-trust\n", mock)
        self.assertIn(
            f'arg projects={{"{self.sandbox.resolve()}"={{trust_level="trusted"}}}}', mock
        )
        self.assertIn("mock codex --rules", mock)


if __name__ == "__main__":
    unittest.main()
