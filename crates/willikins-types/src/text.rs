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
}
