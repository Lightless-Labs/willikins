//! `FakeState`'s own bookkeeping, attacked directly: the call counters,
//! the one-shot failure injection, the one-shot `next_token`, and the
//! one-way redacted serialization.
//!
//! The per-tool halves of these facts live in each tool's own `#[cfg(test)]`
//! module; this file pins the parts that only show up *between* tools —
//! one seeded `next_token` shared by the two minting tools, a
//! `fail_ensure_once` entry keyed to one tool not firing for another, and
//! what a dumped state does when it is loaded back.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_fake::state::{FakeState, call_key, doppler_service_token_key, repo_key};
use willikins_providers_fake::tools::{
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate, GitHubRepoEnsure,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerServiceToken, DopplerTokenName, GitHubRepo, RepoVisibility,
};

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

fn token_name() -> DopplerTokenName {
    DopplerTokenName::parse("ci").unwrap()
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()
}

fn token_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(config()));
    inputs.insert(port("name"), Value::known(token_name()));
    inputs
}

fn repo_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("repo"), Value::known(repo()));
    inputs.insert(port("visibility"), Value::known(RepoVisibility::Private));
    inputs
}

fn minted(outputs: &willikins_core::Outputs) -> DopplerServiceToken {
    outputs
        .get(&port("token"))
        .expect("a token output")
        .downcast::<DopplerServiceToken>()
        .cloned()
        .expect("a known, minted token")
}

fn seeded_marker() -> DopplerServiceToken {
    DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7))).unwrap()
}

/// A seeded `next_token` is one token, not one per minting tool: whichever
/// mint reaches it first consumes it, and every later mint — in the same
/// tool or another one — is a generated value.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own SinkToken
fn a_seeded_next_token_is_consumed_once_across_every_minting_tool() {
    let state = Arc::new(Mutex::new(
        FakeState::new().with_next_token(seeded_marker()),
    ));
    let ensure = DopplerServiceTokenEnsure::new(Arc::clone(&state));
    let rotate = DopplerServiceTokenRotate::new(Arc::clone(&state));
    let sink = SinkToken::new();

    let first = minted(&ensure.ensure(&token_inputs(), &sink).unwrap().outputs);
    assert_eq!(first, seeded_marker(), "the first mint takes the seed");

    let second = minted(&rotate.ensure(&token_inputs(), &sink).unwrap().outputs);
    assert_ne!(second, seeded_marker(), "the seed is consumed, not reused");

    let third = minted(&rotate.ensure(&token_inputs(), &sink).unwrap().outputs);
    assert_ne!(third, seeded_marker());
    assert_ne!(
        second, third,
        "two generated mints for the same key must differ"
    );
}

/// `read` and `ensure` count separately, exactly once per call, and an
/// `ensure` that observes its own state on the way through does not also
/// count as a `read`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own SinkToken
fn read_and_ensure_calls_count_exactly_once_per_call() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let tool = GitHubRepoEnsure::new(Arc::clone(&state));
    let sink = SinkToken::new();

    tool.read(&repo_inputs()).unwrap();
    tool.read(&repo_inputs()).unwrap();
    tool.ensure(&repo_inputs(), &sink).unwrap();
    tool.ensure(&repo_inputs(), &sink).unwrap();
    tool.ensure(&repo_inputs(), &sink).unwrap();

    let key = call_key("github.repo.ensure", &repo_key(&repo()));
    let locked = state.lock().unwrap();
    assert_eq!(locked.read_calls.get(&key).copied(), Some(2));
    assert_eq!(locked.ensure_calls.get(&key).copied(), Some(3));
}

/// A `fail_ensure_once` entry is keyed by tool *and* key: the same
/// resource key under another tool's name does not fire it, and the entry
/// survives until its own tool asks for it.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own SinkToken
fn an_injected_failure_belongs_to_one_tool_and_fires_once() {
    let key = doppler_service_token_key(&config(), &token_name());
    let state = Arc::new(Mutex::new(
        FakeState::new().with_fail_ensure_once("doppler.service_token.rotate", &key),
    ));
    let ensure = DopplerServiceTokenEnsure::new(Arc::clone(&state));
    let rotate = DopplerServiceTokenRotate::new(Arc::clone(&state));
    let sink = SinkToken::new();

    // The same resource key, a different tool: unaffected.
    ensure
        .ensure(&token_inputs(), &sink)
        .expect("the entry belongs to `rotate`, not to `ensure`");
    assert_eq!(
        state.lock().unwrap().fail_ensure_once.len(),
        1,
        "another tool's call must not consume the entry"
    );

    let err = rotate
        .ensure(&token_inputs(), &sink)
        .expect_err("the injected failure must fire for its own tool");
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        state.lock().unwrap().fail_ensure_once.is_empty(),
        "the entry is consumed"
    );

    rotate
        .ensure(&token_inputs(), &sink)
        .expect("a one-shot failure fires exactly once");
}

/// A dumped state prints the `next_token` marker rather than its bytes,
/// and loading that dump back fails outright rather than resurrecting the
/// marker as a seeded token: `FakeState`'s serialization is a one-way,
/// redacted view.
#[test]
fn a_dumped_state_redacts_next_token_and_refuses_to_reload() {
    let state = FakeState::new().with_next_token(seeded_marker());
    let json = serde_json::to_string(&state).expect("FakeState serializes");
    assert!(
        !json.contains("MARKERMARKER"),
        "the seeded token's bytes leaked: {json}"
    );
    assert!(
        json.contains("REDACTED"),
        "expected the redaction marker: {json}"
    );
    let reloaded = FakeState::from_json(&json);
    assert!(
        reloaded.is_err(),
        "a dump holding a seeded next_token must not reload"
    );
}

/// `doppler.service_token.rotate` on a token that does not exist yet
/// creates one rather than refusing: the tool's contract is "revoke and
/// re-mint", and revoking nothing is not an error — what makes it
/// `Destructive` is that it always replaces whatever is there, which a
/// plan shows as `Create` every time (see the tool's own module doc).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own SinkToken
fn rotate_on_an_absent_token_creates_it() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let rotate = DopplerServiceTokenRotate::new(Arc::clone(&state));
    let sink = SinkToken::new();

    let ensured = rotate.ensure(&token_inputs(), &sink).unwrap();
    assert!(ensured.changed);
    // A secret type's `Display` is its redaction marker, so the minted
    // value is checked by identity (it is a parsed `DopplerServiceToken`
    // at all, and differs from a second rotation's) rather than by text.
    assert_ne!(
        minted(&ensured.outputs),
        minted(&rotate.ensure(&token_inputs(), &sink).unwrap().outputs)
    );
    assert!(
        state
            .lock()
            .unwrap()
            .doppler_service_tokens
            .contains(&doppler_service_token_key(&config(), &token_name())),
        "the rotated token now exists"
    );
}
