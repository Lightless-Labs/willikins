//! `doppler.project.ensure`: creates (in memory) a Doppler project.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, DopplerProject, EnvironmentSlug, naming};

use crate::state::{DopplerProjectRecord, FakeState, doppler_config_key, doppler_project_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// The environments `doppler.project.ensure` seeds a root config for when
/// it creates a project, modelling Doppler's own default of `dev`, `stg`,
/// and `prd`. A following `doppler.config.ensure` for one of these finds
/// it present and reports `changed: false`; an environment outside this
/// list (such as `qa`) is created fresh, `changed: true`.
const DEFAULT_ENVIRONMENTS: [&str; 3] = ["dev", "stg", "prd"];

/// `doppler.project.ensure`.
pub struct DopplerProjectEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerProjectEnsure {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("project"), scalar("DopplerProject"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.project.ensure"),
                description: "Ensure a Doppler project exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn outputs_for(project: &DopplerProject) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("project"), Value::known(project.clone()));
        outputs
    }

    /// Seed this project's default root configs (see
    /// [`DEFAULT_ENVIRONMENTS`]) into `state`. Only called on the
    /// create path: a present-and-ours project's `ensure` never touches
    /// `doppler_configs`.
    fn seed_default_configs(state: &mut FakeState, project: &DopplerProject) {
        for name in DEFAULT_ENVIRONMENTS {
            let environment = EnvironmentSlug::parse(name)
                .unwrap_or_else(|err| unreachable!("{name:?} is a valid EnvironmentSlug: {err}"));
            let config = naming::v1::doppler_root_config(project, &environment);
            state.doppler_configs.insert(doppler_config_key(&config));
        }
    }
}

impl Tool for DopplerProjectEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        let state = self.state.lock().unwrap();
        match state.doppler_projects.get(&doppler_project_key(&project)) {
            None => Ok(Observation::Absent {
                predicted: Self::outputs_for(&project),
            }),
            Some(record) if record.ours => Ok(Observation::Present(Self::outputs_for(&project))),
            Some(_) => Ok(Observation::Foreign),
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        let mut state = self.state.lock().unwrap();
        match state.doppler_projects.get(&doppler_project_key(&project)) {
            Some(existing) if !existing.ours => Err(conflict(format!(
                "`{project}` already exists and is not ours"
            ))),
            Some(_) => Ok(Ensured {
                outputs: Self::outputs_for(&project),
                changed: false,
            }),
            None => {
                state.doppler_projects.insert(
                    doppler_project_key(&project),
                    DopplerProjectRecord { ours: true },
                );
                Self::seed_default_configs(&mut state, &project);
                Ok(Ensured {
                    outputs: Self::outputs_for(&project),
                    changed: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};
    use willikins_types::DomainType;

    fn project() -> DopplerProject {
        DopplerProject::parse("third-thoughts").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        inputs
    }

    fn tool() -> DopplerProjectEnsure {
        DopplerProjectEnsure::new(Arc::new(Mutex::new(FakeState::new())))
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
    fn read_reports_present_when_seeded_and_ours() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_project(&project(), true),
        ));
        let observation = DopplerProjectEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_reports_foreign_when_seeded_and_not_ours() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_project(&project(), false),
        ));
        let observation = DopplerProjectEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("project").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("DopplerProject").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("project"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("project"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_a_foreign_project_conflicts_and_does_not_overwrite() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_project(&project(), false),
        ));
        let tool = DopplerProjectEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&full_inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains("third-thoughts"));
        let observation = tool.read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Foreign));
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

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed, "creating the project must report changed");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !second.changed,
            "ensure on an already-present project must report changed: false"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_seeds_the_dev_stg_prd_root_configs() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let tool = DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let locked = state.lock().unwrap();
        for name in ["dev", "stg", "prd"] {
            let environment = willikins_types::EnvironmentSlug::parse(name).unwrap();
            let config = willikins_types::naming::v1::doppler_root_config(&project(), &environment);
            assert!(
                locked
                    .doppler_configs
                    .contains(&crate::state::doppler_config_key(&config)),
                "expected the seeded `{name}` root config to be present"
            );
        }
    }

    /// A present-and-ours project must not touch `doppler_configs` on a
    /// following `ensure`: only the create path seeds anything.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_a_present_project_does_not_reseed_configs() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_project(&project(), true),
        ));
        let tool = DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        let ensured = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(!ensured.changed);
        assert!(
            state.lock().unwrap().doppler_configs.is_empty(),
            "ensure on an already-present project must not seed configs"
        );
    }
}
