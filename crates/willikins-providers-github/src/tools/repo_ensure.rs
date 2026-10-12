//! `github.repo.ensure`: creates a real GitHub repository. Port table and
//! behaviour identical to `willikins_providers_fake`'s tool of the same
//! name (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Created, Deleted, Ensured, Inputs, Inverse, Observation, Outputs, Reversal, SinkToken,
    Standing, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{ActionsSecretName, GitHubRepo, GitHubToken, RepoVisibility, TeardownToken};

use crate::client::{GitHubClient, RepoBody, ScopedClient, to_tool_error};

/// `github.repo.ensure`.
pub struct GitHubRepoEnsure {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
    inverse: GitHubRepoInverse,
}

impl GitHubRepoEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("visibility"), exact("RepoVisibility", true));
        inputs.insert(port("token"), exact("GitHubToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        let inverse = GitHubRepoInverse {
            client: Arc::clone(&client),
        };
        Self {
            spec: ToolSpec {
                name: tool_name("github.repo.ensure"),
                description: "Ensure a GitHub repository exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
            client,
            inverse,
        }
    }

    /// The outputs a `Present` or newly created repository reports.
    fn outputs_for(repo: &GitHubRepo) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("url"), Value::known(repo.url()));
        outputs
    }

    /// The requested `visibility` input, when bound and
    /// [`Value::is_known`]. See the fake tool's identical helper: a not-
    /// yet-known `visibility` at plan time is "no mismatch determinable
    /// from here", not an error.
    fn requested_visibility(inputs: &Inputs) -> Option<RepoVisibility> {
        inputs
            .get(&port("visibility"))
            .filter(|value| value.is_known())
            .and_then(|value| value.downcast::<RepoVisibility>())
            .copied()
    }

    /// The `Conflict` a foreign (not-ours) repository at this key
    /// produces, shared between the direct `Foreign` observation and the
    /// "re-read after an ambiguous create" path.
    fn foreign_conflict(repo: &GitHubRepo) -> ToolError {
        conflict(format!("`{repo}` already exists and is not ours"))
    }

    /// The `Conflict` an owned-but-visibility-mismatched repository at
    /// this key produces, shared the same way as [`Self::foreign_conflict`].
    fn mismatch_conflict(repo: &GitHubRepo) -> ToolError {
        conflict(format!(
            "`{repo}` is ours, but its visibility does not match what was requested and \
             this tool will not change it; change it by hand, or pass its current value \
             instead"
        ))
    }

    /// `GET /repos/{owner}/{name}`, mapped to an [`Observation`]. Shared
    /// by `read` and `ensure`, both of which pass the [`ScopedClient`]
    /// this tool's own optional `token` port implies -- see
    /// `crate::client`'s "GitHub credentials as ports" doc section.
    fn observe(
        client: &GitHubClient,
        repo: &GitHubRepo,
        inputs: &Inputs,
    ) -> Result<Observation, ToolError> {
        match client.get_repo(repo) {
            Ok(RepoBody {
                visibility,
                topics,
                archived: _,
                default_branch: _,
                full_name: _,
                created_at: _,
                pushed_at: _,
                open_issues_count: _,
                forks_count: _,
            }) => {
                if !topics.iter().any(|topic| topic == crate::MANAGED_TOPIC) {
                    return Ok(Observation::Foreign);
                }
                match Self::requested_visibility(inputs) {
                    Some(requested) if requested != visibility => Ok(Observation::Mismatch {
                        port: port("visibility"),
                    }),
                    _ => Ok(Observation::Present(Self::outputs_for(repo))),
                }
            }
            // GitHub answers 404 both for a truly missing repository and
            // for one this credential cannot see — the same conflation
            // `willikins_providers_fake`'s tool models.
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(repo),
            }),
            // A renamed repository's old owner/name 301s rather than
            // 404ing; treated as Foreign, same as a repository that
            // exists but lost its ownership topic.
            Err(err) if err.status == Some(301) => Ok(Observation::Foreign),
            Err(err) => Err(to_tool_error(err)),
        }
    }
}

