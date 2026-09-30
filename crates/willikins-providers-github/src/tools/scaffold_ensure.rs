//! `github.scaffold.ensure`: milestone 3g, task G2, decisions (b) and (c)
//! of `docs/plans/2026-09-30-milestone-3g-file-writing.md`.
//!
//! The resource this tool ensures is "this scaffold has landed on this
//! branch", keyed by `(repo, branch, marker)` -- `files` is the content,
//! not the key. A scaffold is a **seed**: once its marker is present, the
//! files it seeded belong to the repository and are never read or written
//! again (decision (c)); before landing, a declared path that already
//! holds different content is a refusal naming that path, never an
//! overwrite. See [`GitHubScaffoldEnsure::observe`] for the read table and
//! [`Tool::ensure`]'s impl for the bounded retry against a moving head.

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, invalid, list, not_found, port, require_present, scalar,
    tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{CommitHeadline, GitBranchName, GitHubRepo, GitHubToken, RepoFile, RepoPath};

use crate::client::{GitHubClient, PathEntry, ScopedClient, git_blob_sha, to_tool_error};

/// The marker file's required first line -- the codebase's own
/// ownership-marker precedent (`managed-by-willikins` topic,
/// `managed-by: willikins` pipeline description).
const MARKER_HEADER: &str = "managed-by: willikins";

/// The fewest files a call may seed.
const MIN_FILES: usize = 1;

/// The most files a call may seed -- SHARED VALUES' `files: list<RepoFile>`
/// bound.
const MAX_FILES: usize = 64;

/// The most `createCommitOnBranch` attempts `ensure` makes in total,
/// including the first: a busy trunk can move twice while this call is in
/// flight, but three attempts, each decided only from a fresh re-read
/// (never from a commit failure's own body), is where decision (b) stops
/// retrying and reports the original failure instead.
const MAX_COMMIT_ATTEMPTS: u32 = 3;

/// `github.scaffold.ensure`.
pub struct GitHubScaffoldEnsure {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
}

/// What [`GitHubScaffoldEnsure::observe`] found at the current head,
/// carrying exactly what `ensure` needs and no more: never a blob sha
/// (outputs, and so this state, must never leak one -- acceptance 8).
enum ScaffoldState {
    /// The marker is present with the right first line: the scaffold has
    /// landed. No seed path was ever read to reach this state.
    Present,
    /// Something exists at the marker's key but is not willikins' own
    /// marker, or a seed path holds content this tool did not put there.
    Foreign,
    /// The marker is absent and every seed path is either absent or
    /// already byte-equal to what this call would write.
    Absent {
        /// The head commit sha this observation was made against --
        /// `ensure`'s compare-and-swap.
        head: String,
        /// Seed paths (by [`RepoPath::as_str`]) already present with the
        /// exact content this call would write: skipped from the commit's
        /// `additions`, so a scaffold half-applied by hand from the same
        /// templates still converges.
        already_equal: HashSet<String>,
    },
}

