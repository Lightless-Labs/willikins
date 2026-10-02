//! `conversions![Identified => Plain]` must fail to compile when
//! `Identified` is an identifier type (`#[domain(identifier)]`) and `Plain`
//! is neither an identifier nor a secret: milestone 3i decision (b9),
//! "disclosure is monotone". This stands in for the production shape the
//! plan names, `AppleIssuerId => Text` — both derived, non-secret, with
//! `Text` carrying no `identifier` attribute — without reaching across the
//! crate boundary for a `From` impl the orphan rules would refuse here.
//!
//! Both types are `#[derive(DomainType)]`, like `missing_from_impl.rs`'s,
//! and the `From` impl here is real (not `todo!()`-bodied) so this fixture
//! fails for exactly one reason: the new const assertion, not a missing
//! trait bound.

#[derive(willikins_types::DomainType)]
#[domain(
    pattern = "[a-z]+",
    identifier,
    description = "A hand-rolled identifier-typed local type.",
    example = "abcd"
)]
struct Identified(String);

#[derive(willikins_types::DomainType)]
#[domain(pattern = "[a-z]+", description = "A plain local type.", example = "plain")]
struct Plain(String);

impl From<Identified> for Plain {
    fn from(value: Identified) -> Self {
        Self(value.to_string())
    }
}

fn main() {
    let _ = willikins_types::conversions![Identified => Plain];
}
