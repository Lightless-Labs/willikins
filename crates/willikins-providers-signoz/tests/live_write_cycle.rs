//! The live `SigNoz` **write** cycle: the one test in this crate that
//! mints a real ingestion key and then removes it again.
//!
//! **This account is the operator's PRODUCTION `SigNoz` account, not a
//! sandbox.** As of 2026-09-21 it holds seven ingestion keys this test
//! must never touch, rename, or delete: `infrastructure`, `pessimal-ios`,
//! `pocket-companion-prd`, `claude-pessimal-test`, `phil-connors-prd-ios`,
//! `phil-connors-prd-backend`, `Danksworth-Ingestion`. Before creating
//! anything, this test lists the account and refuses to proceed --
//! minting nothing -- unless the name set matches that seven exactly.
//! After the cycle, it re-lists and asserts the name set is back to
//! those same seven. Everything this test creates is named
//! `willikins-<random>-delete-me`, deleted through a panic-safe
//! [`KeyGuard`] so a failed assertion or an early return still cleans up.
//!
//! Compiled only with the crate's `live-tests` feature (this crate's
//! `Cargo.toml` carries `required-features` on this file's `[[test]]`
//! entry), so a plain `cargo test --workspace` never builds it.
//! `#[ignore]` on top of that, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- the credential and host are read only
//! past that gate.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-providers-signoz \
//!   --features live-tests --test live_write_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! No `gh` command is run. The credential is sourced only inside the
//! single command above and is never printed.

use std::sync::Arc;

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};
use willikins_types::{DomainType, SigNozIngestionKeyName};

/// The seven keys already in the operator's production account
/// (2026-09-21). Never touched, renamed, updated, or deleted by this
/// test -- only ever compared against, as a set, before and after.
const EXPECTED_EXISTING_NAMES: [&str; 7] = [
    "infrastructure",
    "pessimal-ios",
    "pocket-companion-prd",
    "claude-pessimal-test",
    "phil-connors-prd-ios",
    "phil-connors-prd-backend",
    "Danksworth-Ingestion",
];

/// This test's own throwaway name: `willikins-<12 random hex chars>-delete-me`.
/// Randomised (never fixed) so a leftover from an aborted run cannot
/// collide with a second run's own name, and always carries both the
/// `willikins-` prefix and `-delete-me` suffix the task requires, so a
/// leftover is unambiguous to find and safe to remove by hand.
fn throwaway_name() -> SigNozIngestionKeyName {
    use rand::Rng;
    let suffix: String = rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(12)
        .map(char::from)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    SigNozIngestionKeyName::parse(&format!("willikins-{suffix}-delete-me"))
        .expect("a generated throwaway name matches SigNozIngestionKeyName's pattern")
}

/// Deletes the ingestion key at [`Self::id`] on drop, unless
/// [`Self::disarm`] was called first — so a panic or a failed assertion
/// anywhere in the run still cleans up. Mirrors
/// `willikins-providers-buildkite/tests/live_write_cycle.rs`'s
/// `PipelineGuard`.
struct KeyGuard {
    client: Arc<SigNozClient>,
    id: String,
    armed: bool,
}

impl KeyGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.client.delete_ingestion_key(&self.id);
        }
    }
}

/// The account's current name set, as a sorted `Vec` (so a mismatched
/// order never fails a set-equality assertion, and so a failure message
/// prints something readable).
fn current_names(client: &SigNozClient) -> Vec<String> {
    let mut names: Vec<String> = client
        .list_ingestion_keys()
        .expect("listing ingestion keys")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
}

fn expected_names_sorted() -> Vec<String> {
    let mut names: Vec<String> = EXPECTED_EXISTING_NAMES
        .iter()
        .map(ToString::to_string)
        .collect();
    names.sort();
    names
}

fn inputs_for(name: &SigNozIngestionKeyName) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name.clone()));
    inputs
}

#[test]
#[ignore = "mints and deletes a real SigNoz ingestion key against the operator's PRODUCTION \
            account; run with WILLIKINS_LIVE_TESTS=1 and the sandbox env sourced in the same \
            command, under the live-tests feature"]
