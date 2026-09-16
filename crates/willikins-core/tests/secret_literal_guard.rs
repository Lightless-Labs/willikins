//! Nothing in this repository may spell a provider-token-shaped literal.
//!
//! The 2026-09-16 credential-boundary addendum (milestone 2 plan) retired
//! the coordinator's practice of clearing a blocked push with GitHub's
//! secret-scanning push-protection bypass API: a push is blocked because
//! a file in this repository contains a literal shaped like a real
//! Doppler or GitHub token, and disabling the control that notices that
//! with the operator's own credential is the wrong end of the problem.
//! The fix is upstream of the scanner: no such literal is ever written
//! in the first place. Every fixture, marker, and test token that needs
//! to satisfy a real type's own pattern (most commonly
//! [`willikins_types::DopplerServiceToken`]'s `^dp\.st\.(?:env\.)?
//! [A-Za-z0-9]{40,44}$`, or `willikins-providers-doppler`'s
//! `CREDENTIAL_PATTERN`, `^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$`) is instead
//! assembled at compile time with `concat!("dp.st.", "...")` (or split
//! across a JSON placeholder substituted at runtime, for the one fixture
//! that cannot use `concat!`: see
//! `crates/willikins-providers-doppler/fixtures/doppler/README.md`) so
//! the compiled value is byte-identical to what the type's own pattern
//! (or a test's deliberately-invalid shape) requires, while no single
//! literal in any source file spells the whole thing contiguously.
//!
//! # What this walks
//!
//! Every file in the repository except [`SKIPPED_DIRS`] and this file
//! itself (which necessarily carries real matching strings, assembled at
//! runtime in its own unit tests below, to prove the matcher recognises
//! them) -- source code and documentation alike, per the addendum's "the
//! synthetic provider-shaped literals in fixtures are what trip the
//! scanner" (fixtures are data, not code, and a `.md` research note is
//! read by the same scanner as a `.rs` file). A file that fails to parse
//! as UTF-8 is skipped as binary; the repository holds none as of this
//! writing (confirmed by [`the_walk_covers_every_known_text_extension`]).
//!
//! # The shapes matched, and why patterns are not literals
//!
//! [`DOPPLER_NON_ST`], [`DOPPLER_ST`], [`GITHUB_TOKEN`], and
//! [`BUILDKITE_TOKEN`] mirror what the *detector* looks for, not what
//! this workspace issues or parses. That distinction is the whole design
//! of these constants, and it was learned the hard way: the first
//! version of this guard was written from `willikins-providers-doppler`'s
//! and `-github`'s own `CREDENTIAL_PATTERN`s -- two GitHub prefixes and
//! three Doppler kinds -- and passed green over a tree that still carried
//! a `gho_` OAuth token quoted from GitHub's own documentation and a
//! `dp.st.PRD.` service token with an uppercase environment segment. Both
//! are shapes GitHub's published secret-scanning pattern list marks
//! push-protected; neither is a shape willikins would ever authenticate
//! as. The GitHub and Doppler lists come from GitHub's own tables (read
//! 2026-09-16, cited on each constant): six GitHub prefixes and six
//! Doppler token kinds. The milestone 3a addendum (2026-09-16) applies
//! the same principle to the new provider before willikins ever holds one
//! of its tokens: [`BUILDKITE_TOKEN`] lists all nine prefixes Buildkite's
//! own security documentation publishes, not only the one family
//! `willikins-providers-buildkite` will authenticate with.
//!
//! The bounds are deliberately unbounded above (`{40,}`) rather than the
//! `{40,44}` willikins' own types enforce: a real scanner has no reason
//! to stop at 44, and neither should this one -- see
//! [`a_45_character_run_still_counts_as_long`].
//!
//! A regex *pattern* that states one of these shapes -- `CREDENTIAL_PATTERN`
//! in `willikins-providers-doppler/src/client.rs`, or
//! [`willikins_types::DopplerServiceToken`]'s own `#[domain(pattern =
//! ...)]` -- is never confused with a literal, and needs no file-based
//! exemption: a pattern spells its separators as `\.` (an escaped dot,
//! two source bytes), while a real literal spells a plain `.` (one
//! byte). This guard's own regexes require an *unescaped* dot at each
//! separator, so they cannot match text that escaped it, by construction
//! -- pinned by [`a_pattern_constant_is_never_flagged`], not merely
//! assumed.

use std::path::{Path, PathBuf};

use regex::Regex;

/// The repository root, from this test's own manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate>/ has a grandparent")
        .to_path_buf()
}

