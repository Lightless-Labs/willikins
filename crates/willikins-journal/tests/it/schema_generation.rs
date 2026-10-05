//! The journal's published shapes -- the ones `willikins-server` (task 7)
//! returns through `rmcp::Json<T>` from `plan`, `approvals`, `runs` and
//! `run` -- generate a JSON schema and are pinned by an insta snapshot, so
//! a change to any of them is a reviewed diff rather than a silent one.
//! Mirrors `willikins-core`'s `tests/schema_generation.rs`, which keeps the
//! same guard for that crate's own MCP result types.
//!
//! `src/journal.rs`'s unit tests additionally assert, without a snapshot,
//! that every field the milestone plan's own wording names is present: a
//! snapshot catches *any* change, those catch the *specific* regression of
//! a field disappearing.

use willikins_journal::{ApprovalState, Entry, Event, PlanRecord, RunNode, RunRecord, RunState};

macro_rules! schema_snapshot {
    ($fn_name:ident, $ty:ty) => {
        #[test]
        fn $fn_name() {
            let schema = schemars::schema_for!($ty);
            insta::assert_json_snapshot!(schema);
        }
    };
}

schema_snapshot!(plan_record_schema_generates, PlanRecord);
schema_snapshot!(run_record_schema_generates, RunRecord);
schema_snapshot!(run_node_schema_generates, RunNode);
schema_snapshot!(approval_state_schema_generates, ApprovalState);
schema_snapshot!(run_state_schema_generates, RunState);

/// `Entry` and `Event` deliberately do **not** derive `JsonSchema`: the
/// journal file is not a published surface, and an `Event` carries a
/// `Redacted<T>` whose own schema is the permissive `{}` (there is nothing
/// specific left to publish once a secret port's content is gone). This
/// test exists to make that a stated decision rather than an oversight --
/// it uses both types so the intent is visible next to the snapshots
/// above, and would need editing if either ever grew a published schema.
#[test]
fn entry_and_event_are_serialized_but_not_published() {
    let entry = Entry {
        seq: 1,
        at: willikins_journal::Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap(),
        event: Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/workflows".to_string(),
            workflow_hashes: std::collections::BTreeMap::new(),
        },
    };
    let json = serde_json::to_value(&entry).expect("an Entry serializes");
    assert_eq!(json["event"]["kind"], "server_started");
}
