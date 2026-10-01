//! Milestone 3e task 3, acceptance test 8 (rewritten for gates, per the
//! plan's T3 row): `workflows/walter-ios-app.yaml` over the fake catalogue.
//!
//! `appstore.profile.ensure` is `Class::Destructive` since operator
//! decision 1, so every run of this document needs approval
//! (`Approval::Human`) -- the class is static, from the graph, unaffected
//! by which gates are open (the milestone plan's decision (j), point 5).
//!
//! Three applied runs plus one plan-only interleaved check, one shared
//! `FakeState`, exactly as the plan's task description asks:
//!
//! 1. **Gates unmet.** Nothing is seeded beyond the credential chain, the
//!    monorepo, the Buildkite cluster, and the distribution certificate.
//!    Every independent node (the three bundle identifiers, the three
//!    capabilities, Doppler, the Buildkite pipeline, the monorepo
//!    reference) plans and applies for real. `app_record` (a leaf), the
//!    three `app_*_app_groups` gates, and `app_app_attest` (the App
//!    Attest gate task's own addition, host identifier only) are
//!    `Blocked` -- the identifiers they check do not exist yet, so their
//!    own `read` (which resolves the parent through `list_bundle_ids`)
//!    finds nothing. The three profile nodes are `Skip`, never read.
//!    The two `operator.acknowledge` leaves (M5, M6) are `Blocked` (no
//!    `done` supplied), and so is `bootstrap_gate` (M7, milestone 3g):
//!    the pipeline's stored configuration is still the frozen upload
//!    bootstrap. `walter_files` lands the scaffold.
//!    - **Plan-only, between run 1 and run 2: App Groups is on, App
//!      Attest is still off.** The fake state gains the app record and
//!      `APP_GROUPS` on all three identifiers, but not yet `APP_ATTEST`
//!      on the host. A fresh `plan` (not applied) shows `app_app_attest`
//!      still `Blocked`, holding back exactly `app_profile` -- while
//!      `nse_profile`/`widgets_profile`, gated only by their own (now
//!      open) app-group gate, plan `Create`. This is the App Attest gate's own
//!      acceptance case: the host profile alone is held back, the
//!      extensions are unaffected.
//! 2. **The fake state satisfies every observed gate** (App Attest is now
//!    seeded on the host identifier too). Same inputs, same
//!    acknowledgements withheld. A fresh `plan` shows every
//!    previously-blocked gate `Compute` and the three profile nodes
//!    `Create` -- **and nothing else
//!    changes**: every node that already ran in step 1 reads
//!    `Unchanged`/`Computed`. The two acknowledgement leaves and
//!    `bootstrap_gate` are still `Blocked`.
//! 3. **The two acknowledgements are supplied** and the operator's paste
//!    is stood in for (the pipeline's stored configuration becomes the
//!    rendered bootstrap). A third run, same fake state, `done` on both
//!    `*_done` inputs: nothing is `Created`, nothing is blocked -- a
//!    converged run end to end.
//!
//! Throughout: `check` succeeds (the type system's own proof that no
//! secret reaches a non-secret port -- `willikins-design`'s "no bypass"
//! invariant), and neither the plan JSON nor the applied JSON, across all
//! three runs, ever carries the signing key's PEM marker or the fake
//! profile tool's own plaintext content prefix -- only `[REDACTED` markers
//! for the secret-typed ports (`key`, `content`).

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Action, Applied, Approval, InputName, NodeStatus, PortName, PrincipalId, RecordingObserver,
    Timestamp, TypeName, TypeRef, Value, apply, check, plan,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::buildkite_pipeline_key;
use willikins_types::{
    BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubRepo, RepoFile,
    RepoVisibility,
};

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> willikins_core::Workflow {
    let path = workspace_root().join("workflows/walter-ios-app.yaml");
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("the document loads: {err}"))
}

fn scalar(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

fn list(type_name: &str, items: &[&str]) -> Value {
    Value::parse_list(
        &TypeRef::list_of(TypeName::parse(type_name).unwrap()),
        items,
    )
    .unwrap()
}

const APP_IDENTIFIER: &str = "com.example.walter";
const NSE_IDENTIFIER: &str = "com.example.walter.nse";
const WIDGETS_IDENTIFIER: &str = "com.example.walter.widgets";
const MONOREPO: &str = "Bande-a-Bonnot/monorepo";
// D2: the document's own literal cluster name (`buildkite_cluster.name`),
// the real org's only cluster, probed read-only 2026-09-29.
const CLUSTER: &str = "Default cluster";
// D2: the document's own literal Buildkite org slug.
const BUILDKITE_ORG: &str = "la-bande-a-bonnot";
/// A valid [`willikins_types::BuildkiteToken`], `concat!`-assembled so no
/// single literal in this file spells a real-shaped Buildkite token
/// contiguously (the same technique
/// `crates/willikins-providers-buildkite/tests/buildkite_token_documents.rs`'s
/// own `SEEDED_TOKEN` uses).
const SEEDED_BUILDKITE_TOKEN: &str = concat!("bkua_", "wlknFixtureTokenNotARealCredential00");

/// Every input the document declares, with a fixed value -- `plan` never
/// backfills a default itself (`Binding::Input` fails `MissingInput` on an
/// absent, non-`OperatorAcknowledgement` input; `crates/willikins-dsl/tests/acceptance.rs`'s
/// own `synthesized_inputs` makes exactly this same choice for the
/// characterization suite). The two acknowledgement inputs are the one
/// exception the caller may leave out (they resolve `Value::unknown`).
fn base_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("app_identifier").unwrap(),
        scalar("AppleBundleIdentifier", APP_IDENTIFIER),
    );
    inputs.insert(
        InputName::parse("nse_identifier").unwrap(),
        scalar("AppleBundleIdentifier", NSE_IDENTIFIER),
    );
    inputs.insert(
        InputName::parse("widgets_identifier").unwrap(),
        scalar("AppleBundleIdentifier", WIDGETS_IDENTIFIER),
    );
    // W1: `data_protection` is gone as a declared input -- the document's
    // `data_protection` node now binds a bare literal instead. See
    // `walter_entitlements_and_data_protection_name_the_same_class` below.
    inputs.insert(
        InputName::parse("certificate_type").unwrap(),
        scalar("AppleCertificateType", "DISTRIBUTION"),
    );
    inputs.insert(
        InputName::parse("serial_number").unwrap(),
        scalar("AppleCertificateSerial", "7B3F2A9C1D4E5F607182930A1B2C3D4E"),
    );
    inputs.insert(
        InputName::parse("environments").unwrap(),
        list("EnvironmentSlug", &["dev", "stg", "prd"]),
    );
    inputs.insert(
        InputName::parse("base_configs").unwrap(),
        list(
            "DopplerConfig",
            &[
                "appstore-connect/deploy_ios",
                "github/bande-a-bonnot",
                "open-telemetry/prd_signoz",
            ],
        ),
    );
    inputs
}

fn with_acknowledgements(mut inputs: IndexMap<InputName, Value>) -> IndexMap<InputName, Value> {
    // W1: M3 and M7 are gone as acknowledgements -- only M5 and M6 are
    // still bare `operator.acknowledge` leaves.
    for name in ["m5_apns_key_done", "m6_ci_doppler_access_done"] {
        inputs.insert(
            InputName::parse(name).unwrap(),
            Value::known(willikins_types::OperatorAcknowledgement::parse("done").unwrap()),
        );
    }
    inputs
}

/// Everything the document needs *before* the two observed gates can ever
/// open: the credential chain's three Doppler values (App Store Connect,
/// read from the real, fixed base config `appstore-connect/deploy_ios`,
/// not a caller-supplied input -- R4), the GitHub token (from
/// `github/bande-a-bonnot`, R4's own new resolver chain, mirroring R2's
/// `workflows/github-repo-token-from-doppler.yaml`), the Buildkite token
/// (from `buildkite/prd`, D2's own resolver chain, mirroring K1's
/// `workflows/buildkite-cluster-token-from-doppler.yaml`), the monorepo
/// (for `github.repo.get`), the Buildkite cluster (for
/// `buildkite.cluster.get`), and the distribution certificate (for
/// `appstore.certificate.get`). No bundle id, no app record, no
/// capability is seeded -- those are exactly what run 1 must create or
/// find blocked.
/// D1: the three base configs `base_config_gate` checks, ahead of
/// `inherit` (the document's own `base_configs` default, in declared
/// order).
const BASE_CONFIGS: [&str; 3] = [
    "appstore-connect/deploy_ios",
    "github/bande-a-bonnot",
    "open-telemetry/prd_signoz",
];

fn seeded_state() -> Arc<Mutex<FakeState>> {
    seeded_state_with_base_configs(&BASE_CONFIGS)
}

