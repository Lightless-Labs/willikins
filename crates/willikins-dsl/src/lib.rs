//! YAML workflow documents to `willikins-core` [`Workflow`]s, and the
//! document JSON schema.
//!
//! See `docs/plans/2026-09-11-milestone-1-core.md`'s `willikins-dsl`
//! section for the document format this crate parses.
//!
//! [`parse_document`] and [`load_document`] are the entry points; both
//! return [`DocumentError`], which distinguishes a YAML-level failure
//! (bad syntax, a duplicate mapping key, a field of the wrong shape) from
//! a semantic one rooted in one place in an otherwise well-formed document
//! (an invalid identifier, an unregistered type, a default that fails to
//! parse against its declared type, a malformed `${{ ... }}` reference).
//! Every error names where it happened: a `line:column` pair for the
//! former, a dotted `path` such as `"steps.token.with.config"` for the
//! latter.
//!
//! This crate is also where an input's declared type is checked against
//! the type registry. `willikins_core::check` does not do this itself —
//! see that crate's `check` module docs — because a workflow built any
//! other way (the builder API, a future composite) has no such string to
//! validate in the first place; the DSL is the one caller that does, so
//! it is the one caller responsible for rejecting an unregistered name
//! before a [`Workflow`] is ever built from it.

pub mod document;
mod reference;

use std::fmt;
use std::path::Path;

use willikins_core::{
    Binding, InputName, InputSpec, Node, NodeName, OutputName, PortName, ToolName, TypeRef, Value,
    Workflow,
};

pub use document::{DefaultValue, Document, InputDecl, StepDecl};

/// Where a [`DocumentError`] happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind")]
pub enum DocumentErrorKind {
    /// A YAML-level failure: malformed syntax, a duplicate mapping key
    /// (see [`document`]'s module docs), a field of the wrong shape, or a
    /// missing required field.
    Yaml {
        /// The underlying message. Any trailing `at line L column C`
        /// `serde_yaml_ng` appends to its own errors is stripped, since
        /// that information lives in `line`/`column` instead.
        message: String,
        /// The 1-indexed line the error was found at, when
        /// `serde_yaml_ng` attaches a location to it.
        line: Option<usize>,
        /// The 1-indexed column the error was found at, when
        /// `serde_yaml_ng` attaches a location to it.
        column: Option<usize>,
    },
    /// A document error rooted in one place in an otherwise well-formed
    /// document: an invalid identifier, an unregistered type, a default
    /// that fails to parse, a malformed reference.
    Semantic {
        /// A dotted path to the offending value, such as
        /// `"steps.token.with.config"` or `"inputs.slug.default"`.
        path: String,
        /// Why it was rejected.
        message: String,
    },
}

/// Everything that can go wrong loading or parsing a workflow document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DocumentError {
    /// Where and why this document failed to parse.
    #[serde(flatten)]
    pub kind: DocumentErrorKind,
}

