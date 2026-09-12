//! `willikins`: validate, describe, and plan a workflow document against
//! the milestone 1 fake providers.
//!
//! Every subcommand follows the same pipeline: `load_document` ->
//! `check` -> (`describe`) -> (`plan`). A [`willikins_dsl::DocumentError`]
//! or another I/O failure reading a file exits 2 with the error on
//! stderr; a [`willikins_core::CheckError`] list, a [`describe`]
//! [`Description`](willikins_core::Description) with missing or invalid
//! inputs, or a [`willikins_core::PlanError`] all exit 1 with the problem
//! printed to stdout; success exits 0.
//!
//! Text rendering never touches a [`willikins_core::Value`] directly —
//! see `render`'s module docs.

mod render;

use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};

use willikins_core::describe::{InputArg, PartialInputs, RawInput};
use willikins_core::{Catalog, Checked, Reported};
use willikins_dsl::DocumentError;
use willikins_providers_fake::FakeState;
use willikins_types::DomainType;

#[derive(Parser)]
#[command(name = "willikins", version, about = "A provisioning butler")]
struct Cli {
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and statically check a workflow document.
    Validate {
        /// Path to the workflow YAML.
        file: String,
    },
    /// Report which inputs a workflow still needs, given the ones provided.
    Describe {
        /// Path to the workflow YAML.
        file: String,
        /// Inputs as `name=value`. A value for a `list<T>`-typed input is
        /// comma-separated.
        #[arg(long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<InputArg>,
    },
    /// Compute a plan against observed state without applying anything.
    Plan {
        /// Path to the workflow YAML.
        file: String,
        /// Inputs as `name=value`. A value for a `list<T>`-typed input is
        /// comma-separated.
        #[arg(long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<InputArg>,
        /// A JSON file seeding the fake providers' state.
        #[arg(long = "fake-state", value_name = "FILE")]
        fake_state: Option<String>,
    },
    /// Print the JSON schema of the document format or the tool catalog.
    Schema {
        /// Print the workflow document schema.
        #[arg(long, conflicts_with = "catalog")]
        document: bool,
        /// Print the tool and type catalog.
        #[arg(long)]
        catalog: bool,
    },
    /// Propose a project slug from a free-form display name.
    ProposeSlug {
        /// The display name to derive a slug from.
        name: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate { file } => cmd_validate(&file, cli.json),
        Command::Describe { file, inputs } => cmd_describe(&file, &inputs, cli.json),
        Command::Plan {
            file,
            inputs,
            fake_state,
        } => cmd_plan(&file, &inputs, fake_state.as_deref(), cli.json),
        Command::Schema { document, catalog } => cmd_schema(document, catalog),
        Command::ProposeSlug { name } => cmd_propose_slug(&name, cli.json),
    }
}

/// Load `file` as a workflow document, printing a [`DocumentError`] to
/// stderr and returning `None` (the caller exits 2) on failure.
fn load_workflow(file: &str, json: bool) -> Result<willikins_core::Workflow, ExitCode> {
    willikins_dsl::load_document(Path::new(file)).map_err(|err| {
        print_document_error(&err, json);
        ExitCode::from(2)
    })
}

fn print_document_error(err: &DocumentError, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::to_string(err).unwrap_or_else(|_| err.to_string())
        );
    } else {
        eprintln!("{err}");
    }
}

/// Load and check `file` against `catalog`. On a [`DocumentError`], prints
/// it to stderr and returns `Err(ExitCode::from(2))`. On [`CheckError`]s,
/// prints them (text or JSON, per `json`) to stdout and returns
/// `Err(ExitCode::from(1))`.
fn load_and_check(file: &str, catalog: &Catalog, json: bool) -> Result<Checked, ExitCode> {
    let workflow = load_workflow(file, json)?;
    willikins_core::check(&workflow, catalog).map_err(|errors| {
        if json {
            println!("{}", render::check_errors_json(&errors));
        } else {
            println!("{}", render::check_errors_text(&errors));
        }
        ExitCode::from(1)
    })
}

/// Build [`PartialInputs`] from the raw `--input` arguments, splitting a
/// value on commas exactly when `checked`'s declared input is list-typed —
/// an argument naming an input `checked` does not declare is passed
/// through as a scalar, since [`willikins_core::describe`] only needs its
/// name to report it as unrecognised.
///
/// Two arguments naming the same input are refused (exit 2, on stderr).
/// [`PartialInputs`] is keyed by name, so the second used to overwrite the
/// first silently and the run continued against a value the caller never
/// meant to send. The error names the input but never echoes either
/// value: which one was dropped is not the point, and the caller has both
/// in hand. Adversarial pass 2, finding 4.
fn build_partial_inputs(checked: &Checked, args: &[InputArg]) -> Result<PartialInputs, ExitCode> {
    let mut partial = PartialInputs::new();
    for arg in args {
        let raw = match checked.workflow.inputs.get(&arg.name) {
            Some(spec) if spec.ty.list => RawInput::from_comma_separated(&arg.raw),
            _ => arg.value(),
        };
        if partial.insert(arg.name.clone(), raw).is_some() {
            eprintln!(
                "--input `{}` was given more than once; supply each input at most once",
                arg.name
            );
            return Err(ExitCode::from(2));
        }
    }
    Ok(partial)
}

