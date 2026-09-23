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
//!
//! # The signing pre-flight (`appstore_signing_probe`), milestone 3c
//!
//! A second `#[test]` in this file, under the same gate, answers the
//! three questions `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`
//! gates on, **as counts and statuses only**:
//!
//! - **(c)** does the credential still authenticate at all
//!   (`GET /v1/bundleIds?limit=1`, status only);
//! - **(d)** certificates, counted by `certificateType`, by `activated`
//!   and by whether `expirationDate` is past, and whether at least one
//!   certificate of a distribution type is unexpired and not
//!   deactivated;
//! - **(e)** whether `GET /v1/profiles` answers `200` or `403` for this
//!   key, and the profiles counted by `profileType` and `profileState`,
//!   with the lengths of their `profileContent` and the sizes of their
//!   device and certificate relationships.
//!
//! **Certificates and profiles belong to the operator.** A certificate's
//! display name, name, serial number or id, and a profile's name, uuid,
//! id or content, never leave this function: the certificate read does
//! not request `certificateContent`; the names it requests are reduced
//! to a *count of distinct values* and to whether they begin with one of
//! Apple's own type labels (`Apple Distribution`, `iOS Distribution`),
//! which are Apple's words, not the operator's; and the serial numbers
//! are reduced to their shape (character class and length). One usable
//! certificate's serial is held in memory to learn what
//! `filter[serialNumber]` matches (whole string, strict prefix, strict
//! suffix -- the method that settled `filter[identifier]`), and only row
//! counts and hit booleans are printed. Every call is a `GET`.
//!
//! A `200` on either list proves only that this key may *read*. It says
//! nothing about permission to *create* a profile; the milestone's live
//! write cycle is the first thing that will know.

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

// ---------------------------------------------------------------------
// The signing pre-flight, milestone 3c. Read-only; counts and statuses.
// ---------------------------------------------------------------------

/// The certificate types Apple's own certificates-overview table says
/// can "submit [an app] to App Store Connect" ("Apple Distribution",
/// "iOS Distribution", "Mac App Distribution"), as the API's
/// `CertificateType` spells the ones this milestone could use. The
/// label-to-enum mapping is itself unverified; the probe reports the
/// label prefixes it sees per type so a reader can check it.
const DISTRIBUTION_FAMILY: [&str; 3] = ["DISTRIBUTION", "IOS_DISTRIBUTION", "MAC_APP_DISTRIBUTION"];

/// A `GET` that answers its status instead of panicking: a `403` on a
/// list is an answer this probe must report, not a crash.
fn get_or_status(
    http: &willikins_providers_http::Http,
    path: &str,
) -> Result<serde_json::Value, Option<u16>> {
    http.get::<serde_json::Value>(path)
        .map_err(|err| err.status)
}

/// Every row of a collection, following `links.next` exactly as
/// [`all_identifiers`] does, plus every `included` resource, plus
/// Apple's `meta.paging.total` when it sends one. Refuses past 50 pages.
struct Collection {
    rows: Vec<serde_json::Value>,
    included: Vec<serde_json::Value>,
    reported_total: Option<u64>,
}

fn collect(http: &willikins_providers_http::Http, first: &str) -> Result<Collection, Option<u16>> {
    let mut path = first.to_string();
    let mut out = Collection {
        rows: Vec::new(),
        included: Vec::new(),
        reported_total: None,
    };
    for _ in 0..50 {
        let page = get_or_status(http, &path)?;
        if out.reported_total.is_none() {
            out.reported_total = page
                .pointer("/meta/paging/total")
                .and_then(serde_json::Value::as_u64);
        }
        for (key, sink) in [("data", &mut out.rows), ("included", &mut out.included)] {
            if let Some(items) = page.get(key).and_then(serde_json::Value::as_array) {
                sink.extend(items.iter().cloned());
            }
        }
        let Some(next) = page
            .pointer("/links/next")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(out);
        };
        path = next
            .strip_prefix(willikins_providers_appstore::APPSTORE_API_BASE_URL)
            .unwrap_or_else(|| panic!("links.next is not on Apple's own host"))
            .to_string();
    }
    panic!("more than 50 pages -- refusing to keep paging");
}

