//! The live Doppler **project member** cycle: `doppler.project_member.ensure`
//! driven through its `Tool` interface against the real sandbox workplace,
//! milestone 3h task D4. This is the only test in this crate that creates a
//! throwaway creator/CI service-account pair and a throwaway project pair,
//! and it is the live proof behind `ci_doppler_access` (W7's m6 node) and
//! verify items 3, 6, 7 and 12 of the milestone 3h plan.
//!
//! Compiled only with this crate's `live-tests` feature (its own `[[test]]`
//! entry in `Cargo.toml` carries `required-features`), so a plain
//! `cargo test --workspace` never builds it. The cycle itself is
//! `#[ignore]`d on top of that, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- the admin credential is read (through
//! [`willikins_providers_doppler::credential_from_env`]) only past that
//! gate, exactly as `tests/live_write_cycle.rs` does. The two tokens this
//! cycle itself mints (for the creator and CI service accounts) are read
//! into this process as plain `String`s -- there is no domain type to carry
//! them as, since they never cross a tool boundary -- and are swept for at
//! the end, the same discipline `tests/live_write_cycle.rs` applies to the
//! service tokens it mints.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-providers-doppler \
//!   --features live-tests --test live_project_member_cycle -j 2 -- \
//!   --ignored --nocapture
//! ```
//!
//! # The ten steps (plan: "The live member cycle")
//!
//! 1. `GET /v3/me` must name the workplace `Willikins - Test - Sandbox`, or
//!    the run refuses before touching anything. Counts the workplace's
//!    projects and service accounts, and refuses if any name already
//!    starts with [`PROBE_PREFIX`] -- a leftover from an aborted run is the
//!    operator's to remove by hand.
//! 2. Creates the creator service account (workplace role
//!    `create_enclave_project`, `team`, `service_accounts`) and the CI
//!    service account (role `no_access`, falling back to no explicit role
//!    at all if Doppler refuses the inline identifier), minting a one-hour
//!    token for each. Each **name** is registered with [`Guard`] before the
//!    request that may create it -- before anything is asserted about it,
//!    and before the account itself necessarily exists. Confirms
//!    the creator's workplace role is exactly its three permissions.
//! 3. As the creator, creates the cycle's project and a `prd_ci` branch
//!    config under `prd`. As the admin, creates a base project with an
//!    inheritable `prd` config holding one secret
//!    ([`INHERITED_SECRET_NAME`], a random value this process never
//!    prints), then makes `prd_ci` inherit it.
//! 4. As the creator (through the tool, never a mock): `read` is `Absent`,
//!    `ensure` creates the CI account as a `viewer` member on `[prd]`
//!    (`changed: true`), confirmed by an independent raw listing.
//! 5. `read` is `Present`; a second `ensure` is `changed: false`.
//! 6. As the admin, a raw `PATCH` narrows the member to `environments:
//!    ["dev"]`. `read` is then `Absent` with `updates()` true; `ensure`
//!    widens it (`changed: true`); the member lists `[dev, prd]`, proving
//!    `dev` was kept (verify item 3).
//! 7. As the admin, a raw `PATCH` raises the role to `collaborator`. `read`
//!    is `Mismatch`; `ensure` is `Conflict`; the member is byte-for-byte
//!    unchanged.
//! 8. With the CI account's *own* minted token: the `prd_ci` secrets
//!    download lists [`INHERITED_SECRET_NAME`], and a single-secret `GET`
//!    answers with a present `value.computed` -- proving a project-scoped
//!    viewer reads an inherited secret through the inheriting branch config
//!    (verify item 12). Only statuses and booleans are recorded; the
//!    secret's own bytes are never read into this process.
//! 9. Tries to create a second service account under the CI account's
//!    exact name (verify item 6). If Doppler allows it, the tool's `read`
//!    for that name must now be `Conflict` (two matches); if Doppler
//!    refuses it, a name with zero matches is confirmed `NotFound` instead,
//!    since the `Conflict` branch could not be exercised live.
//! 10. [`Guard::teardown`] deletes every project and service account this
//!     run created, confirms each project is unreadable afterward, and the
//!     workplace's counts return to step 1's.
//!
//! A final sweep (mirroring the 3c harnesses' own) checks every line this
//! process printed, plus the two minted tokens' own bytes, for anything
//! credential-shaped.
//!
//! # Trust boundary 1 and the role grammar
//!
//! Every throwaway name this cycle creates starts with [`PROBE_PREFIX`]
//! (`willikins-probe-delete-me`), and [`Guard`] deletes every one of them
//! again before the process exits (or, on a panic, tries to and says so
//! loudly if it cannot). The tool itself is never asked for anything the
//! live `doppler.project_member.ensure` cannot express: no `DELETE` on a
//! member, no request for `admin` or `owner` (refused by
//! [`DopplerProjectRole`]'s own grammar before this file is ever reached).

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value as Json;
use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{
    DOPPLER_API_BASE_URL, DopplerClient, DopplerProjectMemberEnsure, credential_from_env,
    http_client,
};
use willikins_providers_http::{Credential, Http, ProviderError};
use willikins_types::{
    DomainType, DopplerProject, DopplerProjectRole, DopplerServiceAccountName, EnvironmentSlug,
};

/// Every throwaway name this cycle creates starts with this. [`step_1`]
/// refuses to proceed if anything already does, and [`Guard`] only ever
/// deletes names it was itself told about -- this prefix is what makes a
/// leftover from an aborted run recognisable to a human, not what this
/// file greps for to decide what to delete.
const PROBE_PREFIX: &str = "willikins-probe-delete-me";

/// The three workplace permissions the creator service account is built
/// with -- the real account's intended set (SHARED VALUES, "Needs the
/// operator" item 1).
const CREATOR_PERMISSIONS: &[&str] = &["create_enclave_project", "team", "service_accounts"];

