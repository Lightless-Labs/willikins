//! Text rendering for every CLI output.
//!
//! Invariant: this module is the *only* place in `willikins-cli` that turns
//! a domain value into human-readable text, and every function in it that
//! touches a [`Value`] calls [`Value::render`] to do so. There is no other
//! path from a `Value` to text anywhere in this crate — no bespoke
//! formatter, no reach into a domain object's own `Display`. This is what
//! keeps the CLI's text output redacting a secret exactly the same way its
//! JSON output does, since both ultimately go through the same
//! `Value::render` / `DomainObject::render` machinery.
//!
//! JSON output, in contrast, is produced by each type's own
//! [`serde::Serialize`] impl wherever one exists (`Plan`, `Description`,
//! `PlanError`, ...); the two exceptions are [`CheckError`] and
//! [`CheckWarning`], which derive no `Serialize` at all, so this module
//! also builds their JSON shape by hand (from the same plain identifiers a
//! secret value never touches — neither variant carries a `Value`).

use willikins_core::{
    Action, CheckError, CheckWarning, Description, Plan, PlannedNode, PortType, Value,
};

/// Render a single [`Value`] for text output. The one and only place in
/// this crate that calls [`Value::render`] directly on a bare value outside
/// a larger structure — every other renderer below goes through this.
fn value_text(value: &Value) -> String {
    value.render().to_string()
}

// ---------------------------------------------------------------------
// check warnings / errors
// ---------------------------------------------------------------------

