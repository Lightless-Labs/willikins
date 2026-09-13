//! The tool contract: a tool's declared shape (`ToolSpec`), the typed
//! values bound to it (`Inputs`, `Outputs`), what a tool reports about
//! existing state (`Observation`), and the `Tool` trait itself.

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use indexmap::IndexMap;

use crate::class::Class;
use crate::value::{PortType, TypeRef, TypeRegistry, Value};
use willikins_types::SinkToken;

pub mod helpers;

/// The pattern every [`PortName`] must match: `snake_case`, starting with a
/// letter.
const PORT_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*$";

static PORT_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(PORT_NAME_PATTERN).expect("PORT_NAME_PATTERN is valid"));

/// The pattern every [`ToolName`] must match: one or more `snake_case`
/// segments joined by `.`, such as `github.repo.ensure`.
const TOOL_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$";

static TOOL_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(TOOL_NAME_PATTERN).expect("TOOL_NAME_PATTERN is valid"));

/// Declares one newtype identifier over `String`, validated on parse
/// against `$pattern`, with `Display`, `serde`, and `JsonSchema` support.
macro_rules! identifier {
    ($name:ident, $pattern_const:ident, $pattern:expr, $regex:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Parse a ", stringify!($name), ", checking it against the required pattern.")]
            ///
            /// # Errors
            ///
            /// Returns [`willikins_types::ParseError`] when `input` does not match the pattern.
            pub fn parse(input: &str) -> Result<Self, willikins_types::ParseError> {
                if $regex.is_match(input) {
                    Ok(Self(input.to_string()))
                } else {
                    Err(willikins_types::ParseError::new(
                        stringify!($name),
                        format!(
                            "{input:?} is not a valid {} (expected to match `{}`)",
                            stringify!($name),
                            $pattern_const
                        ),
                    ))
                }
            }

            /// Borrow this identifier as a plain string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": $pattern,
                })
            }
        }
    };
}

identifier!(
    PortName,
    PORT_NAME_PATTERN,
    PORT_NAME_PATTERN,
    PORT_NAME_REGEX,
    "The name of a tool's input or output port: `snake_case`, starting with a letter."
);

identifier!(
    ToolName,
    TOOL_NAME_PATTERN,
    TOOL_NAME_PATTERN,
    TOOL_NAME_REGEX,
    "The name of a tool: dotted `snake_case` segments, such as `github.repo.ensure`."
);

/// The declared shape of one input or output port on a [`ToolSpec`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct PortSpec {
    /// The type this port accepts.
    pub ty: PortType,
    /// Whether a workflow must bind this port for the tool to run at all.
    pub required: bool,
}

/// The declared shape of a tool: its ports, its natural key, and how much
/// trust its `ensure` should be given.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct ToolSpec {
    /// The tool's name, as it appears in a workflow document.
    pub name: ToolName,
    /// One-line description shown to an agent.
    pub description: String,
    /// The tool's input ports, in declaration order.
    pub inputs: IndexMap<PortName, PortSpec>,
    /// The tool's output ports and their types, in declaration order.
    pub outputs: IndexMap<PortName, TypeRef>,
    /// The subset of `inputs` that identifies a resource: the ports `read`
    /// uses to look up whether it already exists.
    pub key: Vec<PortName>,
    /// How much trust this tool's `ensure` should be given.
    pub class: Class,
    /// Whether this tool has no external state: `read` computes its
    /// outputs from its inputs alone, and `ensure` is the identity. A pure
    /// tool has no key and its class is always [`Class::Reversible`].
    pub pure: bool,
}

/// Why a [`ToolSpec`] failed [`ToolSpec::validate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// An input port's [`PortType::Exact`] names a type the registry does
    /// not have.
    #[error("input `{port}` references unknown type `{ty}`")]
    UnknownInputType {
        /// The offending input port.
        port: PortName,
        /// The unregistered type it named.
        ty: TypeRef,
    },
    /// An output port names a type the registry does not have.
    #[error("output `{port}` references unknown type `{ty}`")]
    UnknownOutputType {
        /// The offending output port.
        port: PortName,
        /// The unregistered type it named.
        ty: TypeRef,
    },
    /// A `key` entry does not name one of the tool's own input ports.
    #[error("key port `{port}` is not one of this tool's inputs")]
    KeyNotAnInput {
        /// The port named in `key` that is not an input.
        port: PortName,
    },
    /// A pure tool declared a non-empty `key`.
    #[error("a pure tool must have an empty key")]
    PureToolWithKey,
    /// A pure tool's class is not [`Class::Reversible`].
    #[error("a pure tool's class must be Reversible")]
    PureToolNotReversible,
}

