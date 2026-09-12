//! Adversarial pass 2, finding 6: a `ParseError` message must never echo
//! an unbounded amount of the text it rejected.
//!
//! Every one of these messages is printed straight to the stdout an agent
//! reads, and the text they quote is whatever a workflow document, a
//! `--input` argument, or a `propose-slug` argument supplied — a hostile
//! document could make it arbitrarily large. `willikins_types::quoted`
//! bounds every quote at `MAX_QUOTED_INPUT` characters; these tests pin
//! that bound both directly and through the registry, which is the door
//! every literal and every `--input` value actually comes through.

use proptest::prelude::*;
use willikins_types::registry::TypeName;
use willikins_types::{MAX_QUOTED_INPUT, quoted, registry};

/// The most a message may be: the longest constraint sentence any parser
/// writes, plus one bounded quote, plus the "(N characters)" suffix.
/// Deliberately loose — the point is that it does not grow with the input.
/// A quote is bounded at `MAX_QUOTED_INPUT` *input* characters, each of
/// which may escape to as many as six.
const MESSAGE_CEILING: usize = 1024;

#[test]
fn quoted_passes_short_input_through_verbatim() {
    assert_eq!(quoted("widgets"), "`widgets`");
    assert_eq!(quoted(""), "``");
    let exact = "a".repeat(MAX_QUOTED_INPUT);
    assert_eq!(quoted(&exact), format!("`{exact}`"));
}

#[test]
fn quoted_cuts_long_input_and_says_how_long_it_was() {
    let long = "a".repeat(10_000);
    let rendered = quoted(&long);
    assert!(
        rendered.chars().count() < MAX_QUOTED_INPUT + 40,
        "quote was not bounded: {} characters",
        rendered.chars().count()
    );
    assert!(rendered.contains("10000 characters"), "{rendered}");
}

/// A YAML double-quoted scalar can carry a real newline or an ANSI escape.
/// Interpolating one raw would let a hostile document split one error line
/// into two, or write terminal control sequences to the stdout an agent
/// reads, so every character is escaped.
#[test]
fn quoted_escapes_control_characters() {
    let hostile = format!("Foo{}Bar{}[2J", '\n', '\u{1b}');
    let rendered = quoted(&hostile);
    assert!(
        !rendered.contains('\n'),
        "a raw newline survived: {rendered:?}"
    );
    assert!(
        !rendered.contains('\u{1b}'),
        "a raw escape survived: {rendered:?}"
    );
    assert!(rendered.contains("\\n"), "{rendered}");
    assert!(rendered.contains("u{1b}"), "{rendered}");
}

/// Ordinary text, including non-ASCII letters, is left alone: escaping is
/// for control characters, not for anything unfamiliar.
#[test]
fn quoted_leaves_printable_text_alone() {
    assert_eq!(quoted("café-münster"), "`café-münster`");
}

#[test]
fn quoted_cuts_on_a_character_boundary() {
    // Multi-byte characters either side of the cut: slicing by byte index
    // would panic if the cut were computed in bytes.
    let long = "é😀".repeat(1_000);
    let rendered = quoted(&long);
    assert!(rendered.contains("2000 characters"), "{rendered}");
}

#[test]
fn a_ten_megabyte_literal_does_not_reach_the_error_message() {
    let huge = "x".repeat(10_000_000);
    let err = registry()
        .parse(&TypeName::parse("ProjectSlug").unwrap(), &huge)
        .expect_err("a ten-megabyte slug must be rejected");
    assert!(
        err.reason.chars().count() < MESSAGE_CEILING,
        "the message grew with the input: {} characters",
        err.reason.chars().count()
    );
    assert!(
        err.reason.contains("10000000 characters"),
        "the message should still say how long it was: {}",
        err.reason
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// No rejection message ever carries a raw control character,
    /// whatever the input held.
    #[test]
    fn no_registry_rejection_carries_a_control_character(input in ".{0,200}") {
        let registry = registry();
        for entry in registry.iter() {
            let name = TypeName::parse(entry.info.name).expect("a registered type name");
            if let Err(err) = registry.parse(&name, &input) {
                prop_assert!(
                    !err.reason.chars().any(char::is_control),
                    "{name}: a control character reached the message: {:?}",
                    err.reason,
                );
            }
        }
    }

    /// For every registered non-secret type and any input at all, a
    /// rejection's message is bounded: it never grows with the input.
    #[test]
    fn every_registry_rejection_has_a_bounded_message(
        input in ".{0,2000}",
        index in 0usize..64,
    ) {
        let registry = registry();
        let names: Vec<TypeName> = registry
            .iter()
            .map(|entry| TypeName::parse(entry.info.name).expect("a registered type name"))
            .collect();
        prop_assume!(!names.is_empty());
        let name = &names[index % names.len()];
        if let Err(err) = registry.parse(name, &input) {
            prop_assert!(
                err.reason.chars().count() < MESSAGE_CEILING,
                "{name}: message of {} characters for an input of {} characters",
                err.reason.chars().count(),
                input.chars().count(),
            );
        }
    }
}
