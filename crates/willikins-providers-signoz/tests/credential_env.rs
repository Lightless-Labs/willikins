//! The one check that reads the operator's real `WILLIKINS_SIGNOZ_API_KEY`:
//! that [`credential_from_env`] accepts what the sandbox environment
//! actually holds, and that an accepted credential shows nothing of the
//! value it wraps. Mirrors `willikins-providers-buildkite/tests/credential_env.rs`
//! exactly.
//!
//! `#[ignore]`, because a plain `cargo test --workspace` must not depend
//! on an operator's environment. Run it with the sandbox env file sourced
//! in the *same* command:
//!
//! ```text
//!   source ~/.config/willikins/sandbox.env \
//!     && cargo test -p willikins-providers-signoz --test credential_env \
//!        -- --ignored --nocapture
//! ```
//!
//! No branch of this test makes a network call.

use willikins_providers_signoz::{CREDENTIAL_VAR, SigNozCredentialError, credential_from_env};

#[test]
#[ignore = "reads the operator's real WILLIKINS_SIGNOZ_API_KEY; run with \
            ~/.config/willikins/sandbox.env sourced in the same command"]
fn the_environment_credential_is_accepted_and_stays_redacted() {
    match credential_from_env() {
        Ok(credential) => {
            println!("{CREDENTIAL_VAR}: accepted");
            let debug = format!("{credential:?}");
            assert!(
                debug.contains("REDACTED"),
                "an accepted Credential's Debug must stay redacted: {debug}"
            );
        }
        Err(SigNozCredentialError::Missing(err)) => {
            let message = err.to_string();
            assert!(message.contains(CREDENTIAL_VAR), "{message}");
            panic!(
                "{CREDENTIAL_VAR} is unset: source ~/.config/willikins/sandbox.env in the \
                 same command that runs this test"
            );
        }
        Err(err @ SigNozCredentialError::Malformed) => {
            assert_eq!(
                err.to_string(),
                SigNozCredentialError::Malformed.to_string(),
                "the refusal must render identically to a Malformed built from nothing"
            );
            panic!(
                "{CREDENTIAL_VAR} does not match CREDENTIAL_PATTERN -- if the real key is \
                 shaped differently than this crate assumed, CREDENTIAL_PATTERN needs \
                 updating, not this test"
            );
        }
    }
}
