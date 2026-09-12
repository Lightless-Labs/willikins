//! Every result type the milestone 2 MCP surface returns through
//! `rmcp::Json<T>` must generate a JSON schema without panicking, and the
//! generated schema is pinned by an insta snapshot so a change to any of
//! these public shapes is a reviewed diff, not a silent one.

use willikins_core::{
    Action, Description, InputError, Inputs, MissingInput, Outputs, Plan, PlannedNode,
};

/// Generate and snapshot `T`'s schema, panicking with `T`'s name in the
/// message (via `$ty`) if generation itself panics -- `schema_for!` already
/// does the "without panicking" half just by being called here at all.
macro_rules! schema_snapshot {
    ($fn_name:ident, $ty:ty) => {
        #[test]
        fn $fn_name() {
            let schema = schemars::schema_for!($ty);
            insta::assert_json_snapshot!(schema);
        }
    };
}

schema_snapshot!(plan_schema_generates, Plan);
schema_snapshot!(planned_node_schema_generates, PlannedNode);
schema_snapshot!(action_schema_generates, Action);
schema_snapshot!(description_schema_generates, Description);
schema_snapshot!(missing_input_schema_generates, MissingInput);
schema_snapshot!(input_error_schema_generates, InputError);
schema_snapshot!(inputs_schema_generates, Inputs);
schema_snapshot!(outputs_schema_generates, Outputs);

/// [`MissingInput`] holds a `schemars::Schema` (the registry's schema for
/// the missing input's own type) in a field that is itself published through
/// `MissingInput`'s generated schema. Two things have to hold and neither
/// is implied by the snapshot above:
///
/// - the generated schema is a real object schema, not the empty `{}` a
///   `schema_with` attribute silently degrades to when it names the wrong
///   thing;
/// - a `schemars::Schema` *value* survives `serde_json` round-tripping, so
///   the `rmcp::Json<Description>` an MCP client reads carries the type
///   schema rather than failing to serialize it.
#[test]
fn missing_input_publishes_a_non_empty_schema_and_its_schema_field_round_trips() {
    let schema = schemars::schema_for!(MissingInput);
    let json = schema.as_value();
    let properties = json["properties"]
        .as_object()
        .expect("MissingInput's schema must publish its properties");
    for field in [
        "name",
        "ty",
        "schema",
        "description",
        "default",
        "example",
        "prompt",
    ] {
        assert!(
            properties.contains_key(field),
            "MissingInput's schema is missing `{field}`: {json}"
        );
    }
    assert_eq!(
        properties["schema"]["type"], "object",
        "the `schema` field must publish as an object: {json}"
    );

    // The field's own runtime value survives serialization: a
    // `schemars::Schema` is just a JSON document, and this is the shape the
    // MCP surface hands a client.
    let type_schema = <willikins_types::GitHubOrg as willikins_types::DomainType>::json_schema();
    let text = serde_json::to_string(&type_schema).expect("a Schema must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(
        parsed.is_object(),
        "a type schema must serialize as a JSON object: {text}"
    );
    assert!(!text.is_empty());
}
