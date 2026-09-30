//! The tool catalog: every tool a workflow can call, plus the type
//! registry they are checked against.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::tool::{GateError, PortName, SpecError, Tool, ToolName, ToolSpec};
use crate::value::{PortType, TypeRegistry};
use willikins_types::TypeInfo;

/// Why [`Catalog::insert`] refused a tool.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The tool's own [`ToolSpec::validate`] failed.
    #[error("tool `{name}` has an invalid spec: {error}")]
    Spec {
        /// The tool that failed to validate.
        name: ToolName,
        /// Why it failed.
        #[source]
        error: SpecError,
    },
    /// A tool with this name is already in the catalog.
    #[error("a tool named `{0}` is already in the catalog")]
    Duplicate(ToolName),
    /// The tool's [`crate::tool::Gate`] declaration is invalid.
    #[error("tool `{name}` has an invalid gate: {error}")]
    Gate {
        /// The tool whose gate declaration is invalid.
        name: ToolName,
        /// Why it is invalid.
        #[source]
        error: GateError,
    },
}

/// Every tool a workflow can call, keyed by name, plus the type registry
/// they are checked against.
pub struct Catalog {
    tools: IndexMap<ToolName, Arc<dyn Tool>>,
    registry: &'static TypeRegistry,
}

impl Catalog {
    /// An empty catalog checked against `registry`.
    #[must_use]
    pub fn new(registry: &'static TypeRegistry) -> Self {
        Self {
            tools: IndexMap::new(),
            registry,
        }
    }

    /// Validate `tool`'s spec against this catalog's registry, then add it.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::Spec`] when the tool's spec does not
    /// validate, [`CatalogError::Gate`] when it declares a [`crate::tool::Gate`]
    /// that is not pure or whose `subject` names anything but one of its
    /// own non-secret, [`PortType::Exact`] input ports, or
    /// [`CatalogError::Duplicate`] when a tool with the same name is
    /// already present.
    pub fn insert(&mut self, tool: Arc<dyn Tool>) -> Result<(), CatalogError> {
        let name = tool.spec().name.clone();
        tool.spec()
            .validate(self.registry)
            .map_err(|error| CatalogError::Spec {
                name: name.clone(),
                error,
            })?;
        if let Some(gate) = tool.gate() {
            self.validate_gate(tool.spec(), gate)
                .map_err(|error| CatalogError::Gate {
                    name: name.clone(),
                    error,
                })?;
        }
        if self.tools.contains_key(&name) {
            return Err(CatalogError::Duplicate(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// A gate's tool must be pure, and every `subject` entry must name one
    /// of the tool's own input ports, of a non-secret [`PortType::Exact`]
    /// type: never [`PortType::AnySecret`], never a secret scalar or list.
    fn validate_gate(&self, spec: &ToolSpec, gate: &crate::tool::Gate) -> Result<(), GateError> {
        if !spec.pure {
            return Err(GateError::NotPure);
        }
        for name in gate.subject {
            let Ok(port) = PortName::parse(name) else {
                return Err(GateError::SubjectNotAnInput {
                    port: (*name).to_string(),
                });
            };
            let Some(port_spec) = spec.inputs.get(&port) else {
                return Err(GateError::SubjectNotAnInput {
                    port: (*name).to_string(),
                });
            };
            match &port_spec.ty {
                PortType::AnySecret => {
                    return Err(GateError::SubjectNotExact {
                        port: (*name).to_string(),
                    });
                }
                PortType::Exact(ty) => {
                    if self.registry.is_secret(&ty.name) == Some(true) {
                        return Err(GateError::SubjectSecret {
                            port: (*name).to_string(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The tool named `name`, if it is in this catalog.
    #[must_use]
    pub fn get(&self, name: &ToolName) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Every tool's spec, in insertion order.
    pub fn specs(&self) -> impl Iterator<Item = &ToolSpec> {
        self.tools.values().map(|tool| tool.spec())
    }

    /// The type registry this catalog validates tools against.
    #[must_use]
    pub fn registry(&self) -> &'static TypeRegistry {
        self.registry
    }

    /// The full catalog — every tool's spec and every registered type — as
    /// JSON, for the `list_tools` MCP tool and the `schema --catalog` CLI
    /// output.
    #[must_use]
    pub fn list_tools_json(&self) -> serde_json::Value {
        let tools: Vec<&ToolSpec> = self.specs().collect();
        let types: Vec<&TypeInfo> = self.registry.iter().map(|entry| &entry.info).collect();
        let mut map = serde_json::Map::new();
        // Milestone 3d, decision (g): every registered conversion, names
        // only and in declaration order (never a `HashMap` iteration).
        // A port of type `to` also accepts a scalar binding of type
        // `from`, converted in one hop; nothing else converts.
        let conversions: Vec<serde_json::Value> = self
            .registry
            .conversion_pairs()
            .map(|(from, to)| serde_json::json!({ "from": from, "to": to }))
            .collect();
        map.insert("tools".to_string(), to_json_value(&tools));
        map.insert("types".to_string(), to_json_value(&types));
        map.insert(
            "conversions".to_string(),
            serde_json::Value::Array(conversions),
        );
        serde_json::Value::Object(map)
    }
}

/// Serialize `value` to `serde_json::Value`.
///
/// Every type this crate feeds through here (`ToolSpec`, `TypeInfo`, and
/// the plain `Vec`s of them above) serializes to JSON without error by
/// construction — none of their `Serialize` impls can fail — so a failure
/// here would be a bug in one of those impls, not bad data.
fn to_json_value<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value)
        .unwrap_or_else(|err| unreachable!("catalog data always serializes to JSON: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::Class;
    use crate::tool::{Ensured, Inputs, Observation, Outputs, PortName, PortSpec, ToolError};
    use crate::value::{PortType, TypeName, TypeRef};
    use willikins_types::SinkToken;

    /// A minimal pure tool: `naming`-shaped, no key, always `Reversible`.
    struct DummyPureTool {
        spec: ToolSpec,
    }

    impl DummyPureTool {
        fn new() -> Self {
            let mut inputs = IndexMap::new();
            inputs.insert(
                PortName::parse("org").unwrap(),
                PortSpec {
                    ty: PortType::Exact(TypeRef::scalar(TypeName::parse("GitHubOrg").unwrap())),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(
                PortName::parse("repo").unwrap(),
                TypeRef::scalar(TypeName::parse("GitHubRepo").unwrap()),
            );
            Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.pure").unwrap(),
                    description: "A dummy pure tool.".to_string(),
                    inputs,
                    outputs,
                    key: Vec::new(),
                    class: Class::Reversible,
                    pure: true,
                },
            }
        }
    }

    impl Tool for DummyPureTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            Ok(Observation::Present(Outputs::new()))
        }

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: false,
            })
        }
    }

    /// A minimal reversible, stateful tool: `github.repo.ensure`-shaped.
    struct DummyEnsureTool {
        spec: ToolSpec,
    }

    impl DummyEnsureTool {
        fn new() -> Self {
            let mut inputs = IndexMap::new();
            inputs.insert(
                PortName::parse("repo").unwrap(),
                PortSpec {
                    ty: PortType::Exact(TypeRef::scalar(TypeName::parse("GitHubRepo").unwrap())),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(
                PortName::parse("repo").unwrap(),
                TypeRef::scalar(TypeName::parse("GitHubRepo").unwrap()),
            );
            Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.ensure").unwrap(),
                    description: "A dummy stateful tool.".to_string(),
                    inputs,
                    outputs,
                    key: vec![PortName::parse("repo").unwrap()],
                    class: Class::Reversible,
                    pure: false,
                },
            }
        }
    }

    impl Tool for DummyEnsureTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
        }
    }

    #[test]
    fn insert_and_get_round_trip() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let name = ToolName::parse("test.pure").unwrap();
        catalog.insert(Arc::new(DummyPureTool::new())).unwrap();
        assert!(catalog.get(&name).is_some());
    }

    #[test]
    fn insert_rejects_a_duplicate_name() {
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(DummyPureTool::new())).unwrap();
        let err = catalog.insert(Arc::new(DummyPureTool::new())).unwrap_err();
        assert!(matches!(err, CatalogError::Duplicate(_)));
    }

    #[test]
    fn insert_rejects_an_invalid_spec() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let mut tool = DummyPureTool::new();
        tool.spec.class = Class::Irreversible;
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(err, CatalogError::Spec { .. }));
    }

