//! `appstore.bundle_id_capability.ensure`: switches on one capability of
//! an already-registered App Store Connect bundle id.
//! `docs/research/2026-09-16-app-store-connect.md`, section 2, is every
//! fact this tool rests on.
//!
//! # Read: list the parent, match on `capabilityType`
//!
//! The capability resource has no `GET` of its own -- the only
//! documented way to read one is `GET
//! /v1/bundleIds/{id}/bundleIdCapabilities`, listing every capability
//! the parent bundle id already carries. So `read` first resolves
//! `identifier` to the parent bundle id's own Apple-assigned `id`
//! (`GET /v1/bundleIds?filter[identifier]=...`, compared exactly, the
//! same client-side comparison `appstore.bundle_id.ensure` uses and for
//! the same reason -- see that tool's own module doc). This makes this
//! tool self-contained: a document can bind `identifier` to a literal or
//! a workflow input directly, without chaining through
//! `appstore.bundle_id.ensure`'s own `id` output first, and it re-derives
//! `id` from `identifier` the same way either way.
//!
//! If no bundle id has `identifier` at all, this tool's `read` reports
//! [`willikins_core::Observation::Absent`] (predicting this tool's own
//! `capability` output, unchanged from what was requested) rather than
//! refusing outright. Milestone 3e's task 1 originally refused here with
//! [`willikins_core::ToolErrorKind::NotFound`], reasoning that `Absent`
//! would wrongly imply *this* tool can create what is missing -- but
//! `plan` reads every node whose key ports are known
//! (`crates/willikins-core/src/plan.rs`), and `appstore.bundle_id.ensure`'s
//! own `Absent` predicts its `identifier` output from its input, so a
//! capability node bound from `steps.app_id.identifier` (decision (c),
//! `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`) has a known key at
//! plan time and is read *before* the bundle id is ever created -- failing
//! every such document at `plan`, before any write, reproduced against the
//! fake catalog (that plan's 2026-09-28 T3-blocking addendum). The
//! in-crate precedent is [`crate::tools::AppstoreProfileEnsure`], which
//! already reports `Absent` for an unregistered parent identifier and
//! only refuses with `NotFound` at `ensure`'s create path, once the
//! parent is confirmed still missing at apply time. This tool now follows
//! the same shape: `read` (and `plan`, which only ever calls `read`) is
//! honest that the capability is absent because nothing is there yet;
//! `ensure` -- the only place that would actually need to create a bundle
//! id -- still refuses with `NotFound` naming `appstore.bundle_id.ensure`,
//! because this tool still cannot create one itself. A document names
//! that tool first; it may now simply run first in the same plan.
//!
//! # `APP_GROUPS`, `APPLE_PAY`, `ICLOUD`: flip-on-able, never
//! configurable -- and what this tool does about it
//!
//! Apple names six capabilities needing an extra portal step (research
//! note, section 2, "Apple names the six capabilities that need extra
//! steps"). Three of those six -- `APP_GROUPS`, `APPLE_PAY`, `ICLOUD` --
//! need an *identifier association* this API has no way to express at
//! all: `BundleIdCapabilityCreateRequest.relationships` names only
//! `bundleId`, and `CapabilitySetting.key`'s three-member enum covers
//! Sign in with Apple, data protection, and iCloud's Xcode-compatibility
//! version -- never "which app group(s)" or "which merchant ID(s)" or
//! "which iCloud container(s)". Switching one of these three on through
//! this API therefore leaves the identifier in a state Apple's own portal
//! would call half-configured: the capability flag is set, but nothing
//! is attached to it, and (per the research note) enabling a capability
//! is also documented to affect every eligible platform's provisioning
//! profiles -- a side effect a caller who only meant to *check* whether
//! the flag could be flipped would not expect.
//!
//! **Decision, and why:** for these three capability types, `ensure`
//! refuses outright ([`willikins_core::ToolErrorKind::Invalid`], naming
//! the capability and the portal step it needs) and never calls `POST`
//! at all -- not "succeeds with a caveat output", not "creates a
//! half-configured flag". Two reasons, both from the live-account
//! guardrails this task was built under: a refusal that never touches
//! the account can be relaxed later without breaking any document that
//! already runs clean, while a "succeeds, but incompletely" observation
//! cannot be tightened later without breaking a document that came to
//! rely on it reading `Present`. And a tool that leaves a real,
//! production bundle id in a state its own author would call incomplete
//! is exactly the kind of surprise this task's operator asked this crate
//! to stop and report rather than improvise around. The other three of
//! Apple's six (Sign in with Apple, data protection, push notifications)
//! are fully expressible through `CapabilitySetting`, not merely
//! flip-on-able, so none of the three is refused here -- but they are
//! not all alike below that: `PUSH_NOTIFICATIONS` is a complete, valid
//! state with no setting at all, while `DATA_PROTECTION` and
//! `APPLE_ID_AUTH` each require one and are refused at `read` without it
//! -- see the next section.
//!
//! `read` still reports `Present`/`Absent` honestly for all 28 capability
//! types, including these three -- refusing only happens in `ensure`,
//! and only on the `Absent` → create path. A document that names one of
//! these three and finds it already `Present` (enabled by a human in the
//! portal, association and all) converges with no error, same as any
//! other capability.
//!
//! # `setting`: an optional port, paired to `capability`, checked before
//! any request
//!
//! Milestone 3e (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`,
//! decision (d)) adds one more input, `setting:
//! `[`AppleCapabilitySetting`], **optional** -- the first port on any
//! tool in this workspace that is. `DATA_PROTECTION` requires a
//! `DATA_PROTECTION_PERMISSION_LEVEL` setting; `APPLE_ID_AUTH` requires
//! an `APPLE_ID_AUTH_APP_CONSENT` one; every other capability -- push
//! notifications among them -- takes none at all. That pairing relates
//! two ports of *this* tool, and `check` types each port independently
//! with no per-tool cross-port hook, so it cannot hold it: [`Self::read`]
//! enforces it first instead, before `client_for` ever mints a
//! credential or a request leaves the process
//! ([`willikins_core::ToolErrorKind::Invalid`], naming the capability and
//! the setting key it needs or forbids). Because `plan` calls `read` on
//! every node before `ensure` runs on any of them, a mismatched pair
//! fails the whole plan up front, never mid-apply.
//!
//! **Reading a setting back.** The capability row is still found by
//! `capabilityType` exactly as before; when a `setting` was requested,
//! every option under the row's `settings[]` entries for that key is
//! collected first. **This rule changed on 2026-09-28**, from a live
//! observation (the milestone 3e plan's second live capability cycle):
//! after creating `DATA_PROTECTION` with
//! `DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH`,
//! Apple's list endpoint echoed back exactly one option under that key,
//! carrying `key` alone -- **no `enabled` field at all** -- so decision
//! (d)'s original assumption ("the selected option is the one marked
//! `enabled: true`") could never match a real row, and a correct create
//! always read back as `Mismatch`. The rule now branches on whether any
//! collected option carries an `enabled` field:
//!
//! - **No option carries `enabled` at all** (Apple's own shape, observed
//!   live): the listed option *is* the selection. Exactly one option
//!   listed, and it is the requested one →
//!   [`willikins_core::Observation::Present`]; more than one option
//!   listed, or none at all, → `Mismatch` -- ambiguous or absent, never a
//!   guess at which one is real.
//! - **At least one option carries an `enabled` field** (never observed
//!   live; kept as a defensive fallback and exercised only by a mock
//!   fixture): the pre-2026-09-28 rule applies unchanged -- exactly one
//!   option marked `enabled: true`, and that option the requested one, is
//!   `Present`; anything else (a different enabled option, two enabled
//!   options, none enabled) is `Mismatch`.
//!
//! Either way, the key or `settings` missing from the row entirely
//! collects zero options, which is `Mismatch` under both branches.
//! Every `Mismatch` →
//! [`willikins_core::Observation::Mismatch`] naming the `setting` port,
//! terminal exactly like [`AppstoreBundleIdEnsure`]'s own
//! `platform` mismatch: `plan` turns every `Mismatch` into a hard
//! `PlanError::AttributeMismatch` before any node's `ensure` runs (this
//! workspace's design doc, "the `Action::Update` gap" -- `PATCH
//! /v1/bundleIdCapabilities/{id}` exists, but adding convergent-update
//! support to the core plan/apply model is out of scope here), and
//! `Self::ensure`, called directly on an already-mismatched resource,
//! returns [`willikins_core::ToolErrorKind::Conflict`] rather than
//! attempting that `PATCH`. Change the setting by hand in App Store
//! Connect, or pass its current value.
//!
//! **Creating with a setting.** The `POST` body's `attributes.settings`
//! carries exactly one entry, `[{"key": KEY, "options": [{"key": OPTION,
//! "enabled": true}]}]`, when a setting was supplied, and omits the key
//! entirely otherwise -- see [`crate::client::AppstoreClient::create_bundle_id_capability`]'s
//! own doc. **Settled live, 2026-09-28:** the create shape is accepted
//! (verify item 1), but Apple's own read-back never echoes the write
//! side's `enabled: true` -- it lists the selected option by `key` alone
//! (verify item 2, superseding decision (d)'s original assumption; see
//! "Reading a setting back" above and the plan's 2026-09-28 addendum).
//!
//! [`AppstoreBundleIdEnsure`]: crate::tools::AppstoreBundleIdEnsure

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, invalid, not_found, port, require_present, scalar,
    tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdentifier, AppleCapabilitySetting, AppleCapabilityType,
    AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType,
};

