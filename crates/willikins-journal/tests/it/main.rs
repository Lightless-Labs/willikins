//! `willikins-journal`'s integration tests, built as one binary. One
//! module per former top-level `tests/<stem>.rs` file; see
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md`.

#[path = "../common/mod.rs"]
mod common;

mod acceptance_10_journal;
mod adversarial_pass_1;
mod event_shapes;
mod locking;
mod post_pass_2_shapes;
mod pre_pass_2_replay;
mod read_only_replay;
mod redaction_attacks;
mod redaction_by_construction;
mod replay_integrity;
mod run_and_journal_refusal;
mod schema_generation;
mod views;
