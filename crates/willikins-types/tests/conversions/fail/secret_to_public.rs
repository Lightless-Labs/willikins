//! `conversions![Leaky => Public]` must fail to compile when `Leaky` is
//! secret and `Public` is not: "secrecy only goes up" is a compile-time
//! guarantee, not a runtime rule. `Leaky`'s `DomainType` impl is the same
//! shape as `non_secret_macro_on_secret_type.rs`'s (`IS_SECRET = true`),
//! but this fixture also gives it a hand-written secret `DomainObject`
//! impl (`__private::conversion` is bounded `A: DomainObject`, and without
//! one this fixture would fail for the wrong reason -- E0277 on a missing
//! `DomainObject` bound, not the secrecy assertion this fixture exists to
//! pin).

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

impl willikins_types::DomainObject for Leaky {
    fn type_name(&self) -> &'static str {
        Self::TYPE_NAME
    }

    fn is_secret(&self) -> bool {
        true
    }

    fn render(&self) -> willikins_types::Rendered {
        willikins_types::Rendered::Redacted {
            type_name: Self::TYPE_NAME,
        }
    }

    fn expose(&self, _token: &willikins_types::SinkToken) -> String {
        self.0.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn dyn_eq(&self, other: &dyn willikins_types::DomainObject) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| other == self)
    }

    fn clone_box(&self) -> Box<dyn willikins_types::DomainObject> {
        Box::new(self.clone())
    }
}

#[derive(willikins_types::DomainType)]
#[domain(pattern = "[a-z]+", description = "A public local type.", example = "public")]
struct Public(String);

impl From<Leaky> for Public {
    fn from(_leaky: Leaky) -> Self {
        todo!("never runs: this fixture must fail to compile before this body could matter")
    }
}

fn main() {
    let _ = willikins_types::conversions![Leaky => Public];
}
