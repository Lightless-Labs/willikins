//! Whether an identifier-typed value prints in full or masked to a prefix.
//!
//! Milestone 3i, decisions (b1) and (b3). Secrecy and identifier-masking
//! are two different, independently-declared properties of a domain type
//! (see [`crate::DomainType::IS_SECRET`] and
//! [`crate::DomainType::IS_IDENTIFIER`]): a secret is always redacted, an
//! identifier is masked to a short prefix by default and printed whole only
//! when disclosure is explicitly requested (the CLI's `--reveal`). This
//! module holds the mechanism both the derive's generated `Debug` and
//! later output surfaces (`willikins-core`'s `Value::display`, `mask_json`)
//! share; it marks no type itself.

/// Whether an identifier-typed value should print as a short prefix or in
/// full.
///
/// [`Self::Masked`] is the default everywhere an identifier-typed value
/// reaches an output surface. [`Self::Revealed`] is reached only through an
/// explicit, operator-initiated request (the CLI's `--reveal`) — never
/// through the MCP server, which always masks (decision (b6)). A secret
/// type is unaffected either way: it stays `[REDACTED ...]` under both
/// variants (trust boundary 7 of this milestone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disclosure {
    /// Print an identifier-typed value as its masked prefix.
    Masked,
    /// Print an identifier-typed value in full.
    Revealed,
}

/// The greatest number of characters of an identifier's prefix
/// [`mask_identifier`] keeps, before it is itself capped to at most half
/// the input's length. See [`mask_identifier`] for the full rule.
pub const IDENTIFIER_PREFIX_CHARS: usize = 4;

/// Mask `s` — an identifier-typed value's canonical string — to its
/// prefix: the first `min(`[`IDENTIFIER_PREFIX_CHARS`]`, chars(s) / 2)`
/// characters, followed by three ASCII full stops.
///
/// Four characters are usually enough to recognise which record a line is
/// about; "never more than half" stops a two-character identifier (or
/// shorter) from printing whole. The three ASCII full stops (never `…`,
/// which keeps every terminal and log readable, and never any other
/// character) are chosen because no identifier grammar among this
/// milestone's seven admits a `.`: a masked value therefore never parses
/// as its own type, so an agent that feeds one back as an input gets a
/// loud `ParseError`, never a silently wrong identifier.
///
/// # Examples
///
/// ```
/// # use willikins_types::mask_identifier;
/// assert_eq!(
///     mask_identifier("57246542-96fe-1a63-e053-0824d011072a"),
///     "5724..."
/// );
/// assert_eq!(mask_identifier("2X9R4HXF34"), "2X9R...");
/// assert_eq!(mask_identifier("AB"), "A...");
/// ```
#[must_use]
pub fn mask_identifier(s: &str) -> String {
    let total_chars = s.chars().count();
    let keep = IDENTIFIER_PREFIX_CHARS.min(total_chars / 2);
    let prefix: String = s.chars().take(keep).collect();
    format!("{prefix}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_keeps_four_characters() {
        assert_eq!(
            mask_identifier("57246542-96fe-1a63-e053-0824d011072a"),
            "5724..."
        );
    }

    #[test]
    fn a_ten_character_key_id_keeps_four_characters() {
        assert_eq!(mask_identifier("2X9R4HXF34"), "2X9R...");
    }

    #[test]
    fn a_two_character_value_keeps_only_one_character() {
        // min(4, 2 / 2) == 1: never more than half the input.
        assert_eq!(mask_identifier("AB"), "A...");
    }

    #[test]
    fn a_one_character_value_keeps_nothing_but_the_dots() {
        // min(4, 1 / 2) == 0.
        assert_eq!(mask_identifier("A"), "...");
    }

    #[test]
    fn an_empty_value_is_just_the_dots() {
        assert_eq!(mask_identifier(""), "...");
    }

    #[test]
    fn a_short_three_character_value_keeps_one_character() {
        // min(4, 3 / 2) == 1.
        assert_eq!(mask_identifier("ABC"), "A...");
    }

    #[test]
    fn an_eight_character_value_keeps_exactly_four() {
        // min(4, 8 / 2) == 4, the cap, not the half.
        assert_eq!(mask_identifier("ABCDEFGH"), "ABCD...");
    }

    #[test]
    fn masking_is_unicode_scalar_aware_not_byte_aware() {
        // Each "é" is two bytes in UTF-8 but one `char`; slicing on byte
        // offsets would panic or split a code point. 10 chars, keep
        // min(4, 10 / 2) == 4.
        assert_eq!(mask_identifier("ééééééééée"), "éééé...");
    }

    #[test]
    fn the_result_always_ends_with_exactly_three_ascii_full_stops() {
        for input in ["", "A", "AB", "ABCDEFGHIJKLMNOP"] {
            let masked = mask_identifier(input);
            assert!(masked.ends_with("..."));
            assert!(!masked.ends_with("...."));
        }
    }
}
