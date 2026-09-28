//! `appstore.app_group.gate`: a gate over `APP_GROUPS` enabled on a
//! bundle identifier
//! (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, "Sample, for
//! T3", and operator decision 6). Unlike [`crate::tools::AppstoreAppGet`],
//! this gate is **not** a leaf: it passes `identifier` through as its own
//! output (decision (j), point 2, "a gate passes through the key it
//! checked"), so a document's profile node binds `identifier` from
//! *this* gate's output rather than from the registration node's --
//! that data edge is what orders the profile after the app-group gate,
//! not merely after registration, so the first run never mints a
//! profile Apple would then invalidate once the group is assigned.
//!
//! # What this gate reads, and what it cannot
//!
//! It resolves `identifier` to its parent bundle id exactly the way
//! [`crate::tools::AppstoreBundleIdCapabilityEnsure`] does (an
//! unregistered identifier is `Absent`, not a refusal -- the same
//! T3-blocking precedent that tool's own module doc explains, duplicated
//! here rather than exposed cross-module, the same "each tool derives its
//! own small helper" choice this crate's fake twins already make), then
//! lists its capabilities and looks for the `APP_GROUPS` row, with no
//! `setting` to check (`APP_GROUPS` takes none). The API only ever shows
//! the flag *enabled*, never whether the group is actually *assigned*
//! (pre-flight row 14: "the API can flip the flag but cannot assign") --
//! so this gate can read `Present` before the operator has actually
//! assigned the group by hand, and a profile minted at that point is one
//! Apple will invalidate once the assignment happens. That is not a
//! defect this gate can fix (there is no API signal it could read
//! instead); `appstore.profile.ensure`'s own replace-when-INVALID
//! (operator decision 1) is what heals it on the document's next run.

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdentifier, AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType,
};

use crate::client::{AppstoreClient, client_for};

/// The one capability this gate ever checks.
const APP_GROUPS: &str = "APP_GROUPS";

/// This tool's one gate: `identifier` is the only subject.
static GATE: Gate = Gate {
    need: "APP_GROUPS enabled on this bundle identifier",
    how: "register group.<app identifier>, enable App Groups on the identifier in App Store \
          Connect (Configure), and assign the group to it (portal Configure, or Xcode)",
    subject: &["identifier"],
};

/// `appstore.app_group.gate`.
pub struct AppstoreAppGroupGate {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreAppGroupGate {
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
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.app_group.gate"),
                description: "A gate: whether APP_GROUPS is enabled on a bundle identifier."
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

    /// Find `identifier`'s parent bundle id's Apple-assigned `id`,
    /// comparing every filtered row exactly -- mirrors
    /// `AppstoreBundleIdCapabilityEnsure::find_parent` (private to its
    /// own module) field for field.
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
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let Some(parent) = Self::find_parent(&client, &identifier)? else {
            return Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            });
        };
        let enabled = client
            .list_bundle_id_capabilities(&parent)?
            .into_iter()
            .any(|row| row.attributes.capability_type == APP_GROUPS);
        if enabled {
            Ok(Observation::Present(Self::outputs_for(&identifier)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            })
        }
    }
}

impl Tool for AppstoreAppGroupGate {
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

    fn tool() -> AppstoreAppGroupGate {
        AppstoreAppGroupGate::new("http://127.0.0.1:1")
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
    fn declares_a_gate_over_identifier_only() {
        let tool = tool();
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["identifier"]);
    }

    #[test]
    fn a_catalog_accepts_this_gate() {
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog.insert(std::sync::Arc::new(tool())).unwrap();
    }
}
