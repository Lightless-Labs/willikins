//! `github.scaffold.ensure`'s fake tool agrees with the live one
//! *behaviourally*, not only on their `ToolSpec`s (`catalog_parity.rs`
//! pins those equal). Milestone 3g, task G2. Mirrors
//! `fake_agrees_with_live.rs`'s own shape for `github.repo.get`: seed the
//! fake's state and serve the live tool a mock body standing for the
//! same real world, then assert both tools answer the same way.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::{GitHubRepoRecord, repo_key};
use willikins_providers_fake::tools::GitHubScaffoldEnsure as FakeGitHubScaffoldEnsure;
use willikins_providers_github::{GitHubClient, GitHubScaffoldEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    CommitHeadline, DomainType, GitBranchName, GitHubRepo, RepoFile, RepoPath, RepoVisibility,
};

/// A test mints its own token; `SinkToken::new` is disallowed elsewhere.
#[allow(clippy::disallowed_methods)]
fn mint() -> SinkToken {
    SinkToken::new()
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

fn live_against(url: String) -> GitHubScaffoldEnsure {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    GitHubScaffoldEnsure::new(Arc::new(GitHubClient::new(Http::new(
        url,
        Vec::new(),
        credential,
    ))))
}

fn shape(observation: &Observation) -> &'static str {
    match observation {
        Observation::Present(_) => "present",
        Observation::Absent { .. } => "absent",
        Observation::Foreign => "foreign",
        Observation::Mismatch { .. } => "mismatch",
    }
}

fn tree_entry(name: &str, mode: &str, kind: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({
        "path": name, "mode": mode, "type": kind, "sha": sha, "size": 1,
        "url": "https://api.github.com/x",
    })
}

/// Milestone 3l, task F1, acceptance 13: `git/ref/heads/{branch}` fails
/// `404` -- decision (b)'s read table's own entry point for every row
/// this test file adds beyond `agrees_on_absent`'s (present-and-missing
/// marker) shape.
fn mock_branch_404(provider: &mut MockProvider) {
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
}

fn mock_repo_404(provider: &mut MockProvider) {
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
}

fn mock_repo_200(provider: &mut MockProvider, default_branch: &str) {
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({"visibility": "private", "default_branch": default_branch})
                .to_string(),
        )
        .create();
}

fn mock_branches(provider: &mut MockProvider, names: &[&str]) {
    let entries: Vec<serde_json::Value> = names
        .iter()
        .map(|name| serde_json::json!({"name": name}))
        .collect();
    provider
        .mock("GET", "/repos/acme/widget/branches?per_page=1")
        .with_status(200)
        .with_body(serde_json::json!(entries).to_string())
        .create();
}

/// Milestone 3l, task F1, acceptance 13: no repository at all (`404` from
/// `GET /repos`). Both sides report `Absent` on `read`, and `NotFound` on
/// `ensure`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_repository_absent() {
    let mut provider = MockProvider::start();
    mock_branch_404(&mut provider);
    mock_repo_404(&mut provider);
    let live_tool = live_against(provider.url());
    let live_read = live_tool.read(&inputs(seed_files())).expect("live reads");
    let token = mint();
    let live_ensure = live_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    let fake_tool = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())));
    let fake_read = fake_tool.read(&inputs(seed_files())).expect("fake reads");
    let fake_ensure = fake_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    assert_eq!(shape(&live_read), shape(&fake_read));
    assert!(matches!(live_read, Observation::Absent { .. }));
    assert_eq!(
        live_ensure.kind,
        ToolErrorKind::NotFound,
        "{}",
        live_ensure.message
    );
    assert_eq!(
        fake_ensure.kind,
        ToolErrorKind::NotFound,
        "{}",
        fake_ensure.message
    );
    // Independent adversarial pass: the fake re-types every SHARED VALUES
    // message by hand, so equal kinds alone let its wording drift.
    assert_eq!(live_ensure.message, fake_ensure.message);
}

