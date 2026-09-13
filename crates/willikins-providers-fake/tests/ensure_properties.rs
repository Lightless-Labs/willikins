//! Property tests for the contract task 4's executor trusts: every fake
//! `ensure` reports a truthful `Ensured::changed`, and calling it twice
//! with the same inputs changes the state at most once.
//!
//! The invariant asserted on every call is stronger than "true then
//! false": `changed` must equal "the serialized state before this call
//! differs from the state after it". That subsumes the two-call pattern
//! *and* catches a collateral write — a tool that reports `changed: false`
//! while touching some other resource fails the same assertion.
//!
//! `github.actions_secret.ensure` is excluded from that rule by contract
//! (its value can never be read back, so it always writes and always
//! reports `changed: true`) and has its own property below. The two pure
//! tools are excluded too: they have no state to change.
//!
//! Every seeded *unrelated* resource's name contains a `-`, and every
//! generated name is drawn from `[a-z][a-z0-9]{0,7}`, which holds none —
//! so the unrelated seeds can never collide with the resource under test,
//! and their presence in the before/after comparison proves no fake
//! `ensure` writes outside its own key.

use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use willikins_core::{
    Ensured, Inputs, PortName, SinkToken, Tool, ToolError, ToolErrorKind, ToolName, Value,
};
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{
    ActionsSecretName, DomainType, DopplerConfig, DopplerProject, DopplerSecretValue,
    DopplerServiceToken, DopplerTokenName, EnvironmentSlug, GitHubRepo, ProjectSlug,
    RepoVisibility, SecretName, naming,
};

/// A test mints its own token; `ensure` needs one and `SinkToken::new` is
/// disallowed outside the apply executor everywhere else.
#[allow(clippy::disallowed_methods)]
fn mint() -> SinkToken {
    SinkToken::new()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a test port name is valid")
}

/// The state, serialized and normalized so two snapshots compare by
/// content alone: `FakeState`'s sets serialize as JSON arrays whose order
/// is a `HashSet`'s, so every array of strings is sorted first.
///
/// `ensure_calls` and `read_calls` are stripped before comparison: they
/// are call-count bookkeeping, not resource state, and change on every
/// call regardless of whether `changed` is true — task 4b's own contract
/// (record the call, *then* decide) means an idempotent second `ensure`
/// still bumps its counter, which this file's "`changed` says exactly
/// whether the state moved" invariant is not about.
fn snapshot(state: &Arc<Mutex<FakeState>>) -> serde_json::Value {
    let mut value =
        serde_json::to_value(&*state.lock().expect("no test panics while holding the lock"))
            .expect("FakeState serializes");
    if let serde_json::Value::Object(map) = &mut value {
        map.remove("ensure_calls");
        map.remove("read_calls");
    }
    normalize(&mut value);
    value
}

fn normalize(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                normalize(item);
            }
            items.sort_by_key(std::string::ToString::to_string);
        }
        serde_json::Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                normalize(item);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------
// generated resource names
// ---------------------------------------------------------------------

/// Every generated identifier: a letter then up to seven letters or
/// digits. No `-`, so no generated name can equal a seeded unrelated one.
const NAME_PATTERN: &str = "[a-z][a-z0-9]{0,7}";

/// One resource set the tools under test are pointed at. Built through
/// each type's own `parse`, so no test here restates a domain type's
/// grammar; a draw that some type refuses (a reserved word, say) is
/// filtered out rather than assumed valid.
#[derive(Debug, Clone)]
struct Params {
    repo: GitHubRepo,
    visibility: RepoVisibility,
    project: DopplerProject,
    environment: EnvironmentSlug,
    token_name: DopplerTokenName,
    slug: ProjectSlug,
}

impl Params {
    /// The config both `doppler.config.ensure` and
    /// `doppler.service_token.ensure` are keyed against, derived the one
    /// way the tools themselves derive it.
    fn config(&self) -> DopplerConfig {
        naming::v1::doppler_root_config(&self.project, &self.environment)
    }
}

