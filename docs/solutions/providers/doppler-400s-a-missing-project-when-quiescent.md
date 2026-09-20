---
title: "Doppler answers 400, not 404, for a missing project when the workplace is quiescent"
category: providers
tags: [doppler, rest, error-handling, not-found, permissions, live-test]
module: willikins-providers-doppler
symptom: "planning a brand-new Doppler project (or config, token, or secret) is refused with `Provider: provider says: This token does not have access to requested project '<name>'`, even though the project genuinely does not exist and creating it directly succeeds"
root_cause: "Doppler's project-scoped GET endpoints answer 404 'Could not find requested project' for an absent project name in the minutes after another project in the workplace was created, but answer 400 'This token does not have access to requested project' for the identical absent name when the workplace has been quiescent, or shortly after a project was deleted; every read in this crate only ever tolerated the 404 shape as 'Absent'"
date: 2026-09-20
---

# Doppler's missing project is sometimes a 400, not a 404

## Symptom

A full rehearsal provisioned a first project end to end, across GitHub, Doppler and
Buildkite, then a second one. The second failed at the planning stage, before touching any
provider for real:

```
planning failed: node `doppler`: Provider: provider says: This token does not have access to
requested project 'harbor-relay'
```

The project `harbor-relay` did not exist. Creating it directly through the API immediately
afterwards succeeded. The milestone 2 smoke run, which planned a brand-new project the same
way, had passed — against the same provider, the same credential shape, the same code.

## Root cause

`doppler.project.ensure::observe` (and every other Doppler read that needs a project to exist
first: `doppler.config.ensure`, `doppler.service_token.ensure`, `doppler.service_token.rotate`,
`doppler.secret.get`) only ever tolerated a `404` as "this project is absent", the fix
`fixtures/doppler/README.md` records for 2026-09-16. Doppler does not answer `404`
deterministically for an absent project. A dedicated probe — six requests per state, the same
full-access sandbox token, one absent project name — found the behaviour is deterministic and
**state-dependent**:

| Workplace state | Status for an absent project name | Message |
| --- | --- | --- |
| In the minutes after another project was **created** | `404` | `Could not find requested project '<name>'` |
| Quiescent, or shortly after a project was **deleted** | `400` | `This token does not have access to requested project '<name>'` |

Same token, same endpoint, same shape of name. The milestone 2 smoke run, and the 2026-09-20
rehearsal's *first* project, both planned successfully, so both met the `404` state; the
rehearsal's *second* project met the `400` state. **Why each of those runs met the state it
met was not recorded.** Nothing captured what the Doppler workplace had done in the minutes
before any of them, and the node that runs first in both documents creates a *GitHub*
repository, which cannot change a Doppler workplace's state at all. The probe's table above is
evidence about the two states the probe itself put the workplace into; it is not evidence
about what put those three runs where they were.

Before this fix, `observe`'s match arm read:

```rust
Err(err) if err.status == Some(404) => Ok(Observation::Absent { .. }),
Err(err) => Err(err.into()),
```

The `400` fell through to the second arm and refused the whole plan — for a workflow whose
entire point, at that node, is to create the project that does not exist yet.

## What this does not settle, on purpose

Neither the status nor this message proves absence. A separate probe on 2026-09-16, with a
service-account token deliberately granted nothing, found the **opposite pairing**: a project
that genuinely *exists* but sits outside the token's grant answered a bare `404` "Could not
find requested project" (recorded in `docs/research/2026-09-12-m2-dependencies.md`,
"Service-account access"). So a `404` was never trustworthy proof of absence either — this fix
does not manufacture that proof, it only stops treating the two statuses inconsistently when
neither one has it.

The reading this crate commits to, and the only one it commits to: **at plan time, both
answers mean "this token cannot see a project by this name right now"**. That is exactly the
information `Observation::Absent` (or, for the token-list endpoint, `false`) already carried
for a `404`; nothing new is being asserted, only applied consistently. The create call is left
as the arbiter of whether that was actually because the project does not exist — the same
deferral `doppler.project.ensure::ensure` already relies on when a create itself fails
ambiguously, by re-reading rather than parsing the error body. For `doppler.secret.get`, which
has no create path, the missing parent still refuses the plan either way; only the `ToolError`
it refuses with changed, from an echoed `Provider` message to a consistent `NotFound` naming
the key.

## Fix

`willikins_providers_doppler::client::looks_like_a_missing_project` is one predicate, shared by
every affected read:

```rust
pub(crate) fn looks_like_a_missing_project(err: &ProviderError) -> bool {
    match err.status {
        Some(404) => true,
        Some(400) => err.message.contains("does not have access to requested project"),
        _ => false,
    }
}
```

It replaces every `err.status == Some(404)` guard that meant "the parent this call needed is
not there yet" in:

- `doppler.project.ensure::observe` — the node the rehearsal actually hit.
- `doppler.config.ensure::observe` — the same ambiguity, one level down: a config's own `GET`
  needs its parent project to exist.
