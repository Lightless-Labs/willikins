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

#[test]
fn the_derive_expands_inside_this_crate() {
    let value = Probe::parse("probe").unwrap();
    assert_eq!(value.as_str(), "probe");
    assert_eq!(Probe::TYPE_NAME, "Probe");
    assert!(Probe::parse("PROBE").is_err());
}

#[test]
fn the_derive_expands_for_a_secret_inside_this_crate() {
    let value = ProbeSecret::parse("probe-secret").unwrap();
    assert_eq!(format!("{value:?}"), "[REDACTED ProbeSecret]");
    assert_eq!(value.expose(&SinkToken::new()), "probe-secret");
}

#[test]
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
