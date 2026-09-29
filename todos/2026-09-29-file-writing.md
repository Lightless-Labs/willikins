---
title: Let willikins write files into a repository
created: 2026-09-29
status: pending
priority: medium
area: providers/github
related:
  - docs/research/2026-09-20-project-survey-and-workflow-library.md
  - docs/research/2026-09-20-workflow-library-design.md
  - docs/plans/2026-09-27-milestone-3e-new-ios-app.md
---

# File-writing

Rank 1 in the 2026-09-20 project survey: no tool can put a file in a repository. It was
deprioritised by the operator ("the least inconvenient part"), but milestone 3e showed what it
still costs. It is Walter's largest remaining manual step (M3: the build files, entitlements,
Info.plist and `.buildkite/` files); it is the only route to app groups, which Xcode creates from
the entitlements file; and it is what makes a freshly created Buildkite pipeline useful, since the
pipeline reads `.buildkite/pipeline.yml` from the repository.

The survey's design notes stand: the file content type is non-secret by construction, so a secret
can never reach a committed file without a new rule; decide one commit versus many, direct commit
versus branch and pull request, where templates live, and what "ours" means for a file that
already exists. When this lands, Walter's M3 gate becomes a node instead of an acknowledgement.
