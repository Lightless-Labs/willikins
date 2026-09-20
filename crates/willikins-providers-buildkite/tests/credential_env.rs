//! The one check that reads the operator's real `WILLIKINS_BUILDKITE_TOKEN`:
//! that [`credential_from_env`] accepts what the sandbox environment
//! actually holds, and that an accepted credential shows nothing of the
//! value it wraps. Mirrors
//! `willikins-providers-doppler/tests/credential_env.rs` exactly.
//!
//! `#[ignore]`, because a plain `cargo test --workspace` must not depend
//! on an operator's environment. Run it with the sandbox env file sourced
//! in the *same* command:
//!
//! ```text
//!   source ~/.config/willikins/sandbox.env \
//!     && cargo test -p willikins-providers-buildkite --test credential_env \
//!        -- --ignored --nocapture
//! ```
//!
//! No branch of this test makes a network call.

use willikins_providers_buildkite::{
    BuildkiteCredentialError, CREDENTIAL_VAR, credential_from_env,
};

#[test]
#[ignore = "reads the operator's real WILLIKINS_BUILDKITE_TOKEN; run with \
            ~/.config/willikins/sandbox.env sourced in the same command"]
fn the_environment_credential_is_an_api_access_token_and_stays_redacted() {
    match credential_from_env() {
        Ok(credential) => {
            println!("{CREDENTIAL_VAR}: accepted -- a `bkua_` API access token");
            let debug = format!("{credential:?}");
            assert!(
                debug.contains("REDACTED"),
                "an accepted Credential's Debug must stay redacted: {debug}"
            );
            for prefix in ["bkua_", "bkct_"] {
                assert!(
                    !debug.contains(prefix),
                    "an accepted Credential's Debug must carry no part of the token"
                );
            }
        }
        Err(BuildkiteCredentialError::Missing(err)) => {
            let message = err.to_string();
            assert!(message.contains(CREDENTIAL_VAR), "{message}");
            panic!(
                "{CREDENTIAL_VAR} is unset: source ~/.config/willikins/sandbox.env in the \
                 same command that runs this test"
            );
        }
        Err(err @ BuildkiteCredentialError::WrongKind) => {
            assert_eq!(
                err.to_string(),
                BuildkiteCredentialError::WrongKind.to_string(),
                "the refusal must render identically to a WrongKind built from nothing"
            );
            assert_eq!(
                format!("{err:?}"),
                "WrongKind",
                "WrongKind's Debug must carry no payload at all"
            );
            panic!(
                "{CREDENTIAL_VAR} is not an API access token (`bkua_`); a `bkct_` agent token \
                 cannot provision"
            );
        }
    }
}
