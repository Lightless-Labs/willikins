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
//! task R3) are the shape an inverse works with. [`plan_teardown`]
//! (milestone 3m tasks R5a and R5b) computes a [`Teardown`] from one
//! run's own [`RunFacts`], over a [`crate::check::Checked`] document and
//! the [`crate::plan::Resolved`] pure-resolver closure
//! [`crate::plan::resolve_pure`] evaluated for it — [`wanted_resolvers`]
//! is the seam a caller (`willikins-server`'s future
//! `Butler::plan_rollback`, task R8) uses to compute that closure's own
//! `wanted` set from `checked` alone. Riding (milestone 3m task R5b,
//! decisions 3 and 3a) is computed once, structurally, over every
//! instance the run named -- never from the resolution the fixed point
//! below produces -- so a [`Rider`] is attached to [`Created::riders`]
//! purely on typed-equal recorded values and "wrote or found something",
//! and the fixed point (`resolve_actions`) only decides, from that same
//! static matching, whether *this* instance itself rides out, is kept
//! with its container, or stands on its own. [`apply_teardown`]
//! (milestone 3m task R6) re-plans (reusing the very same
//! `plan_teardown_steps` [`plan_teardown`] itself calls), refuses when the
//! fresh observation disagrees with what was approved
//! ([`StepFingerprint`]'s own `standing`, decision 8's "a loss that
//! appears between approval and apply therefore differs"), mints the
//! one [`willikins_types::TeardownToken`] this module's executor ever
//! mints, and deletes every `Delete` step in order, stopping at the
//! first failure.

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
/// container gets [`KEPT_RIDER`] instead (decision 3a).
pub const KEPT: &str = "the document keeps it (no teardown: delete)";

/// [`TeardownAction::Kept`]'s `why` for a [`Reversal::Contained`] or
/// transit rider whose matched container is itself [`TeardownAction::Kept`]
/// (milestone 3m decision 3a): "its container is kept by the document".
/// A `Delete`-capable candidate's own `teardown: retain` always gives
/// [`KEPT`] instead, whatever it matches or does not.
pub const KEPT_RIDER: &str = "its container is kept by the document";

/// [`TeardownError::Refused`]'s reason for a `Delete`-capable candidate
/// whose own node keeps it (`teardown: retain`, explicit or default)
/// while it structurally rides inside a container this teardown deletes
/// (milestone 3m decision 3a): the document cannot keep something inside
/// a container it deletes, and silently deleting it anyway would break
/// the conservative default.
pub const RIDER_KEPT_INSIDE_DELETED_CONTAINER: &str =
    "a node the document keeps is inside a container it deletes";

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

/// One instance this run recorded that structurally rides with (or
/// inside) a [`Reversal::Delete`] instance's own container, per that
/// instance's own declared [`Within`] (milestone 3m decisions 3, 3a).
/// Carries only its journal-recorded, non-secret values, re-parsed by
/// declared type -- never resolved through `Secrets`: "no rider port is
/// ever added to `resolve_pure`'s `wanted`" (decision 5). A secret port
/// of a rider's own is left exactly as the journal keeps it,
/// [`crate::value::Value::unknown`].
///
/// Included here only when this run's own record of it is something a
/// container must tolerate: finished [`crate::apply::NodeStatus::Created`],
/// or `Create`+[`crate::apply::NodeStatus::Unchanged`] in transit
/// (decision 3's "riding is not tolerance"). A rider two or more `Within`
/// hops away from the container being observed (a token inside a config
/// inside a project) is still carried here, by the same transitive
/// closure over structural matching -- never only its own direct
/// container's riders.
#[derive(Debug, Clone, Copy)]
pub struct Rider<'a> {
    /// The rider's own tool name.
    pub tool: &'a str,
    /// The rider's own instance identity.
    pub instance: &'a crate::apply::InstanceRef,
    /// The rider's own recorded, non-secret inputs; secret ones `Unknown`.
    pub inputs: &'a crate::tool::Inputs,
    /// The rider's own recorded, non-secret outputs; secret ones
    /// `Unknown`.
    pub outputs: &'a crate::tool::Outputs,
}

/// What a [`Reversal::Delete`] tool's inverse is given to work with: the
/// journal's own record of what this run created, re-resolved just
/// enough to delete it (milestone 3m decisions 2 and 5). Debug only —
/// unlike [`crate::tool::Inputs`]/[`crate::tool::Outputs`], whose own
/// `Serialize` already redacts a secret, nothing serializes a `Created`
/// directly: a teardown never journals one (trust boundary 7).
///
/// This is R3's shape, plus T1's `accept`, R5a's `window` and R5b's
/// `riders`.
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
    /// What this run recorded riding inside this instance (milestone 3m
    /// decisions 3, 3a): tolerated contents only -- see [`Rider`]'s own
    /// doc. Empty for a container nothing this run rode into.
    pub riders: &'a [Rider<'a>],
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
///
/// `Serialize` only (milestone 3m task R7), the same deliberate half of
/// the usual pair [`crate::plan::Plan`] and [`crate::tool::Outputs`]
/// already carry: this type, and [`TeardownStep`]/[`TeardownAction`]
/// beneath it, exist so `willikins_journal::Redacted<Teardown>` can
/// serialize one (`willikins-journal`'s own `redacted.rs` adds the
/// `Redactable` impl, since its sealing module is private to that file).
/// Never `Deserialize`: a `Redacted<T>` never reconstructs a live `T` (see
/// that type's own module doc), and [`TeardownAction::Kept`]'s and
/// [`TeardownAction::Retained`]'s own `why: &'static str` could not round
/// trip anyway. The journal's own byte-exact comparison at apply time
/// uses [`StepFingerprint`] instead, which is built to round-trip.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Teardown {
    /// Every step, in the order they would run: the reverse of the
    /// applied plan's own order.
    pub steps: Vec<TeardownStep>,
}

/// One instance's own place in a [`Teardown`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TeardownStep {
    /// The instance.
    pub instance: crate::apply::InstanceRef,
    /// What this step does.
    pub action: TeardownAction,
}

/// What one [`TeardownStep`] does (SHARED VALUES).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
    /// This instance structurally rides out with `container`'s own delete
    /// (decision 3, milestone 3m task R5b): `container` is the instance
    /// this one's own declared [`Within`] matched directly, which may
    /// itself be a `RidesWith` (a token inside a config inside a
    /// project). Never called through `inverse().standing`/`delete` on
    /// its own: its container's own inverse is the one that observes and
    /// deletes it, tolerating it as a [`Rider`].
    RidesWith {
        /// The container instance this one rides out with.
        container: crate::apply::InstanceRef,
    },
    /// The resource no longer exists; nothing to delete.
    Gone,
    /// The document keeps it (decision 3a): never observed, so a step
    /// with this action called no `standing`.
    Kept {
        /// Why it is kept — [`KEPT`] under the node's own default policy,
        /// or [`KEPT_RIDER`] for a rider whose matched container is
        /// itself `Kept`.
        why: &'static str,
    },
    /// The tool itself cannot undo this: `why` is its own declared
    /// [`Reversal::Retained::why`], or a [`Reversal::Contained`]
    /// instance's own [`Reversal::Contained::otherwise`] when nothing
    /// this run deletes matches its declared [`Within`] (milestone 3m
    /// task R5b).
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

/// What a [`TeardownPolicy::Delete`] step's own `standing()` call
/// actually reported (milestone 3m task R6, decision 8), carried on its
/// [`StepFingerprint`] so a later re-observation can be compared value
/// for value, not only by which [`TeardownAction`] it produced: an
/// accepted loss never changes a step's own action (still
/// `Delete { uncertain }` either way), so the action alone cannot tell
/// "no loss yet" apart from "a loss, but one this node's policy already
/// accepts" -- exactly the gap decision 8's "a loss that appears between
/// approval and apply therefore differs" closes. `None` on
/// [`StepFingerprint::standing`] for a step whose action never called
/// `standing` at all (`RidesWith`, `Kept`, `Retained`, `MadeByHand`,
/// `Unfinished`) -- only `Gone` and the `Delete`-producing `AsCreated`
/// ever do (decision 6).
///
/// SHARED VALUES names this type (credited to tasks R5a and R7, for the
/// journal's own typed comparison); it is taken over here, by R6, because
/// R6's own row is the first that actually needs it ("compare
/// fingerprints... standing kinds and losses included"), ahead of R7's
/// journal work. See this task's own plan addendum.
///
/// `losing` is `Vec<String>`, not SHARED VALUES' own `Vec<&'static str>`
/// (milestone 3m task R7, recorded in that task's own plan addendum):
/// [`StepFingerprint`] is stored directly (unwrapped) on a journal event,
/// the same way [`crate::plan::InstanceFingerprint`] already is, so it
/// must round-trip through `Deserialize` -- and `serde` has no way to
/// produce a `&'static str` back out of a deserializer without leaking
/// memory on every replay. The *names themselves* are still exactly the
/// tool's own declared [`Reversal::Delete::losses`] strings, just owned
/// here; comparing them by content (`==`) works the same either way.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StandingKind {
    /// [`Standing::Gone`] was observed.
    Gone,
    /// [`Standing::AsCreated`] was observed, with these declared losses
    /// currently reported (empty when none).
    AsCreated {
        /// The declared losses this observation reported.
        losing: Vec<String>,
    },
}

/// A discriminant-only mirror of [`TeardownAction`] for
/// [`StepFingerprint::action`] (milestone 3m task R7, recorded in that
/// task's own plan addendum): keeps exactly what decides whether a fresh
/// re-observation still agrees with what was approved (decision 8) --
/// which variant, [`Self::Delete`]'s own `uncertain` flag, and
/// [`Self::RidesWith`]'s own `container` -- and drops each variant's own
/// purely descriptive `why`/`need` text, which is `&'static str` (cannot
/// round-trip through `Deserialize`, the same reason [`StandingKind`]
/// above moved off it) and carries no identity of its own: the *display*
/// text for the same step, at the same code path, is already shown to the
/// operator via [`Teardown`] (`willikins_journal::Redacted<Teardown>`),
/// and decision 8's own "`StepFingerprint.action` carries the
/// discriminant, so a step the document keeps and one the tool cannot
/// undo never compare equal" already holds without it -- `Kept` and
/// `Retained` are different variants of this enum regardless of any
/// payload.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeardownActionKind {
    /// Mirrors [`TeardownAction::Delete`].
    Delete {
        /// See [`TeardownAction::Delete::uncertain`].
        uncertain: bool,
    },
    /// Mirrors [`TeardownAction::RidesWith`].
    RidesWith {
        /// See [`TeardownAction::RidesWith::container`].
        container: crate::apply::InstanceRef,
    },
    /// Mirrors [`TeardownAction::Gone`].
    Gone,
    /// Mirrors [`TeardownAction::Kept`], without its own `why`.
    Kept,
    /// Mirrors [`TeardownAction::Retained`], without its own `why`.
    Retained,
    /// Mirrors [`TeardownAction::MadeByHand`], without its own `need`.
    MadeByHand,
    /// Mirrors [`TeardownAction::Unfinished`].
    Unfinished,
}

impl From<&TeardownAction> for TeardownActionKind {
    fn from(action: &TeardownAction) -> Self {
        match action {
            TeardownAction::Delete { uncertain } => Self::Delete {
                uncertain: *uncertain,
            },
            TeardownAction::RidesWith { container } => Self::RidesWith {
                container: container.clone(),
            },
            TeardownAction::Gone => Self::Gone,
            TeardownAction::Kept { .. } => Self::Kept,
            TeardownAction::Retained { .. } => Self::Retained,
            TeardownAction::MadeByHand { .. } => Self::MadeByHand,
            TeardownAction::Unfinished => Self::Unfinished,
        }
    }
}

/// One step's own fingerprint (milestone 3m decision 8, SHARED VALUES
/// "Step fingerprint"): everything [`apply_teardown`]'s own comparison
/// needs to tell "this run's own teardown, re-observed, is still exactly
/// what was approved" from "something changed" -- without comparing a
/// secret or otherwise-unredacted value (trust boundaries 5, 7). Produced
/// by [`plan_teardown_fingerprint`], one entry per [`Teardown::steps`]
/// entry, in the same order; the caller that approves a teardown
/// (`willikins-server`'s future `Butler::plan_rollback`, task R8) keeps
/// this alongside the [`Teardown`] it shows the operator, and passes it
/// back to [`apply_teardown`] at apply time.
///
/// Bundles `node` and `instance` into one [`crate::apply::InstanceRef`]
/// rather than SHARED VALUES' own two separate fields, matching
/// [`TeardownStep::instance`]'s own shape -- a deviation recorded in R6's
/// own plan addendum, not a change of meaning.
///
/// `Serialize`/`Deserialize`/`JsonSchema` (milestone 3m task R7): this
/// type is stored directly (unwrapped) on `willikins_journal::Event`'s
/// `TeardownPlanned.fingerprint`, exactly the way
/// [`crate::plan::InstanceFingerprint`] already sits on `PlanRecorded`'s
/// own `fingerprint` -- so a restarted `willikins-server` can read back a
/// typed, comparable fingerprint from the journal at apply time, the same
/// way it already does for a forward run's drift check. `action` is
/// [`TeardownActionKind`], not [`TeardownAction`] itself, for exactly the
/// reason that type's own doc gives.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct StepFingerprint {
    /// The instance.
    pub instance: crate::apply::InstanceRef,
    /// What `plan_teardown` decided for it, as a discriminant.
    pub action: TeardownActionKind,
    /// What its own `standing()` call reported, if it called one at all.
    pub standing: Option<StandingKind>,
}