impl DocumentError {
    /// Build a [`DocumentErrorKind::Semantic`] error at `path`.
    fn semantic(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: DocumentErrorKind::Semantic {
                path: path.into(),
                message: message.into(),
            },
        }
    }

    /// Build a [`DocumentErrorKind::Yaml`] error from a `serde_yaml_ng`
    /// failure, pulling out its location when it has one and stripping
    /// the matching `" at line L column C"` suffix from the message.
    fn from_yaml(err: &serde_yaml_ng::Error) -> Self {
        let location = err.location();
        let mut message = err.to_string();
        if let Some(location) = &location {
            let suffix = format!(" at line {} column {}", location.line(), location.column());
            if let Some(stripped) = message.strip_suffix(&suffix) {
                message = stripped.to_string();
            }
        }
        Self {
            kind: DocumentErrorKind::Yaml {
                message,
                line: location.as_ref().map(serde_yaml_ng::Location::line),
                column: location.as_ref().map(serde_yaml_ng::Location::column),
            },
        }
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DocumentErrorKind::Yaml {
                message,
                line: Some(line),
                column: Some(column),
            } => write!(f, "{line}:{column}: {message}"),
            DocumentErrorKind::Yaml { message, .. } => write!(f, "{message}"),
            DocumentErrorKind::Semantic { path, message } => write!(f, "{path}: {message}"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Parse a workflow document from YAML source.
///
/// # Errors
///
/// Returns [`DocumentError`] when `source` is not valid YAML, when it
/// does not match [`Document`]'s shape (including a duplicate mapping key
/// or a `with` value that is a YAML list or map), or when a name, type,
/// default value, or `${{ ... }}` reference inside it fails to resolve.
pub fn parse_document(source: &str) -> Result<Workflow, DocumentError> {
    let document: Document =
        serde_yaml_ng::from_str(source).map_err(|err| DocumentError::from_yaml(&err))?;
    document_to_workflow(&document)
}

/// Load and parse a workflow document from a file.
///
/// # Errors
///
/// Returns [`DocumentError`] when `path` cannot be read, or for any
/// reason [`parse_document`] would.
pub fn load_document(path: &Path) -> Result<Workflow, DocumentError> {
    let source = std::fs::read_to_string(path).map_err(|err| {
        DocumentError::semantic(
            path.display().to_string(),
            format!("failed to read document: {err}"),
        )
    })?;
    parse_document(&source)
}

/// The document format's published JSON schema, generated from
/// [`Document`].
#[must_use]
pub fn document_schema() -> schemars::Schema {
    schemars::schema_for!(Document)
}

/// Convert a deserialized [`Document`] into a [`Workflow`], resolving
/// every name, type, default, and reference along the way.
fn document_to_workflow(document: &Document) -> Result<Workflow, DocumentError> {
    let mut workflow = Workflow::new(document.name.clone());
    if let Some(description) = &document.description {
        workflow = workflow.with_description(description.clone());
    }

    for (raw_name, decl) in &document.inputs {
        let path = format!("inputs.{raw_name}");
        let name = InputName::parse(raw_name)
            .map_err(|err| DocumentError::semantic(path.clone(), err.to_string()))?;
        let ty = parse_input_type(&decl.ty, &format!("{path}.type"))?;
        let mut spec = InputSpec::new(ty.clone());
        if let Some(description) = &decl.description {
            spec = spec.with_description(description.clone());
        }
        if let Some(default) = &decl.default {
            let value = parse_default(&ty, default)
                .map_err(|err| DocumentError::semantic(format!("{path}.default"), err.reason))?;
            spec = spec.with_default(value);
        }
        workflow = workflow.input(name, spec);
    }

    for (raw_name, decl) in &document.steps {
        let path = format!("steps.{raw_name}");
        let name = NodeName::parse(raw_name)
            .map_err(|err| DocumentError::semantic(path.clone(), err.to_string()))?;
        let tool = ToolName::parse(&decl.tool)
            .map_err(|err| DocumentError::semantic(format!("{path}.tool"), err.to_string()))?;
        let mut node = Node::new(tool);

        if let Some(raw) = &decl.for_each {
            let binding = reference::parse_for_each_value(raw)
                .map_err(|message| DocumentError::semantic(format!("{path}.for_each"), message))?;
            node = node.for_each(binding);
        }

        for (raw_port, raw_value) in &decl.with {
            let port_path = format!("{path}.with.{raw_port}");
            let port = PortName::parse(raw_port)
                .map_err(|err| DocumentError::semantic(port_path.clone(), err.to_string()))?;
            let binding = parse_reference_value(raw_value, &port_path)?;
            node = node.port(port, binding);
        }

        workflow = workflow.node(name, node);
    }

    for (raw_name, raw_value) in &document.outputs {
        let path = format!("outputs.{raw_name}");
        let name = OutputName::parse(raw_name)
            .map_err(|err| DocumentError::semantic(path.clone(), err.to_string()))?;
        let binding = parse_reference_value(raw_value, &path)?;
        workflow = workflow.output(name, binding);
    }

    Ok(workflow)
}

/// Parse one `with` or output value into a [`Binding`], reporting a
/// malformed `${{ ... }}` reference as a [`DocumentError::semantic`] at
/// `path`.
fn parse_reference_value(raw_value: &str, path: &str) -> Result<Binding, DocumentError> {
    match reference::parse_with_value(raw_value)
        .map_err(|message| DocumentError::semantic(path, message))?
    {
        reference::Parsed::Literal(text) => Ok(Binding::Literal(text)),
        reference::Parsed::Binding(binding) => Ok(binding),
    }
}

/// Parse an input's declared type string, checking both that it is
/// well-formed (`TypeRef::parse`) and that it names a type the registry
/// actually has — a check `willikins_core::check` does not perform itself
/// (see the module docs).
fn parse_input_type(raw: &str, path: &str) -> Result<TypeRef, DocumentError> {
    let ty = TypeRef::parse(raw).map_err(|err| DocumentError::semantic(path, err.to_string()))?;
    if willikins_types::registry().get(&ty.name).is_none() {
        return Err(DocumentError::semantic(
            path,
            format!("unknown type `{}`", ty.name),
        ));
    }
    Ok(ty)
}

/// Parse a declared default value against its input's resolved type.
fn parse_default(
    ty: &TypeRef,
    default: &DefaultValue,
) -> Result<Value, willikins_types::ParseError> {
    match (ty.list, default) {
        (false, DefaultValue::String(scalar)) => Value::parse(ty, scalar),
        (true, DefaultValue::List(items)) => {
            let refs: Vec<&str> = items.iter().map(String::as_str).collect();
            Value::parse_list(ty, &refs)
        }
        (false, DefaultValue::List(_)) => Err(willikins_types::ParseError::new(
            "Document",
            format!("default for scalar type `{ty}` must be a single string, not a list"),
        )),
        (true, DefaultValue::String(_)) => Err(willikins_types::ParseError::new(
            "Document",
            format!("default for list type `{ty}` must be a list of strings"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow_json(source: &str) -> serde_json::Value {
        serde_json::to_value(parse_document(source).unwrap()).unwrap()
    }

    #[test]
    fn parses_a_minimal_workflow() {
        let json = workflow_json(
            "\
name: demo
description: A demo workflow.
inputs:
  org: { type: GitHubOrg, description: The org }
steps:
  names:
    tool: naming.v1
    with:
      org: ${{ inputs.org }}
      slug: literal-slug
outputs:
  org_out: ${{ inputs.org }}
",
        );
        assert_eq!(json["name"], "demo");
        assert_eq!(json["description"], "A demo workflow.");
        assert_eq!(json["inputs"]["org"]["ty"], "GitHubOrg");
        assert_eq!(json["nodes"]["names"]["tool"], "naming.v1");
        assert_eq!(json["nodes"]["names"]["with"]["org"]["kind"], "input");
        assert_eq!(json["nodes"]["names"]["with"]["slug"]["kind"], "literal");
        assert_eq!(
            json["nodes"]["names"]["with"]["slug"]["value"],
            "literal-slug"
        );
        assert_eq!(json["outputs"]["org_out"]["kind"], "input");
    }

    #[test]
    fn for_each_and_item_and_keyed_reference_convert() {
        let json = workflow_json(
            "\
name: demo
inputs:
  environments: { type: list<EnvironmentSlug>, default: [dev, stg, prd] }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with:
      project: literal-project
      environment: ${{ item }}
  token:
    tool: doppler.service_token.ensure
    with:
      config: ${{ steps.configs[prd].config }}
      name: ci
",
        );
        assert_eq!(json["nodes"]["configs"]["for_each"]["kind"], "input");
        assert_eq!(
            json["nodes"]["configs"]["with"]["environment"]["kind"],
            "item"
        );
        assert_eq!(json["nodes"]["token"]["with"]["config"]["kind"], "keyed");
        assert_eq!(
            json["nodes"]["token"]["with"]["config"]["value"]["key"],
            "prd"
        );
    }

    #[test]
    fn invalid_input_name_is_a_semantic_error_at_the_input_path() {
        let err = parse_document(
            "\
name: demo
inputs:
  Bad: { type: GitHubOrg }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, .. } => assert_eq!(path, "inputs.Bad"),
            DocumentErrorKind::Yaml { .. } => panic!("expected a semantic error"),
        }
    }

    #[test]
    fn unknown_type_string_is_a_semantic_error_at_the_type_path() {
        let err = parse_document(
            "\
name: demo
inputs:
  x: { type: Bogus }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "inputs.x.type");
                assert!(message.contains("Bogus"), "{message}");
            }
            DocumentErrorKind::Yaml { .. } => panic!("expected a semantic error"),
        }
    }

    #[test]
    fn unparseable_type_string_is_a_semantic_error() {
        let err = parse_document(
            "\
name: demo
inputs:
  x: { type: not_pascal }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        assert!(matches!(err.kind, DocumentErrorKind::Semantic { .. }));
    }

    #[test]
    fn default_that_fails_to_parse_is_a_semantic_error_at_the_default_path() {
        let err = parse_document(
            "\
name: demo
inputs:
  visibility: { type: RepoVisibility, default: internal }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, .. } => {
                assert_eq!(path, "inputs.visibility.default");
            }
            DocumentErrorKind::Yaml { .. } => panic!("expected a semantic error"),
        }
    }

    #[test]
    fn secret_typed_default_is_refused_without_echoing_the_value() {
        let err = parse_document(
            "\
name: demo
inputs:
  token: { type: DopplerServiceToken, default: dp.st.prd.exampleexampleexample }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "inputs.token.default");
                assert!(!message.contains("exampleexampleexample"), "{message}");
            }
            DocumentErrorKind::Yaml { .. } => panic!("expected a semantic error"),
        }
    }

    #[test]
    fn bad_reference_is_a_semantic_error_naming_the_step_and_port() {
        let err = parse_document(
            "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: ${{ org.x }}
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "steps.a.with.org");
                assert!(message.contains("org.x"), "{message}");
            }
            DocumentErrorKind::Yaml { .. } => panic!("expected a semantic error"),
        }
    }

    fn semantic_path(source: &str) -> String {
        match parse_document(source).unwrap_err().kind {
            DocumentErrorKind::Semantic { path, .. } => path,
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => panic!(
                "expected a semantic error, got a YAML error at {line:?}:{column:?}: {message}"
            ),
        }
    }

    #[test]
    fn invalid_step_name_is_a_semantic_error_at_the_step_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  Bad: { tool: naming.v1, with: {} }
