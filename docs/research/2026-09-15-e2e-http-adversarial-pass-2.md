# End-to-end adversarial pass 2 (acceptance test 19, second half)

**Date:** 2026-09-15
**Target:** the finished milestone 2 — `willikins-server` (the `Butler`, the rmcp tool
surface, the Streamable HTTP transport, the approvals page), the CLI, `willikins-tools`,
and the journal's wire format — attacked **end to end over TCP against the real binary**.
Every HTTP attack starts a `willikins-server serve --http --bind 127.0.0.1:0` child
process with `env_clear()` and `WILLIKINS_FAKE_CATALOG=1`, learns the port by parsing the
`willikins-server listening` line out of its own JSON `tracing` output, and speaks HTTP by
hand on a `std::net::TcpStream` — which is the only way to send the requests a client
library will not: `HTTP/1.0`, a repeated `Authorization` header, a body whose bytes never
arrive, a `Content-Length` that lies.
**Boundaries kept:** no Railway command of any kind; the sandbox credential file was never
read; no call reached GitHub or Doppler; the fake catalog is the only catalog; the binary
bound `127.0.0.1` on an ephemeral port only; the doppler crate's `live-tests` feature
stayed off; `Plan::fingerprint` is unchanged.
**Plan:** `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` — "Trust boundaries",
the `willikins-server` crate contract, acceptance test 19, "Risks", "Review resolutions",
and the 2026-09-14/15 addenda for tasks 10a, 10b, 11 and 12.
**Inherits:** `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`'s "Handed to
adversarial pass 2", and the four verifies' carried items listed in the plan's addenda.

**Reviewed:** 2026-09-15 (completeness critic) — every amendment below marked
*completeness critic, 2026-09-15*. The critic added no source change: nothing in
`crates/*/src` moved. What it changed is the evidence — one vacuous test rewritten, one
test-harness defect that turned a regression into a hang, two handed-over items the pass
never attacked, a second frozen fixture for the shapes the first one cannot carry, and a
liveness assertion under each of the two sweeps the headline rests on. Its own section is
at the end, with the mutation table.

**Tests added:** 45 by the pass, in three files; 10 more by the critic (below), for 55.
`crates/willikins-server/tests/adversarial_13.rs` (31 attacks against the real binary over
TCP, two of them `#[ignore]`d because they hold a connection for tens of seconds),
`crates/willikins-server/tests/blocking_pool_13.rs` (4: the blocking pool and journal failure
injection, in process, both needing a tool and a journal that misbehave on command),
`crates/willikins-journal/tests/pre_pass_2_replay.rs` (10, plus the one-shot `#[ignore]`d
generator) over the frozen fixture
`crates/willikins-journal/tests/fixtures/pre-pass-2-every-event.jsonl`. Plus the unit tests
that came with each fix: `template.render`'s bound and non-features, and `NonceStore`'s
compare-then-remove.

**Fixtures added:** none. No attack in this pass produced a *document* that got past a
boundary, so `workflows/fixtures/` gains nothing: the rule is a fixture where a document is
the vector and a test otherwise, and every document-shaped attack here was refused. The
hostile documents this pass did author (an ANSI-escape description, a `name:`/stem
mismatch dropped in after startup, a symlinked document) are written by the tests into
their own temporary trusted directories, because each is about *where and when* the file
appears, which a checked-in fixture cannot express.

## Goals, from acceptance test 19

> bypass or confuse authentication; exceed a limit; inject through document text; escape the
> trusted directory; replay or forge a `plan_id`; exhaust the blocking pool.

Plus the quarter pass 1 could not test (a `tracing` line, because nothing below the server
emits `tracing`) and the items the four verifies handed over.

## Headline

**No attack reached a secret byte, forged an approval, ran an unapproved plan, or escaped the
trusted directory.** Authentication held over TCP against every shape a client library will
not send. The two real defects are both *availability*, not confidentiality: one tool could
be made to allocate about 390 MB from 128 KiB of bounded input, and a single hung provider
read could take the whole MCP surface down while `/healthz` went on reporting the server
healthy. Both are fixed. Beside them, three of the journal's own words were wrong — the audit
trail said "invalid credential" when no credential was wrong, named a planning-error kind
that does not exist, and could not say who asked for a plan — and all three are corrected
additively, with a frozen pre-change journal proving every earlier line still replays.

## The wire-format debts, paid first

Every change below is additive under the rule this pass works to: a new variant, or a field
that is `Option` with `#[serde(default)]`. Before touching the types, a journal written at the
pre-change HEAD and exercising every event kind, every `ApplyRefusedReason`, every
`DriftReasonKind`, both `Outcome`s, both transports and every `AuthFailedReason` was committed
as `crates/willikins-journal/tests/fixtures/pre-pass-2-every-event.jsonl`. It is frozen; the
tests beside it only read it, and they replay it through both `FileJournal::open` and the
lock-free `willikins_journal::replay` after every change in this pass.

**Amended — completeness critic, 2026-09-15.** That fixture is the *backward* direction
only, and it cannot be anything else: a journal written before the change has no
`invalid_nonce`, no `foreign_origin`, no `malformed_username`, no
`recorded_input_unreadable`, no `apply_preparing` and no `principal` on a `plan_recorded`
line, because none of them existed when it was written. Nothing in the tree pinned the
*spelling* of a single new variant, so a later pass that renamed `invalid_nonce` or
dropped `input` from `recorded_input_unreadable` would have orphaned every journal this
milestone's binary wrote, with no test to say so.
`crates/willikins-journal/tests/fixtures/post-pass-2-new-shapes.jsonl` and
`tests/post_pass_2_shapes.rs` (6 tests plus its own `#[ignore]`d generator) are that pin,
and they are the *next* pass's pre-change baseline. Two facts the pass's own claim did not
cover, now covered there: `RecordedInputUnreadable` is frozen in **both** shapes (`input:
Some("slug")` and `input: null`, the case where the whole recorded payload is unreadable
and naming an input would be a lie), and `NodeStatus` — which the claim never mentions —
is exercised in the pre-change fixture only as `created` and `failed`, so the four it
misses (`computed`, `unchanged`, `converged`, `not_run`) are frozen in the new file.

**The direction that does not work, recorded as a decision.** `Event` carries
`#[serde(deny_unknown_fields)]`, so a journal written by a *newer* binary is a replay error
for an older one. Rolling a deployment back past a wire-format change needs a fresh journal
file, not the one the newer image wrote. The new `principal` field is written only when
present (`skip_serializing_if`), so a plan with no requester is still byte-identical to what
the pre-change binary wrote — but a new `AuthFailedReason` variant on a line is not, and
neither is a new `ApplyRefusedReason`. This is the first time the project has had a
forward-incompatible change to defend; it is cheap to defend today (one volume, one file) and
worth a `journal_version` field before there are two deployments sharing a journal.

### 1. A plan did not record who asked for it

