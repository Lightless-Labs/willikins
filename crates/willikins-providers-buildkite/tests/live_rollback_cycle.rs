//! The live Buildkite **rollback** cycle: milestone 3m task L3
//! (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`), verify item 8,
//! read "as L1" (`willikins-providers-github`'s own
//! `tests/live_rollback_cycle.rs`): this task's own scoped command names
//! only this crate, so it calls `buildkite.pipeline.ensure`'s tool and its
//! inverse directly, building `Created` by hand (the same shape
//! `tests/it/pipeline_inverse_mock.rs` already uses, but against the real
//! sandbox org), never through `willikins-core`'s
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
//! own task table, sonnet writes L3, the coordinator runs it once:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
//!   cargo test -p willikins-providers-buildkite --features live-tests \
//!   --test live_rollback_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! # Three independent sub-cycles, each disposing of its own throwaway pipeline
//!
//! Every pipeline slug starts with the plan's own SHARED VALUES "Live
//! probe prefix", [`PROBE_PREFIX`] (`willikins-probe-delete-me-`),
//! followed by the current Unix time and a one-letter tag.
//! `BuildkitePipelineSlug`'s own 100-character bound leaves far more room
//! than `GitHubRepo`'s 32 did for `willikins-providers-github`'s own
//! `tests/live_rollback_cycle.rs`, so no truncation trick is needed here.
//!
//! - **`-a`: clean rollback.** `buildkite.pipeline.ensure` creates it; a
//!   raw `GET` confirms verify item 8's own `created_at` field is present
//!   and records which of [`local_parse_pipeline_created_at`]'s two forms
//!   a real pipeline's response actually uses (`crate::client`'s own
//!   `parse_pipeline_created_at` doc flags this as unconfirmed, left to
//!   this task). The inverse's `standing` reports `AsCreated` with no
//!   losing names (no build was ever triggered); `delete` (this file's
//!   own `TeardownToken`) deletes it; a raw `GET` afterwards is `404`.
//! - **`-b`: an unaccepted build loss.** Created, then a build is
//!   triggered on it through a raw `POST
//!   /v2/organizations/{org}/pipelines/{slug}/builds` (Buildkite's own
//!   documented minimal body: `commit: "HEAD"`, `branch: "main"` --
//!   fetched verbatim 2026-10-12, `builds.md`'s "Create a Build"
//!   section). A raw `GET .../builds?per_page=1` confirms verify item
//!   8's own builds-listing endpoint now lists one (the other half of
//!   verify item 8, complementing `-a`'s `created_at` check). The
//!   inverse's `standing` then reports `AsCreated { losing: ["builds"] }`
//!   -- decision 3a's own declared loss, never `Drifted`; `delete`, with
//!   `Created::accept` empty (this run's own `teardown: delete` carries
//!   no `accept: [builds]`), refuses as a `Conflict`. Deleted by hand.
//! - **`-c`: the same build loss, accepted.** Created and built exactly
//!   as `-b`; `standing` again reports `AsCreated { losing: ["builds"] }`,
//!   but this time `Created::accept` carries `Loss::parse("builds")`
//!   (decision 3a: a document may accept the loss), so `delete` succeeds
//!   despite the build -- a raw `GET` afterwards is `404`.
//!
//! No `-b`/`-c` pipeline's build is ever itself inspected past "does the
//! list come back non-empty": `has_any_build`'s own contract (`crate::client`)
//! needs to know only *that* a build exists, never which one, and this
//! file follows the same rule for its own raw check.
//!
//! # Why raw requests for verification, and for triggering a build
//!
//! `BuildkiteClient`'s read methods this crate's own P4 task added
//! (`get_pipeline_standing`, `has_any_build`, `parse_pipeline_created_at`)
//! stay `pub(crate)`, exactly as `tests/live_write_cycle.rs`'s own doc
//! explains for this crate's read/write methods generally: a `tests/*.rs`
//! target links this crate as an external dependency and cannot see
//! `pub(crate)` items. Every independent check this file makes, and the
//! one write this crate's own tools have no method for at all (creating a
//! build), is therefore a raw request against the shared
//! [`willikins_providers_http::Http`] client. [`local_parse_pipeline_created_at`]
//! is this file's own copy of `client.rs`'s private
//! `parse_pipeline_created_at`, kept independent on purpose, the same
//! precedent `willikins-providers-github`'s own `repo_path`/`shift` set.
//!
//! # No sandbox-org literal check
//!
//! Unlike `willikins-providers-github`'s own `tests/live_rollback_cycle.rs`
//! (whose `sandbox_org_from` refuses every org but `Willikins-Test`,
//! inherited from milestone 3l's own L1), this crate's two pre-existing
//! live cycles (`tests/live_write_cycle.rs`, `tests/live_bootstrap_cycle.rs`)
//! read `WILLIKINS_SANDBOX_BUILDKITE_ORG` directly, with no hardcoded
//! organisation literal to check it against -- this file follows that
//! existing precedent rather than importing GitHub's stricter one, since
//! nothing in this crate names the sandbox org anywhere else either.
//!
//! # Redaction
//!
//! Nothing this file prints, records, or formats may contain the real
//! token's bytes or anything credential-shaped
//! ([`CREDENTIAL_PREFIXES`]). No secret-shaped value is ever hand-set by
//! this cycle (unlike `willikins-providers-github`'s and
//! `willikins-providers-doppler`'s own `-c` sub-cycles): a pipeline's only
//! declared loss is its builds, which carry no secret of their own.
//! [`sweep_for_secrets`] checks every string kept for that purpose at the
//! end.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value as Json;
use willikins_core::rollback::{CREATION_SKEW, CreationWindow};
use willikins_core::{
    Created, Inputs, Inverse, Loss, Outputs, PortName, SinkToken, Standing, Timestamp, Tool,
    ToolErrorKind, Value,
};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure, credential_from_env,
    http_client,
};
use willikins_providers_http::Http;
use willikins_types::{
    BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubOrg, GitHubRepo,
    ProjectSlug, TeardownToken,
};

