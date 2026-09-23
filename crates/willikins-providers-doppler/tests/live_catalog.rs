//! The *whole* live catalog — `willikins-tools`' seven pure tools,
//! `willikins-providers-github`'s two live tools, this crate's nine live
//! Doppler tools (milestone 3 added `doppler.config.inheritable.ensure`
//! and `doppler.config.inherits.ensure`; the `SigNoz` task added
//! `doppler.secret.set`; the App Store Connect credential correction
//! added `doppler.value.get`), (milestone 3a)
//! `willikins-providers-buildkite`'s two live tools, and (the App Store
//! Connect provider crate, plus milestone 3c's `appstore.certificate.get`)
//! `willikins-providers-appstore`'s four live tools (milestone 3c task 2
//! added `appstore.profile.ensure`). Twenty-five tools, no fake among
//! them.
//!
//! The assembly itself lives in `willikins-server` now
//! (`willikins_server::live_catalog_with`, task 10a's `Butler::live_catalog`)
//! rather than here: this file used to keep its own copy, which is
//! exactly the duplication task 10a's prompt asked to close ("move that
//! assembly here and make the doppler test call it, so it exists once").
//! A dev-only cycle back to `willikins-server` (which depends on this
//! crate normally) is what makes that possible; see this crate's
//! `Cargo.toml` for why that is safe.
//!
//! `catalog_parity.rs` pins each Doppler `ToolSpec` equal to the fake's
//! one spec at a time, and `catalog_check_parity.rs` pins that swapping
//! the live Doppler tools into an otherwise-fake catalog changes nothing
//! `check` can see. Neither of those, nor
//! `willikins-providers-github`'s mirror-image file (live GitHub, fake
//! Doppler), ever builds the catalog with *both* providers live — so
//! until this file, nothing proved the assembly `willikins-server` needs
//! is even constructible: that the nine specs' names do not collide, that
//! every one validates against the shared type registry, and that the
//! milestone's two positive fixtures -- plus the two App Store Connect
//! credential documents the credential-ports correction added, plus the
//! two documents that chain those parts into a real
//! `appstore.bundle_id.ensure` call -- still `check` against it,
//! resolving the same types in the same order as against the all-fake
//! catalog.
//!
//! No tool here is ever called. `check` is pure, so every client points
//! at a port nothing listens on, and every `Credential` is a
//! `for_testing` one.

use willikins_core::Catalog;
use willikins_providers_http::{Credential, Http};

/// The milestone's original two positive fixtures, plus the two App
/// Store Connect credential documents proving the credential's three
/// parts are genuinely free ports, plus the two documents that chain
/// those parts all the way into a real `appstore.bundle_id.ensure` call,
/// plus milestone 3c's positive document, which extends that chain through
/// `appstore.certificate.get` and `appstore.profile.ensure` into
/// `doppler.secret.set`.
const POSITIVE_FIXTURES: [&str; 7] = [
    "new-rust-service.yaml",
    "rotate-service-token.yaml",
    "apple-signing-credential-from-doppler.yaml",
    "apple-signing-credential-from-inputs.yaml",
    "appstore-bundle-id-from-doppler.yaml",
    "appstore-bundle-id-from-inputs.yaml",
    "appstore-signing-profile-from-doppler.yaml",
];

/// Every tool name the assembled live catalog must hold, exactly.
const LIVE_TOOL_NAMES: [&str; 25] = willikins_server::LIVE_TOOL_NAMES;

fn workflows_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("workflows")
}

/// A port nothing listens on: a `check` never calls a tool, and a test
/// that accidentally did would fail as a transport error rather than
/// reach the network.
const NOWHERE: &str = "http://127.0.0.1:1";

/// The live catalog, assembled by `willikins-server`.
fn live_catalog() -> Catalog {
    let doppler_credential =
        Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let github_credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let buildkite_credential =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken12345678");
    let signoz_credential =
        Credential::for_testing("WILLIKINS_TEST_SIGNOZ_API_KEY", "testsignozapikey00000000");
    willikins_server::live_catalog_with(
        Http::new(
            NOWHERE,
            willikins_providers_github::default_headers(),
            github_credential,
        ),
        Http::new(NOWHERE, Vec::new(), doppler_credential),
        Http::new(NOWHERE, Vec::new(), buildkite_credential),
        Http::new(NOWHERE, Vec::new(), signoz_credential),
    )
}

/// The assembly itself: twenty-five tools, no name collision, every spec
/// valid against the shared type registry.
#[test]
fn the_live_catalog_assembles_and_every_spec_validates() {
    let catalog = live_catalog();
    for name in LIVE_TOOL_NAMES {
        let tool_name = willikins_core::ToolName::parse(name)
            .unwrap_or_else(|err| panic!("`{name}` is a tool name: {err}"));
        let tool = catalog
            .get(&tool_name)
            .unwrap_or_else(|| panic!("the live catalog holds `{name}`"));
        tool.spec()
            .validate(willikins_types::registry())
            .unwrap_or_else(|err| panic!("`{name}`'s spec validates: {err:?}"));
    }
}

/// All six positive fixtures `check` against the fully-live catalog, and
/// resolve exactly what they resolve against the all-fake one: the same
/// types, output types, order, and class.
#[test]
fn both_positive_fixtures_check_the_same_against_the_live_catalog() {
    let (_state, fake_catalog) = willikins_providers_fake::empty();
    let live = live_catalog();

    for fixture in POSITIVE_FIXTURES {
        let workflow = willikins_dsl::load_document(&workflows_dir().join(fixture))
            .unwrap_or_else(|err| panic!("{fixture} loads: {err}"));
        let against_fake =
            willikins_core::check(&workflow, &fake_catalog).unwrap_or_else(|errors| {
                panic!("{fixture} checks against the fake catalog: {errors:?}")
            });
        let against_live = willikins_core::check(&workflow, &live).unwrap_or_else(|errors| {
            panic!("{fixture} checks against the fully-live catalog: {errors:?}")
        });
        assert_eq!(against_live.types, against_fake.types, "{fixture}: types");
        assert_eq!(
            against_live.output_types, against_fake.output_types,
            "{fixture}: output types"
        );
        assert_eq!(against_live.order, against_fake.order, "{fixture}: order");
        assert_eq!(against_live.class, against_fake.class, "{fixture}: class");
    }
}

/// Nothing fake survives in it: inserting any of the twenty-five fake tools
/// of the same names on top is refused as a duplicate, which is what
/// makes the two tests above statements about the live tools at all.
#[test]
fn no_fake_tool_fits_into_the_live_catalog() {
    let (_fake_state, fake_catalog) = willikins_providers_fake::empty();
    let mut catalog = live_catalog();
    for name in LIVE_TOOL_NAMES {
        let tool_name = willikins_core::ToolName::parse(name).expect("a tool name");
        let fake = fake_catalog
            .get(&tool_name)
            .unwrap_or_else(|| panic!("the fake catalog holds `{name}` too"));
        catalog
            .insert(fake.clone())
            .expect_err(&format!("`{name}` is already in the live catalog"));
    }
}
