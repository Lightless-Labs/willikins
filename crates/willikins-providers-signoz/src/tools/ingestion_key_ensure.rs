//! `signoz.ingestion_key.ensure`: mints a real `SigNoz` ingestion key.
//!
//! `read` (and `ensure` on an already-existing key) always report the
//! `key` output `Unknown`: an ingestion key's value can never be re-read
//! once minted — `GET /api/v2/gateway/ingestion_keys` *does* return
//! `value` for every listed entry (research section 3, the one fact its
//! own `OpenAPI` schema omits), but this tool never reads it back.
//!
//! **This is a decision, not a limitation.** A tool that *can* hand back
//! an existing secret is a strictly wider surface than one that can only
//! mint a new one, nothing in any workflow this crate serves needs the
//! reread, and the narrower behaviour is the one the type system was
//! built around: exactly `doppler.service_token.ensure`'s own shape
//! (`willikins_providers_doppler::tools::service_token_ensure`), which
//! this tool is otherwise a transcription of. The list response's
//! `value` field is not merely unread — `IngestionKeyListEntry` has no
//! field for it at all, so it is discarded by `serde` before it ever
//! becomes a `String` in this process; see
//! `crate::client::SigNozClient::list_ingestion_keys`'s own doc comment.
//!
//! The documented `409` ("already exists") is a belt-and-braces check
//! *after* the read-then-create, never the primary signal: `ensure`
//! lists first, exactly like `doppler.service_token.ensure` does, and
//! only consults the `409` when a create raced ahead of this call's own
//! listing — see [`SigNozIngestionKeyEnsure::ensure`]'s doc comment.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::SigNozIngestionKeyName;

use crate::client::{SigNozClient, looks_like_a_duplicate_name};

/// `signoz.ingestion_key.ensure`.
pub struct SigNozIngestionKeyEnsure {
    spec: ToolSpec,
    client: Arc<SigNozClient>,
}

impl SigNozIngestionKeyEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<SigNozClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("name"), exact("SigNozIngestionKeyName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("key"), scalar("SigNozIngestionKeyValue"));
        Self {
            spec: ToolSpec {
                name: tool_name("signoz.ingestion_key.ensure"),
                description: "Ensure a SigNoz ingestion key exists. Its value can never be re-read once minted.".to_string(),
                inputs,
                outputs,
                key: vec![port("name")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn name_port(&self, inputs: &Inputs) -> Result<SigNozIngestionKeyName, ToolError> {
        require_present(&self.spec, inputs)?;
        get(inputs, "name")
    }

    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("key"),
            Value::unknown(scalar("SigNozIngestionKeyValue")),
        );
        outputs
    }

    /// Whether a key named `name` is already listed. Shared by `read` and
    /// `ensure`.
    fn is_listed(&self, name: &SigNozIngestionKeyName) -> Result<bool, ToolError> {
        let listed = self.client.list_ingestion_keys()?;
        Ok(listed.iter().any(|entry| entry.name == name.as_str()))
    }
}

impl Tool for SigNozIngestionKeyEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let name = self.name_port(inputs)?;
        if self.is_listed(&name)? {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let name = self.name_port(inputs)?;
        if self.is_listed(&name)? {
            return Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            });
        }
        match self.client.create_ingestion_key(&name) {
            Ok(created) => {
                let mut outputs = Outputs::new();
                outputs.insert(port("key"), Value::known(created.value));
                Ok(Ensured {
                    outputs,
                    changed: true,
                })
            }
            // Belt and braces: this call's own listing above found
            // nothing, but a concurrent caller may have minted `name`
            // between that read and this create. Re-list once; if it is
            // there now, this call converges rather than failing a
            // document that asked for exactly the state that exists.
            // Anything else — including a `409` that a re-list still
            // does not explain — propagates as the original error.
            Err(err) if looks_like_a_duplicate_name(&err) => {
                if self.is_listed(&name)? {
                    Ok(Ensured {
                        outputs: Self::unknown_outputs(),
                        changed: false,
                    })
                } else {
                    Err(err.into())
                }
            }
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::Credential;

    fn tool() -> SigNozIngestionKeyEnsure {
        let credential =
            Credential::for_testing("WILLIKINS_TEST_SIGNOZ_API_KEY", "test-api-key-0000000000");
        let http = crate::client::http_client("http://127.0.0.1:1", credential);
        SigNozIngestionKeyEnsure::new(Arc::new(SigNozClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_key_is_name() {
        assert_eq!(tool().spec().key, vec![port("name")]);
    }

    #[test]
    fn spec_is_reversible_and_impure() {
        assert_eq!(tool().spec().class, Class::Reversible);
        assert!(!tool().spec().pure);
    }
}
