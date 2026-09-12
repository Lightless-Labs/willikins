# FakeState serialization is a redacted view, not a round trip

**Filed:** 2026-09-12 (end-to-end adversarial pass 2)

Serializing a `FakeState` prints the redaction marker in place of each seeded secret, so a
dump reloads with the marker as the value. Correct for a test fixture that must never leak,
but undocumented. Say so in the `--fake-state` docs, or split a `dump` that requires a
`SinkToken` from the redacted `Serialize`.
