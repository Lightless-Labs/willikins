# Milestone 3d: adversarial pass over total conversions, and a read-only live plan

**Date:** 2026-09-23 to 2026-09-24
**Task:** the verifier's pass over `docs/plans/2026-09-23-milestone-3d-conversions.md`
**Subject:** C1 to C3 as landed (`553b231`, `79b7090`, `076efbb`), the uncommitted C4 draft, and
everything C5 to C7 still had to build.
**Method:** read the plan and every landed file, then attack through tests. Every claim below is a
run, not a reading. Mutations were made by editing the source, running the named test target, and
restoring from a copy saved before the edit (never `git checkout`, never `git reset`); after each
restore the files were byte-compared with the saved copies (`cmp`) and the tree's diff was the
pre-mutation diff. The live plan resolved the Apple credential out of the sandbox Doppler workplace
inside the process that used it and printed counts, statuses and booleans only.

## 0. The state the pass started from

C1 to C3 were committed and green. C4 was drafted and uncommitted, and had never compiled end to end.
C5, C6 and C7 did not exist. The pass finished C4 and built C5 to C7 before attacking, because a
guarantee can only be attacked on a tree that has it.

What the C4 draft still got wrong, none of which its own report caught:

- **It did not compile.** `crates/willikins-dsl/tests/acceptance.rs` formatted each
  `Checked::types` entry with `{}`, and `Edge` has no `Display`. The report's "passed willikins-dsl"
  was cargo's `Checking` line for the library, not the test target. The characterization now renders
  `edge.ty()` and appends ` -> <to>` only for a converted edge, as equivalence item 1 specifies; its
  snapshot was unchanged at C4 and C5.
