# Adversarial pass 1 on the executor, the journal and the approval gate (acceptance test 19, first pass)

**Date:** 2026-09-14
**Target:** milestone 2 tasks 4, 5 and 6 as merged on `main` (`3a8e58c`):
`crates/willikins-core/src/apply.rs`, `crates/willikins-core/src/plan.rs`'s
`Plan::fingerprint`, and the whole of `crates/willikins-journal/`.
**Plan:** `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, its "Trust boundaries"
1, 2 and 5, its "Apply executor" rules with the 2026-09-14 addendum, its `willikins-journal`
section, and acceptance test 19.
**Todos read:** `todos/2026-09-14-plan-identity-must-cover-inputs.md`,
`todos/2026-09-14-journal-follow-ups.md`.
**Tests:** `crates/willikins-core/tests/apply_adversarial.rs` (11),
`crates/willikins-journal/tests/adversarial_pass_1.rs` (13),
`crates/willikins-cli/tests/adversarial_pass_1_cli.rs` (1).
**Todo added:** `todos/2026-09-14-pass-1-items-for-task-10a.md`.
**Fixtures added:** `workflows/fixtures/plan-identity-a.yaml`,
`workflows/fixtures/plan-identity-b.yaml`,
`workflows/fixtures/state/plan-identity-secrets.json`,
`workflows/fixtures/redaction-marker-default.yaml`.

The goals set for this pass, from acceptance test 19: (1) run an unapproved or drifted plan;
(2) get a secret byte into the journal, an `ApplyError`, an `Applied`, a `PlanRecord` or
`RunRecord` view, or a rendered CLI line; (3) make two applies interleave; (4) corrupt the
journal into an accepted replay. Plus the plan-identity gap the task 4 verifier left open, and
a short list of hostile-tool and hostile-observer probes.

**No attack reached a secret byte.** Every seeded marker — a minted service token, two seeded
Doppler secret values, and the two constant tokens `fake.secret_list` reports — stayed inside
its redaction in every `Applied`, every `ApplyError`, every JSONL line, every replayed view,
every `Debug`, and the CLI's text and JSON output. **One defect was found and fixed**: a
journal line could rewrite the record it named. The approval gate and the drift check held
under every probe; what they do *not* cover is now pinned as `boundary_` tests naming the
task (10a, or pass 2) that closes each one.

**Commits:** `5916f54` (executor pins), `de9f41c` (the fix, with the journal pins that share
its test binary), `ea5b053` (the CLI pin), and this note. The fix and its pins share a commit
deliberately: this host compiled `willikins-journal`'s test binary in twenty-one minutes on
the day, and splitting them would have bought one more compile cycle rather than one more
reviewable diff. The two duplicate-record tests were observed failing before the fix
(`sha256-forged` where `sha256-original` was expected) and passing after.

## Baseline

The tree was inherited at `3a8e58c` with the four gates recorded green by the preceding
task's own verification. This pass ran crate-scoped tests between
commits, on a host that compiled `willikins-journal`'s test binary in twenty-one minutes, and
the four gates in full on the tree it finishes with: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -j 2 -- -D warnings`, `RUST_TEST_THREADS=2 cargo test --workspace
-j 2 --no-fail-fast`, `cargo check -p willikins-types -j 2` — all four green, 1,325 tests
passing, with clippy taking twenty-four minutes and the test gate its own hour.

## Finding 1 — a journal line could rewrite the record it named (fixed)

`willikins-journal`'s own module doc says it plainly: "`append` is the only way to add an
entry; there is no delete or rewrite of any kind … an audit trail that could edit its own past
would not be one." It was not true. Four of the folded events keyed a record by an id and
*overwrote* whatever was already there, so appending one line — the one operation the design
does allow — rewrote the past:

