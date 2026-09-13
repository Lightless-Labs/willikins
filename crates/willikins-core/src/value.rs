//! Typed values that flow across a tool boundary.
//!
//! A [`Value`] pairs a [`TypeRef`] (what the value's type is declared to be)
//! with a [`ValueState`] (what, if anything, is actually known about it).
//! `Value` never stores a bare string: a [`Known`] value always holds the
//! parsed, domain-typed object behind an [`Arc`], so a secret stays a secret
//! all the way through — [`Value`]'s own [`std::fmt::Debug`] and
//! [`serde::Serialize`] both route through [`DomainObject::render`], so a
//! secret [`Value`] prints and serializes as `[REDACTED <TypeName>]` in
//! every container that derives `Debug` or `Serialize` from it, with no
//! extra effort required at the call site.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use willikins_types::object::DomainObject;
use willikins_types::{DomainType, ParseError, Rendered};

pub use willikins_types::registry::{TypeName, TypeRef, TypeRegistry};

/// Build the [`TypeName`] for `T`.
///
/// `T::TYPE_NAME` is validated by construction: the derive and every
/// hand-written [`DomainType`] implementation in `willikins-types` produce a
/// `PascalCase` identifier that satisfies [`TypeName`]'s own pattern. This
/// cannot fail for any domain type that exists, so it is kept private and
/// infallible rather than surfacing a `Result` at every [`Value`]
/// constructor.
fn type_name_of<T: DomainType>() -> TypeName {
    TypeName::parse(T::TYPE_NAME)
        .unwrap_or_else(|err| unreachable!("DomainType::TYPE_NAME must be a TypeName: {err}"))
}

/// Build the [`TypeName`] for an object-safe [`DomainObject`], the same way
/// [`type_name_of`] does for a statically known `T`.
fn type_name_of_object(obj: &dyn DomainObject) -> TypeName {
    TypeName::parse(obj.type_name())
        .unwrap_or_else(|err| unreachable!("DomainObject::type_name must be a TypeName: {err}"))
}

/// The type a tool port accepts.
///
/// `AnySecret` exists only for sinks such as
/// `github.actions_secret.ensure`'s `value` port, which must take any
/// secret scalar type without naming one in particular. Serialized as an
/// adjacently tagged object (`{"kind":"exact","type":"T"}` or
/// `{"kind":"any_secret"}`) rather than a bare string, so a `PortType`
/// value can never be confused with a domain type literally named
/// `AnySecret`.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", content = "type", rename_all = "snake_case")]
pub enum PortType {
    /// Accepts exactly this type reference: same name, same list-ness.
    Exact(TypeRef),
    /// Accepts any secret scalar type. Rejects a non-secret value and a
    /// list of secrets alike.
    AnySecret,
}

impl PortType {
    /// Whether a value of type `ty` may be bound to a port of this type.
    ///
    /// `Exact` requires equality with the wrapped [`TypeRef`]. `AnySecret`
    /// requires a scalar (`!ty.list`) whose type is registered and secret;
    /// an unregistered type name is never accepted.
    #[must_use]
    pub fn accepts(&self, ty: &TypeRef, registry: &TypeRegistry) -> bool {
        match self {
            Self::Exact(expected) => expected == ty,
            Self::AnySecret => !ty.list && registry.is_secret(&ty.name) == Some(true),
        }
    }
}

impl fmt::Display for PortType {
    /// The port's type as an agent reads it: the type reference itself for
    /// [`Self::Exact`], the literal `AnySecret` for [`Self::AnySecret`].
    ///
    /// Exists because a [`CheckError`](crate::CheckError) message names a
    /// port's declared type, and `Reported` publishes that message verbatim
    /// to every agent. Formatting a `PortType` with `Debug` there put
    /// `Exact(TypeRef { name: TypeName("Text"), list: false })` in front of
    /// the caller; this is the one rendering both that message and the
    /// CLI's text renderer go through.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(ty) => write!(f, "{ty}"),
            Self::AnySecret => f.write_str("AnySecret"),
        }
    }
}

