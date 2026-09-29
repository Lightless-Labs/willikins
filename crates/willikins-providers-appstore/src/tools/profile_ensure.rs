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
//! # `INVALID` is replaced, not terminal; expiry stays terminal
//!
//! `profileState`'s enum is `ACTIVE | INVALID` -- there is no `EXPIRED`
//! member, so expiry is never a *state* Apple reports on its own; it is
//! checked independently of `profileState` below, and stays a terminal
//! [`willikins_core::ToolErrorKind::Conflict`] (the pre-flight observed
//! `INVALID` profiles with a future expiry on the operator's own
//! account, so the two are independent facts, not one implying the
//! other).
//!
//! `profileState == INVALID`, at this exact `(identifier, name)` key, is
//! **not** terminal (2026-09-28 addendum, milestone 3e's decision 1,
//! "replace-when-INVALID"): Apple invalidates a profile whenever the App
//! ID it names is modified (a capability enabled, say), so a document
//! that runs `appstore.bundle_id_capability.ensure` and then a signing
//! document across two runs leaves the first run's profile INVALID
//! through no fault of the operator's. `read` reports it exactly like a
//! missing profile ([`Observation::Absent`]), so `plan` shows the
//! replacement as an ordinary create rather than failing the whole plan;
//! `ensure` deletes it **by the id the read returned**, then creates
//! fresh. This is checked before `profile_type` or `certificate`, so a
//! stale profile of any shape at this key is replaced rather than
//! reported `Mismatch`. It is never reached for a profile of another
//! identifier or another name: [`AppstoreProfileEnsure::find_bundle_id`]
//! and [`AppstoreProfileEnsure::find_profile_row`] only ever resolve the
//! row at the exact key requested, so an `ACTIVE` profile, or an
//! `INVALID` one under a different identifier or name, is never touched.
//!
//! **This makes the tool [`Class::Destructive`]**, not
//! [`Class::Reversible`]: `ensure` can now delete a resource it did not
//! itself just create in the same call, which is exactly what
//! `willikins_core::class`'s own doc calls destructive ("destroys or
//! overwrites something"), the same reasoning that makes
//! `doppler.service_token.rotate` destructive. The class is a static
//! property of the tool, not a fact about one particular run, so a plan
//! containing this tool requires approval on every run, whether or not
//! that run's own key is `INVALID` today.
//!
//! **The plan itself says so too (2026-09-29 addendum, milestone 3e's
//! finding 4).** [`Tool::replaces`] answers, for this exact call, whether
//! `read`'s `Absent` is "nothing here" or "an `INVALID` profile at this
//! key" -- `willikins_core::plan` turns the latter into `Action::Replace`
//! rather than `Action::Create`, and lists the profile's own
//! `(identifier, name)` key in `Plan::replacing`, so an approver reading a
//! plan that shows `Replace` sees the delete named, not only a class flag
//! that was already going to say "approve me" either way.
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
                // Destructive, not Reversible: see this module's own doc,
                // "`INVALID` is replaced, not terminal" -- `ensure` can now
                // delete an `INVALID` profile at this key before creating
                // fresh.
                class: Class::Destructive,
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
    /// `certificates` relationship both present) and decide what to do
    /// with it, in the order this module's own doc specifies: `INVALID`
    /// first (replaced, never a `Mismatch`), then `profile_type`
    /// mismatch, then `certificate` mismatch, then expiry, then
    /// `Present`.
    fn resolve_instance(
        resource: &ProfileResource,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<ProfileResolution, ToolError> {
        let profile_id = AppleProfileId::parse(&resource.id).map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed profile id: {err}"),
        })?;
        if resource.attributes.profile_state == "INVALID" {
            return Ok(ProfileResolution::Invalid { id: profile_id });
        }
        if resource.attributes.profile_type != profile_type.as_str() {
            return Ok(ProfileResolution::Decided(Observation::Mismatch {
                port: port("profile_type"),
            }));
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
            return Ok(ProfileResolution::Decided(Observation::Mismatch {
                port: port("certificate"),
            }));
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
        Ok(ProfileResolution::Decided(Observation::Present(
            Self::outputs_for(&profile_id, &content),
        )))
    }

    /// The full resolution: resolve the bundle id, find the profile row
    /// by name, then (only on a match) the single-instance read and its
    /// checks. Never touches a profile of another identifier or another
    /// name -- both lookups below are exact, byte-for-byte compares
    /// against the requested key, never a provider filter.
    fn resolve(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
        name: &AppleProfileName,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<ProfileResolution, ToolError> {
        let Some(bundle_id_resource) = Self::find_bundle_id(client, identifier)? else {
            return Ok(ProfileResolution::NotFound);
        };
        let bundle_id =
            AppleBundleIdId::parse(&bundle_id_resource.id).map_err(|err| ToolError {
                kind: ToolErrorKind::Provider,
                message: format!("App Store Connect returned a malformed bundle id id: {err}"),
            })?;
        let Some(row) = Self::find_profile_row(client, &bundle_id, name)? else {
            return Ok(ProfileResolution::NotFound);
        };
        let profile_id = AppleProfileId::parse(&row.id).map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed profile id: {err}"),
        })?;
        let instance = client.get_profile(&profile_id)?;
        // The id `ensure` deletes on the `INVALID` path is the one this
        // instance read answered with, so it must be the row the exact
        // name compare above matched: an answer carrying another id is
        // tied to nothing this tool asked for, and is never acted on
        // (2026-09-29 adversarial pass).
        if instance.id != row.id {
            return Err(ToolError {
                kind: ToolErrorKind::Provider,
                message: "App Store Connect answered the read of one profile with a \
                          different profile's id; refusing to act on it"
                    .to_string(),
            });
        }
        Self::resolve_instance(&instance, profile_type, certificate)
    }

    /// Create a fresh profile at `(identifier, name)`: resolve the
    /// bundle id (again -- the caller may have reached here from
    /// [`ProfileResolution::Invalid`], which already deleted a stale row
    /// at this key but never learned the bundle id's own opaque id), then
    /// `POST`. On an ambiguous create failure, re-resolve rather than
    /// parse the error body, exactly as `appstore.bundle_id.ensure`'s own
    /// module doc explains for its own ambiguous create.
    fn create_new(
        client: &AppstoreClient,
        identifier: &AppleBundleIdentifier,
        name: &AppleProfileName,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<Ensured, ToolError> {
        let Some(bundle_id_resource) = Self::find_bundle_id(client, identifier)? else {
            return Err(not_found(format!(
                "bundle identifier `{identifier}` is not registered; register it (for \
                 example via appstore.bundle_id.ensure) before creating a profile for it"
            )));
        };
        let bundle_id =
            AppleBundleIdId::parse(&bundle_id_resource.id).map_err(|err| ToolError {
                kind: ToolErrorKind::Provider,
                message: format!("App Store Connect returned a malformed bundle id id: {err}"),
            })?;
        match client.create_profile(name, profile_type, &bundle_id, certificate) {
            Ok(created) => {
                let profile_id = AppleProfileId::parse(&created.id).map_err(|err| ToolError {
                    kind: ToolErrorKind::Provider,
                    message: format!("App Store Connect returned a malformed profile id: {err}"),
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
                            message: "App Store Connect returned no profileContent for a \
                                  just-created profile, even after a follow-up GET"
                                .to_string(),
                        })?
                };
                Ok(Ensured {
                    outputs: Self::outputs_for(&profile_id, &content),
                    changed: true,
                })
            }
            Err(err) => match Self::resolve(client, identifier, name, profile_type, certificate)? {
                ProfileResolution::Decided(Observation::Present(outputs)) => Ok(Ensured {
                    outputs,
                    changed: false,
                }),
                ProfileResolution::Decided(Observation::Mismatch { .. }) => Err(ToolError {
                    kind: ToolErrorKind::Conflict,
                    message: "the existing profile does not match what was requested, and \
                              cannot be converged (there is no update operation for a \
                              profile); replace it instead"
                        .to_string(),
                }),
                ProfileResolution::Decided(Observation::Foreign) => {
                    unreachable!("appstore.profile.ensure never observes Foreign")
                }
                ProfileResolution::Decided(Observation::Absent { .. }) => {
                    unreachable!("resolve_instance never decides Absent")
                }
                // Still (or again) nothing at this key, or the re-read
                // finds it INVALID once more: neither retries the create
                // within this one `ensure` call, so the original failure
                // is what the caller sees.
                ProfileResolution::NotFound | ProfileResolution::Invalid { .. } => Err(err.into()),
            },
        }
    }
}

