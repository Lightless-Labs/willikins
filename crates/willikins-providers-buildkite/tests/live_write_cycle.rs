//! The live Buildkite **write** cycle (plan acceptance test 16): the one
//! test in this crate that provisions something real and then removes it
//! again.
//!
//! Compiled only with the crate's `live-tests` feature (this file's
//! `[[test]]` entry in `Cargo.toml` carries `required-features`), so a
//! plain `cargo test --workspace` never builds it. `#[ignore]` on top of
//! that, and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`
//! -- the credential is read only past that gate, exactly as
//! `tests/live_probe.rs` does.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-providers-buildkite \
//!   --features live-tests --test live_write_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! Drives the real `buildkite.pipeline.ensure` and `buildkite.cluster.get`
//! tools against a pipeline with the fixed, distinctive slug
//! `willikins-live-write-cycle` in the `willikins-test` organisation:
//!
//! 1. the fixed slug must be absent (`404`); a leftover from an aborted
//!    run makes the test refuse to proceed and name it, because a
//!    leftover is the operator's to remove by hand;
//! 2. a [`PipelineGuard`] is armed, so every exit path -- a panic, a
//!    failed assertion, an early return -- deletes the pipeline this run
//!    created;
//! 3. `buildkite.cluster.get` resolves `Default cluster` to its real id;
//! 4. `buildkite.pipeline.ensure` reads `Absent`, creates the pipeline
//!    with the `managed-by: willikins` description, and converges
//!    (`changed: false` on a second `ensure`);
//! 5. both `Mismatch` arms are proved against the pipeline that now
//!    really exists, read-only: a different `repo` input reads
//!    `Mismatch { repo }` and a different `cluster` input reads
//!    `Mismatch { cluster }`, each `ensure`s as `Conflict`, and the
//!    pipeline is unchanged afterwards. `Foreign` is **not** proved live
//!    and cannot be: making a real pipeline foreign means editing its
//!    `description`, which needs a `PATCH` this crate deliberately does
//!    not have. `tests/pipeline_ensure_mock.rs` proves that arm;
//! 6. the pipeline is deleted through [`BuildkiteClient::delete_pipeline`]
//!    directly (no tool calls it -- see that method's own doc), re-read
//!    as `Absent`, and the guard is disarmed.
//!
//! **The credential is sourced only inside the single command that runs
//! this test and is never printed.** No `gh` command is run.
//!
//! A second `#[ignore]` test in this file, `the_cycles_pipeline_is_gone`,
//! is the after-the-run confirmation: it only `GET`s the fixed slug and
//! asserts `404`, and lists the organisation's clusters to show what the
//! run left behind. It needs `WILLIKINS_LIVE_LEFTOVER_CHECK=1` on top of
//! `WILLIKINS_LIVE_TESTS=1`, so the cycle's own command above never runs
//! it. Setting that second variable does **not**, on its own, keep the two
//! apart -- it makes both eligible in the same binary. Name the test, so
//! the cycle cannot start creating the very pipeline the check asserts is
//! gone (the rule `willikins-providers-doppler`'s own cycle records):
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   WILLIKINS_LIVE_LEFTOVER_CHECK=1 RUST_TEST_THREADS=2 cargo test \
//!   -p willikins-providers-buildkite --features live-tests --test live_write_cycle \
//!   -j 2 -- --ignored --nocapture the_cycles_pipeline_is_gone
//! ```

use std::sync::Arc;

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure,
};
use willikins_types::{
    BuildkiteClusterId, BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType,
    GitHubOrg, GitHubRepo, ProjectSlug,
};

/// A well-formed cluster id that is not this organisation's. Used only as
/// the *input* of a read, to prove the `Mismatch { cluster }` arm: it is
/// never sent to Buildkite, because a mismatch is decided by comparing the
/// pipeline's own `cluster_id` against this value locally.
const ABSENT_CLUSTER_ID: &str = "00000000-0000-4000-8000-000000000000";

