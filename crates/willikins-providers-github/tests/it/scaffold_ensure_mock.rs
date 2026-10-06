//! Acceptance test 8: `github.scaffold.ensure` against mocks, each row of
//! decision (b)/(c)'s table
//! (`docs/plans/2026-09-30-milestone-3g-file-writing.md`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{GitHubClient, GitHubScaffoldEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, partial_json_body};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{CommitHeadline, DomainType, GitBranchName, GitHubRepo, RepoFile, RepoPath};

/// The same algorithm `client::git_blob_sha` uses (`pub(crate)`, so this
/// integration test crate cannot call it directly): SHA-1 over
/// `blob <byte length>\0<bytes>`.
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

fn client_against(url: String) -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    Arc::new(GitHubClient::new(Http::new(url, Vec::new(), credential)))
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("acme/widget").unwrap()
}

fn branch() -> GitBranchName {
    GitBranchName::parse("main").unwrap()
}

fn marker() -> RepoPath {
    RepoPath::parse(".willikins-scaffold").unwrap()
}

/// Two seed files: one at the root, one nested a directory deep, so a
/// scenario can prove both "resolved in the same tree as the marker" and
/// "a subdirectory is walked only when the marker turns out absent".
fn seed_files() -> Vec<RepoFile> {
    vec![
        RepoFile::new(RepoPath::parse("BUILD.bazel").unwrap(), "# reserve\n").unwrap(),
        RepoFile::new(RepoPath::parse("ios/BUILD.bazel").unwrap(), "ios content\n").unwrap(),
    ]
}

fn scaffold_inputs(files: Vec<RepoFile>) -> Inputs {
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

fn tree_entry(name: &str, mode: &str, kind: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({
        "path": name, "mode": mode, "type": kind, "sha": sha, "size": 1,
        "url": "https://api.github.com/x",
    })
}

/// `ref` then `commit`, both against `head`, for every scenario below.
fn mock_ref_and_commit(provider: &mut MockProvider, head: &str, root_tree: &str) {
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": head}}).to_string())
        .create();
    provider
        .mock("GET", &format!("/repos/acme/widget/git/commits/{head}"))
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": root_tree}}).to_string())
        .create();
}

#[allow(clippy::needless_pass_by_value)] // every call site passes an owned `vec![...]` literal
fn mock_tree(provider: &mut MockProvider, sha: &str, entries: Vec<serde_json::Value>) {
    provider
        .mock("GET", &format!("/repos/acme/widget/git/trees/{sha}"))
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": sha, "tree": entries}).to_string())
        .create();
}

// -----------------------------------------------------------------------
// `ensure`: decision (a)'s own table, milestone 3l, task S3.
// -----------------------------------------------------------------------

/// Three files whose byte-order-smallest path (`.editorconfig`, `'.'` <
/// `'B'` < `'i'`) is the root file S3 writes through the Contents API.
fn three_files() -> Vec<RepoFile> {
    vec![
        RepoFile::new(RepoPath::parse(".editorconfig").unwrap(), "root = true\n").unwrap(),
        RepoFile::new(RepoPath::parse("BUILD.bazel").unwrap(), "# reserve\n").unwrap(),
        RepoFile::new(RepoPath::parse("ios/BUILD.bazel").unwrap(), "ios content\n").unwrap(),
    ]
}

/// A [`Sleeper`] that records every requested duration instead of
/// waiting, so a test can assert exactly which waits `ensure` recorded,
/// in order, without actually pausing.
#[derive(Default)]
struct RecordingSleeper(Mutex<Vec<Duration>>);

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.0.lock().unwrap().push(duration);
    }
}

impl RecordingSleeper {
    fn waits(&self) -> Vec<Duration> {
        self.0.lock().unwrap().clone()
    }
}

/// A [`Sleeper`] that returns immediately -- used for [`Http`]'s own
/// 429/5xx retry backoff in these tests, which is not what any
/// acceptance here is pinning (that is [`RecordingSleeper`]'s job, on
/// [`GitHubClient::with_sleeper`] instead).
struct NoopSleeper;

impl Sleeper for NoopSleeper {
    fn sleep(&self, _duration: Duration) {}
}

/// A client whose own [`GitHubClient::pause`] waits are captured by a
/// fresh [`RecordingSleeper`] (acceptance 7, 8's recorded waits), while
/// [`Http`]'s unrelated retry backoff (acceptance 6's lost-response
/// case) never actually sleeps.
fn client_against_with_sleeper(url: String) -> (Arc<GitHubClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(url, Vec::new(), credential).with_sleeper(Arc::new(NoopSleeper));
    let recorder = Arc::new(RecordingSleeper::default());
    let scaffold_sleeper: Arc<dyn Sleeper> = Arc::clone(&recorder) as Arc<dyn Sleeper>;
    let client = GitHubClient::new(http).with_sleeper(scaffold_sleeper);
    (Arc::new(client), recorder)
}

/// The three requests [`GitHubScaffoldEnsure`]'s `observe_without_branch`
/// issues once the branch ref read fails: the branch ref itself (`404`),
/// `GET /repos` (`default_branch` equal to [`branch`]), and an empty
/// `branches` listing -- pinned with `.expect(calls)` each, since a
/// scenario that re-observes while still empty (a `409` retry, or a
/// ref-visibility poll) re-issues this same triple every time.
fn mock_empty_repository(
    provider: &mut MockProvider,
    calls: usize,
) -> (mockito::Mock, mockito::Mock, mockito::Mock) {
    let ref_mock = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(404)
        .expect(calls)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({"visibility": "private", "topics": [], "default_branch": "main"})
                .to_string(),
        )
        .expect(calls)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .with_status(200)
        .with_body("[]")
        .expect(calls)
        .create();
    (ref_mock, repo_mock, branches_mock)
}

// -----------------------------------------------------------------------
// `read`: decision (b)'s table.
// -----------------------------------------------------------------------

#[test]
fn read_reports_present_and_never_walks_a_seed_path() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            tree_entry(".willikins-scaffold", "100644", "blob", "marker-sha"),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    // Proves "Present issues no tree walk past the marker": if `read`
    // resolved the seed paths too, this would be hit.
    let ios_tree = provider
        .mock("GET", "/repos/acme/widget/git/trees/ios-tree")
        .expect(0)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "content": STANDARD.encode("managed-by: willikins\n"),
                "encoding": "base64",
            })
            .to_string(),
        )
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    assert!(
        matches!(observation, Observation::Present(_)),
        "{observation:?}"
    );
    ios_tree.assert();
}

