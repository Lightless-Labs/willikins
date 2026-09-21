//! `SigNoz` domain types: an ingestion key's name and minted value.
//!
//! See `docs/research/2026-09-20-signoz-ingestion-keys.md`. `SigNoz`
//! documents no naming grammar for an ingestion key (its `OpenAPI`
//! description gives `name` as a bare required `string`), so
//! [`SigNozIngestionKeyName`]'s pattern is deliberately permissive rather
//! than invented: every name already in the operator's live account
//! parses under it (`infrastructure`, `pessimal-ios`,
//! `pocket-companion-prd`, `claude-pessimal-test`, `phil-connors-prd-ios`,
//! `phil-connors-prd-backend`, `Danksworth-Ingestion` — mixed case, so
//! this type does not force lowercase the way a Doppler or Buildkite slug
//! does), and so does the `willikins-<random>-delete-me` shape this
//! crate's own live tests mint. Unlike [`crate::doppler::DopplerServiceToken`],
//! this is not a credential willikins *authenticates* as, so no
//! `docs/plans/2026-09-11-willikins-design.md` invariant requires a
//! provider-published exact shape here the way it does for a Doppler or
//! GitHub token.

/// The name of a `SigNoz` ingestion key: the tool's natural key (its id is
/// only known after creation, so `name` is what a document names and
/// `read` looks up). Letters, digits, hyphen, and underscore, starting
/// with a letter or digit — matches every name already in the operator's
/// account and the `willikins-...-delete-me` shape this crate's own live
/// tests mint.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Za-z0-9][A-Za-z0-9_-]*",
    max_len = 128,
    description = "The name of a SigNoz ingestion key.",
    example = "willikins-example-key"
)]
pub struct SigNozIngestionKeyName(String);

/// A `SigNoz` ingestion key's minted value. Secret.
///
/// `SigNoz`'s `OpenAPI` description gives `value` as a bare required
/// `string` with no documented pattern or length — this crate never
/// re-reads one (see `signoz.ingestion_key.ensure`'s module docs for
/// why), so the only value this type ever parses is the one the create
/// response hands back in the same process; the bound is generous rather
/// than exact.
#[derive(willikins_derive::DomainType)]
#[domain(
    min_len = 1,
    max_len = 4096,
    secret,
    description = "A SigNoz ingestion key's minted value.",
    example = "signoz-ingestion-key-example-value-not-real"
)]
pub struct SigNozIngestionKeyValue(secrecy::SecretString);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainType;

    #[test]
    fn name_accepts_every_name_already_in_the_operators_account() {
        for name in [
            "infrastructure",
            "pessimal-ios",
            "pocket-companion-prd",
            "claude-pessimal-test",
            "phil-connors-prd-ios",
            "phil-connors-prd-backend",
            "Danksworth-Ingestion",
        ] {
            assert!(
                SigNozIngestionKeyName::parse(name).is_ok(),
                "{name} should parse"
            );
        }
    }

    #[test]
    fn name_accepts_the_live_test_delete_me_shape() {
        assert!(SigNozIngestionKeyName::parse("willikins-abc123-delete-me").is_ok());
    }

    #[test]
    fn name_rejects_a_slash_or_space() {
        assert!(SigNozIngestionKeyName::parse("a/b").is_err());
        assert!(SigNozIngestionKeyName::parse("a b").is_err());
    }

    #[test]
    fn name_rejects_empty() {
        assert!(SigNozIngestionKeyName::parse("").is_err());
    }

    #[test]
    fn value_is_secret_and_redacted() {
        let value = SigNozIngestionKeyValue::parse("a-minted-value-0123456789").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED SigNozIngestionKeyValue]");
        assert_eq!(value.to_string(), "[REDACTED SigNozIngestionKeyValue]");
    }

    #[test]
    fn value_rejects_empty() {
        assert!(SigNozIngestionKeyValue::parse("").is_err());
    }

    #[test]
    fn examples_parse() {
        crate::assert_example_parses::<SigNozIngestionKeyName>();
        crate::assert_example_parses::<SigNozIngestionKeyValue>();
    }
}
