//! The only permitted certificate operation anywhere in this crate is
//! `GET`.
//!
//! Trust boundary 1 of `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`:
//! never create, modify, (de)activate, revoke or delete a certificate --
//! not in code, a test, or a probe. Revoking a distribution certificate is
//! team-wide, and Apple's own words are "Builds already uploaded to App
//! Store Connect but not yet submitted for App Review may be marked as
//! Invalid Binary". Certificate writes are **structurally absent** from
//! this crate, not merely unused: this guard is what keeps that true after
//! the people who remember why have moved on, in the manner of
//! `crates/willikins-cli/tests/no_gh_writes_guard.rs` and
//! `crates/willikins-core/tests/secret_literal_guard.rs`.
//!
//! # Scope
//!
//! Only this crate's own `src/` and `tests/` (not the whole workspace --
//! `no_gh_writes_guard`'s tree-wide walk exists because *any* file could
//! spend the operator's `gh` credential; a certificate write can only ever
//! happen through this crate's own [`crate::client::AppstoreClient`] or a
//! test built against a mock server standing in for it).
//!
//! # Method: statement-level, not line-level
//!
//! Comments are stripped first (string literals are left alone, so a
//! doc-comment URL's `//` does not falsely truncate a real string, and a
//! real string's content is never mistaken for a comment). What remains
//! is split into "statements" on `;` and `}`, and whitespace within each
//! is collapsed to single spaces. A statement is flagged when it contains
//! **both**:
//!
//! - a write-method token: `.post(`, `.patch(`, `.delete(` (each also
//!   matched with an explicit turbofish in between, `.post::<T>(` --
//!   every real call site in this crate spells it that way, and a plain
//!   substring check for `.post(` alone missed it, which is exactly the
//!   honest mistake this guard exists to catch), `"POST"`, `"PATCH"`, or
//!   `"DELETE"`;
//! - a certificates **path**: the literal `/certificates`, with a
//!   non-alphanumeric character (or nothing) immediately after it -- so
//!   `/v1/certificates`, `/v1/certificates/{id}` and
//!   `/v1/certificates?filter[...]` all count, but a hypothetical
//!   `/v1/certificatesFoo` does not, and neither does the *JSON
//!   relationship key* `"certificates"` a profile create legitimately
//!   carries (`relationships.certificates.data`), which has no leading
//!   slash at all.
//!
//! Splitting on `;`/`}` rather than scanning line by line is what catches
//! a call split across lines
//! (`self.http.post(\n &format!("/v1/certificates/{id}"))`): both tokens
//! land in the same statement even though neither appears on the same
//! *line*. Like its two siblings, this is statement-level text scanning,
//! not a real parser: it catches the honest mistake, not a determined one
//! (a path assembled from fragments at runtime that never spells
//! `/certificates` contiguously in source).

use std::path::{Path, PathBuf};

/// This crate's own root -- the test binary's manifest directory already
/// *is* `crates/willikins-providers-appstore`, so no walk up to the
/// workspace root is needed (contrast `no_gh_writes_guard`'s tree-wide
/// scope).
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// This file itself: it necessarily names every token and path shape this
/// guard looks for, in its own doc comment and unit tests.
fn is_exempt(relative: &str) -> bool {
    relative == "tests/no_certificate_writes_guard.rs"
}

/// Every `.rs` file under `src/` and `tests/`, crate-relative,
/// `/`-separated, excluding [`is_exempt`].
fn rust_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("readable directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        if entry.file_type().expect("file type").is_dir() {
            rust_files(&entry.path(), &format!("{relative}/"), out);
            continue;
        }
        if name.ends_with(".rs") && !is_exempt(&relative) {
            out.push(relative);
        }
    }
}