#[test]
fn read_reports_foreign_when_the_markers_first_line_is_wrong() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".willikins-scaffold",
            "100644",
            "blob",
            "marker-sha",
        )],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "content": STANDARD.encode("not ours\n"),
                "encoding": "base64",
            })
            .to_string(),
        )
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "{observation:?}"
    );
}

/// The first line must *equal* the header, not merely start with it.
/// Adversarial pass (render and write): a `starts_with` mutation of the
/// comparison survived every test while the only foreign first line was
/// `not ours`.
#[test]
fn read_reports_foreign_when_the_markers_first_line_only_starts_with_the_header() {
    for first_line in [
        "managed-by: willikins-impostor\n",
        "managed-by: willikins \n",
        "managed-by: willikins\r\n",
    ] {
        let mut provider = MockProvider::start();
        mock_ref_and_commit(&mut provider, "head-1", "root-tree");
        mock_tree(
            &mut provider,
            "root-tree",
            vec![tree_entry(
                ".willikins-scaffold",
                "100644",
                "blob",
                "marker-sha",
            )],
        );
        provider
            .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "content": STANDARD.encode(first_line),
                    "encoding": "base64",
                })
                .to_string(),
            )
            .create();

        let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
        let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
        assert!(
            matches!(observation, Observation::Foreign),
            "{first_line:?}: {observation:?}"
        );
    }
}

#[test]
fn read_reports_foreign_when_the_marker_path_is_a_directory() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".willikins-scaffold",
            "040000",
            "tree",
            "marker-as-dir-sha",
        )],
    );
    let blob = provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-as-dir-sha")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "{observation:?}"
    );
    blob.assert();
}

#[test]
fn read_reports_absent_and_predicts_the_pass_through_outputs() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    // No marker entry at all; `BUILD.bazel` absent too; `ios` is a
    // directory whose own tree also lacks `BUILD.bazel`.
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    mock_tree(&mut provider, "ios-tree", vec![]);

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert_eq!(
        predicted
            .get(&PortName::parse("repo").unwrap())
            .unwrap()
            .render()
            .to_string(),
        "acme/widget"
    );
    assert_eq!(
        predicted
            .get(&PortName::parse("branch").unwrap())
            .unwrap()
            .render()
            .to_string(),
        "main"
    );
    assert_eq!(
        predicted
            .get(&PortName::parse("marker").unwrap())
            .unwrap()
            .render()
            .to_string(),
        ".willikins-scaffold"
    );
}

#[test]
fn read_conflicts_naming_the_differing_path_and_never_its_content() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            tree_entry("BUILD.bazel", "100644", "blob", "some-other-sha"),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    let ios_sha = blob_sha(seed_files()[1].content().as_bytes());
    mock_tree(
        &mut provider,
        "ios-tree",
        vec![tree_entry("BUILD.bazel", "100644", "blob", &ios_sha)],
    );

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("BUILD.bazel"), "{}", err.message);
    assert!(!err.message.contains("ios/BUILD.bazel"), "{}", err.message);
    assert!(!err.message.contains("# reserve"), "{}", err.message);
    assert!(!err.message.contains("ios content"), "{}", err.message);
}

/// Every differing path is named, not only the first one found.
#[test]
fn read_conflict_names_every_differing_path_not_only_the_first() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            tree_entry("BUILD.bazel", "100644", "blob", "some-other-sha"),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    mock_tree(
        &mut provider,
        "ios-tree",
        vec![tree_entry(
            "BUILD.bazel",
            "100644",
            "blob",
            "yet-another-sha",
        )],
    );

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("BUILD.bazel"), "{}", err.message);
    assert!(err.message.contains("ios/BUILD.bazel"), "{}", err.message);
}

/// A marker blob with the right content but the *executable* mode is
/// not the `100644` this tool ever writes, so it is `Foreign`, not
/// `Present` -- decision (b)'s table: "any other first line, **or the
/// marker path is not a `100644` blob**".
#[test]
fn read_reports_foreign_when_the_marker_is_a_100755_executable() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".willikins-scaffold",
            "100755",
            "blob",
            "marker-sha",
        )],
    );
    let blob = provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "{observation:?}"
    );
    blob.assert();
}

/// A seed path present as a `100755` executable, even with the exact
/// sha this tool would write as a plain `100644` file, is still a
/// conflict: the mode differs, and this tool never changes a mode.
#[test]
fn read_conflicts_when_a_seed_path_is_present_as_an_executable_with_a_matching_sha() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    let build_bazel_sha = blob_sha(seed_files()[0].content().as_bytes());
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            tree_entry("BUILD.bazel", "100755", "blob", &build_bazel_sha),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    mock_tree(&mut provider, "ios-tree", vec![]);

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("BUILD.bazel"), "{}", err.message);
}

/// A seed path present as a symlink (a `NonBlob`) is a conflict too,
/// whatever its sha.
#[test]
fn read_conflicts_when_a_seed_path_is_a_symlink() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            tree_entry("BUILD.bazel", "120000", "blob", "symlink-target-sha"),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    mock_tree(&mut provider, "ios-tree", vec![]);

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("BUILD.bazel"), "{}", err.message);
}

/// A seed path beneath an existing *file* (`ios` is a blob, the seed is
/// `ios/BUILD.bazel`) is not absent: a tree cannot hold both, so the
/// commit could only fail or replace `ios` with a directory, deleting
/// it. Adversarial pass (render and write): refused as a `Conflict`
/// naming the path, and nothing is committed.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_seed_path_beneath_an_existing_file_conflicts_and_is_never_committed() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "100644", "blob", "ios-is-a-file-sha")],
    );
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict, "{}", err.message);
    assert!(err.message.contains("ios/BUILD.bazel"), "{}", err.message);

    let token = SinkToken::new();
    let err = tool
        .ensure(&scaffold_inputs(seed_files()), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict, "{}", err.message);
    commit.assert();
}

/// The marker beneath an existing file is `Foreign`, never `Absent`.
#[test]
fn a_marker_beneath_an_existing_file_is_foreign() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("app", "100644", "blob", "app-is-a-file-sha")],
    );
    let mut inputs = scaffold_inputs(seed_files());
    inputs.insert(
        PortName::parse("marker").unwrap(),
        Value::known(RepoPath::parse("app/.willikins-scaffold").unwrap()),
    );

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "{observation:?}"
    );
}

/// A tree listing GitHub marks `truncated: true` may have dropped the very
/// entry a declared path names; reading its absence as `Absent` would
/// hand an existing file to `createCommitOnBranch` as a new one, an
/// overwrite. Adversarial pass (render and write): refused as `Provider`,
/// and nothing is committed.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_truncated_tree_is_a_provider_failure_never_absent() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(
            serde_json::json!({"sha": "root-tree", "tree": [], "truncated": true}).to_string(),
        )
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider, "{}", err.message);

    let token = SinkToken::new();
    let err = tool
        .ensure(&scaffold_inputs(seed_files()), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider, "{}", err.message);
    commit.assert();
}

