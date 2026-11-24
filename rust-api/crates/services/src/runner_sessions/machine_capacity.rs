//! Persisted dev-machine concurrent-run limit (NEWPIDASH-398).
//!
//! The daemon reports its machine limit (`PollCapacity.active` /
//! `limit`) on every poll of a held run and in
//! `POST /api/v1/runner/runs/<id>/capacity/`. The matcher gate
//! ([`super::drain::machine_gate_lock_stmt`]) reads the last reported
//! value from `dev_machine.run_concurrency_limit`; this module is the
//! single writer. Only validated snapshots are persisted
//! (`1..=65535`, enforced by both callers before calling here); a
//! missing machine row (deleted mid-flight) is a no-op, never an
//! error. Writes happen only when the value changes, so steady-state
//! polls cost one locked row read and no write.
//!
//! Lock order: callers hold Runner (then Session/Run) and take the
//! machine row lock here, so the order is Runner → … → Machine —
//! never the reverse. The drains lock machines in the same direction
//! (Runner → Machine), so no cycle is possible.

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use pidash_db::entity::dev_machine;

/// The outcome of persisting one validated limit report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitChange {
    /// The stored value before this write (`None` = never reported).
    pub previous: Option<i32>,
    /// The value just stored.
    pub updated: i32,
}

/// Persist one validated machine-limit report.
///
/// Locks the machine row (`FOR UPDATE`), so concurrent persists and
/// drains serialize on it. Answers `None` when nothing changed (same
/// value already stored, or no machine row): no write happens then.
/// Otherwise writes `run_concurrency_limit` and answers the change.
///
/// `limit` arrives pre-validated (`1..=65535`) from the capacity
/// endpoint and the poll snapshot; an out-of-`i32` value is refused
/// (`None`, fail-closed) rather than truncated.
pub async fn persist_reported_limit<C: ConnectionTrait>(
    db: &C,
    machine_id: Uuid,
    limit: u64,
) -> Result<Option<LimitChange>, sea_orm::DbErr> {
    let Ok(updated) = i32::try_from(limit) else {
        return Ok(None);
    };
    let Some(machine) = dev_machine::Entity::find_by_id(machine_id)
        .lock_exclusive()
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if machine.run_concurrency_limit == Some(updated) {
        return Ok(None);
    }
    let previous = machine.run_concurrency_limit;
    dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::RunConcurrencyLimit,
            sea_orm::sea_query::Expr::value(Some(updated)),
        )
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await?;
    Ok(Some(LimitChange { previous, updated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `i32` conversion is fail-closed, never truncating.
    #[test]
    fn limit_fits_i32() {
        assert!(i32::try_from(1u64).is_ok());
        assert!(i32::try_from(65535u64).is_ok());
        assert!(i32::try_from(i32::MAX as u64).is_ok());
        assert!(i32::try_from(i32::MAX as u64 + 1).is_err());
    }

    /// Change detection is a pure comparison: same value means no
    /// write, a raised value means drain, a lowered one does not.
    #[test]
    fn limit_change_signals_raise() {
        let raised = LimitChange {
            previous: Some(2),
            updated: 5,
        };
        assert!(raised.previous.is_some_and(|p| p < raised.updated));
        let lowered = LimitChange {
            previous: Some(5),
            updated: 2,
        };
        assert!(!lowered.previous.is_some_and(|p| p < lowered.updated));
        let first = LimitChange {
            previous: None,
            updated: 3,
        };
        assert!(first.previous.is_none_or(|p| p < first.updated));
    }
}
