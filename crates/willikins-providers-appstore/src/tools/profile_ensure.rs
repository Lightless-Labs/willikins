//! `appstore.profile.ensure`: produces (or reports) a real App Store
//! Connect `IOS_APP_STORE` provisioning profile relating a bundle
//! identifier to a distribution certificate. Port table and behaviour
//! from `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`,
//! decisions (b), (d), (e), and (f) -- and this plan's 2026-09-22
//! task-2 Addendum, which corrects `certificate` from
//! `exact_derived_only` to an ordinary port (see that Addendum for why).
//!
//! # Scope: `IOS_APP_STORE` only, refused by type
//!
//! [`willikins_types::AppleProfileType`]'s grammar admits nothing but
//! `IOS_APP_STORE` -- a document naming any other `profileType` fails
//! `check`/`Value::parse` before this tool ever runs, and the create
//! body this tool sends carries no `devices` key at all (decision (b)).
//!
//! # No `PATCH`: `ensure` creates or reports; drift is terminal
//!
//! Apple's specification declares only `DELETE` and `GET` on
//! `/v1/profiles/{id}` -- there is no update. So a profile whose
//! `profileType` differs from what was requested, or whose certificate
//! relationship is not exactly the one requested, is a terminal
//! [`Observation::Mismatch`] (`willikins_core::plan` turns every
//! `Mismatch` into a hard `PlanError::AttributeMismatch`, exactly as
//! `appstore.bundle_id.ensure`'s own module doc explains for its
//! `platform` mismatch). Replacing a profile is the calling document's
//! job -- delete it by hand, or give the document a new `name` -- never
//! this tool's.
//!
//! # `INVALID` and expiry are both terminal, and checked independently
//!
//! `profileState`'s enum is `ACTIVE | INVALID` -- there is no `EXPIRED`
//! member, so expiry is never a *state* Apple reports on its own. This
//! tool's read therefore checks both, independently: `profileState ==
//! INVALID` is a [`willikins_core::ToolErrorKind::Conflict`] regardless
//! of the date, and `expirationDate` at or before the wall clock is a
//! separate `Conflict` regardless of `profileState` -- the pre-flight
//! observed `INVALID` profiles with a future expiry on the operator's
//! own account, so the two are independent facts, not one implying the
//! other.
//!
//! # The key: `(identifier, name)`, and the read that uses the
//! relationship rather than a filter
//!
//! The identifier scopes a profile name to one App ID; the name is what
//! the operator sees in the portal and what an `exportOptions.plist`'s
//! `provisioningProfiles` map references. The read resolves `identifier`
//! to its opaque id via [`crate::client::AppstoreClient::list_bundle_ids`]
//! (already paginated and byte-exact -- `appstore.bundle_id.ensure`'s own
//! module doc), then reads the *relationship*
//! (`GET /v1/bundleIds/{id}/profiles`), which is scoped by construction
//! to that one bundle id -- `filter[name]` on `/v1/profiles` is team-wide
//! and, by every filter this provider has probed so far, almost
//! certainly a substring hint too, so this tool never uses it. The
//! relationship read still paginates and still compares `name`
//! byte-for-byte, for the identical reason
//! [`crate::client::AppstoreClient::list_bundle_ids`]'s own doc gives.
//!
//! Zero exact matches: `Absent`. Two or more: `ToolError::Conflict`
//! naming the count -- name uniqueness per identifier is unverified
//! before the live cycle runs (verify item 1), so this tool refuses
//! rather than guesses either answer. Exactly one: the single-instance
//! read (`GET /v1/profiles/{id}?include=certificates&fields[profiles]=...`)
//! follows, which is the only call that reads `profileContent` and the
//! `certificates` relationship at all -- the list read's own
//! `fields[profiles]` deliberately omits both.

use willikins_core::tool::helpers::{
    conflict, exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, TypeName, TypeRef, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdentifier, AppleCertificateId, AppleIssuerId, AppleKeyId,
    AppleProfileContent, AppleProfileId, AppleProfileName, AppleProfileType, AppleSigningKey,
    DomainType,
};

use crate::client::{AppstoreClient, BundleIdResource, ProfileResource, client_for};