/// Milestone 3l, decision (b): a `404` on the branch ref alone no longer
/// settles `NotFound` -- this repository exists and is non-empty, so the
/// two new calls run first, and the message stays exactly what it always
/// was (acceptance 10: the one existing test this milestone's table
/// changes, gaining the two mocks it now needs).
#[test]
fn read_reports_not_found_when_the_branch_does_not_exist() {
    let mut provider = MockProvider::start();
    let ref_mock = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(404)
        .expect(1)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({"visibility": "private", "topics": [], "default_branch": "main"})
                .to_string(),
        )
        .expect(1)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .with_status(200)
        .with_body(serde_json::json!([{"name": "main"}]).to_string())
        .expect(1)
        .create();
    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert_eq!(
        err.message,
        "branch `main` does not exist on `acme/widget`; this tool never creates one"
    );
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
}

// -----------------------------------------------------------------------
// Milestone 3l, task S2, decision (b): a repository that does not exist
// yet, or is empty.
// -----------------------------------------------------------------------

/// Row: `404`/`409` on the branch ref, then `404` on `GET /repos` --
/// `RepositoryAbsent`, mapped to `Absent` with the pass-through outputs.
#[test]
fn read_reports_absent_when_the_repository_does_not_exist() {
    for branch_status in [404, 409] {
        let mut provider = MockProvider::start();
        let ref_mock = provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(branch_status)
            .expect(1)
            .create();
        let repo_mock = provider
            .mock("GET", "/repos/acme/widget")
            .with_status(404)
            .expect(1)
            .create();
        let branches_mock = provider
            .mock("GET", "/repos/acme/widget/branches?per_page=1")
            .expect(0)
            .create();

        let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
        let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("{branch_status}: expected Absent, got {observation:?}");
        };
        assert_eq!(
            predicted
                .get(&PortName::parse("repo").unwrap())
                .unwrap()
                .render()
                .to_string(),
            "acme/widget",
            "{branch_status}"
        );
        ref_mock.assert();
        repo_mock.assert();
        branches_mock.assert();
    }
}

/// The same row, through `ensure`: the repository-absent `NotFound`,
/// naming `github.repo.ensure` as the tool that creates one. No write.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_not_found_naming_repo_ensure_when_the_repository_does_not_exist() {
    for branch_status in [404, 409] {
        let mut provider = MockProvider::start();
        let ref_mock = provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(branch_status)
            .expect(1)
            .create();
        let repo_mock = provider
            .mock("GET", "/repos/acme/widget")
            .with_status(404)
            .expect(1)
            .create();
        let branches_mock = provider
            .mock("GET", "/repos/acme/widget/branches?per_page=1")
            .expect(0)
            .create();
        let commit = provider.mock("POST", "/graphql").expect(0).create();
        let first_file = provider
            .mock("PUT", "/repos/acme/widget/contents/BUILD.bazel")
            .expect(0)
            .create();

        let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
        let token = SinkToken::new();
        let err = tool
            .ensure(&scaffold_inputs(seed_files()), &token)
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::NotFound, "{branch_status}");
        assert_eq!(
            err.message,
            "`acme/widget` does not exist; this tool never creates a repository \
             (github.repo.ensure does)",
            "{branch_status}"
        );
        ref_mock.assert();
        repo_mock.assert();
        branches_mock.assert();
        commit.assert();
        first_file.assert();
    }
}

/// Row: `404`/`409` on the branch ref, `200` on `GET /repos` with
/// `default_branch` equal to the requested `branch`, and an empty
/// `branches` listing -- `Empty`, mapped to `Absent` with the
/// pass-through outputs.
#[test]
fn read_reports_absent_when_the_repository_is_empty_and_branch_is_the_default() {
    for branch_status in [404, 409] {
        let mut provider = MockProvider::start();
        let ref_mock = provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(branch_status)
            .expect(1)
            .create();
        let repo_mock = provider
            .mock("GET", "/repos/acme/widget")
            .with_status(200)
            .with_body(
                serde_json::json!({"visibility": "private", "topics": [], "default_branch": "main"})
                    .to_string(),
            )
            .expect(1)
            .create();
        let branches_mock = provider
            .mock("GET", "/repos/acme/widget/branches?per_page=1")
            .with_status(200)
            .with_body("[]")
            .expect(1)
            .create();

        let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
        let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("{branch_status}: expected Absent, got {observation:?}");
        };
        assert_eq!(
            predicted
                .get(&PortName::parse("branch").unwrap())
                .unwrap()
                .render()
                .to_string(),
            "main",
            "{branch_status}"
        );
        ref_mock.assert();
        repo_mock.assert();
        branches_mock.assert();
    }
}