/// Why [`plan_teardown`] could not produce an approvable [`Teardown`].
///
/// `Serialize` (milestone 3m task R7), the same one-way half
/// [`Teardown`] itself carries and for the same reason: nested inside
/// [`TeardownApplyError::Replan`], this is what
/// `willikins_journal::Redacted<TeardownApplyError>` wraps for
/// `TeardownFinished`'s own `Failed` outcome. Never `Deserialize`: a
/// `Redacted<T>` never reconstructs a live `T`, and `Refused`'s own
/// `&'static str` reasons could not round-trip anyway.
#[derive(Debug, Clone, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind")]
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
    /// A `Delete` tool's secret `needs` port cannot be honestly
    /// re-resolved (decision 5): bound to a node that is not itself pure,
    /// bound (however many pure hops away) to an impure node's own
    /// output, or (not yet supported; see this variant's own call site)
    /// the current `for_each` item.
    #[error(
        "node `{node}` port `{port}` cannot be re-resolved by a teardown: it is not honestly \
         bound to a pure resolver chain"
    )]
    UnresolvableCredential {
        /// The node.
        node: crate::workflow::NodeName,
        /// The port.
        port: crate::tool::PortName,
    },
    /// Resolving a secret `needs` port's own pure resolver chain failed
    /// the same way [`crate::plan::resolve_pure`] itself can.
    ///
    /// A struct variant with one named field, like [`Self::Tool`] below
    /// and [`crate::apply::ApplyError::Plan`] -- not a tuple variant
    /// (milestone 3m task R7): this enum is now internally tagged
    /// (`#[serde(tag = "kind")]`) for `willikins_journal::Redacted`, and
    /// [`crate::plan::PlanError`] carries its own `kind` tag too; nesting
    /// it under a named field avoids the two tags colliding in one JSON
    /// object. `#[from]` works the same on a named field, so every `?`
    /// call site is unchanged.
    #[error("resolving a teardown step's credential failed: {error}")]
    Resolve {
        /// The failure.
        #[from]
        error: crate::plan::PlanError,
    },
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
///
/// **Over-approximates decision 5 on purpose.** Decision 5 names the
/// pure nodes a `Delete` *step's* own secret `needs` ports are bound to
/// — but a step only exists for an instance `plan_teardown`'s own
/// candidate predicate admits, which needs the run's `RunFacts`, not
/// just `checked`. This function has no `RunFacts`, so it reads every
/// `teardown: delete` node's own `needs` ports, whatever a given run's
/// instances actually did. Safe (`resolve_pure` still never calls an
/// impure tool's `read`; the only cost is evaluating a resolver chain
/// `plan_teardown` then does not end up needing) but wider than
/// necessary. Tightening it to the run's own candidates is a caller's
/// call (task R8), not this function's.
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

/// Everything [`decide_standalone`] needs to re-resolve a `Delete` step's
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
/// `Failed` candidate instance that structurally rides with nothing being
/// deleted or kept (milestone 3m decision 6): capability ([`Reversal`])
/// first, then the node's own policy ([`TeardownPolicy`]), then -- only
/// under `Delete` -- its inverse's own `standing`, and decision 3a's loss
/// rules. `riders` is this instance's own structurally matched, tolerated
/// contents (milestone 3m task R5b, decision 3) -- attached to
/// [`Created::riders`] exactly when this reversal is `Delete` and its
/// policy says to delete, the only case that ever observes a resource at
/// all.
///
/// Returns, alongside the [`Decision`], the [`StandingKind`] its own
/// `standing()` call reported -- `None` for a branch that never calls one
/// (milestone 3m task R6): [`Resolution::standing`] carries it into the
/// matching [`StepFingerprint`].
fn decide_standalone(
    secrets: &Secrets,
    instance: &RunInstance,
    checked_node: &crate::check::CheckedNode,
    started: crate::apply::Timestamp,
    finished: crate::apply::Timestamp,
    uncertain: bool,
    riders: &[Rider<'_>],
) -> Result<(Decision, Option<StandingKind>), TeardownError> {
    match checked_node.tool.reversal() {
        Reversal::Retained { why } => Ok((Decision::Step(TeardownAction::Retained { why }), None)),
        Reversal::Contained { otherwise, .. } => Ok((
            Decision::Step(TeardownAction::Retained { why: otherwise }),
            None,
        )),
        Reversal::Delete { needs, losses, .. } => match &checked_node.teardown {
            crate::workflow::TeardownPolicy::Retain => {
                Ok((Decision::Step(TeardownAction::Kept { why: KEPT }), None))
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
                    riders,
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
                // Milestone 3m task R6: the fingerprint's own half of this
                // observation, kept apart from `decision` below --
                // `NotOurs`/`Drifted` never reach a `Teardown` at all (they
                // only ever refuse), so neither needs a `StandingKind`.
                let standing_kind = match &standing {
                    Standing::Gone => Some(StandingKind::Gone),
                    Standing::AsCreated { losing } => Some(StandingKind::AsCreated {
                        losing: losing.iter().map(ToString::to_string).collect(),
                    }),
                    Standing::NotOurs { .. } | Standing::Drifted { .. } => None,
                };
                let decision = match standing {
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
                };
                Ok((decision, standing_kind))
            }
        },
    }
}

// ---------------------------------------------------------------------
// Riding (milestone 3m task R5b, decisions 3, 3a)
// ---------------------------------------------------------------------

/// One instance, reduced to exactly what [`rides_with`] needs (milestone
/// 3m task R5b): its own tool name, identity, and recorded values on both
/// sides. Never tied to [`RunInstance`]: `Plan::not_reversible` (task F1)
/// matches the very same way over a forward `Plan`'s own recorded
/// instances, before any run exists, which has no `RunInstance` or
/// `RunProgress` to build one from. [`RideView`] wraps one of these with
/// the journal-only facts riding's own fixed point needs besides.
struct Recorded<'a> {
    /// This instance's own tool name.
    tool: &'a str,
    /// This instance's own identity.
    instance: crate::apply::InstanceRef,
    /// This instance's own recorded, non-secret inputs; secret ones
    /// `Unknown`.
    inputs: &'a crate::tool::Inputs,
    /// This instance's own recorded, non-secret outputs; secret ones
    /// `Unknown`.
    outputs: &'a crate::tool::Outputs,
}

/// One instance, reduced to exactly what the ride fixed point below needs
/// on top of [`Recorded`] (milestone 3m task R5b): the checked node its
/// own tool and policy come from, the journal facts behind it, and
/// whether this run's own record of it is a candidate, a transit rider,
/// or neither.
struct RideView<'a> {
    /// This instance's own recorded identity and values -- what
    /// [`rides_with`] alone needs.
    recorded: Recorded<'a>,
    /// The checked node this instance's own tool and policy come from.
    checked_node: &'a crate::check::CheckedNode,
    /// The journal facts this instance's own view comes from -- also
    /// `decide_standalone`'s own `instance` parameter, so `idx` (into the
    /// ride resolver's own filtered sequence) is never confused with an
    /// index into `facts.instances` itself.
    run_instance: &'a RunInstance,
    /// Whether this run's own record of this instance is something a
    /// container must tolerate (decision 3's "riding is not tolerance"):
    /// finished `Created`, or `Create`+`Unchanged` in transit. `false`
    /// for `Failed`, `NotRun`, `Skipped`, `Blocked` or an unfinished
    /// instance -- each wrote nothing this run can vouch for.
    wrote: bool,
    /// Whether this instance is a transit rider (`Create`+`Unchanged`,
    /// decision 1) rather than a genuine candidate (`Create`+`Created` or
    /// `Create`+`Failed`): a transit rider never has its own standalone
    /// `Delete`, and decision 3a's "must say `teardown: delete` or
    /// refuse" rule never applies to it.
    transit: bool,
}

/// Whether `within`'s own mapped ports hold equal, known values between
/// `rider` (always the left, own-port side) and `container` (`within`'s
/// declared [`Side`]) -- milestone 3m decisions 3 and 3a's "typed
/// equality on recorded values". `within.tool` must name `container`'s
/// own tool, and `rider` is never matched against itself (a `Within` may
/// name its own tool -- milestone 3m's 2026-10-12 X1 addendum). Neither
/// side ever counts as a match when it is
/// [`crate::value::Value::is_known`]`() == false`: two `Unknown`s of one
/// type compare equal (`value.rs`), so a port neither this run's rider
/// nor the container ever bound (an unstarted `NotRun` instance's own
/// empty recorded inputs, say) must never ride by default. Takes a
/// [`Recorded`], never a [`RideView`]: the one thing `Plan::not_reversible`
/// (task F1) will call this same function with, over a forward plan's
/// own instances, which carry no [`RunInstance`].
fn rides_with(within: &Within, rider: &Recorded, container: &Recorded) -> bool {
    if rider.instance == container.instance {
        return false;
    }
    if container.tool != within.tool {
        return false;
    }
    within.ports.iter().all(|&(own_port, side, their_port)| {
        let (Ok(own_port), Ok(their_port)) = (
            crate::tool::PortName::parse(own_port),
            crate::tool::PortName::parse(their_port),
        ) else {
            return false;
        };
        let Some(own_value) = rider.inputs.get(&own_port) else {
            return false;
        };
        let their_value = match side {
            Side::Input => container.inputs.get(&their_port),
            Side::Output => container.outputs.get(&their_port),
        };
        let Some(their_value) = their_value else {
            return false;
        };
        own_value.is_known() && their_value.is_known() && own_value == their_value
    })
}

/// `view`'s own declared [`Within`] list, by its tool's [`Reversal`]
/// (empty for [`Reversal::Retained`], which never rides).
fn within_list(view: &RideView) -> &'static [Within] {
    match view.checked_node.tool.reversal() {
        Reversal::Delete { within, .. } | Reversal::Contained { within, .. } => within,
        Reversal::Retained { .. } => &[],
    }
}

/// The first instance among `views` that `me`'s own declared `Within`
/// list matches, trying each declared entry in order and, within one
/// entry, every other instance in `views`' own order (milestone 3m task
/// R5b: deterministic, and needs no graph, per decision 3). `None` when
/// no declared entry matches anything this run named at all.
fn find_container(views: &[RideView], me: usize) -> Option<usize> {
    for within in within_list(&views[me]) {
        for (idx, container) in views.iter().enumerate() {
            if idx != me && rides_with(within, &views[me].recorded, &container.recorded) {
                return Some(idx);
            }
        }
    }
    None
}

/// Every instance's own direct matched container, by structural matching
/// alone (milestone 3m decision 3: "riding is computed over all of the
/// run's instances, whatever their action or status"). Independent of
/// any instance's own eventual teardown action -- see this module's doc.
fn container_of(views: &[RideView]) -> Vec<Option<usize>> {
    (0..views.len())
        .map(|me| find_container(views, me))
        .collect()
}

/// `container_of`'s own reverse image, filtered to riders a container
/// must tolerate (`view.wrote`, decision 3's "riding is not tolerance").
fn riders_of(views: &[RideView], container_of: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut riders = vec![Vec::new(); views.len()];
    for (idx, container) in container_of.iter().enumerate() {
        if let Some(container) = container
            && views[idx].wrote
        {
            riders[*container].push(idx);
        }
    }
    riders
}

/// Every instance riding inside `start`, directly or transitively
/// (milestone 3m decision 3: a token inside a config inside a project
/// must still reach the project's own `Created::riders`). `visited`
/// guards the one case two instances can ride each other
/// (`riders_of`/`container_of` forming a 2-cycle): each index is
/// collected at most once.
fn transitive_riders<'a>(
    start: usize,
    riders_of: &[Vec<usize>],
    views: &'a [RideView<'a>],
) -> Vec<Rider<'a>> {
    let mut collected = Vec::new();
    let mut visited = vec![false; views.len()];
    let mut stack = riders_of[start].clone();
    while let Some(idx) = stack.pop() {
        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        collected.push(Rider {
            tool: views[idx].recorded.tool,
            instance: &views[idx].recorded.instance,
            inputs: views[idx].recorded.inputs,
            outputs: views[idx].recorded.outputs,
        });
        stack.extend(riders_of[idx].iter().copied());
    }
    collected
}

/// What the ride fixed point below settled one instance to, for its own
/// riders' sake (milestone 3m task R5b): whether its own resource goes
/// away, is kept by the document, or neither.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Settled {
    /// This instance's own resource goes away this teardown: a genuine
    /// `Delete`, or [`TeardownAction::RidesWith`] something that does. A
    /// rider matching this instance rides with it in turn.
    GoesAway,
    /// The document keeps this instance (decision 3a): a rider matching
    /// it is [`KEPT_RIDER`]-kept too, never refused, and never rides.
    Kept,
    /// Anything else (`Retained`, `Gone`, a refusal, or no step at all):
    /// a rider matching this is decided exactly as if it matched
    /// nothing.
    Other,
}

/// One instance's own resolved place in the ride fixed point: its
/// [`Settled`] state (for its own riders), the [`Decision`] it itself
/// produces, if any (`None` for "not a step at all"), and -- milestone
/// 3m task R6 -- what its own `standing()` call reported, if it called
/// one (`None` for every branch that never observes anything:
/// `RidesWith`, `Kept`-by-matching-a-kept-container, a refusal, or no
/// step at all).
struct Resolution {
    settled: Settled,
    outcome: Option<Decision>,
    standing: Option<StandingKind>,
}

impl Resolution {
    fn other(outcome: Option<Decision>) -> Self {
        Self {
            settled: Settled::Other,
            outcome,
            standing: None,
        }
    }
}

