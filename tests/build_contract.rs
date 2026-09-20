// llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `cargo test` is what runs it.
//! Proof that `.cargo/config.toml` does what its comment says, answered by cargo
//! itself rather than by a parser of the file.
//!
//! The file is this repository's copy of a shared build contract — `[build]
//! target-dir = "target"` and `[profile.dev] debug = 1` — and each key's effect
//! is only observable through a build: which directory an artifact lands in,
//! and which `-C debuginfo` level `rustc` is handed. Both are proven here by
//! copying the file beside a trivial crate under a scratch directory (the crate
//! nested one level below, since "resolved against the directory holding
//! `.cargo/`" is a claim a crate *at* that directory could not tell from "the
//! crate's own root") and reading cargo's verbose invocations back. Building
//! outside the clone keeps the proof off this workspace's own target directory
//! and its lock.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

/// The crate the probe builds; the name cargo's `--crate-name` line carries.
const PROBE: &str = "contract_probe";

/// Lay out `<scratch>/.cargo/config.toml` (the repository's, byte for byte) and
/// `<scratch>/nested/` holding a dependency-free library crate, and return the
/// crate's directory.
fn probe_crate(scratch: &Path) -> PathBuf {
    let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join(".cargo/config.toml");
    let cargo_dir = scratch.join(".cargo");
    std::fs::create_dir_all(&cargo_dir).expect("scratch .cargo");
    std::fs::copy(&contract, cargo_dir.join("config.toml")).expect("copy the contract");

    let nested = scratch.join("nested");
    std::fs::create_dir_all(nested.join("src")).expect("probe src");
    // `[workspace]` makes the probe its own root, so cargo does not walk up past
    // the scratch directory looking for a workspace that would own it.
    std::fs::write(
        nested.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{PROBE}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n"
        ),
    )
    .expect("probe manifest");
    std::fs::write(nested.join("src/lib.rs"), "pub fn probe() {}\n").expect("probe source");
    nested
}

/// Run cargo in `dir` with the target-directory and profile overrides an outer
/// invocation may have exported stripped away, so the config file — and nothing
/// above it — decides what cargo does.
fn cargo(dir: &Path, args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO"))
        .args(args)
        .current_dir(dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_PROFILE_DEV_DEBUG")
        .env_remove("CARGO_PROFILE_RELEASE_DEBUG")
        .output()
        .expect("spawn cargo");
    assert!(
        output.status.success(),
        "cargo {args:?} failed in {}:\n{}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// The `rustc` invocation that compiled the probe, from `cargo build -v`'s
/// stderr.
fn probe_rustc_line(build: &Output) -> String {
    let stderr = String::from_utf8_lossy(&build.stderr);
    stderr
        .lines()
        .find(|line| line.contains("rustc") && line.contains(&format!("--crate-name {PROBE}")))
        .unwrap_or_else(|| panic!("no rustc invocation for {PROBE} in:\n{stderr}"))
        .to_string()
}

#[test]
fn the_contract_builds_into_the_directory_holding_dot_cargo_with_line_tables_only() {
    let scratch = ScratchDir::new("build-contract").expect("scratch dir");
    let nested = probe_crate(scratch.path());

    let dev = cargo(&nested, &["build", "-v", "--offline"]);
    let dev_rustc = probe_rustc_line(&dev);
    assert!(
        dev_rustc.contains("-C debuginfo=1"),
        "a dev build must carry line tables only (`-C debuginfo=1`), got:\n{dev_rustc}"
    );

    let release = cargo(&nested, &["build", "-v", "--offline", "--release"]);
    let release_rustc = probe_rustc_line(&release);
    assert!(
        !release_rustc.contains("debuginfo=1"),
        "`release` is untouched by the dev profile, got:\n{release_rustc}"
    );

    // Both builds landed under the directory holding `.cargo/`, not the crate's
    // own root — which is what lets a crate outside the workspace share the
    // clone's one target directory.
    let shared = scratch.path().join("target");
    for profile in ["debug", "release"] {
        let rlib = shared.join(profile).join(format!("lib{PROBE}.rlib"));
        assert!(
            rlib.is_file(),
            "expected {} from the {profile} build",
            rlib.display()
        );
    }
    assert!(
        !nested.join("target").exists(),
        "the probe crate must not have grown a target directory of its own"
    );

    let metadata = cargo(
        &nested,
        &[
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ],
    );
    let metadata: Value = serde_json::from_slice(&metadata.stdout).expect("cargo metadata is JSON");
    let reported = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .expect("metadata names a target_directory"),
    );
    assert_eq!(
        reported.canonicalize().expect("reported target dir exists"),
        shared.canonicalize().expect("shared target dir exists"),
        "cargo metadata must report the directory holding `.cargo/`'s target"
    );
}
