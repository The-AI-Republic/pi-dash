//! Recent-visit + page-transaction tasks (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry points of
//! `apps/api/pi_dash/bgtasks/recent_visited_task.py:17-61`
//! (`recent_visited_task`) and
//! `apps/api/pi_dash/bgtasks/page_transaction_task.py:84-142`
//! (`page_transaction`). The pure component extraction both tasks share
//! lives in `pidash-services` (`tasks_webhooks::page_extract`).
//! Fixtures: `FX-VISIT-01`, `FX-PAGE-01`
//! (`rust-api/fixtures/tasks_webhooks/fx-visit-01-*.json`,
//! `fx-page-01-*.json`).
//!
//! This module owns the Celery wire surface (the two task names, the
//! `.delay()` payload constructors, the positional-or-keyword arg
//! binding) and registers [`Registry`] handlers that run the same SQL
//! the Django ORM renders, statement by statement (no wrapping
//! transaction: the ORM calls autocommit individually).
//!
//! Ported quirks (translate, don't redesign):
//!
//! * QUIRK-1 (`recent_visited_task.py:38`): eviction fires only when the
//!   live-row count is EXACTLY 20 (`==`, not `>=`); 21+ rows never evict.
//!   [`needs_eviction`] keeps the `==`.
//! * QUIRK-2 (`page_transaction_task.py:112`): a new mention whose id is
//!   already tracked is skipped ONLY when page logs already exist; the
//!   first-ever run re-inserts every id. [`plan_page_logs`] keeps the
//!   `has_existing_logs` gate.
//! * QUIRK-3: eviction `victim.delete()` is an instance delete, i.e. a
//!   SOFT delete (`SoftDeleteModel.delete(soft=True)` renders
//!   `UPDATE ... SET deleted_at`, plus the `auto_now` bumps and the
//!   in-memory audit nulling of the accompanying full `save()`), not a
//!   hard `DELETE`. [`evict_oldest`] ports that exact column set.
//! * QUIRK-4: the `PageLog` cleanup
//!   (`filter(transaction__in=...).delete()`) is a queryset delete, i.e.
//!   a soft `UPDATE deleted_at` over the soft-filtered set, not a hard
//!   `DELETE`. [`delete_page_logs`] ports it.
//! * QUIRK-5 (`recent_visited_task.py:53-55`): the create-path backfill
//!   `save(update_fields=["created_by_id", "updated_by_id"])` routes
//!   through `BaseModel.save` (`db/models/base.py`), which re-stamps
//!   audit from `crum.get_current_user()` — `None` in a worker — wiping
//!   the task's `user_id` assignments before the `UPDATE` renders. The
//!   live second statement is therefore `SET created_by_id = NULL,
//!   updated_by_id = NULL`, not `= user_id` (same trap as
//!   `bgtasks/github_sync_task.py:139-156`). [`create_recent_visit`]
//!   keeps the two-statement shape and binds NULLs.
//!
//! Deliberate transport note (documented, not a bug): the eviction's
//! `soft_delete_related_objects.delay(...)` cascade has no observable DB
//! effect here (`UserRecentVisit` carries no reverse relations) and the
//! handler owns no broker handle, so it is a NO-OP (D-26 precedent for
//! `.delay()` call sites inside ported tasks). Ownership of
//! `deletion_task` stays Python-owned.
//!
//! Ack parity: plain `@shared_task` means ack-on-success with no
//! overrides. Every ported control path ends in `return` (success or
//! logged swallow), so the drivers always resolve to [`Verdict::Ack`];
//! only an unbindable payload fails, which the worker settles into
//! requeue-with-budget exactly like the F-09 mechanism does for every
//! handler.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::Pools;
use pidash_services::tasks_webhooks::page_extract::{
    self, EntityDetails, ImageAttrs, MentionAttrs,
};

use crate::celery::CeleryTaskMessage;
use crate::worker::{Handler, Registry, Verdict};

/// `recent_visited_task` (`recent_visited_task.py:17`).
pub const RECENT_VISITED_TASK_NAME: &str =
    "pi_dash.bgtasks.recent_visited_task.recent_visited_task";

