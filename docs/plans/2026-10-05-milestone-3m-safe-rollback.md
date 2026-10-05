# Milestone 3m: deterministic, safe rollback of what one run created

**Created:** 2026-10-05 (from `todos/2026-09-29-deterministic-safe-rollback.md`)

## Goal

Given one recorded run, willikins computes a **teardown plan** that deletes exactly what that run created and nothing
else, shows it the way it shows any plan, waits for a human to approve it, and runs it only if a fresh observation
still agrees with what was approved. Concretely:

1. **Each tool declares its reversal** (decision 3): a typed inverse that deletes one resource, "rides out with its
   container", "left in place, because …", or nothing at all. The declaration is static, like a tool's class. A tool
   that declares nothing is never undone.
2. **A teardown plan is computed from the journal** (decisions 1, 2, 5, 6): the run's planned actions and node
   outcomes say what it created, the journal's own timestamps bracket when, and the document at its recorded hash
   re-resolves the credentials the deletes need. Steps run in reverse of the applied plan's order.
3. **Drift refuses the whole teardown** (decision 7): before anything is deleted, every candidate is observed. A
   resource that is gone is skipped; one that is not willikins', that was created outside this run's window, or that
   has changed since (a push, a secret nobody in this run wrote, a config someone else inherits, a build) refuses the
   teardown before any delete.
4. **Approved like any plan** (decision 8): a teardown is always `Destructive`, always needs a human, is journaled,
   and is re-observed and compared step by step before its first delete.
5. **The forward plan says what a rollback could not undo** (decision 10), before anything runs.
6. **The CLI drives it** (decision 9): `willikins rollback`. MCP is out of scope (below).

This milestone covers the todo's three parts: undoing pure creates (Delete), per-tool inverses or their declared
absence (Contained, Retained), and the reverse of a gate (Made by hand).

## The operator's words (2026-09-29)

"If all this works, all we need is deterministic safe rollback, and we'll have basically reinvented CloudFormation."

The todo's own framing stands: willikins keeps no stack state and spans several vendors, so rollback cannot mean
"restore a snapshot". It means converging back, from the journal and from reads of reality, with the plan showing
exactly what it will delete and what it cannot undo.

## Out of scope

- **Undoing an attribute change or an overwrite.** Rollback deletes things this run created. It never restores a
  prior value: a rotated token, an overwritten secret, a raised member role, a replaced profile, a rewritten pipeline
  configuration, or an inheritance set extended in place. Those are `Retained` with a reason (the per-tool table).
- **The MCP surface.** An agent-callable `rollback_plan` is safe (the plan is always approval-gated), but the
  approvals page and `run_status` need a teardown-shaped rendering first. A follow-up todo, written by task D1.
- **Rolling back across journals.** The CLI and a server keep separate journals. A teardown reads one.
- **SigNoz.** Its sandbox key expired on 2026-09-23, so no delete can be proven live. `signoz.ingestion_key.ensure`
  declares `Retained` until a live cycle can run (decision 4).
- **Any rollback of the operator's real runs.** That is the operator's act, from their own journal, after reading the
  teardown plan. No agent plans or applies one.
- **Restoring something the run deleted.** `appstore.profile.ensure`'s Replace deletes an INVALID profile first; it
  cannot come back.

## Trust boundaries (normative)

1. **No inverse is a catalog tool.** Nothing named `*.delete` is registered, documented in the schema, or nameable in a
   workflow document. An inverse is reachable only from the teardown executor, with facts read from the journal. A
   delete node a document could name is the open step the design forbids: an agent could delete any repository its
   credential can reach.
2. **A delete takes a `TeardownToken`.** `TeardownToken::new` exists only behind `willikins-types`' `executor` feature,
   is disallowed by `clippy.toml` everywhere except `willikins-core/src/rollback.rs`'s executor, and is never passed to
   `Tool::read`, `Tool::ensure` or `Inverse::standing`. It is the `SinkToken` mechanism, unchanged.
3. **Typed deletes, never paths.** Each new client method takes domain types (`&GitHubRepo`, `&DopplerProject`,
   `&DopplerProject` + `&EnvironmentSlug`, `&DopplerConfig`, `&BuildkiteOrg` + `&BuildkitePipelineSlug`) and builds its
   own path. No inverse takes a URL, a path or a `Text`.
4. **Only what this run created** (decision 1). An instance the run found `NoOp`, `Unchanged` or `Converged` is never
   a candidate, nor is anything another run created.
5. **No value in a reason.** Every drift and refusal reason is a `&'static str`, like `Gate::need`. A teardown step
   names its subject only through non-secret `Value`s, which every output surface masks or redacts as usual.
6. **All reads before any delete.** As `deploy/teardown.sh` did: the teardown plan observes every candidate, and the
   executor re-observes all of them, before the first delete.
7. **A secret is never journaled for a teardown.** Credentials are re-resolved from the document's pure resolver
   nodes at teardown time (decision 5). They are never read back from the journal, which holds only redaction markers
   for them.

## SHARED VALUES

