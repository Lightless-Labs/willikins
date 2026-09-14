//! `willikins-server`: the MCP server binary.
//!
//! `serve --stdio` runs the tools `crate::mcp` defines over stdio,
//! against the live catalog by default or, with `--fake`, an empty fake
//! one for local exploration -- a decision this binary makes that the
//! plan itself does not (recorded in the task report, not the plan).
//! Streamable HTTP (`--http`) is the next step's; this binary does not
//! implement it yet.
//!
//! Configuration comes from the environment (`ServerConfig::from_vars`,
//! plus each provider crate's own `credential_from_env` for the two
//! provisioning credentials in live mode) -- never a command-line flag,
//! so a credential can never appear in `ps` output or a shell history.
//! Any [`StartError`] refuses to start with a distinct message on stderr
//! and exit code 2; nothing is journaled on a startup refusal.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};

use willikins_journal::{Clock, FileJournal, JournalError, PrincipalId, SystemClock};
use willikins_server::{Butler, ButlerConfig, ConfigError, SharedJournal, StartupError};

#[derive(Parser)]
#[command(name = "willikins-server", version, about = "The willikins MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server.
    Serve {
        /// Serve over stdio. The only transport this binary implements
        /// so far -- the spec's own rule for a local, stdio-transported
        /// server is no authentication, credentials from the
        /// environment.
        #[arg(long)]
        stdio: bool,
        /// The principal every tool call over stdio runs as.
        #[arg(long, default_value = "local")]
        principal: String,
        /// Serve the empty fake catalog instead of the live one --
        /// local exploration; no request reaches a real provider.
        #[arg(long)]
        fake: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            stdio,
            principal,
            fake,
        } => cmd_serve(stdio, &principal, fake),
    }
}

/// Why the server refused to start. Never carries a credential's value:
/// every variant either names a variable only (through the wrapped
/// error types, each of which already follows that rule) or is built
/// from an already-redacted `Display`.
#[derive(Debug, thiserror::Error)]
enum StartError {
    /// `--principal` is not a valid [`PrincipalId`].
    #[error("--principal: {0}")]
    Principal(willikins_types::ParseError),
    /// A required or malformed environment variable, from
    /// [`willikins_server::ServerConfig::from_vars`].
    #[error("{0}")]
    Config(#[from] ConfigError),
    /// A provisioning credential (`WILLIKINS_GITHUB_TOKEN` or
    /// `WILLIKINS_DOPPLER_TOKEN`) is missing or does not look like a
    /// valid token for its provider.
    #[error("{0}")]
    Credential(String),
    /// The journal file at the configured path could not be opened.
    #[error("journal at {path}: {source}")]
    Journal {
        /// The configured path.
        path: PathBuf,
        /// The underlying failure.
        source: JournalError,
    },
    /// The trusted workflow directory failed startup validation -- see
    /// [`StartupError`] for what it names.
    #[error("{0}")]
    Startup(#[from] StartupError),
}

fn cmd_serve(stdio: bool, principal: &str, fake: bool) -> ExitCode {
    if !stdio {
        eprintln!(
            "willikins-server serve: pass --stdio (the only transport this binary implements so far)"
        );
        return ExitCode::from(2);
    }
    let principal = match PrincipalId::parse(principal) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("{}", StartError::Principal(error));
            return ExitCode::from(2);
        }
    };
    if fake {
        eprintln!(
            "willikins-server: serving the fake catalog (--fake); no request reaches a real provider"
        );
    }
    let butler = match build_butler(fake) {
        Ok(butler) => butler,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start the async runtime: {error}");
            return ExitCode::from(2);
        }
    };
    match runtime.block_on(willikins_server::serve_stdio(Arc::new(butler), principal)) {
        Ok(()) => ExitCode::from(0),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

/// Build a [`Butler`] from the environment: [`ServerConfig::from_vars`]
/// for the trusted directory, journal path, and windows/rates, the live
/// catalog (via each provider crate's own `credential_from_env`) unless
/// `fake`, and [`Butler::start`] for the trusted-directory validation.
fn build_butler(fake: bool) -> Result<Butler, StartError> {
    let config = willikins_server::ServerConfig::from_vars(|name| std::env::var(name).ok())?;

    let catalog = if fake {
        let (_state, catalog) = Butler::fake_catalog();
        catalog
    } else {
        let github = willikins_providers_github::credential_from_env()
            .map_err(|error| StartError::Credential(error.to_string()))?;
        let doppler = willikins_providers_doppler::credential_from_env()
            .map_err(|error| StartError::Credential(error.to_string()))?;
        Butler::live_catalog(github, doppler)
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let journal: SharedJournal = Arc::new(Mutex::new(
        FileJournal::open_with_clock(&config.journal_path, clock.clone()).map_err(|source| {
            StartError::Journal {
                path: config.journal_path.clone(),
                source,
            }
        })?,
    ));

    Butler::start(ButlerConfig {
        workflows_dir: config.workflows_dir,
        journal,
        catalog,
        clock,
        approval_window: config.approval_window,
        apply_window: config.apply_window,
        plan_rate_per_minute: config.plan_rate_per_minute,
        read_rate_per_minute: config.read_rate_per_minute,
    })
    .map_err(StartError::from)
}