/// Like [`seeded_state`], but only `present` of the three base configs
/// are seeded as existing and marked inheritable -- D1's own negative
/// case, where `base_config_gate` finds one of them missing.
fn seeded_state_with_base_configs(present: &[&str]) -> Arc<Mutex<FakeState>> {
    let config = willikins_types::DopplerConfig::parse("appstore-connect/deploy_ios").unwrap();
    let github_config = willikins_types::DopplerConfig::parse("github/bande-a-bonnot").unwrap();
    // W1: the new, narrower write credential -- a Doppler config no app
    // config inherits (decision (l)), never the older, wider
    // `GH_CLONE_TOKEN` above.
    let write_config =
        willikins_types::DopplerConfig::parse("github/bande-a-bonnot_willikins").unwrap();
    let json = serde_json::json!({
        "doppler_values": {
            format!("{config}#APP_STORE_CONNECT_API_KEY_ISSUER_ID"): "57246542-96fe-1a63-e053-0824d011072a",
            format!("{config}#APP_STORE_CONNECT_API_KEY_ID"): "2X9R4HXF34",
        },
        "doppler_secrets": {
            format!("{config}#APP_STORE_CONNECT_API_KEY_BASE64"): "VGhpcyBpcyBhbiBleGFtcGxlIGtleSBmb3IgdGVzdHMgb25seS4KLS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1JR0hBZ0VBTUJNR0J5cUdTTTQ5QWdFR0NDcUdTTTQ5QXdFSEJHMHdhd0lCQVFRZ3ZMNTJyZWtFcWdHcW9XbjkKK1lCa0lRdVFXRU9UaEtxcUlYYnZvbmVuY0FXaFJBTkNBQVRkdC9YZDRjL0NMT0thMmpvRDlHMXBCOTh1d0tOKwpMR0p2SzNoS1RyeFRXbkowR3lRaVAzUm1DdWJ6bCtHUVIvL2g5Y2lGYW1qeU5jSE1qVlUyY0tiQQotLS0tLUVORCBQUklWQVRFIEtFWS0tLS0tCg==",
            format!("{github_config}#GH_CLONE_TOKEN"): "ghp_example",
            format!("{write_config}#GH_CONTENTS_WRITE_TOKEN"): "ghp_write_example",
            "buildkite/prd#PIPELINE_CREATION_TOKEN": SEEDED_BUILDKITE_TOKEN,
        },
    })
    .to_string();
    let mut state = FakeState::from_json(&json).unwrap_or_else(|err| panic!("seeded state: {err}"));
    state = state
        .with_repo(
            &GitHubRepo::parse(MONOREPO).unwrap(),
            RepoVisibility::Private,
            false,
        )
        .with_buildkite_cluster(
            &BuildkiteClusterName::parse(CLUSTER).unwrap(),
            "018e5a22-d14c-7085-bb28-db0f83f43a1c",
        )
        .with_apple_certificate(
            &willikins_types::AppleCertificateType::parse("DISTRIBUTION").unwrap(),
            &willikins_types::AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E")
                .unwrap(),
            "C3RT1F1CATE1",
            false,
            Some(true),
        );
    for name in present {
        let base_config = willikins_types::DopplerConfig::parse(name)
            .unwrap_or_else(|err| panic!("`{name}` parses as a DopplerConfig: {err}"));
        state = state
            .with_doppler_config(&base_config)
            .with_doppler_config_inheritable(&base_config);
    }
    Arc::new(Mutex::new(state))
}

fn status_of<'a>(
    applied: &'a willikins_core::Applied,
    node: &str,
    instance: Option<&str>,
) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was applied"))
        .status
}

fn action_of(planned: &willikins_core::Plan, node: &str, instance: Option<&str>) -> Action {
    planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was planned"))
        .action
}

/// An applied node's own outputs, by name -- W1's way of extracting what a
/// `repo.file.render` node actually produced, or what a pipeline's own
/// `slug` output resolved to, without re-typing either by hand and
/// risking drift from the document.
fn output_of<'a>(applied: &'a Applied, node: &str) -> &'a willikins_core::Outputs {
    &applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.is_none())
        .unwrap_or_else(|| panic!("node `{node}` was applied"))
        .outputs
}

/// The [`RepoFile`] a `repo.file.render` node (`node`) produced on its
/// `file` output.
fn rendered_file<'a>(applied: &'a Applied, node: &str) -> &'a RepoFile {
    output_of(applied, node)
        .get(&PortName::parse("file").unwrap())
        .unwrap_or_else(|| panic!("`{node}.file` was applied"))
        .downcast::<RepoFile>()
        .unwrap_or_else(|| panic!("`{node}.file` is a RepoFile"))
}

fn approval() -> Approval {
    Approval::Human {
        approver: PrincipalId::parse("operator").unwrap(),
        at: Timestamp::parse("2026-09-29T00:00:00Z").unwrap(),
    }
}

/// No secret value ever reaches JSON output: only its `[REDACTED` marker
/// does. The signing key's own PEM marker, the fake profile tool's own
/// plaintext content prefix, and R4/D2's seeded GitHub/Buildkite tokens
/// are the concrete secrets this graph carries.
fn assert_no_secret_leaked(json: &str) {
    assert!(
        !json.contains("PRIVATE KEY"),
        "the signing key's PEM marker leaked into JSON output"
    );
    assert!(
        !json.contains("ghp_example"),
        "the seeded GitHub token's raw value leaked into JSON output"
    );
    assert!(
        !json.contains("ghp_write_example"),
        "the seeded write token's raw value leaked into JSON output"
    );
    assert!(
        !json.contains(SEEDED_BUILDKITE_TOKEN),
        "the seeded Buildkite token's raw value leaked into JSON output"
    );
    assert!(
        !json.contains("fakeprofilecontent"),
        "a fake profile's plaintext content leaked into JSON output"
    );
    assert!(
        json.contains("[REDACTED"),
        "expected at least one redacted secret port (key or content) in the output"
    );
}

