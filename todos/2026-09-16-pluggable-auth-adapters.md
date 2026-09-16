---
title: "Pluggable authentication: keep the in-house default, allow adapters for external identity providers"
created: 2026-09-16
status: open
priority: medium
area: auth
related:
  - docs/plans/2026-09-16-milestone-2c-authorization.md
  - docs/research/2026-09-16-m2c-own-authorization-server.md
---

# Pluggable authentication, after milestone 2c ships

The operator's words, 2026-09-16, straight after ruling out any vendor identity provider as a
dependency: "If you want, we *can* think about later having auth be a pluggable system, with a
default in-house implementation and the ability to swap it (using adapters?) with other systems
such as self-hosted or cloud Logto, Clerk, and co. But that's something I'd bank as a later
improvement in a todo."

So this is banked, not scheduled. Milestone 2c builds one thing: willikins issues its own tokens
and authenticates its own humans, because a self-hoster must not inherit a dependency on someone
else's identity product. This item is the second half, for whoever wants the opposite: an
organisation that already runs an identity provider and would rather willikins delegate to it.

## Why it is cheap if the seam is right, and expensive if it is not

The seam already exists and is not an abstraction anyone had to invent: a resource server
validates a token against an **issuer** and a **key set**, and it does not care who minted it.
Milestone 2c's validation path (`oauth::validate`: issuer, audience, expiry, algorithm allowlist,
signature against a cached JWKS, required claims) is written against configuration, not against
willikins' own issuing code. If that stays true, delegating is a configuration change plus a login
flow, rather than a rewrite.

What milestone 2c must therefore avoid, and what this todo exists to check afterwards:

- The validator must never shortcut to the in-house signing key. It reads the configured issuer
  and JWKS like any other, even when both are its own.
- The principal derivation (`oauth-<12 hex of sha256(iss ‖ 0x00 ‖ sub)>`) already folds in the
  issuer, so tokens from a different issuer derive different principals by construction. Keep it.
- The two subject allowlists are per-issuer in effect. A migration between issuers rewrites them,
  which the plan already records; an adapter makes that a supported operation rather than an
  accident.
- The scope set is willikins' own vocabulary. An adapter maps a provider's claim shape onto it,
  which is the one piece of real translation work an adapter has to do.

## What this item would deliver

1. A trait or configuration boundary with two implementations: the built-in authorization server
   (the default, always present, no configuration required beyond a public URL) and a delegating
   mode (issuer, JWKS URI, the browser login's client credentials, and a claim mapping).
2. A conformance test suite both must pass, driven by the in-process fake authorization server
   milestone 2c already builds for its own tests.
3. Documentation saying plainly that the built-in path is the supported default and delegation is
   for organisations that already run a provider.

Not now. When someone actually wants it, or when milestone 2c's seam is proved by writing the
second implementation against it.
