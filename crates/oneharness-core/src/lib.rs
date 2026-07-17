//! oneharness-core: the reusable engine behind the `oneharness` CLI, and a
//! depend-able library for tools that install hooks into — or sync configs for
//! — the supported agentic coding harnesses (Claude Code, Codex, OpenCode,
//! Goose, Qwen Code, Crush, Copilot CLI, Cursor).
//!
//! - [`domain`] is pure: it builds argv, parses output, shapes the report,
//!   holds the harness registry, renders hooks, layers config, and computes the
//!   sync merge — with no process / filesystem / env / clock I/O.
//! - [`io`] performs the real I/O: PATH resolution, version probing, spawning,
//!   reading config, and writing harness config files and hooks.
//!
//! The cross-harness capability a consumer most often wants: build a
//! [`domain::hooks::HookSpec`] and call [`io::hooks::install`] to write a
//! pre-tool hook into any harness in that harness's native shape (a shared
//! config file, a dedicated hooks file, or a plugin). It is generic — the
//! command and plugin identity are caller-supplied, never hardcoded.

pub mod domain;
pub mod errors;
pub mod io;
#[cfg(feature = "sdk-schema")]
pub mod sdk_schema;
