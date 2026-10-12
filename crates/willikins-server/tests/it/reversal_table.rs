//! Milestone 3m task P7 (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`,
//! acceptance 13; decision 3's `Within` type check): the live-catalog half
//! of what `willikins_core::Catalog::insert` cannot check on its own (it
//! sees one tool at a time). `willikins-providers-fake`'s own
//! `tests/it/within_container_check.rs` (task P6) is the fake-catalog
//! half of the same check, over a catalog this crate cannot import a test
//! from (it lives in another crate's own `tests/it` binary) -- the two
//! copies share logic, not code.
//!
//! No tool here is ever called: `reversal()` is a pure method and every
//! `Http` below points at a port nothing listens on
//! ([`live_catalog_with`](willikins_server::live_catalog_with)'s own
//! `tests/it/live_catalog.rs` precedent, in
//! `willikins-providers-doppler`, built the live catalog the same way).

use willikins_core::{Catalog, PortName, PortType, Reversal, Side, ToolName, TypeRef, Within};
use willikins_providers_http::{Credential, Http};
use willikins_server::LIVE_TOOL_NAMES;

/// A port nothing listens on: `reversal()` makes no network call either
/// way, but every other test in this crate that builds the live catalog
/// points its `Http`s here, and this file follows the same convention.
const NOWHERE: &str = "http://127.0.0.1:1";

/// The live catalog, exactly as
/// `crates/willikins-providers-doppler/tests/it/live_catalog.rs` builds
/// it: every `Credential` a short `for_testing` literal, every `Http`
/// pointed at [`NOWHERE`].
fn live_catalog() -> Catalog {
    let github_credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let doppler_credential =
        Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let buildkite_credential =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken12345678");
    let signoz_credential =
        Credential::for_testing("WILLIKINS_TEST_SIGNOZ_API_KEY", "testsignozapikey00000000");
    willikins_server::live_catalog_with(
        Http::new(
            NOWHERE,
            willikins_providers_github::default_headers(),
            github_credential,
        ),
        Http::new(NOWHERE, Vec::new(), doppler_credential),
        Http::new(NOWHERE, Vec::new(), buildkite_credential),
        Http::new(NOWHERE, Vec::new(), signoz_credential),
    )
}

// ---------------------------------------------------------------------
// Acceptance 13: one snapshot of every live tool's reversal, losses
// included. Adding a tool without a declaration changes the snapshot
// (it still validates, under the default `Retained { why: UNDECLARED }`,
// so only a read-and-accept insta diff catches it -- never a failure).
// ---------------------------------------------------------------------

#[test]
fn the_declaration_table_is_pinned() {
    let catalog = live_catalog();
    assert_eq!(
        catalog.specs().count(),
        LIVE_TOOL_NAMES.len(),
        "the live catalog's own tool count drifted from `LIVE_TOOL_NAMES`"
    );

    let table: Vec<(&str, Reversal)> = LIVE_TOOL_NAMES
        .iter()
        .map(|&name| {
            let tool_name = ToolName::parse(name)
                .unwrap_or_else(|err| panic!("`{name}` is a tool name: {err}"));
            let tool = catalog
                .get(&tool_name)
                .unwrap_or_else(|| panic!("the live catalog holds `{name}`"));
            (name, tool.reversal())
        })
        .collect();

    insta::assert_debug_snapshot!(table);
}

// ---------------------------------------------------------------------
// Decision 3's own `Within` type check, over the live catalog: the
// container tool exists, the mapped port exists on the stated `Side`,
// and the two ports' types agree. `Catalog::insert` validates a `Within`
// on the declaring tool's own side only (it sees one tool at a time);
// this is the other half.
// ---------------------------------------------------------------------

/// Every `(own_type, side, container_tool, container_port)` the live
/// catalog's own tools declare, collected once.
fn every_within_entry(catalog: &Catalog) -> Vec<(TypeRef, Side, String, String)> {
    let mut entries = Vec::new();
    let names: Vec<ToolName> = catalog.specs().map(|spec| spec.name.clone()).collect();
    for name in names {
        let tool = catalog
            .get(&name)
            .unwrap_or_else(|| unreachable!("`{name}` was just listed by `specs()`"));
        let spec = tool.spec();
        let within_list: &[Within] = match &tool.reversal() {
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
fn every_within_container_exists_in_the_live_catalog() {
    let catalog = live_catalog();
    for (_own_type, _side, container_tool, _container_port) in every_within_entry(&catalog) {
        let name = ToolName::parse(&container_tool).unwrap_or_else(|err| {
            unreachable!("`{container_tool}` must already be a valid ToolName: {err}")
        });
        assert!(
            catalog.get(&name).is_some(),
            "`{container_tool}` names a container tool not registered in the live catalog"
        );
    }
}

#[test]
fn every_within_container_port_exists_on_its_stated_side_with_the_same_type() {
    let catalog = live_catalog();
    for (own_type, side, container_tool, container_port) in every_within_entry(&catalog) {
        let name = ToolName::parse(&container_tool).unwrap_or_else(|err| {
            unreachable!("`{container_tool}` must already be a valid ToolName: {err}")
        });
        let container = catalog.get(&name).unwrap_or_else(|| {
            panic!("`{container_tool}` names a container tool not registered in the live catalog")
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
    let catalog = live_catalog();
    assert!(
        !every_within_entry(&catalog).is_empty(),
        "no live tool declared a `Within` at all; this walk would otherwise pass vacuously"
    );
}
