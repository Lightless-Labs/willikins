//! Helpers for `Butler::plan_rollback`/`apply_rollback` (milestone 3m task
//! R8): rebuilding one run's own [`willikins_core::rollback::RunFacts`]
//! from the raw journal, and finding whether a later run of the same
//! workflow must be rolled back first (decision 8, "why LIFO per
//! workflow").
//!
//! # Rebuilding `RunFacts` from the journal
//!
//! [`willikins_core::rollback::RunInstance`] needs each instance's own
//! `NodeStarted.at`/`inputs` and `NodeFinished.at`/`status`/`outputs` --
//! `willikins_journal::RunRecord::nodes` carries none of the former (only
//! `status`/`outputs`), and synthesizes `NodeStatus::NotRun` for an
//! instance the run never reached, which folds together "never started at
//! all" and "started but the journal write for its own finish never
//! landed" -- exactly the two cases decision 1's own table (the
//! `Unfinished` row) and [`RunProgress`]'s own doc say a caller must tell
//! apart. So [`build_run_instances`] walks `entries()` itself for this
//! `run_id`'s own `NodeStarted`/`NodeFinished` pairs, in the order the
//! run's own recorded plan `fingerprint` names them (one entry per
//! [`InstanceFingerprint`], which is already in `Plan::nodes`' own order --
//! `plan_teardown` reverses it itself, decision 6).
//!
//! A `NodeStarted`/`NodeFinished` event's own `inputs`/`outputs` are
//! `Redacted<Inputs>`/`Redacted<Outputs>` -- opaque, already-serialized
//! JSON (see `willikins_journal::redacted`'s module docs: `Value` has no
//! `Deserialize`, by design). [`decode_value_json`] reads one port's own
//! value back from that JSON directly, using the `type`/`list` fields the
//! wire shape itself always carries (`willikins_core::value::Value`'s own
//! pinned `Serialize`), rather than asking the checked document which
//! ports are secret: a *known* secret value's own rendering is already the
//! fixed `[REDACTED <Type>]` marker with `redacted: true` set (never the
//! real bytes -- `Value::render`'s own doc), so this function treats
//! `redacted: true` (or `state: "unknown"`) as "re-resolve this port
//! yourself, never from here" and produces [`Value::unknown`] -- exactly
//! decision 5's own rule ("a secret port's own value is `Value::unknown`
//! here regardless"), without needing to know in advance which ports are
//! secret at all.

use indexmap::IndexMap;

use willikins_core::rollback::{RunInstance, RunProgress};
use willikins_core::{
    Action, Inputs, InstanceFingerprint, InstanceRef, NodeName, NodeStatus, Outputs, PortName,
    StepFingerprint, Timestamp, TypeName, TypeRef, Value,
};
use willikins_journal::{Entry, Event, RunId};

/// Read one port's own recorded value back from its already-redacted JSON
/// (see the module docs). Never trusts a `redacted: true` or
/// `state: "unknown"` entry's own `value` field, even when present:
/// produces [`Value::unknown`] for either.
fn decode_value_json(json: &serde_json::Value) -> Result<Value, String> {
    let type_name = json
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "a recorded value has no `type`".to_string())?;
    let name = TypeName::parse(type_name).map_err(|error| error.to_string())?;
    let list = json
        .get("list")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let ty = if list {
        TypeRef::list_of(name)
    } else {
        TypeRef::scalar(name)
    };

    let state = json
        .get("state")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "a recorded value has no `state`".to_string())?;
    if state == "unknown" {
        return Ok(Value::unknown(ty));
    }
    if state != "known" {
        return Err(format!(
            "a recorded value has an unrecognised state `{state}`"
        ));
    }
    let redacted = json
        .get("redacted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if redacted {
        return Ok(Value::unknown(ty));
    }
    let value = json
        .get("value")
        .ok_or_else(|| "a known recorded value has no `value`".to_string())?;
    if list {
        let items = value
            .as_array()
            .ok_or_else(|| "a recorded list value's `value` is not an array".to_string())?;
        let strings = items
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or_else(|| "a recorded list element is not a string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Value::parse_list(&ty, &strings).map_err(|error| error.to_string())
    } else {
        let text = value
            .as_str()
            .ok_or_else(|| "a recorded scalar value's `value` is not a string".to_string())?;
        Value::parse(&ty, text).map_err(|error| error.to_string())
    }
}

/// Decode a whole `Redacted<Inputs>`/`Redacted<Outputs>` JSON object
/// (a port name to [`decode_value_json`]'s shape) into a live port map.
fn decode_port_map(json: &serde_json::Value) -> Result<IndexMap<PortName, Value>, String> {
    let object = json
        .as_object()
        .ok_or_else(|| "a recorded port map is not a JSON object".to_string())?;
    let mut map = IndexMap::new();
    for (key, value) in object {
        let port = PortName::parse(key).map_err(|error| error.to_string())?;
        map.insert(port, decode_value_json(value)?);
    }
    Ok(map)
}

fn decode_inputs(json: &serde_json::Value) -> Result<Inputs, String> {
    let mut inputs = Inputs::new();
    for (port, value) in decode_port_map(json)? {
        inputs.insert(port, value);
    }
    Ok(inputs)
}

fn decode_outputs(json: &serde_json::Value) -> Result<Outputs, String> {
    let mut outputs = Outputs::new();
    for (port, value) in decode_port_map(json)? {
        outputs.insert(port, value);
    }
    Ok(outputs)
}

