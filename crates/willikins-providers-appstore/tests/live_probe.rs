//! The read-only live probe against the operator's real App Store
//! Connect account. `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_PROBE=1` -- the credential parts are read only past
//! that gate, mirroring `willikins-providers-buildkite/tests/live_probe.rs`.
//!
//! **This is the operator's live developer account. There is no sandbox
//! team.** This probe makes exactly one call,
//! `GET /v1/bundleIds?filter[identifier]=willikins-probe-does-not-exist.<marker>`
//! -- a read that cannot match anything real (the identifier string is
//! deliberately not a valid reverse-DNS bundle id shape at all) -- and
//! prints the **count** of existing bundle ids the account holds
//! (`GET /v1/bundleIds` with no filter, page 1), never their identifiers
//! or names. Nothing is created, modified, or deleted.
//!
//! The three credential parts are read from the same three environment
//! variable names the operator's own Doppler chain uses
//! (`ASC_API_KEY_ISSUER_ID`, `ASC_API_KEY_ID`, `ASC_API_KEY_BASE64` --
//! the private key, base64-wrapped, per the operator's own habit
//! documented in `docs/plans/2026-09-11-willikins-design.md`'s
//! 2026-09-21 addendum), sourced from `~/.config/willikins/sandbox.env`
//! in the same command:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_PROBE=1 \
//!   cargo test -p willikins-providers-appstore --test live_probe -- --ignored \
//!   --nocapture
//! ```
//!
//! No `gh` command is run. Nothing here prints a credential, a JWT, or
//! any bundle id's identifier or name.

use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType};

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

#[test]
#[ignore = "opt-in read-only probe against the operator's LIVE App Store Connect account; run \
            with WILLIKINS_LIVE_PROBE=1 and the sandbox credential sourced in the same command. \
            Makes GET calls only; never creates, modifies, or deletes anything."]
fn appstore_live_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }

    let (issuer_id, key_id, key) = credential_parts();
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
    let bearer = willikins_providers_http::Credential::from_bearer_token(
        "WILLIKINS_APPSTORE_LIVE_PROBE",
        token.as_str().to_owned(),
    );
    let http = willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer,
    );

    // Report the count of existing bundle ids, never their identifiers
    // or names -- "report the identifier count before writing anything".
    let count_before: usize = http
        .get::<serde_json::Value>("/v1/bundleIds?page[limit]=1")
        .expect("lists bundle ids")
        .get("meta")
        .and_then(|meta| meta.get("paging"))
        .and_then(|paging| paging.get("total"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        // Apple's `meta.paging.total` is documented but not proved live
        // yet -- fall back to 0 rather than failing the probe outright.
        .unwrap_or(0);
    println!("account bundle id count (best-effort, see meta.paging.total): {count_before}");

    // One read that cannot match anything real.
    let marker = format!("willikins-probe-does-not-exist-{}", std::process::id());
    let response = http
        .get::<serde_json::Value>(&format!("/v1/bundleIds?filter[identifier]={marker}"))
        .expect("the filtered list call succeeds even with zero matches");
    let matches = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    assert_eq!(matches, 0, "a made-up marker identifier must match nothing");
    println!("filtered probe for a made-up identifier: 0 matches (pass)");
}
