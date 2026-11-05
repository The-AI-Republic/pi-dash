//! `domain::comments`: the `IssueComment.save()` port (`db/models/issue.py:598-647`)
//! over the SeaORM transaction, with the linked `Description` row and the
//! activity fan-out folded into the commit chain.
//!
//! The save has three statements, in Django order: the comment `INSERT`
//! (stripped recompute runs in the
//! [`before_save`](pidash_db::entity::comment_hooks) hook), the
//! `Description.objects.create` (`issue.py:623-627`), and the
//! `save(update_fields=["description_id"])` link (`issue.py:628`) — a
//! single-column `UPDATE` that leaves `updated_at`/`updated_by` alone.
//! Updates run the full-column save plus the changed-subset description
//! sync (`issue.py:630-643`); soft deletes run the full-save audit stamp
//! plus the sweep task. Callers supply explicit stamps and pre-built
//! activity kwargs: Django passes different kwargs per view (design D6),
//! and the one-now versus two-now clock shapes stay with the drivers so
//! this migration changes no bytes.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, InsertStatement};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait,
    QueryFilter, QueryTrait, UpdateMany,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::entity::descriptions;
use pidash_db::entity::issue_comments;
use pidash_db::ops::repair::NewDescription;
use pidash_db::orm_tx::{into_sqlx, TxConn};

use super::activity::{TaskEnqueueHook, ISSUE_ACTIVITY_TASK, MODEL_ACTIVITY_TASK};
use super::ctx::WriteCtx;
use super::error::DomainError;
use super::issues::{enqueue_hook, ChainOutcome, ChainStop};
use super::txn::WriteTxn;

/// The `deleted` sweep (`db/mixins.py:77`): full Celery name.
/// Canonical for the services crate (the `jobs` twin stays for jobs'
/// direct use — services cannot depend on jobs).
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// A comment `issue_activity` enqueue with driver-built payloads.
/// `actor` is `None` only on the v1 create path whose `created_by`
/// override is null — Django's `str(None)` renders `"None"`
/// (`api/views/issue.py:1944`). `require_origin` keeps the app
/// stop-on-`None` (`comment.py` always passes an origin, so the stop
/// only fires when resolution failed); v1/space omit the key.
#[derive(Debug, Clone)]
pub struct CommentActivitySpec {
    pub event: String,
    pub requested: Value,
    pub actor: Option<Uuid>,
    pub issue: Uuid,
    pub project: Uuid,
    pub current: Value,
    pub notification: Option<bool>,
    pub origin: Option<String>,
    pub require_origin: bool,
}

/// A comment `model_activity` enqueue (`model_name` is always
/// `"issue_comment"`); the v1 delete and every space path pass `None`.
#[derive(Debug, Clone)]
pub struct CommentModelSpec {
    pub model: Uuid,
    pub requested: Value,
    pub current: Value,
    pub actor: Uuid,
    pub slug: String,
    pub origin: Option<String>,
}

fn comment_activity_kwargs(spec: &CommentActivitySpec) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("type".to_owned(), Value::String(spec.event.clone()));
    kwargs.insert("requested_data".to_owned(), spec.requested.clone());
    let actor = spec
        .actor
        .map_or_else(|| "None".to_owned(), |id| id.to_string());
    kwargs.insert("actor_id".to_owned(), Value::String(actor));
    kwargs.insert("issue_id".to_owned(), Value::String(spec.issue.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(spec.project.to_string()),
    );
    kwargs.insert("current_instance".to_owned(), spec.current.clone());
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(serde_json::Number::from(Utc::now().timestamp())),
    );
    if let Some(notification) = spec.notification {
        kwargs.insert("notification".to_owned(), Value::Bool(notification));
    }
    if let Some(origin) = &spec.origin {
        kwargs.insert("origin".to_owned(), Value::String(origin.clone()));
    }
    kwargs
}

/// Shared [`crate::v1_projects::tasks::model_activity_kwargs`] kernel
/// with `model_name="issue_comment"` (PIDASHCONV-1623): the same seven
/// keys in the same order. `current_instance` is `Null` or a string at
/// every producer, so `.as_str()` maps it exactly.
fn comment_model_kwargs(spec: &CommentModelSpec, origin: &str) -> Map<String, Value> {
    crate::v1_projects::tasks::model_activity_kwargs(
        "issue_comment",
        &spec.model.to_string(),
        spec.requested.clone(),
        spec.current.as_str(),
        &spec.actor.to_string(),
        &spec.slug,
        Some(origin),
    )
}

fn sweep_kwargs() -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    kwargs
}

/// One post-commit effect, in handler order. The chain runs as a single
/// queued step: the origin stop records a [`ChainStop`] and skips every
/// later leg; every other leg is infallible (the hook never fails).
enum ChainEffect {
    CommentActivity(CommentActivitySpec),
    CommentModel(CommentModelSpec),
    SweepComment { id: Uuid },
}

async fn run_chain(
    enqueue: &Arc<dyn TaskEnqueueHook>,
    chain: Vec<ChainEffect>,
    outcome: &ChainOutcome,
) {
    for effect in chain {
        match effect {
            ChainEffect::CommentActivity(spec) => {
                if spec.require_origin && spec.origin.is_none() {
                    outcome.record(ChainStop::Origin);
                    return;
                }
                let kwargs = comment_activity_kwargs(&spec);
                enqueue
                    .enqueue_task(ISSUE_ACTIVITY_TASK, vec![], kwargs)
                    .await;
            }
            ChainEffect::CommentModel(spec) => {
                let Some(origin) = spec.origin.clone() else {
                    continue;
                };
                let kwargs = comment_model_kwargs(&spec, &origin);
                enqueue
                    .enqueue_task(MODEL_ACTIVITY_TASK, vec![], kwargs)
                    .await;
            }
            ChainEffect::SweepComment { id } => {
                enqueue
                    .enqueue_task(
                        SOFT_DELETE_TASK,
                        vec![
                            Value::String("db".to_owned()),
                            Value::String("issuecomment".to_owned()),
                            Value::String(id.to_string()),
                        ],
                        sweep_kwargs(),
                    )
                    .await;
            }
        }
    }
}

/// The `IssueCommentSerializer.save` input: every column the views pass,
/// with the driver's own stamps. `created_by`/`updated_by` come from the
/// [`WriteCtx`] actor (`BaseModel.save`, `base.py:23-42`): the actor on
/// create, `None` for `updated_by` — the in-memory `updated_by` gotcha
/// (set on the instance, never written) is reproduced on the returned
/// model so renders match.
pub struct CreateComment {
    pub id: Uuid,
    pub description_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The description row's own stamps: Django's `Description.save()`
    /// reads the clock twice more, so writers that port that shape
    /// (v1, lifecycle) pass fresh values while single-`now()` writers
    /// repeat the comment stamps.
    pub desc_created_at: DateTime<Utc>,
    pub desc_updated_at: DateTime<Utc>,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub html: String,
    pub json: Value,
    pub access: String,
    pub speaker_type: String,
    pub speaker_label: String,
    pub speaker_agent_run_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub parent_id: Option<Uuid>,
    pub attachments: Vec<String>,
    pub labels: Vec<String>,
    pub edited_at: Option<DateTime<Utc>>,
    pub activity: Option<CommentActivitySpec>,
    pub model_activity: Option<CommentModelSpec>,
}

/// The created row: `description_id` linked, `updated_by_id` set
/// in-memory only (the `BaseModel.save` gotcha — the row keeps `NULL`).
pub struct CommentOutcome {
    pub comment: issue_comments::Model,
    pub chain: ChainOutcome,
}

