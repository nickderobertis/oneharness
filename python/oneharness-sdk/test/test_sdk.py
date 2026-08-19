"""Public Python SDK tests across real subprocess boundaries."""

from __future__ import annotations

import asyncio
import json
import os
import re
import sys
import tempfile
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any, cast

from oneharness_sdk import (
    ContractError,
    HistoryNotFoundError,
    OneHarness,
    OneHarnessProcessError,
)
from oneharness_sdk._client import (
    _CAPABILITIES,
    _capability_arguments,
    _input,
    _states_a_choice,
    _validate,
)
from oneharness_sdk._client import (
    _SCHEMAS as SCHEMAS,
)

from .scratch import scratch


@contextmanager
def without_ambient_overrides() -> Iterator[None]:
    """Hide the machine's own `ONEHARNESS_*` overrides from the spawned CLI.

    The client passes the parent environment through, so a developer box — or an
    orchestrator that exports `ONEHARNESS_HARNESSES` — would otherwise reshape
    the very layering the config and sync tests assert on. `ONEHARNESS_NO_CONFIG`
    is the usual escape hatch and is unavailable here: those verbs *are* the
    layering.
    """
    saved = {key: value for key, value in os.environ.items() if key.startswith("ONEHARNESS_")}
    for key in saved:
        del os.environ[key]
    try:
        yield
    finally:
        os.environ.update(saved)


ROOT = Path(__file__).resolve().parents[3]
SUFFIX = ".exe" if os.name == "nt" else ""
BINARY = ROOT / "target" / "debug" / f"oneharness{SUFFIX}"
MOCK = ROOT / "target" / "debug" / f"oneharness-mock-harness{SUFFIX}"
HISTORY_TRACE = "\n".join(
    (
        '{"type":"turn.started"}',
        '{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"hello from python"}}',
        '{"type":"turn.completed"}',
    )
)
FIXTURE = Path(__file__).with_name("fixture_cli.py")
CONTRACT_MATRIX = json.loads(
    (ROOT / "tests" / "fixtures" / "sdk-contract-matrix.json").read_text(encoding="utf-8")
)
INPUT_KEYS = json.loads(
    (
        ROOT
        / "python"
        / "oneharness-sdk"
        / "src"
        / "oneharness_sdk"
        / "_generated"
        / "input-keys.json"
    ).read_text(encoding="utf-8")
)


# One schema-valid value per option the manifest binds, keyed by the option's
# own name so an option shared across verbs is populated the same way in each.
# A binding with no entry here fails
# `test_every_bound_option_is_a_field_of_its_options_contract` by name, which is
# what keeps this table from going stale as the CLI grows flags.
POPULATED: dict[str, Any] = {
    "after": "0198f0d0-7b31-7000-8000-000000000001",
    "all": True,
    "allProjects": False,
    "batchPrompts": ["first", "second"],
    "batchStrategy": "speed",
    "bins": {"codex": "/bin/codex"},
    "check": True,
    "config": "/nowhere/oneharness.toml",
    "control": True,
    "cwd": "/nowhere",
    "denyIfContains": "rm -rf",
    "env": {"MOCK_STDOUT": "hi"},
    "event": "{}",
    "events": True,
    "exclude": ["goose"],
    "force": True,
    "fork": True,
    "global": False,
    "harness": "claude-code",
    "harnesses": ["codex"],
    "history": True,
    "historyDir": "/nowhere/history",
    "historyLabels": {"run": "sdk"},
    "historyName": "session",
    "input": "do this instead",
    "labels": {"run": "sdk"},
    "last": False,
    "maxParallel": 2,
    "mockHarnesses": ["codex"],
    "mockRules": "/nowhere/rules.json",
    "mode": "bypass",
    "models": ["gpt-5"],
    "noConfig": False,
    "noHistory": False,
    "outputDir": "/nowhere/out",
    "outputFormat": "json",
    "passthrough": ["--verbose"],
    "path": "/nowhere/oneharness.toml",
    "permitPrompts": True,
    "printCommand": True,
    "project": "oneharness",
    "prompt": "hello",
    "promptFiles": ["/nowhere/prompt.txt"],
    "reason": "policy",
    "reasoning": "high",
    "requireAvailable": True,
    "resume": "prior-session",
    "rules": "/nowhere/rules.json",
    "runMode": "parallel",
    "schema": "/nowhere/schema.json",
    "schemaMaxRetries": 2,
    "session": "work",
    "sessionDir": "/nowhere/sessions",
    "spyFile": "/nowhere/spy.jsonl",
    "system": "be terse",
    "systemFile": "/nowhere/system.txt",
    "timeoutSeconds": 30,
    "variant": "claude-code:work",
    "yes": True,
}


