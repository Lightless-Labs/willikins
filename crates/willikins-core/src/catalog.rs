//! The tool catalog: every tool a workflow can call, plus the type
//! registry they are checked against.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::tool::{SpecError, Tool, ToolName, ToolSpec};
use crate::value::TypeRegistry;
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
    /// validate, or [`CatalogError::Duplicate`] when a tool with the same
    /// name is already present.
    pub fn insert(&mut self, tool: Arc<dyn Tool>) -> Result<(), CatalogError> {
        let name = tool.spec().name.clone();
        tool.spec()
            .validate(self.registry)
            .map_err(|error| CatalogError::Spec {
                name: name.clone(),
                error,
            })?;
        if self.tools.contains_key(&name) {
            return Err(CatalogError::Duplicate(name));
        }
        self.tools.insert(name, tool);
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
        map.insert("tools".to_string(), to_json_value(&tools));
        map.insert("types".to_string(), to_json_value(&types));
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
    use crate::tool::{Inputs, Observation, Outputs, PortName, PortSpec, ToolError};
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

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
            Ok(Outputs::new())
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

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
            Ok(Outputs::new())
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
    }
}