`Event::PlanRecorded` carried no principal, so `Butler::requested_by` was a
process-lifetime `HashMap` and the approvals page printed `requester: unknown` for every plan
recorded before a restart. Approval is human-paced by design — a 24-hour window — so a
redeploy between `plan` and a decision is the normal case, and "unknown" was the normal
answer. `PlanRecorded` now carries `principal: Option<PrincipalId>`, `PlanRecord` folds it as
`requested_by`, and the in-memory map is gone. Pinned over TCP by
`the_approvals_page_names_the_requester_after_a_restart`, which plans against one child
process, kills it, starts a second over the same journal, and reads the page.

### 2. The journal said "invalid credential" when no credential was wrong

`AuthFailedReason` had three variants, and task 10b had to map four distinct refusals onto
`InvalidCredential`: a missing, reused, expired or cross-plan nonce; an `Origin`/`Referer`
naming a host the deployment does not allow; and HTTP Basic credentials whose password was
the approver's but whose username was not a usable principal. Every one of those is a line
that should make an operator reach for a *different* response, and `InvalidCredential` is the
line that makes them rotate a credential. Three variants added — `InvalidNonce`,
`ForeignOrigin`, `MalformedUsername` — and each call site records the one that is true.
`InvalidCredential` again means only what it says.

### 3. `PlanFailed { error_kind: "Unavailable" }` named a kind that does not exist

`error_kind` is documented as "the `PlanError`'s own internally tagged `kind`, a serde variant
name". `Unavailable` is not a `PlanError` variant, so an operator who went looking for it in
the error list found nothing. The case it stood for — a value the plan recorded for one of its
own workflow inputs no longer parses — is now
`ApplyRefusedReason::RecordedInputUnreadable { input: Option<InputName> }`: nothing planned,
and the fault is in the record, not the provider. `input` is an `Option` because the other way
that check can fail is the whole recorded `inputs` payload being unreadable, which names no
single input, and inventing one would be a lie in the audit trail.

### 4. Decision: any agent principal may apply any recorded plan

Asked to decide and pin. **Allowed**, and `Butler::apply`'s doc carries the reasoning:

- What a plan *is* is fixed at `plan` time — workflow name, document SHA-256, resolved
  inputs, per-node actions, class — and `apply` re-verifies every one of them against the
  journal and against current provider state. Nothing in that set depends on the caller, so a
  second agent token applying another agent's plan runs exactly what the first agent planned
  and what an approver, for a plan above the threshold, actually saw.
- Refusing would invent a principal class the trust boundaries do not have. The plan names
  three principals with one *agent* role and says the agent "may call every MCP tool".
- Agent principals are *derived* (`agent-<12 hex of the token hash>`), so rotating an agent
  token silently changes the principal. A requester check would strand every plan recorded
  before a rotation, with no way to apply it and no way to say why.

Both names are on the record — `plan_recorded.principal` and `run_started.principal` — which
is what an operator actually needs. Pinned over TCP with two configured agent tokens by
`a_second_agent_token_may_apply_the_first_agents_plan_and_both_are_journaled`. If a later
milestone wants per-agent ownership it is an authorization feature with its own policy (who
may take over a stale plan, and how), not a line in `apply`.

### 5. Decision: a mismatched nonce must not burn the other plan's nonce

Task 10b's `NonceStore::consume` removed the entry whether or not the presented value
matched, so `POST /approvals/{B}` carrying plan A's nonce burned B's — the approver's own
pending decision stopped working because of a request that never proved it knew anything.
**Changed to compare-then-remove.** What burning bought: nothing. The nonce is 32 bytes from
the system CSPRNG, so there is no guessing attack to slow down, and the case where burning
would matter — a forged cross-site POST — never reaches the nonce check at all, because the
`Origin`/`Referer` check refuses it first. What it cost was real: the approval page is the
only out-of-band decision channel this milestone has, and one stray or stale same-origin POST
could deny it. Single-use is unchanged: a nonce that matches is removed, and replaying it
fails. Pinned in `nonce.rs`'s own tests, and over TCP by
`a_nonce_posted_against_another_plan_burns_neither`.

## Goal: inject through document text

### 6. `template.render` amplified 128 KiB of bounded input into about 390 MB

The one real finding of this goal, and the first place to look precisely because template text
is document-authored. The engine itself is a single `str::replace` of the literal
`{{ value }}` — no includes, no environment lookups, no file-reading filters, no recursion, no
second pass over its own output — and tests now pin each of those non-features
(`no_other_template_syntax_is_honoured`, `a_substituted_value_is_never_rendered_again`). What
was not bounded was the *product* of two bounded inputs. `TemplateSource` and `Text` are each
capped at 65,536 characters and the placeholder is 11 characters, so a document may carry
about 5,900 placeholders and each may expand to a full-length `Text`:

```
5_957 placeholders x 65_536 characters = ~390 MB
```

`str::replace` built that whole string and only then handed it to `Text::parse`, which
refused it for being over the bound. 128 KiB in, a third of a gigabyte of resident memory out,
per call, on a host with 11 GB shared between everything — and reachable from `plan`, whose
template comes from a document and whose value comes from a caller-supplied workflow input.
Fixed by arithmetic: the rendered length is computed from the placeholder count before
anything is allocated, and an over-long render is refused naming the two lengths and neither
text. Pinned by
`a_template_that_would_amplify_past_the_bound_is_refused_without_allocating` and by a test
that keeps the projected bound and `Text`'s own `max_len` equal.

### 7. The DSL closes the injection vector before the server sees it

The classic log-forging payload — a newline and a plausible JSON object — and the
terminal-painting payload — an ANSI escape — are both refused *at parse*: a `description`
carrying any control character is a `DocumentErrorKind::Semantic` naming the character, on
every surface. That is stronger than this pass expected to find, and it means the remaining
question is only about text the DSL does accept. A description made entirely of JSON
punctuation (`","level":"ERROR","message":"forged …`) was planted in a document's own
description and in an input's, driven through `validate`, `describe`, `plan` and
`list_workflows`, and swept for: it reaches the agent under `document_description`, it never
reaches willikins' own `prompt` for that input (trust boundary 4), and it never reaches
stderr at all — because `TraceLayer::new_for_http()`'s defaults record the method, the path,
the status and the latency and nothing else. Every line the real binary wrote to stderr in
that test is still one well-formed JSON object.

The approvals page's own escaping is unchanged and stays pinned where task 10b put it
(`workflows/fixtures/hostile-description-pending.yaml` and `hostile-markup-pending.yaml`,
driven through a real `Butler::plan` and a real render): every string the page writes goes
through `escape_html`, and document-authored text carries the `document says:` label the CLI
uses. This pass added no new attack there because the existing pair already covers the two
shapes that matter -- an instruction-shaped description and markup that tries to close the
element it sits in -- and neither got through.

## Goal: bypass or confuse authentication

Every attack below ran over TCP against the real binary. **None got through.**

