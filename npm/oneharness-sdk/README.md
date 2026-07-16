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

Named exports include `RunOptionsSchema`, `HistoryLookupSchema`, `HistoryListOptionsSchema`, `RunReportSchema`, `RunStreamEnvelopeSchema`, `RunResultSchema`, `ActionEventSchema`, `UsageSchema`, `HistoryRecordSchema`, `HistoryStreamEnvelopeSchema`, `HistoryRecordsSchema`, `HistoryListSchema`, `HistorySessionSummarySchema`, `ListReportSchema`, `HarnessInfoSchema`, and the registry/detection enum and object schemas. `RunStreamEnvelope` and `HistoryStreamEnvelope` are also exported TypeScript types for consumers building JSONL clients. Each schema's `z.infer` type is compile-time checked against its generated TypeScript type.

Output objects accept and preserve unknown fields. That deliberate loose-object behavior lets an older SDK validate a newer additive CLI response without erasing fields before an application can inspect them. Known fields are still validated recursively. The input schemas `RunOptionsSchema`, `HistoryLookupSchema`, and `HistoryListOptionsSchema` are deliberately strict instead: unknown input keys are rejected because this SDK version cannot forward an option it does not understand, which also catches misspellings. `run`, `history`, and `historyList` validate against them before reading any option, so an unusable input raises `invalid oneharness run options` / `invalid oneharness history options` / `invalid oneharness history list options` rather than reaching the CLI.

Nullable CLI fields remain required object keys: the generated response schemas model Rust's serialization contract, so an unavailable value is `null`, while an omitted guaranteed field is malformed. Optional `RunOptions`, `HistoryLookup`, and `HistoryListOptions` fields may be absent or explicitly `undefined`; every `HistoryListOptions` field is optional, so `historyList()` and `historyList({})` both list the default store.

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

`HistoryLookupSchema` states the selector rule itself: a lookup is a union of the only two ways to select a session, so it accepts `last: true` or a non-empty `session` and rejects a lookup that selects neither. `history({})`, `history({ last: false })`, and `history({ session: "" })` fail validation rather than reaching the CLI, and `HistoryLookup` rejects them at compile time too.

`last: true` has priority over a name, so `history({ session: "old", last: true })` returns the most recent session and `history({ session: "old", last: false })` returns `old`. The two cases overlap deliberately — the union resolves `last: true` to its last-session variant first — which keeps `last` an ordinary `boolean` beside a named session, so a caller can pass one straight through.
