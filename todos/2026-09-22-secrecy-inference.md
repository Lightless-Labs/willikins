---
title: Infer secrecy from use sites instead of declaring it at the source
created: 2026-09-22
status: pending
priority: high
area: core/check
related:
  - docs/plans/2026-09-11-willikins-design.md
  - crates/willikins-core/src/check.rs
---

# Infer secrecy from use sites

The operator's design, 2026-09-22, offered while the App Store Connect lane was being
launched. It supersedes the two-tool split (`doppler.secret.get` / `doppler.value.get`)
that lane ships, and should delete `doppler.value.get` by making it redundant.

## The rule

In their words: *"At parsing time, likely: 'this value is used as a secret here, so it's
secret, so it cannot be used there, where it would treated as not a secret', or something
like that."*

Formally, a join over use sites. Secrecy is a two-point lattice with secret at the top.
A resolver's output carries a secrecy *variable* rather than a declaration. `check`
collects one constraint per use site, takes their least upper bound, and then re-checks
every use site against it:

- any use requiring secret makes the value secret;
- a non-secret use of a value whose join is secret is an **error**, not a silent choice;
- an output nothing consumes has no constraints, so it defaults to secret; a dead output
  is harmless but should not default to the bottom of the lattice.

Monotone, and secrecy only ever propagates upward.

## Why this is better than declaring it

It buys a property declaration cannot: **conflict detection**. Under declaration-by-tool-
choice, a document that binds one resolver output to both a secret and a non-secret port
silently gets whichever the tool declared. Under inference that document does not check.

It also removes a primitive per provider: one `doppler.get`, not two, and the document
says less.

## The two boundaries, both of which must hold

1. **Direction.** Inference solves only for *unannotated* resolver outputs. It never
   relaxes a port, and a concrete secret type (`DopplerSecretValue`, `AppleSigningKey`,
   `OpaqueSecret`) stays concrete and still cannot reach a non-secret port. If a use site
   could determine the secrecy of a known-secret value, declassification becomes automatic
   and invisible — the failure mode that makes taint systems leak.
2. **Where it ends.** The operator: *"Leaves the issue of CI env params and secrets,
   obviously."* Correct — once a value is written out to a CI environment variable it has
   left the graph and nothing downstream is inferable. That edge is what `SinkToken` and
   the secret-sink rule already exist for. So: inference inside the graph, the sink rule
   at the boundary. Say in the plan how the two meet, because a sink is exactly a use site
   whose secrecy requirement is not a port's.

## What inference still cannot do

Nothing recovers the fact that `ASC_API_KEY_BASE64` is sensitive and
`ASC_API_KEY_ISSUER_ID` is not. That lives in Doppler's per-secret `computedVisibility`
(`masked` / `restricted` / `unmasked`, and `computed` propagates through references) or in
the author's head. The operator declined to gate on visibility for a practical reason
worth recording: *"I only ever mark Doppler secrets as masked, no restricted, so....."* —
an `unmasked` gate would refuse their entire vault. So inference relocates the footgun; it
does not close it, and the plan should say so rather than imply otherwise.

## It must be enforceable with types, in two layers

The operator, 2026-09-22: *"And it should be enforceable with types"*. Reachable, with one
honest limit, and the plan must state both.

**The limit.** A workflow document is runtime data — node names, tool names and bindings
are read from YAML — so rustc cannot check the operator's document. Anyone who claims
otherwise is describing code generation nobody asked for.

**Layer 1, the document, runtime, type-enforced result.** `check` solves the join and the
only way to obtain a `Checked` is through `check`. That smart constructor already exists
and is the project's parse-don't-validate posture; extend it to carry the *solved*
secrecy assignment, so a `Checked` is a proof that every use site agreed, not merely that
someone once ran a validation function.

**Layer 2, willikins' own plumbing, compile time.** Make secrecy a phantom parameter on
`Value` so the executor cannot move a secret value into a non-secret slot: no such
function exists, and a public value is never constructible from a secret one. The join
becomes a total operation over distinct types rather than a runtime comparison a future
refactor can get backwards. This is the trick `SinkToken` already plays, and it is what
keeps a later refactor from laundering a secret inside the executor even when `check`
was right about the document.

The two layers meet at the sink, per the boundary section above: a sink is a use site
whose secrecy requirement is not a port's, so it is the one place both layers must agree
explicitly.

## The algorithm: solve backwards, from the ports that require secrets

The operator, 2026-09-22: *"We could parse backwards from output / value using nodes,
starting with those that take secrets. Then parsing would break on the rest."*

This is the right direction. There is no declared source to propagate forward from — that
is the entire point of inferring — so the only ground truth in the document is what
consumers *require*. Solve backwards from those.

A willikins graph is a DAG and `plan` already computes a topological order, so this is a
**single reverse-topological pass**, not an iterated fixpoint. State that in the plan; it
bounds the cost and makes the pass easy to prove terminating.

1. **Seed.** Every port whose spec requires secret — `exact("<a secret type>")` and
   `any_secret` — marks the producer output bound to it as secret.
2. **Propagate backward** through the producing node to its inputs, per that tool's
   secrecy signature (below).
3. **Break.** Any output marked secret in the pass that is *also* bound to a non-secret
   port fails to parse. This is the join re-check, reached backwards.

### The missing ingredient: a per-tool secrecy signature

Step 2 needs to know which inputs an output inherits secrecy from. Without it the pass
either over-taints (everything upstream of any secret becomes secret) or under-taints
(propagation stops at a node that in fact launders). Three kinds, and the existing tools
sort cleanly:

- **Transparent** — `base64.decode`, `apple.signing_key.parse`. Output secrecy *is* input
  secrecy; propagation passes through and continues upstream.
- **Polymorphic source** — `doppler.get`, `env.get`. Propagation terminates; this is the
  output whose secrecy the pass solves for.
- **Fixed** — every tool that does real work. Ports carry declared secrecy and propagation
  stops: a bundle identifier is not secret merely because a signing key is another input
  to the same node.

So the pass terminates at exactly the resolvers, which is what it is for.

### What it still will not catch

A resolver output consumed *only* by non-secret ports is never seeded, so it stays public.
That is correct for the Apple issuer id and it is the declassification footgun for the
key. The hole closes only at the sink boundary, per the section above — do not let the
plan imply inference closes it.