#[allow(clippy::disallowed_methods)] // a live-cycle test mints its own token, as every other does
fn signoz_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential =
        willikins_providers_signoz::credential_from_env().expect("a valid SigNoz API key");
    let base_url =
        willikins_providers_signoz::base_url_from_env().expect("WILLIKINS_SIGNOZ_HOST is set");
    let client = Arc::new(SigNozClient::new(willikins_providers_signoz::http_client(
        base_url, credential,
    )));

    // Precondition: exactly the seven known keys, nothing named like this
    // run's own throwaway shape already present from an aborted run.
    // Refuses -- minting nothing -- if this does not hold.
    let before = current_names(&client);
    assert_eq!(
        before,
        expected_names_sorted(),
        "refusing to proceed: the account's key set is not exactly the seven expected keys \
         (found: {before:?}). This test never mints when the account does not match what it \
         was told to expect."
    );
    assert!(
        !before
            .iter()
            .any(|name| name.starts_with("willikins-") && name.ends_with("-delete-me")),
        "refusing to proceed: a leftover willikins-*-delete-me key is already present \
         (found: {before:?}) -- remove it by hand before running this test"
    );
    println!("precondition: exactly the seven expected keys, no leftover: pass");

    let name = throwaway_name();
    println!("this run's throwaway key: {name}");

    // Create directly through the client (not `Tool::ensure`), so the
    // `id` this cycle needs to delete the key is available from the same
    // response that creates it -- no window between "the key exists" and
    // "this test knows how to remove it".
    let created = client
        .create_ingestion_key(&name)
        .expect("creating the throwaway key");
    let guard = KeyGuard {
        client: Arc::clone(&client),
        id: created.id.clone(),
        armed: true,
    };
    println!("created: id={}", created.id);

    let tool = SigNozIngestionKeyEnsure::new(Arc::clone(&client));
    let inputs = inputs_for(&name);

    // `read` now reports Present, with `key` Unknown -- the decision this
    // tool exists to make concrete: the list response does carry the real
    // value, and this tool never surfaces it, even for a key this very
    // process just minted.
    match tool.read(&inputs).expect("reading the just-created key") {
        Observation::Present(outputs) => {
            let value = outputs.get(&PortName::parse("key").unwrap()).unwrap();
            assert!(!value.is_known(), "read must report `key` Unknown");
            println!("read: Present, key Unknown: pass");
        }
        other => panic!("expected Present, got {other:?}"),
    }

    // `ensure` on an already-existing key converges: `changed: false`,
    // `key` still Unknown, and -- proved by `before`/`after` staying
    // equal -- no second key was minted.
    let sink = SinkToken::new();
    let before_ensure = current_names(&client);
    let ensured = tool
        .ensure(&inputs, &sink)
        .expect("ensure on an already-existing key");
    assert!(
        !ensured.changed,
        "ensure on an existing key must not report changed"
    );
    let key_output = ensured
        .outputs
        .get(&PortName::parse("key").unwrap())
        .unwrap();
    assert!(
        !key_output.is_known(),
        "ensure's converge arm must report `key` Unknown"
    );
    let after_ensure = current_names(&client);
    assert_eq!(
        before_ensure, after_ensure,
        "a converging ensure must not change the account's key set"
    );
    println!("ensure on an existing key: changed=false, key Unknown, no new key minted: pass");

    // Clean up, then disarm: an explicit, ordered delete rather than
    // leaving it to `Drop` alone, so a failure in the delete itself is
    // this test's own failure, not a silently swallowed one.
    client
        .delete_ingestion_key(&guard.id)
        .expect("deleting the throwaway key");
    guard.disarm();
    println!("deleted: id={}", created.id);

    // Postcondition: the account is back to exactly the seven expected
    // keys -- proving this run left nothing behind, on the successful
    // path.
    let after = current_names(&client);
    assert_eq!(
        after,
        expected_names_sorted(),
        "the account must hold exactly the seven expected keys after cleanup (found: {after:?})"
    );
    println!("postcondition: exactly the seven expected keys, nothing left behind: pass");
}
