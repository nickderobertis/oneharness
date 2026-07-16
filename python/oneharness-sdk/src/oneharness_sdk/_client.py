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
from typing import Any, Optional, cast

from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator

from ._errors import ContractError, HistoryNotFoundError, OneHarnessProcessError
from ._generated_types import (
    Detection,
    HarnessInfo,
    HistoryListOptions,
    HistoryLookup,
    HistoryRecord,
    HistoryStreamEnvelope,
    HistoryWatchOptions,
    RunOptions,
    RunReport,
    RunStreamEnvelope,
)

_STREAM_LIMIT = 16 * 1024 * 1024
_INPUT_ROOTS = {
    "history_list_options",
    "history_lookup",
    "history_watch_options",
    "run_options",
}


def _load_json(name: str) -> dict[str, Any]:
    path = Path(__file__).with_name("_generated") / name
    return cast("dict[str, Any]", json.loads(path.read_text(encoding="utf-8")))


_SCHEMAS = _load_json("schemas.json")
_INPUT_KEYS = cast("dict[str, dict[str, str]]", _load_json("input-keys.json"))


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


def _many(args: list[str], flag: str, values: Optional[Sequence[str]]) -> None:
    for value in values or ():
        args.extend((flag, value))


def _run_arguments(options: Mapping[str, Any], *, stream: bool) -> list[str]:
    args = ["run", "--prompt", cast("str", options["prompt"]), "--compact"]
    _many(args, "--harness", cast("Optional[Sequence[str]]", options.get("harnesses")))
    _many(args, "--model", cast("Optional[Sequence[str]]", options.get("models")))
    scalar_flags = {
        "system": "--system",
        "reasoning": "--reasoning",
        "resume": "--resume",
        "session": "--session",
        "mode": "--mode",
        "historyName": "--history-name",
        "historyDir": "--history-dir",
    }
    for key, flag in scalar_flags.items():
        if key in options:
            args.extend((flag, cast("str", options[key])))
    if options.get("fork"):
        args.append("--fork")
    if "timeoutSeconds" in options:
        args.extend(("--timeout", str(options["timeoutSeconds"])))
    if options.get("events"):
        args.append("--events")
    if stream:
        args.append("--stream")
    if options.get("history"):
        args.append("--history")
    for key, value in cast("Mapping[str, str]", options.get("historyLabels", {})).items():
        args.extend(("--history-label", f"{key}={value}"))
    for key, value in cast("Mapping[str, str]", options.get("env", {})).items():
        args.extend(("--env", f"{key}={value}"))
    for key, value in cast("Mapping[str, str]", options.get("bins", {})).items():
        args.extend(("--bin", f"{key}={value}"))
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
        self, args: Sequence[str], cwd: Optional[str] = None
    ) -> asyncio.subprocess.Process:
        return await asyncio.create_subprocess_exec(
            *self._command(args),
            cwd=cwd,
            env={**os.environ, **self._env},
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            limit=_STREAM_LIMIT,
        )

    async def _invoke(
        self,
        args: Sequence[str],
        *,
        cwd: Optional[str] = None,
        accept_json_on_nonzero: bool = False,
        history: bool = False,
    ) -> Any:
        process = await self._spawn(args, cwd)
        try:
            stdout_bytes, stderr_bytes = await process.communicate()
        except BaseException:
            await _terminate(process)
            raise
        stderr = stderr_bytes.decode("utf-8", errors="replace")
        if process.returncode != 0 and not accept_json_on_nonzero:
            raise _process_error(process.returncode or 0, stderr, history=history)
        try:
            return json.loads(stdout_bytes)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            if process.returncode != 0:
                raise _process_error(process.returncode or 0, stderr, history=history) from error
            raise ContractError(f"oneharness returned invalid JSON: {error}") from error

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
        parsed = _input("run_options", options, "invalid oneharness run options")
        value = await self._invoke(
            _run_arguments(parsed, stream=False),
            cwd=cast("Optional[str]", parsed.get("cwd")),
            accept_json_on_nonzero=True,
        )
        return cast("RunReport", _validate("run_report", value, "invalid oneharness run contract"))

    def run_stream(self, options: RunOptions) -> AsyncIterator[RunStreamEnvelope]:
        """Yield validated action/result envelopes as the harness runs."""
        parsed = _input("run_options", options, "invalid oneharness run options")
        return self._stream(
            _run_arguments(parsed, stream=True),
            "run_stream_envelope",
            "invalid oneharness run stream contract",
            cwd=cast("Optional[str]", parsed.get("cwd")),
        )

    async def list(self) -> builtins.list[HarnessInfo]:
        """Return the validated harness registry."""
        value = _validate(
            "list_report",
            await self._invoke(("list", "--compact")),
            "invalid oneharness list contract",
        )
        return cast("builtins.list[HarnessInfo]", value["harnesses"])

    async def detect(self, harnesses: Sequence[str] = ()) -> builtins.list[Detection]:
        """Probe selected harness binaries."""
        if isinstance(harnesses, (str, bytes)) or not all(
            isinstance(harness, str) for harness in harnesses
        ):
            raise ContractError("invalid oneharness detect options: harnesses must be strings")
        args = ["detect", "--compact"]
        _many(args, "--harness", harnesses)
        value = _validate(
            "detect_report",
            await self._invoke(args),
            "invalid oneharness detect contract",
        )
        return cast("builtins.list[Detection]", value["detected"])

    async def history(self, lookup: HistoryLookup) -> builtins.list[HistoryRecord]:
        """Resolve one history record or session."""
        parsed = _input("history_lookup", lookup, "invalid oneharness history options")
        args = ["history", "show", "--compact"]
        if parsed.get("last") is True:
            args.append("--last")
        else:
            args.append(cast("str", parsed["session"]))
        if parsed.get("project"):
            args.extend(("--project", cast("str", parsed["project"])))
        if parsed.get("allProjects"):
            args.append("--all-projects")
        if parsed.get("historyDir"):
            args.extend(("--history-dir", cast("str", parsed["historyDir"])))
        value = await self._invoke(args, history=True)
        return cast(
            "builtins.list[HistoryRecord]",
            _validate("history_records", value, "invalid oneharness history contract"),
        )

    async def history_list(
        self, options: Optional[HistoryListOptions] = None
    ) -> builtins.list[dict[str, Any]]:
        """List standardized history sessions."""
        parsed = _input(
            "history_list_options", options or {}, "invalid oneharness history list options"
        )
        args = ["history", "list", "--compact"]
        if parsed.get("project"):
            args.extend(("--project", cast("str", parsed["project"])))
        if parsed.get("allProjects"):
            args.append("--all-projects")
        if parsed.get("historyDir"):
            args.extend(("--history-dir", cast("str", parsed["historyDir"])))
        value = await self._invoke(args)
        return cast(
            "builtins.list[dict[str, Any]]",
            _validate("history_list", value, "invalid oneharness history list contract"),
        )

    def history_watch(
        self, options: Optional[HistoryWatchOptions] = None
    ) -> AsyncIterator[HistoryStreamEnvelope]:
        """Follow validated standardized history records."""
        parsed = _input(
            "history_watch_options", options or {}, "invalid oneharness history watch options"
        )
        args = ["history", "watch", "--format", "jsonl"]
        if parsed.get("after"):
            args.extend(("--after", cast("str", parsed["after"])))
        for key, value in cast("Mapping[str, str]", parsed.get("labels", {})).items():
            args.extend(("--label", f"{key}={value}"))
        if parsed.get("project"):
            args.extend(("--project", cast("str", parsed["project"])))
        if parsed.get("allProjects"):
            args.append("--all-projects")
        if parsed.get("historyDir"):
            args.extend(("--history-dir", cast("str", parsed["historyDir"])))
        return self._stream(
            args,
            "history_stream_envelope",
            "invalid oneharness history watch contract",
            history=True,
        )