/// Strip `//` line comments and `/* ... */` block comments from `text`,
/// leaving double-quoted string literals untouched (so a string carrying
/// `//` -- `APPSTORE_API_BASE_URL`'s `"https://..."`, say -- is never
/// truncated, and so a comment's own text, which may say anything, is
/// never mistaken for code). Not aware of char literals or raw strings:
/// this crate uses neither in a way that could hide a write call, and a
/// lifetime like `'de` is left alone because this function only enters
/// its string state on `"`, never on `'`.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next(); // consume the `*`
            let mut prev = '\0';
            for c in chars.by_ref() {
                if prev == '*' && c == '/' {
                    break;
                }
                prev = c;
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Split comment-stripped `text` into statements on `;` and `}`, with
/// interior whitespace collapsed to single spaces. Every resulting chunk
/// (the delimiter itself dropped) is one statement to search.
fn statements(text: &str) -> Vec<String> {
    let collapsed: String = {
        let mut out = String::with_capacity(text.len());
        let mut last_was_space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                if !last_was_space {
                    out.push(' ');
                }
                last_was_space = true;
            } else {
                out.push(c);
                last_was_space = false;
            }
        }
        out
    };
    collapsed
        .split(|c| c == ';' || c == '}')
        .map(str::to_owned)
        .collect()
}

