//! `doppler.secret_name.gate`: a gate over "this secret **name** is
//! visible in this Doppler config, set there directly or inherited from
//! a config it inherits" (milestone 3j, decisions (b1)-(b4)).
//!
//! # Why this exists
//!
//! The operator's words (2026-10-03), on an earlier design that only
//! made the operator acknowledge a key by hand: "It shouldn't. It should
//! check." This gate is that check: it proves a secret's *name* is
//! reachable from a config, so a document can order a step that needs
//! the key after proving the name is there, instead of taking the
//! operator's word for it.
//!
//! # Names, never values (trust boundary 1)
//!
//! This tool calls only [`DopplerClient::secret_name_listed`] (`GET
//! /v3/configs/config/secrets/names`) and [`DopplerClient::get_config`]
//! (`GET /v3/configs/config`, to read `inherits`). It never reads a
//! secret's value, and never asks Doppler to issue a dynamic-secret
//! lease — `tests/secret_name_gate_mock.rs`'s source guard greps this
//! very file for exactly the endpoint shapes and query flag a
//! value-reading call would use. The other names listed in a config are
//! the operator's own layout and never reach an output, a
//! [`ToolError`], `tracing`, or a panic message (trust boundary 2) —
//! see [`DopplerClient::secret_name_listed`]'s own doc for how its
//! response type enforces that at the client layer; this tool only ever
//! sees the one `bool` it reduces to.
//!
//! # (b2) What `observe` does
//!
//! One shared `observe(config, name)`, read by both `read` and `ensure`:
//!
//! 1. [`DopplerClient::secret_name_listed`] on `config` directly.
//!    `Ok(true)` → `Present`. `Ok(false)` → step 2. An error that
//!    [`looks_like_a_missing_project`] → `Absent`, with no walk at all
//!    (decision (b3), below). Any other error → `Err`.
//! 2. [`DopplerClient::get_config`] on `config`, then for each entry of
//!    its `inherits` array, in order, `secret_name_listed` on that base.
//!    The first `Ok(true)` → `Present`, and the remaining bases are
//!    never read (a document's base list can be long; the gate stops as
//!    soon as it has its answer). `Ok(false)`, or an error that
//!    `looks_like_a_missing_project`, means that base contributes
//!    nothing, and the walk continues to the next one. Any other error →
//!    `Err`. If every base has been read and none listed the name (or
//!    `inherits` was empty or absent), or if `get_config` itself answers
//!    the missing-project shape → `Absent`.
//!
//! `Present` and `Absent` both carry `config` as their one output, like
//! [`crate::tools::DopplerConfigInheritableGate`]. `ensure` re-observes
//! and always returns `changed: false`: a gate never writes.
//!
//! # (b3) Deviation: `Absent` for the ambiguous pair, `Err` for the rest, never `Foreign`
//!
//! The milestone brief asked for "`Foreign` or an error for a config it
//! cannot read". This tool deviates for one pair of answers. On a fresh
//! run, the app's own document creates the project and the deployment
//! branch config in the same plan, so this gate's first-ever `read` sees
//! a config that does not exist *yet*. Doppler answers that with the
//! same `404`, or `400` "does not have access to requested project",
//! that it gives for a project that exists but sits outside this token's
//! grant — the two cannot be told apart from the response alone
//! ([`looks_like_a_missing_project`]'s own doc;
//! `docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
//! A hard `PlanError` there would make every fresh document unplannable,
//! so the pair reads `Absent` instead: the run reports a blocked gate,
//! and the gate's own `how` names both readings so a human is not left
//! guessing which one applies. This mirrors
//! [`crate::tools::DopplerConfigInheritableGate`]'s own
//! `read_reports_absent_when_this_token_cannot_see_the_project` precedent
//! exactly.
//!
//! Everything Doppler *can* tell apart from "does not exist yet" fails
//! loudly instead: `401`, `403`, any other `4xx`, `5xx`, a transport
//! failure, or a malformed `2xx` body. `Foreign` is never returned by
//! this tool at all — `Foreign` would make `plan_one` report
//! `PlanError::NameTaken` ("name taken by a resource this workflow did
//! not create"), which is meaningless for a read-only observation of
//! someone else's config: this gate never creates anything, so there is
//! no resource of its own a listed name could collide with.
//!
//! # (b4) Correct under either answer to "does the names endpoint list inherited names?"
//!
//! Nothing in Doppler's own reference
//! (`https://docs.doppler.com/reference/secrets-names.md`) says whether
//! `GET /v3/configs/config/secrets/names` lists a name inherited from a
//! base config (milestone 3j, verify item 1). Step 2's walk makes this
//! gate correct either way. The live names cycle settled it on 2026-10-04:
//! the endpoint does list inherited names, so the walk is a backstop.
//! Both answers, as written before it was settled:
//!
//! - **If the endpoint lists inherited names,** step 1 already answers
//!   `Present` for an inherited key, and the walk only ever runs when the
//!   key is visible nowhere — so it reads each base's names and finds
//!   nothing there either, because a name in a base the config actually
//!   inherits would already have been listed in step 1. The walk cannot
//!   produce a `Present` that step 1 missed; its only cost under this
//!   answer is one extra read per base when the key is genuinely absent.
//! - **If the endpoint omits inherited names,** step 1 answers `false`
//!   for an inherited key, and the walk finds it in the base the config
//!   *actually* inherits — read from Doppler's own `inherits` array on
//!   the config object, never from a document's stated intent. That is
//!   what makes this gate answer `Present` at all under this answer.
//!
//! The walk always reads the config's real inheritance, never a
//! document's intent: a name in a base a document merely *asks*
//! `doppler.config.inherits.ensure` to add, before that write has
//! applied, is not visible yet and reads `Absent` — which is also why
//! this gate's `how` tells a blocked run to re-run once the inheritance
//! it is waiting on has been set.
//!
//! **One level only.** The walk reads exactly the bases named in
//! `config`'s own `inherits` array; it does not recurse into any of
//! *their* `inherits`. An inheritable config that itself inherits would
//! therefore hide a name that lives two levels up — whether Doppler
//! permits a base to itself inherit is unverified (verify item 2). If it
//! does, this gate reads `Absent` for such a key: a false block, never a
//! false `Present`, because the only way to reach `Present` is an actual
//! `Ok(true)` from `secret_name_listed`. The fix, if ever needed, is
//! walking further — a change reviewed on its own, not a silent widening
//! here.
//!
//! **Access cost.** The walk needs read access to each base config,
//! which a plain reader of `config` does not otherwise need (milestone
//! 3h's probe). Any document that inherits a base already needs that
//! same access to set the inheritance
//! ([`crate::tools::DopplerConfigInheritsEnsure`]) and to pass
//! [`crate::tools::DopplerConfigInheritableGate`] on it, so the walk asks
//! for nothing new. A base this token cannot see simply contributes
//! nothing to the walk and the gate blocks, rather than failing.
//!
//! # A gate never writes
//!
//! `ensure` calls the same `observe` `read` does and always answers
//! `changed: false`, on both `Present` and `Absent` — [`SinkToken`] is
//! still required by the trait, but nothing in this tool ever reaches a
//! write.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_providers_http::ProviderError;
use willikins_types::{DopplerConfig, SecretName};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// This tool's gate: both ports are the subject, so a blocked report
/// names exactly which config lacks which name.
static GATE: Gate = Gate {
    need: "this secret name is visible in this Doppler config, set there or inherited from a config it inherits",
    how: "store the secret in this config, or in an inheritable config this one inherits (doppler.config.inherits.ensure); if this run also changed what this config inherits, re-run: a gate observes before the run applies; a config that does not exist yet, or one this token cannot see (Doppler answers both the same way), also blocks here until it is created or this token is granted read access",
    subject: &["config", "name"],
};

