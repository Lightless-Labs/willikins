//! In-memory state for every fake tool, seedable from JSON.
//!
//! Keyed maps use each resource's own canonical string (or a documented
//! join of two canonical strings) as the map key, so a seeded state file is
//! readable without any extra decoding step. A secret value is stored
//! through its own domain type (never a bare string) and the map holding
//! [`DopplerSecretValue`]s implements [`serde::Serialize`] by hand so a
//! re-serialized [`FakeState`] never prints the seeded bytes.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use willikins_types::{
    ActionsSecretName, DopplerConfig, DopplerProject, DopplerSecretValue, DopplerTokenName,
    GitHubRepo, ProjectSlug, RepoVisibility, SecretName,
};

/// A GitHub repository record: enough to answer `github.repo.ensure`'s
/// `read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepoRecord {
    /// The repository's visibility.
    pub visibility: RepoVisibility,
    /// Whether this repository was created by us (`false` means the
    /// natural key exists but the resource is [`Foreign`](willikins_core::Observation::Foreign)).
    pub ours: bool,
}

/// A Doppler project record: enough to answer `doppler.project.ensure`'s
/// `read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DopplerProjectRecord {
    /// Whether this project was created by us.
    pub ours: bool,
}

/// A map from a Doppler secret's key (`project/config#SECRET`) to its
/// seeded value.
///
/// [`DopplerSecretValue`] implements no [`Serialize`] at all (per the
/// domain type model, no secret type does), so this wrapper implements it
/// by hand: every value is written through its own redacted [`Display`](std::fmt::Display),
/// never its raw bytes. `Debug` is derived from the inner map, which is
/// already redacted the same way because [`DopplerSecretValue`]'s own
/// `Debug` is redacted.
#[derive(Debug, Clone, Default)]
pub struct SecretsMap(HashMap<String, DopplerSecretValue>);

impl SecretsMap {
    /// The seeded value at `key`, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&DopplerSecretValue> {
        self.0.get(key)
    }

    /// Seed `value` at `key`, returning the value it replaced, if any.
    pub fn insert(&mut self, key: String, value: DopplerSecretValue) -> Option<DopplerSecretValue> {
        self.0.insert(key, value)
    }
}

impl Serialize for SecretsMap {
    /// Writes every value through its own [`Display`](std::fmt::Display),
    /// which for a secret type is always the redaction marker
    /// (`"[REDACTED DopplerSecretValue]"`), never the seeded bytes.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, &value.to_string())?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SecretsMap {
    /// Deserializes each value through [`DopplerSecretValue`]'s own
    /// `Deserialize`, which parses it directly from the JSON string rather
    /// than through the type registry (which refuses secret literals
    /// outright).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = HashMap::<String, DopplerSecretValue>::deserialize(deserializer)?;
        Ok(Self(inner))
    }
}

/// The shared, in-memory state behind every fake tool.
///
/// Cheap to construct empty ([`Self::new`]) or seed from a JSON file
/// ([`Self::from_json`]); every field defaults to empty so a seed file may
/// mention only the resources a test cares about. Shared between tools
/// through `Arc<Mutex<FakeState>>`.
///
/// An *unrecognised* field is refused rather than ignored
/// (`deny_unknown_fields`, which composes with the defaults above): a
/// misspelled key used to leave the resource it meant to seed absent, so
/// `plan` reported `Create` where the author had asked for `NoOp` and
/// nothing anywhere said why. Adversarial pass 2, finding 3.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FakeState {
    /// GitHub repositories, keyed by [`GitHubRepo`]'s canonical string.
    pub github_repos: HashMap<String, GitHubRepoRecord>,
    /// GitHub Actions repository secrets that exist, keyed by
    /// `"<repo>#<SECRET_NAME>"`. Membership is the only fact recorded: a
    /// secret's value is never stored.
    pub github_actions_secrets: HashSet<String>,
    /// Doppler projects, keyed by [`DopplerProject`]'s canonical string.
    pub doppler_projects: HashMap<String, DopplerProjectRecord>,
    /// Doppler configs that exist, keyed by [`DopplerConfig`]'s canonical
    /// string (`"<project>/<name>"`).
    pub doppler_configs: HashSet<String>,
    /// Doppler service tokens that exist, keyed by
    /// `"<config>#<token name>"`. Membership is the only fact recorded: a
    /// service token's value cannot be re-read once issued.
    pub doppler_service_tokens: HashSet<String>,
    /// Doppler secrets, keyed by `"<config>#<SECRET_NAME>"`.
    pub doppler_secrets: SecretsMap,
    /// The [`ProjectSlug`] canonical strings `fake.irreversible.ensure`
    /// has created.
    pub irreversible: HashSet<String>,
}