/// `appstore.profile.ensure`.
pub struct AppstoreProfileEnsure {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreProfileEnsure {
    /// Build the tool against `base_url` -- App Store Connect's real API
    /// in production ([`crate::APPSTORE_API_BASE_URL`]), a mock server's
    /// URL in a test. See this crate's own module doc for why there is
    /// no client or credential to hold at construction time.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(port("name"), exact("AppleProfileName", true));
        inputs.insert(port("profile_type"), exact("AppleProfileType", true));
        // Not `exact_derived_only`: see this module's own doc and the
        // plan's 2026-09-22 task-2 Addendum for why.
        inputs.insert(port("certificate"), exact("AppleCertificateId", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("profile"), scalar("AppleProfileId"));
        outputs.insert(port("content"), scalar("AppleProfileContent"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.profile.ensure"),
                description: "Produce (or report) an App Store Connect IOS_APP_STORE \
                               provisioning profile relating a bundle identifier to a \
                               distribution certificate."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            base_url: base_url.into(),
        }
    }

    fn predicted_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("profile"),
            Value::unknown(TypeRef::scalar(TypeName::parse("AppleProfileId").unwrap())),
        );
        outputs.insert(
            port("content"),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("AppleProfileContent").unwrap(),
            )),
        );
        outputs
    }

    fn outputs_for(profile: &AppleProfileId, content: &AppleProfileContent) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("profile"), Value::known(profile.clone()));
        outputs.insert(port("content"), Value::known(content.clone()));
        outputs
    }

    /// Find the (at most one) bundle id row whose `identifier` equals
    /// `identifier` exactly -- reuses
    /// [`crate::client::AppstoreClient::list_bundle_ids`], the same
    /// paginated, byte-exact read `appstore.bundle_id.ensure::find_one`
    /// uses.
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Conflict`] when more than one row
    /// matches exactly; [`willikins_core::ToolErrorKind::Provider`] on
    /// any transport or non-2xx failure.
    fn find_bundle_id(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
    ) -> Result<Option<BundleIdResource>, ToolError> {
        let mut matches: Vec<BundleIdResource> = client
            .list_bundle_ids(identifier)?
            .into_iter()
            .filter(|resource| resource.attributes.identifier == identifier.as_str())
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.remove(0))),
            count => Err(conflict(format!(
                "{count} App Store Connect bundle ids already have identifier `{identifier}`; \
                 this tool cannot disambiguate"
            ))),
        }
    }

    /// Find the (at most one) profile row, under `bundle_id`'s
    /// relationship, whose `name` equals `name` exactly -- see this
    /// module's own doc, "The key ... and the read that uses the
    /// relationship".
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Conflict`] naming the count when
    /// more than one row matches exactly; [`willikins_core::ToolErrorKind::Provider`]
    /// on any transport or non-2xx failure.
    fn find_profile_row(
        client: &AppstoreClient,
        bundle_id: &AppleBundleIdId,
        name: &AppleProfileName,
    ) -> Result<Option<ProfileResource>, ToolError> {
        let mut matches: Vec<ProfileResource> = client
            .list_bundle_id_profiles(bundle_id)?
            .into_iter()
            .filter(|resource| resource.attributes.name == name.as_str())
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.remove(0))),
            count => Err(conflict(format!(
                "{count} profiles named `{name}` already exist on this bundle id; this tool \
                 cannot disambiguate"
            ))),
        }
    }

    /// Read the single-instance resource (`profileContent` and the
    /// `certificates` relationship both present) and build the
    /// [`Observation`] decisions (d)/(e) specify, in the order they list
    /// them: `profile_type` mismatch, then `certificate` mismatch, then
    /// `INVALID`, then expiry, then `Present`.
    fn observe_instance(
        resource: &ProfileResource,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<Observation, ToolError> {
        if resource.attributes.profile_type != profile_type.as_str() {
            return Ok(Observation::Mismatch {
                port: port("profile_type"),
            });
        }
        // Only the `include=certificates` instance read reaches here, and
        // it must carry the relationship's `data`: its absence means
        // nothing was read, which is a provider fault, never a
        // `Mismatch { certificate }` claiming the profile names the wrong
        // certificate.
        let certs = resource
            .relationships
            .as_ref()
            .and_then(|relationships| relationships.certificates.as_ref())
            .and_then(|certificates| certificates.data.as_ref())
            .ok_or_else(|| ToolError {
                kind: ToolErrorKind::Provider,
                message: "App Store Connect returned a profile with no certificates \
                          relationship data, although the read asked it to include them"
                    .to_string(),
            })?;
        if certs.len() != 1 || certs[0].id != certificate.as_str() {
            return Ok(Observation::Mismatch {
                port: port("certificate"),
            });
        }
        if resource.attributes.profile_state == "INVALID" {
            return Err(conflict(
                "the profile exists but Apple reports it INVALID; willikins cannot repair a \
                 profile, replace it instead (delete it by hand, or give the document a new \
                 `name`)"
                    .to_string(),
            ));
        }
        if let Some(expiration_date) = &resource.attributes.expiration_date {
            let expires =
                chrono::DateTime::parse_from_rfc3339(expiration_date).map_err(|err| ToolError {
                    kind: ToolErrorKind::Provider,
                    message: format!(
                        "App Store Connect returned a malformed profile expirationDate: {err}"
                    ),
                })?;
            if expires <= chrono::Utc::now() {
                return Err(conflict(format!(
                    "the profile expired on {expiration_date}; willikins cannot repair a \
                     profile, replace it instead (delete it by hand, or give the document a \
                     new `name`)"
                )));
            }
        }
        let content = resource
            .attributes
            .profile_content
            .clone()
            .ok_or_else(|| ToolError {
                kind: ToolErrorKind::Provider,
                message: "App Store Connect returned no profileContent for an existing profile"
                    .to_string(),
            })?;
        let profile = AppleProfileId::parse(&resource.id).map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed profile id: {err}"),
        })?;
        Ok(Observation::Present(Self::outputs_for(&profile, &content)))
    }

    /// The full read: resolve the bundle id, find the profile row by
    /// name, then (only on a match) the single-instance read and its
    /// checks.
    fn observe(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
        name: &AppleProfileName,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<Observation, ToolError> {
        let Some(bundle_id_resource) = Self::find_bundle_id(client, identifier)? else {
            return Ok(Observation::Absent {
                predicted: Self::predicted_outputs(),
            });
        };
        let bundle_id =
            AppleBundleIdId::parse(&bundle_id_resource.id).map_err(|err| ToolError {
                kind: ToolErrorKind::Provider,
                message: format!("App Store Connect returned a malformed bundle id id: {err}"),
            })?;
        let Some(row) = Self::find_profile_row(client, &bundle_id, name)? else {
            return Ok(Observation::Absent {
                predicted: Self::predicted_outputs(),
            });
        };
        let profile_id = AppleProfileId::parse(&row.id).map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed profile id: {err}"),
        })?;
        let instance = client.get_profile(&profile_id)?;
        Self::observe_instance(&instance, profile_type, certificate)
    }
}

