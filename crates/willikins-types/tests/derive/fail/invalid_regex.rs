// An invalid `pattern` is a compile error at macro-expansion time.

#[derive(willikins_derive::DomainType)]
#[domain(pattern = "[", description = "d", example = "e")]
struct Foo(String);

fn main() {}
