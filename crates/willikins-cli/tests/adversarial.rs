//! End-to-end adversarial pass 2 (acceptance test 12, second pass): one
//! `#[test]` per bypass found by attacking the finished milestone from the
//! outside — YAML documents, `--fake-state` JSON, CLI invocations, and the
//! public library API only.
//!
//! Every test here corresponds to a numbered finding in
//! `docs/research/2026-09-12-e2e-adversarial-pass-2.md`, and every negative
//! fixture it loads lives under `workflows/fixtures/` with a header comment
//! naming the finding and the exact error the fixture must produce. The
//! attacks that found *nothing* are pinned here too, so a later change that
//! opens one of those holes fails a test rather than only contradicting a
//! table in the research note.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{Catalog, CheckError, OutputName, Workflow};
use willikins_providers_fake::FakeState;

// ---------------------------------------------------------------------
// paths, loading, and process helpers
// ---------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name)
}

fn positive_fixture() -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("new-rust-service.yaml")
}

/// Load and parse a workflow document, panicking on a parse failure: a
/// fixture this suite loads is expected to reach `check`, so a
/// `DocumentError` here is a bug in the fixture.
fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

fn empty_catalog() -> Catalog {
    willikins_providers_fake::empty().1
}

fn seeded_catalog(json: &str) -> Catalog {
    let state = Arc::new(Mutex::new(
        FakeState::from_json(json).expect("fake-state JSON should parse"),
    ));
    willikins_providers_fake::catalog(state)
}

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

/// Run the built `willikins` binary with `args`.
fn willikins(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
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

/// Write `contents` to a uniquely named file in this test binary's own
/// temp directory and return its path. Used for the hostile documents and
/// state files that are one-off probes rather than checked-in fixtures.
fn temp_file(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "willikins-adversarial-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("failed to create the temp directory");
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("failed to write the temp file");
    path
}

// ---------------------------------------------------------------------
// finding 1: a literal workflow output
// ---------------------------------------------------------------------

/// Before the fix, `check` accepted a workflow output bound to a bare
/// literal, recorded no type for it, and `plan` silently omitted it from
/// `Plan::outputs` — so `plan` reported one output for a document that
/// declares two, a plan that misrepresents the workflow's own surface.
/// `check` now refuses the binding outright: a workflow output has no port
/// to parse a literal against, so there is no honest type to give it.
#[test]
fn finding_01_a_literal_workflow_output_is_refused() {
    let workflow = load(&fixture("literal-output.yaml"));
    let errors = willikins_core::check(&workflow, &empty_catalog())
        .expect_err("finding 1: a literal workflow output must be refused");
    assert_eq!(
        errors,
        vec![CheckError::LiteralOutput {
            output: output("repo_url"),
        }],
        "finding 1: expected exactly one LiteralOutput error"
    );
}

/// The invariant the fix buys, stated over the milestone's own positive
/// fixture: every output a checked workflow declares has a resolved type,
/// and every one of those appears in the finished `Plan`. Nothing a
/// document can declare is silently dropped between `check` and `plan`.
#[test]
fn finding_01_every_declared_output_reaches_the_plan() {
    let workflow = load(&positive_fixture());
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog).expect("the positive fixture checks");

    let declared: Vec<&OutputName> = checked.workflow.outputs.keys().collect();
    let typed: Vec<&OutputName> = checked.output_types.keys().collect();
    assert_eq!(
        declared, typed,
        "finding 1: every declared output must have a resolved type"
    );

    let mut partial = PartialInputs::new();
    partial.insert(
        willikins_core::InputName::parse("slug").unwrap(),
        RawInput::Scalar("widgets".to_string()),
    );
    partial.insert(
        willikins_core::InputName::parse("org").unwrap(),
        RawInput::Scalar("lightless-labs".to_string()),
    );
    let description = willikins_core::describe(&checked, &partial);
    assert!(description.errors.is_empty() && description.missing.is_empty());
    let plan = willikins_core::plan(&checked, &description.resolved, &catalog).expect("plans");

    let planned: Vec<&OutputName> = plan.outputs.keys().collect();
    assert_eq!(
        declared, planned,
        "finding 1: every declared output must appear in Plan::outputs"
    );
}

