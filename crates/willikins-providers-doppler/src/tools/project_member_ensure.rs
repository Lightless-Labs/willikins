//! `doppler.project_member.ensure`: makes a named Doppler service account
//! a member of a project with a given role on given environments. Port
//! table and behaviour identical to `willikins_providers_fake`'s tool of
//! the same name (`tests/catalog_parity.rs` pins the two `ToolSpec`s
//! equal). Milestone 3h task D3, decision (a).
//!
//! # Add or raise, never remove
//!
//! This tool's `ensure` only ever `POST`s a brand-new member or `PATCH`es
//! an existing one's role upward and its environment set wider -- never a
//! `DELETE`, never a role lowered, never an environment dropped (trust
//! boundaries 1-2 of the milestone 3h plan). A member already at or above
//! the requested role and environments reads `Present` and is left alone;
//! one whose role this tool would have to *lower* to match, or whose role
//! this tool cannot even express (`admin`, `owner`, a custom identifier),
//! reports [`Observation::Mismatch`] and `ensure` refuses with `Conflict`
//! rather than silently doing nothing or silently overreaching.
//!
//! # `Absent` covers two different real states
//!
//! [`Observation::Absent`] here means either "no member at this slug at
//! all" (`ensure` then `POST`s) or "a member exists, but at a role or
//! environment set this tool must still raise" (`ensure` then `PATCH`es).
//! `read` cannot tell the two apart from the outside -- the resource's
//! *identity* is the pair `(project, service_account)`, not the triple
//! `(project, service_account, role)`, so a member present at a lower
//! role is not "a different resource not yet created" the way a
//! `Doppler` project's own 404 is. [`Tool::updates`] is this crate's
//! declared answer to that gap (milestone 3h decision (b),
//! `willikins_core::tool::Tool::updates`'s own doc): `true` for exactly
//! the two rows of decision (a)'s table that need a `PATCH`, so
//! `crate::plan::plan` plans [`willikins_core::plan::Action::Update`]
//! instead of `Create` for them, while every other tool (whose `updates`
//! still defaults to `false`) is unaffected.
//!
//! [`MemberState`] is this module's own, richer answer -- computed once
//! by [`DopplerProjectMemberEnsure::analyze`] and shared by `read`,
//! `updates`, and `ensure` -- so the POST/PATCH choice in `ensure` is
//! never re-derived from the coarser [`Observation`] it also reports.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, invalid, list, not_found, port, require_present, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerProject, DopplerProjectRole, DopplerServiceAccountName, EnvironmentSlug,
};

use crate::client::{DopplerClient, DopplerSlug, ProjectMemberEntry, looks_like_a_missing_project};

/// The greatest number of distinct environments one `ensure` call may
/// request -- SHARED VALUES' own bound, refused by shape before any
/// request is made.
const MAX_ENVIRONMENTS: usize = 16;

/// `doppler.project_member.ensure`.
pub struct DopplerProjectMemberEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

/// Where a role identifier -- this tool's own [`DopplerProjectRole`]
/// request, or whatever bare string a listed member's `role.identifier`
/// holds -- ranks for deciding whether it already satisfies a request,
/// needs raising, or cannot be expressed by this tool at all (decision
/// (a)'s "Role order"). `None` is "unrankable": `admin`, `owner`, or any
/// custom role identifier a workplace may have defined.
fn role_rank(role: &str) -> Option<u8> {
    match role {
        "no_access" => Some(0),
        "viewer" => Some(1),
        "collaborator" => Some(2),
        _ => None,
    }
}

