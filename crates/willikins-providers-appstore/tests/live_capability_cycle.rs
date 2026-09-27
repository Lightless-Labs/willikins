//! The live App Store Connect **capability** cycle (milestone 3e, task 1,
//! acceptance test 10). **Written by the implementer; run by the
//! attacker, once, against the operator's live account** -- this file
//! makes no call on its own; nothing here executes without
//! `WILLIKINS_LIVE_TESTS=1` on top of the `live-tests` feature this
//! crate's `[[test]]` entry gates it behind.
//!
//! **This is the operator's live developer account. There is no sandbox
//! team, and Apple offers no sandbox for this API.** Every trust
//! boundary `docs/plans/2026-09-27-milestone-3e-new-ios-app.md` states is
//! absolute here, and this file follows `tests/live_write_cycle.rs`'s own
//! shape and idioms (credential resolution, counting, the throwaway
//! identifier, the drop guard) rather than reinventing them:
//!
//! - **The only permitted certificate operation is `GET`.** This file
//!   only ever counts certificates (trust boundary 5), the same
//!   read-only listing `tests/live_write_cycle.rs`'s own `count_certificates`
//!   performs -- never a single certificate's own fields, and no
//!   certificate is created, modified, or touched in any other way.
//! - **Never `APPLE_ID_AUTH`, live, on any identifier, throwaway
//!   included** (trust boundary 4): "App IDs can't be deleted if they are
//!   grouped with other apps for features like Sign in with Apple," which
//!   would break the guarantee this test's own guard depends on. Only
//!   `HEALTHKIT`, `PUSH_NOTIFICATIONS`, and `DATA_PROTECTION` are ever
//!   enabled here; `APPLE_ID_AUTH`'s pairing is exercised by mocks only
//!   (`bundle_id_capability_ensure_mock.rs`), never live.
//! - **At most one identifier is created**, with an unmistakable
//!   throwaway name (`com.willikins.probe.delete-me.<pid>-<unix-time>`,
//!   unique per run), created and deleted within this same test, with the
//!   account's bundle id, profile, and API-visible certificate counts
//!   (trust boundary 5) read and reported both before and after -- this
//!   cycle creates no profile and touches no certificate, so all three
//!   must come back unchanged, and a mover is exactly the surprise trust
//!   boundary 6 says to stop for.
//! - Never modifies, renames, or deletes any identifier, capability, app,
//!   certificate, profile, or device that already exists. This test only
//!   ever touches the one identifier it creates itself.
//! - If anything surprises (an unexpected status, a capability row whose
//!   settings shape differs from decision (d)'s assumption, a count that
//!   moved): the guard below still deletes what this run created, but the
//!   test fails loudly rather than continuing or improvising around it.
//! - **No identifier, capability row, or setting's raw JSON ever reaches
//!   a file, a log, a commit, a command line, or this test's own panic
//!   messages** -- only counts, statuses, capability and setting *key
//!   names* (all four of which are already public in this crate's own
//!   grammar, never a real record's content), and booleans.
//!
//! Compiled only with the crate's `live-tests` feature (this file's own
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. `#[ignore]` on top of
//! that, and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`.
//!
//! The three credential parts live in the operator's **sandbox Doppler
//! workplace**, project `app-store-connect`, config `prd` -- resolve them
//! in the same command, exactly as `tests/live_write_cycle.rs`'s own
//! module doc shows, so no value ever reaches a file, a log, or a command
//! line:
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
//!        --test live_capability_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! The Doppler token reaches `curl` on its standard input (`-K -`), never
//! its argument list, and each value reaches `jq` through a pipe from the
//! `printf` builtin, never a here-string.
//!
//! **Verify items this run settles (decision (d), the plan's "Verify
//! before relying on them"):** item 1 (does a create carrying `settings`
//! succeed at all), item 2 (is the selected option the one marked
//! `enabled: true`), item 3 (can this key enable a capability at all).
//! If Apple's shape differs, this test's own assertions name exactly
//! where, and the fix is a plan addendum plus an adapted parse, test
//! first -- never a workaround that makes the assertion pass without
//! settling the fact.

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_appstore::{AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCapabilitySetting,
    AppleCapabilityType, AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType,
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
    // Dropped rather than `.expect()`ed: `FromUtf8Error`'s `Debug` carries
    // the bytes it rejected, which would print the decoded private key
    // into the test log on exactly the run where the decode went wrong
    // (`tests/live_write_cycle.rs`'s own doc names this same defect).
    let key_pem = String::from_utf8(key_bytes)
        .unwrap_or_else(|_| panic!("the decoded ASC_API_KEY_BASE64 is not valid UTF-8"));
    (
        AppleIssuerId::parse(&issuer_id).expect("ASC_API_KEY_ISSUER_ID is a valid issuer id"),
        AppleKeyId::parse(&key_id).expect("ASC_API_KEY_ID is a valid key id"),
        AppleSigningKey::parse(&key_pem)
            .unwrap_or_else(|_| panic!("ASC_API_KEY_BASE64 does not decode to a valid P-256 key")),
    )
}

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