#[test]
// One long, linear three-run scenario, kept as a single test rather than
// split across helpers: each run's assertions read against the run
// immediately above them, and splitting would hide that ordering behind
// indirection for no real gain.
#[allow(clippy::too_many_lines)]
fn gates_unmet_then_satisfied_then_acknowledged() {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());

    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));

    // ------------------------------------------------------------------
    // Run 1: gates unmet.
    // ------------------------------------------------------------------
    let inputs = base_inputs();
    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));

    // Exactly the App Store Connect gates, the new bootstrap gate (W1: the
    // pipeline does not exist yet, so its stored configuration cannot
    // equal anything), plus the two remaining acknowledgement leaves are
    // blocked; nothing else. M3 is gone entirely (it is real work now,
    // below), and M7 is `bootstrap_gate`, not an acknowledgement.
    let blocked_nodes: std::collections::BTreeSet<&str> =
        planned1.blocked.iter().map(|b| b.node.as_str()).collect();
    assert_eq!(
        blocked_nodes,
        std::collections::BTreeSet::from([
            "app_record",
            "app_app_groups",
            "nse_app_groups",
            "widgets_app_groups",
            "app_app_attest",
            "bootstrap_gate",
            "m5_apns_key",
            "m6_ci_doppler_access",
        ]),
        "run 1's blocked set"
    );

    // The three profiles are skipped, never planned as Create -- they are
    // held back by their own app-group gate.
    for node in ["app_profile", "nse_profile", "widgets_profile"] {
        assert_eq!(
            action_of(&planned1, node, None),
            Action::Skip,
            "run 1: `{node}` must be Skip"
        );
    }

    // Every independent node still plans for real.
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert_eq!(
            action_of(&planned1, node, None),
            Action::Create,
            "run 1: `{node}` must be Create"
        );
    }
    assert_eq!(action_of(&planned1, "monorepo_ref", None), Action::Compute);
    assert_eq!(action_of(&planned1, "doppler", None), Action::Create);
    // W1: the scaffold and every render node it depends on plan for real
    // on a first run, and the pipeline -- now rebound to `walter_files.repo`
    // rather than `monorepo_ref.repo` -- still plans, ordered after it.
    for node in ["build_bazel_app", "build_bazel_ios", "walter_entitlements"] {
        assert_eq!(
            action_of(&planned1, node, None),
            Action::Compute,
            "run 1: pure render node `{node}` must be Compute"
        );
    }
    assert_eq!(action_of(&planned1, "walter_files", None), Action::Create);
    assert_eq!(action_of(&planned1, "pipeline", None), Action::Create);

    let plan1_json = serde_json::to_string(&planned1).unwrap();
    assert_no_secret_leaked(&plan1_json);

    let mut observer1 = RecordingObserver::new();
    let applied1 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned1,
        &approval(),
        &mut observer1,
    )
    .expect("run 1's blocked run is Ok, not an error");

    assert!(matches!(
        status_of(&applied1, "app_record", None),
        NodeStatus::Blocked
    ));
    assert!(matches!(
        status_of(&applied1, "app_app_groups", None),
        NodeStatus::Blocked
    ));
    assert!(matches!(
        status_of(&applied1, "app_app_attest", None),
        NodeStatus::Blocked
    ));
    for node in ["app_profile", "nse_profile", "widgets_profile"] {
        assert!(
            matches!(status_of(&applied1, node, None), NodeStatus::Skipped),
            "run 1: `{node}` must be Skipped"
        );
    }
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert!(
            matches!(status_of(&applied1, node, None), NodeStatus::Created),
            "run 1: `{node}` must be Created"
        );
    }
    assert_eq!(applied1.blocked.len(), 8, "run 1's Applied.blocked");

    let applied1_json = serde_json::to_string(&applied1).unwrap();
    assert_no_secret_leaked(&applied1_json);

    // The identifiers are genuinely registered now: neither app-group gate
    // nor the app-record gate found anything to read at plan time, but
    // apply really created them.
    {
        let locked = state.lock().unwrap();
        assert!(locked.apple_bundle_ids.contains_key(APP_IDENTIFIER));
        assert!(locked.apple_bundle_ids.contains_key(NSE_IDENTIFIER));
        assert!(locked.apple_bundle_ids.contains_key(WIDGETS_IDENTIFIER));
        assert!(!locked.apple_apps.contains(APP_IDENTIFIER));
    }

    // ------------------------------------------------------------------
    // Between run 1 and run 2: the operator's own manual work, standing
    // in for the App Store Connect portal -- the app record now exists,
    // and APP_GROUPS is enabled on all three identifiers.
    // ------------------------------------------------------------------
    {
        let mut locked = state.lock().unwrap();
        locked.apple_apps.insert(APP_IDENTIFIER.to_string());
        for identifier in [APP_IDENTIFIER, NSE_IDENTIFIER, WIDGETS_IDENTIFIER] {
            locked
                .apple_bundle_id_capabilities
                .entry(identifier.to_string())
                .or_default()
                .insert("APP_GROUPS".to_string());
        }
    }

    // ------------------------------------------------------------------
    // Interleaved plan: APP_GROUPS is on everywhere, but App Attest is
    // still off on the host identifier -- the App Attest gate task's own
    // acceptance case. `app_app_attest` alone must still be Blocked, and
    // it must hold back exactly the host app's profile; the NSE and
    // widgets profiles have no App Attest gate
    // at all, so they proceed. Plan only, not applied -- the next block
    // seeds App Attest and run 2 below is what actually applies this
    // state.
    // ------------------------------------------------------------------
    let planned_attest_off =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("interleaved plan: {err}"));
    assert_eq!(
        action_of(&planned_attest_off, "app_app_attest", None),
        Action::Blocked,
        "App Attest is still off on the host identifier: app_app_attest must be Blocked"
    );
    assert_eq!(
        action_of(&planned_attest_off, "app_profile", None),
        Action::Skip,
        "`app_profile` is held back by the still-unmet App Attest gate"
    );
    for node in ["nse_profile", "widgets_profile"] {
        assert_eq!(
            action_of(&planned_attest_off, node, None),
            Action::Create,
            "`{node}` has no App Attest gate and must proceed once its own app-group gate is open"
        );
    }
    let attest_blocked = planned_attest_off
        .blocked
        .iter()
        .find(|b| b.node.as_str() == "app_app_attest")
        .expect("app_app_attest is blocked");
    assert_eq!(
        attest_blocked.need, "the named capability enabled on this bundle identifier",
        "app_app_attest's own need text"
    );
    let attest_holds_back: std::collections::BTreeSet<&str> = attest_blocked
        .holds_back
        .iter()
        .map(willikins_core::NodeName::as_str)
        .collect();
    assert_eq!(
        attest_holds_back,
        std::collections::BTreeSet::from(["app_profile"]),
        "app_app_attest must hold back exactly the host profile"
    );
    // Adversarial pass 7: `need`/`how` are `&'static` and cannot name the
    // capability, so the rendered `subject` is the only place the operator
    // learns *which* capability is missing on *which* identifier.
    let attest_subject: Vec<(&str, &str)> = attest_blocked
        .subject
        .iter()
        .map(|(port, text)| (port.as_str(), text.as_str()))
        .collect();
    assert_eq!(
        attest_subject,
        vec![("identifier", APP_IDENTIFIER), ("capability", "APP_ATTEST")],
        "app_app_attest's blocked report must name the host identifier and APP_ATTEST"
    );
    // And nothing but App Attest, the bootstrap gate and the two
    // acknowledgements is still blocked: the three app-group gates and
    // the app record are open. W1: `bootstrap_gate` remains blocked here
    // too -- the pipeline's stored configuration is not touched by
    // seeding App Groups, and nothing has yet stood in for the
    // operator's own paste (that happens between run 2 and run 3, below).
    let blocked_attest_off: std::collections::BTreeSet<&str> = planned_attest_off
        .blocked
        .iter()
        .map(|b| b.node.as_str())
        .collect();
    assert_eq!(
        blocked_attest_off,
        std::collections::BTreeSet::from([
            "app_app_attest",
            "bootstrap_gate",
            "m5_apns_key",
            "m6_ci_doppler_access",
        ]),
        "with App Groups on and App Attest off, only App Attest, the bootstrap gate and the \
         acknowledgements block"
    );

    // ------------------------------------------------------------------
    // Between the interleaved plan and run 2: the operator finishes the
    // portal-side work -- App Attest is enabled on the host identifier.
    // ------------------------------------------------------------------
    {
        let mut locked = state.lock().unwrap();
        locked
            .apple_bundle_id_capabilities
            .entry(APP_IDENTIFIER.to_string())
            .or_default()
            .insert("APP_ATTEST".to_string());
    }

    // ------------------------------------------------------------------
    // Run 2: every App Store Connect gate opens; the bootstrap gate and
    // the two acknowledgements are still withheld.
    // ------------------------------------------------------------------
    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));

    let blocked_nodes2: std::collections::BTreeSet<&str> =
        planned2.blocked.iter().map(|b| b.node.as_str()).collect();
    assert_eq!(
        blocked_nodes2,
        std::collections::BTreeSet::from(["bootstrap_gate", "m5_apns_key", "m6_ci_doppler_access"]),
        "run 2's blocked set: the bootstrap gate and the two acknowledgement leaves remain"
    );

    for node in [
        "app_record",
        "app_app_groups",
        "nse_app_groups",
        "widgets_app_groups",
        "app_app_attest",
    ] {
        assert_eq!(
            action_of(&planned2, node, None),
            Action::Compute,
            "run 2: `{node}` must be Compute"
        );
    }
    for node in ["app_profile", "nse_profile", "widgets_profile"] {
        assert_eq!(
            action_of(&planned2, node, None),
            Action::Create,
            "run 2: `{node}` must be Create -- the app-group gate just opened"
        );
    }
    // Nothing that already ran in run 1 is re-created.
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert_eq!(
            action_of(&planned2, node, None),
            Action::NoOp,
            "run 2: `{node}` must read NoOp, already registered"
        );
    }
    assert_eq!(action_of(&planned2, "doppler", None), Action::NoOp);
    assert_eq!(action_of(&planned2, "pipeline", None), Action::NoOp);

    let mut observer2 = RecordingObserver::new();
    let applied2 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned2,
        &approval(),
        &mut observer2,
    )
    .expect("run 2 succeeds");

    for node in ["app_profile", "nse_profile", "widgets_profile"] {
        assert!(
            matches!(status_of(&applied2, node, None), NodeStatus::Created),
            "run 2: `{node}` must be Created"
        );
    }
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert!(
            matches!(status_of(&applied2, node, None), NodeStatus::Unchanged),
            "run 2: `{node}` must be Unchanged"
        );
    }
    assert_eq!(applied2.blocked.len(), 3, "run 2's Applied.blocked");

    // The universal claim, not a spot check: run 2's `Created` set is
    // EXACTLY the three nodes the app-group gates just unblocked -- nothing
    // else moved.
    let created2: std::collections::BTreeSet<&str> = applied2
        .nodes
        .iter()
        .filter(|n| matches!(n.status, NodeStatus::Created))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        created2,
        std::collections::BTreeSet::from(["app_profile", "nse_profile", "widgets_profile"]),
        "run 2 must create the three profiles, and nothing else"
    );

    // The ordering claim the whole design rests on (decision (j), point 2):
    // the host app-group gate holds back exactly its own profile, never
    // anything else. `planned2` has no
    // blocked entries any more (run 2 is past both observed gates), so this
    // is asserted against run 1's plan, where the gate really was blocked.
    let app_gate_blocked1 = planned1
        .blocked
        .iter()
        .find(|b| b.node.as_str() == "app_app_groups")
        .expect("run 1: app_app_groups is blocked");
    let holds_back: std::collections::BTreeSet<&str> = app_gate_blocked1
        .holds_back
        .iter()
        .map(willikins_core::NodeName::as_str)
        .collect();
    assert_eq!(
        holds_back,
        std::collections::BTreeSet::from(["app_profile"]),
        "app_app_groups must hold back exactly its own profile"
    );

    // Each acknowledgement gate names exactly its own input in
    // `awaiting_inputs` -- the data the CLI's `supply:` line renders from.
    // W1: only M5 and M6 are acknowledgements any more.
    for (node, input) in [
        ("m5_apns_key", "m5_apns_key_done"),
        ("m6_ci_doppler_access", "m6_ci_doppler_access_done"),
    ] {
        let entry = planned1
            .blocked
            .iter()
            .find(|b| b.node.as_str() == node)
            .unwrap_or_else(|| panic!("run 1: `{node}` is blocked"));
        assert_eq!(
            entry
                .awaiting_inputs
                .iter()
                .map(willikins_core::InputName::as_str)
                .collect::<Vec<_>>(),
            vec![input],
            "`{node}`'s awaiting_inputs"
        );
    }

    let applied2_json = serde_json::to_string(&applied2).unwrap();
    assert_no_secret_leaked(&applied2_json);

    // ------------------------------------------------------------------
    // Between run 2 and run 3: the operator's own paste, standing in --
    // the pipeline's stored `configuration` is set to exactly the
    // bootstrap this document rendered (`bootstrap_yml`'s own output,
    // extracted from `applied2` rather than re-typed here, so this test
    // cannot silently drift from what the document actually wrote).
    // ------------------------------------------------------------------
    let bootstrap_content = rendered_file(&applied2, "bootstrap_yml")
        .content()
        .to_string();
    let pipeline_slug = output_of(&applied2, "pipeline")
        .get(&PortName::parse("slug").unwrap())
        .expect("pipeline.slug was applied")
        .downcast::<BuildkitePipelineSlug>()
        .expect("pipeline.slug is a BuildkitePipelineSlug")
        .clone();
    {
        let mut locked = state.lock().unwrap();
        let key =
            buildkite_pipeline_key(&BuildkiteOrg::parse(BUILDKITE_ORG).unwrap(), &pipeline_slug);
        locked
            .buildkite_pipelines
            .get_mut(&key)
            .unwrap_or_else(|| panic!("pipeline `{key}` was seeded by run 1's apply"))
            .configuration = bootstrap_content;
    }

    // ------------------------------------------------------------------
    // Run 3: the two acknowledgements are supplied, and the bootstrap
    // gate now reads the paste above as equal. Everything converges.
    // ------------------------------------------------------------------
    let inputs3 = with_acknowledgements(inputs.clone());
    let planned3 =
        plan(&checked, &inputs3, &catalog).unwrap_or_else(|err| panic!("run 3 plans: {err}"));
    assert!(
        planned3.blocked.is_empty(),
        "run 3 must have no blocked gate left: {:?}",
        planned3.blocked
    );

    for node in [
        "app_id",
        "nse_id",
        "widgets_id",
        "app_record",
        "app_app_groups",
        "nse_app_groups",
        "widgets_app_groups",
        "app_app_attest",
        "app_profile",
        "nse_profile",
        "widgets_profile",
        "doppler",
        "walter_files",
        "pipeline",
        "bootstrap_gate",
        "m5_apns_key",
        "m6_ci_doppler_access",
    ] {
        assert_ne!(
            action_of(&planned3, node, None),
            Action::Blocked,
            "run 3: `{node}` must not be Blocked"
        );
        assert_ne!(
            action_of(&planned3, node, None),
            Action::Skip,
            "run 3: `{node}` must not be Skip"
        );
    }

    let mut observer3 = RecordingObserver::new();
    let applied3 = apply(
        &checked,
        &inputs3,
        &catalog,
        &planned3,
        &approval(),
        &mut observer3,
    )
    .expect("run 3 is a clean NoOp run");

    // The universal claim, not a spot check: nothing is freshly `Created` on
    // a converged run -- a stray `Created` here would mean something
    // silently re-ran instead of reading its already-converged state. No
    // exclusions: the document holds no write-only `doppler.secret.set`
    // sink, which would report `Created` on every apply.
    let created3: Vec<&str> = applied3
        .nodes
        .iter()
        .filter(|n| matches!(n.status, NodeStatus::Created))
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        created3.is_empty(),
        "run 3 must create nothing: {created3:?}"
    );

    for node in &applied3.nodes {
        assert!(
            !matches!(node.status, NodeStatus::Failed { .. } | NodeStatus::NotRun),
            "run 3: `{}` must not fail or be skipped: {:?}",
            node.name,
            node.status
        );
    }
    assert!(applied3.blocked.is_empty(), "run 3 has no blocked gate");

    let applied3_json = serde_json::to_string(&applied3).unwrap();
    assert_no_secret_leaked(&applied3_json);
}