fn report_total(label: &str, collection: &Collection) {
    let counted = collection.rows.len();
    match collection.reported_total {
        Some(total) if u64::try_from(counted).ok() == Some(total) => {
            println!("{label} rows counted: {counted}; meta.paging.total agrees: {total}");
        }
        Some(total) => println!(
            "{label} rows counted: {counted}; meta.paging.total DISAGREES: {total} -- STOP and report"
        ),
        None => println!("{label} rows counted: {counted}; meta.paging.total: ABSENT"),
    }
}

/// Now, in UTC, as `YYYY-MM-DDTHH:MM:SS` -- the civil-from-days
/// conversion, so the probe needs no date crate.
fn utc_now_prefix() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// Whether an Apple `date-time` is in the past: `Some(true)` expired,
/// `Some(false)` not, `None` when it is absent or carries an offset this
/// probe does not compare (only `Z` and `+00:00` are compared, on their
/// first 19 characters).
fn expired(date: Option<&str>, now: &str) -> Option<bool> {
    let date = date?;
    let utc = date.ends_with('Z') || date.ends_with("+00:00");
    if !utc || date.len() < 19 || !date.is_char_boundary(19) {
        return None;
    }
    Some(&date[..19] <= now)
}

fn attr<'a>(row: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    row.get("attributes").and_then(|a| a.get(name))
}

fn attr_str<'a>(row: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    attr(row, name).and_then(serde_json::Value::as_str)
}

#[derive(Default, Debug)]
struct CertTally {
    total: usize,
    expired: usize,
    unexpired: usize,
    expiry_unparsed: usize,
    activated_true: usize,
    activated_false: usize,
    activated_absent: usize,
    usable: usize,
    label_apple_distribution: usize,
    label_ios_distribution: usize,
    label_other: usize,
    serial_upper_hex: usize,
    serial_lower_hex: usize,
    serial_other_shape: usize,
    serial_min_len: Option<usize>,
    serial_max_len: usize,
}

#[derive(Default, Debug)]
struct ProfileTally {
    total: usize,
    expired_by_date: usize,
    unexpired_by_date: usize,
    expiry_unparsed: usize,
    content_min_chars: Option<usize>,
    content_max_chars: usize,
    content_absent: usize,
    with_devices: usize,
    without_devices: usize,
    devices_max_seen: usize,
    certificates_per_profile: std::collections::BTreeMap<usize, usize>,
    signing_certificate_types: std::collections::BTreeMap<String, usize>,
}

#[test]
#[ignore = "opt-in read-only signing pre-flight against the operator's LIVE App Store Connect \
            account (milestone 3c); run with WILLIKINS_LIVE_PROBE=1 and the credential resolved \
            out of Doppler in the same command. GET only; prints counts and statuses only."]
