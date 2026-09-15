//! `willikins serve` / `willikins-server serve`: one implementation both
//! binaries' `main`s call, so there is exactly one `serve` -- one flag
//! set, one set of startup refusals with their exact messages and exit
//! code, one dispatch to the stdio or HTTP transport -- for the plan's
//! own words on the CLI's own `serve`: "the `willikins-server` binary's
//! entry point re-exported so there is one `willikins` binary."
//!
//! Configuration comes from the environment (`ServerConfig::from_vars`,
//! plus each provider crate's own `credential_from_env` for the two
//! provisioning credentials in live mode) -- never a command-line flag,
//! so a credential can never appear in `ps` output or a shell history.
//! Any startup failure refuses with a distinct message on stderr and
//! exit code 2; nothing is journaled on a startup refusal.
//!
//! **Where the two binaries may differ: nothing.** [`run_serve`] takes
//! only a [`ServeArgs`] and reads the process environment itself, so a
//! call from `willikins-server`'s `main` and one from `willikins`'s
//! `main` (task 11) behave identically for identical arguments and
//! environment. The two binaries' `main`s differ only in their own
//! top-level `clap::Command` -- the binary name and `--version`/`--help`
//! text `clap` derives from it, and the fact that `willikins` has sibling
//! subcommands (`apply`, `approve`, `plan`, ...) this one does not --
//! never in what `serve` itself does once dispatched. `willikins-server`'s
//! own process-level tests (`tests/binary_startup.rs`, `tests/http_server.rs`)
//! exercise exactly this function through that binary and stay green
//! unchanged by this move.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, FileJournal, JournalError, PrincipalId, SystemClock};

use crate::{
    Butler, ButlerConfig, ConfigError, HttpConfig, HttpConfigError, ServerConfig, SharedJournal,
    StartupError, WillikinsHandler,
};

