//! The one check that reads the operator's real `WILLIKINS_DOPPLER_TOKEN`:
//! that [`credential_from_env`] accepts what the sandbox environment
//! actually holds, and that an accepted credential shows nothing of the
//! value it wraps.
//!
//! Since 2026-09-14 the sandbox file carries a `dp.sa.` service-account
//! token for the operator's dedicated Doppler test workplace, so this
//! test asserts acceptance: the `Missing` and `WrongKind` branches are
//! failures now, not outcomes, and each says what to do about it. Before
//! that date the file held a `dp.st.` service token and this test could
//! only observe the refusal. No branch ever shows the value.
//!
//! `src/client.rs`'s own unit tests pin [`CREDENTIAL_PATTERN`] against
//! hand-built token strings, and the `WrongKind` message against a
//! marker. What they cannot reach is the
//! `CredentialError::Malformed` -> [`DopplerCredentialError::WrongKind`]
//! mapping itself: [`Credential::from_env`] reads the process
//! environment, and mutating that is `unsafe` in edition 2024, which this
//! workspace forbids outright — so only a real environment exercises it.
//!
//! `#[ignore]`, because a plain `cargo test --workspace` must not depend
//! on an operator's environment. Run it with the sandbox env file sourced
//! in the *same* command, never by exporting the value into a shell that
//! outlives it:
//!
//! ```text
//!   source ~/.config/willikins/sandbox.env \
//!     && cargo test -p willikins-providers-doppler --test credential_env \
//!        -- --ignored --nocapture
//! ```
//!
//! No branch of this test makes a network call: the format check happens
//! before a [`willikins_providers_http::Http`] is ever built, which is
//! the whole point — a `dp.st.` service token is refused without one
//! request being sent to Doppler.

use willikins_providers_doppler::{CREDENTIAL_VAR, DopplerCredentialError, credential_from_env};

/// What the environment holds, named without ever being shown.
///
/// The sandbox file holds a provisioning token, so `Ok` is the expected
/// branch and the two refusals fail. The `Ok` branch checks the invariant
/// that matters on its own side: an accepted `Credential`'s `Debug` is
/// the redaction marker and carries no part of the token, not even the
/// prefix that classified it. The refusal branches still assert that a
/// `WrongKind` renders identically to one built from nothing — a future
/// variant that captured the value would show it in `Debug` first — and
/// then fail with willikins' own words.
#[test]
#[ignore = "reads the operator's real WILLIKINS_DOPPLER_TOKEN; run with \
            ~/.config/willikins/sandbox.env sourced in the same command"]
fn the_environment_credential_is_a_provisioning_token_and_stays_redacted() {
    match credential_from_env() {
        Ok(credential) => {
            println!("{CREDENTIAL_VAR}: accepted — a `dp.sa.` or `dp.pt.` provisioning token");
            let debug = format!("{credential:?}");
            assert!(
                debug.contains("REDACTED"),
                "an accepted Credential's Debug must stay redacted: {debug}"
            );
            for prefix in ["dp.sa.", "dp.pt.", "dp.st."] {
                assert!(
                    !debug.contains(prefix),
                    "an accepted Credential's Debug must carry no part of the token"
                );
            }
        }
        Err(DopplerCredentialError::Missing(err)) => {
            let message = err.to_string();
            assert!(message.contains(CREDENTIAL_VAR), "{message}");
            panic!(
                "{CREDENTIAL_VAR} is unset: source ~/.config/willikins/sandbox.env in the \
                 same command that runs this test"
            );
        }
        Err(err @ DopplerCredentialError::WrongKind) => {
            assert_eq!(
                err.to_string(),
                DopplerCredentialError::WrongKind.to_string(),
                "the refusal must render identically to a WrongKind built from nothing"
            );
            assert_eq!(
                format!("{err:?}"),
                "WrongKind",
                "WrongKind's Debug must carry no payload at all"
            );
            let message = err.to_string();
            assert!(message.contains("service-account"), "{message}");
            assert!(message.contains("personal"), "{message}");
            panic!(
                "{CREDENTIAL_VAR} is not a provisioning token; the sandbox file has held a \
                 `dp.sa.` service-account token since 2026-09-14"
            );
        }
    }
}
