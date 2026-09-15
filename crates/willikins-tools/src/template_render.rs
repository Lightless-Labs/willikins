//! `template.render`: the non-secret sink the taint proof needs. Pure:
//! replaces every literal `{{ value }}` placeholder with `value`'s string.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, TemplateSource, Text};

/// The exact placeholder `template.render` replaces.
const PLACEHOLDER: &str = "{{ value }}";

/// The most characters a rendered template may hold: `Text`'s own
/// `max_len`, since the output port is a `Text` and
/// `willikins_types::Text::parse` refuses anything longer anyway.
///
/// Named here so [`TemplateRender::compute`] can refuse an over-long
/// render *before* building it. Adversarial pass 2's finding: both
/// `TemplateSource` and `Text` are bounded at 65,536 characters, and the
/// placeholder is 11 characters, so a template may carry about 5,900
/// placeholders and each may expand to 65,536 characters --
/// `str::replace` allocated the whole ~390 MB result and only then handed
/// it to `Text::parse`, which refused it. 128 KiB of bounded input for a
/// third of a gigabyte of resident memory, per call, on a host with 11 GB
/// shared between everything. The bound is now checked by arithmetic
/// first; nothing is allocated for a render that cannot be a `Text`.
/// `tests::a_template_that_would_amplify_past_the_bound_is_refused_without_allocating`
/// pins it.
const MAX_RENDERED_CHARS: usize = 65_536;

/// `template.render`.
pub struct TemplateRender {
    spec: ToolSpec,
}

