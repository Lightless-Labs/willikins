//! Adversarial pass 1 over the apply executor (acceptance test 19, first
//! pass; task 9). Recorded in
//! `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`.
//!
//! Everything here attacks `willikins_core::apply` itself: the approval
//! gate, the drift check, what the executor trusts a tool to return, what
//! it does with an observer, and what two concurrent runs against one
//! provider state may do to each other. The journal half of the same pass
//! lives in `willikins-journal`'s `tests/adversarial_pass_1.rs`.
//!
//! Unlike `tests/apply.rs`, these tests drive the real fake catalog
//! (`willikins_providers_fake::catalog`) rather than
//! `common::apply_test_catalog`'s substitute: an attack on the executor
//! must run against the tools a real caller has, not a stand-in.
//!
//! Tests whose name begins `boundary_` pin something core deliberately
//! does *not* do, so the guarantee it depends on is visible in code
//! rather than only in a plan document.

mod common;

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use common::{
    distinctive_token, input, node, port, principal, timestamp, tool_name, ty, workflow_name,
};
use willikins_core::{
    Action, ApplyError, ApplyEvent, ApplyObserver, Approval, Binding, Catalog, Class, DriftKind,
    Ensured, InputSpec, Inputs, Node, NodeStatus, Observation, Outputs, PortSpec, PortType,
    RecordingObserver, SinkToken, Tool, ToolError, ToolSpec, Value, Workflow, apply, check, plan,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::call_key;
use willikins_types::{DomainType, GitHubRepo, RepoVisibility};

/// The bytes of [`common::distinctive_token`] that must never appear in
/// any rendering of a result, an error, or an event.
const TOKEN_MARKER_BYTES: &str = "MARKERMARKERMARKERMARKERMARKERMARKERMARKER";

/// A fake state and the real fake catalog over it, with `next_token`
/// seeded to the distinctive marker.
fn seeded_state_and_catalog() -> (Arc<Mutex<FakeState>>, Catalog) {
    let state = Arc::new(Mutex::new(
        FakeState::new().with_next_token(distinctive_token()),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    (state, catalog)
}

// ---------------------------------------------------------------------
// 1. The approval gate
// ---------------------------------------------------------------------

/// An `Approval::Human` carries no plan id, so core cannot tell an
/// approval granted for *this* plan from one granted for another: it
/// checks only that a human is claimed. Binding an approval to a
/// `plan_id` is the server's job. Closed by `willikins-server`'s
/// `Butler` (task 10a): its `apply(plan_id, principal)` takes no
/// caller-supplied `Approval` at all -- it constructs `Human` only from
/// the *given* `plan_id`'s own journaled `ApprovalGranted` event, so
/// there is no code path through the public API that could hand one
/// plan's approval to another. `crates/willikins-server/tests/acceptance_7_identity.rs`'s
/// `an_approved_irreversible_plan_runs` and `a_rejected_plan_still_refuses_apply`
/// exercise exactly this construction, one plan at a time.
#[test]
fn boundary_an_approval_is_not_bound_to_the_plan_it_approves() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::irreversible_workflow();
    let checked = check(&workflow, &catalog).expect("the irreversible workflow checks");
    assert_eq!(checked.class, Class::Irreversible);
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    // An approval a human granted for some entirely different plan --
    // core has nothing to compare it against.
    let approval = Approval::Human {
        approver: principal("operator"),
        at: timestamp("2026-09-14T09:00:00+00:00"),
    };
    let mut observer = RecordingObserver::new();
    apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &approval,
        &mut observer,
    )
    .expect("core accepts any Human approval; the plan binding is the server's");

    assert!(
        state
            .lock()
            .unwrap()
            .irreversible
            .contains("third-thoughts"),
        "the irreversible node really ran"
    );
}

/// `Approval::Human { at }` is never compared with anything: an approval
/// timestamped in the far future (a clock-skewed or forged approver) runs
/// exactly like one stamped now. The approval and apply *windows* the
/// trust boundaries describe are the server's, measured from journaled
/// events, not from this field. Closed by `willikins-server`'s `Butler`
/// (task 10a): `at` is never a caller-supplied value at all -- it is the
/// `Timestamp` the journal itself stamped on the plan's `ApprovalGranted`
/// entry (`crates/willikins-journal/src/journal.rs`'s
/// `ApprovalState::Granted::at`), read from the journal's own shared
/// clock, so there is no way to forge it through the public API. Its
/// window checks (`crates/willikins-server/src/butler.rs`'s
/// `elapsed_since`) are what `crates/willikins-server/tests/acceptance_8_plan_identity.rs`'s
/// four window tests pin.
#[test]
fn boundary_an_approval_timestamped_in_the_future_still_runs() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::irreversible_workflow();
    let checked = check(&workflow, &catalog).expect("the irreversible workflow checks");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    let approval = Approval::Human {
        approver: principal("operator"),
        at: timestamp("2999-01-01T00:00:00+00:00"),
    };
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &approval,
        &mut observer,
    )
    .expect("core does not judge an approval's timestamp");
    assert!(!applied.nodes.is_empty());
    assert!(
        state
            .lock()
            .unwrap()
            .irreversible
            .contains("third-thoughts"),
        "the irreversible node really ran"
    );
}

