//! The live Doppler **write** cycle: the one test in this crate that
//! provisions something real and then removes it again.
//!
//! Compiled only with the crate's `live-tests` feature (its `[[test]]`
//! entry in `Cargo.toml` carries `required-features`), so a plain
//! `cargo test --workspace` never builds it. `#[ignore]` on top of that,
//! and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1` —
//! the credential is read (through
//! [`willikins_providers_doppler::credential_from_env`], itself
//! `Credential::from_env`) only past that gate, exactly as
//! `tests/live_probe.rs` does.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-providers-doppler \
//!   --features live-tests --test live_write_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! It drives the five real tools — never a mock — against a project with
//! the fixed, distinctive name `willikins-live-write-cycle` in the
//! operator's dedicated Doppler test workplace, in ten asserted steps:
//!
//! 1. both fixed project names must be absent (`404`); a leftover from an
//!    aborted run makes the test refuse to proceed and name it, because a
//!    leftover is the operator's to remove by hand;
//! 2. a [`ProjectGuard`] is armed, so every exit path — a panic, a failed
//!    assertion, an early return — deletes every project this run
//!    created;
//! 3. `doppler.project.ensure` reads `Absent`, creates the project with
//!    the `managed-by: willikins` description, and converges
//!    (`changed: false`, the project body unchanged);
//! 4. a foreign project, created by a raw `POST` with a different
//!    description, reads `Foreign`, `ensure`s as `Conflict`, and is
//!    untouched afterwards;
//! 5. `doppler.config.ensure` for `dev`, `stg` and `prd` — the names
//!    `naming::v1::doppler_root_config` derives, exactly as the positive
//!    fixture's plan derives them — each reads `Present` and each
//!    `ensure`s `changed: false`, with the project's environment list
//!    identical before and after: review resolution 15's claim, live and
//!    mock-free. Then `qa`, which Doppler does not auto-create, is
//!    created;
//! 6. `doppler.service_token.ensure` mints a real token: `Absent` with
//!    the value `Unknown`, then `changed: true` with it `Known`, then
//!    `Present` with it `Unknown` again (Doppler never re-issues a
//!    token's bytes), then `changed: false` with the token list
//!    unchanged;
//! 7. `doppler.service_token.rotate` leaves exactly one token of that
//!    name, under a different slug and with different bytes;
//! 8. `doppler.secret.get` reads Doppler's auto-injected
//!    `DOPPLER_PROJECT`, whose value is the project's own name, and a
//!    name that does not exist is `NotFound` naming the key;
//! 9. every endpoint reached whose body this crate can see is recorded,
//!    redacted, under `fixtures/doppler/live/` and compared by top-level
//!    key set with the authored fixture of the same endpoint; the
//!    read-only observations the milestone plan still wants are printed;
//! 10. both projects are deleted explicitly, neither can be read back,
//!     and the guard is disarmed.
//!
//! Doppler documents `DELETE /v3/projects/project` as taking the project
//! in the **request body** (`{"project": "<name>"}`, its only required
//! field) and answering `200` with `{"success": true}`; the environment
//! delete is `DELETE /v3/environments/environment` with `project` and
//! `environment` as **query** parameters (`projects-delete.md` and
//! `environments-delete.md`, both fetched verbatim 2026-09-14; neither
//! page defines any non-2xx response). Deleting a project removes its
//! environments, configs and service tokens with it, so the guard needs
//! nothing but the project delete.
//! [`willikins_providers_http::Http::delete_with_body`] collapses every
//! `2xx` into `Ok(())` and hands back no status or body, so step 10
//! asserts the success and the following `404` rather than a literal
//! `200` or `{"success": true}`.
//!
//! **Deletion stays in this test.** No tool and no client method of
//! `willikins-providers-doppler` gains a project or environment delete.
//! The guard reaches [`willikins_providers_http::Http`] directly, on its
//! own client, which the tools do not share.
//!
//! **Redaction.** Two service tokens' bytes really pass through this
//! process: the one `doppler.service_token.ensure` mints and the one
//! `doppler.service_token.rotate` mints. Both are collected (through the
//! derive's own `expose`, with this test's sink token — never
//! `expose_secret`) and swept for at the end, together with the token
//! prefixes `willikins_providers_doppler::CREDENTIAL_PATTERN` accepts
//! and the `dp.st.` prefix Doppler issues, over every string this cycle
//! produced and every fixture it recorded. The real credential's bytes
//! are never read into this test: it never calls `std::env::var` on
//! `WILLIKINS_DOPPLER_TOKEN`, because a second raw read would create
//! exactly the plaintext `Credential` exists to prevent.
//!
//! The one secret this cycle reads back, `DOPPLER_PROJECT`'s value, is
//! deliberately **not** swept for: Doppler sets it to the project's own
//! name, which every line of this test prints legitimately, so a byte
//! sweep for it would either fail on nothing or prove nothing. What is
//! asserted instead is the property that matters — the `Value` holding
//! it renders and `Debug`s as `[REDACTED DopplerSecretValue]`, and its
//! equality with the expected value is checked through the derive's
//! generated `PartialEq`, which never produces a printable string.
//!
//! A second `#[ignore]` test in this file,
//! `the_cycles_projects_are_gone`, is the after-the-run confirmation: it
//! only `GET`s the two fixed names and asserts `404`. It needs
//! `WILLIKINS_LIVE_LEFTOVER_CHECK=1` on top of `WILLIKINS_LIVE_TESTS=1`,
//! so the cycle's own command above never runs it at all.
//!
//! Setting that second variable does **not**, on its own, keep the two
//! apart: it makes both tests eligible in the same binary, and the
//! harness would run them side by side. Name the test when running the
//! leftover check, so the cycle cannot start creating the very projects
//! the check is asserting are gone:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   WILLIKINS_LIVE_LEFTOVER_CHECK=1 RUST_TEST_THREADS=2 cargo test \
//!   -p willikins-providers-doppler --features live-tests --test live_write_cycle \
//!   -j 2 -- --ignored --nocapture the_cycles_projects_are_gone
//! ```

mod common;

use std::sync::Arc;

use serde_json::Value as Json;
use willikins_core::{
    Ensured, Inputs, Observation, PortName, SinkToken, Tool, ToolError, ToolErrorKind, Value,
};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigEnsure, DopplerProjectEnsure, DopplerSecretGet,
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate, MANAGED_DESCRIPTION, credential_from_env,
    http_client,
};
use willikins_providers_http::Http;
use willikins_types::{
    DomainType, DopplerConfig, DopplerProject, DopplerSecretValue, DopplerServiceToken,
    DopplerTokenName, EnvironmentSlug, SecretName, naming,
};

