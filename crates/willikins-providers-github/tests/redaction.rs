//! Acceptance test 4's share for this crate: a credential marker and a
//! secret marker appear in no fixture, no recorded request except the
//! `Authorization` header, and no error. The plaintext-never-leaks half
//! (`ensure` never places the sealed secret's bytes in a `ToolError`) is
//! `actions_secret_ensure_mock.rs`'s
//! `ensure_never_leaks_the_marker_in_an_error`; the ciphertext-never-
//! contains-the-plaintext half is `seal.rs`'s own tests. This file covers
//! the two remaining claims: no authored fixture carries either marker,
//! and a credential built from a distinctive marker never reaches a
//! recorded request anywhere but the `Authorization` header it belongs
//! in.

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{PortName, Tool, Value};
use willikins_providers_github::{GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, GitHubRepo, RepoVisibility};

/// Stands in for a real GitHub token: shaped like one (so nothing in the
/// pipeline could reject it before the point this test cares about) but
/// distinctive enough that its presence anywhere but the `Authorization`
/// header is unambiguously a bug.
const CREDENTIAL_MARKER: &str = "ghp_wlknCredentialMarker00000000000000000";

/// Stands in for a real Doppler service token this crate would seal and
/// send to GitHub's Actions secret endpoint.
const SECRET_MARKER: &str = "dp.st.prd.wlknSecretMarker00000000000000000000000000wlkn";

#[test]
fn no_authored_fixture_carries_either_marker() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/github");
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures/github/ is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() || path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "{} carries the credential marker",
            path.display()
        );
        assert!(
            !text.contains(SECRET_MARKER),
            "{} carries the secret marker",
            path.display()
        );
    }
}

#[test]
fn a_marker_credential_reaches_no_header_but_authorization() {
    let mut provider = MockProvider::start();
    let captured_headers = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = captured_headers.clone();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body_from_request(move |request| {
            let headers = request
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    )
                })
                .collect();
            *capture.lock().expect("not poisoned") = headers;
            br#"{"visibility":"private","topics":["managed-by-willikins"]}"#.to_vec()
        })
        .create();

    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", CREDENTIAL_MARKER);
    let http = Http::new(
        provider.url(),
        willikins_providers_github::default_headers(),
        credential,
    );
    let client = Arc::new(GitHubClient::new(http));
    let tool = GitHubRepoEnsure::new(client);

    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("repo").unwrap(),
        Value::known(GitHubRepo::parse("acme/widget").unwrap()),
    );
    inputs.insert(
        PortName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );
    tool.read(&inputs).expect("the mock answers 200");

    let headers = captured_headers.lock().expect("not poisoned").clone();
    assert!(
        !headers.is_empty(),
        "the mock handler ran and captured headers"
    );
    let mut saw_authorization_with_marker = false;
    for (name, value) in &headers {
        if name.eq_ignore_ascii_case("authorization") {
            saw_authorization_with_marker |= value.contains(CREDENTIAL_MARKER);
        } else {
            assert!(
                !value.contains(CREDENTIAL_MARKER),
                "header `{name}` leaked the credential marker: {value}"
            );
        }
    }
    assert!(
        saw_authorization_with_marker,
        "the Authorization header should carry the marker"
    );
}
