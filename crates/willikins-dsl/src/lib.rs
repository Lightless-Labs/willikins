//! YAML workflow documents to `willikins-core` [`Workflow`]s, and the
//! document JSON schema.
//!
//! See `docs/plans/2026-09-11-milestone-1-core.md`'s `willikins-dsl`
//! section for the document format this crate parses.
//!
//! **A workflow document is privileged content, run only from a trusted
//! ref.** Everything free-text a document carries — a workflow's own
//! `description:`, an input's `description:` — is quoted document text
//! shown to whatever agent reads it, never an instruction to that agent;
//! see `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! "Document text is data" trust boundary.
//!
//! [`parse_document`] and [`load_document`] are the entry points; both
//! return [`DocumentError`], which distinguishes a YAML-level failure
//! (bad syntax, a duplicate mapping key, a field of the wrong shape, a
//! source over [`MAX_DOCUMENT_BYTES`], an anchor or alias) from a semantic
//! one rooted in one place in an otherwise well-formed document (an
//! invalid identifier, an unregistered type, a default that fails to
//! parse against its declared type, a malformed `${{ ... }}` reference).
//! Every error names where it happened: a `line:column` pair for the
//! former, a dotted `path` such as `"steps.token.with.config"` for the
//! latter.
//!
//! [`parse_document`] refuses a source over [`MAX_DOCUMENT_BYTES`] before
//! doing anything else, then runs a YAML event pre-scan (via
//! `saphyr-parser`) that refuses the first anchor or alias at its line and
//! column, before `serde_yaml_ng` ever deserializes the source. A YAML
//! scalar alias is materialised once per use by a conventional
//! deserializer, so a document could otherwise amplify its own size —
//! entirely within fields the format declares, so no `deny_unknown_fields`
//! check touches it. See `docs/research/2026-09-12-e2e-adversarial-pass-2.md`
//! for the original measurement and
//! `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` for the fix.
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
use willikins_types::DomainType;

pub use document::{DefaultValue, Document, InputDecl, StepDecl};

