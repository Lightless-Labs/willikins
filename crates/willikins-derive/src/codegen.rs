//! Code generation for each storage kind.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::attrs::DomainAttrs;

/// A compiled-once regex static plus the identifier the generated `parse`
/// body refers to it by, when the type has a `pattern`.
struct PatternStatic {
    declaration: TokenStream,
    ident: syn::Ident,
}

fn pattern_static(name: &syn::Ident, anchored: Option<&str>) -> Option<PatternStatic> {
    let anchored = anchored?;
    let ident = format_ident!("__{}_PATTERN", name);
    let declaration = quote! {
        #[allow(non_upper_case_globals)]
        static #ident: ::std::sync::LazyLock<::willikins_types::__private::regex::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::willikins_types::__private::regex::Regex::new(#anchored)
                    .expect("pattern was validated as a regex at macro-expansion time")
            });
    };
    Some(PatternStatic { declaration, ident })
}

/// Build the `min_len`/`max_len`/`pattern` validation statements that run
/// against `target` (an expression yielding `&str`) inside `parse`. Every
/// rejection reason describes the constraint, never the rejected value, so
/// it is safe to reuse for secret storage.
fn checks(target: &TokenStream, attrs: &DomainAttrs, pattern: Option<&syn::Ident>) -> TokenStream {
    let mut out = TokenStream::new();

    if let Some(min) = attrs.min_len {
        out.extend(quote! {
            if #target.chars().count() < #min as usize {
                return ::std::result::Result::Err(::willikins_types::ParseError::new(
                    <Self as ::willikins_types::DomainType>::TYPE_NAME,
                    ::std::format!("must be at least {} characters long", #min),
                ));
            }
        });
    }

    if let Some(max) = attrs.max_len {
        out.extend(quote! {
            if #target.chars().count() > #max as usize {
                return ::std::result::Result::Err(::willikins_types::ParseError::new(
                    <Self as ::willikins_types::DomainType>::TYPE_NAME,
                    ::std::format!("must be at most {} characters long", #max),
                ));
            }
        });
    }

    if let Some(pattern) = pattern {
        out.extend(quote! {
            if !#pattern.is_match(#target) {
                return ::std::result::Result::Err(::willikins_types::ParseError::new(
                    <Self as ::willikins_types::DomainType>::TYPE_NAME,
                    "does not match the required pattern",
                ));
            }
        });
    }

    out
}

