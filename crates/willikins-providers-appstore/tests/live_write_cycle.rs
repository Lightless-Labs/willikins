//! The live App Store Connect **write** cycle: the one test in this
//! crate that creates something real and then removes it again. Extended
//! by milestone 3c (decision (h)) to also select a real distribution
//! certificate and produce a real `IOS_APP_STORE` profile, on top of the
//! bundle identifier this file already created and deleted.
//!
//! **This is the operator's live developer account. There is no sandbox
//! team, and Apple offers no sandbox for this API.** Every rule below is
//! absolute, per every trust boundary
//! `docs/plans/2026-09-22-milestone-3c-app-store-signing.md` states:
//!
//! - **The only permitted certificate operation is `GET`.** Never
//!   create, modify, (de)activate, revoke, or delete a certificate --
//!   not here, not anywhere in this crate
//!   (`tests/no_certificate_writes_guard.rs` enforces it structurally).
//!   This file only ever selects one that already exists.
//! - Never modify, rename, delete, or change a capability on any
//!   identifier, app, certificate, profile, or device that already
//!   exists. This test only ever touches the one identifier and the (at
//!   most two) profiles it creates itself.
//! - At most **one** identifier and **at most two** profiles are created
//!   in this whole test, each with an unmistakable throwaway name on a
//!   reverse-domain (or prefix) obviously not the operator's own
//!   (`com.willikins.probe.delete-me.<pid>-<unix-time>`,
//!   `willikins-probe-delete-me-<pid>-<unix-time>`), unique per run so a
//!   leftover from an aborted run is unambiguous, created and deleted
//!   within this same test, with the account's certificate, profile, and
//!   bundle id counts read and reported both before and after.
//! - **No capability is ever enabled here.** See
//!   `tests/bundle_id_capability_ensure_mock.rs`'s own note (unchanged
//!   from before this milestone).
//! - If anything surprises (a duplicate, an unexpected status, a filter
//!   that matches more than the one identifier or profile this run
//!   made): the guard below still deletes what this run created, but the
//!   test fails loudly rather than continuing.
//! - **A profile references a real production certificate.** That
//!   reference is read-only -- nothing about the certificate changes
//!   when a profile names it, and deleting the profile deletes only the
//!   reference -- stated here so no reviewer discovers it by surprise.
//! - **No certificate name, serial, id, or any profile's name, uuid, id,
//!   or content, ever reaches a file, a log, a commit, a command line, or
//!   this test's own panic messages** -- only counts, statuses, and the
//!   lengths of things, and only the throwaway names this run itself
//!   picked (never a real one).
//!
//! Compiled only with the crate's `live-tests` feature (this file's
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. `#[ignore]` on top of
//! that, and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`.
//!
//! The three credential parts live in the operator's **sandbox Doppler
//! workplace**, project `app-store-connect`, config `prd` -- *not* in
//! `~/.config/willikins/sandbox.env`, which holds only the Doppler token
//! that unlocks them. Resolve them in the same command, so no value ever
//! reaches a file, a log, or a command line:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env \
//!   && J=$(printf 'header = "Authorization: Bearer %s"\n' "$WILLIKINS_DOPPLER_TOKEN" \
//!        | curl -sf -K - \
//!        "https://api.doppler.com/v3/configs/config/secrets/download?project=app-store-connect&config=prd&format=json") \
//!   && export ASC_API_KEY_ISSUER_ID=$(printf '%s' "$J" | jq -r .ASC_API_KEY_ISSUER_ID) \
//!             ASC_API_KEY_ID=$(printf '%s' "$J" | jq -r .ASC_API_KEY_ID) \
//!             ASC_API_KEY_BASE64=$(printf '%s' "$J" | jq -r .ASC_API_KEY_BASE64) \
//!   && unset J \
//!   && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 cargo test \
//!        -p willikins-providers-appstore --features live-tests \
//!        --test live_write_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! The Doppler token reaches `curl` on its standard input (`-K -`), never
//! its argument list, and each value reaches `jq` through a pipe from the
//! `printf` builtin, never a here-string (which zsh spills to a temporary
//! file, private key included).
//!
//! If, and only if, more than one usable `DISTRIBUTION` certificate
//! exists on the account (the milestone 3c pre-flight found exactly
//! one), this test also requires `WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL`
//! -- the serial of the certificate to select -- since this test must
//! not guess one (`appstore.certificate.get`'s own refusal to
//! disambiguate is exactly the property this harness respects rather
//! than works around).
//!
//! No `gh` command is run. Nothing here prints a credential, a JWT, or
//! any *other* bundle id's or profile's identifier, name, serial, uuid,
//! id, or content -- only what this run itself created, which is by
//! design never a real one, and only counts and statuses for everything
//! else.

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_appstore::{
    AppstoreBundleIdEnsure, AppstoreCertificateGet, AppstoreClient, AppstoreProfileEnsure,
};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCertificateId,
    AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId, AppleProfileContent,
    AppleProfileName, AppleProfileType, AppleSigningKey, DomainType,
};

