# AGENTS (exploration probes)

Subtree rules for the `explore-*` projects: investigative probes that dump what a
real harness does (its output shapes, hook behavior, stdin handling, turn
control) so an adapter is written from evidence rather than guessed. They are
informational, never a gate: each declares only an `explore` target, run from
its dispatch-only workflow (or `bash scripts/nx run explore-<id>:explore`), and
neither tier ever runs one. Their scripts are still shellchecked in the gate by
the `scripts` project, and `e2e-support` holds the probes' capability tables to
the registry. Root `AGENTS.md` still applies.