- `doppler.service_token.ensure::is_listed` — widening the 2026-09-16 fix, which had tolerated
  only the `404` shape of the identical "parent not there yet" case and left this `400`
  deliberately unhandled, pinned by a test that has since been re-purposed (below).
- `doppler.service_token.rotate::read` — the same widening, `ensure`'s own listing call left
  strict exactly as it was before (a rotate that cannot list the tokens it is about to revoke
  must not mint a replacement).
- `doppler.secret.get::lookup` — not to widen what it tolerates (there is still no create path
  to defer to, so it still refuses), but so the refusal is the same `NotFound` regardless of
  which shape Doppler answered, rather than leaking the provider's own "does not have access"
  words under `ToolErrorKind::Provider`.

Those five are every read the crate's client exposes (`get_project`, `get_config`,
`list_service_tokens`, `get_secret`). The crate's *tests* were a second home for the same
assumption, missed by the first pass and fixed on the same day: `tests/live_write_cycle.rs`'s
`step_1_absent` panicked on anything but a `404`, so the whole live write cycle could not start
against a quiescent workplace — its commonest state, since it deletes everything it makes; the
same file's `the_cycles_projects_are_gone` leftover check counted a `400` as a leftover needing
a human, which is backwards for the status a *just-deleted* project most likely answers (that
file's own step 10 has recorded since 2026-09-14 that a `GET` straight after the `DELETE`
answers `400`); and `tests/live_probe.rs`'s `check_missing_project_error_shape` recorded a
failure for any status but `404`. All three now ask the same predicate, which is `pub` for that
reason.

The tolerance stays exactly one *message* wide, not "any `400`". Doppler's other documented
`400` — a duplicate `POST /v3/projects`, `"Project name already exists in this workplace."`
(confirmed live 2026-09-14, `fixtures/doppler/README.md`) — must never be read as absence.

Being precise about how much that narrowness is currently doing: **nothing in this crate routes
a create's error through the predicate**, and no call is retried on a `400` at all
(`willikins_providers_http`'s `is_retryable_status` is `429` and `5xx` only), so a wider
predicate could not today produce a retry loop or a silently-swallowed conflict. The narrowness
is defensive, not load-bearing: it exists so the next read that reaches for this predicate, or
the next endpoint that answers a duplicate-shaped `400` to a `GET`, cannot quietly inherit "any
`400` means absent". Widening it was tried as a mutation (`Some(400) => true`) on 2026-09-20:
it fails both client unit tests that pin the stopping point, the four
`read_still_propagates_a_400_with_an_unrelated_message` tests, `service_token_ensure_mock`'s
`read_still_propagates_a_400_from_the_listing`, and three `provider_messages.rs` tests — but
*not* `a_duplicate_create_is_still_distinguished_from_an_absent_project`, which pins that
`ensure` surfaces the create's own error and not that the predicate is narrow. Both directions
are pinned; they are just pinned by different tests than the fix's own commit message implies.

Every mock-test file touched by this fix pins both directions: the no-access `400` reads as
absent (or, for `secret.get`, `NotFound`), and a `400` naming anything else — including the
exact already-exists text — still fails. `project_ensure_mock.rs` additionally pins the create
call itself as the arbiter: a `GET` reading the no-access `400` (so `Absent`), followed by a
`POST` that fails with the already-exists `400`, surfaces the create's own conflict rather than
silently succeeding. Reverting the predicate to `404`-only (`Some(400) => false`) fails exactly
the eight tests the fix added for the tolerant direction, one per affected read path plus the
client's own unit test and the rehearsal-body test — checked by mutation on 2026-09-20, with the
file restored from a saved copy afterwards.

## What to take from it

- **A provider's status code is not a fact about the resource; it can be a fact about how
  recently the provider's own state changed.** The exact same "this project does not exist"
  truth produced two different, non-overlapping HTTP statuses depending on workplace
  quiescence — nothing about the request, the credential, or the resource itself differed.
- **Fixing the production read paths is half the job; the tests that encode the same
  assumption are the other half.** Three assertions in this crate's own live tests still
  demanded a `404`, and one of them would have reported two successfully-deleted projects as
  leftovers for a human to clean up. A guard written around a status is a claim about the
  provider, in exactly the way the code under it is.
- **A live smoke test that only ever runs against a workplace state that just changed will
  never see the state that matters most for a real operator: quiescence.** The milestone 2 and
  3a smoke runs both happened to run in the `404` state; the defect had been live in the
  codebase since project-create was first written, and surfaced only when a second, unhurried
  provisioning run followed the first by more than a few minutes.
- **When a status cannot be trusted to mean one specific thing, name what it is being trusted
  to mean instead**, and keep that as narrow as the evidence allows. This fix does not claim
  "400 means absent" — it claims "400 with this exact message, like a bare 404, means this
  token cannot currently see a project by this name", which is both true and exactly as much
  as the evidence supports, and it leaves a differently-worded `400` (the duplicate-create
  conflict) failing precisely because nothing licenses reading it the same way.
