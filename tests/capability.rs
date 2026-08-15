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
use oneharness_core::domain::capability::{
    Capability, FlagKind, OptionBinding, Suppression, UnlessResolution, CAPABILITIES,
};

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
        .filter_map(|binding| binding.kind.flag())
        .map(str::to_string)
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

// A binding that says "positional" while carrying a flag — or the reverse —
// used to be asserted here. It is now unrepresentable: `FlagKind` holds the
// flag inside the variants that have one, so there is no state to test. The
// SDK argv builders switch on exactly that discriminant, and a type is a
// stronger guarantee for them than a test that has to remember to look.

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
                    .any(|other| other.option == unless.option),
                "`{}` suppresses `{}` on `{}`, which it does not bind",
                capability.method,
                binding.option,
                unless.option,
            );
        }
    }
}

/// The resolutions both SDK argv builders implement.
///
/// A suppression cannot ship *unannotated* — `Suppression` carries its
/// resolution, so omitting one does not compile — but it can ship annotated with
/// a variant the generated clients have never heard of, which they would read as
/// the safe default and silently under-refuse. Adding a variant means teaching
/// `npm/oneharness-sdk/src/index.ts` and
/// `python/oneharness-sdk/src/oneharness_sdk/_client.py` first, then this list.
const GENERATOR_RESOLUTIONS: &[&str] = &["refuse", "prefer"];

#[test]
fn every_suppression_declares_a_resolution_the_generators_understand() {
    for capability in CAPABILITIES {
        for binding in capability.bindings {
            let Some(unless) = binding.unless else {
                continue;
            };
            let spelling = unless.resolution.wire_name();
            assert!(
                GENERATOR_RESOLUTIONS.contains(&spelling),
                "`{}` resolves `{}` against `{}` as `{spelling}`, which no SDK argv builder \
                 implements. Teach both clients the new resolution, then add it to \
                 GENERATOR_RESOLUTIONS — a resolution only Rust knows is one the SDKs fall \
                 back from, quietly, on the calls it was added to refuse.",
                capability.method,
                binding.option,
                unless.option,
            );
        }
    }
}

#[test]
fn a_resolution_reaches_the_wire_and_an_unsuppressed_binding_stays_unchanged() {
    // The manifest is what the SDKs generate from, so a resolution the
    // serializer drops is a refusal that never happens. The other half is why
    // the key is optional rather than always emitted: a binding with no
    // suppression has nothing to resolve, and its four keys are the shape every
    // released generator already reads.
    let refusing = serde_json::to_value(OptionBinding {
        option: "all",
        kind: FlagKind::Switch("--all"),
        unless: Some(Suppression {
            option: "harnesses",
            resolution: UnlessResolution::Refuse,
        }),
    })
    .expect("a binding serializes");
    assert_eq!(
        refusing,
        serde_json::json!({
            "option": "all",
            "flag": "--all",
            "kind": "switch",
            "unless": "harnesses",
            "unless_resolution": "refuse",
        })
    );

    let plain = serde_json::to_value(OptionBinding {
        option: "exclude",
        kind: FlagKind::Repeated("--exclude"),
        unless: None,
    })
    .expect("a binding serializes");
    assert_eq!(
        plain,
        serde_json::json!({
            "option": "exclude",
            "flag": "--exclude",
            "kind": "repeated",
            "unless": null,
        })
    );
}

// "A JSON capability names an output contract" was asserted here too. It is
// now unrepresentable for the same reason: `StdoutShape::Json`/`Jsonl` carry
// their schema root, and `Text` has no field to put one in. The half that a
// type cannot state — that the named root is one `bundle()` actually emits —
// is still a test, and still runs, in `sdk_schema`'s
// `every_named_schema_root_is_emitted_by_the_bundle`.

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
