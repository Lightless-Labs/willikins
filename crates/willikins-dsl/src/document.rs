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
use willikins_types::DomainType;

/// The published schema for [`Document::name`]: [`willikins_types::WorkflowName`]'s
/// own schema (pattern and length), even though the field is stored as a
/// plain `String` here. `serde` never validates it: `Document::name` is
/// checked by parsing it as a `WorkflowName` in
/// `willikins_dsl::document_to_workflow`, the same way every other typed
/// field in this format is checked, so a bad name is a located
/// [`crate::DocumentErrorKind::Semantic`] rather than a `serde` error.
fn workflow_name_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    willikins_types::WorkflowName::json_schema()
}

/// The published schema for a document's free-text `description:` field
/// (on [`Document`] and [`InputDecl`]): [`willikins_types::Description`]'s
/// own schema. See [`workflow_name_schema`] for why the Rust field type
/// stays `Option<String>`.
fn description_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    willikins_types::Description::json_schema()
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

/// One declared workflow node (a `steps.<name>` entry).
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepDecl {
    /// The tool this step calls, such as `github.repo.ensure`.
    pub tool: String,
    /// When set, this step runs once per item of the bound list. Must be
    /// a reference (the same grammar as a `with` value), never a literal.
    #[serde(default)]
    pub for_each: Option<String>,
    /// This step's input port bindings, keyed by port name. Each value is
    /// a reference or a literal string.
    #[serde(default, deserialize_with = "deserialize_with_map")]
    pub with: IndexMap<String, String>,
}

/// A `with` map value: like a plain `String`, except a YAML sequence or
/// mapping is rejected with a message naming what a `with` value may
/// actually be, rather than serde's generic "invalid type" wording.
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
fn deserialize_with_map<'de, D>(deserializer: D) -> Result<IndexMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: IndexMap<String, WithString> = deserialize_unique_map(deserializer)?;
    Ok(map.into_iter().map(|(k, v)| (k, v.0)).collect())
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
        assert_eq!(step.tool, "naming.v1");
        assert_eq!(step.with["org"], "literal-org");
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

    #[test]
    fn with_value_that_is_a_list_is_rejected() {
        let yaml = "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: [one, two]
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

    #[test]
    fn document_schema_generates_without_panicking() {
        let schema = schemars::schema_for!(Document);
        let json = serde_json::to_value(&schema).unwrap();
        assert_eq!(json["type"], "object");
    }
}
