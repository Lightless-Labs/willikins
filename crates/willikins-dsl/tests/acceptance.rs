//! Loads the milestone 1 reference documents and checks them against
//! `willikins_providers_fake`'s catalog, per the plan's acceptance tests
//! 1 and 6.

use std::path::Path;

use indexmap::IndexMap;
use willikins_core::{Class, InputName, NodeName, Value};
use willikins_dsl::load_document;

fn fixture_path(relative: &str) -> String {
    format!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../{}"), relative)
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

#[test]
fn new_rust_service_checks_against_the_fake_catalog() {
    let path = fixture_path("workflows/new-rust-service.yaml");
    let workflow = load_document(Path::new(&path)).expect("the positive fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = willikins_core::check(&workflow, &catalog).unwrap_or_else(|errors| {
        panic!("expected the positive fixture to check cleanly: {errors:?}")
    });

    assert_eq!(
        checked.order,
        vec![
            node("names"),
            node("repo"),
            node("doppler"),
            node("configs"),
            node("token"),
            node("ci_secret"),
        ]
    );
    assert_eq!(checked.class, Class::Reversible);
    assert!(checked.warnings.is_empty());
}

#[test]
fn secret_into_template_fails_check_with_exactly_one_taint_error() {
    let path = fixture_path("workflows/fixtures/secret-into-template.yaml");
    let workflow = load_document(Path::new(&path)).expect("the negative fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("the tainted fixture must fail check");

    assert_eq!(errors.len(), 1, "{errors:?}");
    match &errors[0] {
        willikins_core::CheckError::SecretToNonSecretSink { from, to } => {
            assert_eq!(
                from,
                &(
                    node("token"),
                    willikins_core::PortName::parse("token").unwrap()
                )
            );
            assert_eq!(
                to,
                &willikins_core::Site::Port {
                    node: node("readme"),
                    port: willikins_core::PortName::parse("value").unwrap(),
                }
            );
        }
        other => panic!("expected SecretToNonSecretSink, got {other:?}"),
    }
}

/// Acceptance 16 (G3): a workflow input declared `OperatorAcknowledgement`
/// with a `default:` fails `check` with exactly one
/// `AcknowledgementDefault`, naming the offending input.
#[test]
fn acknowledgement_default_fails_check_with_exactly_one_error() {
    let path = fixture_path("workflows/fixtures/acknowledgement-default.yaml");
    let workflow = load_document(Path::new(&path)).expect("the negative fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("an operator-acknowledgement default must fail check");

    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        errors[0],
        willikins_core::CheckError::AcknowledgementDefault {
            input: willikins_core::InputName::parse("m7_bootstrap_done").unwrap(),
        }
    );
}

/// Acceptance 16 (G3): a literal bound to a port typed
/// `OperatorAcknowledgement` (here, `operator.acknowledge`'s own
/// `acknowledged` port) fails `check` with exactly one
/// `AcknowledgementLiteral`, naming the node and port.
#[test]
fn acknowledgement_literal_fails_check_with_exactly_one_error() {
    let path = fixture_path("workflows/fixtures/acknowledgement-literal.yaml");
    let workflow = load_document(Path::new(&path)).expect("the negative fixture loads and parses");

    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("a literal operator-acknowledgement must fail check");

    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        errors[0],
        willikins_core::CheckError::AcknowledgementLiteral {
            node: node("m7_bootstrap"),
            port: willikins_core::PortName::parse("acknowledged").unwrap(),
        }
    );
}

// ---------------------------------------------------------------------
// Milestone 3d, equivalence item 1: a characterization snapshot of every
// shipped document, committed before any production change (C1). After
// this commit, no later commit may change this snapshot except C7, which
// may only change the signing document's own entry (one fewer input, one
// edge growing ` -> AppleProfileName`, and the synthesized `identifier`
// example also naming the profile) and add one new entry for the new
// negative fixture. `docs/plans/2026-09-23-milestone-3d-conversions.md`,
// "Equivalence, and how it is proven".
// ---------------------------------------------------------------------

/// Every `workflows/*.yaml` and `workflows/fixtures/*.yaml` path, relative
/// to the workspace root, in sorted order. A single sort over both
/// directories' relative paths interleaves them exactly as the plan asks
/// (`workflows/fixtures/...` sorts after every top-level `workflows/*.yaml`
/// whose name is alphabetically before `fixtures`, and before any whose
/// name sorts after it -- the same order `ls` would show).
fn every_document_path() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for dir in ["workflows", "workflows/fixtures"] {
        let full = root.join(dir);
        let mut entries: Vec<_> = std::fs::read_dir(&full)
            .unwrap_or_else(|err| panic!("{}: {err}", full.display()))
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yaml"))
            .map(|entry| {
                format!(
                    "{dir}/{}",
                    entry.file_name().to_str().expect("utf8 file name")
                )
            })
            .collect();
        entries.sort();
        paths.extend(entries);
    }
    paths.sort();
    paths
}

