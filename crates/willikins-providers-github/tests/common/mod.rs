//! Helpers shared by this crate's two opt-in live tests: the read-only
//! probe (`tests/live_probe.rs`) and the write cycle
//! (`tests/live_write_cycle.rs`).
//!
//! Both record a real response body, redacted, under
//! `fixtures/github/live/` (gitignored) and compare it by top-level key
//! set against the authored fixture of the same name. The recording and
//! comparison rules live here so the two agree by construction rather
//! than by a copied function drifting.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::testing::load_fixture;

/// Field names stripped, recursively, from every value a live test writes
/// to disk or compares. None of the endpoints the read-only probe calls
/// returns anything secret (GitHub's own schemas confirm this: `/user`,
/// `/orgs/{org}`, a repository, and the Actions public key — which is,
/// itself, public), but the redaction step exists here so a probe that
/// does call an endpoint capable of carrying a secret value copies this
/// exact shape rather than inventing its own.
pub const REDACTED_FIELD_NAMES: &[&str] = &[
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
pub fn redact(value: Value) -> Value {
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

/// This crate's `fixtures/` directory.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// `fixtures/github/live/`, where a live test's own recordings go. Never
/// committed (`.gitignore` names it): a recording carries the sandbox
/// org's and the operator's identity.
pub fn live_dir() -> PathBuf {
    fixtures_dir().join("github").join("live")
}

/// The top-level keys of a JSON object, empty for anything else.
pub fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Record `live` (redacted) under `name.json`, compare its top-level key
/// set against `fixtures/github/<name>.json`'s, and report `Ok` or the
/// difference (never panicking on a mismatch by itself — the caller
/// decides whether a given endpoint's drift should fail the run).
///
/// The comparison is a subset check — every key the authored fixture
/// names must be present in the live response — not exact equality. A
/// real GitHub response carries dozens of fields no fixture here
/// authors (`user.json` and `org.json` most starkly: they name only a
/// handful of well-known fields, not the full schema); failing on an
/// unauthored *extra* field would make a live test brittle to GitHub
/// adding fields, which is not "the shape drifted" in any sense this
/// crate's tools care about. A field this crate actually reads
/// (`repo_get_present.json`'s `visibility`/`topics`, both authored) going
/// *missing* from a live response is exactly the drift worth failing on.
pub fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
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
