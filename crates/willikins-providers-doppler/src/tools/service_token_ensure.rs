//! `doppler.service_token.ensure`: mints a real Doppler service token.
//! `read` (and `ensure` on an already-existing token) always report the
//! `token` output `Unknown`: a service token's value cannot be re-read
//! once issued — the list endpoint omits `key` entirely. `ensure`'s
//! *create* path is the one exception, matching
//! `willikins_providers_fake`'s tool of the same name.

use std::sync::Arc;

use willikins_core::rollback::{Side, Within};
use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Created, Deleted, Ensured, Inputs, Inverse, Observation, Outputs, Reversal, SinkToken,
    Standing, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName, TeardownToken};

use crate::client::{DopplerClient, MANAGED_DESCRIPTION, looks_like_a_missing_project};

/// `doppler.service_token.ensure`.
pub struct DopplerServiceTokenEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
    inverse: DopplerServiceTokenInverse,
}

impl DopplerServiceTokenEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("DopplerTokenName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("token"), scalar("DopplerServiceToken"));
        let inverse = DopplerServiceTokenInverse {
            client: Arc::clone(&client),
        };
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.service_token.ensure"),
                description: "Ensure a Doppler service token exists. Its value can never be re-read once issued.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            client,
            inverse,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, DopplerTokenName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }

    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::unknown(scalar("DopplerServiceToken")));
        outputs
    }

    /// Whether a token by `name` is already listed for `config`.
    ///
    /// **2026-09-16 defect, fixed here.** Doppler 404s the token-list
    /// endpoint when the `project` or `config` it was asked about does
    /// not exist yet — which, at plan time, is exactly the state before
    /// this workflow's own `doppler.project.ensure` and
    /// `doppler.config.ensure` nodes have run (the first live smoke run
    /// found this: planning the positive fixture against a fresh Doppler
    /// account failed outright at this node). "No parent yet" answers
    /// "is a token named `name` already listed?" the same way "an empty
    /// list" does, so a 404 here reads as `false`, exactly the way
    /// `doppler.config.ensure::observe` already reads a 404 as `Absent`
    /// two tools over — see `fixtures/doppler/README.md` for the full
    /// note and the recorded fixture. `ensure` shares this method with
    /// `read`, so the same tolerance applies to both; that is safe
    /// because a project or config still missing by *apply* time
    /// (ordering should already have created both) makes `ensure` fall
    /// through to the mint `POST` below, which then fails for real
    /// against the still-missing parent — nothing is silently swallowed,
    /// only the question this method answers changes from "does Doppler
    /// error" to "is a token already there".
    ///
    /// **What this 404 cannot tell apart.** Doppler documents no
    /// error-body schema for any non-2xx response at all (research note
    /// `docs/research/2026-09-12-m2-dependencies.md`, section 3, and its
    /// own open-questions list), so a 404 here is a bare status and
    /// nothing else. It certainly covers "no such project or config":
    /// that is the case the live smoke run met, and the one this arm
    /// exists for. What it cannot be shown *not* to cover is "a project
    /// or config this credential is not granted" — a Service Account
    /// token holds "a granular set of resources within your workplace"
    /// (same section), and another workplace's projects lie outside it
    /// entirely. **Which status Doppler answers for a project outside
    /// the grant has since been observed**, twice — by the 2026-09-16
    /// grant probe (`docs/research/2026-09-12-m2-dependencies.md`, "3.y
    /// Service-account access") and again on 2026-09-20: a `404`
    /// "Could not find requested project", byte-identical to a project
    /// that does not exist. Never a `403`. So this 404 provably covers
    /// both, and the conflation is not a worry to be recorded but a
    /// fact. GitHub's 404 is documented to conflate
    /// the two, which is why
    /// `willikins_providers_github::tools::GitHubRepoEnsure::observe`
    /// says so outright. For Doppler the honest statement is the weaker
    /// one: this status cannot *prove* absence, so nothing downstream
    /// may treat it as proof.
    ///
    /// [`Observation::Foreign`] is nonetheless the wrong answer for the
    /// ambiguity: `willikins_core::plan` turns `Foreign` into
    /// `PlanError::NameTaken` and refuses the whole plan, which is the
    /// 2026-09-16 defect over again for the ordinary "not created yet"
    /// case — the common one. Reading it `Absent` reaches nothing a `200`
    /// did not already reach: this tool has never had an ownership check
    /// of its own. The plan's ownership rule puts that on
    /// `doppler.project.ensure`'s `managed-by: willikins` marker, which
    /// refuses a project that is not ours and which the data dependency
    /// through `doppler.config.ensure` orders ahead of this tool in every
    /// workflow that derives its `config` — and in a workflow that binds
    /// a literal `DopplerConfig` instead there is no ownership gate at
    /// all, as true of an empty `200` listing as of this 404. Either way
    /// an empty `200` from a project that was not ours already fell
    /// through to the mint before this fix. A parent that is genuinely
    /// unreachable therefore fails one call later, at the mint
    /// `POST`, loudly and with Doppler's own status — pinned by
    /// `ensure_fails_when_the_parent_is_still_missing_at_apply`. A
    /// credential Doppler *does* answer about (401 or 403) never reaches
    /// this arm.
    ///
    /// **2026-09-20 defect, widened here.** Doppler also answers **`400`**
    /// for a project that is not there: the live write cycle's step 10
    /// recorded a `GET` of a just-deleted project answering `400` for one
    /// of its two projects and `404` for the other, inside a single run
    /// (`fixtures/doppler/README.md`, "Undocumented facts the cycle
    /// settled") — which the 2026-09-20 probe explains: step 10 deletes
    /// and re-reads one project at a time, so the first re-read happens
    /// while the second project is still visible (`400`) and the second
    /// after nothing is (`404`). Whether *this* endpoint answered the
    /// same `400` shape
    /// stayed unobserved for a while, so this tolerance was originally
    /// left exactly one status wide, pinned by
    /// `read_still_propagates_a_400_from_the_listing`.
    ///
    /// A live rehearsal planning a second project, in a workplace whose
    /// first project this token could already see, then hit
    /// exactly that `400` at `doppler.project.ensure`'s own `GET`, with
    /// the message "This token does not have access to requested project
    /// '<name>'" — the same fact
    /// `docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`
    /// records. [`looks_like_a_missing_project`] is the one predicate
    /// every read in this crate now shares for "the parent this call
    /// needed is not visible to this token right now", and this method
    /// uses it too: the tolerance is no longer one status, it is one
    /// *message* — a `400` naming anything else (a duplicate-create
    /// conflict elsewhere, or simply an unrelated failure) still fails,
    /// which `read_still_propagates_a_400_from_the_listing` still pins
    /// with its own, differently-worded `400` body.
    fn is_listed(
        &self,
        config: &DopplerConfig,
        name: &DopplerTokenName,
    ) -> Result<bool, ToolError> {
        match self
            .client
            .list_service_tokens(config.project(), config.name())
        {
            Ok(listed) => Ok(listed.iter().any(|entry| entry.name == name.as_str())),
            Err(err) if looks_like_a_missing_project(&err) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

impl Tool for DopplerServiceTokenEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        if self.is_listed(&config, &name)? {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        if self.is_listed(&config, &name)? {
            return Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            });
        }
        let created = self
            .client
            .create_service_token(config.project(), config.name(), &name)?;
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::known(created.key));
        Ok(Ensured {
            outputs,
            changed: true,
        })
    }

    /// Milestone 3m decisions 4, 5, task P3: deletable, riding out with
    /// whichever of `doppler.config.ensure`/`doppler.branch_config.ensure`
    /// produced this token's own `config` -- decision 4's table lists
    /// both, since a token can sit in either a root or a branch config.
    /// `needs` names only `config` and `name`: `config` already carries
    /// the project (`DopplerConfig::project`), and this tool holds no
    /// optional, document-bound credential port of its own.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["config", "name"],
            within: &[
                Within {
                    tool: "doppler.config.ensure",
                    ports: &[("config", Side::Output, "config")],
                },
                Within {
                    tool: "doppler.branch_config.ensure",
                    ports: &[("config", Side::Output, "config")],
                },
            ],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// [`Standing::NotOurs`]: the project's own `description` no longer