/// Directories that hold no file worth scanning: build output, VCS
/// internals, an npm tree, this session's own gitignored worktree
/// scratch space (`.claude/worktrees/`, per the monorepo's process doc),
/// and the two gitignored `fixtures/*/live/` directories a live test run
/// leaves on disk (`.gitignore` names both). Those hold real, redacted
/// recordings -- never pushed, since they are gitignored, so scanning
/// them serves nothing this guard exists for and would make its result
/// depend on whether the machine running it has ever run a live test.
const SKIPPED_DIRS: &[&str] = &["target", ".git", "node_modules", ".claude", "live"];

/// This file: it necessarily carries real matching strings (assembled at
/// runtime in its own unit tests, never spelled contiguously in its own
/// source) to prove the matcher below recognises them.
fn is_exempt(relative: &str) -> bool {
    relative == "crates/willikins-core/tests/secret_literal_guard.rs"
}

/// Every file in the tree, repository-relative, `/`-separated, skipping
/// [`SKIPPED_DIRS`] and [`is_exempt`].
fn swept_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("readable directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        if entry.file_type().expect("file type").is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                swept_files(&entry.path(), &format!("{relative}/"), out);
            }
            continue;
        }
        if !is_exempt(&relative) {
            out.push(relative);
        }
    }
}

/// Every Doppler token kind that is not a service token: `dp.` then one
/// of `sa`, `pt`, `ct`, `scim` or `audit`, then 40 or more plain
/// alphanumeric characters, no dots or hyphens inside the run.
///
/// The kind list is not this workspace's -- it is GitHub's. Their
/// published secret-scanning pattern list (`/code-security/secret-scanning/
/// introduction/supported-secret-scanning-patterns`, read 2026-09-16)
/// carries six Doppler rows, each with push protection supported:
/// Audit Token, CLI Token, Personal Token, SCIM Token, Service Account
/// Token and Service Token. A guard narrower than the detector it exists
/// to stay ahead of would let exactly the push this repository must not
/// need a bypass for get blocked. `willikins-providers-doppler`'s own
/// `CREDENTIAL_PATTERN` covers only `sa`/`pt` because those are the only
/// kinds it will *authenticate* as; the other four are just as real a
/// leak.
const DOPPLER_NON_ST: &str = r"dp\.(?:sa|pt|ct|scim|audit)\.[A-Za-z0-9]{40,}";

/// A Doppler service token: `dp.st.`, an optional environment-like
/// segment (2-35 characters, then a dot), then 40 or more plain
/// alphanumeric characters.
///
/// Deliberately *wider* than [`willikins_types::DopplerServiceToken`]'s
/// own pattern, which requires that segment to be lowercase because that
/// is all Doppler issues. What Doppler issues and what a detector matches
/// are different questions, and this guard answers the second: an
/// uppercase segment is one keypress from a lowercase one, and nothing
/// is known about the case-sensitivity of GitHub's side. The cost of
/// being wide here is a `concat!` at a call site; the cost of being
/// narrow is a blocked push -- see
/// [`an_uppercase_environment_segment_is_still_flagged`].
const DOPPLER_ST: &str = r"dp\.st\.(?:[A-Za-z0-9_-]{2,35}\.)?[A-Za-z0-9]{40,}";

/// Any token GitHub issues, by the prefix it stamps on it.
///
/// All six, not the two this workspace authenticates with: GitHub's
/// `about-authentication-to-github` table (read 2026-09-16) names `ghp_`
/// (personal access, classic), `github_pat_` (fine-grained), `gho_`
/// (OAuth access), `ghu_` (user-to-server), `ghs_` (server-to-server)
/// and `ghr_` (refresh), and their secret-scanning pattern list marks
/// push protection supported for every one. The first version of this
/// guard listed only the two `willikins-providers-github`'s
/// `CREDENTIAL_PATTERN` accepts, and a `gho_` token quoted verbatim from
/// GitHub's own OAuth documentation sat unflagged in
/// `docs/research/2026-09-16-m2c-authorization.md` the whole time it was
/// green -- see [`flags_every_github_prefix`].
const GITHUB_TOKEN: &str = r"(?:github_pat_|ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9_]{20,}";

