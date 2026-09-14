//! Compile-fail tests for `Credential`, via `trybuild`: acceptance test 4
//! asks that `serde_json::to_string(&credential)` and
//! `format!("{credential}")` both fail to compile. `.stderr` files were
//! generated with `TRYBUILD=overwrite` (see
//! `willikins-types/tests/derive_compile_fail.rs` for the same pattern).

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
