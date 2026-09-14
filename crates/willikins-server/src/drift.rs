//! Compare an approved plan's [`InstanceFingerprint`]s against a fresh
//! re-plan's, the same three-step check (identity, then action, then
//! output value) `willikins_core::apply`'s own rule 2 runs internally --
//! reimplemented here rather than reused because `check_drift` is
//! private to that module and, more fundamentally, because `Butler`
//! needs to run this check *before* spawning the run thread (a
//! pre-write refusal it can report synchronously), while the recorded
//! approval is only ever available here as the two `Vec<InstanceFingerprint>`
//! a [`willikins_journal::journal::PlanRecord`] actually carries, not as a
//! `willikins_core::Plan` (see `crate::butler`'s module docs for why).
//! The run thread's own call into `willikins_core::apply` re-runs the
//! same comparison internally as part of its own rule 2 -- redundant
//! provider reads, never redundant writes, and harmless: see
//! `crate::butler::Butler::apply`'s doc comment.

use willikins_core::{Action, InstanceFingerprint, InstanceRef, NodeName, PortName};

/// The server's own analogue of `willikins_core::apply::DriftKind`: the
/// same three shapes, but built from two rendered
/// [`InstanceFingerprint`]s rather than two live `Plan`s, so `Output`
/// carries the two *rendered strings* a fingerprint already reduced a
/// value to (already secret-safe: a secret port's rendering is always the
/// fixed `<secret>` marker, never its bytes) instead of a
/// `willikins_core::value::Value`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftDetail {
    /// The two sides do not describe the same instance at this position:
    /// a different node, a different `for_each` key, a different set of
    /// output ports, or an instance one side has and the other does not.
    Instance {
        /// The instance the approved plan has here, if any.
        planned: Option<InstanceRef>,
        /// The instance the fresh re-plan has here, if any.
        observed: Option<InstanceRef>,
    },
    /// The instance agrees, but its planned action does not.
    Action {
        /// What was approved.
        planned: Action,
        /// What a fresh re-plan says now.
        observed: Action,
    },
    /// The instance and its action agree, but a non-secret output's
    /// rendered value has changed.
    Output {
        /// The output port whose rendering changed.
        port: PortName,
        /// What was approved.
        planned: String,
        /// What a fresh re-plan observes now.
        observed: String,
    },
}

/// The result of comparing two fingerprints at the same position: either
/// they agree, or the first disagreement found, attributed to the node
/// (and instance, if any) it concerns.
pub struct Drifted {
    /// The node whose instance drifted.
    pub node: NodeName,
    /// Its `for_each` instance key, if any.
    pub instance: Option<String>,
    /// How.
    pub detail: DriftDetail,
}

/// Compare `approved` and `fresh`, instance by instance in order (the
/// same order `willikins_core::plan::Plan::fingerprint` produces them
/// in), and return the first disagreement, if any.
#[must_use]
pub fn first_drift(
    approved: &[InstanceFingerprint],
    fresh: &[InstanceFingerprint],
) -> Option<Drifted> {
    let len = approved.len().max(fresh.len());
    for idx in 0..len {
        match (approved.get(idx), fresh.get(idx)) {
            (Some(a), Some(b)) => {
                if a.name != b.name || a.instance != b.instance {
                    return Some(instance_drift(Some(a), Some(b)));
                }
                if a.action != b.action {
                    return Some(Drifted {
                        node: a.name.clone(),
                        instance: a.instance.clone(),
                        detail: DriftDetail::Action {
                            planned: a.action,
                            observed: b.action,
                        },
                    });
                }
                match first_output_difference(a, b) {
                    None => {}
                    Some(OutputDifference::Shape) => {
                        return Some(instance_drift(Some(a), Some(b)));
                    }
                    Some(OutputDifference::Value {
                        port,
                        planned,
                        observed,
                    }) => {
                        return Some(Drifted {
                            node: a.name.clone(),
                            instance: a.instance.clone(),
                            detail: DriftDetail::Output {
                                port,
                                planned,
                                observed,
                            },
                        });
                    }
                }
            }
            (Some(a), None) => return Some(instance_drift(Some(a), None)),
            (None, Some(b)) => return Some(instance_drift(None, Some(b))),
            (None, None) => unreachable!("idx < len = max(approved.len(), fresh.len())"),
        }
    }
    None
}

fn instance_drift(
    planned: Option<&InstanceFingerprint>,
    observed: Option<&InstanceFingerprint>,
) -> Drifted {
    fn reference(fp: &InstanceFingerprint) -> InstanceRef {
        InstanceRef {
            node: fp.name.clone(),
            instance: fp.instance.clone(),
        }
    }
    let anchor = planned
        .or(observed)
        .unwrap_or_else(|| unreachable!("a drift always has at least one side"));
    Drifted {
        node: anchor.name.clone(),
        instance: anchor.instance.clone(),
        detail: DriftDetail::Instance {
            planned: planned.map(reference),
            observed: observed.map(reference),
        },
    }
}

