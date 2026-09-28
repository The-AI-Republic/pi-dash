//! Loop eligibility + settings + admin query units (D-03, stage 4).
//!
//! Ports (all under `apps/api/pi_dash/`):
//!
//! * `loop/eligibility.py:27-51` (`_usable_llm_filter`, `llm_available_q`,
//!   `user_has_llm`) — the shared LLM-credential predicate, in queryset
//!   (`Exists`) and row (`SELECT 1 … LIMIT 1`) form.
//! * `loop/eligibility.py:54-85` (`_member_q`, `_job_off_q`,
//!   `_master_paused_q`) — the three annotated `Exists` subqueries.
//! * `loop/eligibility.py:88-100` ([`due_targets_sql`]) and `:103-114`
//!   ([`eligible_due_targets_sql`]) — the scanner pre-filter closure; the
//!   exact scanner id slice (`bgtasks/loop.py:163-165`,
//!   `order_by("next_run_at").values_list("id")`) is
//!   [`eligible_due_target_ids_sql`].
//! * `loop/views.py:24-49` ([`off_job_ids_sql`], [`master_enabled_sql`],
//!   [`enabled_jobs_sql`]) — the settings-payload reads.
//! * `loop/admin_views.py:136-149` ([`job_stats_sql`]) — the 24h rollup.
//! * `loop/admin_views.py:183-210` ([`targets_list_sql`]) — the filtered,
//!   paginated targets query; [`TargetListRow`] carries every `_row`
//!   (`:213-234`) input.
//!
//! Dynamic statements are sea-query builders; fixed reads are string
//! constants executed with runtime `sqlx::query` (no `query!` macros:
//! there is no build-time database, same as the merged `license/queries`
//! precedent). Every executor is generic over `sqlx::Executor`.
//!
//! SQL semantics are Django's, quirks included (translate, don't
//! redesign):
//!
//! * The default manager plus the explicit `deleted_at__isnull=True`
//!   filter double the `deleted_at IS NULL` conjunct on `loop_targets`
//!   (and on the `Exists` subquery tables) — real compiler output,
//!   preserved here.
//! * `enabled=False` renders as `NOT "…"."enabled"`; the builders emit
//!   the equivalent `= FALSE` and the tests normalize both spellings.
//! * `role__gte=OuterRef("job__min_role")` is a column-to-column
//!   comparison, kept exactly.
//! * `NULL` `next_run_at` (newly reconciled edges) counts as due.
//! * `TurnStatus` values (`assistant/models.py:64-70`) gate the `status`
//!   filter; unknown `skip_reason`/`status` params are ignored (no
//!   filter), exactly as the view does.
//!
//! Fixture source of truth: `rust-api/fixtures/loop/queries/*.sql` +
//! `*.rows.json` (FX-LOOP-03, recorded by PIDASHCONV-151). The
//! `#[cfg(test)]` suite matches every builder against its fixture
//! statement in normalized semantic form.

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query, SelectStatement};
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::models::{
    loop_job::{self, LoopJob},
    loop_target::{self, LoopTarget},
    SkipReason,
};

// ---------------------------------------------------------------------------
// Shared fragments
// ---------------------------------------------------------------------------

/// `assistant_user_llm_config` table holding BYOK rows
/// (`eligibility.py:40`, via `UserLLMConfig`).
pub const LLM_CONFIG_TABLE: &str = "assistant_user_llm_config";
/// `workspace_members` table (`eligibility.py:56`).
pub const WORKSPACE_MEMBER_TABLE: &str = "workspace_members";
/// `assistant_turn` table holding loop runs (`admin_views.py:141-147`).
pub const ASSISTANT_TURN_TABLE: &str = "assistant_turn";

/// The single source of truth for "usable LLM credentials"
/// (`eligibility.py:27-35`, `_usable_llm_filter`):
/// a `UserLLMConfig` row with a stored key.
pub fn usable_llm_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new("api_key_encrypted")).is_not_null())
}

/// `SELECT 1 AS "a" …` Exists-style subquery shell shared by the four
/// annotations.
fn exists_shell(table: &str) -> SelectStatement {
    let mut sel = Query::select();
    sel.expr_as(Expr::val(1), Alias::new("a"))
        .from(Alias::new(table.to_owned()));
    sel
}

/// Membership/role annotation (`eligibility.py:54-63`, `_member_q`).
pub fn member_exists_select() -> SelectStatement {
    let mut sel = exists_shell(WORKSPACE_MEMBER_TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("is_active")).eq(true))
            .add(
                Expr::col(Alias::new("member_id"))
                    .equals((Alias::new(loop_target::TABLE), Alias::new("user_id"))),
            )
            .add(Expr::col(Alias::new("role")).gte(Expr::col((
                Alias::new(loop_job::TABLE),
                Alias::new("min_role"),
            ))))
            .add(
                Expr::col(Alias::new("workspace_id"))
                    .equals((Alias::new(loop_target::TABLE), Alias::new("workspace_id"))),
            ),
    )
    .limit(1);
    sel
}

