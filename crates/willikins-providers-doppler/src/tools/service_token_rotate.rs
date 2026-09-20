//! `doppler.service_token.rotate`: revokes and re-mints a real Doppler
//! service token. [`Class::Destructive`] — the highest class, since
//! revoking a token that is still in use elsewhere breaks whatever
//! depended on the old bytes. Port table identical to
//! `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! `read` always reports [`Observation::Absent`]: see this crate's own
//! module docs (`src/lib.rs`) for why, and how that differs from the
//! plan's literal port-table wording without changing what a live apply
//! does.
//!
//! If a listed token's `DELETE` fails, this tool mints nothing: a
//! rotation that fails to revoke the old token but still hands out a new
//! one is the opposite of a rotation, and Doppler documents no error-body
//! schema that would let this crate tell "already revoked" apart from a
//! genuine failure. The old token is left in place and the whole call
//! fails; a caller retries.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.service_token.rotate`.
pub struct DopplerServiceTokenRotate {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerServiceTokenRotate {
    /// Build the tool against `client`, constructing its spec. Same ports
    /// as `doppler.service_token.ensure`: `config`, `name` in, `token`
    /// out.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("DopplerTokenName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("token"), scalar("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.service_token.rotate"),
                description: "Revoke and re-mint a Doppler service token.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Destructive,
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
}

impl Tool for DopplerServiceTokenRotate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, _name) = self.key_ports(inputs)?;
        // Performed so a bad credential, or a genuine provider failure,
        // fails at plan time, but its result never changes the
        // observation — see this crate's module docs (`src/lib.rs`) for
        // why a rotate must never plan as `NoOp`.
        //
        // **2026-09-16 defect, fixed here.** A 404 is tolerated, not
        // propagated: Doppler 404s this same listing endpoint when the
        // `project` or `config` does not exist yet, which at plan time is
        // exactly the state before this workflow's own
        // `doppler.project.ensure`/`doppler.config.ensure` nodes have
        // run — the same "no parent yet" case
        // `doppler.service_token.ensure`'s `is_listed` was fixed for
        // (see that fix's doc comment and `fixtures/doppler/README.md`).
        // Since the observation is `Absent` either way, there is nothing
        // to compute from the listing beyond "did the call fail for a
        // real reason" — a bad credential (401/403) or a 5xx still fails
        // here, pinned by `read_still_propagates_a_listing_failure`.
        //
        // The 404 carries the same ambiguity `is_listed`'s doc spells
        // out — it cannot prove the parent is merely absent rather than
        // outside this credential's grant, a case whose status Doppler
        // has never been observed to give.
        //
        // **2026-09-20 defect, widened here.** The tolerance shares
        // `is_listed`'s own fix: a live rehearsal planning a second
        // project, in a workplace whose first project this token could
        // already see, found the identical missing-parent `GET`
        // answering `400` "This token does not have access to requested
        // project" rather than `404`
        // (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
        // `looks_like_a_missing_project` reads both the same way. Here it
        // costs even less than in `is_listed`: the observation is
        // `Absent` either way, so tolerating either shape changes only
        // whether the plan is refused, never what the plan says this
        // `Destructive` step will do. `read` still never reports
        // `Present`, so this step can never plan as `Action::NoOp`
        // (`read_always_reports_absent_even_when_a_token_is_listed`).
        // `ensure`'s own listing call below stays strict, this predicate
        // included: a rotate that cannot list the tokens it is about to
        // revoke must not mint a replacement
        // (`ensure_still_fails_outright_on_a_listing_404`).
        match self
            .client
            .list_service_tokens(config.project(), config.name())
        {
            Ok(_) => {}
            Err(err) if looks_like_a_missing_project(&err) => {}
            Err(err) => return Err(err.into()),
        }
        Ok(Observation::Absent {
            predicted: Self::unknown_outputs(),
        })
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let listed = self
            .client
            .list_service_tokens(config.project(), config.name())?;
        for entry in listed.iter().filter(|entry| entry.name == name.as_str()) {
            self.client
                .delete_service_token(config.project(), config.name(), &entry.slug)?;
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