/// `doppler.secret_name.gate`, registered in the live and fake catalogs.
pub struct DopplerSecretNameGate {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerSecretNameGate {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.secret_name.gate"),
                description: "A gate: whether a secret name is visible in a Doppler config, set there or inherited."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            client,
        }
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// Maps a [`ProviderError`] this tool's own reads produced into a
    /// [`ToolError`], once the caller has already ruled out
    /// [`looks_like_a_missing_project`]. A `2xx` whose body failed to
    /// parse (`err.status` in `200..300`) becomes a
    /// [`ToolErrorKind::Provider`] naming `config` and `name` only, never
    /// the response text — the same shape
    /// `DopplerSecretGet::lookup`'s malformed-body arm uses, and for the
    /// same reason: without it, the only thing an operator would see is
    /// [`willikins_providers_http::Http::finish`]'s deliberately
    /// content-free parse-failure message, which says nothing about
    /// *which* config or name was being checked. Every other status (a
    /// real `4xx`/`5xx`, or a transport failure) is left exactly as
    /// [`ToolError::from`] already renders it, carrying the provider's
    /// own bounded message.
    fn provider_failure(
        config: &DopplerConfig,
        name: &SecretName,
        err: ProviderError,
    ) -> ToolError {
        if err
            .status
            .is_some_and(|status| (200..300).contains(&status))
        {
            ToolError {
                kind: ToolErrorKind::Provider,
                message: format!(
                    "reading whether `{name}` is visible in `{config}`: the response body did not parse"
                ),
            }
        } else {
            ToolError::from(err)
        }
    }

