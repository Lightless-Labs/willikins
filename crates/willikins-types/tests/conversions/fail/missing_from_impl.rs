//! `conversions![A => B]` must fail to compile when no `From<A> for B`
//! exists: the row cannot drift from an impl that does not exist. Both
//! types are public and derived, so both implement `DomainType` and
//! `DomainObject`; the only thing missing is the `From` impl
//! `__private::conversion::<A, B>` requires.

#[derive(willikins_types::DomainType)]
#[domain(pattern = "[a-z]+", description = "A.", example = "a")]
struct A(String);

#[derive(willikins_types::DomainType)]
#[domain(pattern = "[a-z]+", description = "B.", example = "b")]
struct B(String);

fn main() {
    let _ = willikins_types::conversions![A => B];
}