/// The token prefixes `willikins_providers_buildkite::CREDENTIAL_PATTERN`
/// accepts, plus the `Authorization` scheme their bytes travel in.
/// Nothing this file prints, records, or formats may contain either.
const CREDENTIAL_PREFIXES: &[&str] = &["bkua_", "Bearer"];

/// Every throwaway pipeline this cycle creates starts with this prefix --
/// the plan's own SHARED VALUES row "Live probe prefix",
/// `willikins-probe-delete-me-`, owned by L1-L3. Step 1 refuses to
/// proceed if one already exists, and the final step deletes exactly the
/// one -b/-c pipeline each sub-cycle's own hand cleanup does not already
/// cover via the inverse.
const PROBE_PREFIX: &str = "willikins-probe-delete-me-";

/// The greatest number of pipeline pages this file's own leftover scan
/// will fetch before giving up -- mirrors
/// `tests/live_bootstrap_cycle.rs`'s own `MAX_PIPELINE_PAGES`.
const MAX_PIPELINE_PAGES: u32 = 20;

/// The current Unix time in whole seconds, for this cycle's own throwaway
/// pipeline slugs.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock reads a time after 1970")
        .as_secs()
}

/// A fresh pipeline slug: `willikins-probe-delete-me-<unix seconds><tag>`.
/// `BuildkitePipelineSlug`'s own 100-character bound (unlike
/// `GitHubRepo`'s 32) needs no truncation trick here.
fn build_slug(timestamp: u64, tag: &str) -> BuildkitePipelineSlug {
    let name = format!("{PROBE_PREFIX}{timestamp}{tag}");
    BuildkitePipelineSlug::parse(&name)
        .unwrap_or_else(|err| panic!("`{name}` must be a valid BuildkitePipelineSlug: {}", err.reason))
}

/// The sandbox Buildkite org this cycle runs against, from
/// `WILLIKINS_SANDBOX_BUILDKITE_ORG` -- see this file's own "No
/// sandbox-org literal check" doc section for why there is no fixed
/// literal to validate it against.
fn sandbox_org() -> BuildkiteOrg {
    let value = std::env::var("WILLIKINS_SANDBOX_BUILDKITE_ORG")
        .expect("WILLIKINS_SANDBOX_BUILDKITE_ORG is set");
    BuildkiteOrg::parse(&value).unwrap_or_else(|err| {
        panic!("`{value}` is not a valid BuildkiteOrg: {}", err.reason)
    })
}

/// A repository this cycle's pipelines point at. Buildkite stores this as
/// an opaque string on the pipeline object and never validates it against
/// a real GitHub repository at create time, so this need not exist --
/// mirrors `tests/live_write_cycle.rs`'s own `repo`.
fn repo() -> GitHubRepo {
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    GitHubRepo::new(
        GitHubOrg::parse(&org_value).expect("valid GitHub org"),
        ProjectSlug::parse("willikins-rollback-cycle").expect("valid slug literal"),
    )
}

