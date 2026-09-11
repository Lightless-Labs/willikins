//! Storage classification for the single tuple field of a `#[derive(DomainType)]` newtype.

/// How the newtype's single field stores its value.
pub enum Storage {
    /// `String`.
    Str,
    /// `secrecy::SecretString` (or the bare `SecretString` path).
    Secret,
    /// Any other type, used through `FromStr` + `Display`.
    Other(Box<syn::Type>),
}

/// Classify `ty` by its last path segment. This is a syntactic check (no
/// type resolution is available in a proc macro): a type whose last
/// segment is a bare, non-generic `String` or `SecretString` is treated as
/// that storage regardless of how it was imported; anything else is
/// `Other`.
pub fn classify(ty: &syn::Type) -> Storage {
    if let syn::Type::Path(type_path) = ty
        && type_path.qself.is_none()
        && let Some(segment) = type_path.path.segments.last()
        && matches!(segment.arguments, syn::PathArguments::None)
    {
        if segment.ident == "String" {
            return Storage::Str;
        }
        if segment.ident == "SecretString" {
            return Storage::Secret;
        }
    }
    Storage::Other(Box::new(ty.clone()))
}
