//! Task 12 verification: the two `Host` headers a Railway deployment
//! actually receives, neither of which any existing test sends.
//!
//! 1. Railway's healthcheck requests come from the hostname
//!    `healthcheck.railway.app` (Railway's healthcheck documentation,
//!    fetched 2026-09-15: <https://docs.railway.com/deployments/healthchecks>
//!    -- "Railway uses the hostname `healthcheck.railway.app` when
//!    performing healthchecks ... For applications that restrict
//!    incoming traffic based on the hostname, you'll need to add
//!    `healthcheck.railway.app` to your list of allowed hosts").
//!    `/healthz` sits outside the allowed-hosts check, so a healthcheck
//!    answers whatever `WILLIKINS_ALLOWED_HOSTS` holds. `.railway/railway.ts`
//!    declares `healthcheck: "/healthz"`; this is the test that says the
//!    declaration is safe to apply without also widening the variable.
//!
//! 2. A client on Railway's private network reaches the service at
//!    `<service>.railway.internal:<port>`, so its `Host` header carries
//!    the port while `WILLIKINS_ALLOWED_HOSTS` holds the bare private
//!    domain. rmcp 3.3.0's own `host_is_allowed` treats an allowed entry
//!    with no port as matching any port
//!    (`transport/streamable_http_server/tower.rs`); this pins that
//!    reading against the version actually in `Cargo.lock`, so task 14's
//!    live smoke run does not discover it against a real deployment.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _;

use willikins_journal::{Clock, ManualClock, MemoryJournal, Timestamp};
use willikins_server::{Butler, ButlerConfig, HttpConfig, SharedJournal, TokenHash};

const AGENT_TOKEN: &str = "agent-token-for-deploy-host-header-tests";
const APPROVER_TOKEN: &str = "approver-token-for-deploy-host-header-tests";
/// What `WILLIKINS_ALLOWED_HOSTS` holds on the live service: Railway's
/// private domain for the service, with no port.
const PRIVATE_DOMAIN: &str = "willikins.railway.internal";

fn manual_clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(
        Timestamp::parse("2026-09-15T00:00:00+00:00").unwrap(),
    ))
}

fn butler(dir: &std::path::Path) -> Arc<Butler> {
    let clock: Arc<dyn Clock> = manual_clock();
    let (_state, catalog) = Butler::fake_catalog();
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(clock.clone())));
    Arc::new(Butler::new(ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal,
        catalog,
        clock,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    }))
}

/// The live deployment's own configuration shape: one allowed host, the
/// bare private domain.
fn config() -> HttpConfig {
    HttpConfig::build(
        "0.0.0.0:8080".parse().unwrap(),
        vec![TokenHash::of(AGENT_TOKEN)],
        TokenHash::of(APPROVER_TOKEN),
        vec![PRIVATE_DOMAIN.to_string()],
    )
    .unwrap()
}

fn initialize_request(host: &str) -> Request<Body> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "deploy-host-header-test", "version": "0.1.0" },
        },
    });
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", host)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {AGENT_TOKEN}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

/// Railway's healthcheck reaches `/healthz` under a hostname that is not
/// in `WILLIKINS_ALLOWED_HOSTS` and must still get a 2xx, or the deploy
/// is marked failed after the healthcheck timeout.
#[tokio::test(flavor = "multi_thread")]
async fn healthz_answers_railways_own_healthcheck_hostname() {
    let dir = tempfile::tempdir().unwrap();
    let router = willikins_server::router(butler(dir.path()), &config());

    let request = Request::builder()
        .method("GET")
        .uri("/healthz")
        .header("host", "healthcheck.railway.app")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A private-network client connects to `<domain>:<port>`, so its `Host`
/// header carries the port the allowed-hosts entry does not.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_accepts_the_private_domain_host_with_a_port() {
    let dir = tempfile::tempdir().unwrap();
    let router = willikins_server::router(butler(dir.path()), &config());

    let response = router
        .oneshot(initialize_request(&format!("{PRIVATE_DOMAIN}:8080")))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a ported Host header must match a portless allowed-hosts entry"
    );
}

/// The same header with no port, for the client that omits it.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_accepts_the_private_domain_host_with_no_port() {
    let dir = tempfile::tempdir().unwrap();
    let router = willikins_server::router(butler(dir.path()), &config());

    let response = router
        .oneshot(initialize_request(PRIVATE_DOMAIN))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// The check is still on: a hostname nobody allowed is refused, with or
/// without a port. Without this the two tests above would also pass on a
/// server that had stopped checking the header at all.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_refuses_a_host_header_nobody_allowed() {
    let dir = tempfile::tempdir().unwrap();

    for host in ["willikins.example.net", "willikins.example.net:8080"] {
        let router = willikins_server::router(butler(dir.path()), &config());
        let response = router.oneshot(initialize_request(host)).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{host} must not be accepted"
        );
    }
}

/// A `Host` header that names the allowed domain as a prefix of a longer
/// one (the DNS-rebinding shape the check exists for) is refused.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_refuses_a_host_header_that_only_starts_with_the_allowed_domain() {
    let dir = tempfile::tempdir().unwrap();
    let router = willikins_server::router(butler(dir.path()), &config());

    let response = router
        .oneshot(initialize_request(&format!(
            "{PRIVATE_DOMAIN}.evil.example"
        )))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
