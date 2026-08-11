//! The capability manifest, reconciled against the real clap surface.
//!
//! `domain::capability::CAPABILITIES` is what the parity gate measures every
//! consumer surface against, so the manifest going stale would quietly hollow
//! out the gate — which is exactly how five verbs came to have no SDK
//! counterpart with nothing objecting. This walks the command tree clap
//! actually builds and holds the manifest to it in both directions:
//!
//! * every verb has a capability (or a declared reason it is not one);
//! * every long flag on a verb is either **bound** to an SDK option, **always**
//!   emitted by the method, or **declared uncovered with a reason**.
//!
//! There is no third option and no default. A flag added to `src/cli.rs` fails
//! this test until someone decides which of the three it is, and the decision
//! is data the SDKs then build their argv from — not a note that can drift from
//! what they do.

use std::collections::BTreeSet;

use clap::CommandFactory;
use oneharness_core::domain::capability::{Capability, FlagKind, CAPABILITIES};

/// Verbs that are deliberately not capabilities, with the reason.
///
/// A verb reaches this list only when driving it through an SDK is meaningless,
/// not when nobody has got to it yet.
const NOT_A_CAPABILITY: &[(&str, &str)] = &[(
    "mock-harness",
    "the deterministic fake provider process oneharness spawns as a harness; it is selected through `run --mock-harness`, which the run capability binds, and has no caller of its own",
)];

/// Flags clap generates rather than the CLI declaring them.
const CLAP_BUILTINS: &[&str] = &["help", "version"];

/// Every leaf verb clap exposes, as its argv path.
fn clap_verbs() -> Vec<(Vec<String>, clap::Command)> {
    let mut found = Vec::new();
    walk(&oneharness::Cli::command(), &[], &mut found);
    found
}

fn walk(command: &clap::Command, path: &[String], out: &mut Vec<(Vec<String>, clap::Command)>) {
    let mut children = command.get_subcommands().peekable();
    if children.peek().is_none() {
        if !path.is_empty() {
            out.push((path.to_vec(), command.clone()));
        }
        return;
    }
    for child in children {
        if child.get_name() == "help" {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(child.get_name().to_string());
        walk(child, &child_path, out);
    }
}

/// The long flags a verb declares, minus clap's own.
fn long_flags(command: &clap::Command) -> BTreeSet<String> {
    command
        .get_arguments()
        .filter_map(|arg| arg.get_long())
        .filter(|long| !CLAP_BUILTINS.contains(long))
        .map(|long| format!("--{long}"))
        .collect()
}

/// Every flag spelling a capability's method emits or declines.
fn decided_flags(capability: &Capability) -> BTreeSet<String> {
    capability
        .bindings
        .iter()
        .filter(|binding| !binding.flag.is_empty())
        .map(|binding| binding.flag.to_string())
        .chain(
            capability
                .always
                .iter()
                .filter(|fragment| fragment.starts_with("--"))
                .map(|fragment| (*fragment).to_string()),
        )
        .chain(
            capability
                .uncovered
                .iter()
                .map(|flag| flag.flag.to_string()),
        )
        .collect()
}

#[test]
fn every_verb_is_a_capability_or_says_why_it_is_not() {
    for (path, _) in clap_verbs() {
        let joined = path.join(" ");
        let declared = CAPABILITIES.iter().any(|c| c.argv == path.as_slice());
        let excused = NOT_A_CAPABILITY.iter().any(|(name, _)| *name == joined);
        assert!(
            declared || excused,
            "`oneharness {joined}` has no capability in `domain::capability::CAPABILITIES`. \
             Add one — with its SDK method, option contract, and Rust entry point — or add it \
             to NOT_A_CAPABILITY with the reason driving it through an SDK is meaningless. \
             A verb with no decision is how a whole verb goes missing from every SDK unnoticed."
        );
    }
}

#[test]
fn every_capability_names_a_verb_clap_actually_has() {
    let verbs: Vec<Vec<String>> = clap_verbs().into_iter().map(|(path, _)| path).collect();
    for capability in CAPABILITIES {
        let path: Vec<String> = capability.argv.iter().map(|s| (*s).to_string()).collect();
        assert!(
            verbs.contains(&path),
            "capability `{}` invokes `oneharness {}`, which clap does not expose. \
             The manifest is what the SDKs build their argv from, so a path that does not \
             exist is a call that fails at runtime.",
            capability.method,
            path.join(" ")
        );
    }
}

#[test]
fn every_flag_is_bound_always_emitted_or_declined_with_a_reason() {
    let mut missing: Vec<String> = Vec::new();
    for (path, command) in clap_verbs() {
        let joined = path.join(" ");
        let capabilities: Vec<&Capability> = CAPABILITIES
            .iter()
            .filter(|c| c.argv == path.as_slice())
            .collect();
        if capabilities.is_empty() {
            continue; // covered by `every_verb_is_a_capability_or_says_why_it_is_not`
        }
        for capability in capabilities {
            let decided = decided_flags(capability);
            for flag in long_flags(&command) {
                if !decided.contains(&flag) {
                    missing.push(format!(
                        "{} ({joined}) does not decide {flag}",
                        capability.method
                    ));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "every CLI flag must be bound to an SDK option, listed in the capability's `always` \
         argv, or declared in `uncovered` with the reason it need not be. These are undecided, \
         which means an SDK caller silently cannot reach them:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn a_declined_flag_carries_a_reason_a_reader_can_act_on() {
    // "Deliberately uncovered" is only a legitimate outcome when it says why.
    for capability in CAPABILITIES {
        for declined in capability.uncovered {
            assert!(
                declined.reason.len() > 20,
                "`{}` declines `{}` with no usable reason: {:?}",
                capability.method,
                declined.flag,
                declined.reason
            );
        }
    }
}

#[test]
fn a_positional_binding_names_no_flag_and_a_flag_binding_names_one() {
    // The SDK argv builders switch on exactly this, so a binding that says
    // "positional" while carrying a flag (or the reverse) builds a broken call.
    for capability in CAPABILITIES {
        for binding in capability.bindings {
            let flagless = matches!(binding.kind, FlagKind::Positional | FlagKind::Trailing);
            assert_eq!(
                binding.flag.is_empty(),
                flagless,
                "`{}` binds `{}` as {:?} with flag {:?}",
                capability.method,
                binding.option,
                binding.kind,
                binding.flag
            );
        }
    }
}

#[test]
fn a_suppressing_option_is_one_the_same_capability_binds() {
    // `unless` names a sibling option, and a typo would silently stop
    // suppressing — reintroducing the conflicting-flag call it exists to avoid.
    for capability in CAPABILITIES {
        for binding in capability.bindings {
            let Some(unless) = binding.unless else {
                continue;
            };
            assert!(
                capability
                    .bindings
                    .iter()
                    .any(|other| other.option == unless),
                "`{}` suppresses `{}` on `{unless}`, which it does not bind",
                capability.method,
                binding.option,
            );
        }
    }
}

#[test]
fn method_names_are_unique_and_translate_to_python() {
    let mut seen = BTreeSet::new();
    for capability in CAPABILITIES {
        assert!(
            seen.insert(capability.method),
            "two capabilities both claim the method `{}`",
            capability.method
        );
    }
    // One declaration, both spellings: the Python SDK snake-cases what the Node
    // SDK camel-cases, so the manifest must translate rather than list twice.
    let history_list = CAPABILITIES
        .iter()
        .find(|c| c.method == "historyList")
        .expect("historyList is a capability");
    assert_eq!(history_list.python_method(), "history_list");
}
