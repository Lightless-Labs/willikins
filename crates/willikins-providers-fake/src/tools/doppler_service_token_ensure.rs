//! `doppler.service_token.ensure`: mints (in memory) a Doppler service
//! token. `read` (and `ensure` on an already-existing token) always
//! report the `token` output `Unknown`: a service token's value cannot be
//! re-read once issued. `ensure`'s *create* path is the one exception —
//! it hands back the freshly minted value as `Known`, since the executor
//! passes an `ensure` call's own outputs downstream in the same run (see
//! `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! "Convergence" section): a plan that stores the token as a GitHub
//! Actions secret in the same run it was minted needs the real bytes to
//! do that with.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::rollback::{Created, Deleted, Inverse, Reversal, Side, Standing, Within};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName, TeardownToken};

use crate::state::{FakeState, doppler_service_token_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// `doppler.service_token.ensure`.
pub struct DopplerServiceTokenEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
    inverse: DopplerServiceTokenInverse,
}

impl DopplerServiceTokenEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.service_token.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("DopplerTokenName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("token"), scalar("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Doppler service token exists. Its value can never be re-read once issued.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: DopplerServiceTokenInverse {
                state: state.clone(),
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, DopplerTokenName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }

    /// The always-`Unknown` `token` output.
    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::unknown(scalar("DopplerServiceToken")));
        outputs
    }
}

impl Tool for DopplerServiceTokenEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let key = doppler_service_token_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.doppler_service_tokens.contains(&key) {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let key = doppler_service_token_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        let call_count = state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        // Read its own state first, so `changed` is truthful: a token
        // that already exists is never re-minted. Its value can never be
        // re-read once issued, so a call on an existing token still
        // reports `Unknown` -- only the create path hands back the real,
        // freshly minted value (`Known`), which is what makes the
        // executor's `ci_secret` node able to consume it in the same run.
        if state.doppler_service_tokens.contains(&key) {
            Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            })
        } else {
            let minted = state.mint_token(Self::TOOL_NAME, &key, call_count);
            state.doppler_service_tokens.insert(key);
            let mut outputs = Outputs::new();
            outputs.insert(port("token"), Value::known(minted));
            Ok(Ensured {
                outputs,
                changed: true,
            })
        }
    }

    /// Milestone 3m task P6: equal to the live tool's own declaration
    /// (decision 4), field for field.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["config", "name"],
            within: &[
                Within {
                    tool: "doppler.config.ensure",
                    ports: &[("config", Side::Output, "config")],
                },
                Within {
                    tool: "doppler.branch_config.ensure",
                    ports: &[("config", Side::Output, "config")],
                },
            ],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `doppler.service_token.ensure`'s fake inverse (milestone 3m task P6).
/// Never a catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2).
///
/// No timestamps exist anywhere in this crate's state, and
/// [`crate::state::FakeState::doppler_service_tokens`] records existence
/// alone (a `HashSet`), so neither the live inverse's "created outside
/// the window" `NotOurs` row nor its "more than one token at
/// `(config, name)`" `Drifted` row is reachable here -- the zero-match
/// case reads [`Standing::Gone`] (the P3 addendum's own reading of
/// decision 7, required for resume to work), and a present token is
/// always [`Standing::AsCreated`].
struct DopplerServiceTokenInverse {
    state: Arc<Mutex<FakeState>>,
}

/// [`crate::support::conflict`]'s message when [`Inverse::delete`] is
/// called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`].
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the token has changed since this run created it, or is no longer ours; the teardown will \
     not delete it";

