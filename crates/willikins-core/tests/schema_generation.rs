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
