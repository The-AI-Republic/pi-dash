//! `domain::clock`: the ticker [`ClockWrite`] performing fn (`scheduling.py`).
//!
//! Every ticker driver — the OSS seams and the overlay MCP agent-ticker
//! driver — reconciles through
//! [`orchestration::clock`](crate::orchestration::clock), then persists
//! the outcome inside its own transaction. Each driver runs pool-direct
//! or inside its own sqlx transaction, which a SeaORM transaction
//! cannot join (design D5), so nothing here needs a [`WriteTxn`]:
//! [`apply_clock_write`] runs the write on the caller's executor, the
//! [`super::relations`] precedent.
//!
//! The statements are SeaORM entity writes with the seam copies'
//! column values: the INSERT writes the [`create_ticker_sql`] row
//! (`used` / `granted` / `waited` 0, `user_disabled` FALSE, the untouched
//! audit stamps NULL), the UPDATE sets the [`SAVE_CLOCK_SQL`] columns
//! (the handler contract `orchestration::clock` documents).
//!
//! Stamp semantics (preserved per caller, design D6 — the drivers
//! disagree today and this consolidation changes no value):
//!
//! * Insert binds the row's own `created_at` for `:now` (which stamps
//!   both `created_at` and `updated_at`). Every create path builds the
//!   row from the reconcile `now`
//!   (`orchestration::clock::reconcile` builds via `new_ticker_row`
//!   with that `now`), so this equals the `now` argument on every
//!   Insert call.
//! * Update binds the caller's `now` for `:updated_at`. The
//!   `CreationSeam` drivers pass `Utc::now()` (now-at-write, Django's
//!   `auto_now` at `save()`); the dispatch seam, the runner disarm
//!   flow and the overlay driver pass their reconcile `now`. Both
//!   predate this fn and stay as they are.
//!
//! [`WriteTxn`]: super::txn::WriteTxn
//! [`ClockWrite`]: crate::orchestration::clock::ClockWrite
//! [`create_ticker_sql`]: crate::orchestration::clock::create_ticker_sql
//! [`SAVE_CLOCK_SQL`]: crate::orchestration::clock::SAVE_CLOCK_SQL

use chrono::{DateTime, Utc};
use pidash_db::entity::issue_agent_ticker;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};

use crate::orchestration::clock::ClockWrite;

/// The [`ClockWrite::Insert`] row: `_lock_ticker`'s create shape with the
/// post-handler clock values. `created_at` stamps both `created_at` and
/// `updated_at`; `used` / `granted` / `waited` stay 0 and `user_disabled`
/// FALSE (no create-path handler changes them); `updated_by_id`,
/// `deleted_at` and `last_tick_at` stay NULL.
fn insert_row(row: &IssueAgentTicker) -> issue_agent_ticker::ActiveModel {
    let created = row.created_at.fixed_offset();
    issue_agent_ticker::ActiveModel {
        created_at: Set(created),
        updated_at: Set(created),
        created_by_id: Set(row.created_by_id),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        id: Set(row.id),
        issue_id: Set(row.issue_id),
        used: Set(0),
        granted: Set(0),
        waited: Set(0),
        user_disabled: Set(false),
        next_run_at: Set(row.next_run_at.map(|at| at.fixed_offset())),
        last_tick_at: Set(None),
        enabled: Set(row.enabled),
        disarm_reason: Set(row.disarm_reason.clone()),
        pending_entry: Set(row.pending_entry),
        pending_entry_free: Set(row.pending_entry_free),
        pending_entry_actor_id: Set(row.pending_entry_actor_id),
        pending_entry_trigger: Set(row.pending_entry_trigger.clone()),
        resume_parent_run_id: Set(row.resume_parent_run_id),
    }
}

/// The [`ClockWrite::Update`] statement (`_save_clock`): the clock
/// columns by id, `updated_at` = `now`. `used`, `waited` and
/// `user_disabled` are never written here.
fn update_stmt(
    row: &IssueAgentTicker,
    now: DateTime<Utc>,
) -> sea_orm::UpdateMany<issue_agent_ticker::Entity> {
    use issue_agent_ticker::Column;
    issue_agent_ticker::Entity::update_many()
        .col_expr(Column::UpdatedAt, Expr::value(now))
        .col_expr(Column::Granted, Expr::value(row.granted))
        .col_expr(Column::NextRunAt, Expr::value(row.next_run_at))
        .col_expr(Column::Enabled, Expr::value(row.enabled))
        .col_expr(Column::DisarmReason, Expr::value(row.disarm_reason.clone()))
        .col_expr(Column::PendingEntry, Expr::value(row.pending_entry))
        .col_expr(
            Column::PendingEntryFree,
            Expr::value(row.pending_entry_free),
        )
        .col_expr(
            Column::PendingEntryActorId,
            Expr::value(row.pending_entry_actor_id),
        )
        .col_expr(
            Column::PendingEntryTrigger,
            Expr::value(row.pending_entry_trigger.clone()),
        )
        .col_expr(
            Column::ResumeParentRunId,
            Expr::value(row.resume_parent_run_id),
        )
        .filter(Column::Id.eq(row.id))
}

