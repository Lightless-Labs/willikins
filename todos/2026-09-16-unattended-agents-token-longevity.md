---
title: "Unattended agents: how an agent's authority outlives a consent screen"
created: 2026-09-16
status: open
priority: medium
area: auth
related:
  - docs/plans/2026-09-16-milestone-2c-authorization.md
  - docs/research/2026-09-16-m2c-own-authorization-server.md
  - todos/2026-09-16-pluggable-auth-adapters.md
---

# Unattended agents, and the three ways to give one durable authority

Milestone 2c issues access tokens that live one hour and **no refresh tokens**, and it publishes
**no revocation endpoint**. Both are deliberate and both are recorded in the plan's deferral list;
this todo is that entry banked properly, because a deferral that lives only inside a 2,800-line
plan is a deferral nobody finds again.

## What was decided, and the reasoning that must survive

**No refresh tokens.** Issuing them is the only stateful thing an authorization server has to do:
done correctly it means rotation on every use, reuse detection, and revoking a whole token family
when an old member is replayed, which is durable state, a garbage collector and a set of races.
The research note's reading of existing Rust identity servers found their worst problems all lived
on that path. The MCP specification permits skipping it: clients "MUST NOT assume refresh tokens
will be issued; the authorization server retains discretion".

**No revocation endpoint.** A grep for revocation across all five fetched MCP authorization pages
returns zero hits, and RFC 8414 makes the metadata field optional.

**They are deferred together, and that pairing is the part to preserve.** Refresh tokens without
revocation are worse than neither: a stolen refresh token is then a grant with no expiry and no way
to cancel it. Revisit both at once or neither.

## The cost that prompted this todo

The access-token lifetime is exactly how often a human re-consents at `/authorize`. That is fine
for a person at a browser and fine for the provisioning cadence the operator describes: "Someone
won't provision a new project five times a day." It stops being fine when an agent is meant to run
unattended and its token expires mid-run, because an agent can neither refresh nor complete a
consent screen. Its only recourse is to fail and wait for a human. The operator, 2026-09-16: "at
some point you might want an agent to be able to do it on its own."

## Three ways out, cheapest first

1. **A longer lifetime for one specifically scoped client.** No new machinery: the client table
   already exists (pre-registered clients, decision 23), so a per-client TTL is a column. It buys
   longevity and buys nothing else, and it widens the leak window in exact proportion. Adequate if
   "unattended" means hours.
2. **Refresh tokens plus revocation, together.** The conventional answer, and the one with the
   most prior art to copy and the most state to get right. Only worth it if something else already
   forces durable authorization-server state.
3. **Capability tokens with offline attenuation** — the operator's suggestion, 2026-09-16: "we
   might end up looking into Biscuits and Macaroons (and maybe a pastry shop side-business)."
   This is the interesting one for willikins specifically, because willikins' authority is already
   *shaped*: a plan is a fixed set of operations against named resources. A capability token can
   carry that shape. The relevant property is attenuation: a holder can narrow a token further
   **without contacting the issuer**, so a long-lived agent credential could be attenuated down to
   one workflow, one project, one run, offline, per task.

   Verified facts only: `biscuit-auth` 6.0.0 is on crates.io, Apache-2.0, last published
   2025-07-16, about 11.4M all-time downloads, repository `biscuit-auth/biscuit-rust`, described by
   its own metadata as "an authorization token with decentralized verification and offline
   attenuation" (crates.io API, fetched 2026-09-16). Everything else about it, and everything about
   macaroons beyond their existence, is **unverified** and must be fetched before any of it enters
   a plan — including the obvious question of how a capability token coexists with the MCP
   authorization specification, which is written around OAuth 2.1 bearer tokens and JWT validation.
   A capability token is plausibly a *second* credential kind for a non-interactive grant rather
   than a replacement for the OAuth path, and that is the first thing to establish.

## What must stay true in milestone 2c for any of these to be cheap later

The same rule as the pluggable-auth todo, and for the same reason: the validation path reads a
configured issuer and key set, and the principal derives from issuer and subject. Nothing
downstream of the middleware knows how the caller proved anything. A second credential kind is
then a second middleware arm producing the same `Principal`, not a change to the `Butler`, the
journal or the tools.

Not now. When an agent actually needs to run without a human, or when someone wants the pastry.
