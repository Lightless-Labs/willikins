//! The YAML document format: serde structs mirroring the `willikins-dsl`
//! section of the milestone 1 plan.
//!
//! Every struct here derives [`serde::Deserialize`] (documents are never
//! serialized back to YAML) and [`schemars::JsonSchema`] so the doc
//! comments below become the published document schema's field
//! descriptions ([`crate::document_schema`]).
//!
//! `serde_yaml_ng` does not reject a duplicate mapping key on its own — it
//! silently keeps the last value, the same as `serde`'s blanket `HashMap`
//! and `IndexMap` support generally does for any format. `inputs`,
//! `steps`, `outputs`, and a step's `with` all go through
//! [`deserialize_unique_map`] (or [`deserialize_with_map`], its `with`
//! specialisation) instead of a plain derived `IndexMap` deserialization,
//! so a duplicate key is reported as an error naming the key, with the
//! line and column `serde_yaml_ng` attaches to any error raised from
//! inside a `Visitor` — including one raised with
//! `serde::de::Error::custom`, not only its own structural errors.

use std::fmt;
use std::marker::PhantomData;

use indexmap::IndexMap;
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

/// The published schema for [`Document::name`]: [`willikins_types::WorkflowName`]'s
/// own schema (pattern and length), even though the field is stored as a
/// plain `String` here. `serde` never validates it: `Document::name` is
/// checked by parsing it as a `WorkflowName` in
/// `willikins_dsl::document_to_workflow`, the same way every other typed
/// field in this format is checked, so a bad name is a located
/// [`crate::DocumentErrorKind::Semantic`] rather than a `serde` error.
///
/// Generated through the `JsonSchema` impl rather than lifted from
/// `DomainType::json_schema` (which is `schema_for!`, a standalone schema
/// *document*): a subschema under `properties` must not carry its own
/// `$schema` dialect declaration or a `title` that renames the field.
fn workflow_name_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    <willikins_types::WorkflowName as schemars::JsonSchema>::json_schema(generator)
}

/// The published schema for a document's free-text `description:` field
/// (on [`Document`] and [`InputDecl`]): [`willikins_types::Description`]'s
/// own schema. See [`workflow_name_schema`] for why the Rust field type
/// stays `Option<String>` and why this goes through `JsonSchema`.
///
/// The field is optional, so `null` is one of its values: `description:`
/// with nothing after it is a document with no description, which the
/// parser accepts and this schema must too. A `schema_with` function
/// replaces whatever schema the field's own type would have produced, so
/// the nullability `Option<String>` would have carried has to be restated
/// here or the published schema refuses what the parser takes.
fn description_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = <willikins_types::Description as schemars::JsonSchema>::json_schema(generator);
    schema.insert("type".to_string(), serde_json::json!(["string", "null"]));
    schema
}

/// A workflow document: named inputs, a graph of tool-calling steps, and
/// named outputs.
///
/// A workflow document is privileged content: run it only from a trusted
/// ref. Its free-text fields (`description:`, here and on an input) are
/// shown to whatever agent reads them as quoted document text, never as
/// an instruction to that agent.
// Every struct in this module carries `deny_unknown_fields`, which the
// published schema reflects as `additionalProperties: false`. Ignoring an
// unrecognised field meant a misspelled key -- `foreach` for `for_each`,
// say -- validated clean while the workflow did something other than what
// its author wrote, and meant a YAML merge key (`<<`, which serde never
// applies to a struct) silently dropped whatever it was merging.
// Adversarial pass 2, finding 3. Kept out of the doc comment so it stays
// out of the published schema's `description`.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// The workflow's name.
    #[schemars(schema_with = "workflow_name_schema")]
    pub name: String,
    /// One-line description shown to an agent.
    #[serde(default)]
    #[schemars(schema_with = "description_schema")]
    pub description: Option<String>,
    /// The workflow's declared inputs, keyed by name.
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    pub inputs: IndexMap<String, InputDecl>,
    /// The workflow's nodes (a `steps.<name>` entry each), keyed by name.
    #[serde(deserialize_with = "deserialize_unique_map")]
    pub steps: IndexMap<String, StepDecl>,
    /// The workflow's outputs, keyed by name: each value is a reference or
    /// a literal string, in the same grammar as a step's `with` value.
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    pub outputs: IndexMap<String, String>,
}

