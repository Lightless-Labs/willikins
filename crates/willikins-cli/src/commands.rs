//! Task 11: `apply`, `approve`, `reject`, `runs`, and `run` -- the CLI's
//! own surface over [`willikins_server::Butler`]. `serve` needs no module
//! of its own: `main.rs` calls `willikins_server::cli::run_serve` directly,
//! since that function already is the whole of `serve`'s behaviour (see
//! its own module docs for why nothing differs between the two binaries).
//!
//! # `apply <file>` runs in an isolated temporary directory
//!
//! `willikins_server::Butler` only ever resolves a workflow by *name* in
//! a trusted directory it scans as a whole (`Butler::start` refuses a
//! symlink and any document that fails to parse or `check`, naming the
//! first one it finds) -- so `apply <file>`, which the plan's own trust
//! boundaries let point at an arbitrary path a human already holds the
//! machine and credentials for, cannot simply point a `Butler` at
//! `file`'s own parent directory: `workflows/fixtures/` alone holds about
//! thirty documents that fail `check` on purpose, and a real trusted
//! directory `Butler::start` would refuse outright.
//!
//! The fix is the same one `willikins-server`'s own 10a tests use
//! (`copy_fixture_as`): copy the *one* named file's bytes into a private
//! [`tempfile::TempDir`], under `<document's own name>.yaml` -- not
//! whatever `file` was originally called, so a fixture such as
//! `workflows/fixtures/irreversible.yaml` (whose own `name:` is
//! `new-rust-service-irreversible`, not `irreversible`) still applies --
//! and start a fresh `Butler` there. The directory, and everything in it,
//! is gone when the command returns.
//!
//! `apply --plan-id <id> --workflows-dir <dir>` is different: `<dir>` is
//! a real trusted directory an operator names on purpose (`workflows` by
//! default), so this mode runs `Butler::start` directly against it,
//! validating every document in it -- including refusing with
//! [`willikins_server::StartupError::NameMismatch`], naming both the
//! filename-derived and the document's own name, exactly as the real
//! server does at startup.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::Reported;
use willikins_core::describe::InputArg;
use willikins_journal::{
    Clock, FileJournal, MemoryJournal, PlanId, PrincipalId, Reason, RunId, RunRecord, RunState,
    SystemClock,
};
use willikins_providers_fake::FakeState;
use willikins_server::{Butler, ButlerConfig, ButlerError, PlanResponse, SharedJournal};
use willikins_types::WorkflowName;

use crate::render;

// ---------------------------------------------------------------------
// clap argument shapes
// ---------------------------------------------------------------------

/// `apply`'s two modes: a file to plan-and-apply in one step, or
/// `--plan-id` to apply a plan an earlier `apply`/`plan` (and, usually, a
/// separate `approve`) already recorded. See the module docs for why each
/// mode picks the `Butler` constructor it does.
#[derive(clap::Args, Debug)]
pub struct ApplyArgs {
    /// Path to the workflow YAML. Mutually exclusive with `--plan-id`;
    /// exactly one of the two is required.
    pub file: Option<String>,
    /// Inputs as `name=value`. A value for a `list<T>`-typed input is
    /// comma-separated. Only valid with `file`: a plan named by
    /// `--plan-id` already has its inputs resolved and recorded.
    #[arg(long = "input", value_name = "NAME=VALUE")]
    pub inputs: Vec<InputArg>,
    /// A JSON file seeding the fake providers' state, as `plan --fake-state`
    /// does. Mutually exclusive with `--live`.
    #[arg(long = "fake-state", value_name = "FILE")]
    pub fake_state: Option<String>,
    /// After the run reaches a final state, dump the fake providers'
    /// (redacted) ending state to this file -- so a later `apply` given
    /// the same file as `--fake-state` sees what this one left behind.
    /// Fake state otherwise never persists across two CLI invocations,
    /// each of which builds its own `FakeState` from scratch. Only valid
    /// with `file` and without `--live`.
    #[arg(long = "fake-state-out", value_name = "FILE")]
    pub fake_state_out: Option<String>,
    /// Real providers, credentials from `WILLIKINS_GITHUB_TOKEN` and
    /// `WILLIKINS_DOPPLER_TOKEN`. Without it, the fake providers.
    #[arg(long)]
    pub live: bool,
    /// When the freshly planned work needs human approval, grant it as
    /// `--principal` before applying -- self-approval, by the plan's own
    /// design for the CLI's local operator. Only valid with `file`: a
    /// plan named by `--plan-id` is expected to already be decided
    /// (`willikins approve`), not decided in the same breath as applied.
    #[arg(long)]
    pub approve: bool,
    /// Where to record this run. Defaults to an in-memory journal, gone
    /// once this process exits -- fine for a one-shot `file` apply, but
    /// the only way `--plan-id` (a *different* process from the one that
    /// planned it) can ever see the plan at all.
    #[arg(long)]
    pub journal: Option<String>,
    /// The principal this command runs as: both the plan's requester and,
    /// with `--approve`, its approver.
    #[arg(long, default_value = "local")]
    pub principal: String,
    /// Apply a plan recorded earlier, instead of planning `file` fresh.
    /// Requires `--journal`.
    #[arg(long = "plan-id", value_name = "ID")]
    pub plan_id: Option<String>,
    /// The trusted workflow directory `--plan-id` resolves its plan's
    /// workflow name against -- a real directory, validated as a whole
    /// (`Butler::start`), unlike `file`'s own private temporary one.
    /// Defaults to `workflows`. Only valid with `--plan-id`.
    #[arg(long = "workflows-dir", value_name = "DIR")]
    pub workflows_dir: Option<String>,
}

