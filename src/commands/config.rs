//! `oneharness config` — show the effective layered configuration as JSON,
//! with each value attributed to the config file (or built-in default) it came
//! from. This is the debugging surface for the layering: when a run behaves
//! unexpectedly, this shows exactly which file shaped which setting. The
//! `--format text` view is the same report, one `field: value (source)` row per
//! resolved field.

use crate::cli::ConfigArgs;
use crate::commands::{print_report, printable, resolve_format};
use oneharness_core::domain::config as domain_config;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;

pub fn run(args: &ConfigArgs) -> Result<i32, OneharnessError> {
    let format = resolve_format(args.format, args.compact)?;
    // Mirror `run`'s discovery exactly (--cwd, else the current directory) so
    // the report shows what a run from that directory would actually load.
    let project_start = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let layers = config_io::load_layers(args.config.as_deref(), args.no_config, &project_start)?;
    let report = domain_config::explain(&layers);
    print_report(&report, format, args.compact, render_text)?;
    Ok(0)
}

/// Every resolved field as `path: value (source)`, walking the report's own
/// JSON shape rather than naming its fields: a `{value, source}` pair is a
/// field wherever it sits (top level, a `[env]` key, a `[harness.<id>]`
/// override, a variant), so a field added to the report appears here without
/// this view having to learn it. An unset field reads `unset`; the source of a
/// built-in default is the `default` token the JSON uses.
fn render_text(report: &domain_config::ConfigReport) -> String {
    let value = serde_json::to_value(report).unwrap_or(serde_json::Value::Null);
    let mut out = String::new();
    if let Some(files) = value
        .get("config_files")
        .and_then(serde_json::Value::as_array)
    {
        let listed: Vec<String> = files
            .iter()
            .map(|f| printable(f.as_str().unwrap_or_default()))
            .collect();
        out.push_str(&format!(
            "config files: {}\n",
            if listed.is_empty() {
                "none".to_string()
            } else {
                listed.join(", ")
            }
        ));
    }
    if let Some(fields) = value.as_object() {
        for (key, field) in fields {
            if key == "schema_version" || key == "config_files" {
                continue;
            }
            render_field(&mut out, key, field);
        }
    }
    out
}

/// Recurse into the report: a `{value, source}` object is a row; any other
/// object is a table whose entries are fields under a dotted path — an empty
/// one (`[env]` with no keys, no `[harness.<id>]` sections) still gets its own
/// row, so a reader sees that the table exists and is empty.
fn render_field(out: &mut String, path: &str, node: &serde_json::Value) {
    let Some(map) = node.as_object() else {
        return;
    };
    if map.len() == 2 && map.contains_key("value") && map.contains_key("source") {
        let (value, source) = (&map["value"], &map["source"]);
        let shown = match value {
            serde_json::Value::Null => "unset".to_string(),
            serde_json::Value::String(s) => printable(s),
            other => printable(&other.to_string()),
        };
        match source.as_str() {
            Some(source) => out.push_str(&format!("{path}: {shown} ({})\n", printable(source))),
            None => out.push_str(&format!("{path}: {shown}\n")),
        }
        return;
    }
    if map.is_empty() {
        out.push_str(&format!("{path}: none\n"));
        return;
    }
    for (key, child) in map {
        render_field(out, &format!("{path}.{}", printable(key)), child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_view_lists_every_field_with_its_value_and_source() {
        let project = domain_config::parse(
            r#"
            model = "opus"
            [env]
            FOO = "bar"
            [harness.codex]
            model = "o3"
            "#,
        )
        .expect("a valid config");
        let report = domain_config::explain(&[("/p/oneharness.toml".to_string(), project)]);
        let text = render_text(&report);

        assert!(
            text.starts_with("config files: /p/oneharness.toml\n"),
            "{text}"
        );
        assert!(
            text.contains("model: opus (/p/oneharness.toml)\n"),
            "{text}"
        );
        assert!(
            text.contains("env.FOO: bar (/p/oneharness.toml)\n"),
            "{text}"
        );
        assert!(
            text.contains("harness.codex.model: o3 (/p/oneharness.toml)\n"),
            "{text}"
        );
        assert!(text.contains("bypass: false (default)\n"), "{text}");
        assert!(text.contains("system: unset\n"), "{text}");
        assert!(text.contains("history_labels: none\n"), "{text}");

        // Every top-level field the JSON carries has a row.
        let json = serde_json::to_value(&report).expect("serializable");
        for key in json.as_object().expect("an object").keys() {
            if key == "schema_version" || key == "config_files" {
                continue;
            }
            assert!(
                text.lines().any(|line| line.starts_with(key.as_str())),
                "`{key}` has no row in:\n{text}"
            );
        }
    }
}