| Second event | What it rewrote |
| --- | --- |
| `PlanRecorded` for a known `plan_id` | the whole `PlanRecord`: `applied` cleared (a spent plan reads as never run, so an `AlreadyApplied` check passes and the plan applies again), `approval` reset to `Pending`, and a fresh `document_sha256`, `fingerprint`, `class` and `requires_approval` swapped in for task 10a's plan identity and drift check to compare against |
| `RunStarted` for a known `run_id` | the run's state back to `Running`, its outcome dropped, and — through the fold's per-run `finished` map — every node event already folded into it erased |
| `RunFinished` for a run already finished | `state`, `outputs`, `error` and `finished_at`: a failed run appended over as a successful one |
| `NodeFinished` for a `(run, node, instance)` already finished | that instance's `status` and `outputs`: a `Failed` instance appended over as `Created`, the finest-grained rewrite of the four |

Observed pre-fix, from `a_second_plan_recorded_event_for_one_id_never_replaces_the_first`:

```
assertion `left == right` failed: the first record of a plan id is the only one
  left: "sha256-forged"
 right: "sha256-original"
```

None of the four is reachable through `append`: a `PlanId`/`RunId` is a fresh uuid v7 minted
once, `run_and_journal` appends exactly one `RunFinished` per run, and `apply` reports exactly
one `NodeFinished` per attempted instance. So a file holding a second one was edited — which
is exactly the attack of goal 4, and the one form of it the file's own replay validation
(contiguous `seq`, monotonic `at`, no truncated tail) did not catch, because a forged line
appended with the next `seq` and a current timestamp is well-formed.

**Fix** (`de9f41c`), in two layers:

- `crate::journal::fold` keeps the **first** record of an id and ignores a later duplicate, so
  every `Journal` implementation — `MemoryJournal`, `FileJournal`, and whatever a server
  wraps them in — reads the same, un-rewritable history.
- `FileJournal::open` **refuses** a file holding any of the four duplicates, naming the line
  the way it already names a `seq` gap or a backwards timestamp.

Fail-closed, deliberately, and with a cost worth stating: if a future `willikins-server` bug
ever appends a duplicate id, the journal stops opening at the next restart until an operator
trims the line by hand. That is the same trade-off `replay` already makes for a truncated
final line, and the alternative — opening a file whose audit trail is known to be
self-contradictory — is worse for the one thing a journal exists to do.

One thing already limited the damage and is worth knowing: a `RunRecord`'s `nodes` are folded
*against its plan's fingerprint* when that plan is in the journal, so a spliced `NodeFinished`
for an instance the plan does not contain is invisible in the view whatever it says. The
rewrite worked only on instances the plan really has — which is precisely the set an operator
reads to decide what happened.

Tests: `a_second_plan_recorded_event_for_one_id_never_replaces_the_first`,
`a_second_run_finished_event_for_one_run_never_replaces_the_first`,
`a_second_node_finished_event_for_one_instance_never_replaces_the_first`,
`a_file_with_a_duplicate_plan_id_is_refused_at_replay`,
`a_file_with_any_duplicated_record_is_refused_at_replay`.

## Attacks that held

Every one of these is a test, so a later change that opens one fails a test rather than only
contradicting this table. A `boundary_` name marks a thing core or the journal deliberately
does not do; the arrow in the "Pin" column names who closes it.

### Goal 1 — run an unapproved or drifted plan