/// Acceptance 4, "Fresh": an empty repository, three files -- exactly one
/// `PUT` of the root file (`.editorconfig`), then one `createCommitOnBranch`
/// whose `additions` are the other two files plus the marker and whose
/// `expectedHeadOid` is the root commit's own head. `changed: true`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_fresh_empty_repository_puts_the_root_file_then_commits_the_rest() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 1);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .match_body(json_body(serde_json::json!({
            "message": "feat: scaffold\n\nThe first commit of an empty repository, seeded by \
                         willikins. Marker: .willikins-scaffold.",
            "content": STANDARD.encode("root = true\n"),
            "branch": "main",
        })))
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );

    let build_bazel_sha = blob_sha(files[1].content().as_bytes());
    let ios_build_sha = blob_sha(files[2].content().as_bytes());
    let expected_marker_content = format!(
        "managed-by: willikins\n{editorconfig_sha} .editorconfig\n{build_bazel_sha} \
         BUILD.bazel\n{ios_build_sha} ios/BUILD.bazel\n"
    );
    let commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(serde_json::json!({
            "variables": {
                "input": {
                    "expectedHeadOid": "root-head",
                    "fileChanges": {
                        "additions": [
                            {
                                "path": ".willikins-scaffold",
                                "contents": STANDARD.encode(&expected_marker_content),
                            },
                            {
                                "path": "BUILD.bazel",
                                "contents": STANDARD.encode("# reserve\n"),
                            },
                            {
                                "path": "ios/BUILD.bazel",
                                "contents": STANDARD.encode("ios content\n"),
                            },
                        ],
                    },
                },
            },
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Milestone 3l, task X1 (adversarial pass): decision (a) step 3 says
/// `ensure` decides what the first-file `PUT` accomplished only from a
/// fresh re-observe, "never ... from the `PUT`'s own body". A mutant that
/// trusted a `2xx` `PUT` response directly -- skipping the re-observe and
/// synthesizing a head instead of reading one -- passed every other
/// acceptance 4 assertion in this file (the commit's `additions` still
/// excluded the root file, since the mutant also knew the root path by
/// construction) and was caught only by the ref-visibility tests below,
/// never by this scenario's own happy path. This test closes that gap
/// directly: it plants a second `POST /graphql` mock that would only ever
/// match a commit compare-and-swapped on an **empty** `expectedHeadOid`
/// (what a `PUT`-trusting implementation has nothing real to put there)
/// and pins it `.expect(0)`, alongside the real mock keyed on the
/// re-observed head.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_decides_the_first_write_landed_from_a_re_observe_never_from_the_puts_own_body() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 1);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );

    // Never hit by a conforming implementation: nothing here read an
    // `expectedHeadOid` of `""`, since the only way to reach `ensure`'s
    // commit step is through a re-observe that reports a real sha.
    let poisoned_commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(serde_json::json!({
            "variables": {"input": {"expectedHeadOid": ""}},
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "poison"}}}})
                .to_string(),
        )
        .expect(0)
        .create();
    let real_commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(serde_json::json!({
            "variables": {"input": {"expectedHeadOid": "root-head"}},
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    poisoned_commit.assert();
    real_commit.assert();
}

/// Acceptance 5, "Resume after the root commit": the repository already
/// holds only the root file, byte-equal, no marker -- no `PUT` at all; one
/// `createCommitOnBranch` without the root file. `changed: true`. This
/// never reaches `ScaffoldState::Empty` (the branch already has a head),
/// so it proves S3's new code stays out of the way of the ordinary resume
/// path.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_resumes_after_the_root_commit_without_a_put() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .expect(0)
        .create();

    let build_bazel_sha = blob_sha(files[1].content().as_bytes());
    let ios_build_sha = blob_sha(files[2].content().as_bytes());
    let expected_marker_content = format!(
        "managed-by: willikins\n{editorconfig_sha} .editorconfig\n{build_bazel_sha} \
         BUILD.bazel\n{ios_build_sha} ios/BUILD.bazel\n"
    );
    let commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(serde_json::json!({
            "variables": {
                "input": {
                    "expectedHeadOid": "root-head",
                    "fileChanges": {
                        "additions": [
                            {
                                "path": ".willikins-scaffold",
                                "contents": STANDARD.encode(&expected_marker_content),
                            },
                            {
                                "path": "BUILD.bazel",
                                "contents": STANDARD.encode("# reserve\n"),
                            },
                            {
                                "path": "ios/BUILD.bazel",
                                "contents": STANDARD.encode("ios content\n"),
                            },
                        ],
                    },
                },
            },
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    put_mock.assert();
    commit.assert();
}

/// Acceptance 6, "Lost response", first half: the first-file `PUT`
/// answers `502` four times (`Http::put`'s own retries: `MAX_RETRIES = 3`,
/// four attempts total) while a re-observe sees the root file landed
/// anyway -- the scaffold finishes, with no scaffold-level wait (a `502`
/// is not a `409`, so decision (a) reports from the re-observe
/// immediately rather than retrying the `PUT` itself).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_finishes_when_a_lost_put_response_already_landed_the_root_file() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 1);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(502)
        .expect(4)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    assert!(sleeper.waits().is_empty());
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Acceptance 6, "Lost response", second half: the first-file `PUT`
/// answers `422` ("the file already exists") -- never retried by `Http`
/// (`422` is not a retryable status) -- with a re-observe showing the
/// root file landed. Same outcome as the `502` case.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_finishes_when_a_422_on_the_put_already_landed_the_root_file() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 1);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(422)
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    assert!(sleeper.waits().is_empty());
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Acceptance 7, "Unavailable", first half: the first-file `PUT` answers
/// `409` twice, then `201` -- two recorded waits of `UNAVAILABLE_WAIT`
/// (`2s`), then the normal commit. Every `409` re-observe still reads
/// empty, so each one re-issues the full empty-repository triple
/// (`mock_empty_repository`'s own `calls` -- one initial, one per `409`).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_retries_the_put_after_409_with_recorded_waits_then_commits() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 3);
    let put_409 = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(409)
        .expect(2)
        .create();
    let put_201 = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    assert_eq!(sleeper.waits(), vec![Duration::from_secs(2); 2]);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_409.assert();
    put_201.assert();
    commit.assert();
}

/// Acceptance 7, "Unavailable", second half: five `409`s in a row --
/// `Err` after `MAX_UNAVAILABLE_ATTEMPTS` (five) attempts, no commit.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_gives_up_after_max_unavailable_attempts_on_persistent_409s() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 6);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(409)
        .expect(5)
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert_eq!(
        err.message,
        "GitHub refused to create the first file of an empty repository"
    );
    assert_eq!(sleeper.waits(), vec![Duration::from_secs(2); 4]);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Acceptance 8, "Ref lag", first half: after a `201`, the re-observe
/// reads empty three times, then `Absent` -- three recorded waits of
/// `REF_VISIBLE_WAIT` (`300ms`). One empty triple for the initial
/// observe plus one per lagging re-observe (`calls: 4`).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_polls_for_ref_visibility_after_a_successful_put_with_recorded_waits() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 4);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    assert_eq!(sleeper.waits(), vec![Duration::from_millis(300); 3]);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Acceptance 8, "Ref lag", second half: the re-observe keeps reading
/// empty for the one unconditional read plus every one of
/// `MAX_REF_VISIBLE_POLLS` (ten) polls -- eleven empty reads in total,
/// ten recorded waits, then the "not visible yet" `Provider` error. No
/// commit.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_ref_not_visible_after_exhausting_ref_visibility_polls() {
    let mut provider = MockProvider::start();
    let files = three_files();
    // One initial empty triple, plus eleven more for the eleven empty
    // re-observes after the `PUT`.
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 12);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let (client, sleeper) = client_against_with_sleeper(provider.url());
    let tool = GitHubScaffoldEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(
        err.message,
        "the first commit of `acme/widget` landed, but `main` is not visible yet; re-run \
         this document to finish the scaffold"
    );
    assert_eq!(sleeper.waits(), vec![Duration::from_millis(300); 10]);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Acceptance 9, one of the refusal/edge rows: a one-file scaffold whose
/// single file is also the root file commits only the marker (`files`
/// filtered by `already_equal` is empty once the root file is excluded).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_an_empty_repository_with_one_file_commits_only_the_marker() {
    let mut provider = MockProvider::start();
    let one_file =
        vec![RepoFile::new(RepoPath::parse(".editorconfig").unwrap(), "root = true\n").unwrap()];
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 1);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();

    let editorconfig_sha = blob_sha(one_file[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "root-head", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".editorconfig",
            "100644",
            "blob",
            &editorconfig_sha,
        )],
    );
    let expected_marker_content =
        format!("managed-by: willikins\n{editorconfig_sha} .editorconfig\n");
    let commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(serde_json::json!({
            "variables": {
                "input": {
                    "expectedHeadOid": "root-head",
                    "fileChanges": {
                        "additions": [
                            {
                                "path": ".willikins-scaffold",
                                "contents": STANDARD.encode(&expected_marker_content),
                            },
                        ],
                    },
                },
            },
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(one_file), &token).unwrap();
    assert!(ensured.changed);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
}

/// Row: `404`/`409` on the branch ref, `200` on `GET /repos` with
/// `default_branch` *not* equal to the requested `branch`, and an empty
/// `branches` listing -- `Conflict` naming `repo`, the requested `branch`
/// and the repository's actual default, at `read` and `ensure` alike. No
/// write.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn read_and_ensure_conflict_when_the_empty_repositorys_default_branch_differs() {
    for branch_status in [404, 409] {
        let mut provider = MockProvider::start();
        // `read` then `ensure`, each against the same provider: every GET
        // below is issued exactly twice.
        let ref_mock = provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(branch_status)
            .expect(2)
            .create();
        let repo_mock = provider
            .mock("GET", "/repos/acme/widget")
            .with_status(200)
            .with_body(
                serde_json::json!({"visibility": "private", "topics": [], "default_branch": "trunk"})
                    .to_string(),
            )
            .expect(2)
            .create();
        let branches_mock = provider
            .mock("GET", "/repos/acme/widget/branches?per_page=1")
            .with_status(200)
            .with_body("[]")
            .expect(2)
            .create();
        let commit = provider.mock("POST", "/graphql").expect(0).create();
        // Milestone 3l, acceptance 9: a mismatched default branch writes
        // nothing, including no first-file `PUT`.
        let first_file = provider
            .mock("PUT", "/repos/acme/widget/contents/BUILD.bazel")
            .expect(0)
            .create();

        let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
        let read_err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
        assert_eq!(read_err.kind, ToolErrorKind::Conflict, "{branch_status}");
        assert_eq!(
            read_err.message,
            "`acme/widget` is empty, and its first commit can only land on its default \
             branch `trunk`, not `main`; name `trunk` in this document, or change the \
             organisation's default branch name before the repository is created",
            "{branch_status}"
        );

        let token = SinkToken::new();
        let ensure_err = tool
            .ensure(&scaffold_inputs(seed_files()), &token)
            .unwrap_err();
        assert_eq!(ensure_err.kind, ToolErrorKind::Conflict, "{branch_status}");
        assert_eq!(ensure_err.message, read_err.message, "{branch_status}");
        first_file.assert();
        ref_mock.assert();
        repo_mock.assert();
        branches_mock.assert();
        commit.assert();
    }
}

