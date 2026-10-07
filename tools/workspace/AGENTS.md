# AGENTS (workspace)

Subtree rules for the repository-wide checks no single project owns. Root
`AGENTS.md` still applies.

- **`lint` is the project graph's boundary check** (`scripts/check-nx-graph.mjs`,
  rules in `boundaries.json` here), run over the graph Nx computes, so an edge
  inferred from an import counts like a declared one. Uncached: the graph it
  reads is no single file. A new project needs one `type:*` tag this file
  declares; a new crate edge needs the matching `implicitDependencies` entry,
  which the same check derives from `cargo metadata`. `check-nx-graph-test.sh`
  holds the refusals red against a scratch copy of the tree.
- **`test` holds the gate's own plumbing**: the affected tier's base
  (`scripts/nx-base.sh`, `check-nx-base.sh`), the scratch-leak and LF gates,
  and the checks that `just check` and `just bootstrap` stay self-sufficient in
  a fresh checkout (`check-sdk-install.sh`, `check-build-mock-harness.sh`).
- **`supply-chain`** (`cargo deny` + `cargo machete`) reads the network advisory
  database, so it is uncached and in neither tier; `just deps-check`, `just gate`
  and ci.yml's `deny` job run it by name.
