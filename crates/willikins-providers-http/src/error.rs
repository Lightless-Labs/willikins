//! [`ProviderError`]: a live provider's HTTP failure, and its mapping into
//! [`willikins_core::ToolError`].
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s trust
//! boundary 5 ("Provider responses are a redaction boundary"): a
//! [`ToolError::message`](willikins_core::ToolError) is built from the
//! HTTP status and the provider's own `message` field, bounded and
//! escaped, never from the raw body, and never with the request URL or
//! headers.

use willikins_core::{ToolError, ToolErrorKind};

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
}

impl ProviderError {
    /// Build a `ProviderError` carrying no already-exists signal — the
    /// common case for a status this crate does not special-case.
    #[must_use]
    pub fn new(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            already_exists: false,
        }
    }

    /// Build a `ProviderError` for a `409` or `422` whose body named
    /// `errors[].code: "already_exists"`.
    #[must_use]
    pub fn already_exists(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            already_exists: true,
        }
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
            Some(401 | 403) => ToolError {
                kind: ToolErrorKind::Provider,
                message: "the credential is missing a permission this request needs".to_string(),
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
