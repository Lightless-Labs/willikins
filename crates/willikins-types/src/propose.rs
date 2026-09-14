//! Turn a free-form [`ProjectName`] into a proposed [`ProjectSlug`].
//!
//! Lossy and run once at project creation. Its result is shown to a human
//! or agent, confirmed, and persisted; it is never re-run against an
//! existing project and may change between versions.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::DomainType;
use crate::name::ProjectName;
use crate::reserved::is_reserved;
use crate::slug::ProjectSlug;

/// Why [`propose_slug`] could not produce a [`ProjectSlug`].
///
/// Every variant carries the original display name: [`ProjectName`] is
/// never secret, so there is nothing to redact.
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`), the same
/// convention every other error enum in this workspace uses (see
/// `willikins-core`'s `CheckError`/`PlanError`), so a caller that wraps
/// this behind a `{kind, message}` shape (`willikins-server`'s
/// `ButlerError::SlugProposal`, task 10a) needs no bespoke mapping.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind")]
pub enum ProposeError {
    /// The name contained no ASCII alphanumeric characters at all.
    #[error("`{input}` contains no usable characters")]
    NoWords {
        /// The display name that produced no words.
        input: String,
    },
    /// The first word begins with a digit, which [`ProjectSlug`] never
    /// allows. Nothing is prefixed to fix this; the failure is reported.
    #[error("`{input}`: the first word (`{first_word}`) must start with a letter, not a digit")]
    LeadingDigit {
        /// The display name that produced a leading-digit first word.
        input: String,
        /// The offending first word.
        first_word: String,
    },
    /// The joined kebab form exceeds [`ProjectSlug::MAX_LEN`].
    #[error("`{input}`: the proposed slug `{joined}` is {len} characters, the limit is {max}")]
    TooLong {
        /// The display name that produced an over-long slug.
        input: String,
        /// The joined kebab-case candidate.
        joined: String,
        /// Its length in characters.
        len: usize,
        /// The limit that was exceeded.
        max: usize,
    },
    /// The name reduces to a single word that is a reserved identifier.
    #[error("`{input}`: `{word}` is a reserved word")]
    Reserved {
        /// The display name that reduced to a reserved word.
        input: String,
        /// The offending word.
        word: String,
    },
    /// The joined result did not parse as a [`ProjectSlug`] for some
    /// other reason.
    #[error("`{input}`: could not derive a project slug: {reason}")]
    Invalid {
        /// The display name that failed to derive.
        input: String,
        /// The underlying parse failure.
        reason: String,
    },
}

/// Propose a [`ProjectSlug`] from a display name.
///
/// NFKD-normalises, drops combining marks, keeps ASCII alphanumerics,
/// treats every other character as a separator, splits `camelCase` and
/// `PascalCase` at case boundaries, lowercases, and joins with hyphens.
///
/// The case-boundary split is applied uniformly, with no special case for
/// a one-letter prefix: `iPhone App` proposes `i-phone-app`, not
/// `iphone-app`, and `iOS Companion` proposes `i-os-companion`. A run of
/// capitals followed by a lowercase letter breaks before the last capital,
/// so `HTTPServer` proposes `http-server`. The proposal is confirmed by a
/// human or agent before it is persisted, so an unwanted split is corrected
/// there rather than guessed at here.
pub fn propose_slug(name: &ProjectName) -> Result<ProjectSlug, ProposeError> {
    let input = name.as_str().to_owned();

    let decomposed: Vec<char> = name
        .as_str()
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect();

    let tokens = tokenize(&decomposed);

    let Some(first) = tokens.first() else {
        return Err(ProposeError::NoWords { input });
    };

    if first.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(ProposeError::LeadingDigit {
            input,
            first_word: first.clone(),
        });
    }

    let joined = tokens.join("-");
    if joined.len() > ProjectSlug::MAX_LEN {
        return Err(ProposeError::TooLong {
            input,
            len: joined.len(),
            max: ProjectSlug::MAX_LEN,
            joined,
        });
    }

    if let [only] = tokens.as_slice()
        && is_reserved(only)
    {
        return Err(ProposeError::Reserved {
            input,
            word: only.clone(),
        });
    }

    ProjectSlug::parse(&joined).map_err(|e| ProposeError::Invalid {
        input,
        reason: e.reason,
    })
}

/// Split a decomposed, mark-stripped character sequence into lowercase
/// ASCII-alphanumeric tokens, treating every non-alphanumeric character as
/// a separator and splitting camelCase/PascalCase at case boundaries.
fn tokenize(chars: &[char]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_alphanumeric() {
            if !current.is_empty() && is_word_boundary(chars, i) {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(c.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether position `i` starts a new camelCase/PascalCase word: a
/// lowercase-to-uppercase transition, or the last uppercase letter of an
/// uppercase run immediately followed by a lowercase letter (so
/// `HTTPServer` splits as `HTTP` / `Server`).
fn is_word_boundary(chars: &[char], i: usize) -> bool {
    if i == 0 || !chars[i].is_ascii_uppercase() {
        return false;
    }
    let prev = chars[i - 1];
    if prev.is_ascii_lowercase() {
        return true;
    }
    prev.is_ascii_uppercase() && chars.get(i + 1).is_some_and(char::is_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn propose(input: &str) -> Result<String, ProposeError> {
        let name = ProjectName::parse(input).expect("valid ProjectName in test fixture");
        propose_slug(&name).map(|slug| slug.to_string())
    }

    #[test]
    fn third_thoughts() {
        assert_eq!(propose("Third Thoughts").unwrap(), "third-thoughts");
    }

    #[test]
    fn foundry_2() {
        assert_eq!(propose("Foundry 2").unwrap(), "foundry-2");
    }

    #[test]
    fn etoile_drops_the_diacritic() {
        assert_eq!(propose("Étoile").unwrap(), "etoile");
    }

    #[test]
    fn camel_case_name() {
        assert_eq!(propose("camelCaseName").unwrap(), "camel-case-name");
    }

    #[test]
    fn http_server_splits_the_acronym_run() {
        assert_eq!(propose("HTTPServer").unwrap(), "http-server");
    }

    #[test]
    fn surrounding_spaces_are_trimmed_away() {
        assert_eq!(propose("  spaces  ").unwrap(), "spaces");
    }

    #[test]
    fn apostrophes_and_spaces_all_separate_words() {
        assert_eq!(
            propose("Lightless Labs' Foundry").unwrap(),
            "lightless-labs-foundry"
        );
    }

    #[test]
    fn no_usable_characters_is_an_error() {
        assert!(matches!(propose("!!!"), Err(ProposeError::NoWords { .. })));
    }

    #[test]
    fn a_reserved_single_word_is_an_error() {
        assert!(matches!(
            propose("Self"),
            Err(ProposeError::Reserved { .. })
        ));
    }

    #[test]
    fn a_leading_digit_is_an_error() {
        assert!(matches!(
            propose("2fast"),
            Err(ProposeError::LeadingDigit { .. })
        ));
    }

    #[test]
    fn too_long_is_an_error() {
        let name = ProjectName::parse(&"word ".repeat(20)).unwrap();
        assert!(matches!(
            propose_slug(&name),
            Err(ProposeError::TooLong { .. })
        ));
    }

    #[test]
    fn digits_inside_a_word_do_not_split_it() {
        assert_eq!(propose("b2b").unwrap(), "b2b");
    }

    #[test]
    fn serializes_with_a_kind_tag() {
        let name = ProjectName::parse("!!!").unwrap();
        let error = propose_slug(&name).unwrap_err();
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "NoWords");
        assert_eq!(json["input"], "!!!");
    }
}
