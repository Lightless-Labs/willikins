//! `operator.acknowledge`: a gate over an operator's own acknowledgement,
//! for the manual steps no provider API exposes -- G3, decision (j) point 6
//! (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`).
//!
//! Pure, read-only, never mutates anything: `step` names the manual step in
//! the document's own words (policy lives in the workflow, never in the
//! tool); `acknowledged` is the operator's typed acknowledgement, supplied
//! only as a workflow input (`check` refuses a default or a literal on this
//! type). `read` answers `Present` once `acknowledged` is known, `Absent`
//! while it is still [`willikins_core::Value::unknown`] -- never through
//! [`willikins_core::tool::helpers::get`], which would fail on an `Unknown`
//! value instead of reporting the gate unmet.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::Text;

/// This tool's one gate: `step` is the only subject, so a blocked report
/// names the manual step and nothing else -- `acknowledged` carries no
/// text of its own worth rendering.
static GATE: Gate = Gate {
    need: "the operator has done the manual step this document names",
    how: "do the step named below, then supply the awaited input",
    subject: &["step"],
};

/// `operator.acknowledge`.
pub struct OperatorAcknowledge {
    spec: ToolSpec,
}

impl OperatorAcknowledge {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("step"), exact("Text", true));
        inputs.insert(port("acknowledged"), exact("OperatorAcknowledgement", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("step"), scalar("Text"));
        Self {
            spec: ToolSpec {
                name: tool_name("operator.acknowledge"),
                description: "A gate: reports whether the operator has acknowledged doing the \
                              manual step `step` names, by supplying `acknowledged: done`."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    /// `step`, passed through as this gate's own output, and whether
    /// `acknowledged` is known. `step` must always be present (`check`
    /// requires it and it is never itself awaited); `acknowledged` may be
    /// `Unknown` -- an unsupplied `OperatorAcknowledgement` input -- which
    /// is exactly what makes this gate `Absent` rather than a `ToolError`.
    fn compute(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let step: Text = get(inputs, "step")?;
        let mut outputs = Outputs::new();
        outputs.insert(port("step"), Value::known(step));
        let acknowledged_known = inputs
            .get(&port("acknowledged"))
            .is_some_and(Value::is_known);
        if acknowledged_known {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }
}

impl Default for OperatorAcknowledge {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for OperatorAcknowledge {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // A gate is pure, so `apply` never reaches this while the node is
        // `Action::Blocked` (decision (j), point 4) -- reached only once
        // `read` already answered `Present`, exactly like every other pure
        // tool's `ensure` (`willikins-tools::TemplateRender`'s own doc).
        match self.compute(inputs)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("compute only ever returns Present or Absent: {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};
    use willikins_types::DomainType;

    fn inputs_with(step: &str, acknowledged: Option<&str>) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("step").unwrap(),
            Value::known(Text::parse(step).unwrap()),
        );
        let value = match acknowledged {
            Some(text) => {
                Value::known(willikins_types::OperatorAcknowledgement::parse(text).unwrap())
            }
            None => Value::unknown(TypeRef::scalar(
                TypeName::parse("OperatorAcknowledgement").unwrap(),
            )),
        };
        inputs.insert(PortName::parse("acknowledged").unwrap(), value);
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        OperatorAcknowledge::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_is_a_well_formed_gate() {
        let tool = OperatorAcknowledge::new();
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog.insert(std::sync::Arc::new(tool)).unwrap();
    }

    #[test]
    fn read_is_absent_while_acknowledged_is_unknown() {
        let inputs = inputs_with("do the thing", None);
        let observation = OperatorAcknowledge::new().read(&inputs).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        let step = predicted.get(&PortName::parse("step").unwrap()).unwrap();
        assert_eq!(step.render().to_string(), "do the thing");
    }

    #[test]
    fn read_is_present_once_acknowledged_is_done() {
        let inputs = inputs_with("do the thing", Some("done"));
        let observation = OperatorAcknowledge::new().read(&inputs).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let step = outputs.get(&PortName::parse("step").unwrap()).unwrap();
        assert_eq!(step.render().to_string(), "do the thing");
    }

    #[test]
    fn ensure_agrees_with_read_on_both_arms() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = OperatorAcknowledge::new();

        let absent_inputs = inputs_with("do the thing", None);
        let ensured = tool.ensure(&absent_inputs, &token).unwrap();
        assert!(!ensured.changed);

        let present_inputs = inputs_with("do the thing", Some("done"));
        let ensured = tool.ensure(&present_inputs, &token).unwrap();
        assert!(!ensured.changed);
    }

    #[test]
    fn is_a_gate_over_step_only() {
        let tool = OperatorAcknowledge::new();
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject.len(), 1);
        assert_eq!(gate.subject[0], "step");
    }

    #[test]
    fn read_rejects_a_missing_step() {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("acknowledged").unwrap(),
            Value::known(willikins_types::OperatorAcknowledgement::parse("done").unwrap()),
        );
        let err = OperatorAcknowledge::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("step"), "{}", err.message);
    }
}
