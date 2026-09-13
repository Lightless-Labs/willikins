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

use proptest::prelude::*;

use crate::value::{TypeRef, Value};
use crate::workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};
use crate::{PortName, ToolName};
use willikins_types::{DomainType, WorkflowName};

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
