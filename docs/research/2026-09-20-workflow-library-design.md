# The workflow library: listing, composing, and proposing documents

**Created:** 2026-09-20
**Status:** design. Nothing here is built. It decides shapes, not a schedule.
**Related:** `docs/plans/2026-09-11-willikins-design.md` (the invariants),
`docs/research/2026-09-20-project-survey-and-workflow-library.md` (the evidence and the
nine-document catalogue), `todos/2026-09-20-agent-authored-workflows.md` (the operator's
correction that this design starts from), `todos/2026-09-20-remote-mcp-from-a-phone.md`,
`todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md`,
`todos/2026-09-16-unattended-agents-token-longevity.md`,
`todos/2026-09-16-pluggable-auth-adapters.md`

The operator, 2026-09-20: *"when the user wants to create a new project, willikins would allow
listing the existing workflows, with a name, set of technologies, etc, and let the agent pick one.
Or compose existing ones. Or make a new one."*

Three asks, and they are not equally hard. **Listing** is a field or two on a response type that
already exists. **Composing** is milestone 2b, already scheduled, plus one DSL keyword. **Making a
new one** is the whole design problem, because a workflow document chooses which tools run against
which resources with the operator's credentials, and the design doc says documents are privileged
content run only from a trusted ref. Stated naively, "an agent writes a workflow and it is stored"
is an agent granting itself new authority. §1 is therefore the longest section and the rest
depends on it.

---

## 1. Trust: the invariant does not move

### 1.1 The reconciliation, in one paragraph

**Nothing about the trust model changes.** A document still runs only when it is in the trusted
source; `plan` and `apply` still take a name resolved there and never a document body (milestone 2
plan, trust boundary 3); the graph is still statically checked before anything runs. What is added
is a **proposal**: a journaled, statically-checked, *unrunnable* draft, plus a **promotion** — a
human act, performed by the operator through their own trusted source, that turns a proposal into a
document. The agent never crosses that line. It writes text and asks; a human puts the file where
willikins reads from. That is the same shape as the approval gate, one level up, which is exactly
the correction recorded in `todos/2026-09-20-agent-authored-workflows.md`: *"The tool could
explicitly tell the agent to check with the user (by returning a confirmation prompt / state on the
initial creation request)."*

### 1.2 Why a draft must never be plannable — the load-bearing argument

This is the one thing in this design that must not be got wrong, and it is not obvious.

