"""Async subprocess client for the oneharness JSON and JSONL interfaces."""

from __future__ import annotations

import asyncio
import builtins
import json
import os
import shutil
from collections.abc import AsyncIterator, Mapping, Sequence
from functools import cache
from pathlib import Path
from typing import Any, Optional, TypedDict, cast

from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator

from ._errors import ContractError, HistoryNotFoundError, OneHarnessProcessError
from ._generated_types import (
    ConfigOptions,
    ConfigReport,
    Detection,
    DetectOptions,
    GateOptions,
    HarnessInfo,
    HistoryClearOptions,
    HistoryClearReport,
    HistoryListOptions,
    HistoryLookup,
    HistoryMigrateOptions,
    HistoryMigrateReport,
    HistoryRecord,
    HistoryStreamEnvelope,
    HistoryWatchOptions,
    InitOptions,
    InterruptOptions,
    InterruptResponse,
    MockOptions,
    RunOptions,
    RunReport,
    RunStreamEnvelope,
    SyncOptions,
    SyncReport,
    UsageOptions,
    UsageReport,
)

_STREAM_LIMIT = 16 * 1024 * 1024


class MockHarnessScript(TypedDict, total=False):
    """Script for the shipped deterministic provider substitute."""

    stdout: str
    stderr: str
    exit_code: int
    latency_ms: int


def _load_json(name: str) -> dict[str, Any]:
    path = Path(__file__).with_name("_generated") / name
    return cast("dict[str, Any]", json.loads(path.read_text(encoding="utf-8")))


_SCHEMAS = _load_json("schemas.json")
_INPUT_KEYS = cast("dict[str, dict[str, str]]", _load_json("input-keys.json"))
_CAPABILITIES = _load_json("capabilities.json")
# The option contracts, taken from the generated key map rather than restated:
# a root is an input exactly when the generator emitted a spelling table for it.
_INPUT_ROOTS = frozenset(_INPUT_KEYS)


@cache
def _validator(root: str) -> Validator:
    schema = _SCHEMAS[root]
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def _validate(root: str, value: Any, label: str) -> Any:
    errors = sorted(_validator(root).iter_errors(value), key=lambda error: list(error.path))
    if not errors:
        return value
    details = []
    for error in errors:
        path = ".".join(str(part) for part in error.absolute_path) or "<root>"
        details.append(f"{path}: {error.message}")
    raise ContractError(f"{label}: {'; '.join(details)}")


def _input(root: str, value: Any, label: str) -> dict[str, Any]:
    if root not in _INPUT_ROOTS:  # pragma: no cover - internal programming guard
        raise AssertionError(f"{root} is not an input schema")
    checked = cast("Mapping[str, Any]", _validate(root, value, label))
    keys = _INPUT_KEYS[root]
    return {keys.get(key, key): item for key, item in checked.items()}


def _text(value: Any) -> str:
    """Render one scalar the way the CLI reads it back off the command line."""
    return json.dumps(value) if isinstance(value, bool) else str(value)


def _renders_argument(binding: Mapping[str, Any], value: Any) -> bool:
    """Does this binding put anything on the argv for ``value``?

    This is the question ``unless`` asks, and asking it any other way is a bug:
    the conflict a suppression encodes is clap's, and clap conflicts on a flag
    being *present*. Truthiness is not that test — ``{"system": ""}`` still
    sends ``--system ""``, so it must suppress the ``--system-file`` clap
    refuses beside it, and ``{"all": True, "harnesses": []}`` sends no
    ``--harness``, so it must keep ``--all``.
    """
    if value is None:
        return False
    kind = binding["kind"]
    if kind in ("repeated", "trailing", "key-value"):
        return len(value) > 0
    if kind == "switch":
        return bool(value)
    # A `positional` or `value` binding renders whatever it carries, an empty
    # string included — which is a present flag to clap.
    return True


def _states_a_choice(binding: Mapping[str, Any], value: Any) -> bool:
    """Does this value assert something, or merely occupy the key?

    Suppression asks whether a binding renders; refusing asks something narrower,
    because a contradiction takes two positive answers. An empty value is not
    one. ``{"system": ""}`` does render ``--system ""`` — which is why it still
    suppresses the ``--system-file`` clap refuses beside it — but an empty system
    prompt asks for nothing, so a file beside it is a defaulted key next to a
    real choice rather than a caller asking for two different things.

    Both parameters stay dynamic because both are deserialization boundaries:
    ``binding`` is one row of ``capabilities.json`` exactly as it was read off
    disk, and ``value`` is whatever the caller passed for that option — a
    ``bool``, ``str``, ``list`` or ``dict`` depending on the row's own ``kind``,
    which is the only thing that says which one to expect.
    """
    if not _renders_argument(binding, value):
        return False
    if binding["kind"] in ("value", "positional"):
        return _text(value) != ""
    return True


