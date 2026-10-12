//! Generated-[`Workflow`] test helpers, behind the `test-support` cargo
//! feature: proptest strategies over arbitrary (mostly nonsensical)
//! workflow graphs, parameterized by the caller's own tool/port/input/node
//! name universes so a caller can restrict what a generated graph can
//! possibly reference.
//!
//! Moved here from `willikins-core/tests/check_adversarial.rs` (task E of
//! `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s task 4b),
//! so `willikins-providers-fake`'s convergence property test can build
//! generated workflows too, restricted to its own catalog's tool names,
//! without duplicating the generator logic. `check_adversarial.rs` keeps
//! its own larger name universes (including deliberately-invalid names,
//! an adversarial concern this module has no opinion on) and calls these
//! functions with them.
//!
//! Gated behind `test-support` (which pulls in `proptest` as an optional
//! dependency) rather than plain `#[cfg(test)]`, because a dependent
//! crate's own tests need to call these functions on `willikins-core` as
//! a compiled dependency, not as `willikins-core`'s own test code —
//! `#[cfg(test)]` items are never visible outside the crate that defines
//! them. See the crate's `Cargo.toml` for the self-dev-dependency that
//! enables this feature for `willikins-core`'s own `tests/`, the same
//! trick `willikins-types` uses for its `executor` feature.

use indexmap::IndexMap;
use proptest::prelude::*;

use crate::class::Class;
use crate::rollback::{Created, Deleted, Inverse, Reversal, Side, Standing, Within};
use crate::tool::{Ensured, Inputs, Observation, Outputs, PortSpec, Tool, ToolError, ToolSpec};
use crate::value::{PortType, TypeRef, Value};
use crate::workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};
use crate::{PortName, ToolName};
use willikins_types::{DomainType, SinkToken, TeardownToken, WorkflowName};

fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}"))
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}"))
}

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}"))
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}"))
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}"))
}

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(
        crate::TypeName::parse(name)
            .unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}")),
    )
}

fn list_ty(name: &str) -> TypeRef {
    TypeRef::list_of(
        crate::TypeName::parse(name)
            .unwrap_or_else(|err| unreachable!("{name:?} is a valid name: {err}")),
    )
}

/// A generated [`Binding`]: an input reference, a step or keyed reference
/// into one of `node_names`, `Item`, or a literal drawn from `literals`.
/// `input_names`, `node_names`, `ports`, and `literals` bound the universe
/// every draw is selected from — the caller's, not this module's.
pub fn arb_binding(
    input_names: &'static [&'static str],
    node_names: &'static [&'static str],
    ports: &'static [&'static str],
    literals: &'static [&'static str],
) -> impl Strategy<Value = Binding> {
    prop_oneof![
        prop::sample::select(input_names).prop_map(|n| Binding::Input(input(n))),
        (
            prop::sample::select(node_names),
            prop::sample::select(ports)
        )
            .prop_map(|(n, p)| Binding::Step {
                node: node(n),
                port: port(p),
            }),
        (
            prop::sample::select(node_names),
            prop::sample::select(literals),
            prop::sample::select(ports)
        )
            .prop_map(|(n, k, p)| Binding::Keyed {
                node: node(n),
                key: k.to_string(),
                port: port(p),
            }),
        Just(Binding::Item),
        prop::sample::select(literals).prop_map(|l| Binding::Literal(l.to_string())),
    ]
}

/// A generated [`Node`]: a tool drawn from `tools`, an optional `for_each`
/// binding, and zero to three port bindings — every binding drawn from
/// [`arb_binding`] over the same name universes.
pub fn arb_node(
    tools: &'static [&'static str],
    ports: &'static [&'static str],
    input_names: &'static [&'static str],
    node_names: &'static [&'static str],
    literals: &'static [&'static str],
) -> impl Strategy<Value = Node> {
    (
        prop::sample::select(tools),
        prop::option::of(arb_binding(input_names, node_names, ports, literals)),
        prop::collection::vec(
            (
                prop::sample::select(ports),
                arb_binding(input_names, node_names, ports, literals),
            ),
            0..4,
        ),
    )
        .prop_map(|(tool, for_each, bindings)| {
            let mut node = Node::new(tool_name(tool));
            if let Some(binding) = for_each {
                node = node.for_each(binding);
            }
            for (p, binding) in bindings {
                node = node.port(port(p), binding);
            }
            node
        })
}

/// A generated [`InputSpec`]: a type drawn from `type_names`, scalar or
/// `list<T>`, with or without an [`Value::unknown`] default.
pub fn arb_input_spec(type_names: &'static [&'static str]) -> impl Strategy<Value = InputSpec> {
    (
        prop::sample::select(type_names),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(name, is_list, has_default)| {
            let declared = if is_list { list_ty(name) } else { ty(name) };
            let spec = InputSpec::new(declared.clone());
            if has_default {
                spec.with_default(Value::unknown(declared))
            } else {
                spec
            }
        })
}

