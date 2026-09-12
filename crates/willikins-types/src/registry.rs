//! The type registry: type references, and a name-keyed catalog of parsers.
//!
//! [`TypeRef`] is how a port or workflow input names a domain type —
//! `"GitHubRepo"` or `"list<GitHubRepo>"` — the string form that appears in
//! the DSL and in JSON schemas. [`TypeRegistry`] is the runtime catalog that
//! resolves a [`TypeName`] to its [`TypeInfo`] and its parser, built once by
//! the [`domain_types`] macro from the same list that builds
//! [`crate::type_infos`], so the two cannot drift.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use crate::object::DomainObject;
use crate::{DomainType, ParseError, TypeInfo};

/// The pattern every [`TypeName`] must match: an uppercase ASCII letter,
/// then any number of ASCII letters or digits.
const TYPE_NAME_PATTERN: &str = "^[A-Z][A-Za-z0-9]*$";

static TYPE_NAME_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(TYPE_NAME_PATTERN).expect("TYPE_NAME_PATTERN is valid"));

/// The name of a domain type, as it appears in [`TypeRef`], port
/// declarations, and JSON schemas: `PascalCase`, no separators.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeName(String);

impl TypeName {
    /// Parse a type name, checking it against [`TYPE_NAME_PATTERN`].
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `input` does not match the pattern.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        if TYPE_NAME_REGEX.is_match(input) {
            Ok(Self(input.to_string()))
        } else {
            Err(ParseError::new(
                "TypeName",
                format!(
                    "{} is not a valid type name (expected to match `{TYPE_NAME_PATTERN}`)",
                    crate::quoted(input)
                ),
            ))
        }
    }

    /// Borrow the name as a plain string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Build a [`TypeName`] from a `&'static str` known to already match the
    /// pattern, such as a [`DomainType::TYPE_NAME`]. Debug-asserts the
    /// pattern rather than returning `Result`, because a `TYPE_NAME`
    /// constant that fails it is a bug in this crate, not bad input.
    fn from_static(name: &'static str) -> Self {
        debug_assert!(
            TYPE_NAME_REGEX.is_match(name),
            "TYPE_NAME {name:?} does not match `{TYPE_NAME_PATTERN}`"
        );
        Self(name.to_string())
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for TypeName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for TypeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for TypeName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TypeName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": TYPE_NAME_PATTERN,
        })
    }
}

/// The published schema pattern for [`TypeRef`]: a bare [`TypeName`], or
/// `list<TypeName>`.
const TYPE_REF_PATTERN: &str = "^(?:[A-Z][A-Za-z0-9]*|list<[A-Z][A-Za-z0-9]*>)$";

/// A reference to a domain type, with an optional `list<...>` cardinality
/// flag. `list<T>` is a cardinality flag on the port or input, not a
/// separate domain type: a list is secret iff its element type is secret.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeRef {
    /// The referenced type's name.
    pub name: TypeName,
    /// Whether this reference names a list of the type, rather than a
    /// single scalar value.
    pub list: bool,
}

impl TypeRef {
    /// A scalar reference to `name`.
    #[must_use]
    pub fn scalar(name: TypeName) -> Self {
        Self { name, list: false }
    }

    /// A `list<name>` reference.
    #[must_use]
    pub fn list_of(name: TypeName) -> Self {
        Self { name, list: true }
    }

    /// The scalar reference to this reference's element type: itself, if
    /// already scalar, or its element type with `list` cleared.
    #[must_use]
    pub fn element(&self) -> Self {
        Self {
            name: self.name.clone(),
            list: false,
        }
    }

    /// Parse `"T"` or `"list<T>"`.
    ///
    /// Rejects any whitespace, `"List<T>"` (the `list` keyword is
    /// case-sensitive), `"list<list<T>>"` (no nested lists), and
    /// `"list<>"` (no empty element name).
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `input` is neither form.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        if input.chars().any(char::is_whitespace) {
            return Err(ParseError::new(
                "TypeRef",
                format!("{} must not contain whitespace", crate::quoted(input)),
            ));
        }
        if let Some(inner) = input
            .strip_prefix("list<")
            .and_then(|s| s.strip_suffix('>'))
        {
            let name = TypeName::parse(inner).map_err(|err| {
                ParseError::new("TypeRef", format!("list element: {}", err.reason))
            })?;
            return Ok(Self { name, list: true });
        }
        let name = TypeName::parse(input)?;
        Ok(Self { name, list: false })
    }
}

impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.list {
            write!(f, "list<{}>", self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

impl serde::Serialize for TypeRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for TypeRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for TypeRef {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TypeRef")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": TYPE_REF_PATTERN,
        })
    }
}

/// The reason a [`TypeRegistry`] gives for refusing to parse a secret type
/// from a string. Fake-state seeding constructs secret values through serde
/// `Deserialize` on the concrete type instead.
const SECRET_LITERAL_REFUSAL: &str = "secret types cannot be supplied as literals or inputs";

/// One entry in a [`TypeRegistry`]: a type's catalog description plus its
/// string parser, type-erased to [`DomainObject`].
pub struct TypeEntry {
    /// The type's catalog description.
    pub info: TypeInfo,
    /// Parse a string as this type, type-erased. For a secret type this
    /// always returns `Err` with [`SECRET_LITERAL_REFUSAL`], without ever
    /// calling the type's own parser.
    pub parse: fn(&str) -> Result<Arc<dyn DomainObject>, ParseError>,
}

impl TypeEntry {
    /// Build the entry for `T`.
    #[must_use]
    pub fn of<T: DomainType + DomainObject + 'static>() -> Self {
        Self {
            info: TypeInfo::of::<T>(),
            parse: parse_entry::<T>,
        }
    }
}

/// The type-erased parser behind every [`TypeEntry`]. Checks
/// [`DomainType::IS_SECRET`] before calling `T::parse`, so a secret type's
/// input is never validated or echoed.
fn parse_entry<T: DomainType + DomainObject + 'static>(
    input: &str,
) -> Result<Arc<dyn DomainObject>, ParseError> {
    if T::IS_SECRET {
        return Err(ParseError::new(T::TYPE_NAME, SECRET_LITERAL_REFUSAL));
    }
    T::parse(input).map(|value| Arc::new(value) as Arc<dyn DomainObject>)
}

/// The runtime catalog of every domain type this crate defines, keyed by
/// [`TypeName`]. Built once by [`domain_types`] from the same list that
/// builds [`crate::type_infos`].
pub struct TypeRegistry {
    entries: Vec<TypeEntry>,
    index: HashMap<TypeName, usize>,
}

impl TypeRegistry {
    /// Build a registry from its entries, indexing them by name.
    ///
    /// # Panics
    ///
    /// Panics (via `debug_assert`) in debug builds if two entries share a
    /// name; the macro that calls this is the only caller, and a duplicate
    /// there is a bug in this crate, not bad input.
    pub(crate) fn from_entries(entries: Vec<TypeEntry>) -> Self {
        let mut index = HashMap::with_capacity(entries.len());
        for (position, entry) in entries.iter().enumerate() {
            let name = TypeName::from_static(entry.info.name);
            let previous = index.insert(name, position);
            debug_assert!(
                previous.is_none(),
                "duplicate type name in registry: {}",
                entry.info.name
            );
        }
        Self { entries, index }
    }

    /// Look up the entry for `name`.
    #[must_use]
    pub fn get(&self, name: &TypeName) -> Option<&TypeEntry> {
        self.index
            .get(name)
            .map(|&position| &self.entries[position])
    }

    /// Resolve the entry for `name` for use as a literal: an unregistered
    /// name and a secret type are both refused here, before any input is
    /// looked at.
    ///
    /// Callers that parse a whole list of literals resolve the element type
    /// through this once, so that a list with *no* elements is refused on
    /// exactly the same grounds as a list with one — the map-over-inputs
    /// shape would otherwise silently accept an empty secret-typed or
    /// unregistered list.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `name` is not registered, or when it
    /// names a secret type.
    pub fn literal_entry(&self, name: &TypeName) -> Result<&TypeEntry, ParseError> {
        let entry = self
            .get(name)
            .ok_or_else(|| ParseError::new("TypeRegistry", format!("unknown type `{name}`")))?;
        if entry.info.secret {
            return Err(ParseError::new(entry.info.name, SECRET_LITERAL_REFUSAL));
        }
        Ok(entry)
    }

