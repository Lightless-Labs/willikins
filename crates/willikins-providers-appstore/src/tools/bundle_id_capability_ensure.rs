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
//! If no bundle id has `identifier` at all, this tool's `read` refuses
//! outright ([`willikins_core::ToolErrorKind::NotFound`]) rather than
//! reporting `Absent`: `Absent` would imply `ensure` can create what is
//! missing, and this tool cannot create a bundle id -- only
//! `appstore.bundle_id.ensure` does that. A document names that tool
//! first.
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
//! are fully expressible through `CapabilitySetting` even though this
//! tool never sets one -- enabling them with no setting is a complete,
//! valid state Apple accepts, not a half-configured one, so they are not
//! refused.
//!
//! `read` still reports `Present`/`Absent` honestly for all 28 capability
//! types, including these three -- refusing only happens in `ensure`,
//! and only on the `Absent` → create path. A document that names one of
//! these three and finds it already `Present` (enabled by a human in the
//! portal, association and all) converges with no error, same as any
//! other capability.

use willikins_core::tool::helpers::{
    exact, get, invalid, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdentifier, AppleCapabilityType, AppleIssuerId, AppleKeyId,
    AppleSigningKey, DomainType,
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

    /// Resolve `identifier` to its parent bundle id's Apple-assigned
    /// `id`, comparing every filtered row exactly (see this module's own
    /// doc).
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::NotFound`] naming `identifier`
    /// when no bundle id has it; [`willikins_core::ToolErrorKind::Conflict`]
    /// when more than one does; [`willikins_core::ToolErrorKind::Provider`]
    /// on a transport or non-2xx failure or a malformed id.
    fn resolve_parent(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
    ) -> Result<AppleBundleIdId, ToolError> {
        let mut matches: Vec<_> = client
            .list_bundle_ids(identifier)?
            .into_iter()
            .filter(|resource| resource.attributes.identifier == identifier.as_str())
            .collect();
        match matches.len() {
            0 => Err(not_found(format!(
                "no App Store Connect bundle id has identifier `{identifier}`; run \
                 appstore.bundle_id.ensure first"
            ))),
            1 => {
                let resource = matches.remove(0);
                AppleBundleIdId::parse(&resource.id).map_err(|err| ToolError {
                    kind: willikins_core::ToolErrorKind::Provider,
                    message: format!("App Store Connect returned a malformed bundle id id: {err}"),
                })
            }
            count => Err(willikins_core::tool::helpers::conflict(format!(
                "{count} App Store Connect bundle ids already have identifier `{identifier}`; \
                 this tool cannot disambiguate"
            ))),
        }
    }

    fn observe(
        client: &AppstoreClient,
        parent: &AppleBundleIdId,
        capability: &AppleCapabilityType,
    ) -> Result<Observation, ToolError> {
        let present = client
            .list_bundle_id_capabilities(parent)?
            .into_iter()
            .any(|resource| resource.attributes.capability_type == capability.as_str());
        Ok(if present {
            Observation::Present(Self::outputs_for(capability))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(capability),
            }
        })
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
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let parent = Self::resolve_parent(&client, &identifier)?;
        Self::observe(&client, &parent, &capability)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleCapabilityType = get(inputs, "capability")?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let parent = Self::resolve_parent(&client, &identifier)?;
        match Self::observe(&client, &parent, &capability)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => {
                if Self::needs_portal_configuration(&capability) {
                    return Err(Self::portal_configuration_refusal(&capability));
                }
                match client.create_bundle_id_capability(&parent, &capability) {
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
                    Err(err) => match Self::observe(&client, &parent, &capability)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Absent { .. } => Err(err.into()),
                        Observation::Foreign | Observation::Mismatch { .. } => {
                            unreachable!("this tool's own observe never returns these")
                        }
                    },
                }
            }
            Observation::Foreign | Observation::Mismatch { .. } => {
                unreachable!("this tool's own observe never returns these")
            }
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
}