/// The shared `schemars::JsonSchema` impl: `{"type": "string", ...}` with
/// whichever of `pattern`/`minLength`/`maxLength` are configured, plus
/// `description` and `examples`.
fn json_schema_impl(name: &syn::Ident, attrs: &DomainAttrs, anchored: Option<&str>) -> TokenStream {
    let description = &attrs.description;
    let example = &attrs.example;

    let insert_pattern = anchored.map(|pattern| {
        quote! {
            object.insert(
                "pattern".to_string(),
                ::willikins_types::__private::serde_json::Value::String(#pattern.to_string()),
            );
        }
    });
    let insert_min = attrs.min_len.map(|min| {
        quote! {
            object.insert(
                "minLength".to_string(),
                ::willikins_types::__private::serde_json::Value::from(#min),
            );
        }
    });
    let insert_max = attrs.max_len.map(|max| {
        quote! {
            object.insert(
                "maxLength".to_string(),
                ::willikins_types::__private::serde_json::Value::from(#max),
            );
        }
    });

    quote! {
        impl ::willikins_types::__private::schemars::JsonSchema for #name {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                ::std::borrow::Cow::Borrowed(stringify!(#name))
            }

            fn json_schema(
                _generator: &mut ::willikins_types::__private::schemars::SchemaGenerator,
            ) -> ::willikins_types::__private::schemars::Schema {
                let mut object = ::willikins_types::__private::serde_json::Map::new();
                object.insert(
                    "type".to_string(),
                    ::willikins_types::__private::serde_json::Value::String("string".to_string()),
                );
                object.insert(
                    "description".to_string(),
                    ::willikins_types::__private::serde_json::Value::String(#description.to_string()),
                );
                object.insert(
                    "examples".to_string(),
                    ::willikins_types::__private::serde_json::Value::Array(::std::vec![
                        ::willikins_types::__private::serde_json::Value::String(#example.to_string()),
                    ]),
                );
                #insert_pattern
                #insert_min
                #insert_max
                <::willikins_types::__private::schemars::Schema as ::std::convert::TryFrom<_>>::try_from(
                    ::willikins_types::__private::serde_json::Value::Object(object),
                )
                .expect("generated schema object is a valid JSON Schema object")
            }
        }
    }
}

/// The `DomainObject` impl shared by the two non-secret storages (`String`
/// and `Other`): rendering and exposing both go through `Display`, since
/// neither storage hides anything.
fn domain_object_non_secret(name: &syn::Ident) -> TokenStream {
    quote! {
        impl ::willikins_types::object::DomainObject for #name {
            fn type_name(&self) -> &'static str {
                <Self as ::willikins_types::DomainType>::TYPE_NAME
            }

            fn is_secret(&self) -> bool {
                false
            }

            fn render(&self) -> ::willikins_types::object::Rendered {
                ::willikins_types::object::Rendered::Plain(::std::string::ToString::to_string(self))
            }

            fn expose(&self, _token: &::willikins_types::SinkToken) -> ::std::string::String {
                ::std::string::ToString::to_string(self)
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn dyn_eq(&self, other: &dyn ::willikins_types::object::DomainObject) -> bool {
                other
                    .as_any()
                    .downcast_ref::<Self>()
                    .is_some_and(|other| other == self)
            }

            fn clone_box(&self) -> ::std::boxed::Box<dyn ::willikins_types::object::DomainObject> {
                ::std::boxed::Box::new(::std::clone::Clone::clone(self))
            }
        }
    }
}

/// The `DomainObject` impl for the secret storage: `render` and `expose`
/// route through the type's own redacted `Display` and inherent `expose`,
/// so this cannot drift from the redaction rule the type already obeys.
/// `self.expose(token)` here resolves to the inherent method (it returns
/// `&str`; the trait method returns `String`), never back to this trait
/// method, because an inherent method always takes priority over a trait
/// method of the same name.
fn domain_object_secret(name: &syn::Ident) -> TokenStream {
    quote! {
        impl ::willikins_types::object::DomainObject for #name {
            fn type_name(&self) -> &'static str {
                <Self as ::willikins_types::DomainType>::TYPE_NAME
            }

            fn is_secret(&self) -> bool {
                true
            }

            fn render(&self) -> ::willikins_types::object::Rendered {
                ::willikins_types::object::Rendered::Redacted {
                    type_name: <Self as ::willikins_types::DomainType>::TYPE_NAME,
                }
            }

            fn expose(&self, token: &::willikins_types::SinkToken) -> ::std::string::String {
                ::std::string::ToString::to_string(self.expose(token))
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn dyn_eq(&self, other: &dyn ::willikins_types::object::DomainObject) -> bool {
                other
                    .as_any()
                    .downcast_ref::<Self>()
                    .is_some_and(|other| other == self)
            }

            fn clone_box(&self) -> ::std::boxed::Box<dyn ::willikins_types::object::DomainObject> {
                ::std::boxed::Box::new(::std::clone::Clone::clone(self))
            }
        }
    }
}

/// The `Clone`/`PartialEq`/`Eq` impls shared by non-secret storages, which
/// simply delegate to the field.
fn structural_eq_and_clone(name: &syn::Ident) -> TokenStream {
    quote! {
        impl ::std::clone::Clone for #name {
            fn clone(&self) -> Self {
                Self(::std::clone::Clone::clone(&self.0))
            }
        }

        impl ::std::cmp::PartialEq for #name {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl ::std::cmp::Eq for #name {}
    }
}

