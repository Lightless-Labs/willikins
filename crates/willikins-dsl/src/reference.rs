//! Parsing a `with` value or a `for_each` source into a [`Binding`].
//!
//! Reference forms, exactly:
//!
//! - `${{ inputs.<name> }}`
//! - `${{ steps.<node>.<port> }}`
//! - `${{ steps.<node>[<key>].<port> }}`
//! - `${{ item }}`
//!
//! with an optional single space just inside each brace, matching every
//! fixture (`${{ x }}` and `${{x}}` both parse; `${{  x }}`, with two
//! spaces, does not). A value that does not start with `${{` is always a
//! literal string ([`parse_with_value`]). A value that does start with
//! `${{` but does not match one of the forms above is a grammar error
//! naming the offending text — this covers `${{ org.x }}` (an unknown
//! root), `${{ steps.a }}` (missing port), `${{ inputs.a.b }}` (extra
//! segment), a nested `{`/`}`, and a missing closing `}}`.
//!
//! `for_each` uses the same grammar but must always be a reference; a
//! literal `for_each` value is rejected by [`parse_for_each_value`].

use std::sync::LazyLock;

use regex::Regex;

use willikins_core::{Binding, InputName, NodeName, PortName};

static INPUT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^inputs\.([a-z][a-z0-9_]*)$").expect("pattern is valid"));
static STEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^steps\.([a-z][a-z0-9_]*)\.([a-z][a-z0-9_]*)$").expect("pattern is valid")
});
static KEYED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^steps\.([a-z][a-z0-9_]*)\[([A-Za-z0-9_-]+)\]\.([a-z][a-z0-9_]*)$")
        .expect("pattern is valid")
});

/// The result of parsing one `with` (or output) value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// A literal string, checked against the bound port's scalar type
    /// later, at `check` time.
    Literal(String),
    /// A resolved reference.
    Binding(Binding),
}

/// Parse one `with` or output value.
///
/// `raw` that does not start with `${{` is always [`Parsed::Literal`].
///
/// # Errors
///
/// Returns a message naming `raw` when it starts with `${{` but does not
/// match one of the reference forms described in the module docs.
pub fn parse_with_value(raw: &str) -> Result<Parsed, String> {
    if !raw.starts_with("${{") {
        return Ok(Parsed::Literal(raw.to_string()));
    }
    parse_reference(raw).map(Parsed::Binding)
}

/// Parse `raw` as a `for_each` source.
///
/// Unlike [`parse_with_value`], a value that does not start with `${{` is
/// itself an error: `for_each` must always be a reference, never a
/// literal.
///
/// # Errors
///
/// Returns a message naming `raw` when it is not a reference at all, or
/// when it starts with `${{` but does not match one of the reference
/// forms.
pub fn parse_for_each_value(raw: &str) -> Result<Binding, String> {
    if !raw.starts_with("${{") {
        return Err(format!(
            "`{raw}` is not a reference; for_each must be a reference"
        ));
    }
    parse_reference(raw)
}

/// Parse `raw` as a reference: strip the `${{ ... }}` delimiters (with at
/// most one space just inside each brace) and match the body against the
/// four reference forms.
fn parse_reference(raw: &str) -> Result<Binding, String> {
    let body = strip_braces(raw).ok_or_else(|| invalid(raw))?;
    parse_body(body).ok_or_else(|| invalid(raw))
}

fn invalid(raw: &str) -> String {
    format!("`{raw}` is not a valid reference")
}

/// Strip the `${{` prefix and `}}` suffix, then at most one leading and
/// one trailing space. Returns `None` when `raw` is not delimited that
/// way at all (no `${{` prefix, no `}}` suffix — including when the two
/// overlap, as in `"${{}}"`, which strips to an empty body and simply
/// fails to match any form).
fn strip_braces(raw: &str) -> Option<&str> {
    let body = raw.strip_prefix("${{")?;
    let body = body.strip_suffix("}}")?;
    let body = body.strip_prefix(' ').unwrap_or(body);
    let body = body.strip_suffix(' ').unwrap_or(body);
    Some(body)
}

