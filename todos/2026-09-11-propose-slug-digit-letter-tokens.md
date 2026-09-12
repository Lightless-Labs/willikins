---
title: "propose_slug: digit-then-letter tokens"
created: 2026-09-11
status: open
priority: low
area: types
related:
  - crates/willikins-types/src/propose.rs
---

# propose_slug: digit-then-letter tokens

`propose_slug("Foo 3D Printing")` returns `ProposeError::Invalid` because the token `3d`
matches neither `[a-z][a-z0-9]*` nor `[0-9]+`. A friendlier proposal would split it into
`3` and `d` or rewrite it as `3-d`, giving `foo-3-d-printing`. propose_slug is off the
idempotence path, so this can change freely. Decide the rule, document it in the function's
doc comment, test it.
