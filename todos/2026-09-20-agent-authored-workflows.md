---
title: "Agent-authored workflows: a future problem, and the shape of the answer"
created: 2026-09-20
status: open
priority: low
area: auth
related:
  - docs/plans/2026-09-11-willikins-design.md
  - todos/2026-09-16-unattended-agents-token-longevity.md
  - todos/2026-09-16-pluggable-auth-adapters.md
---

# An agent writing a workflow, and why it is not the problem it looks like

The operator wants an agent to be able to compose a workflow from the patterns in existing
projects, store it, and later list the stored workflows by name and technology so an agent can
pick one, compose several, or write a new one.

I flagged that as colliding with the design's invariant that workflow documents are privileged
content run only from a trusted ref, and proposed that an agent's document become a pull request
against the trusted repository. The operator corrected both halves, 2026-09-20.

**The PR framing was this repository's habit projected onto other people's deployments.**
"We won't be storing other people's workflows in *our* repository. It will be up to them how they
decide to store / version them." willikins is self-hosted software. What counts as the trusted ref
is a property of a deployment, not of the project: a repository, a directory, a volume, whatever
the operator of that deployment decides. The invariant is that documents come from somewhere the
operator trusts; *which* somewhere is theirs to choose, and willikins should not assume a
repository, let alone this one's review conventions.

**The mechanism already exists, one level up.** "The tool could explicitly tell the agent to check
with the user (by returning a confirmation prompt / state on the initial creation request)." That
is exactly what plans already do: a plan whose class requires approval is recorded, answers
`ApprovalRequired`, and waits for a human at the approvals page before anything runs. A new
document is the same shape of thing one level higher — it is proposed, it is pending, a human
approves it, and only then can it be applied. Nothing new has to be invented, which is the same
lesson as entitlements being ordinary inputs: notice the machinery that is there before building
more.

**And the stronger version is a credential question, not a storage question.** "Later we could
have those use different credentials, if willikins is hosted on a backend." An agent-authored
document could run with a narrower credential set than one the operator wrote, which turns "may
this document run" from a yes/no into a question about what authority it carries. That is
attenuation, and it is the same shape as the capability tokens recorded in
`todos/2026-09-16-unattended-agents-token-longevity.md` — worth noticing that the two land in the
same place from opposite directions.

**Sequencing: this is a future problem.** The operator said so plainly, twice. Nothing in it is
needed to provision the projects that are waiting. What is needed first is the one missing
capability every catalogue entry depends on — no tool can put a file in a repository — and the
workflow documents themselves, which a human writes for now.

## Also banked, same day: bringing an existing project into a monorepo

`apps/phil-connors` holds a single `BUILD.bazel` while the real project still lives in its own
repository, so a migration is in progress. That makes a third workflow shape, beside "new
repository" and "new directory in a monorepo": move an existing project in. The operator's verdict:
"that's a future improvement". Recorded so the survey's catalogue does not quietly grow a column
for it.
