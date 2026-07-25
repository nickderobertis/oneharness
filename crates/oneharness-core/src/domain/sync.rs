//! Pure logic for `oneharness sync`: building the JSON fragment each harness's
//! project config file must contain, and the non-destructive deep merge that
//! folds it into whatever the file already holds. All I/O (reading and writing
//! the actual files) lives in `src/io/sync.rs`.

use serde_json::{Map, Value};

use crate::domain::config::FileConfig;
use crate::domain::harness::HarnessSpec;

/// What `sync` should do for one harness, computed purely from the unified
/// config and the registry's [`crate::domain::harness::SyncSpec`].
pub struct SyncPlan {
    /// The JSON object to merge into the harness's config file; `None` when
    /// nothing is configured for it (the harness is then reported `skipped`).
    pub fragment: Option<Value>,
    /// Top-level settings that exist but have no mapping for this harness
    /// (e.g. a top-level `allowed_tools` while the harness has no allow-list
    /// concept). Surfaced in the report and on stderr so a rule that did not
    /// land is always visible — never silently dropped. Per-harness fields
    /// can't end up here; those are rejected at config parse time.
    pub unmapped: Vec<&'static str>,
}

/// Build the sync plan for one harness. Pure.
///
/// The fragment starts from the harness's raw `[harness.<id>.settings]` table
/// (converted to JSON), then the unified rule lists and hooks are inserted at
/// the registry's key paths — an explicit `allowed_tools`/`denied_tools`/
/// `hooks` wins over the same key in the raw table.
pub fn plan(cfg: &FileConfig, spec: &HarnessSpec) -> Result<SyncPlan, String> {
    plan_for(cfg, spec, spec.id)
}

pub fn plan_for(cfg: &FileConfig, spec: &HarnessSpec, id: &str) -> Result<SyncPlan, String> {
    let Some(sync) = &spec.sync else {
        // No config surface at all: anything aimed at this harness from the
        // top level is unmapped (per-harness fields were parse-rejected).
        let mut unmapped = Vec::new();
        if !cfg.allowed_tools_for(id).is_empty() {
            unmapped.push("allowed_tools");
        }
        if !cfg.denied_tools_for(id).is_empty() {
            unmapped.push("denied_tools");
        }
        return Ok(SyncPlan {
            fragment: None,
            unmapped,
        });
    };

    let mut root = match cfg.settings_for(id) {
        Some(table) => {
            let value = serde_json::to_value(table)
                .map_err(|e| format!("settings table is not representable as JSON: {e}"))?;
            match value {
                Value::Object(map) => map,
                _ => return Err("`settings` must be a table".to_string()),
            }
        }
        None => Map::new(),
    };
    let mut unmapped = Vec::new();

    let rules = [
        ("allowed_tools", cfg.allowed_tools_for(id), sync.allow_path),
        ("denied_tools", cfg.denied_tools_for(id), sync.deny_path),
    ];
    for (name, values, path) in rules {
        if values.is_empty() {
            continue;
        }
        match path {
            Some(path) => {
                let list = Value::Array(values.iter().cloned().map(Value::String).collect());
                insert_at(&mut root, path, list);
            }
            None => unmapped.push(name),
        }
    }

    // Complete the schema for any top-level key the fragment touches (e.g.
    // Cursor requires both permissions arrays); untouched keys stay unseeded.
    if let Some(seed) = sync.schema_seed {
        let seed: Value =
            serde_json::from_str(seed).expect("registry schema_seed is valid JSON (test-pinned)");
        if let Value::Object(seed_map) = seed {
            for (key, seed_value) in seed_map {
                if let Some(current) = root.get(&key) {
                    let completed = deep_merge(&seed_value, current);
                    root.insert(key, completed);
                }
            }
        }
    }

    if let Some(hooks) = cfg.hooks_for(id) {
        // Parse-time validation guarantees hooks only appear where a path
        // exists, so this expect cannot fire on user input.
        let path = sync.hooks_path.expect("hooks validated against hooks_path");
        let value = serde_json::to_value(hooks)
            .map_err(|e| format!("hooks table is not representable as JSON: {e}"))?;
        insert_at(&mut root, path, value);
    }

    Ok(SyncPlan {
        fragment: if root.is_empty() {
            None
        } else {
            Some(Value::Object(root))
        },
        unmapped,
    })
}

