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
//! [`Inverse`], [`Created`], [`Standing`] and [`Deleted`] (milestone 3m
//! task R3) are the shape an inverse works with; `plan_teardown` and
//! `apply_teardown` — lands in later milestone 3m tasks (R5a, R5b, R6) —
//! against this file. `Created` is still missing two fields decision 3's
//! design names (`window`, `riders`): their types do not exist yet
//! (`CreationWindow` and `Rider` are task R5a/R5b's). `accept` (whose type,
//! [`crate::workflow::Loss`], task T1 adds to `workflow.rs`) is now
//! present. See the plan's 2026-10-12 addendum (R3) for which task adds
//! which field; `Inverse::delete`'s documented contract already states the
//! rule those fields will carry, so their arrival changes no contract,
//! only the type that can express it.

use std::sync::LazyLock;

use willikins_types::TeardownToken;

/// The pattern every [`Reversal::Delete`] loss name must match: unlike
/// [`crate::tool::PortName`], no digit — a loss is a fixed English word
/// ("builds"), never a generated identifier. `pub(crate)` so
/// [`crate::workflow::Loss`] (milestone 3m task T1) can build on the same
/// pattern rather than duplicating it — a node's accepted losses must
/// never drift from the names a tool's `Reversal::Delete::losses` can
/// actually declare.
pub(crate) const LOSS_NAME_PATTERN: &str = "^[a-z][a-z_]*$";

pub(crate) static LOSS_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(LOSS_NAME_PATTERN).expect("LOSS_NAME_PATTERN is valid"));

/// [`Reversal::Retained`]'s `why` when a tool declares no reversal at all
/// — [`crate::tool::Tool::reversal`]'s default.
pub const UNDECLARED: &str = "this tool declares no reversal";

/// [`TeardownAction::Kept`]'s `why` under a node's own default policy
/// (milestone 3m decision 3a): the document declared no `teardown:` at
/// all, so the conservative default — retain — applies. A rider of a kept
/// container gets a different, R5b-owned reason instead (decision 3a).
pub const KEPT: &str = "the document keeps it (no teardown: delete)";

/// How many seconds either side of an instance's own recorded
/// `NodeStarted.at`/`NodeFinished.at` still count as this run's own doing
/// (milestone 3m decision 2): provider clocks and this host's clock are
/// independent, two minutes is wider than any skew a synced clock shows,
/// and narrower than the gap between two human-paced runs.
pub const CREATION_SKEW: i64 = 120;

/// One instance's own creation window (milestone 3m decision 2):
/// `[NodeStarted.at − `[`CREATION_SKEW`]`, NodeFinished.at + `[`CREATION_SKEW`]`]`.
/// Carried on [`Created`] for a `Delete` tool's own `standing` to compare
/// a resource's provider-reported creation timestamp against — that
/// comparison belongs to each tool, never to this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreationWindow {
    /// The window's own start: `NodeStarted.at − CREATION_SKEW`.
    pub start: crate::apply::Timestamp,
    /// The window's own end: `NodeFinished.at + CREATION_SKEW`.
    pub end: crate::apply::Timestamp,
}

