//! Loop beat scanner: `scan_due_targets` (D-03).
//!
//! Ports the scanner half of `pi_dash/bgtasks/loop.py`
//! (`_is_enabled`, `_stagger`, `_next_fire_for_job`, `_reconcile_targets`,
//! `_advance_ineligible_due`, `scan_due_targets`, lines 40-177) via the
//! D-03 queries layer ([`pidash_db::r#loop::queries`], owned by
//! PIDASHCONV-154 — called, never re-ported) and the shared predicate
//! logic ([`pidash_services::r#loop::eligibility`]).
//!
//! The scanner runs once a minute under Celery Beat
//! (`celery.py:127-130`, `scan-due-loop-targets`) and does two things:
//! reconcile (create missing targets for new membership edges, throttled)
//! and fan-out (one `fire_loop_target` job per eligible due target, capped).
//! Unlike the scheduler there is **no rollback phase**: dispatch here is
//! local row creation, not a remote pod match that can transiently fail.
//!
//! **Beat must run as a singleton** — multiple Beat schedulers double the
//! scan rate. The atomic SFU claim in the fire task (PIDASHCONV-157) keeps
//! fan-out race-safe regardless.
//!
//! Translation notes (translate, don't redesign):
//!
//! * `LOOP_ENABLED` follows `settings/common.py:279` exactly:
//!   `get_config("LOOP_ENABLED", "true").lower() in ("1", "true", "yes")`.
//!   Missing means enabled. This is *not* the scheduler's `== "true"`
//!   spelling (`common.py:446`) — `"1"` and `"yes"` enable the loop.
//! * The stagger seed is `f"{job_id}:{workspace_id}:{user_id}"` with
//!   `str(uuid)` rendering; [`Uuid::to_string`] renders identically, and
//!   `crc32fast` is the same ISO 3309 CRC-32 as `zlib.crc32`, so offsets
//!   match fixture `stagger.golden.json` bit for bit.
//! * `bulk_update` bypasses `auto_now`, so the advance pass writes each
//!   row's `updated_at` back unchanged (a faithful no-op, kept as-is).
//! * `_reconcile_targets` returns `len(batch)` summed — attempted inserts,
//!   including rows silently skipped by `ignore_conflicts`. Kept as-is.
//! * The eligible id slice keeps the Python order
//!   (`order_by("next_run_at")`, `eligible_due_target_ids_sql`): Postgres
//!   `ASC` puts `NULL`s last on both planes — same database, same order.
//!
//! Fixture: FX-LOOP-05 (`fixtures/loop/tasks/`; recorded by PIDASHCONV-151).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Timelike, Utc};
use sea_orm::{ConnectionTrait, DbBackend};
use sea_query::{Alias, Condition, Expr, InsertStatement, JoinType, Order, Query, SelectStatement};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::queue::{enqueue, JobRow, NewJob};
use crate::schedule::{beat_schedule, BeatEntry};
use crate::task_message::TaskMessage;
use crate::tasks_ticker::rrule::next_fire_from_rrule;
use crate::worker::{HandlerError, Registry, Verdict};
use crate::Error;
use pidash_db::orm_tx::into_sqlx;
use pidash_db::r#loop::models::loop_job::{self, LoopJob};
use pidash_db::r#loop::models::loop_target;
use pidash_db::r#loop::queries;

// ---------------------------------------------------------------------------
// Task names, beat entry, settings
// ---------------------------------------------------------------------------

/// Celery task name of the loop scanner
/// (`bgtasks/loop.py:148`, `@shared_task(name=...)`).
pub const SCAN_DUE_TARGETS_TASK: &str = "pi_dash.bgtasks.loop.scan_due_targets";
/// Celery task name fanned out per eligible due target
/// (`bgtasks/loop.py:168`, `fire_loop_target.delay(str(target_id))`).
pub const FIRE_LOOP_TARGET_TASK: &str = "pi_dash.bgtasks.loop.fire_loop_target";

/// Every Celery task name this module owns. The scan name gets a local
/// handler in [`register_scanner`]; the fire name stays Python-owned (see
/// [`crate::worker::route_for`]) until PIDASHCONV-157 registers its own —
/// either side speaks the same wire payloads, so no fan-out is ever
/// dropped or double-run across the handoff.
pub const TASK_NAMES: [&str; 2] = [SCAN_DUE_TARGETS_TASK, FIRE_LOOP_TARGET_TASK];

/// Beat entry name for the loop scanner (`celery.py:127-130`).
pub const SCAN_DUE_LOOP_TARGETS_BEAT: &str = "scan-due-loop-targets";

/// Instance-level kill switch (`bgtasks/loop.py:40-41`).
pub const LOOP_ENABLED_ENV: &str = "LOOP_ENABLED";
/// Stagger window in minutes (`bgtasks/loop.py:47`).
pub const LOOP_STAGGER_WINDOW_ENV: &str = "LOOP_STAGGER_WINDOW_MINUTES";
/// Reconcile throttle in minutes (`bgtasks/loop.py:69`).
pub const LOOP_RECONCILE_EVERY_ENV: &str = "LOOP_RECONCILE_EVERY_MINUTES";
/// Fan-out cap per tick (`bgtasks/loop.py:160`).
pub const LOOP_MAX_DISPATCH_ENV: &str = "LOOP_MAX_DISPATCH_PER_TICK";

