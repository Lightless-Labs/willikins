//! The live Doppler **rollback** cycle: milestone 3m task L2
//! (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`), verify items
//! 3-7 and 9, read "as L1" (`willikins-providers-github`'s own
//! `tests/live_rollback_cycle.rs`): this task's own scoped command names
//! only this crate, so it calls `doppler.project.ensure`'s,
//! `doppler.config.ensure`'s, `doppler.branch_config.ensure`'s and
//! `doppler.service_token.ensure`'s tools and their inverses directly,
//! building `Created`/`Rider` by hand (the same shape
//! `tests/it/project_inverse_mock.rs`, `config_inverse_mock.rs` and
//! `branch_config_inverse_mock.rs` already use, but against the real
//! sandbox workplace), never through `willikins-core`'s
//! `plan_teardown`/`apply_teardown` or a document. L4 is the
//! document-and-CLI-shaped task.
//!
//! `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- exactly `tests/live_write_cycle.rs`'s own
//! gate, read only past that check. Compiled only under this crate's
//! `live-tests` feature (its own `[[test]]` entry in `Cargo.toml`), so a
//! plain `cargo test --workspace` never builds this file at all.
//!
//! **This task writes the harness; it does not run it.** Per the plan's
//! own task table, sonnet writes L2, the coordinator runs it once:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
//!   cargo test -p willikins-providers-doppler --features live-tests \
//!   --test live_rollback_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! # Four independent sub-cycles, each disposing of its own throwaway project(s)
//!
//! Every project name starts with the plan's own SHARED VALUES "Live
//! probe prefix", [`PROBE_PREFIX`] (`willikins-probe-delete-me-`),
//! followed by the current Unix time and a one-letter tag --
//! `DopplerProject`'s own 64-character bound leaves far more room than
//! `GitHubRepo`'s 32 did for `tests/live_rollback_cycle.rs` in
//! `willikins-providers-github`, so no truncation is needed here.
//!
//! - **`-a`: clean, full-stack LIFO rollback** (verify 5, 6, 7, 9; P2's
//!   own "left for L2" `per_page` question). `doppler.project.ensure`
//!   creates it; an environment `qa` (`doppler.config.ensure`), a branch
//!   config under it (`doppler.branch_config.ensure`), and a service
//!   token in that branch (`doppler.service_token.ensure`) are created on
//!   top. Every one of the four inverses' own `standing` then reads
//!   `AsCreated` with **no rider at all** -- by the time each parent's
//!   `standing` is checked, every tool riding inside it has *already*
//!   been deleted, LIFO (token, then branch, then environment, then
//!   project), so nothing foreign remains to tolerate. `delete` on each
//!   exercises one of verify item 9's three endpoint shapes in turn
//!   (the branch's own `DELETE /v3/configs/config`, the environment's
//!   `DELETE /v3/environments/environment`, the project's `DELETE
//!   /v3/projects/project` with a body) plus the service token's own
//!   fourth delete endpoint, not itself one of the three verify item 9
//!   names but exercised anyway. Along the way: the project's, the
//!   environment's and the token list's own raw bodies are checked for
//!   `created_at` and parsed as RFC 3339 (verify 6); the environments
//!   list is asserted to be exactly `{dev, stg, prd}` before `qa` is
//!   added (verify 7); the token list's entries are checked for `slug`
//!   and `created_at` (verify 5); and `GET /v3/environments` is probed at
//!   `per_page=1` to record whether Doppler actually honours it once
//!   four environments exist (P2's own addendum).
//! - **`-b`: the rider mechanic, and verify item 3.** `qa` plus a branch
//!   config that is *never* torn down on its own. The environment's own
//!   `standing`/`delete` are called **twice**: once with no rider (must
//!   refuse, decision 7's own row: "a config in the environment that no
//!   rider created") and once with the branch recorded as a tolerated
//!   `doppler.branch_config.ensure` rider (must read `AsCreated`,
//!   proving decision 3a's rider mechanic). The second `delete` is a
//!   **real** `DELETE /v3/environments/environment` issued while the
//!   branch config genuinely still exists live -- verify item 3's own
//!   question, which this file answers by recording the outcome rather
//!   than assuming it.
//! - **`-c`: hand-set secret drift**, the Doppler analogue of
//!   `willikins-providers-github`'s own `-c` sub-cycle. A secret hand-set
//!   into the project's default `dev` root config, through a raw `POST`
//!   this crate's own tools never call, is never a rider; the project's
//!   own `standing` must read `Drifted`, naming the secret, and `delete`
//!   must refuse.
//! - **`-d1`/`-d2`: cross-project inheritance, verify item 4.** Two
//!   projects, named and ordered exactly as the proven
//!   `tests/live_secret_name_gate_cycle.rs` base/child pair: `-d1`'s
//!   `dev` root config is made inheritable
//!   (`doppler.config.inheritable.ensure`) and `-d2`'s `dev` root
//!   inherits it (`doppler.config.inherits.ensure`). `-d1`'s environment
//!   inverse then reads `Drifted`, naming a foreign inheritor (decision
//!   7's own row). A real `DELETE /v3/environments/environment` on
//!   `-d1`'s `dev`, issued while `-d2` still inherits it, records
//!   whether Doppler itself refuses or allows it -- verify item 4's own
//!   question. Cleanup deletes `-d2` before `-d1`, the same order the
//!   proven base/child cycle already uses and for the same stated reason
//!   ("Doppler may refuse to delete a project whose config another
//!   config inherits").
//!
//! # Why raw requests for verification, not the client's own typed calls
//!
//! `DopplerClient`'s own read/write methods stay `pub(crate)`, exactly as
//! `tests/live_write_cycle.rs`'s own doc explains. Every independent
//! check this file makes is a raw request against the shared
//! [`willikins_providers_http::Http`] client instead.
//!
//! # No `settled_by` sleep
//!
//! Unlike `willikins-providers-github`'s own `-b` sub-cycle, no step here
//! sleeps past `settled_by`: none of the four Doppler inverses' own
//! `standing` reads `Created::settled_by` at all (decision 7's table),
//! only `Created::window`. `settled_by` is still supplied (`Created`
//! requires it), set to `window.end`, since nothing in this file ever
//! reads it back either.
//!
//! # A note on fragility, by design
//!
//! `doppler.project.ensure`'s own `standing` walks **every** config in
//! the project, including the three Doppler creates automatically
//! (`dev`/`stg`/`prd`), and asks `config_contents_drift` about each one
//! -- which, per that function's own fail-closed rule, reads `Drifted`
//! the moment a config's single-`GET` response omits `inheritedBy`
//! outright (`crate::inverse_support::config_contents_drift`'s own doc:
//! "`None` => return `Some(ConfigContentsDrift::Inheritor)`"). So **every**
//! assertion below that expects `AsCreated` or a *specific* `Drifted`
//! reason is also, incidentally, the first live proof that Doppler's
//! single-config `GET` really does carry `inheritedBy` on an ordinary,
//! untouched root config -- P2's own addendum already assumed this from
//! the fetched reference page, never from a live call. A failure here
//! whose panic message names `DRIFTED_FOREIGN_INHERITOR`'s own text
//! rather than the reason a step expected is exactly that finding, not a
//! bug in this harness, and [`verify_6_and_inheritedby_shape`] prints
//! the raw fact directly before any sub-cycle relies on it.
//!
//! # Redaction
//!
//! The one service token this cycle mints is read back only through
//! [`willikins_types::DomainType::expose`] (this file's own sink token,
//! never `expose_secret`), kept in a buffer swept at the end together
//! with every credential-shaped prefix this crate's
//! [`willikins_providers_doppler::CREDENTIAL_PATTERN`] and Doppler's own
//! service-token prefix admit. The hand-set secret in `-c` is a plain,
//! non-credential-shaped word on purpose, so `secret_literal_guard.rs`
//! has nothing to catch in this file's own source.

