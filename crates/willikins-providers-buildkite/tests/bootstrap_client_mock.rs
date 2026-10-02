//! Acceptance test 1 (milestone 3i, task A1) for
//! `BuildkiteClient::get_pipeline_bootstrap` and
//! `BuildkiteClient::update_pipeline_configuration`.

use willikins_core::{ToolError, ToolErrorKind};
use willikins_providers_buildkite::{BuildkiteClient, PipelineBootstrapBody};
use willikins_providers_http::testing::{MockProvider, json_body};
use willikins_providers_http::{Credential, Http};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, DomainType};

/// Stands in for Buildkite's credential-bearing webhook URL field
/// (`provider.webhook_url`, trust boundary 7/4): distinctive enough that
/// its presence anywhere but inside the mocked response body is
/// unambiguously a leak.
const WEBHOOK_MARKER: &str = "https://webhook.buildkite.com/deliver/wlkn-webhook-marker-7f2q";

/// Stands in for a stored pipeline configuration's own content
/// (trust boundary 4: compared, never echoed).
const CONFIG_MARKER: &str = "# wlkn-config-marker-9k3r";

/// A validation error's own words, as Buildkite's `422` body nests them
/// under `errors[]` -- never read for its content by this client
/// (acceptance test 1; `provider_error_from_body`'s `errors[]` handling
/// is reserved for "already exists" detection, which this body does not
/// trigger).
const VALIDATION_ERROR_MARKER: &str = "wlkn-validation-marker-4m1x";

/// The bootstrap content `buildkite.pipeline.bootstrap.ensure` writes
/// (SHARED VALUES, milestone 3i plan, live cycle section): a block step,
/// not a command, so no caller-supplied YAML in this test is ever
/// command-shaped.
const BOOTSTRAP_CONTENT: &str = "steps:\n  - block: \"willikins bootstrap probe\"\n";

fn client_against(url: String) -> BuildkiteClient {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    BuildkiteClient::new(Http::new(url, Vec::new(), credential))
}

fn org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").unwrap()
}

fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").unwrap()
}

fn pipeline_path() -> String {
    "/v2/organizations/willikins-test/pipelines/third-thoughts".to_string()
}

// ---------------------------------------------------------------------
// get_pipeline_bootstrap: an exhaustive destructure pins two fields
// ---------------------------------------------------------------------

#[test]
fn get_pipeline_bootstrap_deserializes_exactly_description_and_configuration() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path().as_str())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "slug": "third-thoughts",
                "web_url": "https://buildkite.com/willikins-test/third-thoughts",
                "repository": "git@github.com:lightless-labs/third-thoughts.git",
                "cluster_id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "description": "managed-by: willikins",
                "configuration": BOOTSTRAP_CONTENT,
                "provider": { "webhook_url": WEBHOOK_MARKER },
            })
            .to_string(),
        )
        .create();
    let client = client_against(provider.url());

    let body = client.get_pipeline_bootstrap(&org(), &slug()).unwrap();
    // The exhaustive destructure: a third field added to the wire
    // response, or to this struct, would show up here as a compile
    // error, not a silently-ignored field.
    let PipelineBootstrapBody {
        description,
        configuration,
    } = body;
    assert_eq!(description, Some("managed-by: willikins".to_string()));
    assert_eq!(configuration, Some(BOOTSTRAP_CONTENT.to_string()));

    // `provider.webhook_url` is not a field on `PipelineBootstrapBody` at
    // all, so it cannot have reached either destructured value.
    assert!(!description.unwrap().contains(WEBHOOK_MARKER));
    assert!(!configuration.unwrap().contains(WEBHOOK_MARKER));
}

#[test]
fn get_pipeline_bootstrap_reads_a_null_description_and_configuration_as_none() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path().as_str())
        .with_status(200)
        .with_body(
            serde_json::json!({
                "id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "slug": "third-thoughts",
                "web_url": "https://buildkite.com/willikins-test/third-thoughts",
                "repository": "git@github.com:lightless-labs/third-thoughts.git",
                "cluster_id": null,
                "description": null,
                "configuration": null,
            })
            .to_string(),
        )
        .create();
    let client = client_against(provider.url());

    let PipelineBootstrapBody {
        description,
        configuration,
    } = client.get_pipeline_bootstrap(&org(), &slug()).unwrap();
    assert_eq!(description, None);
    assert_eq!(configuration, None);
}

#[test]
fn get_pipeline_bootstrap_maps_a_404_to_not_found() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path().as_str())
        .with_status(404)
        .create();
    let client = client_against(provider.url());

    // `Result::unwrap_err` needs `T: Debug` to build its panic message,
    // and `PipelineBootstrapBody` deliberately has none (acceptance test
    // 1), so this uses `let...else` instead.
    let Err(err) = client.get_pipeline_bootstrap(&org(), &slug()) else {
        panic!("expected a 404 to read as an error")
    };
    let err: ToolError = err.into();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

// ---------------------------------------------------------------------
// update_pipeline_configuration: the PATCH body carries exactly one key
// ---------------------------------------------------------------------

#[test]
fn update_pipeline_configuration_sends_exactly_the_configuration_key() {
    let mut provider = MockProvider::start();
    let patch = provider
        .mock("PATCH", pipeline_path().as_str())
        .match_body(json_body(
            serde_json::json!({ "configuration": BOOTSTRAP_CONTENT }),
        ))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "slug": "third-thoughts",
                "web_url": "https://buildkite.com/willikins-test/third-thoughts",
                "repository": "git@github.com:lightless-labs/third-thoughts.git",
                "cluster_id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "description": "managed-by: willikins",
                "configuration": CONFIG_MARKER,
                "provider": { "webhook_url": WEBHOOK_MARKER },
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());

    let result = client.update_pipeline_configuration(&org(), &slug(), BOOTSTRAP_CONTENT);

    // Neither marker, both present in the mocked response body, reaches
    // the return value: the response is deserialized into
    // `serde::de::IgnoredAny` and discarded, so the only possible return
    // value on a 2xx is `Ok(())`.
    assert_eq!(result, Ok(()));
    patch.assert();
}

#[test]
fn update_pipeline_configuration_maps_a_422_to_provider_with_only_the_bounded_message() {
    let mut provider = MockProvider::start();
    let patch = provider
        .mock("PATCH", pipeline_path().as_str())
        .match_body(json_body(
            serde_json::json!({ "configuration": BOOTSTRAP_CONTENT }),
        ))
        .with_status(422)
        .with_body(
            serde_json::json!({
                "message": "Validation Failed",
                "errors": [
                    { "message": VALIDATION_ERROR_MARKER, "field": "configuration" }
                ],
            })
            .to_string(),
        )
        // 422 is not in `is_retryable_status` (429 or 5xx only), so
        // exactly one attempt.
        .expect(1)
        .create();
    let client = client_against(provider.url());

    let err = client
        .update_pipeline_configuration(&org(), &slug(), BOOTSTRAP_CONTENT)
        .unwrap_err();
    let err: ToolError = err.into();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("Validation Failed"));
    assert!(!err.message.contains(VALIDATION_ERROR_MARKER));
    patch.assert();
}
