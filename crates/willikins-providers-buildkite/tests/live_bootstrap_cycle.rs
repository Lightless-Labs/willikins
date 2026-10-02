//! The live Buildkite **bootstrap** cycle (milestone 3i task A7, plan
//! `docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md`'s
//! "The live bootstrap cycle" and trust boundary 5): the one test in
//! this crate that writes a real pipeline's stored `configuration`
//! through `buildkite.pipeline.bootstrap.ensure`, then removes the
//! throwaway pipeline that held it.
//!
//! Compiled only with the crate's `live-tests` feature (this file's
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. `#[ignore]` on top of
//! that, and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`
//! -- the credential is read only past that gate, exactly as
//! `tests/live_write_cycle.rs` and `tests/live_probe.rs` do.
//!
//! **Trust boundary 5: writes only in the sandbox organisation**
//! (`WILLIKINS_SANDBOX_BUILDKITE_ORG`), only on the throwaway pipeline
//! `willikins-bootstrap-<unix-seconds>` this run creates and deletes
//! itself. Nothing here ever touches `la-bande-a-bonnot`.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
//!   cargo test -p willikins-providers-buildkite --features live-tests --test live_bootstrap_cycle \
//!   -j 2 -- --ignored --nocapture
//! ```
//!
//! Steps (plan section "The live bootstrap cycle"):
//!
//! 1. count the sandbox organisation's pipelines by paging
//!    `GET /v2/organizations/{org}/pipelines?page=N&per_page=100` into
//!    `Vec<serde::de::IgnoredAny>` with explicit `page`/`per_page`
//!    parameters, to the first short page -- the `Link` pagination
//!    header is never read, followed, or logged (milestone 3a trust
//!    boundary 7), the same rule
//!    `BuildkiteClient::list_clusters_page` already follows for
//!    clusters. Record `N0`;
//! 2. the generated slug must read `404` before this run starts -- a
//!    leftover from an aborted run is the operator's to remove by hand,
//!    the same rule `tests/live_write_cycle.rs` follows for its own fixed
//!    slug;
//! 3. a [`PipelineGuard`] is armed **before any assertion**, so a panic
//!    or a failed assertion anywhere after this point still deletes the
//!    pipeline this run creates;
//! 4. `buildkite.cluster.get` resolves `Default cluster` to its real id,
//!    then `buildkite.pipeline.ensure` creates the pipeline (the frozen
//!    bootstrap, `managed-by: willikins`); the count is `N0 + 1`;
//! 5. `buildkite.pipeline.bootstrap.ensure`, with the plan's SHARED
//!    VALUES `RepoFile` (`probe/.buildkite/bootstrap.yml`, a `block`
//!    step -- no command exists to run), reads `Absent` with `updates()`
//!    `true`; `buildkite.pipeline.bootstrap.gate` with the same file
//!    also reads `Absent`;
//! 6. `ensure` -> `changed: true`; `read` -> `Present`; a second `ensure`
//!    -> `changed: false`; the gate now reads `Present`. Scheduled and
//!    running build counts are read straight off the pipeline's own JSON
//!    (verify item 3) before and after the write, into a test-local
//!    struct -- never added to [`BuildkiteClient`] itself, since nothing
//!    in this crate's tools needs them;
//! 7. `buildkite.pipeline.ensure`'s own `read` is still `Present`:
//!    `repository`, `cluster_id` and `description` are unchanged by the
//!    `PATCH` (verify item 1);
//! 8. the pipeline is deleted through [`BuildkiteClient::delete_pipeline`]
//!    directly, re-read as `404`, the count is back to `N0`, and the
//!    guard is disarmed.
//!
//! `Foreign` cannot be proved live (making a pipeline foreign needs a
//! `description` `PATCH`, which this crate never sends) -- the mocks in
//! `tests/pipeline_bootstrap_ensure_mock.rs` prove that arm. Nothing here
//! prints a token, the stored configuration read back, or any pipeline
//! other than the one throwaway slug this run owns.
//!
//! A second `#[ignore]` test in this file, `the_bootstrap_cycles_pipeline_is_gone`,
//! is the after-the-run confirmation: it lists the organisation's
//! pipelines and asserts that no `willikins-bootstrap-*` slug remains. It
//! needs `WILLIKINS_LIVE_LEFTOVER_CHECK=1` on top of `WILLIKINS_LIVE_TESTS=1`,
//! so the cycle's own command above never runs it, and it is named so the
//! cycle cannot start creating the very pipeline the check asserts is
//! gone, the same rule `tests/live_write_cycle.rs`'s own leftover test
//! follows:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   WILLIKINS_LIVE_LEFTOVER_CHECK=1 RUST_TEST_THREADS=2 cargo test \
//!   -p willikins-providers-buildkite --features live-tests --test live_bootstrap_cycle \
//!   -j 2 -- --ignored --nocapture the_bootstrap_cycles_pipeline_is_gone
//! ```

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineBootstrapEnsure,
    BuildkitePipelineBootstrapGate, BuildkitePipelineEnsure,
};
use willikins_providers_http::Http;
use willikins_types::{
    BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubOrg, GitHubRepo,
    ProjectSlug, RepoFile, RepoPath,
};