#[allow(clippy::too_many_lines)] // one linear read-only report, kept in one place on purpose
fn appstore_signing_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }
    let http = live_http();
    let now = utc_now_prefix();

    // (c) Does the credential authenticate at all?
    match get_or_status(&http, "/v1/bundleIds?limit=1&fields[bundleIds]=platform") {
        Ok(_) => println!("PROBE(c) credential authenticates: GET /v1/bundleIds answered 200"),
        Err(status) => {
            println!(
                "PROBE(c) credential FAILED: GET /v1/bundleIds answered {status:?} -- STOP; \
                 GATE BLOCKED"
            );
            return;
        }
    }

    // (d) Certificates. Never requests serialNumber or certificateContent.
    match get_or_status(
        &http,
        "/v1/certificates?limit=1&fields[certificates]=certificateType",
    ) {
        Ok(_) => println!("PROBE(d) GET /v1/certificates status: 200"),
        Err(status) => println!("PROBE(d) GET /v1/certificates status: {status:?}"),
    }
    let mut any_usable = false;
    // One usable certificate's serial, held in memory only, to learn what
    // `filter[serialNumber]` matches. Never printed.
    let mut sample_serial: Option<String> = None;
    match collect(
        &http,
        "/v1/certificates?limit=200&fields[certificates]=certificateType,displayName,name,\
         expirationDate,activated,serialNumber",
    ) {
        Err(status) => println!("PROBE(d) certificate listing FAILED with status {status:?}"),
        Ok(certs) => {
            report_total("PROBE(d) certificates", &certs);
            let mut by_type: std::collections::BTreeMap<String, CertTally> =
                std::collections::BTreeMap::new();
            let mut usable_display_names: std::collections::BTreeMap<
                String,
                std::collections::BTreeSet<&str>,
            > = std::collections::BTreeMap::new();
            for row in &certs.rows {
                let kind = attr_str(row, "certificateType")
                    .unwrap_or("<none>")
                    .to_string();
                let tally = by_type.entry(kind.clone()).or_default();
                tally.total += 1;
                let is_expired = expired(attr_str(row, "expirationDate"), &now);
                match is_expired {
                    Some(true) => tally.expired += 1,
                    Some(false) => tally.unexpired += 1,
                    None => tally.expiry_unparsed += 1,
                }
                let activated = attr(row, "activated").and_then(serde_json::Value::as_bool);
                match activated {
                    Some(true) => tally.activated_true += 1,
                    Some(false) => tally.activated_false += 1,
                    None => tally.activated_absent += 1,
                }
                // Apple's own type label, never the operator's part of
                // the name: only a prefix test, reduced to a count.
                let name = attr_str(row, "name").unwrap_or_default();
                if name.starts_with("Apple Distribution") {
                    tally.label_apple_distribution += 1;
                } else if name.starts_with("iOS Distribution") {
                    tally.label_ios_distribution += 1;
                } else {
                    tally.label_other += 1;
                }
                // The serial's *shape* only: its character class and
                // length, never its value.
                let serial = attr_str(row, "serialNumber").unwrap_or_default();
                let hex = !serial.is_empty() && serial.chars().all(|c| c.is_ascii_hexdigit());
                if hex && !serial.chars().any(|c| c.is_ascii_lowercase()) {
                    tally.serial_upper_hex += 1;
                } else if hex && !serial.chars().any(|c| c.is_ascii_uppercase()) {
                    tally.serial_lower_hex += 1;
                } else {
                    tally.serial_other_shape += 1;
                }
                tally.serial_max_len = tally.serial_max_len.max(serial.len());
                tally.serial_min_len = Some(
                    tally
                        .serial_min_len
                        .map_or(serial.len(), |m| m.min(serial.len())),
                );
                if DISTRIBUTION_FAMILY.contains(&kind.as_str())
                    && is_expired == Some(false)
                    && activated != Some(false)
                {
                    tally.usable += 1;
                    any_usable = true;
                    if sample_serial.is_none() && serial.len() >= 3 && serial.is_ascii() {
                        sample_serial = Some(serial.to_string());
                    }
                    usable_display_names
                        .entry(kind.clone())
                        .or_default()
                        .insert(attr_str(row, "displayName").unwrap_or_default());
                }
            }
            for (kind, tally) in &by_type {
                println!("PROBE(d) certificateType {kind}: {tally:?}");
            }
            for (kind, names) in &usable_display_names {
                println!(
                    "PROBE(d) usable {kind}: distinct displayName values among them: {}",
                    names.len()
                );
            }
        }
    }
    // What does `filter[serialNumber]` match? The same method that settled
    // `filter[identifier]`: the whole string, a strict prefix, a strict
    // suffix; only row counts and whether the original came back.
    if let Some(serial) = &sample_serial {
        let probe = |needle: &str| -> String {
            match get_or_status(
                &http,
                &format!(
                    "/v1/certificates?limit=200&fields[certificates]=serialNumber\
                     &filter[serialNumber]={needle}"
                ),
            ) {
                Err(status) => format!("status {status:?}"),
                Ok(page) => {
                    let rows = page
                        .get("data")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let hit = rows
                        .iter()
                        .any(|row| attr_str(row, "serialNumber") == Some(serial.as_str()));
                    format!("{} rows, original present: {hit}", rows.len())
                }
            }
        };
        println!(
            "PROBE(d) filter[serialNumber] whole string: {}",
            probe(serial)
        );
        println!(
            "PROBE(d) filter[serialNumber] strict prefix: {}",
            probe(&serial[..serial.len() - 1])
        );
        println!(
            "PROBE(d) filter[serialNumber] strict suffix: {}",
            probe(&serial[1..])
        );
    } else {
        println!("PROBE(d) filter[serialNumber] SKIPPED: no usable certificate to probe with");
    }
    println!(
        "PROBE(d) GATE: at least one unexpired, not-deactivated distribution-family \
         certificate exists: {any_usable}"
    );

    // (e) Profiles.
    let profiles_status =
        match get_or_status(&http, "/v1/profiles?limit=1&fields[profiles]=profileType") {
            Ok(_) => Some(200),
            Err(status) => status,
        };
    println!("PROBE(e) GET /v1/profiles status: {profiles_status:?}");
    if profiles_status != Some(200) {
        println!("PROBE(e) GATE BLOCKED: GET /v1/profiles did not answer 200");
        return;
    }
    match collect(
        &http,
        "/v1/profiles?limit=200&fields[profiles]=profileType,profileState,expirationDate,\
         profileContent,certificates,devices&include=certificates,devices\
         &fields[certificates]=certificateType&fields[devices]=platform\
         &limit[certificates]=50&limit[devices]=50",
    ) {
        Err(status) => println!("PROBE(e) profile listing FAILED with status {status:?}"),
        Ok(profiles) => {
            report_total("PROBE(e) profiles", &profiles);
            let certificate_types: std::collections::BTreeMap<&str, &str> = profiles
                .included
                .iter()
                .filter(|row| {
                    row.get("type").and_then(serde_json::Value::as_str) == Some("certificates")
                })
                .filter_map(|row| {
                    Some((
                        row.get("id").and_then(serde_json::Value::as_str)?,
                        attr_str(row, "certificateType").unwrap_or("<none>"),
                    ))
                })
                .collect();
            let mut by_kind: std::collections::BTreeMap<(String, String), ProfileTally> =
                std::collections::BTreeMap::new();
            for row in &profiles.rows {
                let key = (
                    attr_str(row, "profileType").unwrap_or("<none>").to_string(),
                    attr_str(row, "profileState")
                        .unwrap_or("<none>")
                        .to_string(),
                );
                let tally = by_kind.entry(key).or_default();
                tally.total += 1;
                match expired(attr_str(row, "expirationDate"), &now) {
                    Some(true) => tally.expired_by_date += 1,
                    Some(false) => tally.unexpired_by_date += 1,
                    None => tally.expiry_unparsed += 1,
                }
                match attr_str(row, "profileContent") {
                    Some(content) => {
                        let chars = content.chars().count();
                        tally.content_max_chars = tally.content_max_chars.max(chars);
                        tally.content_min_chars =
                            Some(tally.content_min_chars.map_or(chars, |m| m.min(chars)));
                    }
                    None => tally.content_absent += 1,
                }
                let related = |name: &str| -> Vec<&serde_json::Value> {
                    row.pointer(&format!("/relationships/{name}/data"))
                        .and_then(serde_json::Value::as_array)
                        .map(|items| items.iter().collect())
                        .unwrap_or_default()
                };
                let devices = related("devices");
                if devices.is_empty() {
                    tally.without_devices += 1;
                } else {
                    tally.with_devices += 1;
                }
                tally.devices_max_seen = tally.devices_max_seen.max(devices.len());
                let certificates = related("certificates");
                *tally
                    .certificates_per_profile
                    .entry(certificates.len())
                    .or_default() += 1;
                for certificate in certificates {
                    let kind = certificate
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|id| certificate_types.get(id).copied())
                        .unwrap_or("<not included>");
                    *tally
                        .signing_certificate_types
                        .entry(kind.to_string())
                        .or_default() += 1;
                }
            }
            for ((kind, state), tally) in &by_kind {
                println!("PROBE(e) profileType {kind} / profileState {state}: {tally:?}");
            }
        }
    }
    println!(
        "PROBE(e) NOTE: a 200 on a list proves read access only; permission to CREATE a profile \
         is unknown until the milestone's live write cycle"
    );
}

