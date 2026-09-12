//! Shared helpers every fake tool's [`Tool`](willikins_core::Tool) impl
//! builds on: port-name and type-reference construction, and the two input
//! checks every tool performs before consulting its state.

use willikins_core::{
    Inputs, PortName, PortSpec, PortType, ToolError, ToolErrorKind, ToolName, ToolSpec, TypeName,
    TypeRef, Value,
};
use willikins_types::DomainType;

/// Parse a port name literal known to be valid at compile time.
///
/// # Panics
///
/// Panics if `name` does not match [`PortName`]'s pattern — a bug in this
/// crate's own tool specs, never in caller input.
pub(crate) fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap_or_else(|err| unreachable!("bad port name {name:?}: {err}"))
}

/// Parse a tool name literal known to be valid at compile time.
///
/// Kept as its own function (rather than inlining `ToolName::parse(...).unwrap()`
/// at each call site) so the panic clippy's `missing_panics_doc` looks for
/// stays inside this private helper instead of surfacing in every public
/// `Tool::new`.
///
/// # Panics
///
/// Panics if `name` does not match [`ToolName`]'s pattern — a bug in this
/// crate's own tool specs, never in caller input.
pub(crate) fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap_or_else(|err| unreachable!("bad tool name {name:?}: {err}"))
}

/// Parse a type name literal known to be valid at compile time.
///
/// # Panics
///
/// Panics if `name` does not match [`TypeName`]'s pattern — a bug in this
/// crate's own tool specs, never in caller input.
fn type_name(name: &str) -> TypeName {
    TypeName::parse(name).unwrap_or_else(|err| unreachable!("bad type name {name:?}: {err}"))
}

/// A scalar [`TypeRef`] for the type named `name`.
pub(crate) fn scalar(name: &str) -> TypeRef {
    TypeRef::scalar(type_name(name))
}

/// A `list<name>` [`TypeRef`].
pub(crate) fn list(name: &str) -> TypeRef {
    TypeRef::list_of(type_name(name))
}

/// A required or optional [`PortSpec`] accepting exactly the scalar type
/// `name`.
pub(crate) fn exact(name: &str, required: bool) -> PortSpec {
    PortSpec {
        ty: PortType::Exact(scalar(name)),
        required,
    }
}

/// A required or optional [`PortSpec`] accepting any secret scalar type.
pub(crate) fn any_secret(required: bool) -> PortSpec {
    PortSpec {
        ty: PortType::AnySecret,
        required,
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::Invalid`].
pub(crate) fn invalid(message: impl Into<String>) -> ToolError {
    ToolError {
        kind: ToolErrorKind::Invalid,
        message: message.into(),
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::NotFound`].
pub(crate) fn not_found(message: impl Into<String>) -> ToolError {
    ToolError {
        kind: ToolErrorKind::NotFound,
        message: message.into(),
    }
}

/// Build a [`ToolError`] of kind [`ToolErrorKind::Conflict`].
pub(crate) fn conflict(message: impl Into<String>) -> ToolError {
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
/// Returns [`ToolError::Invalid`](ToolErrorKind::Invalid) naming the first
/// missing required port.
pub(crate) fn require_present(spec: &ToolSpec, inputs: &Inputs) -> Result<(), ToolError> {
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
/// [`Unknown`](willikins_core::ValueState::Unknown), or bound to a value of
/// the wrong domain type — every case a tool's `read` or `ensure` cannot
/// proceed without the concrete value in hand.
///
/// # Errors
///
/// Returns [`ToolError::Invalid`](ToolErrorKind::Invalid) naming `name` in
/// each of the three cases above.
pub(crate) fn get<T: DomainType + 'static>(inputs: &Inputs, name: &str) -> Result<T, ToolError> {
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
/// Returns [`ToolError::Invalid`](ToolErrorKind::Invalid) naming `name`.
pub(crate) fn known<T: DomainType + 'static>(value: &Value, name: &str) -> Result<T, ToolError> {
    if !value.is_known() {
        return Err(invalid(format!("port `{name}` is unknown")));
    }
    value
        .downcast::<T>()
        .cloned()
        .ok_or_else(|| invalid(format!("port `{name}` has an unexpected type")))
}