/// Row: `409` on the branch ref (not `404`), `200` on `GET /repos`, and a
/// non-empty `branches` listing -- the repository is not actually empty,
/// but the ref call's own `409` means it may simply not be visible yet:
/// `Provider`, "not available yet", never the missing-branch `NotFound`.
#[test]
fn read_reports_provider_unavailable_when_the_branch_ref_is_409_on_a_non_empty_repository() {
    let mut provider = MockProvider::start();
    let ref_mock = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(409)
        .expect(1)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({"visibility": "private", "topics": [], "default_branch": "main"})
                .to_string(),
        )
        .expect(1)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .with_status(200)
        .with_body(serde_json::json!([{"name": "main"}]).to_string())
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(
        err.message,
        "`acme/widget` is not available yet (GitHub may still be creating it); re-run this \
         document"
    );
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
}

/// Row: `404`/`409` on the branch ref, `200` on `GET /repos` whose
/// `default_branch` is absent, or is not a valid [`GitBranchName`] --
/// `Provider`, a static message naming `repo` alone, never GitHub's own
/// string. The branches call is never made (`.expect(0)`).
#[test]
fn read_is_provider_when_default_branch_is_absent_or_invalid() {
    for branch_status in [404, 409] {
        for default_branch in [serde_json::Value::Null, serde_json::json!("..bad..")] {
            let mut provider = MockProvider::start();
            let ref_mock = provider
                .mock("GET", "/repos/acme/widget/git/ref/heads/main")
                .with_status(branch_status)
                .expect(1)
                .create();
            let repo_mock = provider
                .mock("GET", "/repos/acme/widget")
                .with_status(200)
                .with_body(
                    serde_json::json!({
                        "visibility": "private",
                        "topics": [],
                        "default_branch": default_branch,
                    })
                    .to_string(),
                )
                .expect(1)
                .create();
            let branches_mock = provider
                .mock("GET", "/repos/acme/widget/branches?per_page=1")
                .expect(0)
                .create();

            let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
            let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
            assert_eq!(
                err.kind,
                ToolErrorKind::Provider,
                "{branch_status} {default_branch:?}"
            );
            assert_eq!(
                err.message, "`acme/widget` did not report a usable default branch",
                "{branch_status} {default_branch:?}"
            );
            ref_mock.assert();
            repo_mock.assert();
            branches_mock.assert();
        }
    }
}

/// No row reached through a successful branch-ref read ever consults
/// `GET /repos` or the branches listing (`.expect(0)` on both): the new
/// calls are only ever made once `get_branch_head` has already failed.
#[test]
fn a_successful_branch_head_never_consults_the_repository_or_branches() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    mock_tree(&mut provider, "ios-tree", vec![]);
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .expect(0)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let observation = tool.read(&scaffold_inputs(seed_files())).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
    repo_mock.assert();
    branches_mock.assert();
}

/// Any branch-ref failure other than `404`/`409` (here, a `422`, which
/// `willikins-providers-http` never retries -- a `5xx` would be, muddying
/// the request count this test pins) is reported as-is (`to_tool_error`)
/// without ever consulting the repository or the branches listing --
/// never `Absent`.
#[test]
fn any_other_branch_head_error_never_consults_the_repository() {
    let mut provider = MockProvider::start();
    let ref_mock = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(422)
        .expect(1)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .expect(0)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
}

