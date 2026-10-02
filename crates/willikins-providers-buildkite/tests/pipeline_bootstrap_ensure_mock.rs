//! `buildkite.pipeline.bootstrap.ensure`, milestone 3i task A2: acceptance
//! tests 2 (one per row of decision (a2)'s table), 3 (the path and
//! content rule), 5 (spec), and 6 (the gate and this tool agree on
//! "equal").

use std::sync::Arc;

use willikins_core::{
    Class, Observation, PortName, PortType, SinkToken, Tool, ToolErrorKind, Value,
};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkitePipelineBootstrapEnsure, BuildkitePipelineBootstrapGate,
};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, DomainType, RepoFile, RepoPath};

fn org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").unwrap()
}

fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").unwrap()
}

fn path() -> &'static str {
    "/v2/organizations/willikins-test/pipelines/third-thoughts"
}

fn valid_configuration() -> RepoFile {
    RepoFile::new(
        RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
        "steps:\n  - command: \"echo hi\"\n",
    )
    .unwrap()
}

fn ensure_tool_against(url: String) -> BuildkitePipelineBootstrapEnsure {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
    let http = Http::new(url, Vec::new(), credential);
    BuildkitePipelineBootstrapEnsure::new(Arc::new(BuildkiteClient::new(http)))
}

fn gate_tool_against(url: String) -> BuildkitePipelineBootstrapGate {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
    let http = Http::new(url, Vec::new(), credential);
    BuildkitePipelineBootstrapGate::new(Arc::new(BuildkiteClient::new(http)))
}

fn ensure_inputs(configuration: &RepoFile) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
    inputs.insert(
        PortName::parse("configuration").unwrap(),
        Value::known(configuration.clone()),
    );
    inputs
}

fn gate_inputs(configuration: &RepoFile) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
    inputs.insert(
        PortName::parse("expected").unwrap(),
        Value::known(configuration.clone()),
    );
    inputs
}

// ---------------------------------------------------------------------
// Acceptance test 2: one row per decision (a2) state
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn missing_reads_absent_updates_true_and_ensure_not_found_with_zero_patch() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(404)
        .with_body(serde_json::json!({"message": "not found"}).to_string())
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .expect(0)
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());

    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
    assert!(tool.updates(&inputs).unwrap());

    let token = SinkToken::new();
    let err = tool.ensure(&inputs, &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    patch.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn foreign_reads_foreign_and_ensure_conflicts_with_zero_patch() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({"description": null, "configuration": "steps:\n  - command: \"echo hi\"\n"})
                .to_string(),
        )
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .expect(0)
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());

    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "{observation:?}"
    );
    assert!(!tool.updates(&inputs).unwrap());

    let token = SinkToken::new();
    let err = tool.ensure(&inputs, &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    patch.assert();
}

/// Adversarial pass (milestone 3i, 2026-10-0x): ownership is *exact*
/// equality against `MANAGED_DESCRIPTION` ("managed-by: willikins"), never
/// a substring test. A pipeline a human or another tool named
/// `"managed-by: willikins-production"` (`MANAGED_DESCRIPTION` as a
/// prefix of a longer, foreign description) or `"custom (managed-by:
/// willikins) pipeline"` (as an embedded substring) is still foreign: a
/// weaker `contains` check would let this tool `PATCH` -- overwriting a
/// stored configuration trust boundary 4 says is never read back -- a
/// pipeline this crate never created. The stored `configuration` is set
/// equal to the document's rendered content on purpose, so a weakened
/// ownership check would otherwise read `Present`/`Equal` and `ensure`
/// would silently report `changed: false` with zero `PATCH`, not merely a
/// wrong `Foreign`/`Conflict` -- either wrong answer is caught here.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_description_that_only_contains_the_marker_as_a_substring_is_still_foreign() {
    let rendered = "steps:\n  - command: \"echo hi\"\n";
    for foreign_description in [
        "managed-by: willikins-production",
        "custom (managed-by: willikins) pipeline",
        "managed-by: willikins ",
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", path())
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "description": foreign_description,
                    "configuration": rendered,
                })
                .to_string(),
            )
            .create();
        let patch = provider
            .mock("PATCH", path())
            .match_query(mockito::Matcher::Missing)
            .expect(0)
            .create();
        let tool = ensure_tool_against(provider.url());
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
            rendered,
        )
        .unwrap();
        let inputs = ensure_inputs(&configuration);

        let observation = tool.read(&inputs).unwrap();
        assert!(
            matches!(observation, Observation::Foreign),
            "{foreign_description:?}: {observation:?}"
        );
        assert!(!tool.updates(&inputs).unwrap(), "{foreign_description:?}");

        let token = SinkToken::new();
        let err = tool.ensure(&inputs, &token).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Conflict, "{foreign_description:?}");
        patch.assert();
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn equal_re_quoted_reads_present_and_ensure_changes_nothing_with_zero_patch() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "description": "managed-by: willikins",
                "configuration": "steps:\n  - command: 'echo hi'\n",
            })
            .to_string(),
        )
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .expect(0)
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());

    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Present(_)),
        "{observation:?}"
    );
    assert!(!tool.updates(&inputs).unwrap());

    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).unwrap();
    assert!(!ensured.changed);
    patch.assert();
}

