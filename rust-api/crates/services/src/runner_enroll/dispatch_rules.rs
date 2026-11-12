#![forbid(unsafe_code)]

//! Dispatch rules for built-in engine runners (#2903, `dispatch_rules.py`).
//!
//! Minimal port: releasing queued locality pins when an engine stops
//! taking pod work, and the pause flip effects. Drains run after
//! commit; the caller drains the returned pod/runner ids.

use sea_orm::{
    sea_query::LockType, ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, QueryFilter,
    QuerySelect,
};
use uuid::Uuid;

use pidash_db::entity::{agent_run, runner};

/// `release_locality_pins` (`dispatch_rules.py:125-145`): send QUEUED
/// `local_runner` runs pinned to `runner_ids` back to their pod (clear
/// `pinned_runner`, clear the parent's `thread_id`). Locality pins only
/// (`pin_kind = "locality"`, §12.1): dedicated runs wait (§11.3). Legacy
/// `managed_runner` runs stay pinned. Must run inside the caller's
/// transaction. Returns `(released, pod_ids)` for post-commit drains.
///
/// The candidate read locks the run rows (`select_for_update(of=
/// ("self",))` — one table, so plain `FOR UPDATE`) and the
/// `thread_id` clear mirrors the `if run.parent_run.thread_id` gate:
/// only parents whose session is actually set are written.
pub async fn release_locality_pins(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<(usize, Vec<Uuid>), DbErr> {
    if runner_ids.is_empty() {
        return Ok((0, Vec::new()));
    }
    let filter = Condition::all()
        .add(agent_run::Column::PinnedRunnerId.is_in(runner_ids.to_vec()))
        .add(agent_run::Column::Status.eq("queued"))
        .add(agent_run::Column::ExecutorKind.eq("local_runner"))
        .add(agent_run::Column::PinKind.eq("locality"));
    let affected = agent_run::Entity::find()
        .filter(filter.clone())
        .lock(LockType::Update)
        .all(db)
        .await?;
    let mut pod_ids: Vec<Uuid> = Vec::new();
    let mut parent_ids: Vec<Uuid> = Vec::new();
    for row in &affected {
        if !pod_ids.contains(&row.pod_id) {
            pod_ids.push(row.pod_id);
        }
        if let Some(parent) = row.parent_run_id {
            if !parent_ids.contains(&parent) {
                parent_ids.push(parent);
            }
        }
    }
    if !parent_ids.is_empty() {
        let parents = agent_run::Entity::find()
            .filter(agent_run::Column::Id.is_in(parent_ids))
            .all(db)
            .await?;
        let stale: Vec<Uuid> = parents
            .into_iter()
            .filter(|parent| !parent.thread_id.is_empty())
            .map(|parent| parent.id)
            .collect();
        if !stale.is_empty() {
            agent_run::Entity::update_many()
                .col_expr(
                    agent_run::Column::ThreadId,
                    sea_orm::sea_query::Expr::value("".to_owned()),
                )
                .filter(agent_run::Column::Id.is_in(stale))
                .exec(db)
                .await?;
        }
    }
    let rows = agent_run::Entity::update_many()
        .col_expr(
            agent_run::Column::PinnedRunnerId,
            sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
        )
        .filter(filter)
        .exec(db)
        .await?;
    Ok((rows.rows_affected as usize, pod_ids))
}

/// `engine_runner_ids_for_machine` (`dispatch_rules.py:148-154`).
pub async fn engine_runner_ids_for_machine(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    let rows = runner::Entity::find()
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .filter(runner::Column::Provisioning.eq("desktop_bundled"))
        .filter(runner::Column::RevokedAt.is_null())
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

/// `DevMachine.engine_paused` (`models.py`): owner pause OR auto reason.
pub fn engine_paused(engine_paused_by_owner: bool, engine_auto_pause_reason: &str) -> bool {
    engine_paused_by_owner || !engine_auto_pause_reason.is_empty()
}

/// Post-commit drains for a pause flip (`on_engine_pause_changed`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PauseEffects {
    pub drain_pod_ids: Vec<Uuid>,
    pub drain_runner_ids: Vec<Uuid>,
}

/// `on_engine_pause_changed` (`dispatch_rules.py:207-216`): pausing
/// releases the engines' pins in-transaction (drain pods after commit);
/// resuming drains each engine after commit.
pub async fn on_engine_pause_changed(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    engine_paused_by_owner: bool,
    engine_auto_pause_reason: &str,
    was_paused: bool,
) -> Result<PauseEffects, DbErr> {
    let paused = engine_paused(engine_paused_by_owner, engine_auto_pause_reason);
    let runner_ids = engine_runner_ids_for_machine(db, machine_id).await?;
    if paused && !was_paused {
        let (_, pods) = release_locality_pins(db, &runner_ids).await?;
        Ok(PauseEffects {
            drain_pod_ids: pods,
            drain_runner_ids: Vec::new(),
        })
    } else if was_paused && !paused {
        Ok(PauseEffects {
            drain_pod_ids: Vec::new(),
            drain_runner_ids: runner_ids,
        })
    } else {
        Ok(PauseEffects {
            drain_pod_ids: Vec::new(),
            drain_runner_ids: Vec::new(),
        })
    }
}