/// Default stagger window (`getattr(settings, "LOOP_STAGGER_WINDOW_MINUTES", 60)`).
pub const DEFAULT_STAGGER_WINDOW_MINUTES: i64 = 60;
/// Default reconcile throttle (`getattr(settings, "LOOP_RECONCILE_EVERY_MINUTES", 15)`).
pub const DEFAULT_RECONCILE_EVERY_MINUTES: i64 = 15;
/// Default dispatch cap (`getattr(settings, "LOOP_MAX_DISPATCH_PER_TICK", 100)`).
pub const DEFAULT_MAX_DISPATCH_PER_TICK: i64 = 100;
/// Rows per bulk statement (`bulk_create` / `bulk_update` `batch_size=500`).
pub const BULK_BATCH_SIZE: usize = 500;

// ---------------------------------------------------------------------------
// Pure settings helpers (env wrappers below; truth tables unit-tested)
// ---------------------------------------------------------------------------

/// `getattr(settings, "LOOP_ENABLED", True)` with the exact
/// `settings/common.py:279` spelling: the lowercased value is in
/// `("1", "true", "yes")`; a missing variable reads as enabled.
pub fn loop_enabled_raw(raw: Option<&str>) -> bool {
    match raw {
        None => true,
        Some(value) => matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"),
    }
}

/// Parse a positive-minutes setting with `max(1, ...)` clamping
/// (`bgtasks/loop.py:47,69,160`). Unparseable input falls back to the
/// default: Python parses the same knob once at Django boot (where an
/// invalid value refuses to boot), while here the value is read per tick
/// and the task must never crash on it.
fn parse_floor_1(raw: Option<&str>, default: i64) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .map(|v| v.max(1))
        .unwrap_or(default)
}

/// Stagger window, floored at 1 (`bgtasks/loop.py:47`).
pub fn stagger_window_raw(raw: Option<&str>) -> i64 {
    parse_floor_1(raw, DEFAULT_STAGGER_WINDOW_MINUTES)
}

/// Reconcile throttle, floored at 1 (`bgtasks/loop.py:69`).
pub fn reconcile_every_raw(raw: Option<&str>) -> i64 {
    parse_floor_1(raw, DEFAULT_RECONCILE_EVERY_MINUTES)
}

/// Dispatch cap, floored at 1 (`bgtasks/loop.py:160`).
pub fn dispatch_cap_raw(raw: Option<&str>) -> i64 {
    parse_floor_1(raw, DEFAULT_MAX_DISPATCH_PER_TICK)
}

/// Instance-level kill switch (`bgtasks/loop.py:40-41`).
pub fn loop_enabled() -> bool {
    let raw = std::env::var(LOOP_ENABLED_ENV).ok();
    loop_enabled_raw(raw.as_deref())
}

/// Stagger window in minutes (`bgtasks/loop.py:47`).
pub fn stagger_window() -> i64 {
    let raw = std::env::var(LOOP_STAGGER_WINDOW_ENV).ok();
    stagger_window_raw(raw.as_deref())
}

/// Reconcile throttle in minutes (`bgtasks/loop.py:69`).
pub fn reconcile_every() -> i64 {
    let raw = std::env::var(LOOP_RECONCILE_EVERY_ENV).ok();
    reconcile_every_raw(raw.as_deref())
}

/// Fan-out cap per tick (`bgtasks/loop.py:160`).
pub fn dispatch_cap() -> i64 {
    let raw = std::env::var(LOOP_MAX_DISPATCH_ENV).ok();
    dispatch_cap_raw(raw.as_deref())
}

/// Reconcile throttle gate (`bgtasks/loop.py:70-71`):
/// `now.minute % every != 0` means this tick does not reconcile.
pub fn should_reconcile(minute: u32, every: i64) -> bool {
    minute as i64 % every.max(1) == 0
}

/// Deterministic per-edge stagger offset in whole minutes
/// (`bgtasks/loop.py:44-49`): `crc32(f"{job_id}:{ws_id}:{user_id}") %
/// window`. No randomness in scheduling paths — reproducible tests,
/// stable per edge.
pub fn stagger_offset_minutes(
    job_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    window: i64,
) -> i64 {
    let window = window.max(1) as u32;
    let seed = format!("{job_id}:{workspace_id}:{user_id}");
    (crc32fast::hash(seed.as_bytes()) % window) as i64
}

/// The stagger as a duration, added to the next occurrence
/// (`bgtasks/loop.py:99,134`, `nxt + _stagger(...)`).
pub fn stagger_duration(job_id: &Uuid, workspace_id: &Uuid, user_id: &Uuid) -> chrono::Duration {
    chrono::Duration::minutes(stagger_offset_minutes(
        job_id,
        workspace_id,
        user_id,
        stagger_window(),
    ))
}

