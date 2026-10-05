//! This crate's default-built integration tests, as one binary (target
//! `it`), per `docs/plans/2026-10-05-milestone-3k-faster-gates.md`
//! milestone 3k. Each module below was formerly its own
//! `tests/<stem>.rs` file; `git log --follow` on a module file still
//! finds its history. The three `required-features = ["live-tests"]`
//! targets (`live_write_cycle`, `live_project_member_cycle`,
//! `live_secret_name_gate_cycle`) stay top-level `tests/*.rs` files and
//! are not part of this binary.

#[path = "../common/mod.rs"]
mod common;

mod apple_signing_credential_chain;
mod branch_config_ensure_mock;
mod catalog_check_parity;
mod catalog_parity;
mod config_ensure_mock;
mod config_inheritable_ensure_mock;
mod config_inherits_ensure_mock;
mod credential_env;
mod fake_agrees_with_live;
mod live_catalog;
mod live_probe;
mod project_ensure_mock;
mod project_member_client_mock;
mod project_member_ensure_mock;
mod provider_messages;
mod redaction;
mod retry_budget;
mod secret_get_mock;
mod secret_name_gate_mock;
mod secret_set_mock;
mod secret_set_provenance;
mod service_token_ensure_mock;
mod service_token_rotate_mock;
mod value_get_mock;