| Attack | Outcome |
| --- | --- |
| No `Authorization` header | 401 with `WWW-Authenticate: Bearer realm="willikins"` |
| The approver's password as a bearer token | 403, journaled `wrong_role` |
| `Authorization` repeated: valid then forged | the *first* value decides — authenticates |
| `Authorization` repeated: forged then valid | 401. A proxy that appends cannot smuggle a token in ahead of the client's |
| A token differing only in case, by a leading space, by one character, or `-` for `_` | 401 each |
| A token with *trailing* whitespace | authenticates — HTTP strips a field value's surrounding whitespace before any handler sees it. Measured, recorded, left alone |
| `Authorization: bearer <token>` (lower-case scheme) | 401. RFC 6750 makes the scheme case-insensitive, so this is an interoperability defect, not a security one — the failure is closed. Recorded; a one-line fix when a client needs it |
| `HTTP/1.0` | same 401 |
| A chunked body | same 401: chunking is not a way around the bearer check |
| A foreign `Host` on `/mcp`, unauthenticated | 401, not 403 — authentication is the outer gate, so rmcp's DNS-rebinding check never becomes an oracle for whether a token is valid |
| A foreign `Host` on `/mcp`, authenticated | refused. *Precision, completeness critic 2026-09-15:* the test asserts only that the status is **not 200**, so "refused by rmcp's `allowed_hosts`" is the mechanism inferred, not the mechanism measured — a 401 would satisfy the assertion equally. Left as written rather than tightened to a status this pass did not record |
| `/healthz` with and without a port in `Host`, and with `localhost` | 200, no credential needed |
| An approver password with the username `agent-0123456789ab`, `not a principal`, or empty | 403 each, journaled `malformed_username` |
| A cross-site `POST /approvals/{id}` with a valid nonce | 403 on the `Origin`, journaled `foreign_origin`, and the approver's own nonce survives it |

## Goal: exceed a limit

| Attack | Outcome |
| --- | --- |
| A `/mcp` body of exactly 1 MiB | accepted (200) |
| One byte more | 413, from rmcp's own streaming limit |
| A 1 MiB `POST /approvals` form | 413, from the configured `DefaultBodyLimit` — task 10b's verify had found this falling back to axum's 2 MiB default, and it is still fixed |
| A request that announces a body and never sends it | dropped inside the 30-second request timeout. `#[ignore]`d (it waits out the real timeout); **re-measured by hand 2026-09-15 by the completeness critic: green** |
| Headers that never terminate, trickled | **not bounded — finding 8 below.** `#[ignore]`d; **re-measured by hand 2026-09-15 by the completeness critic: green** (both ignored tests, 43 s together) |
| A tool that never returns, times four | **exhausts the blocking pool — finding 9 below** |

### 8. Slowloris: a trickled header stream is bounded by nothing

The 30-second `tower-http` timeout bounds the *service call*, and a service call does not
begin until hyper has read a complete request head. A client that sends a request line, a
`Host`, and then one more header every couple of seconds forever never starts that clock: the
test holds a connection well past the request timeout and the server neither answers nor
closes it.

**Pinned, not fixed**, and the reasoning is in the test. The fix is hyper's own
`http1::Builder::header_read_timeout`, which `axum::serve` does not expose — reaching it means
replacing `axum::serve` with a hand-written accept loop over
`hyper_util::server::conn::auto::Builder` plus its own graceful shutdown and connection
accounting, which is a transport rewrite rather than a line. What bounds the cost today is the
deployment the plan actually describes: no public domain until milestone 3, so the only
reachable listener is behind Railway's proxy on the private network, and each held connection
costs a socket and a buffer rather than a thread. Handed to milestone 3, where a public
listener and this fix belong together. The test is `#[ignore]`d (it holds a connection for
about 40 seconds) and is written so that it fails if the header read is ever bounded — at
which point it becomes the assertion that it is.

## Goal: exhaust the blocking pool

### 9. One hung provider read took the whole MCP surface down, while `/healthz` said "ok"

The plan's own risk section names this shape ("a synchronous core under an asynchronous
server") and leaves it as "if the blocking pool is ever the bottleneck". It is, and a
test-only tool that blocks on a condition variable makes it deterministic rather than a
story. With a runtime whose blocking pool is four threads, four `plan` calls against that tool
fill it, and the fifth request — a *different* principal's — never completes, because
`spawn_blocking` queues once the pool is full. `/healthz` answers 200 throughout, because it
uses no blocking thread at all: **a deployment's own health check reports a server that
answers nothing as healthy.** Production's pool is tokio's default 512. That is the same shape at a
different scale and was **not** measured by hand here: 512 stuck threads on an 11 GB host
shared with other sessions is a measurement that costs more than it tells, since the shape is
already established at four and the only new fact would be the resident cost of 512 stacks
(tokio's blocking threads take the platform default stack, 2 MiB on this target, committed
lazily). What the scale does change is latency rather than kind: with the bound in place the
refusal arrives immediately, and without it the wait is unbounded either way.

Two fixes, because there were two ways in.

**The single-apply lock was held across the pre-run checks.** `Butler::apply` took the
`Mutex<Option<RunId>>` and kept it across reloading the document, rebuilding the recorded
inputs, re-planning against every provider, and the drift comparison — so a provider that
stopped answering blocked every later `apply` on that mutex, *inside its own blocking thread*,
one thread per caller, until the pool was gone. `Butler::run_in_progress` locks the same
mutex and is polled from async code during graceful shutdown, so it blocked a runtime worker
too. The slot is now a three-state value (`Idle`, `Preparing`, `Running(run_id)`) that the
mutex is held only long enough to read and change; an `ApplyGuard` returns it to `Idle` on
every path out of `apply`, including a panic, unless a run has actually started. A second
`apply` arriving during the first one's checks is refused at once with the new
`ApplyPreparing` — its own reason, because there is no run id to name and there may never be
one. Task 11's verify flagged "a request that times out inside apply's drift re-plan must not
leave the single-apply lock held forever" as unproven; it is now proven, by a test that starts
a second `apply` while the first is blocked inside its re-plan and requires it to answer.

**One existing test changed meaning, deliberately.** Task 10a's
`two_applies_racing_on_two_threads_leave_exactly_one_run` required the loser of a race to be
refused with `RunInProgress` naming the winner's run. A loser that now arrives while the
winner is still *preparing* gets `ApplyPreparing` instead -- immediately, rather than after
blocking on the mutex until `RunStarted` is journaled. Which of the two arrives is a
scheduling detail; both mean "someone else is applying, retry", both are journaled, and the
invariant the test is actually about (exactly one run) is unchanged and still asserted. The
test now accepts either and says why.

**And a bound on the surface itself.** 64 concurrent tool calls, far below the 512-thread
pool, enforced with a `try_acquire` (never an `acquire`: waiting for a permit rebuilds the
queue one level up) on the five tools that can block on the filesystem or a provider —
`validate`, `describe`, `plan`, `apply`, `list_workflows`. The next call is refused at once
with a kind-tagged `Busy` an agent can retry. `run_status`, `list_tools` and `propose_slug`
stay unbounded on purpose: they are in-memory reads, and `run_status` in particular is how a
caller learns what happened to a run — refusing it while the bound is reached would hide the
state of the very calls that reached it. The approvals page is unbounded for the same reason
at a higher stake: the approver's channel must not be refused because an agent is looping.

