//! The language SDKs, held to the capability manifest they are generated from.
//!
//! `domain::capability::CAPABILITIES` is the one declaration every surface is
//! measured against; these are the checks that read the SDKs' own hand-written
//! files — each client's argv builder and each README's method inventory — to
//! prove they keep up with it. They live in a crate of their own, beside the
//! SDKs rather than inside the binary crate, so an SDK edit re-runs this
//! conformance suite and nothing the SDKs do not reach.

use oneharness_core::domain::capability::CAPABILITIES;

/// The resolutions both SDK argv builders implement.
///
/// A suppression cannot ship *unannotated* — `Suppression` carries its
/// resolution, so omitting one does not compile — but it can ship annotated with
/// a variant the generated clients have never heard of, which they would read as
/// the safe default and silently under-refuse. Adding a variant means teaching
/// `npm/oneharness-sdk/src/index.ts` and
/// `python/oneharness-sdk/src/oneharness_sdk/_client.py` first, then this list —
/// an order `a_listed_resolution_is_one_the_generated_surfaces_spell` enforces
/// rather than asks for.
const GENERATOR_RESOLUTIONS: &[&str] = &["refuse", "prefer"];

/// The generated surfaces `GENERATOR_RESOLUTIONS` is a claim about: each SDK's
/// argv builder, and the TypeScript union its generator writes beside them.
const RESOLUTION_SOURCES: &[&str] = &[
    "npm/oneharness-sdk/src/index.ts",
    "npm/oneharness-sdk/src/generated/capabilities.ts",
    "python/oneharness-sdk/src/oneharness_sdk/_client.py",
];

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
fn a_listed_resolution_is_one_the_generated_surfaces_spell() {
    // Until something reads them, `GENERATOR_RESOLUTIONS` is a claim about three
    // files this crate never opens — and the list's own doc comment describes
    // the order it needs (clients first, list second) with nothing holding
    // anyone to it. This is the reconciliation `check-sdk-coverage.sh` already
    // does for capabilities: derive from each surface's own source rather than
    // trust a list beside it. A resolution listed here but absent there is
    // exactly the failure the list exists to prevent — the clients read it as
    // the default and under-refuse the calls it was added to refuse.
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    for source in RESOLUTION_SOURCES {
        let text = std::fs::read_to_string(root.join(source))
            .unwrap_or_else(|error| panic!("cannot read `{source}`: {error}"));
        for resolution in GENERATOR_RESOLUTIONS {
            assert!(
                text.contains(&format!("\"{resolution}\"")),
                "`{source}` never spells the `{resolution}` resolution that \
                 GENERATOR_RESOLUTIONS says the generated surfaces implement. Teach the \
                 surface the resolution before listing it here.",
            );
        }
    }
}

/// Each SDK's README lists the client's methods in one sentence, by hand — the
/// one inventory the generators do not write. Hold it to the manifest, in each
/// language's own spelling, so a capability added here reaches the sentence a
/// reader meets first.
#[test]
fn every_capability_is_named_in_each_sdk_readme() {
    let node = include_str!("../../../npm/oneharness-sdk/README.md");
    let python = include_str!("../../../python/oneharness-sdk/README.md");
    for capability in CAPABILITIES {
        let camel = format!("`{}`", capability.method);
        assert!(
            node.contains(&camel),
            "npm/oneharness-sdk/README.md must list {camel} in its method inventory"
        );
        let mut snake = String::new();
        for c in capability.method.chars() {
            if c.is_ascii_uppercase() {
                snake.push('_');
                snake.push(c.to_ascii_lowercase());
            } else {
                snake.push(c);
            }
        }
        let snake = format!("`{snake}`");
        assert!(
            python.contains(&snake),
            "python/oneharness-sdk/README.md must list {snake} in its method inventory"
        );
    }
}
