//! Acceptance test: `underived_config_binding_is_refused`. Pins
//! `doppler.secret.set`'s provenance rule (that tool's own module docs
//! carry the full argument): the `config` port may only be bound to the
//! output of an earlier, non-pure node. `check` needs only tool specs,
//! not live network access, so the all-fake catalog (whose
//! `doppler.secret.set`/`doppler.secret.get` specs are pinned equal to
//! the live ones by `tests/catalog_parity.rs`) is enough to check
//! against.

fn workflows_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("workflows")
}

fn fixture(name: &str) -> std::path::PathBuf {
    workflows_dir().join("fixtures").join(name)
}

fn load(path: &std::path::Path) -> willikins_core::Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{} loads: {err}", path.display()))
}

#[test]
fn underived_config_binding_is_refused() {
    let (_state, catalog) = willikins_providers_fake::empty();

    for name in [
        "secret-set-literal-config.yaml",
        "secret-set-input-config.yaml",
    ] {
        let workflow = load(&fixture(name));
        let errors = willikins_core::check(&workflow, &catalog)
            .expect_err(&format!("{name}: check must fail"));
        assert_eq!(
            errors,
            vec![willikins_core::CheckError::UnderivedBinding {
                node: willikins_core::NodeName::parse("sink").unwrap(),
                port: willikins_core::PortName::parse("config").unwrap(),
            }],
            "{name}: expected exactly one UnderivedBinding"
        );
    }
}

/// The positive case: `config` bound to `doppler.config.ensure`'s own
/// output (a real, non-pure node) is accepted -- the rule refuses a
/// literal, a workflow input, and a pure node's output, never a genuine
/// upstream node's.
#[test]
fn a_config_derived_from_an_earlier_non_pure_node_is_accepted() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let workflow = load(&fixture("secret-set-derived-config.yaml"));
    willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("expected check to succeed, got {errors:?}"));
}