/// One human-readable line per warning.
#[must_use]
pub fn check_warnings_text(warnings: &[CheckWarning]) -> String {
    warnings
        .iter()
        .map(check_warning_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn check_warning_line(warning: &CheckWarning) -> String {
    match warning {
        CheckWarning::UnusedInput { input } => format!("warning: unused input `{input}`"),
    }
}

/// `warnings` as a JSON array, one object per warning.
#[must_use]
pub fn check_warnings_json(warnings: &[CheckWarning]) -> serde_json::Value {
    serde_json::Value::Array(
        warnings
            .iter()
            .map(|warning| match warning {
                CheckWarning::UnusedInput { input } => serde_json::json!({
                    "kind": "unused_input",
                    "input": input.to_string(),
                    "message": check_warning_line(warning),
                }),
            })
            .collect(),
    )
}

/// One human-readable line per error: `VariantName: node.port ...`, `node`
/// and `port` dotted where the error concerns a single port, so both the
/// error's own kind and the offending site are single copyable tokens in
/// both text and JSON output.
#[must_use]
pub fn check_errors_text(errors: &[CheckError]) -> String {
    errors
        .iter()
        .map(check_error_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `errors` as a JSON array; see the module docs for why this is built by
/// hand rather than derived.
#[must_use]
pub fn check_errors_json(errors: &[CheckError]) -> serde_json::Value {
    serde_json::Value::Array(errors.iter().map(check_error_json).collect())
}

fn port_type_text(ty: &PortType) -> String {
    match ty {
        PortType::Exact(ty) => ty.to_string(),
        PortType::AnySecret => "AnySecret".to_string(),
    }
}

/// One line: `VariantName: <detail>`. The variant name matches
/// [`CheckError`]'s own Rust identifier (`PascalCase`), the same
/// externally-tagged convention `serde` gives [`willikins_core::PlanError`],
/// so an agent can grep for either kind of failure by its type name in
/// either output mode.
fn check_error_line(error: &CheckError) -> String {
    format!("{}: {}", check_error_kind(error), check_error_detail(error))
}

fn check_error_detail(error: &CheckError) -> String {
    match error {
        CheckError::UnknownTool { node, tool } => {
            format!("{node}: unknown tool `{tool}`")
        }
        CheckError::UnknownPort { node, tool, port } => {
            format!("{node}.{port}: no such port on tool `{tool}`")
        }
        CheckError::UnknownNode {
            node,
            port,
            referenced,
        } => format!("{node}.{port}: references unknown node `{referenced}`"),
        CheckError::UnboundInput { node, port } => {
            format!("{node}.{port}: required input is not bound")
        }
        CheckError::UndeclaredInput { node, port, input } => {
            format!("{node}.{port}: references undeclared input `{input}`")
        }
        CheckError::InvalidLiteral { node, port, error } => {
            format!("{node}.{port}: invalid literal: {error}")
        }
        CheckError::TypeMismatch {
            node,
            port,
            expected,
            found,
        } => format!(
            "{node}.{port}: expected {}, found `{found}`",
            port_type_text(expected)
        ),
        CheckError::SecretLiteral { node, port } => {
            format!("{node}.{port}: a literal cannot supply a secret value")
        }
        CheckError::SecretToNonSecretSink { from, to } => {
            format!("{}.{} -> {}.{}", from.0, from.1, to.0, to.1)
        }
        CheckError::SecretWorkflowInput { input, ty } => {
            format!("input `{input}` has secret type `{ty}`; a workflow input may not be secret")
        }
        CheckError::SecretForEachSource { node } => {
            format!("{node}: for_each source is secret")
        }
        CheckError::ForEachOverScalar { node } => {
            format!("{node}: for_each source is not a list")
        }
        CheckError::ItemOutsideForEach { node, port } => {
            format!("{node}.{port}: `item` is only valid inside a for_each node")
        }
        CheckError::KeyedOnScalarNode {
            node,
            port,
            referenced,
        } => format!("{node}.{port}: keyed reference to non-for_each node `{referenced}`"),
        CheckError::Cycle { nodes } => {
            let joined = nodes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ");
            format!("cycle among nodes: {joined}")
        }
        CheckError::DefaultTypeMismatch {
            input,
            expected,
            found,
        } => format!("input `{input}`: default value has type `{found}`, expected `{expected}`"),
        CheckError::NestedList {
            node,
            port,
            referenced,
        } => format!(
            "{node}.{port}: node `{referenced}` runs once per item and its port is already a list"
        ),
        CheckError::DuplicateNode { node } => format!("duplicate node name `{node}`"),
        CheckError::UnregisteredInputType { input, ty } => {
            format!("input `{input}`: declared type `{ty}` is not a registered type")
        }
        CheckError::DuplicateForEachDefault { node, input, key } => {
            format!("{node}: input `{input}`'s default has two items both keyed `{key}`")
        }
        CheckError::LiteralOutput { output } => {
            format!("output `{output}`: a workflow output must be a reference, not a literal")
        }
    }
}

/// The `kind` tag for `error`: its own Rust variant name, `PascalCase`,
/// matching how [`willikins_core::PlanError`] tags itself when serialized
/// (serde's externally-tagged default).
fn check_error_kind(error: &CheckError) -> &'static str {
    match error {
        CheckError::UnknownTool { .. } => "UnknownTool",
        CheckError::UnknownPort { .. } => "UnknownPort",
        CheckError::UnknownNode { .. } => "UnknownNode",
        CheckError::UnboundInput { .. } => "UnboundInput",
        CheckError::UndeclaredInput { .. } => "UndeclaredInput",
        CheckError::InvalidLiteral { .. } => "InvalidLiteral",
        CheckError::TypeMismatch { .. } => "TypeMismatch",
        CheckError::SecretLiteral { .. } => "SecretLiteral",
        CheckError::SecretToNonSecretSink { .. } => "SecretToNonSecretSink",
        CheckError::SecretWorkflowInput { .. } => "SecretWorkflowInput",
        CheckError::SecretForEachSource { .. } => "SecretForEachSource",
        CheckError::ForEachOverScalar { .. } => "ForEachOverScalar",
        CheckError::ItemOutsideForEach { .. } => "ItemOutsideForEach",
        CheckError::KeyedOnScalarNode { .. } => "KeyedOnScalarNode",
        CheckError::Cycle { .. } => "Cycle",
        CheckError::DefaultTypeMismatch { .. } => "DefaultTypeMismatch",
        CheckError::NestedList { .. } => "NestedList",
        CheckError::DuplicateNode { .. } => "DuplicateNode",
        CheckError::UnregisteredInputType { .. } => "UnregisteredInputType",
        CheckError::DuplicateForEachDefault { .. } => "DuplicateForEachDefault",
        CheckError::LiteralOutput { .. } => "LiteralOutput",
    }
}

fn check_error_json(error: &CheckError) -> serde_json::Value {
    serde_json::json!({
        "kind": check_error_kind(error),
        "message": check_error_line(error),
    })
}

// ---------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------

/// Render a [`Description`] for text output: errors, then missing inputs
/// (each with its type, prompt, example, and default), then resolved
/// values.
#[must_use]
pub fn describe_text(description: &Description) -> String {
    let mut lines = Vec::new();
    for error in &description.errors {
        lines.push(format!("error: {}: {}", error.input, error.error));
    }
    for missing in &description.missing {
        lines.push(format!(
            "missing `{}` (type `{}`): {}",
            missing.name, missing.ty, missing.prompt
        ));
        lines.push(format!("  example: {}", missing.example));
        if let Some(default) = &missing.default {
            lines.push(format!("  default: {default}"));
        }
    }
    for (name, value) in &description.resolved {
        lines.push(format!("{name}: {}", value_text(value)));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------

/// Render a [`Plan`] for text output: one line per planned node instance
/// (its instance key, tool, and action) followed by its outputs, then
/// workflow outputs, then the plan's class and approval requirement.
#[must_use]
pub fn plan_text(plan: &Plan) -> String {
    let mut lines = Vec::new();
    for node in &plan.nodes {
        lines.push(planned_node_line(node));
        for (port, value) in node.outputs.iter() {
            lines.push(format!("    {port}: {}", value_text(value)));
        }
    }
    lines.push("outputs:".to_string());
    for (name, value) in &plan.outputs {
        lines.push(format!("  {name}: {}", value_text(value)));
    }
    lines.push(format!("class: {:?}", plan.class));
    lines.push(format!("requires_approval: {}", plan.requires_approval));
    lines.join("\n")
}

fn planned_node_line(node: &PlannedNode) -> String {
    let action = action_text(node.action);
    match &node.instance {
        Some(instance) => format!("{}[{instance}] ({}): {action}", node.name, node.tool),
        None => format!("{} ({}): {action}", node.name, node.tool),
    }
}

fn action_text(action: Action) -> &'static str {
    match action {
        Action::Compute => "Compute",
        Action::Create => "Create",
        Action::NoOp => "NoOp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use willikins_core::{Class, NodeName, Outputs, PortName, ToolName};
    use willikins_types::DomainType;

    /// A [`Plan`] holding a `Known` secret must never print its bytes
    /// through the text renderer, and must carry the redaction marker
    /// instead — the same guarantee acceptance test 8 pins for JSON.
    #[test]
    fn plan_text_redacts_a_known_secret_output() {
        let token = willikins_types::DopplerServiceToken::parse("dp.st.fake-secret-bytes").unwrap();
        let mut outputs = Outputs::new();
        outputs.insert(PortName::parse("token").unwrap(), Value::known(token));

        let node = PlannedNode {
            name: NodeName::parse("token").unwrap(),
            instance: None,
            tool: ToolName::parse("doppler.service_token.ensure").unwrap(),
            action: Action::Create,
            inputs: willikins_core::Inputs::new(),
            outputs,
        };
        let plan = Plan {
            workflow: "test".to_string(),
            nodes: vec![node],
            outputs: IndexMap::new(),
            class: Class::Reversible,
            requires_approval: false,
        };

        let text = plan_text(&plan);
        assert!(
            text.contains("[REDACTED DopplerServiceToken]"),
            "text: {text}"
        );
        assert!(!text.contains("fake-secret-bytes"), "text leaked: {text}");
    }

    #[test]
    fn check_warnings_text_names_the_unused_input() {
        let warnings = vec![CheckWarning::UnusedInput {
            input: willikins_core::InputName::parse("environments").unwrap(),
        }];
        let text = check_warnings_text(&warnings);
        assert!(text.contains("environments"));
    }

    #[test]
    fn check_errors_text_and_json_both_name_the_dotted_ports() {
        let error = CheckError::SecretToNonSecretSink {
            from: (
                NodeName::parse("token").unwrap(),
                PortName::parse("token").unwrap(),
            ),
            to: (
                NodeName::parse("readme").unwrap(),
                PortName::parse("value").unwrap(),
            ),
        };
        let text = check_errors_text(std::slice::from_ref(&error));
        assert!(text.contains("SecretToNonSecretSink"), "text: {text}");
        assert!(text.contains("token.token"), "text: {text}");
        assert!(text.contains("readme.value"), "text: {text}");

        let json = check_errors_json(std::slice::from_ref(&error));
        let json_text = json.to_string();
        assert!(
            json_text.contains("SecretToNonSecretSink"),
            "json: {json_text}"
        );
        assert!(json_text.contains("token.token"), "json: {json_text}");
        assert!(json_text.contains("readme.value"), "json: {json_text}");
    }
}