/// The test's own sink token, for a real `ensure` call (see
/// `tests/live_write_cycle.rs`'s own `sink_token` for why this is the
/// sanctioned narrow opt-in).
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// This file's own teardown token, the one legitimate test-side mint of
/// the capability `willikins_core::rollback::apply_teardown` otherwise
/// holds exclusively (milestone 3m trust boundary 2, task R2). This file
/// calls an inverse's `delete` directly, outside `apply_teardown`
/// entirely, so it must opt into the same narrow exemption every mock
/// test in this crate's own `tests/it/pipeline_inverse_mock.rs` already
/// uses.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn teardown_token() -> TeardownToken {
    TeardownToken::new()
}

/// `at` shifted by `seconds` (positive or negative) -- this file's own
/// copy of `willikins_core::rollback`'s private `shift`, unreachable from
/// this separate integration-test crate. Mirrors
/// `willikins-providers-github`'s own `tests/live_rollback_cycle.rs` copy
/// of the same algorithm.
fn shift(at: Timestamp, seconds: i64) -> Timestamp {
    Timestamp::from_datetime(*at.as_datetime() + chrono::Duration::seconds(seconds))
}

/// This cycle's own creation window (milestone 3m decision 2):
/// `[before - CREATION_SKEW, after + CREATION_SKEW]`, around a real
/// `ensure` call's own before/after wall-clock instants.
fn creation_window(before: Timestamp, after: Timestamp) -> CreationWindow {
    CreationWindow {
        start: shift(before, -CREATION_SKEW),
        end: shift(after, CREATION_SKEW),
    }
}

/// This cycle's own `settled_by` (decision 2): `after + CREATION_SKEW`.
/// Never actually read by `BuildkitePipelineInverse::standing`
/// (decision 7's buildkite row consults only `Created::window`), but
/// `Created` requires one, mirroring
/// `willikins-providers-doppler`'s own `tests/live_rollback_cycle.rs`
/// doc section "No `settled_by` sleep".
fn settled_by_from(after: Timestamp) -> Timestamp {
    shift(after, CREATION_SKEW)
}

/// This file's own copy of `client.rs`'s private
/// `parse_pipeline_created_at` (unreachable, `pub(crate)`, from this
/// separate integration-test crate): [`Timestamp::parse`] (RFC 3339)
/// tried first, Buildkite's own documented `"<date> <time> UTC"` form
/// second. Unlike the crate's own copy, this one also records *which*
/// form matched, settling verify item 8's own open question.
fn local_parse_pipeline_created_at(raw: &str) -> Option<(Timestamp, &'static str)> {
    if let Ok(parsed) = Timestamp::parse(raw) {
        return Some((parsed, "rfc3339"));
    }
    let naive = raw
        .strip_suffix(" UTC")
        .and_then(|rest| chrono::NaiveDateTime::parse_from_str(rest, "%Y-%m-%d %H:%M:%S").ok())?;
    Some((Timestamp::from_datetime(naive.and_utc()), "buildkite documented form"))
}

/// `GET /v2/organizations/{org}/pipelines/{slug}`, raw.
fn get_pipeline_raw(raw: &Http, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Json {
    raw.get(&format!("/v2/organizations/{org}/pipelines/{slug}"))
        .unwrap_or_else(|err| panic!("GET of `{org}/{slug}` failed (status {:?})", err.status))
}

/// Verify item 8's own `created_at` half: a raw `GET` must carry a
/// non-null `created_at`, and it must parse under
/// [`local_parse_pipeline_created_at`]. Returns which form matched, for
/// the caller to print.
fn assert_created_at_present_and_parseable(body: &Json) -> &'static str {
    let raw = body
        .get("created_at")
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("verify item 8: the pipeline's raw GET carried no `created_at`"));
    let (_, form) = local_parse_pipeline_created_at(raw).unwrap_or_else(|| {
        panic!("verify item 8: `created_at` ({raw:?}) parsed under neither RFC 3339 nor Buildkite's documented form")
    });
    form
}