/// Per-job opt-out annotation (`eligibility.py:66-74`, `_job_off_q`).
pub fn job_off_exists_select() -> SelectStatement {
    preference_exists_select(false)
}

/// Master-pause annotation (`eligibility.py:77-85`, `_master_paused_q`).
pub fn master_paused_exists_select() -> SelectStatement {
    preference_exists_select(true)
}

fn preference_exists_select(master: bool) -> SelectStatement {
    let table = super::models::loop_user_preference::TABLE;
    // Conjunct order follows the Django compiler's emission (job
    // predicate before user predicate), not the Python kwargs order.
    let mut cond = Condition::all()
        .add(Expr::col(Alias::new("deleted_at")).is_null())
        .add(Expr::col(Alias::new("deleted_at")).is_null())
        .add(Expr::col(Alias::new("enabled")).eq(false));
    if master {
        cond = cond.add(Expr::col(Alias::new("job_id")).is_null());
    } else {
        cond = cond.add(
            Expr::col(Alias::new("job_id"))
                .equals((Alias::new(loop_target::TABLE), Alias::new("job_id"))),
        );
    }
    cond = cond.add(
        Expr::col(Alias::new("user_id"))
            .equals((Alias::new(loop_target::TABLE), Alias::new("user_id"))),
    );
    let mut sel = exists_shell(table);
    sel.cond_where(cond).limit(1);
    sel
}

/// LLM-credential annotation (`eligibility.py:38-42`, `llm_available_q`
/// over `OuterRef("user_id")`).
pub fn llm_exists_select() -> SelectStatement {
    let mut sel = exists_shell(LLM_CONFIG_TABLE);
    sel.cond_where(
        Condition::all()
            .add(Expr::col(Alias::new("api_key_encrypted")).is_not_null())
            .add(
                Expr::col(Alias::new("user_id"))
                    .equals((Alias::new(loop_target::TABLE), Alias::new("user_id"))),
            ),
    )
    .limit(1);
    sel
}

/// Join + scope shared by the due-target closures
/// (`eligibility.py:88-100`): enabled, non-deleted jobs; due or NULL
/// cursor (`Q(next_run_at__lte=now) | Q(next_run_at__isnull=True)`).
/// `now` renders as the `$N` bind placeholder at `now_bind`. The OR
/// is one `SimpleExpr` so the group stays parenthesized under every
/// trailing conjunct.
fn due_scope(now_bind: &str) -> Condition {
    let due_or = Expr::col((Alias::new(loop_target::TABLE), Alias::new("next_run_at")))
        .lte(Expr::cust(now_bind.to_owned()))
        .or(Expr::col((Alias::new(loop_target::TABLE), Alias::new("next_run_at"))).is_null());
    Condition::all()
        .add(Expr::col((Alias::new(loop_target::TABLE), Alias::new("deleted_at"))).is_null())
        .add(Expr::col((Alias::new(loop_target::TABLE), Alias::new("deleted_at"))).is_null())
        .add(Expr::col((Alias::new(loop_job::TABLE), Alias::new("deleted_at"))).is_null())
        .add(Expr::col((Alias::new(loop_job::TABLE), Alias::new("enabled"))).eq(true))
        .add(due_or)
}

fn join_job(sel: &mut SelectStatement) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(loop_job::TABLE.to_owned()),
        Expr::col((Alias::new(loop_target::TABLE), Alias::new("job_id")))
            .equals((Alias::new(loop_job::TABLE), Alias::new("id"))),
    );
}

fn target_columns(sel: &mut SelectStatement) {
    for col in loop_target::COLUMNS {
        sel.column((Alias::new(loop_target::TABLE), Alias::new(*col)));
    }
}

// ---------------------------------------------------------------------------
// due_targets / eligible_due_targets
// ---------------------------------------------------------------------------