/// What is known about a [`Value`]'s content.
#[derive(Clone)]
pub enum ValueState {
    /// Nothing is known yet: neither the value's identity nor its content.
    /// Downstream nodes see this as a value they cannot read from.
    Unknown,
    /// The value is known: either a single domain object or a list of them.
    Known(Known),
}

/// The content of a [`ValueState::Known`] value.
#[derive(Clone)]
pub enum Known {
    /// A single domain-typed value.
    Scalar(Arc<dyn DomainObject>),
    /// A list of values of the same domain type.
    List(Vec<Arc<dyn DomainObject>>),
}

/// A typed value flowing across a tool boundary.
///
/// Pairs a [`TypeRef`] (what the value is declared to be) with a
/// [`ValueState`] (what is actually known about it). Cloning a `Value`
/// clones the `Arc`s inside it, never the underlying domain object.
#[derive(Clone)]
pub struct Value {
    ty: TypeRef,
    state: ValueState,
}

impl Value {
    /// A known scalar value of domain type `T`.
    pub fn known<T: DomainType + DomainObject + 'static>(value: T) -> Self {
        Self {
            ty: TypeRef::scalar(type_name_of::<T>()),
            state: ValueState::Known(Known::Scalar(Arc::new(value))),
        }
    }

    /// A known list of domain type `T`, empty or not.
    pub fn known_list<T: DomainType + DomainObject + 'static>(values: Vec<T>) -> Self {
        let items = values
            .into_iter()
            .map(|value| Arc::new(value) as Arc<dyn DomainObject>)
            .collect();
        Self {
            ty: TypeRef::list_of(type_name_of::<T>()),
            state: ValueState::Known(Known::List(items)),
        }
    }

    /// A known scalar value already behind a type-erased [`DomainObject`].
    #[must_use]
    pub fn known_dyn(object: Arc<dyn DomainObject>) -> Self {
        let ty = TypeRef::scalar(type_name_of_object(object.as_ref()));
        Self {
            ty,
            state: ValueState::Known(Known::Scalar(object)),
        }
    }

    /// A value whose type is declared but whose content is not known.
    #[must_use]
    pub fn unknown(ty: TypeRef) -> Self {
        Self {
            ty,
            state: ValueState::Unknown,
        }
    }

    /// Parse `input` as a known scalar value of `ty`, through the global
    /// type registry.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `ty` names a list type (use
    /// [`Self::parse_list`] instead), when `ty` is not a registered type,
    /// when `ty` names a secret type (the registry refuses secret
    /// literals), or when `input` does not parse as `ty`.
    pub fn parse(ty: &TypeRef, input: &str) -> Result<Self, ParseError> {
        if ty.list {
            return Err(ParseError::new(
                "Value",
                format!("{ty} is a list type; use Value::parse_list"),
            ));
        }
        let object = willikins_types::registry().parse(&ty.name, input)?;
        Ok(Self {
            ty: ty.clone(),
            state: ValueState::Known(Known::Scalar(object)),
        })
    }

    /// Parse `inputs` as a known list value of `ty`, through the global type
    /// registry.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `ty` names a scalar type (use
    /// [`Self::parse`] instead), when `ty`'s element type is not registered,
    /// when it is secret, or when any of `inputs` fails to parse.
    pub fn parse_list(ty: &TypeRef, inputs: &[&str]) -> Result<Self, ParseError> {
        if !ty.list {
            return Err(ParseError::new(
                "Value",
                format!("{ty} is a scalar type; use Value::parse"),
            ));
        }
        let element = ty.element();
        // Resolve the element type once, before looking at any input: an
        // empty slice must be refused for a secret or unregistered element
        // type just as a non-empty one is.
        let entry = willikins_types::registry().literal_entry(&element.name)?;
        let items = inputs
            .iter()
            .map(|input| (entry.parse)(input))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            ty: ty.clone(),
            state: ValueState::Known(Known::List(items)),
        })
    }

    /// This value's declared type.
    #[must_use]
    pub fn ty(&self) -> &TypeRef {
        &self.ty
    }

    /// Whether this value's type is secret.
    ///
    /// Consults the global type registry first; when `ty`'s name is not
    /// registered there, falls back to asking a known object directly (an
    /// empty, unregistered list reports `false`, since there is no object
    /// left to ask and an unregistered type cannot be looked up any other
    /// way).
    #[must_use]
    pub fn is_secret(&self) -> bool {
        if let Some(secret) = willikins_types::registry().is_secret(&self.ty.name) {
            return secret;
        }
        match &self.state {
            ValueState::Known(Known::Scalar(object)) => object.is_secret(),
            ValueState::Known(Known::List(items)) => {
                items.first().is_some_and(|object| object.is_secret())
            }
            ValueState::Unknown => false,
        }
    }

    /// Whether this value's content is known.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self.state, ValueState::Known(_))
    }

    /// The known scalar object, or `None` when this value is a list or
    /// unknown.
    #[must_use]
    pub fn as_scalar(&self) -> Option<&dyn DomainObject> {
        match &self.state {
            ValueState::Known(Known::Scalar(object)) => Some(object.as_ref()),
            _ => None,
        }
    }

    /// The known list of objects, or `None` when this value is a scalar or
    /// unknown.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Arc<dyn DomainObject>]> {
        match &self.state {
            ValueState::Known(Known::List(items)) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// The known scalar object as a shared, type-erased pointer, or `None`
    /// when this value is a list or unknown. Cloning is a cheap `Arc`
    /// clone, not a deep copy — used by `plan` to collect a `for_each`
    /// node's per-instance outputs into a [`Self::known_dyn_list`] without
    /// knowing the element's concrete Rust type.
    #[must_use]
    pub fn as_scalar_arc(&self) -> Option<Arc<dyn DomainObject>> {
        match &self.state {
            ValueState::Known(Known::Scalar(object)) => Some(Arc::clone(object)),
            _ => None,
        }
    }

    /// A known list of domain objects already behind type-erased pointers,
    /// declared as `list<element>`.
    ///
    /// Unlike [`Self::known_list`], this does not require the element's
    /// concrete Rust type at the call site: `plan` uses it to aggregate a
    /// `for_each` node's per-instance scalar outputs (each recovered via
    /// [`Self::as_scalar_arc`]) into one list value, without knowing which
    /// domain type the tool's output port declares.
    #[must_use]
    pub fn known_dyn_list(element: TypeName, items: Vec<Arc<dyn DomainObject>>) -> Self {
        Self {
            ty: TypeRef::list_of(element),
            state: ValueState::Known(Known::List(items)),
        }
    }

    /// Recover the concrete type `T` from this value's known scalar, or
    /// `None` when it is a list, unknown, or holds a different type.
    #[must_use]
    pub fn downcast<T: DomainType + 'static>(&self) -> Option<&T> {
        self.as_scalar().and_then(willikins_types::downcast::<T>)
    }

    /// The `&'static` type name backing this value, from the registry when
    /// registered, or from a known object otherwise. Used only to build a
    /// [`Rendered::Redacted`] marker, which requires a `&'static str`.
    fn static_type_name(&self) -> &'static str {
        if let Some(entry) = willikins_types::registry().get(&self.ty.name) {
            return entry.info.name;
        }
        match &self.state {
            ValueState::Known(Known::Scalar(object)) => object.type_name(),
            ValueState::Known(Known::List(items)) => {
                items.first().map_or("<unregistered type>", |object| {
                    DomainObject::type_name(object.as_ref())
                })
            }
            ValueState::Unknown => "<unregistered type>",
        }
    }

    /// Render this value for display: the canonical string for a known
    /// non-secret scalar, a bracketed join of the same for a known
    /// non-secret list, a redaction marker for a secret value (scalar or
    /// list alike), or `"<unknown>"` when nothing is known yet.
    #[must_use]
    pub fn render(&self) -> Rendered {
        match &self.state {
            ValueState::Unknown => Rendered::Plain("<unknown>".to_string()),
            ValueState::Known(Known::Scalar(object)) => object.render(),
            ValueState::Known(Known::List(items)) => {
                if self.is_secret() {
                    Rendered::Redacted {
                        type_name: self.static_type_name(),
                    }
                } else {
                    let joined = items
                        .iter()
                        .map(|object| object.render().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Rendered::Plain(format!("[{joined}]"))
                }
            }
        }
    }
}

