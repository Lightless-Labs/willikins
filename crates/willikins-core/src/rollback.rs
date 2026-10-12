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
/// [`Created::settled_by`] (milestone 3m decision 2) all go through this,
/// from [`plan_teardown`] on.
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

/// [`TeardownAction::Retained`]'s `why` for an instance planned
/// [`crate::plan::Action::Replace`] or [`crate::plan::Action::Update`]
/// (milestone 3m decision 1): the prior state at this key cannot be
/// restored either way, whatever the tool's own [`Reversal`] says.
const CHANGED_IN_PLACE: &str = "changed in place, or replaced; the prior state cannot be restored";

/// `checked`'s own node named `name`, if any.
fn find_checked_node<'a>(
    checked: &'a crate::check::Checked,
    name: &crate::workflow::NodeName,
) -> Option<&'a crate::check::CheckedNode> {
    checked.nodes().iter().find(|node| &node.name == name)
}

/// Whether `checked`'s own node named `name` is pure (milestone 3m
/// decision 5). `false` for a name `checked` has no node for -- never
/// actually reached from [`Secrets::resolve_secret_source`]: every
/// `Source::Step`/`Aggregate`/`Keyed` it resolves names a producer
/// `check` itself already resolved against this same `checked`.
fn node_is_pure(checked: &crate::check::Checked, name: &crate::workflow::NodeName) -> bool {
    find_checked_node(checked, name).is_some_and(|node| node.tool.spec().pure)
}

/// `port`'s own scalar [`crate::check::Source`], if `ports` binds it to
/// one (never a `Bound::List` -- a credential port is always scalar) and
/// binds it at all. `None` for an unbound port: decision 5's own
/// "falls back to its execution-context credential" for an optional one.
fn scalar_source<'a, I>(
    ports: &'a [crate::check::PortEdge<I>],
    port: &crate::tool::PortName,
) -> Option<&'a crate::check::Source<I>> {
    ports
        .iter()
        .find(|edge| &edge.port == port)
        .and_then(|edge| match &edge.bound {
            crate::check::Bound::Scalar { source, .. } => Some(source),
            crate::check::Bound::List { .. } => None,
        })
}

/// `source`'s own producer node's name, by [`crate::check::Source::Step`]/
/// `Aggregate`/`Keyed`; `None` for [`crate::check::Source::Literal`],
/// `Input` or `Item`, which never name another node.
fn source_producer_name<I>(
    source: &crate::check::Source<I>,
    single_names: &[crate::workflow::NodeName],
    each_names: &[crate::workflow::NodeName],
) -> Option<crate::workflow::NodeName> {
    match source {
        crate::check::Source::Step { from, .. } => Some(single_names[from.0 as usize].clone()),
        crate::check::Source::Aggregate { from, .. } | crate::check::Source::Keyed { from, .. } => {
            Some(each_names[from.0 as usize].clone())
        }
        crate::check::Source::Literal(_)
        | crate::check::Source::Input(_)
        | crate::check::Source::Item(_) => None,
    }
}

/// Every pure resolver node a `Delete` step's own secret `needs` ports
/// are directly bound to, over `checked`'s own nodes (milestone 3m
/// decision 5): exactly the `wanted` [`crate::plan::resolve_pure`] needs
/// to honestly resolve every credential [`plan_teardown`] might read
/// from `checked`. A caller has no other way to derive it:
/// [`crate::check::Checked::edge`] returns an `Edge`, never a `Source`,
/// and `CheckedNode`/`Shape`/`Source` are crate-private -- this is the
/// one function outside `plan_teardown` itself that needs to see them for
/// it (milestone plan addendum, 2026-10-12: `willikins-server`'s future
/// `Butler::plan_rollback`, task R8, calls this and feeds its result
/// straight to `resolve_pure`).
///
/// Only a node whose own tool declares [`Reversal::Delete`] **and**
/// whose `teardown:` policy is [`crate::workflow::TeardownPolicy::Delete`]
/// is walked at all: a node `plan_teardown` would never observe (kept, or
/// `Contained`/`Retained`) never reads its `needs` ports either, so
/// nothing it is bound to needs to be in `wanted`. Among that node's own
/// declared `needs` ports, a non-secret one contributes nothing (its
/// value comes from the journal, decision 5), and neither does one that
/// is unbound, literal- or workflow-input-bound: only a secret port bound
/// to a `Step`/`Aggregate`/`Keyed` source names a producer. May repeat a
/// name; [`crate::plan::resolve_pure`]'s own `wanted` is read into a
/// `HashSet` regardless.
pub fn wanted_resolvers(checked: &crate::check::Checked) -> Vec<crate::workflow::NodeName> {
    let (single_names, each_names) = crate::plan::node_names_by_shape(checked);
    let mut wanted = Vec::new();
    for checked_node in checked.nodes() {
        let Reversal::Delete { needs, .. } = checked_node.tool.reversal() else {
            continue;
        };
        if !matches!(
            checked_node.teardown,
            crate::workflow::TeardownPolicy::Delete { .. }
        ) {
            continue;
        }
        for &needs_port in needs {
            let Ok(port) = crate::tool::PortName::parse(needs_port) else {
                continue;
            };
            let is_secret = checked_node
                .tool
                .spec()
                .inputs
                .get(&port)
                .is_some_and(|spec| match &spec.ty {
                    crate::value::PortType::AnySecret => true,
                    crate::value::PortType::Exact(ty) => {
                        checked.registry().is_secret(&ty.name) == Some(true)
                    }
                });
            if !is_secret {
                continue;
            }
            let producer = match &checked_node.shape {
                crate::check::Shape::Single { ports, .. } => scalar_source(ports, &port)
                    .and_then(|source| source_producer_name(source, &single_names, &each_names)),
                crate::check::Shape::Each { ports, .. } => scalar_source(ports, &port)
                    .and_then(|source| source_producer_name(source, &single_names, &each_names)),
            };
            if let Some(name) = producer {
                wanted.push(name);
            }
        }
    }
    wanted
}

