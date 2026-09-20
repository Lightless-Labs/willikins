//! `doppler.service_token.ensure`: mints a real Doppler service token.
//! `read` (and `ensure` on an already-existing token) always report the
//! `token` output `Unknown`: a service token's value cannot be re-read
//! once issued — the list endpoint omits `key` entirely. `ensure`'s
//! *create* path is the one exception, matching
//! `willikins_providers_fake`'s tool of the same name.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.service_token.ensure`.
pub struct DopplerServiceTokenEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
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
}