/// `page_transaction` (`page_transaction_task.py:84`).
pub const PAGE_TRANSACTION_TASK_NAME: &str =
    "pi_dash.bgtasks.page_transaction_task.page_transaction";

/// `recent_visited_task.delay(slug, entity_name, entity_identifier,
/// user_id, project_id)`: the Django call sites pass everything as
/// keywords, so the constructor emits kwargs in signature order.
pub fn recent_visited_task_message(
    entity_name: &str,
    entity_identifier: Option<&str>,
    user_id: &str,
    project_id: Option<&str>,
    slug: &str,
) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert(
        "entity_name".to_owned(),
        Value::String(entity_name.to_owned()),
    );
    kwargs.insert(
        "entity_identifier".to_owned(),
        entity_identifier.map_or(Value::Null, |v| Value::String(v.to_owned())),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    kwargs.insert(
        "project_id".to_owned(),
        project_id.map_or(Value::Null, |v| Value::String(v.to_owned())),
    );
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    CeleryTaskMessage::new(RECENT_VISITED_TASK_NAME, Vec::new(), kwargs)
}

/// `page_transaction.delay(new_description_html, old_description_html,
/// page_id)`: keyword form, as the page views call it.
pub fn page_transaction_message(
    new_description_html: Option<&str>,
    old_description_html: Option<&str>,
    page_id: &str,
) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert(
        "new_description_html".to_owned(),
        new_description_html.map_or(Value::Null, |v| Value::String(v.to_owned())),
    );
    kwargs.insert(
        "old_description_html".to_owned(),
        old_description_html.map_or(Value::Null, |v| Value::String(v.to_owned())),
    );
    kwargs.insert("page_id".to_owned(), Value::String(page_id.to_owned()));
    CeleryTaskMessage::new(PAGE_TRANSACTION_TASK_NAME, Vec::new(), kwargs)
}

struct RecentVisitedArgs {
    entity_name: String,
    entity_identifier: Option<String>,
    user_id: String,
    project_id: Option<String>,
    slug: String,
}

struct PageTransactionArgs {
    new_description_html: Option<String>,
    old_description_html: Option<String>,
    page_id: String,
}

fn opt_str(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!("expected string-or-null, got {other}")),
    }
}

fn req_str(value: Option<&Value>, name: &str) -> Result<String, String> {
    match value {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(format!("{name} must be a string")),
    }
}

/// Bind `recent_visited_task` payloads. Celery binds positionally first,
/// then by keyword: five positional args
/// `(entity_name, entity_identifier, user_id, project_id, slug)` or the
/// same five keywords. Anything else is a `TypeError` in Python, i.e. a
/// handler failure here.
fn bind_recent_visited(args: &Value, kwargs: &Value) -> Result<RecentVisitedArgs, String> {
    let args = args.as_array().ok_or("args must be an array")?;
    let kwargs = kwargs.as_object().ok_or("kwargs must be an object")?;
    if !args.is_empty() {
        if args.len() != 5 || !kwargs.is_empty() {
            return Err(format!(
                "{} takes 5 positional arguments",
                RECENT_VISITED_TASK_NAME
            ));
        }
        return Ok(RecentVisitedArgs {
            entity_name: req_str(args.first(), "entity_name")?,
            entity_identifier: opt_str(args.get(1))?,
            user_id: req_str(args.get(2), "user_id")?,
            project_id: opt_str(args.get(3))?,
            slug: req_str(args.get(4), "slug")?,
        });
    }
    Ok(RecentVisitedArgs {
        entity_name: req_str(kwargs.get("entity_name"), "entity_name")?,
        entity_identifier: opt_str(kwargs.get("entity_identifier"))?,
        user_id: req_str(kwargs.get("user_id"), "user_id")?,
        project_id: opt_str(kwargs.get("project_id"))?,
        slug: req_str(kwargs.get("slug"), "slug")?,
    })
}

