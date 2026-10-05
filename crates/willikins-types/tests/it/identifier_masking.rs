//! Milestone 3i, task B2, acceptance 13's masking half: for each of the
//! seven identifier types, `mask_identifier` of a valid value never parses
//! back as that type. Decision (b3) argues this holds because no identifier
//! grammar among the seven admits the `.` the mask always ends in; these
//! tests pin that argument against the real types, not just the free
//! function (`disclosure.rs`'s own unit tests already cover
//! `mask_identifier` in isolation).
//!
//! Each type gets one example-based test (its own `DomainType::example()`)
//! and one property test over generated valid values matching the type's
//! own grammar, so the claim holds for more than the one example string a
//! future edit might special-case.
//!
//! Scoped to exactly these seven, deliberately not all eight registered
//! identifier types (task B8 added `DopplerValue`, so the registry's own
//! pinned set -- `registry.rs`'s `the_identifier_set_is_exactly_these_eight_types`
//! -- is now eight). `DopplerValue` carries no `pattern` at all (`max_len`
//! only, the same shape as `Text`), so it admits the `.` `mask_identifier`
//! always appends, and a masked `DopplerValue` *can* still parse back as a
//! `DopplerValue`. That is `DopplerValue`'s own module doc's stated gap,
//! not a bug this file's property would catch — `DopplerValue`'s whole
//! point is to carry a value before a document has named what shape it
//! has, one node before the parse tool that turns it into one of the
//! seven types below (which keep their own tighter grammars, and this
//! property, unchanged).

use proptest::prelude::*;
use willikins_types::{
    AppleBundleIdId, AppleCertificateId, AppleCertificateSerial, AppleIssuerId, AppleKeyId,
    AppleProfileId, BuildkiteClusterId, DomainType, mask_identifier,
};

/// A valid UUID-shaped string: `AppleIssuerId` and `BuildkiteClusterId`'s
/// grammar, lowercase hex in five hyphen-separated groups (8-4-4-4-12).
fn valid_uuid() -> impl Strategy<Value = String> {
    "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
}

/// A valid `AppleKeyId`: uppercase alphanumeric, 2 to 32 characters.
fn valid_key_id() -> impl Strategy<Value = String> {
    "[A-Z0-9]{2,32}"
}

/// A valid opaque Apple record id (`AppleBundleIdId`, `AppleCertificateId`,
/// `AppleProfileId`): mixed-case alphanumeric, 2 to 64 characters.
fn valid_opaque_record_id() -> impl Strategy<Value = String> {
    "[A-Za-z0-9]{2,64}"
}

/// A valid `AppleCertificateSerial`: uppercase hexadecimal, 1 to 64
/// characters.
fn valid_certificate_serial() -> impl Strategy<Value = String> {
    "[0-9A-F]{1,64}"
}

macro_rules! masked_example_never_parses_back {
    ($test_name:ident, $ty:ty) => {
        #[test]
        fn $test_name() {
            let masked = mask_identifier(<$ty>::example());
            assert!(
                <$ty>::parse(&masked).is_err(),
                "{}: mask_identifier(example()) == {masked:?} parsed back as the type",
                <$ty>::TYPE_NAME
            );
        }
    };
}

masked_example_never_parses_back!(
    masked_apple_issuer_id_example_never_parses_back,
    AppleIssuerId
);
masked_example_never_parses_back!(masked_apple_key_id_example_never_parses_back, AppleKeyId);
masked_example_never_parses_back!(
    masked_apple_certificate_serial_example_never_parses_back,
    AppleCertificateSerial
);
masked_example_never_parses_back!(
    masked_apple_certificate_id_example_never_parses_back,
    AppleCertificateId
);
masked_example_never_parses_back!(
    masked_apple_bundle_id_id_example_never_parses_back,
    AppleBundleIdId
);
masked_example_never_parses_back!(
    masked_apple_profile_id_example_never_parses_back,
    AppleProfileId
);
masked_example_never_parses_back!(
    masked_buildkite_cluster_id_example_never_parses_back,
    BuildkiteClusterId
);

proptest! {
    #[test]
    fn masked_generated_apple_issuer_id_never_parses_back(value in valid_uuid()) {
        AppleIssuerId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleIssuerId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_buildkite_cluster_id_never_parses_back(value in valid_uuid()) {
        BuildkiteClusterId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(BuildkiteClusterId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_apple_key_id_never_parses_back(value in valid_key_id()) {
        AppleKeyId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleKeyId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_apple_bundle_id_id_never_parses_back(value in valid_opaque_record_id()) {
        AppleBundleIdId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleBundleIdId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_apple_certificate_id_never_parses_back(value in valid_opaque_record_id()) {
        AppleCertificateId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleCertificateId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_apple_profile_id_never_parses_back(value in valid_opaque_record_id()) {
        AppleProfileId::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleProfileId::parse(&masked).is_err());
    }

    #[test]
    fn masked_generated_apple_certificate_serial_never_parses_back(value in valid_certificate_serial()) {
        AppleCertificateSerial::parse(&value).expect("generated value must be valid");
        let masked = mask_identifier(&value);
        prop_assert!(AppleCertificateSerial::parse(&masked).is_err());
    }
}
