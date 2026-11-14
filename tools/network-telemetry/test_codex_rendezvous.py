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
import shutil
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from typing import TypedDict, cast

from support import BASH, HERE, LIB, MOCK, ONEHARNESS, SCRIPTS, WINDOWS, need_builds, scratch


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
# The server marker the parity runs pin (ONEHARNESS_INTERNAL_APP_SERVER_MARKER),
# shaped as oneharness's own, a hyphenated UUIDv7.
PINNED_MARKER = "01a1d7b3-0000-7000-8000-00000000c0de"


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


@unittest.skipIf(WINDOWS, "e2e-codex.sh fronts codex with the wrapper on the unix legs alone")
class Resolved(unittest.TestCase):
    """e2e-lib.sh's _oh_resolved_file, which spells the real codex as the
    server pool's resolve_executable does: every link followed."""

    def resolve(self, path: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [BASH, "-c", 'source "$1"; _oh_resolved_file "$2"', "resolve", str(LIB), str(path)],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )

    def test_a_chain_of_links_through_a_linked_directory_resolves_as_realpath_does(self) -> None:
        tmp = scratch(self)
        (tmp / "real" / "lib").mkdir(parents=True)
        (tmp / "real" / "lib" / "codex.js").write_text("")
        (tmp / "linked").symlink_to(tmp / "real")
        (tmp / "bin").mkdir()
        (tmp / "bin" / "hop").symlink_to("../linked/lib/codex.js")
        (tmp / "bin" / "codex").symlink_to(tmp / "bin" / "hop")
        entry = tmp / "linked" / ".." / "bin" / "codex"
        run = self.resolve(entry)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout, os.path.realpath(entry))
        self.assertEqual(run.stdout, str((tmp / "real" / "lib" / "codex.js").resolve()))

    def test_a_link_loop_fails_naming_the_path(self) -> None:
        tmp = scratch(self)
        (tmp / "a").symlink_to(tmp / "b")
        (tmp / "b").symlink_to(tmp / "a")
        run = self.resolve(tmp / "a")
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(run.stdout, "")
        self.assertIn(f"{tmp / 'a'}: too many levels of symbolic links", run.stderr)


