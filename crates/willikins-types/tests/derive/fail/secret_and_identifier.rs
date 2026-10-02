// Milestone 3i, decision (b1): `#[domain(secret, identifier)]` together is
// not allowed. A secret type is already redacted everywhere; identifier
// masking is a strictly weaker guarantee.

#[derive(willikins_derive::DomainType)]
#[domain(secret, identifier, description = "d", example = "e")]
struct Foo(secrecy::SecretString);

fn main() {}
