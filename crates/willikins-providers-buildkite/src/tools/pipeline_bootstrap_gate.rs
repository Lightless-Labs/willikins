//! `buildkite.pipeline.bootstrap.gate`: a gate over "this pipeline's
//! stored `configuration` equals the bootstrap `RepoFile` this document
//! renders" (`docs/plans/2026-09-30-milestone-3g-file-writing.md`,
//! decision (h), task B1).
//!
//! # Why this exists
//!
//! Milestone 3a decision (a) froze `buildkite.pipeline.ensure`'s own
//! `configuration` at the minimal "upload the real pipeline from the
//! repository" bootstrap, and decision (h) kept that *at the time*: no
//! tool took a pipeline configuration, a command, a step, or any YAML as
//! an input -- a pipeline configuration "becomes something an agent
//! machine executes ... the moment a build is triggered, with no diff in
//! between" (CLAUDE.md forbids exactly that shape of tool). What decision
//! (h) made automatable instead was the *check*: whether the operator had
//! already pasted the real bootstrap (Sample's own
//! `.buildkite/bootstrap.yml`, or any other document's) into the
//! pipeline's Settings -> Steps by hand. This gate reads the stored
//! configuration and compares it against the bootstrap file the calling
//! document renders (typically through `repo.file.render`, milestone 3g
//! task T1); it never writes anything, and its own `ensure` is a read.
//!
//! **Superseded for `configuration` only, milestone 3i decision (a1):**
//! the paste is no longer manual. `buildkite.pipeline.bootstrap.ensure`
//! (`crate::tools::pipeline_bootstrap_ensure`) now writes the stored
//! `configuration`, but only from a `RepoFile` the document renders --
//! never a literal, a command, a step, or any value from a workflow
//! input. This gate still exists for a document whose pipeline an
//! operator manages by hand instead.
//!
//! # Structural comparison, not byte comparison
//!
//! Buildkite's own documentation shows a stored `configuration` coming
//! back re-quoted (verify item 9, settled 2026-09-30: a live `GET` on
//! Sample's own pipeline answered 200 with a `configuration` field). Both
//! strings are parsed as YAML into a value and compared for structural
//! equality ([`crate::tools::compare::structurally_equal`], shared with
//! `buildkite.pipeline.bootstrap.ensure` since milestone 3i decision
//! (a3) so the two tools cannot disagree about "equal"), not compared
//! byte for byte. A stored configuration that fails to parse as YAML at
//! all is "different", never a hard error: the gate's only two outcomes
//! besides a genuine provider failure are "equal" and "not yet", exactly
//! like every other gate in this codebase (`appstore.app_group.gate`,
//! `doppler.config.inheritable.gate`).
//!
//! # `configuration` never escapes this module
//!
//! Trust boundary 6 of milestone 3a ("Buildkite's stored `configuration`
//! is compared, never written, echoed, logged or output") widens, only
//! for this one tool, to allow the read: [`crate::client::PipelineConfigurationBody`]
//! deserializes exactly that one field, carries no `Debug` impl, and
//! [`BuildkitePipelineBootstrapGate::observe`] (the only caller) compares
//! it and drops it before returning -- neither the stored value nor the
//! document's own rendered bootstrap ever reaches an output, a
//! [`willikins_core::ToolError`] message, or anything this process logs.
//! A stored configuration may carry an operator's own `env`, which is
//! exactly the thing this rule protects.
//!
//! # A gate over two ports, not one
//!
//! Unlike [`crate::tools::BuildkitePipelineEnsure`] (whose natural key is
//! also `org` plus `slug`), this tool has no key at all -- like every
//! other gate in this codebase, it is pure and never mutates anything, so
//! there is nothing to key. Its [`willikins_core::tool::Gate::subject`]
//! names both `org` and `slug` (not `expected`, which is a `RepoFile`: a
//! gate's `need`/`how` strings are `&'static str`, so they cannot be
//! authored from a caller's input, and a blocked report's own rendering
//! of `subject` is what lets an operator find the right pipeline without
//! this gate ever building a message from `expected`'s content).

use std::sync::Arc;

use willikins_core::tool::helpers::{
    exact, get, get_optional, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, BuildkiteToken, RepoFile};

use crate::client::{BuildkiteClient, ScopedClient};
use crate::tools::compare::structurally_equal;

/// This tool's one gate: `org` and `slug` together, so a blocked report
/// names exactly the pipeline the operator must paste the bootstrap into.
static GATE: Gate = Gate {
    need: "this pipeline's stored configuration equals the bootstrap this document renders",
    how: "in the pipeline's Settings, Steps page, replace the YAML with the rendered bootstrap \
          file this document committed, save, then re-run this document",
    subject: &["org", "slug"],
};

/// `buildkite.pipeline.bootstrap.gate`.
pub struct BuildkitePipelineBootstrapGate {
    spec: ToolSpec,
    client: Arc<BuildkiteClient>,
}

