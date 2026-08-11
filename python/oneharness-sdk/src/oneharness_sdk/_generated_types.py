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
    history_dir: str
    project: str


class HistoryListOptions(_HistoryListOptionsOptional):
    pass


class _HistoryWatchOptionsOptional(TypedDict, total=False):
    after: str
    all_projects: bool
    events: bool
    history_dir: str
    labels: dict[str, str]
    project: str


class HistoryWatchOptions(_HistoryWatchOptionsOptional):
    pass


HistoryLookup = dict[str, Any]
RunReport = dict[str, Any]
RunStreamEnvelope = dict[str, Any]
HistoryRecord = dict[str, Any]
HistoryLine = dict[str, Any]
HistoryStreamEnvelope = dict[str, Any]
HarnessInfo = dict[str, Any]
Detection = dict[str, Any]
