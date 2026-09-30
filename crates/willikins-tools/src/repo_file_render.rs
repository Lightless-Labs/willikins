//! `repo.file.render`: milestone 3g, decision (d). Pure: renders a
//! [`TemplateSource`] against up to 16 positional [`TemplateValue`]s into a
//! [`RepoFile`] at a document-chosen [`RepoPath`].
//!
//! This is deliberately not a template engine. The only construct it
//! understands is the placeholder `{{ N }}` -- exactly two opening braces,
//! one space, a decimal index `0`..`15`, one space, two closing braces --
//! substituted with the string of the `values` element at that index.
//! Everything else in the template, including a mistyped or foreign
//! placeholder syntax (`{{0}}`, `{% include %}`, `${HOME}`), is refused
//! rather than silently ignored or passed through: a document author who
//! gets a placeholder wrong learns that at `plan` time, not by reading the
//! committed file afterwards. See
//! `docs/plans/2026-09-30-milestone-3g-file-writing.md`, decision (d).

use std::collections::BTreeSet;

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, invalid, list, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{RepoFile, RepoPath, TemplateSource, TemplateValue};

/// The greatest placeholder index this tool understands (`{{ 15 }}`), and
/// so the greatest number of `values` a call may bind.
const MAX_INDEX: usize = 15;

/// The most values a single call may bind -- indices `0` through
/// [`MAX_INDEX`], inclusive.
const MAX_VALUES: usize = MAX_INDEX + 1;

/// The most characters a rendered file may hold: [`RepoFile`]'s own
/// content bound, so a render that cannot become a `RepoFile` is refused
/// before it is built rather than after. See [`render`].
const MAX_RENDERED_CHARS: usize = 65_536;

/// One valid `{{ N }}` occurrence found in a template: its byte range (so
/// the render pass can copy around it without re-scanning) and the index
/// it names.
struct Placeholder {
    /// The byte offset of this occurrence's opening `{{`.
    start: usize,
    /// The byte offset just past this occurrence's closing `}}`.
    end: usize,
    /// The decimal index between the braces, already known to be at most
    /// [`MAX_INDEX`].
    index: usize,
}

/// `repo.file.render`.
pub struct RepoFileRender {
    spec: ToolSpec,
}