fn params() -> impl Strategy<Value = Params> {
    (
        NAME_PATTERN,
        NAME_PATTERN,
        any::<bool>(),
        NAME_PATTERN,
        NAME_PATTERN,
        NAME_PATTERN,
        NAME_PATTERN,
    )
        .prop_filter_map(
            "every drawn name parses as its domain type",
            |(org, repo_slug, public, project, environment, token_name, slug)| {
                Some(Params {
                    repo: GitHubRepo::parse(&format!("{org}/{repo_slug}")).ok()?,
                    visibility: if public {
                        RepoVisibility::Public
                    } else {
                        RepoVisibility::Private
                    },
                    project: DopplerProject::parse(&project).ok()?,
                    environment: EnvironmentSlug::parse(&environment).ok()?,
                    token_name: DopplerTokenName::parse(&token_name).ok()?,
                    slug: ProjectSlug::parse(&slug).ok()?,
                })
            },
        )
}

// ---------------------------------------------------------------------
// the tools under test and the state they start from
// ---------------------------------------------------------------------

/// One non-pure fake tool other than `github.actions_secret.ensure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Repo,
    Project,
    Config,
    ServiceToken,
    Irreversible,
}

/// The state the resource under test starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pre {
    /// Nothing at the natural key.
    Absent,
    /// The resource exists, is ours, and matches every input.
    Present,
    /// The resource exists and is not ours.
    Foreign,
    /// The resource exists and is ours, but a non-key input differs.
    Mismatched,
}

impl Which {
    fn tool_name(self) -> ToolName {
        let name = match self {
            Self::Repo => "github.repo.ensure",
            Self::Project => "doppler.project.ensure",
            Self::Config => "doppler.config.ensure",
            Self::ServiceToken => "doppler.service_token.ensure",
            Self::Irreversible => "fake.irreversible.ensure",
        };
        ToolName::parse(name).expect("a fake tool name is valid")
    }

    fn inputs(self, params: &Params) -> Inputs {
        let mut inputs = Inputs::new();
        match self {
            Self::Repo => {
                inputs.insert(port("repo"), Value::known(params.repo.clone()));
                inputs.insert(port("visibility"), Value::known(params.visibility));
            }
            Self::Project => {
                inputs.insert(port("project"), Value::known(params.project.clone()));
            }
            Self::Config => {
                inputs.insert(port("project"), Value::known(params.project.clone()));
                inputs.insert(
                    port("environment"),
                    Value::known(params.environment.clone()),
                );
            }
            Self::ServiceToken => {
                inputs.insert(port("config"), Value::known(params.config()));
                inputs.insert(port("name"), Value::known(params.token_name.clone()));
            }
            Self::Irreversible => {
                inputs.insert(port("key"), Value::known(params.slug.clone()));
            }
        }
        inputs
    }

    /// The states this tool's resource can be in: only `github.repo.ensure`
    /// has a non-key input to mismatch, and only it and
    /// `doppler.project.ensure` record an owner at all — a config, a
    /// service token and `fake.irreversible.ensure`'s resource are
    /// membership in a set and nothing else, so no `Foreign` state is
    /// representable for them.
    fn possible_pre_states(self) -> Vec<Pre> {
        match self {
            Self::Repo => vec![Pre::Absent, Pre::Present, Pre::Foreign, Pre::Mismatched],
            Self::Project => vec![Pre::Absent, Pre::Present, Pre::Foreign],
            Self::Config | Self::ServiceToken | Self::Irreversible => {
                vec![Pre::Absent, Pre::Present]
            }
        }
    }

