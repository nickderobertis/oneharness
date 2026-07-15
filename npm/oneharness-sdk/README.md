# @oneharness/sdk

Typed Node.js access to the `oneharness` engine. The SDK launches the packaged CLI, validates every run/history response against JSON Schemas generated from the Rust wire types, and returns generated TypeScript declarations.

```ts
import { OneHarness } from "@oneharness/sdk";

const oneharness = new OneHarness();
const report = await oneharness.run({ prompt: "Summarize this repository", harnesses: ["codex"], events: true });
console.log(report.results[0]?.text, report.results[0]?.usage.input_tokens);
```

`null` usage fields mean the harness did not report the value; zero remains a real measured zero. String-valued harness/model/event identifiers should be treated as open sets for forward compatibility.
