#![forbid(unsafe_code)]

//! Agent workpad accessors (D-12 L2, stage 5).
//!
//! Agent workpad read (`workpad.py:74-76`, D-12 L2, stage 5).
//!
//! Ports the `get_workpad` half of `orchestration/workpad.py`. The
//! agent system user half (`get_agent_system_user`, `:44-71`) moved to
//! `pidash_services::domain::identity` (PIDASHCONV-1525), and the
//! `set_workpad` write (`:79-84`, `@transaction.atomic`) moved to
//! `pidash_services::domain::issues::update_workpad_save`
//! (PIDASHCONV-1630): covered tables write only from domain/ per
//! design D7.
//!
//! [`fetch_workpad`] takes the issue id where Python takes the
//! instance.
//!
//! Fixture: `rust-api/fixtures/orchestration/fx02_reads/` (FX-ORCH-02:
//! `workpad.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use sea_orm::{ConnectionTrait, EntityTrait, FromQueryResult, QuerySelect};

use crate::entity::issues;
use crate::orm_tx;

// ---------------------------------------------------------------------------
// Workpad accessors
// ---------------------------------------------------------------------------

/// `get_workpad` read (`workpad.py:74-76`): the issue's workpad body.
/// `$1` is the issue id.
pub const GET_WORKPAD_SQL: &str = "SELECT workpad FROM issues WHERE id = $1";

#[derive(Debug, Clone, FromQueryResult)]
struct WorkpadRow {
    workpad: Option<String>,
}

/// Return the issue's workpad body, `""` when never written
/// (`issue.workpad or ""`). A missing row is `RowNotFound` — Python
/// takes a live instance, so "missing" is unreachable there.
///
/// Takes a [`ConnectionTrait`] (the pool connection or a
/// transaction-held [`TxConn`](crate::orm_tx::TxConn)).
pub async fn fetch_workpad(
    db: &impl ConnectionTrait,
    issue_id: uuid::Uuid,
) -> Result<String, sqlx::Error> {
    let row = issues::Entity::find_by_id(issue_id)
        .select_only()
        .column(issues::Column::Workpad)
        .into_model::<WorkpadRow>()
        .one(db)
        .await
        .map_err(orm_tx::into_sqlx)?;
    match row {
        None => Err(sqlx::Error::RowNotFound),
        Some(row) => Ok(row.workpad.unwrap_or_default()),
    }
}

// NOTE: the agent system user flow (`get_agent_system_user` +
// helpers, plus the own-transaction `agent_system_user_id` wrapper)
// moved to `pidash_services::domain::identity` (PIDASHCONV-1525;
// the wrapper is OVL-06 PIDASHCONV-1544); the `users` write belongs
// to the identity domain. The `set_workpad` write moved to
// `pidash_services::domain::issues::update_workpad_save`
// (PIDASHCONV-1630). This module keeps the workpad read only.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_workpad_sql_pins_read() {
        // (The write pins + the live round-trip moved with the write
        // to `domain::issues`, PIDASHCONV-1630.)
        assert_eq!(GET_WORKPAD_SQL, "SELECT workpad FROM issues WHERE id = $1");
    }
}
