# AGENTS (install-surface)

Subtree rules for `scripts/install.sh` (the published installer, whose URL is a
public contract and never moves) and its hermetic e2e. Root `AGENTS.md` still applies.

<!-- llmlint: ignore-block[agents_md_durable_and_terse] Moved verbatim from the root AGENTS.md under this change's instruction to move — not rewrite or trim — the text that governs this project (standalone trimming of AGENTS.md is outside its scope); only references to where its checks now run were updated. Its account of the release jobs is where the constraints it states apply; a durability pass over it is a change of its own. -->
- **Sigstore release signing + mirror-safe `install.sh`** (mirroring llmlint).
  `release.yml`'s `upload` job signs each archive with a
  keyless [Sigstore](https://www.sigstore.dev/) build-provenance attestation
  (`actions/attest-build-provenance@v2`, OIDC `id-token` — no secret) and
  publishes the `.sigstore.json` bundle beside the archive. `scripts/install.sh`
  verifies the downloaded archive against a trust root **independent of the
  mirror it came from**, in order: (1) the Sigstore bundle, verified OFFLINE by
  `cosign` → `sigstore` (python) → `gh` (whichever is installed), pinned to this
  repo's `release.yml` signer identity + SLSA-provenance predicate; (2) a SHA-256
  checksum from canonical GitHub — and it **refuses** a checksum that shares the
  mirror's origin (a mirror vouching for its own download is no trust root),
  aborting instead. Never re-introduce a "trust the mirror's own checksum" escape
  hatch. The `verify-attestation` release job runs the exact `cosign`/`sigstore`
  commands `install.sh` uses against the real published bundle — the drift alarm
  for the signing identity/flags. The install path is proven hermetically by
  `scripts/install-e2e.sh` (this project's `test`, through `scripts/smoke.sh
  --install`): independent
  checksum installs, tampered mirror rejected, mirror-origin checksum refused, and
  a stubbed `cosign`/`sigstore`/`gh` proves the Sigstore gate (pass installs, fail
  aborts) without a live signature.
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
