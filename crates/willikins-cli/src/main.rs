//! The willikins command line. Each subcommand mirrors a milestone 2 MCP tool.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "willikins", version, about = "A provisioning butler")]
struct Cli {
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
        /// Inputs as `name=value`.
        #[arg(long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<String>,
    },
    /// Compute a plan against observed state without applying anything.
    Plan {
        /// Path to the workflow YAML.
        file: String,
        /// Inputs as `name=value`.
        #[arg(long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<String>,
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let name = match cli.command {
        Command::Validate { .. } => "validate",
        Command::Describe { .. } => "describe",
        Command::Plan { .. } => "plan",
        Command::Schema { .. } => "schema",
    };
    eprintln!("willikins {name}: not implemented yet");
    ExitCode::from(2)
}
