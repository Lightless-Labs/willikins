//! [`Redacted<T>`]: the one place a journal event stores the *result* of
//! a core type's own redacting [`serde::Serialize`], rather than the type
//! itself.
//!
//! [`willikins_core::value::Value`] renders a secret as a fixed marker
//! through its own `Serialize` impl and, by design, has no `Deserialize`
//! at all -- redaction is one-way; a `[REDACTED DopplerServiceToken]`
//! marker cannot become a real token again, and no journal type may
//! invent a way around that (see the crate's module docs). But
//! [`crate::Journal::append`] writes one JSON line per event and
//! [`crate::FileJournal::open`] must read those lines back into memory
//! (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-journal` section: "replayed into memory on open"), and an
//! [`crate::Event`] that contained a live [`willikins_core::Plan`],
//! [`willikins_core::Inputs`], [`willikins_core::Outputs`], or
//! [`willikins_core::ApplyError`] directly could never derive
//! `Deserialize` itself, for exactly the same reason `Value` cannot.
//!
//! `Redacted<T>` resolves this: it is constructed only via `From<&T>` for
//! a closed set of core types (see [`Redactable`]), by calling
//! `serde_json::to_value` on the borrowed value -- the exact same
//! `Serialize` impl `serde_json::to_string` would use, so the JSON stored
//! here is byte-for-byte what a direct `Serialize` of `T` would have
//! produced, redaction included. Once built, a `Redacted<T>` is opaque
//! JSON data with nothing left to redact (the original bytes are gone,
//! not merely hidden), so it can `Serialize`/`Deserialize` freely: that
//! round trip cannot resurrect a secret that redaction already erased on
//! the way in. This is what "no journal type may implement `Serialize`
//! over a secret in any other way" (the module docs' own rule) actually
//! means in code: the *only* door from a `T` into the journal is this
//! `From` impl, and it is the same door `Serialize` already uses
//! elsewhere.
//!
//! A test in `tests/redaction_by_construction.rs` seeds a distinctive
//! secret value, builds a [`crate::Event::PlanRecorded`] and a
//! [`crate::Event::NodeFinished`] from it, and greps the appended JSONL
//! line for the seeded bytes.

use std::fmt;
use std::marker::PhantomData;

use indexmap::IndexMap;

mod sealed {
    pub trait Sealed {}
}

/// A core type `Redacted<T>` may wrap: closed over exactly the types that
/// can carry a [`willikins_core::value::Value`] and therefore need
/// [`crate::redacted::Redacted`]'s one-way construction rather than a
/// direct derive. Sealed: nothing outside this module can add a new
/// instance, so the closed set is enforced by the compiler, not by
/// convention.
pub trait Redactable: sealed::Sealed + serde::Serialize {
    /// A short, unique name for `Self`, used only to keep
    /// [`schemars::JsonSchema::schema_name`] distinct per instantiation of
    /// [`Redacted`] -- it never appears in the wire format.
    const KIND: &'static str;
}

impl sealed::Sealed for willikins_core::Plan {}
impl Redactable for willikins_core::Plan {
    const KIND: &'static str = "Plan";
}

impl sealed::Sealed for willikins_core::Inputs {}
impl Redactable for willikins_core::Inputs {
    const KIND: &'static str = "Inputs";
}

impl sealed::Sealed for willikins_core::Outputs {}
impl Redactable for willikins_core::Outputs {
    const KIND: &'static str = "Outputs";
}

impl sealed::Sealed for willikins_core::ApplyError {}
impl Redactable for willikins_core::ApplyError {
    const KIND: &'static str = "ApplyError";
}

impl sealed::Sealed for IndexMap<willikins_core::InputName, willikins_core::Value> {}
impl Redactable for IndexMap<willikins_core::InputName, willikins_core::Value> {
    const KIND: &'static str = "ResolvedInputs";
}

impl sealed::Sealed for IndexMap<willikins_core::OutputName, willikins_core::Value> {}
impl Redactable for IndexMap<willikins_core::OutputName, willikins_core::Value> {
    const KIND: &'static str = "ResolvedOutputs";
}

