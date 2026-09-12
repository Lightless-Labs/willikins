---
title: "YAML scalar-alias memory amplification"
created: 2026-09-12
status: open
priority: high
area: dsl
related:
  - docs/research/2026-09-12-e2e-adversarial-pass-2.md
---

# YAML scalar-alias memory amplification

A 1 MB document with a 1 MB `description:` anchor referenced 2,000 times from a `list<Text>`
default peaked at 952 MB resident before `Text`'s length bound rejected it. The allocation
happens inside serde_yaml_ng before any domain type sees the value, so type bounds apply too
late, and a cap on document size only shrinks the quadratic. Options: parse into
`serde_yaml_ng::Value` first and count alias expansions, reject anchors and aliases outright
in the document format (nothing in milestone 1 needs them), or size-limit the raw document
and the number of aliases. The MCP server in milestone 2 must resolve this before accepting
documents from agents.
