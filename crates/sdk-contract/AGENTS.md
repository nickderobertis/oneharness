# AGENTS (sdk-contract)

Subtree rules for the cross-language SDK contract: the generator every SDK runs
(`examples/generate_sdk_schema.rs`, which emits `sdk_schema::bundle()`) and the
acceptance matrix all three languages are held to
(`tests/fixtures/sdk-contract-matrix.json`). Tagged `type:contract`: it depends
only on the engine it is generated from, never on an SDK that consumes it.

- A doc comment on a schema-carrying type is also both SDKs' generated
  `description`, so rewording one (even a rustdoc link repair) needs `just
  sdk-generate` and `just python-sdk-generate` in the same change, or the drift
  check refuses the push.
