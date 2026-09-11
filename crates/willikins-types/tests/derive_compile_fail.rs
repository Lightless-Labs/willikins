//! Compile-fail tests for `#[derive(DomainType)]`, via `trybuild`.
//!
//! Covers, per acceptance test 11 and the milestone plan's derive
//! contract: `#[domain(secret)]` on `String` storage, `secrecy::SecretString`
//! storage without `secret`, an invalid `pattern`, a non-newtype input
//! (an enum), and calling `serde_json::to_string` on a secret type (which
//! fails because secret types generate no `Serialize`, not because of a
//! macro-time compile error).
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
//! The guarantee is instead checked structurally, in
//! `src/sink.rs`: `SinkToken::new` is declared under
//! `#[cfg(feature = "executor")]`, and a private function under
//! `#[cfg(not(feature = "executor"))]` calls it, so that function only
//! compiles (and only then proves the call resolves) when the feature is
//! off — which is exactly the condition this crate's own gates never
//! exercise (`cargo test` and `cargo clippy --all-targets` both unify
//! `executor` on), but which is exactly the condition every other crate
//! in this workspace is in, since only `willikins-core`'s milestone-2
//! apply executor enables `executor` on purpose.
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/derive/fail/*.rs");
}
