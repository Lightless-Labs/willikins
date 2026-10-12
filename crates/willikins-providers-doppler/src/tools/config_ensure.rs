//! `doppler.config.ensure`: creates a real Doppler environment (and,
//! implicitly, its root config), named after its environment by
//! `naming::v1::doppler_root_config`. Port table and behaviour identical
//! to `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! A `200` at the derived name is only `Present` when the fetched config
//! is a *root* config; anything else sitting at that name is `Foreign`
//! and `ensure` refuses it. See [`DopplerClient::get_config`].
//!
//! Milestone 3m task P3 adds this tool's inverse: decision 7's
//! `doppler.config.ensure` row
//! (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`), reusing
//! `DopplerClient::delete_environment`/`list_configs` and
//! [`crate::inverse_support::config_contents_drift`].

use std::collections::HashSet;
use std::sync::Arc;

use willikins_core::rollback::{Rider, Side, Within};
use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Created, Deleted, Ensured, Inputs, Inverse, Observation, Outputs, Reversal, SinkToken,
    Standing, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug, TeardownToken,
    naming,
};

use crate::client::{DopplerClient, MANAGED_DESCRIPTION, looks_like_a_missing_project};
use crate::inverse_support::{
    ConfigContentsDrift, config_contents_drift, config_output, tolerated_secret_names,
    tolerated_token_names,
};

/// `doppler.config.ensure`.
pub struct DopplerConfigEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
    inverse: DopplerConfigInverse,
}

impl DopplerConfigEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        let inverse = DopplerConfigInverse {
            client: Arc::clone(&client),
        };
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.ensure"),
                description: "Ensure an environment's root Doppler config exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment")],
                class: Class::Reversible,
                pure: false,
            },
            client,
            inverse,
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

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// `Present` needs `200` **and** `root: true`, per the plan's port
    /// table. A `200` carrying `root: false` — or no usable `root` at all
    /// — is a config that merely occupies this name, not this
    /// environment's root config, and is reported `Foreign`: see
    /// [`DopplerClient::get_config`]'s own docs for the branch-config
    /// name collision that makes this reachable rather than theoretical.
    ///
    /// This `GET` needs the parent project to exist, and at plan time —
    /// before this workflow's own `doppler.project.ensure` node has run —
    /// it usually does not yet. The same 2026-09-20 defect
    /// `doppler.project.ensure::observe` fixes applies here identically:
    /// Doppler answers either `404` or a `400` naming "does not have
    /// access to requested project" for that missing parent, depending on
    /// whether this token can see any project in the workplace at all,
    /// and both must read as `Absent`
    /// rather than refusing the plan. See
    /// [`looks_like_a_missing_project`]'s own doc for what that does and
    /// does not assume.
    fn observe(
        &self,
        project: &DopplerProject,
        config: &DopplerConfig,
    ) -> Result<Observation, ToolError> {
        match self.client.get_config(project, config.name()) {
            Ok(body) if body.root == Some(true) => {
                Ok(Observation::Present(Self::outputs_for(config)))
            }
            Ok(_) => Ok(Observation::Foreign),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(config: &DopplerConfig) -> ToolError {
        conflict(format!(
            "`{config}` already exists and is not the environment's root config"
        ))
    }
}

impl Tool for DopplerConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        self.observe(&project, &config)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        match self.observe(&project, &config)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Foreign => Err(Self::foreign_conflict(&config)),
            Observation::Mismatch { .. } => unreachable!(
                "doppler.config.ensure's own observe never returns Mismatch: it has no \
                 non-key input to mismatch on"
            ),
            Observation::Absent { .. } => {
                match self.client.create_environment(&project, config.name()) {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&config),
                        changed: true,
                    }),
                    // Doppler documents no error-body schema at all, so a
                    // conflict after a possibly delivered create cannot
                    // be told apart from a genuine failure by its body:
                    // re-read either way.
                    Err(err) => match self.observe(&project, &config)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Foreign => Err(Self::foreign_conflict(&config)),
                        Observation::Absent { .. } | Observation::Mismatch { .. } => {
                            Err(err.into())
                        }
                    },
                }
            }
        }
    }

    /// Milestone 3m decisions 4, 5, task P3: deletable, riding out with
    /// `doppler.project.ensure` alone (the table's own "Why": "a root
    /// config goes only with its environment" -- never with another
    /// environment's own instance of this same tool). `needs` names
    /// `project` and `environment`: no optional, document-bound
    /// credential port of its own, the same reasoning
    /// `doppler.project.ensure::reversal` already gives.
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

