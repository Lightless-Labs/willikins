//! `GET /approvals` and `POST /approvals/{plan_id}`: the human decision
//! surface, HTTP Basic-protected (see `crate::http::auth::basic_auth`).
//!
//! Every string this module writes into the page is HTML-escaped
//! ([`escape_html`]), including every document-authored one (the
//! workflow name is grammar-bounded and safe by construction, but a
//! description is free text) -- those carry the `document says:` label
//! the CLI already uses (`willikins-cli`'s `render` module), so a reader
//! of the rendered page has the same signal a CLI user does: text after
//! that label came from the workflow document, not from willikins.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as PathExtractor, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Router, routing};

use willikins_journal::{
    AuthFailedReason, PlanId, PlanRecord, PrincipalId, Reason, Timestamp, Transport,
};
use willikins_types::WorkflowName;

use super::nonce::NonceStore;
use crate::Butler;
use crate::mcp::run_blocking;

/// State `GET`/`POST /approvals` share: the [`Butler`] both read and
/// decide against, the allowed-hosts list `POST` checks a request's
/// `Origin`/`Referer` against, and this process's own outstanding
/// nonces.
pub(crate) struct ApprovalsState {
    butler: Arc<Butler>,
    allowed_hosts: Vec<String>,
    nonces: NonceStore,
}

impl ApprovalsState {
    pub(crate) fn new(butler: Arc<Butler>, allowed_hosts: Vec<String>) -> Self {
        Self {
            butler,
            allowed_hosts,
            nonces: NonceStore::new(),
        }
    }
}

/// The `/approvals` sub-router: `GET` renders the page, `POST /{plan_id}`
/// decides. Basic auth is layered on by `crate::http::router`, which is
/// also what attaches the approver's [`PrincipalId`] to the request
/// extensions this module reads via axum's own `Extension` extractor.
pub(crate) fn approvals_router(state: Arc<ApprovalsState>) -> Router<()> {
    Router::new()
        .route("/approvals", routing::get(get_approvals))
        .route("/approvals/{plan_id}", routing::post(post_decision))
        .with_state(state)
}

/// Escape `text` for inclusion in HTML: `&`, `<`, `>`, `"`, `'` become
/// their named entities; everything else is left alone. The only path
/// from any string (document text, a plan's rendered JSON, an id) to
/// this page's markup -- acceptance test 12's hostile-description and
/// `<script>`/`</textarea>` fixtures both exist to pin that no call site
/// below skips it.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn elapsed(since: Timestamp, now: Timestamp) -> Duration {
    (*now.as_datetime() - *since.as_datetime())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

/// Best-effort: the workflow's own top-level description, plus every
/// input that declares one, each labelled `<input>: <description>` --
/// reloaded fresh from `dir` rather than taken from the plan record,
/// since `PlanRecord` carries no description at all (only the resolved
/// values `check`/`describe` already consumed it to produce). `None`
/// when the document cannot be reloaded (deleted or changed since
/// `plan`; the page still renders the rest) or declares no description
/// anywhere.
fn document_says(dir: &Path, name: &WorkflowName) -> Option<String> {
    let (_sha, workflow) = crate::document::load_named_document(dir, name).ok()?;
    let mut lines = Vec::new();
    if let Some(description) = &workflow.description {
        lines.push(description.as_str().to_string());
    }
    for (input, spec) in &workflow.inputs {
        if let Some(description) = &spec.description {
            lines.push(format!("{input}: {description}"));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// One pending plan's section of the page: name, age, requester, class,
/// any document-authored description, the redacted plan (its own
/// already-redacted JSON -- see `willikins_journal::journal::PlanRecord::plan`'s
/// doc: every value in it already went through `Value::render()`/`Value::Serialize`,
/// so this is exactly "the redacted plan rendered through `render()`",
/// just JSON-shaped rather than the CLI's line-oriented text), and one
/// approve and one reject form, each carrying this render's fresh
/// single-use nonce.
fn render_plan_section(butler: &Butler, dir: &Path, record: &PlanRecord, nonce: &str) -> String {
    let now = butler.now();
    let age = elapsed(record.recorded_at, now).as_secs();
    let requester = butler.requested_by(record.plan_id).map_or_else(
        || {
            "unknown (recorded before this server process started, or by another instance)"
                .to_string()
        },
        |principal| principal.to_string(),
    );
    let class = format!("{:?}", record.class);
    let plan_json =
        serde_json::to_string_pretty(record.plan.as_json()).unwrap_or_else(|_| "{}".to_string());
    let plan_id = record.plan_id.to_string();

    let mut out = String::new();
    out.push_str("<section class=\"plan\">\n");
    let _ = writeln!(out, "<h2>{}</h2>", escape_html(record.workflow.as_str()));
    let _ = writeln!(
        out,
        "<p>plan_id: <code>{}</code></p>",
        escape_html(&plan_id)
    );
    let _ = writeln!(out, "<p>age: {age} second(s)</p>");
    let _ = writeln!(out, "<p>requester: {}</p>", escape_html(&requester));
    let _ = writeln!(out, "<p>class: {}</p>", escape_html(&class));
    if let Some(description) = document_says(dir, &record.workflow) {
        for line in description.lines() {
            let _ = writeln!(out, "<p>document says: {}</p>", escape_html(line));
        }
    }
    out.push_str("<pre class=\"plan-json\">");
    out.push_str(&escape_html(&plan_json));
    out.push_str("</pre>\n");
    let plan_id_escaped = escape_html(&plan_id);
    let nonce_escaped = escape_html(nonce);
    let _ = writeln!(
        out,
        "<form method=\"post\" action=\"/approvals/{plan_id_escaped}\">\
<input type=\"hidden\" name=\"nonce\" value=\"{nonce_escaped}\">\
<input type=\"hidden\" name=\"decision\" value=\"approve\">\
<button type=\"submit\">Approve</button>\
</form>"
    );
    let _ = writeln!(
        out,
        "<form method=\"post\" action=\"/approvals/{plan_id_escaped}\">\
<input type=\"hidden\" name=\"nonce\" value=\"{nonce_escaped}\">\
<input type=\"hidden\" name=\"decision\" value=\"reject\">\
<label>reason <input type=\"text\" name=\"reason\" maxlength=\"256\"></label>\
<button type=\"submit\">Reject</button>\
</form>"
    );
    out.push_str("</section>\n");
    out
}

fn render_page(sections: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\
<title>willikins: pending approvals</title></head>\n\
<body>\n<h1>Pending approvals</h1>\n{sections}</body></html>\n"
    )
}

/// `GET /approvals`: every plan still waiting on a human decision
/// (`Butler::pending_approvals`), each with a freshly issued nonce (see
/// `NonceStore::issue`'s own doc: a page reload always matches its own
/// forms).
pub(crate) async fn get_approvals(State(state): State<Arc<ApprovalsState>>) -> Html<String> {
    let butler = Arc::clone(&state.butler);
    let plans = run_blocking(move || butler.pending_approvals()).await;
    let now = state.butler.now();
    let mut sections = String::new();
    for record in &plans {
        let nonce = state.nonces.issue(record.plan_id, now);
        sections.push_str(&render_plan_section(
            &state.butler,
            state.butler.workflows_dir(),
            record,
            &nonce,
        ));
    }
    if plans.is_empty() {
        sections.push_str("<p>No plans are waiting on a decision.</p>\n");
    }
    Html(render_page(&sections))
}

/// `POST /approvals/{plan_id}`'s form body.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct DecisionForm {
    decision: String,
    nonce: String,
    #[serde(default)]
    reason: String,
}

/// Parse the authority (`host` or `host:port`) out of an absolute URL
/// (`Origin`'s or `Referer`'s own shape: `scheme://authority[/...]`), by
/// hand -- no URL-parsing crate is a workspace dependency, and this needs
/// only the one component.
fn extract_authority(value: &str) -> Option<&str> {
    let after_scheme = value.split_once("://")?.1;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())?;
    Some(authority)
}