/// All targets whose cursor is due (or NULL), for enabled non-deleted
/// jobs (`eligibility.py:88-100`). `$1` is `now`. Ordering follows the
/// executed fixture form (`ORDER BY "loop_targets"."id" ASC`).
pub fn due_targets_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(loop_target::TABLE.to_owned()));
    target_columns(&mut sel);
    join_job(&mut sel);
    sel.cond_where(due_scope("$1")).order_by(
        (Alias::new(loop_target::TABLE), Alias::new("id")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Due targets passing every eligibility predicate
/// (`eligibility.py:103-114`): the four annotations plus the
/// `_member=True, _job_off=False, _paused=False, _llm=True` filter,
/// which the compiler collapses to `WHERE EXISTS / NOT EXISTS`.
pub fn eligible_due_targets_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(loop_target::TABLE.to_owned()));
    target_columns(&mut sel);
    sel.expr_as(Expr::exists(member_exists_select()), Alias::new("_member"))
        .expr_as(
            Expr::exists(job_off_exists_select()),
            Alias::new("_job_off"),
        )
        .expr_as(
            Expr::exists(master_paused_exists_select()),
            Alias::new("_paused"),
        )
        .expr_as(Expr::exists(llm_exists_select()), Alias::new("_llm"));
    join_job(&mut sel);
    sel.cond_where(
        due_scope("$1")
            .add(Expr::exists(job_off_exists_select()).not())
            .add(Expr::exists(llm_exists_select()))
            .add(Expr::exists(member_exists_select()))
            .add(Expr::exists(master_paused_exists_select()).not()),
    )
    .order_by(
        (Alias::new(loop_target::TABLE), Alias::new("id")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// The exact scanner id slice (`bgtasks/loop.py:163-165`):
/// `eligible_due_targets(now).order_by("next_run_at").values_list("id")`.
/// `$1` is `now`.
pub fn eligible_due_target_ids_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(loop_target::TABLE.to_owned()));
    sel.column((Alias::new(loop_target::TABLE), Alias::new("id")));
    join_job(&mut sel);
    sel.cond_where(
        due_scope("$1")
            .add(Expr::exists(job_off_exists_select()).not())
            .add(Expr::exists(llm_exists_select()))
            .add(Expr::exists(member_exists_select()))
            .add(Expr::exists(master_paused_exists_select()).not()),
    )
    .order_by(
        (Alias::new(loop_target::TABLE), Alias::new("next_run_at")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Row-level form of `llm_available_q` (`eligibility.py:45-51`,
/// `user_has_llm`): presence check for one user. Binds `$1 = user_id`.
pub fn user_has_llm_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.expr(Expr::val(1))
        .from(Alias::new(LLM_CONFIG_TABLE.to_owned()))
        .cond_where(
            Condition::all()
                .add(Expr::col(Alias::new("user_id")).eq(Expr::cust("$1")))
                .add(usable_llm_condition()),
        )
        .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Presence check for one user (`user_has_llm`).
pub async fn fetch_user_has_llm<'e, E>(ex: E, user_id: uuid::Uuid) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&user_has_llm_sql())
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// Map one full `loop_targets` row (column names, order-independent).
pub fn map_loop_target_row(row: &PgRow) -> Result<LoopTarget, sqlx::Error> {
    Ok(LoopTarget {
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        id: row.try_get("id")?,
        job_id: row.try_get("job_id")?,
        workspace_id: row.try_get("workspace_id")?,
        user_id: row.try_get("user_id")?,
        thread_id: row.try_get("thread_id")?,
        next_run_at: row.try_get("next_run_at")?,
        last_run_id: row.try_get("last_run_id")?,
        last_skipped_at: row.try_get("last_skipped_at")?,
        last_skip_reason: row.try_get("last_skip_reason")?,
    })
}

/// Map one full `loop_jobs` row (column names, order-independent).
pub fn map_loop_job_row(row: &PgRow) -> Result<LoopJob, sqlx::Error> {
    Ok(LoopJob {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        public_name: row.try_get("public_name")?,
        public_description: row.try_get("public_description")?,
        prompt: row.try_get("prompt")?,
        min_role: row.try_get("min_role")?,
        enabled: row.try_get("enabled")?,
        is_builtin: row.try_get("is_builtin")?,
        dtstart: row.try_get("dtstart")?,
        rrule: row.try_get("rrule")?,
        tzid: row.try_get("tzid")?,
    })
}

// ---------------------------------------------------------------------------
// Settings reads (loop/views.py:24-49)
// ---------------------------------------------------------------------------

/// Off-job-id set for one user (`views.py:33-40`, `_job_enabled_map`):
/// live `enabled=False` rows with a job. Binds `$1 = user_id`.
pub fn off_job_ids_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let table = super::models::loop_user_preference::TABLE;
    let mut sel = Query::select();
    sel.from(Alias::new(table.to_owned()));
    sel.column((Alias::new(table), Alias::new("job_id")));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(table), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(table), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(table), Alias::new("enabled"))).eq(false))
            .add(Expr::col((Alias::new(table), Alias::new("job_id"))).is_not_null())
            .add(Expr::col((Alias::new(table), Alias::new("user_id"))).eq(Expr::cust("$1"))),
    )
    .order_by((Alias::new(table), Alias::new("created_at")), Order::Desc);
    sel.to_string(PostgresQueryBuilder)
}

