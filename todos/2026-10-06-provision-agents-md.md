---
title: Provision a project's AGENTS.md
created: 2026-10-06
status: pending
priority: high
area: tools, documents
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

- **A file write, nothing more.** Write `AGENTS.md` where the project lives if it is absent;
  never overwrite it, never track edits afterwards. Re-running converges: present means done.
- **The content comes from the document.** The operator, 2026-10-06: "the content would have to
  come from the document. It's just file writing, with limited text-only templating capability."
  The whole file is a template in the document (the organisation's and monorepo's part in the
  organisation document, through milestone 2b's `uses:`; the project's part in the project's
  document), rendered by `repo.file.render`. Inputs only fill placeholders; no input supplies a
  template or a free-form section.
- **Projects already scaffolded.** A scaffold is a seed: once its marker exists it never writes
  again, so a document that adds AGENTS.md to an already-landed scaffold would never write it. This
  needs a small write-if-absent file tool (one file, no marker), usable for any later file.

## Design points for the plan

- Placeholders today take `TemplateValue`, an identifier-like grammar with no spaces. Prose values
  (a project's one-line description, say) need a text placeholder kind. Decide whether that kind is
  admitted only in Markdown files, how it is bounded (length, no control characters, no fence or
  heading injection), and that a secret type can never fill it.
- The rendered file is shown in full in the plan, so approving the plan approves the words.
- Which repository and path: a monorepo app writes `apps/<slug>/AGENTS.md`; a standalone repository
  writes `AGENTS.md` at its root (milestone 3l's new repositories).

## When

After milestone 2b (it needs the organisation document), before the rest of milestone 3n.