    /// Seed `state` so this tool's resource is in `pre`.
    fn seed(self, state: FakeState, params: &Params, pre: Pre) -> FakeState {
        match (self, pre) {
            (_, Pre::Absent) => state,
            (Self::Repo, Pre::Present) => state.with_repo(&params.repo, params.visibility, true),
            (Self::Repo, Pre::Foreign) => state.with_repo(&params.repo, params.visibility, false),
            (Self::Repo, Pre::Mismatched) => {
                state.with_repo(&params.repo, flip(params.visibility), true)
            }
            (Self::Project, Pre::Present) => state.with_doppler_project(&params.project, true),
            (Self::Project, Pre::Foreign) => state.with_doppler_project(&params.project, false),
            (Self::Config, Pre::Present) => state.with_doppler_config(&params.config()),
            (Self::ServiceToken, Pre::Present) => {
                state.with_doppler_service_token(&params.config(), &params.token_name)
            }
            (Self::Irreversible, Pre::Present) => state.with_irreversible(&params.slug),
            (which, pre) => {
                unreachable!("{which:?} has no {pre:?} state; see `possible_pre_states`")
            }
        }
    }
}

fn flip(visibility: RepoVisibility) -> RepoVisibility {
    match visibility {
        RepoVisibility::Public => RepoVisibility::Private,
        RepoVisibility::Private => RepoVisibility::Public,
    }
}

fn which() -> impl Strategy<Value = Which> {
    prop_oneof![
        Just(Which::Repo),
        Just(Which::Project),
        Just(Which::Config),
        Just(Which::ServiceToken),
        Just(Which::Irreversible),
    ]
}

/// A tool and one of the pre-states that tool can actually be in.
fn case() -> impl Strategy<Value = (Which, Pre)> {
    which().prop_flat_map(|which| {
        let states = which.possible_pre_states();
        (Just(which), proptest::sample::select(states))
    })
}

/// A distinctive secret seeded into the unrelated state: it must never
/// move, and never appear in a serialized state.
const UNRELATED_SECRET: &str = "unrelated-secret-bytes-nobody-should-see";

/// Resources no tool under test is pointed at, every one of them named
/// with a `-` no generated name can contain. Their survival across the
/// before/after comparison is what proves an `ensure` writes only at its
/// own key.
fn unrelated(state: FakeState) -> FakeState {
    let repo = GitHubRepo::parse("unrelated-org/unrelated-repo").expect("a valid repo");
    let project = DopplerProject::parse("unrelated-project").expect("a valid project");
    let environment = EnvironmentSlug::parse("dev").expect("a valid environment");
    let config = naming::v1::doppler_root_config(&project, &environment);
    state
        .with_repo(&repo, RepoVisibility::Private, true)
        .with_actions_secret(
            &repo,
            &ActionsSecretName::parse("UNRELATED_SECRET").expect("a valid secret name"),
        )
        .with_doppler_project(&project, true)
        .with_doppler_config(&config)
        .with_doppler_service_token(
            &config,
            &DopplerTokenName::parse("unrelated-token").expect("a valid token name"),
        )
        .with_doppler_secret(
            &config,
            &SecretName::parse("UNRELATED").expect("a valid secret name"),
            DopplerSecretValue::parse(UNRELATED_SECRET).expect("a valid secret value"),
        )
        .with_irreversible(&ProjectSlug::parse("unrelated-slug").expect("a valid slug"))
}

/// One `ensure` call, with the state snapshotted either side of it.
struct Call {
    result: Result<Ensured, ToolError>,
    before: serde_json::Value,
    after: serde_json::Value,
}

fn call(state: &Arc<Mutex<FakeState>>, tool: &dyn Tool, inputs: &Inputs) -> Call {
    let before = snapshot(state);
    let result = tool.ensure(inputs, &mint());
    let after = snapshot(state);
    Call {
        result,
        before,
        after,
    }
}