use common::{live_dir, record_and_compare, record_raw, top_level_keys};

/// The cycle's fixed project name. Distinctive on purpose: anything by
/// this name in the test workplace is this test's, and a leftover one is
/// deleted by hand.
const PROJECT_NAME: &str = "willikins-live-write-cycle";

/// The project step 4 creates behind willikins' back, to prove a project
/// that is not ours reads `Foreign` and `ensure`s as `Conflict`.
const FOREIGN_PROJECT_NAME: &str = "willikins-live-write-cycle-foreign";

/// The foreign project's description. Anything but
/// [`MANAGED_DESCRIPTION`] makes it foreign; this says so in words, in
/// case a human meets it in the dashboard after an aborted run.
const FOREIGN_DESCRIPTION: &str = "not willikins: the live write cycle's foreign-project case";

/// The service token this cycle mints, twice (once by `ensure`, once by
/// `rotate`).
const TOKEN_NAME: &str = "live-write-cycle";

/// The environments the positive fixture's plan derives root configs for.
const DEFAULT_ENVIRONMENTS: &[&str] = &["dev", "stg", "prd"];

/// The environment Doppler does not auto-create, which step 5 does.
const EXTRA_ENVIRONMENT: &str = "qa";

/// Doppler auto-injects this secret into every config, setting it to the
/// project's own name — which is what step 8 asserts.
const WELL_KNOWN_SECRET: &str = "DOPPLER_PROJECT";

/// A secret name that cannot exist, for the `NotFound` half of step 8.
const MISSING_SECRET: &str = "WILLIKINS_DOES_NOT_EXIST";

/// Token prefixes that must appear in nothing this test produces: the two
/// [`willikins_providers_doppler::CREDENTIAL_PATTERN`] accepts, the one
/// Doppler issues service tokens under, and the `Authorization` scheme
/// their bytes would travel in.
const CREDENTIAL_PREFIXES: &[&str] = &["dp.sa.", "dp.pt.", "dp.st.", "Bearer"];

/// The test's own sink token. `SinkToken::new` is disallowed outside the
/// apply executor; an executor-level test opts in narrowly, which is the
/// convention every such test in this workspace uses.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// A port name, parsed. Every name this file passes is a literal.
fn port(name: &'static str) -> PortName {
    PortName::parse(name).expect("the cycle's port names are valid")
}

/// `GET /v3/projects/project?project=<name>`.
fn project_path(project: &DopplerProject) -> String {
    format!("/v3/projects/project?project={project}")
}

/// `GET /v3/configs/config?project=<project>&config=<name>`.
fn config_path(config: &DopplerConfig) -> String {
    format!(
        "/v3/configs/config?project={}&config={}",
        config.project(),
        config.name()
    )
}

/// `GET /v3/configs/config/tokens?project=<project>&config=<name>`.
fn tokens_path(config: &DopplerConfig) -> String {
    format!(
        "/v3/configs/config/tokens?project={}&config={}",
        config.project(),
        config.name()
    )
}

/// `doppler.project.ensure`'s one input. A free function, not a method:
/// it needs nothing from the cycle but the identity it is given, and
/// step 4 builds it for the *foreign* project.
fn project_inputs(project: &DopplerProject) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project.clone()));
    inputs
}

/// `doppler.secret.get`'s inputs, for a config and a secret name.
fn secret_inputs(config: &DopplerConfig, name: &str) -> Inputs {
    let name = SecretName::parse(name).expect("the cycle's secret names are valid");
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config.clone()));
    inputs.insert(port("name"), Value::known(name));
    inputs
}

/// The config `naming::v1` derives for one environment of this cycle's
/// project — the same derivation the positive fixture's plan performs.
fn derived_config(project: &DopplerProject, environment: &str) -> DopplerConfig {
    let environment =
        EnvironmentSlug::parse(environment).expect("the cycle's environments are valid slugs");
    naming::v1::doppler_root_config(project, &environment)
}

/// Deletes every project this run created, on every exit path — a panic,
/// a failed assertion, or an early return — unless the test body already
/// deleted them and disarmed the guard. Deleting a project takes its
/// environments, configs and tokens with it.
///
/// A `Drop` implementation must not panic while the thread is already
/// panicking (that aborts the process and hides the original failure), so
/// a failed delete during a panic prints the project's full name loudly
/// and returns; it panics only when it is itself the first failure.
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

    /// Take responsibility for `project`. Called *before* the request
    /// that may create it, so a create that landed and then failed is
    /// still cleaned up.
    fn register(&mut self, project: &DopplerProject) {
        self.projects.push(project.clone());
    }

    fn disarm(&mut self) {
        self.projects.clear();
    }

    /// `DELETE /v3/projects/project` with `{"project": "<name>"}`.
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
                // Nothing left to delete: the body removed it and failed
                // afterwards, before disarming. Doppler answers `400`,
                // not `404`, when asked to delete a project that is not
                // there — observed live on 2026-09-14, when this guard
                // ran straight after step 10's own successful `DELETE`
                // and reported a leftover that did not exist. Both
                // statuses mean gone; `the_cycles_projects_are_gone` is
                // the backstop that says so independently.
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
                        "the guard could not delete `{project}` (status {status:?}); \
                         it must be deleted by hand"
                    );
                }
            }
        }
    }
}

/// Everything one run of the cycle needs: a raw HTTP channel (shared with
/// the guard) for the `GET`s, the `POST`s no tool performs, and the
/// `DELETE`s; the five real tools; the identities; and the buffers every
/// produced string and every minted token's bytes land in.
struct Cycle {
    raw: Arc<Http>,
    project_tool: DopplerProjectEnsure,
    config_tool: DopplerConfigEnsure,
    token_tool: DopplerServiceTokenEnsure,
    rotate_tool: DopplerServiceTokenRotate,
    secret_tool: DopplerSecretGet,
    project: DopplerProject,
    foreign: DopplerProject,
    token_name: DopplerTokenName,
    /// Every string this cycle produced, swept at the end.
    sweep: Vec<String>,
    /// The plaintext of every token Doppler actually minted here. Never
    /// printed, never written: the needles of the final sweep.
    minted: Vec<String>,
    /// The live fixtures recorded so far, and the drift found in them.
    recorded: Vec<String>,
    fixture_failures: Vec<String>,
    /// The read-only observations the milestone plan asks for.
    observations: Vec<String>,
}

