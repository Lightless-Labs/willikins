//! The five live Doppler tools. Their [`willikins_core::ToolSpec`]s must
//! equal, field for field, `willikins_providers_fake`'s tools of the same
//! names — pinned in `tests/catalog_parity.rs` by comparing each spec's
//! JSON form (`ToolSpec` derives `Serialize` but not `PartialEq`) and by
//! an insta snapshot of all five.

mod config_ensure;
mod project_ensure;
mod secret_get;
mod service_token_ensure;
mod service_token_rotate;

pub use config_ensure::DopplerConfigEnsure;
pub use project_ensure::DopplerProjectEnsure;
pub use secret_get::DopplerSecretGet;
pub use service_token_ensure::DopplerServiceTokenEnsure;
pub use service_token_rotate::DopplerServiceTokenRotate;