/// A `301` from `GET /repos` (the repository renamed away) is, per
/// decision (b), "any other error": `to_tool_error`, failing loudly,
/// never treated as `RepositoryAbsent`/`Absent`.
#[test]
fn a_301_from_get_repos_fails_loudly_never_absent() {
    let mut provider = MockProvider::start();
    let ref_mock = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(404)
        .expect(1)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(301)
        .expect(1)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let err = tool.read(&scaffold_inputs(seed_files())).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider, "{}", err.message);
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
}

// -----------------------------------------------------------------------
// `Invalid`, before any request.
// -----------------------------------------------------------------------

/// An address nothing listens on: if any of these shape refusals ever
/// made a request first, the result would be `Provider` (a connection
/// failure), not `Invalid` -- so this is itself part of the assertion.
fn unreachable_client() -> Arc<GitHubClient> {
    client_against("http://127.0.0.1:1".to_string())
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_zero_files_before_any_request() {
    let tool = GitHubScaffoldEnsure::new(unreachable_client());
    let token = SinkToken::new();
    let err = tool
        .ensure(&scaffold_inputs(Vec::new()), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_over_64_files_before_any_request() {
    let files: Vec<RepoFile> = (0..65)
        .map(|i| RepoFile::new(RepoPath::parse(&format!("f{i}.txt")).unwrap(), "x").unwrap())
        .collect();
    let tool = GitHubScaffoldEnsure::new(unreachable_client());
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

/// 64 files (the maximum `validate_shape` allows) each with a long
/// `RepoPath` produce a marker over `RepoFile`'s own 65,536-character
/// bound. That must be caught as `Invalid` before any request, on both
/// `read` and `ensure` -- decision (b)'s table puts every shape refusal
/// "before any request", and this one is only knowable once every
/// file's path is in hand, not from the file count alone.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_over_long_marker_is_invalid_before_any_request_on_read_and_ensure() {
    let files: Vec<RepoFile> = (0..64)
        .map(|i| {
            let path = format!("{}{i}.txt", "a".repeat(990));
            RepoFile::new(RepoPath::parse(&path).unwrap(), "x").unwrap()
        })
        .collect();
    let tool = GitHubScaffoldEnsure::new(unreachable_client());

    let read_err = tool.read(&scaffold_inputs(files.clone())).unwrap_err();
    assert_eq!(read_err.kind, ToolErrorKind::Invalid);

    let token = SinkToken::new();
    let ensure_err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(ensure_err.kind, ToolErrorKind::Invalid);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_a_duplicate_path_before_any_request() {
    let path = RepoPath::parse("a.txt").unwrap();
    let files = vec![
        RepoFile::new(path.clone(), "one").unwrap(),
        RepoFile::new(path, "two").unwrap(),
    ];
    let tool = GitHubScaffoldEnsure::new(unreachable_client());
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

/// Declared paths where one is a directory of another -- two files, or a
/// file and the marker, either way round -- cannot all exist in one tree.
/// Knowable from inputs alone, so refused `Invalid` before any request on
/// both `read` and `ensure`, never left for GitHub to refuse (or the fake
/// to accept). Adversarial pass (render and write).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn declared_paths_nested_under_one_another_are_invalid_before_any_request() {
    let file = |path: &str| RepoFile::new(RepoPath::parse(path).unwrap(), "x").unwrap();
    let cases: Vec<(Vec<RepoFile>, &str)> = vec![
        (vec![file("a"), file("a/b")], ".willikins-scaffold"),
        (vec![file("a/b/c"), file("a/b")], ".willikins-scaffold"),
        (vec![file("ios/BUILD.bazel")], "ios"),
        (vec![file("BUILD.bazel")], "BUILD.bazel/.willikins-scaffold"),
    ];
    let tool = GitHubScaffoldEnsure::new(unreachable_client());
    let token = SinkToken::new();
    for (files, marker) in cases {
        let mut inputs = scaffold_inputs(files);
        inputs.insert(
            PortName::parse("marker").unwrap(),
            Value::known(RepoPath::parse(marker).unwrap()),
        );
        let err = tool.read(&inputs).unwrap_err();
        assert_eq!(
            err.kind,
            ToolErrorKind::Invalid,
            "{marker}: {}",
            err.message
        );
        let err = tool.ensure(&inputs, &token).unwrap_err();
        assert_eq!(
            err.kind,
            ToolErrorKind::Invalid,
            "{marker}: {}",
            err.message
        );
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_when_the_marker_path_is_also_one_of_files() {
    let files = vec![RepoFile::new(marker(), "collides with the marker").unwrap()];
    let tool = GitHubScaffoldEnsure::new(unreachable_client());
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

// -----------------------------------------------------------------------
// `ensure`: the write and its bounded retry.
// -----------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_commits_only_the_non_equal_files_plus_a_marker_in_the_shared_values_format() {
    let mut provider = MockProvider::start();
    let files = seed_files();
    let build_bazel_sha = blob_sha(files[0].content().as_bytes());
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![
            // Byte-equal already: skipped from `additions`.
            tree_entry("BUILD.bazel", "100644", "blob", &build_bazel_sha),
            tree_entry("ios", "040000", "tree", "ios-tree"),
        ],
    );
    // Absent at this path (the ordinary case for a file this scaffold has
    // never seeded before): must be committed. A *present-but-different*
    // path would instead be `Conflict` (decision (b)'s table; covered by
    // `read_conflicts_naming_the_differing_path_and_never_its_content`).
    mock_tree(&mut provider, "ios-tree", vec![]);

    let ios_build_sha = blob_sha(files[1].content().as_bytes());
    let expected_marker_content = format!(
        "managed-by: willikins\n{build_bazel_sha} BUILD.bazel\n{ios_build_sha} ios/BUILD.bazel\n"
    );
    let expected_body = serde_json::json!({
        "variables": {
            "input": {
                "expectedHeadOid": "head-1",
                "fileChanges": {
                    "additions": [
                        {
                            "path": ".willikins-scaffold",
                            "contents": STANDARD.encode(&expected_marker_content),
                        },
                        {
                            "path": "ios/BUILD.bazel",
                            "contents": STANDARD.encode("ios content\n"),
                        },
                    ],
                },
                "message": {
                    "headline": "feat: scaffold",
                    "body": "Seeded by willikins. Marker: .willikins-scaffold.",
                },
            },
        },
    });
    let commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(expected_body))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    // Outputs never carry a sha or a commit oid.
    assert_eq!(
        ensured
            .outputs
            .get(&PortName::parse("marker").unwrap())
            .unwrap()
            .render()
            .to_string(),
        ".willikins-scaffold"
    );
    commit.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_present_reports_unchanged_and_writes_nothing() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".willikins-scaffold",
            "100644",
            "blob",
            "marker-sha",
        )],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "content": STANDARD.encode("managed-by: willikins\n"),
                "encoding": "base64",
            })
            .to_string(),
        )
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(seed_files()), &token).unwrap();
    assert!(!ensured.changed);
    commit.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_foreign_conflicts_and_writes_nothing() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider, "head-1", "root-tree");
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry(
            ".willikins-scaffold",
            "100644",
            "blob",
            "marker-sha",
        )],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({"content": STANDARD.encode("not ours\n"), "encoding": "base64"})
                .to_string(),
        )
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool
        .ensure(&scaffold_inputs(seed_files()), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    commit.assert();
}

