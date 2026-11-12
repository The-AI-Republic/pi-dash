//! Loop per-target fire: claim, re-check, advance, dispatch (D-03).
//!
//! Ports the fire half of `pi_dash/bgtasks/loop.py`
//! (`fire_loop_target`, lines 184-226, `bind=True, max_retries=0`) via
//! the D-03 queries layer ([`pidash_db::r#loop::queries`], owned by
//! PIDASHCONV-154 — called, never re-ported), the shared predicate logic
//! ([`pidash_services::r#loop::eligibility`]), the scanner's settings and
//! scheduling helpers ([`crate::loop_scan`], owned by PIDASHCONV-156 —
//! called, never re-ported), and the dispatch half
//! ([`crate::loop_dispatch`], the sibling module of this issue).
//!
//! The two phases mirror the Python section comments:
//!
//! 1. **Claim, re-check eligibility, advance cursor** inside one
//!    transaction: SFU-claim the target, bail on a future cursor (a raced
//!    scanner fan-out already claimed it), advance `next_run_at` to next
//!    fire plus stagger, re-check eligibility freshest-wins, and on skip
//!    save the cursor with the reason — then commit.
//! 2. **Dispatch a turn** in its own transaction with no rollback
//!    ([`crate::loop_dispatch::dispatch_loop_turn`]).
//!
//! Translation notes (translate, don't redesign):
//!
//! * The claim (`bgtasks/loop.py:196-201`) is
//!   `select_for_update(of=("self",))` + `select_related("job",
//!   "workspace", "user")` + `filter(pk, deleted_at__isnull=True)`.
//!   `FOR UPDATE OF t` locks only the target row (the F-09 SFU pattern);
//!   the job join is inner (non-nullable FK, as Django emits) and the
//!   workspace/user ids ride on the target row. The compiler-visible
//!   `deleted_at IS NULL` is doubled — the default manager plus the
//!   explicit filter, as recorded for the eligibility SQL in
//!   `fixtures/loop/queries/eligible_due_targets.sql`.
//! * A `None` claim row is the `.first()` miss (`:200-203`): `False` with
//!   no write. The dispatch-side miss (`.get()` raising `DoesNotExist`)
//!   belongs to [`crate::loop_dispatch`].
//! * `now` is read once per phase (`timezone.now()` at `:204` and inside
//!   each dispatch write): the fire entry takes the firing instant and
//!   the dispatch reads its own, exactly as the two Python frames do.
//! * The eligibility re-check resolves the same four reads as the
//!   scanner's advance pass (`loop/scan.rs`): master switch, opt-out set,
//!   member role, LLM presence — then the shared fixed-precedence
//!   [`check`][pidash_services::r#loop::eligibility::check]. The member
//!   role read is duplicated from `loop/scan.rs` (private there) rather
//!   than editing that module: the fire re-check
//!   (`eligibility.py:136-148`, `Meta.ordering` under `.first()`) is a
//!   separate call site owned by this issue.
//! * `next_run_at` advance is `nxt + _stagger(...)` with
//!   `nxt=None` (exhausted RRULE) clearing the cursor to NULL
//!   (`:210-213`); the skip path additionally stamps `last_skipped_at`
//!   and the reason (`:216-222`), while the eligible path writes only
//!   the cursor (`:223`). `auto_now` fires `updated_at` on both saves —
//!   mirrored with an explicit `now()`.
//! * With `bind=True, max_retries=0` the task never asks for a retry:
//!   every settled fire (skip or dispatch) is
//!   [`crate::worker::Verdict::Ack`]; only an infrastructure failure
//!   surfaces as a handler error, spending the worker-loop budget like
//!   every other jobs-plane handler.
//!
//! Fixture: FX-LOOP-05 (`fixtures/loop/tasks/`; recorded by PIDASHCONV-151).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend};
use sea_query::{Alias, Condition, Expr, LockType, Order, Query, SelectStatement};
use sqlx::PgPool;
use uuid::Uuid;

