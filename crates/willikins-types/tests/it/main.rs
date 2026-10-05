//! `willikins-types`'s integration tests, built as one binary. See
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md`, decision (d1).

mod catalog;
mod derive_compile_fail;
mod derive_pass;
mod identifier_masking;
mod message_bounds;
mod naming_adversarial;
mod naming_properties;
mod naming_v1_properties;
mod redaction_adversarial;
