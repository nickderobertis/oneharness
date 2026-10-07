# AGENTS (sdk-conformance)

Subtree rules for the SDKs' conformance to the capability manifest: the checks
that read each SDK's hand-written client and README. A leaf that reads the SDKs
through its inputs rather than depending on them.

- The generated-contract drift checks never see a method that was never written, so
  `check-sdk-coverage.sh` (in this project's `test`) fails when a `domain::capability`
  entry has no method on a client — derived from the manifest and each client's
  own source, never a list — and `check-sdk-coverage-test.sh` holds that red in
  place, since a gate
  whose only job is to fail proves nothing unexercised. Every capability, flag
  and output field is tabulated per surface in `docs/sdk-parity.md`, which
  `just parity-audit` regenerates and `check-parity-audit.sh` pins. A binding
  whose `unless` names a sibling also says what BOTH halves asserting means —
  `refuse`, where the SDKs end the call naming both options, or `prefer`, where
  the suppressor deliberately wins (`{session, last}` is "the most recent"). The
  manifest has no third state: `Suppression` carries the resolution, so a new
  pair cannot inherit the old silent edit by omission. Refusing asks a narrower
  question than suppressing — a contradiction takes two positive assertions, and
  an empty value asserts nothing, so `{system: "", systemFile: …}` still
  suppresses rather than refusing.