impl Tool for GitHubRepoEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        Self::observe(&client, &repo, inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let visibility: RepoVisibility = get(inputs, "visibility")?;
        let bound_token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, bound_token.as_ref());
        match Self::observe(&client, &repo, inputs)? {
            Observation::Foreign => Err(Self::foreign_conflict(&repo)),
            Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&repo)),
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => match client.create_repo(&repo, visibility) {
                Ok(()) => {
                    client.put_managed_topic(&repo).map_err(to_tool_error)?;
                    Ok(Ensured {
                        outputs: Self::outputs_for(&repo),
                        changed: true,
                    })
                }
                // The create may have landed despite the error (a retried
                // ambiguous failure, or simply a name already taken):
                // re-read rather than assume either way.
                Err(err) if err.already_exists => match Self::observe(&client, &repo, inputs)? {
                    Observation::Present(outputs) => Ok(Ensured {
                        outputs,
                        changed: false,
                    }),
                    Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&repo)),
                    _ => Err(Self::foreign_conflict(&repo)),
                },
                Err(err) => Err(to_tool_error(err)),
            },
        }
    }

    /// Milestone 3m decisions 4, 5, task P1: deletable, with no
    /// container of its own (`within: &[]`) and no declared loss (only
    /// `buildkite.pipeline.ensure`'s `builds` exists). `needs` names
    /// `token` alongside the key port `repo`, so a document-bound
    /// `token` port (decision 5's own example) is re-resolved for the
    /// teardown exactly as `read`/`ensure` resolve it for a forward run,
    /// rather than always falling back to this tool's held
    /// execution-context credential.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["repo", "token"],
            within: &[],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `github.repo.ensure`'s inverse (milestone 3m task P1, decisions 4, 7).
/// Never a catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2).
struct GitHubRepoInverse {
    client: Arc<GitHubClient>,
}

/// [`Standing::NotOurs`]: the repository has been renamed or transferred
/// since this run created it (its `full_name` no longer matches the
/// recorded key), or GitHub's `GET` answered a `3xx` for the same reason
/// (`Http` follows no redirect) — including a response that omitted
/// `full_name` outright (fail closed, decision 7).
const NOT_OURS_RENAMED_OR_TRANSFERRED: &str =
    "the repository has been renamed or transferred since this run created it";

/// [`Standing::NotOurs`]: the repository no longer carries the
/// `managed-by-willikins` topic.
const NOT_OURS_LOST_THE_TOPIC: &str =
    "the repository no longer carries the managed-by-willikins ownership topic";

/// [`Standing::NotOurs`]: the repository's own `created_at` falls outside
/// this instance's creation window (decision 2) — including a response
/// that omitted `created_at` outright (fail closed).
const NOT_OURS_OUTSIDE_WINDOW: &str =
    "the repository's own creation date falls outside this run's creation window";

/// [`Standing::Drifted`]: the repository has been pushed to since this
/// run created it (`pushed_at` later than `settled_by`) — including a
/// response that omitted `pushed_at` outright, which cannot rule a push
/// out (fail closed).
const DRIFTED_PUSHED_SINCE_CREATED: &str =
    "the repository has been pushed to since this run created it";

/// [`Standing::Drifted`]: the repository has an open issue or pull
/// request this run never made (GitHub counts both in
/// `open_issues_count`) — including a response that omitted the count
/// outright (fail closed).
const DRIFTED_HAS_OPEN_ISSUES: &str =
    "the repository has an open issue or pull request this run never made";

/// [`Standing::Drifted`]: the repository has been forked since this run
/// created it — including a response that omitted `forks_count` outright
/// (fail closed).
const DRIFTED_HAS_BEEN_FORKED: &str = "the repository has been forked since this run created it";

/// [`Standing::Drifted`]: the repository carries an Actions secret name
/// this run's own record (its riders) does not account for.
const DRIFTED_FOREIGN_ACTIONS_SECRET: &str =
    "the repository carries an Actions secret this run's own riders did not create";

/// [`crate::tool::helpers::conflict`]'s message when [`Inverse::delete`]
/// is called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`] (the documented contract every real inverse in
/// this crate honours, mirroring `willikins_core::rollback`'s own
/// `ObservableInverse` test double).
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the repository has changed since this run created it, or is no longer ours; the teardown \
     will not delete it";

