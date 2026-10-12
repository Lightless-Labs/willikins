//! `doppler.project.ensure`: creates a real Doppler project. Port table
//! and behaviour identical to `willikins_providers_fake`'s tool of the
//! same name (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! Milestone 3m task P2 adds this tool's inverse: decision 7's project
//! row (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`), reusing
//! `DopplerClient::delete_project`/`list_environments`/`list_configs`/
//! `config_has_foreign_secret_name` (task P2 commit 1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use willikins_core::rollback::Rider;
use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Created, Deleted, Ensured, Inputs, Inverse, Observation, Outputs, Reversal, SinkToken,
    Standing, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, DopplerTokenName, SecretName,
    TeardownToken,
};

use crate::client::{DopplerClient, MANAGED_DESCRIPTION, looks_like_a_missing_project};

/// `doppler.project.ensure`.
pub struct DopplerProjectEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
    inverse: DopplerProjectInverse,
}

impl DopplerProjectEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("project"), scalar("DopplerProject"));
        let inverse = DopplerProjectInverse {
            client: Arc::clone(&client),
        };
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
            client,
            inverse,
        }
    }

    fn outputs_for(project: &DopplerProject) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("project"), Value::known(project.clone()));
        outputs
    }

    /// `GET` the project, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// The ownership check is exact equality against
    /// [`MANAGED_DESCRIPTION`], not a substring match: an operator who
    /// appends a note to the description (or a project this tool created
    /// whose description was later edited) reads as `Foreign` rather than
    /// `Present`, which is the safer failure mode — an ensure that would
    /// otherwise silently claim a description-edited project as still
    /// ours risks acting on a resource a human has started managing by
    /// hand. Pinned by `tests/project_ensure_mock.rs`.
    ///
    /// **2026-09-20 defect, fixed here.** A brand-new project name did
    /// not always answer `404`: as soon as this token could see *any*
    /// project in the workplace — which is every provisioning run after
    /// the first — the same absent name
    /// answered `400` "This token does not have access to requested
    /// project" instead, which fell through to the `Err(err) =>
    /// Err(err.into())` arm below and refused the whole plan before this
    /// tool's own `create_project` ever ran — the very case a plan
    /// exists for. [`looks_like_a_missing_project`] reads both shapes the
    /// same way `Observation::Absent` already read a `404`; see its own
    /// doc for what that does and does not assume, and
    /// `docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`
    /// for the finding.
    fn observe(&self, project: &DopplerProject) -> Result<Observation, ToolError> {
        match self.client.get_project(project) {
            Ok(body) if body.description.as_deref() == Some(MANAGED_DESCRIPTION) => {
                Ok(Observation::Present(Self::outputs_for(project)))
            }
            Ok(_) => Ok(Observation::Foreign),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Self::outputs_for(project),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(project: &DopplerProject) -> ToolError {
        conflict(format!("`{project}` already exists and is not ours"))
    }
}

impl Tool for DopplerProjectEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        self.observe(&project)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        match self.observe(&project)? {
            Observation::Foreign => Err(Self::foreign_conflict(&project)),
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => unreachable!(
                "doppler.project.ensure's own observe never returns Mismatch: it has no \
                 non-key input to mismatch on"
            ),
            Observation::Absent { .. } => match self.client.create_project(&project) {
                Ok(()) => Ok(Ensured {
                    outputs: Self::outputs_for(&project),
                    changed: true,
                }),
                // The create may have landed despite the error (Doppler
                // documents no error-body schema at all, so this crate
                // cannot tell "already exists" apart from any other
                // failure the way GitHub's `already_exists` flag lets
                // `willikins-providers-github` do): re-read rather than
                // assume either way.
                Err(err) => match self.observe(&project)? {
                    Observation::Present(outputs) => Ok(Ensured {
                        outputs,
                        changed: false,
                    }),
                    Observation::Foreign => Err(Self::foreign_conflict(&project)),
                    Observation::Absent { .. } | Observation::Mismatch { .. } => Err(err.into()),
                },
            },
        }
    }

    /// Milestone 3m decisions 4, 5, task P2: deletable, with no
    /// container of its own (`within: &[]`, this tool sits at the root
    /// of every other Doppler tool's own `within`) and no declared loss.
    /// `needs` names only `project`: unlike `github.repo.ensure`, this
    /// tool holds no optional, document-bound credential port of its
    /// own (`crate::client`'s own "Doppler credentials" section; every
    /// call goes through the one client this tool was built with).
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

/// The three environments every Doppler project arrives with, whatever
/// this run itself created (milestone 3m task P2, verify item 7, settled
/// live by the milestone 1 research note and `docs/HANDOFF.md`'s own
/// "three auto-created root configs"). An environment id outside this
/// set, and not a rider's own (see [`tolerated_environments_and_branch_configs`]),
/// is [`Standing::Drifted`].
const DEFAULT_ENVIRONMENTS: [&str; 3] = ["dev", "stg", "prd"];