impl BuildkitePipelineBootstrapGate {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<BuildkiteClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("expected"), exact("RepoFile", true));
        inputs.insert(port("token"), exact("BuildkiteToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.pipeline.bootstrap.gate"),
                description: "A gate: whether a Buildkite pipeline's stored configuration \
                              equals the bootstrap a document renders."
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

    fn outputs_for(slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs
    }

    /// `GET` the pipeline's stored configuration and compare it against
    /// `expected`, mapped to an [`Observation`]. Shared by `read` and
    /// `ensure`. `Present` needs a stored configuration that parses and
    /// is structurally equal to `expected`'s content; a missing pipeline,
    /// an absent or differing configuration, or one that fails to parse
    /// are all `Absent` -- only a genuine provider failure escapes as an
    /// `Err`. See this module's own doc for why `configuration` itself
    /// never appears in either branch's own message.
    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let expected: RepoFile = get(inputs, "expected")?;
        let token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        match client.get_pipeline_configuration(&org, &slug) {
            Ok(body) => {
                let equal = body
                    .configuration
                    .as_deref()
                    .is_some_and(|stored| structurally_equal(stored, expected.content()));
                if equal {
                    Ok(Observation::Present(Self::outputs_for(&slug)))
                } else {
                    Ok(Observation::Absent {
                        predicted: Self::outputs_for(&slug),
                    })
                }
            }
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(&slug),
            }),
            Err(err) => Err(err.into()),
        }
    }
}

impl Tool for BuildkitePipelineBootstrapGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.observe(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // See `AppstoreAppGroupGate::ensure`'s own doc: a gate is pure, so
        // `apply` never reaches this while the node is `Action::Blocked`.
        match self.observe(inputs)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("this gate's own observe never returns {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_providers_http::testing::MockProvider;
    use willikins_providers_http::{Credential, Http};
    use willikins_types::{DomainType, RepoPath};

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn slug() -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse("third-thoughts").unwrap()
    }

    fn expected() -> RepoFile {
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
        inputs.insert(port("expected"), Value::known(expected()));
        inputs
    }

    fn tool_against(url: String) -> BuildkitePipelineBootstrapGate {
        let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_test");
        let http = Http::new(url, Vec::new(), credential);
        BuildkitePipelineBootstrapGate::new(Arc::new(BuildkiteClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool_against("http://127.0.0.1:1".to_string())
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure_and_reversible() {
        let spec = tool_against("http://127.0.0.1:1".to_string()).spec;
        assert!(spec.key.is_empty());
        assert!(spec.pure);
        assert_eq!(spec.class, Class::Reversible);
    }

    #[test]
    fn spec_output_is_slug_only() {
        let spec = tool_against("http://127.0.0.1:1".to_string()).spec;
        assert_eq!(spec.outputs.len(), 1);
        assert!(spec.outputs.contains_key(&port("slug")));
    }

    #[test]
    fn declares_a_gate_over_org_and_slug() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["org", "slug"]);
    }

    #[test]
    fn a_catalog_accepts_this_gate() {
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog
            .insert(std::sync::Arc::new(tool_against(
                "http://127.0.0.1:1".to_string(),
            )))
            .unwrap();
    }

    #[test]
    fn read_reports_present_when_the_stored_configuration_matches_structurally() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(
                serde_json::json!({
                    // Re-quoted, exactly the shape Buildkite's own
                    // documentation shows for a stored configuration --
                    // still structurally equal to `expected()`'s content.
                    "configuration": "steps:\n  - command: 'echo hi'\n",
                })
                .to_string(),
            )
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Present(_)),
            "{observation:?}"
        );
    }

    #[test]
    fn present_passes_slug_through_as_its_own_output() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(
                serde_json::json!({"configuration": "steps:\n  - command: \"echo hi\"\n"})
                    .to_string(),
            )
            .create();
        let tool = tool_against(provider.url());
        let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("slug").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts");
    }

    #[test]
    fn read_reports_absent_when_the_stored_configuration_differs() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(
                serde_json::json!({"configuration": "steps:\n  - command: \"buildkite-agent pipeline upload\"\n"})
                    .to_string(),
            )
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_the_stored_configuration_does_not_parse_as_yaml() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(
                serde_json::json!({"configuration": "not: valid: yaml: at: all:"}).to_string(),
            )
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_there_is_no_stored_configuration_at_all() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(serde_json::json!({"configuration": null}).to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_the_pipeline_does_not_exist_yet() {
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
    fn read_propagates_a_genuine_provider_failure_rather_than_blocking() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(500)
            .with_body(serde_json::json!({"message": "internal error"}).to_string())
            .create();
        let tool = tool_against(provider.url());
        let err = tool.read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Provider);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_never_writes_and_never_reports_changed() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body(
                serde_json::json!({"configuration": "steps:\n  - command: \"buildkite-agent pipeline upload\"\n"})
                    .to_string(),
            )
            .create();
        let tool = tool_against(provider.url());
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
        // Only the one `GET` above was mocked; a second, unexpected
        // request (a write this gate must never issue) would fail with a
        // connection or 501 error from the mock server, not silently
        // succeed.
    }
}
