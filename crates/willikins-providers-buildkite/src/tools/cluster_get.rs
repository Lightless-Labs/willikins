//! `buildkite.cluster.get`: resolves a human-written Buildkite cluster
//! name to its opaque UUID. Pure and read-only, modelled one-for-one on
//! `doppler.secret.get`: `ensure` is the identity of `read`.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_types::{BuildkiteClusterId, BuildkiteClusterName, BuildkiteOrg, DomainType};

use crate::client::{BuildkiteClient, CLUSTERS_PER_PAGE, MAX_CLUSTER_PAGES};

/// `buildkite.cluster.get`.
pub struct BuildkiteClusterGet {
    spec: ToolSpec,
    client: Arc<BuildkiteClient>,
}

impl BuildkiteClusterGet {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<BuildkiteClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("name"), exact("BuildkiteClusterName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("cluster"), scalar("BuildkiteClusterId"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.cluster.get"),
                description: "Resolve a Buildkite cluster's human-written name to its id."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            client,
        }
    }

    /// Page through every cluster (stopping at the first short page or at
    /// [`MAX_CLUSTER_PAGES`]) collecting every cluster whose `name`
    /// equals `name`, and build this tool's one output from exactly one
    /// match.
    ///
    /// # Errors
    ///
    /// [`ToolErrorKind::NotFound`] naming `name` (zero matches),
    /// [`ToolErrorKind::Conflict`] naming `name` and the match count (two
    /// or more -- never the ids, since a mutable, non-unique name cannot
    /// be disambiguated by this tool), or
    /// [`ToolErrorKind::Provider`] naming the page bound (every page came
    /// back full) or a malformed id.
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let name: BuildkiteClusterName = get(inputs, "name")?;

        let mut matches: Vec<String> = Vec::new();
        let mut found_short_page = false;
        for page in 1..=MAX_CLUSTER_PAGES {
            let clusters = self.client.list_clusters_page(&org, page)?;
            let len = clusters.len();
            matches.extend(
                clusters
                    .into_iter()
                    .filter(|cluster| cluster.name == name.as_str())
                    .map(|cluster| cluster.id),
            );
            if len < CLUSTERS_PER_PAGE as usize {
                found_short_page = true;
                break;
            }
        }

        if !found_short_page {
            return Err(ToolError {
                kind: ToolErrorKind::Provider,
                message: format!(
                    "organisation `{org}` has more clusters than buildkite.cluster.get will \
                     page through (more than {} at {CLUSTERS_PER_PAGE} per page)",
                    MAX_CLUSTER_PAGES * CLUSTERS_PER_PAGE
                ),
            });
        }

        match matches.len() {
            0 => Err(not_found(format!("no Buildkite cluster named `{name}`"))),
            1 => {
                let id = matches.into_iter().next().expect("length checked above");
                let cluster = BuildkiteClusterId::parse(&id).map_err(|err| ToolError {
                    kind: ToolErrorKind::Provider,
                    message: format!("cluster `{name}` has a malformed id: {err}"),
                })?;
                let mut outputs = Outputs::new();
                outputs.insert(port("cluster"), Value::known(cluster));
                Ok(outputs)
            }
            count => Err(conflict(format!(
                "{count} Buildkite clusters are named `{name}`; this tool cannot disambiguate \
                 by name alone"
            ))),
        }
    }
}

impl Tool for BuildkiteClusterGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.lookup(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.lookup(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};

    fn tool() -> BuildkiteClusterGet {
        let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        BuildkiteClusterGet::new(Arc::new(BuildkiteClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key() {
        assert!(tool().spec().key.is_empty());
    }

    #[test]
    fn spec_is_pure() {
        assert!(tool().spec().pure);
    }
}