fn different_stored_body() -> String {
    serde_json::json!({
        "description": "managed-by: willikins",
        "configuration": "steps:\n  - command: \"buildkite-agent pipeline upload\"\n",
    })
    .to_string()
}

#[test]
fn different_reads_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(different_stored_body())
        .create();
    let tool = ensure_tool_against(provider.url());
    let observation = tool.read(&ensure_inputs(&valid_configuration())).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
}

#[test]
fn different_updates_reports_true() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(different_stored_body())
        .create();
    let tool = ensure_tool_against(provider.url());
    assert!(
        tool.updates(&ensure_inputs(&valid_configuration()))
            .unwrap()
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn different_ensure_patches_then_re_reads_equal_and_reports_changed() {
    let mut provider = MockProvider::start();
    // Both the pre-patch `analyze` and the post-patch re-`analyze` GET the
    // same route; this one mock, with no `.expect()` cap, answers every
    // GET with the pre-patch "different" body until it is replaced below
    // -- but `ensure` itself only ever issues two GETs (pre- and
    // post-patch), so a plain two-mock, registration-ordered sequence
    // (mockito plays an exactly-once mock out before falling through to
    // the next one registered for the same route) is enough: the first
    // GET this test will see is answered by `pre_patch_read` (its own
    // `.expect(1)`), and the second by `post_patch_read`.
    let pre_patch_read = provider
        .mock("GET", path())
        .with_status(200)
        .with_body(different_stored_body())
        .expect(1)
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "configuration": "steps:\n  - command: \"echo hi\"\n",
            }),
        ))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "configuration": "steps:\n  - command: \"echo hi\"\n",
                "provider": {"webhook_url": "https://webhook.buildkite.com/deliver/NEVER-READ"},
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let post_patch_read = provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "description": "managed-by: willikins",
                "configuration": "steps:\n  - command: \"echo hi\"\n",
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());

    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).unwrap();
    assert!(ensured.changed);
    pre_patch_read.assert();
    patch.assert();
    post_patch_read.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn different_whose_patch_succeeds_but_re_read_still_differs_reports_provider() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "description": "managed-by: willikins",
                "configuration": "steps:\n  - command: \"buildkite-agent pipeline upload\"\n",
            })
            .to_string(),
        )
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(
            serde_json::json!({"configuration": "steps:\n  - command: \"still wrong\"\n"})
                .to_string(),
        )
        .expect(1)
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());

    let token = SinkToken::new();
    let err = tool.ensure(&inputs, &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("does not equal"), "{}", err.message);
    patch.assert();
}