// ---------------------------------------------------------------------
// 2. The drift check
// ---------------------------------------------------------------------

/// A `for_each` plan whose approved side carries the same instance twice
/// -- a duplicated line in whatever rebuilt it -- is instance drift, not a
/// silently doubled run of that instance.
#[test]
fn a_duplicated_for_each_instance_in_the_approved_plan_is_instance_drift() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks");
    let inputs = common::new_rust_service_inputs();
    let mut approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    let first_config = approved
        .nodes
        .iter()
        .position(|planned| planned.name.as_str() == "configs")
        .expect("the fixture has a configs node");
    let duplicate = approved.nodes[first_config].clone();
    approved.nodes.insert(first_config + 1, duplicate);

    {
        let mut locked = state.lock().unwrap();
        locked.read_calls.clear();
        locked.ensure_calls.clear();
    }

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a duplicated instance must be refused");
    let ApplyError::Drift { node: name, .. } = &err else {
        panic!("expected Drift, got {err:?}");
    };
    assert_eq!(name.as_str(), "configs");
    assert!(observer.events.is_empty(), "nothing ran");
    assert!(
        state.lock().unwrap().ensure_calls.is_empty(),
        "drift is checked before any provider write"
    );
}

/// Two plans of two *different* workflows can fingerprint identically
/// when their nodes' names, actions and outputs agree and only an input
/// differs -- here the literal Actions secret name each writes to, which
/// `Plan::fingerprint` does not cover (`github.actions_secret.ensure`
/// declares no outputs at all). Core runs it: the executor executes
/// `checked`, and the approved plan is only compared with a fresh plan of
/// that same `checked`. Nothing here is a defect in `apply`; it is why
/// plan identity -- `(workflow name, document sha256)` -- is the server's
/// (`todos/2026-09-14-plan-identity-must-cover-inputs.md`, task 10a).
#[test]
fn boundary_an_approved_plan_of_another_workflow_with_an_equal_fingerprint_runs() {
    let (state, catalog) = seeded_state_and_catalog();

    let secret_workflow = |name: &str, secret_name: &str| {
        Workflow::new(workflow_name(name))
            .input(input("repo"), InputSpec::new(ty("GitHubRepo")))
            .node(
                node("secret"),
                Node::new(tool_name("doppler.secret.get"))
                    .port(
                        port("config"),
                        Binding::Literal("third-thoughts/prd".to_string()),
                    )
                    .port(port("name"), Binding::Literal(secret_name.to_string())),
            )
            .node(
                node("ci_secret"),
                Node::new(tool_name("github.actions_secret.ensure"))
                    .port(port("repo"), Binding::Input(input("repo")))
                    .port(port("name"), Binding::Literal("DOPPLER_TOKEN".to_string()))
                    .port(
                        port("value"),
                        Binding::Step {
                            node: node("secret"),
                            port: port("value"),
                        },
                    ),
            )
    };

    {
        let mut locked = state.lock().unwrap();
        let config = willikins_types::DopplerConfig::parse("third-thoughts/prd").unwrap();
        for (name, value) in [
            ("DATABASE_URL", "approved-secret-bytes"),
            ("ADMIN_PASSWORD", "swapped-secret-bytes"),
        ] {
            locked.doppler_secrets.insert(
                willikins_providers_fake::state::doppler_secret_key(
                    &config,
                    &willikins_types::SecretName::parse(name).unwrap(),
                ),
                willikins_types::DopplerSecretValue::parse(value).unwrap(),
            );
        }
    }

    let mut inputs = IndexMap::new();
    inputs.insert(
        input("repo"),
        Value::known(GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()),
    );

    let approved_document = secret_workflow("plan-identity-a", "DATABASE_URL");
    let swapped_document = secret_workflow("plan-identity-b", "ADMIN_PASSWORD");
    let approved_checked = check(&approved_document, &catalog).expect("document A checks");
    let swapped_checked = check(&swapped_document, &catalog).expect("document B checks");

    let approved = plan(&approved_checked, &inputs, &catalog).expect("document A plans");
    let swapped = plan(&swapped_checked, &inputs, &catalog).expect("document B plans");
    assert_eq!(
        approved.fingerprint(),
        swapped.fingerprint(),
        "the two documents' plans are indistinguishable by fingerprint alone"
    );
    assert_ne!(
        approved.workflow, swapped.workflow,
        "and core never compares the workflow name either"
    );

    let mut observer = RecordingObserver::new();
    apply(
        &swapped_checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("core runs document B under document A's approval");

    assert!(
        state
            .lock()
            .unwrap()
            .github_actions_secrets
            .contains("lightless-labs/third-thoughts#DOPPLER_TOKEN"),
        "document B's secret really was written"
    );
}

/// The approved plan's own per-node `inputs` are never what runs: the
/// executor walks the *fresh* plan and re-resolves every upstream-bound
/// port against this run's own results, so tampering with the approved
/// plan's inputs changes nothing about the run. (What a human read in the
/// approved plan can therefore differ from what runs, which is the other
/// half of the same plan-identity gap above.)
#[test]
fn boundary_tampering_with_the_approved_plans_inputs_changes_nothing_that_runs() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks");
    let inputs = common::new_rust_service_inputs();
    let mut approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    for planned in &mut approved.nodes {
        if planned.name.as_str() == "repo" {
            planned.inputs.insert(
                port("repo"),
                Value::known(GitHubRepo::parse("attacker/elsewhere").unwrap()),
            );
        }
    }

    let mut observer = RecordingObserver::new();
    apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("the tampered inputs are not even looked at");

    let locked = state.lock().unwrap();
    assert!(
        locked
            .github_repos
            .contains_key("lightless-labs/third-thoughts"),
        "the checked workflow's own repository is what was created"
    );
    assert!(
        !locked.github_repos.contains_key("attacker/elsewhere"),
        "the approved plan's tampered input reached nothing"
    );
}