/// Next fire for one job's RRULE bundle (`bgtasks/loop.py:52-55`).
/// Loop jobs carry the same bundle subset as scheduler bindings
/// (`dtstart`, `rrule`, `tzid`) with no `rdates`/`exdates`, so both sides
/// call the single `_rrule` port owned by PIDASHCONV-205.
pub fn next_fire_for_job(job: &LoopJob, now: &DateTime<Utc>) -> Option<DateTime<Utc>> {
    next_fire_from_rrule(job.dtstart, &job.rrule, &job.tzid, &[], &[], *now)
}

// ---------------------------------------------------------------------------
// Scanner SQL owned by this module (eligibility SQL stays in the queries layer)
// ---------------------------------------------------------------------------

/// Missing-edge anti-join for one job (`bgtasks/loop.py:80-93`).
/// Arm for arm with the ORM:
///
/// * `WorkspaceMember(is_active=True, member__is_active=True,
///   deleted_at__isnull=True)` — the member join follows the non-nullable
///   FK with `INNER JOIN` onto `users` (`db_table = "users"`,
///   `models/user.py:136`); that table carries no `deleted_at` column, so
///   only `workspace_members.deleted_at` is scoped (doubled: default
///   manager plus the explicit filter, as in `member_exists_select`).
/// * `Exists(has_target)` negated — the `NOT EXISTS` on the live
///   `(job, workspace, user)` edge (`loop_target_unique_edge_when_active`).
fn missing_edges_select(job_id: Uuid) -> SelectStatement {
    let wm = Alias::new("wm");
    let lt = Alias::new("lt");
    let mut sub = Query::select();
    sub.expr(Expr::val(1))
        .from_as(Alias::new(loop_target::TABLE), lt.clone())
        .cond_where(
            Condition::all()
                .add(Expr::col((lt.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((lt.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((lt.clone(), Alias::new("job_id"))).eq(job_id))
                .add(
                    Expr::col((lt.clone(), Alias::new("workspace_id")))
                        .equals((wm.clone(), Alias::new("workspace_id"))),
                )
                .add(
                    Expr::col((lt.clone(), Alias::new("user_id")))
                        .equals((wm.clone(), Alias::new("member_id"))),
                ),
        )
        .limit(1);
    let mut sel = Query::select();
    sel.column((wm.clone(), Alias::new("workspace_id")))
        .column((wm.clone(), Alias::new("member_id")))
        .from_as(Alias::new("workspace_members"), wm.clone())
        .join_as(
            JoinType::InnerJoin,
            Alias::new("users"),
            Alias::new("u"),
            Expr::col((Alias::new("u"), Alias::new("id")))
                .equals((wm.clone(), Alias::new("member_id"))),
        )
        .cond_where(
            Condition::all()
                .add(Expr::col((wm.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((wm.clone(), Alias::new("deleted_at"))).is_null())
                .add(Expr::col((wm.clone(), Alias::new("is_active"))).eq(true))
                .add(Expr::col((Alias::new("u"), Alias::new("is_active"))).eq(true))
                .add(Expr::exists(sub).not()),
        );
    sel
}

/// Live membership role for the fire-time re-check
/// (`eligibility.py:138-146`, `check`'s `WorkspaceMember` read).
/// `ORDER BY created_at DESC` is the queryset's
/// `Meta.ordering = ("-created_at",)` under `.first()`; the live-edge
/// partial unique admits at most one row anyway.
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

/// One job by id: the advance pass warms its per-job cache from
/// `fetch_enabled_jobs` and only falls back here on a mid-tick race
/// (a job disabled between the two reads). Note: no `deleted_at`
/// filter — the Python `.get(pk)` falls back to the job row itself
/// and the caller re-checks `enabled` on the returned row.
fn job_by_id_select(job_id: Uuid) -> SelectStatement {
    let mut sel = Query::select();
    for col in loop_job::COLUMNS {
        sel.column((Alias::new(loop_job::TABLE), Alias::new(*col)));
    }
    sel.from(Alias::new(loop_job::TABLE))
        .cond_where(Expr::col((Alias::new(loop_job::TABLE), Alias::new("id"))).eq(job_id));
    sel
}

/// One fire job per due id: `fire_loop_target.delay(str(id))` is
/// `args=[str(id)]`, `kwargs={}` on the wire, so the queue row carries
/// exactly that. The worker loop's forward path
/// ([`crate::worker::dispatch`]) converts the row back to the identical
/// Celery v2 body — wire-identical whichever plane serves the fire task.
fn fire_job(id: &Uuid) -> NewJob {
    NewJob::new(FIRE_LOOP_TARGET_TASK, json!([id.to_string()]), json!({}))
}

/// The Celery v2 message a fan-out row becomes on the wire: same task
/// name, `args=[str(id)]`, empty kwargs — byte-shape identical to the
/// Python `.delay(str(id))` call. Repeats
/// [`crate::worker::dispatch`]'s forward path arm for arm (that function
/// is the runtime source of truth; this one exists so tests and the
/// PIDASHCONV-157 fire port can assert the wire contract broker-free).
pub fn fire_message(id: &Uuid) -> TaskMessage {
    let (args, kwargs) = fire_job(id).into_message_parts();
    TaskMessage::new(FIRE_LOOP_TARGET_TASK, args, kwargs)
}

/// Cap the dispatch slice in Python (`bgtasks/loop.py:166`, `ids =
/// eligible_ids[:cap]`). Over-cap eligibles stay due — backpressure; they
/// drain next tick.
pub fn cap_ids(mut ids: Vec<Uuid>, cap: i64) -> Vec<Uuid> {
    ids.truncate(cap.max(1) as usize);
    ids
}

// ---------------------------------------------------------------------------
// Reconcile + advance + scan
// ---------------------------------------------------------------------------

/// Fetch one job by id (mid-tick race fallback for the advance cache).
async fn fetch_job_by_id(
    db: &impl ConnectionTrait,
    job_id: &Uuid,
) -> Result<Option<LoopJob>, Error> {
    let row = db
        .query_one(DbBackend::Postgres.build(&job_by_id_select(*job_id)))
        .await
        .map_err(into_sqlx)?;
    Ok(row
        .map(|row| queries::map_loop_job_row_q(&row))
        .transpose()?)
}

/// Live membership role for one edge, or `None` when no active row
/// (`eligibility.py:138-148`, `membership is None` → `MEMBERSHIP_GONE`).
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

/// Multi-row INSERT for one reconcile chunk
/// (`bulk_create(batch, ignore_conflicts=True, batch_size=500)`).
/// Eight values per row in column order (`id`, `created_at`,
/// `updated_at`, `job_id`, `workspace_id`, `user_id`, `next_run_at`,
/// `last_skip_reason`); `ON CONFLICT DO NOTHING` is
/// `ignore_conflicts=True`.
type ReconcileRow = (
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Uuid,
    Uuid,
    Uuid,
    DateTime<Utc>,
);

fn reconcile_insert(rows: &[ReconcileRow]) -> InsertStatement {
    let mut stmt = Query::insert();
    stmt.into_table(Alias::new(loop_target::TABLE)).columns([
        Alias::new("id"),
        Alias::new("created_at"),
        Alias::new("updated_at"),
        Alias::new("job_id"),
        Alias::new("workspace_id"),
        Alias::new("user_id"),
        Alias::new("next_run_at"),
        Alias::new("last_skip_reason"),
    ]);
    for (id, created_at, updated_at, job_id, workspace_id, user_id, next_run_at) in rows {
        stmt.values_panic([
            Expr::val(*id).into(),
            Expr::val(created_at.fixed_offset()).into(),
            Expr::val(updated_at.fixed_offset()).into(),
            Expr::val(*job_id).into(),
            Expr::val(*workspace_id).into(),
            Expr::val(*user_id).into(),
            Expr::val(next_run_at.fixed_offset()).into(),
            Expr::val(loop_target::DEFAULT_LAST_SKIP_REASON.to_owned()).into(),
        ]);
    }
    stmt.on_conflict(sea_query::OnConflict::new().do_nothing().to_owned());
    stmt
}

/// Skip-advance write for the ineligible pass (`bgtasks/loop.py:111-145`):
/// `bulk_update` of the cursor + reason + timestamps for one target.
fn advance_update(
    next_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    reason: &str,
    updated_at: DateTime<Utc>,
    target_id: Uuid,
) -> sea_query::UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(loop_target::TABLE))
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
                Expr::val(updated_at.fixed_offset()).into(),
            ),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(target_id));
    stmt
}

/// Create missing `LoopTarget` rows for (enabled job × active edge)
/// (`bgtasks/loop.py:62-108`). Throttled to once per
/// `LOOP_RECONCILE_EVERY_MINUTES`; first fire is next occurrence plus
/// stagger regardless. Returns attempted inserts (see module docs).
pub async fn reconcile_targets(pool: &PgPool, now: &DateTime<Utc>) -> Result<usize, Error> {
    let every = reconcile_every();
    if !should_reconcile(now.minute(), every) {
        return Ok(0);
    }
    let db = pidash_db::sea_orm_connection(pool);
    let mut created = 0usize;
    for job in queries::fetch_enabled_jobs(&db).await? {
        let Some(next) = next_fire_for_job(&job, now) else {
            // Bad RRULE — admin API validation should make this unreachable.
            tracing::warn!(job_slug = %job.slug, rrule = %job.rrule, "loop.reconcile: unusable rrule");
            continue;
        };
        let edge_rows = db
            .query_all(DbBackend::Postgres.build(&missing_edges_select(job.id)))
            .await
            .map_err(into_sqlx)?;
        let edges: Vec<(Uuid, Uuid)> = edge_rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<Uuid>("", "workspace_id").map_err(into_sqlx)?,
                    row.try_get::<Uuid>("", "member_id").map_err(into_sqlx)?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        for chunk in edges.chunks(BULK_BATCH_SIZE) {
            // One multi-row INSERT per chunk; `ON CONFLICT DO NOTHING` is
            // `bulk_create(ignore_conflicts=True)`. Django fills
            // `auto_now_add`/`auto_now`/uuid defaults at `bulk_create`
            // time, so the statement carries them explicitly.
            let mut values = Vec::with_capacity(chunk.len());
            for (workspace_id, user_id) in chunk {
                let next_run_at = next
                    + chrono::Duration::minutes(stagger_offset_minutes(
                        &job.id,
                        workspace_id,
                        user_id,
                        stagger_window(),
                    ));
                values.push((
                    Uuid::new_v4(),
                    *now,
                    *now,
                    job.id,
                    *workspace_id,
                    *user_id,
                    next_run_at,
                ));
            }
            db.execute(DbBackend::Postgres.build(&reconcile_insert(&values)))
                .await
                .map_err(into_sqlx)?;
            created += chunk.len();
        }
    }
    if created > 0 {
        tracing::info!(created, "loop.reconcile: created targets");
    }
    Ok(created)
}

/// Advance the cursor for due-but-ineligible targets
/// (`bgtasks/loop.py:111-145`): recompute each job's next fire once,
/// resolve the fixed-precedence skip reason per target, and write
/// `next_run_at` + `last_skipped_at` + `last_skip_reason` back
/// (`bulk_update`, batch 500 — one transaction here for the same
/// batch-atomicity). Returns the number advanced.
pub async fn advance_ineligible_due(
    pool: &PgPool,
    now: &DateTime<Utc>,
    eligible_ids: &HashSet<Uuid>,
) -> Result<usize, Error> {
    let db = pidash_db::sea_orm_connection(pool);
    let targets = queries::fetch_due_targets(&db, *now).await?;
    // Per-job next-fire cache (`bgtasks/loop.py:125,129-131`), warmed
    // from the enabled-job read the scanner already trusts.
    let mut jobs: HashMap<Uuid, LoopJob> = HashMap::new();
    for job in queries::fetch_enabled_jobs(&db).await? {
        jobs.insert(job.id, job);
    }
    let mut next_by_job: HashMap<Uuid, Option<DateTime<Utc>>> = HashMap::new();
    let mut advanced = 0usize;
    let mut tx = pool.begin().await?;
    for target in &targets {
        if eligible_ids.contains(&target.id) {
            continue;
        }
        let job = match jobs.get(&target.job_id) {
            Some(job) => job.clone(),
            None => {
                let fetched = {
                    let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
                    fetch_job_by_id(&conn, &target.job_id).await?
                };
                match fetched {
                    Some(job) => {
                        jobs.insert(job.id, job.clone());
                        job
                    }
                    None => continue,
                }
            }
        };
        let next = match next_by_job.get(&job.id) {
            Some(cached) => *cached,
            None => {
                let next = next_fire_for_job(&job, now);
                next_by_job.insert(job.id, next);
                next
            }
        };
        // Fixed precedence, same reads as the fire-time re-check
        // (`eligibility.py:122-154`): master pause → job opt-out →
        // membership/role → LLM credentials. The `queries` reads run
        // on the same transaction through `TxConn`; `fetch_member_role`
        // stays on the sqlx executor between them.
        let (master_paused, job_opted_out) = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            let master_paused = !queries::fetch_master_enabled(&conn, target.user_id).await?;
            let off_ids = queries::fetch_off_job_ids(&conn, target.user_id).await?;
            (master_paused, off_ids.contains(&target.job_id))
        };
        let role = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            fetch_member_role(&conn, &target.workspace_id, &target.user_id)
                .await?
                .map(|r| r as i32)
        };
        let has_llm = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            queries::fetch_user_has_llm(&conn, target.user_id).await?
        };
        let reason = pidash_services::r#loop::eligibility::check(
            master_paused,
            job_opted_out,
            role,
            job.min_role as i32,
            has_llm,
        );
        let next_run_at = next.map(|nxt| {
            nxt + chrono::Duration::minutes(stagger_offset_minutes(
                &job.id,
                &target.workspace_id,
                &target.user_id,
                stagger_window(),
            ))
        });
        // `bulk_update` bypasses `auto_now`: `updated_at` goes back
        // unchanged (faithful no-op, kept as-is — see module docs).
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            conn.execute(DbBackend::Postgres.build(&advance_update(
                next_run_at,
                *now,
                reason.map(|r| r.as_str()).unwrap_or(""),
                target.updated_at,
                target.id,
            )))
            .await
            .map_err(into_sqlx)?;
        }
        advanced += 1;
    }
    tx.commit().await?;
    Ok(advanced)
}