/// Master-switch read for one user (`views.py:24-30`, `_master_enabled`):
/// the live NULL-job row's `enabled`, first row wins. Binds `$1`.
pub fn master_enabled_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let table = super::models::loop_user_preference::TABLE;
    let mut sel = Query::select();
    sel.from(Alias::new(table.to_owned()));
    sel.column((Alias::new(table), Alias::new("enabled")));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(table), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(table), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(table), Alias::new("job_id"))).is_null())
            .add(Expr::col((Alias::new(table), Alias::new("user_id"))).eq(Expr::cust("$1"))),
    )
    .order_by((Alias::new(table), Alias::new("created_at")), Order::Desc)
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Enabled-job cards in display order (`views.py:45`,
/// `order_by("public_name")`). No binds.
pub fn enabled_jobs_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(loop_job::TABLE.to_owned()));
    for col in loop_job::COLUMNS {
        sel.column((Alias::new(loop_job::TABLE), Alias::new(*col)));
    }
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(loop_job::TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(loop_job::TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(loop_job::TABLE), Alias::new("enabled"))).eq(true)),
    )
    .order_by(
        (Alias::new(loop_job::TABLE), Alias::new("public_name")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Off-job ids for one user (fixed read).
pub async fn fetch_off_job_ids<'e, E>(
    ex: E,
    user_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&off_job_ids_sql())
        .bind(user_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(|r| r.try_get("job_id")).collect()
}

/// Master `enabled` for one user; absent row reads as `true`
/// (`views.py:30`, `True if pref is None else bool(pref)`).
pub async fn fetch_master_enabled<'e, E>(ex: E, user_id: uuid::Uuid) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&master_enabled_sql())
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    Ok(row
        .map(|r| r.try_get("enabled"))
        .transpose()?
        .unwrap_or(true))
}

/// Enabled jobs in `public_name` order (fixed read).
pub async fn fetch_enabled_jobs<'e, E>(ex: E) -> Result<Vec<LoopJob>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&enabled_jobs_sql()).fetch_all(ex).await?;
    rows.iter().map(map_loop_job_row).collect()
}

// ---------------------------------------------------------------------------
// Admin detail stats (loop/admin_views.py:136-149)
// ---------------------------------------------------------------------------

/// 24h run rollup for one job's detail GET: `target_count` plus
/// completed/failed (via the last run's status + `completed_at`) and
/// skipped (via `last_skipped_at`). Binds `$1 = job_id`,
/// `$2 = since (now - 24h)`.
pub fn job_stats_sql() -> String {
    format!(
        "SELECT COUNT(\"{t}\".\"id\") AS \"target_count\", \
        COUNT(\"{t}\".\"last_run_id\") FILTER (WHERE (\"{turn}\".\"completed_at\" >= $2 AND \"{turn}\".\"status\" = 'completed')) AS \"completed\", \
        COUNT(\"{t}\".\"last_run_id\") FILTER (WHERE (\"{turn}\".\"completed_at\" >= $2 AND \"{turn}\".\"status\" = 'failed')) AS \"failed\", \
        COUNT(\"{t}\".\"id\") FILTER (WHERE \"{t}\".\"last_skipped_at\" >= $2) AS \"skipped\" \
        FROM \"{t}\" LEFT OUTER JOIN \"{turn}\" ON (\"{t}\".\"last_run_id\" = \"{turn}\".\"id\") \
        WHERE (\"{t}\".\"deleted_at\" IS NULL AND \"{t}\".\"deleted_at\" IS NULL AND \"{t}\".\"job_id\" = $1)",
        t = loop_target::TABLE,
        turn = ASSISTANT_TURN_TABLE,
    )
}

/// The 24h rollup (`admin_views.py:137-148`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopJobStats {
    pub target_count: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
}

/// Map the single aggregate row (column names, order-independent).
pub fn map_job_stats_row(row: &PgRow) -> Result<LoopJobStats, sqlx::Error> {
    Ok(LoopJobStats {
        target_count: row.try_get("target_count")?,
        completed: row.try_get("completed")?,
        failed: row.try_get("failed")?,
        skipped: row.try_get("skipped")?,
    })
}

/// 24h rollup for one job (fixed read).
pub async fn fetch_job_stats<'e, E>(
    ex: E,
    job_id: uuid::Uuid,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<LoopJobStats, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: PgRow = sqlx::query(&job_stats_sql())
        .bind(job_id)
        .bind(since)
        .fetch_one(ex)
        .await?;
    map_job_stats_row(&row)
}

// ---------------------------------------------------------------------------
// Admin targets list (loop/admin_views.py:176-210)
// ---------------------------------------------------------------------------

/// `TurnStatus` stored values (`assistant/models.py:64-70`); the
/// `status` filter applies only to one of these (`admin_views.py:195`).
pub const VALID_TURN_STATUSES: &[&str] = &["queued", "running", "completed", "failed", "cancelled"];

/// Completed/failed rollup statuses read by the stats query.
pub const TURN_STATUS_COMPLETED: &str = "completed";
/// See [`TURN_STATUS_COMPLETED`].
pub const TURN_STATUS_FAILED: &str = "failed";

/// Page size for the targets list (`admin_views.py:201`, `per = 50`).
pub const TARGETS_PAGE_SIZE: i64 = 50;

/// Filters for the targets list (`admin_views.py:187-196`). Each is
/// applied only when valid/non-empty; anything else leaves the
/// queryset unfiltered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetsFilter {
    /// Applied only when a known `SkipReason` value.
    pub skip_reason: Option<SkipReason>,
    /// Applied only when non-empty (`workspace__slug`).
    pub workspace_slug: Option<String>,
    /// Applied only when a known `TurnStatus` value
    /// (`last_run__status`).
    pub run_status: Option<String>,
}