impl Cycle {
    /// Print one line and keep it for the final redaction sweep: this
    /// test prints only step names, pass/fail, and observations — never a
    /// response body.
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    /// Keep a produced string (a `Debug`, a `render`, a `ToolError`
    /// message) for the final sweep without printing it.
    fn note(&mut self, text: String) {
        self.sweep.push(text);
    }

    /// Record one read-only observation, printed and kept.
    fn observe(&mut self, text: String) {
        println!("observation: {text}");
        self.observations.push(text.clone());
        self.sweep.push(text);
    }

    /// A raw `GET` that must succeed.
    fn get(&self, step: &str, path: &str) -> Json {
        self.raw
            .get::<Json>(path)
            .unwrap_or_else(|err| panic!("{step}: a raw GET failed (status {:?})", err.status))
    }

    /// Record a live response under `fixture`, redacted, and compare its
    /// top-level key set with the authored fixture of the same name.
    fn record_fixture(&mut self, fixture: &'static str, body: &Json) {
        self.recorded.push(fixture.to_string());
        match record_and_compare(fixture, body) {
            Ok(()) => self.say(&format!("fixture `{fixture}`: pass")),
            Err(diff) => {
                self.say(&format!("fixture `{fixture}`: fail"));
                self.fixture_failures.push(format!("{fixture}: {diff}"));
            }
        }
    }

    /// Record an `Observation` and every value it carries, redaction and
    /// all, into the sweep.
    fn record_observation(&mut self, step: &str, observation: &Observation) {
        self.note(format!("{step} observation: {observation:?}"));
        if let Observation::Present(outputs) | Observation::Absent { predicted: outputs } =
            observation
        {
            let rendered: Vec<String> = outputs
                .iter()
                .map(|(port, value)| format!("{port}={}", value.render()))
                .collect();
            self.note(format!("{step} rendered: {rendered:?}"));
        }
    }

    fn record_ensured(&mut self, step: &str, ensured: &Ensured) {
        self.note(format!("{step} ensured: {ensured:?}"));
        let rendered: Vec<String> = ensured
            .outputs
            .iter()
            .map(|(port, value)| format!("{port}={}", value.render()))
            .collect();
        self.note(format!("{step} rendered: {rendered:?}"));
    }

    fn record_error(&mut self, step: &str, err: &ToolError) {
        self.note(format!("{step} error: {err:?}"));
        self.note(err.message.clone());
    }

    fn config_inputs(&self, environment: &str) -> Inputs {
        let environment =
            EnvironmentSlug::parse(environment).expect("the cycle's environments are valid slugs");
        let mut inputs = Inputs::new();
        inputs.insert(port("project"), Value::known(self.project.clone()));
        inputs.insert(port("environment"), Value::known(environment));
        inputs
    }

    fn token_inputs(&self, config: &DopplerConfig) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(port("config"), Value::known(config.clone()));
        inputs.insert(port("name"), Value::known(self.token_name.clone()));
        inputs
    }

    /// The bytes of a `token` output, kept for the final sweep and
    /// returned so the caller can compare two mintings. Reached through
    /// the derive's generated `expose`, the designed way out of a secret
    /// domain type, never through `expose_secret`.
    fn take_token_bytes(&mut self, step: &str, ensured: &Ensured) -> String {
        let value = ensured
            .outputs
            .get(&port("token"))
            .unwrap_or_else(|| panic!("{step}: the ensure returned no `token` output"));
        assert!(
            value.is_known(),
            "{step}: a freshly minted token's value is Known"
        );
        let token = value
            .downcast::<DopplerServiceToken>()
            .unwrap_or_else(|| panic!("{step}: the `token` output is not a DopplerServiceToken"));
        let bytes = token.expose(&sink_token()).to_string();
        assert!(
            DopplerServiceToken::parse(&bytes).is_ok(),
            "{step}: the minted token does not parse as a DopplerServiceToken"
        );
        assert_eq!(
            value.render().to_string(),
            "[REDACTED DopplerServiceToken]",
            "{step}: a secret value must render as its redaction marker"
        );
        self.minted.push(bytes.clone());
        bytes
    }
}

/// Step 1: neither fixed name may exist. A leftover from an aborted run
/// is the operator's to remove by hand, so the test refuses to proceed
/// and names it rather than deleting something it did not create here.
fn step_1_absent(raw: &Http, projects: &[&DopplerProject]) {
    for project in projects {
        match raw.get::<Json>(&project_path(project)) {
            Err(err) if err.status == Some(404) => {}
            Ok(_) => panic!(
                "step 1: `{project}` already exists, left behind by an aborted run; \
                 a leftover is the operator's to delete by hand, so this test refuses to proceed"
            ),
            Err(err) => panic!(
                "step 1: GET of `{project}` answered an unexpected status {:?}",
                err.status
            ),
        }
    }
    println!("step 1 (both fixed project names are 404): pass");
}

/// Step 2b: the 2026-09-16 smoke-run defect, live. Both token tools
/// `read` a token in a config of the project step 1 has just proved does
/// not exist, and both must answer `Absent` rather than failing. That is
/// the whole of what the fix changed, and no other step here meets it:
/// step 3 creates the project before anything ever lists a token, which
/// is exactly why this cycle was green while the live smoke run died four
/// seconds in.
///
/// Numbered `2b` beside the guard rather than inserted as a new step, so
/// the steps this file's own docs and sweep accounting name keep their
/// numbers (the same reason steps `5a`/`5b` and `9a`/`9b`/`9c` are
/// lettered).
///
/// **Not yet run live** (added 2026-09-16, after the smoke run, in a
/// session with no credentials). It depends on the status that run itself
/// observed — `404`, "Could not find requested project" — recorded as
/// `fixtures/doppler/service_tokens_list_project_missing.json`. If Doppler
/// answers something else here, a `400` most plausibly (it answers that
/// for a project deleted moments earlier; see step 10), this step fails,
/// and that failure is the finding:
/// `DopplerServiceTokenEnsure::is_listed`'s tolerance is one status wide
/// on purpose.
fn step_2b_token_read_before_the_project_exists(cycle: &mut Cycle) {
    let config = derived_config(&cycle.project, "dev");
    let inputs = cycle.token_inputs(&config);

    let observation = cycle
        .token_tool
        .read(&inputs)
        .expect("step 2b: doppler.service_token.ensure must read, not fail, with no parent");
    cycle.record_observation("step 2b ensure read (no parent yet)", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 2b: doppler.service_token.ensure must read Absent before its project exists"
    );

    let rotation = cycle
        .rotate_tool
        .read(&inputs)
        .expect("step 2b: doppler.service_token.rotate must read, not fail, with no parent");
    cycle.record_observation("step 2b rotate read (no parent yet)", &rotation);
    assert!(
        matches!(rotation, Observation::Absent { .. }),
        "step 2b: doppler.service_token.rotate must read Absent before its project exists"
    );

    cycle.say("step 2b (both token reads are Absent before the project exists): pass");
}

