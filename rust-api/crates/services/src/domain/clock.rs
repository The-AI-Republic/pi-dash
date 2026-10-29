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
//! SQL text and bind orders are byte-identical to the seam copies this
//! replaces: the INSERT is [`create_ticker_sql`] translated `:name` →
//! `$n` in [`CREATE_TICKER_PARAMS`] order, the UPDATE is
//! [`SAVE_CLOCK_SQL`] translated in [`SAVE_CLOCK_PARAMS`] order (the
//! handler contract `orchestration::clock` documents; longest names
//! first so `:pending_entry` never corrupts `:pending_entry_free`).
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
//! [`CREATE_TICKER_PARAMS`]: crate::orchestration::clock::CREATE_TICKER_PARAMS
//! [`SAVE_CLOCK_SQL`]: crate::orchestration::clock::SAVE_CLOCK_SQL
//! [`SAVE_CLOCK_PARAMS`]: crate::orchestration::clock::SAVE_CLOCK_PARAMS

use chrono::{DateTime, Utc};

use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;

use crate::orchestration::clock::{
    create_ticker_sql, ClockWrite, CREATE_TICKER_PARAMS, SAVE_CLOCK_PARAMS, SAVE_CLOCK_SQL,
};

/// Translate a clock statement's `:name` placeholders to positional `$n`
/// in `params` order (first appearance). Longer names first so `:id`
/// never corrupts `:issue_id` and `:pending_entry` never corrupts
/// `:pending_entry_free` — the same translation every seam ran inline.
fn translate_clock_sql(sql: &str, params: &[&str]) -> String {
    let mut ordered: Vec<(usize, &&str)> = params.iter().enumerate().collect();
    ordered.sort_by_key(|item| std::cmp::Reverse(item.1.len()));
    let mut out = sql.to_owned();
    for (index, name) in ordered {
        out = out.replace(&format!(":{name}"), &format!("${}", index + 1));
    }
    out
}

/// Persist a reconciled ticker row: [`create_ticker_sql`] on
/// [`ClockWrite::Insert`], [`SAVE_CLOCK_SQL`] on [`ClockWrite::Update`],
/// no statement on [`ClockWrite::None`].
///
/// `ex` is the caller's executor — a pool reference or a transaction
/// the caller owns (`&mut PgConnection`, `&PgPool`, ...). `now` stamps
/// `:updated_at` on the Update path only (see the module docs); the
/// Insert path stamps the row's own `created_at`.
pub async fn apply_clock_write<'e, E>(
    ex: E,
    write: &ClockWrite,
    row: &IssueAgentTicker,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    match write {
        ClockWrite::None => Ok(()),
        // CREATE_TICKER_PARAMS order: now, created_by_id, id,
        // issue_id, next_run_at, enabled, disarm_reason, pending_entry,
        // pending_entry_free, pending_entry_actor_id,
        // pending_entry_trigger, resume_parent_run_id (`used` /
        // `granted` / `waited` / `user_disabled` are 0/0/0/FALSE
        // literals — no create-path handler changes them).
        ClockWrite::Insert => {
            sqlx::query(&translate_clock_sql(
                &create_ticker_sql(),
                CREATE_TICKER_PARAMS,
            ))
            .bind(row.created_at)
            .bind(row.created_by_id)
            .bind(row.id)
            .bind(row.issue_id)
            .bind(row.next_run_at)
            .bind(row.enabled)
            .bind(row.disarm_reason.as_str())
            .bind(row.pending_entry)
            .bind(row.pending_entry_free)
            .bind(row.pending_entry_actor_id)
            .bind(row.pending_entry_trigger.as_str())
            .bind(row.resume_parent_run_id)
            .execute(ex)
            .await?;
            Ok(())
        }
        // SAVE_CLOCK_PARAMS order: updated_at, granted, next_run_at,
        // enabled, disarm_reason, pending_entry, pending_entry_free,
        // pending_entry_actor_id, pending_entry_trigger,
        // resume_parent_run_id, id.
        ClockWrite::Update => {
            sqlx::query(&translate_clock_sql(SAVE_CLOCK_SQL, SAVE_CLOCK_PARAMS))
                .bind(now)
                .bind(row.granted)
                .bind(row.next_run_at)
                .bind(row.enabled)
                .bind(row.disarm_reason.as_str())
                .bind(row.pending_entry)
                .bind(row.pending_entry_free)
                .bind(row.pending_entry_actor_id)
                .bind(row.pending_entry_trigger.as_str())
                .bind(row.resume_parent_run_id)
                .bind(row.id)
                .execute(ex)
                .await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_write_sql_translation() {
        // `:name` → `$n` in PARAMS order, longest names first — the
        // save/create half of the jobs `clock_sql_translation` golden
        // this fn's callers replace (the lock-query half stays with
        // the seams, which still own the lock path).
        let save = translate_clock_sql(SAVE_CLOCK_SQL, SAVE_CLOCK_PARAMS);
        assert!(!save.contains(':'));
        for (index, name) in SAVE_CLOCK_PARAMS.iter().enumerate() {
            let placeholder = format!("${}", index + 1);
            assert!(
                save.contains(&placeholder),
                "missing {placeholder} for {name}"
            );
        }
        // Spot-check the bind order against the const text.
        let set = save
            .split("SET ")
            .nth(1)
            .unwrap()
            .split(" WHERE")
            .next()
            .unwrap();
        let cols: Vec<&str> = set.split(", ").collect();
        assert_eq!(cols.len(), SAVE_CLOCK_PARAMS.len() - 1);
        assert!(cols[0].starts_with("updated_at = $1"));
        assert!(save.ends_with("WHERE id = $11"));
        let create = translate_clock_sql(&create_ticker_sql(), CREATE_TICKER_PARAMS);
        assert!(!create.contains(':'));
        for (index, name) in CREATE_TICKER_PARAMS.iter().enumerate() {
            let placeholder = format!("${}", index + 1);
            assert!(
                create.contains(&placeholder),
                "missing {placeholder} for {name}"
            );
        }
    }
}
