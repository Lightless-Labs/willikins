//! `buildkite.pipeline.ensure`: creates a real Buildkite pipeline. Port
//! table and behaviour from
//! `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`'s
//! "The tool table" and decision (d) ("Idempotence: key, ownership, and
//! the four observations"); shaped exactly like
//! `willikins_providers_doppler::tools::project_ensure`.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubRepo, HttpsUrl,
};

use crate::client::{BuildkiteClient, MANAGED_DESCRIPTION, pipeline_web_url, ssh_repository_url};

/// `buildkite.pipeline.ensure`.
pub struct BuildkitePipelineEnsure {
    spec: ToolSpec,
    client: Arc<BuildkiteClient>,
}

impl BuildkitePipelineEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<BuildkiteClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("cluster"), exact("BuildkiteClusterId", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.pipeline.ensure"),
                description: "Ensure a Buildkite pipeline exists in a cluster, pointed at a GitHub repository.".to_string(),
                inputs,
                outputs,
                key: vec![port("org"), port("slug")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    /// The predicted or confirmed outputs for `slug`, whose URL is always
    /// this crate's own derived form ([`pipeline_web_url`]), never the
    /// provider's own `web_url` field.
    ///
    /// # Panics
    ///
    /// Never: `org` and `slug` are already-parsed domain types whose
    /// grammars keep [`pipeline_web_url`]'s output well inside
    /// [`HttpsUrl`]'s pattern and length limit.
    fn outputs_for(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs.insert(
            port("url"),
            Value::known(
                HttpsUrl::parse(&pipeline_web_url(org, slug))
                    .expect("org and slug always join into a valid HttpsUrl"),
            ),
        );
        outputs
    }

    /// `GET` the pipeline, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// Ownership is exact equality against [`MANAGED_DESCRIPTION`], the
    /// same rule `doppler.project.ensure` uses for the same reason: a
    /// human-edited description reads `Foreign` rather than `Present`.
    /// `Mismatch` is checked `repo` before `cluster` (decision (d)): a
    /// pipeline pointed at the wrong repository is the more alarming of
    /// the two. `configuration` and `name` are never compared -- neither
    /// is a port, and this crate has no call that could change them
    /// (decision (a)).
    fn observe(
        &self,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        repo: &GitHubRepo,
        cluster: &BuildkiteClusterId,
    ) -> Result<Observation, ToolError> {
        match self.client.get_pipeline(org, slug) {
            Ok(body) if body.description.as_deref() != Some(MANAGED_DESCRIPTION) => {
                Ok(Observation::Foreign)
            }
            Ok(body) if body.repository != ssh_repository_url(repo) => {
                Ok(Observation::Mismatch { port: port("repo") })
            }
            Ok(body) if body.cluster_id.as_deref() != Some(cluster.to_string().as_str()) => {
                Ok(Observation::Mismatch {
                    port: port("cluster"),
                })
            }
            Ok(_) => Ok(Observation::Present(Self::outputs_for(org, slug))),
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(org, slug),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> ToolError {
        conflict(format!("`{org}/{slug}` already exists and is not ours"))
    }

    fn mismatch_conflict(
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        port: &str,
    ) -> ToolError {
        conflict(format!(
            "`{org}/{slug}` is ours, but its `{port}` does not match what was requested and \
             this tool will not change it; change it by hand, or pass its current value instead"
        ))
    }
}

impl Tool for BuildkitePipelineEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let cluster: BuildkiteClusterId = get(inputs, "cluster")?;
        self.observe(&org, &slug, &repo, &cluster)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let cluster: BuildkiteClusterId = get(inputs, "cluster")?;
        match self.observe(&org, &slug, &repo, &cluster)? {
            Observation::Foreign => Err(Self::foreign_conflict(&org, &slug)),
            Observation::Mismatch { port } => {
                Err(Self::mismatch_conflict(&org, &slug, port.as_str()))
            }
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => {
                let repository = ssh_repository_url(&repo);
                match self
                    .client
                    .create_pipeline(&org, &slug, &cluster, &repository)
                {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&org, &slug),
                        changed: true,
                    }),
                    // Buildkite documents no status or body for a
                    // duplicate create at all (decision (d)): resolve any
                    // create failure by re-reading rather than parsing
                    // the error body.
                    Err(err) => match self.observe(&org, &slug, &repo, &cluster)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Foreign => Err(Self::foreign_conflict(&org, &slug)),
                        Observation::Mismatch { port } => {
                            Err(Self::mismatch_conflict(&org, &slug, port.as_str()))
                        }
                        Observation::Absent { .. } => Err(err.into()),
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};

    fn tool() -> BuildkitePipelineEnsure {
        let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        BuildkitePipelineEnsure::new(Arc::new(BuildkiteClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_org_and_slug() {
        assert_eq!(tool().spec().key, vec![port("org"), port("slug")]);
    }
}
