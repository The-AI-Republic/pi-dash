#![forbid(unsafe_code)]

//! Agent workpad accessors (D-12 L2, stage 5).
//!
//! Ports the `get_workpad` (`workpad.py:74-76`) / `set_workpad`
//! (`:79-84`, `@transaction.atomic`) half of
//! `orchestration/workpad.py`. The agent system user half
//! (`get_agent_system_user`, `:44-71`) moved to
//! `pidash_services::domain::identity` (PIDASHCONV-1525).
//!
//! The workpad functions take the issue id where Python takes the
//! instance; [`set_workpad`] writes `workpad` + `updated_at` exactly
//! like `update_fields=["workpad", "updated_at"]` (the `SET` order
//! follows Django's `_meta` order per the fixture).
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

/// `set_workpad` write (`workpad.py:79-84`):
/// `update_fields=["workpad", "updated_at"]`, rendered in `_meta`
/// order per the fixture. `$1` is the stamp, `$2` the body, `$3` the
/// issue id.
pub const SET_WORKPAD_SQL: &str = "UPDATE issues SET updated_at = $1, workpad = $2 WHERE id = $3";

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

/// Overwrite the workpad body; `None`/empty clears it
/// (`body or ""`). `now` stands in for the `auto_now` stamp. A
/// missing row is a silent no-op (Python's `save()` would attempt an
/// `INSERT` fallback there — unreachable from live callers and
/// deliberately not recreated).
pub async fn set_workpad<'e, E>(
    ex: E,
    issue_id: uuid::Uuid,
    body: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SET_WORKPAD_SQL)
        .bind(now)
        .bind(body.unwrap_or(""))
        .bind(issue_id)
        .execute(ex)
        .await?;
    Ok(())
}

// NOTE: the agent system user flow (`get_agent_system_user` +
// helpers) moved to `pidash_services::domain::identity`
// (PIDASHCONV-1525); the `users` write belongs to the identity
// domain. This module keeps the workpad accessors only.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static WORKPAD_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/workpad.golden.json");

    #[test]
    fn set_workpad_sql_sets_both_update_fields() {
        // Fixture `set_update_sql[0]` renders Django's `_meta` order
        // (`updated_at` first); the bind order follows it.
        let parsed: Value = serde_json::from_str(WORKPAD_FIXTURE).expect("fixture parses");
        let rendered = parsed["set_update_sql"][0]
            .as_str()
            .expect("set_update_sql");
        assert!(rendered.starts_with("UPDATE \"issues\" SET \"updated_at\" = "));
        assert!(rendered.contains(", \"workpad\" = "));
        assert_eq!(
            SET_WORKPAD_SQL,
            "UPDATE issues SET updated_at = $1, workpad = $2 WHERE id = $3"
        );
        assert_eq!(GET_WORKPAD_SQL, "SELECT workpad FROM issues WHERE id = $1");
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const ISSUES_DDL: &str = "CREATE TEMPORARY TABLE issues (
        id UUID PRIMARY KEY, workpad TEXT, updated_at TIMESTAMPTZ NOT NULL)";

    fn live_uuid(tag: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    /// `workpad.golden.json` replay: fresh issue reads `""`, set /
    /// overwrite / clear round-trip, `None` and `NULL` read as `""`.
    #[tokio::test]
    async fn live_workpad_roundtrip_replays_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(ISSUES_DDL)
            .execute(&mut *tx)
            .await
            .expect("temp issues");
        let issue = live_uuid("000000003020");
        let epoch: chrono::DateTime<chrono::Utc> =
            "2026-10-03T11:00:00+00:00".parse().expect("epoch");
        sqlx::query("INSERT INTO issues (id, workpad, updated_at) VALUES ($1, '', $2)")
            .bind(issue)
            .bind(epoch)
            .execute(&mut *tx)
            .await
            .expect("seed issue");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get fresh"), "");
        let now: chrono::DateTime<chrono::Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        set_workpad(
            &mut *tx,
            issue,
            Some("## Agent Workpad\n\n- implementing\n"),
            now,
        )
        .await
        .expect("set");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get set"),
            "## Agent Workpad\n\n- implementing\n"
        );
        let stamp: chrono::DateTime<chrono::Utc> =
            sqlx::query("SELECT updated_at FROM issues WHERE id = $1")
                .bind(issue)
                .fetch_one(&mut *tx)
                .await
                .expect("read back")
                .try_get("updated_at")
                .expect("updated_at");
        assert_eq!(stamp, now);
        set_workpad(&mut *tx, issue, Some("second"), now)
            .await
            .expect("overwrite");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get overwrite"),
            "second"
        );
        set_workpad(&mut *tx, issue, Some(""), now)
            .await
            .expect("clear");
        assert_eq!(
            fetch_workpad(&mut *tx, issue).await.expect("get cleared"),
            ""
        );
        set_workpad(&mut *tx, issue, None, now)
            .await
            .expect("set none");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get none"), "");
        // A `NULL` workpad (unreachable on the real `NOT NULL`
        // column) still reads as `""`, like `issue.workpad or ""`.
        sqlx::query("UPDATE issues SET workpad = NULL WHERE id = $1")
            .bind(issue)
            .execute(&mut *tx)
            .await
            .expect("null out");
        assert_eq!(fetch_workpad(&mut *tx, issue).await.expect("get null"), "");
        assert!(matches!(
            fetch_workpad(&mut *tx, live_uuid("000000009999")).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }
}