/// Bind `page_transaction` payloads: three positional args
/// `(new_description_html, old_description_html, page_id)` or the same
/// three keywords.
fn bind_page_transaction(args: &Value, kwargs: &Value) -> Result<PageTransactionArgs, String> {
    let args = args.as_array().ok_or("args must be an array")?;
    let kwargs = kwargs.as_object().ok_or("kwargs must be an object")?;
    if !args.is_empty() {
        if args.len() != 3 || !kwargs.is_empty() {
            return Err(format!(
                "{} takes 3 positional arguments",
                PAGE_TRANSACTION_TASK_NAME
            ));
        }
        return Ok(PageTransactionArgs {
            new_description_html: opt_str(args.first())?,
            old_description_html: opt_str(args.get(1))?,
            page_id: req_str(args.get(2), "page_id")?,
        });
    }
    Ok(PageTransactionArgs {
        new_description_html: opt_str(kwargs.get("new_description_html"))?,
        old_description_html: opt_str(kwargs.get("old_description_html"))?,
        page_id: req_str(kwargs.get("page_id"), "page_id")?,
    })
}

/// Eviction gate (`recent_visited_task.py:38`): the count comparison is
/// `== 20` exactly (QUIRK-1). Pure so the quirk is pinned without a DB.
pub fn needs_eviction(live_row_count: i64) -> bool {
    live_row_count == 20
}

/// One `PageLog` row the planner wants inserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPageLog {
    pub transaction: String,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub entity_identifier: Option<String>,
}

/// The `page_transaction` diff: rows to bulk-insert plus the global
/// `transaction__in` set to soft-delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLogPlan {
    pub inserts: Vec<PlannedPageLog>,
    pub deleted_transaction_ids: Vec<String>,
}

fn plan_component(
    new_entities: &[(Option<String>, EntityDetails)],
    old_entities: &[Option<String>],
    has_existing_logs: bool,
    inserts: &mut Vec<PlannedPageLog>,
    deleted: &mut HashSet<String>,
) {
    let old_ids: HashSet<&str> = old_entities
        .iter()
        .filter_map(|id| id.as_deref().filter(|id| !id.is_empty()))
        .collect();
    let new_ids: HashSet<&str> = new_entities
        .iter()
        .filter_map(|(id, _)| id.as_deref().filter(|id| !id.is_empty()))
        .collect();
    deleted.extend(old_ids.difference(&new_ids).map(|id| (*id).to_owned()));
    for (mention_id, details) in new_entities {
        let Some(mention_id) = mention_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        // QUIRK-2: the backfill skip applies only when logs exist; the
        // first-ever run re-inserts every tracked id.
        if old_ids.contains(mention_id) && has_existing_logs {
            continue;
        }
        inserts.push(PlannedPageLog {
            transaction: mention_id.to_owned(),
            entity_name: details.entity_name.clone(),
            entity_type: details.entity_type.clone(),
            entity_identifier: details.entity_identifier.clone(),
        });
    }
}

/// Pure `page_transaction` diff over the extracted components
/// (`page_transaction_task.py:99-129`): per component, `deleted +=
/// old_ids - new_ids` (one GLOBAL set across components), and every new
/// entity with a truthy id becomes a `PageLog` row unless the QUIRK-2
/// skip applies.
pub fn plan_page_logs(
    new_mentions: &[MentionAttrs],
    new_images: &[ImageAttrs],
    old_mentions: &[MentionAttrs],
    old_images: &[ImageAttrs],
    has_existing_logs: bool,
) -> PageLogPlan {
    let mut inserts = Vec::new();
    let mut deleted = HashSet::new();
    plan_component(
        &new_mentions
            .iter()
            .map(|m| (m.id.clone(), page_extract::mention_details(m)))
            .collect::<Vec<_>>(),
        &old_mentions
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        has_existing_logs,
        &mut inserts,
        &mut deleted,
    );
    plan_component(
        &new_images
            .iter()
            .map(|i| (i.id.clone(), page_extract::image_details(i)))
            .collect::<Vec<_>>(),
        &old_images.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
        has_existing_logs,
        &mut inserts,
        &mut deleted,
    );
    let mut deleted_transaction_ids: Vec<String> = deleted.into_iter().collect();
    deleted_transaction_ids.sort();
    PageLogPlan {
        inserts,
        deleted_transaction_ids,
    }
}

/// Driver failure: `Quiet` mirrors the bare `except Page.DoesNotExist:
/// return` (ack, no log); `Log` mirrors `log_exception(e)` + return
/// (ack after logging). Both settle to [`Verdict::Ack`].
enum DriverFailure {
    Quiet,
    Log(String),
}

