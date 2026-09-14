//! [`ProviderError`]: a live provider's HTTP failure, and its mapping into
//! [`willikins_core::ToolError`].
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s trust
//! boundary 5 ("Provider responses are a redaction boundary"): a
//! [`ToolError::message`](willikins_core::ToolError) is built from the
//! HTTP status and the provider's own `message` field, bounded and
//! escaped, never from the raw body, and never with the request URL or
//! headers.

use std::time::Duration;

use willikins_core::{ToolError, ToolErrorKind};

/// What a `401` or `403` says instead of anything the provider sent.
/// Applied where the error is *built*, not only where it becomes a
/// [`ToolError`]: a `ProviderError` is public, carries a public `message`,
/// and a provider crate may hold (or log) one long before any conversion.
pub const MISSING_PERMISSION: &str = "the credential is missing a permission this request needs";

/// Repeat a provider's own words under a label, bounded and escaped.
///
/// Escaping alone is not enough to keep provider text from impersonating
/// willikins: [`char::escape_debug`] leaves letters, `[` and `]` alone, so
/// a provider answering `[REDACTED DopplerServiceToken]` would otherwise
/// hand an agent a string indistinguishable from willikins' own redaction
/// marker. Trust boundary 4 solves the same problem for document text with
/// a `document says:` prefix; this is that rule for provider text.
#[must_use]
pub fn provider_says(text: &str) -> String {
    format!("provider says: {}", bounded_message(text))
}

/// The greatest number of characters a [`ProviderError::message`] carries.
/// Mirrors `willikins_types::MAX_QUOTED_INPUT`'s reasoning: a provider's
/// error body is text willikins does not control, and an agent reads
/// whatever this crate echoes.
pub const MAX_MESSAGE_CHARS: usize = 256;

/// A live provider's response to a request this crate could not treat as
/// a plain success.
///
/// Built once, at the HTTP boundary in [`crate::http::Http`], from the
/// response status and (when present) the provider's own `message` field —
/// never from the raw body text, which may carry a plaintext secret this
/// crate never inspects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// The HTTP status code, when the failure is one the provider
    /// answered at all (`None` for a transport-level failure: a timeout,
    /// a connection refusal, an unparseable response).
    pub status: Option<u16>,
    /// A bounded, escaped, human-readable explanation. Never the
    /// credential, never the raw response body, never the request URL.
    pub message: String,
    /// Whether the response body named an `errors[].code` of
    /// `already_exists` (GitHub's shape for "this name is taken",
    /// surfaced on some `422` responses). Not itself part of the plan's
    /// `ProviderError { status, message }` shorthand, but the only place
    /// this information can live: the classification `409 and 422
    /// whose errors[].code is already_exists -> Conflict` needs it at the
    /// point a `ProviderError` becomes a [`ToolError`], and by then the
    /// raw body is gone. Defaults to `false`; a provider crate (task 7)
    /// sets it while parsing an error body it recognises the shape of.
    pub already_exists: bool,
    /// The delay a `Retry-After` response header named, parsed the same
    /// way a retryable status's own backoff is (plain seconds or an
    /// `IMF-fixdate`). Never itself a reason to retry inside this crate —
    /// `willikins-providers-http` never retries a `401`/`403` — but a
    /// provider crate whose API distinguishes "missing permission" from
    /// "temporarily rate limited" through exactly this header (GitHub's
    /// secondary rate limit is a `403` carrying one) needs it to decide.
    /// `None` when the response carried no such header, including every
    /// transport-level failure.
    pub retry_after: Option<Duration>,
    /// The response's `x-ratelimit-remaining` header, parsed as a plain
    /// integer, when present. A convention several REST APIs (GitHub's
    /// among them) use to say how many requests are left in the current
    /// window; `Some(0)` on a `403` is GitHub's secondary-rate-limit
    /// signal in the absence of `Retry-After`. Never interpreted by this
    /// crate itself.
    pub rate_limit_remaining: Option<u64>,
    /// The response's `x-ratelimit-reset` header (a Unix epoch second),
    /// parsed as a plain integer, when present. Paired with
    /// [`Self::rate_limit_remaining`].
    pub rate_limit_reset: Option<u64>,
}

/// The three rate-limit-adjacent header facts [`crate::http::Http`]
/// captures off the final response for any request, success or failure,
/// and attaches to a [`ProviderError`] via [`ProviderError::with_facts`].
/// See [`ProviderError::retry_after`], [`ProviderError::rate_limit_remaining`],
/// and [`ProviderError::rate_limit_reset`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderFacts {
    /// See [`ProviderError::retry_after`].
    pub retry_after: Option<Duration>,
    /// See [`ProviderError::rate_limit_remaining`].
    pub rate_limit_remaining: Option<u64>,
    /// See [`ProviderError::rate_limit_reset`].
    pub rate_limit_reset: Option<u64>,
}