/// `at` shifted by `seconds` (positive or negative). The one place this
/// module adds or subtracts [`CREATION_SKEW`] from a recorded
/// [`crate::apply::Timestamp`] — [`CreationWindow::start`]/`end` and
/// [`Created::settled_by`] (milestone 3m decision 2) all go through this.
///
/// `plan_teardown` is this function's first non-test caller and lands in
/// a later commit of this same task; until then only this module's own
/// tests call it, so the plain `lib` build (no `--cfg test`) would flag
/// it dead without the attribute below — self-removing the moment that
/// commit adds the real call, the same discipline milestone 3m task R4's
/// `Resolved`/`ctx` already uses (plan addendum, 2026-10-12).
#[cfg_attr(not(test), expect(dead_code))]
fn shift(at: crate::apply::Timestamp, seconds: i64) -> crate::apply::Timestamp {
    crate::apply::Timestamp::from_datetime(*at.as_datetime() + chrono::Duration::seconds(seconds))
}

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
    /// The tool declares `Reversal::Delete` but [`crate::tool::Tool::inverse`]
    /// returns `None`: `Delete` promises a working `inverse()`, so
    /// nothing can ever carry it out.
    #[error("reversal declares `Delete` but `Tool::inverse` returns `None`")]
    DeleteWithoutInverse,
    /// [`crate::tool::Tool::inverse`] returns `Some`, but the tool's
    /// reversal is not `Delete`: an inverse with nothing to call it would
    /// be dead code, and a future `Contained`/`Retained` tool that grows
    /// one without updating its `Reversal` would silently never run it.
    #[error("`Tool::inverse` returns `Some` but reversal does not declare `Delete`")]
    InverseWithoutDelete,
}

/// What an [`Inverse::standing`] call found when it looked at the
/// resource [`Created`] describes, before any delete (milestone 3m
/// decision 3). Carries no secret and no arbitrary value (trust boundary
/// 5): a [`Standing::NotOurs`]/[`Standing::Drifted`] reason is always a
/// fixed `&'static str`, the same discipline as [`crate::tool::Gate::need`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The resource still looks like what this run created: at the run's
    /// recorded key and non-key values, inside its creation window.
    /// `losing` names every declared [`Reversal::Delete`] loss (e.g. a
    /// Buildkite pipeline's `"builds"`) the resource currently reports —
    /// empty when it reports none. Whether a delete may proceed despite a
    /// non-empty `losing` is the node's own accepted losses, decided by
    /// the caller (milestone 3m decision 3a), never by this type.
    AsCreated {
        /// The declared losses this resource currently reports, each one
        /// of this tool's own `Reversal::Delete::losses` names.
        losing: Vec<&'static str>,
    },
    /// The resource no longer exists: deleting it is a no-op.
    Gone,
    /// A resource exists at this natural key, but it is not the one this
    /// run created — created outside the run's window, or someone
    /// else's.
    NotOurs {
        /// Why it is not this run's, fixed and non-secret.
        why: &'static str,
    },
    /// The resource exists and is ours, but it has changed since this run
    /// created it in a way a delete would silently destroy (a push, a
    /// hand-set secret, an inheriting config).
    Drifted {
        /// What changed, fixed and non-secret.
        what: &'static str,
    },
}

/// What [`Inverse::delete`] reports after deleting the resource, or
/// finding it already gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deleted {
    /// Whether the resource was already gone when `delete` observed it,
    /// so nothing was actually deleted this call.
    pub already_gone: bool,
}

/// What a [`Reversal::Delete`] tool's inverse is given to work with: the
/// journal's own record of what this run created, re-resolved just
/// enough to delete it (milestone 3m decisions 2 and 5). Debug only —
/// unlike [`crate::tool::Inputs`]/[`crate::tool::Outputs`], whose own
/// `Serialize` already redacts a secret, nothing serializes a `Created`
/// directly: a teardown never journals one (trust boundary 7).
///
/// This is R3's shape, plus T1's `accept` and R5a's `window`; one more
/// field lands once milestone 3m task R5b reaches the type it needs: the
/// tolerated riders. See this module's doc and the plan's 2026-10-12
/// addendum (R3).
#[derive(Debug)]
pub struct Created<'a> {
    /// Exactly this tool's declared `Reversal::Delete::needs` ports:
    /// non-secret values taken from the journal, secret ones re-resolved
    /// (decision 5).
    pub inputs: &'a crate::tool::Inputs,
    /// The run's recorded non-secret outputs; secret ones `Unknown`.
    pub outputs: &'a crate::tool::Outputs,
    /// This instance's own creation window (milestone 3m decision 2):
    /// `[NodeStarted.at − CREATION_SKEW, NodeFinished.at + CREATION_SKEW]`.
    /// A resource whose provider-reported creation timestamp falls
    /// outside it is [`Standing::NotOurs`] — that comparison is each
    /// tool's own `standing`, never this type's.
    pub window: CreationWindow,
    /// The point after which this run's own, later change to a resource
    /// still counts as this run's doing, rather than drift (decision 2):
    /// the run's `RunFinished.at` plus the creation-window skew.
    pub settled_by: crate::apply::Timestamp,
    /// The node's own accepted losses (milestone 3m decision 3a): empty
    /// under a plain `teardown: delete`. Each entry is one of this
    /// tool's own declared `Reversal::Delete::losses` names — `check`
    /// (`CheckError::TeardownLossNotDeclared`) refuses any other name
    /// before a workflow is ever checked, so an inverse may trust that
    /// every entry here is one of its own.
    pub accept: &'a [crate::workflow::Loss],
}