use crate::loop_dispatch::dispatch_loop_turn;
use crate::loop_scan::{
    loop_enabled, next_fire_for_job, stagger_offset_minutes, stagger_window, FIRE_LOOP_TARGET_TASK,
};
use crate::queue::JobRow;
use crate::worker::{HandlerError, Registry, Verdict};
use crate::Error;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::r#loop::models::loop_job::{self, LoopJob};
use pidash_db::r#loop::queries;

// ---------------------------------------------------------------------------
// Fire SQL owned by this module
// ---------------------------------------------------------------------------

/// Phase-1 claim select (`bgtasks/loop.py:196-201`):
/// `select_for_update(of=("self",))` + `select_related("job",
/// "workspace", "user")` + `filter(pk=target_id,
/// deleted_at__isnull=True)` + `.first()`.
///
/// The full job row rides along so the cursor advance
/// (`_next_fire_for_job`, `:210`) and the re-check (`job.min_role`,
/// `:149` via `check`) need no second read. `FOR UPDATE OF t` locks
/// only the target row; the job join is inner (non-nullable FK).
/// `t.deleted_at IS NULL` appears twice — the soft-delete manager plus
/// the explicit filter, exactly as Django compiles it.
pub fn fire_claim_select(target_id: Uuid) -> SelectStatement {
    // The job columns keep their bare names so the queries layer can map
    // the row ([`queries::map_loop_job_row_q`], owned by PIDASHCONV-154);
    // the target columns take a `t_` prefix so no name collides.
    let t = Alias::new("t");
    let mut sel = Query::select();
    sel.expr_as(Expr::col((t.clone(), Alias::new("id"))), Alias::new("t_id"));
    sel.expr_as(
        Expr::col((t.clone(), Alias::new("workspace_id"))),
        Alias::new("t_workspace_id"),
    );
    sel.expr_as(
        Expr::col((t.clone(), Alias::new("user_id"))),
        Alias::new("t_user_id"),
    );
    sel.expr_as(
        Expr::col((t.clone(), Alias::new("next_run_at"))),
        Alias::new("t_next_run_at"),
    );
    for col in loop_job::COLUMNS {
        sel.column((Alias::new("j"), Alias::new(*col)));
    }
    sel.from_as(Alias::new("loop_targets"), t.clone())
        .join_as(
            sea_query::JoinType::InnerJoin,
            Alias::new("loop_jobs"),
            Alias::new("j"),
            Expr::col((Alias::new("j"), Alias::new("id")))
                .equals((t.clone(), Alias::new("job_id"))),
        )
        .cond_where(
            Condition::all()
                .add(Expr::col((t.clone(), Alias::new("id"))).eq(target_id))
                .add(Expr::col((t.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((t.clone(), Alias::new("deleted_at"))).is_null()),
        )
        .lock_with_tables(LockType::Update, [t]);
    sel
}

/// One fire claim row: the cursor plus the edge ids the stagger seeds on
/// plus the full job row for the advance and the re-check.
pub struct FireClaim {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub next_run_at: Option<DateTime<Utc>>,
    pub job: LoopJob,
}

/// Live membership role for the fire-time re-check
/// (`eligibility.py:136-148`, `check`'s `WorkspaceMember` read).
/// `ORDER BY created_at DESC` is the queryset's
/// `Meta.ordering = ("-created_at",)` under `.first()`; the live-edge
/// partial unique admits at most one row anyway.
///
/// Fire-owned twin of the scanner's `member_role_select` (`loop/scan.rs`):
/// same read, separate call site — this issue owns no other paths, so
/// the scanner's private helper is repeated here, not edited there.
fn member_role_select(workspace_id: Uuid, user_id: Uuid) -> SelectStatement {
    let wm = Alias::new("wm");
    let mut sel = Query::select();
    sel.column((wm.clone(), Alias::new("role")))
        .from_as(Alias::new("workspace_members"), wm.clone())
        .cond_where(
            Condition::all()
                .add(Expr::col((wm.clone(), Alias::new("workspace_id"))).eq(workspace_id))
                .add(Expr::col((wm.clone(), Alias::new("member_id"))).eq(user_id))
                .add(Expr::col((wm.clone(), Alias::new("is_active"))).eq(true))
                .add(Expr::col((wm.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((wm.clone(), Alias::new("deleted_at"))).is_null()),
        )
        .order_by((wm, Alias::new("created_at")), Order::Desc)
        .limit(1);
    sel
}

/// Live membership role for one edge, or `None` when no active row
/// (`eligibility.py:147-148`, `membership is None` → `MEMBERSHIP_GONE`).
async fn fetch_member_role(
    db: &impl ConnectionTrait,
    workspace_id: &Uuid,
    user_id: &Uuid,
) -> Result<Option<i16>, Error> {
    let row = db
        .query_one(DbBackend::Postgres.build(&member_role_select(*workspace_id, *user_id)))
        .await
        .map_err(into_sqlx)?;
    Ok(row
        .map(|r| r.try_get::<i16>("", "role").map_err(into_sqlx))
        .transpose()?)
}

/// Skip write (`bgtasks/loop.py:217-221`,
/// `update_fields=["next_run_at", "last_skipped_at", "last_skip_reason",
/// "updated_at"]`).
pub fn fire_skip_update(
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    reason: &str,
    target_id: Uuid,
) -> sea_query::UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new("loop_targets"))
        .values([
            (
                Alias::new("next_run_at"),
                Expr::val(next_run_at.map(|n| n.fixed_offset())).into(),
            ),
            (
                Alias::new("last_skipped_at"),
                Expr::val(now.fixed_offset()).into(),
            ),
            (
                Alias::new("last_skip_reason"),
                Expr::val(reason.to_owned()).into(),
            ),
            (
                Alias::new("updated_at"),
                Expr::val(now.fixed_offset()).into(),
            ),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(target_id));
    stmt
}

/// Eligible write (`bgtasks/loop.py:223`,
/// `update_fields=["next_run_at", "updated_at"]`).
pub fn fire_advance_update(
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    target_id: Uuid,
) -> sea_query::UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new("loop_targets"))
        .values([
            (
                Alias::new("next_run_at"),
                Expr::val(next_run_at.map(|n| n.fixed_offset())).into(),
            ),
            (
                Alias::new("updated_at"),
                Expr::val(now.fixed_offset()).into(),
            ),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(target_id));
    stmt
}

// ---------------------------------------------------------------------------
// Phase 1: claim, re-check eligibility, advance cursor
// ---------------------------------------------------------------------------

/// What phase 1 decided inside its commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase1 {
    /// Missing row, raced future cursor, or ineligible re-check: nothing
    /// to dispatch. (The skip branch commits its cursor+reason row first.)
    Skipped,
    /// Claimed and advanced: hand the id to phase 2.
    Claimed { target: Uuid },
}

/// Phase 1: claim under SFU and advance `next_run_at`
/// (`bgtasks/loop.py:194-223`).
///
/// Commits before returning whenever it wrote (skip row, advance); pure
/// skips return with nothing pending so the dropped transaction rolls
/// back an empty write set — observably identical to Python's early
/// `False` returns inside `transaction.atomic()`.
pub async fn phase1_claim(
    pool: &PgPool,
    target_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<Phase1, Error> {
    let mut tx = pool.begin().await?;
    let claim: Option<FireClaim> = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let row = conn
            .query_one(DbBackend::Postgres.build(&fire_claim_select(*target_id)))
            .await
            .map_err(into_sqlx)?;
        row.map(|row| {
            Ok::<_, sqlx::Error>(FireClaim {
                id: row.try_get("", "t_id").map_err(into_sqlx)?,
                workspace_id: row.try_get("", "t_workspace_id").map_err(into_sqlx)?,
                user_id: row.try_get("", "t_user_id").map_err(into_sqlx)?,
                next_run_at: row.try_get("", "t_next_run_at").map_err(into_sqlx)?,
                job: queries::map_loop_job_row_q(&row)?,
            })
        })
        .transpose()?
    };
    let Some(claim) = claim else {
        return Ok(Phase1::Skipped);
    };
    // Future cursor = raced the scanner; another fire already claimed
    // (`:205-207`). No write.
    if claim.next_run_at.is_some_and(|next| next > *now) {
        return Ok(Phase1::Skipped);
    }

    let job = &claim.job;
    let next = next_fire_for_job(job, now);
    let advanced = next.map(|nxt| {
        nxt + chrono::Duration::minutes(stagger_offset_minutes(
            &job.id,
            &claim.workspace_id,
            &claim.user_id,
            stagger_window(),
        ))
    });

    // Freshest-wins re-check under the row lock (`:215`): the same four
    // reads the scanner's advance pass resolves, in the fixed precedence
    // order shared with `eligibility.check`. The `queries` reads run on
    // the same transaction through `TxConn`; `fetch_member_role` stays
    // on the sqlx executor between them.
    let (master_paused, job_opted_out) = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let master_paused = !queries::fetch_master_enabled(&conn, claim.user_id).await?;
        let off_ids = queries::fetch_off_job_ids(&conn, claim.user_id).await?;
        (master_paused, off_ids.contains(&job.id))
    };
    let role = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        fetch_member_role(&conn, &claim.workspace_id, &claim.user_id)
            .await?
            .map(|r| r as i32)
    };
    let has_llm = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        queries::fetch_user_has_llm(&conn, claim.user_id).await?
    };
    let skip = pidash_services::r#loop::eligibility::check(
        master_paused,
        job_opted_out,
        role,
        job.min_role as i32,
        has_llm,
    );
    match skip {
        Some(reason) => {
            {
                let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
                conn.execute(DbBackend::Postgres.build(&fire_skip_update(
                    advanced,
                    *now,
                    reason.as_str(),
                    claim.id,
                )))
                .await
                .map_err(into_sqlx)?;
            }
            tx.commit().await?;
            tracing::info!(
                target = %claim.id,
                reason = reason.as_str(),
                "loop.fire: skipped, cursor advanced"
            );
            Ok(Phase1::Skipped)
        }
        None => {
            {
                let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
                conn.execute(
                    DbBackend::Postgres.build(&fire_advance_update(advanced, *now, claim.id)),
                )
                .await
                .map_err(into_sqlx)?;
            }
            tx.commit().await?;
            Ok(Phase1::Claimed { target: claim.id })
        }
    }
}

