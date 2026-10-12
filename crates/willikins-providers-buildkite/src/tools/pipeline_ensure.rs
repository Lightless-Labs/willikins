//! `buildkite.pipeline.ensure`: creates a real Buildkite pipeline. Port
//! table and behaviour from
//! `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`'s
//! "The tool table" and decision (d) ("Idempotence: key, ownership, and
//! the four observations"); shaped exactly like
//! `willikins_providers_doppler::tools::project_ensure`.
//!
//! Milestone 3e task K1 gave this tool an optional, secret-typed `token`
//! port ([`BuildkiteToken`]), mirroring `github.repo.ensure`'s own -- see
//! `crate::client`'s "Buildkite credentials as ports" doc section.
//!
//! Milestone 3m task P4 adds this tool's inverse: decision 7's buildkite
//! row (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`), reusing
//! `BuildkiteClient::get_pipeline_standing`/`has_any_build`/`delete_pipeline`.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Created, Deleted, Ensured, Inputs, Inverse, Observation, Outputs, Reversal, SinkToken,
    Standing, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, BuildkiteToken, DomainType,
    GitHubRepo, HttpsUrl, TeardownToken,
};

use crate::client::{
    BuildkiteClient, MANAGED_DESCRIPTION, ScopedClient, parse_pipeline_created_at,
    pipeline_web_url, ssh_repository_url,
};

