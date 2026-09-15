//! Task 10b: bearer authentication in front of `/mcp`, HTTP Basic in
//! front of `/approvals`.
//!
//! Both middlewares share one [`AuthTokens`] (the configured agent and
//! approver hashes) and one [`Arc<Butler>`] (to journal a failure through
//! [`Butler::record_auth_failure`]). A presented credential is always
//! reduced to a [`TokenHash`] and compared against every configured hash
//! in constant time (`subtle::ConstantTimeEq`, accumulated with `|`, no
//! early return) before any branch is taken on the result -- this is what
//! stops a timing attack from recovering a valid token one byte at a
//! time; branching on the final yes/no *after* that point is fine and
//! unavoidable (every authentication check ends in a branch on its
//! result somewhere).
//!
//! [`AuthFailedReason`] has no variant for "valid credential, wrong
//! role", "missing nonce", or "foreign origin" beyond the three the
//! journal wire format already defines
//! ([`AuthFailedReason::MissingCredential`],
//! [`AuthFailedReason::InvalidCredential`], [`AuthFailedReason::WrongRole`]):
//! adding one is exactly the kind of journal wire-format change this task
//! was told not to make. `WrongRole` is used for its literal meaning (an
//! agent hash presented where the approver's belongs, or vice versa);
//! every other new failure this transport introduces (a missing or
//! reused nonce, a foreign `Origin`/`Referer`, a malformed Basic username)
//! is recorded as `InvalidCredential` -- documented here, and at each call
//! site, as a deliberate mapping rather than an oversight, and named as a
//! gap for whichever pass next owns journal wire-format changes.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use subtle::ConstantTimeEq;

use willikins_journal::{AuthFailedReason, PrincipalId, Transport};

use super::config::TokenHash;
use crate::Butler;

/// The configured credential hashes both middlewares check against, plus
/// the [`Butler`] they journal a failure through.
pub(crate) struct AuthTokens {
    pub(crate) butler: Arc<Butler>,
    pub(crate) agent_hashes: Vec<TokenHash>,
    pub(crate) approver_hash: TokenHash,
}

/// Whether `candidate` equals any of `hashes`, computed without any
/// early return on the first (or any) match -- every hash is compared,
/// every time, and the results are combined with `subtle::Choice`'s own
/// bitwise `|`.
fn matches_any(candidate: &TokenHash, hashes: &[TokenHash]) -> subtle::Choice {
    hashes.iter().fold(subtle::Choice::from(0u8), |acc, hash| {
        acc | candidate.as_bytes().ct_eq(hash.as_bytes())
    })
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// `agent-<first 12 hex chars of the token's own hash>` -- deterministic
/// (the same token always derives the same principal) and distinct per
/// token, inside [`PrincipalId`]'s grammar (`^[A-Za-z0-9][A-Za-z0-9._@-]{0,127}$`,
/// which `agent-` plus 12 hex characters always satisfies).
fn agent_principal(hash: &TokenHash) -> PrincipalId {
    PrincipalId::parse(&format!("agent-{}", hash.short_hex()))
        .unwrap_or_else(|error| unreachable!("`agent-<12 hex>` always parses: {error}"))
}

fn unauthorized_bearer() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer realm=\"willikins\"")],
        "",
    )
        .into_response()
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "").into_response()
}

/// `axum::middleware::from_fn_with_state` layer for `/mcp`: extracts
/// `Authorization: Bearer <token>`, hashes it, and either attaches the
/// derived `agent-*` [`PrincipalId`] to the request's extensions (which
/// `crate::http::router` nests the MCP service behind, and which
/// [`crate::mcp::WillikinsHandler::principal_for`] reads back out) or
/// refuses: 401 with `WWW-Authenticate` when the token is missing or
/// matches nothing configured, 403 when it is exactly the approver's own
/// credential (a valid credential, wrong role).
pub(crate) async fn bearer_auth(
    State(tokens): State<Arc<AuthTokens>>,
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(token) = extract_bearer(&headers) else {
        tokens
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::MissingCredential);
        return unauthorized_bearer();
    };
    let candidate = TokenHash::of(&token);
    let is_agent = matches_any(&candidate, &tokens.agent_hashes);
    let is_approver = candidate.as_bytes().ct_eq(tokens.approver_hash.as_bytes());
    if bool::from(is_agent) {
        request.extensions_mut().insert(agent_principal(&candidate));
        return next.run(request).await;
    }
    if bool::from(is_approver) {
        tokens
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::WrongRole);
        return forbidden();
    }
    tokens
        .butler
        .record_auth_failure(Transport::Http, AuthFailedReason::InvalidCredential);
    unauthorized_bearer()
}

fn extract_basic(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = BASE64.decode(encoded).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

fn unauthorized_basic() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"willikins\"")],
        "",
    )
        .into_response()
}

/// `axum::middleware::from_fn_with_state` layer for `/approvals`:
/// extracts HTTP Basic credentials, hashes the password, and either
/// attaches the Basic username -- parsed as a [`PrincipalId`] -- to the
/// request's extensions or refuses: 401 when no credential was
/// presented or the password matches nothing configured, 403 when the
/// password is exactly an agent hash (a valid credential, wrong role) or
/// when it is the approver's own but the username fails
/// [`PrincipalId`]'s grammar or starts with `agent-` (the plan's own
/// rule: an approver's identity may never collide with the `agent-*`
/// namespace `bearer_auth` derives its own principals in). The username
/// case has no `AuthFailedReason` of its own -- see the module doc's
/// mapping note -- and is recorded as `InvalidCredential`.
pub(crate) async fn basic_auth(
    State(tokens): State<Arc<AuthTokens>>,
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some((username, password)) = extract_basic(&headers) else {
        tokens
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::MissingCredential);
        return unauthorized_basic();
    };
    let candidate = TokenHash::of(&password);
    let is_approver = candidate.as_bytes().ct_eq(tokens.approver_hash.as_bytes());
    let is_agent = matches_any(&candidate, &tokens.agent_hashes);
    if bool::from(is_approver) {
        match PrincipalId::parse(&username) {
            Ok(principal) if !principal.as_str().starts_with("agent-") => {
                request.extensions_mut().insert(principal);
                return next.run(request).await;
            }
            _ => {
                tokens
                    .butler
                    .record_auth_failure(Transport::Http, AuthFailedReason::InvalidCredential);
                return forbidden();
            }
        }
    }
    if bool::from(is_agent) {
        tokens
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::WrongRole);
        return forbidden();
    }
    tokens
        .butler
        .record_auth_failure(Transport::Http, AuthFailedReason::InvalidCredential);
    unauthorized_basic()
}
