# Milestone 3d: total conversions between domain types, parsed into the checked graph

**Created:** 2026-09-23
**Addendum:** 2026-09-23 (C2) — verify item 1 settled empirically. A `const _: () = assert!(…)`
nested in block position inside `conversion_rows()`'s `vec![…]` (where `conversions!` emits it)
**is** evaluated by `cargo check`, the same as `object.rs:105`'s item-position precedent: the
`secret_to_public.rs` trybuild fixture fails with exactly E0080 and the secrecy message, nothing
else, on Rust 1.97/trybuild 1.0.121. Decision (b)'s item-position fallback (splitting into an
`conversion_asserts!` invoked at module level) is **not needed** and was not built. The belt
(`__private::conversion`'s inline `const { }`) was left in place regardless, unexercised by any
fixture, per decision (b)'s own reasoning that trybuild cannot prove a monomorphization-gated
check either way. `missing_from_impl.rs` fails with exactly E0277 naming `From<A>`, nothing else.
**Addendum:** 2026-09-24 (verifier) — C4 finished and landed (`6fcb456`) after three fixes its
draft needed (the characterization formatted an `Edge`; a literal test used a synthetic type, which
cannot parse; the probe's new clippy entry broke `willikins-types`' own probe tests); C5 landed
(`a14ad7f`) with one design change: `Value::converted` passes through a value whose type is not the
conversion's source, because the converter's downcast was reachable through a wrong-typed input to
`plan` or a forged `Checked`. C6 and C7 landed as one commit (`d862203`), the merge "Budget" offers,
after a host-wide `cargo-sweep` forced a 129-minute rebuild mid-gate. Two verifier tests added
(`bc2b286`, `1a22498`). Verify items 1 to 8 answered and the post-flight run: see "Verify list,
answered" and "Post-flight" below, and `docs/research/2026-09-23-m3d-adversarial-pass.md`.
**Addendum:** 2026-09-24 (independent review) — decision (f)'s pass-through compared type
*names*, while the converter downcast by `TypeId`, so a same-named value of another Rust type
still reached the downcast and panicked inside `plan`. The converter now returns `None` on a
failed downcast, `Conversion::apply` returns `Option`, and `Value::converted` passes through on
`None` (`b470206`). See "Independent review of the verifier-written commits" in
`docs/research/2026-09-23-m3d-adversarial-pass.md`.
**Addendum:** 2026-09-27 (fail-loudly follow-up) — the operator, 2026-09-24: "I'd much rather have
it fail loudly at parsing than silently go through." Two changes replace the pass-through that
`b470206` left in decision (f). **Root cause, at parsing:** `plan` now checks every workflow input
the caller supplied against its declared `TypeRef` before any node is planned, and refuses a
mismatch with the new `PlanError::InputTypeMismatch { input, expected, found }`. The value's own
`TypeRef` must equal the declared one (list flag included), and every known object in it, each list
element included, must pass the registry entry's `TypeId` test (`TypeRegistry::type_matches`,
`0787a28`). A matching name alone never admits an object: the declared `TypeRef` is compared
first, then every known object by `TypeId`. `apply` inherits the check through its opening replan.
Before this, a wrong-typed input on an *exact* edge reached the tool unchecked. **Backstop:**
`Value::converted` and `Edge::deliver` return `ConversionMismatch` instead of passing a
foreign value through, and `plan`'s `bind_ports` and `apply`'s `Step`/`Keyed` re-delivery surface
it as `PlanError::EdgeTypeMismatch { site, expected, found }` (`1e29caf`). The parse-time check
makes this unreachable for any `Checked` that `check` built and any workflow input. It still
catches two routes: a hand-built `Checked` that moves a converting edge onto another port, and a
tool whose `ensure` returns a value of another type that `plan` only saw as `Unknown`. Both errors
carry type names only. The CLI and MCP parse inputs from text by declared type, so neither error
is reachable from them, and their messages and the characterization snapshot are unchanged. See
"Fail-loudly follow-up, independent review" in `docs/research/2026-09-23-m3d-adversarial-pass.md`.
That review also removed a panic from naming a refused object whose `TYPE_NAME` is not a valid
`TypeName` (`483b165`). It left one gap open: a tool's own wrong-typed output is not checked where
it is produced.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (type system: "parse, don't validate",
"a secret output may only flow to a secret-accepting input. No coercion."; "policy lives in the
workflow, never in the tool"; the workflow-inputs rule "no secret input types")
**Depends on:** milestone 3c (`docs/plans/2026-09-22-milestone-3c-app-store-signing.md`, Completed),
whose `appstore.profile.ensure` and `workflows/appstore-signing-profile-from-doppler.yaml` are the
motivating case.
**Follow-up:** `todos/2026-09-23-checked-as-a-typed-graph.md`. This milestone's edge record is its
first brick.

## Goal

A tool port of type `B` accepts a binding whose resolved type is `A` when a total conversion from
`A` to `B` is registered, the way a Rust function taking `impl Into<B>` accepts an `A`. The
conversion is something `check` **constructs into the checked graph**, and `plan` and `apply`
**apply** it from that graph without ever looking it up again.

The motivating case: `appstore.profile.ensure`'s `name` port is `AppleProfileName`. The operator
names every profile after its bundle identifier. Today the signing document has to take the same
string twice, as two inputs (`identifier` and `profile_name`). That duplication is how the
operator's live account came to hold an App ID with a typo in it. After this milestone the
document takes the identifier once and binds the profile's name to it.

The milestone is done when every acceptance test below passes, every existing negative fixture's
errors and every existing document's plan are byte-identical to before, and the signing document
plans to exactly the plan its two-input predecessor produced when both inputs carried the same
string.

## The operator's constraints (normative)

The operator's words, 2026-09-23. They are not paraphrased away anywhere below.

1. **"Secrecy only goes up."** A conversion's target is at least as secret as its source. This is
   a **compile-time** guarantee, not a runtime rule: a registered secret-to-public conversion does
   not compile.
2. **"One hop, no chains."** Resolution is one probe of a `(from, to)` map. There is **no
   transitive search**: if `A` converts to `B` and `B` converts to `C`, an `A` bound where a `C`
   is wanted is refused exactly as it is today.
3. **"This conversion is a fact, not my convention."** `AppleBundleIdentifier` converts to
   `AppleProfileName` because every bundle identifier is a valid profile name. That is a property
   of the two grammars, not a naming policy. The operator's convention lives only in their document,
   which chooses to bind the profile's name to the identifier. Another document stays free to pass
   any other name, and nothing in a tool assumes or enforces the convention.

And the principle for how: **parse, don't validate.** The operator also wrote: *"there's a world
between [making workflows Rust code] and making an actual parser that doesn't rely on post-parsing
checks."* So the conversion is not something a later pass re-verifies. `check` produces it, and
`plan` and `apply` consume it. If `plan` or `apply` looked the conversion up again, that would be
validating twice.

## Out of scope

- **Rewriting `Checked` into a typed graph.** That is `todos/2026-09-23-checked-as-a-typed-graph.md`.
  This milestone must not extend the pattern it replaces: no new `unreachable!`, `expect` or
  `unwrap` in `plan.rs`, `apply.rs` or `describe.rs`, and no new `pub` field anyone can forge.
- **Transitive conversion, conversion paths, or a closure computed when the table is built.**
- **Cardinality lifting** (`list<A>` into a `list<B>` port). See decision (e).
- **Any conversion row other than `AppleBundleIdentifier` to `AppleProfileName`.**
- **Any public-to-secret row.** The type rule admits one; the reasons none is registered are in
  decision (b).
- **Changes to `appstore.profile.ensure`.** Decision (d) picks implicit acceptance, so the tool
  is unchanged byte for byte, including its `ToolSpec` snapshot.
- `naming::v1`, the journal wire format, `Value`'s JSON shape, `Plan::fingerprint`'s definition,
  every `CheckError` variant and its `Display`. No Railway command, and no provider write, live or
  sandbox.

## Facts re-verified by the planner, 2026-09-23

The coordinator's brief was re-checked against the tree at `8f85739`. Two of its figures were
wrong and are corrected here.

| Claim | Verified |
| --- | --- |
| `AppleBundleIdentifier` is `[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*`, `max_len = 255` | Yes (`crates/willikins-types/src/appstore.rs:426-432`). The derive anchors it as `^(?:…)$` (`willikins-derive/src/attrs.rs:111-120`), counts `chars()` against `max_len` (`codegen.rs:47-49`), and stores the input verbatim (`Self(input.to_string())`, `codegen.rs:286`). |
| `AppleProfileName::parse` refuses only empty, more than 255 characters, any `char::is_control`, and any `is_invisible_or_bidi_control` | Yes (`appstore.rs:749-780`). It stores verbatim (`Self(input.to_owned())`). `is_invisible_or_bidi_control` (`name.rs:43-59`) lists only code points at or above `U+00AD`. |
| `DomainType::IS_SECRET` is an associated const that the registry reads | Yes (`lib.rs:100`; `TypeInfo::of` at `lib.rs:132`; `TypeEntry` and `parse_entry` at `registry.rs:237-255`). |
| `object.rs` asserts on `IS_SECRET` at compile time inside a macro | Yes (`object.rs:105-111`, `const _: () = assert!(…)`). A trybuild fixture already proves it (`tests/derive/fail/non_secret_macro_on_secret_type.{rs,stderr}`, E0080). |
| trybuild uses `cargo check` for a compile-fail-only project | Yes. trybuild 1.0.121, `src/cargo.rs:97,130,153`: `if project.has_pass { "build" } else { "check" }`. This matters in decision (b). |
| `Checked` has public fields and can be built with a struct literal anywhere | Its six fields are `pub` (`check.rs:86-106`), so it **could** be. **Today nothing does, apart from `check` itself** (`check.rs:586`). The brief was wrong about `describe.rs` around line 409: `checked_positive_inputs` there calls `check(…).expect(…)` and builds no struct literal. |
| 16, 7 and 5 `unreachable!`/`expect` sites in `plan.rs`, `apply.rs` and `describe.rs` | **16, 7 and 1.** Recounted over non-test code only (everything above the first `#[cfg(test)]`): `plan.rs` has 16 `unreachable!` and 0 `expect`; `apply.rs` has 7 and 0; `describe.rs` has **1** `unreachable!` (line 301) and 0 `expect`. All five of `describe.rs`'s `.expect(` calls are inside its `#[cfg(test)]` module, which is where the brief's 5 came from. For comparison, `check.rs` has 7 `unreachable!` and 2 `unwrap` outside tests. |
| The CLI and server read `Checked`'s fields directly | Yes, but only `workflow` and `warnings`: `willikins-cli/src/main.rs:241,261,263`, `commands.rs:620`, `willikins-server/src/butler.rs:307,1195`. `startup.rs:143` holds a `pub checked: Checked`. Nothing outside `willikins-core` reads `types`, `order`, `class` or `output_types` except tests. |
| Who reads `Checked::types` | `plan.rs` and `apply.rs` do **not** read it today. Plan resolves every binding again from `workflow.nodes[..].with`. Only tests read it: about 15 sites comparing `checked.types[&n][&p]` to a `TypeRef`, plus three parity tests that `assert_eq!` whole maps between the live and fake catalogs. |

Two facts the brief did not mention, both of which shape the design:

- `apply` resolves a node instance's inputs twice. It reuses `planned.inputs` for `Literal`,
  `Input` and `Item` ports, and re-resolves `Step` and `Keyed` ports against the run's own results
  (`apply.rs:782-809`). A conversion therefore has to be applied at **two** sites, not one:
  decision (f).
- `Value::parse` and `Value::is_secret` consult the **global** `willikins_types::registry()`, not
  the catalog's registry. A test that registers synthetic types in a custom registry therefore
  cannot use a literal of those types. It binds a declared workflow input instead: decision (c).

## SHARED VALUES

Implementers read this table instead of their prompts. Nothing in it may be retyped from memory.

| What | Value |
| --- | --- |
| The one row | `AppleBundleIdentifier => AppleProfileName` |
| Row macro | `willikins_types::conversions!`, `#[macro_export]`, defined in `crates/willikins-types/src/registry.rs` beside `domain_types!` |
| Where the production rows are listed | `crates/willikins-types/src/lib.rs`, a private `fn conversion_rows()` directly under the `registry::domain_types! { … }` invocation |
| Row type | `willikins_types::registry::Conversion`, re-exported as `willikins_types::Conversion` and as `willikins_core::value::Conversion` |
| Generic constructor behind the macro | `willikins_types::__private::conversion::<A, B>()`, bounded `A: DomainType + DomainObject + 'static`, `B: DomainType + DomainObject + From<A> + 'static` |
| The single probe | `TypeRegistry::probe_conversion(&self, from: &TypeName, to: &TypeName) -> Option<Conversion>` |
| Listing (names only, no converter) | `TypeRegistry::conversion_pairs(&self) -> impl Iterator<Item = (&TypeName, &TypeName)>`, in declaration order |
| Registry constructor (public) | `TypeRegistry::new(entries: Vec<TypeEntry>, conversions: Vec<Conversion>) -> Self` (replaces the `pub(crate) from_entries`) |
| Only caller of `probe_conversion` | `willikins-core/src/check.rs`, inside `Resolver::check_with_port`, under a narrowly scoped `#[allow(clippy::disallowed_methods)]` |
| Edge record | `willikins_core::check::Edge`, private fields `ty: TypeRef` and `conversion: Option<Conversion>` |
| `Checked::types` | `IndexMap<NodeName, IndexMap<PortName, Edge>>` (was `IndexMap<PortName, TypeRef>`) |
| Applying an edge | `Edge::deliver(&self, Value) -> Value`, reached from `plan.rs` and `apply.rs` only through `ResolveCtx::deliver(node, port, value)` |
| Catalogue key | `list_tools_json()["conversions"]`: `[{"from": "AppleBundleIdentifier", "to": "AppleProfileName"}]` |
| Secrecy assertion message | `conversions!: a conversion may not make a value less secret; its target must be at least as secret as its source` |
| New negative fixture | `workflows/fixtures/appstore-profile-name-into-identifier.yaml` → `TypeMismatch { node: profile, port: identifier, expected: Exact(AppleBundleIdentifier), found: AppleProfileName }` |
| Document binding | `name: ${{ steps.bundle_id.identifier }}` in the `profile` step; the `profile_name` input is removed |

## Decisions

### (a) The `conversions!` macro: location, syntax, and its three-part expansion

Rust has no trait reflection. Nothing can list `From` impls, at run time or at compile time, so
the table cannot be *discovered* from the impls. What can be guaranteed is that a table row cannot
**drift** from an impl: the row only compiles if the impl exists. That is the design.

**Location.** The macro is defined in `crates/willikins-types/src/registry.rs`, next to
`domain_types!`. Unlike `domain_types!` (which is `pub(crate)`), it is `#[macro_export]`, for two
reasons: the trybuild fixtures are separate crates, and the no-chains test in `willikins-core`
registers synthetic rows. The production rows are listed once, in `lib.rs`, directly under the
type list:

```rust
registry::domain_types! {
    WordList,
    // … unchanged …
    AppleProfileContent,
}

/// Every total conversion between two registered domain types, one row per fact about their
/// grammars. A row is admissible only if every value of the source type is, byte for byte, a
/// valid value of the target type (proved by a property test beside its `From` impl), and the
/// target is at least as secret as the source (proved at compile time by the macro).
fn conversion_rows() -> Vec<registry::Conversion> {
    conversions![
        // Every bundle identifier is a valid profile name. See the `From` impl in appstore.rs.
        AppleBundleIdentifier => AppleProfileName,
    ]
}
```

`domain_types!`'s generated `registry()` changes from `TypeRegistry::from_entries(vec![…])` to
`TypeRegistry::new(vec![…], $crate::conversion_rows())`. `type_infos()` and
`assert_all_examples_parse()` are unchanged.

**Syntax.** `conversions![A => B, C => D, …]` is an **expression** of type `Vec<Conversion>`. It
has an optional trailing comma, and an empty list is allowed. An expression form lets the same
macro serve the production list, a unit test and a trybuild fixture without a naming convention
for a generated function.

**Expansion.** For `conversions![AppleBundleIdentifier => AppleProfileName]`:

```rust
::std::vec![
    {
        // (b) Secrecy only goes up, at compile time. A free `const` item over two
        //     concrete types is evaluated by `cargo check` (trybuild's mode), not
        //     only by a build: the same mechanism as `impl_domain_object_non_secret!`.
        const _: () = ::std::assert!(
            !<AppleBundleIdentifier as $crate::DomainType>::IS_SECRET
                || <AppleProfileName as $crate::DomainType>::IS_SECRET,
            "conversions!: a conversion may not make a value less secret; \
             its target must be at least as secret as its source"
        );
        // (a) `conversion::<A, B>` is bounded `B: From<A>`: a row without a
        //     hand-written impl is E0277, so the row cannot drift from the impl.
        // (c) It returns the row itself, carrying the type-erased converter.
        #[allow(clippy::disallowed_methods)] // the macro is the one sanctioned caller
        let row = $crate::__private::conversion::<AppleBundleIdentifier, AppleProfileName>();
        row
    },
]
```

The generic constructor and the converter live in `crates/willikins-types/src/__private.rs`. That
module is already documented as "consumed by generated code … do not use this module directly":

```rust
pub fn conversion<A, B>() -> crate::registry::Conversion
where
    A: DomainType + DomainObject + 'static,
    B: DomainType + DomainObject + From<A> + 'static,
{
    // Belt to the macro's braces: fires for a direct call, but only at
    // monomorphization (see decision (b) for why this is not the guarantee).
    const { assert!(!A::IS_SECRET || B::IS_SECRET, "…same message…") };
    crate::registry::Conversion::new(A::TYPE_NAME, B::TYPE_NAME, convert::<A, B>)
}

/// The one runtime assertion conversions carry. Values travel as the dynamically
/// typed `Value`, so the converter must downcast. `check` bound this edge only
/// after resolving the binding's type to `A`, so the downcast cannot fail for a
/// `Checked` that `check` built. It is still an assertion, confined here, in code
/// only `conversions!` instantiates. Its message names the two types and nothing
/// else: never the object, whose bytes a secret type must not print.
fn convert<A, B>(source: &dyn DomainObject) -> Arc<dyn DomainObject>
where /* same bounds */
{
    let Some(source) = crate::downcast::<A>(source) else {
        unreachable!("conversion {} -> {}: the source is not a {}", A::TYPE_NAME, B::TYPE_NAME, A::TYPE_NAME)
    };
    Arc::new(B::from(source.clone()))
}
```

`Conversion` (in `registry.rs`) is `Clone`. It holds `from: TypeName`, `to: TypeName` (built with
the existing private `TypeName::from_static`) and `convert: fn(&dyn DomainObject) -> Arc<dyn
DomainObject>`. It has hand-written `PartialEq`/`Eq` over `(from, to)` only, because comparing fn
pointers compares addresses, and a hand-written `Debug` that prints `Conversion(A -> B)`. Its
constructor `Conversion::new` is `pub(crate)`. Its public surface is `from()`, `to()` and
`apply(&self, &dyn DomainObject) -> Arc<dyn DomainObject>`. So outside `willikins-types` the only
way to hold a `Conversion` is through `__private::conversion`, which is to say through the macro.

MSRV: inline `const { }` blocks that use generic parameters are stable since 1.79, and the
workspace declares 1.88.

### (b) Secrecy only goes up, proved at compile time

The rule is `!A::IS_SECRET || B::IS_SECRET`. The macro emits it as a free `const _` over two
concrete types, so `cargo check` evaluates it. That is the same mechanism, and the same E0080, as
`impl_domain_object_non_secret!`, whose trybuild fixture already passes in this tree.

**The trybuild proof.** Two fixtures go under `crates/willikins-types/tests/conversions/fail/`.
They are wired into the **existing** `tests/derive_compile_fail.rs` by adding a second
`t.compile_fail("tests/conversions/fail/*.rs")` line to its one test function. That adds no new
test binary: this host has 143 of them and a 60 GB `target/`.

- `secret_to_public.rs` defines a local secret type `Leaky`, starting from the `DomainType` impl in
  `non_secret_macro_on_secret_type.rs` (`IS_SECRET = true`). **That `Leaky` has no
  `DomainObject` impl**, because the point of that fixture was that the macro refused to give it
  one. `__private::conversion` is bounded `A: DomainObject`, so without one the fixture fails E0277
  for the wrong reason. The fixture therefore also gives `Leaky` a hand-written **secret**
  `DomainObject` impl: `is_secret` → `true`, `render` → `Rendered::Redacted { type_name: "Leaky" }`,
  `expose(&self, _token: &SinkToken)` → its string, plus `as_any`, `dyn_eq` and `clone_box`, the
  shape a hand-written secret type in the crate already has. Next it defines a local public type
  with `#[derive(DomainType)]` and `impl From<Leaky> for Public` (the body may be `todo!()`;
  nothing runs), then `let _ = willikins_types::conversions![Leaky => Public];`. The fixture must
  fail with **E0080 carrying the secrecy message, and nothing else**.
- `missing_from_impl.rs` has two public local types (both derived, so both implement `DomainType`
  and `DomainObject`) and no `From` impl, then `conversions![A => B]`. It must fail with **E0277
  naming `From<A>`, and nothing else**.

Both `.stderr` files are generated once with `TRYBUILD=overwrite`, then **read** and committed. That
is one scoped cargo run of about 90 seconds. trybuild's `compile_fail` passes on *any* error that
matches the committed `.stderr`. A fixture that fails for an unrelated reason (a missing bound, a
typo) is therefore a green test that proves nothing, and the reviewer of C2 must confirm that each
`.stderr` shows exactly the intended error.

If `secret_to_public.rs` **compiles** (that is, a `const _` item nested in block position inside a
function body is not evaluated under `cargo check`; see verify item 1), hoist the assertion to item
position. Split the macro into an item-position `conversion_asserts!` invoked at module level in
`lib.rs`, emitting one `const _` per row exactly as `object.rs:105` does, and have the expression
macro emit only the row. The fixture then invokes both, and the guarantee rests on the item-position
form.

**Why the macro's `const _` is the guarantee and the inline `const { }` is only a belt.** trybuild
runs `cargo check` for a compile-fail-only project (verified above). An inline `const` block inside
a generic function is evaluated at monomorphization, which `cargo check` is believed not to reach.
So a fixture that calls `__private::conversion::<Leaky, Public>()` directly might *compile* under
trybuild. The workspace gate's `cargo test --workspace` does build, so the belt still refuses such
a call before anything merges, but trybuild cannot be the proof of it. Verify item 1 settles
whether `cargo check` on Rust 1.97 reports it. If it does, add the third fixture; if it does not,
record that here. As a second belt, `clippy.toml` gains `willikins_types::__private::conversion` as
a disallowed method (the macro's expansion carries the one `#[allow]`), so a hand-written direct
call fails the clippy gate.

**The positive half is proved too.** In `crates/willikins-types/src/probe.rs` (already
`#[cfg(test)]`), `impl From<Probe> for ProbeSecret` and a unit test using `conversions![Probe =>
ProbeSecret]`. The public-to-secret direction compiles, and the converted object renders
`[REDACTED ProbeSecret]`. This registers nothing in production.

**Public-to-secret rows: admitted by the type rule, none registered, and why the first one needs
its own argument.** The operator's rule is a floor ("only goes up"), and the macro enforces exactly
that floor. A public-to-secret row leaks nothing: redaction only increases. It does, however, let a
document feed a secret-typed port from a public workflow input. For example, a hypothetical `Text =>
OpaqueSecret` row would let an operator paste a real secret as a `Text` input, which puts it on
argv and in the journal's recorded inputs. That is the exact path the design's "no secret input
types" rule closes. No such row exists or is proposed. Any future public-to-secret row must say, in
its own plan, why that path stays closed: for example by also requiring its source edge to be
derived. The admission rule for rows (the doc comment on `conversion_rows`) states this.

**The existing secret rule is untouched, and its precedence is preserved.** In
`check_with_port`, the `SecretToNonSecretSink` test still runs before any type test. Because a
conversion can only raise secrecy, no registered row can turn a refused secret flow into an accepted
one. The design doc's "No coercion" means no coercion of a secret into a non-secret sink, and it
still holds. Decision (e) records the order.

### (c) The table: key, single probe, no transitive search

**Key and storage.** `TypeRegistry` gains two fields: `conversions: Vec<Conversion>` in declaration
order, and `conversion_index: HashMap<(TypeName, TypeName), usize>`. This mirrors the existing
`entries` and `index` pair. `TypeRegistry::new` builds both. It `debug_assert!`s no duplicate `(from,
to)`, no row with `from == to` (an identity row compiles, because `From<T> for T` is blanket, but it
means nothing), and that both ends of every row are among `entries`. A unit test over the production
registry asserts the same three things in every build profile.

**The single probe.** `probe_conversion(from, to)` does exactly one `HashMap::get` on the owned
pair and clones the row. There is **no transitive search**. There is no closure computed at
construction either: the table holds exactly the rows the macro listed, and nothing derived from
them.

**Where the table lives, and why there.** The table lives in the registry the catalog already
carries (`Catalog::registry()`), so `check` reaches it through the value it already has, and a test
can substitute a registry with synthetic types and rows. The other placements were weighed:

- A free global `willikins_types::conversions()` is reachable from `plan.rs` as easily as from
  `check.rs`, and a test cannot substitute it.
- `Catalog` could hold the table, but that is still reachable from `plan` and `apply`, which receive
  the catalog.

Rust privacy cannot make a cross-crate method callable from one module only. This repository's
established answer for that is the one `SinkToken::new` uses, and it is used here:

1. `clippy.toml` gains `{ path = "willikins_types::registry::TypeRegistry::probe_conversion",
   reason = "only check may resolve a conversion; plan and apply apply the one check recorded in
   Checked::types" }`. The one `#[allow(clippy::disallowed_methods)]` sits on the single statement
   in `check_with_port` that probes. Clippy's lint is type-resolved, so an auto-deref'd call through
   `catalog.registry()` is caught too (verify item 2).
2. A tripwire unit test in `check.rs`'s test module asserts that `include_str!("plan.rs")`,
   `include_str!("apply.rs")` and `include_str!("describe.rs")` do not contain the method's name. The
   needle is assembled with `concat!("probe_", "conversion")` so the test does not trip on itself.
   It catches an `#[allow]` smuggled into those files, which clippy alone would not.
3. **Structurally**, `plan`'s and `apply`'s signatures are unchanged and carry no table. What they
   do receive is the `Checked` edge map, whose records already hold the chosen converter. The listing
   `conversion_pairs()` returns names only, never a `Conversion`, so even the catalogue path cannot
   hand `plan` a converter.

**The no-chains test** goes in the existing `crates/willikins-core/tests/check.rs`, with no new
binary:

- Three local types `ChainA`, `ChainB`, `ChainC`, each `#[derive(DomainType)]` over `String` with
  `pattern = "[a-z]+"`, plus `impl From<ChainA> for ChainB` and `impl From<ChainB> for ChainC`.
  They are local types, so there is no orphan-rule problem. `derive_pass.rs` proves the derive works
  outside the crate.
- `let registry: &'static TypeRegistry = Box::leak(Box::new(TypeRegistry::new(vec![TypeEntry::of::<ChainA>(),
  TypeEntry::of::<ChainB>(), TypeEntry::of::<ChainC>()], willikins_types::conversions![ChainA =>
  ChainB, ChainB => ChainC])));`, then `Catalog::new(registry)` and one `DummyTool` whose required
  port `c` is `Exact(ChainC)`.
- The workflow declares an input `a: ChainA` and binds it to `c`. It uses an **input**, not a
  literal, because literals parse through the global registry (see the re-verified facts).
- It asserts `check` returns exactly `vec![CheckError::TypeMismatch { node: sink, port: c,
  expected: PortType::Exact(ChainC), found: ChainA }]`, and that its `Display` is exactly
  ``node `sink`, port `c`: expected ChainC, found `ChainA` ``. That is today's error, byte for byte.
- Positive controls in the same test: an input `b: ChainB` bound to `c` checks, and its edge's
  conversion is `ChainB -> ChainC`. An input `a` bound to a `ChainB` port checks with `ChainA ->
  ChainB`.
- A two-node chain written out by the document itself (a pure echo tool from `ChainB` to `ChainB`
  between them) checks. Each edge is one hop: the refusal is of chains **inside one edge**, not of a
  document that spells out two edges.

### (d) Port opt-in versus implicit acceptance: implicit

**The case for an opt-in.** Rust is the operator's analogy, and in Rust a function taking `B` does
not accept an `A` even when `B: From<A>`. The author opts in by writing `impl Into<B>`. A
`PortSpec` flag (`accepts_conversions`, `skip_serializing_if` false, following the `derived_only`
precedent) would mirror that.

**Why it does not earn its place here.** Rust needs the opt-in because Rust's `From` impls carry no
constraint at all: they can allocate, lose meaning (`From<u8> for char`), or encode anyone's
convention, and Rust also needs a generic signature to know the argument's type. A row in this table
is admitted only as a **fact**: every value of `A` is, byte for byte, a valid `B` (a property test
proves it), and secrecy only goes up (the compiler proves it). So:

1. **A tool cannot observe the difference.** A converted value is a valid `B` that the tool could
   equally have been handed as a literal or an input, and `Value` carries no provenance. A tool
   whose contract is "any valid `B`" has no basis on which to refuse one.
2. **A refusal could only express policy about provenance** ("I don't want `B`s that were `A`s"),
   and the design doc puts policy in the workflow, never in the tool. The operator's third
   constraint is the specific case: an opt-in on `appstore.profile.ensure`'s `name` would be the
   tool anticipating the naming convention.
3. **The opt-in already exists, at the right level.** Two gates remain. The row is the fact. The
   binding is the document author's explicit choice, per edge, because nothing is ever converted
   unless a document binds an `A` to a `B` port. A per-port flag would be a third gate, placed in
   the one layer the design says must hold no opinion.
4. **Cost.** Every future row would need edits to every port of its target type, and every tool
   crate would carry knowledge of rows that live in `willikins-types`.

The honest cost of choosing implicit: registering a row widens every port of the target type at
once. That is acceptable because rows are rare, each one is argued as a grammar fact with a property
test, and the catalogue lists them (decision (g)).

**What a consumer sees, either way.** With implicit acceptance (chosen), every `ToolSpec`
serializes byte-identically, so no snapshot of a tool changes, and a top-level `conversions` key
lists the rows. With an opt-in, `PortSpec` would gain a serialized flag on each opted-in port, and
the fake catalogue's `ToolSpec` snapshot would change for `appstore.profile.ensure`.

### (e) The edge record: shape, where `check` builds it, what it excludes

**Shape** (`crates/willikins-core/src/check.rs`):

```rust
/// One checked data-flow edge into a node's input port: the binding's own
/// resolved type, and the conversion `check` chose to deliver it to the port's
/// type, if they differ. `check` is the only constructor. `plan` and `apply`
/// read the conversion from here and never resolve one themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    ty: TypeRef,
    conversion: Option<Conversion>,
}

impl Edge {
    pub(crate) fn exact(ty: TypeRef) -> Self;
    pub(crate) fn converted(ty: TypeRef, conversion: Conversion) -> Self;
    /// The binding's own resolved type: exactly what `Checked::types` held before 3d.
    pub fn ty(&self) -> &TypeRef;
    pub fn conversion(&self) -> Option<&Conversion>;
    /// The type the port receives: `ty`, or the conversion's target.
    pub fn delivered(&self) -> TypeRef;
    /// Apply this edge to a value resolved for it; see decision (f).
    pub fn deliver(&self, value: Value) -> Value;
}
```

`ty` deliberately keeps its current meaning (the binding's own type, the conversion's *source*), so
every existing assertion keeps its expected value and changes only in form. The fields are private
and the constructors `pub(crate)`, so outside `willikins-core` an `Edge` can only come from a real
`check`. This does not extend the forgeable-`pub`-field pattern. `Checked::types` becomes
`IndexMap<NodeName, IndexMap<PortName, Edge>>`. `output_types` is unchanged, because an output has
no port and so never converts.

**The test sites that change, all mechanically (`[..]` becomes `[..].ty()`, expected values
untouched):** `willikins-core/tests/check.rs:379`; `tests/check_adversarial.rs:444, 558, 814, 866,
967`, and the loop at `1121`/`1141`; `willikins-cli/tests/acceptance.rs:438`;
`willikins-cli/tests/adversarial.rs:446`. The parity tests that compare whole maps
(`willikins-providers-doppler/tests/live_catalog.rs:132`, `catalog_check_parity.rs:85`,
`willikins-providers-github/tests/catalog_check_parity.rs:86`, and
`willikins-cli/tests/acceptance_m3a_buildkite.rs:279-280`) compile unchanged through `Edge:
PartialEq`. We considered `impl PartialEq<TypeRef> for Edge`, which would leave the other sites
untouched, and rejected it: it would make an edge "equal" to its source type, which misleads the
moment a conversion exists.

**Where `check` builds it, and in what order.** Only `Resolver::check_with_port` changes. Its
sequence becomes, with the new step marked:

1. unbound → `UnboundInput` (unchanged);
2. a `Literal` → `derived_only` refusal, else `check_literal` against the port's own type. A literal
   is **never** converted: it has no source type, and it parses as `B` directly.
   `record_type(found)` becomes `Edge::exact(found)`;
3. `resolve` → `(found, source)` (unchanged);
4. `derived_only` → `UnderivedBinding` (unchanged; provenance is checked on the source before any
   conversion, so a conversion cannot launder a literal past a derived-only port);
5. secret found and the port does not accept secrets → `SecretToNonSecretSink` (unchanged, and still
   first);
6. `port_spec.ty.accepts(&found)` → `Edge::exact(found)` (unchanged);
7. **new:** else, if `port_spec.ty` is `PortType::Exact(to)` and `!found.list && !to.list`, then
   `#[allow(clippy::disallowed_methods)] let row = self.registry.probe_conversion(&found.name,
   &to.name);`. If that is `Some(row)`, record `Edge::converted(found, row)`;
8. else → `TypeMismatch { expected, found }` (unchanged, same fields).

Error order, error variants and messages are untouched: step 7 only turns what would have been a
`TypeMismatch` into an accepted edge, and only for a registered pair.

**Exclusions, each by construction:**

- **Literals** parse as the port's type (step 2).
- **`AnySecret` ports** accept any secret scalar in step 6 and have no single target type.
- **`for_each` sources** and **workflow outputs** have no port type to convert to, and `types` does
  not cover them.
- **Lists.** The probe requires two scalars. `list<A>` into a `list<B>` port stays `TypeMismatch`.
  In Rust, `Vec<A>` is not `Into<Vec<B>>` either. Lifting would be a check-only change later, since
  `Value::converted` (decision (f)) is already total over lists.
- **`derived_only`** is unaffected (step 4).
- **Key ports** convert like any other port. A converted `name` is a valid `AppleProfileName` key.

### (f) Applying the edge in `plan` and `apply`, without the table

**One application function, total, no panic** (`crates/willikins-core/src/value.rs`):

```rust
impl Value {
    /// This value delivered through `conversion`. Total over every state:
    /// Unknown(A) becomes Unknown(B), preserving the list flag; a known scalar
    /// converts; a known list converts element-wise (check never builds a list
    /// edge today, but no state panics here).
    #[must_use]
    pub fn converted(&self, conversion: &Conversion) -> Value;
}
```

`Edge::deliver(value)` is `match &self.conversion { None => value, Some(c) => value.converted(c) }`.
**With no conversion it returns its argument unchanged.** That identity is what makes every existing
document's plan byte-identical.

**The two sites.** `ResolveCtx` (`plan.rs:352`) gains `edges: &'a IndexMap<NodeName,
IndexMap<PortName, Edge>>` and one method:

```rust
/// The value `node`.`port` receives: `value` delivered through the edge `check` recorded.
/// A missing edge means `check` recorded no conversion, so the value passes through
/// unchanged. That is today's behaviour exactly, and no assertion is needed.
fn deliver(&self, node: &NodeName, port: &PortName, value: Value) -> Value
```

- `plan.rs`, `bind_ports`: the non-literal arm becomes `ctx.deliver(node_name, port,
  resolve_binding(ctx, &site, binding, item)?)`. That covers `Input`, `Item`, `Step` and `Keyed`.
  The literal arm is unchanged.
- `apply.rs`, `resolve_instance_inputs`: its re-resolution of `Step`/`Keyed` ports becomes
  `resolved.insert(port.clone(), ctx.deliver(&planned.name, port, resolve_binding(…)?))`. Its
  signature takes `checked: &Checked` in place of `workflow: &Workflow` (it only used `workflow` to
  build the context). `Literal`, `Input` and `Item` ports keep `planned.inputs`, which `plan` already
  delivered.
- Every `ResolveCtx` construction (`plan.rs` twice, `apply.rs` twice) passes `&checked.types`.
- `resolve_binding` itself does **not** deliver, because it also resolves `for_each` sources and
  workflow outputs, which have no edge.

The "missing edge passes through" rule is how this avoids adding an `unreachable!`. It is sound
because `check` only accepts a type difference by recording an edge that carries the conversion. A
`Checked` without that edge could only have been built by hand, and it then behaves exactly as today
(the tool refuses a wrong-typed value with `port … has an unexpected type`).

**A value known at plan time.** In the signing document, `steps.bundle_id.identifier` is `Known`
during `plan` (it already feeds the profile's `identifier` **key** port, so it must be, or the
document would fail with `KeyUnknown` today). `plan` delivers `Known(AppleBundleIdentifier
"com.example.willikins-demo")` as `Known(AppleProfileName "com.example.willikins-demo")`, `read`
sees an `AppleProfileName`, and `PlannedNode.inputs["name"]` holds it.

**A value known only at apply time.** A `Step` from a non-pure node whose `read` predicts the output
`Unknown`: during `plan` the edge delivers `Unknown(A)` as `Unknown(B)`, typed as the port's type,
so `read` sees what it would see for any unknown `B`. If the port is a key port, `plan` refuses with
`KeyUnknown` exactly as it would for an unconverted unknown key. During `apply`,
`resolve_instance_inputs` re-resolves the `Step` against the run's real results, gets `Known(A)`,
and delivers `Known(B)` from **the same edge**, before `ensure`. Drift is unaffected: the fingerprint
covers outputs only.

**How a converted value renders.** It is a `B`. In the plan, `PlannedNode.inputs["name"]`
serializes as `{"type":"AppleProfileName","list":false,"state":"known","value":"com.example.willikins-demo"}`,
or `{"type":"AppleProfileName","list":false,"state":"unknown"}` while unknown. The journal's
`NodeStarted.inputs` carries the same `Value` JSON inside its existing redacted wrapper. **There is
no provenance marker, by decision.** `Value`'s JSON schema is `additionalProperties: false`, its
shape is pinned by insta, and the journal wire format is frozen. Beyond that, the tool received a
`B`, and that is what the record should say. Provenance lives in `Checked` (the edge), which is also
where a later `check --explain` or typed-graph rendering would read it. A converted **secret** value
(none exists today; the synthetic test in acceptance 7 exercises one) renders
`[REDACTED <target>]` and contributes `SECRET_FINGERPRINT_MARKER` if it ever reaches an output.

### (g) What the catalogue and `describe` say

**The catalogue.** `Catalog::list_tools_json()` gains a third top-level key, next to `tools` and
`types`:

```json
"conversions": [ { "from": "AppleBundleIdentifier", "to": "AppleProfileName" } ]
```

It is built from `registry.conversion_pairs()`, in declaration order, and never from a `HashMap`
iteration. Its meaning is fixed in the MCP `list_tools` description (`willikins-server/src/mcp.rs:669`),
which gains one sentence: *"`conversions` lists every `(from, to)` pair for which a port of type
`to` also accepts a scalar binding of type `from`, converted in one hop; nothing else converts."*
The same JSON is what `willikins schema --catalog` prints and what `list_tools` returns (their
equality test, `willikins-server/tests/acceptance_read_ops.rs:181`, keeps passing). The `tools`
array is byte-identical, so the three snapshots that pin `json["tools"]`
(`willikins-core/src/catalog.rs:269`, `willikins-providers-fake/src/lib.rs:178`) and the type
catalogue snapshot do not move. The catalog unit test gains an assertion on `json["conversions"]`.
An agent composing a document over MCP learns, from one list, that a port of type `to` also takes a
`from`.

**`describe`.** Unchanged. It resolves a document's *inputs* against partial raw values, never
looks at a port, and a conversion is a property of an edge. With implicit acceptance there is
nothing per-port to report: an input typed `AppleBundleIdentifier` is described exactly as before,
with its type's own prompt and example. The checked document's edges (`Checked::types`) are where a
future `explain` rendering would show `name ← AppleBundleIdentifier (converted to AppleProfileName)`.
That rendering is out of scope, and it belongs to the typed-graph follow-up.

**`check` output (CLI, MCP `validate`).** Unchanged. A successful check prints only warnings, as
today. A failed conversion is the same `TypeMismatch`, with the same text.

### (h) The one row: `AppleBundleIdentifier` → `AppleProfileName`

**The grammar-containment argument.** Let `s` be any string that `AppleBundleIdentifier::parse`
accepts. Then:

- `s` matches `^(?:[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)$`, so it is **non-empty** (the `+` needs one
  alphanumeric), and every character is in `[A-Za-z0-9.-]`;
- none of those 64 ASCII characters is `char::is_control` (`U+0000`–`U+001F`, `U+007F`–`U+009F`),
  and none is in `is_invisible_or_bidi_control`, whose smallest member is `U+00AD`;
- `s.chars().count() <= 255` by `max_len`, and `AppleProfileName` caps at the same 255, counted the
  same way (`chars().count()`).

Those are all four of `AppleProfileName`'s refusals, so `AppleProfileName::parse(s)` accepts `s`.
Both types store the input verbatim, so the result is **byte-identical**. The conversion is total,
and it is the identity on the string.

**The impl** (in `appstore.rs`, beside `AppleProfileName`, same module, so it can use the private
field). It constructs the value directly and never calls `parse(…).expect(…)`: containment is proven
once, above and by the test below, not asserted on every call.

```rust
/// Every bundle identifier is a valid profile name. This is a fact about the two grammars,
/// proved in the doc above `conversion_rows` and pinned by
/// `every_bundle_identifier_is_a_valid_profile_name`. It is not a naming policy: which name a
/// profile gets is the document's choice.
impl From<AppleBundleIdentifier> for AppleProfileName {
    fn from(identifier: AppleBundleIdentifier) -> Self {
        Self(identifier.as_str().to_owned())
    }
}
```

**The property test** goes in `appstore.rs`'s own `#[cfg(test)] mod tests`. `proptest` is already a
dev-dependency of `willikins-types`, and a unit test lands in the library's existing test binary, so
no new binary is created.

- Strategy 1 generates from the grammar: `"[A-Za-z0-9]{1,8}([.-][A-Za-z0-9]{1,8}){0,40}"`, keeping
  only candidates with `chars().count() <= 255`. For each, `AppleBundleIdentifier::parse` is `Ok(id)`,
  and `AppleProfileName::parse(id.as_str())` is `Ok(p)` with `p == AppleProfileName::from(id)` and
  `p.as_str() == id.as_str()`.
- Strategy 2 uses `any::<String>()` and states the implication itself: if `AppleBundleIdentifier`
  parses `s`, then `AppleProfileName` parses `s` to the same string.
- Boundary unit cases: a 255-character identifier converts, and a 256-character one is not an
  identifier at all. `"com.example.MyApp\n"` is refused by `AppleBundleIdentifier` (this pins the
  regex `$` not matching before a trailing newline; verify item 4). The reverse is **not** a fact:
  `"has space"` is a profile name and not an identifier. No reverse row exists, and the new negative
  fixture (below) pins that the reverse binding is still `TypeMismatch`.

### (i) The document change

`workflows/appstore-signing-profile-from-doppler.yaml`:

- **Remove** the `profile_name` input.
- In the `profile` step, bind `name: ${{ steps.bundle_id.identifier }}`. It binds the same output
  that step's `identifier` port already binds, and not `${{ inputs.identifier }}`. Two reasons. The
  profile is named after the identifier it actually signs, the same value by construction. And it
  exercises the `Step` path, the one `apply` re-resolves and therefore re-delivers through the edge,
  in the one real document that uses a conversion.
- Append to the header comment, verbatim in substance: *"The profile is named after the bundle
  identifier it signs. That is this operator's convention, not willikins': `appstore.profile.ensure`
  accepts any `AppleProfileName`, and another document may bind `name` to an input or a literal
  instead. The binding type-checks because every `AppleBundleIdentifier` is a valid
  `AppleProfileName`, a registered conversion (milestone 3d, `docs/plans/2026-09-23-milestone-3d-conversions.md`).
  Nothing in the tool knows the convention."*
- `appstore.profile.ensure` is **unchanged**. Implicit acceptance needs nothing from it.

**The ripples, all in the same commit:**

- `workflows/fixtures/state/appstore-signing-profile.json`: the profile key becomes
  `com.example.willikins-demo#com.example.willikins-demo` (the profile's key is `(identifier,
  name)`), so `profile_documents.rs`'s positive plan still finds `PROFILE1` `Present`. Add a second
  bundle identifier `com.example.willikins-demo-two` (`name` `willikins-demo`, `platform`
  `UNIVERSAL`, a fresh fake id) that has **no** profile.
- `crates/willikins-providers-appstore/tests/profile_documents.rs`: `positive_inputs()` drops
  `profile_name`.
- `crates/willikins-cli/tests/appstore_profile_apply_redaction.rs`: this test forced a create by
  passing a profile name the seed lacked. It can no longer choose the name, so it passes
  `identifier=com.example.willikins-demo-two` instead, and drops `profile_name`. Its dump assertion
  must look for the created **profile's** key, `com.example.willikins-demo-two#com.example.willikins-demo-two`,
  or for the fake's created-profile id. It must **never** look for the bare identifier: after this
  change the seed puts that string in the dump whether or not a profile was created, so the
  assertion would pass without proving anything.
- The three negative fixtures that declare `profile_name` (`appstore-profile-content-into-template`,
  `-development-type`, `-wrong-typed-certificate`) are **not touched**. Their errors stay
  byte-identical.
- **New negative fixture** `workflows/fixtures/appstore-profile-name-into-identifier.yaml`: an
  `AppleProfileName` input bound to `appstore.profile.ensure`'s `identifier` port. Its header names
  its acceptance test and the exact error `TypeMismatch { node: profile, port: identifier, expected:
  Exact(AppleBundleIdentifier), found: AppleProfileName }`. It pins that the conversion runs one way
  only.

## Equivalence, and how it is proven

What must not change: every existing negative fixture's exact errors; every existing document's
`Plan::fingerprint` (and, stronger, its whole plan JSON); the journal wire format; `Value`'s JSON
shape.

1. **A characterization snapshot, committed first**, before any production change, in the
   **existing** `crates/willikins-dsl/tests/acceptance.rs` (it already depends on
   `willikins-providers-fake` and `insta`). For every `workflows/*.yaml` and
   `workflows/fixtures/*.yaml` in sorted order, record one of these:
   - the load error's `Display`;
   - each `CheckError`'s `kind()`, `Display` and `serde_json` form;
   - on success: the warnings, every `node.port: <type>` in `Checked::types` (rendered from `.ty()`
     after commit C4, appending ` -> <to>` only when an edge carries a conversion), every output
     type, and a plan against a default fake state. The plan uses inputs synthesized from each
     declared input's default, else its type's registry `example` (`[example]` for a list), and the
     snapshot records `serde_json::to_string(&plan)` and `plan.fingerprint()`, or the `PlanError`'s
     `Display`.
   The snapshot is green on the current tree. Afterwards, **no commit may change it except C7**,
   where only the signing document's entry changes, in the reviewed ways (one fewer input, one edge
   with ` -> AppleProfileName`, the synthesized identifier example now also naming the profile), and
   one new entry appears for the new negative fixture.
2. **Identity, by unit test.** `Edge::deliver` on an edge without a conversion returns a value
   `==` to its argument, with identical `serde_json`. Combined with the snapshot showing that no
   existing document has a converted edge, every plan is identical by construction: `deliver` is
   the only new step on `plan`'s and `apply`'s path.
3. **The signing document against its predecessor.** The pre-change document is kept verbatim as a
   test constant in the test that uses it, and not as a file under `workflows/fixtures/`, which
   holds one document per negative case. Plan the new
   document with `positive_inputs()` minus `profile_name`, and the old one with
   `profile_name = "com.example.willikins-demo"`, both against the same seed. Assert
   `serde_json::to_value(&new_plan) == serde_json::to_value(&old_plan)` (the whole plan: nodes,
   inputs, outputs, class) and equal `fingerprint()`s. Then `apply` both on fresh copies of the
   seed with `Approval::Human` and a `RecordingObserver`, and assert the two event streams serialize
   identically.
4. **Journal and `Value`.** No file under `crates/willikins-journal/` changes, and its insta
   snapshots (event shapes, `Value` JSON, schema) pass untouched. `value.rs`'s `json_shape_*`
   snapshots pass untouched.
5. **For the verifier:** `git diff <base>..<head> -- 'crates/**/snapshots/**'` shows only the C1
   snapshot's creation and its C7 changes. `git diff <base>..<head> -- workflows/fixtures/` shows the
   seed file and the new negative fixture, and nothing else.

## Acceptance tests

1. **Secrecy at compile time** (trybuild): `secret_to_public.rs` fails with E0080 and the secrecy
   message; `missing_from_impl.rs` fails with E0277.
2. **Secrecy upward compiles**: `Probe => ProbeSecret` builds a row whose converted object renders
   `[REDACTED ProbeSecret]`.
3. **Table invariants**: the production registry has no duplicate row, no identity row, and every row
   names two registered types. `probe_conversion` finds the one row and returns `None` for the
   reverse pair.
4. **No chains** (decision (c)): `ChainA` bound where `ChainC` is wanted fails with today's exact
   `TypeMismatch` and `Display`; the positive controls pass.
5. **Check integration** (real types, fake catalog): an `AppleBundleIdentifier` input bound to the
   profile's `name` checks, with edge `AppleBundleIdentifier -> AppleProfileName`. The new negative
   fixture fails with its named error. A literal on the `name` port records an exact edge. `list<A>`
   into a `list<B>` port (synthetic) stays `TypeMismatch`. A secret bound to a non-secret port where
   no row exists is still `SecretToNonSecretSink`.
6. **Plan and apply** (synthetic types, `willikins-core/tests/apply.rs`): a known-at-plan `Input`
   edge delivers `Known(B)` into `PlannedNode.inputs`. An unknown-at-plan `Step` edge plans as
   `Unknown(B)`, and `ensure` receives `Known(B)` with the source's canonical string; the consumer
   tool records the inputs it was called with. A `Keyed` and an `Item` edge each deliver.
7. **Secret-to-secret** (two synthetic secret types): a converted secret renders `[REDACTED <target>]`
   in the plan and the observer's events, and never its bytes.
8. **Isolation**: the tripwire test (the probe's name is absent from `plan.rs`, `apply.rs` and
   `describe.rs`), plus the clippy entries.
9. **The row** (decision (h)): the property tests and boundary cases.
10. **Catalogue**: `list_tools_json()["conversions"]` equals the one row; `tools` is unchanged.
11. **Document** (decision (i)): it checks clean against both catalogs (the live-shaped parity
    tests); `profile_documents.rs` plans it with `PROFILE1` `Present`; the fake-apply redaction test
    creates a profile named after `com.example.willikins-demo-two`, with no content in any output;
    equivalence item 3 passes.
12. **Equivalence**: equivalence items 1 to 4.

## Verify before relying on them

1. Two `const` evaluations under `cargo check` on Rust 1.97, which is trybuild's mode. (a) **The
   guarantee:** is a `const _: () = assert!(…)` item **nested in block position** inside a function
   body (where `conversions!` emits it, inside `vec![…]` inside `conversion_rows()`) evaluated? The
   precedent at `object.rs:105` is a module-level item, so it does not settle this. The
   `secret_to_public.rs` fixture is the proof: if it compiles, take decision (b)'s item-position
   fallback. (b) **The belt:** is an inline `const { assert!(…) }` inside a generic function that
   fails for one instantiation reported? That decides whether a third, direct-call fixture can
   exist. Record both answers in decision (b).
2. Does clippy's `disallowed_methods` fire on `catalog.registry().probe_conversion(..)` (an
   auto-deref'd `&'static TypeRegistry`)? Prove it by mutation: plant one call in `plan.rs`, see
   clippy fail, then remove it.
3. Does `disallowed_methods` honour a `#[allow]` on a `let` statement inside a `macro_rules!`
   expansion, and does it lint a local `#[macro_export]` macro's expansion at all? If it does not
   lint inside the expansion, the `#[allow]` is harmless. If it cannot be allowed there, drop the
   `__private::conversion` entry and say so here.
4. Does the `regex` crate's `$` match only at the end of the haystack (not before a trailing `\n`)?
   The boundary case in (h) settles it for `AppleBundleIdentifier`.
5. Is `FakeState: Default`, and does `willikins_providers_fake::catalog` carry every tool every
   `workflows/*.yaml` uses? If not, the characterization records `UnknownTool` for that document,
   which is still a stable characterization, but say so.
6. Is `bundle_id.ensure`'s `identifier` output `Known` at plan time when the identifier is `Absent`
   (`com.example.willikins-demo-two` is seeded, so the redaction test does not depend on this, but a
   first-time run does)? Read `observe_from`/`outputs_for`. The existing `identifier` key binding
   implies yes.
7. The exact E0080 and E0277 texts on Rust 1.97, from `TRYBUILD=overwrite`: read them before
   committing them.
8. Is the MCP tool list, with its descriptions, snapshotted anywhere? If so, the `list_tools`
   description change updates that snapshot in C6.

## Gates

All four before every commit, exactly as `CLAUDE.md` spells them. Use bare `cargo`, run each in the
background with a 600,000 ms timeout, and read the log body. Never pipe gate output through `tail`
or `tee`, and never read an exit code after a pipe.

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

Run `pgrep -x cargo` before **every** cargo command (never `pgrep -f`) and wait while it prints
anything. Never run two cargo commands at once. Pass `-j 2` on every cargo command, including a
scoped test. Never edit a tracked source file while a cargo command is building. On ENOSPC, run
`cargo clean -p` for the affected crates and restart; never delete under `target/` while any cargo
runs. If CPU shows no growth across two samples five minutes apart, the build is livelocked: kill it
and retry. During development, use crate-scoped runs (`cargo test -p willikins-types -j 2 …`,
`cargo clippy -p willikins-core --all-targets -j 2 -- -D warnings`) before each full gate.

## Tasks

One implementer, on `main`, no worktree, commits in this order. Each commit is one behaviour,
test-first, staged by path with `git commit --only <paths>`, and ends with the implementer's own
`Co-Authored-By` trailer. Do not push. Do not edit `docs/HANDOFF.md`, and do not mark this plan
Completed; the coordinator does both after re-gating.

| # | Commit (one behaviour) | Test first |
| --- | --- | --- |
| C1 | Characterize every document's check and plan (equivalence item 1). Test only. | The snapshot itself, green on the current tree |
| C2 | `willikins-types`: `Conversion`, the registry table, `TypeRegistry::new`, `probe_conversion`, `conversion_pairs`, `__private::conversion`/`convert`, `conversions!`, the `clippy.toml` entry for `__private::conversion`, the two trybuild fixtures, `Probe => ProbeSecret` | Acceptance 1, 2, and 3's mechanism half (with a `#[cfg(test)]` row) |
| C3 | The row: `impl From<AppleBundleIdentifier> for AppleProfileName`, registration in `conversion_rows`, containment doc | Acceptance 9; acceptance 3 over the production registry |
| C4 | `check` builds the edge: `Edge`, `Checked::types` retyped, step 7, the `clippy.toml` entry for `probe_conversion`, the test-site `.ty()` edits, the characterization's rendering switched to `.ty()` (snapshot unchanged), and the design-doc addendum (below) | Acceptance 4, 5 (synthetic half), tripwire half of 8 |
| C5 | `plan` and `apply` deliver through the edge: `Value::converted`, `Edge::deliver`, `ResolveCtx::{edges, deliver}`, `resolve_instance_inputs` taking `checked` | Acceptance 6, 7; equivalence item 2 |
| C6 | Catalogue: `list_tools_json()["conversions"]` and the MCP `list_tools` description | Acceptance 10 |
| C7 | The signing document, the seed, the redaction test's identifier, `positive_inputs()`, the new negative fixture, the equivalence test against the two-input predecessor; the characterization snapshot changes only as equivalence item 1 allows | Acceptance 5 (real half), 11; equivalence item 3 |

**The design-doc addendum (in C4).** Add a header line: `**Addendum:** 2026-09-23 — total
conversions (milestone 3d): a port of type B accepts a scalar A iff A = B or a registered row A → B
exists; one hop, no transitive search; secrecy only goes up, checked at compile time; check records
the chosen conversion in Checked, and plan and apply apply it without the table.` Under "Type
system", add a bullet stating the admission rule for a row: a grammar-containment fact with a
property test, secrecy monotone, and a stated argument for any public-to-secret row. Under "Tool
contract", annotate "No coercion" to say it means no coercion of a secret into a non-secret sink,
and that conversions cannot produce one.

**Recommended to the coordinator, not done by this lane:** a one-line invariant in `CLAUDE.md`
("Conversions: secrecy only goes up, at compile time; one hop; a row is a grammar fact; only `check`
probes the table"). `CLAUDE.md` is operator-owned, and this plan does not edit it.

**Budget.** Seven commits at about two hours of full gate each is roughly fourteen hours of gating
on this host, before development runs. C1 is test-only, and C6 is small. If the coordinator wants
fewer gate runs, the natural merge is C6 into C7 (both are what an agent sees). Nothing else should
merge, because each of the others is one behaviour a verifier must be able to bisect to.

## Risks

- **`Checked::types` retyping touches about fifteen test sites across four crates.** They are
  mechanical, but a missed one is a compile error in a crate the implementer did not otherwise
  touch. The full workspace gate catches it; `cargo check --workspace --all-targets -j 2` before the
  full gate catches it sooner.
- **The characterization's plan section may be brittle** if a fake tool's predicted output depends
  on anything nondeterministic. If a document's plan differs between two runs on the current tree,
  record only its fingerprint (or only its check result) and say so in the test's doc.
- **Secrecy inference** (`todos/2026-09-22-secrecy-inference.md`, agreed next) will add secrecy
  *variables* on resolver outputs. A converted edge is a use site whose requirement is the target
  type's secrecy. That plan must say so, and it must keep this milestone's compile-time rule for
  declared types. Nothing here blocks it.
- **Host.** One new trybuild pair and a few tests. No new test binary is created: every test lands in
  an existing one, deliberately, given the 143-binary, 60 GB `target/` problem another session's todo
  is addressing.

## Follow-up

`todos/2026-09-23-checked-as-a-typed-graph.md`: turn `Checked` into a typed graph that `plan`,
`apply` and `describe` consume, so they cannot meet an invalid state. `Edge` is its first brick: the
first piece of `Checked` that `plan` and `apply` *read* rather than re-derive from the document, and
the first with private fields and a `check`-only constructor.

## Verify list, answered (verifier, 2026-09-24)

Each by a run, not a reading; the runs are in `docs/research/2026-09-23-m3d-adversarial-pass.md`.

1. (a) **Yes.** The macro's block-position `const _` is evaluated by `cargo check` (C2's finding,
   and now proven the other way round: removing it makes `secret_to_public.rs` compile under
   trybuild). (b) **No.** The inline `const { }` belt in `__private::conversion` is not evaluated by
   `cargo check`: with the macro's `const _` removed, the fixture compiled. So no direct-call fixture
   can exist; the belt fires only when the call is compiled to code, and clippy refuses a direct call
   before that (item 3).
2. **Yes.** A `catalog.registry().probe_conversion(..)` call planted in `plan.rs` fails clippy
   `-D warnings` with `disallowed_methods`, through the auto-deref.
3. **Yes, and the `#[allow]` is load-bearing.** With the `#[allow]` stripped from the macro's
   expansion, clippy fails on `conversion_rows()`'s own expansion in `willikins-types`. The
   `__private::conversion` entry stays.
4. **Yes.** `AppleBundleIdentifier::parse("com.example.MyApp\n")` is refused (C3's test).
5. **Yes, with one note.** `willikins_providers_fake::empty()` carries every tool; the signing
   document's characterized plan stops at its first Doppler read against the default fake state
   (`NotFound`), before and after, which is a stable characterization.
6. **Yes.** Live, with a throwaway identifier nothing matches (`bundle_id` `create`), the profile's
   `name` and `identifier` ports were both known at plan time.
7. **Read before committing** (C2): E0080 with the secrecy message only; E0277 naming `From<A>` only.
8. **Yes.** `mcp_server__the_tool_list_and_every_schema_is_snapshotted.snap` pins the `list_tools`
   description; its one line changed in `d862203`.

## Post-flight (verifier, 2026-09-24)

Read-only `plan --live` of `workflows/appstore-signing-profile-from-doppler.yaml` against the
operator's live App Store Connect account, the credential resolved out of the sandbox Doppler
workplace in the same process. **No `profile_name` input.** Exit 0; `bundle_id`, `profile`,
`destination`, `store` `create`, the rest `compute`; `class: reversible`. The profile node's `name`
port was a known `AppleProfileName` equal, byte for byte, to the `AppleBundleIdentifier` on its
`identifier` port. Counts before and after: 5 certificates, 13 profiles, 21 bundle identifiers; the
throwaway identifier absent both times. Every provider call was a `GET`. Not marked Completed: the
coordinator does that after re-gating.