/// Reconcile, then fan out `fire_loop_target` for eligible due targets
/// (`bgtasks/loop.py:148-177`). Returns the number of fan-outs.
pub async fn scan_due_targets(pool: &PgPool, now: &DateTime<Utc>) -> Result<usize, Error> {
    if !loop_enabled() {
        return Ok(0);
    }
    reconcile_targets(pool, now).await?;

    let cap = dispatch_cap();
    // Evaluate the eligible set once; cap the dispatch slice in Rust and
    // reuse the full set to exclude eligibles from the ineligible pass.
    let db = pidash_db::sea_orm_connection(pool);
    let eligible_ids: Vec<Uuid> = queries::fetch_eligible_due_target_ids(&db, *now).await?;
    let ids = cap_ids(eligible_ids.clone(), cap);
    for target_id in &ids {
        enqueue(pool, &fire_job(target_id)).await?;
    }

    // Due but ineligible targets still need their cursor advanced, or
    // they'd be re-scanned every minute forever. (Over-cap *eligible*
    // targets are intentionally left due — backpressure.)
    let eligible_set: HashSet<Uuid> = eligible_ids.into_iter().collect();
    advance_ineligible_due(pool, now, &eligible_set).await?;

    if !ids.is_empty() {
        tracing::info!(
            count = ids.len(),
            "loop.scan: dispatched fire_loop_target tasks"
        );
    }
    Ok(ids.len())
}

