//! Helpers shared by this crate's two opt-in live tests: the read-only
//! probe (`tests/live_probe.rs`) and the write cycle
//! (`tests/live_write_cycle.rs`, compiled only with the `live-tests`
//! feature).
//!
//! Both record a real response body, redacted, under
//! `fixtures/doppler/live/` (gitignored) and compare it by top-level key
//! set against the authored fixture of the same name. The recording and
//! comparison rules live here so the two agree by construction rather
//! than by a copied function drifting — the same arrangement
//! `willikins-providers-github`'s `tests/common/mod.rs` uses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::testing::load_fixture;

/// Field names redacted, recursively and by name alone (regardless of
/// nesting), from every value a live test writes to disk or compares.
///
/// `raw` and `computed` are redacted as individual leaf fields — not by
/// replacing their parent `value` object wholesale — so a secret
/// response's `{raw, computed, note}` sub-shape survives the key-set
/// comparison intact; only the two fields capable of carrying a real
/// secret's bytes are blanked.
pub const REDACTED_FIELD_NAMES: &[&str] = &[
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

/// `fixtures/doppler/live/`, where a live test's own recordings go. Never
/// committed (`.gitignore` names it): a recording carries the sandbox
/// workplace's and the operator's identity.
pub fn live_dir() -> PathBuf {
    fixtures_dir().join("doppler").join("live")
}

/// The top-level keys of a JSON object, empty for anything else.
pub fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Write `live`, redacted, to `fixtures/doppler/live/<name>.json`.
///
/// Used on its own for an endpoint with no authored fixture to compare
/// against (the environment and config listings), and by
/// [`record_and_compare`] for the rest.
pub fn record_raw(name: &str, live: &Value) {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/doppler/live/");
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redact(live.clone())).expect("serializes"),
    )
    .expect("can write the recorded fixture");
}

/// Record `live` (redacted) under `name.json`, compare its top-level key
/// set against `fixtures/doppler/<name>.json`'s, and report `Ok` or the
/// difference (never panicking on a mismatch by itself — the caller
/// decides whether a given endpoint's drift should fail the run).
///
/// The comparison is a subset check — every key the authored fixture
/// names must be present in the live response — not exact equality, the
/// same rule `willikins-providers-github`'s live tests use and for the
/// same reason: failing on an unauthored *extra* field would make a live
/// test brittle to Doppler adding fields, which is not "the shape
/// drifted" in any sense this crate's tools care about. A field this
/// crate actually reads going *missing* from a live response is exactly
/// the drift worth failing on.
pub fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
    record_raw(name, live);

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