type DriverResult<T> = Result<T, DriverFailure>;

/// `Workspace.objects.get(slug=slug)`: the default manager soft-filters,
/// `get` needs exactly one row.
async fn workspace_id_by_slug(pool: &PgPool, slug: &str) -> DriverResult<Uuid> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_all(pool)
            .await
            .map_err(|error| DriverFailure::Log(format!("workspace lookup: {error}")))?;
    match rows.as_slice() {
        [(id,)] => Ok(*id),
        _ => Err(DriverFailure::Log(
            "workspace lookup: no single workspace for slug".to_owned(),
        )),
    }
}

/// The update-path lookup
/// (`filter(entity_name, entity_identifier, user_id, project_id,
/// workspace).first()`): soft-filtered, `None` renders as `IS NULL`
/// (Django `None` exact lookup). `Meta.ordering = ("-created_at",)`
/// (`recent_visit.py`) plus `.first()` renders
/// `ORDER BY created_at DESC LIMIT 1`.
async fn find_recent_visit(
    pool: &PgPool,
    entity_name: &str,
    entity_identifier: Option<&str>,
    user_id: &str,
    project_id: Option<&str>,
    workspace_id: Uuid,
) -> DriverResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = match (entity_identifier, project_id) {
        (None, None) => sqlx::query_as(
            "SELECT id FROM user_recent_visits WHERE entity_name = $1 AND entity_identifier IS NULL \
             AND user_id = $2::uuid AND project_id IS NULL AND workspace_id = $3 \
             AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .bind(entity_name)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DriverFailure::Log(format!("recent-visit lookup: {error}")))?,
        (Some(entity_identifier), None) => sqlx::query_as(
            "SELECT id FROM user_recent_visits WHERE entity_name = $1 AND entity_identifier = $2::uuid \
             AND user_id = $3::uuid AND project_id IS NULL AND workspace_id = $4 \
             AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .bind(entity_name)
        .bind(entity_identifier)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DriverFailure::Log(format!("recent-visit lookup: {error}")))?,
        (None, Some(project_id)) => sqlx::query_as(
            "SELECT id FROM user_recent_visits WHERE entity_name = $1 AND entity_identifier IS NULL \
             AND user_id = $2::uuid AND project_id = $3::uuid AND workspace_id = $4 \
             AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .bind(entity_name)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DriverFailure::Log(format!("recent-visit lookup: {error}")))?,
        (Some(entity_identifier), Some(project_id)) => sqlx::query_as(
            "SELECT id FROM user_recent_visits WHERE entity_name = $1 AND entity_identifier = $2::uuid \
             AND user_id = $3::uuid AND project_id = $4::uuid AND workspace_id = $5 \
             AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .bind(entity_name)
        .bind(entity_identifier)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DriverFailure::Log(format!("recent-visit lookup: {error}")))?,
    };
    Ok(row.map(|(id,)| id))
}

/// `recent_visited.visited_at = now(); save(update_fields=["visited_at"])`
/// renders `UPDATE ... SET visited_at` only (verified against Django 4.2
/// `_save_table`: `update_fields` restricts the written columns; the
/// `auto_now` `updated_at` is NOT bumped). A database error is swallowed
/// silently (`except DatabaseError: pass`); any other failure logs.
async fn touch_visited_at(pool: &PgPool, id: Uuid) -> DriverResult<()> {
    let now = Utc::now();
    match sqlx::query("UPDATE user_recent_visits SET visited_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
    {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(_)) => Ok(()),
        Err(error) => Err(DriverFailure::Log(format!("recent-visit touch: {error}"))),
    }
}

async fn live_visit_count(pool: &PgPool, user_id: &str, workspace_id: Uuid) -> DriverResult<i64> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM user_recent_visits WHERE user_id = $1::uuid \
         AND workspace_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("recent-visit count: {error}")))?;
    Ok(count)
}