# The launches the live Codex failures were in, made exactly as the suite
# makes them: the denied-tools journey's codex started directly in its trusted
# scratch project with the synced rules file, the shell-rewrite journey's
# `oneharness run --mock-rules`, whose codex reads the hooks it installs, and
# the controlled turn's shared app-server, which the pool starts and then
# dials at its --listen path.
LAUNCHES = r"""
source "$LIB"
[ "$1" = bare ] || oh_codex_rendezvous_setup
printf 'codex resolves to %s\n' "$(command -v codex)"
_oh_codex_direct "$PROJ" "$CODEX_HOME_DIR" "mkdir $DENIED" --dangerously-bypass-approvals-and-sandbox
"$ONEHARNESS_BIN" run --harness codex --mode bypass --cwd "$SANDBOX" --mock-rules "$RULES" \
  --prompt 'touch OHMOCKORIG1' --compact >/dev/null
"$ONEHARNESS_BIN" run --harness codex --mode bypass --control --session "oh-$1" \
  --session-dir "$SERVE/sessions-$1" --cwd "$SERVE" --prompt "SERVED-$1" --compact
[ "$1" = bare ] || cat "$_oh_codex_rendezvous_dir/rendezvous.jsonl"
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
        # Rooted under /tmp, not the platform temp dir: macOS's, under
        # /var/folders, leaves the controlled turn's control socket past
        # `sun_path`'s 104 bytes, which oneharness refuses before launching.
        self.tmp = Path(tempfile.mkdtemp(prefix="oh-", dir="/tmp"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        # Every journey's scratch, and the double's directory on PATH, is
        # reached through a link, as macOS's /tmp resolves through /private:
        # Codex looks a project up by the physical spelling, the suite passes
        # the linked one, and the pool launches the resolved executable where
        # a PATH lookup launches the linked one.
        self.real = self.tmp / "real"
        self.real.mkdir()
        self.base = self.tmp / "linked"
        self.base.symlink_to(self.real)
        self.bin = self.base / "bin"
        self.bin.mkdir()
        self.record = self.tmp / "record"
        # The pool starts a shared server from a scrubbed environment, so the
        # double's settings reach it from here; every launch sees them alike.
        settings = {
            "FAKE": "serve",
            "FAKE_RECORD": self.record,
            "FAKE_ARGV": self.tmp / "argv",
            "FAKE_PID": self.tmp / "pid",
            "FAKE_SERVER_PID": self.tmp / "server-pid",
            "FAKE_MOCK": MOCK,
        }
        (self.bin / "codex").write_text(
            "#!/usr/bin/env bash\n"
            + "".join(f"export {k}={shlex.quote(str(v))}\n" for k, v in settings.items())
            + f'FAKE_EXE="$0" exec bash {shlex.quote(str(FAKE))} "$@"\n'
        )
        (self.bin / "codex").chmod(0o755)
        self.proj = self.base / "rules" / "project"
        self.proj.mkdir(parents=True)
        self.home = self.base / "rules" / "codex-home"
        self.home.mkdir()
        self.sandbox = self.base / "mock"
        self.sandbox.mkdir()
        self.serve_dir = self.base / "served"
        self.serve_dir.mkdir()
        self.rules = self.base / "mock-rules.json"
        self.rules.write_text(
            '{"rules":[{"match":{"input":{"command":{"regex":"OHMOCKORIG1"}}},'
            '"action":{"rewrite":{"input":{"command":"true"}}}}]}'
        )
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}",
            "TMPDIR": str(self.base),
            "ONEHARNESS_NO_CONFIG": "1",
            "ONEHARNESS_BIN": str(ONEHARNESS),
            "LIB": str(LIB),
            "PROJ": str(self.proj),
            "CODEX_HOME_DIR": str(self.home),
            "DENIED": "rules-deny-1",
            "SANDBOX": str(self.sandbox),
            "RULES": str(self.rules),
            "SERVE": str(self.serve_dir),
            "XDG_STATE_HOME": str(self.base / "state"),
            # Each run's server stops with its turn, so the next run starts
            # its own rather than reuse it.
            "ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS": "0",
            # Each run's shared server is started under this one marker
            # rather than a fresh one, so both runs give Codex the same input.
            "ONEHARNESS_INTERNAL_APP_SERVER_MARKER": PINNED_MARKER,
            "ONEHARNESS_INTERNAL_CODEX_DAEMON_DIR": str(self.base / "daemon"),
            "CODEX_MOCK_DAEMON_DIR": str(self.base / "daemon"),
        }
        trust = self.bash(
            'source "$LIB"; _oh_codex_trust_config "$PROJ" "$CODEX_HOME_DIR/config.toml"'
        )
        self.assertEqual(trust.returncode, 0, trust.stderr)
        policy = self.base / "oneharness.toml"
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

    def launches(self, wrap: bool) -> tuple[str, list[str]]:
        """Each codex launch's record, and what the launches printed: where
        `codex` resolved, the controlled turn's report and, wrapped, the
        wrapper's evidence."""
        self.record.write_text("")
        run = self.bash(LAUNCHES, "wrapped" if wrap else "bare")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        # A stop reaches every process carrying the marker, so this run's
        # server is gone before the next starts under the same one.
        server = int((self.tmp / "server-pid").read_text())
        deadline = time.monotonic() + 30
        while True:
            try:
                os.kill(server, 0)
            except ProcessLookupError:
                break
            self.assertLess(time.monotonic(), deadline, f"server {server} outlived its turn")
            time.sleep(0.05)
        return self.record.read_text(), run.stdout.splitlines()

    def served(self, record: str, printed: list[str], name: str) -> tuple[str, str]:
        """The controlled turn's --listen path, from its report, having
        checked that the turn completed on the server it dialled there; and
        `record` with that path and the pool entry's name (keyed by the
        executable the pool resolved, the wrapper's own when it fronts codex)
        written generically."""
        report = json.loads(printed[1])
        (result,) = report["results"]
        self.assertEqual((result["status"], result["text"]), ("ok", f"SERVED-{name}"), result)
        command = [str(arg) for arg in result["command"]]
        listen = command[command.index("--listen") + 1].removeprefix("unix://")
        key = str(report["control"]["app_server"]["pool_key"])
        self.assertIn(f"/{key}/", listen)
        # The server reached the pool under the pinned marker, the one the
        # pool recorded for it.
        (server,) = [launch for launch in record.split("---\n") if "\narg app-server\n" in launch]
        self.assertIn(f"\nenv ONEHARNESS_APP_SERVER_MARKER={PINNED_MARKER}\n", server)
        self.assertIn(f"\npool-marker {PINNED_MARKER}\n", server)
        return listen, record.replace(listen, "<listen>").replace(key, "<pool-key>")

    def test_each_launch_sees_what_it_would_without_the_wrapper(self) -> None:
        self.prepare()
        bare, bare_printed = self.launches(wrap=False)
        wrapped, wrapped_printed = self.launches(wrap=True)
        self.assertEqual(bare_printed[0], f"codex resolves to {self.bin}/codex")
        self.assertRegex(
            wrapped_printed[0], r"codex resolves to .*/oh-codex-rendezvous\.[^/]+/codex"
        )
        _, bare = self.served(bare, bare_printed, "bare")
        wrapped_listen, wrapped = self.served(wrapped, wrapped_printed, "wrapped")
        # The shared server alone is bound at a private path: PATH's length,
        # in PATH's directory.
        private = f"{os.path.dirname(wrapped_listen)}/.{LISTEN_BASE[1:]}"
        self.assertEqual(len(private), len(wrapped_listen))
        key = wrapped_listen.split("/")[-3]
        private = private.replace(key, "<pool-key>")
        self.assertEqual(wrapped.count(f"arg unix://{private}\n"), 1, wrapped)
        self.assertEqual(bare.count("arg unix://<listen>\n"), 1, bare)
        # Every launch, each version probe included, recorded the same
        # executable, argv, cwd, environment and files but for that path.
        self.assertEqual(wrapped.replace(private, "<listen>"), bare)
        # PATH was linked to the server's socket before the turn dialled it.
        (evidence,) = [json.loads(line) for line in wrapped_printed[2:]]
        self.assertEqual(evidence["listen"], wrapped_listen)
        self.assertTrue(evidence["linked"], evidence)
        self.assertEqual(evidence["bound"], str((self.real / "daemon").resolve()))
        direct, *rest = bare.split("---\n")
        self.assertIn(f"cwd {self.proj}\n", direct)
        self.assertIn(
            'prefix_rule(pattern=["mkdir", "rules-deny-1"], decision="forbidden")', direct
        )
        # Trusted by the spelling the suite passes and the physical one.
        for spelling in (self.proj, self.proj.resolve()):
            self.assertIn(f'[projects."{spelling}"]', direct)
        mock = "".join(rest)
        self.assertIn("arg --dangerously-bypass-hook-trust\n", mock)
        sandbox = (self.sandbox, self.sandbox.resolve())
        trusted = ",".join(f'"{spelling}"={{trust_level="trusted"}}' for spelling in sandbox)
        self.assertIn(f"arg projects={{{trusted}}}\n", mock)
        self.assertIn("mock codex --rules", mock)
        self.assertIn("arg app-server\n", mock)


if __name__ == "__main__":
    unittest.main()
