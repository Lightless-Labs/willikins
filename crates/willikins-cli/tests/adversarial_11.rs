//! Task 11's adversarial pass: attacks on what the CLI's `apply`,
//! `approve`, `reject`, `runs`, `run`, `serve` and `--live` added, and on
//! the read-only journal replay behind `runs`/`run`.
//!
//! The claims under attack, in the order the sections below take them:
//!
//! 1. **The CLI can only run what a `Butler` over a trusted copy of one
//!    document would run.** `apply <file>` copies the one named file into
//!    a private temporary directory under the document's *own* name, so
//!    the filename an operator typed can neither pick the workflow name
//!    nor escape the directory.
//! 2. **A recorded approval is honoured exactly once.** The same plan id,
//!    approved once and applied twice, runs once.
//! 3. **The journal lock is respected in both directions.** A writer
//!    (`apply`, `approve`) refuses plainly while another holder has the
//!    file; a reader (`runs`, `run`) never contends for it, and the file
//!    still validates afterwards.
//! 4. **Text and JSON never carry a secret**, in every position a `Value`
//!    can occupy.
//! 5. **The CLI's kinds and exit codes agree with the MCP surface.** One
//!    plan per refusal class, and the whole exit-code/stream table.
//!
//! CLI self-approval by the `--principal` operator is the plan's own
//! design for a local operator who already holds the machine and the
//! credentials (see the `willikins-cli` section of the milestone 2 plan);
//! it is pinned here, not flagged.
//!
//! Every test here uses the fake catalog and never sets a credential
//! variable: `run` clears the environment, so no `--live` test can reach
//! a network.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Deserializer as _;
use serde::de::{IgnoredAny, MapAccess, Visitor};

// ---------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

fn run(args: &[&str]) -> Output {
    run_with_env(args, &[])
}

fn run_with_env(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins"));
    command.args(args).env_clear();
    for (name, value) in vars {
        command.env(name, value);
    }
    command
        .output()
        .expect("failed to run the willikins binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process was not signalled")
}

fn json_documents(text: &str) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .map(|result| result.unwrap_or_else(|err| panic!("invalid JSON in {text:?}: {err}")))
        .collect()
}

/// Every top-level JSON object key in `json`, in encounter order,
/// **including duplicates** -- the same token-walking collector
/// `willikins-core`'s own `Reported` tests use, and for the same reason:
/// parsing into a [`serde_json::Value`] folds a duplicate key silently,
/// which would make a `message`/`message` collision indistinguishable
/// from the non-colliding case.
fn top_level_keys(json: &str) -> Vec<String> {
    struct KeyCollector(Vec<String>);

    impl<'de> Visitor<'de> for KeyCollector {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON object")
        }

        fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            while let Some(key) = map.next_key::<String>()? {
                self.0.push(key);
                map.next_value::<IgnoredAny>()?;
            }
            Ok(self.0)
        }
    }

    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer
        .deserialize_map(KeyCollector(Vec::new()))
        .unwrap_or_else(|error| panic!("not a JSON object: {error}; input was {json:?}"))
}