// ---------------------------------------------------------------------
// 3. What the executor trusts a tool to return
// ---------------------------------------------------------------------

/// A tool whose `ensure` answers with ports it never declared, omits one
/// it did, and returns a *secret* value on a port it declared non-secret.
struct LyingEnsureTool {
    spec: ToolSpec,
}

impl LyingEnsureTool {
    fn new(name: &str) -> Self {
        let mut outputs = IndexMap::new();
        outputs.insert(port("org"), ty("GitHubOrg"));
        outputs.insert(port("label"), ty("Text"));
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: lies about its own outputs.".to_string(),
                inputs: IndexMap::new(),
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for LyingEnsureTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let mut outputs = Outputs::new();
        // A secret value on a port declared `GitHubOrg`.
        outputs.insert(port("org"), Value::known(distinctive_token()));
        // A port this tool never declared.
        outputs.insert(port("ghost"), Value::known(distinctive_token()));
        // `label`, which it did declare, is simply absent.
        Ok(Ensured {
            outputs,
            changed: true,
        })
    }
}

/// A tool that requires one known `GitHubOrg` input and records what it
/// was handed, so a test can see what actually reached a sink.
struct RecordingSinkTool {
    spec: ToolSpec,
    seen: Arc<Mutex<Vec<Inputs>>>,
}

impl RecordingSinkTool {
    fn new(name: &str, seen: Arc<Mutex<Vec<Inputs>>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("value"),
            PortSpec {
                ty: PortType::Exact(ty("GitHubOrg")),
                required: true,
                derived_only: false,
            },
        );
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: records the inputs it was handed.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
            seen,
        }
    }
}

