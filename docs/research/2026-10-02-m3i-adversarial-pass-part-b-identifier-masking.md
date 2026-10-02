---
title: "Milestone 3i adversarial pass, part B: identifiers print as prefixes"
created: 2026-10-02
status: complete
area: types, core, cli, server
related:
  - docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md
---

# Milestone 3i adversarial pass, part B

An independent attack (task X1) on everything task group B landed (B1 through B8): the
`IS_IDENTIFIER` property and `#[domain(identifier)]` (`willikins-derive`, `willikins-types`), the
seven marked types and the registry's disclosure-monotone `conversions!` assertion,
`Value::display`/`mask_json` (`willikins-core`), the CLI's `--reveal` and every text/JSON render
path (`willikins-cli`), the MCP server's `Masked<T>` and the approvals page (`willikins-server`),
the three pre-rendered-string guards and the `parse_recorded_value` error-audit fix (task B7), and
`DopplerValue` (task B8). By a reviewer that wrote none of it. Priority targets, from the plan's
task X1 row: a full identifier reaching CLI text or JSON, MCP, an error or the approvals page
without `--reveal`; masking leaking into `Plan::fingerprint`, `Value`'s `Serialize`, the journal or
recorded inputs; a secret printed under `--reveal`.

At the start, nothing was uncommitted. `cargo test -p willikins-types`, `-p willikins-core`
(disclosure/value modules) were green before any mutation.

## Finding

**None survived.** Four mutations were tried against the priority targets above; all four were
already killed by the existing suite (table below). This pass's value is negative: it confirms, by
actually breaking each rule and watching a specific test go red, that the properties decision
(b3)/(b4)/(b9) claim are load-bearing rather than merely documented. No fixture or fix was added.

## Mutations (each restored from a saved copy, `cmp` byte-identical)

| # | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M-1 | `willikins-core/src/disclosure.rs`'s `mask_known_value` stripped of its registry lookup (`TypeName::parse`/`registry().get(...).info.identifier`), masking **every** known `Value`'s `"value"` field regardless of type — the "mask everything" over-masking direction, priority target "full identifier... without `--reveal`" attacked from the opposite side (does the registry gate actually gate, or would any object slip the same code path through anyway) | killed | `mask_json_leaves_a_non_identifier_value_untouched` (a `GitHubOrg` value was masked and marked, when it must print whole) and `mask_json_leaves_a_secret_value_untouched` (the secret's `"[REDACTED ...]"` marker string was itself re-masked and `"masked": true` added, which would make a secret's wire shape depend on masking at all — trust boundary 7's "secrets are unchanged, with or without `--reveal`") |
| M-2 | `willikins-types/src/disclosure.rs`'s `mask_identifier`: the "never more than half" cap (`IDENTIFIER_PREFIX_CHARS.min(total_chars / 2)`) weakened to `IDENTIFIER_PREFIX_CHARS.min(total_chars)` — a short identifier (2–7 characters) would print more than half of itself, eroding the "enough to recognise, not enough to leak" rule decision (b3) states for exactly this case | killed | `a_two_character_value_keeps_only_one_character`, `a_short_three_character_value_keeps_one_character`, `a_one_character_value_keeps_nothing_but_the_dots` (three independent cases, each pinning a different point on the `total_chars / 2` curve) |
| M-3 | `willikins-core/src/value.rs`'s `Value::display`: the `if disclosure == Disclosure::Revealed { return self.render(); }` early return deleted, so `--reveal` would still mask — the inverse failure direction from every other finding in this milestone (under-revealing rather than over-revealing), but still a real defect: an operator who explicitly asked to see a full identifier would silently get the prefix instead, with no error to notice | killed | `display_revealed_equals_render_for_an_identifier` (`willikins-core`'s own unit test on `Value::display`, independent of any CLI-level test) |
| M-4 | `willikins-types/src/registry.rs`'s `conversions!` macro: the disclosure-monotonicity `const` assertion (decision (b9), "a conversion may not make an identifier plain") deleted entirely, leaving only the pre-existing secrecy assertion | killed | the `tests/conversions/fail/identifier_to_plain.rs` `trybuild` fixture, which is supposed to fail to compile and, under the mutation, compiled successfully — `cargo test -p willikins-types --test derive_compile_fail` reported `1 of 11 tests failed` ("Expected test case to fail to compile, but it succeeded") |

## Checked and accepted (no change)

- **`Value`'s `Serialize`, `Plan::fingerprint`, and the journal's `Redacted<T>` never mask.**
  Read directly rather than inferred from the plan's prose: `Value::serialize` (the `impl
  serde::Serialize for Value` block) calls `object.render()`, never `display()`, for both the
  scalar and list branches; `Plan::fingerprint` (`plan.rs`) calls `value.render()` directly and is
  pinned by its own `fingerprint_of_an_identifier_output_keeps_the_full_value` test, which goes
  further than a bare assertion — it fingerprints two plans differing only in the identifier value
  and asserts the fingerprints differ, so a mutation that masked both to the same prefix would be
  caught even if a reviewer forgot to check the literal string. `willikins-journal`'s
  `Redacted<T>`'s own `JsonSchema` is the permissive `{}` for every `T` (confirmed in the milestone
  3i plan's own task B4 addendum, and by reading `crates/willikins-journal/src/redacted.rs`), so
  `Value`'s schema's new optional `masked` property never even reaches a journal-shaped schema —
  not re-attacked by mutation here since the plan's own addendum already demonstrates it by
  regenerating the snapshots and showing none of `willikins-journal`'s changed.
- **`parse_recorded_value`'s `bad_shape()` masks before formatting (task B7's own fix), and the two
  other spots that build a `ParseError` or `ToolError` from a rejected value do not need the same
  fix**, confirmed by reading rather than re-deriving the audit: `willikins-derive`'s generated
  `checks()` function (every `#[domain(...)]` type's `min_len`/`max_len`/`pattern` validation,
  including all seven identifiers) builds its rejection reason from the violated constraint's name
  alone (e.g. `"must be at least N characters long"`), never the input; `TypeName::parse`/
  `TypeRef::parse` (`registry.rs`) do quote their rejected input through `crate::quoted`, but what
  they reject is a *type name or type reference string* from document YAML, never a domain value.
  `willikins_core::value::Value::parse`/`parse_list` delegate straight to
  `willikins_types::registry().parse(...)` with no added wrapping that could reintroduce a quoted
  value between the derive's clean error and the caller.
