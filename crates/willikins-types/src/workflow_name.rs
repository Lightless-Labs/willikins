//! `WorkflowName`: a workflow document's own name.

/// A workflow's name: lowercase, hyphen-separated, at most 64 characters —
/// the same slug grammar `naming::v1` uses for a derived provider name.
///
/// `Document::name` (see `willikins-dsl`) parses into this type instead of
/// a bare `String`, so an agent-facing surface can never be made to echo
/// an unbounded value through a workflow's own name. Before this type
/// existed, a 200,000-character `name:` was echoed verbatim by `plan
/// --json` (see `docs/research/2026-09-12-e2e-adversarial-pass-2.md`,
/// plan defect 4).
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z][a-z0-9]*(-[a-z0-9]+)*",
    max_len = 64,
    description = "A workflow's name: lowercase, hyphen-separated, at most 64 characters.",
    example = "new-rust-service"
)]
pub struct WorkflowName(String);

// `#[derive(DomainType)]` emits `Clone`/`PartialEq`/`Eq`/`Debug` only (see
// `willikins-derive`'s `structural_eq_and_clone`), never an ordering --
// most domain types have no use for one, and the macro must not hand
// `Ord` to a *secret* storage, where even a constant-time comparison
// would leak length through timing. `WorkflowName` is never secret (it
// names a document, not a value), and milestone 2b's `PlanRecorded.used`
// (decision (d10), willikins-journal) is keyed on it in a `BTreeMap`,
// which needs `Ord` on the key. Implemented here by hand, delegating to
// the same field `PartialEq` already compares, rather than widening the
// macro for every domain type at once.
impl PartialOrd for WorkflowName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WorkflowName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DomainObject, DomainType};

    #[test]
    fn accepts_a_single_word() {
        assert_eq!(WorkflowName::parse("demo").unwrap().as_str(), "demo");
    }

    #[test]
    fn accepts_hyphenated_words() {
        assert_eq!(
            WorkflowName::parse("new-rust-service").unwrap().as_str(),
            "new-rust-service"
        );
    }

    #[test]
    fn accepts_digits_after_the_first_letter() {
        assert_eq!(
            WorkflowName::parse("s3-bucket").unwrap().as_str(),
            "s3-bucket"
        );
    }

    #[test]
    fn rejects_a_leading_digit() {
        assert!(WorkflowName::parse("1-service").is_err());
    }

    #[test]
    fn rejects_uppercase() {
        assert!(WorkflowName::parse("New-Rust-Service").is_err());
    }

    #[test]
    fn rejects_underscores() {
        assert!(WorkflowName::parse("new_rust_service").is_err());
    }

    #[test]
    fn rejects_a_leading_hyphen() {
        assert!(WorkflowName::parse("-service").is_err());
    }

    #[test]
    fn rejects_a_trailing_hyphen() {
        assert!(WorkflowName::parse("service-").is_err());
    }

    #[test]
    fn rejects_a_doubled_hyphen() {
        assert!(WorkflowName::parse("new--service").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(WorkflowName::parse("").is_err());
    }

    #[test]
    fn rejects_a_path_traversal_segment() {
        // A workflow name is used to select a document from a trusted
        // directory (milestone 2's MCP surface); the grammar alone must
        // refuse `.` and `/`, with no separate check relied on.
        assert!(WorkflowName::parse("../x").is_err());
        assert!(WorkflowName::parse("a/b").is_err());
    }

    #[test]
    fn accepts_exactly_64_characters() {
        // "a" then 62 more characters split into two hyphenated groups so
        // the pattern (not just the length check) is exercised at the
        // boundary: "a" + "-" + 31 chars + "-" + 30 chars = 64.
        let at_limit = format!("a-{}-{}", "b".repeat(31), "c".repeat(30));
        assert_eq!(at_limit.chars().count(), 64);
        assert!(WorkflowName::parse(&at_limit).is_ok());
    }

    #[test]
    fn rejects_65_characters() {
        let too_long = format!("a-{}-{}", "b".repeat(31), "c".repeat(31));
        assert_eq!(too_long.chars().count(), 65);
        assert!(WorkflowName::parse(&too_long).is_err());
    }

    #[test]
    fn a_length_rejection_does_not_echo_the_input() {
        let too_long = "a".repeat(65);
        let err = WorkflowName::parse(&too_long).unwrap_err();
        assert!(!err.reason.contains(&too_long));
        assert!(err.reason.contains("64"));
    }

    #[test]
    fn implements_domain_object_via_the_macro() {
        let value: Box<dyn DomainObject> = Box::new(WorkflowName::parse("demo").unwrap());
        assert_eq!(value.type_name(), "WorkflowName");
        assert!(!value.is_secret());
    }

    #[test]
    fn serde_round_trips() {
        let name = WorkflowName::parse("demo").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"demo\"");
        assert_eq!(serde_json::from_str::<WorkflowName>(&json).unwrap(), name);
    }

    #[test]
    fn json_schema_carries_pattern_and_max_length() {
        let schema = serde_json::to_value(WorkflowName::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["maxLength"], 64);
        assert!(schema["pattern"].as_str().unwrap().contains("a-z"));
    }

    #[test]
    fn example_parses() {
        crate::assert_example_parses::<WorkflowName>();
    }

    #[test]
    fn orders_by_its_text_so_a_btreemap_can_key_on_it() {
        let a = WorkflowName::parse("example-org").unwrap();
        let b = WorkflowName::parse("new-rust-service").unwrap();
        assert!(a < b, "lexicographic order must hold on the inner string");

        let map = std::collections::BTreeMap::from([(b.clone(), 2), (a.clone(), 1)]);
        let names: Vec<&WorkflowName> = map.keys().collect();
        assert_eq!(
            names,
            vec![&a, &b],
            "a BTreeMap keyed on it iterates sorted"
        );
    }
}
