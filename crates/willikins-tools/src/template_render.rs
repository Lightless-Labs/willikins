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
        let rendered_text = template.as_str().replace(PLACEHOLDER, value.as_str());
        let rendered = Text::parse(&rendered_text)
            .map_err(|err| invalid(format!("rendered text is invalid: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("rendered"), Value::known(rendered));
        Ok(outputs)
    }
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
