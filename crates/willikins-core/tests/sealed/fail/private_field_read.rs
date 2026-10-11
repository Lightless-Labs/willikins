// Milestone 3n, decision (s2): outside `willikins_core::check`, reading
// `Checked`'s (private) `order` field must not compile. (G7's addendum:
// this fixture read `types` until G7 deleted that stored field --
// `order` has a public accessor of the same name, same as `types` did,
// so the error's own shape -- "a method also exists" -- is unchanged.)

use willikins_types::DomainType;

fn main() {
    let catalog = willikins_core::Catalog::new(willikins_types::registry());
    let workflow =
        willikins_core::Workflow::new(willikins_types::WorkflowName::parse("w").unwrap());
    let checked = willikins_core::check(&workflow, &catalog).unwrap();
    let _ = checked.order;
}
