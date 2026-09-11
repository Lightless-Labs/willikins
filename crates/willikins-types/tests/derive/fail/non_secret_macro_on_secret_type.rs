//! `impl_domain_object_non_secret!` must refuse a type whose `DomainType`
//! says it is secret: the macro hardcodes `is_secret() -> false` and
//! `render() -> Rendered::Plain(self.to_string())`, so applying it to a
//! secret type would publish the secret's `Display` form as a plain value.

use willikins_types::{DomainType, ParseError};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Leaky(String);

impl std::fmt::Display for Leaky {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl DomainType for Leaky {
    const TYPE_NAME: &'static str = "Leaky";
    const IS_SECRET: bool = true;

    fn description() -> &'static str {
        "A hand-written secret type."
    }

    fn example() -> &'static str {
        "placeholder"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        Ok(Self(input.to_string()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

willikins_types::impl_domain_object_non_secret!(Leaky);

fn main() {}
