# AGENTS (workspace)

Subtree rules for the repository-wide checks no single project owns.

- **`lint` is the project graph's boundary check** (`scripts/check-nx-graph.mjs`,
  rules in `boundaries.json` here), run over the graph Nx computes, so an edge
  inferred from an import counts like a declared one. Uncached: the graph it
  reads is no single file. A new project needs one `type:*` tag this file
  declares; a new crate edge needs the matching `implicitDependencies` entry,
  which the same check derives from `cargo metadata`. `check-nx-graph-test.sh`
  holds the refusals red against a scratch copy of the tree, in
  `workspace-integration` (`tools/workspace-integration`): it computes real Nx
  graphs, so it stays out of this project's offline `test`.
- **A gate test edits a staged copy, never the tracked file.** Projects run in
  parallel, so a check that mutates `release.yml`, `check-temp-leaks.sh` or a
  scratch prefix in place to watch a gate fail can hand a sibling task a file
  nobody committed; copy what the gate reads into scratch space and drift that.
- **`supply-chain`** (`cargo deny` + `cargo machete`) reads the network advisory
  database, so it is uncached and in neither tier; `just deps-check`, `just gate`
  and ci.yml's `deny` job run it by name.