| Name | Value | Where |
| --- | --- | --- |
| Core module | `crates/willikins-core/src/rollback.rs` (re-exported as `willikins_core::rollback`) | R1, R3, R5, R6 |
| Declaration method | `fn reversal(&self) -> Reversal` on `Tool`, default `Reversal::Retained { why: UNDECLARED }` | R1 |
| `UNDECLARED` | `"this tool declares no reversal"` | R1 |
| Inverse method | `fn inverse(&self) -> Option<&dyn Inverse>` on `Tool`, default `None` | R3 |
| `Reversal` variants | `Delete { needs, within }`, `Contained { within, otherwise }`, `Retained { why }` | R1 |
| `Within` | `{ tool: &'static str, ports: &'static [(&'static str, Side, &'static str)] }`, `Side` = `Input` \| `Output` | R1 |
| `Standing` variants | `AsCreated`, `Gone`, `NotOurs { why }`, `Drifted { what }` (both `&'static str`) | R3 |
| Step fingerprint | `StepFingerprint { node, instance, action, standing }`, typed, discriminants only, in `TeardownPlanned` | R5, R7 |
| `TeardownAction` variants | `Delete { uncertain: bool }`, `RidesWith { container: InstanceRef }`, `Gone`, `Retained { why }`, `MadeByHand { need }`, `Unfinished` | R5 |
| Token | `willikins_types::TeardownToken` in `crates/willikins-types/src/sink.rs`, `executor` feature | R2 |
| `CREATION_SKEW` | 120 seconds, `pub const` in `rollback.rs` | R5, P1–P4 |
| Teardown class | always `Class::Destructive`; `requires_approval` always `true` | R5, R8 |
| Journal events | `TeardownPlanned`, `TeardownStarted`, `TeardownStepFinished`, `TeardownFinished` | R7 |
| New `ApplyRefusedReason`s | `AlreadyRolledBack { run_id }`, `LaterRunFirst { run_id }`, `NotAForwardRun`, `RunUnfinished` | R7 |
| New `DriftReasonKind` | `Standing` | R7 |
| CLI | `willikins rollback <file> --run <run-id> --journal <path> [--live \| --fake-state F] [--fake-state-out F]`; `willikins rollback --plan-id <id> --journal <path> [...]` | R9 |
| Live probe prefix | `willikins-probe-delete-me-` (`willikins_probe_delete_me_` where `-` is refused) | L1–L3 |
| Forward plan field | `Plan::not_reversible: Vec<NotReversible>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]` | F1 |

## Decisions

### 1. What "this run created" means: two journal facts, joined

`NodeStatus::Created` means only that `ensure` reported `changed: true` (`apply.rs`). An instance planned `Update` or
`Replace` finishes `Created` as well, and `doppler.secret.set` and `github.actions_secret.ensure` report it on every
apply. So `Created` alone does not mean "created".

**An instance is a candidate when its `PlanRecorded` action is `Create`** (from the run's own plan, joined by node and
`for_each` instance) **and its outcome is one of these:**

