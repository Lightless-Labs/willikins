// `secrecy::SecretString` storage requires `#[domain(secret, ...)]`.

#[derive(willikins_derive::DomainType)]
#[domain(description = "d", example = "e")]
struct Foo(secrecy::SecretString);

fn main() {}