/// The document itself, independent of any input: `check` alone is the
/// static proof that no secret reaches a non-secret port here (the design
/// doc's own invariant, enforced structurally, never by review).
/// Adversarial pass 4 (2026-09-29): the operator's own names are this
/// document's policy ("Just update the doc"), so they are pinned here
/// against the document itself. Every other test in this file supplies
/// `base_configs` explicitly and seeds its fake state under the same
/// names the document reads, so a drifted default, a drifted literal, or
/// a GitHub node that stopped binding its Doppler-resolved `token` (and
/// silently fell back to `WILLIKINS_GITHUB_TOKEN`) would pass unnoticed
/// there.
#[test]
// D1 grew this test with its own literal/gate pinning assertions, one
// long linear scenario against the same document, same as this file's
// other over-100-line test.
#[allow(clippy::too_many_lines)]
fn the_document_reads_the_real_layout_by_name() {
    use willikins_core::{Binding, NodeName, PortName};

    let workflow = document();
    let node = |name: &str| {
        workflow
            .nodes
            .get(&NodeName::parse(name).unwrap())
            .unwrap_or_else(|| panic!("node `{name}` exists"))
    };
    let literal =
        |name: &str, port: &str| match node(name).with.get(&PortName::parse(port).unwrap()) {
            Some(Binding::Literal(text)) => text.clone(),
            other => panic!("`{name}.{port}` must be a literal, got {other:?}"),
        };
    let from = |node: &str, port: &str| Binding::Step {
        node: NodeName::parse(node).unwrap(),
        port: PortName::parse(port).unwrap(),
    };

    // T4/fact 1: `platform` is the bare literal `UNIVERSAL` on all three
    // bundle identifiers, not a declared input -- IOS was a permanent
    // trap once the account's identifiers all read UNIVERSAL and the API
    // refused to change platform back.
    for name in ["app_id", "nse_id", "widgets_id"] {
        assert_eq!(literal(name, "platform"), "UNIVERSAL", "`{name}.platform`");
    }
    assert!(
        !workflow
            .inputs
            .contains_key(&InputName::parse("platform").unwrap()),
        "`platform` must not be a declared input any more"
    );

    // T4: the App Attest gate -- host identifier only, reading from
    // `app_id` directly (not chained through `app_app_groups`), and the
    // host profile's `identifier` binds through it while `name` still
    // binds through `app_app_groups` -- two independent ports of the same
    // node, ordered after both gates.
    assert_eq!(
        node("app_app_attest").tool.as_str(),
        "appstore.bundle_id_capability.gate"
    );
    assert_eq!(literal("app_app_attest", "capability"), "APP_ATTEST");
    assert_eq!(
        node("app_app_attest")
            .with
            .get(&PortName::parse("identifier").unwrap()),
        Some(&from("app_id", "identifier")),
        "app_app_attest must read from app_id directly, not chained through app_app_groups"
    );
    assert_eq!(
        node("app_profile")
            .with
            .get(&PortName::parse("identifier").unwrap()),
        Some(&from("app_app_attest", "identifier")),
        "app_profile.identifier must bind through app_app_attest"
    );
    assert_eq!(
        node("app_profile")
            .with
            .get(&PortName::parse("name").unwrap()),
        Some(&from("app_app_groups", "identifier")),
        "app_profile.name must still bind through app_app_groups"
    );
    // The NSE and widgets profiles are unaffected: no App Attest gate
    // applies to either extension.
    for (profile, gate) in [
        ("nse_profile", "nse_app_groups"),
        ("widgets_profile", "widgets_app_groups"),
    ] {
        assert_eq!(
            node(profile)
                .with
                .get(&PortName::parse("identifier").unwrap()),
            Some(&from(gate, "identifier")),
            "`{profile}.identifier` must still bind through `{gate}` alone"
        );
    }

    // App Store Connect: the real base config, the real secret names.
    for (name, tool, secret) in [
        (
            "issuer_id_text",
            "doppler.value.get",
            "APP_STORE_CONNECT_API_KEY_ISSUER_ID",
        ),
        (
            "key_id_text",
            "doppler.value.get",
            "APP_STORE_CONNECT_API_KEY_ID",
        ),
        (
            "key_base64",
            "doppler.secret.get",
            "APP_STORE_CONNECT_API_KEY_BASE64",
        ),
    ] {
        assert_eq!(node(name).tool.as_str(), tool, "`{name}`'s tool");
        assert_eq!(literal(name, "config"), "appstore-connect/deploy_ios");
        assert_eq!(literal(name, "name"), secret);
    }

    // GitHub: the read-only clone token comes from github/bande-a-bonnot,
    // through the parse tool, into `monorepo_ref`'s own `token` port.
    assert_eq!(node("gh_token_secret").tool.as_str(), "doppler.secret.get");
    assert_eq!(
        literal("gh_token_secret", "config"),
        "github/bande-a-bonnot"
    );
    assert_eq!(literal("gh_token_secret", "name"), "GH_CLONE_TOKEN");
    assert_eq!(node("gh_token").tool.as_str(), "github.token.parse");
    assert_eq!(
        node("gh_token")
            .with
            .get(&PortName::parse("value").unwrap()),
        Some(&from("gh_token_secret", "value"))
    );
    assert_eq!(
        node("monorepo_ref")
            .with
            .get(&PortName::parse("token").unwrap()),
        Some(&from("gh_token", "value")),
        "monorepo_ref must authenticate with the Doppler-resolved clone token"
    );

    // W1: the write token is new and narrower, from a Doppler config no
    // app config inherits, and binds only `walter_files.token` -- never
    // `monorepo_ref`'s own, older, wider `gh_token`.
    assert_eq!(
        node("gh_write_token_secret").tool.as_str(),
        "doppler.secret.get"
    );
    assert_eq!(
        literal("gh_write_token_secret", "config"),
        "github/bande-a-bonnot_willikins"
    );
    assert_eq!(
        literal("gh_write_token_secret", "name"),
        "GH_CONTENTS_WRITE_TOKEN"
    );
    assert_eq!(node("gh_write_token").tool.as_str(), "github.token.parse");
    assert_eq!(
        node("gh_write_token")
            .with
            .get(&PortName::parse("value").unwrap()),
        Some(&from("gh_write_token_secret", "value"))
    );
    assert_eq!(
        node("walter_files")
            .with
            .get(&PortName::parse("token").unwrap()),
        Some(&from("gh_write_token", "value")),
        "walter_files must authenticate with its own, narrower write token"
    );
    assert_eq!(node("walter_files").tool.as_str(), "github.scaffold.ensure");
    assert_eq!(literal("walter_files", "branch"), "main");
    assert_eq!(
        literal("walter_files", "marker"),
        "apps/walter/.willikins-scaffold"
    );

    // Every `github.*` node (other than the parse tool, which has no
    // `token` port at all) is accounted for above -- `monorepo_ref` and
    // `walter_files`, nothing else.
    let github_nodes: Vec<&str> = workflow
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.tool.as_str().starts_with("github.") && n.tool.as_str() != "github.token.parse"
        })
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(github_nodes, ["monorepo_ref", "walter_files"]);

    // D2: Buildkite, the same shape -- the token comes from buildkite/prd,
    // through the parse tool, into every Buildkite provider node's `token`
    // port.
    assert_eq!(node("bk_token_secret").tool.as_str(), "doppler.secret.get");
    assert_eq!(literal("bk_token_secret", "config"), "buildkite/prd");
    assert_eq!(
        literal("bk_token_secret", "name"),
        "PIPELINE_CREATION_TOKEN"
    );
    assert_eq!(node("bk_token").tool.as_str(), "buildkite.token.parse");
    assert_eq!(
        node("bk_token")
            .with
            .get(&PortName::parse("value").unwrap()),
        Some(&from("bk_token_secret", "value"))
    );
    let buildkite_nodes: Vec<&str> = workflow
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.tool.as_str().starts_with("buildkite.") && n.tool.as_str() != "buildkite.token.parse"
        })
        .map(|(name, _)| name.as_str())
        .collect();
    // W1: `bootstrap_gate` (`buildkite.pipeline.bootstrap.gate`) joins
    // the set -- it authenticates and references the org exactly like
    // `buildkite_cluster`/`pipeline`.
    assert_eq!(
        buildkite_nodes,
        ["buildkite_cluster", "pipeline", "bootstrap_gate"]
    );
    for name in &buildkite_nodes {
        assert_eq!(
            node(name).with.get(&PortName::parse("token").unwrap()),
            Some(&from("bk_token", "value")),
            "`{name}` must authenticate with the Doppler-resolved token, never the environment"
        );
        assert_eq!(
            literal(name, "org"),
            "la-bande-a-bonnot",
            "`{name}` must reference the real Buildkite org, not a caller-supplied input"
        );
    }
    assert_eq!(literal("buildkite_cluster", "name"), "Default cluster");
    assert_eq!(
        node("bootstrap_gate")
            .with
            .get(&PortName::parse("slug").unwrap()),
        Some(&from("pipeline", "slug")),
        "bootstrap_gate.slug must bind from pipeline.slug"
    );
    assert_eq!(
        node("bootstrap_gate")
            .with
            .get(&PortName::parse("expected").unwrap()),
        Some(&from("bootstrap_yml", "file")),
        "bootstrap_gate.expected must bind from bootstrap_yml.file"
    );
    for removed in ["buildkite_org", "cluster"] {
        assert!(
            !workflow
                .inputs
                .contains_key(&InputName::parse(removed).unwrap()),
            "`{removed}` must not be a declared input any more"
        );
    }

    // D1: `org`, `slug` and `monorepo` are bare literals, not inputs --
    // "Just update the doc". `base_configs` alone stays a defaulted
    // input: the document format has no syntax to bind a list literal to
    // a `with:` port, and `for_each` must always be a reference.
    for removed in ["org", "slug", "monorepo"] {
        assert!(
            !workflow
                .inputs
                .contains_key(&InputName::parse(removed).unwrap()),
            "`{removed}` must not be a declared input any more"
        );
    }
    assert_eq!(literal("names", "org"), "Bande-a-Bonnot");
    assert_eq!(literal("names", "slug"), "walter");
    assert_eq!(literal("monorepo_ref", "repo"), "Bande-a-Bonnot/monorepo");

    // D1: `base_config_gate` sits ahead of `inherit`, one instance per
    // base config, and `inherit.inherits` binds the *aggregate* of all
    // three instances -- never `${{ inputs.base_configs }}` directly, so
    // a missing or non-inheritable base config blocks its own gate
    // instance rather than reaching `inherit` at all.
    assert_eq!(
        node("base_config_gate").tool.as_str(),
        "doppler.config.inheritable.gate"
    );
    assert_eq!(
        node("base_config_gate").for_each.as_ref(),
        Some(&Binding::Input(InputName::parse("base_configs").unwrap())),
        "base_config_gate must expand over the base_configs input"
    );
    assert_eq!(
        node("base_config_gate")
            .with
            .get(&PortName::parse("config").unwrap()),
        Some(&Binding::Item),
        "each instance's own config port must bind ${{{{ item }}}}"
    );
    assert_eq!(
        node("inherit")
            .with
            .get(&PortName::parse("inherits").unwrap()),
        Some(&from("base_config_gate", "config")),
        "inherit.inherits must bind the aggregate of base_config_gate's own instances, \
         never inputs.base_configs directly"
    );

    // The three shared base configs prd_config inherits, by default.
    let default = workflow
        .inputs
        .get(&InputName::parse("base_configs").unwrap())
        .and_then(|input| input.default.as_ref())
        .expect("base_configs has a default");
    let names: Vec<String> = default
        .as_list()
        .expect("a list default")
        .iter()
        .map(|item| item.render().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "appstore-connect/deploy_ios",
            "github/bande-a-bonnot",
            "open-telemetry/prd_signoz"
        ]
    );
}

