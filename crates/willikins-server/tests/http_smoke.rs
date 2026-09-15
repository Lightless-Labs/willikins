//! The one test in task 10b that binds a real port: `serve_http_with`
//! over a pre-bound `127.0.0.1:0` `TcpListener` (so the OS picks a free
//! port and the test learns it from `local_addr()`, never guessing one),
//! answers `/healthz`, then shuts down on a oneshot signal. Every other
//! HTTP acceptance test drives `router()` directly through
//! `tower::ServiceExt::oneshot` (`tests/http_server.rs`), with no port
//! bound at all.

use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, MemoryJournal, Timestamp};
use willikins_server::{Butler, ButlerConfig, HttpConfig, SharedJournal, TokenHash};

#[tokio::test(flavor = "multi_thread")]
async fn serve_http_with_answers_healthz_then_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    let clock: Arc<dyn Clock> = Arc::new(willikins_journal::ManualClock::new(
        Timestamp::parse("2026-09-15T00:00:00+00:00").unwrap(),
    ));
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(clock.clone())));
    let (_state, catalog) = Butler::fake_catalog();
    let butler = Arc::new(Butler::new(ButlerConfig {
        workflows_dir: dir.path().to_path_buf(),
        journal,
        catalog,
        clock,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let config = HttpConfig::build(
        addr,
        vec![TokenHash::of("agent-token")],
        TokenHash::of("approver-token"),
        vec!["127.0.0.1".to_string()],
    )
    .unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(willikins_server::serve_http_with(
        Arc::clone(&butler),
        config,
        listener,
        async move {
            let _ = shutdown_rx.await;
        },
    ));

    // The listener is already bound and accepting by the time `bind`
    // returned above, but `axum::serve` needs a moment to start polling
    // it; a short, bounded retry loop is simpler and faster than a fixed
    // sleep and does not flake on a slow CI host.
    let url = format!("http://{addr}/healthz");
    let mut last_error = None;
    let mut healthy = false;
    for _ in 0..100 {
        match tokio::task::spawn_blocking({
            let url = url.clone();
            move || ureq::get(&url).call()
        })
        .await
        .unwrap()
        {
            Ok(mut response) => {
                assert_eq!(response.status().as_u16(), 200);
                let body = response.body_mut().read_to_string().unwrap();
                assert_eq!(body, "ok");
                healthy = true;
                break;
            }
            Err(error) => {
                last_error = Some(error.to_string());
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
    }
    assert!(healthy, "server never became healthy: {last_error:?}");

    shutdown_tx.send(()).unwrap();
    server
        .await
        .expect("the server task does not panic")
        .expect("serve_http_with returns cleanly on shutdown");
}