/// `POST /v2/organizations/{org}/pipelines/{slug}/builds`, raw --
/// Buildkite's own documented minimal body (`builds.md`'s "Create a
/// Build" section, fetched verbatim 2026-10-12): `commit: "HEAD"`,
/// `branch: "main"`. No tool in this crate has a method for this at all
/// (decision 4's table declares no `buildkite.build.*` tool), so this is
/// the one write in this file that is not exercising an existing client
/// method.
fn trigger_a_build(raw: &Http, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) {
    let body = serde_json::json!({
        "commit": "HEAD",
        "branch": "main",
        "message": "willikins live rollback cycle (L3): throwaway build, never inspected past existence",
    });
    raw.post::<Json>(
        &format!("/v2/organizations/{org}/pipelines/{slug}/builds"),
        &body,
    )
    .unwrap_or_else(|err| {
        panic!(
            "triggering a build on `{org}/{slug}` failed (status {:?})",
            err.status
        )
    });
}

/// `GET /v2/organizations/{org}/pipelines/{slug}/builds?per_page=1`, raw
/// -- this file's own copy of `has_any_build`'s own query (`pub(crate)`,
/// unreachable from this separate integration-test crate). Verify item
/// 8's other half.
fn has_any_build_raw(raw: &Http, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> bool {
    let builds: Vec<Json> = raw
        .get(&format!(
            "/v2/organizations/{org}/pipelines/{slug}/builds?per_page=1"
        ))
        .unwrap_or_else(|err| {
            panic!(
                "listing `{org}/{slug}`'s builds failed (status {:?})",
                err.status
            )
        });
    !builds.is_empty()
}

/// Every pipeline name in `org`, paged to the first short page -- mirrors
/// `tests/live_bootstrap_cycle.rs`'s own `count_pipelines`/`PipelineSlugOnly`,
/// but collecting names (for the leftover check) rather than only a
/// count.
fn list_pipeline_slugs(raw: &Http, org: &BuildkiteOrg) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct SlugOnly {
        slug: String,
    }
    let mut names = Vec::new();
    for page in 1..=MAX_PIPELINE_PAGES {
        let items: Vec<SlugOnly> = raw
            .get(&format!(
                "/v2/organizations/{org}/pipelines?page={page}&per_page=100"
            ))
            .unwrap_or_else(|err| {
                panic!(
                    "listing `{org}`'s pipelines failed (status {:?})",
                    err.status
                )
            });
        let len = items.len();
        names.extend(items.into_iter().map(|item| item.slug));
        if len < 100 {
            return names;
        }
    }
    panic!(
        "more than {MAX_PIPELINE_PAGES} pages of pipelines in `{org}`; raise the bound or \
         narrow the organisation this test runs against"
    );
}

/// `buildkite.cluster.get`'s own resolution of `Default cluster` to its
/// real id -- mirrors `tests/live_write_cycle.rs`'s own
/// `resolve_default_cluster`.
fn resolve_default_cluster(cluster_tool: &BuildkiteClusterGet, org: &BuildkiteOrg) -> Value {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("org").unwrap(),
        Value::known(org.clone()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(BuildkiteClusterName::parse("Default cluster").unwrap()),
    );
    let willikins_core::Observation::Present(outputs) = cluster_tool
        .read(&inputs)
        .expect("resolves the default cluster")
    else {
        panic!("buildkite.cluster.get always reports Present on success");
    };
    outputs
        .get(&PortName::parse("cluster").unwrap())
        .unwrap()
        .clone()
}

/// `buildkite.pipeline.ensure`'s own inputs for one real `ensure` call.
fn ensure_inputs(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug, cluster: Value) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("org").unwrap(),
        Value::known(org.clone()),
    );
    inputs.insert(
        PortName::parse("slug").unwrap(),
        Value::known(slug.clone()),
    );
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(PortName::parse("cluster").unwrap(), cluster);
    inputs
}

/// Exactly `buildkite.pipeline.ensure`'s declared `Reversal::Delete::needs`
/// minus `token` (this cycle never binds a document `token` port, so the
/// inverse falls back to the tool's own held credential, the same
/// default a bare `read`/`ensure` call uses): `org` and `slug` alone --
/// the two key ports `BuildkitePipelineInverse::org_of`/`slug_of` read.
fn created_inputs(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("org").unwrap(),
        Value::known(org.clone()),
    );
    inputs.insert(
        PortName::parse("slug").unwrap(),
        Value::known(slug.clone()),
    );
    inputs
}

