//! `willikins-server`: the MCP server binary.
//!
//! A thin wrapper: this binary's own top-level `clap::Command` (name
//! `willikins-server`) parses argv, and `serve` dispatches straight to
//! [`willikins_server::cli::run_serve`], the one implementation the
//! `willikins` binary's own `serve` subcommand (task 11) also calls --
//! see that module's docs for everything `serve` itself does, and for
//! why nothing about its behavior may differ between the two binaries.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use willikins_server::cli::{ServeArgs, run_serve};

#[derive(Parser)]
#[command(name = "willikins-server", version, about = "The willikins MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server.
    Serve(ServeArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => run_serve(&args),
    }
}