/// The same finding through the CLI, which is the surface an agent
/// actually reads: the document is refused with exit 1 and a
/// `LiteralOutput` line on stdout, rather than planning cleanly with an
/// output missing.
#[test]
fn finding_01_the_cli_refuses_a_literal_output() {
    let path = fixture("literal-output.yaml");
    let path = path.to_str().unwrap();

    let validated = willikins(&["validate", path]);
    assert_eq!(
        exit_code(&validated),
        1,
        "finding 1: {}",
        stdout(&validated)
    );
    assert!(
        stdout(&validated).contains("LiteralOutput"),
        "finding 1: {}",
        stdout(&validated)
    );

    let planned = willikins(&["plan", path]);
    assert_eq!(exit_code(&planned), 1, "finding 1: {}", stdout(&planned));
    assert!(
        !stdout(&planned).contains("class:"),
        "finding 1: plan must not produce a plan: {}",
        stdout(&planned)
    );
}

// ---------------------------------------------------------------------
// pinned: attacks that found nothing
// ---------------------------------------------------------------------

/// A seeded secret whose bytes contain newlines, a tab, an ANSI escape,
/// the redaction marker itself, and JSON metacharacters still never
/// reaches stdout: every rendering path goes through `Value`, which
/// redacts by construction, so the marker is all either output mode ever
/// carries.
#[test]
fn pinned_a_hostile_secret_value_never_reaches_stdout() {
    let nasty = format!(
        "line1{}line2{}tab {}[2J [REDACTED DopplerSecretValue] \"quote\" \\ backslash",
        '\n', '\t', '\u{1b}'
    );
    let state = serde_json::json!({
        "doppler_secrets": { "widgets/prd#DATABASE_URL": nasty },
    })
    .to_string();
    let state_path = temp_file("hostile-secret.json", &state);

    let workflow = load(&fixture("secret-get.yaml"));
    let catalog = seeded_catalog(&state);
    let checked = willikins_core::check(&workflow, &catalog).expect("secret-get.yaml checks");
    let mut partial = PartialInputs::new();
    partial.insert(
        willikins_core::InputName::parse("project").unwrap(),
        RawInput::Scalar("widgets".to_string()),
    );
    let description = willikins_core::describe(&checked, &partial);
    let plan = willikins_core::plan(&checked, &description.resolved, &catalog).expect("plans");

    let json = serde_json::to_string_pretty(&plan).expect("Plan serializes");
    let debug = format!("{plan:?}");
    for rendering in [&json, &debug] {
        assert!(
            !rendering.contains("backslash"),
            "pinned: a hostile secret leaked: {rendering}"
        );
        assert!(
            rendering.contains("[REDACTED DopplerSecretValue]"),
            "pinned: the redaction marker is missing: {rendering}"
        );
    }

    for args in [
        vec![
            "plan",
            fixture("secret-get.yaml").to_str().unwrap(),
            "--input",
            "project=widgets",
            "--fake-state",
            state_path.to_str().unwrap(),
        ],
        vec![
            "--json",
            "plan",
            fixture("secret-get.yaml").to_str().unwrap(),
            "--input",
            "project=widgets",
            "--fake-state",
            state_path.to_str().unwrap(),
        ],
    ] {
        let output = willikins(&args);
        assert_eq!(exit_code(&output), 0, "pinned: {}", stderr(&output));
        assert!(
            !stdout(&output).contains("backslash"),
            "pinned: a hostile secret leaked through the CLI: {}",
            stdout(&output)
        );
    }
}