/// The greatest number of pipeline pages [`count_pipelines`] will fetch
/// before giving up -- mirrors
/// [`willikins_providers_buildkite::MAX_CLUSTER_PAGES`]'s own reasoning,
/// sized for this test's own organisation rather than imported from the
/// crate, since no tool in this crate ever lists pipelines.
const MAX_PIPELINE_PAGES: u32 = 20;

/// The generated slug's fixed prefix, shared by the cycle and by
/// [`the_bootstrap_cycles_pipeline_is_gone`]'s own leftover scan.
const SLUG_PREFIX: &str = "willikins-bootstrap-";

/// The throwaway slug this run creates and deletes: `willikins-bootstrap-<unix-seconds>`,
/// distinct on every run so a previous run's own cleanup race can never
/// collide with this one.
fn slug() -> BuildkitePipelineSlug {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock reads after 1970")
        .as_secs();
    BuildkitePipelineSlug::parse(&format!("{SLUG_PREFIX}{seconds}"))
        .expect("a generated slug is always valid")
}

fn org() -> BuildkiteOrg {
    let org_value = std::env::var("WILLIKINS_SANDBOX_BUILDKITE_ORG")
        .expect("WILLIKINS_SANDBOX_BUILDKITE_ORG is set");
    BuildkiteOrg::parse(&org_value).expect("a valid Buildkite organisation slug")
}

/// A repository this pipeline points at. Buildkite stores this as an
/// opaque string on the pipeline object and never validates it against a
/// real GitHub repository at create time, so this need not exist --
/// mirrors `tests/live_write_cycle.rs`'s own `repo`.
fn repo() -> GitHubRepo {
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    GitHubRepo::new(
        GitHubOrg::parse(&org_value).expect("valid GitHub org"),
        ProjectSlug::parse("willikins-bootstrap-cycle").expect("valid slug literal"),
    )
}

/// The plan's SHARED VALUES `RepoFile`: a `.buildkite`-rooted path and a
/// `block` step, since no command exists to run against a throwaway
/// pipeline.
fn bootstrap_file() -> RepoFile {
    RepoFile::new(
        RepoPath::parse("probe/.buildkite/bootstrap.yml").expect("a valid repo path literal"),
        "steps:\n  - block: \"willikins bootstrap probe\"\n",
    )
    .expect("a valid bootstrap RepoFile")
}

/// Scheduled and running build counts, deserialized straight off a
/// pipeline's own response (verify item 3: "a `PATCH` does not itself
/// trigger a build"). Diagnostic only -- printed, never asserted
/// against, since a single live run cannot rule out a build Buildkite
/// schedules for an unrelated reason. Kept test-local, never added to
/// [`BuildkiteClient`]: nothing in this crate's tools needs either
/// field. Its own two-field `Deserialize`, not `serde_json::Value`, so
/// the credential-bearing fields the same response carries
/// (`configuration`, `provider.webhook_url`, trust boundary 4) are never
/// even parsed into a value this test could print or `{:?}`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
struct BuildCounts {
    scheduled_builds_count: Option<i64>,
    running_builds_count: Option<i64>,
}

