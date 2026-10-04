//! Every pure tool in the catalog answers `ensure` exactly the way it
//! answers `read`, and never reports `changed`.
//!
//! A pure tool's `ensure` is its `read` by contract (`Tool::ensure`'s own
//! doc), and the executor of an applied plan leans on that: a `Compute`
//! node must produce the same outputs at apply time as the plan showed.
//! `naming.v1`, `template.render`, `env.get`, `base64.decode`,
//! `apple.signing_key.parse`, `apple.issuer_id.parse`,
//! `apple.key_id.parse`, (task R2) `github.token.parse`, (task K1)
//! `buildkite.token.parse`, and (milestone 3g task T1) `repo.file.render`
//! from
//! `willikins-tools`, `doppler.secret.get`,
//! `doppler.value.get`, `fake.secret_list`, (milestone 3a)
//! `buildkite.cluster.get`, (milestone 3c) `appstore.certificate.get`,
//! (milestone 3e task 2) `github.repo.get`, (G3) `operator.acknowledge`,
//! (task R3) `doppler.config.inheritable.gate`, (milestone 3g task
//! B1) `buildkite.pipeline.bootstrap.gate`, and (milestone 3j task B3)
//! `doppler.secret_name.gate`
//! from this crate — are checked here in one
//! place, through the catalog, so a new pure tool registered later is a
//! one-line addition rather than a test nobody writes.
//!
//! **`env.get` is the one exception, and it is excluded by name, not
//! silently uncounted.** Proving it agrees on a `Present` case would mean
//! mutating the real process environment, and `std::env::set_var` is
//! `unsafe` under edition 2024, which this workspace's
//! `unsafe_code = "forbid"` lint blocks outright -- the same wall
//! `willikins-tools/src/env_get.rs`'s own module doc documents. That
//! crate's own `ensure_agrees_with_read` test proves the identical
//! property over the `NotFound` path instead (no mutation needed, since
//! `WILLIKINS_TEST_ENV_GET_ABSENT` is simply never set); this file's
//! catalog-driven sweep cannot reach the `Present` path without breaking
//! that rule, so it does not try.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, Outputs, PortName, SinkToken, ToolName, Value};
use willikins_providers_fake::state::BuildkitePipelineRecord;
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCapabilityType,
    AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId,
    AppleObservableCapabilityType, AppleSigningKey, BuildkiteClusterName, BuildkiteOrg,
    BuildkitePipelineSlug, BuildkiteToken, DomainType, DopplerConfig, DopplerSecretValue,
    DopplerValue, GitHubOrg, GitHubRepo, GitHubToken, OpaqueSecret, OperatorAcknowledgement,
    ProjectSlug, RepoFile, RepoPath, RepoVisibility, SecretName, TemplateSource, TemplateValue,
    Text,
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

fn value_name() -> SecretName {
    SecretName::parse("ASC_API_KEY_ISSUER_ID").expect("a valid secret name")
}

fn buildkite_org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").expect("a valid Buildkite org")
}

fn bootstrap_slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").expect("a valid Buildkite pipeline slug")
}

/// The bootstrap `buildkite.pipeline.bootstrap.gate`'s own case below
/// seeds the fake pipeline with, and binds as `expected` -- re-quoted
/// between the two so the case also proves the comparison is structural,
/// not byte equality.
const BOOTSTRAP_SEEDED: &str = "steps:\n  - command: \"echo hi\"\n";
const BOOTSTRAP_EXPECTED: &str = "steps:\n  - command: 'echo hi'\n";

fn certificate_type() -> AppleCertificateType {
    AppleCertificateType::parse("DISTRIBUTION").expect("a valid certificate type")
}

fn serial_number() -> AppleCertificateSerial {
    AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E").expect("a valid serial")
}

fn cluster_name() -> BuildkiteClusterName {
    BuildkiteClusterName::parse("Default cluster").expect("a valid cluster name")
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("example-org/monorepo").expect("a valid repo")
}

fn app_identifier() -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse("com.example.MyApp").expect("a valid bundle identifier")
}

/// Ports compared pairwise, so a differing `Value` fails on its own port
/// rather than inside an opaque map comparison.
fn ports(outputs: &Outputs) -> Vec<(&PortName, &Value)> {
    outputs.iter().collect()
}

