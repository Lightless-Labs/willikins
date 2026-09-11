// A secret type generates no `Serialize`, so this fails with an ordinary
// trait-bound error, not a macro-time compile error.

#[derive(willikins_derive::DomainType)]
#[domain(secret, description = "d", example = "e")]
struct Foo(secrecy::SecretString);

fn main() {
    let value = <Foo as willikins_types::DomainType>::parse("hunter2").unwrap();
    let _ = serde_json::to_string(&value);
}
