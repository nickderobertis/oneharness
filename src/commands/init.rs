//! `oneharness init [PATH]` — scaffold a starter `oneharness.toml`.
//!
//! Unlike the other verbs, `init` *creates* configuration rather than reading
//! it, so it takes no `--config`/`--no-config` discovery and emits a plain
//! human-readable confirmation line (not the JSON contract the query verbs emit
//! for programmatic consumers): a scaffold is run interactively, and a
//! downstream tool that calls it twice for two differently named configs only
//! needs the exit code and the file on disk.
//!
//! The scaffold itself is a library call ([`oneharness_core::io::init::init`]),
//! so a Rust consumer writes the same file without spawning anything; this is
//! the shell that prints the confirmation line.

use crate::cli::InitArgs;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::init::{self, InitRequest};

pub use oneharness_core::io::init::starter_config;

pub fn run(args: &InitArgs) -> Result<i32, OneharnessError> {
    let report = init::init(&InitRequest {
        path: Some(args.path.clone()),
        force: args.force,
    })?;

    // A scaffold's output is the file; a plain confirmation line on stdout (not
    // a JSON envelope) matches a command a human runs interactively.
    println!("wrote {}", report.path);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oneharness_core::domain::config;
    use oneharness_core::domain::fallback::RunMode;
    use oneharness_core::io::scratch::ScratchDir;

    fn temp_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("init-{tag}")).unwrap()
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
