//! Now that `CheckError` and `PlanError` serialize *every* field an agent
//! sees (`#[serde(tag = "kind")]` plus
//! [`Reported`](willikins_core::Reported)'s `message`), rather than the
//! hand-picked `kind`/`message` pair the CLI used to build, every field on
//! every variant is a new place a secret could surface.
//!
//! This file attacks the three shapes that can carry content rather than
//! only identifiers:
//!
//! - `CheckError::InvalidLiteral`'s [`ParseError`], for the case where the
//!   rejected literal was handed to a *secret* type's parser;
//! - `PlanError::NameTaken`'s `key: Inputs`, the one error field in either
//!   type that holds [`Value`]s;
//! - the `key: String` fields (`PlanError::KeyNotInForEach`,
//!   `PlanError::DuplicateForEachKey`,
//!   `CheckError::DuplicateForEachDefault`), which hold a `for_each`
//!   item's canonical string.
//!
//! Every secret here carries a canary substring that appears nowhere else
//! in the workspace, so a leak cannot be masked by an unrelated match. The
//! assertions cover both renderings an agent can read: the serialized
//! `Reported` JSON and the error's own `Display`.

mod common;

use common::{input, node, port, tool_name, ty};
use willikins_core::{CheckError, Inputs, PlanError, PortType, Reported, Value};
use willikins_types::{DomainType, DopplerServiceToken, ParseError};

/// A substring that must never appear in any rendering of a secret.
const CANARY: &str = "leakcanary91b4e";

/// A well-formed `DopplerServiceToken` carrying [`CANARY`]. Padded to a
/// 40-character alphanumeric tail so it still parses once task 0 tightens
/// the pattern to Doppler's documented
/// `dp\.st\.(?:[a-z0-9\-_]{2,35}\.)?[a-zA-Z0-9]{40,44}` (the plan's
/// "willikins-types (changes)" section).
fn token_text() -> String {
    let tail = format!("{CANARY}{}", "a".repeat(40 - CANARY.len()));
    format!("dp.st.prd.{tail}")
}

/// The marker a redacted `DopplerServiceToken` shows instead.
const MARKER: &str = "[REDACTED DopplerServiceToken]";

/// Both renderings an agent reads: the `Reported` JSON (`{"kind", "message",
/// ...fields}`) and the value's own `Display`.
fn renderings<T: serde::Serialize + std::fmt::Display>(value: &T) -> (String, String) {
    let json = serde_json::to_string(&Reported::new(value)).expect("must serialize");
    (json, value.to_string())
}

fn assert_no_canary(label: &str, json: &str, display: &str) {
    assert!(!json.contains(CANARY), "{label}: JSON leaked: {json}");
    assert!(
        !display.contains(CANARY),
        "{label}: Display leaked: {display}"
    );
}

/// A literal bound to a secret-typed port is refused as
/// `CheckError::SecretLiteral` before any parser sees it, so `check` itself
/// never builds this error. A caller constructing `CheckError` by hand can,
/// though, and the JSON now carries the `ParseError` verbatim — so the
/// guarantee has to hold at the type level: a secret type's parser reports
/// only its constraint, never its input.
#[test]
fn invalid_literal_from_a_secret_types_failed_parse_carries_no_bytes() {
    let literal = format!("dp.st.{CANARY}!!!");
    let error = DopplerServiceToken::parse(&literal).expect_err("`!` is outside the pattern");
    let check_error = CheckError::InvalidLiteral {
        node: node("token"),
        port: port("value"),
        error,
    };

    let (json, display) = renderings(&check_error);
    assert_no_canary("InvalidLiteral", &json, &display);
    // The error still says which type rejected it, so the report is not
    // merely empty.
    assert!(json.contains("DopplerServiceToken"), "json: {json}");
    assert!(json.contains("\"kind\":\"InvalidLiteral\""), "json: {json}");
}

