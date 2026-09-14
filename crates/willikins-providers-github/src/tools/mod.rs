//! The two live GitHub tools: `github.repo.ensure` and
//! `github.actions_secret.ensure`. Their [`willikins_core::ToolSpec`]s
//! must equal, field for field, `willikins_providers_fake`'s tools of the
//! same names — pinned in `tests/catalog_parity.rs` by comparing each
//! spec's JSON form (`ToolSpec` derives `Serialize` but not `PartialEq`)
//! and by an insta snapshot of both.

mod actions_secret_ensure;
mod repo_ensure;

pub use actions_secret_ensure::GitHubActionsSecretEnsure;
pub use repo_ensure::GitHubRepoEnsure;