- **One of its tests failed.** `a_literal_records_an_edge_with_no_conversion` bound a literal of a
  synthetic type. Literals parse through the *global* registry (the plan's own re-verified fact), so
  it failed with `unknown type ChainB`. Rewritten against the production row: the literal
  `com.example.MyApp` on an `AppleProfileName` port records an exact edge even though the same text
  is a valid `AppleBundleIdentifier`.
- **Its `clippy.toml` entry broke clippy in another crate.** `willikins-types`' own unit tests of
  `probe_conversion` (landed in C2, before the entry existed) had no `#[allow]`. The first full gate
  failed there; each test now opts in with a scoped allow and a reason.
- **The design-doc addendum the plan assigns to C4 was missing.** Added.

## 1. What was attacked, and what held

### Secrecy only goes up, at compile time

`conversions!` emits `const _: () = assert!(!A::IS_SECRET || B::IS_SECRET, …)` per row.
`secret_to_public.rs` fails with exactly E0080 and the secrecy message; `missing_from_impl.rs`
fails with exactly E0277 naming `From<A>`. `Probe => ProbeSecret` (public to secret) compiles and
the converted object renders `[REDACTED ProbeSecret]`.

**Mutation:** delete the `const _` from the macro. trybuild reports `secret_to_public.rs`:
*"Expected test case to fail to compile, but it succeeded."* So the test is not vacuous, and this
also settles verify item 1(b): the inline `const { assert!(…) }` belt inside the generic
`__private::conversion` is **not** evaluated by `cargo check`, which is trybuild's mode. The macro's
`const _` is the only check-time guarantee; the belt fires only when a direct call is compiled to
code, and clippy refuses a direct call first (verify item 3 below).

**Can a hand-written type bypass it?** The assertion reads `DomainType::IS_SECRET`, and that const
**defaults to `false`** in the trait. A hand-written secret type that forgot to set it would make a
secret-to-public row compile, and `check`'s secret rule, which reads the same flag through the
registry, would miss it as well. Every hand-written `DomainType` impl in the tree was enumerated:
`AppleSigningKey` is the one hand-written secret type and sets `IS_SECRET = true` beside a
`DomainObject` whose `is_secret()` is `true`; every other hand-written type goes through
`impl_domain_object_non_secret!`, which already asserts `!IS_SECRET` at compile time; the derive
emits the const and `is_secret()` from the same branch. They all agree today. **New test,**
`every_registered_type_is_as_secret_as_its_is_secret_const_says` (`registry.rs`): for every
registered type the registry calls public, its example parses, and the object says
`is_secret() == false` and renders plain; every type it calls secret is refused as a literal.
**Mutation:** make `impl_domain_object_non_secret!`'s `is_secret()` return `true`; the test goes
red (with the per-module macro tests).

### One hop, no chains

`probe_conversion` is one `HashMap::get` on the owned `(from, to)` pair. With `ChainA => ChainB` and
`ChainB => ChainC` registered in a test catalogue, `ChainA` bound where `ChainC` is wanted fails with
exactly `TypeMismatch { node: sink, port: c, expected: Exact(ChainC), found: ChainA }` and the Display
``node `sink`, port `c`: expected ChainC, found `ChainA` ``, today's error byte for byte. Both single
hops check, a document that spells the two edges out checks, and `list<ChainA>` into a
`list<ChainB>` port stays `TypeMismatch`.

**Mutation:** give `probe_conversion` a transitive second hop. `a_bound_where_c_is_wanted_is_refused_exactly_as_before`
goes red, and only it.

### Total: every bundle identifier is a profile name, byte for byte

The C3 property tests held, but neither reached the bound. Strategy 1 builds segments of at most 8
characters, so it never produces a long segment; strategy 2 draws arbitrary Unicode strings, which
almost never parse as an identifier, so its implication is close to vacuous. The only 255-character
case was a single 255-`a` segment. **New:** a third strategy over the identifier's own alphabet,
`[A-Za-z0-9.-]{1,300}`, whose candidates straddle 255 and include every near miss (leading, trailing
and doubled separators); whenever one parses as an identifier it is at most 255 characters and
converts to the byte-identical profile name `parse` gives. Plus a boundary case with separators
(`a.` × 127 + `a`, exactly 255, converts; `a-` × 128 + `a`, 257, is not an identifier). Both types
count `chars()`, both store the input verbatim, and the `From` impl copies `as_str()`.

### No second lookup

What is **structural**: `Edge`'s fields are private and its constructors `pub(crate)`, so outside
`willikins-core` an edge only comes from a real `check`. `Conversion::new` is `pub(crate)` to
`willikins-types`, so the only ways to hold a converter are the macro and `probe_conversion`.
`conversion_pairs` returns names only, so the catalogue path cannot hand anyone a converter. `plan`
and `apply` read converters only from `Checked::types`, through `ResolveCtx::deliver`.

What is **not** structural, said plainly: `plan` and `apply` receive `&Catalog`, `Catalog::registry()`
is public, and `probe_conversion` is public. Rust privacy cannot restrict a cross-crate method to one
module of another crate, and the plan says so. The table is kept out of `plan` and `apply` by two
independent enforcements, both proven by mutation rather than read:

- **Clippy.** A call planted in `plan.rs` through `catalog.registry().probe_conversion(…)` (an
  auto-deref'd `&'static TypeRegistry`) fails `-D warnings` with `use of a disallowed method
  willikins_types::registry::TypeRegistry::probe_conversion`. Verify item 2: settled, yes.
- **The tripwire.** A comment naming the method planted in `apply.rs` turns
  `plan_apply_and_describe_never_name_probe_conversion` red. It catches the `#[allow]` a clippy-only
  guard would not.

### Applying the edge: plan and apply both deliver

**Mutation 1:** `ResolveCtx::deliver` returns its argument. Four of the six new `tests/apply.rs`
tests go red (the `Input`, `Step`, `Keyed`/`Item` and secret-to-secret edges); the two that call
`Edge::deliver` directly stay green, as they should. **Mutation 2:** only `apply`'s re-resolution
skips `deliver`. Three go red: the unknown-at-plan `Step` edge (ensure received `ConvA`), the
`Keyed` edge, and the secret edge. So both delivery sites are pinned independently.

### No declassification by any route

- **Rows:** a secret-to-public row does not compile (above).
- **`check`:** `SecretToNonSecretSink` still runs before any type test, and the probe runs only after
  both, so a conversion can only turn a would-be `TypeMismatch` into an accepted edge.
- **Plan, events, journal:** a converted secret renders `[REDACTED SecB]` in the plan JSON and
  `Debug`, in `Applied`, and in every `RecordingObserver` event, and its bytes appear in none of them,
  while the sink still receives them (acceptance 7, synthetic secret types).
- **Errors:** `TypeMismatch` names types only; `Edge` and `Conversion` `Debug` print type names only.
- **The downcast's panic.** `__private::convert`'s `unreachable!` names the two types, never the
  object. More to the point, the pass found it **reachable**: `plan` does not check that a
  caller-supplied workflow input has its declared type, and a forged `Checked` (its fields are
  public) can move a real converted edge onto another port. Either would have handed the converter an
  object of the wrong type and panicked. **Fixed in C5:** `Value::converted` passes a value through
  unchanged when its declared type, or any object in it, is not the conversion's source, so the
  converter is only ever called on a source-typed object. Pinned by
  `a_converted_edge_passes_a_foreign_value_through_unchanged` (a wrong-typed input through `plan`,
  no panic) and `converted_passes_a_value_of_another_type_through_unchanged` (including a list
  declared as the source type holding a foreign object).

### Fact, not convention

`appstore.profile.ensure`, the fake tools and `willikins-tools` are unchanged since the base
(`git diff c991a24 -- crates/willikins-providers-appstore/src crates/willikins-providers-fake/src
crates/willikins-tools/src` is empty), so no tool can know the convention. The predecessor document,
with the profile's name bound to its own input, still checks with an exact edge and plans a profile
named `Any Name The Operator Likes` (`a_profile_name_other_than_the_identifier_still_checks_and_plans`).
The new negative fixture pins the one direction: an `AppleProfileName` bound to `identifier` is
`TypeMismatch`, as before.

**Mutation:** bind the document's `name` to a literal again. Three tests go red: the positive plan
(`PROFILE1` is no longer `Present` at the new key), the edge-carries-the-conversion test, and the
equivalence test.

### Unchanged

- The C1 characterization, committed on the pre-conversions tree, holds every document's check
  result and plan JSON and fingerprint, and every fixture's exact errors. It was unchanged through
  C4 and C5. At C7 it changed in exactly the reviewed ways: `profile.name: AppleProfileName` became
  `profile.name: AppleBundleIdentifier -> AppleProfileName`, and the new fixture's entry appeared.
  The signing document's plan line did not move, because against the default fake state its plan
  stops at the first Doppler read, as it did before.
- `git diff c991a24` over every `*.snap`: the characterization's creation and C7 change, and the MCP
  tool-list snapshot's one `list_tools` description line (verify item 8: yes, it is snapshotted).
  `crates/willikins-journal/` and `naming` untouched. Under `workflows/fixtures/`: the seed and the
  new fixture, nothing else.
- Equivalence item 3: the new document and its verbatim two-input predecessor plan to equal JSON and
  fingerprints and apply to equal event streams, both for the seeded identifier (profile `Present`)
  and for the second seeded identifier (profile created through the converted edge).

## 2. Findings, each now a test

| # | Finding | Now pinned by |
| --- | --- | --- |
| 1 | C4 did not compile: the characterization formatted an `Edge` | the characterization itself |
| 2 | C4's literal test used a synthetic literal, which cannot parse | `a_literal_records_an_edge_with_no_conversion` |
| 3 | C4's clippy entry broke `willikins-types`' own probe tests | the clippy gate |
| 4 | The converter's downcast was reachable through a wrong-typed input or a forged `Checked` | `a_converted_edge_passes_a_foreign_value_through_unchanged`, `converted_passes_a_value_of_another_type_through_unchanged` |
| 5 | `IS_SECRET` defaults to `false`; nothing tied it to the object's own secrecy | `every_registered_type_is_as_secret_as_its_is_secret_const_says` |
| 6 | The property tests never reached a long segment or the bound with separators | `over_the_identifier_alphabet_every_identifier_converts_byte_for_byte`, `a_255_character_identifier_with_separators_converts` |

## 3. Mutations

| Mutation | Test that went red |
| --- | --- |
| drop the macro's `const _` secrecy assertion | trybuild `secret_to_public.rs` ("succeeded") |
| `impl_domain_object_non_secret!`'s `is_secret()` → `true` | `every_registered_type_is_as_secret_as_its_is_secret_const_says` |
| `probe_conversion` with a transitive second hop | `a_bound_where_c_is_wanted_is_refused_exactly_as_before` |
| `ResolveCtx::deliver` returns its argument | 4 of the 6 new `tests/apply.rs` conversion tests |
| `apply`'s re-resolution skips `deliver` | the `Step`, `Keyed` and secret-to-secret apply tests |
| the document binds `name` to a literal | the positive plan, the edge test, the equivalence test |
| a `probe_conversion` call in `plan.rs` | clippy `disallowed_methods` |
| the method's name in `apply.rs` | `plan_apply_and_describe_never_name_probe_conversion` |
| the `#[allow]` stripped from the macro's expansion | clippy `disallowed_methods` in `willikins-types` (verify item 3) |

## 4. Live run: `plan --live` of the rewired document, read-only

One script, one process tree. The Doppler token was read from `~/.config/willikins/sandbox.env`
(that one variable only) into the environment of the command that used it; the Apple credential was
read out of the sandbox workplace's `app-store-connect/prd` inside the Python process, used to sign
short-lived tokens for `GET` calls only, and never printed. The one unexpired `DISTRIBUTION`
certificate's serial was read into memory and passed to the CLI as `--input serial_number=…` (so it
sat on the `plan` process's argv for the run, the known CLI gap recorded in milestone 3c), never
printed. The CLI ran with `--json`, and its plan was reduced in-process to the lines below. The
output was scanned by count before it was read: UUID shapes 0, runs of 16 or more hex characters 0,
10-character upper-case identifiers 0, `eyJ` 0, `PRIVATE KEY` 0.

```
BEFORE: certificates=5 profiles=13 bundle_ids=21 throwaway_identifier_present=0
DISTRIBUTION certificates: 1, unexpired: 1
plan --live exit=0 stdout_bytes=10177 stderr_lines=0
node actions: issuer_id_text, issuer_id, key_id_text, key_id, key_base64, key_decoded, key,
              certificate: compute; bundle_id, profile, destination, store: create
class: reversible requires_approval: False
profile.name type: AppleProfileName state: known
profile.identifier type: AppleBundleIdentifier state: known
profile.name equals profile.identifier, byte for byte: True
profile.name equals the identifier input: True
AFTER: certificates=5 profiles=13 bundle_ids=21 throwaway_identifier_present=0
```

The document planned against the operator's live account with **no `profile_name` input**. The
profile node's `name` port received a known `AppleProfileName` that equals, byte for byte, the
`AppleBundleIdentifier` on its `identifier` port, which is the throwaway identifier the run passed
in (`com.willikins.probe.plan3d`, which nothing matches, hence `bundle_id` and `profile` `create`).
Counts before and after are equal, the throwaway identifier is absent before and after, and every
provider call was a `GET`: the plan wrote nothing.


## 5. Host conditions worth knowing

One full gate took between 35 minutes (incremental) and well over two hours on this host. During
the C6 gate a `cargo-sweep --maxsize 15GB` over this workspace, started by another session, deleted
compiled artifacts mid-build (`E0463: can't find crate for willikins_server`, missing `.rcgu.o`
objects at link). The pass waited for the sweep to finish without touching `target/`, and the next
gate rebuilt the workspace from scratch in 129 minutes. That is why C6 and C7 landed as one commit,
the merge the plan itself offers when gate runs are scarce.

Final gate on the tree carrying every code change of this pass (the docs commit adds only this
record and the plan's addenda): `cargo fmt --all --check` clean;
`cargo clippy --workspace --all-targets -j 2 -- -D warnings` clean;
`RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast` **171 suites, 2330 passed, 0
failed, 18 ignored**, with `secret_literal_guard`, `no_gh_writes_guard` and
`no_certificate_writes_guard` among them; `cargo check -p willikins-types -j 2` clean.



## Independent review of the verifier-written commits, 2026-09-24

**Reviewer:** an independent adversarial pass (not the verifier) over `a14ad7f` (C5), `d862203`
(C6+C7), `bc2b286` and `1a22498`, on `main` at `5252b06`.
**Method:** the claims above were reproduced, not read. Every mutation below edited source,
ran the named scoped test target, and restored from a copy saved before the edit; each restore
was checked with `cmp` against the saved copy and `git status --short` showed a clean tree
(except the untracked todo that belongs to another session).

### One defect, fixed

**The converter's downcast was still reachable, and panicked inside `plan`.** Finding 4 of
this record says `Value::converted` passes a foreign value through so "the converter is only
ever called on a source-typed object". It was not. `Value::converted` decided whether an object
was the source by comparing its type **name** (`object.type_name() == from`), and the generated
converter then downcast it by **`TypeId`**, with an `unreachable!` if that failed. A type name
does not determine a `TypeId`: `TYPE_NAME` is `stringify!` of the struct's own name, so any
crate can `#[derive(DomainType)] struct AppleBundleIdentifier(String)`, and `Value::known`
accepts it. A new unit test, written first, went red on `5252b06` with exactly:

```
panicked at crates/willikins-types/src/__private.rs:74:9:
internal error: entered unreachable code: conversion AppleBundleIdentifier -> AppleProfileName: the source is not a AppleBundleIdentifier
```

User-visible failure before the fix: a library caller handing `plan` such a value as a
workflow input on a converted edge got a panic, where before conversions it got the tool's
`port … has an unexpected type` error. Not reachable from the CLI or MCP: both parse every
input against its declared type through the global registry (`describe.rs:281`,
`butler.rs:1244`), so a name there always means the registered type. The panic message named
types only, so nothing leaked.

**Fix (`b470206`):** `__private::convert` returns `None` when its downcast fails, and
`Conversion::apply` returns that `Option`; `Value::converted` passes the value through on
`None`. Whether an object converts is now decided once, by the downcast itself, and no
`unreachable!` remains on the path. Pinned by
`converted_passes_a_same_named_value_of_another_rust_type_through_unchanged` (scalar and list,
`value.rs`), `a_same_named_input_of_another_rust_type_plans_without_a_panic` (through `plan`,
`tests/apply.rs`), and `a_conversion_applied_to_another_type_is_none` (`probe.rs`). The same
commit adds an unknown value of another type to the existing pass-through test: before it,
nothing pinned that an edge never retypes an `Unknown` it does not convert. The trybuild
`.stderr` files are unchanged (`conversion`'s signature did not move) and the suite passes.

### Every reachable route to the pass-through, and what the user sees

`check` only records a conversion on an edge whose binding it resolved to exactly the source
type, and the CLI and MCP parse inputs by declared type, so for a run that goes through either,
the pass-through is unreachable. It is reached only through the library:

| Route | What happens now |
| --- | --- |
| `plan` given a workflow input of another declared type (e.g. a `ConvB` for a `ConvA` input) | the value reaches the tool unconverted; a real tool refuses it with `port … has an unexpected type` (a `PlanError::Tool`) |
| `plan` given a same-named value of another Rust type | the same, since `b470206`; a panic before it |
| a hand-built `Checked` (its fields are `pub`) moving a converted `Edge` onto another port | the value reaches that port's tool unconverted and is refused by type |
| `Value::known_dyn_list` declaring the source type over a foreign object | the whole list passes through unconverted |
| a caller holding a `Conversion` (via `Edge::conversion()`, `probe_conversion`, or `conversions!`) calling `apply` on a foreign object | `None`, since `b470206`; a panic before it |

### Delivery: what held, with evidence

- **No double conversion, but only because a second one is masked.** Mutation M2 made
  `apply`'s `resolve_instance_inputs` deliver **every** port a second time, including the
  `Input`/`Item` ports `plan` had already delivered. All 23 `tests/apply.rs` tests stayed
  green. A second delivery is a no-op because the value is already target-typed and
  `Value::converted` passes it through; nothing *detects* a double delivery. Harmless for any
  row (the pass-through makes delivery idempotent), and said plainly so no one reads the green
  suite as a proof that each value is delivered exactly once. That property rests on reading:
  `plan` delivers each non-literal port once in `bind_ports`; `apply` reuses `planned.inputs`
  for `Literal`/`Input`/`Item` and re-resolves and delivers only `Step`/`Keyed`, from raw
  results; neither delivers `for_each` sources or workflow outputs.
- **List conversion is pinned by one unit test only.** Mutation M1 made `Value::converted`'s
  `Known::List` arm return its argument. Only `converted_covers_every_state` went red; no
  integration test did, because `check` never builds a list edge. That is correct today
  (decision (e)), and it means list delivery through `plan`/`apply` is untested because it is
  unreachable.
- **The real document's equivalence test is not vacuous.** Mutation M3 made
  `ResolveCtx::deliver` return its argument, run against
  `willikins-providers-appstore --test profile_documents`, which the record did not report.
  Two went red: `the_document_is_equivalent_to_its_two_input_predecessor` (`must plan: Tool
  { node: "profile", error: … "port `name` has an unexpected type" }`) and
  `the_doppler_chain_plans_and_produces_a_redacted_profile_content`, the same error. So the
  real `appstore.profile.ensure` does receive an `AppleProfileName` only through the edge.
- **Literals and `AnySecret` never convert.** By construction: `bind_ports`' literal arm parses
  against the port's own type and never calls `deliver`, and `check` records a conversion only
  for a `PortType::Exact` scalar port. Confirmed by reading, not mutated.
- **Unknown-at-plan `Keyed` edges** are not tested separately: the `Keyed` test's upstream is
  pure, so its value is known at plan. The `apply` code path is the one the unknown `Step` test
  already pins (the same `resolve_instance_inputs` branch), so this is noted, not fixed.

### Secrecy: held

- Plan JSON, `Debug`, `Applied`, and `RecordingObserver` events: the record's test
  (`a_converted_secret_is_redacted_as_its_target_everywhere`) passes and does what it says.
- The journal: `NodeStarted.inputs` is `Redacted<Inputs>`, built by `Redacted::from`, which
  serializes through `Value`'s own redacting `Serialize` (`willikins-journal/src/redacted.rs`),
  so there is no second rendering path to leak through.
- Errors: `TypeMismatch` names types only; `Edge`/`Conversion` `Debug` print type names only;
  the removed `unreachable!` named types only.
- The catalogue and MCP `conversions` key is built from `conversion_pairs()` and carries
  `{"from", "to"}` type names and nothing else. With one production row, "declaration order"
  cannot be observed; it is unpinned, not wrong.

### Equivalence and unchanged fixtures: held

- `PREDECESSOR` in `profile_documents.rs` is byte-identical to
  `git show c991a24:workflows/appstore-signing-profile-from-doppler.yaml` (compared by script).
- `git diff --stat c991a24 HEAD` over `workflows/`, every `snapshots/` directory,
  `crates/willikins-journal`, and the appstore, fake and tools crates' `src/` shows only the
  signing document, the new fixture, the seed, the characterization snapshot and the MCP
  tool-list snapshot. The characterization snapshot has two commits (`553b231` created it,
  `d862203` changed it) and exactly the two reviewed hunks. It contains one ` -> `, on
  `profile.name`. No existing negative fixture's error moved.
- The new negative fixture's error is pinned twice: struct equality in
  `appstore_profile_name_into_identifier_is_rejected`, and `Display` plus JSON in the snapshot.
- No stale reference to the old seed key `willikins-demo-profile` remains outside `docs/`.
- The limit of the characterization: the signing document's characterized plan stops at the
  first Doppler read against the default fake state, so it does not cover the converted edge.
  The equivalence test does, and M3 shows it would catch a lost delivery.

### Mutations

| # | Mutation | Result |
| --- | --- | --- |
| M1 | `Value::converted`'s list arm returns its argument | red: `converted_covers_every_state` only |
| M2 | `apply` delivers every port a second time | green, all 23 `tests/apply.rs` (masked, see above) |
| M3 | `ResolveCtx::deliver` returns its argument, appstore suite | red: the equivalence test and the positive plan |
| M4 | the fixed converter's `?` put back as `unreachable!` | red: both new same-named tests, and `converted_passes_a_value_of_another_type_through_unchanged` (its forged `known_dyn_list` now reaches the downcast, which is the only object-level guard left) |

### Not settled

- `Conversion::apply` is public, and the fix changed its return type to `Option`. The only
  callers are in-tree (`value.rs`, `probe.rs`); anything outside the workspace calling it would
  need to handle `None`.
- The full workspace gate was not run by this review, by instruction; the coordinator runs it.
  Scoped runs: `willikins-core` and `willikins-types` `--lib`, `willikins-core --test apply`,
  `willikins-types --test derive_compile_fail`, `willikins-providers-appstore --test
  profile_documents`, `cargo clippy -p willikins-types -p willikins-core --all-targets -- -D
  warnings`, `cargo check -p willikins-types`, and `cargo fmt --all --check`, all clean on
  `b470206`.

## Fail-loudly follow-up, independent review, 2026-09-27

**Reviewer:** an independent adversarial pass. This reviewer did not write the backstop or the
`TypeId` predicate. It covers `0787a28` (`TypeRegistry::type_matches`), the backstop the previous
implementer left uncommitted, and the parse-time input check, which had not been written.
**The operator, 2026-09-24:** "I'd much rather have it fail loudly at parsing than silently go
through."
**Method:** as in the previous review, every claim was reproduced. Each mutation edited source,
ran `willikins-core --test apply`, and restored the file from a copy saved before the edit. `cmp`
confirmed every restore byte-identical. No `git checkout` or `reset` was used.

### State found, and what was finished

- **Backstop (A).** The previous implementer left it uncommitted with its scoped gate green (fmt,
  clippy, `willikins-core` tests). It was committed unchanged as `1e29caf`. `Value::converted`
  and `Edge::deliver` now return `ConversionMismatch` rather than passing a foreign value through.
  `plan`'s `bind_ports` and `apply`'s `Step`/`Keyed` re-delivery surface it as
  `PlanError::EdgeTypeMismatch { site, expected, found }`.
- **Root cause (B), written here** (`4d44fb3`). `plan` now checks every workflow input the caller
  supplied against its declared `TypeRef` before any node is planned, and refuses a mismatch with
  `PlanError::InputTypeMismatch { input, expected, found }`. Three conditions apply:
  - the value's own `TypeRef` must equal the declared one, list flag included;
  - every known object must pass the catalog registry entry's `TypeId` test. That includes each
    element of a list;
  - a name the registry does not hold is refused.

  An `Unknown` value of the declared type is accepted, as before. `apply` inherits the check
  through its opening replan. Two things are unchanged: a declared input that was not supplied
  still reaches `MissingInput` only through a binding, and an undeclared extra key is still
  ignored.

  The two tests whose docs promised the flip now expect `InputTypeMismatch`. The impostor test
  also covers `apply`: no events, no read, no ensure. Four tests are new:
  - a list with one foreign element and a list with one impostor element, with no instance read
    (before the change, the first well-typed item was already read);
  - an exact, unconverted edge;
  - a wrong shape or wrong `Unknown`, plus a correctly typed `Unknown` that still plans;
  - a secret supplied for a public input.

### Defects found

1. **The root cause itself, before (B).** A wrong-typed workflow input on an *exact* edge went
   through silently. Red run on `1e29caf` plus the new tests:
   `plan` returned `Ok` with `inputs: {in: bad}` for a `ConvB` in the `conv.echo` double's `ConvA`
   port. The impostor case was the same (`{in: imp}`). Only a tool that reads through
   `helpers::get` would have refused it. Six tests were red before the check and all 30 in
   `tests/apply.rs` passed after it.
2. **Naming a refused object could panic.** Both `ConversionMismatch` (A) and
   `InputTypeMismatch` (B, as first written) named the offending object through
   `type_name_of_object`, which contains an `unreachable!` if `DomainObject::type_name()` is not
   a valid `TypeName`. Nothing checks that it is. The derive sets `TYPE_NAME` to `stringify!` of
   the struct's name, so `#[allow(non_camel_case_types)] #[derive(DomainType)] struct conv_a`
   compiles and reports `conv_a`. `Value::known_dyn_list` never looks at its items' names. So a
   declared `list<ConvA>` input holding one such object panicked inside `plan`:
   `entered unreachable code: DomainObject::type_name must be a TypeName: TypeName: \`conv_a\` is
   not a valid type name`. The same object panicked inside the public `Value::converted`. Both
   routes were written as tests first and went red with exactly that message. Before (B), the
   `plan` route already panicked in `for_each`'s keying (`Value::known_dyn`). **Fix
   (`483b165`):** the new `reported_type_name_or` names the object, falling back to the declared
   type (for an input) or the conversion's source (for an edge) when the reported name does not
   parse. `type_name_of_object` is private again. Pinned by
   `a_list_element_whose_type_name_is_not_a_type_name_is_refused_not_panicked` (`tests/apply.rs`)
   and `converted_refuses_an_object_whose_type_name_is_not_a_type_name_without_a_panic`
   (`value.rs`).

### Where a wrong-typed value can still go, with evidence

| Route | Result now |
| --- | --- |
| Library caller: a workflow input of another declared type, shape, or `Unknown` type | `InputTypeMismatch`, before any read |
| Library caller: a same-named impostor, scalar or list element | `InputTypeMismatch` (by `TypeId`; `found` prints like `expected`, so `Display` says "a value of another Rust type also named") |
| Library caller: a secret for a public input | `InputTypeMismatch` naming `SecA`; no bytes in `Display`, `Debug`, JSON or `ApplyError`; no apply event |
| Hand-built `Checked` moving a converting edge | `EdgeTypeMismatch` at `plan` and at `apply`'s replan |
| `ensure` returning an impostor that `plan` saw as `Unknown`, on a converting edge | `EdgeTypeMismatch` at apply re-delivery, sink never ensured |
| **A tool's own output of the wrong type, on an exact edge** | **reaches the next tool.** Probe (temporary test, removed, `cmp`-restored): a pure `conv.liar` declaring `out: ConvA` and returning a `ConvB`. Into the untyped echo: `Ok`, `inputs = {in: lie}`. Into a tool reading through `helpers::get`: `node \`down\`: Invalid: port \`in\` has an unexpected type`. Into a converting sink: `down.b: expected ConvA, found \`ConvB\`` |
| A tool's list output holding a misnamed object, as a `for_each` source | still panics in `plan`'s keying (`Value::known_dyn`); not reachable from any in-tree tool |

**Not fixed, by scope:** tool outputs are never checked against the tool's declared output types
(`fill_outputs` clones what the tool gave, and `apply` does the same with `ensure`'s outputs).
Tools are catalog code, not caller input. Every provider tool reads an exact-typed port through `helpers::get` or a
`downcast`, so the result is loud, but it arrives as a `ToolError` from the *receiving*
tool, and at apply time only after earlier ensures have run. Checking outputs where they are
produced is the natural next step. It would change which error a buggy tool produces, so it
belongs to its own change, alongside `todos/2026-09-23-checked-as-a-typed-graph.md`. A hand-written
`DomainObject` whose `as_any` returns another object is consistent with itself: the `TypeId` test,
the converter and the tool all see the same `as_any`. It is noted, not attacked.

### Held

- **A matching name alone never admits an object.** `check_input_types` first compares the
  value's declared `TypeRef` (name and list flag) with the input's, then tests every known object
  by `TypeId`. `type_matches` is the only per-object test there, and `Value::converted` decides by
  the converter's own downcast. M1 (below) shows that a name comparison would let an impostor
  through on an exact edge.
- **Secrecy.** Both new errors carry `InputName`, `Site` and `TypeRef` only. The journal records a
  plan refusal as its `kind` string alone (`willikins-journal/src/observer.rs::plan_error_kind`).
  The server boxes the `PlanError`, whose JSON carries only these fields. The secret test pins
  `Display`, `Debug`, JSON, `ApplyError` and the observer.
- **Unchanged.** `willikins-dsl --test acceptance` passes on `4d44fb3`, including
  `characterization_of_every_document`, with no `.snap.new` written. The characterization committed
  in `553b231` (as C7 amended it) therefore did not move: every negative fixture's error and every
  document's plan and fingerprint are the same. `willikins-providers-appstore --test
  profile_documents` (the converted-edge equivalence) passes. The CLI and MCP parse every input from
  text against its declared type through the global registry, which is the catalog's, so
  `type_matches` is always `Some(true)` for their values and neither new error is reachable from
  them. Their messages cannot have changed. That last point comes from reading `describe.rs`, the
  server's input parsing and the characterization run, not from running the CLI and server suites,
  which this review was told not to run. No downstream crate matches `PlanError` exhaustively, and
  no snapshot enumerates its kinds (searched).
- **Panics.** Apart from defect 2 and the tool-output keying route above, the new code has no
  `unwrap` or `unreachable!`.

### Mutations

| # | Mutation | Result |
| --- | --- | --- |
| M1 | `matches_entry` compares `object.type_name() == T::TYPE_NAME` instead of `TypeId` | red, 3: the scalar impostor and list impostor fall to the backstop's `EdgeTypeMismatch`; the exact-edge impostor **plans**, reaching the echo (`{in: imp}`) |
| M2 | `plan` never calls `check_input_types` | red, 6: every new parse-time test |
| M3 | `Edge::deliver` passes a mismatched value through (`unwrap_or(value)`) | red, 3: the moved-edge test (plans `sink2` with `world` unconverted), the direct-edge test, and the ensure-impostor test (applies both nodes) |
| M4 | `apply`'s `Step`/`Keyed` re-delivery skips `deliver` | red, 4: the ensure impostor, the unknown `Step`, the keyed/item delivery and the converted-secret test |
| M5 | `check_input_types` skips list elements | red, 1: the list-element test |
| M6 | `InputTypeMismatch`'s `expected == found` `Display` case removed | red, 1: the impostor test's message |

Every restore compared byte-identical with `cmp`. `git status` showed only the operator's
`CLAUDE.md` edit and the two untracked files from other sessions afterwards.

### Scope of the checks run

The following ran, each with `-j 2`, alone on the host, with its log read in full:
- `cargo fmt --all --check`;
- `cargo clippy -p willikins-core --all-targets -- -D warnings`;
- `cargo test -p willikins-core`: the whole crate on `4d44fb3`'s tree, and `--lib --test apply
  --test plan_error_serde` for `483b165`;
- `cargo check -p willikins-types`;
- `willikins-dsl --test acceptance`;
- `willikins-providers-appstore --test profile_documents`.

The full workspace gate was not run, by instruction.