**One more way the bound could have been defeated, attacked and held — completeness
critic, 2026-09-15.** The bound counts *permits*, and it takes its permit in an **async
frame**: `run_bounded` holds it across `spawn_blocking(f).await` and drops it after.
Nothing cancels a `spawn_blocking` closure, so if that frame were dropped while its thread
was still wedged — which is exactly what the 30-second `tower-http` timeout does to a
service call — the permit would come back while the thread did not, and a caller who let
every call time out could accumulate wedged threads up to the pool's 512 while the bound
went on reading "none in flight" and `/healthz` went on answering 200.

It does not happen. Measured in
`blocking_pool_13.rs::an_abandoned_tool_call_keeps_its_permit_until_its_blocking_thread_ends`:
with one permit, a wedged call, and that call's request future abandoned, the next call is
still refused `Busy` and never reaches the tool. The reason is rmcp's own structure —
`streamable_http_server/tower.rs` runs the stateless handler on a task of its own
(`tokio::spawn(async move { service.waiting().await })`), so the permit does not live on
the axum request future the timeout drops. The test is checked by mutation: releasing the
permit *before* `run_blocking` instead of after makes it fail in five seconds.

**What that test does not reach**, named rather than implied: the same rmcp path arms a
`CancellationToken` drop-guard for a client that disconnects *before the handler emits its
first message*, and that token does cancel the handler future. A disconnect inside that
window would drop the permit with the frame while the thread ran on. Driving it needs a
real half-closed hyper connection against a wedged tool, which the in-process harness
cannot make. The hardening is one line — move the permit into the blocking closure,
`run_blocking(move || { let _permit = permit; f() })` — which costs nothing and stops the
property depending on rmcp's internal task structure. **Not made here**, because a source
change whose failing test cannot be written is exactly the discipline this pass is
measuring; handed to milestone 3.

**The bound is not configurable**, and that is recorded rather than hidden: a deployment
variable here would need a startup refusal, a README row and an image test, for a number whose
only job is to stay well under a pool size this process does not configure either. A follow-up
if a deployment ever needs it.

## Goal: replay or forge a `plan_id`

| Attack | Outcome |
| --- | --- |
| A `UUIDv7` minted with the current timestamp, shaped exactly like one this server mints | `UnknownPlan`, journaled. A plan is what the journal recorded, not what an id looks like |
| A `plan_id` copied out of another server's journal | `UnknownPlan`. Nothing about an id carries authority across journals |
| A plan applied, then applied again after a restart | `AlreadyApplied`, journaled. `PlanRecord::applied` is folded from the journal's own `RunStarted` rather than held in memory, so the single-apply rule survives the process — which is the shape that matters, since a 24-hour approval window guarantees the process will be replaced inside it. Proven over TCP through two child processes against one journal file |
| A second agent token applying the first agent's plan | **allowed, by decision** — see finding 4 |

## Goal: escape the trusted directory

Every spelling below was refused, and all of them at *parameter parsing*, before the trusted
directory was touched at all: `../new-rust-service`, `..%2fnew-rust-service`,
`%2e%2e%2fnew-rust-service`, a name with a NUL, a name with a NUL and `.yaml`, full-width
look-alikes for `.` and `/`, a name using U+2010 HYPHEN instead of `-`, `NEW-RUST-SERVICE`,
`New-Rust-Service`, `/etc/passwd`, and `new-rust-service.yaml` (the file name rather than the
workflow name). `WorkflowName`'s grammar — `^[a-z0-9]([a-z0-9-]*[a-z0-9])?$`, at most 64
characters — is what refuses them, and the honest spelling still plans in the same test, so
the refusals are the grammar's doing and not a broken directory.

**Both readings of the case question — one of them was not pinned.** *Corrected,
completeness critic, 2026-09-15.* The test that carried this claim,
`this_hosts_filesystem_case_sensitivity_is_recorded`, measured the fold, `println!`ed it
into output a green `cargo test` run discards, and then asserted only that the file it had
just written existed. It could not have failed for any reason this section is about, and
it recorded nothing durable. It is now
`an_upper_case_name_cannot_reach_the_filesystem_whatever_this_host_folds`, which asserts
the invariant that actually makes the difference unobservable and holds on both hosts:
`WorkflowName::parse("Case-Probe")` is an error and `parse("case-probe")` is not, so an
upper-case spelling never reaches a path at all. The fold itself stays printed, not
asserted — asserting either answer would fail on the other host, which is the whole point.
The claim below is otherwise unchanged and correct.

This host's filesystem is case-insensitive and
Railway's is not, so `New-Rust-Service.yaml` would find `new-rust-service.yaml` here and not
there. The grammar makes that difference unobservable, because an upper-case name never
reaches the filesystem: the request is refused identically on both, and the test that measures
this host's own case folding exists so a future change that started resolving names *through*
the filesystem inherits a recorded fact rather than a surprise.

**A `WILLIKINS_WORKFLOWS_DIR` that is itself a symlink is followed**, and the server starts
and serves what is behind it. Recorded, not changed, and the asymmetry is deliberate: the
refusal that exists is about an *entry inside* the trusted directory, which is content a
document's author could introduce, while the directory itself is named by an environment
variable only the operator sets, on the host that holds the two provider credentials. Refusing
a symlinked path there would break the ordinary deployment shapes — a mounted
volume, a `current -> release-N` layout — to defend against someone who already
chooses the value. Pinned so the next reader finds a decision rather than an oversight.

Two more, both about *when* a file appears rather than what it is called:

- **A symlinked document placed after startup** is refused, not followed: `plan` collapses it
  to `UnknownWorkflow`, exactly as if the directory held nothing by that name.
- **A document whose `name:` does not match its stem, dropped in after startup, denies
  `list_workflows` to every principal** — `scan_directory` refuses the whole scan at the first
  bad entry, naming the file, rather than skipping it. Recorded, not changed: the trusted
  directory is a checkout of a trusted ref that only the operator writes, so this is an
  operator-level footgun and not an attack surface, and "refuse the listing, naming the file"
  is the same fail-closed rule startup itself follows — which is what you want when the
  alternative is silently listing a directory that is not what the operator thinks it is. The
  cost is on the record all the same, and the test pins that `plan` of an unrelated, valid
  document still works.

## The tracing quarter pass 1 could not test

### 10. INFO is the ceiling, and `RUST_LOG` is not read

`cmd_serve_http` builds its subscriber as
`tracing_subscriber::fmt().json().with_writer(stderr).try_init()` — the *builder's*
`try_init`, which, unlike the module-level `fmt::try_init`, installs no `EnvFilter` — and the
workspace does not enable tracing-subscriber's `env-filter` feature at all
(`Cargo.toml`: `features = ["json"]`). So the level is the `fmt` default, INFO, whatever the
environment says. Measured against the real binary with `RUST_LOG=trace`: the startup line
appears, and not one DEBUG or TRACE line does.

Two consequences, both recorded:

- **An operator cannot raise verbosity in production without a code change.** That is a real
  limitation and belongs in milestone 3's exposure work beside the rest of the observability
  question.