/// A generated [`Workflow`] named `"generated"`: zero to three declared
/// inputs (from `arb_input_spec` over `type_names`), zero to four nodes
/// (from `arb_node` over `tools`/`ports`/`input_names`/`node_names`/`literals`),
/// and zero to two workflow outputs (bindings over the same universes).
/// Every name is drawn from the caller's own universe, so the caller
/// controls what a generated graph can possibly reference — including,
/// deliberately, names the caller's own catalog does not have, if its
/// universe includes one.
pub fn arb_workflow(
    tools: &'static [&'static str],
    ports: &'static [&'static str],
    input_names: &'static [&'static str],
    node_names: &'static [&'static str],
    literals: &'static [&'static str],
    type_names: &'static [&'static str],
) -> impl Strategy<Value = Workflow> {
    (
        prop::collection::vec(
            (
                prop::sample::select(input_names),
                arb_input_spec(type_names),
            ),
            0..3,
        ),
        prop::collection::vec(
            (
                prop::sample::select(node_names),
                arb_node(tools, ports, input_names, node_names, literals),
            ),
            0..4,
        ),
        prop::collection::vec(
            (
                prop::sample::select(input_names),
                arb_binding(input_names, node_names, ports, literals),
            ),
            0..2,
        ),
    )
        .prop_map(|(inputs, nodes, outputs)| {
            let mut workflow = Workflow::new(
                WorkflowName::parse("generated")
                    .unwrap_or_else(|err| unreachable!("\"generated\" is a valid name: {err}")),
            );
            for (name, spec) in inputs {
                workflow = workflow.input(input(name), spec);
            }
            for (name, n) in nodes {
                workflow = workflow.node(node(name), n);
            }
            for (name, binding) in outputs {
                workflow = workflow.output(output(name), binding);
            }
            workflow
        })
}

// ---------------------------------------------------------------------
// Arbitrary values of every registered domain type
// ---------------------------------------------------------------------

/// Every non-secret registered type's own scalar [`TypeRef`] paired with
/// its [`willikins_types::DomainType::example`] -- a worked example the
/// registry guarantees parses as its own type
/// (`willikins_types::assert_all_examples_parse`), so it can seed a value
/// without this module needing a text generator of its own for every
/// domain grammar.
fn non_secret_type_examples() -> Vec<(TypeRef, &'static str)> {
    willikins_types::type_infos()
        .into_iter()
        .filter(|info| !info.secret)
        .map(|info| {
            let name = crate::TypeName::parse(info.name)
                .unwrap_or_else(|err| unreachable!("a registered type name is valid: {err}"));
            (TypeRef::scalar(name), info.example)
        })
        .collect()
}

/// A known [`Value`] of a randomly chosen non-secret registered type,
/// scalar or list-valued (0 to 4 elements, every element built from that
/// type's own worked example).
///
/// Exists for `willikins-server`'s own restart-survival property test
/// (task 10b): `Butler::apply` rebuilds a plan's resolved inputs from the
/// journal by parsing each recorded input's *rendered* string back
/// through the type registry, which only works if
/// `Value::parse`/`Value::parse_list` round-trips `Value::render`'s own
/// output for every registered type -- what this generator lets that
/// property test check, rather than the specific input text (which the
/// registry already guarantees parses at least once, via each type's own
/// example).
pub fn arb_registered_value() -> impl Strategy<Value = Value> {
    let types = non_secret_type_examples();
    (prop::sample::select(types), any::<bool>(), 0usize..5).prop_map(
        |((ty, example), is_list, count)| {
            if is_list {
                let list_ty = TypeRef::list_of(ty.name.clone());
                let items = vec![example; count];
                Value::parse_list(&list_ty, &items).unwrap_or_else(|err| {
                    unreachable!("{list_ty}'s own worked example must parse: {err}")
                })
            } else {
                Value::parse(&ty, example).unwrap_or_else(|err| {
                    unreachable!("{ty}'s own worked example must parse: {err}")
                })
            }
        },
    )
}

// ---------------------------------------------------------------------
// Reversal-declaring stub tools (milestone 3m task R3)
// ---------------------------------------------------------------------
//
// [`StubDeletableTool`] and [`StubContainedTool`]: a pair a document can
// bind to exercise teardown policy against a catalog that actually
// admits `teardown:` (milestone 3m task T1's own stubs, and any later
// task's). Behind `test-support`, the same reason `arb_registered_value`
// is: a dependent crate's own tests need these as a compiled dependency,
// not as this crate's `#[cfg(test)]` code.