/// One declared workflow input.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputDecl {
    /// The type this input accepts: a bare type name such as `GitHubOrg`,
    /// or `list<TypeName>`.
    #[serde(rename = "type")]
    pub ty: String,
    /// The value used when the workflow is run without this input bound.
    #[serde(default)]
    pub default: Option<DefaultValue>,
    /// One-line description shown to an agent.
    #[serde(default)]
    #[schemars(schema_with = "description_schema")]
    pub description: Option<String>,
}

/// An input's default value: a single scalar for a scalar-typed input, or
/// a list of scalars for a `list<T>`-typed one. Checked against the
/// input's declared type when the document is loaded.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum DefaultValue {
    /// A single scalar default.
    String(String),
    /// A list default.
    List(Vec<String>),
}

/// One declared workflow node (a `steps.<name>` entry): a call to one
/// tool (`tool:`), or a reference to another document's workflow
/// (`uses:`, milestone 2b decision (d1)) -- exactly one of the two.
///
/// `document_to_workflow` enforces "exactly one", not `serde`: a plain
/// `#[serde(flatten)]` untagged enum would only ever give a generic "data
/// did not match any variant" message (the same reason [`WithValue`]'s
/// `Deserialize` is hand-written), so both fields stay `Option` here and
/// the located `Semantic` error is built once the whole step has
/// deserialized.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = step_decl_exactly_one_of_tool_or_uses)]
pub struct StepDecl {
    /// The tool this step calls, such as `github.repo.ensure`. Exactly
    /// one of `tool` or `uses` must be set.
    #[serde(default)]
    pub tool: Option<String>,
    /// The name of another document's workflow this step uses, instead
    /// of calling a tool directly (milestone 2b decision (d1)). Exactly
    /// one of `tool` or `uses` must be set. A `uses:` step may not have a
    /// `for_each:` (decision (d12)).
    #[serde(default)]
    pub uses: Option<String>,
    /// When set, this step runs once per item of the bound list. Must be
    /// a reference (the same grammar as a `with` value), never a literal.
    /// Never set together with `uses:`.
    #[serde(default)]
    pub for_each: Option<String>,
    /// This step's input bindings, keyed by port name for a `tool:` step
    /// or by the used document's own input name for a `uses:` step. Each
    /// value is a reference or a literal string ([`WithValue::Scalar`]),
    /// or a YAML sequence of the same ([`WithValue::List`], milestone 3g
    /// decision (a)).
    #[serde(default, deserialize_with = "deserialize_with_map")]
    pub with: IndexMap<String, WithValue>,
}

/// Adds the `oneOf` milestone 2b decision (d1) requires to [`StepDecl`]'s
/// derived schema: exactly one of `tool` or `uses` is required, so an
/// agent validating a document locally learns the rule before it ever
/// calls `validate`. A plain JSON Schema object validates against every
/// keyword present at its own level (`type`, `properties`, `required`,
/// and this `oneOf` all apply at once), so adding the key here combines
/// with the schema the derive already produced rather than replacing it
/// -- the same technique [`description_schema`] uses for a single field,
/// lifted to the whole container via `#[schemars(transform = ...)]`
/// (a post-mutator: it runs after the derive's own fields and
/// descriptions are in place, so neither is lost).
///
/// Each branch pins its own field's type to `"string"`, narrower than the
/// derived schema's `["string", "null"]`: `required` alone only checks
/// key *presence*, so without this, `{tool: null, uses: x}` would match
/// both branches (`tool` is present, if `null`) and the schema would
/// refuse a step `document_to_workflow` accepts (`serde` reads a `null`
/// scalar the same as an absent key for an `Option<String>`). With it,
/// the branch for a `null` field fails on type instead of passing on
/// presence, so the schema agrees with the parser on every case: neither
/// set, either `null`, or both set as strings.
fn step_decl_exactly_one_of_tool_or_uses(schema: &mut schemars::Schema) {
    schema.insert(
        "oneOf".to_string(),
        serde_json::json!([
            { "required": ["tool"], "properties": { "tool": { "type": "string" } } },
            { "required": ["uses"], "properties": { "uses": { "type": "string" } } },
        ]),
    );
}

