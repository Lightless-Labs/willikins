//! `github.actions_secret.ensure`: records (in memory) that a repository
//! secret exists. Never inspects, stores, or returns its `value`.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::{ActionsSecretName, GitHubRepo};

use crate::state::{FakeState, actions_secret_key};
use crate::support::{any_secret, exact, get, port, require_present, tool_name};

/// `github.actions_secret.ensure`.
pub struct GitHubActionsSecretEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl GitHubActionsSecretEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "github.actions_secret.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("name"), exact("ActionsSecretName", true));
        inputs.insert(port("value"), any_secret(true));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
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
        let key = actions_secret_key(&repo, &name);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.github_actions_secrets.contains(&key) {
            Ok(Observation::Present(Outputs::new()))
        } else {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (repo, name) = self.key_ports(inputs)?;
        let key = actions_secret_key(&repo, &name);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        // A sink whose value cannot be read back always writes when
        // called, whether or not the secret already exists: `changed` is
        // always `true`, matching GitHub's own 201-or-204 (both success).
        state.github_actions_secrets.insert(key);
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
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
        DopplerServiceToken::parse("dp.st.prd.exampleexampleexampleexampleexampleexample").unwrap()
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

    /// Unlike every other fake `ensure`, this one reports `changed: true`
    /// on *every* call, not just the first: its value can never be read
    /// back, so a call that finds the secret already present still wrote
    /// (a rotation propagating a fresh value that must land).
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_both_times() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            second.changed,
            "an unreadable-back sink always reports changed: true"
        );
    }
}