/// Eviction (`:39-44` + QUIRK-3): oldest live row by `created_at`, then
/// the instance-delete full-write (`deleted_at` set, `auto_now` bumps of
/// `visited_at`/`updated_at`, audit nulled by the worker-context
/// `save()`). A vanished victim (race) mirrors the `AttributeError`
/// path: log and skip the create.
async fn evict_oldest(pool: &PgPool, user_id: &str, workspace_id: Uuid) -> DriverResult<()> {
    let victim: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM user_recent_visits WHERE user_id = $1::uuid AND workspace_id = $2 \
         AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("recent-visit victim: {error}")))?;
    let Some((victim,)) = victim else {
        return Err(DriverFailure::Log(
            "recent-visit eviction: no victim row".to_owned(),
        ));
    };
    let now = Utc::now();
    sqlx::query(
        "UPDATE user_recent_visits SET deleted_at = $1, visited_at = $1, updated_at = $1, \
         created_by_id = NULL, updated_by_id = NULL WHERE id = $2",
    )
    .bind(now)
    .bind(victim)
    .execute(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("recent-visit evict: {error}")))?;
    Ok(())
}

/// Create path (`:46-56` + QUIRK-5): full `INSERT` (audit `NULL`, both
/// timestamps `now()`), then the backfill second write. The Python
/// assigns `user_id` to both audit fields, but the accompanying
/// `save()` re-stamps them from `crum.get_current_user()` (`None` in a
/// worker), so the rendered `UPDATE` binds `NULL, NULL` — ported here
/// exactly, two-statement shape kept.
async fn create_recent_visit(
    pool: &PgPool,
    entity_name: &str,
    entity_identifier: Option<&str>,
    user_id: &str,
    project_id: Option<&str>,
    workspace_id: Uuid,
) -> DriverResult<()> {
    let now = Utc::now();
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO user_recent_visits (id, created_at, updated_at, created_by_id, updated_by_id, \
         deleted_at, workspace_id, project_id, entity_identifier, entity_name, user_id, visited_at) \
         VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4::uuid, $5::uuid, $6, $7::uuid, $2)",
    )
    .bind(id)
    .bind(now)
    .bind(workspace_id)
    .bind(project_id)
    .bind(entity_identifier)
    .bind(entity_name)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("recent-visit create: {error}")))?;
    sqlx::query(
        "UPDATE user_recent_visits SET created_by_id = NULL, updated_by_id = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("recent-visit backfill: {error}")))?;
    Ok(())
}

async fn run_recent_visited(pool: &PgPool, args: RecentVisitedArgs) -> DriverResult<()> {
    let workspace_id = workspace_id_by_slug(pool, &args.slug).await?;
    let existing = find_recent_visit(
        pool,
        &args.entity_name,
        args.entity_identifier.as_deref(),
        &args.user_id,
        args.project_id.as_deref(),
        workspace_id,
    )
    .await?;
    if let Some(id) = existing {
        return touch_visited_at(pool, id).await;
    }
    if needs_eviction(live_visit_count(pool, &args.user_id, workspace_id).await?) {
        // A vanished victim aborts before the create (the Python
        // `AttributeError` unwinds to the outer handler first).
        if evict_oldest(pool, &args.user_id, workspace_id)
            .await
            .is_err()
        {
            return Err(DriverFailure::Log(
                "recent-visit eviction: no victim row".to_owned(),
            ));
        }
    }
    create_recent_visit(
        pool,
        &args.entity_name,
        args.entity_identifier.as_deref(),
        &args.user_id,
        args.project_id.as_deref(),
        workspace_id,
    )
    .await
}

