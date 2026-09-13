//! [`ApplyError`] serializes internally tagged: every variant's JSON
//! carries `{"kind": "<Variant>", ...its own fields}`. Exhaustive over
//! every variant the same way `plan_error_serde.rs` is for
//! [`willikins_core::PlanError`]; see that file's own doc for why the
//! `variant_kinds!` macro is duplicated here rather than shared.

mod common;

use std::collections::HashSet;

use common::{node, port};
use willikins_core::{
    Action, Applied, ApplyError, Class, DriftKind, PlanError, ToolError, ToolErrorKind,
};

fn empty_applied() -> Applied {
    Applied {
        nodes: Vec::new(),
        outputs: indexmap::IndexMap::new(),
    }
}

/// One instance of every [`ApplyError`] variant.
fn apply_error_samples() -> Vec<ApplyError> {
    vec![
        ApplyError::ApprovalRequired {
            class: Class::Irreversible,
        },
        ApplyError::Plan {
            error: PlanError::MissingInput {
                input: willikins_core::InputName::parse("i").unwrap(),
            },
        },
        ApplyError::Drift {
            node: node("n"),
            instance: None,
            kind: DriftKind::Action {
                planned: Action::Create,
                observed: Action::NoOp,
            },
        },
        ApplyError::UnknownInput {
            node: node("n"),
            port: port("p"),
            from: node("upstream"),
            applied: Box::new(empty_applied()),
        },
        ApplyError::Tool {
            node: node("n"),
            instance: None,
            error: ToolError {
                kind: ToolErrorKind::Provider,
                message: "boom".to_string(),
            },
            applied: Box::new(empty_applied()),
        },
    ]
}

/// Generates a wildcard-free `match` from a variant name to its `kind` tag
/// *and* the variant count, from one list of names — see
/// `plan_error_serde.rs`'s own copy of this macro for the full rationale.
macro_rules! variant_kinds {
    ($fn_name:ident, $count:ident, $enum:ident, $($variant:ident),+ $(,)?) => {
        fn $fn_name(value: &$enum) -> &'static str {
            match value {
                $($enum::$variant { .. } => stringify!($variant),)+
            }
        }

        const $count: usize = [$(stringify!($variant)),+].len();
    };
}

variant_kinds!(
    apply_error_kind_of,
    APPLY_ERROR_VARIANT_COUNT,
    ApplyError,
    ApprovalRequired,
    Plan,
    Drift,
    UnknownInput,
    Tool,
);

#[test]
fn every_apply_error_variant_serializes_with_its_kind() {
    let samples = apply_error_samples();
    assert_eq!(
        samples.len(),
        APPLY_ERROR_VARIANT_COUNT,
        "apply_error_samples must carry exactly one sample per ApplyError variant"
    );
    let mut seen_kinds: HashSet<&'static str> = HashSet::new();
    for sample in &samples {
        let kind = apply_error_kind_of(sample);
        assert!(
            seen_kinds.insert(kind),
            "duplicate sample for ApplyError::{kind}"
        );
        let json = serde_json::to_value(sample).expect("ApplyError must serialize");
        assert_eq!(json["kind"], kind, "sample: {sample:?}");
        assert!(json.get(kind).is_none(), "still externally tagged: {json}");
        assert!(
            json.as_object().unwrap().get("message").is_none(),
            "ApplyError::{kind} must not have a top-level field named `message`: {json}"
        );
    }
    assert_eq!(seen_kinds.len(), APPLY_ERROR_VARIANT_COUNT);
}

/// `Drift`'s own `kind` field (a [`DriftKind`]) is renamed `detail` on the
/// wire so it cannot collide with the outer `ApplyError`'s own internal
/// tag key, which is also named `kind`.
#[test]
fn drift_renames_its_inner_kind_field_to_avoid_the_outer_tag() {
    let error = ApplyError::Drift {
        node: node("n"),
        instance: Some("k".to_string()),
        kind: DriftKind::Action {
            planned: Action::Create,
            observed: Action::NoOp,
        },
    };
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["kind"], "Drift");
    assert_eq!(json["detail"]["kind"], "action");
    assert_eq!(json["detail"]["planned"], "create");
    assert_eq!(json["detail"]["observed"], "noop");
    // Exactly one `kind` key exists at the top level -- serde would have
    // refused to compile a genuine collision, but this proves the actual
    // wire shape carries the renamed field, not two `kind`s silently
    // folded into one by a JSON map.
    assert!(json.as_object().unwrap().contains_key("detail"));
}

#[test]
fn unknown_input_and_tool_carry_the_partial_applied() {
    let unknown_input = ApplyError::UnknownInput {
        node: node("n"),
        port: port("p"),
        from: node("upstream"),
        applied: Box::new(empty_applied()),
    };
    let json = serde_json::to_value(&unknown_input).unwrap();
    assert!(json["applied"]["nodes"].as_array().unwrap().is_empty());

    let tool = ApplyError::Tool {
        node: node("n"),
        instance: None,
        error: ToolError {
            kind: ToolErrorKind::Provider,
            message: "boom".to_string(),
        },
        applied: Box::new(empty_applied()),
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["error"]["kind"], "Provider");
    assert_eq!(json["error"]["message"], "boom");
}

#[test]
fn every_variant_displays_a_non_empty_message() {
    for sample in apply_error_samples() {
        let message = sample.to_string();
        assert!(!message.is_empty(), "{sample:?} displayed an empty message");
    }
}