/// [`Standing::NotOurs`]: the project's own `description` no longer
/// carries the [`MANAGED_DESCRIPTION`] marker -- the same fail-closed
/// reading `doppler.project.ensure`'s own inverse gives, including a
/// response that omitted the field outright.
const NOT_OURS_LOST_THE_MARKER: &str =
    "the project's description no longer carries the managed-by: willikins marker";

/// [`Standing::NotOurs`]: the environment's own root config was created
/// outside this instance's creation window (decision 2) -- read off the
/// matching [`crate::client::ConfigListEntry::created_at`], since the
/// single-config `GET` ([`crate::client::ConfigBody`]) carries no
/// timestamp of its own. The environment and its root config are
/// created together by one `POST` ([`DopplerClient::create_environment`]'s
/// own doc), so the root config's own creation date stands in for "the
/// environment's own creation date" (decision 7's literal wording)
/// without a second client method -- recorded in this milestone's plan
/// as an addendum.
const NOT_OURS_OUTSIDE_WINDOW: &str =
    "the environment's own creation date falls outside this run's creation window";

/// [`Standing::Drifted`]: the environment holds a config neither its own
/// root nor a rider `doppler.branch_config.ensure`'s own recorded branch.
const DRIFTED_FOREIGN_CONFIG: &str = "the environment has a config this run did not create";

/// [`Standing::Drifted`]: a config in the environment carries a secret
/// name this run's own riders did not create.
const DRIFTED_FOREIGN_SECRET: &str =
    "a config in the environment carries a secret name this run's own riders did not create";

/// [`Standing::Drifted`]: a config in the environment is inherited by a
/// config outside this project.
const DRIFTED_FOREIGN_INHERITOR: &str =
    "a config in the environment is inherited by a config outside this project";

/// [`Standing::Drifted`]: a config in the environment carries a service
/// token this run's own riders did not create.
const DRIFTED_FOREIGN_TOKEN: &str =
    "a config in the environment carries a service token this run's own riders did not create";

/// [`crate::tool::helpers::conflict`]'s message when [`Inverse::delete`]
/// is called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`] -- mirrors `DopplerProjectInverse`'s own
/// constant of the same shape.
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the environment has changed since this run created it, or is no longer ours; the \
     teardown will not delete it";

/// The `project`/`environment` key ports, re-resolved from the journal
/// (decision 2): exactly `Created::inputs`, never read a second time
/// from anywhere else.
fn environment_of(created: &Created<'_>) -> Result<(DopplerProject, EnvironmentSlug), ToolError> {
    let project = get(created.inputs, "project")?;
    let environment = get(created.inputs, "environment")?;
    Ok((project, environment))
}