def _caller_spelling(method: str, option: str) -> str:
    """Name one bound option the way this SDK's caller wrote it.

    The manifest binds camelCase and `_input` has already renamed the keys, so a
    message built from it would name `system_file` as `systemFile` — an option
    the Python caller cannot find in their own call.
    """
    root = _CAPABILITIES[method]["options"]
    keys = _INPUT_KEYS.get(root, {}) if root else {}
    return next((python for python, wire in keys.items() if wire == option), option)


def _refuse_contradiction(method: str, option: str, unless: str) -> ContractError:
    """Refuse a call that asked for both halves of a mutually exclusive pair."""
    verb = " ".join(_CAPABILITIES[method]["argv"])
    first, second = _caller_spelling(method, option), _caller_spelling(method, unless)
    return ContractError(
        f"invalid oneharness {verb} options: `{first}` and `{second}` are mutually "
        f"exclusive, and this call sets both to values the CLI would send. Pass only "
        f"the one you mean — resolving it here would run the call as something you "
        f"did not ask for."
    )


def _capability_arguments(method: str, options: Mapping[str, Any]) -> list[str]:
    """Render one capability's argv from its declared bindings.

    The client names no flags of its own. `capabilities.json` — the same manifest
    the Rust gates hold to clap and to the option contracts — says which option
    renders which flag and how, so a flag reaches every method as soon as it is
    bound, and a method cannot quietly omit one.
    """
    capability = _CAPABILITIES[method]
    args: list[str] = [*capability["argv"], *capability["always"]]
    positional: list[str] = []
    trailing: list[str] = []
    # `a_suppressing_option_is_one_the_same_capability_binds` proves in Rust that
    # an `unless` always resolves here, so a missing entry is unreachable rather
    # than a case to render through.
    bound = {binding["option"]: binding for binding in capability["bindings"]}
    for binding in capability["bindings"]:
        unless = binding["unless"]
        if unless is not None and _renders_argument(bound[unless], options.get(unless)):
            # What both halves asserting means is the manifest's question, not
            # this client's: `prefer` is a request with one meaning
            # (`{session, last: True}` is "the most recent"), `refuse` is a caller
            # who asked for two different things. Editing the second one out picks
            # an answer silently, which is how a turn gets billed to a harness
            # nobody selected. So `prefer` is the only waiver, and everything
            # else refuses: an absent annotation (a manifest older than the key)
            # and a resolution added after this client shipped both read as the
            # recoverable half, because a refusal the manifest meant to resolve
            # is a message the caller can act on and the reverse is not.
            resolution = binding.get("unless_resolution", "refuse")
            if resolution != "prefer" and _states_a_choice(
                binding, options.get(binding["option"])
            ) and _states_a_choice(bound[unless], options.get(unless)):
                raise _refuse_contradiction(method, binding["option"], unless)
            continue
        value = options.get(binding["option"])
        if value is None:
            continue
        kind, flag = binding["kind"], binding["flag"]
        if kind == "positional":
            positional.append(_text(value))
        elif kind == "value":
            args.extend((flag, _text(value)))
        elif kind == "repeated":
            args.extend(part for item in value for part in (flag, _text(item)))
        elif kind == "switch":
            if value:
                args.append(flag)
        elif kind == "key-value":
            for key, item in cast("Mapping[str, Any]", value).items():
                args.extend((flag, f"{key}={_text(item)}"))
        else:  # trailing
            trailing.extend(_text(item) for item in value)
    args.extend(positional)
    # After a `--` separator, so a passthrough argument that looks like a flag
    # reaches the harness instead of being read by oneharness.
    if trailing:
        args.extend(("--", *trailing))
    return args


async def _terminate(process: asyncio.subprocess.Process) -> None:
    if process.returncode is not None:
        return
    try:
        process.terminate()
    except ProcessLookupError:  # pragma: no cover - OS race after returncode check
        return
    try:
        await asyncio.wait_for(process.wait(), timeout=2)
    except asyncio.TimeoutError:  # pragma: no cover - defensive hard-kill fallback
        process.kill()
        await process.wait()


