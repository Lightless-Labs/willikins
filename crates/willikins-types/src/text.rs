//! Free-form text domain types: [`Text`] and [`TemplateSource`].

/// A free-form piece of text, such as a value substituted into a rendered
/// template. Never secret: `template.render`'s non-secret sink accepts only
/// this and types like it, which is exactly how the taint check knows
/// rendering a template can never leak a secret.
#[derive(willikins_derive::DomainType)]
#[domain(
    max_len = 65536,
    description = "A free-form piece of text. Never secret.",
    example = "Third Thoughts"
)]
pub struct Text(String);

/// The source text of a template, such as a Handlebars-style document with
/// `{{ value }}` placeholders. Free-form, non-secret.
#[derive(willikins_derive::DomainType)]
#[domain(
    max_len = 65536,
    description = "The source text of a template.",
    example = "Hello, {{ value }}!"
)]
pub struct TemplateSource(String);

/// Every bundle identifier is a valid piece of free-form text, byte for
/// byte -- `Text::parse` refuses only a string over 65536 characters, and
/// [`crate::AppleBundleIdentifier`]'s own bound is 255, so every
/// identifier's string is within it and nothing else in `Text::parse`
/// refuses anything. Milestone 3e
/// (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, task T3b)
/// registers this as a conversion so a document may hand a bundle
/// identifier straight to `template.render`'s non-secret `value` sink, or
/// any other `Text` port, with no parse step in between.
///
/// The proof, by containment: `Text::parse`'s only refusal is length over
/// 65536 characters; every `AppleBundleIdentifier` is at most 255
/// characters (its own `max_len`). Both types store their input verbatim,
/// so the result is byte-identical to the source. Pinned by
/// `every_bundle_identifier_is_valid_text` below.
///
/// The reverse is not a fact -- `Text` accepts the empty string and any
/// character, including a space or a control character, that
/// `AppleBundleIdentifier` refuses -- so no reverse row is registered;
/// pinned at the document level by the negative fixture
/// `appstore-bundle-id-text-into-identifier.yaml`.
impl From<crate::AppleBundleIdentifier> for Text {
    fn from(identifier: crate::AppleBundleIdentifier) -> Self {
        Self(identifier.as_str().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainType;

    #[test]
    fn text_accepts_the_empty_string() {
        assert_eq!(Text::parse("").unwrap().as_str(), "");
    }

    #[test]
    fn text_accepts_arbitrary_content() {
        let value = Text::parse("Line one\nLine two — with an em dash").unwrap();
        assert_eq!(value.as_str(), "Line one\nLine two — with an em dash");
    }

    #[test]
    fn text_rejects_content_over_the_length_limit() {
        let too_long = "a".repeat(65537);
        let err = Text::parse(&too_long).unwrap_err();
        assert!(
            err.reason.contains("at most"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn text_accepts_content_at_exactly_the_length_limit() {
        let at_limit = "a".repeat(65536);
        assert!(Text::parse(&at_limit).is_ok());
    }

    #[test]
    fn text_serde_round_trips() {
        let value = Text::parse("hello").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"hello\"");
        let back: Text = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn template_source_accepts_the_empty_string() {
        assert_eq!(TemplateSource::parse("").unwrap().as_str(), "");
    }

    #[test]
    fn template_source_accepts_placeholders() {
        let value = TemplateSource::parse("Hello, {{ value }}!").unwrap();
        assert_eq!(value.as_str(), "Hello, {{ value }}!");
    }

    #[test]
    fn template_source_rejects_content_over_the_length_limit() {
        let too_long = "a".repeat(65537);
        assert!(TemplateSource::parse(&too_long).is_err());
    }

    #[test]
    fn template_source_serde_round_trips() {
        let value = TemplateSource::parse("{{ value }}").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"{{ value }}\"");
        let back: TemplateSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn examples_parse_as_their_own_types() {
        crate::assert_example_parses::<Text>();
        crate::assert_example_parses::<TemplateSource>();
    }

    #[test]
    fn schema_shape_is_string_with_max_length_and_no_pattern() {
        let schema = serde_json::to_value(Text::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["maxLength"], 65536);
        assert!(schema.get("pattern").is_none());
    }

    // -------------------------------------------------------------
    // `AppleBundleIdentifier => Text` (milestone 3e, task T3b): every
    // bundle identifier is a valid piece of free-form text, byte for
    // byte, proved the same three ways as the other two conversion rows
    // in `appstore.rs`.
    // -------------------------------------------------------------

    mod bundle_identifier_to_text {
        use proptest::prelude::*;

        use super::*;
        use crate::AppleBundleIdentifier;

        proptest! {
            /// Strategy 1: generate directly from the bundle-identifier
            /// grammar, discarding any candidate over 255 characters.
            #[test]
            fn every_bundle_identifier_is_valid_text(
                raw in "[A-Za-z0-9]{1,8}([.-][A-Za-z0-9]{1,8}){0,40}"
            ) {
                prop_assume!(raw.chars().count() <= 255);
                let identifier = AppleBundleIdentifier::parse(&raw)
                    .unwrap_or_else(|err| panic!("{raw:?} must be a valid AppleBundleIdentifier: {err}"));
                let text = Text::parse(identifier.as_str())
                    .unwrap_or_else(|err| panic!("{raw:?} must also be valid Text: {err}"));
                let converted = Text::from(identifier.clone());
                prop_assert_eq!(&converted, &text);
                prop_assert_eq!(converted.as_str(), identifier.as_str());
            }

            /// Strategy 2: the implication stated directly over arbitrary
            /// strings, with no grammar-shaped generator to bias
            /// coverage. `Text::parse` refuses nothing an
            /// `AppleBundleIdentifier` could contain, so this always
            /// holds -- unlike the profile-name and bundle-id-name rows,
            /// there is no character `Text` could still refuse here.
            #[test]
            fn every_string_a_bundle_identifier_accepts_text_also_accepts(s in ".*") {
                if let Ok(identifier) = AppleBundleIdentifier::parse(&s) {
                    let text = Text::parse(identifier.as_str())
                        .unwrap_or_else(|err| panic!("{s:?} parsed as AppleBundleIdentifier but not Text: {err}"));
                    prop_assert_eq!(text.as_str(), identifier.as_str());
                }
            }
        }

        proptest! {
            /// Strategy 3: the identifier's own alphabet with no
            /// grammar-shaped structure and lengths from 1 to 300, so
            /// candidates straddle the 255 bound.
            #[test]
            fn over_the_identifier_alphabet_every_identifier_converts_byte_for_byte(
                raw in "[A-Za-z0-9.-]{1,300}"
            ) {
                if let Ok(identifier) = AppleBundleIdentifier::parse(&raw) {
                    prop_assert!(raw.chars().count() <= 255);
                    let converted = Text::from(identifier);
                    prop_assert_eq!(converted.as_str(), raw.as_str());
                    prop_assert_eq!(
                        Text::parse(&raw).expect("an identifier is valid text"),
                        converted
                    );
                }
            }
        }

        /// A 255-character identifier -- the bound -- converts byte for
        /// byte, far under `Text`'s own 65536-character bound.
        #[test]
        fn a_255_character_identifier_converts() {
            let raw = "a".repeat(255);
            let identifier = AppleBundleIdentifier::parse(&raw)
                .expect("255 characters is the limit, not over it");
            let text = Text::from(identifier);
            assert_eq!(text.as_str(), raw);
        }

        /// The reverse is not a fact: the empty string is valid `Text`
        /// and not a valid bundle identifier, so no reverse row exists
        /// (pinned at the document level by the negative fixture
        /// `appstore-bundle-id-text-into-identifier.yaml`).
        #[test]
        fn empty_text_is_not_a_bundle_identifier() {
            let text = Text::parse("").expect("the empty string is valid text");
            assert_eq!(text.as_str(), "");
            assert!(AppleBundleIdentifier::parse("").is_err());
        }
    }
}
