// `#[domain(secret)]` is only valid on `secrecy::SecretString` storage.

#[derive(willikins_derive::DomainType)]
#[domain(secret, description = "d", example = "e")]
struct Foo(String);

fn main() {}
