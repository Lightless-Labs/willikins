// `#[derive(DomainType)]` only supports a newtype struct, not an enum.

#[derive(willikins_derive::DomainType)]
#[domain(description = "d", example = "e")]
enum Foo {
    A,
    B,
}

fn main() {}