/// This instance's own `NodeStarted` event for `run_id`, if the run ever
/// reached it: its own `at` and raw `inputs` JSON.
fn find_node_started<'a>(
    entries: &'a [Entry],
    run_id: RunId,
    node: &NodeName,
    instance: Option<&str>,
) -> Option<(Timestamp, &'a serde_json::Value)> {
    entries.iter().find_map(|entry| match &entry.event {
        Event::NodeStarted {
            run_id: r,
            node: n,
            instance: i,
            inputs,
        } if *r == run_id && n == node && i.as_deref() == instance => {
            Some((entry.at, inputs.as_json()))
        }
        _ => None,
    })
}

/// This instance's own `NodeFinished` event for `run_id`, if it finished:
/// its own `at`, `status`, and raw `outputs` JSON.
fn find_node_finished<'a>(
    entries: &'a [Entry],
    run_id: RunId,
    node: &NodeName,
    instance: Option<&str>,
) -> Option<(Timestamp, &'a NodeStatus, &'a serde_json::Value)> {
    entries.iter().find_map(|entry| match &entry.event {
        Event::NodeFinished {
            run_id: r,
            node: n,
            instance: i,
            status,
            outputs,
            ..
        } if *r == run_id && n == node && i.as_deref() == instance => {
            Some((entry.at, status, outputs.as_json()))
        }
        _ => None,
    })
}

/// Rebuild `run_id`'s own [`RunInstance`]s from the raw journal (see the
/// module docs), in `fingerprint`'s own order -- the run's recorded plan's
/// own `Plan::nodes` order, one entry per instance.
///
/// # Errors
///
/// A `String` naming the first instance whose recorded `inputs`/`outputs`
/// could not be read back -- cannot happen for a journal this crate's own
/// `append` wrote, only for one hand-edited since (the same "checked
/// rather than assumed" discipline `Butler::resolve_recorded_inputs`
/// already applies to a plan's own recorded inputs).
pub(crate) fn build_run_instances(
    entries: &[Entry],
    run_id: RunId,
    fingerprint: &[InstanceFingerprint],
) -> Result<Vec<RunInstance>, String> {
    let mut instances = Vec::with_capacity(fingerprint.len());
    for fp in fingerprint {
        let started = find_node_started(entries, run_id, &fp.name, fp.instance.as_deref());
        let finished = find_node_finished(entries, run_id, &fp.name, fp.instance.as_deref());

        let progress = match (&started, &finished) {
            (Some((started_at, _)), Some((finished_at, status, _))) => RunProgress::Finished {
                started: *started_at,
                finished: *finished_at,
                status: (*status).clone(),
            },
            (Some((started_at, _)), None) => RunProgress::NotFinished {
                started: Some(*started_at),
            },
            (None, _) => RunProgress::NotFinished { started: None },
        };
        let inputs = match &started {
            Some((_, json)) => decode_inputs(json)
                .map_err(|error| format!("{}: recorded inputs: {error}", fp.name))?,
            None => Inputs::new(),
        };
        let outputs = match &finished {
            Some((_, _, json)) => decode_outputs(json)
                .map_err(|error| format!("{}: recorded outputs: {error}", fp.name))?,
            None => Outputs::new(),
        };

        instances.push(RunInstance {
            node: fp.name.clone(),
            instance: fp.instance.clone(),
            action: fp.action,
            progress,
            inputs,
            outputs,
        });
    }
    Ok(instances)
}

/// Whether `fingerprint` (a run's own recorded plan fingerprint) shows at
/// least one instance this run actually created (milestone 3m decision 8,
/// "why LIFO per workflow": "a later run of the same document may have
/// built on what this one created"). Read narrowly, the same row decision
/// 1's own primary candidate row names: a planned [`Action::Create`] whose
/// `RunRecord` status is [`NodeStatus::Created`] -- never `Replace`/
/// `Update` (`Retained` either way, decision 1: "the prior state cannot be
/// restored"), which is "changed", not "a new resource this run added on
/// top".
pub(crate) fn run_created_something(
    fingerprint: &[InstanceFingerprint],
    nodes: &[willikins_journal::RunNode],
) -> bool {
    fingerprint.iter().any(|fp| {
        fp.action == Action::Create
            && nodes.iter().any(|node| {
                node.node == fp.name
                    && node.instance == fp.instance
                    && matches!(node.status, NodeStatus::Created)
            })
    })
}

/// The first position at which `approved` and `fresh` disagree
/// (milestone 3m task R8, decision 8's apply): a different length, or a
/// [`StepFingerprint`] that no longer compares equal. `None` when every
/// position agrees.
///
/// `willikins_core::rollback::apply_teardown` already runs this exact
/// comparison itself (it refuses `TeardownApplyError::Drift`, a unit
/// variant, the moment `fresh.as_slice() != approved`), but that error
/// carries no instance to journal the refusal against -- decision 8's own
/// `ApplyRefusedReason::Drift` needs a `node`/`instance` -- so
/// `Butler::apply_rollback` runs the same comparison itself first, purely
/// to attribute the refusal, and only then calls `apply_teardown` (which
/// re-runs it internally): the same "redundant provider reads, never a
/// redundant write" cost `Butler::apply`'s own doc already accepts for a
/// forward run's drift check.
pub(crate) fn first_step_drift(
    approved: &[StepFingerprint],
    fresh: &[StepFingerprint],
) -> Option<InstanceRef> {
    let len = approved.len().max(fresh.len());
    for idx in 0..len {
        match (approved.get(idx), fresh.get(idx)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return Some(a.instance.clone());
                }
            }
            (Some(a), None) => return Some(a.instance.clone()),
            (None, Some(b)) => return Some(b.instance.clone()),
            (None, None) => unreachable!("idx < len = max(approved.len(), fresh.len())"),
        }
    }
    None
}