/// Everything [`decide_candidate`] needs to re-resolve a `Delete` step's
/// own secret `needs` ports (milestone 3m decision 5), bundled so neither
/// it nor [`Secrets::build_needs_inputs`] trips clippy's own
/// too-many-arguments lint.
struct Secrets<'a> {
    checked: &'a crate::check::Checked,
    resolved: &'a crate::plan::Resolved,
    facts: &'a RunFacts,
    single_names: &'a [crate::workflow::NodeName],
    each_names: &'a [crate::workflow::NodeName],
}

impl Secrets<'_> {
    /// Resolve one `Delete` step's secret `needs` port's own bound
    /// `source` (milestone 3m decision 5): a literal or a declared
    /// workflow input resolves the same way
    /// [`crate::plan::resolve_pure`] itself resolves one; a step,
    /// aggregate or keyed reference resolves only when its own producer
    /// node is pure -- `self.resolved`'s own tables hold a value for it
    /// only then (`resolve_pure`'s own backward closure never reads past
    /// a non-pure node) -- and otherwise is
    /// [`TeardownError::UnresolvableCredential`]. A `${{ item }}`-bound
    /// credential port is the same error: resolving it would need this
    /// instance's own recorded `for_each` item, which [`RunFacts`] does
    /// not carry (no current tool needs one; flagged for a follow-up
    /// rather than widened on a guess -- see the plan's 2026-10-12 R5a
    /// addendum).
    fn resolve_secret_source<I>(
        &self,
        node: &crate::workflow::NodeName,
        port: &crate::tool::PortName,
        source: &crate::check::Source<I>,
        site: &crate::site::Site,
    ) -> Result<crate::value::Value, TeardownError> {
        let unresolvable = || TeardownError::UnresolvableCredential {
            node: node.clone(),
            port: port.clone(),
        };
        match source {
            crate::check::Source::Item(_) => return Err(unresolvable()),
            crate::check::Source::Step { from, .. } => {
                if !node_is_pure(self.checked, &self.single_names[from.0 as usize]) {
                    return Err(unresolvable());
                }
            }
            crate::check::Source::Aggregate { from, .. }
            | crate::check::Source::Keyed { from, .. } => {
                if !node_is_pure(self.checked, &self.each_names[from.0 as usize]) {
                    return Err(unresolvable());
                }
            }
            crate::check::Source::Literal(_) | crate::check::Source::Input(_) => {}
        }
        let ctx = self.resolved.ctx(self.checked, &self.facts.inputs);
        let value = crate::plan::resolve_source(&ctx, source, site, None)?;
        // A `Step`/`Aggregate`/`Keyed` producer can itself be pure yet
        // bound, in turn, to an impure node's output: `resolve_pure`
        // leaves such a node `Skipped` (decision 5: "a pure node bound to
        // an impure node's output resolves `Unknown` and is not read"),
        // so `value` comes back `Unknown` rather than erroring. A `Delete`
        // tool must never observe an unresolved credential as though it
        // were a real one.
        if value.is_known() {
            Ok(value)
        } else {
            Err(unresolvable())
        }
    }

    /// Every one of `needs`' own ports, resolved for a [`Created`]
    /// (milestone 3m decision 5): a non-secret port's value comes
    /// straight from `instance`'s own recorded, re-parsed inputs; a
    /// secret one is re-resolved through [`Self::resolve_secret_source`],
    /// and left unbound when the document never bound it at all.
    fn build_needs_inputs(
        &self,
        instance: &RunInstance,
        checked_node: &crate::check::CheckedNode,
        needs: &'static [&'static str],
    ) -> Result<crate::tool::Inputs, TeardownError> {
        let mut built = crate::tool::Inputs::new();
        for &needs_port in needs {
            let Ok(port) = crate::tool::PortName::parse(needs_port) else {
                continue;
            };
            let is_secret = checked_node
                .tool
                .spec()
                .inputs
                .get(&port)
                .is_some_and(|spec| match &spec.ty {
                    crate::value::PortType::AnySecret => true,
                    crate::value::PortType::Exact(ty) => {
                        self.checked.registry().is_secret(&ty.name) == Some(true)
                    }
                });
            if is_secret {
                let site = crate::site::Site::Port {
                    node: instance.node.clone(),
                    port: port.clone(),
                };
                let resolved_value = match &checked_node.shape {
                    crate::check::Shape::Single { ports, .. } => {
                        scalar_source(ports, &port).map(|source| {
                            self.resolve_secret_source(&instance.node, &port, source, &site)
                        })
                    }
                    crate::check::Shape::Each { ports, .. } => {
                        scalar_source(ports, &port).map(|source| {
                            self.resolve_secret_source(&instance.node, &port, source, &site)
                        })
                    }
                };
                if let Some(value) = resolved_value.transpose()? {
                    built.insert(port, value);
                }
            } else if let Some(value) = instance.inputs.get(&port) {
                built.insert(port, value.clone());
            }
        }
        Ok(built)
    }
}

