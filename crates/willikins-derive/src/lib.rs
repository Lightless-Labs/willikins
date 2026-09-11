//! `#[derive(DomainType)]` for willikins domain newtypes.
//!
//! ```ignore
//! #[derive(willikins_derive::DomainType)]
//! #[domain(pattern = "^[a-z][a-z0-9-]*$", max_len = 32,
//!          description = "A GitHub organization login", example = "lightless-labs")]
//! pub struct GitHubOrg(String);
//! ```
//!
//! Storage is chosen by the single tuple field's type:
//!
//! - `String`: the canonical string is validated and stored verbatim.
//! - `secrecy::SecretString`: requires `#[domain(secret, ...)]`. Validation
//!   runs on the raw `&str` before it is wrapped. No `Serialize` is
//!   generated; `Display`/`Debug` print `[REDACTED <TypeName>]`; the value
//!   is reachable only through `expose(&self, &willikins_types::SinkToken)`.
//! - Any other type implementing `FromStr + Display + Clone + Eq`: `parse`
//!   delegates to `Inner::from_str`, then validates the `Display` form.
//!
//! `secret` on `String` or any other non-`SecretString` storage, and
//! `secrecy::SecretString` storage without `secret`, are compile errors.
//! An invalid `pattern` is a compile error at the attribute's span. A
//! non-newtype input (an enum, a struct with named fields, a tuple struct
//! with more than one field) is a compile error.

use proc_macro::TokenStream;

mod attrs;
mod classify;
mod codegen;

use attrs::DomainAttrs;
use classify::Storage;

/// Derive `DomainType` and its companion impls for a newtype.
#[proc_macro_derive(DomainType, attributes(domain))]
pub fn derive_domain_type(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(input: &syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let field_ty = single_newtype_field(input)?;

    let attrs = DomainAttrs::parse(name.span(), &input.attrs)?;
    let anchored_pattern = attrs.anchored_pattern()?;

    match classify::classify(field_ty) {
        Storage::Str => {
            if attrs.secret {
                return Err(syn::Error::new_spanned(
                    name,
                    "#[domain(secret)] is not allowed on `String` storage; use \
                     `secrecy::SecretString` instead",
                ));
            }
            Ok(codegen::gen_string(
                name,
                &attrs,
                anchored_pattern.as_deref(),
            ))
        }
        Storage::Secret => {
            if !attrs.secret {
                return Err(syn::Error::new_spanned(
                    name,
                    "`secrecy::SecretString` storage requires #[domain(secret, ...)]",
                ));
            }
            Ok(codegen::gen_secret(
                name,
                &attrs,
                anchored_pattern.as_deref(),
            ))
        }
        Storage::Other(inner) => {
            if attrs.secret {
                return Err(syn::Error::new_spanned(
                    name,
                    "#[domain(secret)] is only allowed on `secrecy::SecretString` storage",
                ));
            }
            Ok(codegen::gen_other(
                name,
                &inner,
                &attrs,
                anchored_pattern.as_deref(),
            ))
        }
    }
}

/// Require `input` to be a tuple struct with exactly one field, returning
/// that field's type.
fn single_newtype_field(input: &syn::DeriveInput) -> syn::Result<&syn::Type> {
    const MSG: &str = "#[derive(DomainType)] requires a newtype struct: a tuple struct with \
                        exactly one field, such as `struct GitHubOrg(String);`";

    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, MSG));
    };

    match &data.fields {
        syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(&fields.unnamed[0].ty),
        _ => Err(syn::Error::new_spanned(&input.ident, MSG)),
    }
}