/// An empty repository (`GET .../branches?per_page=1` answers `[]`)
/// whose default branch equals the requested `branch`: both sides report
/// `Absent` on `read`.
#[test]
fn agrees_on_an_empty_repository_with_the_matching_default_branch() {
    let mut provider = MockProvider::start();
    mock_branch_404(&mut provider);
    mock_repo_200(&mut provider, "main");
    mock_branches(&mut provider, &[]);
    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .expect("live reads");

    let state = FakeState::new().with_empty_repo(&repo(), RepoVisibility::Private, None);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .expect("fake reads");

    assert_eq!(shape(&live), shape(&fake));
    assert!(matches!(live, Observation::Absent { .. }));
}

/// An empty repository whose default branch differs from the requested
/// `branch`: both sides refuse `Conflict`, on `read` and `ensure` alike.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_an_empty_repository_with_a_mismatched_default_branch() {
    let mut provider = MockProvider::start();
    mock_branch_404(&mut provider);
    mock_repo_200(&mut provider, "trunk");
    mock_branches(&mut provider, &[]);
    let live_tool = live_against(provider.url());
    let live_read = live_tool.read(&inputs(seed_files())).unwrap_err();
    let token = mint();
    let live_ensure = live_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    let trunk = GitBranchName::parse("trunk").unwrap();
    let state = FakeState::new().with_empty_repo(&repo(), RepoVisibility::Private, Some(&trunk));
    let fake_tool = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)));
    let fake_read = fake_tool.read(&inputs(seed_files())).unwrap_err();
    let fake_ensure = fake_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    assert_eq!(
        live_read.kind,
        ToolErrorKind::Conflict,
        "{}",
        live_read.message
    );
    assert_eq!(
        fake_read.kind,
        ToolErrorKind::Conflict,
        "{}",
        fake_read.message
    );
    assert_eq!(
        live_ensure.kind,
        ToolErrorKind::Conflict,
        "{}",
        live_ensure.message
    );
    assert_eq!(
        fake_ensure.kind,
        ToolErrorKind::Conflict,
        "{}",
        fake_ensure.message
    );
    // Independent adversarial pass: messages, not only kinds.
    assert_eq!(live_read.message, fake_read.message);
    assert_eq!(live_ensure.message, fake_ensure.message);
}

/// A non-empty repository (`branches` reports a different branch) still
/// missing the requested `branch`: both sides keep the unchanged
/// missing-branch `NotFound`, on `read` and `ensure` alike.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_a_non_empty_repository_missing_the_branch() {
    let mut provider = MockProvider::start();
    mock_branch_404(&mut provider);
    mock_repo_200(&mut provider, "main");
    mock_branches(&mut provider, &["other"]);
    let live_tool = live_against(provider.url());
    let live_read = live_tool.read(&inputs(seed_files())).unwrap_err();
    let token = mint();
    let live_ensure = live_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    let mut fake_state = FakeState::new();
    fake_state.github_repos.insert(
        repo_key(&repo()),
        GitHubRepoRecord {
            visibility: RepoVisibility::Private,
            ours: true,
            archived: false,
            branches: Some(vec![GitBranchName::parse("other").unwrap()]),
            default_branch: None,
        },
    );
    let fake_tool = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(fake_state)));
    let fake_read = fake_tool.read(&inputs(seed_files())).unwrap_err();
    let fake_ensure = fake_tool.ensure(&inputs(seed_files()), &token).unwrap_err();

    assert_eq!(
        live_read.kind,
        ToolErrorKind::NotFound,
        "{}",
        live_read.message
    );
    assert_eq!(
        fake_read.kind,
        ToolErrorKind::NotFound,
        "{}",
        fake_read.message
    );
    assert_eq!(
        live_ensure.kind,
        ToolErrorKind::NotFound,
        "{}",
        live_ensure.message
    );
    assert_eq!(
        fake_ensure.kind,
        ToolErrorKind::NotFound,
        "{}",
        fake_ensure.message
    );
    // Independent adversarial pass: messages, not only kinds.
    assert_eq!(live_read.message, fake_read.message);
    assert_eq!(live_ensure.message, fake_ensure.message);
}

fn mock_ref_and_commit(provider: &mut MockProvider) {
    provider
        .mock("GET", "/repos/acme/widget/git/ref/heads/main")
        .with_status(200)
        .with_body(serde_json::json!({"object": {"sha": "head-1"}}).to_string())
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/commits/head-1")
        .with_status(200)
        .with_body(serde_json::json!({"tree": {"sha": "root-tree"}}).to_string())
        .create();
}

