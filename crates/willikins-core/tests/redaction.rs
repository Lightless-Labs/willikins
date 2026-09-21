//! Acceptance test 8a: redaction by construction.
//!
//! Builds a secret [`Value`] directly, embeds it in [`Inputs`], [`Outputs`],
//! [`Observation::Present`], [`Observation::Absent`], and a [`ToolError`]
//! produced by a tool that is handed the value, and asserts the secret's
//! bytes appear in none of `serde_json::to_string`, `format!("{:?}")`, or
//! `format!("{:#?}")` of any of them, nor in the `ToolError`'s `Display`,
//! while the redaction marker does. This does not depend on any provider
//! state, per the plan's "neither depending on empty state" requirement.

use std::sync::Arc;

use indexmap::IndexMap;
use willikins_core::{
    Catalog, Class, Ensured, Inputs, Observation, Outputs, PortName, PortSpec, PortType, Tool,
    ToolError, ToolErrorKind, ToolName, ToolSpec, Value,
};
use willikins_types::{DomainType, DopplerServiceToken};

/// The distinctive bytes a leak would reveal.
const SECRET_TAIL: &str = "fakesecretbytes0001aaaaaaaaaaaaaaaaaaaaaaa";

/// The full token string, matching `DopplerServiceToken`'s pattern.
/// `concat!`-joined with the same tail text as [`SECRET_TAIL`] so this
/// file holds no literal spelling the whole thing contiguously.
const TOKEN: &str = concat!("dp.st.prd.", "fakesecretbytes0001aaaaaaaaaaaaaaaaaaaaaaa");

/// The marker every redaction is expected to show instead.
const MARKER: &str = "[REDACTED DopplerServiceToken]";

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("valid port name")
}

fn token_value() -> Value {
    Value::known(DopplerServiceToken::parse(TOKEN).expect("the fixture token parses"))
}

/// A tool that is *careless* on purpose: its `read` builds a `ToolError`
/// message directly from `format!("{inputs:?}")`, so this test proves that
/// `Inputs`' `Debug` is safe by construction — not merely that a careful
/// tool happened to stay quiet. Its `ensure` is unused; this test never
/// mints a `SinkToken`.
struct CarelessTool {
    spec: ToolSpec,
}

impl CarelessTool {
    fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("token"),
            PortSpec {
                ty: PortType::AnySecret,
                required: true,
                derived_only: false,
            },
        );
        Self {
            spec: ToolSpec {
                name: ToolName::parse("test.careless").unwrap(),
                description: "A tool that echoes its inputs into its own error.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for CarelessTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        Err(ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("provider rejected inputs: {inputs:?}"),
        })
    }

    fn ensure(
        &self,
        _inputs: &Inputs,
        _token: &willikins_types::SinkToken,
    ) -> Result<Ensured, ToolError> {
        unimplemented!("not exercised by this test")
    }
}

fn assert_redacted(what: &str, haystack: &str) {
    assert!(
        !haystack.contains(SECRET_TAIL),
        "{what} leaked the secret's bytes: {haystack:?}"
    );
    assert!(
        haystack.contains(MARKER),
        "{what} did not show the redaction marker: {haystack:?}"
    );
}

#[test]
fn secret_value_never_leaks_through_inputs_outputs_observation_or_tool_error() {
    let value = token_value();
    assert!(value.is_secret());

    let mut inputs = Inputs::new();
    inputs.insert(port("token"), value.clone());

    let mut outputs = Outputs::new();
    outputs.insert(port("token"), value.clone());

    let present = Observation::Present(outputs.clone());
    let absent = Observation::Absent {
        predicted: outputs.clone(),
    };

    let tool = CarelessTool::new();
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(CarelessTool::new())).unwrap();
    let error = tool.read(&inputs).unwrap_err();

    // The error message itself embeds a redacted `Inputs` debug dump, which
    // is exactly the case this test exists to cover.
    assert_redacted("ToolError.message", &error.message);

    let cases: Vec<(&str, String)> = vec![
        (
            "serde_json(inputs)",
            serde_json::to_string(&inputs).unwrap(),
        ),
        (
            "serde_json(outputs)",
            serde_json::to_string(&outputs).unwrap(),
        ),
        (
            "serde_json(present)",
            serde_json::to_string(&present).unwrap(),
        ),
        (
            "serde_json(absent)",
            serde_json::to_string(&absent).unwrap(),
        ),
        ("serde_json(error)", serde_json::to_string(&error).unwrap()),
        ("Debug(inputs)", format!("{inputs:?}")),
        ("Debug(outputs)", format!("{outputs:?}")),
        ("Debug(present)", format!("{present:?}")),
        ("Debug(absent)", format!("{absent:?}")),
        ("Debug(error)", format!("{error:?}")),
        ("Debug#(inputs)", format!("{inputs:#?}")),
        ("Debug#(outputs)", format!("{outputs:#?}")),
        ("Debug#(present)", format!("{present:#?}")),
        ("Debug#(absent)", format!("{absent:#?}")),
        ("Debug#(error)", format!("{error:#?}")),
        ("Display(error)", error.to_string()),
    ];

    for (what, haystack) in &cases {
        assert_redacted(what, haystack);
    }

    // The catalog itself never sees the secret value, but its JSON must
    // stay clean regardless — nothing here should ever be able to leak it.
    let catalog_json = catalog.list_tools_json().to_string();
    assert!(!catalog_json.contains(SECRET_TAIL));
}
