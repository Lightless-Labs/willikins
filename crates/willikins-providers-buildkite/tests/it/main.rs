//! This crate's default-built integration tests, folded into one binary
//! per milestone 3k (`docs/plans/2026-10-05-milestone-3k-faster-gates.md`,
//! decision (d1)): one `mod` per former top-level `tests/<stem>.rs` file.
//! The two `live-tests`-gated targets, `tests/live_write_cycle.rs` and
//! `tests/live_bootstrap_cycle.rs`, stay top-level per decision (d3) and
//! are unaffected by this binary.

#[path = "../common/mod.rs"]
mod common;

mod bootstrap_client_mock;
mod buildkite_token_documents;
mod catalog_parity;
mod cluster_get_mock;
mod credential_env;
mod fake_agrees_with_live;
mod live_probe;
mod pipeline_bootstrap_ensure_mock;
mod pipeline_ensure_mock;
mod redaction;
