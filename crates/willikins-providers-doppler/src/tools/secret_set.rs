//! `doppler.secret.set`: writes a real secret into a Doppler config. The
//! sink half of the `SigNoz` pair (`signoz.ingestion_key.ensure` mints;
//! this writes it somewhere the app actually reads) — and the first
//! willikins tool that writes a secret at all, which is why it exists
//! under a constraint no sink before it needed.
//!
//! # The provenance rule, and why it is load-bearing
//!
//! This crate already has `doppler.secret.get`, which *reads* a secret
//! from anywhere a workflow names. Put the two side by side and a
//! document `secret.get(someone-elses-project/prd, DATABASE_URL) ->
//! secret.set(a-config-I-can-read, X)` moves a secret from one place to
//! another the document's author chooses — and every existing gate would
//! be green, because `check` already permits a secret to bind to a
//! secret-accepting port, which is exactly this edge. The one thing
//! standing between that and an exfiltration primitive is the invariant
//! that workflow documents are privileged content, run only from a
//! trusted ref — and the operator's own stated direction is to relax
//! exactly that invariant for agent-composed workflows. So this sink
//! ships *with* a narrower rule, enforced by `check` rather than left to
//! the trusted-ref invariant alone: **the `config` port may only be
//! bound to the output of an earlier, non-pure node — never a literal,
//! a workflow input, a `for_each` item, or a pure node's output.**
//!
//! A document may therefore write a secret only into a config *the same
//! graph produced* (typically `doppler.config.ensure`'s own output),
//! never one it names by fiat or receives as a caller-supplied input.
//! `PortSpec::derived_only` is the mechanism (`willikins_core::check`'s
//! own module docs and `CheckError::UnderivedBinding` carry the full
//! reasoning, including why a *pure* node's output is refused too — the
//! short version: a pure tool is evaluated from literals at plan time,
//! so accepting its output here would let a document launder a literal
//! through a passthrough node and defeat the whole restriction).
//! `workflows/fixtures/secret-set-literal-config.yaml` and
//! `secret-set-input-config.yaml` are the negative fixtures; this
//! crate's own `tests/secret_set_provenance.rs` is the acceptance test.
//!
//! This rule does not, on its own, close the exfiltration path
//! completely: a document can still chain `doppler.config.ensure` (a
//! real, cheap, `Reversible` node any document can run) immediately
//! before `secret.set`, producing a config this graph "made" in exactly
//! the sense the rule asks for, and then write into it. What the rule
//! *does* do is remove the zero-cost version of the attack (naming an
//! existing, already-populated config directly) and make every write
//! traceable to a node the plan shows creating or naming that specific
//! config — the plan a human approves says which config, not only that
//! *a* config was written to. Closing the rest is the trusted-ref
//! invariant's job, not this port's.
//!
//! # What `read` reports, and why
//!
//! **This tool's `read` reports `Present`/`Absent` by existence alone —
//! never by comparing values — which is a narrower claim than
//! `doppler.service_token.rotate`'s "always `Absent`", and deliberately
//! so.** The two are not analogous: `rotate` has no upstream secret port
//! at all, so it can afford to never plan as `Action::NoOp` — a human
//! approves its `Destructive` rotation every time, by design. `secret.set`
//! has a required `value` port that, in the shape this tool exists to
//! serve, is fed by a resolver like `signoz.ingestion_key.ensure`, whose
//! own `read`/converging `ensure` report that value `Unknown` once
//! minted (that tool's own module docs). Trace a second `apply` of the
//! same document: the resolver's node re-plans `NoOp` and its output
//! stays `Unknown`; `secret.set`'s `value` input therefore arrives
//! `Unknown` too. `willikins_core::apply`'s executor
//! (`first_unknown_required_input`, `ApplyError::UnknownInput`) treats an
//! `Unknown` required input as fatal on any node whose planned action is
//! *not* `Action::NoOp` — it only converges quietly on a `NoOp` node.
//! Reporting `Absent` unconditionally, as an earlier draft of this tool
//! did, plans this node `Action::Create` every time regardless of
//! upstream state, so the *second* apply of any workflow chaining a
//! minting resolver into this sink would hard-fail with `UnknownInput`
//! instead of converging — breaking the exact pair this tool exists to
//! make work. `github.actions_secret.ensure` (whose own `value` port has
//! the identical "fed by an unrereadable mint" shape) already solves this
//! the same way: `read` reports existence, not value equality, precisely
//! so a `NoOp` plan can swallow the `Unknown` (see the milestone 2 smoke
//! run recorded in `docs/HANDOFF.md`: "`ci_secret` `Converged` ... token
//! output `Unknown`").
//!
//! So: `read` calls [`DopplerClient::get_secret`] (the same lookup
//! `doppler.secret.get` makes) and reports `Present` when a value comes
//! back, `Absent` when it does not — the looked-up
//! [`willikins_types::DopplerSecretValue`] is immediately dropped,
//! unexposed, never compared against the `value` port's own content
//! (which may itself be `Unknown` at the very moment `read` runs). This
//! is existence-by-name, exactly [`crate::tools::DopplerSecretGet`]'s own
//! `NotFound` distinction, reused rather than re-invented. A config
//! `read` finds absent (the common case: the provenance rule above
//! forces `config` to come from a `doppler.config.ensure` node in the
//! *same* graph, so the config itself is typically freshly created too)
//! reports `Absent` through [`crate::client::looks_like_a_missing_project`],
//! the same tolerance every other Doppler tool in this crate gives a
//! parent not yet created at plan time. `ensure` is unchanged: it always
//! writes and always reports `changed: true` when called — Doppler's own
//! upsert semantics (`POST /v3/configs/config/secrets`, one key merged
//! into whatever the config already holds) make a repeated write cheap
//! and side-effect-free beyond "the value is now this", so nothing is
//! lost by never comparing the value itself, only by not pretending a
//! `NoOp` plan proves equality it cannot check.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    any_secret, exact, exact_derived_only, get, invalid, port, require_present, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.secret.set`.
pub struct DopplerSecretSet {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerSecretSet {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact_derived_only("DopplerConfig"));
        inputs.insert(port("name"), exact("SecretName", true));
        inputs.insert(port("value"), any_secret(true));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.secret.set"),
                description: "Write a secret into a Doppler config. `config` must be the output of an earlier node — never a literal or a workflow input.".to_string(),
                inputs,
                outputs: indexmap::IndexMap::new(),
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, SecretName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }
}

