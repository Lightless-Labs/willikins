// `Credential` has no `Serialize` impl: this must not compile.

fn main() {
    let credential = willikins_providers_http::Credential::from_env(
        "PATH",
        &regex::Regex::new(".*").unwrap(),
    )
    .unwrap();
    let _ = serde_json::to_string(&credential);
}