| Planned | `NodeFinished` | Teardown treats it as |
| --- | --- | --- |
| `Create` | `Created` | created by this run: the inverse decides |
| `Create` | `Failed` | **uncertain**: the create may have landed before the failure (milestone 3c's 201-with-no-data orphan). A `Delete` tool's standing decides: `AsCreated` gives `Delete { uncertain: true }`, shown apart; `Gone` gives `Gone` |
| `Create` | `NodeStarted` with no `NodeFinished` (the process died) | `Unfinished`: no window end exists, so the teardown names it as a manual check and never deletes it |
| `Create` | `Unchanged` | the resource existed by the time `ensure` ran (a new Doppler project's auto-created `dev`/`stg`/`prd` root config, HANDOFF's 2026-09 live record). Not this node's creation, but it **may ride** with a container (decision 3) |
| `Create` | `NotRun`, `Skipped`, `Blocked` | nothing was created |
| `Replace`, `Update` | any | `Retained` ("changed in place, or replaced; the prior state cannot be restored") |
| `NoOp`, `Compute` | any | not this run's doing; never listed, except gates (decision 4) |

**Why not "status `Created` plus a marker":** a marker proves willikins made a resource, not which run made it. A
second document can converge on the same key.

### 2. The creation window: the journal already timestamps it

Every journal entry carries `at`. For a candidate, the window is `[NodeStarted.at − CREATION_SKEW, NodeFinished.at +
CREATION_SKEW]`. A resource whose provider-reported `created_at` falls outside it is `NotOurs` ("created outside this
run's window"): someone deleted and recreated it, or it predates the run. `settled_by` is the run's `RunFinished.at +
CREATION_SKEW`. A container whose change timestamp (GitHub's `pushed_at`) is later than that has changed since the run.

**Why 120 seconds:** provider clocks and this host's clock are independent. Two minutes is wider than any skew a
synced clock shows, and narrower than the gap between two human-paced runs. A provider that reports no `created_at` for
a resource cannot get a `Delete` inverse (decision 4 lists which do).

### 3. How a tool declares its reversal

A default trait method in the manner of `gate()`, `replaces()` and `updates()`: no `Observation` variant (tool.rs
explains the cost) and no change to any existing tool until it opts in.

```rust
pub enum Reversal {
    /// `inverse()` deletes this resource. `needs` names the input ports the delete needs (key ports and credential
    /// ports). `within` names the containers it rides out with when this run created one of them too.
    Delete { needs: &'static [&'static str], within: &'static [Within] },
    /// No delete of its own: it goes when a container this run created goes, and is otherwise left in place for
    /// `otherwise`.
    Contained { within: &'static [Within], otherwise: &'static str },
    /// Never undone by rollback, for `why`.
    Retained { why: &'static str },
}

pub trait Inverse: Send + Sync {
    /// Observe the resource `created` describes. No token: this runs at teardown plan time.
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError>;
    /// Delete it. Observes first, as `ensure` does: anything but `AsCreated` is `Conflict`, and `Gone` is
    /// `Ok(Deleted { already_gone: true })`.
    fn delete(&self, created: &Created<'_>, token: &TeardownToken) -> Result<Deleted, ToolError>;
}

pub struct Created<'a> {
    pub inputs: &'a Inputs,          // exactly `needs`: non-secret from the journal, secret re-resolved (decision 5)
    pub outputs: &'a Outputs,        // the recorded non-secret outputs; secret ones Unknown
    pub window: CreationWindow,      // decision 2
    pub settled_by: Timestamp,       // decision 2
    pub riders: &'a [Rider<'a>],     // the instances riding out with this one: tool name, inputs, outputs
}
```

`Catalog::insert` validates a declaration as it validates a `Gate`: every `needs` and `within` port names one of the
tool's own input ports (or, for `Side::Output`, one of the container tool's outputs), `Delete` holds exactly when
`inverse()` is `Some`, and a pure tool declares nothing. A `Within` names its container by tool name. A test over
`LIVE_TOOL_NAMES` checks that each one names a registered tool and that the ports it maps have the same type.

**Containment is typed equality on recorded values, over every instance of the run.** An instance rides with a
container instance when the container's tool matches the `Within`, the container is itself being deleted or rides
with something being deleted, and every mapped port pair holds equal `Value`s in the journal (the `TypeRef` and the
full recorded value). Riding is computed over **all** of the run's instances, whatever their action or status, so a
non-candidate can be a **transit rider**. A new project's auto-created `prd` config finishes `Unchanged`, yet a
`doppler.secret.set` into it must still ride with the project. Without transit, that secret would be `Retained`, and
the project's own drift rule would then refuse the whole teardown over a name this run wrote. A transit rider is listed
as `RidesWith`, since it does go with its container. The mapping is deterministic and needs no graph. When it misfires
it can only cause an under-deletion: a rider is never deleted on its own, and the container's own drift check
(decision 7) must account for every rider.

**Why `riders` reach the container's inverse:** a container must tolerate exactly what its own run put inside it.
A new Doppler project holds the configs and secret names this run's riders created, plus Doppler's three default
environments, and nothing more.

### 4. The per-tool table

Eighteen non-pure live tools and seven gates. "Within" lists the containers each one rides with.

| Tool | Class | Reversal | Inverse (typed delete) | Within | Why |
| --- | --- | --- | --- | --- | --- |
| `github.repo.ensure` | Reversible | **Delete** | `GitHubClient::delete_repo(&GitHubRepo)` | none | marker topic, `created_at`, no later push |
| `github.actions_secret.ensure` | Reversible | Contained | none | repo (`repo`→`repo`) | always writes, prior value unknowable |
| `github.scaffold.ensure` | Irreversible | Contained | none | repo (`repo`→`repo`) | a signed commit on a branch. A revert is a new forward change the document must make |
| `doppler.project.ensure` | Reversible | **Delete** | `delete_project(&DopplerProject)` | none | marker description, `created_at`, contents (decision 7) |
| `doppler.config.ensure` | Reversible | **Delete** | `delete_environment(&DopplerProject, &EnvironmentSlug)` | project (`project`→`project`) | a root config goes only with its environment |
| `doppler.branch_config.ensure` | Reversible | **Delete** | `delete_config(&DopplerConfig)` | project (`project`→`project`); `doppler.config.ensure` (`project`, `environment`) | |
| `doppler.config.inheritable.ensure` | Reversible | Contained | none | `doppler.config.ensure`/`branch_config.ensure` (`config`→output `config`) | a flag on an existing config. Whether it was already set is not recorded |
| `doppler.config.inherits.ensure` | Reversible | Contained | none | same | add-only, and which bases were already there is not recorded |
| `doppler.project_member.ensure` | Reversible | Contained | none | project | add or raise. The prior role is not recorded |
| `doppler.service_token.ensure` | Reversible | **Delete** | existing `delete_service_token`, by the one slug created in the window | config (`config`→output `config`) | |
| `doppler.service_token.rotate` | Destructive | Retained | none | | the revoked token cannot be restored |
| `doppler.secret.set` | Reversible | Contained | none | config (`config`→output `config`) | always writes. The prior value is never read, by design |
| `signoz.ingestion_key.ensure` | Reversible | Retained | none (a delete exists, unproven) | | no live sandbox (out of scope) |
| `buildkite.pipeline.ensure` | Reversible | **Delete** | existing `delete_pipeline(&BuildkiteOrg, &BuildkitePipelineSlug)` | none | marker description, `created_at`, no builds |
| `buildkite.pipeline.bootstrap.ensure` | Destructive | Contained | none | pipeline (`org`, `slug`) | the prior configuration is not recorded |
| `appstore.bundle_id.ensure` | Reversible | Retained | none | | App Store identity the operator keeps: no ownership marker exists, and deleting an identifier takes its capabilities and profiles with it |
| `appstore.bundle_id_capability.ensure` | Reversible | Retained | none | | disabling a capability invalidates every profile on the identifier |
| `appstore.profile.ensure` | Destructive | Retained | none | | CI fetches profiles by name, and a re-run of the document recreates one on demand (Needs the operator, 1) |

**Gates** (`appstore.app.get`, `appstore.app_group.gate`, `appstore.bundle_id_capability.gate`,
`buildkite.pipeline.bootstrap.gate`, `doppler.config.inheritable.gate`, `doppler.secret_name.gate`,
`operator.acknowledge`) declare nothing. The engine reports each gate instance the run found `Present` as `MadeByHand
{ need }`, using the gate's own static `need`: this is the todo's "gate in reverse". A Blocked gate was never satisfied
and is not listed.

**Why App Store is all `Retained`:** none of its resources carries an ownership marker, so willikins cannot prove at
delete time what it owns. Apple's deletes cascade (an identifier takes its profiles), and the operator has said these
resources are kept. The live cycles still delete their throwaway identifiers by id, as before, outside rollback.

### 5. Credentials: re-resolved from the document, never from the journal

The journal keeps full non-secret values (masking is an output-surface post-pass, `disclosure.rs`) but only markers
for secrets. A `Delete` tool's `needs` may name a secret credential port (`github.repo.ensure`'s and
`buildkite.pipeline.ensure`'s `token`). Under the design's "credentials are ports, resolvers are nodes", such a port is
bound to a **pure** resolver node, so:

- The teardown requires the workflow document, by name from the trusted directory, **at the run's recorded
  `document_sha256`**. A changed document refuses with the existing `DocumentChanged`.
- New `willikins_core::plan::resolve_pure(checked, catalog, inputs, wanted)` evaluates, in `Checked::order` and from
  the recorded workflow inputs (rebuilt with `Butler`'s existing `parse_recorded_value`), **only the backward closure
  of `wanted`**: the pure nodes the `Delete` steps' secret `needs` ports are bound to, and their own pure ancestors.
  Pure includes gates that read providers (`appstore.app.get`, `doppler.secret_name.gate`). Evaluating every pure node
  would make deleting a GitHub repository need App Store credentials. The resolver chains are all a teardown reads. A
  pure node bound to an impure node's output resolves `Unknown` and is not read.
- Each `needs` port takes its non-secret value from the journal (`NodeStarted.inputs`, re-parsed by its declared
  type) and its secret value from `resolve_pure`. An unbound optional credential port stays unbound, and the tool
  falls back to its execution-context credential exactly as forward apply does. A secret `needs` port bound to an
  impure node is `TeardownError::UnresolvableCredential { node, port }`.

**Why not re-plan the whole document:** a forward re-plan reads every node. After a half-finished teardown it reads
`Absent` for what is gone, and `plan` turns other states into hard errors (`AttributeMismatch`). The pure subgraph is
the only part a teardown needs, and it is exactly the part that must be evaluable before anything exists.

### 6. Order, and what a teardown plan is

`plan_teardown(facts, checked, catalog, resolved) -> Result<Teardown, TeardownError>` is a pure function of the run's
journal facts, the checked document, and what each inverse observes. Its steps are the run's applied plan instances
**in reverse `Plan::nodes` order**, which is a topological order of the graph reversed: dependents come before what
they depend on. A rider is listed in its own position as `RidesWith { container }`. Only `Delete` steps call a
provider at apply time.

If any candidate's standing is `NotOurs` or `Drifted`, `plan_teardown` returns
`TeardownError::Refused { steps: Vec<(InstanceRef, &'static str)> }` and **nothing is recorded as approvable**. The
operator fixes the drift by hand (or deletes the thing by hand, which then reads `Gone`) and plans again. A partial
teardown that deletes some things and refuses others is never offered. It would be worse than one that never started
(`deploy/teardown.sh`'s rule).

### 7. Drift: what "changed since this run created it" means, per Delete tool

Every rule below runs inside the tool's own `standing`, from one or a few `GET`s, and returns a static reason.

| Tool | `NotOurs` when | `Drifted` when |
| --- | --- | --- |
| `github.repo.ensure` | `404` → `Gone`; any `3xx` (renamed or transferred: `Http` follows no redirect); `full_name` ≠ the recorded key byte for byte; topic `managed-by-willikins` missing; `created_at` outside the window | `pushed_at` > `settled_by`; `open_issues_count` > 0 (issues and pull requests); `forks_count` > 0 |
| `doppler.project.ensure` | `description` ≠ `managed-by: willikins`; `created_at` outside the window | an environment other than `dev`/`stg`/`prd` or a rider's; a config other than those environments' roots or a rider's; any config's secret names (`include_managed_secrets=false`, names only) not written by a rider `doppler.secret.set` into that config or inherited (decision note below); a config with `inheritedBy` naming a config outside this project; a service token whose name no rider created |
| `doppler.config.ensure` (environment) | the project lacks the marker; the environment's `created_at` outside the window | a config in the environment that no rider created; secret names, `inheritedBy`, tokens as for the project, scoped to the environment |
| `doppler.branch_config.ensure` | the project lacks the marker; `root` is true; `environment` differs; `created_at` outside the window | secret names, `inheritedBy`, tokens as above, scoped to the config |
| `doppler.service_token.ensure` | the project lacks the marker; no token at `(config, name)` created in the window | more than one token at `(config, name)` |
| `buildkite.pipeline.ensure` | `description` ≠ `managed-by: willikins`; `created_at` outside the window; the slug differs | any build exists (deleting a pipeline deletes its build history, which this run did not make) |

**Inherited names.** The names endpoint lists inherited names (`docs/solutions/providers/doppler-names-endpoint-lists-inherited-names.md`).
A name visible only through a base config the rider `inherits.ensure` added is not drift. Implementation: a name
counts as drift only if it is not written by a rider and not present in any base the config inherits (one names read
per base, the 3j walk's shape).

**Marker-less resources inside a project willikins does not own** (a config this run created in an operator-made
project) are `NotOurs`, so the whole teardown refuses. The Doppler client's own rule applies: configs and tokens are
willikins' only under an owned project. The operator deletes such a config by hand and re-plans; it then reads `Gone`.

### 8. Journal, approval, refusals

- **Teardown plans share the `PlanId` namespace**, so the approval and apply windows (`ButlerConfig`) carry over.
  `approve`/`reject` and their replay must resolve a `plan_id` in either `PlanRecord` or the new `TeardownRecord` (R7).
  `TeardownPlanned { plan_id, of_run, workflow, document_sha256, teardown: Redacted<Teardown>, fingerprint:
  Vec<StepFingerprint>, class, requires_approval: true, principal }` populates the `TeardownRecord`. `Teardown` joins
  `Redactable`'s sealed set. `Redacted` JSON is one-way, so the apply-time comparison reads the **typed**
  `StepFingerprint { node, instance, action, standing }` (action and standing as discriminants, no values). This is
  the role `PlanRecorded.fingerprint` plays for forward apply.
- **A teardown run** is `TeardownStarted { run_id, plan_id, of_run, principal }`, one `TeardownStepFinished { run_id,
  node, instance, status }` per step (`Deleted`, `AlreadyGone`, `Failed { error }`, `NotRun`, `Listed` for every
  non-delete step), then `TeardownFinished { run_id, outcome }` (`Succeeded` or `Failed { error }`). Separate events
  keep forward `Outcome`, `NodeStatus` and `RunRecord` byte-identical. An older binary cannot read a journal holding
  them, the forward-compatibility cost `Outcome::Blocked` already documents.
- **On replay**, a forward `RunRecord` gains `rolled_back_by: Option<RunId>` (`skip_serializing_if = "Option::is_none"`),
  set when a teardown of it finishes `Succeeded`.
- **Apply** mirrors forward apply's rules: refuse an unknown plan, an expired window, a missing approval, a changed
  document, or a second apply (`AlreadyApplied`). Then re-plan the teardown afresh and compare its fingerprint with
  the approved one step by step (same instances, same actions, same standings). A difference is `ApplyRefused { reason: Drift { …, detail: Standing }
  }`, and nothing is deleted. Then mint the `TeardownToken`, run every `Delete` step in order, and stop at the first
  failure. The remaining steps are `NotRun`.
- **Refusals at teardown plan time** (journaled as `ApplyRefused` with the new reasons): the run is a teardown
  (`NotAForwardRun`); it has no `RunFinished`, because it is still running or the process died (`RunUnfinished`:
  `settled_by` is undefined for it; the existing `RunInProgress` means another run holds the apply lock and stays
  for that); a teardown of it already succeeded
  (`AlreadyRolledBack { run_id }`); or a **later** forward run of the same workflow in this journal mutated something
  (any planned `Create`/`Replace`/`Update` that finished `Created`) and has not itself been rolled back
  (`LaterRunFirst { run_id }`).
- **A half-finished teardown resumes by planning again.** What it deleted reads `Gone`. The run is marked rolled back
  only when a teardown finishes `Succeeded`. A teardown with no `Delete` step (everything `Retained`, `Gone` or
  `MadeByHand`) is still recorded, approved and applied, so the run is marked and an earlier run is no longer blocked
  by `LaterRunFirst`.

**Why LIFO per workflow:** a later run of the same document may have built on what this one created (a config in its
project). Its own teardown is the only one that knows what it added. A different document's later additions are caught
by the container drift checks instead.

### 9. The CLI

`willikins rollback <file> --run <run-id> --journal <path> [--live | --fake-state F] [--fake-state-out F]` plans a
teardown, records it, and prints it: steps in order, uncertain deletes apart, then "rides with", "already gone", "left
in place" (each with its reason), and "made by hand". It prints the `plan_id` and the `approve` command, and always
requires approval. `willikins rollback --plan-id <id> --journal <path> [--live | ...]` applies an approved teardown.
`--json` and `--reveal` behave as everywhere else. Exit codes are `apply`'s, plus `1` for a refused (drifted)
teardown. `<file>` is copied into the CLI's private trusted directory exactly as `apply <file>` does, so its hash is
checked against the run's.

`deploy/teardown.sh` is **retired** once L4 has done its job live: task D1 deletes it and points the HANDOFF and
README at `willikins rollback`.

### 10. The forward plan names what rollback could not undo

`Plan::not_reversible` lists every instance planned `Create`, `Replace` or `Update` whose rollback would not undo it,
in the plan's order. Each entry is `{ node, instance, tool, why: &'static str }`:

- the tool is `Retained`;
- the tool is `Contained` and no container instance in the same plan is planned `Create` with matching values;
- or the instance is `Replace`/`Update`, whatever the tool declares.

Rendered after `replacing` in text, and present in `--json` and MCP plan results. Computed at plan time from the same
`Within` matching as decision 3, so the forward plan and the teardown cannot disagree. **The cost:** every
characterization snapshot of a document with such a node gains this array. The diff review accepts only added
`not_reversible` arrays.

## Acceptance tests

1. **Declarations validate** (R1, R3): `Catalog::insert` refuses a `needs` or `within` port that is not an input, a
   `Delete` without an inverse, an inverse without `Delete`, and a pure tool declaring anything.
2. **`TeardownToken` is gated** (R2): a trybuild compile-fail without `executor`, and a tripwire test (beside
   `sink_token_guard.rs`) that no source outside `rollback.rs` names `TeardownToken::new`.
3. **`resolve_pure`** (R4) evaluates a resolver chain (`env.get` → parse), leaves a pure node bound to an impure output
   `Unknown`, and calls no impure tool's `read` and no pure node outside the wanted closure (a gate stub counting
   zero reads).
4. **The candidate predicate** (R5): `Create`+`Created` is a candidate; `Update`+`Created`, `Replace`+`Created` and
   `NoOp`+`Unchanged` are not; `Create`+`Failed`+`AsCreated` gives `Delete { uncertain: true }`; a started, unfinished
   instance is `Unfinished` and never deleted.
5. **Order and riding** (R5): steps come out in reverse plan order. A config rides with the project created in the same
   run. A config inside a pre-existing project is its own `Delete`. Mismatched values do not ride. A `Create` +
   `Unchanged` root config is a transit rider, and a secret set into it rides with the project (the
   `new-rust-service` shape).
6. **Refusal is total** (R5): one `Drifted` step refuses the whole teardown, with no `TeardownPlanned` recorded.
7. **Apply re-observes** (R6): a standing that changes between approval and apply refuses with `Drift`/`Standing`
   before any delete (a counting stub sees zero deletes). A delete failure stops the run, and later steps are
   `NotRun`.
8. **Resume** (R6, R8): after a half-finished teardown, planning again reads the deleted steps `Gone`, and the second
   teardown succeeds and marks the run.
9. **Journal** (R7): every new event round-trips. A journal written before this milestone replays byte for byte.
   `redaction_by_construction.rs` seeds a secret into a credential port of a teardown's `Created` and finds no byte
   of it in any line.
10. **Refusals** (R8): `AlreadyRolledBack`, `LaterRunFirst`, `RunUnfinished`, `NotAForwardRun`, `DocumentChanged`,
    `ApprovalRequired`, `PlanExpired`, each journaled.
11. **CLI round trip on fake state** (R9): `apply --fake-state S --fake-state-out A`, then `rollback --run R
    --fake-state A`, then approve, then `rollback --plan-id P --fake-state A --fake-state-out B`. `B` holds none of
    the run's created resources and everything seeded in `S`.
12. **Per-tool drift** (P1–P4): a mock test per `NotOurs`/`Drifted` row of decision 7, each pinning the exact query,
    including a `3xx` repo and a `created_at` one second outside the window.
13. **The declaration table is pinned** (P7): one snapshot of every live tool's reversal. Adding a tool without a
    declaration changes the snapshot.
14. **Delete call sites are inventoried** (P8): a guard lists every `.delete(` and `delete_with_body(` call site in the
    provider crates by file and enclosing function. A new one fails until reviewed.
15. **Forward visibility** (F1): a plan with a scaffold node and no repo `Create` lists the scaffold in
    `not_reversible`. With a repo `Create`, it does not.
16. **Live, sandbox only** (L1–L4): create, roll back, `GET` is `404`. Create, drift (a push, a hand-set secret), and
    the teardown refuses with the drift named. The test then deletes by hand.
17. **Adversarial pass** (X1), recorded under `docs/research/`. Every bypass becomes a fixture or a test.

## Verify before relying on them

Each item is settled by fetching the primary source verbatim, or by the first live cycle that touches it (the result
is recorded as an addendum here and in `docs/solutions/providers/`).

1. GitHub `GET /repos/{owner}/{repo}` carries `created_at`, `pushed_at`, `open_issues_count`, `forks_count`,
   `full_name` and `topics`, and **`createCommitOnBranch` bumps `pushed_at`** (L1: if it does not, the scaffold is
   invisible to the drift rule, and a push after the run is still caught).
2. GitHub `DELETE /repos/{owner}/{repo}` answers `204`, and `403` when the org forbids member deletion (the live write
   cycle already met that body).
3. Doppler's environment delete removes the environment's branch configs, or refuses while they exist. Either way, the
   environment's drift rule must hold every branch config as a rider or refuse.
4. Doppler refuses to delete a config another config inherits, or does not. The `inheritedBy` drift rule is correct
   either way, and L2 records which.
5. Doppler's service token list carries `slug` and `created_at` (fetched 2026-10-05 from
   `service_tokens-list.md`: both appear in the example). Confirm live in L2.
6. Doppler's project, environment and config objects carry `created_at` (fetched 2026-10-05: `projects-get.md`,
   `environments-get.md`, `configs-get.md` examples). Confirm the format parses as RFC 3339 in L2.
7. A new Doppler project arrives with exactly `dev`, `stg` and `prd` (m1 research note, and HANDOFF's "three
   auto-created root configs"). L2 records the list a fresh sandbox project shows.
8. Buildkite's pipeline carries `created_at`, and `GET /v2/organizations/{org}/pipelines/{slug}/builds?per_page=1`
   lists builds. `DELETE` answers `204`.
9. Doppler's three delete endpoints (`DELETE /v3/projects/project` with a JSON body, `DELETE
   /v3/environments/environment?project&environment`, `DELETE /v3/configs/config?project&config`) are as fetched on
   2026-10-05 (`projects-delete.md`, `environments-delete.md`, `configs-delete.md`). The project delete takes a
   **body** (`Http::delete_with_body`); the other two take a query.

## Gates

Scoped per task, as each row says. `-j 2`, `RUST_TEST_THREADS=2`, in the background with a 600,000 ms timeout,
reading the log body. Never two cargo commands at once. The coordinator runs the full four-command gate once after
F1 and once at the end.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| R1 | **`Reversal` and `Within`** (decision 3; acceptance 1). `rollback.rs` with the declaration types, `Tool::reversal()` defaulting to `Retained { why: UNDECLARED }`, and `Catalog::insert`'s port validation. No tool changes. Scoped: `-p willikins-core` | sonnet implements |
| R2 | **`TeardownToken`** (trust boundary 2; acceptance 2). `sink.rs`, the `clippy.toml` row, the trybuild case, the tripwire test. Scoped: `-p willikins-types`, `-p willikins-core --test sink_token_guard` and the new guard, then `cargo check -p willikins-types` | sonnet implements |
| R3 | **`Inverse`, `Created`, `Standing`, `Deleted`** (decision 3). `Tool::inverse()`, and `Catalog::insert`'s "`Delete` exactly when `inverse()` is `Some`". Test stubs in `testing.rs`. Scoped: `-p willikins-core` | sonnet implements |
| R4 | **`resolve_pure`** (decision 5; acceptance 3). In `plan.rs`, sharing `plan`'s binding resolution. Not a second probe of the conversion table: the tripwire test that greps `plan.rs` must still pass. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R5 | **`plan_teardown`** (decisions 1, 2, 4's gate rows, 6; acceptance 4–6). `RunFacts` (built by callers, so core does not depend on the journal crate), `Teardown`, `TeardownStep`, `TeardownAction`, riding, ordering, total refusal. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R6 | **`apply_teardown`** (decision 8's apply; acceptance 7, 8's core half). Re-plan, compare, mint, delete in order, stop on failure. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R7 | **Journal** (decision 8; acceptance 9). The four events, the new reasons, `Teardown: Redactable`, the typed `StepFingerprint`, `TeardownRecord`, approval and rejection replay resolving either record kind, `RunRecord::rolled_back_by`, the pre-milestone replay test, and the redaction case. Scoped: `-p willikins-journal` | sonnet implements |
| P1 | **GitHub inverse** (decisions 4, 7; acceptance 12). `delete_repo`, a repo read with the decision 7 fields, `GitHubRepoEnsure`'s `Delete` and inverse; `Contained` for `actions_secret` and `scaffold`. Mock tests pin each query. Commit 1: the client. Commit 2: the tool. Scoped: `-p willikins-providers-github` | sonnet implements, opus attacks |
| P2 | **Doppler project inverse** (decision 7's project row). `delete_project` (body), `delete_environment`, `delete_config`, and the list reads (environments, configs with `created_at`/`inheritedBy`, names, tokens). Commit 1: the client. Commit 2: `doppler.project.ensure`. Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| P3 | **Doppler config, branch config and token inverses**, and the `Contained`/`Retained` declarations of the other six Doppler tools. Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| P4 | **Buildkite inverse.** The builds read, `buildkite.pipeline.ensure`'s inverse, `bootstrap.ensure` `Contained`; `delete_pipeline`'s doc now names its one non-test caller. Scoped: `-p willikins-providers-buildkite` | sonnet implements |
| P5 | **Retained declarations**: the three App Store tools and SigNoz, with decision 4's reasons verbatim. Scoped: `-p willikins-providers-appstore -p willikins-providers-signoz` | sonnet implements |
| P6 | **Fake twins**: the same declarations on every fake tool, fake inverses for the six `Delete` tools over `FakeState` (no timestamps: the fake skips the window rule, which the mock tests pin instead), parity of declarations in `catalog_parity.rs`, and `fake_agrees_with_live.rs` rows for standing. Scoped: `-p willikins-providers-fake` | sonnet implements |
| P7 | **Declaration table** (acceptance 13). `crates/willikins-server/tests/reversal_table.rs` over `LIVE_TOOL_NAMES`, plus the `Within` type check of decision 3. Scoped: `-p willikins-server --test reversal_table` | sonnet implements |
| P8 | **Delete call-site guard** (acceptance 14). `crates/willikins-cli/tests/delete_call_sites_guard.rs`, written like `no_gh_writes_guard.rs`. Scoped: `-p willikins-cli --test delete_call_sites_guard` | sonnet implements |
| R8 | **`Butler::plan_rollback` / `apply_rollback`** (decisions 5, 8; acceptance 8, 10). `RunFacts` from the journal through `parse_recorded_value`, the document hash check, `resolve_pure`, LIFO, double rollback, windows. On the fake catalog. Scoped: `-p willikins-server` | sonnet implements, opus attacks |
| R9 | **CLI `rollback`** (decision 9; acceptance 11). The subcommand, text and JSON rendering through `mask_json`, and the fake-state round trip test. Scoped: `-p willikins-cli` (the new tests and `render` only) | sonnet implements |
| F1 | **`Plan::not_reversible`** (decision 10; acceptance 15). Commit 1: the field and its computation in core, with tests. Commit 2: rendering and every characterization snapshot, diff-reviewed (only added arrays). Scoped: `-p willikins-core`, then `-p willikins-dsl -p willikins-cli` | sonnet implements |
| L1 | **GitHub live rollback cycle** (acceptance 16, verify 1–2), behind `live-tests`, in the sandbox org only. Written and compiling, **never run by the implementer**. Scoped: `cargo clippy -p willikins-providers-github --features live-tests --all-targets -j 2 -- -D warnings` | sonnet writes, coordinator runs once |
| L2 | **Doppler live rollback cycle** (verify 3–7, 9), as L1, in the sandbox workplace | sonnet writes, coordinator runs once |
| L3 | **Buildkite live rollback cycle** (verify 8), as L1, in the sandbox org | sonnet writes, coordinator runs once |
| L4 | **End to end through the CLI**: `apply` then `rollback` of `workflows/new-rust-service-buildkite.yaml` in the sandbox, with a scratch journal. A coordinator runbook in this plan's addendum, not a test | coordinator |
| X1 | **Adversarial pass** (acceptance 17), recorded under `docs/research/2026-10-0x-m3m-adversarial-pass.md`. Priority targets: a delete reachable without a `TeardownToken` or from a document; a candidate that the run did not create (Update, Replace, NoOp, another run's); a rider deleted on its own; a container deleted despite foreign contents (a hand-set secret, an inheriting config elsewhere, a build, a push); a window bypass by clock or format; a `3xx` repo followed; a credential read from the journal; a value in a reason; LIFO bypass; a teardown applied after its standing changed | opus |
| D1 | **Docs**: retire `deploy/teardown.sh` after L4, add a design-doc addendum and a CLAUDE.md invariant (edit `AGENTS.md` identically), update the HANDOFF, close the todo, and write the MCP follow-up todo | coordinator |

## Risks

- **Over-refusal.** The drift rules are strict on purpose: a pipeline with one build, or a repo with one issue, refuses.
  The remedy is by hand and stated in the reason. Loosening a rule is a later decision with its own plan.
- **Clock skew larger than two minutes** turns a resource willikins created into `NotOurs`, which refuses (safe) and
  never deletes the wrong thing.
- **Cross-document overlap.** Two documents creating inside one container are separated only by the container's drift
  check, not by LIFO. A rider from the other document's run is foreign contents, so it refuses.
- **The journal is the source of truth.** A truncated journal (`todos/2026-09-15-journal-repair-subcommand.md`) can
  lose the `NodeFinished` that bounds a window. That instance is then `Unfinished`, never deleted.
- **Characterization churn** from F1 touches many snapshots. It is diff-reviewed as additions only, and lands last
  among the core tasks so no other task rebases over it.
- **Deletes are permanent.** No live test runs outside the sandbox. The probe prefix and the sandbox variables are
  required, and a live cycle rolls back only the run it just made, from a journal it wrote.
- **Privacy.** No real name in this plan, the tests, or the fixtures. The hooks refuse one, and it must be rewritten,
  never bypassed.
- **Host contention.** If no quiet three-second window comes within 40 minutes, the task stops and reports the
  contention.

## Needs the operator

1. **`appstore.profile.ensure`: keep `Retained`, or give it an inverse?** A created profile is deletable by its
   recorded id, but it carries no marker, and CI fetches profiles by name. This plan keeps it, as with every App Store
   resource. A yes adds one task after P5.
2. **A pipeline with builds: drift (this plan) or deletable?** Deleting the pipeline takes its build history.
3. **Sandbox credentials for L1–L3** in `~/.config/willikins/sandbox.env`: the GitHub PAT with repository
   administration in the sandbox org (already there), the sandbox Doppler token (already there), and a Buildkite token
   able to delete pipelines in the sandbox org (the renewed one is broad enough; confirm).