/// A [`Created`] over `inputs`/`outputs`, `window`/`settled_by`, and
/// `accept` -- `buildkite.pipeline.ensure` declares no `Within`, so
/// `riders` is always empty (decision 7's buildkite row: "`created.riders`
/// is never read here").
fn created<'a>(
    inputs: &'a Inputs,
    outputs: &'a Outputs,
    window: CreationWindow,
    settled_by: Timestamp,
    accept: &'a [Loss],
) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window,
        settled_by,
        accept,
        riders: &[],
    }
}

/// `buildkite.pipeline.ensure`'s inverse, over `tool`.
fn inverse(tool: &BuildkitePipelineEnsure) -> &dyn Inverse {
    Tool::inverse(tool).expect("buildkite.pipeline.ensure declares Delete, so inverse() is Some")
}

/// Deletes `-a`, `-b` and `-c` on every exit path -- a panic, a failed
/// assertion, or an early return -- unless the test body already deleted
/// all three and disarmed the guard. Armed from before step 1's own
/// first write, so a pipeline that was in fact created despite a later
/// failure is never left behind; a `404` on any of their `DELETE` is
/// treated exactly like success (either already deleted through the
/// inverse, by hand, or never created at all). Mirrors
/// `willikins-providers-github`'s own `tests/live_rollback_cycle.rs`
/// `DeleteGuard`.
struct DeleteGuard {
    client: Arc<BuildkiteClient>,
    org: BuildkiteOrg,
    slugs: [BuildkitePipelineSlug; 3],
    armed: bool,
}

impl DeleteGuard {
    fn new(client: Arc<BuildkiteClient>, org: BuildkiteOrg, slugs: [BuildkitePipelineSlug; 3]) -> Self {
        Self {
            client,
            org,
            slugs,
            armed: false,
        }
    }

    fn arm(&mut self) {
        self.armed = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DeleteGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let already_panicking = std::thread::panicking();
        let mut leftover = false;
        for slug in &self.slugs {
            println!("guard: deleting `{}/{slug}`", self.org);
            match self.client.delete_pipeline(&self.org, slug) {
                Ok(()) => println!("guard: deleted `{}/{slug}`", self.org),
                Err(err) if err.status == Some(404) => {
                    println!("guard: `{}/{slug}` was already gone (404)", self.org);
                }
                Err(err) => {
                    leftover = true;
                    println!(
                        "guard: !!! LEFTOVER PIPELINE `{}/{slug}` !!! the guard's DELETE failed \
                         (status {:?}); it must be deleted by hand",
                        self.org, err.status
                    );
                }
            }
        }
        assert!(
            !leftover || already_panicking,
            "the guard could not delete every pipeline; see the LEFTOVER lines above -- they \
             must be deleted by hand"
        );
    }
}

/// Everything one run of this cycle shares.
struct Cycle {
    raw: Arc<Http>,
    client: Arc<BuildkiteClient>,
    pipeline_ensure: BuildkitePipelineEnsure,
    sweep: Vec<String>,
}

impl Cycle {
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    fn note(&mut self, text: String) {
        self.sweep.push(text);
    }
}

/// Step 1: list the org's pipelines, refuse to start if a
/// `willikins-probe-delete-me-` leftover from an aborted run already
/// exists.
fn step_1_count_and_refuse_leftovers(raw: &Http, org: &BuildkiteOrg) -> usize {
    let names = list_pipeline_slugs(raw, org);
    let leftovers = names
        .iter()
        .filter(|name| name.starts_with(PROBE_PREFIX))
        .count();
    assert_eq!(
        leftovers, 0,
        "step 1: {leftovers} leftover `{PROBE_PREFIX}*` pipeline(s) from an aborted run; delete \
         them by hand before running this cycle"
    );
    println!(
        "step 1 (no `{PROBE_PREFIX}*` leftover; `{org}` holds {} pipelines): pass",
        names.len()
    );
    names.len()
}

