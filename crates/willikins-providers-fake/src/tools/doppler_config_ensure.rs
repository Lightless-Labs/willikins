//! `doppler.config.ensure`: creates (in memory) a Doppler config, named
//! after its environment by `naming::v1::doppler_root_config`.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::rollback::{Created, Deleted, Inverse, Reversal, Side, Standing, Within};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerProject, EnvironmentSlug, TeardownToken, naming};

use crate::inverse_support::{self, ConfigContentsDrift};
use crate::state::{FakeState, doppler_config_key, doppler_project_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// `doppler.config.ensure`.
pub struct DopplerConfigEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
    inverse: DopplerConfigInverse,
}

impl DopplerConfigEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.config.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure an environment's root Doppler config exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: DopplerConfigInverse {
                state: state.clone(),
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerProject, EnvironmentSlug), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        Ok((project, environment))
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }
}

impl Tool for DopplerConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.doppler_configs.contains(&key) {
            Ok(Observation::Present(Self::outputs_for(&config)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&config),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        // Read its own state first, so `changed` is truthful: a config
        // `doppler.project.ensure` already seeded (see its own module
        // doc) is not created a second time here.
        let changed = state.doppler_configs.insert(key);
        Ok(Ensured {
            outputs: Self::outputs_for(&config),
            changed,
        })
    }

    /// Milestone 3m task P6: equal to the live tool's own declaration
    /// (decision 4), field for field.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["project", "environment"],
            within: &[Within {
                tool: "doppler.project.ensure",
                ports: &[("project", Side::Input, "project")],
            }],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `doppler.config.ensure`'s fake inverse (milestone 3m task P6). Never a
/// catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2). No timestamps exist
/// anywhere in this crate's state, so unlike the live inverse this never
/// consults a creation window.
struct DopplerConfigInverse {
    state: Arc<Mutex<FakeState>>,
}

/// [`Standing::NotOurs`]: the project lacks a record, or its own `ours`
/// flag is `false`.
const NOT_OURS: &str = "the project is not ours";

/// [`Standing::Drifted`]: the config carries a secret name this run's own
/// riders did not create.
const DRIFTED_FOREIGN_SECRET: &str =
    "the config carries a secret name this run's own riders did not create";

/// [`Standing::Drifted`]: the config is inherited by a config outside
/// this project.
const DRIFTED_FOREIGN_INHERITOR: &str = "the config is inherited by a config outside this project";

/// [`Standing::Drifted`]: the config carries a service token this run's
/// own riders did not create.
const DRIFTED_FOREIGN_TOKEN: &str =
    "the config carries a service token this run's own riders did not create";

/// [`crate::support::conflict`]'s message when [`Inverse::delete`] is
/// called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`].
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the config has changed since this run created it, or is no longer ours; the teardown \
     will not delete it";

impl DopplerConfigInverse {
    fn config_of(created: &Created<'_>) -> Result<DopplerConfig, ToolError> {
        let project: DopplerProject = get(created.inputs, "project")?;
        let environment: EnvironmentSlug = get(created.inputs, "environment")?;
        Ok(naming::v1::doppler_root_config(&project, &environment))
    }
}

impl Inverse for DopplerConfigInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let config = Self::config_of(created)?;
        let state = self.state.lock().unwrap();
        match state
            .doppler_projects
            .get(&doppler_project_key(config.project()))
        {
            None => return Ok(Standing::NotOurs { why: NOT_OURS }),
            Some(record) if !record.ours => return Ok(Standing::NotOurs { why: NOT_OURS }),
            Some(_) => {}
        }
        if !state.doppler_configs.contains(&doppler_config_key(&config)) {
            return Ok(Standing::Gone);
        }
        let mut secrets_by_config = inverse_support::tolerated_secret_names(created.riders);
        let tolerated_secrets = secrets_by_config
            .remove(config.name().as_str())
            .unwrap_or_default();
        let mut tokens_by_config = inverse_support::tolerated_token_names(created.riders);
        let tolerated_tokens = tokens_by_config
            .remove(config.name().as_str())
            .unwrap_or_default();
        if let Some(drift) = inverse_support::config_contents_drift(
            &state,
            &doppler_config_key(&config),
            &tolerated_secrets,
            &tolerated_tokens,
        ) {
            return Ok(Standing::Drifted {
                what: match drift {
                    ConfigContentsDrift::Secret => DRIFTED_FOREIGN_SECRET,
                    ConfigContentsDrift::Inheritor => DRIFTED_FOREIGN_INHERITOR,
                    ConfigContentsDrift::Token => DRIFTED_FOREIGN_TOKEN,
                },
            });
        }
        Ok(Standing::AsCreated { losing: Vec::new() })
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
                let config = Self::config_of(created)?;
                let mut state = self.state.lock().unwrap();
                inverse_support::remove_config(&mut state, &doppler_config_key(&config));
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

    fn environment() -> EnvironmentSlug {
        EnvironmentSlug::parse("prd").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        inputs
    }

