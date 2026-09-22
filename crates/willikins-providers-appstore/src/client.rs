//! [`AppstoreClient`]: typed calls for exactly the App Store Connect
//! endpoints the two tools in this crate need, plus the JSON:API request
//! and response shapes those calls use.
//!
//! No tool ever builds a URL itself; every path this client builds is
//! assembled from already-validated domain types
//! ([`AppleBundleIdentifier`], [`AppleBundleIdId`]), whose grammars keep
//! a `/`, `?`, or `&` from ever reaching a request line.
//!
//! # Minting a fresh client per call, and how the key's bytes reach it
//! without a `SinkToken`
//!
//! Unlike `willikins-providers-buildkite`/`-doppler`/`-github`, this
//! crate builds its client fresh on every `Tool::read`/`Tool::ensure`
//! call, from that call's own three already-typed credential ports (see
//! this crate's own module doc). [`client_for`] is the one place that
//! happens: it mints an
//! [`AppleSigningCredential`](willikins_providers_http::AppleSigningCredential)
//! (which itself reads `key`'s bytes through
//! [`willikins_types::AppleSigningKey::reveal_for_signing`], the
//! token-less exception that type's own doc explains), signs one JWT
//! against the current wall-clock time, and wraps that JWT in a
//! [`willikins_providers_http::Credential`] via
//! [`willikins_providers_http::Credential::from_bearer_token`] — so
//! every request this client sends carries a fresh
//! `Authorization: Bearer <jwt>`, and `Http`'s own retry loop attaches it
//! exactly as it would any other provider's credential.
//!
//! See `docs/research/2026-09-16-app-store-connect.md` for every fact
//! this module rests on.

use serde::{Deserialize, Serialize};

use willikins_core::{ToolError, ToolErrorKind};
use willikins_providers_http::{Credential, Http, ProviderError};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier,
    AppleCapabilityType, AppleIssuerId, AppleKeyId, AppleSigningKey,
};

/// App Store Connect's REST API base URL
/// (`docs/research/2026-09-16-app-store-connect.md`, section 1).
pub const APPSTORE_API_BASE_URL: &str = "https://api.appstoreconnect.apple.com";

/// Rows per page this client asks App Store Connect for when listing.
/// Apple's documented maximum for a collection is 200; asking for it
/// keeps the number of round trips down without relying on whatever
/// default Apple would otherwise apply (this crate has never observed
/// one, and does not assume it).
const PAGE_LIMIT: usize = 200;

/// The most pages [`AppstoreClient::list_bundle_ids`] will follow before
/// refusing. At [`PAGE_LIMIT`] rows each this is 10 000 bundle ids
/// matching one substring, which is not a real account; reaching it
/// means a paging bug, and spinning forever is the one outcome worse
/// than an error.
const MAX_PAGES: usize = 50;

/// The label [`willikins_providers_http::Credential::from_bearer_token`]
/// carries for this crate's minted JWTs. `Debug`-only; no environment
/// variable of this name is ever read (this crate has no environment
/// credential at all — see the crate's own module doc).
const CREDENTIAL_LABEL: &str = "WILLIKINS_APPSTORE_JWT";

/// The three [`AppleCapabilityType`] members Apple documents as needing
/// an extra portal "Configure" step this API cannot perform
/// (`docs/research/2026-09-16-app-store-connect.md`, section 2, "Apple
/// names the six capabilities that need extra steps" — three of Apple's
/// six are identifier-association capabilities this API can flip on but
/// never finish; the other three of Apple's six, Sign in with Apple,
/// Data protection, and push notifications, are configurable entirely
/// through `CapabilitySetting`, which this crate does not set either,
/// but whose *absence* of a setting is not the same defect: enabling
/// those three with no setting is still a complete, valid state Apple
/// accepts, where enabling one of these three with no group/merchant/
/// container attached is not). `appstore.bundle_id_capability.ensure`'s
/// own module doc explains what this crate does about it.
pub const CAPABILITIES_NEEDING_PORTAL_CONFIGURATION: [&str; 3] =
    ["APP_GROUPS", "APPLE_PAY", "ICLOUD"];

