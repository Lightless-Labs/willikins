//! The unreachable ratchet (milestone 3n, S1 on; SHARED VALUES
//! "Unreachable ratchet"): counts non-test `unreachable!`/`.expect(`/
//! `.unwrap()`/`panic!` per file, in the text above that file's first
//! top-level `#[cfg(test)]` module line (the whole file, for one with
//! none), and pins today's counts. A task lowers these constants to what
//! it leaves and never raises one. `apply.rs` reached its final value (2)
//! at G6; `describe.rs` reaches its final value (0) at G7.
//!
//! `plan.rs` lands at G7 at **8**, not SHARED VALUES' pinned 6 -- see
//! `the_unreachable_ratchet_never_rises`' own G7 comment and the plan's
//! G7 addendum for the two fallbacks that account for the gap
//! (`resolve_source`'s `Source::Item` arm and `resolve_list_source`'s
//! `edge.elements()`), neither of which can be deleted or honestly
//! relocated out of this file without either redesigning `Bound::List`'s
//! own frozen (g1) shape or reopening the `Shape`-vs-`Group`
//! reconciliation `erase_ports` (G3) was built to avoid. Flagged for the
//! coordinator.

use std::fs;
use std::path::Path;

/// The four ratchet tokens, summed as non-overlapping substring matches --
/// exactly how the milestone plan's own baseline (`28dc3e1`) was measured.
const TOKENS: [&str; 4] = ["unreachable!", ".expect(", ".unwrap()", "panic!"];

/// `path`'s own text (relative to this crate's root), restricted to the
/// lines above its first top-level `#[cfg(test)]` module line, or the
/// whole file when it has none. Shared by [`ratchet_count`] and
/// [`registry_call_count`]: both pin something about a file's non-test
/// body only, never its tests.
fn non_test_body(path: &str) -> String {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = fs::read_to_string(&full_path)
        .unwrap_or_else(|error| panic!("{}: {error}", full_path.display()));
    match text.lines().position(|line| line.trim() == "#[cfg(test)]") {
        Some(index) => text.lines().take(index).collect::<Vec<_>>().join("\n"),
        None => text,
    }
}

/// Counts the ratchet tokens in `path`'s non-test body.
fn ratchet_count(path: &str) -> usize {
    let body = non_test_body(path);
    TOKENS.iter().map(|token| body.matches(token).count()).sum()
}