impl TargetsFilter {
    /// Parse raw query params exactly as the view does: unknown
    /// `skip_reason`/`status` values are ignored, an empty `workspace`
    /// is ignored.
    pub fn from_params(
        skip_reason: Option<&str>,
        workspace: Option<&str>,
        status: Option<&str>,
    ) -> Self {
        Self {
            skip_reason: skip_reason.and_then(|s| s.parse::<SkipReason>().ok()),
            workspace_slug: workspace.filter(|s| !s.is_empty()).map(str::to_string),
            run_status: status
                .filter(|s| VALID_TURN_STATUSES.contains(s))
                .map(str::to_string),
        }
    }
}

/// Clamp the `page` query param (`admin_views.py:198-201`):
/// `max(1, int(page or 1))`, falling back to 1 on any parse failure.
pub fn clamp_page(raw: Option<&str>) -> i64 {
    raw.and_then(|s| s.parse::<i64>().ok())
        .map(|p| p.max(1))
        .unwrap_or(1)
}

/// `(LIMIT, OFFSET)` for a clamped page.
pub fn targets_page_window(page: i64) -> (i64, i64) {
    (TARGETS_PAGE_SIZE, (page - 1) * TARGETS_PAGE_SIZE)
}

/// Targets list for one job (`admin_views.py:183-210`):
/// `select_related` join shape, `-updated_at` order, `page/per=50`
/// slice. Binds: `$1 = job_id`, then one bind per active filter in
/// `skip_reason, workspace_slug, run_status` order.
pub fn targets_list_sql(filter: &TargetsFilter, page: i64) -> String {
    let mut where_parts = vec![
        format!(
            "\"{t}\".\"deleted_at\" IS NULL AND \"{t}\".\"deleted_at\" IS NULL AND \"{t}\".\"job_id\" = $1",
            t = loop_target::TABLE
        ),
    ];
    let mut next_bind = 2;
    if filter.skip_reason.is_some() {
        where_parts.push(format!(
            "\"{t}\".\"last_skip_reason\" = ${next_bind}",
            t = loop_target::TABLE
        ));
        next_bind += 1;
    }
    if filter.workspace_slug.is_some() {
        where_parts.push(format!("\"workspaces\".\"slug\" = ${next_bind}"));
        next_bind += 1;
    }
    if filter.run_status.is_some() {
        where_parts.push(format!(
            "\"{turn}\".\"status\" = ${next_bind}",
            turn = ASSISTANT_TURN_TABLE
        ));
        let _ = next_bind;
    }
    let (limit, offset) = targets_page_window(page);
    // Column lists mirror the fixture's select_related shape: full
    // target columns, then the joined workspace / user / last-run
    // columns Django emits for select_related("workspace", "user",
    // "last_run").
    let turn_cols = format!(
        "\"{turn}\".\"id\", \"{turn}\".\"status\", \"{turn}\".\"error_code\", \
        \"{turn}\".\"model_used\", \"{turn}\".\"usage\", \"{turn}\".\"completed_at\"",
        turn = ASSISTANT_TURN_TABLE
    );
    let mut sql = format!(
        "SELECT {t_cols}, {ws_cols}, {u_cols}, {turn_cols} \
        FROM \"{t}\" \
        INNER JOIN \"workspaces\" ON (\"{t}\".\"workspace_id\" = \"workspaces\".\"id\") \
        INNER JOIN \"users\" ON (\"{t}\".\"user_id\" = \"users\".\"id\") \
        LEFT OUTER JOIN \"{turn}\" ON (\"{t}\".\"last_run_id\" = \"{turn}\".\"id\") \
        WHERE ({where_clause}) ORDER BY \"{t}\".\"updated_at\" DESC LIMIT {limit}",
        t_cols = loop_target::COLUMNS
            .iter()
            .map(|c| format!("\"{t}\".\"{c}\"", t = loop_target::TABLE))
            .collect::<Vec<_>>()
            .join(", "),
        ws_cols = "\"workspaces\".\"id\", \"workspaces\".\"slug\"",
        u_cols = "\"users\".\"id\", \"users\".\"email\"",
        t = loop_target::TABLE,
        turn = ASSISTANT_TURN_TABLE,
        where_clause = where_parts.join(" AND "),
    );
    // Django's qs[start:start+per] omits OFFSET when start is 0.
    if offset > 0 {
        use std::fmt::Write as _;
        let _ = write!(sql, " OFFSET {offset}");
    }
    sql
}

/// Every `_row` (`admin_views.py:216-234`) input for one targets-list
/// entry: the target cursor, the joined workspace slug / user email,
/// and the last run's summary (with `usage.total_tokens`).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetListRow {
    pub id: uuid::Uuid,
    pub workspace_slug: Option<String>,
    pub user_email: Option<String>,
    pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_skipped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_skip_reason: String,
    pub last_run: Option<TargetLastRun>,
}

