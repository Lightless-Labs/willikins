//! `doppler.project.ensure` seeds the default root configs when it
//! creates a project, and the interaction with `doppler.config.ensure`
//! must stay truthful in both directions.
//!
//! Task 4's executor reads `Ensured::changed` to decide `Created` versus
//! `Unchanged`, so these two tools must agree about who created a config:
//! the project tool seeding `dev`, `stg` and `prd` is exactly why a
//! planned `Create` on those three can honestly finish `Unchanged` (the
//! milestone's acceptance test 5). What no test pinned before is the
//! collision: a config that already exists when the project is created.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, PortName, SinkToken, ToolName, Value};
use willikins_providers_fake::state::doppler_config_key;
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{DomainType, DopplerProject, EnvironmentSlug, naming};

/// A test mints its own token; `SinkToken::new` is disallowed elsewhere.
#[allow(clippy::disallowed_methods)]
fn mint() -> SinkToken {
    SinkToken::new()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a test port name is valid")
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).expect("a test tool name is valid")
}

fn project() -> DopplerProject {
    DopplerProject::parse("third-thoughts").expect("a valid project")
}

fn environment(name: &str) -> EnvironmentSlug {
    EnvironmentSlug::parse(name).expect("a valid environment")
}

fn project_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project()));
    inputs
}

fn config_inputs(environment_name: &str) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project()));
    inputs.insert(
        port("environment"),
        Value::known(environment(environment_name)),
    );
    inputs
}

/// A config seeded before the project exists is *not* re-created by the
/// project's own seeding: the project still reports `changed: true` (it
/// created the project), the config is still there exactly once, and
/// `doppler.config.ensure` for that environment reports `changed: false`
/// because there was nothing left to create.
#[test]
fn a_config_seeded_before_the_project_is_created_stays_and_reports_changed_false() {
    let seeded_config = naming::v1::doppler_root_config(&project(), &environment("stg"));
    let state = Arc::new(Mutex::new(
        FakeState::new().with_doppler_config(&seeded_config),
    ));
    let fake_catalog = catalog(Arc::clone(&state));

    let project_tool = fake_catalog
        .get(&tool_name("doppler.project.ensure"))
        .unwrap();
    let created = project_tool.ensure(&project_inputs(), &mint()).unwrap();
    assert!(created.changed, "the project itself was created");

    let config_tool = fake_catalog
        .get(&tool_name("doppler.config.ensure"))
        .unwrap();
    let stg = config_tool.ensure(&config_inputs("stg"), &mint()).unwrap();
    assert!(
        !stg.changed,
        "a config that already existed must not be reported as created"
    );

    let locked = state.lock().unwrap();
    assert!(
        locked
            .doppler_configs
            .contains(&doppler_config_key(&seeded_config)),
        "the pre-seeded config must survive the project's own seeding"
    );
    // dev, stg and prd, and nothing else: the pre-seeded `stg` was not
    // duplicated under a second key.
    assert_eq!(locked.doppler_configs.len(), 3);
}

/// An environment outside the seeded defaults is still created fresh
/// after the project exists, and only once.
#[test]
fn an_environment_outside_the_defaults_is_created_once_after_the_project() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let fake_catalog = catalog(Arc::clone(&state));

    fake_catalog
        .get(&tool_name("doppler.project.ensure"))
        .unwrap()
        .ensure(&project_inputs(), &mint())
        .unwrap();

    let config_tool = fake_catalog
        .get(&tool_name("doppler.config.ensure"))
        .unwrap();
    let first = config_tool.ensure(&config_inputs("qa"), &mint()).unwrap();
    assert!(first.changed, "`qa` is not a default, so it is created");
    let second = config_tool.ensure(&config_inputs("qa"), &mint()).unwrap();
    assert!(!second.changed, "the second call creates nothing");
    assert_eq!(state.lock().unwrap().doppler_configs.len(), 4);
}

/// A project that already exists and is ours never re-seeds its defaults,
/// even when a caller deleted one of them: the tool's `ensure` touches
/// `doppler_configs` on the create path alone.
#[test]
fn ensure_on_an_existing_project_does_not_restore_a_missing_default_config() {
    let state = Arc::new(Mutex::new(
        FakeState::new().with_doppler_project(&project(), true),
    ));
    let fake_catalog = catalog(Arc::clone(&state));
    let ensured = fake_catalog
        .get(&tool_name("doppler.project.ensure"))
        .unwrap()
        .ensure(&project_inputs(), &mint())
        .unwrap();
    assert!(!ensured.changed);
    assert!(
        state.lock().unwrap().doppler_configs.is_empty(),
        "an already-present project must leave `doppler_configs` alone"
    );
}