#[test]
fn the_document_checks_cleanly_against_the_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&document(), &catalog).expect("the document checks cleanly");
}

/// D1: `base_config_gate` sits ahead of `inherit` -- one gate per base
/// config, `inherit.inherits` bound to the aggregate of all three
/// instances. One base config missing (`github/bande-a-bonnot`, seeded
/// as neither existing nor inheritable) blocks exactly that instance;
/// the other two, seeded present and inheritable, still `Compute`.
/// `inherit` itself -- a `Step` binding aggregating a `for_each` gate --
/// plans `Skip` rather than reaching Doppler with an incomplete list at
/// apply time. Nothing else in the graph is held back: `inherit`'s own
/// output feeds no other node, so every
/// independent node -- the three bundle identifiers, `doppler`, the
/// Buildkite pipeline -- still plans and applies for real.
#[test]
fn a_missing_base_config_blocks_its_gate_and_skips_inherit() {
    const MISSING: &str = "github/bande-a-bonnot";
    const PRESENT: [&str; 2] = ["appstore-connect/deploy_ios", "open-telemetry/prd_signoz"];

    let workflow = document();
    let state = seeded_state_with_base_configs(&PRESENT);
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));

    let inputs = with_acknowledgements(base_inputs());
    let planned =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("the document plans: {err}"));

    assert_eq!(
        action_of(&planned, "base_config_gate", Some(MISSING)),
        Action::Blocked,
        "the missing base config's own gate instance must be Blocked"
    );
    for present in PRESENT {
        assert_eq!(
            action_of(&planned, "base_config_gate", Some(present)),
            Action::Compute,
            "`{present}` is seeded present and inheritable, so its gate instance must be Compute"
        );
    }
    assert_eq!(
        action_of(&planned, "inherit", None),
        Action::Skip,
        "inherit aggregates all three base_config_gate instances, so one Blocked instance skips it"
    );

    let entry = planned
        .blocked
        .iter()
        .find(|b| b.node.as_str() == "base_config_gate" && b.instance.as_deref() == Some(MISSING))
        .unwrap_or_else(|| {
            panic!(
                "`base_config_gate[{MISSING}]` is blocked: {:?}",
                planned.blocked
            )
        });
    let holds_back: std::collections::BTreeSet<&str> = entry
        .holds_back
        .iter()
        .map(willikins_core::NodeName::as_str)
        .collect();
    assert_eq!(
        holds_back,
        std::collections::BTreeSet::from(["inherit"]),
        "the missing base config must hold back exactly `inherit`, nothing else"
    );

    // Independent nodes -- nothing downstream of `inherit` exists in this
    // graph, so no *other* node is skipped by this gate.
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert_eq!(
            action_of(&planned, node, None),
            Action::Create,
            "`{node}` does not depend on `inherit` and must still plan for real"
        );
    }
    assert_eq!(action_of(&planned, "doppler", None), Action::Create);
    assert_eq!(action_of(&planned, "pipeline", None), Action::Create);

    let plan_json = serde_json::to_string(&planned).unwrap();
    assert_no_secret_leaked(&plan_json);

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &planned,
        &approval(),
        &mut observer,
    )
    .expect("a blocked run is Ok, not an error");

    assert!(
        matches!(status_of(&applied, "inherit", None), NodeStatus::Skipped),
        "`inherit` must be Skipped, never attempted, when one of its own base configs is missing"
    );
    for node in ["app_id", "nse_id", "widgets_id"] {
        assert!(
            matches!(status_of(&applied, node, None), NodeStatus::Created),
            "`{node}` must still run and be Created"
        );
    }
    assert!(
        !applied.blocked.is_empty(),
        "the missing base config must still be reported blocked"
    );

    let applied_json = serde_json::to_string(&applied).unwrap();
    assert_no_secret_leaked(&applied_json);
}

/// Decision (i): `data_protection`'s literal setting and
/// `Walter.entitlements`' own `com.apple.developer.default-data-protection`
/// value must name the same Apple data-protection class, so the two
/// cannot drift apart now that `data_protection` is no longer a
/// caller-supplied input.
#[test]
fn data_protection_literal_and_entitlement_name_the_same_class() {
    fn normalized(text: &str) -> String {
        text.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect()
    }

    use willikins_core::{Binding, NodeName, PortName};
    let workflow = document();
    let data_protection_setting = match workflow.nodes[&NodeName::parse("data_protection").unwrap()]
        .with
        .get(&PortName::parse("setting").unwrap())
    {
        Some(Binding::Literal(text)) => text.clone(),
        other => panic!("data_protection.setting must be a literal, got {other:?}"),
    };
    let entitlements_template = match workflow.nodes
        [&NodeName::parse("walter_entitlements").unwrap()]
        .with
        .get(&PortName::parse("template").unwrap())
    {
        Some(Binding::Literal(text)) => text.clone(),
        other => panic!("walter_entitlements.template must be a literal, got {other:?}"),
    };
    let class_token = "FIRSTUSERAUTH";
    assert!(
        normalized(&data_protection_setting).contains(class_token),
        "data_protection.setting must name the FirstUserAuth class: {data_protection_setting}"
    );
    assert!(
        normalized(&entitlements_template).contains(class_token),
        "Walter.entitlements must name the FirstUserAuth class"
    );
}

/// Decision (b)/(c): a scaffold conflict at plan time. Pre-landing fake
/// state (no marker seeded) with the exact path
/// `apps/walter/ios/BUILD.bazel` holding different content than what this
/// document would render: `plan` fails, naming that path -- never an
/// overwrite.
#[test]
fn a_preexisting_differing_file_fails_plan_naming_the_path() {
    let workflow = document();
    let state = seeded_state();
    {
        let mut locked = state.lock().unwrap();
        *locked = std::mem::take(&mut *locked).with_scaffold_files(
            &GitHubRepo::parse(MONOREPO).unwrap(),
            &willikins_types::GitBranchName::parse("main").unwrap(),
            &[("apps/walter/ios/BUILD.bazel", "# someone else's file\n")],
        );
    }
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks: {errors:?}"));
    let inputs = with_acknowledgements(base_inputs());
    let err = plan(&checked, &inputs, &catalog)
        .expect_err("plan must fail: a differing file already exists at that path");
    let message = err.to_string();
    assert!(
        message.contains("apps/walter/ios/BUILD.bazel"),
        "plan error must name the differing path: {message}"
    );
}