impl fmt::Debug for Value {
    /// Goes through [`Self::render`], so a secret `Value` never prints its
    /// bytes through `{:?}` no matter what container holds it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

impl PartialEq for Value {
    /// Equal when the declared type matches and the states hold equal
    /// content, comparing known objects with
    /// [`DomainObject::dyn_eq`] rather than by declared type alone.
    fn eq(&self, other: &Self) -> bool {
        if self.ty != other.ty {
            return false;
        }
        match (&self.state, &other.state) {
            (ValueState::Unknown, ValueState::Unknown) => true,
            (ValueState::Known(Known::Scalar(a)), ValueState::Known(Known::Scalar(b))) => {
                a.dyn_eq(b.as_ref())
            }
            (ValueState::Known(Known::List(a)), ValueState::Known(Known::List(b))) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.dyn_eq(y.as_ref()))
            }
            _ => false,
        }
    }
}

impl serde::Serialize for Value {
    /// Emits exactly the shape documented in the design plan: `type`,
    /// `list`, `state`, then `value` (present only for
    /// [`ValueState::Known`]) and `redacted` (present, and `true`, only for
    /// a known secret value — an unknown value carries no `value` to mark
    /// as redacted, so it carries no `redacted` key either, matching the
    /// plan's pinned `{"type": "DopplerServiceToken", ..., "state":
    /// "unknown"}` example).
    ///
    /// A known list's `value` is a JSON array of its elements' rendered
    /// strings — the marker string, repeated, for a secret list.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let known = self.is_known();
        let secret = known && self.is_secret();
        let mut field_count = 3;
        if known {
            field_count += 1;
        }
        if secret {
            field_count += 1;
        }

