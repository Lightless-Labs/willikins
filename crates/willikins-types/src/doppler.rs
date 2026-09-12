//! Doppler domain types: projects, configs, tokens, and secrets.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::{DomainType, ParseError};

/// A Doppler project slug.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z0-9]+(?:-[a-z0-9]+)*",
    max_len = 64,
    description = "A Doppler project slug.",
    example = "third-thoughts"
)]
pub struct DopplerProject(String);

/// A Doppler config name, such as an environment's root config.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z0-9_]+",
    max_len = 64,
    description = "A Doppler config name.",
    example = "prd"
)]
pub struct DopplerConfigName(String);

/// A Doppler service token name.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z0-9]+(?:-[a-z0-9]+)*",
    max_len = 64,
    description = "A Doppler service token name.",
    example = "ci"
)]
pub struct DopplerTokenName(String);

/// A secret's name within a Doppler config.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Z_][A-Z0-9_]*",
    max_len = 256,
    description = "A secret's name within a Doppler config.",
    example = "DATABASE_URL"
)]
pub struct SecretName(String);

/// A Doppler service token value. Secret.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = r"dp\.st\.[A-Za-z0-9._-]{8,}",
    secret,
    description = "A Doppler service token value.",
    example = "dp.st.prd.exampleexampleexample"
)]
pub struct DopplerServiceToken(secrecy::SecretString);

/// A secret's value, as returned by `doppler.secret.get`. Secret.
#[derive(willikins_derive::DomainType)]
#[domain(
    min_len = 1,
    max_len = 65536,
    secret,
    description = "A secret's value.",
    example = "s3cr3t-value"
)]
pub struct DopplerSecretValue(secrecy::SecretString);

/// A Doppler config identity: `project/name`.
///
/// Hand-written because its canonical string is a documented join of two
/// other domain types, not a validated string in its own right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DopplerConfig {
    project: DopplerProject,
    name: DopplerConfigName,
}

/// `project/name`, both parts bounded by 64 characters, plus the separator.
const DOPPLER_CONFIG_MAX_LEN: usize = 64 + 1 + 64;

/// The published schema pattern: [`DopplerProject`]'s pattern, a literal
/// slash, then [`DopplerConfigName`]'s pattern, unanchored individually so
/// they combine into one whole-string match.
const DOPPLER_CONFIG_PATTERN: &str = r"^[a-z0-9]+(?:-[a-z0-9]+)*/[a-z0-9_]+$";

impl DopplerConfig {
    /// Build a config identity directly from its already-parsed parts.
    #[must_use]
    pub fn new(project: DopplerProject, name: DopplerConfigName) -> Self {
        Self { project, name }
    }

    /// The project this config belongs to.
    #[must_use]
    pub fn project(&self) -> &DopplerProject {
        &self.project
    }

    /// The config's name.
    #[must_use]
    pub fn name(&self) -> &DopplerConfigName {
        &self.name
    }
}

impl fmt::Display for DopplerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.project, self.name)
    }
}

impl FromStr for DopplerConfig {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for DopplerConfig {
    const TYPE_NAME: &'static str = "DopplerConfig";

    fn description() -> &'static str {
        "A Doppler config identity: project/name."
    }

    fn example() -> &'static str {
        "third-thoughts/prd"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.matches('/').count() != 1 {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must be exactly `project/name`, with a single `/`",
            ));
        }
        let (project, name) = input
            .split_once('/')
            .expect("checked above: input contains exactly one `/`");
        let project = DopplerProject::parse(project)
            .map_err(|err| ParseError::new(Self::TYPE_NAME, format!("project: {}", err.reason)))?;
        let name = DopplerConfigName::parse(name)
            .map_err(|err| ParseError::new(Self::TYPE_NAME, format!("name: {}", err.reason)))?;
        Ok(Self { project, name })
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for DopplerConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for DopplerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for DopplerConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("DopplerConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": DOPPLER_CONFIG_PATTERN,
            "maxLength": DOPPLER_CONFIG_MAX_LEN,
            "description": "A Doppler config identity: project/name.",
            "examples": ["third-thoughts/prd"]
        })
    }
}

