// `Credential` has no `Display` impl (only a redacted `Debug`): this must
// not compile.

fn main() {
    let credential = willikins_providers_http::Credential::from_env(
        "PATH",
        &regex::Regex::new(".*").unwrap(),
    )
    .unwrap();
    println!("{}", credential);
}
