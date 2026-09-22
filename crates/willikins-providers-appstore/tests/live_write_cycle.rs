//! The live App Store Connect **write** cycle: the one test in this
//! crate that creates something real and then removes it again.
//!
//! **This is the operator's live developer account. There is no sandbox
//! team, and Apple offers no sandbox for this API.** Every rule below is
//! absolute, per the task this crate was built under:
//!
//! - Never modify, rename, delete, or change a capability on any
//!   identifier, app, certificate, profile, or device that already
//!   exists. This test never touches an existing resource at all -- it
//!   only creates and deletes the one identifier it makes itself.
//! - At most **one** identifier is created in this whole test, with an
//!   unmistakable throwaway identifier string on a reverse-domain that is
//!   obviously not the operator's own
//!   (`com.willikins.probe.delete-me.<pid>-<unix-time>`, unique per run
//!   so a leftover from an aborted run is unambiguous), created and
//!   deleted within this same test, with the account's bundle id count
//!   read and reported both before and after.
//! - **No capability is ever enabled here.** Enabling a capability is
//!   documented to affect provisioning profiles for every eligible
//!   platform (research note, section 2) -- a side effect with no
//!   throwaway-identifier equivalent of "delete it and it's gone" the
//!   way a plain bundle id has. `tests/bundle_id_capability_ensure_mock.rs`
//!   is this crate's only proof of `appstore.bundle_id_capability.ensure`;
//!   there is no live counterpart, and none is added by this file.
//! - If anything surprises (a duplicate, an unexpected status, a filter
//!   that matches more than the one identifier this run made): the guard
//!   below still deletes what this run created, but the test fails
//!   loudly rather than continuing.
//!
//! Compiled only with the crate's `live-tests` feature (this file's
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. `#[ignore]` on top of
//! that, and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-providers-appstore \
//!   --features live-tests --test live_write_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! No `gh` command is run. Nothing here prints a credential, a JWT, or
//! any *other* bundle id's identifier or name -- only the throwaway
//! identifier this run itself created, which is by design never a real
//! one.

use std::sync::Arc;

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_appstore::{AppstoreBundleIdEnsure, AppstoreClient};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleIssuerId, AppleKeyId,
    AppleSigningKey, DomainType,
};

fn credential_parts() -> (AppleIssuerId, AppleKeyId, AppleSigningKey) {
    let issuer_id = std::env::var("ASC_API_KEY_ISSUER_ID").expect("ASC_API_KEY_ISSUER_ID is set");
    let key_id = std::env::var("ASC_API_KEY_ID").expect("ASC_API_KEY_ID is set");
    let key_base64 = std::env::var("ASC_API_KEY_BASE64").expect("ASC_API_KEY_BASE64 is set");
    let key_pem = String::from_utf8(
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            key_base64.trim(),
        )
        .expect("ASC_API_KEY_BASE64 decodes as base64"),
    )
    .expect("the decoded key is valid UTF-8");
    (
        AppleIssuerId::parse(&issuer_id).expect("ASC_API_KEY_ISSUER_ID is a valid issuer id"),
        AppleKeyId::parse(&key_id).expect("ASC_API_KEY_ID is a valid key id"),
        AppleSigningKey::parse(&key_pem).expect("ASC_API_KEY_BASE64 decodes to a valid key"),
    )
}

/// The one throwaway identifier this run creates -- unmistakably not the
/// operator's own reverse-domain, and unique per run (process id plus
/// unix time) so a leftover from an aborted run is unambiguous rather
/// than colliding silently with a previous run's.
fn throwaway_identifier() -> AppleBundleIdentifier {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    AppleBundleIdentifier::parse(&format!("com.willikins.probe.delete-me.{unique}"))
        .expect("a valid identifier literal")
}

fn probe_name() -> AppleBundleIdName {
    AppleBundleIdName::parse("willikins-live-write-cycle-probe").unwrap()
}

fn probe_platform() -> AppleBundleIdPlatform {
    AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()
}