impl BuildCounts {
    /// `GET /v2/organizations/{org}/pipelines/{slug}` through the raw
    /// [`Http`] client (never through [`BuildkiteClient`], which has no
    /// typed field for either count and must not grow one just for this
    /// test).
    fn read(http: &Http, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Self {
        http.get(&format!("/v2/organizations/{org}/pipelines/{slug}"))
            .expect("the pipeline exists at this point in the cycle")
    }
}

/// A one-field `Deserialize`, not `serde_json::Value`: `the_bootstrap_cycles_pipeline_is_gone`'s
/// leftover scan reads a pipeline list response whose items carry no
/// credential-bearing field (unlike the single-pipeline response
/// [`BuildCounts`] and [`check_absent_before_start`] each avoid parsing
/// in full), but there is still no reason to deserialize more of each
/// item than the one field that scan reads.
#[derive(serde::Deserialize)]
struct PipelineSlugOnly {
    slug: String,
}

/// Pages `GET /v2/organizations/{org}/pipelines` to the first short page,
/// deserializing each page into `Vec<serde::de::IgnoredAny>` -- nothing
/// but the page's length is ever read, and the `Link` header is never
/// inspected (milestone 3a trust boundary 7).
fn count_pipelines(http: &Http, org: &BuildkiteOrg) -> usize {
    let mut total = 0usize;
    for page in 1..=MAX_PIPELINE_PAGES {
        let items: Vec<serde::de::IgnoredAny> = http
            .get(&format!(
                "/v2/organizations/{org}/pipelines?page={page}&per_page=100"
            ))
            .expect("lists the organisation's pipelines");
        let len = items.len();
        total += len;
        if len < 100 {
            return total;
        }
    }
    panic!(
        "more than {MAX_PIPELINE_PAGES} pages of pipelines in this organisation; raise the \
         bound or narrow the organisation this test runs against"
    );
}

/// Deletes [`slug`]'s pipeline on drop, unless [`Self::disarm`] was
/// called first -- so a panic or a failed assertion anywhere in the run
/// still cleans up. Mirrors `tests/live_write_cycle.rs`'s own
/// `PipelineGuard`, but over the generated (not fixed) slug.
struct PipelineGuard {
    client: Arc<BuildkiteClient>,
    org: BuildkiteOrg,
    slug: BuildkitePipelineSlug,
    armed: bool,
}

impl PipelineGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.client.delete_pipeline(&self.org, &self.slug);
        }
    }
}

/// Step 2: the generated slug must be absent before this run starts. A
/// leftover from an aborted run is the operator's to remove by hand --
/// this slug is generated from the current time, so a collision should
/// never happen on its own.
///
/// Reads into `serde::de::IgnoredAny`, not `serde_json::Value`: the
/// "already exists" arm must never format a parsed body, since a real
/// pipeline's response carries `configuration` and
/// `provider.webhook_url` (trust boundary 4) -- only the HTTP status is
/// ever named.
fn check_absent_before_start(raw_http: &Http, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) {
    match raw_http
        .get::<serde::de::IgnoredAny>(&format!("/v2/organizations/{org}/pipelines/{slug}"))
    {
        Err(err) if err.status == Some(404) => {
            println!("step 2: `{org}/{slug}` reads 404 before this run: pass");
        }
        Ok(_) => panic!(
            "`{org}/{slug}` already exists before this run starts; it must be removed by \
             hand before this test can run"
        ),
        Err(err) => panic!(
            "`{org}/{slug}` answered status {:?} before this run starts, expected 404",
            err.status
        ),
    }
}

