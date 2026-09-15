//! Task 10b: the Streamable HTTP transport's own configuration --
//! [`HttpConfig`] and [`TokenHash`] -- for the environment variables
//! [`crate::ServerConfig`] (task 10a's plan/apply library config) does
//! not need to exist at all: bearer/basic auth token hashes, allowed
//! hosts (also fed to rmcp's own `Host`/`Origin` DNS-rebinding defence --
//! see `crate::http::router`), and the bind address. `ServerConfig` grew
//! the raw, permissive fields (`agent_token_hashes`, `approver_token_hash`,
//! `allowed_hosts`, `port`) because `ServerConfig::from_vars` is the one
//! place this crate already reads environment variables from, and the
//! plan's own env-var list groups them there; the *validation* that these
//! fields make an actually startable http-mode configuration -- at least
//! one agent hash, the approver hash not doubling as an agent hash, at
//! least one allowed host -- lives here instead, in [`HttpConfig::build`],
//! because those rules apply only in http mode (stdio needs none of this)
//! and the binary is what knows which mode it is starting in.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// A validated SHA-256 digest: exactly 64 lower-case hex characters --
/// the shape `WILLIKINS_AGENT_TOKEN_HASHES` and
/// `WILLIKINS_APPROVER_TOKEN_HASH` carry on the wire, and what a
/// presented bearer token or Basic password is hashed down to
/// ([`TokenHash::of`]) before ever being compared. Never carries a raw
/// token: only an operator-supplied hex digest, or a hash this process
/// computed from request bytes that are immediately discarded once
/// hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    /// Parse a `TokenHash` from its 64-character lower-case hex form.
    ///
    /// # Errors
    ///
    /// Returns [`TokenHashError`] when `input` is not exactly 64
    /// characters of `0`-`9`/lower-case `a`-`f`.
    pub fn parse(input: &str) -> Result<Self, TokenHashError> {
        let valid = input.len() == 64
            && input
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !valid {
            return Err(TokenHashError);
        }
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&input[index * 2..index * 2 + 2], 16)
                .map_err(|_| TokenHashError)?;
        }
        Ok(Self(bytes))
    }

    /// The SHA-256 of `presented`'s UTF-8 bytes -- what a bearer token or
    /// a Basic password is reduced to before ever being compared against
    /// a configured [`TokenHash`].
    #[must_use]
    pub fn of(presented: &str) -> Self {
        let digest = Sha256::digest(presented.as_bytes());
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }

    /// The raw 32 bytes, for a constant-time comparison
    /// (`crate::http::auth`) -- never printed or serialized directly.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// This hash's first 12 hex characters (6 bytes): deterministic and
    /// distinct per distinct token, without publishing the whole 64-hex
    /// digest in a [`willikins_journal::PrincipalId`]. Used to build the
    /// `agent-<12 hex>` principal id `crate::http::auth::bearer_auth`
    /// derives from a valid agent token.
    #[must_use]
    pub fn short_hex(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(12);
        for byte in &self.0[..6] {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}

/// Why [`TokenHash::parse`] refused its input. Carries no value: even
/// echoing "a 63-character string" back would leak length information
/// about a value adjacent to a secret in the environment; the error
/// names only the shape rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("must be exactly 64 lower-case hexadecimal characters")]
pub struct TokenHashError;

/// Everything [`crate::http::router`] and [`crate::http::serve_http`]
/// need beyond a [`crate::Butler`]. Built only through [`Self::build`],
/// which is where http mode's three startup refusals
/// (no agent hash; the approver hash doubling as an agent hash; no
/// allowed host) are enforced -- never bypassable by constructing this
/// struct field-by-field, since every field but the two size defaults is
/// private.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub(crate) bind: SocketAddr,
    pub(crate) agent_token_hashes: Vec<TokenHash>,
    pub(crate) approver_token_hash: TokenHash,
    pub(crate) allowed_hosts: Vec<String>,
    pub(crate) request_timeout: Duration,
    pub(crate) max_body_bytes: usize,
}

impl HttpConfig {
    /// The design's default request timeout: 30 seconds (review
    /// resolution 16 -- `apply` itself never blocks a request this long;
    /// only `plan`'s handful of sequential provider reads can).
    pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
    /// The design's default request body cap: 1 MiB.
    pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

    /// Build a `HttpConfig`, applying every http-mode startup rule.
    ///
    /// # Errors
    ///
    /// [`HttpConfigError::NoAgentHash`] when `agent_token_hashes` is
    /// empty; [`HttpConfigError::ApproverAmongAgentHashes`] when
    /// `approver_token_hash` equals one of them; [`HttpConfigError::EmptyAllowedHosts`]
    /// when `allowed_hosts` is empty.
    pub fn build(
        bind: SocketAddr,
        agent_token_hashes: Vec<TokenHash>,
        approver_token_hash: TokenHash,
        allowed_hosts: Vec<String>,
    ) -> Result<Self, HttpConfigError> {
        if agent_token_hashes.is_empty() {
            return Err(HttpConfigError::NoAgentHash);
        }
        if agent_token_hashes.contains(&approver_token_hash) {
            return Err(HttpConfigError::ApproverAmongAgentHashes);
        }
        if allowed_hosts.is_empty() {
            return Err(HttpConfigError::EmptyAllowedHosts);
        }
        Ok(Self {
            bind,
            agent_token_hashes,
            approver_token_hash,
            allowed_hosts,
            request_timeout: Self::DEFAULT_REQUEST_TIMEOUT,
            max_body_bytes: Self::DEFAULT_MAX_BODY_BYTES,
        })
    }

