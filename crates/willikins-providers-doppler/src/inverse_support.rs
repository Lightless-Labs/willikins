//! Shared helpers for this crate's `Inverse` implementations (milestone
//! 3m tasks P2, P3). `doppler.project.ensure`'s inverse
//! (`tools::project_ensure`) and task P3's `doppler.config.ensure`/
//! `doppler.branch_config.ensure` inverses each walk the same shapes --
//! a rider's own recorded identity, and one config's own contents
//! against what this run's riders are known to have written -- scoped
//! to a different set of configs (the whole project, one environment,
//! or one config). Hoisted here once P3 needed a second and third copy,
//! so a bug X1 finds in one walk is fixed in every caller at once,
//! rather than in whichever copy it happened to be reported against.
//!
//! `pub(crate)`: every item here is this crate's own cross-tool plumbing,
//! never part of a `Tool`'s own public surface.

use std::collections::{HashMap, HashSet};

use willikins_core::rollback::Rider;
use willikins_core::tool::helpers::get;
use willikins_core::{Outputs, ToolError};
use willikins_types::{DopplerConfig, DopplerConfigName, DopplerProject, SecretName};

use crate::client::{ConfigRefBody, DopplerClient};

/// The `config` *output* of a rider that only ever reports its identity
/// through an output port (`doppler.config.ensure`/`doppler.branch_config.ensure`:
/// their own `config` is derived from `project`+`environment`/`branch`,
/// never a port a document binds directly) -- `rider.outputs`, never
/// `rider.inputs`, and a downcast failure is read the same as "not
/// present": a rider this crate cannot identify tolerates nothing (fail
/// closed, never widened by guessing).
pub(crate) fn config_output(outputs: &Outputs) -> Option<DopplerConfig> {
    outputs
        .get(&willikins_core::tool::helpers::port("config"))
        .and_then(|value| value.downcast::<DopplerConfig>())
        .cloned()
}

/// Every rider `doppler.secret.set`'s own recorded `(config, name)` pair,
/// grouped by the config's own name. A rider whose own `config`/`name`
/// port this crate cannot read (fails to parse, or is unbound) tolerates
/// nothing, the same fail-closed reasoning [`config_output`] documents.
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
/// name)` pair, grouped by the config's own name -- the same shape
/// [`tolerated_secret_names`] builds, for token names instead.
pub(crate) fn tolerated_token_names(riders: &[Rider<'_>]) -> HashMap<String, HashSet<String>> {
    let mut by_config: HashMap<String, HashSet<String>> = HashMap::new();
    for rider in riders {
        if rider.tool != "doppler.service_token.ensure" {
            continue;
        }
        let Ok(config) = get::<DopplerConfig>(rider.inputs, "config") else {
            continue;
        };
        let Ok(name) = get::<willikins_types::DopplerTokenName>(rider.inputs, "name") else {
            continue;
        };
        by_config
            .entry(config.name().to_string())
            .or_default()
            .insert(name.as_str().to_string());
    }
    by_config
}

/// Why one config's own contents are not provably all this run's own
/// (decision 7's `inheritedBy`/secret-name/token rows), past the point
/// the caller has already decided the config *itself* is tolerated --
/// each caller maps this back to its own `&'static str` reason, scoped
/// to whatever this config sits inside (a project, an environment, or
/// just itself).
pub(crate) enum ConfigContentsDrift {
    /// A secret name this run's own riders did not write, directly or
    /// through an inherited base.
    Secret,
    /// The config is inherited by a config outside this project, or the
    /// response omitted `inheritedBy` outright (fail closed).
    Inheritor,
    /// A service token this run's own riders did not create.
    Token,
}

/// One config's own secret names, `inheritedBy`, and service tokens,
/// against `tolerated_secrets`/`tolerated_tokens` (each built by
/// [`tolerated_secret_names`]/[`tolerated_token_names`] and reduced to
/// this one config's own entry by its caller). `Ok(None)` is "nothing
/// foreign found". Shared by `doppler.project.ensure`'s inverse (one
/// call per config in the project) and `doppler.config.ensure`'s/
/// `doppler.branch_config.ensure`'s own inverses (task P3; one call per
/// config in the environment, or once for the one branch config).
pub(crate) fn config_contents_drift(
    client: &DopplerClient,
    project: &DopplerProject,
    config_name: &DopplerConfigName,
    tolerated_secrets: &HashSet<String>,
    tolerated_tokens: &HashSet<String>,
) -> Result<Option<ConfigContentsDrift>, ToolError> {
    let config_body = client.get_config(project, config_name)?;

    let bases: Vec<ConfigRefBody> = config_body.inherits.unwrap_or_default();
    if client.config_has_foreign_secret_name(project, config_name, tolerated_secrets, &bases)? {
        return Ok(Some(ConfigContentsDrift::Secret));
    }

    match &config_body.inherited_by {
        None => return Ok(Some(ConfigContentsDrift::Inheritor)),
        Some(entries) => {
            if entries
                .iter()
                .any(|entry| entry.project != project.as_str())
            {
                return Ok(Some(ConfigContentsDrift::Inheritor));
            }
        }
    }

    let tokens = client.list_service_tokens(project, config_name)?;
    if tokens
        .iter()
        .any(|token| !tolerated_tokens.contains(&token.name))
    {
        return Ok(Some(ConfigContentsDrift::Token));
    }

    Ok(None)
}