`plan` accepts a workflow *name* only, and acceptance test 13 pins that the `plan` parameter schema
has no `document` field at all (`crates/willikins-server/tests/acceptance_13_trusted_directory.rs`;
`crates/willikins-server/src/mcp.rs`'s `PlanParams` says so in a comment). It would be easy to read
that as belt-and-braces — `plan` writes nothing, so why not let an agent plan its draft and show
the human what it would do?

Because **a plan whose every node is `Reversible` auto-approves**. `Checked.class` is the maximum
over the graph (`crates/willikins-core/src/check.rs:92-94`), `Class::requires_approval` is false
for `Reversible` (`class.rs:35-38`), and `Butler::plan` returns `ApprovalRequirement::Automatic` in
that case. `apply` consumes a `plan_id`. So if a draft could be planned, an agent could author a
document of entirely reversible steps, plan it, and apply it, with no human anywhere in the loop.
The trusted-directory restriction on `plan` is not defence in depth. **It is the gate.** The
existing positive fixture proves the case is real rather than theoretical:
`workflows/new-rust-service-buildkite.yaml`'s own header says *"every node is `Reversible`"* — a
repository, a Doppler project, its configs and a Buildkite pipeline, all auto-approving.

Consequences, stated so nobody re-derives them later:

- A proposal is **never** a `DocumentSource::Body` reaching `plan`. `validate` and `describe` stay
  the only body-accepting operations, they call no provider, and that stays true.
- A proposal produces **no `plan_id`**. Whatever preview a reviewer gets must not be a currency
  `apply` accepts.
- "Let the reviewer see a real plan against live state" is deferred (§1.7). It is the right feature
  eventually and it is not free: it is a provider-calling action on the approvals page, which is
  the surface milestone 2c is currently hardening, and adding one before 2c lands changes 2c's
  threat model. v1 review is static.

### 1.3 The three tiers, and what a closed composite actually buys

The task asks whether composing already-trusted documents is safer than authoring new tool calls.
It is, but the naive reason ("every tool in it is already trusted") is false, and the true reason
is narrower and checkable.

Composition adds authority that neither part had, in three ways. **New literals**: a composite may
bind any port to a value it chooses, and a literal is an attacker-chosen value, so a composite with
literals is authoring with extra steps. **New targets**: the same tool set pointed at a different
org, project or cluster is a different blast radius. **A removed human**: applying A and then B is
two plans and two approval gates with a human seeing A's outputs in between; the composite is one.

So define the safe subset syntactically, so `check` can decide it:

> **A closed composite** is a document in which every step is a workflow call (`uses:`, §2), no
> step calls a primitive tool, every `with` binding and every `outputs` entry is a reference
> (`${{ … }}`) rather than a literal, and no declared input carries a `default`.

Two properties follow. First, its authority over any run is a function of the input values the
caller supplies at plan time — the same position a caller is in for any trusted document, because
the composite introduces no value of its own. (An input `default` *is* a literal in disguise: it is
substituted at plan time and, on an auto-approving graph, no human ever sees it. Hence the ban.
The blunt rule costs an inert `description:` literal; a later refinement can let the *type* decide,
since `Description` is inert where `RepoVisibility` and `GitHubOrg` are authority-bearing. v1 keeps
the blunt rule, because nine catalogue documents do not justify a second mechanism.)

Second — and this rules out the risk this design initially worried about — **a closed composite
cannot create a new secret edge.** The design doc's workflow-inputs rule is that a signature may
not contain a secret type (*"No secret input types. The checker rejects a signature containing a
secret type"*). A closed composite's steps are workflow calls, so its only input ports are other
documents' signatures, so none of them accepts a secret. phil-connors' nested token — a
`prd_app-ios` service token stored as `DOPPLER_TOKEN_APP_IOS` inside `prd_deployment`, the survey's
§6 Rank 7 example — is a *primitive-level* secret-output-to-secret-input binding. It is expressible
only in a document that calls primitives, which is Tier 2.

| Tier | What it is | Promotion review | Plan-time gate |
| --- | --- | --- | --- |
| 0 | A document already in the trusted source | n/a | unchanged: `check` at startup, plan class, approval |
| 1 | A **closed composite** of Tier-0 documents | The reviewer reads a list — *these N named workflows, in this order, wired this way* — not a diff. Machine-checkable, so the review is seconds | unchanged |
| 2 | Anything else: any primitive step, any literal, any input default | The reviewer reads the document text, with the static summary beside it | unchanged |

The tiers change **only how a promotion is reviewed**. They never change the plan gate, and a Tier
1 composite of three `Irreversible` documents still plans to `Irreversible` and still waits for a
human on every run (§2.3). That is why a lighter promotion review is tolerable: the two gates
compose (§1.6).

### 1.4 Promotion is the operator's act, through the operator's own mechanism

The first draft of this idea, recorded and corrected in
`todos/2026-09-20-agent-authored-workflows.md`, was "the proposal becomes a pull request against
the trusted repository". The operator rejected it: *"We won't be storing other people's workflows
in **our** repository. It will be up to them how they decide to store / version them."* willikins
is self-hosted software; what counts as the trusted ref is a property of a deployment, not of this
project, and willikins must not assume a repository, let alone this repository's review
conventions.

So promotion is a **trusted-source adapter** — the same seam word as
`todos/2026-09-16-pluggable-auth-adapters.md`, deliberately, because it is the same kind of seam: a
default that needs no external system, plus room for an organisation that already has one.

**`manual` — the default, and the only one available to this deployment today.** willikins writes
nothing. A proposal sits in the journal; the operator reads it (CLI, or the proposals page), puts
the file in their trusted source however they normally would, and restarts or rescans. Promotion is
detected by **sha match on scan**: a trusted document whose `document_sha256` equals a pending
proposal's closes that proposal as `promoted`, recording who and when. There is deliberately no
approve button here, because *the operator placing the file is the review* — a button would be
ceremony in front of an act that already happened. A proposal is advisory; the trusted source is
authoritative. An operator who fixes a typo before placing the file leaves the proposal to expire
unmatched, and that is correct behaviour, not a bug to design around.

**`directory` — a willikins-owned directory.** On approve at the proposals page, the butler writes
`<trusted-dir>/<name>.yaml` and rescans. Requires that the trusted directory is willikins' own, not
a git checkout: writing an untracked file into a checkout collides with the operator's next pull,
and the survey's own estate is full of files that exist in one place and are believed to live in
another. This repository's trusted directory (`workflows/`) is a checkout, so **this deployment
gets `manual` until an operator reconfigures it.**

**`git` — offered, never privileged.** The butler commits the document on a branch of the trusted
checkout and, optionally, opens a review on whatever host the operator uses. Beyond the operator's
correction, there is a concrete cost worth naming: willikins would need a **push credential to the
trusted source's remote**, which is a new class of server-held credential — the thing this
workspace is most careful about (`no_gh_writes_guard.rs`, and the survey's release tokens scoped to
exactly two repositories). A credential that can write the documents that decide what willikins
does is the highest-value credential in the system. That is why this adapter is available and is
not the default.

### 1.5 Refusals that belong in v1

- **A proposal naming an existing trusted document is refused.** Replacing a document changes what
  that name means for every future run and for every composite over it. That goes through the
  operator's own source, where their diff review lives, not through a proposal. (The *name* is the
  natural key: `document::load_named_document` resolves `<name>.yaml` and `check` refuses a
  document whose internal `name:` disagrees with its filename.)
- **A proposal that fails `check` is not stored.** It comes back as check errors, and the agent
  iterates with `validate`/`describe`, which is what that loop is for. The proposal store holds
  only documents that would pass startup's own scan.
- **A proposal is capped and expires.** Same reasoning as a pending plan's approval window: an
  unbounded store of agent-written text is a liability, and a proposal nobody promoted within its
  window was not wanted.

### 1.6 The two gates compose

Promotion answers *"may this text ever run here?"*. Plan approval answers *"may this run, now,
with these inputs, against this state?"*. They are different questions and both survive:

- A promoted Tier 1 composite over three `Irreversible` documents still plans to `Irreversible`,
  still requires an approver, every run.
- A promoted document's plan still carries its `document_sha256`, so `apply` still refuses a
  document that changed between plan and apply (`ButlerError::DocumentChanged` — the plan-identity
  attack from `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`).
- Promotion never pre-approves anything. There is no "trusted document, so skip the gate" path,
  and adding one later would be the mistake this section exists to prevent.

### 1.7 Provenance lives in the journal, never in the file — and it is the attenuation hook

**No field an agent could write may be read for a trust decision.** A document is a file; an agent
that authors a file authors every field in it. A self-declared `technologies: [docs]` on a document
that calls `doppler.secret.set` is the obvious attack, and it is the reason every discovery field
in §3 is computed from the graph rather than asserted in the document.

Provenance is the same rule applied to authorship. Who proposed a document, who promoted it, and
when, cannot live in the document. It lives in the journal, as an additive event —
`WorkflowProposed { proposal_id, name, sha, by }` and `WorkflowPromoted { name, sha, by, adapter }`
— which also keeps the audit trail honest: today the journal records the trusted directory's hash
set once, at `ServerStarted.workflow_hashes` (`crates/willikins-server/src/butler.rs:171-186`), so
a document that appears mid-session without an event would be a silent gap in the ledger.

This is cheap now and expensive to retrofit, and it is the hook the operator's own "stronger
version" hangs on. From the same todo: *"Later we could have those use different credentials, if
willikins is hosted on a backend."* That is **attenuation** — an agent-authored document running
with a narrower credential set than an operator-authored one — and it is the same landing place as
the capability tokens in `todos/2026-09-16-unattended-agents-token-longevity.md` (the operator's
"Biscuits and Macaroons"), reached from the opposite direction. Attenuation is not v1. Recording
who authored what, so that a later policy can *say* "agent-authored documents may not touch
production", is.

---

## 2. Composition: 2b plus one keyword

### 2.1 Not a new DSL feature

The design doc already claims it: *"Workflows are tools. Same typed interface, so a composite is a
node in a larger graph. Primitives are Rust, composites are DSL, one abstraction."* Milestone 2b
exists to make `Workflow` implement `Tool` with typed composite output ports. A document already
declares its input ports (`inputs:`) and its output ports (`outputs:`). So composition is not a new
capability to invent; it is 2b landing, plus three specific things 2b's plan must cover.

### 2.2 A `uses:` keyword, not a `ToolName`

There is a concrete obstruction, and it decides the syntax. `ToolName`'s grammar is dotted
snake_case (`^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$`, `crates/willikins-core/src/tool.rs:24-26`).
`WorkflowName`'s is kebab (`[a-z][a-z0-9]*(-[a-z0-9]+)*`,
`crates/willikins-types/src/workflow_name.rs`). So `tool: workflow.new-rust-service-buildkite`
**does not parse today**, and every document in `workflows/` is kebab-named.

