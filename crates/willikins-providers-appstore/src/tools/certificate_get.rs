//! `appstore.certificate.get`: selects a distribution certificate by
//! type and serial number. Pure and read-only, modelled one-for-one on
//! `buildkite.cluster.get`: `ensure` is the identity of `read`, it
//! creates nothing, and its output is not an idempotence key. Port table
//! and behaviour from `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`,
//! decision (c).
//!
//! # Why `certificate_type` and `serial_number`, both required
//!
//! Certificates are **team-scoped** with no bundle-identifier filter and
//! no relationship to one, so nothing project-derived can find "this
//! project's certificate" -- the document must name it. `displayName`
//! cannot discriminate (every certificate of one type on one team shares
//! Apple's own naming convention, and the operator held three "Apple
//! Distribution" certificates at once on 2026-09-22), and the opaque `id`
//! is invisible in the portal and in the keychain. The serial number is
//! the one key visible from the *private-key* side: it is in the `.p12`
//! a signer holds (`openssl x509 -serial`) and in Keychain Access, so
//! naming it is how a document says which private key a profile must be
//! signed by.
//!
//! # The read: filter, then compare client-side, both fields
//!
//! `filter[serialNumber]` is proven substring, the same way
//! `filter[identifier]` is (milestone 3c pre-flight, and this crate's own
//! `appstore.bundle_id.ensure`, whose module doc gives the full
//! reasoning). So this tool never trusts the filter: every row the
//! provider returns is compared against **both** `certificate_type` and
//! `serial_number` byte-for-byte before it counts as a match --
//! `tests/certificate_get_mock.rs`'s
//! `read_reports_not_found_when_only_a_substring_neighbor_matches` and
//! `read_reports_not_found_when_the_serial_matches_but_the_type_does_not`
//! are its proof.
//!
//! # Health, not only identity
//!
//! A match that exists is not necessarily usable: `activated: false`
//! (Apple's own words -- "PATCH... this attribute... can deactivate a
//! certificate") or an `expirationDate` at or before the wall clock each
//! refuse with [`willikins_core::ToolErrorKind::Conflict`], naming the
//! certificate by its type and "the requested serial" (never the serial
//! itself), never anything from the response --
//! `activated` absent (observed on all 5 of the operator's own
//! certificates during the pre-flight) is treated as "not deactivated",
//! and only an explicit `false` refuses.
//!
//! # Zero or more than one match
//!
//! **Zero** matches (whether because none exist, or because every
//! candidate the filter returned failed the exact compare) is
//! [`willikins_core::ToolErrorKind::NotFound`], naming the type and
//! saying no certificate of it has the requested serial -- never repeating
//! the serial itself (the document already holds it, and a refusal travels
//! further than an input does: an agent's transcript, a harness panic),
//! nor echoing any other certificate's own fields.
//! **More than one** exact match is
//! [`willikins_core::ToolErrorKind::Conflict`] naming the count alone,
//! never the ids: this team held three distribution certificates at once
//! earlier the same day this milestone's research was written, so
//! picking one because it is merely "the first" is exactly the guess
//! this design refuses to make.

use willikins_core::tool::helpers::{
    conflict, exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_types::{
    AppleCertificateId, AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId,
    AppleSigningKey, DomainType,
};

use crate::client::{AppstoreClient, CertificateResource, client_for};

/// `appstore.certificate.get`.
pub struct AppstoreCertificateGet {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreCertificateGet {
    /// Build the tool against `base_url` -- App Store Connect's real API
    /// in production ([`crate::APPSTORE_API_BASE_URL`]), a mock server's
    /// URL in a test. See `crate` module doc for why there is no client
    /// or credential to hold at construction time.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(
            port("certificate_type"),
            exact("AppleCertificateType", true),
        );
        inputs.insert(port("serial_number"), exact("AppleCertificateSerial", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("certificate"), scalar("AppleCertificateId"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.certificate.get"),
                description: "Select an App Store Connect distribution certificate by type and \
                               serial number."
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

    /// Find the (at most one) row whose `certificateType` and
    /// `serialNumber` both equal what was requested, exactly -- see this
    /// module's own doc, "The read: filter, then compare client-side".
    ///
    /// # Errors
    ///
    /// [`ToolErrorKind::Conflict`] naming the count when more than one row
    /// matches exactly; [`ToolErrorKind::Provider`] on any transport or
    /// non-2xx failure.
    fn find_one(
        client: &AppstoreClient,
        certificate_type: &AppleCertificateType,
        serial_number: &AppleCertificateSerial,
    ) -> Result<Option<CertificateResource>, ToolError> {
        let mut matches: Vec<CertificateResource> = client
            .list_certificates(certificate_type, serial_number)?
            .into_iter()
            .filter(|resource| {
                resource.attributes.certificate_type == certificate_type.as_str()
                    && resource.attributes.serial_number == serial_number.as_str()
            })
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.remove(0))),
            count => Err(conflict(format!(
                "{count} {certificate_type} certificates have the requested serial; this \
                 tool cannot disambiguate"
            ))),
        }
    }

    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let issuer_id: AppleIssuerId = get(inputs, "issuer_id")?;
        let key_id: AppleKeyId = get(inputs, "key_id")?;
        let key: AppleSigningKey = get(inputs, "key")?;
        let certificate_type: AppleCertificateType = get(inputs, "certificate_type")?;
        let serial_number: AppleCertificateSerial = get(inputs, "serial_number")?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let Some(resource) = Self::find_one(&client, &certificate_type, &serial_number)? else {
            return Err(not_found(format!(
                "no {certificate_type} certificate has the requested serial"
            )));
        };
        if let Some(expiration_date) = &resource.attributes.expiration_date {
            let expires =
                chrono::DateTime::parse_from_rfc3339(expiration_date).map_err(|err| ToolError {
                    kind: ToolErrorKind::Provider,
                    message: format!(
                        "App Store Connect returned a malformed certificate expirationDate: {err}"
                    ),
                })?;
            if expires <= chrono::Utc::now() {
                return Err(conflict(format!(
                    "the {certificate_type} certificate with the requested serial is expired"
                )));
            }
        }
        if resource.attributes.activated == Some(false) {
            return Err(conflict(format!(
                "the {certificate_type} certificate with the requested serial is deactivated"
            )));
        }
        let certificate = AppleCertificateId::parse(&resource.id).map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed certificate id: {err}"),
        })?;
        let mut outputs = Outputs::new();
        outputs.insert(port("certificate"), Value::known(certificate));
        Ok(outputs)
    }
}

impl Tool for AppstoreCertificateGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.lookup(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.lookup(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> AppstoreCertificateGet {
        AppstoreCertificateGet::new("http://127.0.0.1:1")
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key() {
        assert!(tool().spec().key.is_empty());
    }

    #[test]
    fn spec_is_reversible_and_pure() {
        assert_eq!(tool().spec().class, Class::Reversible);
        assert!(tool().spec().pure);
    }
}