fn bearer_for(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> willikins_providers_http::Credential {
    willikins_providers_http::Credential::from_bearer_token(
        "WILLIKINS_APPSTORE_LIVE_CAPABILITY_CYCLE",
        raw_jwt(issuer_id, key_id, key),
    )
}

fn http_for(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> willikins_providers_http::Http {
    willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer_for(issuer_id, key_id, key),
    )
}

/// Unwrap a tool call, or STOP naming the step and the error's kind only
/// -- mirrors `tests/live_write_cycle.rs`'s own `tool_ok` exactly, for
/// the same reason: any other message may quote Apple's own `detail`
/// text, which can name a real record, so it is withheld.
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
/// already included) and count every row in every page's `data` array --
/// mirrors `tests/live_write_cycle.rs`'s own `count_rows` exactly, and is
/// shared here the same way that file shares it across
/// `count_bundle_ids`/`count_certificates`/`count_profiles`: never
/// returns or prints a row's own fields, only how many there were.
fn count_rows(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
    path_and_query: &str,
) -> usize {
    let http = http_for(issuer_id, key_id, key);
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

/// The account's real bundle identifier **count**, never returning or
/// printing an identifier.
fn count_bundle_ids(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/bundleIds?limit=200")
}

/// The account's real certificate **count**, every type, team-wide --
/// this run makes no certificate write of any kind (trust boundary 1),
/// so this count must be identical before and after (trust boundary 5),
/// and a difference is exactly the kind of surprise trust boundary 6
/// says to stop for. `GET` only, the one permitted certificate operation.
fn count_certificates(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/certificates?limit=200")
}

/// The account's real provisioning profile **count**, team-wide -- this
/// cycle mints no profile at all, so before and after must match exactly
/// (trust boundary 5).
fn count_profiles(issuer_id: &AppleIssuerId, key_id: &AppleKeyId, key: &AppleSigningKey) -> usize {
    count_rows(issuer_id, key_id, key, "/v1/profiles?limit=200")
}

/// One capability row's settings shape, recorded by **key names and
/// booleans only** (decision (d)'s own instruction): every `settings[]`
/// entry's `key`, and for each of its `options[]`, the option `key` and
/// whether it is `enabled` -- never `name`, `description`, or anything
/// else the row might carry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsShape(Vec<(String, Vec<(String, bool)>)>);

/// Every capability row on `bundle_id`, as `(capabilityType, settings
/// shape)` pairs -- one raw `GET`, parsed only for the fields this test
/// needs (mirrors `tests/live_write_cycle.rs`'s own raw-JSON pattern for
/// exactly the reason that file's module doc gives: this crate's own
/// client keeps `list_bundle_id_capabilities` `pub(crate)`, so an
/// integration test, compiled as an external crate, reaches it only
/// through a fresh, ad hoc `Http` call).
fn capability_rows(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
    bundle_id: &willikins_types::AppleBundleIdId,
) -> Vec<(String, SettingsShape)> {
    let http = http_for(issuer_id, key_id, key);
    let path = format!("/v1/bundleIds/{bundle_id}/bundleIdCapabilities");
    let page: serde_json::Value = http.get(&path).unwrap_or_else(|err| {
        panic!(
            "STOP: capability listing GET failed, status {:?}",
            err.status
        )
    });
    page.get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let capability_type = row
                .pointer("/attributes/capabilityType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing>")
                .to_string();
            let settings = row
                .pointer("/attributes/settings")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .map(|setting| {
                    let key = setting
                        .get("key")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("<missing>")
                        .to_string();
                    let options = setting
                        .get("options")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|option| {
                            let option_key = option
                                .get("key")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("<missing>")
                                .to_string();
                            let enabled = option
                                .get("enabled")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            (option_key, enabled)
                        })
                        .collect();
                    (key, options)
                })
                .collect();
            (capability_type, SettingsShape(settings))
        })
        .collect()
}

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

fn throwaway_identifier(unique: &str) -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse(&format!("com.willikins.probe.delete-me.{unique}"))
        .expect("a valid identifier literal")
}

fn probe_bundle_name() -> AppleBundleIdName {
    AppleBundleIdName::parse("willikins-live-capability-cycle-probe").unwrap()
}

fn probe_platform() -> AppleBundleIdPlatform {
    AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()
}