/// Sub-cycle `-a`: clean rollback. Creates the pipeline, confirms verify
/// item 8's own `created_at` field, confirms `standing` reports
/// `AsCreated` with no losing names, deletes through the inverse, and
/// confirms a raw `GET` afterwards is `404`.
fn step_2_clean_rollback(cycle: &mut Cycle, org: &BuildkiteOrg, slug: &BuildkitePipelineSlug, cluster: Value) {
    let before = Timestamp::now();
    let ensured = cycle
        .pipeline_ensure
        .ensure(&ensure_inputs(org, slug, cluster), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 2: creating `-a` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    let after = Timestamp::now();
    assert!(ensured.changed, "step 2: creating `-a` must report changed: true");

    let raw_body = get_pipeline_raw(&cycle.raw, org, slug);
    let form = assert_created_at_present_and_parseable(&raw_body);
    cycle.say(&format!(
        "step 2 (verify item 8: `-a`'s raw GET carries a non-null `created_at`, parsed as \
         {form}): pass"
    ));

    let window = creation_window(before, after);
    let settled_by = settled_by_from(after);
    let inputs = created_inputs(org, slug);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.pipeline_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by, &[]))
        .unwrap_or_else(|err| panic!("step 2: standing on `-a` failed: {}", err.message));
    cycle.note(format!("step 2 standing: {standing:?}"));
    assert_eq!(
        standing,
        Standing::AsCreated { losing: Vec::new() },
        "step 2: a freshly created, build-free pipeline must report AsCreated with no losing \
         names"
    );

    let token = teardown_token();
    let deleted = inverse(&cycle.pipeline_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by, &[]), &token)
        .unwrap_or_else(|err| panic!("step 2: delete of `-a` failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "step 2: deleting a freshly created pipeline must not report already_gone"
    );

    match cycle.raw.get::<Json>(&format!("/v2/organizations/{org}/pipelines/{slug}")) {
        Err(err) if err.status == Some(404) => {}
        Ok(_) => panic!("step 2: `-a` still exists after the inverse's own delete"),
        Err(err) => panic!(
            "step 2: the GET after the inverse's own delete answered an unexpected status {:?}",
            err.status
        ),
    }
    cycle.say(
        "step 2 (clean rollback: standing is AsCreated with no losing names, delete reports \
         already_gone: false, a raw GET afterwards is 404): pass",
    );
}

/// Shared by `-b` and `-c`: create the pipeline and trigger one build on
/// it, returning the `window`/`settled_by` pair `standing`/`delete` need.
fn create_and_build(
    cycle: &mut Cycle,
    org: &BuildkiteOrg,
    slug: &BuildkitePipelineSlug,
    cluster: Value,
    tag: &str,
) -> (CreationWindow, Timestamp) {
    let before = Timestamp::now();
    let ensured = cycle
        .pipeline_ensure
        .ensure(&ensure_inputs(org, slug, cluster), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "sub-cycle `{tag}`: creating the pipeline failed ({:?}): {}",
                err.kind, err.message
            )
        });
    let after = Timestamp::now();
    assert!(
        ensured.changed,
        "sub-cycle `{tag}`: creating the pipeline must report changed: true"
    );

    trigger_a_build(&cycle.raw, org, slug);
    let has_build = has_any_build_raw(&cycle.raw, org, slug);
    assert!(
        has_build,
        "verify item 8: `GET .../builds?per_page=1` must list the build this sub-cycle just \
         triggered"
    );
    cycle.say(&format!(
        "sub-cycle `{tag}` (verify item 8: the builds-listing endpoint now lists one build): pass"
    ));

    (creation_window(before, after), settled_by_from(after))
}

/// Sub-cycle `-b`: an unaccepted build loss. `standing` reports
/// `AsCreated { losing: ["builds"] }`; `delete`, with no accepted loss,
/// refuses as a `Conflict`. Deleted by hand afterwards.
fn step_3_unaccepted_build_loss(
    cycle: &mut Cycle,
    org: &BuildkiteOrg,
    slug: &BuildkitePipelineSlug,
    cluster: Value,
) {
    let (window, settled_by) = create_and_build(cycle, org, slug, cluster, "-b");

    let inputs = created_inputs(org, slug);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.pipeline_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by, &[]))
        .unwrap_or_else(|err| panic!("step 3: standing on `-b` failed: {}", err.message));
    cycle.note(format!("step 3 standing: {standing:?}"));
    assert_eq!(
        standing,
        Standing::AsCreated {
            losing: vec!["builds"]
        },
        "step 3: a pipeline with one build must report AsCreated {{ losing: [\"builds\"] }}, \
         never Drifted"
    );
    cycle.say(
        "step 3 (decision 3a: a pipeline with a build reports AsCreated {{ losing: [\"builds\"] \
         }}): pass",
    );

    let token = teardown_token();
    let err = inverse(&cycle.pipeline_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by, &[]), &token)
        .expect_err("step 3: delete on a pipeline with an unaccepted build loss must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 3: the refusal must be a Conflict"
    );
    cycle.say("step 3 (delete refuses the unaccepted build loss as a Conflict): pass");
}