fn credential_parts() -> (AppleIssuerId, AppleKeyId, AppleSigningKey) {
    let issuer_id = std::env::var("ASC_API_KEY_ISSUER_ID").expect("ASC_API_KEY_ISSUER_ID is set");
    let key_id = std::env::var("ASC_API_KEY_ID").expect("ASC_API_KEY_ID is set");
    let key_base64 = std::env::var("ASC_API_KEY_BASE64").expect("ASC_API_KEY_BASE64 is set");
    let key_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        key_base64.trim(),
    )
    .unwrap_or_else(|_| panic!("ASC_API_KEY_BASE64 does not decode as base64"));
    // `String::from_utf8`'s `FromUtf8Error` carries the bytes it
    // rejected in its `Debug`, so an `.expect()` here would print the
    // decoded private key into the test log on exactly the run where
    // something went wrong. Drop the error instead.
    let key_pem = String::from_utf8(key_bytes)
        .unwrap_or_else(|_| panic!("the decoded ASC_API_KEY_BASE64 is not valid UTF-8"));
    (
        AppleIssuerId::parse(&issuer_id).expect("ASC_API_KEY_ISSUER_ID is a valid issuer id"),
        AppleKeyId::parse(&key_id).expect("ASC_API_KEY_ID is a valid key id"),
        AppleSigningKey::parse(&key_pem)
            .unwrap_or_else(|_| panic!("ASC_API_KEY_BASE64 does not decode to a valid P-256 key")),
    )
}

/// The raw JWT text this run's credential mints -- the one thing
/// [`bearer_for`] wraps into an opaque, redacted
/// [`willikins_providers_http::Credential`] for every typed call this
/// file makes, and the one thing [`raw_post_profile`]
/// needs directly, since that helper deliberately steps outside
/// `willikins-providers-http`'s typed client (see this file's own module
/// doc, and that function's).
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
fn raw_jwt(issuer_id: &AppleIssuerId, key_id: &AppleKeyId, key: &AppleSigningKey) -> String {
    let credential = willikins_providers_http::AppleSigningCredential::new(issuer_id, key_id, key)
        .expect("builds a credential from the sandbox key");
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(i64::MAX);
    let token = credential.sign(now).expect("signs a token");
    token.as_str().to_owned()
}

