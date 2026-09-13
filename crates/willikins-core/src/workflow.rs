//! Workflow graph types: inputs, nodes, bindings, and outputs.
//!
//! A [`Workflow`] is built by hand (a builder, or plain struct construction
//! with public fields) rather than parsed here — parsing a YAML document
//! into a [`Workflow`] is `willikins-dsl`'s job. [`crate::check::check`]
//! validates a `Workflow` against a [`crate::Catalog`].

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use indexmap::IndexMap;

use crate::tool::{PortName, ToolName};
use crate::value::{TypeRef, Value};

/// The pattern every [`InputName`], [`NodeName`], and [`OutputName`] must
/// match: `snake_case`, starting with a letter. Shared with
/// [`crate::tool::PortName`], but declared separately here rather than
/// reused from `tool`, to keep the two modules independent.
const WORKFLOW_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*$";

static WORKFLOW_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(WORKFLOW_NAME_PATTERN).expect("pattern is valid"));

/// Declares one newtype identifier over `String`, validated on parse
/// against [`WORKFLOW_NAME_PATTERN`], with `Display`, `serde`, and
/// `JsonSchema` support. Mirrors [`crate::tool`]'s `identifier!` macro.
macro_rules! workflow_identifier {
    ($name:ident, $doc:literal) => {
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
                if WORKFLOW_NAME_REGEX.is_match(input) {
                    Ok(Self(input.to_string()))
                } else {
                    Err(willikins_types::ParseError::new(
                        stringify!($name),
                        format!(
                            "{input:?} is not a valid {} (expected to match `{WORKFLOW_NAME_PATTERN}`)",
                            stringify!($name),
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
                    "pattern": WORKFLOW_NAME_PATTERN,
                })
            }
        }
    };
}

workflow_identifier!(
    InputName,
    "The name of a workflow input: `snake_case`, starting with a letter."
);
workflow_identifier!(
    NodeName,
    "The name of a workflow node (a `steps.<name>` entry): `snake_case`, starting with a letter."
);
workflow_identifier!(
    OutputName,
    "The name of a workflow output: `snake_case`, starting with a letter."
);

/// The declared shape of one workflow input.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InputSpec {
    /// The type this input accepts.
    pub ty: TypeRef,
    /// The value used when the workflow is run without this input bound.
    /// An input with a default is never [`crate::check::CheckWarning`]'s
    /// sibling concept of "missing" at the `describe` stage.
    pub default: Option<Value>,
    /// One-line description shown to an agent, verbatim from the
    /// document. Bounded and control-character-free by construction — see
    /// [`willikins_types::Description`].
    pub description: Option<willikins_types::Description>,
}

impl InputSpec {
    /// A required input of type `ty`, with no default and no description.
    #[must_use]
    pub fn new(ty: TypeRef) -> Self {
        Self {
            ty,
            default: None,
            description: None,
        }
    }

    /// Attach a default value, making this input optional.
    #[must_use]
    pub fn with_default(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    /// Attach a one-line description.
    #[must_use]
    pub fn with_description(mut self, description: willikins_types::Description) -> Self {
        self.description = Some(description);
        self
    }
}

/// Where one node port's value, a `for_each` source, or a workflow output
/// comes from.
///
/// The only expression form: a workflow document's `${{ ... }}`
/// placeholders parse into these, and anything else is a [`Self::Literal`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Binding {
    /// A workflow input, by name.
    Input(InputName),
    /// One port's output on another node.
    Step {
        /// The node whose output this binding reads.
        node: NodeName,
        /// The output port to read.
        port: PortName,
    },
    /// One instance's scalar output on a `for_each` node, selected by key.
    ///
    /// The key is not validated by [`crate::check::check`]; it is matched
    /// against each item's canonical string at plan time.
    Keyed {
        /// The `for_each` node whose instance this binding reads.
        node: NodeName,
        /// The instance key.
        key: String,
        /// The output port to read from that instance.
        port: PortName,
    },
    /// The current item inside a `for_each` node. Valid only there.
    Item,
    /// A literal string, parsed against the bound port's scalar type at
    /// check time. Never valid for a secret-accepting port.
    Literal(String),
}

/// One node in a workflow graph: a call to one tool, optionally expanded
/// once per item of a `for_each` source.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    /// The tool this node calls.
    pub tool: ToolName,
    /// When set, this node runs once per item of the bound list, with
    /// [`Binding::Item`] available inside its own `with` bindings.
    pub for_each: Option<Binding>,
    /// This node's input port bindings, in declaration order.
    pub with: IndexMap<PortName, Binding>,
}

impl Node {
    /// A node calling `tool`, with no `for_each` and no bound ports.
    #[must_use]
    pub fn new(tool: ToolName) -> Self {
        Self {
            tool,
            for_each: None,
            with: IndexMap::new(),
        }
    }

    /// Expand this node once per item of `binding`'s list.
    #[must_use]
    pub fn for_each(mut self, binding: Binding) -> Self {
        self.for_each = Some(binding);
        self
    }

    /// Bind `port` to `binding`.
    #[must_use]
    pub fn port(mut self, port: PortName, binding: Binding) -> Self {
        self.with.insert(port, binding);
        self
    }
}

/// A provisioning workflow: named inputs, a graph of tool-calling nodes,
/// and named outputs. Checked by [`crate::check::check`] before it can be
/// described or planned.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Workflow {
    /// The workflow's name.
    pub name: willikins_types::WorkflowName,
    /// One-line description shown to an agent, verbatim from the
    /// document.
    pub description: Option<willikins_types::Description>,
    /// The workflow's declared inputs, in declaration order.
    pub inputs: IndexMap<InputName, InputSpec>,
    /// The workflow's nodes, in declaration order.
    pub nodes: IndexMap<NodeName, Node>,
    /// The workflow's outputs, in declaration order.
    pub outputs: IndexMap<OutputName, Binding>,
}

