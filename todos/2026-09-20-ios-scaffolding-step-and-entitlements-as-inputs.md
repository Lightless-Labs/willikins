---
title: "The iOS scaffolding step: entitlements are inputs, the build system is the document"
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

## The scaffolding step itself, and what is NOT an input

A first draft of this note said the build system is an input too. The operator corrected it the
same day: "Nope. It can just be a pre-assembled workflow for 'creating a new iOS app in monorepo x
of org y'." That is the sharper reading of "policy lives in the workflow", and the distinction is
worth stating exactly, because the first draft got it backwards.

**An input is what varies between runs of the same process.** The project slug varies. The
entitlements vary, because this app needs HealthKit and the next one needs push notifications.

**The document is what is fixed for that process.** An organisation that builds with Bazel does not
choose Bazel per app; the Bazel step simply *is* the step in their document. A `build_system:
bazel | tuist` input would be a switch nobody ever flips, and the cost is not only clutter.

The technical reason it is the better answer, and not merely the tidier one: a parameter that
selects *which tool runs* cannot be checked statically. `check` exists to reject everything
knowable before anything runs, and it can do that because the graph's tools are fixed in the
document and their ports are typed. Push tool selection into a runtime value and the DSL needs
conditionals it deliberately does not have (control flow is `when` guards and `for_each`, nothing
more), and a class of error moves from check time to apply time. Two small documents stay
statically checkable where one branching document would not.

So the shape is several pre-assembled documents — one per process an organisation actually has —
each with the build system baked in, each readable end to end, each taking only what genuinely
varies per app.

Note what the operator's own example changes about the graph: "a new iOS app in monorepo x" has no
`github.repo.ensure` node at all. The repository already exists, and the scaffolding step adds a
directory to it, which is a commit or a pull request against an existing repository rather than a
generate-from-template. That is a different missing capability from the one the greenfield case
needs, and both are the same underlying gap: no tool can put a file in a repository.

For the greenfield case, the cheap first version is not a templating engine. GitHub can create a
repository from a template repository in one call, so the hard part — making Bazel, `rules_rust`, UniFFI and `rules_apple`
actually build together — is authored once as a real repository that can be tested, and willikins
stamps it and then edits the few files that carry derived values. Full templating, with layered
profiles and re-rendering when a convention changes, is what the design doc describes and is a
milestone of its own.

## What stays manual whatever we build

The app record itself (website only, and gated behind the account holder signing the current
agreement), app groups and iCloud containers (absent from the API entirely), and the signing
certificate (its request needs a private key on a developer's machine, and a team gets one
distribution certificate of each type). All three are in the research note with their evidence.