proptest! {
    /// `changed` is truthful on every call, `ensure` is idempotent, and a
    /// refusal writes nothing.
    #[test]
    fn ensure_is_truthful_and_idempotent((which, pre) in case(), params in params()) {
        let seeded = which.seed(unrelated(FakeState::new()), &params, pre);
        let state = Arc::new(Mutex::new(seeded));
        let fake_catalog = catalog(Arc::clone(&state));
        let tool = fake_catalog
            .get(&which.tool_name())
            .expect("every fake tool is registered");
        let inputs = which.inputs(&params);

        let first = call(&state, tool.as_ref(), &inputs);
        let second = call(&state, tool.as_ref(), &inputs);

        match pre {
            Pre::Foreign | Pre::Mismatched => {
                for attempt in [&first, &second] {
                    let err = attempt
                        .result
                        .as_ref()
                        .err()
                        .unwrap_or_else(|| panic!("{which:?} in {pre:?} must refuse to ensure"));
                    prop_assert_eq!(err.kind, ToolErrorKind::Conflict);
                    prop_assert_eq!(&attempt.before, &attempt.after, "a refusal wrote to the state");
                }
            }
            Pre::Absent | Pre::Present => {
                let first_ensured = first.result.as_ref().expect("ensure must succeed");
                let second_ensured = second.result.as_ref().expect("a second ensure must succeed");
                prop_assert_eq!(first_ensured.changed, pre == Pre::Absent);
                prop_assert!(!second_ensured.changed, "a second ensure must not change anything");
                for (attempt, ensured) in [(&first, first_ensured), (&second, second_ensured)] {
                    prop_assert_eq!(
                        ensured.changed,
                        attempt.before != attempt.after,
                        "`changed` must say exactly whether the state moved"
                    );
                }
            }
        }

        prop_assert_eq!(
            &first.after,
            &second.after,
            "a second ensure must leave the state where the first left it"
        );
        let rendered = serde_json::to_string(&second.after).expect("a snapshot serializes");
        prop_assert!(
            !rendered.contains(UNRELATED_SECRET),
            "a seeded secret's bytes reached a serialized state: {}",
            rendered
        );
    }
}

proptest! {
    /// `github.actions_secret.ensure` is the documented exception: its
    /// value can never be read back, so it writes on every call and says
    /// so. What it records is existence and nothing else.
    #[test]
    fn the_actions_secret_always_reports_changed_and_stores_only_existence(
        params in params(),
        already_seeded in any::<bool>(),
    ) {
        let name = ActionsSecretName::parse("DOPPLER_TOKEN").expect("a valid secret name");
        let value = DopplerServiceToken::parse(
            "dp.st.prd.distinctivesecretvaluedistinctivesecretvalue",
        )
        .expect("a valid service token");
        let base = unrelated(FakeState::new());
        let seeded = if already_seeded {
            base.with_actions_secret(&params.repo, &name)
        } else {
            base
        };
        let state = Arc::new(Mutex::new(seeded));
        let fake_catalog = catalog(Arc::clone(&state));
        let tool = fake_catalog
            .get(&ToolName::parse("github.actions_secret.ensure").expect("a valid tool name"))
            .expect("every fake tool is registered");

        let mut inputs = Inputs::new();
        inputs.insert(port("repo"), Value::known(params.repo.clone()));
        inputs.insert(port("name"), Value::known(name.clone()));
        inputs.insert(port("value"), Value::known(value));

        let first = call(&state, tool.as_ref(), &inputs);
        let second = call(&state, tool.as_ref(), &inputs);
        for attempt in [&first, &second] {
            let ensured = attempt.result.as_ref().expect("ensure must succeed");
            prop_assert!(
                ensured.changed,
                "an unreadable-back sink always reports changed: true"
            );
            prop_assert!(ensured.outputs.is_empty(), "this tool declares no outputs");
        }
        prop_assert_eq!(
            &first.after,
            &second.after,
            "writing the same secret twice records the same existence marker"
        );

        let locked = state.lock().expect("no test panics while holding the lock");
        prop_assert!(
            locked
                .github_actions_secrets
                .contains(&format!("{}#{}", params.repo, name)),
            "the existence marker must be recorded"
        );
        let rendered = serde_json::to_string(&*locked).expect("FakeState serializes");
        prop_assert!(
            !rendered.contains("distinctivesecretvalue"),
            "the secret's bytes reached the state: {}",
            rendered
        );
    }
}
