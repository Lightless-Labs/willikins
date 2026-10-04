//! The live Doppler **secret-name gate** cycle: milestone 3j task B5,
//! `doppler.secret_name.gate` driven through its `Tool` interface (never
//! a mock) against a throwaway base/child project pair in the operator's
//! sandbox Doppler workplace.
//!
//! Compiled only with this crate's `live-tests` feature (its own
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. The cycle itself is
//! `#[ignore]`d on top of that, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` — the sandbox credential is read (through
//! [`willikins_providers_doppler::credential_from_env`]) only past that
//! gate, exactly as `tests/live_write_cycle.rs` does.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
//!   cargo test -p willikins-providers-doppler --features live-tests --test live_secret_name_gate_cycle \
//!   -j 2 -- --ignored --nocapture
//! ```
//!
//! # The thirteen steps (plan: "The live names cycle")
//!
//! One `<unix-seconds>` names both throwaway projects: a **base** with an
//! underscore (`willikins_names_probe_<unix>`, (a1) live) and a **child**
//! with hyphens (`willikins-names-probe-<unix>`), so the cycle proves the
//! underscore grammar on the project a document would actually name
//! `shared_keys`-style, while the child matches this crate's other live
//! cycles' own naming.
//!
//! 1. Both project names read absent through a raw `GET`
//!    ([`willikins_providers_doppler::looks_like_a_missing_project`], not
//!    a literal `404` — the identical absence answers `400` "does not
//!    have access" once this token can already see any project in the
//!    workplace). [`ProjectGuard`] is armed and **both** projects are
//!    registered with it *before* this step's own assertions run, so a
//!    failure here still cleans up anything already created by an
//!    aborted earlier run. The guard deletes the **child** before the
//!    **base** on every exit path, because Doppler may refuse to delete
//!    a project whose config another config inherits.
//! 2. `doppler.project.ensure` creates both. A test-local raw `GET` on
//!    the base reads its slug equal to the underscored name.
//! 3. `doppler.config.inheritable.ensure` on `<base>/prd`;
//!    `doppler.config.inheritable.gate` on it reads `Present`.
//! 4. `doppler.secret_name.gate` on `<child>/prd` for the probe secret
//!    reads `Absent` — nothing is set or inherited yet.
//! 5. A raw `POST /v3/configs/config/secrets` sets the probe secret in
//!    `<base>/prd`, with the plain-text marker value. The gate on
//!    `<base>/prd` reads `Present` (direct listing).
//! 6. The gate on `<child>/prd` still reads `Absent` — nothing inherited
//!    yet.
//! 7. `doppler.config.inherits.ensure` makes `<child>/prd` inherit
//!    `[<base>/prd]`: `changed: true`, then `read` is `Present`. This
//!    proves a config body with an underscored `inherits` entry parses
//!    live.
//! 8. **Verify item 1.** A test-local `GET
//!    /v3/configs/config/secrets/names` on `<child>/prd` (the same query
//!    the client sends), deserialized into [`NamesProbe`] (no `Debug`),
//!    prints exactly one line: whether the probe secret is listed.
//!    Nothing else from the body is printed, and nothing is asserted
//!    about the answer.
//! 9. The gate on `<child>/prd` reads `Present`, which must hold under
//!    either answer to step 8 (decision (b4)'s walk).
//! 10. The gate on `<child>/prd` for a never-set name reads `Absent`.
//! 11. The gate on a project name this token has never seen reads
//!     `Absent`, not `Err` (decision (b3)).
//! 12. The child is deleted, then the base; both re-read absent
//!     ([`looks_like_a_missing_project`] again, not a literal `404`);
//!     the guard is disarmed.
//!
//! A final redaction sweep is unnecessary here, unlike
//! `tests/live_write_cycle.rs`: this cycle mints no credential and reads
//! no secret *value* — only names, and only the one this process itself
//! set, which is plain text by design (SHARED VALUES).
//!
//! # Step 13: the after-the-run leftover check
//!
//! A second `#[ignore]` test, [`the_names_probe_projects_are_gone`], gated
//! on `WILLIKINS_LIVE_LEFTOVER_CHECK=1` on top of `WILLIKINS_LIVE_TESTS=1`,
//! lists the workplace's project names (a test-local struct holding only
//! `name`) and asserts none starts with either prefix this cycle uses.
//! Naming the test when running the leftover check keeps it from racing
//! the cycle itself, which the cycle's own command above never does
//! anyway (it does not set the second variable):
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   WILLIKINS_LIVE_LEFTOVER_CHECK=1 RUST_TEST_THREADS=2 cargo test \
//!   -p willikins-providers-doppler --features live-tests --test live_secret_name_gate_cycle \
//!   -j 2 -- --ignored --nocapture the_names_probe_projects_are_gone
//! ```
//!
//! Nothing this file prints is a token, a secret value, or the name of a
//! project other than this cycle's own throwaway pair.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value as Json;
use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigInheritableEnsure, DopplerConfigInheritableGate,
    DopplerConfigInheritsEnsure, DopplerProjectEnsure, DopplerSecretNameGate, credential_from_env,
    http_client, looks_like_a_missing_project,
};
use willikins_providers_http::{Http, ProviderError};
use willikins_types::{DomainType, DopplerConfig, DopplerProject, SecretName};