use std::sync::Arc;

use serde_json::Value as Json;
use willikins_core::rollback::{CREATION_SKEW, CreationWindow, Rider};
use willikins_core::{
    Created, Inputs, InstanceRef, Inverse, NodeName, Outputs, PortName, Standing, Timestamp, Tool,
    ToolErrorKind, Value,
};
use willikins_providers_doppler::{
    DopplerBranchConfigEnsure, DopplerClient, DopplerConfigEnsure, DopplerProjectEnsure,
    DopplerServiceTokenEnsure, credential_from_env, http_client, looks_like_a_missing_project,
};
use willikins_providers_http::Http;
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, DopplerServiceToken,
    DopplerTokenName, EnvironmentSlug, SecretName, TeardownToken, naming,
};

/// The plan's own SHARED VALUES "Live probe prefix", owned by L1-L3.
/// `DopplerProject`'s grammar (max 64 characters) leaves far more room
/// than `GitHubRepo`'s 32 did for `willikins-providers-github`'s own
/// `tests/live_rollback_cycle.rs`, so this file names projects
/// `<PROBE_PREFIX><unix seconds>-<tag>` with no truncation.
const PROBE_PREFIX: &str = "willikins-probe-delete-me-";

/// The environment every sub-cycle but `-c` and `-d*` adds on top of the
/// three Doppler creates by default. Not `GITHUB_`-shaped, not one of
/// `dev`/`stg`/`prd`/`qa` any real document is likely to collide with.
const EXTRA_ENVIRONMENT: &str = "qa";

/// The branch suffix `-a` creates and tears down on its own, exercising
/// verify item 9's `DELETE /v3/configs/config` endpoint in isolation.
const BRANCH_SUFFIX_A: &str = "probe";

/// The branch suffix `-b` creates and deliberately never tears down on
/// its own -- the rider `-b`'s own environment inverse must tolerate.
const BRANCH_SUFFIX_B: &str = "keep";

/// The service token name `-a` mints.
const TOKEN_NAME: &str = "rollback-probe";

/// The Doppler default root environments every fresh project arrives
/// with (verify item 7).
const DEFAULT_ENVIRONMENTS: [&str; 3] = ["dev", "stg", "prd"];

/// The secret name `-c` hand-sets into `dev`, never through a rider.
const HAND_SECRET_NAME: &str = "WILLIKINS_ROLLBACK_PROBE";

/// `-c`'s hand-set secret value: a plain word, deliberately not shaped
/// like any credential this workspace's `secret_literal_guard.rs` or
/// `willikins_providers_doppler::CREDENTIAL_PATTERN` would ever
/// recognise, so this file's own source trips neither.
const HAND_SECRET_VALUE: &str = "not-a-credential-just-a-probe-marker";

/// Token prefixes that must appear in nothing this file produces: the two
/// `willikins_providers_doppler::CREDENTIAL_PATTERN` accepts, the one
/// Doppler issues service tokens under, and the `Authorization` scheme
/// their bytes travel in.
const CREDENTIAL_PREFIXES: &[&str] = &["dp.sa.", "dp.pt.", "dp.st.", "Bearer"];

/// A port name, parsed. Every name this file passes is a literal.
fn port(name: &'static str) -> PortName {
    PortName::parse(name).expect("the cycle's port names are valid")
}

/// The test's own sink token -- see `tests/live_write_cycle.rs`'s own
/// `sink_token` for why this narrow opt-in is the sanctioned one.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> willikins_core::SinkToken {
    willikins_core::SinkToken::new()
}

/// This file's own teardown token -- the one legitimate test-side mint
/// of the capability `willikins_core::rollback::apply_teardown`
/// otherwise holds exclusively (milestone 3m trust boundary 2, task R2).
/// This file calls an inverse's `delete` directly, outside
/// `apply_teardown` entirely, the same opt-in every mock test in this
/// crate's own `tests/it/*_inverse_mock.rs` files already uses.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn teardown_token() -> TeardownToken {
    TeardownToken::new()
}

/// The current Unix time in whole seconds, for this cycle's own
/// throwaway project names.
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock reads a time after 1970")
        .as_secs()
}

/// A fresh project identity: `<PROBE_PREFIX><unix seconds>-<tag>`.
fn probe_project(timestamp: u64, tag: &str) -> DopplerProject {
    let name = format!("{PROBE_PREFIX}{timestamp}-{tag}");
    DopplerProject::parse(&name)
        .unwrap_or_else(|err| panic!("`{name}` must be a valid DopplerProject: {}", err.reason))
}

/// `at` shifted by `seconds` -- this file's own copy of
/// `willikins_core::rollback`'s private `shift`, unreachable from this
/// separate integration-test crate.
fn shift(at: Timestamp, seconds: i64) -> Timestamp {
    Timestamp::from_datetime(*at.as_datetime() + chrono::Duration::seconds(seconds))
}

/// This cycle's own creation window (decision 2):
/// `[before - CREATION_SKEW, after + CREATION_SKEW]`.
fn creation_window(before: Timestamp, after: Timestamp) -> CreationWindow {
    CreationWindow {
        start: shift(before, -CREATION_SKEW),
        end: shift(after, CREATION_SKEW),
    }
}

/// A [`Created`] over `inputs`/`outputs` and `window`, with no accepted
/// loss (no Doppler inverse this file calls declares one) and whatever
/// `riders` the caller tolerates. `settled_by` is `window.end`: no
/// Doppler inverse's own `standing` reads it (see this file's own module
/// doc, "No `settled_by` sleep").
fn created<'a>(
    inputs: &'a Inputs,
    outputs: &'a Outputs,
    window: CreationWindow,
    riders: &'a [Rider<'a>],
) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window,
        settled_by: window.end,
        accept: &[],
        riders,
    }
}

/// `tool`'s inverse, unwrapped: every tool this file calls declares
/// `Reversal::Delete`, so `inverse()` is always `Some`.
fn inverse_of<T: Tool>(tool: &T) -> &dyn Inverse {
    tool.inverse()
        .expect("this tool declares Delete, so inverse() is Some")
}

