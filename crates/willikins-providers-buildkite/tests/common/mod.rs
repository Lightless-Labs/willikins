//! Helpers shared by this crate's two opt-in live tests: the read-only
//! probe (`tests/live_probe.rs`) and the write cycle
//! (`tests/live_write_cycle.rs`, compiled only with the `live-tests`
//! feature). Mirrors `willikins-providers-doppler/tests/common/mod.rs`
//! exactly, redefined for Buildkite's own credential-bearing field names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::testing::load_fixture;

/// Field names redacted, recursively and by name alone (regardless of
/// nesting), from every value a live test writes to disk or compares.
/// `webhook_url` is the credential-bearing field trust boundary 7 names
/// (a delivery URL whose path segment is the shared secret); `key`,
/// `token`, and `secret` are included defensively, the same way
/// `willikins-providers-doppler`'s own list is, even though nothing this
/// crate reads is documented to carry them.
pub const REDACTED_FIELD_NAMES: &[&str] = &["webhook_url", "key", "token", "secret"];

/// Recursively strip [`REDACTED_FIELD_NAMES`] from `value`, replacing
/// each with a fixed marker rather than deleting the key outright, so a
/// recorded fixture's shape (which fields exist) survives redaction --
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

/// `fixtures/buildkite/live/`, where a live test's own recordings go.
/// Never committed (`.gitignore` names it): a recording carries the
/// sandbox organisation's identity.
pub fn live_dir() -> PathBuf {
    fixtures_dir().join("buildkite").join("live")
}

/// The top-level keys of a JSON object, empty for anything else.
pub fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Write `live`, redacted, to `fixtures/buildkite/live/<name>.json`.
pub fn record_raw(name: &str, live: &Value) {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/buildkite/live/");
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redact(live.clone())).expect("serializes"),
    )
    .expect("can write the recorded fixture");
}

/// Record `live` (redacted) under `name.json`, compare its top-level key
/// set against `fixtures/buildkite/<name>.json`'s, and report `Ok` or the
/// difference. A subset check: every key the authored fixture names must
/// be present in the live response, so an unauthored *extra* field
/// Buildkite might add does not fail this on its own.
pub fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
    record_raw(name, live);
    let authored = load_fixture(&fixtures_dir(), "buildkite", name);
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