/// What [`AppstoreProfileEnsure::resolve`] decided, before it becomes
/// either an [`Observation`] (`read`) or an `ensure` action.
enum ProfileResolution {
    /// No bundle id, or no profile row, at this exact `(identifier,
    /// name)` key.
    NotFound,
    /// A profile row at this exact key exists and Apple reports its
    /// `profileState` as `INVALID` -- carries the id `ensure` deletes
    /// before creating fresh. See this module's own doc.
    Invalid { id: AppleProfileId },
    /// The full instance check ran and decided.
    Decided(Observation),
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
        match Self::resolve(&client, &identifier, &name, &profile_type, &certificate)? {
            // A replaced profile plans exactly like a missing one -- see
            // this module's own doc, "`INVALID` is replaced, not
            // terminal".
            ProfileResolution::NotFound | ProfileResolution::Invalid { .. } => {
                Ok(Observation::Absent {
                    predicted: Self::predicted_outputs(),
                })
            }
            ProfileResolution::Decided(observation) => Ok(observation),
        }
    }

    /// 2026-09-29 addendum, milestone 3e's finding 4: whether the `Absent`
    /// `read` just reported is a genuine "nothing here" or "an `INVALID`
    /// profile at this key that `ensure` would delete first" --
    /// `resolve`'s own distinction, which `read` collapses into one
    /// `Observation::Absent` (this module's own doc, "`INVALID` is
    /// replaced, not terminal"). Re-resolves rather than caching `read`'s
    /// own answer: `Tool::replaces`'s doc explains why a second call is
    /// the chosen cost, and this tool's `ensure` already re-resolves for
    /// the identical reason.
    fn replaces(&self, inputs: &Inputs) -> Result<bool, ToolError> {
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
        Ok(matches!(
            Self::resolve(&client, &identifier, &name, &profile_type, &certificate)?,
            ProfileResolution::Invalid { .. }
        ))
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
        match Self::resolve(&client, &identifier, &name, &profile_type, &certificate)? {
            ProfileResolution::Decided(Observation::Present(outputs)) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            ProfileResolution::Decided(Observation::Mismatch { .. }) => Err(ToolError {
                kind: ToolErrorKind::Conflict,
                message: "the existing profile does not match what was requested, and cannot \
                          be converged (there is no update operation for a profile); replace \
                          it instead"
                    .to_string(),
            }),
            ProfileResolution::Decided(Observation::Foreign) => {
                unreachable!("appstore.profile.ensure never observes Foreign")
            }
            ProfileResolution::Decided(Observation::Absent { .. }) => {
                unreachable!("resolve_instance never decides Absent")
            }
            ProfileResolution::NotFound => {
                Self::create_new(&client, &identifier, &name, &profile_type, &certificate)
            }
            // Replace-when-INVALID: delete by the id `resolve` returned,
            // then create fresh. A delete failure propagates before any
            // create is attempted -- `?` never reaches `create_new`. A
            // create failure *after* the delete leaves no profile at this
            // key (a re-run creates it), so the error says the delete
            // already happened rather than naming only the create's own
            // failure (2026-09-29 adversarial pass).
            ProfileResolution::Invalid { id } => {
                client.delete_profile(&id)?;
                Self::create_new(&client, &identifier, &name, &profile_type, &certificate).map_err(
                    |error| ToolError {
                        kind: error.kind,
                        message: format!(
                            "the INVALID profile at this key was deleted, but creating its \
                             replacement failed; re-run to create it: {}",
                            error.message
                        ),
                    },
                )
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
    fn spec_is_destructive_and_not_pure() {
        // Class::Destructive, not Reversible: `ensure` can now delete an
        // `INVALID` profile before creating fresh. See this module's own
        // doc, "`INVALID` is replaced, not terminal".
        assert_eq!(tool().spec().class, Class::Destructive);
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