/// The probe secret (SHARED VALUES). Visible only through the gate's one
/// `bool`; this file never reads its value back.
const PROBE_SECRET_NAME: &str = "WILLIKINS_NAMES_PROBE";

/// A name this cycle never sets, for step 10's `Absent`.
const ABSENT_SECRET_NAME: &str = "WILLIKINS_NAMES_PROBE_ABSENT";

/// The probe secret's plain-text value (SHARED VALUES) — not a provider
/// token of any kind, and never read back by this file, only set.
const PROBE_SECRET_VALUE: &str = "willikins names probe, not a secret";

/// The base project's prefix (underscore-joined, (a1) live).
const BASE_PREFIX: &str = "willikins_names_probe_";

/// The child project's, and the never-created missing project's, prefix
/// (hyphen-joined).
const CHILD_PREFIX: &str = "willikins-names-probe-";

/// `SinkToken::new` is disallowed outside the apply executor; a test
/// opts in narrowly, the convention every such test in this workspace
/// uses.
#[allow(clippy::disallowed_methods)]
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// A port name, parsed. Every name this file passes is a literal.
fn port(name: &'static str) -> PortName {
    PortName::parse(name).expect("the cycle's port names are valid")
}

/// The unix-second suffix this run's two throwaway project names share.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the unix epoch")
        .as_secs()
}

/// `GET /v3/projects/project?project=<name>`.
fn project_path(project: &DopplerProject) -> String {
    format!("/v3/projects/project?project={project}")
}

/// The `prd` config `naming::v1` would derive for `project` — built
/// directly, since every config this cycle touches is the root `prd`
/// config Doppler auto-creates with the project.
fn config_for(project: &DopplerProject) -> DopplerConfig {
    DopplerConfig::parse(&format!("{project}/prd")).expect("the cycle's config name parses")
}

/// `doppler.project.ensure`'s one input.
fn project_inputs(project: &DopplerProject) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project.clone()));
    inputs
}

/// The one input shared by `doppler.config.inheritable.ensure` and
/// `doppler.config.inheritable.gate`.
fn config_inputs(config: &DopplerConfig) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config.clone()));
    inputs
}

/// `doppler.secret_name.gate`'s two inputs.
fn gate_inputs(config: &DopplerConfig, name: &SecretName) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config.clone()));
    inputs.insert(port("name"), Value::known(name.clone()));
    inputs
}

/// `doppler.config.inherits.ensure`'s two inputs.
fn inherits_inputs(config: &DopplerConfig, bases: Vec<DopplerConfig>) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config.clone()));
    inputs.insert(port("inherits"), Value::known_list(bases));
    inputs
}

/// A parsed [`SecretName`] literal.
fn secret_name(name: &str) -> SecretName {
    SecretName::parse(name).expect("the cycle's secret names are valid")
}

/// Deletes the **child** before the **base** on every exit path — a
/// panic, a failed assertion, or an early return — unless the test body
/// already deleted both and disarmed the guard. Doppler may refuse to
/// delete a project whose config another config inherits, which is
/// exactly the relationship step 7 creates between these two.
///
/// A `Drop` implementation must not panic while the thread is already
/// panicking (that aborts the process and hides the original failure),
/// so a failed delete during a panic prints the project's full name
/// loudly and returns; it panics only when it is itself the first
/// failure. "Already gone" is read through
/// [`looks_like_a_missing_project`], never a literal `404`, for the same
/// reason `tests/live_write_cycle.rs`'s own `ProjectGuard` tolerates
/// `400` here: once this token can see any project, a project that is
/// genuinely gone answers either shape.
struct ProjectGuard {
    http: Arc<Http>,
    child: Option<DopplerProject>,
    base: Option<DopplerProject>,
}