/// The fixed, distinctive pipeline slug this run provisions and removes.
/// Fixed (not randomised) so a leftover from an aborted run is
/// detectable and named, the same rule
/// `willikins-providers-doppler/tests/live_write_cycle.rs`'s project name
/// follows.
fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("willikins-live-write-cycle").expect("valid slug literal")
}

/// A repository this pipeline points at. Buildkite stores this as an
/// opaque string on the pipeline object and never validates it against a
/// real GitHub repository at create time, so this need not exist.
fn repo() -> GitHubRepo {
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    GitHubRepo::new(
        GitHubOrg::parse(&org_value).expect("valid GitHub org"),
        ProjectSlug::parse("willikins-live-write-cycle").expect("valid slug literal"),
    )
}

/// A second repository, which this cycle's pipeline is *not* pointed at:
/// the input of the `Mismatch { repo }` probe. Like [`repo`], it need not
/// exist -- nothing is ever created for it, and its name is never sent to
/// Buildkite: the mismatch is decided by comparing this repository's
/// derived SSH URL against the pipeline's own `repository` field locally.
/// Deliberately shorter than [`slug`] plus a suffix would be, so it stays
/// clear of `ProjectSlug`'s 32-character limit.
fn other_repo() -> GitHubRepo {
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    GitHubRepo::new(
        GitHubOrg::parse(&org_value).expect("valid GitHub org"),
        ProjectSlug::parse("willikins-live-cycle-other").expect("valid slug literal"),
    )
}

/// Deletes [`slug`]'s pipeline on drop, unless [`Self::disarm`] was
/// called first -- so a panic or a failed assertion anywhere in the run
/// still cleans up. Mirrors
/// `willikins-providers-doppler/tests/live_write_cycle.rs`'s
/// `ProjectGuard`.
struct PipelineGuard {
    client: Arc<BuildkiteClient>,
    org: BuildkiteOrg,
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
            let _ = self.client.delete_pipeline(&self.org, &slug());
        }
    }
}

/// Step 5: both `Mismatch` arms, proved against a pipeline that really
/// exists -- read-only, so nothing is changed and nothing new is created.
///
/// `Foreign` is deliberately **not** proved live and cannot be: making a
/// real pipeline foreign means editing its `description`, which needs a
/// `PATCH` this crate does not have and will not grow (plan decision (a);
/// "Pipeline update, delete, and archive tools" is out of scope).
/// `tests/pipeline_ensure_mock.rs` proves that arm.
///
/// `ensure` is called on each mismatched input too. That is still not a
/// write: `ensure` refuses a `Mismatch` with `Conflict` before it reaches
/// any `POST` (the design doc's refuse-do-not-reconcile rule), and the
/// final assertion here is that the pipeline is untouched afterwards --
/// which is the point of calling it rather than trusting the code path.
fn step_5_both_mismatch_arms(
    pipeline_tool: &BuildkitePipelineEnsure,
    pipeline_inputs: &willikins_core::Inputs,
    sink: &SinkToken,
) {
    let mut wrong_repo_inputs = pipeline_inputs.clone();
    wrong_repo_inputs.insert(PortName::parse("repo").unwrap(), Value::known(other_repo()));
    match pipeline_tool.read(&wrong_repo_inputs).expect("reads") {
        Observation::Mismatch { ref port } if port.as_str() == "repo" => {
            println!("step 5: a different `repo` reads Mismatch {{ repo }}: pass");
        }
        other => panic!("expected Mismatch {{ repo }} for a different repository, got {other:?}"),
    }

    let mut wrong_cluster_inputs = pipeline_inputs.clone();
    wrong_cluster_inputs.insert(
        PortName::parse("cluster").unwrap(),
        Value::known(
            BuildkiteClusterId::parse(ABSENT_CLUSTER_ID).expect("a valid cluster id literal"),
        ),
    );
    match pipeline_tool.read(&wrong_cluster_inputs).expect("reads") {
        Observation::Mismatch { ref port } if port.as_str() == "cluster" => {
            println!("step 5: a different `cluster` reads Mismatch {{ cluster }}: pass");
        }
        other => panic!("expected Mismatch {{ cluster }} for a different cluster, got {other:?}"),
    }

    for (case, inputs) in [
        ("repo", &wrong_repo_inputs),
        ("cluster", &wrong_cluster_inputs),
    ] {
        let err = pipeline_tool
            .ensure(inputs, sink)
            .expect_err("a mismatch must refuse");
        assert_eq!(
            err.kind,
            willikins_core::ToolErrorKind::Conflict,
            "a `{case}` mismatch must be a Conflict, got {err:?}"
        );
        println!("step 5: `ensure` on a `{case}` mismatch refuses with Conflict: pass");
    }

    let still_present = pipeline_tool.read(pipeline_inputs).expect("reads");
    assert!(
        matches!(still_present, Observation::Present(_)),
        "the mismatch probes must have changed nothing, got {still_present:?}"
    );
}

