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
- **Two sources of content.** The organisation's and monorepo's part lives in the organisation
  document (milestone 2b's `uses:`), so every project in that organisation gets the same part. The
  project's own part is an input supplied when willikins is called, by the operator or by an agent
  provisioning a project on the operator's behalf.
- **Projects already scaffolded.** A scaffold is a seed: once its marker exists it never writes
  again, so a document that adds AGENTS.md to an already-landed scaffold would never write it. This
  needs a small write-if-absent file tool (one file, no marker), usable for any later file.

## Design points for the plan

- The project's part is text an agent may write that later agents will read as instructions. It
  should be a bounded, typed Markdown value (not `Text`, not a template), refused if it is a secret
  type, and shown in full in the plan so approving the plan approves the words.
- The organisation's part is document content (privileged, from a trusted ref), rendered through
  `repo.file.render`; the input is placed only as a whole section, never spliced into a command,
  path or code fence.
- Which repository and path: a monorepo app writes `apps/<slug>/AGENTS.md`; a standalone repository
  writes `AGENTS.md` at its root (milestone 3l's new repositories).

## When

After milestone 2b (it needs the organisation document), before the rest of milestone 3n.