/// Decision (i)/the survey: the Buildkite pipeline is created only after
/// its own `.buildkite/` files exist on the branch -- `pipeline.repo`
/// binds from `walter_files.repo`, never `monorepo_ref.repo` directly.
#[test]
fn pipeline_is_ordered_after_the_scaffold() {
    use willikins_core::{Binding, NodeName, PortName};
    let workflow = document();
    let pipeline = &workflow.nodes[&NodeName::parse("pipeline").unwrap()];
    assert_eq!(
        pipeline.with.get(&PortName::parse("repo").unwrap()),
        Some(&Binding::Step {
            node: NodeName::parse("walter_files").unwrap(),
            port: PortName::parse("repo").unwrap(),
        }),
        "pipeline.repo must bind from walter_files.repo, so the pipeline is ordered after the \
         scaffold"
    );
}

/// Acceptance 10: `walter_files` binds every render `WALTER_FILES` names,
/// and no `operator.acknowledge` node remains for M3 or M7.
#[test]
fn walter_files_binds_every_render_and_m3_m7_acknowledgements_are_gone() {
    use willikins_core::{Binding, NodeName, PortName};
    let workflow = document();
    let walter_files = &workflow.nodes[&NodeName::parse("walter_files").unwrap()];
    let files = match walter_files.with.get(&PortName::parse("files").unwrap()) {
        Some(Binding::List(elements)) => elements,
        other => panic!("walter_files.files must be a list binding, got {other:?}"),
    };
    assert_eq!(
        files.len(),
        WALTER_FILES.len(),
        "walter_files.files must bind every render in WALTER_FILES"
    );

    for name in ["m3_repo_files", "m7_bootstrap"] {
        assert!(
            !workflow.nodes.contains_key(&NodeName::parse(name).unwrap()),
            "`{name}` must not exist as a node any more"
        );
    }
    for input in ["m3_repo_files_done", "m7_bootstrap_done", "data_protection"] {
        assert!(
            !workflow
                .inputs
                .contains_key(&InputName::parse(input).unwrap()),
            "`{input}` must not be a declared input any more"
        );
    }
}

/// Acceptance 11: one `insta` snapshot per rendered file, for the real
/// identifiers `com.bande-a-bonnot.walter`, `.nse`, `.widgets` -- the
/// artefact the operator reviews and builds (verify item 8). Read at plan
/// time: `repo.file.render` is pure, so `plan` itself computes and
/// carries every render's known `file` output (no apply needed).
#[test]
fn rendered_files_snapshot_for_the_real_identifiers() {
    for (node, file) in rendered_files_for_the_real_identifiers() {
        insta::assert_snapshot!(
            format!("walter_render_{node}"),
            format!("{}\n---\n{}", file.path(), file.content())
        );
    }
}

/// Verify item 8 (2026-10-01): `bazel build --config=ci //apps/walter/...`
/// over the rendered files refused `apps/walter/ios/BUILD.bazel` with
/// "invalid escape sequence: \d" -- the template's YAML literal block keeps
/// a backslash as written, so `"\d+"` in the template is `\d` in Starlark,
/// which Bazel rejects (Danksworth and Pocket Claw write `\\d`). The
/// snapshot above recorded the broken file faithfully; this pins the rule
/// the build enforces: every backslash in a rendered Starlark file starts
/// an escape Starlark accepts.
#[test]
fn every_rendered_starlark_file_uses_only_valid_escapes() {
    const STARLARK_ESCAPES: &[u8] = b"abfnrtv\\'\"\n01234567xuU";
    let mut starlark_files = 0;
    for (node, file) in rendered_files_for_the_real_identifiers() {
        let extension = std::path::Path::new(file.path().as_str()).extension();
        if extension.is_none_or(|ext| ext != "bazel") {
            continue;
        }
        starlark_files += 1;
        let bytes = file.content().as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                let next = bytes.get(i + 1).copied();
                assert!(
                    next.is_some_and(|b| STARLARK_ESCAPES.contains(&b)),
                    "`{node}` ({}) has an escape Starlark rejects at byte {i}: {:?}",
                    file.path(),
                    next.map(char::from)
                );
                i += 2;
            } else {
                i += 1;
            }
        }
    }
    assert_eq!(starlark_files, 2, "Walter renders two BUILD.bazel files");
}

/// Acceptance 7 (W1, decision (g)): the rendered `ios/BUILD.bazel` has the
/// `app_icon` `genrule` with exactly three `outs`, in this order, and
/// `ios_application` `Walter` wires it in through `app_icons`.
#[test]
fn app_icon_genrule_declares_exactly_three_outs_and_is_wired_to_the_application() {
    let (_, file) = rendered_files_for_the_real_identifiers()
        .into_iter()
        .find(|(node, _)| *node == "build_bazel_ios")
        .expect("build_bazel_ios was rendered");
    let content = file.content();

    let start = content
        .find("genrule(\n    name = \"app_icon\",")
        .expect("the app_icon genrule is present, with exactly this attribute order");
    let close = content[start..]
        .find("\n)\n")
        .expect("the genrule block is closed");
    let block = &content[start..start + close];

    assert!(
        block.contains("srcs = [\"tools/generate_app_icon.py\"],"),
        "the genrule's srcs: {block}"
    );
    assert!(
        block.contains("cmd = \"python3 $(location tools/generate_app_icon.py) $(OUTS)\","),
        "the genrule's cmd: {block}"
    );

    let outs_start = block.find("outs = [").expect("outs is present") + "outs = [".len();
    let outs_end = block[outs_start..].find(']').expect("outs is closed") + outs_start;
    let outs: Vec<&str> = block[outs_start..outs_end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_end_matches(',').trim_matches('"'))
        .collect();
    assert_eq!(
        outs,
        vec![
            "Resources/AppIcon.xcassets/Contents.json",
            "Resources/AppIcon.xcassets/AppIcon.appiconset/Contents.json",
            "Resources/AppIcon.xcassets/AppIcon.appiconset/AppIcon-1024.png",
        ],
        "the genrule must declare exactly these three outs, in this order, and no other"
    );

    let app_index = content
        .find("name = \"Walter\",")
        .expect("ios_application Walter is present");
    let icons_index = content
        .find("app_icons = [\":app_icon\"],")
        .expect("app_icons = [\":app_icon\"] is present");
    assert!(
        icons_index > app_index,
        "app_icons must sit inside the Walter ios_application block, after its name"
    );
}

/// Acceptance 7 (W1, decision (g)): the rendered `Info.plist` declares all
/// four `UISupportedInterfaceOrientations`, in Danksworth's exact order,
/// and sets no `CFBundleIconName` -- `actool`'s partial plist supplies it.
#[test]
fn info_plist_declares_all_four_interface_orientations_in_danksworths_order() {
    let (_, file) = rendered_files_for_the_real_identifiers()
        .into_iter()
        .find(|(node, _)| *node == "info_plist")
        .expect("info_plist was rendered");
    let content = file.content();

    assert!(
        !content.contains("CFBundleIconName"),
        "actool's partial Info.plist supplies CFBundleIconName; the template must not set it"
    );

    let expected = "    <key>UISupportedInterfaceOrientations</key>\n\
         \x20   <array>\n\
         \x20       <string>UIInterfaceOrientationPortrait</string>\n\
         \x20       <string>UIInterfaceOrientationPortraitUpsideDown</string>\n\
         \x20       <string>UIInterfaceOrientationLandscapeLeft</string>\n\
         \x20       <string>UIInterfaceOrientationLandscapeRight</string>\n\
         \x20   </array>\n";
    assert!(
        content.contains(expected),
        "the four orientations, in Danksworth's order, must appear exactly like this: \
         {content}"
    );
}

/// Acceptance 8 (W2, decision (h)): the rendered `tools/release-config.json`
/// parses with `serde_json` and holds the three real bundle identifiers.
#[test]
fn rendered_release_config_json_parses_and_holds_the_real_identifiers() {
    let (_, file) = rendered_files_for_the_real_identifiers()
        .into_iter()
        .find(|(node, _)| *node == "release_config_json")
        .expect("release_config_json was rendered");
    let parsed: serde_json::Value = serde_json::from_str(file.content())
        .unwrap_or_else(|err| panic!("rendered release-config.json must parse as JSON: {err}"));
    assert_eq!(
        parsed["bundle_ids"]["app"], "com.bande-a-bonnot.walter",
        "bundle_ids.app must hold the real app identifier"
    );
    assert_eq!(
        parsed["bundle_ids"]["nse"], "com.bande-a-bonnot.walter.nse",
        "bundle_ids.nse must hold the real NSE identifier"
    );
    assert_eq!(
        parsed["bundle_ids"]["widgets"], "com.bande-a-bonnot.walter.widgets",
        "bundle_ids.widgets must hold the real widgets identifier"
    );
}