/// Build a fresh [`AppstoreClient`] against `base_url`, minting one JWT
/// from `issuer_id`/`key_id`/`key` (see this module's own doc).
///
/// # Errors
///
/// Returns [`ToolError`] (kind [`ToolErrorKind::Provider`]) if the key
/// does not load as a signable EC key, or if `jsonwebtoken` itself fails
/// to sign — both wrap
/// [`willikins_providers_http::AppleCredentialError`], never the key's
/// bytes.
pub(crate) fn client_for(
    base_url: &str,
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> Result<AppstoreClient, ToolError> {
    let credential = willikins_providers_http::AppleSigningCredential::new(issuer_id, key_id, key)
        .map_err(|err| ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("could not build an App Store Connect credential: {err}"),
        })?;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(i64::MAX);
    let token = credential.sign(now).map_err(|err| ToolError {
        kind: ToolErrorKind::Provider,
        message: format!("could not sign an App Store Connect API token: {err}"),
    })?;
    let bearer = Credential::from_bearer_token(CREDENTIAL_LABEL, token.as_str().to_owned());
    Ok(AppstoreClient::new(Http::new(base_url, Vec::new(), bearer)))
}

/// A typed App Store Connect REST client, bound to one [`Http`] (which
/// itself owns the freshly minted bearer [`Credential`]).
pub struct AppstoreClient {
    http: Http,
}

impl AppstoreClient {
    /// Build a client over `http`. `pub` (not `pub(crate)`) so a test can
    /// build one directly against a mock server, exactly like
    /// `willikins_providers_buildkite::BuildkiteClient::new`.
    #[must_use]
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// `GET /v1/bundleIds?filter[identifier]={identifier}`, every page of
    /// it.
    ///
    /// Returns every row the provider's filter matched — **never**
    /// treated as an exact match by this client itself. That was already
    /// the rule when `filter[identifier]`'s semantics were merely
    /// undocumented; a live read against a real account on 2026-09-22
    /// settled them, and the answer is the worst of the three:
    /// **`filter[identifier]` matches by substring** (research note,
    /// section 2, "Reading back by key"). A strict prefix of a real
    /// identifier and a strict suffix of the same identifier each
    /// returned that identifier's own row.
    ///
    /// Two consequences, both load-bearing:
    ///
    /// 1. The caller's byte-for-byte comparison
    ///    (`appstore.bundle_id.ensure::find_one`) is **the** thing that
    ///    decides a match, not a belt-and-braces double-check. Without
    ///    it a read for `com.acme.app` would report `Present` for
    ///    `com.acme.app.extension`.
    /// 2. **This call must paginate.** Under substring matching the
    ///    result set is "every identifier on the team containing this
    ///    string", which is unbounded in a way an exact filter never
    ///    would be. A single unpaginated page would silently drop the
    ///    exact match whenever it sorted past the page boundary, and the
    ///    tool would read `Absent` for something that exists — then
    ///    `POST`, take Apple's duplicate error, re-read `Absent` again,
    ///    and fail. So this asks for [`PAGE_LIMIT`] rows and follows
    ///    `links.next` until Apple stops sending one.
    ///
    /// `links.next` is an absolute URL. Only its query string is used,
    /// re-attached to this client's own `/v1/bundleIds` path, so a
    /// response can never redirect this client at a host or a path of
    /// the provider's choosing.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for any non-2xx response, a transport
    /// failure, or more than [`MAX_PAGES`] pages (which would mean a
    /// paging bug, not a real account).
    pub(crate) fn list_bundle_ids(
        &self,
        identifier: &AppleBundleIdentifier,
    ) -> Result<Vec<BundleIdResource>, ProviderError> {
        let mut path = format!("/v1/bundleIds?filter[identifier]={identifier}&limit={PAGE_LIMIT}");
        let mut rows: Vec<BundleIdResource> = Vec::new();
        for _ in 0..MAX_PAGES {
            let response: BundleIdListResponse = self.http.get(&path)?;
            rows.extend(response.data);
            let Some(next) = response.links.and_then(|links| links.next) else {
                return Ok(rows);
            };
            let Some((_, query)) = next.split_once('?') else {
                // A `next` with no query is not a page this client can
                // follow; treat the listing as finished rather than
                // re-requesting page one forever.
                return Ok(rows);
            };
            path = format!("/v1/bundleIds?{query}");
        }
        Err(ProviderError::new(
            None,
            format!(
                "App Store Connect returned more than {MAX_PAGES} pages of bundle ids for one \
                 filter; refusing to keep paging"
            ),
        ))
    }