/// The seven parsed input ports, in the order the spec declares them.
struct ProfileInputs {
    issuer_id: AppleIssuerId,
    key_id: AppleKeyId,
    key: AppleSigningKey,
    identifier: AppleBundleIdentifier,
    name: AppleProfileName,
    profile_type: AppleProfileType,
    certificate: AppleCertificateId,
}

fn inputs_of(inputs: &Inputs) -> Result<ProfileInputs, ToolError> {
    Ok(ProfileInputs {
        issuer_id: get(inputs, "issuer_id")?,
        key_id: get(inputs, "key_id")?,
        key: get(inputs, "key")?,
        identifier: get(inputs, "identifier")?,
        name: get(inputs, "name")?,
        profile_type: get(inputs, "profile_type")?,
        certificate: get(inputs, "certificate")?,
    })
}

impl Tool for AppstoreProfileEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let ProfileInputs {
            issuer_id,
            key_id,
            key,
            identifier,
            name,
            profile_type,
            certificate,
        } = inputs_of(inputs)?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        Self::observe(&client, &identifier, &name, &profile_type, &certificate)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let ProfileInputs {
            issuer_id,
            key_id,
            key,
            identifier,
            name,
            profile_type,
            certificate,
        } = inputs_of(inputs)?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let observation = Self::observe(&client, &identifier, &name, &profile_type, &certificate)?;
        match observation {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => Err(ToolError {
                kind: ToolErrorKind::Conflict,
                message: "the existing profile does not match what was requested, and cannot \
                          be converged (there is no update operation for a profile); replace \
                          it instead"
                    .to_string(),
            }),
            Observation::Foreign => unreachable!("appstore.profile.ensure never observes Foreign"),
            Observation::Absent { .. } => {
                let Some(bundle_id_resource) = Self::find_bundle_id(&client, &identifier)? else {
                    return Err(not_found(format!(
                        "bundle identifier `{identifier}` is not registered; register it (for \
                         example via appstore.bundle_id.ensure) before creating a profile for it"
                    )));
                };
                let bundle_id =
                    AppleBundleIdId::parse(&bundle_id_resource.id).map_err(|err| ToolError {
                        kind: ToolErrorKind::Provider,
                        message: format!(
                            "App Store Connect returned a malformed bundle id id: {err}"
                        ),
                    })?;
                match client.create_profile(&name, &profile_type, &bundle_id, &certificate) {
                    Ok(created) => {
                        let profile_id =
                            AppleProfileId::parse(&created.id).map_err(|err| ToolError {
                                kind: ToolErrorKind::Provider,
                                message: format!(
                                    "App Store Connect returned a malformed profile id: {err}"
                                ),
                            })?;
                        // Verify item 5 ("does the 201 carry profileContent?")
                        // is unsettled until the live cycle runs; if the
                        // create response omits it, one GET follows rather
                        // than assuming either answer.
                        let content = if let Some(content) = created.attributes.profile_content {
                            content
                        } else {
                            let instance = client.get_profile(&profile_id)?;
                            instance
                                .attributes
                                .profile_content
                                .ok_or_else(|| ToolError {
                                    kind: ToolErrorKind::Provider,
                                    message: "App Store Connect returned no profileContent for \
                                              a just-created profile, even after a follow-up GET"
                                        .to_string(),
                                })?
                        };
                        Ok(Ensured {
                            outputs: Self::outputs_for(&profile_id, &content),
                            changed: true,
                        })
                    }
                    // Ambiguous create failure: re-read rather than parse
                    // the error body, exactly as
                    // `appstore.bundle_id.ensure::ensure` does.
                    Err(err) => {
                        let observation = Self::observe(
                            &client,
                            &identifier,
                            &name,
                            &profile_type,
                            &certificate,
                        )?;
                        match observation {
                            Observation::Present(outputs) => Ok(Ensured {
                                outputs,
                                changed: false,
                            }),
                            Observation::Absent { .. } => Err(err.into()),
                            Observation::Mismatch { .. } => Err(ToolError {
                                kind: ToolErrorKind::Conflict,
                                message: "the existing profile does not match what was \
                                          requested, and cannot be converged (there is no \
                                          update operation for a profile); replace it instead"
                                    .to_string(),
                            }),
                            Observation::Foreign => {
                                unreachable!("appstore.profile.ensure never observes Foreign")
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn tool() -> AppstoreProfileEnsure {
        AppstoreProfileEnsure::new("http://127.0.0.1:1")
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_identifier_and_name() {
        assert_eq!(
            tool().spec().key,
            vec![
                PortName::parse("identifier").unwrap(),
                PortName::parse("name").unwrap()
            ]
        );
    }

    #[test]
    fn spec_certificate_port_is_not_derived_only() {
        assert!(
            !tool()
                .spec()
                .inputs
                .get(&port("certificate"))
                .expect("certificate port exists")
                .derived_only
        );
    }

    #[test]
    fn spec_is_reversible_and_not_pure() {
        assert_eq!(tool().spec().class, Class::Reversible);
        assert!(!tool().spec().pure);
    }

    #[test]
    fn spec_content_output_is_secret() {
        assert!(
            willikins_types::registry()
                .is_secret(&TypeName::parse("AppleProfileContent").unwrap())
                .unwrap_or(false)
        );
    }
}