/// What capability × policy × standing decided for one `Create`d or
/// `Create`+`Failed` candidate (milestone 3m decision 6).
enum Decision {
    /// A step to record.
    Step(TeardownAction),
    /// A refusal reason to collect ([`TeardownError::Refused`]).
    Refused(&'static str),
}

/// Decide `checked_node`'s own action for one `Create`d or `Create`+
/// `Failed` candidate instance (milestone 3m decision 6): capability
/// ([`Reversal`]) first, then the node's own policy ([`TeardownPolicy`]),
/// then -- only under `Delete` -- its inverse's own `standing`, and
/// decision 3a's loss rules.
fn decide_candidate(
    secrets: &Secrets,
    instance: &RunInstance,
    checked_node: &crate::check::CheckedNode,
    started: crate::apply::Timestamp,
    finished: crate::apply::Timestamp,
    uncertain: bool,
) -> Result<Decision, TeardownError> {
    match checked_node.tool.reversal() {
        Reversal::Retained { why } => Ok(Decision::Step(TeardownAction::Retained { why })),
        Reversal::Contained { otherwise, .. } => {
            Ok(Decision::Step(TeardownAction::Retained { why: otherwise }))
        }
        Reversal::Delete { needs, losses, .. } => match &checked_node.teardown {
            crate::workflow::TeardownPolicy::Retain => {
                Ok(Decision::Step(TeardownAction::Kept { why: KEPT }))
            }
            crate::workflow::TeardownPolicy::Delete { accept } => {
                let window = CreationWindow {
                    start: shift(started, -CREATION_SKEW),
                    end: shift(finished, CREATION_SKEW),
                };
                let settled_by = shift(secrets.facts.finished_at, CREATION_SKEW);
                let needs_inputs = secrets.build_needs_inputs(instance, checked_node, needs)?;
                let created = Created {
                    inputs: &needs_inputs,
                    outputs: &instance.outputs,
                    window,
                    settled_by,
                    accept,
                };
                // `Catalog::insert` requires a `Delete` reversal to have
                // `Some(inverse())` (milestone 3m task R3); not visible
                // to the type system at this call site.
                let inverse = checked_node.tool.inverse().unwrap_or_else(|| {
                    unreachable!("Catalog::insert requires a Delete reversal to have Some(inverse)")
                });
                let standing = inverse
                    .standing(&created)
                    .map_err(|error| TeardownError::Tool {
                        instance: crate::apply::InstanceRef {
                            node: instance.node.clone(),
                            instance: instance.instance.clone(),
                        },
                        error,
                    })?;
                Ok(match standing {
                    Standing::Gone => Decision::Step(TeardownAction::Gone),
                    Standing::NotOurs { why } => Decision::Refused(why),
                    Standing::Drifted { what } => Decision::Refused(what),
                    Standing::AsCreated { losing } => {
                        // Fail safe: a loss the tool reports but never
                        // declared is treated as drift (decision 3a).
                        if let Some(undeclared) =
                            losing.iter().copied().find(|loss| !losses.contains(loss))
                        {
                            Decision::Refused(undeclared)
                        } else if let Some(unaccepted) = losing
                            .iter()
                            .copied()
                            .find(|loss| !accept.iter().any(|a| a.as_str() == *loss))
                        {
                            Decision::Refused(unaccepted)
                        } else {
                            Decision::Step(TeardownAction::Delete { uncertain })
                        }
                    }
                })
            }
        },
    }
}

/// Compute a teardown plan from one applied run's own journal facts
/// (milestone 3m decisions 1, 2, 3a, 4's gate rows, and 6).
///
/// Steps come out in reverse of `facts.instances`' own (forward,
/// applied-plan) order. Per candidate, capability × policy × standing
/// decides the action (decision 6): a gate instance the run found
/// satisfied is [`TeardownAction::MadeByHand`] (decision 4); `Replace`/
/// `Update` is always [`TeardownAction::Retained`] (decision 1); a
/// `Create`d or `Create`+`Failed` instance whose tool is
/// [`Reversal::Retained`] is `Retained` for the tool's own reason, and
/// one whose tool is [`Reversal::Contained`] is `Retained` for
/// `otherwise` (milestone 3m task R5b replaces this placeholder with a
/// real rider decision); one whose tool is [`Reversal::Delete`] and whose
/// node keeps it (`TeardownPolicy::Retain`) is [`TeardownAction::Kept`],
/// never observed; one whose node says to delete it is observed through
/// `standing`, and becomes [`TeardownAction::Gone`],
/// [`TeardownAction::Delete`] (`uncertain` when the create itself
/// finished `Failed`), or a refusal, per decision 3a's loss rules. A
/// started instance with no `NodeFinished` is [`TeardownAction::Unfinished`]
/// (decisions 1, 2), unconditionally. Every other candidate (a `Create`
/// that finished `Unchanged`, `Converged`, `NotRun`, `Skipped` or
/// `Blocked`, or any `Compute`/`NoOp`/`Skip` instance that is not a
/// satisfied gate) is not a step at all.
///
/// A `Create`+`Unchanged` instance's own transit-riding is milestone 3m
/// task R5b's, not this function's: it produces no step here.
///
/// # Errors
///
/// [`TeardownError::Refused`] when any observed candidate's standing
/// refuses (decision 6): every refusing instance is collected, not only
/// the first -- trust boundary 6 observes every delete-policy candidate
/// before any delete, plan time included, so this function never returns
/// on the first refusal it finds. [`TeardownError::UnresolvableCredential`]
/// or [`TeardownError::Resolve`] when a `Delete` step's own secret
/// `needs` port cannot be honestly re-resolved (decision 5); either one
/// stops this function immediately, since it names a document defect,
/// never drift. [`TeardownError::Tool`] when a `Delete` tool's own
/// `inverse().standing` call itself fails.
pub fn plan_teardown(
    facts: &RunFacts,
    checked: &crate::check::Checked,
    resolved: &crate::plan::Resolved,
) -> Result<Teardown, TeardownError> {
    let (single_names, each_names) = crate::plan::node_names_by_shape(checked);
    let secrets = Secrets {
        checked,
        resolved,
        facts,
        single_names: &single_names,
        each_names: &each_names,
    };
    let mut steps: Vec<TeardownStep> = Vec::new();
    let mut refusals: Vec<(crate::apply::InstanceRef, &'static str)> = Vec::new();

    for instance in facts.instances.iter().rev() {
        let Some(checked_node) = find_checked_node(checked, &instance.node) else {
            continue;
        };
        let instance_ref = crate::apply::InstanceRef {
            node: instance.node.clone(),
            instance: instance.instance.clone(),
        };

        // Decision 4's gate rows: pure, so never `Create`-actioned.
        if let Some(gate) = checked_node.tool.gate() {
            if let RunProgress::Finished { status, .. } = &instance.progress
                && matches!(status, crate::apply::NodeStatus::Computed)
            {
                steps.push(TeardownStep {
                    instance: instance_ref,
                    action: TeardownAction::MadeByHand { need: gate.need },
                });
            }
            continue;
        }

        match instance.action {
            crate::plan::Action::Compute
            | crate::plan::Action::NoOp
            | crate::plan::Action::Skip
            | crate::plan::Action::Blocked => continue,
            crate::plan::Action::Replace | crate::plan::Action::Update => {
                steps.push(TeardownStep {
                    instance: instance_ref,
                    action: TeardownAction::Retained {
                        why: CHANGED_IN_PLACE,
                    },
                });
                continue;
            }
            crate::plan::Action::Create => {}
        }

        let (started, finished, status) = match &instance.progress {
            RunProgress::NotFinished { started } => {
                if started.is_some() {
                    steps.push(TeardownStep {
                        instance: instance_ref,
                        action: TeardownAction::Unfinished,
                    });
                }
                continue;
            }
            RunProgress::Finished {
                started,
                finished,
                status,
            } => (*started, *finished, status),
        };

        let uncertain = match status {
            crate::apply::NodeStatus::Created => false,
            crate::apply::NodeStatus::Failed { .. } => true,
            // `Unchanged` (transit riding is task R5b's), `Converged`,
            // `Blocked`, `Skipped`, `NotRun`, `Computed`: not a step.
            _ => continue,
        };

        match decide_candidate(
            &secrets,
            instance,
            checked_node,
            started,
            finished,
            uncertain,
        )? {
            Decision::Step(action) => steps.push(TeardownStep {
                instance: instance_ref,
                action,
            }),
            Decision::Refused(why) => refusals.push((instance_ref, why)),
        }
    }

    if !refusals.is_empty() {
        return Err(TeardownError::Refused { steps: refusals });
    }
    Ok(Teardown { steps })
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

    // -----------------------------------------------------------------
    // Milestone 3m, task R5a, acceptance 4, 6, 19: `plan_teardown`.
    // -----------------------------------------------------------------

    use crate::ToolName;
    use crate::apply::{InstanceRef, NodeStatus, Timestamp};
    use crate::catalog::Catalog;
    use crate::check::{Checked, check};
    use crate::class::Class;
    use crate::plan::{Action, resolve_pure};
    use crate::testing::StubDeletableTool;
    use crate::tool::{Ensured, Inputs, Observation, Outputs, PortName, PortSpec, Tool, ToolSpec};
    use crate::value::{PortType, TypeName, TypeRef, Value};
    use crate::workflow::{Binding, Loss, Node, NodeName, TeardownPolicy, Workflow};
    use indexmap::IndexMap;
    use std::sync::{Arc, Mutex};
    use willikins_types::{DomainType, DopplerServiceToken, SinkToken};

    fn node(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    fn port(name: &str) -> PortName {
        PortName::parse(name).unwrap()
    }

    fn workflow_name(name: &str) -> willikins_types::WorkflowName {
        willikins_types::WorkflowName::parse(name).unwrap()
    }

    fn ts(at: &str) -> Timestamp {
        Timestamp::parse(at).unwrap()
    }

    /// A one-node workflow: `repo`, calling `tool`, `repo` port bound to
    /// a literal `GitHubRepo`, with `policy` as its own `teardown:`
    /// (`None` leaves the field unset — the conservative default).
    fn checked_with(tool: Arc<StubDeletableTool>, policy: Option<TeardownPolicy>) -> Checked {
        let mut repo_node = Node::new(ToolName::parse("test.deletable").unwrap()).port(
            PortName::parse("repo").unwrap(),
            Binding::Literal("example-org/example-repo".to_string()),
        );
        if let Some(policy) = policy {
            repo_node = repo_node.teardown(policy);
        }
        let workflow = Workflow::new(workflow_name("rollback-test")).node(node("repo"), repo_node);
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(tool as Arc<dyn crate::tool::Tool>).unwrap();
        check(&workflow, &catalog).unwrap_or_else(|errors| {
            unreachable!("a one-node Delete-capable workflow must check cleanly: {errors:?}")
        })
    }

    /// A trivial, empty `Resolved`: every test here declares `needs:
    /// &["repo"]`, a non-secret port, so no pure resolver chain is ever
    /// read.
    fn empty_resolved(checked: &Checked) -> crate::plan::Resolved {
        resolve_pure(checked, &IndexMap::new(), &[]).unwrap()
    }

    fn facts(instance: RunInstance, finished_at: &str) -> RunFacts {
        RunFacts {
            finished_at: ts(finished_at),
            inputs: IndexMap::new(),
            instances: vec![instance],
        }
    }

    fn instance(action: Action, progress: RunProgress) -> RunInstance {
        RunInstance {
            node: node("repo"),
            instance: None,
            action,
            progress,
            inputs: Inputs::new(),
            outputs: Outputs::new(),
        }
    }

    fn finished(started: &str, finished: &str, status: NodeStatus) -> RunProgress {
        RunProgress::Finished {
            started: ts(started),
            finished: ts(finished),
            status,
        }
    }

    const START: &str = "2026-10-12T00:00:00+00:00";
    const END: &str = "2026-10-12T00:01:00+00:00";
    const RUN_FINISHED: &str = "2026-10-12T00:02:00+00:00";

    /// Acceptance 4: `Create` + `Created` is a candidate -- under a plain
    /// `teardown: delete`, `Standing::Gone` gives `Gone`.
    #[test]
    fn create_plus_created_is_a_candidate() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Gone,
            }]
        );
        assert_eq!(tool.standing_calls(), 1);
    }

    /// Acceptance 4: `Update` + `Created` is not a candidate -- always
    /// `Retained`, whatever the tool or policy.
    #[test]
    fn update_plus_created_is_retained_not_a_candidate() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Update, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Retained {
                    why: CHANGED_IN_PLACE
                },
            }]
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 4: `Replace` + `Created` is not a candidate -- same as
    /// `Update`.
    #[test]
    fn replace_plus_created_is_retained_not_a_candidate() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Replace, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Retained {
                    why: CHANGED_IN_PLACE
                },
            }]
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 4: `NoOp` + `Unchanged` is not a candidate -- not this
    /// run's doing at all, so it is not even a step.
    #[test]
    fn noop_plus_unchanged_is_not_a_step() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::NoOp, finished(START, END, NodeStatus::Unchanged)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert!(teardown.steps.is_empty());
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 4: `Create` + `Failed`, standing `AsCreated`, gives
    /// `Delete { uncertain: true }` -- the create may have landed before
    /// the failure.
    #[test]
    fn create_plus_failed_as_created_gives_uncertain_delete() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec![],
        }));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(
                Action::Create,
                finished(
                    START,
                    END,
                    NodeStatus::Failed {
                        error: crate::tool::ToolError {
                            kind: crate::tool::ToolErrorKind::Provider,
                            message: "boom".to_string(),
                        },
                    },
                ),
            ),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Delete { uncertain: true },
            }]
        );
    }

    /// Acceptance 4: a started, unfinished instance in a finished run is
    /// `Unfinished`, and never deletes -- `standing` is never called.
    #[test]
    fn started_but_unfinished_instance_is_unfinished_and_never_deleted() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(
                Action::Create,
                RunProgress::NotFinished {
                    started: Some(ts(START)),
                },
            ),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Unfinished,
            }]
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 4: a kept `Create` + `Failed` is `Kept`, with zero
    /// `standing` calls -- a kept resource's drift is not this
    /// teardown's concern (decision 6).
    #[test]
    fn kept_create_plus_failed_is_kept_with_no_standing_calls() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec![],
        }));
        // No `teardown:` declared at all -- the conservative default.
        let checked = checked_with(Arc::clone(&tool), None);
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(
                Action::Create,
                finished(
                    START,
                    END,
                    NodeStatus::Failed {
                        error: crate::tool::ToolError {
                            kind: crate::tool::ToolErrorKind::Provider,
                            message: "boom".to_string(),
                        },
                    },
                ),
            ),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Kept { why: KEPT },
            }]
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 19: the default deletes nothing. A document with no
    /// `teardown:` anywhere plans a teardown with no `Delete` step and
    /// calls no inverse.
    #[test]
    fn the_default_policy_deletes_nothing() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(Arc::clone(&tool), None);
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert!(
            teardown
                .steps
                .iter()
                .all(|step| !matches!(step.action, TeardownAction::Delete { .. }))
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    /// Acceptance 6: one `Drifted` step refuses the whole teardown, with
    /// no `Teardown` ever built.
    #[test]
    fn a_drifted_step_refuses_the_whole_teardown() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Drifted {
            what: "a push after this run",
        }));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::Refused { steps } => {
                assert_eq!(steps.len(), 1);
                assert_eq!(steps[0].1, "a push after this run");
            }
            other => panic!("expected Refused, got {other:?}"),
        }
    }

    /// Acceptance 6: a pipeline-shaped tool whose inverse reports
    /// `builds` under a plain `teardown: delete` refuses; with
    /// `accept: [builds]` it is a `Delete`.
    #[test]
    fn an_unaccepted_loss_refuses_but_an_accepted_one_deletes() {
        let plain_tool = Arc::new(StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec!["builds"],
        }));
        let checked = checked_with(
            Arc::clone(&plain_tool),
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::Refused { steps } => {
                assert_eq!(
                    steps,
                    vec![(
                        InstanceRef {
                            node: node("repo"),
                            instance: None
                        },
                        "builds"
                    )]
                );
            }
            other => panic!("expected Refused, got {other:?}"),
        }

        let accepting_tool = Arc::new(StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec!["builds"],
        }));
        let checked = checked_with(
            Arc::clone(&accepting_tool),
            Some(TeardownPolicy::Delete {
                accept: vec![Loss::parse("builds").unwrap()],
            }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Delete { uncertain: false },
            }]
        );
    }

    /// Acceptance 6: a drifted resource on a kept node refuses nothing --
    /// its inverse is never called at all.
    #[test]
    fn a_drifted_resource_on_a_kept_node_refuses_nothing() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Drifted {
            what: "a push after this run",
        }));
        let checked = checked_with(Arc::clone(&tool), Some(TeardownPolicy::Retain));
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None
                },
                action: TeardownAction::Kept { why: KEPT },
            }]
        );
        assert_eq!(tool.standing_calls(), 0);
    }

    // -----------------------------------------------------------------
    // Milestone 3m, task R5a (bug fix + coverage, 2026-10-12 addendum):
    // the secret `needs` resolution path (decision 5) and
    // `wanted_resolvers`.
    // -----------------------------------------------------------------

    fn ty(name: &str) -> TypeRef {
        TypeRef::scalar(TypeName::parse(name).unwrap())
    }

    /// A fixed secret value every resolver stub below returns: a
    /// `DopplerServiceToken`, assembled with `concat!` the same way
    /// `tests/it/check_adversarial.rs` does, so the secret-literal guard
    /// never flags a fake-but-shaped token in this source.
    fn secret_value() -> Value {
        Value::known(
            DopplerServiceToken::parse(concat!(
                "dp.st.",
                "fakesecretbytesaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ))
            .unwrap(),
        )
    }

    /// A pure or impure resolver stub with no inputs, one output
    /// (`value`, a secret `DopplerServiceToken` fixed to
    /// [`secret_value`]).
    struct FixedTool {
        spec: ToolSpec,
    }

    impl Tool for FixedTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, crate::tool::ToolError> {
            let mut outputs = Outputs::new();
            outputs.insert(port("value"), secret_value());
            Ok(Observation::Absent { predicted: outputs })
        }

        fn ensure(
            &self,
            _inputs: &Inputs,
            _token: &SinkToken,
        ) -> Result<Ensured, crate::tool::ToolError> {
            unreachable!("not used by this test")
        }
    }

    fn resolver_tool(name: &str, pure: bool) -> Arc<FixedTool> {
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), ty("DopplerServiceToken"));
        Arc::new(FixedTool {
            spec: ToolSpec {
                name: ToolName::parse(name).unwrap(),
                description: "test".to_string(),
                inputs: IndexMap::new(),
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure,
            },
        })
    }

    /// A pure, one-input passthrough-shaped stub: input `raw`
    /// (`AnySecret`), output `parsed` (a secret `DopplerServiceToken`).
    /// Used to bind a secret `needs` port to a pure node that is itself
    /// bound to an *impure* node's own output.
    fn wrap_tool(name: &str) -> Arc<FixedTool> {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("raw"),
            PortSpec {
                ty: PortType::AnySecret,
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("parsed"), ty("DopplerServiceToken"));
        Arc::new(FixedTool {
            spec: ToolSpec {
                name: ToolName::parse(name).unwrap(),
                description: "test".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        })
    }

    /// A `Delete`-capable stub with a secret `needs` port (`token`),
    /// whose inverse records exactly what `Created.inputs` carried for
    /// it: acceptance that a resolved secret actually reaches the
    /// inverse, not only that resolution itself did not error.
    struct SecretNeedTool {
        spec: ToolSpec,
        inverse: CapturingInverse,
    }

    struct CapturingInverse {
        captured: Mutex<Option<Value>>,
    }

    impl Inverse for CapturingInverse {
        fn standing(&self, created: &Created<'_>) -> Result<Standing, crate::tool::ToolError> {
            *self.captured.lock().unwrap() = created.inputs.get(&port("token")).cloned();
            Ok(Standing::Gone)
        }

        fn delete(
            &self,
            _created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, crate::tool::ToolError> {
            Ok(Deleted { already_gone: true })
        }
    }

    impl Tool for SecretNeedTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, crate::tool::ToolError> {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }

        fn ensure(
            &self,
            _inputs: &Inputs,
            _token: &SinkToken,
        ) -> Result<Ensured, crate::tool::ToolError> {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
        }

        fn reversal(&self) -> Reversal {
            Reversal::Delete {
                needs: &["repo", "token"],
                within: &[],
                losses: &[],
            }
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            Some(&self.inverse)
        }
    }

    fn secret_need_tool() -> Arc<SecretNeedTool> {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("repo"),
            PortSpec {
                ty: PortType::Exact(ty("GitHubRepo")),
                required: true,
                derived_only: false,
            },
        );
        inputs.insert(
            port("token"),
            PortSpec {
                ty: PortType::AnySecret,
                required: false,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), ty("GitHubRepo"));
        Arc::new(SecretNeedTool {
            spec: ToolSpec {
                name: ToolName::parse("test.secret_deletable").unwrap(),
                description: "A stub tool with a secret needs port.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: CapturingInverse {
                captured: Mutex::new(None),
            },
        })
    }

    fn repo_port_bindings(tool: Node) -> Node {
        tool.port(
            port("repo"),
            Binding::Literal("example-org/example-repo".to_string()),
        )
    }

    /// Acceptance (R5a's own 2026-10-12 addendum): a secret `needs` port
    /// bound to a pure resolver node resolves through
    /// `resolve_pure`/`resolve_source`, and the resolved value really
    /// reaches the inverse's own `Created.inputs` -- not only that
    /// resolution did not error.
    #[test]
    fn secret_needs_port_resolves_through_a_pure_resolver_chain() {
        let get_token = resolver_tool("test.get_token_pure", true);
        let tool = secret_need_tool();
        let workflow = Workflow::new(workflow_name("secret-resolve-pure"))
            .node(node("get_token"), Node::new(get_token.spec.name.clone()))
            .node(
                node("repo"),
                repo_port_bindings(Node::new(tool.spec.name.clone()))
                    .port(
                        port("token"),
                        Binding::Step {
                            node: node("get_token"),
                            port: port("value"),
                        },
                    )
                    .teardown(TeardownPolicy::Delete { accept: vec![] }),
            );
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::clone(&get_token) as Arc<dyn Tool>)
            .unwrap();
        catalog.insert(Arc::clone(&tool) as Arc<dyn Tool>).unwrap();
        let checked = check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("must check cleanly: {errors:?}"));

        let wanted = wanted_resolvers(&checked);
        assert_eq!(wanted, vec![node("get_token")]);

        let resolved = resolve_pure(&checked, &IndexMap::new(), &wanted).unwrap();
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(teardown.steps.len(), 1);
        assert_eq!(teardown.steps[0].action, TeardownAction::Gone);
        assert_eq!(*tool.inverse.captured.lock().unwrap(), Some(secret_value()));
    }

    /// Acceptance: a secret `needs` port bound directly to an *impure*
    /// node is `UnresolvableCredential` -- never silently `Unknown`.
    #[test]
    fn secret_needs_port_bound_to_an_impure_node_is_unresolvable() {
        let get_token = resolver_tool("test.get_token_impure", false);
        let tool = secret_need_tool();
        let workflow = Workflow::new(workflow_name("secret-resolve-impure"))
            .node(node("get_token"), Node::new(get_token.spec.name.clone()))
            .node(
                node("repo"),
                repo_port_bindings(Node::new(tool.spec.name.clone()))
                    .port(
                        port("token"),
                        Binding::Step {
                            node: node("get_token"),
                            port: port("value"),
                        },
                    )
                    .teardown(TeardownPolicy::Delete { accept: vec![] }),
            );
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::clone(&get_token) as Arc<dyn Tool>)
            .unwrap();
        catalog.insert(Arc::clone(&tool) as Arc<dyn Tool>).unwrap();
        let checked = check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("must check cleanly: {errors:?}"));

        let wanted = wanted_resolvers(&checked);
        let resolved = resolve_pure(&checked, &IndexMap::new(), &wanted).unwrap();
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::UnresolvableCredential { port, .. } => {
                assert_eq!(port.as_str(), "token");
            }
            other => panic!("expected UnresolvableCredential, got {other:?}"),
        }
    }

    /// Acceptance (the bug `resolve_secret_source`'s own `is_known` check
    /// fixes): a secret `needs` port bound to a *pure* node that is
    /// itself bound to an impure node's own output is also
    /// `UnresolvableCredential` -- `resolve_pure` leaves such a node
    /// `Skipped`, so without the check this would silently observe an
    /// `Unknown` credential as though it were real.
    #[test]
    fn secret_needs_port_bound_to_a_pure_node_downstream_of_an_impure_one_is_unresolvable() {
        let get_token = resolver_tool("test.get_token_impure2", false);
        let wrap = wrap_tool("test.wrap");
        let tool = secret_need_tool();
        let workflow = Workflow::new(workflow_name("secret-resolve-downstream"))
            .node(node("get_token"), Node::new(get_token.spec.name.clone()))
            .node(
                node("wrap"),
                Node::new(wrap.spec.name.clone()).port(
                    port("raw"),
                    Binding::Step {
                        node: node("get_token"),
                        port: port("value"),
                    },
                ),
            )
            .node(
                node("repo"),
                repo_port_bindings(Node::new(tool.spec.name.clone()))
                    .port(
                        port("token"),
                        Binding::Step {
                            node: node("wrap"),
                            port: port("parsed"),
                        },
                    )
                    .teardown(TeardownPolicy::Delete { accept: vec![] }),
            );
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::clone(&get_token) as Arc<dyn Tool>)
            .unwrap();
        catalog.insert(Arc::clone(&wrap) as Arc<dyn Tool>).unwrap();
        catalog.insert(Arc::clone(&tool) as Arc<dyn Tool>).unwrap();
        let checked = check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("must check cleanly: {errors:?}"));

        let wanted = wanted_resolvers(&checked);
        assert_eq!(wanted, vec![node("wrap")]);

        let resolved = resolve_pure(&checked, &IndexMap::new(), &wanted).unwrap();
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::UnresolvableCredential { port, .. } => {
                assert_eq!(port.as_str(), "token");
            }
            other => panic!("expected UnresolvableCredential, got {other:?}"),
        }
    }

    /// `wanted_resolvers` skips a node the teardown would never observe
    /// (its policy is `retain`): a secret `needs` port there is never
    /// read at all, so its producer is not `wanted`.
    #[test]
    fn wanted_resolvers_skips_a_kept_nodes_own_producer() {
        let get_token = resolver_tool("test.get_token_kept", true);
        let tool = secret_need_tool();
        let workflow = Workflow::new(workflow_name("secret-resolve-kept"))
            .node(node("get_token"), Node::new(get_token.spec.name.clone()))
            .node(
                node("repo"),
                repo_port_bindings(Node::new(tool.spec.name.clone())).port(
                    port("token"),
                    Binding::Step {
                        node: node("get_token"),
                        port: port("value"),
                    },
                ),
                // No `teardown:` at all -- the conservative default, `retain`.
            );
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::clone(&get_token) as Arc<dyn Tool>)
            .unwrap();
        catalog.insert(Arc::clone(&tool) as Arc<dyn Tool>).unwrap();
        let checked = check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("must check cleanly: {errors:?}"));

        assert!(wanted_resolvers(&checked).is_empty());
    }
}
