//! The operator's own acknowledgement: the type behind a gate whose
//! condition no provider API exposes, so the only way to observe it is to
//! ask.
//!
//! See `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, decision (j),
//! point 6 ("Operator acknowledgement (G3)").

/// An operator's plain acknowledgement that a manual step is done.
///
/// The grammar admits exactly one value, `done`, so supplying it is a
/// deliberate act: there is no partial or near-miss spelling that parses,
/// and no tool ever outputs one -- it can only arrive as a workflow input
/// the operator themselves typed.
///
/// This type is never defaulted and never a literal: `willikins-core`'s
/// `check` refuses both, recognising the type by its registry entry's
/// `TypeId` (never by comparing [`crate::DomainType::TYPE_NAME`] strings —
/// the milestone 3d rule, `docs/research/2026-09-23-m3d-adversarial-pass.md`).
/// An unsupplied input of this type is *awaited*, not missing: `describe`
/// lists it under `awaiting`, and `plan` resolves it to `Value::unknown`
/// rather than refusing the plan, so a gate reading it can report
/// `Action::Blocked` with a `--input name=done` the operator can supply on
/// the next run.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "done",
    description = "An operator's acknowledgement that a manual step is done (the only valid value is `done`); never defaulted, never a literal.",
    example = "done"
)]
pub struct OperatorAcknowledgement(String);

#[cfg(test)]
mod tests {
    use super::OperatorAcknowledgement;
    use crate::DomainType;

    #[test]
    fn accepts_exactly_done() {
        assert!(OperatorAcknowledgement::parse("done").is_ok());
    }

    #[test]
    fn refuses_every_near_miss() {
        for other in ["Done", "DONE", "", "done ", " done", "donee", "do", "ne"] {
            assert!(
                OperatorAcknowledgement::parse(other).is_err(),
                "expected `{other}` to be refused"
            );
        }
    }

    #[test]
    fn is_not_secret_and_serializes_as_plain_json() {
        // Not `assert!(!OperatorAcknowledgement::IS_SECRET)`: that is a
        // compile-time constant and clippy refuses it outright. Serializing
        // to plain JSON is the observable behaviour a secret type's
        // `Serialize` impl never has (a secret prints a redaction marker
        // instead), so this proves the same fact through what a caller
        // would actually see.
        let value = OperatorAcknowledgement::parse("done").unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), "\"done\"");
    }

    #[test]
    fn example_parses_as_its_own_type() {
        crate::assert_example_parses::<OperatorAcknowledgement>();
    }
}
