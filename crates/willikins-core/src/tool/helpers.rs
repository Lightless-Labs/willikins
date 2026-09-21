//! Generic tool-authoring helpers: port-name and type-reference
//! construction, the two input checks every `Tool` implementation
//! performs before consulting its own state, and the `ToolError`
//! constructors those checks (and providers) raise.
//!
//! Every fake tool in `willikins-providers-fake`, both pure tools in
//! `willikins-tools`, and the live providers built on top of them share
//! this module rather than re-deriving it: it depends on nothing but
//! [`crate::tool`], [`crate::value`], and `willikins_types::DomainType`.

use crate::tool::{Inputs, PortName, PortSpec, ToolError, ToolErrorKind, ToolName, ToolSpec};
use crate::value::{PortType, TypeName, TypeRef, Value};
use willikins_types::DomainType;

/// Parse a port name literal known to be valid at compile time.
///
/// # Panics
///
/// Panics if `name` does not match [`PortName`]'s pattern — a bug in the
/// calling tool's own spec, never in caller input.
pub fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap_or_else(|err| unreachable!("bad port name {name:?}: {err}"))
}

/// Parse a tool name literal known to be valid at compile time.
///
/// Kept as its own function (rather than inlining `ToolName::parse(...).unwrap()`
/// at each call site) so the panic clippy's `missing_panics_doc` looks for
/// stays inside this one helper instead of surfacing in every tool's own
/// `new`.
///
/// # Panics
///
/// Panics if `name` does not match [`ToolName`]'s pattern — a bug in the
/// calling tool's own spec, never in caller input.
pub fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap_or_else(|err| unreachable!("bad tool name {name:?}: {err}"))
}

/// Parse a type name literal known to be valid at compile time.
///
/// # Panics
///
/// Panics if `name` does not match [`TypeName`]'s pattern — a bug in the
/// calling tool's own spec, never in caller input.
fn type_name(name: &str) -> TypeName {
    TypeName::parse(name).unwrap_or_else(|err| unreachable!("bad type name {name:?}: {err}"))
}

/// A scalar [`TypeRef`] for the type named `name`.
pub fn scalar(name: &str) -> TypeRef {
    TypeRef::scalar(type_name(name))
}

/// A `list<name>` [`TypeRef`].
pub fn list(name: &str) -> TypeRef {
    TypeRef::list_of(type_name(name))
}

/// A required or optional [`PortSpec`] accepting exactly the scalar type
/// `name`.
pub fn exact(name: &str, required: bool) -> PortSpec {
    PortSpec {
        ty: PortType::Exact(scalar(name)),
        required,
        derived_only: false,
    }
}

/// A required or optional [`PortSpec`] accepting any secret scalar type.
pub fn any_secret(required: bool) -> PortSpec {
    PortSpec {
        ty: PortType::AnySecret,
        required,
        derived_only: false,
    }
}