def _contradicts(bound: dict[str, Any], option: str) -> bool:
    """Would populating this option and its suppressor be a refused pair?

    Asked with the client's own predicate rather than a second copy of the rule:
    a table that decided contradictions differently from the code under test
    would drift into asserting nothing.

    `bound` carries `Any` values because it holds rows of the generated
    `capabilities.json` as they were deserialized — the same boundary
    `_states_a_choice` reads them at, and this helper only forwards them there.
    """
    binding = bound.get(option)
    if binding is None or binding["unless"] is None:
        return False
    if binding.get("unless_resolution", "refuse") == "prefer":
        return False
    return _states_a_choice(binding, POPULATED.get(option)) and _states_a_choice(
        bound[binding["unless"]], POPULATED.get(binding["unless"])
    )


def python_input(root: str, value: Any) -> Any:
    """Translate the shared camelCase contract fixture to Python public names."""
    if not isinstance(value, dict):
        return value
    inverse = {camel: snake for snake, camel in INPUT_KEYS.get(root, {}).items()}
    return {inverse.get(key, key): item for key, item in value.items()}


class OneHarnessTests(unittest.IsolatedAsyncioTestCase):
    """Exercise every public method through the built CLI."""

    def client(self) -> OneHarness:
        """Return a hermetic client for the real development binary."""
        return OneHarness(executable=str(BINARY), env={"ONEHARNESS_NO_CONFIG": "1"})

    def test_variant_composed_id_is_a_typed_run_input(self) -> None:
        value = {
            "prompt": "typed variant",
            "harnesses": ["codex:apikey"],
        }
        self.assertEqual(_validate("run_options", value, "variant options"), value)

    async def test_run_mock_uses_shipped_responder(self) -> None:
        report = await self.client().run_mock(
            "claude-code",
            {"prompt": "deterministic", "mode": "bypass"},
            {"stdout": '{"result":"python mock"}', "exit_code": 0, "latency_ms": 1},
        )
        self.assertEqual(report["results"][0]["text"], "python mock")

    def fixture(self, mode: str) -> OneHarness:
        """Return an external fixture process for malformed-contract tests."""
        return OneHarness(
            executable=sys.executable,
            executable_args=(str(FIXTURE),),
            env={"PYTHON_SDK_FIXTURE_MODE": mode},
        )

    def test_generated_validators_match_the_shared_sdk_acceptance_matrix(self) -> None:
        """Keep Python acceptance identical to the Rust and Node contracts."""
        self.assertGreater(len(CONTRACT_MATRIX["cases"]), 0)
        for fixture in CONTRACT_MATRIX["cases"]:
            root = fixture["root"]
            value = python_input(root, fixture["value"])
            with self.subTest(case=fixture["name"]):
                if fixture["accepted"]:
                    self.assertEqual(_validate(root, value, "shared SDK fixture"), value)
                else:
                    with self.assertRaises(ContractError):
                        _validate(root, value, "shared SDK fixture")

    async def test_run_list_detect_and_history_cross_the_cli_boundary(self) -> None:
        """Cover every bounded method with observable real CLI behavior."""
        history_dir = str(scratch(self, "history"))
        client = self.client()
        report = await client.run(
            {
                "prompt": "python sdk boundary",
                "harnesses": ["codex"],
                "mode": "bypass",
                "history": True,
                "history_name": "python-session",
                "history_dir": history_dir,
                "history_labels": {"graph": "release"},
                "events": True,
                "timeout_seconds": 30,
                "env": {"MOCK_STDOUT": HISTORY_TRACE},
                "bins": {"codex": str(MOCK)},
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
        exact = await client.history(
            {"session": records[0]["history_id"], "history_dir": history_dir}
        )
        self.assertEqual(len(exact), 1)
        self.assertEqual(exact[0]["history_id"], records[0]["history_id"])
        sessions = await client.history_list({"history_dir": history_dir, "all_projects": True})
        self.assertEqual(sessions[0]["name"], "python-session")
        project_sessions = await client.history_list(
            {"history_dir": history_dir, "project": str(ROOT)}
        )
        self.assertEqual(project_sessions[0]["name"], "python-session")

        await client.run(
            {
                "prompt": "python history without timing",
                "harnesses": ["claude-code"],
                "history": True,
                "history_name": "python-session-unmeasured",
                "history_dir": history_dir,
                "env": {"MOCK_STDOUT": '{"type":"result","result":"done"}'},
                "bins": {"claude-code": str(MOCK)},
            }
        )
        unmeasured = await client.history(
            {"session": "python-session-unmeasured", "history_dir": history_dir}
        )
        self.assertEqual(unmeasured[0]["schema_version"], "1.1")
        self.assertNotIn("model_ms", unmeasured[0])
        unmeasured_event = json.loads(json.dumps(unmeasured[0]))
        unmeasured_event["events"] = [
            {
                "kind": "tool_call",
                "name": "shell",
                "input": {},
                "output": None,
                "index": 0,
                "tool_call_id": "unmeasured-call",
            }
        ]
        self.assertEqual(
            _validate("history_record", unmeasured_event, "history record"),
            unmeasured_event,
        )
        unmeasured_event["events"][0]["started_at"] = "2026-07-19T00:00:00Z"
        with self.assertRaises(ContractError):
            _validate("history_record", unmeasured_event, "history record")

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
        directory = scratch(self, "cancel")
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
        """Resume after one record and filter later records without duplication."""
        history_dir = str(scratch(self, "watch"))
        client = self.client()
        records: list[Any] = []
        for name, prompt, graph in (
            ("cursor-record", "cursor record", "release"),
            ("filtered-record", "filtered record", "other"),
            ("resumed-record", "resumed record", "release"),
        ):
            await client.run(
                {
                    "prompt": prompt,
                    "harnesses": ["codex"],
                    "mode": "bypass",
                    "history": True,
                    "history_name": name,
                    "history_dir": history_dir,
                    "history_labels": {"graph": graph, "task": "python"},
                    "env": {"MOCK_STDOUT": HISTORY_TRACE},
                    "bins": {"codex": str(MOCK)},
                }
            )
            records.extend(await client.history({"session": name, "history_dir": history_dir}))
        watch = client.history_watch(
            {
                "after": records[0]["history_id"],
                "all_projects": True,
                "history_dir": history_dir,
                "labels": {"graph": "release", "task": "python"},
            }
        )
        envelope = await watch.__anext__()
        self.assertEqual(envelope["record"]["prompt"], "resumed record")
        self.assertEqual(envelope["record"]["history_id"], records[2]["history_id"])
        self.assertNotEqual(envelope["record"]["history_id"], records[0]["history_id"])
        await cast("Any", watch).aclose()

    async def test_history_label_precedence_crosses_the_cli_boundary(self) -> None:
        """Apply CLI labels over environment and project configuration."""
        directory = scratch(self, "labels")
        project = directory / "project"
        project.mkdir()
        (project / "oneharness.toml").write_text(
            'history_labels = { graph = "project", project = "kept" }\n', encoding="utf-8"
        )
        user_config = directory / "user.toml"
        user_config.write_text("", encoding="utf-8")
        history_dir = str(directory / "history")
        client = OneHarness(
            executable=str(BINARY),
            env={
                "ONEHARNESS_CONFIG": str(user_config),
                "ONEHARNESS_HISTORY_LABELS": "graph=environment,env=kept",
                "ONEHARNESS_NO_CONFIG": "0",
            },
        )
        await client.run(
            {
                "prompt": "label precedence",
                "cwd": str(project),
                "harnesses": ["codex"],
                "mode": "bypass",
                "history": True,
                "history_name": "label-precedence",
                "history_dir": history_dir,
                "history_labels": {"graph": "cli", "cli": "kept"},
                "env": {"MOCK_STDOUT": HISTORY_TRACE},
                "bins": {"codex": str(MOCK)},
            }
        )
        records = await client.history(
            {
                "session": "label-precedence",
                "all_projects": True,
                "history_dir": history_dir,
            }
        )
        self.assertEqual(
            records[0]["labels"],
            {"cli": "kept", "env": "kept", "graph": "cli", "project": "kept"},
        )

    async def test_missing_history_raises_the_typed_error(self) -> None:
        """Map both bounded lookups and missing watch cursors to one error type."""
        history_dir = str(scratch(self, "missing"))
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
        with self.assertRaisesRegex(ContractError, "invalid mock harness"):
            await client.run_mock("", {"prompt": "no harness"})
        with self.assertRaisesRegex(ContractError, "invalid oneharness gate options"):
            await client.gate(cast("Any", {"harness": "claude-code"}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness mock options"):
            await client.mock(cast("Any", {"event": "{}"}))
        with self.assertRaisesRegex(ContractError, "invalid oneharness interrupt options"):
            await client.interrupt(cast("Any", {}))

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
        directory = scratch(self, "path")
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

    def layered(self, project: Path) -> OneHarness:
        """Return a client that reads config files but only the ones under test.

        `no_config` cannot be used here — the verbs under test are about the
        layering — so the machine's own user config is displaced by an empty
        file instead, leaving the project layer as the only one with content.
        """
        empty = project / "user.toml"
        empty.write_text("", encoding="utf-8")
        return OneHarness(
            executable=str(BINARY),
            env={"ONEHARNESS_NO_CONFIG": "0", "ONEHARNESS_CONFIG": str(empty)},
        )

    async def test_config_reports_the_layered_values_and_their_sources(self) -> None:
        """Attribute a project-file value to the file it came from."""
        project = scratch(self, "config")
        (project / "oneharness.toml").write_text(
            'harnesses = ["codex"]\nmode = "bypass"\n', encoding="utf-8"
        )
        with without_ambient_overrides():
            report = await self.layered(project).config({"cwd": str(project)})
        self.assertEqual(report["harnesses"]["value"], ["codex"])
        self.assertIn("oneharness.toml", report["harnesses"]["source"])
        self.assertEqual(report["mode"]["value"], "bypass")

    async def test_sync_plans_then_writes_a_harness_policy_file(self) -> None:
        """Report what a check would change, write it, then change nothing."""
        project = scratch(self, "sync")
        (project / "oneharness.toml").write_text(
            'allowed_tools = ["Bash(echo:*)"]\n', encoding="utf-8"
        )
        client = self.layered(project)
        options: Any = {"cwd": str(project), "harnesses": ["claude-code"]}
        with without_ambient_overrides():
            # `--check` exits non-zero precisely because a file *would* change,
            # and the method has to surface that as the report rather than as a
            # raise.
            planned = await client.sync(cast("Any", {**options, "check": True}))
            self.assertEqual(self.claude(planned), "created")
            # Still `created` on the real write, which is what proves the check
            # reported the status a write would reach while writing nothing.
            self.assertEqual(self.claude(await client.sync(options)), "created")
            # Idempotent, so a second sync of the same policy changes nothing.
            self.assertEqual(self.claude(await client.sync(options)), "unchanged")

    def claude(self, report: Any) -> str:
        """Return the claude-code result's status from one sync report."""
        results = [item for item in report["results"] if item["harness"] == "claude-code"]
        return cast("str", results[0]["status"])

    def test_a_python_keyword_option_still_renders_its_cli_flag(self) -> None:
        """`sync --global` reaches argv through the `global_` public spelling.

        A TypedDict cannot declare a field called `global`, so the generator
        suffixes it; this is the proof the suffix is undone on the way out
        rather than becoming an option a caller can set and the CLI never sees.
        """
        parsed = _input("sync_options", {"global_": True}, "sync options")
        self.assertIn("--global", _capability_arguments("sync", parsed))

    async def test_a_suppression_waits_for_its_suppressor_to_render(self) -> None:
        """An option that carries nothing suppresses nothing.

        An `unless` encodes a clap conflict, and clap conflicts on a flag being
        *present*, so the test is whether the named option renders — not whether
        the value looks true. `{"all": True, "harnesses": []}` sends no
        `--harness`, so it must keep the `--all` that is the call's only
        selection; `{"system": ""}` does send `--system ""`, so it must suppress
        the `--system-file` clap refuses beside it.
        """
        client = self.client()
        registry = sorted(item["id"] for item in await client.list())
        everything = await client.run(
            {
                "prompt": "empty suppressor keeps --all",
                "all": True,
                "harnesses": [],
                "mode": "bypass",
                "print_command": True,
            }
        )
        self.assertTrue(everything["dry_run"])
        self.assertEqual(sorted(result["harness"] for result in everything["results"]), registry)

        prompt = "empty system still suppresses --system-file"
        empty_system = await client.run(
            {
                "prompt": prompt,
                "harnesses": ["codex"],
                "system": "",
                "system_file": str(Path(tempfile.gettempdir()) / "never-read.txt"),
                "mode": "bypass",
                "print_command": True,
            }
        )
        self.assertEqual(
            empty_system["results"][0]["command"],
            ["codex", "exec", "--dangerously-bypass-approvals-and-sandbox", "--json", prompt],
        )

    def test_a_preferred_pair_still_resolves_instead_of_refusing(self) -> None:
        """`{session, last}` is one request, and `--last` is what it means.

        The lookup union deliberately accepts both halves and defines them as
        "the most recent", so this is the pair the manifest annotates `prefer`.
        Refusing it the way a contradiction is refused would break the very
        lookup the suppression mechanism was written for.
        """
        parsed = _input("history_lookup", {"session": "older", "last": True}, "lookup")
        rendered = _capability_arguments("history", parsed)
        self.assertIn("--last", rendered)
        self.assertNotIn("older", rendered)

    async def test_a_contradiction_is_refused_rather_than_edited_out(self) -> None:
        """A caller who asked for two different things is told, not answered.

        `{"all": True, "harnesses": ["codex"]}` asks for every harness and for
        one harness at once. Suppression alone answers it by running `codex` — a
        paid turn on an identity nobody chose, reported as a success — so the
        manifest annotates the pair `refuse` and the call ends before a spawn.
        """
        client = self.client()
        with self.assertRaisesRegex(
            ContractError,
            r"invalid oneharness run options: `all` and `harnesses` are mutually exclusive",
        ):
            await client.run(
                {
                    "prompt": "never spawned",
                    "all": True,
                    "harnesses": ["codex"],
                    "mode": "bypass",
                    "print_command": True,
                }
            )

        # The same pair the empty-`system` suppression turns on, read from its
        # other side: two non-empty system sources are two answers to "what
        # instructs this turn", and suppression picks the file silently. Only the
        # stated choice refuses — `system: ""` still suppresses, unchanged.
        with self.assertRaisesRegex(
            ContractError,
            r"invalid oneharness run options: `system_file` and `system` are mutually "
            r"exclusive",
        ):
            await client.run(
                {
                    "prompt": "never spawned",
                    "harnesses": ["codex"],
                    "system": "be terse",
                    "system_file": str(Path(tempfile.gettempdir()) / "never-read.txt"),
                    "mode": "bypass",
                    "print_command": True,
                }
            )

        # Every refuse pair, not just the selectors: two answers to "which config
        # layers", "is this run recorded", "which project's store". The Python
        # spelling is the caller's own, not the manifest's camelCase.
        with self.assertRaisesRegex(
            ContractError, r"`config` and `no_config` are mutually exclusive"
        ):
            await client.run(
                {"prompt": "never spawned", "config": "oneharness.toml", "no_config": True}
            )
        with self.assertRaisesRegex(
            ContractError, r"`history` and `no_history` are mutually exclusive"
        ):
            await client.run({"prompt": "never spawned", "history": True, "no_history": True})
        with self.assertRaisesRegex(
            ContractError,
            r"invalid oneharness history list options: `project` and `all_projects` are "
            r"mutually exclusive",
        ):
            await client.history_list({"project": "somewhere", "all_projects": True})

        # The switch half is a value like any other: `False` renders nothing, so
        # there is no second answer and the call proceeds on the one it has.
        one = await client.run(
            {
                "prompt": "an unset switch contradicts nothing",
                "all": False,
                "harnesses": ["codex"],
                "mode": "bypass",
                "print_command": True,
            }
        )
        self.assertEqual([result["harness"] for result in one["results"]], ["codex"])

    async def test_init_scaffolds_a_config_and_refuses_to_clobber_it(self) -> None:
        """Write a starter file, then treat an existing one as a refusal."""
        project = scratch(self, "init")
        path = str(project / "oneharness.toml")
        client = self.client()
        self.assertEqual(await client.init({"path": path}), path)
        self.assertIn("run_mode", Path(path).read_text(encoding="utf-8"))

        with self.assertRaises(OneHarnessProcessError):
            await client.init({"path": path})
        self.assertEqual(await client.init({"path": path, "force": True}), path)

    async def test_usage_reports_an_honest_tier_for_an_unprobeable_binary(self) -> None:
        """Report a state rather than a fabricated headroom number."""
        # `oneharness-mock-harness` is a real executable answering no usage
        # protocol, so this drives the whole probe path.
        report = await self.client().usage(
            {
                "harnesses": ["claude-code"],
                "bins": {"claude-code": str(MOCK)},
                "timeout_seconds": 20,
            }
        )
        self.assertEqual(report["schema_version"], "0.1")
        claude = [item for item in report["identities"] if item["harness"] == "claude-code"]
        self.assertIn(claude[0]["availability"]["state"], {"known", "unknown", "unavailable"})

    async def test_gate_answers_a_hook_event_with_the_harness_verdict(self) -> None:
        """Deny a marked call and say an allowed one with silence."""
        client = self.client()
        blocked = await client.gate(
            {
                "harness": "claude-code",
                "deny_if_contains": "BLOCKED",
                "reason": "policy",
                "event": json.dumps(
                    {"tool_name": "Bash", "tool_input": {"command": "echo BLOCKED"}}
                ),
            }
        )
        self.assertIsNotNone(blocked)
        self.assertEqual(
            json.loads(cast("str", blocked))["hookSpecificOutput"]["permissionDecision"], "deny"
        )

        # An allowed call is said with silence, so `None` is the answer rather
        # than a missing one.
        allowed = await client.gate(
            {
                "harness": "claude-code",
                "deny_if_contains": "BLOCKED",
                "event": json.dumps({"tool_name": "Bash", "tool_input": {"command": "echo fine"}}),
            }
        )
        self.assertIsNone(allowed)

    async def test_mock_applies_a_ruleset_and_spies_the_original_call(self) -> None:
        """Deny through a rules file while recording the call as observed."""
        directory = scratch(self, "mock")
        rules = directory / "rules.json"
        spy = directory / "spy.jsonl"
        rules.write_text(
            json.dumps(
                {
                    "rules": [
                        {
                            "match": {"tool": "Bash", "input": {"command": {"contains": "secret"}}},
                            "action": {"deny": {"message": "no secrets"}},
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        verdict = await self.client().mock(
            {
                "harness": "claude-code",
                "rules": str(rules),
                "spy_file": str(spy),
                "event": json.dumps({"tool_name": "Bash", "tool_input": {"command": "cat secret"}}),
            }
        )
        self.assertEqual(
            json.loads(cast("str", verdict))["hookSpecificOutput"]["permissionDecision"], "deny"
        )
        self.assertIn("cat secret", spy.read_text(encoding="utf-8"))

    async def test_interrupt_refuses_a_session_no_run_is_serving(self) -> None:
        """Return the refusal frame instead of raising on a non-zero exit."""
        session_dir = str(scratch(self, "interrupt"))
        response = await self.client().interrupt(
            {"session": "no-such-session", "session_dir": session_dir}
        )
        self.assertIs(response["ok"], False)
        self.assertEqual(response["reason"], "not_running")

    async def test_history_clear_is_a_dry_run_until_confirmed(self) -> None:
        """Migrate, report what a clear would remove, then remove it."""
        history_dir = str(scratch(self, "clear"))
        client = self.client()
        await client.run(
            {
                "prompt": "history clear from python",
                "harnesses": ["codex"],
                "mode": "bypass",
                "history": True,
                "history_dir": history_dir,
                "env": {"MOCK_STDOUT": HISTORY_TRACE},
                "bins": {"codex": str(MOCK)},
            }
        )
        self.assertEqual(len(await client.history_list({"history_dir": history_dir})), 1)

        migrated = await client.history_migrate({"history_dir": history_dir})
        self.assertIsInstance(migrated["files_processed"], int)

        dry = await client.history_clear({"history_dir": history_dir})
        self.assertIs(dry["dry_run"], True)
        self.assertEqual(dry["would_remove"], 1)
        # Nothing was removed, which the session still being listed proves.
        self.assertEqual(len(await client.history_list({"history_dir": history_dir})), 1)

        cleared = await client.history_clear({"history_dir": history_dir, "yes": True})
        self.assertIs(cleared["dry_run"], False)
        self.assertEqual(cleared["removed"], 1)
        self.assertEqual(await client.history_list({"history_dir": history_dir}), [])

    async def test_detect_accepts_the_whole_verb_not_only_a_harness_list(self) -> None:
        """Reach the rest of `detect`'s flags through the options mapping."""
        detected = await self.client().detect({"all": True, "exclude": ["codex"]})
        identifiers = {item["id"] for item in detected}
        self.assertIn("claude-code", identifiers)
        self.assertNotIn("codex", identifiers)

    def test_every_capability_the_manifest_declares_has_a_method(self) -> None:
        """The client-side half of the coverage gate.

        `scripts/sdk-coverage.mjs` enforces this across both languages in
        `just check`; asserting it here too means a missing method fails the
        package's own suite rather than only a repo-level script.
        """
        client = self.client()
        for method in _CAPABILITIES:
            name = re.sub(r"(?<!^)(?=[A-Z])", "_", method).lower()
            self.assertTrue(callable(getattr(client, name, None)), f"{name} is missing")

    def test_every_bound_option_is_a_field_of_its_options_contract(self) -> None:
        """Reject a manifest binding no caller could ever set.

        One populated value per contract, so each validator is walked over every
        option the manifest binds rather than the handful a happy-path call
        happens to set — which is how a flag bound in Rust and absent from the
        Python contract is caught here rather than by a consumer.
        """
        for method, capability in _CAPABILITIES.items():
            root = capability["options"]
            if root is None:
                continue
            inverse = {camel: snake for snake, camel in INPUT_KEYS[root].items()}
            # Bound options, plus any the contract requires without binding —
            # `gate`/`mock` take their event on stdin rather than in argv.
            names = {binding["option"] for binding in capability["bindings"]}
            names.update(INPUT_KEYS[root][key] for key in SCHEMAS[root].get("required", []))
            populated = {inverse.get(name, name): POPULATED[name] for name in names}
            # Every option at once is a legal *document* but not a legal *call*:
            # the manifest annotates mutually exclusive pairs `refuse`, and a
            # caller setting both halves is the contradiction the client now
            # rejects. So the contract is still walked over everything, while the
            # argv walk drops the suppressed half of each refusing pair — the
            # request the CLI itself would accept.
            bound = {binding["option"]: binding for binding in capability["bindings"]}
            renderable = {
                inverse.get(name, name): POPULATED[name]
                for name in names
                if not _contradicts(bound, name)
            }
            with self.subTest(method=method):
                self.assertEqual(_validate(root, populated, "populated options"), populated)
                rendered = _capability_arguments(method, _input(root, renderable, "populated"))
                self.assertEqual(rendered[: len(capability["argv"])], capability["argv"])

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
        report = await client.run(
            {
                "prompt": "future CLI",
                "harnesses": ["claude-code"],
                "mode": "bypass",
                "bins": {"claude-code": str(MOCK)},
            }
        )
        self.assertEqual(report["future_output_field"], {"preserved": True})
        self.assertEqual(report["results"][0]["future_result_field"], 7)


if __name__ == "__main__":
    unittest.main()
