---
title: "Usable from a phone: self-describing workflows and enumerable input values"
created: 2026-09-20
status: open
priority: high
area: server
related:
  - docs/plans/2026-09-16-milestone-2c-authorization.md
  - docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md
  - todos/2026-09-20-agent-authored-workflows.md
---

# The use case that reorders the roadmap

The operator, 2026-09-20: "ideally, I should be able to use it straight from the Claude or ChatGPT
iOS apps, as a simple MCP, without the assistant having any idea what my machine's or Github set
up are. So workflows should be self descriptive, and available values (ie different github org
names if there are credentials for more than one org) should be surfaced to the agent."

This is a different deployment from the one every plan has assumed. Every acceptance test to date
runs willikins locally, where the operator's shell already holds the credentials and the agent
runs on the same machine as the repository. A phone changes three things at once: the server must
be reachable, the agent has no local context whatsoever, and the human is not at a terminal.

## What already works, and should not be rebuilt

`describe` is already rich. For each input a document needs it returns the name, the domain type,
that type's full JSON Schema (pattern, maximum length, examples), the document's own description of
the input, any default, an example, and a ready-made natural-language prompt. An agent that has
never seen this deployment can already ask a good question about *shape*.

## The gap, stated exactly

What `describe` cannot say is which **values** are available here. `GitHubOrg` gives the grammar of
an organisation login; it does not say that this deployment holds credentials for two particular
organisations and no others. The agent is left guessing at precisely the fact it cannot know.

Two kinds, and they differ in cost:

- **Static, from configuration.** Which GitHub organisations, Doppler workplaces and Buildkite
  organisations this deployment has credentials for. No network call, no secret exposed — the names
  are not secret, only the tokens are. This is the same fact as milestone 3's per-target credential
  routing seen from the other side: once a credential is chosen by the target's name, the set of
  configured credentials *is* the enumerable set. The two features are one.
- **Dynamic, from a provider.** Which Buildkite clusters exist in that organisation, which Doppler
  projects already exist, whether a repository name is taken. Costs an API call and a rate-limit
  slot, so it belongs behind an explicit call rather than inside every `describe`.

## Workflow self-description without new metadata to maintain

`list_workflows` should report, per document, its name, its description, and the technologies it
touches. The last needs no hand-written field and should not have one: the set of providers is
computable from the tool names in the graph, so it cannot drift from what the document does. A
document that gains a Buildkite step starts reporting Buildkite the moment it does, without anyone
remembering to update a tag.

## What this reorders

1. **Milestone 2c stops being optional.** A phone means a remote server, which means a public
   domain, which is precisely what 2c's authorisation work exists to make safe. The plan's own
   framing — that 2c buys a reachable deployment and nothing for local use — was right, and this
   is the use case that wants a reachable deployment.
2. **Per-target credential routing becomes a prerequisite**, not a milestone 3 nicety: enumeration
   has nothing to enumerate until more than one credential per provider is expressible.
3. **The approval gate matters more than the plan assumed.** Milestone 2 deferred push
   notifications because "one operator polling a page is enough to prove the gate". On a phone, an
   approval that requires noticing a web page is friction at exactly the wrong moment. The agent
   can hand over a link, which may be enough; whether it is should be decided by using it rather
   than by argument.
