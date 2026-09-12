//! [`Site`]: identifies where in a workflow a binding lives, so `check` and
//! `plan` errors do not need a synthetic node or port name to describe a
//! workflow output's or a `for_each` binding's own location.
//!
//! Before this type, an output binding's own errors were reported under a
//! synthetic node named `outputs`, and a `for_each` binding's under a
//! synthetic port named `for_each` (`check.rs`'s own now-deleted
//! `outputs_node()` and `for_each_port()` helpers; see
//! `todos/2026-09-12-check-error-site-enum.md`). A workflow with a real
//! node literally named `outputs`, or a real `with` key literally named
//! `for_each`, rendered identically to the synthetic case in both text and
//! JSON -- the adversarial pass's finding 2 is one consequence of this. A
//! `Site` gives each of the three locations its own `kind` tag on the wire,
//! so the two can never be confused there.
//!
//! Text rendering is disambiguated too, which matters because
//! `willikins-cli`'s `check_error_detail` renders a site through
//! [`fmt::Display`] and nothing else. Every identifier matches
//! `^[a-z][a-z0-9_]*$`, so it holds no `.` and no `[`: a [`Site::Port`] is
//! always two dot-separated segments, a [`Site::ForEach`] always carries a
//! `[`, and a [`Site::Output`] is always three dot-separated segments
//! under the `workflow.outputs` prefix. No two forms can coincide, whatever
//! a document names its steps, ports and outputs; see
//! `tests::no_two_site_forms_share_a_display_string`.

use std::fmt;

use crate::tool::PortName;
use crate::workflow::{NodeName, OutputName};

/// Where a binding that `check` or `plan` reported an error about lives.
///
/// Serializes internally tagged (`#[serde(tag = "kind", rename_all =
/// "snake_case")]`) as `{"kind": "port" | "for_each" | "output", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Site {
    /// A node's `with`-bound port -- either the binding's own location, or
    /// (for [`crate::CheckError::UnknownPort`]'s referenced-node form) the
    /// port that was looked up on the node a `Step` or `Keyed` binding
    /// named.
    Port {
        /// The node.
        node: NodeName,
        /// The port.
        port: PortName,
    },
    /// A node's own `for_each` binding.
    ForEach {
        /// The node.
        node: NodeName,
    },
    /// A workflow output's binding.
    Output {
        /// The output.
        name: OutputName,
    },
}

impl Site {
    /// The node this site belongs to, when it names one: `Some` for
    /// [`Self::Port`] and [`Self::ForEach`], `None` for [`Self::Output`]
    /// (a workflow output is not itself a node).
    #[must_use]
    pub fn node(&self) -> Option<&NodeName> {
        match self {
            Self::Port { node, .. } | Self::ForEach { node } => Some(node),
            Self::Output { .. } => None,
        }
    }
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port { node, port } => write!(f, "{node}.{port}"),
            Self::ForEach { node } => write!(f, "{node}[for_each]"),
            Self::Output { name } => write!(f, "workflow.outputs.{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    fn port(name: &str) -> PortName {
        PortName::parse(name).unwrap()
    }

    fn output(name: &str) -> OutputName {
        OutputName::parse(name).unwrap()
    }

    #[test]
    fn port_displays_dotted() {
        let site = Site::Port {
            node: node("n"),
            port: port("p"),
        };
        assert_eq!(site.to_string(), "n.p");
    }

    #[test]
    fn for_each_displays_bracketed() {
        let site = Site::ForEach { node: node("n") };
        assert_eq!(site.to_string(), "n[for_each]");
    }

    #[test]
    fn node_is_some_for_port_and_for_each_and_none_for_output() {
        assert_eq!(
            Site::Port {
                node: node("n"),
                port: port("p")
            }
            .node(),
            Some(&node("n"))
        );
        assert_eq!(Site::ForEach { node: node("n") }.node(), Some(&node("n")));
        assert_eq!(Site::Output { name: output("x") }.node(), None);
    }

    /// The `workflow.` prefix is what keeps this apart from a
    /// [`Site::Port`] on a node literally named `outputs`; see
    /// [`no_two_site_forms_share_a_display_string`].
    #[test]
    fn output_displays_under_workflow_outputs() {
        let site = Site::Output { name: output("x") };
        assert_eq!(site.to_string(), "workflow.outputs.x");
    }

    /// Three pairs a single document can produce at once: a node named
    /// `outputs`, a port named `for_each`, and an output named `for_each`
    /// are all legal identifiers. No two site forms may render to the same
    /// text -- `willikins-cli`'s `check_error_detail` renders a site
    /// through this `Display` and nothing else, so a collision here is a
    /// collision an agent reading the text output cannot resolve.
    #[test]
    fn no_two_site_forms_share_a_display_string() {
        assert_ne!(
            Site::Output { name: output("x") }.to_string(),
            Site::Port {
                node: node("outputs"),
                port: port("x"),
            }
            .to_string(),
        );
        assert_ne!(
            Site::Output {
                name: output("for_each"),
            }
            .to_string(),
            Site::ForEach {
                node: node("outputs"),
            }
            .to_string(),
        );
        assert_ne!(
            Site::Port {
                node: node("n"),
                port: port("for_each"),
            }
            .to_string(),
            Site::ForEach { node: node("n") }.to_string(),
        );
    }

    #[test]
    fn every_site_variant_serializes_with_its_kind() {
        let cases = [
            (
                Site::Port {
                    node: node("n"),
                    port: port("p"),
                },
                "port",
            ),
            (Site::ForEach { node: node("n") }, "for_each"),
            (Site::Output { name: output("x") }, "output"),
        ];
        for (site, kind) in cases {
            let json = serde_json::to_value(&site).unwrap();
            assert_eq!(json["kind"], kind, "site: {site:?}");
        }
    }

    /// Not just "does not panic": a real object schema for every variant,
    /// tagged the same way as the type's own `Serialize`.
    #[test]
    fn schema_generates_and_tags_every_variant() {
        let schema = schemars::schema_for!(Site);
        let json = schema.as_value();
        let rendered = serde_json::to_string(json).expect("schema serializes");
        for kind in ["port", "for_each", "output"] {
            assert!(
                rendered.contains(kind),
                "schema does not mention `{kind}`: {rendered}"
            );
        }
    }
}