impl ProjectGuard {
    fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            child: None,
            base: None,
        }
    }

    /// Take responsibility for both projects at once. Called *before*
    /// any assertion this cycle makes, so an aborted run's cleanup never
    /// depends on how far step 1 got.
    fn register(&mut self, child: &DopplerProject, base: &DopplerProject) {
        self.child = Some(child.clone());
        self.base = Some(base.clone());
    }

    fn disarm(&mut self) {
        self.child = None;
        self.base = None;
    }

    /// `DELETE /v3/projects/project` with `{"project": "<name>"}`.
    fn delete(http: &Http, project: &DopplerProject) -> Result<(), ProviderError> {
        let body = serde_json::json!({ "project": project.to_string() });
        http.delete_with_body("/v3/projects/project", &body)
    }
}

impl Drop for ProjectGuard {
    fn drop(&mut self) {
        let already_panicking = std::thread::panicking();
        for project in [self.child.take(), self.base.take()].into_iter().flatten() {
            println!("guard: deleting `{project}`");
            match Self::delete(&self.http, &project) {
                Ok(()) => println!("guard: deleted `{project}`"),
                Err(err) if looks_like_a_missing_project(&err) => {
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

/// Step 1: both fixed project names must be absent — read through
/// [`looks_like_a_missing_project`], never a literal `404` (its own doc:
/// once this token can see any project in the workplace, a missing one
/// answers `400` "does not have access" instead). A leftover from an
/// aborted run is the operator's to remove by hand, so this test refuses
/// to proceed and names it rather than deleting something it did not
/// create here.
fn step_1_absent(raw: &Http, base: &DopplerProject, child: &DopplerProject) {
    for project in [base, child] {
        match raw.get::<Json>(&project_path(project)) {
            Err(err) if looks_like_a_missing_project(&err) => {}
            Ok(_) => panic!(
                "step 1: `{project}` already exists, left behind by an aborted run; a leftover \
                 is the operator's to delete by hand, so this test refuses to proceed"
            ),
            Err(err) => panic!(
                "step 1: GET of `{project}` answered an unexpected status {:?}",
                err.status
            ),
        }
    }
    println!("step 1 (neither fixed project name is visible to this token): pass");
}

/// Step 2: `doppler.project.ensure` creates both, each reporting
/// `changed: true`; a raw `GET` of the base confirms Doppler stored the
/// underscored name verbatim as its slug — (a1) live, through the real
/// tool.
fn step_2_create_projects(
    client: &Arc<DopplerClient>,
    raw: &Http,
    base: &DopplerProject,
    child: &DopplerProject,
) {
    let tool = DopplerProjectEnsure::new(Arc::clone(client));
    for project in [base, child] {
        let inputs = project_inputs(project);
        let ensured = tool
            .ensure(&inputs, &sink_token())
            .unwrap_or_else(|err| panic!("step 2: creating `{project}` failed: {err:?}"));
        assert!(
            ensured.changed,
            "step 2: creating `{project}` must report changed: true"
        );
    }

    let body = raw
        .get::<Json>(&project_path(base))
        .unwrap_or_else(|err| panic!("step 2: reading the base project back failed: {err:?}"));
    assert_eq!(
        body.pointer("/project/slug").and_then(Json::as_str),
        Some(base.to_string().as_str()),
        "step 2: the base project's slug must equal the underscored name verbatim"
    );
    println!("step 2 (both projects created, the base project's slug keeps its underscores): pass");
}

/// Step 3: `doppler.config.inheritable.ensure` on `<base>/prd`, then
/// `doppler.config.inheritable.gate` on it reads `Present`.
fn step_3_base_inheritable(client: &Arc<DopplerClient>, base_config: &DopplerConfig) {
    let ensure_tool = DopplerConfigInheritableEnsure::new(Arc::clone(client));
    let inputs = config_inputs(base_config);
    let ensured = ensure_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!("step 3: marking `{base_config}` inheritable failed: {err:?}")
        });
    assert!(
        ensured.changed,
        "step 3: marking a fresh config inheritable must report changed: true"
    );

    let gate_tool = DopplerConfigInheritableGate::new(Arc::clone(client));
    let observation = gate_tool
        .read(&inputs)
        .unwrap_or_else(|err| panic!("step 3: reading the inheritable gate failed: {err:?}"));
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 3: `{base_config}` must read Present on the inheritable gate, observed \
         {observation:?}"
    );
    println!("step 3 (`{base_config}` marked inheritable, inheritable gate Present): pass");
}

/// Shared by steps 4, 6, 9, 10 and 11: `doppler.secret_name.gate`'s
/// `read`, built fresh each call so no step depends on another's tool
/// instance.
fn gate_read(
    client: &Arc<DopplerClient>,
    config: &DopplerConfig,
    name: &SecretName,
) -> Observation {
    let tool = DopplerSecretNameGate::new(Arc::clone(client));
    let inputs = gate_inputs(config, name);
    tool.read(&inputs).unwrap_or_else(|err| {
        panic!("the gate must read, not fail, on `{config}`/`{name}`: {err:?}")
    })
}

/// Step 4: the gate on `<child>/prd` for the probe secret reads `Absent`
/// — nothing is set or inherited yet.
fn step_4_gate_absent_before_anything(client: &Arc<DopplerClient>, child_config: &DopplerConfig) {
    let name = secret_name(PROBE_SECRET_NAME);
    let observation = gate_read(client, child_config, &name);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 4: `{child_config}` must read Absent before the secret exists anywhere, observed \
         {observation:?}"
    );
    println!("step 4 (gate on `{child_config}` is Absent before anything is set): pass");
}

/// Step 5: a raw `POST /v3/configs/config/secrets` sets the probe secret
/// in `<base>/prd` with the plain-text marker value; the gate on
/// `<base>/prd` then reads `Present` (direct listing).
fn step_5_set_secret_in_base(
    raw: &Http,
    client: &Arc<DopplerClient>,
    base: &DopplerProject,
    base_config: &DopplerConfig,
) {
    let body = serde_json::json!({
        "project": base.to_string(),
        "config": base_config.name().to_string(),
        "secrets": { PROBE_SECRET_NAME: PROBE_SECRET_VALUE },
    });
    raw.post::<Json>("/v3/configs/config/secrets", &body)
        .unwrap_or_else(|err| {
            panic!("step 5: setting the probe secret on `{base_config}` failed: {err:?}")
        });

    let name = secret_name(PROBE_SECRET_NAME);
    let observation = gate_read(client, base_config, &name);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 5: `{base_config}` must read Present right after the secret is set directly, \
         observed {observation:?}"
    );
    println!("step 5 (probe secret set in `{base_config}`, gate on it is Present): pass");
}