// ---------------------------------------------------------------------------
// Fire entry + registration
// ---------------------------------------------------------------------------

/// Claim one target under SFU, re-check eligibility, advance the cursor,
/// and dispatch a turn (`bgtasks/loop.py:184-226`). Returns `True` if a
/// turn was queued.
pub async fn fire_loop_target(
    pool: &PgPool,
    target_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<bool, Error> {
    // Instance-level kill switch (`bgtasks/loop.py:189-190`), owned by the
    // scanner module — called, never re-ported.
    if !loop_enabled() {
        return Ok(false);
    }

    let Phase1::Claimed { target } = phase1_claim(pool, target_id, now).await? else {
        return Ok(false);
    };

    // ----- Phase 2: dispatch a turn (own transaction; no rollback) -----
    dispatch_loop_turn(pool, &target).await
}

/// Parse the `fire_loop_target.delay(str(target_id))` wire args back into
/// a target id. Shared with the dispatch module's twin for the registry
/// contract test below.
pub fn fire_target_id_from_job(job: &JobRow) -> Result<Uuid, String> {
    crate::loop_dispatch::target_id_from_job(job)
}

/// Register the local fire handler: ownership of
/// `pi_dash.bgtasks.loop.fire_loop_target` flips from the Python plane to
/// Rust the moment this runs (see [`crate::worker::route_for`]). The pool
/// is captured because [`crate::worker::Handler`] receives only the
/// claimed row; each fire uses the firing instant as `now`.
pub fn register_fire(registry: &mut Registry, pool: PgPool) {
    registry.register(
        FIRE_LOOP_TARGET_TASK,
        Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let fut: Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> =
                Box::pin(async move {
                    let target_id = fire_target_id_from_job(&job)?;
                    let now = Utc::now();
                    fire_loop_target(&pool, &target_id, &now)
                        .await
                        .map(|_| Verdict::Ack)
                        .map_err(|error| error.to_string())
                });
            fut
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{route_for, Route};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/tasks")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn norm(sql: &str) -> String {
        sql.to_ascii_lowercase()
            .replace('"', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn parse(sql: &str) -> sqlparser::ast::Statement {
        let mut stmts =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
        assert_eq!(stmts.len(), 1);
        stmts.pop().unwrap()
    }

    fn pinned() -> Uuid {
        Uuid::from_bytes([7; 16])
    }

    fn render_stmt(stmt: &SelectStatement) -> String {
        stmt.to_string(sea_query::PostgresQueryBuilder)
    }

    fn render_update(stmt: &sea_query::UpdateStatement) -> String {
        stmt.to_string(sea_query::PostgresQueryBuilder)
    }

    // Fire claim SQL (`bgtasks/loop.py:196-201`): the target row plus the
    // full job column list over an inner join, the manager's
    // `deleted_at IS NULL` doubled (manager + explicit filter), and
    // `FOR UPDATE OF` the target table only.
    #[test]
    fn fire_claim_sql_carries_every_python_arm() {
        let sql = render_stmt(&fire_claim_select(pinned()));
        assert!(matches!(parse(&sql), sqlparser::ast::Statement::Query(_)));
        let mine = norm(&sql);
        for fragment in [
            "from loop_targets as t",
            "inner join loop_jobs as j on j.id = t.job_id",
            "t.id = '07070707-0707-0707-0707-070707070707'",
            "for update of t",
            "t.workspace_id as t_workspace_id",
            "t.user_id as t_user_id",
            "t.next_run_at as t_next_run_at",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        // Doubled: the soft-delete manager plus the explicit
        // `deleted_at__isnull=True` (the dispatch claim keeps one).
        assert_eq!(mine.matches("t.deleted_at is null").count(), 2);
        // The full job row rides along: cursor advance needs
        // dtstart/rrule/tzid, the re-check needs id/min_role, dispatch
        // needs prompt/public_name.
        for col in [
            "j.prompt",
            "j.public_name",
            "j.min_role",
            "j.dtstart",
            "j.rrule",
            "j.tzid",
        ] {
            assert!(mine.contains(col), "missing {col} in:\n{sql}");
        }
        for col in loop_job::COLUMNS {
            assert!(
                mine.contains(&format!("j.{col}")),
                "missing job column j.{col} in:\n{sql}"
            );
        }
        assert!(mine.contains("loop_targets"));
        // No two output columns share a name: a bare `j.id` next to a
        // bare `t.id` would make name-based mapping (`try_get("id")`)
        // return the wrong row's id at runtime while every fragment
        // assertion above still passes.
        let select_list = mine
            .split(" from loop_targets")
            .next()
            .expect("select list");
        let mut aliases: Vec<&str> = select_list
            .split(',')
            .map(|item| item.trim().rsplit(' ').next().expect("column"))
            .collect();
        aliases.sort_unstable();
        let before = aliases.len();
        aliases.dedup();
        assert_eq!(aliases.len(), before, "duplicate output names in:\n{sql}");
    }

    // Member-role read (`eligibility.py:136-148`): active edge only with
    // the doubled manager condition, newest row first, one row. Twin of
    // the scanner's probe — same arms, separate call site.
    #[test]
    fn member_role_sql_matches_check_read() {
        let sql = render_stmt(&member_role_select(pinned(), pinned()));
        assert!(matches!(parse(&sql), sqlparser::ast::Statement::Query(_)));
        let mine = norm(&sql);
        for fragment in [
            "select wm.role",
            "from workspace_members as wm",
            "wm.workspace_id = '07070707-0707-0707-0707-070707070707'",
            "wm.member_id = '07070707-0707-0707-0707-070707070707'",
            "wm.is_active = true",
            "wm.deleted_at is null",
            "order by wm.created_at desc",
            "limit 1",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{sql}");
        }
    }

    // Phase-1 writes (`bgtasks/loop.py:217-223`): the skip path stamps
    // cursor + skipped-at + reason, the eligible path advances only the
    // cursor; both refresh `updated_at` (`auto_now`).
    #[test]
    fn phase1_writes_match_save_fields() {
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0).expect("fits");
        let skip = render_update(&fire_skip_update(Some(now), now, "master_paused", pinned()));
        assert!(matches!(parse(&skip), sqlparser::ast::Statement::Update(_)));
        let mine = norm(&skip);
        for fragment in [
            "update loop_targets",
            "next_run_at =",
            "last_skipped_at =",
            "last_skip_reason = 'master_paused'",
            "updated_at =",
            "where id =",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{skip}");
        }
        let advance = render_update(&fire_advance_update(Some(now), now, pinned()));
        assert!(matches!(
            parse(&advance),
            sqlparser::ast::Statement::Update(_)
        ));
        let mine = norm(&advance);
        for fragment in [
            "update loop_targets",
            "next_run_at =",
            "updated_at =",
            "where id =",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{advance}");
        }
        assert!(!mine.contains("last_skip"));
    }

    // Fixture cross-checks (FX-LOOP-05, `fire.before_after.json` +
    // `fire_future_noop.json`): the happy path advanced the cursor past
    // the before value and queued exactly one downstream run; the future
    // cursor is the race that writes nothing.
    #[test]
    fn fixtures_describe_claim_race_and_advance() {
        let happy = fixture("fire.before_after.json");
        assert!(happy["returned"].as_bool().unwrap());
        assert!(happy["after"]["cursor_advanced_past_now"]
            .as_bool()
            .unwrap());
        assert!(
            happy["after"]["next_run_at"].as_str().unwrap()
                > happy["before"]["next_run_at"].as_str().unwrap()
        );
        assert_eq!(happy["run_assistant_turn_delay_calls"].as_i64().unwrap(), 1);
        let noop = fixture("fire_future_noop.json");
        assert!(!noop["returned"].as_bool().unwrap());
        assert!(noop["method"].as_str().unwrap().contains("no writes"));
    }

    // Registry layer: the fire name is the scanner's fan-out contract
    // (`loop/scan.rs` `TASK_NAMES` + `fire_message`): registering the fire
    // handler flips ownership Local while the scan name stays untouched
    // here — unported groups keep serving through ownership routing.
    #[test]
    fn task_name_matches_scanner_fanout() {
        assert_eq!(
            FIRE_LOOP_TARGET_TASK,
            "pi_dash.bgtasks.loop.fire_loop_target"
        );
        assert!(crate::loop_scan::TASK_NAMES.contains(&FIRE_LOOP_TARGET_TASK));
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let message = crate::loop_scan::fire_message(&id);
        assert_eq!(message.task, FIRE_LOOP_TARGET_TASK);
    }

    #[tokio::test]
    async fn registry_owns_fire_after_register() {
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let mut registry = Registry::new();
        assert_eq!(route_for(&registry, FIRE_LOOP_TARGET_TASK), Route::Unknown);
        register_fire(&mut registry, pool);
        assert!(registry.owns(FIRE_LOOP_TARGET_TASK));
        assert_eq!(route_for(&registry, FIRE_LOOP_TARGET_TASK), Route::Local);
    }

    // The kill switch itself (`loop_enabled`, `LOOP_ENABLED_ENV`) is owned
    // and covered by `loop/scan.rs` — including its no-DB short-circuit
    // test. A second mutator of that process-wide variable here would race
    // the scanner's test under parallel execution, so the fire's own
    // `if !loop_enabled()` gate is verified by inspection against
    // `bgtasks/loop.py:189-190`, not by a second env flip.
    #[test]
    fn fire_entry_keeps_python_task_shape() {
        // `bind=True, max_retries=0` has no Rust spelling: the handler
        // below always settles (`Verdict::Ack`) and surfaces infra failure
        // as a handler error for the worker-loop budget — the same shape
        // as `register_fire_binding` (PIDASHCONV-209).
        assert_eq!(
            FIRE_LOOP_TARGET_TASK,
            "pi_dash.bgtasks.loop.fire_loop_target"
        );
    }
}
