//! Helpers shared by this crate's opt-in live tests. Mirrors
//! `willikins-providers-buildkite/tests/common/mod.rs` exactly, redefined
//! for `SigNoz`'s own credential-bearing field names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use willikins_providers_http::testing::load_fixture;

/// Field names redacted, recursively and by name alone, from every value
/// a live test writes to disk or compares. `value` is the ingestion
/// key's own minted secret; `key`, `token`, and `secret` are included
/// defensively, matching every sibling crate's own list.
pub const REDACTED_FIELD_NAMES: &[&str] = &["value", "key", "token", "secret"];

/// Recursively strip [`REDACTED_FIELD_NAMES`] from `value`, replacing
/// each with a fixed marker rather than deleting the key outright, so a
/// recorded fixture's shape survives redaction — only its content is
/// removed.
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

/// `fixtures/signoz/live/`, where a live test's own recordings go. Never
/// committed (`.gitignore` names it): a recording carries the operator's
/// own account identity, and — before redaction — the account's real
/// ingestion key values.
pub fn live_dir() -> PathBuf {
    fixtures_dir().join("signoz").join("live")
}

/// The top-level keys of a JSON object, empty for anything else.
pub fn top_level_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Write `live`, redacted, to `fixtures/signoz/live/<name>.json`.
pub fn record_raw(name: &str, live: &Value) {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/signoz/live/");
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redact(live.clone())).expect("serializes"),
    )
    .expect("can write the recorded fixture");
}

/// Record `live` (redacted) under `name.json`, compare its top-level key
/// set against `fixtures/signoz/<name>.json`'s, and report `Ok` or the
/// difference. A subset check: every key the authored fixture names must
/// be present in the live response.
pub fn record_and_compare(name: &str, live: &Value) -> Result<(), String> {
    record_raw(name, live);
    let authored = load_fixture(&fixtures_dir(), "signoz", name);
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