/// Step 6: the gate on `<child>/prd` still reads `Absent` — nothing has
/// been inherited yet.
fn step_6_child_still_absent(client: &Arc<DopplerClient>, child_config: &DopplerConfig) {
    let name = secret_name(PROBE_SECRET_NAME);
    let observation = gate_read(client, child_config, &name);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 6: `{child_config}` must still read Absent before inheritance is set, observed \
         {observation:?}"
    );
    println!("step 6 (gate on `{child_config}` is still Absent before inheritance): pass");
}

/// Step 7: `doppler.config.inherits.ensure` makes `<child>/prd` inherit
/// `[<base>/prd]` (`changed: true`), then `read` is `Present`. Proves a
/// config body with an underscored `inherits` entry parses live.
fn step_7_set_inherits(
    client: &Arc<DopplerClient>,
    child_config: &DopplerConfig,
    base_config: &DopplerConfig,
) {
    let tool = DopplerConfigInheritsEnsure::new(Arc::clone(client));
    let inputs = inherits_inputs(child_config, vec![base_config.clone()]);
    let ensured = tool.ensure(&inputs, &sink_token()).unwrap_or_else(|err| {
        panic!("step 7: making `{child_config}` inherit `{base_config}` failed: {err:?}")
    });
    assert!(
        ensured.changed,
        "step 7: setting a fresh inherits list must report changed: true"
    );

    let observation = tool.read(&inputs).unwrap_or_else(|err| {
        panic!("step 7: re-reading `{child_config}`'s inherits failed: {err:?}")
    });
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 7: `{child_config}` must read Present once it inherits `{base_config}`, observed \
         {observation:?}"
    );
    println!(
        "step 7 (`{child_config}` now inherits `{base_config}`, underscored entry parses): pass"
    );
}

