//! The approval-class of a tool or a plan: how much a person should trust
//! it to run unattended.

/// How much a tool's `ensure` should be trusted to run unattended.
///
/// Ordered `Reversible < Irreversible < Destructive`, so
/// [`Class::max_of`] over every node in a plan gives the plan's own class.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Can be undone by another `ensure` call: creating a resource that can
    /// simply be deleted again.
    Reversible,
    /// Cannot be trivially undone, but is not destructive: minting a
    /// service token that cannot be re-read once issued, say.
    Irreversible,
    /// Destroys or overwrites something. The highest class.
    Destructive,
}

impl Class {
    /// Whether a plan or tool of this class should require human approval
    /// before running: true for anything above [`Class::Reversible`].
    #[must_use]
    pub fn requires_approval(self) -> bool {
        self > Self::Reversible
    }

    /// The maximum class over `classes`, or [`Class::Reversible`] when
    /// `classes` is empty — the class of a plan with no nodes, or of a plan
    /// whose nodes are all pure (pure nodes are excluded from this
    /// computation by the caller).
    #[must_use]
    pub fn max_of(classes: impl IntoIterator<Item = Self>) -> Self {
        classes.into_iter().max().unwrap_or(Self::Reversible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_reversible_below_irreversible_below_destructive() {
        assert!(Class::Reversible < Class::Irreversible);
        assert!(Class::Irreversible < Class::Destructive);
    }

    #[test]
    fn only_reversible_does_not_require_approval() {
        assert!(!Class::Reversible.requires_approval());
        assert!(Class::Irreversible.requires_approval());
        assert!(Class::Destructive.requires_approval());
    }

    #[test]
    fn max_of_empty_is_reversible() {
        assert_eq!(Class::max_of(std::iter::empty()), Class::Reversible);
    }

    #[test]
    fn max_of_picks_the_highest_class() {
        assert_eq!(
            Class::max_of([Class::Reversible, Class::Destructive, Class::Irreversible]),
            Class::Destructive
        );
    }

    #[test]
    fn serializes_as_a_lowercase_string() {
        assert_eq!(
            serde_json::to_string(&Class::Irreversible).unwrap(),
            "\"irreversible\""
        );
    }

    #[test]
    fn deserializes_from_a_lowercase_string() {
        let class: Class = serde_json::from_str("\"destructive\"").unwrap();
        assert_eq!(class, Class::Destructive);
    }
}