/// A failed commit attempt whose re-read finds the scaffold `Present`
/// (someone else's identical attempt landed first): `changed: false`,
/// never treated as this call's own success or failure.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_after_a_failed_commit_whose_re_read_is_present_reports_unchanged() {
    let mut provider = MockProvider::start();
    let files = seed_files();

    // First observe: Absent at head-1.
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(2)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree-1"}}).to_string())
        .expect(1)
        .create();
    mock_tree(
        &mut provider,
        "root-tree-1",
        vec![tree_entry("ios", "040000", "tree", "ios-tree-1")],
    );
    mock_tree(&mut provider, "ios-tree-1", vec![]);

    let commit = provider
        .mock("POST", "/graphql")
        .with_status(502)
        .expect(1)
        .create();

    // Re-observe after the failed commit: still head-1, but now Present.
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree-2"}}).to_string())
        .expect(1)
        .create();
    mock_tree(
        &mut provider,
        "root-tree-2",
        vec![tree_entry(
            ".willikins-scaffold",
            "100644",
            "blob",
            "marker-sha",
        )],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "content": STANDARD.encode("managed-by: willikins\n"),
                "encoding": "base64",
            })
            .to_string(),
        )
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(!ensured.changed);
    commit.assert();
}

/// Same head on re-read: the branch did not move, so retrying would fail
/// identically. Reported as the original `Provider` failure, with exactly
/// one commit attempt.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_a_same_head_failure_is_reported_as_provider_with_one_attempt() {
    let mut provider = MockProvider::start();
    let files = seed_files();
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(2)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree"}}).to_string())
        .expect(2)
        .create();
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    // This call's own resolve_tree_paths caches per call, so the second
    // observe (the re-read) fetches `ios-tree` again -- expect(2).
    provider
        .mock("GET", "/repos/acme/widget/git/trees/ios-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "ios-tree", "tree": []}).to_string())
        .expect(2)
        .create();
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(502)
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    commit.assert();
}

// -----------------------------------------------------------------------
// `ensure`: acceptance 11, milestone 3l, task S4, "Rule suffix (S4)" --
// a refused commit with an unmoved head, or a refused first-file `PUT`
// (non-409), names the branch rule types in force.
// -----------------------------------------------------------------------

/// Acceptance 11: a refused `createCommitOnBranch` whose re-read shows an
/// unmoved head gets the rule types in force on `branch` appended to its
/// message, sorted, deduplicated, with every non-`^[a-z_]{1,40}$` element
/// (including a shape outside the regex) folded into one `unrecognised`.
/// The rules body's other fields (`parameters`, an unused `ruleset_id`)
/// never appear in the message.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_a_same_head_failure_names_the_rules_in_force() {
    let mut provider = MockProvider::start();
    let files = seed_files();
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(2)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree"}}).to_string())
        .expect(2)
        .create();
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/trees/ios-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "ios-tree", "tree": []}).to_string())
        .expect(2)
        .create();
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(502)
        .expect(1)
        .create();
    let rules = provider
        .mock("GET", "/repos/acme/widget/rules/branches/main")
        .with_status(200)
        .with_body(
            serde_json::json!([
                {"type": "pull_request", "parameters": {"secret": "wlkn-test-marker-rule-body"}},
                {"type": "required_signatures", "ruleset_id": 99},
                {"type": "Pull Request!"},
            ])
            .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(
        err.message,
        "GitHub's GraphQL API did not report the commit as successful; rules in force on \
         `main`: pull_request, required_signatures, unrecognised"
    );
    assert!(!err.message.contains("wlkn-test-marker-rule-body"));
    assert!(!err.message.contains("ruleset_id"));
    commit.assert();
    rules.assert();
}

/// Acceptance 11: when the rules read itself fails, the original
/// refusal is reported alone -- the diagnostic never masks or replaces
/// it.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_a_same_head_failure_with_a_failing_rules_read_reports_the_original_message_alone() {
    let mut provider = MockProvider::start();
    let files = seed_files();
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(2)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree"}}).to_string())
        .expect(2)
        .create();
    mock_tree(
        &mut provider,
        "root-tree",
        vec![tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    provider
        .mock("GET", "/repos/acme/widget/git/trees/ios-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "ios-tree", "tree": []}).to_string())
        .expect(2)
        .create();
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(502)
        .expect(1)
        .create();
    let rules = provider
        .mock("GET", "/repos/acme/widget/rules/branches/main")
        .with_status(404)
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(
        err.message,
        "GitHub's GraphQL API did not report the commit as successful"
    );
    commit.assert();
    rules.assert();
}

/// Acceptance 11: a refused first-file `PUT` (non-409) -- the repository
/// stays empty after it -- gets the same suffix. The persistent-`409`
/// case (`ensure_gives_up_after_max_unavailable_attempts_on_persistent_409s`,
/// above) deliberately keeps its unsuffixed message: SHARED VALUES names
/// only "a refused first-file `PUT` (non-409)".
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_a_non_409_first_file_put_failure_names_the_rules_in_force() {
    let mut provider = MockProvider::start();
    let files = three_files();
    let (ref_mock, repo_mock, branches_mock) = mock_empty_repository(&mut provider, 2);
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/.editorconfig")
        .with_status(422)
        .expect(1)
        .create();
    let commit = provider.mock("POST", "/graphql").expect(0).create();
    let rules = provider
        .mock("GET", "/repos/acme/widget/rules/branches/main")
        .with_status(200)
        .with_body(
            serde_json::json!([
                {"type": "pull_request"},
                {"type": "required_signatures"},
                {"type": "Pull Request!"},
            ])
            .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(
        err.message,
        "GitHub refused to create the first file of an empty repository; rules in force on \
         `main`: pull_request, required_signatures, unrecognised"
    );
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    commit.assert();
    rules.assert();
}

/// The branch moved under us: retried against the new head, then
/// succeeds. At most three attempts in all; this one needs two.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_retries_once_against_a_moved_head_then_succeeds() {
    let mut provider = MockProvider::start();
    let files = seed_files();

    // First ref read: head-1.
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree-1"}}).to_string())
        .create();
    mock_tree(
        &mut provider,
        "root-tree-1",
        vec![tree_entry("ios", "040000", "tree", "ios-tree-1")],
    );
    mock_tree(&mut provider, "ios-tree-1", vec![]);

    let first_commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(
            serde_json::json!({"variables": {"input": {"expectedHeadOid": "head-1"}}}),
        ))
        .with_status(502)
        .expect(1)
        .create();

    // Second ref read (the re-observe): head-2, still nothing landed.
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-2"}}).to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-2")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree-2"}}).to_string())
        .create();
    mock_tree(
        &mut provider,
        "root-tree-2",
        vec![tree_entry("ios", "040000", "tree", "ios-tree-2")],
    );
    mock_tree(&mut provider, "ios-tree-2", vec![]);

    let second_commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(
            serde_json::json!({"variables": {"input": {"expectedHeadOid": "head-2"}}}),
        ))
        .with_status(200)
        .with_body(
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string(),
        )
        .expect(1)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&scaffold_inputs(files), &token).unwrap();
    assert!(ensured.changed);
    first_commit.assert();
    second_commit.assert();
}