/// `approve <plan_id> --journal <path> [--principal <id>]`.
#[derive(clap::Args, Debug)]
pub struct ApproveArgs {
    /// The plan to approve.
    pub plan_id: String,
    /// The file journal holding it. Opened exclusively: a running server
    /// (or another CLI command) already holding this journal makes this
    /// command refuse, plainly, rather than wait.
    #[arg(long)]
    pub journal: String,
    /// The approving principal.
    #[arg(long, default_value = "local")]
    pub principal: String,
}

/// `reject <plan_id> --journal <path> --reason <text> [--principal <id>]`.
#[derive(clap::Args, Debug)]
pub struct RejectArgs {
    /// The plan to reject.
    pub plan_id: String,
    /// The file journal holding it. See [`ApproveArgs::journal`].
    #[arg(long)]
    pub journal: String,
    /// Why. Recorded in the journal; at most 256 characters.
    #[arg(long)]
    pub reason: String,
    /// The rejecting principal.
    #[arg(long, default_value = "local")]
    pub principal: String,
}

/// `runs --journal <path>`.
#[derive(clap::Args, Debug)]
pub struct RunsArgs {
    /// The file journal to read. Read-only ([`willikins_journal::replay`]):
    /// works alongside a running server or another CLI command that holds
    /// the journal's own exclusive lock.
    #[arg(long)]
    pub journal: String,
}

/// `run <run_id> --journal <path>`.
#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// The run to look up.
    pub run_id: String,
    /// The file journal to read. See [`RunsArgs::journal`].
    #[arg(long)]
    pub journal: String,
}

// ---------------------------------------------------------------------
// shared, kind-tagged CLI-local refusals
// ---------------------------------------------------------------------