use crate::client::{AppstoreClient, CAPABILITIES_NEEDING_PORTAL_CONFIGURATION, client_for};

/// `appstore.bundle_id_capability.ensure`.
pub struct AppstoreBundleIdCapabilityEnsure {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreBundleIdCapabilityEnsure {
    /// Build the tool against `base_url`. See
    /// [`crate::tools::AppstoreBundleIdEnsure::new`]'s own doc for why
    /// there is no client or credential to hold at construction time.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(port("capability"), exact("AppleCapabilityType", true));
        inputs.insert(port("setting"), exact("AppleCapabilitySetting", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("capability"), scalar("AppleCapabilityType"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.bundle_id_capability.ensure"),
                description:
                    "Switch on one capability of an already-registered App Store Connect bundle id."
                        .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier"), port("capability")],
                class: Class::Reversible,
                pure: false,
            },
            base_url: base_url.into(),
        }
    }

    fn outputs_for(capability: &AppleCapabilityType) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("capability"), Value::known(capability.clone()));
        outputs
    }

    /// Find `identifier`'s parent bundle id's Apple-assigned `id`,
    /// comparing every filtered row exactly (see this module's own doc).
    /// `Ok(None)` when no bundle id has it -- the identifier is simply not
    /// registered yet -- mirrors
    /// [`crate::tools::AppstoreProfileEnsure::find_bundle_id`], the
    /// precedent this tool now follows for the same absent-parent case.
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Conflict`] when more than one row
    /// matches exactly; [`willikins_core::ToolErrorKind::Provider`] on a
    /// transport or non-2xx failure or a malformed id.
    fn find_parent(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
    ) -> Result<Option<AppleBundleIdId>, ToolError> {
        let mut matches: Vec<_> = client
            .list_bundle_ids(identifier)?
            .into_iter()
            .filter(|resource| resource.attributes.identifier == identifier.as_str())
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => {
                let resource = matches.remove(0);
                AppleBundleIdId::parse(&resource.id)
                    .map(Some)
                    .map_err(|err| ToolError {
                        kind: willikins_core::ToolErrorKind::Provider,
                        message: format!(
                            "App Store Connect returned a malformed bundle id id: {err}"
                        ),
                    })
            }
            count => Err(willikins_core::tool::helpers::conflict(format!(
                "{count} App Store Connect bundle ids already have identifier `{identifier}`; \
                 this tool cannot disambiguate"
            ))),
        }
    }

    /// The error `ensure` raises when the parent bundle id is still
    /// missing at apply time -- this tool cannot create one itself.
    fn parent_not_found(identifier: &AppleBundleIdentifier) -> ToolError {
        not_found(format!(
            "no App Store Connect bundle id has identifier `{identifier}`; run \
             appstore.bundle_id.ensure first"
        ))
    }

    /// The setting key `capability` requires, if any (decision (d)):
    /// `DATA_PROTECTION` pairs with `DATA_PROTECTION_PERMISSION_LEVEL`,
    /// `APPLE_ID_AUTH` with `APPLE_ID_AUTH_APP_CONSENT`, and every other
    /// capability -- including `PUSH_NOTIFICATIONS`, which is fully
    /// enabled with no setting -- takes none.
    fn required_setting_key(capability: &AppleCapabilityType) -> Option<&'static str> {
        match capability.as_str() {
            "DATA_PROTECTION" => Some("DATA_PROTECTION_PERMISSION_LEVEL"),
            "APPLE_ID_AUTH" => Some("APPLE_ID_AUTH_APP_CONSENT"),
            _ => None,
        }
    }

    /// Enforce the capability/setting pairing before any request -- see
    /// this module's own doc for why `check` cannot hold it.
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Invalid`] naming the capability
    /// and the setting key it needs or forbids.
    fn check_setting_pairing(
        capability: &AppleCapabilityType,
        setting: Option<&AppleCapabilitySetting>,
    ) -> Result<(), ToolError> {
        match (Self::required_setting_key(capability), setting) {
            (None, None) => Ok(()),
            (Some(required), Some(setting)) if setting.key() == required => Ok(()),
            (Some(required), Some(setting)) => Err(invalid(format!(
                "`{capability}` requires a `{required}` setting, not `{}`",
                setting.key()
            ))),
            (Some(required), None) => Err(invalid(format!(
                "`{capability}` requires a `{required}` setting"
            ))),
            (None, Some(setting)) => Err(invalid(format!(
                "`{capability}` takes no setting (got `{}`)",
                setting.key()
            ))),
        }
    }

    /// `parent` is `None` when `identifier` is not registered yet --
    /// `Absent`, exactly as when the identifier is registered but the
    /// capability itself is not on it, since either way there is nothing
    /// this tool can report but "not there".
    fn observe(
        client: &AppstoreClient,
        parent: Option<&AppleBundleIdId>,
        capability: &AppleCapabilityType,
        setting: Option<&AppleCapabilitySetting>,
    ) -> Result<Observation, ToolError> {
        let Some(parent) = parent else {
            return Ok(Observation::Absent {
                predicted: Self::outputs_for(capability),
            });
        };
        let Some(row) = client
            .list_bundle_id_capabilities(parent)?
            .into_iter()
            .find(|resource| resource.attributes.capability_type == capability.as_str())
        else {
            return Ok(Observation::Absent {
                predicted: Self::outputs_for(capability),
            });
        };
        if let Some(setting) = setting {
            // Every option under this setting's key, across every entry
            // carrying that key.
            let options: Vec<_> = row
                .attributes
                .settings
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter(|entry| entry.key.as_deref() == Some(setting.key()))
                .flat_map(|entry| entry.options.as_deref().unwrap_or(&[]))
                .collect();
            // Two read-back shapes (this module's own doc, "Reading a
            // setting back"): when no option carries `enabled` at all
            // (Apple's own shape, observed live 2026-09-28), the listed
            // option *is* the selection. When at least one does (never
            // observed live; a defensive fallback), the pre-2026-09-28
            // rule applies: exactly one option marked `enabled: true`.
            let matches = if options.iter().any(|option| option.enabled.is_some()) {
                let enabled: Vec<Option<&str>> = options
                    .iter()
                    .filter(|option| option.enabled == Some(true))
                    .map(|option| option.key.as_deref())
                    .collect();
                enabled == [Some(setting.option())]
            } else {
                matches!(options.as_slice(), [only] if only.key.as_deref() == Some(setting.option()))
            };
            if !matches {
                return Ok(Observation::Mismatch {
                    port: port("setting"),
                });
            }
        }
        Ok(Observation::Present(Self::outputs_for(capability)))
    }

    fn needs_portal_configuration(capability: &AppleCapabilityType) -> bool {
        CAPABILITIES_NEEDING_PORTAL_CONFIGURATION.contains(&capability.as_str())
    }

    fn portal_configuration_refusal(capability: &AppleCapabilityType) -> ToolError {
        invalid(format!(
            "`{capability}` needs an identifier association (an app group, a merchant id, or an \
             iCloud container) that the App Store Connect API cannot express -- enabling it here \
             would leave the bundle id half-configured. Enable and configure it by hand in App \
             Store Connect's \"Certificates, Identifiers & Profiles\" > Configure flow instead."
        ))
    }

    /// A `setting` mismatch is terminal, exactly like
    /// [`crate::tools::AppstoreBundleIdEnsure`]'s own `platform` mismatch:
    /// this tool cannot `PATCH` it away (this milestone does not add
    /// convergent-update support). Mirrors that tool's own
    /// `platform_mismatch_conflict` in shape and in reasoning.
    fn setting_mismatch_conflict(capability: &AppleCapabilityType) -> ToolError {
        conflict(format!(
            "`{capability}` is already enabled on this bundle id, but with a different setting \
             than requested, and this tool cannot change it once set; change it by hand in App \
             Store Connect, or pass its current value instead"
        ))
    }
}

impl Tool for AppstoreBundleIdCapabilityEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleCapabilityType = get(inputs, "capability")?;
        let setting: Option<AppleCapabilitySetting> = get_optional(inputs, "setting")?;
        Self::check_setting_pairing(&capability, setting.as_ref())?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let parent = Self::find_parent(&client, &identifier)?;
        Self::observe(&client, parent.as_ref(), &capability, setting.as_ref())
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleCapabilityType = get(inputs, "capability")?;
        let setting: Option<AppleCapabilitySetting> = get_optional(inputs, "setting")?;
        Self::check_setting_pairing(&capability, setting.as_ref())?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let parent = Self::find_parent(&client, &identifier)?;
        match Self::observe(&client, parent.as_ref(), &capability, setting.as_ref())? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => Err(Self::setting_mismatch_conflict(&capability)),
            Observation::Absent { .. } => {
                // `observe` reported `Absent` either because the parent
                // does not exist yet (this tool's own module doc, "Read:
                // list the parent") or because it exists but the
                // capability itself is not on it. Only the first is a
                // hard refusal here: this tool still cannot create a
                // bundle id, only report on and switch capabilities on
                // one that already exists.
                let Some(parent) = parent else {
                    return Err(Self::parent_not_found(&identifier));
                };
                if Self::needs_portal_configuration(&capability) {
                    return Err(Self::portal_configuration_refusal(&capability));
                }
                match client.create_bundle_id_capability(&parent, &capability, setting.as_ref()) {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&capability),
                        changed: true,
                    }),
                    // Belt-and-braces: Apple documents no stable code for
                    // "already enabled" on this resource (research note,
                    // section 2), so an ambiguous create failure is
                    // resolved by re-reading rather than parsing the
                    // error body -- the same pattern
                    // `appstore.bundle_id.ensure` and
                    // `buildkite.pipeline.ensure` both use.
                    Err(err) => {
                        match Self::observe(&client, Some(&parent), &capability, setting.as_ref())?
                        {
                            Observation::Present(outputs) => Ok(Ensured {
                                outputs,
                                changed: false,
                            }),
                            Observation::Mismatch { .. } => {
                                Err(Self::setting_mismatch_conflict(&capability))
                            }
                            Observation::Absent { .. } => Err(err.into()),
                            Observation::Foreign => {
                                unreachable!("this tool's own observe never returns this")
                            }
                        }
                    }
                }
            }
            Observation::Foreign => unreachable!("this tool's own observe never returns this"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn tool() -> AppstoreBundleIdCapabilityEnsure {
        AppstoreBundleIdCapabilityEnsure::new("http://127.0.0.1:1")
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_identifier_and_capability() {
        assert_eq!(
            tool().spec().key,
            vec![
                PortName::parse("identifier").unwrap(),
                PortName::parse("capability").unwrap()
            ]
        );
    }

    #[test]
    fn needs_portal_configuration_names_exactly_the_three_documented_capabilities() {
        for capability in ["APP_GROUPS", "APPLE_PAY", "ICLOUD"] {
            assert!(
                AppstoreBundleIdCapabilityEnsure::needs_portal_configuration(
                    &AppleCapabilityType::parse(capability).unwrap()
                )
            );
        }
        for capability in ["PUSH_NOTIFICATIONS", "GAME_CENTER", "SIRIKIT"] {
            assert!(
                !AppstoreBundleIdCapabilityEnsure::needs_portal_configuration(
                    &AppleCapabilityType::parse(capability).unwrap()
                )
            );
        }
    }

    #[test]
    fn spec_carries_setting_as_an_optional_port() {
        let spec = tool().spec;
        let setting = spec
            .inputs
            .get(&PortName::parse("setting").unwrap())
            .unwrap();
        assert!(!setting.required, "`setting` must be optional");
    }

    fn capability(name: &str) -> AppleCapabilityType {
        AppleCapabilityType::parse(name).unwrap()
    }

    fn setting(text: &str) -> AppleCapabilitySetting {
        AppleCapabilitySetting::parse(text).unwrap()
    }

    #[test]
    fn pairing_accepts_data_protection_with_its_own_key_and_apple_id_auth_with_its_own() {
        AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("DATA_PROTECTION"),
            Some(&setting(
                "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
            )),
        )
        .unwrap();
        AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("APPLE_ID_AUTH"),
            Some(&setting("APPLE_ID_AUTH_APP_CONSENT=PRIMARY_APP_CONSENT")),
        )
        .unwrap();
    }

    #[test]
    fn pairing_accepts_an_ordinary_capability_with_no_setting() {
        AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("PUSH_NOTIFICATIONS"),
            None,
        )
        .unwrap();
        AppstoreBundleIdCapabilityEnsure::check_setting_pairing(&capability("HEALTHKIT"), None)
            .unwrap();
    }

    #[test]
    fn pairing_refuses_data_protection_with_no_setting() {
        let err = AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("DATA_PROTECTION"),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Invalid);
        assert!(err.message.contains("DATA_PROTECTION_PERMISSION_LEVEL"));
    }

    #[test]
    fn pairing_refuses_apple_id_auth_with_a_data_protection_setting() {
        let err = AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("APPLE_ID_AUTH"),
            Some(&setting(
                "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
            )),
        )
        .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Invalid);
    }

    #[test]
    fn pairing_refuses_an_ordinary_capability_given_any_setting() {
        let err = AppstoreBundleIdCapabilityEnsure::check_setting_pairing(
            &capability("HEALTHKIT"),
            Some(&setting(
                "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
            )),
        )
        .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Invalid);
        assert!(err.message.contains("HEALTHKIT"));
    }
}
