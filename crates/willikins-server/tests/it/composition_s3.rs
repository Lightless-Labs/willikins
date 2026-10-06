//! Milestone 2b, task S3 (`docs/plans/2026-10-05-milestone-2b-composition.md`,
//! decision (d11), acceptance 10, verify item 6): the approvals page for a
//! composite's pending plan lists each used document's name and short sha,
//! and every string on the page -- including a node's `/`-separated path --
//! appears only in escaped text, never in a URL or a form field name.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _;

use willikins_journal::DocumentSha256;
use willikins_server::{Butler, HttpConfig, TokenHash};
use willikins_types::{DomainType, WorkflowName};

use crate::common;

const ALLOWED_HOST: &str = "willikins.example";
const AGENT_TOKEN: &str = "agent-token-for-composition-s3-tests";
const APPROVER_TOKEN: &str = "approver-token-for-composition-s3-tests";

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn write(dir: &std::path::Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

// The child's own node name is `danger`, the step that uses it is named
// `c` -- deliberately different from the child's own workflow name
// (`child`), so a test reading the page can tell apart "the used
// document's name" (what the used-documents list must show) from "the
// step/node name" (what it must never need to, and what the plan's own
// JSON shows instead).
const CHILD: &str = "name: child\ninputs:\n  slug: { type: ProjectSlug }\nsteps:\n  danger:\n    tool: fake.irreversible.ensure\n    with:\n      key: ${{ inputs.slug }}\n";
const ROOT: &str =
    "name: root\nsteps:\n  c:\n    uses: child\n    with:\n      slug: third-thoughts\n";

fn base_config() -> HttpConfig {
    HttpConfig::build(
        "127.0.0.1:0".parse().unwrap(),
        vec![TokenHash::of(AGENT_TOKEN)],
        TokenHash::of(APPROVER_TOKEN),
        vec![ALLOWED_HOST.to_string()],
    )
    .unwrap()
}

fn basic_auth_header(username: &str, password: &str) -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    )
}

async fn get_approvals(router: axum::Router) -> String {
    let request = Request::builder()
        .method("GET")
        .uri("/approvals")
        .header("host", ALLOWED_HOST)
        .header(
            "authorization",
            basic_auth_header("approver-1", APPROVER_TOKEN),
        )
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Every value of every `attr="..."` occurrence of `attr` in `html`, in
/// document order -- a hand-rolled extraction (no HTML parser is a
/// workspace dependency) good enough for this page's own fixed markup.
fn attr_values(html: &str, attr: &str) -> Vec<String> {
    let marker = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(idx) = rest.find(&marker) {
        let after = &rest[idx + marker.len()..];
        let end = after
            .find('"')
            .unwrap_or_else(|| panic!("unterminated {attr}= attribute in: {after}"));
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn acceptance_10_the_approvals_page_lists_each_used_documents_name_and_short_sha() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD);
    write(dir.path(), "root.yaml", ROOT);
    let (_state, catalog) = Butler::fake_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);
    let butler = Arc::new(butler);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("a composite with one irreversible node plans cleanly");
    assert!(
        response.requires_approval,
        "fake.irreversible.ensure is Irreversible: this plan must need approval, or the test \
         would find an empty approvals page and vacuously pass"
    );
    let plan_id = response.plan_id.to_string();

    // Read the recorded closure back through the journal-backed read view
    // (`pending_approvals`), not recomputed from the fixture's own bytes:
    // this pins the page to what S2 actually wrote, not to an
    // independent recomputation that could agree with the page by
    // coincidence while both disagreed with the journal.
    let pending = butler.pending_approvals();
    let record = pending
        .iter()
        .find(|record| record.plan_id == response.plan_id)
        .expect("the plan is pending approval");
    let child_sha = record
        .used
        .get(&wf("child"))
        .expect("PlanRecorded.used names the child document");
    let expected_short = &child_sha.as_str()[..12];
    assert_eq!(
        child_sha,
        &DocumentSha256::compute(CHILD.as_bytes()),
        "sanity: the recorded sha is the child document's own content hash"
    );

    let router = willikins_server::router(Arc::clone(&butler), &base_config());
    let html = get_approvals(router).await;
    assert!(
        html.contains(&plan_id),
        "the page does not show the pending plan at all: {html}"
    );

    // 1. The used document's *name* and *short sha*, in escaped text.
    let expected_entry = format!("<li>child @ {expected_short}</li>");
    assert!(
        html.contains(&expected_entry),
        "expected `{expected_entry}` on the page: {html}"
    );

    // 2. Verify item 6: a node's `/`-separated path never needs encoding
    // in a URL or a form field name, because it never appears in either.
    // Checked by construction, not by absence: every `action=` is this
    // one plan's decision endpoint, and every `name=` is one of the
    // page's own three fixed form fields -- neither kind of attribute
    // carries a document or node name at all.
    let actions = attr_values(&html, "action");
    assert!(!actions.is_empty(), "the page has no form: {html}");
    let expected_action = format!("/approvals/{plan_id}");
    for action in &actions {
        assert_eq!(*action, expected_action, "unexpected form action: {html}");
    }
    let names = attr_values(&html, "name");
    assert!(!names.is_empty(), "the page has no named field: {html}");
    let allowed: std::collections::BTreeSet<&str> =
        ["nonce", "decision", "reason"].into_iter().collect();
    for name in &names {
        assert!(
            allowed.contains(name.as_str()),
            "a form field name must be one of {allowed:?}, found `{name}`: {html}"
        );
    }

    // 3. The node path *does* appear -- just in the plan's own escaped
    // JSON text, not in a URL or a field name: `c` is the `uses:` step,
    // `danger` the child's own node (decision (d3)'s `<step>/<node>`).
    assert!(
        html.contains("c/danger"),
        "the plan's own JSON should still show the flattened node path: {html}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_documents_approvals_section_has_no_used_documents_list() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = Butler::fake_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);
    let butler = Arc::new(butler);

    let response = butler
        .plan(
            willikins_types::WorkflowName::parse(common::IRREVERSIBLE_NAME).unwrap(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the irreversible fixture plans cleanly");
    assert!(response.requires_approval, "{response:?}");

    let router = willikins_server::router(Arc::clone(&butler), &base_config());
    let html = get_approvals(router).await;
    assert!(
        html.contains(&response.plan_id.to_string()),
        "the plan is not on the page: {html}"
    );
    assert!(
        !html.contains("used-documents"),
        "a plan with no `uses:` step must carry no used-documents list: {html}"
    );
}