/// Refusals this crate raises itself, around a `Butler` operation rather
/// than from one: a journal that will not open, an id that does not even
/// parse, or a `run`/`runs` lookup that found nothing. Kind-tagged the
/// same way every other error in this workspace is, so `--json` prints it
/// through [`Reported`] exactly like a [`ButlerError`].
/// **No variant here declares a field named `message`.** [`Reported`]
/// adds one of its own with `serde(flatten)`, and flatten resolves a
/// collision by writing *both* keys rather than refusing: the object
/// then carries `message` twice, which every JSON reader folds back to
/// one silently, with no rule saying which one survives. The same
/// invariant `willikins-core`'s own error enums keep (see
/// [`Reported`]'s docs); pinned end to end by
/// `tests/adversarial_11.rs`'s `assert_no_duplicate_keys`.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind")]
enum CliError {
    /// The journal at `path` could not be opened or read.
    Journal {
        /// The configured path.
        path: String,
        /// The underlying failure.
        error: String,
    },
    /// An argument that should have parsed as a [`PlanId`]/[`RunId`] did
    /// not.
    InvalidId {
        /// The offending argument, verbatim.
        argument: String,
        /// Why.
        error: String,
    },
    /// `run <id>` named no run this journal has recorded.
    UnknownRun {
        /// The unrecognised id.
        run_id: String,
    },
    /// An I/O failure this crate hit on its own account: a temporary
    /// directory, a `--fake-state`/`--fake-state-out` file, or copying
    /// the named document.
    Io {
        /// What went wrong.
        error: String,
    },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Journal { path, error } => write!(f, "{path}: {error}"),
            Self::InvalidId { argument, error } => write!(f, "`{argument}`: {error}"),
            Self::UnknownRun { run_id } => write!(f, "no run `{run_id}` is recorded"),
            Self::Io { error } => write!(f, "{error}"),
        }
    }
}

/// A plain flag-combination or malformed-argument refusal: stderr, exit
/// 2, no JSON form -- the same convention `willikins_server::cli::run_serve`
/// uses for `--stdio`/`--http` (this is a usage mistake, not a document
/// or plan result an agent ever parses).
fn usage_error(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(2)
}

/// Print `error` -- JSON through [`Reported`], text as one escaped line
/// -- to stderr and exit 2: the CLI's convention (see `main::load_workflow`'s
/// own `print_document_error`) for a failure that happens before a
/// document or plan can even be attempted: a journal that will not open,
/// a missing or malformed `--live` credential, a directory that fails
/// startup validation, or an I/O failure this crate hit on its own
/// account.
fn fail_config<E>(error: &E, json: bool) -> ExitCode
where
    E: serde::Serialize + std::fmt::Display,
{
    if json {
        eprintln!(
            "{}",
            serde_json::to_string(&Reported::new(error)).unwrap_or_else(|_| error.to_string())
        );
    } else {
        eprintln!("{}", render::single_line(&error.to_string()));
    }
    ExitCode::from(2)
}

/// A trusted directory that failed startup validation, reported the way
/// the MCP surface reports the same failure: wrapped in
/// [`ButlerError::Startup`], not as a bare [`willikins_server::StartupError`].
///
/// Two reasons, and they are the same reason twice. The `kind` an agent
/// reads is then `Startup` whether it came from this CLI or from the
/// `apply` tool over MCP, with the scan's own error nested under `error`
/// in both -- the parity the plan's acceptance test 11 asks for. And
/// nesting is what keeps `StartupError`'s own `message` field (on
/// `Directory`, `InvalidName` and `Journal`) off the top level, where
/// [`Reported`]'s added `message` would collide with it and emit the key
/// twice. The stream and exit code are unchanged: this is still a
/// configuration refusal, stderr and exit 2.
fn fail_startup(error: willikins_server::StartupError, json: bool) -> ExitCode {
    fail_config(&ButlerError::Startup { error }, json)
}

/// Print `error` -- JSON through [`Reported`], text as one escaped line
/// -- to stdout and exit 1: the CLI's convention (see `main::cmd_plan`'s
/// own handling of a [`willikins_core::PlanError`]) for a domain result
/// reached only after a document at least parsed and checked: a
/// `Butler` refusal, or an unrecognised run id.
fn fail_domain<E>(error: &E, json: bool) -> ExitCode
where
    E: serde::Serialize + std::fmt::Display,
{
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&Reported::new(error))
                .unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("{}", render::single_line(&error.to_string()));
    }
    ExitCode::from(1)
}