async fn run_page_transaction(pool: &PgPool, args: PageTransactionArgs) -> DriverResult<()> {
    // `Page.objects.get(pk=page_id)`: an unparseable pk is a
    // `ValidationError` (logged); a missing row is `DoesNotExist`
    // (bare return).
    let page_id: Uuid = args
        .page_id
        .parse()
        .map_err(|_| DriverFailure::Log("page_transaction: invalid page_id".to_owned()))?;
    let page: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT id, workspace_id FROM pages WHERE id = $1 AND deleted_at IS NULL")
            .bind(page_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| {
                DriverFailure::Log(format!("page_transaction: page lookup: {error}"))
            })?;
    let Some((_, workspace_id)) = page else {
        return Err(DriverFailure::Quiet);
    };

    let (has_existing_logs,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM page_logs WHERE page_id = $1 AND deleted_at IS NULL)",
    )
    .bind(page_id)
    .fetch_one(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("page_transaction: logs check: {error}")))?;

    let old = page_extract::extract_all_components(args.old_description_html.as_deref());
    let new = page_extract::extract_all_components(args.new_description_html.as_deref());
    let plan = plan_page_logs(
        &new.mention_component,
        &new.image_component,
        &old.mention_component,
        &old.image_component,
        has_existing_logs,
    );

    // `bulk_create(..., batch_size=50, ignore_conflicts=True)`: skipped
    // when empty; `ON CONFLICT DO NOTHING` over unique(page,
    // transaction) makes re-inserts silent no-ops.
    let now = Utc::now();
    for chunk in plan.inserts.chunks(50) {
        let mut builder = sqlx::QueryBuilder::new(
            "INSERT INTO page_logs (id, created_at, updated_at, transaction, page_id, \
             entity_identifier, entity_name, entity_type, workspace_id) ",
        );
        builder.push_values(chunk, |mut row, item| {
            row.push_bind(Uuid::new_v4())
                .push_bind(now)
                .push_bind(now)
                .push_bind(item.transaction.clone())
                .push_bind(page_id)
                .push_bind(item.entity_identifier.clone())
                .push_bind(item.entity_name.clone())
                .push_bind(item.entity_type.clone())
                .push_bind(workspace_id);
        });
        builder.push(" ON CONFLICT DO NOTHING");
        builder
            .build()
            .execute(pool)
            .await
            .map_err(|error| DriverFailure::Log(format!("page_transaction: insert: {error}")))?;
    }

    // Cleanup (`:135-136` + QUIRK-4): ONE global soft-delete over the
    // union set, skipped when empty.
    delete_page_logs(pool, &plan.deleted_transaction_ids).await?;

    Ok(())
}

