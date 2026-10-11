# Milestone 3o: a document provisions a project's AGENTS.md

**Created:** 2026-10-06 (from `todos/2026-10-06-provision-agents-md.md`; three research passes: the agent-instruction
**Completed:** 2026-10-11 — the operator's app received its two approved AGENTS.md files (app root and ios/) through a second scaffold with its own marker: one signed commit, files byte-identical to the approved words; a fresh live plan reads every node NoOp (20 NoOp, 64 Compute).
file conventions and the operator's monorepo, the facts an app file needs, and what the types allow across a `uses:`
boundary)

**Addendum:** 2026-10-09 — the operator approved **two files**, not one: `apps/<slug>/AGENTS.md` and
`apps/<slug>/ios/AGENTS.md`, both owned by the project document (decision (d2) extends to the second file the same
way: the monorepo root's own `AGENTS.md` already carries the organisation's conventions, so neither app file repeats
them). Task O1's nodes are now two renders, `agents_md` and `ios_agents_md`, both bound into the second scaffold's
`files` beside the single render SHARED VALUES' "Private nodes (O1)" row originally described; the monolith baseline
gained the same three nodes (two renders plus the second scaffold), and the converged counts move to 20 `NoOp`, 64
`Compute` (one more `Compute` than the single-file shape, the second render; the second scaffold still converges to
one `NoOp` regardless of how many files its marker seeds).

Separately, the monorepo's root `CLAUDE.md` was replaced by an identical `AGENTS.md`. This closes risk 3 and verify
item 1: there is no longer a root `CLAUDE.md` to make Claude Code skip an app's own `AGENTS.md` by default, so "Needs
the operator" item 2 needed no monorepo edit after all. Decision (d5) stands unchanged for both files: neither seeds
a `docs/HANDOFF.md` stub — the first session that changes anything still creates it by hand.

The expected live plan (task L1), once the operator approves it: the second scaffold (`<slug>_agents`) `Create`,
both renders (`agents_md`, `ios_agents_md`) `Compute`, everything else already converged reads `NoOp`.

## Goal

A document writes a project's `AGENTS.md` with the tools that exist (`repo.file.render`, `github.scaffold.ensure`,
`uses:`), no new tool, no CLAUDE.md. Concretely:

1. **A monorepo app** gets `apps/<slug>/AGENTS.md`, rendered by the project's own document. That includes an app whose
   scaffold has already landed, through a second scaffold node with its own marker.
2. **A standalone repository** (milestone 3l) gets `AGENTS.md` at its root, carrying the organisation's conventions,
   rendered by an organisation-owned used document and landed in the project's one scaffold commit.
3. **Public examples** of both shapes, with placeholders and tests over the fake.
4. **The operator's already-scaffolded iOS app** gets its file: the private documents are edited so that a fake plan
   renders the reviewed draft byte for byte, and a live run, after the operator approves, writes it in one commit.

The operator, 2026-10-06: willikins "will also need to be able to provision the AGENTS.md file"; "Willikins doesn't
*own* anything, does it? It's just about *provisioning*"; "AGENTS.md only"; "the content would have to come from the
document. It's just file writing, with limited text-only templating capability."; "No need to make a dedicated tool or
anything."

## What the types already decide

Every point below was checked in the source or with scratch documents over the fake (research pass 3):

- `repo.file.render` has one scalar `template: TemplateSource` port. No tool takes a list of templates, and no tool
  joins two `RepoFile`s. **The bytes of one file come from one template literal in one document**, plus token values.
- `TemplateValue` is `[A-Za-z0-9_][A-Za-z0-9._/-]*`, at most 255 characters: no space, no newline. **Prose never
  arrives through `values`**. A prose literal at a `uses:` boundary is `InvalidLiteral`, and a root input with a space
  is refused at `describe`/`plan`.
