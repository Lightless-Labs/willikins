//! `appstore.bundle_id.ensure`: registers (or converges) a real App Store
//! Connect bundle identifier. Port table and behaviour from the task
//! brief this crate implements and
//! `docs/research/2026-09-16-app-store-connect.md`, section 2.
//!
//! # Read: filter, then compare client-side
//!
//! `filter[identifier]` **matches by substring** -- observed live
//! against a real App Store Connect account on 2026-09-22, settling what
//! was until then the research note's single load-bearing unknown
//! (section 2, "Reading back by key"): a strict prefix of a real
//! identifier and a strict suffix of the same identifier each returned
//! that identifier's own row. Apple documents only "filter by attribute
//! 'identifier'" and says nothing about how.
//!
//! So this tool never trusts the filter -- every row the provider
//! returns is compared against `identifier` byte-for-byte
//! (`BundleIdResource::attributes::identifier == identifier.as_str()`)
//! before it counts as a match. That comparison was written when the
//! semantics were merely unknown; now that they are known it is the only
//! thing standing between a read for `com.acme.app` and a `Present`
//! reported for `com.acme.app.extension`. It is load-bearing, not
//! defensive, and `tests/bundle_id_ensure_mock.rs`'s
//! `read_reports_absent_when_only_a_prefix_neighbor_matches_the_filter`
//! is its proof.
//!
//! Substring matching is also why
//! [`crate::client::AppstoreClient::list_bundle_ids`] paginates: the
//! result set is "every identifier on the team containing this string",
//! and an exact match that sorted onto a later page would otherwise read
//! `Absent` for something that exists. See that method's own doc.
//!
//! # No `Foreign` observation
//!
//! `willikins_types::AppleBundleIdName`'s own doc states the gap this
//! tool accepts: a bundle id's only free-text attribute is `name`, so
//! there is no slot for an ownership marker the way
//! `buildkite.pipeline.ensure`'s `description` field carries one. This
//! tool's `read` therefore reports exactly four observations, never
//! `Foreign`:
//!
//! - **`Absent`** -- no row matched `identifier`.
//! - **`Present`** -- one row matched, and its `platform` and `name`
//!   both equal what was requested.
//! - **`Mismatch { port: "name" }`** -- one row matched and its
//!   `platform` is right but its `name` differs. **Convergent**:
//!   `ensure` issues `PATCH /v1/bundleIds/{id}` (the one attribute
//!   Apple's own update schema allows) and reports `changed: true`.
//! - **`Mismatch { port: "platform" }`** -- one row matched and its
//!   `platform` differs (checked first: a wrong platform is the more
//!   structural mismatch, and Apple's schema gives no way to repair it
//!   at all). **Terminal**: `ensure` refuses with
//!   [`willikins_core::ToolErrorKind::Conflict`] naming the constraint,
//!   the same "this tool will not change it" shape
//!   `buildkite.pipeline.ensure`'s own terminal mismatches use.
//!
//! This is a real, stated trade, not an oversight: two different callers
//! naming the same `identifier` with different `name`s will silently
//! rewrite each other's `name` rather than conflict, because nothing
//! about a bundle id record lets this tool tell "ours, drifted" apart
//! from "already someone else's". An operator who needs that
//! distinction must keep `identifier`s disjoint across callers, the same
//! discipline Apple's own reverse-DNS convention already assumes.
//!
//! # A documented gap: `willikins_core::plan` cannot reach the
//! convergent `PATCH` today
//!
//! `Self::ensure`'s `Mismatch { name }` arm genuinely issues the `PATCH`
//! and is exercised directly by this crate's own mock tests
//! (`tests/bundle_id_ensure_mock.rs`'s
//! `ensure_converges_a_name_mismatch_via_patch`) -- calling
//! [`willikins_core::Tool::ensure`] on this tool with a drifted `name`
//! really does converge it. But **`willikins_core::plan`'s existing,
//! workspace-wide invariant makes every `Observation::Mismatch` a hard
//! `PlanError::AttributeMismatch` before any node's `ensure` runs at
//! all** (`willikins_core::plan::plan_one`), and `willikins_core::apply`
//! never calls `Tool::ensure` for a node planned `Action::NoOp` either --
//! it reuses the planned outputs and marks the node `Converged` without
//! touching the tool again. So a document that reaches this tool with a
//! drifted `name` fails at `plan()`, the same as
//! `buildkite.pipeline.ensure`'s own terminal mismatches, *not* the
//! convergent behaviour this module's own doc above describes at the
//! tool level. `tests/bundle_id_documents.rs`'s
//! `a_drifted_name_fails_plan_even_though_ensure_can_converge_it` is the
//! test that makes this concrete rather than left implicit.
//!
//! This is a pre-existing gap in `willikins_core`'s `Observation`/`Action`
//! model (there is no `Action::Update` anywhere in this workspace,
//! reachable from any tool), not something this crate introduced or can
//! fix by itself -- changing `willikins_core::plan`/`apply` to add an
//! update path is a cross-cutting change to every tool's contract and is
//! deliberately left out of this task's scope. `ensure`'s convergent
//! `PATCH` is implemented and tested exactly as instructed (the one real
//! write Apple's API offers for this attribute) so that the day
//! `willikins_core` gains a way to reach it, this tool needs no further
//! change -- but until then, an operator hits `AttributeMismatch`, not a
//! silent rename, which is the safer failure mode of the two.
//!
//! # More than one match
//!
//! If `identifier`'s filter (however it actually matches) ever returns
//! more than one row whose `identifier` equals the requested one
//! exactly, this tool refuses with
//! [`willikins_core::ToolErrorKind::Conflict`] rather than guessing which
//! one is "ours" -- Apple documents no uniqueness constraint on
//! `identifier` this tool could rely on to rule the case out.

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, TypeName,
    TypeRef, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier,
    AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType,
};