/// Match a stripped reference body against the four forms, in no
/// particular order since each is fully anchored.
fn parse_body(body: &str) -> Option<Binding> {
    if body == "item" {
        return Some(Binding::Item);
    }
    if let Some(caps) = INPUT_RE.captures(body) {
        return Some(Binding::Input(InputName::parse(&caps[1]).ok()?));
    }
    if let Some(caps) = KEYED_RE.captures(body) {
        return Some(Binding::Keyed {
            node: NodeName::parse(&caps[1]).ok()?,
            key: caps[2].to_string(),
            port: PortName::parse(&caps[3]).ok()?,
        });
    }
    if let Some(caps) = STEP_RE.captures(body) {
        return Some(Binding::Step {
            node: NodeName::parse(&caps[1]).ok()?,
            port: PortName::parse(&caps[2]).ok()?,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    fn port(name: &str) -> PortName {
        PortName::parse(name).unwrap()
    }

    fn input(name: &str) -> InputName {
        InputName::parse(name).unwrap()
    }

    #[test]
    fn non_reference_text_is_a_literal() {
        assert_eq!(
            parse_with_value("private").unwrap(),
            Parsed::Literal("private".to_string())
        );
        assert_eq!(
            parse_with_value("DOPPLER_TOKEN").unwrap(),
            Parsed::Literal("DOPPLER_TOKEN".to_string())
        );
    }

    #[test]
    fn parses_input_reference() {
        assert_eq!(
            parse_with_value("${{ inputs.org }}").unwrap(),
            Parsed::Binding(Binding::Input(input("org")))
        );
    }

    #[test]
    fn parses_step_reference() {
        assert_eq!(
            parse_with_value("${{ steps.names.github_repo }}").unwrap(),
            Parsed::Binding(Binding::Step {
                node: node("names"),
                port: port("github_repo"),
            })
        );
    }

    #[test]
    fn parses_keyed_reference() {
        assert_eq!(
            parse_with_value("${{ steps.configs[prd].config }}").unwrap(),
            Parsed::Binding(Binding::Keyed {
                node: node("configs"),
                key: "prd".to_string(),
                port: port("config"),
            })
        );
    }

    #[test]
    fn keyed_reference_key_accepts_letters_digits_underscore_and_hyphen() {
        assert_eq!(
            parse_with_value("${{ steps.configs[prd-2_a].config }}").unwrap(),
            Parsed::Binding(Binding::Keyed {
                node: node("configs"),
                key: "prd-2_a".to_string(),
                port: port("config"),
            })
        );
    }

    #[test]
    fn parses_item_reference() {
        assert_eq!(
            parse_with_value("${{ item }}").unwrap(),
            Parsed::Binding(Binding::Item)
        );
    }

    #[test]
    fn accepts_no_inner_spaces_as_well_as_one() {
        assert_eq!(
            parse_with_value("${{item}}").unwrap(),
            Parsed::Binding(Binding::Item)
        );
        assert_eq!(
            parse_with_value("${{item }}").unwrap(),
            Parsed::Binding(Binding::Item)
        );
        assert_eq!(
            parse_with_value("${{ item}}").unwrap(),
            Parsed::Binding(Binding::Item)
        );
    }

    #[test]
    fn rejects_two_inner_spaces() {
        assert!(parse_with_value("${{  item }}").is_err());
        assert!(parse_with_value("${{ item  }}").is_err());
    }

    #[test]
    fn rejects_unknown_root() {
        let err = parse_with_value("${{ org.x }}").unwrap_err();
        assert!(err.contains("org.x"), "{err}");
    }

    #[test]
    fn rejects_step_reference_missing_port() {
        let err = parse_with_value("${{ steps.a }}").unwrap_err();
        assert!(err.contains("steps.a"), "{err}");
    }

    #[test]
    fn rejects_input_reference_with_extra_segment() {
        let err = parse_with_value("${{ inputs.a.b }}").unwrap_err();
        assert!(err.contains("inputs.a.b"), "{err}");
    }

    #[test]
    fn rejects_nested_braces() {
        assert!(parse_with_value("${{ steps.a.{b} }}").is_err());
    }

    #[test]
    fn rejects_missing_closing_braces() {
        assert!(parse_with_value("${{ inputs.org").is_err());
    }

    #[test]
    fn for_each_rejects_a_literal() {
        let err = parse_for_each_value("dev").unwrap_err();
        assert!(err.contains("for_each"), "{err}");
    }

    #[test]
    fn for_each_accepts_a_reference() {
        assert_eq!(
            parse_for_each_value("${{ inputs.environments }}").unwrap(),
            Binding::Input(input("environments"))
        );
    }

    #[test]
    fn for_each_rejects_a_malformed_reference() {
        assert!(parse_for_each_value("${{ steps.a }}").is_err());
    }
}
