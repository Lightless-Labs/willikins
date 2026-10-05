//! This crate's default-built integration tests, as one binary. See
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md` (task T12). The
//! two `required-features = ["live-tests"]` targets, `live_write_cycle`
//! and `live_capability_cycle`, stay top-level and are not declared here.

mod bundle_id_capability_ensure_mock;
mod bundle_id_documents;
mod bundle_id_ensure_mock;
mod capability_documents;
mod capability_gate_mock;
mod catalog_parity;
mod certificate_get_mock;
mod certificate_refusals_carry_no_serial;
mod fake_agrees_with_live;
mod live_probe;
mod no_certificate_writes_guard;
mod profile_create_response_shapes;
mod profile_documents;
mod profile_ensure_mock;
mod redaction;