    /// Parse `input` as `name`.
    ///
    /// An unknown type name is a [`ParseError`] naming the type. A secret
    /// type refuses with [`SECRET_LITERAL_REFUSAL`] without calling its own
    /// parser.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `name` is not registered, or when the
    /// registered type refuses or fails to parse `input`.
    pub fn parse(&self, name: &TypeName, input: &str) -> Result<Arc<dyn DomainObject>, ParseError> {
        (self.literal_entry(name)?.parse)(input)
    }

    /// Whether `name` is a secret type, or `None` if `name` is not
    /// registered.
    #[must_use]
    pub fn is_secret(&self, name: &TypeName) -> Option<bool> {
        self.get(name).map(|entry| entry.info.secret)
    }

    /// Iterate every registered entry.
    pub fn iter(&self) -> impl Iterator<Item = &TypeEntry> {
        self.entries.iter()
    }
}

/// Generate `type_infos()`, `registry()`, and `assert_all_examples_parse()`
/// from one list of domain types, so the type catalog and the type registry
/// cannot drift apart.
///
/// Invoked exactly once, in `lib.rs`, listing every domain type this crate
/// defines.
macro_rules! domain_types {
    ($($ty:ty),+ $(,)?) => {
        /// The beginning of the type catalog: every domain type this crate
        /// defines. Later milestone-1 tasks append the org,
        /// resource-identity, and credential types.
        #[must_use]
        pub fn type_infos() -> ::std::vec::Vec<$crate::TypeInfo> {
            ::std::vec![$($crate::TypeInfo::of::<$ty>()),+]
        }

        /// The type registry: every domain type in [`type_infos`], keyed by
        /// name, with its string parser.
        #[must_use]
        pub fn registry() -> &'static $crate::registry::TypeRegistry {
            static REGISTRY: ::std::sync::LazyLock<$crate::registry::TypeRegistry> =
                ::std::sync::LazyLock::new(|| {
                    $crate::registry::TypeRegistry::from_entries(::std::vec![
                        $($crate::registry::TypeEntry::of::<$ty>()),+
                    ])
                });
            &REGISTRY
        }

        /// Assert every listed type's own example parses through its own
        /// [`crate::DomainType::parse`] — bypassing the registry, which
        /// refuses to parse a secret type from a string at all, so this is
        /// the only way to check a secret type's example.
        pub fn assert_all_examples_parse() {
            $($crate::assert_example_parses::<$ty>();)+
        }
    };
}