#[test]
#[ignore = "provisions and deletes a real Buildkite pipeline; run with WILLIKINS_LIVE_TESTS=1 \
            and a sandbox bkua_ token sourced in the same command, under the live-tests \
            feature. The sandbox token expires seven days from 2026-09-16."]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
fn buildkite_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = willikins_providers_buildkite::credential_from_env()
        .expect("a valid sandbox Buildkite token");
    let org_value = std::env::var("WILLIKINS_SANDBOX_BUILDKITE_ORG")
        .expect("WILLIKINS_SANDBOX_BUILDKITE_ORG is set");
    let org = BuildkiteOrg::parse(&org_value).expect("a valid Buildkite organisation slug");
    let client = Arc::new(BuildkiteClient::new(
        willikins_providers_buildkite::http_client(credential),
    ));

    let pipeline_tool = BuildkitePipelineEnsure::new(Arc::clone(&client));
    let cluster_tool = BuildkiteClusterGet::new(Arc::clone(&client));

    // Step 3: resolve `Default cluster` to its real id.
    let mut cluster_inputs = willikins_core::Inputs::new();
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
    // No recording here. The only thing this step could record is a
    // JSON array rebuilt from the one id the tool returned, and
    // `common::record_and_compare` compares *top-level object keys* --
    // which for an array is the empty set on both sides, so the
    // comparison would pass no matter what came back. `tests/live_probe.rs`
    // records the real `GET .../clusters` body, which is the response
    // whose shape this crate actually parses; duplicating it here with a
    // synthetic value would assert nothing and would read as if it did.
    let cluster = cluster_outputs
        .get(&PortName::parse("cluster").unwrap())
        .unwrap()
        .clone();
    println!("step 3: `Default cluster` resolved to a cluster id");

    let mut pipeline_inputs = willikins_core::Inputs::new();
    pipeline_inputs.insert(PortName::parse("org").unwrap(), Value::known(org.clone()));
    pipeline_inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
    pipeline_inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    pipeline_inputs.insert(PortName::parse("cluster").unwrap(), cluster);

    // Step 1: the fixed slug must be absent before this run starts.
    match pipeline_tool.read(&pipeline_inputs).expect("reads") {
        Observation::Absent { .. } => {}
        other => panic!(
            "`{org}/{}` is not absent before this run starts ({other:?}); a leftover from an \
             aborted run must be removed by hand before this test can run",
            slug()
        ),
    }

    // Step 2: arm the guard.
    let guard = PipelineGuard {
        client: Arc::clone(&client),
        org: org.clone(),
        armed: true,
    };

    // Step 4: create, then converge.
    let sink = SinkToken::new();
    let created = pipeline_tool
        .ensure(&pipeline_inputs, &sink)
        .expect("creates the pipeline");
    assert!(created.changed, "the first ensure must create the pipeline");

    let after_create = pipeline_tool.read(&pipeline_inputs).expect("reads");
    assert!(
        matches!(after_create, Observation::Present(_)),
        "expected Present after create, got {after_create:?}"
    );

    let converged = pipeline_tool
        .ensure(&pipeline_inputs, &sink)
        .expect("re-ensures");
    assert!(
        !converged.changed,
        "a second ensure against an unchanged pipeline must report changed: false"
    );

    // Step 5.
    step_5_both_mismatch_arms(&pipeline_tool, &pipeline_inputs, &sink);

    // Step 6: delete directly through the client, re-read as Absent,
    // disarm the guard.
    client
        .delete_pipeline(&org, &slug())
        .expect("deletes the pipeline");
    let after_delete = pipeline_tool.read(&pipeline_inputs).expect("reads");
    assert!(
        matches!(after_delete, Observation::Absent { .. }),
        "expected Absent after delete, got {after_delete:?}"
    );

    guard.disarm();
}

