//! `doppler.branch_config.ensure`: creates (in memory) a Doppler branch
//! config at `<environment's snake join>_<branch>`, mirroring the live
//! tool's own name assembly (`willikins_providers_doppler::tools::DopplerBranchConfigEnsure`'s
//! module doc, "The name: a suffix port, not a full-name port"). Like
//! this crate's own `doppler.config.ensure` twin, membership in
//! [`crate::state::FakeState::doppler_configs`] is the only fact
//! recorded: this fake has no concept of `root` or of one config's
//! `environment`, so it cannot model the live tool's `Foreign` collision
//! case (a branch config's literal name squatted on by something else) —
//! the same limit that crate's `fake_agrees_with_live.rs` already
//! records for `doppler.config.inheritable.ensure`'s own `Foreign`-adjacent
//! case.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::rollback::{Created, Deleted, Inverse, Reversal, Side, Standing, Within};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug, TeardownToken,
};

use crate::inverse_support::{self, ConfigContentsDrift};
use crate::state::{FakeState, doppler_config_key, doppler_project_key};
use crate::support::{conflict, exact, get, invalid, port, require_present, scalar, tool_name};

/// `doppler.branch_config.ensure`.
pub struct DopplerBranchConfigEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
    inverse: DopplerBranchConfigInverse,
}

impl DopplerBranchConfigEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.branch_config.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        inputs.insert(port("branch"), exact("DopplerConfigName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description:
                    "Ensure a named branch config exists under an environment's root config."
                        .to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment"), port("branch")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: DopplerBranchConfigInverse {
                state: state.clone(),
            },
            state,
        }
    }

    fn key_ports(
        &self,
        inputs: &Inputs,
    ) -> Result<(DopplerProject, EnvironmentSlug, DopplerConfigName), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        let branch = get(inputs, "branch")?;
        Ok((project, environment, branch))
    }

    /// See the live tool's own `full_name`: the same snake-join assembly,
    /// so the two agree on which config a given `(environment, branch)`
    /// pair names.
    fn full_name(
        environment: &EnvironmentSlug,
        branch: &DopplerConfigName,
    ) -> Result<DopplerConfigName, ToolError> {
        let candidate = format!("{}_{branch}", environment.words().snake());
        DopplerConfigName::parse(&candidate).map_err(|err| {
            invalid(format!(
                "`{candidate}` (environment `{environment}` plus branch `{branch}`) is not a \
                 valid Doppler config name: {err}"
            ))
        })
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }
}

impl Tool for DopplerBranchConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project, name);
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
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project, name);
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
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
            needs: &["project", "environment", "branch"],
            within: &[
                Within {
                    tool: "doppler.project.ensure",
                    ports: &[("project", Side::Input, "project")],
                },
                Within {
                    tool: "doppler.config.ensure",
                    ports: &[
                        ("project", Side::Input, "project"),
                        ("environment", Side::Input, "environment"),
                    ],
                },
            ],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `doppler.branch_config.ensure`'s fake inverse (milestone 3m task P6).
/// Never a catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2). No timestamps exist
/// anywhere in this crate's state, so unlike the live inverse this never
/// consults a creation window; this fake also has no `root`/`environment`
/// field on a config record (this module's own doc), so it cannot model
/// the live inverse's own "`root` is true"/"`environment` differs"
/// `NotOurs` rows -- only "the project is not ours" is reachable here.
struct DopplerBranchConfigInverse {
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

impl DopplerBranchConfigInverse {
    fn config_of(created: &Created<'_>) -> Result<DopplerConfig, ToolError> {
        let project: DopplerProject = get(created.inputs, "project")?;
        let environment: EnvironmentSlug = get(created.inputs, "environment")?;
        let branch: DopplerConfigName = get(created.inputs, "branch")?;
        let name = DopplerBranchConfigEnsure::full_name(&environment, &branch)?;
        Ok(DopplerConfig::new(project, name))
    }
}

impl Inverse for DopplerBranchConfigInverse {
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