/// Like [`fail_domain`], for a [`ButlerError`] specifically: in text
/// mode, when `error` is [`ButlerError::ApprovalRequired`] and a
/// `plan_id` is in hand, also prints
/// [`render::approval_required_guidance`] -- `plan_id` is not itself a
/// field of `ButlerError` (see that enum's own doc), so only a caller
/// that still has it can add this -- and, when a `--journal` backs this
/// `butler`, [`render::pending_approvals_text`] for every plan it still
/// holds pending, not only the one just refused, so an operator can
/// decide the whole backlog in one pass.
fn fail_apply(
    error: &ButlerError,
    plan_id: Option<PlanId>,
    journal_path: Option<&str>,
    butler: &Butler,
    json: bool,
) -> ExitCode {
    if json {
        return fail_domain(error, json);
    }
    println!("{}", render::single_line(&error.to_string()));
    if let (ButlerError::ApprovalRequired { .. }, Some(plan_id)) = (error, plan_id) {
        println!(
            "{}",
            render::approval_required_guidance(plan_id, journal_path)
        );
        if journal_path.is_some() {
            let pending = butler.pending_approvals();
            if !pending.is_empty() {
                println!("pending approvals:");
                println!("{}", render::pending_approvals_text(&pending));
            }
        }
    }
    ExitCode::from(1)
}

// ---------------------------------------------------------------------
// catalog / journal construction, shared with `main::cmd_plan`
// ---------------------------------------------------------------------

/// A built catalog, plus the seeded [`FakeState`] handle behind it when
/// it is the fake catalog (`None` for the live one) -- what
/// [`build_catalog`] returns. Named so its `Result` doesn't trip
/// `clippy::type_complexity`.
pub(crate) type CatalogAndState = (willikins_core::Catalog, Option<Arc<Mutex<FakeState>>>);

/// Build the catalog `plan`/`apply` run against: the live catalog from
/// the process environment when `live` (real credentials, real network
/// calls; refuses before any of them with a kind-tagged
/// [`willikins_server::LiveCredentialError`] when a credential is missing
/// or malformed), otherwise the fake providers, optionally seeded from
/// `fake_state_path` -- returning the seeded [`FakeState`] handle too, so
/// `apply`'s own `--fake-state-out` can dump it back out once a run
/// reaches its final state. `live` and `fake_state_path` are mutually
/// exclusive.
pub(crate) fn build_catalog(
    live: bool,
    fake_state_path: Option<&str>,
    json: bool,
) -> Result<CatalogAndState, ExitCode> {
    if live {
        if fake_state_path.is_some() {
            return Err(usage_error(
                "--live and --fake-state are mutually exclusive",
            ));
        }
        let catalog =
            willikins_server::live_catalog_from_env().map_err(|error| fail_config(&error, json))?;
        return Ok((catalog, None));
    }
    let state = match fake_state_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path).map_err(|error| {
                usage_error(&format!("{path}: failed to read fake state: {error}"))
            })?;
            FakeState::from_json(&contents)
                .map_err(|error| usage_error(&format!("{path}: invalid fake state: {error}")))?
        }
        None => FakeState::new(),
    };
    let state = Arc::new(Mutex::new(state));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    Ok((catalog, Some(state)))
}

/// Open `path` as a [`FileJournal`], or build a fresh in-memory one when
/// `path` is `None` -- the CLI's own default, gone once this process
/// exits. A [`willikins_journal::JournalError::Locked`] (a running server,
/// or another CLI command, already holds `path`) surfaces plainly through
/// [`CliError::Journal`], never as a generic I/O message.
fn open_journal(
    path: Option<&str>,
    clock: Arc<dyn Clock>,
    json: bool,
) -> Result<SharedJournal, ExitCode> {
    match path {
        Some(path) => FileJournal::open_with_clock(Path::new(path), clock)
            .map(|journal| Arc::new(Mutex::new(journal)) as SharedJournal)
            .map_err(|error| {
                fail_config(
                    &CliError::Journal {
                        path: path.to_string(),
                        error: error.to_string(),
                    },
                    json,
                )
            }),
        None => Ok(Arc::new(Mutex::new(MemoryJournal::with_clock(clock))) as SharedJournal),
    }
}

fn parse_principal(raw: &str) -> Result<PrincipalId, ExitCode> {
    PrincipalId::parse(raw).map_err(|error| usage_error(&format!("--principal: {error}")))
}

