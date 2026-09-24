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
