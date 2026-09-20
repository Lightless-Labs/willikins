//! Every pure tool in the catalog answers `ensure` exactly the way it
//! answers `read`, and never reports `changed`.
//!
//! A pure tool's `ensure` is its `read` by contract (`Tool::ensure`'s own
//! doc), and the executor of an applied plan leans on that: a `Compute`
//! node must produce the same outputs at apply time as the plan showed.
//! The five pure tools — `naming.v1` and `template.render` from
//! `willikins-tools`, `doppler.secret.get`, `fake.secret_list`, and
//! (milestone 3a) `buildkite.cluster.get` from this crate — are checked
//! here in one place, through the catalog, so a sixth pure tool
//! registered later is a one-line addition rather than a test nobody
//! writes.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, Outputs, PortName, SinkToken, ToolName, Value};
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{
    BuildkiteClusterName, BuildkiteOrg, DomainType, DopplerConfig, DopplerSecretValue, GitHubOrg,
    ProjectSlug, SecretName, TemplateSource, Text,
};

/// A test mints its own token; `SinkToken::new` is disallowed elsewhere.
#[allow(clippy::disallowed_methods)]
fn mint() -> SinkToken {
    SinkToken::new()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a test port name is valid")
}

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").expect("a valid config")
}

fn secret_name() -> SecretName {
    SecretName::parse("DATABASE_URL").expect("a valid secret name")
}

fn buildkite_org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").expect("a valid Buildkite org")
}

fn cluster_name() -> BuildkiteClusterName {
    BuildkiteClusterName::parse("Default cluster").expect("a valid cluster name")
}

/// Ports compared pairwise, so a differing `Value` fails on its own port
/// rather than inside an opaque map comparison.
fn ports(outputs: &Outputs) -> Vec<(&PortName, &Value)> {
    outputs.iter().collect()
}

#[test]
fn every_pure_tool_answers_ensure_exactly_the_way_it_answers_read() {
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_secret(
                &config(),
                &secret_name(),
                DopplerSecretValue::parse("s3cr3t-bytes-nobody-should-see")
                    .expect("a valid secret"),
            )
            .with_buildkite_cluster(&cluster_name(), "018e5a22-d14c-7085-bb28-db0f83f43a1c"),
    ));
    let fake_catalog = catalog(state);

    let mut naming_inputs = Inputs::new();
    naming_inputs.insert(
        port("org"),
        Value::known(GitHubOrg::parse("lightless-labs").expect("a valid org")),
    );
    naming_inputs.insert(
        port("slug"),
        Value::known(ProjectSlug::parse("third-thoughts").expect("a valid slug")),
    );

    let mut template_inputs = Inputs::new();
    template_inputs.insert(
        port("template"),
        Value::known(TemplateSource::parse("Hello, {{ value }}!").expect("valid template source")),
    );
    template_inputs.insert(
        port("value"),
        Value::known(Text::parse("World").expect("valid text")),
    );

    let mut secret_get_inputs = Inputs::new();
    secret_get_inputs.insert(port("config"), Value::known(config()));
    secret_get_inputs.insert(port("name"), Value::known(secret_name()));

    let mut secret_list_inputs = Inputs::new();
    secret_list_inputs.insert(port("config"), Value::known(config()));

    let mut cluster_get_inputs = Inputs::new();
    cluster_get_inputs.insert(port("org"), Value::known(buildkite_org()));
    cluster_get_inputs.insert(port("name"), Value::known(cluster_name()));

    let cases = [
        ("naming.v1", naming_inputs),
        ("template.render", template_inputs),
        ("doppler.secret.get", secret_get_inputs),
        ("fake.secret_list", secret_list_inputs),
        ("buildkite.cluster.get", cluster_get_inputs),
    ];

    let mut checked = 0;
    for (name, inputs) in cases {
        let tool = fake_catalog
            .get(&ToolName::parse(name).expect("a valid tool name"))
            .unwrap_or_else(|| panic!("`{name}` is registered"));
        assert!(tool.spec().pure, "`{name}` is expected to be pure");

        let observation = tool
            .read(&inputs)
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        let Observation::Present(via_read) = observation else {
            panic!("{name}: a pure tool always reports Present, got {observation:?}");
        };
        let ensured = tool
            .ensure(&inputs, &mint())
            .unwrap_or_else(|err| panic!("{name}: {err}"));

        assert!(
            !ensured.changed,
            "{name}: a pure tool never changes anything"
        );
        assert_eq!(
            ports(&ensured.outputs),
            ports(&via_read),
            "{name}: ensure and read disagree"
        );
        checked += 1;
    }

    let pure_in_catalog = fake_catalog.specs().filter(|spec| spec.pure).count();
    assert_eq!(
        checked, pure_in_catalog,
        "a pure tool was added to the catalog without a case here"
    );
}
