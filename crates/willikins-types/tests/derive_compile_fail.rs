//! Compile-fail tests for `#[derive(DomainType)]`, via `trybuild`.
//!
//! Covers, per acceptance test 11 and the milestone plan's derive
//! contract: `#[domain(secret)]` on `String` storage, `secrecy::SecretString`
//! storage without `secret`, an invalid `pattern`, a non-newtype input
//! (an enum), a generic struct, and calling `serde_json::to_string` on a
//! secret type (which fails because secret types generate no `Serialize`,
//! not because of a macro-time compile error).
//!
//! `.stderr` files were generated with `TRYBUILD=overwrite`.
//!
//! # `SinkToken::new()` without the `executor` feature
//!
//! Acceptance test 11 also asks for a `trybuild` case proving
//! `SinkToken::new()` does not compile without this crate's `executor`
//! feature. That case cannot be expressed here: `trybuild` compiles its
//! fixtures against the same dependency graph as this test binary, and
//! this crate's own `[dev-dependencies]` entry
//! (`willikins-types = { path = ".", features = ["executor"] }`) exists
//! so that *this crate's own tests* — this file's fixtures included — can
//! build `SinkToken`s. Cargo's feature unification then turns `executor`
//! on for every test/dev build of this package, so a fixture that calls
//! `SinkToken::new()` would compile here regardless of whether it "should"
//! be allowed to.
//!
//! A same-crate `#[cfg(not(feature = "executor"))]` item that calls
//! `SinkToken::new()` to "prove" the call fails doesn't work either: such
//! an item is only ever compiled in the one configuration where the call
//! is invalid, so it would break every ordinary build of this crate
//! without `executor` (every crate in this workspace except
//! `willikins-core`) instead of proving anything.
//!
//! The guarantee is therefore structural, not a runnable check: in
//! `src/sink.rs`, `SinkToken::new` is declared under
//! `#[cfg(feature = "executor")]` and nothing else in that module
//! references it. The nearest automated check is `cargo check -p
//! willikins-types` (no `--tests`, so no dev-dependency feature
//! unification) succeeding with `executor` off — which every gate that
//! isn't `--all-targets`/`cargo test` already exercises.
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/derive/fail/*.rs");
}
