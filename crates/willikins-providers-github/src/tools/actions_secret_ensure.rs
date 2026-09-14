//! `github.actions_secret.ensure`: creates or updates a real GitHub
//! Actions repository secret. Port table and behaviour identical to
//! `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal). Never
//! reads, stores, or returns the secret's value: GitHub's own `GET`
//! endpoint cannot answer with it either.
//!
//! The plaintext exists in this process only between
//! [`willikins_types::DomainObject::expose`] and
//! [`crate::seal::seal`], inside [`GitHubActionsSecretEnsure::ensure`]:
//! it is never logged, never placed in an error message, and never
//! written to a fixture.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    any_secret, exact, get, invalid, port, require_present, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::{ActionsSecretName, GitHubRepo};

use crate::client::{GitHubClient, to_tool_error};

/// `github.actions_secret.ensure`.
pub struct GitHubActionsSecretEnsure {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
}

impl GitHubActionsSecretEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("name"), exact("ActionsSecretName", true));
        inputs.insert(port("value"), any_secret(true));
        Self {
            spec: ToolSpec {
                name: tool_name("github.actions_secret.ensure"),
                description: "Ensure a GitHub Actions repository secret exists. Never reads or stores its value.".to_string(),
                inputs,
                outputs: indexmap::IndexMap::new(),
                key: vec![port("repo"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    /// Both key ports, validated and typed.
    fn key_ports(&self, inputs: &Inputs) -> Result<(GitHubRepo, ActionsSecretName), ToolError> {
        require_present(&self.spec, inputs)?;
        let repo = get(inputs, "repo")?;
        let name = get(inputs, "name")?;
        Ok((repo, name))
    }
}

impl Tool for GitHubActionsSecretEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (repo, name) = self.key_ports(inputs)?;
        match self.client.get_actions_secret(&repo, &name) {
            Ok(()) => Ok(Observation::Present(Outputs::new())),
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) => Err(to_tool_error(err)),
        }
    }

    fn ensure(&self, inputs: &Inputs, token: &SinkToken) -> Result<Ensured, ToolError> {
        let (repo, name) = self.key_ports(inputs)?;
        let value = inputs
            .get(&port("value"))
            .ok_or_else(|| invalid("port `value` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `value` is unknown"));
        }
        let object = value
            .as_scalar()
            .ok_or_else(|| invalid("port `value` must be a scalar secret"))?;
        // Fetch the key first, so the plaintext's lifetime in this
        // process is exactly "between expose and seal" — not stretched
        // across a network round trip it does not need to survive.
        let public_key = self.client.get_public_key(&repo).map_err(to_tool_error)?;
        // The one place in this crate the plaintext exists: read here,
        // handed straight to `seal`, and dropped immediately after.
        // Never `Debug`-formatted, never placed in a `ToolError`.
        let plaintext = object.expose(token);
        let encrypted_value = crate::seal::seal(&public_key.key, plaintext.as_bytes())?;
        drop(plaintext);
        self.client
            .put_actions_secret(&repo, &name, &encrypted_value, &public_key.key_id)
            .map_err(to_tool_error)?;
        // A sink whose value can never be read back always writes when
        // called, whether or not the secret already existed — matching
        // GitHub's own 201-or-204 (both success) and the fake tool's
        // identical `changed: true` on every call.
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}
