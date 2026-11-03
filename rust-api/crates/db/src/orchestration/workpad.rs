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

use sqlx::postgres::PgRow;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Workpad accessors
// ---------------------------------------------------------------------------

/// `get_workpad` read (`workpad.py:74-76`): the issue's workpad body.
/// `$1` is the issue id.
pub const GET_WORKPAD_SQL: &str = "SELECT workpad FROM issues WHERE id = $1";

/// Return the issue's workpad body, `""` when never written
/// (`issue.workpad or ""`). A missing row is `RowNotFound` — Python
/// takes a live instance, so "missing" is unreachable there.
pub async fn fetch_workpad<'e, E>(ex: E, issue_id: uuid::Uuid) -> Result<String, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(GET_WORKPAD_SQL)
        .bind(issue_id)
        .fetch_optional(ex)
        .await?;
    match row {
        None => Err(sqlx::Error::RowNotFound),
        Some(row) => {
            let body: Option<String> = row.try_get("workpad")?;
            Ok(body.unwrap_or_default())
        }
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