impl GitHubScaffoldEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("branch"), exact("GitBranchName", true));
        inputs.insert(port("marker"), exact("RepoPath", true));
        inputs.insert(
            port("files"),
            PortSpec {
                ty: PortType::Exact(list("RepoFile")),
                required: true,
                derived_only: false,
            },
        );
        inputs.insert(port("message"), exact("CommitHeadline", true));
        inputs.insert(port("token"), exact("GitHubToken", false));
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        outputs.insert(port("branch"), scalar("GitBranchName"));
        outputs.insert(port("marker"), scalar("RepoPath"));
        Self {
            spec: ToolSpec {
                name: tool_name("github.scaffold.ensure"),
                description: "Ensure a set of files, plus an ownership marker, has landed on a \
                               branch as one commit."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("repo"), port("branch"), port("marker")],
                class: Class::Irreversible,
                pure: false,
            },
            client,
        }
    }

    /// The outputs this tool ever reports: pass-through only, never a
    /// blob sha or a commit oid (decision (b): an output carrying either
    /// would make a plan-to-apply window on a busy `main` drift).
    fn outputs_for(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("branch"), Value::known(branch.clone()));
        outputs.insert(port("marker"), Value::known(marker.clone()));
        outputs
    }

    /// Read the required `files` port as a list of [`RepoFile`]s.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] of kind `Invalid` when the port is bound but
    /// unknown, or not a list of `RepoFile` -- both mean `check` was
    /// bypassed, since `plan`/`apply` only ever deliver a well-typed value
    /// here.
    fn read_files(inputs: &Inputs) -> Result<Vec<RepoFile>, ToolError> {
        let value = inputs
            .get(&port("files"))
            .ok_or_else(|| invalid("port `files` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `files` is unknown"));
        }
        let items = value
            .as_list()
            .ok_or_else(|| invalid("port `files` has an unexpected type"))?;
        items
            .iter()
            .map(|object| {
                willikins_types::downcast::<RepoFile>(object.as_ref())
                    .cloned()
                    .ok_or_else(|| invalid("port `files` has an unexpected type"))
            })
            .collect()
    }

    /// Every shape refusal decision (b)'s table makes **before any
    /// request**: a file count out of `1..=64`, a path declared twice, the
    /// marker path equal to one of `files`, or one declared path (a file or
    /// the marker) a directory of another.
    fn validate_shape(files: &[RepoFile], marker: &RepoPath) -> Result<(), ToolError> {
        if files.len() < MIN_FILES || files.len() > MAX_FILES {
            return Err(invalid(format!(
                "`files` must hold between {MIN_FILES} and {MAX_FILES} entries, but held {}",
                files.len()
            )));
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(files.len());
        for file in files {
            if !seen.insert(file.path().as_str()) {
                return Err(invalid(format!(
                    "path `{}` is declared more than once in `files`",
                    file.path()
                )));
            }
        }
        if seen.contains(marker.as_str()) {
            return Err(invalid(format!(
                "marker path `{marker}` must not also be one of `files`"
            )));
        }
        // A declared path that is a directory of another (two files, or a
        // file and the marker, either way round) cannot coexist in one
        // tree. Adversarial pass (render and write).
        // Walked in declaration order, so the message is deterministic.
        seen.insert(marker.as_str());
        let declared = files
            .iter()
            .map(|file| file.path().as_str())
            .chain(std::iter::once(marker.as_str()));
        for path in declared {
            if let Some((index, _)) = path
                .match_indices('/')
                .find(|(index, _)| seen.contains(&path[..*index]))
            {
                return Err(invalid(format!(
                    "path `{}` is declared as a file and also as a directory of `{path}`",
                    &path[..index]
                )));
            }
        }
        Ok(())
    }

    /// The marker file's content: [`MARKER_HEADER`], then one
    /// `<40-hex blob sha> <path>` line per seeded file, sorted by path,
    /// trailing newline (SHARED VALUES' exact format).
    fn marker_content(files: &[RepoFile]) -> String {
        let mut lines: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    file.path().as_str().to_string(),
                    git_blob_sha(file.content().as_bytes()),
                )
            })
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let mut content = String::from(MARKER_HEADER);
        content.push('\n');
        for (path, sha) in lines {
            content.push_str(&sha);
            content.push(' ');
            content.push_str(&path);
            content.push('\n');
        }
        content
    }

    /// Build and validate the marker [`RepoFile`] from `files` and
    /// `marker` -- both fully known from inputs alone, so this is a shape
    /// refusal decision (b)'s table puts "before any request", not one
    /// [`ensure`](Tool::ensure) may discover only after committing
    /// everything else. Without this, 64 files each near [`RepoPath`]'s
    /// own 1,024-character bound can produce a marker over
    /// [`RepoFile`]'s 65,536-character bound, and the failure would
    /// otherwise surface from inside the commit attempt instead of here.
    fn validated_marker_file(files: &[RepoFile], marker: &RepoPath) -> Result<RepoFile, ToolError> {
        let content = Self::marker_content(files);
        RepoFile::new(marker.clone(), content)
            .map_err(|err| invalid(format!("marker file is invalid: {}", err.reason)))
    }

    /// The `Conflict` a foreign marker or an owned-but-differing seed path
    /// produces.
    fn foreign_conflict(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> ToolError {
        conflict(format!(
            "`{marker}` on `{repo}`@`{branch}` already exists and is not willikins' scaffold \
             marker; this tool will not overwrite it"
        ))
    }

    /// Resolve the current head, the marker's state, and -- only when the
    /// marker is absent -- every seed path's state, exactly as decision
    /// (b)'s read table describes. Never downloads a seed file's content;
    /// content is compared by [`git_blob_sha`]. The marker's own blob is
    /// the only one this tool ever fetches.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] of kind `NotFound` when `branch` does not
    /// exist (this tool never creates one) and `Conflict` naming every
    /// seed path that holds content this tool did not put there, sorted,
    /// never their content. Any other provider failure is [`to_tool_error`].
    fn observe(
        client: &GitHubClient,
        repo: &GitHubRepo,
        branch: &GitBranchName,
        marker: &RepoPath,
        files: &[RepoFile],
    ) -> Result<ScaffoldState, ToolError> {
        let head = match client.get_branch_head(repo, branch) {
            Ok(head) => head,
            Err(err) if err.status == Some(404) => {
                return Err(not_found(format!(
                    "branch `{branch}` does not exist on `{repo}`; this tool never creates one"
                )));
            }
            Err(err) => return Err(to_tool_error(err)),
        };
        let root_tree = client
            .get_commit_root_tree(repo, &head)
            .map_err(to_tool_error)?;

        // The marker alone, first: "Present issues no tree walk past the
        // marker" (acceptance 8) -- a seed path is never resolved once
        // the scaffold has already landed.
        let marker_paths = std::slice::from_ref(marker);
        let marker_entries = client
            .resolve_tree_paths(repo, &root_tree, marker_paths)
            .map_err(to_tool_error)?;
        match marker_entries.get(marker) {
            Some(PathEntry::Blob { mode, sha }) if mode.as_str() == "100644" => {
                let bytes = client.get_blob(repo, sha).map_err(to_tool_error)?;
                let first_line = bytes.split(|&b| b == b'\n').next().unwrap_or(&[]);
                if first_line == MARKER_HEADER.as_bytes() {
                    return Ok(ScaffoldState::Present);
                }
                return Ok(ScaffoldState::Foreign);
            }
            // `resolve_tree_paths` inserts an entry for every requested
            // path, `Absent` included -- `None` never actually occurs
            // here, but is handled the same way defensively.
            None | Some(PathEntry::Absent) => {}
            Some(_) => return Ok(ScaffoldState::Foreign),
        }

        // Marker absent: every seed path must be either absent or
        // byte-equal to what this call would write.
        let paths: Vec<RepoPath> = files.iter().map(|file| file.path().clone()).collect();
        let entries = client
            .resolve_tree_paths(repo, &root_tree, &paths)
            .map_err(to_tool_error)?;
        let mut already_equal: HashSet<String> = HashSet::new();
        let mut conflicts: Vec<String> = Vec::new();
        for file in files {
            match entries.get(file.path()) {
                None | Some(PathEntry::Absent) => {}
                Some(PathEntry::Blob { mode, sha }) if mode.as_str() == "100644" => {
                    let expected = git_blob_sha(file.content().as_bytes());
                    if sha.as_str() == expected.as_str() {
                        already_equal.insert(file.path().as_str().to_string());
                    } else {
                        conflicts.push(file.path().as_str().to_string());
                    }
                }
                Some(_) => conflicts.push(file.path().as_str().to_string()),
            }
        }
        if !conflicts.is_empty() {
            conflicts.sort();
            conflicts.dedup();
            return Err(conflict(format!(
                "the scaffold would overwrite existing, different content at: {}",
                conflicts.join(", ")
            )));
        }
        Ok(ScaffoldState::Absent {
            head,
            already_equal,
        })
    }
}

