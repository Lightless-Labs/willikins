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

/// The pattern every [`OutputName`] must match: `snake_case`, starting
/// with a letter, exactly one segment. Shared in spirit with
/// [`crate::tool::PortName`], but declared separately here rather than
/// reused from `tool`, to keep the two modules independent.
const SEGMENT_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*$";

static SEGMENT_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(SEGMENT_NAME_PATTERN).expect("pattern is valid"));

/// The pattern every [`InputName`] and [`NodeName`] must match:
/// `snake_case` segments, each starting with a letter, joined by `/`.
///
/// Milestone 2b decision (d3): a linked node (or a used document's fixed
/// input) is named `<uses step>/<child name>`, applied recursively
/// (`app/org/base_gate`). An *authored* name is always one segment —
/// `willikins-dsl` refuses a `/` in a step key or input name
/// (`document_to_workflow`) — so a path of more than one segment can only
/// ever come from the linker (milestone 2b's `compose::link`), never from
/// a document directly.
const PATH_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*(/[a-z][a-z0-9_]*)*$";

static PATH_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(PATH_NAME_PATTERN).expect("pattern is valid"));

/// Declares one newtype identifier over `String`, validated on parse
/// against `$pattern`, with `Display`, `serde`, and `JsonSchema` support.
/// Mirrors [`crate::tool`]'s `identifier!` macro, parameterized the same
/// way: [`NodeName`] and [`InputName`] share [`PATH_NAME_PATTERN`] /
/// [`PATH_NAME_REGEX`], while [`OutputName`] keeps
/// [`SEGMENT_NAME_PATTERN`] / [`SEGMENT_NAME_REGEX`].
macro_rules! workflow_identifier {
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

workflow_identifier!(
    InputName,
    PATH_NAME_PATTERN,
    PATH_NAME_PATTERN,
    PATH_NAME_REGEX,
    "The name of a workflow input: `snake_case` segments joined by `/`, \
     each starting with a letter. An authored input name is always one \
     segment; a `/`-separated path names a used document's fixed input \
     (milestone 2b decision (d6))."
);
workflow_identifier!(
    NodeName,
    PATH_NAME_PATTERN,
    PATH_NAME_PATTERN,
    PATH_NAME_REGEX,
    "The name of a workflow node (a `steps.<name>` entry): `snake_case` \
     segments joined by `/`, each starting with a letter. An authored \
     step key is always one segment; a `/`-separated path names a linked \
     node produced by composing a `uses:` step with the document it uses \
     (milestone 2b decision (d3))."
);
workflow_identifier!(
    OutputName,
    SEGMENT_NAME_PATTERN,
    SEGMENT_NAME_PATTERN,
    SEGMENT_NAME_REGEX,
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
    /// `None` for every authored input. `Some(<uses step>)` when the
    /// linker (`willikins_core::compose::link`, milestone 2b decision
    /// (d6)) produced this input by leaving a used document's defaulted
    /// input unbound: the `uses:` step that fixed it, itself a path when
    /// the fixing happened more than one level down
    /// (`mid/leafstep` for an input fixed two levels deep). A fixed input
    /// is never settable by a caller — see the module and `describe`'s
    /// own docs for which surfaces hide it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_by: Option<NodeName>,
}

impl InputSpec {
    /// A required input of type `ty`, with no default and no description.
    #[must_use]
    pub fn new(ty: TypeRef) -> Self {
        Self {
            ty,
            default: None,
            description: None,
            fixed_by: None,
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
    /// A YAML sequence under `with:`: one binding per element, each a
    /// reference or a literal, never itself a [`Self::List`] (no nested
    /// lists; `willikins-dsl` refuses one at parse time, and `check`
    /// refuses one reaching it any other way). Valid only bound to a
    /// node's `with` port whose declared type is `list<T>`; never valid as
    /// a `for_each` source or a workflow output (milestone 3g, decision
    /// (a)).
    List(Vec<Binding>),
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

/// One `uses:` step: a reference to another document's workflow, not yet
/// linked into the graph (milestone 2b, part P).
///
/// A `uses:` step is not a node at run time: [`crate::check::check`]
/// refuses a [`Workflow`] whose `uses` map is non-empty
/// ([`crate::check::CheckError::Unlinked`]), before anything else, so a
/// composite must first go through the linker (`willikins_core::compose`,
/// added by milestone 2b task L1), which replaces every `uses:` step with
/// the used document's own nodes and clears this map.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Uses {
    /// The name of the workflow this step uses.
    pub workflow: willikins_types::WorkflowName,
    /// This step's input bindings, keyed by the used document's own
    /// [`InputName`]s (not [`crate::tool::PortName`]s — a used document
    /// has no ports of its own; its *declared inputs* are what a `with:`
    /// binds here).
    pub with: IndexMap<InputName, Binding>,
    /// This step's position among the document's `steps:` entries: where
    /// the used document's nodes are inserted once linked (SHARED VALUES,
    /// "Step order", milestone 2b plan).
    pub position: usize,
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
    /// The workflow's `uses:` steps, not yet linked into `nodes`, in
    /// declaration order. Empty for every workflow that has gone through
    /// the linker, or that never had a `uses:` step — and so, by
    /// `#[serde(skip_serializing_if)]`, a document without `uses:`
    /// serializes byte-identically to before milestone 2b.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub uses: IndexMap<NodeName, Uses>,
    /// The workflow's outputs, in declaration order.
    pub outputs: IndexMap<OutputName, Binding>,
    /// Every `uses:` step's input boundary, across the whole tree that was
    /// linked into this workflow. Filled only by the linker
    /// (`willikins_core::compose::link`, milestone 2b task L1); empty for
    /// a workflow that never had a `uses:` step, or that has not gone
    /// through the linker yet. Skipped when empty, alongside
    /// [`Self::uses`], so a document without `uses:` serializes
    /// byte-identically to before milestone 2b.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub boundaries: Vec<crate::compose::Boundary>,
}

impl Workflow {
    /// An empty workflow named `name`, with no description, inputs,
    /// nodes, `uses:` steps, outputs, or boundaries.
    #[must_use]
    pub fn new(name: willikins_types::WorkflowName) -> Self {
        Self {
            name,
            description: None,
            inputs: IndexMap::new(),
            nodes: IndexMap::new(),
            uses: IndexMap::new(),
            outputs: IndexMap::new(),
            boundaries: Vec::new(),
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

    /// Add a `uses:` step, unlinked.
    #[must_use]
    pub fn uses(mut self, name: NodeName, uses: Uses) -> Self {
        self.uses.insert(name, uses);
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
    fn node_name_and_output_name_both_reject_a_leading_digit_or_uppercase() {
        assert!(NodeName::parse("ci_secret").is_ok());
        assert!(OutputName::parse("repo_url").is_ok());
        assert!(NodeName::parse("1bad").is_err());
        assert!(OutputName::parse("Bad").is_err());
    }

    /// Milestone 2b, decision (d3) and acceptance 1: `NodeName` and
    /// `InputName` widen to a `/`-separated path of one-segment names,
    /// `PortName` and `OutputName` stay one segment.
    #[test]
    fn node_name_and_input_name_accept_a_slash_separated_path() {
        for ok in ["org/x", "a/b/c", "x"] {
            assert!(NodeName::parse(ok).is_ok(), "NodeName should accept {ok:?}");
            assert!(
                InputName::parse(ok).is_ok(),
                "InputName should accept {ok:?}"
            );
        }
        for bad in ["org/", "/x", "org//x", "Org/x", "org.x", "org/x.y", ""] {
            assert!(
                NodeName::parse(bad).is_err(),
                "NodeName should refuse {bad:?}"
            );
            assert!(
                InputName::parse(bad).is_err(),
                "InputName should refuse {bad:?}"
            );
        }
    }

    #[test]
    fn port_name_and_output_name_still_refuse_a_slash() {
        assert!(crate::tool::PortName::parse("a/b").is_err());
        assert!(OutputName::parse("a/b").is_err());
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

        let list = Binding::List(vec![
            Binding::Literal("a".to_string()),
            Binding::Input(InputName::parse("b").unwrap()),
        ]);
        assert_eq!(
            serde_json::to_string(&list).unwrap(),
            r#"{"kind":"list","value":[{"kind":"literal","value":"a"},{"kind":"input","value":"b"}]}"#
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

    /// Milestone 2b, decision (d1): `Workflow.uses` is skipped when empty,
    /// so a document without `uses:` serializes with no `uses` key at
    /// all -- the byte-identity claim the plan's Gates section pins for
    /// every pre-2b document. A workflow with one `uses:` step serializes
    /// its `workflow`, `with`, and `position` fields.
    #[test]
    fn workflow_uses_is_omitted_when_empty_and_present_when_not() {
        let empty = Workflow::new(workflow_name("demo"));
        let json = serde_json::to_value(&empty).unwrap();
        assert!(
            json.as_object().unwrap().get("uses").is_none(),
            "a uses-less workflow must not serialize a `uses` key: {json}"
        );

        let mut with = IndexMap::new();
        with.insert(
            InputName::parse("slug").unwrap(),
            Binding::Literal("example-app".to_string()),
        );
        let with_uses = empty.uses(
            NodeName::parse("org").unwrap(),
            Uses {
                workflow: workflow_name("example-org"),
                with,
                position: 0,
            },
        );
        let json = serde_json::to_value(&with_uses).unwrap();
        assert_eq!(json["uses"]["org"]["workflow"], "example-org");
        assert_eq!(json["uses"]["org"]["with"]["slug"]["kind"], "literal");
        assert_eq!(json["uses"]["org"]["position"], 0);
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
