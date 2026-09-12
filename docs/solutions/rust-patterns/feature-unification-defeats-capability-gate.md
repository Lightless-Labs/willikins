---
title: "A cargo feature cannot gate a capability inside the workspace that enables it"
category: rust-patterns
tags: [cargo, features, capability-token, clippy, disallowed-methods, secrets]
module: willikins-types
symptom: "A constructor meant to be reachable only from one crate is callable from every crate in the workspace"
root_cause: "Cargo unifies features per build, so enabling a feature in one dependent enables it for every crate that sees the same dependency"
date: 2026-09-12
---

# Feature unification defeats a capability gate

## Symptom

`SinkToken::new()` in `willikins-types` was placed behind an `executor` cargo feature so
that only the apply executor in `willikins-core` could construct the token that unlocks
`expose()` on a secret. A pre-task review pointed out, and a probe confirmed, that once
`willikins-core` enables the feature the CLI and the fake providers can call the
constructor too, because Cargo compiles `willikins-types` once with the union of features.

## What does and does not work

- A cargo feature gate protects external consumers who do not enable it. It does nothing
  inside the workspace that enables it.
- Sealed traits cannot help either: the implementor would have to live in the defining crate.
- `unsafe` tricks are forbidden by the workspace lints.

## Fix

Keep the feature gate for outside users, and enforce the rule inside the workspace with a
lint the gates already turn into an error:

```toml
# clippy.toml
disallowed-methods = [
  { path = "willikins_types::sink::SinkToken::new", reason = "only the apply executor may construct a SinkToken" },
]
```

Every legitimate call site carries a narrowly scoped `#[allow(clippy::disallowed_methods)]`
with a one-line comment, so `grep` lists them all. `cargo check -p willikins-types` stays a
fourth gate because the crate's self dev-dependency enables the feature for its own tests.

Say "enforced by lint" in the docs, not "provably". The structural aid is that `Tool::read`
takes no token, so a `read` implementation has no legitimate way to expose a secret.

## References

- `docs/plans/2026-09-11-milestone-1-core.md` "Type model", secret types bullet.
- `clippy.toml` at the workspace root.