/// Both sides report `Present` for a scaffold whose marker is already
/// there, whatever the seed files' own state.
#[test]
fn agrees_on_present() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(
            serde_json::json!({"sha": "root-tree", "tree": [
                tree_entry(".willikins-scaffold", "100644", "blob", "marker-sha"),
            ]})
            .to_string(),
        )
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

    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .expect("live tool reads");

    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(
            &repo(),
            &branch(),
            &[(".willikins-scaffold", "managed-by: willikins\n")],
        );
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .expect("fake tool reads");

    assert_eq!(shape(&live), shape(&fake));
}

/// Both sides report `Absent` when nothing has landed.
#[test]
fn agrees_on_absent() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "root-tree", "tree": []}).to_string())
        .create();

    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .expect("live tool reads");

    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .read(&inputs(seed_files()))
        .expect("fake tool reads");

    assert_eq!(shape(&live), shape(&fake));
}

/// Both sides report `Foreign` when the marker's first line is not
/// willikins' own.
#[test]
fn agrees_on_foreign() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(
            serde_json::json!({"sha": "root-tree", "tree": [
                tree_entry(".willikins-scaffold", "100644", "blob", "marker-sha"),
            ]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({"content": STANDARD.encode("not ours\n"), "encoding": "base64"})
                .to_string(),
        )
        .create();

    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .expect("live tool reads");

    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[(".willikins-scaffold", "not ours\n")]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .expect("fake tool reads");

    assert_eq!(shape(&live), shape(&fake));
    assert!(matches!(live, Observation::Foreign));
}

/// Both sides report `Foreign` for a marker whose first line merely
/// starts with the header (adversarial pass, render and write).
#[test]
fn agrees_on_a_marker_whose_first_line_only_starts_with_the_header() {
    let impostor = "managed-by: willikins-impostor\n";
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(
            serde_json::json!({"sha": "root-tree", "tree": [
                tree_entry(".willikins-scaffold", "100644", "blob", "marker-sha"),
            ]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
        .with_status(200)
        .with_body(
            serde_json::json!({"content": STANDARD.encode(impostor), "encoding": "base64"})
                .to_string(),
        )
        .create();

    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .expect("live tool reads");
    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[(".willikins-scaffold", impostor)]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .expect("fake tool reads");

    assert!(matches!(live, Observation::Foreign), "{live:?}");
    assert!(matches!(fake, Observation::Foreign), "{fake:?}");
}

fn serve_root_tree(provider: &mut MockProvider, entries: &[serde_json::Value]) {
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "root-tree", "tree": entries}).to_string())
        .create();
}

/// A seed path beneath an existing file (`ios` is a file; the seed is
/// `ios/BUILD.bazel`): both sides refuse with `Conflict`. Adversarial pass
/// (render and write) -- before it, the live tool read `Absent` and the
/// fake, a flat map with no directories, agreed only by accident.
#[test]
fn agrees_on_a_seed_path_beneath_an_existing_file() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    serve_root_tree(
        &mut provider,
        &[tree_entry("ios", "100644", "blob", "ios-is-a-file-sha")],
    );
    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .unwrap_err();

    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[("ios", "a file\n")]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .unwrap_err();

    assert_eq!(live.kind, ToolErrorKind::Conflict, "{}", live.message);
    assert_eq!(fake.kind, ToolErrorKind::Conflict, "{}", fake.message);
    assert!(fake.message.contains("ios/BUILD.bazel"), "{}", fake.message);
}

/// A seed path that is an existing *directory* (`ios` holds files; the
/// seed is a file named `ios`): both sides refuse with `Conflict`. Before
/// the adversarial pass the live tool refused (a tree is a non-blob) but
/// the fake read `Absent` and planned `Create`.
#[test]
fn agrees_on_a_seed_path_that_is_an_existing_directory() {
    let files = vec![RepoFile::new(RepoPath::parse("ios").unwrap(), "a file now\n").unwrap()];
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    serve_root_tree(
        &mut provider,
        &[tree_entry("ios", "040000", "tree", "ios-tree")],
    );
    let live = live_against(provider.url())
        .read(&inputs(files.clone()))
        .unwrap_err();

    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[("ios/BUILD.bazel", "someone's\n")]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(files))
        .unwrap_err();

    assert_eq!(live.kind, ToolErrorKind::Conflict, "{}", live.message);
    assert_eq!(fake.kind, ToolErrorKind::Conflict, "{}", fake.message);
}

