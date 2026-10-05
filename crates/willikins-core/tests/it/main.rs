//! `willikins-core`'s integration tests, built as one binary. One module per
//! former top-level `tests/<stem>.rs` file; see
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md`.

#[path = "../common/mod.rs"]
mod common;

mod apply;
mod apply_adversarial;
mod apply_error_serde;
mod apply_gates;
mod apply_replaces;
mod check;
mod check_adversarial;
mod error_report_redaction;
mod expose_secret_guard;
mod operator_acknowledge_document;
mod plan;
mod plan_adversarial;
mod plan_error_serde;
mod plan_gates;
mod plan_updates;
mod redaction;
mod redaction_adversarial;
mod schema_generation;
mod secret_literal_guard;
mod sink_token_guard;
mod test_layout_guard;
mod value_render_parse_round_trip;