/// `serializer.save(...)` on create: the comment `INSERT`, the
/// `Description.objects.create`, the `description_id` link `UPDATE`,
/// then the `comment.activity.created` leg and the optional model leg —
/// in handler order.
pub async fn create(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    input: CreateComment,
) -> Result<CommentOutcome, DomainError> {
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let db = txn.connection();
    let model = issue_comments::ActiveModel {
        id: ActiveValue::Set(input.id),
        created_at: ActiveValue::Set(input.created_at.into()),
        updated_at: ActiveValue::Set(input.updated_at.into()),
        created_by_id: ActiveValue::Set(Some(actor)),
        updated_by_id: ActiveValue::Set(None),
        deleted_at: ActiveValue::Set(None),
        workspace_id: ActiveValue::Set(input.workspace_id),
        project_id: ActiveValue::Set(input.project_id),
        issue_id: ActiveValue::Set(input.issue_id),
        actor_id: ActiveValue::Set(input.actor_id),
        comment_html: ActiveValue::Set(input.html.clone()),
        // Recomputed by `before_save`; the initial value never survives.
        comment_stripped: ActiveValue::Set(String::new()),
        comment_json: ActiveValue::Set(input.json.clone()),
        description_id: ActiveValue::Set(None),
        attachments: ActiveValue::Set(input.attachments.clone()),
        labels: ActiveValue::Set(input.labels.clone()),
        access: ActiveValue::Set(input.access.clone()),
        external_source: ActiveValue::Set(input.external_source.clone()),
        external_id: ActiveValue::Set(input.external_id.clone()),
        speaker_type: ActiveValue::Set(input.speaker_type.clone()),
        speaker_label: ActiveValue::Set(input.speaker_label.clone()),
        speaker_agent_run_id: ActiveValue::Set(input.speaker_agent_run_id),
        edited_at: ActiveValue::Set(input.edited_at.map(|t| t.into())),
        parent_id: ActiveValue::Set(input.parent_id),
    }
    .insert(&db)
    .await
    .map_err(DomainError::Db)?;
    // `Description.objects.create(workspace, project, created_by,
    // updated_by, description_stripped, description_json,
    // description_html)` (`issue.py:620-627`): the audit pair is the
    // comment's own (`created_by` actor, `updated_by` `None`); the
    // stripped value the hook recomputes (Django's `strip_tags`,
    // `None` when the HTML is empty).
    let _description = descriptions::ActiveModel {
        id: ActiveValue::Set(input.description_id),
        created_at: ActiveValue::Set(input.desc_created_at.into()),
        updated_at: ActiveValue::Set(input.desc_updated_at.into()),
        created_by_id: ActiveValue::Set(Some(actor)),
        updated_by_id: ActiveValue::Set(None),
        deleted_at: ActiveValue::Set(None),
        workspace_id: ActiveValue::Set(input.workspace_id),
        project_id: ActiveValue::Set(Some(input.project_id)),
        description_json: ActiveValue::Set(input.json.clone()),
        description_html: ActiveValue::Set(input.html.clone()),
        description_binary: ActiveValue::Set(None),
        // Recomputed by `before_save`; the initial value never survives.
        description_stripped: ActiveValue::Set(None),
    }
    .insert(&db)
    .await
    .map_err(DomainError::Db)?;
    // `save(update_fields=["description_id"])` (`issue.py:628`): the
    // single-column link — `updated_at`/`updated_by` stay as inserted.
    issue_comments::ActiveModel {
        id: ActiveValue::Unchanged(input.id),
        description_id: ActiveValue::Set(Some(input.description_id)),
        ..Default::default()
    }
    .update(&db)
    .await
    .map_err(DomainError::Db)?;
    let mut chain = Vec::new();
    if let Some(spec) = input.activity {
        chain.push(ChainEffect::CommentActivity(spec));
    }
    if let Some(spec) = input.model_activity {
        chain.push(ChainEffect::CommentModel(spec));
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    txn.on_commit(move || async move {
        run_chain(&enqueue, chain, &step).await;
    });
    let mut comment = model;
    comment.description_id = Some(input.description_id);
    // The `BaseModel.save` gotcha: `updated_by` is assigned on the
    // instance (`base.py:23-42`) but `update_fields` never writes it —
    // the row keeps `NULL` while renders see the actor.
    comment.updated_by_id = Some(actor);
    Ok(CommentOutcome {
        comment,
        chain: outcome,
    })
}

/// The `IssueCommentSerializer.save` input on update: the stored row plus
/// the driver's resolved new values (`None` = key absent, stored value
/// kept; `Some(None)` = explicit null, written). `description_id` accepts
/// a re-link (the app PATCH `description` key); a resolved `None` mints a
/// fresh description, exactly like a missing link (`issue.py:614-628`).
pub struct UpdateComment {
    pub stored: issue_comments::Model,
    pub updated_at: DateTime<Utc>,
    pub html: Option<String>,
    pub json: Option<Value>,
    pub access: Option<String>,
    pub actor_id: Option<Option<Uuid>>,
    pub external_id: Option<Option<String>>,
    pub external_source: Option<Option<String>>,
    pub edited_at: Option<Option<DateTime<Utc>>>,
    pub description_id: Option<Option<Uuid>>,
    pub parent_id: Option<Option<Uuid>>,
    pub activity: Option<CommentActivitySpec>,
    pub model_activity: Option<CommentModelSpec>,
}

/// `serializer.save(...)` on update: the full-column save (`updated_by`
/// stamped — an update, not a create), the changed-subset description
/// sync or mint, then the `comment.activity.updated` leg and the
/// optional model leg.
pub async fn update(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    input: UpdateComment,
) -> Result<CommentOutcome, DomainError> {
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let db = txn.connection();
    let stored = input.stored;
    let html = input.html.unwrap_or_else(|| stored.comment_html.clone());
    let json = input.json.unwrap_or_else(|| stored.comment_json.clone());
    let access = input.access.unwrap_or_else(|| stored.access.clone());
    let actor_id = input.actor_id.unwrap_or(stored.actor_id);
    let external_id = input.external_id.unwrap_or(stored.external_id.clone());
    let external_source = input
        .external_source
        .unwrap_or(stored.external_source.clone());
    let edited_at: Option<DateTime<Utc>> = input
        .edited_at
        .unwrap_or_else(|| stored.edited_at.map(|t| t.with_timezone(&Utc)));
    let description_id = input.description_id.unwrap_or(stored.description_id);
    let parent_id = input.parent_id.unwrap_or(stored.parent_id);
    let model = issue_comments::ActiveModel {
        id: ActiveValue::Unchanged(stored.id),
        updated_at: ActiveValue::Set(input.updated_at.into()),
        updated_by_id: ActiveValue::Set(Some(actor)),
        comment_html: ActiveValue::Set(html.clone()),
        // Recomputed by `before_save`.
        comment_stripped: ActiveValue::Set(String::new()),
        comment_json: ActiveValue::Set(json.clone()),
        access: ActiveValue::Set(access.clone()),
        actor_id: ActiveValue::Set(actor_id),
        external_id: ActiveValue::Set(external_id.clone()),
        external_source: ActiveValue::Set(external_source.clone()),
        edited_at: ActiveValue::Set(edited_at.map(|t| t.into())),
        description_id: ActiveValue::Set(description_id),
        parent_id: ActiveValue::Set(parent_id),
        ..Default::default()
    }
    .update(&db)
    .await
    .map_err(DomainError::Db)?;
    // `if creating or not self.description_id: <mint> else: <sync the
    // changed subset>` (`issue.py:614-643`). The subset compares the new
    // triple against the STORED comment row (the change tracker's loaded
    // values), and syncs into whichever description the resolved link
    // names — including a freshly re-linked one.
    let html_changed = html != stored.comment_html;
    let json_changed = json != stored.comment_json;
    let stripped_now = model.comment_stripped.clone();
    let stripped_changed = stripped_now != stored.comment_stripped;
    let link = description_id;
    if html_changed || json_changed || stripped_changed || link.is_none() {
        match link {
            None => {
                let description_id = Uuid::new_v4();
                descriptions::ActiveModel {
                    id: ActiveValue::Set(description_id),
                    created_at: ActiveValue::Set(input.updated_at.into()),
                    updated_at: ActiveValue::Set(input.updated_at.into()),
                    created_by_id: ActiveValue::Set(model.created_by_id),
                    updated_by_id: ActiveValue::Set(Some(actor)),
                    deleted_at: ActiveValue::Set(None),
                    workspace_id: ActiveValue::Set(stored.workspace_id),
                    project_id: ActiveValue::Set(Some(stored.project_id)),
                    description_json: ActiveValue::Set(json.clone()),
                    description_html: ActiveValue::Set(html.clone()),
                    description_binary: ActiveValue::Set(None),
                    description_stripped: ActiveValue::Set(None),
                }
                .insert(&db)
                .await
                .map_err(DomainError::Db)?;
                issue_comments::ActiveModel {
                    id: ActiveValue::Unchanged(stored.id),
                    description_id: ActiveValue::Set(Some(description_id)),
                    ..Default::default()
                }
                .update(&db)
                .await
                .map_err(DomainError::Db)?;
            }
            Some(id) => {
                // `Description.objects.filter(pk).update(...)`
                // (`issue.py:640-642`): bare `update_many` statements, not an entity
                // `.update()` — that would fire
                // `descriptions::before_save` and recompute
                // `description_stripped` (Django flavor), where Django
                // stores the comment's own ML-flavored value verbatim.
                // Only the changed subset (`issue.py:630-639`) — an
                // unchanged field keeps the description row's own value,
                // even when the link just moved to another row.
                if html_changed {
                    description_column_update(
                        id,
                        descriptions::Column::DescriptionHtml,
                        html.clone(),
                    )
                    .exec(&db)
                    .await
                    .map_err(DomainError::Db)?;
                }
                if stripped_changed {
                    description_column_update(
                        id,
                        descriptions::Column::DescriptionStripped,
                        stripped_now.clone(),
                    )
                    .exec(&db)
                    .await
                    .map_err(DomainError::Db)?;
                }
                if json_changed {
                    description_column_update(
                        id,
                        descriptions::Column::DescriptionJson,
                        json.clone(),
                    )
                    .exec(&db)
                    .await
                    .map_err(DomainError::Db)?;
                }
                description_touch_update(id, Some(actor), input.updated_at)
                    .exec(&db)
                    .await
                    .map_err(DomainError::Db)?;
            }
        }
    }
    let mut chain = Vec::new();
    if let Some(spec) = input.activity {
        chain.push(ChainEffect::CommentActivity(spec));
    }
    if let Some(spec) = input.model_activity {
        chain.push(ChainEffect::CommentModel(spec));
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    txn.on_commit(move || async move {
        run_chain(&enqueue, chain, &step).await;
    });
    Ok(CommentOutcome {
        comment: model,
        chain: outcome,
    })
}

/// The `.delete()` input: the row id, the driver's stamp, and the
/// pre-built `comment.activity.deleted` leg. No model leg: no Django
/// comment destroy enqueues one.
pub struct SoftDeleteComment {
    pub id: Uuid,
    pub deleted_at: DateTime<Utc>,
    pub activity: Option<CommentActivitySpec>,
}

/// `.delete()` (`mixins.py:70-78`): the full-save audit stamp
/// (`deleted_at`, `updated_at`, `updated_by` — the description sync
/// finds no tracked changes and writes nothing), the sweep task, and
/// the `comment.activity.deleted` leg.
pub async fn soft_delete(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    input: SoftDeleteComment,
) -> Result<ChainOutcome, DomainError> {
    let enqueue = enqueue_hook(txn);
    let actor = ctx.actor.user_id;
    let db = txn.connection();
    issue_comments::ActiveModel {
        id: ActiveValue::Unchanged(input.id),
        deleted_at: ActiveValue::Set(Some(input.deleted_at.into())),
        updated_at: ActiveValue::Set(input.deleted_at.into()),
        updated_by_id: ActiveValue::Set(Some(actor)),
        ..Default::default()
    }
    .update(&db)
    .await
    .map_err(DomainError::Db)?;
    let mut chain = Vec::new();
    chain.push(ChainEffect::SweepComment { id: input.id });
    if let Some(spec) = input.activity {
        chain.push(ChainEffect::CommentActivity(spec));
    }
    let outcome = ChainOutcome::new();
    let step = outcome.clone();
    txn.on_commit(move || async move {
        run_chain(&enqueue, chain, &step).await;
    });
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// Executor twins: the same save() for callers that must stay inside their
// own `sqlx` transaction (the `insert_mirror_issue` precedent). The
// stripped values come from the same pure functions the hooks use, so
// both paths write identical bytes.
// ---------------------------------------------------------------------------

/// The full comment row for [`create_comment_rows`]: explicit audit
/// (syncs pass the actor twice — the collapsed restamp, `git_sync_task.py:170-173` —
/// everyone else the actor plus `None`) and explicit stamps.
pub struct CommentRows<'a> {
    pub id: Uuid,
    pub description_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub desc_created_at: DateTime<Utc>,
    pub desc_updated_at: DateTime<Utc>,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    /// Client-set soft-delete stamp on create: the app create binds
    /// validated `deleted_at` input (every other writer binds NULL).
    pub deleted_at: Option<DateTime<Utc>>,
    pub html: &'a str,
    pub json: &'a Value,
    /// Caller-computed stripped values: writers disagree on the empty
    /// edge (lifecycle binds `""`, v1/app/space bind NULL for the
    /// description row), so the twin binds verbatim instead of
    /// recomputing. Comment-shape writers pass
    /// `comment_stripped_for(html)`; description-shape writers pass
    /// `description_stripped_for(Some(html)).as_deref()`.
    pub stripped: &'a str,
    pub desc_stripped: Option<&'a str>,
    pub access: &'a str,
    pub speaker_type: &'a str,
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<Uuid>,
    pub external_id: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub parent_id: Option<Uuid>,
    pub attachments: &'a [String],
    pub labels: &'a [String],
    pub edited_at: Option<DateTime<Utc>>,
}

/// Run one bare-column `UPDATE` (no hooks) for the `sqlx::Error` twins.
async fn exec_update<E: EntityTrait>(
    db: &impl ConnectionTrait,
    update: UpdateMany<E>,
) -> Result<u64, sqlx::Error> {
    update
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// The full bare comment row (`Entity::insert`, so the `before_save`
/// hook does NOT run: the caller's stripped value lands verbatim).
/// `description_id`/`deleted_at` differ per shape: the plain create
/// binds NULL plus the caller's `deleted_at`, the linked sync create
/// binds the description id plus NULL.
fn comment_row(
    v: &CommentRows<'_>,
    description_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
) -> issue_comments::ActiveModel {
    issue_comments::ActiveModel {
        id: ActiveValue::Set(v.id),
        created_at: ActiveValue::Set(v.created_at.into()),
        updated_at: ActiveValue::Set(v.updated_at.into()),
        created_by_id: ActiveValue::Set(v.created_by),
        updated_by_id: ActiveValue::Set(v.updated_by),
        deleted_at: ActiveValue::Set(deleted_at.map(Into::into)),
        workspace_id: ActiveValue::Set(v.workspace_id),
        project_id: ActiveValue::Set(v.project_id),
        issue_id: ActiveValue::Set(v.issue_id),
        actor_id: ActiveValue::Set(v.actor_id),
        comment_html: ActiveValue::Set(v.html.to_owned()),
        comment_stripped: ActiveValue::Set(v.stripped.to_owned()),
        comment_json: ActiveValue::Set(v.json.clone()),
        description_id: ActiveValue::Set(description_id),
        attachments: ActiveValue::Set(v.attachments.to_vec()),
        labels: ActiveValue::Set(v.labels.to_vec()),
        access: ActiveValue::Set(v.access.to_owned()),
        external_source: ActiveValue::Set(v.external_source.map(str::to_owned)),
        external_id: ActiveValue::Set(v.external_id.map(str::to_owned)),
        speaker_type: ActiveValue::Set(v.speaker_type.to_owned()),
        speaker_label: ActiveValue::Set(v.speaker_label.to_owned()),
        speaker_agent_run_id: ActiveValue::Set(v.speaker_agent_run_id),
        edited_at: ActiveValue::Set(v.edited_at.map(Into::into)),
        parent_id: ActiveValue::Set(v.parent_id),
    }
}

/// The comment `INSERT` (`issue.py:606-613`, stripped recomputed):
/// statement 1 of 3. Callers run
/// [`insert_comment_row`]→[`insert_description_row`]→[`link_comment_description`]
/// in Django order on their own transaction.
pub async fn insert_comment_row(
    db: &impl ConnectionTrait,
    v: &CommentRows<'_>,
) -> Result<(), sqlx::Error> {
    issue_comments::Entity::insert(comment_row(v, None, v.deleted_at))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// `Description.objects.create` (`issue.py:620-627`, stripped via
/// `Description.save`, `description.py:23-29`): statement 2 of 3.
pub async fn insert_description_row(
    db: &impl ConnectionTrait,
    v: &CommentRows<'_>,
) -> Result<(), sqlx::Error> {
    let values = DescriptionValues {
        id: v.description_id,
        created_at: v.desc_created_at,
        updated_at: v.desc_updated_at,
        created_by: v.created_by,
        updated_by: v.updated_by,
        workspace_id: v.workspace_id,
        project_id: v.project_id,
        json: v.json,
        html: v.html,
        stripped: v.desc_stripped,
    };
    insert_description_values(db, &values).await
}

/// The description row's ten columns, for writers that mint the side
/// row without a comment insert beside it (the app update-mint path).
/// `stripped` binds verbatim (caller-computed, as on [`CommentRows`]).
pub struct DescriptionValues<'a> {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub json: &'a Value,
    pub html: &'a str,
    pub stripped: Option<&'a str>,
}

/// The bare description row (`Entity::insert`: no `before_save`, the
/// caller's stripped value lands verbatim); `deleted_at` and
/// `description_binary` are NULL.
fn description_row(v: &DescriptionValues<'_>) -> descriptions::ActiveModel {
    descriptions::ActiveModel {
        id: ActiveValue::Set(v.id),
        created_at: ActiveValue::Set(v.created_at.into()),
        updated_at: ActiveValue::Set(v.updated_at.into()),
        created_by_id: ActiveValue::Set(v.created_by),
        updated_by_id: ActiveValue::Set(v.updated_by),
        deleted_at: ActiveValue::Set(None),
        workspace_id: ActiveValue::Set(v.workspace_id),
        project_id: ActiveValue::Set(Some(v.project_id)),
        description_json: ActiveValue::Set(v.json.clone()),
        description_html: ActiveValue::Set(v.html.to_owned()),
        description_binary: ActiveValue::Set(None),
        description_stripped: ActiveValue::Set(v.stripped.map(str::to_owned)),
    }
}

/// See [`DescriptionValues`]: the shared description `INSERT` all
/// three create shapes ride ([`insert_description_row`] delegates).
pub async fn insert_description_values(
    db: &impl ConnectionTrait,
    v: &DescriptionValues<'_>,
) -> Result<(), sqlx::Error> {
    descriptions::Entity::insert(description_row(v))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// `save(update_fields=["description_id"])`: the single-column link,
/// no stamp bump.
fn comment_link_update(
    comment_id: Uuid,
    description_id: Uuid,
) -> UpdateMany<issue_comments::Entity> {
    issue_comments::Entity::update_many()
        .col_expr(
            issue_comments::Column::DescriptionId,
            Expr::value(description_id),
        )
        .filter(issue_comments::Column::Id.eq(comment_id))
}

/// `save(update_fields=["description_id"])` (`issue.py:628`): statement
/// 3 of 3 — the single-column link, no stamp bump.
pub async fn link_comment_description(
    db: &impl ConnectionTrait,
    comment_id: Uuid,
    description_id: Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(db, comment_link_update(comment_id, description_id)).await?;
    Ok(())
}

/// The sync-mirror comment `INSERT` (git/github `_upsert_comment`
/// create arm): unlike [`insert_comment_row`], the description row is
/// written FIRST and its id binds inline, so no link step follows.
/// Audit binds the scan actor twice (restamp); `deleted_at` is NULL.
/// Statement 2 of the sync create pair (after
/// [`insert_description_row`]), taking the shared [`CommentRows`].
pub async fn insert_comment_row_linked(
    db: &impl ConnectionTrait,
    v: &CommentRows<'_>,
) -> Result<(), sqlx::Error> {
    issue_comments::Entity::insert(comment_row(v, Some(v.description_id), None))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The `update_or_create` update branch (`git_sync_task.py:156-170`,
/// `github_sync_task.py:208-219` twin): render triple + scope +
/// `actor_id` restamp + audit restamp. Django's update path is
/// `setattr` over every default + `save(update_fields=...)`, and
/// `actor_id` IS in `defaults` (`git_sync_task.py:165`,
/// `github_sync_task.py:217`) — the restamp matches Django
/// (PIDASHCONV-1581 item 3 was filed on a misread and disproven:
/// no divergence). Both sync writers bind the scan actor to
/// `actor_id`, `created_by_id` and `updated_by_id`. The description
/// subset refresh and its `changed` gate stay driver-side.
pub struct SyncCommentUpdate<'a> {
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub html: &'a str,
    /// Caller-computed `comment_stripped` (bound verbatim).
    pub stripped: &'a str,
    pub json: &'a Value,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    /// The scan actor: restamps `actor_id`, `created_by_id` and
    /// `updated_by_id` (matching Django's defaults restamp).
    pub actor: Uuid,
}

fn sync_comment_update(v: &SyncCommentUpdate<'_>) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    issue_comments::Entity::update_many()
        .col_expr(C::CommentHtml, Expr::value(v.html))
        .col_expr(C::CommentStripped, Expr::value(v.stripped))
        .col_expr(C::CommentJson, Expr::value(v.json.clone()))
        .col_expr(C::WorkspaceId, Expr::value(v.workspace_id))
        .col_expr(C::ProjectId, Expr::value(v.project_id))
        .col_expr(C::ActorId, Expr::value(v.actor))
        .col_expr(C::CreatedById, Expr::value(v.actor))
        .col_expr(C::UpdatedById, Expr::value(v.actor))
        .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
        .filter(C::Id.eq(v.id))
}

/// See [`SyncCommentUpdate`]: the comment `UPDATE` only.
pub async fn update_sync_comment(
    db: &impl ConnectionTrait,
    v: &SyncCommentUpdate<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, sync_comment_update(v)).await?;
    Ok(())
}

/// One changed-subset description column: a queryset `update()`, so no
/// `Description.save()` re-strip.
fn description_column_update(
    description_id: Uuid,
    column: descriptions::Column,
    value: impl Into<sea_orm::Value>,
) -> UpdateMany<descriptions::Entity> {
    descriptions::Entity::update_many()
        .col_expr(column, Expr::value(value))
        .filter(descriptions::Column::Id.eq(description_id))
}

/// The audit pair of the description sync (`updated_by_id`,
/// `updated_at`).
fn description_touch_update(
    description_id: Uuid,
    updated_by: Option<Uuid>,
    updated_at: DateTime<Utc>,
) -> UpdateMany<descriptions::Entity> {
    descriptions::Entity::update_many()
        .col_expr(descriptions::Column::UpdatedById, Expr::value(updated_by))
        .col_expr(descriptions::Column::UpdatedAt, Expr::value(updated_at))
        .filter(descriptions::Column::Id.eq(description_id))
}

/// The changed-subset description sync (`issue.py:630-639`), one
/// single-column `UPDATE` per changed field (statement count is
/// unobservable; rows match Django's one multi-column `UPDATE`). The
/// caller invokes one fn per changed field plus
/// [`touch_synced_description`] for the audit pair.
pub async fn sync_description_html(
    db: &impl ConnectionTrait,
    description_id: Uuid,
    html: &str,
) -> Result<(), sqlx::Error> {
    let update =
        description_column_update(description_id, descriptions::Column::DescriptionHtml, html);
    exec_update(db, update).await?;
    Ok(())
}

/// See [`sync_description_html`].
pub async fn sync_description_stripped(
    db: &impl ConnectionTrait,
    description_id: Uuid,
    stripped: &str,
) -> Result<(), sqlx::Error> {
    let update = description_column_update(
        description_id,
        descriptions::Column::DescriptionStripped,
        stripped,
    );
    exec_update(db, update).await?;
    Ok(())
}

/// See [`sync_description_html`].
pub async fn sync_description_json(
    db: &impl ConnectionTrait,
    description_id: Uuid,
    json: &Value,
) -> Result<(), sqlx::Error> {
    let update = description_column_update(
        description_id,
        descriptions::Column::DescriptionJson,
        json.clone(),
    );
    exec_update(db, update).await?;
    Ok(())
}

/// The audit pair Django's sync `UPDATE` always carries
/// (`updated_by_id`, `updated_at`, `issue.py:640-642`).
/// Request writers pass the actor; the sync writers pass `None`
/// (`BaseModel.save` blanks `updated_by` with no crum user, and the
/// sync reads `self.updated_by_id` after that — `issue.py:644-646`).
pub async fn touch_synced_description(
    db: &impl ConnectionTrait,
    description_id: Uuid,
    updated_by: Option<Uuid>,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        description_touch_update(description_id, updated_by, updated_at),
    )
    .await?;
    Ok(())
}

/// The v1 create override (`api/views/issue.py:1920-1930`):
/// `save(update_fields=["created_at", "created_by"])` — the bare
/// two-column restamp, no `updated_at` bump, no description sync (the
/// tracked triple is unchanged).
pub async fn restamp_comment_created(
    db: &impl ConnectionTrait,
    id: Uuid,
    created_at: DateTime<Utc>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let update = issue_comments::Entity::update_many()
        .col_expr(issue_comments::Column::CreatedAt, Expr::value(created_at))
        .col_expr(issue_comments::Column::CreatedById, Expr::value(created_by))
        .filter(issue_comments::Column::Id.eq(id));
    exec_update(db, update).await?;
    Ok(())
}

/// The MCP `update_comment` row write (`pi_dash_cloud/mcp/tools.py:1168-1209`,
/// partial serializer save of `comment_html` only): stamps `updated_at` /
/// `updated_by_id`, rewrites the render pair, and sets `description_id`
/// (the mint arm links a fresh description; the sync arm re-binds the
/// linked id). No scope / `actor_id` / `created_by_id` restamp and no
/// `comment_json` write — unlike [`update_sync_comment`] (git-sync-shaped)
/// and [`update`] (full-column save).
fn mcp_comment_update(
    id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by: Uuid,
    stripped: &str,
    html: &str,
    description_id: Option<Uuid>,
) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    issue_comments::Entity::update_many()
        .col_expr(C::UpdatedAt, Expr::value(updated_at))
        .col_expr(C::UpdatedById, Expr::value(updated_by))
        .col_expr(C::CommentStripped, Expr::value(stripped))
        .col_expr(C::CommentHtml, Expr::value(html))
        .col_expr(C::DescriptionId, Expr::value(description_id))
        .filter(C::Id.eq(id))
}

/// See [`mcp_comment_update`]: the row `UPDATE` only. The description
/// mint/sync stays driver-side (the overlay already runs it through
/// [`insert_description_values`] / the `sync_description_*` fns).
pub async fn update_comment_row(
    db: &impl ConnectionTrait,
    id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by: Uuid,
    stripped: &str,
    html: &str,
    description_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let update = mcp_comment_update(id, updated_at, updated_by, stripped, html, description_id);
    exec_update(db, update).await?;
    Ok(())
}

/// The app comment PATCH full-row save: every writable column (`save()`
/// writes the instance back with only provided attributes changed — the
/// driver merges first, so this is a plain full-row write). No
/// `deleted_at` guard: the app destroy path re-saves through here. Runs
/// on any connection so the driver keeps the write inside its transaction.
pub struct AppCommentUpdate<'a> {
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub stripped: &'a str,
    pub json: &'a Value,
    pub html: &'a str,
    pub description_id: Option<Uuid>,
    pub attachments: &'a [String],
    pub labels: &'a [String],
    pub actor_id: Option<Uuid>,
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub speaker_type: &'a str,
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub parent_id: Option<Uuid>,
}

fn app_comment_update(v: &AppCommentUpdate<'_>) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    issue_comments::Entity::update_many()
        .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
        .col_expr(C::UpdatedById, Expr::value(v.updated_by))
        .col_expr(C::DeletedAt, Expr::value(v.deleted_at))
        .col_expr(C::CommentStripped, Expr::value(v.stripped))
        .col_expr(C::CommentJson, Expr::value(v.json.clone()))
        .col_expr(C::CommentHtml, Expr::value(v.html))
        .col_expr(C::DescriptionId, Expr::value(v.description_id))
        .col_expr(C::Attachments, Expr::value(v.attachments.to_vec()))
        .col_expr(C::Labels, Expr::value(v.labels.to_vec()))
        .col_expr(C::ActorId, Expr::value(v.actor_id))
        .col_expr(C::Access, Expr::value(v.access))
        .col_expr(
            C::ExternalSource,
            Expr::value(v.external_source.map(str::to_owned)),
        )
        .col_expr(C::ExternalId, Expr::value(v.external_id.map(str::to_owned)))
        .col_expr(C::SpeakerType, Expr::value(v.speaker_type))
        .col_expr(C::SpeakerLabel, Expr::value(v.speaker_label))
        .col_expr(C::SpeakerAgentRunId, Expr::value(v.speaker_agent_run_id))
        .col_expr(C::EditedAt, Expr::value(v.edited_at))
        .col_expr(C::ParentId, Expr::value(v.parent_id))
        .filter(C::Id.eq(v.id))
}

/// See [`AppCommentUpdate`]: the comment `UPDATE` only.
pub async fn save_app_comment_update(
    db: &impl ConnectionTrait,
    v: &AppCommentUpdate<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, app_comment_update(v)).await?;
    Ok(())
}

