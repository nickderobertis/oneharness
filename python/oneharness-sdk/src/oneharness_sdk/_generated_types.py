"""Generated from oneharness-core. Do not edit."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any, TypedDict


class _RunOptionsOptional(TypedDict, total=False):
    all: bool
    batch_prompts: Sequence[str]
    batch_strategy: str
    bins: dict[str, str]
    config: str
    control: bool
    cwd: str
    env: dict[str, str]
    events: bool
    exclude: Sequence[str]
    fork: bool
    harnesses: Sequence[str]
    history: bool
    history_dir: str
    history_labels: dict[str, str]
    history_name: str
    max_parallel: int
    mock_harnesses: Sequence[str]
    mock_rules: str
    mode: str
    models: Sequence[str]
    no_config: bool
    no_history: bool
    output_dir: str
    output_format: str
    passthrough: Sequence[str]
    permit_prompts: bool
    print_command: bool
    prompt_files: Sequence[str]
    reasoning: str
    require_available: bool
    resume: str
    run_mode: str
    schema: str
    schema_max_retries: int
    session: str
    session_dir: str
    spy_file: str
    system: str
    system_file: str
    timeout_seconds: int


class RunOptions(_RunOptionsOptional):
    prompt: str


class _HistoryListOptionsOptional(TypedDict, total=False):
    all_projects: bool
    config: str
    history_dir: str
    no_config: bool
    project: str
    variant: str


class HistoryListOptions(_HistoryListOptionsOptional):
    pass


class _HistoryWatchOptionsOptional(TypedDict, total=False):
    after: str
    all_projects: bool
    config: str
    events: bool
    history_dir: str
    labels: dict[str, str]
    no_config: bool
    project: str
    variant: str


class HistoryWatchOptions(_HistoryWatchOptionsOptional):
    pass


class _DetectOptionsOptional(TypedDict, total=False):
    all: bool
    bins: dict[str, str]
    config: str
    exclude: Sequence[str]
    harnesses: Sequence[str]
    no_config: bool
    require_available: bool


class DetectOptions(_DetectOptionsOptional):
    pass


class _ConfigOptionsOptional(TypedDict, total=False):
    config: str
    cwd: str
    no_config: bool


class ConfigOptions(_ConfigOptionsOptional):
    pass


class _SyncOptionsOptional(TypedDict, total=False):
    check: bool
    config: str
    cwd: str
    global: bool
    harnesses: Sequence[str]
    no_config: bool


class SyncOptions(_SyncOptionsOptional):
    pass


class _InitOptionsOptional(TypedDict, total=False):
    force: bool
    path: str


class InitOptions(_InitOptionsOptional):
    pass


class _UsageOptionsOptional(TypedDict, total=False):
    all: bool
    bins: dict[str, str]
    config: str
    cwd: str
    exclude: Sequence[str]
    harnesses: Sequence[str]
    no_config: bool
    timeout_seconds: int


class UsageOptions(_UsageOptionsOptional):
    pass


class _GateOptionsOptional(TypedDict, total=False):
    deny_if_contains: str
    reason: str


class GateOptions(_GateOptionsOptional):
    event: str
    harness: str


class _MockOptionsOptional(TypedDict, total=False):
    rules: str
    spy_file: str


class MockOptions(_MockOptionsOptional):
    event: str
    harness: str


class _InterruptOptionsOptional(TypedDict, total=False):
    cwd: str
    input: str
    session_dir: str


class InterruptOptions(_InterruptOptionsOptional):
    session: str


class _HistoryClearOptionsOptional(TypedDict, total=False):
    all_projects: bool
    config: str
    history_dir: str
    no_config: bool
    project: str
    yes: bool


class HistoryClearOptions(_HistoryClearOptionsOptional):
    pass


class _HistoryMigrateOptionsOptional(TypedDict, total=False):
    config: str
    history_dir: str
    no_config: bool


class HistoryMigrateOptions(_HistoryMigrateOptionsOptional):
    pass


HistoryLookup = dict[str, Any]
RunReport = dict[str, Any]
RunStreamEnvelope = dict[str, Any]
HistoryRecord = dict[str, Any]
HistoryLine = dict[str, Any]
HistoryStreamEnvelope = dict[str, Any]
HarnessInfo = dict[str, Any]
Detection = dict[str, Any]
ConfigReport = dict[str, Any]
SyncReport = dict[str, Any]
UsageReport = dict[str, Any]
InterruptResponse = dict[str, Any]
HistoryClearReport = dict[str, Any]
HistoryMigrateReport = dict[str, Any]
