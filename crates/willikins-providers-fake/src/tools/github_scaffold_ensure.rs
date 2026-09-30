//! `github.scaffold.ensure`: lands (in memory) a set of files plus an
//! ownership marker on a branch, as `willikins-providers-github`'s live
//! tool of the same name does against a real commit. Milestone 3g, task
//! G2. Port table and behaviour must equal the live tool's field for
//! field (`tests/catalog_parity.rs`, `tests/fake_agrees_with_live.rs` in
//! `willikins-providers-github`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use sha1::{Digest, Sha1};

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{CommitHeadline, GitBranchName, GitHubRepo, RepoFile, RepoPath};

use crate::state::{FakeState, scaffold_key};
use crate::support::{
    conflict, exact, get, invalid, list, port, require_present, scalar, tool_name,
};

/// See `willikins_providers_github`'s live tool -- both crates share this
/// exact literal.
const MARKER_HEADER: &str = "managed-by: willikins";

/// See the live tool.
const MIN_FILES: usize = 1;

/// See the live tool.
const MAX_FILES: usize = 64;

/// This tool's own observation, carrying exactly what `ensure` needs.
enum ScaffoldState {
    Present,
    Foreign,
    Absent {
        /// Seed paths already present with byte-equal content: skipped
        /// when writing, mirroring the live tool's own convergence.
        already_equal: HashSet<String>,
    },
}