/// `project`'s own `project` input, the shape every one of this file's
/// four tools' `inputs` starts from.
fn project_inputs(project: &DopplerProject) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project.clone()));
    inputs
}

fn config_inputs(project: &DopplerProject, environment: &str) -> Inputs {
    let mut inputs = project_inputs(project);
    inputs.insert(
        port("environment"),
        Value::known(EnvironmentSlug::parse(environment).expect("a valid EnvironmentSlug")),
    );
    inputs
}

fn branch_inputs(project: &DopplerProject, environment: &str, branch: &str) -> Inputs {
    let mut inputs = config_inputs(project, environment);
    inputs.insert(
        port("branch"),
        Value::known(DopplerConfigName::parse(branch).expect("a valid DopplerConfigName")),
    );
    inputs
}

fn token_inputs(config: &DopplerConfig, name: &str) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config.clone()));
    inputs.insert(
        port("name"),
        Value::known(DopplerTokenName::parse(name).expect("a valid DopplerTokenName")),
    );
    inputs
}

/// The `config` output of an `Ensured`/`Outputs` this file's own
/// `doppler.config.ensure`/`doppler.branch_config.ensure` calls just
/// produced -- this file's own copy of `inverse_support::config_output`,
/// `pub(crate)` there and unreachable from this separate
/// integration-test crate.
fn config_output(outputs: &Outputs) -> DopplerConfig {
    outputs
        .get(&port("config"))
        .and_then(|value| value.downcast::<DopplerConfig>())
        .cloned()
        .expect("an ensure's own outputs carry a known `config` port")
}

/// `project`'s derived root config for `environment`, exactly as
/// `naming::v1::doppler_root_config` derives it.
fn root_config(project: &DopplerProject, environment: &str) -> DopplerConfig {
    let slug = EnvironmentSlug::parse(environment).expect("a valid EnvironmentSlug");
    naming::v1::doppler_root_config(project, &slug)
}

/// Panics with `other`'s full `Debug` unless `standing` is `AsCreated`
/// with no losing names -- every assertion in this file that expects a
/// clean resource goes through this, rather than a bare `assert_eq!`,
/// so a surprise (most plausibly `inheritedBy` missing from a live
/// response -- see this file's own module doc, "A note on fragility, by
/// design") is reported with the actual reason, not just "not equal".
fn expect_as_created(standing: &Standing, context: &str) {
    match standing {
        Standing::AsCreated { losing } if losing.is_empty() => {}
        other => panic!("{context}: expected AsCreated with no losses, got {other:?}"),
    }
}

/// Panics with `other`'s full `Debug` unless `standing` is `Drifted`
/// whose own reason contains `expect_substring` -- the same "name the
/// actual reason on a surprise" discipline as [`expect_as_created`].
fn expect_drifted_containing<'a>(
    standing: &'a Standing,
    expect_substring: &str,
    context: &str,
) -> &'a str {
    match standing {
        Standing::Drifted { what } if what.contains(expect_substring) => what,
        other => panic!("{context}: expected Drifted naming {expect_substring:?}, got {other:?}"),
    }
}

/// Everything one run of this cycle shares.
struct Cycle {
    raw: Arc<Http>,
    project_tool: DopplerProjectEnsure,
    config_tool: DopplerConfigEnsure,
    branch_tool: DopplerBranchConfigEnsure,
    token_tool: DopplerServiceTokenEnsure,
    sweep: Vec<String>,
    minted: Vec<String>,
    observations: Vec<String>,
}

impl Cycle {
    fn new(credential: willikins_providers_http::Credential) -> Self {
        let raw = Arc::new(http_client(credential.clone()));
        let client = Arc::new(DopplerClient::new(http_client(credential)));
        Self {
            raw,
            project_tool: DopplerProjectEnsure::new(Arc::clone(&client)),
            config_tool: DopplerConfigEnsure::new(Arc::clone(&client)),
            branch_tool: DopplerBranchConfigEnsure::new(Arc::clone(&client)),
            token_tool: DopplerServiceTokenEnsure::new(client),
            sweep: Vec::new(),
            minted: Vec::new(),
            observations: Vec::new(),
        }
    }

    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    fn observe(&mut self, text: String) {
        println!("observation: {text}");
        self.observations.push(text.clone());
        self.sweep.push(text);
    }

    fn get(&self, path: &str) -> Json {
        self.raw
            .get::<Json>(path)
            .unwrap_or_else(|err| panic!("raw GET {path} failed (status {:?})", err.status))
    }
}

/// `GET /v3/projects/project?project=<name>`.
fn project_path(project: &DopplerProject) -> String {
    format!("/v3/projects/project?project={project}")
}

/// Deletes every project this run created, on every exit path (a panic,
/// a failed assertion, or an early return) unless the test body already
/// deleted them and disarmed the guard. Copied from
/// `tests/live_write_cycle.rs`'s own `ProjectGuard`, down to the
/// `400`-or-`404`-both-mean-gone reading
/// (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
struct ProjectGuard {
    http: Arc<Http>,
    projects: Vec<DopplerProject>,
}

impl ProjectGuard {
    fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            projects: Vec::new(),
        }
    }

    fn register(&mut self, project: &DopplerProject) {
        self.projects.push(project.clone());
    }

    /// Removes one project from the guard's own list, once the test body
    /// has itself confirmed it is gone -- every other registered
    /// project's own protection is left in place.
    fn disarm_one(&mut self, project: &DopplerProject) {
        self.projects.retain(|registered| registered != project);
    }

    fn delete(
        http: &Http,
        project: &DopplerProject,
    ) -> Result<(), willikins_providers_http::ProviderError> {
        let body = serde_json::json!({ "project": project.to_string() });
        http.delete_with_body("/v3/projects/project", &body)
    }
}

impl Drop for ProjectGuard {
    fn drop(&mut self) {
        let already_panicking = std::thread::panicking();
        for project in &self.projects {
            println!("guard: deleting `{project}`");
            match Self::delete(&self.http, project) {
                Ok(()) => println!("guard: deleted `{project}`"),
                Err(err) if err.status == Some(404) || err.status == Some(400) => {
                    println!(
                        "guard: `{project}` was already gone (status {:?})",
                        err.status
                    );
                }
                Err(err) => {
                    let status = err.status;
                    println!(
                        "guard: !!! LEFTOVER PROJECT `{project}` !!! the guard's DELETE failed \
                         (status {status:?}); it must be deleted by hand"
                    );
                    assert!(
                        already_panicking,
                        "the guard could not delete `{project}` (status {status:?}); it must be \
                         deleted by hand"
                    );
                }
            }
        }
    }
}

