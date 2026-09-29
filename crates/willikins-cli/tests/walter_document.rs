//! Milestone 3e task 3, acceptance test 8 (rewritten for gates, per the
//! plan's T3 row): `workflows/walter-ios-app.yaml` over the fake catalogue.
//!
//! `appstore.profile.ensure` is `Class::Destructive` since operator
//! decision 1, so every run of this document needs approval
//! (`Approval::Human`) -- the class is static, from the graph, unaffected
//! by which gates are open (the milestone plan's decision (j), point 5).
//!
//! Three runs, one shared `FakeState`, exactly as the plan's task
//! description asks:
//!
//! 1. **Gates unmet.** Nothing is seeded beyond the credential chain, the
//!    monorepo, the Buildkite cluster, and the distribution certificate.
//!    Every independent node (the three bundle identifiers, the three
//!    capabilities, Doppler, the Buildkite pipeline, the monorepo
//!    reference) plans and applies for real. `app_record` (a leaf) and
//!    the three `app_*_app_groups` gates are `Blocked` -- the identifiers
//!    they check do not exist yet, so their own `read` (which resolves the
//!    parent through `list_bundle_ids`) finds nothing. The three profile
//!    nodes and the three `doppler.secret.set` nodes that depend on them
//!    are `Skip`, never read. So are the four `operator.acknowledge`
//!    leaves (no `done` supplied).
//! 2. **The fake state satisfies the two observed gates** (the app record
//!    is seeded; `APP_GROUPS` is seeded as enabled on all three
//!    identifiers -- standing in for the operator's own manual work in
//!    the portal). Same inputs, same acknowledgements withheld. A fresh
//!    `plan` shows every previously-blocked gate `Compute` and the three
//!    profile nodes and their `doppler.secret.set` nodes `Create` --
//!    **and nothing else changes**: every node that already ran in step 1
//!    reads `Unchanged`/`Computed`. The four acknowledgement leaves are
//!    still `Blocked`, since no API and no seeded state can satisfy them.
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
    Action, Approval, InputName, NodeStatus, PrincipalId, RecordingObserver, Timestamp, TypeName,
    TypeRef, Value, apply, check, plan,
};
use willikins_providers_fake::FakeState;
use willikins_types::{BuildkiteClusterName, DomainType, GitHubRepo, RepoVisibility};

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
const CLUSTER: &str = "ci-macos-apple-silicon";
const BUILDKITE_ORG: &str = "bande-a-bonnot";

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
    inputs.insert(
        InputName::parse("platform").unwrap(),
        scalar("AppleBundleIdPlatform", "IOS"),
    );
    inputs.insert(
        InputName::parse("data_protection").unwrap(),
        scalar(
            "AppleCapabilitySetting",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ),
    );
    inputs.insert(
        InputName::parse("certificate_type").unwrap(),
        scalar("AppleCertificateType", "DISTRIBUTION"),
    );
    inputs.insert(
        InputName::parse("serial_number").unwrap(),
        scalar("AppleCertificateSerial", "7B3F2A9C1D4E5F607182930A1B2C3D4E"),
    );
    inputs.insert(
        InputName::parse("org").unwrap(),
        scalar("GitHubOrg", "Bande-a-Bonnot"),
    );
    inputs.insert(
        InputName::parse("slug").unwrap(),
        scalar("ProjectSlug", "walter"),
    );
    inputs.insert(
        InputName::parse("monorepo").unwrap(),
        scalar("GitHubRepo", MONOREPO),
    );
    inputs.insert(
        InputName::parse("buildkite_org").unwrap(),
        scalar("BuildkiteOrg", BUILDKITE_ORG),
    );
    inputs.insert(
        InputName::parse("cluster").unwrap(),
        scalar("BuildkiteClusterName", CLUSTER),
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
    for name in [
        "m3_repo_files_done",
        "m5_apns_key_done",
        "m6_ci_doppler_access_done",
        "m7_bootstrap_done",
    ] {
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
/// `workflows/github-repo-token-from-doppler.yaml`), the monorepo (for
/// `github.repo.get`), the Buildkite cluster (for `buildkite.cluster.get`),
/// and the distribution certificate (for `appstore.certificate.get`). No
/// bundle id, no app record, no capability is seeded -- those are exactly
/// what run 1 must create or find blocked.
fn seeded_state() -> Arc<Mutex<FakeState>> {
    let config = willikins_types::DopplerConfig::parse("appstore-connect/deploy_ios").unwrap();
    let github_config = willikins_types::DopplerConfig::parse("github/bande-a-bonnot").unwrap();
    let json = serde_json::json!({
        "doppler_values": {
            format!("{config}#APP_STORE_CONNECT_API_KEY_ISSUER_ID"): "57246542-96fe-1a63-e053-0824d011072a",
            format!("{config}#APP_STORE_CONNECT_API_KEY_ID"): "2X9R4HXF34",
        },
        "doppler_secrets": {
            format!("{config}#APP_STORE_CONNECT_API_KEY_BASE64"): "VGhpcyBpcyBhbiBleGFtcGxlIGtleSBmb3IgdGVzdHMgb25seS4KLS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1JR0hBZ0VBTUJNR0J5cUdTTTQ5QWdFR0NDcUdTTTQ5QXdFSEJHMHdhd0lCQVFRZ3ZMNTJyZWtFcWdHcW9XbjkKK1lCa0lRdVFXRU9UaEtxcUlYYnZvbmVuY0FXaFJBTkNBQVRkdC9YZDRjL0NMT0thMmpvRDlHMXBCOTh1d0tOKwpMR0p2SzNoS1RyeFRXbkowR3lRaVAzUm1DdWJ6bCtHUVIvL2g5Y2lGYW1qeU5jSE1qVlUyY0tiQQotLS0tLUVORCBQUklWQVRFIEtFWS0tLS0tCg==",
            format!("{github_config}#GH_CLONE_TOKEN"): "ghp_example",
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

fn approval() -> Approval {
    Approval::Human {
        approver: PrincipalId::parse("operator").unwrap(),
        at: Timestamp::parse("2026-09-29T00:00:00Z").unwrap(),
    }
}

/// No secret value ever reaches JSON output: only its `[REDACTED` marker
/// does. The signing key's own PEM marker, the fake profile tool's own
/// plaintext content prefix, and R4's seeded GitHub token are the
/// concrete secrets this graph carries.
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

    // Exactly the two observed gates plus the four acknowledgement leaves
    // are blocked; nothing else.
    let blocked_nodes: std::collections::BTreeSet<&str> =
        planned1.blocked.iter().map(|b| b.node.as_str()).collect();
    assert_eq!(
        blocked_nodes,
        std::collections::BTreeSet::from([
            "app_record",
            "app_app_groups",
            "nse_app_groups",
            "widgets_app_groups",
            "m3_repo_files",
            "m5_apns_key",
            "m6_ci_doppler_access",
            "m7_bootstrap",
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
    // Run 2: the two observed gates open; the four acknowledgements are
    // still withheld.
    // ------------------------------------------------------------------
    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));

    let blocked_nodes2: std::collections::BTreeSet<&str> =
        planned2.blocked.iter().map(|b| b.node.as_str()).collect();
    assert_eq!(
        blocked_nodes2,
        std::collections::BTreeSet::from([
            "m3_repo_files",
            "m5_apns_key",
            "m6_ci_doppler_access",
            "m7_bootstrap",
        ]),
        "run 2's blocked set: only the four acknowledgement leaves remain"
    );

    for node in [
        "app_record",
        "app_app_groups",
        "nse_app_groups",
        "widgets_app_groups",
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
    assert_eq!(applied2.blocked.len(), 4, "run 2's Applied.blocked");

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
    for (node, input) in [
        ("m3_repo_files", "m3_repo_files_done"),
        ("m5_apns_key", "m5_apns_key_done"),
        ("m6_ci_doppler_access", "m6_ci_doppler_access_done"),
        ("m7_bootstrap", "m7_bootstrap_done"),
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
    // Run 3: the four acknowledgements are supplied. Everything converges.
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
        "app_profile",
        "nse_profile",
        "widgets_profile",
        "app_profile_to_doppler",
        "nse_profile_to_doppler",
        "widgets_profile_to_doppler",
        "doppler",
        "pipeline",
        "m3_repo_files",
        "m5_apns_key",
        "m6_ci_doppler_access",
        "m7_bootstrap",
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
#[test]
fn the_document_checks_cleanly_against_the_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&document(), &catalog).expect("the document checks cleanly");
}
