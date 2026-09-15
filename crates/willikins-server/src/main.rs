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

use willikins_server::cli::run_hash_token;

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server.
    Serve(ServeArgs),
    /// Read one token from stdin and print its SHA-256 as 64 lower-case
    /// hex characters -- the form `WILLIKINS_AGENT_TOKEN_HASHES` and
    /// `WILLIKINS_APPROVER_TOKEN_HASH` expect. The token is never a
    /// command-line argument, only stdin, so it never appears in `ps`
    /// output or a shell history.
    HashToken,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => run_serve(&args),
        Command::HashToken => run_hash_token(),
    }
}
