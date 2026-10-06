---
title: A reusable iOS-app document, and the RepoPath derivation it needs
created: 2026-10-06
status: pending
priority: medium
area: documents, types
related:
  - docs/plans/2026-10-05-milestone-2b-composition.md
---

# A reusable iOS-app document

Milestone 2b's stage 1 split the operator's iOS app document into an organisation document and the
app's own document. Stage 2 (decision (o2)) is the end state: one reusable `ios-app` document that
any app in the organisation uses with its identifiers and slug, so a new app is a two-step root
(`org: { uses: example-org }`, `app: { uses: ios-app, with: { slug, identifiers, ... } }`).

## The missing primitive

Every rendered file is written at an exact `RepoPath`, and today every path is a literal under the
app's directory (`apps/<slug>/...`). A reusable document cannot write literals naming one app. No pure
tool derives a `RepoPath` from typed parts, so stage 2 waits for one. It needs its own reviewed
decision: a typed path derivation, such as a `RepoPath`-under-directory join, or a `naming` row.
Where an organisation keeps its apps is policy, so the directory comes from the organisation
document, never from a tool.

## Done when

The operator's iOS app document is a two-step root using the organisation document and `ios-app`,
and a real `plan --live` still reads every node NoOp.