- **`Reported<T>`'s added `message` field is `Display`-only (`self.0.to_string()`), and its
  `#[serde(flatten)]` of the inner error's own derived fields is what lets a structured `Value` —
  not a flattened string — reach `domain_error`'s JSON.** This matters because `mask_json` is a
  *structural* walk: it can mask a nested `Value`-shaped JSON object anywhere in a larger document,
  but it cannot find an identifier that has already been formatted into a plain `String` field (that
  is exactly the shape of bug `bad_shape()` had before task B7's fix). Read `ApplyError::Drift`'s
  `DriftKind::Output { port, planned: Value, observed: Value }` specifically, since it is the one
  variant the plan's own error audit flags as "clean in `Display`" but reaching `--json` through
  ordinary `Serialize`: its two `Value` fields are real, nested JSON objects in `Value`'s own wire
  shape once serialized (not pre-rendered strings), so `mask_json`'s structural walk does reach and
  mask them — verified by reading the field types and `DriftKind`'s derive, not only trusting the
  audit's own conclusion.
- **No identifier grammar among the seven admits a `.`** (decision (b3)'s stated reason the
  three-dot suffix never collides with a real value). Verified directly against each type's
  `#[domain(pattern = ...)]` attribute in `willikins-types/src/appstore.rs` and `buildkite.rs`:
  `AppleIssuerId` (`[0-9a-f]{8}-...`, hyphens only), `AppleKeyId` (`[A-Z0-9]{2,32}`),
  `AppleBundleIdId`/`AppleCertificateId`/`AppleProfileId` (`[A-Za-z0-9]{2,64}`),
  `AppleCertificateSerial` (`[0-9A-F]{1,64}`), `BuildkiteClusterId` (a UUID pattern, hyphens only)
  — none contains a literal `.` in its character class.
- **`DomainObject::is_identifier`'s default (`false`) and the `impl_domain_object_non_secret!`
  macro's compile-time assertion against a secret type using it** mean a hand-written type cannot
  accidentally become maskable (or accidentally bypass redaction) by one line of macro misuse — the
  same structural guarantee the plan claims for secrecy is extended to identifier-ness. Read the
  macro's `const _: () = ::std::assert!(!<$ty as DomainType>::IS_SECRET, ...)` directly; not
  re-attacked by mutation, since defeating a `const` assertion inside a macro a caller cannot edit
  without also editing the macro itself is not a realistic bypass surface for this milestone.
- **`missing_input`'s `default: Option<String>` field, hardcoded to `Disclosure::Masked`, is dead
  code by construction**, not an unreveal-able identifier default: `missing_input` is only ever
  built for "a declared input with neither a raw value nor a default" (its own doc, and
  confirmed by reading every call site), so `spec.default.as_ref()` is always `None` inside it and
  the `.map(...)` never runs. The reachable route — a document input that *does* carry a default —
  resolves through `describe`'s `resolved` map instead, which is disclosure-aware end to end
  (confirmed by `crates/willikins-cli/tests/prerendered_identifier_guards.rs`'s own
  `describe_masks_an_identifier_typed_input_default_and_reveal_shows_it_whole`, which shows the
  `--reveal` flag working on exactly this shape). This matches the milestone plan's own task B8
  addendum, which independently reaches the same "dead code" conclusion for the same reason.
- **The sibling tool `buildkite.pipeline.ensure` (milestone 3a, not 3i) shares `MANAGED_DESCRIPTION`'s
  exact-equality ownership check and the identical near-miss test gap** `docs/research/2026-10-02-m3i-adversarial-pass-part-a-bootstrap-writer.md`
  found and fixed in the bootstrap writer — noted there as out of this milestone's task table and
  not duplicated here, since it belongs to part A's own tool, not identifier masking.
- **The MCP server's `list_tools`/catalog JSON cannot false-positive against `is_known_value_shape`.**
  A tool's own `ToolSpec` schema shape (what `list_tools` serializes) has no sibling `"list"`/`"state"`
  keys alongside a port's `"type"` string — confirmed by reading `ToolSpec`'s and its ports' own
  `Serialize` derives, which have no field named `state` at all — so the schema-shaped guard test
  (`mask_json_leaves_a_schema_shaped_object_untouched`) is not merely testing a contrived shape; it
  is testing the actual shape this exact surface produces.

## Verify (not settled by this pass)

- None. Part B calls no provider and needs no live credential; every claim above was settled by
  reading production code and by mutation against the offline suite.
