---
title: Remove plan.rs's last two counted fallbacks (ratchet 8 to 6)
created: 2026-10-11
status: pending
priority: low
area: core
related:
  - docs/plans/2026-10-05-milestone-3n-checked-as-a-typed-graph.md
---

# plan.rs's last two counted fallbacks

Milestone 3n's ratchet pins plan.rs at 8 `unreachable!` sites; its plan aimed for 6. The two left are
`resolve_source`'s `Source::Item` arm and `resolve_list_source`'s `edge.elements()` unwrap. Removing the
first needs a closure-typed item resolver (and a Walk/EachInstance shape change so apply does not need
`erase_ports`); removing the second needs `Bound::List` to carry its element edges without an `Option`.
See the plan's G7 addendum for the reasoning. Lower the ratchet constant when done.