/// Step 3: `read` is `Absent`, `ensure` creates the project with the
/// ownership marker and reports `changed: true`, a raw `GET` shows the
/// description, `read` is now `Present`, and a second `ensure` is
/// `changed: false` with the project body identical before and after.
fn step_3_project(cycle: &mut Cycle) {
    let inputs = project_inputs(&cycle.project);
    let observation = cycle
        .project_tool
        .read(&inputs)
        .expect("step 3: read of an absent project");
    cycle.record_observation("step 3 read (absent)", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 3: read of an absent project should be Absent"
    );

    let ensured = cycle
        .project_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 3: ensure failed ({:?}): {}", err.kind, err.message));
    cycle.record_ensured("step 3 ensure (create)", &ensured);
    assert!(ensured.changed, "step 3: a create reports changed: true");

    let before = cycle.get("step 3", &project_path(&cycle.project));
    assert_eq!(
        before
            .pointer("/project/description")
            .and_then(Json::as_str),
        Some(MANAGED_DESCRIPTION),
        "step 3: the created project carries the ownership marker"
    );
    cycle.record_fixture("project_get_present", &before);

    let observation = cycle
        .project_tool
        .read(&inputs)
        .expect("step 3: read of the project just created");
    cycle.record_observation("step 3 read (present)", &observation);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 3: read of our own project should be Present"
    );

    let ensured = cycle
        .project_tool
        .ensure(&inputs, &sink_token())
        .expect("step 3: a second ensure of a converged project");
    cycle.record_ensured("step 3 ensure (converged)", &ensured);
    assert!(
        !ensured.changed,
        "step 3: a second ensure reports changed: false"
    );
    let after = cycle.get("step 3", &project_path(&cycle.project));
    assert_eq!(
        serde_json::to_string(&before).expect("serializes"),
        serde_json::to_string(&after).expect("serializes"),
        "step 3: the second ensure changed the project body"
    );
    cycle.say("step 3 (create, marker, converge, body unchanged): pass");
}

/// Step 4: a project that exists but is not ours. Created here by a raw
/// `POST` with a different description, it reads `Foreign`, `ensure`s as
/// `Conflict`, and is untouched afterwards.
fn step_4_foreign(cycle: &mut Cycle) {
    let created = cycle
        .raw
        .post::<Json>(
            "/v3/projects",
            &serde_json::json!({
                "name": cycle.foreign.to_string(),
                "description": FOREIGN_DESCRIPTION,
            }),
        )
        .unwrap_or_else(|err| {
            panic!(
                "step 4: the raw POST creating `{}` failed (status {:?})",
                cycle.foreign, err.status
            )
        });
    cycle.record_fixture("project_post_created", &created);

    let inputs = project_inputs(&cycle.foreign);
    let before = cycle.get("step 4", &project_path(&cycle.foreign));
    cycle.record_fixture("project_get_foreign", &before);

    let observation = cycle
        .project_tool
        .read(&inputs)
        .expect("step 4: read of a foreign project");
    cycle.record_observation("step 4 read", &observation);
    assert!(
        matches!(observation, Observation::Foreign),
        "step 4: a project without the marker reads Foreign, observed {observation:?}"
    );

    let err = cycle
        .project_tool
        .ensure(&inputs, &sink_token())
        .expect_err("step 4: ensure on a foreign project must conflict");
    cycle.record_error("step 4", &err);
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 4: a foreign project is a Conflict"
    );

    let after = cycle.get("step 4", &project_path(&cycle.foreign));
    assert_eq!(
        serde_json::to_string(&before).expect("serializes"),
        serde_json::to_string(&after).expect("serializes"),
        "step 4: the refused ensure changed the foreign project"
    );
    cycle.say("step 4 (a foreign project reads Foreign, conflicts, and is untouched): pass");
}