impl Workflow {
    /// An empty workflow named `name`, with no description, inputs, nodes,
    /// or outputs.
    #[must_use]
    pub fn new(name: willikins_types::WorkflowName) -> Self {
        Self {
            name,
            description: None,
            inputs: IndexMap::new(),
            nodes: IndexMap::new(),
            outputs: IndexMap::new(),
        }
    }

    /// Attach a one-line description.
    #[must_use]
    pub fn with_description(mut self, description: willikins_types::Description) -> Self {
        self.description = Some(description);
        self
    }

    /// Declare an input.
    #[must_use]
    pub fn input(mut self, name: InputName, spec: InputSpec) -> Self {
        self.inputs.insert(name, spec);
        self
    }

    /// Add a node.
    #[must_use]
    pub fn node(mut self, name: NodeName, node: Node) -> Self {
        self.nodes.insert(name, node);
        self
    }

    /// Declare an output.
    #[must_use]
    pub fn output(mut self, name: OutputName, binding: Binding) -> Self {
        self.outputs.insert(name, binding);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::DomainType;

    #[test]
    fn input_name_accepts_snake_case_and_rejects_uppercase() {
        assert!(InputName::parse("org").is_ok());
        assert!(InputName::parse("Org").is_err());
        assert!(InputName::parse("").is_err());
    }

    #[test]
    fn node_name_and_output_name_share_the_same_pattern() {
        assert!(NodeName::parse("ci_secret").is_ok());
        assert!(OutputName::parse("repo_url").is_ok());
        assert!(NodeName::parse("1bad").is_err());
        assert!(OutputName::parse("Bad").is_err());
    }

    #[test]
    fn names_display_and_serialize_as_their_string() {
        let name = NodeName::parse("repo").unwrap();
        assert_eq!(name.to_string(), "repo");
        assert_eq!(serde_json::to_string(&name).unwrap(), "\"repo\"");
        assert_eq!(serde_json::from_str::<NodeName>("\"repo\"").unwrap(), name);
    }

    #[test]
    fn binding_serializes_adjacently_tagged() {
        let input = Binding::Input(InputName::parse("org").unwrap());
        assert_eq!(
            serde_json::to_string(&input).unwrap(),
            r#"{"kind":"input","value":"org"}"#
        );

        let step = Binding::Step {
            node: NodeName::parse("names").unwrap(),
            port: PortName::parse("github_repo").unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&step).unwrap(),
            r#"{"kind":"step","value":{"node":"names","port":"github_repo"}}"#
        );

        let keyed = Binding::Keyed {
            node: NodeName::parse("configs").unwrap(),
            key: "prd".to_string(),
            port: PortName::parse("config").unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&keyed).unwrap(),
            r#"{"kind":"keyed","value":{"node":"configs","key":"prd","port":"config"}}"#
        );

        assert_eq!(
            serde_json::to_string(&Binding::Item).unwrap(),
            r#"{"kind":"item"}"#
        );
        assert_eq!(
            serde_json::to_string(&Binding::Literal("private".to_string())).unwrap(),
            r#"{"kind":"literal","value":"private"}"#
        );
    }

    fn workflow_name(name: &str) -> willikins_types::WorkflowName {
        willikins_types::WorkflowName::parse(name).unwrap()
    }

    fn description(text: &str) -> willikins_types::Description {
        willikins_types::Description::parse(text).unwrap()
    }

    #[test]
    fn workflow_builder_round_trips_into_the_expected_shape() {
        let workflow = Workflow::new(workflow_name("demo"))
            .with_description(description("A demo workflow."))
            .input(
                InputName::parse("org").unwrap(),
                InputSpec::new(TypeRef::scalar(
                    crate::value::TypeName::parse("GitHubOrg").unwrap(),
                )),
            )
            .node(
                NodeName::parse("names").unwrap(),
                Node::new(ToolName::parse("naming.v1").unwrap()).port(
                    PortName::parse("org").unwrap(),
                    Binding::Input(InputName::parse("org").unwrap()),
                ),
            )
            .output(
                OutputName::parse("repo").unwrap(),
                Binding::Step {
                    node: NodeName::parse("names").unwrap(),
                    port: PortName::parse("github_repo").unwrap(),
                },
            );

        assert_eq!(workflow.name.as_str(), "demo");
        assert_eq!(
            workflow
                .description
                .as_ref()
                .map(willikins_types::Description::as_str),
            Some("A demo workflow.")
        );
        assert_eq!(workflow.inputs.len(), 1);
        assert_eq!(workflow.nodes.len(), 1);
        assert_eq!(workflow.outputs.len(), 1);

        let json = serde_json::to_value(&workflow).unwrap();
        assert_eq!(json["name"], "demo");
        assert_eq!(json["description"], "A demo workflow.");
        assert_eq!(json["inputs"]["org"]["ty"], "GitHubOrg");
        assert_eq!(json["nodes"]["names"]["tool"], "naming.v1");
        assert_eq!(json["outputs"]["repo"]["kind"], "step");
    }

    #[test]
    fn input_spec_default_serializes_through_value() {
        let spec = InputSpec::new(TypeRef::scalar(
            crate::value::TypeName::parse("RepoVisibility").unwrap(),
        ))
        .with_default(Value::known(willikins_types::RepoVisibility::Private));
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["default"]["value"], "private");
    }
}
