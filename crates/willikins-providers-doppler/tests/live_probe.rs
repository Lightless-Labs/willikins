//! The Doppler half of the read-only live probe. `#[ignore]`, and inert
//! even under `--ignored` unless `WILLIKINS_LIVE_PROBE=1` — the credential
//! is read (through [`willikins_providers_doppler::credential_from_env`],
//! itself `Credential::from_env`) only past that gate, per
//! `docs/HANDOFF.md`'s credentials rule: nothing else in this workspace
//! makes a network call to Doppler, and this file is the one place a
//! real token's bytes exist in a test process.
//!
//! # This probe has never run
//!
//! The operator's sandbox environment (`~/.config/willikins/sandbox.env`)
//! carries a `dp.st.` (Service) token for `WILLIKINS_DOPPLER_TOKEN`,
//! scoped to secrets-only access within `willikins-test/dev` — it cannot
//! provision, which is exactly what this crate's credential regex
//! (`^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$`) refuses at construction. This
//! probe is written, complete, and ready to run, but neither this
//! session nor its verifier can run it: doing so requires the operator
//! to supply a `dp.sa.` (Service Account) or `dp.pt.` (Personal) token.
//! Every fixture under `fixtures/doppler/` therefore stays marked
//! "entirely unverified" in that directory's `README.md` until someone
//! runs:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_PROBE=1 \
//!   cargo test -p willikins-providers-doppler --test live_probe -- --ignored \
//!   --nocapture
//! ```
//!
//! (`--nocapture` because `cargo test` otherwise swallows a passing
//! test's `println!` output, and this test's whole point is those
//! pass/skip/fail lines.)
//!
//! Performs only `GET`s: the project, its environments, its configs, the
//! `dev` config's service-token list, and (best-effort — see
//! [`well_known_secret_name`]) one secret of the `dev` config. Each
//! response is written, redacted, to `fixtures/doppler/live/<name>.json`
//! and compared by top-level key set against the authored fixture of the
//! same endpoint. Also records, for the milestone plan's "Verify before
//! relying on them" item 4, what a real `404`'s error body shape and the
//! environment list's slug character classes look like — read-only
//! observations the plan names as still open.
//!
//! The endpoints for *listing* environments and configs
//! (`GET /v3/environments?project=`, `GET /v3/configs?project=`) are not
//! quoted anywhere in the research note (which only fetched the
//! create/get-one pages for each) — they are inferred from Doppler's own
//! REST convention that a plural resource path with no further segment
//! lists, the same convention `GET /v3/configs/config/tokens` (this
//! crate's own, quoted and load-bearing) already follows for tokens.
//! Confirm both exist, and their response shapes, the first time this
//! probe actually runs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::ProviderError;
use willikins_providers_http::testing::load_fixture;
use willikins_types::{DomainType, DopplerProject};

/// Field names redacted, recursively and by name alone (regardless of
/// nesting), from every value this probe writes to disk or compares.
/// `raw` and `computed` are redacted as individual leaf fields — not by
/// replacing their parent `value` object wholesale — so a secret
/// response's `{raw, computed, note}` sub-shape survives the key-set
/// comparison intact; only the two fields capable of carrying a real
/// secret's bytes are blanked.
const REDACTED_FIELD_NAMES: &[&str] = &[
    "token",
    "key",
    "secret",
    "password",
    "client_secret",
    "raw",
    "computed",
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
    fixtures_dir().join("doppler").join("live")
}

fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Record `live` (redacted) under `name.json` and compare its top-level
/// key set against `fixtures/doppler/<name>.json`'s — a subset check
/// (every key the authored fixture names must be present in the live
/// response), the same rule `willikins-providers-github`'s probe uses and
/// for the same reason: failing on an unauthored *extra* field would make
/// this brittle to Doppler adding fields, which is not "the shape
/// drifted" in any sense this crate's tools care about.
fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/doppler/live/");
    let redacted = redact(live.clone());
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redacted).expect("serializes"),
    )
    .expect("can write the recorded fixture");

    let authored = load_fixture(&fixtures_dir(), "doppler", name);
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
/// `display_name` (a path *template*, never the real project), and push
/// a detail string onto `failures` on either kind of failure.
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

/// `GET /v3/environments?project=<project>` and `GET
/// /v3/configs?project=<project>`: recorded for inspection (redacted,
/// under `fixtures/doppler/live/`) with no authored fixture to compare
/// against — this crate reads neither list's body beyond membership —
/// and, for environments, prints the observed slugs for the plan's
/// "Verify before relying on them" item 4.
fn record_list_endpoint(
    http: &willikins_providers_http::Http,
    display_name: &str,
    path: &str,
    live_name: &str,
    failures: &mut Vec<String>,
) -> Option<Value> {
    match http.get::<Value>(path) {
        Ok(body) => {
            std::fs::create_dir_all(live_dir()).expect("can create fixtures/doppler/live/");
            std::fs::write(
                live_dir().join(format!("{live_name}.json")),
                serde_json::to_string_pretty(&redact(body.clone())).expect("serializes"),
            )
            .expect("can write the recorded fixture");
            println!("GET {display_name}: pass");
            Some(body)
        }
        Err(err) => {
            println!("GET {display_name}: fail");
            failures.push(format!("{display_name}: request failed: {err}"));
            None
        }
    }
}