/// The space comment PATCH full-row save (`IssueComment.save`,
/// `db/models/issue.py:598-647`): the merged row plus the audit touch,
/// guarded by `deleted_at IS NULL`. The stripped recompute
/// (`:606`), the missing-link mint (`:618-621`) and the changed-subset
/// description sync (`:625-641`) stay driver-side, sequenced around it.
pub struct SpaceCommentUpdate<'a> {
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Uuid,
    pub stripped: &'a str,
    pub json: &'a Value,
    pub html: Option<&'a str>,
    pub attachments: &'a [String],
    pub labels: &'a [String],
    pub access: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub speaker_type: Option<&'a str>,
    pub speaker_label: Option<&'a str>,
    pub speaker_agent_run_id: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub parent_id: Option<Uuid>,
    pub description_id: Option<Uuid>,
}

fn space_comment_update(v: &SpaceCommentUpdate<'_>) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    let owned = |value: Option<&str>| value.map(str::to_owned);
    issue_comments::Entity::update_many()
        .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
        .col_expr(C::UpdatedById, Expr::value(v.updated_by))
        .col_expr(C::CommentStripped, Expr::value(v.stripped))
        .col_expr(C::CommentJson, Expr::value(v.json.clone()))
        .col_expr(C::CommentHtml, Expr::value(owned(v.html)))
        .col_expr(C::Attachments, Expr::value(v.attachments.to_vec()))
        .col_expr(C::Labels, Expr::value(v.labels.to_vec()))
        .col_expr(C::Access, Expr::value(owned(v.access)))
        .col_expr(C::ExternalSource, Expr::value(owned(v.external_source)))
        .col_expr(C::ExternalId, Expr::value(owned(v.external_id)))
        .col_expr(C::SpeakerType, Expr::value(owned(v.speaker_type)))
        .col_expr(C::SpeakerLabel, Expr::value(owned(v.speaker_label)))
        .col_expr(C::SpeakerAgentRunId, Expr::value(v.speaker_agent_run_id))
        .col_expr(C::EditedAt, Expr::value(v.edited_at))
        .col_expr(C::ParentId, Expr::value(v.parent_id))
        .col_expr(C::DescriptionId, Expr::value(v.description_id))
        .filter(C::Id.eq(v.id))
        .filter(C::DeletedAt.is_null())
}

