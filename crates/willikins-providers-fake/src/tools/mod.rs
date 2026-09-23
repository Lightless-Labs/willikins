//! One fake [`Tool`](willikins_core::Tool) implementation per module, one
//! module per row of the fake-provider port table in
//! `docs/plans/2026-09-11-milestone-1-core.md`.
//!
//! `naming.v1` and `template.render` — pure, provider-independent — moved
//! to the `willikins-tools` crate; [`crate::catalog`](crate::catalog())
//! registers them from there alongside this crate's own eight tools.

pub mod appstore_bundle_id_capability_ensure;
pub mod appstore_bundle_id_ensure;
pub mod appstore_certificate_get;
pub mod appstore_profile_ensure;
pub mod buildkite_cluster_get;
pub mod buildkite_pipeline_ensure;
pub mod doppler_config_ensure;
pub mod doppler_config_inheritable_ensure;
pub mod doppler_config_inherits_ensure;
pub mod doppler_project_ensure;
pub mod doppler_secret_get;
pub mod doppler_secret_set;
pub mod doppler_service_token_ensure;
pub mod doppler_service_token_rotate;
pub mod doppler_value_get;
pub mod fake_irreversible_ensure;
pub mod fake_secret_list;
pub mod github_actions_secret_ensure;
pub mod github_repo_ensure;
pub mod signoz_ingestion_key_ensure;

pub use appstore_bundle_id_capability_ensure::FakeAppstoreBundleIdCapabilityEnsure;
pub use appstore_bundle_id_ensure::FakeAppstoreBundleIdEnsure;
pub use appstore_certificate_get::FakeAppstoreCertificateGet;
pub use appstore_profile_ensure::FakeAppstoreProfileEnsure;
pub use buildkite_cluster_get::FakeBuildkiteClusterGet;
pub use buildkite_pipeline_ensure::FakeBuildkitePipelineEnsure;
pub use doppler_config_ensure::DopplerConfigEnsure;
pub use doppler_config_inheritable_ensure::DopplerConfigInheritableEnsure;
pub use doppler_config_inherits_ensure::DopplerConfigInheritsEnsure;
pub use doppler_project_ensure::DopplerProjectEnsure;
pub use doppler_secret_get::DopplerSecretGet;
pub use doppler_secret_set::DopplerSecretSet;
pub use doppler_service_token_ensure::DopplerServiceTokenEnsure;
pub use doppler_service_token_rotate::DopplerServiceTokenRotate;
pub use doppler_value_get::DopplerValueGet;
pub use fake_irreversible_ensure::FakeIrreversibleEnsure;
pub use fake_secret_list::FakeSecretList;
pub use github_actions_secret_ensure::GitHubActionsSecretEnsure;
pub use github_repo_ensure::GitHubRepoEnsure;
pub use signoz_ingestion_key_ensure::SigNozIngestionKeyEnsure;
