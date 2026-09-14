//! The GitHub half of the read-only live probe. `#[ignore]`, and inert
//! even under `--ignored` unless `WILLIKINS_LIVE_PROBE=1` — the credential
//! is read (through [`willikins_providers_github::credential_from_env`],
//! itself `Credential::from_env`) only past that gate, per
//! `docs/HANDOFF.md`'s credentials rule: nothing else in this workspace
//! makes a network call to GitHub, and this file is the one place a real
//! token's bytes exist in a test process.
//!
//! Performs only `GET`s: `/user`, `/orgs/{org}`,
//! `/repos/{org}/willikins-probe-does-not-exist` (expects `404`), and, if
//! the org has at least one repository, that repository plus its Actions
//! public-key endpoint (skipped with a named reason otherwise — the
//! public key needs a real repository). Each response is written,
//! redacted, to `fixtures/github/live/<name>.json` and compared by
//! top-level key set against the authored fixture of the same endpoint;
//! a mismatch fails, naming the two key sets. Prints only endpoint names
//! and pass/skip/fail — never a body, a header, or the credential.
//!
//! The coordinator's own instructions say this session does not run this
//! test; the verifier does, sourcing
//! `~/.config/willikins/sandbox.env` in the same command:
//! `source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_PROBE=1 \
//! cargo test -p willikins-providers-github --test live_probe -- --ignored \
//! --nocapture` (`--nocapture` because `cargo test` otherwise swallows a
//! passing test's `println!` output, and this test's whole point is
//! those pass/skip/fail lines).

mod common;

use serde_json::Value;
use willikins_providers_http::ProviderError;
use willikins_types::{DomainType, GitHubOrg};

use common::record_and_compare;

/// One endpoint's whole check: record `result` (if it succeeded) against
/// `fixture_name`, print exactly one `pass`/`fail` line naming
/// `display_name` (a path *template*, never the real org or repo), and
/// push a detail string onto `failures` on either kind of failure. Shared
/// by every endpoint below except the deliberate-404 one, which wants the
/// opposite success condition.
fn check(
    display_name: &str,
    fixture_name: &str,
    result: Result<Value, ProviderError>,
    failures: &mut Vec<String>,
) {
    match result {
        Ok(body) => match record_and_compare(fixture_name, &body) {
            Ok(()) => println!("GET {display_name}: pass"),
            Err(diff) => {
                println!("GET {display_name}: fail");
                failures.push(format!("{display_name}: {diff}"));
            }
        },
        Err(err) => {
            println!("GET {display_name}: fail");
            failures.push(format!("{display_name}: request failed: {err}"));
        }
    }
}

/// The full name (`owner/repo`) of the org's first repository.
///
/// `Ok(None)` means the org genuinely has no repository; `Err` means the
/// listing call itself failed or answered a shape this probe could not
/// read. Either way the repository-dependent checks are skipped rather
/// than failed — a transport or auth problem there is not this probe's
/// story to tell twice — but the two must be told apart in the skip line,
/// because "skipped, the org is empty" and "skipped, willikins could not
/// ask" leave the repository and public-key fixtures unverified for
/// completely different reasons.
fn first_repo_full_name(
    http: &willikins_providers_http::Http,
    org: &GitHubOrg,
) -> Result<Option<String>, String> {
    let listing = http
        .get::<Value>(&format!("/orgs/{org}/repos"))
        .map_err(|err| format!("the listing failed: {err}"))?;
    let repos = listing
        .as_array()
        .ok_or_else(|| "the listing was not a JSON array".to_string())?;
    let Some(first) = repos.first() else {
        return Ok(None);
    };
    first
        .get("full_name")
        .and_then(Value::as_str)
        .map(|name| Some(name.to_string()))
        .ok_or_else(|| "the first repository carried no `full_name` string".to_string())
}

#[test]
#[ignore = "opt-in live probe against a real GitHub org; run with WILLIKINS_LIVE_PROBE=1 \
            and sandbox credentials sourced in the same command"]
fn github_live_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }

    let credential =
        willikins_providers_github::credential_from_env().expect("a valid sandbox GitHub token");
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    let org = GitHubOrg::parse(&org_value).expect("a valid GitHub org slug");
    let http = willikins_providers_github::http_client(credential);

    let mut failures = Vec::new();

    check("/user", "user", http.get::<Value>("/user"), &mut failures);
    check(
        "/orgs/{org}",
        "org",
        http.get::<Value>(&format!("/orgs/{org}")),
        &mut failures,
    );

    match http.get::<Value>(&format!("/repos/{org}/willikins-probe-does-not-exist")) {
        Ok(_) => {
            println!("GET /repos/{{org}}/willikins-probe-does-not-exist: fail");
            failures.push("/repos/{org}/willikins-probe-does-not-exist: expected 404".to_string());
        }
        Err(err) if err.status == Some(404) => {
            println!("GET /repos/{{org}}/willikins-probe-does-not-exist: pass");
        }
        Err(err) => {
            println!("GET /repos/{{org}}/willikins-probe-does-not-exist: fail");
            failures.push(format!(
                "/repos/{{org}}/willikins-probe-does-not-exist: unexpected status {:?}",
                err.status
            ));
        }
    }

    match first_repo_full_name(&http, &org) {
        Ok(None) => println!("repo + public-key: skip (the org has no repository)"),
        Err(reason) => println!("repo + public-key: skip ({reason})"),
        Ok(Some(full_name)) => {
            check(
                "/repos/{org}/{repo}",
                "repo_get_present",
                http.get::<Value>(&format!("/repos/{full_name}")),
                &mut failures,
            );
            check(
                "/repos/{org}/{repo}/actions/secrets/public-key",
                "actions_secret_public_key",
                http.get::<Value>(&format!("/repos/{full_name}/actions/secrets/public-key")),
                &mut failures,
            );
        }
    }

    assert!(failures.is_empty(), "live probe failures: {failures:?}");
}