/// Sub-cycle `-c`: the same build loss, accepted. `standing` again
/// reports `AsCreated { losing: ["builds"] }`, but `Created::accept`
/// carries `Loss::parse("builds")` this time, so `delete` succeeds
/// despite the build.
fn step_4_accepted_build_loss(
    cycle: &mut Cycle,
    org: &BuildkiteOrg,
    slug: &BuildkitePipelineSlug,
    cluster: Value,
) {
    let (window, settled_by) = create_and_build(cycle, org, slug, cluster, "-c");

    let accept = [Loss::parse("builds").expect("\"builds\" is a valid Loss name")];
    let inputs = created_inputs(org, slug);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.pipeline_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by, &accept))
        .unwrap_or_else(|err| panic!("step 4: standing on `-c` failed: {}", err.message));
    cycle.note(format!("step 4 standing: {standing:?}"));
    assert_eq!(
        standing,
        Standing::AsCreated {
            losing: vec!["builds"]
        },
        "step 4: `standing` reports the same losing names regardless of `accept` -- only \
         `delete` consults it"
    );

    let token = teardown_token();
    let deleted = inverse(&cycle.pipeline_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by, &accept), &token)
        .unwrap_or_else(|err| {
            panic!(
                "step 4: delete of `-c` must succeed once the document accepts the \"builds\" \
                 loss: {}",
                err.message
            )
        });
    assert!(
        !deleted.already_gone,
        "step 4: deleting a pipeline that still has its build must not report already_gone"
    );

    match cycle.raw.get::<Json>(&format!("/v2/organizations/{org}/pipelines/{slug}")) {
        Err(err) if err.status == Some(404) => {}
        Ok(_) => panic!("step 4: `-c` still exists after the inverse's own delete"),
        Err(err) => panic!(
            "step 4: the GET after the inverse's own delete answered an unexpected status {:?}",
            err.status
        ),
    }
    cycle.say(
        "step 4 (decision 3a: delete succeeds once the document accepts the \"builds\" loss; a \
         raw GET afterwards is 404): pass",
    );
}

/// Step 5: `-b` (refused by its own inverse) is deleted by hand --
/// verify item 8's own "DELETE answers 204" (an `Ok(())` from this
/// client method, the same reading `tests/live_rollback_cycle.rs`'s
/// sibling crates give their own hand deletes); it re-reads `404`. `-a`
/// and `-c` are already gone (their own inverse deletes). The
/// organisation's pipeline count returns to step 1's; the guard is
/// disarmed.
fn step_5_manual_cleanup_and_recount(
    cycle: &mut Cycle,
    guard: &mut DeleteGuard,
    org: &BuildkiteOrg,
    slug_b: &BuildkitePipelineSlug,
    before_count: usize,
) {
    cycle
        .client
        .delete_pipeline(org, slug_b)
        .unwrap_or_else(|err| {
            panic!(
                "step 5: the hand DELETE of `-b` failed (status {:?})",
                err.status
            )
        });
    match cycle.raw.get::<Json>(&format!("/v2/organizations/{org}/pipelines/{slug_b}")) {
        Err(err) if err.status == Some(404) => {}
        Ok(_) => panic!("step 5: `-b` still exists after its hand DELETE"),
        Err(err) => panic!(
            "step 5: the GET after `-b`'s hand DELETE answered an unexpected status {:?}",
            err.status
        ),
    }
    guard.disarm();

    let after_count = list_pipeline_slugs(&cycle.raw, org).len();
    assert_eq!(
        after_count, before_count,
        "step 5: the org's pipeline count must return to what step 1 saw"
    );
    cycle.say(
        "step 5 (`-b` deleted by hand, re-read 404 -- verify item 8's DELETE 204; pipeline count \
         restored; guard disarmed): pass",
    );
}

/// The final redaction sweep: nothing this file kept for it may contain
/// anything credential-shaped.
fn sweep_for_secrets(cycle: &Cycle) {
    for (index, text) in cycle.sweep.iter().enumerate() {
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "produced string {index} looks credential-shaped (`{prefix}`): refusing to let \
                 it stand"
            );
        }
    }
    println!("redaction sweep over {} strings: pass", cycle.sweep.len());
}