fn cmd_validate(file: &str, json: bool) -> ExitCode {
    let (_state, catalog) = willikins_providers_fake::empty();
    match load_and_check(file, &catalog, json) {
        Ok(checked) => {
            if json {
                println!("{}", render::check_warnings_json(&checked.warnings));
            } else {
                let text = render::check_warnings_text(&checked.warnings);
                if !text.is_empty() {
                    println!("{text}");
                }
            }
            ExitCode::from(0)
        }
        Err(code) => code,
    }
}

fn print_description(description: &willikins_core::Description, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(description).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("{}", render::describe_text(description));
    }
}

fn cmd_describe(file: &str, inputs: &[InputArg], json: bool) -> ExitCode {
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = match load_and_check(file, &catalog, json) {
        Ok(checked) => checked,
        Err(code) => return code,
    };
    let partial = match build_partial_inputs(&checked, inputs) {
        Ok(partial) => partial,
        Err(code) => return code,
    };
    let description = willikins_core::describe(&checked, &partial);
    let ok = description.errors.is_empty() && description.missing.is_empty();
    print_description(&description, json);
    if ok {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

/// Build the fake-provider [`Catalog`] for `plan`: an empty state, or one
/// seeded from `fake_state_path`. A missing or malformed seed file exits 2,
/// the same as any other input the CLI cannot make sense of before it ever
/// reaches `check`.
fn build_fake_catalog(fake_state_path: Option<&str>) -> Result<Catalog, ExitCode> {
    let state = match fake_state_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path).map_err(|err| {
                eprintln!("{path}: failed to read fake state: {err}");
                ExitCode::from(2)
            })?;
            FakeState::from_json(&contents).map_err(|err| {
                eprintln!("{path}: invalid fake state: {err}");
                ExitCode::from(2)
            })?
        }
        None => FakeState::new(),
    };
    Ok(willikins_providers_fake::catalog(Arc::new(Mutex::new(
        state,
    ))))
}

fn cmd_plan(file: &str, inputs: &[InputArg], fake_state: Option<&str>, json: bool) -> ExitCode {
    let catalog = match build_fake_catalog(fake_state) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let checked = match load_and_check(file, &catalog, json) {
        Ok(checked) => checked,
        Err(code) => return code,
    };

    let partial = match build_partial_inputs(&checked, inputs) {
        Ok(partial) => partial,
        Err(code) => return code,
    };
    let description = willikins_core::describe(&checked, &partial);
    if !description.errors.is_empty() || !description.missing.is_empty() {
        print_description(&description, json);
        return ExitCode::from(1);
    }

    match willikins_core::plan(&checked, &description.resolved, &catalog) {
        Ok(plan) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plan).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                println!("{}", render::plan_text(&plan));
            }
            ExitCode::from(0)
        }
        Err(err) => {
            if json {
                // Through `Reported`, so the plan error carries `message`
                // (its own `Display`) alongside the `kind` its internal tag
                // gives it -- the one object shape every error an agent
                // reads has, the same one `render::check_errors_json` emits.
                println!(
                    "{}",
                    serde_json::to_string_pretty(&Reported::new(&err))
                        .unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                println!("{err}");
            }
            ExitCode::from(1)
        }
    }
}

fn cmd_schema(document: bool, catalog: bool) -> ExitCode {
    if document {
        let schema = willikins_dsl::document_schema();
        println!(
            "{}",
            serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string())
        );
        ExitCode::from(0)
    } else if catalog {
        let (_state, catalog) = willikins_providers_fake::empty();
        println!(
            "{}",
            serde_json::to_string_pretty(&catalog.list_tools_json())
                .unwrap_or_else(|_| "{}".to_string())
        );
        ExitCode::from(0)
    } else {
        eprintln!("schema: pass --document or --catalog");
        ExitCode::from(2)
    }
}

fn cmd_propose_slug(name: &str, json: bool) -> ExitCode {
    let project_name = match willikins_types::ProjectName::parse(name) {
        Ok(name) => name,
        Err(err) => {
            if json {
                println!("{}", serde_json::to_string(&err).unwrap_or_default());
            } else {
                println!("{err}");
            }
            return ExitCode::from(1);
        }
    };
    match willikins_types::propose_slug(&project_name) {
        Ok(slug) => {
            println!("{slug}");
            ExitCode::from(0)
        }
        Err(err) => {
            if json {
                println!("{}", serde_json::json!({ "error": err.to_string() }));
            } else {
                println!("{err}");
            }
            ExitCode::from(1)
        }
    }
}
