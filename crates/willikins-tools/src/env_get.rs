//! `env.get`: reads a `WILLIKINS_`-namespaced environment variable. Pure,
//! no credential of its own — see
//! `docs/plans/2026-09-11-willikins-design.md`'s "Credentials are ports,
//! resolvers are nodes": this is the resolver every other one is optional
//! next to, the root of any chain, and the default for an operator who
//! runs no vault.
//!
//! **Bound.** [`willikins_types::EnvVarName`] refuses any name outside the
//! `WILLIKINS_` namespace before this tool's own logic ever runs — see
//! that type's module doc for why an unbounded version of this tool would
//! be a real widening (a document could otherwise read `PATH`, `HOME`, or
//! anything else the platform happens to inject into the butler's own
//! process).
//!
//! **Absent, empty, and non-UTF-8.** All three refuse with the same
//! [`ToolErrorKind::NotFound`], naming the variable but never a value —
//! there is only ever one value it could be for the empty and absent
//! cases (nothing), and the non-Unicode case is refused rather than
//! reported some other way because `OpaqueSecret` is textual and a lossy
//! rendering of non-UTF-8 bytes would not be the value either. This
//! mirrors `willikins_providers_http::Credential::from_value`, which
//! treats `FOO=` (set to the empty string) as `Missing` rather than a
//! distinct outcome — an operator who exported `WILLIKINS_FOO=` almost
//! certainly meant "unset", not "the credential is the empty string".
//!
//! **What this does not yet wire up.** The named consumer is the Doppler
//! credential that unlocks the Apple triple
//! (`docs/plans/2026-09-11-willikins-design.md`'s addendum) — but
//! `willikins-providers-doppler`'s tools still read their own credential
//! out of band via `Credential::from_env`, not from a graph port. Giving
//! `doppler.secret.get` (and friends) a credential *port* this tool's
//! output could bind to is a separate, larger migration, out of scope
//! here; this tool exists and is tested on its own terms in the meantime.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, EnvVarName, OpaqueSecret};

/// `env.get`.
pub struct EnvGet {
    spec: ToolSpec,
}

impl EnvGet {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("name"), exact("EnvVarName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("OpaqueSecret"));
        Self {
            spec: ToolSpec {
                name: tool_name("env.get"),
                description: "Read a WILLIKINS_-namespaced environment variable's value."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let name: EnvVarName = get(inputs, "name")?;
        let value = read_var(name.as_str())?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(value));
        Ok(outputs)
    }
}

/// Read `name` from the process environment, refusing an absent, empty, or
/// non-UTF-8 value with [`ToolErrorKind::NotFound`](willikins_core::ToolErrorKind::NotFound).
///
/// Split out from [`EnvGet::lookup`] so tests can call it directly without
/// mutating real process-wide environment state — `std::env::set_var` is
/// `unsafe` under edition 2024, which this workspace forbids outright,
/// including in tests. This function itself only *reads* the environment,
/// which is not `unsafe`; tests exercise it against variable names that
/// are already known to be absent (nothing sets `WILLIKINS_TEST_...` names
/// in this process) or present (`PATH`, inherited from the shell that
/// launched the test binary).
fn read_var(name: &str) -> Result<OpaqueSecret, ToolError> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => OpaqueSecret::parse(&value).map_err(|err| ToolError {
            kind: willikins_core::ToolErrorKind::Invalid,
            message: format!("environment variable `{name}`: {}", err.reason),
        }),
        Ok(_) | Err(std::env::VarError::NotPresent) => Err(not_found(format!(
            "environment variable `{name}` is not set"
        ))),
        Err(std::env::VarError::NotUnicode(_)) => Err(not_found(format!(
            "environment variable `{name}` is not set to valid Unicode text"
        ))),
    }
}

impl Default for EnvGet {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for EnvGet {
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
    use willikins_core::PortName;

    fn inputs_naming(name: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("name").unwrap(),
            Value::known(EnvVarName::parse(name).unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        EnvGet::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn reads_a_present_variable() {
        // `PATH` is not `WILLIKINS_`-namespaced, so it cannot be bound
        // through the real port type -- but `read_var` is the tool's
        // whole logic below the port layer, and testing it directly
        // avoids mutating real process environment state (`set_var` is
        // `unsafe` under edition 2024).
        let value = read_var("PATH").expect("PATH is set in any process that can run this test");
        let seen: Result<String, std::convert::Infallible> =
            value.reveal_for_transform(|s| Ok(s.to_string()));
        assert!(!seen.unwrap().is_empty());
    }

    #[test]
    fn refuses_an_absent_variable_naming_it_but_not_a_value() {
        let err = read_var("WILLIKINS_TEST_ENV_GET_DOES_NOT_EXIST").unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
        assert!(
            err.message
                .contains("WILLIKINS_TEST_ENV_GET_DOES_NOT_EXIST")
        );
    }

    #[test]
    fn read_refuses_a_variable_outside_the_willikins_namespace_at_the_port_layer() {
        // `EnvVarName::parse` itself refuses this, so the invalid input
        // never reaches `read_var` at all -- proven here through the real
        // `Tool::read` path with a literal, unknown-typed value standing
        // in for what `check` would have refused earlier still.
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("name").unwrap(),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("EnvVarName").unwrap(),
            )),
        );
        let err = EnvGet::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("name"), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = EnvGet::new();
        let via_read = match tool.read(&inputs_naming("WILLIKINS_TEST_ENV_GET_ABSENT")) {
            Err(err) => err,
            Ok(observation) => panic!("expected NotFound, got {observation:?}"),
        };
        let via_ensure = tool
            .ensure(&inputs_naming("WILLIKINS_TEST_ENV_GET_ABSENT"), &token)
            .unwrap_err();
        assert_eq!(via_read.kind, via_ensure.kind);
        assert_eq!(via_read.message, via_ensure.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = EnvGet::new().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("name"), "{}", err.message);
    }
}
