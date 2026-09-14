//! Task 10b, part A: `Butler` (and `Arc<Butler>`, the handle every MCP
//! tool handler will hold across a `tokio::task::spawn_blocking` call) is
//! `Send + Sync`.
//!
//! This is a compile-time assertion, not a runtime one: `assert_send_sync`
//! never actually runs anything against `T`, it just requires `T: Send +
//! Sync` at the call site, so the `#[test]` function's only job is to be
//! present and get compiled. If `Butler` (or one of the trait objects it
//! holds -- `dyn Tool`, `dyn Clock`, `dyn Journal`) ever loses `Send` or
//! `Sync`, this file fails to compile rather than a test failing at
//! runtime.

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn butler_and_arc_butler_are_send_and_sync() {
    assert_send_sync::<willikins_server::Butler>();
    assert_send_sync::<std::sync::Arc<willikins_server::Butler>>();
}