enum OutputDifference {
    Shape,
    Value {
        port: PortName,
        planned: String,
        observed: String,
    },
}

fn first_output_difference(
    a: &InstanceFingerprint,
    b: &InstanceFingerprint,
) -> Option<OutputDifference> {
    if a.outputs.len() != b.outputs.len() {
        return Some(OutputDifference::Shape);
    }
    for ((a_port, a_rendered), (b_port, b_rendered)) in a.outputs.iter().zip(&b.outputs) {
        if a_port != b_port {
            return Some(OutputDifference::Shape);
        }
        if a_rendered != b_rendered {
            return Some(OutputDifference::Value {
                port: a_port.clone(),
                planned: a_rendered.clone(),
                observed: b_rendered.clone(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(name: &str, action: Action, outputs: Vec<(&str, &str)>) -> InstanceFingerprint {
        InstanceFingerprint {
            name: NodeName::parse(name).unwrap(),
            instance: None,
            action,
            outputs: outputs
                .into_iter()
                .map(|(p, v)| (PortName::parse(p).unwrap(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn identical_fingerprints_have_no_drift() {
        let a = vec![fp("n", Action::Create, vec![("x", "1")])];
        assert!(first_drift(&a, &a).is_none());
    }

    #[test]
    fn a_changed_action_is_action_drift() {
        let approved = vec![fp("n", Action::Create, vec![])];
        let fresh = vec![fp("n", Action::NoOp, vec![])];
        let drift = first_drift(&approved, &fresh).unwrap();
        assert!(matches!(drift.detail, DriftDetail::Action { .. }));
    }

    #[test]
    fn a_changed_output_value_is_output_drift() {
        let approved = vec![fp("n", Action::Create, vec![("x", "1")])];
        let fresh = vec![fp("n", Action::Create, vec![("x", "2")])];
        let drift = first_drift(&approved, &fresh).unwrap();
        match drift.detail {
            DriftDetail::Output {
                port,
                planned,
                observed,
            } => {
                assert_eq!(port.as_str(), "x");
                assert_eq!(planned, "1");
                assert_eq!(observed, "2");
            }
            other => panic!("expected Output drift, got {other:?}"),
        }
    }

    #[test]
    fn a_different_node_at_the_same_position_is_instance_drift() {
        let approved = vec![fp("a", Action::Create, vec![])];
        let fresh = vec![fp("b", Action::Create, vec![])];
        let drift = first_drift(&approved, &fresh).unwrap();
        assert!(matches!(drift.detail, DriftDetail::Instance { .. }));
    }

    #[test]
    fn a_shorter_fresh_plan_is_instance_drift() {
        let approved = vec![
            fp("a", Action::Create, vec![]),
            fp("b", Action::Create, vec![]),
        ];
        let fresh = vec![fp("a", Action::Create, vec![])];
        let drift = first_drift(&approved, &fresh).unwrap();
        match drift.detail {
            DriftDetail::Instance { planned, observed } => {
                assert!(planned.is_some(), "the approved plan has this instance");
                assert!(observed.is_none(), "the fresh re-plan does not");
            }
            other => panic!("expected Instance drift, got {other:?}"),
        }
    }

    /// The other direction: a `for_each` that expanded to one instance
    /// *more* on the re-plan. Both directions matter, because the loop in
    /// [`first_drift`] walks to `max(approved.len(), fresh.len())` and
    /// each end of the comparison has its own `(Some, None)` branch.
    #[test]
    fn a_longer_fresh_plan_is_instance_drift() {
        let approved = vec![fp("a", Action::Create, vec![])];
        let fresh = vec![
            fp("a", Action::Create, vec![]),
            fp("b", Action::Create, vec![]),
        ];
        let drift = first_drift(&approved, &fresh).unwrap();
        assert_eq!(drift.node.as_str(), "b");
        match drift.detail {
            DriftDetail::Instance { planned, observed } => {
                assert!(planned.is_none(), "the approved plan lacks this instance");
                assert!(observed.is_some(), "the fresh re-plan has it");
            }
            other => panic!("expected Instance drift, got {other:?}"),
        }
    }

    /// Same node name, different `for_each` key at the same position: the
    /// instances are not the same instance, whatever their actions say.
    #[test]
    fn the_same_node_under_a_different_for_each_key_is_instance_drift() {
        let mut approved = fp("loop", Action::Create, vec![]);
        approved.instance = Some("dev".to_string());
        let mut fresh = fp("loop", Action::Create, vec![]);
        fresh.instance = Some("prd".to_string());
        let drift = first_drift(&[approved], &[fresh]).unwrap();
        assert!(matches!(drift.detail, DriftDetail::Instance { .. }));
    }
}