/// carries the [`MANAGED_DESCRIPTION`] marker.
const NOT_OURS_LOST_THE_MARKER: &str =
    "the project's description no longer carries the managed-by: willikins marker";

/// [`Standing::NotOurs`]: a token by this name exists, but its own
/// `created_at` falls outside this instance's creation window (decision
/// 2) -- including an entry that omitted `created_at` outright (fail
/// closed).
const NOT_OURS_OUTSIDE_WINDOW: &str =
    "the token's own creation date falls outside this run's creation window";

/// [`Standing::Drifted`]: more than one token is listed at this
/// `(config, name)` pair -- this inverse cannot tell which one this run
/// minted, so it deletes neither.
const DRIFTED_MULTIPLE_TOKENS: &str =
    "more than one service token is listed at this config and name";

/// [`crate::tool::helpers::conflict`]'s message when [`Inverse::delete`]
/// is called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`].
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the token has changed since this run created it, \
     or is no longer ours; the teardown will not delete it";

/// The `config`/`name` key ports, re-resolved from the journal (decision
/// 2): exactly `Created::inputs`, never read a second time from
/// anywhere else.
fn token_of(created: &Created<'_>) -> Result<(DopplerConfig, DopplerTokenName), ToolError> {
    let config = get(created.inputs, "config")?;
    let name = get(created.inputs, "name")?;
    Ok((config, name))
}

