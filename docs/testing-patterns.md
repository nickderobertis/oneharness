# Testing patterns

## Replace only the paid provider

Use the deterministic harness when a test needs the real oneharness CLI, adapter
argv, subprocess handling, parsing, and SDK contract but not a model's judgment.
It is fast, offline, and deterministic. It differs from `oneharness mock` and
`run --mock-rules`, which mock tool calls while still using a real model.

The responder ships inside the main executable as `oneharness mock-harness`.
`run --mock-harness ID` selects it and supplies adapter argv automatically:

```sh
MOCK_STDOUT='{"result":"fixed answer"}' \
  oneharness run --harness claude-code --mock-harness claude-code \
  --prompt 'ignored by the fake provider' --compact
```

```python
from oneharness_sdk import OneHarness

report = await OneHarness().run_mock(
    "claude-code", {"prompt": "deterministic", "mode": "bypass"},
    {"stdout": '{"result":"fixed answer"}', "exit_code": 0, "latency_ms": 5},
)
```

```ts
import { OneHarness } from "@oneharness/sdk";

const report = await new OneHarness().runMock(
  "claude-code", { prompt: "deterministic", mode: "bypass" },
  { stdout: '{"result":"fixed answer"}', exitCode: 0, latencyMs: 5 },
);
```

### Environment contract

| Variable | Behavior |
| --- | --- |
| `MOCK_STDOUT` | Exact stdout bytes; defaults to `{"result":"mock ok"}`. |
| `MOCK_STDERR` | Exact stderr bytes. |
| `MOCK_EXIT` | Exit code; defaults to `0`. |
| `MOCK_SLEEP_MS` | Delay before output/exit, in milliseconds. |
| `MOCK_ARGV_FILE` | Write argv, one argument per line, to this path. |
| `MOCK_ECHO_PWD` | Emit `PWD=<inherited PWD>` and exit. |
| `MOCK_ECHO_ENV` | Emit the named environment variable and value, then exit. |
| `MOCK_CAT_FILE` | Emit the current contents of this file and exit. |
| `MOCK_CAT_ARG_AFTER` | Find this argv flag, emit the file named by its next argument, and exit. |
| `MOCK_ECHO_STDIN` | Copy stdin verbatim to stdout and exit. |
| `MOCK_ATTEMPT_FILE` | Increment a counter file and prefer `MOCK_STDOUT_<n>` for that 1-based attempt. |
| `MOCK_STDOUT_<n>` | Attempt-specific stdout used with `MOCK_ATTEMPT_FILE`. |
| `MOCK_STREAM_DELAY_MS` | Emit one line at a time with this delay; in chunk mode, delay between chunks. |
| `MOCK_STREAM_CHUNK_BYTES` | Emit stdout in fixed-size byte chunks; must be positive. |
| `MOCK_FAIL_IF_MODEL` | Exit `1` when argv selects this model. |
| `MOCK_FAIL_STDERR` | Error for `MOCK_FAIL_IF_MODEL`; defaults to model-not-found text. |
| `MOCK_LOG_FILE` | Append `S`/`E` scheduling lines and `COMPLETE` after a completed stream. |
| `MOCK_NATIVE_GRANDCHILD_MS` | Launch a native descendant for this many milliseconds. |
| `MOCK_TICK_FILE` | In native-descendant mode, append one byte per tick. |

The SDK helpers cover stdout, stderr, exit, and latency. Put advanced variables
in the run options' `env` map; explicit helper script values take precedence.

`MOCK_ATTEMPT_FILE` sequences invocations across process boundaries: each new
mock responder reads and increments the shared file before choosing
`MOCK_STDOUT_1`, `MOCK_STDOUT_2`, and so on. This lets a sequential multi-party
test give an agent and its judge different adapter-shaped responses even when
both inherit one process-wide environment. Give both oneharness invocations the
same counter path and numbered responses; the first can select one harness with
`--mock-harness` and the second another. The SDKs expose the same mechanism
through the run options' `env` map.

The counter orders invocations; it does not route by harness id. Do not use it
to assign identities inside one parallel multi-harness invocation, where launch
order is intentionally unspecified.

This deterministic responder replaces a whole paid provider process while
retaining oneharness's real adapter argv and parsing. In contrast,
`oneharness mock` and `run --mock-rules` intercept individual tool calls made by
a real model; the statement that whole-harness mocking is out of scope for that
hook feature does not apply to `run --mock-harness`.