/// A seeded secret whose bytes are *exactly* a non-secret value that
/// appears elsewhere in the same plan (a repository's canonical name).
/// **Accepted, deliberately.** The repository name still prints, because
/// it is a `GitHubRepo` the document itself supplies — the secret's own
/// `Value` is redacted as always. No implementation can distinguish "these
/// bytes are also a secret somewhere" without comparing plaintext, which
/// would require exposing the secret to do it. The consequence for tests:
/// a `!output.contains(secret_bytes)` assertion is only meaningful when the
/// chosen bytes appear nowhere else, which is why every redaction test in
/// this workspace seeds a distinctive marker string.
#[test]
fn pinned_a_secret_equal_to_a_public_value_still_prints_the_public_one() {
    let state = serde_json::json!({
        "doppler_secrets": { "widgets/prd#DATABASE_URL": "lightless-labs/third-thoughts" },
    })
    .to_string();
    let state_path = temp_file("colliding-secret.json", &state);

    let output = willikins(&[
        "--json",
        "plan",
        fixture("secret-get.yaml").to_str().unwrap(),
        "--input",
        "project=widgets",
        "--fake-state",
        state_path.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 0, "pinned: {}", stderr(&output));
    let out = stdout(&output);
    // The secret's own port carries the marker...
    assert!(
        out.contains("[REDACTED DopplerSecretValue]"),
        "pinned: {out}"
    );
    // ...while the identically-valued repository name prints, because it is
    // a public `GitHubRepo` the document wrote down, not the secret.
    assert!(
        out.contains("lightless-labs/third-thoughts"),
        "pinned: the public repo name should still print: {out}"
    );
}

/// A YAML alias chain that expands exponentially ("billion laughs") in a
/// field the document format does not declare is skipped, not expanded:
/// `serde` walks the ignored value without resolving the aliases, so the
/// document loads in ordinary time and `check` reports its ordinary
/// errors.
#[test]
fn pinned_a_yaml_alias_bomb_in_an_ignored_field_does_not_expand() {
    let mut lines = vec![
        "name: bomb".to_string(),
        "steps:".to_string(),
        "  a: { tool: naming.v1, with: { org: lightless-labs, slug: demo } }".to_string(),
        "junk: &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol]".to_string(),
    ];
    for i in 1..10 {
        let prev = format!("*a{}", i - 1);
        let row = vec![prev; 9].join(", ");
        lines.push(format!("j{i}: &a{i} [{row}]"));
    }
    lines.push("final: *a9".to_string());
    let source = lines.join("\n");

    // Before the unknown-field fix this parsed and checked cleanly; after
    // it, it is refused as an unknown field. Either way the point stands:
    // the expansion never happens, so this returns rather than hanging.
    let result = willikins_dsl::parse_document(&source);
    match result {
        Ok(workflow) => {
            willikins_core::check(&workflow, &empty_catalog()).expect("the bomb workflow checks");
        }
        Err(err) => {
            let message = err.to_string();
            assert!(
                message.contains("junk") || message.contains("unknown field"),
                "pinned: unexpected failure: {message}"
            );
        }
    }
}

/// A YAML stream holding more than one document is refused outright, so a
/// second document cannot smuggle a different workflow past a reader who
/// only looked at the first.
#[test]
fn pinned_a_multi_document_yaml_stream_is_refused() {
    let source = "\
name: first
steps:
  a: { tool: naming.v1, with: { org: lightless-labs, slug: demo } }
---
name: second
steps:
  b: { tool: naming.v1, with: { org: evil-org, slug: pwned } }
";
    let Err(err) = willikins_dsl::parse_document(source) else {
        panic!("pinned: a multi-document stream must be refused");
    };
    assert!(
        err.to_string().contains("more than one document"),
        "pinned: {err}"
    );
}

// ---------------------------------------------------------------------
// finding 2: the `outputs` sentinel swallowed a real reference
// ---------------------------------------------------------------------

/// A step literally named `outputs` keeps its own port types, and a
/// workflow output that references one of that step's ports resolves
/// against the step it actually names.
///
/// `check` reports an output binding's own errors under the synthetic node
/// name `outputs` (an output is not a node). Before the fix, the
/// self-reference shortcut in `resolve_reference` compared that synthetic
/// name against the referenced node's name, so an output referencing a
/// real step named `outputs` looked like a node referencing itself and was
/// dropped: no error, and no entry in `Checked::output_types`. `plan`
/// resolves bindings itself and still produced a value for it, so `check`
/// and `plan` disagreed about the workflow's output surface — the same
/// class of bug as finding 1, reached from the other side.
#[test]
fn finding_02_an_output_referencing_a_step_named_outputs_resolves() {
    let workflow = load(&fixture("output-from-step-named-outputs.yaml"));
    let checked = willikins_core::check(&workflow, &empty_catalog())
        .expect("finding 2: this document must check cleanly");

    // The node's own port types are untouched by the output sentinel...
    assert_eq!(
        checked.types[&willikins_core::NodeName::parse("outputs").unwrap()]
            [&willikins_core::PortName::parse("slug").unwrap()],
        willikins_core::TypeRef::scalar(willikins_core::TypeName::parse("ProjectSlug").unwrap()),
        "finding 2: the step named `outputs` must keep its own port types"
    );
    // ...and the workflow output resolved to the port it actually names.
    assert_eq!(
        checked.output_types[&output("github_repo")],
        willikins_core::TypeRef::scalar(willikins_core::TypeName::parse("DopplerProject").unwrap()),
        "finding 2: the output must resolve against the step it names"
    );

    // `check` and `plan` now agree: the output is typed and planned.
    let partial = PartialInputs::new();
    let description = willikins_core::describe(&checked, &partial);
    let plan = willikins_core::plan(&checked, &description.resolved, &empty_catalog())
        .expect("finding 2: plans");
    assert_eq!(
        plan.outputs.keys().collect::<Vec<_>>(),
        checked.output_types.keys().collect::<Vec<_>>(),
        "finding 2: check and plan must agree on the output surface"
    );
}

/// A node that references *itself* is still a cycle, not an accepted
/// binding: the finding 2 fix narrowed the self-reference shortcut to real
/// node sites rather than removing it.
#[test]
fn finding_02_a_node_referencing_itself_is_still_a_cycle() {
    let source = "\
name: self-reference
steps:
  a:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: ${{ steps.a.doppler_project }}
";
    let workflow = willikins_dsl::parse_document(source).expect("finding 2: parses");
    let errors = willikins_core::check(&workflow, &empty_catalog())
        .expect_err("finding 2: a self-reference must still be refused");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, CheckError::Cycle { .. })),
        "finding 2: expected a Cycle, got {errors:?}"
    );
}