/// [`Standing::NotOurs`]: the project's `description` no longer carries
/// the [`MANAGED_DESCRIPTION`] marker — including a response that omitted
/// it outright, the same `None` `looks_like_a_missing_project` cannot
/// already explain (fail closed, decision 7).
const NOT_OURS_LOST_THE_MARKER: &str =
    "the project's description no longer carries the managed-by: willikins marker";

/// [`Standing::NotOurs`]: the project's own `created_at` falls outside
/// this instance's creation window (decision 2) — including a response
/// that omitted `created_at` outright (fail closed).
const NOT_OURS_OUTSIDE_WINDOW: &str =
    "the project's own creation date falls outside this run's creation window";

/// [`Standing::Drifted`]: the project holds an environment outside the
/// three Doppler creates by default and outside every rider
/// `doppler.config.ensure` this run's own record tolerates.
const DRIFTED_FOREIGN_ENVIRONMENT: &str = "the project has an environment this run did not create";

/// [`Standing::Drifted`]: the project holds a config that is neither the
/// root of a tolerated environment nor a rider `doppler.branch_config.ensure`'s
/// own recorded branch — including one whose own name this crate's
/// grammar refuses outright (fail closed: a name Doppler accepts but
/// this crate's own naming scheme never produces is not one of ours
/// either).
const DRIFTED_FOREIGN_CONFIG: &str = "the project has a config this run did not create";

/// [`Standing::Drifted`]: a config in the project carries a secret name
/// this run's own riders did not write into it, directly or through a
/// base it inherits (the decision note on inherited names).
const DRIFTED_FOREIGN_SECRET: &str =
    "a config in the project carries a secret name this run's own riders did not create";

/// [`Standing::Drifted`]: a config in the project is inherited by a
/// config outside this project — including a response whose
/// `inheritedBy` was absent outright, which cannot rule that out (fail
/// closed; see [`crate::client::ConfigBody::inherited_by`]'s own doc).
const DRIFTED_FOREIGN_INHERITOR: &str =
    "a config in the project is inherited by a config outside this project";

/// [`Standing::Drifted`]: a config in the project carries a service token
/// this run's own riders did not create.
const DRIFTED_FOREIGN_TOKEN: &str =
    "a config in the project carries a service token this run's own riders did not create";

/// [`crate::tool::helpers::conflict`]'s message when [`Inverse::delete`]
/// is called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`] — the documented contract every real inverse
/// in this workspace honours (mirrors `GitHubRepoInverse`'s own constant
/// of the same shape).
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the project has changed since this run created it, or is no longer ours; the teardown \
     will not delete it";

/// `doppler.project.ensure`'s inverse (milestone 3m task P2, decisions 4,
/// 7). Never a catalog tool in its own right, never reachable except
/// from `apply_teardown` (trust boundaries 1, 2).
struct DopplerProjectInverse {
    client: Arc<DopplerClient>,
}

/// The `project` key port, re-resolved from the journal (decision 2):
/// exactly `Created::inputs`, never read a second time from anywhere
/// else.
fn project_of(created: &Created<'_>) -> Result<DopplerProject, ToolError> {
    get(created.inputs, "project")
}

/// The `config` *output* of a rider that only ever reports its identity
/// through an output port (`doppler.config.ensure`/`doppler.branch_config.ensure`:
/// their own `config` is derived from `project`+`environment`/`branch`,
/// never a port a document binds directly) — `rider.outputs`, never
/// `rider.inputs`, and a downcast failure is read the same as "not
/// present": a rider this crate cannot identify tolerates nothing (fail
/// closed, never widened by guessing).
fn config_output(outputs: &Outputs) -> Option<DopplerConfig> {
    outputs
        .get(&port("config"))
        .and_then(|value| value.downcast::<DopplerConfig>())
        .cloned()
}