Three ways out: relax `ToolName` to admit hyphens, map kebab to snake on the way in, or add a
field. Add the field: **`uses: <WorkflowName>` on `StepDecl`, mutually exclusive with `tool:`.**

- It makes composition syntactically visible without resolving anything against the catalog, which
  is exactly what the closed-composite check needs: *every step has `uses`, none has `tool`, no
  literal in any `with` or `outputs`, no input `default`* is a pure syntax predicate over the
  parsed document.
- `StepDecl` carries `#[serde(deny_unknown_fields)]`, so an older willikins meeting a composite
  **fails closed** with a located error rather than silently ignoring the field and running a step
  with no tool.
- It removes a whole class of ambiguity: a primitive and a workflow can never collide in one
  namespace, so nobody has to decide what happens when a document is named after a tool.

The "one abstraction" claim survives intact: **one abstraction in `willikins-core`** (both
implement `Tool`, both have typed ports, `check` treats a composite node like any other),
**two spellings in `willikins-dsl`**, because the two name grammars are genuinely different and
pretending otherwise costs more than a keyword.

### 2.3 What 2b's plan must cover

- **Class is the max over children**, by the same `Class::max_of` the checker already uses for a
  flat graph. A composite containing one `Irreversible` node is `Irreversible`.
- **Cross-document acyclicity.** Today's cycle check is within one document
  (`workflows/fixtures/cycle.yaml`). A composite can cycle through two files, and the check must
  run over the resolution graph at scan time, so a cycle refuses startup rather than recursing at
  plan time.