/// `serve`'s own command-line arguments, as a `clap::Args`-deriving
/// struct so both binaries' top-level `Command` enums can hold one
/// `Serve(ServeArgs)` variant (clap flattens a single-field tuple
/// variant's `Args` type automatically) rather than each declaring the
/// same five flags by hand.
#[derive(clap::Args, Debug, Clone)]
pub struct ServeArgs {
    /// Serve over stdio: the spec's own rule for a local,
    /// stdio-transported server is no authentication, credentials
    /// from the environment. Exactly one of `--stdio`/`--http` is
    /// required.
    #[arg(long)]
    pub stdio: bool,
    /// Serve over Streamable HTTP: bearer-authenticated `/mcp`,
    /// `/healthz`, Basic-authenticated `/approvals`. Exactly one of
    /// `--stdio`/`--http` is required.
    #[arg(long)]
    pub http: bool,
    /// The address to bind in `--http` mode. When absent, binds
    /// `0.0.0.0:$PORT` (the `PORT` environment variable) --
    /// Railway's own convention. Meaningless with `--stdio`.
    #[arg(long)]
    pub bind: Option<String>,
    /// The principal every tool call over stdio runs as. Meaningless
    /// with `--http`, where each request's own bearer token
    /// determines its principal.
    #[arg(long, default_value = "local")]
    pub principal: String,
    /// Serve the empty fake catalog instead of the live one --
    /// local exploration; no request reaches a real provider.
    #[arg(long)]
    pub fake: bool,
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
    /// [`ServerConfig::from_vars`].
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
    /// `--bind` is not a valid socket address.
    #[error("--bind: {0}")]
    Bind(String),
    /// `--http` was given with no `--bind` and no `PORT` set.
    #[error("--http needs --bind <addr> or the PORT environment variable")]
    NoBindOrPort,
    /// One of http mode's own startup rules (see
    /// [`HttpConfig::build`]).
    #[error("{0}")]
    HttpConfig(#[from] HttpConfigError),
}

/// Run `serve`: parse `args`, load configuration from the process
/// environment, build a [`Butler`], and dispatch to the stdio or HTTP
/// transport, blocking until it exits. The one implementation both
/// `willikins-server`'s and `willikins`'s `main` call -- see the module
/// docs for why nothing differs between the two.
#[must_use]
pub fn run_serve(args: &ServeArgs) -> ExitCode {
    match (args.stdio, args.http) {
        (true, true) => {
            // No binary name in the message: this is shared by both
            // binaries' `serve` subcommands (see the module doc's "where
            // the two binaries may differ" answer), and hardcoding
            // `willikins-server` here would make it wrong for `willikins`.
            eprintln!("serve: pass exactly one of --stdio or --http, not both");
            ExitCode::from(2)
        }
        (false, false) => {
            eprintln!("serve: pass --stdio or --http");
            ExitCode::from(2)
        }
        (true, false) => cmd_serve_stdio(&args.principal, args.fake),
        (false, true) => cmd_serve_http(args.bind.as_deref(), args.fake),
    }
}

fn cmd_serve_stdio(principal: &str, fake: bool) -> ExitCode {
    let principal = match PrincipalId::parse(principal) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("{}", StartError::Principal(error));
            return ExitCode::from(2);
        }
    };
    let config = match ServerConfig::from_vars(|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{}", StartError::from(error));
            return ExitCode::from(2);
        }
    };
    let butler = match build_butler(fake, config) {
        Ok(butler) => butler,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let mut handler = WillikinsHandler::new(Arc::new(butler), principal);
    if fake {
        // `--fake` is a decision this binary makes that the plan does not
        // (see the module doc); the server's own `initialize` response
        // says so via this sentence, rather than only a stderr line a
        // caller talking MCP over the pipe would never see.
        handler = handler.with_fake_catalog_note();
    }
    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match runtime.block_on(crate::serve_stdio_handler(handler)) {
        Ok(()) => ExitCode::from(0),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn cmd_serve_http(bind: Option<&str>, fake: bool) -> ExitCode {
    // JSON to stderr, never a body or header value (trust boundary 5):
    // the journal is the audit source of truth, this is only for an
    // operator watching the process. `try_init` rather than `init`
    // since a repeated call (this crate's own tests build multiple
    // routers in one process) would otherwise panic.
    let _ = tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .try_init();
    let config = match ServerConfig::from_vars(|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{}", StartError::from(error));
            return ExitCode::from(2);
        }
    };
    let http_config = match build_http_config(&config, bind, fake) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let butler = match build_butler(fake, config) {
        Ok(butler) => butler,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match runtime.block_on(crate::serve_http(Arc::new(butler), http_config)) {
        Ok(()) => ExitCode::from(0),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn build_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to start the async runtime: {error}"))
}

/// Resolve `--http`'s bind address (`--bind`, or `0.0.0.0:$PORT`) and the
/// three http-mode startup rules ([`HttpConfig::build`]), including that
/// `WILLIKINS_APPROVER_TOKEN_HASH` is actually set (a fourth rule
/// `HttpConfig::build` itself cannot enforce, since it takes an
/// already-resolved [`crate::TokenHash`], not an `Option`) -- reusing
/// [`ConfigError::Missing`]'s own existing shape and message rather than
/// adding a new variant just for this.
fn build_http_config(
    config: &ServerConfig,
    bind: Option<&str>,
    fake: bool,
) -> Result<HttpConfig, StartError> {
    let bind = if let Some(addr) = bind {
        addr.parse::<SocketAddr>()
            .map_err(|error| StartError::Bind(error.to_string()))?
    } else {
        let port = config.port.ok_or(StartError::NoBindOrPort)?;
        SocketAddr::from(([0, 0, 0, 0], port))
    };
    let approver_hash = config.approver_token_hash.ok_or_else(|| {
        StartError::from(ConfigError::Missing {
            variable: "WILLIKINS_APPROVER_TOKEN_HASH",
        })
    })?;
    let http_config = HttpConfig::build(
        bind,
        config.agent_token_hashes.clone(),
        approver_hash,
        config.allowed_hosts.clone(),
    )
    .map_err(StartError::from)?;
    // `--fake` says so in `initialize`'s own `instructions`, over this
    // transport exactly as over stdio -- see
    // `HttpConfig::announcing_fake_catalog`.
    Ok(if fake {
        http_config.announcing_fake_catalog()
    } else {
        http_config
    })
}

/// Build a [`Butler`] from `config` (already loaded, so both transports'
/// `cmd_serve_*` share one [`ServerConfig::from_vars`] call): the live
/// catalog (via each provider crate's own `credential_from_env`) unless
/// `fake`, and [`Butler::start`] for the trusted-directory validation.
fn build_butler(fake: bool, config: ServerConfig) -> Result<Butler, StartError> {
    let catalog = if fake {
        let (_state, catalog) = Butler::fake_catalog();
        catalog
    } else {
        crate::live_catalog_from_env().map_err(|error| StartError::Credential(error.to_string()))?
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
