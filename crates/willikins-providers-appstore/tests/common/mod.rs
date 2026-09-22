//! Helpers shared by this crate's two opt-in live tests: the read-only
//! probe (`tests/live_probe.rs`) and the write cycle
//! (`tests/live_write_cycle.rs`, compiled only with the `live-tests`
//! feature). Mirrors `willikins-providers-buildkite/tests/common/mod.rs`.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Field names redacted, recursively and by name alone, from every value
/// a live test writes to disk. Nothing this crate reads is documented to
/// carry a credential-bearing field (unlike Buildkite's `webhook_url`),
/// but `key`, `token`, and `secret` are kept defensively, the same
/// reason `willikins-providers-buildkite`'s own list keeps them.
pub const REDACTED_FIELD_NAMES: &[&str] = &["key", "token", "secret"];

/// Recursively strip [`REDACTED_FIELD_NAMES`] from `value`.
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

/// `fixtures/appstore/live/`, where a live test's own recordings go.
/// Never committed (`.gitignore` names it): a recording carries the
/// operator's own team's identifiers.
pub fn live_dir() -> PathBuf {
    fixtures_dir().join("appstore").join("live")
}

/// Write `live`, redacted, to `fixtures/appstore/live/<name>.json`.
pub fn record_raw(name: &str, live: &Value) {
    std::fs::create_dir_all(live_dir()).expect("can create fixtures/appstore/live/");
    std::fs::write(
        live_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redact(live.clone())).expect("serializes"),
    )
    .expect("can write the recorded fixture");
}
