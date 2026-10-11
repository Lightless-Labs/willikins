---
title: A readable multi-line view of rendered files in plan text and on the approvals page
created: 2026-10-11
status: pending
priority: medium
area: cli, server
related:
  - docs/plans/2026-10-06-milestone-3o-agents-md.md
---

# A readable view of rendered files

"Approving the plan approves the words" holds today only through escaped one-line text or JSON: no
output shows a `RepoFile` as the multi-line file it is. For milestone 3o the operator approved the
words from a draft shown separately, then the plan proved the rendered bytes equal to it. The plan
text and the approvals page should show each rendered file as a readable block (path, then content,
with the identifier masking of milestone 3i applied), so the plan itself is what gets read.