// ---------------------------------------------------------------------
// finding 3: an unknown field in a file willikins reads
// ---------------------------------------------------------------------

/// A typo'd key in a workflow document used to be silently ignored, so a
/// document could validate clean while doing something other than what its
/// author wrote. The worst shape is a misspelled `for_each`: the step then
/// runs once instead of once per item, with nothing said about it.
/// Every unrecognised field is now a `DocumentError` naming it.
#[test]
fn finding_03_a_typod_document_field_is_refused() {
    let cases = [
        // A misspelled `for_each` on a step.
        (
            "foreach",
            "\
name: typo
inputs:
  environments: { type: list<EnvironmentSlug>, default: [dev, prd] }
steps:
  configs:
    tool: doppler.config.ensure
    foreach: ${{ inputs.environments }}
    with:
      project: widgets
      environment: prd
",
        ),
        // A misspelled `default` on an input.
        (
            "defualt",
            "\
name: typo
inputs:
  org: { type: GitHubOrg, defualt: lightless-labs }
steps:
  a: { tool: naming.v1, with: { org: lightless-labs, slug: demo } }
",
        ),
        // A misspelled `description` at the top level.
        (
            "descriptin",
            "\
name: typo
descriptin: a typo
steps:
  a: { tool: naming.v1, with: { org: lightless-labs, slug: demo } }
",
        ),
    ];

    for (field, source) in cases {
        let Err(err) = willikins_dsl::parse_document(source) else {
            panic!("finding 3: `{field}` must be refused");
        };
        assert!(
            err.to_string().contains(field),
            "finding 3: the error must name `{field}`: {err}"
        );
    }
}

/// A YAML merge key (`<<`) is not a field the document format knows, and
/// `serde` never applies one when deserializing into a struct. Silently
/// ignoring it meant a document written with merge-key defaults lost them
/// without a word; it is now refused like any other unknown field.
#[test]
fn finding_03_a_yaml_merge_key_is_refused() {
    let source = "\
name: merge
base: &base
  tool: naming.v1
  with: { org: lightless-labs, slug: demo }
steps:
  a:
    <<: *base
";
    let Err(err) = willikins_dsl::parse_document(source) else {
        panic!("finding 3: a merge key must not be silently dropped");
    };
    assert!(
        err.to_string().contains("base") || err.to_string().contains("<<"),
        "finding 3: {err}"
    );
}

