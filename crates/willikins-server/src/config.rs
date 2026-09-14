//! [`ServerConfig::from_vars`]: build a server configuration from a pure
//! lookup function, never `std::env` directly -- the workspace forbids
//! `unsafe`, and `std::env::set_var`/`remove_var` are `unsafe` in edition
//! 2024, so a test that wants to exercise this has no safe way to mutate
//! real process environment variables anyway. A pure `impl Fn(&str) ->
//! Option<String>` sidesteps that entirely and is trivial to fake in a
//! test.
//!
//! Only the six variables task 10a owns are read here: the trusted
//! workflow directory, the journal path, the two windows, and the two
//! rate-limit rates. The token hashes, allowed hosts, and bind address
//! belong to task 10b's HTTP transport, not this library core.

use std::path::PathBuf;
use std::time::Duration;

use crate::butler::ButlerConfig;

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
        Ok(Self {
            workflows_dir,
            journal_path,
            approval_window,
            apply_window,
            plan_rate_per_minute,
            read_rate_per_minute,
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
        assert_eq!(config.approval_window, ButlerConfig::DEFAULT_APPROVAL_WINDOW);
        assert_eq!(config.apply_window, ButlerConfig::DEFAULT_APPLY_WINDOW);
        assert_eq!(
            config.plan_rate_per_minute,
            ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE
        );
        assert_eq!(
            config.read_rate_per_minute,
            ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE
        );
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