/// One `with:` map value: a single reference-or-literal string
/// ([`Self::Scalar`]), or a YAML sequence of them ([`Self::List`],
/// milestone 3g decision (a)) -- one binding per element, each itself a
/// reference or a literal, never nested. A mapping is rejected wherever it
/// appears (as the whole value, or as a sequence element) with a message
/// naming what a `with` value may actually be, rather than serde's generic
/// "invalid type" wording; so is a nested sequence, as a sequence element.
///
/// `Deserialize` is hand-written, not derived, because `#[serde(untagged)]`
/// would give a generic "data did not match any variant" message for a
/// mapping instead of the message above -- the same reason
/// [`WithString`], its predecessor, existed before this type did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithValue {
    /// A single reference or literal.
    Scalar(String),
    /// A sequence of references and/or literals, one binding per element.
    List(Vec<String>),
}

impl<'de> Deserialize<'de> for WithValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WithValueVisitor;

        impl<'de> Visitor<'de> for WithValueVisitor {
            type Value = WithValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string, or a sequence of strings")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WithValue::Scalar(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WithValue::Scalar(value))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(WithString(item)) = seq.next_element::<WithString>()? {
                    items.push(item);
                }
                Ok(WithValue::List(items))
            }

            fn visit_map<A>(self, _map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                Err(de::Error::custom(
                    "with values must be strings or references",
                ))
            }
        }

        deserializer.deserialize_any(WithValueVisitor)
    }
}

impl schemars::JsonSchema for WithValue {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("WithValue")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [
                { "type": "string" },
                { "type": "array", "items": { "type": "string" } },
            ],
        })
    }
}

/// A `with` map value: like a plain `String`, except a YAML sequence or
/// mapping is rejected with a message naming what a `with` value may
/// actually be, rather than serde's generic "invalid type" wording. Also
/// used, unchanged, to parse each element of a [`WithValue::List`]
/// sequence -- a nested sequence or a mapping element is rejected the same
/// way.
struct WithString(String);

impl<'de> Deserialize<'de> for WithString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WithStringVisitor;

        impl<'de> Visitor<'de> for WithStringVisitor {
            type Value = WithString;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WithString(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WithString(value))
            }

            fn visit_seq<A>(self, _seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                Err(de::Error::custom(
                    "with values must be strings or references",
                ))
            }

            fn visit_map<A>(self, _map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                Err(de::Error::custom(
                    "with values must be strings or references",
                ))
            }
        }

        deserializer.deserialize_any(WithStringVisitor)
    }
}

/// A `Visitor` that collects a YAML mapping into an [`IndexMap`], erroring
/// on a repeated key rather than silently keeping the last value.
struct UniqueMapVisitor<V> {
    marker: PhantomData<V>,
}

impl<'de, V: Deserialize<'de>> Visitor<'de> for UniqueMapVisitor<V> {
    type Value = IndexMap<String, V>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a mapping with unique keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut out = IndexMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let value = map.next_value::<V>()?;
            out.insert(key, value);
        }
        Ok(out)
    }
}

/// Deserialize a YAML mapping into an [`IndexMap`], rejecting a duplicate
/// key.
fn deserialize_unique_map<'de, D, V>(deserializer: D) -> Result<IndexMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    deserializer.deserialize_map(UniqueMapVisitor {
        marker: PhantomData,
    })
}

