---
title: "willikins-core's apply tests can drop the FixedTokenService substitute"
created: 2026-09-13
status: open
priority: low
area: core, providers-fake
related:
  - crates/willikins-core/tests/common/mod.rs
  - crates/willikins-core/tests/apply.rs
  - crates/willikins-providers-fake/src/tools/doppler_service_token_ensure.rs
---

# willikins-core's apply tests can drop the FixedTokenService substitute

Task 4a wrote `crates/willikins-core/tests/apply.rs` against
`common::apply_test_catalog`, which substitutes `common::FixedTokenService`
for the fake `doppler.service_token.ensure` because that tool then reported
its `token` output `Unknown` even on the freshly minting `ensure` call, so
`ci_secret` would have been blocked on the very first run.

Task 4b fixed the real tool: its create path now returns the minted token as
`Value::known`, and `FakeState::next_token` lets a test plant a distinctive
marker. Both 4a's and 4b's reports flag the substitute as redundant
follow-up work.

The swap is mechanical but touches every test in the file: use
`willikins_providers_fake::catalog(state)` directly, seed
`FakeState::with_next_token(distinctive_token())` wherever a test asserts on
the marker, and delete `FixedTokenService` and `apply_test_catalog` from
`tests/common/mod.rs`. Do it with the whole file's tests in view; the
`willikins-cli` acceptance suite already drives the real catalog end to end,
so nothing about the fake's behaviour is in question — only which catalog
these particular tests build.