    /// Shared by `read` and `ensure` (decision (b2)).
    fn observe(&self, config: &DopplerConfig, name: &SecretName) -> Result<Observation, ToolError> {
        match self
            .client
            .secret_name_listed(config.project(), config.name(), name)
        {
            Ok(true) => return Ok(Observation::Present(Self::outputs_for(config))),
            Ok(false) => {}
            Err(err) if looks_like_a_missing_project(&err) => {
                return Ok(Observation::Absent {
                    predicted: Self::outputs_for(config),
                });
            }
            Err(err) => return Err(Self::provider_failure(config, name, err)),
        }

        let body = match self.client.get_config(config.project(), config.name()) {
            Ok(body) => body,
            Err(err) if looks_like_a_missing_project(&err) => {
                return Ok(Observation::Absent {
                    predicted: Self::outputs_for(config),
                });
            }
            Err(err) => return Err(Self::provider_failure(config, name, err)),
        };

        for base in body.inherits.into_iter().flatten() {
            match self
                .client
                .secret_name_listed(&base.project, &base.config, name)
            {
                Ok(true) => return Ok(Observation::Present(Self::outputs_for(config))),
                Ok(false) => {}
                Err(err) if looks_like_a_missing_project(&err) => {}
                Err(err) => return Err(Self::provider_failure(config, name, err)),
            }
        }

        Ok(Observation::Absent {
            predicted: Self::outputs_for(config),
        })
    }
}

impl Tool for DopplerSecretNameGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        self.observe(&config, &name)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // See `DopplerConfigInheritableGate::ensure`'s own doc: a gate is
        // pure, so `apply` never reaches this while the node is
        // `Action::Blocked`.
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        match self.observe(&config, &name)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("this gate's own observe never returns {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};

    fn client_against(url: String) -> Arc<DopplerClient> {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let http = Http::new(url, Vec::new(), credential);
        Arc::new(DopplerClient::new(http))
    }

    fn tool_against(url: String) -> DopplerSecretNameGate {
        DopplerSecretNameGate::new(client_against(url))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure_and_reversible() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let spec = tool.spec();
        assert!(spec.key.is_empty());
        assert!(spec.pure);
        assert_eq!(spec.class, Class::Reversible);
    }

    #[test]
    fn declares_a_gate_over_config_and_name() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["config", "name"]);
    }

    #[test]
    fn a_catalog_accepts_this_gate() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(tool)).unwrap();
    }
}
