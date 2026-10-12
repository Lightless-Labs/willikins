//! Shared helpers for this crate's fake `Inverse` implementations
//! (milestone 3m task P6): the same drift shapes
//! `willikins_providers_doppler::inverse_support` computes against a
//! real Doppler project, computed instead against [`FakeState`]'s own
//! maps -- one level of inheritance, "foreign secret name", "foreign
//! service token", and "inherited by something outside this config's
//! own project", scoped by a caller-given key prefix so one set of
//! helpers serves the project, config, and branch-config inverses alike
//! (task P6's own row: "fake inverses for the seven `Delete` tools").
//!
//! No timestamps exist anywhere in [`FakeState`] (milestone 3m's own row
//! for this task: "no timestamps: the fake skips the window rule, which
//! the mock tests pin instead"), so nothing here, or in any inverse that
//! calls it, ever consults a creation window; every `Standing::NotOurs`
//! row this crate's fakes report is an ownership check alone, never a
//! timing one.
//!
//! `pub(crate)`: this crate's own cross-tool plumbing, never part of a
//! `Tool`'s own public surface -- mirrors
//! `willikins_providers_doppler::inverse_support`'s identical visibility
//! and identical reason.

use std::collections::{HashMap, HashSet};

use willikins_core::rollback::Rider;
use willikins_core::tool::helpers::get;
use willikins_types::{DopplerConfig, DopplerTokenName, SecretName};

use crate::state::FakeState;