| Attack | Outcome | Pin |
| --- | --- | --- |
| Clear `Plan::requires_approval`, downgrade `Plan::class`, apply with `Auto` | `ApprovalRequired { class: Irreversible }`; no `read`, no `ensure` — the gate reads the *checked* workflow's class | `apply.rs::a_cleared_approval_flag_does_not_bypass_the_gate` (task 4) |
| Approve plan A, present that approval for plan B | runs — `Approval::Human` carries no plan id at all | `boundary_an_approval_is_not_bound_to_the_plan_it_approves` → 10a |
| `Approval::Human { at: 2999-01-01 }` | runs — core never reads `at`; the two windows are the server's | `boundary_an_approval_timestamped_in_the_future_still_runs` → 10a |
| Replay a plan out of the journal with fields altered | **impossible by construction**: `PlanRecord::plan` is a `Redacted<Plan>`, opaque JSON with no way back to a `Plan` (which has no `Deserialize`). What *does* replay typed is the `fingerprint`, and a hand-edited one replays as truth | `boundary_a_hand_edited_plan_record_replays_as_truth` → 10a / operator |
| Re-order a `for_each` plan's instances | `Drift { DriftKind::Instance }`, nothing run | `apply.rs::for_each_instances_are_compared_by_key_not_only_by_position` (task 4) |
| Duplicate one `for_each` instance in the approved plan | `Drift` at `configs`, nothing run, no `ensure` | `a_duplicated_for_each_instance_in_the_approved_plan_is_instance_drift` |
| Tamper with the approved plan's per-node `inputs` | no effect: the executor walks the *fresh* plan and re-resolves every upstream port against this run's own results | `boundary_tampering_with_the_approved_plans_inputs_changes_nothing_that_runs` |
| Apply a plan approved for document A against document B with an equal fingerprint | **runs** — see below | `boundary_an_approved_plan_of_another_workflow_with_an_equal_fingerprint_runs`, `boundary_a_plan_approved_for_one_document_applies_to_another_with_the_same_fingerprint` → 10a |

**The plan-identity swap** is the one attack of goal 1 that succeeds, by design, and it is now
a fixture pair rather than a paragraph in a todo. `plan-identity-a.yaml` and
`plan-identity-b.yaml` differ only in which Doppler secret they read into the same GitHub
Actions secret. `doppler.secret.get` is pure (`Compute`) and its one output is secret, so it
contributes the fixed `<secret>` marker instead of a value; `github.actions_secret.ensure`
declares no outputs at all, and node *inputs* are not part of a fingerprint. The two plans are
therefore equal under `Plan::fingerprint()`, core never compares `approved.workflow` with the
checked workflow's name, and applying A's approved plan against B's `Checked` writes B's
secret with the journal recording a run of A. Closed by task 10a's `(workflow name, document
sha256)` plan identity — both fields are already in `PlanRecorded`; nothing yet compares them.
Core's fingerprint is deliberately unchanged here: whether it should fold inputs in is the
milestone 3 question `todos/2026-09-14-plan-identity-must-cover-inputs.md` holds.

### Goal 2 — a secret byte out

Every fake tool is exercised by at least one attack in this pass (`naming.v1`,
`github.repo.ensure`, `github.actions_secret.ensure`, `doppler.project.ensure`,
`doppler.config.ensure`, `doppler.service_token.ensure`, `doppler.service_token.rotate`,
`doppler.secret.get`, `fake.secret_list`, `fake.irreversible.ensure`, `template.render`).

| Attack | Outcome | Pin |
| --- | --- | --- |
| `rotate-service-token.yaml` under `Approval::Human`, `next_token` seeded to a marker, `fail_ensure_once` at the sink: mint a secret, hand it to a sink, then fail | marker in no JSONL line, no `RunRecord`/`PlanRecord` (JSON or `Debug`), no `ApplyError` rendering, and none after a reopen | `the_rotation_workflow_journals_a_minted_secret_and_a_tool_failure_without_leaking` |
| Two seeded `doppler.secret.get` values through the plan-identity pair | neither appears in the file | same test as the swap above |
| A secret *list* (`fake.secret_list`) into a `list<DopplerServiceToken>` port | `NodeStarted.inputs` carries exactly two markers, one per element, and neither token's bytes | `node_started_inputs_holding_a_secret_list_are_redacted_element_by_element` |
| A hostile `ToolError` message (`fail_ensure_once`) | the fake builds it from its own tool name and key, never from an input value; it reaches caller and journal verbatim, as `ToolError`'s contract says it may | pass-1 rotation test + `redaction_attacks.rs::a_tool_error_message_is_written_verbatim_because_only_the_tool_can_keep_it_clean` (task 5) |
| A secret literal in a document | refused by `check` before anything runs | `workflows/fixtures/secret-literal.yaml` (milestone 1) |
| A `for_each` over a secret list | `check` refuses it; and were it reachable, the instance key is `Value::render()`, which is the marker, not the bytes | `workflows/fixtures/secret-for-each.yaml` (milestone 1) |
| A tool returning undeclared output ports, or omitting a declared one | undeclared ports are dropped by `fill_outputs`; an omitted one becomes `Unknown` | `undeclared_outputs_are_dropped_and_forgotten_ones_become_unknown` |
| A tool returning a *secret* value on a port it declared non-secret | the value flows to a non-secret sink — the static taint rule holds only as far as a tool tells the truth about its own output types — but nothing prints it: redaction travels with the value, not with the port | `boundary_a_secret_returned_on_a_non_secret_port_flows_on_but_never_prints` → pass 2 |
| A document input default spelling `[REDACTED DopplerServiceToken]` | no leak (there is no secret), but text output cannot distinguish the forgery; JSON can | `a_document_default_spelling_the_redaction_marker_is_not_marked_redacted`, `adversarial_pass_1_cli.rs` → pass 2 |

The forged-marker case, from the CLI on `workflows/fixtures/redaction-marker-default.yaml`:

```
$ willikins plan workflows/fixtures/redaction-marker-default.yaml
readme (template.render): Compute
    rendered: Note: [REDACTED DopplerServiceToken]
