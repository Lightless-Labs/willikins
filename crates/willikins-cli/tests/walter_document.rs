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
//!    finds nothing. The three profile nodes and the three
//!    `doppler.secret.set` nodes that depend on them are `Skip`, never
//!    read. So are the four `operator.acknowledge` leaves (no `done`
//!    supplied).
//!    - **Plan-only, between run 1 and run 2: App Groups is on, App
//!      Attest is still off.** The fake state gains the app record and
//!      `APP_GROUPS` on all three identifiers, but not yet `APP_ATTEST`
//!      on the host. A fresh `plan` (not applied) shows `app_app_attest`
//!      still `Blocked`, holding back exactly `app_profile` and
//!      `app_profile_to_doppler` -- while `nse_profile`/`widgets_profile`
//!      and their own Doppler writes, gated only by their own (now open)
//!      app-group gate, plan `Create`. This is the App Attest gate's own
//!      acceptance case: the host profile alone is held back, the
//!      extensions are unaffected.
//! 2. **The fake state satisfies every observed gate** (App Attest is now
//!    seeded on the host identifier too). Same inputs, same
//!    acknowledgements withheld. A fresh `plan` shows every
//!    previously-blocked gate `Compute` and the three profile nodes and
//!    their `doppler.secret.set` nodes `Create` -- **and nothing else
//!    changes**: every node that already ran in step 1 reads
//!    `Unchanged`/`Computed`. The four acknowledgement leaves are still
//!    `Blocked`, since no API and no seeded state can satisfy them.
//! 3. **The four acknowledgements are supplied.** A third run, same fake
//!    state, `done` on all four `*_done` inputs: every node reads
//!    `Unchanged`/`Computed`/`Converged` -- a `NoOp` run end to end.
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
/// characterization suite). The four acknowledgement inputs are the one
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

    // The three profiles and their Doppler writes are skipped, never
    // planned as Create -- they are held back by their own app-group gate.
    for node in [
        "app_profile",
        "nse_profile",
        "widgets_profile",
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
    ] {
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
    // it must hold back exactly the host app's profile and that profile's
    // Doppler write; the NSE and widgets profiles have no App Attest gate
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
    for node in ["app_profile", "app_profile_to_doppler"] {
        assert_eq!(
            action_of(&planned_attest_off, node, None),
            Action::Skip,
            "`{node}` is held back by the still-unmet App Attest gate"
        );
    }
    for node in [
        "nse_profile",
        "widgets_profile",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
    ] {
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
        std::collections::BTreeSet::from(["app_profile", "app_profile_to_doppler"]),
        "app_app_attest must hold back exactly the host profile and its Doppler write"
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
    for node in [
        "app_profile",
        "nse_profile",
        "widgets_profile",
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
    ] {
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
    for node in [
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
    ] {
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
    // EXACTLY the six nodes the app-group gates just unblocked -- nothing
    // else moved.
    let created2: std::collections::BTreeSet<&str> = applied2
        .nodes
        .iter()
        .filter(|n| matches!(n.status, NodeStatus::Created))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        created2,
        std::collections::BTreeSet::from([
            "app_profile",
            "nse_profile",
            "widgets_profile",
            "app_profile_to_doppler",
            "nse_profile_to_doppler",
            "widgets_profile_to_doppler",
        ]),
        "run 2 must create the three profiles and their Doppler writes, and nothing else"
    );

    // The ordering claim the whole design rests on (decision (j), point 2):
    // the host app-group gate holds back exactly its own profile and that
    // profile's Doppler write, never anything else's. `planned2` has no
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
        std::collections::BTreeSet::from(["app_profile", "app_profile_to_doppler"]),
        "app_app_groups must hold back exactly its own profile and that profile's Doppler write"
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
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
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
    // silently re-ran instead of reading its already-converged state.
    //
    // Exactly three nodes are excluded, by design, not by omission:
    // `doppler.secret.set` is a write-only sink that can never compare its
    // `value` against what is already stored (`willikins_providers_doppler::tools::secret_set`'s
    // own module doc; the fake mirrors it), so its own `ensure` reports
    // `changed: true` -- `NodeStatus::Created` -- on *every* call, run 3
    // included. That is this tool's documented behaviour everywhere it is
    // used in this workspace, not a defect this document introduces.
    let always_created: std::collections::BTreeSet<&str> = std::collections::BTreeSet::from([
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
    ]);
    let unexpectedly_created: Vec<&str> = applied3
        .nodes
        .iter()
        .filter(|n| {
            matches!(n.status, NodeStatus::Created) && !always_created.contains(n.name.as_str())
        })
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        unexpectedly_created.is_empty(),
        "run 3 must create nothing outside the three write-only Doppler sinks: {unexpectedly_created:?}"
    );
    for node in &always_created {
        assert!(
            matches!(status_of(&applied3, node, None), NodeStatus::Created),
            "`{node}` is a write-only sink and must report Created on every apply, run 3 included"
        );
    }

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
/// output feeds no other node (the three `doppler.secret.set` nodes bind
/// `config` from `prd_config`, never from `inherit`), so every
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
