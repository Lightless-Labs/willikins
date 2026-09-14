// `Http` holds a `Credential`, so it deliberately has no `Debug`: there is
// no derived formatter that could ever reach the token. This must not
// compile.

fn main() {
    let credential = willikins_providers_http::Credential::from_env(
        "PATH",
        &regex::Regex::new(".*").unwrap(),
    )
    .unwrap();
    let http = willikins_providers_http::Http::new("http://127.0.0.1:1", Vec::new(), credential);
    println!("{:?}", http);
}