pub(crate) use domain_types;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DopplerSecretValue, DopplerServiceToken, GitHubOrg};

    // -------------------------------------------------------------
    // TypeName
    // -------------------------------------------------------------

    #[test]
    fn type_name_accepts_pascal_case() {
        assert_eq!(
            TypeName::parse("GitHubRepo").unwrap().as_str(),
            "GitHubRepo"
        );
    }

    #[test]
    fn type_name_accepts_a_single_letter() {
        assert!(TypeName::parse("T").is_ok());
    }

    #[test]
    fn type_name_rejects_lowercase_start() {
        assert!(TypeName::parse("gitHubRepo").is_err());
    }

    #[test]
    fn type_name_rejects_empty() {
        assert!(TypeName::parse("").is_err());
    }

    #[test]
    fn type_name_rejects_separators() {
        assert!(TypeName::parse("GitHub-Repo").is_err());
        assert!(TypeName::parse("GitHub_Repo").is_err());
        assert!(TypeName::parse("GitHub Repo").is_err());
    }

    #[test]
    fn type_name_displays_as_its_string() {
        assert_eq!(
            TypeName::parse("GitHubRepo").unwrap().to_string(),
            "GitHubRepo"
        );
    }

    #[test]
    fn type_name_serde_round_trips() {
        let name = TypeName::parse("GitHubRepo").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"GitHubRepo\"");
        assert_eq!(serde_json::from_str::<TypeName>(&json).unwrap(), name);
    }

    #[test]
    fn type_name_deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<TypeName>("\"not valid\"").is_err());
    }

    #[test]
    fn type_name_orders_lexicographically() {
        let a = TypeName::parse("Alpha").unwrap();
        let b = TypeName::parse("Beta").unwrap();
        assert!(a < b);
    }

    #[test]
    fn type_name_schema_is_a_pattern_string() {
        let schema = serde_json::to_value(schemars::schema_for!(TypeName)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["pattern"], TYPE_NAME_PATTERN);
    }

    // -------------------------------------------------------------
    // TypeRef
    // -------------------------------------------------------------

    #[test]
    fn type_ref_parses_a_scalar() {
        let ty_ref = TypeRef::parse("GitHubRepo").unwrap();
        assert_eq!(ty_ref.name.as_str(), "GitHubRepo");
        assert!(!ty_ref.list);
    }

    #[test]
    fn type_ref_parses_a_list() {
        let ty_ref = TypeRef::parse("list<GitHubRepo>").unwrap();
        assert_eq!(ty_ref.name.as_str(), "GitHubRepo");
        assert!(ty_ref.list);
    }

    #[test]
    fn type_ref_rejects_capitalized_list_keyword() {
        assert!(TypeRef::parse("List<GitHubRepo>").is_err());
    }

    #[test]
    fn type_ref_rejects_nested_lists() {
        assert!(TypeRef::parse("list<list<GitHubRepo>>").is_err());
    }

    #[test]
    fn type_ref_rejects_an_empty_list_element() {
        assert!(TypeRef::parse("list<>").is_err());
    }

    #[test]
    fn type_ref_rejects_whitespace_variants() {
        assert!(TypeRef::parse(" GitHubRepo").is_err());
        assert!(TypeRef::parse("GitHubRepo ").is_err());
        assert!(TypeRef::parse("list< GitHubRepo>").is_err());
        assert!(TypeRef::parse("list<GitHubRepo >").is_err());
        assert!(TypeRef::parse("list <GitHubRepo>").is_err());
    }

    #[test]
    fn type_ref_accepts_the_no_whitespace_variant() {
        assert!(TypeRef::parse("list<GitHubRepo>").is_ok());
    }

    #[test]
    fn type_ref_displays_a_scalar_and_a_list() {
        assert_eq!(
            TypeRef::parse("GitHubRepo").unwrap().to_string(),
            "GitHubRepo"
        );
        assert_eq!(
            TypeRef::parse("list<GitHubRepo>").unwrap().to_string(),
            "list<GitHubRepo>"
        );
    }

    #[test]
    fn type_ref_serde_round_trips() {
        for text in ["GitHubRepo", "list<GitHubRepo>"] {
            let ty_ref = TypeRef::parse(text).unwrap();
            let json = serde_json::to_string(&ty_ref).unwrap();
            assert_eq!(json, format!("{text:?}"));
            assert_eq!(serde_json::from_str::<TypeRef>(&json).unwrap(), ty_ref);
        }
    }

    #[test]
    fn type_ref_schema_is_a_pattern_string() {
        let schema = serde_json::to_value(schemars::schema_for!(TypeRef)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["pattern"], TYPE_REF_PATTERN);
    }

    #[test]
    fn type_ref_scalar_and_list_of_constructors() {
        let name = TypeName::parse("GitHubRepo").unwrap();
        assert_eq!(
            TypeRef::scalar(name.clone()),
            TypeRef::parse("GitHubRepo").unwrap()
        );
        assert_eq!(
            TypeRef::list_of(name),
            TypeRef::parse("list<GitHubRepo>").unwrap()
        );
    }

    #[test]
    fn type_ref_element_strips_the_list_flag() {
        let list_ref = TypeRef::parse("list<GitHubRepo>").unwrap();
        let element = list_ref.element();
        assert_eq!(element.name, list_ref.name);
        assert!(!element.list);
    }

    // -------------------------------------------------------------
    // TypeRegistry
    // -------------------------------------------------------------

    #[test]
    fn registry_gets_a_registered_non_secret_type() {
        let name = TypeName::parse("GitHubOrg").unwrap();
        let entry = crate::registry()
            .get(&name)
            .expect("GitHubOrg is registered");
        assert_eq!(entry.info.name, "GitHubOrg");
        assert!(!entry.info.secret);
    }

    #[test]
    fn registry_get_returns_none_for_an_unknown_type() {
        let name = TypeName::parse("NoSuchType").unwrap();
        assert!(crate::registry().get(&name).is_none());
    }

    #[test]
    fn registry_parses_a_non_secret_type() {
        let name = TypeName::parse("GitHubOrg").unwrap();
        let value = crate::registry().parse(&name, "lightless-labs").unwrap();
        assert_eq!(value.type_name(), "GitHubOrg");
        let org = crate::downcast::<GitHubOrg>(&*value).expect("downcasts to GitHubOrg");
        assert_eq!(org.as_str(), "lightless-labs");
    }

    #[test]
    fn registry_parse_of_unknown_type_names_the_type() {
        let name = TypeName::parse("NoSuchType").unwrap();
        let err = crate::registry().parse(&name, "anything").unwrap_err();
        assert!(
            err.reason.contains("NoSuchType"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn registry_is_secret_reports_secrecy_for_registered_types() {
        let secret = TypeName::parse("DopplerServiceToken").unwrap();
        let plain = TypeName::parse("GitHubOrg").unwrap();
        let unknown = TypeName::parse("NoSuchType").unwrap();
        assert_eq!(crate::registry().is_secret(&secret), Some(true));
        assert_eq!(crate::registry().is_secret(&plain), Some(false));
        assert_eq!(crate::registry().is_secret(&unknown), None);
    }

    #[test]
    fn registry_iter_covers_every_registered_type() {
        let names: std::collections::HashSet<&str> = crate::registry()
            .iter()
            .map(|entry| entry.info.name)
            .collect();
        assert!(names.contains("GitHubOrg"));
        assert!(names.contains("DopplerServiceToken"));
        assert_eq!(names.len(), crate::type_infos().len());
    }

    // -------------------------------------------------------------
    // Secret refusal
    // -------------------------------------------------------------

    #[test]
    fn registry_refuses_to_parse_doppler_service_token_as_a_literal() {
        let name = TypeName::parse("DopplerServiceToken").unwrap();
        let err = crate::registry()
            .parse(&name, "dp.st.prd.exampleexampleexample")
            .unwrap_err();
        assert_eq!(err.reason, SECRET_LITERAL_REFUSAL);
    }

    #[test]
    fn registry_refuses_to_parse_doppler_secret_value_as_a_literal() {
        let name = TypeName::parse("DopplerSecretValue").unwrap();
        let err = crate::registry().parse(&name, "s3cr3t-value").unwrap_err();
        assert_eq!(err.reason, SECRET_LITERAL_REFUSAL);
    }

    #[test]
    fn registry_secret_refusal_never_touches_the_input_even_when_invalid() {
        // An input that would fail DopplerServiceToken's own pattern check
        // (too short) still gets the generic refusal, not a pattern error,
        // proving the type's own parser is never called.
        let name = TypeName::parse("DopplerServiceToken").unwrap();
        let err = crate::registry()
            .parse(&name, "MARKER-too-short")
            .unwrap_err();
        assert_eq!(err.reason, SECRET_LITERAL_REFUSAL);
        assert!(!err.reason.contains("MARKER"));
    }

    #[test]
    fn literal_entry_resolves_a_non_secret_type() {
        let name = TypeName::parse("GitHubOrg").unwrap();
        let Ok(entry) = crate::registry().literal_entry(&name) else {
            panic!("GitHubOrg is registered and not secret")
        };
        assert_eq!(entry.info.name, "GitHubOrg");
    }

    #[test]
    fn literal_entry_refuses_a_secret_type_with_no_input_at_all() {
        // The refusal must not depend on there being an input to reject:
        // this is what stops an empty `list<Secret>` literal.
        let name = TypeName::parse("DopplerServiceToken").unwrap();
        let Err(err) = crate::registry().literal_entry(&name) else {
            panic!("a secret type must be refused")
        };
        assert_eq!(err.reason, SECRET_LITERAL_REFUSAL);
        assert_eq!(err.type_name, "DopplerServiceToken");
    }

    #[test]
    fn literal_entry_refuses_an_unregistered_type() {
        let name = TypeName::parse("NoSuchType").unwrap();
        let Err(err) = crate::registry().literal_entry(&name) else {
            panic!("an unregistered type must be refused")
        };
        assert!(err.reason.contains("NoSuchType"), "{}", err.reason);
    }

    #[test]
    fn secret_type_deserialize_is_unchanged_by_the_registry_refusal() {
        // The registry refuses secret literals, but plain serde
        // `Deserialize` on the concrete type still works: this is how
        // fake-state seeding constructs secret values.
        let token: DopplerServiceToken =
            serde_json::from_str("\"dp.st.prd.exampleexampleexample\"").unwrap();
        assert_eq!(format!("{token:?}"), "[REDACTED DopplerServiceToken]");

        let value: DopplerSecretValue = serde_json::from_str("\"s3cr3t-value\"").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerSecretValue]");
    }
}
