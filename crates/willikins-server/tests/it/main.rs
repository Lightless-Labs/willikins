//! `willikins-server`'s default-built integration tests, in one binary
//! (milestone 3k, `docs/plans/2026-10-05-milestone-3k-faster-gates.md`,
//! task T13). One module per former top-level `tests/<stem>.rs` file.

#[path = "../common/mod.rs"]
mod common;

mod acceptance_13_trusted_directory;
mod acceptance_7_identity;
mod acceptance_8_plan_identity;
mod acceptance_read_ops;
mod acknowledgement_gate_blocked_resume;
mod adversarial_10a;
mod adversarial_10b;
mod adversarial_13;
mod binary_startup;
mod blocking_pool_13;
mod composition_s1;
mod composition_s2;
mod deploy_host_headers;
mod fake_catalog_env;
mod file_journal_round_trip;
mod hash_token;
mod http_server;
mod http_smoke;
mod image_contents;
mod mcp_output_schema_conformance;
mod mcp_server;
mod plan_failure_message;
mod readme_variables;
mod send_sync;
mod serve_http_deploy_pins;
