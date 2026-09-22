//! One live [`willikins_core::Tool`] implementation per module:
//! `appstore.bundle_id.ensure` and `appstore.bundle_id_capability.ensure`.

pub mod bundle_id_capability_ensure;
pub mod bundle_id_ensure;

pub use bundle_id_capability_ensure::AppstoreBundleIdCapabilityEnsure;
pub use bundle_id_ensure::AppstoreBundleIdEnsure;
