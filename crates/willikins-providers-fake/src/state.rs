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

use willikins_core::{ToolError, ToolErrorKind};
use willikins_types::{
    ActionsSecretName, BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType,
    DopplerConfig, DopplerProject, DopplerSecretValue, DopplerServiceToken, DopplerTokenName,
    GitHubRepo, ProjectSlug, RepoVisibility, SecretName, SigNozIngestionKeyName,
    SigNozIngestionKeyValue, Text,
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

/// A Buildkite pipeline record: enough to answer
/// `buildkite.pipeline.ensure`'s `read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildkitePipelineRecord {
    /// The SSH repository URL this pipeline points at
    /// (`git@github.com:{owner}/{name}.git`), compared exactly against
    /// what `read` derives from the `repo` input, mirroring
    /// `willikins_providers_buildkite::ssh_repository_url`.
    pub repository: String,
    /// The cluster this pipeline belongs to, as [`crate::state`]'s own
    /// canonical string (`BuildkiteClusterId`'s `Display`).
    pub cluster_id: String,
    /// Whether this pipeline was created by us (`false` means the
    /// natural key exists but the resource is
    /// [`Foreign`](willikins_core::Observation::Foreign)).
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

/// A single, optionally seeded [`DopplerServiceToken`], consumed by
/// whichever mint reaches it first: [`FakeState::mint_token`], called
/// from `doppler.service_token.ensure`'s create path and from
/// `doppler.service_token.rotate`. [`Self::take`] clears it, so a second
/// mint in the same run never reuses a seeded marker.
///
/// Serializes the same one-way, redacted way [`SecretsMap`] does: `Some`
/// writes the value's own redaction marker (`"[REDACTED
/// DopplerServiceToken]"`), never its bytes; `None` writes JSON `null`.
/// Because the marker is not itself a valid `DopplerServiceToken` (its
/// pattern is `dp\.st\....`, not `[REDACTED ...]`), reloading a dump that
/// had a seeded `next_token` fails deserialization outright rather than
/// silently resurrecting the marker string as a "seeded" token — stricter
/// than [`SecretsMap`], whose value type ([`DopplerSecretValue`]) accepts
/// any non-empty string and so *would* reload the marker as if it were a
/// real seeded secret. Closes `todos/2026-09-12-fake-state-write-only.md`:
/// `FakeState`'s serialization is a one-way, redacted view, never an
/// export/import round trip, and this field makes that literally true for
/// at least one field rather than merely documented.
#[derive(Debug, Clone, Default)]
pub struct NextToken(Option<DopplerServiceToken>);

impl NextToken {
    /// Take the seeded value, if any, leaving `None` behind.
    fn take(&mut self) -> Option<DopplerServiceToken> {
        self.0.take()
    }
}

impl Serialize for NextToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Some(value) => serializer.serialize_str(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for NextToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = Option::<DopplerServiceToken>::deserialize(deserializer)?;
        Ok(Self(inner))
    }
}

/// [`NextToken`]'s own twin for [`SigNozIngestionKeyValue`]: a single,
/// optionally seeded value, consumed by [`FakeState::mint_signoz_key`].
/// Serializes the same one-way, redacted shape [`NextToken`] does, for
/// the identical reason.
#[derive(Debug, Clone, Default)]
pub struct NextSigNozKey(Option<SigNozIngestionKeyValue>);

impl NextSigNozKey {
    /// Take the seeded value, if any, leaving `None` behind.
    fn take(&mut self) -> Option<SigNozIngestionKeyValue> {
        self.0.take()
    }
}

impl Serialize for NextSigNozKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Some(value) => serializer.serialize_str(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for NextSigNozKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = Option::<SigNozIngestionKeyValue>::deserialize(deserializer)?;
        Ok(Self(inner))
    }
}

/// The key [`FakeState::fail_ensure_once`], [`FakeState::ensure_calls`],
/// and [`FakeState::read_calls`] all share: `"<tool>#<key>"`, where `<key>`
/// is the same key string the tool's own state map already uses (so a
/// caller who knows, say, [`doppler_service_token_key`] builds the same
/// string here without a second encoding).
#[must_use]
pub fn call_key(tool: &str, key: &str) -> String {
    format!("{tool}#{key}")
}

