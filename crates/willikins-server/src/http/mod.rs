//! Task 10b: the Streamable HTTP transport -- `/mcp` (bearer auth),
//! `/healthz`, and `/approvals` (Basic auth, nonce, origin check). See
//! the plan's `willikins-server` section, "Transports" through the
//! configuration paragraph, and review resolutions 1, 8, 10, 16.
//!
//! [`router`] builds the whole `axum::Router` without binding a port, so
//! `crates/willikins-server/tests/http_server.rs` drives it with
//! `tower::ServiceExt::oneshot`; [`serve_http`] is production's own entry
//! point, and [`serve_http_with`] is the seam a smoke test binds a real
//! `TcpListener` and a controllable shutdown future through.

mod approvals;
mod auth;
mod config;
mod nonce;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::response::IntoResponse;
use axum::routing::get;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use config::{HttpConfig, HttpConfigError, TokenHash, TokenHashError};

use crate::Butler;
use crate::mcp::WillikinsHandler;
use approvals::ApprovalsState;
use auth::AuthTokens;
use willikins_journal::PrincipalId;

/// The service factory's own fixed principal, standing in for the
/// per-request one every call actually runs as -- see
/// [`WillikinsHandler::requiring_request_principal`]'s doc. Never
/// reaches a `Butler` call: `requiring_request_principal` makes an
/// absent per-request principal a domain error instead of a silent
/// fallback to this value.
fn placeholder_principal() -> PrincipalId {
    PrincipalId::parse("http-transport-placeholder")
        .unwrap_or_else(|error| unreachable!("a fixed literal always parses: {error}"))
}

fn build_mcp_service(
    butler: Arc<Butler>,
    config: &HttpConfig,
) -> StreamableHttpService<WillikinsHandler, LocalSessionManager> {
    let mut handler =
        WillikinsHandler::new(butler, placeholder_principal()).requiring_request_principal();
    if config.fake_catalog {
        handler = handler.with_fake_catalog_note();
    }
    let session_manager = Arc::new(LocalSessionManager::default());
    let streamable_config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_max_request_body_bytes(config.max_body_bytes)
        .with_allowed_hosts(config.allowed_hosts.clone());
    StreamableHttpService::new(
        move || Ok(handler.clone()),
        session_manager,
        streamable_config,
    )
}

async fn healthz() -> impl IntoResponse {
    "ok"
}

/// Build the whole router: `GET /healthz`, `/mcp` (bearer auth, nested
/// rmcp Streamable HTTP), `/approvals` and `/approvals/{plan_id}` (Basic
/// auth). Outermost middleware is the request timeout
/// (`tower-http`, `config.request_timeout`); binds no port -- see the
/// module doc.
pub fn router(butler: Arc<Butler>, config: &HttpConfig) -> Router {
    // Two of `butler`'s three consumers below only need a clone; the
    // third (`build_mcp_service`) takes the parameter's own last, moved
    // handle, so this function's `Arc<Butler>` parameter is genuinely
    // consumed rather than merely borrowed through clones the whole way
    // down.
    let approvals_state = Arc::new(ApprovalsState::new(
        Arc::clone(&butler),
        config.allowed_hosts.clone(),
    ));
    let auth_tokens = Arc::new(AuthTokens {
        butler: Arc::clone(&butler),
        agent_hashes: config.agent_token_hashes.clone(),
        approver_hash: config.approver_token_hash,
    });
    let mcp_service = build_mcp_service(butler, config);

    let mcp_router = Router::new().nest_service("/mcp", mcp_service).layer(
        axum::middleware::from_fn_with_state(Arc::clone(&auth_tokens), auth::bearer_auth),
    );

    let approvals_router = approvals::approvals_router(approvals_state)
        // The same body cap `/mcp` runs under (rmcp enforces its own
        // while streaming; this is axum's, for the one form this
        // transport parses). Without it the approvals form fell back to
        // axum's own 2 MiB default, so the deployment's configured cap
        // bounded only half the surface.
        .layer(axum::extract::DefaultBodyLimit::max(config.max_body_bytes))
        .layer(axum::middleware::from_fn_with_state(
            auth_tokens,
            auth::basic_auth,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(mcp_router)
        .merge(approvals_router)
        // `TraceLayer::new_for_http()`'s defaults never include headers
        // (that needs an explicit `.include_headers(true)` this crate
        // never opts into) or a body, only method, path, status, and
        // latency -- exactly trust boundary 5's tracing rule. Applied
        // before (so, once wrapped by the timeout layer below, inside)
        // the request timeout, so a span still covers a request that
        // times out.
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ))
}

/// Failed to bind or run the Streamable HTTP server.
#[derive(Debug, thiserror::Error)]
pub enum ServeHttpError {
    /// Could not bind `config.bind`.
    #[error("failed to bind {0}")]
    Bind(std::io::Error),
    /// The server itself failed while accepting or serving connections.
    #[error("http server error: {0}")]
    Serve(std::io::Error),
}

/// How long [`serve_http`] waits, after it stops accepting new
/// connections, for an in-progress run to finish -- the journal's own
/// `NodeStarted`/`NodeFinished` pairs (never this bound) are what keep
/// the record truthful if a run outlives it: a run that is still going
/// when the process actually exits leaves its last `NodeStarted` with no
/// matching `NodeFinished`, which is a truthful "this got no further",
/// not a lie.
const RUN_DRAIN_BOUND: Duration = Duration::from_secs(30);

/// Bind `config.bind` and serve `butler`'s HTTP transport until SIGTERM
/// or Ctrl-C, then stop accepting new requests and wait up to
/// [`RUN_DRAIN_BOUND`] for an in-progress run to finish (see that
/// constant's own doc) before returning.
///
/// # Errors
///
/// See [`ServeHttpError`].
pub async fn serve_http(butler: Arc<Butler>, config: HttpConfig) -> Result<(), ServeHttpError> {
    let bind = config.bind;
    let listener = TcpListener::bind(bind)
        .await
        .map_err(ServeHttpError::Bind)?;
    serve_http_with(butler, config, listener, shutdown_signal()).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

/// As [`serve_http`], but over an already-bound `listener` and a
/// caller-supplied `shutdown` future -- the seam
/// `crates/willikins-server/tests/http_smoke.rs` uses to bind
/// `127.0.0.1:0` (learning the real port from `listener.local_addr()`)
/// and trigger shutdown deterministically, rather than sending a real
/// signal to the test process.
///
/// # Errors
///
/// See [`ServeHttpError`].
pub async fn serve_http_with(
    butler: Arc<Butler>,
    config: HttpConfig,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeHttpError> {
    let app = router(Arc::clone(&butler), &config);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(ServeHttpError::Serve)?;
    wait_for_run_to_finish(&butler, RUN_DRAIN_BOUND).await;
    Ok(())
}

async fn wait_for_run_to_finish(butler: &Butler, bound: Duration) {
    let deadline = tokio::time::Instant::now() + bound;
    while butler.run_in_progress().is_some() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
