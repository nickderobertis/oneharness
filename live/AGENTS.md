# AGENTS (live suites)

Subtree rules for the `live-*` projects: the paid, credential-gated suites that
drive real harnesses. None declares a gate target, so neither tier ever runs one;
each runs from its own `.github/workflows/e2e-*.yml` (and `just live-<id>`). Root `AGENTS.md` still applies.

- Also out of the core gate; the workflows are gated to the canonical repo and
  non-fork PRs. Auth comes from the `gh-secrets.json` manifest (Bitwarden secure
  notes → `.env` + GitHub Actions secrets via `just secrets-sync`); values never
  enter the repo and `.env` / `.gh-secrets-state.json` are gitignored.