/// The environment identifiers a `GET /v3/environments` body lists.
fn environment_ids(body: &Json) -> Vec<String> {
    body.get("environments")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|env| env.get("id").and_then(Json::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Step 5a: review resolution 15, live. Doppler creates `dev`, `stg` and
/// `prd` with the project, so each of the three reads `Present` and each
/// `ensure` is `changed: false`. The mock-free proof that no `POST`
/// happened is the project's environment list being the same before and
/// after all six calls.
fn step_5a_default_configs(cycle: &mut Cycle) {
    let list_path = format!("/v3/environments?project={}", cycle.project);
    let before = cycle.get("step 5a", &list_path);
    record_raw("environments_list_before", &before);

    for environment in DEFAULT_ENVIRONMENTS {
        let inputs = cycle.config_inputs(environment);
        let observation = cycle
            .config_tool
            .read(&inputs)
            .unwrap_or_else(|err| panic!("step 5a: read of `{environment}` failed: {err:?}"));
        cycle.record_observation(&format!("step 5a read ({environment})"), &observation);
        assert!(
            matches!(observation, Observation::Present(_)),
            "step 5a: `{environment}`'s root config is auto-created, so it reads Present; \
             observed {observation:?}"
        );

        let ensured = cycle
            .config_tool
            .ensure(&inputs, &sink_token())
            .unwrap_or_else(|err| panic!("step 5a: ensure of `{environment}` failed: {err:?}"));
        cycle.record_ensured(&format!("step 5a ensure ({environment})"), &ensured);
        assert!(
            !ensured.changed,
            "step 5a: an auto-created root config ensures changed: false"
        );
    }

    let after = cycle.get("step 5a", &list_path);
    record_raw("environments_list_after", &after);
    assert_eq!(
        environment_ids(&before),
        environment_ids(&after),
        "step 5a: an ensure created an environment it should have found"
    );
    let identical = serde_json::to_string(&before).expect("serializes")
        == serde_json::to_string(&after).expect("serializes");
    cycle.observe(format!(
        "the environment list's full body was identical before and after the three \
         converging ensures: {identical}"
    ));

    let dev = derived_config(&cycle.project, "dev");
    let body = cycle.get("step 5a", &config_path(&dev));
    cycle.record_fixture("config_get_present", &body);
    assert_eq!(
        body.pointer("/config/root").and_then(Json::as_bool),
        Some(true),
        "step 5a: an environment's own config is a root config"
    );
    cycle.say(
        "step 5a (dev, stg, prd read Present, ensure changed: false, no environment created): pass",
    );
}

/// Step 5b: an environment Doppler does not auto-create. `read` is
/// `Absent`, `ensure` creates it with `name` and `slug` both equal to the
/// derived config name, and `read` is `Present`.
fn step_5b_extra_config(cycle: &mut Cycle) {
    let inputs = cycle.config_inputs(EXTRA_ENVIRONMENT);
    let observation = cycle
        .config_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 5b: read of `qa` failed: {err:?}"));
    cycle.record_observation("step 5b read (absent)", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 5b: an environment Doppler did not create reads Absent; observed {observation:?}"
    );

    let ensured = cycle
        .config_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 5b: ensure of `qa` failed: {err:?}"));
    cycle.record_ensured("step 5b ensure (create)", &ensured);
    assert!(ensured.changed, "step 5b: a create reports changed: true");

    let body = cycle.get(
        "step 5b",
        &format!(
            "/v3/environments/environment?project={}&environment={EXTRA_ENVIRONMENT}",
            cycle.project
        ),
    );
    assert_eq!(
        body.pointer("/environment/id").and_then(Json::as_str),
        Some(EXTRA_ENVIRONMENT),
        "step 5b: the created environment's slug is the derived config name"
    );
    assert_eq!(
        body.pointer("/environment/name").and_then(Json::as_str),
        Some(EXTRA_ENVIRONMENT),
        "step 5b: the created environment's name is the derived config name"
    );
    // Doppler's create and get answer the same `{"environment": {...}}`
    // envelope (`environments-create.md`, `environments-get.md`), and
    // `create_environment` discards its response body, so the authored
    // create fixture is verified against this `GET` of what the create
    // just made.
    cycle.record_fixture("environment_post_created", &body);

    let observation = cycle
        .config_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 5b: second read of `qa` failed: {err:?}"));
    cycle.record_observation("step 5b read (present)", &observation);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 5b: the environment just created reads Present; observed {observation:?}"
    );
    cycle.say("step 5b (qa is Absent, created with name and slug `qa`, then Present): pass");
}

/// Step 6: `doppler.service_token.ensure`. `Absent` with the value
/// `Unknown`, then `changed: true` with it `Known`, then `Present` with
/// it `Unknown` again, then `changed: false` with the token list
/// unchanged. Returns the minted token's slug, for step 7 to compare.
fn step_6_token(cycle: &mut Cycle, config: &DopplerConfig) -> String {
    let inputs = cycle.token_inputs(config);
    let absent = cycle.get("step 6", &tokens_path(config));
    cycle.record_fixture("service_tokens_list_absent", &absent);
    assert_eq!(
        token_slugs(&absent, &cycle.token_name),
        Vec::<String>::new(),
        "step 6: a fresh config holds no token by this name"
    );

    let observation = cycle
        .token_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 6: read failed: {err:?}"));
    cycle.record_observation("step 6 read (absent)", &observation);
    match &observation {
        Observation::Absent { predicted } => assert!(
            !predicted
                .get(&port("token"))
                .expect("step 6: the read reports a `token` port")
                .is_known(),
            "step 6: an unminted token's value is Unknown"
        ),
        other => panic!("step 6: expected Absent, observed {other:?}"),
    }

    let ensured = cycle
        .token_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 6: the mint failed ({:?}): {}", err.kind, err.message));
    cycle.record_ensured("step 6 ensure (mint)", &ensured);
    assert!(ensured.changed, "step 6: a mint reports changed: true");
    let minted = cycle.take_token_bytes("step 6", &ensured);

    let observation = cycle
        .token_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 6: read after the mint failed: {err:?}"));
    cycle.record_observation("step 6 read (present)", &observation);
    match &observation {
        Observation::Present(outputs) => assert!(
            !outputs
                .get(&port("token"))
                .expect("step 6: the read reports a `token` port")
                .is_known(),
            "step 6: a minted token's value can never be re-read, so it stays Unknown"
        ),
        other => panic!("step 6: expected Present, observed {other:?}"),
    }

    let present = cycle.get("step 6", &tokens_path(config));
    cycle.record_fixture("service_tokens_list_present", &present);
    let slugs = token_slugs(&present, &cycle.token_name);
    assert_eq!(
        slugs.len(),
        1,
        "step 6: exactly one token by this name exists after the mint"
    );

    let ensured = cycle
        .token_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 6: the second ensure failed: {err:?}"));
    cycle.record_ensured("step 6 ensure (converged)", &ensured);
    assert!(
        !ensured.changed,
        "step 6: a listed token ensures changed: false"
    );
    assert!(
        !ensured
            .outputs
            .get(&port("token"))
            .expect("step 6: the ensure reports a `token` port")
            .is_known(),
        "step 6: a converged ensure cannot produce the token's value"
    );
    let again = cycle.get("step 6", &tokens_path(config));
    assert_eq!(
        token_slugs(&again, &cycle.token_name),
        slugs,
        "step 6: the second ensure minted a second token"
    );
    assert!(
        DopplerServiceToken::parse(&minted).is_ok(),
        "step 6: the minted token parses"
    );
    cycle.say("step 6 (absent, minted Known, present Unknown, second ensure changed: false): pass");
    slugs.into_iter().next().expect("exactly one slug")
}

/// The slugs of every listed token named `name`.
fn token_slugs(body: &Json, name: &DopplerTokenName) -> Vec<String> {
    body.get("tokens")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|token| token.get("name").and_then(Json::as_str) == Some(name.as_str()))
                .filter_map(|token| token.get("slug").and_then(Json::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Step 7: `doppler.service_token.rotate` revokes and re-mints. Exactly
/// one token of that name is left, under a different slug, with different
/// bytes, and the call reports `changed: true`.
fn step_7_rotate(cycle: &mut Cycle, config: &DopplerConfig, slug_before: &str) {
    let inputs = cycle.token_inputs(config);
    let observation = cycle
        .rotate_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 7: read failed: {err:?}"));
    cycle.record_observation("step 7 read", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 7: a Destructive step must never plan as NoOp, so rotate always reads Absent"
    );

    let ensured = cycle
        .rotate_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 7: the rotation failed ({:?}): {}",
                err.kind, err.message
            )
        });
    cycle.record_ensured("step 7 ensure (rotate)", &ensured);
    assert!(ensured.changed, "step 7: a rotation reports changed: true");
    let rotated = cycle.take_token_bytes("step 7", &ensured);
    // `assert!`, never `assert_ne!`: a failing `assert_ne!` prints both
    // operands, and both operands here are live service tokens.
    assert!(
        cycle.minted.first() != Some(&rotated),
        "step 7: the rotation returned the same bytes as the first minting"
    );

    let after = cycle.get("step 7", &tokens_path(config));
    let slugs = token_slugs(&after, &cycle.token_name);
    assert_eq!(
        slugs.len(),
        1,
        "step 7: a rotation leaves exactly one token of that name, found {}",
        slugs.len()
    );
    assert_ne!(
        slugs[0], slug_before,
        "step 7: the rotated token must have a new slug"
    );
    cycle.say("step 7 (rotate leaves one token, new slug, new bytes, changed: true): pass");
}