use crate::client::{AppstoreClient, BundleIdResource, client_for};

/// `appstore.bundle_id.ensure`.
pub struct AppstoreBundleIdEnsure {
    spec: ToolSpec,
    base_url: String,
}

impl AppstoreBundleIdEnsure {
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
        inputs.insert(port("name"), exact("AppleBundleIdName", true));
        inputs.insert(port("platform"), exact("AppleBundleIdPlatform", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("id"), scalar("AppleBundleIdId"));
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        outputs.insert(port("name"), scalar("AppleBundleIdName"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.bundle_id.ensure"),
                description: "Register or converge an App Store Connect bundle identifier."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier")],
                class: Class::Reversible,
                pure: false,
            },
            base_url: base_url.into(),
        }
    }

    fn outputs_for(
        id: &AppleBundleIdId,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
    ) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("id"), Value::known(id.clone()));
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs.insert(port("name"), Value::known(name.clone()));
        outputs
    }

    fn predicted_outputs(identifier: &AppleBundleIdentifier, name: &AppleBundleIdName) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("id"),
            Value::unknown(TypeRef::scalar(TypeName::parse("AppleBundleIdId").unwrap())),
        );
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs.insert(port("name"), Value::known(name.clone()));
        outputs
    }

    /// Find the (at most one) row whose `identifier` equals `identifier`
    /// exactly -- see this module's own doc, "Read: filter, then compare
    /// client-side".
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::Conflict`] naming `identifier`
    /// and the match count when more than one row matches exactly;
    /// [`willikins_core::ToolErrorKind::Provider`] on any transport or
    /// non-2xx failure.
    fn find_one(
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

    /// Build an [`Observation`] from an already-fetched (optional)
    /// matched resource -- shared by `read` and `ensure`, which both
    /// need the underlying [`BundleIdResource`] (for `ensure`'s
    /// convergent `PATCH`), not only the [`Observation`] it implies.
    fn observe_from(
        resource: Option<&BundleIdResource>,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
        platform: &AppleBundleIdPlatform,
    ) -> Result<Observation, ToolError> {
        let Some(resource) = resource else {
            return Ok(Observation::Absent {
                predicted: Self::predicted_outputs(identifier, name),
            });
        };
        if resource.attributes.platform != platform.as_str() {
            return Ok(Observation::Mismatch {
                port: port("platform"),
            });
        }
        let id = AppleBundleIdId::parse(&resource.id).map_err(|err| ToolError {
            kind: willikins_core::ToolErrorKind::Provider,
            message: format!("App Store Connect returned a malformed bundle id id: {err}"),
        })?;
        if resource.attributes.name != name.as_str() {
            return Ok(Observation::Mismatch { port: port("name") });
        }
        Ok(Observation::Present(Self::outputs_for(
            &id, identifier, name,
        )))
    }

    fn platform_mismatch_conflict(identifier: &AppleBundleIdentifier) -> ToolError {
        conflict(format!(
            "`{identifier}` already exists, but its `platform` does not match what was \
             requested and Apple's API cannot change a bundle id's platform once created; \
             change it by hand in App Store Connect, or pass its current value instead"
        ))
    }

    /// Converge a `Mismatch { name }` by issuing the `PATCH`, or return
    /// the already-`Present` outputs unchanged. Never called for
    /// `Mismatch { platform }` (terminal, handled by the caller) or
    /// `Absent` (handled by the caller, which creates instead).
    fn converge_or_present(
        client: &AppstoreClient,
        resource: Option<BundleIdResource>,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
        observation: Observation,
    ) -> Result<Ensured, ToolError> {
        match observation {
            Observation::Mismatch { port: mismatched } if mismatched == port("platform") => {
                Err(Self::platform_mismatch_conflict(identifier))
            }
            Observation::Mismatch { .. } => {
                let resource =
                    resource.expect("a Mismatch observation always carries a matched resource");
                let id = AppleBundleIdId::parse(&resource.id).map_err(|err| ToolError {
                    kind: willikins_core::ToolErrorKind::Provider,
                    message: format!("App Store Connect returned a malformed bundle id id: {err}"),
                })?;
                client.update_bundle_id_name(&id, name)?;
                Ok(Ensured {
                    outputs: Self::outputs_for(&id, identifier, name),
                    changed: true,
                })
            }
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Foreign | Observation::Absent { .. } => {
                unreachable!("the caller only reaches this helper for Present or Mismatch")
            }
        }
    }
}

fn inputs_of(
    inputs: &Inputs,
) -> Result<
    (
        AppleIssuerId,
        AppleKeyId,
        AppleSigningKey,
        AppleBundleIdentifier,
        AppleBundleIdName,
        AppleBundleIdPlatform,
    ),
    ToolError,
> {
    Ok((
        get(inputs, "issuer_id")?,
        get(inputs, "key_id")?,
        get(inputs, "key")?,
        get(inputs, "identifier")?,
        get(inputs, "name")?,
        get(inputs, "platform")?,
    ))
}

impl Tool for AppstoreBundleIdEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let (issuer_id, key_id, key, identifier, name, platform) = inputs_of(inputs)?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let resource = Self::find_one(&client, &identifier)?;
        Self::observe_from(resource.as_ref(), &identifier, &name, &platform)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let (issuer_id, key_id, key, identifier, name, platform) = inputs_of(inputs)?;
        let client = client_for(&self.base_url, &issuer_id, &key_id, &key)?;
        let resource = Self::find_one(&client, &identifier)?;
        let observation = Self::observe_from(resource.as_ref(), &identifier, &name, &platform)?;
        match observation {
            Observation::Absent { .. } => {
                match client.create_bundle_id(&identifier, &name, &platform) {
                    Ok(resource) => {
                        let id = AppleBundleIdId::parse(&resource.id).map_err(|err| ToolError {
                            kind: willikins_core::ToolErrorKind::Provider,
                            message: format!(
                                "App Store Connect returned a malformed bundle id id: {err}"
                            ),
                        })?;
                        Ok(Ensured {
                            outputs: Self::outputs_for(&id, &identifier, &name),
                            changed: true,
                        })
                    }
                    // Ambiguous create failure: re-read rather than parse
                    // the error body (mirrors
                    // `buildkite.pipeline.ensure`'s own decision, and
                    // "ensure creates, 409 as belt-and-braces after the
                    // read").
                    Err(err) => {
                        let resource = Self::find_one(&client, &identifier)?;
                        let observation =
                            Self::observe_from(resource.as_ref(), &identifier, &name, &platform)?;
                        match observation {
                            Observation::Absent { .. } => Err(err.into()),
                            other => Self::converge_or_present(
                                &client,
                                resource,
                                &identifier,
                                &name,
                                other,
                            ),
                        }
                    }
                }
            }
            other => Self::converge_or_present(&client, resource, &identifier, &name, other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn tool() -> AppstoreBundleIdEnsure {
        AppstoreBundleIdEnsure::new("http://127.0.0.1:1")
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_identifier() {
        assert_eq!(
            tool().spec().key,
            vec![PortName::parse("identifier").unwrap()]
        );
    }

    #[test]
    fn spec_is_reversible_and_not_pure() {
        assert_eq!(tool().spec().class, Class::Reversible);
        assert!(!tool().spec().pure);
    }
}
