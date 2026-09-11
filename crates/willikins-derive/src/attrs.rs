//! Parsing for `#[domain(...)]`.

/// The parsed contents of a type's `#[domain(...)]` attribute.
pub struct DomainAttrs {
    pub pattern: Option<syn::LitStr>,
    pub min_len: Option<u64>,
    pub max_len: Option<u64>,
    pub secret: bool,
    pub description: syn::LitStr,
    pub example: syn::LitStr,
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
        let mut example: Option<syn::LitStr> = None;

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

    /// The pattern anchored with `^(?:...)$ ` unless the author already
    /// anchored it themselves, validated as a real regex at
    /// macro-expansion time so an invalid pattern is a compile error at
    /// the attribute's span.
    pub fn anchored_pattern(&self) -> syn::Result<Option<String>> {
        let Some(pattern) = &self.pattern else {
            return Ok(None);
        };
        let raw = pattern.value();
        let anchored = if raw.starts_with('^') && raw.ends_with('$') {
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