impl Inverse for DopplerServiceTokenInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let config: DopplerConfig = get(created.inputs, "config")?;
        let name: DopplerTokenName = get(created.inputs, "name")?;
        let key = doppler_service_token_key(&config, &name);
        let state = self.state.lock().unwrap();
        if state.doppler_service_tokens.contains(&key) {
            Ok(Standing::AsCreated { losing: Vec::new() })
        } else {
            Ok(Standing::Gone)
        }
    }

    fn delete(&self, created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        match self.standing(created)? {
            Standing::Gone => Ok(Deleted { already_gone: true }),
            Standing::AsCreated { losing }
                if losing.iter().all(|loss| {
                    created
                        .accept
                        .iter()
                        .any(|accepted| accepted.as_str() == *loss)
                }) =>
            {
                let config: DopplerConfig = get(created.inputs, "config")?;
                let name: DopplerTokenName = get(created.inputs, "name")?;
                let key = doppler_service_token_key(&config, &name);
                let mut state = self.state.lock().unwrap();
                state.doppler_service_tokens.remove(&key);
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};
    use willikins_types::{DomainType, DopplerServiceToken};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn name() -> DopplerTokenName {
        DopplerTokenName::parse("ci").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn tool() -> DopplerServiceTokenEnsure {
        DopplerServiceTokenEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    fn token_value(outputs: &Outputs) -> Value {
        outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .clone()
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_with_an_unknown_token_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        assert!(!token_value(&predicted).is_known());
    }

    #[test]
    fn read_reports_present_with_an_unknown_token_when_seeded() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_service_token(&config(), &name()),
        ));
        let observation = DopplerServiceTokenEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        assert!(!token_value(&outputs).is_known());
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("config").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("DopplerConfig").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present_and_stays_unknown() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let observation = tool.read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        assert!(!token_value(&outputs).is_known());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed, "minting the token must report changed");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !second.changed,
            "ensure on an already-minted token must report changed: false, never re-minting"
        );
    }

    /// Task 4b's fix for the gap task 4a's report flagged: the create
    /// path must hand back a `Known` value, or `ci_secret` could never
    /// consume it in the same run.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_creation_returns_the_minted_token_as_known() {
        let state = Arc::new(Mutex::new(FakeState::new().with_next_token(
            DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7))).unwrap(),
        )));
        let tool = DopplerServiceTokenEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(ensured.changed);
        let value = token_value(&ensured.outputs);
        assert!(value.is_known());
        assert_eq!(
            value.render().to_string(),
            "[REDACTED DopplerServiceToken]",
            "the redacted rendering, not a claim about which bytes: the point is `is_known()`"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_records_a_call_per_attempt_and_an_injected_failure_fires_once_and_writes_nothing() {
        let state = Arc::new(Mutex::new(FakeState::new().with_fail_ensure_once(
            DopplerServiceTokenEnsure::TOOL_NAME,
            &doppler_service_token_key(&config(), &name()),
        )));
        let tool = DopplerServiceTokenEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();

        let err = tool.ensure(&full_inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Provider);
        assert!(
            matches!(
                tool.read(&full_inputs()).unwrap(),
                Observation::Absent { .. }
            ),
            "an injected failure must not mutate state"
        );

        let ensured = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(ensured.changed, "the second attempt actually mints");

        let key = doppler_service_token_key(&config(), &name());
        let locked = state.lock().unwrap();
        assert_eq!(
            locked.ensure_calls
                [&crate::state::call_key(DopplerServiceTokenEnsure::TOOL_NAME, &key)],
            2,
            "both the failed and the successful attempt must count"
        );
    }

    /// Milestone 3m task P6: the fake inverse's own unit tests.
    mod inverse {
        use super::*;
        use willikins_core::Timestamp;
        use willikins_core::rollback::{CreationWindow, Standing};

        fn window() -> CreationWindow {
            CreationWindow {
                start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
            }
        }

        fn created(inputs: &Inputs) -> Created<'_> {
            Created {
                inputs,
                outputs: Box::leak(Box::new(Outputs::new())),
                window: window(),
                settled_by: window().end,
                accept: &[],
                riders: &[],
            }
        }

        #[test]
        fn declares_delete_within_either_config_tool() {
            assert_eq!(
                tool().reversal(),
                Reversal::Delete {
                    needs: &["config", "name"],
                    within: &[
                        Within {
                            tool: "doppler.config.ensure",
                            ports: &[("config", Side::Output, "config")],
                        },
                        Within {
                            tool: "doppler.branch_config.ensure",
                            ports: &[("config", Side::Output, "config")],
                        },
                    ],
                    losses: &[],
                }
            );
        }

        #[test]
        fn standing_reports_gone_when_absent() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs())).unwrap(),
                Standing::Gone
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn standing_as_created_then_delete_removes_the_token() {
            let tool = tool();
            let sink = SinkToken::new();
            tool.ensure(&full_inputs(), &sink).unwrap();
            let inverse = Tool::inverse(&tool).unwrap();
            assert_eq!(
                inverse.standing(&created(&full_inputs())).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse.delete(&created(&full_inputs()), &token).unwrap();
            assert!(!deleted.already_gone);
            assert_eq!(
                inverse.standing(&created(&full_inputs())).unwrap(),
                Standing::Gone
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_on_an_already_gone_token_reports_already_gone() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse.delete(&created(&full_inputs()), &token).unwrap();
            assert!(deleted.already_gone);
        }
    }
}