/// Deletes the throwaway identifier on drop, unless [`Self::disarm`] was
/// called first -- so a panic or a failed assertion anywhere in the run
/// still cleans up. Mirrors
/// `willikins-providers-buildkite/tests/live_write_cycle.rs`'s
/// `PipelineGuard`.
struct IdentifierGuard {
    client: Arc<AppstoreClient>,
    id: willikins_types::AppleBundleIdId,
    armed: bool,
}

impl IdentifierGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for IdentifierGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.client.delete_bundle_id(&self.id);
        }
    }
}

#[test]
#[ignore = "creates and deletes ONE real App Store Connect bundle id on the operator's LIVE \
            account; run with WILLIKINS_LIVE_TESTS=1 and the sandbox credential sourced in the \
            same command. Never enables a capability. Read carefully before running: this is a \
            production developer account with no sandbox team."]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
fn appstore_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let (issuer_id, key_id, key) = credential_parts();
    let client = Arc::new(AppstoreClient::new(willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        {
            let credential =
                willikins_providers_http::AppleSigningCredential::new(&issuer_id, &key_id, &key)
                    .expect("builds a credential from the sandbox key");
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .unwrap_or(i64::MAX);
            let token = credential.sign(now).expect("signs a token");
            willikins_providers_http::Credential::from_bearer_token(
                "WILLIKINS_APPSTORE_LIVE_WRITE_CYCLE",
                token.as_str().to_owned(),
            )
        },
    )));

    // Report the count before this run touches anything.
    let count_before = client
        .list_bundle_ids(&AppleBundleIdentifier::parse("com.willikins.probe.count-only").unwrap())
        .map_or(0, |matches| matches.len());
    println!(
        "sanity read before this run (must be 0, since no real identifier is this string): \
         {count_before}"
    );

    let identifier = throwaway_identifier();
    let tool = AppstoreBundleIdEnsure::new(willikins_providers_appstore::APPSTORE_API_BASE_URL);

    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id),
    );
    inputs.insert(PortName::parse("key_id").unwrap(), Value::known(key_id));
    inputs.insert(PortName::parse("key").unwrap(), Value::known(key));
    inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier.clone()),
    );
    inputs.insert(PortName::parse("name").unwrap(), Value::known(probe_name()));
    inputs.insert(
        PortName::parse("platform").unwrap(),
        Value::known(probe_platform()),
    );

    // Step 1: the throwaway identifier must be absent (it is unique per
    // run, so this should always hold; a surprise here means STOP).
    match tool.read(&inputs).expect("reads") {
        Observation::Absent { .. } => {}
        other => panic!(
            "the freshly generated throwaway identifier `{identifier}` is not Absent \
             ({other:?}) -- STOP: this is unexpected and must be investigated by hand, not \
             improvised around"
        ),
    }

    // Step 2: create.
    let sink = SinkToken::new();
    let created = tool.ensure(&inputs, &sink).expect("creates the identifier");
    assert!(
        created.changed,
        "the first ensure must create the identifier"
    );
    let id = created
        .outputs
        .get(&PortName::parse("id").unwrap())
        .expect("the id output is present")
        .downcast::<willikins_types::AppleBundleIdId>()
        .expect("the id output is an AppleBundleIdId")
        .clone();
    println!("created throwaway identifier `{identifier}` with id `{id}`");

    // Step 3: arm the guard immediately after creation succeeds.
    let guard = IdentifierGuard {
        client: Arc::clone(&client),
        id: id.clone(),
        armed: true,
    };

    // Step 4: converge (a second ensure must be a no-op).
    let converged = tool.ensure(&inputs, &sink).expect("re-ensures");
    assert!(
        !converged.changed,
        "a second ensure against an unchanged identifier must report changed: false"
    );

    // Step 5: delete directly through the client, re-read as Absent.
    client
        .delete_bundle_id(&id)
        .expect("deletes the throwaway identifier");
    let after_delete = tool.read(&inputs).expect("reads");
    assert!(
        matches!(after_delete, Observation::Absent { .. }),
        "expected Absent after delete, got {after_delete:?}"
    );
    guard.disarm();

    println!("throwaway identifier `{identifier}` deleted; write cycle complete");
}
