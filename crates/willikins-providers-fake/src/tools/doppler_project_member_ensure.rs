//! `doppler.project_member.ensure`: makes (in memory) a Doppler service
//! account a member of a project with a given role on given environments.
//! Mirrors the live tool's `Observation`/`updates` split -- see its own
//! module doc (`willikins_providers_doppler::tools::project_member_ensure`)
//! for why `Absent` covers two different real states and why `updates`
//! exists at all (milestone 3h decision (b)). Port table and behaviour
//! identical to the live tool (`tests/catalog_parity.rs` pins the two
//! `ToolSpec`s equal).

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{
    DopplerProject, DopplerProjectRole, DopplerServiceAccountName, EnvironmentSlug,
};

use crate::state::{
    DopplerProjectMemberRecord, FakeState, doppler_project_key, doppler_project_member_key,
};
use crate::support::{
    conflict, exact, get, invalid, list, not_found, port, require_present, scalar, tool_name,
};

/// SHARED VALUES' own bound, refused by shape before any state is
/// consulted.
const MAX_ENVIRONMENTS: usize = 16;

/// `doppler.project_member.ensure`.
pub struct DopplerProjectMemberEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

/// See the live tool's own doc for why this exists distinctly from
/// [`Observation`].
enum MemberState {
    Absent,
    Present,
    NeedsUpdate {
        existing_environments: Vec<String>,
        access_all_environments: bool,
    },
    Mismatch,
}

/// See the live tool's identical helper.
fn role_rank(role: &str) -> Option<u8> {
    match role {
        "no_access" => Some(0),
        "viewer" => Some(1),
        "collaborator" => Some(2),
        _ => None,
    }
}

impl DopplerProjectMemberEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.project_member.ensure";

    /// Build the tool against `state`, constructing its spec. Field for
    /// field identical to the live tool's own `new` -- `tests/catalog_parity.rs`
    /// pins the two equal.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(
            port("service_account"),
            exact("DopplerServiceAccountName", true),
        );
        inputs.insert(port("role"), exact("DopplerProjectRole", true));
        inputs.insert(
            port("environments"),
            PortSpec {
                ty: PortType::Exact(list("EnvironmentSlug")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("project"), scalar("DopplerProject"));
        outputs.insert(port("service_account"), scalar("DopplerServiceAccountName"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Doppler service account is a member of a project with \
                               at least a given role on given environments."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("service_account")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(
        &self,
        inputs: &Inputs,
    ) -> Result<
        (
            DopplerProject,
            DopplerServiceAccountName,
            DopplerProjectRole,
            Vec<EnvironmentSlug>,
        ),
        ToolError,
    > {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        let service_account: DopplerServiceAccountName = get(inputs, "service_account")?;
        let role: DopplerProjectRole = get(inputs, "role")?;
        let environments = Self::get_environment_list(inputs, "environments")?;
        Self::validate_environments(&environments)?;
        Ok((project, service_account, role, environments))
    }

    /// See the live tool's identical helper.
    fn get_environment_list(
        inputs: &Inputs,
        name: &str,
    ) -> Result<Vec<EnvironmentSlug>, ToolError> {
        let value = inputs
            .get(&port(name))
            .ok_or_else(|| invalid(format!("port `{name}` is required")))?;
        if !value.is_known() {
            return Err(invalid(format!("port `{name}` is unknown")));
        }
        let items = value
            .as_list()
            .ok_or_else(|| invalid(format!("port `{name}` is not a list")))?;
        items
            .iter()
            .map(|item| {
                willikins_types::downcast::<EnvironmentSlug>(item.as_ref())
                    .cloned()
                    .ok_or_else(|| invalid(format!("port `{name}` has an unexpected type")))
            })
            .collect()
    }

    /// See the live tool's identical helper.
    fn validate_environments(environments: &[EnvironmentSlug]) -> Result<(), ToolError> {
        if environments.is_empty() {
            return Err(invalid(
                "port `environments` must name at least one environment",
            ));
        }
        if environments.len() > MAX_ENVIRONMENTS {
            return Err(invalid(format!(
                "port `environments` names {} environments, more than the limit of {MAX_ENVIRONMENTS}",
                environments.len()
            )));
        }
        let mut seen = HashSet::new();
        for environment in environments {
            if !seen.insert(environment.to_string()) {
                return Err(invalid(format!(
                    "port `environments` names `{environment}` more than once"
                )));
            }
        }
        Ok(())
    }

    fn outputs_for(
        project: &DopplerProject,
        service_account: &DopplerServiceAccountName,
    ) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("project"), Value::known(project.clone()));
        outputs.insert(
            port("service_account"),
            Value::known(service_account.clone()),
        );
        outputs
    }

    /// Resolve `name` to its fake slug, the same "zero/one/many" rule the
    /// live tool's own `resolve_slug` applies.
    fn resolve_slug(
        state: &FakeState,
        name: &DopplerServiceAccountName,
    ) -> Result<String, ToolError> {
        let slugs = state
            .doppler_service_accounts
            .get(name.as_str())
            .cloned()
            .unwrap_or_default();
        match slugs.len() {
            0 => Err(not_found(format!(
                "no Doppler service account in this workplace is named `{name}`"
            ))),
            1 => Ok(slugs.into_iter().next().unwrap()),
            n => Err(conflict(format!(
                "`{n}` service accounts are named `{name}`; this tool will not guess between \
                 them; rename until the name is unique"
            ))),
        }
    }

    /// See the live tool's identical helper.
    fn classify(
        members: &[DopplerProjectMemberRecord],
        slug: &str,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
    ) -> MemberState {
        let Some(member) = members.iter().find(|member| member.slug == slug) else {
            return MemberState::Absent;
        };
        let requested_rank = role_rank(role.as_str())
            .unwrap_or_else(|| unreachable!("a DopplerProjectRole is always rankable"));
        let Some(actual_rank) = role_rank(&member.role) else {
            return MemberState::Mismatch;
        };
        if actual_rank > requested_rank {
            return MemberState::Mismatch;
        }
        if actual_rank < requested_rank {
            return MemberState::NeedsUpdate {
                existing_environments: member.environments.clone(),
                access_all_environments: member.access_all_environments,
            };
        }
        if member.access_all_environments {
            return MemberState::Present;
        }
        let existing: HashSet<&str> = member.environments.iter().map(String::as_str).collect();
        let fully_covered = environments
            .iter()
            .all(|environment| existing.contains(environment.to_string().as_str()));
        if fully_covered {
            MemberState::Present
        } else {
            MemberState::NeedsUpdate {
                existing_environments: member.environments.clone(),
                access_all_environments: false,
            }
        }
    }

    /// Resolve the slug and classify this member's state against `state`
    /// -- shared by `read`, `updates`, and `ensure`.
    fn analyze(
        state: &FakeState,
        project: &DopplerProject,
        service_account: &DopplerServiceAccountName,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
    ) -> Result<(MemberState, String), ToolError> {
        let slug = Self::resolve_slug(state, service_account)?;
        let members = state
            .doppler_project_members
            .get(&doppler_project_key(project))
            .cloned()
            .unwrap_or_default();
        Ok((Self::classify(&members, &slug, role, environments), slug))
    }

    /// The sorted union of `existing` and `requested`'s canonical
    /// strings -- this fake never refuses an unparseable existing
    /// environment the way the live client does, since every field here
    /// is already a bare `String`.
    fn union_environments(existing: &[String], requested: &[EnvironmentSlug]) -> Vec<String> {
        let mut union: BTreeSet<String> = existing.iter().cloned().collect();
        union.extend(requested.iter().map(ToString::to_string));
        union.into_iter().collect()
    }

    fn mismatch_conflict() -> ToolError {
        conflict(
            "this member holds a role this tool would have to lower or cannot express; it \
             changes neither; change it by hand or ask for that role",
        )
    }
}

