//! The one check that reads the operator's real `WILLIKINS_DOPPLER_TOKEN`:
//! that [`credential_from_env`] classifies whatever is actually in the
//! environment, and that a refusal carries nothing from the value it
//! refused.
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
/// Every branch asserts the same invariant from a different side: the
/// rendered refusal is a fixed string, identical to the one the variant
/// renders when constructed out of thin air, so it cannot be carrying any
/// part of what was actually supplied. `Debug` is checked too, because a
/// future variant that *did* capture the value would show it there first.
#[test]
#[ignore = "reads the operator's real WILLIKINS_DOPPLER_TOKEN; run with \
            ~/.config/willikins/sandbox.env sourced in the same command"]
fn the_environment_credential_is_classified_and_a_refusal_echoes_nothing() {
    match credential_from_env() {
        Err(DopplerCredentialError::Missing(err)) => {
            println!("{CREDENTIAL_VAR}: unset — nothing to classify");
            let message = err.to_string();
            assert!(message.contains(CREDENTIAL_VAR), "{message}");
        }
        Err(err @ DopplerCredentialError::WrongKind) => {
            println!(
                "{CREDENTIAL_VAR}: refused — not a provisioning token \
                 (a `dp.st.` service token, or not a Doppler token at all)"
            );
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
        }
        Ok(credential) => {
            println!("{CREDENTIAL_VAR}: accepted — a `dp.sa.` or `dp.pt.` provisioning token");
            let debug = format!("{credential:?}");
            assert!(
                debug.contains("REDACTED"),
                "an accepted Credential's Debug must stay redacted: {debug}"
            );
        }
    }
}
