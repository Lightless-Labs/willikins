//! Resolve a workflow's inputs against partially supplied raw values, with
//! no provider calls: [`describe`] tells an agent what is still missing,
//! what was rejected, and what fully resolved already.
//!
//! [`describe`] never touches a [`crate::Catalog`] or a [`crate::Tool`] —
//! its signature does not even accept one — so it is structurally
//! incapable of calling a provider, unlike [`crate::plan::plan`].

use indexmap::IndexMap;

use crate::check::Checked;
use crate::value::{TypeRef, Value};
use crate::workflow::{InputName, InputSpec};
use willikins_types::ParseError;

/// One raw input value, before being checked against its workflow's
/// declared type. `check` already fixed each input's cardinality (scalar
/// or `list<T>`), so a raw value whose own shape disagrees — a
/// [`Self::Scalar`] handed to a list-typed input or vice versa — is a
/// cardinality mismatch, reported the same way [`Value::parse`] and
/// [`Value::parse_list`] already report one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawInput {
    /// A single raw value.
    Scalar(String),
    /// A list of raw values, in order.
    List(Vec<String>),
}

impl RawInput {
    /// Split `s` on commas into a raw list value, trimming surrounding
    /// whitespace from each element and dropping empty elements — so an
    /// empty string yields an empty list rather than a list holding one
    /// empty element. Used once a caller (the CLI, task 11) already knows
    /// the input it is building this value for is list-typed; `describe`
    /// itself never calls this.
    #[must_use]
    pub fn from_comma_separated(s: &str) -> Self {
        Self::List(
            s.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(String::from)
                .collect(),
        )
    }
}

/// The raw values supplied for some subset of a workflow's declared inputs,
/// by name. An input this map has no entry for is either defaulted or
/// [`describe`]-reported as missing.
pub type PartialInputs = IndexMap<InputName, RawInput>;

/// One `name=value` command-line argument, split into an input name and its
/// raw text.
///
/// Implements [`std::str::FromStr`] so it can be handed straight to `clap`
/// as a value parser (milestone 1's CLI, task 11). Always yields a raw
/// *scalar* — a caller that already knows the named input is list-typed
/// builds a [`RawInput::from_comma_separated`] from [`Self::raw`] instead of
/// using [`Self::value`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputArg {
    /// The input name, to the left of `=`.
    pub name: InputName,
    /// The raw text, to the right of `=`, unsplit.
    pub raw: String,
}

impl InputArg {
    /// This argument's raw text as a [`RawInput::Scalar`].
    #[must_use]
    pub fn value(&self) -> RawInput {
        RawInput::Scalar(self.raw.clone())
    }
}

impl std::str::FromStr for InputArg {
    type Err = ParseError;

    /// Splits on the first `=`. Neither side is trimmed: a value with
    /// leading or trailing whitespace is passed through unchanged, since a
    /// domain type's own parser decides what whitespace it accepts.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `s` contains no `=`, or when the text
    /// before it is not a valid [`InputName`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name, raw) = s
            .split_once('=')
            .ok_or_else(|| ParseError::new("InputArg", format!("{s:?} is not `name=value`")))?;
        Ok(Self {
            name: InputName::parse(name)?,
            raw: raw.to_string(),
        })
    }
}

/// Why one raw input, or one unrecognised input name, was rejected.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InputError {
    /// The input name the raw value was supplied for — declared or not.
    pub input: InputName,
    /// Why it was rejected. For a secret-typed input this can only ever be
    /// the registry's cardinality-blind refusal, never an echo of the raw
    /// text: `describe` calls the same [`Value::parse`] / [`Value::parse_list`]
    /// that `check`'s literal handling does, and those never see a secret
    /// declared type in a successfully [`Checked`] workflow (`check`
    /// rejects one first).
    pub error: ParseError,
}

/// A declared input `describe` found neither a raw value nor a default for.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MissingInput {
    /// The missing input's name.
    pub name: InputName,
    /// Its declared type.
    pub ty: TypeRef,
    /// JSON schema for the type, from the registry's [`willikins_types::TypeInfo`].
    pub schema: schemars::Schema,
    /// The input's own one-line description, if it declared one.
    pub description: Option<String>,
    /// The input's default value, rendered — always `None` here: an input
    /// with a default is never missing (see [`describe`]). Kept as a field
    /// rather than dropped so `MissingInput` carries the same shape as an
    /// [`InputSpec`] would, in case a future revision reports an input as
    /// "missing but defaulted" for some other reason; a plan defect, not
    /// exercised by any test today.
    pub default: Option<String>,
    /// A valid example value for the type, from the registry.
    pub example: &'static str,
    /// A one-sentence question an agent could ask a human to fill this
    /// input in, built from the name, description, and example.
    pub prompt: String,
}

