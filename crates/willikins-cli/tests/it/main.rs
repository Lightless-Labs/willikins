//! One integration-test binary for `willikins-cli`'s default-built
//! tests, per milestone 3k (`docs/plans/2026-10-05-milestone-3k-faster-gates.md`).
//! Each module below used to be its own `tests/<stem>.rs` target; `git
//! log --follow` on a moved file still finds its history. The gated
//! `tests/live_smoke.rs` and the three private, gitignored
//! `tests/operator_*.rs` targets stay outside this binary (decisions
//! (d3) and (d4)).

#[path = "../common/mod.rs"]
mod common;

mod acceptance;
mod acceptance_11_mcp_parity;
mod acceptance_11_parity;
mod acceptance_m3a_buildkite;
mod adversarial;
mod adversarial_11;
mod adversarial_pass_1_cli;
mod apply_and_journal;
mod appstore_profile_apply_redaction;
mod cli;
mod identifier_masking;
mod inherited_secret_gate_document;
mod no_gh_writes_guard;
mod prerendered_identifier_guards;
mod serve_and_live;
mod smoke_parity;
mod teardown_script;
mod unique_temp_prefixes;