- **The sweep below is a sweep of INFO only**, which is weaker evidence than it looks.
  `TraceLayer::new_for_http()`'s own defaults put its request and response events at DEBUG, so
  the real binary emits almost nothing: a 401 does not even produce a line. What that sweep
  *can* prove is that nothing leaks at the level this deployment actually runs at; it cannot
  prove that nothing would leak if someone raised it. The specific thing to look at when
  someone does is rmcp's own `debug!`/`trace!` of received JSON-RPC bodies, which would carry
  document text and tool inputs the moment verbosity went up.

### 11. The sweep itself

Against the real binary, one session: `initialize`, `plan`, `apply`, `run_status` polled to a
final state (so the fake Doppler service token is actually minted), a pending plan,
`GET /approvals`, a nonce round trip that really decides, and a 401. Then every byte the
process wrote to **stderr and stdout** was searched for the agent bearer token, the approver
password, the base64 Basic credential, the nonce it had just issued, the literal `Bearer `,
and `dp.st.` — the prefix every fake-minted Doppler service token carries. **None of them
appears.** The journal file the same run wrote was searched for the same six and is equally
clean.

The nonce matters as much as the token here: a nonce in a log line is a replay for anyone who
can read logs, which on a hosted deployment is everyone with access to the project.

**Amended — completeness critic, 2026-09-15.** The sweep had no *liveness* assertion: it
read the child's stderr and stdout and asserted six needles were absent, and a capture that
came back empty — a broken drain thread, a tracing writer redirected, a child that died
before it logged — would have satisfied every one of them without looking at a single line
the binary wrote. The same was true of finding 7's document-text sweep, whose
"every line is one well-formed JSON object" loop iterates over `stderr.lines()` and checks
nothing when there are none. Both now assert that the capture carries the
`willikins-server listening` line first. The sweep itself was checked by mutation:
`tracing::info!(presented = %token, …)` injected into `bearer_auth` makes it fail, naming
the token, in 4 s — so the capture is live today and the assertion is what keeps it so.

**The limitation, next to the claim rather than three sections away:** this is a sweep of what
the binary emits, and the binary emits INFO (finding 10). **rmcp's own `debug!`/`trace!` of
received JSON-RPC bodies was not swept**, because there is no way to make this binary emit
them — and those are exactly the lines that would carry document text and tool
inputs. Whoever adds a way to raise the level must sweep them in the same change; it is the
first item on the log-level hand-off below.

### 12. The listening line named the address that was *asked for*

Found by the harness rather than by an attack, and fixed: `serve_http` logged `config.bind`,
so a server started on port 0 — the OS-assigned-port convention every test harness and every
"just give me a free port" invocation uses — announced `:0`, which is not an address anything
can connect to. Its own doc claims to be "the only place that ever names the literal address
bound". It now logs `listener.local_addr()`, which makes that true. On Railway the bind is
`0.0.0.0:$PORT` and the line was already accurate, which is why nothing had caught it.

## The items the four verifies handed over

| Item | Outcome |
| --- | --- |
| `AuthFailedReason` variants for nonce and origin refusals (10b) | done — finding 2 |
| `PlanRecorded.principal` (10b) | done — finding 1 |
| `PlanFailed { error_kind: "Unavailable" }` (10b) | done — finding 3 |
| A nonce posted against the wrong plan burns that plan's nonce (10b) | decided and changed — finding 5 |
| Any agent token may apply another agent's plan (10b) | decided, allowed, pinned — finding 4 |
| The origin parser is not IPv6-bracket-aware (10b) | **recorded, not fixed.** `host_matches` strips a port with `rsplit_once(':')`, so an `Origin` of `https://[::1]:8080` compares `[::1]` against an allowed host written `::1` and does not match. Unreachable in this deployment: the allowed-hosts list holds Railway's private domain and loopback names, and a browser reaching a bracketed IPv6 literal origin is not a configuration this milestone has. The fix is three lines (strip brackets before the port split) and belongs with the exposure work that makes an IPv6 origin possible |
| The CLI's unbounded `wait_for_run` (11) | fixed — finding 13 |
| `DocumentErrorKind`'s `message` colliding with `Reported`'s (11) | **swept, and no collision reachable** — finding 14 |
| The cached journal fold: every view folds the whole entry list and `apply` folds twice (10a) | **recorded, not fixed** — finding 15 |
| A hand-edited `fingerprint` line replays as truth (10a) | recorded — finding 16 |
| The truncated last journal line: refuse versus truncate-and-warn (12) | decided: keep refusing — finding 17 |
| The tracing quarter (pass 1) | done — findings 10 and 11 |
| A tool that lies about its own output types (pass 1) | recorded — finding 16 |
| No `catch_unwind` around observers (pass 1) | recorded — finding 16 |
| The text renderer's forged redaction marker (pass 1) | recorded — finding 16 |
| **rmcp round-trips for the four remaining apply refusal kinds are pinned by type identity only (11)** | **missed by this pass entirely.** *Completeness critic, 2026-09-15:* attacked, held, now pinned. See "What the pass did not attack" below |
| **The crash-recovery detour — volume detach and reattach — is untested (12)** | **missed by this pass entirely.** *Completeness critic, 2026-09-15:* attacked without Railway, held, now pinned. See "What the pass did not attack" below |

### 13. A journal that stops accepting leaves a run `Running` for ever

Injected rather than simulated, and the injection is the point: making the journal's
*directory* read-only proves nothing, because the file descriptor `FileJournal` already holds
keeps writing through a `chmod` on macOS and Linux both. The fault has to be introduced where
the appends happen, so the test wraps the `Journal` in one that accepts a fixed number of
appends and refuses every one after. A run's terminal state is folded from `RunFinished`, and
`JournalObserver` *stashes* an append failure rather than panicking — by design, since a run's
outcome is not less true for the journal having trouble recording it — so a journal that stops
accepting mid-run (a read-only or full volume, which on a hosted deployment is a detached or
exhausted disk) leaves the record reading `Running` for good.

What is not broken: the single-apply slot is released either way, so the server keeps
accepting applies, and `run_in_progress()` returning `None` while the record still says
`Running` is exactly the signal that tells "still going" from "died unrecorded".

The CLI's `wait_for_run` was polling that record forever — task 11's verify flagged it and
could not prove it. It now stops when the run thread is gone, re-reads once (the append may
have landed between the two reads), and returns the record as it stands. A run still reading
`Running` exits 1, which is the honest answer: willikins does not know how it ended.

**What is pinned and what is not:** the `Butler`-level facts are pinned by the injection test
(the record stays `Running`, `run_in_progress()` is `None`, the slot is released). The CLI's
own branch is not, because `willikins apply` builds its own `FileJournal` inside
`default_butler_config` and there is no seam to inject a failing one through. Adding that seam
is a CLI change with its own design question, so the branch ships reasoned rather than tested,
and that is the honest state of it.

### 14. No error any MCP surface emits carries a duplicate key

