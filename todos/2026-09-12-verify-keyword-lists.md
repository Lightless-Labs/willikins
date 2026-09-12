---
title: "Verify the Swift and Kotlin reserved-word lists against their source pages"
created: 2026-09-12
status: open
priority: high
area: types
related:
  - crates/willikins-types/src/reserved.rs
  - docs/plans/2026-09-11-milestone-1-core.md
---

# Verify the Swift and Kotlin reserved-word lists

`crates/willikins-types/src/reserved.rs` holds the union of Rust, Java, Kotlin hard, and
Swift keywords plus Windows device names. The Rust and Java lists are well established. The
Swift and Kotlin lists were written from recall because WebFetch was blocked for every agent
in the session that wrote them. The Rust 2024 `gen` reservation rests on RFC 3513.

The slug grammar is frozen once a project exists: a word accepted today and rejected later
duplicates resources on the next run. Check each list against docs.swift.org "Lexical
Structure" and kotlinlang.org "Keywords and operators" (hard keywords) with a browser, add
any missing entries test-first, and note the check date in the module doc. Do this before
the first real provisioning run.