/// The owned beat entry, selected from the F-09 schedule
/// ([`crate::schedule::beat_schedule`], transcribed from `celery.py`):
/// `scan-due-loop-targets`, `crontab(minute=*)`. Every other entry belongs
/// to its own domain — selection, not a fork.
pub fn beat_entries() -> Vec<BeatEntry> {
    beat_schedule()
        .into_iter()
        .filter(|entry| entry.name == SCAN_DUE_LOOP_TARGETS_BEAT)
        .collect()
}

/// Register the local scan handler. The pool is captured by the closure
/// because [`crate::worker::Handler`] receives only the claimed row; each
/// fire runs the scan with the firing instant as `now`, then enqueues its
/// fan-outs. A database failure reports the error text so the worker loop
/// retries with budget instead of acking a missed minute.
pub fn register_scanner(registry: &mut Registry, pool: PgPool) {
    registry.register(
        SCAN_DUE_TARGETS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = pool.clone();
            let fut: std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Verdict, HandlerError>> + Send>,
            > = Box::pin(async move {
                let now = Utc::now();
                scan_due_targets(&pool, &now)
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
    use crate::schedule::{is_due, Cadence};
    use crate::scheduler::default_schedule;
    use crate::worker::{route_for, Route};
    use chrono::TimeZone;

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

    // FX-LOOP-05 `stagger.golden.json` (`bgtasks/loop.py:44-49`): the seed
    // is `f"{job_id}:{workspace_id}:{user_id}"` with `str(uuid)` rendering
    // and the digest is plain CRC-32, so `crc32fast` reproduces the
    // recorded digests and the `% 60` offsets exactly.
    #[test]
    fn stagger_replays_fx_loop_05_vectors() {
        let golden = fixture("stagger.golden.json");
        assert!(golden["window_setting"]
            .as_str()
            .unwrap()
            .contains("default 60"));
        for vector in golden["vectors"].as_array().unwrap() {
            let job = Uuid::parse_str(vector["job_id"].as_str().unwrap()).unwrap();
            let ws = Uuid::parse_str(vector["workspace_id"].as_str().unwrap()).unwrap();
            let user = Uuid::parse_str(vector["user_id"].as_str().unwrap()).unwrap();
            let seed = format!(
                "{}:{}:{}",
                vector["job_id"].as_str().unwrap(),
                vector["workspace_id"].as_str().unwrap(),
                vector["user_id"].as_str().unwrap()
            );
            let crc = crc32fast::hash(seed.as_bytes());
            // Only the first vector records the raw digest; every vector
            // records the `% 60` offset the scanner actually schedules.
            if let Some(recorded) = vector.get("crc32").and_then(|v| v.as_i64()) {
                assert_eq!(crc as i64, recorded);
            }
            assert_eq!(crc % 60, vector["offset_minutes"].as_f64().unwrap() as u32);
            assert_eq!(
                stagger_offset_minutes(&job, &ws, &user, 60) as f64,
                vector["offset_minutes"].as_f64().unwrap()
            );
        }
    }

    // `LOOP_ENABLED` (`settings/common.py:279`): missing reads enabled and
    // only `"1"`/`"true"`/`"yes"` (any case) enable — notably `"1"` and
    // `"yes"` count, unlike the scheduler's `== "true"` spelling.
    #[test]
    fn loop_enabled_matches_python_semantics() {
        assert!(loop_enabled_raw(None));
        for enabled in ["true", "TRUE", "True", "1", "yes", "YES", "Yes"] {
            assert!(loop_enabled_raw(Some(enabled)), "{enabled} enables");
        }
        for disabled in ["false", "FALSE", "0", "", "no", "on", "2"] {
            assert!(!loop_enabled_raw(Some(disabled)), "{disabled} disables");
        }
    }

    // Floors and defaults (`bgtasks/loop.py:47,69,160`): `max(1, int(...))`
    // with 60/15/100 defaults; surrounding whitespace parses like Python's
    // `int()`, garbage falls back instead of crashing the tick.
    #[test]
    fn settings_floors_and_defaults_match_python() {
        assert_eq!(stagger_window_raw(None), 60);
        assert_eq!(stagger_window_raw(Some("30")), 30);
        assert_eq!(stagger_window_raw(Some(" 45 ")), 45);
        assert_eq!(stagger_window_raw(Some("0")), 1);
        assert_eq!(stagger_window_raw(Some("-5")), 1);
        assert_eq!(stagger_window_raw(Some("bogus")), 60);
        assert_eq!(reconcile_every_raw(None), 15);
        assert_eq!(reconcile_every_raw(Some("1")), 1);
        assert_eq!(reconcile_every_raw(Some("0")), 1);
        assert_eq!(dispatch_cap_raw(None), 100);
        assert_eq!(dispatch_cap_raw(Some("100")), 100);
        assert_eq!(dispatch_cap_raw(Some("0")), 1);
    }

    // Reconcile throttle (`bgtasks/loop.py:70-71`): only minutes divisible
    // by `every` reconcile; `every=1` reconciles every tick (the contract
    // suite runs the backend with `LOOP_RECONCILE_EVERY_MINUTES=1`).
    #[test]
    fn reconcile_throttle_matches_python() {
        assert!(should_reconcile(0, 15));
        assert!(should_reconcile(15, 15));
        assert!(should_reconcile(30, 15));
        assert!(should_reconcile(45, 15));
        assert!(!should_reconcile(7, 15));
        assert!(!should_reconcile(1, 15));
        assert!(!should_reconcile(59, 15));
        assert!(should_reconcile(3, 1));
        assert!(should_reconcile(59, 1));
    }

    // Dispatch cap (`bgtasks/loop.py:160,166`): `eligible_ids[:cap]` keeps
    // order and the over-cap tail stays due; the floor keeps cap 0/negative
    // at one dispatch, exactly like `max(1, ...)`.
    #[test]
    fn dispatch_cap_slices_in_order() {
        let ids: Vec<Uuid> = (1..=5).map(|n| Uuid::from_bytes([n; 16])).collect();
        assert_eq!(cap_ids(ids.clone(), 100), ids);
        assert_eq!(cap_ids(ids.clone(), 5), ids);
        assert_eq!(cap_ids(ids.clone(), 2), ids[..2]);
        assert_eq!(cap_ids(ids.clone(), 0).len(), 1);
        assert_eq!(cap_ids(ids.clone(), -3).len(), 1);
        assert!(cap_ids(vec![], 100).is_empty());
    }

    fn job_with(rrule: &str, dtstart: DateTime<Utc>) -> LoopJob {
        LoopJob {
            id: Uuid::from_bytes([9; 16]),
            created_at: dtstart,
            updated_at: dtstart,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            slug: "fx".to_owned(),
            name: "fx".to_owned(),
            public_name: "fx".to_owned(),
            public_description: String::new(),
            prompt: "fx".to_owned(),
            min_role: 15,
            enabled: true,
            is_builtin: true,
            dtstart,
            rrule: rrule.to_owned(),
            tzid: "UTC".to_owned(),
        }
    }

    // Next-fire delegation (`bgtasks/loop.py:52-55`): a daily bundle fires
    // next midnight strictly after now; an empty rrule is the single shot
    // at dtstart (ahead fires, past is exhausted); the bundle subset means
    // no rdates/exdates are consulted.
    #[test]
    fn next_fire_follows_rrule_bundle() {
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
        let dtstart = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(
            next_fire_for_job(&job_with("FREQ=DAILY", dtstart), &now),
            Some(Utc.with_ymd_and_hms(2026, 9, 29, 0, 0, 0).unwrap())
        );
        let future = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        assert_eq!(next_fire_for_job(&job_with("", future), &now), Some(future));
        assert_eq!(next_fire_for_job(&job_with("", dtstart), &now), None);
    }

    // Scanner-owned SQL parses and carries every ORM arm: the member edge
    // filters (both `is_active` flags), the negated live-edge `EXISTS`
    // with the `$1` job bind, and the role read's ordering + limit.
    #[test]
    fn scanner_sql_carries_every_python_arm() {
        let pinned = Uuid::from_bytes([7; 16]);
        let missing = missing_edges_select(pinned).to_string(sea_query::PostgresQueryBuilder);
        assert!(matches!(
            parse(&missing),
            sqlparser::ast::Statement::Query(_)
        ));
        let mine = norm(&missing);
        for fragment in [
            "from workspace_members as wm",
            "inner join users as u on u.id = wm.member_id",
            "wm.deleted_at is null",
            "wm.is_active = true",
            "u.is_active = true",
            "not exists",
            "from loop_targets as lt",
            "lt.job_id = '07070707-0707-0707-0707-070707070707'",
            "lt.workspace_id = wm.workspace_id",
            "lt.user_id = wm.member_id",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{missing}");
        }
        let role = member_role_select(pinned, pinned).to_string(sea_query::PostgresQueryBuilder);
        assert!(matches!(parse(&role), sqlparser::ast::Statement::Query(_)));
        let mine = norm(&role);
        for fragment in [
            "select wm.role",
            "from workspace_members as wm",
            "wm.workspace_id = '07070707-0707-0707-0707-070707070707'",
            "wm.member_id = '07070707-0707-0707-0707-070707070707'",
            "wm.is_active = true",
            "order by wm.created_at desc limit 1",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{role}");
        }
        let by_id = job_by_id_select(pinned).to_string(sea_query::PostgresQueryBuilder);
        assert!(matches!(parse(&by_id), sqlparser::ast::Statement::Query(_)));
        let by_id_where = norm(&by_id);
        assert!(by_id_where.contains("from loop_jobs where loop_jobs.id ="));
        assert!(!by_id_where.contains("deleted_at is null"));
    }

    // Reconcile INSERT (`bulk_create(ignore_conflicts=True)`): one
    // VALUES tuple per row with eight fields per row, so a short tuple
    // list compiles and passes every broker-free test, then fails at
    // runtime with a column-count mismatch on the first non-empty
    // reconcile — the tuple shape is asserted exactly.
    #[test]
    fn reconcile_insert_binds_match_placeholders() {
        fn dummy_rows(n: usize) -> Vec<ReconcileRow> {
            let now = Utc.timestamp_opt(1_700_000_000, 0).single().expect("fits");
            (0..n)
                .map(|_| {
                    (
                        Uuid::from_bytes([7; 16]),
                        now,
                        now,
                        Uuid::from_bytes([7; 16]),
                        Uuid::from_bytes([7; 16]),
                        Uuid::from_bytes([7; 16]),
                        now,
                    )
                })
                .collect()
        }
        for n in [1, 2, 3] {
            let sql = reconcile_insert(&dummy_rows(n)).to_string(sea_query::PostgresQueryBuilder);
            assert!(matches!(parse(&sql), sqlparser::ast::Statement::Insert(_)));
            let mine = norm(&sql);
            assert!(mine.contains("insert into loop_targets"), "{sql}");
            assert!(mine.contains("on conflict do nothing"), "{sql}");
            // Eight fields per row: n tuples means 8n value slots.
            assert_eq!(mine.matches("values (").count(), 1, "{sql}");
            let tail = mine.split("values").nth(1).expect("has VALUES");
            let tail = tail.split("on conflict").next().expect("has ON CONFLICT");
            assert_eq!(tail.matches('(').count(), n, "{sql}");
            assert_eq!(tail.matches(')').count(), n, "{sql}");
        }
    }

    // Fan-out payloads are Celery v2 wire-identical to the Python
    // `.delay(str(id))` call (`bgtasks/loop.py:168`): exact task name,
    // `args=[str(id)]`, empty kwargs — the shape the worker forward path
    // publishes.
    #[test]
    fn fanout_message_is_celery_wire_identical() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let message = fire_message(&id);
        assert_eq!(message.task, FIRE_LOOP_TARGET_TASK);
        assert_eq!(message.args, vec![json!(id.to_string())]);
        assert!(message.kwargs.is_empty());
        assert_eq!(
            FIRE_LOOP_TARGET_TASK,
            "pi_dash.bgtasks.loop.fire_loop_target"
        );
        assert_eq!(
            SCAN_DUE_TARGETS_TASK,
            "pi_dash.bgtasks.loop.scan_due_targets"
        );
    }

    // Registry layer: the scan name is locally owned once registered; the
    // fire name routes Python-owned until PIDASHCONV-157 claims it —
    // unported groups keep serving through ownership routing.
    #[tokio::test]
    async fn registry_owns_scanner_and_routes_fire_to_python() {
        assert_eq!(TASK_NAMES, [SCAN_DUE_TARGETS_TASK, FIRE_LOOP_TARGET_TASK]);
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let mut registry = Registry::new();
        for name in TASK_NAMES {
            assert_eq!(route_for(&registry, name), Route::Unknown);
        }
        register_scanner(&mut registry, pool);
        assert!(registry.owns(SCAN_DUE_TARGETS_TASK));
        assert_eq!(route_for(&registry, SCAN_DUE_TARGETS_TASK), Route::Local);
        assert!(!registry.owns(FIRE_LOOP_TARGET_TASK));
        assert_eq!(route_for(&registry, FIRE_LOOP_TARGET_TASK), Route::Unknown);
    }

    // The kill switch short-circuits before any database touch: with the
    // switch off, the scan returns 0 fan-outs on a pool that could never
    // connect (`connect_lazy` opens no connection until first use).
    #[tokio::test]
    async fn disabled_switch_scans_zero_without_db() {
        let saved = std::env::var(LOOP_ENABLED_ENV).ok();
        std::env::set_var(LOOP_ENABLED_ENV, "no");
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
        let count = scan_due_targets(&pool, &now).await.expect("kill switch");
        assert_eq!(count, 0);
        match saved {
            Some(v) => std::env::set_var(LOOP_ENABLED_ENV, v),
            None => std::env::remove_var(LOOP_ENABLED_ENV),
        }
    }

    // `celery.py:127-130`: exactly the owned entry resolves from the F-09
    // schedule with the recorded task name, firing every minute; it rides
    // the scheduler loop's default schedule.
    #[test]
    fn beat_entry_matches_celery_beat() {
        let entries = beat_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, SCAN_DUE_LOOP_TARGETS_BEAT);
        assert_eq!(entries[0].task, SCAN_DUE_TARGETS_TASK);
        let Cadence::Crontab(crontab) = &entries[0].cadence else {
            panic!("{} is not a crontab entry", entries[0].name);
        };
        for (month, day, hour, minute) in [(1, 2, 0, 0), (6, 15, 12, 30), (12, 31, 23, 59)] {
            let at = Utc
                .with_ymd_and_hms(2026, month, day, hour, minute, 0)
                .unwrap();
            assert!(crontab.matches(&at), "{} misses {at}", entries[0].name);
        }
        let at = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
        assert!(is_due(&entries[0], &at, None));
        assert!(default_schedule()
            .iter()
            .any(|e| e.name == SCAN_DUE_LOOP_TARGETS_BEAT));
    }
}