/// A generated [`DopplerServiceToken`] matching its own pattern (`dp.st.`
/// plus 40-44 alphanumeric characters), varied by `seed` so two mints in
/// the same run produce different values without needing a real random
/// number generator. Never `unwrap`s: a parse failure here would be this
/// function's own bug, not a caller's, so it panics with a clear message
/// the same way `doppler_project_ensure`'s `seed_default_configs` does for
/// its own always-valid input.
fn generate_token(seed: &str) -> DopplerServiceToken {
    use std::collections::hash_map::DefaultHasher;
    use std::fmt::Write as _;
    use std::hash::{Hash, Hasher};

    let mut body = String::with_capacity(48);
    for salt in 0u8..3 {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        salt.hash(&mut hasher);
        let _ = write!(body, "{:016x}", hasher.finish());
    }
    body.truncate(42);
    DopplerServiceToken::parse(&format!("dp.st.{body}")).unwrap_or_else(|err| {
        unreachable!("a generated fake token must match its own pattern: {err}")
    })
}

/// [`generate_token`]'s own twin for [`SigNozIngestionKeyValue`], whose
/// pattern is far more permissive (no fixed prefix, minimum length 1) —
/// the generated body is still varied by `seed` the same way, for the
/// same reason.
fn generate_signoz_key(seed: &str) -> SigNozIngestionKeyValue {
    use std::collections::hash_map::DefaultHasher;
    use std::fmt::Write as _;
    use std::hash::{Hash, Hasher};

    let mut body = String::with_capacity(32);
    for salt in 0u8..2 {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        salt.hash(&mut hasher);
        let _ = write!(body, "{:016x}", hasher.finish());
    }
    SigNozIngestionKeyValue::parse(&format!("fake-signoz-key-{body}")).unwrap_or_else(|err| {
        unreachable!("a generated fake `SigNoz` key must match its own pattern: {err}")
    })
}

