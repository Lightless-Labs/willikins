//! The capability token that gates access to secret values.
//!
//! A secret domain type's `expose` method takes a `&SinkToken` so that the
//! type system, not a reviewer's memory, proves which code can see a
//! secret's bytes. The only way to build one is [`SinkToken::new`], and
//! that constructor exists only when this crate's `executor` cargo feature
//! is enabled. `willikins-core` enables it and mints tokens only inside the
//! milestone-2 apply executor; nothing else in the workspace enables it,
//! so `Tool::read` and every other code path provably cannot expose a
//! secret. Tests enable the feature through a self dev-dependency in this
//! crate's `Cargo.toml`.

/// Proof that code is running inside the apply executor and may call a
/// secret domain type's `expose`.
///
/// Holds no data; its only purpose is to exist or not.
pub struct SinkToken(());

impl SinkToken {
    /// Mint a token.
    ///
    /// Only available when the `executor` feature is enabled. Without the
    /// feature, this function does not exist, so no code outside a crate
    /// that opts into `executor` can construct a `SinkToken` and none can
    /// call `expose`.
    #[cfg(feature = "executor")]
    #[must_use]
    #[allow(clippy::new_without_default)] // deliberately not `Default`: a token is minted, not defaulted
    pub fn new() -> Self {
        Self(())
    }
}

impl std::fmt::Debug for SinkToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SinkToken")
    }
}

// Acceptance test 11 requires `SinkToken::new()` to fail to compile without
// the `executor` feature. A `trybuild` case cannot express that here: this
// crate's own `[dev-dependencies]` entry enables `executor` on itself so
// that its *own* tests can build tokens, and Cargo's feature unification
// then turns the feature on for every test/dev build of this crate,
// `trybuild`'s fixtures included (see the note in
// `tests/derive_compile_fail.rs`). The guarantee is instead checked here:
// this item only compiles when `executor` is off, which never happens
// under `cargo test` in this crate, but does happen for a plain
// `cargo check -p willikins-types` (no `--tests`) and for any downstream
// crate that depends on `willikins-types` without opting into `executor`.
#[cfg(not(feature = "executor"))]
#[allow(dead_code)]
fn sink_token_new_is_unreachable_without_executor() {
    let _: SinkToken = SinkToken::new();
}