/// Every Buildkite token family, by the prefix it stamps on it: `bk` plus
/// one of nine published acronyms, an underscore, then a long run.
///
/// All nine prefixes Buildkite documents
/// (`platform/security/tokens.md`, read 2026-09-16 for the milestone 3a
/// research note), not only `bkua_`, the one family willikins itself
/// authenticates with: `bkua_` (API access -- "Buildkite user access"),
/// `bkaa_` (agent session), `bkaj_` (agent job), `bkar_` (unclustered
/// agent), `bkct_` (agent / cluster), `bkpt_` (registry), `bkpat_`
/// (portal), `bkps_` (portal secret), `bkjat_` (job acquisition). A guard
/// that knew only the token willikins holds would miss every agent,
/// portal and registry token -- the same argument section 4 of the
/// research note (`docs/research/2026-09-16-m3a-buildkite.md`) makes for
/// the type-level pattern.
///
/// The body class is `[A-Za-z0-9_-]`, wider than the plain-alphanumeric
/// run the other two constants use above: `willikins-providers-buildkite`'s
/// own `CREDENTIAL_PATTERN` (`^bkua_[A-Za-z0-9_-]{20,}$`) is deliberately
/// tolerant of the same class, because Buildkite masks every published
/// token body with asterisks and gives no character-class rule at all --
/// a pattern narrower than the credential it watches for would be the
/// same mistake the module doc's `gho_` story already recorded once. The
/// length floor, 20, is likewise chosen rather than derived, for the same
/// masked-body reason; it is not copied from any type's own upper bound
/// because none of these families has one this crate could cite.
const BUILDKITE_TOKEN: &str = r"bk(?:ua|aa|aj|ar|ct|pt|pat|ps|jat)_[A-Za-z0-9_-]{20,}";

/// Every offense a compiled matcher finds in `text`, as `(line_number,
/// matched_text)`, 1-indexed to match an editor's line numbers. Every
/// match on a line, not the first: a line that spells two tokens must
/// name two, or fixing the one reported uncovers the other on the next
/// run instead of the same one.
fn offenses_in(matcher: &Regex, text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let found: Vec<(usize, String)> = matcher
                .find_iter(line)
                .map(|found| (index + 1, found.as_str().to_string()))
                .collect();
            (!found.is_empty()).then_some(found)
        })
        .flatten()
        .collect()
}

