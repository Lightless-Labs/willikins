//! Structural YAML comparison, shared by every tool in this crate that
//! must decide whether a *stored* Buildkite pipeline configuration equals
//! a bootstrap the calling document renders: `buildkite.pipeline.bootstrap.gate`
//! (read-only) and `buildkite.pipeline.bootstrap.ensure` (milestone 3i
//! decision (a3)). Moved out of `pipeline_bootstrap_gate.rs` so the two
//! tools share one answer to "equal" rather than two copies that could
//! drift apart.
//!
//! `pub(crate)`: nothing outside this crate's own tools needs it, and the
//! fake crate keeps its own independent copy (a fake never depends on its
//! live counterpart) -- `tests/fake_agrees_with_live.rs` pins the two to
//! the same answers instead of sharing code.

/// Parse `a` and `b` as YAML into a JSON value (decision (h): "both
/// strings parsed as YAML into a JSON value and compared for equality")
/// and compare for structural equality -- never byte equality, since
/// Buildkite's own documentation shows a stored configuration coming back
/// re-quoted. Either side failing to parse makes the two "different",
/// never a hard error: a stored configuration this cannot even parse is
/// not equal to anything, the same way a wrong value would be.
pub(crate) fn structurally_equal(a: &str, b: &str) -> bool {
    // A JSON map keeps a duplicate key's last value silently; YAML's own
    // value refuses a duplicate key outright. A side that repeats a key
    // is "different": which of its values Buildkite runs is Buildkite's
    // call, not this crate's (adversarial pass, Sample group, 2026-10-01).
    if [a, b]
        .iter()
        .any(|side| serde_yaml_ng::from_str::<serde_yaml_ng::Value>(side).is_err())
    {
        return false;
    }
    let parsed_a = serde_yaml_ng::from_str::<serde_json::Value>(a);
    let parsed_b = serde_yaml_ng::from_str::<serde_json::Value>(b);
    matches!((parsed_a, parsed_b), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structurally_equal_ignores_quoting_differences() {
        assert!(structurally_equal(
            "steps:\n  - command: \"echo hi\"\n",
            "steps:\n  - command: 'echo hi'\n"
        ));
    }

    /// Adversarial pass (Sample group, 2026-10-01): parsed straight into
    /// a JSON map, a duplicate key silently keeps its last value, so a
    /// stored configuration naming `command` twice compared equal to the
    /// document's single `command` -- although Buildkite, not this
    /// function, decides which one runs. A configuration with a
    /// duplicate key is "different", never equal, on either side.
    #[test]
    fn structurally_equal_is_false_when_either_side_repeats_a_key() {
        let rendered = "steps:\n  - command: \"bash upload.sh\"\n";
        let repeated = "steps:\n  - command: \"echo other\"\n    command: \"bash upload.sh\"\n";
        assert!(!structurally_equal(repeated, rendered));
        assert!(!structurally_equal(rendered, repeated));
        assert!(!structurally_equal(
            "{\"steps\": [], \"steps\": []}",
            "{\"steps\": []}"
        ));
    }

    #[test]
    fn structurally_equal_is_false_when_content_differs() {
        assert!(!structurally_equal(
            "steps:\n  - command: \"echo hi\"\n",
            "steps:\n  - command: \"echo bye\"\n"
        ));
    }

    #[test]
    fn structurally_equal_is_false_when_either_side_fails_to_parse() {
        assert!(!structurally_equal(
            "not: valid: yaml: at: all:",
            "steps:\n  - command: \"echo hi\"\n"
        ));
        assert!(!structurally_equal(
            "steps:\n  - command: \"echo hi\"\n",
            "not: valid: yaml: at: all:"
        ));
    }
}