impl ToolSpec {
    /// Check this spec's internal consistency against `registry`.
    ///
    /// Checks that every input's [`PortType::Exact`] and every output
    /// names a registered type, that every `key` port is one of this
    /// tool's own inputs, and that a pure tool has an empty key and
    /// [`Class::Reversible`].
    ///
    /// # Errors
    ///
    /// Returns the first [`SpecError`] found, in the order described above.
    pub fn validate(&self, registry: &TypeRegistry) -> Result<(), SpecError> {
        for (port, spec) in &self.inputs {
            if let PortType::Exact(ty) = &spec.ty
                && registry.get(&ty.name).is_none()
            {
                return Err(SpecError::UnknownInputType {
                    port: port.clone(),
                    ty: ty.clone(),
                });
            }
        }
        for (port, ty) in &self.outputs {
            if registry.get(&ty.name).is_none() {
                return Err(SpecError::UnknownOutputType {
                    port: port.clone(),
                    ty: ty.clone(),
                });
            }
        }
        for port in &self.key {
            if !self.inputs.contains_key(port) {
                return Err(SpecError::KeyNotAnInput { port: port.clone() });
            }
        }
        if self.pure {
            if !self.key.is_empty() {
                return Err(SpecError::PureToolWithKey);
            }
            if self.class != Class::Reversible {
                return Err(SpecError::PureToolNotReversible);
            }
        }
        Ok(())
    }
}

/// The typed values bound to a tool's input ports.
///
/// A newtype over an ordered map so a tool always sees its inputs in
/// declaration order. `Debug` and `Serialize` both come from [`Value`]'s
/// own redacting implementations, so a secret input never leaks through
/// either.
#[derive(Debug, Clone, Default, serde::Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct Inputs(IndexMap<PortName, Value>);

/// The typed values produced by a tool's output ports.
///
/// See [`Inputs`]: same shape, same redaction guarantee, opposite
/// direction.
#[derive(Debug, Clone, Default, serde::Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct Outputs(IndexMap<PortName, Value>);

/// Shared behaviour for [`Inputs`] and [`Outputs`]: an ordered map from
/// [`PortName`] to [`Value`].
macro_rules! port_map {
    ($name:ident) => {
        impl $name {
            /// An empty port map.
            #[must_use]
            pub fn new() -> Self {
                Self(IndexMap::new())
            }

            /// The value bound to `port`, if any.
            #[must_use]
            pub fn get(&self, port: &PortName) -> Option<&Value> {
                self.0.get(port)
            }

            /// Bind `value` to `port`, returning the value it replaced, if
            /// any.
            pub fn insert(&mut self, port: PortName, value: Value) -> Option<Value> {
                self.0.insert(port, value)
            }

            /// Iterate the bound ports in declaration order.
            pub fn iter(&self) -> impl Iterator<Item = (&PortName, &Value)> {
                self.0.iter()
            }

            /// The number of bound ports.
            #[must_use]
            pub fn len(&self) -> usize {
                self.0.len()
            }

            /// Whether no ports are bound.
            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }
    };
}

port_map!(Inputs);
port_map!(Outputs);

/// What a tool's `read` reports about the resource its inputs identify.
#[derive(Debug, Clone, serde::Serialize)]
pub enum Observation {
    /// The resource does not exist yet. Carries every output the tool can
    /// derive from its inputs alone (an identity, a computed URL); outputs
    /// that cannot be predicted (a token's value) are left
    /// [`crate::value::ValueState::Unknown`], so downstream nodes can
    /// still `read` at plan time.
    Absent {
        /// The outputs predicted from this tool's inputs.
        predicted: Outputs,
    },
    /// The resource exists and is ours, with these outputs.
    Present(Outputs),
    /// A resource exists at this natural key, but it is not ours.
    Foreign,
    /// The resource exists at this natural key and is ours, but a
    /// non-key input differs from what the tool would have to change to
    /// match it, and the tool will not change it. [`crate::plan::plan`]
    /// turns this into `PlanError::AttributeMismatch`; `ensure` on such a
    /// resource returns [`ToolErrorKind::Conflict`]. The first (and, this
    /// milestone, only) user is `github.repo.ensure`'s `visibility`: see
    /// `crate::plan`'s module docs for why the refusal is deliberately
    /// symmetric.
    Mismatch {
        /// The non-key input port whose requested value does not match
        /// the resource's actual one.
        port: PortName,
    },
}

/// The kind of failure a [`Tool`] reported.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub enum ToolErrorKind {
    /// The resource `ensure` was asked to act on does not exist.
    NotFound,
    /// The resource exists but not in the state the tool was asked to
    /// bring it to, and the tool will not overwrite it.
    Conflict,
    /// The underlying provider reported a failure.
    Provider,
    /// The tool's inputs are not valid, independent of any provider.
    Invalid,
}

