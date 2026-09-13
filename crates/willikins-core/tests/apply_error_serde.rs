//! [`ApplyError`] serializes internally tagged: every variant's JSON
//! carries `{"kind": "<Variant>", ...its own fields}`. Exhaustive over
//! every variant the same way `plan_error_serde.rs` is for
//! [`willikins_core::PlanError`]; see that file's own doc for why the
//! `variant_kinds!` macro is duplicated here rather than shared.

mod common;

use std::collections::HashSet;

use common::{node, port};
use willikins_core::{
    Action, Applied, ApplyError, Class, DriftKind, InstanceRef, PlanError, ToolError,
    ToolErrorKind, Value,
};
use willikins_types::DomainType;

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
            kind: Box::new(DriftKind::Action {
                planned: Action::Create,
                observed: Action::NoOp,
            }),
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
        kind: Box::new(DriftKind::Action {
            planned: Action::Create,
            observed: Action::NoOp,
        }),
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

/// Every [`DriftKind`] variant serializes internally tagged too, nested
/// under `Drift`'s `detail` key, with its own `snake_case` tag. The
/// wildcard-free `match` is the exhaustiveness guard: a new variant fails
/// to compile until it is listed here with a sample of its own.
#[test]
fn every_drift_kind_variant_serializes_with_its_own_snake_case_tag() {
    let samples = vec![
        DriftKind::Instance {
            planned: Some(InstanceRef {
                node: node("n"),
                instance: Some("dev".to_string()),
            }),
            observed: None,
        },
        DriftKind::Action {
            planned: Action::Create,
            observed: Action::NoOp,
        },
        DriftKind::Output {
            port: port("p"),
            planned: Value::known(willikins_types::GitHubOrg::parse("lightless-labs").unwrap()),
            observed: Value::known(willikins_types::GitHubOrg::parse("other-org").unwrap()),
        },
    ];

    for kind in samples {
        let tag = match &kind {
            DriftKind::Instance { .. } => "instance",
            DriftKind::Action { .. } => "action",
            DriftKind::Output { .. } => "output",
        };
        let error = ApplyError::Drift {
            node: node("n"),
            instance: None,
            kind: Box::new(kind),
        };
        let json: serde_json::Value = serde_json::to_value(&error).expect("ApplyError serializes");
        assert_eq!(json["kind"], "Drift", "{json}");
        assert_eq!(json["detail"]["kind"], tag, "{json}");
    }
}

/// `DriftKind::Output` holds two [`Value`]s, and a `Value` renders through
/// its own redaction whatever it holds — so even a `Drift` hand-built
/// around a secret (which `Plan::fingerprint` never produces, since a
/// secret port contributes a fixed marker instead of its value) cannot
/// print one.
#[test]
fn a_drift_kind_output_holding_a_secret_still_serializes_redacted() {
    let token =
        willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7)))
            .expect("a valid token literal");
    let error = ApplyError::Drift {
        node: node("n"),
        instance: None,
        kind: Box::new(DriftKind::Output {
            port: port("value"),
            planned: Value::known(token.clone()),
            observed: Value::known(token),
        }),
    };
    let json = serde_json::to_string(&error).expect("ApplyError serializes");
    assert!(!json.contains(&"MARKER".repeat(7)), "leaked: {json}");
    assert!(json.contains("REDACTED"), "{json}");
    let debug = format!("{error:?}");
    assert!(!debug.contains(&"MARKER".repeat(7)), "leaked: {debug}");
    assert!(
        !error.to_string().contains(&"MARKER".repeat(7)),
        "Display leaked: {error}"
    );
}