/// The marker beneath an existing file, or the marker path an existing
/// directory: `Foreign` on both sides.
#[test]
fn agrees_on_a_marker_path_occupied_by_a_file_or_a_directory() {
    // Marker `.willikins-scaffold/inner` beneath a file `.willikins-scaffold`.
    let mut under_file = inputs(seed_files());
    under_file.insert(
        PortName::parse("marker").unwrap(),
        Value::known(RepoPath::parse(".willikins-scaffold/inner").unwrap()),
    );
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    serve_root_tree(
        &mut provider,
        &[tree_entry(".willikins-scaffold", "100644", "blob", "a-sha")],
    );
    let live = live_against(provider.url()).read(&under_file).unwrap();
    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[(".willikins-scaffold", "a file\n")]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&under_file)
        .unwrap();
    assert!(matches!(live, Observation::Foreign), "{live:?}");
    assert!(matches!(fake, Observation::Foreign), "{fake:?}");

    // Marker `.willikins-scaffold` that is itself a directory.
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    serve_root_tree(
        &mut provider,
        &[tree_entry(
            ".willikins-scaffold",
            "040000",
            "tree",
            "dir-sha",
        )],
    );
    let live = live_against(provider.url())
        .read(&inputs(seed_files()))
        .unwrap();
    let state = FakeState::new()
        .with_repo(&repo(), RepoVisibility::Private, true)
        .with_scaffold_files(&repo(), &branch(), &[(".willikins-scaffold/x", "inside\n")]);
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(state)))
        .read(&inputs(seed_files()))
        .unwrap();
    assert!(matches!(live, Observation::Foreign), "{live:?}");
    assert!(matches!(fake, Observation::Foreign), "{fake:?}");
}

/// Both sides refuse the same shape problems before touching any state
/// or request at all.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_invalid_shape() {
    let token = mint();
    let live = live_against("http://127.0.0.1:1".to_string())
        .ensure(&inputs(Vec::new()), &token)
        .unwrap_err();
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .ensure(&inputs(Vec::new()), &token)
        .unwrap_err();
    assert_eq!(live.kind, ToolErrorKind::Invalid);
    assert_eq!(fake.kind, ToolErrorKind::Invalid);
}

/// Nested declared paths (`a` and `a/b`) are `Invalid` on both sides,
/// before any request or state lookup (adversarial pass, render and
/// write): before it, the live tool sent the commit and the fake wrote
/// both paths into its map.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_declared_paths_nested_under_one_another() {
    let files = vec![
        RepoFile::new(RepoPath::parse("a").unwrap(), "x").unwrap(),
        RepoFile::new(RepoPath::parse("a/b").unwrap(), "y").unwrap(),
    ];
    let token = mint();
    let live = live_against("http://127.0.0.1:1".to_string())
        .ensure(&inputs(files.clone()), &token)
        .unwrap_err();
    let fake = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .ensure(&inputs(files), &token)
        .unwrap_err();
    assert_eq!(live.kind, ToolErrorKind::Invalid, "{}", live.message);
    assert_eq!(fake.kind, ToolErrorKind::Invalid, "{}", fake.message);
}