/// One secret of the `dev` config, by [`well_known_secret_name`]. A `404`
/// (the well-known name is not present for some reason) is a skip, not a
/// failure — this probe does not require the operator to pre-seed a
/// secret.
fn check_secret(
    http: &willikins_providers_http::Http,
    project: &DopplerProject,
    failures: &mut Vec<String>,
) {
    let path = format!(
        "/v3/configs/config/secret?project={project}&config=dev&name={}",
        well_known_secret_name()
    );
    match http.get::<Value>(&path) {
        Ok(body) => match record_and_compare("secret_get", &body) {
            Ok(()) => println!("GET /v3/configs/config/secret: pass"),
            Err(diff) => {
                println!("GET /v3/configs/config/secret: fail");
                failures.push(format!("/v3/configs/config/secret: {diff}"));
            }
        },
        Err(err) if err.status == Some(404) => {
            println!(
                "GET /v3/configs/config/secret: skip (no `{}` secret found)",
                well_known_secret_name()
            );
        }
        Err(err) => {
            println!("GET /v3/configs/config/secret: fail");
            failures.push(format!("/v3/configs/config/secret: request failed: {err}"));
        }
    }
}

/// The plan's "Verify before relying on them" item 4: the Doppler
/// error-body shape, observed from a `404` on a project that cannot
/// exist. [`ProviderError`]'s own construction already drops the raw
/// body (trust boundary 5), so what this can observe is only whether the
/// shared client found a `messages` array to label (`provider says:
/// ...`) or found nothing recognisable (`provider returned status 404`)
/// — never the body itself.
fn check_missing_project_error_shape(
    http: &willikins_providers_http::Http,
    failures: &mut Vec<String>,
) {
    match http.get::<Value>("/v3/projects/project?project=willikins-probe-does-not-exist") {
        Ok(_) => {
            println!("GET /v3/projects/project (missing): fail");
            failures.push("expected 404 for a nonexistent project".to_string());
        }
        Err(err) if err.status == Some(404) => {
            let shape = if err.message.starts_with("provider says: ") {
                "a `messages` array was present"
            } else {
                "no recognisable message field was present"
            };
            println!("GET /v3/projects/project (missing): pass (404 error body shape: {shape})");
        }
        Err(err) => {
            println!("GET /v3/projects/project (missing): fail");
            failures.push(format!("unexpected status {:?}", err.status));
        }
    }
}

/// A secret name virtually guaranteed to exist in any Doppler config:
/// Doppler auto-injects `DOPPLER_PROJECT`, `DOPPLER_CONFIG`, and
/// `DOPPLER_ENVIRONMENT` into every config's secrets. Chosen over
/// requiring the operator to pre-seed a real application secret, and
/// documented rather than assumed silently.
fn well_known_secret_name() -> &'static str {
    "DOPPLER_PROJECT"
}

#[test]
#[ignore = "opt-in live probe against a real Doppler project; run with WILLIKINS_LIVE_PROBE=1 \
            and a sandbox dp.sa. or dp.pt. token sourced in the same command — the sandbox \
            env file on hand as of 2026-09-14 carries only a dp.st. token, which this \
            crate's own credential regex refuses, so this test has never run"]
fn doppler_live_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }

    let credential =
        willikins_providers_doppler::credential_from_env().expect("a valid sandbox Doppler token");
    let project_value = std::env::var("WILLIKINS_SANDBOX_DOPPLER_PROJECT")
        .expect("WILLIKINS_SANDBOX_DOPPLER_PROJECT is set");
    let project = DopplerProject::parse(&project_value).expect("a valid Doppler project slug");
    let http = willikins_providers_doppler::http_client(credential);

    let mut failures = Vec::new();

    check(
        "/v3/projects/project",
        "project_get_present",
        http.get::<Value>(&format!("/v3/projects/project?project={project}")),
        &mut failures,
    );

    if let Some(body) = record_list_endpoint(
        &http,
        "/v3/environments",
        &format!("/v3/environments?project={project}"),
        "environments_list",
        &mut failures,
    ) {
        // Plan "Verify before relying on them" item 4: the environment
        // slug's character class, read off whatever slugs this
        // project's own environments actually use.
        if let Some(list) = body.get("environments").and_then(Value::as_array) {
            let slugs: Vec<_> = list
                .iter()
                .filter_map(|env| env.get("id").and_then(Value::as_str))
                .collect();
            println!("environment slugs observed: {slugs:?}");
        }
    }

    record_list_endpoint(
        &http,
        "/v3/configs",
        &format!("/v3/configs?project={project}"),
        "configs_list",
        &mut failures,
    );

    check(
        "/v3/configs/config/tokens",
        "service_tokens_list_present",
        http.get::<Value>(&format!(
            "/v3/configs/config/tokens?project={project}&config=dev"
        )),
        &mut failures,
    );

    check_secret(&http, &project, &mut failures);
    check_missing_project_error_shape(&http, &mut failures);

    assert!(failures.is_empty(), "live probe failures: {failures:?}");
}
