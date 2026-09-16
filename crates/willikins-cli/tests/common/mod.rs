//! Task 14: everything acceptance test 18 asserts that is *not* a
//! provider.
//!
//! Two test targets include this module, and the whole point is that
//! they share it:
//!
//! - `tests/smoke_parity.rs` -- ungated, part of the workspace gate,
//!   runs the same four `willikins` invocations against the **fake**
//!   catalog;
//! - `tests/live_smoke.rs` -- behind this crate's `live-tests` feature,
//!   `#[ignore]`d, inert without `WILLIKINS_LIVE_TESTS=1`, runs them
//!   against the **real** GitHub and Doppler.
//!
//! Every expectation table, every JSON reader and the secret sweep live
//! here, so the live run's first execution differs from a run the gate
//! has already made hundreds of times in exactly one respect: which
//! catalog answered. A shape that drifts breaks the gate, not the live
//! run.
//!
//! Nothing here string-matches a rendered line: the CLI's `--json`
//! documents are parsed with `serde_json` and read by field.

// The two targets use different subsets of this module -- `node_output`
// and `plan_response`, for instance, are read by `smoke_parity.rs` and
// not by `live_smoke.rs` -- and an item one target leaves alone is not
// a defect in the other.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

/// The inputs the coordinator fixed for the smoke run, and the names
/// `naming::v1` derives from them. Written out rather than re-derived so
/// a change to `naming::v1` (which is frozen) breaks this test loudly
/// instead of quietly agreeing with itself.
pub const SLUG: &str = "willikins-smoke";
/// The sandbox GitHub organization. Not read from
/// `WILLIKINS_SANDBOX_GITHUB_ORG`: acceptance test 18 names one org, and
/// a run pointed at another one by a stale variable is a run whose
/// teardown would look in the wrong place.
pub const ORG: &str = "Willikins-Test";
/// `naming::v1::github_repo(ORG, SLUG)`.
pub const REPO: &str = "Willikins-Test/willikins-smoke";
/// `naming::v1::doppler_project(SLUG)`.
pub const PROJECT: &str = "willikins-smoke";
/// The principal every invocation runs as.
pub const PRINCIPAL: &str = "smoke-operator";
/// The `repo_url` output the positive fixture must produce.
pub const REPO_URL_PREFIX: &str = "https://github.com/Willikins-Test/willikins-smoke";

/// One expected node instance of a run record: the node name, its
/// `for_each` instance key, and the `kind` its `status` must carry.
pub struct ExpectedNode {
    /// The step's name in the document.
    pub node: &'static str,
    /// Its `for_each` instance key, `None` for a scalar node.
    pub instance: Option<&'static str>,
    /// `NodeStatus`'s serde tag: `computed`, `created`, `unchanged`,
    /// `converged`, `failed` or `not_run`.
    pub status: &'static str,
}