/// The key `github.repo.ensure` looks a [`GitHubRepo`] up by: its own
/// canonical string.
#[must_use]
pub fn repo_key(repo: &GitHubRepo) -> String {
    repo.to_string()
}

/// The key `github.actions_secret.ensure` looks a secret up by.
#[must_use]
pub fn actions_secret_key(repo: &GitHubRepo, name: &ActionsSecretName) -> String {
    format!("{repo}#{name}")
}

/// The key `doppler.project.ensure` looks a [`DopplerProject`] up by: its
/// own canonical string.
#[must_use]
pub fn doppler_project_key(project: &DopplerProject) -> String {
    project.to_string()
}

/// The key `doppler.config.ensure` looks a [`DopplerConfig`] up by: its own
/// canonical string.
#[must_use]
pub fn doppler_config_key(config: &DopplerConfig) -> String {
    config.to_string()
}

/// The key `doppler.service_token.ensure` looks a service token up by.
#[must_use]
pub fn doppler_service_token_key(config: &DopplerConfig, name: &DopplerTokenName) -> String {
    format!("{config}#{name}")
}

/// The key `doppler.secret.get` looks a secret value up by.
#[must_use]
pub fn doppler_secret_key(config: &DopplerConfig, name: &SecretName) -> String {
    format!("{config}#{name}")
}

/// The key `fake.irreversible.ensure` looks its resource up by: the
/// [`ProjectSlug`]'s own canonical string.
#[must_use]
pub fn irreversible_key(slug: &ProjectSlug) -> String {
    slug.to_string()
}

impl FakeState {
    /// An empty state: every resource absent.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a state from a JSON document, such as a `--fake-state` file.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `json` is not a valid `FakeState`
    /// document.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Seed a GitHub repository.
    #[must_use]
    pub fn with_repo(mut self, repo: &GitHubRepo, visibility: RepoVisibility, ours: bool) -> Self {
        self.github_repos
            .insert(repo_key(repo), GitHubRepoRecord { visibility, ours });
        self
    }

    /// Seed a GitHub Actions repository secret's existence (never a value).
    #[must_use]
    pub fn with_actions_secret(mut self, repo: &GitHubRepo, name: &ActionsSecretName) -> Self {
        self.github_actions_secrets
            .insert(actions_secret_key(repo, name));
        self
    }

    /// Seed a Doppler project.
    #[must_use]
    pub fn with_doppler_project(mut self, project: &DopplerProject, ours: bool) -> Self {
        self.doppler_projects
            .insert(doppler_project_key(project), DopplerProjectRecord { ours });
        self
    }

    /// Seed a Doppler config's existence.
    #[must_use]
    pub fn with_doppler_config(mut self, config: &DopplerConfig) -> Self {
        self.doppler_configs.insert(doppler_config_key(config));
        self
    }

    /// Seed a Doppler service token's existence (never a value).
    #[must_use]
    pub fn with_doppler_service_token(
        mut self,
        config: &DopplerConfig,
        name: &DopplerTokenName,
    ) -> Self {
        self.doppler_service_tokens
            .insert(doppler_service_token_key(config, name));
        self
    }