`Reported`'s `serde(flatten)` writes *both* keys when the wrapped type declares one of its
own, and every JSON reader then folds them back to one silently, with no rule saying which —
so an agent can be handed a `message` that is not the message willikins meant. Task 11's
verify found four of these and renamed the fields; its own helper looked at the top level
only, and the collision this pass was sent after (`DocumentErrorKind`'s `message`) is one
level down.

A quote- and escape-aware scan over the raw response bytes, at *every* nesting level, was run
over ten provoked errors across the whole MCP surface: malformed YAML, a semantic document
error, an over-large document, an unknown workflow, a rejected input, a missing input, an
unknown plan, an unknown run, an invalid project name, and a document that fails `check`.
**None carries a duplicate key at any level.** `DocumentError` is never itself wrapped in
`Reported`: it reaches a caller as the *value* of `ButlerError::Document`'s `error` field, so
its own flattened `message` sits one level below the one `Reported` adds and the two never
collide. The collision is real in the type system and unreachable on this surface; the scan is
the guard that says so, and it is the right shape to catch the next one.

### 15. The cached journal fold: recorded, not fixed

Every view (`plan`, `runs`, `run`, `pending_approvals`) folds the whole entry list, and
`apply` folds twice per call. Task 10a handed this over as a performance item. It stays open,
deliberately: a cache is a second source of truth for the one structure this system treats as
the truth, and introducing it during an adversarial pass — whose whole job is to check that
the recorded state and the acted-on state agree — would be the wrong order of work. The
number that decides it is entries-per-journal, and no deployment has produced one yet. What
this pass can add is that the fold is *not* on any hot path a hostile caller controls: the
rate limiter bounds `plan` at 10 a minute and the read tools at 60, and the new concurrency
bound caps how many folds can be in flight at once, so an attacker cannot turn an O(n) fold
into an availability problem faster than the journal itself grows. Milestone 3, with a
measurement first.

### 16. Hardening items, recorded with their reasoning

- **A hand-edited `fingerprint` line replays as truth.** Unchanged, and correctly so: there is
  no hash chain, by decision (a keyless chain is not tamper evidence), and someone who can
  edit the journal file is the operator or has the operator's host. Operator-level.
- **A tool that lies about its own output types is not caught** by `fill_outputs`, in `plan`
  or in `apply`. Every tool in the workspace is our own code. The fix — validating each
  returned value against its declared port type — is a `willikins-core` change with its own
  error variant, which is not this pass's to make unilaterally. Still a hardening item;
  milestone 3.
- **No `catch_unwind` around observers.** A panicking observer still unwinds out of the run,
  which pass 1 pinned as a decision rather than an accident: a journal that cannot record is a
  reason to stop writing to providers. The server's run thread *does* catch it and journals a
  `RunFinished` for the run, so the process survives, which is what changed since pass 1.
- **The text renderer cannot tell a forged redaction marker from a real one.** A document may
  put the literal marker string in a description or a default, and the CLI's text output will
  show it exactly where a real redaction would appear. It cannot manufacture a secret — the
  marker is what a secret *looks like*, not what one is — so the worst case is a reader
  misled about which of two non-secret values was redacted. A distinguishable rendering (a
  marker that carries the port name, say) is a renderer change with its own decisions about
  what an agent is allowed to learn; recorded for milestone 3.

### 17. The truncated last journal line: keep refusing

`FileJournal::open` refuses a journal whose last line is truncated, naming the line and the
reason, and `todos/2026-09-15-journal-repair-subcommand.md` already proposes a `journal
repair` subcommand as the operator's path out. Task 12 asked this pass to weigh
truncate-and-warn against refuse, and to keep refusing unless the experiment showed a worse
failure. It does not.

The experiment this pass *could* run is finding 13's neighbour: what a journal failure looks
like from the outside. Refusing at startup is loud, immediate, and names the file and the
line; the alternative — silently dropping the last line and warning — would start a server
whose journal is missing a record that something else may already have acted on, and the one
record most likely to be truncated is the most recent, which is the one a restart is most
likely to be deciding against (a `RunStarted` with no `RunFinished`, say). Truncate-and-warn
trades a refusal an operator cannot miss for a warning in a log stream this binary emits at
INFO and nobody is tailing. Keep refusing; keep the repair subcommand as the way out, and give
it priority when a production journal actually needs it.

## Decisions made in this pass

1. **Any agent principal may apply any recorded plan** (finding 4). The plan is fixed at plan
   time and re-verified at apply time; the requester is audit, not authorization.
2. **A mismatched nonce burns nothing** (finding 5). Compare, then remove.
3. **The concurrency bound is 64, hardcoded** (finding 9), with the reasoning for not making
   it a deployment variable in the constant's own doc.
4. **`run_status`, `list_tools` and `propose_slug` are not bounded** (finding 9): a caller
   must always be able to learn what happened to a run.
5. **Slowloris is pinned, not fixed** (finding 8), because the fix is a transport rewrite and
   the deployment has no public listener.
6. **The truncated last line keeps refusing** (finding 17).
7. **A trailing-whitespace bearer token authenticates** (goal 1), because that is HTTP's own
   field-value rule and second-guessing the transport is worse than the (already refused)
   way an operator could get bitten.

*Added by the completeness critic, 2026-09-15:*

8. **The permit stays in the async frame** (finding 9's amendment). The property holds
   today because of how rmcp schedules the handler; the one-line hardening that would stop
   it depending on that goes to milestone 3, rather than shipping a source change whose
   failing test cannot be written.

## Plan defects found

1. **The `willikins-server` section's `ApplyRefused` reason list is short by two.** It now
   needs `RecordedInputUnreadable` and `ApplyPreparing` beside the eight task 10a added. The
   plan's own list has been amended once already (task 10a's addendum); this is the second
   amendment and suggests the list belongs in the crate's doc, with the plan naming the rule
   rather than the members.
2. **"`tracing` output carries request method, path *template*, status, and duration"**
   (trust boundary 5) is stricter than what ships. `TraceLayer::new_for_http()` records the
   *concrete* path, not a template, and it records it at DEBUG — which the binary never
   emits. Neither is a leak (a path carries no document text; `WorkflowName`s never appear in
   a URL on this transport), but the boundary's wording describes a filter nothing
   implements, and a reader checking compliance would go looking for it.
3. **The plan says nothing about the log level or about how an operator raises it.** The
   configuration list has eleven `WILLIKINS_*` variables and no way to see more than INFO.
   Milestone 3's exposure work should decide this together with the OAuth transport, not
   after it.
4. **The "Risks" entry for the blocking pool ("if the blocking pool is ever the bottleneck,
   the fix is an async `Tool`") understates the failure mode.** The pool is not merely a
   throughput bottleneck: once it is full, *unrelated principals'* calls queue behind stuck
   ones and `/healthz` keeps answering, so the deployment's own health check cannot see it.
   The risk entry should say that, and name the concurrency bound as the interim answer.
5. **Acceptance test 19's second half asks to "exhaust the blocking pool" but the plan gives
   no bound to exhaust.** A reader cannot tell whether exhausting it is expected to be
   possible. It now is bounded, and the plan should say so.

## Handed to milestone 3 (or 2c, the OAuth milestone)

The exposure items belong together, because every one of them is a consequence of the same
decision — no public domain until authentication is stronger than a static bearer token — and
every one of them changes when that decision does:

1. **Bound the header read.** Slowloris (finding 8) is unfixable without replacing
   `axum::serve`; do it when there is a public listener to protect.
2. **Make the origin check IPv6-aware.** Three lines, and pointless until an origin can be a
   bracketed literal.
3. **Decide the log level story.** An operator cannot raise verbosity today (finding 10), and
   raising it is exactly what someone will do during the first incident. Whatever answer
   milestone 3 picks must come with a sweep of rmcp's own `debug!`/`trace!` of JSON-RPC
   bodies, which this pass could not sweep because the level cannot be raised.
4. **Revisit the concurrency bound with a real measurement** — and make it configurable only
   if a deployment needs it (finding 9).
5. **Validate a tool's returned values against its declared port types**, and decide what a
   distinguishable redaction marker should say (finding 16).
6. **Measure the journal fold before caching it** (finding 15).
7. **A `journal_version` field** before two deployments can share a journal file: a journal
   written by a newer binary cannot be replayed by an older one, and today the only defence is
   that there is one volume and one image.
8. **`willikins journal repair`** (`todos/2026-09-15-journal-repair-subcommand.md`), which is
   now the recorded answer to a truncated last line rather than an idea (finding 17).

*Added by the completeness critic, 2026-09-15:*

9. **Move the concurrency permit into the blocking closure.** One line, no behaviour
   change, and it stops the bound's correctness depending on rmcp running the stateless
   handler on its own task (finding 9's amendment). The path that would defeat it —
   rmcp's disconnect `CancellationToken`, armed only until the handler's first message —
   needs a real half-closed hyper connection against a wedged tool to drive, which is a
   harness milestone 3 should build alongside the public listener.
10. **Tighten the authenticated foreign-`Host` assertion** to the status rmcp actually
    returns, so the auth table's mechanism is measured rather than inferred.

## Completeness critic, 2026-09-15

A second reader, sent after this pass with one question per section: what did it not attack,
which of its tests would pass with the defence removed, which claim is stronger than its
test, which fix has no failing-test-first history, and does the frozen fixture really cover
what it says. **No source file changed.** Everything below is evidence, not behaviour: the
pass's fixes are sound, and what was thin was the proof.

### What the pass did not attack

Two items the four addenda handed over are absent from the "items the four verifies handed
over" table entirely — not deferred, not decided, simply not picked up. Both were attacked
here, and both held.

**Task 11: "rmcp round-trips for the four remaining apply refusal kinds are pinned by type
identity only."** A `matches!` on the Rust enum says nothing about the JSON that crosses
the transport, and this pass then *added two more kinds* to the same list without
round-tripping either. Three refusals are provokable from outside the process and are now
driven through the real binary and read back as the `kind` string an agent branches on:
`ApprovalRequired`, `DocumentChanged`
(`the_apply_refusals_an_agent_can_provoke_name_their_kind_over_the_wire`) and
`RecordedInputUnreadable`, which needs a restart because the refusal only exists in a
process that did not record the inputs
(`a_recorded_input_that_no_longer_parses_names_the_input_over_the_wire`, which also asserts
the refusal names `slug` rather than inventing a planning error — finding 3's whole point,
previously pinned only as a Rust type). The five that are not reachable from outside are
named in the test rather than quietly skipped: `PlanExpired` needs the clock, `Drift` and
`PlanFailed` need a provider that changes its answer, `ApplyPreparing` and `RunInProgress`
need two applies in flight.

**Task 12: "the crash-recovery detour (volume detach and reattach) is untested."** Railway
is out of bounds, but the shape is not: what a detached volume leaves behind is a journal
whose last record is a `RunStarted` with no `RunFinished`. Reproduced deterministically —
plan against one child, stop it, append the orphan `RunStarted` by hand, start a second
child over the same file — in
`a_run_that_never_finished_recovers_without_wedging_the_server`. Four things hold: the
server starts (an unfinished run is a *valid* journal, not a truncated one, so finding 17's
refusal is a different fault); the run reads `running` for ever, which is finding 13's
state reached by the other road; the crashed plan is **`AlreadyApplied`**, because
`PlanRecord::applied` folds from `RunStarted` and not from a finished run — the opposite
answer would make "kill the process mid-run" a way to apply an irreversible plan twice;
and an unrelated plan still applies, so a crash does not wedge the server.

Acceptance test 19's six classes were all attacked by the pass. Nothing there is missing.

### The vacuous test

`this_hosts_filesystem_case_sensitivity_is_recorded` measured this host's case folding,
`println!`ed the answer into output a green run discards, and then asserted only that the
file it had just written existed. It could not fail for any reason the section around it
was about. The note's claim "both readings of the case question, pinned" was true of one
reading. Rewritten as
`an_upper_case_name_cannot_reach_the_filesystem_whatever_this_host_folds`, which asserts
the invariant that holds on both hosts — `WorkflowName::parse("Case-Probe")` is an error,
`parse("case-probe")` is not — and leaves the fold printed rather than asserted, because
asserting either answer would fail on the other host.

Two more tests were *latently* vacuous rather than vacuous: the secret sweep (finding 11)
and the document-text sweep (finding 7) both read the child's captured stderr and assert
that things are **absent** from it. An empty capture satisfies every such assertion. Both
now assert the capture carries the `willikins-server listening` line before they sweep it.

### The test-harness defect

`blocking_pool_13.rs`'s two runtime-building tests opened their gate at the *end* of the
`block_on` body, so any failing assertion before that line unwound past it and left the
blocking threads wedged — and `tokio::runtime::Runtime`'s own `Drop` waits for every
blocking task to finish. A regression in the concurrency bound therefore hung the suite
instead of failing it, which is how the mutation below was discovered rather than a thing
the mutation was looking for. An `OpenOnDrop` guard, declared after the runtime so it drops
before it, turns the hang into a five-second `FAILED`.

### The mutation table

Six mutations, each applied to a green tree, the named test run crate-scoped, then the tree
restored from `HEAD` and `git diff` confirmed empty.

| # | Mutation | Test | Result | Conclusion |
| --- | --- | --- | --- | --- |
| 1 | `NonceStore::consume` reverted to remove-before-compare (`356cdd3^`) | `a_wrong_nonce_is_refused_and_leaves_the_real_one_usable`, `a_nonce_presented_against_another_plan_burns_neither` | both **FAILED** | finding 5 is really pinned |
| 2 | `try_acquire_owned` → `acquire_owned().await` | `the_concurrency_bound_answers_busy_instead_of_queueing_on_a_full_pool` | **hung** (> 60 s, killed); after the `OpenOnDrop` fix, **FAILED in 5.1 s** | the bound is pinned; the *harness* was not, and now is |
| 3 | `butler.rs` reverted to before the apply-guard fix (`294c97a^`) | `a_second_apply_during_the_first_ones_pre_run_checks_is_refused_not_blocked` | **FAILED in 10.2 s**, at "the second apply blocked behind the first" | finding 9's first half is really pinned |
| 4 | the `projected_length(...)?` call deleted from `TemplateRender::compute` | `a_template_that_would_amplify_past_the_bound_is_refused_without_allocating` | **FAILED** | the assertion distinguishes the arithmetic refusal from `Text::parse`'s own, which was the vacuity risk |
| 5 | the frozen fixture's `plan_failed` line rewritten as `already_applied` (seq preserved, so replay still succeeds) | `every_apply_refused_reason_in_the_frozen_fixture_still_deserializes` | **FAILED**, naming `plan_failed` | the test checks the *set*, not mere presence. (Deleting the line outright fails all nine tests on the sequence check, which is why the mutation had to be this precise) |
| 6 | `tracing::info!(presented = %token, …)` injected into `bearer_auth` | `no_secret_token_password_or_nonce_reaches_stderr_or_stdout` | **FAILED in 4.4 s**, naming the token | the headline sweep is live: the capture works and the assertion fires |

A seventh, on the critic's own new test — releasing the concurrency permit *before*
`run_blocking` instead of after — makes
`an_abandoned_tool_call_keeps_its_permit_until_its_blocking_thread_ends` fail in five
seconds, so that test is not vacuous either.

### Claims stronger than their tests

1. **"Both readings of the case question, pinned"** — corrected above.
2. **The authenticated foreign-`Host` row** says "refused by rmcp's `allowed_hosts`". The
   test asserts only that the status is **not 200**; a 401 would satisfy it. The mechanism
   is inferred, not measured. Left as written rather than tightened to a status this pass
   did not record; handed to milestone 3.
3. **The two `#[ignore]`d rows in the "exceed a limit" table** read as measurements. They
   are, but nothing runs them: re-measured by hand on 2026-09-15, both green, 43 s
   together. Recorded in the table so the next reader knows when it was last true.
4. **"At most 64 concurrent MCP tool calls may hold a blocking thread"** is stronger than
   the mechanism that delivers it: the permit lives in an async frame and survives only
   because rmcp runs the handler on its own task. Attacked, held, pinned, and the one path
   that could still defeat it is named in finding 9's amendment.
5. **The pre-change fixture's coverage claim** is accurate for the enums it names and silent
   about `NodeStatus`, which it exercises as two of six. Closed by the new fixture.

### Failing-test-first history

There is none in git for any fix in this pass, and that is structural rather than sloppy:
the repo's convention is one *behaviour* per commit, so every fix landed together with its
test and no commit is ever red. The mutation table above is therefore the actual
failing-test-first record — it is the only evidence that any of these tests would have
failed before its fix. Three commits are worth naming separately:

- **`a45552f`** (`wait_for_run`) contains 26 inserted lines in `commands.rs` and **no test
  at all**. The note says so, and it remains the one fix in this pass with no pin of any
  kind.
- **`294c97a`** (the apply guard) landed with only a *modified* `adversarial_10a` test,
  widened to accept either refusal. Its actual pin,
  `a_second_apply_during_the_first_ones_pre_run_checks_is_refused_not_blocked`, arrived in
  `fba9bc0`, one commit later. Mutation 3 confirms it now holds.
- **`f6eef32`** (the three authentication-failure reasons) landed with a serialization-shape
  test. The tests proving that each *call site* records the true reason arrived in
  `11e6787`, five commits later.

### The frozen fixture

It does exercise every `Event` kind (12 of 12), every pre-change `ApplyRefusedReason` (8),
every `DriftReasonKind` (3), both `Outcome`s, both `Transport`s and every pre-change
`AuthFailedReason` (3) — the claim holds. It cannot exercise a single *new* variant, and
nothing else did either; `post-pass-2-new-shapes.jsonl` closes that, and covers the four
`NodeStatus` kinds the pre-change file misses. See the amendment under "The wire-format
debts, paid first".

## Commits

- `49a352f` — freeze a journal written before this pass and prove it still replays.
- `f6eef32` — the three wire-format debts: the requester, the real authentication-failure
  reasons, the real refusal reason for an unreadable recorded input. One commit because all
  three change `Event`, which `git commit --only` cannot split.
- `356cdd3` — a wrong nonce no longer burns the right one (findings 5).
- `9afb38f` — `template.render` refuses an over-long render before building it (finding 6).
- `294c97a` — the single-apply lock is no longer held across `apply`'s pre-run checks
  (finding 9, first half), with `ApplyPreparing` as its own refusal.
- `fba9bc0` — at most 64 concurrent tool calls, `Busy` rather than a queue (finding 9, second
  half); carries the listening-line fix (finding 12) because it is the same file.
- `a45552f` — `wait_for_run` stops when the run thread is gone (finding 13).
- `11e6787` — the 31 attacks against the real binary, and what held.
- this note, with `todos/2026-09-12-error-json-uniformity-gaps.md` amended by finding 14.

*Completeness critic, 2026-09-15 — six further commits, no source file among them:*

- a `blocking_pool_13.rs` gate guard, so a wedged pool fails the test rather than hanging it.
- `an_abandoned_tool_call_keeps_its_permit_until_its_blocking_thread_ends`: the permit
  attack, which held.
- the case-sensitivity test rewritten from a `println!` into an assertion, and a liveness
  assertion under each of the two stderr sweeps.
- the two handed-over items the pass never attacked: crash recovery, and the apply refusals
  an agent can provoke, over the wire.
- `post-pass-2-new-shapes.jsonl` and `post_pass_2_shapes.rs`: the new variants frozen, and
  the next pass's baseline.
- this note's amendments.

## Gate discipline, disclosed

The four gates ran green before every commit. Two mechanical mistakes are on the record rather
than hidden: an early gate run was started while an unfinished test file was still in the tree
(fmt failed on it, the file was moved out, and that run was discarded and re-run from the
start), and during one clippy phase a source file was edited and restored within the same run,
so that run's clippy result for `willikins-server` covers a tree that existed for about two
minutes rather than the one that was committed. The tree that *was* committed had clippy run
over it cleanly in the runs that followed. A third run was aborted in its clippy phase on a
single leftover binding (`let described = ...`, unused after a test was rewritten) and had to
be restarted from the start rather than resumed. The lesson, for whoever inherits this: on a
host where a full gate is forty minutes, run the crate-scoped clippy over *every* crate the
change touches — including the ones whose tests merely `match` on a type you
extended — before starting the full one.

**Completeness critic, 2026-09-15 — its own gate structure, disclosed.** Every change it
made is strictly additive (new tests, a new fixture, a guard used only by tests, and this
note), and it changed no file under any `src/`. Rather than pay a forty-minute gate six
times for six independent additions, it ran the four gates **once on the union** — the
finished tree — and then committed the additions in an order where each commit is
self-contained, so every intermediate tree is a prefix of a gated one and differs from it
only by additions that are absent. The one ordering constraint is real and was respected:
the gate guard must precede the permit test that uses it. The gates were then run again on
the committed tree. This is a weaker discipline than a gate per commit and is recorded as
such rather than described as the same thing. Following the lesson above, `cargo fmt` and a
crate-scoped run of every touched test file came first, and the full clippy caught three
lint failures in the new tests before the test gate was started.
