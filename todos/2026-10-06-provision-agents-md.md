---
title: Provision a project's AGENTS.md
created: 2026-10-06
status: pending
priority: medium
area: documents
related:
  - docs/plans/2026-10-05-milestone-2b-composition.md
  - docs/plans/2026-09-30-milestone-3g-file-writing.md
---

# Provision a project's AGENTS.md

The operator, 2026-10-06: willikins "will also need to be able to provision the AGENTS.md file".
Asked who owns it afterwards: "Willikins doesn't *own* anything, does it? It's just about
*provisioning*, no? (Which is just a file write, with part of the content org / monorepo specific,
and some of it provided as input when calling Willikins, likely either by the user or the agent
provisioning a project on the user's behalf)." AGENTS.md only, no CLAUDE.md.

## What it means

- **Document work, no new tool.** The operator, 2026-10-06: "No need to make a dedicated tool or
  anything." `AGENTS.md` is one more file a document renders and writes with the tools that exist:
  a `repo.file.render` node and the scaffold's file list.
- **The content comes from the document.** The operator, 2026-10-06: "the content would have to
  come from the document. It's just file writing, with limited text-only templating capability."
  The whole file is a template in the document: the organisation's and monorepo's part in the
  organisation document (milestone 2b's `uses:`), the project's part in the project's document.
  Inputs only fill placeholders (`TemplateValue`, as today); prose lives in the template itself.
- **New projects:** `AGENTS.md` joins the scaffold's `files`. **A project already scaffolded**
  (the operator's iOS app): a scaffold never writes again once its marker exists, so the document
  adds a second `github.scaffold.ensure` node with its own marker whose only file is `AGENTS.md`.
- Written once, never owned: the seed semantics already mean willikins never overwrites it.

## Design points

- The rendered file shows in the plan, so approving the plan approves the words.
- Path: a monorepo app writes `apps/<slug>/AGENTS.md`; a standalone repository writes `AGENTS.md`
  at its root (milestone 3l's new repositories).

## When

After milestone 2b (it needs the organisation document), before the rest of milestone 3n.