"
            ),
            "steps.Bad"
        );
    }

    #[test]
    fn invalid_tool_name_is_a_semantic_error_at_the_tool_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a: { tool: Not.Valid, with: {} }
"
            ),
            "steps.a.tool"
        );
    }

    #[test]
    fn invalid_port_name_is_a_semantic_error_at_the_with_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      Bad: literal
"
            ),
            "steps.a.with.Bad"
        );
    }

    #[test]
    fn invalid_output_name_is_a_semantic_error_at_the_output_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
outputs:
  Bad: literal
"
            ),
            "outputs.Bad"
        );
    }

    #[test]
    fn bad_reference_in_an_output_is_a_semantic_error_at_the_output_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
outputs:
  out: ${{ steps.a }}
"
            ),
            "outputs.out"
        );
    }

    #[test]
    fn literal_for_each_is_a_semantic_error_at_the_for_each_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a:
    tool: naming.v1
    for_each: dev
    with: {}
"
            ),
            "steps.a.for_each"
        );
    }

    #[test]
    fn malformed_for_each_reference_is_a_semantic_error_at_the_for_each_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
steps:
  a:
    tool: naming.v1
    for_each: ${{ steps.a }}
    with: {}
"
            ),
            "steps.a.for_each"
        );
    }

    #[test]
    fn list_default_for_a_scalar_type_is_a_semantic_error_at_the_default_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
