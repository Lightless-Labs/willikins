//! Attacks on `willikins-core`'s public API from outside the crate.
//!
//! `tests/redaction.rs` is acceptance test 8a: a secret does not leak
//! through the containers a tool actually hands back. This file is the
//! adversarial companion: it wraps a secret [`Value`] in the *ad hoc*
//! containers a caller might reach for (`Vec`, `IndexMap`, `Option`,
//! `Result`) and asserts their derived `Debug` still redacts, and it
//! attacks the type model — the registry's secret refusal, `AnySecret`
//! port matching, `TypeRef` parsing, spec validation, and catalog
//! uniqueness — from the outside, where only the public API is reachable.
//!
//! Every secret here carries a canary substring that appears nowhere else
//! in the crate, so a leak cannot be masked by an unrelated match.

use std::sync::Arc;

use indexmap::IndexMap;

use willikins_core::tool::{Ensured, Observation, Outputs, PortSpec};
use willikins_core::{
    Catalog, CatalogError, Class, PortName, PortType, SpecError, ToolError, ToolErrorKind,
    ToolName, ToolSpec, TypeName, TypeRef, Value,
};
use willikins_types::{DomainType, DopplerServiceToken, ProjectSlug, Rendered};

/// A substring that must never appear in any rendering of a secret.
const CANARY: &str = "leakcanary7f3a";

/// A well-formed `DopplerServiceToken` carrying [`CANARY`], padded out to
/// the 40-44 character alphanumeric suffix Doppler's real tokens use.
fn token_text() -> String {
    format!("dp.st.prd.{CANARY}bbbbbbbbbbbbbbbbbbbbbbbbbbbb")
}

fn secret_value() -> Value {
    Value::known(DopplerServiceToken::parse(&token_text()).expect("a well-formed token"))
}

fn type_name(name: &str) -> TypeName {
    TypeName::parse(name).expect("a valid type name")
}

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a valid port name")
}

/// Assert `haystack` carries the redaction marker and never the canary.
fn assert_redacted(what: &str, haystack: &str) {
    assert!(
        !haystack.contains(CANARY),
        "{what} leaked the secret: {haystack}"
    );
    assert!(
        haystack.contains("[REDACTED DopplerServiceToken]"),
        "{what} did not show the redaction marker: {haystack}"
    );
}

// ---------------------------------------------------------------------
// Derived `Debug` of ad hoc containers
// ---------------------------------------------------------------------

#[test]
fn debug_of_a_vec_of_values_redacts_in_both_forms() {
    let values = vec![secret_value(), secret_value()];
    assert_redacted("Vec<Value> {:?}", &format!("{values:?}"));
    assert_redacted("Vec<Value> {:#?}", &format!("{values:#?}"));
}

#[test]
fn debug_of_an_index_map_of_port_to_value_redacts_in_both_forms() {
    let mut map: IndexMap<PortName, Value> = IndexMap::new();
    map.insert(port("token"), secret_value());
    assert_redacted("IndexMap<PortName, Value> {:?}", &format!("{map:?}"));
    assert_redacted("IndexMap<PortName, Value> {:#?}", &format!("{map:#?}"));
}

#[test]
fn debug_of_an_option_of_value_redacts_in_both_forms() {
    let value = Some(secret_value());
    assert_redacted("Option<Value> {:?}", &format!("{value:?}"));
    assert_redacted("Option<Value> {:#?}", &format!("{value:#?}"));
}

#[test]
fn debug_of_a_result_holding_a_value_redacts_in_both_forms() {
    let ok: Result<Value, ToolError> = Ok(secret_value());
    assert_redacted("Result<Value, ToolError> {:?}", &format!("{ok:?}"));
    assert_redacted("Result<Value, ToolError> {:#?}", &format!("{ok:#?}"));

    // The `Err` arm carries no value at all, by the `ToolError` contract.
    let err: Result<Value, ToolError> = Err(ToolError {
        kind: ToolErrorKind::Provider,
        message: "the provider refused the request".to_string(),
    });
    assert!(!format!("{err:?}").contains(CANARY));
    assert!(!format!("{err:#?}").contains(CANARY));
}

#[test]
fn debug_of_deeply_nested_containers_still_redacts() {
    let nested: Vec<Option<Result<Value, ToolError>>> = vec![Some(Ok(secret_value()))];
    assert_redacted("Vec<Option<Result<..>>> {:#?}", &format!("{nested:#?}"));
}

