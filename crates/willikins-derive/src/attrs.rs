//! Parsing for `#[domain(...)]`.

/// The parsed contents of a type's `#[domain(...)]` attribute.
pub struct DomainAttrs {
    pub pattern: Option<syn::LitStr>,
    pub min_len: Option<u64>,
    pub max_len: Option<u64>,
    pub secret: bool,
    pub description: syn::LitStr,
    /// An expression rather than a plain `syn::LitStr` on purpose: a
    /// secret type's example must satisfy its own pattern
    /// (`assert_example_parses`), and for Doppler's token types that
    /// pattern is exactly the shape a secret-scanner looks for. Accepting
    /// any constant expression lets the value be written as
    /// `concat!("dp.st.prd.", "...")` — the compiled `&'static str` is
    /// byte-identical to a single literal, but no one string in this
    /// source file spells the whole token. A plain `"literal"` is itself
    /// a valid `Expr`, so every non-secret type's attribute is unchanged.
    pub example: syn::Expr,
}

impl DomainAttrs {
    /// Parse every `#[domain(...)]` attribute on `attrs`, merging their
    /// keys, then validate that the required keys are present.
    pub fn parse(item_span: proc_macro2::Span, attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut pattern: Option<syn::LitStr> = None;
        let mut min_len: Option<syn::LitInt> = None;
        let mut max_len: Option<syn::LitInt> = None;
        let mut secret = false;
        let mut description: Option<syn::LitStr> = None;
        let mut example: Option<syn::Expr> = None;

        let mut saw_domain_attr = false;

        for attr in attrs {
            if !attr.path().is_ident("domain") {
                continue;
            }
            saw_domain_attr = true;

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("pattern") {
                    pattern = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("min_len") {
                    min_len = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("max_len") {
                    max_len = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("secret") {
                    secret = true;
                } else if meta.path.is_ident("description") {
                    description = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("example") {
                    example = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "unknown `#[domain(...)]` key; expected one of pattern, min_len, \
                         max_len, secret, description, example",
                    ));
                }
                Ok(())
            })?;
        }

        if !saw_domain_attr {
            return Err(syn::Error::new(
                item_span,
                "#[derive(DomainType)] requires a `#[domain(description = \"...\", \
                 example = \"...\")]` attribute",
            ));
        }

        let description = description.ok_or_else(|| {
            syn::Error::new(
                item_span,
                "#[domain(...)] is missing required key `description`",
            )
        })?;
        let example = example.ok_or_else(|| {
            syn::Error::new(
                item_span,
                "#[domain(...)] is missing required key `example`",
            )
        })?;

        let min_len = min_len.map(|lit| lit.base10_parse::<u64>()).transpose()?;
        let max_len = max_len.map(|lit| lit.base10_parse::<u64>()).transpose()?;

        if let (Some(min), Some(max)) = (min_len, max_len)
            && min > max
        {
            return Err(syn::Error::new(
                item_span,
                format!("#[domain(min_len = {min}, max_len = {max})] has min_len > max_len"),
            ));
        }

        Ok(Self {
            pattern,
            min_len,
            max_len,
            secret,
            description,
            example,
        })
    }

    /// The pattern anchored with `^(?:...)$` unless the author already
    /// anchored it themselves, validated as a real regex at
    /// macro-expansion time so an invalid pattern is a compile error at
    /// the attribute's span.
    pub fn anchored_pattern(&self) -> syn::Result<Option<String>> {
        let Some(pattern) = &self.pattern else {
            return Ok(None);
        };
        let raw = pattern.value();
        let anchored = if is_anchored(&raw) {
            raw
        } else {
            format!("^(?:{raw})$")
        };

        if let Err(err) = regex::Regex::new(&anchored) {
            return Err(syn::Error::new(
                pattern.span(),
                format!("#[domain(pattern = ...)] is not a valid regex: {err}"),
            ));
        }

        Ok(Some(anchored))
    }
}

/// Whether `raw` already matches whole inputs only.
///
/// Decided on the parsed syntax tree, not on the pattern's first and last
/// characters: `^a|b$` starts with `^` and ends with `$` yet reads as
/// `(^a)|(b$)`, and `^price\$` ends with a literal dollar rather than an
/// anchor. Both would be left under-anchored by a textual check. A
/// pattern that does not parse is reported as unanchored so that the
/// caller's `Regex::new` produces the diagnostic.
fn is_anchored(raw: &str) -> bool {
    let Ok(hir) = regex_syntax::Parser::new().parse(raw) else {
        return false;
    };
    hir_is_anchored(&hir)
}

fn hir_is_anchored(hir: &regex_syntax::hir::Hir) -> bool {
    starts_anchored(hir) && ends_anchored(hir)
}

/// Whether every path through `hir` begins with `^`.
///
/// Recursive because the parser factors a common anchor out of an
/// alternation: `^abc$|^def$` parses as `^(abc$|def$)`.
fn starts_anchored(hir: &regex_syntax::hir::Hir) -> bool {
    use regex_syntax::hir::{HirKind, Look};

    match hir.kind() {
        HirKind::Look(Look::Start) => true,
        HirKind::Capture(capture) => starts_anchored(&capture.sub),
        HirKind::Concat(parts) => parts.first().is_some_and(starts_anchored),
        HirKind::Alternation(branches) => branches.iter().all(starts_anchored),
        _ => false,
    }
}

/// Whether every path through `hir` ends with `$`.
fn ends_anchored(hir: &regex_syntax::hir::Hir) -> bool {
    use regex_syntax::hir::{HirKind, Look};

    match hir.kind() {
        HirKind::Look(Look::End) => true,
        HirKind::Capture(capture) => ends_anchored(&capture.sub),
        HirKind::Concat(parts) => parts.last().is_some_and(ends_anchored),
        HirKind::Alternation(branches) => branches.iter().all(ends_anchored),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_anchored;

    #[test]
    fn anchored_patterns_are_recognised() {
        assert!(is_anchored("^abc$"));
        assert!(is_anchored("^[a-z]+$"));
        assert!(is_anchored("^abc$|^def$"));
        assert!(is_anchored("^(?:abc|def)$"));
        assert!(is_anchored("^(abc)$"));
    }

    #[test]
    fn under_anchored_patterns_are_not_recognised() {
        // `(^a)|(b$)`: neither branch is anchored at both ends.
        assert!(!is_anchored("^abc|def$"));
        // A literal dollar, not an end anchor.
        assert!(!is_anchored("^price\\$"));
        assert!(!is_anchored("abc"));
        assert!(!is_anchored("^abc"));
        assert!(!is_anchored("abc$"));
        assert!(!is_anchored("^abc$|def"));
    }

    #[test]
    fn a_pattern_that_does_not_parse_is_reported_as_unanchored() {
        assert!(!is_anchored("^[$"));
    }
}