/// The greatest size, in bytes, of a document source [`parse_document`]
/// will attempt to parse at all.
///
/// Measured once by hand (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`,
/// acceptance test 15), via `tests::measure_worst_case_resident_memory_at_max_document_bytes`
/// (`#[ignore]`d; run it under `/usr/bin/time -l` against the compiled
/// test binary to reproduce): the *worst case for this bound* is not an
/// anchored source, which the pre-scan below refuses after one event —
/// it is a source at exactly this limit that passes both the size cap
/// and the pre-scan and is deserialized in full (here, a 256 KiB
/// `description:` plain scalar, refused only afterwards by
/// `Description`'s own 1,024-character bound). That run's "maximum
/// resident set size" was 6,422,528 bytes (about 6.1 MiB); the same
/// binary running one trivial test instead peaked at 2,998,272 bytes
/// (about 2.9 MiB), so the parse itself accounts for 3,424,256 bytes
/// (about 3.3 MiB) — thirteen times the 256 KiB source, a small bounded
/// multiple rather than the quadratic blow-up an unbounded anchor/alias
/// amplification produces. Both numbers are whole-process peaks, harness
/// and allocator baseline included; the difference between them is the
/// closest this measurement gets to isolating `parse_document`.
/// Re-measured 2026-09-13 and unchanged.
pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024;

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
    /// The source was larger than [`MAX_DOCUMENT_BYTES`]. Refused before
    /// any YAML parsing is attempted, so this is the one [`DocumentError`]
    /// variant that never comes with a location.
    TooLarge {
        /// The source's actual size, in bytes.
        bytes: usize,
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

    /// Build a [`DocumentErrorKind::Yaml`] error at a known `line`/`column`,
    /// for the anchor/alias pre-scan, which never goes through
    /// `serde_yaml_ng` and so never has a `serde_yaml_ng::Error` to build
    /// [`Self::from_yaml`] from.
    fn yaml_at(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            kind: DocumentErrorKind::Yaml {
                message: message.into(),
                line: Some(line),
                column: Some(column),
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
            DocumentErrorKind::TooLarge { bytes } => write!(
                f,
                "document is {bytes} bytes, the limit is {MAX_DOCUMENT_BYTES}"
            ),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Parse a workflow document from YAML source.
///
/// # Errors
///
/// Returns [`DocumentErrorKind::TooLarge`] when `source` is over
/// [`MAX_DOCUMENT_BYTES`]; a [`DocumentErrorKind::Yaml`] naming a leading
/// byte-order mark, or naming an anchor's or alias's line and column when
/// the source has one (see the module docs); a [`DocumentErrorKind::Yaml`] when `source` is otherwise not
/// valid YAML or does not match [`Document`]'s shape (including a
/// duplicate mapping key or a `with` value that is a YAML list or map);
/// or a [`DocumentErrorKind::Semantic`] when a name, type, default value,
/// or `${{ ... }}` reference inside it fails to resolve. Both of the first
/// two checks run before any deserialization is attempted, so neither ever
/// constructs a [`Document`] from a source that fails them.
pub fn parse_document(source: &str) -> Result<Workflow, DocumentError> {
    if source.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError {
            kind: DocumentErrorKind::TooLarge {
                bytes: source.len(),
            },
        });
    }
    if source.starts_with('\u{FEFF}') {
        // `serde_yaml_ng` reads a leading BOM as a document separator, so
        // the source becomes a two-document stream and the failure it
        // reports is about whichever field landed in the wrong half --
        // never about the mark itself. Saying which character it is is
        // the whole value of this check; it is not a safety one.
        return Err(DocumentError::yaml_at(
            "a byte-order mark is not supported",
            1,
            1,
        ));
    }
    refuse_anchors_and_aliases(source)?;
    let document: Document =
        serde_yaml_ng::from_str(source).map_err(|err| DocumentError::from_yaml(&err))?;
    document_to_workflow(&document)
}

/// Walk `source`'s YAML event stream and refuse the first anchor
/// definition or alias reference found, naming its line and column.
///
/// This runs before `serde_yaml_ng` deserializes anything. A conventional
/// deserializer materialises a YAML alias's target once per use, so a
/// small document can amplify its own size by repeating an alias to a
/// large anchor — see the module docs and
/// `docs/research/2026-09-12-e2e-adversarial-pass-2.md`. Refusing at the
/// event level, before any `Document` is built, means the amplification
/// never happens at all rather than being merely bounded.
///
/// A `saphyr_parser::ScanError` here is reported as a `DocumentError` too,
/// with `saphyr_parser`'s own message and location, rather than swallowed
/// to let `serde_yaml_ng` have the only say: this pre-scan is a security
/// boundary (a workflow document reaches this crate over the network once
/// the MCP server exists), and failing open on anything `saphyr_parser`
/// itself cannot make sense of would let a source crafted to trip *only*
/// `saphyr_parser`'s scanner -- and not `serde_yaml_ng`'s -- skip the
/// anchor/alias check entirely and reach the deserializer regardless. A
/// document that is simply malformed YAML still ends up a
/// [`DocumentErrorKind::Yaml`] either way, just attributed to whichever
/// parser saw it first; no currently-passing test pins the *exact*
/// message of a low-level syntax error, only that it carries a location.
fn refuse_anchors_and_aliases(source: &str) -> Result<(), DocumentError> {
    for event in saphyr_parser::Parser::new_from_str(source) {
        let (event, span) = event.map_err(|err| {
            DocumentError::yaml_at(err.info(), err.marker().line(), column_of(err.marker()))
        })?;
        if is_anchored_or_aliased(&event) {
            return Err(DocumentError::yaml_at(
                "anchors and aliases are not supported",
                span.start.line(),
                column_of(&span.start),
            ));
        }
    }
    Ok(())
}

/// `marker`'s column, converted to the 1-indexed column
/// [`DocumentErrorKind::Yaml`] documents and `serde_yaml_ng` produces.
///
/// `saphyr_parser::Marker` documents its column as 1-indexed but counts
/// from zero: its scanner starts each line at `col: 0`
/// (`saphyr-parser-0.0.12/src/scanner.rs`), so an anchor at the very start
/// of a line reports column `0` — a value the documented contract cannot
/// express. Every location this crate hands out must mean the same thing
/// whichever parser produced it, so the pre-scan's columns are shifted
/// here rather than at each call site.
fn column_of(marker: &saphyr_parser::Marker) -> usize {
    marker.col() + 1
}

/// Whether `event` defines or uses a YAML anchor.
///
/// `saphyr_parser`'s anchor id `0` means "this event carries no anchor":
/// `Parser`'s `anchor_id_count` starts at `1` and only advances when an
/// anchor is actually registered (`saphyr-parser-0.0.12/src/parser.rs`,
/// `Parser::new` and `Parser::register_anchor`), so `!= 0` on a `Scalar`,
/// `SequenceStart`, or `MappingStart` is exactly "this event defines an
/// anchor". Every `Alias` refers to one by construction, so it is always
/// refused regardless of its id.
fn is_anchored_or_aliased(event: &saphyr_parser::Event<'_>) -> bool {
    match event {
        saphyr_parser::Event::Scalar(_, _, anchor_id, _)
        | saphyr_parser::Event::SequenceStart(anchor_id, _)
        | saphyr_parser::Event::MappingStart(anchor_id, _) => *anchor_id != 0,
        saphyr_parser::Event::Alias(_) => true,
        _ => false,
    }
}

/// Load and parse a workflow document from a file.
///
/// The file's size is judged before its contents are: a file the
/// filesystem already reports as larger than [`MAX_DOCUMENT_BYTES`] is
/// refused as [`DocumentErrorKind::TooLarge`] without being opened, and
/// the read that follows is itself bounded at one byte past the cap, so
/// nothing the filesystem misreports its size for — a FIFO, a character
/// device, a file that grows between the two calls — can make this
/// function allocate more than the cap either. [`parse_document`]'s own
/// cap would otherwise be the second bound on a read that had no first
/// one.
///
/// # Errors
///
/// Returns [`DocumentErrorKind::TooLarge`] when the file is larger than
/// [`MAX_DOCUMENT_BYTES`], a [`DocumentErrorKind::Semantic`] at `path`
/// when the file cannot be read or is not UTF-8, or any error
/// [`parse_document`] would return.
pub fn load_document(path: &Path) -> Result<Workflow, DocumentError> {
    use std::io::Read;

    let read_error = |err: &dyn fmt::Display| {
        DocumentError::semantic(
            path.display().to_string(),
            format!("failed to read document: {err}"),
        )
    };
    let too_large = |bytes: usize| DocumentError {
        kind: DocumentErrorKind::TooLarge { bytes },
    };

    // For a regular file this is the exact size, which is what
    // `TooLarge` promises to report; for anything it under-reports, the
    // bounded read below is the backstop.
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() > MAX_DOCUMENT_BYTES as u64
    {
        return Err(too_large(
            usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        ));
    }

    let file = std::fs::File::open(path).map_err(|err| read_error(&err))?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| read_error(&err))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(too_large(bytes.len()));
    }

    let source = String::from_utf8(bytes).map_err(|err| read_error(&err))?;
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
    let name = willikins_types::WorkflowName::parse(&document.name)
        .map_err(|err| DocumentError::semantic("name", err.reason))?;
    let mut workflow = Workflow::new(name.to_string());
    if let Some(description) = &document.description {
        let description = willikins_types::Description::parse(description)
            .map_err(|err| DocumentError::semantic("description", err.reason))?;
        workflow = workflow.with_description(description.to_string());
    }

    for (raw_name, decl) in &document.inputs {
        let path = format!("inputs.{raw_name}");
        let name = InputName::parse(raw_name)
            .map_err(|err| DocumentError::semantic(path.clone(), err.to_string()))?;
        let ty = parse_input_type(&decl.ty, &format!("{path}.type"))?;
        let mut spec = InputSpec::new(ty.clone());
        if let Some(description) = &decl.description {
            let description = willikins_types::Description::parse(description).map_err(|err| {
                DocumentError::semantic(format!("{path}.description"), err.reason)
            })?;
            spec = spec.with_description(description.to_string());
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
            DocumentErrorKind::Yaml { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a semantic error")
            }
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
            DocumentErrorKind::Yaml { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a semantic error")
            }
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
            DocumentErrorKind::Yaml { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a semantic error")
            }
        }
    }

    #[test]
    fn secret_typed_default_is_refused_without_echoing_the_value() {
        let err = parse_document(
            "\
name: demo
inputs:
  token: { type: DopplerServiceToken, default: dp.st.prd.exampleexampleexampleexampleexampleexample }
steps:
  a: { tool: naming.v1, with: {} }
",
        )
        .unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "inputs.token.default");
                assert!(
                    !message.contains("exampleexampleexampleexampleexampleexample"),
                    "{message}"
                );
            }
            DocumentErrorKind::Yaml { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a semantic error")
            }
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
            DocumentErrorKind::Yaml { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a semantic error")
            }
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
            DocumentErrorKind::TooLarge { bytes } => {
                panic!("expected a semantic error, got TooLarge {{ bytes: {bytes} }}")
            }
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
            DocumentErrorKind::Semantic { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a YAML error")
            }
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
            DocumentErrorKind::Semantic { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a YAML error")
            }
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
            DocumentErrorKind::Semantic { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a YAML error")
            }
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

    /// The published schema is the only description of this format an
    /// agent authoring a document ever sees, so it must not refuse what
    /// [`parse_document`] accepts. `description:` with no value is a
    /// document with no description — every caller treats it as absent —
    /// and the schema said its type was `"string"` while also declaring
    /// its default to be `null`, a pair no value can satisfy.
    #[test]
    fn the_published_schema_admits_every_description_the_parser_does() {
        let schema = serde_json::to_value(document_schema()).unwrap();
        for path in [
            &["properties", "description"][..],
            &["$defs", "InputDecl", "properties", "description"][..],
        ] {
            let mut node = &schema;
            for segment in path {
                node = &node[segment];
            }
            assert_eq!(
                node["type"],
                serde_json::json!(["string", "null"]),
                "{path:?} does not admit the null the parser accepts: {node}"
            );
        }

        // The parser's half of the same claim.
        let source = "\
name: demo
description:
inputs:
  org: { type: GitHubOrg, description: }
steps:
  a: { tool: naming.v1, with: {} }
";
        parse_document(source).expect("a null description parses");
    }

    /// `$schema` declares the dialect of a schema *document*; a subschema
    /// under `properties` is not one, and a nested declaration there
    /// either reads as noise or, to a validator that honours it, opens a
    /// new schema resource in the middle of this one. A field's schema
    /// has to be generated as a subschema, not lifted from a standalone
    /// `schema_for!`.
    #[test]
    fn no_subschema_redeclares_the_dialect_or_renames_a_field() {
        fn walk(node: &serde_json::Value, path: &str, found: &mut Vec<String>) {
            if let Some(object) = node.as_object() {
                if !path.is_empty() && object.contains_key("$schema") {
                    found.push(path.to_string());
                }
                for (key, value) in object {
                    walk(value, &format!("{path}.{key}"), found);
                }
            }
        }

        let schema = serde_json::to_value(document_schema()).unwrap();
        let mut found = Vec::new();
        walk(&schema, "", &mut found);
        assert!(found.is_empty(), "nested `$schema` at {found:?}");
    }

    // -------------------------------------------------------------
    // MAX_DOCUMENT_BYTES (acceptance test 15)
    // -------------------------------------------------------------

    #[test]
    fn a_document_at_exactly_max_document_bytes_is_not_too_large() {
        // Not valid YAML, but `TooLarge` is a byte-count check that runs
        // before any parsing is attempted, so its content never matters.
        let source = "a".repeat(MAX_DOCUMENT_BYTES);
        let err = parse_document(&source).unwrap_err();
        assert!(
            !matches!(err.kind, DocumentErrorKind::TooLarge { .. }),
            "a source at exactly the limit must not be TooLarge: {err:?}"
        );
    }

    /// A leading U+FEFF used to reach `serde_yaml_ng`, which treats it as
    /// a document separator: the source became a two-document stream and
    /// the report named a missing `steps` field — a message about the one
    /// part of the document that was plainly there, sending whoever read
    /// it to look in the wrong place. An editor that writes a BOM is the
    /// only way this happens, and the fix is to say so.
    #[test]
    fn a_leading_byte_order_mark_is_refused_by_name() {
        let source = "\
\u{FEFF}name: demo
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = parse_document(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => {
                assert_eq!(message, "a byte-order mark is not supported");
                assert_eq!(line, Some(1));
                assert_eq!(column, Some(1));
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    /// U+FEFF anywhere else is `serde_yaml_ng`'s business (it is a
    /// zero-width no-break space there, not a mark), and where it lands
    /// in text this format types, `Description` refuses it.
    #[test]
    fn a_byte_order_mark_inside_a_description_is_refused_as_an_invisible() {
        let source = "\
name: demo
description: \"a\u{FEFF}b\"
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = parse_document(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "description");
                assert!(message.contains("invisible"), "{message}");
            }
            other => panic!("expected a Semantic error, got {other:?}"),
        }
    }

    #[test]
    fn a_257_kib_document_is_too_large_before_any_parsing_is_attempted() {
        let source = "a".repeat(257 * 1024);
        let err = parse_document(&source).unwrap_err();
        match err.kind {
            DocumentErrorKind::TooLarge { bytes } => assert_eq!(bytes, 257 * 1024),
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    /// The cap is a byte count, not a character count: a source that is
    /// comfortably inside 256 KiB *characters* can be well over 256 KiB
    /// of memory, and memory is what the bound is about. The two numbers
    /// only disagree when the source is not ASCII, so the test says so
    /// in multibyte characters or it says nothing.
    #[test]
    fn the_cap_counts_bytes_not_characters() {
        let prefix = "name: demo\ndescription: ";
        let suffix = "\nsteps:\n  a: { tool: naming.v1, with: {} }\n";
        let filler_bytes = MAX_DOCUMENT_BYTES - prefix.len() - suffix.len();
        assert_eq!(
            filler_bytes % 3,
            0,
            "the filler must divide into 3-byte characters"
        );

        // Exactly at the cap: about a third as many characters as bytes,
        // and not refused by the cap.
        let at_cap = format!("{prefix}{}{suffix}", "\u{4E00}".repeat(filler_bytes / 3));
        assert_eq!(at_cap.len(), MAX_DOCUMENT_BYTES);
        assert!(at_cap.chars().count() < MAX_DOCUMENT_BYTES / 2);
        let err = parse_document(&at_cap).unwrap_err();
        assert!(
            !matches!(err.kind, DocumentErrorKind::TooLarge { .. }),
            "a source at exactly the limit must not be TooLarge: {err:?}"
        );

        // One character further: three bytes over, still nowhere near
        // the cap in characters, and refused.
        let over = format!(
            "{prefix}{}{suffix}",
            "\u{4E00}".repeat(filler_bytes / 3 + 1)
        );
        assert!(over.chars().count() < MAX_DOCUMENT_BYTES / 2);
        match parse_document(&over).unwrap_err().kind {
            DocumentErrorKind::TooLarge { bytes } => {
                assert_eq!(bytes, MAX_DOCUMENT_BYTES + 3);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    /// Nesting depth is the one dimension a byte cap does not obviously
    /// bound: 10,000 nested flow sequences cost two bytes each, so the
    /// source is tiny while the structure is not. Neither parser recurses
    /// on the stack for it — `saphyr_parser` counts flow levels in a
    /// `u8` and refuses past 255 with a scan error the pre-scan then
    /// reports, and `serde_yaml_ng` carries its own depth budget of 128
    /// — so this is an ordinary refusal rather than the stack overflow
    /// it would be under a recursive-descent parser. A stack overflow
    /// aborts the process, which no `Result` can express and no caller
    /// can catch, so it is worth a test that would die rather than fail.
    #[test]
    fn deeply_nested_flow_collections_are_refused_rather_than_overflowing_the_stack() {
        for depth in [100_usize, 255, 256, 10_000] {
            let nested = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
            let source = format!(
                "\
name: demo
inputs:
  a: {{ type: list<Text>, default: {nested} }}
steps: {{}}
"
            );
            assert!(source.len() < MAX_DOCUMENT_BYTES);
            let err = parse_document(&source).unwrap_err();
            assert!(
                matches!(err.kind, DocumentErrorKind::Yaml { .. }),
                "depth {depth}: expected a Yaml error, got {err:?}"
            );
        }
    }

    /// The block-style half of the same claim. Block nesting costs two
    /// characters of indentation per level *per line*, so the deepest
    /// nesting that fits inside the cap is quadratically shallower than
    /// the flow-style one — the cap bounds depth here on its own — and
    /// what is left still has to be refused rather than recursed.
    #[test]
    fn deeply_nested_block_collections_inside_the_cap_do_not_overflow_the_stack() {
        let depth = 400;
        let mut nested = String::new();
        for level in 0..depth {
            nested.push_str(&" ".repeat(level * 2));
            nested.push_str("- \n");
        }
        let source = format!(
            "\
name: demo
inputs:
  a:
    type: list<Text>
    default:
{nested}
steps: {{}}
"
        );
        assert!(source.len() < MAX_DOCUMENT_BYTES, "{}", source.len());
        let err = parse_document(&source).unwrap_err();
        assert!(
            matches!(err.kind, DocumentErrorKind::Yaml { .. }),
            "expected a Yaml error, got {err:?}"
        );
    }

    /// Not run by the gates: builds the worst case the module doc's
    /// resident-memory measurement is about (a source at exactly
    /// [`MAX_DOCUMENT_BYTES`] that passes the size cap and the pre-scan
    /// and reaches the deserializer in full, rather than being refused
    /// after one event the way an anchored source is). Run by hand under
    /// `/usr/bin/time -l` against the compiled test binary -- see the
    /// module doc for the recorded number.
    #[test]
    #[ignore = "run by hand under /usr/bin/time -l; see MAX_DOCUMENT_BYTES's doc"]
    fn measure_worst_case_resident_memory_at_max_document_bytes() {
        let prefix = "name: demo\ndescription: ";
        let suffix = "\nsteps:\n  a: { tool: naming.v1, with: {} }\n";
        let filler_len = MAX_DOCUMENT_BYTES - prefix.len() - suffix.len();
        let source = format!("{prefix}{}{suffix}", "a".repeat(filler_len));
        assert_eq!(source.len(), MAX_DOCUMENT_BYTES);
        let err = parse_document(&source).unwrap_err();
        match err.kind {
            // Reaching `Description`'s own bound (rather than `TooLarge`
            // or the pre-scan) is the proof this source made it all the
            // way through: past the size cap, past the pre-scan, through
            // `serde_yaml_ng`, into a real `Document`, into
            // `document_to_workflow`.
            DocumentErrorKind::Semantic { path, .. } => assert_eq!(path, "description"),
            other => panic!("expected a Semantic error at `description`, got {other:?}"),
        }
    }

    /// Write `contents` to a uniquely named file in this test binary's
    /// own temp directory and return its path.
    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("willikins-dsl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the temp directory");
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("failed to write the temp file");
        path
    }

    /// [`load_document`] used to hand the whole file to
    /// `std::fs::read_to_string` and only then let [`parse_document`]
    /// measure it, so a file of any size was fully resident before the
    /// cap was ever consulted — the cap bounded the parser but not the
    /// read that feeds it, which is the same claim one step earlier.
    ///
    /// Making the file unreadable is how a test states "the size was
    /// judged before the contents were": a reader that opens the file
    /// first cannot get past the permission error to the size, so it
    /// reports a failed read; one that asks the filesystem for the size
    /// first answers `TooLarge` without ever needing the bytes.
    // Unix-only: the premise is a file mode that denies reading, which
    // is how this test distinguishes "asked the filesystem for the size"
    // from "read the bytes and measured them". The behaviour it pins is
    // not platform-specific; the way of observing it is.
    #[cfg(unix)]
    #[test]
    fn load_document_refuses_an_oversized_file_without_reading_its_contents() {
        use std::os::unix::fs::PermissionsExt;

        let size = MAX_DOCUMENT_BYTES + 1;
        let path = temp_file("oversized-unreadable.yaml", &vec![b'a'; size]);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("failed to drop the file's permissions");
        if std::fs::File::open(&path).is_ok() {
            // Running as a user permissions do not apply to (root, or a
            // filesystem that ignores the mode); the premise is gone, so
            // the test would prove nothing either way.
            std::fs::remove_file(&path).ok();
            return;
        }

        let err = load_document(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        match err.kind {
            DocumentErrorKind::TooLarge { bytes } => assert_eq!(bytes, size),
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn load_document_still_parses_a_file_at_the_cap() {
        let prefix = "name: demo\ndescription: ";
        let suffix = "\nsteps:\n  a: { tool: naming.v1, with: {} }\n";
        let filler = MAX_DOCUMENT_BYTES - prefix.len() - suffix.len();
        let source = format!("{prefix}{}{suffix}", "a".repeat(filler));
        assert_eq!(source.len(), MAX_DOCUMENT_BYTES);
        let path = temp_file("at-the-cap.yaml", source.as_bytes());
        let err = load_document(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        // Refused by `Description`'s own bound, not by the size cap: the
        // file was read and parsed in full.
        match err.kind {
            DocumentErrorKind::Semantic { path, .. } => assert_eq!(path, "description"),
            other => panic!("expected a Semantic error at `description`, got {other:?}"),
        }
    }

    // -------------------------------------------------------------
    // The anchor/alias pre-scan: `saphyr_parser`'s anchor id 0 means
    // "this event carries no anchor" (`anchor_id_count` starts at 1 and
    // only advances when an anchor is actually registered -- see
    // `saphyr-parser-0.0.12/src/parser.rs`), so `!= 0` is exactly "this
    // event defines or uses an anchor".
    // -------------------------------------------------------------

    /// Pins `saphyr_parser`'s anchor id `0` meaning "no anchor" directly
    /// against synthetic events, independent of whether the full parser
    /// pipeline can ever actually produce every one of these
    /// combinations (an `Alias` in particular is only ever emitted after
    /// its anchor was already registered, so its own id argument does not
    /// matter -- it is always refused).
    #[test]
    fn is_anchored_or_aliased_settles_the_anchor_id_zero_question() {
        use saphyr_parser::{Event, ScalarStyle};

        assert!(!is_anchored_or_aliased(&Event::Scalar(
            "x".into(),
            ScalarStyle::Plain,
            0,
            None
        )));
        assert!(is_anchored_or_aliased(&Event::Scalar(
            "x".into(),
            ScalarStyle::Plain,
            1,
            None
        )));
        assert!(!is_anchored_or_aliased(&Event::SequenceStart(0, None)));
        assert!(is_anchored_or_aliased(&Event::SequenceStart(1, None)));
        assert!(!is_anchored_or_aliased(&Event::MappingStart(0, None)));
        assert!(is_anchored_or_aliased(&Event::MappingStart(1, None)));
        // An alias is refused regardless of the anchor id it names.
        assert!(is_anchored_or_aliased(&Event::Alias(1)));
        // Structural events never carry an anchor.
        assert!(!is_anchored_or_aliased(&Event::StreamStart));
        assert!(!is_anchored_or_aliased(&Event::DocumentStart(false)));
    }

    #[test]
    fn pre_scan_accepts_a_plain_document_with_no_anchors() {
        let source = "\
name: demo
steps:
  a: { tool: naming.v1, with: {} }
";
        assert!(refuse_anchors_and_aliases(source).is_ok());
    }

    #[test]
    fn pre_scan_refuses_an_anchored_scalar_at_its_location() {
        let source = "\
name: demo
description: &s a
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => {
                assert_eq!(message, "anchors and aliases are not supported");
                assert!(line.is_some());
                assert!(column.is_some());
            }
            DocumentErrorKind::Semantic { .. } | DocumentErrorKind::TooLarge { .. } => {
                panic!("expected a Yaml error")
            }
        }
    }

    /// `DocumentErrorKind::Yaml`'s `line` and `column` are documented as
    /// 1-indexed, which is what `serde_yaml_ng` produces. `saphyr_parser`'s
    /// `Marker` documents its column as 1-indexed too but actually counts
    /// from zero (its scanner starts a line at `col: 0`), so the pre-scan
    /// has to add one or it reports a column one to the left of the token
    /// — and `column: Some(0)`, which the documented contract cannot
    /// express at all.
    #[test]
    fn pre_scan_locations_are_one_indexed_like_serde_yaml_ngs() {
        // "description: &s a": the anchored scalar `a` is the 17th
        // character of line 2.
        let source = "\
name: demo
description: &s a
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { line, column, .. } => {
                assert_eq!(line, Some(2));
                assert_eq!(column, Some(17));
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }

        // An anchor on an implicit empty scalar: the scalar's span starts
        // at the first column of the following line, which is column 1,
        // never column 0.
        let empty = "\
name: demo
description: &x
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(empty).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { line, column, .. } => {
                assert_eq!(line, Some(3));
                assert_eq!(column, Some(1));
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    /// Pins that the pre-scan's fail-closed `ScanError` path reports its
    /// location on the same 1-indexed footing as its anchor refusal, so a
    /// reader never has to know which of the two parsers spoke.
    #[test]
    fn a_scan_error_from_the_pre_scan_is_also_one_indexed() {
        // A tab where a plain scalar's indentation must be: only
        // `saphyr_parser`'s scanner is consulted.
        let source = "name: demo\nsteps:\tx: y\n";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { line, column, .. } => {
                assert_eq!(line, Some(2));
                assert!(
                    column.is_some_and(|column| column >= 1),
                    "a 1-indexed column is never 0: {column:?}"
                );
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    #[test]
    fn pre_scan_refuses_an_anchored_mapping_at_its_location() {
        let source = "\
name: demo
steps:
  a: &s { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { message, .. } => {
                assert_eq!(message, "anchors and aliases are not supported");
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    #[test]
    fn pre_scan_refuses_an_anchored_sequence_at_its_location() {
        let source = "\
name: demo
inputs:
  environments:
    type: list<EnvironmentSlug>
    default: &s [dev, stg]
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { message, .. } => {
                assert_eq!(message, "anchors and aliases are not supported");
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    #[test]
    fn pre_scan_refuses_an_alias() {
        // The anchor definition comes first in document order, so the
        // pre-scan trips on it before it ever reaches the alias -- proof
        // that no anchored value can survive long enough to be aliased.
        let source = "\
name: demo
inputs:
  a: { type: Text, default: &s x }
  b: { type: Text, default: *s }
steps:
  a: { tool: naming.v1, with: {} }
";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { message, .. } => {
                assert_eq!(message, "anchors and aliases are not supported");
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    /// Every syntactic shape an anchor or an alias can take, pinned in
    /// one place. The pre-scan works on the event stream rather than on
    /// the source text, so none of these is a separate code path — which
    /// is exactly the claim worth pinning, since a text-level check
    /// would need a case for each and would miss at least one.
    #[test]
    fn pre_scan_refuses_every_shape_an_anchor_or_alias_can_take() {
        let cases = [
            // An alias used as a flow mapping's value.
            (
                "alias in a flow mapping",
                "\
name: demo
inputs: { a: { type: Text, default: &s q }, b: { type: Text, default: *s } }
steps: {}
",
            ),
            // An alias used as a flow sequence's item.
            (
                "alias in a flow sequence",
                "\
name: demo
inputs:
  a: { type: Text, default: &s q }
  b: { type: list<Text>, default: [*s] }
steps: {}
",
            ),
            // An anchor on a mapping *key* rather than on a value.
            (
                "anchor on a mapping key",
                "\
name: demo
&k description: hi
steps: {}
",
            ),
            // An anchor on an explicitly empty scalar.
            (
                "anchor on an empty quoted scalar",
                "\
name: demo
description: &s \"\"
steps: {}
",
            ),
            // An anchor on an *implicit* empty scalar: the anchored node
            // is the nothing between the colon and the next line.
            (
                "anchor on an implicit empty scalar",
                "\
name: demo
description: &s
steps: {}
",
            ),
            // A merge key. `serde` never applies one to a struct, so
            // before the pre-scan this was caught only by
            // `deny_unknown_fields` refusing the `<<` key itself; it is
            // now refused for carrying an alias at all, which holds even
            // in a world where `<<` were a field this format declares.
            (
                "a merge key",
                "\
name: demo
inputs:
  a: &base { type: Text }
  b: { <<: *base }
steps: {}
",
            ),
            // A tag on an anchored node: the tag is not what is refused,
            // the anchor riding along with it is.
            (
                "a tag on an anchored node",
                "\
name: demo
description: !!str &s hello
steps: {}
",
            ),
        ];

        for (label, source) in cases {
            let Err(err) = refuse_anchors_and_aliases(source) else {
                panic!("{label}: was accepted");
            };
            match err.kind {
                DocumentErrorKind::Yaml {
                    message,
                    line,
                    column,
                } => {
                    assert_eq!(message, "anchors and aliases are not supported", "{label}");
                    assert!(line.is_some_and(|line| line >= 1), "{label}: {line:?}");
                    assert!(column.is_some_and(|col| col >= 1), "{label}: {column:?}");
                }
                other => panic!("{label}: expected a Yaml error, got {other:?}"),
            }
        }
    }

    /// A tag on its own is not an anchor and is left to `serde_yaml_ng`,
    /// so the pre-scan is not quietly refusing every document that uses
    /// YAML's type syntax.
    #[test]
    fn pre_scan_does_not_refuse_a_bare_tag() {
        assert!(refuse_anchors_and_aliases("name: !!str demo\nsteps: {}\n").is_ok());
    }

    /// A source whose anchors sit *after* a point `saphyr_parser`'s own
    /// scanner cannot get past: the pre-scan's fail-closed path reports
    /// that scan error rather than reaching the anchor, and in
    /// particular never panics or silently returns `Ok`.
    #[test]
    fn a_yaml_error_before_an_anchor_surfaces_as_an_error_not_a_panic() {
        let source = "name: demo\n\tbad: [\ndescription: &s hi\n";
        let err = refuse_anchors_and_aliases(source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml { line, column, .. } => {
                assert!(line.is_some());
                assert!(column.is_some_and(|col| col >= 1));
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    /// Acceptance test 15: the milestone 1 pass 2 amplification shape (a
    /// large anchored scalar referenced many times) is refused by the
    /// pre-scan alone, called directly rather than through
    /// [`parse_document`] -- a document this large is already over
    /// [`MAX_DOCUMENT_BYTES`] and would be refused as `TooLarge` first,
    /// which is a *different* guarantee (tested above) from the one this
    /// test pins: that the pre-scan mechanism itself refuses this exact
    /// historic attack shape, naming a location, and never so much as
    /// looks at `Document` to do it -- no `Document` is even in scope in
    /// this test.
    #[test]
    fn acceptance_15_the_pass_2_amplification_document_is_refused_by_the_pre_scan() {
        let anchor = "z".repeat(1_000_000);
        let aliases = vec!["*s"; 2_000].join(", ");
        let source = format!(
            "\
name: alias-amplification
description: &s \"{anchor}\"
inputs:
  t:
    type: list<Text>
    default: [{aliases}]
steps:
  a: {{ tool: naming.v1, with: {{}} }}
"
        );
        let err = refuse_anchors_and_aliases(&source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Yaml {
                message,
                line,
                column,
            } => {
                assert_eq!(message, "anchors and aliases are not supported");
                // The anchor definition is on line 2; the pre-scan must
                // trip there, well before any of the 2,000 aliases.
                assert_eq!(line, Some(2));
                assert!(column.is_some());
            }
            other => panic!("expected a Yaml error, got {other:?}"),
        }
    }

    // -------------------------------------------------------------
    // WorkflowName / Description bounds (acceptance test 14, bounds half)
    // -------------------------------------------------------------

    #[test]
    fn a_name_over_64_characters_is_refused_at_parse_with_a_bounded_message() {
        let source = format!(
            "\
name: {}
steps:
  a: {{ tool: naming.v1, with: {{}} }}
",
            "a".repeat(65)
        );
        let err = parse_document(&source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "name");
                assert!(!message.contains(&"a".repeat(65)), "{message}");
                assert!(message.len() < 100, "message was {} bytes", message.len());
            }
            other => panic!("expected a Semantic error, got {other:?}"),
        }
    }

    #[test]
    fn a_description_over_1024_characters_is_refused_at_parse_with_a_bounded_message() {
        let too_long = "a".repeat(1_025);
        let source = format!(
            "\
name: demo
description: {too_long}
steps:
  a: {{ tool: naming.v1, with: {{}} }}
"
        );
        let err = parse_document(&source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "description");
                assert!(!message.contains(&too_long), "{message}");
                assert!(message.len() < 100, "message was {} bytes", message.len());
            }
            other => panic!("expected a Semantic error, got {other:?}"),
        }
    }

    #[test]
    fn an_input_description_over_1024_characters_is_refused_at_parse_with_a_bounded_message() {
        let too_long = "a".repeat(1_025);
        let source = format!(
            "\
name: demo
inputs:
  org: {{ type: GitHubOrg, description: {too_long} }}
steps:
  a: {{ tool: naming.v1, with: {{}} }}
"
        );
        let err = parse_document(&source).unwrap_err();
        match err.kind {
            DocumentErrorKind::Semantic { path, message } => {
                assert_eq!(path, "inputs.org.description");
                assert!(!message.contains(&too_long), "{message}");
                assert!(message.len() < 100, "message was {} bytes", message.len());
            }
            other => panic!("expected a Semantic error, got {other:?}"),
        }
    }

    /// A `description:` is a single line, and the DSL does not trim a
    /// block scalar's trailing newline before handing the text to
    /// [`willikins_types::Description`]. Trimming would put the DSL and
    /// the type into disagreement about the same string — the type would
    /// refuse text the format quietly rewrote into something it accepts —
    /// and a document is privileged content whose words are quoted to an
    /// agent verbatim, not edited on the way past. `|` and `>` keep a
    /// final newline by definition, so both are refused, and the message
    /// names the character; `|-` and `>-` strip it and are accepted. A
    /// block scalar with a newline *inside* it is refused whichever
    /// chomping indicator it carries, which is the "single line" rule
    /// itself rather than a quirk of trailing newlines.
    #[test]
    fn a_block_scalar_description_keeps_its_newlines_and_is_refused_for_them() {
        let refused = [
            ("| keeps a trailing newline", "|\n  a line"),
            ("> keeps a trailing newline", ">\n  a line"),
            ("|- still has an inner newline", "|-\n  first\n  second"),
        ];
        for (label, scalar) in refused {
            let source = format!(
                "\
name: demo
description: {scalar}
steps:
  a: {{ tool: naming.v1, with: {{}} }}
"
            );
            let err = parse_document(&source).map(|_| ()).expect_err(label);
            match err.kind {
                DocumentErrorKind::Semantic { path, message } => {
                    assert_eq!(path, "description", "{label}");
                    assert!(message.contains("control character"), "{label}: {message}");
                }
                other => panic!("{label}: expected a Semantic error, got {other:?}"),
            }
        }

        let accepted = [
            ("|- strips the trailing newline", "|-\n  a line"),
            (">- strips the trailing newline", ">-\n  a line"),
        ];
        for (label, scalar) in accepted {
            let source = format!(
                "\
name: demo
description: {scalar}
steps:
  a: {{ tool: naming.v1, with: {{}} }}
"
            );
            let json = workflow_json(&source);
            assert_eq!(json["description"], "a line", "{label}");
        }
    }

    #[test]
    fn a_valid_name_and_description_convert_via_to_string_into_the_core_workflow() {
        // `Workflow::name`/`description` stay `String` in this task (task
        // 1e flips them); the DSL validates through `WorkflowName` and
        // `Description` and converts with `to_string()`.
        let json = workflow_json(
            "\
name: new-rust-service
description: Provision a Rust service.
steps:
  a: { tool: naming.v1, with: {} }
",
        );
        assert_eq!(json["name"], "new-rust-service");
        assert_eq!(json["description"], "Provision a Rust service.");
    }
}
