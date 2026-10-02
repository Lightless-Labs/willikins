//! `buildkite.pipeline.bootstrap.ensure`: writes a willikins-owned
//! Buildkite pipeline's stored YAML `configuration`, and only from a
//! [`RepoFile`] the calling document renders -- never a literal, an
//! input, a default, or `Text` (milestone 3i decisions (a1)-(a6),
//! `docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md`).
//! Milestone 3a decision (a) and trust boundary 8 ("the crate contains no
//! `PATCH`, no shell, and no caller-supplied YAML") are superseded for
//! `configuration` only; trust boundary 7 (credential-bearing response
//! values never leave the client) is unchanged, and this tool restates it
//! by never deserializing the `PATCH` response at all
//! ([`BuildkiteClient::update_pipeline_configuration`]).
//!
//! # Only `configuration` is ever written (trust boundary 2)
//!
//! This tool's `ensure` sends exactly one `PATCH` body, `{"configuration":
//! "<content>"}`. It never sends `name`, `slug`, `steps`, `env`, or
//! `description` -- Buildkite's own documentation warns that a new `name`
//! without a `slug` regenerates the slug, which this tool must never
//! trigger. `buildkite.pipeline.ensure`'s own create-time constant
//! (`UPLOAD_CONFIGURATION`) is untouched; this tool only ever updates an
//! *existing* pipeline this crate already created.
//!
//! # Four states, one shared `analyze` (decision (a2))
//!
//! | State | When | `read` | `updates()` | `ensure` |
//! | --- | --- | --- | --- | --- |
//! | [`BootstrapState::Missing`] | `GET` answers `404` | `Absent` | `true` | [`willikins_core::ToolErrorKind::NotFound`] |
//! | [`BootstrapState::Foreign`] | `description` is not [`MANAGED_DESCRIPTION`] | `Foreign` | `false` | `Conflict` |
//! | [`BootstrapState::Equal`] | stored `configuration` structurally equals `configuration`'s content | `Present` | `false` | `changed: false`, no write |
//! | [`BootstrapState::Different`] | anything else, including a `null` or unparsable stored configuration | `Absent` | `true` | `PATCH`, then re-read |
//!
//! `Missing` reads `Absent` rather than failing at plan time
//! (`doppler.project_member.ensure`'s own reason, which this tool's
//! module doc restates): on a document's first run the upstream
//! `pipeline` node plans `Create` and this pipeline does not exist yet, so
//! a plan-time error here would make every fresh document unplannable.
//! A document that binds this tool with nothing creating the pipeline
//! still fails loudly, at apply, with `NotFound`.
//!
//! # The path and content rule (decision (a4))
//!
//! Before any HTTP call, `configuration`'s path must have a
//! second-to-last segment exactly `.buildkite` and a last segment ending
//! in `.yml` or `.yaml` (lowercase), and its content must parse as strict
//! YAML (a duplicate key on either side is a parse failure, the same rule
//! [`crate::tools::compare::structurally_equal`] already relies on) into
//! a mapping holding a non-empty `steps` sequence. Either failure is
//! [`willikins_core::ToolErrorKind::Invalid`] with a static message that
//! quotes neither the path nor the content.
//!
//! # Structural comparison, shared with the gate (decision (a3))
//!
//! [`crate::tools::compare::structurally_equal`] is the one function both
//! this tool and `buildkite.pipeline.bootstrap.gate` call, so the two can
//! never disagree about "equal" -- `tests/pipeline_bootstrap_ensure_mock.rs`
//! includes a gate-and-writer agreement table (acceptance 6).
//!
//! # Never echoed (trust boundary 4)
//!
//! The stored configuration and the `PATCH` response (which carries
//! `provider.webhook_url` and `configuration`) never reach an output, a
//! [`willikins_core::ToolError`] message, the journal, `tracing`, or a
//! `Debug`. [`crate::client::PipelineBootstrapBody`] has no `Debug`, and
//! `update_pipeline_configuration` discards the `PATCH` response entirely
//! (`serde::de::IgnoredAny`).

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, invalid, not_found, port, require_present, scalar,
    tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, BuildkiteToken, RepoFile};