/// Step 8: `doppler.secret.get`. Doppler's auto-injected
/// `DOPPLER_PROJECT` reads `Known` and equals the project's own name; a
/// name that does not exist is `NotFound` naming the key.
fn step_8_secret(cycle: &mut Cycle, config: &DopplerConfig) {
    let inputs = secret_inputs(config, WELL_KNOWN_SECRET);
    let observation = cycle
        .secret_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 8: reading `{WELL_KNOWN_SECRET}` failed: {err:?}"));
    cycle.record_observation("step 8 read", &observation);
    let Observation::Present(outputs) = &observation else {
        panic!("step 8: a secret that exists reads Present, observed {observation:?}");
    };
    let value = outputs
        .get(&port("value"))
        .expect("step 8: the read reports a `value` port");
    assert!(
        value.is_known(),
        "step 8: an existing secret's value is Known"
    );
    assert_eq!(
        value.render().to_string(),
        "[REDACTED DopplerSecretValue]",
        "step 8: a secret value renders as its redaction marker"
    );
    // Compared, never printed: `DopplerSecretValue`'s generated
    // `PartialEq` produces no string at all. Doppler sets
    // `DOPPLER_PROJECT` to the project's own name.
    let expected = DopplerSecretValue::parse(PROJECT_NAME).expect("the project name is a value");
    assert_eq!(
        value.downcast::<DopplerSecretValue>(),
        Some(&expected),
        "step 8: DOPPLER_PROJECT's value is the project's own name"
    );

    let body = cycle.get(
        "step 8",
        &format!(
            "/v3/configs/config/secret?project={}&config={}&name={WELL_KNOWN_SECRET}",
            config.project(),
            config.name()
        ),
    );
    cycle.record_fixture("secret_get", &body);

    let missing = secret_inputs(config, MISSING_SECRET);
    let err = cycle
        .secret_tool
        .read(&missing)
        .expect_err("step 8: a secret that does not exist must fail");
    cycle.record_error("step 8 (missing)", &err);
    // Recorded before it is asserted: what Doppler actually answers for a
    // secret that does not exist is undocumented (the research note's
    // `secrets-get` page defines only a `200`), and the answer decides
    // whether the plan's `404 -> NotFound` row is reachable at all. The
    // message is willikins' own, bounded and built from the provider's
    // `message` field only, and a missing secret has no value to carry.
    cycle.observe(format!(
        "doppler.secret.get on a name that does not exist: kind {:?}, message {:?}",
        err.kind, err.message
    ));
    assert_eq!(
        err.kind,
        ToolErrorKind::NotFound,
        "step 8: a missing secret is NotFound, not {:?} ({})",
        err.kind,
        err.message
    );
    assert!(
        err.message.contains(MISSING_SECRET) && err.message.contains(&config.to_string()),
        "step 8: the NotFound message names the key it looked for: {}",
        err.message
    );
    cycle.say("step 8 (DOPPLER_PROJECT is the project name; a missing name is NotFound): pass");
}

/// Step 9a: the open question of the milestone plan's "Verify before
/// relying on them" item 4 that only a write answers — whether Doppler's
/// environment slug accepts the underscore `naming::v1`'s snake join
/// emits for a multi-word environment (`pre-prod` -> `pre_prod`).
///
/// An **observation, not an assertion**: a refusal here would mean
/// `naming::v1` can emit a name Doppler will not take, which the plan
/// says is fixed by a `naming::v2` row and never by editing `v1` — a
/// decision for the plan, not a failure of this cycle.
///
/// Doppler caps a Developer-plan project at four environments and this
/// project already holds four (`dev`, `stg`, `prd`, `qa`), so a first
/// refusal is ambiguous. Two follow-ups disambiguate it: free a slot by
/// deleting `qa` (this test created it and this test is about to delete
/// the whole project anyway) and retry, then fall back to a control
/// environment whose name is plain lower-case letters. Only "the
/// underscore was refused while the control was accepted" is a finding.
fn step_9a_underscore_environment(cycle: &mut Cycle) {
    let first = cycle
        .config_tool
        .ensure(&cycle.config_inputs("pre-prod"), &sink_token());
    if let Ok(ensured) = &first {
        cycle.record_ensured("step 9a ensure (pre-prod)", ensured);
        cycle.observe(
            "naming::v1's snake join is live-safe: Doppler accepted an environment whose \
             name and slug are `pre_prod`"
                .to_string(),
        );
        return;
    }
    if let Err(err) = &first {
        cycle.record_error("step 9a (pre-prod, first attempt)", err);
    }

    // Free one environment slot, in case the refusal was the cap.
    let freed = cycle.raw.delete(&format!(
        "/v3/environments/environment?project={}&environment={EXTRA_ENVIRONMENT}",
        cycle.project
    ));
    cycle.observe(format!(
        "deleted the `{EXTRA_ENVIRONMENT}` environment to free a slot: {}",
        if freed.is_ok() { "ok" } else { "failed" }
    ));

    let retry = cycle
        .config_tool
        .ensure(&cycle.config_inputs("pre-prod"), &sink_token());
    match &retry {
        Ok(ensured) => {
            cycle.record_ensured("step 9a ensure (pre-prod, retry)", ensured);
            cycle.observe(
                "naming::v1's snake join is live-safe: Doppler accepted `pre_prod` once a \
                 slot was free, so the first refusal was the project's environment cap"
                    .to_string(),
            );
            return;
        }
        Err(err) => cycle.record_error("step 9a (pre-prod, retry)", err),
    }

    let control = cycle
        .config_tool
        .ensure(&cycle.config_inputs("uat"), &sink_token());
    match &control {
        Ok(ensured) => {
            cycle.record_ensured("step 9a ensure (uat control)", ensured);
            cycle.observe(
                "FINDING: Doppler refused an environment slug with an underscore (`pre_prod`) \
                 while accepting the plain control `uat` in the same project, so naming::v1's \
                 snake join for a multi-word environment needs a naming::v2 row"
                    .to_string(),
            );
        }
        Err(err) => {
            cycle.record_error("step 9a (uat control)", err);
            cycle.observe(
                "inconclusive: Doppler refused `pre_prod` and the plain `uat` control alike, \
                 so the refusal is not a character class; the environment slug question stays \
                 open"
                    .to_string(),
            );
        }
    }
}