/// Synthesize a value for a declared workflow input that has no default:
/// its type's registry example, parsed the same way a literal is. Used
/// only to produce a plannable input set for the characterization; it
/// says nothing about what any real caller would pass.
fn synthesized_input(ty: &willikins_core::TypeRef) -> Value {
    let entry = willikins_types::registry()
        .get(&ty.name)
        .unwrap_or_else(|| panic!("{ty}: declared input type is not registered"));
    if ty.list {
        Value::parse_list(ty, &[entry.info.example])
            .unwrap_or_else(|err| panic!("{ty}: example does not parse as a list element: {err}"))
    } else {
        Value::parse(ty, entry.info.example)
            .unwrap_or_else(|err| panic!("{ty}: own example does not parse as itself: {err}"))
    }
}

/// Every declared input of `workflow`, resolved from its own default when
/// it has one, else synthesized from its type's registry example.
fn synthesized_inputs(workflow: &willikins_core::Workflow) -> IndexMap<InputName, Value> {
    workflow
        .inputs
        .iter()
        .map(|(name, spec)| {
            let value = spec
                .default
                .clone()
                .unwrap_or_else(|| synthesized_input(&spec.ty));
            (name.clone(), value)
        })
        .collect()
}

/// One document's whole characterization: its load result, its check
/// result (every error, or every checked binding's type and a plan
/// against the fake catalog), rendered as plain text so an insta diff
/// reads directly as what changed.
fn characterize(path: &str) -> String {
    use std::fmt::Write as _;

    let mut report = String::new();
    writeln!(report, "=== {path} ===").unwrap();

    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    let workflow = match load_document(&full) {
        Ok(workflow) => workflow,
        Err(err) => {
            writeln!(report, "LOAD ERROR: {err}").unwrap();
            return report;
        }
    };

    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = match willikins_core::check(&workflow, &catalog) {
        Err(errors) => {
            writeln!(report, "CHECK ERRORS:").unwrap();
            for error in &errors {
                let json = serde_json::to_string(error).unwrap();
                writeln!(
                    report,
                    "- kind={} display={error} json={json}",
                    error.kind()
                )
                .unwrap();
            }
            return report;
        }
        Ok(checked) => checked,
    };

    writeln!(report, "WARNINGS:").unwrap();
    for warning in &checked.warnings {
        writeln!(report, "- {warning}").unwrap();
    }

    writeln!(report, "TYPES:").unwrap();
    for (node, ports) in &checked.types {
        for (port, edge) in ports {
            // The binding's own type, exactly what `Checked::types` held
            // before milestone 3d; ` -> <to>` only when `check` recorded
            // a conversion on the edge (equivalence item 1).
            let ty = edge.ty();
            match edge.conversion() {
                None => writeln!(report, "{node}.{port}: {ty}").unwrap(),
                Some(conversion) => {
                    writeln!(report, "{node}.{port}: {ty} -> {}", conversion.to()).unwrap();
                }
            }
        }
    }

    writeln!(report, "OUTPUTS:").unwrap();
    for (name, ty) in &checked.output_types {
        writeln!(report, "{name}: {ty}").unwrap();
    }

    writeln!(report, "PLAN:").unwrap();
    let inputs = synthesized_inputs(&workflow);
    match willikins_core::plan(&checked, &inputs, &catalog) {
        Err(err) => {
            writeln!(report, "PLAN ERROR: {err}").unwrap();
        }
        Ok(plan) => {
            let plan_json = serde_json::to_string(&plan).unwrap();
            let fingerprint_json = serde_json::to_string(&plan.fingerprint()).unwrap();
            writeln!(report, "plan_json: {plan_json}").unwrap();
            writeln!(report, "fingerprint: {fingerprint_json}").unwrap();
        }
    }

    report
}

/// The full characterization, one document at a time in sorted path
/// order. See the module doc above: after this commit, only C7 may move
/// this snapshot, and only in the reviewed ways.
#[test]
fn characterization_of_every_document() {
    let mut report = String::new();
    for path in every_document_path() {
        report.push_str(&characterize(&path));
        report.push('\n');
    }
    insta::assert_snapshot!(report);
}
