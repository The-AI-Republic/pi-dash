//! Revoke post-commit firing order (OVL-06, PIDASHCONV-1544).
//!
//! [`EffectBundle`] collects one revoke's post-commit effects in
//! Python's registration order; [`fire_plan`] flattens them into fire
//! order. Moved verbatim from the teardown executor
//! (`api::runner_enroll::teardown`), which is now a thin caller, so
//! the overlay shares the order instead of copying it.
//!
//! Re-exported through `pidash-sdk::ops`.

use uuid::Uuid;

use crate::runner_runs::LifecycleEffect;

/// One revoke's post-commit effects, in Python's registration order:
/// the per-run publish pairs (registered during S3, in run order) fire
/// first, then the handoffs, drains, and stream cleanup (`models.py`
/// `finalize_agent_run:125` + `Runner.revoke:673-685`).
#[derive(Debug)]
pub struct EffectBundle {
    /// The revoked runner (owns the bundle; unused by [`fire_plan`]).
    pub runner_id: Uuid,
    /// Per-run publish pairs, in S3 run order.
    pub publish: Vec<LifecycleEffect>,
    /// Project-move handoffs, one per cancelled run.
    pub handoffs: Vec<Uuid>,
    /// Pod drains, one per affected pod.
    pub drains: Vec<Uuid>,
    /// Stream cleanup (always present on a fresh revoke).
    pub cleanup: Option<Uuid>,
}

/// One post-commit step, in fire order.
#[derive(Debug)]
pub enum FireOp {
    /// A terminal-effects publish pair.
    Publish(LifecycleEffect),
    /// A project-move handoff.
    Handoff(Uuid),
    /// A pod drain.
    Drain(Uuid),
    /// The stream cleanup.
    Cleanup(Uuid),
}

/// Flatten one revoke's bundle into fire order: the publish pairs
/// first (registered during S3, in run order), then handoffs, drains,
/// and the stream cleanup (`models.py` `finalize_agent_run:125` +
/// `Runner.revoke:673-685`). Pure, so the order is unit-pinned.
pub fn fire_plan(bundle: EffectBundle) -> Vec<FireOp> {
    let mut plan =
        Vec::with_capacity(bundle.publish.len() + bundle.handoffs.len() + bundle.drains.len() + 1);
    plan.extend(bundle.publish.into_iter().map(FireOp::Publish));
    plan.extend(bundle.handoffs.into_iter().map(FireOp::Handoff));
    plan.extend(bundle.drains.into_iter().map(FireOp::Drain));
    plan.extend(bundle.cleanup.into_iter().map(FireOp::Cleanup));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flows_fixture() -> serde_json::Value {
        let text = include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
        serde_json::from_str(text).expect("flows.golden.json parses")
    }

    #[test]
    fn f6_fire_plan_order() {
        let flows = flows_fixture();
        let order = flows["revoke_cascade_steps"]["post_commit_hooks"]
            .as_str()
            .unwrap();
        assert!(order.starts_with("handoffs"), "{order}");
        let bundle = EffectBundle {
            runner_id: Uuid::nil(),
            publish: vec![
                LifecycleEffect::PublishTerminalEffects {
                    run_id: Uuid::nil(),
                },
                LifecycleEffect::ApplyTerminalEffectsInline {
                    run_id: Uuid::nil(),
                },
            ],
            handoffs: vec![Uuid::nil()],
            drains: vec![Uuid::nil()],
            cleanup: Some(Uuid::nil()),
        };
        let plan = fire_plan(bundle);
        assert_eq!(plan.len(), 5);
        assert!(matches!(plan[0], FireOp::Publish(_)));
        assert!(matches!(plan[1], FireOp::Publish(_)));
        assert!(matches!(plan[2], FireOp::Handoff(_)));
        assert!(matches!(plan[3], FireOp::Drain(_)));
        assert!(matches!(plan[4], FireOp::Cleanup(_)));
    }
}
