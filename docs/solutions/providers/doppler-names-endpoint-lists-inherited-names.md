---
title: "Doppler's secret-names endpoint lists names a config inherits from a base config"
category: providers
tags: [doppler, rest, config-inheritance, secret-names, live-test]
module: willikins-providers-doppler
symptom: "unknown before milestone 3j: Doppler's reference for GET /v3/configs/config/secrets/names does not say whether a name inherited from a base config is listed, so doppler.secret_name.gate also walks the config's inherits array to stay correct under either answer"
root_cause: "not a defect; an undocumented behaviour, settled by the milestone 3j live names cycle in the sandbox workplace on 2026-10-04: the endpoint lists inherited names"
date: 2026-10-04
---

# Doppler's names endpoint lists inherited names

## The question

`GET /v3/configs/config/secrets/names?project=...&config=...` returns `{"names": [...]}`. Doppler's reference page
(`https://docs.doppler.com/reference/secrets-names.md`) does not say whether a name the config inherits from a base
config (Doppler's config inheritance) is listed. Milestone 3j's `doppler.secret_name.gate` needs to know when a name
is visible in a config, whether it is set there or inherited. The plan made the gate correct under either answer: if
the direct listing does not have the name, the gate reads the config's own `inherits` array and lists each base
(`docs/plans/2026-10-03-milestone-3j-apns-key-observed.md`, decision (b4), verify item 1).

## The answer

**It does.** The coordinator ran `crates/willikins-providers-doppler/tests/live_secret_name_gate_cycle.rs` once in
the sandbox workplace on 2026-10-04, and all 12 steps passed. Step 8 lists names on a child config that inherits a
base config holding the probe secret, which was never set on the child itself. It reported that the endpoint
lists the inherited name. Step 9's gate read `Present`.

## What follows

- A reader of a config sees inherited names directly. This matches milestone 3h's probe, where a reader of a config
  saw inherited secret values through the download and single-secret endpoints without any access to the base
  project.
- In `doppler.secret_name.gate`, the walk over `inherits` is now a backstop. It runs only when Doppler's own direct
  listing says the name is not visible. Its one observable cost: a base that answers `403` or `5xx` turns that
  `Absent` into an `Err`, so the run fails instead of blocking. Whether to keep the walk is a reviewed change of its
  own (`docs/research/2026-10-03-m3j-adversarial-pass.md`, design notes).
- Still unverified: whether a branch config's listing also includes its environment root's names, and whether an
  inheritable config may itself inherit (verify item 2).
