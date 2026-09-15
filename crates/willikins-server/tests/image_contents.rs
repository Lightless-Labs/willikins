//! Task 12 verification: what the container image holds, read from the
//! repository's own `Dockerfile` and `.dockerignore` rather than from a
//! built image (this host must never build one -- see the milestone 2
//! plan's deployment addendum; Railway's builder is the build proof).
//!
//! The three claims pinned here:
//!
//! 1. Every `COPY` source in the file is one of a named, small set, so a
//!    new one cannot appear without this test being updated on purpose.
//! 2. The trusted workflow directory inside the image holds exactly the
//!    two positive documents. Nothing under `workflows/fixtures/` can
//!    reach it, by the `COPY` glob and by `.dockerignore` independently.
//!    `Butler::start` scans that directory flat and refuses to start on
//!    the first negative fixture it reads, so a third document there is
//!    a dead deployment.
//! 3. The image's own command is `serve --http` and nothing else: no
//!    `--fake`, no bind address, no principal. Only the environment may
//!    make a deployment fake.
//!
//! What this test cannot pin: Railway's own start command overrides the
//! image's `ENTRYPOINT` in exec form (Railway's start-command
//! documentation, fetched 2026-09-15:
//! <https://docs.railway.com/deployments/start-command>), so an operator
//! with dashboard access can run any argument vector they like. That is
//! the same trust level as setting a variable, not a gap this file could
//! close. The live service's `startCommand` is null.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn read(name: &str) -> String {
    let path = repo_root().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every instruction in the Dockerfile, with comments dropped, blank
/// lines dropped, and backslash continuations joined into one line --
/// so a `COPY` a later edit spreads over two lines is still seen whole.
fn instructions(dockerfile: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pending = String::new();
    for raw in dockerfile.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head.trim_end());
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        out.push(pending.split_whitespace().collect::<Vec<_>>().join(" "));
        pending = String::new();
    }
    assert!(pending.is_empty(), "Dockerfile ends in a continuation");
    out
}

fn starting_with<'a>(instructions: &'a [String], keyword: &str) -> Vec<&'a str> {
    instructions
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with(keyword))
        .collect()
}

/// Every `COPY` in the whole file, in order. A source that is not listed
/// here reaches the build context or the image without anyone having
/// decided it should.
#[test]
fn every_copy_instruction_is_one_of_the_five_this_image_declares() {
    let dockerfile = read("Dockerfile");
    let all = instructions(&dockerfile);
    let copies = starting_with(&all, "COPY ");
    assert_eq!(
        copies,
        vec![
            // planner: the manifests `cargo chef prepare` reads.
            "COPY . .",
            // builder: the recipe, then the sources.
            "COPY --from=planner /app/recipe.json recipe.json",
            "COPY . .",
            // runtime: the one binary, and the two workflow documents.
            "COPY --from=builder /app/target/release/willikins-server /usr/local/bin/willikins-server",
            "COPY workflows/*.yaml /app/workflows/",
        ],
        "the Dockerfile's COPY set changed; update this test on purpose"
    );
    assert!(
        starting_with(&all, "ADD ").is_empty(),
        "ADD can fetch a remote URL into the image; this Dockerfile uses COPY only"
    );
}

/// The runtime stage -- everything after the last `FROM` -- copies the
/// binary and the workflow documents and nothing else. In particular it
/// never carries `COPY . .` forward, which would put the whole
/// repository (`workflows/fixtures/` included) into the shipped image.
#[test]
fn the_runtime_stage_copies_only_the_binary_and_the_workflow_documents() {
    let dockerfile = read("Dockerfile");
    let all = instructions(&dockerfile);
    let last_from = all
        .iter()
        .rposition(|line| line.starts_with("FROM "))
        .expect("the Dockerfile has a FROM");
    assert_eq!(
        all[last_from], "FROM gcr.io/distroless/cc-debian12",
        "the runtime base image changed; update this test on purpose"
    );
    let runtime = &all[last_from..];
    let copies = starting_with(runtime, "COPY ");
    assert_eq!(copies.len(), 2, "runtime stage COPY lines: {copies:?}");
    assert!(
        !copies.contains(&"COPY . ."),
        "the runtime stage must never copy the whole context"
    );
}