    #[test]
    fn get_returns_none_for_an_unknown_name() {
        let catalog = Catalog::new(willikins_types::registry());
        assert!(
            catalog
                .get(&ToolName::parse("no.such.tool").unwrap())
                .is_none()
        );
    }

    #[test]
    fn specs_lists_every_tool_in_insertion_order() {
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(DummyPureTool::new())).unwrap();
        catalog.insert(Arc::new(DummyEnsureTool::new())).unwrap();
        let names: Vec<&str> = catalog.specs().map(|spec| spec.name.as_str()).collect();
        assert_eq!(names, vec!["test.pure", "test.ensure"]);
    }

    #[test]
    fn list_tools_json_snapshot() {
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(DummyPureTool::new())).unwrap();
        catalog.insert(Arc::new(DummyEnsureTool::new())).unwrap();
        let json = catalog.list_tools_json();
        // The type catalog is large and covered by willikins-types' own
        // snapshot; only the tool list is worth pinning here.
        insta::assert_json_snapshot!(json["tools"]);
        assert!(
            json["types"]
                .as_array()
                .is_some_and(|types| !types.is_empty())
        );
        // Milestone 3d, acceptance test 10 (`AppleProfileName`), plus
        // task T3b's `Text` row out of the same `from` -- every
        // registered conversion, and nothing else. T3b's third row,
        // `AppleBundleIdName`, was removed on 2026-09-30 (0f15efd): Apple
        // refuses a dot in a bundle id's name (409
        // ENTITY_ERROR.ATTRIBUTE.INVALID, probed live), so that
        // conversion was never total.
        assert_eq!(
            json["conversions"],
            serde_json::json!([
                { "from": "AppleBundleIdentifier", "to": "AppleProfileName" },
                { "from": "AppleBundleIdentifier", "to": "Text" },
            ])
        );
    }
}