/// `github.scaffold.ensure`.
pub struct GitHubScaffoldEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl GitHubScaffoldEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "github.scaffold.ensure";

    /// Build the tool against `state`, constructing its spec -- the exact
    /// shape `tests/catalog_parity.rs` (in `willikins-providers-github`)
    /// pins equal to the live tool's.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
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
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a set of files, plus an ownership marker, has landed on a \
                               branch as one commit."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("repo"), port("branch"), port("marker")],
                class: Class::Irreversible,
                pure: false,
            },
            state,
        }
    }

    /// The outputs this tool ever reports: pass-through only, exactly as
    /// the live tool's own.
    fn outputs_for(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("branch"), Value::known(branch.clone()));
        outputs.insert(port("marker"), Value::known(marker.clone()));
        outputs
    }

    /// Read the required `files` port as a list of [`RepoFile`]s --
    /// mirrors `repo.file.render`'s own `read_values` and the live
    /// tool's `read_files`.
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

    /// Every shape refusal the live tool makes before any state lookup at
    /// all.
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
        Ok(())
    }

    /// SHA-1 over `blob <byte length>\0<bytes>`, the same algorithm git
    /// (and `willikins_providers_github`'s own `git_blob_sha`) uses --
    /// duplicated here (`pub(crate)` there, a different crate here) so
    /// this fake's marker content is byte-identical to the live tool's,
    /// which `tests/fake_agrees_with_live.rs` pins.
    fn blob_sha(content: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha1::new();
        hasher.update(format!("blob {}\0", content.len()));
        hasher.update(content);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(40);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }

    /// The marker file's content -- byte-identical to the live tool's own
    /// [SHARED VALUES format].
    fn marker_content(files: &[RepoFile]) -> String {
        let mut lines: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    file.path().as_str().to_string(),
                    Self::blob_sha(file.content().as_bytes()),
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

    /// The `Conflict` a foreign marker or an owned-but-differing seed path
    /// produces -- the same message shape as the live tool's.
    fn foreign_conflict(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> ToolError {
        conflict(format!(
            "`{marker}` on `{repo}`@`{branch}` already exists and is not willikins' scaffold \
             marker; this tool will not overwrite it"
        ))
    }

    /// Observe `entry` (the scaffold's current path-to-content map, or
    /// none at all) the way decision (b)'s read table describes, with
    /// string equality standing in for the live tool's blob sha
    /// comparison.
    fn observe(
        entry: Option<&HashMap<String, String>>,
        marker: &RepoPath,
        files: &[RepoFile],
    ) -> Result<ScaffoldState, ToolError> {
        let empty = HashMap::new();
        let map = entry.unwrap_or(&empty);
        if let Some(content) = map.get(marker.as_str()) {
            let first_line = content.split('\n').next().unwrap_or("");
            if first_line == MARKER_HEADER {
                return Ok(ScaffoldState::Present);
            }
            return Ok(ScaffoldState::Foreign);
        }
        let mut already_equal: HashSet<String> = HashSet::new();
        let mut conflicts: Vec<String> = Vec::new();
        for file in files {
            match map.get(file.path().as_str()) {
                None => {}
                Some(existing) if existing == file.content() => {
                    already_equal.insert(file.path().as_str().to_string());
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
        Ok(ScaffoldState::Absent { already_equal })
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
        let mut state = self.state.lock().unwrap();
        let key = scaffold_key(&repo, &branch);
        state.record_read_call(Self::TOOL_NAME, &key);
        match Self::observe(state.scaffolds.get(&key), &marker, &files)? {
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
        let _message: CommitHeadline = get(inputs, "message")?;
        let mut state = self.state.lock().unwrap();
        let key = scaffold_key(&repo, &branch);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let outputs = Self::outputs_for(&repo, &branch, &marker);
        match Self::observe(state.scaffolds.get(&key), &marker, &files)? {
            ScaffoldState::Present => Ok(Ensured {
                outputs,
                changed: false,
            }),
            ScaffoldState::Foreign => Err(Self::foreign_conflict(&repo, &branch, &marker)),
            ScaffoldState::Absent { already_equal } => {
                let marker_content = Self::marker_content(&files);
                let map = state.scaffolds.entry(key).or_default();
                for file in &files {
                    if !already_equal.contains(file.path().as_str()) {
                        map.insert(file.path().as_str().to_string(), file.content().to_string());
                    }
                }
                map.insert(marker.as_str().to_string(), marker_content);
                Ok(Ensured {
                    outputs,
                    changed: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::DomainType;

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("acme/widget").unwrap()
    }

    fn branch() -> GitBranchName {
        GitBranchName::parse("main").unwrap()
    }

    fn marker() -> RepoPath {
        RepoPath::parse(".willikins-scaffold").unwrap()
    }

    fn seed_files() -> Vec<RepoFile> {
        vec![
            RepoFile::new(RepoPath::parse("BUILD.bazel").unwrap(), "# reserve\n").unwrap(),
            RepoFile::new(RepoPath::parse("ios/BUILD.bazel").unwrap(), "ios content\n").unwrap(),
        ]
    }

    fn inputs(files: Vec<RepoFile>) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
        inputs.insert(PortName::parse("marker").unwrap(), Value::known(marker()));
        inputs.insert(PortName::parse("files").unwrap(), Value::known_list(files));
        inputs.insert(
            PortName::parse("message").unwrap(),
            Value::known(CommitHeadline::parse("feat: scaffold").unwrap()),
        );
        inputs
    }

    fn tool() -> GitHubScaffoldEnsure {
        GitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_is_idempotent_and_reports_unchanged_on_the_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&inputs(seed_files()), &token).unwrap();
        let second = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(!second.changed);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_scaffold_half_applied_by_hand_still_converges() {
        let state = Arc::new(Mutex::new(FakeState::new().with_scaffold_files(
            &repo(),
            &branch(),
            &[("BUILD.bazel", "# reserve\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_conflicts_on_a_differing_seed_path_and_writes_nothing() {
        let state = Arc::new(Mutex::new(FakeState::new().with_scaffold_files(
            &repo(),
            &branch(),
            &[("BUILD.bazel", "someone else's content\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(seed_files()), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains("BUILD.bazel"));
        assert!(!err.message.contains("someone else's content"));
    }

    #[test]
    fn read_reports_foreign_when_the_marker_is_not_ours() {
        let state = Arc::new(Mutex::new(FakeState::new().with_scaffold_files(
            &repo(),
            &branch(),
            &[(".willikins-scaffold", "not ours\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_refuses_zero_files_before_touching_state() {
        let tool = tool();
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(Vec::new()), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Invalid);
    }
}