/// The write-method tokens a statement must carry, alongside a
/// certificates path, to be flagged.
///
/// `.post`/`.patch`/`.delete` are regexes, not plain substrings: every
/// real call site in this crate spells them with an explicit turbofish
/// (`self.http.post::<BundleIdResponse>(...)`, mirroring
/// `AppstoreClient::create_bundle_id`'s own shape), and a first version of
/// this guard that looked for the literal `.post(` missed exactly that
/// shape -- proved by [`tests::a_turbofish_post_to_certificates_is_flagged`],
/// planted the same way the honest-mistake proof in this milestone's own
/// task list was. `[^>(]*` (never `>` or `(`) keeps the generic argument
/// bounded to a plain type path -- this crate's own turbofishes are all
/// that shape -- without ever crossing into the argument list itself.
fn write_token_regex() -> regex::Regex {
    regex::Regex::new(r#"\.(?:post|patch|delete)(?:::<[^>(]*>)?\s*\(|"POST"|"PATCH"|"DELETE""#)
        .expect("WRITE_TOKEN pattern is valid")
}

/// Whether `statement` names a certificates **path** -- `/certificates`
/// followed by end-of-statement or a non-alphanumeric character, never a
/// bare JSON key with no leading slash.
fn names_certificates_path(statement: &str) -> bool {
    const NEEDLE: &str = "/certificates";
    let bytes = statement.as_bytes();
    let mut start = 0;
    while let Some(found) = statement[start..].find(NEEDLE) {
        let at = start + found;
        let after = at + NEEDLE.len();
        let boundary_ok = bytes
            .get(after)
            .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        if boundary_ok {
            return true;
        }
        start = after;
    }
    false
}

/// Every statement in `text` (already read from a file) that names both a
/// write token and a certificates path -- used by both the tree-wide test
/// and this file's own unit tests.
fn violations_in(text: &str) -> Vec<String> {
    let write_token = write_token_regex();
    let stripped = strip_comments(text);
    statements(&stripped)
        .into_iter()
        .filter(|statement| write_token.is_match(statement) && names_certificates_path(statement))
        .collect()
}

#[test]
fn only_get_ever_touches_the_certificates_path_in_this_crate() {
    let root = crate_root();
    let mut files = Vec::new();
    rust_files(&root.join("src"), "src/", &mut files);
    rust_files(&root.join("tests"), "tests/", &mut files);
    assert!(
        files.len() > 3,
        "the walk found only {} files; it is not reading the tree",
        files.len()
    );

    let mut offenders = Vec::new();
    for relative in &files {
        let text = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|e| panic!("{relative}: {e}"));
        for statement in violations_in(&text) {
            offenders.push(format!("{relative}: {statement}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "these statements write to the certificates path; the only permitted certificate \
         operation anywhere in this crate is GET (trust boundary 1,\n\
         docs/plans/2026-09-22-milestone-3c-app-store-signing.md):\n{}",
        offenders.join("\n---\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multiline_post_to_certificates_is_flagged() {
        let snippet = "self.http.post(\n    &format!(\"/v1/certificates\"),\n    &body,\n);";
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// The honest mistake this guard's own first draft missed: every real
    /// `.post`/`.patch` call in this crate carries a turbofish
    /// (`self.http.post::<BundleIdResponse>(...)`), and a version of
    /// [`write_token_regex`] that looked for the literal `.post(` did not
    /// match `.post::<serde_json::Value>(`. Planted directly in
    /// `src/client.rs` (never committed) and watched fail before this
    /// fix -- see the milestone plan's own "prove it is not vacuous" step.
    #[test]
    fn a_turbofish_post_to_certificates_is_flagged() {
        let snippet = r#"self.http.post::<serde_json::Value>(&format!("/v1/certificates/{id}/revoke"), &())?;"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_turbofish_delete_to_certificates_is_flagged() {
        let snippet = r#"self.http.delete::<()>(&format!("/v1/certificates/{id}"))?;"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_delete_mock_against_a_certificate_id_is_flagged() {
        let snippet = r#"provider.mock("DELETE", "/v1/certificates/X").create();"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_patch_built_through_format_is_flagged() {
        let snippet = r#"self.http.patch(&format!("/v1/certificates/{id}"), &body)?;"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_bare_post_string_literal_paired_with_the_path_is_flagged() {
        let snippet = r#"let (method, path) = ("POST", "/v1/certificates");"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_profile_create_naming_the_certificates_relationship_is_not_flagged() {
        let snippet = r#"
            let body = ProfileCreateBody {
                data: ProfileCreateData {
                    type_: "profiles",
                    relationships: ProfileRelationships {
                        certificates: RelationshipList { data: vec![certificate_ref] },
                    },
                },
            };
            self.http.post::<ProfileResponse>("/v1/profiles", &body)?;
        "#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_get_read_of_certificates_is_not_flagged() {
        let snippet = r#"
            let path = format!("/v1/certificates?filter[certificateType]={cert_type}&limit={PAGE_LIMIT}");
            let response: CertificateListResponse = self.http.get(&path)?;
        "#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_get_mock_against_certificates_is_not_flagged() {
        let snippet = r#"provider.mock("GET", "/v1/certificates").with_status(200).create();"#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_write_call_to_an_unrelated_path_is_not_flagged() {
        let snippet = r#"self.http.post::<BundleIdResponse>("/v1/bundleIds", &body)?;"#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_hypothetical_longer_path_segment_is_not_a_false_positive() {
        let snippet = r#"self.http.post("/v1/certificatesFoo", &body)?;"#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_comment_mentioning_a_certificate_delete_is_not_flagged() {
        let snippet =
            "// self.http.delete(&format!(\"/v1/certificates/{id}\"));\nself.http.get(&path)?;";
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_block_comment_mentioning_a_certificate_delete_is_not_flagged() {
        let snippet =
            "/* self.http.delete(&format!(\"/v1/certificates/{id}\")); */\nself.http.get(&path)?;";
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    #[test]
    fn a_url_literal_carrying_a_double_slash_is_not_truncated_by_the_comment_stripper() {
        // `APPSTORE_API_BASE_URL`'s own shape: a string literal containing
        // `//`, which a naive line-comment stripper would truncate.
        let stripped = strip_comments(
            r#"pub const APPSTORE_API_BASE_URL: &str = "https://api.appstoreconnect.apple.com";"#,
        );
        assert!(stripped.contains("https://api.appstoreconnect.apple.com"));
    }

    #[test]
    fn the_walk_actually_reaches_client_and_lib() {
        let root = crate_root();
        let mut files = Vec::new();
        rust_files(&root.join("src"), "src/", &mut files);
        rust_files(&root.join("tests"), "tests/", &mut files);
        for expected in ["src/client.rs", "src/lib.rs"] {
            assert!(files.iter().any(|f| f == expected), "{expected} missing");
        }
    }

    #[test]
    fn this_guard_file_is_exempt_from_itself() {
        assert!(is_exempt("tests/no_certificate_writes_guard.rs"));
        assert!(!is_exempt("tests/bundle_id_ensure_mock.rs"));
    }
}