/// Doppler's secret-names-list response, read by this test alone and
/// deliberately carrying no `Debug`: every name in it is a name the
/// operator chose for *some* secret in the config, and this file's job
/// is to ask one question about one of them, never to print the rest
/// (milestone 3j trust boundary 2).
#[derive(Deserialize)]
struct NamesProbe {
    names: Vec<String>,
}

/// Step 8 (verify item 1): a test-local `GET
/// /v3/configs/config/secrets/names` on `<child>/prd`, with the same
/// query the client itself sends, deserialized into [`NamesProbe`]. Prints
/// exactly one line — whether the probe secret is listed — and asserts
/// nothing about the answer: the gate (step 9) is correct either way.
fn step_8_inherited_names_probe(raw: &Http, child_config: &DopplerConfig) {
    let path = format!(
        "/v3/configs/config/secrets/names?project={}&config={}&include_dynamic_secrets=false&include_managed_secrets=false",
        child_config.project(),
        child_config.name()
    );
    let body: NamesProbe = raw
        .get(&path)
        .unwrap_or_else(|err| panic!("step 8: listing names on `{child_config}` failed: {err:?}"));
    let lists_inherited = body.names.iter().any(|listed| listed == PROBE_SECRET_NAME);
    println!("names endpoint lists inherited names: {lists_inherited}");
}

/// Step 9: the gate on `<child>/prd` reads `Present`, which must hold
/// under either answer step 8 just observed (decision (b4)'s walk).
fn step_9_gate_present_either_way(client: &Arc<DopplerClient>, child_config: &DopplerConfig) {
    let name = secret_name(PROBE_SECRET_NAME);
    let observation = gate_read(client, child_config, &name);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 9: `{child_config}` must read Present once it inherits the base, observed \
         {observation:?}"
    );
    println!("step 9 (gate on `{child_config}` is Present through inheritance): pass");
}

/// Step 10: the gate on `<child>/prd` for a never-set name reads
/// `Absent`.
fn step_10_absent_name(client: &Arc<DopplerClient>, child_config: &DopplerConfig) {
    let name = secret_name(ABSENT_SECRET_NAME);
    let observation = gate_read(client, child_config, &name);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 10: a name never set anywhere must read Absent, observed {observation:?}"
    );
    println!("step 10 (gate on `{child_config}` for an unset name is Absent): pass");
}

/// Step 11: the gate on a project this token has never seen reads
/// `Absent`, not `Err` (decision (b3)) — the ambiguous pair this gate
/// deliberately cannot, and does not try to, tell apart from a project
/// outside its grant.
fn step_11_gate_on_missing_project(client: &Arc<DopplerClient>, missing: &DopplerProject) {
    let config = config_for(missing);
    let name = secret_name(PROBE_SECRET_NAME);
    let observation = gate_read(client, &config, &name);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 11: a config whose project does not exist must read Absent, not Err, observed \
         {observation:?}"
    );
    println!("step 11 (gate on a never-created project `{missing}` is Absent, not Err): pass");
}

/// Step 12: delete the child, then the base; both re-read absent
/// ([`looks_like_a_missing_project`], never a literal `404`); disarm the
/// guard.
fn step_12_delete(
    guard: &mut ProjectGuard,
    raw: &Http,
    base: &DopplerProject,
    child: &DopplerProject,
) {
    ProjectGuard::delete(raw, child)
        .unwrap_or_else(|err| panic!("step 12: deleting `{child}` failed: {err:?}"));
    ProjectGuard::delete(raw, base)
        .unwrap_or_else(|err| panic!("step 12: deleting `{base}` failed: {err:?}"));

    for project in [child, base] {
        match raw.get::<Json>(&project_path(project)) {
            Err(err) if looks_like_a_missing_project(&err) => {}
            Ok(_) => panic!("step 12: `{project}` still exists after its own DELETE"),
            Err(err) => panic!(
                "step 12: `{project}` answered an unexpected status after DELETE: {:?}",
                err.status
            ),
        }
    }
    guard.disarm();
    println!("step 12 (child then base deleted, both unreadable, guard disarmed): pass");
}

