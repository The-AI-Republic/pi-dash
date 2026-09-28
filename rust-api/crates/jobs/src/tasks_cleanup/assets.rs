//! File-asset background tasks: Celery-wire handlers (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/file_asset_task.py`,
//! `apps/api/pi_dash/bgtasks/storage_metadata_task.py` and
//! `apps/api/pi_dash/bgtasks/copy_s3_object.py`. The pure domain logic
//! (entity map, tag walk, key building, SQL text) lives in
//! [`pidash_services::tasks_cleanup::assets`]; this module owns the task
//! names, argument binding, the store traits and [`register_assets`].
//!
//! Celery names, exactly as beat and `.delay()` call them:
//!
//! * `pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset`
//! * `pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata`
//! * `pi_dash.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets`
//!
//! # Verdict mapping (translate, don't redesign)
//!
//! Python swallows most failures (`except ...: log_exception; return`),
//! so those paths ack. Only the sweep has no `try`: its errors escape,
//! which parks the row without retry (Celery records a failure; the bare
//! `@shared_task` carries no autoretry). Handler `Err` would *retry*, so
//! Python-raise paths return `Ok(Verdict::Fail)`, never `Err`.
//!
//! # Sinks, never live buckets
//!
//! S3 goes through [`ObjectStore`] and the live-document conversion
//! through [`LiveConvert`]. [`UnavailableObjectStore`] degrades exactly
//! like boto with dead credentials (log + `None`, ignored copies) and
//! [`NoopLiveConvert`] degrades like an unset `LIVE_URL` (the duplicate
//! is still created; the binary regenerates on first open). Tests inject
//! fakes that capture every side effect.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_services::tasks_cleanup::assets as logic;
use pidash_services::tasks_cleanup::assets::{AssetIdPair, CopyEntity, HeadMeta};

use crate::celery::CeleryTaskMessage;
use crate::worker::{Handler, Registry, Verdict};

/// `pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset`
/// (beat `check-every-day-to-delete-file-asset`, `celery.py:50-53`).
pub const TASK_DELETE_UNUPLOADED: &str =
    "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset";

/// `pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata`.
pub const TASK_GET_METADATA: &str =
    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// `pi_dash.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets`.
pub const TASK_COPY_S3_OBJECTS: &str =
    "pi_dash.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets";

/// The tag the copy task walks (`copy_s3_object.py:137,140`).
pub const DESCRIPTION_TAG: &str = "image-component";

/// Bind one task argument: the kwarg wins when the key is present (even
/// when null, mirroring Python binding), otherwise the positional.
/// Call sites use kwargs for the copy task (`app/views/page/base.py:620`)
/// and either form for the metadata task.
fn task_arg<'v>(args: &'v Value, kwargs: &'v Value, key: &str, pos: usize) -> Option<&'v Value> {
    if let Value::Object(fields) = kwargs {
        if let Some(value) = fields.get(key) {
            return Some(value);
        }
    }
    args.get(pos)
}

fn as_text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// A pages/issues row for the copy task.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRow {
    pub id: Uuid,
    /// Nullable FK: `None` fails the task (`entity.workspace.id` raises
    /// `AttributeError` in Python).
    pub workspace_id: Option<Uuid>,
    pub description_html: String,
}

/// A source asset row for the copy scope.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginalAsset {
    pub id: Uuid,
    pub asset_key: String,
    pub attributes: Option<Value>,
    pub size: f64,
    pub entity_type: Option<String>,
    pub storage_metadata: Option<Value>,
}

/// A duplicate row to insert. `entity_*` FKs start empty;
/// [`NewAsset::with_entity_field`] applies the mapped one.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAsset {
    pub id: Uuid,
    pub created_by_id: Uuid,
    pub attributes: Value,
    pub asset_key: String,
    pub workspace_id: Uuid,
    pub user_id: Option<Uuid>,
    pub draft_issue_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    pub comment_id: Option<Uuid>,
    pub page_id: Option<Uuid>,
    pub entity_type: Option<String>,
    pub size: f64,
    pub storage_metadata: Option<Value>,
}

impl NewAsset {
    /// Port of `**get_entity_id_field(entity_type, entity_identifier)`
    /// (`copy_s3_object.py:107`): set the mapped FK column, if any.
    /// Every mapped column exists on `file_assets`, so the spread never
    /// fails — including `user_id` (the `user` FK).
    pub fn with_entity_field(mut self, entity_type: Option<&str>, entity_id: Uuid) -> Self {
        match entity_type.and_then(logic::entity_id_field) {
            Some("workspace_id") => self.workspace_id = entity_id,
            Some("project_id") => self.project_id = Some(entity_id),
            Some("user_id") => self.user_id = Some(entity_id),
            Some("issue_id") => self.issue_id = Some(entity_id),
            Some("page_id") => self.page_id = Some(entity_id),
            Some("comment_id") => self.comment_id = Some(entity_id),
            Some("draft_issue_id") => self.draft_issue_id = Some(entity_id),
            Some(_) | None => {}
        }
        self
    }
}

/// A converted document from the live server.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedDoc {
    /// `None` (or a missing key) stores `{}` — the `or {}` fallback.
    pub description_json: Option<Value>,
    /// `None` leaves the binary column untouched.
    pub description_binary: Option<Vec<u8>>,
}

/// What the copy task did. `Skipped` is the `[]` path (log + ack);
/// `Completed` is the bare-`return` path (ack).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    Completed,
    Skipped,
}