/// Step 0: every probe project name must be absent before this cycle
/// writes anything -- a leftover from an aborted run is the operator's
/// to delete by hand.
fn step_0_absent(raw: &Http, projects: &[&DopplerProject]) {
    for project in projects {
        match raw.get::<Json>(&project_path(project)) {
            Err(err) if looks_like_a_missing_project(&err) => {}
            Ok(_) => panic!(
                "step 0: `{project}` already exists, left behind by an aborted run; a leftover \
                 is the operator's to delete by hand"
            ),
            Err(err) => panic!(
                "step 0: GET of `{project}` answered an unexpected status {:?}",
                err.status
            ),
        }
    }
    println!("step 0 (no probe project is visible to this token): pass");
}

/// Verify item 6's own project half, plus a direct, raw look at whether
/// a single config's own `GET` really carries `inheritedBy` -- see this
/// file's own module doc, "A note on fragility, by design". Printed
/// before any sub-cycle relies on the answer.
fn verify_6_and_inheritedby_shape(cycle: &mut Cycle, project: &DopplerProject) {
    let body = cycle.get(&project_path(project));
    let created_at = body.pointer("/project/created_at").and_then(Json::as_str);
    assert!(
        created_at.is_some(),
        "verify 6: the project object must carry created_at, got: {body:?}"
    );
    Timestamp::parse(created_at.expect("checked above"))
        .expect("verify 6: the project's created_at must parse as RFC 3339");
    cycle.say("verify 6 (the project's created_at is present and parses as RFC 3339): pass");

    let dev = cycle.get(&format!("/v3/configs/config?project={project}&config=dev"));
    let has_inherited_by = dev.pointer("/config/inheritedBy").is_some();
    let has_config_created_at = dev.pointer("/config/created_at").is_some();
    cycle.observe(format!(
        "the single-config GET of an untouched `dev` root carries inheritedBy: \
         {has_inherited_by}, created_at: {has_config_created_at} (P3's own addendum says the \
         single-config GET carries no timestamp of its own; verify item 6's own text names \
         configs-get.md as showing one -- this line settles which, live)"
    ));

    let dev_env = cycle.get(&format!(
        "/v3/environments/environment?project={project}&environment=dev"
    ));
    let env_created_at = dev_env
        .pointer("/environment/created_at")
        .and_then(Json::as_str);
    assert!(
        env_created_at.is_some(),
        "verify 6: the single-environment GET must carry created_at, got: {dev_env:?}"
    );
    Timestamp::parse(env_created_at.expect("checked above"))
        .expect("verify 6: the environment's created_at must parse as RFC 3339");
    cycle.say(
        "verify 6 (the single-environment GET's created_at is present and parses as RFC 3339): \
         pass",
    );
}

/// Verify item 7: a fresh project arrives with exactly `dev`, `stg` and
/// `prd`.
fn verify_7_default_environments(cycle: &mut Cycle, project: &DopplerProject) {
    let body = cycle.get(&format!("/v3/environments?project={project}"));
    let mut ids: Vec<String> = body
        .pointer("/environments")
        .and_then(Json::as_array)
        .expect("an environments list carries an array")
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Json::as_str))
        .map(ToString::to_string)
        .collect();
    ids.sort();
    let mut expected: Vec<String> = DEFAULT_ENVIRONMENTS
        .iter()
        .map(ToString::to_string)
        .collect();
    expected.sort();
    assert_eq!(
        ids, expected,
        "verify 7: a fresh project must arrive with exactly dev, stg and prd"
    );
    cycle.say("verify 7 (a fresh project arrives with exactly dev, stg, prd): pass");
}

/// P2's own "left for L2" question: does `GET /v3/environments` honour
/// `per_page`? Recorded, not asserted either way -- the project already
/// holds four environments (`dev`/`stg`/`prd`/`qa`) by the time this
/// runs.
fn observe_environments_per_page(cycle: &mut Cycle, project: &DopplerProject) {
    let page_1 = cycle.get(&format!(
        "/v3/environments?project={project}&per_page=1&page=1"
    ));
    let len = page_1
        .pointer("/environments")
        .and_then(Json::as_array)
        .map_or(0, Vec::len);
    cycle.observe(format!(
        "GET /v3/environments?per_page=1&page=1 on a project with 4 environments returned {len} \
         entries ({})",
        if len <= 1 {
            "per_page honoured"
        } else {
            "per_page ignored"
        }
    ));
}

/// Verify item 5: the token list carries `slug` and `created_at`, which
/// parses as RFC 3339.
fn verify_5_token_list_shape(cycle: &mut Cycle, project: &DopplerProject, config: &DopplerConfig) {
    let tokens_body = cycle.get(&format!(
        "/v3/configs/config/tokens?project={project}&config={}",
        config.name()
    ));
    let entries = tokens_body
        .pointer("/tokens")
        .and_then(Json::as_array)
        .expect("a tokens list carries an array");
    let entry = entries
        .iter()
        .find(|entry| entry.get("name").and_then(Json::as_str) == Some(TOKEN_NAME))
        .unwrap_or_else(|| panic!("-a: the minted token is not listed: {tokens_body:?}"));
    assert!(
        entry.get("slug").and_then(Json::as_str).is_some(),
        "verify 5: a listed token must carry `slug`, got: {entry:?}"
    );
    let created_at = entry
        .get("created_at")
        .and_then(Json::as_str)
        .unwrap_or_else(|| {
            panic!("verify 5: a listed token must carry `created_at`, got: {entry:?}")
        });
    Timestamp::parse(created_at).expect("verify 5: a token's created_at must parse as RFC 3339");
    cycle.say("verify 5 (the token list carries slug and created_at, which parses): pass");
}

// ---------------------------------------------------------------------
// Sub-cycle -a: clean, full-stack LIFO rollback.
// ---------------------------------------------------------------------

/// Sub-cycle `-a`. Every resource it creates is deleted by the end of
/// this function, through the real inverses, LIFO; `guard` is disarmed
/// of `project` only once that delete is confirmed.
/// Everything [`sub_cycle_a_setup`] creates, for [`sub_cycle_a_teardown`]
/// to roll back LIFO. Every field is owned (`Inputs`/`Outputs` are not
/// references), so this bundle carries no lifetime of its own.
struct StackA {
    project_inputs: Inputs,
    project_outputs: Outputs,
    project_window: CreationWindow,
    qa_inputs: Inputs,
    qa_outputs: Outputs,
    qa_window: CreationWindow,
    branch_inputs: Inputs,
    branch_outputs: Outputs,
    branch_window: CreationWindow,
    branch_config: DopplerConfig,
    token_inputs: Inputs,
    token_outputs: Outputs,
    token_window: CreationWindow,
}

