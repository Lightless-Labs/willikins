//! The read-only live probe (plan acceptance test 15). `#[ignore]`, and
//! inert even under `--ignored` unless `WILLIKINS_LIVE_PROBE=1` -- the
//! credential is read (through
//! [`willikins_providers_buildkite::credential_from_env`]) only past that
//! gate, mirroring `willikins-providers-doppler/tests/live_probe.rs`.
//!
//! Performs only `GET`s, and only the three the plan names as the price
//! of running while the sandbox token still lives (it expires seven days
//! from 2026-09-16):
//!
//! 1. `GET /v2/access-token` -- needs **no** scope at all, so it is the
//!    cheapest possible proof the credential authenticates. Records the
//!    real scope spellings and `expires_at`, and asserts the three scopes
//!    this crate needs are present -- `read_pipelines`, `write_pipelines`,
//!    `read_clusters` -- without ever printing the token or any boolean
//!    derived from its bytes: `credential_from_env`'s own
//!    [`CREDENTIAL_PATTERN`](willikins_providers_buildkite::CREDENTIAL_PATTERN)
//!    already proves the `bkua_` prefix before this file ever runs, so
//!    nothing here needs to re-derive it from the raw value.
//! 2. `GET /v2/organizations/{org}/clusters` -- records the real cluster
//!    shape.
//! 3. `GET /v2/organizations/{org}/pipelines/<absent-slug>` -- records
//!    the status of a not-found pipeline. The raw body is not
//!    recoverable from this level ([`willikins_providers_http::ProviderError`]
//!    already drops it per trust boundary 5, keeping only a bounded
//!    `message`), so this probe records the status and whatever
//!    `message` shape the shared client extracted, not the literal body
//!    -- narrower than the plan's "status and body" phrasing, and
//!    recorded here as the limit it is.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_PROBE=1 \
//!   cargo test -p willikins-providers-buildkite --test live_probe -- --ignored \
//!   --nocapture
//! ```

mod common;

use serde_json::Value;
use willikins_types::{BuildkiteOrg, DomainType};

use common::record_and_compare;

/// The three scopes this crate's credential needs (plan trust boundary
/// 6). Checked both as the plural spelling the scope table uses and,
/// defensively, the singular the one worked example uses (research note
/// section 3, "verify" item 5) -- whichever the sandbox token's real
/// `GET /v2/access-token` response actually spells.
const NEEDED_SCOPES: [(&str, &str); 3] = [
    ("read_pipelines", "read_pipeline"),
    ("write_pipelines", "write_pipeline"),
    ("read_clusters", "read_cluster"),
];

#[test]
#[ignore = "opt-in live probe against a real Buildkite organisation; run with \
            WILLIKINS_LIVE_PROBE=1 and a sandbox bkua_ token sourced in the same command. \
            The sandbox token expires seven days from 2026-09-16."]
fn buildkite_live_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }

    let credential = willikins_providers_buildkite::credential_from_env()
        .expect("a valid sandbox Buildkite token");
    let org_value = std::env::var("WILLIKINS_SANDBOX_BUILDKITE_ORG")
        .expect("WILLIKINS_SANDBOX_BUILDKITE_ORG is set");
    let org = BuildkiteOrg::parse(&org_value).expect("a valid Buildkite organisation slug");
    let http = willikins_providers_buildkite::http_client(credential);

    let mut failures = Vec::new();

    // 1. GET /v2/access-token -- no scope needed.
    match http.get::<Value>("/v2/access-token") {
        Ok(body) => {
            common::record_raw("access_token_probe", &body);
            let scopes: Vec<String> = body
                .get("scopes")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            println!("access-token scopes observed: {scopes:?}");
            println!(
                "access-token expires_at: {}",
                body.get("expires_at")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing>")
            );
            println!(
                "credential prefix bkua_: true (enforced by CREDENTIAL_PATTERN at construction)"
            );
            for (plural, singular) in NEEDED_SCOPES {
                let has = scopes.iter().any(|s| s == plural || s == singular);
                if has {
                    println!("scope present: {plural} (or {singular})");
                } else {
                    failures.push(format!(
                        "missing needed scope: neither `{plural}` nor `{singular}` present in {scopes:?}"
                    ));
                }
            }
        }
        Err(err) => {
            println!("GET /v2/access-token: fail");
            failures.push(format!("access-token request failed: {err}"));
        }
    }

    // 2. GET /v2/organizations/{org}/clusters
    match http.get::<Value>(&format!(
        "/v2/organizations/{org}/clusters?page=1&per_page=100"
    )) {
        Ok(body) => match record_and_compare("clusters_list_page", &body) {
            Ok(()) => println!("GET /v2/organizations/{org}/clusters: pass"),
            Err(diff) => {
                println!("GET /v2/organizations/{org}/clusters: fail");
                failures.push(format!("clusters: {diff}"));
            }
        },
        Err(err) => {
            println!("GET /v2/organizations/{org}/clusters: fail");
            failures.push(format!("clusters request failed: {err}"));
        }
    }

    // 3. GET /v2/organizations/{org}/pipelines/<absent-slug>
    let absent = "willikins-probe-does-not-exist";
    match http.get::<Value>(&format!("/v2/organizations/{org}/pipelines/{absent}")) {
        Ok(_) => {
            println!("GET .../pipelines/{absent}: fail (expected the slug to be absent)");
            failures.push("expected 404 for a nonexistent pipeline slug".to_string());
        }
        Err(err) => {
            println!(
                "GET .../pipelines/{absent}: observed status {:?}, message shape: {}",
                err.status,
                if err.message.starts_with("provider says: ") {
                    "a `message` field was present"
                } else {
                    "no recognisable message field was present"
                }
            );
            if err.status != Some(404) {
                failures.push(format!(
                    "expected status 404 for an absent pipeline, got {:?}",
                    err.status
                ));
            }
        }
    }

    assert!(failures.is_empty(), "live probe failures: {failures:?}");
}