- **Output port types.** `Checked.output_types` already resolves each output's type; a composite's
  output port takes it. If an output binds to a secret-typed value, the composite's port is
  secret, and the existing taint rule then applies to whatever binds it.
- **Children are pinned by name, not by sha.** A composite says `uses: new-rust-service-buildkite`,
  and it resolves to whatever that name means in the trusted source now. Convergence is the
  feature (§5), and the child's change was already reviewed by the operator's own process when it
  entered the trusted source, so it does not reopen review of every composite over it. What the
  *proposal record* must capture is the children's shas **at review time**, so the reviewer's
  decision stays auditable against what they actually read; and `list_workflows` reports a
  composite's currently resolved children with their shas (§3), so "what does this mean today" is
  one call rather than archaeology.

### 2.4 What this buys the catalogue

The survey's §5 catalogue is nine documents whose intended use is *"#1 + one of {#2..#6} +
optionally #7, #8, #9"*. That sentence is a closed composite. `project-docs-scaffold` (#1) plus
`new-ios-app-in-monorepo` (#5) plus `buildkite-pipeline-for-monorepo-app` (#7) is Walter, and it is
three `uses:` steps wiring the same `slug` and `monorepo` inputs through. The organisation already
does this by hand and says so in comments — pessimal's `MODULE.bazel` copies *"the set proven in
kumbaya, phil-connors, bande-a-bonnot"*, its pipeline copies *"the shape of Descartes'"*. A
catalogue plus closed composites is those two comments mechanised, with the copy replaced by a
reference that cannot drift.

---

## 3. What `list_workflows` must report

The operator's words are *"a name, set of technologies, etc"*. In typed terms, extending the
existing `WorkflowSummary` (`crates/willikins-server/src/startup.rs`), which today carries `name`,
`document_description` and `inputs`:

| Field | Type | Where it comes from |
| --- | --- | --- |
| `name` | `WorkflowName` | the filename stem; the natural key (exists) |
| `document_description` | `Option<Description>` | the document, **as quoted data** — trust boundary 4 (exists) |
| `inputs` | `Vec<InputSummary>` | declared inputs: name, `TypeRef`, required (exists); add each input's own `description` |
| `document_sha256` | `DocumentSha256` | already computed in `Loaded`; the version key everything else joins on |
| `providers` | `BTreeSet<ProviderId>` | **computed**: the first segment of every non-pure step's `ToolName`, so `naming.v1` and `template.render` are not reported as technologies |
| `tools` | `BTreeSet<ToolName>` | **computed**: every step's tool, transitively through `uses:` |
| `class` | `Class` | `Checked.class`, already computed at scan time |
| `requires_approval` | `bool` | `class.requires_approval()` — "this one will need you" |
| `mints_secrets` | `bool` | **computed** from `Checked.types` against the registry's per-type `secret` flag (`willikins_types::lib.rs:118`) |
| `composes` | `Vec<(WorkflowName, DocumentSha256)>` | for a composite: its currently resolved children |
| `provenance` | `Provenance` | from the **journal**, never the file: operator-authored, or proposed-by/promoted-by (§1.7) |

Three decisions inside that table are worth stating on their own.

**"Technologies" is computed, never asserted.** A hand-written `technologies:` field would drift —
the argument in `todos/2026-09-20-remote-mcp-from-a-phone.md`, *"a document that gains a Buildkite
step starts reporting Buildkite the moment it does, without anyone remembering to update a tag"* —
and, worse, it would be a self-label an agent-authored document could lie with (§1.7). A new type
`ProviderId` (the first segment of a `ToolName`) is what the operator means by a technology, and it
is free: `scan_directory` already parses every document and holds every step's tool name.

**`class`, `requires_approval` and `mints_secrets` are discovery fields, not decoration.** An agent
choosing on a phone, with no context about the deployment, needs to know before it starts whether
the run will stop and wait for a human, and whether anything secret moves. The fixture's header
already brags that *"nothing in a run of this document is ever redacted, because nothing in it is
secret"*; this is that sentence made machine-readable.

**No taxonomy field in v1.** "Standalone repo vs monorepo directory" is a real distinction the
survey documents (§4.2) and the graph only partly implies — a document with no `github.repo.ensure`
step is probably a monorepo document, which is a heuristic, not a fact. If a taxonomy is wanted
later it must be **advisory only**: displayed, never read by `check`, never an input to a trust or
tier decision. Nine documents do not need one, and the free-text `description` already carries it.

---

## 4. The surface

### 4.1 MCP

Three additions to the existing eight tools (`validate`, `describe`, `plan`, `apply`,
`run_status`, `list_workflows`, `list_tools`, `propose_slug`), and one deliberate omission.

- **`list_workflows`** — unchanged name, the fields of §3. No new tool.
- **`propose_workflow { document, name }`** → `{ proposal_id, name, document_sha256, tier,
  summary, status: "pending" }`, or the `check` errors on refusal. Runs `check`, refuses a name
  already in the trusted source, stores nothing that would not pass startup's scan, calls no
  provider. `summary` is the §3 fields computed over the draft, so the agent can show the human
  what it is asking for in the same vocabulary the catalogue uses.
- **`get_proposal { proposal_id }`** → status, summary, and the document text, so an agent can
  iterate on its own draft across sessions.
- **`list_proposals`** → the pending set, for an agent polling for a decision, exactly as it polls
  `run_status`.

**There is no `approve_proposal` MCP tool**, for the same reason the server's own instructions
already say *"There is no `approve` or `reject` tool: a pending plan is decided by a human,
elsewhere."* The MCP surface is the agent's surface; approval is not the agent's to make. Promotion
happens through the adapter (§1.4) — for the default `manual` adapter, by the operator placing the
file.

The server instructions gain one sentence: a proposal is a request, not a workflow; it cannot be
planned or applied until it exists in the trusted source, and `list_workflows` is the only thing
that says what is runnable here.

### 4.2 CLI

- `willikins workflows` — the §3 listing, human table or `--json`. Today this is only reachable by
  running a server.
- `willikins propose <file>` — check, summarise, store a proposal. Prints the tier, the computed
  providers and tools, the class, and where the operator should put the file for the configured
  adapter.
- `willikins proposals [--show <id>]` — list, inspect, and (for the `manual` adapter) print the
  document so the operator can place it.
- Locally the CLI keeps taking a path for `plan`/`apply` and that does not change: trust boundary 3
  says whoever runs the CLI already holds the machine that holds the credentials, so a path
  restriction there buys nothing. **The proposal mechanism is for the remote deployment** — the
  phone case, where the agent has no machine and no context.

---

## 5. When a convention changes

The design doc's phrase is *"the second run is the feature"*, about templates. For a stored
workflow it splits into three separate mechanisms, only one of which exists today.

**Mid-flight: already handled.** A document that changes between `plan` and `apply` is caught —
the plan carries its `document_sha256`, `apply` reloads and compares, and refuses with
`DocumentChanged`. Nothing to add.

**The stored document itself: it is a file, so it changes like code.** The operator edits it in
their trusted source; startup's scan re-checks it and re-records its hash. A composite over it
picks up the change by name (§2.3). This is the convergence the catalogue wants: the tart-ci pin
and the 2026-09-14 move to a shared Doppler service account (survey §4.1) are exactly "one document
changed, and every project made from the old one is now behind".

**Across already-provisioned projects: the gap.** Idempotent-ensure converges provider resources on
a second run — re-running the new document against an existing project reconciles the repository,
the project, the configs, the pipeline. What it does **not** converge is (a) files, because no tool
can write one yet (survey §6 Rank 1), and (b) removals, because ensure says nothing about deletion
and should not (survey §6, "Decommissioning"). So "re-run the new document everywhere" is a real
and partial answer, and saying which half it covers matters more than the mechanism.

What makes it usable is the **project record** the design doc already schedules for milestone 3.
It must carry, per provisioned project: the workflow name, the `document_sha256` it was provisioned
from, the naming scheme version, the inputs, and the overrides. With that, "which projects are
behind this document" is a query rather than archaeology, and the survey's drift findings become a
report. Without it, a convention change is what it is in the operator's estate today: a thing you
discover six months later in one repository and fix on one lane and not its sibling.

---

## 6. Open questions

1. **Does the `manual` adapter need an explicit "I placed it" call**, or is sha-match-on-scan
   enough? Sha match is elegant and needs no new verb, but it silently fails to close a proposal
   the operator edited before placing. Proposed: sha match only, and a proposal simply expires
   unmatched, because the alternative is a button that asserts something willikins can already see.
2. **Where does the proposal store live?** The journal is append-only and already the audit
   surface, which fits provenance; a proposal's *body* is bulkier than any event the journal
   carries today. Measure before deciding.
3. **The reviewer's preview plan (§1.2).** The right feature, deferred past 2c. When it lands it
   must be an approver-session action, producing no `plan_id`, and the reconnaissance question
   (a plan of an untrusted draft is an enumeration oracle over the operator's providers) has to be
   answered on purpose rather than by omission.
4. **Per-type literal policy for closed composites.** Whether `Description`-typed literals may
   appear in a Tier 1 composite without demoting it to Tier 2. Needs the registry to expose
   "inert" as a property, which is a second axis beside `secret`.
5. **Does a composite need its own `read`?** The design doc flags it: *"it needs typed composite
   output ports and a `read` semantic over a sub-graph"*. Whether a composite's `read` is the
   conjunction of its children's is 2b's question, not this one's.
6. **Attenuation.** Out of scope here beyond recording provenance. Converges with
   `todos/2026-09-16-unattended-agents-token-longevity.md`; revisit when one of the two is picked
   up.

---

## 7. Sequencing

This design is **not** the next thing to build, and it would be a mistake to read it as a claim
that it is. The survey's §8 is blunt: most of what the operator is asking for is files, and no tool
can put a file in a repository. A library of nine documents whose every entry is mostly scaffolding
is worth little until that lands.

What is worth doing before then, in order, because each is cheap and each is a prerequisite for
something else:

1. **§3's computed fields on `list_workflows`.** No trust implications, no new storage, and the
   "self-describing workflows" half of `todos/2026-09-20-remote-mcp-from-a-phone.md`. A day.
2. **`document_sha256` on the summary, and the project record's shape.** It is the key everything
   in §5 joins on, and retrofitting a key is worse than adding one.
3. **2b, with `uses:`** — already scheduled, now with the syntax and the three checks it needs.
4. **Proposals (§1)** — after 2c, because it is the remote deployment's feature and 2c is what
   makes a remote deployment safe. The operator said twice that agent authorship is a future
   problem; this design exists so that when it arrives, nobody re-derives §1.2 under time pressure.