#[test]
fn no_provider_token_shaped_literal_anywhere_in_the_tree() {
    let matcher = Regex::new(&format!(
        "{DOPPLER_NON_ST}|{DOPPLER_ST}|{GITHUB_TOKEN}|{BUILDKITE_TOKEN}"
    ))
    .expect("the combined matcher is a valid regex");

    let root = repo_root();
    let mut files = Vec::new();
    swept_files(&root, "", &mut files);
    assert!(
        files.len() > 100,
        "the walk found only {} files; it is not reading the tree",
        files.len()
    );

    let mut offenders = Vec::new();
    for relative in &files {
        // A file that does not parse as UTF-8 is binary; this repository
        // holds none as of this writing, so skipping it costs nothing
        // real and keeps this guard from panicking on one added later.
        let Ok(text) = std::fs::read_to_string(root.join(relative)) else {
            continue;
        };
        for (line, matched) in offenses_in(&matcher, &text) {
            offenders.push(format!("{relative}:{line}: {matched}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "these lines carry a provider-token-shaped literal; assemble the \
         value at compile time instead (`concat!(\"dp.st.\", \"...\")`, or \
         a JSON placeholder substituted at runtime -- see \
         `crates/willikins-providers-doppler/fixtures/doppler/README.md`), \
         and never clear the resulting push block with the secret-scanning \
         bypass API (the 2026-09-16 credential-boundary addendum):\n{}",
        offenders.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> Regex {
        Regex::new(&format!(
            "{DOPPLER_NON_ST}|{DOPPLER_ST}|{GITHUB_TOKEN}|{BUILDKITE_TOKEN}"
        ))
        .unwrap()
    }

    /// The two shapes stated in this repository's own research note
    /// (`docs/research/2026-09-12-m2-dependencies.md`, section 3, now
    /// masked there for the same reason this guard exists) -- one with
    /// the optional environment segment and one without -- assembled at
    /// runtime so this file's own source never spells either
    /// contiguously.
    #[test]
    fn flags_dopplers_own_documented_service_token_examples() {
        let with_env = concat!("dp.st.dev.", "bAqhcVzrhy5cRHkOlNTc0Ve6w5NUDCpcutm8vGE9myi");
        let without_env = concat!("dp.st.", "gJ23agW5s09x4TKLMJMc4OPIr9fCm3bIs0QAC2L5");
        assert!(matcher().is_match(with_env), "{with_env}");
        assert!(matcher().is_match(without_env), "{without_env}");
    }

    #[test]
    fn flags_a_service_account_and_a_personal_token() {
        let sa = concat!("dp.sa.", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let pt = concat!("dp.pt.", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert!(matcher().is_match(sa), "{sa}");
        assert!(matcher().is_match(pt), "{pt}");
    }

    #[test]
    fn flags_a_github_classic_and_fine_grained_pat() {
        let classic = concat!("ghp_", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let fine_grained = concat!(
            "github_pat_",
            "11AAAAAAA0aaaaaaaaaaaa_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(matcher().is_match(classic), "{classic}");
        assert!(matcher().is_match(fine_grained), "{fine_grained}");
    }

    /// Every prefix GitHub stamps on a token it issues, not only the two
    /// `willikins-providers-github`'s `CREDENTIAL_PATTERN` accepts. The
    /// `gho_` case is the one that was actually missed: GitHub's OAuth
    /// documentation spells a worked example token, this repository's
    /// milestone 2c research note quoted that example verbatim, and the
    /// first version of this guard did not look for the prefix.
    #[test]
    fn flags_every_github_prefix() {
        let suffix = "16C7e42F292c6912E7710c838347Ae178B4a";
        for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
            let token = format!("{prefix}{suffix}");
            assert!(matcher().is_match(&token), "{prefix} was not flagged");
        }
    }

    /// Every Doppler token kind GitHub's pattern list marks
    /// push-protected, including the two (`scim`, `audit`) this workspace
    /// has no type for and will never authenticate as. A leak is a leak
    /// whether or not willikins can parse what leaked.
    #[test]
    fn flags_every_doppler_token_kind() {
        let suffix = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for kind in ["sa", "pt", "ct", "scim", "audit"] {
            let token = format!("dp.{kind}.{suffix}");
            assert!(matcher().is_match(&token), "dp.{kind}. was not flagged");
        }
        let service = format!("dp.st.{suffix}");
        assert!(matcher().is_match(&service), "{service}");
    }

    /// Every prefix Buildkite's own security documentation stamps on a
    /// token it issues, not only `bkua_`, the one family willikins itself
    /// authenticates with (see [`BUILDKITE_TOKEN`]'s doc comment for the
    /// full list and its source).
    #[test]
    fn flags_every_buildkite_prefix() {
        let suffix = "aaaaaaaaaaaaaaaaaaaaaaaa";
        for prefix in [
            "bkua_", "bkaa_", "bkaj_", "bkar_", "bkct_", "bkpt_", "bkpat_", "bkps_", "bkjat_",
        ] {
            let token = format!("{prefix}{suffix}");
            assert!(matcher().is_match(&token), "{prefix} was not flagged");
        }
    }

    /// `bkup_` is not one of the nine prefixes Buildkite publishes -- it
    /// is not, for instance, a typo `bkua_` would fold to -- so a run
    /// behind it must not be flagged. A guard that matched any `bk..._`
    /// shape indiscriminately would also light up on unrelated
    /// identifiers this workspace writes for its own reasons.
    #[test]
    fn does_not_flag_an_undocumented_buildkite_prefix() {
        let token = concat!("bkup_", "aaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(!matcher().is_match(token), "{token}");
    }

    /// `willikins-providers-buildkite`'s own `CREDENTIAL_PATTERN`
    /// (`^bkua_[A-Za-z0-9_-]{20,}$`) must not be flagged. Unlike the
    /// Doppler patterns above, the reason is not an escaped separator --
    /// this prefix has no dot to escape -- it is that the bracket
    /// expression `[A-Za-z0-9_-]` immediately after `bkua_` opens with
    /// `[`, a character outside [`BUILDKITE_TOKEN`]'s own run class, so
    /// the run this guard looks for never starts: a regex *source* is
    /// syntax, not a run of token-shaped bytes, and it is the character
    /// class's own bracket that breaks the match here, not escaping.
    #[test]
    fn a_buildkite_pattern_constant_is_never_flagged() {
        let credential_pattern = "^bkua_[A-Za-z0-9_-]{20,}$";
        assert!(
            !matcher().is_match(credential_pattern),
            "{credential_pattern}"
        );
    }

    /// An uppercase environment segment. `DopplerServiceToken` rejects
    /// this shape -- Doppler only ever issues a lowercase one -- and
    /// `willikins-types/src/doppler.rs` carried it for exactly that
    /// reason, as a "the type refuses this" case. It is still a
    /// token-shaped literal: what the type accepts and what a detector
    /// matches are different questions, and only the second one decides
    /// whether a push is blocked.
    #[test]
    fn an_uppercase_environment_segment_is_still_flagged() {
        let uppercase = concat!("dp.st.PRD.", "exampleexampleexampleexampleexampleexample");
        assert!(matcher().is_match(uppercase), "{uppercase}");
    }

    /// A shape only 45 characters long -- one past
    /// `DopplerServiceToken`'s own `{40,44}` upper bound -- must still be
    /// flagged: a real scanner has no reason to stop at 44, and this
    /// guard's own bound is deliberately unbounded (`{40,}`), not copied
    /// from the type's.
    #[test]
    fn a_45_character_run_still_counts_as_long() {
        let too_long_for_the_type =
            concat!("dp.st.", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(matcher().is_match(too_long_for_the_type));
    }

    /// A hyphen, an internal dot, or too short a run is not a real
    /// Doppler token shape and must not be flagged -- these are exactly
    /// the deliberately-invalid shapes `willikins-types/src/doppler.rs`'s
    /// own rejection tests use.
    #[test]
    fn a_short_or_malformed_run_is_not_flagged() {
        for safe in [
            "dp.st.short",
            "dp.sa.teardown-shape-test-token",
            "dp.ct.wrong-kind-of-token-ccccccc",
            concat!("dp.st.", "aaaaaaaaaa-aaaaaaaaaa-aaaaaaaaaa-aaaaaaaaaa"),
            concat!("dp.st.", "aaaaaaaaaa.aaaaaaaaaa.aaaaaaaaaa.aaaaaaaaaa"),
        ] {
            assert!(!matcher().is_match(safe), "{safe} was flagged");
        }
    }

    /// The single most important exemption: a regex *pattern* that
    /// states one of these shapes escapes its dots (`dp\.st\.`), so it
    /// never contains the plain-dot text this guard's own regexes
    /// require. `CREDENTIAL_PATTERN` and `DopplerServiceToken`'s
    /// `#[domain(pattern = ...)]` are both this shape.
    #[test]
    fn a_pattern_constant_is_never_flagged() {
        let credential_pattern = r"^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$";
        let service_token_pattern = r"dp\.st\.(?:[a-z0-9\-_]{2,35}\.)?[a-zA-Z0-9]{40,44}";
        // `willikins-providers-github`'s own `CREDENTIAL_PATTERN`. It has
        // no dot to escape, so it survives for a different reason: an
        // alternation bar follows the prefix where a token's random run
        // would be, and `|` is not in `[A-Za-z0-9_]`.
        let github_credential_pattern = "^(github_pat_|ghp_)[A-Za-z0-9_]+$";
        assert!(
            !matcher().is_match(credential_pattern),
            "{credential_pattern}"
        );
        assert!(
            !matcher().is_match(github_credential_pattern),
            "{github_credential_pattern}"
        );
        assert!(
            !matcher().is_match(service_token_pattern),
            "{service_token_pattern}"
        );
    }

    /// A `concat!`-split literal -- this repository's own remedy -- must
    /// not be flagged when read from the file's actual source text: the
    /// separate arguments are not adjacent in the file (there is a `",
    /// "` between them), only in the value the macro produces at compile
    /// time. This test reads the words `concat!` itself takes as
    /// arguments, not the assembled result, to prove the *source text*
    /// shape (two short literals, a comma, a space) is what a line-based
    /// scan actually sees.
    #[test]
    fn a_concat_split_literal_is_not_flagged_in_source_form() {
        let source_form = r#"concat!("dp.st.prd.", "exampleexampleexampleexampleexampleexample")"#;
        assert!(!matcher().is_match(source_form), "{source_form}");
    }

    /// Every file in the repository is UTF-8 (no binary asset exists),
    /// so [`no_provider_token_shaped_literal_anywhere_in_the_tree`]'s
    /// silent skip of a file that fails to parse as UTF-8 is exercising
    /// dead code today, not quietly hiding a real one. If this ever
    /// fails, a binary file has been added and the skip is now load-
    /// bearing -- decide then whether it needs its own scan.
    #[test]
    fn the_walk_covers_every_known_text_extension() {
        let root = repo_root();
        let mut files = Vec::new();
        swept_files(&root, "", &mut files);
        let unreadable: Vec<&String> = files
            .iter()
            .filter(|relative| std::fs::read_to_string(root.join(relative)).is_err())
            .collect();
        assert!(
            unreadable.is_empty(),
            "these files are not UTF-8 text; give this guard a plan for them: {unreadable:?}"
        );
    }
}