/// `TypeMismatch` carries only type names (`PortType`, `TypeRef`), never the
/// offending value — pinned here so a future revision that adds a `found`
/// *value* to it has to break this test on its way past.
#[test]
fn type_mismatch_carries_type_names_and_no_value() {
    let check_error = CheckError::TypeMismatch {
        node: node("readme"),
        port: port("value"),
        expected: PortType::Exact(ty("Text")),
        found: ty("DopplerServiceToken"),
    };

    let (json, display) = renderings(&check_error);
    assert_no_canary("TypeMismatch", &json, &display);
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    // Sorted, because `serde_json::Map` is a `BTreeMap` here: this asserts
    // the *set* of fields, not the order they go out on the wire.
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["expected", "found", "kind", "message", "node", "port"],
        "TypeMismatch's JSON gained a field: {json}"
    );
}

/// `PlanError::NameTaken`'s `key: Inputs` is the one error field in either
/// error type that holds [`Value`]s. A key port is never secret in any
/// milestone-2 tool, but nothing in the *type* says so, and `Inputs` is
/// serialized whole into the error an agent reads — so the redaction has to
/// come from `Value`'s own `Serialize`, which it does.
#[test]
fn name_taken_redacts_a_secret_bound_at_a_key_port() {
    let token =
        DopplerServiceToken::parse(&token_text()).expect("a well-formed token carrying the canary");
    let mut key = Inputs::new();
    key.insert(port("token"), Value::known(token));

    let plan_error = PlanError::NameTaken {
        node: node("ci_secret"),
        tool: tool_name("github.actions_secret.ensure"),
        key,
    };

    let (json, display) = renderings(&plan_error);
    assert_no_canary("NameTaken", &json, &display);
    assert!(
        json.contains(MARKER),
        "the redaction marker must be there in place of the bytes: {json}"
    );
    assert!(json.contains("\"kind\":\"NameTaken\""), "json: {json}");
}

/// The `key: String` fields hold a `for_each` item's *canonical string* —
/// the output of `Value::render`, which is the redaction marker for a secret
/// (`plan.rs`'s `value.render().to_string()`, `check.rs`'s
/// `object.render().to_string()`). A secret `for_each` source is refused by
/// `CheckError::SecretForEachSource` long before either error is built, so
/// this pins the weaker property that actually matters: whatever string a
/// key carries is echoed verbatim and nothing reconstitutes bytes from it.
#[test]
fn a_key_string_is_echoed_verbatim_and_a_redacted_one_stays_redacted() {
    for (label, key) in [
        ("redacted", MARKER.to_string()),
        ("plain", "prd".to_string()),
    ] {
        for error in [
            PlanError::KeyNotInForEach {
                node: node("token"),
                key: key.clone(),
            },
            PlanError::DuplicateForEachKey {
                node: node("configs"),
                key: key.clone(),
            },
        ] {
            let (json, display) = renderings(&error);
            assert_no_canary(label, &json, &display);
            assert!(json.contains(&key), "{label}: key not echoed: {json}");
        }
        let check_error = CheckError::DuplicateForEachDefault {
            node: node("configs"),
            input: input("environments"),
            key: key.clone(),
        };
        let (json, display) = renderings(&check_error);
        assert_no_canary(label, &json, &display);
        assert!(json.contains(&key), "{label}: key not echoed: {json}");
    }
}

/// A [`ParseError`] built by hand from a non-secret type's parser may quote
/// the rejected text, which is intended (it is the caller's own value). This
/// pins that the intent is a *type-level* distinction, not something
/// `CheckError` filters: the same variant carries the quote through.
#[test]
fn invalid_literal_from_a_non_secret_type_does_quote_its_input() {
    let check_error = CheckError::InvalidLiteral {
        node: node("names"),
        port: port("slug"),
        error: ParseError::new("ProjectSlug", "must match `[a-z][a-z0-9-]*`: `Nope!`"),
    };
    let (json, _) = renderings(&check_error);
    assert!(json.contains("Nope!"), "json: {json}");
}
