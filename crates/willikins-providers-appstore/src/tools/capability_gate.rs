//! `appstore.bundle_id_capability.gate`: a read-only gate over *any*
//! [`AppleObservableCapabilityType`] enabled on a bundle identifier
//! (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, the App Attest
//! gate task, 2026-09-30). Generalizes
//! [`crate::tools::AppstoreAppGroupGate`] -- which checks exactly
//! `APP_GROUPS` -- to any capability this crate can observe, including
//! the two Apple's specification never lists as writable at all
//! (`AppleObservableCapabilityType`'s own doc has the live evidence).
//!
//! Like that gate, this one is **not** a leaf: it passes `identifier`
//! through as its own output (decision (j), point 2, "a gate passes
//! through the key it checked"), so a document's dependent node binds
//! `identifier` from *this* gate's output rather than from the
//! registration node's -- ordering the dependent after the capability is
//! confirmed enabled, not merely after the identifier is registered.
//!
//! # Why `capability` is a port, not a per-tool constant
//!
//! `appstore.app_group.gate` hardcodes `APP_GROUPS` because it is the one
//! capability milestone 3e's app-group flow ever needs. This gate exists
//! because a *second* capability -- App Attest -- needs the identical
//! shape, and a third is plausible; rather than adding a third
//! near-duplicate module, `capability` is a normal input port of
//! [`AppleObservableCapabilityType`], and one tool covers every capability
//! this crate can read. `Gate::need`/`Gate::how` are `&'static str` and
//! cannot interpolate which capability was requested, so `capability` is
//! also named in [`GATE`]'s own `subject` alongside `identifier` -- the
//! engine renders both in a blocked report, which is what actually tells
//! the operator which capability, on which identifier, is missing.
//!
//! # What this gate reads, and what it cannot
//!
//! Exactly [`crate::tools::AppstoreAppGroupGate::find_parent`]'s own
//! approach, duplicated rather than shared cross-module for the same
//! "each tool derives its own small helper" reason that gate's own module
//! doc gives: resolve `identifier` to its parent bundle id (an
//! unregistered identifier is `Absent`, not a refusal -- T3's own
//! precedent), then list its capabilities and look for a row whose
//! `capabilityType` equals `capability`, with no `setting` to check (this
//! gate never writes, so it has no create body needing one). The API only
//! ever shows a capability *enabled*, never any further configuration a
//! given capability might still need (App Groups' own group assignment,
//! for instance) -- this gate can only observe what Apple's own read
//! exposes, exactly the same honest limit `AppstoreAppGroupGate`'s own
//! module doc already states.

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdentifier, AppleIssuerId, AppleKeyId,
    AppleObservableCapabilityType, AppleSigningKey, DomainType,
};

use crate::client::{AppstoreClient, client_for};

/// This tool's one gate: `identifier` names the bundle id, `capability`
/// names which capability was checked -- both rendered in a blocked
/// report, since `need`/`how` cannot interpolate either.
static GATE: Gate = Gate {
    need: "the named capability enabled on this bundle identifier",
    how: "willikins cannot enable this: the operator enables the named capability for the named \
          identifier in App Store Connect (Configure), or from Xcode's Signing & Capabilities.",
    subject: &["identifier", "capability"],
};

/// `appstore.bundle_id_capability.gate`.
pub struct AppstoreBundleIdCapabilityGate {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreBundleIdCapabilityGate {
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
        inputs.insert(
            port("capability"),
            exact("AppleObservableCapabilityType", true),
        );
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.bundle_id_capability.gate"),
                description: "A gate: whether a named capability is enabled on a bundle \
                              identifier."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            base_url: base_url.into(),
        }
    }

    fn outputs_for(identifier: &AppleBundleIdentifier) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs
    }

    /// Find `identifier`'s parent bundle id's Apple-assigned `id` --
    /// mirrors `AppstoreAppGroupGate::find_parent` field for field (that
    /// gate's own module doc explains why this is duplicated rather than
    /// shared).
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
                 this gate cannot disambiguate"
            ))),
        }
    }

    /// # Errors
    ///
    /// See [`Self::find_parent`].
    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleObservableCapabilityType = get(inputs, "capability")?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let Some(parent) = Self::find_parent(&client, &identifier)? else {
            return Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            });
        };
        let enabled = client
            .list_bundle_id_capabilities(&parent)?
            .into_iter()
            .any(|row| row.attributes.capability_type == capability.as_str());
        if enabled {
            Ok(Observation::Present(Self::outputs_for(&identifier)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            })
        }
    }
}

impl Tool for AppstoreBundleIdCapabilityGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.observe(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // See `AppstoreAppGet::ensure`'s own doc: a gate is pure, so
        // `apply` never reaches this while the node is `Action::Blocked`.
        match self.observe(inputs)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("this gate's own observe never returns {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> AppstoreBundleIdCapabilityGate {
        AppstoreBundleIdCapabilityGate::new("http://127.0.0.1:1")
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure_and_reversible() {
        let spec = tool().spec;
        assert!(spec.key.is_empty());
        assert!(spec.pure);
        assert_eq!(spec.class, Class::Reversible);
    }

    #[test]
    fn spec_output_identifier_passes_the_input_through() {
        let spec = tool().spec;
        assert!(spec.outputs.contains_key(&port("identifier")));
    }

    #[test]
    fn declares_a_gate_over_identifier_and_capability() {
        let tool = tool();
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["identifier", "capability"]);
    }

    #[test]
    fn a_catalog_accepts_this_gate() {
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog.insert(std::sync::Arc::new(tool())).unwrap();
    }
}