/// Persist a reconciled ticker row: the create row on
/// [`ClockWrite::Insert`], the `_save_clock` UPDATE on
/// [`ClockWrite::Update`], no statement on [`ClockWrite::None`].
///
/// `db` is the caller's connection — the pool
/// (`pidash_db::pool::sea_orm_connection`) or a transaction the caller
/// owns (`pidash_db::orm_tx::TxConn`). `now` stamps `updated_at` on the
/// Update path only (see the module docs); the Insert path stamps the
/// row's own `created_at`.
pub async fn apply_clock_write(
    db: &impl ConnectionTrait,
    write: &ClockWrite,
    row: &IssueAgentTicker,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    match write {
        ClockWrite::None => Ok(()),
        ClockWrite::Insert => issue_agent_ticker::Entity::insert(insert_row(row))
            .exec_without_returning(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx),
        ClockWrite::Update => update_stmt(row, now)
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::QueryTrait;

    fn sample_row() -> IssueAgentTicker {
        let at = DateTime::parse_from_rfc3339("2026-11-05T10:00:00Z")
            .expect("rfc3339")
            .with_timezone(&Utc);
        IssueAgentTicker {
            id: uuid::Uuid::nil(),
            created_at: at,
            updated_at: at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: uuid::Uuid::nil(),
            used: 7,
            granted: 3,
            waited: 2,
            user_disabled: true,
            next_run_at: None,
            last_tick_at: None,
            enabled: true,
            disarm_reason: "r".to_owned(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: "t".to_owned(),
            resume_parent_run_id: None,
        }
    }

    #[test]
    fn clock_write_statements() {
        let row = sample_row();
        let update = update_stmt(&row, row.created_at)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        let set = update
            .split("SET ")
            .nth(1)
            .unwrap()
            .split(" WHERE")
            .next()
            .unwrap();
        let cols: Vec<&str> = set
            .split(", ")
            .map(|item| item.split(" = ").next().unwrap())
            .collect();
        assert_eq!(
            cols,
            [
                "\"updated_at\"",
                "\"granted\"",
                "\"next_run_at\"",
                "\"enabled\"",
                "\"disarm_reason\"",
                "\"pending_entry\"",
                "\"pending_entry_free\"",
                "\"pending_entry_actor_id\"",
                "\"pending_entry_trigger\"",
                "\"resume_parent_run_id\""
            ]
        );
        assert!(update.ends_with(
            "WHERE \"issue_agent_ticker\".\"id\" = '00000000-0000-0000-0000-000000000000'"
        ));
        // The create shape ignores the row's counters: they start at 0,
        // and `created_at` stamps both stamps.
        let insert = insert_row(&row);
        assert_eq!(insert.used, Set(0));
        assert_eq!(insert.granted, Set(0));
        assert_eq!(insert.waited, Set(0));
        assert_eq!(insert.user_disabled, Set(false));
        assert_eq!(insert.updated_by_id, Set(None));
        assert_eq!(insert.last_tick_at, Set(None));
        assert_eq!(insert.updated_at, Set(row.created_at.fixed_offset()));
        assert_eq!(insert.created_at, Set(row.created_at.fixed_offset()));
    }

    /// Live: the Insert row and the Update stamp round-trip through a
    /// temp copy of `issue_agent_ticker` (no constraints, dropped at
    /// commit) on one transaction.
    #[tokio::test]
    async fn live_clock_write_roundtrip() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "CREATE TEMP TABLE issue_agent_ticker ON COMMIT DROP AS \
             SELECT * FROM public.issue_agent_ticker WITH NO DATA",
        )
        .execute(&mut *tx)
        .await
        .expect("temp table");
        let mut row = sample_row();
        row.id = uuid::Uuid::new_v4();
        row.created_by_id = Some(uuid::Uuid::new_v4());
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        apply_clock_write(&conn, &ClockWrite::Insert, &row, Utc::now())
            .await
            .expect("insert");
        let stored = issue_agent_ticker::Entity::find_by_id(row.id)
            .one(&conn)
            .await
            .expect("read")
            .expect("row");
        assert_eq!((stored.used, stored.granted, stored.waited), (0, 0, 0));
        assert!(!stored.user_disabled);
        assert_eq!(stored.created_at, stored.updated_at);
        assert_eq!(stored.created_by_id, row.created_by_id);
        assert_eq!(stored.disarm_reason, "r");
        row.granted = 5;
        row.enabled = false;
        row.pending_entry = true;
        let later = row.created_at + chrono::Duration::seconds(30);
        apply_clock_write(&conn, &ClockWrite::Update, &row, later)
            .await
            .expect("update");
        apply_clock_write(&conn, &ClockWrite::None, &row, later)
            .await
            .expect("none");
        let stored = issue_agent_ticker::Entity::find_by_id(row.id)
            .one(&conn)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(stored.granted, 5);
        assert!(!stored.enabled);
        assert!(stored.pending_entry);
        assert_eq!(stored.used, 0, "Update never writes `used`");
        assert_eq!(stored.updated_at.with_timezone(&Utc), later);
        drop(conn);
        tx.rollback().await.expect("rollback");
    }
}