/// Insert `value` at a key path, creating intermediate objects. An explicit
/// unified field overwrites the same key from the raw settings table (the
/// dedicated field is the more specific intent).
fn insert_at(root: &mut Map<String, Value>, path: &[&str], value: Value) {
    let (last, parents) = path.split_last().expect("key paths are non-empty");
    let mut node = root;
    for key in parents {
        let entry = node
            .entry(key.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        node = entry.as_object_mut().expect("just ensured an object");
    }
    node.insert(last.to_string(), value);
}

/// Merge `fragment` into `existing`, non-destructively:
///
/// - objects merge per key — keys absent from the fragment are never touched;
/// - arrays union — existing entries keep their order, fragment entries not
///   already present are appended (so re-syncing is idempotent);
/// - anything else (scalars, or a type mismatch) takes the fragment's value —
///   for keys oneharness manages, the unified config is the source of truth.
pub fn deep_merge(existing: &Value, fragment: &Value) -> Value {
    match (existing, fragment) {
        (Value::Object(a), Value::Object(b)) => {
            let mut merged = a.clone();
            for (key, frag) in b {
                let entry = match a.get(key) {
                    Some(prev) => deep_merge(prev, frag),
                    None => frag.clone(),
                };
                merged.insert(key.clone(), entry);
            }
            Value::Object(merged)
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut merged = a.clone();
            for item in b {
                if !merged.contains(item) {
                    merged.push(item.clone());
                }
            }
            Value::Array(merged)
        }
        _ => fragment.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{config, harness};
    use serde_json::json;

    fn cfg(text: &str) -> FileConfig {
        config::parse(text).expect("config should parse")
    }

    fn plan_for(text: &str, id: &str) -> SyncPlan {
        plan(&cfg(text), harness::by_id(id).unwrap()).expect("plan should build")
    }

    #[test]
    fn nothing_configured_means_no_fragment() {
        let p = plan_for("model = \"x\"", "claude-code");
        assert!(p.fragment.is_none());
        assert!(p.unmapped.is_empty());
    }

    #[test]
    fn rules_and_hooks_land_at_the_registry_paths() {
        let p = plan_for(
            concat!(
                "allowed_tools = [\"Bash(git log:*)\"]\n",
                "denied_tools = [\"Bash(rm:*)\"]\n",
                "[harness.claude-code.hooks]\n",
                "PreToolUse = []\n",
            ),
            "claude-code",
        );
        assert_eq!(
            p.fragment.unwrap(),
            json!({
                "permissions": {
                    "allow": ["Bash(git log:*)"],
                    "deny": ["Bash(rm:*)"],
                },
                "hooks": { "PreToolUse": [] },
            })
        );
        assert!(p.unmapped.is_empty());
    }

    #[test]
    fn crush_deny_maps_to_disabled_tools() {
        let p = plan_for("denied_tools = [\"bash\"]", "crush");
        assert_eq!(
            p.fragment.unwrap(),
            json!({ "options": { "disabled_tools": ["bash"] } })
        );
    }

    #[test]
    fn settings_table_passes_through_and_explicit_rules_win() {
        let p = plan_for(
            concat!(
                "[harness.opencode.settings.permission]\n",
                "edit = \"deny\"\n",
                "[harness.opencode.settings.permission.bash]\n",
                "\"git *\" = \"allow\"\n",
            ),
            "opencode",
        );
        assert_eq!(
            p.fragment.unwrap(),
            json!({ "permission": { "edit": "deny", "bash": { "git *": "allow" } } })
        );

        // An explicit rule list overwrites the same key from the raw table.
        let p = plan_for(
            concat!(
                "[harness.claude-code]\n",
                "allowed_tools = [\"Read\"]\n",
                "[harness.claude-code.settings.permissions]\n",
                "allow = [\"stale\"]\n",
                "defaultMode = \"acceptEdits\"\n",
            ),
            "claude-code",
        );
        assert_eq!(
            p.fragment.unwrap(),
            json!({ "permissions": { "allow": ["Read"], "defaultMode": "acceptEdits" } })
        );
    }

    #[test]
    fn top_level_rules_without_a_mapping_are_reported_unmapped() {
        // opencode has a config file but no list-shaped permission concept;
        // codex has no config surface at all. Both must say so, loudly.
        let text = "allowed_tools = [\"x\"]\ndenied_tools = [\"y\"]";
        let p = plan_for(text, "opencode");
        assert!(p.fragment.is_none());
        assert_eq!(p.unmapped, ["allowed_tools", "denied_tools"]);
        let p = plan_for(text, "codex");
        assert!(p.fragment.is_none());
        assert_eq!(p.unmapped, ["allowed_tools", "denied_tools"]);
    }

    #[test]
    fn every_registry_schema_seed_is_valid_json() {
        for spec in harness::all() {
            if let Some(seed) = spec.sync.as_ref().and_then(|s| s.schema_seed) {
                let value: Value = serde_json::from_str(seed)
                    .unwrap_or_else(|e| panic!("{}: schema_seed is not JSON: {e}", spec.id));
                assert!(
                    value.is_object(),
                    "{}: schema_seed must be an object",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn cursor_allow_only_is_completed_with_an_empty_deny() {
        // Cursor's CLI rejects a permissions block missing either array
        // (observed live: "permissions.deny Required"), so a partial write
        // must be seeded into a schema-valid shape.
        let p = plan_for(
            "[harness.cursor]\nallowed_tools = [\"Shell(touch)\"]",
            "cursor",
        );
        assert_eq!(
            p.fragment.unwrap(),
            json!({ "permissions": { "allow": ["Shell(touch)"], "deny": [] } })
        );
        // And the seed never invents a permissions block out of nothing.
        let p = plan_for(
            "[harness.cursor.settings]\neditor = { vimMode = true }",
            "cursor",
        );
        assert_eq!(
            p.fragment.unwrap(),
            json!({ "editor": { "vimMode": true } })
        );
    }

    #[test]
    fn deep_merge_preserves_unrelated_keys_and_unions_arrays() {
        let existing = json!({
            "permissions": { "allow": ["Read", "Bash(ls *)"], "defaultMode": "plan" },
            "env": { "FOO": "bar" },
        });
        let fragment = json!({
            "permissions": { "allow": ["Bash(ls *)", "Edit"], "deny": ["Bash(rm *)"] },
        });
        let merged = deep_merge(&existing, &fragment);
        assert_eq!(
            merged,
            json!({
                "permissions": {
                    "allow": ["Read", "Bash(ls *)", "Edit"],
                    "defaultMode": "plan",
                    "deny": ["Bash(rm *)"],
                },
                "env": { "FOO": "bar" },
            })
        );
    }

    #[test]
    fn deep_merge_is_idempotent_and_scalars_take_the_fragment() {
        let fragment = json!({ "a": { "b": [1, 2], "c": "new" } });
        let once = deep_merge(&json!({ "a": { "c": "old" } }), &fragment);
        assert_eq!(once, json!({ "a": { "b": [1, 2], "c": "new" } }));
        let twice = deep_merge(&once, &fragment);
        assert_eq!(twice, once, "re-syncing must change nothing");
    }
}