impl RepoFileRender {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("path"), exact("RepoPath", true));
        inputs.insert(port("template"), exact("TemplateSource", true));
        inputs.insert(
            port("values"),
            PortSpec {
                ty: PortType::Exact(list("TemplateValue")),
                required: false,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("file"), scalar("RepoFile"));
        Self {
            spec: ToolSpec {
                name: tool_name("repo.file.render"),
                description:
                    "Render a template against up to 16 positional `{{ N }}` values into a file \
                     at a repository path."
                        .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    fn compute(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let path: RepoPath = get(inputs, "path")?;
        let template: TemplateSource = get(inputs, "template")?;
        let values = read_values(inputs)?;
        if values.len() > MAX_VALUES {
            return Err(invalid(format!(
                "at most {MAX_VALUES} values may be bound, but {} were",
                values.len()
            )));
        }
        let placeholders = scan_placeholders(template.as_str())?;
        check_indices_match_values(&placeholders, values.len())?;
        let rendered_text = render(template.as_str(), &placeholders, &values)?;
        let file = RepoFile::new(path, rendered_text)
            .map_err(|err| invalid(format!("rendered file is invalid: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("file"), Value::known(file));
        Ok(outputs)
    }
}

/// Read the optional `values` port as a list of [`TemplateValue`]s, or an
/// empty list when the port is not bound at all -- a template with no
/// placeholder needs none (several of Walter's files, such as the
/// `BUILD.bazel` name reservations, are static).
///
/// # Errors
///
/// Returns [`ToolError`] of kind `Invalid` when the port is bound but
/// [`Unknown`](willikins_core::ValueState::Unknown), or bound to a value
/// that is not a list of `TemplateValue` -- both mean `check` was bypassed,
/// since `plan`/`apply` only ever deliver a well-typed value here.
fn read_values(inputs: &Inputs) -> Result<Vec<TemplateValue>, ToolError> {
    let Some(value) = inputs.get(&port("values")) else {
        return Ok(Vec::new());
    };
    if !value.is_known() {
        return Err(invalid("port `values` is unknown"));
    }
    let items = value
        .as_list()
        .ok_or_else(|| invalid("port `values` has an unexpected type"))?;
    items
        .iter()
        .map(|object| {
            willikins_types::downcast::<TemplateValue>(object.as_ref())
                .cloned()
                .ok_or_else(|| invalid("port `values` has an unexpected type"))
        })
        .collect()
}

/// Scan `template` for every `{{`, one at a time, requiring each to open
/// exactly one valid placeholder: `{{`, one space, one or more ASCII
/// digits, one space, `}}`. Any `{{` that does not is refused as a whole
/// -- a mistyped or foreign placeholder syntax must never reach a
/// committed file silently. A well-formed placeholder whose index is over
/// [`MAX_INDEX`] is refused too, distinctly from a malformed one.
///
/// Never echoes the template's own text: every refusal names indices and
/// counts only.
///
/// # Errors
///
/// Returns [`ToolError`] of kind `Invalid` on the first `{{` that is not a
/// valid, in-range placeholder.
fn scan_placeholders(template: &str) -> Result<Vec<Placeholder>, ToolError> {
    let mut placeholders = Vec::new();
    let mut offset = 0usize;
    loop {
        let remaining = &template[offset..];
        let Some(relative) = remaining.find("{{") else {
            break;
        };
        let start = offset + relative;
        let after_open = &template[start + 2..];
        let malformed = || {
            invalid(
                "the template contains `{{` that is not a valid `{{ N }}` placeholder \
                 (exactly one space inside each brace, N a decimal index from 0 to 15)",
            )
        };
        let after_space1 = after_open.strip_prefix(' ').ok_or_else(malformed)?;
        let digit_count = after_space1
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        if digit_count == 0 {
            return Err(malformed());
        }
        let digits = &after_space1[..digit_count];
        let after_digits = &after_space1[digit_count..];
        let after_space2 = after_digits.strip_prefix(' ').ok_or_else(malformed)?;
        let after_close = after_space2.strip_prefix("}}").ok_or_else(malformed)?;
        // Parsed as `u64` rather than `usize` so an absurdly long digit run
        // (still bounded by the template's own 65,536-character limit)
        // cannot overflow; a value that does not fit is certainly over
        // `MAX_INDEX`.
        let index: u64 = digits.parse().unwrap_or(u64::MAX);
        if index > MAX_INDEX as u64 {
            return Err(invalid(format!(
                "the template contains a placeholder index outside the allowed range 0..={MAX_INDEX}"
            )));
        }
        #[allow(clippy::cast_possible_truncation)]
        let index = index as usize;
        let token_len = template.len() - after_close.len() - start;
        placeholders.push(Placeholder {
            start,
            end: start + token_len,
            index,
        });
        offset = start + token_len;
    }
    Ok(placeholders)
}

/// Every index referenced by a placeholder in `placeholders` must have a
/// bound value, and every bound value (`0..value_count`) must be
/// referenced by at least one placeholder -- decision (d)'s "an index with
/// no value" and "a value no placeholder uses" refusals. Duplicate
/// occurrences of the same index (substituted more than once) are fine.
///
/// # Errors
///
/// Returns [`ToolError`] of kind `Invalid` naming the first offending
/// index either way.
fn check_indices_match_values(
    placeholders: &[Placeholder],
    value_count: usize,
) -> Result<(), ToolError> {
    let referenced: BTreeSet<usize> = placeholders.iter().map(|p| p.index).collect();
    for &index in &referenced {
        if index >= value_count {
            return Err(invalid(format!(
                "the template uses placeholder index {index}, but only {value_count} value(s) are bound"
            )));
        }
    }
    for index in 0..value_count {
        if !referenced.contains(&index) {
            return Err(invalid(format!(
                "value at index {index} is bound, but no placeholder in the template uses it"
            )));
        }
    }
    Ok(())
}

/// Build the rendered text, refusing first -- by arithmetic, never by
/// building the string and measuring it afterwards -- a render that would
/// produce more than [`MAX_RENDERED_CHARS`] characters.
///
/// Mirrors `template.render`'s own `projected_length` (adversarial pass
/// 2): a document-authored template and caller-reachable values are each
/// individually bounded, but their combination is not, so the bound is
/// checked before any allocation proportional to the rendered size.
///
/// # Errors
///
/// Returns [`ToolError`] of kind `Invalid` when the projected length
/// exceeds [`MAX_RENDERED_CHARS`]. Never echoes the template or any
/// value's text.
fn render(
    template: &str,
    placeholders: &[Placeholder],
    values: &[TemplateValue],
) -> Result<String, ToolError> {
    let template_chars = template.chars().count();
    let mut consumed = 0usize;
    let mut substituted = 0usize;
    for placeholder in placeholders {
        // Every byte of a placeholder token (`{`, ` `, an ASCII digit,
        // `}`) is exactly one character, so its byte length is its char
        // length.
        consumed = consumed.saturating_add(placeholder.end - placeholder.start);
        substituted =
            substituted.saturating_add(values[placeholder.index].as_str().chars().count());
    }
    let projected = template_chars
        .saturating_sub(consumed)
        .saturating_add(substituted);
    if projected > MAX_RENDERED_CHARS {
        return Err(invalid(format!(
            "rendering this template would produce {projected} characters, more than the \
             {MAX_RENDERED_CHARS} a repository file may hold ({} placeholder(s))",
            placeholders.len()
        )));
    }
    let mut rendered = String::with_capacity(projected);
    let mut last = 0usize;
    for placeholder in placeholders {
        rendered.push_str(&template[last..placeholder.start]);
        rendered.push_str(values[placeholder.index].as_str());
        last = placeholder.end;
    }
    rendered.push_str(&template[last..]);
    Ok(rendered)
}

impl Default for RepoFileRender {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for RepoFileRender {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.compute(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::DomainType;

    fn full_inputs(path: &str, template: &str, values: &[&str]) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("path").unwrap(),
            Value::known(RepoPath::parse(path).unwrap()),
        );
        inputs.insert(
            PortName::parse("template").unwrap(),
            Value::known(TemplateSource::parse(template).unwrap()),
        );
        if !values.is_empty() {
            let parsed: Vec<TemplateValue> = values
                .iter()
                .map(|v| TemplateValue::parse(v).unwrap())
                .collect();
            inputs.insert(
                PortName::parse("values").unwrap(),
                Value::known_list(parsed),
            );
        }
        inputs
    }

    fn rendered_content(observation: Observation) -> String {
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let file = outputs.get(&PortName::parse("file").unwrap()).unwrap();
        let file: &RepoFile = file.downcast().unwrap();
        file.content().to_string()
    }

    #[test]
    fn spec_validates_against_the_registry() {
        RepoFileRender::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn read_substitutes_every_occurrence_of_each_index() {
        let inputs = full_inputs(
            "apps/walter/ios/BUILD.bazel",
            "app: {{ 0 }}, nse: {{ 1 }}, widgets: {{ 2 }}, app again: {{ 0 }}",
            &[
                "com.example.app",
                "com.example.app.nse",
                "com.example.app.widgets",
            ],
        );
        let observation = RepoFileRender::new().read(&inputs).unwrap();
        assert_eq!(
            rendered_content(observation),
            "app: com.example.app, nse: com.example.app.nse, widgets: com.example.app.widgets, \
             app again: com.example.app"
        );
    }

    #[test]
    fn read_with_no_placeholders_and_no_values_renders_the_template_verbatim() {
        let inputs = full_inputs("apps/walter/BUILD.bazel", "# Reserve the app name.\n", &[]);
        let observation = RepoFileRender::new().read(&inputs).unwrap();
        assert_eq!(rendered_content(observation), "# Reserve the app name.\n");
    }

    #[test]
    fn read_produces_the_declared_path() {
        let inputs = full_inputs("apps/walter/BUILD.bazel", "static\n", &[]);
        let Observation::Present(outputs) = RepoFileRender::new().read(&inputs).unwrap() else {
            panic!("expected Present");
        };
        let file: &RepoFile = outputs
            .get(&PortName::parse("file").unwrap())
            .unwrap()
            .downcast()
            .unwrap();
        assert_eq!(file.path().as_str(), "apps/walter/BUILD.bazel");
    }

    #[test]
    fn refuses_an_index_with_no_value() {
        let inputs = full_inputs("a", "hello {{ 0 }}", &[]);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains('0'), "{}", err.message);
        assert!(err.message.contains("only 0"), "{}", err.message);
    }

    #[test]
    fn refuses_an_unused_value() {
        let inputs = full_inputs("a", "hello {{ 0 }}", &["World", "Unused"]);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("index 1"), "{}", err.message);
        assert!(!err.message.contains("Unused"), "{}", err.message);
    }

    #[test]
    fn refuses_a_stray_double_brace() {
        // Each of these is a malformed placeholder, not a template whose
        // *text* the refusal must avoid echoing -- that property has its
        // own test, `no_error_message_echoes_the_template_or_value_text`,
        // using a marker distinctive enough that it could never coincide
        // with the fixed wording of this refusal (several of these
        // templates are themselves short substrings of that wording, so
        // asserting non-containment here would be a false positive, not a
        // real check).
        for template in [
            "{{ value }}",
            "{{ }}",
            "{{",
            "{{{{ 0 }}",
            "{{ 0 }",
            "{{  0 }}",
        ] {
            let inputs = full_inputs("a", template, &[]);
            let err = RepoFileRender::new().read(&inputs).unwrap_err();
            assert!(
                err.message.contains("not a valid"),
                "template {template:?}: {}",
                err.message
            );
        }
    }

    #[test]
    fn refuses_a_double_brace_placeholder_without_spaces() {
        let inputs = full_inputs("a", "{{0}}", &["x"]);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("not a valid"), "{}", err.message);
    }

    #[test]
    fn refuses_index_16() {
        let inputs = full_inputs("a", "{{ 16 }}", &[]);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("0..=15"), "{}", err.message);
    }

    #[test]
    fn refuses_more_than_sixteen_values() {
        let values: Vec<String> = (0..17).map(|i| format!("v{i}")).collect();
        let value_refs: Vec<&str> = values.iter().map(String::as_str).collect();
        let template: String = (0..17).fold(String::new(), |mut acc, i| {
            use std::fmt::Write;
            let _ = write!(acc, "{{{{ {i} }}}}");
            acc
        });
        let inputs = full_inputs("a", &template, &value_refs);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("at most 16"), "{}", err.message);
    }

    /// Adversarial pass 2's lesson, replayed for this tool: the template
    /// and each value are individually bounded, but a template can repeat
    /// one placeholder many times, so the *rendered* size is their
    /// product, not their sum. Refused by arithmetic before any
    /// allocation proportional to the rendered size.
    #[test]
    fn a_template_that_would_amplify_past_the_bound_is_refused_without_allocating() {
        let placeholder = "{{ 0 }}";
        let placeholders = 65536 / placeholder.len();
        let template = placeholder.repeat(placeholders);
        let value = "a".repeat(255);
        let inputs = full_inputs("a", &template, &[&value]);
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(
            err.message.contains("more than the 65536"),
            "{}",
            err.message
        );
        assert!(!err.message.contains("aaaa"), "{}", err.message);
    }

    #[test]
    fn the_projected_bound_is_exactly_the_bound_repo_file_itself_enforces() {
        let value = "a".repeat(255);
        let placeholder = "{{ 0 }}";
        // As many substitutions as fit under the bound, then pad with a
        // handful of literal (non-placeholder) characters to land on
        // exactly `MAX_RENDERED_CHARS` -- not merely under it, so this
        // pins the boundary itself: a `>=` in place of `render`'s `>`
        // would refuse this render and still pass a looser assertion.
        let at_bound_count = MAX_RENDERED_CHARS / value.len();
        let substituted = at_bound_count * value.len();
        let padding = "x".repeat(MAX_RENDERED_CHARS - substituted);
        let exactly_at_bound = format!("{}{padding}", placeholder.repeat(at_bound_count));
        let inputs = full_inputs("a", &exactly_at_bound, &[&value]);
        let observation = RepoFileRender::new().read(&inputs).unwrap();
        assert_eq!(
            rendered_content(observation).chars().count(),
            MAX_RENDERED_CHARS
        );

        let one_over_bound = format!("{exactly_at_bound}x");
        let inputs = full_inputs("a", &one_over_bound, &[&value]);
        assert!(RepoFileRender::new().read(&inputs).is_err());
    }

    #[test]
    fn ensure_matches_read_and_is_never_changed() {
        let inputs = full_inputs("a", "hello {{ 0 }}", &["World"]);
        let tool = RepoFileRender::new();
        let read = tool.read(&inputs).unwrap();
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs, &token).unwrap();
        assert!(!ensured.changed);
        let Observation::Present(read_outputs) = read else {
            panic!("expected Present");
        };
        let read_file: &RepoFile = read_outputs
            .get(&PortName::parse("file").unwrap())
            .unwrap()
            .downcast()
            .unwrap();
        let ensured_file: &RepoFile = ensured
            .outputs
            .get(&PortName::parse("file").unwrap())
            .unwrap()
            .downcast()
            .unwrap();
        assert_eq!(read_file, ensured_file);
    }

    #[test]
    fn read_rejects_a_missing_required_port() {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("path").unwrap(),
            Value::known(RepoPath::parse("a").unwrap()),
        );
        let err = RepoFileRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("template"), "{}", err.message);
    }

    #[test]
    fn no_error_message_echoes_the_template_or_value_text() {
        let secret_shaped = "SUPER-SECRET-TEMPLATE-CONTENT";
        for template in [
            "{{ 0 }} {{ foo }}".to_string(),
            format!("no placeholder here, just prose about {secret_shaped}"),
        ] {
            let inputs = full_inputs("a", &template, &["irrelevant"]);
            if let Err(err) = RepoFileRender::new().read(&inputs) {
                assert!(!err.message.contains(secret_shaped), "{}", err.message);
            }
        }
    }
}