/// Whether `authority` (a `host` or `host:port`) matches `allowed` (one
/// `WILLIKINS_ALLOWED_HOSTS` entry, itself a `host` or `host:port`):
/// exact match, or `authority`'s host with any port stripped, ASCII
/// case-insensitively.
fn host_matches(allowed: &str, authority: &str) -> bool {
    if allowed.eq_ignore_ascii_case(authority) {
        return true;
    }
    let host_only = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    allowed.eq_ignore_ascii_case(host_only)
}

/// `Origin` (or, when absent, `Referer`) must name one of `allowed_hosts`
/// -- review resolution 1's second defence. Both headers absent, or
/// present but unparseable/foreign, refuses.
fn origin_allowed(headers: &HeaderMap, allowed_hosts: &[String]) -> bool {
    let header = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER));
    let Some(value) = header.and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(authority) = extract_authority(value) else {
        return false;
    };
    allowed_hosts
        .iter()
        .any(|allowed| host_matches(allowed, authority))
}

/// `POST /approvals/{plan_id}`: checks `Origin`/`Referer` first, then
/// consumes the presented nonce (see `NonceStore::consume`'s own doc --
/// single-use, regardless of outcome), then decides. Both the origin
/// check and the nonce check refuse 403 and journal `AuthFailed` (mapped
/// to `InvalidCredential` -- see `crate::http::auth`'s module doc for why
/// neither gets its own `AuthFailedReason`), leaving the plan untouched
/// (pending) either way, before `Butler::approve`/`reject` is ever
/// called. The nonce is deliberately not consumed on a foreign-origin
/// refusal: a legitimate approver retrying from the right origin should
/// not find their own still-fresh nonce burned by an unrelated forged
/// request.
pub(crate) async fn post_decision(
    State(state): State<Arc<ApprovalsState>>,
    PathExtractor(plan_id): PathExtractor<PlanId>,
    headers: HeaderMap,
    axum::Extension(approver): axum::Extension<PrincipalId>,
    Form(form): Form<DecisionForm>,
) -> Response {
    if !origin_allowed(&headers, &state.allowed_hosts) {
        state
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::InvalidCredential);
        return (
            StatusCode::FORBIDDEN,
            "Origin/Referer is not an allowed host",
        )
            .into_response();
    }

    let now = state.butler.now();
    let window = state.butler.approval_window();
    if !state.nonces.consume(plan_id, &form.nonce, now, window) {
        state
            .butler
            .record_auth_failure(Transport::Http, AuthFailedReason::InvalidCredential);
        return (StatusCode::FORBIDDEN, "missing, reused, or expired nonce").into_response();
    }

    let butler = Arc::clone(&state.butler);
    let result = match form.decision.as_str() {
        "approve" => run_blocking(move || butler.approve(plan_id, approver)).await,
        "reject" => match Reason::parse(&form.reason) {
            Ok(reason) => run_blocking(move || butler.reject(plan_id, approver, reason)).await,
            Err(error) => {
                return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
            }
        },
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "decision must be `approve` or `reject`",
            )
                .into_response();
        }
    };

    match result {
        Ok(()) => Redirect::to("/approvals").into_response(),
        Err(error) => (StatusCode::CONFLICT, error.to_string()).into_response(),
    }
}
