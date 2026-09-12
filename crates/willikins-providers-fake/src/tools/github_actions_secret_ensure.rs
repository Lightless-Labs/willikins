//! `github.actions_secret.ensure`: records (in memory) that a repository
//! secret exists. Never inspects, stores, or returns its `value`.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{Class, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec};
use willikins_types::{ActionsSecretName, GitHubRepo};

use crate::state::{FakeState, actions_secret_key};
use crate::support::{any_secret, exact, get, port, require_present, tool_name};

/// `github.actions_secret.ensure`.
pub struct GitHubActionsSecretEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl GitHubActionsSecretEnsure {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("name"), exact("ActionsSecretName", true));
        inputs.insert(port("value"), any_secret(true));
        Self {
            spec: ToolSpec {
                name: tool_name("github.actions_secret.ensure"),
                description: "Ensure a GitHub Actions repository secret exists. Never reads or stores its value.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![port("repo"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    /// Both key ports, validated and typed.
    fn key_ports(&self, inputs: &Inputs) -> Result<(GitHubRepo, ActionsSecretName), ToolError> {
        require_present(&self.spec, inputs)?;
        let repo = get(inputs, "repo")?;
        let name = get(inputs, "name")?;
        Ok((repo, name))
    }
}

impl Tool for GitHubActionsSecretEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (repo, name) = self.key_ports(inputs)?;
        let state = self.state.lock().unwrap();
        if state
            .github_actions_secrets
            .contains(&actions_secret_key(&repo, &name))
        {
            Ok(Observation::Present(Outputs::new()))
        } else {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
        let (repo, name) = self.key_ports(inputs)?;
        let mut state = self.state.lock().unwrap();
        state
            .github_actions_secrets
            .insert(actions_secret_key(&repo, &name));
        Ok(Outputs::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef, Value};
    use willikins_types::{DomainType, DopplerServiceToken};

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()
    }

    fn name() -> ActionsSecretName {
        ActionsSecretName::parse("DOPPLER_TOKEN").unwrap()
    }

    fn value() -> DopplerServiceToken {
        DopplerServiceToken::parse("dp.st.prd.exampleexampleexample").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs.insert(PortName::parse("value").unwrap(), Value::known(value()));
        inputs
    }

    fn tool() -> GitHubActionsSecretEnsure {
        GitHubActionsSecretEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_reports_present_when_seeded() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_actions_secret(&repo(), &name()),
        ));
        let observation = GitHubActionsSecretEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_never_looks_at_value_even_when_unknown() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("DopplerServiceToken").unwrap(),
            )),
        );
        let observation = tool().read(&inputs).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("name").unwrap(),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("ActionsSecretName").unwrap(),
            )),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("name"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_value_port() {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let observation = tool.read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }
}
