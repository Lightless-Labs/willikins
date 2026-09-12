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
//! *Text* rendering is not fully disambiguated: [`Site::Output`]'s
//! `outputs.<name>` and a real node literally named `outputs` with a port
//! named `<name>` (a [`Site::Port`]) render to the identical string. JSON
//! output still disambiguates unconditionally (`kind` differs); only the
//! CLI's hand-written text renderer can coincide, and only in that one
//! contrived case. Not fixed here -- flagged for whoever next revisits
//! `willikins-cli`'s text rendering.

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
            Self::Output { name } => write!(f, "outputs.{name}"),
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

    #[test]
    fn output_displays_under_outputs() {
        let site = Site::Output { name: output("x") };
        assert_eq!(site.to_string(), "outputs.x");
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