    /// `POST /v1/bundleIds` with exactly `identifier`, `name`, and
    /// `platform` -- no `seedId` (the one other create attribute,
    /// optional and undocumented in shape; this crate never sets it).
    ///
    /// # Errors
    ///
    /// See [`Self::list_bundle_ids`].
    pub(crate) fn create_bundle_id(
        &self,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
        platform: &AppleBundleIdPlatform,
    ) -> Result<BundleIdResource, ProviderError> {
        let body = BundleIdCreateBody {
            data: BundleIdCreateData {
                type_: "bundleIds",
                attributes: BundleIdCreateAttributes {
                    name: name.to_string(),
                    platform: platform.to_string(),
                    identifier: identifier.to_string(),
                },
            },
        };
        let response: BundleIdResponse = self.http.post("/v1/bundleIds", &body)?;
        Ok(response.data)
    }

    /// `PATCH /v1/bundleIds/{id}` with exactly `name` -- the only
    /// attribute `BundleIdUpdateRequest` declares (research note,
    /// section 2, "Immutability is provable from the shape of the update
    /// schema").
    ///
    /// # Errors
    ///
    /// See [`Self::list_bundle_ids`].
    pub(crate) fn update_bundle_id_name(
        &self,
        id: &AppleBundleIdId,
        name: &AppleBundleIdName,
    ) -> Result<(), ProviderError> {
        let path = format!("/v1/bundleIds/{id}");
        let body = BundleIdUpdateBody {
            data: BundleIdUpdateData {
                type_: "bundleIds",
                id: id.to_string(),
                attributes: BundleIdUpdateAttributes {
                    name: name.to_string(),
                },
            },
        };
        self.http.patch::<BundleIdResponse>(&path, &body)?;
        Ok(())
    }

    /// `DELETE /v1/bundleIds/{id}`.
    ///
    /// **Used only by the opt-in live write cycle**
    /// (`tests/live_write_cycle.rs`, behind the `live-tests` feature), to
    /// remove the throwaway identifier it created; no tool in this crate
    /// calls it -- `pub` rather than `pub(crate)` only because that test
    /// lives in a separate crate, mirroring
    /// `willikins_providers_buildkite::BuildkiteClient::delete_pipeline`'s
    /// own doc comment exactly.
    ///
    /// # Errors
    ///
    /// See [`Self::list_bundle_ids`].
    pub fn delete_bundle_id(&self, id: &AppleBundleIdId) -> Result<(), ProviderError> {
        self.http.delete(&format!("/v1/bundleIds/{id}"))
    }

    /// `GET /v1/bundleIds/{id}/bundleIdCapabilities`.
    ///
    /// The only way to read a bundle id's capabilities at all: the
    /// capability resource has no `GET` of its own (research note,
    /// section 2, "Capabilities live on a separate resource whose
    /// operation list is conspicuously asymmetric").
    ///
    /// # Errors
    ///
    /// See [`Self::list_bundle_ids`].
    pub(crate) fn list_bundle_id_capabilities(
        &self,
        id: &AppleBundleIdId,
    ) -> Result<Vec<CapabilityResource>, ProviderError> {
        let path = format!("/v1/bundleIds/{id}/bundleIdCapabilities");
        let response: CapabilityListResponse = self.http.get(&path)?;
        Ok(response.data)
    }

    /// `POST /v1/bundleIdCapabilities` with exactly `capabilityType` and
    /// the `bundleId` relationship -- no `settings` (research note,
    /// section 2: "the only configuration surface is
    /// `attributes.settings[]`"; this crate never sets one, and
    /// [`CAPABILITIES_NEEDING_PORTAL_CONFIGURATION`] is exactly the set
    /// whose configuration `settings` cannot express in the first
    /// place -- `appstore.bundle_id_capability.ensure` refuses those
    /// before ever reaching this call).
    ///
    /// # Errors
    ///
    /// See [`Self::list_bundle_ids`].
    pub(crate) fn create_bundle_id_capability(
        &self,
        bundle_id: &AppleBundleIdId,
        capability: &AppleCapabilityType,
    ) -> Result<(), ProviderError> {
        let body = CapabilityCreateBody {
            data: CapabilityCreateData {
                type_: "bundleIdCapabilities",
                attributes: CapabilityCreateAttributes {
                    capability_type: capability.to_string(),
                },
                relationships: CapabilityRelationships {
                    bundle_id: RelationshipRef {
                        data: RelationshipData {
                            type_: "bundleIds",
                            id: bundle_id.to_string(),
                        },
                    },
                },
            },
        };
        self.http
            .post::<CapabilityCreateResponse>("/v1/bundleIdCapabilities", &body)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------
// JSON:API shapes. Every struct deserializes (or serializes) *exactly*
// the fields this crate reads or sends -- serde's default behaviour (no
// `deny_unknown_fields`) silently discards anything else Apple's real
// response carries.
// ---------------------------------------------------------------------

/// One `bundleIds` resource's attributes -- exactly the three create
/// requires, the only three this client ever reads back.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BundleIdAttributes {
    pub(crate) name: String,
    pub(crate) platform: String,
    pub(crate) identifier: String,
}

/// One `bundleIds` resource.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BundleIdResource {
    pub(crate) id: String,
    pub(crate) attributes: BundleIdAttributes,
}

