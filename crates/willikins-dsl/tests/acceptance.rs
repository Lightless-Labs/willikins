//! Loads the milestone 1 reference documents and checks them against
//! `willikins_providers_fake`'s catalog, per the plan's acceptance tests
//! 1 and 6.

use std::path::Path;

use willikins_core::{Class, NodeName};
use willikins_dsl::load_document;

fn fixture_path(relative: &str) -> String {
    format!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../{}"), relative)
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

#[test]
fn new_rust_service_checks_against_the_fake_catalog() {
    let path = fixture_path("workflows/new-rust-service.yaml");
    let workflow = load_document(Path::new(&path)).expect("the positive fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = willikins_core::check(&workflow, &catalog).unwrap_or_else(|errors| {
        panic!("expected the positive fixture to check cleanly: {errors:?}")
    });

    assert_eq!(
        checked.order,
        vec![
            node("names"),
            node("repo"),
            node("doppler"),
            node("configs"),
            node("token"),
            node("ci_secret"),
        ]
    );
    assert_eq!(checked.class, Class::Reversible);
    assert!(checked.warnings.is_empty());
}

#[test]
fn secret_into_template_fails_check_with_exactly_one_taint_error() {
    let path = fixture_path("workflows/fixtures/secret-into-template.yaml");
    let workflow = load_document(Path::new(&path)).expect("the negative fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("the tainted fixture must fail check");

    assert_eq!(errors.len(), 1, "{errors:?}");
    match &errors[0] {
        willikins_core::CheckError::SecretToNonSecretSink { from, to } => {
            assert_eq!(
                from,
                &(
                    node("token"),
                    willikins_core::PortName::parse("token").unwrap()
                )
            );
            assert_eq!(
                to,
                &(
                    node("readme"),
                    willikins_core::PortName::parse("value").unwrap()
                )
            );
        }
        other => panic!("expected SecretToNonSecretSink, got {other:?}"),
    }
}