/// `String` storage: `parse` validates and stores the input verbatim.
pub fn gen_string(name: &syn::Ident, attrs: &DomainAttrs, anchored: Option<&str>) -> TokenStream {
    let description = &attrs.description;
    let example = &attrs.example;

    let pattern = pattern_static(name, anchored);
    let pattern_decl = pattern.as_ref().map(|p| &p.declaration);
    let pattern_ident = pattern.as_ref().map(|p| &p.ident);

    let target = quote!(input);
    let checks = checks(&target, attrs, pattern_ident);
    let json_schema_impl = json_schema_impl(name, attrs, anchored);
    let eq_and_clone = structural_eq_and_clone(name);
    let domain_object_impl = domain_object_non_secret(name);

    quote! {
        #pattern_decl

        impl #name {
            /// Borrow the canonical string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::willikins_types::DomainType for #name {
            const TYPE_NAME: &'static str = stringify!(#name);
            const IS_SECRET: bool = false;

            fn description() -> &'static str {
                #description
            }

            fn example() -> &'static str {
                #example
            }

            fn parse(input: &str) -> ::std::result::Result<Self, ::willikins_types::ParseError> {
                #checks
                ::std::result::Result::Ok(Self(input.to_string()))
            }

            fn json_schema() -> ::willikins_types::__private::schemars::Schema {
                ::willikins_types::__private::schemars::schema_for!(Self)
            }
        }

        impl ::std::str::FromStr for #name {
            type Err = ::willikins_types::ParseError;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                <Self as ::willikins_types::DomainType>::parse(s)
            }
        }

        #eq_and_clone

        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::std::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_tuple(stringify!(#name)).field(&self.0).finish()
            }
        }

        impl ::willikins_types::__private::serde::Serialize for #name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::willikins_types::__private::serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> ::willikins_types::__private::serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::willikins_types::__private::serde::Deserializer<'de>,
            {
                let s = <::std::string::String as ::willikins_types::__private::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                <Self as ::willikins_types::DomainType>::parse(&s)
                    .map_err(::willikins_types::__private::serde::de::Error::custom)
            }
        }

        #json_schema_impl
        #domain_object_impl
    }
}

/// `secrecy::SecretString` storage: `parse` validates the raw `&str`
/// before wrapping it. No `Serialize` is generated.
pub fn gen_secret(name: &syn::Ident, attrs: &DomainAttrs, anchored: Option<&str>) -> TokenStream {
    let description = &attrs.description;
    let example = &attrs.example;

    let pattern = pattern_static(name, anchored);
    let pattern_decl = pattern.as_ref().map(|p| &p.declaration);
    let pattern_ident = pattern.as_ref().map(|p| &p.ident);

    let target = quote!(input);
    let checks = checks(&target, attrs, pattern_ident);
    let json_schema_impl = json_schema_impl(name, attrs, anchored);
    let domain_object_impl = domain_object_secret(name);

    quote! {
        #pattern_decl

        impl #name {
            /// Expose the secret's raw value. Requires a
            /// [`SinkToken`](::willikins_types::SinkToken), which only code
            /// running inside the apply executor can construct, so callers
            /// cannot expose a secret by accident.
            #[must_use]
            pub fn expose(&self, _token: &::willikins_types::SinkToken) -> &str {
                ::willikins_types::__private::secrecy::ExposeSecret::expose_secret(&self.0)
            }
        }

        impl ::willikins_types::DomainType for #name {
            const TYPE_NAME: &'static str = stringify!(#name);
            const IS_SECRET: bool = true;

            fn description() -> &'static str {
                #description
            }

            fn example() -> &'static str {
                #example
            }

            fn parse(input: &str) -> ::std::result::Result<Self, ::willikins_types::ParseError> {
                #checks
                ::std::result::Result::Ok(Self(
                    ::willikins_types::__private::secrecy::SecretString::from(input),
                ))
            }

            fn json_schema() -> ::willikins_types::__private::schemars::Schema {
                ::willikins_types::__private::schemars::schema_for!(Self)
            }
        }

        impl ::std::str::FromStr for #name {
            type Err = ::willikins_types::ParseError;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                <Self as ::willikins_types::DomainType>::parse(s)
            }
        }

        impl ::std::clone::Clone for #name {
            fn clone(&self) -> Self {
                Self(::std::clone::Clone::clone(&self.0))
            }
        }

        impl ::std::cmp::PartialEq for #name {
            fn eq(&self, other: &Self) -> bool {
                ::willikins_types::__private::secrecy::ExposeSecret::expose_secret(&self.0)
                    == ::willikins_types::__private::secrecy::ExposeSecret::expose_secret(&other.0)
            }
        }

        impl ::std::cmp::Eq for #name {}

        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "[REDACTED {}]", stringify!(#name))
            }
        }

        impl ::std::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "[REDACTED {}]", stringify!(#name))
            }
        }

        impl<'de> ::willikins_types::__private::serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::willikins_types::__private::serde::Deserializer<'de>,
            {
                let s = <::std::string::String as ::willikins_types::__private::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                <Self as ::willikins_types::DomainType>::parse(&s)
                    .map_err(::willikins_types::__private::serde::de::Error::custom)
            }
        }

        #json_schema_impl
        #domain_object_impl
    }
}