/// A [`Reversal::Delete`] tool's inverse: observes a resource's
/// [`Standing`], or deletes it. Implemented beside the tool itself, never
/// registered as a catalog tool in its own right (trust boundary 1).
/// [`crate::catalog::Catalog::insert`] requires a `Delete` reversal to
/// have one, and requires a `Contained`/`Retained` reversal not to
/// (milestone 3m task R3).
pub trait Inverse: Send + Sync {
    /// Observe the resource `created` describes, without changing
    /// anything. Takes no token: a teardown plan calls this before
    /// approval, and the only way to mint a
    /// [`willikins_types::TeardownToken`] is `apply_teardown` (trust
    /// boundary 2).
    ///
    /// # Errors
    ///
    /// Returns [`crate::tool::ToolError`] when the underlying provider
    /// cannot be queried.
    fn standing(&self, created: &Created<'_>) -> Result<Standing, crate::tool::ToolError>;

    /// Delete the resource `created` describes.
    ///
    /// # Contract
    ///
    /// An implementation observes first, the same discipline
    /// [`crate::tool::Tool::ensure`] already follows: any [`Standing`]
    /// but [`Standing::AsCreated`] is
    /// [`crate::tool::ToolErrorKind::Conflict`], so is `AsCreated` with a
    /// declared loss the node has not accepted, and [`Standing::Gone`] is
    /// `Ok(Deleted { already_gone: true })` rather than an error — a
    /// resumed teardown calls this on a step it already deleted.
    ///
    /// # Errors
    ///
    /// Returns [`crate::tool::ToolError`] on the same terms as `standing`,
    /// and as the contract above describes.
    fn delete(
        &self,
        created: &Created<'_>,
        token: &TeardownToken,
    ) -> Result<Deleted, crate::tool::ToolError>;
}

// ---------------------------------------------------------------------
// plan_teardown (milestone 3m task R5a)
// ---------------------------------------------------------------------

/// Facts about one applied run, enough for [`plan_teardown`] to compute a
/// teardown plan from. Built by the caller — `willikins-server`'s future
/// `Butler::plan_rollback` (milestone 3m task R8), from its own journal
/// replay — never read from a journal here: `willikins-core` does not
/// depend on `willikins-journal` (SHARED VALUES).
///
/// A caller only ever builds one for a run that has actually finished
/// (its own `RunFinished`): refusing an unfinished run
/// (`TeardownError::RunUnfinished`-shaped refusal, milestone 3m task R8)
/// happens before a `RunFacts` exists at all, so [`Self::finished_at`] is
/// never optional here.
#[derive(Debug, Clone)]
pub struct RunFacts {
    /// This run's own `RunFinished.at` (decision 2's `settled_by` is this
    /// plus [`CREATION_SKEW`]).
    pub finished_at: crate::apply::Timestamp,
    /// This run's own recorded workflow inputs (`PlanRecorded.inputs`,
    /// decision 5), rebuilt by the caller with `Butler`'s own
    /// `parse_recorded_value` — the same map
    /// [`crate::plan::resolve_pure`] takes as `inputs`.
    pub inputs: indexmap::IndexMap<crate::workflow::InputName, crate::value::Value>,
    /// Every instance the applied plan named, in the applied plan's own
    /// forward `Plan::nodes` order. [`plan_teardown`] walks this in
    /// reverse (decision 6) — the caller never reverses it itself.
    pub instances: Vec<RunInstance>,
}

/// Whether, and when, one instance's own `NodeFinished` landed in the
/// journal (milestone 3m decision 1's table; decision 2's window).
#[derive(Debug, Clone)]
pub enum RunProgress {
    /// No `NodeFinished` for this instance.
    NotFinished {
        /// When this instance's own `NodeStarted` landed, if it did.
        /// `Some` only for a journal write that failed mid-run (decision
        /// 1's "Unfinished" row); `None` when the run stopped before
        /// even starting it — the journal replay's own synthesized
        /// [`crate::apply::NodeStatus::NotRun`], folded by the caller
        /// into this variant rather than carried as a status: nothing
        /// was created either way.
        started: Option<crate::apply::Timestamp>,
    },
    /// This instance's own `NodeFinished` landed.
    Finished {
        /// This instance's own `NodeStarted.at`.
        started: crate::apply::Timestamp,
        /// This instance's own `NodeFinished.at`.
        finished: crate::apply::Timestamp,
        /// What happened.
        status: crate::apply::NodeStatus,
    },
}

/// One node instance's own journal facts (milestone 3m decision 1): what
/// the approved plan said to do, and what actually happened.
#[derive(Debug, Clone)]
pub struct RunInstance {
    /// The node.
    pub node: crate::workflow::NodeName,
    /// Its `for_each` instance key, if any.
    pub instance: Option<String>,
    /// The approved plan's own action for this instance.
    pub action: crate::plan::Action,
    /// Whether, and when, this instance finished.
    pub progress: RunProgress,
    /// This instance's recorded inputs (`NodeStarted.inputs`, re-parsed
    /// by each port's own declared type): a secret port's own value is
    /// `Value::unknown` here regardless — decision 5's whole point is
    /// that a teardown re-resolves a secret `needs` port itself, never
    /// from the journal, which keeps only a redaction marker for one.
    pub inputs: crate::tool::Inputs,
    /// This instance's recorded outputs (`NodeFinished.outputs`), on the
    /// same secret-is-`Unknown` terms as `inputs`. Empty for an instance
    /// with no [`RunProgress::Finished`].
    pub outputs: crate::tool::Outputs,
}

/// A computed teardown plan (milestone 3m decision 6): every candidate
/// instance this run created, in reverse applied-plan order, with the
/// action [`plan_teardown`] decided for it. Never built outside
/// `plan_teardown`'s own `Ok`: a [`TeardownError::Refused`] carries no
/// `Teardown` at all ("nothing is recorded as approvable").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teardown {
    /// Every step, in the order they would run: the reverse of the
    /// applied plan's own order.
    pub steps: Vec<TeardownStep>,
}