crate::impl_domain_object_non_secret!(DopplerConfig);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DomainObject, SinkToken};

    // -------------------------------------------------------------
    // DopplerProject
    // -------------------------------------------------------------

    #[test]
    fn doppler_project_accepts_a_valid_slug() {
        assert_eq!(
            DopplerProject::parse("third-thoughts").unwrap().as_str(),
            "third-thoughts"
        );
    }

    #[test]
    fn doppler_project_rejects_uppercase() {
        assert!(DopplerProject::parse("Third-Thoughts").is_err());
    }

    #[test]
    fn doppler_project_rejects_leading_hyphen() {
        assert!(DopplerProject::parse("-third").is_err());
    }

    #[test]
    fn doppler_project_rejects_over_max_len() {
        let too_long = "a".repeat(65);
        assert!(DopplerProject::parse(&too_long).is_err());
    }

    #[test]
    fn doppler_project_accepts_exactly_max_len() {
        let at_limit = "a".repeat(64);
        assert!(DopplerProject::parse(&at_limit).is_ok());
    }

    #[test]
    fn doppler_project_serde_round_trips() {
        let value = DopplerProject::parse("third-thoughts").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"third-thoughts\"");
        assert_eq!(
            serde_json::from_str::<DopplerProject>(&json).unwrap(),
            value
        );
    }

    // -------------------------------------------------------------
    // DopplerConfigName
    // -------------------------------------------------------------

    #[test]
    fn doppler_config_name_accepts_underscores() {
        assert_eq!(
            DopplerConfigName::parse("dev_ci").unwrap().as_str(),
            "dev_ci"
        );
    }

    #[test]
    fn doppler_config_name_rejects_hyphens() {
        assert!(DopplerConfigName::parse("dev-ci").is_err());
    }

    #[test]
    fn doppler_config_name_rejects_uppercase() {
        assert!(DopplerConfigName::parse("PRD").is_err());
    }

    #[test]
    fn doppler_config_name_rejects_over_max_len() {
        let too_long = "a".repeat(65);
        assert!(DopplerConfigName::parse(&too_long).is_err());
    }

    #[test]
    fn doppler_config_name_serde_round_trips() {
        let value = DopplerConfigName::parse("dev_ci").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"dev_ci\"");
        assert_eq!(
            serde_json::from_str::<DopplerConfigName>(&json).unwrap(),
            value
        );
    }

    // -------------------------------------------------------------
    // DopplerTokenName
    // -------------------------------------------------------------

    #[test]
    fn doppler_token_name_accepts_a_valid_name() {
        assert_eq!(DopplerTokenName::parse("ci").unwrap().as_str(), "ci");
    }

    #[test]
    fn doppler_token_name_rejects_underscores() {
        assert!(DopplerTokenName::parse("ci_token").is_err());
    }

    #[test]
    fn doppler_token_name_rejects_over_max_len() {
        let too_long = "a".repeat(65);
        assert!(DopplerTokenName::parse(&too_long).is_err());
    }

    #[test]
    fn doppler_token_name_serde_round_trips() {
        let value = DopplerTokenName::parse("ci").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"ci\"");
        assert_eq!(
            serde_json::from_str::<DopplerTokenName>(&json).unwrap(),
            value
        );
    }

    // -------------------------------------------------------------
    // SecretName
    // -------------------------------------------------------------

    #[test]
    fn secret_name_accepts_a_valid_name() {
        assert_eq!(
            SecretName::parse("DATABASE_URL").unwrap().as_str(),
            "DATABASE_URL"
        );
    }

    #[test]
    fn secret_name_rejects_lowercase() {
        assert!(SecretName::parse("database_url").is_err());
    }

    #[test]
    fn secret_name_rejects_leading_digit() {
        assert!(SecretName::parse("1SECRET").is_err());
    }

    #[test]
    fn secret_name_rejects_over_max_len() {
        let too_long = "A".repeat(257);
        assert!(SecretName::parse(&too_long).is_err());
    }

    #[test]
    fn secret_name_accepts_exactly_max_len() {
        let at_limit = "A".repeat(256);
        assert!(SecretName::parse(&at_limit).is_ok());
    }

    #[test]
    fn secret_name_serde_round_trips() {
        let value = SecretName::parse("DATABASE_URL").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"DATABASE_URL\"");
        assert_eq!(serde_json::from_str::<SecretName>(&json).unwrap(), value);
    }

    // -------------------------------------------------------------
    // DopplerServiceToken (secret)
    // -------------------------------------------------------------

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn doppler_service_token_accepts_a_valid_token() {
        let value = DopplerServiceToken::parse("dp.st.prd.exampleexampleexample").unwrap();
        assert_eq!(
            value.expose(&SinkToken::new()),
            "dp.st.prd.exampleexampleexample"
        );
    }

    #[test]
    fn doppler_service_token_rejects_a_short_suffix() {
        assert!(DopplerServiceToken::parse("dp.st.short").is_err());
    }

    #[test]
    fn doppler_service_token_rejects_a_missing_prefix() {
        assert!(DopplerServiceToken::parse("hunter2hunter2hunter2").is_err());
    }

    #[test]
    fn doppler_service_token_debug_and_display_are_redacted_and_never_echo_input() {
        let value = DopplerServiceToken::parse("dp.st.prd.exampleexampleexample").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerServiceToken]");
        assert_eq!(value.to_string(), "[REDACTED DopplerServiceToken]");

        let err = DopplerServiceToken::parse("dp.st.short").unwrap_err();
        assert!(
            !err.reason.contains("short"),
            "reason leaked input: {:?}",
            err.reason
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn doppler_service_token_deserialize_works_and_stays_redacted() {
        let value: DopplerServiceToken =
            serde_json::from_str("\"dp.st.prd.exampleexampleexample\"").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerServiceToken]");
        assert_eq!(
            value.expose(&SinkToken::new()),
            "dp.st.prd.exampleexampleexample"
        );

        let err = serde_json::from_str::<DopplerServiceToken>("\"dp.st.short\"").unwrap_err();
        assert!(
            !err.to_string().contains("short"),
            "error leaked input: {err}"
        );
    }

    // -------------------------------------------------------------
    // DopplerSecretValue (secret)
    // -------------------------------------------------------------

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn doppler_secret_value_accepts_any_non_empty_string() {
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        assert_eq!(value.expose(&SinkToken::new()), "s3cr3t-value");
    }

    #[test]
    fn doppler_secret_value_rejects_empty() {
        assert!(DopplerSecretValue::parse("").is_err());
    }

    #[test]
    fn doppler_secret_value_rejects_over_max_len() {
        let too_long = "a".repeat(65537);
        assert!(DopplerSecretValue::parse(&too_long).is_err());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn doppler_secret_value_deserialize_works_and_stays_redacted() {
        let value: DopplerSecretValue = serde_json::from_str("\"s3cr3t-value\"").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerSecretValue]");
        assert_eq!(value.expose(&SinkToken::new()), "s3cr3t-value");

        // Reject on the length rule, with a distinctive marker inside the
        // over-limit input, and check the marker never reaches the error.
        let too_long = format!("MARKER-{}", "a".repeat(65537));
        let json = serde_json::to_string(&too_long).unwrap();
        let err = serde_json::from_str::<DopplerSecretValue>(&json).unwrap_err();
        assert!(
            !err.to_string().contains("MARKER"),
            "error leaked input: {err}"
        );
    }

    #[test]
    fn doppler_secret_value_is_redacted_and_never_echoes_the_input() {
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED DopplerSecretValue]");
        assert!(
            !serde_json::to_string(&value.render())
                .unwrap()
                .contains("s3cr3t")
        );
    }

    // -------------------------------------------------------------
    // DopplerConfig
    // -------------------------------------------------------------

    #[test]
    fn doppler_config_parses_project_slash_name() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        assert_eq!(config.project().as_str(), "third-thoughts");
        assert_eq!(config.name().as_str(), "prd");
        assert_eq!(config.to_string(), "third-thoughts/prd");
    }

    #[test]
    fn doppler_config_rejects_missing_slash() {
        let err = DopplerConfig::parse("third-thoughts").unwrap_err();
        assert_eq!(err.type_name, "DopplerConfig");
        assert!(err.reason.contains('/'));
    }

    #[test]
    fn doppler_config_rejects_more_than_one_slash() {
        assert!(DopplerConfig::parse("third-thoughts/prd/extra").is_err());
    }

    #[test]
    fn doppler_config_propagates_the_project_error_naming_which_part_failed() {
        let err = DopplerConfig::parse("Bad-Project/prd").unwrap_err();
        assert_eq!(err.type_name, "DopplerConfig");
        assert!(
            err.reason.starts_with("project:"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn doppler_config_propagates_the_name_error_naming_which_part_failed() {
        let err = DopplerConfig::parse("third-thoughts/Bad-Name").unwrap_err();
        assert_eq!(err.type_name, "DopplerConfig");
        assert!(
            err.reason.starts_with("name:"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn doppler_config_serde_round_trips() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(json, "\"third-thoughts/prd\"");
        assert_eq!(
            serde_json::from_str::<DopplerConfig>(&json).unwrap(),
            config
        );
    }

    #[test]
    fn doppler_config_schema_shape() {
        let schema = serde_json::to_value(DopplerConfig::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["maxLength"], 129);
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let pattern = regex::Regex::new(schema["pattern"].as_str().unwrap()).unwrap();
        assert!(pattern.is_match(&config.to_string()));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn doppler_config_domain_object_view() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let value: Box<dyn DomainObject> = Box::new(config.clone());
        assert_eq!(value.type_name(), "DopplerConfig");
        assert_eq!(value.expose(&SinkToken::new()), config.to_string());
    }

    // -------------------------------------------------------------
    // Catalog examples
    // -------------------------------------------------------------

    #[test]
    fn examples_parse_as_their_own_types() {
        crate::assert_example_parses::<DopplerProject>();
        crate::assert_example_parses::<DopplerConfigName>();
        crate::assert_example_parses::<DopplerTokenName>();
        crate::assert_example_parses::<SecretName>();
        crate::assert_example_parses::<DopplerServiceToken>();
        crate::assert_example_parses::<DopplerSecretValue>();
        crate::assert_example_parses::<DopplerConfig>();
    }
}