/// A required [`PortSpec`] accepting exactly the scalar type `name`, which
/// `check` refuses to bind to anything but the output of an earlier,
/// non-pure node — see [`PortSpec::derived_only`]. Always required: a port
/// a document may leave unbound has no provenance to police.
pub fn exact_derived_only(name: &str) -> PortSpec {
    PortSpec {
        ty: PortType::Exact(scalar(name)),
        required: true,
        derived_only: true,
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::Invalid`].
pub fn invalid(message: impl Into<String>) -> ToolError {
    ToolError {
        kind: ToolErrorKind::Invalid,
        message: message.into(),
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::NotFound`].
pub fn not_found(message: impl Into<String>) -> ToolError {
    ToolError {
        kind: ToolErrorKind::NotFound,
        message: message.into(),
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::Conflict`].
pub fn conflict(message: impl Into<String>) -> ToolError {
    ToolError {
        kind: ToolErrorKind::Conflict,
        message: message.into(),
    }
}

/// Check that every one of `spec`'s required input ports is present in
/// `inputs`, regardless of whether the tool's own logic goes on to consult
/// its value.
///
/// # Errors
///
/// Returns [`ToolError`] of kind [`ToolErrorKind::Invalid`] naming the
/// first missing required port.
pub fn require_present(spec: &ToolSpec, inputs: &Inputs) -> Result<(), ToolError> {
    for (name, port_spec) in &spec.inputs {
        if port_spec.required && inputs.get(name).is_none() {
            return Err(invalid(format!("port `{name}` is required")));
        }
    }
    Ok(())
}

/// Read a required, known, typed input by its port name.
///
/// Fails the same way whether the port is altogether missing, present but
/// [`Unknown`](crate::value::ValueState::Unknown), or bound to a value of
/// the wrong domain type — every case a tool's `read` or `ensure` cannot
/// proceed without the concrete value in hand.
///
/// # Errors
///
/// Returns [`ToolError`] of kind [`ToolErrorKind::Invalid`] naming `name`
/// in each of the three cases above.
pub fn get<T: DomainType + 'static>(inputs: &Inputs, name: &str) -> Result<T, ToolError> {
    let value = inputs
        .get(&port(name))
        .ok_or_else(|| invalid(format!("port `{name}` is required")))?;
    known(value, name)
}

/// Recover a known, typed value already in hand, failing the way [`get`]
/// does for the "unknown" and "wrong type" cases.
///
/// # Errors
///
/// Returns [`ToolError`] of kind [`ToolErrorKind::Invalid`] naming `name`.
pub fn known<T: DomainType + 'static>(value: &Value, name: &str) -> Result<T, ToolError> {
    if !value.is_known() {
        return Err(invalid(format!("port `{name}` is unknown")));
    }
    value
        .downcast::<T>()
        .cloned()
        .ok_or_else(|| invalid(format!("port `{name}` has an unexpected type")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::Class;
    use willikins_types::GitHubOrg;

    #[test]
    fn port_parses_a_valid_literal() {
        assert_eq!(port("org").as_str(), "org");
    }

    #[test]
    #[should_panic(expected = "bad port name")]
    fn port_panics_on_an_invalid_literal() {
        port("Not Valid");
    }

    #[test]
    fn tool_name_parses_a_valid_literal() {
        assert_eq!(tool_name("naming.v1").as_str(), "naming.v1");
    }

    #[test]
    #[should_panic(expected = "bad tool name")]
    fn tool_name_panics_on_an_invalid_literal() {
        tool_name("Not Valid");
    }

    #[test]
    fn scalar_builds_a_scalar_type_ref() {
        assert_eq!(
            scalar("GitHubOrg"),
            TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap())
        );
    }

    #[test]
    fn list_builds_a_list_type_ref() {
        assert_eq!(
            list("GitHubOrg"),
            TypeRef::list_of(TypeName::parse("GitHubOrg").unwrap())
        );
    }

    #[test]
    fn exact_builds_a_port_spec_with_the_given_requiredness() {
        let required = exact("GitHubOrg", true);
        assert_eq!(required.ty, PortType::Exact(scalar("GitHubOrg")));
        assert!(required.required);
        let optional = exact("GitHubOrg", false);
        assert!(!optional.required);
    }

    #[test]
    fn any_secret_builds_an_any_secret_port_spec() {
        let spec = any_secret(true);
        assert_eq!(spec.ty, PortType::AnySecret);
        assert!(spec.required);
    }

    #[test]
    fn invalid_not_found_and_conflict_tag_the_right_kind() {
        assert_eq!(invalid("x").kind, ToolErrorKind::Invalid);
        assert_eq!(not_found("x").kind, ToolErrorKind::NotFound);
        assert_eq!(conflict("x").kind, ToolErrorKind::Conflict);
    }

    fn dummy_spec() -> ToolSpec {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("GitHubOrg", true));
        inputs.insert(port("nickname"), exact("GitHubOrg", false));
        ToolSpec {
            name: tool_name("test.dummy"),
            description: "A dummy tool for helper tests.".to_string(),
            inputs,
            outputs: indexmap::IndexMap::new(),
            key: Vec::new(),
            class: Class::Reversible,
            pure: true,
        }
    }

    #[test]
    fn require_present_accepts_every_required_port_bound() {
        let mut inputs = Inputs::new();
        inputs.insert(
            port("org"),
            Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
        );
        require_present(&dummy_spec(), &inputs).unwrap();
    }

    #[test]
    fn require_present_ignores_a_missing_optional_port() {
        let mut inputs = Inputs::new();
        inputs.insert(
            port("org"),
            Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
        );
        require_present(&dummy_spec(), &inputs).unwrap();
    }

    #[test]
    fn require_present_rejects_a_missing_required_port() {
        let err = require_present(&dummy_spec(), &Inputs::new()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
        assert!(err.message.contains("org"), "{}", err.message);
    }

    #[test]
    fn get_returns_the_bound_value() {
        let mut inputs = Inputs::new();
        inputs.insert(
            port("org"),
            Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
        );
        let org: GitHubOrg = get(&inputs, "org").unwrap();
        assert_eq!(org.as_str(), "lightless-labs");
    }

    #[test]
    fn get_rejects_a_missing_port() {
        let err = get::<GitHubOrg>(&Inputs::new(), "org").unwrap_err();
        assert!(err.message.contains("org"), "{}", err.message);
    }

    #[test]
    fn get_rejects_an_unknown_value() {
        let mut inputs = Inputs::new();
        inputs.insert(port("org"), Value::unknown(scalar("GitHubOrg")));
        let err = get::<GitHubOrg>(&inputs, "org").unwrap_err();
        assert!(err.message.contains("unknown"), "{}", err.message);
    }

    #[test]
    fn get_rejects_a_value_of_the_wrong_type() {
        let mut inputs = Inputs::new();
        // Bind a `ProjectSlug`-shaped unknown-but-wrong-downcast case by
        // reusing a known value of a different domain type at the same
        // port name.
        inputs.insert(
            port("org"),
            Value::known(willikins_types::ProjectSlug::parse("third-thoughts").unwrap()),
        );
        let err = get::<GitHubOrg>(&inputs, "org").unwrap_err();
        assert!(err.message.contains("unexpected type"), "{}", err.message);
    }
}
