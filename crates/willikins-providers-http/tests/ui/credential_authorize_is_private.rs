// `Credential::authorize` is crate-private: the builder it returns carries
// the bearer token in a header a caller could read straight back, which no
// clippy entry and no call-site guard would see. Outside the crate the only
// way to send a credential is through `Http`.

fn main() {
    let credential = willikins_providers_http::Credential::from_env(
        "PATH",
        &regex::Regex::new(".*").unwrap(),
    )
    .unwrap();
    let _ = credential.authorize(ureq::get("http://127.0.0.1:1/"));
}
