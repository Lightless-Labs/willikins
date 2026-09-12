//! [`Reported`]: adds a human-readable `message` to whatever a domain error
//! or warning already serializes as.
//!
//! Every error and warning an agent sees (`CheckError`, `CheckWarning`,
//! `PlanError`, ...) is internally tagged (`#[serde(tag = "kind")]`), so it
//! already serializes as `{"kind": "<Variant>", ...its own fields}`.
//! `Reported` wraps one of these for output, adding one more field --
//! `message`, the value's own [`std::fmt::Display`] rendering -- without
//! requiring every such type to carry a redundant `message` field of its
//! own that some other caller (a `match` on the type, its own `Display`)
//! would have to keep in sync by hand.

use std::fmt;

/// Wraps a `&T` so it serializes as `T`'s own fields plus one more:
/// `message`, `T`'s [`fmt::Display`] rendering.
///
/// `T` must already serialize to a JSON object (every type this wraps is
/// `#[serde(tag = "kind")]`, which guarantees that) and must not declare a
/// field named `message`: [`serde(flatten)`](serde::Serialize) resolves
/// such a collision by silently keeping one of the two values, not by
/// refusing to serialize, so nothing here can catch it at the call site --
/// see the type's own tests for the check instead (`check.rs`'s
/// `every_check_error_variant_serializes_with_its_kind` and its siblings,
/// and `tests/plan_error_serde.rs`).
pub struct Reported<'a, T>(&'a T);

impl<'a, T> Reported<'a, T> {
    /// Wrap `value` for serialization.
    #[must_use]
    pub fn new(value: &'a T) -> Self {
        Self(value)
    }
}

impl<T> serde::Serialize for Reported<'_, T>
where
    T: serde::Serialize + fmt::Display,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(serde::Serialize)]
        struct WithMessage<'a, T> {
            #[serde(flatten)]
            inner: &'a T,
            message: String,
        }
        WithMessage {
            inner: self.0,
            message: self.0.to_string(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserializer as _;
    use serde::de::{IgnoredAny, MapAccess, Visitor};

    /// Every top-level JSON object key found in `json`, in encounter order,
    /// including duplicates. Unlike parsing into a [`serde_json::Value`]
    /// (whose object map silently folds duplicate keys, keeping only one --
    /// which would make a `message`/`message` collision indistinguishable
    /// from the non-colliding case), this walks the token stream directly
    /// and records every key it sees.
    fn top_level_keys(json: &str) -> Vec<String> {
        struct KeyCollector(Vec<String>);

        impl<'de> Visitor<'de> for KeyCollector {
            type Value = Vec<String>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                while let Some(key) = map.next_key::<String>()? {
                    self.0.push(key);
                    map.next_value::<IgnoredAny>()?;
                }
                Ok(self.0)
            }
        }

        let mut deserializer = serde_json::Deserializer::from_str(json);
        deserializer
            .deserialize_map(KeyCollector(Vec::new()))
            .expect("valid JSON object")
    }

    fn count(keys: &[String], key: &str) -> usize {
        keys.iter().filter(|k| k.as_str() == key).count()
    }

    /// A minimal internally tagged sample, standing in for `CheckError` and
    /// friends without pulling in this crate's real ones.
    #[derive(serde::Serialize)]
    #[serde(tag = "kind")]
    enum Sample {
        Detail {
            #[allow(dead_code)]
            node: &'static str,
        },
    }

    impl fmt::Display for Sample {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "sample display text")
        }
    }

    #[test]
    fn reported_adds_exactly_one_message_alongside_the_one_kind() {
        let sample = Sample::Detail { node: "n" };
        let json = serde_json::to_string(&Reported::new(&sample)).unwrap();
        let keys = top_level_keys(&json);
        assert_eq!(count(&keys, "kind"), 1, "keys: {keys:?}");
        assert_eq!(count(&keys, "message"), 1, "keys: {keys:?}");

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kind"], "Detail");
        assert_eq!(value["message"], "sample display text");
    }

    /// Proves [`top_level_keys`] actually detects a collision rather than
    /// vacuously passing: a type with its own `message` field, once wrapped
    /// in [`Reported`], serializes with the key `message` twice on the
    /// wire. This is exactly the shape [`Reported`]'s own doc comment warns
    /// never to feed it; every real type this crate wraps is checked
    /// elsewhere (see the module docs) to have no such field.
    #[derive(serde::Serialize)]
    #[serde(tag = "kind")]
    enum Colliding {
        Detail {
            #[allow(dead_code)]
            message: &'static str,
        },
    }

    impl fmt::Display for Colliding {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "reported display text")
        }
    }

    #[test]
    fn a_field_named_message_collides_and_this_test_methodology_catches_it() {
        let colliding = Colliding::Detail {
            message: "the field's own value",
        };
        let json = serde_json::to_string(&Reported::new(&colliding)).unwrap();
        let keys = top_level_keys(&json);
        assert_eq!(
            count(&keys, "message"),
            2,
            "a colliding field must appear twice on the wire: keys: {keys:?}"
        );
    }
}
