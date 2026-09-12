//! The object-safe view of a domain type.
//!
//! [`crate::DomainType`] is not object-safe (it returns `Self`), so nothing
//! that needs to hold values of many different domain types behind one
//! pointer — a workflow's typed [`crate::TypeInfo`] catalog is the current
//! example, `willikins-core`'s `Value` will be a later one — can use it
//! directly. [`DomainObject`] is the erased view that such code holds
//! instead: it never returns `Self`, so `Box<dyn DomainObject>` and
//! `Arc<dyn DomainObject>` both work.

use std::any::Any;
use std::fmt;

use crate::{DomainType, SinkToken};

/// A domain value rendered for display: either its plain canonical string,
/// or a marker naming the type it redacts.
///
/// [`Rendered`] always serializes as a bare JSON string — the value itself
/// for [`Rendered::Plain`], the marker text for [`Rendered::Redacted`] —
/// never as a tagged object, because it is meant to sit inside a larger
/// structure that carries the type name separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rendered {
    /// The value's canonical string form.
    Plain(String),
    /// Stands in for a secret value everywhere but behind a [`SinkToken`].
    Redacted {
        /// The domain type name, so the marker still says what was redacted.
        type_name: &'static str,
    },
}

impl fmt::Display for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(value) => f.write_str(value),
            Self::Redacted { type_name } => write!(f, "[REDACTED {type_name}]"),
        }
    }
}

impl serde::Serialize for Rendered {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

/// The object-safe view of a domain type.
///
/// Every `#[derive(DomainType)]`-generated type implements this
/// automatically. A hand-written non-secret type gets it from
/// [`impl_domain_object_non_secret`]; a hand-written secret type must
/// implement it directly so that [`Self::render`] and [`Self::expose`]
/// route through the same redaction rule the type's `Display` already
/// obeys.
pub trait DomainObject: Send + Sync + fmt::Debug {
    /// [`crate::DomainType::TYPE_NAME`].
    fn type_name(&self) -> &'static str;
    /// [`crate::DomainType::IS_SECRET`].
    fn is_secret(&self) -> bool;
    /// The value's display form: its canonical string for a non-secret
    /// type, or a redaction marker for a secret one.
    fn render(&self) -> Rendered;
    /// The value's raw string, gated by a [`SinkToken`]. A non-secret type
    /// returns the same string as [`Self::render`]; a secret type returns
    /// the value it otherwise hides.
    fn expose(&self, token: &SinkToken) -> String;
    /// Downcast support, so callers can recover the concrete type.
    fn as_any(&self) -> &dyn Any;
    /// Type-erased equality: `true` only when `other` is the same concrete
    /// type and holds an equal value.
    fn dyn_eq(&self, other: &dyn DomainObject) -> bool;
    /// Clone into a fresh trait object.
    fn clone_box(&self) -> Box<dyn DomainObject>;
}

/// Recover the concrete type `T` from an object-safe [`DomainObject`]
/// reference, or `None` when `obj` does not hold a `T`.
#[must_use]
pub fn downcast<T: DomainType + 'static>(obj: &dyn DomainObject) -> Option<&T> {
    obj.as_any().downcast_ref::<T>()
}

/// Implement [`DomainObject`] for a hand-written, non-secret domain type,
/// from its existing `DomainType`, `Display`, `Clone`, `PartialEq` impls.
///
/// The storages `#[derive(DomainType)]` produces get `DomainObject` from
/// the derive itself; use this macro only for a type you write by hand,
/// such as a hand-written enum or a structured identity whose canonical
/// string is a documented join of its fields.
///
/// Applying it to a type whose [`crate::DomainType::IS_SECRET`] is `true`
/// is a compile error. Without that guard, one line of macro would give a
/// hand-written secret type an `is_secret()` of `false` and a `render()`
/// that publishes the secret's `Display` form as a plain value — the
/// exact leak the type system exists to prevent, introduced by the
/// shortest possible diff.
#[macro_export]
macro_rules! impl_domain_object_non_secret {
    ($ty:ty) => {
        const _: () = ::std::assert!(
            !<$ty as $crate::DomainType>::IS_SECRET,
            "impl_domain_object_non_secret! was used on a type whose \
             DomainType::IS_SECRET is true. A secret type must implement \
             DomainObject by hand, so that render() returns \
             Rendered::Redacted and expose() is the only way out."
        );

        impl $crate::object::DomainObject for $ty {
            fn type_name(&self) -> &'static str {
                <$ty as $crate::DomainType>::TYPE_NAME
            }

            fn is_secret(&self) -> bool {
                false
            }

            fn render(&self) -> $crate::object::Rendered {
                $crate::object::Rendered::Plain(::std::string::ToString::to_string(self))
            }

            fn expose(&self, _token: &$crate::SinkToken) -> ::std::string::String {
                ::std::string::ToString::to_string(self)
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn dyn_eq(&self, other: &dyn $crate::object::DomainObject) -> bool {
                other
                    .as_any()
                    .downcast_ref::<$ty>()
                    .is_some_and(|other| other == self)
            }

            fn clone_box(&self) -> ::std::boxed::Box<dyn $crate::object::DomainObject> {
                ::std::boxed::Box::new(::std::clone::Clone::clone(self))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_plain_displays_the_value() {
        assert_eq!(Rendered::Plain("hello".to_string()).to_string(), "hello");
    }

    #[test]
    fn rendered_redacted_displays_the_marker() {
        assert_eq!(
            Rendered::Redacted {
                type_name: "SomeSecret"
            }
            .to_string(),
            "[REDACTED SomeSecret]"
        );
    }

    #[test]
    fn downcast_recovers_the_concrete_type() {
        use crate::GitHubOrg;

        let org = GitHubOrg::parse("lightless-labs").unwrap();
        let obj: Box<dyn DomainObject> = Box::new(org.clone());
        let recovered = downcast::<GitHubOrg>(&*obj).expect("obj holds a GitHubOrg");
        assert_eq!(recovered, &org);
    }

    #[test]
    fn downcast_returns_none_for_the_wrong_type() {
        use crate::{GitHubOrg, HttpsUrl};

        let obj: Box<dyn DomainObject> = Box::new(HttpsUrl::parse("https://example.com").unwrap());
        assert!(downcast::<GitHubOrg>(&*obj).is_none());
    }

    #[test]
    fn rendered_serializes_as_a_plain_json_string() {
        assert_eq!(
            serde_json::to_string(&Rendered::Plain("hello".to_string())).unwrap(),
            "\"hello\""
        );
        assert_eq!(
            serde_json::to_string(&Rendered::Redacted {
                type_name: "SomeSecret"
            })
            .unwrap(),
            "\"[REDACTED SomeSecret]\""
        );
    }
}
