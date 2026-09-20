//! The Doppler half of the read-only live probe. `#[ignore]`, and inert
//! even under `--ignored` unless `WILLIKINS_LIVE_PROBE=1` — the credential
//! is read (through [`willikins_providers_doppler::credential_from_env`],
//! itself `Credential::from_env`) only past that gate, per
//! `docs/HANDOFF.md`'s credentials rule: nothing else in this workspace
//! makes a network call to Doppler, and this file is the one place a
//! real token's bytes exist in a test process.
//!
//! # What this probe has and has not answered
//!
//! It ran for the first time on 2026-09-14, against the operator's new
//! dedicated Doppler test workplace, with the `dp.sa.` (Service Account)
//! token that workplace's `WILLIKINS_DOPPLER_TOKEN` now carries. Until
//! that token existed the probe could not run at all: the sandbox file
//! held a `dp.st.` (Service) token, scoped to secrets-only access within
//! one config, which this crate's credential regex
//! (`^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$`) refuses at construction
//! because a service token cannot provision.
//!
//! That run authenticated and reached Doppler, and every check that
//! needs a project failed with `404`: the workplace is empty and holds
//! no project named by `WILLIKINS_SANDBOX_DOPPLER_PROJECT`
//! (`willikins-test`). Only [`check_missing_project_error_shape`], whose
//! whole point is a project that cannot exist, passed. So this probe
//! verified no fixture, and it stays here for a workplace that keeps a
//! persistent project: run it there, not against an empty one.
//!
//! The fixtures are verified instead by `tests/live_write_cycle.rs`,
//! which provisions the project it reads and deletes it again, and which
//! records the same endpoints through the same helpers
//! (`tests/common/mod.rs`). `fixtures/doppler/README.md`'s status column
//! names, per endpoint, which of the two verified it and when.
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

mod common;

use serde_json::Value;
use willikins_providers_doppler::looks_like_a_missing_project;
use willikins_providers_http::ProviderError;
use willikins_types::{DomainType, DopplerProject};

use common::{record_and_compare, record_raw};

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
            record_raw(live_name, &body);
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
/// error-body shape, observed on a project that cannot exist.
/// [`ProviderError`]'s own construction already drops the raw body
/// (trust boundary 5), so what this can observe is only whether the
/// shared client found a `messages` array to label (`provider says:
/// ...`) or found nothing recognisable (`provider returned status
/// <n>`) — never the body itself.
///
/// **2026-09-20.** This check asserted `404` outright until a live
/// rehearsal found that an absent project name answers `400` "This
/// token does not have access to requested project" instead whenever
/// the calling token can already see at least one project in the
/// workplace
/// (`docs/solutions/providers/doppler-400s-a-missing-project-when-quiescent.md`,
/// whose addendum carries the probe; its filename records the first,
/// superseded reading). Which of the two a given run meets is a fact
/// about this token's own visible project set, not about the request —
/// and since the probe's whole purpose is a workplace that *holds* a
/// persistent project, `400` is the answer it should normally expect. It asks
/// [`looks_like_a_missing_project`] the same question the five tools
/// ask, and records which status actually came back instead of
/// demanding one.
fn check_missing_project_error_shape(
    http: &willikins_providers_http::Http,
    failures: &mut Vec<String>,
) {
    match http.get::<Value>("/v3/projects/project?project=willikins-probe-does-not-exist") {
        Ok(_) => {
            println!("GET /v3/projects/project (missing): fail");
            failures.push("expected a missing-project error for a nonexistent project".to_string());
        }
        Err(err) if looks_like_a_missing_project(&err) => {
            let shape = if err.message.starts_with("provider says: ") {
                "a `messages` array was present"
            } else {
                "no recognisable message field was present"
            };
            let status = err.status;
            println!(
                "GET /v3/projects/project (missing): pass \
                 (status {status:?}, error body shape: {shape})"
            );
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
            and a sandbox dp.sa. or dp.pt. token sourced in the same command. It needs a \
            workplace that keeps a persistent project: the run of 2026-09-14 authenticated \
            but found no WILLIKINS_SANDBOX_DOPPLER_PROJECT in the empty test workplace"]
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

/// [`redact`] is the only thing standing between a real Doppler response
/// and a file on disk, and it is reached exclusively from an `#[ignore]`d
/// test that has never run — so nothing had ever executed it. This test
/// is *not* ignored: it seeds the same markers `tests/redaction.rs`
/// sweeps, in every position a real response could put them (a bare
/// object, nested inside another, inside an array, and inside an array
/// nested inside an object), and proves none survives.
#[test]
fn redact_strips_every_secret_bearing_field_at_every_depth() {
    // `concat!`-joined so this file holds no literal spelling the whole
    // thing contiguously (same marker as `tests/redaction.rs`).
    const TOKEN_MARKER: &str = concat!("dp.st.", "wlknTokenMarker0000000000000000000000000");
    const SECRET_MARKER: &str = "wlkn-secret-marker-9f2h7ap5rz8s";

    let live = serde_json::json!({
        "token": {"name": "ci", "key": TOKEN_MARKER},
        "tokens": [
            {"name": "ci", "slug": "s1", "key": TOKEN_MARKER},
            {"name": "other", "slug": "s2"},
        ],
        "name": "DATABASE",
        "value": {"raw": SECRET_MARKER, "computed": SECRET_MARKER, "note": ""},
        "nested": {"deeper": [{"secret": SECRET_MARKER, "password": SECRET_MARKER}]},
        "client_secret": SECRET_MARKER,
    });

    let redacted = common::redact(live);
    let rendered = serde_json::to_string(&redacted).expect("serializes");
    assert!(
        !rendered.contains(TOKEN_MARKER),
        "the token marker survived redaction: {rendered}"
    );
    assert!(
        !rendered.contains(SECRET_MARKER),
        "the secret marker survived redaction: {rendered}"
    );

    // Shape survives: redaction blanks content, never deletes keys, so a
    // recorded fixture still answers "which fields exist".
    assert!(redacted["value"].get("raw").is_some());
    assert!(redacted["value"].get("computed").is_some());
    assert_eq!(redacted["value"]["note"], serde_json::json!(""));
    assert_eq!(redacted["name"], serde_json::json!("DATABASE"));
    assert_eq!(redacted["tokens"][1]["slug"], serde_json::json!("s2"));
}

/// The redaction list is keyed by field *name*, so a field this crate
/// never reads but Doppler might one day add — carrying a token under a
/// name nobody listed — would pass through. Pinned as a known limit, not
/// as a guarantee: what the list does cover is every field name any
/// endpoint in the research note actually returns a secret under.
#[test]
fn redaction_is_by_field_name_and_the_covered_names_are_the_documented_ones() {
    for name in ["key", "raw", "computed", "token_preview"] {
        let live = serde_json::json!({ name: "whatever" });
        assert_eq!(
            common::redact(live)[name],
            serde_json::json!("[REDACTED]"),
            "`{name}` is a documented secret-bearing field and must be redacted"
        );
    }
}