/// A stub [`Inverse`] that reports a fixed [`Standing`] and, on delete,
/// `Deleted { already_gone }` matching it — never a real provider call.
/// Shared by [`StubDeletableTool`]. Counts its own `standing` calls
/// (milestone 3m task R5a, acceptance 4 and 6: a kept or refused-earlier
/// candidate must call `standing` zero times).
struct StubInverse {
    standing: Standing,
    standing_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Inverse for StubInverse {
    fn standing(&self, _created: &Created<'_>) -> Result<Standing, ToolError> {
        self.standing_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.standing.clone())
    }

    fn delete(&self, _created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        Ok(Deleted {
            already_gone: matches!(self.standing, Standing::Gone),
        })
    }
}

/// A stub, non-pure tool declaring [`Reversal::Delete`] with one declared
/// loss (`"builds"`): one input port `repo` (`GitHubRepo`, required, also
/// its `key`), one output port of the same name and type.
///
/// `inverse()` returns `Some`, so [`crate::catalog::Catalog::insert`]
/// accepts the declaration (task R3's own "`Delete` exactly when
/// `inverse()` is `Some`" rule).
pub struct StubDeletableTool {
    spec: ToolSpec,
    inverse: StubInverse,
}

impl StubDeletableTool {
    /// A tool whose inverse reports [`Standing::Gone`] — a safe default
    /// for a test that never calls it, and for one that reaches
    /// `plan_teardown` without seeding anything for it to find.
    #[must_use]
    pub fn new() -> Self {
        Self::with_standing(Standing::Gone)
    }

    /// The same tool, with its inverse's `standing` fixed to `standing`.
    #[must_use]
    pub fn with_standing(standing: Standing) -> Self {
        Self::with_standing_and_counter(
            standing,
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        )
    }

    /// The same tool, sharing `standing_calls` with the caller so it can
    /// read how many times `standing` was called after the fact
    /// (milestone 3m task R5a, acceptance 4 and 6).
    #[must_use]
    pub fn with_standing_and_counter(
        standing: Standing,
        standing_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("repo"),
            PortSpec {
                ty: PortType::Exact(ty("GitHubRepo")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), ty("GitHubRepo"));
        Self {
            spec: ToolSpec {
                name: tool_name("test.deletable"),
                description: "A stub tool declaring Reversal::Delete with a loss.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: StubInverse {
                standing,
                standing_calls,
            },
        }
    }

    /// How many times this tool's inverse's `standing` has been called so
    /// far (milestone 3m task R5a, acceptance 4 and 6).
    #[must_use]
    pub fn standing_calls(&self) -> usize {
        self.inverse
            .standing_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for StubDeletableTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for StubDeletableTool {
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
        Reversal::Delete {
            needs: &["repo"],
            within: &[],
            losses: &["builds"],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// A stub, non-pure tool declaring [`Reversal::Contained`], riding out
/// with [`StubDeletableTool`] (its `repo` input mapped to that tool's
/// `repo` input): the same port shape as `StubDeletableTool`, and no
/// inverse of its own.
pub struct StubContainedTool {
    spec: ToolSpec,
}

impl StubContainedTool {
    /// A tool declaring `Contained`, riding out with
    /// [`StubDeletableTool`]'s `test.deletable`.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("repo"),
            PortSpec {
                ty: PortType::Exact(ty("GitHubRepo")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), ty("GitHubRepo"));
        Self {
            spec: ToolSpec {
                name: tool_name("test.contained"),
                description: "A stub tool declaring Reversal::Contained.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Default for StubContainedTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for StubContainedTool {
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
        Reversal::Contained {
            within: &[Within {
                tool: "test.deletable",
                ports: &[("repo", Side::Input, "repo")],
            }],
            otherwise: "left in place, outside its container",
        }
    }
}

#[cfg(test)]
mod reversal_stub_tests {
    use super::*;
    use crate::catalog::Catalog;
    use std::sync::Arc;

    #[test]
    fn stub_deletable_tool_is_accepted_by_catalog_insert() {
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::new(StubDeletableTool::new()))
            .unwrap_or_else(|err| unreachable!("StubDeletableTool must validate: {err}"));
    }

    #[test]
    fn stub_contained_tool_is_accepted_by_catalog_insert() {
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::new(StubContainedTool::new()))
            .unwrap_or_else(|err| unreachable!("StubContainedTool must validate: {err}"));
    }

    #[test]
    fn stub_deletable_tool_inverse_reports_the_fixed_standing() {
        let tool = StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec!["builds"],
        });
        let inverse = Tool::inverse(&tool).unwrap_or_else(|| unreachable!("Delete has Some"));
        let inputs = Inputs::new();
        let outputs = Outputs::new();
        let now = crate::apply::Timestamp::now();
        let created = Created {
            inputs: &inputs,
            outputs: &outputs,
            window: crate::rollback::CreationWindow {
                start: now,
                end: now,
            },
            settled_by: now,
            accept: &[],
            riders: &[],
        };
        assert_eq!(
            inverse.standing(&created).unwrap(),
            Standing::AsCreated {
                losing: vec!["builds"]
            }
        );
    }
}
