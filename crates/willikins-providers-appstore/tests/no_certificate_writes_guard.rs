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
//! # Method: statements, followed through bindings within a function
//!
//! Comments are stripped first (string literals are left alone, so a
//! doc-comment URL's `//` does not falsely truncate a real string, and a
//! real string's content is never mistaken for a comment). What remains
//! is split into "statements" on `;` and `}`, and whitespace within each
//! is collapsed to single spaces. A statement is flagged by any one of
//! three rules (see [`violations_in`]):
//!
//! 1. it carries a **write token** and a **certificates path**;
//! 2. it carries a write token and names a **binding that holds a
//!    certificates path** -- a `let` in the same function, followed
//!    transitively (`let base = "/v1/certificates"; let path =
//!    format!("{base}/{id}"); self.http.delete(&path)`), or a `const` /
//!    `static` item anywhere in the file;
//! 3. it **calls something whose own name says it writes a certificate**
//!    (`revoke_certificate(..)`, `create_certificate(..)`), path or no
//!    path.
//!
//! A write token is a method named as a string (`"POST"`, `"PATCH"`,
//! `"PUT"`, `"DELETE"`) or as a constant (`Method::DELETE`), or a call to
//! any identifier with a write verb as one of its `_`-separated parts --
//! `.post(`, `.delete::<()>(`, `Http::delete(&self.http, ..)`,
//! `raw_post(..)`, whatever the turbofish or call syntax. A certificates
//! path is the literal `/certificates` with a non-alphanumeric character
//! (or nothing) after it, so `/v1/certificates`, `/v1/certificates/{id}`
//! and `/v1/certificates?filter[...]` all count, but `/v1/certificatesFoo`
//! does not, and neither does the JSON relationship key `"certificates"` a
//! profile create legitimately carries, which has no leading slash.
//!
//! # What it does not catch, stated
//!
//! This is text scanning, not a parser. It catches the honest mistake --
//! every shape above is one this crate already spells its own `GET`s in,
//! or one the milestone 3c task-3 adversarial pass got past the first
//! draft (`docs/research/2026-09-22-m3c-adversarial-pass.md`) -- not a
//! determined one: a path assembled from fragments that never spells
//! `/certificates` contiguously (`concat!("/v1/cert", "ificates")`,
//! `format!("/v1/{}", "certificates")`), a path that crosses a function
//! boundary as an argument into a helper whose name carries no write verb,
//! or a path read from outside the source. The trust boundary is what
//! forbids those; this guard only makes the honest version of them fail a
//! gate.

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
        let is_rust = Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"));
        if is_rust && !is_exempt(&relative) {
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

/// Split comment-stripped `text` into statements on `;` and `}` **outside
/// string literals**, with interior whitespace collapsed to single spaces.
/// Every resulting chunk (the delimiter itself dropped) is one statement
/// to search.
///
/// String-aware because a format string's own braces are not statement
/// boundaries: the first draft split `format!("{BASE}/v1/certificates/{id}")`
/// at `{BASE}`'s `}`, which put the write call and the certificates path in
/// two different "statements" (the task-3 adversarial pass,
/// [`tests::a_method_constant_paired_with_the_path_is_flagged`]).
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
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut chars = collapsed.chars();
    while let Some(c) = chars.next() {
        if in_string {
            current.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                current.push(c);
            }
            ';' | '}' => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out
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

/// The literal write tokens a statement may carry: a method named as a
/// string (`"POST"`, a mockito `mock("DELETE", ..)`) or as a constant
/// (`Method::DELETE`). Called write methods (`.post(`, `.delete::<T>(`,
/// `Http::delete(`, `raw_post(`) are found by [`called_identifiers`]
/// instead, which is what made the turbofish shape and fully qualified
/// call syntax stop mattering -- the first draft matched `.post(` as a
/// substring and missed `.post::<T>(` (see
/// [`tests::a_turbofish_post_to_certificates_is_flagged`]).
fn literal_write_token_regex() -> regex::Regex {
    regex::Regex::new(r#""(?:POST|PATCH|PUT|DELETE)"|\bMethod::(?:POST|PATCH|PUT|DELETE)\b"#)
        .expect("literal write-token pattern is valid")
}

/// Every identifier in `statement` that is *called*: immediately followed
/// (after an optional turbofish) by `(`. A `fn` definition's own name is
/// not a call and is skipped. Lower-cased.
fn called_identifiers(statement: &str) -> Vec<String> {
    let call = regex::Regex::new(r"(\bfn\s+)?\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::<[^;{}]*?>)?\s*\(")
        .expect("call pattern is valid");
    call.captures_iter(statement)
        .filter(|captures| captures.get(1).is_none())
        .map(|captures| captures[2].to_ascii_lowercase())
        .collect()
}

/// A called identifier counts as a write when one of its `_`-separated
/// parts is one of these: `post`, `raw_post`, `delete_profile`,
/// `Http::delete`. Deliberately not `create` (mockito's `.create()`
/// registers a GET mock just as readily) nor `remove` (`Vec::remove`):
/// those two count only beside a certificate noun, in
/// [`CERTIFICATE_WRITE_VERBS`].
const WRITE_VERBS: &[&str] = &[
    "post", "patch", "put", "delete", "revoke", "update", "modify",
];

/// A called identifier that carries one of these parts *and* a
/// `certificate`/`certificates` part is a certificate write by its own
/// name -- `revoke_certificate(&id)` needs no path in sight to be one.
const CERTIFICATE_WRITE_VERBS: &[&str] = &[
    "post",
    "patch",
    "put",
    "delete",
    "revoke",
    "update",
    "modify",
    "create",
    "remove",
    "add",
    "upload",
    "activate",
    "deactivate",
    "set",
    "rotate",
    "renew",
    "replace",
];

fn has_part(identifier: &str, parts: &[&str]) -> bool {
    identifier.split('_').any(|part| parts.contains(&part))
}

/// Whether `statement` performs a write of any kind: a literal write
/// token, or a call whose name carries a [`WRITE_VERBS`] part.
fn has_write_token(statement: &str, literal: &regex::Regex) -> bool {
    literal.is_match(statement)
        || called_identifiers(statement)
            .iter()
            .any(|identifier| has_part(identifier, WRITE_VERBS))
}

/// Whether `statement` calls something whose own name says it writes a
/// certificate.
fn calls_a_certificate_writer(statement: &str) -> bool {
    called_identifiers(statement).iter().any(|identifier| {
        has_part(identifier, &["certificate", "certificates"])
            && has_part(identifier, CERTIFICATE_WRITE_VERBS)
    })
}

/// Every identifier token in `statement` (string contents included --
/// over-matching here only ever flags more).
fn identifier_tokens(statement: &str) -> std::collections::BTreeSet<String> {
    let token = regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("token pattern is valid");
    token
        .find_iter(statement)
        .map(|found| found.as_str().to_owned())
        .collect()
}

/// The names a `let` binds in `statement` (`let path`, `let mut path`,
/// `let (method, path)`, `if let Some(rows)`), skipping `mut`, `ref` and
/// capitalised pattern constructors (`Some`, `Ok`).
fn let_bound_names(statement: &str) -> Vec<String> {
    let binding = regex::Regex::new(r"\blet\s+([^=:]+)").expect("let pattern is valid");
    let token = regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("token pattern is valid");
    binding
        .captures_iter(statement)
        .flat_map(|captures| {
            token
                .find_iter(&captures[1])
                .map(|found| found.as_str().to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|name| {
            name != "mut" && name != "ref" && !name.starts_with(|c: char| c.is_ascii_uppercase())
        })
        .collect()
}

/// The name a `const` or `static` item binds in `statement`, if any.
fn item_bound_name(statement: &str) -> Option<String> {
    let item = regex::Regex::new(r"\b(?:const|static)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:")
        .expect("item pattern is valid");
    item.captures(statement)
        .map(|captures| captures[1].to_owned())
}

/// Whether `statement` names a certificates path, or mentions a binding
/// already known to hold one.
fn carries_certificates_path(
    statement: &str,
    tainted: &std::collections::BTreeSet<String>,
) -> bool {
    names_certificates_path(statement)
        || identifier_tokens(statement)
            .iter()
            .any(|token| tainted.contains(token))
}

/// Split comment-stripped `text` at every `fn` item, so a `let` binding
/// is followed only within the function that made it (this crate's own
/// `list_certificates` and `update_bundle_id_name` both bind a `path`).
fn function_chunks(text: &str) -> Vec<&str> {
    let item = regex::Regex::new(r"\bfn\s+[A-Za-z_]").expect("fn pattern is valid");
    let mut starts: Vec<usize> = item.find_iter(text).map(|found| found.start()).collect();
    starts.insert(0, 0);
    starts.push(text.len());
    starts
        .windows(2)
        .map(|pair| &text[pair[0]..pair[1]])
        .collect()
}

/// Every statement in `text` (already read from a file) that writes to a
/// certificates path -- used by both the tree-wide test and this file's
/// own unit tests. Three rules, any one of which flags a statement:
///
/// 1. a write token and a certificates path in the same statement (the
///    first draft's rule);
/// 2. a write token and a binding that holds a certificates path --
///    followed through `let` within one function, transitively
///    (`let base = "/v1/certificates"; let path = format!("{base}/{id}");
///    self.http.delete(&path)`), and through a `const`/`static` item
///    across the whole file;
/// 3. a call whose own name says it writes a certificate
///    (`revoke_certificate(..)`), path or no path.
fn violations_in(text: &str) -> Vec<String> {
    let literal = literal_write_token_regex();
    let stripped = strip_comments(text);

    let mut file_tainted = std::collections::BTreeSet::new();
    loop {
        let before = file_tainted.len();
        for statement in statements(&stripped) {
            if let Some(name) = item_bound_name(&statement)
                && carries_certificates_path(&statement, &file_tainted)
            {
                file_tainted.insert(name);
            }
        }
        if file_tainted.len() == before {
            break;
        }
    }

    let mut offenders = Vec::new();
    for chunk in function_chunks(&stripped) {
        let chunk_statements = statements(chunk);
        let mut tainted = file_tainted.clone();
        loop {
            let before = tainted.len();
            for statement in &chunk_statements {
                if carries_certificates_path(statement, &tainted) {
                    tainted.extend(let_bound_names(statement));
                }
            }
            if tainted.len() == before {
                break;
            }
        }
        for statement in chunk_statements {
            let writes_a_certificates_path = has_write_token(&statement, &literal)
                && carries_certificates_path(&statement, &tainted);
            if writes_a_certificates_path || calls_a_certificate_writer(&statement) {
                offenders.push(statement);
            }
        }
    }
    offenders
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
    /// the write-token rule that looked for the literal `.post(` did not
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

    // -----------------------------------------------------------------
    // Bypasses found by the task-3 adversarial pass (2026-09-22,
    // `docs/research/2026-09-22-m3c-adversarial-pass.md`). Each one is an
    // honest shape -- the way this crate already spells a GET -- that the
    // statement-local rule let through.
    // -----------------------------------------------------------------

    /// `list_certificates` builds its path in a `let` and reads it in the
    /// next statement; a write spelled the same way split the path and the
    /// write token across two statements, and neither statement carried
    /// both.
    #[test]
    fn a_write_through_a_let_bound_certificates_path_is_flagged() {
        let snippet = r#"
            pub(crate) fn revoke(&self, id: &str) -> Result<(), ProviderError> {
                let path = format!("/v1/certificates/{id}");
                self.http.delete(&path)
            }
        "#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// Transitive: the path reaches the write through a second binding.
    #[test]
    fn a_write_through_a_twice_bound_certificates_path_is_flagged() {
        let snippet = r#"
            fn revoke(&self, id: &str) {
                let base = "/v1/certificates";
                let path = format!("{base}/{id}");
                self.http.delete(&path)
            }
        "#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// A module-level constant, used from any function in the file.
    #[test]
    fn a_write_through_a_const_certificates_path_is_flagged() {
        let snippet = r#"
            const CERTIFICATES: &str = "/v1/certificates";
            fn create(&self, body: &Body) {
                self.http.post::<CertificateResponse>(CERTIFICATES, body)
            }
        "#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// Fully-qualified call syntax: `Http::delete(&self.http, ...)` has no
    /// `.delete(`.
    #[test]
    fn a_fully_qualified_delete_is_flagged() {
        let snippet = r#"Http::delete(&self.http, &format!("/v1/certificates/{id}"))?;"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// A method constant rather than a string: `Method::DELETE`.
    #[test]
    fn a_method_constant_paired_with_the_path_is_flagged() {
        let snippet = r#"agent.request(Method::DELETE, &format!("{BASE}/v1/certificates/{id}"));"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// A PUT, which the first draft did not list at all.
    #[test]
    fn a_put_to_certificates_is_flagged() {
        let snippet = r#"self.http.put(&format!("/v1/certificates/{id}"), &body)?;"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// A generic writer that takes its path as an argument -- the shape
    /// `tests/live_write_cycle.rs`'s own `raw_post(.., path, ..)` had before
    /// this pass fixed its path. The call site is the only place the
    /// certificates path and the write meet.
    #[test]
    fn a_path_taking_write_helper_called_with_the_certificates_path_is_flagged() {
        let snippet =
            r#"let result = raw_post(&issuer_id, &key_id, &key, "/v1/certificates", &body);"#;
        assert!(!violations_in(snippet).is_empty(), "{snippet}");
    }

    /// A helper whose own name says what it does, called with no path in
    /// sight: the path lives inside it (possibly assembled from pieces),
    /// but the call is still a certificate write.
    #[test]
    fn a_call_to_a_certificate_writing_helper_is_flagged() {
        for snippet in [
            "client.revoke_certificate(&id)?;",
            "client.create_certificate(&csr)?;",
            "client.delete_certificates(&ids)?;",
            "client.update_certificate_activation(&id, false)?;",
        ] {
            assert!(!violations_in(snippet).is_empty(), "{snippet}");
        }
    }

    /// Precision: a `let`-bound certificates path in one function does not
    /// taint an identically named binding in the next function (this
    /// crate's own `list_certificates` and `update_bundle_id_name` both bind
    /// a `path`).
    #[test]
    fn a_certificates_path_binding_does_not_taint_another_function() {
        let snippet = r#"
            pub(crate) fn list_certificates(&self) -> Result<Vec<C>, E> {
                let mut path = format!("/v1/certificates?limit={PAGE_LIMIT}");
                let response: CertificateListResponse = self.http.get(&path)?;
                Ok(response.data)
            }
            pub(crate) fn update_bundle_id_name(&self, id: &str) -> Result<(), E> {
                let path = format!("/v1/bundleIds/{id}");
                self.http.patch::<BundleIdResponse>(&path, &body)?;
                Ok(())
            }
        "#;
        assert!(violations_in(snippet).is_empty(), "{snippet}");
    }

    /// Precision: mockito's `.create()` on a GET mock, `Vec::remove`, and
    /// a profile create passing a certificate id are none of them a
    /// certificate write.
    #[test]
    fn ordinary_calls_near_certificates_are_not_flagged() {
        for snippet in [
            r#"provider.mock("GET", "/v1/certificates").match_query(Matcher::Any).create();"#,
            "let one = matches.remove(0);",
            "client.create_profile(&name, &profile_type, &bundle_id, &certificate)?;",
            r#"let n = count_rows(issuer_id, key_id, key, "/v1/certificates?limit=200");"#,
        ] {
            assert!(violations_in(snippet).is_empty(), "{snippet}");
        }
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