/// Decide every instance's own place in the teardown (milestone 3m
/// decisions 1, 2, 3, 3a, 4's gate rows, 6): gates, `Compute`/`NoOp`/
/// `Skip`/`Blocked`, `Replace`/`Update` and an unfinished instance settle
/// on their own, exactly as before milestone 3m task R5b. Every other
/// `Create`-actioned instance (`Created`, `Failed`, or `Unchanged` in
/// transit) is resolved against [`container_of`]`[idx]`'s own
/// [`Resolution`] once that is known -- in any order, never only reverse
/// `Plan::nodes` order, since riding needs no graph (decision 3) and a
/// container can be declared, and resolved, after its rider in that
/// order. A pass that makes no progress with instances still unresolved
/// means two or more ride each other with nothing else grounding either
/// (the X1 addendum's own risk): the earliest such index is force-decided
/// as if it matched nothing at all, which can only ever break a cycle,
/// never produce a mutual ride (milestone 3m task R5b's own acceptance
/// test pins this).
fn resolve_actions(
    secrets: &Secrets,
    views: &[RideView],
    container_of: &[Option<usize>],
    riders_of: &[Vec<usize>],
) -> Result<Vec<Resolution>, TeardownError> {
    let mut resolved: Vec<Option<Resolution>> = (0..views.len()).map(|_| None).collect();

    loop {
        let mut progress = false;
        for idx in 0..views.len() {
            if resolved[idx].is_some() {
                continue;
            }
            if let Some(resolution) =
                try_resolve(secrets, views, idx, container_of[idx], riders_of, &resolved)?
            {
                resolved[idx] = Some(resolution);
                progress = true;
            }
        }
        if progress {
            continue;
        }
        // Force the earliest unresolved index to ignore its own match
        // (the only way two instances could still be stuck on each
        // other): breaks any cycle, since a forced resolution can never
        // itself be pending.
        let Some(stuck) = resolved.iter().position(Option::is_none) else {
            break;
        };
        resolved[stuck] = Some(standalone_resolution(secrets, views, stuck, riders_of)?);
    }

    // Every entry is `Some` here: the `loop` above only `break`s once
    // `resolved.iter().position(Option::is_none)` itself finds none.
    // `flatten` keeps exactly that already-proven-complete sequence, with
    // no fallback for the case it cannot reach.
    Ok(resolved.into_iter().flatten().collect())
}

/// Try to resolve `idx`, given what is resolved so far; `None` when its
/// own matched container is itself still unresolved (try again next
/// pass).
fn try_resolve(
    secrets: &Secrets,
    views: &[RideView],
    idx: usize,
    container: Option<usize>,
    riders_of: &[Vec<usize>],
    resolved: &[Option<Resolution>],
) -> Result<Option<Resolution>, TeardownError> {
    let Some(container) = container else {
        return standalone_resolution(secrets, views, idx, riders_of).map(Some);
    };
    let Some(container_resolution) = &resolved[container] else {
        return Ok(None);
    };
    let view = &views[idx];
    let container_ref = views[container].recorded.instance.clone();
    // Decision 3a's "must say `teardown: delete` or refuse" rule applies
    // only to a genuine `Delete`-capable candidate, never to a transit
    // rider (`view.transit`) or a `Contained` instance.
    let candidate =
        !view.transit && matches!(view.checked_node.tool.reversal(), Reversal::Delete { .. });
    match container_resolution.settled {
        Settled::GoesAway => {
            if candidate {
                match &view.checked_node.teardown {
                    crate::workflow::TeardownPolicy::Retain => {
                        return Ok(Some(Resolution::other(Some(Decision::Refused(
                            RIDER_KEPT_INSIDE_DELETED_CONTAINER,
                        )))));
                    }
                    crate::workflow::TeardownPolicy::Delete { .. } => {}
                }
            }
            Ok(Some(Resolution {
                settled: Settled::GoesAway,
                outcome: Some(Decision::Step(TeardownAction::RidesWith {
                    container: container_ref,
                })),
                // R5b's own addendum: a `RidesWith` step's own `standing`
                // is never called -- only the chain's grounding instance
                // observes anything.
                standing: None,
            }))
        }
        Settled::Kept => {
            if candidate {
                // "A `Delete`-capable candidate under `teardown: delete`
                // whose container is kept is its own `Delete` step"
                // (decision 3a): decide exactly as if it matched nothing.
                return standalone_resolution(secrets, views, idx, riders_of).map(Some);
            }
            Ok(Some(Resolution {
                settled: Settled::Kept,
                outcome: Some(Decision::Step(TeardownAction::Kept { why: KEPT_RIDER })),
                standing: None,
            }))
        }
        Settled::Other => standalone_resolution(secrets, views, idx, riders_of).map(Some),
    }
}

/// Resolve `idx` as if it matched no container at all: a genuine
/// candidate's own capability × policy × standing (via
/// [`decide_standalone`], attaching its own transitive riders), a
/// `Contained` instance's `otherwise`, or -- for a transit rider with no
/// match -- no step at all (decision 1: "nothing was created").
fn standalone_resolution(
    secrets: &Secrets,
    views: &[RideView],
    idx: usize,
    riders_of: &[Vec<usize>],
) -> Result<Resolution, TeardownError> {
    let view = &views[idx];
    let instance = view.run_instance;
    let Some((started, finished, status)) = instance_window(instance) else {
        // Never reached: every `RideView` here comes from a `Finished`
        // instance (`build_ride_views` only builds one for those).
        return Ok(Resolution::other(None));
    };
    if matches!(status, crate::apply::NodeStatus::Unchanged) {
        // A transit rider matching nothing: decision 1's "not this
        // node's creation", so it is not a step at all.
        return Ok(Resolution::other(None));
    }
    let uncertain = matches!(status, crate::apply::NodeStatus::Failed { .. });
    let riders = transitive_riders(idx, riders_of, views);
    let (decision, standing) = decide_standalone(
        secrets,
        instance,
        view.checked_node,
        started,
        finished,
        uncertain,
        &riders,
    )?;
    let settled = match &decision {
        Decision::Step(TeardownAction::Delete { .. }) => Settled::GoesAway,
        Decision::Step(TeardownAction::Kept { .. }) => Settled::Kept,
        _ => Settled::Other,
    };
    Ok(Resolution {
        settled,
        outcome: Some(decision),
        standing,
    })
}

/// `instance`'s own `(started, finished, status)` when it finished at
/// all; `None` for [`RunProgress::NotFinished`] (never reached from the
/// ride resolver, which only ever builds a [`RideView`] for a finished
/// instance).
fn instance_window(
    instance: &RunInstance,
) -> Option<(
    crate::apply::Timestamp,
    crate::apply::Timestamp,
    &crate::apply::NodeStatus,
)> {
    match &instance.progress {
        RunProgress::Finished {
            started,
            finished,
            status,
        } => Some((*started, *finished, status)),
        RunProgress::NotFinished { .. } => None,
    }
}

/// Every `Create`-actioned, finished instance of `facts` whose status is
/// `Created`, `Failed` or `Unchanged` (milestone 3m task R5b: the ride
/// resolver's own candidate and transit-rider universe), reduced to a
/// [`RideView`] -- `None` for every other instance, which the ride
/// resolver never touches: `plan_teardown`'s own main loop still decides
/// gates, `Compute`/`NoOp`/`Skip`/`Blocked`, `Replace`/`Update` and an
/// unfinished instance exactly as before this task.
fn build_ride_views<'a>(
    checked: &'a crate::check::Checked,
    facts: &'a RunFacts,
) -> Vec<Option<RideView<'a>>> {
    facts
        .instances
        .iter()
        .map(|instance| {
            let checked_node = find_checked_node(checked, &instance.node)?;
            if checked_node.tool.gate().is_some()
                || !matches!(instance.action, crate::plan::Action::Create)
            {
                return None;
            }
            let (_, _, status) = instance_window(instance)?;
            let transit = matches!(status, crate::apply::NodeStatus::Unchanged);
            let wrote = transit || matches!(status, crate::apply::NodeStatus::Created);
            if !wrote && !matches!(status, crate::apply::NodeStatus::Failed { .. }) {
                // `Converged`, `Blocked`, `Skipped`, `NotRun`, `Computed`:
                // outside the ride resolver's own universe (decision 1).
                return None;
            }
            Some(RideView {
                recorded: Recorded {
                    tool: checked_node.tool.spec().name.as_str(),
                    instance: crate::apply::InstanceRef {
                        node: instance.node.clone(),
                        instance: instance.instance.clone(),
                    },
                    inputs: &instance.inputs,
                    outputs: &instance.outputs,
                },
                checked_node,
                run_instance: instance,
                wrote,
                transit,
            })
        })
        .collect()
}

/// Push one step onto both `steps` and its own [`StepFingerprint`] onto
/// `fingerprint`, in lockstep (milestone 3m task R6) -- every push site in
/// [`plan_teardown_steps`] needs exactly this pair, and factoring it out
/// is what keeps that function under clippy's own `too_many_lines`.
fn record_step(
    steps: &mut Vec<TeardownStep>,
    fingerprint: &mut Vec<StepFingerprint>,
    instance: crate::apply::InstanceRef,
    action: TeardownAction,
    standing: Option<StandingKind>,
) {
    let action_kind = TeardownActionKind::from(&action);
    steps.push(TeardownStep {
        instance: instance.clone(),
        action,
    });
    fingerprint.push(StepFingerprint {
        instance,
        action: action_kind,
        standing,
    });
}

/// The ride resolver's own setup and fixed point, factored out of
/// [`plan_teardown_steps`] so that function stays under clippy's own
/// `too_many_lines` (milestone 3m task R6): builds `secrets`, runs the
/// riding fixed point ([`resolve_actions`]) over every candidate and
/// transit rider `build_ride_views` admits, and returns each one's own
/// [`Resolution`], keyed by its original `facts.instances` index -- the
/// same map [`plan_teardown_steps`]'s own loop (unchanged otherwise)
/// looks up by `facts_idx`.
fn resolve_instances(
    secrets: &Secrets<'_>,
) -> Result<std::collections::HashMap<usize, Resolution>, TeardownError> {
    let ride_views = build_ride_views(secrets.checked, secrets.facts);
    // The ride resolver's own universe: `Create`-actioned, finished,
    // `Created`/`Failed`/`Unchanged` instances only (every `Some` entry
    // above). `container_of`/`riders_of` index this sub-sequence, not
    // `facts.instances` itself, so riding needs no graph over the full
    // run (decision 3) and a gate or a `Replace`/`Update` instance is
    // never a candidate container or rider.
    let (ride_indices, ride_views): (Vec<usize>, Vec<RideView>) = ride_views
        .into_iter()
        .enumerate()
        .filter_map(|(idx, view)| view.map(|view| (idx, view)))
        .unzip();
    let container_of = container_of(&ride_views);
    let riders_of = riders_of(&ride_views, &container_of);
    let resolutions = resolve_actions(secrets, &ride_views, &container_of, &riders_of)?;
    Ok(ride_indices.into_iter().zip(resolutions).collect())
}

/// The shared computation behind [`plan_teardown`] and
/// [`plan_teardown_fingerprint`] (milestone 3m task R6): everything
/// [`plan_teardown`]'s own doc comment describes, returning each step
/// alongside its own [`StepFingerprint`] -- `plan_teardown` keeps only
/// the former (its signature is frozen, SHARED VALUES), so this is the
/// one place either public function's own logic lives. No behaviour
/// changes from before this task: the fingerprint is read straight off
/// values ([`Resolution::standing`]) each branch below already computed,
/// never a second round of `standing()` calls.
fn plan_teardown_steps(
    facts: &RunFacts,
    checked: &crate::check::Checked,
    resolved: &crate::plan::Resolved,
) -> Result<(Vec<TeardownStep>, Vec<StepFingerprint>), TeardownError> {
    let (single_names, each_names) = crate::plan::node_names_by_shape(checked);
    let secrets = Secrets {
        checked,
        resolved,
        facts,
        single_names: &single_names,
        each_names: &each_names,
    };

    let mut by_facts_index = resolve_instances(&secrets)?;

    let mut steps: Vec<TeardownStep> = Vec::new();
    let mut fingerprint: Vec<StepFingerprint> = Vec::new();
    let mut refusals: Vec<(crate::apply::InstanceRef, &'static str)> = Vec::new();

    for (facts_idx, instance) in facts.instances.iter().enumerate().rev() {
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
                record_step(
                    &mut steps,
                    &mut fingerprint,
                    instance_ref,
                    TeardownAction::MadeByHand { need: gate.need },
                    None,
                );
            }
            continue;
        }

        match instance.action {
            crate::plan::Action::Compute
            | crate::plan::Action::NoOp
            | crate::plan::Action::Skip
            | crate::plan::Action::Blocked => continue,
            crate::plan::Action::Replace | crate::plan::Action::Update => {
                record_step(
                    &mut steps,
                    &mut fingerprint,
                    instance_ref,
                    TeardownAction::Retained {
                        why: CHANGED_IN_PLACE,
                    },
                    None,
                );
                continue;
            }
            crate::plan::Action::Create => {}
        }

        if matches!(
            instance.progress,
            RunProgress::NotFinished { started: None }
        ) {
            continue;
        }
        if matches!(
            instance.progress,
            RunProgress::NotFinished { started: Some(_) }
        ) {
            record_step(
                &mut steps,
                &mut fingerprint,
                instance_ref,
                TeardownAction::Unfinished,
                None,
            );
            continue;
        }

        let Some(resolution) = by_facts_index.remove(&facts_idx) else {
            // Not in the ride resolver's own universe at all (status
            // `Converged`, `Blocked`, `Skipped` or `NotRun`): nothing was
            // created (decision 1).
            continue;
        };
        match resolution.outcome {
            Some(Decision::Step(action)) => {
                record_step(
                    &mut steps,
                    &mut fingerprint,
                    instance_ref,
                    action,
                    resolution.standing,
                );
            }
            Some(Decision::Refused(why)) => refusals.push((instance_ref, why)),
            None => {}
        }
    }

    if !refusals.is_empty() {
        return Err(TeardownError::Refused { steps: refusals });
    }
    Ok((steps, fingerprint))
}