/// The richer answer [`DopplerProjectMemberEnsure::analyze`] computes,
/// from which `read`, `updates`, and `ensure` each derive their own
/// narrower report. See the module doc for why `ensure` needs this
/// instead of branching on [`Observation`] alone.
enum MemberState {
    /// No member at this slug exists yet. `ensure` `POST`s.
    Absent,
    /// A member exists, already at or above the requested role and
    /// environments. `ensure` changes nothing.
    Present,
    /// A member exists, but its role must be raised, its environments
    /// widened, or both. Carries the member's *actual* environments and
    /// `access_all_environments`, exactly as listed -- what `ensure`
    /// unions with the request before `PATCH`ing, so an environment the
    /// member already has that this request never named is still kept
    /// (trust boundary 2). `ensure` `PATCH`es.
    NeedsUpdate {
        existing_environments: Vec<String>,
        access_all_environments: bool,
    },
    /// A member exists at a role this tool would have to lower to match
    /// (`collaborator` when `viewer` was asked), or at a role this tool
    /// cannot express at all (`admin`, `owner`, a custom identifier).
    /// `ensure` refuses with `Conflict`; `plan` reports
    /// `PlanError::AttributeMismatch`.
    Mismatch,
}

impl DopplerProjectMemberEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
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
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(
            port("project"),
            willikins_core::tool::helpers::scalar("DopplerProject"),
        );
        outputs.insert(
            port("service_account"),
            willikins_core::tool::helpers::scalar("DopplerServiceAccountName"),
        );
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.project_member.ensure"),
                description: "Ensure a Doppler service account is a member of a project with \
                               at least a given role on given environments."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("service_account")],
                class: Class::Reversible,
                pure: false,
            },
            client,
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

    /// Read a required `list<EnvironmentSlug>` input by its port name.
    /// See `doppler.config.inherits.ensure`'s identical helper for why
    /// this is not shared through `willikins_core::tool::helpers::get`.
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

    /// Shape, before any request (decision (a), step 1): 1-16 entries,
    /// no two naming the same environment.
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

    /// Resolve `name` to the one Doppler service account slug it
    /// identifies (decision (a), step 2). Never retried: this is a `GET`,
    /// and its own paging already bounds how long it will run.
    ///
    /// # Errors
    ///
    /// `ToolErrorKind::NotFound` for zero matches, `Conflict` for more
    /// than one, and whatever [`DopplerClient::list_service_accounts`]
    /// itself reports (already remapped on a `403`) for anything else.
    fn resolve_slug(&self, name: &DopplerServiceAccountName) -> Result<DopplerSlug, ToolError> {
        let accounts = self.client.list_service_accounts()?;
        let mut matches: Vec<DopplerSlug> = accounts
            .into_iter()
            .filter(|account| account.name == name.as_str())
            .map(|account| account.slug)
            .collect();
        match matches.len() {
            0 => Err(not_found(format!(
                "no Doppler service account in this workplace is named `{name}`"
            ))),
            1 => Ok(matches.remove(0)),
            n => Err(conflict(format!(
                "`{n}` service accounts are named `{name}`; this tool will not guess between \
                 them; rename until the name is unique"
            ))),
        }
    }

    /// Decide this member's [`MemberState`] against an already-fetched
    /// listing (decision (a), step 4).
    fn classify(
        members: &[ProjectMemberEntry],
        slug: &DopplerSlug,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
    ) -> MemberState {
        let Some(member) = members
            .iter()
            .find(|member| member.member_type == "service_account" && member.slug == *slug)
        else {
            return MemberState::Absent;
        };
        // `DopplerProjectRole`'s grammar admits exactly `viewer` and
        // `collaborator`, both rankable -- so this is a bug in the type,
        // not in caller input, if it ever fired.
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

    /// Resolve the slug, list the project's members, and classify this
    /// member's state -- shared by `read`, `updates`, and `ensure`. The
    /// slug is returned alongside the state so `ensure` never resolves
    /// the name a second time.
    fn analyze(
        &self,
        project: &DopplerProject,
        service_account: &DopplerServiceAccountName,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
    ) -> Result<(MemberState, DopplerSlug), ToolError> {
        let slug = self.resolve_slug(service_account)?;
        let state = match self.client.list_project_members(project) {
            Ok(members) => Self::classify(&members, &slug, role, environments),
            Err(err) if looks_like_a_missing_project(&err) => MemberState::Absent,
            Err(err) => return Err(err.into()),
        };
        Ok((state, slug))
    }

    /// The sorted union of `existing` (a member's actual environments, as
    /// Doppler listed them -- possibly outside this crate's own
    /// [`EnvironmentSlug`] grammar; see
    /// [`crate::client::ProjectMemberEntry`]'s own doc) and `requested`.
    /// Refuses, rather than silently dropping, an existing environment
    /// this tool cannot re-express as an [`EnvironmentSlug`] on the way
    /// back out: trust boundary 2 forbids ever narrowing a member's
    /// access, and a `PATCH` that quietly left an unparseable entry out
    /// of the union would do exactly that.
    ///
    /// # Errors
    ///
    /// `ToolErrorKind::Conflict` naming the environment this tool cannot
    /// safely include.
    fn union_environments(
        existing: &[String],
        requested: &[EnvironmentSlug],
    ) -> Result<Vec<EnvironmentSlug>, ToolError> {
        let mut union: BTreeSet<String> = existing.iter().cloned().collect();
        union.extend(requested.iter().map(ToString::to_string));
        union
            .into_iter()
            .map(|raw| {
                EnvironmentSlug::parse(&raw).map_err(|_| {
                    conflict(format!(
                        "this member already has an environment (`{raw}`) this tool cannot \
                         represent, so it will not patch its environments without risking \
                         dropping it; change it by hand"
                    ))
                })
            })
            .collect()
    }

    fn mismatch_conflict() -> ToolError {
        conflict(
            "this member holds a role this tool would have to lower or cannot express; it \
             changes neither; change it by hand or ask for that role",
        )
    }

    /// Re-read after a failed write (decision (a)'s own deferral, the
    /// same one every other `ensure` in this crate relies on): `Present`
    /// means the write landed despite the ambiguous failure, so this
    /// reports `changed: false` rather than the original error; anything
    /// else returns `err` unchanged.
    fn reread_after_write_failure(
        &self,
        project: &DopplerProject,
        service_account: &DopplerServiceAccountName,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
        err: ToolError,
    ) -> Result<Ensured, ToolError> {
        let (state, _slug) = self.analyze(project, service_account, role, environments)?;
        match state {
            MemberState::Present => Ok(Ensured {
                outputs: Self::outputs_for(project, service_account),
                changed: false,
            }),
            MemberState::Absent | MemberState::NeedsUpdate { .. } | MemberState::Mismatch => {
                Err(err)
            }
        }
    }
}

