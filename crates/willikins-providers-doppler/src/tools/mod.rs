//! The eight live Doppler tools (milestone 3's config-inheritance
//! additions: `doppler.config.inheritable.ensure` and
//! `doppler.config.inherits.ensure`, alongside the original five, plus
//! the sink `doppler.secret.set`). Their [`willikins_core::ToolSpec`]s
//! must equal, field for field, `willikins_providers_fake`'s tools of the
//! same names — pinned in `tests/catalog_parity.rs` by comparing each
//! spec's JSON form (`ToolSpec` derives `Serialize` but not `PartialEq`)
//! and by an insta snapshot of all eight.

mod config_ensure;
mod config_inheritable_ensure;
mod config_inherits_ensure;
mod project_ensure;
mod secret_get;
mod secret_set;
mod service_token_ensure;
mod service_token_rotate;

pub use config_ensure::DopplerConfigEnsure;
pub use config_inheritable_ensure::DopplerConfigInheritableEnsure;
pub use config_inherits_ensure::DopplerConfigInheritsEnsure;
pub use project_ensure::DopplerProjectEnsure;
pub use secret_get::DopplerSecretGet;
pub use secret_set::DopplerSecretSet;
pub use service_token_ensure::DopplerServiceTokenEnsure;
pub use service_token_rotate::DopplerServiceTokenRotate;