/// Every render node's planned `file`, for the real identifiers
/// `com.bande-a-bonnot.walter`, `.nse`, `.widgets`. Read at plan time:
/// `repo.file.render` is pure, so `plan` itself computes and carries every
/// render's known `file` output (no apply needed).
fn rendered_files_for_the_real_identifiers() -> Vec<(&'static str, RepoFile)> {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));

    let mut inputs = base_inputs();
    inputs.insert(
        InputName::parse("app_identifier").unwrap(),
        scalar("AppleBundleIdentifier", "com.bande-a-bonnot.walter"),
    );
    inputs.insert(
        InputName::parse("nse_identifier").unwrap(),
        scalar("AppleBundleIdentifier", "com.bande-a-bonnot.walter.nse"),
    );
    inputs.insert(
        InputName::parse("widgets_identifier").unwrap(),
        scalar("AppleBundleIdentifier", "com.bande-a-bonnot.walter.widgets"),
    );
    let inputs = with_acknowledgements(inputs);

    let planned = plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("{err}"));

    WALTER_FILES
        .iter()
        .map(|(node, path)| {
            let planned_node = planned
                .nodes
                .iter()
                .find(|n| n.name.as_str() == *node && n.instance.is_none())
                .unwrap_or_else(|| panic!("node `{node}` was planned"));
            let file = planned_node
                .outputs
                .get(&PortName::parse("file").unwrap())
                .unwrap_or_else(|| panic!("`{node}.file` was planned"))
                .downcast::<RepoFile>()
                .unwrap_or_else(|| panic!("`{node}.file` is a RepoFile"));
            assert_eq!(
                file.path().as_str(),
                *path,
                "`{node}`'s rendered path must match its WALTER_FILES row"
            );
            (*node, file.clone())
        })
        .collect()
}

/// W0: the one table every count and every ordered list in this file
/// derives from -- each render node `walter_files.files` binds, in the
/// order it binds them, paired with its repo path. A later task adds a
/// row here instead of editing five assertions.
const WALTER_FILES: &[(&str, &str)] = &[
    ("build_bazel_app", "apps/walter/BUILD.bazel"),
    ("build_bazel_ios", "apps/walter/ios/BUILD.bazel"),
    (
        "walter_app_swift",
        "apps/walter/ios/Walter/Sources/WalterApp.swift",
    ),
    (
        "nse_swift",
        "apps/walter/ios/WalterNotificationService/Sources/NotificationService.swift",
    ),
    (
        "widgets_swift",
        "apps/walter/ios/WalterWidgets/Sources/WalterWidgets.swift",
    ),
    ("info_plist", "apps/walter/ios/Resources/Info.plist"),
    (
        "nse_info_plist",
        "apps/walter/ios/Resources/WalterNotificationService-Info.plist",
    ),
    (
        "widgets_info_plist",
        "apps/walter/ios/Resources/WalterWidgets-Info.plist",
    ),
    (
        "walter_entitlements",
        "apps/walter/ios/Resources/Walter.entitlements",
    ),
    (
        "nse_entitlements",
        "apps/walter/ios/Resources/WalterNotificationService.entitlements",
    ),
    (
        "widgets_entitlements",
        "apps/walter/ios/Resources/WalterWidgets.entitlements",
    ),
    (
        "privacy_manifest",
        "apps/walter/ios/Resources/PrivacyInfo.xcprivacy",
    ),
    ("pipeline_yml", "apps/walter/.buildkite/pipeline.yml"),
    (
        "upload_pipeline_sh",
        "apps/walter/.buildkite/upload-pipeline.sh",
    ),
    ("bootstrap_yml", "apps/walter/.buildkite/bootstrap.yml"),
    (
        "provider_settings",
        "apps/walter/.buildkite/provider-settings.json",
    ),
    ("buildkite_readme", "apps/walter/.buildkite/README.md"),
    (
        "generate_app_icon_py",
        "apps/walter/ios/tools/generate_app_icon.py",
    ),
    ("walter_gitignore", "apps/walter/.gitignore"),
    (
        "release_config_json",
        "apps/walter/tools/release-config.json",
    ),
];

/// W0: the number of `{{ N }}` placeholders `every_placeholder_sits_in_a_quoted_or_identifier_only_position`
/// expects across every render in [`WALTER_FILES`] (six in
/// `ios/BUILD.bazel`: three profile names, three bundle ids; one app
/// group per entitlements file; three JSON-value lines in
/// `tools/release-config.json`, W2).
const PLACEHOLDER_COUNT: usize = 12;

/// Walter's scaffold marker, `walter_files.marker`'s own literal.
const MARKER: &str = "apps/walter/.willikins-scaffold";

/// Adversarial pass (Walter group, 2026-10-01): every `{{ N }}`
/// placeholder in every `repo.file.render` template sits in a quoted or
/// identifier-only position -- never a command, a path, or an unquoted
/// YAML/JSON scalar. `TemplateValue`'s grammar protects *quoting* (no
/// `"`, `\`, `<`, `&`, whitespace, `$`, backtick, `;`, `|`, newline, no
/// leading `-`), not *placement*: a value is inert inside a Starlark
/// double-quoted string (it can hold neither `"` nor `\`) and inside an
/// XML `<string>` element (it can hold neither `<` nor `&`), but the same
/// value spliced into `upload-pipeline.sh`'s command line or a
/// `.buildkite/` step would be an argument CI executes. Both earlier
/// passes carried this forward to W1.
///
/// A strict allowlist on purpose: a template change that adds a
/// placeholder anywhere else must change this test, which is the review
/// that change needs.
///
/// W2 extends the allowlist by exactly one shape, `is_quoted_json_value`:
/// a whole JSON string value `"<key>": "{{ N }}"`, optional trailing
/// comma, `<key>` in {`app`, `nse`, `widgets`}, allowed only in
/// `apps/walter/tools/release-config.json` (decision (h)).
#[test]
fn every_placeholder_sits_in_a_quoted_or_identifier_only_position() {
    use willikins_core::{Binding, PortName};

    const RELEASE_CONFIG_PATH: &str = "apps/walter/tools/release-config.json";

    /// `bundle_id = "{{ N }}",` or `profile_name = "{{ N }}",` -- a whole
    /// Starlark string literal holding exactly one placeholder.
    fn is_quoted_starlark_attribute(line: &str) -> bool {
        let line = line.trim();
        ["bundle_id = \"", "profile_name = \""]
            .iter()
            .any(|prefix| {
                line.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix("\","))
                    .is_some_and(is_exact_placeholder)
            })
    }

    /// `<string>group.{{ N }}</string>` -- an app group identifier in an
    /// XML text node.
    fn is_app_group_string(line: &str) -> bool {
        line.trim()
            .strip_prefix("<string>group.")
            .and_then(|rest| rest.strip_suffix("</string>"))
            .is_some_and(is_exact_placeholder)
    }

    /// `"app": "{{ 0 }}"`, `"nse": "{{ 1 }}",`, `"widgets": "{{ 2 }}"` --
    /// a whole JSON string value for one of `release-config.json`'s three
    /// bundle-id keys, with an optional trailing comma. This shape says
    /// nothing about which file it is in: the caller restricts it to
    /// `RELEASE_CONFIG_PATH`.
    fn is_quoted_json_value(line: &str) -> bool {
        let line = line.trim();
        let line = line.strip_suffix(',').unwrap_or(line);
        ["app", "nse", "widgets"].iter().any(|key| {
            line.strip_prefix('"')
                .and_then(|rest| rest.strip_prefix(key))
                .and_then(|rest| rest.strip_prefix("\": \""))
                .and_then(|rest| rest.strip_suffix('"'))
                .is_some_and(is_exact_placeholder)
        })
    }

    fn is_exact_placeholder(text: &str) -> bool {
        text.strip_prefix("{{ ")
            .and_then(|rest| rest.strip_suffix(" }}"))
            .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
    }

    /// The gate every placeholder line must pass: which shapes are
    /// allowed, and (for the JSON-value shape) in which one file.
    fn placeholder_is_allowed(path: &str, line: &str) -> bool {
        is_quoted_starlark_attribute(line)
            || is_app_group_string(line)
            || (path == RELEASE_CONFIG_PATH && is_quoted_json_value(line))
    }

    // Negative cases (W2), checked before the real document is walked:
    // the JSON-value shape must be refused outside release-config.json,
    // and a different shape (a key outside {app, nse, widgets}) must be
    // refused even inside release-config.json.
    assert!(
        placeholder_is_allowed(RELEASE_CONFIG_PATH, "\"app\": \"{{ 0 }}\","),
        "sanity: the real shape must be accepted in its own file"
    );
    assert!(
        !placeholder_is_allowed("apps/walter/ios/BUILD.bazel", "\"app\": \"{{ 0 }}\","),
        "the release-config.json JSON-value shape must be refused in any other path"
    );
    assert!(
        !placeholder_is_allowed(RELEASE_CONFIG_PATH, "\"other\": \"{{ 0 }}\","),
        "a key outside {{app, nse, widgets}} must be refused even in release-config.json"
    );

    let workflow = document();
    let literal = |node: &willikins_core::Node, port: &str| match node
        .with
        .get(&PortName::parse(port).unwrap())
    {
        Some(Binding::Literal(text)) => text.clone(),
        other => panic!("`{port}` must be a literal, got {other:?}"),
    };

    let mut render_nodes = 0;
    let mut placeholders = 0;
    for (name, node) in &workflow.nodes {
        if node.tool.as_str() != "repo.file.render" {
            continue;
        }
        render_nodes += 1;
        let path = literal(node, "path");
        let template = literal(node, "template");
        let templated_file = path == "apps/walter/ios/BUILD.bazel"
            || path == RELEASE_CONFIG_PATH
            || std::path::Path::new(&path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("entitlements"));
        for line in template.lines().filter(|line| line.contains("{{")) {
            assert!(
                templated_file,
                "`{name}` ({path}) holds a placeholder, but only ios/BUILD.bazel, the \
                 entitlements files, and release-config.json may: {line}"
            );
            assert!(
                placeholder_is_allowed(&path, line),
                "`{name}` ({path}): a placeholder outside a quoted or identifier-only \
                 position: {line}"
            );
            placeholders += 1;
        }
    }
    assert_eq!(
        render_nodes,
        WALTER_FILES.len(),
        "every render node was inspected"
    );
    // Six in ios/BUILD.bazel (three profile names, three bundle ids), one
    // app group per entitlements file, and three JSON-value lines in
    // tools/release-config.json (W2).
    assert_eq!(
        placeholders, PLACEHOLDER_COUNT,
        "every placeholder was inspected"
    );
}