/// A typo'd key in a `--fake-state` file used to be silently ignored, so a
/// seeded resource simply did not exist and `plan` reported `Create` where
/// the author had asked for `NoOp` — a plan that misrepresents what would
/// happen, with no diagnostic anywhere.
#[test]
fn finding_03_a_typod_fake_state_field_is_refused() {
    // `github_repo`, not `github_repos`.
    let typod = serde_json::json!({
        "github_repo": {
            "lightless-labs/widgets": { "visibility": "private", "ours": true },
        },
    })
    .to_string();
    let err = FakeState::from_json(&typod)
        .expect_err("finding 3: an unknown fake-state field must be refused");
    assert!(
        err.to_string().contains("github_repo"),
        "finding 3: the error must name the field: {err}"
    );

    // A typo inside a record, which decides `Create` versus `NoOp`.
    let typod_record = serde_json::json!({
        "github_repos": {
            "lightless-labs/widgets": { "visibility": "private", "ours": true, "our": true },
        },
    })
    .to_string();
    FakeState::from_json(&typod_record)
        .expect_err("finding 3: an unknown record field must be refused");

    // Through the CLI: exit 2, on stderr, before anything is planned.
    let path = temp_file("typod-state.json", &typod);
    let output = willikins(&[
        "plan",
        positive_fixture().to_str().unwrap(),
        "--input",
        "slug=widgets",
        "--input",
        "org=lightless-labs",
        "--fake-state",
        path.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 2, "finding 3: {}", stdout(&output));
    assert!(
        stdout(&output).is_empty(),
        "finding 3: nothing may reach stdout: {}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains("invalid fake state"),
        "finding 3: {}",
        stderr(&output)
    );
}

/// The state fixtures this workspace ships must still load: `deny_unknown_fields`
/// composes with `#[serde(default)]`, so a file naming only the resources
/// a test cares about is still valid.
#[test]
fn finding_03_the_shipped_state_fixtures_still_load() {
    for name in ["repo-ours.json", "repo-foreign.json", "secret-seeded.json"] {
        let path = fixture("state").join(name);
        let json = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        FakeState::from_json(&json)
            .unwrap_or_else(|err| panic!("finding 3: {name} must still load: {err}"));
    }
    FakeState::from_json("{}").expect("finding 3: an empty state is still valid");
}

// ---------------------------------------------------------------------
// finding 4: a repeated `--input` for the same name
// ---------------------------------------------------------------------

/// `--input slug=a --input slug=b` used to resolve silently to `b`: the
/// partial-input map is keyed by name, so the second argument overwrote
/// the first with nothing said. An agent assembling a command line by
/// concatenation could therefore run against a value it never meant to
/// send, and `describe` would report the whole thing as resolved. A
/// repeated name is now refused before any input is parsed.
#[test]
fn finding_04_a_repeated_input_argument_is_refused() {
    for command in ["describe", "plan"] {
        let output = willikins(&[
            command,
            positive_fixture().to_str().unwrap(),
            "--input",
            "slug=widgets",
            "--input",
            "slug=something-else",
            "--input",
            "org=lightless-labs",
        ]);
        assert_eq!(
            exit_code(&output),
            2,
            "finding 4: {command}: {}",
            stdout(&output)
        );
        assert!(
            stdout(&output).is_empty(),
            "finding 4: {command}: nothing may reach stdout: {}",
            stdout(&output)
        );
        assert!(
            stderr(&output).contains("slug"),
            "finding 4: {command}: the error must name the input: {}",
            stderr(&output)
        );
        assert!(
            !stderr(&output).contains("something-else"),
            "finding 4: {command}: the error need not echo the value: {}",
            stderr(&output)
        );
    }
}

/// The same name supplied once is of course still fine, and two *different*
/// names are unaffected.
#[test]
fn finding_04_distinct_input_arguments_still_resolve() {
    let output = willikins(&[
        "describe",
        positive_fixture().to_str().unwrap(),
        "--input",
        "slug=widgets",
        "--input",
        "org=lightless-labs",
    ]);
    assert_eq!(exit_code(&output), 0, "finding 4: {}", stderr(&output));
    assert!(
        stdout(&output).contains("widgets"),
        "finding 4: {}",
        stdout(&output)
    );
}