impl Tool for GitHubScaffoldEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let branch: GitBranchName = get(inputs, "branch")?;
        let marker: RepoPath = get(inputs, "marker")?;
        let files = Self::read_files(inputs)?;
        Self::validate_shape(&files, &marker)?;
        Self::validated_marker_file(&files, &marker)?;
        let token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        match Self::observe(&client, &repo, &branch, &marker, &files)? {
            ScaffoldState::Present => Ok(Observation::Present(Self::outputs_for(
                &repo, &branch, &marker,
            ))),
            ScaffoldState::Foreign => Ok(Observation::Foreign),
            ScaffoldState::Absent { .. } => Ok(Observation::Absent {
                predicted: Self::outputs_for(&repo, &branch, &marker),
            }),
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let branch: GitBranchName = get(inputs, "branch")?;
        let marker: RepoPath = get(inputs, "marker")?;
        let files = Self::read_files(inputs)?;
        Self::validate_shape(&files, &marker)?;
        let marker_file = Self::validated_marker_file(&files, &marker)?;
        let message: CommitHeadline = get(inputs, "message")?;
        let bound_token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, bound_token.as_ref());
        let outputs = Self::outputs_for(&repo, &branch, &marker);
        let body = format!("Seeded by willikins. Marker: {marker}.");

        let mut state = Self::observe(&client, &repo, &branch, &marker, &files)?;
        let mut attempts: u32 = 0;
        loop {
            let (head, already_equal) = match state {
                ScaffoldState::Present => {
                    return Ok(Ensured {
                        outputs,
                        changed: false,
                    });
                }
                ScaffoldState::Foreign => {
                    return Err(Self::foreign_conflict(&repo, &branch, &marker));
                }
                ScaffoldState::Absent {
                    head,
                    already_equal,
                } => (head, already_equal),
            };

            let mut additions: Vec<RepoFile> = files
                .iter()
                .filter(|file| !already_equal.contains(file.path().as_str()))
                .cloned()
                .collect();
            additions.push(marker_file.clone());

            attempts += 1;
            match client.create_commit_on_branch(
                &repo,
                &branch,
                &head,
                &additions,
                &message,
                Some(&body),
            ) {
                Ok(_oid) => {
                    return Ok(Ensured {
                        outputs,
                        changed: true,
                    });
                }
                Err(original_err) => {
                    // Decision (b): the decision is made from a fresh
                    // re-read, never from the failed commit's own error
                    // body -- the compare-and-swap forbids a duplicate,
                    // which is what makes retrying safe.
                    match Self::observe(&client, &repo, &branch, &marker, &files)? {
                        ScaffoldState::Present => {
                            return Ok(Ensured {
                                outputs,
                                changed: false,
                            });
                        }
                        ScaffoldState::Foreign => {
                            return Err(Self::foreign_conflict(&repo, &branch, &marker));
                        }
                        ScaffoldState::Absent {
                            head: new_head,
                            already_equal: new_already_equal,
                        } if new_head == head => {
                            // The branch did not move: nothing about our
                            // attempt could have landed, and re-trying
                            // against the same head would fail the same
                            // way, so the original failure is reported
                            // as-is.
                            let _ = new_already_equal;
                            return Err(to_tool_error(original_err));
                        }
                        ScaffoldState::Absent {
                            head: new_head,
                            already_equal: new_already_equal,
                        } => {
                            if attempts >= MAX_COMMIT_ATTEMPTS {
                                return Err(to_tool_error(original_err));
                            }
                            state = ScaffoldState::Absent {
                                head: new_head,
                                already_equal: new_already_equal,
                            };
                        }
                    }
                }
            }
        }
    }
}
