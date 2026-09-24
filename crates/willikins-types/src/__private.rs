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

/// The type-erased converter behind every registered conversion. Values
/// travel as the dynamically typed `Value`
/// (`willikins-core::value::Value`), so applying a conversion must
/// downcast to the source type before converting it. The downcast is by
/// `TypeId`, and a type *name* does not determine a `TypeId`: any crate can
/// derive a type whose `TYPE_NAME` equals `A`'s, and `Value::known` accepts
/// it. So a failed downcast is an ordinary input, not an impossible state:
/// it is `None`, and the caller passes the value through unconverted
/// (independent review of milestone 3d, 2026-09-24; this was an
/// `unreachable!` that a same-named value reached through `plan`).
fn convert<A, B>(source: &dyn DomainObject) -> Option<Arc<dyn DomainObject>>
where
    A: DomainType + DomainObject + 'static,
    B: DomainType + DomainObject + From<A> + 'static,
{
    let source = crate::downcast::<A>(source)?;
    Some(Arc::new(B::from(source.clone())))
}