/// The tolerated Doppler-side environment ids (the three defaults, plus
/// every `doppler.config.ensure` rider's own recorded root config name —
/// which doubles as the environment's own id, `naming::v1::doppler_root_config`'s
/// snake join) and the tolerated branch config names (every
/// `doppler.branch_config.ensure` rider's own recorded config name),
/// milestone 3m task P2, decision 7's project row.
fn tolerated_environments_and_branch_configs(
    riders: &[Rider<'_>],
) -> (HashSet<String>, HashSet<String>) {
    let mut environments: HashSet<String> = DEFAULT_ENVIRONMENTS
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut branch_configs = HashSet::new();
    for rider in riders {
        match rider.tool {
            "doppler.config.ensure" => {
                if let Some(config) = config_output(rider.outputs) {
                    environments.insert(config.name().to_string());
                }
            }
            "doppler.branch_config.ensure" => {
                if let Some(config) = config_output(rider.outputs) {
                    branch_configs.insert(config.name().to_string());
                }
            }
            _ => {}
        }
    }
    (environments, branch_configs)
}

/// Every rider `doppler.secret.set`'s own recorded `(config, name)` pair,
/// grouped by the config's own name — the tolerated secret names
/// [`crate::client::DopplerClient::config_has_foreign_secret_name`] takes
/// per config. A rider whose own `config`/`name` port this crate cannot
/// read (fails to parse, or is unbound) tolerates nothing, the same
/// fail-closed reasoning [`config_output`] documents.
fn tolerated_secret_names(riders: &[Rider<'_>]) -> HashMap<String, HashSet<String>> {
    let mut by_config: HashMap<String, HashSet<String>> = HashMap::new();
    for rider in riders {
        if rider.tool != "doppler.secret.set" {
            continue;
        }
        let Ok(config) = get::<DopplerConfig>(rider.inputs, "config") else {
            continue;
        };
        let Ok(name) = get::<SecretName>(rider.inputs, "name") else {
            continue;
        };
        by_config
            .entry(config.name().to_string())
            .or_default()
            .insert(name.as_str().to_string());
    }
    by_config
}

/// Every rider `doppler.service_token.ensure`'s own recorded `(config,
/// name)` pair, grouped by the config's own name — the same shape
/// [`tolerated_secret_names`] builds, for token names instead.
fn tolerated_token_names(riders: &[Rider<'_>]) -> HashMap<String, HashSet<String>> {
    let mut by_config: HashMap<String, HashSet<String>> = HashMap::new();
    for rider in riders {
        if rider.tool != "doppler.service_token.ensure" {
            continue;
        }
        let Ok(config) = get::<DopplerConfig>(rider.inputs, "config") else {
            continue;
        };
        let Ok(name) = get::<DopplerTokenName>(rider.inputs, "name") else {
            continue;
        };
        by_config
            .entry(config.name().to_string())
            .or_default()
            .insert(name.as_str().to_string());
    }
    by_config
}

impl DopplerProjectInverse {
    /// Decision 7's project row, past the point `standing` has already
    /// confirmed the project is not [`Standing::Gone`].
    fn standing_from_body(
        &self,
        project: &DopplerProject,
        body: &crate::client::ProjectBody,
        created: &Created<'_>,
    ) -> Result<Standing, ToolError> {
        if body.description.as_deref() != Some(MANAGED_DESCRIPTION) {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_LOST_THE_MARKER,
            });
        }
        let in_window = body
            .created_at
            .is_some_and(|at| at >= created.window.start && at <= created.window.end);
        if !in_window {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_OUTSIDE_WINDOW,
            });
        }

        let (tolerated_environments, tolerated_branch_configs) =
            tolerated_environments_and_branch_configs(created.riders);

        let environments = self.client.list_environments(project)?;
        if environments
            .iter()
            .any(|env| !tolerated_environments.contains(&env.id))
        {
            return Ok(Standing::Drifted {
                what: DRIFTED_FOREIGN_ENVIRONMENT,
            });
        }

        let configs = self.client.list_configs(project)?;
        let tolerated_secrets = tolerated_secret_names(created.riders);
        let tolerated_tokens = tolerated_token_names(created.riders);

        for entry in &configs {
            let Ok(config_name) = DopplerConfigName::parse(&entry.name) else {
                return Ok(Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG,
                });
            };
            let tolerated = (entry.root && tolerated_environments.contains(&entry.environment))
                || tolerated_branch_configs.contains(config_name.as_str());
            if !tolerated {
                return Ok(Standing::Drifted {
                    what: DRIFTED_FOREIGN_CONFIG,
                });
            }

            let config_body = self.client.get_config(project, &config_name)?;

            let names_tolerated = tolerated_secrets
                .get(config_name.as_str())
                .cloned()
                .unwrap_or_default();
            let bases = config_body.inherits.unwrap_or_default();
            if self.client.config_has_foreign_secret_name(
                project,
                &config_name,
                &names_tolerated,
                &bases,
            )? {
                return Ok(Standing::Drifted {
                    what: DRIFTED_FOREIGN_SECRET,
                });
            }

            match &config_body.inherited_by {
                None => {
                    return Ok(Standing::Drifted {
                        what: DRIFTED_FOREIGN_INHERITOR,
                    });
                }
                Some(entries) => {
                    if entries
                        .iter()
                        .any(|entry| entry.project != project.as_str())
                    {
                        return Ok(Standing::Drifted {
                            what: DRIFTED_FOREIGN_INHERITOR,
                        });
                    }
                }
            }

            let tokens = self.client.list_service_tokens(project, &config_name)?;
            let tokens_tolerated = tolerated_tokens
                .get(config_name.as_str())
                .cloned()
                .unwrap_or_default();
            if tokens
                .iter()
                .any(|token| !tokens_tolerated.contains(&token.name))
            {
                return Ok(Standing::Drifted {
                    what: DRIFTED_FOREIGN_TOKEN,
                });
            }
        }

        Ok(Standing::AsCreated { losing: Vec::new() })
    }
}

impl Inverse for DopplerProjectInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let project = project_of(created)?;
        match self.client.get_project(&project) {
            Err(err) if looks_like_a_missing_project(&err) => Ok(Standing::Gone),
            Err(err) => Err(err.into()),
            Ok(body) => self.standing_from_body(&project, &body, created),
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
                let project = project_of(created)?;
                self.client.delete_project(&project)?;
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
        }
    }
}