/// Step 4a: resolve `Default cluster` to its real id.
fn resolve_default_cluster(cluster_tool: &BuildkiteClusterGet, org: &BuildkiteOrg) -> Value {
    let mut cluster_inputs = Inputs::new();
    cluster_inputs.insert(PortName::parse("org").unwrap(), Value::known(org.clone()));
    cluster_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(BuildkiteClusterName::parse("Default cluster").unwrap()),
    );
    let Observation::Present(cluster_outputs) = cluster_tool
        .read(&cluster_inputs)
        .expect("resolves the default cluster")
    else {
        panic!("buildkite.cluster.get always reports Present on success");
    };
    println!("step 4: `Default cluster` resolved to a cluster id");
    cluster_outputs
        .get(&PortName::parse("cluster").unwrap())
        .unwrap()
        .clone()
}

fn pipeline_inputs_for(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug, cluster: Value) -> Inputs {
    let mut pipeline_inputs = Inputs::new();
    pipeline_inputs.insert(PortName::parse("org").unwrap(), Value::known(org.clone()));
    pipeline_inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug.clone()));
    pipeline_inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    pipeline_inputs.insert(PortName::parse("cluster").unwrap(), cluster);
    pipeline_inputs
}

/// Step 4b: create the pipeline and confirm the organisation's pipeline
/// count rose by exactly one.
fn step4_create_and_verify(
    pipeline_tool: &BuildkitePipelineEnsure,
    pipeline_inputs: &Inputs,
    raw_http: &Http,
    org: &BuildkiteOrg,
    n0: usize,
    sink: &SinkToken,
) {
    let created = pipeline_tool
        .ensure(pipeline_inputs, sink)
        .expect("creates the pipeline");
    assert!(created.changed, "the first ensure must create the pipeline");

    let n_after_create = count_pipelines(raw_http, org);
    assert_eq!(
        n_after_create,
        n0 + 1,
        "the organisation must hold exactly one more pipeline after creation"
    );
    println!("step 4: pipeline created, count is now {n_after_create} (N0 + 1): pass");
}

fn bootstrap_inputs_for(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org.clone()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug.clone()));
    inputs.insert(
        PortName::parse("configuration").unwrap(),
        Value::known(bootstrap_file()),
    );
    inputs
}

fn gate_inputs_for(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org.clone()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug.clone()));
    inputs.insert(
        PortName::parse("expected").unwrap(),
        Value::known(bootstrap_file()),
    );
    inputs
}

/// Step 5: before any write, the bootstrap writer reads `Absent` with
/// `updates() == true`, and the gate (the same comparison, read-only)
/// reads `Absent` too.
fn step5_reads_absent_before_write(
    bootstrap_tool: &BuildkitePipelineBootstrapEnsure,
    gate_tool: &BuildkitePipelineBootstrapGate,
    bootstrap_inputs: &Inputs,
    gate_inputs: &Inputs,
) {
    let before_write = bootstrap_tool.read(bootstrap_inputs).expect("reads");
    assert!(
        matches!(before_write, Observation::Absent { .. }),
        "expected Absent before the first write, got {before_write:?}"
    );
    assert!(
        bootstrap_tool
            .updates(bootstrap_inputs)
            .expect("updates() reads"),
        "updates() must be true before the configuration has been written"
    );
    let gate_before = gate_tool.read(gate_inputs).expect("reads");
    assert!(
        matches!(gate_before, Observation::Absent { .. }),
        "expected the gate to read Absent before the first write, got {gate_before:?}"
    );
    println!("step 5: bootstrap writer and gate both read Absent before the first write: pass");
}