/// The one secret the base project's inheritable `prd` config holds, and
/// the name step 8 proves the CI account's own token can read through
/// `prd_ci`'s inheritance.
const INHERITED_SECRET_NAME: &str = "WILLIKINS_PROBE_INHERITED";

/// Token-shaped prefixes that must appear in nothing this cycle prints --
/// the two Doppler issues service-account tokens under
/// ([`willikins_providers_doppler::CREDENTIAL_PATTERN`]'s own two kinds),
/// the service-token prefix no tool here mints but which still must never
/// leak, and the `Authorization` scheme itself.
const CREDENTIAL_PREFIXES: &[&str] = &["dp.sa.", "dp.pt.", "dp.st.", "Bearer"];

/// `SinkToken::new` is disallowed outside the apply executor; a test opts
/// in narrowly, the convention every such test in this workspace uses.
#[allow(clippy::disallowed_methods)]
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// A port name, parsed. Every name this file passes is a literal.
fn port(name: &'static str) -> PortName {
    PortName::parse(name).expect("the cycle's port names are valid")
}

/// Whether `name` is one of this cycle's own throwaway names. Pure, and
/// exercised offline by [`has_probe_prefix_matches_only_its_own_names`].
fn has_probe_prefix(name: &str) -> bool {
    name.starts_with(PROBE_PREFIX)
}

