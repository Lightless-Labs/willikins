---
title: "The iOS scaffolding step, and entitlements as ordinary unbound inputs"
created: 2026-09-20
status: open
priority: medium
area: providers
related:
  - docs/research/2026-09-16-app-store-connect.md
  - docs/plans/2026-09-11-willikins-design.md
---

# Entitlements are inputs, not a special case

The operator's framing, 2026-09-20, when asked what it would take to provision an iOS health
app end to end: "entitlements should just be parameters required by a Bazel or Tuist iOS app
creation step. And since no workflow step would provide them, they should surface as parameters
the caller has to provide. You know, dependency resolution."

That is right, and the important part is that **it needs no new mechanism**. willikins already
derives edges from data flow: a port bound by an upstream node is an edge, and a required port
that nothing produces is a required document input, which `describe` already reports by name (the
milestone 3a acceptance tests pin exactly that for `slug`, `org` and `buildkite_org`). So
entitlements are an input port on whatever step scaffolds the app, and the caller supplies them
because no node can.

## Two things that fall out, and are the reason to write this down

**One value, two consumers, one graph.** The same entitlement list feeds the Apple side
(`appstore.bundle_id_capability.ensure`, enabling the capability on the bundle identifier) and the
repository side (the scaffolding step, writing the entitlements file and the matching Info.plist
usage strings). That is the design doc's own argument for templating and provisioning sharing one
graph, made concrete: the two cannot disagree because they read the same typed value, and a
document that enables HealthKit on the identifier but omits it from the app is not expressible.

**The type has to carry what the API cannot do.** Not `list<String>`: Apple's `CapabilityType` is
a closed 28-member enum (research §2), which is exactly the shape of a willikins domain enum. But
the research also found that three of those members — `APP_GROUPS`, `APPLE_PAY` and `ICLOUD` — can
be *switched on* through the API and cannot be *configured*, because
`BundleIdCapabilityCreateRequest.relationships` has one member and it is the bundle identifier.
Enabling one of those produces a half-configured identifier. So the type should make the two
classes visible, and the tool must not report success as though the association happened. Whether
the ensure is idempotent at all is undocumented (a 409 is listed, but it is boilerplate on every
create), so read-then-create, never create-and-catch.

## The scaffolding step itself

Also an input, also not willikins' opinion: Bazel or Tuist is the operator's process, per the
design doc's "policy lives in the workflow, never in the tool". The step takes the build system,
the entitlements, the bundle identifier and the derived names, and produces a repository that
builds.

The cheap first version is not a templating engine. GitHub can create a repository from a template
repository in one call, so the hard part — making Bazel, `rules_rust`, UniFFI and `rules_apple`
actually build together — is authored once as a real repository that can be tested, and willikins
stamps it and then edits the few files that carry derived values. Full templating, with layered
profiles and re-rendering when a convention changes, is what the design doc describes and is a
milestone of its own.

## What stays manual whatever we build

The app record itself (website only, and gated behind the account holder signing the current
agreement), app groups and iCloud containers (absent from the API entirely), and the signing
certificate (its request needs a private key on a developer's machine, and a team gets one
distribution certificate of each type). All three are in the research note with their evidence.