/// Counts `willikins_types::registry()` calls in `path`'s non-test body
/// (milestone 3n, decision (g5); acceptance 12): `describe.rs` stops
/// consulting the global registry once `CheckedInput::entry` carries the
/// catalog's own entry for it, computed once by `check`.
fn registry_call_count(path: &str) -> usize {
    non_test_body(path)
        .matches("willikins_types::registry()")
        .count()
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
    //
    // G3 (2026-10-11): `plan.rs`'s old `bind_ports` (two literal-port
    // fallbacks) is gone, replaced by scalar-port resolution straight off
    // `Source`/`PortEdge` -- net -1 even after the one new fallback
    // `resolve_source`'s own `Source::Item` arm needs (the same shape
    // `resolve_binding`'s matching arm already has, for the new source
    // path rather than the old `Binding` one). `apply.rs` holds flat:
    // `Shape`'s two variants and `Group`'s three are reconciled once, via
    // `erase_ports` (a total function, no fallback of its own) plus a
    // plain `matches!` check, not via a per-arm "the other Group variant
    // cannot happen here" fallback -- see `apply`'s own comment at the
    // call site. The one new index-out-of-range backstop
    // (`FilledOutputs::slot`'s own) lives in `willikins-core::tool`
    // instead, which this ratchet does not cover, for exactly this reason.
    //
    // G4 (2026-10-11): `Bound::List` gives a list-bound port its own
    // `PortEdge` too, so `bind_ports` resolves every port -- scalar or
    // list -- straight off `Source`/`Bound`, with no `node.with` fallback.
    // That was the old `Binding`-based resolver's only remaining caller,
    // so it and everything it alone kept alive go in the same commit:
    // `resolve_and_deliver`, the old by-name `resolve_list_binding`,
    // `resolve_binding`, `resolve_step`, `resolve_keyed`,
    // `aggregate_for_each_port`, `output_port_type` and
    // `ResolveCtx::deliver` -- each carrying one or more of its own
    // `unreachable!`/`unwrap_or_else` fallbacks. `plan.rs` drops from 29
    // to 10. `apply.rs` holds flat at 5: its own mirror
    // (`resolve_instance_inputs`'s list-port loop) swaps `resolve_and_deliver`
    // for the same `resolve_list_source` `plan.rs` gained, with no new
    // fallback either way.
    //
    // G6 (2026-10-11): `apply` now resolves every port through
    // `crate::plan::bind_ports` directly -- the exact function `plan`
    // itself calls -- instead of a layered `resolve_instance_inputs` that
    // started from `planned.inputs` and re-resolved only a
    // `source_may_change_mid_run` subset. `source_may_change_mid_run`,
    // `classify_unknown_binding` (two `unreachable!`s) and
    // `first_unknown_required_input`'s own "unbound required port" one
    // are all deleted: `bind_ports` returns an `UnknownOrigin` beside
    // every `Unknown` port the moment it is produced, so there is nothing
    // left to re-derive from a `Binding`. `apply.rs` drops from 5 to 2
    // (`check_drift`'s and `instance_drift`'s own, unmoved) -- this
    // milestone's final value for the file (SHARED VALUES). `plan.rs`
    // holds flat at 10: `bind_ports`'s and `resolve_list_source`'s new
    // `UnknownOrigin` side channel adds no fallback of its own.
    //
    // G7 (2026-10-11): `plan.rs` drops from 10 to 8, not SHARED VALUES'
    // pinned 6 -- flagged for the coordinator, with the full reasoning in
    // the plan's G7 addendum. One real deletion: `check_fixed_inputs`'s
    // own "a fixed input is always defaulted" fallback (reading
    // `CheckedInput::spec.default` through `Option::is_none_or`, which
    // fails *closed* on the impossible case rather than trusting it
    // silently, now that it no longer needs `Checked::workflow()` to walk
    // `workflow.inputs` either) -- this closes the gap G2's own addendum
    // flagged for the coordinator, and the matching `unreachable!` in
    // `describe.rs` (below) closes the same way. Two fallbacks the task
    // row's itemization did not list stay, honestly, right where they
    // were: `resolve_source`'s own `Source::Item` arm and
    // `resolve_list_source`'s `edge.elements()` unwrap. Both were
    // considered for relocation (mirroring `crate::tool::Outputs::
    // get_index`'s own precedent, G3) and reverted: `Outputs::get_index`
    // moved a *new* G3 fallback that had nowhere else to live; these two
    // are *existing*, already-counted members of this file's own ratchet
    // since G3/G4 (G3's own addendum explicitly counted `Source::Item`
    // here, the same addendum that moved `Outputs::get_index` elsewhere
    // -- the authors had the same choice available for this one and
    // declined it). Relocating either now would hide, not eliminate, the
    // value-level trust they stand for -- see the inline comment at each
    // one's own call site for why neither can be made unrepresentable by
    // the type system without either redesigning `Bound::List`'s frozen
    // shape or reopening `apply`'s `Shape`-vs-`Group` reconciliation.
    // `apply.rs` holds flat at its own final value, reached at G6.
    // `describe.rs` drops from 1 to 0: the same "always defaulted"
    // fallback as `plan.rs`'s, closed the same way (fails closed too,
    // though its own shape never risked opening: the error is pushed
    // unconditionally either way).
    assert_eq!(
        ratchet_count("src/plan.rs"),
        8,
        "plan.rs's unreachable ratchet moved; lower it, never raise it"
    );
    assert_eq!(
        ratchet_count("src/apply.rs"),
        2,
        "apply.rs's unreachable ratchet moved; lower it, never raise it"
    );
    assert_eq!(
        ratchet_count("src/describe.rs"),
        0,
        "describe.rs's unreachable ratchet moved; lower it, never raise it"
    );
}

/// The lines in `path` that are exactly a `#[cfg(test)]` attribute,
/// trimmed: the lines [`non_test_body`] (and `check.rs`'s own tripwire,
/// which splits the same way) may cut a file at.
fn cfg_test_line_count(path: &str) -> usize {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&full_path)
        .unwrap_or_else(|error| panic!("{}: {error}", full_path.display()))
        .lines()
        .filter(|line| line.trim() == "#[cfg(test)]")
        .count()
}

/// Every count above, and `check.rs`'s comment-stripped tripwire
/// (acceptance 7), reads only the text above a file's *first*
/// `#[cfg(test)]` line. Any other `#[cfg(test)]` item placed before the
/// test module -- a `use`, a helper, an empty `mod` -- would silently
/// exempt everything below it from both: `describe.rs`'s pinned 0 stays 0
/// however many fallbacks or registry calls follow (milestone 3n's X1
/// pass, mutation M-A, ran exactly that and every guard stayed green).
/// So the split itself is pinned: one `#[cfg(test)]` line, the test
/// module's own, in each file that has a test module, and none in
/// `apply.rs`. A new one is a stop: either it gates something below the
/// test module, or the split above must learn to skip it first.
#[test]
fn each_ratchet_file_is_split_at_its_test_module_only() {
    for (path, expected) in [
        ("src/plan.rs", 1),
        ("src/apply.rs", 0),
        ("src/describe.rs", 1),
    ] {
        assert_eq!(
            cfg_test_line_count(path),
            expected,
            "{path} gained or lost a `#[cfg(test)]` line: the ratchet and the tripwire read \
             only the text above the first one, so an early one hides the rest of the file"
        );
    }
}

/// Acceptance 12's second half (milestone 3n, task G2): `describe.rs`'s
/// non-test body calls `willikins_types::registry()` zero times.
/// `missing_input` and `awaiting_input` used to call it directly; both now
/// read `CheckedInput::entry` instead, which `check` resolved once against
/// the catalog's own registry. The file's tests still build fixtures
/// against the global registry (every other test file does too), so this
/// only ever reads the non-test body, like `ratchet_count` above.
#[test]
fn describe_consults_no_second_registry() {
    assert_eq!(
        registry_call_count("src/describe.rs"),
        0,
        "describe.rs calls `willikins_types::registry()` outside its own tests"
    );
}
