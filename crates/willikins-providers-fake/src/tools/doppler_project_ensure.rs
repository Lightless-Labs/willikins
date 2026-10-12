//! `doppler.project.ensure`: creates (in memory) a Doppler project.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use std::collections::HashSet;

use willikins_core::rollback::{Created, Deleted, Inverse, Reversal, Rider, Standing};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, DopplerProject, EnvironmentSlug, TeardownToken, naming};

use crate::inverse_support::{self, ConfigContentsDrift};
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
    inverse: DopplerProjectInverse,
}

impl DopplerProjectEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.project.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("project"), scalar("DopplerProject"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Doppler project exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: DopplerProjectInverse {
                state: state.clone(),
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
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &doppler_project_key(&project));
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
        let key = doppler_project_key(&project);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
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

    /// Milestone 3m task P6: equal to the live tool's own declaration
    /// (decisions 4, 5), field for field.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["project"],
            within: &[],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `doppler.project.ensure`'s fake inverse (milestone 3m task P6). Never
/// a catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2).
///
/// No timestamps exist anywhere in this crate's state, so unlike the
/// live inverse this never consults a creation window. This fake also
/// has no separate concept of a Doppler "environment" apart from a
/// config's own name: [`DEFAULT_ENVIRONMENTS`] plus every rider
/// `doppler.config.ensure`/`doppler.branch_config.ensure`'s own recorded
/// config name is the one tolerated set, collapsing the live inverse's
/// two separate "foreign environment"/"foreign config" rows into one.
struct DopplerProjectInverse {
    state: Arc<Mutex<FakeState>>,
}

/// [`Standing::NotOurs`]: a project record exists at this key, but its
/// own `ours` flag is `false`.
const NOT_OURS: &str = "the project is not ours";

/// [`Standing::Drifted`]: the project holds a config outside the three
/// default environments and outside every rider this run's own record
/// tolerates.
const DRIFTED_FOREIGN_CONFIG: &str = "the project has a config this run did not create";

/// [`Standing::Drifted`]: a config in the project carries a secret name
/// this run's own riders did not create.
const DRIFTED_FOREIGN_SECRET: &str =
    "a config in the project carries a secret name this run's own riders did not create";

/// [`Standing::Drifted`]: a config in the project is inherited by a
/// config outside this project.
const DRIFTED_FOREIGN_INHERITOR: &str =
    "a config in the project is inherited by a config outside this project";

/// [`Standing::Drifted`]: a config in the project carries a service token
/// this run's own riders did not create.
const DRIFTED_FOREIGN_TOKEN: &str =
    "a config in the project carries a service token this run's own riders did not create";

/// [`crate::support::conflict`]'s message when [`Inverse::delete`] is
/// called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`].
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the project has changed since this run created it, or is no longer ours; the teardown \
     will not delete it";

/// The tolerated config names: the three defaults, plus every rider
/// `doppler.config.ensure`/`doppler.branch_config.ensure`'s own recorded
/// `config` output's own name part.
fn tolerated_config_names(riders: &[Rider<'_>]) -> HashSet<String> {
    let mut names: HashSet<String> = DEFAULT_ENVIRONMENTS
        .iter()
        .map(ToString::to_string)
        .collect();
    for rider in riders {
        if rider.tool != "doppler.config.ensure" && rider.tool != "doppler.branch_config.ensure" {
            continue;
        }
        if let Some(config) = rider
            .outputs
            .get(&port("config"))
            .and_then(|value| value.downcast::<willikins_types::DopplerConfig>())
        {
            names.insert(config.name().to_string());
        }
    }
    names
}

impl DopplerProjectInverse {
    fn project_of(created: &Created<'_>) -> Result<DopplerProject, ToolError> {
        get(created.inputs, "project")
    }

    fn standing_from_record(
        state: &FakeState,
        project: &DopplerProject,
        created: &Created<'_>,
    ) -> Standing {
        let tolerated_names = tolerated_config_names(created.riders);
        let tolerated_secrets = inverse_support::tolerated_secret_names(created.riders);
        let tolerated_tokens = inverse_support::tolerated_token_names(created.riders);
        let prefix = format!("{project}/");
        for config_key in state
            .doppler_configs
            .iter()
            .filter(|key| key.starts_with(&prefix))
        {
            let name = config_key.trim_start_matches(&prefix);
            if !tolerated_names.contains(name) {
                return Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG,
                };
            }
            let secrets = tolerated_secrets.get(name).cloned().unwrap_or_default();
            let tokens = tolerated_tokens.get(name).cloned().unwrap_or_default();
            if let Some(drift) =
                inverse_support::config_contents_drift(state, config_key, &secrets, &tokens)
            {
                return Standing::Drifted {
                    what: match drift {
                        ConfigContentsDrift::Secret => DRIFTED_FOREIGN_SECRET,
                        ConfigContentsDrift::Inheritor => DRIFTED_FOREIGN_INHERITOR,
                        ConfigContentsDrift::Token => DRIFTED_FOREIGN_TOKEN,
                    },
                };
            }
        }
        Standing::AsCreated { losing: Vec::new() }
    }
}

impl Inverse for DopplerProjectInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let project = Self::project_of(created)?;
        let state = self.state.lock().unwrap();
        match state.doppler_projects.get(&doppler_project_key(&project)) {
            None => Ok(Standing::Gone),
            Some(record) if !record.ours => Ok(Standing::NotOurs { why: NOT_OURS }),
            Some(_) => Ok(Self::standing_from_record(&state, &project, created)),
        }
    }

    fn delete(&self, created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        match self.standing(created)? {
            Standing::Gone => Ok(Deleted { already_gone: true }),
            Standing::AsCreated { losing }
                if losing.iter().all(|loss| {
                    created
                        .accept
                        .iter()
                        .any(|accepted| accepted.as_str() == *loss)
                }) =>
            {
                let project = Self::project_of(created)?;
                let mut state = self.state.lock().unwrap();
                let prefix = format!("{project}/");
                let configs: Vec<String> = state
                    .doppler_configs
                    .iter()
                    .filter(|key| key.starts_with(&prefix))
                    .cloned()
                    .collect();
                for config_key in configs {
                    inverse_support::remove_config(&mut state, &config_key);
                }
                state
                    .doppler_projects
                    .remove(&doppler_project_key(&project));
                state
                    .doppler_project_members
                    .remove(&doppler_project_key(&project));
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
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

    /// Milestone 3m task P6: the fake inverse's own unit tests, parallel
    /// to the live `DopplerProjectInverse`'s mock tests but against
    /// `FakeState` directly.
    mod inverse {
        use super::*;
        use willikins_core::rollback::{CreationWindow, Standing};
        use willikins_core::{InstanceRef, NodeName, PortName, Timestamp};
        use willikins_types::{DopplerConfig, DopplerConfigName, SecretName};

        fn window() -> CreationWindow {
            CreationWindow {
                start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
            }
        }

        /// Leaks an empty [`Outputs`] per call -- acceptable in a short-lived
        /// test process, and avoids threading an extra `outputs` binding
        /// through every call site below (`github.repo.ensure`'s own fake
        /// inverse tests use this crate's existing `Outputs::new()`
        /// binding instead; this module has many more call sites, so this
        /// helper takes just `inputs`/`riders`).
        fn created<'a>(inputs: &'a Inputs, riders: &'a [Rider<'a>]) -> Created<'a> {
            Created {
                inputs,
                outputs: Box::leak(Box::new(Outputs::new())),
                window: window(),
                settled_by: window().end,
                accept: &[],
                riders,
            }
        }

        #[test]
        fn declares_delete_with_no_within_and_no_loss() {
            assert_eq!(
                tool().reversal(),
                Reversal::Delete {
                    needs: &["project"],
                    within: &[],
                    losses: &[],
                }
            );
        }

        #[test]
        fn standing_reports_gone_when_absent() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::Gone
            );
        }

        #[test]
        fn standing_reports_not_ours_when_foreign() {
            let state = Arc::new(Mutex::new(
                FakeState::new().with_doppler_project(&project(), false),
            ));
            let tool = DopplerProjectEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::NotOurs { why: NOT_OURS }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_reports_as_created_with_only_default_configs() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            tool.ensure(&full_inputs(), &token).unwrap();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_reports_drifted_on_a_foreign_config() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            tool.ensure(&full_inputs(), &token).unwrap();
            state.lock().unwrap().doppler_configs.insert(format!(
                "{}/{}",
                project(),
                DopplerConfigName::parse("qa").unwrap()
            ));
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG
                }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_tolerates_a_rider_config() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            tool.ensure(&full_inputs(), &token).unwrap();
            let qa = DopplerConfig::new(project(), DopplerConfigName::parse("qa").unwrap());
            state.lock().unwrap().doppler_configs.insert(qa.to_string());
            let rider_instance = InstanceRef {
                node: NodeName::parse("qa").unwrap(),
                instance: None,
            };
            let rider_inputs = Inputs::new();
            let mut rider_outputs = Outputs::new();
            rider_outputs.insert(PortName::parse("config").unwrap(), Value::known(qa));
            let riders = [Rider {
                tool: "doppler.config.ensure",
                instance: &rider_instance,
                inputs: &rider_inputs,
                outputs: &rider_outputs,
            }];
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &riders)).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_reports_drifted_on_a_foreign_secret() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            tool.ensure(&full_inputs(), &token).unwrap();
            let dev = DopplerConfig::new(project(), DopplerConfigName::parse("dev").unwrap());
            {
                let mut locked = state.lock().unwrap();
                locked
                    .doppler_secret_writes
                    .insert(format!("{dev}#FOREIGN"));
            }
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::Drifted {
                    what: DRIFTED_FOREIGN_SECRET
                }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_tolerates_a_rider_secret() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            tool.ensure(&full_inputs(), &token).unwrap();
            let dev = DopplerConfig::new(project(), DopplerConfigName::parse("dev").unwrap());
            {
                let mut locked = state.lock().unwrap();
                locked.doppler_secret_writes.insert(format!("{dev}#OURS"));
            }
            let rider_instance = InstanceRef {
                node: NodeName::parse("secret").unwrap(),
                instance: None,
            };
            let mut rider_inputs = Inputs::new();
            rider_inputs.insert(PortName::parse("config").unwrap(), Value::known(dev));
            rider_inputs.insert(
                PortName::parse("name").unwrap(),
                Value::known(SecretName::parse("OURS").unwrap()),
            );
            let rider_outputs = Outputs::new();
            let riders = [Rider {
                tool: "doppler.secret.set",
                instance: &rider_instance,
                inputs: &rider_inputs,
                outputs: &rider_outputs,
            }];
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &riders)).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_removes_the_project_and_its_configs_and_secrets() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let sink = SinkToken::new();
            tool.ensure(&full_inputs(), &sink).unwrap();
            let dev = DopplerConfig::new(project(), DopplerConfigName::parse("dev").unwrap());
            {
                let mut locked = state.lock().unwrap();
                locked.doppler_secret_writes.insert(format!("{dev}#OURS"));
            }
            let inverse = Tool::inverse(&tool).unwrap();
            let token = willikins_types::TeardownToken::new();
            // The one secret is a rider this run itself wrote (decision
            // 3's "riding is not tolerance"), so `standing` tolerates it
            // rather than refusing as drift.
            let rider_instance = InstanceRef {
                node: NodeName::parse("secret").unwrap(),
                instance: None,
            };
            let mut rider_inputs = Inputs::new();
            rider_inputs.insert(
                PortName::parse("config").unwrap(),
                Value::known(dev.clone()),
            );
            rider_inputs.insert(
                PortName::parse("name").unwrap(),
                Value::known(SecretName::parse("OURS").unwrap()),
            );
            let rider_outputs = Outputs::new();
            let riders = [Rider {
                tool: "doppler.secret.set",
                instance: &rider_instance,
                inputs: &rider_inputs,
                outputs: &rider_outputs,
            }];
            let deleted = inverse
                .delete(&created(&full_inputs(), &riders), &token)
                .unwrap();
            assert!(!deleted.already_gone);
            let locked = state.lock().unwrap();
            assert!(
                !locked
                    .doppler_projects
                    .contains_key(&doppler_project_key(&project()))
            );
            assert!(locked.doppler_configs.is_empty());
            assert!(locked.doppler_secret_writes.is_empty());
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_on_an_already_gone_project_reports_already_gone() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse
                .delete(&created(&full_inputs(), &[]), &token)
                .unwrap();
            assert!(deleted.already_gone);
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_refuses_a_drifted_project_and_changes_nothing() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let tool = DopplerProjectEnsure::new(state.clone());
            let sink = SinkToken::new();
            tool.ensure(&full_inputs(), &sink).unwrap();
            state.lock().unwrap().doppler_configs.insert(format!(
                "{}/{}",
                project(),
                DopplerConfigName::parse("qa").unwrap()
            ));
            let inverse = Tool::inverse(&tool).unwrap();
            let token = willikins_types::TeardownToken::new();
            let err = inverse
                .delete(&created(&full_inputs(), &[]), &token)
                .unwrap_err();
            assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
            assert!(
                state
                    .lock()
                    .unwrap()
                    .doppler_projects
                    .contains_key(&doppler_project_key(&project()))
            );
        }
    }
}
