//! The unreachable ratchet (milestone 3n, S1 on; SHARED VALUES
//! "Unreachable ratchet"): counts non-test `unreachable!`/`.expect(`/
//! `.unwrap()`/`panic!` per file, in the text above that file's first
//! top-level `#[cfg(test)]` module line (the whole file, for one with
//! none), and pins today's counts. A task lowers these constants to what
//! it leaves and never raises one; after G7 this test asserts the plan's
//! final values (`plan.rs` 6, `apply.rs` 2, `describe.rs` 0).

use std::fs;
use std::path::Path;

/// The four ratchet tokens, summed as non-overlapping substring matches --
/// exactly how the milestone plan's own baseline (`28dc3e1`) was measured.
const TOKENS: [&str; 4] = ["unreachable!", ".expect(", ".unwrap()", "panic!"];

/// Counts the ratchet tokens in `path` (relative to this crate's root),
/// restricted to the lines above its first top-level `#[cfg(test)]`
/// module line, or the whole file when it has none.
fn ratchet_count(path: &str) -> usize {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = fs::read_to_string(&full_path)
        .unwrap_or_else(|error| panic!("{}: {error}", full_path.display()));
    let body = match text.lines().position(|line| line.trim() == "#[cfg(test)]") {
        Some(index) => text.lines().take(index).collect::<Vec<_>>().join("\n"),
        None => text,
    };
    TOKENS.iter().map(|token| body.matches(token).count()).sum()
}

#[test]
fn the_unreachable_ratchet_never_rises() {
    // Measured at HEAD (milestone 3n's G1, 2026-10-11), after S1's 32/9/3
    // baseline: G1 gives `plan.rs` and `apply.rs` a node table with an
    // already-resolved tool handle (`CheckedNode::tool`), so neither looks
    // a node up in the workflow and its tool up in the catalog a second
    // time, and `apply`'s `Walk`/`Group` (decision (g6)) replace the
    // post-hoc "was this node a single instance or a `for_each`'s several"
    // unwraps with a shape known up front. `plan.rs` loses its one
    // node/tool-lookup pair (-2); `apply.rs` loses its own node/tool-lookup
    // pair plus its group-classification pair (-4). `describe.rs` is G2's.
    // G2 gives `describe` a `CheckedInput` table (`check`'s own one-time
    // registry lookup), which removes `missing_input`'s and
    // `awaiting_input`'s matching pair of "`check` already rejected an
    // unregistered input type" fallbacks (-2), leaving only the
    // unmoved "a fixed input is always defaulted" one. The plan's final
    // targets are `plan.rs` 6, `apply.rs` 2, `describe.rs` 0 (after G7).
    assert_eq!(
        ratchet_count("src/plan.rs"),
        30,
        "plan.rs's unreachable ratchet moved; lower it, never raise it"
    );
    assert_eq!(
        ratchet_count("src/apply.rs"),
        5,
        "apply.rs's unreachable ratchet moved; lower it, never raise it"
    );
    assert_eq!(
        ratchet_count("src/describe.rs"),
        1,
        "describe.rs's unreachable ratchet moved; lower it, never raise it"
    );
}