/// Deserialize a step's `with` mapping: unique keys, string-only values.
fn deserialize_with_map<'de, D>(deserializer: D) -> Result<IndexMap<String, WithValue>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_map(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_document() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: literal-org
";
        let document: Document = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(document.name, "demo");
        assert!(document.inputs.is_empty());
        assert!(document.outputs.is_empty());
        assert_eq!(document.steps.len(), 1);
        let step = &document.steps["a"];
        assert_eq!(step.tool.as_deref(), Some("naming.v1"));
        assert!(step.uses.is_none());
        assert!(matches!(&step.with["org"], WithValue::Scalar(value) if value == "literal-org"));
        assert!(step.for_each.is_none());
    }

    #[test]
    fn input_default_scalar_deserializes_as_string_variant() {
        let yaml = "\
name: demo
inputs:
  visibility: { type: RepoVisibility, default: private }
steps:
  a: { tool: naming.v1, with: {} }
";
        let document: Document = serde_yaml_ng::from_str(yaml).unwrap();
        let decl = &document.inputs["visibility"];
        assert_eq!(decl.ty, "RepoVisibility");
        assert!(matches!(decl.default, Some(DefaultValue::String(ref s)) if s == "private"));
    }

    #[test]
    fn input_default_list_deserializes_as_list_variant() {
        let yaml = "\
name: demo
inputs:
  environments: { type: list<EnvironmentSlug>, default: [dev, stg, prd] }
steps:
  a: { tool: naming.v1, with: {} }
";
        let document: Document = serde_yaml_ng::from_str(yaml).unwrap();
        let decl = &document.inputs["environments"];
        assert!(
            matches!(decl.default, Some(DefaultValue::List(ref items)) if items == &["dev", "stg", "prd"])
        );
    }

    #[test]
    fn duplicate_input_key_is_rejected() {
        let yaml = "\
name: demo
inputs:
  slug: { type: ProjectSlug }
  slug: { type: ProjectSlug }
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(err.to_string().contains("duplicate key"), "{err}");
    }

    #[test]
    fn duplicate_step_key_is_rejected() {
        let yaml = "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
  a: { tool: naming.v1, with: {} }
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(err.to_string().contains("duplicate key"), "{err}");
        assert!(err.location().is_some());
    }

    #[test]
    fn duplicate_with_key_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: one
      org: two
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(err.to_string().contains("duplicate key"), "{err}");
    }

    #[test]
    fn duplicate_output_key_is_rejected() {
        let yaml = "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
outputs:
  x: literal
  x: other
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(err.to_string().contains("duplicate key"), "{err}");
    }

    /// Milestone 3g, decision (a): a `with:` value that is a YAML sequence
    /// now parses, as [`WithValue::List`] -- one element per binding, each
    /// a reference or a literal.
    #[test]
    fn with_value_that_is_a_list_parses_as_a_list_of_scalars() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: [one, '${{ inputs.two }}']
";
        let document: Document = serde_yaml_ng::from_str(yaml).unwrap();
        let step = &document.steps["a"];
        assert!(matches!(
            &step.with["org"],
            WithValue::List(items) if items == &["one".to_string(), "${{ inputs.two }}".to_string()]
        ));
    }

    #[test]
    fn with_value_that_is_an_empty_list_parses_as_an_empty_list() {
        let yaml = "\
name: demo
steps:
  a: { tool: naming.v1, with: { org: [] } }
";
        let document: Document = serde_yaml_ng::from_str(yaml).unwrap();
        let step = &document.steps["a"];
        assert!(matches!(&step.with["org"], WithValue::List(items) if items.is_empty()));
    }

    /// A sequence element that is itself a sequence is rejected -- no
    /// nested lists (decision (a)).
    #[test]
    fn with_list_element_that_is_a_nested_sequence_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: [one, [two, three]]
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(
            err.to_string()
                .contains("values must be strings or references"),
            "{err}"
        );
    }

    /// A sequence element that is a mapping is rejected the same way.
    #[test]
    fn with_list_element_that_is_a_mapping_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: [one, { nested: true }]
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(
            err.to_string()
                .contains("values must be strings or references"),
            "{err}"
        );
    }

    #[test]
    fn with_value_that_is_a_map_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: { nested: true }
";
        let err = serde_yaml_ng::from_str::<Document>(yaml).unwrap_err();
        assert!(
            err.to_string()
                .contains("values must be strings or references"),
            "{err}"
        );
    }

    /// Decision (a): `for_each:` never accepts a sequence -- its field
    /// type stays a plain `String`, so a YAML sequence there fails to
    /// deserialize exactly as before milestone 3g.
    #[test]
    fn for_each_that_is_a_sequence_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    for_each: [one, two]
    with: {}
";
        assert!(serde_yaml_ng::from_str::<Document>(yaml).is_err());
    }

    /// Decision (a): `outputs:` never accepts a sequence either, for the
    /// same reason.
    #[test]
    fn output_value_that_is_a_sequence_is_rejected() {
        let yaml = "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
outputs:
  x: [one, two]
";
        assert!(serde_yaml_ng::from_str::<Document>(yaml).is_err());
    }

    #[test]
    fn document_schema_generates_without_panicking() {
        let schema = schemars::schema_for!(Document);
        let json = serde_json::to_value(&schema).unwrap();
        assert_eq!(json["type"], "object");
    }
}