impl Tool for DopplerProjectMemberEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, service_account, role, environments) = self.key_ports(inputs)?;
        let (state, _slug) = self.analyze(&project, &service_account, &role, &environments)?;
        Ok(match state {
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
        let (state, _slug) = self.analyze(&project, &service_account, &role, &environments)?;
        Ok(matches!(state, MemberState::NeedsUpdate { .. }))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, service_account, role, environments) = self.key_ports(inputs)?;
        let (state, slug) = self.analyze(&project, &service_account, &role, &environments)?;
        match state {
            MemberState::Present => Ok(Ensured {
                outputs: Self::outputs_for(&project, &service_account),
                changed: false,
            }),
            MemberState::Mismatch => Err(Self::mismatch_conflict()),
            MemberState::Absent => {
                match self
                    .client
                    .add_project_member(&project, &slug, &role, &environments)
                {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&project, &service_account),
                        changed: true,
                    }),
                    Err(err) => self.reread_after_write_failure(
                        &project,
                        &service_account,
                        &role,
                        &environments,
                        err.into(),
                    ),
                }
            }
            MemberState::NeedsUpdate {
                existing_environments,
                access_all_environments,
            } => {
                let envs = if access_all_environments {
                    None
                } else {
                    Some(Self::union_environments(
                        &existing_environments,
                        &environments,
                    )?)
                };
                match self
                    .client
                    .update_project_member(&project, &slug, &role, envs.as_deref())
                {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&project, &service_account),
                        changed: true,
                    }),
                    Err(err) => self.reread_after_write_failure(
                        &project,
                        &service_account,
                        &role,
                        &environments,
                        err.into(),
                    ),
                }
            }
        }
    }
}