#[test]
// One `inputs` value per pure tool in the catalog, built up in a flat
// sequence rather than split across helpers so each case's inputs stay
// next to the tool name that uses them -- length is the honest cost of
// that, not a sign the test does too much.
#[allow(clippy::too_many_lines)]
fn every_pure_tool_answers_ensure_exactly_the_way_it_answers_read() {
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_secret(
                &config(),
                &secret_name(),
                DopplerSecretValue::parse("s3cr3t-bytes-nobody-should-see")
                    .expect("a valid secret"),
            )
            .with_doppler_value(
                &config(),
                &value_name(),
                DopplerValue::parse("57246542-96fe-1a63-e053-0824d011072a")
                    .expect("a valid Doppler value"),
            )
            .with_doppler_config(&config())
            .with_doppler_config_inheritable(&config())
            .with_buildkite_cluster(&cluster_name(), "018e5a22-d14c-7085-bb28-db0f83f43a1c")
            .with_apple_certificate(
                &certificate_type(),
                &serial_number(),
                "C3RT1F1CATE1",
                false,
                None,
            )
            .with_repo(&repo(), RepoVisibility::Private, false)
            .with_apple_app(&app_identifier())
            .with_apple_bundle_id(
                &app_identifier(),
                &AppleBundleIdName::parse("my-app").expect("a valid bundle id name"),
                &AppleBundleIdPlatform::parse("UNIVERSAL").expect("a valid platform"),
            )
            .with_apple_bundle_id_capability(
                &app_identifier(),
                &AppleCapabilityType::parse("APP_GROUPS").expect("a valid capability"),
            )
            .with_buildkite_pipeline(
                &buildkite_org(),
                &bootstrap_slug(),
                BuildkitePipelineRecord {
                    repository: "git@github.com:lightless-labs/third-thoughts.git".to_string(),
                    cluster_id: "018e5a22-d14c-7085-bb28-db0f83f43a1c".to_string(),
                    ours: true,
                    configuration: BOOTSTRAP_SEEDED.to_string(),
                },
            ),
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

    let mut value_get_inputs = Inputs::new();
    value_get_inputs.insert(port("config"), Value::known(config()));
    value_get_inputs.insert(port("name"), Value::known(value_name()));

    let mut base64_decode_inputs = Inputs::new();
    base64_decode_inputs.insert(
        port("value"),
        // base64 of "hello world".
        Value::known(OpaqueSecret::parse("aGVsbG8gd29ybGQ=").expect("valid opaque secret")),
    );

    let mut apple_key_parse_inputs = Inputs::new();
    apple_key_parse_inputs.insert(
        port("value"),
        Value::known(OpaqueSecret::parse(AppleSigningKey::example()).expect("valid opaque secret")),
    );

    let mut apple_issuer_id_parse_inputs = Inputs::new();
    apple_issuer_id_parse_inputs.insert(
        port("value"),
        Value::known(
            DopplerValue::parse("57246542-96fe-1a63-e053-0824d011072a")
                .expect("valid Doppler value"),
        ),
    );

    let mut apple_key_id_parse_inputs = Inputs::new();
    apple_key_id_parse_inputs.insert(
        port("value"),
        Value::known(DopplerValue::parse("2X9R4HXF34").expect("valid Doppler value")),
    );

    let mut github_token_parse_inputs = Inputs::new();
    github_token_parse_inputs.insert(
        port("value"),
        Value::known(OpaqueSecret::parse(GitHubToken::example()).expect("valid opaque secret")),
    );

    let mut buildkite_token_parse_inputs = Inputs::new();
    buildkite_token_parse_inputs.insert(
        port("value"),
        Value::known(OpaqueSecret::parse(BuildkiteToken::example()).expect("valid opaque secret")),
    );

    let mut certificate_get_inputs = Inputs::new();
    certificate_get_inputs.insert(
        port("issuer_id"),
        Value::known(
            AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a")
                .expect("a valid issuer id"),
        ),
    );
    certificate_get_inputs.insert(
        port("key_id"),
        Value::known(AppleKeyId::parse("2X9R4HXF34").expect("a valid key id")),
    );
    certificate_get_inputs.insert(
        port("key"),
        Value::known(AppleSigningKey::parse(AppleSigningKey::example()).expect("a valid key")),
    );
    certificate_get_inputs.insert(port("certificate_type"), Value::known(certificate_type()));
    certificate_get_inputs.insert(port("serial_number"), Value::known(serial_number()));

    let mut repo_get_inputs = Inputs::new();
    repo_get_inputs.insert(port("repo"), Value::known(repo()));

    let mut acknowledge_inputs = Inputs::new();
    acknowledge_inputs.insert(
        port("step"),
        Value::known(Text::parse("do the thing").expect("valid text value")),
    );
    acknowledge_inputs.insert(
        port("acknowledged"),
        Value::known(
            OperatorAcknowledgement::parse("done").expect("valid operator acknowledgement"),
        ),
    );

    let mut app_get_inputs = Inputs::new();
    app_get_inputs.insert(
        port("issuer_id"),
        Value::known(
            AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a")
                .expect("a valid issuer id"),
        ),
    );
    app_get_inputs.insert(
        port("key_id"),
        Value::known(AppleKeyId::parse("2X9R4HXF34").expect("a valid key id")),
    );
    app_get_inputs.insert(
        port("key"),
        Value::known(AppleSigningKey::parse(AppleSigningKey::example()).expect("a valid key")),
    );
    app_get_inputs.insert(port("identifier"), Value::known(app_identifier()));

    let app_group_gate_inputs = app_get_inputs.clone();

    // `capability` is bound to `APP_GROUPS`, already seeded above --
    // this test only proves the pure-tool `ensure` == `read` contract,
    // not the gate's own generality over read-only capabilities (that is
    // `appstore_bundle_id_capability_gate.rs`'s own unit tests and
    // `fake_agrees_with_live.rs`'s job).
    let mut capability_gate_inputs = app_get_inputs.clone();
    capability_gate_inputs.insert(
        port("capability"),
        Value::known(
            AppleObservableCapabilityType::parse("APP_GROUPS").expect("a valid capability"),
        ),
    );

    let mut config_inheritable_gate_inputs = Inputs::new();
    config_inheritable_gate_inputs.insert(port("config"), Value::known(config()));

    // Reuses `config()`/`secret_name()`, already seeded into
    // `doppler_secrets` above, so this gate reads `Present` directly
    // (milestone 3j task B3).
    let mut secret_name_gate_inputs = Inputs::new();
    secret_name_gate_inputs.insert(port("config"), Value::known(config()));
    secret_name_gate_inputs.insert(port("name"), Value::known(secret_name()));

    let mut buildkite_pipeline_bootstrap_gate_inputs = Inputs::new();
    buildkite_pipeline_bootstrap_gate_inputs.insert(port("org"), Value::known(buildkite_org()));
    buildkite_pipeline_bootstrap_gate_inputs.insert(port("slug"), Value::known(bootstrap_slug()));
    buildkite_pipeline_bootstrap_gate_inputs.insert(
        port("expected"),
        Value::known(
            RepoFile::new(
                RepoPath::parse("apps/example/.buildkite/bootstrap.yml")
                    .expect("a valid repo path"),
                BOOTSTRAP_EXPECTED,
            )
            .expect("a valid repo file"),
        ),
    );

    let mut repo_file_render_inputs = Inputs::new();
    repo_file_render_inputs.insert(
        port("path"),
        Value::known(RepoPath::parse("apps/example/file.txt").expect("a valid repo path")),
    );
    repo_file_render_inputs.insert(
        port("template"),
        Value::known(TemplateSource::parse("Hello, {{ 0 }}!").expect("valid template source")),
    );
    repo_file_render_inputs.insert(
        port("values"),
        Value::known_list(vec![
            TemplateValue::parse("World").expect("a valid template value"),
        ]),
    );

    let cases = [
        ("naming.v1", naming_inputs),
        ("template.render", template_inputs),
        ("base64.decode", base64_decode_inputs),
        ("apple.signing_key.parse", apple_key_parse_inputs),
        ("apple.issuer_id.parse", apple_issuer_id_parse_inputs),
        ("apple.key_id.parse", apple_key_id_parse_inputs),
        ("github.token.parse", github_token_parse_inputs),
        ("buildkite.token.parse", buildkite_token_parse_inputs),
        ("doppler.secret.get", secret_get_inputs),
        ("doppler.value.get", value_get_inputs),
        ("fake.secret_list", secret_list_inputs),
        ("buildkite.cluster.get", cluster_get_inputs),
        ("appstore.certificate.get", certificate_get_inputs),
        ("github.repo.get", repo_get_inputs),
        ("operator.acknowledge", acknowledge_inputs),
        ("appstore.app.get", app_get_inputs),
        ("appstore.app_group.gate", app_group_gate_inputs),
        ("appstore.bundle_id_capability.gate", capability_gate_inputs),
        (
            "doppler.config.inheritable.gate",
            config_inheritable_gate_inputs,
        ),
        ("doppler.secret_name.gate", secret_name_gate_inputs),
        ("repo.file.render", repo_file_render_inputs),
        (
            "buildkite.pipeline.bootstrap.gate",
            buildkite_pipeline_bootstrap_gate_inputs,
        ),
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

    // `env.get` is pure but deliberately has no case above -- see this
    // file's own module doc. Named explicitly here, not folded into a
    // fudge factor, so a *second* untestable-this-way tool still trips
    // the assertion below rather than silently widening the gap.
    let pure_in_catalog = fake_catalog
        .specs()
        .filter(|spec| spec.pure && spec.name.as_str() != "env.get")
        .count();
    assert_eq!(
        checked, pure_in_catalog,
        "a pure tool was added to the catalog without a case here"
    );
}