/// Compute a teardown plan from one applied run's own journal facts
/// (milestone 3m decisions 1, 2, 3, 3a, 4's gate rows, and 6).
///
/// Steps come out in reverse of `facts.instances`' own (forward,
/// applied-plan) order. Per candidate, capability × policy × standing ×
/// riding decides the action (decisions 6, 3, 3a; `resolve_actions`):
/// a gate instance the run found satisfied is [`TeardownAction::MadeByHand`]
/// (decision 4); `Replace`/`Update` is always [`TeardownAction::Retained`]
/// (decision 1); a `Create`d, `Create`+`Failed` or `Create`+`Unchanged`
/// instance is resolved against what it structurally rides with (decision
/// 3): `Retained` for a tool that cannot undo it, [`TeardownAction::RidesWith`]
/// a container that itself goes away, [`TeardownAction::Kept`] for a
/// rider of a kept container or a node the document keeps outright, a
/// refusal when the document keeps a candidate riding inside a container
/// this teardown deletes (decision 3a), or -- matching nothing -- its own
/// standalone `Delete`/`Gone`/refusal (observed through `standing`, with
/// its own riders tolerated) or, for an unmatched transit rider, no step
/// at all. A started instance with no `NodeFinished` is
/// [`TeardownAction::Unfinished`] (decisions 1, 2), unconditionally.
/// Every other instance (a `Create` that finished `Converged`, `NotRun`,
/// `Skipped` or `Blocked`, or any `Compute`/`NoOp`/`Skip` instance that is
/// not a satisfied gate) is not a step at all.
///
/// # Errors
///
/// [`TeardownError::Refused`] when any observed candidate's standing
/// refuses (decision 6), including decision 3a's "kept inside a deleted
/// container": every refusing instance is collected, not only the first
/// -- trust boundary 6 observes every delete-policy candidate before any
/// delete, plan time included, so this function never returns on the
/// first refusal it finds. [`TeardownError::UnresolvableCredential`] or
/// [`TeardownError::Resolve`] when a `Delete` step's own secret `needs`
/// port cannot be honestly re-resolved (decision 5); either one stops
/// this function immediately, since it names a document defect, never
/// drift. [`TeardownError::Tool`] when a `Delete` tool's own
/// `inverse().standing` call itself fails.
pub fn plan_teardown(
    facts: &RunFacts,
    checked: &crate::check::Checked,
    resolved: &crate::plan::Resolved,
) -> Result<Teardown, TeardownError> {
    let (steps, _fingerprint) = plan_teardown_steps(facts, checked, resolved)?;
    Ok(Teardown { steps })
}

/// Same computation as [`plan_teardown`], also returning each step's own
/// [`StepFingerprint`] (milestone 3m task R6; decision 8: "the apply-time
/// comparison reads the typed `StepFingerprint`"). The caller that
/// approves a teardown (`willikins-server`'s future
/// `Butler::plan_rollback`, task R8) keeps the fingerprint alongside the
/// [`Teardown`] it shows the operator, and passes it back to
/// [`apply_teardown`] at apply time.
///
/// # Errors
///
/// Exactly [`plan_teardown`]'s own.
pub fn plan_teardown_fingerprint(
    facts: &RunFacts,
    checked: &crate::check::Checked,
    resolved: &crate::plan::Resolved,
) -> Result<(Teardown, Vec<StepFingerprint>), TeardownError> {
    let (steps, fingerprint) = plan_teardown_steps(facts, checked, resolved)?;
    Ok((Teardown { steps }, fingerprint))
}

// ---------------------------------------------------------------------
// apply_teardown (milestone 3m task R6)
// ---------------------------------------------------------------------

/// What [`apply_teardown`] ran -- or chose not to run -- one approved
/// [`StepFingerprint`] into (milestone 3m decision 8): the five outcomes
/// `TeardownStepFinished` (task R7) journals directly, unwrapped, the
/// same way [`crate::apply::NodeStatus`] already sits on `NodeFinished`
/// (that type's own doc: its only payload, `ToolError`, is "a plain
/// message with no secret obligation of its own").
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeardownStepOutcome {
    /// A `Delete` step's own `inverse().delete` deleted the resource.
    Deleted,
    /// A `Delete` step's own `inverse().delete` found it already gone --
    /// a resumed teardown calling `delete` on a step an earlier,
    /// half-finished one already deleted (decision 8).
    AlreadyGone,
    /// A `Delete` step's own `inverse().delete` failed: this teardown
    /// stops here. Every `Delete` step after this one in `approved`'s own
    /// order is [`Self::NotRun`].
    Failed {
        /// The failure.
        error: crate::tool::ToolError,
    },
    /// A `Delete` step that never ran because an earlier one failed.
    NotRun,
    /// Any non-`Delete` step (`RidesWith`, `Gone`, `Kept`, `Retained`,
    /// `MadeByHand`, `Unfinished`): recorded, never run, whatever its own
    /// position relative to a failure -- decision 8's "`Listed` for every
    /// non-delete step", unconditionally.
    Listed,
}

/// Why [`apply_teardown`] refused to delete anything, or stopped before
/// finishing a per-step re-resolution (milestone 3m decision 8). Never
/// returned once at least one `delete` call has actually run: a `Delete`
/// step's own failure is [`TeardownStepOutcome::Failed`], part of the
/// `Ok` result, not this type (decision 8: "a delete failure stops the
/// run", never the whole call).
///
/// `Serialize` (milestone 3m task R7): `willikins_journal::Redacted` wraps
/// this for `TeardownFinished`'s own `Failed` outcome, the same one-way
/// half [`Teardown`] and [`TeardownError`] already carry and for the same
/// reason -- see those types' own docs. `Replan` is a struct variant for
/// exactly [`TeardownError::Resolve`]'s own reason: this enum is now
/// internally tagged too, and nesting the inner, also-tagged
/// `TeardownError` under a named field avoids the collision.
#[derive(Debug, Clone, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind")]
pub enum TeardownApplyError {
    /// Re-planning this teardown from the same facts, checked document
    /// and resolved credentials `approved` itself came from failed
    /// outright -- the same way [`plan_teardown`] itself can fail, either
    /// while re-observing every step (the same call [`plan_teardown`]
    /// makes) or while re-resolving a `Delete` step's own secret `needs`
    /// port a second time, just before calling `delete`. Never drift: a
    /// document defect, or a tool failure while merely observing.
    #[error("{error}")]
    Replan {
        /// The failure.
        #[from]
        error: TeardownError,
    },
    /// The freshly re-observed teardown disagrees with what was approved
    /// (milestone 3m decision 8): a different step, a different action,
    /// or the same action with a different `standing` -- a loss that
    /// was not there, or was not reported, when this was approved (the
    /// one case the action alone cannot show: an *accepted* loss never
    /// changes `Delete { uncertain }`). Nothing is deleted.
    #[error(
        "the teardown has drifted since it was approved: re-observing it now gives a different \
         result"
    )]
    Drift,
    /// `approved` names an instance, tool declaration or policy this
    /// run's own facts and checked document no longer support --
    /// defensive: by construction this never happens when `approved`
    /// really is this same call's own freshly re-observed fingerprint
    /// (just checked equal to it, above), but `apply_teardown` trusts no
    /// caller's data blindly, the same discipline `apply`'s own
    /// `ApplyError::Drift` already affords a forward run.
    #[error("{instance}: {why}")]
    Inconsistent {
        /// The instance.
        instance: crate::apply::InstanceRef,
        /// What is inconsistent.
        why: &'static str,
    },
}

/// Re-resolve and `delete` one approved `Delete` step, named by
/// `instance_ref` (milestone 3m task R6): rebuilds the very same
/// [`Created`] [`decide_standalone`] built for it at plan time (its own
/// `needs_inputs`, [`CreationWindow`], `settled_by`, `accept` and
/// [`Rider`]s), then calls `inverse().delete`, never `standing` again --
/// `apply_teardown`'s own re-plan, just before this is called, already
/// re-observed every step's `standing` (trust boundary 6: never a
/// `RidesWith` step on its own).
fn delete_one(
    secrets: &Secrets,
    instance_ref: &crate::apply::InstanceRef,
    ride_views: &[RideView],
    riders_of: &[Vec<usize>],
    ride_view_index_of: &std::collections::HashMap<usize, usize>,
    token: &TeardownToken,
) -> Result<TeardownStepOutcome, TeardownApplyError> {
    let facts_idx = secrets
        .facts
        .instances
        .iter()
        .position(|candidate| {
            candidate.node == instance_ref.node && candidate.instance == instance_ref.instance
        })
        .ok_or_else(|| TeardownApplyError::Inconsistent {
            instance: instance_ref.clone(),
            why: "names no instance in this run's own facts",
        })?;
    let instance = &secrets.facts.instances[facts_idx];
    let checked_node = find_checked_node(secrets.checked, &instance.node).ok_or_else(|| {
        TeardownApplyError::Inconsistent {
            instance: instance_ref.clone(),
            why: "names no node in the checked document",
        }
    })?;
    let Reversal::Delete { needs, .. } = checked_node.tool.reversal() else {
        return Err(TeardownApplyError::Inconsistent {
            instance: instance_ref.clone(),
            why: "its tool no longer declares Reversal::Delete",
        });
    };
    let accept = match &checked_node.teardown {
        crate::workflow::TeardownPolicy::Delete { accept } => accept,
        crate::workflow::TeardownPolicy::Retain => {
            return Err(TeardownApplyError::Inconsistent {
                instance: instance_ref.clone(),
                why: "its node's own policy no longer says teardown: delete",
            });
        }
    };
    let inverse = checked_node
        .tool
        .inverse()
        .ok_or_else(|| TeardownApplyError::Inconsistent {
            instance: instance_ref.clone(),
            why: "its tool declares Delete but Tool::inverse returns None",
        })?;
    let (started, finished, _status) =
        instance_window(instance).ok_or_else(|| TeardownApplyError::Inconsistent {
            instance: instance_ref.clone(),
            why: "has no recorded NodeFinished",
        })?;
    let window = CreationWindow {
        start: shift(started, -CREATION_SKEW),
        end: shift(finished, CREATION_SKEW),
    };
    let settled_by = shift(secrets.facts.finished_at, CREATION_SKEW);
    let needs_inputs = secrets.build_needs_inputs(instance, checked_node, needs)?;
    let riders = ride_view_index_of
        .get(&facts_idx)
        .map(|&view_idx| transitive_riders(view_idx, riders_of, ride_views))
        .unwrap_or_default();
    let created = Created {
        inputs: &needs_inputs,
        outputs: &instance.outputs,
        window,
        settled_by,
        accept,
        riders: &riders,
    };
    Ok(match inverse.delete(&created, token) {
        Ok(Deleted { already_gone: true }) => TeardownStepOutcome::AlreadyGone,
        Ok(Deleted {
            already_gone: false,
        }) => TeardownStepOutcome::Deleted,
        Err(error) => TeardownStepOutcome::Failed { error },
    })
}