    fn project() -> DopplerProject {
        DopplerProject::parse("sample").unwrap()
    }

    fn environment() -> EnvironmentSlug {
        EnvironmentSlug::parse("prd").unwrap()
    }

    fn branch() -> DopplerConfigName {
        DopplerConfigName::parse("deployment_ios").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
        inputs
    }

    fn tool() -> DopplerBranchConfigEnsure {
        DopplerBranchConfigEnsure::new(Arc::new(Mutex::new(FakeState::new())))
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
        assert_eq!(config.render().to_string(), "sample/prd_deployment_ios");
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("branch").unwrap(),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("DopplerConfigName").unwrap(),
            )),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("branch"), "{}", err.message);
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

    /// A branch config and its environment's root config never collide:
    /// `doppler.config.ensure` seeds `dev`/`stg`/`prd` for a fresh
    /// project, and this tool's own assembled name always carries an
    /// extra `_<branch>` suffix, so the two tools' keys never coincide.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_branch_config_is_independent_of_its_environment_s_root_config() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let project_tool = crate::tools::DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        let mut project_inputs = Inputs::new();
        project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        project_tool.ensure(&project_inputs, &token).unwrap();

        let branch_tool = DopplerBranchConfigEnsure::new(Arc::clone(&state));
        let branch = branch_tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            branch.changed,
            "the branch config is not one of the seeded root configs"
        );

        let root_tool = crate::tools::DopplerConfigEnsure::new(state);
        let mut root_inputs = Inputs::new();
        root_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        root_inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        let root = root_tool.ensure(&root_inputs, &token).unwrap();
        assert!(
            !root.changed,
            "prd's root config was already seeded by doppler.project.ensure, untouched by the \
             branch config's own creation"
        );
    }

    /// Milestone 3m task P6: the fake inverse's own unit tests.
    mod inverse {
        use super::*;
        use willikins_core::Timestamp;
        use willikins_core::rollback::{CreationWindow, Standing};

        fn window() -> CreationWindow {
            CreationWindow {
                start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
            }
        }

        fn created(inputs: &Inputs) -> Created<'_> {
            Created {
                inputs,
                outputs: Box::leak(Box::new(Outputs::new())),
                window: window(),
                settled_by: window().end,
                accept: &[],
                riders: &[],
            }
        }

        #[test]
        fn declares_delete_within_the_project_and_config() {
            assert_eq!(
                tool().reversal(),
                Reversal::Delete {
                    needs: &["project", "environment", "branch"],
                    within: &[
                        Within {
                            tool: "doppler.project.ensure",
                            ports: &[("project", Side::Input, "project")],
                        },
                        Within {
                            tool: "doppler.config.ensure",
                            ports: &[
                                ("project", Side::Input, "project"),
                                ("environment", Side::Input, "environment"),
                            ],
                        },
                    ],
                    losses: &[],
                }
            );
        }

        #[test]
        fn standing_reports_not_ours_when_the_project_has_no_record() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs())).unwrap(),
                Standing::NotOurs { why: NOT_OURS }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_as_created_then_delete_removes_the_branch_config() {
            let state = Arc::new(Mutex::new(
                FakeState::new().with_doppler_project(&project(), true),
            ));
            let tool = DopplerBranchConfigEnsure::new(state.clone());
            let sink = SinkToken::new();
            tool.ensure(&full_inputs(), &sink).unwrap();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs())).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse.delete(&created(&full_inputs()), &token).unwrap();
            assert!(!deleted.already_gone);
            let name = DopplerBranchConfigEnsure::full_name(&environment(), &branch()).unwrap();
            let config = DopplerConfig::new(project(), name);
            assert!(
                !state
                    .lock()
                    .unwrap()
                    .doppler_configs
                    .contains(&doppler_config_key(&config))
            );
        }
    }
}