#[test]
#[ignore = "opt-in live rollback cycle against a real Buildkite org; creates, drifts (an \
            unaccepted build, then an accepted one), and deletes three pipelines plus two \
            throwaway builds. Run with WILLIKINS_LIVE_TESTS=1, the `live-tests` feature, and \
            sandbox credentials sourced in the same command: `source \
            ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
            cargo test -p willikins-providers-buildkite --features live-tests --test \
            live_rollback_cycle -j 2 -- --ignored --nocapture`"]
fn buildkite_live_rollback_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox Buildkite token");
    let org = sandbox_org();
    let raw = Arc::new(http_client(credential.clone()));
    let client = Arc::new(BuildkiteClient::new(http_client(credential)));

    let before_count = step_1_count_and_refuse_leftovers(&raw, &org);

    let timestamp = unix_seconds();
    let slug_a = build_slug(timestamp, "a");
    let slug_b = build_slug(timestamp, "b");
    let slug_c = build_slug(timestamp, "c");

    let mut guard = DeleteGuard::new(
        Arc::clone(&client),
        org.clone(),
        [slug_a.clone(), slug_b.clone(), slug_c.clone()],
    );
    guard.arm();
    println!("the delete guard is armed before any write");

    let cluster_tool = BuildkiteClusterGet::new(Arc::clone(&client));
    let mut cycle = Cycle {
        raw: Arc::clone(&raw),
        client: Arc::clone(&client),
        pipeline_ensure: BuildkitePipelineEnsure::new(Arc::clone(&client)),
        sweep: Vec::new(),
    };

    let cluster_a = resolve_default_cluster(&cluster_tool, &org);
    step_2_clean_rollback(&mut cycle, &org, &slug_a, cluster_a);

    let cluster_b = resolve_default_cluster(&cluster_tool, &org);
    step_3_unaccepted_build_loss(&mut cycle, &org, &slug_b, cluster_b);

    let cluster_c = resolve_default_cluster(&cluster_tool, &org);
    step_4_accepted_build_loss(&mut cycle, &org, &slug_c, cluster_c);

    step_5_manual_cleanup_and_recount(&mut cycle, &mut guard, &org, &slug_b, before_count);

    sweep_for_secrets(&cycle);
}

/// Pins this file's own slug construction against
/// `BuildkitePipelineSlug`'s grammar, without any network call.
#[test]
fn probe_prefix_and_slug_produce_a_valid_pipeline_slug() {
    for tag in ["a", "b", "c"] {
        let slug = build_slug(unix_seconds(), tag);
        assert!(slug.as_str().starts_with(PROBE_PREFIX));
    }
}

/// Pins this file's own `shift`/`creation_window`/`settled_by_from`
/// arithmetic against `CREATION_SKEW`, without any network call --
/// mirrors `willikins-providers-github`'s own test of the same name.
#[test]
fn creation_window_and_settled_by_apply_the_skew_on_the_right_side() {
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
    assert_eq!(settled_by_from(after), window.end);
}

/// Pins [`local_parse_pipeline_created_at`] against both forms, without
/// any network call -- the same two cases `client.rs`'s own
/// `parse_pipeline_created_at` tests pin, plus the "which form matched"
/// tag this file's own copy adds.
#[test]
fn local_parse_pipeline_created_at_tries_rfc3339_then_buildkites_documented_form() {
    let (rfc3339, form) = local_parse_pipeline_created_at("2026-01-01T00:00:00Z")
        .expect("RFC 3339 must parse");
    assert_eq!(rfc3339, Timestamp::parse("2026-01-01T00:00:00Z").unwrap());
    assert_eq!(form, "rfc3339");

    let (documented, form) = local_parse_pipeline_created_at("2013-09-03 13:24:38 UTC")
        .expect("Buildkite's documented form must parse");
    assert_eq!(documented, Timestamp::parse("2013-09-03T13:24:38Z").unwrap());
    assert_eq!(form, "buildkite documented form");

    assert_eq!(local_parse_pipeline_created_at("not a timestamp"), None);
}

/// A sleep helper this file's own doc comments reference but that no test
/// here actually calls -- `Duration` stays imported for a future reader
/// who wants to add a window-edge sub-cycle (`tests/it/pipeline_inverse_mock.rs`
/// already proves that arm on mocks); kept as a compile-time pin that the
/// import is still exactly `std::time::Duration`, not a renamed re-export.
#[test]
fn duration_import_is_std_time_duration() {
    let _: Duration = Duration::from_secs(1);
}