    /// Override the request timeout (default [`Self::DEFAULT_REQUEST_TIMEOUT`]).
    /// The binary never calls this (it always takes the default); this
    /// crate's own HTTP acceptance tests use it to make the 504/408 case
    /// reachable without a 30-second test.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Override the request body cap (default [`Self::DEFAULT_MAX_BODY_BYTES`]).
    /// The binary never calls this; this crate's own HTTP acceptance
    /// tests use it to make the 413 case reachable without a 1 MiB body.
    #[must_use]
    pub fn with_max_body_bytes(mut self, bytes: usize) -> Self {
        self.max_body_bytes = bytes;
        self
    }
}

/// Why [`HttpConfig::build`] refused: every rule http mode enforces that
/// [`crate::ServerConfig::from_vars`] itself does not (that function
/// reads these fields permissively; the three checks below apply only
/// when a server is actually about to start over HTTP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HttpConfigError {
    /// http mode needs at least one agent bearer-token hash.
    #[error(
        "http mode needs at least one agent token hash (WILLIKINS_AGENT_TOKEN_HASHES is empty)"
    )]
    NoAgentHash,
    /// The approver's token hash must not equal any agent token hash --
    /// otherwise a single presented token could not be told apart as
    /// agent or approver at all.
    #[error("the approver token hash must not equal any agent token hash")]
    ApproverAmongAgentHashes,
    /// http mode needs at least one allowed host (also rmcp's own
    /// `Host`-header DNS-rebinding defence, and this transport's
    /// `Origin`/`Referer` check on `POST /approvals/{plan_id}`).
    #[error("http mode needs at least one allowed host (WILLIKINS_ALLOWED_HOSTS is empty)")]
    EmptyAllowedHosts,
}

impl fmt::Display for TokenHash {
    /// Never the hash itself -- a fixed placeholder, so an accidental
    /// `{token_hash}` in a log line or error message cannot leak even the
    /// hashed form of a credential. Use [`Self::short_hex`] deliberately
    /// when a short, non-reversible fragment is actually wanted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<token hash>")
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
    }

    fn hash_of(text: &str) -> String {
        hex_of(TokenHash::of(text).as_bytes())
    }

    #[test]
    fn parses_exactly_64_lower_hex_characters() {
        let hex = hash_of("agent-token-one");
        assert_eq!(hex.len(), 64);
        let parsed = TokenHash::parse(&hex).unwrap();
        assert_eq!(parsed, TokenHash::of("agent-token-one"));
    }

    #[test]
    fn refuses_wrong_length_and_upper_case() {
        assert!(TokenHash::parse("abc").is_err());
        let hex = hash_of("x").to_uppercase();
        assert!(TokenHash::parse(&hex).is_err());
    }

    #[test]
    fn refuses_a_hash_that_is_a_prefix_of_another() {
        // Not a constant-time claim (this is `PartialEq` on parsed
        // bytes) -- just pins that a 64-char prefix-sharing string is a
        // different, non-matching 32-byte value, the structural fact
        // `crate::http::auth`'s constant-time compare then builds on.
        let hash = TokenHash::of("abc");
        let hex = hex_of(hash.as_bytes());
        let prefix = &hex[..63];
        let mut extended = prefix.to_string();
        extended.push(if hex.ends_with('0') { '1' } else { '0' });
        assert_ne!(TokenHash::parse(&extended).unwrap(), hash);
    }

    #[test]
    fn short_hex_is_the_first_twelve_hex_characters() {
        let hash = TokenHash::of("agent-token-one");
        let full = hex_of(hash.as_bytes());
        assert_eq!(hash.short_hex(), &full[..12]);
    }

    fn hashes(texts: &[&str]) -> Vec<TokenHash> {
        texts.iter().map(|t| TokenHash::of(t)).collect()
    }

    #[test]
    fn build_refuses_with_no_agent_hash() {
        let err = HttpConfig::build(
            "127.0.0.1:0".parse().unwrap(),
            Vec::new(),
            TokenHash::of("approver"),
            vec!["localhost".to_string()],
        )
        .unwrap_err();
        assert_eq!(err, HttpConfigError::NoAgentHash);
    }

    #[test]
    fn build_refuses_when_approver_hash_is_also_an_agent_hash() {
        let shared = TokenHash::of("shared-secret");
        let err = HttpConfig::build(
            "127.0.0.1:0".parse().unwrap(),
            vec![shared],
            shared,
            vec!["localhost".to_string()],
        )
        .unwrap_err();
        assert_eq!(err, HttpConfigError::ApproverAmongAgentHashes);
    }

    #[test]
    fn build_refuses_with_no_allowed_hosts() {
        let err = HttpConfig::build(
            "127.0.0.1:0".parse().unwrap(),
            hashes(&["agent-one"]),
            TokenHash::of("approver"),
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err, HttpConfigError::EmptyAllowedHosts);
    }

    #[test]
    fn build_succeeds_with_distinct_agent_and_approver_hashes_and_a_host() {
        let config = HttpConfig::build(
            "127.0.0.1:0".parse().unwrap(),
            hashes(&["agent-one", "agent-two"]),
            TokenHash::of("approver"),
            vec!["example.com".to_string()],
        )
        .unwrap();
        assert_eq!(config.agent_token_hashes.len(), 2);
    }
}