#[test]
fn a_known_list_of_secrets_redacts_every_element() {
    // The one shape in the type model the containers above do not reach:
    // secrecy is a property of the element type, so a `list<Secret>` must
    // redact through `Debug` and through every element of its JSON array.
    let values = Value::known_list(vec![
        DopplerServiceToken::parse(&token_text()).expect("a well-formed token"),
        DopplerServiceToken::parse(&token_text()).expect("a well-formed token"),
    ]);
    assert!(values.is_secret());
    assert_redacted("list<Secret> {:#?}", &format!("{values:#?}"));
    let json = serde_json::to_string(&values).expect("a Value serializes");
    assert_redacted("list<Secret> JSON", &json);
    assert!(json.contains("\"list\":true"), "{json}");
    assert!(json.contains("\"redacted\":true"), "{json}");
}

// ---------------------------------------------------------------------
// Serde of an `Observation`
// ---------------------------------------------------------------------

fn outputs_with_a_secret() -> Outputs {
    let mut outputs = Outputs::new();
    outputs.insert(port("token"), secret_value());
    outputs
}

#[test]
fn json_of_an_absent_observation_redacts_its_predicted_secret() {
    let observation = Observation::Absent {
        predicted: outputs_with_a_secret(),
    };
    let json = serde_json::to_string(&observation).expect("an Observation serializes");
    assert_redacted("Observation::Absent JSON", &json);
    assert!(json.contains("\"redacted\":true"), "{json}");

    // `serde_json::to_value` takes the same path; assert it separately
    // because a `Value` tree is what an MCP response is actually built from.
    let tree = serde_json::to_value(&observation).expect("an Observation serializes to a tree");
    assert_redacted("Observation::Absent value tree", &tree.to_string());
}

#[test]
fn json_of_a_present_observation_redacts_its_secret() {
    let observation = Observation::Present(outputs_with_a_secret());
    let json = serde_json::to_string(&observation).expect("an Observation serializes");
    assert_redacted("Observation::Present JSON", &json);
}

#[test]
fn json_of_an_unknown_secret_output_carries_neither_a_value_nor_the_canary() {
    let mut outputs = Outputs::new();
    outputs.insert(
        port("token"),
        Value::unknown(TypeRef::scalar(type_name("DopplerServiceToken"))),
    );
    let json = serde_json::to_string(&Observation::Absent { predicted: outputs })
        .expect("an Observation serializes");
    assert!(!json.contains(CANARY), "{json}");
    assert!(!json.contains("\"value\""), "{json}");
}

// ---------------------------------------------------------------------
// The registry's secret refusal, seen through `Value`
// ---------------------------------------------------------------------

#[test]
fn parse_of_a_secret_type_is_refused_before_the_input_is_validated() {
    // The input is not even a well-formed token, so a refusal that ran the
    // type's own parser would talk about the pattern and quote the input.
    let ty = TypeRef::scalar(type_name("DopplerServiceToken"));
    let err = Value::parse(&ty, &format!("{CANARY}-not-a-token")).expect_err("secrets are refused");
    let rendered = err.to_string();
    assert!(!rendered.contains(CANARY), "{rendered}");
    assert!(!rendered.contains("pattern"), "{rendered}");
    assert!(
        rendered.contains("secret types cannot be supplied as literals or inputs"),
        "{rendered}"
    );
}

#[test]
fn parse_list_of_a_secret_type_is_refused_even_with_no_inputs() {
    let ty = TypeRef::list_of(type_name("DopplerServiceToken"));
    let err = Value::parse_list(&ty, &[]).expect_err("secrets are refused");
    assert!(
        err.reason
            .contains("secret types cannot be supplied as literals or inputs"),
        "{}",
        err.reason
    );
}

#[test]
fn parse_list_of_a_non_secret_type_with_no_inputs_is_a_known_empty_list() {
    let ty = TypeRef::list_of(type_name("GitHubOrg"));
    let value = Value::parse_list(&ty, &[]).expect("an empty list of a public type is fine");
    assert!(value.is_known());
    assert!(value.as_list().expect("a known list").is_empty());
    assert!(!value.is_secret());
}

// ---------------------------------------------------------------------
// `PortType::AnySecret`
// ---------------------------------------------------------------------

#[test]
fn any_secret_accepts_an_unknown_valued_secret_by_its_declared_type() {
    // An `AnySecret` sink port must still accept a value that has not been
    // produced yet: at check time a secret output is always `Unknown`.
    let unknown = Value::unknown(TypeRef::scalar(type_name("DopplerServiceToken")));
    assert!(!unknown.is_known());
    assert!(unknown.is_secret());
    assert!(PortType::AnySecret.accepts(unknown.ty(), willikins_types::registry()));
}