/// The image's command carries no flag at all beyond `serve --http`.
/// `--fake` in particular is never baked in: `WILLIKINS_FAKE_CATALOG` is
/// the only thing that may make a deployment serve the fake catalog.
#[test]
fn the_image_command_is_serve_http_and_names_no_other_flag() {
    let dockerfile = read("Dockerfile");
    let all = instructions(&dockerfile);
    assert_eq!(
        starting_with(&all, "ENTRYPOINT "),
        vec![r#"ENTRYPOINT ["/usr/local/bin/willikins-server"]"#]
    );
    assert_eq!(
        starting_with(&all, "CMD "),
        vec![r#"CMD ["serve", "--http"]"#]
    );
    let cmd = starting_with(&all, "CMD ").join(" ");
    for flag in ["--fake", "--bind", "--principal", "--stdio"] {
        assert!(!cmd.contains(flag), "the image's CMD names {flag}: {cmd}");
    }
    // The workflow directory the image sets must be the directory the
    // runtime stage actually copies the documents into.
    assert!(
        all.contains(&"ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows".to_string()),
        "instructions: {all:?}"
    );
}

/// The image carries no credential of its own. It sets exactly one
/// environment variable, and declares no build argument -- a token baked
/// into an `ENV` would sit in the image's own metadata for anyone who
/// can pull it, and one passed as an `ARG` would sit in the build log.
/// Every credential and every token hash reaches the server from the
/// deployment's own variables instead.
#[test]
fn the_image_bakes_in_no_credential() {
    let dockerfile = read("Dockerfile");
    let all = instructions(&dockerfile);
    assert_eq!(
        starting_with(&all, "ENV "),
        vec!["ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows"],
        "the image's ENV set changed; update this test on purpose"
    );
    assert!(
        starting_with(&all, "ARG ").is_empty(),
        "this image takes no build argument"
    );
    for fragment in [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "ghp_",
        "dp.sa.",
        "github_pat_",
    ] {
        assert!(
            !dockerfile.contains(fragment),
            "the Dockerfile names {fragment}"
        );
    }
}

/// Every `.dockerignore` pattern, with comments and blank lines dropped.
fn dockerignore_patterns() -> Vec<String> {
    read(".dockerignore")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Whether `relative` (a repository-relative path with `/` separators)
/// is excluded from the build context by `patterns`.
///
/// This understands only the two pattern shapes `.dockerignore` actually
/// uses -- a plain path (matching that path and everything under it) and
/// a `*.<extension>` suffix on a basename. A test below asserts no other
/// shape is present, so this stays an honest reading of the real file
/// rather than a partial reimplementation of Docker's matcher that could
/// silently disagree with it.
fn is_ignored(patterns: &[String], relative: &str) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(extension) = pattern.strip_prefix("*.") {
            let basename = relative.rsplit('/').next().unwrap_or(relative);
            return basename.ends_with(&format!(".{extension}"));
        }
        relative == pattern || relative.starts_with(&format!("{pattern}/"))
    })
}

#[test]
fn every_dockerignore_pattern_has_one_of_the_two_shapes_this_test_reads() {
    for pattern in dockerignore_patterns() {
        let plain = !pattern.contains('*') && !pattern.contains('!') && !pattern.contains('?');
        let suffix = pattern.starts_with("*.") && !pattern[2..].contains('*');
        assert!(
            plain || suffix,
            "{pattern:?} is a shape `is_ignored` does not read; \
             teach it the shape or drop the pattern"
        );
    }
}

/// Every path under `dir`, repository-relative, with `/` separators.
fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("readable directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        if entry.file_type().expect("file type").is_dir() {
            walk(&entry.path(), &format!("{relative}/"), out);
        } else {
            out.push(relative);
        }
    }
}

/// The set of files `COPY workflows/*.yaml /app/workflows/` puts in the
/// image, computed from the repository tree and the `.dockerignore`: the
/// glob is not recursive, so it sees only `workflows/`'s own entries,
/// and `.dockerignore` removes the rest from the build context first.
#[test]
fn the_image_workflows_directory_holds_exactly_the_two_positive_documents() {
    let patterns = dockerignore_patterns();
    let mut all = Vec::new();
    walk(&repo_root().join("workflows"), "workflows/", &mut all);

    let admitted: BTreeSet<String> = all
        .iter()
        .filter(|relative| !is_ignored(&patterns, relative))
        // `workflows/*.yaml` matches one path segment below `workflows/`
        // and nothing deeper.
        .filter(|relative| {
            relative.strip_prefix("workflows/").is_some_and(|rest| {
                !rest.contains('/')
                    && std::path::Path::new(rest)
                        .extension()
                        .is_some_and(|e| e == "yaml")
            })
        })
        .map(|relative| relative.trim_start_matches("workflows/").to_string())
        .collect();

    let expected: BTreeSet<String> = ["new-rust-service.yaml", "rotate-service-token.yaml"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        admitted, expected,
        "/app/workflows must hold exactly the two positive documents"
    );
}

/// Nothing under `workflows/fixtures/` can reach the image, for two
/// independent reasons: `.dockerignore` keeps the directory out of the
/// build context, and the `COPY` glob matches one level only. Either one
/// alone is enough; this asserts both, so removing one does not silently
/// leave the image one edit away from a negative fixture that stops
/// `Butler::start`.
#[test]
fn no_fixture_document_can_reach_the_image() {
    let patterns = dockerignore_patterns();
    assert!(
        patterns.iter().any(|p| p == "workflows/fixtures"),
        ".dockerignore no longer excludes workflows/fixtures: {patterns:?}"
    );

    let mut fixtures = Vec::new();
    walk(
        &repo_root().join("workflows").join("fixtures"),
        "workflows/fixtures/",
        &mut fixtures,
    );
    assert!(!fixtures.is_empty(), "expected fixture documents to exist");
    for relative in &fixtures {
        assert!(
            is_ignored(&patterns, relative),
            "{relative} is still in the build context"
        );
        let rest = relative.strip_prefix("workflows/").expect("prefix");
        assert!(
            rest.contains('/'),
            "{relative} sits directly in workflows/ and the COPY glob would take it"
        );
    }
}