impl Tool for RecordingSinkTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        self.seen.lock().unwrap().push(inputs.clone());
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}

/// The executor keeps exactly the ports a tool's spec declares: an
/// undeclared port is dropped, a declared one the tool forgot becomes
/// `Unknown`. Neither reaches a downstream node or a result.
#[test]
fn undeclared_outputs_are_dropped_and_forgotten_ones_become_unknown() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(LyingEnsureTool::new("test.lies")))
        .unwrap();

    let workflow = Workflow::new(workflow_name("lying-tool"))
        .node(node("liar"), Node::new(tool_name("test.lies")));
    let checked = check(&workflow, &catalog).expect("the workflow checks");
    let inputs = IndexMap::new();
    let approved = plan(&checked, &inputs, &catalog).expect("plans");

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("runs");

    let outputs = &applied.nodes[0].outputs;
    assert_eq!(
        outputs
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>(),
        vec!["org".to_string(), "label".to_string()],
        "exactly the declared ports, in declaration order"
    );
    assert!(
        !outputs
            .get(&port("label"))
            .expect("the declared port is present")
            .is_known(),
        "a declared port the tool omitted is Unknown"
    );
    let json = serde_json::to_string(&applied).expect("Applied serializes");
    assert!(
        !json.contains("ghost"),
        "the undeclared port is gone: {json}"
    );
}

/// A tool that returns a *secret* value on a port it declared non-secret
/// breaks the static taint rule at run time -- `check` typed that port
/// from the spec, so it let it bind to a non-secret sink, and the
/// executor hands the sink the value the tool actually returned. No byte
/// escapes (a `Value`'s redaction travels with the value, not with the
/// port it sits in), but the guarantee "a secret output may only bind to
/// a secret-accepting input" holds only as far as a tool tells the truth
/// about its own output types. Neither `plan` nor `apply` re-checks a
/// returned value against the declared port type; every tool in the
/// workspace is our own code, which is why this is pinned rather than
/// fixed here (see the pass-1 note's "Handed to pass 2").
#[test]
fn boundary_a_secret_returned_on_a_non_secret_port_flows_on_but_never_prints() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(LyingEnsureTool::new("test.lies")))
        .unwrap();
    catalog
        .insert(Arc::new(RecordingSinkTool::new(
            "test.sink",
            Arc::clone(&seen),
        )))
        .unwrap();

    let workflow = Workflow::new(workflow_name("lying-tool-sink"))
        .node(node("liar"), Node::new(tool_name("test.lies")))
        .node(
            node("sink"),
            Node::new(tool_name("test.sink")).port(
                port("value"),
                Binding::Step {
                    node: node("liar"),
                    port: port("org"),
                },
            ),
        );
    let checked = check(&workflow, &catalog).expect("check types `value` from the spec: GitHubOrg");
    let inputs = IndexMap::new();
    let approved = plan(&checked, &inputs, &catalog).expect("plans");

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("runs");

    let handed = seen.lock().unwrap();
    assert_eq!(handed.len(), 1);
    assert!(
        handed[0]
            .get(&port("value"))
            .expect("the sink's port was bound")
            .is_secret(),
        "the sink was handed a secret value on a non-secret port"
    );

    // ... and still nothing prints it.
    for rendering in [
        serde_json::to_string(&applied).expect("Applied serializes"),
        format!("{applied:?}"),
        format!("{:?}", observer.events),
        serde_json::to_string(&observer.events).expect("events serialize"),
        format!("{:?}", handed[0]),
    ] {
        assert!(
            !rendering.contains(TOKEN_MARKER_BYTES),
            "the secret's bytes leaked: {rendering}"
        );
    }
}

// ---------------------------------------------------------------------
// 4. Observers
// ---------------------------------------------------------------------

/// An observer that panics on the nth event it sees.
struct PanickingObserver {
    seen: usize,
    panic_at: usize,
}

impl ApplyObserver for PanickingObserver {
    fn on(&mut self, _event: ApplyEvent) {
        self.seen += 1;
        assert_ne!(
            self.seen, self.panic_at,
            "the observer panicked on this event"
        );
    }
}