/// See [`SpaceCommentUpdate`]: the comment `UPDATE` only.
pub async fn save_space_comment_update(
    db: &impl ConnectionTrait,
    v: &SpaceCommentUpdate<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, space_comment_update(v)).await?;
    Ok(())
}

/// The space missing-link relink (`db/models/issue.py:618-621`): the
/// second save stamps `updated_by`/`updated_at` again (a fresh clock
/// read — the driver's `relinked_at`, not the save stamp).
pub async fn relink_space_comment_description(
    db: &impl ConnectionTrait,
    id: Uuid,
    description_id: Uuid,
    updated_by: Uuid,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    use issue_comments::Column as C;
    let update = issue_comments::Entity::update_many()
        .col_expr(C::DescriptionId, Expr::value(description_id))
        .col_expr(C::UpdatedById, Expr::value(updated_by))
        .col_expr(C::UpdatedAt, Expr::value(updated_at))
        .filter(C::Id.eq(id))
        .filter(C::DeletedAt.is_null());
    exec_update(db, update).await?;
    Ok(())
}

/// The soft-delete audit stamp (`db/mixins.py:72-77`): `deleted_at`,
/// the full-save `updated_at`, and CRUM `updated_by`.
fn comment_soft_delete_update(
    id: Uuid,
    actor: Uuid,
    deleted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    issue_comments::Entity::update_many()
        .col_expr(C::DeletedAt, Expr::value(deleted_at))
        .col_expr(C::UpdatedAt, Expr::value(updated_at))
        .col_expr(C::UpdatedById, Expr::value(actor))
        .filter(C::Id.eq(id))
}

/// `comment.delete()` on space (`db/mixins.py:72-77`): `deleted_at` plus
/// a full `save()` (`updated_at`, CRUM `updated_by`). One clock read,
/// bound twice — the one-now shape stays with the driver, so this takes
/// a single `now`. Guarded by `deleted_at IS NULL`.
pub async fn soft_delete_space_comment(
    db: &impl ConnectionTrait,
    id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let update = comment_soft_delete_update(id, actor, now, now)
        .filter(issue_comments::Column::DeletedAt.is_null());
    exec_update(db, update).await?;
    Ok(())
}

/// The v1 comment PATCH save (`IssueComment.save`,
/// `db/models/issue.py:598-647`): the merged triple plus the audit pair.
/// The stripped value is the `:606` recompute (even when `comment_html`
/// is untouched); the `Description` INSERT-or-sync and the link write
/// stay driver-side inside the same atomic block.
pub struct V1CommentUpdate<'a> {
    pub id: Uuid,
    pub json: &'a Value,
    pub html: &'a str,
    pub stripped: &'a str,
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub labels: &'a [String],
    pub speaker_type: &'a str,
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Uuid,
}

fn v1_comment_update(v: &V1CommentUpdate<'_>) -> UpdateMany<issue_comments::Entity> {
    use issue_comments::Column as C;
    issue_comments::Entity::update_many()
        .col_expr(C::CommentJson, Expr::value(v.json.clone()))
        .col_expr(C::CommentHtml, Expr::value(v.html))
        .col_expr(C::CommentStripped, Expr::value(v.stripped))
        .col_expr(C::Access, Expr::value(v.access))
        .col_expr(
            C::ExternalSource,
            Expr::value(v.external_source.map(str::to_owned)),
        )
        .col_expr(C::ExternalId, Expr::value(v.external_id.map(str::to_owned)))
        .col_expr(C::Labels, Expr::value(v.labels.to_vec()))
        .col_expr(C::SpeakerType, Expr::value(v.speaker_type))
        .col_expr(C::SpeakerLabel, Expr::value(v.speaker_label))
        .col_expr(C::SpeakerAgentRunId, Expr::value(v.speaker_agent_run_id))
        .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
        .col_expr(C::UpdatedById, Expr::value(v.updated_by))
        .filter(C::Id.eq(v.id))
}

/// See [`V1CommentUpdate`]: the comment `UPDATE` only.
pub async fn save_v1_comment_update(
    db: &impl ConnectionTrait,
    v: &V1CommentUpdate<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, v1_comment_update(v)).await?;
    Ok(())
}

/// The v1 `Description` side of `IssueComment.save`
/// (`db/models/issue.py:623-647`): only changed tracked fields sync
/// (`_changes_on_save` compares values), plus `updated_by`/`updated_at`
/// — via queryset `update()`, so no `save()` override re-strips. Each
/// `Some` arm joins the SET list in fixed html/stripped/json order; all
/// `None` is a no-op (the driver guards the call, as Django's
/// `if creating or ...` branch does).
pub struct V1DescriptionSync<'a> {
    pub id: Uuid,
    pub html: Option<&'a str>,
    pub stripped: Option<&'a str>,
    pub json: Option<&'a Value>,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

/// The changed-subset statement for [`sync_v1_description_subset`]:
/// each `Some` arm joins the SET list in fixed html/stripped/json
/// order, then the audit pair. `None` when no arm runs.
fn v1_description_sync_update(
    v: &V1DescriptionSync<'_>,
) -> Option<UpdateMany<descriptions::Entity>> {
    use descriptions::Column as C;
    if v.html.is_none() && v.stripped.is_none() && v.json.is_none() {
        return None;
    }
    let mut update = descriptions::Entity::update_many();
    if let Some(html) = v.html {
        update = update.col_expr(C::DescriptionHtml, Expr::value(html));
    }
    if let Some(stripped) = v.stripped {
        update = update.col_expr(C::DescriptionStripped, Expr::value(stripped));
    }
    if let Some(json) = v.json {
        update = update.col_expr(C::DescriptionJson, Expr::value(json.clone()));
    }
    Some(
        update
            .col_expr(C::UpdatedById, Expr::value(v.updated_by))
            .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
            .filter(C::Id.eq(v.id)),
    )
}

/// See [`V1DescriptionSync`]: the changed-subset description `UPDATE`.
pub async fn sync_v1_description_subset(
    db: &impl ConnectionTrait,
    v: &V1DescriptionSync<'_>,
) -> Result<u64, sqlx::Error> {
    match v1_description_sync_update(v) {
        Some(update) => exec_update(db, update).await,
        None => Ok(0),
    }
}