impl Tool for DopplerProjectMemberEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, service_account, role, environments) = self.key_ports(inputs)?;
        let key = doppler_project_member_key(&project, &service_account);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        let (member_state, _slug) =
            Self::analyze(&state, &project, &service_account, &role, &environments)?;
        Ok(match member_state {
            MemberState::Absent | MemberState::NeedsUpdate { .. } => Observation::Absent {
                predicted: Self::outputs_for(&project, &service_account),
            },
            MemberState::Present => {
                Observation::Present(Self::outputs_for(&project, &service_account))
            }
            MemberState::Mismatch => Observation::Mismatch { port: port("role") },
        })
    }

    fn updates(&self, inputs: &Inputs) -> Result<bool, ToolError> {
        let (project, service_account, role, environments) = self.key_ports(inputs)?;
        let state = self.state.lock().unwrap();
        let (member_state, _slug) =
            Self::analyze(&state, &project, &service_account, &role, &environments)?;
        Ok(matches!(member_state, MemberState::NeedsUpdate { .. }))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, service_account, role, environments) = self.key_ports(inputs)?;
        let key = doppler_project_member_key(&project, &service_account);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let (member_state, slug) =
            Self::analyze(&state, &project, &service_account, &role, &environments)?;
        match member_state {
            MemberState::Present => Ok(Ensured {
                outputs: Self::outputs_for(&project, &service_account),
                changed: false,
            }),
            MemberState::Mismatch => Err(Self::mismatch_conflict()),
            MemberState::Absent => {
                state
                    .doppler_project_members
                    .entry(doppler_project_key(&project))
                    .or_default()
                    .push(DopplerProjectMemberRecord {
                        slug,
                        role: role.as_str().to_string(),
                        access_all_environments: false,
                        environments: environments.iter().map(ToString::to_string).collect(),
                    });
                Ok(Ensured {
                    outputs: Self::outputs_for(&project, &service_account),
                    changed: true,
                })
            }
            MemberState::NeedsUpdate {
                existing_environments,
                access_all_environments,
            } => {
                let members = state
                    .doppler_project_members
                    .entry(doppler_project_key(&project))
                    .or_default();
                if let Some(member) = members.iter_mut().find(|member| member.slug == slug) {
                    member.role = role.as_str().to_string();
                    if !access_all_environments {
                        member.environments =
                            Self::union_environments(&existing_environments, &environments);
                    }
                }
                Ok(Ensured {
                    outputs: Self::outputs_for(&project, &service_account),
                    changed: true,
                })
            }
        }
    }
}
