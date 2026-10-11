//! Compile-fail tests for `Checked`'s seal (milestone 3n, decision (s2)):
//! outside `willikins_core::check`, neither a `Checked { .. }` struct
//! literal nor a read of one of its (private) fields compiles. `.stderr`
//! files were generated with `TRYBUILD=overwrite`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/sealed/fail/*.rs");
}