/// `-a`'s own setup: project, `qa`, a branch config under it, and a
/// service token in `qa`'s own root config. Verify items 6, 7 and P2's
/// own `per_page` question are checked along the way (while only the
/// defaults, then `qa` too, exist); verify item 5 is checked once the
/// token exists.
fn sub_cycle_a_setup(cycle: &mut Cycle, project: &DopplerProject) -> StackA {
    let before = Timestamp::now();
    let project_inputs = project_inputs(project);
    let project_ensured = cycle
        .project_tool
        .ensure(&project_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-a: creating the project failed: {}", err.message));
    let after = Timestamp::now();
    assert!(
        project_ensured.changed,
        "-a: creating the project must report changed: true"
    );
    let project_window = creation_window(before, after);

    verify_6_and_inheritedby_shape(cycle, project);
    verify_7_default_environments(cycle, project);

    let before = Timestamp::now();
    let qa_inputs = config_inputs(project, EXTRA_ENVIRONMENT);
    let qa_ensured = cycle
        .config_tool
        .ensure(&qa_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-a: creating `qa` failed: {}", err.message));
    let after = Timestamp::now();
    assert!(
        qa_ensured.changed,
        "-a: creating `qa` must report changed: true"
    );
    let qa_window = creation_window(before, after);
    let qa_config = config_output(&qa_ensured.outputs);

    observe_environments_per_page(cycle, project);

    let before = Timestamp::now();
    let branch_inputs = branch_inputs(project, EXTRA_ENVIRONMENT, BRANCH_SUFFIX_A);
    let branch_ensured = cycle
        .branch_tool
        .ensure(&branch_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-a: creating the branch config failed: {}", err.message));
    let after = Timestamp::now();
    assert!(
        branch_ensured.changed,
        "-a: creating the branch config must report changed: true"
    );
    let branch_window = creation_window(before, after);
    let branch_config = config_output(&branch_ensured.outputs);

    let before = Timestamp::now();
    let token_inputs = token_inputs(&qa_config, TOKEN_NAME);
    let token_ensured = cycle
        .token_tool
        .ensure(&token_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-a: minting the token failed: {}", err.message));
    let after = Timestamp::now();
    assert!(
        token_ensured.changed,
        "-a: minting the token must report changed: true"
    );
    let token_window = creation_window(before, after);
    let minted_token = token_ensured
        .outputs
        .get(&port("token"))
        .and_then(|value| value.downcast::<DopplerServiceToken>())
        .expect("-a: the mint's own outputs carry a known `token` port")
        .expose(&sink_token())
        .to_string();
    cycle.minted.push(minted_token);

    verify_5_token_list_shape(cycle, project, &qa_config);

    StackA {
        project_inputs,
        project_outputs: project_ensured.outputs,
        project_window,
        qa_inputs,
        qa_outputs: qa_ensured.outputs,
        qa_window,
        branch_inputs,
        branch_outputs: branch_ensured.outputs,
        branch_window,
        branch_config,
        token_inputs,
        token_outputs: token_ensured.outputs,
        token_window,
    }
}

/// `-a`'s own teardown: token, branch config, environment, project, in
/// that order, every `standing`/`delete` with no rider at all -- by the
/// time each parent is checked, every tool riding inside it is already
/// gone. Exercises verify item 9's three endpoint shapes (plus the
/// token's own fourth).
fn sub_cycle_a_teardown(
    cycle: &mut Cycle,
    project: &DopplerProject,
    guard: &mut ProjectGuard,
    stack: &StackA,
) {
    let token_created = created(
        &stack.token_inputs,
        &stack.token_outputs,
        stack.token_window,
        &[],
    );
    let standing = inverse_of(&cycle.token_tool)
        .standing(&token_created)
        .unwrap_or_else(|err| panic!("-a: the token's own standing failed: {}", err.message));
    expect_as_created(&standing, "-a token standing");
    let deleted = inverse_of(&cycle.token_tool)
        .delete(&token_created, &teardown_token())
        .unwrap_or_else(|err| panic!("-a: deleting the token failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "-a: deleting a freshly minted token must not be already_gone"
    );
    cycle.say("-a (token: standing AsCreated, delete succeeds): pass");

    let branch_created = created(
        &stack.branch_inputs,
        &stack.branch_outputs,
        stack.branch_window,
        &[],
    );
    let standing = inverse_of(&cycle.branch_tool)
        .standing(&branch_created)
        .unwrap_or_else(|err| panic!("-a: the branch's own standing failed: {}", err.message));
    expect_as_created(&standing, "-a branch standing");
    let deleted = inverse_of(&cycle.branch_tool)
        .delete(&branch_created, &teardown_token())
        .unwrap_or_else(|err| panic!("-a: deleting the branch config failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "-a: deleting a freshly created branch config must not be already_gone"
    );
    if cycle
        .client_get_config_raw(project, stack.branch_config.name().as_str())
        .is_some()
    {
        panic!("-a: the branch config is still readable after its own delete");
    }
    cycle.say(
        "-a (branch config: standing AsCreated, delete succeeds via DELETE /v3/configs/config -- \
         verify item 9): pass",
    );

    let qa_created = created(&stack.qa_inputs, &stack.qa_outputs, stack.qa_window, &[]);
    let standing = inverse_of(&cycle.config_tool)
        .standing(&qa_created)
        .unwrap_or_else(|err| panic!("-a: `qa`'s own standing failed: {}", err.message));
    expect_as_created(&standing, "-a qa standing");
    let deleted = inverse_of(&cycle.config_tool)
        .delete(&qa_created, &teardown_token())
        .unwrap_or_else(|err| panic!("-a: deleting `qa` failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "-a: deleting a freshly created environment must not be already_gone"
    );
    cycle.say(
        "-a (environment qa: standing AsCreated, delete succeeds via DELETE \
         /v3/environments/environment -- verify item 9): pass",
    );

    let project_created = created(
        &stack.project_inputs,
        &stack.project_outputs,
        stack.project_window,
        &[],
    );
    let standing = inverse_of(&cycle.project_tool)
        .standing(&project_created)
        .unwrap_or_else(|err| panic!("-a: the project's own standing failed: {}", err.message));
    expect_as_created(&standing, "-a project standing");
    let deleted = inverse_of(&cycle.project_tool)
        .delete(&project_created, &teardown_token())
        .unwrap_or_else(|err| panic!("-a: deleting the project failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "-a: deleting a freshly created project must not be already_gone"
    );
    match cycle.raw.get::<Json>(&project_path(project)) {
        Err(err) if looks_like_a_missing_project(&err) => {}
        other => panic!("-a: the project is still readable after its own delete: {other:?}"),
    }
    guard.disarm_one(project);
    cycle.say(
        "-a (project: standing AsCreated, delete succeeds via DELETE /v3/projects/project (a \
         body) -- verify item 9): pass",
    );
}

fn sub_cycle_a_clean_lifo(cycle: &mut Cycle, project: &DopplerProject, guard: &mut ProjectGuard) {
    guard.register(project);
    let stack = sub_cycle_a_setup(cycle, project);
    sub_cycle_a_teardown(cycle, project, guard, &stack);
}

impl Cycle {
    /// A raw, best-effort look at whether a config is still readable --
    /// `None` on anything `looks_like_a_missing_project` recognises,
    /// `Some` otherwise. Used only to confirm a branch config's own
    /// delete took effect; never load-bearing for any other step.
    fn client_get_config_raw(&self, project: &DopplerProject, config: &str) -> Option<Json> {
        match self.raw.get::<Json>(&format!(
            "/v3/configs/config?project={project}&config={config}"
        )) {
            Ok(body) => Some(body),
            Err(err) if looks_like_a_missing_project(&err) => None,
            Err(err) => panic!(
                "GET of config `{config}` answered an unexpected status {:?}",
                err.status
            ),
        }
    }
}

// ---------------------------------------------------------------------
// Sub-cycle -b: the rider mechanic, and verify item 3.
// ---------------------------------------------------------------------

/// Everything `-b` creates: project, `qa`, and the `keep` branch config
/// under it -- the branch is never torn down by [`sub_cycle_b_setup`]
/// itself, by design.
struct StackB {
    project_inputs: Inputs,
    project_outputs: Outputs,
    project_window: CreationWindow,
    qa_inputs: Inputs,
    qa_outputs: Outputs,
    qa_window: CreationWindow,
    branch_outputs: Outputs,
    branch_config: DopplerConfig,
}

fn sub_cycle_b_setup(cycle: &mut Cycle, project: &DopplerProject) -> StackB {
    let project_before = Timestamp::now();
    let project_inputs = project_inputs(project);
    let project_ensured = cycle
        .project_tool
        .ensure(&project_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-b: creating the project failed: {}", err.message));
    let project_window = creation_window(project_before, Timestamp::now());

    let before = Timestamp::now();
    let qa_inputs = config_inputs(project, EXTRA_ENVIRONMENT);
    let qa_ensured = cycle
        .config_tool
        .ensure(&qa_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-b: creating `qa` failed: {}", err.message));
    let after = Timestamp::now();
    let qa_window = creation_window(before, after);

    let branch_inputs = branch_inputs(project, EXTRA_ENVIRONMENT, BRANCH_SUFFIX_B);
    let branch_ensured = cycle
        .branch_tool
        .ensure(&branch_inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "-b: creating the `keep` branch config failed: {}",
                err.message
            )
        });
    let branch_config = config_output(&branch_ensured.outputs);

    StackB {
        project_inputs,
        project_outputs: project_ensured.outputs,
        project_window,
        qa_inputs,
        qa_outputs: qa_ensured.outputs,
        qa_window,
        branch_outputs: branch_ensured.outputs,
        branch_config,
    }
}

/// The rider mechanic (no rider refuses, a tolerated rider reads
/// `AsCreated`) and verify item 3's own real `DELETE
/// /v3/environments/environment`, issued while the branch config
/// genuinely still exists live. Leaves `qa` torn down one way or the
/// other by the time it returns.
/// No rider: the untolerated branch config must read Drifted and refuse
/// delete -- decision 7's own row, "a config in the environment that no
/// rider created".
fn verify_decision_7_no_rider_refuses(cycle: &mut Cycle, stack: &StackB) {
    let qa_created_no_riders = created(&stack.qa_inputs, &stack.qa_outputs, stack.qa_window, &[]);
    let standing = inverse_of(&cycle.config_tool)
        .standing(&qa_created_no_riders)
        .unwrap_or_else(|err| {
            panic!(
                "-b: `qa`'s own standing (no riders) failed: {}",
                err.message
            )
        });
    let what =
        expect_drifted_containing(&standing, "config", "-b qa standing (no riders)").to_string();
    cycle.say(&format!(
        "-b (qa with an untolerated branch config reads Drifted: {what:?}): pass"
    ));
    let err = inverse_of(&cycle.config_tool)
        .delete(&qa_created_no_riders, &teardown_token())
        .expect_err("-b: delete on a drifted environment must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "-b: the refusal must be a Conflict"
    );
    cycle.say("-b (delete on qa with an untolerated branch refuses as a Conflict): pass");
}

fn sub_cycle_b_rider_mechanic_and_verify_3(
    cycle: &mut Cycle,
    project: &DopplerProject,
    stack: &StackB,
) {
    verify_decision_7_no_rider_refuses(cycle, stack);

    // With the branch recorded as a tolerated rider: AsCreated -- proves
    // decision 3a's own rider mechanic.
    let branch_rider_instance = InstanceRef {
        node: NodeName::parse("branch_keep").expect("a valid NodeName"),
        instance: None,
    };
    let empty_rider_inputs = Inputs::new();
    let riders = [Rider {
        tool: "doppler.branch_config.ensure",
        instance: &branch_rider_instance,
        inputs: &empty_rider_inputs,
        outputs: &stack.branch_outputs,
    }];
    let qa_created_with_rider = created(
        &stack.qa_inputs,
        &stack.qa_outputs,
        stack.qa_window,
        &riders,
    );
    let standing = inverse_of(&cycle.config_tool)
        .standing(&qa_created_with_rider)
        .unwrap_or_else(|err| {
            panic!(
                "-b: `qa`'s own standing (with rider) failed: {}",
                err.message
            )
        });
    expect_as_created(
        &standing,
        "-b qa standing (with the branch tolerated as a rider)",
    );
    cycle.say("-b (qa with the branch tolerated as a rider reads AsCreated): pass");

    let delete_result =
        inverse_of(&cycle.config_tool).delete(&qa_created_with_rider, &teardown_token());
    match &delete_result {
        Ok(deleted) => {
            assert!(!deleted.already_gone);
            let branch_gone = cycle
                .client_get_config_raw(project, stack.branch_config.name().as_str())
                .is_none();
            cycle.observe(format!(
                "verify item 3: DELETE /v3/environments/environment succeeded while a branch \
                 config still existed; Doppler took the branch down with it: {branch_gone}"
            ));
        }
        Err(err) => {
            cycle.observe(format!(
                "verify item 3: DELETE /v3/environments/environment refused while a branch \
                 config still existed (kind {:?}, message {:?}); deleting the branch by hand and \
                 retrying",
                err.kind, err.message
            ));
            cycle
                .raw
                .delete(&format!(
                    "/v3/configs/config?project={project}&config={}",
                    stack.branch_config.name()
                ))
                .unwrap_or_else(|err| {
                    panic!(
                        "-b: hand-deleting the branch config failed (status {:?})",
                        err.status
                    )
                });
            let qa_created_now_clean =
                created(&stack.qa_inputs, &stack.qa_outputs, stack.qa_window, &[]);
            let standing = inverse_of(&cycle.config_tool)
                .standing(&qa_created_now_clean)
                .unwrap_or_else(|err| {
                    panic!(
                        "-b: `qa`'s own standing after cleanup failed: {}",
                        err.message
                    )
                });
            expect_as_created(&standing, "-b qa standing after hand-deleting the branch");
            inverse_of(&cycle.config_tool)
                .delete(&qa_created_now_clean, &teardown_token())
                .unwrap_or_else(|err| {
                    panic!("-b: deleting qa after cleanup failed: {}", err.message)
                });
        }
    }
    cycle.say("-b (verify item 3 recorded; qa is torn down one way or the other): pass");
}

fn sub_cycle_b_project_teardown(
    cycle: &mut Cycle,
    project: &DopplerProject,
    guard: &mut ProjectGuard,
    stack: &StackB,
) {
    let project_created = created(
        &stack.project_inputs,
        &stack.project_outputs,
        stack.project_window,
        &[],
    );
    let standing = inverse_of(&cycle.project_tool)
        .standing(&project_created)
        .unwrap_or_else(|err| panic!("-b: the project's own standing failed: {}", err.message));
    expect_as_created(&standing, "-b project standing");
    inverse_of(&cycle.project_tool)
        .delete(&project_created, &teardown_token())
        .unwrap_or_else(|err| panic!("-b: deleting the project failed: {}", err.message));
    guard.disarm_one(project);
    cycle.say("-b (project torn down): pass");
}

fn sub_cycle_b_rider_and_environment_delete(
    cycle: &mut Cycle,
    project: &DopplerProject,
    guard: &mut ProjectGuard,
) {
    guard.register(project);
    let stack = sub_cycle_b_setup(cycle, project);
    sub_cycle_b_rider_mechanic_and_verify_3(cycle, project, &stack);
    sub_cycle_b_project_teardown(cycle, project, guard, &stack);
}

// ---------------------------------------------------------------------
// Sub-cycle -c: hand-set secret drift.
// ---------------------------------------------------------------------

/// Sub-cycle `-c`: a secret hand-set into the project's default `dev`
/// root config, through a raw `POST` this crate's own tools never call,
/// is never a rider. The project's own `standing` must read `Drifted`,
/// naming the secret; `delete` must refuse. Mirrors
/// `willikins-providers-github`'s own `-c` sub-cycle.
fn sub_cycle_c_hand_set_secret_drift(
    cycle: &mut Cycle,
    project: &DopplerProject,
    guard: &mut ProjectGuard,
) {
    guard.register(project);
    let before = Timestamp::now();
    let project_inputs = project_inputs(project);
    let ensured = cycle
        .project_tool
        .ensure(&project_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("-c: creating the project failed: {}", err.message));
    let window = creation_window(before, Timestamp::now());

    let name = SecretName::parse(HAND_SECRET_NAME).expect("HAND_SECRET_NAME is a valid SecretName");
    let mut secrets = serde_json::Map::new();
    secrets.insert(
        name.as_str().to_string(),
        Json::String(HAND_SECRET_VALUE.to_string()),
    );
    let body = serde_json::json!({
        "project": project.to_string(),
        "config": "dev",
        "secrets": Json::Object(secrets),
    });
    cycle
        .raw
        .post::<Json>("/v3/configs/config/secrets", &body)
        .unwrap_or_else(|err| {
            panic!(
                "-c: the raw POST hand-setting the secret failed (status {:?})",
                err.status
            )
        });

    let project_created = created(&project_inputs, &ensured.outputs, window, &[]);
    let standing = inverse_of(&cycle.project_tool)
        .standing(&project_created)
        .unwrap_or_else(|err| panic!("-c: the project's own standing failed: {}", err.message));
    let what = expect_drifted_containing(&standing, "secret", "-c project standing").to_string();
    cycle.say(&format!(
        "-c (a hand-set secret in dev reads Drifted: {what:?}): pass"
    ));

    let err = inverse_of(&cycle.project_tool)
        .delete(&project_created, &teardown_token())
        .expect_err("-c: delete on a project with a hand-set secret must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "-c: the refusal must be a Conflict"
    );
    cycle.say("-c (delete refuses the secret-drifted project as a Conflict): pass");

    // `-c`'s own project is never meant to reach `AsCreated` (the
    // hand-set secret never goes away on its own): cleaned up by a raw
    // project delete, which removes everything in it regardless of the
    // tool's own refusal above.
    ProjectGuard::delete(&cycle.raw, project).unwrap_or_else(|err| {
        panic!(
            "-c: the raw project delete failed (status {:?})",
            err.status
        )
    });
    match cycle.raw.get::<Json>(&project_path(project)) {
        Err(err) if looks_like_a_missing_project(&err) => {}
        other => panic!("-c: the project is still readable after its raw delete: {other:?}"),
    }
    guard.disarm_one(project);
    cycle.say("-c (project removed by a raw delete after the tool's own refusal): pass");
}

// ---------------------------------------------------------------------
// Sub-cycle -d1/-d2: cross-project inheritance, verify item 4.
// ---------------------------------------------------------------------

/// Sub-cycle `-d1`/`-d2`: `-d1`'s `dev` root config is made inheritable;
/// `-d2`'s `dev` root inherits it. `-d1`'s environment inverse must then
/// read `Drifted`, naming a foreign inheritor. A real `DELETE
/// /v3/environments/environment` on `-d1`'s `dev`, issued while `-d2`
/// still inherits it, records whether Doppler itself refuses or allows
/// it (verify item 4). Cleanup deletes `-d2` before `-d1`, the same
/// order `tests/live_secret_name_gate_cycle.rs`'s own base/child guard
/// already uses, for the same stated reason.
fn sub_cycle_d_cross_project_inheritance(
    cycle: &mut Cycle,
    base: &DopplerProject,
    child: &DopplerProject,
    guard: &mut ProjectGuard,
) {
    guard.register(base);
    guard.register(child);

    // Captured around the base project's own creation: its `dev` root
    // config is created in the same call, so this is the window that
    // call's own `before`/`after` -- not a fresh `now()` taken after
    // both projects already exist, which would fall outside the window
    // entirely and read `NotOurs` before ever reaching the inheritedBy
    // check this sub-cycle exists to exercise.
    let base_before = Timestamp::now();
    cycle
        .project_tool
        .ensure(&project_inputs(base), &sink_token())
        .unwrap_or_else(|err| panic!("-d: creating the base project failed: {}", err.message));
    let base_after = Timestamp::now();
    cycle
        .project_tool
        .ensure(&project_inputs(child), &sink_token())
        .unwrap_or_else(|err| panic!("-d: creating the child project failed: {}", err.message));

    let base_dev = root_config(base, "dev");
    let child_dev = root_config(child, "dev");

    cycle
        .raw
        .post::<Json>(
            "/v3/configs/config/inheritable",
            &serde_json::json!({
                "project": base.to_string(),
                "config": base_dev.name().to_string(),
                "inheritable": true,
            }),
        )
        .unwrap_or_else(|err| {
            panic!(
                "-d: marking base's dev inheritable failed (status {:?})",
                err.status
            )
        });
    cycle
        .raw
        .post::<Json>(
            "/v3/configs/config/inherits",
            &serde_json::json!({
                "project": child.to_string(),
                "config": child_dev.name().to_string(),
                "inherits": [{"project": base.to_string(), "config": base_dev.name().to_string()}],
            }),
        )
        .unwrap_or_else(|err| {
            panic!(
                "-d: making child's dev inherit base's dev failed (status {:?})",
                err.status
            )
        });

    let base_dev_inputs = config_inputs(base, "dev");
    let base_dev_window = creation_window(base_before, base_after);
    // `doppler.config.ensure`'s own read reports `changed: false` for an
    // auto-created root config; its own `Ensured.outputs` carries the
    // derived `config` identity regardless.
    let mut base_dev_outputs = Outputs::new();
    base_dev_outputs.insert(port("config"), Value::known(base_dev.clone()));
    let base_dev_created = created(&base_dev_inputs, &base_dev_outputs, base_dev_window, &[]);

    let standing = inverse_of(&cycle.config_tool)
        .standing(&base_dev_created)
        .unwrap_or_else(|err| panic!("-d: base dev's own standing failed: {}", err.message));
    let what =
        expect_drifted_containing(&standing, "inherited", "-d base dev standing").to_string();
    cycle.say(&format!(
        "-d (verify item 4: base's dev, inherited by a config outside its project, reads \
         Drifted: {what:?}): pass"
    ));

    let delete_result = cycle.raw.delete(&format!(
        "/v3/environments/environment?project={base}&environment={}",
        base_dev.name()
    ));
    match delete_result {
        Ok(()) => cycle.observe(
            "verify item 4: DELETE /v3/environments/environment succeeded on a config a \
             foreign project still inherits"
                .to_string(),
        ),
        Err(err) => cycle.observe(format!(
            "verify item 4: DELETE /v3/environments/environment refused (status {:?}) on a \
             config a foreign project still inherits",
            err.status
        )),
    }

    // Cleanup: child before base, mirroring
    // `tests/live_secret_name_gate_cycle.rs`'s own guard order.
    ProjectGuard::delete(&cycle.raw, child).unwrap_or_else(|err| {
        panic!(
            "-d: deleting the child project failed (status {:?})",
            err.status
        )
    });
    ProjectGuard::delete(&cycle.raw, base).unwrap_or_else(|err| {
        panic!(
            "-d: deleting the base project failed (status {:?})",
            err.status
        )
    });
    guard.disarm_one(child);
    guard.disarm_one(base);
    cycle.say("-d (verify item 4 recorded; child then base torn down): pass");
}

/// The final redaction sweep. The one minted service token's bytes, and
/// anything credential-shaped, must appear in nothing this cycle
/// produced.
fn sweep_for_secrets(cycle: &Cycle) {
    assert_eq!(
        cycle.minted.len(),
        1,
        "the sweep needs the minted token's bytes to be meaningful"
    );
    for (index, text) in cycle.sweep.iter().enumerate() {
        for token in &cycle.minted {
            assert!(
                !text.contains(token),
                "the minted token's bytes reached produced string {index}"
            );
        }
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "produced string {index} looks credential-shaped (`{prefix}`): refusing to let \
                 it stand"
            );
        }
    }
    println!("redaction sweep over {} strings: pass", cycle.sweep.len());
    println!("recorded observations: {:?}", cycle.observations);
}

#[test]
#[ignore = "opt-in live rollback cycle against a real Doppler workplace; creates, drifts, and \
            deletes six throwaway projects. Run with WILLIKINS_LIVE_TESTS=1, the `live-tests` \
            feature, and sandbox credentials sourced in the same command: `source \
            ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
            cargo test -p willikins-providers-doppler --features live-tests --test \
            live_rollback_cycle -j 2 -- --ignored --nocapture`"]
fn doppler_live_rollback_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox Doppler token");
    let mut cycle = Cycle::new(credential);

    let timestamp = unix_seconds();
    let project_a = probe_project(timestamp, "a");
    let project_b = probe_project(timestamp, "b");
    let project_c = probe_project(timestamp, "c");
    let project_d1 = probe_project(timestamp, "d1");
    let project_d2 = probe_project(timestamp, "d2");

    step_0_absent(
        &cycle.raw,
        &[&project_a, &project_b, &project_c, &project_d1, &project_d2],
    );

    let mut guard = ProjectGuard::new(Arc::clone(&cycle.raw));
    println!(
        "the delete guard is created before any write (armed per sub-cycle as each registers)"
    );

    sub_cycle_a_clean_lifo(&mut cycle, &project_a, &mut guard);
    sub_cycle_b_rider_and_environment_delete(&mut cycle, &project_b, &mut guard);
    sub_cycle_c_hand_set_secret_drift(&mut cycle, &project_c, &mut guard);
    sub_cycle_d_cross_project_inheritance(&mut cycle, &project_d1, &project_d2, &mut guard);

    sweep_for_secrets(&cycle);
}

/// This file's own probe project names are valid `DopplerProject`s, well
/// under the 64-character bound -- no network call.
#[test]
fn probe_project_names_are_valid() {
    for tag in ["a", "b", "c", "d1", "d2"] {
        let project = probe_project(1_738_000_000, tag);
        assert!(project.to_string().starts_with(PROBE_PREFIX));
        assert!(project.to_string().len() <= 64);
    }
}

/// `shift`/`creation_window`'s own arithmetic, against `CREATION_SKEW`
/// -- no network call.
#[test]
fn creation_window_applies_the_skew_on_the_right_side() {
    let before = Timestamp::parse("2026-01-01T00:00:00Z").expect("a valid Timestamp");
    let after = Timestamp::parse("2026-01-01T00:00:05Z").expect("a valid Timestamp");
    let window = creation_window(before, after);
    assert_eq!(
        window.start,
        Timestamp::parse("2025-12-31T23:58:00Z").expect("a valid Timestamp")
    );
    assert_eq!(
        window.end,
        Timestamp::parse("2026-01-01T00:02:05Z").expect("a valid Timestamp")
    );
}

/// This file's own constant names are valid against their respective
/// grammars -- no network call.
#[test]
fn constant_names_are_valid() {
    SecretName::parse(HAND_SECRET_NAME).expect("HAND_SECRET_NAME must be a valid SecretName");
    DopplerTokenName::parse(TOKEN_NAME).expect("TOKEN_NAME must be a valid DopplerTokenName");
    DopplerConfigName::parse(BRANCH_SUFFIX_A)
        .expect("BRANCH_SUFFIX_A must be a valid DopplerConfigName");
    DopplerConfigName::parse(BRANCH_SUFFIX_B)
        .expect("BRANCH_SUFFIX_B must be a valid DopplerConfigName");
    EnvironmentSlug::parse(EXTRA_ENVIRONMENT)
        .expect("EXTRA_ENVIRONMENT must be a valid EnvironmentSlug");
}

/// [`HAND_SECRET_VALUE`] must never look like any credential this
/// crate's own `CREDENTIAL_PATTERN` accepts, or Doppler's own service
/// token shape -- `secret_literal_guard.rs`'s own concern, pinned here
/// independently, no network call.
#[test]
fn hand_secret_value_is_not_credential_shaped() {
    for prefix in CREDENTIAL_PREFIXES {
        assert!(
            !HAND_SECRET_VALUE.contains(prefix),
            "HAND_SECRET_VALUE must not look credential-shaped (`{prefix}`)"
        );
    }
}