/// Step 9b: the two read-only observations the plan's item 4 asks for
/// that a `GET` answers — the `404` error-body shape and the status a
/// duplicate `POST /v3/projects` answers — plus the environment slugs
/// this project ended up with.
///
/// The duplicate `POST` is the one observation here that could create
/// something. Doppler documents `project` as a unique identifier, so a
/// second create under a name that exists should be refused — but "should
/// be" is the kind of assumption this whole test exists to stop trusting.
/// If it answers `2xx`, whatever name came back is registered with the
/// guard before anything else happens, so the guard's contract ("every
/// project this test creates") survives being wrong about Doppler.
fn step_9b_observations(cycle: &mut Cycle, guard: &mut ProjectGuard) {
    let missing = cycle
        .raw
        .get::<Json>("/v3/projects/project?project=willikins-probe-does-not-exist");
    match missing {
        Err(err) if err.status == Some(404) => {
            let shape = if err.message.starts_with("provider says: ") {
                "a `messages` array was present and the shared client labelled it"
            } else {
                "no recognisable `messages` field was present"
            };
            cycle.observe(format!("a 404 on a missing project: {shape}"));
        }
        Ok(_) => cycle.observe("a project that cannot exist answered 200".to_string()),
        Err(err) => cycle.observe(format!(
            "a missing project answered an unexpected status {:?}",
            err.status
        )),
    }

    let duplicate = cycle.raw.post::<Json>(
        "/v3/projects",
        &serde_json::json!({
            "name": cycle.project.to_string(),
            "description": MANAGED_DESCRIPTION,
        }),
    );
    match duplicate {
        Ok(body) => {
            let created = body
                .pointer("/project/name")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_string();
            match DopplerProject::parse(&created) {
                Ok(project) if project != cycle.project => {
                    guard.register(&project);
                    cycle.observe(format!(
                        "POST /v3/projects with an existing name answered 2xx and created a \
                         SECOND project named `{project}`; it is registered with the guard"
                    ));
                }
                Ok(_) => cycle.observe(
                    "POST /v3/projects with an existing name answered 2xx naming the project \
                     that already exists (Doppler did not refuse it, and created nothing new)"
                        .to_string(),
                ),
                Err(_) => cycle.observe(format!(
                    "POST /v3/projects with an existing name answered 2xx naming \
                     `{created}`, which is not a project slug this test can delete; if that \
                     is a real project it must be deleted BY HAND"
                )),
            }
        }
        Err(err) => cycle.observe(format!(
            "POST /v3/projects with an existing name answered status {:?}; the shared client \
             {} a provider message",
            err.status,
            if err.message.starts_with("provider says: ") {
                "found"
            } else {
                "found no"
            }
        )),
    }

    let environments = cycle.get(
        "step 9b",
        &format!("/v3/environments?project={}", cycle.project),
    );
    record_raw("environments_list", &environments);
    let ids = environment_ids(&environments);
    cycle.observe(format!("environment slugs Doppler ended up with: {ids:?}"));

    let configs = cycle.get("step 9b", &format!("/v3/configs?project={}", cycle.project));
    record_raw("configs_list", &configs);
}

/// Step 9c: the milestone 3 note — whether `POST /v3/configs` expects the
/// caller to prefix a branch config's name with `<environment>_` or
/// applies the prefix itself. Two raw `POST`s under `dev`, one with a
/// bare name and one already prefixed, read back by the name Doppler
/// reports. Observation only; no tool in this crate creates a branch
/// config, and none is added.
fn step_9c_branch_config_prefix(cycle: &mut Cycle) {
    for requested in ["probe", "dev_probe"] {
        let created = cycle.raw.post::<Json>(
            "/v3/configs",
            &serde_json::json!({
                "project": cycle.project.to_string(),
                "environment": "dev",
                "name": requested,
            }),
        );
        match created {
            Ok(body) => {
                record_raw(&format!("branch_config_post_{requested}"), &body);
                let stored = body
                    .pointer("/config/name")
                    .and_then(Json::as_str)
                    .unwrap_or("<no name in the response>")
                    .to_string();
                let root = body.pointer("/config/root").and_then(Json::as_bool);
                cycle.observe(format!(
                    "POST /v3/configs name `{requested}` under environment `dev` was stored as \
                     `{stored}` (root: {root:?})"
                ));
            }
            Err(err) => cycle.observe(format!(
                "POST /v3/configs name `{requested}` under `dev` answered status {:?}",
                err.status
            )),
        }
    }
}

/// Step 10: delete both projects explicitly, confirm neither can be read
/// back, and disarm the guard.
///
/// "Cannot be read back" rather than "is `404`". A `GET` issued
/// immediately after the `DELETE` answers **`400`**, not `404`; a `GET`
/// of the same name a minute later answers `404`, which is what
/// `the_cycles_projects_are_gone` sees. Both were observed live on
/// 2026-09-14, and Doppler documents no non-2xx response for any
/// endpoint at all, so neither status is written down anywhere. What
/// this test can insist on without guessing is the part that matters:
/// the project is not readable, and a `2xx` here would mean the delete
/// did not happen. The status actually seen is recorded as an
/// observation rather than asserted.
fn step_10_delete(cycle: &mut Cycle, guard: &mut ProjectGuard) {
    let mut statuses = Vec::new();
    for project in [cycle.project.clone(), cycle.foreign.clone()] {
        ProjectGuard::delete(&cycle.raw, &project).unwrap_or_else(|err| {
            panic!(
                "step 10: DELETE of `{project}` failed (status {:?}); a service account needs \
                 project-admin rights in the workplace to delete a project",
                err.status
            )
        });
        match cycle.raw.get::<Json>(&project_path(&project)) {
            Err(err) => statuses.push(err.status),
            Ok(_) => panic!("step 10: `{project}` still exists after the DELETE"),
        }
    }
    guard.disarm();
    cycle.observe(format!(
        "a GET of a project immediately after its own DELETE answered {statuses:?}"
    ));
    cycle.say("step 10 (both DELETEs succeed and neither project can be read back): pass");
}