fn capability_inputs(
    issuer_id: &AppleIssuerId,
    key_id: &AppleKeyId,
    key: &AppleSigningKey,
    identifier: &AppleBundleIdentifier,
    capability: &AppleCapabilityType,
    setting: Option<&AppleCapabilitySetting>,
) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id.clone()),
    );
    inputs.insert(
        PortName::parse("key_id").unwrap(),
        Value::known(key_id.clone()),
    );
    inputs.insert(PortName::parse("key").unwrap(), Value::known(key.clone()));
    inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier.clone()),
    );
    inputs.insert(
        PortName::parse("capability").unwrap(),
        Value::known(capability.clone()),
    );
    if let Some(setting) = setting {
        inputs.insert(
            PortName::parse("setting").unwrap(),
            Value::known(setting.clone()),
        );
    }
    inputs
}

/// Deletes the recorded identifier by its own create id -- runs on every
/// drop (including an early return from a failed assertion), disarmed
/// only after this test's own explicit cleanup has already run and been
/// confirmed. Mirrors `tests/live_write_cycle.rs`'s own `Guard` exactly,
/// scoped down to the one resource this cycle creates.
struct Guard {
    credential: (AppleIssuerId, AppleKeyId, AppleSigningKey),
    bundle_id: Option<willikins_types::AppleBundleIdId>,
    armed: bool,
}