/// Poll `butler.run(run_id)` until it reports something other than
/// `Running`. `apply` never exits before the run's final state (the
/// plan's own decision 5), so this has no bound: the run thread it is
/// waiting on always terminates on its own (`willikins_journal::continue_run_and_journal`
/// catches a panic and still journals `RunFinished`).
fn wait_for_run(butler: &Butler, run_id: RunId) -> RunRecord {
    loop {
        if let Some(record) = butler.run(run_id)
            && !matches!(record.state, RunState::Running)
        {
            return record;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn print_plan_response(response: &PlanResponse, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(response).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("{}", render::plan_response_text(response));
    }
}

fn print_run_record(run: &RunRecord, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(run).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("{}", render::run_record_text(run));
    }
}

fn exit_for_run_state(run: &RunRecord) -> ExitCode {
    match run.state {
        RunState::Succeeded => ExitCode::from(0),
        RunState::Running | RunState::Failed => ExitCode::from(1),
    }
}

fn default_butler_config(
    workflows_dir: PathBuf,
    journal: SharedJournal,
    catalog: willikins_core::Catalog,
    clock: Arc<dyn Clock>,
) -> ButlerConfig {
    ButlerConfig {
        workflows_dir,
        journal,
        catalog,
        clock,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    }
}

/// Dump `state`'s current (already-redacted) contents to `path` -- best
/// effort, called only after a run has reached its final state (see
/// [`ApplyArgs::fake_state_out`]'s own doc for why that matters: a
/// seeded-but-unconsumed `next_token` dumps as its marker and refuses to
/// reload).
fn dump_fake_state(state: &Mutex<FakeState>, path: &str) -> std::io::Result<()> {
    let state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let json = serde_json::to_string_pretty(&*state)
        .unwrap_or_else(|_| unreachable!("FakeState always serializes"));
    std::fs::write(path, json)
}

// ---------------------------------------------------------------------
// apply
// ---------------------------------------------------------------------

/// `apply`: dispatch on `file` vs `--plan-id`. See the module docs.
pub fn cmd_apply(args: &ApplyArgs, json: bool) -> ExitCode {
    match (&args.file, &args.plan_id) {
        (Some(_), Some(_)) => usage_error("apply: pass a file or --plan-id, not both"),
        (None, None) => usage_error("apply: pass a file, or --plan-id <id> --journal <path>"),
        (Some(file), None) => cmd_apply_file(args, file, json),
        (None, Some(plan_id)) => cmd_apply_plan_id(args, plan_id, json),
    }
}

fn cmd_apply_file(args: &ApplyArgs, file: &str, json: bool) -> ExitCode {
    if args.workflows_dir.is_some() {
        return usage_error("apply: --workflows-dir only applies with --plan-id");
    }
    let principal = match parse_principal(&args.principal) {
        Ok(principal) => principal,
        Err(code) => return code,
    };

    let (catalog, fake_state) = match build_catalog(args.live, args.fake_state.as_deref(), json) {
        Ok(built) => built,
        Err(code) => return code,
    };
    if args.fake_state_out.is_some() && fake_state.is_none() {
        return usage_error("apply: --fake-state-out needs the fake providers (drop --live)");
    }

    let checked = match crate::load_and_check(file, &catalog, json) {
        Ok(checked) => checked,
        Err(code) => return code,
    };
    let partial = match crate::build_partial_inputs(&checked, &args.inputs) {
        Ok(partial) => partial,
        Err(code) => return code,
    };
    let workflow_name = checked.workflow.name.clone();

    let temp_dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => {
            return fail_config(
                &CliError::Io {
                    error: format!("failed to create a temporary directory: {error}"),
                },
                json,
            );
        }
    };
    let dest = temp_dir.path().join(format!("{workflow_name}.yaml"));
    if let Err(error) = std::fs::copy(file, &dest) {
        return fail_config(
            &CliError::Io {
                error: format!("{file}: failed to copy into a temporary directory: {error}"),
            },
            json,
        );
    }

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let journal = match open_journal(args.journal.as_deref(), Arc::clone(&clock), json) {
        Ok(journal) => journal,
        Err(code) => return code,
    };

    let butler = match Butler::start(default_butler_config(
        temp_dir.path().to_path_buf(),
        journal,
        catalog,
        clock,
    )) {
        Ok(butler) => butler,
        Err(error) => return fail_startup(error, json),
    };

    let code = plan_and_apply(
        &butler,
        workflow_name,
        &partial,
        &principal,
        args.approve,
        args.journal.as_deref(),
        json,
    );

    if let Some(path) = &args.fake_state_out
        && let Some(state) = &fake_state
        && let Err(error) = dump_fake_state(state, path)
    {
        eprintln!("{path}: failed to write fake state: {error}");
    }

    code
}