/// One freshly minted ES256 JWT, wrapped as a bearer `Credential`.
fn bearer_for(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> willikins_providers_http::Credential {
    willikins_providers_http::Credential::from_bearer_token(
        "WILLIKINS_APPSTORE_LIVE_WRITE_CYCLE",
        raw_jwt(issuer_id, key_id, key),
    )
}

/// A fresh `AppstoreClient` with its own freshly minted JWT -- what the
/// cleanup guard builds at drop time, so a cycle that ran past one
/// token's lifetime still cleans up.
fn fresh_client(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> AppstoreClient {
    AppstoreClient::new(willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(issuer_id, key_id, key),
    ))
}

/// Unwrap a tool call, or STOP naming the step and the error's kind only.
/// A `ToolError`'s message is printed only when it is one of the two
/// fixed status messages (`401`/`403`), which carry nothing of the
/// provider's; any other message may quote Apple's own `detail` text,
/// which can name a certificate or profile id, so it is withheld.
fn tool_ok<T>(result: Result<T, willikins_core::ToolError>, step: &str) -> T {
    result.unwrap_or_else(|err| {
        let fixed = [
            willikins_providers_http::UNAUTHENTICATED,
            willikins_providers_http::MISSING_PERMISSION,
        ];
        let message = if fixed.contains(&err.message.as_str()) {
            err.message.as_str()
        } else {
            "<withheld: may quote provider text>"
        };
        panic!("STOP at {step}: {:?}: {message}", err.kind)
    })
}

/// Paginate `path_and_query` (a full path plus query string, `limit=200`
/// already included) and count every row in every page's `data` array.
/// Shared shape for [`count_bundle_ids`], [`count_certificates`], and
/// [`count_profiles`] -- never returns or prints a row's own fields,
/// only how many there were.
fn count_rows(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
    path_and_query: &str,
) -> usize {
    let http = willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(issuer_id, key_id, key),
    );
    let mut path = path_and_query.to_string();
    let mut total = 0usize;
    for _ in 0..50 {
        let page: serde_json::Value = http
            .get(&path)
            .unwrap_or_else(|err| panic!("STOP: a listing GET failed, status {:?}", err.status));
        total += page
            .get("data")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let Some(next) = page
            .pointer("/links/next")
            .and_then(serde_json::Value::as_str)
        else {
            return total;
        };
        path = next
            .strip_prefix(willikins_providers_appstore::APPSTORE_API_BASE_URL)
            .unwrap_or_else(|| panic!("links.next is not on Apple's own host"))
            .to_string();
    }
    panic!("more than 50 pages for one filter -- refusing to keep paging");
}

/// The account's real bundle identifier **count**. Never returns or
/// prints an identifier: the whole point is to prove that the count
/// after this run equals the count before it, which a filtered read for
/// a string nothing matches cannot do.
fn count_bundle_ids(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/bundleIds?limit=200")
}

/// The account's real certificate **count**, every type, team-wide --
/// this run makes no certificate write of any kind
/// (`tests/no_certificate_writes_guard.rs`), so this count must be
/// identical before and after, and a difference is exactly the kind of
/// surprise trust boundary 6 says to stop for.
fn count_certificates(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/certificates?limit=200")
}

/// The account's real provisioning profile **count**, team-wide (every
/// bundle id, every profile type) -- this run's own two throwaway
/// profiles are deleted before this is read the second time, so before
/// and after must match exactly.
fn count_profiles(issuer_id: &AppleIssuerId, key_id: &AppleKeyId, key: &AppleSigningKey) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/profiles?limit=200")
}

/// Whether a certificate row (as generic JSON) is usable: unexpired, and
/// `activated` either absent or `true` -- mirrors
/// `appstore.certificate.get`'s own health check exactly.
fn certificate_row_is_usable(row: &serde_json::Value) -> bool {
    let attributes = row.get("attributes");
    let activated_ok = attributes
        .and_then(|a| a.get("activated"))
        .and_then(serde_json::Value::as_bool)
        != Some(false);
    let unexpired = attributes
        .and_then(|a| a.get("expirationDate"))
        .and_then(serde_json::Value::as_str)
        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
        .is_none_or(|expires| expires > chrono::Utc::now());
    activated_ok && unexpired
}