/// The final redaction sweep. Neither minted token's bytes, nor anything
/// credential-shaped, may appear in a string this cycle produced or in a
/// fixture it recorded.
fn sweep_for_secrets(cycle: &Cycle) {
    let mut haystacks: Vec<(String, String)> = cycle
        .sweep
        .iter()
        .enumerate()
        .map(|(index, text)| (format!("produced string {index}"), text.clone()))
        .collect();
    // Every file under `live/`, not only the ones with an authored
    // counterpart: the listings and the branch-config probes are written
    // by `record_raw` and are just as much this cycle's output.
    let entries = std::fs::read_dir(live_dir()).expect("the live fixture directory exists");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        haystacks.push((format!("the recorded file `{}`", path.display()), text));
    }

    assert_eq!(
        cycle.minted.len(),
        2,
        "the sweep needs both minted tokens' bytes to be meaningful"
    );
    for (where_, text) in &haystacks {
        for (index, token) in cycle.minted.iter().enumerate() {
            assert!(
                !text.contains(token),
                "minted token {index}'s bytes reached {where_}"
            );
        }
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "something credential-shaped (`{prefix}`) reached {where_}"
            );
        }
    }
    println!("redaction sweep over {} strings: pass", haystacks.len());
    println!("recorded fixtures: {:?}", cycle.recorded);
    for observation in &cycle.observations {
        println!("recorded observation: {observation}");
    }
}

/// The cycle's two fixed project identities.
fn fixed_projects() -> (DopplerProject, DopplerProject) {
    (
        DopplerProject::parse(PROJECT_NAME).expect("the cycle's fixed name is a valid project"),
        DopplerProject::parse(FOREIGN_PROJECT_NAME)
            .expect("the cycle's foreign name is a valid project"),
    )
}

#[test]
#[ignore = "opt-in live write cycle against a real Doppler workplace; creates and deletes two \
            projects. Run with WILLIKINS_LIVE_TESTS=1, --features live-tests, and sandbox \
            credentials sourced in the same command"]
fn doppler_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox Doppler token");
    let (project, foreign) = fixed_projects();
    // Two clients: one the tools own (through `DopplerClient`, which
    // consumes it), one this test keeps for the raw calls no tool in this
    // crate performs.
    let raw = Arc::new(http_client(credential.clone()));
    let client = Arc::new(DopplerClient::new(http_client(credential)));

    step_1_absent(&raw, &[&project, &foreign]);

    // Declared before the cycle so it drops after it, and armed by
    // registering each project before the request that may create it.
    let mut guard = ProjectGuard::new(Arc::clone(&raw));
    guard.register(&project);
    guard.register(&foreign);
    println!("step 2 (the delete guard is armed for both projects): pass");

    let mut cycle = Cycle {
        raw,
        project_tool: DopplerProjectEnsure::new(Arc::clone(&client)),
        config_tool: DopplerConfigEnsure::new(Arc::clone(&client)),
        token_tool: DopplerServiceTokenEnsure::new(Arc::clone(&client)),
        rotate_tool: DopplerServiceTokenRotate::new(Arc::clone(&client)),
        secret_tool: DopplerSecretGet::new(client),
        project: project.clone(),
        foreign,
        token_name: DopplerTokenName::parse(TOKEN_NAME).expect("the cycle's token name is valid"),
        sweep: Vec::new(),
        minted: Vec::new(),
        recorded: Vec::new(),
        fixture_failures: Vec::new(),
        observations: Vec::new(),
    };

    step_2b_token_read_before_the_project_exists(&mut cycle);
    step_3_project(&mut cycle);
    step_4_foreign(&mut cycle);
    step_5a_default_configs(&mut cycle);
    step_5b_extra_config(&mut cycle);

    let dev = derived_config(&project, "dev");
    let slug = step_6_token(&mut cycle, &dev);
    step_7_rotate(&mut cycle, &dev, &slug);
    step_8_secret(&mut cycle, &dev);

    step_9a_underscore_environment(&mut cycle);
    step_9b_observations(&mut cycle, &mut guard);
    step_9c_branch_config_prefix(&mut cycle);
    assert!(
        cycle.fixture_failures.is_empty(),
        "step 9: recorded-fixture drift: {:?}",
        cycle.fixture_failures
    );

    step_10_delete(&mut cycle, &mut guard);
    sweep_for_secrets(&cycle);
}

/// The after-the-run confirmation, read-only: both of the cycle's
/// projects are gone. Gated behind a second variable so the cycle's own
/// command never runs the two concurrently.
#[test]
#[ignore = "opt-in read-only check that the write cycle left nothing behind; run with \
            WILLIKINS_LIVE_TESTS=1 and WILLIKINS_LIVE_LEFTOVER_CHECK=1"]
fn the_cycles_projects_are_gone() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1")
        || std::env::var("WILLIKINS_LIVE_LEFTOVER_CHECK").as_deref() != Ok("1")
    {
        println!("skip: WILLIKINS_LIVE_TESTS and WILLIKINS_LIVE_LEFTOVER_CHECK are not both 1");
        return;
    }
    let credential = credential_from_env().expect("a valid sandbox Doppler token");
    let raw = http_client(credential);
    let (project, foreign) = fixed_projects();
    let mut leftovers = Vec::new();
    for candidate in [project, foreign] {
        match raw.get::<Json>(&project_path(&candidate)) {
            Err(err) if err.status == Some(404) => {
                println!("leftover check (`{candidate}` is gone): pass");
            }
            Ok(_) => {
                println!("leftover check (`{candidate}` still exists): fail");
                leftovers.push(candidate.to_string());
            }
            Err(err) => {
                println!("leftover check (`{candidate}`): fail");
                leftovers.push(format!("{candidate} answered status {:?}", err.status));
            }
        }
    }
    assert!(
        leftovers.is_empty(),
        "leftover check: these must be deleted by hand: {leftovers:?}"
    );
}

/// `top_level_keys` is the comparison the recorded fixtures rest on, and
/// every other item in `common` is reached only from an opt-in live test.
/// This one is not ignored: it pins the rule that an authored fixture's
/// keys must be a subset of a live response's, in both directions.
#[test]
fn top_level_keys_names_an_objects_own_keys_only() {
    let body = serde_json::json!({"project": {"id": "x", "name": "y"}, "page": 1});
    let keys: Vec<String> = top_level_keys(&body).into_iter().collect();
    assert_eq!(keys, vec!["page".to_string(), "project".to_string()]);
    assert!(top_level_keys(&serde_json::json!([1, 2])).is_empty());
}
