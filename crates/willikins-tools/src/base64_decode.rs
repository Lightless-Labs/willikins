//! `base64.decode`: an opaque-secret-to-opaque-secret transform. Pure, and
//! the first tool in this crate whose input port is itself secret — see
//! `willikins_types::secret`'s module doc for why that is possible at all
//! (a pure tool's `Tool::read` is never given a `SinkToken`) and why the
//! escape hatch it uses is confined to `OpaqueSecret` alone.
//!
//! # What this accepts, and why
//!
//! **Alphabet: RFC 4648 §4 standard only, never URL-safe.** The two
//! alphabets differ in exactly two characters (`+`/`/` versus `-`/`_`),
//! so accepting both silently would mean this tool sometimes decodes a
//! URL-safe payload as if it were standard and produces wrong bytes
//! without an error, rather than a clear refusal telling the document's
//! author to reach for a URL-safe decode instead — a different encoding
//! habit is a different node, not a wider one.
//!
//! **Padding: indifferent.** A base64 blob copied out of a vault UI or a
//! `.env` file just as often lacks its trailing `=` padding as carries
//! it; both decode to the same bytes, so there is no ambiguity to protect
//! an author from by picking one.
//!
//! **Whitespace: stripped before decoding.** A key pasted into a vault
//! frequently carries the line breaks its own PEM formatting used, or a
//! trailing newline from whatever put it there. Every ASCII whitespace
//! character (space, `\t`, `\r`, `\n`) is removed first; nothing else is
//! — a stray non-whitespace character (a smart quote from a pasted
//! document, say) is exactly the kind of corruption this tool should
//! refuse rather than silently discard.
//!
//! **Decoded bytes must be UTF-8.** [`willikins_types::OpaqueSecret`] is
//! textual — see its own module doc — so a base64 payload that decodes to
//! arbitrary binary (DER, say, rather than PEM) is refused with a message
//! naming the constraint, never the bytes.
//!
//! Every error names the constraint that failed and never the input:
//! `Tool::read` runs during `plan`, and a `ToolError`'s message is
//! published to every agent watching (`ParseError`'s own module doc makes
//! the same point about literals). A base64 blob is not a literal, but
//! the same discipline applies to anything provider- or document-supplied
//! that reaches an error message.

use indexmap::IndexMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_PAD_INDIFFERENT;

use willikins_core::tool::helpers::{
    exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, OpaqueSecret};

/// `base64.decode`.
pub struct Base64Decode {
    spec: ToolSpec,
}

impl Base64Decode {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), exact("OpaqueSecret", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("OpaqueSecret"));
        Self {
            spec: ToolSpec {
                name: tool_name("base64.decode"),
                description: "Base64-decode an opaque secret (RFC 4648 standard alphabet, \
                               padding either way, ASCII whitespace stripped first); the \
                               decoded bytes must be UTF-8."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    fn compute(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let value: OpaqueSecret = get(inputs, "value")?;
        let decoded = value.reveal_for_transform(decode)?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(decoded));
        Ok(outputs)
    }
}

/// Strip ASCII whitespace, base64-decode, and require the result be UTF-8
/// and non-empty, wrapping it back into an [`OpaqueSecret`].
///
/// Free function (rather than a closure at the call site) so it can be
/// unit-tested directly against known bytes, without going through a
/// `Tool` at all.
fn decode(encoded: &str) -> Result<OpaqueSecret, ToolError> {
    let stripped: String = encoded
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD_PAD_INDIFFERENT
        .decode(&stripped)
        .map_err(|_| invalid("value is not valid base64 (RFC 4648 standard alphabet)"))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid("value decodes to bytes that are not valid UTF-8"))?;
    OpaqueSecret::parse(&text).map_err(|err| invalid(format!("decoded value: {}", err.reason)))
}

impl Default for Base64Decode {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for Base64Decode {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.compute(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn inputs_with(value: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::known(OpaqueSecret::parse(value).unwrap()),
        );
        inputs
    }

    fn decoded(tool: &Base64Decode, inputs: &Inputs) -> String {
        let Observation::Present(outputs) = tool.read(inputs).unwrap() else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        let object = value.as_scalar().expect("scalar secret");
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        object.expose(&token)
    }

    #[test]
    fn spec_validates_against_the_registry() {
        Base64Decode::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn decodes_a_padded_standard_value() {
        // "hello world" in standard base64.
        let inputs = inputs_with("aGVsbG8gd29ybGQ=");
        assert_eq!(decoded(&Base64Decode::new(), &inputs), "hello world");
    }

    #[test]
    fn decodes_an_unpadded_value_identically() {
        let inputs = inputs_with("aGVsbG8gd29ybGQ");
        assert_eq!(decoded(&Base64Decode::new(), &inputs), "hello world");
    }

    #[test]
    fn strips_embedded_whitespace_before_decoding() {
        let inputs = inputs_with("aGVsbG8g\nd29ybGQ=\n");
        assert_eq!(decoded(&Base64Decode::new(), &inputs), "hello world");
    }

    #[test]
    fn strips_spaces_and_tabs_too() {
        let inputs = inputs_with("aGVs bG8g\td29ybGQ=");
        assert_eq!(decoded(&Base64Decode::new(), &inputs), "hello world");
    }

    #[test]
    fn refuses_url_safe_input() {
        // Standard-alphabet base64 of bytes 0xFB 0xFF 0xBF (chosen so the
        // encoding differs between the two alphabets) would use `+`/`/`;
        // the URL-safe form below uses `-`/`_` and must be refused rather
        // than silently misdecoded.
        let inputs = inputs_with("-_-_");
        let err = Base64Decode::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("base64"), "{}", err.message);
    }

    #[test]
    fn refuses_bytes_that_are_not_utf8() {
        // Base64 of 0xFF 0xFE, which is not valid UTF-8 in any grouping.
        let inputs = inputs_with(&STANDARD_PAD_INDIFFERENT.encode([0xFFu8, 0xFE]));
        let err = Base64Decode::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("UTF-8"), "{}", err.message);
    }

    #[test]
    fn refuses_garbage_input_without_echoing_it() {
        let marker = "wlkn-test-marker-not-base64-!!!";
        let inputs = inputs_with(marker);
        let err = Base64Decode::new().read(&inputs).unwrap_err();
        assert!(!err.message.contains(marker), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = Base64Decode::new();
        let inputs = inputs_with("aGVsbG8gd29ybGQ=");
        let via_ensure = tool.ensure(&inputs, &token).unwrap();
        assert!(!via_ensure.changed);
        assert_eq!(decoded(&tool, &inputs), "hello world");
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = Base64Decode::new().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }
}
