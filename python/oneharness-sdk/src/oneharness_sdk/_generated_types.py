"""Generated from oneharness-core. Do not edit."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any, TypedDict


class _RunOptionsOptional(TypedDict, total=False):
    bins: dict[str, str]
    cwd: str
    env: dict[str, str]
    events: bool
    fork: bool
    harnesses: Sequence[str]
    history: bool
    history_dir: str
    history_labels: dict[str, str]
    history_name: str
    mode: str
    models: Sequence[str]
    reasoning: str
    resume: str
    session: str
    system: str
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
    history_dir: str
    labels: dict[str, str]
    project: str


class HistoryWatchOptions(_HistoryWatchOptionsOptional):
    pass


HistoryLookup = dict[str, Any]
RunReport = dict[str, Any]
RunStreamEnvelope = dict[str, Any]
HistoryRecord = dict[str, Any]
HistoryStreamEnvelope = dict[str, Any]
HarnessInfo = dict[str, Any]
Detection = dict[str, Any]
