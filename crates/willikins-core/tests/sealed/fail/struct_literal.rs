// Milestone 3n, decision (s2): outside `willikins_core::check`, a
// `Checked { .. }` struct-update literal must not compile once its six
// fields are private to that module.

use willikins_types::DomainType;

fn main() {
    let catalog = willikins_core::Catalog::new(willikins_types::registry());
    let workflow =
        willikins_core::Workflow::new(willikins_types::WorkflowName::parse("w").unwrap());
    let checked = willikins_core::check(&workflow, &catalog).unwrap();
    let _forged = willikins_core::Checked {
        order: Vec::new(),
        ..checked
    };
}
