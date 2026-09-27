//! `github.repo.get`: a pure, read-only reference to a GitHub repository
//! this crate does not manage. Modelled on
//! `willikins_providers_buildkite::tools::BuildkiteClusterGet`: `ensure` is
//! the identity of `read`, there is no key, and the class is `Reversible`.
//!
//! Unlike `github.repo.ensure`, this tool never checks the
//! `managed-by-willikins` ownership topic -- it exists so a document can
//! *reference* a repository it does not own (milestone 3e's monorepo),
//! never to claim one. The only refusal besides a plain 404 is an
//! archived repository: a Buildkite pipeline can never build on one, so
//! `plan` should fail before any write rather than create a pipeline that
//! is inert on arrival.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::GitHubRepo;

use crate::client::{GitHubClient, to_tool_error};

/// `github.repo.get`.
pub struct GitHubRepoGet {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
}

impl GitHubRepoGet {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        Self {
            spec: ToolSpec {
                name: tool_name("github.repo.get"),
                description: "Reference an existing GitHub repository, checked against the \
                               live API."
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

    /// `GET /repos/{owner}/{name}`, mapped to this tool's one output.
    ///
    /// # Errors
    ///
    /// [`willikins_core::ToolErrorKind::NotFound`] on a `404` (the
    /// repository does not exist, or this credential cannot see it --
    /// [`GitHubClient::get_repo`]'s own doc explains why the two are the
    /// same status), [`willikins_core::ToolErrorKind::Conflict`] naming
    /// `repo` when it is archived, or
    /// [`willikins_core::ToolErrorKind::Provider`] for anything else
    /// (including a `401`/`403`, mapped generically by
    /// `willikins_providers_http::ProviderError`'s own conversion).
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        match self.client.get_repo(&repo) {
            Ok(body) if body.archived => Err(conflict(format!(
                "`{repo}` is archived; a pipeline could never build on it"
            ))),
            Ok(_) => {
                let mut outputs = Outputs::new();
                outputs.insert(port("repo"), Value::known(repo));
                Ok(outputs)
            }
            Err(err) => Err(to_tool_error(err)),
        }
    }
}

impl Tool for GitHubRepoGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.lookup(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.lookup(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};

    fn tool() -> GitHubRepoGet {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        GitHubRepoGet::new(Arc::new(GitHubClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key() {
        assert!(tool().spec().key.is_empty());
    }

    #[test]
    fn spec_is_pure() {
        assert!(tool().spec().pure);
    }
}