/// One instance's own place in a [`Teardown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownStep {
    /// The instance.
    pub instance: crate::apply::InstanceRef,
    /// What this step does.
    pub action: TeardownAction,
}

/// What one [`TeardownStep`] does (SHARED VALUES).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeardownAction {
    /// Call the tool's own `inverse().delete`. `uncertain` is set when
    /// the create that made this resource itself finished
    /// [`crate::apply::NodeStatus::Failed`] (decision 1): the create may
    /// have landed before the failure.
    Delete {
        /// Whether the create that made this resource may not actually
        /// have landed.
        uncertain: bool,
    },
    /// This instance rides out with `container`'s own delete (decision
    /// 3). Never produced by this task's own `plan_teardown` — milestone
    /// 3m task R5b computes riding; the variant exists now because
    /// [`TeardownStep`]'s own shape is SHARED VALUES across both tasks.
    RidesWith {
        /// The container instance this one rides out with.
        container: crate::apply::InstanceRef,
    },
    /// The resource no longer exists; nothing to delete.
    Gone,
    /// The document keeps it (decision 3a): never observed, so a step
    /// with this action called no `standing`.
    Kept {
        /// Why it is kept — [`KEPT`] under the node's own default policy;
        /// a rider of a kept container (milestone 3m task R5b) gets a
        /// different fixed reason.
        why: &'static str,
    },
    /// The tool itself cannot undo this: `why` is its own declared
    /// [`Reversal::Retained::why`]. Also stands in, for now, for a
    /// [`Reversal::Contained`] instance's own [`Reversal::Contained::otherwise`]
    /// — milestone 3m task R5b replaces that placeholder with a real
    /// rider decision (riding out with a container being deleted, or
    /// this same `Retained` otherwise).
    Retained {
        /// Why the tool cannot undo it.
        why: &'static str,
    },
    /// This instance is a gate the run found satisfied by hand (decision
    /// 4's "gate in reverse"): `need` is the gate's own static
    /// [`crate::tool::Gate::need`].
    MadeByHand {
        /// What the operator did by hand.
        need: &'static str,
    },
    /// This instance's own `NodeStarted` landed but its `NodeFinished`
    /// never did: no window end exists, so this is a manual check, never
    /// a delete (decision 1, 2).
    Unfinished,
}