/// Every rider `doppler.branch_config.ensure`'s own recorded config name
/// -- the branch-config half of `doppler.project.ensure`'s own
/// `tolerated_environments_and_branch_configs`, reused here scoped to
/// whichever environment the caller is actually asking about (every
/// branch config this run created, in any environment; the caller
/// narrows by the config's own recorded `environment` field, read off
/// the same [`crate::client::ConfigListEntry`] its membership check
/// already walks).
fn tolerated_branch_configs(riders: &[Rider<'_>]) -> HashSet<String> {
    let mut branch_configs = HashSet::new();
    for rider in riders {
        if rider.tool != "doppler.branch_config.ensure" {
            continue;
        }
        if let Some(config) = config_output(rider.outputs) {
            branch_configs.insert(config.name().to_string());
        }
    }
    branch_configs
}

/// `doppler.config.ensure`'s inverse (milestone 3m task P3, decisions 4,
/// 7). Never a catalog tool in its own right, never reachable except
/// from `apply_teardown` (trust boundaries 1, 2).
struct DopplerConfigInverse {
    client: Arc<DopplerClient>,
}

impl DopplerConfigInverse {
    /// Decision 7's `doppler.config.ensure` row, past the point the
    /// project itself is confirmed to carry the managed marker and the
    /// environment's own root config entry (`root_name`, `root_entry`)
    /// has been found in `configs` (a `list_configs` read of the whole
    /// project, shared with the membership walk below so this method
    /// makes exactly the reads decision 7's own intro promises: "one or
    /// a few `GET`s").
    fn drift_in_environment(
        &self,
        created: &Created<'_>,
        project: &DopplerProject,
        root_name: &DopplerConfigName,
        configs: &[crate::client::ConfigListEntry],
    ) -> Result<Option<Standing>, ToolError> {
        let tolerated_branches = tolerated_branch_configs(created.riders);
        let in_environment = configs
            .iter()
            .filter(|entry| entry.environment == root_name.as_str());

        for entry in in_environment.clone() {
            let tolerated =
                entry.name == root_name.as_str() || tolerated_branches.contains(&entry.name);
            if !tolerated {
                return Ok(Some(Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG,
                }));
            }
        }

        let tolerated_secrets = tolerated_secret_names(created.riders);
        let tolerated_tokens = tolerated_token_names(created.riders);
        for entry in in_environment {
            let Ok(config_name) = DopplerConfigName::parse(&entry.name) else {
                return Ok(Some(Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG,
                }));
            };
            let names_tolerated = tolerated_secrets
                .get(config_name.as_str())
                .cloned()
                .unwrap_or_default();
            let tokens_tolerated = tolerated_tokens
                .get(config_name.as_str())
                .cloned()
                .unwrap_or_default();
            if let Some(drift) = config_contents_drift(
                &self.client,
                project,
                &config_name,
                &names_tolerated,
                &tokens_tolerated,
            )? {
                return Ok(Some(Standing::Drifted {
                    what: match drift {
                        ConfigContentsDrift::Secret => DRIFTED_FOREIGN_SECRET,
                        ConfigContentsDrift::Inheritor => DRIFTED_FOREIGN_INHERITOR,
                        ConfigContentsDrift::Token => DRIFTED_FOREIGN_TOKEN,
                    },
                }));
            }
        }

        Ok(None)
    }
}

impl Inverse for DopplerConfigInverse {
    /// **Gone, read two ways.** A missing project
    /// ([`looks_like_a_missing_project`]) is `Gone` outright. A project
    /// that still exists and still carries the marker, but no longer
    /// lists a root config at this environment's own derived name, is
    /// also `Gone`: the operator may have deleted the environment by
    /// hand, or an earlier teardown step in this same run already did
    /// (decision 6's resume case). Neither reaches `NotOurs`: that
    /// variant refuses the *whole* teardown, which an already-gone
    /// resource must never do.
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let (project, environment) = environment_of(created)?;
        let root_name = naming::v1::doppler_root_config(&project, &environment);
        let root_name = root_name.name();

        match self.client.get_project(&project) {
            Err(err) if looks_like_a_missing_project(&err) => return Ok(Standing::Gone),
            Err(err) => return Err(err.into()),
            Ok(body) if body.description.as_deref() != Some(MANAGED_DESCRIPTION) => {
                return Ok(Standing::NotOurs {
                    why: NOT_OURS_LOST_THE_MARKER,
                });
            }
            Ok(_) => {}
        }

        let configs = self.client.list_configs(&project)?;
        let Some(root_entry) = configs
            .iter()
            .find(|entry| entry.root && entry.name == root_name.as_str())
        else {
            return Ok(Standing::Gone);
        };
        let in_window = root_entry
            .created_at
            .is_some_and(|at| at >= created.window.start && at <= created.window.end);
        if !in_window {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_OUTSIDE_WINDOW,
            });
        }

        if let Some(standing) = self.drift_in_environment(created, &project, root_name, &configs)? {
            return Ok(standing);
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
                let (project, environment) = environment_of(created)?;
                let root_name = naming::v1::doppler_root_config(&project, &environment);
                self.client.delete_environment(&project, root_name.name())?;
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
        }
    }
}
