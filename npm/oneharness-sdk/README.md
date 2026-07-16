# @oneharness/sdk

Typed Node.js access to the `oneharness` engine. The SDK launches the packaged CLI and validates every response with named Zod schemas generated from the Rust wire types. The corresponding TypeScript declarations come from the same Rust JSON Schema bundle.

```ts
import { OneHarness, RunReportSchema, type RunReport } from "@oneharness/sdk";

const oneharness = new OneHarness();
const report = await oneharness.run({ prompt: "Summarize this repository", harnesses: ["codex"], events: true });
const checked: RunReport = RunReportSchema.parse(report);
console.log(checked.results[0]?.text, checked.results[0]?.usage.input_tokens);
```

`null` usage fields mean the harness did not report the value; zero remains a real measured zero. String-valued harness/model/event identifiers should be treated as open sets for forward compatibility.

Named exports include `RunOptionsSchema`, `RunReportSchema`, `RunResultSchema`, `ActionEventSchema`, `UsageSchema`, `HistoryRecordSchema`, `HistoryRecordsSchema`, `HistoryListSchema`, `HistorySessionSummarySchema`, `ListReportSchema`, `HarnessInfoSchema`, and the registry/detection enum and object schemas. Each schema's `z.infer` type is compile-time checked against its generated TypeScript type.

Output objects accept and preserve unknown fields. That deliberate loose-object behavior lets an older SDK validate a newer additive CLI response without erasing fields before an application can inspect them. Known fields are still validated recursively. `RunOptionsSchema` is deliberately strict instead: unknown input keys are rejected because this SDK version cannot forward an option it does not understand, which also catches misspellings.

Nullable CLI fields remain required object keys: the generated response schemas model Rust's serialization contract, so an unavailable value is `null`, while an omitted guaranteed field is malformed. Optional `RunOptions` fields may be absent or explicitly `undefined`.

Continuation passes the prior result's native session id with a new user message:

```ts
const first = await oneharness.run({ prompt: "Inspect the bug", harnesses: ["codex"] });
const next = await oneharness.run({
  prompt: "Now propose the smallest fix",
  harnesses: ["codex"],
  resume: first.results[0]?.session_id ?? undefined,
});
```

Standardized history records and session summaries use their generated schemas too:

```ts
import { HistoryListSchema, HistoryRecordSchema } from "@oneharness/sdk";

const records = await oneharness.history({ last: true });
const sessions = await oneharness.historyList({ allProjects: true });
HistoryRecordSchema.parse(records[0]);
HistoryListSchema.parse(sessions);
```