- No `Text => TemplateValue` row exists (3g decision (e), pinned in `registry.rs`). The only row into `TemplateValue`
  is `AppleBundleIdentifier => TemplateValue`. No row targets `TemplateSource` or `RepoFile`.
- `TemplateSource` and `RepoFile` are refused as input types, a used document's included (`DisallowedInputType`). A
  `RepoFile` literal is refused, including one that arrives through a child's literal output (`RepoFileLiteral`).
- A `RepoFile` may cross a boundary outward as a child's `Step`-bound output. `check_boundaries` checks inputs only,
  with an exact type match and no conversion.
- A literal output is refused at the root (`LiteralOutput`, `check.rs` `check_outputs`). A document whose only use is
  as a child may hold one (2b decision (d7)), but it then fails `validate` when run alone.
- Template grammar: any `{{` that is not exactly `{{ N }}` (N from 0 to 15) is refused at plan. There is no escaping.
  There are at most 16 values, every bound value must be used, and the rendered file is at most 65,536 characters.
- A scaffold never reads or writes its files again once its marker exists. The key is `(repo, branch, marker)`, so two
  scaffolds on one branch with different markers are distinct resources. A different file already at a seed path is
  `Conflict` at plan; a byte-equal one converges (only the marker is committed).

So prose from two documents cannot be joined into one file without a new primitive (a multi-template render or a
concatenation tool), and the operator refused a new tool. Every decision below gives each `AGENTS.md` **exactly one
owning document**, the one that holds all of its prose. Any other document contributes tokens only.

## Out of scope

