# Milestone 3m adversarial pass (X1, independent), over R1

**Date:** 2026-10-12
**Reviewer:** Claude Opus 5.5 (independent: wrote none of the code under review)
**Scope:** task R1 of `docs/plans/2026-10-05-milestone-3m-safe-rollback.md`, commits `c970ef3` (`Reversal`,
`Within`, `Side`, `UNDECLARED`, `Tool::reversal()`), `e9bf9c5` (`Catalog::insert` validates the tool's own side),
`48fac7c` (an intra-doc link fix). Every later 3m task (R2 onward) is unwritten; the attacks that target them are
recorded as forward requirements, not as findings.
**Method:** read the plan in full (both 2026-10-12 addenda and the Reviewed lines) and the three diffs; attacked the
declaration surface for its use by R3, R5a/R5b, P1-P7 and F1; proved six conclusions by mutation; fixed the two
real defects test-first.

## Verdict

R1 is sound on the plan's top priority. A `Reversal` states **capability only**: its three variants
(`rollback.rs` lines 77-99) carry `needs`, `within` and `losses` (`Delete`), `within` and `otherwise` (`Contained`),
and `why` (`Retained`). No field says whether to delete, and `losses` exists only on `Delete`. The default is
`Retained { why: UNDECLARED }`, so a tool that declares nothing can never be undone. Two declaration shapes that
`Catalog::insert` could see on the tool's own side, but admitted, would let a later rider vouch for contents it never
wrote. Both are fixed (below).

## Findings

### F1 (fixed, `d916efc`): a `Within` that maps no port rides with every container

Decision 3: an instance rides with a container instance when "every mapped port pair holds equal `Value`s". With
`ports: &[]` that holds vacuously, for every instance of the container tool in the run. R1 admitted it on `Delete`
and `Contained` alike. Consequence once R5b lands: such an instance rides with every container of that tool being
deleted, and decision 3 hands every rider to the container's inverse as **tolerated contents** (`Created.riders`). A
`doppler.secret.set` declared this way would make a deleted config tolerate, as this run's own, a hand-set secret
that merely shares a name with one the run wrote into a *different* config: a container deleted despite foreign
contents, the exact case trust boundary 4 and decision 7 exist to refuse. Decision 3's "when it misfires it can only
cause an under-deletion" does not hold for tolerance.

Fix: `ReversalError::WithinMapsNoPort { tool }`, refused at insert. Tests
`insert_rejects_a_within_that_maps_no_port` and `..._on_a_contained_reversal_too`.

### F2 (fixed, `9d9fcf9`): a `Within` pair on a secret own port compares nothing

Trust boundary 7 and decision 3: the journal keeps only a redaction marker for a secret (`Value`'s `Serialize` emits
`render()`, the type's fixed marker), and a rider's secret ports are re-parsed as `Value::Unknown`. `Value`'s
`PartialEq` (`value.rs`, `(Unknown, Unknown) => true` for one type) makes two such values equal. So a `Within` whose
every pair is secret matches any two instances, with the same tolerance consequence as F1. P7's container-side check
(types equal) would pass a secret-to-secret pair. R1 admitted it.

Fix: `ReversalError::WithinPortSecret { port }`, refused at insert for a `PortType::AnySecret` or registry-secret
own-side port, as `validate_gate` already refuses a secret subject. `validate_reversal` became a method so it can read
the catalog's registry. `needs` stays free to name a secret credential port (decision 5 requires it; pinned by
`insert_accepts_a_secret_needs_port`). Tests `insert_rejects_a_within_own_side_port_of_a_secret_type` and
`..._of_any_secret`.

## Mutations (each restored from a saved copy, touched, and `cmp`-confirmed byte-identical)

Scoped run: `cargo test -p willikins-core --lib -j 2 -- catalog::tests:: rollback::tests::`.

| # | Mutation | Result |
| --- | --- | --- |
| M1 | `insert` no longer calls `validate_reversal` (replaced by `Ok(())`) | killed: 5 `insert_rejects_*` tests red. The hook is live, not dead code |
| M2 | the pure guard inverted (`if !spec.pure`) | killed: 9 red, including `insert_rejects_a_pure_tool_declaring_a_reversal` |
| M3 | the loss pattern widened to `^[a-z][a-z0-9_]*$` | killed: `loss_name_regex_rejects_digits_and_uppercase` red |
| M4 | `Tool::reversal()`'s default changed to `Delete { needs: &[], within: &[], losses: &[] }` | killed: 6 red (`tool_reversal_defaults_to_retained_undeclared`, plus every insert of a pure dummy, refused as `PureToolDeclaresReversal`). Within the scoped run the default is pinned only by core's unit tests; P7's table is what will pin it per live tool |
| M5 | after F1: the empty-mapping check disabled (`if false && ...`) | killed: both `insert_rejects_a_within_that_maps_no_port*` tests red |
| M6 | after F2: the secret check disabled (`if false && ...`) | killed: both `insert_rejects_a_within_own_side_port_of_*` secret tests red |

After both fixes, the crate-scoped run (`cargo test -p willikins-core -j 2 --no-fail-fast`) passes: 265 unit, 302
integration, 2 doctests. `cargo fmt --all --check` and `cargo clippy -p willikins-core --all-targets -j 2 -- -D
warnings` are clean.

## Priorities checked, and where each stands after R1

- **Policy reintroduced in a tool declaration:** none (verdict above). The default is `Retained`, and a pure tool can
  declare nothing else (M2).
- **A delete reachable on a node that declares nothing, from a document, or without a `TeardownToken`:** no delete
  path exists yet. `TeardownToken` is named nowhere in `crates/`, no `Inverse` exists, and no tool name ending
  `.delete` is registered. R1 touches only `willikins-core` (`catalog.rs`, `lib.rs`, `rollback.rs`, `tool.rs`).
- **Any App Store call outside mocks:** none. R1 touches no provider crate.
- **A value or full identifier in a reason:** `ReversalError` carries only declaration strings (port, loss and tool
  names, all `&'static str` in the source). `why` and `otherwise` are `&'static str`.
- **Refused tool left in the catalog:** no. Every validation precedes `self.tools.insert`.
- **Privacy:** the three commits and both fixes name no private organisation, application, host or path.
- **A tool wrapper silently dropping `reversal()`:** no non-test `Tool` impl delegates to another tool, so no forwarder
  can lose the method. A future wrapper must forward `reversal()` (and R3's `inverse()`), or its tool reads `Retained`.

## Forward requirements (not R1 defects; for the rows that own them)

1. **R3** must add "`Delete` exactly when `inverse()` is `Some`". Until then nothing stops a tool declaring `Delete`
   with no inverse. No tool overrides `reversal()` today (only the catalog's own test dummy does), so nothing is exposed.
2. **R5a** adds `rollback.rs` to 3n's ratchet and tripwires: the file already holds one non-test `.expect(` (the
   `LazyLock` loss regex, line 24). Count it in the pin, or replace it with a fallible construction, before the pin is
   written.
3. **R5b** must not recurse unboundedly: a `Within` may name its own tool (nothing refuses it), so two instances of
   one tool with equal mapped values could each ride with the other. Compute riding as a fixed point from the
   instances whose own step is `Delete`, or refuse a self-naming `Within` (P7 is the natural place).
4. **P1-P5 before P7:** the container side of every `Within` is unchecked until P7 (and P6 for the fakes). A
   misspelled container tool rides with nothing. That direction is safe (under-deletion, and the forward plan lists
   it), but P7 should land before L1-L4 run.
5. **Accepted but inert declarations:** `Contained { within: &[] }` can never ride (it behaves as `Retained` under
   another name); an empty `why` or `otherwise`, and duplicate `needs` or `losses` entries, are admitted. None is
   unsafe. P7's snapshot will show them; refuse them there if they appear.
6. **Rustdoc is not a gate.** `48fac7c` fixed a broken intra-doc link that no scoped gate caught. A
   `RUSTDOCFLAGS="-D warnings" cargo doc -p willikins-core --no-deps` run finds no error in `catalog.rs`,
   `rollback.rs`, `tool.rs` or `lib.rs`, but 65 older ones elsewhere in the crate (mostly public docs linking private
   items in `apply.rs`, `check.rs`, `compose.rs`, `describe.rs`, `plan.rs`, `disclosure.rs`). Later 3m rows should not
   rely on rustdoc to catch a dangling forward reference.
7. **`reversal()` is read once, at insert.** Nothing pins it constant afterwards, the same posture as `gate()`. The
   plan accepts it ("static, like a tool's class"). R5a and F1 should read it through the same `Arc<dyn Tool>` the
   catalog validated, never a re-built tool.
