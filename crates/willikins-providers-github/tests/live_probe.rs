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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::ProviderError;
use willikins_providers_http::testing::load_fixture;
use willikins_types::{DomainType, GitHubOrg};

/// Field names stripped, recursively, from every value this probe writes
/// to disk or compares. None of the four endpoints this probe calls
/// returns anything secret (GitHub's own schemas confirm this: `/user`,
/// `/orgs/{org}`, a repository, and the Actions public key — which is,
/// itself, public), but the redaction step exists here so task 8's
/// Doppler probe, which does call an endpoint capable of carrying a
/// secret value, can copy this exact shape rather than invent its own.
const REDACTED_FIELD_NAMES: &[&str] = &[
    "token",
    "secret",
    "password",
    "encrypted_value",
    "client_secret",
    "value",
];

/// Recursively strip [`REDACTED_FIELD_NAMES`] from `value`, replacing
/// each with a fixed marker rather than deleting the key outright, so a
/// recorded fixture's shape (which fields exist) survives redaction —
/// only their content is removed.
fn redact(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, val)| {
                    let redacted = if REDACTED_FIELD_NAMES.contains(&key.as_str()) {
                        Value::String("[REDACTED]".to_string())
                    } else {
                        redact(val)
                    };
                    (key, redacted)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn live_dir() -> PathBuf {
    fixtures_dir().join("github").join("live")
}

fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Record `live` (redacted) under `name.json`, compare its top-level key
/// set against `fixtures/github/<name>.json`'s, and report `pass` or
/// `fail` (never panicking on a mismatch by itself — the caller decides
/// whether a given endpoint's drift should fail the run).
///
/// The comparison is a subset check — every key the authored fixture
/// names must be present in the live response — not exact equality. A
/// real GitHub response carries dozens of fields no fixture here
/// authors (`user.json` and `org.json` most starkly: they name only a
/// handful of well-known fields, not the full schema); failing on an
/// unauthored *extra* field would make this probe brittle to GitHub
/// adding fields, which is not "the shape drifted" in any sense this
/// crate's tools care about. A field this crate actually reads
/// (`repo_get_present.json`'s `visibility`/`topics`, both authored) going
/// *missing* from a live response is exactly the drift worth failing on.
fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/github/live/");
    let redacted = redact(live.clone());
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redacted).expect("serializes"),
    )
    .expect("can write the recorded fixture");

    let authored = load_fixture(&fixtures_dir(), "github", name);
    let live_keys = top_level_keys(live);
    let authored_keys = top_level_keys(&authored);
    let missing: Vec<_> = authored_keys.difference(&live_keys).cloned().collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "missing from the live response, present in the authored fixture: {missing:?}"
        ))
    }
}

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
