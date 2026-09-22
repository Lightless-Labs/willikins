//! The environment: [`EnvVarName`], the port type `env.get` reads its
//! variable name from.
//!
//! **Why a pattern, not a bare `String`.** `env.get` is the zero-dependency
//! resolver: no credential of its own, the root of any chain, the default
//! for an operator who has never heard of a vault (design addendum
//! `docs/plans/2026-09-11-willikins-design.md`, "Credentials are ports,
//! resolvers are nodes"). That is exactly what makes it dangerous to leave
//! unbounded — a tool that copies *any* butler process environment
//! variable into a graph secret would let a document exfiltrate `PATH`,
//! `HOME`, or a Railway/CI secret injected into the butler's own process
//! for some unrelated reason, none of which the operator meant to hand a
//! workflow. [`EnvVarName`] closes that off structurally, before
//! `env.get`'s own logic ever runs: only a name in the `WILLIKINS_`
//! namespace parses at all, so a document cannot even *express* a bind to
//! `PATH`. Every credential this workspace already reads from the process
//! environment lives in that namespace (`WILLIKINS_DOPPLER_TOKEN`,
//! `WILLIKINS_GITHUB_TOKEN`), so the bound costs nothing to an operator who
//! already follows the convention and refuses everyone else by
//! construction.
/// The name of a `WILLIKINS_`-namespaced environment variable. Never
/// secret: the *name* is not sensitive, only the value `env.get` reads
/// for it.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "WILLIKINS_[A-Z][A-Z0-9_]*",
    max_len = 128,
    description = "The name of a WILLIKINS_-namespaced environment variable.",
    example = "WILLIKINS_DOPPLER_TOKEN"
)]
pub struct EnvVarName(String);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainType;

    #[test]
    fn accepts_a_willikins_namespaced_name() {
        assert_eq!(
            EnvVarName::parse("WILLIKINS_DOPPLER_TOKEN")
                .unwrap()
                .as_str(),
            "WILLIKINS_DOPPLER_TOKEN"
        );
    }

    #[test]
    fn accepts_the_shortest_possible_name() {
        assert_eq!(
            EnvVarName::parse("WILLIKINS_A").unwrap().as_str(),
            "WILLIKINS_A"
        );
    }

    #[test]
    fn rejects_a_name_outside_the_namespace() {
        for name in ["PATH", "HOME", "AWS_SECRET_ACCESS_KEY", "GITHUB_TOKEN"] {
            assert!(
                EnvVarName::parse(name).is_err(),
                "{name} must not parse as an EnvVarName"
            );
        }
    }

    #[test]
    fn rejects_a_prefix_with_nothing_after_it() {
        assert!(EnvVarName::parse("WILLIKINS_").is_err());
    }

    #[test]
    fn rejects_a_lowercase_name() {
        assert!(EnvVarName::parse("willikins_doppler_token").is_err());
    }

    #[test]
    fn rejects_a_name_starting_with_a_digit_or_underscore_after_the_prefix() {
        assert!(EnvVarName::parse("WILLIKINS_1FOO").is_err());
        assert!(EnvVarName::parse("WILLIKINS__FOO").is_err());
    }

    #[test]
    fn rejects_a_name_merely_containing_the_prefix() {
        assert!(EnvVarName::parse("NOT_WILLIKINS_FOO").is_err());
    }

    #[test]
    fn example_parses_as_its_own_type() {
        crate::assert_example_parses::<EnvVarName>();
    }
}
