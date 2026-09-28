//! `appstore.app.get`: a gate over `GET /v1/apps` -- "an App Store
//! Connect app record exists for this bundle identifier"
//! (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, "Walter, for
//! T3", and the pre-flight checklist's row 18: "App Store Connect app
//! record | website only | No").
//!
//! # A leaf gate
//!
//! Decision (j), point 2: "a gate passes through the key it checked" --
//! but an app record cannot be *created* through this API at all
//! (`docs/research/2026-09-16-app-store-connect.md`, section 4: no
//! release note in the API's history announces app-record creation, and
//! the operation list has no `POST /v1/apps`), so nothing in the Walter
//! document is ordered by this gate's own state the way a profile is
//! ordered by [`crate::tools::AppstoreAppGroupGate`]. This tool is a
//! **leaf**: it blocks the run and is reported, but holds nothing back.
//! It still passes `identifier` through as its own output, for symmetry
//! with every other gate in this crate and in case a later document ever
//! wants to consume it, but no node in this milestone's document does.
//!
//! # Reading back by key
//!
//! `GET /v1/apps?filter[bundleId]={identifier}`. Whether `filter[bundleId]`
//! is an exact match or a substring one is unobserved
//! ([`crate::client::AppstoreClient::list_apps`]'s own doc) -- this tool
//! never trusts the filter alone, exactly like every other filtered read
//! in this crate: it compares every returned row's `bundleId` attribute
//! against `identifier` byte for byte, and `Present` only on an exact
//! match.

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleBundleIdentifier, AppleIssuerId, AppleKeyId, AppleSigningKey};

use crate::client::client_for;

/// This tool's one gate: `identifier` is the only subject, so a blocked
/// report names the bundle identifier the app record is missing for.
static GATE: Gate = Gate {
    need: "an App Store Connect app record exists for this bundle identifier",
    how: "create it in App Store Connect (Apps -> + -> New App); the Account Holder must have \
          signed the latest agreement first",
    subject: &["identifier"],
};

/// `appstore.app.get`.
pub struct AppstoreAppGet {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreAppGet {
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
                name: tool_name("appstore.app.get"),
                description: "A gate: whether an App Store Connect app record exists for a \
                              bundle identifier."
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

    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Provider`] on a transport or
    /// non-2xx failure.
    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let found = client
            .list_apps(&identifier)?
            .into_iter()
            .any(|app| app.attributes.bundle_id == identifier.as_str());
        if found {
            Ok(Observation::Present(Self::outputs_for(&identifier)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            })
        }
    }
}

impl Tool for AppstoreAppGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.observe(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // A gate is pure, so `apply` never reaches this while the node is
        // `Action::Blocked` -- reached only once `read` already answered
        // `Present`, exactly like `willikins_tools::OperatorAcknowledge`'s
        // own `ensure`.
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

    fn tool() -> AppstoreAppGet {
        AppstoreAppGet::new("http://127.0.0.1:1")
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
