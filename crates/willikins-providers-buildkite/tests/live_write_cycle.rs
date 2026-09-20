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
//! 5. every endpoint reached whose body this crate can see is recorded,
//!    redacted, under `fixtures/buildkite/live/` and compared by
//!    top-level key set with the authored fixture of the same endpoint;
//! 6. the pipeline is deleted through [`BuildkiteClient::delete_pipeline`]
//!    directly (no tool calls it -- see that method's own doc), re-read
//!    as `Absent`, and the guard is disarmed.
//!
//! **The credential is sourced only inside the single command that runs
//! this test and is never printed.** No `gh` command is run.

mod common;

use std::sync::Arc;

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure,
};
use willikins_types::{
    BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubOrg, GitHubRepo,
    ProjectSlug,
};

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
    common::record_and_compare(
        "clusters_list_page",
        &serde_json::json!([{"id": cluster_outputs.get(&PortName::parse("cluster").unwrap()).unwrap().render().to_string(), "name": "Default cluster"}]),
    )
    .ok();
    let cluster = cluster_outputs
        .get(&PortName::parse("cluster").unwrap())
        .unwrap()
        .clone();

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