/// A `T` that has already gone through its own redacting `Serialize` and
/// is stored here as the resulting JSON. See the module docs for why this
/// exists and what it does and does not let back in.
///
/// `T` is phantom: nothing here ever holds a live `T` again. Every trait
/// this type implements (`Clone`, `Debug`, `PartialEq`, `Serialize`,
/// `Deserialize`) is written by hand rather than derived, precisely so
/// none of them require `T` to implement it too -- the wrapped JSON is
/// all there is.
pub struct Redacted<T> {
    json: serde_json::Value,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Redacted<T> {
    fn from_json(json: serde_json::Value) -> Self {
        Self {
            json,
            _marker: PhantomData,
        }
    }

    /// Borrow the redacted JSON this wraps.
    #[must_use]
    pub fn as_json(&self) -> &serde_json::Value {
        &self.json
    }
}

impl<T: Redactable> From<&T> for Redacted<T> {
    fn from(value: &T) -> Self {
        let json = serde_json::to_value(value)
            .unwrap_or_else(|err| unreachable!("{} always serializes: {err}", T::KIND));
        Self::from_json(json)
    }
}

impl<T> Clone for Redacted<T> {
    fn clone(&self) -> Self {
        Self::from_json(self.json.clone())
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Redacted").field(&self.json).finish()
    }
}

impl<T> PartialEq for Redacted<T> {
    fn eq(&self, other: &Self) -> bool {
        self.json == other.json
    }
}

impl<T> serde::Serialize for Redacted<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.json.serialize(serializer)
    }
}

impl<'de, T> serde::Deserialize<'de> for Redacted<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_json(serde_json::Value::deserialize(
            deserializer,
        )?))
    }
}

impl<T: Redactable> schemars::JsonSchema for Redacted<T> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(format!("Redacted{}", T::KIND))
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Permissive on purpose: the field holds arbitrary already-redacted
        // JSON, not a value shaped like `T` any more (a secret port's
        // content is gone, not merely disguised), so there is nothing more
        // specific to publish.
        schemars::json_schema!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema as _;
    use willikins_core::{InputName, OutputName, Value};
    use willikins_types::DomainType;

    #[test]
    fn round_trips_a_non_secret_inputs_map() {
        let mut map: IndexMap<InputName, Value> = IndexMap::new();
        map.insert(
            InputName::parse("slug").unwrap(),
            Value::known(willikins_types::ProjectSlug::parse("widgets").unwrap()),
        );
        let redacted = Redacted::from(&map);
        let json = serde_json::to_string(&redacted).unwrap();
        assert!(json.contains("widgets"));
        let back: Redacted<IndexMap<InputName, Value>> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, redacted);
    }

    #[test]
    fn redacts_a_secret_output() {
        let mut outputs = willikins_core::Outputs::new();
        outputs.insert(
            willikins_core::PortName::parse("token").unwrap(),
            Value::known(
                willikins_types::DopplerServiceToken::parse(&format!(
                    "dp.st.prd.{}",
                    "MARKER".repeat(7)
                ))
                .unwrap(),
            ),
        );
        let redacted = Redacted::from(&outputs);
        let json = serde_json::to_string(&redacted).unwrap();
        assert!(!json.contains("MARKER"));
        assert!(json.contains("REDACTED"));
    }

    #[test]
    fn schema_names_differ_per_wrapped_type() {
        let inputs_schema = Redacted::<willikins_core::Inputs>::schema_name();
        let outputs_schema = Redacted::<willikins_core::Outputs>::schema_name();
        assert_ne!(inputs_schema, outputs_schema);
    }

    #[test]
    fn output_map_redacts_too() {
        let mut map: IndexMap<OutputName, Value> = IndexMap::new();
        let ty = willikins_core::TypeRef::scalar(willikins_core::TypeName::parse("Url").unwrap());
        map.insert(OutputName::parse("repo_url").unwrap(), Value::unknown(ty));
        let redacted = Redacted::from(&map);
        assert!(redacted.as_json().is_object());
    }
}