use crate::client::{BuildkiteClient, MANAGED_DESCRIPTION, ScopedClient};
use crate::tools::compare::structurally_equal;

/// Decision (a2)'s four states, computed once by
/// [`BuildkitePipelineBootstrapEnsure::analyze`] and shared by `read`,
/// `updates`, and `ensure` -- so the three never re-derive or disagree
/// about which of the four applies.
enum BootstrapState {
    /// No pipeline at this key exists yet (`GET` answered `404`).
    Missing,
    /// A pipeline exists at this key, but its `description` is not
    /// [`MANAGED_DESCRIPTION`]: not willikins' own.
    Foreign,
    /// A pipeline exists, is ours, and its stored configuration already
    /// equals `configuration`'s content, structurally.
    Equal,
    /// A pipeline exists and is ours, but its stored configuration is
    /// anything else -- absent, `null`, unparsable, or merely different.
    Different,
}

/// `buildkite.pipeline.bootstrap.ensure`.
pub struct BuildkitePipelineBootstrapEnsure {
    spec: ToolSpec,
    client: Arc<BuildkiteClient>,
}

impl BuildkitePipelineBootstrapEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<BuildkiteClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("configuration"), exact("RepoFile", true));
        inputs.insert(port("token"), exact("BuildkiteToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.pipeline.bootstrap.ensure"),
                description: "Write a willikins-owned Buildkite pipeline's stored YAML \
                              configuration from a bootstrap file this document renders."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("org"), port("slug")],
                class: Class::Destructive,
                pure: false,
            },
            client,
        }
    }

    fn outputs_for(slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs
    }

    /// A static [`willikins_core::ToolErrorKind::Invalid`] naming neither
    /// the content nor the path -- shared by every branch of
    /// [`Self::validate_configuration`] that fails on content, so the
    /// message is identical regardless of *which* content rule tripped.
    fn content_invalid() -> ToolError {
        invalid(
            "this tool only writes a bootstrap whose content parses as strict YAML into a \
             mapping holding a non-empty `steps` sequence",
        )
    }

    /// Decision (a4): `configuration`'s path must have a second-to-last
    /// segment exactly `.buildkite` and a last segment ending in `.yml`
    /// or `.yaml` (both checks case-sensitive, lowercase only), and its
    /// content must parse as strict YAML (duplicate keys refused -- see
    /// [`crate::tools::compare::structurally_equal`]'s own doc for why
    /// that holds for [`serde_yaml_ng::Value`]) into a mapping holding a
    /// non-empty `steps` sequence. Raised before any HTTP call. Neither
    /// error message quotes the path or the content (trust boundary 4
    /// extends to the input side too: a rejected template should not
    /// become a place a secret-shaped `env` block gets echoed back).
    fn validate_configuration(configuration: &RepoFile) -> Result<(), ToolError> {
        let segments: Vec<&str> = configuration.path().segments().collect();
        // Deliberately case-sensitive (decision (a4)): `std::path::Path`'s
        // own case-insensitive comparison the lint suggests is exactly
        // the wrong tool here, since Buildkite's agent itself only looks
        // for a lowercase `.yml`/`.yaml` name.
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let valid_path = segments.len() >= 2
            && segments[segments.len() - 2] == ".buildkite"
            && segments
                .last()
                .is_some_and(|last| last.ends_with(".yml") || last.ends_with(".yaml"));
        if !valid_path {
            return Err(invalid(
                "this tool only writes a pipeline's configuration from a bootstrap file whose \
                 path is directly under a `.buildkite/` directory and ends in `.yml` or `.yaml`",
            ));
        }
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(configuration.content())
            .map_err(|_| Self::content_invalid())?;
        let has_steps = parsed
            .as_mapping()
            .and_then(|mapping| mapping.get("steps"))
            .and_then(serde_yaml_ng::Value::as_sequence)
            .is_some_and(|steps| !steps.is_empty());
        if !has_steps {
            return Err(Self::content_invalid());
        }
        Ok(())
    }

    /// Validate `configuration` (decision (a4)), then `GET` and classify
    /// into one of decision (a2)'s four states. Shared by `read`,
    /// `updates`, and `ensure`, all of which pass the [`ScopedClient`]
    /// this tool's own optional `token` port implies.
    fn analyze(
        client: &BuildkiteClient,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        configuration: &RepoFile,
    ) -> Result<BootstrapState, ToolError> {
        Self::validate_configuration(configuration)?;
        match client.get_pipeline_bootstrap(org, slug) {
            Ok(body) => {
                if body.description.as_deref() != Some(MANAGED_DESCRIPTION) {
                    return Ok(BootstrapState::Foreign);
                }
                let equal = body
                    .configuration
                    .as_deref()
                    .is_some_and(|stored| structurally_equal(stored, configuration.content()));
                Ok(if equal {
                    BootstrapState::Equal
                } else {
                    BootstrapState::Different
                })
            }
            Err(err) if err.status == Some(404) => Ok(BootstrapState::Missing),
            Err(err) => Err(err.into()),
        }
    }

    /// Read all four ports this tool's own spec declares, build the
    /// scoped client, and run [`Self::analyze`] -- the common prelude
    /// shared by `read`, `updates`, and `ensure`.
    fn analyze_from_inputs(
        &self,
        inputs: &Inputs,
    ) -> Result<(BootstrapState, BuildkitePipelineSlug), ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let configuration: RepoFile = get(inputs, "configuration")?;
        let token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        let state = Self::analyze(&client, &org, &slug, &configuration)?;
        Ok((state, slug))
    }
}

impl Tool for BuildkitePipelineBootstrapEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (state, slug) = self.analyze_from_inputs(inputs)?;
        Ok(match state {
            BootstrapState::Missing | BootstrapState::Different => Observation::Absent {
                predicted: Self::outputs_for(&slug),
            },
            BootstrapState::Equal => Observation::Present(Self::outputs_for(&slug)),
            BootstrapState::Foreign => Observation::Foreign,
        })
    }

    fn updates(&self, inputs: &Inputs) -> Result<bool, ToolError> {
        let (state, _slug) = self.analyze_from_inputs(inputs)?;
        Ok(matches!(
            state,
            BootstrapState::Missing | BootstrapState::Different
        ))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let configuration: RepoFile = get(inputs, "configuration")?;
        let bound_token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, bound_token.as_ref());
        match Self::analyze(&client, &org, &slug, &configuration)? {
            BootstrapState::Missing => Err(not_found(
                "this pipeline does not exist yet; this tool only writes the configuration of \
                 a pipeline another node has already created",
            )),
            BootstrapState::Foreign => Err(conflict(format!(
                "`{org}/{slug}` already exists and is not ours"
            ))),
            BootstrapState::Equal => Ok(Ensured {
                outputs: Self::outputs_for(&slug),
                changed: false,
            }),
            BootstrapState::Different => {
                match client.update_pipeline_configuration(&org, &slug, configuration.content()) {
                    Ok(()) => match Self::analyze(&client, &org, &slug, &configuration)? {
                        BootstrapState::Equal => Ok(Ensured {
                            outputs: Self::outputs_for(&slug),
                            changed: true,
                        }),
                        _ => Err(ToolError {
                            kind: ToolErrorKind::Provider,
                            message: "the stored configuration does not equal the \
                                          configuration written"
                                .to_string(),
                        }),
                    },
                    // Decision (a2): resolved the same way
                    // `doppler.project_member.ensure` resolves a failed
                    // write -- re-read; `Equal` means the write landed
                    // despite the ambiguous failure, anything else
                    // returns the original error.
                    Err(err) => match Self::analyze(&client, &org, &slug, &configuration)? {
                        BootstrapState::Equal => Ok(Ensured {
                            outputs: Self::outputs_for(&slug),
                            changed: false,
                        }),
                        _ => Err(err.into()),
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::{Credential, Http};
    use willikins_types::{DomainType, RepoPath};

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn slug() -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse("third-thoughts").unwrap()
    }

    fn valid_configuration() -> RepoFile {
        RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(port("org"), Value::known(org()));
        inputs.insert(port("slug"), Value::known(slug()));
        inputs.insert(port("configuration"), Value::known(valid_configuration()));
        inputs
    }

    fn tool_against(url: String) -> BuildkitePipelineBootstrapEnsure {
        let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
        let http = Http::new(url, Vec::new(), credential);
        BuildkitePipelineBootstrapEnsure::new(Arc::new(BuildkiteClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool_against("http://127.0.0.1:1".to_string())
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_key_is_org_and_slug_class_is_destructive_impure_no_gate() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        assert_eq!(tool.spec().key, vec![port("org"), port("slug")]);
        assert_eq!(tool.spec().class, Class::Destructive);
        assert!(!tool.spec().pure);
        assert!(tool.gate().is_none());
    }

    #[test]
    fn spec_output_is_slug_only_and_configuration_port_is_repo_file() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let spec = tool.spec();
        assert_eq!(spec.outputs.len(), 1);
        assert!(spec.outputs.contains_key(&port("slug")));
        let configuration_port = spec.inputs.get(&port("configuration")).unwrap();
        assert_eq!(
            configuration_port.ty,
            willikins_core::PortType::Exact(willikins_core::tool::helpers::scalar("RepoFile"))
        );
    }

    #[test]
    fn rejects_a_path_not_directly_under_a_buildkite_directory() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/plugins/stage-input/plugin.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        let err =
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
        assert!(!err.message.contains("stage-input"));
    }

    #[test]
    fn rejects_a_path_whose_parent_is_not_dot_buildkite() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_an_uppercase_extension() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.YML").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_an_uppercase_buildkite_directory_segment() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.Buildkite/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_a_non_yaml_extension() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/upload-pipeline.sh").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn accepts_a_two_segment_path() {
        let configuration = RepoFile::new(
            RepoPath::parse(".buildkite/pipeline.yaml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration).unwrap();
    }

    #[test]
    fn rejects_content_with_no_steps_key() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "env: {}\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_content_whose_steps_is_empty() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "steps: []\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_content_whose_top_level_is_a_sequence_not_a_mapping() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "- steps\n- more\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_unparsable_content() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "not: valid: yaml: at: all:",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_content_with_a_duplicate_top_level_key() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"a\"\nsteps:\n  - command: \"b\"\n",
        )
        .unwrap();
        assert_eq!(
            BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn accepts_json_shaped_content() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            r#"{"steps": [{"command": "echo hi"}]}"#,
        )
        .unwrap();
        BuildkitePipelineBootstrapEnsure::validate_configuration(&configuration).unwrap();
    }

    #[test]
    fn read_reports_absent_when_the_pipeline_does_not_exist_yet() {
        use willikins_providers_http::testing::MockProvider;
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(404)
            .with_body(serde_json::json!({"message": "not found"}).to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn validation_runs_before_any_http_call() {
        // No mock is registered at all: a request would fail the test
        // with a connection error, proving none was ever sent.
        let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        let tool = BuildkitePipelineBootstrapEnsure::new(Arc::new(BuildkiteClient::new(http)));
        let mut bad_inputs = inputs();
        bad_inputs.insert(
            port("configuration"),
            Value::known(
                RepoFile::new(
                    RepoPath::parse("apps/sample/bootstrap.yml").unwrap(),
                    "steps:\n  - command: \"echo hi\"\n",
                )
                .unwrap(),
            ),
        );
        let err = tool.read(&bad_inputs).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
    }
}