/// A tool's failure to `read` or `ensure`.
///
/// Never includes an input value: an implementation must build `message`
/// from the tool's own state and provider response, not from the
/// [`Inputs`] it was given, so this type carries no obligation for callers
/// to redact it themselves.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    thiserror::Error,
)]
#[error("{kind:?}: {message}")]
pub struct ToolError {
    /// The kind of failure.
    pub kind: ToolErrorKind,
    /// A human-readable explanation. Never a secret value.
    pub message: String,
}

/// What [`Tool::ensure`] reports after bringing a resource to the state
/// its inputs describe.
#[derive(Debug, Clone)]
pub struct Ensured {
    /// The tool's declared output ports, filled the same way
    /// [`Observation::Present`]'s would be.
    pub outputs: Outputs,
    /// Whether this call changed the resource's state. `false` means the
    /// resource already matched `inputs` before this call ran — the
    /// executor reports such a node `Unchanged` rather than `Created`.
    pub changed: bool,
}

/// A provisioning tool: one node kind in a workflow graph.
///
/// Object-safe, so a [`crate::catalog::Catalog`] can hold many different
/// tools behind `Arc<dyn Tool>`.
pub trait Tool: Send + Sync {
    /// This tool's declared shape.
    fn spec(&self) -> &ToolSpec;

    /// Observe the resource `inputs`' key ports identify, without changing
    /// anything. Takes no [`SinkToken`], so an implementation has no
    /// legitimate way to expose a secret from here.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] when the underlying provider cannot be
    /// queried.
    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError>;

    /// Bring the resource `inputs` identify to the state `inputs`
    /// describes, minting or consuming secrets through `token` as needed.
    ///
    /// # Contract
    ///
    /// An implementation first observes the resource itself — its own
    /// `read` logic, without a token — so it is safe to call `ensure` on a
    /// resource that already exists and is ours: a resource that exists
    /// and is not ours is [`ToolErrorKind::Conflict`], and one whose
    /// natural key is ours but a non-key input mismatches (see
    /// [`Observation::Mismatch`]) is `Conflict` too. A tool whose resource
    /// has a comparable state (a repository, a project, a config, a
    /// token's existence) creates only what is missing and reports
    /// [`Ensured::changed`] `false` when the resource already matched. A
    /// sink whose value cannot be read back (`github.actions_secret.ensure`)
    /// always writes when called and always reports `changed: true`. A
    /// pure tool's `ensure` is its `read` and always reports `changed:
    /// false`: there is no external state to change.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] when the underlying provider cannot satisfy
    /// the request.
    fn ensure(&self, inputs: &Inputs, token: &SinkToken) -> Result<Ensured, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // PortName / ToolName
    // -------------------------------------------------------------

    #[test]
    fn port_name_accepts_snake_case() {
        assert!(PortName::parse("repo_url").is_ok());
        assert!(PortName::parse("value").is_ok());
    }

    #[test]
    fn port_name_rejects_uppercase_and_leading_digit() {
        assert!(PortName::parse("Repo").is_err());
        assert!(PortName::parse("1repo").is_err());
        assert!(PortName::parse("").is_err());
    }

    #[test]
    fn port_name_displays_and_serializes_as_its_string() {
        let name = PortName::parse("repo_url").unwrap();
        assert_eq!(name.to_string(), "repo_url");
        assert_eq!(serde_json::to_string(&name).unwrap(), "\"repo_url\"");
        assert_eq!(
            serde_json::from_str::<PortName>("\"repo_url\"").unwrap(),
            name
        );
    }

    #[test]
    fn tool_name_accepts_dotted_segments() {
        assert!(ToolName::parse("github.repo.ensure").is_ok());
        assert!(ToolName::parse("naming.v1").is_ok());
    }

    #[test]
    fn tool_name_rejects_a_bare_dot_or_uppercase() {
        assert!(ToolName::parse(".ensure").is_err());
        assert!(ToolName::parse("github..ensure").is_err());
        assert!(ToolName::parse("GitHub.repo.ensure").is_err());
    }

    #[test]
    fn port_name_and_tool_name_have_string_json_schemas() {
        let port_schema = serde_json::to_value(schemars::schema_for!(PortName)).unwrap();
        assert_eq!(port_schema["type"], "string");
        let tool_schema = serde_json::to_value(schemars::schema_for!(ToolName)).unwrap();
        assert_eq!(tool_schema["type"], "string");
    }