inputs:
  org: { type: GitHubOrg, default: [a] }
steps:
  a: { tool: naming.v1, with: {} }
"
            ),
            "inputs.org.default"
        );
    }

    #[test]
    fn scalar_default_for_a_list_type_is_a_semantic_error_at_the_default_path() {
        assert_eq!(
            semantic_path(
                "\
name: demo
inputs:
  environments: { type: list<EnvironmentSlug>, default: dev }
steps:
  a: { tool: naming.v1, with: {} }
"
            ),
            "inputs.environments.default"
        );
    }

    #[test]
    fn with_value_that_is_a_list_is_a_yaml_error_with_a_location() {
        let err = parse_document(
            "\
name: demo
steps:
  a:
    tool: naming.v1
    with:
      org: [one, two]
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => {
                assert!(
                    message.contains("values must be strings or references"),
                    "{message}"
                );
                assert!(line.is_some());
                assert!(column.is_some());
            }
            DocumentErrorKind::Semantic { .. } => panic!("expected a YAML error"),
        }
    }

    #[test]
    fn duplicate_step_key_is_a_yaml_error_with_a_location() {
        let err = parse_document(
            "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => {
                assert!(message.contains("duplicate key"), "{message}");
                assert!(line.is_some());
                assert!(column.is_some());
            }
            DocumentErrorKind::Semantic { .. } => panic!("expected a YAML error"),
        }
    }

    #[test]
    fn yaml_syntax_error_carries_line_and_column() {
        let err = parse_document(
            "\
name: demo
steps:
  a:
    tool: naming.v1
    with: [unterminated
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { line, column, .. } => {
                assert!(line.is_some());
                assert!(column.is_some());
            }
            DocumentErrorKind::Semantic { .. } => panic!("expected a YAML error"),
        }
    }

    #[test]
    fn missing_required_field_is_a_yaml_error() {
        let err = parse_document("name: demo\n").unwrap_err();
        assert!(matches!(err.kind, DocumentErrorKind::Yaml { .. }));
    }

    #[test]
    fn document_error_display_matches_the_documented_shape() {
        let semantic = DocumentError::semantic("inputs.slug.default", "boom");
        assert_eq!(semantic.to_string(), "inputs.slug.default: boom");

        let yaml = DocumentError {
            kind: DocumentErrorKind::Yaml {
                message: "boom".to_string(),
                line: Some(2),
                column: Some(3),
            },
        };
        assert_eq!(yaml.to_string(), "2:3: boom");
    }

    #[test]
    fn document_error_serializes_with_an_adjacent_kind_tag() {
        let semantic = DocumentError::semantic("inputs.slug.default", "boom");
        let json = serde_json::to_value(&semantic).unwrap();
        assert_eq!(json["kind"], "Semantic");
        assert_eq!(json["path"], "inputs.slug.default");
        assert_eq!(json["message"], "boom");
    }

    #[test]
    fn load_document_reports_an_unreadable_file() {
        let err = load_document(Path::new("/no/such/file.yaml")).unwrap_err();
        assert!(matches!(err.kind, DocumentErrorKind::Semantic { .. }));
    }

    #[test]
    fn document_schema_snapshot() {
        let schema = document_schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();
        insta::assert_snapshot!(json);
    }
}