/// `willikins_providers_http::http::run_retrying`'s own doc: a retryable
/// verb is attempted `MAX_RETRIES + 1` times in total. `MAX_RETRIES` is
/// `3` (`willikins-providers-http/src/http.rs`), so a `PATCH` that fails
/// with a `5xx` on every attempt is sent exactly 4 times -- pinned here
/// rather than left unbounded, per decision (a2)'s acceptance test 2.
const PATCH_RETRY_ATTEMPTS: usize = 4;

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_patch_500_then_a_re_read_equal_reports_changed_false() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "description": "managed-by: willikins",
                "configuration": "steps:\n  - command: \"buildkite-agent pipeline upload\"\n",
            })
            .to_string(),
        )
        .create();
    let patch = provider
        .mock("PATCH", path())
        .match_query(mockito::Matcher::Missing)
        .with_status(500)
        .with_body(serde_json::json!({"message": "boom"}).to_string())
        .expect(PATCH_RETRY_ATTEMPTS)
        .create();
    let second_read = provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "description": "managed-by: willikins",
                "configuration": "steps:\n  - command: \"echo hi\"\n",
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
    let http = Http::new(provider.url(), Vec::new(), credential).with_sleeper(Arc::new(NoSleep));
    let tool = BuildkitePipelineBootstrapEnsure::new(Arc::new(BuildkiteClient::new(http)));
    let inputs = ensure_inputs(&valid_configuration());

    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).unwrap();
    assert!(!ensured.changed);
    patch.assert();
    second_read.assert();
}

/// A [`willikins_providers_http::Sleeper`] that never actually sleeps,
/// so the retry test above runs instantly instead of waiting out real
/// exponential backoff.
struct NoSleep;

impl willikins_providers_http::Sleeper for NoSleep {
    fn sleep(&self, _duration: std::time::Duration) {}
}

#[test]
fn a_null_stored_configuration_reads_different_never_equal() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", path())
        .with_status(200)
        .with_body(
            serde_json::json!({"description": "managed-by: willikins", "configuration": null})
                .to_string(),
        )
        .create();
    let tool = ensure_tool_against(provider.url());
    let inputs = ensure_inputs(&valid_configuration());
    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
    assert!(tool.updates(&inputs).unwrap());
}

// ---------------------------------------------------------------------
// Acceptance test 3: the path and content rule, at the full-tool level
// (zero HTTP calls on refusal)
// ---------------------------------------------------------------------

fn unreachable_tool() -> BuildkitePipelineBootstrapEnsure {
    // No mock server at all: a request landing here fails the test with a
    // transport error, which proves the refusal happened before any HTTP
    // call was ever attempted.
    ensure_tool_against("http://127.0.0.1:1".to_string())
}

fn accepts(path: &str, content: &str) {
    let configuration = RepoFile::new(RepoPath::parse(path).unwrap(), content).unwrap();
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(404)
        .create();
    let tool = ensure_tool_against(provider.url());
    let result = tool.read(&ensure_inputs(&configuration));
    assert!(result.is_ok(), "{path} should be accepted: {result:?}");
}

#[test]
fn accepted_paths() {
    accepts(
        "apps/walter/.buildkite/bootstrap.yml",
        "steps:\n  - command: \"echo hi\"\n",
    );
    accepts(
        ".buildkite/pipeline.yaml",
        "steps:\n  - command: \"echo hi\"\n",
    );
}

#[test]
fn refused_paths_make_zero_http_calls_and_name_neither_path_nor_content() {
    let cases = [
        "apps/walter/.buildkite/plugins/stage-input/plugin.yml",
        "apps/walter/bootstrap.yml",
        "apps/walter/.buildkite/bootstrap.YML",
        "apps/walter/.buildkite/upload-pipeline.sh",
        "apps/walter/.Buildkite/bootstrap.yml",
    ];
    for case in cases {
        let configuration = RepoFile::new(
            RepoPath::parse(case).unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        let tool = unreachable_tool();
        let err = match tool.read(&ensure_inputs(&configuration)) {
            Ok(observation) => panic!("{case} should have been refused, got {observation:?}"),
            Err(err) => err,
        };
        assert_eq!(err.kind, ToolErrorKind::Invalid, "{case}");
        assert!(!err.message.contains(case), "{case}: {}", err.message);
    }
}

#[test]
fn refused_content_makes_zero_http_calls_and_names_neither_path_nor_content() {
    let cases: &[(&str, &str)] = &[
        ("steps: []", "an empty steps sequence"),
        ("env: {}", "no steps key at all"),
        ("- steps\n- more\n", "a top-level sequence"),
        ("not: valid: yaml: at: all:", "unparsable text"),
        (
            "steps:\n  - command: \"a\"\nsteps:\n  - command: \"b\"\n",
            "a duplicate top-level key",
        ),
    ];
    for (content, label) in cases {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
            *content,
        )
        .unwrap();
        let tool = unreachable_tool();
        let err = match tool.read(&ensure_inputs(&configuration)) {
            Ok(observation) => panic!("{label} should have been refused, got {observation:?}"),
            Err(err) => err,
        };
        assert_eq!(err.kind, ToolErrorKind::Invalid, "{label}");
        assert!(!err.message.contains(content), "{label}: {}", err.message);
    }
}