/// Plan `workflow` against `butler`, print the [`PlanResponse`], apply it
/// (self-approving first when `approve` and the plan needs it), wait for
/// the run to finish, and print the [`RunRecord`]. Shared by
/// `cmd_apply_file`'s one-shot flow; `--plan-id` mode has an existing
/// plan and so calls `butler.apply` directly instead.
fn plan_and_apply(
    butler: &Butler,
    workflow: WorkflowName,
    partial: &willikins_core::describe::PartialInputs,
    principal: &PrincipalId,
    approve: bool,
    journal_path: Option<&str>,
    json: bool,
) -> ExitCode {
    let response = match butler.plan(workflow, partial, principal.clone()) {
        Ok(response) => response,
        Err(error) => return fail_apply(&error, None, journal_path, butler, json),
    };
    print_plan_response(&response, json);

    if approve
        && response.requires_approval
        && let Err(error) = butler.approve(response.plan_id, principal.clone())
    {
        return fail_apply(&error, Some(response.plan_id), journal_path, butler, json);
    }

    let handle = match butler.apply(response.plan_id, principal.clone()) {
        Ok(handle) => handle,
        Err(error) => {
            return fail_apply(&error, Some(response.plan_id), journal_path, butler, json);
        }
    };
    let run = wait_for_run(butler, handle.run_id);
    print_run_record(&run, json);
    exit_for_run_state(&run)
}