/// Step 6: write, converge, and confirm the gate agrees.
fn step6_write_and_converge(
    bootstrap_tool: &BuildkitePipelineBootstrapEnsure,
    gate_tool: &BuildkitePipelineBootstrapGate,
    bootstrap_inputs: &Inputs,
    gate_inputs: &Inputs,
    sink: &SinkToken,
) {
    let ensured = bootstrap_tool
        .ensure(bootstrap_inputs, sink)
        .expect("writes the configuration");
    assert!(
        ensured.changed,
        "the first ensure must write the configuration"
    );

    let after_write = bootstrap_tool.read(bootstrap_inputs).expect("reads");
    assert!(
        matches!(after_write, Observation::Present(_)),
        "expected Present after the write, got {after_write:?}"
    );

    let converged = bootstrap_tool
        .ensure(bootstrap_inputs, sink)
        .expect("re-ensures");
    assert!(
        !converged.changed,
        "a second ensure against an unchanged configuration must report changed: false"
    );

    let gate_after = gate_tool.read(gate_inputs).expect("reads");
    assert!(
        matches!(gate_after, Observation::Present(_)),
        "expected the gate to read Present after the write, got {gate_after:?}"
    );
    println!("step 6: ensure writes and converges, the gate now reads Present: pass");
}

/// Step 7: the pipeline's other attributes are unchanged by the `PATCH`
/// (verify item 1).
fn step7_pipeline_still_present(pipeline_tool: &BuildkitePipelineEnsure, pipeline_inputs: &Inputs) {
    let pipeline_after_write = pipeline_tool.read(pipeline_inputs).expect("reads");
    assert!(
        matches!(pipeline_after_write, Observation::Present(_)),
        "buildkite.pipeline.ensure must still read Present after the configuration PATCH, \
         got {pipeline_after_write:?}"
    );
    println!(
        "step 7: buildkite.pipeline.ensure still reads Present (repository, cluster and \
         description unchanged): pass"
    );
}

/// Step 8: delete directly through the client, re-read as `Absent`, and
/// confirm the organisation's pipeline count is back to `n0`.
fn step8_delete_and_confirm(
    client: &Arc<BuildkiteClient>,
    pipeline_tool: &BuildkitePipelineEnsure,
    raw_http: &Http,
    org: &BuildkiteOrg,
    slug: &BuildkitePipelineSlug,
    pipeline_inputs: &Inputs,
    n0: usize,
) {
    client
        .delete_pipeline(org, slug)
        .expect("deletes the pipeline");

    let after_delete = pipeline_tool.read(pipeline_inputs).expect("reads");
    assert!(
        matches!(after_delete, Observation::Absent { .. }),
        "expected Absent after delete, got {after_delete:?}"
    );

    let n_after_delete = count_pipelines(raw_http, org);
    assert_eq!(
        n_after_delete, n0,
        "the organisation must hold exactly the pipelines it held before this run"
    );
    println!("step 8: pipeline deleted, count is back to {n_after_delete} (N0): pass");
}

#[test]
#[ignore = "provisions and deletes a real Buildkite pipeline, and writes its stored \
            configuration; run with WILLIKINS_LIVE_TESTS=1 and a sandbox bkua_ token \
            sourced in the same command, under the live-tests feature."]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
