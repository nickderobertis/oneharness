//! `oneharness init [PATH]` — scaffold a starter `oneharness.toml`.
//!
//! Unlike the other verbs, `init` *creates* configuration rather than reading
//! it, so it takes no `--config`/`--no-config` discovery and emits a plain
//! human-readable confirmation line (not the JSON contract the query verbs emit
//! for programmatic consumers): a scaffold is run interactively, and a
//! downstream tool that calls it twice for two differently named configs only
//! needs the exit code and the file on disk.
//!
//! The starter string is pure ([`starter_config`]) so a test can round-trip it
//! through `config::parse`; the thin write/overwrite shell is the only I/O.

use crate::cli::InitArgs;
use oneharness_core::errors::OneharnessError;

/// The starter `oneharness.toml` contents. A commented fallback-mode chain
/// (codex → claude-code), the recommended shape: the same committed file works
/// wherever one of the harnesses happens to be authenticated. Every key here is
/// a real `FileConfig`/`HarnessConfig` field (`run_mode`, `harnesses`,
/// `[harness.<id>].model`), so it parses via `config::parse`.
pub fn starter_config() -> &'static str {
    "\
# Starter oneharness config. Keep it at your project root as `oneharness.toml`;
# it sets the default harness/model selection for `oneharness run` in this repo.

# run_mode: how the selected harnesses are run.
#   \"parallel\" (default) runs them all at once and reports each.
#   \"fallback\" tries them in priority order and stops at the first that can
#   actually run, falling through only harnesses that cannot run at all (not
#   installed, unspawnable, or an auth/quota rejection) — so one committed file
#   works wherever a given harness happens to be authenticated.
run_mode = \"fallback\"

# harnesses: the selection, in priority order (the first is preferred under
# fallback). See `oneharness list` for every supported id.
harnesses = [\"codex\", \"claude-code\"]

# [harness.<id>]: per-harness overrides. Model names differ per provider, so
# each harness names its own.
[harness.codex]
model = \"gpt-5.5\"

[harness.claude-code]
model = \"claude-opus-4-8\"

# Inspect the effective, fully layered config (and where each value came from)
# with `oneharness config`.
"
}

pub fn run(args: &InitArgs) -> Result<i32, OneharnessError> {
    // Safe by default: never clobber an existing file without --force. A read
    // error from `try_exists` (e.g. a permission fault) is treated as "present"
    // so we refuse rather than risk an overwrite.
    if !args.force && args.path.try_exists().unwrap_or(true) {
        return Err(OneharnessError::InitFileExists {
            path: args.path.display().to_string(),
        });
    }

    std::fs::write(&args.path, starter_config()).map_err(|source| OneharnessError::InitWrite {
        path: args.path.display().to_string(),
        source,
    })?;

    // A scaffold's output is the file; a plain confirmation line on stdout (not
    // a JSON envelope) matches a command a human runs interactively.
    println!("wrote {}", args.path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oneharness_core::domain::config;
    use oneharness_core::domain::fallback::RunMode;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oneharness-init-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn starter_parses_as_a_valid_config() {
        // The whole point: the scaffold must be a config `config::parse` accepts.
        let cfg = config::parse(starter_config()).expect("starter must parse");
        assert_eq!(cfg.run_mode, Some(RunMode::Fallback));
        assert_eq!(cfg.harnesses.as_deref().unwrap(), ["codex", "claude-code"]);
        assert_eq!(cfg.model_for("codex"), Some("gpt-5.5"));
        assert_eq!(cfg.model_for("claude-code"), Some("claude-opus-4-8"));
    }

    #[test]
    fn writes_to_a_fresh_path() {
        let dir = temp_dir("fresh");
        let path = dir.join("oneharness.toml");
        let args = InitArgs {
            path: path.clone(),
            force: false,
        };
        assert_eq!(run(&args).unwrap(), 0);
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, starter_config());
        // And what landed on disk still parses.
        assert!(config::parse(&written).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let dir = temp_dir("nooverwrite");
        let path = dir.join("oneharness.toml");
        std::fs::write(&path, "harnesses = [\"codex\"]\n").unwrap();
        let args = InitArgs {
            path: path.clone(),
            force: false,
        };
        let err = run(&args).unwrap_err();
        assert!(matches!(err, OneharnessError::InitFileExists { .. }));
        // The original content is untouched.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "harnesses = [\"codex\"]\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_overwrites_an_existing_file() {
        let dir = temp_dir("force");
        let path = dir.join("oneharness.toml");
        std::fs::write(&path, "harnesses = [\"codex\"]\n").unwrap();
        let args = InitArgs {
            path: path.clone(),
            force: true,
        };
        assert_eq!(run(&args).unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), starter_config());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_error_surfaces_as_init_write() {
        // Target a path whose parent directory does not exist: the write fails
        // and must surface as the typed InitWrite error, not a panic.
        let dir = temp_dir("writefail");
        let path = dir.join("no-such-subdir").join("oneharness.toml");
        let args = InitArgs { path, force: false };
        let err = run(&args).unwrap_err();
        assert!(matches!(err, OneharnessError::InitWrite { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
