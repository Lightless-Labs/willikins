---
title: Let willikins write files into a repository
created: 2026-09-29
status: done
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
still costs. It is Sample's largest remaining manual step (M3: the build files, entitlements,
Info.plist and `.buildkite/` files); it is the only route to app groups, which Xcode creates from
the entitlements file; and it is what makes a freshly created Buildkite pipeline useful, since the
pipeline reads `.buildkite/pipeline.yml` from the repository.

The survey's design notes stand: the file content type is non-secret by construction, so a secret
can never reach a committed file without a new rule; decide one commit versus many, direct commit
versus branch and pull request, where templates live, and what "ours" means for a file that
already exists. When this lands, Sample's M3 gate becomes a node instead of an acknowledgement.

**Done 2026-10-04:** milestone 3g (`docs/plans/2026-09-30-milestone-3g-file-writing.md`) built it: `repo.file.render`, `github.scaffold.ensure` (one signed seed commit, never overwrites), list bindings; milestones 3h and 3i added the pipeline templates and the bootstrap writer.
