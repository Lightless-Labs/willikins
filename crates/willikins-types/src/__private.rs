//! Re-exports consumed by `#[derive(DomainType)]`-generated code, plus the
//! generic constructor and converter behind the [`crate::conversions`]
//! macro.
//!
//! Not part of the public API of this crate: generated code reaches
//! `serde`, `schemars`, `secrecy`, `regex`, and `serde_json` through these
//! paths so that a crate using `#[derive(DomainType)]` needs no extra
//! dependencies of its own. Names and shapes here may change without
//! notice; do not use this module directly.
#![allow(missing_docs)]

use std::sync::Arc;

use crate::object::DomainObject;
use crate::registry::Conversion;
use crate::{DomainType, TypeName};

pub use regex;
pub use schemars;
pub use secrecy;
pub use serde;
pub use serde_json;

/// Build the [`Conversion`] row for `A => B`. The only caller is
/// [`crate::conversions`]'s own expansion (`clippy.toml` disallows calling
/// this any other way; the macro's expansion carries the one `#[allow]`).
///
/// Bounded `B: From<A>`, so a row with no hand-written `From` impl is a
/// plain E0277 at the call site -- the row cannot drift from the impl it
/// names, because it is generated from that impl's own bound, never
/// discovered from it (Rust has no trait reflection).
///
/// The `const { }` block re-asserts "secrecy only goes up" a second time,
/// inside a generic function. It is a belt, not the guarantee: it is only
/// evaluated at monomorphization, which a `cargo check` (trybuild's own
/// mode for a compile-fail fixture) is not proven to reach. The macro's own
/// `const _` at the call site, over the two concrete types directly, is
/// the guarantee `secret_to_public.rs` exists to pin.
pub fn conversion<A, B>() -> Conversion
where
    A: DomainType + DomainObject + 'static,
    B: DomainType + DomainObject + From<A> + 'static,
{
    const {
        assert!(
            !A::IS_SECRET || B::IS_SECRET,
            "conversions!: a conversion may not make a value less secret; its target must be \
             at least as secret as its source"
        );
    };
    Conversion::new(
        TypeName::from_static(A::TYPE_NAME),
        TypeName::from_static(B::TYPE_NAME),
        convert::<A, B>,
    )
}

/// The one runtime assertion a registered conversion carries. Values
/// travel as the dynamically typed `Value`
/// (`willikins-core::value::Value`), so applying a conversion must
/// downcast to the source type before converting it. `willikins-core`'s
/// `check` only ever records this conversion on an edge after resolving
/// that edge's binding to exactly type `A`, so for a `Checked` `check`
/// itself built, this downcast cannot fail. It is still an assertion,
/// confined to this generated-code-only module. Its message names the two
/// types and nothing else: never the object's own bytes, which a secret
/// type must never let a panic message carry.
fn convert<A, B>(source: &dyn DomainObject) -> Arc<dyn DomainObject>
where
    A: DomainType + DomainObject + 'static,
    B: DomainType + DomainObject + From<A> + 'static,
{
    let Some(source) = crate::downcast::<A>(source) else {
        unreachable!(
            "conversion {} -> {}: the source is not a {}",
            A::TYPE_NAME,
            B::TYPE_NAME,
            A::TYPE_NAME
        )
    };
    Arc::new(B::from(source.clone()))
}
