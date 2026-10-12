//! Teardown: how willikins reverses what one run created.
//!
//! A tool's [`Reversal`] declares only **capability** — what `ensure`'s
//! effect can be undone, and how — never policy: no field of it says
//! whether a run's own teardown should delete anything. A document's own
//! node decides that for itself (decision 3a,
//! `docs/plans/2026-10-05-milestone-3m-safe-rollback.md`'s `teardown:`
//! field, milestone 3m task T1). [`crate::catalog::Catalog::insert`]
//! validates a `Reversal` declaration on the tool's own side only, the
//! same way it validates a [`crate::tool::Gate`] (milestone 3m task R1).
//!
//! The rest of this module — the `Inverse` trait, `Created`, `Standing`,
//! `plan_teardown`, `apply_teardown` — lands in later milestone 3m tasks
//! (R3, R5a, R5b, R6), against this file.

use std::sync::LazyLock;

/// The pattern every [`Reversal::Delete`] loss name must match: unlike
/// [`crate::tool::PortName`], no digit — a loss is a fixed English word
/// ("builds"), never a generated identifier.
const LOSS_NAME_PATTERN: &str = "^[a-z][a-z_]*$";

pub(crate) static LOSS_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(LOSS_NAME_PATTERN).expect("LOSS_NAME_PATTERN is valid"));

/// [`Reversal::Retained`]'s `why` when a tool declares no reversal at all
/// — [`crate::tool::Tool::reversal`]'s default.
pub const UNDECLARED: &str = "this tool declares no reversal";

/// Which side of a [`Within`]'s container tool its mapped port is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The container's input port.
    Input,
    /// The container's output port.
    Output,
}

/// What a [`Reversal::Delete`] or [`Reversal::Contained`] instance rides
/// out with: the container it goes with when the container itself is
/// deleted (milestone 3m decision 3).
///
/// `tool` names the container by tool name. Each entry of `ports` is
/// `(own_port, side, container_port)`: `own_port` must name one of this
/// tool's own input ports — [`crate::catalog::Catalog::insert`] enforces
/// that, on this tool's own side. `container_port` names a port of `tool`,
/// on the stated `side`; that container tool exists, has that port, and
/// the two ports' types agree is checked elsewhere (milestone 3m's
/// declaration table test over `LIVE_TOOL_NAMES`, and the fake-catalog
/// parity test), never by `Catalog::insert`, which sees one tool at a
/// time. Riding holds when every pair's recorded values are typed-equal
/// in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Within {
    /// The container's tool name.
    pub tool: &'static str,
    /// `(own_port, side, container_port)` triples; see the struct doc.
    pub ports: &'static [(&'static str, Side, &'static str)],
}

/// A tool's declared reversal: what `ensure`'s effect **can** be undone,
/// and how — capability only. Whether a run's own teardown undoes it is
/// the document's own `teardown:` policy (milestone 3m decision 3a),
/// never a field of this type: a `Reversal` never says whether to delete.
///
/// A default trait method in the manner of [`crate::tool::Tool::gate`],
/// [`crate::tool::Tool::replaces`] and [`crate::tool::Tool::updates`]: no
/// existing tool, catalog entry or document changes until a tool opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversal {
    /// `inverse()` (milestone 3m task R3) deletes this
    /// resource. `needs` names the input ports the delete needs (key
    /// ports and credential ports). `within` names the containers this
    /// instance rides out with when this run deletes one of them too.
    /// `losses` names what a delete would destroy that a run never makes
    /// (decision 3a); a node's policy may accept one of them by name.
    Delete {
        /// The input ports a delete needs; each must be one of this
        /// tool's own input ports.
        needs: &'static [&'static str],
        /// The containers this instance rides out with.
        within: &'static [Within],
        /// What a delete would destroy that a run never makes, e.g. a
        /// Buildkite pipeline's `"builds"`. Each name must match
        /// `^[a-z][a-z_]*$`.
        losses: &'static [&'static str],
    },
    /// No delete of its own: this instance goes when a container this run
    /// created goes, and is otherwise left in place, for `otherwise`.
    Contained {
        /// The containers this instance rides out with.
        within: &'static [Within],
        /// Why it is left in place when no such container is deleted.
        otherwise: &'static str,
    },
    /// Never undone by rollback, for `why`.
    Retained {
        /// Why this tool's effect cannot be undone.
        why: &'static str,
    },
}

/// Why [`crate::catalog::Catalog::insert`] refused a tool's [`Reversal`]
/// declaration (milestone 3m decision 3). Checked on the tool's own side
/// only: a [`Within`]'s container side is checked elsewhere (see that
/// type's doc), never here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReversalError {
    /// A `Reversal::Delete`'s `needs` entry does not name one of the
    /// tool's own input ports.
    #[error("reversal `needs` port `{port}` is not one of this tool's own input ports")]
    NeedsNotAnInput {
        /// The offending `needs` entry.
        port: String,
    },
    /// A `Within`'s own-side port (the left side of the pair) does not
    /// name one of the tool's own input ports.
    #[error("reversal `within` port `{port}` is not one of this tool's own input ports")]
    WithinNotAnInput {
        /// The offending `within` entry.
        port: String,
    },
    /// A `Within` maps no port at all. Riding holds when every mapped
    /// pair's recorded values are equal, so an empty mapping holds for
    /// every instance of the container tool: the instance would ride
    /// with, and be tolerated inside, every such container the run
    /// deletes, vouching for contents in containers it never touched.
    #[error("reversal `within` container `{tool}` maps no port")]
    WithinMapsNoPort {
        /// The container tool the empty `Within` names.
        tool: String,
    },
    /// A `Within`'s own-side port is secret: [`crate::value::PortType::AnySecret`]
    /// or a type the registry marks secret. The journal keeps no secret's
    /// bytes (a redaction marker, re-parsed as `Unknown`), and two
    /// `Unknown`s of one type compare equal, so a secret pair holds for
    /// any two instances: it can only widen riding, never tell two
    /// containers apart.
    #[error("reversal `within` port `{port}` is secret, so it cannot tell two instances apart")]
    WithinPortSecret {
        /// The offending `within` entry.
        port: String,
    },
    /// A `Reversal::Delete`'s `losses` entry does not match the required
    /// pattern `^[a-z][a-z_]*$`.
    #[error("reversal loss `{loss}` does not match the required pattern `^[a-z][a-z_]*$`")]
    LossNameInvalid {
        /// The offending loss name.
        loss: String,
    },
    /// A pure tool declared a `Reversal` other than the default: a pure
    /// tool has no external state, so nothing about it can be undone.
    #[error("a pure tool must not declare a reversal")]
    PureToolDeclaresReversal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undeclared_is_a_fixed_sentence() {
        assert_eq!(UNDECLARED, "this tool declares no reversal");
    }

    #[test]
    fn loss_name_regex_accepts_lowercase_words() {
        assert!(LOSS_NAME_REGEX.is_match("builds"));
        assert!(LOSS_NAME_REGEX.is_match("a_b"));
    }

    #[test]
    fn loss_name_regex_rejects_digits_and_uppercase() {
        assert!(!LOSS_NAME_REGEX.is_match("builds1"));
        assert!(!LOSS_NAME_REGEX.is_match("Builds"));
        assert!(!LOSS_NAME_REGEX.is_match(""));
        assert!(!LOSS_NAME_REGEX.is_match("_builds"));
    }
}