/// The result of resolving [`PartialInputs`] against a [`Checked`]
/// workflow's declared inputs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Description {
    /// Every rejected raw value or unrecognised input name, in the order
    /// documented on [`describe`].
    pub errors: Vec<InputError>,
    /// Every declared input with neither a raw value nor a default, in
    /// declaration order.
    pub missing: Vec<MissingInput>,
    /// Every input that resolved to a concrete [`Value`]: a parsed raw
    /// value, or a declared default. In declaration order.
    pub resolved: IndexMap<InputName, Value>,
}

/// Resolve `partial` against `checked`'s declared inputs. Makes no provider
/// call: it never touches a [`crate::Catalog`] or a [`crate::Tool`], so it
/// cannot even accidentally do so.
///
/// Walks `checked.workflow.inputs` in declaration order. For each declared
/// input:
/// - a raw value in `partial` is parsed against the input's declared type
///   with [`Value::parse`] (a [`RawInput::Scalar`]) or [`Value::parse_list`]
///   (a [`RawInput::List`]) — either function's own cardinality check
///   reports a [`RawInput`] of the wrong shape for the declared type, so no
///   separate check is needed here; a parse failure becomes an
///   [`InputError`] without ever echoing the raw text;
/// - otherwise, a declared default fills [`Description::resolved`];
/// - otherwise the input is [`MissingInput`].
///
/// After every declared input, any name in `partial` that is not one of
/// `checked.workflow.inputs` is an [`InputError`] of its own, in
/// `partial`'s own iteration order, appended after the declared-input
/// errors above.
#[must_use]
pub fn describe(checked: &Checked, partial: &PartialInputs) -> Description {
    let mut errors = Vec::new();
    let mut missing = Vec::new();
    let mut resolved = IndexMap::new();

    for (name, spec) in &checked.workflow.inputs {
        match partial.get(name) {
            Some(raw) => match parse_raw(&spec.ty, raw) {
                Ok(value) => {
                    resolved.insert(name.clone(), value);
                }
                Err(error) => errors.push(InputError {
                    input: name.clone(),
                    error,
                }),
            },
            None => match &spec.default {
                Some(default) => {
                    resolved.insert(name.clone(), default.clone());
                }
                None => missing.push(missing_input(name, spec)),
            },
        }
    }

    for name in partial.keys() {
        if !checked.workflow.inputs.contains_key(name) {
            errors.push(InputError {
                input: name.clone(),
                error: ParseError::new(
                    "Workflow",
                    format!("the workflow declares no such input `{name}`"),
                ),
            });
        }
    }

    Description {
        errors,
        missing,
        resolved,
    }
}

/// Parse one raw input against `ty`, dispatching on [`RawInput`]'s own
/// shape: [`Value::parse`] for a [`RawInput::Scalar`], [`Value::parse_list`]
/// for a [`RawInput::List`]. Either call's own cardinality check rejects a
/// shape that does not match `ty.list`, so a scalar raw value handed to a
/// list-typed input (or the reverse) surfaces as an ordinary [`ParseError`]
/// here, with no extra branch required.
fn parse_raw(ty: &TypeRef, raw: &RawInput) -> Result<Value, ParseError> {
    match raw {
        RawInput::Scalar(text) => Value::parse(ty, text),
        RawInput::List(items) => {
            let refs: Vec<&str> = items.iter().map(String::as_str).collect();
            Value::parse_list(ty, &refs)
        }
    }
}

/// Build the [`MissingInput`] entry for a declared input with neither a raw
/// value nor a default.
///
/// # Panics
///
/// Panics if `spec.ty`'s name is not in the global type registry — cannot
/// happen for an input inside a [`Checked`] workflow, since `check` rejects
/// [`crate::check::CheckError::UnregisteredInputType`] before a `Checked`
/// value can ever exist.
fn missing_input(name: &InputName, spec: &InputSpec) -> MissingInput {
    let entry = willikins_types::registry()
        .get(&spec.ty.name)
        .unwrap_or_else(|| unreachable!("`check` already rejected an unregistered input type"));
    let example = entry.info.example;
    let prompt = build_prompt(name, spec.description.as_deref(), example);
    MissingInput {
        name: name.clone(),
        ty: spec.ty.clone(),
        schema: entry.info.schema.clone(),
        description: spec.description.clone(),
        default: spec
            .default
            .as_ref()
            .map(|value| value.render().to_string()),
        example,
        prompt,
    }
}

