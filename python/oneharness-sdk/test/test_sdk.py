"""Public Python SDK tests across real subprocess boundaries."""

from __future__ import annotations

import asyncio
import os
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any, cast

from oneharness_sdk import (
    ContractError,
    HistoryNotFoundError,
    OneHarness,
    OneHarnessProcessError,
)

ROOT = Path(__file__).resolve().parents[3]
SUFFIX = ".exe" if os.name == "nt" else ""
BINARY = ROOT / "target" / "debug" / f"oneharness{SUFFIX}"
MOCK = ROOT / "target" / "debug" / f"oneharness-mock-harness{SUFFIX}"
FIXTURE = Path(__file__).with_name("fixture_cli.py")


class OneHarnessTests(unittest.IsolatedAsyncioTestCase):
    """Exercise every public method through the built CLI."""

    def client(self) -> OneHarness:
        """Return a hermetic client for the real development binary."""
        return OneHarness(executable=str(BINARY), env={"ONEHARNESS_NO_CONFIG": "1"})

    def fixture(self, mode: str) -> OneHarness:
        """Return an external fixture process for malformed-contract tests."""
        return OneHarness(
            executable=sys.executable,
            executable_args=(str(FIXTURE),),
            env={"PYTHON_SDK_FIXTURE_MODE": mode},
        )

    async def test_run_list_detect_and_history_cross_the_cli_boundary(self) -> None:
        """Cover every bounded method with observable real CLI behavior."""
        history_dir = tempfile.mkdtemp(prefix="oneharness-python-history-")
        client = self.client()
        report = await client.run(
            {
                "prompt": "python sdk boundary",
                "harnesses": ["claude-code"],
                "mode": "bypass",
                "history": True,
                "history_name": "python-session",
                "history_dir": history_dir,
                "history_labels": {"graph": "release"},
                "events": True,
                "timeout_seconds": 30,
                "env": {"MOCK_STDOUT": '{"result":"hello from python"}'},
                "bins": {"claude-code": str(MOCK)},
            }
        )
        self.assertEqual(report["results"][0]["text"], "hello from python")
        self.assertIsNone(report["results"][0]["usage"]["input_tokens"])

        listed = await client.list()
        self.assertIn("claude-code", {item["id"] for item in listed})
        detected = await client.detect(("claude-code",))
        self.assertEqual(detected[0]["id"], "claude-code")

        records = await client.history(
            {
                "session": "python-session",
                "project": str(ROOT),
                "history_dir": history_dir,
            }
        )
        self.assertEqual(records[0]["labels"], {"graph": "release"})
        sessions = await client.history_list({"history_dir": history_dir, "all_projects": True})
        self.assertEqual(sessions[0]["name"], "python-session")
        project_sessions = await client.history_list(
            {"history_dir": history_dir, "project": str(ROOT)}
        )
        self.assertEqual(project_sessions[0]["name"], "python-session")

        resumed = await client.run(
            {
                "prompt": "fork from python",
                "harnesses": ["claude-code"],
                "mode": "bypass",
                "resume": "python-sdk-session",
                "fork": True,
                "bins": {"claude-code": str(MOCK)},
            }
        )
        self.assertEqual(resumed["results"][0]["status"], "ok")

    async def test_run_stream_validates_each_envelope(self) -> None:
        """Yield normalized events followed by the complete report."""
        envelopes = []
        async for envelope in self.client().run_stream(
            {
                "prompt": "python stream",
                "harnesses": ["opencode"],
                "mode": "bypass",
                "env": {
                    "MOCK_STDOUT": "\n".join(
                        (
                            '{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"input":{"command":"echo hi"}}}}',
                            '{"type":"text","part":{"type":"text","text":"stream done"}}',
                        )
                    )
                },
                "bins": {"opencode": str(MOCK)},
            }
        ):
            envelopes.append(envelope)
        self.assertEqual([item["type"] for item in envelopes], ["event", "result"])
        self.assertEqual(envelopes[0]["event"]["name"], "bash")
        self.assertEqual(envelopes[1]["report"]["results"][0]["text"], "stream done")

    async def test_run_stream_cancellation_terminates_the_subprocess(self) -> None:
        """Closing early prevents the provider fixture from completing its stream."""
        directory = Path(tempfile.mkdtemp(prefix="oneharness-python-cancel-"))
        log = directory / "mock.log"
        stream = self.client().run_stream(
            {
                "prompt": "stop after one action",
                "harnesses": ["opencode"],
                "mode": "bypass",
                "env": {
                    "MOCK_LOG_FILE": str(log),
                    "MOCK_STREAM_DELAY_MS": "500",
                    "MOCK_STDOUT": "\n".join(
                        (
                            '{"type":"tool_use","part":{"type":"tool","tool":"first","state":{"input":{}}}}',
                            '{"type":"tool_use","part":{"type":"tool","tool":"second","state":{"input":{}}}}',
                            '{"type":"tool_use","part":{"type":"tool","tool":"third","state":{"input":{}}}}',
                        )
                    ),
                },
                "bins": {"opencode": str(MOCK)},
            }
        )
        first = await stream.__anext__()
        self.assertEqual(first["event"]["name"], "first")
        await cast("Any", stream).aclose()
        await asyncio.sleep(0.7)
        self.assertNotIn("COMPLETE", log.read_text(encoding="utf-8"))

    async def test_history_watch_filters_records_and_closes(self) -> None:
        """Follow labeled records through the live CLI watch process."""
        history_dir = tempfile.mkdtemp(prefix="oneharness-python-watch-")
        client = self.client()
        await client.run(
            {
                "prompt": "watch from python",
                "harnesses": ["claude-code"],
                "mode": "bypass",
                "history": True,
                "history_dir": history_dir,
                "history_labels": {"graph": "release", "task": "python"},
                "bins": {"claude-code": str(MOCK)},
            }
        )
        watch = client.history_watch(
            {
                "project": str(ROOT),
                "history_dir": history_dir,
                "labels": {"graph": "release", "task": "python"},
            }
        )
        envelope = await watch.__anext__()
        self.assertEqual(envelope["record"]["prompt"], "watch from python")
        await cast("Any", watch).aclose()

    async def test_missing_history_raises_the_typed_error(self) -> None:
        """Map both bounded lookups and missing watch cursors to one error type."""
        history_dir = tempfile.mkdtemp(prefix="oneharness-python-missing-")
        client = self.client()
        with self.assertRaises(HistoryNotFoundError):
            await client.history({"session": "missing", "history_dir": history_dir})
        watch = client.history_watch(
            {
                "after": "00000000-0000-7000-8000-000000000000",
                "all_projects": True,
                "history_dir": history_dir,
            }
        )
        with self.assertRaises(HistoryNotFoundError):
            await watch.__anext__()

    async def test_inputs_are_strict_before_spawning(self) -> None:
        """Reject unknown and malformed fields without reaching a missing binary."""
        client = OneHarness(executable=str(ROOT / "missing-oneharness"))
        with self.assertRaisesRegex(ContractError, "invalid oneharness run options"):
            await client.run(cast("Any", {"prompt": "typo", "harneses": ["codex"]}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness history options"):
            await client.history(cast("Any", {}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness history list options"):
            await client.history_list(cast("Any", {"all_project": True}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness history watch options"):
            client.history_watch(cast("Any", {"all_project": True}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness detect options"):
            await client.detect(cast("Any", "codex"))

    async def test_malformed_external_contracts_are_rejected(self) -> None:
        """Validate bounded values and every stream line from an external process."""
        cases = (
            ("run", self.fixture("run").run({"prompt": "bad"})),
            ("history", self.fixture("history").history({"last": True})),
            ("history list", self.fixture("history-list").history_list()),
            ("list", self.fixture("list").list()),
            ("detect", self.fixture("detect").detect()),
        )
        for label, operation in cases:
            with self.subTest(label=label), self.assertRaises(ContractError):
                await operation

        with self.assertRaises(ContractError):
            await self.fixture("run-stream").run_stream({"prompt": "bad"}).__anext__()
        with self.assertRaises(ContractError):
            await self.fixture("history-watch").history_watch().__anext__()

        with self.assertRaisesRegex(OneHarnessProcessError, "fixture process failed"):
            await self.fixture("process-error").list()
        with self.assertRaisesRegex(ContractError, "returned invalid JSON"):
            await self.fixture("invalid-json").list()
        with self.assertRaisesRegex(OneHarnessProcessError, "fixture invalid response"):
            await self.fixture("invalid-json-nonzero").run({"prompt": "bad"})
        with self.assertRaisesRegex(ContractError, "invalid JSON"):
            await self.fixture("invalid-json-stream").run_stream({"prompt": "bad"}).__anext__()

    async def test_default_executable_is_resolved_from_path(self) -> None:
        """The package default executes an installed oneharness command."""
        directory = Path(tempfile.mkdtemp(prefix="oneharness-python-path-"))
        installed = directory / f"oneharness{SUFFIX}"
        try:
            installed.symlink_to(BINARY)
        except OSError:
            self.skipTest("creating an executable symlink is unavailable")
        old_path = os.environ.get("PATH")
        os.environ["PATH"] = os.pathsep.join((str(directory), old_path or ""))
        try:
            listed = await OneHarness(env={"ONEHARNESS_NO_CONFIG": "1"}).list()
        finally:
            if old_path is None:
                os.environ.pop("PATH", None)
            else:
                os.environ["PATH"] = old_path
        self.assertIn("codex", {item["id"] for item in listed})

    async def test_additive_output_fields_are_preserved(self) -> None:
        """Accept a newer compatible CLI response without erasing its additions."""
        client = OneHarness(
            executable=sys.executable,
            executable_args=(str(FIXTURE),),
            env={
                "ONEHARNESS_NO_CONFIG": "1",
                "PYTHON_SDK_FIXTURE_MODE": "additive",
                "PYTHON_SDK_REAL_CLI": str(BINARY),
            },
        )
        listed = await client.list()
        self.assertEqual(listed[0]["future_harness_field"], 7)


if __name__ == "__main__":
    unittest.main()