/// The first apply of `workflows/new-rust-service.yaml` against an
/// account where none of it exists yet.
///
/// The three `configs` instances are **`unchanged`, not `created`**, and
/// that is not a weakness in the test. Doppler creates the three root
/// configs `dev`, `stg` and `prd` together with the project itself, so
/// by the time `doppler.config.ensure` runs, its `ensure()` re-observes
/// each one `Present` and answers `changed: false`
/// (`crates/willikins-providers-doppler/src/tools/config_ensure.rs`),
/// which the executor maps to `NodeStatus::Unchanged`
/// (`crates/willikins-core/src/apply.rs`). The fake provider seeds the
/// same three configs with the project for the same reason, so both
/// catalogs agree. Acceptance test 18's wording "every node `Created`"
/// is wrong for those three; the coordinator amends the plan.
pub const FIRST_APPLY: &[ExpectedNode] = &[
    ExpectedNode {
        node: "names",
        instance: None,
        status: "computed",
    },
    ExpectedNode {
        node: "repo",
        instance: None,
        status: "created",
    },
    ExpectedNode {
        node: "doppler",
        instance: None,
        status: "created",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("dev"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("stg"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("prd"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "token",
        instance: None,
        status: "created",
    },
    ExpectedNode {
        node: "ci_secret",
        instance: None,
        status: "created",
    },
];

/// The second, identical apply: the convergence claim.
///
/// `ci_secret` is **`converged`**, which is the no-op outcome, not a
/// loosened assertion. Its `value` input is the minted service token,
/// whose `Value` is `Unknown` on a re-read (Doppler never re-issues a
/// token's bytes), and the planned action for it is `NoOp`, so the
/// executor called nothing at all -- `NodeStatus::Converged`'s exact
/// definition. Anything else here (`created`, say) would mean the sink
/// wrote again, and the assertion prints the status it found rather than
/// accepting it.
pub const SECOND_APPLY: &[ExpectedNode] = &[
    ExpectedNode {
        node: "names",
        instance: None,
        status: "computed",
    },
    ExpectedNode {
        node: "repo",
        instance: None,
        status: "unchanged",
    },
    ExpectedNode {
        node: "doppler",
        instance: None,
        status: "unchanged",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("dev"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("stg"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "configs",
        instance: Some("prd"),
        status: "unchanged",
    },
    ExpectedNode {
        node: "token",
        instance: None,
        status: "unchanged",
    },
    ExpectedNode {
        node: "ci_secret",
        instance: None,
        status: "converged",
    },
];

/// `workflows/rotate-service-token.yaml` applied with approval:
/// `doppler.service_token.rotate` always revokes and re-mints (it reads
/// `Absent` by construction), so the token is `created` and the sink
/// that stores it writes again.
pub const ROTATION: &[ExpectedNode] = &[
    ExpectedNode {
        node: "config",
        instance: None,
        status: "unchanged",
    },
    ExpectedNode {
        node: "token",
        instance: None,
        status: "created",
    },
    ExpectedNode {
        node: "ci_secret",
        instance: None,
        status: "created",
    },
];

/// The token prefixes a real credential of either provider carries --
/// `willikins_providers_github::CREDENTIAL_PATTERN` and
/// `willikins_providers_doppler::CREDENTIAL_PATTERN` pin the first four
/// -- plus `dp.st.`, the prefix Doppler puts on a **minted service
/// token**, which is the one secret this workflow really creates.
pub const CREDENTIAL_PREFIXES: &[&str] = &["github_pat_", "ghp_", "dp.sa.", "dp.pt.", "dp.st."];

/// The workspace root, from this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// A workspace-relative path, as a string the CLI can take.
pub fn workspace_path(relative: &str) -> String {
    workspace_root()
        .join(relative)
        .to_str()
        .expect("the workspace path is valid UTF-8")
        .to_string()
}

/// One finished `willikins` invocation: its raw streams and, for
/// `--json` runs, every top-level JSON document `stdout` carried.
pub struct Invocation {
    /// The exit status.
    pub output: Output,
    /// Everything the child wrote to stdout.
    pub stdout: String,
    /// Everything the child wrote to stderr.
    pub stderr: String,
    /// `stdout` split into top-level JSON documents (see
    /// [`json_documents`]).
    pub docs: Vec<Value>,
}

impl Invocation {
    /// The process exit code.
    pub fn code(&self) -> i32 {
        self.output
            .status
            .code()
            .expect("process was not signalled")
    }

    /// The `RunRecord` document: the last one carrying a `run_id`.
    pub fn run_record(&self) -> &Value {
        self.docs
            .iter()
            .rev()
            .find(|doc| doc.get("run_id").is_some())
            .unwrap_or_else(|| panic!("no run record in: {}", self.stdout))
    }

    /// The `PlanResponse` document: the first one carrying a `plan_id`
    /// and a `plan`.
    pub fn plan_response(&self) -> &Value {
        self.docs
            .iter()
            .find(|doc| doc.get("plan_id").is_some() && doc.get("plan").is_some())
            .unwrap_or_else(|| panic!("no plan response in: {}", self.stdout))
    }

    /// This run's id.
    pub fn run_id(&self) -> String {
        self.run_record()["run_id"]
            .as_str()
            .expect("a run record's run_id is a string")
            .to_string()
    }
}

/// Run the built `willikins` binary with `args`, plus any extra
/// environment variables.
///
/// **No `env_clear()`, on purpose.** The live smoke test needs the two
/// provider credentials in the child, and the only way to put them there
/// without reading their bytes into this process is to let the child
/// inherit the environment its parent was started with -- "by name
/// only", the coordinator's own phrasing. The parity test reaches no
/// provider at all (no `--live`), so inheriting costs it nothing.
pub fn willikins(args: &[&str], vars: &[(&str, &str)]) -> Invocation {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins"));
    command.args(args).current_dir(workspace_root());
    for (name, value) in vars {
        command.env(name, value);
    }
    let output = command
        .output()
        .expect("failed to run the willikins binary");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let docs = json_documents(&stdout);
    Invocation {
        output,
        stdout,
        stderr,
        docs,
    }
}

/// Every top-level JSON value concatenated in `text`, in order.
///
/// `apply --json` prints the `PlanResponse` and then the `RunRecord` (or
/// a refusal) as two separate pretty-printed documents on stdout, not
/// one envelope -- the same reader `tests/apply_and_journal.rs` uses.
/// Text that is not JSON at all yields an empty list rather than
/// panicking, so a refusal printed on stderr still reaches an assertion
/// that can name it.
pub fn json_documents(text: &str) -> Vec<Value> {
    let mut docs = Vec::new();
    for next in serde_json::Deserializer::from_str(text).into_iter::<Value>() {
        let Ok(doc) = next else { break };
        docs.push(doc);
    }
    docs
}

/// Assert that `record`'s `nodes` are exactly `expected`, in order, each
/// with the `status.kind` the table names. `label` names the step in
/// every failure message.
pub fn assert_nodes(record: &Value, expected: &[ExpectedNode], label: &str) {
    let nodes = record["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("{label}: the run record has no `nodes` array: {record}"));
    let found: Vec<String> = nodes.iter().map(describe_node).collect();
    assert_eq!(
        nodes.len(),
        expected.len(),
        "{label}: expected {} node instances, found {}: {found:?}",
        expected.len(),
        nodes.len()
    );
    for (node, want) in nodes.iter().zip(expected) {
        assert_eq!(
            node["node"].as_str(),
            Some(want.node),
            "{label}: node order differs: {found:?}"
        );
        assert_eq!(
            node["instance"].as_str(),
            want.instance,
            "{label}: `{}`'s instance key differs: {found:?}",
            want.node
        );
        assert_eq!(
            node["status"]["kind"].as_str(),
            Some(want.status),
            "{label}: `{}` is {}, expected `{}`",
            describe_node(node),
            node["status"],
            want.status
        );
    }
}

/// `node[instance]=status`, for a failure message.
fn describe_node(node: &Value) -> String {
    let name = node["node"].as_str().unwrap_or("?");
    let status = node["status"]["kind"].as_str().unwrap_or("?");
    match node["instance"].as_str() {
        Some(instance) => format!("{name}[{instance}]={status}"),
        None => format!("{name}={status}"),
    }
}

/// One node instance's named output, as a JSON `Value` object (`type`,
/// `list`, `state`, and `value`/`redacted` when known).
pub fn node_output<'a>(record: &'a Value, node: &str, port: &str) -> &'a Value {
    let nodes = record["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("the run record has no `nodes` array: {record}"));
    let Some(entry) = nodes
        .iter()
        .find(|entry| entry["node"].as_str() == Some(node))
    else {
        panic!("no node `{node}` in: {record}")
    };
    &entry["outputs"][port]
}

/// Assert `record`'s `repo_url` workflow output is present, known, and
/// starts with [`REPO_URL_PREFIX`].
pub fn assert_repo_url(record: &Value, label: &str) {
    let url = &record["outputs"]["repo_url"];
    assert_eq!(
        url["state"].as_str(),
        Some("known"),
        "{label}: repo_url is not known: {url}"
    );
    let value = url["value"]
        .as_str()
        .unwrap_or_else(|| panic!("{label}: repo_url carries no string value: {url}"));
    assert!(
        value.starts_with(REPO_URL_PREFIX),
        "{label}: repo_url is `{value}`, expected it to start with `{REPO_URL_PREFIX}`"
    );
}

/// Assert the minted service token's `Value` reads `unknown` -- Doppler
/// cannot re-read a token, so a second observation carries no bytes and
/// therefore no `value` key at all.
pub fn assert_token_unknown(record: &Value, label: &str) {
    let token = node_output(record, "token", "token");
    assert_eq!(
        token["state"].as_str(),
        Some("unknown"),
        "{label}: the token output is not unknown: {token}"
    );
    assert!(
        token.get("value").is_none(),
        "{label}: an unknown value must carry no `value` key: {token}"
    );
}

/// Assert no credential byte reached any of `streams` (each a
/// `(name, text)` pair).
///
/// Two sweeps, and the second is the reason this helper is not simply a
/// prefix check:
///
/// 1. every prefix in [`CREDENTIAL_PREFIXES`] -- which catches any
///    token-shaped string at all, including the `dp.st.` service token
///    this workflow mints and whose bytes really do pass through the
///    executor;
/// 2. the literal value of `WILLIKINS_GITHUB_TOKEN` and
///    `WILLIKINS_DOPPLER_TOKEN`, when they are set.
///
/// The second sweep deliberately departs from the rule the two provider
/// write cycles keep (they never call `std::env::var` on a credential,
/// so no plaintext copy is ever made): the coordinator asked for the
/// values themselves to be swept. [`contains_credential_value`] narrows
/// the departure to the smallest possible scope -- it reads the value,
/// answers a `bool`, and drops it. The value is never stored, never
/// returned, and never named in an assertion message: a failure says
/// *which variable's* value leaked, never the value.
pub fn assert_no_credential_bytes(label: &str, streams: &[(&str, &str)]) {
    for (stream, text) in streams {
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "{label}: {stream} carries the token-shaped prefix `{prefix}`"
            );
        }
        for name in ["WILLIKINS_GITHUB_TOKEN", "WILLIKINS_DOPPLER_TOKEN"] {
            assert!(
                !contains_credential_value(text, name),
                "{label}: {stream} carries the value of {name}"
            );
        }
    }
}

/// Whether `haystack` contains the current value of the environment
/// variable `name`. Answers a `bool` and nothing else; see
/// [`assert_no_credential_bytes`] for why this is the only place in this
/// module that reads a credential at all.
fn contains_credential_value(haystack: &str, name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.is_empty() && haystack.contains(&value))
}

/// The `--input` pairs the positive fixture takes.
pub fn positive_inputs() -> Vec<String> {
    vec![
        "--input".to_string(),
        format!("slug={SLUG}"),
        "--input".to_string(),
        format!("org={ORG}"),
    ]
}

/// The `--input` pairs the rotation workflow takes.
pub fn rotation_inputs() -> Vec<String> {
    vec![
        "--input".to_string(),
        format!("project={PROJECT}"),
        "--input".to_string(),
        format!("repo={REPO}"),
    ]
}

/// How many runs the journal at `path` has recorded, read through
/// `willikins runs --json` (a read-only replay, so it never contends
/// with anything holding the journal's lock).
pub fn recorded_run_count(journal: &str) -> usize {
    let listed = willikins(&["--json", "runs", "--journal", journal], &[]);
    assert_eq!(listed.code(), 0, "`runs --json` failed: {}", listed.stderr);
    let Some(runs) = listed.docs.first().and_then(Value::as_array) else {
        panic!("`runs --json` printed no array: {}", listed.stdout)
    };
    runs.len()
}