#[test]
fn any_secret_rejects_a_non_secret_scalar_and_a_list_of_secrets() {
    let public = TypeRef::scalar(type_name("GitHubOrg"));
    let secret_list = TypeRef::list_of(type_name("DopplerServiceToken"));
    assert!(!PortType::AnySecret.accepts(&public, willikins_types::registry()));
    assert!(!PortType::AnySecret.accepts(&secret_list, willikins_types::registry()));
}

// ---------------------------------------------------------------------
// `TypeRef` parsing
// ---------------------------------------------------------------------

#[test]
fn type_ref_parse_accepts_only_the_exact_list_spelling() {
    assert!(TypeRef::parse("list<GitHubOrg>").is_ok());
    assert!(TypeRef::parse("list< GitHubOrg >").is_err());
    assert!(TypeRef::parse("list<GitHubOrg").is_err());
    assert!(TypeRef::parse("<GitHubOrg>").is_err());
}

// ---------------------------------------------------------------------
// `ToolSpec::validate` and `Catalog`
// ---------------------------------------------------------------------

fn spec_with(key: Vec<PortName>, pure: bool, class: Class) -> ToolSpec {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("org"),
        PortSpec {
            ty: PortType::Exact(TypeRef::scalar(type_name("GitHubOrg"))),
            required: true,
        },
    );
    ToolSpec {
        name: ToolName::parse("test.spec").expect("a valid tool name"),
        description: "A spec built outside the crate.".to_string(),
        inputs,
        outputs: IndexMap::new(),
        key,
        class,
        pure,
    }
}

#[test]
fn validate_rejects_a_key_port_that_is_not_an_input() {
    let spec = spec_with(vec![port("not_an_input")], false, Class::Reversible);
    assert!(matches!(
        spec.validate(willikins_types::registry()),
        Err(SpecError::KeyNotAnInput { .. })
    ));
}

#[test]
fn validate_rejects_a_pure_tool_with_a_key() {
    let spec = spec_with(vec![port("org")], true, Class::Reversible);
    assert!(matches!(
        spec.validate(willikins_types::registry()),
        Err(SpecError::PureToolWithKey)
    ));
}

/// A tool wrapping whatever spec it is handed, so a catalog can be attacked
/// from outside the crate.
struct SpecOnlyTool(ToolSpec);

impl willikins_core::Tool for SpecOnlyTool {
    fn spec(&self) -> &ToolSpec {
        &self.0
    }

    fn read(&self, _inputs: &willikins_core::Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Foreign)
    }

    fn ensure(
        &self,
        _inputs: &willikins_core::Inputs,
        _token: &willikins_core::SinkToken,
    ) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}

#[test]
fn catalog_rejects_a_second_tool_with_the_same_name() {
    let mut catalog = Catalog::new(willikins_types::registry());
    let first = SpecOnlyTool(spec_with(Vec::new(), false, Class::Reversible));
    let second = SpecOnlyTool(spec_with(Vec::new(), false, Class::Destructive));
    catalog.insert(Arc::new(first)).expect("the first insert");
    let err = catalog
        .insert(Arc::new(second))
        .expect_err("a duplicate name is refused");
    assert!(matches!(err, CatalogError::Duplicate(name) if name.as_str() == "test.spec"));
}

// ---------------------------------------------------------------------
// A `WordList`-backed domain type keeps its canonical kebab rendering
// ---------------------------------------------------------------------

#[test]
fn a_word_list_backed_slug_renders_as_its_kebab_string() {
    let slug = ProjectSlug::parse("third-thoughts").expect("a valid project slug");
    let value = Value::known(slug);
    assert_eq!(
        value.render(),
        Rendered::Plain("third-thoughts".to_string())
    );
    assert_eq!(format!("{value:?}"), "third-thoughts");
    assert_eq!(
        serde_json::to_value(&value).expect("a Value serializes")["value"],
        "third-thoughts"
    );
}

// ---------------------------------------------------------------------
// `Value` equality
// ---------------------------------------------------------------------

#[test]
fn a_known_secret_is_not_equal_to_an_unknown_of_the_same_type() {
    let known = secret_value();
    let unknown = Value::unknown(TypeRef::scalar(type_name("DopplerServiceToken")));
    assert_ne!(known, unknown);
    assert_ne!(unknown, known);
}
