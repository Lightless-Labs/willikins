# Milestone 2b: composition, a document uses another document

**Created:** 2026-10-05 (the composition lane of the operator's "what's next? We can explore multiple lanes in
parallel")
**Reviewed:** 2026-10-05 (portfolio review of the five plans of 2026-10-05: 3k, 2b, 3l, 3m, 3n)
**Addendum:** 2026-10-05 — portfolio review. Order: 3k, then 3n F1–F3, then 3l, then this milestone, then 3n's S and
G parts, then 3m. Fixes, each so a task is green alone: `crates/willikins-cli/src/render.rs`'s `check_error_detail`
matches `CheckError` exhaustively, so P2, L2 and C1 add their variants' render arms in the same commit and gate
`willikins-cli`'s clippy; P1's patterns and R1's `InputError::NotSettable` are published by the server's `mcp_server`
snapshot, so those tasks regenerate it; J1's new `PlanRecorded` field has construction sites in `willikins-server`,
so J1 fills them. A general snapshot rule joins the Gates section, and acceptance 1's "dotted path" now says a
`/`-separated path. After 3k, new integration tests live under `tests/it/` (Gates).

**Addendum:** 2026-10-06 (task L2, commit 2) — the SHARED VALUES table names `UsedDocument`'s third field `message`.
Implemented as `reason` instead: `willikins-core/src/check.rs`'s own test
`every_check_error_variant_serializes_with_its_kind` enforces, for every existing `CheckError` variant, that none
has a field literally named `message` (it would collide with `crate::Reported`'s own added field when a renderer
wraps the error). `message` was never free to reuse here; the content and the variant name are otherwise exactly as
specified.

**Addendum:** 2026-10-06 (task C1) — three readings and one narrowing, recorded since the SHARED VALUES line
"Plus the existing signature variants, reported with the node path" does not spell any of them out. (1) "Reported
with the node path" means exactly: `SecretWorkflowInput`, `UnregisteredInputType`, `DisallowedInputType`,
`AcknowledgementDefault`, and `DefaultTypeMismatch` need no new field at all, because each already carries an
`input: InputName`, and `InputName` already accepts the `/`-separated form (decision (d3)); `check` simply passes
`<uses step>/<child input>` (`crate::compose::prefixed_input`, widened to `pub(crate)` for this) as that field
where it would otherwise pass a root input's own authored name. (2) This rerun covers only a *bound* boundary
(`Boundary::binding.is_some()`): an unbound one is already a fixed input of the flat `workflow.inputs` under this
same combined name (decision (d6)), and `check_workflow_inputs` already walks every one of those — rerunning the
same five rules over its `Boundary` entry too would report the identical declaration twice. (3) `check_input_spec`
(the five rules, extracted from `check_workflow_inputs` so both callers share one body) returns whether it pushed
an error, and `Resolver::check_boundaries` skips the exact-type check for that boundary when it did — the same
cascade-suppression the module docs already describe for the default check, now applied across the five rules as
a group rather than only within one of them. **Narrowing:** `check_boundaries` does not type-check a `Binding::List`
boundary binding at all (recorded in check.rs's own "Known gaps"): every list element is copied unchanged by
substitution and so reaches a real node port downstream with a real expected type, unless the child never
references that input anywhere, a pass-through case no acceptance test here exercises. Also: the
`AppleBundleIdentifier => TemplateValue` conversion-blocked case (acceptance 5) is a hand-built `check.rs` unit
test (`a_boundary_binding_that_only_converts_to_the_declared_type_still_mismatches`, with a trivial
`fake.identifier.get` added to `check.rs`'s own `test_catalog`), not a `workflows/fixtures/composition/` DSL
fixture: every real tool that outputs `AppleBundleIdentifier` in `willikins-providers-fake` (`appstore.app.get` and
siblings) requires a full App Store Connect credential chain (`issuer_id`/`key_id`/`key`), which would make the
fixture about credential resolution rather than the boundary rule it exists to pin.

**Addendum:** 2026-10-06 (task J1) — `PlanRecorded.used: BTreeMap<WorkflowName, DocumentSha256>` (SHARED VALUES)
needs `WorkflowName: Ord` for the map key, and `#[derive(DomainType)]` emits `Clone`/`PartialEq`/`Eq`/`Debug` only
(deliberately: the macro must not hand `Ord` to a secret storage, where even a constant-time comparison would leak
length through timing). Resolved with a one-commit prerequisite, `willikins-types/src/workflow_name.rs`: a hand-written
`PartialOrd`/`Ord` on `WorkflowName` itself (never secret), delegating to the same field `PartialEq` already compares.
Committed alone, ahead of J1's own commit, with its own test and gates (`cargo check -p willikins-types` joins J1's
scoped gates for this reason). `PlanRecorded.used` itself is filled empty at every construction site this task
touches (`willikins-server/src/butler.rs`'s `plan_inner`, and every `willikins-journal` test that builds a
`PlanRecorded`); `PlanRecord` (the replay-view struct, distinct from the `Event` variant) does not gain a `used`
field here — S2/S3 fold it once `apply`'s drift check has a closure to compare against.

**Addendum:** 2026-10-06 (task R1, commit 1) — the SHARED VALUES line "`InputError::NotSettable { input }` (the
exact shape follows `InputError`'s existing variants)" cannot be taken literally: `InputError` has no existing
variants today, because it is a plain struct (`{ input, error: ParseError }`), not a `#[serde(tag = "kind")]` enum
— `willikins-server/src/error.rs`'s own pinned test and doc comment say so explicitly. Converting it to a real enum
would not be "additions only" (the published schema goes from one object shape to a tagged union) and would break a
crate outside this task's scope: `willikins-cli::render::describe_text` reads `.input`/`.error` as struct fields
directly, never through `Display`, and is not in R1's gates. Implemented instead as `InputError::not_settable`, an
associated function building the *existing* struct shape with a `ParseError` whose `type_name` is the literal
`"NotSettable"` — the same device `describe`'s own undeclared-name branch already uses (`ParseError::new("Workflow",
...)`), just with its own discriminable `type_name`. This keeps `InputError`'s wire shape, and
`willikins-server/src/error.rs`'s pinned test, byte-for-byte unchanged: regenerating `input_error_schema_generates`
and `description_schema_generates` produced no diff at all beyond this task's own doc-comment wording (confirmed by
running both), and the `mcp_server` snapshot's only diff is catching up task C1's `UsesInputTypeMismatch`, which
(per the 2026-10-05 portfolio addendum above) was never C1's job to regenerate — R1's gate is the first one after it
to touch that snapshot. A caller discriminates `NotSettable` by `error.error.type_name == "NotSettable"`, not by a
`kind` tag.

**Addendum:** 2026-10-06 (task S1) — four points not spelled out by the task row. (1) **`catalog.rs` gets no
functional change.** `live_catalog_for_document`'s own credential computation is never called from anywhere inside
`willikins-server` itself — its only caller is `willikins-cli/src/commands.rs` (task K1), outside S1's gates — so
there is no server-internal call site for S1 to hand the linked graph to. S1's actual contribution is the contract,
stated on that function's own doc comment, plus a pinning test pair (`catalog.rs`'s own
`an_unlinked_composite_hides_a_childs_node_from_the_credential_scan` /
`the_linked_graph_surfaces_the_childs_node_under_its_renamed_path`) proving the unlinked root hides a used document's
own node from the scan while the linked graph surfaces it under its renamed path — the exact failure K1 must avoid.
(2) **`startup::Loaded.workflow` stays the authored, unlinked document**; only `Loaded.checked` is built from the
*linked* graph (`check` refuses outright on an unlinked `uses:` step). This is deliberate, not an oversight:
`WorkflowSummary::from(Loaded)` already reads only `loaded.workflow.inputs`/`.uses`, which are therefore already a
composite's own authored surface, never a used document's fixed inputs or renamed nodes — task S3 should filter
`WorkflowSummary` from `loaded.workflow`, not from `loaded.checked.workflow`. (3) **`Butler::validate` folds a
linking failure into a normal `ok: false` response**, exactly as it already folds a `check` failure (both are
`Vec<CheckError>` — `link`'s errors already are `CheckError`s); `Butler::describe` instead wraps the identical list
as `ButlerError::Check`, exactly as it already does for its own `check` call. Each method keeps its own pre-existing
success/error split; linking adds no new one. (4) **`Butler::describe` filters `Description::resolved`** to drop
every fixed input before returning (ties to R1; acceptance 7's "the MCP `describe` response ... do not show it").
`willikins_core::Description` itself is untouched — core is outside S1's gates, and changing it would regenerate the
server's `mcp_server` snapshot, which is not this task's to touch.

## Goal

1. **Part P (the document).** A step may say `uses: <workflow-name>` instead of `tool: <tool-name>`. It binds the
   used document's declared inputs under `with:` and exposes the used document's declared outputs as
   `${{ steps.<step>.<output> }}`, exactly as a tool node exposes its ports.
2. **Part L (the linker).** Before `check`, a composite is linked: every `uses:` step is replaced by the used
   document's nodes, renamed `<step>/<node>`, with the caller's bindings substituted for the used document's inputs.
   `check`, `plan`, `apply`, the journal and the approvals page then see one flat graph, and treat it the way they
   already treat any other graph.
3. **Part S (trust, identity, surfaces).** A used document is found by name, in the same trusted source as its
   parent. A plan records the content hash of every document it was built from, and `apply` refuses the plan if
   any of them changed. The server, the CLI and the MCP surface all link before they check.
4. **Part O (the operator's document).** The operator's iOS app document (gitignored, under `private/workflows/`)
   splits in two. An organisation document holds what the organisation owns: the shared base configs, the GitHub org
   and monorepo reference, the Buildkite org and cluster name, the CI service account, and where each credential
   lives. The app document `uses:` it. The split is proven converged twice: a private test shows the split plan
   equals the monolith's plan apart from node names, and a real `plan --live` still reads 19 NoOp, 62 Compute and
   nothing else.

Stage 2 is the end state: a reusable iOS-app document that any app in the organisation can `use`. It is described
below (decision (o2)) but not built here. It needs a primitive this milestone does not add, and decision (o2) names
that primitive.

The real names (the organisation, its accounts, the app, its identifiers, the shared configs, the secret names) appear
only in the private documents, their fake state and their gitignored tests. Every tracked file uses placeholders:
`example-org`, `example-bk-org`, `com.example.app`, "the shared base configs", "the CI service account".

## What earlier documents already decided

Each of these items is cited, not decided again.

- **Design doc, "Workflow DSL":** *"Workflows are tools. Same typed interface, so a composite is a node in a larger
  graph. Primitives are Rust, composites are DSL, one abstraction."* Decision (d2) keeps the typed interface and
  replaces the execution model. A design-doc addendum (task D1) records why.
- **Design doc, "Milestone 2 decisions":** composition was split out of milestone 2 *"because it needs typed composite
  output ports and a `read` semantic for a whole sub-graph"*. Decision (d2) gives the outputs their types and
  removes the need for a sub-graph `read`.
- **`docs/research/2026-09-20-workflow-library-design.md` §2.2:** `uses: <WorkflowName>` is a `StepDecl` field and
  is mutually exclusive with `tool:`. A `ToolName` is dotted snake_case and a `WorkflowName` is kebab, so they
  cannot share one field. Because `StepDecl` is `deny_unknown_fields`, an older willikins refuses a composite with a
  located error instead of running it.
- **The same note, §2.3:** the composite's class is the maximum over its children. Cross-document cycles are refused
  at scan time, so a cycle refuses startup. Output port types come from the child's resolved output types, and
  carry their secrecy with them. Children are pinned by name, not by sha, and the shas are recorded at plan time.
- **The same note, §1.2 and §1.6:** `plan` and `apply` over the network take a trusted name, never a body. Promotion
  never pre-approves anything. Neither changes here.
- **Design doc, "Workflow inputs":** a signature has no secret input type, and inputs are never inferred from free
  variables. Decisions (d4) and (d6) are both consequences of these two rules.

## Out of scope

- **`for_each` on a `uses:` step.** It is refused (decision (d12)). Several apps in one organisation are several
  root documents, one per app, each using the same organisation document.
- **The reusable iOS-app document (stage 2).** Decision (o2) describes its shape and names the missing primitive. It
  gets its own plan.
- **Proposals, tiers, promotion** (research note §1). Composition is what they build on. They are not part of this
  milestone.
- **Pinning a child by sha in the document** (decision (d10)).
- **Plan identity covering inputs** (`todos/2026-09-14-plan-identity-must-cover-inputs.md`). It is related but
  separate. This milestone widens identity to the used documents only.
- **A check for two ensures on one natural key** (risk 1).

## Trust boundaries (normative)

1. **A used document is privileged content, found only by name in the trusted source.** `uses:` takes a
   `WorkflowName` (kebab grammar). It never takes a path, a URL, a git ref or a sha. Resolution goes through
   `load_named_document`'s rules: `<name>.yaml` then `<name>.yml`, in the parent's own directory, a symlink is
   refused, and the document's internal `name:` must match its file name.
2. **A body never supplies a child.** `validate` and `describe` accept a document body, which may name trusted
   children. Those children are always read from the trusted directory. `plan` and `apply` over the network still
   take a name only.
3. **No secret enters a used document.** Every input a used document declares is checked by the same rules a root
   signature is (no secret type, no unregistered type, no `TemplateSource`/`RepoFile`, no acknowledgement default).
   Every binding into one must match its declared type exactly.
4. **Only declared outputs cross outwards.** A parent can reach a child's value only through
   `${{ steps.<step>.<output> }}` for an output the child declares. The reference grammar cannot name a node inside a
   child.
5. **A plan is bound to every document it was built from.** If any used document's bytes, or the set of used
   documents, differ between plan and apply, the result is `DocumentChanged`. This is the same refusal a changed root
   gets today.
6. **Expansion is bounded.** Depth and the total number of linked nodes are capped before anything is checked. Each
   file has its own cap (`MAX_DOCUMENT_BYTES`), but that cap says nothing about the graph after expansion.

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Document keyword (P2) | `uses: <WorkflowName>` on `StepDecl`. Exactly one of `tool`, `uses` |
| `with:` under `uses:` | keys are the child's `InputName`s; values use the tool-step grammar (scalar reference or literal, or a list of them) |
| Path separator (P1) | `/` |
| `NodeName` / `InputName` pattern (P1) | `^[a-z][a-z0-9_]*(/[a-z][a-z0-9_]*)*$` (was `^[a-z][a-z0-9_]*$`). `PortName` and `OutputName` are unchanged |
| Authored names (P1) | one segment. The DSL refuses a `/` in a step key, an input name or a `with:` key under `uses:` |
| Linked node name | `<uses step>/<child node>`, applied recursively: `app/org/base_gate` |
| Fixed input name | `<uses step>/<child input>` |
| Core model (P2) | `Workflow.uses: IndexMap<NodeName, Uses>` with `#[serde(skip_serializing_if = "IndexMap::is_empty")]`, `pub struct Uses { pub workflow: WorkflowName, pub with: IndexMap<InputName, Binding>, pub position: usize }` (`position` = index in the document's `steps:` map) |
| Step order (L1) | walk indices `0..nodes.len() + uses.len()`: at index `i`, take the `uses` entry whose `position == i` if there is one, else the next tool node in `nodes` order. A used document's nodes are inserted there, in its own order |
| `InputSpec` field (L1) | `pub fixed_by: Option<NodeName>`, `None` for every authored input, `#[serde(skip_serializing_if = "Option::is_none")]` |
| Linker entry (L1) | `willikins_core::compose::link(root: &Workflow, resolve: &mut dyn FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure>) -> Result<Linked, Vec<CheckError>>` |
| `Linked` (L1) | `pub struct Linked { pub workflow: Workflow, pub used: Vec<WorkflowName> }`. `workflow.uses` is empty. `used` lists each distinct used name once, in first-resolution order |
| `ResolveFailure` (L1) | `NotFound`, `Refused` (symlink or name mismatch, deliberately not told apart), `Document { message: String }` |
| Boundary record (L1) | `Workflow.boundaries: Vec<Boundary>` with `#[serde(skip_serializing_if = "Vec::is_empty")]`, `pub struct Boundary { pub uses: NodeName, pub workflow: WorkflowName, pub input: InputName, pub spec: InputSpec, pub binding: Option<Binding> }`. Filled only by `link`. Both new `Workflow` fields skip when empty, so a document without `uses:` serializes byte-identically |
| `MAX_USES_DEPTH` (L2) | `8` (root = depth 0; a `uses:` at depth 8 is refused) |
| `MAX_LINKED_NODES` (L2) | `2048` tool nodes after expansion, counted before `for_each` |
| New `CheckError` variants (P2, L2, C1) | `Unlinked { node }`, `UnknownWorkflow { node, workflow }`, `UsedDocument { node, workflow, message }`, `UsesCycle { chain: Vec<WorkflowName> }`, `UsesTooDeep { chain: Vec<WorkflowName> }`, `UsesTooLarge { nodes: usize }`, `UnknownUsesInput { node, input }`, `UnboundUsesInput { node, input }`, `UnknownUsesOutput { site, node, output }`, `ItemInUses { node, input }`, `KeyedOnUses { site, node }`, `UsesOutputCycle { node, output }`, `PathInAuthoredName { name }`, `UsesInputTypeMismatch { node, input, expected, found }`. Plus the existing signature variants, reported with the node path: `node` names the `uses:` step |
| New `PlanError` variant (R1) | `InputNotSettable { input }` |
| New describe error (R1) | `InputError::NotSettable { input }` (the exact shape follows `InputError`'s existing variants) |
| Journal field (J1) | `PlanRecorded.used: BTreeMap<WorkflowName, DocumentSha256>`, `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]` |
| `WorkflowSummary` field (S3) | `uses: Vec<WorkflowName>` (direct children, declaration order), skipped when empty |
| Fixture directory (P2 on) | `workflows/fixtures/composition/`. It is not scanned by the existing characterization, which stays byte-identical |
| Public positive pair (F1) | `workflows/fixtures/composition/example-org.yaml` and `workflows/fixtures/composition/new-rust-service-in-org.yaml`, compared with `workflows/new-rust-service-buildkite.yaml` |
| Placeholders (O1, tracked text only) | organisation document `example-org`, GitHub org `Example-Org`, monorepo `Example-Org/monorepo`, Buildkite org `example-bk-org`, bundle prefix `com.example.app` |
| Convergence counts (O2) | 19 NoOp, 62 Compute, 0 Blocked, 0 Skip, 0 Create, 0 Update, 0 Replace |

## Decisions

### (d1) Syntax: `uses:` beside `tool:`

The syntax is the research note's (§2.2): `uses: new-rust-service` is a step field, exclusive with `tool:`. The DSL
also refuses three more things, each with a located `Semantic` error:

- **A step with both fields, or with neither.**
- **`for_each` on a `uses:` step.** See decision (d12).
- **A `uses:` value that does not parse as `WorkflowName`.**

The document schema publishes `tool` and `uses` as a `oneOf`, so an agent validating locally learns the rule before
it calls `validate`.

### (d2) Inclusion: flattened into one graph, never a sub-graph node

A `uses:` step is not a node at run time. The linker replaces it with the used document's own nodes. This
supersedes the design doc's milestone-list line "`Workflow` implements `Tool`". The reasons come from how `plan` works
today:

- **One `Observation` cannot describe a sub-graph.** One run of the operator's document has NoOp nodes, Compute
  nodes, and could have one Blocked gate whose dependents Skip. `Tool::read` returns one `Observation` and
  `PlannedNode` has one `Action`. A sub-graph node would hide from the approver exactly the plan they are asked to
  approve.
- **Gates block per node, along data edges** (milestone 3e, decision (j)). If a child were one node, an unmet gate
  inside it would either block the whole child or need a nested `BlockedGate` model. Every node that does not depend
  on the gate should still run, and flattening keeps that.
- **Pure nodes run during `plan`.** The "Credentials are ports, resolvers are nodes" addendum rests on this: a
  credential resolved by a pure node exists before the provider reads that depend on it. A sub-graph node would need
  a recursive `plan` that returns a nested `Plan`, with nested journal events and nested fingerprints.
- **Ordering is per edge.** A parent node that reads one child output waits for the node that produces it, not for
  the whole child.

**What stays one abstraction:** the signature. A `uses:` step is checked against the child's declared inputs and
outputs in the same way a tool node is checked against its `ToolSpec`. Each binding must be of the declared type,
must be bound unless it has a default, and must name a declared port, and the child's outputs are typed ports. The
graph the user approves is the graph that runs.

### (d3) Identity: `<step>/<node>`, everywhere a node is named

- `NodeName` and `InputName` widen from one snake_case segment to a `/`-separated path of them (SHARED VALUES). Every
  name a pre-2b journal holds is one segment, so every journal line replays unchanged. A new line with a path is
  unreadable by an older binary. That is acceptable, because a journal is only ever read forward.
- **Why `/` and not `.`:** `Site`'s text form is unambiguous because every identifier holds no `.`
  (`tests::no_two_site_forms_share_a_display_string`). With `.`, a `uses:` step named `workflow` holding a node
  `outputs` would make `Site::Port` print `workflow.outputs.x`, which is exactly what `Site::Output` prints. A `/`
  keeps `Site`'s proof intact without changing it. It also reads as what it is, a path.
- **No collision is possible.** Authored names are one segment (the DSL refuses a `/`). Every linked name is
  `<parent step>/<child name>`. Parent step names are unique, and child names are unique within the child. The linker
  also refuses a `/` in any name of any workflow it is given (`PathInAuthoredName`). That protects a hand-built
  `Workflow`, which never passes through the DSL.
- **References cannot reach inside.** The reference parser keeps `steps.<node>.<port>` with a one-segment node.
  `${{ steps.org.base_gate.config }}` is already a parse error (an extra segment) and stays one.
- **Plan, journal and approvals use the path as-is.** `PlannedNode.name`, `InstanceFingerprint.name`,
  `NodeStarted`/`NodeFinished.node`, `BlockedGate.node`/`holds_back` and `Replacing.node` all carry `org/base_gate`.
  `for_each` instance keys are unchanged. No new field is needed to tell the approver which document a node came
  from, because the path says so.

### (d4) Inputs across the boundary: substituted, exact type, checked as a signature

- **Substitution.** Inside the child, every `Binding::Input(x)` (in a `with:`, a `for_each`, a list element, or an
  output) is replaced by the parent's binding for `x`, already rewritten into the flat namespace. Parent
  `Step`/`Keyed` references get the parent's own prefix, an `Input` names the parent's own (possibly fixed) input,
  and a `Literal` or `List` is copied unchanged. A literal is then parsed against the inner port's type by the
  existing `check`. This is correct, because the boundary type equals the declared type (next bullet).
- **Exact type at the boundary, no conversion.** `check` resolves each `Boundary.binding`'s type in the same way it
  resolves a port binding's, and requires it to equal `Boundary.spec.ty` (`UsesInputTypeMismatch`). A conversion is
  not allowed here, because substitution moves the *unconverted* binding inside. A converted-at-the-boundary value
  would reach inner ports unconverted, and the inner edge would then probe its own conversion from the wrong source
  type. Exact match makes the child see the type it was written against. A literal is checked by parsing it against
  the declared type, and a list by checking each element, as a port's literal and list are checked.
- **The child's signature is checked again on every use.** `check` runs the root-input rules (`SecretInput`,
  `UnregisteredInputType`, `DisallowedInputType`, `AcknowledgementDefault`, `DefaultTypeMismatch`) over every
  `Boundary.spec`. Its errors name the `uses:` step and the child input. This matters because in CLI file mode a
  sibling document is never checked on its own. Without this rule, a child declaring a secret input that the parent
  binds to a secret step output would pass the exact-type match and carry a secret inside (trust boundary 3).
- **`${{ item }}` in a `uses:` step's `with:` is refused** (`ItemInUses`). After substitution it would silently
  rebind to the item of whatever `for_each` node inside the child consumes that input.
- **An unknown `with:` key is `UnknownUsesInput`.** A required child input with no binding and no default is
  `UnboundUsesInput`. The linker never hoists an unbound input into the root's signature, because inputs are not
  inferred from free variables.

### (d5) Secrecy across the boundary

- **Inwards: never.** This follows from (d4): no child input is secret-typed, and every binding into one matches
  exactly. Credentials a used document needs are resolved inside that document, by its own resolver nodes, from
  non-secret references (a `DopplerConfig` and a `SecretName`) that the parent may pass in.
- **Outwards: through a declared output only.** A child output's type is its binding's resolved type in the flat
  graph, which is what the child's own `Checked.output_types` would say, secrecy included. A parent may bind a secret
  child output to a secret-accepting port of its own tool node, and the existing sink check decides that on the flat
  edge. A root may declare it as an output, since outputs are not sinks and every render goes through `Value`.
  Nothing new can carry a secret into a template, a `RepoFile` or a non-secret port, because those are the existing
  rules applied to a flat edge.
- **Recommended, not enforced** (policy lives in the workflow): an organisation document exports *where* credentials
  live, never the credentials themselves. A reusable child cannot accept a secret anyway, so a secret output from an
  organisation document is only useful to the root's own tool nodes.

### (d6) Defaults: fixed inputs

The question: when a parent leaves a defaulted child input unbound, where does the default go? An organisation
document must be able to export its constants as `outputs: x: ${{ inputs.x }}` over defaulted inputs (only a literal
output is refused, `LiteralOutput`). Two existing rules rule out the easy answers:

- A default cannot be inlined as a literal, because a `for_each` source must be a reference (milestone 3g, decision
  (a)). The operator's document iterates over `${{ inputs.environments }}` and over the shared base config list.
- A default cannot be hoisted into the root's public signature, because inputs are not inferred.

**Decision:** an unbound defaulted child input becomes a **fixed input** of the flat workflow, named
`<step>/<input>`, with the child's `InputSpec` and `fixed_by: Some(<step>)`. Every child reference to it becomes
`Binding::Input(<step>/<input>)`. The existing default resolution, the `for_each`-over-input path and the default type
check all apply unchanged. A fixed input:

- is never in `describe`'s `missing`. It **is** in `Description.resolved`, the internal map: `Butler::plan` passes
  `describe(..).resolved` straight to `plan` (`butler.rs`, `plan_inner`), and `plan` resolves
  `Binding::Input(org/base_configs)` from that map. Leaving the fixed input out would make the first child node fail
  `MissingInput`. Only the agent-facing surfaces omit it: the MCP `describe` response, the CLI's `describe` output,
  `WorkflowSummary.inputs`, and any per-workflow input schema the server publishes;
- cannot be set by a caller: `describe` reports `NotSettable`, and `plan` refuses `InputNotSettable` as a backstop
  when the value it receives differs from the default;
- is in `PlanRecorded.inputs` (which is the same `resolved` map), so the journal shows which organisation constants
  a plan used;
- can be overridden only by the parent binding it in `with:`. That is the child's signature, as it would be for a
  caller running the child alone.

**Acknowledgements:** an `OperatorAcknowledgement` input can never carry a default (`AcknowledgementDefault`), so a
child gate's acknowledgement input is always bound by the parent, normally to the parent's own input. After
substitution, `BlockedGate.awaiting_inputs` (every `Binding::Input` among the gate's subject bindings) names the
root's input, which is the one the operator can actually supply. R1 pins this.

### (d7) Outputs across the boundary

- `${{ steps.<uses step>.<output> }}` resolves, at link time, to the child's output binding as rewritten into the
  flat namespace. That binding is an `Input` (a fixed or substituted input), a `Step`/`Keyed` onto a child node, or a
  `Literal` (which the parent may only bind to a port, since `LiteralOutput` refuses one at the root). An undeclared
  output is `UnknownUsesOutput`.
- **A keyed reference onto a `uses:` step is refused** (`KeyedOnUses`). There are no instances to key into.
- **An alias cycle is refused** (`UsesOutputCycle`). Two `uses:` steps may legitimately feed each other's inputs
  when the node-level graph has no cycle, and `check`'s cycle detection judges that on the flat graph. A loop made
  only of pass-throughs (`a.out = inputs.x`, `x ← b.out`, `b.out = inputs.y`, `y ← a.out`) contains no node, so the
  node-level check never sees it. The linker resolves aliases with a visiting set and refuses the loop.
- A child's outputs are not outputs of the flat workflow. Only the root's declared outputs are, and they may
  re-export a child's.

### (d8) Location and trust

- **Server:** children resolve in the trusted directory through `load_named_document`. `Butler::plan`,
  `Butler::apply`'s reload, `validate` and `describe` (body or name) all link through one resolver, which records
  each loaded document's `DocumentSha256`.
- **Startup:** `scan_directory` links every top-level document against the directory itself. A cycle, an unknown
  child or a boundary error refuses startup, naming the document. This is §2.3's "a cycle refuses startup".
- **CLI file mode** (`describe`, `plan`, `apply <file>`): children resolve in `<file>`'s own parent directory, with
  the same rules. Trust boundary 3 of milestone 2 holds: whoever runs the CLI already holds the machine.
- **`apply <file>`** copies exactly one file into its private temporary directory today. It now copies the linked
  closure (the root and every used document, each as `<name>.yaml`), so `Butler::start`'s scan of that directory
  links the same way.
- **`apply --plan-id --workflows-dir`** resolves in that trusted directory, like the server.

### (d9) Cycles and bounds

- The linker walks depth-first with a stack of workflow names. A name already on the stack is `UsesCycle { chain }`
  (a self-use included), with the chain from the first occurrence to the repeat.
- A `uses:` at depth `MAX_USES_DEPTH` is `UsesTooDeep`. More than `MAX_LINKED_NODES` tool nodes after expansion is
  `UsesTooLarge`. The check runs as nodes are added, so an exponential diamond is refused before it is materialised.
  The YAML alias-amplification refusal is the precedent (`docs/research/2026-09-12-e2e-adversarial-pass-2.md`).
- A diamond is allowed: the same child used by two steps is expanded twice, under two prefixes (risk 1).

### (d10) Versioning: by name, recorded per plan

- `uses: example-org` means whatever `example-org` is in the trusted source now (§2.3). The trusted source (a git
  checkout, reviewed by the operator's own process) is the version control.
- Every plan records `{name → sha}` for its whole closure (J1), and `apply` refuses any change (trust boundary 5).
- Startup checks every composite against its current children. A child's signature change that breaks a parent
  refuses startup loudly instead of failing at plan time.
- **No pin syntax in v1.** `uses: name@sha256:…` would only ever be able to refuse, because a trusted directory
  keeps no old versions to run. Plan-time recording plus a startup check already make any drift loud. If an operator
  wants an author-side pin, it is a refusal-only check that can be added later without changing the model.

### (d11) Plan, journal and approval

- Node paths (d3) are the identity in `Plan`, the fingerprint, `NodeStarted`/`NodeFinished`, `BlockedGate`,
  `Replacing`, the CLI's blocked report and the approvals page.
- `PlanRecorded.used` (J1) records the closure. The approvals page lists the used documents and their short shas
  under the plan's workflow name, so the approver sees which documents the plan was built from.
- `list_workflows` reports each composite's direct children (`uses`), which is §3's `composes`, names only. Shas are
  already in `ServerStarted.workflow_hashes`.
- Approval: `Checked.class` is the maximum over the flat graph's non-pure nodes, which is §2.3 with no extra code. A
  composite does not have a class of its own.

### (d12) `for_each` on a `uses:` step: refused in v1

An instance of an expanded child would need a compound identity (`apps[example]/app_id`). Keyed references would
need to reach into it, and gate tracking would have to key on both levels. Nothing the operator needs requires
this: one root per app, each using the organisation document, covers the multi-app case. Refused in the DSL (d1),
and refused again by the linker for a hand-built `Workflow` (`Uses` has no `for_each` field to carry it).

## Decisions, part O: the operator's document

### (o1) Stage 1 (this milestone): the organisation document is extracted

The private root keeps its file name and its document name, so its CLI invocation, its journal and its tests' file
names stay put. It gains one step, `org: { uses: example-org }`, and loses the nodes and literals that belong to the
organisation. Every other node keeps its current name. In placeholders:

```yaml
# private/workflows/example-org.yaml  (real name from the coordinator's brief)
name: example-org
description: What the organisation owns -- shared base configs, the monorepo, the Buildkite org and cluster, the CI
  service account, and where each credential lives.
inputs:
  github_org:         { type: GitHubOrg,             default: Example-Org }
  monorepo:           { type: GitHubRepo,            default: Example-Org/monorepo }
  buildkite_org:      { type: BuildkiteOrg,          default: example-bk-org }
  cluster_name:       { type: BuildkiteClusterName,  default: <the cluster's name> }
  ci_service_account: { type: <the port's type>,     default: <the CI service account> }
  base_configs:       { type: list<DopplerConfig>,   default: [<the shared base configs, in today's order>] }
  asc_config:         { type: DopplerConfig,         default: <where the App Store Connect key lives> }
  github_read_config: { type: DopplerConfig,         default: <...> }
  github_read_secret: { type: SecretName,            default: <...> }
  # ...one config/secret-name pair per credential the root resolves today
steps:
  base_gate:        { tool: doppler.config.inheritable.gate, for_each: ${{ inputs.base_configs }}, with: { config: ${{ item }} } }
  gh_token_secret:  { tool: doppler.secret.get, with: { config: ${{ inputs.github_read_config }}, name: ${{ inputs.github_read_secret }} } }
  gh_token:         { tool: github.token.parse, with: { value: ${{ steps.gh_token_secret.value }} } }
  monorepo_ref:     { tool: github.repo.get,    with: { repo: ${{ inputs.monorepo }}, token: ${{ steps.gh_token.value }} } }
outputs:
  github_org: ${{ inputs.github_org }}
  monorepo: ${{ steps.monorepo_ref.repo }}
  base_configs: ${{ steps.base_gate.config }}
  buildkite_org: ${{ inputs.buildkite_org }}
  # ...every other constant, passed through
```

The step names inside the private organisation document are the root's current names for the same nodes. The
sketch's `base_gate` stands for the real one. What moves:

- **Moved nodes** (all pure): the base-config gate, and the GitHub read-token pair together with the monorepo read
  that consumes it. Because the token never leaves the organisation document, no secret crosses (d5).
- **Moved literals:** every organisation fact the root wrote as a literal becomes a defaulted input of the
  organisation document. That covers the `naming.v1` org, the monorepo reference, the Buildkite org and cluster
  name, the CI service account, and each credential's config and secret name. The root binds
  `${{ steps.org.<output> }}` where the literal was. Each default's declared type must equal the type of the port
  the literal used to feed (verify item 4).
- **Stays in the root:** everything app-specific, together with every credential chain whose secret is consumed by
  an app node. That covers the App Store Connect chain, the GitHub write token for the scaffold, and the Buildkite
  token used by the cluster lookup, the pipeline and the bootstrap. A secret cannot cross into a used document, and
  moving only the cluster lookup would split the Buildkite chain across the boundary. Those chains keep their nodes
  and read their *locations* from `steps.org`.
- **The root's signature** loses `base_configs`, which becomes the organisation's fixed input `org/base_configs`.
  `environments` and the app identifiers stay. No real invocation ever passed `base_configs`.

The node set is the same set the monolith has. Four nodes are renamed to `org/…`, and the counts cannot move:
19 NoOp, 62 Compute.

### (o2) Stage 2 (the end state, its own plan): a reusable iOS-app document

```yaml
# private/workflows/<app>-ios-app.yaml becomes a two-step root
name: <app>-ios-app
inputs: { app_identifier, nse_identifier, widgets_identifier, certificate_type, serial_number }  # as today
steps:
  org: { uses: example-org }
  app:
    uses: ios-app
    with:
      slug: example-app
      app_identifier: ${{ inputs.app_identifier }}            # com.example.app
      # ...the other identifiers, the certificate selection
      github_org: ${{ steps.org.github_org }}
      monorepo: ${{ steps.org.monorepo }}
      base_configs: ${{ steps.org.base_configs }}
      buildkite_org: ${{ steps.org.buildkite_org }}
      cluster_name: ${{ steps.org.cluster_name }}
      ci_service_account: ${{ steps.org.ci_service_account }}
      asc_config: ${{ steps.org.asc_config }}                 # a location, never the key
      # ...
outputs: { app_id: ${{ steps.app.app_id }}, ... }
```

`ios-app` holds every node that is app-specific today, with each app literal turned into an input. Composition is
enough for everything in it except one thing, the **files**. The scaffold's `repo.file.render` steps take `path` as an
exact `RepoPath`, and every path today is a literal under the app's directory. No pure tool derives a `RepoPath` from
typed parts. Templates and repository files can never be inputs (`template-source-input.yaml`,
`repo-file-input.yaml`, milestone 3g decision (e)). So a reusable document cannot place one app's files under
`apps/<slug>/` until such a primitive exists. The primitive needs its own reviewed decision: a typed path derivation,
such as a `RepoPath`-under-directory join, or a `naming` row. Where an organisation keeps its apps is policy, and
policy lives in the workflow. D1 writes that follow-up as a todo. The app-specific display strings inside templates
already have a route: `values` (`TemplateValue`, with the registered `AppleBundleIdentifier => TemplateValue` row).

### (o3) Migration, and why it stays converged

- **Idempotence is by natural key, and there is no saved run state.** Renaming a node changes nothing a provider sees.
  A plan reads every resource again by its key. The journal is history, and the next run reads none of it as state.
- **Step 1 (O1, private, no commit):** keep a byte copy of today's monolith as a private baseline fixture. Write the
  organisation document, and rewrite the root as (o1) describes. The private test plans both against the same private
  fake state, then asserts that the multiset of `(tool, instance, action, inputs, outputs)` is equal once each node's
  `org/` prefix is removed. Inputs and outputs are compared rendered, with full disclosure. It also asserts the
  counts.
- **Step 2 (O2, the coordinator):** a read-only `plan --live` of the split root reads the convergence counts
  (SHARED VALUES). The operator is shown it. Only then is it applied: an all-NoOp apply calls no `ensure`, and records
  a run whose counts match run `01a108f9`'s.
- **Rollback:** the monolith baseline still plans, because a single document never needed composition. Reverting is
  putting the old file back.

## Acceptance tests

Part P:
1. **Paths** (P1). `NodeName`/`InputName` accept `org/x`, `a/b/c`, `x` and refuse `org/`, `/x`, `org//x`, `Org/x`,
   `org.x`, `org/x.y`, the empty string. `PortName`/`OutputName` still refuse `a/b`. A document with a step key, input
   name or `uses:` `with:` key containing `/` fails to load with a located `Semantic` error. A journal line whose
   node is a `/`-separated path (`org/base_gate`) round-trips. `pre_pass_2_replay.rs`, and every other replay test, still pass unchanged. The existing
   characterization snapshot is byte-identical.
2. **The keyword** (P2). `uses-and-tool.yaml`, `uses-neither.yaml`, `uses-for-each.yaml` and
   `uses-bad-name.yaml` each fail to load with one located error. A `Workflow` with a non-empty `uses` map that
   reaches `check` unlinked returns exactly `Unlinked { node }`. The document schema snapshot gains the `oneOf`, and
   nothing else changes.

Part L:
3. **Linking** (L1). For a two-level composite (root → middle → leaf, in-memory resolver), the linked `Workflow`'s
   JSON equals a hand-written flat `Workflow`. The comparison covers the prefixed nodes in `position` order,
   substituted bindings (`Input`, `Step`, `Keyed`, `Literal`, `List`), one fixed input with `fixed_by`, and a root
   output re-exporting a leaf output through the middle. `used` is `[middle, leaf]`. The flat graph's `check` gives
   the same `order` a hand-written flat document gives.
4. **Refusals** (L2). Each produces exactly one error, of the named variant, and none panics. A self-use and a
   two-document cycle give `UsesCycle` with the chain. A nine-deep chain gives `UsesTooDeep`. A diamond whose
   expansion exceeds 2048 nodes gives `UsesTooLarge`, raised before 2048 more nodes are allocated (in-memory resolver
   counting calls). The rest: an unknown child, a child that fails to parse (`UsedDocument`, naming the child),
   `UnknownUsesInput`, `UnboundUsesInput`, `UnknownUsesOutput`, `ItemInUses`, `KeyedOnUses`, `UsesOutputCycle`, and
   a hand-built workflow with a `/` in a node name (`PathInAuthoredName`). Every new variant serializes with its
   `kind` (`every_check_error_variant_serializes_with_its_kind`).
5. **The boundary** (C1). A parent binding a `GitHubRepo` step output to a child's `DopplerProject` input gives
   `UsesInputTypeMismatch`. So does a parent binding a type that only *converts* to the declared type (the
   `AppleBundleIdentifier => TemplateValue` row). Another case is a child declaring a secret input that the parent
   binds to a secret step output of the same type (`uses-secret-input.yaml` with its child). It gives `SecretInput`
   naming the `uses:` step and the input, and never reaches `plan`. A child declaring `TemplateSource` gives
   `DisallowedInputType`. A child's defaulted acknowledgement gives `AcknowledgementDefault`. A secret child output
   bound to a parent template input gives the existing `SecretToNonSecretSink` on the flat edge. A secret child
   output bound to a secret-accepting parent port checks clean.
6. **Class** (C1). A root of only pure nodes using a child with one `Irreversible` node has class `Irreversible` and
   requires approval.

Part R and J:
7. **Fixed inputs** (R1). `describe` of a composite never lists a fixed input in `missing`, and its `resolved` map
   holds the fixed input's default, so `plan` consumes it. The MCP `describe` response and the CLI's `describe`
   output do not show it (S1 and K1 each pin their surface). A caller value for `org/base_configs` gives
   `NotSettable` from `describe` and `InputNotSettable` from `plan`. The plan's resolved inputs, as recorded, contain
   the fixed input's default. A child gate whose acknowledgement input is bound
   to the root's input `m_done` plans `Blocked` with `awaiting_inputs == [m_done]`.
8. **Plan identity** (J1). `PlanRecorded` with an empty `used` serializes byte-identically to a pre-2b line, and
   pre-2b lines replay. A non-empty `used` round-trips.

Part S and K:
9. **Server** (S1, S2). A trusted directory holding a two-document cycle refuses startup, naming the chain. A
   symlinked child and a child whose internal name mismatches both refuse as `UnknownWorkflow`, without saying which.
   `validate` with a body that `uses:` a trusted child checks against the trusted file's content. `Butler::plan`
   records `used` with the child's sha. If the child is edited between plan and apply, the result is
   `DocumentChanged`. The same happens if the root is edited to add a second child, or if a child changes from one
   used document to another under the same root bytes.
10. **Surfaces** (S3). `list_workflows` reports `uses` for a composite and omits it otherwise. The approvals page for
    a composite's pending plan lists each used document's name and short sha, escaped.
11. **CLI** (K1). `describe`, `plan` and `apply <file>` on `workflows/fixtures/composition/new-rust-service-in-org.yaml`
    link its sibling. `apply <file>` with `--fake-state` succeeds, which shows the closure was copied. A composite
    whose child lives only in another directory fails with `UnknownWorkflow`. `apply --live`'s credential
    requirement is computed over the linked graph (a root whose only Buildkite node is inside the child still
    requires the Buildkite token).

Part F and O:
12. **The public equivalence** (F1). Under one fake state, the plan of `new-rust-service-in-org.yaml` (inputs `slug`,
    `visibility`) equals the plan of `new-rust-service-buildkite.yaml` (inputs `slug`, `org=Example-Org`,
    `buildkite_org=example-bk-org`, `visibility`) as a multiset of `(tool, instance, action, inputs, outputs)`, with
    the `org/` prefix stripped. `apply` with `--fake-state-out`, then `plan` again, reads every non-pure node NoOp. A
    composition characterization snapshot covers every document under `workflows/fixtures/composition/`.
13. **The operator's split** (O1, private test). The same equivalence, between the private baseline monolith and the
    split root, with counts 19 NoOp and 62 Compute against the private fake state.
14. **Live** (O2, the coordinator). `plan --live` of the split root reads the convergence counts, and the all-NoOp
    apply finishes `Succeeded` with them. The counts are recorded in HANDOFF without a name.
15. **Adversarial pass** (X1), recorded under `docs/research/`. Each bypass becomes a fixture under
    `workflows/fixtures/composition/` plus a test.

Every negative fixture carries a header comment naming its acceptance test and the exact error it must produce.

## Verify before relying on them

1. **`serde` with two `Option<String>` fields and `deny_unknown_fields`** gives a located error for "both" and
   "neither" through `document_to_workflow`'s `Semantic` path, not a generic `serde` message. P2's test settles it.
2. **Whether `describe` or `plan` already refuses an undeclared caller input name.** `check`'s `UndeclaredInput` is
   about document bindings. R1 first pins the current behaviour for an undeclared name, then adds `NotSettable` beside
   it.
3. **A `Binding::Step` onto a `for_each` node is accepted as a workflow output** and resolves to the aggregated list,
   and a list-typed `Binding::Input` output resolves its default at plan time. Both are needed by the organisation
   document's `base_configs` and `environments` pass-throughs. L1's tests settle it on the flat graph.
4. **Each moved literal's port type equals the type its new organisation input declares** (o1). The private O1 test
   proves it by checking clean. If a port accepted the literal only through a conversion row, that constant's input
   declares the port's type, because the boundary does not convert.
5. **`Checked.order` breaks ties by declaration index** (milestone 1). L1 inserts child nodes at the `uses:` step's
   `position`, so a composite written in dependency order keeps its order.
6. **Node names never appear in an approvals-page URL or a form field name.** Only in escaped text. S3 settles it,
   because `/` would need encoding there.
7. **`willikins-server/src/catalog.rs`'s credential computation** reads `document.nodes`. S1 and K1 must hand it
   the linked graph.
8. **`scan_directory` reads the top level only** (`read_dir`, no recursion), so `workflows/fixtures/composition/` is
   never scanned as part of the trusted directory.
9. **Gate subject validation (`Catalog::validate_gate`) is per tool spec, not per workflow,** so substitution does
   not affect it. R1's gate test covers the plan side.

## Gates

Per task, scoped, never the full workspace gate (the coordinator runs that). Before **each** cargo command, wait for 3
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second,
then start the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`. Run fmt, clippy and tests as separate
commands, in the background with a 600,000 ms timeout, and read the output file's body, never piped through `tail` or
`tee`. A linker "missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild.
Never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <crate> [-p <crate>...] --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <crate> [-p <crate>...] -j 2 --no-fail-fast
cargo check -p willikins-types -j 2        # whenever willikins-types changed
```

**Every task from P1 on also runs `-p willikins-dsl`'s `acceptance` target.** Its existing characterization snapshot
must stay byte-identical. That is the evidence that linking changes nothing for a document without `uses:`. A
snapshot update to it is a defect, not a review item.

**Published shapes (portfolio review).** Any commit that changes a type published by a `schema_generation` snapshot
(core, journal) or by the server's `mcp_server__the_tool_list_and_every_schema_is_snapshotted` snapshot regenerates
that snapshot in the same commit, reviewed as pattern lines or additions only, and runs the owning crate's test. Any
commit that adds a `CheckError` variant adds its arm to `crates/willikins-cli/src/render.rs`'s `check_error_detail`
(an exhaustive match) and runs `cargo clippy -p willikins-cli --all-targets -j 2 -- -D warnings`.

**After milestone 3k.** If 3k has landed (the portfolio order puts it first), a new integration test is
`crates/<c>/tests/it/<stem>.rs` plus a `mod <stem>;` line in `tests/it/main.rs`, never a top-level file (3k's layout
guard refuses one); a `--test <stem>` gate becomes `--test it <stem>::`; the dsl characterization snapshot is
`crates/willikins-dsl/tests/it/snapshots/it__acceptance__characterization_of_every_document.snap`, and a new
composition snapshot lands under `tests/it/snapshots/`. Paths below name files by their pre-3k location.

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`. Local hooks refuse any commit
or message naming the operator's private setup: never `--no-verify`; rewrite with placeholders.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| P1 | **Node and input paths** (decision (d3); acceptance 1). Commit 1, `willikins-core` `workflow.rs`: the widened pattern for `NodeName`/`InputName` and their tests. Check `Site`'s `no_two_site_forms_share_a_display_string` still holds with a `/` path, and add a path case to it. Commit 2, `willikins-dsl`: refuse `/` in authored step keys and input names, with a located error, tests, and the journal round-trip test in `willikins-journal`. Snapshots that publish the patterns are regenerated, and the diff is reviewed: pattern lines only. They include the server's `mcp_server` snapshot, which publishes `NodeName` and `InputName` (commit 1 regenerates it). Scoped: `-p willikins-core -p willikins-dsl -p willikins-journal`, plus `-p willikins-server --test mcp_server` | sonnet implements |
| P2 | **`uses:` in the document and the model** (decisions (d1), (d12); acceptance 2). Commit 1, `willikins-core`: `Uses`, `Workflow.uses`, `check`'s `Unlinked` (first thing `check` does), the variant's serialization test, and its arm in `crates/willikins-cli/src/render.rs`'s `check_error_detail` (exhaustive). Commit 2, `willikins-dsl`: `StepDecl.tool`/`uses` as `Option`, exactly-one, no `for_each`, `WorkflowName` parse, `with:` keys parsed as `InputName`, the schema's `oneOf`, the four negative fixtures under `workflows/fixtures/composition/` with headers and tests. Scoped: `-p willikins-core -p willikins-dsl`, plus `cargo clippy -p willikins-cli --all-targets` | sonnet implements |
| L1 | **The linker, structure** (decisions (d2), (d4) substitution, (d6), (d7) aliasing; acceptance 3; verify items 3, 5). One commit: `crates/willikins-core/src/compose.rs` (`link`, `Linked`, `ResolveFailure`, `Boundary`, `InputSpec.fixed_by`), exported from `lib.rs`. Unit tests with an in-memory resolver. No refusal beyond what the happy path needs. Scoped: `-p willikins-core`, then `-p willikins-dsl` acceptance | sonnet implements, opus attacks |
| L2 | **The linker, refusals** (decisions (d4), (d7), (d9); acceptance 4). Commit 1: cycle, depth and size bounds, with the counting-resolver test. Commit 2: the boundary and alias refusals and `PathInAuthoredName`. When the reference sits in another `uses:` step's `with:`, `UnknownUsesOutput`'s and `KeyedOnUses`'s `Site::Port` names that `uses:` step as `node` and the child input as `port`. The grammars coincide, so no new `Site` variant is added for it. Each refusal that a document can express gets a fixture under `workflows/fixtures/composition/` (children beside it) and a test in `crates/willikins-dsl/tests/composition.rs`, which links with a directory resolver over that folder. Each new `CheckError` variant gets its `render.rs` arm in the commit that adds it. Scoped: `-p willikins-core -p willikins-dsl`, plus `cargo clippy -p willikins-cli --all-targets` | sonnet implements, opus attacks |
| C1 | **`check` at the boundary** (decisions (d4), (d5); acceptance 5, 6). One commit, `check.rs`: the signature rules over `Workflow.boundaries`, `UsesInputTypeMismatch` (exact, no probe), the fixtures for acceptance 5, the class test, and `UsesInputTypeMismatch`'s `render.rs` arm. Scoped: `-p willikins-core -p willikins-dsl`, plus `cargo clippy -p willikins-cli --all-targets` | sonnet implements, opus attacks |
| R1 | **Fixed inputs are not settable** (decision (d6); acceptance 7; verify items 2, 9). Commit 1, `describe.rs`: pin today's undeclared-name behaviour, then add hiding and `NotSettable`. Commit 2, `plan.rs`: `InputNotSettable`, and the acknowledgement gate's `awaiting_inputs` test through a linked composite. `InputError` is published by core's `description`/`input_error` schema snapshots and by the server's `mcp_server` snapshot: commit 1 regenerates all three (additions only). Scoped: `-p willikins-core`, plus `-p willikins-server --test mcp_server` | sonnet implements |
| J1 | **Plan identity covers the used documents** (decision (d10); acceptance 8). One commit, `willikins-journal` `event.rs`: the field, the byte-identical pre-2b replay test (the `principal` field's pattern), round-trip. `Event::PlanRecorded` is also constructed in `crates/willikins-server/src/butler.rs` (`plan_inner`; server tests only pattern-match it with `..`): the same commit fills `used` with an empty map there (S2 fills it for real). Journal schema snapshots that publish the event or `PlanRecord` are regenerated (additions only). Scoped: `-p willikins-journal`, plus `cargo clippy -p willikins-server --all-targets` | sonnet implements |
| S1 | **Server: resolver, startup, validate and describe** (decision (d8); acceptance 9 first half; verify items 7, 8). One commit: a trusted-directory resolver in `document.rs` that records shas; `scan_directory` links every document; `Butler::validate`/`describe` link (a body's children come from the trusted directory), and the MCP `describe` response omits fixed inputs; `catalog.rs` credential computation over the linked graph. Scoped: `-p willikins-server` | sonnet implements, opus attacks |
| S2 | **Server: plan and apply identity** (trust boundary 5; acceptance 9 second half). One commit: `Butler::plan` records `used`; `reload_and_check` re-links and compares the root sha and the `used` map; tests for each `DocumentChanged` case. Scoped: `-p willikins-server` | sonnet implements, opus attacks |
| S3 | **Surfaces** (decision (d11); acceptance 10; verify item 6). One commit: `WorkflowSummary.uses`, `WorkflowSummary.inputs` excluding every input with `fixed_by.is_some()`, the approvals page's used-documents list, the MCP `list_workflows` snapshot. Scoped: `-p willikins-server` | sonnet implements |
| K1 | **CLI** (decision (d8); acceptance 11). Commit 1, `main.rs`: `describe`/`plan` link siblings of `<file>`, and `describe`'s output omits fixed inputs. Commit 2, `commands.rs`: `apply <file>` copies the closure, `--live` credentials over the linked graph, `--plan-id` links in `--workflows-dir`. `render.rs` prints paths unchanged; check the blocked report shows `org/…`. Scoped: `-p willikins-cli` (the composition and render tests only, then the whole crate's tests once) | sonnet implements |
| F1 | **The public pair and its equivalence** (acceptance 12). Commit 1: `example-org.yaml` (defaulted `github_org`, `buildkite_org`, `cluster`, `environments`; the cluster lookup; pass-through outputs) and `new-rust-service-in-org.yaml`, with headers. Commit 2: `crates/willikins-cli/tests/composition_equivalence.rs` (the multiset comparison, then apply and re-plan NoOp), and the composition characterization snapshot in `willikins-dsl`. Scoped: `-p willikins-cli -p willikins-dsl` | sonnet implements |
| X1 | **Adversarial pass**, recorded under `docs/research/2026-10-0x-m2b-adversarial-pass.md`, placeholders only. Every bypass becomes a fixture plus a test. At least four mutations restored from saved copies (`cmp` for byte identity). Priority targets: a secret reaching inside a used document by any route; a parent reaching a child's internal node; a body supplying or shadowing a child's content; a child resolved outside the trusted directory (symlink, `..`, a name that is not a `WorkflowName`, `.yml` beside `.yaml`); an `item` or keyed binding rebinding inside a child; a caller setting a fixed input through the CLI, MCP `plan` or `describe`; a child changed between plan and apply without `DocumentChanged`; an expansion bomb that allocates before refusal; a name collision between a linked path and an authored name; a `Site` display collision; class or approval lowered by composition | opus |
| O1 | **The operator's split, stage 1** (decisions (o1), (o3); acceptance 13; verify item 4). Gitignored paths only, **no commit**: the baseline copy of the monolith, the organisation document, the rewritten root, their fake state and the `crates/willikins-cli/tests/operator_*.rs` tests and snapshots. The real names come from the coordinator's brief, never from this plan. Scoped: `-p willikins-cli`, the `operator_*` test targets | sonnet implements |
| O2 | **Live convergence** (decision (o3) step 2; acceptance 14). A read-only `plan --live` of the split root, shown to the operator, then the all-NoOp apply with the real journal. | coordinator |
| D1 | **Documents** (one commit each). The design doc gets an addendum and an updated milestone line, for decision (d2): the "one abstraction" survives at the signature, the execution model is flattened, and this plan's path. CLAUDE.md and AGENTS.md (edited together, byte-identical) get one invariant: "a used document is found by name in its parent's trusted source; no secret enters one; its nodes are named `<step>/<node>`". The README's document-format section documents `uses:`. HANDOFF gets the RESUME HERE block and the todo table, and `todos/2026-10-05-reusable-ios-app-document.md` records decision (o2)'s missing primitive. This plan gets its addenda and **Completed** header. | coordinator |

## Risks

1. **Two ensures on one natural key.** The same child used twice, or a diamond, puts two `ensure` nodes on the same
   resource in one plan. On a fresh resource the plan shows two Creates, and at apply the second converges by reading
   first. The hazard existed before (two monolith nodes can do it). Composition only makes it easier to write. A
   `check` warning for two non-pure nodes with the same tool and identical bindings is a candidate follow-up.
2. **Exact typing at the boundary is strict.** A parent that relied on a conversion has to convert in a tool node of
   its own, or declare the child input at the source type. The strictness is what makes substitution sound (d4).
3. **The node-path widening touches every crate that names a node.** The characterization snapshot staying
   byte-identical (Gates) is the regression evidence, and P1 lands alone so a break is attributable.
4. **A child's signature change refuses startup** for every composite over it (d10). That is loud by design, but one
   edit can take the server down until the composites are fixed. The CLI is unaffected.
5. **The private split may not be exactly node-for-node** if a moved literal's port only accepted it through a
   conversion (verify item 4). In that case the constant declares the port's type, and the equivalence test stays
   exact.
6. **Privacy.** The organisation document is the most name-dense file the operator has. Every real name in it stays
   in `private/`. A task that pastes one into a tracked file or a message is refused by the hooks, and must be
   rewritten with placeholders, never bypassed.
7. **Host contention.** Other sessions run cargo almost continuously. If no 3-second quiet window comes within 40
   minutes, the task stops and reports the contention.

## Needs the operator

Nothing blocks this milestone. O2 shows the operator the split document's live plan before its all-NoOp apply, as
earlier real runs did.