/// `doppler.service_token.ensure`'s inverse (milestone 3m task P3,
/// decisions 4, 7). Never a catalog tool in its own right, never
/// reachable except from `apply_teardown` (trust boundaries 1, 2).
struct DopplerServiceTokenInverse {
    client: Arc<DopplerClient>,
}

impl DopplerServiceTokenInverse {
    /// Decision 7's `doppler.service_token.ensure` row. Doppler's own
    /// list endpoint is the only read this tool has (no single-token
    /// `GET`), so standing and the slug `delete` needs are found
    /// together: `Ok((Standing::AsCreated { .. }, Some(slug)))` is the
    /// only case `delete` may act on; every other `Standing` carries
    /// `None`.
    ///
    /// **Zero matches reads `Gone`, not `NotOurs`.** The row's own
    /// wording ("no token ... created in the window") could be read
    /// either way, but `Gone` is the universal "this resource is
    /// already gone" reading every other inverse in this crate gives an
    /// absent resource (decision 6), and it is the only reading under
    /// which a second teardown of the same run (decision 8's resume
    /// case) still succeeds once this step's own first delete already
    /// removed it. `NotOurs` is reserved for a token that still exists
    /// by this name but was created outside the window -- a name this
    /// run's token shares with something else entirely. Recorded as an
    /// addendum to this milestone's plan.
    fn locate(&self, created: &Created<'_>) -> Result<(Standing, Option<String>), ToolError> {
        let (config, name) = token_of(created)?;

        match self.client.get_project(config.project()) {
            Err(err) if looks_like_a_missing_project(&err) => return Ok((Standing::Gone, None)),
            Err(err) => return Err(err.into()),
            Ok(body) if body.description.as_deref() != Some(MANAGED_DESCRIPTION) => {
                return Ok((
                    Standing::NotOurs {
                        why: NOT_OURS_LOST_THE_MARKER,
                    },
                    None,
                ));
            }
            Ok(_) => {}
        }

        let tokens = match self
            .client
            .list_service_tokens(config.project(), config.name())
        {
            Ok(tokens) => tokens,
            Err(err) if looks_like_a_missing_project(&err) => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        let matching: Vec<_> = tokens
            .iter()
            .filter(|entry| entry.name == name.as_str())
            .collect();

        match matching.as_slice() {
            [] => Ok((Standing::Gone, None)),
            [one] => {
                let in_window = one
                    .created_at
                    .is_some_and(|at| at >= created.window.start && at <= created.window.end);
                if in_window {
                    Ok((
                        Standing::AsCreated { losing: Vec::new() },
                        Some(one.slug.clone()),
                    ))
                } else {
                    Ok((
                        Standing::NotOurs {
                            why: NOT_OURS_OUTSIDE_WINDOW,
                        },
                        None,
                    ))
                }
            }
            _ => Ok((
                Standing::Drifted {
                    what: DRIFTED_MULTIPLE_TOKENS,
                },
                None,
            )),
        }
    }
}

impl Inverse for DopplerServiceTokenInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        Ok(self.locate(created)?.0)
    }

    fn delete(&self, created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        match self.locate(created)? {
            (Standing::Gone, _) => Ok(Deleted { already_gone: true }),
            (Standing::AsCreated { losing }, Some(slug))
                if losing.iter().all(|loss| {
                    created
                        .accept
                        .iter()
                        .any(|accepted| accepted.as_str() == *loss)
                }) =>
            {
                let (config, _name) = token_of(created)?;
                self.client
                    .delete_service_token(config.project(), config.name(), &slug)?;
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
        }
    }
}
