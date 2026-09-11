//! Adversarial tests for the crate's central claim: no secret byte reaches
//! any output.
//!
//! Every test here is written as an attack. It takes a secret value, puts
//! it somewhere a secret could plausibly escape — a `Box<dyn DomainObject>`
//! formatted with `{:?}`, a `Rendered` serialized to JSON, a `ParseError`'s
//! `Display` *and* `Debug` — and asserts the bytes are not there. The
//! parsing tests alongside them attack the grammar boundaries of the
//! structured identities, where a too-loose split is the way a wrong
//! resource gets addressed.

use willikins_types::{
    ActionsSecretName, DomainObject, DomainType, DopplerConfig, DopplerSecretValue,
    DopplerServiceToken, GitHubOrg, GitHubRepo, HttpsUrl, ProjectName, RepoVisibility, SinkToken,
    type_infos,
};

/// A token whose bytes are distinctive enough that any appearance in any
/// rendering is unmistakable.
const SECRET_BYTES: &str = "dp.st.fake-secret-bytes-aaaaaaaa";

/// The distinctive tail of [`SECRET_BYTES`], so a check also catches a
/// partial leak that drops the `dp.st.` prefix.
const SECRET_TAIL: &str = "fake-secret-bytes-aaaaaaaa";

fn boxed_token() -> Box<dyn DomainObject> {
    Box::new(DopplerServiceToken::parse(SECRET_BYTES).expect("the fixture token parses"))
}

/// Assert that `haystack`, produced by `what`, holds none of the secret.
fn assert_no_secret(what: &str, haystack: &str) {
    assert!(
        !haystack.contains(SECRET_BYTES) && !haystack.contains(SECRET_TAIL),
        "{what} leaked secret bytes: {haystack:?}"
    );
}

// -----------------------------------------------------------------
// A secret behind `Box<dyn DomainObject>`
// -----------------------------------------------------------------

#[test]
fn a_boxed_secret_never_shows_its_bytes_in_debug() {
    let boxed = boxed_token();
    let debug = format!("{boxed:?}");
    assert_no_secret("Debug of Box<dyn DomainObject>", &debug);
    assert_eq!(debug, "[REDACTED DopplerServiceToken]");
}

#[test]
fn a_boxed_secret_renders_and_serializes_as_the_marker() {
    let boxed = boxed_token();
    let rendered = boxed.render();

    let displayed = rendered.to_string();
    assert_no_secret("Display of render()", &displayed);
    assert_eq!(displayed, "[REDACTED DopplerServiceToken]");

    let json = serde_json::to_string(&rendered).expect("Rendered serializes");
    assert_no_secret("JSON of render()", &json);
    assert_eq!(json, "\"[REDACTED DopplerServiceToken]\"");

    assert!(boxed.is_secret());
    assert_eq!(boxed.type_name(), "DopplerServiceToken");
}

#[test]
fn a_boxed_secret_yields_its_bytes_only_through_the_sink_token() {
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = SinkToken::new();
    let boxed = boxed_token();
    assert_eq!(boxed.expose(&token), SECRET_BYTES);
}

#[test]
fn clone_box_preserves_secrecy() {
    let clone = boxed_token().clone_box();
    assert!(clone.is_secret(), "the clone forgot it was secret");
    assert_no_secret("Debug of clone_box()", &format!("{clone:?}"));
    assert_eq!(clone.render().to_string(), "[REDACTED DopplerServiceToken]");

    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = SinkToken::new();
    assert_eq!(
        clone.expose(&token),
        SECRET_BYTES,
        "the clone lost the value it was supposed to carry"
    );
}

// -----------------------------------------------------------------
// `dyn_eq` on secrets
// -----------------------------------------------------------------

#[test]
fn dyn_eq_is_true_for_two_secrets_with_the_same_bytes() {
    let one = boxed_token();
    let two = boxed_token();
    assert!(one.dyn_eq(two.as_ref()));
}

#[test]
fn dyn_eq_is_false_across_secret_values_and_types_without_leaking() {
    let one = boxed_token();
    let other: Box<dyn DomainObject> = Box::new(
        DopplerServiceToken::parse("dp.st.some-other-token-bbbbbbbb").expect("fixture parses"),
    );
    assert!(!one.dyn_eq(other.as_ref()));

    // The failing comparison is the moment a naive assertion message would
    // print both operands, so pin that both Debugs stay redacted.
    let both = format!("{one:?} vs {other:?}");
    assert_no_secret("Debug of an unequal pair", &both);
    assert!(!both.contains("some-other-token"), "leaked: {both}");

    // A different secret type with the same bytes is still not equal.
    let same_bytes_other_type: Box<dyn DomainObject> =
        Box::new(DopplerSecretValue::parse(SECRET_BYTES).expect("any non-empty string parses"));
    assert!(
        !one.dyn_eq(same_bytes_other_type.as_ref()),
        "dyn_eq crossed a type boundary"
    );
    assert_no_secret(
        "Debug of a cross-type comparison",
        &format!("{same_bytes_other_type:?}"),
    );
}

