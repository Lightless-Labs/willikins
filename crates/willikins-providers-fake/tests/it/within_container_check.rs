//! Milestone 3m task P6: the container side of every fake tool's own
//! [`Reversal::Delete`]/[`Reversal::Contained`] declaration, over this
//! crate's own fake catalog -- the half `willikins_core::Catalog::insert`
//! cannot check on its own (it sees one tool at a time): that the named
//! container tool exists in this catalog, that it carries the mapped
//! port on the stated [`Side`], and that the two ports' types agree.
//! Decision 3's own text: "the container side... is checked... by P6
//! over the fake catalog." (`willikins-server`'s own table test over
//! `LIVE_TOOL_NAMES`, task P7, is the live-catalog half of the same
//! check; it has not landed yet, so this is a fresh implementation, not
//! a reuse of its code.)

use willikins_core::rollback::{Reversal, Side};
use willikins_core::{PortName, PortType, ToolName, TypeRef};

/// Every `(own_type, side, container_tool, container_port)` the catalog's
/// own tools declare, collected once.
fn every_within_entry(catalog: &willikins_core::Catalog) -> Vec<(TypeRef, Side, String, String)> {
    let mut entries = Vec::new();
    let names: Vec<ToolName> = catalog.specs().map(|spec| spec.name.clone()).collect();
    for name in names {
        let tool = catalog
            .get(&name)
            .unwrap_or_else(|| unreachable!("`{name}` was just listed by `specs()`"));
        let spec = tool.spec();
        let within_list: &[willikins_core::rollback::Within] = match &tool.reversal() {
            Reversal::Delete { within, .. } | Reversal::Contained { within, .. } => within,
            Reversal::Retained { .. } => &[],
        };
        for within in within_list {
            for &(own_port, side, container_port) in within.ports {
                let own_port_name = PortName::parse(own_port).unwrap_or_else(|err| {
                    unreachable!("`{name}`'s own `within` port `{own_port}` must already be a valid PortName (Catalog::insert validated it): {err}")
                });
                let port_spec = spec.inputs.get(&own_port_name).unwrap_or_else(|| {
                    unreachable!(
                        "`{name}`'s own `within` port `{own_port}` must already be one of its own inputs (Catalog::insert validated it)"
                    )
                });
                let own_type = match &port_spec.ty {
                    PortType::Exact(ty) => ty.clone(),
                    PortType::AnySecret => unreachable!(
                        "`{name}`'s own `within` port `{own_port}` must already be non-secret (Catalog::insert validated it)"
                    ),
                };
                entries.push((
                    own_type,
                    side,
                    within.tool.to_string(),
                    container_port.to_string(),
                ));
            }
        }
    }
    entries
}

#[test]
fn every_within_container_exists_in_the_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    for (_own_type, _side, container_tool, _container_port) in every_within_entry(&catalog) {
        let name = ToolName::parse(&container_tool).unwrap_or_else(|err| {
            unreachable!("`{container_tool}` must already be a valid ToolName: {err}")
        });
        assert!(
            catalog.get(&name).is_some(),
            "`{container_tool}` names a container tool not registered in the fake catalog"
        );
    }
}

#[test]
fn every_within_container_port_exists_on_its_stated_side_with_the_same_type() {
    let (_state, catalog) = willikins_providers_fake::empty();
    for (own_type, side, container_tool, container_port) in every_within_entry(&catalog) {
        let name = ToolName::parse(&container_tool).unwrap_or_else(|err| {
            unreachable!("`{container_tool}` must already be a valid ToolName: {err}")
        });
        let container = catalog.get(&name).unwrap_or_else(|| {
            panic!("`{container_tool}` names a container tool not registered in the fake catalog")
        });
        let container_spec = container.spec();
        let container_port_name = PortName::parse(&container_port).unwrap_or_else(|err| {
            panic!("`{container_tool}`'s mapped port `{container_port}` must be a valid PortName: {err}")
        });
        let container_type = match side {
            Side::Input => {
                let port_spec = container_spec.inputs.get(&container_port_name).unwrap_or_else(|| {
                    panic!(
                        "`{container_tool}` has no own input port `{container_port}`, but a `Within` maps to it on `Side::Input`"
                    )
                });
                match &port_spec.ty {
                    PortType::Exact(ty) => ty.clone(),
                    PortType::AnySecret => panic!(
                        "`{container_tool}`'s input `{container_port}` is secret, but a `Within` maps to it"
                    ),
                }
            }
            Side::Output => container_spec
                .outputs
                .get(&container_port_name)
                .unwrap_or_else(|| {
                    panic!(
                        "`{container_tool}` has no own output port `{container_port}`, but a `Within` maps to it on `Side::Output`"
                    )
                })
                .clone(),
        };
        assert_eq!(
            own_type, container_type,
            "`{container_tool}`'s mapped port `{container_port}` has a different type than the \
             rider's own `within` port"
        );
    }
}

#[test]
fn at_least_one_within_entry_exists_to_prove_this_walk_is_not_vacuous() {
    let (_state, catalog) = willikins_providers_fake::empty();
    assert!(
        !every_within_entry(&catalog).is_empty(),
        "no fake tool declared a `Within` at all; this walk would otherwise pass vacuously"
    );
}