impl TemplateRender {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("template"), exact("TemplateSource", true));
        inputs.insert(port("value"), exact("Text", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("rendered"), scalar("Text"));
        Self {
            spec: ToolSpec {
                name: tool_name("template.render"),
                description: format!(
                    "Render a template, replacing every `{PLACEHOLDER}` with a plain-text value."
                ),
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
        let template: TemplateSource = get(inputs, "template")?;
        let value: Text = get(inputs, "value")?;
        projected_length(template.as_str(), value.as_str())?;
        let rendered_text = template.as_str().replace(PLACEHOLDER, value.as_str());
        let rendered = Text::parse(&rendered_text)
            .map_err(|err| invalid(format!("rendered text is invalid: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("rendered"), Value::known(rendered));
        Ok(outputs)
    }
}

/// Refuse, before any allocation, when rendering `template` with `value`
/// would produce more characters than
/// [`MAX_RENDERED_CHARS`]. Counts characters, not bytes, because that is
/// what `Text`'s own bound counts.
///
/// Saturating arithmetic throughout: the product of two bounded inputs
/// cannot overflow `usize` on any target this builds for, but a
/// saturating multiply costs nothing and makes that not something to
/// reason about.
fn projected_length(template: &str, value: &str) -> Result<(), ToolError> {
    let occurrences = template.matches(PLACEHOLDER).count();
    let template_chars = template.chars().count();
    let value_chars = value.chars().count();
    let projected = template_chars
        .saturating_sub(occurrences.saturating_mul(PLACEHOLDER.chars().count()))
        .saturating_add(occurrences.saturating_mul(value_chars));
    if projected > MAX_RENDERED_CHARS {
        // Names the two lengths, never the text: the template is
        // document-authored and the value is caller-supplied, and neither
        // belongs in an error message this tool builds.
        return Err(invalid(format!(
            "rendering this template would produce {projected} characters, \
             more than the {MAX_RENDERED_CHARS} a rendered text may hold \
             ({occurrences} placeholder(s) at {value_chars} character(s) each)"
        )));
    }
    Ok(())
}

impl Default for TemplateRender {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for TemplateRender {
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
    use willikins_core::{PortName, TypeName, TypeRef};

    fn full_inputs(template: &str, value: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("template").unwrap(),
            Value::known(TemplateSource::parse(template).unwrap()),
        );
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::known(Text::parse(value).unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        TemplateRender::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn read_replaces_every_placeholder() {
        let inputs = full_inputs("Hello, {{ value }}! Bye, {{ value }}.", "World");
        let observation = TemplateRender::new().read(&inputs).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let rendered = outputs.get(&PortName::parse("rendered").unwrap()).unwrap();
        assert_eq!(rendered.render().to_string(), "Hello, World! Bye, World.");
    }

    #[test]
    fn read_rejects_an_unknown_input() {
        let mut inputs = full_inputs("Hello, {{ value }}!", "World");
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("Text").unwrap())),
        );
        let err = TemplateRender::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }

    /// Adversarial pass 2, goal "inject through document text": the
    /// template is document-authored and the value is caller-supplied,
    /// and both are bounded at 65,536 characters -- but their *product*
    /// was not. `str::replace` built the whole result before anything
    /// checked it. The refusal is now arithmetic, before the allocation.
    #[test]
    fn a_template_that_would_amplify_past_the_bound_is_refused_without_allocating() {
        // The worst case a document can author: as many placeholders as
        // `TemplateSource` will hold, each expanding to a full-length
        // `Text`. Before the fix this allocated roughly 390 MB.
        let placeholders = 65536 / PLACEHOLDER.len();
        let template = PLACEHOLDER.repeat(placeholders);
        let value = "a".repeat(65536);
        let inputs = full_inputs(&template, &value);
        let err = TemplateRender::new().read(&inputs).unwrap_err();
        assert!(
            err.message.contains("more than the 65536"),
            "the refusal names the bound: {}",
            err.message
        );
        // Neither the template nor the value is echoed back.
        assert!(!err.message.contains("aaaa"), "{}", err.message);
        assert!(!err.message.contains("{{ value }}"), "{}", err.message);
    }

    /// The projected bound agrees with `Text`'s own: exactly at the
    /// bound renders, one character more is refused. This is what keeps
    /// [`MAX_RENDERED_CHARS`] honest if `Text`'s `max_len` ever moves.
    #[test]
    fn the_projected_bound_is_exactly_the_bound_text_itself_enforces() {
        assert!(Text::parse(&"a".repeat(MAX_RENDERED_CHARS)).is_ok());
        assert!(Text::parse(&"a".repeat(MAX_RENDERED_CHARS + 1)).is_err());

        let value = "a".repeat(MAX_RENDERED_CHARS);
        let inputs = full_inputs(PLACEHOLDER, &value);
        let observation = TemplateRender::new().read(&inputs).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present");
        };
        assert_eq!(
            outputs
                .get(&PortName::parse("rendered").unwrap())
                .unwrap()
                .render()
                .to_string()
                .chars()
                .count(),
            MAX_RENDERED_CHARS
        );

        let one_over = full_inputs(&format!("a{PLACEHOLDER}"), &value);
        assert!(TemplateRender::new().read(&one_over).is_err());
    }

    /// The placeholder is the only thing this engine understands. There
    /// is no include, no environment lookup, no file-reading filter, no
    /// recursion: a value that is itself a placeholder is substituted
    /// once and never re-scanned, so a document cannot build a second
    /// round of expansion out of its own output.
    #[test]
    fn a_substituted_value_is_never_rendered_again() {
        let inputs = full_inputs("{{ value }}", "{{ value }}");
        let observation = TemplateRender::new().read(&inputs).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present");
        };
        let rendered = outputs.get(&PortName::parse("rendered").unwrap()).unwrap();
        assert_eq!(rendered.render().to_string(), "{{ value }}");
    }

    /// Nothing that looks like another engine's syntax is honoured:
    /// `{{value}}` (no spaces), `{%- include -%}`, `${...}`,
    /// `{{ env.SECRET }}` and `{{ value | read_file }}` are all left
    /// exactly as the document wrote them.
    #[test]
    fn no_other_template_syntax_is_honoured() {
        for template in [
            "{{value}}",
            "{% include \"/etc/passwd\" %}",
            "${HOME}",
            "{{ env.WILLIKINS_GITHUB_TOKEN }}",
            "{{ value | read_file }}",
            "{{ self.template }}",
        ] {
            let inputs = full_inputs(template, "substituted");
            let observation = TemplateRender::new().read(&inputs).unwrap();
            let Observation::Present(outputs) = observation else {
                panic!("expected Present");
            };
            let rendered = outputs.get(&PortName::parse("rendered").unwrap()).unwrap();
            assert_eq!(
                rendered.render().to_string(),
                template,
                "`{template}` must be left alone"
            );
        }
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = TemplateRender::new().read(&Inputs::new()).unwrap_err();
        assert!(
            err.message.contains("template") || err.message.contains("value"),
            "{}",
            err.message
        );
    }
}