impl GitHubRepoInverse {
    /// The `repo` key port, re-resolved from the journal (decision 2):
    /// exactly `Created::inputs`, never read a second time from anywhere
    /// else.
    fn repo_of(created: &Created<'_>) -> Result<GitHubRepo, ToolError> {
        get(created.inputs, "repo")
    }

    /// The [`ScopedClient`] this call should use: the tool's own held
    /// client, or one built from a resolved `token` port (decision 5) —
    /// the same choice `read`/`ensure` make, via `created.inputs` instead
    /// of a tool call's own `Inputs`.
    fn scoped_client(&self, created: &Created<'_>) -> Result<ScopedClient<'_>, ToolError> {
        let token: Option<GitHubToken> = get_optional(created.inputs, "token")?;
        Ok(ScopedClient::default_for(&self.client, token.as_ref()))
    }

    /// Decision 7's drift table, past the point `standing` has already
    /// confirmed the repository exists and did not 3xx.
    fn standing_from_body(
        client: &ScopedClient<'_>,
        repo: &GitHubRepo,
        body: &RepoBody,
        created: &Created<'_>,
    ) -> Result<Standing, ToolError> {
        if body.full_name.as_deref() != Some(repo.to_string().as_str()) {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_RENAMED_OR_TRANSFERRED,
            });
        }
        if !body
            .topics
            .iter()
            .any(|topic| topic == crate::MANAGED_TOPIC)
        {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_LOST_THE_TOPIC,
            });
        }
        let in_window = body
            .created_at
            .is_some_and(|at| at >= created.window.start && at <= created.window.end);
        if !in_window {
            return Ok(Standing::NotOurs {
                why: NOT_OURS_OUTSIDE_WINDOW,
            });
        }
        if body
            .pushed_at
            .is_none_or(|pushed_at| pushed_at > created.settled_by)
        {
            return Ok(Standing::Drifted {
                what: DRIFTED_PUSHED_SINCE_CREATED,
            });
        }
        if body.open_issues_count.is_none_or(|count| count > 0) {
            return Ok(Standing::Drifted {
                what: DRIFTED_HAS_OPEN_ISSUES,
            });
        }
        if body.forks_count.is_none_or(|count| count > 0) {
            return Ok(Standing::Drifted {
                what: DRIFTED_HAS_BEEN_FORKED,
            });
        }
        let tolerated: Vec<String> = created
            .riders
            .iter()
            .filter(|rider| rider.tool == "github.actions_secret.ensure")
            .filter_map(|rider| get::<ActionsSecretName>(rider.inputs, "name").ok())
            .map(|name| name.as_str().to_string())
            .collect();
        let names = client
            .list_actions_secret_names(repo)
            .map_err(to_tool_error)?;
        if names.iter().any(|name| !tolerated.contains(name)) {
            return Ok(Standing::Drifted {
                what: DRIFTED_FOREIGN_ACTIONS_SECRET,
            });
        }
        Ok(Standing::AsCreated { losing: Vec::new() })
    }
}

impl Inverse for GitHubRepoInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let repo = Self::repo_of(created)?;
        let client = self.scoped_client(created)?;
        match client.get_repo(&repo) {
            Err(err) if err.status == Some(404) => Ok(Standing::Gone),
            // Decision 7: "any 3xx (renamed or transferred: `Http`
            // follows no redirect)" -- not only the `301` `observe`
            // itself special-cases, since a teardown must treat every
            // redirect the same way `read`/`ensure` never see (this
            // client follows none).
            Err(err)
                if err
                    .status
                    .is_some_and(|status| (300..400).contains(&status)) =>
            {
                Ok(Standing::NotOurs {
                    why: NOT_OURS_RENAMED_OR_TRANSFERRED,
                })
            }
            Err(err) => Err(to_tool_error(err)),
            Ok(body) => Self::standing_from_body(&client, &repo, &body, created),
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
                let repo = Self::repo_of(created)?;
                let client = self.scoped_client(created)?;
                client.delete_repo(&repo).map_err(to_tool_error)?;
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
        }
    }
}