/// The database surface the three tasks need, mirroring the Django calls.
/// Errors are plain text; the run functions decide ack vs park.
pub trait AssetStore: Send + Sync {
    /// The sweep `UPDATE` (`file_asset_task.py:21-26`); returns rows stamped.
    fn sweep_unuploaded(
        &self,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<u64, String>> + Send;
    /// `FileAsset.objects.get(pk)` key column, live rows only.
    fn asset_key(
        &self,
        asset_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send;
    /// `save(update_fields=["storage_metadata"])` (+ the `auto_now` bump).
    fn save_storage_metadata(
        &self,
        asset_id: Uuid,
        metadata: Option<Value>,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    /// `Page/Issue.objects.get(id)`, live rows only.
    fn entity(
        &self,
        entity: CopyEntity,
        id: Uuid,
    ) -> impl std::future::Future<Output = Result<Option<EntityRow>, String>> + Send;
    /// The copy scope (`copy_s3_object.py:90`).
    fn original_assets(
        &self,
        workspace_id: Uuid,
        project_id: Option<Uuid>,
        ids: Vec<Uuid>,
    ) -> impl std::future::Future<Output = Result<Vec<OriginalAsset>, String>> + Send;
    /// `FileAsset.objects.create(...)`; returns the row id.
    fn insert_duplicate(
        &self,
        asset: NewAsset,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<Uuid, String>> + Send;
    /// `filter(pk__in=...).update(is_uploaded=True)`.
    fn mark_uploaded(
        &self,
        ids: Vec<Uuid>,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    /// `entity.save()` after the description rewrite.
    fn save_description_html(
        &self,
        entity: CopyEntity,
        id: Uuid,
        html: &str,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    /// The second save when the live service answered.
    fn save_description_docs(
        &self,
        entity: CopyEntity,
        id: Uuid,
        description_json: Value,
        description_binary: Option<Vec<u8>>,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// Object storage. Failures degrade inside the impl (log + `None` /
/// ignore), exactly like `S3Storage` catching `ClientError` — they never
/// fail the task.
pub trait ObjectStore: Send + Sync {
    /// `get_object_metadata`: `None` means unavailable (stored as `NULL`).
    fn head_object(&self, key: &str) -> impl std::future::Future<Output = Option<HeadMeta>> + Send;
    /// `copy_object`: failures are logged and ignored (the row still counts).
    fn copy_object(
        &self,
        src_key: &str,
        dst_key: &str,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// The live-document conversion (`utils/live_document.py`). `Ok(None)` is
/// the `LiveConversionError` path → `{}` → no second save. `Err` is an
/// unexpected transport failure → the task's `[]` path.
pub trait LiveConvert: Send + Sync {
    fn convert(
        &self,
        html: &str,
        variant: &str,
    ) -> impl std::future::Future<Output = Result<Option<ConvertedDoc>, String>> + Send;
}

/// S3 unavailable: every head answers `None`, every copy is logged and
/// dropped — the same degradation as boto with dead credentials.
pub struct UnavailableObjectStore;

impl ObjectStore for UnavailableObjectStore {
    async fn head_object(&self, key: &str) -> Option<HeadMeta> {
        tracing::warn!(object = key, "s3 unavailable: head_object degraded to None");
        None
    }

    async fn copy_object(&self, src_key: &str, dst_key: &str) {
        tracing::warn!(src = src_key, dst = dst_key, "s3 unavailable: copy dropped");
    }
}

/// Live server unavailable: conversion answers `None`, so the duplicate
/// is still created and the binary regenerates on first open — the
/// documented `sync_with_external_service` fallback.
pub struct NoopLiveConvert;

impl LiveConvert for NoopLiveConvert {
    async fn convert(&self, _html: &str, _variant: &str) -> Result<Option<ConvertedDoc>, String> {
        Ok(None)
    }
}

/// Run the sweep: stamp `deleted_at` on rows older than `days`.
/// Errors escape (no `try` in Python) → the caller parks.
pub async fn run_sweep<S: AssetStore>(
    store: &S,
    days: i64,
    now: DateTime<Utc>,
) -> Result<u64, String> {
    let cutoff = now - chrono::Duration::days(days);
    store.sweep_unuploaded(cutoff, now).await
}

/// Run the metadata task. Every path is silent (ack): unknown id, bad
/// UUID text (`ValidationError` → the broad `except`), S3 outage.
pub async fn run_metadata<S: AssetStore, O: ObjectStore>(
    store: &S,
    s3: &O,
    asset_id_raw: &str,
    now: DateTime<Utc>,
) {
    let asset_id = match Uuid::parse_str(asset_id_raw) {
        Ok(id) => id,
        Err(_) => {
            tracing::warn!(asset_id = asset_id_raw, "invalid asset id: silent skip");
            return;
        }
    };
    let key = match store.asset_key(asset_id).await {
        Ok(key) => key,
        Err(error) => {
            tracing::warn!(%asset_id, %error, "metadata fetch failed: silent skip");
            return;
        }
    };
    let key = match key {
        Some(key) => key,
        None => return,
    };
    let metadata = s3.head_object(&key).await.map(|head| head.to_json());
    if let Err(error) = store.save_storage_metadata(asset_id, metadata, now).await {
        tracing::warn!(%asset_id, %error, "metadata save failed: silent skip");
    }
}

/// The copy task's bound arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyRequest {
    pub entity_name: String,
    pub entity_identifier: String,
    pub project_id: Option<String>,
    /// Accepted and ignored (`copy_s3_object.py:123` never reads it).
    pub slug: String,
    pub user_id: String,
}

impl CopyRequest {
    pub fn from_job(args: &Value, kwargs: &Value) -> Option<CopyRequest> {
        Some(CopyRequest {
            entity_name: as_text(task_arg(args, kwargs, "entity_name", 0))?.to_string(),
            entity_identifier: as_text(task_arg(args, kwargs, "entity_identifier", 1))?.to_string(),
            project_id: as_text(task_arg(args, kwargs, "project_id", 2)).map(str::to_string),
            slug: as_text(task_arg(args, kwargs, "slug", 3))
                .unwrap_or_default()
                .to_string(),
            user_id: as_text(task_arg(args, kwargs, "user_id", 4))?.to_string(),
        })
    }
}

/// Run the copy task: extract → duplicate → rewrite → convert → save.
/// Any failure logs and takes the `[]` path (`Skipped`); partial writes
/// stay, exactly like the broad `except` around the Python body.
pub async fn run_copy<S: AssetStore, O: ObjectStore, L: LiveConvert>(
    store: &S,
    s3: &O,
    live: &L,
    request: &CopyRequest,
    now: DateTime<Utc>,
    new_uuid: &impl Fn() -> Uuid,
) -> CopyOutcome {
    let outcome = run_copy_inner(store, s3, live, request, now, new_uuid).await;
    if outcome.is_none() {
        tracing::warn!(
            entity = request.entity_name.as_str(),
            "copy task failed: returning []"
        );
        return CopyOutcome::Skipped;
    }
    CopyOutcome::Completed
}

async fn run_copy_inner<S: AssetStore, O: ObjectStore, L: LiveConvert>(
    store: &S,
    s3: &O,
    live: &L,
    request: &CopyRequest,
    now: DateTime<Utc>,
    new_uuid: &impl Fn() -> Uuid,
) -> Option<()> {
    let entity = CopyEntity::for_name(&request.entity_name)?;
    let entity_id = Uuid::parse_str(&request.entity_identifier).ok()?;
    let user_id = Uuid::parse_str(&request.user_id).ok()?;
    let project_id = request
        .project_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .ok()?;
    let row = store.entity(entity, entity_id).await.ok()??;
    // `entity.workspace.id`: a null workspace fails the task.
    let workspace_id = row.workspace_id?;
    let asset_ids: Vec<Uuid> = logic::extract_asset_ids(&row.description_html, DESCRIPTION_TAG)
        .iter()
        .map(|raw| Uuid::parse_str(raw).ok())
        .collect::<Option<Vec<_>>>()?;
    let originals = store
        .original_assets(workspace_id, project_id, asset_ids)
        .await
        .ok()?;
    let workspace_id_text = workspace_id.to_string();
    let mut pairs: Vec<AssetIdPair> = Vec::with_capacity(originals.len());
    for original in &originals {
        // `attributes.get("name")` on a non-dict crashes the task.
        let attributes = original.attributes.as_ref().and_then(Value::as_object)?;
        let name = logic::render_attribute_name(attributes);
        let dst_key =
            logic::destination_key(&workspace_id_text, &new_uuid().simple().to_string(), &name);
        let duplicate = NewAsset {
            id: new_uuid(),
            created_by_id: user_id,
            attributes: Value::Object(logic::duplicate_attributes(attributes)),
            asset_key: dst_key.clone(),
            workspace_id,
            user_id: None,
            draft_issue_id: None,
            project_id,
            issue_id: None,
            comment_id: None,
            page_id: None,
            entity_type: original.entity_type.clone(),
            size: original.size,
            storage_metadata: original.storage_metadata.clone(),
        };
        let entity_type = duplicate.entity_type.clone();
        let duplicate = duplicate.with_entity_field(entity_type.as_deref(), entity_id);
        let new_id = store.insert_duplicate(duplicate, now).await.ok()?;
        // A copy failure is logged and ignored: the pair still counts.
        s3.copy_object(&original.asset_key, &dst_key).await;
        pairs.push(AssetIdPair {
            old_asset_id: original.id.to_string(),
            new_asset_id: new_id.to_string(),
        });
    }
    if !pairs.is_empty() {
        let new_ids: Vec<Uuid> = pairs
            .iter()
            .filter_map(|pair| Uuid::parse_str(&pair.new_asset_id).ok())
            .collect();
        store.mark_uploaded(new_ids).await.ok()?;
    }
    // Unconditional: re-serialize, save, convert — even with zero pairs.
    let updated_html = logic::replace_asset_ids(&row.description_html, DESCRIPTION_TAG, &pairs);
    store
        .save_description_html(entity, entity_id, &updated_html, now)
        .await
        .ok()?;
    // `LiveConversionError` → `{}` → falsy → no second save, but the
    // task still succeeds (the duplicate is created regardless).
    let converted = match live.convert(&updated_html, entity.convert_variant()).await {
        Err(_) => return None,
        Ok(None) => return Some(()),
        Ok(Some(doc)) => doc,
    };
    let description_json = logic::description_json_or_empty(converted.description_json);
    store
        .save_description_docs(
            entity,
            entity_id,
            description_json,
            converted.description_binary,
            now,
        )
        .await
        .ok()?;
    Some(())
}

/// Postgres [`AssetStore`]: the exact Django query shapes from
/// [`logic`]'s SQL builders. Every table name is a literal at the call
/// sites below, never caller input.
#[derive(Debug, Clone)]
pub struct PgAssetStore {
    pool: sqlx::PgPool,
}

impl PgAssetStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

fn db_error(context: &str, error: sqlx::Error) -> String {
    format!("{context}: {error}")
}

impl AssetStore for PgAssetStore {
    async fn sweep_unuploaded(
        &self,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<u64, String> {
        let result = sqlx::query(logic::SWEEP_SQL)
            .bind(now)
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| db_error("sweep_unuploaded", e))?;
        Ok(result.rows_affected())
    }

    async fn asset_key(&self, asset_id: Uuid) -> Result<Option<String>, String> {
        sqlx::query_scalar::<_, Option<String>>(logic::METADATA_SELECT_SQL)
            .bind(asset_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| db_error("asset_key", e))
            .map(|row| row.flatten())
    }

    async fn save_storage_metadata(
        &self,
        asset_id: Uuid,
        metadata: Option<Value>,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        sqlx::query(logic::METADATA_SAVE_SQL)
            .bind(metadata.map(sqlx::types::Json))
            .bind(now)
            .bind(asset_id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| db_error("save_storage_metadata", e))
    }

    async fn entity(&self, entity: CopyEntity, id: Uuid) -> Result<Option<EntityRow>, String> {
        let row: Option<(Uuid, Option<Uuid>, String)> =
            sqlx::query_as(&logic::entity_select_sql(entity))
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| db_error("entity", e))?;
        Ok(row.map(|(id, workspace_id, description_html)| EntityRow {
            id,
            workspace_id,
            description_html,
        }))
    }

    async fn original_assets(
        &self,
        workspace_id: Uuid,
        project_id: Option<Uuid>,
        ids: Vec<Uuid>,
    ) -> Result<Vec<OriginalAsset>, String> {
        let sql = logic::original_assets_sql(project_id.is_none());
        let mut query =
            sqlx::query_as::<_, (Uuid, String, Value, f64, Option<String>, Option<Value>)>(&sql)
                .bind(workspace_id);
        if let Some(project) = project_id {
            query = query.bind(project);
        }
        let rows = query
            .bind(ids)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| db_error("original_assets", e))?;
        Ok(rows
            .into_iter()
            .map(
                |(id, asset_key, attributes, size, entity_type, storage_metadata)| OriginalAsset {
                    id,
                    asset_key,
                    attributes: Some(attributes),
                    size,
                    entity_type,
                    storage_metadata,
                },
            )
            .collect())
    }

    async fn insert_duplicate(&self, asset: NewAsset, now: DateTime<Utc>) -> Result<Uuid, String> {
        let id = Uuid::new_v4();
        sqlx::query(logic::DUPLICATE_INSERT_SQL)
            .bind(id)
            .bind(now)
            .bind(now)
            .bind(asset.created_by_id)
            .bind(None::<Uuid>)
            .bind(None::<DateTime<Utc>>)
            .bind(sqlx::types::Json(asset.attributes))
            .bind(asset.asset_key)
            .bind(None::<Uuid>)
            .bind(asset.workspace_id)
            .bind(asset.draft_issue_id)
            .bind(asset.project_id)
            .bind(asset.issue_id)
            .bind(asset.comment_id)
            .bind(asset.page_id)
            .bind(asset.entity_type)
            .bind(None::<String>)
            .bind(false)
            .bind(false)
            .bind(None::<String>)
            .bind(None::<String>)
            .bind(asset.size)
            .bind(false)
            .bind(asset.storage_metadata.map(sqlx::types::Json))
            .execute(&self.pool)
            .await
            .map(|_| id)
            .map_err(|e| db_error("insert_duplicate", e))
    }

    async fn mark_uploaded(&self, ids: Vec<Uuid>) -> Result<(), String> {
        sqlx::query(logic::MARK_UPLOADED_SQL)
            .bind(ids)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| db_error("mark_uploaded", e))
    }

    async fn save_description_html(
        &self,
        entity: CopyEntity,
        id: Uuid,
        html: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        sqlx::query(&logic::description_save_sql(entity))
            .bind(html)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| db_error("save_description_html", e))
    }

    async fn save_description_docs(
        &self,
        entity: CopyEntity,
        id: Uuid,
        description_json: Value,
        description_binary: Option<Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let sql = logic::description_docs_save_sql(entity, description_binary.is_some());
        let mut query = sqlx::query(&sql).bind(sqlx::types::Json(description_json));
        if let Some(binary) = description_binary {
            query = query.bind(binary);
        }
        query
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| db_error("save_description_docs", e))
    }
}

/// The `.delay()` equivalent for the sweep: no args, exactly what beat
/// publishes (`celery.py:50-53` carries no args or kwargs).
pub fn delay_sweep() -> CeleryTaskMessage {
    CeleryTaskMessage::new(TASK_DELETE_UNUPLOADED, Vec::new(), Map::new())
}

/// `get_asset_object_metadata.delay(asset_id=...)` (the kwarg form used
/// by `api/views/asset.py` and `app/views/asset/v2.py`).
pub fn delay_metadata(asset_id: &str) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("asset_id".to_string(), Value::String(asset_id.to_string()));
    CeleryTaskMessage::new(TASK_GET_METADATA, Vec::new(), kwargs)
}

/// `copy_s3_objects_of_description_and_assets.delay(entity_name=...,
/// entity_identifier=..., project_id=..., slug=..., user_id=...)` (the
/// kwarg form used by `app/views/page/base.py:620-626`).
pub fn delay_copy(
    entity_name: &str,
    entity_identifier: &str,
    project_id: Option<&str>,
    slug: &str,
    user_id: &str,
) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert(
        "entity_name".to_string(),
        Value::String(entity_name.to_string()),
    );
    kwargs.insert(
        "entity_identifier".to_string(),
        Value::String(entity_identifier.to_string()),
    );
    kwargs.insert(
        "project_id".to_string(),
        project_id
            .map(|p| Value::String(p.to_string()))
            .unwrap_or(Value::Null),
    );
    kwargs.insert("slug".to_string(), Value::String(slug.to_string()));
    kwargs.insert("user_id".to_string(), Value::String(user_id.to_string()));
    CeleryTaskMessage::new(TASK_COPY_S3_OBJECTS, Vec::new(), kwargs)
}

/// Install the three worker handlers owning the asset task names.
/// Failures that Python raises (sweep env/DB errors) park the row;
/// swallowed paths ack. Retries never trigger: Python has no autoretry
/// on these tasks, so handlers return `Ok` verdicts only.
pub fn register_assets<S, L>(registry: &mut Registry, pool: sqlx::PgPool, s3: Arc<S>, live: Arc<L>)
where
    S: ObjectStore + 'static,
    L: LiveConvert + 'static,
{
    register_sweep(registry, pool.clone());
    register_metadata(registry, pool.clone(), s3.clone());
    register_copy(registry, pool, s3, live);
}

/// Install the sweep handler.
pub fn register_sweep(registry: &mut Registry, pool: sqlx::PgPool) {
    let handler: Handler = Arc::new(move |_job: crate::queue::JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            let days = match std::env::var(logic::UNUPLOADED_ASSET_DELETE_DAYS_ENV)
                .ok()
                .as_deref()
            {
                None => logic::UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT,
                Some(raw) => match logic::parse_delete_days(raw) {
                    Ok(days) => days,
                    Err(error) => return Ok(Verdict::Fail { error }),
                },
            };
            let store = PgAssetStore::new(pool);
            match run_sweep(&store, days, Utc::now()).await {
                Ok(_) => Ok(Verdict::Ack),
                Err(error) => Ok(Verdict::Fail { error }),
            }
        })
    });
    registry.register(TASK_DELETE_UNUPLOADED, handler);
}

