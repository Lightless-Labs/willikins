//! [`PlanError`] serializes internally tagged: every variant's JSON carries
//! `{"kind": "<Variant>", ...its own fields}`. Exhaustive over every
//! variant the same way `willikins-core`'s own `check.rs` unit tests are
//! exhaustive over `CheckError` and `CheckWarning`: a `match` with no
//! wildcard arm, so a variant added to [`PlanError`] without a matching
//! sample and arm here fails to compile rather than silently escaping this
//! test.

mod common;

use std::collections::HashSet;

use common::{input, node, port, tool_name};
use willikins_core::{Inputs, PlanError, ToolError, ToolErrorKind};

/// One instance of every [`PlanError`] variant.
fn plan_error_samples() -> Vec<PlanError> {
    vec![
        PlanError::MissingInput { input: input("i") },
        PlanError::ForEachUnknown { node: node("n") },
        PlanError::DuplicateForEachKey {
            node: node("n"),
            key: "k".to_string(),
        },
        PlanError::KeyNotInForEach {
            node: node("n"),
            key: "k".to_string(),
        },
        PlanError::KeyUnknown {
            node: node("n"),
            port: port("p"),
        },
        PlanError::NameTaken {
            node: node("n"),
            tool: tool_name("bogus.tool"),
            key: Inputs::new(),
        },
        PlanError::Tool {
            node: node("n"),
            error: ToolError {
                kind: ToolErrorKind::Provider,
                message: "boom".to_string(),
            },
        },
    ]
}

/// The Rust variant identifier for `error` -- exhaustive, so a variant added
/// to [`PlanError`] without a matching arm here fails to compile.
fn plan_error_kind_of(error: &PlanError) -> &'static str {
    match error {
        PlanError::MissingInput { .. } => "MissingInput",
        PlanError::ForEachUnknown { .. } => "ForEachUnknown",
        PlanError::DuplicateForEachKey { .. } => "DuplicateForEachKey",
        PlanError::KeyNotInForEach { .. } => "KeyNotInForEach",
        PlanError::KeyUnknown { .. } => "KeyUnknown",
        PlanError::NameTaken { .. } => "NameTaken",
        PlanError::Tool { .. } => "Tool",
    }
}

/// The number of [`PlanError`] variants today; see [`plan_error_samples`]
/// and [`plan_error_kind_of`].
const PLAN_ERROR_VARIANT_COUNT: usize = 7;

#[test]
fn every_plan_error_variant_serializes_with_its_kind() {
    let samples = plan_error_samples();
    assert_eq!(
        samples.len(),
        PLAN_ERROR_VARIANT_COUNT,
        "plan_error_samples must carry exactly one sample per PlanError variant"
    );
    let mut seen_kinds: HashSet<&'static str> = HashSet::new();
    for sample in &samples {
        let kind = plan_error_kind_of(sample);
        assert!(
            seen_kinds.insert(kind),
            "duplicate sample for PlanError::{kind}"
        );
        let json = serde_json::to_value(sample).expect("PlanError must serialize");
        assert_eq!(json["kind"], kind, "sample: {sample:?}");
        // Internally tagged: the old externally-tagged shape
        // (`{"NameTaken": {...}}`) must be gone.
        assert!(json.get(kind).is_none(), "still externally tagged: {json}");
        // No variant declares a top-level field named `message`: it would
        // collide with the field `willikins_core::Reported` adds. (A field
        // literally named `kind` is refused at compile time by serde's
        // derive under `#[serde(tag = "kind")]`.) `PlanError::Tool`'s own
        // `error` field nests a `message` one level down, inside `error`,
        // never at the top level, so this check still holds for it.
        assert!(
            json.as_object().unwrap().get("message").is_none(),
            "PlanError::{kind} must not have a top-level field named `message`: {json}"
        );
    }
    assert_eq!(seen_kinds.len(), PLAN_ERROR_VARIANT_COUNT);
}

/// `PlanError::Tool`'s nested `error: ToolError` still serializes as its own
/// object (`{"kind": ..., "message": ...}` from `ToolError`'s own,
/// non-tagged `Serialize`), nested under the outer `error` key -- proving
/// the internal tag on `PlanError` does not disturb a field that happens to
/// itself hold something serializing with its own `kind`-shaped object.
#[test]
fn plan_error_tool_nests_the_tool_error_object() {
    let error = PlanError::Tool {
        node: node("n"),
        error: ToolError {
            kind: ToolErrorKind::Conflict,
            message: "already exists".to_string(),
        },
    };
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["kind"], "Tool");
    assert_eq!(json["error"]["kind"], "Conflict");
    assert_eq!(json["error"]["message"], "already exists");
}