/// The nested `last_run` block of `_row`.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetLastRun {
    pub status: String,
    pub error_code: String,
    pub model_used: String,
    pub total_tokens: Option<i64>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/queries")
    }

    fn read_fixture(name: &str) -> String {
        let path = fixtures_dir().join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    /// SELECT statements inside a fixture file: full-line statements
    /// (comment lines starting with `--` are stripped first; several
    /// records carry prose prefixes before the statement text).
    fn fixture_statements(name: &str) -> Vec<String> {
        read_fixture(name)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| l.strip_prefix("--").unwrap_or(l).trim())
            .filter_map(|l| {
                ["SELECT ", "SELECT\"", "SELECTCOUNT", "(SELECT"]
                    .iter()
                    .filter_map(|kw| {
                        let key = kw.trim_matches('"');
                        l.find(key).map(|pos| &l[pos..])
                    })
                    .max_by_key(|s| s.len())
                    .map(|s| s.trim_end_matches(';').to_string())
            })
            .collect()
    }

    /// Normalize two SQL spellings to one semantic form:
    /// * drop parentheses (Django wraps `ON`/`WHERE` groups and
    ///   `OuterRef` columns; sea-query does not),
    /// * drop Django's `U0` subquery aliases,
    /// * number `'literal'::timestamptz` / `'literal'::uuid` casts as
    ///   `$N` binds in order (builders bind `now`/ids; the remaining
    ///   string literals — `completed`, `min_role`, … — stay, they are
    ///   semantic),
    /// * unify boolean spellings: bare `"is_active"` / `"enabled"` and
    ///   `NOT "t"."enabled"` become `= TRUE` / `= FALSE`,
    /// * `LIMIT $N` (sea-query renders limits as binds) becomes
    ///   `LIMIT 1`,
    /// * collapse whitespace.
    // Bare datetimes in the canonical compiler form (`str(qs.query)`,
    // e.g. the SCANNER block) carry no quotes or cast; number them as
    // binds too.
    fn number_bare_datetimes(sql: &str) -> String {
        let bytes = sql.as_bytes();
        let mut out = String::with_capacity(sql.len());
        let mut i = 0;
        let mut in_quote = false;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                in_quote = !in_quote;
                out.push('\'');
                i += 1;
                continue;
            }
            if !in_quote && bytes[i].is_ascii_digit() && i + 10 <= bytes.len() {
                let window = &sql[i..(i + 10).min(bytes.len())];
                let is_date = window.len() == 10
                    && window.as_bytes()[4] == b'-'
                    && window.as_bytes()[7] == b'-'
                    && window.as_bytes()[..4].iter().all(|b| b.is_ascii_digit())
                    && window.as_bytes()[5..7].iter().all(|b| b.is_ascii_digit())
                    && window.as_bytes()[8..10].iter().all(|b| b.is_ascii_digit());
                if is_date {
                    let mut j = i + 10;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_digit()
                            || matches!(bytes[j], b'-' | b':' | b'.' | b'+' | b' ' | b'T'))
                    {
                        j += 1;
                    }
                    out.push_str(" B I N D ");
                    i = j;
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn normalize(sql: &str) -> String {
        let mut out = number_bare_datetimes(&sql.replace(['(', ')'], " "));
        // Drop Django's compiler-generated subquery alias.
        out = out.replace(" U0.", " ").replace(" U0 ", " ");
        // Number cast literals as binds.
        let mut numbered = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(start) = rest.find('\'') {
            let tail = &rest[start + 1..];
            let Some(end) = tail.find('\'') else {
                break;
            };
            let after = tail[end + 1..].trim_start();
            let cast_len = if after.starts_with("::timestamptz") {
                "::timestamptz".len()
            } else if after.starts_with("::uuid") {
                "::uuid".len()
            } else {
                0
            };
            if cast_len > 0 {
                numbered.push_str(&rest[..start]);
                // Sentinel: renumbered with the `$N` binds below in
                // encounter order, so builder bind numbering choices
                // never matter, only positions.
                numbered.push_str(" B I N D ");
                rest = after[cast_len..].trim_start();
            } else {
                // Plain string literal: keep it, it is semantic.
                numbered.push_str(&rest[..start + end + 2]);
                rest = &tail[end + 1..];
            }
        }
        numbered.push_str(rest);
        let mut out = numbered;
        // Unify boolean spellings (both qualified and bare forms).
        for col in [
            "\"loop_jobs\".\"enabled\"",
            "\"loop_user_preferences\".\"enabled\"",
            "\"enabled\"",
            "\"workspace_members\".\"is_active\"",
            "\"is_active\"",
        ] {
            out = out.replace(&format!("NOT {col}"), &format!("{col} = FALSE"));
        }
        out = out.replace("\"is_active\" = TRUE", "\"is_active\"");
        out = out
            .replace(
                "\"loop_jobs\".\"enabled\" = TRUE",
                "\"loop_jobs\".\"enabled\"",
            )
            .replace("\"enabled\" = TRUE", "\"enabled\"");
        // sea-query renders LIMIT values as binds.
        let mut flat = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(pos) = rest.find("LIMIT $") {
            flat.push_str(&rest[..pos]);
            flat.push_str("LIMIT 1");
            let tail = &rest[pos + "LIMIT $".len()..];
            let digits = tail.chars().take_while(|c| c.is_ascii_digit()).count();
            rest = &tail[digits..];
        }
        flat.push_str(rest);
        // Renumber every bind (builder `$N` and literal sentinels) in
        // encounter order.
        let mut renumbered = String::with_capacity(flat.len());
        let mut rest = flat.as_str();
        let mut bind = 1;
        loop {
            let dollar = rest.find('$');
            let sentinel = rest.find("B I N D");
            match (dollar, sentinel) {
                (None, None) => {
                    renumbered.push_str(rest);
                    break;
                }
                (d, s) => {
                    let use_dollar = match (d, s) {
                        (Some(a), Some(b)) => a < b,
                        (Some(_), None) => true,
                        _ => false,
                    };
                    if use_dollar {
                        let pos = d.unwrap();
                        renumbered.push_str(&rest[..pos]);
                        let tail = &rest[pos + 1..];
                        let digits = tail.chars().take_while(|c| c.is_ascii_digit()).count();
                        renumbered.push_str(&format!("${bind}"));
                        bind += 1;
                        rest = &tail[digits..];
                    } else {
                        let pos = s.unwrap();
                        renumbered.push_str(&rest[..pos]);
                        renumbered.push_str(&format!("${bind}"));
                        bind += 1;
                        rest = &rest[pos + "B I N D".len()..];
                    }
                }
            }
        }
        renumbered.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn assert_semantic_eq(got: &str, fixture_stmt: &str) {
        assert_eq!(
            normalize(got),
            normalize(fixture_stmt),
            "semantic mismatch:\n got: {got}\nfixture: {fixture_stmt}"
        );
    }

    #[test]
    fn due_targets_matches_fixture() {
        let stmts = fixture_statements("due_targets.sql");
        let full = stmts
            .iter()
            .find(|s| s.contains("loop_targets"))
            .expect("due fixture has a target statement");
        assert_semantic_eq(&due_targets_sql(), full);
    }

    #[test]
    fn eligible_due_targets_matches_fixture() {
        let stmts = fixture_statements("eligible_due_targets.sql");
        let full = stmts
            .iter()
            .find(|s| s.contains("_member") && s.contains("FROM"))
            .expect("eligible fixture has the annotated statement");
        assert_semantic_eq(&eligible_due_targets_sql(), full);
    }

    #[test]
    fn eligible_scanner_ids_match_fixture() {
        let stmts = fixture_statements("eligible_due_targets.sql");
        let scanner = stmts
            .iter()
            .find(|s| s.contains("ORDER BY \"loop_targets\".\"next_run_at\" ASC"))
            .expect("eligible fixture has the SCANNER statement");
        assert_semantic_eq(&eligible_due_target_ids_sql(), scanner);
    }

    #[test]
    fn eligible_carries_all_four_exists_annotations() {
        let sql = eligible_due_targets_sql();
        for ann in ["_member", "_job_off", "_paused", "_llm"] {
            assert!(
                sql.contains(&format!("AS \"{ann}\"")),
                "missing annotation {ann}: {sql}"
            );
        }
        // The filter half collapses annotations to WHERE EXISTS forms.
        assert!(
            sql.contains("NOT EXISTS"),
            "missing NOT EXISTS filter: {sql}"
        );
        assert!(sql.contains("EXISTS"), "missing EXISTS filter: {sql}");
        // role__gte column comparison survived.
        assert!(
            sql.contains("\"role\" >= \"loop_jobs\".\"min_role\"")
                || sql.contains("\"role\" >= (\"loop_jobs\".\"min_role\")")
                || normalize(&sql).contains("\"role\" >= \"loop_jobs\" . \"min_role\""),
            "missing role__gte comparison: {sql}"
        );
        // Usable-LLM predicate shared with the row check.
        assert!(
            sql.contains("\"api_key_encrypted\" IS NOT NULL"),
            "missing LLM filter: {sql}"
        );
    }

    #[test]
    fn settings_reads_match_fixture() {
        let stmts = fixture_statements("settings_payload_reads.sql");
        assert_eq!(
            stmts.len(),
            3,
            "settings fixture records 3 reads: {stmts:?}"
        );
        let off = stmts
            .iter()
            .find(|s| s.contains("SELECT \"loop_user_preferences\".\"job_id\""))
            .expect("off-job read");
        let master = stmts
            .iter()
            .find(|s| s.contains("SELECT \"loop_user_preferences\".\"enabled\""))
            .expect("master read");
        let jobs = stmts
            .iter()
            .find(|s| s.contains("\"slug\""))
            .expect("job list read");
        assert_semantic_eq(&off_job_ids_sql(), off);
        assert_semantic_eq(&master_enabled_sql(), master);
        assert_semantic_eq(&enabled_jobs_sql(), jobs);
        // Jobs come back in public_name order; absent master row = enabled.
        assert!(enabled_jobs_sql().contains("ORDER BY \"loop_jobs\".\"public_name\" ASC"));
    }

    #[test]
    fn admin_stats_match_fixture() {
        let stmts = fixture_statements("admin_stats_24h.sql");
        let agg = stmts
            .iter()
            .find(|s| s.contains("target_count"))
            .expect("stats aggregate statement");
        assert_semantic_eq(&job_stats_sql(), agg);
    }

    #[test]
    fn targets_list_filters_match_fixture() {
        let text = read_fixture("targets_list.sql");
        let unfiltered = targets_list_sql(&TargetsFilter::default(), 1);
        assert!(
            normalize(&text).contains(&normalize(
                "ORDER BY \"loop_targets\".\"updated_at\" DESC LIMIT 50"
            )),
            "fixture orders by -updated_at with per=50"
        );
        assert!(unfiltered.contains("ORDER BY \"loop_targets\".\"updated_at\" DESC"));
        // Django's qs[0:50] emits LIMIT with no OFFSET.
        assert!(unfiltered.contains("LIMIT 50"));
        assert!(!unfiltered.contains("OFFSET"));
        // skip_reason filter applies only for known values.
        let with_reason =
            targets_list_sql(&TargetsFilter::from_params(Some("min_role"), None, None), 1);
        assert!(with_reason.contains("\"last_skip_reason\" = $2"));
        assert!(
            text.contains("\"last_skip_reason\" = 'min_role'"),
            "fixture records the skip_reason filter"
        );
        // Unknown filter values leave the queryset unfiltered.
        assert_eq!(
            targets_list_sql(
                &TargetsFilter::from_params(Some("bogus"), Some(""), Some("bogus")),
                1
            ),
            unfiltered
        );
        // Workspace slug + run status filters.
        let with_both = targets_list_sql(
            &TargetsFilter::from_params(None, Some("fxloop-ws"), Some("completed")),
            2,
        );
        assert!(with_both.contains("\"workspaces\".\"slug\" = $2"));
        assert!(with_both.contains("\"assistant_turn\".\"status\" = $3"));
        assert!(with_both.contains("LIMIT 50 OFFSET 50"));
    }

    #[test]
    fn page_clamp_matches_view() {
        assert_eq!(clamp_page(None), 1);
        assert_eq!(clamp_page(Some("bogus")), 1);
        assert_eq!(clamp_page(Some("0")), 1);
        assert_eq!(clamp_page(Some("-3")), 1);
        assert_eq!(clamp_page(Some("2")), 2);
        assert_eq!(targets_page_window(1), (50, 0));
        assert_eq!(targets_page_window(3), (50, 100));
    }

    #[test]
    fn fixture_rows_replay_documented_sets() {
        // due_targets.rows.json: two due edges (member + guest); NULL
        // cursors count as due per eligibility.py:97.
        let due: serde_json::Value =
            serde_json::from_str(&read_fixture("due_targets.rows.json")).expect("due rows parse");
        assert_eq!(due["rows"].as_array().unwrap().len(), 2);
        // eligible rows: member passes, guest excluded with min_role —
        // the same verdict check() must return (precedence vectors live
        // in pidash-services eligibility tests).
        let elig: serde_json::Value =
            serde_json::from_str(&read_fixture("eligible_due_targets.rows.json"))
                .expect("eligible rows parse");
        assert_eq!(elig["rows"].as_array().unwrap().len(), 1);
        assert_eq!(elig["rows"][0]["user_email"], "fxloop-member@e.com");
        assert_eq!(elig["excluded"][0]["check"], "min_role");
        // Settings payload: absent master row reads enabled with one card.
        let settings: serde_json::Value =
            serde_json::from_str(&read_fixture("settings_payload_reads.rows.json"))
                .expect("settings rows parse");
        assert_eq!(settings["payload"]["enabled"], true);
        assert_eq!(settings["payload"]["jobs"].as_array().unwrap().len(), 1);
        // Admin stats rollup shape.
        let stats: serde_json::Value =
            serde_json::from_str(&read_fixture("admin_stats_24h.rows.json")).expect("stats parse");
        assert_eq!(stats["stats"]["target_count"], 3);
        // Targets list: unfiltered page of 3, -updated_at order.
        let targets: serde_json::Value =
            serde_json::from_str(&read_fixture("targets_list.rows.json")).expect("targets parse");
        assert_eq!(
            targets["unfiltered"]["results"].as_array().unwrap().len(),
            3
        );
        assert_eq!(targets["page_size"], 50);
    }

    #[test]
    fn user_has_llm_sql_checks_stored_key() {
        let sql = normalize(&user_has_llm_sql());
        assert!(sql.contains("\"api_key_encrypted\" IS NOT NULL"), "{sql}");
        assert!(sql.contains("\"user_id\" = $1"), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }
}
