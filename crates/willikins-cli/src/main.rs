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

mod commands;
mod render;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use willikins_core::describe::{InputArg, PartialInputs, RawInput};
use willikins_core::{Catalog, Checked, Reported};
use willikins_dsl::DocumentError;
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
        /// A JSON file seeding the fake providers' state (see
        /// `FakeState`). Read once, to build the starting state; never
        /// written back to. A `FakeState` dump (its own `Serialize`) is a
        /// one-way, redacted view for inspection, not an export/import
        /// round trip: a seeded secret reserializes as its redaction
        /// marker, never its bytes, and a seeded `next_token` marker is
        /// not itself a valid token, so reloading such a dump fails.
        /// Mutually exclusive with `--live`.
        #[arg(long = "fake-state", value_name = "FILE")]
        fake_state: Option<String>,
        /// Plan against the real providers, with credentials from
        /// `WILLIKINS_GITHUB_TOKEN` and `WILLIKINS_DOPPLER_TOKEN`. Without
        /// it, the fake providers.
        #[arg(long)]
        live: bool,
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
    /// Plan and apply a workflow document (or apply one already planned
    /// by an earlier `apply`/`plan`, named by `--plan-id`).
    Apply(commands::ApplyArgs),
    /// Grant a pending plan's approval.
    Approve(commands::ApproveArgs),
    /// Reject a pending plan.
    Reject(commands::RejectArgs),
    /// List every recorded run in a file journal.
    Runs(commands::RunsArgs),
    /// Show one recorded run from a file journal.
    Run(commands::RunArgs),
    /// Run the MCP server: `willikins serve --stdio | --http --bind <addr>
    /// [--fake] [--principal <id>]`. The same implementation
    /// `willikins-server serve` calls -- see
    /// `willikins_server::cli::run_serve`'s own docs.
    Serve(willikins_server::cli::ServeArgs),
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
            live,
        } => cmd_plan(&file, &inputs, fake_state.as_deref(), live, cli.json),
        Command::Schema { document, catalog } => cmd_schema(document, catalog),
        Command::ProposeSlug { name } => cmd_propose_slug(&name, cli.json),
        Command::Apply(args) => commands::cmd_apply(&args, cli.json),
        Command::Approve(args) => commands::cmd_approve(&args, cli.json),
        Command::Reject(args) => commands::cmd_reject(&args, cli.json),
        Command::Runs(args) => commands::cmd_runs(&args, cli.json),
        Command::Run(args) => commands::cmd_run(&args, cli.json),
        Command::Serve(args) => willikins_server::cli::run_serve(&args),
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

/// Print `err` to stderr, as JSON or as one line of text.
///
/// A document error's message quotes the document that failed — a YAML
/// scalar, a field name — so it is document text (trust boundary 4), and a
/// field name is text no domain type ever parses: nothing downstream can
/// bound it. Rendered raw, a field name carrying a newline printed a second
/// line that read like one of willikins' own `check` errors, so the text
/// form goes through [`render::single_line`]. The JSON form needs no help:
/// a string cannot escape its field.
fn print_document_error(err: &DocumentError, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::to_string(err).unwrap_or_else(|_| err.to_string())
        );
    } else {
        eprintln!("{}", render::single_line(&err.to_string()));
    }
}

/// Load and check `file` against `catalog`. On a [`DocumentError`], prints
/// it to stderr and returns `Err(ExitCode::from(2))`. On [`CheckError`]s,
/// prints them (text or JSON, per `json`) to stdout and returns
/// `Err(ExitCode::from(1))`.
///
/// `pub(crate)`: `commands::cmd_apply` shares this exact pipeline for the
/// document/check half of `apply <file>`, so a document error or a check
/// failure gives the same exit code and message whether it was hit by
/// `plan`/`validate`/`describe` or by `apply`.
pub(crate) fn load_and_check(
    file: &str,
    catalog: &Catalog,
    json: bool,
) -> Result<Checked, ExitCode> {
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
///
/// `pub(crate)`: shared with `commands::cmd_apply`'s own `--input`
/// handling.
pub(crate) fn build_partial_inputs(
    checked: &Checked,
    args: &[InputArg],
) -> Result<PartialInputs, ExitCode> {
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

fn cmd_plan(
    file: &str,
    inputs: &[InputArg],
    fake_state: Option<&str>,
    live: bool,
    json: bool,
) -> ExitCode {
    // Shared with `apply`'s own catalog construction (task 11): the same
    // `--live`/`--fake-state` semantics, so the two subcommands can never
    // silently drift apart on which providers a given flag combination
    // selects. `plan` never seeds `--fake-state-out`, so the `FakeState`
    // handle this also returns is simply dropped here.
    let (catalog, _fake_state) = match commands::build_catalog(live, fake_state, json) {
        Ok(built) => built,
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
                // Not `{err}`: a `PlanError` can carry a rendered item as
                // a key, which a document shaped. See
                // `render::plan_error_text`.
                println!("{}", render::plan_error_text(&err));
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

/// `propose-slug`'s JSON output is aligned with
/// `willikins_server::Butler::propose_slug`'s own shape (todo item 4 of
/// `todos/2026-09-12-error-json-uniformity-gaps.md`): success is
/// `{"slug": "..."}`, matching [`willikins_server::ProposeSlugResponse`];
/// a failure is built as the same [`willikins_server::ButlerError`]
/// variant `Butler::propose_slug` itself would return
/// (`InvalidProjectName`/`SlugProposal`) and printed through [`Reported`],
/// so the CLI's `--json propose-slug` and the MCP `propose_slug` tool's
/// error carry the same `kind`. No `Butler` is built for this: both
/// variants are constructed directly from the same parse this subcommand
/// already ran, exactly as `Butler::propose_slug_inner` does internally.
fn cmd_propose_slug(name: &str, json: bool) -> ExitCode {
    let project_name = match willikins_types::ProjectName::parse(name) {
        Ok(name) => name,
        Err(error) => {
            return propose_slug_failure(
                &willikins_server::ButlerError::InvalidProjectName { error },
                json,
            );
        }
    };
    match willikins_types::propose_slug(&project_name) {
        Ok(slug) => {
            if json {
                println!("{}", serde_json::json!({ "slug": slug.to_string() }));
            } else {
                println!("{slug}");
            }
            ExitCode::from(0)
        }
        Err(error) => {
            propose_slug_failure(&willikins_server::ButlerError::SlugProposal { error }, json)
        }
    }
}

fn propose_slug_failure(error: &willikins_server::ButlerError, json: bool) -> ExitCode {
    if json {
        println!(
            "{}",
            serde_json::to_string(&Reported::new(error)).unwrap_or_default()
        );
    } else {
        println!("{error}");
    }
    ExitCode::from(1)
}
