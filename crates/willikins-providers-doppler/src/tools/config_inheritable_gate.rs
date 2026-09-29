//! `doppler.config.inheritable.gate`: a gate over "this Doppler config
//! exists and is marked inheritable" (task R3,
//! `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, decision (j)).
//!
//! # Why this exists
//!
//! The 2026-09-29 dry run found that `doppler.config.inherits.ensure`
//! plans `Create` without ever checking that the base configs it is
//! about to ask a config to inherit actually exist and are inheritable —
//! so a missing base config (the operator's own M0: "confirm the shared
//! base config names... they must exist and be inheritable") would fail
//! *mid-apply*, after earlier nodes had already written. This gate closes
//! that at plan time, before any write: a document places one of these
//! ahead of the base configs it is about to name in an
//! `doppler.config.inherits.ensure` node's own `inherits` list, and
//! `plan` refuses to proceed past a missing or non-inheritable one.
//!
//! # A scalar gate, not a list gate, and why
//!
//! `doppler.config.inherits.ensure`'s own `inherits` port is a
//! `list<DopplerConfig>` — but this gate's `config` port is a single
//! `DopplerConfig`, one gate per base, `for_each`-expanded over a
//! document's own base-config list. A list-typed subject could only ever
//! render the *whole* list back at a blocked report, never say which
//! entry was the problem, and `need`/`how` are `&'static str` (decision
//! (j): a gate never authors a string from its inputs), so they cannot
//! name it either. One gate per base config, each with its own `config`
//! subject, is the only shape that can point at the actual offender.
//!
//! # Missing and non-inheritable both read `Absent`, never `Mismatch`
//!
//! This mirrors `crate::tools::DopplerConfigInheritableEnsure::observe`
//! field for field — same client call, same `Absent` reading of
//! `inheritable: false`/absent and of a missing parent project or
//! config — with one difference: that tool's `Absent` means "ensure will
//! `POST` it inheritable"; this gate's `Absent` means "the operator must
//! make it true", because a shared base config living in another
//! project (App Store Connect's, GitHub's, `open-telemetry`'s) is not
//! this document's to flip inheritable out from under whoever else
//! depends on its current state — the same posture the sibling
//! `.ensure` tool's own module doc already takes toward an unexpected
//! *extra* entry it will not silently drop. And this is not a `Mismatch`
//! either: `appstore.app_group.gate`'s own precedent already reads
//! "the flag exists but is not yet the value we need" as `Absent` (a
//! thing the operator has not yet made true), not as "something wrong" —
//! a boolean not yet flipped is not an ownership conflict. So the only
//! way this gate's own `read` can produce a hard `PlanError` is a real
//! provider failure (a non-2xx this crate does not otherwise tolerate,
//! or a transport error), never a shape of the config it merely dislikes.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::DopplerConfig;

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// This tool's one gate: `config` is the only subject, so a blocked
/// report names exactly the base config that is missing or not yet
/// inheritable.
static GATE: Gate = Gate {
    need: "this Doppler config exists and is marked inheritable",
    how: "create it (`doppler.config.ensure` or `doppler.branch_config.ensure`) and mark it \
          inheritable (`doppler.config.inheritable.ensure`), or do both in the Doppler \
          dashboard; a `400` naming \"does not have access\" can also mean this token cannot \
          see the project it lives in",
    subject: &["config"],
};

/// `doppler.config.inheritable.gate`.
pub struct DopplerConfigInheritableGate {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerConfigInheritableGate {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.inheritable.gate"),
                description: "A gate: whether a Doppler config exists and is inheritable."
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

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`. See the module doc: `Present` needs `200` and
    /// `inheritable: true`; everything else this crate can read
    /// (`false`, absent, or a missing parent) is `Absent`, never
    /// `Mismatch` or `Foreign` — only a genuine provider failure escapes
    /// as an `Err`.
    fn observe(&self, config: &DopplerConfig) -> Result<Observation, ToolError> {
        match self.client.get_config(config.project(), config.name()) {
            Ok(body) if body.inheritable == Some(true) => {
                Ok(Observation::Present(Self::outputs_for(config)))
            }
            Ok(_) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) => Err(err.into()),
        }
    }
}

impl Tool for DopplerConfigInheritableGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        self.observe(&config)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        // See `AppstoreAppGroupGate::ensure`'s own doc: a gate is pure, so
        // `apply` never reaches this while the node is `Action::Blocked`.
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        match self.observe(&config)? {
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
    use willikins_providers_http::testing::{MockProvider, load_fixture};
    use willikins_providers_http::{Credential, Http};
    use willikins_types::DomainType;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn fixture(name: &str) -> serde_json::Value {
        load_fixture(&fixtures_dir(), "doppler", name)
    }

    fn config() -> DopplerConfig {
        DopplerConfig::parse("appstore-connect/deploy_ios").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(port("config"), Value::known(config()));
        inputs
    }

    fn client_against(url: String) -> Arc<DopplerClient> {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let http = Http::new(url, Vec::new(), credential);
        Arc::new(DopplerClient::new(http))
    }

    fn tool_against(url: String) -> DopplerConfigInheritableGate {
        DopplerConfigInheritableGate::new(client_against(url))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure_and_reversible() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let spec = tool.spec();
        assert!(spec.key.is_empty());
        assert!(spec.pure);
        assert_eq!(spec.class, Class::Reversible);
    }

    #[test]
    fn declares_a_gate_over_config_only() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["config"]);
    }

    #[test]
    fn a_catalog_accepts_this_gate() {
        let tool = tool_against("http://127.0.0.1:1".to_string());
        let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(tool)).unwrap();
    }

    #[test]
    fn read_reports_present_when_the_config_is_already_inheritable() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(200)
            .with_body(fixture("config_get_inheritable_true").to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Present(_)),
            "{observation:?}"
        );
    }

    #[test]
    fn present_passes_the_config_through_as_its_own_output() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(200)
            .with_body(fixture("config_get_inheritable_true").to_string())
            .create();
        let tool = tool_against(provider.url());
        let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "appstore-connect/deploy_ios");
    }

    #[test]
    fn read_reports_absent_when_the_config_exists_but_is_not_inheritable() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(200)
            .with_body(fixture("config_get_inheritable_false").to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_inheritable_is_never_mentioned() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(200)
            .with_body(fixture("config_get_present").to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_the_config_does_not_exist_yet() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(404)
            .with_body(fixture("error_404").to_string())
            .create();
        let tool = tool_against(provider.url());
        let observation = tool.read(&inputs()).unwrap();
        assert!(
            matches!(observation, Observation::Absent { .. }),
            "{observation:?}"
        );
    }

    #[test]
    fn read_reports_absent_when_this_token_cannot_see_the_project() {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(400)
            .with_body(fixture("error_400_no_access").to_string())
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
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(500)
            .with_body(fixture("error_5xx").to_string())
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
                "/v3/configs/config?project=appstore-connect&config=deploy_ios",
            )
            .with_status(200)
            .with_body(fixture("config_get_inheritable_false").to_string())
            .create();
        let tool = tool_against(provider.url());
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
        // Only the one `GET` above was mocked; a second, unexpected
        // request (a `POST` this gate must never issue) would fail with
        // a connection or 501 error from the mock server, not silently
        // succeed.
    }
}
