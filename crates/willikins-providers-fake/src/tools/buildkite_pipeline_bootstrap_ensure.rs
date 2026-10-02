//! `buildkite.pipeline.bootstrap.ensure`: mirrors
//! `willikins_providers_buildkite::tools::BuildkitePipelineBootstrapEnsure`
//! -- see that crate's own module doc for the full reasoning (milestone
//! 3i decisions (a1)-(a6)). Not a leaf: it passes `slug` through as its
//! own output, exactly like the live tool.
//!
//! Shares decision (a2)'s four states with
//! [`crate::tools::buildkite_pipeline_bootstrap_gate`], over the same
//! seeded `BuildkitePipelineRecord` (`crate::state::BuildkitePipelineRecord`):
//! no record at all is `Missing`, a record whose `ours` is `false` is
//! `Foreign`, and a record whose `configuration` is structurally equal
//! (or not) to the bound `RepoFile`'s content is `Equal` or `Different`.
//! `structurally_equal` is this module's own local copy of the live
//! crate's function -- the same duplication
//! [`crate::tools::buildkite_pipeline_ensure`]'s own `ssh_repository_url`
//! copy makes, for the same reason (a fake tool never depends on its live
//! counterpart's crate); the path/content validation rule is likewise
//! this module's own local copy of the live tool's
//! `validate_configuration`.

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, BuildkiteToken, RepoFile};

use crate::state::{FakeState, buildkite_pipeline_key};
use crate::support::{
    conflict, exact, get, get_optional, invalid, not_found, port, require_present, scalar,
    tool_name,
};

/// Decision (a2)'s four states, shared by `read`, `updates`, and
/// `ensure` -- the fake's own copy of the live tool's `BootstrapState`.
enum BootstrapState {
    /// No pipeline is seeded at this key.
    Missing,
    /// A pipeline is seeded at this key, but its `ours` is `false`.
    Foreign,
    /// A pipeline is seeded, is ours, and its stored `configuration`
    /// already equals `configuration`'s content, structurally.
    Equal,
    /// A pipeline is seeded and is ours, but its stored `configuration`
    /// is anything else -- empty, unparsable, or merely different.
    Different,
}

/// Parse `a` and `b` as YAML into a JSON value and compare for
/// structural equality -- the fake's own copy of
/// `willikins_providers_buildkite::tools::compare::structurally_equal`.
/// See [`crate::tools::buildkite_pipeline_bootstrap_gate`]'s identical
/// copy for the full reasoning (a duplicate key is refused by
/// [`serde_yaml_ng::Value`], so a side that repeats a key is "different",
/// never a hard error).
fn structurally_equal(a: &str, b: &str) -> bool {
    if [a, b]
        .iter()
        .any(|side| serde_yaml_ng::from_str::<serde_yaml_ng::Value>(side).is_err())
    {
        return false;
    }
    let parsed_a = serde_yaml_ng::from_str::<serde_json::Value>(a);
    let parsed_b = serde_yaml_ng::from_str::<serde_json::Value>(b);
    matches!((parsed_a, parsed_b), (Ok(a), Ok(b)) if a == b)
}

/// `buildkite.pipeline.bootstrap.ensure`.
pub struct FakeBuildkitePipelineBootstrapEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeBuildkitePipelineBootstrapEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "buildkite.pipeline.bootstrap.ensure";

    /// Build the tool against `state`, constructing its spec. Field for
    /// field identical to the live tool's own `new` --
    /// `tests/catalog_parity.rs` pins the two equal.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("configuration"), exact("RepoFile", true));
        inputs.insert(port("token"), exact("BuildkiteToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Write a willikins-owned Buildkite pipeline's stored YAML \
                              configuration from a bootstrap file this document renders."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("org"), port("slug")],
                class: Class::Destructive,
                pure: false,
            },
            state,
        }
    }

    fn outputs_for(slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs
    }

    /// A static `Invalid`-kind error naming neither the content nor the
    /// path -- the fake's own copy of the live tool's identical helper.
    fn content_invalid() -> ToolError {
        invalid(
            "this tool only writes a bootstrap whose content parses as strict YAML into a \
             mapping holding a non-empty `steps` sequence",
        )
    }

    /// The fake's own copy of the live tool's `validate_configuration`:
    /// `configuration`'s path must have a second-to-last segment exactly
    /// `.buildkite` and a last segment ending in `.yml` or `.yaml` (both
    /// checks case-sensitive, lowercase only), and its content must parse
    /// as strict YAML (duplicate keys refused) into a mapping holding a
    /// non-empty `steps` sequence. Neither error message quotes the path
    /// or the content.
    fn validate_configuration(configuration: &RepoFile) -> Result<(), ToolError> {
        let segments: Vec<&str> = configuration.path().segments().collect();
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

    /// Validate `configuration` (mirroring the live tool's own
    /// `analyze`), then classify the seeded record into one of decision
    /// (a2)'s four states. Shared by `read`, `updates`, and `ensure`.
    fn analyze(
        state: &FakeState,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        configuration: &RepoFile,
    ) -> Result<BootstrapState, ToolError> {
        Self::validate_configuration(configuration)?;
        match state
            .buildkite_pipelines
            .get(&buildkite_pipeline_key(org, slug))
        {
            None => Ok(BootstrapState::Missing),
            Some(record) if !record.ours => Ok(BootstrapState::Foreign),
            Some(record) => {
                if structurally_equal(&record.configuration, configuration.content()) {
                    Ok(BootstrapState::Equal)
                } else {
                    Ok(BootstrapState::Different)
                }
            }
        }
    }
}

