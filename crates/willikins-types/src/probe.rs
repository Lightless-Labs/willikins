//! Proof that `#[derive(DomainType)]` works *inside* `willikins-types`.
//!
//! The derive's generated code names this crate absolutely, as
//! `::willikins_types::...`. That path does not resolve inside the crate
//! itself unless the crate aliases itself, which `lib.rs` does with
//! `extern crate self as willikins_types;`. Later tasks define this
//! crate's own domain types with the derive, so the alias is load-bearing;
//! this module is the test that keeps it that way. It is never exported.

use crate::{DomainType, SinkToken};

#[derive(DomainType)]
#[domain(
    pattern = "[a-z]+",
    max_len = 8,
    description = "A probe type derived inside willikins-types itself",
    example = "probe"
)]
struct Probe(String);

#[derive(DomainType)]
#[domain(
    min_len = 4,
    secret,
    description = "A secret probe type derived inside willikins-types itself",
    example = "probe-secret"
)]
struct ProbeSecret(secrecy::SecretString);

/// Milestone 3i, task B1: a `String`-storage identifier type, proving the
/// derive's `identifier` codegen path actually compiles and masks (not
/// merely `mask_identifier` the free function, which `disclosure.rs`'s own
/// unit tests already cover). No production type is marked
/// `#[domain(identifier)]` yet (that is task B2); this probe exists only
/// so a future regression in the derive's expansion surfaces here, not one
/// task later when B2 marks the first real type.
#[derive(DomainType)]
#[domain(
    identifier,
    description = "A probe identifier type (String storage) derived inside willikins-types itself",
    example = "57246542-96fe-1a63-e053-0824d011072a"
)]
struct ProbeIdentifier(String);

/// The same proof for `Other` storage (anything used through `FromStr` +
/// `Display`), whose `Debug` codegen path is different from `String`
/// storage's (it masks the `Display` form, not the raw field).
#[derive(DomainType)]
#[domain(
    identifier,
    description = "A probe identifier type (Other storage) derived inside willikins-types itself",
    example = "123456789"
)]
struct ProbeIdentifierOther(u64);

/// Proves the public-to-secret half of "secrecy only goes up": a
/// conversion whose target is *more* secret than its source is admitted
/// and compiles. Registers nothing in production; see
/// `docs/plans/2026-09-23-milestone-3d-conversions.md`, decision (b), for
/// why no such row exists in `conversion_rows`.
impl From<Probe> for ProbeSecret {
    fn from(probe: Probe) -> Self {
        Self::parse(probe.as_str()).unwrap_or_else(|err| {
            unreachable!("a Probe's own string is always a ProbeSecret: {err}")
        })
    }
}

#[test]
fn a_conversion_may_raise_secrecy() {
    let row = crate::conversions![Probe => ProbeSecret]
        .into_iter()
        .next()
        .expect("one row");
    let probe = Probe::parse("probe").unwrap();
    let converted = row.apply(&probe).expect("a Probe converts");
    assert!(converted.is_secret());
    assert_eq!(converted.render().to_string(), "[REDACTED ProbeSecret]");
}

/// A row applied to an object that is not of its source type returns
/// `None`, never panics: the test is the downcast, by `TypeId`.
#[test]
fn a_conversion_applied_to_another_type_is_none() {
    let row = crate::conversions![Probe => ProbeSecret]
        .into_iter()
        .next()
        .expect("one row");
    let other = crate::GitHubOrg::parse("lightless-labs").unwrap();
    assert!(row.apply(&other).is_none());
}

#[test]
fn the_derive_expands_inside_this_crate() {
    let value = Probe::parse("probe").unwrap();
    assert_eq!(value.as_str(), "probe");
    assert_eq!(Probe::TYPE_NAME, "Probe");
    assert!(Probe::parse("PROBE").is_err());
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn the_derive_expands_for_a_secret_inside_this_crate() {
    let value = ProbeSecret::parse("probe-secret").unwrap();
    assert_eq!(format!("{value:?}"), "[REDACTED ProbeSecret]");
    assert_eq!(value.expose(&SinkToken::new()), "probe-secret");
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn the_derive_emits_domain_object_inside_this_crate() {
    use crate::DomainObject;

    let plain: Box<dyn DomainObject> = Box::new(Probe::parse("probe").unwrap());
    assert!(!plain.is_secret());
    assert_eq!(plain.expose(&SinkToken::new()), "probe");

    let secret: Box<dyn DomainObject> = Box::new(ProbeSecret::parse("probe-secret").unwrap());
    assert!(secret.is_secret());
    assert_eq!(secret.render().to_string(), "[REDACTED ProbeSecret]");
    assert_eq!(secret.expose(&SinkToken::new()), "probe-secret");
}

#[test]
fn a_non_identifier_derived_type_reports_is_identifier_false() {
    use crate::DomainObject;

    let value = Probe::parse("probe").unwrap();
    const { assert!(!<Probe as DomainType>::IS_IDENTIFIER) };
    assert!(!DomainObject::is_identifier(&value));
    // `Debug` is unchanged by this milestone for a non-identifier type.
    assert_eq!(format!("{value:?}"), "Probe(\"probe\")");
}

#[test]
fn the_derive_marks_a_string_storage_identifier_type() {
    use crate::DomainObject;

    let full = "57246542-96fe-1a63-e053-0824d011072a";
    let value = ProbeIdentifier::parse(full).unwrap();

    const { assert!(<ProbeIdentifier as DomainType>::IS_IDENTIFIER) };
    const { assert!(!<ProbeIdentifier as DomainType>::IS_SECRET) };
    assert!(DomainObject::is_identifier(&value));
    assert!(!DomainObject::is_secret(&value));

    // `Debug` masks; `Display`, `render()` and `Serialize` stay full
    // (trust boundary 6: masking touches only output surfaces).
    assert_eq!(format!("{value:?}"), "ProbeIdentifier(\"5724...\")");
    assert_eq!(value.to_string(), full);
    assert_eq!(value.render().to_string(), full);
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        format!("\"{full}\"")
    );
}

#[test]
fn the_derive_marks_an_other_storage_identifier_type() {
    use crate::DomainObject;

    let value = ProbeIdentifierOther::parse("123456789").unwrap();

    const { assert!(<ProbeIdentifierOther as DomainType>::IS_IDENTIFIER) };
    assert!(DomainObject::is_identifier(&value));

    assert_eq!(format!("{value:?}"), "ProbeIdentifierOther(\"1234...\")");
    assert_eq!(value.to_string(), "123456789");
    assert_eq!(value.render().to_string(), "123456789");
}
