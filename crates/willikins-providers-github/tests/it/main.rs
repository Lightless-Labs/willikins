//! This crate's integration tests, folded into one binary per
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md` (milestone 3k,
//! task T8). Each module below used to be its own top-level
//! `tests/<stem>.rs` file and its own statically linked test executable;
//! now all of them link once, as `tests/it/main.rs`. The gated
//! `tests/live_scaffold_cycle.rs` (`required-features = ["live-tests"]`)
//! stays a separate top-level target, per decision (d3).

#[path = "../common/mod.rs"]
mod common;

mod actions_secret_ensure_mock;
mod catalog_check_parity;
mod catalog_parity;
mod fake_agrees_with_live;
mod github_token_documents;
mod live_probe;
mod live_write_cycle;
mod redaction;
mod repo_ensure_mock;
mod repo_get_mock;
mod scaffold_ensure_mock;
mod scaffold_fake_agrees_with_live;