impl Tool for FakeBuildkitePipelineBootstrapEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let configuration: RepoFile = get(inputs, "configuration")?;
        let _token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &buildkite_pipeline_key(&org, &slug));
        let bootstrap_state = Self::analyze(&state, &org, &slug, &configuration)?;
        Ok(match bootstrap_state {
            BootstrapState::Missing | BootstrapState::Different => Observation::Absent {
                predicted: Self::outputs_for(&slug),
            },
            BootstrapState::Equal => Observation::Present(Self::outputs_for(&slug)),
            BootstrapState::Foreign => Observation::Foreign,
        })
    }

    fn updates(&self, inputs: &Inputs) -> Result<bool, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let configuration: RepoFile = get(inputs, "configuration")?;
        let _token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let state = self.state.lock().unwrap();
        let bootstrap_state = Self::analyze(&state, &org, &slug, &configuration)?;
        Ok(matches!(
            bootstrap_state,
            BootstrapState::Missing | BootstrapState::Different
        ))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let configuration: RepoFile = get(inputs, "configuration")?;
        let _bound_token: Option<BuildkiteToken> = get_optional(inputs, "token")?;
        let key = buildkite_pipeline_key(&org, &slug);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        match Self::analyze(&state, &org, &slug, &configuration)? {
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
                if let Some(record) = state.buildkite_pipelines.get_mut(&key) {
                    record.configuration = configuration.content().to_string();
                }
                Ok(Ensured {
                    outputs: Self::outputs_for(&slug),
                    changed: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, ToolErrorKind};
    use willikins_types::{DomainType, RepoPath};

    use crate::state::BuildkitePipelineRecord;

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn slug() -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse("third-thoughts").unwrap()
    }

    fn valid_configuration() -> RepoFile {
        RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
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

    fn tool(state: Arc<Mutex<FakeState>>) -> FakeBuildkitePipelineBootstrapEnsure {
        FakeBuildkitePipelineBootstrapEnsure::new(state)
    }

    fn foreign_record() -> BuildkitePipelineRecord {
        BuildkitePipelineRecord {
            repository: "git@github.com:lightless-labs/other.git".to_string(),
            cluster_id: "cluster".to_string(),
            ours: false,
            configuration: String::new(),
        }
    }

    fn owned_record(configuration: &str) -> BuildkitePipelineRecord {
        BuildkitePipelineRecord {
            repository: "git@github.com:lightless-labs/other.git".to_string(),
            cluster_id: "cluster".to_string(),
            ours: true,
            configuration: configuration.to_string(),
        }
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool(Arc::new(Mutex::new(FakeState::new())))
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_key_is_org_and_slug_class_is_destructive_impure_no_gate() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        assert_eq!(t.spec().key, vec![port("org"), port("slug")]);
        assert_eq!(t.spec().class, Class::Destructive);
        assert!(!t.spec().pure);
        assert!(t.gate().is_none());
    }

    #[test]
    fn spec_output_is_slug_only_and_configuration_port_is_repo_file() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let spec = t.spec();
        assert_eq!(spec.outputs.len(), 1);
        assert!(spec.outputs.contains_key(&port("slug")));
        let configuration_port = spec.inputs.get(&port("configuration")).unwrap();
        assert_eq!(
            configuration_port.ty,
            willikins_core::PortType::Exact(willikins_core::tool::helpers::scalar("RepoFile"))
        );
    }

    // -----------------------------------------------------------------
    // Decision (a2)'s four states.
    // -----------------------------------------------------------------

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn missing_reads_absent_updates_true_and_ensure_not_found() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
        assert!(t.updates(&inputs()).unwrap());
        let token = SinkToken::new();
        let err = t.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::NotFound);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn foreign_reads_foreign_updates_false_and_ensure_conflict() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            foreign_record(),
        )));
        let t = tool(state);
        assert!(matches!(t.read(&inputs()).unwrap(), Observation::Foreign));
        assert!(!t.updates(&inputs()).unwrap());
        let token = SinkToken::new();
        let err = t.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Conflict);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn equal_re_quoted_reads_present_updates_false_and_ensure_unchanged_with_no_write() {
        // Re-quoted, exactly the shape Buildkite's own documentation
        // shows for a stored configuration -- still structurally equal.
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            owned_record("steps:\n  - command: 'echo hi'\n"),
        )));
        let t = tool(state.clone());
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
        assert!(!t.updates(&inputs()).unwrap());
        let token = SinkToken::new();
        let ensured = t.ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
        assert_eq!(
            state
                .lock()
                .unwrap()
                .buildkite_pipelines
                .get(&buildkite_pipeline_key(&org(), &slug()))
                .unwrap()
                .configuration,
            "steps:\n  - command: 'echo hi'\n"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn different_reads_absent_updates_true_and_ensure_writes_then_changed_true() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            owned_record("steps:\n  - command: \"buildkite-agent pipeline upload\"\n"),
        )));
        let t = tool(state.clone());
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
        assert!(t.updates(&inputs()).unwrap());
        let token = SinkToken::new();
        let ensured = t.ensure(&inputs(), &token).unwrap();
        assert!(ensured.changed);
        assert_eq!(
            state
                .lock()
                .unwrap()
                .buildkite_pipelines
                .get(&buildkite_pipeline_key(&org(), &slug()))
                .unwrap()
                .configuration,
            valid_configuration().content()
        );
        // A second ensure is now unchanged, same as the live tool's own
        // re-read after a successful `PATCH`.
        let second = t.ensure(&inputs(), &token).unwrap();
        assert!(!second.changed);
    }

    #[test]
    fn null_stored_configuration_is_different() {
        // The fake's own record field is a plain `String` (`#[serde(default)]`
        // makes it empty rather than `Option`-null), but an empty stored
        // configuration is still structurally unequal to any real
        // bootstrap, exactly like the live tool's `null` case.
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            owned_record(""),
        )));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
        assert!(t.updates(&inputs()).unwrap());
    }

    #[test]
    fn present_passes_slug_through_as_its_own_output() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            owned_record("steps:\n  - command: \"echo hi\"\n"),
        )));
        let t = tool(state);
        let Observation::Present(outputs) = t.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("slug").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts");
    }

    // -----------------------------------------------------------------
    // Path and content rule, mirroring the live tool's own tests.
    // -----------------------------------------------------------------

    #[test]
    fn rejects_a_path_not_directly_under_a_buildkite_directory() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/plugins/stage-input/plugin.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap();
        let err = FakeBuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
        assert!(!err.message.contains("stage-input"));
    }

    #[test]
    fn rejects_content_with_no_steps_key() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
            "env: {}\n",
        )
        .unwrap();
        assert_eq!(
            FakeBuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn rejects_content_with_a_duplicate_top_level_key() {
        let configuration = RepoFile::new(
            RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"a\"\nsteps:\n  - command: \"b\"\n",
        )
        .unwrap();
        assert_eq!(
            FakeBuildkitePipelineBootstrapEnsure::validate_configuration(&configuration)
                .unwrap_err()
                .kind,
            ToolErrorKind::Invalid
        );
    }

    #[test]
    fn validation_runs_before_any_state_lookup() {
        // No pipeline is seeded at all: if validation ran after the
        // lookup, a missing pipeline would read `Absent`/`NotFound`
        // rather than `Invalid`, since the state has nothing to object
        // to either way. Asserting `Invalid` here proves validation ran
        // first regardless.
        let state = Arc::new(Mutex::new(FakeState::new()));
        let t = tool(state);
        let mut bad_inputs = inputs();
        bad_inputs.insert(
            port("configuration"),
            Value::known(
                RepoFile::new(
                    RepoPath::parse("apps/walter/bootstrap.yml").unwrap(),
                    "steps:\n  - command: \"echo hi\"\n",
                )
                .unwrap(),
            ),
        );
        let err = t.read(&bad_inputs).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
    }

    // -----------------------------------------------------------------
    // `fail_ensure_once` (acceptance 7).
    // -----------------------------------------------------------------

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn fail_ensure_once_fires_once_then_a_retry_succeeds() {
        let key = buildkite_pipeline_key(&org(), &slug());
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_buildkite_pipeline(
                    &org(),
                    &slug(),
                    owned_record("steps:\n  - command: \"buildkite-agent pipeline upload\"\n"),
                )
                .with_fail_ensure_once(FakeBuildkitePipelineBootstrapEnsure::TOOL_NAME, &key),
        ));
        let t = tool(state);
        let token = SinkToken::new();
        let first = t.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(first.kind, ToolErrorKind::Provider);
        // Planned as `Update` still, since nothing was written.
        assert!(t.updates(&inputs()).unwrap());
        let second = t.ensure(&inputs(), &token).unwrap();
        assert!(second.changed);
    }
}