impl Tool for DopplerSecretSet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        match self
            .client
            .get_secret(config.project(), config.name(), &name)
        {
            Ok(Some(_value)) => Ok(Observation::Present(Outputs::new())),
            Ok(None) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn ensure(&self, inputs: &Inputs, token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let value = inputs
            .get(&port("value"))
            .ok_or_else(|| invalid("port `value` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `value` is unknown"));
        }
        let object = value
            .as_scalar()
            .ok_or_else(|| invalid("port `value` must be a scalar secret"))?;
        // The one place in this crate the plaintext exists: read here,
        // handed straight to the client, dropped immediately after.
        // Never `Debug`-formatted, never placed in a `ToolError`. Unlike
        // `github.actions_secret.ensure`, no client-side sealing step
        // sits between -- Doppler's own endpoint takes the value directly
        // over TLS, which is the transport boundary this crate's design
        // already trusts for every other Doppler write.
        let plaintext = object.expose(token);
        self.client
            .set_secret(config.project(), config.name(), &name, &plaintext)?;
        drop(plaintext);
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};

    fn tool() -> DopplerSecretSet {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        DopplerSecretSet::new(Arc::new(DopplerClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_config_and_name() {
        assert_eq!(tool().spec().key, vec![port("config"), port("name")]);
    }

    #[test]
    fn spec_config_port_is_derived_only() {
        assert!(
            tool()
                .spec()
                .inputs
                .get(&port("config"))
                .expect("config port exists")
                .derived_only
        );
    }

    #[test]
    fn spec_value_port_accepts_any_secret() {
        assert_eq!(
            tool().spec().inputs.get(&port("value")).unwrap().ty,
            willikins_core::PortType::AnySecret
        );
    }
}