/// Build a one-sentence question for a human to answer, from an input's
/// name, its own description if it has one, and a valid example value.
fn build_prompt(name: &InputName, description: Option<&str>, example: &str) -> String {
    match description {
        Some(description) => {
            format!("What should `{name}` be? {description} (for example, `{example}`).")
        }
        None => format!("What should `{name}` be? (for example, `{example}`)."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::check::check;
    use crate::tool::{Inputs, Observation, Outputs, Tool, ToolError};
    use crate::value::TypeName;
    use crate::workflow::{Node, NodeName, Workflow};
    use willikins_types::{DomainType, SinkToken};

    fn ty(name: &str) -> TypeRef {
        TypeRef::scalar(TypeName::parse(name).unwrap())
    }

    fn list_ty(name: &str) -> TypeRef {
        TypeRef::list_of(TypeName::parse(name).unwrap())
    }

    fn input_name(name: &str) -> InputName {
        InputName::parse(name).unwrap()
    }

    /// A tool with a normal spec (so `check`, which calls [`Tool::spec`] to
    /// validate and to check node ports, succeeds and produces a
    /// [`Checked`]) whose `read` and `ensure` both panic. `describe`'s own
    /// signature does not accept a [`Catalog`] at all, so it cannot call
    /// either regardless; this tool exists to make that structural fact
    /// into an executable check, by giving `describe` a `Checked` workflow
    /// with a node it would have to call `read` on if it ever tried.
    struct PanicsOnRead {
        spec: crate::tool::ToolSpec,
    }

    impl PanicsOnRead {
        fn new() -> Self {
            Self {
                spec: crate::tool::ToolSpec {
                    name: crate::tool::ToolName::parse("test.panics").unwrap(),
                    description: "A tool that panics if ever read or ensured.".to_string(),
                    inputs: IndexMap::new(),
                    outputs: IndexMap::new(),
                    key: Vec::new(),
                    class: crate::class::Class::Reversible,
                    pure: true,
                },
            }
        }
    }

    impl Tool for PanicsOnRead {
        fn spec(&self) -> &crate::tool::ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            panic!("describe must never call Tool::read")
        }

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
            panic!("describe must never call Tool::ensure")
        }
    }

    /// A `Checked` workflow with two required inputs (`slug`, `org`), an
    /// optional `visibility` with a default, and an optional `environments`
    /// list with a three-element default — the same input shape as the
    /// milestone's positive fixture, checked against an empty catalog since
    /// `describe` never consults tools.
    fn checked_positive_inputs() -> Checked {
        let workflow = Workflow::new("describe-fixture")
            .input(
                input_name("slug"),
                InputSpec::new(ty("ProjectSlug")).with_description("Canonical project slug"),
            )
            .input(
                input_name("org"),
                InputSpec::new(ty("GitHubOrg"))
                    .with_description("GitHub organization that owns the repository"),
            )
            .input(
                input_name("visibility"),
                InputSpec::new(ty("RepoVisibility"))
                    .with_default(Value::known(willikins_types::RepoVisibility::Private)),
            )
            .input(
                input_name("environments"),
                InputSpec::new(list_ty("EnvironmentSlug")).with_default(Value::known_list(vec![
                    willikins_types::EnvironmentSlug::parse("dev").unwrap(),
                    willikins_types::EnvironmentSlug::parse("stg").unwrap(),
                    willikins_types::EnvironmentSlug::parse("prd").unwrap(),
                ])),
            );
        let catalog = Catalog::new(willikins_types::registry());
        check(&workflow, &catalog).expect("no nodes: nothing to fail check")
    }

    #[test]
    fn describe_never_calls_a_tool() {
        // A node calling a tool that panics on `read`/`ensure`, checked
        // successfully against a catalog holding that tool. If `describe`
        // ever called either method, this test would panic rather than
        // pass; since `describe`'s signature does not even accept a
        // `Catalog`, this mostly pins that structural guarantee.
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(std::sync::Arc::new(PanicsOnRead::new()))
            .unwrap();
        let workflow = Workflow::new("w").node(
            NodeName::parse("boom").unwrap(),
            Node::new(crate::tool::ToolName::parse("test.panics").unwrap()),
        );
        let checked = check(&workflow, &catalog).expect("a pure, portless tool checks cleanly");
        let partial = PartialInputs::new();
        let description = describe(&checked, &partial);
        assert!(description.errors.is_empty());
        assert!(description.missing.is_empty());
    }

    #[test]
    fn acceptance_5_no_inputs_lists_slug_and_org_missing_with_prompts() {
        let checked = checked_positive_inputs();
        let partial = PartialInputs::new();
        let description = describe(&checked, &partial);

        assert!(description.errors.is_empty());
        let missing_names: Vec<&str> = description
            .missing
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(missing_names, vec!["slug", "org"]);
        for missing in &description.missing {
            assert!(!missing.prompt.is_empty());
            assert!(missing.prompt.contains(missing.name.as_str()));
            assert!(missing.default.is_none());
        }
        assert!(description.resolved.contains_key(&input_name("visibility")));
        assert!(
            description
                .resolved
                .contains_key(&input_name("environments"))
        );
    }

    #[test]
    fn acceptance_5_full_inputs_resolve_everything_including_the_default_list() {
        let checked = checked_positive_inputs();
        let mut partial = PartialInputs::new();
        partial.insert(
            input_name("slug"),
            RawInput::Scalar("third-thoughts".to_string()),
        );
        partial.insert(
            input_name("org"),
            RawInput::Scalar("lightless-labs".to_string()),
        );
        let description = describe(&checked, &partial);

        assert!(description.errors.is_empty(), "{:?}", description.errors);
        assert!(description.missing.is_empty(), "{:?}", description.missing);
        assert_eq!(description.resolved.len(), 4);
        let environments = description
            .resolved
            .get(&input_name("environments"))
            .unwrap();
        assert_eq!(environments.as_list().unwrap().len(), 3);
    }

    #[test]
    fn acceptance_5_an_unknown_input_name_is_an_input_error() {
        let checked = checked_positive_inputs();
        let mut partial = PartialInputs::new();
        partial.insert(
            input_name("bogus"),
            RawInput::Scalar("whatever".to_string()),
        );
        let description = describe(&checked, &partial);
        assert_eq!(description.errors.len(), 1);
        assert_eq!(description.errors[0].input, input_name("bogus"));
        assert!(description.errors[0].error.reason.contains("no such input"));
    }

    #[test]
    fn acceptance_5_a_bad_slug_is_an_input_error() {
        let checked = checked_positive_inputs();
        let mut partial = PartialInputs::new();
        partial.insert(
            input_name("slug"),
            RawInput::Scalar("Not A Valid Slug!".to_string()),
        );
        let description = describe(&checked, &partial);
        assert_eq!(description.errors.len(), 1);
        assert_eq!(description.errors[0].input, input_name("slug"));
    }

    #[test]
    fn a_scalar_raw_value_for_a_list_typed_input_is_a_cardinality_error() {
        let checked = checked_positive_inputs();
        let mut partial = PartialInputs::new();
        partial.insert(
            input_name("environments"),
            RawInput::Scalar("dev".to_string()),
        );
        let description = describe(&checked, &partial);
        assert_eq!(description.errors.len(), 1);
        assert_eq!(description.errors[0].input, input_name("environments"));
        assert!(description.errors[0].error.reason.contains("parse_list"));
    }

    #[test]
    fn a_list_raw_value_for_a_scalar_typed_input_is_a_cardinality_error() {
        let checked = checked_positive_inputs();
        let mut partial = PartialInputs::new();
        partial.insert(
            input_name("org"),
            RawInput::List(vec!["lightless-labs".to_string()]),
        );
        let description = describe(&checked, &partial);
        assert_eq!(description.errors.len(), 1);
        assert_eq!(description.errors[0].input, input_name("org"));
        assert!(description.errors[0].error.reason.contains("Value::parse"));
    }

    #[test]
    fn raw_input_from_comma_separated_splits_and_trims() {
        assert_eq!(
            RawInput::from_comma_separated("dev, stg,prd"),
            RawInput::List(vec![
                "dev".to_string(),
                "stg".to_string(),
                "prd".to_string()
            ])
        );
    }

    #[test]
    fn raw_input_from_comma_separated_of_empty_string_is_an_empty_list() {
        assert_eq!(RawInput::from_comma_separated(""), RawInput::List(vec![]));
    }

    #[test]
    fn input_arg_parses_name_and_value() {
        use std::str::FromStr;
        let arg = InputArg::from_str("org=lightless-labs").unwrap();
        assert_eq!(arg.name, input_name("org"));
        assert_eq!(arg.raw, "lightless-labs");
        assert_eq!(arg.value(), RawInput::Scalar("lightless-labs".to_string()));
    }

    #[test]
    fn input_arg_rejects_missing_equals() {
        use std::str::FromStr;
        assert!(InputArg::from_str("org").is_err());
    }

    #[test]
    fn input_arg_rejects_a_bad_input_name() {
        use std::str::FromStr;
        assert!(InputArg::from_str("Org=lightless-labs").is_err());
    }

    #[test]
    fn description_serializes_to_json() {
        let checked = checked_positive_inputs();
        let partial = PartialInputs::new();
        let description = describe(&checked, &partial);
        let json = serde_json::to_value(&description).unwrap();
        assert!(json["missing"].is_array());
        assert!(json["resolved"].is_object());
    }
}
