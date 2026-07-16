# oneharness-sdk

Async Python access to the `oneharness` engine, generated from and validated against the Rust-owned JSON Schema contracts. The distribution is `oneharness-sdk`, the import is `oneharness_sdk`, and every release depends on the exact same `oneharness-cli` version.

```python
import asyncio

from oneharness_sdk import OneHarness


async def main() -> None:
    oneharness = OneHarness()
    report = await oneharness.run(
        {"prompt": "Summarize this repository", "harnesses": ["codex"]}
    )
    print(report["results"][0]["text"])


asyncio.run(main())
```

The complete client surface is `run`, `run_stream`, `list`, `detect`, `history`, `history_list`, and `history_watch`. `run_stream` and `history_watch` are async iterators. Every envelope is validated before it is yielded; closing or cancelling an iterator terminates its subprocess.

```python
async for envelope in oneharness.run_stream(
    {"prompt": "Inspect this repository", "harnesses": ["codex"]}
):
    if envelope["type"] == "event" and envelope["event"]["name"] == "shell":
        break

async for envelope in oneharness.history_watch(
    {"labels": {"graph": "release"}, "after": last_history_id}
):
    print(envelope["record"]["history_id"])
```

Python input keys are snake case and strict: unknown fields and misspellings fail before the CLI starts. Output dictionaries preserve additive fields from newer compatible CLI versions. `history` and `history_watch` raise `HistoryNotFoundError` when a session, record, or cursor cannot be resolved.

