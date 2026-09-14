#![allow(dead_code)]
//! Small shared constructors for `willikins-journal`'s own integration
//! tests, mirroring `crates/willikins-core/tests/common/mod.rs`'s style.

use indexmap::IndexMap;

use willikins_core::{InputName, NodeName, OutputName, PortName, ToolName, Value};
use willikins_journal::{DocumentSha256, PlanId, PrincipalId, Reason};
use willikins_types::{DomainType, WorkflowName};

pub fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

pub fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

pub fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

pub fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

pub fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap()
}

pub fn workflow_name(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

pub fn principal(name: &str) -> PrincipalId {
    PrincipalId::parse(name).unwrap()
}

pub fn reason(text: &str) -> Reason {
    Reason::parse(text).unwrap()
}

/// A distinct, validly-shaped [`DocumentSha256`] derived from `label` --
/// tests that only need two document hashes to *differ* (never a real
/// file's actual digest) use this instead of an arbitrary string, which
/// `DocumentSha256`'s 64-lower-case-hex grammar now refuses.
pub fn document_sha256(label: &str) -> DocumentSha256 {
    DocumentSha256::compute(label.as_bytes())
}

pub fn empty_inputs() -> IndexMap<InputName, Value> {
    IndexMap::new()
}

pub fn empty_outputs() -> IndexMap<OutputName, Value> {
    IndexMap::new()
}

pub fn empty_plan(workflow: WorkflowName) -> willikins_core::Plan {
    willikins_core::Plan {
        workflow,
        nodes: Vec::new(),
        outputs: IndexMap::new(),
        class: willikins_core::Class::Reversible,
        requires_approval: false,
    }
}

pub fn dummy_plan_id() -> PlanId {
    PlanId::new()
}