    fn tool() -> DopplerConfigEnsure {
        DopplerConfigEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_with_the_predicted_config_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        let config = predicted.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(config.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    fn read_reports_present_when_seeded() {
        let config = naming::v1::doppler_root_config(&project(), &environment());
        let state = Arc::new(Mutex::new(FakeState::new().with_doppler_config(&config)));
        let observation = DopplerConfigEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("EnvironmentSlug").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("environment"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("project"), "{}", err.message);
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
        assert!(first.changed, "creating the config must report changed");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !second.changed,
            "ensure on an already-present config must report changed: false"
        );
    }

    /// Acceptance test 5's own claim: `doppler.project.ensure` seeds the
    /// `dev`/`stg`/`prd` root configs when it creates the project, so a
    /// following `doppler.config.ensure` for one of those finds it present
    /// (`changed: false`), while an environment outside that set (`qa`)
    /// is still created fresh (`changed: true`).
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_default_environment_is_unchanged_after_project_ensure_but_qa_is_created() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let project_tool = crate::tools::DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        let mut project_inputs = Inputs::new();
        project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        project_tool.ensure(&project_inputs, &token).unwrap();

        let config_tool = DopplerConfigEnsure::new(Arc::clone(&state));
        let prd = config_tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !prd.changed,
            "prd was already seeded by doppler.project.ensure"
        );

        let mut qa_inputs = Inputs::new();
        qa_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        qa_inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(EnvironmentSlug::parse("qa").unwrap()),
        );
        let qa = config_tool.ensure(&qa_inputs, &token).unwrap();
        assert!(qa.changed, "qa is not one of the seeded defaults");
    }

    /// Milestone 3m task P6: the fake inverse's own unit tests.
    mod inverse {
        use super::*;
        use willikins_core::Timestamp;
        use willikins_core::rollback::{CreationWindow, Rider, Standing};

        fn window() -> CreationWindow {
            CreationWindow {
                start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
            }
        }

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
        fn declares_delete_within_the_project() {
            assert_eq!(
                tool().reversal(),
                Reversal::Delete {
                    needs: &["project", "environment"],
                    within: &[Within {
                        tool: "doppler.project.ensure",
                        ports: &[("project", Side::Input, "project")],
                    }],
                    losses: &[],
                }
            );
        }

        #[test]
        fn standing_reports_not_ours_when_the_project_has_no_record() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::NotOurs { why: NOT_OURS }
            );
        }

        #[test]
        fn standing_reports_gone_when_the_project_is_ours_but_the_config_is_not() {
            let state = Arc::new(Mutex::new(
                FakeState::new().with_doppler_project(&project(), true),
            ));
            let tool = DopplerConfigEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::Gone
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_reports_as_created_when_present_with_no_foreign_contents() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let project_tool = crate::tools::DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            let mut project_inputs = Inputs::new();
            project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
            project_tool.ensure(&project_inputs, &token).unwrap();
            let tool = DopplerConfigEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs(), &[])).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_reports_drifted_on_a_foreign_secret() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let project_tool = crate::tools::DopplerProjectEnsure::new(state.clone());
            let token = SinkToken::new();
            let mut project_inputs = Inputs::new();
            project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
            project_tool.ensure(&project_inputs, &token).unwrap();
            state.lock().unwrap().doppler_secret_writes.insert(format!(
                "{}/{}#FOREIGN",
                project(),
                environment()
            ));
            let tool = DopplerConfigEnsure::new(state);
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
        fn delete_removes_the_config_and_its_secrets() {
            let state = Arc::new(Mutex::new(FakeState::new()));
            let project_tool = crate::tools::DopplerProjectEnsure::new(state.clone());
            let sink = SinkToken::new();
            let mut project_inputs = Inputs::new();
            project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
            project_tool.ensure(&project_inputs, &sink).unwrap();
            let key = format!("{}/{}", project(), environment());
            state
                .lock()
                .unwrap()
                .doppler_secret_writes
                .insert(format!("{key}#OURS"));
            let tool = DopplerConfigEnsure::new(state.clone());
            let inverse = Tool::inverse(&tool).unwrap();
            let token = willikins_types::TeardownToken::new();
            // The one secret is a rider this run itself wrote (decision
            // 3's "riding is not tolerance"), so `standing` tolerates it
            // rather than refusing as drift.
            let rider_instance = willikins_core::InstanceRef {
                node: willikins_core::NodeName::parse("secret").unwrap(),
                instance: None,
            };
            let mut rider_inputs = Inputs::new();
            rider_inputs.insert(
                PortName::parse("config").unwrap(),
                Value::known(naming::v1::doppler_root_config(&project(), &environment())),
            );
            rider_inputs.insert(
                PortName::parse("name").unwrap(),
                Value::known(willikins_types::SecretName::parse("OURS").unwrap()),
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
            assert!(!locked.doppler_configs.contains(&key));
            assert!(locked.doppler_secret_writes.is_empty());
        }
    }
}
