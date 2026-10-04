//! The twelve live Doppler tools (milestone 3's config-inheritance
//! additions: `doppler.config.inheritable.ensure` and
//! `doppler.config.inherits.ensure`, alongside the original five, plus
//! the sink `doppler.secret.set`; milestone 3e's task B1 added
//! `doppler.branch_config.ensure`; task R3 added the gate
//! `doppler.config.inheritable.gate`; milestone 3j task B2 added the gate
//! `doppler.secret_name.gate`, not yet registered in any catalog). Their
//! [`willikins_core::ToolSpec`]s must equal, field for field,
//! `willikins_providers_fake`'s tools of the same names — pinned in
//! `tests/catalog_parity.rs` by comparing each spec's JSON form
//! (`ToolSpec` derives `Serialize` but not `PartialEq`) and by an insta
//! snapshot of all eight.

mod branch_config_ensure;
mod config_ensure;
mod config_inheritable_ensure;
mod config_inheritable_gate;
mod config_inherits_ensure;
mod project_ensure;
mod project_member_ensure;
mod secret_get;
mod secret_name_gate;
mod secret_set;
mod service_token_ensure;
mod service_token_rotate;
mod value_get;

pub use branch_config_ensure::DopplerBranchConfigEnsure;
pub use config_ensure::DopplerConfigEnsure;
pub use config_inheritable_ensure::DopplerConfigInheritableEnsure;
pub use config_inheritable_gate::DopplerConfigInheritableGate;
pub use config_inherits_ensure::DopplerConfigInheritsEnsure;
pub use project_ensure::DopplerProjectEnsure;
pub use project_member_ensure::DopplerProjectMemberEnsure;
pub use secret_get::DopplerSecretGet;
pub use secret_name_gate::DopplerSecretNameGate;
pub use secret_set::DopplerSecretSet;
pub use service_token_ensure::DopplerServiceTokenEnsure;
pub use service_token_rotate::DopplerServiceTokenRotate;
pub use value_get::DopplerValueGet;