// ---------------------------------------------------------------------
// Counts and leftovers, milestone 3c task 3. Read-only; counts only.
// ---------------------------------------------------------------------

/// The throwaway prefixes `tests/live_write_cycle.rs` names everything it
/// creates with -- by design never one of the operator's own.
const THROWAWAY_IDENTIFIER_PREFIX: &str = "com.willikins.probe.delete-me.";
const THROWAWAY_PROFILE_NAME_PREFIX: &str = "willikins-probe-delete-me-";

/// How many rows' `expirationDate` parse as RFC 3339 -- the parse both
/// `appstore.certificate.get` and `appstore.profile.ensure` apply, which
/// refuses Apple's answer outright if Apple spells the offset any other
/// way (`+0000`, say, which every task-1 and task-2 fixture used).
fn report_dates(label: &str, rows: &[serde_json::Value]) {
    let (mut parses, mut refused, mut absent) = (0usize, 0usize, 0usize);
    for row in rows {
        match attr_str(row, "expirationDate") {
            Some(date) if chrono::DateTime::parse_from_rfc3339(date).is_ok() => parses += 1,
            Some(_) => refused += 1,
            None => absent += 1,
        }
    }
    println!(
        "GRAMMAR {label} expirationDate parses as RFC 3339: {parses}; refused: {refused}; \
         absent: {absent}"
    );
}

