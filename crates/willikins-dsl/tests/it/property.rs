//! `parse_document` must never panic, no matter what garbage it is handed
//! — a malformed document is always a `DocumentError`, never a crash.

use proptest::prelude::*;
use willikins_dsl::parse_document;

proptest! {
    #[test]
    fn parse_document_never_panics_on_arbitrary_strings(source in ".*") {
        let _ = parse_document(&source);
    }

    #[test]
    fn parse_document_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let source = String::from_utf8_lossy(&bytes);
        let _ = parse_document(&source);
    }
}
