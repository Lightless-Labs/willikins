//! The read-only live probe against the operator's real App Store
//! Connect account. `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_PROBE=1` -- the credential parts are read only past
//! that gate, mirroring `willikins-providers-buildkite/tests/live_probe.rs`.
//!
//! **This is the operator's live developer account. There is no sandbox
//! team.** Every call this file makes is a `GET`. Nothing is created,
//! modified, or deleted, and no capability is touched.
//!
//! It answers two questions:
//!
//! 1. **How many bundle identifiers does the account hold?** Reported as
//!    a count (and a per-platform tally), never as identifiers or names.
//!    The count is read by paginating `GET /v1/bundleIds?limit=200` and
//!    counting rows, cross-checked against `meta.paging.total` when
//!    Apple sends one. A missing total is *reported*, never silently
//!    turned into zero -- a false "the account is empty" is the one
//!    answer this probe must never give, since the whole point of the
//!    count is to prove afterwards that nothing was left behind.
//! 2. **What does `filter[identifier]` actually match -- exactly, by
//!    prefix, or by substring?** This is the single load-bearing
//!    unknown in `docs/research/2026-09-16-app-store-connect.md`'s
//!    "Verify with a browser before relying on them" list (section 2:
//!    "if it matched by prefix, a read for `com.acme.app` would also
//!    return `com.acme.app.extension`"). The probe takes one identifier
//!    the account already has, filters for a strict *prefix* of it and
//!    for a strict *suffix* of it, and reports only whether the original
//!    row came back -- never the identifier itself:
//!
//!    | prefix hit | suffix hit | semantics   |
//!    |------------|------------|-------------|
//!    | no         | no         | exact       |
//!    | yes        | no         | prefix      |
//!    | yes        | yes        | substring   |
//!
//!    Whatever the answer, `appstore.bundle_id.ensure` is already
//!    correct: it compares every returned row byte-for-byte itself and
//!    never trusts the filter (`tests/bundle_id_ensure_mock.rs`'s
//!    `read_reports_absent_when_only_a_prefix_neighbor_matches_the_filter`).
//!    The probe settles the research note's unresolved item; it does not
//!    change what the tool does.
//!
//! # Where the credential comes from
//!
//! The three parts live in the operator's **sandbox Doppler workplace**,
//! project `app-store-connect`, config `prd`, under the three names
//! below -- *not* in `~/.config/willikins/sandbox.env`, which holds only
//! the Doppler token that unlocks them. Resolve them into this process's
//! environment in the same command that runs the probe, so no value is
//! ever written to a file, a log, or a command line:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env \
//!   && J=$(curl -sf -H "Authorization: Bearer $WILLIKINS_DOPPLER_TOKEN" \
//!        "https://api.doppler.com/v3/configs/config/secrets/download?project=app-store-connect&config=prd&format=json") \
//!   && export ASC_API_KEY_ISSUER_ID=$(jq -r .ASC_API_KEY_ISSUER_ID <<<"$J") \
//!             ASC_API_KEY_ID=$(jq -r .ASC_API_KEY_ID <<<"$J") \
//!             ASC_API_KEY_BASE64=$(jq -r .ASC_API_KEY_BASE64 <<<"$J") \
//!   && unset J \
//!   && WILLIKINS_LIVE_PROBE=1 cargo test -p willikins-providers-appstore \
//!        --test live_probe -j 2 -- --ignored --nocapture
//! ```
//!
//! No `gh` command is run. Nothing here prints a credential, a JWT, or
//! any bundle id's identifier or name.

use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType};

/// Read the three credential parts from the environment, base64-decoding
/// the key (the operator's own storage habit) and parsing all three into
/// their domain types.
///
/// Every failure path here drops the underlying error rather than
/// printing it. `String::from_utf8`'s own `FromUtf8Error` carries the
/// *bytes it rejected* in its `Debug` output, so an `.expect()` on it
/// would print the decoded private key into the test log on exactly the
/// run where something went wrong -- the opposite of what this file is
/// for.
fn credential_parts() -> (AppleIssuerId, AppleKeyId, AppleSigningKey) {
    let issuer_id = std::env::var("ASC_API_KEY_ISSUER_ID").expect("ASC_API_KEY_ISSUER_ID is set");
    let key_id = std::env::var("ASC_API_KEY_ID").expect("ASC_API_KEY_ID is set");
    let key_base64 = std::env::var("ASC_API_KEY_BASE64").expect("ASC_API_KEY_BASE64 is set");
    let key_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        key_base64.trim(),
    )
    .unwrap_or_else(|_| panic!("ASC_API_KEY_BASE64 does not decode as base64"));
    let key_pem = String::from_utf8(key_bytes)
        .unwrap_or_else(|_| panic!("the decoded ASC_API_KEY_BASE64 is not valid UTF-8"));
    (
        AppleIssuerId::parse(&issuer_id).expect("ASC_API_KEY_ISSUER_ID is a valid issuer id"),
        AppleKeyId::parse(&key_id).expect("ASC_API_KEY_ID is a valid key id"),
        AppleSigningKey::parse(&key_pem)
            .unwrap_or_else(|_| panic!("ASC_API_KEY_BASE64 does not decode to a valid P-256 key")),
    )
}