- **A new tool, a multi-template render, or file concatenation.**
- **CLAUDE.md** (the operator: "AGENTS.md only"), including an app `CLAUDE.md` that imports `@AGENTS.md`.
- **Provisioning a monorepo's root `AGENTS.md`.** It exists, written by hand. A scaffold naming it would get `Conflict`.
- **A readable multi-line file view** in plan text or on the approvals page (risk 4, a follow-up todo).
- **A `ProjectSlug => TemplateValue` conversion row** (decision (d3)'s cost). It would be its own reviewed change.
- **Deriving `apps/<slug>/AGENTS.md` from a slug.** Paths stay literals (`todos/2026-10-05-reusable-ios-app-document.md`).
- **Any live provider call by an agent.** The one live run is the coordinator's (task L1), after the operator approves.

## Trust boundaries (normative)

1. **The words are document content.** Every sentence of every `AGENTS.md` is a template literal in a trusted
   document. A caller or a provisioning agent contributes tokens only, through `TemplateValue` or a typed input
   converted by a registered row. No route lets a caller's text become prose.
2. **Written once, never owned.** Seed semantics: willikins writes the file once and never overwrites it, and a
   different file already at the path fails the plan instead of being replaced.
3. **The project's own GitHub credential, never the operator's** (CLAUDE.md, `no_gh_writes_guard`). The private app's
   second scaffold uses the same narrow write token as its first.
4. **Placeholders only in tracked files.** The draft and the private documents stay gitignored.

## SHARED VALUES

| What | Value |
| --- | --- |
| Fixture directory | `workflows/fixtures/agents-md/` (new). Not scanned by the dsl characterization (top level and `workflows/fixtures/` only) nor by the composition characterization (`fixtures/composition/` only); `image_contents.rs` only requires it to be nested and dockerignored |
| Organisation AGENTS.md document (E1) | `workflows/fixtures/agents-md/example-org-agents-md.yaml`, `name: example-org-agents-md` (unique across `workflows/`). Input `project: { type: TemplateValue }`, no default. One node `agents`: `repo.file.render`, `path: AGENTS.md` (literal), `values: ['${{ inputs.project }}']`, template = the organisation's conventions, `{{ 0 }}` = the project. Output `file: ${{ steps.agents.file }}` |
| Standalone project document (E1) | `workflows/fixtures/agents-md/standalone-service.yaml`, `name: standalone-service`. Inputs `repo: GitHubRepo`, `project: TemplateValue`, `visibility: { type: RepoVisibility, default: private }`. Steps, in order: `agents_md` (`uses: example-org-agents-md`, `with: { project: ${{ inputs.project }} }`), `repo` (`github.repo.ensure`), `readme_file` (`repo.file.render`, `README.md`), `scaffold` (`github.scaffold.ensure`, `repo: ${{ steps.repo.repo }}`, `branch: main`, `marker: .willikins-scaffold`, `files: [readme_file.file, agents_md.file]`, `message: "chore: scaffold the repository"`). Linked render node: `agents_md/agents` |
| Monorepo app document (E2) | `workflows/fixtures/agents-md/monorepo-app.yaml`, `name: monorepo-app`. Input `app_identifier: AppleBundleIdentifier`, no default. Steps: `monorepo` (`github.repo.get`, `repo: Example-Org/monorepo`), `app_build` (`repo.file.render`, `apps/sample/BUILD.bazel`), `app_files` (`github.scaffold.ensure`, `repo: ${{ steps.monorepo.repo }}`, `branch: main`, `marker: apps/sample/.willikins-scaffold`, `files: [app_build.file]`), `agents_md` (`repo.file.render`, `path: apps/sample/AGENTS.md`, `values: ['${{ inputs.app_identifier }}']`), `agents` (`github.scaffold.ensure`, `repo: ${{ steps.app_files.repo }}`, `branch: main`, `marker: apps/sample/.willikins-agents`, `files: [agents_md.file]`, `message: "docs(sample): seed AGENTS.md"`) |
| Expected bytes (E1, E2) | `workflows/fixtures/agents-md/expected/standalone-AGENTS.md` (rendered with `project: sample-service`), `workflows/fixtures/agents-md/expected/monorepo-app-AGENTS.md` (rendered with `app_identifier: com.example.app`). Each ends in exactly one `\n` |
| Negative fixture (E1) | `workflows/fixtures/agents-md/project-prose-root.yaml`: `uses: example-org-agents-md` with `project: "A sentence with spaces"`, refused `InvalidLiteral` naming the `with:` input |
| Test module | `crates/willikins-cli/tests/it/agents_md_documents.rs`, declared in `crates/willikins-cli/tests/it/main.rs` (`test_layout_guard`) |
| Second-scaffold marker | `apps/<slug>/.willikins-agents`, one file: `apps/<slug>/AGENTS.md` |
| Second-scaffold ordering | `repo: ${{ steps.<first scaffold>.repo }}`: the data edge orders the second commit after the first |
| Private nodes (O1) | `agents_md` (`repo.file.render`, `path: apps/<slug>/AGENTS.md`, `values` = the three bundle-identifier inputs, `0` host, `1` notification service, `2` widgets, the same order as every other render in that document) and `<slug>_agents` (`github.scaffold.ensure`, the marker above, `repo: ${{ steps.<slug>_files.repo }}`, `branch: main`, the same write token as `<slug>_files`, `message: "docs(<slug>): seed AGENTS.md"`). The real names come from the coordinator's brief, never from this plan |
| Private draft | `private/agents-md-draft/AGENTS.md` (gitignored, written 2026-10-06 with this plan; the operator reviews it) |
| Converged counts after O1 | 20 NoOp, 63 Compute, 0 Blocked, 0 Skip, 0 Create, 0 Update, 0 Replace (was 19 / 62) |

## Decisions

### (d1) One file, one owning document; the other side gives tokens only

**Decided.** Every `AGENTS.md` is one `repo.file.render` whose template literal sits in exactly one document. A second
document contributes tokens only: `TemplateValue` inputs, or typed inputs through a registered row, bound as `values`.

**Why.** The types forbid anything else without a new primitive ("What the types already decide"). This is also what
the operator described: "part of the content org / monorepo specific, and some of it provided as input". The input
part is tokens, and the operator said inputs fill placeholders. The todo's wording ("the organisation's and
monorepo's part in the organisation document, the project's part in the project's document") cannot hold literally
for prose on both sides. Decisions (d2) and (d3) show that it does not need to.

**Cost.** A project document cannot add its own prose to a file whose template the organisation holds, and the
reverse. Where both kinds of prose are needed in one file, the owning document copies the other's sentences.

### (d2) A monorepo app: the project document owns the file, and it has no organisation part

**Decided.** In a monorepo whose root `AGENTS.md` already carries the organisation's conventions, the app file has no
organisation part. The project document renders `apps/<slug>/AGENTS.md` from its own template, next to the files it
describes. It names the root in one line (`../../AGENTS.md`) and holds only what is the app's: where its handoff
lives, its identity (bundle identifiers through the existing `AppleBundleIdentifier => TemplateValue` row), the
commands its own scaffold's CI runs, a pointer to its release README, and cautions about its own seeded files. The
organisation document is not touched.

**Why.**
- **The root file is the organisation part.** Codex walks from the git root to the working directory and joins every
  `AGENTS.md` it finds, root first. The root file in the operator's monorepo also already states the precedence: a
  more specific `AGENTS.md` overrides it "for handoff docs, process, commands, priorities, and cautions", and an
  app-scoped session follows the app's handoff. Repeating any of that in the app file adds nothing.
- **Repetition rots.** Both hand-written sibling app files in the operator's monorepo restate root sections, and both
  have drifted from the root: one still describes branch worktrees that predate the trunk-based ruling. A seed is
  written once and never updated, so any copied convention would drift the same way, with no owner.
- **The file describes the scaffold.** Its commands and cautions name the targets, scripts and pipeline files that
  same document seeds, so they change together in one document. The organisation document does not know what an
  iOS app's scaffold contains. With the reusable iOS-app document (2b decision (o2)), the template is written once for
  every app of that kind.
- **Paths are relative to the app directory** (`docs/HANDOFF.md`, `../../AGENTS.md`), so the file reads correctly in
  a session launched in `apps/<slug>/`, the only place Codex loads it.

**Cost.** None for the organisation document. If the monorepo's root file is ever removed, the app files carry no
conventions.

### (d3) A standalone repository: an organisation-owned used document renders the whole file

**Decided.** The organisation's `AGENTS.md` for a standalone repository is its own small used document
(`example-org-agents-md`). It has one `repo.file.render` node with `path: AGENTS.md` as a literal, the organisation's
conventions as the template, one `project: TemplateValue` input, and `outputs: file: ${{ steps.agents.file }}`. The
standalone project document `uses:` it and lists `${{ steps.agents_md.file }}` in its one scaffold's `files`. A new
repository therefore lands in one commit with one marker (milestone 3l's flow, with the todo's "AGENTS.md joins the
scaffold's files").

**Why this shape over the alternatives.**
- **Not a literal template output from the organisation document** (the organisation exports its template text and
  the project renders it). It links, and plans over the fake, but `check_outputs` refuses a literal output at the
  root, so the organisation document would fail `validate` when run alone. A trusted directory whose documents fail
  `check` stops `Butler::start`. 3g decision (e) also says a `TemplateSource` is always bound straight to
  `repo.file.render.template`. This shape would be the first to route one through an alias.
- **Not a render node inside the organisation document itself.** Every parent of the organisation document, the
  monorepo app included, would then have to bind the render's inputs and would get an unused `org/agents` node. A
  dedicated document is linked only by the parents that want it.
- **Not the project document owning the template** (the organisation gives only tokens). The organisation's prose
  would then be copied into every standalone project's document.
- **Not two files.** A standalone repository has one root.

**Cost.**
- The project contributes tokens only. A project kind that needs its own prose in a standalone file must own the
  whole template instead, copying the organisation's sentences.
- The `uses:` boundary matches exactly, with no conversion (`UsesInputTypeMismatch`), and no `ProjectSlug =>
  TemplateValue` row exists. So the standalone document declares `project: TemplateValue` beside its own naming input,
  and the caller types the name twice.
- The organisation document has to name every project token it uses, in placeholder order.

### (d4) An already-scaffolded project: a second scaffold node with its own marker

**Decided.** For a project whose scaffold has already landed, the document adds a second `github.scaffold.ensure` node
whose only file is `AGENTS.md`, with marker `apps/<slug>/.willikins-agents`, the same repository, branch and write
token, and `repo` bound from the first scaffold's pass-through output.

**Why.** Adding `AGENTS.md` to the first scaffold's `files` does nothing, with no warning: the marker is present, so
that scaffold reads `NoOp` and never looks at its files again (research pass 3, N8). A second marker is a distinct
resource, so it plans `Create` on an already-scaffolded project and on a brand-new one alike (N7c), and one document
shape covers both. Binding `repo` from the first scaffold orders the two commits by a data edge rather than by the
commit retry alone.

**Cost.** One more marker file committed per project, and two commits on a brand-new project. For the operator's app,
only the second scaffold writes: one commit.

### (d5) The handoff the app file points at may not exist yet

**Decided.** The app file says to read `docs/HANDOFF.md`, and to create it in the first session that changes
anything. No stub is seeded.

**Why.** The operator asked for `AGENTS.md` only, and a seeded handoff would be a second seed with its own content to
review. The operator can choose otherwise ("Needs the operator", item 3). That costs one more file in the same second
scaffold, under the same marker.

### (d6) Only what does not rot

**Decided.** The template holds identity, paths, commands copied from what the scaffold's CI runs, and cautions about
seeded files. It holds no current state, priorities, pending work or product direction. Those belong in the app's
handoff, which the root file already defers to.

**Why.** The file is written once and never updated by willikins. A hand-written sibling file's model-trial sentence
has already drifted between its two copies.

### (d7) Reviewing the words

**Decided.** The rendered file appears in the plan, so approving the plan approves the words (the todo). Because a
long `RepoFile` prints as one escaped line in plan text (and as a JSON string under `--json` and on the approvals
page), the operator reviews the draft file. A private test pins that the plan's rendered bytes equal that draft, byte
for byte. The public examples do the same against `expected/` files.

## Acceptance tests

Standalone (E1, `agents_md_documents.rs`, over the fake):
1. `example-org-agents-md` checks **as a root** against the fake catalog: the decision (d3) property the alternative
   with a literal output fails.
2. `standalone-service` links and checks. With an empty `FakeState`, run 1 plans `repo` `Create`, `agents_md/agents`
   `Compute`, `scaffold` `Create`. The rendered `agents_md/agents` file's path is `AGENTS.md` and its content equals
   `expected/standalone-AGENTS.md`. Apply (approving the scaffold's class the way `new_repository_scaffold_document.rs`
   does), then run 2 plans every node `NoOp`, and the fake holds `README.md`, `AGENTS.md` and the marker.
3. `project-prose-root.yaml` is refused `InvalidLiteral` naming the `with:` input, at `validate`, before any plan. Its
   header names this test and the error.

Monorepo app (E2):
4. `monorepo-app` checks. **Fresh monorepo** (the repository exists, no markers): `app_files` and `agents` both plan
   `Create`. The rendered `agents_md` file's path is `apps/sample/AGENTS.md` and its content equals
   `expected/monorepo-app-AGENTS.md`, with `com.example.app` in place of `{{ 0 }}`.
5. **Already scaffolded** (the first marker present in the fake, through the fake's existing seeding): `app_files`
   plans `NoOp` and `agents` `Create`. Apply, then re-plan: every node `NoOp`.
6. **A different `apps/sample/AGENTS.md` already present** (`with_scaffold_files`): plan fails, naming the path.
   **A byte-equal one present:** `agents` plans `Create`, the apply commits only the marker, and the re-plan is `NoOp`.
7. `agents.repo` binds `app_files.repo` (a `Binding::Step`), pinning the order.

Private (O1, gitignored tests in `crates/willikins-cli/tests/operator_*.rs`, never committed):
8. **Byte for byte.** With the document's own test inputs (the real identifiers), the plan's `agents_md` file has path
   `apps/<slug>/AGENTS.md`, and its content equals `fs::read_to_string("private/agents-md-draft/AGENTS.md")`.
9. With the first scaffold's marker already landed, `<slug>_files` plans `NoOp` and `<slug>_agents` `Create`. Apply,
   then re-plan: `NoOp`. A different `apps/<slug>/AGENTS.md` already present fails the plan, naming the path.
10. The split stays equivalent: the monolith baseline gains the same two nodes, and the converged counts are 20 NoOp
    and 63 Compute.

Guards (all tasks):
11. `secret_literal_guard`, `no_gh_writes_guard` and `test_layout_guard` green. No operator name in any tracked file
    (the local hooks).

## Verify before relying on them

1. **Claude Code does not load `apps/<slug>/AGENTS.md`** in the operator's monorepo with default settings. This is from
   the Claude Code memory documentation ("When Claude Code reads AGENTS.md": by default only when no CLAUDE.md exists
   in the working directory or above it, and the monorepo has a root CLAUDE.md), not tested live. Check with `/context`
   in a session launched in `apps/<slug>/` after L1. This decides "Needs the operator", item 2.
2. **Codex loads root then app** when launched in `apps/<slug>/`, within its 32 KiB default budget (the root file is
   about 8 KB, the draft about 2.5 KB). From Codex's `agents-md` guide, not tested live.
3. **The draft's commands.** The tools' unit-test command passed on a scratch copy of the app's `tools/` directory,
   run from a stand-in monorepo root (36 tests, OK, 2026-10-06). The two `bazel build` commands are copied from the
   app's own validation script and were **not** run here: running them writes into the monorepo and needs disk.
   The pipeline's first validation build, or the operator, confirms them.
4. **Two scaffold nodes on one branch in one live apply** (the brand-new project case). Proven over the fake only; the
   fake does not model head movement. The `repo` data edge orders them, and `ensure` re-observes the head. The
   operator's app writes one commit, so its L1 run does not test this. A sandbox cycle would.
5. **No hand-written `apps/<slug>/AGENTS.md` exists** in the monorepo when L1 runs. It did not on 2026-10-06. If one
   appears, the plan fails with `Conflict` and writes nothing.

## Gates

Scoped only, never the full workspace gate. Before each cargo command, wait for 3 consecutive quiet seconds
(`pgrep -x cargo` and `pgrep -f cargo-sweep` empty). Run cargo with `-j 2` and `RUST_TEST_THREADS=2`, in the
background with a 600000 ms timeout, and read the log body.

- E1, E2: `cargo fmt --all --check`; `cargo clippy -p willikins-cli --all-targets -j 2 -- -D warnings`;
  `cargo test -p willikins-cli --test it -j 2 agents_md` and then `test_layout_guard`; `cargo test -p willikins-dsl
  --test it -j 2` once (the characterization must stay byte-identical); `cargo test -p willikins-server --test it
  -j 2 image_contents` once.
- O1: `cargo test -p willikins-cli -j 2 --test operator_<slug>_document --test operator_<slug>_split_equivalence`
  (the real target names; the files are gitignored), plus the remaining `operator_*` targets once.
- Before each commit, the coordinator runs the four CLAUDE.md commands scoped to `-p willikins-cli`. The workspace
  check of `willikins-types` is not needed: no crate source changes.

## Tasks

| # | Task | Delegate to |
| --- | --- | --- |
| E1 | **The standalone pair** (decisions (d1), (d3); acceptance 1-3). Test first: `crates/willikins-cli/tests/it/agents_md_documents.rs` (and its `mod` line in `tests/it/main.rs`), reusing the linking helper `composition_equivalence.rs` uses and the apply/approval helpers `new_repository_scaffold_document.rs` uses. Then `example-org-agents-md.yaml`, `standalone-service.yaml`, `project-prose-root.yaml` and `expected/standalone-AGENTS.md`, each document with a header comment naming its acceptance test (and, for the negative, the exact error). The organisation template is short placeholder prose for an `Example-Org` repository (title `# {{ 0 }}`, a conventions list, a commands section), with no `{{` other than `{{ 0 }}` and no trailing whitespace. One commit: "Example: an organisation document renders a standalone repository's AGENTS.md". Gates: E1/E2 row | sonnet implements |
| E2 | **The monorepo app document** (decisions (d2), (d4); acceptance 4-7). Test first, in the same module: the four acceptance cases. Then `monorepo-app.yaml` and `expected/monorepo-app-AGENTS.md`: a short app file that names `../../AGENTS.md` in one line, and has a handoff pointer, the bundle identifier as `{{ 0 }}`, a commands block, and a caution to commit only `apps/sample/` paths. No organisation conventions. One commit: "Example: a monorepo app seeds its AGENTS.md through a second scaffold". Gates: E1/E2 row | sonnet implements |
| X1 | **Adversarial pass on E1/E2**, recorded at `docs/research/2026-10-0x-m3o-adversarial-pass.md` (placeholders only). Every bypass becomes a fixture under `workflows/fixtures/agents-md/` plus a test. Targets: any route by which caller text becomes prose (an input, a default, a `with:` literal, a list element, a conversion); a parent supplying or replacing the organisation's template (a `TemplateSource` or `RepoFile` input, a literal output, a shadowed name); the two scaffolds sharing a marker, or one listing the other's marker or files; a seed path outside the app (`..`, `.git`, `.github/workflows`); a pre-existing file overwritten; `{{` smuggled through a value. Restore at least three mutations from saved copies (`cmp` for byte identity) | opus |
| O1 | **The operator's documents and tests** (decisions (d2), (d4), (d5); acceptance 8-10). Gitignored paths only, **no commit**. Test first, in the existing operator test files: acceptance 8 and 9; update `the_split_root_plans_the_identical_multiset_of_tool_calls_as_the_monolith`'s counts to 20 / 63; let the test that asserts every render is bound by the first scaffold also accept `agents_md` bound by `<slug>_agents` alone; add `agents_md` to the rendered-files list (one new gitignored snapshot). Then, in the app document: `agents_md` and `<slug>_agents` (SHARED VALUES), placed after the first scaffold, and a header paragraph naming this milestone. The template is the draft with the three bundle identifiers replaced by `{{ 0 }}`, `{{ 1 }}`, `{{ 2 }}`, in a YAML `\|` block (one trailing newline, no trailing whitespace). Add the same two nodes to the monolith baseline under `private/workflows/fixtures/`. The organisation document is **not** edited (decision (d2)). The real names come from the coordinator's brief. Never write to the operator's monorepo. Gates: O1 row | sonnet implements |
| O2 | **Attack O1**, findings returned to the coordinator, never committed (they name the operator's setup): the rendered bytes equal the draft; no other node's inputs, outputs or action changed (compare the plan's node multiset before and after, ignoring the two new nodes); the template has no `{{` other than the three placeholders and binds each once; no secret value, Doppler secret value or token shape in the draft; `<slug>_agents` uses the narrow write token, not the read token | opus |
| L1 | **The live run** (verify items 1, 5). After the operator has approved the draft and decided "Needs the operator" items 2 and 3: a read-only `plan --live` of the app document, shown to the operator (expected: everything as converged, plus `<slug>_agents` `Create` and `agents_md` `Compute`). Apply only after the operator approves that plan, then re-plan: all NoOp at 20 / 63. Then check verify item 1 with `/context` in `apps/<slug>/` | coordinator |
| D1 | **Documents.** The todo gets `status: completed`; HANDOFF gets the RESUME HERE block; the README's document-format section gets two sentences: an `AGENTS.md` is a file a document renders, and an already-scaffolded project uses a second marker. A follow-up todo for a readable multi-line `RepoFile` view in plan text and on the approvals page. This plan gets its **Reviewed**, addenda and **Completed** headers | coordinator |

Order: E1, E2, X1, then O1, O2, then the operator's review, L1, D1. E1 and E2 share one test module and one crate,
so they run one after the other (one cargo lane on this host).

## Risks

1. **The words are frozen on landing.** Willikins never rewrites the file, so a mistake in the template is fixed by
   hand in the monorepo, not by editing the document. Mitigation: the operator reviews the draft before O1, and
   acceptance 8 pins the plan to it.
2. **Commands rot.** If the scaffold's targets or flags change, the seeded file goes stale. Mitigation: decision (d6),
   and the commands are copied from what the scaffold's own CI runs, so the same edit breaks CI first.
3. **The file may be invisible to Claude Code** in a monorepo with a root CLAUDE.md (verify item 1). Codex reads it.
   The remedy is outside willikins ("Needs the operator", item 2).
4. **Readability of the plan.** A rendered file prints as one escaped line. Mitigation: decision (d7). A readable view
   is a follow-up todo, a CLI and server change, not a new tool.
5. **A project with a hand-written `AGENTS.md`** gets `Conflict` at plan. That is the intended refusal (trust
   boundary 2). Such a project's document should leave out the second scaffold.
6. **Two commits on a brand-new project** (decision (d4)). Stated, and ordered by a data edge.
7. **Privacy.** Placeholders only in tracked files, and the local hooks refuse otherwise, never bypassed. The draft
   and the private documents name the operator's setup and stay under `private/` and the gitignored tests.
8. **Host contention.** If no 3-second quiet window comes within 40 minutes, the task stops and reports it.

## Needs the operator

1. **Approve the draft's words** (`private/agents-md-draft/AGENTS.md`) before O1 freezes them into the document. Once
   landed, willikins never changes the file.
2. **Claude Code and the app file** (verify item 1, unconfirmed). Under the monorepo's root CLAUDE.md, Claude Code by
   default reads CLAUDE.md files only, so it would not load `apps/<slug>/AGENTS.md`. Codex would. Options, all outside
   willikins: (a) the user-level Claude Code setting that loads both CLAUDE.md and AGENTS.md files (it is ignored in
   project settings, so it is set once per machine that runs agents); (b) a line in the monorepo's root CLAUDE.md that
   points at app `AGENTS.md` files (a monorepo edit); (c) accept that only AGENTS.md-native agents read it. An app
   CLAUDE.md that imports `@AGENTS.md` is excluded ("AGENTS.md only").
3. **A handoff stub or not** (decision (d5)). The default is no stub; the file tells the agent to create
   `docs/HANDOFF.md`. A stub would be one more seeded file under the same marker.
4. **Approve the live plan** (L1) before the apply. It makes one commit, by willikins with the project's write token,
   on the monorepo's `main`.

**Addendum:** 2026-10-11 (coordinator, L1 and D1) — the operator approved the two files' words on 2026-10-09 and the live plan on 2026-10-11 (the second scaffold Create, two renders Compute, everything else NoOp). The apply made one signed commit adding the two files and the marker; both read back byte-identical to the approved texts. Before it, the monorepo's root CLAUDE.md was replaced by an identical AGENTS.md (by hand, outside willikins), so Claude Code and Codex both read nested AGENTS.md files. The stale SHARED VALUES rows (converged counts, the draft path, one render) stay superseded by the 2026-10-09 addendum rather than rewritten, as this repository's addenda do.

