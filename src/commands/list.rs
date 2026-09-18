//! `oneharness list` — describe the supported harnesses as JSON (or, under
//! `--format text`, one line per harness).
//!
//! The description itself is a library call ([`oneharness_core::io::registry::list`]),
//! so a Rust consumer reads the same [`ListReport`] without spawning anything;
//! this is the shell that prints it.

use crate::cli::ListArgs;
use crate::commands::{print_report, printable, resolve_format};
use oneharness_core::domain::mode::ModeHeadless;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::registry::{self, ListRequest};

// Re-exported so the schema generator and existing consumers keep one import
// path for the CLI's output contract, wherever the type is defined.
pub use oneharness_core::io::registry::{HarnessInfo, ListReport, ModeInfo, VariantInfo};

pub fn run(args: &ListArgs) -> Result<i32, OneharnessError> {
    let format = resolve_format(args.format, args.compact)?;
    let report = registry::list(&ListRequest::default())?;
    print_report(&report, format, args.compact, render_text)?;
    Ok(0)
}

/// One line per harness: its id, display name, binary, and every capability
/// flag the JSON carries — `yes`/`no` for the booleans, the value (or `none`)
/// for the optional mechanisms — so a reader can tell at a glance what a verb
/// will refuse before trying it. Configured variants follow, indented.
fn render_text(report: &ListReport) -> String {
    let mut out = String::new();
    for h in &report.harnesses {
        let modes: Vec<String> = h
            .modes
            .iter()
            .map(|m| format!("{}{}", m.mode.as_str(), mode_suffix(m)))
            .collect();
        out.push_str(&format!(
            "{id} ({display}) bin {bin} · output {output} · modes {modes}\n",
            id = h.id,
            display = h.display,
            bin = h.default_bin,
            output = h.output_format.as_str(),
            modes = modes.join(","),
        ));
        out.push_str(&format!(
            "  resume {resume} · session {session} · fork {fork} · fork_reuses_cache {reuse} · control {control}\n",
            resume = yes_no(h.supports_resume),
            session = yes_no(h.session_capable),
            fork = yes_no(h.supports_fork),
            reuse = yes_no(h.fork_reuses_cache),
            control = h.control.map_or("none", |c| c.as_str()),
        ));
        out.push_str(&format!(
            "  native_schema {schema} · reasoning {reasoning} · prompt_stdin {stdin} · system_file {sysfile}\n",
            schema = yes_no(h.supports_native_schema),
            reasoning = yes_no(h.supports_reasoning),
            stdin = yes_no(h.supports_prompt_stdin),
            sysfile = yes_no(h.supports_system_file),
        ));
        out.push_str(&format!(
            "  sync_file {file} · allowed_tools {allow} · denied_tools {deny} · hooks {hooks} · mock_deny {deny_mock} · mock_rewrite {rewrite}\n",
            file = h.sync_file.unwrap_or("none"),
            allow = yes_no(h.supports_allowed_tools),
            deny = yes_no(h.supports_denied_tools),
            hooks = yes_no(h.supports_hooks),
            deny_mock = yes_no(h.supports_mock_deny),
            rewrite = h.mock_rewrite.unwrap_or("none"),
        ));
        for v in &h.variants {
            out.push_str(&format!(
                "  variant {name} → {selector}{model}{bin}\n",
                name = printable(&v.name),
                selector = printable(&v.harness_id),
                model = v
                    .model
                    .as_deref()
                    .map_or_else(String::new, |m| format!(" · model {}", printable(m))),
                bin = v
                    .bin
                    .as_deref()
                    .map_or_else(String::new, |b| format!(" · bin {}", printable(b))),
            ));
        }
    }
    out
}

/// The headless marker beside a mode: nothing when it is clean, `(hangs)`
/// when it may block on an approval prompt.
fn mode_suffix(mode: &ModeInfo) -> &'static str {
    match mode.headless {
        ModeHeadless::Clean => "",
        ModeHeadless::Hangs => "(hangs)",
    }
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_view_names_every_harness_with_its_binary_and_capability_flags() {
        let report = registry::list(&ListRequest {
            no_config: true,
            ..ListRequest::default()
        })
        .expect("the registry lists without config");
        let text = render_text(&report);
        for h in &report.harnesses {
            let line = text
                .lines()
                .find(|line| line.starts_with(&format!("{} (", h.id)))
                .unwrap_or_else(|| panic!("no line for {}:\n{text}", h.id));
            assert!(line.contains(&format!("bin {}", h.default_bin)), "{line}");
            assert!(
                line.contains("· modes ") && line.contains("bypass"),
                "{line}"
            );
        }
        // Every capability the JSON carries has a named flag in the view.
        for key in [
            "resume ",
            "session ",
            "fork ",
            "fork_reuses_cache ",
            "control ",
            "native_schema ",
            "reasoning ",
            "prompt_stdin ",
            "system_file ",
            "sync_file ",
            "allowed_tools ",
            "denied_tools ",
            "hooks ",
            "mock_deny ",
            "mock_rewrite ",
        ] {
            assert!(text.contains(key), "`{key}` is missing from:\n{text}");
        }
        assert!(text.contains("control claude-control-request"), "{text}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&text).is_err(),
            "the text view is not a JSON document"
        );
    }
}
