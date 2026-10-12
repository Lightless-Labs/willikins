//! The tool catalog: every tool a workflow can call, plus the type
//! registry they are checked against.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::rollback::{Reversal, ReversalError, Within};
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
    /// The tool's [`crate::rollback::Reversal`] declaration is invalid.
    #[error("tool `{name}` has an invalid reversal: {error}")]
    Reversal {
        /// The tool whose reversal declaration is invalid.
        name: ToolName,
        /// Why it is invalid.
        #[source]
        error: ReversalError,
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
    /// own non-secret, [`PortType::Exact`] input ports,
    /// [`CatalogError::Reversal`] when its [`crate::rollback::Reversal`]
    /// declaration is invalid on its own side (milestone 3m decision 3),
    /// including when it declares `Delete` without [`Tool::inverse`]
    /// returning `Some`, or declares `Contained`/`Retained` while
    /// `inverse()` returns `Some` (milestone 3m task R3), or
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
        self.validate_reversal(tool.spec(), &tool.reversal(), tool.inverse().is_some())
            .map_err(|error| CatalogError::Reversal {
                name: name.clone(),
                error,
            })?;
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

    /// This tool's own side of its [`crate::rollback::Reversal`]
    /// declaration (milestone 3m decisions 3 and, for the `has_inverse`
    /// check, R3): `reversal` must be `Delete` exactly when `has_inverse`
    /// (the tool's own [`Tool::inverse`] returning `Some`) is `true`;
    /// every `needs` port and the left side of every `within` pair must
    /// name one of the tool's own input ports (a `within` one also
    /// non-secret, and each `within` mapping at least one port), every
    /// loss name must match the required pattern, and a pure tool's
    /// reversal must be the default ([`crate::rollback::Reversal::Retained`]
    /// with [`crate::rollback::UNDECLARED`]) — a pure tool has no external
    /// state, so nothing about it can be undone. The container side of a
    /// `within` pair names another tool entirely, which this method, like
    /// `validate_gate`, cannot see: that is checked elsewhere (milestone
    /// 3m's declaration table test over `LIVE_TOOL_NAMES`, and the
    /// fake-catalog parity test).
    fn validate_reversal(
        &self,
        spec: &ToolSpec,
        reversal: &Reversal,
        has_inverse: bool,
    ) -> Result<(), ReversalError> {
        match reversal {
            Reversal::Delete { .. } if !has_inverse => {
                return Err(ReversalError::DeleteWithoutInverse);
            }
            Reversal::Contained { .. } | Reversal::Retained { .. } if has_inverse => {
                return Err(ReversalError::InverseWithoutDelete);
            }
            _ => {}
        }
        if spec.pure {
            return match reversal {
                Reversal::Retained { why } if *why == crate::rollback::UNDECLARED => Ok(()),
                _ => Err(ReversalError::PureToolDeclaresReversal),
            };
        }
        match reversal {
            Reversal::Delete {
                needs,
                within,
                losses,
            } => {
                for port in *needs {
                    Self::validate_own_input_port(spec, port)
                        .map_err(|port| ReversalError::NeedsNotAnInput { port })?;
                }
                for within in *within {
                    self.validate_within_own_side(spec, within)?;
                }
                for loss in *losses {
                    if !crate::rollback::LOSS_NAME_REGEX.is_match(loss) {
                        return Err(ReversalError::LossNameInvalid {
                            loss: (*loss).to_string(),
                        });
                    }
                }
                Ok(())
            }
            Reversal::Contained {
                within,
                otherwise: _,
            } => {
                for within in *within {
                    self.validate_within_own_side(spec, within)?;
                }
                Ok(())
            }
            Reversal::Retained { why: _ } => Ok(()),
        }
    }

    /// Whether `name` is one of `spec`'s own input ports, as a `String`
    /// error carrying `name` back to the caller's own error variant.
    fn validate_own_input_port(spec: &ToolSpec, name: &str) -> Result<(), String> {
        let Ok(port) = PortName::parse(name) else {
            return Err(name.to_string());
        };
        if spec.inputs.contains_key(&port) {
            Ok(())
        } else {
            Err(name.to_string())
        }
    }

    /// A [`Within`]'s own side: it maps at least one port (an empty
    /// mapping would match every container instance, since "every pair is
    /// equal" holds vacuously), and every `(own_port, side,
    /// container_port)` triple's `own_port` must name one of `spec`'s own
    /// input ports, of a non-secret type: the journal keeps only a
    /// redaction marker for a secret, re-parsed as `Unknown`, so a secret
    /// pair would compare equal for any two instances.
    fn validate_within_own_side(
        &self,
        spec: &ToolSpec,
        within: &Within,
    ) -> Result<(), ReversalError> {
        if within.ports.is_empty() {
            return Err(ReversalError::WithinMapsNoPort {
                tool: within.tool.to_string(),
            });
        }
        for &(own_port, _side, _container_port) in within.ports {
            Self::validate_own_input_port(spec, own_port)
                .map_err(|port| ReversalError::WithinNotAnInput { port })?;
            let secret = PortName::parse(own_port)
                .ok()
                .and_then(|port| spec.inputs.get(&port))
                .is_some_and(|port_spec| match &port_spec.ty {
                    PortType::AnySecret => true,
                    PortType::Exact(ty) => self.registry.is_secret(&ty.name) == Some(true),
                });
            if secret {
                return Err(ReversalError::WithinPortSecret {
                    port: own_port.to_string(),
                });
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
    use crate::rollback::{Created, Deleted, Inverse, Side, Standing};
    use crate::tool::{Ensured, Inputs, Observation, Outputs, PortName, PortSpec, ToolError};
    use crate::value::{PortType, TypeName, TypeRef};
    use willikins_types::{SinkToken, TeardownToken};

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

    /// A no-op [`Inverse`], never actually called: these tests exercise
    /// `Catalog::insert`'s validation only, never a teardown (milestone 3m
    /// task R3).
    struct DummyInverse;

    impl Inverse for DummyInverse {
        fn standing(&self, _created: &Created<'_>) -> Result<Standing, ToolError> {
            Ok(Standing::Gone)
        }

        fn delete(
            &self,
            _created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, ToolError> {
            Ok(Deleted { already_gone: true })
        }
    }

    /// A dummy tool, otherwise shaped like [`DummyPureTool`] or
    /// [`DummyEnsureTool`], whose declared [`Reversal`] and
    /// [`Tool::inverse`] are both configurable, to exercise
    /// `Catalog::insert`'s reversal validation (milestone 3m tasks R1 and
    /// R3).
    struct DummyReversalTool {
        spec: ToolSpec,
        reversal: Reversal,
        inverse: Option<DummyInverse>,
    }

    impl DummyReversalTool {
        fn pure(reversal: Reversal) -> Self {
            Self {
                spec: DummyPureTool::new().spec,
                reversal,
                inverse: None,
            }
        }

        fn stateful(reversal: Reversal) -> Self {
            Self {
                spec: DummyEnsureTool::new().spec,
                reversal,
                inverse: None,
            }
        }

        /// [`Self::stateful`], plus two secret input ports: `value`, of
        /// the registered secret type `DopplerSecretValue`, and `any`, of
        /// [`PortType::AnySecret`].
        fn stateful_with_secret_inputs(reversal: Reversal) -> Self {
            let mut spec = DummyEnsureTool::new().spec;
            spec.inputs.insert(
                PortName::parse("value").unwrap(),
                PortSpec {
                    ty: PortType::Exact(TypeRef::scalar(
                        TypeName::parse("DopplerSecretValue").unwrap(),
                    )),
                    required: true,
                    derived_only: false,
                },
            );
            spec.inputs.insert(
                PortName::parse("any").unwrap(),
                PortSpec {
                    ty: PortType::AnySecret,
                    required: true,
                    derived_only: false,
                },
            );
            Self {
                spec,
                reversal,
                inverse: None,
            }
        }

        /// This tool's [`Tool::inverse`] now returns `Some`.
        fn with_inverse(mut self) -> Self {
            self.inverse = Some(DummyInverse);
            self
        }
    }

    impl Tool for DummyReversalTool {
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

        fn reversal(&self) -> Reversal {
            self.reversal
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            self.inverse.as_ref().map(|inverse| inverse as &dyn Inverse)
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
        // task T3b's `Text` row and milestone 3g task E2's `TemplateValue`
        // row, both out of the same `from` -- every registered conversion,
        // and nothing else. T3b's third row, `AppleBundleIdName`, was
        // removed on 2026-09-30 (0f15efd): Apple refuses a dot in a bundle
        // id's name (409 ENTITY_ERROR.ATTRIBUTE.INVALID, probed live), so
        // that conversion was never total.
        assert_eq!(
            json["conversions"],
            serde_json::json!([
                { "from": "AppleBundleIdentifier", "to": "AppleProfileName" },
                { "from": "AppleBundleIdentifier", "to": "Text" },
                { "from": "AppleBundleIdentifier", "to": "TemplateValue" },
            ])
        );
    }

    // -------------------------------------------------------------
    // Reversal validation (milestone 3m task R1, acceptance test 1's R1
    // half). The inverse-matching half of that acceptance test is below,
    // under "Reversal <-> inverse validation" (task R3).
    // -------------------------------------------------------------

    #[test]
    fn tool_reversal_defaults_to_retained_undeclared() {
        // Neither dummy tool overrides `Tool::reversal`, so this calls the
        // trait's own default directly -- not a value this test built
        // itself.
        assert!(matches!(
            DummyPureTool::new().reversal(),
            Reversal::Retained { why } if why == crate::rollback::UNDECLARED
        ));
        assert!(matches!(
            DummyEnsureTool::new().reversal(),
            Reversal::Retained { why } if why == crate::rollback::UNDECLARED
        ));
    }

    #[test]
    fn insert_accepts_a_tool_with_the_default_undeclared_reversal() {
        // Every existing tool, pure or not, is unaffected: it simply does
        // not override `reversal()`.
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(DummyPureTool::new())).unwrap();
        catalog.insert(Arc::new(DummyEnsureTool::new())).unwrap();
    }

    #[test]
    fn insert_rejects_a_pure_tool_declaring_a_reversal() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::pure(Reversal::Retained { why: "custom" });
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::PureToolDeclaresReversal,
                ..
            }
        ));
    }

    #[test]
    fn insert_accepts_a_pure_tool_with_the_default_reversal_declared_explicitly() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::pure(Reversal::Retained {
            why: crate::rollback::UNDECLARED,
        });
        catalog.insert(Arc::new(tool)).unwrap();
    }

    #[test]
    fn insert_rejects_a_needs_port_that_is_not_an_input() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &["no_such_port"],
            within: &[],
            losses: &[],
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::NeedsNotAnInput { .. },
                ..
            }
        ));
    }

    #[test]
    fn insert_accepts_a_needs_port_that_is_an_input() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &["repo"],
            within: &[],
            losses: &[],
        })
        .with_inverse();
        catalog.insert(Arc::new(tool)).unwrap();
    }

    #[test]
    fn insert_rejects_a_within_own_side_port_that_is_not_an_input() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[Within {
                tool: "test.container",
                ports: &[("no_such_port", Side::Output, "container_port")],
            }],
            losses: &[],
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinNotAnInput { .. },
                ..
            }
        ));
    }

    #[test]
    fn insert_accepts_a_within_own_side_port_that_is_an_input() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[Within {
                tool: "test.container",
                ports: &[("repo", Side::Output, "container_port")],
            }],
            losses: &[],
        })
        .with_inverse();
        catalog.insert(Arc::new(tool)).unwrap();
    }

    #[test]
    fn insert_rejects_a_within_own_side_port_on_a_contained_reversal_too() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Contained {
            within: &[Within {
                tool: "test.container",
                ports: &[("no_such_port", Side::Output, "container_port")],
            }],
            otherwise: "left in place",
        });
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinNotAnInput { .. },
                ..
            }
        ));
    }

    #[test]
    fn insert_rejects_a_malformed_loss_name() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[],
            losses: &["Builds"],
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::LossNameInvalid { .. },
                ..
            }
        ));
    }

    #[test]
    fn insert_accepts_a_well_formed_loss_name() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[],
            losses: &["builds"],
        })
        .with_inverse();
        catalog.insert(Arc::new(tool)).unwrap();
    }

    #[test]
    fn insert_accepts_a_nonpure_tool_with_a_custom_retained_reason() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Retained {
            why: "no ownership marker exists",
        });
        catalog.insert(Arc::new(tool)).unwrap();
    }

    // Milestone 3m's X1 pass over R1 (2026-10-12): a `Within` that maps no
    // port rides with every instance of its container tool, since "every
    // mapped pair is equal" holds vacuously. The container's inverse would
    // then tolerate, as this run's own, contents in containers the
    // instance never touched. Refused at insert, on the tool's own side.

    #[test]
    fn insert_rejects_a_within_that_maps_no_port() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[Within {
                tool: "test.container",
                ports: &[],
            }],
            losses: &[],
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinMapsNoPort { ref tool },
                ..
            } if tool == "test.container"
        ));
    }

    #[test]
    fn insert_rejects_a_within_that_maps_no_port_on_a_contained_reversal_too() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Contained {
            within: &[Within {
                tool: "test.container",
                ports: &[],
            }],
            otherwise: "left in place",
        });
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinMapsNoPort { .. },
                ..
            }
        ));
    }

    // Milestone 3m's X1 pass over R1 (2026-10-12): the journal keeps only a
    // redaction marker for a secret, re-parsed as `Unknown`, and two
    // `Unknown`s of one type compare equal. A `Within` pair on a secret own
    // port therefore matches any two instances. Refused at insert, as
    // `validate_gate` refuses a secret subject. `needs` stays free to name
    // a secret credential port (decision 5).

    #[test]
    fn insert_rejects_a_within_own_side_port_of_a_secret_type() {
        assert_eq!(
            willikins_types::registry().is_secret(&TypeName::parse("DopplerSecretValue").unwrap()),
            Some(true)
        );
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful_with_secret_inputs(Reversal::Contained {
            within: &[Within {
                tool: "test.container",
                ports: &[
                    ("repo", Side::Input, "repo"),
                    ("value", Side::Input, "value"),
                ],
            }],
            otherwise: "left in place",
        });
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinPortSecret { ref port },
                ..
            } if port == "value"
        ));
    }

    #[test]
    fn insert_rejects_a_within_own_side_port_of_any_secret() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful_with_secret_inputs(Reversal::Delete {
            needs: &[],
            within: &[Within {
                tool: "test.container",
                ports: &[("any", Side::Output, "any")],
            }],
            losses: &[],
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::WithinPortSecret { ref port },
                ..
            } if port == "any"
        ));
    }

    #[test]
    fn insert_accepts_a_secret_needs_port() {
        // A credential port is secret by design (decision 5): `needs` may
        // name one; only `within` may not.
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful_with_secret_inputs(Reversal::Delete {
            needs: &["repo", "value", "any"],
            within: &[],
            losses: &[],
        })
        .with_inverse();
        catalog.insert(Arc::new(tool)).unwrap();
    }

    // -------------------------------------------------------------
    // Reversal <-> inverse validation (milestone 3m task R3, acceptance
    // test 1's inverse-matching half): `Catalog::insert` requires
    // `Reversal::Delete` exactly when `Tool::inverse` returns `Some`.
    // -------------------------------------------------------------

    #[test]
    fn insert_rejects_a_delete_reversal_without_an_inverse() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &[],
            within: &[],
            losses: &[],
        });
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::DeleteWithoutInverse,
                ..
            }
        ));
    }

    #[test]
    fn insert_rejects_a_contained_reversal_with_an_inverse() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Contained {
            within: &[],
            otherwise: "left in place",
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::InverseWithoutDelete,
                ..
            }
        ));
    }

    #[test]
    fn insert_rejects_a_retained_reversal_with_an_inverse() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Retained {
            why: "no ownership marker exists",
        })
        .with_inverse();
        let err = catalog.insert(Arc::new(tool)).unwrap_err();
        assert!(matches!(
            err,
            CatalogError::Reversal {
                error: ReversalError::InverseWithoutDelete,
                ..
            }
        ));
    }

    #[test]
    fn insert_accepts_a_delete_reversal_with_an_inverse() {
        let mut catalog = Catalog::new(willikins_types::registry());
        let tool = DummyReversalTool::stateful(Reversal::Delete {
            needs: &["repo"],
            within: &[],
            losses: &[],
        })
        .with_inverse();
        catalog.insert(Arc::new(tool)).unwrap();
    }
}