/// `apply` does not isolate its observer: a panic in `ApplyObserver::on`
/// unwinds straight out of the run, leaving whatever the run had already
/// written in place and every later node unattempted. The executor makes
/// no attempt to catch it -- a journal that cannot record is a reason to
/// stop, not to keep writing to providers -- but a caller that survives
/// the panic must treat the run as unfinished, not as refused. Pinned so
/// that a future `catch_unwind` (or the lack of one) is a deliberate
/// choice.
#[test]
fn a_panicking_observer_unwinds_out_of_apply_leaving_earlier_writes_in_place() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    // Event 5 is the third instance's `NodeStarted` (`names` and `repo`
    // contribute two events each), so `repo` has already been created.
    let mut observer = PanickingObserver {
        seen: 0,
        panic_at: 5,
    };
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply(
            &checked,
            &inputs,
            &catalog,
            &approved,
            &Approval::Auto,
            &mut observer,
        )
    }));
    std::panic::set_hook(previous_hook);
    assert!(outcome.is_err(), "the panic unwound out of apply");

    let locked = state.lock().unwrap();
    assert!(
        locked
            .github_repos
            .contains_key("lightless-labs/third-thoughts"),
        "the write made before the panic is still there"
    );
    assert!(
        locked.doppler_projects.is_empty(),
        "and nothing after the panic ran"
    );
    assert!(
        locked.doppler_service_tokens.is_empty(),
        "no token was minted before the panic"
    );
}

// ---------------------------------------------------------------------
// 5. Two applies at once
// ---------------------------------------------------------------------

/// Core provides no mutual exclusion between two `apply` calls: two
/// threads may run the same approved plan against one provider state at
/// the same time. What holds is what each tool's own `ensure` holds
/// (every fake tool reads and writes under one lock, so no resource is
/// created twice and the state is never left inconsistent); what does
/// *not* hold is that a plan's outputs describe the state afterwards --
/// the two runs mint two tokens and the last write wins. Closed by
/// `willikins-server`'s `Butler` (task 10a): a `Mutex<Option<RunId>>`
/// held across each `apply` call's own synchronous checks (never across
/// the run itself) answers a second `apply` with `RunInProgress` before
/// a second `willikins_core::apply` is ever called, so this interleaving
/// is unreachable through the public API. Pinned by
/// `crates/willikins-server/tests/acceptance_8_plan_identity.rs`'s
/// `a_second_apply_while_a_run_is_in_progress_is_refused`.
#[test]
fn boundary_two_threads_applying_one_plan_share_state_with_no_mutual_exclusion() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let outcomes: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let checked = &checked;
                let inputs = &inputs;
                let catalog = &catalog;
                let approved = &approved;
                scope.spawn(move || {
                    barrier.wait();
                    let mut observer = RecordingObserver::new();
                    let result = apply(
                        checked,
                        inputs,
                        catalog,
                        approved,
                        &Approval::Auto,
                        &mut observer,
                    );
                    (
                        result.is_ok(),
                        result.map_or_else(|err| format!("{err:?}"), |ok| format!("{ok:?}")),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("no thread panicked"))
            .collect()
    });

    for (_, rendering) in &outcomes {
        assert!(
            !rendering.contains(TOKEN_MARKER_BYTES),
            "a concurrent run leaked the marker: {rendering}"
        );
    }
    assert!(
        outcomes.iter().any(|(ok, _)| *ok),
        "at least one of the two runs finished: {outcomes:?}"
    );

    let locked = state.lock().unwrap();
    assert_eq!(
        locked.github_repos.len(),
        1,
        "the resource was created once, whatever the interleaving"
    );
    assert_eq!(locked.doppler_projects.len(), 1);
    assert_eq!(
        locked.doppler_service_tokens.len(),
        1,
        "one token *record*, even though each run minted its own value"
    );
    // How far the *second* run gets is interleaving-dependent, which is
    // exactly the point: it may drift (the first run created the
    // repository between the approval and this re-plan), it may be
    // blocked by `UnknownInput` (the first run minted the token, so this
    // one cannot re-read it), or it may write the same sink a second time
    // -- last write wins, and core never notices either way.
    assert!(
        *locked
            .ensure_calls
            .get(&call_key(
                "github.actions_secret.ensure",
                "lightless-labs/third-thoughts#DOPPLER_TOKEN"
            ))
            .unwrap_or(&0)
            >= 1,
        "at least one run reached the sink"
    );
}