/// Any other inner type, used through `FromStr` + `Display`: `parse`
/// delegates to `Inner::from_str`, then validates the *canonical* (`Display`)
/// form against `min_len`/`max_len`/`pattern`.
pub fn gen_other(
    name: &syn::Ident,
    inner_ty: &syn::Type,
    attrs: &DomainAttrs,
    anchored: Option<&str>,
) -> TokenStream {
    let description = &attrs.description;
    let example = &attrs.example;

    let pattern = pattern_static(name, anchored);
    let pattern_decl = pattern.as_ref().map(|p| &p.declaration);
    let pattern_ident = pattern.as_ref().map(|p| &p.ident);

    let target = quote!(canonical.as_str());
    let checks = checks(&target, attrs, pattern_ident);
    let json_schema_impl = json_schema_impl(name, attrs, anchored);
    let eq_and_clone = structural_eq_and_clone(name);
    let domain_object_impl = domain_object_non_secret(name);

    quote! {
        #pattern_decl

        impl #name {
            /// Borrow the inner value.
            #[must_use]
            pub fn inner(&self) -> &#inner_ty {
                &self.0
            }
        }

        impl ::willikins_types::DomainType for #name {
            const TYPE_NAME: &'static str = stringify!(#name);
            const IS_SECRET: bool = false;

            fn description() -> &'static str {
                #description
            }

            fn example() -> &'static str {
                #example
            }

            fn parse(input: &str) -> ::std::result::Result<Self, ::willikins_types::ParseError> {
                let inner = <#inner_ty as ::std::str::FromStr>::from_str(input).map_err(|err| {
                    ::willikins_types::ParseError::new(
                        <Self as ::willikins_types::DomainType>::TYPE_NAME,
                        ::std::string::ToString::to_string(&err),
                    )
                })?;
                let canonical = ::std::string::ToString::to_string(&inner);
                #checks
                ::std::result::Result::Ok(Self(inner))
            }

            fn json_schema() -> ::willikins_types::__private::schemars::Schema {
                ::willikins_types::__private::schemars::schema_for!(Self)
            }
        }

        impl ::std::str::FromStr for #name {
            type Err = ::willikins_types::ParseError;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                <Self as ::willikins_types::DomainType>::parse(s)
            }
        }

        #eq_and_clone

        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_tuple(stringify!(#name)).field(&self.0).finish()
            }
        }

        impl ::willikins_types::__private::serde::Serialize for #name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::willikins_types::__private::serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> ::willikins_types::__private::serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::willikins_types::__private::serde::Deserializer<'de>,
            {
                let s = <::std::string::String as ::willikins_types::__private::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                <Self as ::willikins_types::DomainType>::parse(&s)
                    .map_err(::willikins_types::__private::serde::de::Error::custom)
            }
        }

        #json_schema_impl
        #domain_object_impl
    }
}