/// The serial of the one usable `DISTRIBUTION` certificate this harness
/// will ask `appstore.certificate.get` to select -- read into memory and
/// never printed. If exactly one usable `DISTRIBUTION` certificate
/// exists (the milestone 3c pre-flight found exactly one), its serial is
/// used directly; otherwise `WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL` is
/// required, since this harness must not guess which certificate to
/// reference (`appstore.certificate.get`'s own refusal, respected rather
/// than worked around).
fn usable_distribution_certificate_serial(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> AppleCertificateSerial {
    let http = willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(issuer_id, key_id, key),
    );
    let mut path = "/v1/certificates?filter[certificateType]=DISTRIBUTION&limit=200".to_string();
    let mut usable_serials: Vec<String> = Vec::new();
    for _ in 0..50 {
        let page: serde_json::Value = http
            .get(&path)
            .unwrap_or_else(|err| panic!("STOP: a listing GET failed, status {:?}", err.status));
        if let Some(rows) = page.get("data").and_then(serde_json::Value::as_array) {
            for row in rows {
                let certificate_type = row
                    .pointer("/attributes/certificateType")
                    .and_then(serde_json::Value::as_str);
                if certificate_type != Some("DISTRIBUTION") {
                    // The filter is proven substring elsewhere in this
                    // crate; never trust it without the byte-exact
                    // compare, here too.
                    continue;
                }
                if certificate_row_is_usable(row)
                    && let Some(serial) = row
                        .pointer("/attributes/serialNumber")
                        .and_then(serde_json::Value::as_str)
                {
                    usable_serials.push(serial.to_string());
                }
            }
        }
        let Some(next) = page
            .pointer("/links/next")
            .and_then(serde_json::Value::as_str)
        else {
            break;
        };
        path = next
            .strip_prefix(willikins_providers_appstore::APPSTORE_API_BASE_URL)
            .unwrap_or_else(|| panic!("links.next is not on Apple's own host"))
            .to_string();
    }

    if usable_serials.len() == 1 {
        return AppleCertificateSerial::parse(&usable_serials[0])
            .expect("a real serial parses as AppleCertificateSerial");
    }
    let from_env = std::env::var("WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL").unwrap_or_else(|_| {
        panic!(
            "found {} usable DISTRIBUTION certificates (not exactly 1); this harness will not \
             guess which one to reference -- set WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL",
            usable_serials.len()
        )
    });
    AppleCertificateSerial::parse(from_env.trim())
        .expect("WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL is a valid AppleCertificateSerial")
}

/// The unique suffix (`<pid>-<unix-seconds>`) shared by this run's
/// throwaway identifier and throwaway profile names -- computed once so
/// both carry the identical suffix, per the milestone plan's SHARED
/// VALUES table.
fn run_unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    )
}

/// The one throwaway identifier this run creates -- unmistakably not the
/// operator's own reverse-domain, and unique per run so a leftover from
/// an aborted run is unambiguous rather than colliding silently with a
/// previous run's.
fn throwaway_identifier(unique: &str) -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse(&format!("com.willikins.probe.delete-me.{unique}"))
        .expect("a valid identifier literal")
}

/// The (at most two) throwaway profile names this run creates -- both
/// carry the same name, since decision (h) step 6 asks for a *second*
/// profile under the *same* name to settle whether names are unique per
/// identifier.
fn throwaway_profile_name(unique: &str) -> AppleProfileName {
    AppleProfileName::parse(&format!("willikins-probe-delete-me-{unique}"))
        .expect("a valid profile name literal")
}

fn probe_bundle_name() -> AppleBundleIdName {
    AppleBundleIdName::parse("willikins-live-write-cycle-probe").unwrap()
}

/// `IOS`, not `UNIVERSAL`: this cycle's profile is `IOS_APP_STORE`, and an
/// iOS-only identifier is the pairing that leaves Apple nothing to refuse.
fn probe_platform() -> AppleBundleIdPlatform {
    AppleBundleIdPlatform::parse("IOS").unwrap()
}