/// Two *different* plans touching one resource are caught by the
/// executor's own rule 2 re-plan, not by anything about concurrency: the
/// second plan's re-plan observes what the first run left behind and
/// refuses before writing.
#[test]
fn a_second_plan_touching_the_same_resource_is_refused_by_the_re_plan() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = Workflow::new(workflow_name("one-repo"))
        .input(input("visibility"), InputSpec::new(ty("RepoVisibility")))
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(
                    port("repo"),
                    Binding::Literal("lightless-labs/third-thoughts".to_string()),
                )
                .port(port("visibility"), Binding::Input(input("visibility"))),
        );
    let checked = check(&workflow, &catalog).expect("the workflow checks");

    let with_visibility = |visibility: RepoVisibility| {
        let mut inputs = IndexMap::new();
        inputs.insert(input("visibility"), Value::known(visibility));
        inputs
    };
    let private = with_visibility(RepoVisibility::Private);
    let public = with_visibility(RepoVisibility::Public);

    let private_plan = plan(&checked, &private, &catalog).expect("plans");
    let public_plan = plan(&checked, &public, &catalog).expect("plans");
    assert_eq!(private_plan.nodes[0].action, Action::Create);
    assert_eq!(public_plan.nodes[0].action, Action::Create);

    let mut observer = RecordingObserver::new();
    apply(
        &checked,
        &private,
        &catalog,
        &private_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("the first plan runs");

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &public,
        &catalog,
        &public_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the second plan must be refused, not reconciled");
    let ApplyError::Plan { error } = &err else {
        panic!("expected a re-plan refusal, got {err:?}");
    };
    assert!(
        format!("{error}").contains("visibility"),
        "the refusal names the attribute: {error}"
    );
    assert!(observer.events.is_empty(), "nothing ran");
    assert_eq!(
        *state
            .lock()
            .unwrap()
            .ensure_calls
            .get(&call_key(
                "github.repo.ensure",
                "lightless-labs/third-thoughts"
            ))
            .expect("the first run called ensure"),
        1,
        "the second apply never reached ensure"
    );
    assert_eq!(
        state.lock().unwrap().github_repos["lightless-labs/third-thoughts"].visibility,
        RepoVisibility::Private,
        "and the repository still has the visibility the first plan gave it"
    );
}

/// A drifted plan is refused with no partial `Applied`, and a mid-run
/// failure carries one: the two shapes must not be confused, since a
/// caller uses the presence of a partial result to decide whether a run
/// happened at all.
#[test]
fn a_drift_refusal_carries_no_partial_result_and_a_tool_failure_does() {
    let (state, catalog) = seeded_state_and_catalog();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");

    // Drift: seed the repository so the fresh plan says NoOp.
    state.lock().unwrap().github_repos.insert(
        "lightless-labs/third-thoughts".to_string(),
        willikins_providers_fake::state::GitHubRepoRecord {
            visibility: RepoVisibility::Private,
            ours: true,
        },
    );
    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the seeded repository is drift");
    match &err {
        ApplyError::Drift { kind, .. } => {
            assert!(matches!(
                kind.as_ref(),
                DriftKind::Action {
                    planned: Action::Create,
                    observed: Action::NoOp
                }
            ));
        }
        other => panic!("expected Drift, got {other:?}"),
    }

    // A tool failure, by contrast, carries its partial result.
    let approved = plan(&checked, &inputs, &catalog).expect("re-plans");
    state
        .lock()
        .unwrap()
        .fail_ensure_once
        .push(call_key("doppler.project.ensure", "third-thoughts"));
    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the injected failure stops the run");
    let ApplyError::Tool { applied, .. } = &err else {
        panic!("expected Tool, got {err:?}");
    };
    assert!(
        applied
            .nodes
            .iter()
            .any(|node| matches!(node.status, NodeStatus::Failed { .. })),
        "the partial result names the failed instance"
    );
    assert!(
        applied
            .nodes
            .iter()
            .any(|node| matches!(node.status, NodeStatus::NotRun)),
        "and every later instance as NotRun"
    );
}