/// Install the metadata handler with the given object store.
pub fn register_metadata<S: ObjectStore + 'static>(
    registry: &mut Registry,
    pool: sqlx::PgPool,
    s3: Arc<S>,
) {
    let handler: Handler = Arc::new(move |job: crate::queue::JobRow| {
        let pool = pool.clone();
        let s3 = s3.clone();
        Box::pin(async move {
            let asset_id = as_text(task_arg(&job.args, &job.kwargs, "asset_id", 0))
                .unwrap_or_default()
                .to_string();
            let store = PgAssetStore::new(pool);
            run_metadata(&store, s3.as_ref(), &asset_id, Utc::now()).await;
            Ok(Verdict::Ack)
        })
    });
    registry.register(TASK_GET_METADATA, handler);
}

/// Install the copy handler with the given object store and converter.
pub fn register_copy<S, L>(registry: &mut Registry, pool: sqlx::PgPool, s3: Arc<S>, live: Arc<L>)
where
    S: ObjectStore + 'static,
    L: LiveConvert + 'static,
{
    let handler: Handler = Arc::new(move |job: crate::queue::JobRow| {
        let pool = pool.clone();
        let s3 = s3.clone();
        let live = live.clone();
        Box::pin(async move {
            let store = PgAssetStore::new(pool);
            match CopyRequest::from_job(&job.args, &job.kwargs) {
                None => {
                    tracing::warn!("copy task missing arguments: returning []");
                    Ok(Verdict::Ack)
                }
                Some(request) => {
                    run_copy(
                        &store,
                        s3.as_ref(),
                        live.as_ref(),
                        &request,
                        Utc::now(),
                        &Uuid::new_v4,
                    )
                    .await;
                    Ok(Verdict::Ack)
                }
            }
        })
    });
    registry.register(TASK_COPY_S3_OBJECTS, handler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn fixture() -> Value {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/tasks_cleanup/assets.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("assets fixture exists"))
            .expect("assets fixture is valid JSON")
    }

    /// Byte-identical replay of the recorded helper goldens.
    #[test]
    fn fixture_goldens_replay() {
        let fx = fixture();
        let html = "<p>t</p><image-component src=\"id-1\"></image-component><img src=\"keep\"/>";
        let golden_ids: Vec<String> = fx["extract_asset_ids"]["golden"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(logic::extract_asset_ids(html, DESCRIPTION_TAG), golden_ids);
        let pairs = [AssetIdPair {
            old_asset_id: "id-1".to_string(),
            new_asset_id: "id-2".to_string(),
        }];
        assert_eq!(
            logic::replace_asset_ids(html, DESCRIPTION_TAG, &pairs),
            fx["replace_asset_ids"]["golden"].as_str().unwrap(),
        );
        let nomatch = [AssetIdPair {
            old_asset_id: "zzz".to_string(),
            new_asset_id: "id-2".to_string(),
        }];
        assert_eq!(
            logic::replace_asset_ids(html, DESCRIPTION_TAG, &nomatch),
            fx["replace_asset_ids"]["no_match_unchanged_except_reparse"]
                .as_str()
                .unwrap(),
        );
        for (entity_type, fields) in fx["entity_id_fields"].as_object().unwrap() {
            let fields = fields.as_object().unwrap();
            if fields.is_empty() {
                assert_eq!(logic::entity_id_field(entity_type), None, "{entity_type}");
            } else {
                let (field, _) = fields.iter().next().unwrap();
                assert_eq!(
                    logic::entity_id_field(entity_type),
                    Some(field.as_str()),
                    "{entity_type}"
                );
            }
        }
    }

    #[test]
    fn wire_names_and_payload_shapes() {
        assert_eq!(
            TASK_DELETE_UNUPLOADED,
            "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset"
        );
        assert_eq!(
            TASK_GET_METADATA,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(
            TASK_COPY_S3_OBJECTS,
            "pi_dash.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets"
        );
        let sweep = delay_sweep();
        assert_eq!(sweep.task, TASK_DELETE_UNUPLOADED);
        assert!(sweep.args.is_empty() && sweep.kwargs.is_empty());
        let meta = delay_metadata("asset-id");
        assert_eq!(meta.task, TASK_GET_METADATA);
        assert_eq!(meta.kwargs["asset_id"], json!("asset-id"));
        let copy = delay_copy("PAGE", "eid", Some("pid"), "slug", "uid");
        assert_eq!(copy.task, TASK_COPY_S3_OBJECTS);
        assert_eq!(copy.kwargs["entity_name"], json!("PAGE"));
        assert_eq!(copy.kwargs["project_id"], json!("pid"));
        assert_eq!(copy.kwargs["slug"], json!("slug"));
        let null_project = delay_copy("ISSUE", "eid", None, "s", "uid");
        assert_eq!(null_project.kwargs["project_id"], Value::Null);
        // The oracle publishes positionally; binding must accept it.
        let positional =
            CopyRequest::from_job(&json!(["ISSUE", "eid", "pid", "ws", "uid"]), &json!({}))
                .unwrap();
        assert_eq!(positional.entity_name, "ISSUE");
        assert_eq!(positional.project_id.as_deref(), Some("pid"));
        assert_eq!(positional.slug, "ws");
    }

    // --- Fakes ---------------------------------------------------------

    #[derive(Debug, Clone)]
    struct FakeAsset {
        key: String,
        attributes: Option<Value>,
        size: f64,
        entity_type: Option<String>,
        storage: Option<Value>,
        uploaded: bool,
        deleted_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        created_by: Option<Uuid>,
        workspace_id: Uuid,
        user_id: Option<Uuid>,
        project_id: Option<Uuid>,
        issue_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        page_id: Option<Uuid>,
        draft_issue_id: Option<Uuid>,
    }

    #[derive(Debug, Clone)]
    struct FakeEntity {
        workspace_id: Option<Uuid>,
        html: String,
        json: Value,
        binary: Option<Vec<u8>>,
    }

    #[derive(Debug, Default)]
    struct StoreState {
        assets: HashMap<Uuid, FakeAsset>,
        entities: HashMap<(String, Uuid), FakeEntity>,
        saves: Vec<String>,
    }

    struct FakeStore {
        state: Mutex<StoreState>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                state: Mutex::new(StoreState::default()),
            }
        }
    }

    impl AssetStore for FakeStore {
        async fn sweep_unuploaded(
            &self,
            cutoff: DateTime<Utc>,
            now: DateTime<Utc>,
        ) -> Result<u64, String> {
            let mut state = self.state.lock().unwrap();
            let mut stamped = 0;
            for asset in state.assets.values_mut() {
                if asset.deleted_at.is_none() && asset.created_at < cutoff && !asset.uploaded {
                    asset.deleted_at = Some(now);
                    stamped += 1;
                }
            }
            Ok(stamped)
        }

        async fn asset_key(&self, asset_id: Uuid) -> Result<Option<String>, String> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .assets
                .get(&asset_id)
                .and_then(|a| {
                    if a.deleted_at.is_none() {
                        Some(a.key.clone())
                    } else {
                        None
                    }
                }))
        }

        async fn save_storage_metadata(
            &self,
            asset_id: Uuid,
            metadata: Option<Value>,
            now: DateTime<Utc>,
        ) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            let asset = state.assets.get_mut(&asset_id).ok_or("missing")?;
            asset.storage = metadata;
            asset.updated_at = now;
            state.saves.push(format!("metadata:{asset_id}"));
            Ok(())
        }

        async fn entity(&self, entity: CopyEntity, id: Uuid) -> Result<Option<EntityRow>, String> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .entities
                .get(&(entity.table().to_string(), id))
                .map(|e| EntityRow {
                    id,
                    workspace_id: e.workspace_id,
                    description_html: e.html.clone(),
                }))
        }

        async fn original_assets(
            &self,
            workspace_id: Uuid,
            project_id: Option<Uuid>,
            ids: Vec<Uuid>,
        ) -> Result<Vec<OriginalAsset>, String> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .assets
                .iter()
                .filter(|(id, a)| {
                    a.deleted_at.is_none()
                        && a.workspace_id == workspace_id
                        && a.project_id == project_id
                        && ids.contains(id)
                })
                .map(|(id, a)| OriginalAsset {
                    id: *id,
                    asset_key: a.key.clone(),
                    attributes: a.attributes.clone(),
                    size: a.size,
                    entity_type: a.entity_type.clone(),
                    storage_metadata: a.storage.clone(),
                })
                .collect())
        }

        async fn insert_duplicate(
            &self,
            asset: NewAsset,
            now: DateTime<Utc>,
        ) -> Result<Uuid, String> {
            let mut state = self.state.lock().unwrap();
            let id = asset.id;
            state.assets.insert(
                id,
                FakeAsset {
                    key: asset.asset_key,
                    attributes: Some(asset.attributes),
                    size: asset.size,
                    entity_type: asset.entity_type,
                    storage: asset.storage_metadata,
                    uploaded: false,
                    deleted_at: None,
                    created_at: now,
                    updated_at: now,
                    created_by: Some(asset.created_by_id),
                    workspace_id: asset.workspace_id,
                    user_id: asset.user_id,
                    project_id: asset.project_id,
                    issue_id: asset.issue_id,
                    comment_id: asset.comment_id,
                    page_id: asset.page_id,
                    draft_issue_id: asset.draft_issue_id,
                },
            );
            Ok(id)
        }

        async fn mark_uploaded(&self, ids: Vec<Uuid>) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            for id in ids {
                if let Some(asset) = state.assets.get_mut(&id) {
                    if asset.deleted_at.is_none() {
                        asset.uploaded = true;
                    }
                }
            }
            Ok(())
        }

        async fn save_description_html(
            &self,
            entity: CopyEntity,
            id: Uuid,
            html: &str,
            now: DateTime<Utc>,
        ) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            let row = state
                .entities
                .get_mut(&(entity.table().to_string(), id))
                .ok_or("missing entity")?;
            row.html = html.to_string();
            let _ = now;
            state.saves.push(format!("html:{id}"));
            Ok(())
        }

        async fn save_description_docs(
            &self,
            entity: CopyEntity,
            id: Uuid,
            description_json: Value,
            description_binary: Option<Vec<u8>>,
            now: DateTime<Utc>,
        ) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            let row = state
                .entities
                .get_mut(&(entity.table().to_string(), id))
                .ok_or("missing entity")?;
            row.json = description_json;
            if description_binary.is_some() {
                row.binary = description_binary;
            }
            let _ = now;
            state.saves.push(format!("docs:{id}"));
            Ok(())
        }
    }

    struct FakeS3 {
        heads: HashMap<String, HeadMeta>,
        copies: Mutex<Vec<(String, String)>>,
    }

    impl ObjectStore for FakeS3 {
        async fn head_object(&self, key: &str) -> Option<HeadMeta> {
            self.heads.get(key).cloned()
        }

        async fn copy_object(&self, src_key: &str, dst_key: &str) {
            self.copies
                .lock()
                .unwrap()
                .push((src_key.to_string(), dst_key.to_string()));
        }
    }

    struct FakeLive {
        answer: Option<Option<ConvertedDoc>>,
        seen: Mutex<Vec<(String, String)>>,
    }

    impl LiveConvert for FakeLive {
        async fn convert(&self, html: &str, variant: &str) -> Result<Option<ConvertedDoc>, String> {
            self.seen
                .lock()
                .unwrap()
                .push((html.to_string(), variant.to_string()));
            self.answer.clone().ok_or("live transport down".to_string())
        }
    }

    /// Deterministic UUID source: each call yields the next id.
    struct CounterUuid {
        next: Mutex<u128>,
    }

    impl CounterUuid {
        fn gen(&self) -> Uuid {
            let mut next = self.next.lock().unwrap();
            let id = Uuid::from_u128(*next);
            *next += 1;
            id
        }
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(6, 0, 0)
            .unwrap()
            .and_utc()
    }

    fn live_doc() -> ConvertedDoc {
        ConvertedDoc {
            description_json: Some(json!({"doc": true})),
            description_binary: Some(vec![1, 2, 3]),
        }
    }

    #[tokio::test]
    async fn sweep_stamps_only_eligible_rows() {
        let store = FakeStore::new();
        let now = utc(2026, 9, 28);
        let stale = Uuid::new_v4();
        let fresh = Uuid::new_v4();
        let uploaded = Uuid::new_v4();
        let deleted = Uuid::new_v4();
        let ws = Uuid::new_v4();
        {
            let mut state = store.state.lock().unwrap();
            let mk = |created: DateTime<Utc>, uploaded: bool, deleted: bool| FakeAsset {
                key: "k".to_string(),
                attributes: Some(json!({})),
                size: 0.0,
                entity_type: None,
                storage: None,
                uploaded,
                deleted_at: deleted.then_some(now),
                created_at: created,
                updated_at: created,
                created_by: None,
                workspace_id: ws,
                user_id: None,
                project_id: None,
                issue_id: None,
                comment_id: None,
                page_id: None,
                draft_issue_id: None,
            };
            state
                .assets
                .insert(stale, mk(utc(2026, 9, 20), false, false));
            state
                .assets
                .insert(fresh, mk(utc(2026, 9, 27), false, false));
            state
                .assets
                .insert(uploaded, mk(utc(2026, 9, 20), true, false));
            state
                .assets
                .insert(deleted, mk(utc(2026, 9, 20), false, true));
        }
        let stamped = run_sweep(&store, 7, now).await.unwrap();
        assert_eq!(stamped, 1);
        let state = store.state.lock().unwrap();
        assert_eq!(state.assets[&stale].deleted_at, Some(now));
        assert_eq!(state.assets[&fresh].deleted_at, None);
        assert_eq!(state.assets[&uploaded].deleted_at, None);
    }

    #[tokio::test]
    async fn metadata_stores_head_or_null() {
        let store = FakeStore::new();
        let now = utc(2026, 9, 28);
        let id = Uuid::new_v4();
        let ws = Uuid::new_v4();
        store.state.lock().unwrap().assets.insert(
            id,
            FakeAsset {
                key: "contract/stale.bin".to_string(),
                attributes: Some(json!({})),
                size: 0.0,
                entity_type: None,
                storage: Some(json!({"old": true})),
                uploaded: false,
                deleted_at: None,
                created_at: now,
                updated_at: now,
                created_by: None,
                workspace_id: ws,
                user_id: None,
                project_id: None,
                issue_id: None,
                comment_id: None,
                page_id: None,
                draft_issue_id: None,
            },
        );
        let head = HeadMeta {
            content_type: Some("image/png".to_string()),
            content_length: Some(9),
            last_modified: None,
            etag: None,
            metadata: Map::new(),
        };
        let s3 = FakeS3 {
            heads: HashMap::from([("contract/stale.bin".to_string(), head.clone())]),
            copies: Mutex::new(vec![]),
        };
        run_metadata(&store, &s3, &id.to_string(), now).await;
        {
            let state = store.state.lock().unwrap();
            assert_eq!(state.assets[&id].storage, Some(head.to_json()));
            assert_eq!(state.assets[&id].updated_at, now);
        }

        // Unknown head answers None → stored as NULL.
        let empty = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        run_metadata(&store, &empty, &id.to_string(), now).await;
        assert_eq!(store.state.lock().unwrap().assets[&id].storage, None);

        // Missing rows and bad UUIDs are silent: no further saves.
        let saves = store.state.lock().unwrap().saves.len();
        run_metadata(&store, &empty, &Uuid::new_v4().to_string(), now).await;
        run_metadata(&store, &empty, "not-a-uuid", now).await;
        assert_eq!(store.state.lock().unwrap().saves.len(), saves);
    }

    fn copy_world() -> (FakeStore, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let store = FakeStore::new();
        let ws = Uuid::from_u128(100);
        let project = Uuid::from_u128(101);
        let user = Uuid::from_u128(102);
        let entity = Uuid::from_u128(103);
        let asset = Uuid::from_u128(104);
        store.state.lock().unwrap().entities.insert(
            ("issues".to_string(), entity),
            FakeEntity {
                workspace_id: Some(ws),
                html: format!("<p>t</p><image-component src=\"{asset}\"></image-component>"),
                json: json!({}),
                binary: None,
            },
        );
        store.state.lock().unwrap().assets.insert(
            asset,
            FakeAsset {
                key: "100/orig-photo.png".to_string(),
                attributes: Some(
                    json!({"name": "photo.png", "type": "png", "size": 9, "extra": 1}),
                ),
                size: 9.0,
                entity_type: Some("ISSUE_ATTACHMENT".to_string()),
                storage: Some(json!({"ETag": "e"})),
                uploaded: true,
                deleted_at: None,
                created_at: utc(2026, 9, 20),
                updated_at: utc(2026, 9, 20),
                created_by: Some(user),
                workspace_id: ws,
                user_id: None,
                project_id: Some(project),
                issue_id: Some(entity),
                comment_id: None,
                page_id: None,
                draft_issue_id: None,
            },
        );
        (store, ws, project, user, entity, asset)
    }

    fn copy_request(entity: Uuid, project: Uuid, user: Uuid) -> CopyRequest {
        CopyRequest {
            entity_name: "ISSUE".to_string(),
            entity_identifier: entity.to_string(),
            project_id: Some(project.to_string()),
            slug: "ws".to_string(),
            user_id: user.to_string(),
        }
    }

    #[tokio::test]
    async fn copy_duplicates_rewrites_and_saves_docs() {
        let (store, ws, project, user, entity, asset) = copy_world();
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: Some(Some(live_doc())),
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(200),
        };
        let now = utc(2026, 9, 28);

        let outcome = run_copy(
            &store,
            &s3,
            &live,
            &copy_request(entity, project, user),
            now,
            &|| uuids.gen(),
        )
        .await;
        assert_eq!(outcome, CopyOutcome::Completed);

        // One S3 copy, key shape `{ws}/{32hex}-{name}`.
        let copies = s3.copies.lock().unwrap();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].0, "100/orig-photo.png");
        let dst = copies[0].1.clone();
        assert!(dst.starts_with(&format!("{ws}/")), "{dst}");
        assert!(dst.ends_with("-photo.png"), "{dst}");
        assert_eq!(
            dst.len(),
            ws.to_string().len() + 1 + 32 + 1 + "photo.png".len()
        );
        drop(copies);

        // One new row: attribute subset, FK mapping, finalized uploaded.
        let state = store.state.lock().unwrap();
        let dup = state
            .assets
            .iter()
            .find(|(id, _)| **id != asset)
            .expect("duplicate");
        assert_eq!(
            dup.1.attributes,
            Some(json!({"name": "photo.png", "type": "png", "size": 9}))
        );
        assert_eq!(dup.1.entity_type.as_deref(), Some("ISSUE_ATTACHMENT"));
        assert_eq!(dup.1.size, 9.0);
        assert_eq!(dup.1.storage, Some(json!({"ETag": "e"})));
        assert_eq!(dup.1.created_by, Some(user));
        assert_eq!(dup.1.issue_id, Some(entity));
        // The copy scope pins workspace + project; every other FK stays null.
        assert_eq!(dup.1.workspace_id, ws);
        assert_eq!(dup.1.project_id, Some(project));
        assert_eq!(dup.1.user_id, None);
        assert_eq!(dup.1.comment_id, None);
        assert_eq!(dup.1.page_id, None);
        assert_eq!(dup.1.draft_issue_id, None);
        assert!(dup.1.uploaded);
        assert_eq!(dup.1.key, s3.copies.lock().unwrap()[0].1);

        // Description rewritten to the new id; docs refreshed.
        let ent = &state.entities[&("issues".to_string(), entity)];
        assert!(ent.html.contains(&dup.0.to_string()));
        assert!(!ent.html.contains(&asset.to_string()));
        assert_eq!(ent.json, json!({"doc": true}));
        assert_eq!(ent.binary, Some(vec![1, 2, 3]));
        assert!(state.saves.iter().any(|s| s.starts_with("html:")));
        assert!(state.saves.iter().any(|s| s.starts_with("docs:")));
        drop(state);

        // Document variant for issues (pages use "rich").
        assert_eq!(live.seen.lock().unwrap()[0].1, "document");
    }

    #[tokio::test]
    async fn copy_skips_silently_on_bad_input() {
        let (store, _ws, project, user, entity, asset) = copy_world();
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: Some(Some(live_doc())),
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(300),
        };
        let now = utc(2026, 9, 28);
        let base = copy_request(entity, project, user);

        // Unsupported entity, missing entity, bad UUIDs: no writes.
        for request in [
            CopyRequest {
                entity_name: "COMMENT".to_string(),
                ..base.clone()
            },
            CopyRequest {
                entity_identifier: Uuid::new_v4().to_string(),
                ..base.clone()
            },
            CopyRequest {
                entity_identifier: "nope".to_string(),
                ..base.clone()
            },
            CopyRequest {
                user_id: "nope".to_string(),
                ..base.clone()
            },
            CopyRequest {
                project_id: Some("nope".to_string()),
                ..base.clone()
            },
        ] {
            // Read each counter under its own statement: holding two
            // guards on the same non-reentrant Mutex (e.g. inside one
            // tuple) deadlocks.
            let before_assets = store.state.lock().unwrap().assets.len();
            let before_saves = store.state.lock().unwrap().saves.len();
            let before_copies = s3.copies.lock().unwrap().len();
            let outcome = run_copy(&store, &s3, &live, &request, now, &|| uuids.gen()).await;
            assert_eq!(outcome, CopyOutcome::Skipped);
            assert_eq!(store.state.lock().unwrap().assets.len(), before_assets);
            assert_eq!(store.state.lock().unwrap().saves.len(), before_saves);
            assert_eq!(s3.copies.lock().unwrap().len(), before_copies);
        }
        // Null workspace fails like `entity.workspace.id` raising.
        store
            .state
            .lock()
            .unwrap()
            .entities
            .get_mut(&("issues".to_string(), entity))
            .unwrap()
            .workspace_id = None;
        assert_eq!(
            run_copy(&store, &s3, &live, &base, now, &|| uuids.gen()).await,
            CopyOutcome::Skipped
        );
        // Corrupt asset ids in HTML fail the whole task before any write.
        {
            let mut state = store.state.lock().unwrap();
            let ent = state
                .entities
                .get_mut(&("issues".to_string(), entity))
                .unwrap();
            ent.workspace_id = Some(Uuid::from_u128(100));
            ent.html = "<image-component src=\"bogus\"></image-component>".to_string();
        }
        let before = store.state.lock().unwrap().assets.len();
        assert_eq!(
            run_copy(&store, &s3, &live, &base, now, &|| uuids.gen()).await,
            CopyOutcome::Skipped
        );
        assert_eq!(store.state.lock().unwrap().assets.len(), before);
        // Non-object attributes fail like `.get` raising AttributeError.
        {
            let mut state = store.state.lock().unwrap();
            let ent = state
                .entities
                .get_mut(&("issues".to_string(), entity))
                .unwrap();
            ent.html = format!("<image-component src=\"{asset}\"></image-component>");
            state.assets.get_mut(&asset).unwrap().attributes = Some(json!([1]));
        }
        assert_eq!(
            run_copy(&store, &s3, &live, &base, now, &|| uuids.gen()).await,
            CopyOutcome::Skipped
        );
    }

    #[tokio::test]
    async fn copy_without_live_answer_skips_docs_save() {
        let (store, _ws, project, user, entity, _asset) = copy_world();
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: Some(None),
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(400),
        };
        let outcome = run_copy(
            &store,
            &s3,
            &live,
            &copy_request(entity, project, user),
            utc(2026, 9, 28),
            &|| uuids.gen(),
        )
        .await;
        assert_eq!(outcome, CopyOutcome::Completed);
        let state = store.state.lock().unwrap();
        assert!(state.saves.iter().any(|s| s.starts_with("html:")));
        assert!(!state.saves.iter().any(|s| s.starts_with("docs:")));
    }

    #[tokio::test]
    async fn copy_transport_failure_skips() {
        let (store, _ws, project, user, entity, _asset) = copy_world();
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: None,
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(500),
        };
        let outcome = run_copy(
            &store,
            &s3,
            &live,
            &copy_request(entity, project, user),
            utc(2026, 9, 28),
            &|| uuids.gen(),
        )
        .await;
        assert_eq!(outcome, CopyOutcome::Skipped);
    }

    #[tokio::test]
    async fn copy_missing_name_renders_none_in_key() {
        let (store, ws, project, user, entity, asset) = copy_world();
        store
            .state
            .lock()
            .unwrap()
            .assets
            .get_mut(&asset)
            .unwrap()
            .attributes = Some(json!({"type": "png"}));
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: Some(None),
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(600),
        };
        let outcome = run_copy(
            &store,
            &s3,
            &live,
            &copy_request(entity, project, user),
            utc(2026, 9, 28),
            &|| uuids.gen(),
        )
        .await;
        assert_eq!(outcome, CopyOutcome::Completed);
        let dst = s3.copies.lock().unwrap()[0].1.clone();
        assert!(dst.ends_with("-None"), "{dst}");
        assert!(dst.starts_with(&format!("{ws}/")), "{dst}");
    }

    #[tokio::test]
    async fn copy_with_no_matching_assets_still_saves_and_converts() {
        let (store, _ws, project, user, entity, _asset) = copy_world();
        store
            .state
            .lock()
            .unwrap()
            .entities
            .get_mut(&("issues".to_string(), entity))
            .unwrap()
            .html = "<p>no assets here</p>".to_string();
        let s3 = FakeS3 {
            heads: HashMap::new(),
            copies: Mutex::new(vec![]),
        };
        let live = FakeLive {
            answer: Some(Some(live_doc())),
            seen: Mutex::new(vec![]),
        };
        let uuids = CounterUuid {
            next: Mutex::new(700),
        };
        let outcome = run_copy(
            &store,
            &s3,
            &live,
            &copy_request(entity, project, user),
            utc(2026, 9, 28),
            &|| uuids.gen(),
        )
        .await;
        assert_eq!(outcome, CopyOutcome::Completed);
        assert!(s3.copies.lock().unwrap().is_empty());
        let state = store.state.lock().unwrap();
        assert_eq!(state.assets.len(), 1);
        assert!(state.saves.iter().any(|s| s.starts_with("html:")));
        assert!(state.saves.iter().any(|s| s.starts_with("docs:")));
        assert_eq!(live.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn new_asset_applies_every_mapped_column() {
        let base = NewAsset {
            id: Uuid::new_v4(),
            created_by_id: Uuid::new_v4(),
            attributes: json!({}),
            asset_key: "k".to_string(),
            workspace_id: Uuid::from_u128(1),
            user_id: None,
            draft_issue_id: None,
            project_id: None,
            issue_id: None,
            comment_id: None,
            page_id: None,
            entity_type: None,
            size: 0.0,
            storage_metadata: None,
        };
        let eid = Uuid::from_u128(9);
        assert_eq!(
            base.clone()
                .with_entity_field(Some("WORKSPACE_LOGO"), eid)
                .workspace_id,
            eid
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("PROJECT_COVER"), eid)
                .project_id,
            Some(eid)
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("USER_AVATAR"), eid)
                .user_id,
            Some(eid)
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("ISSUE_ATTACHMENT"), eid)
                .issue_id,
            Some(eid)
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("PAGE_DESCRIPTION"), eid)
                .page_id,
            Some(eid)
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("COMMENT_DESCRIPTION"), eid)
                .comment_id,
            Some(eid)
        );
        assert_eq!(
            base.clone()
                .with_entity_field(Some("DRAFT_ISSUE_DESCRIPTION"), eid)
                .draft_issue_id,
            Some(eid)
        );
        // Unmapped types set no column.
        assert_eq!(
            base.clone()
                .with_entity_field(Some("DRAFT_ISSUE_ATTACHMENT"), eid),
            base
        );
        assert_eq!(base.clone().with_entity_field(None, eid), base);
    }

    #[test]
    fn request_binding_accepts_kwargs_and_positional() {
        let kwargs = json!({
            "entity_name": "PAGE",
            "entity_identifier": "eid",
            "project_id": "pid",
            "slug": "s",
            "user_id": "uid",
        });
        let from_kwargs = CopyRequest::from_job(&json!([]), &kwargs).unwrap();
        assert_eq!(from_kwargs.slug, "s");
        // Explicit null project binds None without falling back.
        let nulls = CopyRequest::from_job(
            &json!(["X"]),
            &json!({"entity_name": "PAGE", "entity_identifier": "e", "project_id": null, "user_id": "u"}),
        )
        .unwrap();
        assert_eq!(nulls.project_id, None);
        // Missing required fields fail binding (the `[]` path upstream).
        assert!(CopyRequest::from_job(&json!([]), &json!({})).is_none());
    }

    #[tokio::test]
    async fn registry_owns_all_three_tasks_after_register() {
        // `connect_lazy` never touches the network, but sqlx still needs
        // a Tokio context to build the pool: registration wiring stays
        // testable with no database.
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        assert!(!registry.owns(TASK_DELETE_UNUPLOADED));
        register_assets(
            &mut registry,
            pool,
            Arc::new(UnavailableObjectStore),
            Arc::new(NoopLiveConvert),
        );
        assert!(registry.owns(TASK_DELETE_UNUPLOADED));
        assert!(registry.owns(TASK_GET_METADATA));
        assert!(registry.owns(TASK_COPY_S3_OBJECTS));
    }
}