// -----------------------------------------------------------------
// `ParseError` from a failed secret parse
// -----------------------------------------------------------------

#[test]
fn a_secret_that_fails_its_pattern_leaves_no_bytes_in_the_error() {
    let rejected = "dp.ct.wrong-kind-of-token-ccccccc";
    let err = DopplerServiceToken::parse(rejected).expect_err("the wrong prefix is rejected");

    let displayed = err.to_string();
    let debugged = format!("{err:?}");
    for (what, text) in [("Display", &displayed), ("Debug", &debugged)] {
        assert!(
            !text.contains(rejected) && !text.contains("wrong-kind-of-token"),
            "{what} of a pattern ParseError leaked the input: {text}"
        );
    }
    assert_eq!(err.type_name, "DopplerServiceToken");
    assert!(displayed.contains("does not match the required pattern"));
}

#[test]
fn a_secret_that_fails_max_len_leaves_no_bytes_in_the_error() {
    let rejected = "q".repeat(65_537);
    let err = DopplerSecretValue::parse(&rejected).expect_err("over the limit is rejected");

    let displayed = err.to_string();
    let debugged = format!("{err:?}");
    for (what, text) in [("Display", &displayed), ("Debug", &debugged)] {
        assert!(
            !text.contains("qqqq"),
            "{what} of a max_len ParseError leaked the input: {text}"
        );
        assert!(
            text.len() < 200,
            "{what} of a max_len ParseError is suspiciously long ({} bytes), \
             which is what echoing the input would look like",
            text.len()
        );
    }
    assert!(displayed.contains("at most"));
}

#[test]
fn an_empty_secret_is_rejected_by_min_len() {
    let err = DopplerSecretValue::parse("").expect_err("the empty string is rejected");
    assert!(err.to_string().contains("at least"), "{err}");
}

// -----------------------------------------------------------------
// `GitHubRepo`
// -----------------------------------------------------------------

#[test]
fn github_repo_accepts_the_canonical_identity() {
    let repo = GitHubRepo::parse("lightless-labs/third-thoughts").expect("the fixture repo parses");
    assert_eq!(repo.to_string(), "lightless-labs/third-thoughts");
    assert_eq!(repo.owner().as_str(), "lightless-labs");
    assert_eq!(repo.name().to_string(), "third-thoughts");
}

#[test]
fn github_repo_rejects_a_malformed_split() {
    for bad in ["a/b/c", "/b", "a/", "", "a", "a//b", "a b/c"] {
        assert!(
            GitHubRepo::parse(bad).is_err(),
            "GitHubRepo accepted {bad:?}"
        );
    }
}

#[test]
fn github_repo_requires_the_name_to_be_a_slug_but_preserves_owner_case() {
    // The name is a `ProjectSlug`: lowercase, no leading digit, no
    // underscore, no doubled or edge hyphen.
    for bad_name in ["a/B", "a/Third-Thoughts", "a/3rd", "a/-x", "a/x-", "a/x_y"] {
        assert!(
            GitHubRepo::parse(bad_name).is_err(),
            "GitHubRepo accepted a non-slug name: {bad_name:?}"
        );
    }

    // The owner is a GitHub login, which GitHub itself case-preserves, so
    // an uppercase owner is valid and stays uppercase.
    let repo = GitHubRepo::parse("A/b").expect("an uppercase owner is a valid GitHub login");
    assert_eq!(repo.to_string(), "A/b");
}

#[test]
fn github_repo_url_is_always_a_valid_https_url() {
    for (owner, name) in [
        ("lightless-labs", "third-thoughts"),
        ("A", "b"),
        ("a0-b1-c2", "x"),
        (&"o".repeat(39), &"n".repeat(32)),
    ] {
        let repo = GitHubRepo::new(
            GitHubOrg::parse(owner).expect("fixture owner parses"),
            name.parse().expect("fixture name parses"),
        );
        let url = repo.url();
        assert_eq!(
            url.to_string(),
            format!("https://github.com/{owner}/{name}")
        );
        // The round trip is the real assertion: `url()` claims to be
        // panic-free, so whatever it produces must re-parse as `HttpsUrl`.
        assert!(HttpsUrl::parse(&url.to_string()).is_ok());
    }
}

// -----------------------------------------------------------------
// `ActionsSecretName`, `DopplerConfig`, `RepoVisibility`
// -----------------------------------------------------------------

#[test]
fn actions_secret_name_enforces_the_grammar_and_the_reserved_prefix() {
    for bad in [
        "GITHUB_TOKEN",
        "GITHUB_",
        "github_token",
        "1ABC",
        "",
        "A-B",
        "A B",
        "Ab",
    ] {
        assert!(
            ActionsSecretName::parse(bad).is_err(),
            "ActionsSecretName accepted {bad:?}"
        );
    }
    for good in ["DOPPLER_TOKEN", "_X", "A", "GITHUBX", "A1_B2"] {
        assert!(
            ActionsSecretName::parse(good).is_ok(),
            "ActionsSecretName rejected {good:?}"
        );
    }
}

