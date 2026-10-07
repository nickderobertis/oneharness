# AGENTS (release-tooling)

Subtree rules for the release lifecycle's own scripts — the verdict reader, the
idempotent publishers, the packaging and semver gates, the release-target drift
gate and probe — and their hermetic tests. Its `package-crates`, `semver-check`
and `probe-live` targets reach crates.io or the public registries, so they are
uncached and in neither gate tier: `just gate` and ci.yml's `package`/`semver`
jobs run them by name. Root `AGENTS.md` still applies.

- `just package-crates` — package both crates as Cargo verifies them at publish
  time. Deliberately NOT in `check`: `release.yml` runs `check` at the tag, and
  there the binary already pins the core version that same run publishes, so
  this can only be red — carrying it took v0.6.14 to no registry at all. It runs
  where it guards a release without being able to block one: `gate` and ci.yml's
  `package` job (which needs `fetch-depth: 0` + `fetch-tags`). Two windows are
  permitted, both of them release-plz's to close and no working tree's: a
  release-worthy core change awaiting its bump, and a core version already
  tagged but not yet on crates.io. The tag state is read ONLY to classify a
  packaging failure, so a checkout that cannot reach it fails nothing else.
- `just release-probe-live` — opt-in network proof of `scripts/release-probe.sh`.
  One target per ARTIFACT, never per repository: `oneharness-cli` is both a PyPI
  project and an npm package, and the two crates have shipped a whole minor
  apart. The probe's three answers must stay distinguishable — a version,
  **empty stdout** for *no release yet*, a **non-zero exit** for *not
  answered* — and everything uncertain resolves to the last, since a consumer
  stops waiting on the middle. `release-targets.toml` is not this repository's
  shape to change: it is written against the canonical schema nickderobertis/
  onevcs defines, so a field goes in only if that schema declares it, and an
  artifact this repository stops publishing is recorded rather than deleted. A
  per-platform `@oneharness/cli-*` package is not a target and needs BOTH halves
  of its accounting — `covers` and the launcher's `optionalDependencies`.
- **PyPI wheels** (mirroring `nickderobertis/llmlint`). The root `pyproject.toml`
  uses maturin's `bindings = "bin"` (the ruff/uv pattern) to wrap the prebuilt
  `oneharness` binary in per-platform wheels, so `pip install oneharness-cli` is a
  seconds-fast binary install where PyPI is reachable but github.com may be
  blocked. The PyPI distribution is **`oneharness-cli`** (the bare name was
  unavailable); the console command it installs is still `oneharness`. The wheel
  version is `dynamic` — maturin reads it from `Cargo.toml`, so release-plz stays
  the single version driver (never hand-set a version in `pyproject.toml`).
  Every channel ships every platform in `release-platforms.toml`, Windows ARM64
  included (the `aarch64-pc-windows-msvc` binary, the `win_arm64` wheel,
  `@oneharness/cli-win32-arm64`): a host with no wheel of its own makes `uv
  sync` of anything depending on `oneharness-cli` refuse outright (#1413).
  `release.yml`'s `build-wheels` job runs on every release (so a packaging break
  surfaces even while publishing is off); `publish-pypi` uses keyless **Trusted
  Publishing** (OIDC, no token secret and **no GitHub Actions environment** — the
  Trusted Publisher is registered without one, so the job must not declare
  `environment:` or the OIDC claim won't match) and stays dormant until the
  `PYPI_PUBLISH` repo variable is `true` and the PyPI project registers this
  repo's `release.yml` as its Trusted Publisher; `verify-pypi` then proves the
  published version is `pip install`-able. The pull-request packaging lane
  (`package-pr.yml`) is advisory, never a required check: an ARM runner queue
  must not extend time to merge.
- **The release reads a sweep, it never sweeps twice.** `scripts/ci-verdict.sh`
  takes the verdict of the full sweep CI ran over the tagged TREE — the merged
  release-plz pull request's run when its head carried exactly that tree (the
  git data API's tree shas are compared), else a `workflow_dispatch` of
  `ci.yml` with `tier=all` on the tag — and counts a check job only when its
  `Full sweep (just check all)` step succeeded: a green affected-tier job
  answers for a diff, not for the tree. Its `# CONTRACT:` line is the one
  statement of the fallback rule; `check-ci-verdict.sh` holds release.yml, the
  root `AGENTS.md`, the README and release-plz.toml to it word for word.