outputs:
  rendered: Note: [REDACTED DopplerServiceToken]
```

The marker holds no character `single_line` escapes, so a forged one prints exactly like a
real one. The JSON surface — what an agent actually reads — does distinguish them: a genuinely
redacted value carries `"redacted": true` beside its marker and this one carries no `redacted`
key at all (`"type": "Text", "state": "known", "value": "[REDACTED DopplerServiceToken]"`).
Handed to pass 2 as a renderer question, not fixed here: the fix is a text mode that marks
redaction structurally rather than by the marker's spelling, and that decision belongs with
the CLI's `apply` and `run` renderers (task 11), which do not exist yet.

### Goal 3 — make two applies interleave

Two threads applying one approved plan against one shared `FakeState`, released together by a
barrier, twenty times over: no panic, no `unreachable!` reached inside the executor, no
resource created twice, and the marker in neither run's output. What core does **not** provide is any mutual exclusion. How far the second run
gets depends on the interleaving, and all three outcomes are legitimate: `Drift` (the first
run created the repository between the approval and this re-plan), `UnknownInput` (the first
run minted the token, so the second cannot re-read it), or a second write of the same Actions
secret — last write wins, and nothing notices. Pinned as
`boundary_two_threads_applying_one_plan_share_state_with_no_mutual_exclusion` with only the
interleaving-independent invariants asserted.

**What core guarantees without the single-apply lock**, stated so 10a can rely on it: a
refusal at rule 1 or rule 2 happens before any provider call; the re-plan at rule 2 observes
whatever the other run has already done and refuses on any difference; per-resource atomicity
is each tool's own (every fake tool reads and writes under one lock, and a live tool's
equivalent is its provider's conditional create). Nothing more. Two applies of *different*
plans touching one resource are caught by that same rule-2 re-plan rather than by any locking:
a plan for a public repository applied after a private one was created refuses with
`ApplyError::Plan { AttributeMismatch }` and never calls `ensure`
(`a_second_plan_touching_the_same_resource_is_refused_by_the_re_plan`). The single-apply lock
(`RunInProgress`) is task 10a's.

### Goal 4 — corrupt the journal into an accepted replay

| Edit | Outcome |
| --- | --- |
| Edit a payload on an earlier line | replays as truth — no hash chain, by decision. Pinned generically (task 5) and, for the plan-identity fields specifically, by `boundary_a_hand_edited_plan_record_replays_as_truth` |
| Swap two lines / delete an interior line / append a lower `seq` | refused, naming the line (task 5) |
| Truncate the last line | refused as truncated (task 5) |
| Back-date a line | refused as non-monotonic; `append` clamps its own clock read so it can never produce one (task 5) |
| Duplicate a line verbatim | refused by the `seq` check (task 5) |
| Duplicate a *record* on a correctly numbered line | **was accepted — finding 1**, now refused at replay and ignored by the fold |
| Splice a run with no `RunStarted` | invisible to every view rather than half-materialising (`a_spliced_run_with_no_run_started_is_invisible_to_the_views`) |
| Splice a `RunStarted` for a plan the journal never recorded | the run replays with the nodes that finished, in arrival order; no `PlanRecord` is invented (`boundary_a_run_started_for_an_unknown_plan_replays_without_inventing_a_plan`) |
| Grant an approval after a rejection | the last decision wins — the fold has to fold what it is given; refusing a second decision is the server's (`boundary_a_grant_after_a_rejection_is_the_decision_the_views_report`) → 10a |

### Observers

`apply` does not isolate its observer: a panic in `ApplyObserver::on` unwinds straight out of
the run, leaving writes already made in place and every later node unattempted, with no
`catch_unwind` anywhere. That is defensible — a journal that cannot record is a reason to stop
writing to providers, not to continue — but it is now a decision with a test
(`a_panicking_observer_unwinds_out_of_apply_leaving_earlier_writes_in_place`) rather than an
accident. A caller that survives such a panic must treat the run as unfinished, never as
refused.

## Handed to task 10a

Items 2 to 5 are also filed as `todos/2026-09-14-pass-1-items-for-task-10a.md`, each naming
the `boundary_` test that will need updating when it is closed; item 1 stays in the todo that
already holds it.

1. **Plan identity.** `Butler::apply` must compare the recorded `workflow` name and
   `document_sha256` with the document it reloads, and refuse with `DocumentChanged`.
   `workflows/fixtures/plan-identity-{a,b}.yaml` is the acceptance-test-8 pair: two documents
   with equal plan fingerprints where applying one's plan against the other writes a different
   secret.
2. **Bind the approval.** `Approval::Human` must be constructed only from a journaled
   `ApprovalGranted` for *that* `plan_id`, and both windows (approval, apply) must be checked
   there: core reads neither the plan id nor the timestamp.
3. **Single-apply lock.** `RunInProgress` and `AlreadyApplied` are the only things standing
   between two agents and the interleaving above; core provides none of it.
4. **A decision must be final.** The fold takes the last approval event for a plan, so an
   `ApprovalGranted` recorded after an `ApprovalRejected` revives the plan. The server must
   refuse to record a second decision for a plan that already has one (the alternative —
   making the fold first-decision-wins — is a semantic change this pass did not take
   unilaterally).
5. **`document_sha256` is a bare `String`** on `Event::PlanRecorded` with no grammar and no
   length bound, unlike every other agent-facing text field in the workspace. It is a
   plan-identity field; give it a validated hex type or validate it at the boundary.
6. **Journal follow-ups** from `todos/2026-09-14-journal-follow-ups.md` are unchanged by this
   pass and still land in 10a; the rename-over-the-path one in particular is the other half of
   "the file can be edited under us".

## Handed to adversarial pass 2

- **The text renderer cannot distinguish a forged redaction marker** from a real one. Decide
  it when the `apply`/`run` renderers land (task 11).
- **A tool that lies about its own output types is not caught** by `fill_outputs`, in `plan`
  or in `apply`. Every tool in the workspace is our own code, so this is a hardening item, not
  a live hole: validate each returned value's type against the declared port type, or state
  the assumption in `Tool`'s contract.
- **No `catch_unwind` around observers**, and no way for an observer to stop a run other than
  panicking.
- The HTTP and MCP surfaces this pass never touched: everything in acceptance test 19's second
  half.

## Plan defects found

1. **Acceptance test 19's first half asks for "a `tracing` line"**, but nothing in
   `willikins-core` or `willikins-journal` emits `tracing` output — it arrives with the server
   (task 10b). That quarter of the goal is untestable at this task and should move to pass 2's
   list.
2. **The `willikins-journal` section's replay-error list** ("Sequence numbers are contiguous; a
   gap or a non-monotonic timestamp on replay is an error") is now short by one class: a
   duplicate `plan_id`/`run_id`, a second `RunFinished` for a run, or a second `NodeFinished`
   for an instance is a replay error too. Normative addendum needed (this pass may not edit
   the plan).