/// The three counts the live write cycle must leave unchanged
/// (certificates by type, profiles by type and state, bundle identifiers),
/// plus how many bundle identifiers and profiles carry a throwaway prefix.
/// Run once before the write cycle and once after it, each time as its
/// own process with its own freshly minted JWT, so the "after" read is
/// independent of the cycle's own client. `GET` only; no name,
/// identifier, serial, id or uuid is printed -- only counts.
#[test]
#[ignore = "opt-in read-only count of the operator's LIVE App Store Connect certificates, \
            profiles and bundle identifiers, plus throwaway leftovers; run with \
            WILLIKINS_LIVE_PROBE=1 and the credential resolved out of Doppler in the same \
            command. GET only; prints counts only."]
fn appstore_counts_and_leftovers_probe() {
    if std::env::var("WILLIKINS_LIVE_PROBE").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_PROBE is not 1");
        return;
    }
    let http = live_http();

    let certificates = collect(
        &http,
        "/v1/certificates?limit=200&fields[certificates]=certificateType,expirationDate",
    )
    .unwrap_or_else(|status| panic!("certificate listing failed with status {status:?}"));
    report_total("COUNTS certificates", &certificates);
    let mut certificates_by_type: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for row in &certificates.rows {
        *certificates_by_type
            .entry(attr_str(row, "certificateType").unwrap_or("<none>"))
            .or_default() += 1;
    }
    println!("COUNTS certificates by type: {certificates_by_type:?}");
    report_dates("certificates", &certificates.rows);

    let profiles = collect(
        &http,
        "/v1/profiles?limit=200&fields[profiles]=name,profileType,profileState,profileContent,\
         expirationDate",
    )
    .unwrap_or_else(|status| panic!("profile listing failed with status {status:?}"));
    report_total("COUNTS profiles", &profiles);
    let mut profiles_by_kind: std::collections::BTreeMap<(&str, &str), usize> =
        std::collections::BTreeMap::new();
    let mut throwaway_profiles = 0usize;
    // Whether every real profile's content parses as the secret type
    // `appstore.profile.ensure` deserializes it into: a content Apple
    // spells outside `AppleProfileContent`'s grammar would make a create's
    // own `201` unparseable, and the created profile's id unrecoverable.
    let (mut content_parses, mut content_refused, mut content_absent) = (0usize, 0usize, 0usize);
    for row in &profiles.rows {
        match attr_str(row, "profileContent") {
            Some(content) if willikins_types::AppleProfileContent::parse(content).is_ok() => {
                content_parses += 1;
            }
            Some(_) => content_refused += 1,
            None => content_absent += 1,
        }
        *profiles_by_kind
            .entry((
                attr_str(row, "profileType").unwrap_or("<none>"),
                attr_str(row, "profileState").unwrap_or("<none>"),
            ))
            .or_default() += 1;
        if attr_str(row, "name").is_some_and(|name| name.starts_with(THROWAWAY_PROFILE_NAME_PREFIX))
        {
            throwaway_profiles += 1;
        }
    }
    println!("COUNTS profiles by (type, state): {profiles_by_kind:?}");
    report_dates("profiles", &profiles.rows);
    println!(
        "GRAMMAR profileContent parses as AppleProfileContent: {content_parses}; refused: \
         {content_refused}; absent: {content_absent}"
    );

    let (identifiers, reported_total) = all_identifiers(&http);
    println!(
        "COUNTS bundle ids rows counted: {}; meta.paging.total: {reported_total:?}",
        identifiers.len()
    );
    let throwaway_identifiers = identifiers
        .iter()
        .filter(|(identifier, _)| identifier.starts_with(THROWAWAY_IDENTIFIER_PREFIX))
        .count();

    println!(
        "LEFTOVERS bundle ids starting {THROWAWAY_IDENTIFIER_PREFIX}: {throwaway_identifiers}"
    );
    println!(
        "LEFTOVERS profiles named starting {THROWAWAY_PROFILE_NAME_PREFIX}: {throwaway_profiles}"
    );
}