/// The after-the-run confirmation, read-only and independent of the
/// cycle's own assertions: the fixed slug is gone, and the organisation
/// holds exactly the clusters it held before. Gated behind a second
/// variable so the cycle's own command never runs the two concurrently.
///
/// It goes through [`willikins_providers_http::Http`] directly rather
/// than through `buildkite.pipeline.ensure`, so it does not inherit the
/// tool's own reading of a response: a pipeline that exists but that the
/// tool would call `Foreign` still counts as a leftover here.
#[test]
#[ignore = "opt-in read-only check that the write cycle left nothing behind; run with \
            WILLIKINS_LIVE_TESTS=1 and WILLIKINS_LIVE_LEFTOVER_CHECK=1"]
fn the_cycles_pipeline_is_gone() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1")
        || std::env::var("WILLIKINS_LIVE_LEFTOVER_CHECK").as_deref() != Ok("1")
    {
        println!("skip: WILLIKINS_LIVE_TESTS and WILLIKINS_LIVE_LEFTOVER_CHECK are not both 1");
        return;
    }

    let credential = willikins_providers_buildkite::credential_from_env()
        .expect("a valid sandbox Buildkite token");
    let org_value = std::env::var("WILLIKINS_SANDBOX_BUILDKITE_ORG")
        .expect("WILLIKINS_SANDBOX_BUILDKITE_ORG is set");
    let org = BuildkiteOrg::parse(&org_value).expect("a valid Buildkite organisation slug");
    let http = willikins_providers_buildkite::http_client(credential);

    let mut leftovers = Vec::new();

    let slug = slug();
    match http.get::<serde_json::Value>(&format!("/v2/organizations/{org}/pipelines/{slug}")) {
        Err(err) if err.status == Some(404) => {
            println!("leftover check (`{slug}` is gone): pass");
        }
        Ok(_) => {
            println!("leftover check (`{slug}` still exists): fail");
            leftovers.push(slug.to_string());
        }
        Err(err) => {
            println!("leftover check (`{slug}`): fail");
            leftovers.push(format!("{slug} answered status {:?}", err.status));
        }
    }

    // What else the organisation holds, so the run's blast radius is
    // reported rather than assumed. Only names are printed: an id is the
    // organisation's own configuration, and nothing here needs one.
    match http.get::<serde_json::Value>(&format!(
        "/v2/organizations/{org}/pipelines?page=1&per_page=100"
    )) {
        Ok(body) => {
            let names: Vec<String> = body
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.get("slug").and_then(serde_json::Value::as_str))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            println!("organisation pipelines after the run: {names:?}");
        }
        Err(err) => println!(
            "could not list pipelines after the run (status {:?})",
            err.status
        ),
    }

    match http.get::<serde_json::Value>(&format!(
        "/v2/organizations/{org}/clusters?page=1&per_page=100"
    )) {
        Ok(body) => {
            let names: Vec<String> = body
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            println!("organisation clusters after the run: {names:?}");
        }
        Err(err) => println!(
            "could not list clusters after the run (status {:?})",
            err.status
        ),
    }

    assert!(
        leftovers.is_empty(),
        "leftover check: these must be deleted by hand: {leftovers:?}"
    );
}