/// Adversarial pass (Walter group, 2026-10-01), W1's own open item: on a
/// first fake run, `walter_files` plans `Create` with its `files` input
/// fully known, each element exactly the corresponding render's planned
/// file. The plan is the operator's approval, and approval is the diff
/// review (decision (f)), so the plan must carry every byte the commit
/// will write. (The tool itself already refuses an unknown `files` as
/// `Invalid`; this pins the approval-shows-content property, not that
/// refusal.)
#[test]
fn walter_files_files_are_fully_known_at_plan_on_a_first_run() {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let planned = plan(&checked, &base_inputs(), &catalog)
        .unwrap_or_else(|err| panic!("a first run plans: {err}"));

    let node = |name: &str| {
        planned
            .nodes
            .iter()
            .find(|n| n.name.as_str() == name && n.instance.is_none())
            .unwrap_or_else(|| panic!("node `{name}` was planned"))
    };
    let walter_files = node("walter_files");
    assert_eq!(walter_files.action, Action::Create);
    let files = walter_files
        .inputs
        .get(&PortName::parse("files").unwrap())
        .expect("walter_files.files is planned");
    assert!(files.is_known(), "walter_files.files must be known at plan");
    let elements = files.as_list().expect("walter_files.files is a known list");
    assert_eq!(elements.len(), WALTER_FILES.len());

    for (element, (render, path)) in elements.iter().zip(WALTER_FILES.iter()) {
        let bound = willikins_types::downcast::<RepoFile>(element.as_ref())
            .unwrap_or_else(|| panic!("the element bound from `{render}` is a RepoFile"));
        let rendered = node(render)
            .outputs
            .get(&PortName::parse("file").unwrap())
            .and_then(|value| value.downcast::<RepoFile>())
            .unwrap_or_else(|| panic!("`{render}.file` is a known RepoFile at plan"));
        assert_eq!(bound.path(), rendered.path(), "`{render}`'s path");
        assert_eq!(bound.content(), rendered.content(), "`{render}`'s content");
        assert_eq!(
            bound.path().as_str(),
            *path,
            "`{render}`'s path must match its WALTER_FILES row"
        );
    }
}

/// Acceptance 10's clause W1 left untested: once the scaffold has landed,
/// a re-run with the marker present and **every** seeded file edited in
/// the fake state plans `walter_files` `NoOp`, applies it `Unchanged`,
/// and leaves every edit -- and the marker -- exactly as it found them.
/// The marker alone decides (decision (c)); a seed is never re-read.
#[test]
fn a_rerun_with_every_seeded_file_edited_plans_walter_files_noop() {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();

    let planned1 = plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("{err}"));
    let mut observer1 = RecordingObserver::new();
    let applied1 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned1,
        &approval(),
        &mut observer1,
    )
    .expect("run 1 applies (blocked gates are Ok)");
    assert!(
        matches!(
            status_of(&applied1, "walter_files", None),
            NodeStatus::Created
        ),
        "run 1 lands the scaffold"
    );

    let key = willikins_providers_fake::state::scaffold_key(
        &GitHubRepo::parse(MONOREPO).unwrap(),
        &willikins_types::GitBranchName::parse("main").unwrap(),
    );
    let edited: std::collections::HashMap<String, String> = {
        let mut locked = state.lock().unwrap();
        let landed = locked
            .scaffolds
            .get_mut(&key)
            .unwrap_or_else(|| panic!("run 1 landed files at `{key}`"));
        assert_eq!(
            landed.len(),
            WALTER_FILES.len() + 1,
            "every seed plus the marker landed"
        );
        for (path, content) in landed.iter_mut() {
            if path != MARKER {
                *content = format!("# a developer rewrote {path}\n");
            }
        }
        landed.clone()
    };

    let planned2 = plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("a re-run over edited seeds plans: {err}"));
    assert_eq!(action_of(&planned2, "walter_files", None), Action::NoOp);
    let mut observer2 = RecordingObserver::new();
    let applied2 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned2,
        &approval(),
        &mut observer2,
    )
    .expect("run 2 applies");
    assert!(
        matches!(
            status_of(&applied2, "walter_files", None),
            NodeStatus::Unchanged
        ),
        "run 2: walter_files must be Unchanged, got {:?}",
        status_of(&applied2, "walter_files", None)
    );
    assert_eq!(
        state.lock().unwrap().scaffolds.get(&key),
        Some(&edited),
        "every developer edit and the marker survive the re-run untouched"
    );
}

/// Decision (b)'s `Foreign` row at the document level: a marker path that
/// already holds a file willikins did not write fails `plan` as
/// `NameTaken` on `walter_files`, naming the marker in its key, and
/// nothing is written.
#[test]
fn a_foreign_marker_fails_plan_and_writes_nothing() {
    let workflow = document();
    let state = seeded_state();
    {
        let mut locked = state.lock().unwrap();
        *locked = std::mem::take(&mut *locked).with_scaffold_files(
            &GitHubRepo::parse(MONOREPO).unwrap(),
            &willikins_types::GitBranchName::parse("main").unwrap(),
            &[(MARKER, "someone else's notes\n")],
        );
    }
    let before = state.lock().unwrap().scaffolds.clone();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks: {errors:?}"));
    let err = plan(&checked, &base_inputs(), &catalog)
        .expect_err("plan must refuse a marker path willikins does not own");
    match &err {
        willikins_core::PlanError::NameTaken { node, key, .. } => {
            assert_eq!(node.as_str(), "walter_files");
            let marker = key
                .get(&PortName::parse("marker").unwrap())
                .map(|value| value.render().to_string());
            assert_eq!(
                marker.as_deref(),
                Some(MARKER),
                "the refusal names the marker"
            );
        }
        other => panic!("expected NameTaken on walter_files, got {other}"),
    }
    assert_eq!(
        state.lock().unwrap().scaffolds,
        before,
        "a refused plan writes nothing"
    );
}

/// W0 (acceptance 6; SHARED VALUES "Size budgets"): the document's own
/// byte length never exceeds three quarters of `MAX_DOCUMENT_BYTES`
/// (192 KiB of the 256 KiB limit). Exceeding it means trimming a
/// template, never raising the budget (the plan's Risk 1).
#[test]
fn the_document_is_within_its_size_budget() {
    let path = workspace_root().join("workflows/walter-ios-app.yaml");
    let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("reading the document: {err}"));
    assert!(
        bytes.len() <= 196_608,
        "the document is {} bytes, over the 196,608-byte (192 KiB) budget",
        bytes.len()
    );
}

/// W0 (acceptance 6; SHARED VALUES "Size budgets"): every
/// `repo.file.render` template literal stays at or under 20,480
/// characters.
#[test]
fn every_render_template_is_within_its_size_budget() {
    use willikins_core::{Binding, PortName};
    let workflow = document();
    let mut checked = 0;
    for (name, node) in &workflow.nodes {
        if node.tool.as_str() != "repo.file.render" {
            continue;
        }
        let template = match node.with.get(&PortName::parse("template").unwrap()) {
            Some(Binding::Literal(text)) => text,
            other => panic!("`{name}.template` must be a literal, got {other:?}"),
        };
        let len = template.chars().count();
        assert!(
            len <= 20_480,
            "`{name}`'s template is {len} characters, over the 20,480-character budget"
        );
        checked += 1;
    }
    assert_eq!(
        checked,
        WALTER_FILES.len(),
        "every render node's template was checked"
    );
}

/// W0 (acceptance 6; SHARED VALUES "Size budgets"): `walter_files.files`
/// never exceeds `github.scaffold.ensure`'s own 64-file bound.
#[test]
fn walter_files_holds_at_most_sixty_four_entries() {
    use willikins_core::{Binding, NodeName, PortName};
    let workflow = document();
    let walter_files = &workflow.nodes[&NodeName::parse("walter_files").unwrap()];
    let files = match walter_files.with.get(&PortName::parse("files").unwrap()) {
        Some(Binding::List(elements)) => elements,
        other => panic!("walter_files.files must be a list binding, got {other:?}"),
    };
    assert!(
        files.len() <= 64,
        "walter_files.files holds {} entries, over github.scaffold.ensure's 64-file bound",
        files.len()
    );
}

/// W0 (acceptance 6): writes every one of Walter's rendered files, for
/// the real identifiers `com.bande-a-bonnot.walter`, `.nse` and
/// `.widgets`, to its repo path under `WILLIKINS_WALTER_RENDER_DIR` --
/// from a plan over the fake catalog, never an apply. This is the
/// rendered-set gate the template tasks (W1-W6) run their own checks
/// against (a scratch `bazel build`, `py_compile`, `bash -n`).
/// `#[ignore]`d: it writes to the filesystem and needs the env var set.
#[test]
#[ignore = "writes under WILLIKINS_WALTER_RENDER_DIR"]
fn render_walter_files_to_dir() {
    let dir = std::env::var("WILLIKINS_WALTER_RENDER_DIR")
        .unwrap_or_else(|_| panic!("WILLIKINS_WALTER_RENDER_DIR must be set"));
    let dir = std::path::PathBuf::from(dir);
    let mut written = 0;
    for (node, file) in rendered_files_for_the_real_identifiers() {
        let dest = dir.join(file.path().as_str());
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("creating `{}`'s parent: {err}", dest.display()));
        }
        std::fs::write(&dest, file.content())
            .unwrap_or_else(|err| panic!("writing `{node}` to `{}`: {err}", dest.display()));
        written += 1;
    }
    assert_eq!(written, WALTER_FILES.len(), "every render node was written");
    println!("wrote {written} files under {}", dir.display());
}
