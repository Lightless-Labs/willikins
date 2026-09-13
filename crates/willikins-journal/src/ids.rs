//! [`PlanId`] and [`RunId`]: opaque identifiers for a recorded plan and a
//! run of it, minted fresh by whoever calls `plan` or starts an `apply`
//! (`willikins-server`, task 7) and carried through every journal event
//! that concerns that plan or run.
//!
//! Both are `UUIDv7` (time-ordered, so a plain sort of ids is chronological)
//! rather than v4: nothing here relies on that ordering yet, but it costs
//! nothing and helps a human skimming a journal file or a database of
//! plans. Neither is a `willikins_types` domain type -- like
//! [`willikins_core::PrincipalId`] and [`willikins_core::apply::Timestamp`],
//! an id never flows through a tool port, so it has no business in the
//! type registry.

use std::fmt;
use std::str::FromStr;

/// Declares one UUID-backed identifier: `Display`, `FromStr`, `serde`, and
/// `JsonSchema`, plus a `new()` that mints a fresh `UUIDv7`.
macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(uuid::Uuid);

        impl $name {
            #[doc = concat!("A fresh ", stringify!($name), ", time-ordered (`UUIDv7`).")]
            #[must_use]
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7())
            }

            /// This id's underlying UUID.
            #[must_use]
            pub fn as_uuid(&self) -> uuid::Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                Ok(Self(uuid::Uuid::from_str(input)?))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::from_str(&raw).map_err(serde::de::Error::custom)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "format": "uuid",
                })
            }
        }
    };
}

uuid_id!(
    PlanId,
    "The identity of one recorded plan: minted when `plan` records a `PlanRecorded` event, \
     carried by every later event about that plan (an approval, a refusal, the run it starts)."
);

uuid_id!(
    RunId,
    "The identity of one `apply` run: minted when a plan is applied, carried by every later \
     event about that run (`NodeStarted`, `NodeFinished`, `RunFinished`)."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_and_parses_back() {
        let id = PlanId::new();
        let text = id.to_string();
        assert_eq!(PlanId::from_str(&text).unwrap(), id);
    }

    #[test]
    fn serializes_as_its_string_form() {
        let id = RunId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<RunId>(&json).unwrap(), id);
    }

    #[test]
    fn rejects_a_non_uuid_string() {
        assert!(serde_json::from_str::<PlanId>("\"not-a-uuid\"").is_err());
    }

    #[test]
    fn two_fresh_ids_differ() {
        assert_ne!(PlanId::new(), PlanId::new());
    }

    #[test]
    fn json_schema_is_a_uuid_string() {
        let schema = serde_json::to_value(schemars::schema_for!(PlanId)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["format"], "uuid");
    }
}
