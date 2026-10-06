//! Acceptance test 8: `github.scaffold.ensure` against mocks, each row of
//! decision (b)/(c)'s table
//! (`docs/plans/2026-09-30-milestone-3g-file-writing.md`).

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{GitHubClient, GitHubScaffoldEnsure};
use willikins_providers_http::testing::{MockProvider, partial_json_body};
use willikins_providers_http::{Credential, Http};
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

/// The same row, through `ensure`: a temporary `NotFound` naming task S3,
/// which replaces it with the scaffold's own first write. No write.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_a_temporary_not_found_when_the_repository_is_empty() {
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
        .with_body("[]")
        .expect(1)
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
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert_eq!(
        err.message,
        "`acme/widget` is empty; this tool does not yet create a repository's first branch \
         (milestone 3l task S3 adds that)"
    );
    ref_mock.assert();
    repo_mock.assert();
    branches_mock.assert();
    commit.assert();
    first_file.assert();
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