/// Doppler's service-account token `expires_at`: one hour past `now`,
/// `%Y-%m-%dT%H:%M:%SZ`. Pure, and exercised offline by
/// [`expires_at_formats_one_hour_ahead_in_the_expected_shape`].
fn expires_at(now: DateTime<Utc>) -> String {
    (now + ChronoDuration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// The unix-second suffix every throwaway name in one run of this cycle
/// shares, so a single run's names are grep-able together and two runs
/// started a second apart never collide.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the unix epoch")
        .as_secs()
}

/// A short, non-secret-looking random suffix, folded from the wall clock
/// and this process's own id -- not cryptographic, and not meant to be:
/// the base project's inherited secret only needs a value this process
/// never printed anywhere, not one no one could ever guess.
fn random_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    nanos.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Build an `Http` against the real Doppler API, carrying `token` as a
/// bearer credential resolved in-process -- never read from an
/// environment variable this crate's own [`willikins_providers_doppler::CREDENTIAL_VAR`]
/// names, since neither the creator's nor the CI account's minted token
/// is that one. [`Credential::for_testing`] skips the format check
/// [`credential_from_env`] runs: a freshly minted service-account token's
/// exact shape is Doppler's business, not this file's.
fn http_for(token: &str) -> Http {
    Http::new(
        DOPPLER_API_BASE_URL,
        Vec::new(),
        Credential::for_testing("WILLIKINS_LIVE_MEMBER_CYCLE_TOKEN", token),
    )
}

/// Collects every line this cycle prints (so the final sweep can grep
/// them) and keeps `say` as the one place a line both reaches the
/// terminal and is remembered -- the same split
/// `tests/live_write_cycle.rs`'s `Cycle::say` makes.
struct Harness {
    sweep: Vec<String>,
}

impl Harness {
    fn new() -> Self {
        Self { sweep: Vec::new() }
    }

    fn say(&mut self, line: impl Into<String>) {
        let line = line.into();
        println!("{line}");
        self.sweep.push(line);
    }

    /// The final redaction sweep: neither minted token's bytes, nor
    /// anything credential-shaped, may appear in a line this cycle
    /// printed.
    fn sweep_for_secrets(&self, minted: &[&str]) {
        for (index, line) in self.sweep.iter().enumerate() {
            for token in minted {
                assert!(
                    !line.contains(token),
                    "printed line {index} leaked a minted token's bytes"
                );
            }
            for prefix in CREDENTIAL_PREFIXES {
                assert!(
                    !line.contains(prefix),
                    "printed line {index} contains the credential-shaped prefix `{prefix}`"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw Doppler calls this cycle needs that no tool in this crate performs.
// ---------------------------------------------------------------------------

/// `GET /v3/me`, returning the authenticated workplace's name.
fn me_workplace_name(http: &Http) -> String {
    let body = http
        .get::<Json>("/v3/me")
        .expect("step 1: GET /v3/me failed");
    body.pointer("/workplace/name")
        .and_then(Json::as_str)
        .expect("step 1: GET /v3/me carries no workplace.name")
        .to_string()
}

/// `GET /v3/projects?per_page=100`, every listed project's `name`.
fn list_project_names(http: &Http) -> Vec<String> {
    let body = http
        .get::<Json>("/v3/projects?per_page=100")
        .expect("listing projects failed");
    body.get("projects")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|project| project.get("name").and_then(Json::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Every listed service account's `(name, slug)`, from an already-fetched
/// `{"service_accounts": [...]}` body. Split out from
/// [`list_service_accounts`] so [`Guard`]'s `Drop` can resolve names to
/// slugs without the panicking `GET` that function itself does -- a
/// listing failure during cleanup must be reported, never crash the
/// unwind in progress.
fn parse_service_accounts(body: &Json) -> Vec<(String, String)> {
    body.get("service_accounts")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|account| {
                    let name = account.get("name").and_then(Json::as_str)?;
                    let slug = account.get("slug").and_then(Json::as_str)?;
                    Some((name.to_string(), slug.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /v3/workplace/service_accounts?per_page=100`, every listed
/// account's `(name, slug)`.
fn list_service_accounts(http: &Http) -> Vec<(String, String)> {
    let body = http
        .get::<Json>("/v3/workplace/service_accounts?per_page=100")
        .expect("listing service accounts failed");
    parse_service_accounts(&body)
}

/// A created-or-looked-up project's slug, tolerant of either
/// `{"project": {"slug": ...}}` or a bare `{"slug": ...}` envelope --
/// used only for the creator's own account, to find its listed
/// permissions in [`step_2`]. [`Guard`] is never given a slug this way:
/// it registers and deletes service accounts **by name**
/// ([`list_service_accounts`] resolves the current slug at delete time),
/// so a surprising create-response shape here cannot leave an
/// unregistered leftover.
fn slug_of(body: &Json, kind: &str) -> String {
    body.pointer(&format!("/{kind}/slug"))
        .and_then(Json::as_str)
        .or_else(|| body.get("slug").and_then(Json::as_str))
        .unwrap_or_else(|| panic!("the created {kind}'s response carries no slug: {body:?}"))
        .to_string()
}

/// `POST /v3/workplace/service_accounts` with the creator's inline
/// workplace role (`CREATOR_PERMISSIONS`).
fn create_creator_account(admin: &Http, name: &str) -> Json {
    let body = serde_json::json!({
        "name": name,
        "workplace_role": {"permissions": CREATOR_PERMISSIONS},
    });
    admin
        .post::<Json>("/v3/workplace/service_accounts", &body)
        .unwrap_or_else(|err| {
            panic!(
                "step 2: creating the creator account failed (status {:?})",
                err.status
            )
        })
}

/// `POST /v3/workplace/service_accounts` with role `no_access`; if that
/// answers anything but 2xx, retries with no explicit role at all (SHARED
/// VALUES / the facts table's own fallback).
fn create_ci_account(admin: &Http, name: &str) -> Json {
    let inline = serde_json::json!({"name": name, "workplace_role": {"identifier": "no_access"}});
    if let Ok(body) = admin.post::<Json>("/v3/workplace/service_accounts", &inline) {
        return body;
    }
    let fallback = serde_json::json!({"name": name});
    admin
        .post::<Json>("/v3/workplace/service_accounts", &fallback)
        .unwrap_or_else(|err| {
            panic!(
                "step 2: creating the CI account failed even with no explicit role (status {:?})",
                err.status
            )
        })
}

/// `POST /v3/workplace/service_accounts/service_account/{slug}/tokens`,
/// returning the minted token's bytes.
fn mint_token(admin: &Http, slug: &str, now: DateTime<Utc>) -> String {
    let path = format!("/v3/workplace/service_accounts/service_account/{slug}/tokens");
    let body = serde_json::json!({"name": "probe", "expires_at": expires_at(now)});
    let response = admin.post::<Json>(&path, &body).unwrap_or_else(|err| {
        panic!(
            "step 2: minting a token for `{slug}` failed (status {:?})",
            err.status
        )
    });
    response
        .get("api_key")
        .and_then(Json::as_str)
        .or_else(|| {
            response
                .pointer("/api_token/api_key")
                .and_then(Json::as_str)
        })
        .or_else(|| response.pointer("/api_token/token").and_then(Json::as_str))
        .unwrap_or_else(|| {
            panic!("step 2: the token response for `{slug}` carries no recognisable key field")
        })
        .to_string()
}

/// The sorted set of a listed service account's workplace-role
/// permissions, found by slug.
fn permissions_of(listing: &Json, slug: &str) -> BTreeSet<String> {
    listing
        .get("service_accounts")
        .and_then(Json::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|account| account.get("slug").and_then(Json::as_str) == Some(slug))
        })
        .and_then(|entry| entry.pointer("/workplace_role/permissions"))
        .and_then(Json::as_array)
        .map(|perms| {
            perms
                .iter()
                .filter_map(Json::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `POST /v3/projects` with the throwaway-project description.
fn create_project(http: &Http, name: &str) {
    let body = serde_json::json!({"name": name, "description": "probe, delete me"});
    http.post::<Json>("/v3/projects", &body)
        .unwrap_or_else(|err| panic!("creating project `{name}` failed (status {:?})", err.status));
}

/// `POST /v3/configs`, Doppler's branch-config create endpoint. `name`
/// must already carry the `<environment>_` prefix.
fn create_branch_config(http: &Http, project: &str, environment: &str, name: &str) {
    let body = serde_json::json!({"project": project, "environment": environment, "name": name});
    http.post::<Json>("/v3/configs", &body)
        .unwrap_or_else(|err| {
            panic!(
                "creating branch config `{name}` under `{project}` failed (status {:?})",
                err.status
            )
        });
}

/// `POST /v3/configs/config/inheritable` with `inheritable: true`.
fn mark_inheritable(http: &Http, project: &str, config: &str) {
    let body = serde_json::json!({"project": project, "config": config, "inheritable": true});
    http.post::<Json>("/v3/configs/config/inheritable", &body)
        .unwrap_or_else(|err| {
            panic!(
                "marking `{project}/{config}` inheritable failed (status {:?})",
                err.status
            )
        });
}

/// `POST /v3/configs/config/inherits`, replacing `config`'s whole
/// inherited set with the one base config.
fn set_inherits(http: &Http, project: &str, config: &str, base_project: &str, base_config: &str) {
    let body = serde_json::json!({
        "project": project,
        "config": config,
        "inherits": [{"project": base_project, "config": base_config}],
    });
    http.post::<Json>("/v3/configs/config/inherits", &body)
        .unwrap_or_else(|err| {
            panic!(
                "making `{project}/{config}` inherit `{base_project}/{base_config}` failed \
                 (status {:?})",
                err.status
            )
        });
}

/// `POST /v3/configs/config/secrets` with one flat `{name: value}` entry.
fn set_secret(http: &Http, project: &str, config: &str, name: &str, value: &str) {
    let body = serde_json::json!({"project": project, "config": config, "secrets": {name: value}});
    http.post::<Json>("/v3/configs/config/secrets", &body)
        .unwrap_or_else(|err| {
            panic!(
                "setting `{name}` on `{project}/{config}` failed (status {:?})",
                err.status
            )
        });
}

/// `GET /v3/projects/project/members?project=<project>&per_page=100`.
fn list_members_raw(http: &Http, project: &DopplerProject) -> Json {
    let path = format!("/v3/projects/project/members?project={project}&per_page=100");
    http.get::<Json>(&path).unwrap_or_else(|err| {
        panic!(
            "listing members of `{project}` failed (status {:?})",
            err.status
        )
    })
}

/// The one listed `service_account` member entry at `slug`, if any.
fn member_entry<'a>(members: &'a Json, slug: &str) -> Option<&'a Json> {
    members.get("members")?.as_array()?.iter().find(|member| {
        member.get("type").and_then(Json::as_str) == Some("service_account")
            && member.get("slug").and_then(Json::as_str) == Some(slug)
    })
}

fn member_role(entry: &Json) -> &str {
    entry
        .pointer("/role/identifier")
        .and_then(Json::as_str)
        .unwrap_or_default()
}

fn member_access_all(entry: &Json) -> bool {
    entry
        .get("access_all_environments")
        .and_then(Json::as_bool)
        .unwrap_or(false)
}

fn member_environments(entry: &Json) -> Vec<String> {
    let mut environments: Vec<String> = entry
        .get("environments")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    environments.sort();
    environments
}

/// A raw `PATCH` of the member at `slug` in `project`, bypassing the tool
/// entirely -- how steps 6 and 7 put the member into a state the tool
/// itself would never reach it.
fn patch_member(http: &Http, project: &DopplerProject, slug: &str, body: &Json) {
    let path =
        format!("/v3/projects/project/members/member/service_account/{slug}?project={project}");
    http.patch::<Json>(&path, body).unwrap_or_else(|err| {
        panic!(
            "raw PATCH of member `{slug}` failed (status {:?})",
            err.status
        )
    });
}

/// `doppler.project_member.ensure`'s inputs, for one read/ensure call.
fn member_inputs(
    project: &DopplerProject,
    service_account: &DopplerServiceAccountName,
    role: &DopplerProjectRole,
    environments: &[EnvironmentSlug],
) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("project"), Value::known(project.clone()));
    inputs.insert(
        port("service_account"),
        Value::known(service_account.clone()),
    );
    inputs.insert(port("role"), Value::known(role.clone()));
    inputs.insert(
        port("environments"),
        Value::known_list(environments.to_vec()),
    );
    inputs
}

/// Everything steps 4-9 need about the one member they all read, `ensure`,
/// or patch -- bundled so none of those steps' own signatures grows past
/// clippy's `too_many_arguments` bound. `ci_slug` is kept alongside
/// `ci_account` (the tool only ever sees the latter) because steps 4, 6
/// and 7 confirm the tool's own write against an independent raw listing,
/// which is addressed by slug.
struct MemberCtx {
    project: DopplerProject,
    ci_account: DopplerServiceAccountName,
    ci_slug: String,
    role: DopplerProjectRole,
    prd: EnvironmentSlug,
}

impl MemberCtx {
    fn inputs(&self) -> Inputs {
        member_inputs(
            &self.project,
            &self.ci_account,
            &self.role,
            std::slice::from_ref(&self.prd),
        )
    }
}

// ---------------------------------------------------------------------------
// The guard.
// ---------------------------------------------------------------------------

/// Deletes every project and service account this run created, on every
/// exit path -- a panic, a failed assertion, or an early return -- unless
/// [`Guard::teardown`] already did so and disarmed it. Names (never
/// slugs -- see [`slug_of`]'s own doc) are registered the moment their
/// creation succeeds, before anything is asserted about them (plan step
/// 2's own instruction), so a later assertion failure still cleans up
/// everything created so far. A service account is therefore deleted **by
/// recorded name**: its current slug is resolved from a fresh listing at
/// delete time, every matching entry at once -- step 9 may register the
/// same name twice (the CI account and, if Doppler allowed it, its
/// duplicate), and both must go.
struct Guard {
    admin: Arc<Http>,
    projects: Vec<String>,
    service_accounts: Vec<String>,
}

impl Guard {
    fn new(admin: Arc<Http>) -> Self {
        Self {
            admin,
            projects: Vec::new(),
            service_accounts: Vec::new(),
        }
    }

    fn register_project(&mut self, name: &str) {
        self.projects.push(name.to_string());
    }

    fn register_service_account(&mut self, name: &str) {
        self.service_accounts.push(name.to_string());
    }

    fn disarm(&mut self) {
        self.projects.clear();
        self.service_accounts.clear();
    }

    fn delete_project(admin: &Http, name: &str) -> Result<(), ProviderError> {
        admin.delete_with_body(
            "/v3/projects/project",
            &serde_json::json!({"project": name}),
        )
    }

    fn delete_service_account(admin: &Http, slug: &str) -> Result<(), ProviderError> {
        admin.delete(&format!(
            "/v3/workplace/service_accounts/service_account/{slug}"
        ))
    }

    /// Every currently-listed slug for `name`, usually zero or one but
    /// possibly more (step 9's duplicate).
    fn slugs_named<'a>(listing: &'a [(String, String)], name: &str) -> Vec<&'a str> {
        listing
            .iter()
            .filter(|(listed, _)| listed == name)
            .map(|(_, slug)| slug.as_str())
            .collect()
    }

    /// Step 10: delete everything registered, confirm each project is
    /// unreadable afterward, then disarm so `Drop` has nothing left to do.
    fn teardown(&mut self, harness: &mut Harness) {
        for project in self.projects.clone() {
            Self::delete_project(&self.admin, &project).unwrap_or_else(|err| {
                panic!(
                    "step 10: deleting project `{project}` failed (status {:?})",
                    err.status
                )
            });
            let gone = self
                .admin
                .get::<Json>(&format!("/v3/projects/project?project={project}"))
                .is_err();
            assert!(
                gone,
                "step 10: project `{project}` is still readable right after its own delete"
            );
        }
        let listing = list_service_accounts(&self.admin);
        for name in self.service_accounts.clone() {
            for slug in Self::slugs_named(&listing, &name) {
                Self::delete_service_account(&self.admin, slug).unwrap_or_else(|err| {
                    panic!(
                        "step 10: deleting service account `{name}` (slug `{slug}`) failed \
                         (status {:?})",
                        err.status
                    )
                });
            }
        }
        self.disarm();
        harness.say(
            "step 10 (every project and service account this run created is deleted, and \
             each project is confirmed unreadable): pass",
        );
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let already_panicking = std::thread::panicking();
        for project in &self.projects {
            match Self::delete_project(&self.admin, project) {
                Ok(()) => println!("guard: deleted project `{project}`"),
                Err(err) if err.status == Some(404) || err.status == Some(400) => {
                    println!(
                        "guard: project `{project}` was already gone (status {:?})",
                        err.status
                    );
                }
                Err(err) => {
                    println!(
                        "guard: !!! LEFTOVER PROJECT `{project}` !!! (status {:?}); delete it \
                         by hand",
                        err.status
                    );
                    assert!(
                        already_panicking,
                        "the guard could not delete project `{project}` (status {:?})",
                        err.status
                    );
                }
            }
        }
        self.drop_service_accounts(already_panicking);
    }
}

impl Guard {
    /// [`Drop`]'s own service-account half, split out so `drop` itself
    /// stays short: a listing failure here must be reported, not let
    /// `list_service_accounts`'s own panicking `GET` abort an unwind
    /// already in progress, so this resolves the listing with `.ok()`
    /// and treats a failure to list as a failure to confirm every name is
    /// gone.
    fn drop_service_accounts(&self, already_panicking: bool) {
        if self.service_accounts.is_empty() {
            // `teardown()` already disarmed this, the common case. Skip
            // the listing `GET` entirely: a transient failure on *that*
            // call must never turn a clean, already-completed teardown
            // into a red test.
            return;
        }
        let listing = self
            .admin
            .get::<Json>("/v3/workplace/service_accounts?per_page=100")
            .ok()
            .map(|body| parse_service_accounts(&body));
        let Some(listing) = listing else {
            println!(
                "guard: !!! could not list service accounts to resolve these by name: {:?} !!! \
                 check them by hand",
                self.service_accounts
            );
            assert!(
                already_panicking,
                "the guard could not list service accounts to clean up: {:?}",
                self.service_accounts
            );
            return;
        };
        for name in &self.service_accounts {
            let slugs = Self::slugs_named(&listing, name);
            if slugs.is_empty() {
                println!("guard: service account `{name}` was already gone");
                continue;
            }
            for slug in slugs {
                Self::delete_one_service_account(&self.admin, name, slug, already_panicking);
            }
        }
    }

    fn delete_one_service_account(admin: &Http, name: &str, slug: &str, already_panicking: bool) {
        match Self::delete_service_account(admin, slug) {
            Ok(()) => println!("guard: deleted service account `{name}`"),
            Err(err) if err.status == Some(404) => {
                println!("guard: service account `{name}` was already gone");
            }
            Err(err) => {
                println!(
                    "guard: !!! LEFTOVER SERVICE ACCOUNT `{name}` !!! (status {:?}); delete it \
                     by hand",
                    err.status
                );
                assert!(
                    already_panicking,
                    "the guard could not delete service account `{name}` (status {:?})",
                    err.status
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The ten steps.
// ---------------------------------------------------------------------------

const SANDBOX_WORKPLACE_NAME: &str = "Willikins - Test - Sandbox";

/// Step 1: refuse unless the token names the sandbox workplace, and
/// unless the workplace holds nothing already named with
/// [`PROBE_PREFIX`]. Returns the starting `(project count, service
/// account count)` step 10 must return to.
fn step_1(admin: &Http, harness: &mut Harness) -> (usize, usize) {
    let workplace = me_workplace_name(admin);
    assert_eq!(
        workplace, SANDBOX_WORKPLACE_NAME,
        "step 1: this harness refuses to run outside the sandbox workplace"
    );

    let projects = list_project_names(admin);
    let accounts = list_service_accounts(admin);
    for name in projects.iter().chain(accounts.iter().map(|(name, _)| name)) {
        assert!(
            !has_probe_prefix(name),
            "step 1: a leftover named `{name}` already exists; delete it by hand before \
             running this cycle"
        );
    }
    harness.say(format!(
        "step 1 (workplace is `{SANDBOX_WORKPLACE_NAME}`, no leftover probe-prefixed name, \
         starting counts {}/{}): pass",
        projects.len(),
        accounts.len()
    ));
    (projects.len(), accounts.len())
}

/// Created identities for the creator and CI service accounts: each
/// one's own minted token, and the CI account's slug (the creator's own
/// slug is only ever used, and only transiently, inside [`step_2`]
/// itself).
struct Accounts {
    creator_token: String,
    ci_slug: String,
    ci_token: String,
}

/// Step 2: create both throwaway service accounts, registering each
/// **name** with `guard` *before* the request that may create it (so a
/// create that lands and then fails some other way is still cleaned up,
/// the same ordering `live_write_cycle.rs`'s own `ProjectGuard` uses);
/// mint a one-hour token for each; confirm the creator's workplace role
/// is exactly its three permissions.
fn step_2(
    admin: &Http,
    guard: &mut Guard,
    harness: &mut Harness,
    now: DateTime<Utc>,
    unix: u64,
) -> Accounts {
    let creator_name = format!("{PROBE_PREFIX}-creator-{unix}");
    guard.register_service_account(&creator_name);
    let creator_body = create_creator_account(admin, &creator_name);
    let creator_slug = slug_of(&creator_body, "service_account");
    let creator_token = mint_token(admin, &creator_slug, now);

    let ci_name = format!("{PROBE_PREFIX}-ci-{unix}");
    guard.register_service_account(&ci_name);
    let ci_body = create_ci_account(admin, &ci_name);
    let ci_slug = slug_of(&ci_body, "service_account");
    let ci_token = mint_token(admin, &ci_slug, now);

    let listing = list_service_accounts(admin);
    let actual: BTreeSet<String> = permissions_of(
        &admin
            .get::<Json>("/v3/workplace/service_accounts?per_page=100")
            .unwrap_or_else(|err| panic!("step 2: re-listing service accounts failed: {err:?}")),
        &creator_slug,
    );
    let expected: BTreeSet<String> = CREATOR_PERMISSIONS
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        actual, expected,
        "step 2: the creator account's workplace role is not exactly its three permissions"
    );
    assert!(
        listing.iter().any(|(_, slug)| slug == &ci_slug),
        "step 2: the CI account is not listed after its own creation"
    );
    harness.say(format!(
        "step 2 (creator `{creator_name}` and CI `{ci_name}` created, each minted a token, \
         creator holds exactly its three permissions): pass"
    ));

    Accounts {
        creator_token,
        ci_slug,
        ci_token,
    }
}

/// Everything the project-creation half of step 3 produces: the two
/// project names and the parsed project identity `read`/`ensure` is
/// called against.
struct Projects {
    project_name: String,
    project: DopplerProject,
    base_project_name: String,
}

/// Step 3: as the creator, the project and its `prd_ci` branch config; as
/// the admin, a base project with an inheritable `prd` config holding one
/// secret, and `prd_ci` made to inherit it.
fn step_3(
    admin: &Http,
    creator: &Http,
    guard: &mut Guard,
    harness: &mut Harness,
    unix: u64,
) -> Projects {
    let project_name = format!("{PROBE_PREFIX}-{unix}");
    guard.register_project(&project_name);
    create_project(creator, &project_name);
    create_branch_config(creator, &project_name, "prd", "prd_ci");

    let base_project_name = format!("{PROBE_PREFIX}-base-{unix}");
    guard.register_project(&base_project_name);
    create_project(admin, &base_project_name);
    mark_inheritable(admin, &base_project_name, "prd");
    let secret_value = format!("probe-{}", random_suffix());
    set_secret(
        admin,
        &base_project_name,
        "prd",
        INHERITED_SECRET_NAME,
        &secret_value,
    );
    // "As the admin": the admin token is the one already used throughout
    // steps 4/6/7 to list and PATCH members on the creator's own project,
    // so it is the principal with visibility into both projects here too.
    // The creator's own workplace role (create_enclave_project, team,
    // service_accounts) gives it no standing to read the *base* project it
    // never created, so this must not run as the creator.
    set_inherits(admin, &project_name, "prd_ci", &base_project_name, "prd");

    let project = DopplerProject::parse(&project_name).expect("step 3: the project name parses");
    harness.say(format!(
        "step 3 (project `{project_name}` with `prd_ci`, base `{base_project_name}` with an \
         inheritable `prd` holding `{INHERITED_SECRET_NAME}`, inherited): pass"
    ));
    Projects {
        project_name,
        project,
        base_project_name,
    }
}

/// Step 4: as the creator, through the tool: `Absent`, `ensure` creates
/// (`changed: true`), confirmed by an independent raw listing.
fn step_4(tool: &DopplerProjectMemberEnsure, admin: &Http, harness: &mut Harness, ctx: &MemberCtx) {
    let inputs = ctx.inputs();
    let observation = tool
        .read(&inputs)
        .expect("step 4: read before any member exists");
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 4: expected Absent before any member exists, observed {observation:?}"
    );

    let ensured = tool
        .ensure(&inputs, &sink_token())
        .expect("step 4: ensure (create) failed");
    assert!(ensured.changed, "step 4: a create reports changed: true");

    let members = list_members_raw(admin, &ctx.project);
    let entry = member_entry(&members, &ctx.ci_slug)
        .expect("step 4: the CI account is not listed as a member after ensure");
    assert_eq!(
        member_role(entry),
        "viewer",
        "step 4: the created member's role"
    );
    assert!(
        !member_access_all(entry),
        "step 4: the created member is not access_all"
    );
    assert_eq!(
        member_environments(entry),
        vec!["prd".to_string()],
        "step 4: the created member's environments"
    );
    harness.say("step 4 (Absent, create, member lists viewer/[prd]): pass".to_string());
}

/// Step 5: `read` is `Present`; a second `ensure` is `changed: false`.
fn step_5(tool: &DopplerProjectMemberEnsure, harness: &mut Harness, ctx: &MemberCtx) {
    let inputs = ctx.inputs();
    let observation = tool.read(&inputs).expect("step 5: read after create");
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 5: expected Present, observed {observation:?}"
    );
    let ensured = tool
        .ensure(&inputs, &sink_token())
        .expect("step 5: second ensure failed");
    assert!(
        !ensured.changed,
        "step 5: a converged ensure reports changed: false"
    );
    harness.say("step 5 (Present, second ensure changed: false): pass".to_string());
}

/// Step 6: a raw narrowing `PATCH` to `[dev]`; `read` is `Absent` with
/// `updates()` true; `ensure` widens to `[dev, prd]` (verify item 3).
fn step_6(tool: &DopplerProjectMemberEnsure, admin: &Http, harness: &mut Harness, ctx: &MemberCtx) {
    patch_member(
        admin,
        &ctx.project,
        &ctx.ci_slug,
        &serde_json::json!({"role": "viewer", "environments": ["dev"]}),
    );

    let inputs = ctx.inputs();
    let observation = tool
        .read(&inputs)
        .expect("step 6: read after the raw narrowing patch");
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 6: expected Absent after narrowing, observed {observation:?}"
    );
    let needs_update = tool.updates(&inputs).expect("step 6: updates() failed");
    assert!(
        needs_update,
        "step 6: updates() must be true after the narrowing patch"
    );

    let ensured = tool
        .ensure(&inputs, &sink_token())
        .expect("step 6: ensure (update) failed");
    assert!(
        ensured.changed,
        "step 6: the widening ensure reports changed: true"
    );

    let members = list_members_raw(admin, &ctx.project);
    let entry =
        member_entry(&members, &ctx.ci_slug).expect("step 6: the member is missing after ensure");
    assert_eq!(
        member_environments(entry),
        vec!["dev".to_string(), "prd".to_string()],
        "step 6: dev must be kept and prd re-added"
    );
    harness
        .say("step 6 (narrowed to [dev], Absent+updates, widened to [dev, prd]): pass".to_string());
}

/// Step 7: a raw role raise to `collaborator`; `read` is `Mismatch`;
/// `ensure` is `Conflict`; the member is unchanged.
fn step_7(tool: &DopplerProjectMemberEnsure, admin: &Http, harness: &mut Harness, ctx: &MemberCtx) {
    patch_member(
        admin,
        &ctx.project,
        &ctx.ci_slug,
        &serde_json::json!({"role": "collaborator"}),
    );

    let before = list_members_raw(admin, &ctx.project);
    let before_entry = member_entry(&before, &ctx.ci_slug)
        .expect("step 7: the member is missing before ensure")
        .clone();

    let inputs = ctx.inputs();
    let observation = tool
        .read(&inputs)
        .expect("step 7: read after the raw role raise");
    assert!(
        matches!(observation, Observation::Mismatch { .. }),
        "step 7: expected Mismatch, observed {observation:?}"
    );
    let err = tool
        .ensure(&inputs, &sink_token())
        .expect_err("step 7: ensure on a Mismatch must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 7: a Mismatch must refuse as Conflict"
    );

    let after = list_members_raw(admin, &ctx.project);
    let after_entry =
        member_entry(&after, &ctx.ci_slug).expect("step 7: the member is missing after ensure");
    assert_eq!(
        &before_entry, after_entry,
        "step 7: the refused ensure changed the member"
    );
    harness.say(
        "step 7 (raised to collaborator, Mismatch, Conflict, member unchanged): pass".to_string(),
    );
}

/// Step 8: with the CI account's own token, the `prd_ci` secrets
/// download lists the inherited secret, and a single-secret `GET`
/// answers with a present `value.computed` (verify item 12). Statuses
/// and booleans only; the secret's own bytes are never read.
fn step_8(ci: &Http, harness: &mut Harness, ctx: &MemberCtx) {
    let project = &ctx.project;
    let download = ci.get::<Json>(&format!(
        "/v3/configs/config/secrets/download?project={project}&config=prd_ci&format=json"
    ));
    let download_ok = download.is_ok();
    let has_key = download
        .as_ref()
        .ok()
        .and_then(Json::as_object)
        .is_some_and(|map| map.contains_key(INHERITED_SECRET_NAME));
    assert!(
        download_ok && has_key,
        "step 8: the CI account's token could not read the inherited secret through \
         prd_ci's download endpoint (ok: {download_ok}, key present: {has_key}, error: {:?})",
        download.as_ref().err()
    );

    let one = ci.get::<Json>(&format!(
        "/v3/configs/config/secret?project={project}&config=prd_ci&name={INHERITED_SECRET_NAME}"
    ));
    let one_ok = one.is_ok();
    let has_value = one
        .as_ref()
        .ok()
        .and_then(|body| body.pointer("/value/computed"))
        .is_some_and(|value| !value.is_null());
    assert!(
        one_ok && has_value,
        "step 8: the CI account's token could not read {INHERITED_SECRET_NAME}'s value \
         (ok: {one_ok}, value present: {has_value})"
    );
    harness.say(format!(
        "step 8 (CI's own token: download ok={download_ok} key_present={has_key}, single-get \
         ok={one_ok} value_present={has_value}): pass"
    ));
}

/// Step 9: tries to create a second service account under the CI
/// account's exact name (verify item 6). Whichever branch Doppler takes,
/// confirms the corresponding `read` outcome the tool was never tested
/// against live before.
fn step_9(admin: &Http, tool: &DopplerProjectMemberEnsure, harness: &mut Harness, ctx: &MemberCtx) {
    let attempt = admin.post::<Json>(
        "/v3/workplace/service_accounts",
        &serde_json::json!({"name": ctx.ci_account.to_string()}),
    );
    // No `guard.register_service_account` call anywhere here: the CI
    // account's name was already registered in step 2, and the guard
    // deletes every account currently listed under a registered name, so
    // a same-named duplicate is covered without being told about it
    // twice (which would double the delete attempts and trip teardown's
    // own no-404-tolerance on the second pass).
    match attempt {
        Ok(_body) => step_9_after_2xx(admin, tool, harness, ctx),
        Err(err) => {
            step_9_notfound_branch(
                tool,
                harness,
                ctx,
                &format!("accepted=false status={:?}", err.status),
            );
        }
    }
}

/// The `POST` answered 2xx. Doppler's create can be idempotent --
/// answering 2xx while naming the *existing* account rather than making a
/// second one (the same shape `live_write_cycle.rs`'s own step 9b
/// anticipates for a duplicate project) -- so a 2xx alone does not prove
/// a duplicate now exists. Re-lists and counts before deciding which of
/// the two live branches this run actually reached.
fn step_9_after_2xx(
    admin: &Http,
    tool: &DopplerProjectMemberEnsure,
    harness: &mut Harness,
    ctx: &MemberCtx,
) {
    let ci_name = ctx.ci_account.to_string();
    let listing = list_service_accounts(admin);
    let duplicates = Guard::slugs_named(&listing, &ci_name).len();
    if duplicates >= 2 {
        let err = tool
            .read(&ctx.inputs())
            .expect_err("step 9: a duplicated name must make read refuse");
        assert_eq!(
            err.kind,
            ToolErrorKind::Conflict,
            "step 9: two same-named service accounts must read Conflict"
        );
        harness.say(
            "step 9 (duplicate service-account name accepted=true, created=true, \
             read=Conflict): pass"
                .to_string(),
        );
    } else {
        step_9_notfound_branch(
            tool,
            harness,
            ctx,
            "accepted=true but created=false (an idempotent create named the existing account)",
        );
    }
}

/// The duplicate was never actually created (either the `POST` itself was
/// refused, or it answered 2xx idempotently without making a second
/// account): a name with zero matches must read `NotFound` instead,
/// since the `Conflict` branch could not be exercised this run.
fn step_9_notfound_branch(
    tool: &DopplerProjectMemberEnsure,
    harness: &mut Harness,
    ctx: &MemberCtx,
    reason: &str,
) {
    let bogus_name = format!("{}-does-not-exist", ctx.ci_account);
    let bogus =
        DopplerServiceAccountName::parse(&bogus_name).expect("step 9: the bogus name parses");
    let inputs = member_inputs(
        &ctx.project,
        &bogus,
        &ctx.role,
        std::slice::from_ref(&ctx.prd),
    );
    let err = tool
        .read(&inputs)
        .expect_err("step 9: a name with zero matches must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::NotFound,
        "step 9: a name with no match must be NotFound"
    );
    harness.say(format!(
        "step 9 (duplicate service-account name {reason}, read=NotFound for a zero-match \
         name): pass"
    ));
}

#[test]
#[ignore = "opt-in live member cycle against the real Doppler sandbox workplace; creates and \
            deletes two projects and two-or-three service accounts. Run with \
            WILLIKINS_LIVE_TESTS=1, --features live-tests, and sandbox credentials sourced in \
            the same command"]
fn doppler_live_project_member_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let admin_credential = credential_from_env().expect("a valid sandbox Doppler admin token");
    let admin = Arc::new(http_client(admin_credential));
    let mut harness = Harness::new();

    let (projects_before, accounts_before) = step_1(&admin, &mut harness);

    let mut guard = Guard::new(Arc::clone(&admin));
    let now = Utc::now();
    let unix = unix_now();
    let accounts = step_2(&admin, &mut guard, &mut harness, now, unix);

    let creator = http_for(&accounts.creator_token);
    let ci = http_for(&accounts.ci_token);
    let projects = step_3(&admin, &creator, &mut guard, &mut harness, unix);

    let creator_client = Arc::new(DopplerClient::new(http_for(&accounts.creator_token)));
    let tool = DopplerProjectMemberEnsure::new(creator_client);

    let ci_account = DopplerServiceAccountName::parse(&format!("{PROBE_PREFIX}-ci-{unix}"))
        .expect("the CI account's name parses");
    let viewer = DopplerProjectRole::parse("viewer").expect("`viewer` parses");
    let prd = EnvironmentSlug::parse("prd").expect("`prd` parses");
    let ctx = MemberCtx {
        project: projects.project.clone(),
        ci_account,
        ci_slug: accounts.ci_slug.clone(),
        role: viewer,
        prd,
    };

    step_4(&tool, &admin, &mut harness, &ctx);
    step_5(&tool, &mut harness, &ctx);
    step_6(&tool, &admin, &mut harness, &ctx);
    step_7(&tool, &admin, &mut harness, &ctx);
    step_8(&ci, &mut harness, &ctx);
    step_9(&admin, &tool, &mut harness, &ctx);

    guard.teardown(&mut harness);
    let projects_after = list_project_names(&admin).len();
    let accounts_after = list_service_accounts(&admin).len();
    assert_eq!(
        (projects_after, accounts_after),
        (projects_before, accounts_before),
        "step 10: the workplace's counts did not return to step 1's"
    );

    harness.say(format!(
        "counts restored: projects {projects_after}=={projects_before}, service accounts \
         {accounts_after}=={accounts_before}"
    ));
    harness.sweep_for_secrets(&[accounts.creator_token.as_str(), accounts.ci_token.as_str()]);
    println!(
        "redaction sweep over {} printed lines: pass (project `{}`, base `{}`)",
        harness.sweep.len(),
        projects.project_name,
        projects.base_project_name
    );
}

#[test]
fn expires_at_formats_one_hour_ahead_in_the_expected_shape() {
    let now: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
        .expect("a fixed instant parses")
        .with_timezone(&Utc);
    assert_eq!(expires_at(now), "2026-10-01T13:00:00Z");
}

#[test]
fn has_probe_prefix_matches_only_its_own_names() {
    assert!(has_probe_prefix("willikins-probe-delete-me-creator-1"));
    assert!(has_probe_prefix("willikins-probe-delete-me"));
    assert!(!has_probe_prefix("willikins-live-write-cycle"));
    assert!(!has_probe_prefix("buildkite-ci"));
}