/// `SoftDeleteModel.delete()` on v1 (`views/issue.py:2103-2121`):
/// `deleted_at` + full `save()` (so `updated_at`/`updated_by` move).
/// Two `now()` calls like `delete()`: `deleted_at` stamps first,
/// `updated_at` re-stamps on the `save()` — the two-now shape stays
/// with the driver, so this takes both stamps.
pub async fn soft_delete_v1_comment(
    db: &impl ConnectionTrait,
    id: Uuid,
    actor: Uuid,
    deleted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        comment_soft_delete_update(id, actor, deleted_at, updated_at),
    )
    .await?;
    Ok(())
}

/// `SoftDeleteModel.delete()` on the app comment destroy
/// (`app/views/issue/comment.py:170-172`, `db/mixins.py:72-77`): one
/// clock read stamps `deleted_at` and the full-save `updated_at`, CRUM
/// stamps `updated_by`. Keyed by pk alone (the scoped `.get()` already
/// found the live row), through SeaORM on any connection.
pub async fn soft_delete_app_comment(
    db: &impl ConnectionTrait,
    id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sea_orm::DbErr> {
    comment_soft_delete_update(id, actor, now, now)
        .exec(db)
        .await?;
    Ok(())
}

/// Multi-row `INSERT INTO descriptions ... RETURNING id` for `bulk_create`
/// (`copy_issue_comment_to_description.py:44`): one statement per batch,
/// zip-aligned `RETURNING id` like Django's. A bare `insert_many` (no
/// `Description.save()` hook: the mapped stripped value lands verbatim).
fn copy_descriptions_insert(descriptions: &[NewDescription]) -> InsertStatement {
    let rows = descriptions.iter().map(|desc| descriptions::ActiveModel {
        id: ActiveValue::Set(desc.id),
        created_at: ActiveValue::Set(desc.created_at.into()),
        updated_at: ActiveValue::Set(desc.updated_at.into()),
        description_json: ActiveValue::Set(desc.description_json.clone()),
        description_html: ActiveValue::Set(desc.description_html.clone()),
        description_stripped: ActiveValue::Set(desc.description_stripped.clone()),
        project_id: ActiveValue::Set(desc.project_id),
        created_by_id: ActiveValue::Set(desc.created_by_id),
        updated_by_id: ActiveValue::Set(desc.updated_by_id),
        workspace_id: ActiveValue::Set(desc.workspace_id),
        ..Default::default()
    });
    let mut stmt = descriptions::Entity::insert_many(rows).into_query();
    stmt.returning_col(descriptions::Column::Id);
    stmt
}

/// One copy batch write (`copy_issue_comment_to_description.py:28-51`):
/// `bulk_create` then the `description_id` backfill (`bulk_update`
/// writes `description_id` only, no `updated_at` bump) in a single
/// transaction. Returns the inserted ids, zip-aligned with
/// `descriptions`.
pub async fn apply_copy_batch(
    pool: &sqlx::PgPool,
    descriptions: &[NewDescription],
    links: &[(Uuid, Uuid)],
) -> Result<Vec<Uuid>, sqlx::Error> {
    if descriptions.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = pool.begin().await?;
    let ids = {
        let db = TxConn::new(&mut tx);
        let inserted = db
            .query_all(DbBackend::Postgres.build(&copy_descriptions_insert(descriptions)))
            .await
            .map_err(into_sqlx)?;
        let mut ids = Vec::with_capacity(inserted.len());
        for row in &inserted {
            ids.push(row.try_get::<Uuid>("", "id").map_err(into_sqlx)?);
        }
        for (comment_id, description_id) in links {
            exec_update(&db, comment_link_update(*comment_id, *description_id)).await?;
        }
        ids
    };
    tx.commit().await?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use chrono::Utc;
    use sea_orm::EntityTrait;

    use super::super::ctx::{WriteActor, WriteCtx};

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn pin_time() -> DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-11-05T10:00:00Z")
            .expect("pin time")
            .with_timezone(&Utc)
    }

    fn render<E: EntityTrait>(update: UpdateMany<E>) -> String {
        update.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn comment_statement_shapes() {
        let json = serde_json::json!({"a": 1});
        let tags = vec!["t".to_owned()];
        let sync = SyncCommentUpdate {
            id: uid(1),
            updated_at: pin_time(),
            html: "<p>h</p>",
            stripped: "h",
            json: &json,
            workspace_id: uid(2),
            project_id: uid(3),
            actor: uid(4),
        };
        let app = AppCommentUpdate {
            id: uid(1),
            updated_at: pin_time(),
            updated_by: Some(uid(4)),
            deleted_at: None,
            stripped: "h",
            json: &json,
            html: "<p>h</p>",
            description_id: Some(uid(5)),
            attachments: &tags,
            labels: &tags,
            actor_id: Some(uid(4)),
            access: "INTERNAL",
            external_source: None,
            external_id: Some("9"),
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            edited_at: None,
            parent_id: None,
        };
        let space = SpaceCommentUpdate {
            id: uid(1),
            updated_at: pin_time(),
            updated_by: uid(4),
            stripped: "h",
            json: &json,
            html: Some("<p>h</p>"),
            attachments: &tags,
            labels: &tags,
            access: None,
            external_source: None,
            external_id: None,
            speaker_type: Some("human"),
            speaker_label: None,
            speaker_agent_run_id: None,
            edited_at: None,
            parent_id: None,
            description_id: None,
        };
        let v1 = V1CommentUpdate {
            id: uid(1),
            json: &json,
            html: "<p>h</p>",
            stripped: "h",
            access: "INTERNAL",
            external_source: None,
            external_id: None,
            labels: &tags,
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            updated_at: pin_time(),
            updated_by: uid(4),
        };
        let all = V1DescriptionSync {
            id: uid(5),
            html: Some("<p>h</p>"),
            stripped: Some("h"),
            json: Some(&json),
            updated_by: uid(4),
            updated_at: pin_time(),
        };
        let only_stripped = V1DescriptionSync {
            html: None,
            json: None,
            ..all
        };
        let copy = NewDescription {
            id: uid(5),
            created_at: pin_time(),
            updated_at: pin_time(),
            description_json: json.clone(),
            description_html: "<p>h</p>".to_owned(),
            description_stripped: Some("h".to_owned()),
            project_id: Some(uid(3)),
            created_by_id: None,
            updated_by_id: None,
            workspace_id: uid(2),
        };
        let got = [
            render(comment_link_update(uid(1), uid(5))),
            render(sync_comment_update(&sync)),
            render(description_column_update(
                uid(5),
                descriptions::Column::DescriptionHtml,
                "<p>h</p>",
            )),
            render(description_touch_update(uid(5), None, pin_time())),
            render(mcp_comment_update(
                uid(1),
                pin_time(),
                uid(4),
                "h",
                "<p>h</p>",
                Some(uid(5)),
            )),
            render(app_comment_update(&app)),
            render(space_comment_update(&space)),
            render(v1_comment_update(&v1)),
            render(comment_soft_delete_update(
                uid(1),
                uid(4),
                pin_time(),
                pin_time(),
            )),
            render(v1_description_sync_update(&all).expect("all arms")),
            render(v1_description_sync_update(&only_stripped).expect("one arm")),
            DbBackend::Postgres
                .build(&copy_descriptions_insert(&[copy.clone(), copy]))
                .to_string(),
        ];
        let want = [
            r#"UPDATE "issue_comments" SET "description_id" = '00000000-0000-0000-0000-000000000005' WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "issue_comments" SET "comment_html" = '<p>h</p>', "comment_stripped" = 'h', "comment_json" = E'{\"a\":1}', "workspace_id" = '00000000-0000-0000-0000-000000000002', "project_id" = '00000000-0000-0000-0000-000000000003', "actor_id" = '00000000-0000-0000-0000-000000000004', "created_by_id" = '00000000-0000-0000-0000-000000000004', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "descriptions" SET "description_html" = '<p>h</p>' WHERE "descriptions"."id" = '00000000-0000-0000-0000-000000000005'"#,
            r#"UPDATE "descriptions" SET "updated_by_id" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "descriptions"."id" = '00000000-0000-0000-0000-000000000005'"#,
            r#"UPDATE "issue_comments" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "comment_stripped" = 'h', "comment_html" = '<p>h</p>', "description_id" = '00000000-0000-0000-0000-000000000005' WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "issue_comments" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "deleted_at" = NULL, "comment_stripped" = 'h', "comment_json" = E'{\"a\":1}', "comment_html" = '<p>h</p>', "description_id" = '00000000-0000-0000-0000-000000000005', "attachments" = ARRAY ['t'], "labels" = ARRAY ['t'], "actor_id" = '00000000-0000-0000-0000-000000000004', "access" = 'INTERNAL', "external_source" = NULL, "external_id" = '9', "speaker_type" = 'human', "speaker_label" = '', "speaker_agent_run_id" = NULL, "edited_at" = NULL, "parent_id" = NULL WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "issue_comments" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "comment_stripped" = 'h', "comment_json" = E'{\"a\":1}', "comment_html" = '<p>h</p>', "attachments" = ARRAY ['t'], "labels" = ARRAY ['t'], "access" = NULL, "external_source" = NULL, "external_id" = NULL, "speaker_type" = 'human', "speaker_label" = NULL, "speaker_agent_run_id" = NULL, "edited_at" = NULL, "parent_id" = NULL, "description_id" = NULL WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001' AND "issue_comments"."deleted_at" IS NULL"#,
            r#"UPDATE "issue_comments" SET "comment_json" = E'{\"a\":1}', "comment_html" = '<p>h</p>', "comment_stripped" = 'h', "access" = 'INTERNAL', "external_source" = NULL, "external_id" = NULL, "labels" = ARRAY ['t'], "speaker_type" = 'human', "speaker_label" = '', "speaker_agent_run_id" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000004' WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "issue_comments" SET "deleted_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000004' WHERE "issue_comments"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "descriptions" SET "description_html" = '<p>h</p>', "description_stripped" = 'h', "description_json" = E'{\"a\":1}', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "descriptions"."id" = '00000000-0000-0000-0000-000000000005'"#,
            r#"UPDATE "descriptions" SET "description_stripped" = 'h', "updated_by_id" = '00000000-0000-0000-0000-000000000004', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "descriptions"."id" = '00000000-0000-0000-0000-000000000005'"#,
            r#"INSERT INTO "descriptions" ("created_at", "updated_at", "id", "description_json", "description_html", "description_stripped", "created_by_id", "project_id", "updated_by_id", "workspace_id") VALUES ('2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000005', E'{\"a\":1}', '<p>h</p>', 'h', NULL, '00000000-0000-0000-0000-000000000003', NULL, '00000000-0000-0000-0000-000000000002'), ('2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000005', E'{\"a\":1}', '<p>h</p>', 'h', NULL, '00000000-0000-0000-0000-000000000003', NULL, '00000000-0000-0000-0000-000000000002') RETURNING "id""#,
        ];
        assert_eq!(got.len(), want.len());
        for (got, want) in got.iter().zip(want) {
            assert_eq!(got, want);
        }
        assert!(v1_description_sync_update(&V1DescriptionSync {
            stripped: None,
            ..only_stripped
        })
        .is_none());
    }
    fn activity_spec() -> CommentActivitySpec {
        CommentActivitySpec {
            event: "comment.activity.created".to_owned(),
            requested: Value::String("req".to_owned()),
            actor: Some(uid(7)),
            issue: uid(8),
            project: uid(9),
            current: Value::Null,
            notification: Some(true),
            origin: Some("https://app.example.test".to_owned()),
            require_origin: true,
        }
    }

    #[test]
    fn comment_activity_kwargs_carry_every_field() {
        let kwargs = comment_activity_kwargs(&activity_spec());
        assert_eq!(
            kwargs["type"],
            Value::String("comment.activity.created".to_owned())
        );
        assert_eq!(kwargs["requested_data"], Value::String("req".to_owned()));
        assert_eq!(kwargs["actor_id"], Value::String(uid(7).to_string()));
        assert_eq!(kwargs["issue_id"], Value::String(uid(8).to_string()));
        assert_eq!(kwargs["project_id"], Value::String(uid(9).to_string()));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["notification"], Value::Bool(true));
        assert_eq!(
            kwargs["origin"],
            Value::String("https://app.example.test".to_owned())
        );
        let epoch = kwargs["epoch"].as_i64().expect("epoch");
        assert!((Utc::now().timestamp() - epoch).abs() < 60);
    }

    #[test]
    fn comment_activity_kwargs_omit_absent_keys() {
        let mut spec = activity_spec();
        spec.notification = None;
        spec.origin = None;
        let kwargs = comment_activity_kwargs(&spec);
        assert!(!kwargs.contains_key("notification"));
        assert!(!kwargs.contains_key("origin"));
    }

    #[test]
    fn comment_activity_actor_none_renders_python_none() {
        // The v1 create override with a null `created_by`: Django's
        // `str(None)` (`api/views/issue.py:1944`).
        let mut spec = activity_spec();
        spec.actor = None;
        let kwargs = comment_activity_kwargs(&spec);
        assert_eq!(kwargs["actor_id"], Value::String("None".to_owned()));
    }

    #[test]
    fn comment_model_kwargs_keep_raw_requested_object() {
        let spec = CommentModelSpec {
            model: uid(1),
            requested: serde_json::json!({"comment_html": "<p>x</p>"}),
            current: Value::Null,
            actor: uid(7),
            slug: "ws".to_owned(),
            origin: Some("https://app.example.test".to_owned()),
        };
        let kwargs = comment_model_kwargs(&spec, "https://app.example.test");
        assert_eq!(
            kwargs["model_name"],
            Value::String("issue_comment".to_owned())
        );
        assert_eq!(kwargs["model_id"], Value::String(uid(1).to_string()));
        assert_eq!(kwargs["requested_data"], spec.requested);
        assert_eq!(kwargs["actor_id"], Value::String(uid(7).to_string()));
        assert_eq!(kwargs["slug"], Value::String("ws".to_owned()));
    }

    type EnqueueCall = (String, Vec<Value>, Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: Map<String, Value>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    #[tokio::test]
    async fn chain_origin_stop_skips_later_legs() {
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let outcome = ChainOutcome::new();
        let hook: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        let mut spec = activity_spec();
        spec.origin = None;
        spec.require_origin = true;
        run_chain(
            &hook,
            vec![
                ChainEffect::CommentActivity(spec),
                ChainEffect::SweepComment { id: uid(1) },
            ],
            &outcome,
        )
        .await;
        assert!(matches!(outcome.stopped(), Some(ChainStop::Origin)));
        assert!(enqueue.calls.lock().expect("calls lock").is_empty());
    }

    #[tokio::test]
    async fn chain_sweep_carries_db_model_pk() {
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let outcome = ChainOutcome::new();
        let hook: Arc<dyn TaskEnqueueHook> = enqueue.clone();
        run_chain(
            &hook,
            vec![ChainEffect::SweepComment { id: uid(1) }],
            &outcome,
        )
        .await;
        assert!(outcome.stopped().is_none());
        let calls = enqueue.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, SOFT_DELETE_TASK);
        assert_eq!(
            calls[0].1,
            vec![
                Value::String("db".to_owned()),
                Value::String("issuecomment".to_owned()),
                Value::String(uid(1).to_string()),
            ]
        );
        assert_eq!(calls[0].2["using"], Value::Null);
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    struct Seed {
        actor: Uuid,
        workspace_id: Uuid,
        project_id: Uuid,
        issue_id: Uuid,
    }

    async fn seed_world(pool: &sqlx::PgPool, tag: &str) -> Seed {
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let state_id = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let username = format!("cm-{tag}-{}", actor.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
             '', false, $2, true, false)",
        )
        .bind(actor)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'cm ws', $2, 'UTC', '#ffffff', \
             $3)",
        )
        .bind(workspace_id)
        .bind(format!("cm-{tag}-{}", workspace_id.simple()))
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed workspace");
        let identifier = format!("C{:08X}", (project_id.as_u128() & 0xffff_ffff) as u32);
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
             $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!("cm-project-{identifier}"))
        .bind(identifier)
        .bind(workspace_id)
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
             ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, 'backlog', true, false)",
        )
        .bind(state_id)
        .bind(format!("cm-backlog-{state_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed state");
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, state_id, \
             name, description_json, description_html, sequence_id, sort_order, priority, \
             complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, now(), now(), $2, \
             $3, $4, 'cm issue', '{}', '<p>cm</p>', 3, 10000.0, 'none', 0, false, '', '')",
        )
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(state_id)
        .execute(pool)
        .await
        .expect("seed issue");
        Seed {
            actor,
            workspace_id,
            project_id,
            issue_id,
        }
    }

    fn write_ctx(actor: Uuid) -> WriteCtx {
        WriteCtx::new(WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        })
    }

    fn create_input(seed: &Seed, html: &str) -> CreateComment {
        CreateComment {
            id: Uuid::new_v4(),
            description_id: Uuid::new_v4(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            desc_created_at: Utc::now(),
            desc_updated_at: Utc::now(),
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            issue_id: seed.issue_id,
            actor_id: Some(seed.actor),
            html: html.to_owned(),
            json: serde_json::json!({}),
            access: "INTERNAL".to_owned(),
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: Vec::new(),
            labels: Vec::new(),
            edited_at: None,
            activity: Some(CommentActivitySpec {
                event: "comment.activity.created".to_owned(),
                requested: Value::String("req".to_owned()),
                actor: Some(seed.actor),
                issue: seed.issue_id,
                project: seed.project_id,
                current: Value::Null,
                notification: Some(true),
                origin: Some("https://app.example.test".to_owned()),
                require_origin: true,
            }),
            model_activity: Some(CommentModelSpec {
                model: Uuid::new_v4(),
                requested: serde_json::json!({"comment_html": html}),
                current: Value::Null,
                actor: seed.actor,
                slug: "cm-ws".to_owned(),
                origin: Some("https://app.example.test".to_owned()),
            }),
        }
    }

    #[tokio::test]
    async fn live_create_links_description_and_drains_legs() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "create").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        // Entities pass through the description stripped verbatim (Django
        // `strip_tags`) while the comment stripped decodes them (ML).
        let html = "<p>R&amp;D</p>";
        let input = create_input(&seed, html);
        let (id, description_id) = (input.id, input.description_id);
        let outcome = super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        assert!(outcome.chain.stopped().is_none());
        assert_eq!(outcome.comment.id, id);
        assert_eq!(outcome.comment.description_id, Some(description_id));
        // In-memory `updated_by` gotcha: the render sees the actor.
        assert_eq!(outcome.comment.updated_by_id, Some(seed.actor));
        let row: (String, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT comment_stripped, created_by_id, updated_by_id \
             FROM issue_comments WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(row.0, "R&D");
        assert_eq!(row.1, Some(seed.actor));
        assert_eq!(row.2, None);
        let desc: (String, Option<String>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT description_html, description_stripped, created_by_id, updated_by_id \
             FROM descriptions WHERE id = $1",
        )
        .bind(description_id)
        .fetch_one(&pool)
        .await
        .expect("read description");
        assert_eq!(desc.0, html);
        assert_eq!(desc.1.as_deref(), Some("R&amp;D"));
        assert_eq!(desc.2, Some(seed.actor));
        assert_eq!(desc.3, None);
        let calls = enqueue.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, ISSUE_ACTIVITY_TASK);
        assert_eq!(calls[1].0, MODEL_ACTIVITY_TASK);
        assert_eq!(calls[0].2["notification"], Value::Bool(true));
    }

    #[tokio::test]
    async fn live_create_empty_html_nulls_description_stripped() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "empty").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let mut input = create_input(&seed, "");
        input.activity = None;
        input.model_activity = None;
        let (id, description_id) = (input.id, input.description_id);
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        let stripped: (String,) =
            sqlx::query_as("SELECT comment_stripped FROM issue_comments WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read comment");
        assert_eq!(stripped.0, "");
        let desc: (Option<String>,) =
            sqlx::query_as("SELECT description_stripped FROM descriptions WHERE id = $1")
                .bind(description_id)
                .fetch_one(&pool)
                .await
                .expect("read description");
        assert_eq!(desc.0, None);
    }

    #[tokio::test]
    async fn live_update_syncs_changed_subset_only() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "subset").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        // Entity-bearing HTML: the comment hook decodes (`R&D`) while the
        // description hook keeps entities verbatim (`R&amp;D`) — the sync
        // below must store the comment's ML value verbatim (Django's
        // `QuerySet.update` skips `Description.save()`), not recompute it.
        let input = create_input(&seed, "<p>R&amp;D v1</p>");
        let (id, description_id) = (input.id, input.description_id);
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        // Independently diverge the description json: the subset sync
        // must preserve it when only the HTML changes.
        sqlx::query("UPDATE descriptions SET description_json = $2 WHERE id = $1")
            .bind(description_id)
            .bind(serde_json::json!({"foreign": true}))
            .execute(&pool)
            .await
            .expect("diverge description");
        let stored = issue_comments::Entity::find_by_id(id)
            .one(&db)
            .await
            .expect("load stored")
            .expect("stored row");
        let outcome = super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                update(
                    &ctx,
                    txn,
                    UpdateComment {
                        stored,
                        updated_at: Utc::now(),
                        html: Some("<p>R&amp;D v2</p>".to_owned()),
                        json: None,
                        access: None,
                        actor_id: None,
                        external_id: None,
                        external_source: None,
                        edited_at: None,
                        description_id: None,
                        parent_id: None,
                        activity: None,
                        model_activity: None,
                    },
                )
                .await
            })
        })
        .await
        .expect("domain::run update");
        assert_eq!(outcome.comment.comment_html, "<p>R&amp;D v2</p>");
        assert_eq!(outcome.comment.comment_stripped, "R&D v2");
        let desc: (String, Option<String>, Value) = sqlx::query_as(
            "SELECT description_html, description_stripped, description_json \
             FROM descriptions WHERE id = $1",
        )
        .bind(description_id)
        .fetch_one(&pool)
        .await
        .expect("read description");
        assert_eq!(desc.0, "<p>R&amp;D v2</p>");
        // The comment's ML value verbatim — a `before_save` recompute
        // would store the Django flavor (`R&amp;D v2`) instead.
        assert_eq!(desc.1.as_deref(), Some("R&D v2"));
        assert_eq!(desc.2, serde_json::json!({"foreign": true}));
    }

    #[tokio::test]
    async fn live_update_untouched_writes_no_description() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "touch").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let input = create_input(&seed, "<p>same</p>");
        let (id, description_id) = (input.id, input.description_id);
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        let before: (chrono::DateTime<Utc>,) =
            sqlx::query_as("SELECT updated_at FROM descriptions WHERE id = $1")
                .bind(description_id)
                .fetch_one(&pool)
                .await
                .expect("read description");
        let stored = issue_comments::Entity::find_by_id(id)
            .one(&db)
            .await
            .expect("load stored")
            .expect("stored row");
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                update(
                    &ctx,
                    txn,
                    UpdateComment {
                        stored,
                        updated_at: Utc::now(),
                        html: None,
                        json: None,
                        access: Some("EXTERNAL".to_owned()),
                        actor_id: None,
                        external_id: None,
                        external_source: None,
                        edited_at: None,
                        description_id: None,
                        parent_id: None,
                        activity: None,
                        model_activity: None,
                    },
                )
                .await
            })
        })
        .await
        .expect("domain::run update");
        let after: (chrono::DateTime<Utc>,) =
            sqlx::query_as("SELECT updated_at FROM descriptions WHERE id = $1")
                .bind(description_id)
                .fetch_one(&pool)
                .await
                .expect("read description");
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn live_soft_delete_stamps_and_sweeps() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "del").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let mut input = create_input(&seed, "<p>bye</p>");
        input.activity = None;
        input.model_activity = None;
        let (id, description_id) = (input.id, input.description_id);
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        let chain = super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                soft_delete(
                    &ctx,
                    txn,
                    SoftDeleteComment {
                        id,
                        deleted_at: Utc::now(),
                        activity: Some(CommentActivitySpec {
                            event: "comment.activity.deleted".to_owned(),
                            requested: serde_json::json!({"comment_id": id.to_string()}),
                            actor: Some(seed.actor),
                            issue: seed.issue_id,
                            project: seed.project_id,
                            current: Value::String("before".to_owned()),
                            notification: None,
                            origin: None,
                            require_origin: false,
                        }),
                    },
                )
                .await
            })
        })
        .await
        .expect("domain::run soft_delete");
        assert!(chain.stopped().is_none());
        let row: (Option<chrono::DateTime<Utc>>,) =
            sqlx::query_as("SELECT deleted_at FROM issue_comments WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read comment");
        assert!(row.0.is_some());
        // The description sync finds no tracked changes: untouched.
        let desc: (Value,) =
            sqlx::query_as("SELECT description_json FROM descriptions WHERE id = $1")
                .bind(description_id)
                .fetch_one(&pool)
                .await
                .expect("read description");
        assert_eq!(desc.0, serde_json::json!({}));
        let calls = enqueue.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, SOFT_DELETE_TASK);
        assert_eq!(calls[1].0, ISSUE_ACTIVITY_TASK);
        assert!(!calls[1].2.contains_key("notification"));
    }

    #[tokio::test]
    async fn live_executor_twins_match_writetxn_rows() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "twin").await;
        let now = Utc::now();
        let desc_now = Utc::now();
        let json = serde_json::json!({});
        let attachments: Vec<String> = Vec::new();
        let labels: Vec<String> = Vec::new();
        let values = CommentRows {
            id: Uuid::new_v4(),
            description_id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            desc_created_at: desc_now,
            desc_updated_at: desc_now,
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            issue_id: seed.issue_id,
            actor_id: Some(seed.actor),
            created_by: Some(seed.actor),
            updated_by: None,
            deleted_at: None,
            html: "<p>R&amp;D</p>",
            json: &json,
            stripped: "R&D",
            desc_stripped: Some("R&amp;D"),
            access: "INTERNAL",
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: &attachments,
            labels: &labels,
            edited_at: None,
        };
        let mut tx = pool.begin().await.expect("begin");
        insert_comment_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("insert");
        insert_description_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("desc");
        link_comment_description(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            values.id,
            values.description_id,
        )
        .await
        .expect("link");
        tx.commit().await.expect("commit");
        let row: (String, Option<Uuid>) = sqlx::query_as(
            "SELECT comment_stripped, updated_by_id FROM issue_comments WHERE id = $1",
        )
        .bind(values.id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(row.0, "R&D");
        assert_eq!(row.1, None);
        let desc: (Option<String>,) =
            sqlx::query_as("SELECT description_stripped FROM descriptions WHERE id = $1")
                .bind(values.description_id)
                .fetch_one(&pool)
                .await
                .expect("read description");
        assert_eq!(desc.0.as_deref(), Some("R&amp;D"));
    }

    #[tokio::test]
    async fn live_restamp_updates_created_pair_only() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "restamp").await;
        let db = pidash_db::sea_orm_connection(&pool);
        let ctx = write_ctx(seed.actor);
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let mut input = create_input(&seed, "<p>r</p>");
        input.activity = None;
        input.model_activity = None;
        let id = input.id;
        super::super::run(&ctx, &db, |txn| {
            let enqueue: Arc<dyn TaskEnqueueHook> = enqueue.clone();
            let ctx = ctx.clone();
            Box::pin(async move {
                txn.hooks_mut().register(enqueue);
                create(&ctx, txn, input).await
            })
        })
        .await
        .expect("domain::run create");
        let before: (chrono::DateTime<Utc>,) =
            sqlx::query_as("SELECT updated_at FROM issue_comments WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read comment");
        let stamp = chrono::DateTime::parse_from_rfc3339("2020-01-02T03:04:05Z")
            .expect("stamp")
            .with_timezone(&Utc);
        restamp_comment_created(&pidash_db::pool::sea_orm_connection(&pool), id, stamp, None)
            .await
            .expect("restamp");
        let after: (chrono::DateTime<Utc>, Option<Uuid>, chrono::DateTime<Utc>) = sqlx::query_as(
            "SELECT created_at, created_by_id, updated_at FROM issue_comments WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(after.0, stamp);
        assert_eq!(after.1, None);
        assert_eq!(after.2, before.0);
    }

    #[tokio::test]
    async fn live_update_comment_row_writes_mcp_shape_only() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "mcpupd").await;
        // A distinct updater: the row must stamp `updated_by_id` with
        // this user while `created_by_id` keeps the seed actor (no
        // restamp — unlike the sync shape).
        let updater = seed_world(&pool, "mcpupdater").await;
        let json = serde_json::json!({"seed": true});
        let attachments: Vec<String> = Vec::new();
        let labels: Vec<String> = Vec::new();
        let created = chrono::DateTime::parse_from_rfc3339("2020-01-02T03:04:05Z")
            .expect("created")
            .with_timezone(&Utc);
        let values = CommentRows {
            id: Uuid::new_v4(),
            description_id: Uuid::new_v4(),
            created_at: created,
            updated_at: created,
            desc_created_at: created,
            desc_updated_at: created,
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            issue_id: seed.issue_id,
            actor_id: Some(seed.actor),
            created_by: Some(seed.actor),
            updated_by: None,
            deleted_at: None,
            html: "<p>v1</p>",
            json: &json,
            stripped: "v1",
            desc_stripped: Some("v1"),
            access: "INTERNAL",
            speaker_type: "agent",
            speaker_label: "MCP",
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: &attachments,
            labels: &labels,
            edited_at: None,
        };
        let mut tx = pool.begin().await.expect("begin");
        insert_comment_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("insert");
        insert_description_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("desc");
        link_comment_description(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            values.id,
            values.description_id,
        )
        .await
        .expect("link");
        tx.commit().await.expect("commit");
        // The overlay's mint arm runs before this UPDATE (fresh
        // description, updater as `created_by`, `updated_by` NULL);
        // the UPDATE links it.
        let stamp = chrono::DateTime::parse_from_rfc3339("2021-06-07T08:09:10Z")
            .expect("stamp")
            .with_timezone(&Utc);
        let minted = Uuid::new_v4();
        let minted_values = DescriptionValues {
            id: minted,
            created_at: stamp,
            updated_at: stamp,
            created_by: Some(updater.actor),
            updated_by: None,
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            json: &json,
            html: "<p>v2</p>",
            stripped: Some("v2"),
        };
        insert_description_values(&pidash_db::pool::sea_orm_connection(&pool), &minted_values)
            .await
            .expect("mint");
        update_comment_row(
            &pidash_db::pool::sea_orm_connection(&pool),
            values.id,
            stamp,
            updater.actor,
            "v2",
            "<p>v2</p>",
            Some(minted),
        )
        .await
        .expect("update");
        // Tuple alias keeps the wide `query_as` shape under the
        // `type_complexity` lint (repo pattern: `prompting` `IssueCols`).
        type McpRow = (
            chrono::DateTime<Utc>,
            Option<Uuid>,
            String,
            String,
            Option<Uuid>,
            Uuid,
            Uuid,
            Option<Uuid>,
            Option<Uuid>,
            Value,
            chrono::DateTime<Utc>,
        );
        let row: McpRow = sqlx::query_as(
            "SELECT updated_at, updated_by_id, comment_stripped, comment_html, description_id, \
             workspace_id, project_id, actor_id, created_by_id, comment_json, created_at \
             FROM issue_comments WHERE id = $1",
        )
        .bind(values.id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(row.0, stamp);
        assert_eq!(row.1, Some(updater.actor));
        assert_eq!(row.2, "v2");
        assert_eq!(row.3, "<p>v2</p>");
        assert_eq!(row.4, Some(minted));
        assert_eq!(row.5, seed.workspace_id);
        assert_eq!(row.6, seed.project_id);
        assert_eq!(row.7, Some(seed.actor));
        assert_eq!(row.8, Some(seed.actor));
        assert_eq!(row.9, serde_json::json!({"seed": true}));
        assert_eq!(row.10, created);
    }

    #[tokio::test]
    async fn live_sync_pair_restamps_actor_and_subset_triple() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let seed = seed_world(&pool, "sync").await;
        let now = Utc::now();
        let desc_now = Utc::now();
        let json = serde_json::json!({});
        let attachments: Vec<String> = Vec::new();
        let labels: Vec<String> = Vec::new();
        let other_actor = Uuid::new_v4();
        let other_name = format!("cm-sync-{}", other_actor.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
             '', false, $2, true, false)",
        )
        .bind(other_actor)
        .bind(other_name.clone())
        .bind(format!("{other_name}@example.test"))
        .bind("t".repeat(64))
        .execute(&pool)
        .await
        .expect("seed other user");
        // Sync create pair, desc first with the id bound inline.
        let values = CommentRows {
            id: Uuid::new_v4(),
            description_id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            desc_created_at: desc_now,
            desc_updated_at: desc_now,
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            issue_id: seed.issue_id,
            actor_id: Some(other_actor),
            created_by: Some(other_actor),
            updated_by: Some(other_actor),
            deleted_at: None,
            html: "<p>v1</p>",
            json: &json,
            stripped: "v1",
            desc_stripped: Some("v1"),
            access: "INTERNAL",
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            external_id: Some("42"),
            external_source: Some("GitHub"),
            parent_id: None,
            attachments: &attachments,
            labels: &labels,
            edited_at: None,
        };
        let mut tx = pool.begin().await.expect("begin");
        insert_description_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("desc");
        insert_comment_row_linked(&pidash_db::orm_tx::TxConn::new(&mut tx), &values)
            .await
            .expect("insert");
        tx.commit().await.expect("commit");
        let linked: (Option<Uuid>,) =
            sqlx::query_as("SELECT description_id FROM issue_comments WHERE id = $1")
                .bind(values.id)
                .fetch_one(&pool)
                .await
                .expect("read link");
        assert_eq!(linked.0, Some(values.description_id));
        // Independently diverge the description json: the subset
        // refresh must preserve it when only html/stripped change
        // (PIDASHCONV-1581 item 2).
        sqlx::query("UPDATE descriptions SET description_json = $2 WHERE id = $1")
            .bind(values.description_id)
            .bind(serde_json::json!({"foreign": true}))
            .execute(&pool)
            .await
            .expect("diverge description");
        // Re-sync: the scan actor restamps actor_id + both audit
        // columns (matching Django's defaults restamp — 1581 item 3
        // was disproven); the description refresh syncs only the
        // changed subset with updated_by NULL (the worker-crum read).
        let new_json = serde_json::json!({});
        let update = SyncCommentUpdate {
            id: values.id,
            updated_at: Utc::now(),
            html: "<p>v2</p>",
            stripped: "v2",
            json: &new_json,
            workspace_id: seed.workspace_id,
            project_id: seed.project_id,
            actor: seed.actor,
        };
        let mut tx = pool.begin().await.expect("begin");
        update_sync_comment(&pidash_db::orm_tx::TxConn::new(&mut tx), &update)
            .await
            .expect("update");
        let synced_at = Utc::now();
        sync_description_html(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            values.description_id,
            "<p>v2</p>",
        )
        .await
        .expect("sync html");
        sync_description_stripped(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            values.description_id,
            "v2",
        )
        .await
        .expect("sync stripped");
        touch_synced_description(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            values.description_id,
            None,
            synced_at,
        )
        .await
        .expect("touch");
        tx.commit().await.expect("commit");
        let row: (Option<Uuid>, Option<Uuid>, Option<Uuid>, String) = sqlx::query_as(
            "SELECT actor_id, created_by_id, updated_by_id, comment_stripped \
             FROM issue_comments WHERE id = $1",
        )
        .bind(values.id)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(row.0, Some(seed.actor));
        assert_eq!(row.1, Some(seed.actor));
        assert_eq!(row.2, Some(seed.actor));
        assert_eq!(row.3, "v2");
        let desc: (String, Option<String>, Value, Option<Uuid>) = sqlx::query_as(
            "SELECT description_html, description_stripped, description_json, updated_by_id \
             FROM descriptions WHERE id = $1",
        )
        .bind(values.description_id)
        .fetch_one(&pool)
        .await
        .expect("read description");
        assert_eq!(desc.0, "<p>v2</p>");
        assert_eq!(desc.1.as_deref(), Some("v2"));
        assert_eq!(desc.2, serde_json::json!({"foreign": true}));
        assert_eq!(desc.3, None);
    }

    /// The temp-table half: session-local copies of `issue_comments` and
    /// `descriptions` (no constraints) on a one-connection pool, so the
    /// view-specific twins and the repair copy batch run with no seeded
    /// parents. Each write is checked by reading the row back.
    #[tokio::test]
    async fn live_twins_on_temp_tables() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        for table in ["issue_comments", "descriptions"] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let db = pidash_db::sea_orm_connection(&pool);
        let json = serde_json::json!({"k": 1});
        let tags = vec!["a".to_owned(), "b".to_owned()];
        let created = pin_time();
        let rows = CommentRows {
            id: uid(1),
            description_id: uid(2),
            created_at: created,
            updated_at: created,
            desc_created_at: created,
            desc_updated_at: created,
            workspace_id: uid(3),
            project_id: uid(4),
            issue_id: uid(5),
            actor_id: Some(uid(6)),
            created_by: Some(uid(6)),
            updated_by: None,
            deleted_at: None,
            html: "<p>v1</p>",
            json: &json,
            stripped: "v1",
            desc_stripped: None,
            access: "INTERNAL",
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: &tags,
            labels: &tags,
            edited_at: None,
        };
        insert_comment_row(&db, &rows)
            .await
            .expect("insert comment");
        insert_description_row(&db, &rows)
            .await
            .expect("insert desc");
        link_comment_description(&db, rows.id, rows.description_id)
            .await
            .expect("link");
        let comment = |id: Uuid| {
            let db = &db;
            async move {
                issue_comments::Entity::find_by_id(id)
                    .one(db)
                    .await
                    .expect("read comment")
                    .expect("comment row")
            }
        };
        let description = |id: Uuid| {
            let db = &db;
            async move {
                descriptions::Entity::find_by_id(id)
                    .one(db)
                    .await
                    .expect("read description")
                    .expect("description row")
            }
        };
        let row = comment(rows.id).await;
        assert_eq!(row.description_id, Some(uid(2)));
        assert_eq!(row.comment_stripped, "v1");
        assert_eq!(row.labels, tags);
        assert_eq!(row.deleted_at, None);
        assert_eq!(description(uid(2)).await.description_stripped, None);

        // App full-row save: every writable column lands.
        let t1 = created + chrono::Duration::seconds(1);
        save_app_comment_update(
            &db,
            &AppCommentUpdate {
                id: rows.id,
                updated_at: t1,
                updated_by: Some(uid(7)),
                deleted_at: None,
                stripped: "v2",
                json: &serde_json::json!({"k": 2}),
                html: "<p>v2</p>",
                description_id: Some(uid(2)),
                attachments: &[],
                labels: &tags[..1],
                actor_id: None,
                access: "EXTERNAL",
                external_source: Some("GitHub"),
                external_id: Some("42"),
                speaker_type: "agent",
                speaker_label: "Bot",
                speaker_agent_run_id: Some(uid(8)),
                edited_at: Some(t1),
                parent_id: Some(uid(9)),
            },
        )
        .await
        .expect("app save");
        let row = comment(rows.id).await;
        assert_eq!(row.comment_html, "<p>v2</p>");
        assert_eq!(row.comment_json, serde_json::json!({"k": 2}));
        assert_eq!(row.updated_by_id, Some(uid(7)));
        assert_eq!(row.attachments, Vec::<String>::new());
        assert_eq!(row.labels, vec!["a".to_owned()]);
        assert_eq!(row.actor_id, None);
        assert_eq!(row.access, "EXTERNAL");
        assert_eq!(row.external_source.as_deref(), Some("GitHub"));
        assert_eq!(row.speaker_agent_run_id, Some(uid(8)));
        assert_eq!(row.edited_at, Some(t1.into()));
        assert_eq!(row.parent_id, Some(uid(9)));
        assert_eq!(row.created_by_id, Some(uid(6)));

        // v1 save + changed-subset sync (stripped only) + JSON sync.
        let t2 = t1 + chrono::Duration::seconds(1);
        save_v1_comment_update(
            &db,
            &V1CommentUpdate {
                id: rows.id,
                json: &json,
                html: "<p>v3</p>",
                stripped: "v3",
                access: "INTERNAL",
                external_source: None,
                external_id: None,
                labels: &tags,
                speaker_type: "human",
                speaker_label: "",
                speaker_agent_run_id: None,
                updated_at: t2,
                updated_by: uid(6),
            },
        )
        .await
        .expect("v1 save");
        let row = comment(rows.id).await;
        assert_eq!(row.comment_html, "<p>v3</p>");
        assert_eq!(row.external_source, None);
        assert_eq!(row.speaker_agent_run_id, None);
        assert_eq!(row.parent_id, Some(uid(9)), "v1 leaves it");
        let synced = sync_v1_description_subset(
            &db,
            &V1DescriptionSync {
                id: uid(2),
                html: None,
                stripped: Some("v3"),
                json: None,
                updated_by: uid(6),
                updated_at: t2,
            },
        )
        .await
        .expect("v1 subset");
        assert_eq!(synced, 1);
        let none = sync_v1_description_subset(
            &db,
            &V1DescriptionSync {
                id: uid(2),
                html: None,
                stripped: None,
                json: None,
                updated_by: uid(6),
                updated_at: t2,
            },
        )
        .await
        .expect("v1 empty subset");
        assert_eq!(none, 0);
        sync_description_json(&db, uid(2), &serde_json::json!({"k": 3}))
            .await
            .expect("json sync");
        let desc = description(uid(2)).await;
        assert_eq!(desc.description_stripped.as_deref(), Some("v3"));
        assert_eq!(desc.description_html, "<p>v1</p>", "html untouched");
        assert_eq!(desc.description_json, serde_json::json!({"k": 3}));
        assert_eq!(desc.updated_by_id, Some(uid(6)));
        assert_eq!(desc.updated_at, t2.fixed_offset());

        // Space save (deleted_at guard), relink, soft delete; then the
        // guard makes a second space save a no-op.
        let t3 = t2 + chrono::Duration::seconds(1);
        let space = SpaceCommentUpdate {
            id: rows.id,
            updated_at: t3,
            updated_by: uid(7),
            stripped: "v4",
            json: &json,
            html: Some("<p>v4</p>"),
            attachments: &tags,
            labels: &tags,
            access: Some("INTERNAL"),
            external_source: None,
            external_id: None,
            speaker_type: Some("human"),
            speaker_label: Some(""),
            speaker_agent_run_id: None,
            edited_at: None,
            parent_id: None,
            description_id: Some(uid(2)),
        };
        save_space_comment_update(&db, &space)
            .await
            .expect("space save");
        assert_eq!(comment(rows.id).await.comment_html, "<p>v4</p>");
        relink_space_comment_description(&db, rows.id, uid(10), uid(6), t3)
            .await
            .expect("relink");
        let row = comment(rows.id).await;
        assert_eq!(row.description_id, Some(uid(10)));
        assert_eq!(row.updated_by_id, Some(uid(6)));
        let t4 = t3 + chrono::Duration::seconds(1);
        soft_delete_space_comment(&db, rows.id, uid(7), t4)
            .await
            .expect("space delete");
        let row = comment(rows.id).await;
        assert_eq!(row.deleted_at, Some(t4.into()));
        assert_eq!(row.updated_at, t4.fixed_offset());
        save_space_comment_update(
            &db,
            &SpaceCommentUpdate {
                html: Some("<p>late</p>"),
                ..space
            },
        )
        .await
        .expect("guarded space save");
        assert_eq!(comment(rows.id).await.comment_html, "<p>v4</p>");

        // v1 two-now soft delete re-stamps the deleted row by pk.
        let t5 = t4 + chrono::Duration::seconds(1);
        let t6 = t5 + chrono::Duration::seconds(1);
        soft_delete_v1_comment(&db, rows.id, uid(6), t5, t6)
            .await
            .expect("v1 delete");
        let row = comment(rows.id).await;
        assert_eq!(row.deleted_at, Some(t5.into()));
        assert_eq!(row.updated_at, t6.fixed_offset());
        assert_eq!(row.updated_by_id, Some(uid(6)));

        // The repair copy batch: multi-row insert RETURNING ids in input
        // order, then the bare description_id links.
        let second = CommentRows {
            id: uid(11),
            ..rows
        };
        insert_comment_row(&db, &second)
            .await
            .expect("second comment");
        let new_desc = |id: u128, project: Option<Uuid>| NewDescription {
            id: uid(id),
            created_at: created,
            updated_at: created,
            description_json: serde_json::json!({}),
            description_html: format!("<p>{id}</p>"),
            description_stripped: Some(id.to_string()),
            project_id: project,
            created_by_id: None,
            updated_by_id: None,
            workspace_id: uid(3),
        };
        let batch = [new_desc(21, Some(uid(4))), new_desc(20, None)];
        let ids = apply_copy_batch(&pool, &batch, &[(uid(1), uid(21)), (uid(11), uid(20))])
            .await
            .expect("copy batch");
        assert_eq!(ids, vec![uid(21), uid(20)]);
        assert_eq!(comment(uid(1)).await.description_id, Some(uid(21)));
        let linked = comment(uid(11)).await;
        assert_eq!(linked.description_id, Some(uid(20)));
        assert_eq!(
            linked.updated_at,
            created.fixed_offset(),
            "link bumps nothing"
        );
        let copied = description(uid(20)).await;
        assert_eq!(copied.project_id, None);
        assert_eq!(copied.description_stripped.as_deref(), Some("20"));
        assert!(apply_copy_batch(&pool, &[], &[])
            .await
            .expect("empty batch")
            .is_empty());
        for table in ["issue_comments", "descriptions"] {
            sqlx::query(&format!("DROP TABLE pg_temp.{table}"))
                .execute(&pool)
                .await
                .expect("drop temp table");
        }
    }
}