#[test]
#[ignore = "opt-in live names cycle against a real Doppler sandbox workplace; creates and \
            deletes a throwaway project pair. Run with WILLIKINS_LIVE_TESTS=1, \
            --features live-tests, and sandbox credentials sourced in the same command"]
fn doppler_live_secret_name_gate_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox Doppler token");
    let unix = unix_now();
    let base = DopplerProject::parse(&format!("{BASE_PREFIX}{unix}"))
        .expect("the cycle's base project name parses");
    let child = DopplerProject::parse(&format!("{CHILD_PREFIX}{unix}"))
        .expect("the cycle's child project name parses");
    let missing = DopplerProject::parse(&format!("{CHILD_PREFIX}missing-{unix}"))
        .expect("the cycle's missing project name parses");

    let raw = Arc::new(http_client(credential.clone()));
    let client = Arc::new(DopplerClient::new(http_client(credential)));

    // Both projects are registered with the guard before any assertion
    // this cycle makes, including step 1's own absence checks, so a
    // failure anywhere still cleans up correctly.
    let mut guard = ProjectGuard::new(Arc::clone(&raw));
    guard.register(&child, &base);
    println!("guard armed for `{child}` (deleted first) and `{base}`");

    step_1_absent(&raw, &base, &child);
    step_2_create_projects(&client, &raw, &base, &child);

    let base_config = config_for(&base);
    let child_config = config_for(&child);

    step_3_base_inheritable(&client, &base_config);
    step_4_gate_absent_before_anything(&client, &child_config);
    step_5_set_secret_in_base(&raw, &client, &base, &base_config);
    step_6_child_still_absent(&client, &child_config);
    step_7_set_inherits(&client, &child_config, &base_config);
    step_8_inherited_names_probe(&raw, &child_config);
    step_9_gate_present_either_way(&client, &child_config);
    step_10_absent_name(&client, &child_config);
    step_11_gate_on_missing_project(&client, &missing);
    step_12_delete(&mut guard, &raw, &base, &child);
}

/// Doppler's project-list response, read by this test alone and limited
/// to the one field it needs: a project's name, never anything else a
/// fuller listing would carry.
#[derive(Deserialize)]
struct ProjectListProbe {
    projects: Vec<ProjectNameOnly>,
}

#[derive(Deserialize)]
struct ProjectNameOnly {
    name: String,
}

/// Step 13: the after-the-run confirmation. Lists the workplace's
/// project names and asserts none starts with either prefix this cycle
/// uses. Gated behind a second variable so the cycle's own command above
/// never runs the two concurrently; naming this test when running the
/// leftover check keeps it from racing a fresh cycle.
///
/// Only a *matched* leftover name is ever printed — this cycle's own
/// throwaway prefix, never the full listing, which may hold projects
/// that are not this test's to name.
#[test]
#[ignore = "opt-in read-only check that the names cycle left nothing behind; run with \
            WILLIKINS_LIVE_TESTS=1 and WILLIKINS_LIVE_LEFTOVER_CHECK=1"]
fn the_names_probe_projects_are_gone() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1")
        || std::env::var("WILLIKINS_LIVE_LEFTOVER_CHECK").as_deref() != Ok("1")
    {
        println!("skip: WILLIKINS_LIVE_TESTS and WILLIKINS_LIVE_LEFTOVER_CHECK are not both 1");
        return;
    }
    let credential = credential_from_env().expect("a valid sandbox Doppler token");
    let raw = http_client(credential);
    let body: ProjectListProbe = raw
        .get("/v3/projects?per_page=100")
        .expect("listing projects failed");
    let leftovers: Vec<String> = body
        .projects
        .into_iter()
        .map(|entry| entry.name)
        .filter(|name| name.starts_with(BASE_PREFIX) || name.starts_with(CHILD_PREFIX))
        .collect();
    assert!(
        leftovers.is_empty(),
        "leftover check: these must be deleted by hand: {leftovers:?}"
    );
    println!("leftover check (no `{BASE_PREFIX}` or `{CHILD_PREFIX}` name remains): pass");
}
