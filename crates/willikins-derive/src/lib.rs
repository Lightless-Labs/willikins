//! `#[derive(DomainType)]` for willikins domain newtypes.
//!
//! Usage, once implemented:
//!
//! ```ignore
//! #[derive(DomainType)]
//! #[domain(pattern = "^[a-z][a-z0-9-]*$", max_len = 32,
//!          description = "A GitHub organization login", example = "lightless-labs")]
//! pub struct GitHubOrg(String);
//! ```
//!
//! `secret` marks the type secret: `Display` and `Debug` redact, and plain
//! serde `Serialize` is not generated.

use proc_macro::TokenStream;

/// Derive `DomainType` and its companion impls for a newtype.
#[proc_macro_derive(DomainType, attributes(domain))]
pub fn derive_domain_type(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = &input.ident;
    let msg = format!("#[derive(DomainType)] is not implemented yet (on `{name}`)");
    syn::Error::new_spanned(name, msg).to_compile_error().into()
}