impl Guard {
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
        // Deleting a bundle id removes its capabilities with it -- no
        // separate capability delete exists, or is needed.
        if let Some(bundle_id) = &self.bundle_id {
            let http = http_for(issuer_id, key_id, key);
            match http.delete(&format!("/v1/bundleIds/{bundle_id}")) {
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
#[ignore = "creates and deletes a throwaway identifier and its capabilities on the operator's \
            LIVE App Store Connect account; run with WILLIKINS_LIVE_TESTS=1 and the sandbox \
            credential sourced in the same command. Never touches a certificate or an existing \
            identifier. Read carefully before running: this is a production developer account \
            with no sandbox team."]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
#[allow(clippy::too_many_lines)] // one linear live cycle, kept in one place like its sibling
fn appstore_live_capability_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let (issuer_id, key_id, key) = credential_parts();

    // Step 1: read-only counts first, for all three resources trust
    // boundary 5 names -- this cycle touches only the first, so all
    // three must come back unchanged at step 8.
    let bundle_ids_before = count_bundle_ids(&issuer_id, &key_id, &key);
    let certificates_before = count_certificates(&issuer_id, &key_id, &key);
    let profiles_before = count_profiles(&issuer_id, &key_id, &key);
    println!(
        "CAPABILITY-CYCLE counts BEFORE: bundle_ids={bundle_ids_before} \
         certificates={certificates_before} profiles={profiles_before}"
    );

    // Step 2: one throwaway identifier.
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
    // recorded before any further step.
    let mut guard = Guard {
        credential: (issuer_id.clone(), key_id.clone(), key.clone()),
        bundle_id: Some(bundle_id.clone()),
        armed: true,
    };

    let capability_tool =
        AppstoreBundleIdCapabilityEnsure::new(willikins_providers_appstore::APPSTORE_API_BASE_URL);

    // Step 3: HEALTHKIT and PUSH_NOTIFICATIONS, neither taking a setting
    // -- verify item 3 (can this key enable a capability at all).
    for capability_name in ["HEALTHKIT", "PUSH_NOTIFICATIONS"] {
        let capability = AppleCapabilityType::parse(capability_name).unwrap();
        let inputs = capability_inputs(&issuer_id, &key_id, &key, &identifier, &capability, None);
        let first = tool_ok(capability_tool.ensure(&inputs, &sink), "capability create");
        assert!(
            first.changed,
            "{capability_name}: expected changed: true on first ensure"
        );
        let second = tool_ok(
            capability_tool.ensure(&inputs, &sink),
            "capability re-ensure",
        );
        assert!(
            !second.changed,
            "{capability_name}: expected changed: false on re-ensure"
        );
        println!("{capability_name}: created, then converged");
    }

    // Step 4: DATA_PROTECTION with its required setting -- verify items 1
    // and 2 (does a create carrying `settings` succeed, and is the
    // selected option the one marked `enabled: true`).
    let data_protection = AppleCapabilityType::parse("DATA_PROTECTION").unwrap();
    let requested_setting = AppleCapabilitySetting::parse(
        "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
    )
    .unwrap();
    let with_setting_inputs = capability_inputs(
        &issuer_id,
        &key_id,
        &key,
        &identifier,
        &data_protection,
        Some(&requested_setting),
    );
    let first = tool_ok(
        capability_tool.ensure(&with_setting_inputs, &sink),
        "data protection create",
    );
    assert!(first.changed, "expected changed: true on first ensure");
    let second = tool_ok(
        capability_tool.ensure(&with_setting_inputs, &sink),
        "data protection re-ensure",
    );
    assert!(!second.changed, "expected changed: false on re-ensure");
    println!("DATA_PROTECTION with PROTECTED_UNTIL_FIRST_USER_AUTH: created, then converged");

    // Step 5: a read with a different option requested reads
    // Mismatch { setting }, and makes no write -- the row list is
    // identical before and after it.
    let rows_before_mismatch_read = capability_rows(&issuer_id, &key_id, &key, &bundle_id);
    let mismatched_setting =
        AppleCapabilitySetting::parse("DATA_PROTECTION_PERMISSION_LEVEL=COMPLETE_PROTECTION")
            .unwrap();
    let mismatch_inputs = capability_inputs(
        &issuer_id,
        &key_id,
        &key,
        &identifier,
        &data_protection,
        Some(&mismatched_setting),
    );
    let observation = tool_ok(
        capability_tool.read(&mismatch_inputs),
        "data protection mismatch read",
    );
    match &observation {
        Observation::Mismatch { port } => {
            assert_eq!(port, &PortName::parse("setting").unwrap());
            println!(
                "DATA_PROTECTION with COMPLETE_PROTECTION requested: Mismatch {{ setting }}, as decision (d) predicts"
            );
        }
        other => panic!(
            "STOP: expected Mismatch {{ setting }} for a differently requested option, got \
             {other:?} -- decision (d)'s assumption about the row shape may not hold; record an \
             addendum and adapt the parse, test first, rather than working around this"
        ),
    }
    let rows_after_mismatch_read = capability_rows(&issuer_id, &key_id, &key, &bundle_id);
    assert_eq!(
        rows_before_mismatch_read, rows_after_mismatch_read,
        "STOP: the capability row list changed across a read-only Mismatch observation -- a \
         read must never write"
    );

    // Step 6: the raw shape, recorded by key names and booleans only.
    for (capability_type, shape) in &rows_after_mismatch_read {
        println!("row: capabilityType={capability_type} settings={shape:?}");
    }
    assert!(
        rows_after_mismatch_read
            .iter()
            .any(|(capability_type, _)| capability_type == "DATA_PROTECTION"),
        "STOP: DATA_PROTECTION does not appear in the row list at all"
    );

    // Step 7: cleanup, by the id this run's own create returned.
    let http = http_for(&issuer_id, &key_id, &key);
    http.delete(&format!("/v1/bundleIds/{bundle_id}"))
        .unwrap_or_else(|err| {
            panic!(
                "STOP: deleting the throwaway identifier failed, status {:?}",
                err.status
            )
        });
    println!(
        "cleanup: the identifier deleted by its recorded id (removes its capabilities with it)"
    );
    let after_delete = tool_ok(bundle_id_tool.read(&bundle_id_inputs), "identifier re-read");
    assert!(
        matches!(after_delete, Observation::Absent { .. }),
        "expected Absent after delete, got {after_delete:?}"
    );
    guard.disarm();

    // Step 8: counts after equal counts before, for all three; an
    // independent read confirms the identifier answers 404.
    let bundle_ids_after = count_bundle_ids(&issuer_id, &key_id, &key);
    let certificates_after = count_certificates(&issuer_id, &key_id, &key);
    let profiles_after = count_profiles(&issuer_id, &key_id, &key);
    println!(
        "CAPABILITY-CYCLE counts AFTER: bundle_ids={bundle_ids_after} \
         certificates={certificates_after} profiles={profiles_after}"
    );
    assert_eq!(
        bundle_ids_before, bundle_ids_after,
        "the account's bundle id count changed -- something this test created was not cleaned \
         up, or something else changed the account while it ran"
    );
    assert_eq!(
        certificates_before, certificates_after,
        "the account's certificate count changed -- this test never writes a certificate, so \
         this would mean something else changed the account while it ran"
    );
    assert_eq!(
        profiles_before, profiles_after,
        "the account's profile count changed -- this test never creates a profile, so this \
         would mean something else changed the account while it ran"
    );

    let independent_http = http_for(&issuer_id, &key_id, &key);
    let status = independent_http
        .get::<serde_json::Value>(&format!("/v1/bundleIds/{bundle_id}"))
        .map_or_else(|err| err.status.unwrap_or(0), |_| 200u16);
    assert_eq!(
        status, 404,
        "STOP: the deleted identifier answers {status}, not 404, on an independent read"
    );
    println!("independent confirmation: the deleted identifier answers 404");
}