/// Why [`plan_teardown`] could not produce an approvable [`Teardown`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum TeardownError {
    /// At least one candidate's own standing refuses the whole teardown
    /// (decision 6): no `Teardown` is ever built when this is returned.
    /// Carries every refusing instance and its reason, not only the
    /// first — trust boundary 6 observes every delete-policy candidate
    /// before any delete, plan time included.
    #[error("the teardown is refused: {steps:?}")]
    Refused {
        /// Every refusing instance and why.
        steps: Vec<(crate::apply::InstanceRef, &'static str)>,
    },
    /// A `Delete` tool's secret `needs` port is bound to something a
    /// teardown's pure resolver closure cannot honestly read: a node
    /// that is not itself pure, or (not yet supported; see this
    /// variant's own call site) the current `for_each` item (decision
    /// 5).
    #[error(
        "node `{node}` port `{port}` cannot be re-resolved by a teardown: it is not bound to a \
         pure resolver chain"
    )]
    UnresolvableCredential {
        /// The node.
        node: crate::workflow::NodeName,
        /// The port.
        port: crate::tool::PortName,
    },
    /// Resolving a secret `needs` port's own pure resolver chain failed
    /// the same way [`crate::plan::resolve_pure`] itself can.
    #[error("resolving a teardown step's credential failed: {0}")]
    Resolve(#[from] crate::plan::PlanError),
    /// A `Delete` tool's own `inverse().standing` call failed.
    #[error("{instance}: {error}")]
    Tool {
        /// The instance being observed.
        instance: crate::apply::InstanceRef,
        /// The failure.
        #[source]
        error: crate::tool::ToolError,
    },
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

    #[test]
    fn creation_skew_is_two_minutes() {
        assert_eq!(CREATION_SKEW, 120);
    }

    #[test]
    fn shift_moves_a_timestamp_by_the_given_seconds_either_way() {
        let at = crate::apply::Timestamp::parse("2026-10-12T00:00:00+00:00").unwrap();
        let later = shift(at, CREATION_SKEW);
        let earlier = shift(at, -CREATION_SKEW);
        assert_eq!(
            later,
            crate::apply::Timestamp::parse("2026-10-12T00:02:00+00:00").unwrap()
        );
        assert_eq!(
            earlier,
            crate::apply::Timestamp::parse("2026-10-11T23:58:00+00:00").unwrap()
        );
    }
}