// ---------------------------------------------------------------------
// Acceptance test 5: spec
// ---------------------------------------------------------------------

#[test]
fn spec_matches_shared_values() {
    let tool = ensure_tool_against("http://127.0.0.1:1".to_string());
    let spec = tool.spec();
    spec.validate(willikins_types::registry()).unwrap();
    assert_eq!(
        spec.key,
        vec![
            PortName::parse("org").unwrap(),
            PortName::parse("slug").unwrap()
        ]
    );
    assert_eq!(spec.class, Class::Destructive);
    assert!(!spec.pure);
    assert!(tool.gate().is_none());
    assert_eq!(spec.outputs.len(), 1);
    let configuration_port = spec
        .inputs
        .get(&PortName::parse("configuration").unwrap())
        .unwrap();
    assert_eq!(
        configuration_port.ty,
        PortType::Exact(willikins_core::tool::helpers::scalar("RepoFile"))
    );
    assert!(configuration_port.required);
    let token_port = spec.inputs.get(&PortName::parse("token").unwrap()).unwrap();
    assert!(!token_port.required);
}

// ---------------------------------------------------------------------
// Acceptance test 6: the gate and this tool agree on "equal"
// ---------------------------------------------------------------------

#[test]
fn the_gate_and_the_writer_agree_on_equal_across_a_shared_table() {
    let rendered = "steps:\n  - command: \"echo hi\"\n";
    let cases: &[(&str, bool)] = &[
        // (stored, expected `Present`/`Equal`)
        ("steps:\n  - command: 'echo hi'\n", true),
        ("steps:\n  - command: \"echo hi\"\n", true),
        ("steps:\n  - command: \"echo bye\"\n", false),
        ("not: valid: yaml: at: all:", false),
    ];
    for (stored, expect_equal) in cases {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
            rendered,
        )
        .unwrap();

        let mut gate_provider = MockProvider::start();
        gate_provider
            .mock("GET", path())
            .with_status(200)
            .with_body(serde_json::json!({"configuration": stored}).to_string())
            .create();
        let gate_observation = gate_tool_against(gate_provider.url())
            .read(&gate_inputs(&configuration))
            .unwrap();
        let gate_says_equal = matches!(gate_observation, Observation::Present(_));

        let mut ensure_provider = MockProvider::start();
        ensure_provider
            .mock("GET", path())
            .with_status(200)
            .with_body(
                serde_json::json!({"description": "managed-by: willikins", "configuration": stored})
                    .to_string(),
            )
            .create();
        let ensure_observation = ensure_tool_against(ensure_provider.url())
            .read(&ensure_inputs(&configuration))
            .unwrap();
        let ensure_says_equal = matches!(ensure_observation, Observation::Present(_));

        assert_eq!(
            gate_says_equal, *expect_equal,
            "gate disagreed with the table for stored = {stored:?}"
        );
        assert_eq!(
            ensure_says_equal, *expect_equal,
            "writer disagreed with the table for stored = {stored:?}"
        );
        assert_eq!(
            gate_says_equal, ensure_says_equal,
            "gate and writer disagreed for stored = {stored:?}"
        );
    }
}
