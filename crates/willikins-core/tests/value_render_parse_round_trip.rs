//! Property test: `Value::parse`/`Value::parse_list` round-trips
//! `Value::render` for every non-secret registered type, scalar and
//! list-valued alike.
//!
//! `willikins-server`'s task 10b restart-survival fold
//! (`crates/willikins-server/src/butler.rs`'s `resolve_recorded_inputs`,
//! called from `Butler::apply`) rebuilds a plan's resolved inputs from
//! the journal's own redacted JSON by parsing each recorded input's
//! *rendered* string back through the type registry, rather than keeping
//! a live `Value` in memory that a process restart would lose. That only
//! works if `parse(render(v)) == v` holds for every registered type --
//! this is what this property test checks, using
//! `willikins_core::testing::arb_registered_value` (behind the
//! `test-support` feature, enabled for this crate's own tests via its
//! self dev-dependency; see `Cargo.toml`).
//!
//! If a registered type ever fails this round trip, the fix belongs in
//! that type's own `parse`/`render`, not in a workaround here or in
//! `willikins-server`: pin the failure as its own named test with a
//! comment (see this crate's and `willikins-types`' own test suites)
//! rather than silently excluding the type from this property test.

use proptest::prelude::*;
use willikins_core::Value;
use willikins_core::testing::arb_registered_value;

/// Render `value` and parse it straight back, dispatching on whether it
/// is a scalar or a list the same way
/// `crate::butler::parse_recorded_value` does in `willikins-server`: a
/// list's *elements* are each rendered and reparsed individually, never
/// the comma-joined `Value::render()` display string (which the
/// server-side fold never sees either -- it reads the JSON array of
/// per-element rendered strings that `Value`'s own `Serialize` writes).
fn round_trip(value: &Value) -> Value {
    if let Some(object) = value.as_scalar() {
        let rendered = object.render().to_string();
        Value::parse(value.ty(), &rendered)
            .unwrap_or_else(|err| panic!("{}: re-parsing {rendered:?} failed: {err}", value.ty()))
    } else {
        let items = value
            .as_list()
            .unwrap_or_else(|| panic!("{value:?} is neither a known scalar nor a known list"));
        let rendered: Vec<String> = items
            .iter()
            .map(|object| object.render().to_string())
            .collect();
        let refs: Vec<&str> = rendered.iter().map(String::as_str).collect();
        Value::parse_list(value.ty(), &refs)
            .unwrap_or_else(|err| panic!("{}: re-parsing {rendered:?} failed: {err}", value.ty()))
    }
}

proptest! {
    #[test]
    fn parse_of_render_is_identity_for_every_non_secret_registered_type(
        value in arb_registered_value()
    ) {
        let round_tripped = round_trip(&value);
        prop_assert_eq!(round_tripped.ty(), value.ty());
        prop_assert!(
            round_tripped == value,
            "round trip changed the value: {round_tripped:?} != {value:?}"
        );
    }
}