/// QUIRK-4 driver: soft `UPDATE deleted_at` over the soft-filtered
/// `transaction__in` set. Binding the ids as text lets Postgres cast to
/// the `uuid` column exactly like the Django-rendered SQL (a garbage id
/// errors into the outer handler there and here).
async fn delete_page_logs(pool: &PgPool, transaction_ids: &[String]) -> DriverResult<()> {
    if transaction_ids.is_empty() {
        return Ok(());
    }
    let now = Utc::now();
    sqlx::query(
        "UPDATE page_logs SET deleted_at = $1 WHERE transaction::text = ANY($2) \
         AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(transaction_ids)
    .execute(pool)
    .await
    .map_err(|error| DriverFailure::Log(format!("page_transaction: cleanup: {error}")))?;
    Ok(())
}

/// Register both D-08 visit/page handlers (ownership flips to Rust the
/// moment these names register; unregistered names still forward to
/// Python).
pub fn register_visit_page_handlers(registry: &mut Registry, pools: Pools) {
    let recent_pools = pools.clone();
    let recent_handler: Handler = Arc::new(move |job| {
        let pools = recent_pools.clone();
        Box::pin(async move {
            if !job.args.is_array() || !job.kwargs.is_object() {
                return Err(format!(
                    "{RECENT_VISITED_TASK_NAME}: unexpected job payload shape"
                ));
            }
            let args = bind_recent_visited(&job.args, &job.kwargs)
                .map_err(|error| format!("{RECENT_VISITED_TASK_NAME}: invalid payload: {error}"))?;
            match run_recent_visited(pools.primary(), args).await {
                Ok(()) => Ok(Verdict::Ack),
                Err(DriverFailure::Quiet) => Ok(Verdict::Ack),
                Err(DriverFailure::Log(error)) => {
                    tracing::error!(task = RECENT_VISITED_TASK_NAME, error = %error, "task failed");
                    Ok(Verdict::Ack)
                }
            }
        })
    });
    registry.register(RECENT_VISITED_TASK_NAME, recent_handler);

    let page_pools = pools.clone();
    let page_handler: Handler = Arc::new(move |job| {
        let pools = page_pools.clone();
        Box::pin(async move {
            if !job.args.is_array() || !job.kwargs.is_object() {
                return Err(format!(
                    "{PAGE_TRANSACTION_TASK_NAME}: unexpected job payload shape"
                ));
            }
            let args = bind_page_transaction(&job.args, &job.kwargs).map_err(|error| {
                format!("{PAGE_TRANSACTION_TASK_NAME}: invalid payload: {error}")
            })?;
            match run_page_transaction(pools.primary(), args).await {
                Ok(()) => Ok(Verdict::Ack),
                Err(DriverFailure::Quiet) => Ok(Verdict::Ack),
                Err(DriverFailure::Log(error)) => {
                    tracing::error!(task = PAGE_TRANSACTION_TASK_NAME, error = %error, "task failed");
                    Ok(Verdict::Ack)
                }
            }
        })
    });
    registry.register(PAGE_TRANSACTION_TASK_NAME, page_handler);
}

/// True for the two D-08 visit/page task names.
pub fn is_visit_page_task(task: &str) -> bool {
    task == RECENT_VISITED_TASK_NAME || task == PAGE_TRANSACTION_TASK_NAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mention(id: Option<&str>, identifier: Option<&str>, name: Option<&str>) -> MentionAttrs {
        MentionAttrs {
            id: id.map(str::to_owned),
            entity_identifier: identifier.map(str::to_owned),
            entity_name: name.map(str::to_owned),
            entity_type: None,
        }
    }

    fn image(id: Option<&str>, src: Option<&str>) -> ImageAttrs {
        ImageAttrs {
            id: id.map(str::to_owned),
            src: src.map(str::to_owned),
        }
    }

    #[test]
    fn task_names_match_python_entry_points() {
        assert_eq!(
            RECENT_VISITED_TASK_NAME,
            "pi_dash.bgtasks.recent_visited_task.recent_visited_task"
        );
        assert_eq!(
            PAGE_TRANSACTION_TASK_NAME,
            "pi_dash.bgtasks.page_transaction_task.page_transaction"
        );
        assert!(is_visit_page_task(RECENT_VISITED_TASK_NAME));
        assert!(is_visit_page_task(PAGE_TRANSACTION_TASK_NAME));
        assert!(!is_visit_page_task(
            "pi_dash.bgtasks.logger_task.process_logs"
        ));
        assert!(!is_visit_page_task(""));
    }

    #[test]
    fn wire_messages_carry_kwargs_like_delay_call_sites() {
        let message = recent_visited_task_message("page", Some("pid"), "uid", None, "ws");
        assert_eq!(message.task, RECENT_VISITED_TASK_NAME);
        assert!(message.args.is_empty());
        assert_eq!(message.kwargs["entity_name"], json!("page"));
        assert_eq!(message.kwargs["slug"], json!("ws"));

        let message = page_transaction_message(Some("<p>n</p>"), None, "pid");
        assert_eq!(message.task, PAGE_TRANSACTION_TASK_NAME);
        assert_eq!(message.kwargs["old_description_html"], Value::Null);
    }

    #[test]
    fn bind_accepts_positional_and_keyword_forms() {
        // Contract-suite form: positional args.
        let bound = bind_recent_visited(&json!(["issue", "eid", "uid", "pid", "ws"]), &json!({}))
            .expect("positional binds");
        assert_eq!(bound.entity_name, "issue");
        assert_eq!(bound.slug, "ws");

        // Django call-site form: kwargs.
        let bound = bind_recent_visited(
            &json!([]),
            &json!({"slug": "ws", "entity_name": "page", "entity_identifier": "pid",
                    "user_id": "uid", "project_id": "pid"}),
        )
        .expect("kwargs bind");
        assert_eq!(bound.entity_name, "page");

        // Null entity_identifier / project_id stay None (IS NULL lookups).
        let bound = bind_recent_visited(
            &json!([]),
            &json!({"slug": "ws", "entity_name": "page", "entity_identifier": null,
                    "user_id": "uid", "project_id": null}),
        )
        .expect("nulls bind");
        assert!(bound.entity_identifier.is_none());
        assert!(bound.project_id.is_none());

        assert!(bind_recent_visited(&json!(["only"]), &json!({})).is_err());
        assert!(bind_recent_visited(&json!([]), &json!({"slug": "ws"})).is_err());

        let bound = bind_page_transaction(&json!(["<p>new</p>", "<p>old</p>", "pid"]), &json!({}))
            .expect("positional binds");
        assert_eq!(bound.page_id, "pid");
        let bound = bind_page_transaction(
            &json!([]),
            &json!({"new_description_html": "<p>n</p>",
                    "old_description_html": null, "page_id": "pid"}),
        )
        .expect("kwargs bind");
        assert!(bound.old_description_html.is_none());
        assert!(bind_page_transaction(&json!(["a"]), &json!({})).is_err());
    }

    #[test]
    fn eviction_gate_ports_the_double_equals_quirk() {
        // FX-VISIT-01: gate is `count == 20` exactly; 21+ rows (reachable
        // via races or the == gap) never evict again.
        assert!(!needs_eviction(0));
        assert!(!needs_eviction(19));
        assert!(needs_eviction(20));
        assert!(!needs_eviction(21));
        assert!(!needs_eviction(100));
    }

    #[test]
    fn plan_inserts_new_ids_and_deletes_removed_ids() {
        let plan = plan_page_logs(
            &[mention(Some("m1"), Some("iid-1"), Some("ISSUE"))],
            &[image(Some("i9"), Some("https://cdn/x.png"))],
            &[],
            &[],
            true,
        );
        assert_eq!(plan.inserts.len(), 2);
        assert_eq!(plan.inserts[0].transaction, "m1");
        assert_eq!(plan.inserts[0].entity_name.as_deref(), Some("ISSUE"));
        assert_eq!(plan.inserts[0].entity_type, None);
        assert_eq!(plan.inserts[0].entity_identifier.as_deref(), Some("iid-1"));
        assert_eq!(plan.inserts[1].transaction, "i9");
        assert_eq!(plan.inserts[1].entity_name.as_deref(), Some("image"));
        assert!(plan.deleted_transaction_ids.is_empty());
    }

    #[test]
    fn plan_backfill_skip_rule_matches_fixture() {
        // FX-PAGE-01 QUIRK-2: id in old_ids skipped ONLY when logs exist.
        let new = vec![mention(Some("m1"), Some("iid-1"), Some("ISSUE"))];
        let old = vec![mention(Some("m1"), Some("iid-1"), Some("ISSUE"))];
        let with_logs = plan_page_logs(&new, &[], &old, &[], true);
        assert!(with_logs.inserts.is_empty());
        assert!(with_logs.deleted_transaction_ids.is_empty());

        let first_run = plan_page_logs(&new, &[], &old, &[], false);
        assert_eq!(first_run.inserts.len(), 1);
        assert_eq!(first_run.inserts[0].transaction, "m1");
    }

    #[test]
    fn plan_skips_idless_mentions_and_unions_deletes() {
        let plan = plan_page_logs(
            &[mention(None, Some("iid-9"), Some("ISSUE"))],
            &[],
            &[
                mention(Some("gone"), Some("iid-2"), Some("ISSUE")),
                mention(None, Some("iid-9"), Some("ISSUE")),
            ],
            &[image(Some("old-img"), Some("https://cdn/old.png"))],
            true,
        );
        // Id-less mentions never produce rows (no PageLog row can exist
        // without a transaction id) and never count as tracked ids.
        assert!(plan.inserts.is_empty());
        // One global delete set across components.
        assert_eq!(plan.deleted_transaction_ids, vec!["gone", "old-img"]);
    }

    #[test]
    fn registry_routes_registered_tasks_locally() {
        let mut registry = Registry::new();
        for name in [RECENT_VISITED_TASK_NAME, PAGE_TRANSACTION_TASK_NAME] {
            let handler: Handler = Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            registry.register(name, handler);
        }
        for name in [RECENT_VISITED_TASK_NAME, PAGE_TRANSACTION_TASK_NAME] {
            assert_eq!(
                crate::worker::route_for(&registry, name),
                crate::worker::Route::Local
            );
        }
        assert_eq!(
            crate::worker::route_for(&registry, "pi_dash.bgtasks.logger_task.process_logs"),
            crate::worker::Route::PythonOwned
        );
    }
}