    #[test]
    fn tool_spec_json_schema_generates_without_panicking() {
        // Exercises the derive over `IndexMap<PortName, _>` fields, which
        // needs schemars' `indexmap2` feature to compile at all — this is
        // the runtime half of that guarantee.
        let schema = serde_json::to_value(schemars::schema_for!(ToolSpec)).unwrap();
        assert_eq!(schema["type"], "object");
    }

    // -------------------------------------------------------------
    // ToolSpec::validate
    // -------------------------------------------------------------

    fn github_repo_ty() -> TypeRef {
        TypeRef::scalar(crate::value::TypeName::parse("GitHubRepo").unwrap())
    }

    fn visibility_ty() -> TypeRef {
        TypeRef::scalar(crate::value::TypeName::parse("RepoVisibility").unwrap())
    }

    fn base_spec() -> ToolSpec {
        let mut inputs = IndexMap::new();
        inputs.insert(
            PortName::parse("repo").unwrap(),
            PortSpec {
                ty: PortType::Exact(github_repo_ty()),
                required: true,
            },
        );
        inputs.insert(
            PortName::parse("visibility").unwrap(),
            PortSpec {
                ty: PortType::Exact(visibility_ty()),
                required: true,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(PortName::parse("repo").unwrap(), github_repo_ty());
        ToolSpec {
            name: ToolName::parse("github.repo.ensure").unwrap(),
            description: "Ensure a GitHub repository exists.".to_string(),
            inputs,
            outputs,
            key: vec![PortName::parse("repo").unwrap()],
            class: Class::Reversible,
            pure: false,
        }
    }

    #[test]
    fn validate_accepts_a_well_formed_spec() {
        base_spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn validate_rejects_an_unknown_input_type() {
        let mut spec = base_spec();
        spec.inputs
            .get_mut(&PortName::parse("repo").unwrap())
            .unwrap()
            .ty = PortType::Exact(TypeRef::scalar(
            crate::value::TypeName::parse("NoSuchType").unwrap(),
        ));
        assert!(matches!(
            spec.validate(willikins_types::registry()),
            Err(SpecError::UnknownInputType { .. })
        ));
    }

    #[test]
    fn validate_rejects_an_unknown_output_type() {
        let mut spec = base_spec();
        *spec
            .outputs
            .get_mut(&PortName::parse("repo").unwrap())
            .unwrap() = TypeRef::scalar(crate::value::TypeName::parse("NoSuchType").unwrap());
        assert!(matches!(
            spec.validate(willikins_types::registry()),
            Err(SpecError::UnknownOutputType { .. })
        ));
    }

    #[test]
    fn validate_rejects_a_key_port_that_is_not_an_input() {
        let mut spec = base_spec();
        spec.key = vec![PortName::parse("nonexistent").unwrap()];
        assert!(matches!(
            spec.validate(willikins_types::registry()),
            Err(SpecError::KeyNotAnInput { .. })
        ));
    }

    #[test]
    fn validate_rejects_a_pure_tool_with_a_key() {
        let mut spec = base_spec();
        spec.pure = true;
        assert!(matches!(
            spec.validate(willikins_types::registry()),
            Err(SpecError::PureToolWithKey)
        ));
    }

    #[test]
    fn validate_rejects_a_pure_tool_that_is_not_reversible() {
        let mut spec = base_spec();
        spec.pure = true;
        spec.key.clear();
        spec.class = Class::Irreversible;
        assert!(matches!(
            spec.validate(willikins_types::registry()),
            Err(SpecError::PureToolNotReversible)
        ));
    }

    #[test]
    fn validate_accepts_a_pure_tool_with_no_key_and_reversible_class() {
        let mut spec = base_spec();
        spec.pure = true;
        spec.key.clear();
        spec.validate(willikins_types::registry()).unwrap();
    }

    // -------------------------------------------------------------
    // Inputs / Outputs
    // -------------------------------------------------------------

    #[test]
    fn inputs_get_and_insert_round_trip() {
        use willikins_types::DomainType;

        let mut inputs = Inputs::new();
        assert!(inputs.is_empty());
        let port = PortName::parse("org").unwrap();
        let value = Value::known(willikins_types::GitHubOrg::parse("lightless-labs").unwrap());
        assert!(inputs.insert(port.clone(), value.clone()).is_none());
        assert_eq!(inputs.get(&port), Some(&value));
        assert_eq!(inputs.len(), 1);
    }

    #[test]
    fn inputs_serialize_via_value() {
        use willikins_types::DomainType;

        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("org").unwrap(),
            Value::known(willikins_types::GitHubOrg::parse("lightless-labs").unwrap()),
        );
        let json = serde_json::to_value(&inputs).unwrap();
        assert_eq!(json["org"]["type"], "GitHubOrg");
        assert_eq!(json["org"]["value"], "lightless-labs");
    }
}