def _process_error(returncode: int, stderr: str, *, history: bool) -> OneHarnessProcessError:
    if history and (returncode == 1 or "was not found" in stderr):
        return HistoryNotFoundError(returncode, stderr)
    return OneHarnessProcessError(returncode, stderr)


class OneHarness:
    """Validated async access to an installed oneharness CLI."""

    def __init__(
        self,
        *,
        executable: Optional[str] = None,
        executable_args: Sequence[str] = (),
        env: Optional[Mapping[str, str]] = None,
    ) -> None:
        self._executable = executable
        self._executable_args = tuple(executable_args)
        self._env = dict(env or {})

    def _command(self, args: Sequence[str]) -> tuple[str, ...]:
        command = self._executable or os.environ.get("ONEHARNESS_BIN")
        if command is None:
            command = shutil.which("oneharness") or "oneharness"
        return (command, *self._executable_args, *args)

    async def _spawn(
        self,
        args: Sequence[str],
        cwd: Optional[str] = None,
        stdin: Optional[str] = None,
    ) -> asyncio.subprocess.Process:
        return await asyncio.create_subprocess_exec(
            *self._command(args),
            cwd=cwd,
            env={**os.environ, **self._env},
            stdin=asyncio.subprocess.PIPE if stdin is not None else None,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            limit=_STREAM_LIMIT,
        )

    async def _output(
        self,
        args: Sequence[str],
        *,
        cwd: Optional[str] = None,
        stdin: Optional[str] = None,
    ) -> tuple[int, bytes, str]:
        """Run one command to completion and return its exit code and streams."""
        process = await self._spawn(args, cwd, stdin)
        try:
            stdout_bytes, stderr_bytes = await process.communicate(
                None if stdin is None else stdin.encode("utf-8")
            )
        except BaseException:
            await _terminate(process)
            raise
        return (
            process.returncode or 0,
            stdout_bytes,
            stderr_bytes.decode("utf-8", errors="replace"),
        )

    async def _invoke(
        self,
        args: Sequence[str],
        *,
        cwd: Optional[str] = None,
        accept_json_on_nonzero: bool = False,
        history: bool = False,
    ) -> Any:
        returncode, stdout_bytes, stderr = await self._output(args, cwd=cwd)
        if returncode != 0 and not accept_json_on_nonzero:
            raise _process_error(returncode, stderr, history=history)
        try:
            return json.loads(stdout_bytes)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            if returncode != 0:
                raise _process_error(returncode, stderr, history=history) from error
            raise ContractError(f"oneharness returned invalid JSON: {error}") from error

    async def _invoke_text(
        self,
        args: Sequence[str],
        *,
        stdin: Optional[str] = None,
    ) -> str:
        """Run one command whose stdout is prose rather than a contract."""
        returncode, stdout_bytes, stderr = await self._output(args, stdin=stdin)
        if returncode != 0:
            raise _process_error(returncode, stderr, history=False)
        return stdout_bytes.decode("utf-8", errors="replace")

    async def _call(
        self,
        method: str,
        options: Any,
        options_root: str,
        output_root: str,
        *,
        accept_json_on_nonzero: bool = False,
        history: bool = False,
        options_label: Optional[str] = None,
        contract_label: Optional[str] = None,
    ) -> Any:
        """Validate options, render the capability's argv, invoke, validate stdout.

        Every JSON-returning method is this call with two contracts, so none of
        them can drift from the manifest in how it builds a command or in what
        it promises back.
        """
        verb = " ".join(_CAPABILITIES[method]["argv"])
        parsed = _input(
            options_root, options, options_label or f"invalid oneharness {verb} options"
        )
        value = await self._invoke(
            _capability_arguments(method, parsed),
            cwd=cast("Optional[str]", parsed.get("cwd")),
            accept_json_on_nonzero=accept_json_on_nonzero,
            history=history,
        )
        return _validate(
            output_root, value, contract_label or f"invalid oneharness {verb} contract"
        )

    async def _stream(
        self,
        args: Sequence[str],
        root: str,
        label: str,
        *,
        cwd: Optional[str] = None,
        history: bool = False,
    ) -> AsyncIterator[dict[str, Any]]:
        process = await self._spawn(args, cwd)
        if process.stdout is None or process.stderr is None:  # pragma: no cover - PIPE invariant
            await _terminate(process)
            raise RuntimeError("oneharness stream pipes were not created")
        stderr_task = asyncio.create_task(process.stderr.read())
        try:
            while line := await process.stdout.readline():
                if not line.strip():
                    continue
                try:
                    value = json.loads(line)
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise ContractError(f"{label}: invalid JSON: {error}") from error
                yield cast("dict[str, Any]", _validate(root, value, label))
            returncode = await process.wait()
            stderr = (await stderr_task).decode("utf-8", errors="replace")
            if returncode != 0:
                raise _process_error(returncode, stderr, history=history)
        finally:
            await _terminate(process)
            if not stderr_task.done():
                stderr_task.cancel()
            await asyncio.gather(stderr_task, return_exceptions=True)

    async def run(self, options: RunOptions) -> RunReport:
        """Run one prompt and return a validated report."""
        # `accept_json_on_nonzero`, because a harness that fails is data in the
        # report rather than a failure of the call: the CLI still prints a whole
        # contract and exits non-zero to say some result was not `ok`.
        return cast(
            "RunReport",
            await self._call(
                "run",
                options,
                "run_options",
                "run_report",
                accept_json_on_nonzero=True,
            ),
        )

    async def run_mock(
        self,
        harness: str,
        options: RunOptions,
        script: Optional[MockHarnessScript] = None,
    ) -> RunReport:
        """Run against oneharness's deterministic, no-model harness responder."""
        if not harness:
            raise ContractError("invalid mock harness: harness must not be empty")
        parsed = _input("run_options", options, "invalid oneharness run options")
        scripted = dict(script or {})
        env = dict(cast("Mapping[str, str]", parsed.get("env", {})))
        mappings = {
            "stdout": "MOCK_STDOUT",
            "stderr": "MOCK_STDERR",
            "exit_code": "MOCK_EXIT",
            "latency_ms": "MOCK_SLEEP_MS",
        }
        for key, variable in mappings.items():
            if key in scripted:
                env[variable] = str(scripted[key])
        parsed = {**parsed, "harnesses": [harness], "mockHarnesses": [harness], "env": env}
        value = await self._invoke(
            _capability_arguments("run", parsed),
            cwd=cast("Optional[str]", parsed.get("cwd")),
            accept_json_on_nonzero=True,
        )
        return cast("RunReport", _validate("run_report", value, "invalid oneharness run contract"))

    def run_stream(self, options: RunOptions) -> AsyncIterator[RunStreamEnvelope]:
        """Yield validated action/result envelopes as the harness runs."""
        parsed = _input("run_options", options, "invalid oneharness run options")
        return self._stream(
            _capability_arguments("runStream", parsed),
            "run_stream_envelope",
            "invalid oneharness run stream contract",
            cwd=cast("Optional[str]", parsed.get("cwd")),
        )

    async def list(self) -> builtins.list[HarnessInfo]:
        """Return the validated harness registry."""
        value = _validate(
            "list_report",
            await self._invoke(_capability_arguments("list", {})),
            "invalid oneharness list contract",
        )
        return cast("builtins.list[HarnessInfo]", value["harnesses"])

    async def detect(
        self, harnesses_or_options: Sequence[str] | DetectOptions = ()
    ) -> builtins.list[Detection]:
        """Probe harness binaries.

        The sequence form is the long-standing shape and still means "these
        harnesses"; the options mapping is what reaches the rest of the verb's
        flags (`--all`, `--exclude`, `--bin`, the config layer).
        """
        if isinstance(harnesses_or_options, Mapping):
            options = harnesses_or_options
        elif isinstance(harnesses_or_options, (str, bytes)) or not all(
            isinstance(harness, str) for harness in harnesses_or_options
        ):
            raise ContractError("invalid oneharness detect options: harnesses must be strings")
        else:
            options = cast("DetectOptions", {"harnesses": list(harnesses_or_options)})
        report = await self._call("detect", options, "detect_options", "detect_report")
        return cast("builtins.list[Detection]", report["detected"])

    async def config(self, options: Optional[ConfigOptions] = None) -> ConfigReport:
        """Return the effective layered configuration and each value's source."""
        return cast(
            "ConfigReport",
            await self._call("config", options or {}, "config_options", "config_report"),
        )

    async def sync(self, options: Optional[SyncOptions] = None) -> SyncReport:
        """Merge the unified policy into each harness's own configuration file."""
        # `--check` exits non-zero when a file would change, which is the answer
        # rather than a failure, and the report says which files those are.
        return cast(
            "SyncReport",
            await self._call(
                "sync",
                options or {},
                "sync_options",
                "sync_report",
                accept_json_on_nonzero=True,
            ),
        )

    async def usage(self, options: Optional[UsageOptions] = None) -> UsageReport:
        """Report subscription headroom per harness identity, before spending it."""
        return cast(
            "UsageReport",
            await self._call("usage", options or {}, "usage_options", "usage_report"),
        )

    async def init(self, options: Optional[InitOptions] = None) -> str:
        """Write a starter ``oneharness.toml`` and return where it landed.

        One of the verbs whose stdout is a human confirmation line rather than a
        JSON contract — the deliverable is the file — so the return is the
        resolved path the caller asked for, not a parsed document.
        """
        parsed = _input("init_options", options or {}, "invalid oneharness init options")
        await self._invoke_text(_capability_arguments("init", parsed))
        return cast("str", parsed.get("path") or "oneharness.toml")

    async def gate(self, options: GateOptions) -> Optional[str]:
        """Run the pre-tool gate over one harness hook event.

        Returns the harness's native deny verdict, or ``None`` when the call is
        allowed through — which the CLI says by printing nothing at all, so an
        empty answer is the allow rather than a missing one.
        """
        parsed = _input("gate_options", options, "invalid oneharness gate options")
        verdict = await self._invoke_text(
            _capability_arguments("gate", parsed),
            stdin=cast("str", parsed["event"]),
        )
        return verdict if verdict.strip() else None

    async def mock(self, options: MockOptions) -> Optional[str]:
        """Apply a mock ruleset to one hook event; the read-write sibling of ``gate``."""
        parsed = _input("mock_options", options, "invalid oneharness mock options")
        verdict = await self._invoke_text(
            _capability_arguments("mock", parsed),
            stdin=cast("str", parsed["event"]),
        )
        return verdict if verdict.strip() else None

    async def interrupt(self, options: InterruptOptions) -> InterruptResponse:
        """Abort a controlled session's in-flight turn, optionally redirecting it.

        A refusal is an answer, not a raise: the response says whether the abort
        was served and, when it was not, why — which is what a supervisor
        branches on. So a non-zero exit still yields the frame.
        """
        return cast(
            "InterruptResponse",
            await self._call(
                "interrupt",
                options,
                "interrupt_options",
                "interrupt_response",
                accept_json_on_nonzero=True,
            ),
        )

    async def history(self, lookup: HistoryLookup) -> builtins.list[HistoryRecord]:
        """Resolve one history record or session."""
        # The `--last`-suppresses-a-name rule is declared, not re-derived here:
        # the union deliberately accepts `{session, last: True}` and resolves it
        # to "the most recent", and the manifest binds `session` with
        # `unless: "last"` so the builder renders only `--last`.
        return cast(
            "builtins.list[HistoryRecord]",
            await self._call(
                "history",
                lookup,
                "history_lookup",
                "history_records",
                history=True,
                options_label="invalid oneharness history options",
                contract_label="invalid oneharness history contract",
            ),
        )

    async def history_list(
        self, options: Optional[HistoryListOptions] = None
    ) -> builtins.list[dict[str, Any]]:
        """List standardized history sessions."""
        return cast(
            "builtins.list[dict[str, Any]]",
            await self._call(
                "historyList",
                options or {},
                "history_list_options",
                "history_list",
                options_label="invalid oneharness history list options",
                contract_label="invalid oneharness history list contract",
            ),
        )

    async def history_clear(
        self, options: Optional[HistoryClearOptions] = None
    ) -> HistoryClearReport:
        """Remove history sessions.

        A dry run until ``yes`` is set, and the two answers are different
        documents: ``dry_run`` discriminates them, so a caller reads ``removed``
        only from a run that removed something.
        """
        return cast(
            "HistoryClearReport",
            await self._call(
                "historyClear",
                options or {},
                "history_clear_options",
                "history_clear_report",
            ),
        )

    async def history_migrate(
        self, options: Optional[HistoryMigrateOptions] = None
    ) -> HistoryMigrateReport:
        """Rewrite legacy session files to the current record version."""
        return cast(
            "HistoryMigrateReport",
            await self._call(
                "historyMigrate",
                options or {},
                "history_migrate_options",
                "history_migrate_report",
            ),
        )

    def history_watch(
        self, options: Optional[HistoryWatchOptions] = None
    ) -> AsyncIterator[HistoryStreamEnvelope]:
        """Follow validated standardized history records."""
        parsed = _input(
            "history_watch_options", options or {}, "invalid oneharness history watch options"
        )
        return self._stream(
            _capability_arguments("historyWatch", parsed),
            "history_stream_envelope",
            "invalid oneharness history watch contract",
            history=True,
        )
