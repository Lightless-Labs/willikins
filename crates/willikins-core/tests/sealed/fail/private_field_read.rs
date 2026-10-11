// Milestone 3n, decision (s2): outside `willikins_core::check`, reading
// `Checked`'s (private) `types` field must not compile.

use willikins_types::DomainType;

fn main() {
    let catalog = willikins_core::Catalog::new(willikins_types::registry());
    let workflow =
        willikins_core::Workflow::new(willikins_types::WorkflowName::parse("w").unwrap());
    let checked = willikins_core::check(&workflow, &catalog).unwrap();
    let _ = checked.types;
}