/// One raw `POST /v1/profiles`, bypassing `willikins-providers-http`'s
/// typed client entirely -- decision (h) step 6's duplicate-name probe is
/// the one place this file needs the response body a call carries: on
/// success, `data.id` (so this run can record the second profile it just
/// made, immediately, for cleanup -- never by a follow-up list-and-guess,
/// which trust boundary 4 forbids just as much as a delete-by-name would
/// be) and whether it carried `profileContent`; on failure, the
/// `errors[].code` leaf, which `willikins-providers-http`'s own
/// `provider_error_from_body` discards by design. Nothing else in the body
/// is ever read, and nothing of it is printed beyond a status, a code and
/// a boolean.
///
/// The path is fixed, not a parameter: a helper that `POST`s to whatever
/// path it is handed is a certificate write waiting for a caller, and the
/// milestone 3c adversarial pass found this one taking one
/// (`tests/no_certificate_writes_guard.rs` now flags such a call).
///
/// # Panics
///
/// Panics on a transport-level failure (no response at all), or on a
/// `2xx` response whose body carries no `data.id` -- either would mean
/// this harness cannot account for what it just created, which is exactly
/// the "surprise: stop" case trust boundary 6 asks for.
fn raw_post_profile(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
    body: &serde_json::Value,
) -> RawPostResult {
    let jwt = raw_jwt(issuer_id, key_id, key);
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let url = format!(
        "{}/v1/profiles",
        willikins_providers_appstore::APPSTORE_API_BASE_URL
    );
    let mut response = agent
        .post(&url)
        .header("Authorization", format!("Bearer {jwt}"))
        .send_json(body)
        .unwrap_or_else(|_| panic!("STOP: POST /v1/profiles failed at the transport level"));
    let status = response.status().as_u16();
    let body_text = response.body_mut().read_to_string().unwrap_or_default();
    let parsed = serde_json::from_str::<serde_json::Value>(&body_text).ok();
    if (200..300).contains(&status) {
        let id = parsed
            .as_ref()
            .and_then(|value| value.pointer("/data/id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "STOP: a {status} response to POST /v1/profiles carried no data.id -- this \
                     harness cannot account for what it just created, so it will not guess"
                )
            })
            .to_string();
        let content_present = parsed
            .as_ref()
            .and_then(|value| value.pointer("/data/attributes/profileContent"))
            .is_some_and(serde_json::Value::is_string);
        RawPostResult {
            status,
            created_id: Some(id),
            error_code: None,
            content_present,
        }
    } else {
        let error_code = parsed.as_ref().and_then(|value| {
            value
                .pointer("/errors/0/code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        });
        RawPostResult {
            status,
            created_id: None,
            error_code,
            content_present: false,
        }
    }
}

/// [`raw_post_profile`]'s result: the status always; `created_id` and
/// whether the `2xx` body carried `profileContent` on a `2xx` only;
/// `error_code` on a non-`2xx` only, when the body named one.
struct RawPostResult {
    status: u16,
    created_id: Option<String>,
    error_code: Option<String>,
    content_present: bool,
}

/// Deletes every recorded profile id first, then the recorded bundle id
/// -- decision (h) step 7's cleanup order ("Provisioning profiles that
/// contain a deleted App ID become invalid"). Runs on every drop
/// (including an early return from a failed assertion), disarmed only
/// after the explicit cleanup this test performs on its own success path
/// has already run and been confirmed.
struct Guard {
    credential: (AppleIssuerId, AppleKeyId, AppleSigningKey),
    profile_ids: Vec<willikins_types::AppleProfileId>,
    bundle_id: Option<willikins_types::AppleBundleIdId>,
    armed: bool,
}

impl Guard {
    /// `&mut self`, not consuming, since this test still reads
    /// `profile_ids` afterward (for the final 404 checks) -- unlike
    /// `IdentifierGuard::disarm` in the pre-milestone-3c version of this
    /// file, which had nothing left to do with the guard once disarmed.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (issuer_id, key_id, key) = &self.credential;
        let client = fresh_client(issuer_id, key_id, key);
        for profile_id in &self.profile_ids {
            match client.delete_profile(profile_id) {
                Ok(()) => println!("GUARD deleted a throwaway profile by its recorded id"),
                Err(err) => println!(
                    "GUARD could not delete a throwaway profile, status {:?} -- report it",
                    err.status
                ),
            }
        }
        if let Some(bundle_id) = &self.bundle_id {
            match client.delete_bundle_id(bundle_id) {
                Ok(()) => println!("GUARD deleted the throwaway identifier by its recorded id"),
                Err(err) => println!(
                    "GUARD could not delete the throwaway identifier, status {:?} -- report it",
                    err.status
                ),
            }
        }
    }
}

#[test]
#[ignore = "creates and deletes throwaway resources on the operator's LIVE App Store Connect \
            account; run with WILLIKINS_LIVE_TESTS=1 and the sandbox credential sourced in the \
            same command. Never touches a certificate beyond a GET. Read carefully before \
            running: this is a production developer account with no sandbox team."]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
#[allow(clippy::too_many_lines)] // one linear live cycle, in decision (h)'s order, kept in one place
fn appstore_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let (issuer_id, key_id, key) = credential_parts();

    // Step 1: read-only counts first, for all three resources this run
    // touches (or, for certificates, deliberately never touches).
    let certificates_before = count_certificates(&issuer_id, &key_id, &key);
    let profiles_before = count_profiles(&issuer_id, &key_id, &key);
    let bundle_ids_before = count_bundle_ids(&issuer_id, &key_id, &key);
    println!(
        "WRITE-CYCLE counts BEFORE: certificates={certificates_before} \
         profiles={profiles_before} bundle_ids={bundle_ids_before}"
    );

    // Step 2: certificate selection through appstore.certificate.get --
    // never a literal serial this harness picked itself.
    let certificate_type = AppleCertificateType::parse("DISTRIBUTION").unwrap();
    let serial_number = usable_distribution_certificate_serial(&issuer_id, &key_id, &key);
    let certificate_tool =
        AppstoreCertificateGet::new(willikins_providers_appstore::APPSTORE_API_BASE_URL);
    let mut certificate_inputs = willikins_core::Inputs::new();
    certificate_inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id.clone()),
    );
    certificate_inputs.insert(
        PortName::parse("key_id").unwrap(),
        Value::known(key_id.clone()),
    );
    certificate_inputs.insert(PortName::parse("key").unwrap(), Value::known(key.clone()));
    certificate_inputs.insert(
        PortName::parse("certificate_type").unwrap(),
        Value::known(certificate_type),
    );
    certificate_inputs.insert(
        PortName::parse("serial_number").unwrap(),
        Value::known(serial_number),
    );
    let Observation::Present(certificate_outputs) = tool_ok(
        certificate_tool.read(&certificate_inputs),
        "certificate selection",
    ) else {
        panic!("STOP: the selected certificate did not read Present -- investigate by hand");
    };
    let certificate: AppleCertificateId = certificate_outputs
        .get(&PortName::parse("certificate").unwrap())
        .expect("certificate output present")
        .downcast::<AppleCertificateId>()
        .expect("certificate output is an AppleCertificateId")
        .clone();
    println!(
        "certificate selected (id length {})",
        certificate.as_str().len()
    );

    // Step 3: one throwaway identifier.
    let unique = run_unique_suffix();
    let identifier = throwaway_identifier(&unique);
    let bundle_id_tool =
        AppstoreBundleIdEnsure::new(willikins_providers_appstore::APPSTORE_API_BASE_URL);
    let mut bundle_id_inputs = willikins_core::Inputs::new();
    bundle_id_inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id.clone()),
    );
    bundle_id_inputs.insert(
        PortName::parse("key_id").unwrap(),
        Value::known(key_id.clone()),
    );
    bundle_id_inputs.insert(PortName::parse("key").unwrap(), Value::known(key.clone()));
    bundle_id_inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier.clone()),
    );
    bundle_id_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(probe_bundle_name()),
    );
    bundle_id_inputs.insert(
        PortName::parse("platform").unwrap(),
        Value::known(probe_platform()),
    );

    match tool_ok(bundle_id_tool.read(&bundle_id_inputs), "identifier read") {
        Observation::Absent { .. } => {}
        other => panic!(
            "the freshly generated throwaway identifier is not Absent ({other:?}) -- STOP: \
             this is unexpected and must be investigated by hand, not improvised around"
        ),
    }

    let sink = SinkToken::new();
    let created_bundle_id = tool_ok(
        bundle_id_tool.ensure(&bundle_id_inputs, &sink),
        "identifier create",
    );
    let bundle_id = created_bundle_id
        .outputs
        .get(&PortName::parse("id").unwrap())
        .expect("the id output is present")
        .downcast::<willikins_types::AppleBundleIdId>()
        .expect("the id output is an AppleBundleIdId")
        .clone();

    // Guard armed immediately after the identifier's create succeeds --
    // recorded before any further step, per decision (h) step 3.
    let mut guard = Guard {
        credential: (issuer_id.clone(), key_id.clone(), key.clone()),
        profile_ids: Vec::new(),
        bundle_id: Some(bundle_id.clone()),
        armed: true,
    };
    assert!(
        created_bundle_id.changed,
        "the first ensure must create the identifier"
    );

    // Step 4: profile one.
    let profile_name = throwaway_profile_name(&unique);
    let profile_type = AppleProfileType::parse("IOS_APP_STORE").unwrap();
    let profile_tool =
        AppstoreProfileEnsure::new(willikins_providers_appstore::APPSTORE_API_BASE_URL);
    let mut profile_inputs = willikins_core::Inputs::new();
    profile_inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id.clone()),
    );
    profile_inputs.insert(
        PortName::parse("key_id").unwrap(),
        Value::known(key_id.clone()),
    );
    profile_inputs.insert(PortName::parse("key").unwrap(), Value::known(key.clone()));
    profile_inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier.clone()),
    );
    profile_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(profile_name.clone()),
    );
    profile_inputs.insert(
        PortName::parse("profile_type").unwrap(),
        Value::known(profile_type.clone()),
    );
    profile_inputs.insert(
        PortName::parse("certificate").unwrap(),
        Value::known(certificate.clone()),
    );

    // A 403 here means the key cannot create profiles: the plan's own risk
    // list says stop, let the guard delete the identifier, and report.
    let created_profile = tool_ok(
        profile_tool.ensure(&profile_inputs, &sink),
        "profile create",
    );
    let profile_id = created_profile
        .outputs
        .get(&PortName::parse("profile").unwrap())
        .expect("the profile output is present")
        .downcast::<willikins_types::AppleProfileId>()
        .expect("the profile output is an AppleProfileId")
        .clone();

    // Recorded in the guard *before any assertion* -- including the
    // `changed` one, which the task-2 draft asserted first -- per decision
    // (h) step 4.
    guard.profile_ids.push(profile_id.clone());
    assert!(
        created_profile.changed,
        "the first ensure must create the profile"
    );

    let content = created_profile
        .outputs
        .get(&PortName::parse("content").unwrap())
        .expect("the content output is present")
        .downcast::<AppleProfileContent>()
        .expect("the content output is an AppleProfileContent")
        .clone();
    let decoded_bytes = {
        #[allow(clippy::disallowed_methods)] // a live-cycle test exposes its own created secret
        let raw = content.expose(&sink).to_string();
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, raw.trim())
            .expect("the profile content decodes as base64")
    };
    let decoded_text = String::from_utf8_lossy(&decoded_bytes);
    assert!(
        decoded_text.contains("ExpirationDate"),
        "TN3125: an App Store profile's plist has an ExpirationDate"
    );
    assert!(
        !decoded_text.contains("ProvisionedDevices"),
        "TN3125: an App Store profile carries no ProvisionedDevices"
    );
    assert!(
        !decoded_text.contains("ProvisionsAllDevices"),
        "TN3125: an App Store profile carries no ProvisionsAllDevices"
    );
    assert!(
        decoded_bytes.len() <= 65536,
        "profile content must fit AppleProfileContent's own bound"
    );

    // Report length and profileState only -- read via one more typed
    // GET so this run can state the live `profileState`, never any of
    // the profile's other real fields.
    let http = willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(&issuer_id, &key_id, &key),
    );
    let instance: serde_json::Value = http
        .get(&format!(
            "/v1/profiles/{profile_id}?fields[profiles]=profileState"
        ))
        .unwrap_or_else(|err| {
            panic!(
                "STOP: re-reading the just-created profile failed, status {:?}",
                err.status
            )
        });
    let profile_state = instance
        .pointer("/data/attributes/profileState")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
        .to_string();
    println!(
        "profile one created: content length {}, profileState {profile_state}",
        content.expose(&sink).len()
    );

    // Step 5: re-read is Present; re-ensure converges with no change.
    match tool_ok(profile_tool.read(&profile_inputs), "profile re-read") {
        Observation::Present(_) => {}
        other => panic!("expected Present on re-read, got {other:?}"),
    }
    let reensured = tool_ok(
        profile_tool.ensure(&profile_inputs, &sink),
        "profile re-ensure",
    );
    assert!(
        !reensured.changed,
        "a second ensure against an unchanged profile must report changed: false"
    );

    // Step 6: profile two, same name, raw POST through the client's own
    // credential (never the tool, which would read Present and refuse to
    // create a second one) -- settles whether names are unique per
    // identifier.
    let create_body = serde_json::json!({
        "data": {
            "type": "profiles",
            "attributes": {
                "name": profile_name.as_str(),
                "profileType": "IOS_APP_STORE",
            },
            "relationships": {
                "bundleId": {"data": {"type": "bundleIds", "id": bundle_id.to_string()}},
                "certificates": {"data": [{"type": "certificates", "id": certificate.to_string()}]},
            },
        },
    });
    let second_create = raw_post_profile(&issuer_id, &key_id, &key, &create_body);
    if let Some(created_id) = &second_create.created_id {
        println!(
            "profile two: {} -- names are NOT unique per identifier; the 2xx carried \
             profileContent: {}",
            second_create.status, second_create.content_present
        );
        // Recorded immediately, from the create response's own `data.id`
        // -- never a follow-up list-and-guess, which would be exactly
        // the delete-by-filter trust boundary 4 forbids, one step removed.
        let second_profile_id =
            willikins_types::AppleProfileId::parse(created_id).expect("a real profile id parses");
        guard.profile_ids.push(second_profile_id);
    } else {
        println!(
            "profile two: status {}, errors[].code {}",
            second_create.status,
            second_create.error_code.as_deref().unwrap_or("<none>")
        );
    }
    assert!(
        guard.profile_ids.len() <= 2,
        "never more than two profiles created by this run"
    );

    // Step 7: cleanup, in order -- profiles first, then the identifier,
    // each by the id its own create returned, through a client with a
    // freshly minted JWT.
    let client = fresh_client(&issuer_id, &key_id, &key);
    for profile_id in guard.profile_ids.clone() {
        client.delete_profile(&profile_id).unwrap_or_else(|err| {
            panic!(
                "STOP: deleting a throwaway profile failed, status {:?}",
                err.status
            )
        });
    }
    client.delete_bundle_id(&bundle_id).unwrap_or_else(|err| {
        panic!(
            "STOP: deleting the throwaway identifier failed, status {:?}",
            err.status
        )
    });
    println!(
        "cleanup: {} profile(s) then the identifier deleted by recorded id",
        guard.profile_ids.len()
    );
    let after_delete = tool_ok(bundle_id_tool.read(&bundle_id_inputs), "identifier re-read");
    assert!(
        matches!(after_delete, Observation::Absent { .. }),
        "expected Absent after delete, got {after_delete:?}"
    );
    guard.disarm();

    // Step 8: counts after equal counts before, for all three; an
    // independent read confirms the identifier and every profile id
    // answer 404.
    let certificates_after = count_certificates(&issuer_id, &key_id, &key);
    let profiles_after = count_profiles(&issuer_id, &key_id, &key);
    let bundle_ids_after = count_bundle_ids(&issuer_id, &key_id, &key);
    println!(
        "WRITE-CYCLE counts AFTER: certificates={certificates_after} profiles={profiles_after} \
         bundle_ids={bundle_ids_after}"
    );
    assert_eq!(
        certificates_before, certificates_after,
        "the account's certificate count changed -- this test never writes a certificate, so \
         this would mean something else changed the account while it ran"
    );
    assert_eq!(
        profiles_before, profiles_after,
        "the account's profile count changed -- something this test created was not cleaned \
         up, or something else changed the account while it ran"
    );
    assert_eq!(
        bundle_ids_before, bundle_ids_after,
        "the account's bundle id count changed -- something this test created was not cleaned \
         up, or something else changed the account while it ran"
    );

    let independent_http = willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(&issuer_id, &key_id, &key),
    );
    // A body on success would be the deleted thing's own record; only the
    // status is ever looked at, never printed beyond it.
    let status_of = |path: &str| -> Option<u16> {
        match independent_http.get::<serde_json::Value>(path) {
            Ok(_) => Some(200),
            Err(err) => err.status,
        }
    };
    assert_eq!(
        status_of(&format!("/v1/bundleIds/{bundle_id}")),
        Some(404),
        "the deleted identifier must answer 404"
    );
    for profile_id in &guard.profile_ids {
        assert_eq!(
            status_of(&format!("/v1/profiles/{profile_id}")),
            Some(404),
            "a deleted profile must answer 404"
        );
    }
    println!("independent read: the identifier and every profile id answer 404");

    println!(
        "write cycle complete: identifier and {} profile(s) deleted",
        guard.profile_ids.len()
    );
}