/// Assert that `json` is one object whose keys are all distinct -- the
/// invariant `willikins_core::Reported` documents and every kind-tagged
/// error in this workspace is supposed to keep: no variant may declare a
/// field named `message`, since `Reported` adds one of its own and
/// `serde(flatten)` writes both rather than refusing. A duplicate key is
/// not a cosmetic flaw: every JSON reader keeps one of the two silently,
/// and which one is not specified.
fn assert_no_duplicate_keys(json: &str, what: &str) {
    let keys = top_level_keys(json);
    let mut seen = std::collections::BTreeSet::new();
    for key in &keys {
        assert!(
            seen.insert(key.clone()),
            "{what}: duplicate top-level key `{key}` in {json}\nkeys: {keys:?}"
        );
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-cli-adversarial-11-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn positive_fixture_text() -> String {
    std::fs::read_to_string(workflow("workflows/new-rust-service.yaml")).unwrap()
}

/// The positive fixture's own `--input` pair, as `apply` takes them.
const POSITIVE_INPUTS: [&str; 4] = [
    "--input",
    "slug=third-thoughts",
    "--input",
    "org=lightless-labs",
];

// =====================================================================
// 1. the temporary-directory copy
// =====================================================================

/// The copy is named after the *document's* own `name:`, never the path
/// an operator typed, so a filename whose stem differs from the name --
/// here only in case, which `WorkflowName`'s grammar refuses outright --
/// still applies rather than tripping `StartupError::NameMismatch` or
/// `InvalidName`. This is what makes `apply <file>` able to run a
/// document out of a directory whose naming convention is not the
/// server's.
#[test]
fn a_filename_stem_that_is_not_the_documents_name_still_applies() {
    let dir = TempDir::new("odd-stem");
    // `New-Rust-Service` is not a `WorkflowName` at all (uppercase), and
    // `totally-different` is a valid one that simply is not the
    // document's. Both must be ignored in favour of the document's own
    // `name:`.
    for stem in ["New-Rust-Service", "totally-different"] {
        let path = dir.join(&format!("{stem}.yaml"));
        std::fs::write(&path, positive_fixture_text()).unwrap();
        let mut args = vec!["apply", path.to_str().unwrap()];
        args.extend_from_slice(&POSITIVE_INPUTS);
        let output = run(&args);
        assert_eq!(
            exit_code(&output),
            0,
            "stem {stem}: stderr {}",
            stderr(&output)
        );
        assert!(
            stdout(&output).contains("state: succeeded"),
            "stem {stem}: {}",
            stdout(&output)
        );
    }
}

/// A document whose own `name:` is not a [`willikins_types::WorkflowName`]
/// is refused by the DSL before anything is copied anywhere -- which is
/// also what keeps the copy's destination path (`<name>.yaml`) inside the
/// temporary directory: every name that reaches `format!` has already
/// been through the grammar that refuses `/`, `..`, and an empty string.
#[test]
fn a_document_whose_name_is_not_a_workflow_name_is_refused_before_any_copy() {
    let dir = TempDir::new("bad-name");
    for name in ["../escape", "New-Rust-Service", "a/b", ""] {
        let path = dir.join("doc.yaml");
        let text = positive_fixture_text().replace(
            "name: new-rust-service",
            &format!("name: {}", serde_json::to_string(name).unwrap()),
        );
        assert!(text.contains("name: \""), "replacement failed for {name}");
        std::fs::write(&path, &text).unwrap();
        let mut args = vec!["apply", path.to_str().unwrap()];
        args.extend_from_slice(&POSITIVE_INPUTS);
        let output = run(&args);
        assert_eq!(
            exit_code(&output),
            2,
            "name {name:?}: stdout {} stderr {}",
            stdout(&output),
            stderr(&output)
        );
        // A document refusal, on stderr, exactly as `validate`/`plan`
        // report one -- not an I/O failure from a copy that was attempted
        // anyway.
        assert!(
            !stderr(&output).contains("failed to copy"),
            "name {name:?}: a copy was attempted: {}",
            stderr(&output)
        );
    }
    // Nothing escaped into the parent of the (private) temporary
    // directory the copy would have used.
    assert!(!workspace_root().join("escape.yaml").exists());
}

/// `apply <file>` given a path that is itself a symlink follows it. The
/// server's symlink refusal guards a *trusted directory* an operator
/// vetted as a whole, where a link could smuggle in content from outside
/// it; `apply <file>` has no such directory -- the operator named this
/// exact path, and the bytes it resolves to are copied into a private
/// directory and validated by `Butler::start` there like any other
/// document. Pinned as accepted behaviour so a later change to it is a
/// deliberate one.
#[cfg(unix)]
#[test]
fn a_symlinked_file_argument_is_followed_and_its_target_validated() {
    let dir = TempDir::new("symlink");
    let link = dir.join("link.yaml");
    std::os::unix::fs::symlink(workflow("workflows/new-rust-service.yaml"), &link).unwrap();
    let mut args = vec!["apply", link.to_str().unwrap()];
    args.extend_from_slice(&POSITIVE_INPUTS);
    let output = run(&args);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("state: succeeded"),
        "{}",
        stdout(&output)
    );

    // A symlink whose target is not a document is refused as a document,
    // not followed into something else: the same exit 2 any unparseable
    // file gets.
    let broken = dir.join("broken.yaml");
    std::os::unix::fs::symlink(dir.join("does-not-exist.yaml"), &broken).unwrap();
    let output = run(&["apply", broken.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2, "stdout: {}", stdout(&output));
}

/// A file larger than the DSL's byte cap is refused by the cap, at
/// `load_document`'s own `stat`, before `apply` copies anything.
#[test]
fn a_file_over_the_dsl_byte_cap_is_refused_by_the_cap() {
    let dir = TempDir::new("too-large");
    let path = dir.join("huge.yaml");
    let text = format!(
        "{}\n# {}\n",
        positive_fixture_text(),
        "x".repeat(willikins_dsl::MAX_DOCUMENT_BYTES)
    );
    std::fs::write(&path, &text).unwrap();

    let output = run(&["--json", "apply", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2, "stdout: {}", stdout(&output));
    assert!(
        stderr(&output).contains("TooLarge"),
        "stderr should be the DSL's own cap refusal, got: {}",
        stderr(&output)
    );
}

/// A directory where a file was expected is a plain document refusal,
/// not a panic and not a temporary directory full of nothing.
#[test]
fn a_directory_instead_of_a_file_is_refused() {
    let dir = TempDir::new("directory-arg");
    let output = run(&["apply", dir.path().to_str().unwrap()]);
    assert_eq!(
        exit_code(&output),
        2,
        "stdout: {} stderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        !stderr(&output).is_empty(),
        "a refusal must say something on stderr"
    );
}

// =====================================================================
// 2. an approval is honoured exactly once
// =====================================================================

fn extract_plan_id(json_stdout: &str) -> String {
    let docs = json_documents(json_stdout);
    docs[0]["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no plan_id in {json_stdout}"))
        .to_string()
}

/// Record a pending plan for `workflows/fixtures/irreversible.yaml` in a
/// trusted directory of its own, returning the directory, the journal
/// path and the plan id.
fn pending_irreversible_plan(label: &str) -> (TempDir, PathBuf, String) {
    let dir = TempDir::new(label);
    std::fs::copy(
        workflow("workflows/fixtures/irreversible.yaml"),
        dir.join("new-rust-service-irreversible.yaml"),
    )
    .unwrap();
    let journal = dir.join("journal.jsonl");
    let mut args = vec![
        "--json".to_string(),
        "apply".to_string(),
        dir.join("new-rust-service-irreversible.yaml")
            .to_str()
            .unwrap()
            .to_string(),
    ];
    args.extend(POSITIVE_INPUTS.map(str::to_string));
    args.push("--journal".to_string());
    args.push(journal.to_str().unwrap().to_string());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run(&borrowed);
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let plan_id = extract_plan_id(&stdout(&output));
    (dir, journal, plan_id)
}

/// The load-bearing claim: one `approve`, two `apply --plan-id`. The
/// second is refused `AlreadyApplied`, naming the run the first started,
/// and the journal holds exactly one `run_started` for that plan.
#[test]
fn an_approved_plan_applies_once_and_the_second_apply_is_refused() {
    let (dir, journal, plan_id) = pending_irreversible_plan("once-only");

    let approved = run(&["approve", &plan_id, "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));

    let apply_args = [
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ];

    let first = run(&apply_args);
    assert_eq!(exit_code(&first), 0, "stderr: {}", stderr(&first));

    let second = run(&apply_args);
    assert_eq!(
        exit_code(&second),
        1,
        "the second apply must be refused: {}",
        stdout(&second)
    );
    let docs = json_documents(&stdout(&second));
    assert_eq!(docs[0]["kind"], "AlreadyApplied", "{docs:?}");
    assert!(docs[0]["run_id"].is_string(), "{docs:?}");

    // And the journal itself records one run for this plan, not two.
    let replayed = willikins_journal::replay(&journal).expect("the journal must still validate");
    let started = replayed
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                &entry.event,
                willikins_journal::Event::RunStarted { plan_id: p, .. }
                    if p.to_string() == plan_id
            )
        })
        .count();
    assert_eq!(started, 1, "exactly one run_started for the approved plan");
}

// =====================================================================
// 3. the journal lock, both directions
// =====================================================================

/// A writer refuses plainly while another holder has the file; a reader
/// goes right through; and once the holder lets go, the writer completes
/// and the file still replays clean.
#[test]
fn a_held_journal_refuses_a_writer_admits_a_reader_and_validates_afterwards() {
    let dir = TempDir::new("lock-both-ways");
    let journal = dir.join("journal.jsonl");
    std::fs::copy(
        workflow("workflows/new-rust-service.yaml"),
        dir.join("new-rust-service.yaml"),
    )
    .unwrap();

    // Seed the file with a real run so `runs` has something to show.
    let document = dir.join("new-rust-service.yaml");
    let document_str = document.to_str().unwrap().to_string();
    let journal_str = journal.to_str().unwrap().to_string();
    let mut seed = vec!["apply", &document_str];
    seed.extend_from_slice(&POSITIVE_INPUTS);
    seed.extend_from_slice(&["--journal", &journal_str]);
    let seeded = run(&seed);
    assert_eq!(exit_code(&seeded), 0, "stderr: {}", stderr(&seeded));

    let held = willikins_journal::FileJournal::open(&journal).expect("the test must take the lock");

    // A writer: refused, exit 2, on stderr, kind-tagged `Journal`.
    let mut writer = vec!["--json", "apply", &document_str];
    writer.extend_from_slice(&POSITIVE_INPUTS);
    writer.extend_from_slice(&["--journal", &journal_str]);
    let refused = run(&writer);
    assert_eq!(exit_code(&refused), 2, "stdout: {}", stdout(&refused));
    let reported = stderr(&refused);
    let docs = json_documents(&reported);
    assert_eq!(docs[0]["kind"], "Journal", "{docs:?}");
    assert_no_duplicate_keys(reported.trim(), "apply against a held journal");

    // A reader: straight through, exit 0, with the seeded run in it.
    let reading = run(&["runs", "--journal", &journal_str]);
    assert_eq!(exit_code(&reading), 0, "stderr: {}", stderr(&reading));
    assert!(
        stdout(&reading).contains("state: succeeded"),
        "{}",
        stdout(&reading)
    );

    drop(held);

    // The writer completes once the lock is free, and the file replays.
    let after = run(&writer);
    assert_eq!(exit_code(&after), 0, "stderr: {}", stderr(&after));
    let replayed = willikins_journal::replay(&journal).expect("the journal must still validate");
    assert_eq!(replayed.runs().len(), 2, "two runs recorded");
}

/// A journal whose last line was cut short -- an unclean exit, or a
/// reader that landed mid-append -- is refused by `runs` with exactly the
/// error `FileJournal::open` gives for the same file, since both go
/// through one validator.
#[test]
fn a_truncated_journal_is_refused_the_same_way_by_runs_and_by_open() {
    let dir = TempDir::new("truncated");
    let journal = dir.join("journal.jsonl");
    std::fs::copy(
        workflow("workflows/new-rust-service.yaml"),
        dir.join("new-rust-service.yaml"),
    )
    .unwrap();
    let journal_str = journal.to_str().unwrap().to_string();
    let document = dir.join("new-rust-service.yaml");
    let document_str = document.to_str().unwrap().to_string();
    let mut seed = vec!["apply", &document_str];
    seed.extend_from_slice(&POSITIVE_INPUTS);
    seed.extend_from_slice(&["--journal", &journal_str]);
    assert_eq!(exit_code(&run(&seed)), 0);

    let text = std::fs::read_to_string(&journal).unwrap();
    let cut = text.len() - 20;
    std::fs::write(&journal, &text[..cut]).unwrap();

    let open_error = match willikins_journal::FileJournal::open(&journal) {
        Ok(_) => panic!("`FileJournal::open` must refuse a truncated journal"),
        Err(error) => error.to_string(),
    };

    let output = run(&["runs", "--journal", &journal_str]);
    assert_eq!(exit_code(&output), 2, "stdout: {}", stdout(&output));
    assert!(
        stderr(&output).contains(&open_error),
        "runs must give `open`'s own error.\nopen: {open_error}\nruns: {}",
        stderr(&output)
    );
}

// =====================================================================
// 4. secrets, every position a Value can occupy
// =====================================================================

const SEEDED_SECRET: &str = "acceptance-test-8b-fake-secret-bytes-do-not-leak";
/// `concat!`-split so this file holds no literal spelling a full
/// Doppler-service-token-shaped string contiguously; the compiled value
/// (still a valid `DopplerServiceToken`) is byte-identical to one that did.
const SEEDED_TOKEN: &str = concat!("dp.st.prd.", "donotleakdonotleakdonotleakdonotleakdono");

/// A secret in each of the positions a `Value` can occupy -- a tool
/// output (`doppler.secret.get`), a freshly minted token flowing into a
/// `for_each`-fed node's input (the positive fixture's `configs` loop
/// feeding `token` feeding `ci_secret`), a secret literal in the document
/// (refused by `check`, so its bytes must not be echoed either), and the
/// redaction marker itself written as a document literal -- never reaches
/// stdout or stderr, in text or JSON, through `apply`, `runs` or `run`.
#[test]
fn no_secret_position_leaks_through_apply_runs_or_run() {
    let dir = TempDir::new("secret-positions");
    let journal = dir.join("journal.jsonl");
    let journal_str = journal.to_str().unwrap().to_string();

    // Position 1 + 2: a seeded secret read by a tool and passed into a
    // secret-accepting input, and a minted token consumed by the
    // `for_each`-fed chain of the positive fixture.
    let state = dir.join("state.json");
    std::fs::write(
        &state,
        serde_json::to_string_pretty(&serde_json::json!({
            "doppler_secrets": { "widgets/prd#DATABASE_URL": SEEDED_SECRET },
            "next_token": SEEDED_TOKEN,
        }))
        .unwrap(),
    )
    .unwrap();

    for json in [false, true] {
        let mut args: Vec<String> = Vec::new();
        if json {
            args.push("--json".to_string());
        }
        args.extend(
            [
                "apply",
                workflow("workflows/fixtures/secret-get.yaml")
                    .to_str()
                    .unwrap(),
                "--input",
                "project=widgets",
                "--fake-state",
                state.to_str().unwrap(),
                "--journal",
                &journal_str,
            ]
            .map(str::to_string),
        );
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = run(&borrowed);
        assert_no_secret(&output, "apply secret-get");
    }

    // The freshly minted service token (position 2), applied out of the
    // positive fixture with the same seeded `next_token`.
    let mut args = vec![
        "apply".to_string(),
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap()
            .to_string(),
    ];
    args.extend(POSITIVE_INPUTS.map(str::to_string));
    args.extend(
        [
            "--fake-state",
            state.to_str().unwrap(),
            "--journal",
            &journal_str,
        ]
        .map(str::to_string),
    );
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run(&borrowed);
    assert_no_secret(&output, "apply new-rust-service with a seeded next_token");
    assert!(
        stdout(&output).contains("[REDACTED DopplerServiceToken]"),
        "the minted token must render as its marker: {}",
        stdout(&output)
    );

    assert_a_secret_literal_is_never_quoted_back();
    assert_a_document_spelling_the_marker_still_applies(&journal_str);

    // Everything the journal recorded, read back both ways.
    for command in [
        vec!["runs", "--journal", &journal_str],
        vec!["--json", "runs", "--journal", &journal_str],
    ] {
        let output = run(&command);
        assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
        assert_no_secret(&output, "runs");
    }

    // ... and the journal file itself.
    let raw = std::fs::read_to_string(&journal).unwrap();
    assert!(!raw.contains(SEEDED_SECRET), "the journal file leaked");
    assert!(!raw.contains(SEEDED_TOKEN), "the journal file leaked");

    // `run <id>` for every run recorded.
    let replayed = willikins_journal::replay(&journal).unwrap();
    for record in replayed.runs() {
        let id = record.run_id.to_string();
        for command in [
            vec!["run", &id, "--journal", &journal_str],
            vec!["--json", "run", &id, "--journal", &journal_str],
        ] {
            let output = run(&command);
            assert_no_secret(&output, "run <id>");
        }
    }
}

/// Position 3: a secret literal written straight into the document.
/// `check` refuses it, and its bytes must not be quoted back in the
/// refusal either -- in text or JSON.
fn assert_a_secret_literal_is_never_quoted_back() {
    let path = workflow("workflows/fixtures/secret-literal.yaml");
    let path = path.to_str().unwrap().to_string();
    for flags in [vec![], vec!["--json"]] {
        let mut args = flags;
        args.extend_from_slice(&["apply", &path]);
        let output = run(&args);
        assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
        assert!(
            !stdout(&output).contains("hunter2"),
            "the secret literal's bytes leaked: {}",
            stdout(&output)
        );
        assert!(
            !stderr(&output).contains("hunter2"),
            "the secret literal's bytes leaked to stderr: {}",
            stderr(&output)
        );
    }
}

/// Position 4: the redaction marker itself, written as a document
/// literal. It renders as the plain text it is -- nothing here is secret,
/// and the run is an ordinary success.
fn assert_a_document_spelling_the_marker_still_applies(journal_str: &str) {
    let path = workflow("workflows/fixtures/redaction-marker-default.yaml");
    let path = path.to_str().unwrap().to_string();
    for flags in [vec![], vec!["--json"]] {
        let mut args = flags;
        args.extend_from_slice(&["apply", &path, "--journal", journal_str]);
        let output = run(&args);
        assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    }
}

fn assert_no_secret(output: &Output, what: &str) {
    for (stream, text) in [("stdout", stdout(output)), ("stderr", stderr(output))] {
        assert!(
            !text.contains(SEEDED_SECRET),
            "{what}: {stream} leaked the seeded secret: {text}"
        );
        assert!(
            !text.contains(SEEDED_TOKEN),
            "{what}: {stream} leaked the seeded token: {text}"
        );
    }
}

// =====================================================================
// 5. kinds and exit codes, one plan per refusal class
// =====================================================================

/// Every refusal class `apply` can reach, with the `kind` its JSON
/// carries, the stream it goes to, and the exit code -- the table the MCP
/// surface's own `apply` tool must agree with on `kind` (it serializes
/// the same `ButlerError` through the same `Reported`) and the CLI's
/// pre-existing refusals must agree with on stream and exit code (a
/// `Butler` refusal is a domain result: stdout, exit 1, exactly as a
/// `PlanError` from `plan` is; a journal, id, credential or startup
/// failure is a configuration refusal: stderr, exit 2, exactly as a
/// `DocumentError` is).
#[test]
fn every_apply_refusal_class_has_its_kind_stream_and_exit_code() {
    // --- ApprovalRequired -------------------------------------------
    let (dir, journal, plan_id) = pending_irreversible_plan("class-approval");
    let journal_str = journal.to_str().unwrap().to_string();
    let refused = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_domain_refusal(&refused, "ApprovalRequired");

    // --- UnknownPlan: an id from another journal --------------------
    let other = TempDir::new("class-unknown-other");
    let other_journal = other.join("journal.jsonl");
    std::fs::write(&other_journal, "").unwrap();
    let elsewhere = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        other_journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_domain_refusal(&elsewhere, "UnknownPlan");

    // --- UnknownPlan: an id nothing ever recorded -------------------
    let never = run(&[
        "--json",
        "apply",
        "--plan-id",
        "018e0000-0000-7000-8000-0000000000ff",
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_domain_refusal(&never, "UnknownPlan");

    // --- DocumentChanged: the document moved under the plan ---------
    let approved = run(&["approve", &plan_id, "--journal", &journal_str]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));
    let doc = dir.join("new-rust-service-irreversible.yaml");
    let original = std::fs::read_to_string(&doc).unwrap();
    std::fs::write(
        &doc,
        format!("{original}\n# a comment the plan never saw\n"),
    )
    .unwrap();
    let changed = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_domain_refusal(&changed, "DocumentChanged");
    std::fs::write(&doc, &original).unwrap();

    // --- Drift: the world moved under the plan ----------------------
    let state = dir.join("drifted.json");
    std::fs::copy(workflow("workflows/fixtures/state/repo-ours.json"), &state).unwrap();
    let drifted = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
        "--fake-state",
        state.to_str().unwrap(),
    ]);
    assert_domain_refusal(&drifted, "Drift");

    // --- AlreadyApplied ---------------------------------------------
    let applied = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    let again = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        &journal_str,
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_domain_refusal(&again, "AlreadyApplied");
}

