// A domain type is a concrete newtype: the generated impls name the type
// without generics, so a generic struct is rejected at the macro rather
// than producing a pile of "undeclared type parameter" errors.

#[derive(willikins_derive::DomainType)]
#[domain(description = "d", example = "e")]
struct Foo<T>(T);

fn main() {}
