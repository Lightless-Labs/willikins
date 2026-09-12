//! Case-insensitive reserved-word check: keywords from Rust, Java, Kotlin,
//! and Swift, plus reserved Windows device names. A single-word slug that
//! matches any of these is rejected; a multi-word slug can never collide
//! because every join keeps the separator or the case boundary.
//!
//! Verified 2026-09-12 against the Rust reference
//! (<https://doc.rust-lang.org/reference/keywords.html>), JLS 21 section
//! 3.9
//! (<https://docs.oracle.com/javase/specs/jls/se21/html/jls-3.html#jls-3.9>),
//! kotlinlang.org's keyword reference
//! (<https://kotlinlang.org/docs/keyword-reference.html>), Microsoft's file
//! naming page
//! (<https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file>),
//! and, for Swift, the `swiftlang/swift-book` `DocC` source that
//! docs.swift.org's lexical-structure page renders
//! (<https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/ReferenceManual/LexicalStructure.md>) —
//! the rendered page itself is a JavaScript shell to every fetcher, so this
//! `DocC` file is its primary source, not a secondary summary. See
//! `docs/research/2026-09-12-m2-dependencies.md` section 5 for the full
//! pass. That pass found Rust, Java, Kotlin hard keywords, and the Windows
//! device names already exact; it added `borrowing`, `consuming`, and
//! `nonisolated` to [`SWIFT_KEYWORDS`], gained by Swift's
//! ownership/concurrency features since this module was written.

/// Rust strict and reserved keywords, across every edition this workspace
/// can build with, including `gen`, reserved by the 2024 edition. `Self`
/// is folded into `self`; weak keywords such as `union` are not included
/// because they remain valid identifiers.
/// Source: <https://doc.rust-lang.org/reference/keywords.html>
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Java reserved keywords, including `const` and `goto`, which are
/// reserved but unused by the language.
/// Source: JLS 3.9, <https://docs.oracle.com/javase/specs/jls/se21/html/jls-3.html#jls-3.9>
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
];

/// Kotlin hard keywords. The operator forms `as?`, `!in`, and `!is` are
/// excluded: they contain characters no slug word can, so they can never
/// collide with one.
/// Source: <https://kotlinlang.org/docs/keyword-reference.html>
const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

/// Swift keywords used in declarations, statements, expressions, and
/// types. Keywords reserved only in particular patterns or contexts are
/// excluded, per the task's scope.
/// Verified 2026-09-12 against the `swiftlang/swift-book` `DocC` source that
/// docs.swift.org's "Lexical Structure" page renders (the rendered page is
/// a JavaScript shell to every fetcher, so this `DocC` file is its primary
/// source, not a secondary summary):
/// <https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/ReferenceManual/LexicalStructure.md>.
/// That pass found `borrowing`, `consuming`, and `nonisolated` missing from
/// the declarations group and added them here.
/// Source: <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/lexicalstructure/>
const SWIFT_KEYWORDS: &[&str] = &[
    "any",
    "as",
    "associatedtype",
    "await",
    "borrowing",
    "break",
    "case",
    "catch",
    "class",
    "consuming",
    "continue",
    "default",
    "defer",
    "deinit",
    "do",
    "else",
    "enum",
    "extension",
    "fallthrough",
    "false",
    "fileprivate",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "is",
    "let",
    "nil",
    "nonisolated",
    "open",
    "operator",
    "precedencegroup",
    "private",
    "protocol",
    "public",
    "repeat",
    "rethrows",
    "return",
    "self",
    "static",
    "struct",
    "subscript",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
];

/// Reserved Windows device names: none of these may be used as a file or
/// directory name component on Windows, regardless of extension.
const WINDOWS_DEVICE_NAMES: &[&str] = &[
    "aux", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "con", "lpt1",
    "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "nul", "prn",
];

const LISTS: &[&[&str]] = &[
    RUST_KEYWORDS,
    JAVA_KEYWORDS,
    KOTLIN_KEYWORDS,
    SWIFT_KEYWORDS,
    WINDOWS_DEVICE_NAMES,
];

/// Whether `word` collides, case-insensitively, with a Rust, Java, Kotlin,
/// or Swift keyword, or with a reserved Windows device name.
#[must_use]
pub fn is_reserved(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    LISTS
        .iter()
        .any(|list| list.binary_search(&lower.as_str()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_sorted_and_lowercase(list: &[&str]) {
        assert!(!list.is_empty());
        for pair in list.windows(2) {
            assert!(
                pair[0] < pair[1],
                "not strictly sorted: `{}` should come after `{}`",
                pair[0],
                pair[1]
            );
        }
        for word in list {
            assert_eq!(
                *word,
                word.to_ascii_lowercase(),
                "`{word}` is not lowercase"
            );
        }
    }

    #[test]
    fn rust_keywords_are_sorted_and_lowercase() {
        assert_sorted_and_lowercase(RUST_KEYWORDS);
    }

    #[test]
    fn java_keywords_are_sorted_and_lowercase() {
        assert_sorted_and_lowercase(JAVA_KEYWORDS);
    }

    #[test]
    fn kotlin_keywords_are_sorted_and_lowercase() {
        assert_sorted_and_lowercase(KOTLIN_KEYWORDS);
    }

    #[test]
    fn swift_keywords_are_sorted_and_lowercase() {
        assert_sorted_and_lowercase(SWIFT_KEYWORDS);
    }

    #[test]
    fn windows_device_names_are_sorted_and_lowercase() {
        assert_sorted_and_lowercase(WINDOWS_DEVICE_NAMES);
    }

    #[test]
    fn is_reserved_is_case_insensitive() {
        assert!(is_reserved("self"));
        assert!(is_reserved("Self"));
        assert!(is_reserved("SELF"));
        assert!(is_reserved("nul"));
        assert!(is_reserved("Nul"));
        assert!(is_reserved("goto"));
        assert!(is_reserved("const"));
        assert!(is_reserved("native"));
        assert!(is_reserved("default"));
        assert!(is_reserved("type"));
        assert!(is_reserved("match"));
        assert!(is_reserved("com1"));
    }

    #[test]
    fn gen_is_reserved_in_the_2024_edition() {
        // `gen` became a reserved keyword in the 2024 edition (RFC 3513),
        // which is the edition this workspace builds with, so a `gen` slug
        // could never be a Rust lib or module name.
        assert!(is_reserved("gen"));
    }

    #[test]
    fn is_reserved_rejects_non_keywords() {
        assert!(!is_reserved("thoughts"));
        assert!(!is_reserved("foundry"));
        assert!(!is_reserved("system"));
    }
}