fn buildkite_live_bootstrap_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let org = org();
    let slug = slug();

    // A second credential read and a second `Http`, independent of the
    // one `BuildkiteClient` below holds: `Credential` is deliberately
    // not `Clone` (its own doc), and `count_pipelines`/`BuildCounts::read`
    // are raw diagnostic reads that go through `Http` directly rather
    // than through any typed tool.
    let raw_http = willikins_providers_buildkite::http_client(
        willikins_providers_buildkite::credential_from_env()
            .expect("a valid sandbox Buildkite token"),
    );

    let credential = willikins_providers_buildkite::credential_from_env()
        .expect("a valid sandbox Buildkite token");
    let client = Arc::new(BuildkiteClient::new(
        willikins_providers_buildkite::http_client(credential),
    ));

    let pipeline_tool = BuildkitePipelineEnsure::new(Arc::clone(&client));
    let cluster_tool = BuildkiteClusterGet::new(Arc::clone(&client));
    let bootstrap_tool = BuildkitePipelineBootstrapEnsure::new(Arc::clone(&client));
    let gate_tool = BuildkitePipelineBootstrapGate::new(Arc::clone(&client));
    let sink = SinkToken::new();

    // Step 1.
    let n0 = count_pipelines(&raw_http, &org);
    println!("step 1: the organisation holds {n0} pipelines before this run");

    // Step 2.
    check_absent_before_start(&raw_http, &org, &slug);

    // Step 3: arm the guard, before any assertion that could panic.
    let guard = PipelineGuard {
        client: Arc::clone(&client),
        org: org.clone(),
        slug: slug.clone(),
        armed: true,
    };

    // Step 4.
    let cluster = resolve_default_cluster(&cluster_tool, &org);
    let pipeline_inputs = pipeline_inputs_for(&org, &slug, cluster);
    step4_create_and_verify(&pipeline_tool, &pipeline_inputs, &raw_http, &org, n0, &sink);

    // Step 5.
    let bootstrap_inputs = bootstrap_inputs_for(&org, &slug);
    let gate_inputs = gate_inputs_for(&org, &slug);
    step5_reads_absent_before_write(&bootstrap_tool, &gate_tool, &bootstrap_inputs, &gate_inputs);

    // Step 6, with build counts recorded around the write (verify item 3).
    let counts_before = BuildCounts::read(&raw_http, &org, &slug);
    println!(
        "step 6: build counts before the write: scheduled={:?} running={:?}",
        counts_before.scheduled_builds_count, counts_before.running_builds_count
    );
    step6_write_and_converge(
        &bootstrap_tool,
        &gate_tool,
        &bootstrap_inputs,
        &gate_inputs,
        &sink,
    );
    let counts_after = BuildCounts::read(&raw_http, &org, &slug);
    println!(
        "step 6: build counts after the write: scheduled={:?} running={:?}",
        counts_after.scheduled_builds_count, counts_after.running_builds_count
    );

    // Step 7.
    step7_pipeline_still_present(&pipeline_tool, &pipeline_inputs);

    // Step 8.
    step8_delete_and_confirm(
        &client,
        &pipeline_tool,
        &raw_http,
        &org,
        &slug,
        &pipeline_inputs,
        n0,
    );

    guard.disarm();
}

/// The after-the-run confirmation, read-only and independent of the
/// cycle's own assertions: no `willikins-bootstrap-*` slug remains in
/// the organisation. Gated behind a second variable so the cycle's own
/// command never runs the two concurrently, and named so the cycle
/// cannot start creating the very pipeline this check asserts is gone.
#[test]
#[ignore = "opt-in read-only check that the bootstrap cycle left nothing behind; run with \
            WILLIKINS_LIVE_TESTS=1 and WILLIKINS_LIVE_LEFTOVER_CHECK=1"]
fn the_bootstrap_cycles_pipeline_is_gone() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1")
        || std::env::var("WILLIKINS_LIVE_LEFTOVER_CHECK").as_deref() != Ok("1")
    {
        println!("skip: WILLIKINS_LIVE_TESTS and WILLIKINS_LIVE_LEFTOVER_CHECK are not both 1");
        return;
    }

    let org = org();
    let http = willikins_providers_buildkite::http_client(
        willikins_providers_buildkite::credential_from_env()
            .expect("a valid sandbox Buildkite token"),
    );

    let mut leftovers = Vec::new();
    for page in 1..=MAX_PIPELINE_PAGES {
        let items: Vec<PipelineSlugOnly> = http
            .get(&format!(
                "/v2/organizations/{org}/pipelines?page={page}&per_page=100"
            ))
            .expect("lists the organisation's pipelines");
        let len = items.len();
        for item in &items {
            if item.slug.starts_with(SLUG_PREFIX) {
                leftovers.push(item.slug.clone());
            }
        }
        if len < 100 {
            break;
        }
    }

    println!(
        "leftover check: {} willikins-bootstrap-* slugs found",
        leftovers.len()
    );
    assert!(
        leftovers.is_empty(),
        "leftover check: these must be deleted by hand: {leftovers:?}"
    );
}
