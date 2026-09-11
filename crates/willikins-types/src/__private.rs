//! Re-exports consumed by `#[derive(DomainType)]`-generated code.
//!
//! Not part of the public API of this crate: generated code reaches
//! `serde`, `schemars`, `secrecy`, `regex`, and `serde_json` through these
//! paths so that a crate using `#[derive(DomainType)]` needs no extra
//! dependencies of its own. Names and shapes here may change without
//! notice; do not use this module directly.
#![allow(missing_docs)]

pub use regex;
pub use schemars;
pub use secrecy;
pub use serde;
pub use serde_json;