#[derive(Debug, Deserialize)]
struct BundleIdResponse {
    data: BundleIdResource,
}

/// JSON:API's `links` object, of which this client reads only `next` --
/// the cursor URL Apple sends while more pages remain, and omits on the
/// last one.
#[derive(Debug, Deserialize)]
struct Links {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleIdListResponse {
    data: Vec<BundleIdResource>,
    /// Absent on a response with no further pages -- and absent from
    /// every mock fixture in this crate that predates pagination, which
    /// is why it is `Option` rather than defaulted.
    links: Option<Links>,
}

#[derive(Debug, Serialize)]
struct BundleIdCreateAttributes {
    name: String,
    platform: String,
    identifier: String,
}

#[derive(Debug, Serialize)]
struct BundleIdCreateData {
    #[serde(rename = "type")]
    type_: &'static str,
    attributes: BundleIdCreateAttributes,
}

#[derive(Debug, Serialize)]
struct BundleIdCreateBody {
    data: BundleIdCreateData,
}

#[derive(Debug, Serialize)]
struct BundleIdUpdateAttributes {
    name: String,
}

#[derive(Debug, Serialize)]
struct BundleIdUpdateData {
    #[serde(rename = "type")]
    type_: &'static str,
    id: String,
    attributes: BundleIdUpdateAttributes,
}

#[derive(Debug, Serialize)]
struct BundleIdUpdateBody {
    data: BundleIdUpdateData,
}

/// One `bundleIdCapabilities` resource's attributes -- only
/// `capabilityType`, the sole field this client reads (never `settings`;
/// see [`AppstoreClient::create_bundle_id_capability`]'s own doc).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CapabilityAttributes {
    #[serde(rename = "capabilityType")]
    pub(crate) capability_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CapabilityResource {
    pub(crate) attributes: CapabilityAttributes,
}

#[derive(Debug, Deserialize)]
struct CapabilityListResponse {
    data: Vec<CapabilityResource>,
}

#[derive(Debug, Deserialize)]
struct CapabilityCreateResponse {
    #[allow(dead_code)] // deserialized only to prove the response parses; never read
    data: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct CapabilityCreateAttributes {
    #[serde(rename = "capabilityType")]
    capability_type: String,
}

#[derive(Debug, Serialize)]
struct RelationshipData {
    #[serde(rename = "type")]
    type_: &'static str,
    id: String,
}

#[derive(Debug, Serialize)]
struct RelationshipRef {
    data: RelationshipData,
}

#[derive(Debug, Serialize)]
struct CapabilityRelationships {
    #[serde(rename = "bundleId")]
    bundle_id: RelationshipRef,
}

#[derive(Debug, Serialize)]
struct CapabilityCreateData {
    #[serde(rename = "type")]
    type_: &'static str,
    attributes: CapabilityCreateAttributes,
    relationships: CapabilityRelationships,
}

#[derive(Debug, Serialize)]
struct CapabilityCreateBody {
    data: CapabilityCreateData,
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::DomainType;

    #[test]
    fn client_for_signs_a_token_and_builds_a_client() {
        let issuer_id = AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap();
        let key_id = AppleKeyId::parse("2X9R4HXF34").unwrap();
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        let client = client_for("http://127.0.0.1:1", &issuer_id, &key_id, &key);
        assert!(client.is_ok());
    }

    #[test]
    fn capabilities_needing_portal_configuration_are_the_three_documented_ones() {
        assert_eq!(
            CAPABILITIES_NEEDING_PORTAL_CONFIGURATION,
            ["APP_GROUPS", "APPLE_PAY", "ICLOUD"]
        );
    }
}
