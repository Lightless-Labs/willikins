//! Milestone 3m task P4: mock tests for `buildkite.pipeline.ensure`'s
//! inverse (decisions 4, 7; acceptance 12, 21). Each test pins the exact
//! query the inverse issues and, where decision 7's own table refuses
//! before the builds read, that the builds `GET` was never issued at
//! all (trust boundary 6: "all reads before any delete").

use std::sync::Arc;

use willikins_core::rollback::CreationWindow;
use willikins_core::{
    Created, Inputs, Loss, Outputs, Standing, Timestamp, Tool, ToolErrorKind, Value,
};
use willikins_providers_buildkite::{BuildkiteClient, BuildkitePipelineEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, DomainType, TeardownToken};

fn client_against(url: String) -> Arc<BuildkiteClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    Arc::new(BuildkiteClient::new(Http::new(url, Vec::new(), credential)))
}

fn org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").unwrap()
}

fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").unwrap()
}

/// `[2026-01-01T00:00:00Z, 2026-01-01T00:10:00Z]`, inclusive both ends.
fn window() -> CreationWindow {
    CreationWindow {
        start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
        end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
    }
}

fn created_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        willikins_core::PortName::parse("org").unwrap(),
        Value::known(org()),
    );
    inputs.insert(
        willikins_core::PortName::parse("slug").unwrap(),
        Value::known(slug()),
    );
    inputs
}

/// A [`Created`] over `inputs`/`accept`, the fixed [`window`], and no
/// riders (this tool's own standing rule reads none, decision 7).
fn created<'a>(inputs: &'a Inputs, outputs: &'a Outputs, accept: &'a [Loss]) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window: window(),
        settled_by: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
        accept,
        riders: &[],
    }
}

/// A pipeline standing-read JSON body matching [`org`]/[`slug`], inside
/// [`window`], with the ownership marker -- the `AsCreated` case before
/// the builds read.
fn as_created_body() -> serde_json::Value {
    serde_json::json!({
        "slug": "third-thoughts",
        "description": "managed-by: willikins",
        "created_at": "2026-01-01T00:05:00Z",
    })
}

fn empty_builds_body() -> serde_json::Value {
    serde_json::json!([])
}

fn one_build_body() -> serde_json::Value {
    serde_json::json!([{"id": "b1", "number": 1}])
}

fn builds_path() -> &'static str {
    "/v2/organizations/willikins-test/pipelines/third-thoughts/builds?per_page=1"
}

fn pipeline_path() -> &'static str {
    "/v2/organizations/willikins-test/pipelines/third-thoughts"
}

/// `BuildkitePipelineEnsure` itself implements nothing beyond `Tool`;
/// its `inverse()` is the one under test.
fn standing(tool: &BuildkitePipelineEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("buildkite.pipeline.ensure declares Delete, so inverse() is Some")
        .standing(created)
        .expect("standing never fails against a well-formed mock response")
}

#[test]
fn standing_reports_gone_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
    let builds = provider.mock("GET", builds_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    builds.assert();
}

#[test]
fn standing_reports_not_ours_when_the_marker_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["description"] = serde_json::json!("some other description");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let builds = provider.mock("GET", builds_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    builds.assert();
}

#[test]
fn standing_reports_not_ours_when_the_description_is_missing_outright() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body.as_object_mut().unwrap().remove("description");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_the_slug_differs() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["slug"] = serde_json::json!("renamed");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let builds = provider.mock("GET", builds_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    builds.assert();
}

#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_outside_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2026-01-01T00:10:01Z");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let builds = provider.mock("GET", builds_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    builds.assert();
}

/// The lower edge, mirroring the upper-edge test above: one second
/// before the window's own start is `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_before_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2025-12-31T23:59:59Z");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// The same instant as the window's own end is still inside it (an
/// inclusive window), so this reaches the builds read and comes back
/// `AsCreated`.
#[test]
fn standing_reports_as_created_when_created_at_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2026-01-01T00:10:00Z");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(empty_builds_body().to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

/// A response that omits `created_at` outright fails closed: never "in
/// the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body.as_object_mut().unwrap().remove("created_at");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// Buildkite's own documented (non-RFC-3339) `created_at` shape is
/// accepted too (`client::tests` already pins the parse itself; this
/// pins that `standing` actually uses it).
#[test]
fn standing_reports_as_created_when_created_at_is_buildkites_own_documented_shape() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2026-01-01 00:05:00 UTC");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(empty_builds_body().to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

#[test]
fn standing_reports_as_created_with_no_loss_when_there_are_no_builds() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(empty_builds_body().to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

#[test]
fn standing_reports_as_created_with_the_builds_loss_when_a_build_exists() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(one_build_body().to_string())
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated {
            losing: vec!["builds"]
        }
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_is_a_noop_when_already_gone() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
    let delete = provider.mock("DELETE", pipeline_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_deletes_when_as_created_with_no_builds() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(empty_builds_body().to_string())
        .create();
    let delete = provider
        .mock("DELETE", pipeline_path())
        .with_status(204)
        .expect(1)
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(!deleted.already_gone);
    delete.assert();
}

/// Acceptance 21's provider half: a build Buildkite reports but the
/// node's own `accept` does not name refuses the delete as a `Conflict`,
/// never a silent `Delete`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_refuses_as_a_conflict_when_a_build_exists_and_is_not_accepted() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(one_build_body().to_string())
        .create();
    let delete = provider.mock("DELETE", pipeline_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let err = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// The accepted half: a node whose policy names `builds` in its own
/// `accept` list lets the delete through despite the same reported loss.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_deletes_when_the_build_loss_is_accepted() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", builds_path())
        .with_status(200)
        .with_body(one_build_body().to_string())
        .create();
    let delete = provider
        .mock("DELETE", pipeline_path())
        .with_status(204)
        .expect(1)
        .create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let accept = [Loss::parse("builds").unwrap()];
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &accept), &token)
        .unwrap();
    assert!(!deleted.already_gone);
    delete.assert();
}

#[test]
fn delete_refuses_as_a_conflict_when_not_ours() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["slug"] = serde_json::json!("renamed");
    provider
        .mock("GET", pipeline_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let delete = provider.mock("DELETE", pipeline_path()).expect(0).create();
    let tool = BuildkitePipelineEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = TeardownToken::new();
    let err = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// Decision 4's reversal declaration itself: `Delete`, no container, and
/// the one declared loss `builds` (acceptance 1's own validation already
/// proves this against `Catalog::insert`; `catalog_check_parity.rs` does
/// so for this crate when a later task inserts this tool into a
/// catalog).
#[test]
fn reversal_declares_delete_with_no_within_and_the_builds_loss() {
    let tool = BuildkitePipelineEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("buildkite.pipeline.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["org", "slug", "token"]);
    assert!(within.is_empty());
    assert_eq!(losses.to_vec(), vec!["builds"]);
}