/// Apply an approved teardown (milestone 3m decision 8's apply; trust
/// boundaries 1, 2, 6): re-plans from the same `facts`, `checked` and
/// `resolved` `approved` itself came from (re-observing exactly the set
/// [`plan_teardown`] observed: every candidate whose node says
/// `teardown: delete` and that is not itself a rider -- never a
/// `RidesWith` step on its own, trust boundary 6), refuses with
/// [`TeardownApplyError::Drift`] when the fresh result disagrees with
/// `approved` (standing kinds and losses included, via
/// [`StepFingerprint`]), mints the one
/// [`willikins_types::TeardownToken`] this module ever mints, and runs
/// every `Delete` step in `approved`'s own order, stopping at the first
/// failure (decision 8). A non-`Delete` step is always
/// [`TeardownStepOutcome::Listed`].
///
/// # Errors
///
/// [`TeardownApplyError::Replan`] when re-planning (or a per-step secret
/// re-resolution) fails outright; [`TeardownApplyError::Drift`] when the
/// fresh observation disagrees with `approved`, before any delete;
/// [`TeardownApplyError::Inconsistent`] only defensively (see its own
/// doc). A `Delete` step's own failure is part of the `Ok` result (see
/// [`TeardownStepOutcome::Failed`]), never this error type.
pub fn apply_teardown(
    approved: &[StepFingerprint],
    facts: &RunFacts,
    checked: &crate::check::Checked,
    resolved: &crate::plan::Resolved,
) -> Result<Vec<TeardownStepOutcome>, TeardownApplyError> {
    let (_, fresh) = plan_teardown_steps(facts, checked, resolved)?;
    if fresh.as_slice() != approved {
        return Err(TeardownApplyError::Drift);
    }

    // The one non-test `TeardownToken::new` call site in the workspace
    // (trust boundary 2; `tests/it/teardown_token_guard.rs`, acceptance
    // test 2's `TeardownToken` half) -- minted only once the fresh
    // observation above has already matched what was approved.
    #[allow(clippy::disallowed_methods)]
    let token = TeardownToken::new();

    let ride_views = build_ride_views(checked, facts);
    let (ride_indices, ride_views): (Vec<usize>, Vec<RideView>) = ride_views
        .into_iter()
        .enumerate()
        .filter_map(|(idx, view)| view.map(|view| (idx, view)))
        .unzip();
    let container_of = container_of(&ride_views);
    let riders_of = riders_of(&ride_views, &container_of);
    let ride_view_index_of: std::collections::HashMap<usize, usize> = ride_indices
        .into_iter()
        .enumerate()
        .map(|(view_idx, facts_idx)| (facts_idx, view_idx))
        .collect();

    let (single_names, each_names) = crate::plan::node_names_by_shape(checked);
    let secrets = Secrets {
        checked,
        resolved,
        facts,
        single_names: &single_names,
        each_names: &each_names,
    };

    let mut outcomes = Vec::with_capacity(approved.len());
    let mut stopped = false;
    for step in approved {
        if !matches!(step.action, TeardownActionKind::Delete { .. }) {
            outcomes.push(TeardownStepOutcome::Listed);
            continue;
        }
        if stopped {
            outcomes.push(TeardownStepOutcome::NotRun);
            continue;
        }
        let outcome = delete_one(
            &secrets,
            &step.instance,
            &ride_views,
            &riders_of,
            &ride_view_index_of,
            &token,
        )?;
        if matches!(outcome, TeardownStepOutcome::Failed { .. }) {
            stopped = true;
        }
        outcomes.push(outcome);
    }
    Ok(outcomes)
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
    use willikins_types::{DomainType, DopplerServiceToken, GitHubRepo, SinkToken};

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

    // -----------------------------------------------------------------
    // Milestone 3m, task R5b: riding (decisions 3, 3a; acceptance 5).
    // -----------------------------------------------------------------

    fn repo_value(s: &str) -> Value {
        Value::known(GitHubRepo::parse(s).unwrap_or_else(|err| unreachable!("{s:?}: {err}")))
    }

    fn inputs_of(pairs: &[(&str, Value)]) -> Inputs {
        let mut inputs = Inputs::new();
        for (p, v) in pairs {
            inputs.insert(port(p), v.clone());
        }
        inputs
    }

    fn outputs_of(pairs: &[(&str, Value)]) -> Outputs {
        let mut outputs = Outputs::new();
        for (p, v) in pairs {
            outputs.insert(port(p), v.clone());
        }
        outputs
    }

    /// A `RunInstance` for `name`, with explicit recorded inputs/outputs
    /// -- unlike [`instance`], which hardcodes node `"repo"` and empty
    /// port maps: riding needs real recorded values to match against
    /// (decision 3's "typed equality on recorded values").
    fn riding_instance(
        name: &str,
        action: Action,
        progress: RunProgress,
        inputs: Inputs,
        outputs: Outputs,
    ) -> RunInstance {
        RunInstance {
            node: node(name),
            instance: None,
            action,
            progress,
            inputs,
            outputs,
        }
    }

    fn facts_multi(instances: Vec<RunInstance>, finished_at: &str) -> RunFacts {
        RunFacts {
            finished_at: ts(finished_at),
            inputs: IndexMap::new(),
            instances,
        }
    }

    /// Shared by [`StubProjectTool`] and [`StubConfigTool`]: a fixed
    /// `Standing` and a shared call counter (acceptance 5, 6, 19's own
    /// "zero `standing` calls" assertions, extended to riding: a step
    /// that rides is never itself observed).
    struct RidingInverse {
        standing: Standing,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Inverse for RidingInverse {
        fn standing(&self, _created: &Created<'_>) -> Result<Standing, crate::tool::ToolError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.standing.clone())
        }

        fn delete(
            &self,
            _created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, crate::tool::ToolError> {
            // Milestone 3m task R6: shares `calls` with `standing` rather
            // than adding a second counter -- every pre-existing test
            // using this inverse only ever calls `plan_teardown` (never
            // `apply_teardown`), so `delete` was never reached and this
            // changes none of their own assertions; a later test that
            // asserts "this inverse's own `delete` was never called"
            // needs `calls` to actually reflect that.
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Deleted {
                already_gone: matches!(self.standing, Standing::Gone),
            })
        }
    }

    /// An inverse that tolerates exactly `expected_riders` of
    /// `Created::riders` and reports `Drifted` otherwise (milestone 3m
    /// task R5b, acceptance 5's "a `NotRun` `secret.set` plus a hand-set
    /// secret at its name refuses"): stands in for a real provider read
    /// that finds one content this run's own riders do not account for.
    struct ToleranceInverse {
        expected_riders: usize,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Inverse for ToleranceInverse {
        fn standing(&self, created: &Created<'_>) -> Result<Standing, crate::tool::ToolError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if created.riders.len() == self.expected_riders {
                Ok(Standing::AsCreated { losing: vec![] })
            } else {
                Ok(Standing::Drifted {
                    what: "an untolerated secret name",
                })
            }
        }

        fn delete(
            &self,
            _created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, crate::tool::ToolError> {
            Ok(Deleted {
                already_gone: false,
            })
        }
    }

    /// A stub, top-level `Delete` tool with no `within`: `project` (key),
    /// one port. Holds any [`Inverse`], like [`StubConfigTool`]: tests
    /// that need `Created.riders` to actually reach a real assertion use
    /// a [`ToleranceInverse`] here, never only a fixed [`RidingInverse`].
    struct StubProjectTool {
        spec: ToolSpec,
        inverse: Box<dyn Inverse>,
    }

    impl StubProjectTool {
        fn with_inverse(tool_name: &str, inverse: Box<dyn Inverse>) -> Arc<Self> {
            let mut inputs = IndexMap::new();
            inputs.insert(
                port("project"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(port("project"), ty("GitHubRepo"));
            Arc::new(Self {
                spec: ToolSpec {
                    name: ToolName::parse(tool_name).unwrap(),
                    description: "stub project tool (milestone 3m task R5b)".to_string(),
                    inputs,
                    outputs,
                    key: vec![port("project")],
                    class: Class::Reversible,
                    pure: false,
                },
                inverse,
            })
        }

        fn with_counter(
            tool_name: &str,
            standing: Standing,
            calls: Arc<std::sync::atomic::AtomicUsize>,
        ) -> Arc<Self> {
            Self::with_inverse(tool_name, Box::new(RidingInverse { standing, calls }))
        }
    }

    impl Tool for StubProjectTool {
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
                needs: &["project"],
                within: &[],
                losses: &[],
            }
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            Some(self.inverse.as_ref())
        }
    }

    /// A stub `Delete` tool riding within `test.project`'s own `project`
    /// port: `config` (key), `project` (its own ride-matching input).
    struct StubConfigTool {
        spec: ToolSpec,
        inverse: Box<dyn Inverse>,
    }

    impl StubConfigTool {
        fn with_inverse(inverse: Box<dyn Inverse>) -> Arc<Self> {
            let mut inputs = IndexMap::new();
            inputs.insert(
                port("config"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            inputs.insert(
                port("project"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(port("config"), ty("GitHubRepo"));
            Arc::new(Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.config").unwrap(),
                    description: "stub config tool (milestone 3m task R5b)".to_string(),
                    inputs,
                    outputs,
                    key: vec![port("config")],
                    class: Class::Reversible,
                    pure: false,
                },
                inverse,
            })
        }

        fn with_standing(
            standing: Standing,
            calls: Arc<std::sync::atomic::AtomicUsize>,
        ) -> Arc<Self> {
            Self::with_inverse(Box::new(RidingInverse { standing, calls }))
        }
    }

    impl Tool for StubConfigTool {
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
                needs: &["config"],
                within: &[Within {
                    tool: "test.project",
                    ports: &[("project", Side::Input, "project")],
                }],
                losses: &[],
            }
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            Some(self.inverse.as_ref())
        }
    }

    /// A stub `Contained` tool riding within `test.config`'s own `config`
    /// port: `secret` (key), `config` (its own ride-matching input). No
    /// inverse: `Contained` never has one.
    struct StubSecretTool {
        spec: ToolSpec,
    }

    impl StubSecretTool {
        fn new() -> Arc<Self> {
            let mut inputs = IndexMap::new();
            inputs.insert(
                port("secret"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            inputs.insert(
                port("config"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(port("secret"), ty("GitHubRepo"));
            Arc::new(Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.secret").unwrap(),
                    description: "stub secret tool (milestone 3m task R5b)".to_string(),
                    inputs,
                    outputs,
                    key: vec![port("secret")],
                    class: Class::Reversible,
                    pure: false,
                },
            })
        }
    }

    impl Tool for StubSecretTool {
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
            Reversal::Contained {
                within: &[Within {
                    tool: "test.config",
                    ports: &[("config", Side::Input, "config")],
                }],
                otherwise: "left in place, outside its container",
            }
        }
    }

    /// A `project`/`config`/`secret` three-node document (milestone 3m
    /// task R5b's own riding fixtures). Binding literal values are
    /// arbitrary (`check` only needs them syntactically valid): riding
    /// matches on each `RunInstance`'s own recorded values, never on a
    /// check-time binding (decision 3, "needs no graph").
    fn checked_riding(
        project: Arc<StubProjectTool>,
        config: Arc<StubConfigTool>,
        secret: Arc<StubSecretTool>,
        project_policy: TeardownPolicy,
        config_policy: Option<TeardownPolicy>,
    ) -> Checked {
        let project_node = Node::new(project.spec.name.clone())
            .port(
                port("project"),
                Binding::Literal("example-org/example-project".to_string()),
            )
            .teardown(project_policy);
        let mut config_node = Node::new(config.spec.name.clone())
            .port(
                port("config"),
                Binding::Literal("example-org/example-config".to_string()),
            )
            .port(
                port("project"),
                Binding::Literal("example-org/example-project".to_string()),
            );
        if let Some(policy) = config_policy {
            config_node = config_node.teardown(policy);
        }
        let secret_node = Node::new(secret.spec.name.clone())
            .port(
                port("secret"),
                Binding::Literal("example-org/example-secret".to_string()),
            )
            .port(
                port("config"),
                Binding::Literal("example-org/example-config".to_string()),
            );
        let workflow = Workflow::new(workflow_name("riding-test"))
            .node(node("project"), project_node)
            .node(node("config"), config_node)
            .node(node("secret"), secret_node);
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(project as Arc<dyn Tool>).unwrap();
        catalog.insert(config as Arc<dyn Tool>).unwrap();
        catalog.insert(secret as Arc<dyn Tool>).unwrap();
        check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("riding workflow must check cleanly: {errors:?}"))
    }

    /// `rides_with` unit tests: typed equality, `Unknown` never matching,
    /// a `Within` never matching its own instance, and `Side::Output`
    /// reading the container's own outputs.
    mod rides_with_tests {
        use super::*;

        fn checked_pair() -> (Checked, Arc<std::sync::atomic::AtomicUsize>) {
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let project =
                StubProjectTool::with_counter("test.project", Standing::Gone, Arc::clone(&calls));
            let config = StubConfigTool::with_standing(Standing::Gone, Arc::clone(&calls));
            let secret = StubSecretTool::new();
            (
                checked_riding(
                    project,
                    config,
                    secret,
                    TeardownPolicy::Delete { accept: vec![] },
                    Some(TeardownPolicy::Delete { accept: vec![] }),
                ),
                calls,
            )
        }

        /// All `rides_with` itself ever reads, built the same way
        /// `build_ride_views` builds one's own `Recorded`: the tool name
        /// comes from `checked`'s own node, never guessed.
        fn view<'a>(checked: &'a Checked, node_name: &str, run: &'a RunInstance) -> Recorded<'a> {
            let checked_node = checked
                .nodes()
                .iter()
                .find(|n| n.name.as_str() == node_name)
                .unwrap_or_else(|| unreachable!("{node_name} must exist"));
            Recorded {
                tool: checked_node.tool.spec().name.as_str(),
                instance: crate::apply::InstanceRef {
                    node: node(node_name),
                    instance: None,
                },
                inputs: &run.inputs,
                outputs: &run.outputs,
            }
        }

        #[test]
        fn equal_typed_values_ride() {
            let (checked, _calls) = checked_pair();
            let project_run = riding_instance(
                "project",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[("project", repo_value("org/same"))]),
                outputs_of(&[("project", repo_value("org/same"))]),
            );
            let config_run = riding_instance(
                "config",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[
                    ("config", repo_value("org/config")),
                    ("project", repo_value("org/same")),
                ]),
                outputs_of(&[("config", repo_value("org/config"))]),
            );
            let project_view = view(&checked, "project", &project_run);
            let config_view = view(&checked, "config", &config_run);
            let within = Within {
                tool: "test.project",
                ports: &[("project", Side::Input, "project")],
            };
            assert!(rides_with(&within, &config_view, &project_view));
        }

        #[test]
        fn mismatched_values_do_not_ride() {
            let (checked, _calls) = checked_pair();
            let project_run = riding_instance(
                "project",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[("project", repo_value("org/one"))]),
                outputs_of(&[("project", repo_value("org/one"))]),
            );
            let config_run = riding_instance(
                "config",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[
                    ("config", repo_value("org/config")),
                    ("project", repo_value("org/two")),
                ]),
                outputs_of(&[("config", repo_value("org/config"))]),
            );
            let project_view = view(&checked, "project", &project_run);
            let config_view = view(&checked, "config", &config_run);
            let within = Within {
                tool: "test.project",
                ports: &[("project", Side::Input, "project")],
            };
            assert!(!rides_with(&within, &config_view, &project_view));
        }

        #[test]
        fn unknown_values_never_ride() {
            let (checked, _calls) = checked_pair();
            let unknown = Value::unknown(ty("GitHubRepo"));
            let project_run = riding_instance(
                "project",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[("project", unknown.clone())]),
                outputs_of(&[("project", unknown.clone())]),
            );
            let config_run = riding_instance(
                "config",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[
                    ("config", repo_value("org/config")),
                    ("project", unknown.clone()),
                ]),
                outputs_of(&[("config", repo_value("org/config"))]),
            );
            let project_view = view(&checked, "project", &project_run);
            let config_view = view(&checked, "config", &config_run);
            let within = Within {
                tool: "test.project",
                ports: &[("project", Side::Input, "project")],
            };
            // Both sides carry the *same* `Unknown` value, which `Value`'s
            // own `PartialEq` holds equal -- `rides_with` must still
            // refuse, since neither side is `is_known`.
            assert!(!rides_with(&within, &config_view, &project_view));
        }

        #[test]
        fn an_instance_never_rides_with_itself() {
            let (checked, _calls) = checked_pair();
            let run = riding_instance(
                "project",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[("project", repo_value("org/same"))]),
                outputs_of(&[("project", repo_value("org/same"))]),
            );
            let project_view = view(&checked, "project", &run);
            let within = Within {
                tool: "test.project",
                ports: &[("project", Side::Input, "project")],
            };
            assert!(!rides_with(&within, &project_view, &project_view));
        }

        #[test]
        fn side_output_reads_the_containers_own_output() {
            let (checked, _calls) = checked_pair();
            // The container's own `project` *input* differs from its
            // `project` *output* -- only `Side::Output` matches here.
            let project_run = riding_instance(
                "project",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[("project", repo_value("org/input-side"))]),
                outputs_of(&[("project", repo_value("org/output-side"))]),
            );
            let config_run = riding_instance(
                "config",
                Action::Create,
                finished(START, END, NodeStatus::Created),
                inputs_of(&[
                    ("config", repo_value("org/config")),
                    ("project", repo_value("org/output-side")),
                ]),
                outputs_of(&[("config", repo_value("org/config"))]),
            );
            let project_view = view(&checked, "project", &project_run);
            let config_view = view(&checked, "config", &config_run);
            let input_side = Within {
                tool: "test.project",
                ports: &[("project", Side::Input, "project")],
            };
            let output_side = Within {
                tool: "test.project",
                ports: &[("project", Side::Output, "project")],
            };
            assert!(!rides_with(&input_side, &config_view, &project_view));
            assert!(rides_with(&output_side, &config_view, &project_view));
        }
    }

    /// Acceptance 5: a config rides with the project created in the same
    /// run, and the project's own inverse tolerates it (`Created.riders`).
    #[test]
    fn config_rides_with_the_project_created_in_the_same_run() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Tolerates exactly one rider: `config` really must reach
        // `Created.riders`, not merely be labelled `RidesWith` -- a
        // `transitive_riders` bug that returned nothing would mismatch
        // this and refuse instead of deleting.
        let project = StubProjectTool::with_inverse(
            "test.project",
            Box::new(ToleranceInverse {
                expected_riders: 1,
                calls: Arc::clone(&calls),
            }),
        );
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(Standing::Gone, Arc::clone(&config_calls));
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let run = facts_multi(vec![project_run, config_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        let project_ref = InstanceRef {
            node: node("project"),
            instance: None,
        };
        let config_ref = InstanceRef {
            node: node("config"),
            instance: None,
        };
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: config_ref,
                    action: TeardownAction::RidesWith {
                        container: project_ref.clone()
                    },
                },
                TeardownStep {
                    instance: project_ref,
                    action: TeardownAction::Delete { uncertain: false },
                },
            ]
        );
        // The config rode with the project; the project's own `standing`
        // tolerated it, and the config's own `standing` was never called
        // at all (a rider is never itself observed).
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Acceptance 5: a config with no `teardown:` inside a deleted
    /// project refuses the teardown with decision 3a's own reason.
    #[test]
    fn a_kept_config_inside_a_deleted_project_refuses() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let project = StubProjectTool::with_counter(
            "test.project",
            Standing::AsCreated { losing: vec![] },
            Arc::clone(&calls),
        );
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(Standing::Gone, Arc::clone(&config_calls));
        let secret = StubSecretTool::new();
        // No `teardown:` at all on `config`: the conservative default,
        // `retain`.
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            None,
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let run = facts_multi(vec![project_run, config_run], RUN_FINISHED);

        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::Refused { steps } => {
                assert_eq!(steps.len(), 1);
                assert_eq!(steps[0].0.node, node("config"));
                assert_eq!(steps[0].1, RIDER_KEPT_INSIDE_DELETED_CONTAINER);
            }
            other => panic!("expected Refused, got {other:?}"),
        }
        // A kept candidate's own standing is never called, whether it
        // rides into something or not.
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Acceptance 5: a `NotRun` `secret.set` plus a hand-set secret at
    /// its name refuses -- the config's own inverse tolerates exactly the
    /// riders this run's own record vouches for, never the `NotRun` one.
    #[test]
    fn a_notrun_rider_plus_an_untolerated_content_refuses() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let project =
            StubProjectTool::with_counter("test.project", Standing::Gone, Arc::clone(&calls));
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // The stand-in provider read expects one tolerated rider (as if
        // this run's own `secret.set` had run); since it did not, the
        // config's own riders are empty and this refuses.
        let config = StubConfigTool::with_inverse(Box::new(ToleranceInverse {
            expected_riders: 1,
            calls: Arc::clone(&config_calls),
        }));
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            project,
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        // No `project` instance in this run's own facts (pre-existing,
        // never a candidate container): `config` is standalone, so its
        // own `standing` really is called, with its own real riders.
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", repo_value("org/pre-existing")),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let secret_run = riding_instance(
            "secret",
            Action::Create,
            RunProgress::NotFinished { started: None },
            Inputs::new(),
            Outputs::new(),
        );
        let run = facts_multi(vec![config_run, secret_run], RUN_FINISHED);

        let err = plan_teardown(&run, &checked, &resolved).unwrap_err();
        match err {
            TeardownError::Refused { steps } => {
                assert_eq!(steps.len(), 1);
                assert_eq!(steps[0].0.node, node("config"));
                assert_eq!(steps[0].1, "an untolerated secret name");
            }
            other => panic!("expected Refused, got {other:?}"),
        }
    }

    /// Acceptance 5: a config inside a pre-existing project (not a
    /// candidate at all this run) is its own standalone `Delete`.
    #[test]
    fn a_config_inside_a_pre_existing_project_is_its_own_delete() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let project =
            StubProjectTool::with_counter("test.project", Standing::Gone, Arc::clone(&calls));
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(
            Standing::AsCreated { losing: vec![] },
            Arc::clone(&config_calls),
        );
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            project,
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        // No `project` instance in this run's own facts at all: it is
        // pre-existing, never a candidate container.
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", repo_value("org/pre-existing")),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let run = facts_multi(vec![config_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("config"),
                    instance: None
                },
                action: TeardownAction::Delete { uncertain: false },
            }]
        );
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Acceptance 5: a `Create`+`Unchanged` root config is a transit
    /// rider, and a secret set into it rides with the project too, two
    /// `Within` hops away (the `new-rust-service` shape): the project's
    /// own `Created.riders` must see it transitively.
    #[test]
    fn a_transit_rider_two_hops_down_still_reaches_the_root_container() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Tolerates exactly two riders: the `Unchanged` root config
        // (direct) and the secret (transitively, through it) -- a
        // `transitive_riders` bug that stopped at the direct match would
        // mismatch this (1, not 2) and refuse instead of deleting.
        let project = StubProjectTool::with_inverse(
            "test.project",
            Box::new(ToleranceInverse {
                expected_riders: 2,
                calls: Arc::clone(&calls),
            }),
        );
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Tolerates exactly one rider: the secret, reached transitively
        // through the `Unchanged` root config -- never the config itself
        // (it is a transit rider, not a candidate, so `riders_of` never
        // lists it as *this* instance's own direct content either; only
        // the project's own riders must include the secret).
        let config = StubConfigTool::with_inverse(Box::new(ToleranceInverse {
            expected_riders: 0,
            calls: Arc::clone(&config_calls),
        }));
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let config_value = repo_value("org/the-root-config");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Unchanged),
            inputs_of(&[
                ("config", config_value.clone()),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", config_value.clone())]),
        );
        let secret_run = riding_instance(
            "secret",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("secret", repo_value("org/the-secret")),
                ("config", config_value.clone()),
            ]),
            outputs_of(&[("secret", repo_value("org/the-secret"))]),
        );
        let run = facts_multi(vec![project_run, config_run, secret_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        let config_ref = InstanceRef {
            node: node("config"),
            instance: None,
        };
        let project_ref = InstanceRef {
            node: node("project"),
            instance: None,
        };
        assert_eq!(teardown.steps.len(), 3);
        assert_eq!(
            teardown.steps[0],
            TeardownStep {
                instance: InstanceRef {
                    node: node("secret"),
                    instance: None
                },
                action: TeardownAction::RidesWith {
                    container: config_ref.clone()
                },
            }
        );
        assert_eq!(
            teardown.steps[1],
            TeardownStep {
                instance: config_ref,
                action: TeardownAction::RidesWith {
                    container: project_ref.clone()
                },
            }
        );
        assert_eq!(
            teardown.steps[2],
            TeardownStep {
                instance: project_ref,
                action: TeardownAction::Delete { uncertain: false },
            }
        );
        // The project's own `standing` tolerated the secret even though
        // it matches `config` directly, not `project`: transitive riders
        // (acceptance 5). The config's own `standing` was never called
        // (it rode, so it was never itself observed).
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Decision 3's "riding is not tolerance": a rider that structurally
    /// matches but `Failed` is still listed `RidesWith` (every matching
    /// instance is), yet never reaches `Created.riders` -- unlike a
    /// `NotRun` rider (which never even builds a `RideView` at all, so it
    /// never exercises `riders_of`'s own `wrote` check), `Failed` is the
    /// one status that reaches that check and is excluded by it.
    #[test]
    fn a_failed_rider_rides_but_is_never_tolerated() {
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Expects zero riders: the `Failed` secret structurally matches
        // but must not count -- a `riders_of` bug that dropped the
        // `wrote` filter would count it (1) and mismatch this, refusing
        // instead of deleting.
        let config = StubConfigTool::with_inverse(Box::new(ToleranceInverse {
            expected_riders: 0,
            calls: Arc::clone(&config_calls),
        }));
        let project = StubProjectTool::with_counter(
            "test.project",
            Standing::Gone,
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        );
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            project,
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        // No `project` instance: `config` is standalone.
        let config_value = repo_value("org/the-config");
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", config_value.clone()),
                ("project", repo_value("org/pre-existing")),
            ]),
            outputs_of(&[("config", config_value.clone())]),
        );
        let secret_run = riding_instance(
            "secret",
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
            inputs_of(&[
                ("secret", repo_value("org/the-secret")),
                ("config", config_value.clone()),
            ]),
            Outputs::new(),
        );
        let run = facts_multi(vec![config_run, secret_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        let config_ref = InstanceRef {
            node: node("config"),
            instance: None,
        };
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: InstanceRef {
                        node: node("secret"),
                        instance: None
                    },
                    action: TeardownAction::RidesWith {
                        container: config_ref.clone()
                    },
                },
                TeardownStep {
                    instance: config_ref,
                    action: TeardownAction::Delete { uncertain: false },
                },
            ]
        );
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Acceptance 5: the same run with the project kept lists the config
    /// and the secret `Kept` (their container is kept), and observes
    /// nothing at all -- not even the project itself, whose own policy
    /// already decided it without observation.
    #[test]
    fn riders_of_a_kept_project_are_kept_and_nothing_is_observed() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let project =
            StubProjectTool::with_counter("test.project", Standing::Gone, Arc::clone(&calls));
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(Standing::Gone, Arc::clone(&config_calls));
        let secret = StubSecretTool::new();
        // The project is kept by the document; `config` has no
        // `teardown:` of its own either (with the project kept, a
        // document declaring `teardown: delete` on `config` would hit the
        // *other* rule decision 3a states, "its own `Delete` step" --
        // pinned separately, below).
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Retain,
            None,
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let config_value = repo_value("org/the-config");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", config_value.clone()),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", config_value.clone())]),
        );
        let secret_run = riding_instance(
            "secret",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("secret", repo_value("org/the-secret")),
                ("config", config_value.clone()),
            ]),
            outputs_of(&[("secret", repo_value("org/the-secret"))]),
        );
        let run = facts_multi(vec![project_run, config_run, secret_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: InstanceRef {
                        node: node("secret"),
                        instance: None
                    },
                    action: TeardownAction::Kept { why: KEPT_RIDER },
                },
                TeardownStep {
                    instance: InstanceRef {
                        node: node("config"),
                        instance: None
                    },
                    // Its own default policy, not a rider reason: `config`
                    // has no `teardown:` of its own either, so it is kept
                    // on its own terms, whatever its container is.
                    action: TeardownAction::Kept { why: KEPT },
                },
                TeardownStep {
                    instance: InstanceRef {
                        node: node("project"),
                        instance: None
                    },
                    action: TeardownAction::Kept { why: KEPT },
                },
            ]
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Decision 3a's other rider rule: a `Delete`-capable candidate under
    /// `teardown: delete` whose matched container is itself kept is its
    /// own standalone `Delete` step, never `Kept` and never refused --
    /// distinct from [`riders_of_a_kept_project_are_kept_and_nothing_is_observed`],
    /// whose `config` has no policy of its own at all.
    #[test]
    fn a_delete_capable_rider_whose_container_is_kept_is_its_own_delete_step() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let project =
            StubProjectTool::with_counter("test.project", Standing::Gone, Arc::clone(&calls));
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(
            Standing::AsCreated { losing: vec![] },
            Arc::clone(&config_calls),
        );
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Retain,
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let run = facts_multi(vec![project_run, config_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: InstanceRef {
                        node: node("config"),
                        instance: None
                    },
                    action: TeardownAction::Delete { uncertain: false },
                },
                TeardownStep {
                    instance: InstanceRef {
                        node: node("project"),
                        instance: None
                    },
                    action: TeardownAction::Kept { why: KEPT },
                },
            ]
        );
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // The kept project is never observed either.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Milestone 3m's 2026-10-12 X1 addendum: "a `Within` may name its
    /// own tool" (`StubSelfRidingTool`, below). Two instances that
    /// structurally match *each other* must never both resolve
    /// `GoesAway` from nothing but that mutual match: the earlier one (by
    /// `facts.instances` order) is force-decided standalone, seeding the
    /// later one's own ride, and `standing` is called exactly once, never
    /// twice.
    struct StubSelfRidingTool {
        spec: ToolSpec,
        inverse: RidingInverse,
    }

    impl StubSelfRidingTool {
        fn with_counter(calls: Arc<std::sync::atomic::AtomicUsize>) -> Arc<Self> {
            let mut inputs = IndexMap::new();
            inputs.insert(
                port("id"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            inputs.insert(
                port("parent"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(port("id"), ty("GitHubRepo"));
            Arc::new(Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.self_riding").unwrap(),
                    description: "stub self-riding tool (milestone 3m X1 addendum)".to_string(),
                    inputs,
                    outputs,
                    key: vec![port("id")],
                    class: Class::Reversible,
                    pure: false,
                },
                inverse: RidingInverse {
                    standing: Standing::AsCreated { losing: vec![] },
                    calls,
                },
            })
        }
    }

    impl Tool for StubSelfRidingTool {
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
                needs: &["id"],
                within: &[Within {
                    tool: "test.self_riding",
                    ports: &[("parent", Side::Input, "id")],
                }],
                losses: &[],
            }
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            Some(&self.inverse)
        }
    }

    #[test]
    fn a_within_naming_its_own_tool_never_produces_a_mutual_ride() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool = StubSelfRidingTool::with_counter(Arc::clone(&calls));
        let node_a = Node::new(tool.spec.name.clone())
            .port(port("id"), Binding::Literal("example-org/a".to_string()))
            .port(
                port("parent"),
                Binding::Literal("example-org/b".to_string()),
            )
            .teardown(TeardownPolicy::Delete { accept: vec![] });
        let node_b = Node::new(tool.spec.name.clone())
            .port(port("id"), Binding::Literal("example-org/b".to_string()))
            .port(
                port("parent"),
                Binding::Literal("example-org/a".to_string()),
            )
            .teardown(TeardownPolicy::Delete { accept: vec![] });
        let workflow = Workflow::new(workflow_name("mutual-ride"))
            .node(node("a"), node_a)
            .node(node("b"), node_b);
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(tool as Arc<dyn Tool>).unwrap();
        let checked = check(&workflow, &catalog)
            .unwrap_or_else(|errors| unreachable!("must check cleanly: {errors:?}"));
        let resolved = empty_resolved(&checked);

        // `a`'s own `id` is "a", `parent` "b"; `b`'s own `id` is "b",
        // `parent` "a" -- each structurally matches the other.
        let a_run = riding_instance(
            "a",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("id", repo_value("example-org/a")),
                ("parent", repo_value("example-org/b")),
            ]),
            outputs_of(&[("id", repo_value("example-org/a"))]),
        );
        let b_run = riding_instance(
            "b",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("id", repo_value("example-org/b")),
                ("parent", repo_value("example-org/a")),
            ]),
            outputs_of(&[("id", repo_value("example-org/b"))]),
        );
        let run = facts_multi(vec![a_run, b_run], RUN_FINISHED);

        let teardown = plan_teardown(&run, &checked, &resolved).unwrap();
        // Exactly one of the two is a genuine `Delete` (the earlier in
        // `facts.instances` order, forced standalone to break the cycle);
        // the other rides with it. Never both `Delete`, never both
        // `RidesWith` (that would mean neither was ever grounded).
        let a_ref = InstanceRef {
            node: node("a"),
            instance: None,
        };
        let b_ref = InstanceRef {
            node: node("b"),
            instance: None,
        };
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: b_ref,
                    action: TeardownAction::RidesWith {
                        container: a_ref.clone()
                    },
                },
                TeardownStep {
                    instance: a_ref,
                    action: TeardownAction::Delete { uncertain: false },
                },
            ]
        );
        // Never a mutual ride: exactly one `standing` call total, not two.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    // -----------------------------------------------------------------
    // Milestone 3m, task R6, acceptance 7, 8 (core half), 21 (core half):
    // `apply_teardown`.
    // -----------------------------------------------------------------

    /// A stub [`Inverse`] whose `standing()` and `delete()` observations
    /// are independently settable *after* construction (milestone 3m
    /// task R6): neither [`StubInverse`] (`testing.rs`, fixed for its
    /// whole life) nor [`RidingInverse`]/[`ToleranceInverse`] (above) can
    /// express "what `standing` reports when asked to plan or re-plan"
    /// and "what `delete` itself re-observes, later, when it actually
    /// runs" disagreeing -- exactly acceptance 21's "no builds at the
    /// re-plan, one build when `delete` observes", and acceptance 7's "a
    /// standing that changes between approval and apply". `delete`
    /// honours the documented [`Inverse::delete`] contract for real,
    /// unlike every fixed stub above: [`Standing::Gone`] is
    /// `already_gone: true`; [`Standing::AsCreated`] with every reported
    /// loss in `created.accept` deletes; anything else is
    /// [`crate::tool::ToolErrorKind::Conflict`].
    struct ObservableInverse {
        plan: std::sync::Mutex<Standing>,
        delete: std::sync::Mutex<Standing>,
        standing_calls: Arc<std::sync::atomic::AtomicUsize>,
        delete_calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ObservableInverse {
        fn new(standing: Standing) -> Arc<Self> {
            Arc::new(Self {
                plan: std::sync::Mutex::new(standing.clone()),
                delete: std::sync::Mutex::new(standing),
                standing_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                delete_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            })
        }

        /// What `standing()` reports from now on -- the planning side
        /// (both the original approval and `apply_teardown`'s own
        /// re-plan read this).
        fn set_plan_standing(&self, standing: Standing) {
            *self.plan.lock().unwrap() = standing;
        }

        /// What `delete()` itself re-observes from now on -- modelling a
        /// resource that changes strictly between the last `standing()`
        /// call and `delete`'s own, later, re-observation of the very
        /// same resource.
        fn set_delete_standing(&self, standing: Standing) {
            *self.delete.lock().unwrap() = standing;
        }

        fn delete_calls(&self) -> usize {
            self.delete_calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Inverse for ObservableInverse {
        fn standing(&self, _created: &Created<'_>) -> Result<Standing, crate::tool::ToolError> {
            self.standing_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.plan.lock().unwrap().clone())
        }

        fn delete(
            &self,
            created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, crate::tool::ToolError> {
            self.delete_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Read, then drop the guard immediately (a plain `{ }` block,
            // not the match scrutinee itself): an arm below re-locks the
            // same `Mutex` on a successful delete, and holding this guard
            // into the match would deadlock it.
            let current = { self.delete.lock().unwrap().clone() };
            match current {
                Standing::Gone => Ok(Deleted { already_gone: true }),
                Standing::AsCreated { losing } => {
                    if losing
                        .iter()
                        .all(|loss| created.accept.iter().any(|a| a.as_str() == *loss))
                    {
                        // Milestone 3m task R6: a real inverse's own
                        // `standing` would report `Gone` after this on
                        // its own; this stub must be told, so it tells
                        // itself -- behaving like an actually-deleted
                        // resource for whichever test plans again.
                        *self.plan.lock().unwrap() = Standing::Gone;
                        *self.delete.lock().unwrap() = Standing::Gone;
                        Ok(Deleted {
                            already_gone: false,
                        })
                    } else {
                        Err(crate::tool::ToolError {
                            kind: crate::tool::ToolErrorKind::Conflict,
                            message: "a declared loss this node's policy has not accepted"
                                .to_string(),
                        })
                    }
                }
                Standing::NotOurs { .. } | Standing::Drifted { .. } => {
                    Err(crate::tool::ToolError {
                        kind: crate::tool::ToolErrorKind::Conflict,
                        message: "no longer AsCreated".to_string(),
                    })
                }
            }
        }
    }

    /// A stub, top-level `Delete` tool declaring the loss `"builds"`,
    /// holding an [`ObservableInverse`] (milestone 3m task R6):
    /// [`StubDeletableTool`] (`testing.rs`) always pairs with its own
    /// fixed `StubInverse`, so this is the one combination R6's own tests
    /// need -- local to this crate's own tests, like every other riding
    /// fixture above (R5b's own addendum).
    struct StubLossyTool {
        spec: ToolSpec,
        inverse: Arc<ObservableInverse>,
    }

    impl StubLossyTool {
        fn new(inverse: Arc<ObservableInverse>) -> Arc<Self> {
            let mut inputs = IndexMap::new();
            inputs.insert(
                port("repo"),
                PortSpec {
                    ty: PortType::Exact(ty("GitHubRepo")),
                    required: true,
                    derived_only: false,
                },
            );
            let mut outputs = IndexMap::new();
            outputs.insert(port("repo"), ty("GitHubRepo"));
            Arc::new(Self {
                spec: ToolSpec {
                    name: ToolName::parse("test.lossy").unwrap(),
                    description: "stub tool declaring a loss, with an independently \
                                   observable standing/delete (milestone 3m task R6)"
                        .to_string(),
                    inputs,
                    outputs,
                    key: vec![port("repo")],
                    class: Class::Reversible,
                    pure: false,
                },
                inverse,
            })
        }
    }

    impl Tool for StubLossyTool {
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
                needs: &["repo"],
                within: &[],
                losses: &["builds"],
            }
        }

        fn inverse(&self) -> Option<&dyn Inverse> {
            Some(self.inverse.as_ref())
        }
    }

    /// A one-node `test.lossy` workflow, `repo` (literal), the given
    /// policy -- the `apply_teardown` counterpart of [`checked_with`]
    /// above.
    fn checked_lossy(tool: Arc<StubLossyTool>, policy: TeardownPolicy) -> Checked {
        let repo_node = Node::new(ToolName::parse("test.lossy").unwrap())
            .port(
                port("repo"),
                Binding::Literal("example-org/example-repo".to_string()),
            )
            .teardown(policy);
        let workflow =
            Workflow::new(workflow_name("apply-teardown-test")).node(node("repo"), repo_node);
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(tool as Arc<dyn Tool>).unwrap();
        check(&workflow, &catalog).unwrap_or_else(|errors| {
            unreachable!("a one-node lossy workflow must check cleanly: {errors:?}")
        })
    }

    /// A two-node `test.lossy` workflow sharing one tool/inverse, `repo_a`
    /// and `repo_b`, both under the given policy -- for the "stop at the
    /// first failure" test, where teardown order (reverse of
    /// `facts.instances`) puts `repo_b` first.
    /// A three-node `test.lossy` workflow sharing one tool/inverse,
    /// `repo_a` and `repo_b` under the given policy (for the "stop at
    /// the first failure" test, where teardown order -- reverse of
    /// `facts.instances` -- puts `repo_b` first), plus a third node,
    /// `repo_c`, declaring no `teardown:` at all (the conservative
    /// default) -- for pinning that a non-`Delete` step stays `Listed`
    /// even *after* a `Delete` failure, not only before one. `repo_c` is
    /// declared first, so it is the *last* teardown step, after both of
    /// `repo_a`/`repo_b`'s.
    fn checked_lossy_trio(tool: Arc<StubLossyTool>, policy: TeardownPolicy) -> Checked {
        let node_c = Node::new(ToolName::parse("test.lossy").unwrap()).port(
            port("repo"),
            Binding::Literal("example-org/repo-c".to_string()),
        );
        let node_a = Node::new(ToolName::parse("test.lossy").unwrap())
            .port(
                port("repo"),
                Binding::Literal("example-org/repo-a".to_string()),
            )
            .teardown(policy.clone());
        let node_b = Node::new(ToolName::parse("test.lossy").unwrap())
            .port(
                port("repo"),
                Binding::Literal("example-org/repo-b".to_string()),
            )
            .teardown(policy);
        let workflow = Workflow::new(workflow_name("apply-teardown-trio-test"))
            .node(node("repo_c"), node_c)
            .node(node("repo_a"), node_a)
            .node(node("repo_b"), node_b);
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(tool as Arc<dyn Tool>).unwrap();
        check(&workflow, &catalog).unwrap_or_else(|errors| {
            unreachable!("a three-node lossy workflow must check cleanly: {errors:?}")
        })
    }

    /// Base case: `apply_teardown` deletes a plain `Delete` step and
    /// reports it `Deleted`.
    #[test]
    fn apply_teardown_deletes_in_order_and_reports_each_outcome() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (_, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        let outcomes = apply_teardown(&approved, &run, &checked, &resolved).unwrap();
        assert_eq!(outcomes, vec![TeardownStepOutcome::Deleted]);
        assert_eq!(inverse.delete_calls(), 1);
    }

    /// Acceptance 7: a standing that changes between approval and apply
    /// (here, to outright drift) refuses with no delete -- the re-plan
    /// itself refuses, before `apply_teardown` even reaches the delete
    /// loop.
    #[test]
    fn a_standing_that_drifts_before_the_replan_refuses_with_no_deletes() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (_, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();

        inverse.set_plan_standing(Standing::Drifted {
            what: "a push after this run",
        });

        let err = apply_teardown(&approved, &run, &checked, &resolved).unwrap_err();
        assert!(matches!(err, TeardownApplyError::Replan { .. }), "{err:?}");
        assert_eq!(inverse.delete_calls(), 0);
    }

    /// Acceptance 21's second half: a build appearing between approval
    /// and the apply-time re-plan, under `accept: [builds]`, refuses as
    /// drift -- the re-plan itself still succeeds (same `Delete { uncertain:
    /// false }` action both times), but the fingerprint's own `standing`
    /// now carries the loss, so the comparison still catches it.
    #[test]
    fn a_loss_appearing_before_the_replan_refuses_as_drift_though_the_action_is_unchanged() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy(
            tool,
            TeardownPolicy::Delete {
                accept: vec![Loss::parse("builds").unwrap()],
            },
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (teardown, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None,
                },
                action: TeardownAction::Delete { uncertain: false },
            }]
        );

        inverse.set_plan_standing(Standing::AsCreated {
            losing: vec!["builds"],
        });

        let err = apply_teardown(&approved, &run, &checked, &resolved).unwrap_err();
        assert!(matches!(err, TeardownApplyError::Drift), "{err:?}");
        assert_eq!(inverse.delete_calls(), 0);
    }

    /// Acceptance 21's core half: a build appearing only when `delete`
    /// itself observes -- never at the re-plan -- is not deleted:
    /// `delete` refuses with `Conflict` on its own.
    #[test]
    fn delete_itself_refuses_an_unaccepted_loss_appearing_only_at_delete_time() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        // A plain `teardown: delete`: no loss is ever accepted.
        let checked = checked_lossy(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (_, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();

        // The re-plan still sees no builds (`plan` standing is untouched);
        // only `delete`'s own observation changes.
        inverse.set_delete_standing(Standing::AsCreated {
            losing: vec!["builds"],
        });

        let outcomes = apply_teardown(&approved, &run, &checked, &resolved).unwrap();
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            TeardownStepOutcome::Failed { error } => {
                assert_eq!(error.kind, crate::tool::ToolErrorKind::Conflict);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(inverse.delete_calls(), 1);
    }

    /// Acceptance 8's core half: once `apply_teardown` has actually
    /// deleted a resource (its own `delete` call flips the stub's
    /// standing to `Gone`), planning again reads that step `Gone`, not
    /// `Delete`.
    #[test]
    fn planning_again_after_a_delete_reads_the_step_gone() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (_, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        let outcomes = apply_teardown(&approved, &run, &checked, &resolved).unwrap();
        assert_eq!(outcomes, vec![TeardownStepOutcome::Deleted]);

        // Planning again (acceptance 8's own "resumes by planning
        // again"): the deleted step now reads `Gone` -- `ObservableInverse`
        // tells itself this on a successful delete, exactly as a real
        // resource's own `standing` would, with nothing done by hand here.
        let (teardown_again, approved_again) =
            plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown_again.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None,
                },
                action: TeardownAction::Gone,
            }]
        );

        // And the second teardown succeeds: a `Gone` step is applied as
        // `Listed`, never its own `delete` call.
        let outcomes_again = apply_teardown(&approved_again, &run, &checked, &resolved).unwrap();
        assert_eq!(outcomes_again, vec![TeardownStepOutcome::Listed]);
        assert_eq!(inverse.delete_calls(), 1);
    }

    /// A `Delete` step's own failure stops the run: a later `Delete` step
    /// in `approved`'s own order is `NotRun`, never attempted.
    #[test]
    fn a_delete_failure_stops_the_run_and_later_delete_steps_are_not_run() {
        let inverse = ObservableInverse::new(Standing::AsCreated { losing: vec![] });
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy_trio(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts_multi(
            vec![
                // Declared (and placed) first, so it is the *last*
                // teardown step: a non-`Delete` step after a `Delete`
                // failure, not only before one.
                riding_instance(
                    "repo_c",
                    Action::Update,
                    finished(START, END, NodeStatus::Created),
                    inputs_of(&[]),
                    outputs_of(&[]),
                ),
                riding_instance(
                    "repo_a",
                    Action::Create,
                    finished(START, END, NodeStatus::Created),
                    inputs_of(&[]),
                    outputs_of(&[]),
                ),
                riding_instance(
                    "repo_b",
                    Action::Create,
                    finished(START, END, NodeStatus::Created),
                    inputs_of(&[]),
                    outputs_of(&[]),
                ),
            ],
            RUN_FINISHED,
        );
        let (_, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        assert_eq!(approved.len(), 3);

        // `repo_a`/`repo_b` are approved as plain `Delete`s; now make
        // `delete` itself refuse every call (an unaccepted loss on every
        // observation), so the *first* one `apply_teardown` runs fails.
        inverse.set_delete_standing(Standing::AsCreated {
            losing: vec!["builds"],
        });

        let outcomes = apply_teardown(&approved, &run, &checked, &resolved).unwrap();
        assert_eq!(outcomes.len(), 3);
        assert!(matches!(outcomes[0], TeardownStepOutcome::Failed { .. }));
        assert_eq!(outcomes[1], TeardownStepOutcome::NotRun);
        // `repo_c`'s own `Retained` step, after the failure: `Listed`,
        // never `NotRun` -- decision 8's "`Listed` for every non-delete
        // step" is unconditional, whatever its own position relative to
        // a failure.
        assert_eq!(outcomes[2], TeardownStepOutcome::Listed);
        assert_eq!(inverse.delete_calls(), 1);
    }

    /// An inverse combining [`ToleranceInverse`]'s own riders check with
    /// recording what `delete` itself was handed (milestone 3m task R6):
    /// neither existing riding fixture lets a test see `Created.riders`
    /// at *delete* time, only at `standing` time, and `delete_one`'s own
    /// rebuild of them (a second, independent computation from the one
    /// `decide_standalone` used at plan time) deserves its own pin, not
    /// just a passing assertion that *something* got deleted.
    struct RidingDeleteInverse {
        expected_riders: usize,
        standing_calls: Arc<std::sync::atomic::AtomicUsize>,
        delete_calls: Arc<std::sync::atomic::AtomicUsize>,
        delete_riders_seen: Arc<std::sync::Mutex<Option<usize>>>,
    }

    impl Inverse for RidingDeleteInverse {
        fn standing(&self, created: &Created<'_>) -> Result<Standing, crate::tool::ToolError> {
            self.standing_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if created.riders.len() == self.expected_riders {
                Ok(Standing::AsCreated { losing: vec![] })
            } else {
                Ok(Standing::Drifted {
                    what: "an untolerated secret name",
                })
            }
        }

        fn delete(
            &self,
            created: &Created<'_>,
            _token: &TeardownToken,
        ) -> Result<Deleted, crate::tool::ToolError> {
            self.delete_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.delete_riders_seen.lock().unwrap() = Some(created.riders.len());
            Ok(Deleted {
                already_gone: false,
            })
        }
    }

    /// Acceptance 5, 7 together, through `apply_teardown`: a config rides
    /// with the project it was created alongside. At apply time, the
    /// `RidesWith` step is `Listed` (never its own `delete` call, never
    /// even its own `standing`), and the project's own `delete` is handed
    /// exactly the rider `standing` already tolerated -- `delete_one`'s
    /// own rebuild of `Created.riders` agrees with `decide_standalone`'s.
    #[test]
    fn apply_teardown_lists_a_rider_and_hands_its_container_the_same_riders_at_delete() {
        let standing_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let delete_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let delete_riders_seen = Arc::new(std::sync::Mutex::new(None));
        let project = StubProjectTool::with_inverse(
            "test.project",
            Box::new(RidingDeleteInverse {
                expected_riders: 1,
                standing_calls: Arc::clone(&standing_calls),
                delete_calls: Arc::clone(&delete_calls),
                delete_riders_seen: Arc::clone(&delete_riders_seen),
            }),
        );
        let config_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = StubConfigTool::with_standing(Standing::Gone, Arc::clone(&config_calls));
        let secret = StubSecretTool::new();
        let checked = checked_riding(
            Arc::clone(&project),
            Arc::clone(&config),
            secret,
            TeardownPolicy::Delete { accept: vec![] },
            Some(TeardownPolicy::Delete { accept: vec![] }),
        );
        let resolved = empty_resolved(&checked);

        let project_value = repo_value("org/the-project");
        let project_run = riding_instance(
            "project",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[("project", project_value.clone())]),
            outputs_of(&[("project", project_value.clone())]),
        );
        let config_run = riding_instance(
            "config",
            Action::Create,
            finished(START, END, NodeStatus::Created),
            inputs_of(&[
                ("config", repo_value("org/the-config")),
                ("project", project_value.clone()),
            ]),
            outputs_of(&[("config", repo_value("org/the-config"))]),
        );
        let run = facts_multi(vec![project_run, config_run], RUN_FINISHED);

        let (teardown, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        let project_ref = InstanceRef {
            node: node("project"),
            instance: None,
        };
        let config_ref = InstanceRef {
            node: node("config"),
            instance: None,
        };
        assert_eq!(
            teardown.steps,
            vec![
                TeardownStep {
                    instance: config_ref,
                    action: TeardownAction::RidesWith {
                        container: project_ref.clone(),
                    },
                },
                TeardownStep {
                    instance: project_ref,
                    action: TeardownAction::Delete { uncertain: false },
                },
            ]
        );
        assert_eq!(standing_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let outcomes = apply_teardown(&approved, &run, &checked, &resolved).unwrap();
        assert_eq!(
            outcomes,
            vec![TeardownStepOutcome::Listed, TeardownStepOutcome::Deleted]
        );
        // The `RidesWith` step's own container (`test.config`'s inverse)
        // never saw a `delete` call -- nor even a second `standing` one
        // (trust boundary 6, as R5b's addendum reworded it: never a
        // `RidesWith` step on its own).
        assert_eq!(config_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        // The project's own `delete` ran exactly once, handed exactly the
        // one rider its own `standing` already tolerated.
        assert_eq!(delete_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(*delete_riders_seen.lock().unwrap(), Some(1));
    }

    /// `apply_teardown`'s own re-plan re-observes a `Gone` step too, not
    /// only `Delete` ones: a resource someone recreated after approval
    /// (`Gone` at approval, `AsCreated` now) must still refuse, which a
    /// loop over `approved`'s own `Delete` steps alone would miss.
    #[test]
    fn a_resource_recreated_after_a_gone_approval_refuses_as_drift() {
        let inverse = ObservableInverse::new(Standing::Gone);
        let tool = StubLossyTool::new(Arc::clone(&inverse));
        let checked = checked_lossy(tool, TeardownPolicy::Delete { accept: vec![] });
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (teardown, approved) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        assert_eq!(
            teardown.steps,
            vec![TeardownStep {
                instance: InstanceRef {
                    node: node("repo"),
                    instance: None,
                },
                action: TeardownAction::Gone,
            }]
        );

        inverse.set_plan_standing(Standing::AsCreated { losing: vec![] });

        let err = apply_teardown(&approved, &run, &checked, &resolved).unwrap_err();
        assert!(matches!(err, TeardownApplyError::Drift), "{err:?}");
        assert_eq!(inverse.delete_calls(), 0);
    }

    // -------------------------------------------------------------
    // Serde round trips (milestone 3m task R7)
    // -------------------------------------------------------------

    /// A real [`StepFingerprint`] (built the same way
    /// `plan_teardown_fingerprint` builds one for a caller) round-trips
    /// through JSON unchanged -- the property `willikins-journal`'s own
    /// `TeardownPlanned.fingerprint: Vec<StepFingerprint>` relies on to
    /// read a typed, comparable fingerprint back out of a replayed
    /// journal at apply time, the same way it already does for
    /// `PlanRecorded.fingerprint: Vec<InstanceFingerprint>`.
    #[test]
    fn step_fingerprint_round_trips_through_json() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::AsCreated {
            losing: vec!["builds"],
        }));
        let checked = checked_with(
            Arc::clone(&tool),
            Some(TeardownPolicy::Delete {
                accept: vec![crate::workflow::Loss::parse("builds").unwrap()],
            }),
        );
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (_, fingerprint) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        assert_eq!(fingerprint.len(), 1);

        let json = serde_json::to_string(&fingerprint).expect("StepFingerprint serializes");
        let back: Vec<StepFingerprint> =
            serde_json::from_str(&json).expect("StepFingerprint deserializes");
        assert_eq!(back, fingerprint);
        assert_eq!(
            back[0].standing,
            Some(StandingKind::AsCreated {
                losing: vec!["builds".to_string()],
            })
        );
    }

    /// Every [`TeardownStepOutcome`] variant round-trips through JSON --
    /// the property `willikins-journal`'s own
    /// `TeardownStepFinished.status` relies on.
    #[test]
    fn every_teardown_step_outcome_round_trips_through_json() {
        for outcome in [
            TeardownStepOutcome::Deleted,
            TeardownStepOutcome::AlreadyGone,
            TeardownStepOutcome::Failed {
                error: crate::tool::ToolError {
                    kind: crate::tool::ToolErrorKind::Provider,
                    message: "boom".to_string(),
                },
            },
            TeardownStepOutcome::NotRun,
            TeardownStepOutcome::Listed,
        ] {
            let json = serde_json::to_string(&outcome).expect("TeardownStepOutcome serializes");
            let back: TeardownStepOutcome =
                serde_json::from_str(&json).expect("TeardownStepOutcome deserializes");
            assert_eq!(back, outcome, "{json}");
        }
    }

    /// A real [`Teardown`] serializes (one-way: no `Deserialize`, see that
    /// type's own doc) without panicking, the shape
    /// `willikins_journal::Redacted<Teardown>` relies on for
    /// `TeardownPlanned.teardown`.
    #[test]
    fn teardown_serializes_one_way() {
        let tool = Arc::new(StubDeletableTool::with_standing(Standing::Gone));
        let checked = checked_with(tool, Some(TeardownPolicy::Delete { accept: vec![] }));
        let resolved = empty_resolved(&checked);
        let run = facts(
            instance(Action::Create, finished(START, END, NodeStatus::Created)),
            RUN_FINISHED,
        );
        let (teardown, _) = plan_teardown_fingerprint(&run, &checked, &resolved).unwrap();
        let json = serde_json::to_value(&teardown).expect("Teardown serializes");
        assert_eq!(json["steps"][0]["action"]["kind"], "gone");
    }

    /// A [`TeardownApplyError::Drift`] serializes (one-way) with a
    /// `"kind"` tag -- the shape
    /// `willikins_journal::Redacted<TeardownApplyError>` relies on for
    /// `TeardownFinished`'s own `Failed` outcome. `Replan`'s own nested,
    /// also-tagged `TeardownError` is a named field, not a tuple, so the
    /// two tags never collide in one JSON object (this task's own plan
    /// addendum).
    #[test]
    fn teardown_apply_error_serializes_with_no_tag_collision() {
        let drift = TeardownApplyError::Drift;
        let json = serde_json::to_value(&drift).expect("TeardownApplyError serializes");
        assert_eq!(json["kind"], "Drift");

        let replan = TeardownApplyError::from(TeardownError::UnresolvableCredential {
            node: node("repo"),
            port: crate::tool::PortName::parse("token").unwrap(),
        });
        let json = serde_json::to_value(&replan).expect("TeardownApplyError::Replan serializes");
        assert_eq!(json["kind"], "Replan");
        assert_eq!(json["error"]["kind"], "UnresolvableCredential");
    }
}
