//! `doppler.branch_config.ensure`: creates a real Doppler *branch*
//! config -- a config sitting under an environment, alongside its root
//! config, at a caller-chosen name -- filling the gap milestone 3e's plan
//! recorded (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, T3c's
//! addendum, "no tool can create a *named branch* Doppler config").
//!
//! # Why a new tool, not a port on `doppler.config.ensure`
//!
//! `doppler.config.ensure`'s own `Present` predicate is "`200` and `root:
//! true`"; a branch config's is "`200`, `root: false`, and this
//! environment". Those are not the same observation under an extra
//! conjunct (the shape `appstore.bundle_id_capability.ensure`'s optional
//! `setting` port already established, where a setting only narrows the
//! *same* endpoint's own predicate) -- they invert the flag the root tool
//! keys its whole identity on, and `ensure`'s create path calls a
//! different endpoint entirely (`POST /v3/environments` versus this
//! tool's `POST /v3/configs`). Three things already on `main` pin the
//! root tool's identity to "root, specifically": its own description
//! ("ensure an environment's root Doppler config exists"), the
//! architecture-gotchas entry in `docs/HANDOFF.md` ("`doppler.config.ensure`
//! needs `root: true`"), and the milestone 2 plan's own framing of this
//! exact gap ("creating a branch config -- the naming rule is settled,
//! **the tool is not**", `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`
//! line 1026). A tool whose observation contract flips depending on
//! whether an optional port is bound is two tools sharing one spec; this
//! crate keeps one tool per provider operation instead (the same reason
//! `appstore.app.get` and `appstore.app_group.gate` are their own tools
//! rather than options on `appstore.bundle_id.ensure`).
//!
//! # The name: a suffix port, not a full-name port
//!
//! Doppler's own create endpoint (`POST /v3/configs`, its `OpenAPI` spec's
//! `configs-create.md` fetched verbatim 2026-09-29) does **not** prefix a
//! branch config's name server-side -- settled empirically, not merely
//! read from docs, by the milestone 2 live write cycle
//! (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, "Notes for
//! milestone 3"): `name: "probe"` posted under environment `dev`
//! answered `400`, while `name: "dev_probe"` was stored as `dev_probe`
//! with `root: false`. So this tool's own `branch` input is the
//! **suffix** alone (`deployment_ios`), and [`Self::full_name`] assembles
//! `<environment's snake join>_<branch>` and re-parses it as a
//! [`DopplerConfigName`] before it ever reaches the wire -- the 60-
//! character "Config Slug" cap (which Doppler documents as *counting* the
//! environment prefix) then fails loudly at construction for a caller
//! whose branch name would not fit, rather than reaching Doppler as an
//! unparseable, silently-truncated, or ambiguously-refused request. This
//! mirrors `naming::v1::doppler_root_config`'s own snake join exactly,
//! but is not added to `naming::v1` (frozen) or a `v2` row: it is
//! provider mechanism (Doppler's own mandatory wire format), not
//! willikins policy, the same distinction that keeps this assembly here
//! rather than as a naming-table entry a document would otherwise have to
//! call explicitly for what is, for now, one document's one config.
//!
//! # Read: `root: false` **and** the right environment
//!
//! Existence at this literal name is not enough, for the same collision
//! reason `doppler.config.ensure`'s own doc gives: a genuinely different
//! config could occupy the exact string this tool derives. Two ways that
//! can happen for a branch config specifically -- a same-named branch
//! under a *different* environment (Doppler configs are named uniquely
//! within a project, not scoped per environment, so nothing prevents the
//! literal string colliding), or -- vanishingly unlikely, but checked for
//! the same reason the root tool checks its own flag -- an environment's
//! own *root* config whose name happens to equal what this tool derived.
//! `Present` therefore needs `root == Some(false)` **and** the fetched
//! config's own `environment` field to equal the one this tool was asked
//! to ensure under; anything else at `200` is `Foreign`, and a missing
//! project or config (`looks_like_a_missing_project`, the same predicate
//! `doppler.config.ensure` already applies to the identical endpoint) is
//! `Absent`.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug,
};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.branch_config.ensure`.
pub struct DopplerBranchConfigEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerBranchConfigEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        inputs.insert(port("branch"), exact("DopplerConfigName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.branch_config.ensure"),
                description:
                    "Ensure a named branch config exists under an environment's root config."
                        .to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment"), port("branch")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn key_ports(
        &self,
        inputs: &Inputs,
    ) -> Result<(DopplerProject, EnvironmentSlug, DopplerConfigName), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        let branch = get(inputs, "branch")?;
        Ok((project, environment, branch))
    }

    /// `<environment's snake join>_<branch>`, re-parsed as a
    /// [`DopplerConfigName`] -- see this module's own doc, "The name: a
    /// suffix port, not a full-name port".
    fn full_name(
        environment: &EnvironmentSlug,
        branch: &DopplerConfigName,
    ) -> Result<DopplerConfigName, ToolError> {
        let candidate = format!("{}_{branch}", environment.words().snake());
        DopplerConfigName::parse(&candidate).map_err(|err| {
            invalid(format!(
                "`{candidate}` (environment `{environment}` plus branch `{branch}`) is not a \
                 valid Doppler config name -- Doppler's 60-character \"Config Slug\" limit \
                 counts the environment prefix: {err}"
            ))
        })
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`. See this module's own doc, "Read: `root: false` and
    /// the right environment".
    fn observe(
        &self,
        environment: &EnvironmentSlug,
        config: &DopplerConfig,
    ) -> Result<Observation, ToolError> {
        let environment_str = environment.to_string();
        match self.client.get_config(config.project(), config.name()) {
            Ok(body)
                if body.root == Some(false)
                    && body.environment.as_deref() == Some(environment_str.as_str()) =>
            {
                Ok(Observation::Present(Self::outputs_for(config)))
            }
            Ok(_) => Ok(Observation::Foreign),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(config: &DopplerConfig) -> ToolError {
        conflict(format!(
            "`{config}` already exists and is not this environment's branch config of this name"
        ))
    }
}

impl Tool for DopplerBranchConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project, name);
        self.observe(&environment, &config)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project.clone(), name.clone());
        match self.observe(&environment, &config)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Foreign => Err(Self::foreign_conflict(&config)),
            Observation::Mismatch { .. } => unreachable!(
                "doppler.branch_config.ensure's own observe never returns Mismatch: it has no \
                 non-key input to mismatch on"
            ),
            Observation::Absent { .. } => {
                match self
                    .client
                    .create_branch_config(&project, &environment, &name)
                {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&config),
                        changed: true,
                    }),
                    // Doppler documents no error-body schema at all, so a
                    // conflict after a possibly delivered create cannot
                    // be told apart from a genuine failure by its body:
                    // re-read either way, exactly as the root config tool
                    // does.
                    Err(err) => match self.observe(&environment, &config)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Foreign => Err(Self::foreign_conflict(&config)),
                        Observation::Absent { .. } | Observation::Mismatch { .. } => {
                            Err(err.into())
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_providers_http::{Credential, Http};

    fn tool() -> DopplerBranchConfigEnsure {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        DopplerBranchConfigEnsure::new(Arc::new(DopplerClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn full_name_snake_joins_the_environment_and_the_branch() {
        let environment = EnvironmentSlug::parse("prd").unwrap();
        let branch = DopplerConfigName::parse("deployment_ios").unwrap();
        assert_eq!(
            DopplerBranchConfigEnsure::full_name(&environment, &branch)
                .unwrap()
                .as_str(),
            "prd_deployment_ios"
        );
    }

    #[test]
    fn full_name_snake_joins_a_multi_word_environment() {
        let environment = EnvironmentSlug::parse("pre-prod").unwrap();
        let branch = DopplerConfigName::parse("aws").unwrap();
        assert_eq!(
            DopplerBranchConfigEnsure::full_name(&environment, &branch)
                .unwrap()
                .as_str(),
            "pre_prod_aws"
        );
    }

    /// The 60-character "Config Slug" cap counts the environment prefix
    /// (Doppler's own platform-limits page); a branch long enough to
    /// blow the combined budget fails loudly here, never reaching the
    /// wire as an unparseable or silently truncated name.
    #[test]
    fn full_name_refuses_a_branch_too_long_once_the_prefix_is_added() {
        let environment = EnvironmentSlug::parse("prd").unwrap();
        // "prd_" (4) + 57 more characters == 61, one past the cap.
        let long_branch = DopplerConfigName::parse(&"a".repeat(57)).unwrap();
        let err = DopplerBranchConfigEnsure::full_name(&environment, &long_branch).unwrap_err();
        assert!(err.message.contains("60-character"), "{}", err.message);
    }

    #[test]
    fn spec_has_no_optional_ports() {
        let tool = tool();
        for (name, spec) in &tool.spec().inputs {
            assert!(spec.required, "port `{name}` must be required");
        }
        assert!(
            tool.spec()
                .inputs
                .contains_key(&PortName::parse("branch").unwrap())
        );
    }
}