fn cmd_apply_plan_id(args: &ApplyArgs, plan_id_str: &str, json: bool) -> ExitCode {
    let Some(journal_path) = args.journal.as_deref() else {
        return usage_error("apply --plan-id needs --journal <path>");
    };
    if args.approve {
        return usage_error(
            "apply --plan-id does not take --approve; approve the plan first with `willikins approve`",
        );
    }
    if args.fake_state_out.is_some() {
        return usage_error("apply --plan-id does not take --fake-state-out");
    }
    if !args.inputs.is_empty() {
        return usage_error(
            "apply --plan-id does not take --input; its inputs were already resolved and recorded at `plan` time",
        );
    }

    let plan_id: PlanId = match plan_id_str.parse() {
        Ok(id) => id,
        Err(error) => {
            return fail_config(
                &CliError::InvalidId {
                    argument: plan_id_str.to_string(),
                    error: error.to_string(),
                },
                json,
            );
        }
    };
    let principal = match parse_principal(&args.principal) {
        Ok(principal) => principal,
        Err(code) => return code,
    };
    let workflows_dir = PathBuf::from(args.workflows_dir.as_deref().unwrap_or("workflows"));

    let (catalog, _fake_state) = match build_catalog(args.live, args.fake_state.as_deref(), json) {
        Ok(built) => built,
        Err(code) => return code,
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let journal = match open_journal(Some(journal_path), Arc::clone(&clock), json) {
        Ok(journal) => journal,
        Err(code) => return code,
    };

    let butler = match Butler::start(default_butler_config(
        workflows_dir,
        journal,
        catalog,
        clock,
    )) {
        Ok(butler) => butler,
        Err(error) => return fail_startup(error, json),
    };

    let handle = match butler.apply(plan_id, principal) {
        Ok(handle) => handle,
        Err(error) => return fail_apply(&error, Some(plan_id), Some(journal_path), &butler, json),
    };
    let run = wait_for_run(&butler, handle.run_id);
    print_run_record(&run, json);
    exit_for_run_state(&run)
}

// ---------------------------------------------------------------------
// approve / reject
// ---------------------------------------------------------------------

enum Decision {
    Approve,
    Reject(String),
}

pub fn cmd_approve(args: &ApproveArgs, json: bool) -> ExitCode {
    decide(
        &args.plan_id,
        &args.journal,
        &args.principal,
        &Decision::Approve,
        json,
    )
}

pub fn cmd_reject(args: &RejectArgs, json: bool) -> ExitCode {
    decide(
        &args.plan_id,
        &args.journal,
        &args.principal,
        &Decision::Reject(args.reason.clone()),
        json,
    )
}

/// `approve`/`reject` share everything but which [`Butler`] method they
/// call: open the named journal exclusively, build a bare [`Butler`]
/// (`Butler::new`, not `Butler::start` -- neither operation reads a
/// workflow document, so there is nothing to validate a directory for;
/// the placeholder `workflows_dir` and the empty fake catalog are never
/// touched), and record the decision.
fn decide(
    plan_id_str: &str,
    journal_path: &str,
    principal_str: &str,
    decision: &Decision,
    json: bool,
) -> ExitCode {
    let plan_id: PlanId = match plan_id_str.parse() {
        Ok(id) => id,
        Err(error) => {
            return fail_config(
                &CliError::InvalidId {
                    argument: plan_id_str.to_string(),
                    error: error.to_string(),
                },
                json,
            );
        }
    };
    let principal = match parse_principal(principal_str) {
        Ok(principal) => principal,
        Err(code) => return code,
    };
    let reason = match decision {
        Decision::Approve => None,
        Decision::Reject(reason) => match Reason::parse(reason) {
            Ok(reason) => Some(reason),
            Err(error) => return usage_error(&format!("--reason: {error}")),
        },
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let journal = match open_journal(Some(journal_path), clock, json) {
        Ok(journal) => journal,
        Err(code) => return code,
    };

    let (_state, fake_catalog) = Butler::fake_catalog();
    let butler = Butler::new(default_butler_config(
        PathBuf::from("."),
        journal,
        fake_catalog,
        Arc::new(SystemClock),
    ));

    let result = match decision {
        Decision::Approve => butler.approve(plan_id, principal),
        Decision::Reject(_) => butler.reject(
            plan_id,
            principal,
            reason.unwrap_or_else(|| unreachable!("Decision::Reject always has a reason")),
        ),
    };

    match result {
        Ok(()) => {
            if json {
                println!("{}", serde_json::json!({ "ok": true, "plan_id": plan_id }));
            } else {
                println!("ok");
            }
            ExitCode::from(0)
        }
        Err(error) => fail_domain(&error, json),
    }
}

// ---------------------------------------------------------------------
// runs / run
// ---------------------------------------------------------------------

fn open_replayed(path: &str, json: bool) -> Result<willikins_journal::ReplayedJournal, ExitCode> {
    willikins_journal::replay(path).map_err(|error| {
        fail_config(
            &CliError::Journal {
                path: path.to_string(),
                error: error.to_string(),
            },
            json,
        )
    })
}

pub fn cmd_runs(args: &RunsArgs, json: bool) -> ExitCode {
    let replayed = match open_replayed(&args.journal, json) {
        Ok(replayed) => replayed,
        Err(code) => return code,
    };
    let runs = replayed.runs();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&runs).unwrap_or_else(|_| "[]".to_string())
        );
    } else if runs.is_empty() {
        println!("no runs recorded");
    } else {
        let text = runs
            .iter()
            .map(render::run_record_text)
            .collect::<Vec<_>>()
            .join("\n---\n");
        println!("{text}");
    }
    ExitCode::from(0)
}

pub fn cmd_run(args: &RunArgs, json: bool) -> ExitCode {
    let run_id: RunId = match args.run_id.parse() {
        Ok(id) => id,
        Err(error) => {
            return fail_config(
                &CliError::InvalidId {
                    argument: args.run_id.clone(),
                    error: error.to_string(),
                },
                json,
            );
        }
    };
    let replayed = match open_replayed(&args.journal, json) {
        Ok(replayed) => replayed,
        Err(code) => return code,
    };
    match replayed.run(&run_id) {
        Some(run) => {
            print_run_record(&run, json);
            exit_for_run_state(&run)
        }
        None => fail_domain(
            &CliError::UnknownRun {
                run_id: run_id.to_string(),
            },
            json,
        ),
    }
}