        let mut out = serializer.serialize_struct("Value", field_count)?;
        out.serialize_field("type", &self.ty.name)?;
        out.serialize_field("list", &self.ty.list)?;
        match &self.state {
            ValueState::Unknown => {
                out.serialize_field("state", "unknown")?;
            }
            ValueState::Known(Known::Scalar(object)) => {
                out.serialize_field("state", "known")?;
                out.serialize_field("value", &object.render().to_string())?;
            }
            ValueState::Known(Known::List(items)) => {
                out.serialize_field("state", "known")?;
                let rendered: Vec<String> = items
                    .iter()
                    .map(|object| object.render().to_string())
                    .collect();
                out.serialize_field("value", &rendered)?;
            }
        }
        if secret {
            out.serialize_field("redacted", &true)?;
        }
        out.end()
    }
}

/// Hand-written to match [`serde::Serialize for Value`](Value)'s pinned
/// shape exactly, rather than derived: [`Value`] is not a plain struct (its
/// content depends on [`ValueState`], not on a fixed set of Rust fields), so
/// there is nothing for `#[derive(JsonSchema)]` to reflect over. States the
/// same shape milestone 1 pinned by snapshot: `type` and `list` always
/// present, `state` one of `"known"`/`"unknown"`, `value` present only when
/// known (a string for a scalar, a list of strings for a list), and
/// `redacted` present (and `true`) only for a known secret value.
///
/// The three `if`/`then` clauses are what make the last two clauses of that
/// sentence more than prose: without them the schema listed the five keys
/// and constrained nothing about how they combine, so a `value` on an
/// `unknown` state, a `redacted: false`, or a scalar `value` under `list:
/// true` all validated -- none of which `Serialize` can emit. An agent
/// generating a `Value` from the published schema would have had no way to
/// learn that from the schema. Both `state` and `list` are `required`, so no
/// `if` is ever vacuously satisfied by a missing key.
impl schemars::JsonSchema for Value {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Value")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "type": { "type": "string" },
                "list": { "type": "boolean" },
                "state": { "type": "string", "enum": ["known", "unknown"] },
                "value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } },
                    ],
                },
                "redacted": { "const": true },
            },
            "required": ["type", "list", "state"],
            "additionalProperties": false,
            "allOf": [
                {
                    "if": {
                        "properties": { "state": { "const": "unknown" } },
                        "required": ["state"],
                    },
                    "then": {
                        "not": {
                            "anyOf": [
                                { "required": ["value"] },
                                { "required": ["redacted"] },
                            ],
                        },
                    },
                    "else": { "required": ["value"] },
                },
                {
                    "if": {
                        "properties": { "list": { "const": false } },
                        "required": ["list"],
                    },
                    "then": { "properties": { "value": { "type": "string" } } },
                    "else": {
                        "properties": {
                            "value": { "type": "array", "items": { "type": "string" } },
                        },
                    },
                },
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::{DopplerServiceToken, GitHubOrg};

    fn github_org(name: &str) -> GitHubOrg {
        GitHubOrg::parse(name).unwrap()
    }

    #[test]
    fn known_reports_its_type_and_is_known() {
        let value = Value::known(github_org("lightless-labs"));
        assert_eq!(value.ty().to_string(), "GitHubOrg");
        assert!(!value.ty().list);
        assert!(value.is_known());
        assert!(!value.is_secret());
    }

    #[test]
    fn known_downcasts_to_the_concrete_type() {
        let value = Value::known(github_org("lightless-labs"));
        let org = value.downcast::<GitHubOrg>().expect("holds a GitHubOrg");
        assert_eq!(org.as_str(), "lightless-labs");
    }

    #[test]
    fn downcast_returns_none_for_the_wrong_type() {
        let value = Value::known(github_org("lightless-labs"));
        assert!(value.downcast::<willikins_types::HttpsUrl>().is_none());
    }

    #[test]
    fn known_list_reports_list_type() {
        let value = Value::known_list(vec![github_org("a"), github_org("b")]);
        assert!(value.ty().list);
        assert_eq!(value.as_list().unwrap().len(), 2);
    }

    #[test]
    fn known_dyn_recovers_the_type_name_from_the_object() {
        let object: Arc<dyn DomainObject> = Arc::new(github_org("lightless-labs"));
        let value = Value::known_dyn(object);
        assert_eq!(value.ty().to_string(), "GitHubOrg");
    }

    #[test]
    fn unknown_has_no_content() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        let value = Value::unknown(ty.clone());
        assert!(!value.is_known());
        assert_eq!(value.ty(), &ty);
        assert!(value.as_scalar().is_none());
    }

    #[test]
    fn parse_builds_a_known_scalar_through_the_registry() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        let value = Value::parse(&ty, "lightless-labs").unwrap();
        assert_eq!(
            value.downcast::<GitHubOrg>().unwrap().as_str(),
            "lightless-labs"
        );
    }

    #[test]
    fn parse_rejects_a_list_type_ref() {
        let ty = TypeRef::list_of(TypeName::parse("GitHubOrg").unwrap());
        let err = Value::parse(&ty, "lightless-labs").unwrap_err();
        assert!(err.reason.contains("parse_list"), "{}", err.reason);
    }

    #[test]
    fn parse_propagates_secret_refusal() {
        let ty = TypeRef::scalar(TypeName::parse("DopplerServiceToken").unwrap());
        let err =
            Value::parse(&ty, "dp.st.prd.exampleexampleexampleexampleexampleexample").unwrap_err();
        assert!(err.reason.contains("cannot be supplied"), "{}", err.reason);
    }

    #[test]
    fn parse_list_builds_a_known_list_through_the_registry() {
        let ty = TypeRef::list_of(TypeName::parse("GitHubOrg").unwrap());
        let value = Value::parse_list(&ty, &["a", "b"]).unwrap();
        assert_eq!(value.as_list().unwrap().len(), 2);
    }

    #[test]
    fn parse_list_of_an_empty_slice_yields_a_known_empty_list() {
        let ty = TypeRef::list_of(TypeName::parse("GitHubOrg").unwrap());
        let value = Value::parse_list(&ty, &[]).unwrap();
        assert!(value.is_known());
        assert!(value.as_list().expect("a known list").is_empty());
    }

    #[test]
    fn parse_list_refuses_a_secret_element_type_even_with_no_inputs() {
        // The element type is resolved before any input is parsed, so an
        // empty slice cannot smuggle a secret-typed literal list past the
        // registry's refusal.
        let ty = TypeRef::list_of(TypeName::parse("DopplerServiceToken").unwrap());
        let err = Value::parse_list(&ty, &[]).unwrap_err();
        assert!(err.reason.contains("cannot be supplied"), "{}", err.reason);
    }

    #[test]
    fn parse_list_refuses_an_unregistered_element_type_even_with_no_inputs() {
        let ty = TypeRef::list_of(TypeName::parse("NoSuchType").unwrap());
        let err = Value::parse_list(&ty, &[]).unwrap_err();
        assert!(err.reason.contains("NoSuchType"), "{}", err.reason);
    }

    #[test]
    fn parse_list_rejects_a_scalar_type_ref() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        let err = Value::parse_list(&ty, &["a"]).unwrap_err();
        assert!(err.reason.contains("Value::parse"), "{}", err.reason);
    }

    #[test]
    fn is_secret_is_true_for_a_known_secret_scalar() {
        let value = Value::known(
            DopplerServiceToken::parse("dp.st.prd.exampleexampleexampleexampleexampleexample")
                .unwrap(),
        );
        assert!(value.is_secret());
    }

    #[test]
    fn is_secret_is_true_for_an_unknown_secret_value() {
        let ty = TypeRef::scalar(TypeName::parse("DopplerServiceToken").unwrap());
        let value = Value::unknown(ty);
        assert!(value.is_secret());
    }

    #[test]
    fn debug_of_a_secret_value_shows_only_the_marker() {
        let value = Value::known(
            DopplerServiceToken::parse("dp.st.prd.exampleexampleexampleexampleexampleexample")
                .unwrap(),
        );
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerServiceToken]");
        assert_eq!(format!("{value:#?}"), "[REDACTED DopplerServiceToken]");
        assert!(!format!("{value:?}").contains("exampleexampleexampleexampleexampleexample"));
    }

    #[test]
    fn debug_of_an_unknown_value_shows_the_unknown_marker() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        let value = Value::unknown(ty);
        assert_eq!(format!("{value:?}"), "<unknown>");
    }

    #[test]
    fn debug_of_a_known_non_secret_value_shows_its_canonical_string() {
        let value = Value::known(github_org("lightless-labs"));
        assert_eq!(format!("{value:?}"), "lightless-labs");
    }

    #[test]
    fn partial_eq_compares_declared_type_and_content() {
        let a = Value::known(github_org("lightless-labs"));
        let b = Value::known(github_org("lightless-labs"));
        let c = Value::known(github_org("other"));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn partial_eq_treats_two_unknowns_of_the_same_type_as_equal() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        assert_eq!(Value::unknown(ty.clone()), Value::unknown(ty));
    }

    #[test]
    fn partial_eq_treats_known_and_unknown_as_different() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        assert_ne!(
            Value::known(github_org("lightless-labs")),
            Value::unknown(ty)
        );
    }

    #[test]
    fn as_scalar_arc_recovers_the_shared_object() {
        let value = Value::known(github_org("lightless-labs"));
        let arc = value.as_scalar_arc().expect("a known scalar has an arc");
        assert_eq!(arc.render().to_string(), "lightless-labs");
    }

    #[test]
    fn as_scalar_arc_is_none_for_a_list_or_unknown_value() {
        let list = Value::known_list(vec![github_org("a")]);
        assert!(list.as_scalar_arc().is_none());
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        assert!(Value::unknown(ty).as_scalar_arc().is_none());
    }

    #[test]
    fn known_dyn_list_builds_a_list_from_recovered_arcs() {
        let a = Value::known(github_org("a"));
        let b = Value::known(github_org("b"));
        let element = TypeName::parse("GitHubOrg").unwrap();
        let list = Value::known_dyn_list(
            element,
            vec![a.as_scalar_arc().unwrap(), b.as_scalar_arc().unwrap()],
        );
        assert!(list.ty().list);
        assert_eq!(list.ty().name.as_str(), "GitHubOrg");
        assert_eq!(list.as_list().unwrap().len(), 2);
        assert_eq!(
            list,
            Value::known_list(vec![github_org("a"), github_org("b")])
        );
    }

    #[test]
    fn clone_shares_the_same_underlying_object() {
        let value = Value::known(github_org("lightless-labs"));
        let cloned = value.clone();
        assert_eq!(value, cloned);
    }

    // -------------------------------------------------------------
    // JSON shape (pinned by insta; see the plan's "Value JSON shape" bullet)
    // -------------------------------------------------------------

    #[test]
    fn json_shape_known_non_secret() {
        let value = Value::known(github_org("lightless-labs"));
        insta::assert_json_snapshot!(value);
    }

    #[test]
    fn json_shape_known_secret() {
        let value = Value::known(
            DopplerServiceToken::parse("dp.st.prd.exampleexampleexampleexampleexampleexample")
                .unwrap(),
        );
        insta::assert_json_snapshot!(value);
    }

    #[test]
    fn json_shape_unknown() {
        let ty = TypeRef::scalar(TypeName::parse("DopplerServiceToken").unwrap());
        let value = Value::unknown(ty);
        insta::assert_json_snapshot!(value);
    }

    #[test]
    fn json_shape_unknown_secret_has_no_value_or_redacted_key() {
        // Matches the plan's pinned example precisely: an unknown value of
        // a secret type has no `value` to redact, so it gets no `redacted`
        // key either, even though the type itself is secret.
        let ty = TypeRef::scalar(TypeName::parse("DopplerServiceToken").unwrap());
        let value = Value::unknown(ty);
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"type":"DopplerServiceToken","list":false,"state":"unknown"}"#
        );
    }

    #[test]
    fn json_shape_known_list() {
        let value = Value::known_list(vec![github_org("a"), github_org("b")]);
        insta::assert_json_snapshot!(value);
    }

    // -------------------------------------------------------------
    // PortType
    // -------------------------------------------------------------

    #[test]
    fn exact_accepts_only_the_same_type_ref() {
        let github_org_ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        let https_url_ty = TypeRef::scalar(TypeName::parse("HttpsUrl").unwrap());
        let port = PortType::Exact(github_org_ty.clone());
        assert!(port.accepts(&github_org_ty, willikins_types::registry()));
        assert!(!port.accepts(&https_url_ty, willikins_types::registry()));
    }

    #[test]
    fn any_secret_accepts_a_secret_scalar() {
        let ty = TypeRef::scalar(TypeName::parse("DopplerServiceToken").unwrap());
        assert!(PortType::AnySecret.accepts(&ty, willikins_types::registry()));
    }

    #[test]
    fn any_secret_rejects_a_non_secret_scalar() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        assert!(!PortType::AnySecret.accepts(&ty, willikins_types::registry()));
    }

    #[test]
    fn any_secret_rejects_a_list_of_secrets() {
        let ty = TypeRef::list_of(TypeName::parse("DopplerServiceToken").unwrap());
        assert!(!PortType::AnySecret.accepts(&ty, willikins_types::registry()));
    }

    #[test]
    fn any_secret_rejects_an_unregistered_type() {
        let ty = TypeRef::scalar(TypeName::parse("NoSuchType").unwrap());
        assert!(!PortType::AnySecret.accepts(&ty, willikins_types::registry()));
    }

    #[test]
    fn port_type_serializes_adjacently_tagged() {
        let ty = TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap());
        assert_eq!(
            serde_json::to_string(&PortType::Exact(ty)).unwrap(),
            r#"{"kind":"exact","type":"GitHubOrg"}"#
        );
        assert_eq!(
            serde_json::to_string(&PortType::AnySecret).unwrap(),
            r#"{"kind":"any_secret"}"#
        );
    }

    // -------------------------------------------------------------
    // Value's hand-written JsonSchema validates every real JSON shape
    // -------------------------------------------------------------

    #[test]
    fn hand_written_schema_validates_every_value_json_shape() {
        let schema = schemars::schema_for!(Value);
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("Value's schema is itself valid");

        let secret_token = || {
            DopplerServiceToken::parse("dp.st.prd.exampleexampleexampleexampleexampleexample")
                .unwrap()
        };

        let known_scalar =
            serde_json::to_value(Value::known(github_org("lightless-labs"))).unwrap();
        let known_list =
            serde_json::to_value(Value::known_list(vec![github_org("a"), github_org("b")]))
                .unwrap();
        let empty_list = serde_json::to_value(Value::known_list(Vec::<GitHubOrg>::new())).unwrap();
        let unknown_scalar = serde_json::to_value(Value::unknown(TypeRef::scalar(
            TypeName::parse("GitHubOrg").unwrap(),
        )))
        .unwrap();
        let unknown_list = serde_json::to_value(Value::unknown(TypeRef::list_of(
            TypeName::parse("GitHubOrg").unwrap(),
        )))
        .unwrap();
        let secret_scalar = serde_json::to_value(Value::known(secret_token())).unwrap();
        let secret_list = serde_json::to_value(Value::known_list(vec![secret_token()])).unwrap();

        for (name, instance) in [
            ("known_scalar", &known_scalar),
            ("known_list", &known_list),
            ("empty_list", &empty_list),
            ("unknown_scalar", &unknown_scalar),
            ("unknown_list", &unknown_list),
            ("secret_scalar", &secret_scalar),
            ("secret_list", &secret_list),
        ] {
            assert!(
                validator.is_valid(instance),
                "{name} must validate against Value's schema: {instance}"
            );
        }

        // Negative cases: the schema must actually constrain something,
        // not merely describe the happy path. An unrecognised `state` and
        // a stray unknown property must both be rejected.
        let bad_state = serde_json::json!({"type": "GitHubOrg", "list": false, "state": "bogus"});
        assert!(
            !validator.is_valid(&bad_state),
            "an invalid `state` must be rejected"
        );
        let stray_property = serde_json::json!({
            "type": "GitHubOrg", "list": false, "state": "unknown", "extra": true
        });
        assert!(
            !validator.is_valid(&stray_property),
            "an unrecognised property must be rejected"
        );
        let missing_required = serde_json::json!({"type": "GitHubOrg", "state": "unknown"});
        assert!(
            !validator.is_valid(&missing_required),
            "a missing required property (`list`) must be rejected"
        );
    }

    /// The schema must also tie `value` to `state` and its cardinality to
    /// `list`, not merely list the five keys: an `Unknown` value carries no
    /// `value` (and so no `redacted`), a `Known` one always carries one,
    /// and a scalar's `value` is a string exactly where a list's is an
    /// array. Without these, a schema consumer (an agent generating a
    /// `Value` from the published schema, or a future
    /// `Deserialize`) could produce a document this crate can never emit.
    #[test]
    fn hand_written_schema_rejects_shapes_serialize_can_never_emit() {
        let schema = schemars::schema_for!(Value);
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("Value's schema is itself valid");

        for (name, instance) in [
            (
                "value on an unknown state",
                serde_json::json!({
                    "type": "GitHubOrg", "list": false,
                    "state": "unknown", "value": "lightless-labs"
                }),
            ),
            (
                "redacted on an unknown state",
                serde_json::json!({
                    "type": "DopplerServiceToken", "list": false,
                    "state": "unknown", "redacted": true
                }),
            ),
            (
                "a known value with no value key",
                serde_json::json!({"type": "GitHubOrg", "list": false, "state": "known"}),
            ),
            (
                "a list flag disagreeing with a scalar value",
                serde_json::json!({
                    "type": "GitHubOrg", "list": true,
                    "state": "known", "value": "lightless-labs"
                }),
            ),
            (
                "a scalar flag disagreeing with a list value",
                serde_json::json!({
                    "type": "GitHubOrg", "list": false,
                    "state": "known", "value": ["a", "b"]
                }),
            ),
            (
                "redacted false, which Serialize never emits",
                serde_json::json!({
                    "type": "GitHubOrg", "list": false,
                    "state": "known", "value": "a", "redacted": false
                }),
            ),
        ] {
            assert!(
                !validator.is_valid(&instance),
                "{name} must be rejected by Value's schema: {instance}"
            );
        }
    }
}