#[test]
fn doppler_config_requires_exactly_one_separator() {
    for bad in ["p/prd/x", "/prd", "p/", "prd", "", "p//prd"] {
        assert!(
            DopplerConfig::parse(bad).is_err(),
            "DopplerConfig accepted {bad:?}"
        );
    }
    let config = DopplerConfig::parse("third-thoughts/prd").expect("the fixture config parses");
    assert_eq!(config.project().as_str(), "third-thoughts");
    assert_eq!(config.name().as_str(), "prd");
}

#[test]
fn repo_visibility_accepts_only_its_two_canonical_strings() {
    for bad in ["Private", "internal", "PUBLIC", "", " public", "public "] {
        assert!(
            RepoVisibility::parse(bad).is_err(),
            "RepoVisibility accepted {bad:?}"
        );
    }
    assert_eq!(
        RepoVisibility::parse("private").unwrap(),
        RepoVisibility::Private
    );
    assert_eq!(
        RepoVisibility::parse("public").unwrap(),
        RepoVisibility::Public
    );
}

// -----------------------------------------------------------------
// `ProjectName` and Unicode
// -----------------------------------------------------------------

#[test]
fn project_name_accepts_a_plain_display_name() {
    assert_eq!(
        ProjectName::parse("Third Thoughts").unwrap().as_str(),
        "Third Thoughts"
    );
}

#[test]
fn project_name_folds_a_no_break_space_to_a_space() {
    assert_eq!(
        ProjectName::parse("Third\u{00A0}Thoughts")
            .unwrap()
            .as_str(),
        "Third Thoughts"
    );
    assert_eq!(
        ProjectName::parse("\u{00A0}Third Thoughts\u{00A0}")
            .unwrap()
            .as_str(),
        "Third Thoughts"
    );
}

#[test]
fn project_name_rejects_invisible_and_bidi_controls() {
    for bad in [
        "Third\u{202E}Thoughts",
        "Third\u{200B}Thoughts",
        "Third\u{200E}Thoughts",
        "Third\u{2066}Thoughts",
        "Third\u{FEFF}Thoughts",
        "Third\u{00AD}Thoughts",
    ] {
        let err = ProjectName::parse(bad)
            .err()
            .unwrap_or_else(|| panic!("ProjectName accepted {bad:?}"));
        assert!(
            err.reason.contains("invisible or bidirectional control"),
            "unexpected reason for {bad:?}: {}",
            err.reason
        );
    }
}

// -----------------------------------------------------------------
// Pattern anchoring
// -----------------------------------------------------------------

#[test]
fn a_derived_pattern_matches_the_whole_input_including_a_trailing_newline() {
    // Rust's `regex` has no Perl-style "`$` also matches before a final
    // newline" rule, but the whole grammar rests on that, so pin it.
    for sneaky in ["lightless-labs\n", "\nlightless-labs", "a\nb", "a\tb"] {
        assert!(
            GitHubOrg::parse(sneaky).is_err(),
            "GitHubOrg accepted {sneaky:?}"
        );
    }
    for sneaky in ["https://github.com/a\n", "https://github.com/a b"] {
        assert!(
            HttpsUrl::parse(sneaky).is_err(),
            "HttpsUrl accepted {sneaky:?}"
        );
    }
}

// -----------------------------------------------------------------
// The catalog
// -----------------------------------------------------------------

#[test]
fn the_catalog_is_well_formed() {
    let infos = type_infos();
    let mut seen = std::collections::HashSet::new();
    for info in &infos {
        assert!(seen.insert(info.name), "duplicate type name {}", info.name);
        assert!(
            !info.description.trim().is_empty(),
            "{} has an empty description",
            info.name
        );
        assert!(
            !info.example.trim().is_empty(),
            "{} has an empty example",
            info.name
        );
    }
    assert!(infos.iter().any(|i| i.secret), "no secret type in catalog");
}

#[test]
fn no_catalog_entry_carries_a_secret_looking_example() {
    // A secret type's `example` is published in the catalog and in every
    // JSON schema, so it must be an obvious placeholder, never a real
    // credential that someone pasted in while testing.
    for info in type_infos().iter().filter(|i| i.secret) {
        let schema = serde_json::to_string(&info.schema).expect("schema serializes");
        assert!(
            info.example.contains("example") || info.example.contains("s3cr3t"),
            "{}'s example {:?} does not read as a placeholder",
            info.name,
            info.example
        );
        assert!(
            !schema.contains("dp.st.prd.") || info.example.contains("example"),
            "{}'s schema embeds a credential-shaped example",
            info.name
        );
    }
}

#[test]
fn project_name_rejects_unicode_line_and_paragraph_separators() {
    // U+2028 and U+2029 are category Zl/Zp, so `char::is_control` (which is
    // Cc only) misses them, yet they break a line in every renderer a
    // `ProjectName` reaches — a rendered `CLAUDE.md` included. A name is a
    // single line by definition, so they belong with the other invisibles.
    for bad in ["Third\u{2028}Thoughts", "Third\u{2029}Thoughts"] {
        assert!(
            ProjectName::parse(bad).is_err(),
            "ProjectName accepted a line separator: {bad:?}"
        );
    }
}