    /// Seed a Doppler secret's value.
    #[must_use]
    pub fn with_doppler_secret(
        mut self,
        config: &DopplerConfig,
        name: &SecretName,
        value: DopplerSecretValue,
    ) -> Self {
        self.doppler_secrets
            .insert(doppler_secret_key(config, name), value);
        self
    }

    /// Seed `fake.irreversible.ensure`'s resource for `slug`.
    #[must_use]
    pub fn with_irreversible(mut self, slug: &ProjectSlug) -> Self {
        self.irreversible.insert(irreversible_key(slug));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::DomainType;

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()
    }

    #[test]
    fn empty_state_round_trips_through_json() {
        let state = FakeState::new();
        let json = serde_json::to_string(&state).unwrap();
        let back = FakeState::from_json(&json).unwrap();
        assert!(back.github_repos.is_empty());
        assert!(back.doppler_secrets.get("anything").is_none());
    }

    #[test]
    fn from_json_of_an_empty_object_is_an_empty_state() {
        let state = FakeState::from_json("{}").unwrap();
        assert!(state.github_repos.is_empty());
        assert!(state.irreversible.is_empty());
    }

    #[test]
    fn builder_seeds_a_repo() {
        let state = FakeState::new().with_repo(&repo(), RepoVisibility::Private, true);
        let record = state.github_repos.get(&repo_key(&repo())).unwrap();
        assert_eq!(record.visibility, RepoVisibility::Private);
        assert!(record.ours);
    }

    #[test]
    fn builder_seeds_a_doppler_secret() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let name = SecretName::parse("DATABASE_URL").unwrap();
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        let state = FakeState::new().with_doppler_secret(&config, &name, value);
        let key = doppler_secret_key(&config, &name);
        assert!(state.doppler_secrets.get(&key).is_some());
    }

    #[test]
    fn seeded_secret_never_prints_in_debug() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let name = SecretName::parse("DATABASE_URL").unwrap();
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        let state = FakeState::new().with_doppler_secret(&config, &name, value);
        let debug = format!("{state:?}");
        assert!(!debug.contains("s3cr3t-value"), "debug leaked: {debug}");
        assert!(debug.contains("REDACTED"), "debug: {debug}");
    }

    #[test]
    fn seeded_secret_never_reserializes_its_value() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let name = SecretName::parse("DATABASE_URL").unwrap();
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        let state = FakeState::new().with_doppler_secret(&config, &name, value);
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("s3cr3t-value"), "json leaked: {json}");
        assert!(json.contains("REDACTED"), "json: {json}");
    }

    /// Re-serializing a state that holds a seeded secret writes the
    /// redaction marker in its place, so loading that dump back seeds the
    /// literal marker string as the "secret" (any non-empty string is a
    /// valid `DopplerSecretValue`). Expected: `FakeState`'s serialization
    /// is a one-way, redacted view, never a way to export and reimport
    /// real seeded values.
    #[test]
    fn reserializing_a_seeded_secret_stays_redacted_even_after_reloading_the_dump() {
        let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
        let name = SecretName::parse("DATABASE_URL").unwrap();
        let value = DopplerSecretValue::parse("s3cr3t-value").unwrap();
        let state = FakeState::new().with_doppler_secret(&config, &name, value);
        let json = serde_json::to_string(&state).unwrap();
        let back = FakeState::from_json(&json).unwrap();
        let json_again = serde_json::to_string(&back).unwrap();
        assert!(!json_again.contains("s3cr3t-value"));
        assert!(json_again.contains("REDACTED"));
    }

    #[test]
    fn actions_secret_existence_round_trips_through_json() {
        let repo = repo();
        let name = ActionsSecretName::parse("DOPPLER_TOKEN").unwrap();
        let state = FakeState::new().with_actions_secret(&repo, &name);
        let json = serde_json::to_string(&state).unwrap();
        let back = FakeState::from_json(&json).unwrap();
        assert!(
            back.github_actions_secrets
                .contains(&actions_secret_key(&repo, &name))
        );
    }
}
