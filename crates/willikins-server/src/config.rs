//! [`ServerConfig::from_vars`]: build a server configuration from a pure
//! lookup function, never `std::env` directly -- the workspace forbids
//! `unsafe`, and `std::env::set_var`/`remove_var` are `unsafe` in edition
//! 2024, so a test that wants to exercise this has no safe way to mutate
//! real process environment variables anyway. A pure `impl Fn(&str) ->
//! Option<String>` sidesteps that entirely and is trivial to fake in a
//! test.
//!
//! Task 10a's six variables are read here: the trusted workflow
//! directory, the journal path, the two windows, and the two rate-limit
//! rates. Task 10b added four more, permissively: `agent_token_hashes`,
//! `approver_token_hash`, `allowed_hosts`, and `port` are all optional
//! here (empty/`None` when unset) even though http mode cannot actually
//! start without the first three and without a bind address one way or
//! another -- this function has no way to know whether the caller is
//! about to start over stdio (which needs none of them) or http, so the
//! http-mode validation rules live in [`crate::http::HttpConfig::build`]
//! instead, which the binary calls only on the `serve --http` path.

use std::path::PathBuf;
use std::time::Duration;

use crate::butler::ButlerConfig;
use crate::http::TokenHash;

/// Why [`ServerConfig::from_vars`] refused. Never carries the malformed
/// value itself -- only the variable name -- so a caller that logs or
/// serializes this cannot accidentally echo a credential or other
/// sensitive text a variable happened to hold (this crate reads no
/// secret-typed variable itself, but the rule is cheap to keep uniform).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind")]
pub enum ConfigError {
    /// A required variable was not set (or was set to an empty string,
    /// treated the same as unset).
    Missing {
        /// The variable name.
        variable: &'static str,
    },
    /// A variable was set but its value did not parse as the type it
    /// controls (a non-numeric string for a seconds-or-count variable).
    Malformed {
        /// The variable name.
        variable: &'static str,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { variable } => write!(f, "{variable} is not set"),
            Self::Malformed { variable } => write!(f, "{variable} is set but not a valid number"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Everything [`crate::Butler`] needs, read from the environment (or any
/// pure lookup shaped like it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// `WILLIKINS_WORKFLOWS_DIR`. Required.
    pub workflows_dir: PathBuf,
    /// `WILLIKINS_JOURNAL_PATH`. Required.
    pub journal_path: PathBuf,
    /// `WILLIKINS_APPROVAL_WINDOW_SECONDS`. Defaults to
    /// [`ButlerConfig::DEFAULT_APPROVAL_WINDOW`].
    pub approval_window: Duration,
    /// `WILLIKINS_PLAN_TTL_SECONDS` -- the plan's own name for the apply
    /// window (see the "Plan identity" trust boundary: it bounds how
    /// stale an approved plan may be by the time it is applied). Defaults
    /// to [`ButlerConfig::DEFAULT_APPLY_WINDOW`].
    pub apply_window: Duration,
    /// `WILLIKINS_PLAN_RATE_PER_MINUTE`. Defaults to
    /// [`ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE`].
    pub plan_rate_per_minute: u32,
    /// `WILLIKINS_READ_RATE_PER_MINUTE`. Defaults to
    /// [`ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE`].
    pub read_rate_per_minute: u32,
    /// `WILLIKINS_AGENT_TOKEN_HASHES`, comma-separated 64-hex-character
    /// entries. Empty when unset -- refused only in http mode, by
    /// [`crate::http::HttpConfig::build`].
    pub agent_token_hashes: Vec<TokenHash>,
    /// `WILLIKINS_APPROVER_TOKEN_HASH`. `None` when unset -- required
    /// only in http mode.
    pub approver_token_hash: Option<TokenHash>,
    /// `WILLIKINS_ALLOWED_HOSTS`, comma-separated. Empty when unset --
    /// refused only in http mode.
    pub allowed_hosts: Vec<String>,
    /// `PORT`, used to build a bind address (`0.0.0.0:$PORT`) when
    /// `serve --http` is given no `--bind`. `None` when unset.
    pub port: Option<u16>,
}

impl ServerConfig {
    /// Build a `ServerConfig` from `lookup`, a pure stand-in for
    /// `std::env::var` (`|name| std::env::var(name).ok()` in production;
    /// a `HashMap` lookup in a test).
    ///
    /// An empty string is treated the same as an unset variable
    /// throughout, matching the CLI's own treatment of an absent
    /// `--input`: an operator's blank environment entry should not silently
    /// behave differently from a missing one.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Missing`] when `WILLIKINS_WORKFLOWS_DIR` or
    /// `WILLIKINS_JOURNAL_PATH` is not set; [`ConfigError::Malformed`]
    /// when a numeric variable is set but does not parse, naming the
    /// variable only, never its value.
    pub fn from_vars(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let workflows_dir = required_path(&lookup, "WILLIKINS_WORKFLOWS_DIR")?;
        let journal_path = required_path(&lookup, "WILLIKINS_JOURNAL_PATH")?;
        let approval_window = optional_seconds(
            &lookup,
            "WILLIKINS_APPROVAL_WINDOW_SECONDS",
            ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        )?;
        let apply_window = optional_seconds(
            &lookup,
            "WILLIKINS_PLAN_TTL_SECONDS",
            ButlerConfig::DEFAULT_APPLY_WINDOW,
        )?;
        let plan_rate_per_minute = optional_u32(
            &lookup,
            "WILLIKINS_PLAN_RATE_PER_MINUTE",
            ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        )?;
        let read_rate_per_minute = optional_u32(
            &lookup,
            "WILLIKINS_READ_RATE_PER_MINUTE",
            ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
        )?;
        let agent_token_hashes = optional_token_hashes(&lookup, "WILLIKINS_AGENT_TOKEN_HASHES")?;
        let approver_token_hash = optional_token_hash(&lookup, "WILLIKINS_APPROVER_TOKEN_HASH")?;
        let allowed_hosts = optional_host_list(&lookup, "WILLIKINS_ALLOWED_HOSTS");
        let port = optional_port(&lookup, "PORT")?;
        Ok(Self {
            workflows_dir,
            journal_path,
            approval_window,
            apply_window,
            plan_rate_per_minute,
            read_rate_per_minute,
            agent_token_hashes,
            approver_token_hash,
            allowed_hosts,
            port,
        })
    }
}

fn non_empty(lookup: &impl Fn(&str) -> Option<String>, variable: &str) -> Option<String> {
    lookup(variable).filter(|value| !value.is_empty())
}

fn required_path(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
) -> Result<PathBuf, ConfigError> {
    non_empty(lookup, variable)
        .map(PathBuf::from)
        .ok_or(ConfigError::Missing { variable })
}

fn optional_seconds(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
    default: Duration,
) -> Result<Duration, ConfigError> {
    match non_empty(lookup, variable) {
        None => Ok(default),
        Some(value) => value
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| ConfigError::Malformed { variable }),
    }
}

fn optional_u32(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
    match non_empty(lookup, variable) {
        None => Ok(default),
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| ConfigError::Malformed { variable }),
    }
}

/// Split `value` on commas, trimming whitespace and dropping empty
/// entries -- the one splitting rule every comma-separated variable in
/// this module follows (`WILLIKINS_AGENT_TOKEN_HASHES`,
/// `WILLIKINS_ALLOWED_HOSTS`).
fn comma_separated(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

fn optional_token_hashes(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
) -> Result<Vec<TokenHash>, ConfigError> {
    match non_empty(lookup, variable) {
        None => Ok(Vec::new()),
        Some(value) => comma_separated(&value)
            .map(TokenHash::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ConfigError::Malformed { variable }),
    }
}

fn optional_token_hash(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
) -> Result<Option<TokenHash>, ConfigError> {
    match non_empty(lookup, variable) {
        None => Ok(None),
        Some(value) => TokenHash::parse(value.trim())
            .map(Some)
            .map_err(|_| ConfigError::Malformed { variable }),
    }
}

fn optional_host_list(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
) -> Vec<String> {
    match non_empty(lookup, variable) {
        None => Vec::new(),
        Some(value) => comma_separated(&value).map(str::to_string).collect(),
    }
}

fn optional_port(
    lookup: &impl Fn(&str) -> Option<String>,
    variable: &'static str,
) -> Result<Option<u16>, ConfigError> {
    match non_empty(lookup, variable) {
        None => Ok(None),
        Some(value) => value
            .parse::<u16>()
            .map(Some)
            .map_err(|_| ConfigError::Malformed { variable }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn lookup(vars: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn every_variable_set_builds_a_config() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            ("WILLIKINS_APPROVAL_WINDOW_SECONDS", "3600"),
            ("WILLIKINS_PLAN_TTL_SECONDS", "120"),
            ("WILLIKINS_PLAN_RATE_PER_MINUTE", "5"),
            ("WILLIKINS_READ_RATE_PER_MINUTE", "30"),
        ]);
        let config = ServerConfig::from_vars(lookup(&vars)).unwrap();
        assert_eq!(config.workflows_dir, PathBuf::from("/wf"));
        assert_eq!(config.journal_path, PathBuf::from("/journal.jsonl"));
        assert_eq!(config.approval_window, Duration::from_secs(3600));
        assert_eq!(config.apply_window, Duration::from_secs(120));
        assert_eq!(config.plan_rate_per_minute, 5);
        assert_eq!(config.read_rate_per_minute, 30);
    }

    #[test]
    fn only_the_two_required_variables_still_builds_with_defaults() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
        ]);
        let config = ServerConfig::from_vars(lookup(&vars)).unwrap();
        assert_eq!(
            config.approval_window,
            ButlerConfig::DEFAULT_APPROVAL_WINDOW
        );
        assert_eq!(config.apply_window, ButlerConfig::DEFAULT_APPLY_WINDOW);
        assert_eq!(
            config.plan_rate_per_minute,
            ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE
        );
        assert_eq!(
            config.read_rate_per_minute,
            ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE
        );
        assert!(config.agent_token_hashes.is_empty());
        assert_eq!(config.approver_token_hash, None);
        assert!(config.allowed_hosts.is_empty());
        assert_eq!(config.port, None);
    }

    fn hash_hex(text: &str) -> String {
        use std::fmt::Write as _;
        TokenHash::of(text)
            .as_bytes()
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    #[test]
    fn http_mode_variables_parse_when_set() {
        let agent_one = hash_hex("agent-one");
        let agent_two = hash_hex("agent-two");
        let approver = hash_hex("approver");
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            (
                "WILLIKINS_AGENT_TOKEN_HASHES",
                &format!("{agent_one}, {agent_two}"),
            ),
            ("WILLIKINS_APPROVER_TOKEN_HASH", &approver),
            ("WILLIKINS_ALLOWED_HOSTS", "example.com, localhost:8080"),
            ("PORT", "8080"),
        ]);
        let config = ServerConfig::from_vars(lookup(&vars)).unwrap();
        assert_eq!(
            config.agent_token_hashes,
            vec![TokenHash::of("agent-one"), TokenHash::of("agent-two")]
        );
        assert_eq!(config.approver_token_hash, Some(TokenHash::of("approver")));
        assert_eq!(
            config.allowed_hosts,
            vec!["example.com".to_string(), "localhost:8080".to_string()]
        );
        assert_eq!(config.port, Some(8080));
    }

    #[test]
    fn a_malformed_agent_token_hash_is_refused_by_variable_name() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            ("WILLIKINS_AGENT_TOKEN_HASHES", "not-a-hash"),
        ]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Malformed {
                variable: "WILLIKINS_AGENT_TOKEN_HASHES"
            }
        );
    }

    #[test]
    fn a_malformed_approver_token_hash_is_refused_by_variable_name() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            ("WILLIKINS_APPROVER_TOKEN_HASH", "not-a-hash"),
        ]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Malformed {
                variable: "WILLIKINS_APPROVER_TOKEN_HASH"
            }
        );
    }

    #[test]
    fn a_malformed_port_is_refused_by_variable_name() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            ("PORT", "not-a-port"),
        ]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(err, ConfigError::Malformed { variable: "PORT" });
    }

    #[test]
    fn a_missing_workflows_dir_is_refused_by_name() {
        let vars = vars(&[("WILLIKINS_JOURNAL_PATH", "/journal.jsonl")]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Missing {
                variable: "WILLIKINS_WORKFLOWS_DIR"
            }
        );
    }

    #[test]
    fn a_missing_journal_path_is_refused_by_name() {
        let vars = vars(&[("WILLIKINS_WORKFLOWS_DIR", "/wf")]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Missing {
                variable: "WILLIKINS_JOURNAL_PATH"
            }
        );
    }

    #[test]
    fn an_empty_string_is_treated_as_unset() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", ""),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
        ]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Missing {
                variable: "WILLIKINS_WORKFLOWS_DIR"
            }
        );
    }

    /// A malformed numeric value is refused by variable name only -- the
    /// value itself (a distinctive marker here) never appears in the
    /// error's `Display` or its JSON.
    #[test]
    fn a_malformed_number_is_refused_by_name_without_echoing_the_value() {
        let vars = vars(&[
            ("WILLIKINS_WORKFLOWS_DIR", "/wf"),
            ("WILLIKINS_JOURNAL_PATH", "/journal.jsonl"),
            ("WILLIKINS_PLAN_RATE_PER_MINUTE", "not-a-number-MARKER"),
        ]);
        let err = ServerConfig::from_vars(lookup(&vars)).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Malformed {
                variable: "WILLIKINS_PLAN_RATE_PER_MINUTE"
            }
        );
        let text = err.to_string();
        assert!(!text.contains("not-a-number-MARKER"));
        assert!(text.contains("WILLIKINS_PLAN_RATE_PER_MINUTE"));
        let json = serde_json::to_value(&err).unwrap();
        assert!(!json.to_string().contains("not-a-number-MARKER"));
    }
}