/// Bump `map`'s counter at `tool`/`key` by one, returning the count after
/// this call (including it).
fn record_call(map: &mut HashMap<String, u32>, tool: &str, key: &str) -> u32 {
    let entry = map.entry(call_key(tool, key)).or_insert(0);
    *entry += 1;
    *entry
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
///
/// Serialization is a one-way, redacted view, never an export/import
/// round trip: every seeded secret ([`Self::doppler_secrets`],
/// [`Self::next_token`]) prints only its redaction marker, and a dump
/// holding a seeded [`Self::next_token`] fails to reload at all (see
/// [`NextToken`]'s own doc). A `--fake-state` file is written by hand or
/// generated from a template, never round-tripped through a running
/// state.
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
    /// Doppler configs marked inheritable, keyed the same way as
    /// [`Self::doppler_configs`]. Membership is the only fact recorded.
    pub doppler_config_inheritable: HashSet<String>,
    /// What each Doppler config inherits: [`doppler_config_key`] to the
    /// set of base configs' own canonical strings. A config absent from
    /// this map inherits nothing, the same as an empty set.
    pub doppler_config_inherits: HashMap<String, HashSet<String>>,
    /// Doppler service tokens that exist, keyed by
    /// `"<config>#<token name>"`. Membership is the only fact recorded: a
    /// service token's value cannot be re-read once issued.
    pub doppler_service_tokens: HashSet<String>,
    /// Doppler secrets, keyed by `"<config>#<SECRET_NAME>"`.
    pub doppler_secrets: SecretsMap,
    /// Doppler variables read as non-secret [`Text`] by
    /// `doppler.value.get`, keyed the same way as [`Self::doppler_secrets`]
    /// (`doppler_secret_key`). Deliberately a separate map rather than a
    /// second read of `doppler_secrets`: `DopplerSecretValue` offers no
    /// plaintext accessor outside a `SinkToken`-gated `expose` (this
    /// crate never mints one outside its own tests), so there is no
    /// token-less way for this map to be derived from that one. A live
    /// Doppler variable answers both tools identically because they hit
    /// the same endpoint; a fake-state seed file that wants both
    /// `doppler.secret.get` and `doppler.value.get` to agree on one
    /// variable must seed both maps at the same key with the same text --
    /// this crate does not enforce that agreement, the same way it does
    /// not enforce any other seeded fixture's internal consistency.
    pub doppler_values: HashMap<String, Text>,
    /// Keys `doppler.secret.set` has been called against, keyed the same
    /// way as [`Self::doppler_secrets`]. Deliberately a separate,
    /// value-free set rather than writing into [`Self::doppler_secrets`]
    /// itself: `doppler.secret.set`'s `read` never reports `Present` (see
    /// its live tool's own module docs), so nothing in this crate should
    /// let a document read back what it wrote through the fake provider
    /// either — a document composing `secret.set` then `secret.get` at
    /// the same key must see the same "no free read" shape the live
    /// providers give it.
    pub doppler_secret_writes: HashSet<String>,
    /// Buildkite clusters, keyed by their human-written name
    /// ([`BuildkiteClusterName::as_str`]) to a list of ids sharing that
    /// name -- a name is not a unique natural key (research note section
    /// 2), so a seed file can express the ambiguous case (two or more
    /// ids under one name) the same way the live provider's own
    /// pagination scan would find it.
    pub buildkite_clusters: HashMap<String, Vec<String>>,
    /// Buildkite pipelines, keyed by `"<org>/<slug>"`
    /// ([`buildkite_pipeline_key`]).
    pub buildkite_pipelines: HashMap<String, BuildkitePipelineRecord>,
    /// The [`ProjectSlug`] canonical strings `fake.irreversible.ensure`
    /// has created.
    pub irreversible: HashSet<String>,
    /// A token to hand out the next time something mints one
    /// (`doppler.service_token.ensure`'s create path, or
    /// `doppler.service_token.rotate`), consumed on first use. See
    /// [`NextToken`]'s own doc for why this seeds but never round-trips.
    pub next_token: NextToken,
    /// `SigNoz` ingestion keys that exist, keyed by
    /// [`SigNozIngestionKeyName`]'s own canonical string. Membership is
    /// the only fact recorded: an ingestion key's value cannot be
    /// re-read once minted, the same reason
    /// [`Self::doppler_service_tokens`] records membership alone.
    pub signoz_ingestion_keys: HashSet<String>,
    /// A key to hand out the next time `signoz.ingestion_key.ensure`'s
    /// create path mints one, consumed on first use. See
    /// [`NextSigNozKey`]'s own doc.
    pub next_signoz_key: NextSigNozKey,
    /// Pending injected failures, each `"<tool>#<key>"` ([`call_key`]).
    /// The next `ensure` matching an entry returns
    /// [`willikins_core::ToolErrorKind::Provider`] and consumes the entry
    /// (that one call only); state is left untouched by a failed call.
    pub fail_ensure_once: Vec<String>,
    /// How many times each `"<tool>#<key>"` has had `ensure` called
    /// against it, including calls an injected failure turned away.
    pub ensure_calls: HashMap<String, u32>,
    /// How many times each `"<tool>#<key>"` has had `read` called against
    /// it.
    pub read_calls: HashMap<String, u32>,
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

/// The key `signoz.ingestion_key.ensure` looks an ingestion key up by:
/// its own name (there is no compound key -- unlike a Doppler service
/// token, a `SigNoz` ingestion key is not scoped to a config).
#[must_use]
pub fn signoz_ingestion_key_key(name: &SigNozIngestionKeyName) -> String {
    name.to_string()
}

/// The key `buildkite.pipeline.ensure` looks a pipeline up by: its
/// `(org, slug)` natural key, joined the same way [`GitHubRepo`]'s own
/// canonical string joins its two parts.
#[must_use]
pub fn buildkite_pipeline_key(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> String {
    format!("{org}/{slug}")
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

    /// Seed a Doppler config as inheritable.
    #[must_use]
    pub fn with_doppler_config_inheritable(mut self, config: &DopplerConfig) -> Self {
        self.doppler_config_inheritable
            .insert(doppler_config_key(config));
        self
    }

    /// Seed what a Doppler config inherits, replacing any set already
    /// seeded for it (mirrors the live `POST`, which replaces the whole
    /// array rather than adding to it).
    #[must_use]
    pub fn with_doppler_config_inherits(
        mut self,
        config: &DopplerConfig,
        inherits: &[DopplerConfig],
    ) -> Self {
        self.doppler_config_inherits.insert(
            doppler_config_key(config),
            inherits.iter().map(doppler_config_key).collect(),
        );
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

    /// Seed a Doppler variable's value for `doppler.value.get`. See
    /// [`Self::doppler_values`] for why this is not derived from
    /// [`Self::with_doppler_secret`].
    #[must_use]
    pub fn with_doppler_value(
        mut self,
        config: &DopplerConfig,
        name: &SecretName,
        value: Text,
    ) -> Self {
        self.doppler_values
            .insert(doppler_secret_key(config, name), value);
        self
    }

    /// Seed a Buildkite cluster: `name` resolves to `id`, appended to any
    /// other id already seeded under the same name (so a second call with
    /// the same `name` and a different `id` seeds the ambiguous case).
    #[must_use]
    pub fn with_buildkite_cluster(mut self, name: &BuildkiteClusterName, id: &str) -> Self {
        self.buildkite_clusters
            .entry(name.as_str().to_string())
            .or_default()
            .push(id.to_string());
        self
    }

    /// Seed a Buildkite pipeline.
    #[must_use]
    pub fn with_buildkite_pipeline(
        mut self,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        record: BuildkitePipelineRecord,
    ) -> Self {
        self.buildkite_pipelines
            .insert(buildkite_pipeline_key(org, slug), record);
        self
    }

    /// Seed `fake.irreversible.ensure`'s resource for `slug`.
    #[must_use]
    pub fn with_irreversible(mut self, slug: &ProjectSlug) -> Self {
        self.irreversible.insert(irreversible_key(slug));
        self
    }

    /// Seed the next token a mint will hand out (see [`NextToken`]).
    #[must_use]
    pub fn with_next_token(mut self, token: DopplerServiceToken) -> Self {
        self.next_token = NextToken(Some(token));
        self
    }

    /// Seed a `SigNoz` ingestion key's existence (never a value).
    #[must_use]
    pub fn with_signoz_ingestion_key(mut self, name: &SigNozIngestionKeyName) -> Self {
        self.signoz_ingestion_keys
            .insert(signoz_ingestion_key_key(name));
        self
    }

    /// Seed the next `SigNoz` key a mint will hand out (see
    /// [`NextSigNozKey`]).
    #[must_use]
    pub fn with_next_signoz_key(mut self, key: SigNozIngestionKeyValue) -> Self {
        self.next_signoz_key = NextSigNozKey(Some(key));
        self
    }

    /// Seed a one-shot injected failure for the next `ensure` at
    /// `tool`/`key` ([`call_key`]).
    #[must_use]
    pub fn with_fail_ensure_once(mut self, tool: &str, key: &str) -> Self {
        self.fail_ensure_once.push(call_key(tool, key));
        self
    }

    /// Record one `read` call against `tool`/`key` ([`call_key`]).
    pub fn record_read_call(&mut self, tool: &str, key: &str) {
        record_call(&mut self.read_calls, tool, key);
    }

    /// Record one `ensure` call against `tool`/`key` ([`call_key`]),
    /// returning the count so far, including this one. Called
    /// unconditionally at the top of every non-pure fake tool's `ensure`,
    /// before checking [`Self::take_fail_ensure_once`], so an injected
    /// failure still counts as a call.
    pub fn record_ensure_call(&mut self, tool: &str, key: &str) -> u32 {
        record_call(&mut self.ensure_calls, tool, key)
    }

    /// If `tool`/`key` has a pending [`Self::fail_ensure_once`] entry,
    /// consume it and return the [`ToolError`] the caller should return
    /// instead of observing or mutating anything: an injected failure
    /// fires exactly once and leaves no other trace.
    #[must_use]
    pub fn take_fail_ensure_once(&mut self, tool: &str, key: &str) -> Option<ToolError> {
        let target = call_key(tool, key);
        let position = self
            .fail_ensure_once
            .iter()
            .position(|entry| entry == &target)?;
        self.fail_ensure_once.remove(position);
        Some(ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("injected failure for `{tool}` at `{key}`"),
        })
    }

    /// Mint a fresh [`DopplerServiceToken`] for `tool`/`key`
    /// ([`call_key`]): [`Self::next_token`] if seeded (consumed, so a
    /// second mint in the same run never reuses it), else a generated
    /// value varied by `call_count` so two mints for the same key differ.
    #[must_use]
    pub fn mint_token(&mut self, tool: &str, key: &str, call_count: u32) -> DopplerServiceToken {
        self.next_token
            .take()
            .unwrap_or_else(|| generate_token(&format!("{}#{call_count}", call_key(tool, key))))
    }

    /// [`Self::mint_token`]'s own twin for [`SigNozIngestionKeyValue`]:
    /// [`Self::next_signoz_key`] if seeded (consumed), else a generated
    /// value varied by `call_count`.
    #[must_use]
    pub fn mint_signoz_key(
        &mut self,
        tool: &str,
        key: &str,
        call_count: u32,
    ) -> SigNozIngestionKeyValue {
        self.next_signoz_key.take().unwrap_or_else(|| {
            generate_signoz_key(&format!("{}#{call_count}", call_key(tool, key)))
        })
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

    fn distinctive_token() -> DopplerServiceToken {
        DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7))).unwrap()
    }

    #[test]
    fn unseeded_mint_generates_a_validly_shaped_token_and_varies_by_call_count() {
        let mut state = FakeState::new();
        let first = state.mint_token("doppler.service_token.ensure", "cfg#ci", 1);
        let second = state.mint_token("doppler.service_token.ensure", "cfg#ci", 2);
        assert_ne!(first, second, "two mints of the same key must differ");
        assert_ne!(first, distinctive_token());
    }

    #[test]
    fn seeded_next_token_is_minted_once_then_generated() {
        let mut state = FakeState::new().with_next_token(distinctive_token());
        let first = state.mint_token("doppler.service_token.ensure", "cfg#ci", 1);
        assert_eq!(first, distinctive_token());
        let second = state.mint_token("doppler.service_token.ensure", "cfg#ci", 2);
        assert_ne!(
            second, first,
            "a second mint must never reuse the seeded marker"
        );
    }

    /// [`NextToken`]'s own doc: seeding it prints only the redaction
    /// marker, never the seeded bytes, and reloading that dump fails
    /// outright rather than resurrecting the marker as a "real" token.
    #[test]
    fn next_token_never_reserializes_its_value_and_its_dump_does_not_reload() {
        let state = FakeState::new().with_next_token(distinctive_token());
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("MARKERMARKER"), "json leaked: {json}");
        assert!(json.contains("REDACTED"), "json: {json}");
        let reloaded = FakeState::from_json(&json);
        assert!(
            reloaded.is_err(),
            "a dumped `next_token` marker is not itself a valid DopplerServiceToken"
        );
    }

    #[test]
    fn next_token_is_null_when_unseeded() {
        let state = FakeState::new();
        let json = serde_json::to_string(&state).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["next_token"], serde_json::Value::Null);
    }

    #[test]
    fn ensure_calls_and_read_calls_count_per_key() {
        let mut state = FakeState::new();
        state.record_read_call("github.repo.ensure", "acme/x");
        assert_eq!(state.record_ensure_call("github.repo.ensure", "acme/x"), 1);
        assert_eq!(state.record_ensure_call("github.repo.ensure", "acme/x"), 2);
        assert_eq!(
            state.ensure_calls[&call_key("github.repo.ensure", "acme/x")],
            2
        );
        assert_eq!(
            state.read_calls[&call_key("github.repo.ensure", "acme/x")],
            1
        );
    }

    #[test]
    fn fail_ensure_once_fires_exactly_once() {
        let mut state = FakeState::new().with_fail_ensure_once("doppler.config.ensure", "p/stg");
        let first = state.take_fail_ensure_once("doppler.config.ensure", "p/stg");
        assert!(first.is_some());
        assert_eq!(first.unwrap().kind, willikins_core::ToolErrorKind::Provider);
        let second = state.take_fail_ensure_once("doppler.config.ensure", "p/stg");
        assert!(second.is_none(), "an injected failure fires exactly once");
    }

    #[test]
    fn fail_ensure_once_never_matches_a_different_key_or_tool() {
        let mut state = FakeState::new().with_fail_ensure_once("doppler.config.ensure", "p/stg");
        assert!(
            state
                .take_fail_ensure_once("doppler.config.ensure", "p/prd")
                .is_none()
        );
        assert!(
            state
                .take_fail_ensure_once("doppler.project.ensure", "p/stg")
                .is_none()
        );
    }
}