/// `buildkite.pipeline.ensure`.
pub struct BuildkitePipelineEnsure {
    spec: ToolSpec,
    client: Arc<BuildkiteClient>,
    inverse: BuildkitePipelineInverse,
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
        inputs.insert(port("token"), exact("BuildkiteToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        let inverse = BuildkitePipelineInverse {
            client: Arc::clone(&client),
        };
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
            inverse,
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
    /// and `ensure`, both of which pass the [`ScopedClient`] this tool's
    /// own optional `token` port implies -- see `crate::client`'s
    /// "Buildkite credentials as ports" doc section.
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
        client: &BuildkiteClient,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        repo: &GitHubRepo,
        cluster: &BuildkiteClusterId,
    ) -> Result<Observation, ToolError> {
        match client.get_pipeline(org, slug) {
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
        let token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        Self::observe(&client, &org, &slug, &repo, &cluster)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let cluster: BuildkiteClusterId = get(inputs, "cluster")?;
        let bound_token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, bound_token.as_ref());
        match Self::observe(&client, &org, &slug, &repo, &cluster)? {
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
                match client.create_pipeline(&org, &slug, &cluster, &repository) {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&org, &slug),
                        changed: true,
                    }),
                    // Buildkite documents no status or body for a
                    // duplicate create at all (decision (d)): resolve any
                    // create failure by re-reading rather than parsing
                    // the error body.
                    Err(err) => match Self::observe(&client, &org, &slug, &repo, &cluster)? {
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

    /// Milestone 3m decisions 4, 5, task P4: deletable, with no
    /// container of its own (`within: &[]`) and one declared loss,
    /// `"builds"` -- the only tool in this workspace that declares one
    /// at all (every other `Delete` tool's own `losses` is empty).
    /// `needs` names `token` alongside the two key ports (decision 5's
    /// own example names this tool's `token` port, the same reasoning
    /// `github.repo.ensure`'s own `reversal()` already documents), so a
    /// document-bound `token` port is re-resolved for the teardown
    /// exactly as `read`/`ensure` resolve it for a forward run.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["org", "slug", "token"],
            within: &[],
            losses: &["builds"],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `buildkite.pipeline.ensure`'s inverse (milestone 3m task P4, decisions
/// 4, 7). Never a catalog tool in its own right, never reachable except
/// from `apply_teardown` (trust boundaries 1, 2).
struct BuildkitePipelineInverse {
    client: Arc<BuildkiteClient>,
}

/// [`Standing::NotOurs`]: the pipeline's `description` no longer carries
/// the [`MANAGED_DESCRIPTION`] marker -- including a response that
/// omitted it outright (fail closed, decision 7).
const NOT_OURS_LOST_THE_MARKER: &str =
    "the pipeline's description no longer carries the managed-by: willikins marker";

/// [`Standing::NotOurs`]: the pipeline's own `slug` no longer matches the
/// key this run recorded.
const NOT_OURS_SLUG_DIFFERS: &str =
    "the pipeline's own slug no longer matches the key this run recorded";

/// [`Standing::NotOurs`]: the pipeline's own `created_at` falls outside
/// this instance's creation window (decision 2) -- including a response
/// that omitted it, or whose value this crate's own
/// `parse_pipeline_created_at` could not parse, outright (fail closed).
const NOT_OURS_OUTSIDE_WINDOW: &str =
    "the pipeline's own creation date falls outside this run's creation window";

/// [`crate::tool::helpers::conflict`]'s message when [`Inverse::delete`]
/// is called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`] -- the documented contract every real inverse
/// in this workspace honours (mirrors `GitHubRepoInverse`'s own constant
/// of the same shape). Unlike every other tool's own copy of this
/// message, this one must also cover the loss case: `AsCreated { losing
/// }` with a `"builds"` this node's own `teardown: delete` policy does
/// not accept also falls through to this arm (decision 3a).
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the pipeline has changed since this run created it, is no longer ours, or has a build \
     this node's policy does not accept; the teardown will not delete it";

impl BuildkitePipelineInverse {
    /// The `org` key port, re-resolved from the journal (decision 2):
    /// exactly `Created::inputs`, never read a second time from
    /// anywhere else.
    fn org_of(created: &Created<'_>) -> Result<BuildkiteOrg, ToolError> {
        get(created.inputs, "org")
    }

    /// The `slug` key port, re-resolved from the journal the same way
    /// [`Self::org_of`] is.
    fn slug_of(created: &Created<'_>) -> Result<BuildkitePipelineSlug, ToolError> {
        get(created.inputs, "slug")
    }

    /// The [`ScopedClient`] this call should use: the tool's own held
    /// client, or one built from a resolved `token` port (decision 5) --
    /// the same choice `read`/`ensure` make, via `created.inputs` instead
    /// of a tool call's own `Inputs`.
    fn scoped_client(&self, created: &Created<'_>) -> Result<ScopedClient<'_>, ToolError> {
        let token: Option<BuildkiteToken> = get_optional(created.inputs, "token")?;
        Ok(ScopedClient::default_for(&self.client, token.as_ref()))
    }

    /// Decision 7's buildkite row, past the point `standing` has already
    /// confirmed the pipeline exists. `created.riders` is never read
    /// here: decision 7 gives this tool no contents rule of its own
    /// (unlike `github.repo.ensure`'s Actions secrets or
    /// `doppler.project.ensure`'s configs) -- a pipeline's only reported
    /// loss is its builds, which this run's own riders could never have
    /// written in the first place.
    fn standing_from_body(
        client: &ScopedClient<'_>,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        body: &crate::client::PipelineStandingBody,
        created: &Created<'_>,
    ) -> Result<Standing, ToolError> {
        // Decision 7's own row order: description, then the window, then
        // the slug.
        if body.description.as_deref() != Some(MANAGED_DESCRIPTION) {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_LOST_THE_MARKER,
            });
        }
        let in_window = body
            .created_at
            .as_deref()
            .and_then(parse_pipeline_created_at)
            .is_some_and(|at| at >= created.window.start && at <= created.window.end);
        if !in_window {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_OUTSIDE_WINDOW,
            });
        }
        if body.slug != slug.to_string() {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_SLUG_DIFFERS,
            });
        }
        let losing = if client.has_any_build(org, slug)? {
            vec!["builds"]
        } else {
            Vec::new()
        };
        Ok(Standing::AsCreated { losing })
    }
}

impl Inverse for BuildkitePipelineInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let org = Self::org_of(created)?;
        let slug = Self::slug_of(created)?;
        let client = self.scoped_client(created)?;
        match client.get_pipeline_standing(&org, &slug) {
            Err(err) if err.status == Some(404) => Ok(Standing::Gone),
            Err(err) => Err(err.into()),
            Ok(body) => Self::standing_from_body(&client, &org, &slug, &body, created),
        }
    }

    fn delete(&self, created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        match self.standing(created)? {
            Standing::Gone => Ok(Deleted { already_gone: true }),
            Standing::AsCreated { losing }
                if losing.iter().all(|loss| {
                    created
                        .accept
                        .iter()
                        .any(|accepted| accepted.as_str() == *loss)
                }) =>
            {
                let org = Self::org_of(created)?;
                let slug = Self::slug_of(created)?;
                let client = self.scoped_client(created)?;
                client.delete_pipeline(&org, &slug)?;
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
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