/// Every rider `doppler.secret.set`'s own recorded `(config, name)` pair,
/// grouped by the config's own bare name (never its full
/// `"project/name"` canonical string) -- mirrors the live crate's
/// `tolerated_secret_names` exactly, over the same rider shape.
pub(crate) fn tolerated_secret_names(riders: &[Rider<'_>]) -> HashMap<String, HashSet<String>> {
    let mut by_config: HashMap<String, HashSet<String>> = HashMap::new();
    for rider in riders {
        if rider.tool != "doppler.secret.set" {
            continue;
        }
        let Ok(config) = get::<DopplerConfig>(rider.inputs, "config") else {
            continue;
        };
        let Ok(name) = get::<SecretName>(rider.inputs, "name") else {
            continue;
        };
        by_config
            .entry(config.name().to_string())
            .or_default()
            .insert(name.as_str().to_string());
    }
    by_config
}

/// Every rider `doppler.service_token.ensure`'s own recorded `(config,
/// name)` pair, grouped the same way [`tolerated_secret_names`] does, for
/// token names instead.
pub(crate) fn tolerated_token_names(riders: &[Rider<'_>]) -> HashMap<String, HashSet<String>> {
    let mut by_config: HashMap<String, HashSet<String>> = HashMap::new();
    for rider in riders {
        if rider.tool != "doppler.service_token.ensure" {
            continue;
        }
        let Ok(config) = get::<DopplerConfig>(rider.inputs, "config") else {
            continue;
        };
        let Ok(name) = get::<DopplerTokenName>(rider.inputs, "name") else {
            continue;
        };
        by_config
            .entry(config.name().to_string())
            .or_default()
            .insert(name.as_str().to_string());
    }
    by_config
}

/// Why `config_key`'s own contents are not provably all this run's own
/// (decision 7's secret-name/`inheritedBy`/token rows), as the fake's own
/// model of them -- each caller maps this back to its own
/// [`willikins_core::rollback::Standing::Drifted`] reason.
pub(crate) enum ConfigContentsDrift {
    /// A secret name this run's own riders did not write, directly or
    /// through one inherited base.
    Secret,
    /// A config outside `config_key`'s own project inherits it.
    Inheritor,
    /// A service token this run's own riders did not create.
    Token,
}

/// Every name this fake crate's own `doppler.secret.set`/`doppler.secret.get`
/// seeding has recorded at `config_key`: the union of
/// [`FakeState::doppler_secret_writes`] and [`FakeState::doppler_secrets`],
/// both keyed `"<config>#<name>"`. Names only, never a value -- the same
/// shape the live provider's own read keeps.
fn secret_names_at(state: &FakeState, config_key: &str) -> HashSet<String> {
    let prefix = format!("{config_key}#");
    let mut names: HashSet<String> = state
        .doppler_secret_writes
        .iter()
        .filter_map(|key| key.strip_prefix(&prefix))
        .map(ToString::to_string)
        .collect();
    names.extend(
        state
            .doppler_secrets
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .map(ToString::to_string),
    );
    names
}

/// Every service token name recorded at `config_key`
/// ([`FakeState::doppler_service_tokens`], keyed `"<config>#<name>"`).
fn token_names_at(state: &FakeState, config_key: &str) -> HashSet<String> {
    let prefix = format!("{config_key}#");
    state
        .doppler_service_tokens
        .iter()
        .filter_map(|key| key.strip_prefix(&prefix))
        .map(ToString::to_string)
        .collect()
}

/// `config_key`'s own project, read off its canonical string
/// (`"project/name"`) -- never a second parse of a typed
/// [`willikins_types::DopplerProject`], since every caller here already
/// has only the bare key.
fn project_of_key(config_key: &str) -> &str {
    config_key.split('/').next().unwrap_or(config_key)
}

/// One config's own secret names, service tokens, and inheriting
/// configs, against `tolerated_secrets`/`tolerated_tokens` (each built by
/// [`tolerated_secret_names`]/[`tolerated_token_names`] and reduced to
/// this one config's own entry by its caller). `None` is "nothing
/// foreign found". Shared by the fake `doppler.project.ensure`,
/// `doppler.config.ensure`, and `doppler.branch_config.ensure` inverses
/// (task P6; one call per config under the project, or once for the one
/// environment/branch config).
pub(crate) fn config_contents_drift(
    state: &FakeState,
    config_key: &str,
    tolerated_secrets: &HashSet<String>,
    tolerated_tokens: &HashSet<String>,
) -> Option<ConfigContentsDrift> {
    let own_project = project_of_key(config_key);

    // Secret names: foreign unless a rider wrote it, or it is already
    // present on a base this config inherits (one level -- "Inherited
    // names", decision 7's own note).
    let bases = state
        .doppler_config_inherits
        .get(config_key)
        .cloned()
        .unwrap_or_default();
    let mut inherited_names: HashSet<String> = HashSet::new();
    for base in &bases {
        inherited_names.extend(secret_names_at(state, base));
    }
    for name in secret_names_at(state, config_key) {
        if !tolerated_secrets.contains(&name) && !inherited_names.contains(&name) {
            return Some(ConfigContentsDrift::Secret);
        }
    }

    // `inheritedBy`: any config outside this one's own project that
    // inherits it.
    let foreign_inheritor = state
        .doppler_config_inherits
        .iter()
        .any(|(inheritor, bases)| {
            bases.contains(config_key) && project_of_key(inheritor) != own_project
        });
    if foreign_inheritor {
        return Some(ConfigContentsDrift::Inheritor);
    }

    // Service tokens: foreign unless a rider created it.
    for name in token_names_at(state, config_key) {
        if !tolerated_tokens.contains(&name) {
            return Some(ConfigContentsDrift::Token);
        }
    }

    None
}

/// Remove everything this crate's state holds under `config_key`
/// (`"<config>#*"`: secrets, secret writes, variables, service tokens)
/// plus the config's own membership (`doppler_configs`, `inheritable`,
/// `inherits`) -- the cascade a config's own delete performs. Does not
/// touch any *other* config's own `inherits` entry that names
/// `config_key` as a base: a dangling base reference in a config outside
/// the one being deleted is exactly the [`ConfigContentsDrift::Inheritor`]
/// case `config_contents_drift` must already have refused before any
/// delete runs.
pub(crate) fn remove_config(state: &mut FakeState, config_key: &str) {
    let prefix = format!("{config_key}#");
    state.doppler_secrets.remove_prefixed(&prefix);
    state
        .doppler_secret_writes
        .retain(|key| !key.starts_with(&prefix));
    state
        .doppler_values
        .retain(|key, _| !key.starts_with(&prefix));
    state
        .doppler_service_tokens
        .retain(|key| !key.starts_with(&prefix));
    state.doppler_configs.remove(config_key);
    state.doppler_config_inheritable.remove(config_key);
    state.doppler_config_inherits.remove(config_key);
}
