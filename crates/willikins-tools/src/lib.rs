//! Pure, provider-independent tools: `naming.v1`, `template.render`, and
//! `env.get`.
//!
//! All three are pure — no key, no external state, `ensure` is the
//! identity of `read` — so they need nothing beyond `willikins-core` and
//! `willikins-types`. `naming.v1` and `template.render` were moved out of
//! `willikins-providers-fake` unchanged in name, ports, behaviour, and
//! tests; see `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-tools` crate contract. `env.get` landed later, once the
//! design addendum "Credentials are ports, resolvers are nodes"
//! (`docs/plans/2026-09-11-willikins-design.md`) gave it a consumer.

mod apple_signing_key_parse;
mod base64_decode;
mod env_get;
mod naming_v1;
mod template_render;

pub use apple_signing_key_parse::AppleSigningKeyParse;
pub use base64_decode::Base64Decode;
pub use env_get::EnvGet;
pub use naming_v1::NamingV1;
pub use template_render::TemplateRender;

use willikins_core::{Catalog, CatalogError};

/// Register every tool in this crate into `catalog`.
///
/// # Errors
///
/// Returns [`CatalogError`] if any tool's spec fails to validate against
/// `catalog`'s registry, or if a tool with the same name is already
/// present.
pub fn register(catalog: &mut Catalog) -> Result<(), CatalogError> {
    catalog.insert(std::sync::Arc::new(NamingV1::new()))?;
    catalog.insert(std::sync::Arc::new(TemplateRender::new()))?;
    catalog.insert(std::sync::Arc::new(EnvGet::new()))?;
    catalog.insert(std::sync::Arc::new(Base64Decode::new()))?;
    catalog.insert(std::sync::Arc::new(AppleSigningKeyParse::new()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        let mut catalog = Catalog::new(willikins_types::registry());
        register(&mut catalog).unwrap();
        catalog
    }

    #[test]
    fn register_adds_both_tools() {
        let catalog = catalog();
        let names: Vec<&str> = catalog.specs().map(|spec| spec.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "naming.v1",
                "template.render",
                "env.get",
                "base64.decode",
                "apple.signing_key.parse",
            ]
        );
    }

    #[test]
    fn every_tool_spec_validates_against_the_registry() {
        let catalog = catalog();
        for spec in catalog.specs() {
            spec.validate(catalog.registry())
                .unwrap_or_else(|err| panic!("{}: {err}", spec.name));
        }
    }

    #[test]
    fn catalog_specs_snapshot() {
        let catalog = catalog();
        let specs: Vec<&willikins_core::ToolSpec> = catalog.specs().collect();
        insta::assert_json_snapshot!(specs);
    }
}