impl ProviderError {
    /// Build a `ProviderError` carrying no already-exists signal and no
    /// rate-limit facts — the common case for a status this crate does
    /// not special-case.
    #[must_use]
    pub fn new(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            already_exists: false,
            retry_after: None,
            rate_limit_remaining: None,
            rate_limit_reset: None,
        }
    }

    /// Build a `ProviderError` for a `409` or `422` whose body named
    /// `errors[].code: "already_exists"` (or, GitHub's other shape for the
    /// same fact, `code: "custom"` on `field: "name"`).
    #[must_use]
    pub fn already_exists(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            already_exists: true,
            ..Self::new(status, message)
        }
    }

    /// Attach header-derived facts captured off the response that produced
    /// this error. A provider crate consults these to distinguish, for
    /// example, a bare permission failure from a rate limit that also
    /// answers `403`; this crate's own `401`/`403` handling never retries
    /// either way.
    #[must_use]
    pub fn with_facts(mut self, facts: ProviderFacts) -> Self {
        self.retry_after = facts.retry_after;
        self.rate_limit_remaining = facts.rate_limit_remaining;
        self.rate_limit_reset = facts.rate_limit_reset;
        self
    }

    /// Whether this looks like a provider's secondary/soft rate limit
    /// rather than a genuine permission failure: a `403` accompanied by a
    /// `Retry-After` header or an exhausted `x-ratelimit-remaining: 0`.
    /// This crate never acts on the answer itself — callers do.
    #[must_use]
    pub fn looks_rate_limited(&self) -> bool {
        self.status == Some(403)
            && (self.retry_after.is_some() || self.rate_limit_remaining == Some(0))
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(f, "provider responded {status}: {}", self.message),
            None => write!(f, "provider request failed: {}", self.message),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<ProviderError> for ToolError {
    fn from(err: ProviderError) -> Self {
        match err.status {
            Some(404) => ToolError {
                kind: ToolErrorKind::NotFound,
                message: err.message,
            },
            Some(409) => ToolError {
                kind: ToolErrorKind::Conflict,
                message: err.message,
            },
            Some(422) if err.already_exists => ToolError {
                kind: ToolErrorKind::Conflict,
                message: err.message,
            },
            // Belt and braces: `provider_error_from_body` already
            // dropped the body for these two statuses, so this arm only
            // matters for a `ProviderError` a provider crate built itself.
            Some(401 | 403) => ToolError {
                kind: ToolErrorKind::Provider,
                message: MISSING_PERMISSION.to_string(),
            },
            _ => ToolError {
                kind: ToolErrorKind::Provider,
                message: err.message,
            },
        }
    }
}

/// Escape `text` onto one line, the same rule `willikins-cli`'s
/// `render::single_line` applies to document text: every character that
/// is not printable is rewritten as its [`char::escape_debug`] form;
/// quotes are left alone.
///
/// Duplicated rather than shared with `willikins-cli` (which does not sit
/// below this crate in the dependency graph, and neither crate sits below
/// the other) — see the crate contract's note that this function may
/// duplicate `willikins-cli`'s. If a third copy is ever needed, it belongs
/// in `willikins-core` instead.
fn single_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\'' | '"' => out.push(c),
            _ => out.extend(c.escape_debug()),
        }
    }
    out
}

/// Bound `text` to [`MAX_MESSAGE_CHARS`] characters (counted before
/// escaping, matching `willikins_types::quoted`'s rule) and escape it onto
/// one line.
#[must_use]
pub fn bounded_message(text: &str) -> String {
    let truncated: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    single_line(&truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_404() {
        let err = ProviderError::new(Some(404), "no such repository");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::NotFound);
    }

    #[test]
    fn conflict_maps_409() {
        let err = ProviderError::new(Some(409), "already exists");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Conflict);
    }

    #[test]
    fn conflict_maps_422_with_already_exists() {
        let err = ProviderError::already_exists(Some(422), "name already exists");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Conflict);
    }

    #[test]
    fn provider_maps_422_without_already_exists() {
        let err = ProviderError::new(Some(422), "some other validation failure");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Provider);
    }

    #[test]
    fn permission_message_never_carries_the_body_on_401() {
        let err = ProviderError::new(Some(401), "Bad credentials: ghp_SECRETVALUE");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Provider);
        assert!(!tool_err.message.contains("SECRETVALUE"));
        assert!(tool_err.message.contains("permission"));
    }

    #[test]
    fn permission_message_never_carries_the_body_on_403() {
        let err = ProviderError::new(Some(403), "Bad credentials: ghp_SECRETVALUE");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Provider);
        assert!(!tool_err.message.contains("SECRETVALUE"));
    }

    #[test]
    fn everything_else_is_provider() {
        for status in [400, 418, 500, 503] {
            let err = ProviderError::new(Some(status), "boom");
            let tool_err: ToolError = err.into();
            assert_eq!(tool_err.kind, ToolErrorKind::Provider, "status {status}");
        }
    }

    #[test]
    fn transport_failure_with_no_status_is_provider() {
        let err = ProviderError::new(None, "connection reset");
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Provider);
    }

    #[test]
    fn bounded_message_truncates_to_256_characters() {
        let long = "a".repeat(10_000);
        let bounded = bounded_message(&long);
        assert_eq!(bounded.chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn bounded_message_escapes_control_characters() {
        let bounded = bounded_message("line one\nline two");
        assert_eq!(bounded, "line one\\nline two");
        assert!(!bounded.contains('\n'));
    }

    #[test]
    fn bounded_message_leaves_quotes_alone() {
        let bounded = bounded_message("it said \"hello\"");
        assert_eq!(bounded, "it said \"hello\"");
    }
}