/// The marker-too-long shape refusal (64 files each with a long path,
/// producing a marker over `RepoFile`'s bound) agrees between both sides
/// too, on both `read` and `ensure` -- the fix commit that added this
/// check to the live tool must add it to the fake as well, or a document
/// would plan `Create` against the fake catalog and fail `Invalid` only
/// once it reached the live one.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn agrees_on_an_over_long_marker() {
    let long_path_files: Vec<RepoFile> = (0..64)
        .map(|i| {
            let path = format!("{}{i}.txt", "a".repeat(990));
            RepoFile::new(RepoPath::parse(&path).unwrap(), "x").unwrap()
        })
        .collect();
    let token = mint();

    let live_read = live_against("http://127.0.0.1:1".to_string())
        .read(&inputs(long_path_files.clone()))
        .unwrap_err();
    let fake_read = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .read(&inputs(long_path_files.clone()))
        .unwrap_err();
    assert_eq!(live_read.kind, ToolErrorKind::Invalid);
    assert_eq!(fake_read.kind, ToolErrorKind::Invalid);

    let live_ensure = live_against("http://127.0.0.1:1".to_string())
        .ensure(&inputs(long_path_files.clone()), &token)
        .unwrap_err();
    let fake_ensure = FakeGitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .ensure(&inputs(long_path_files), &token)
        .unwrap_err();
    assert_eq!(live_ensure.kind, ToolErrorKind::Invalid);
    assert_eq!(fake_ensure.kind, ToolErrorKind::Invalid);
}

/// The marker this crate's live tool sends is byte-identical to what the
/// fake tool stores for the same files -- both compute the SHARED VALUES
/// format (header, then `<sha> <path>` sorted by path) the same way, so a
/// document cannot tell them apart from the marker's own content.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn the_written_marker_is_byte_identical_between_fake_and_live() {
    let mut provider = MockProvider::start();
    mock_ref_and_commit(&mut provider);
    provider
        .mock("GET", "/repos/acme/widget/git/trees/root-tree")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(serde_json::json!({"sha": "root-tree", "tree": []}).to_string())
        .create();
    let commit = provider
        .mock("POST", "/graphql")
        .with_status(200)
        .with_body_from_request(|request| {
            // Capture the request body itself so this test can extract
            // the marker's base64 content below, rather than pinning the
            // whole body with a second copy of the mutation's shape.
            let mut captured = CAPTURED_BODY.lock().unwrap();
            *captured = Some(request.body().ok().cloned().unwrap_or_default());
            serde_json::json!({"data": {"createCommitOnBranch": {"commit": {"oid": "new-sha"}}}})
                .to_string()
                .into_bytes()
        })
        .create();

    let token = mint();
    live_against(provider.url())
        .ensure(&inputs(seed_files()), &token)
        .expect("live tool commits");
    commit.assert();

    let body_bytes = CAPTURED_BODY
        .lock()
        .unwrap()
        .take()
        .expect("the graphql request was captured");
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let additions = body["variables"]["input"]["fileChanges"]["additions"]
        .as_array()
        .expect("additions is an array");
    let marker_entry = additions
        .iter()
        .find(|entry| entry["path"] == ".willikins-scaffold")
        .expect("the marker is among the additions");
    let live_marker_base64 = marker_entry["contents"].as_str().unwrap();
    let live_marker_bytes = STANDARD.decode(live_marker_base64).unwrap();

    let state = Arc::new(Mutex::new(FakeState::new().with_repo(
        &repo(),
        RepoVisibility::Private,
        true,
    )));
    FakeGitHubScaffoldEnsure::new(state.clone())
        .ensure(&inputs(seed_files()), &token)
        .expect("fake tool commits");
    let fake_marker = state
        .lock()
        .unwrap()
        .scaffolds
        .get(&format!("{}#{}", repo(), branch()))
        .expect("the fake recorded the scaffold")
        .get(".willikins-scaffold")
        .expect("the fake wrote a marker")
        .clone();

    assert_eq!(live_marker_bytes, fake_marker.into_bytes());

    // And both agree with a from-scratch computation of the same
    // algorithm, so this test is not merely pinning the two copies
    // against each other's possible shared bug.
    let mut expected = String::from("managed-by: willikins\n");
    let mut lines: Vec<(String, String)> = seed_files()
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_string(),
                blob_sha(file.content().as_bytes()),
            )
        })
        .collect();
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, sha) in lines {
        expected.push_str(&sha);
        expected.push(' ');
        expected.push_str(&path);
        expected.push('\n');
    }
    assert_eq!(String::from_utf8(live_marker_bytes).unwrap(), expected);
}

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

/// A process-wide slot the `with_body_from_request` closure above writes
/// into, since that closure borrows nothing from its call site.
static CAPTURED_BODY: Mutex<Option<Vec<u8>>> = Mutex::new(None);