/// Build an `Http` bound to Apple's real API, carrying one freshly
/// minted ES256 JWT.
fn live_http() -> willikins_providers_http::Http {
    let (issuer_id, key_id, key) = credential_parts();
    let credential =
        willikins_providers_http::AppleSigningCredential::new(&issuer_id, &key_id, &key)
            .expect("builds a credential from the sandbox key");
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(i64::MAX);
    let token = credential.sign(now).expect("signs a token");
    let bearer = willikins_providers_http::Credential::from_bearer_token(
        "WILLIKINS_APPSTORE_LIVE_PROBE",
        token.as_str().to_owned(),
    );
    willikins_providers_http::Http::new(
        willikins_providers_appstore::APPSTORE_API_BASE_URL,
        Vec::new(),
        bearer,
    )
}

/// Every bundle identifier the account holds, as raw strings.
///
/// **The returned strings are the operator's real identifiers and are
/// never printed by this file** -- only counts and booleans derived from
/// them are. Follows `links.next` so an account with more than one page
/// is counted correctly, with a hard cap so a paging bug cannot spin.
fn all_identifiers(http: &willikins_providers_http::Http) -> (Vec<(String, String)>, Option<u64>) {
    let mut path = "/v1/bundleIds?limit=200".to_string();
    let mut rows = Vec::new();
    let mut reported_total = None;
    for _ in 0..50 {
        let page: serde_json::Value = http
            .get(&path)
            .unwrap_or_else(|err| panic!("GET {path} failed: {err}"));
        if reported_total.is_none() {
            reported_total = page
                .pointer("/meta/paging/total")
                .and_then(serde_json::Value::as_u64);
        }
        for row in page
            .get("data")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let identifier = row
                .pointer("/attributes/identifier")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let platform = row
                .pointer("/attributes/platform")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<none>")
                .to_string();
            rows.push((identifier, platform));
        }
        let Some(next) = page
            .pointer("/links/next")
            .and_then(serde_json::Value::as_str)
        else {
            return (rows, reported_total);
        };
        // `links.next` is an absolute URL; `Http` takes a path.
        path = next
            .strip_prefix(willikins_providers_appstore::APPSTORE_API_BASE_URL)
            .unwrap_or_else(|| panic!("links.next is not on Apple's own host"))
            .to_string();
    }
    panic!("more than 50 pages of bundle ids -- refusing to keep paging");
}

/// How many rows a `filter[identifier]=<needle>` call returns, and
/// whether `original` is among them. Neither the needle nor the returned
/// identifiers are printed.
fn filter_probe(
    http: &willikins_providers_http::Http,
    needle: &str,
    original: &str,
) -> (usize, bool) {
    let page: serde_json::Value = http
        .get(&format!("/v1/bundleIds?filter[identifier]={needle}"))
        .unwrap_or_else(|err| panic!("a filtered GET failed: {err}"));
    let rows = page
        .get("data")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let hit = rows.iter().any(|row| {
        row.pointer("/attributes/identifier")
            .and_then(serde_json::Value::as_str)
            == Some(original)
    });
    (rows.len(), hit)
}

#[test]
#[ignore = "opt-in read-only probe against the operator's LIVE App Store Connect account; run \
            with WILLIKINS_LIVE_PROBE=1 and the credential resolved out of Doppler in the same \
            command. Makes GET calls only; never creates, modifies, or deletes anything."]
fn appstore_live_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }

    let http = live_http();

    // (a) The count. Identifiers and names never leave this function.
    let (rows, reported_total) = all_identifiers(&http);
    println!("PROBE(a) bundle id count (rows counted): {}", rows.len());
    match reported_total {
        Some(total) => {
            println!("PROBE(a) meta.paging.total: {total}");
            assert_eq!(
                u64::try_from(rows.len()).unwrap(),
                total,
                "counted rows disagree with Apple's own meta.paging.total"
            );
        }
        // Reported, never silently treated as zero.
        None => println!(
            "PROBE(a) meta.paging.total: ABSENT (Apple sent no total; the counted row figure \
             above is the answer)"
        ),
    }
    let mut platforms: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, platform) in &rows {
        *platforms.entry(platform.as_str()).or_default() += 1;
    }
    println!("PROBE(a) by platform: {platforms:?}");
    println!("PROBE(a) the JWT was accepted: every GET above returned 2xx");

    // (b) What does `filter[identifier]` actually match?
    let Some((original, _)) = rows.iter().find(|(identifier, _)| identifier.len() >= 3) else {
        println!(
            "PROBE(b) SKIPPED: the account holds no identifier long enough to take a strict \
             prefix and suffix of"
        );
        return;
    };
    let prefix = &original[..original.len() - 1];
    let suffix = &original[1..];

    let (exact_rows, exact_hit) = filter_probe(&http, original, original);
    println!(
        "PROBE(b) control, filtering for the whole string: {exact_rows} rows, original present: {exact_hit}"
    );
    assert!(
        exact_hit,
        "filtering for an identifier the account demonstrably holds did not return it -- STOP"
    );

    let (prefix_rows, prefix_hit) = filter_probe(&http, prefix, original);
    println!(
        "PROBE(b) strict prefix (last character dropped): {prefix_rows} rows, original present: \
         {prefix_hit}"
    );
    let (suffix_rows, suffix_hit) = filter_probe(&http, suffix, original);
    println!(
        "PROBE(b) strict suffix (first character dropped): {suffix_rows} rows, original present: \
         {suffix_hit}"
    );

    let verdict = match (prefix_hit, suffix_hit) {
        (false, false) => "EXACT",
        (true, false) => "PREFIX",
        (true, true) => "SUBSTRING",
        // A suffix hit with no prefix hit would be a suffix match, which
        // no one has ever reported; surface it rather than mislabel it.
        (false, true) => "SUFFIX (unexpected -- report, do not rely on this)",
    };
    println!("PROBE(b) VERDICT: filter[identifier] matches {verdict}");
}