/// A `Butler` refusal: stdout, exit 1, one kind-tagged JSON object whose
/// `kind` is `expected`, carrying a `message`, with no duplicate key.
fn assert_domain_refusal(output: &Output, expected: &str) {
    assert_eq!(
        exit_code(output),
        1,
        "{expected}: expected exit 1.\nstdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
    assert!(
        stderr(output).is_empty(),
        "{expected}: a domain refusal belongs on stdout, stderr said: {}",
        stderr(output)
    );
    let text = stdout(output);
    let docs = json_documents(&text);
    let refusal = docs.last().expect("a refusal document");
    assert_eq!(refusal["kind"], expected, "got {docs:?}");
    assert!(
        refusal["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{expected}: no message in {docs:?}"
    );
    // The refusal is the last document printed, so it is the last object
    // in the stream: check its own keys for a `Reported` collision.
    let last = text
        .rfind("\n{")
        .map_or(text.as_str(), |at| &text[at + 1..])
        .trim();
    assert_no_duplicate_keys(last, expected);
}

/// The configuration-refusal half of the same table: stderr, exit 2, one
/// kind-tagged object, no duplicate key. These are the shapes task 11
/// newly hands to `Reported` at top level for the first time.
#[test]
fn every_configuration_refusal_is_kind_tagged_on_stderr_with_exit_2() {
    let dir = TempDir::new("config-refusals");

    // A journal that will not open.
    let missing = dir.join("nope").join("journal.jsonl");
    let output = run(&["--json", "runs", "--journal", missing.to_str().unwrap()]);
    assert_config_refusal(&output, "Journal");

    // An id that does not parse.
    let empty = dir.join("journal.jsonl");
    std::fs::write(&empty, "").unwrap();
    let output = run(&[
        "--json",
        "run",
        "not-a-run-id",
        "--journal",
        empty.to_str().unwrap(),
    ]);
    assert_config_refusal(&output, "InvalidId");

    // A trusted directory that fails startup validation. `Bad_Stem` is
    // not a `WorkflowName`, so the scan refuses with `InvalidName` --
    // reached only through `--plan-id`, which points a `Butler` at a real
    // directory rather than a private, always-consistent temporary one.
    let bad = TempDir::new("config-bad-dir");
    std::fs::write(bad.join("Bad_Stem.yaml"), positive_fixture_text()).unwrap();
    let output = run(&[
        "--json",
        "apply",
        "--plan-id",
        "018e0000-0000-7000-8000-0000000000ff",
        "--journal",
        empty.to_str().unwrap(),
        "--workflows-dir",
        bad.path().to_str().unwrap(),
    ]);
    // The kind is `Startup`, exactly as the MCP surface reports the same
    // failure (`ButlerError::Startup { error }`), with the scan's own
    // `StartupError` nested inside rather than flattened to the top --
    // which is also what keeps its `message` field from colliding with
    // the one `Reported` adds.
    assert_config_refusal(&output, "Startup");
    assert!(
        stderr(&output).contains("InvalidName"),
        "stderr: {}",
        stderr(&output)
    );

    // A missing or malformed `--live` credential, refused before any
    // network call could be attempted.
    let output = run_with_env(
        &[
            "--json",
            "plan",
            workflow("workflows/new-rust-service.yaml")
                .to_str()
                .unwrap(),
            "--live",
        ],
        &[("WILLIKINS_GITHUB_TOKEN", "not-a-github-token")],
    );
    assert_config_refusal(&output, "GitHub");
    assert!(
        !stderr(&output).contains("not-a-github-token"),
        "the credential's value must never be echoed: {}",
        stderr(&output)
    );
}

fn assert_config_refusal(output: &Output, expected: &str) {
    assert_eq!(
        exit_code(output),
        2,
        "{expected}: expected exit 2.\nstdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
    assert!(
        stdout(output).is_empty(),
        "{expected}: a configuration refusal belongs on stderr, stdout said: {}",
        stdout(output)
    );
    let text = stderr(output);
    let trimmed = text.trim();
    let value: serde_json::Value =
        serde_json::from_str(trimmed).unwrap_or_else(|err| panic!("not JSON: {err}: {trimmed}"));
    assert_eq!(value["kind"], expected, "got {value}");
    assert!(
        value["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{expected}: no message in {value}"
    );
    assert_no_duplicate_keys(trimmed, expected);
}

// =====================================================================
// 6. a Destructive document, self-approved, and what the journal keeps
// =====================================================================

/// `workflows/rotate-service-token.yaml` is `Destructive` and always
/// needs a decision. `--approve` self-approves as `--principal` -- the
/// plan's own design for the CLI's local operator -- and the journal then
/// holds the approval naming that principal and a finished run, with the
/// rotated token's bytes nowhere in it.
#[test]
fn a_destructive_document_self_approved_is_journaled_as_approved_and_run() {
    let dir = TempDir::new("destructive");
    let journal = dir.join("journal.jsonl");
    let journal_str = journal.to_str().unwrap().to_string();
    let state = dir.join("state.json");
    std::fs::write(
        &state,
        serde_json::to_string_pretty(&serde_json::json!({ "next_token": SEEDED_TOKEN })).unwrap(),
    )
    .unwrap();

    let path = workflow("workflows/rotate-service-token.yaml");
    let base = [
        "apply",
        path.to_str().unwrap(),
        "--input",
        "project=widgets",
        "--input",
        "repo=lightless-labs/third-thoughts",
        "--fake-state",
        state.to_str().unwrap(),
        "--journal",
        &journal_str,
    ];

    // Without --approve: refused, and the refusal names the recovery.
    let refused = run(&base);
    assert_eq!(exit_code(&refused), 1, "stderr: {}", stderr(&refused));
    let text = stdout(&refused);
    assert!(
        text.contains("ApprovalRequired") || text.contains("approval"),
        "{text}"
    );
    assert!(
        text.contains("willikins approve") && text.contains(&journal_str),
        "the text refusal must name the recovery: {text}"
    );

    // With --approve, as a named principal.
    let mut approved: Vec<&str> = base.to_vec();
    approved.extend_from_slice(&["--approve", "--principal", "release-manager"]);
    let output = run(&approved);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("state: succeeded"),
        "{}",
        stdout(&output)
    );
    assert_no_secret(&output, "rotate-service-token");

    let raw = std::fs::read_to_string(&journal).unwrap();
    assert!(raw.contains("\"approval_granted\""), "{raw}");
    assert!(raw.contains("release-manager"), "{raw}");
    assert!(raw.contains("\"run_finished\""), "{raw}");
    assert!(!raw.contains(SEEDED_TOKEN), "the journal leaked the token");

    let replayed = willikins_journal::replay(&journal).expect("the journal must validate");
    let runs = replayed.runs();
    assert_eq!(runs.len(), 1, "one run recorded");
    assert!(
        matches!(runs[0].state, willikins_journal::RunState::Succeeded),
        "{:?}",
        runs[0].state
    );
}

/// With the default in-memory journal there is no `approve` command that
/// could ever reach the plan, so the text refusal says exactly that
/// rather than naming commands that cannot work; the JSON refusal carries
/// the same `kind`/`message` the MCP surface's own `apply` gives, and
/// nothing else -- the guidance is a CLI affordance, not a wire field
/// that would break structural parity.
#[test]
fn approval_required_without_a_journal_names_the_recovery_in_text_and_stays_parity_shaped_in_json()
{
    let path = workflow("workflows/fixtures/irreversible.yaml");
    let mut args = vec!["apply", path.to_str().unwrap()];
    args.extend_from_slice(&POSITIVE_INPUTS);

    let text_output = run(&args);
    assert_eq!(
        exit_code(&text_output),
        1,
        "stderr: {}",
        stderr(&text_output)
    );
    let text = stdout(&text_output);
    assert!(
        text.contains("no --journal was given"),
        "the in-memory refusal must say the plan is gone: {text}"
    );
    assert!(
        text.contains("--journal <path>") && text.contains("--approve"),
        "and must name the recovery: {text}"
    );
    assert!(
        !text.contains("willikins approve "),
        "it must not name a command that cannot reach this plan: {text}"
    );

    let mut json_args = vec!["--json"];
    json_args.extend_from_slice(&args);
    let json_output = run(&json_args);
    assert_eq!(exit_code(&json_output), 1);
    let docs = json_documents(&stdout(&json_output));
    let refusal = docs.last().unwrap();
    assert_eq!(refusal["kind"], "ApprovalRequired", "{docs:?}");
    assert!(refusal["message"].as_str().is_some_and(|m| !m.is_empty()));
    // No CLI-only guidance field: the JSON object is the shared
    // `ButlerError` and nothing more.
    assert!(refusal.get("guidance").is_none(), "{refusal}");
}

/// `--fake-state-out` documents itself as a dump taken "after the run
/// reaches a final state", and its whole purpose is to be reloadable as
/// the next invocation's `--fake-state`. On a refusal no run ever starts,
/// so there is no ending state to dump -- and dumping the starting one
/// produces a file that cannot be reloaded at all: a seeded but
/// unconsumed `next_token` serializes as its redaction marker, which is
/// not a `DopplerServiceToken`. Nothing is written, and the command says
/// why on stderr without changing the refusal's own exit code.
#[test]
fn fake_state_out_writes_nothing_when_no_run_reached_a_final_state() {
    let dir = TempDir::new("fake-state-out-refusal");
    let seed = dir.join("seed.json");
    std::fs::write(
        &seed,
        serde_json::to_string_pretty(&serde_json::json!({ "next_token": SEEDED_TOKEN })).unwrap(),
    )
    .unwrap();
    let out = dir.join("out.json");

    let path = workflow("workflows/fixtures/irreversible.yaml");
    let mut args = vec!["apply", path.to_str().unwrap()];
    args.extend_from_slice(&POSITIVE_INPUTS);
    let seed_str = seed.to_str().unwrap().to_string();
    let out_str = out.to_str().unwrap().to_string();
    args.extend_from_slice(&["--fake-state", &seed_str, "--fake-state-out", &out_str]);

    let output = run(&args);
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    assert!(
        !out.exists(),
        "no run finished, so no ending state may be written: {}",
        std::fs::read_to_string(&out).unwrap_or_default()
    );
    assert!(
        stderr(&output).contains("--fake-state-out"),
        "the command must say nothing was written: {}",
        stderr(&output)
    );
}

// =====================================================================
// 7. serve, through this binary
// =====================================================================

/// `willikins serve` is `willikins-server serve`: one implementation, so
/// the same arguments in the same environment give the same refusal and
/// the same exit code. The three assertions below are exactly what
/// `crates/willikins-server/tests/binary_startup.rs` pins for its own
/// binary (`serve_without_stdio_refuses_with_exit_code_2`,
/// `serve_http_and_serve_stdio_together_refuses`,
/// `serve_stdio_with_no_environment_refuses_naming_the_missing_variable`).
#[test]
fn serve_through_the_willikins_binary_refuses_exactly_as_the_server_binary_does() {
    let neither = run(&["serve"]);
    assert_eq!(exit_code(&neither), 2);
    assert!(stderr(&neither).contains("--stdio"), "{}", stderr(&neither));

    let both = run(&["serve", "--stdio", "--http"]);
    assert_eq!(exit_code(&both), 2);
    assert!(stderr(&both).contains("not both"), "{}", stderr(&both));

    let bare = run(&["serve", "--stdio"]);
    assert_eq!(exit_code(&bare), 2);
    assert!(
        stderr(&bare).contains("WILLIKINS_WORKFLOWS_DIR"),
        "{}",
        stderr(&bare)
    );

    // And neither message names a binary, since one function serves both.
    assert!(
        !stderr(&neither).contains("willikins-server"),
        "{}",
        stderr(&neither)
    );
    assert!(
        !stderr(&both).contains("willikins-server"),
        "{}",
        stderr(&both)
    );
}