/// A head that keeps moving on every re-read, and every commit attempt
/// keeps failing: exactly three attempts in total, then the original
/// failure, never a fourth.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_gives_up_after_three_attempts_on_a_persistently_moving_head() {
    let mut provider = MockProvider::start();
    let files = seed_files();

    for (index, head) in ["head-1", "head-2", "head-3"].iter().enumerate() {
        let tree_sha = format!("root-tree-{index}");
        let ios_tree_sha = format!("ios-tree-{index}");
        provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(200)
            .with_body(serde_json::json!({"object": {"sha": head}}).to_string())
            .expect(1)
            .create();
        provider
            .mock("GET", &format!("/repos/acme/widget/git/commits/{head}"))
            .with_status(200)
            .with_body(serde_json::json!({"tree": {"sha": tree_sha}}).to_string())
            .create();
        mock_tree(
            &mut provider,
            &tree_sha,
            vec![tree_entry("ios", "040000", "tree", &ios_tree_sha)],
        );
        mock_tree(&mut provider, &ios_tree_sha, vec![]);
    }
    // One more ref read: the final re-observe after the third failed
    // commit, landing on yet another new head (so the loop's own
    // "give up" bound, not a same-head coincidence, is what stops it).
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-4"}}).to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-4")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree-4"}}).to_string())
        .create();
    mock_tree(
        &mut provider,
        "root-tree-4",
        vec![tree_entry("ios", "040000", "tree", "ios-tree-4")],
    );
    mock_tree(&mut provider, "ios-tree-4", vec![]);

    let commit = provider
        .mock("POST", "/graphql")
        .with_status(502)
        .expect(3)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    commit.assert();
}

/// Milestone 3l, 2026-10-06 plan addendum (task S3): decisions (a)/(b)
/// are silent on what happens when `ensure`'s own commit-retry loop takes
/// a `createCommitOnBranch` failure, re-observes to decide what to do
/// next, and that re-observe reads `ScaffoldState::Empty` -- the branch's
/// head it just compared against has vanished, and the repository has
/// reverted to having no branches at all, between the first observe and
/// this re-read. The addendum decided this the same way the loop already
/// treats an unmoved head: report the original commit failure, never
/// re-enter task S3's own initialisation a second time from inside this
/// loop. Flagged there for X1's attack pass; this is that test. Also the
/// strongest form of "a second root commit on resume": a mutant that
/// re-entered `initialize_empty_repository` here would issue a `PUT` and
/// a second `createCommitOnBranch`, both pinned `.expect(0)` below, and
/// the rules diagnostic (task S4) is pinned `.expect(0)` too -- this path
/// returns the original error directly, never through
/// [`with_rule_suffix`]'s own call to `branch_rule_types`, unlike the
/// unmoved-head case right above.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_the_original_commit_failure_when_a_post_failure_reobserve_finds_the_repository_newly_empty()
 {
    let mut provider = MockProvider::start();
    let files =
        vec![RepoFile::new(RepoPath::parse("BUILD.bazel").unwrap(), "# reserve\n").unwrap()];

    // First observe: an ordinary non-empty repository with a head.
    let ref_mock_1 = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree"}}).to_string())
        .create();
    mock_tree(&mut provider, "root-tree", vec![]);

    let commit = provider
        .mock("POST", "/graphql")
        .match_body(partial_json_body(
            serde_json::json!({"variables": {"input": {"expectedHeadOid": "head-1"}}}),
        ))
        .with_status(502)
        .expect(1)
        .create();

    // The re-observe after that failure: the branch itself is gone, and
    // the repository now reads as empty -- `ScaffoldState::Empty`, not
    // `ScaffoldState::Absent` with a moved or unmoved head.
    let ref_mock_2 = provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(404)
        .expect(1)
        .create();
    let repo_mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({"visibility": "private", "topics": [], "default_branch": "main"})
                .to_string(),
        )
        .expect(1)
        .create();
    let branches_mock = provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .with_status(200)
        .with_body("[]")
        .expect(1)
        .create();

    // Never reached: `ensure` must not re-enter task S3's initialisation
    // from inside the commit-retry loop.
    let put_mock = provider
        .mock("PUT", "/repos/acme/widget/contents/BUILD.bazel")
        .with_status(201)
        .with_body("{}")
        .expect(0)
        .create();
    let rules_mock = provider
        .mock("GET", "/repos/acme/widget/rules/branches/main")
        .with_status(200)
        .with_body("[]")
        .expect(0)
        .create();

    let tool = GitHubScaffoldEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&scaffold_inputs(files), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(
        err.message,
        "GitHub's GraphQL API did not report the commit as successful"
    );
    ref_mock_1.assert();
    commit.assert();
    ref_mock_2.assert();
    repo_mock.assert();
    branches_mock.assert();
    put_mock.assert();
    rules_mock.assert();
}

/// The spec's `token` port is optional, exactly like the other GitHub
/// tools -- required is `false`, so `require_present` never demands it.
#[test]
fn spec_carries_token_as_an_optional_port() {
    let tool = GitHubScaffoldEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let token_port = tool
        .spec()
        .inputs
        .get(&PortName::parse("token").unwrap())
        .expect("a `token` port is declared");
    assert!(!token_port.required, "`token` must be optional");
}

#[test]
fn spec_validates_against_the_registry() {
    GitHubScaffoldEnsure::new(client_against("http://127.0.0.1:1".to_string()))
        .spec()
        .validate(willikins_types::registry())
        .unwrap();
}
