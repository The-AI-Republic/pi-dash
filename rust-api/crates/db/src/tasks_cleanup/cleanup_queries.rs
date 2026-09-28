//! D-09 cleanup retention + mongo flush queries.
//!
//! Port of the driver and queryset layer of
//! `apps/api/pi_dash/bgtasks/cleanup_task.py` (`:38-:163`, `:267-:421`):
//! `get_mongo_collection`, `flush_to_mongo_and_delete`,
//! `process_cleanup_task`, and the five `get_*_queryset` functions.
//!
//! Static statements are string constants or builder functions emitting the
//! same text Django's compiler emits (recorded in
//! `rust-api/fixtures/tasks_cleanup/cleanup.json`), with the cutoff rendered
//! exactly as Django renders it (`2026-08-29 06:00:00+00:00`). SQL execution
//! uses runtime `sqlx::query` (no `query!` macros): there is no build-time
//! database, matching the merged `api/src/app_issues` precedent.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `HARD_DELETE_AFTER_DAYS` comes from the environment with default `30`;
//!   `settings.HARD_DELETE_AFTER_DAYS` (default `60`) is NOT consulted.
//! * The email predicate is `sent_at__lte`, not `created_at__lte`.
//! * `total_processed` counts flushed records even when Mongo archival
//!   failed and nothing was deleted (the counter increments
//!   unconditionally after each flush call, `:139` / `:153`).
//! * On archival failure the loop continues with the next batch; only the
//!   failed batch skips its delete.
//! * `delete()` cascades are counted in Python's logged count; here
//!   `rows_affected` (the same rows; nothing references these log tables).
//! * There is no per-record `try/except` in this file: only `BulkWriteError`
//!   around the whole `bulk_write` is caught. Any other failure (row decode,
//!   Mongo transport, Postgres) aborts the task, which the worker retries.
//!   The `Result` types below model exactly that.

use std::future::Future;

use chrono::{DateTime, Utc};
use mongodb::bson::{spec::BinarySubtype, Binary, Bson, Document};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_types::tasks_cleanup::cleanup_dto::{
    self, ApiLogRow, BlobValue, EmailLogRow, IssueDescriptionVersionRow, PageVersionRow, ScalarKey,
    WebhookLogRow,
};

/// Flush threshold shared by all five tasks (`cleanup_task.py:35`).
///
/// Note: the webhook iterator uses `chunk_size=100`, but that is the fetch
/// chunk, not the flush threshold — every task flushes at 500.
pub const BATCH_SIZE: usize = 500;

pub const TABLE_API_LOGS: &str = "api_activity_logs";
pub const TABLE_EMAIL_LOGS: &str = "email_notification_logs";
pub const TABLE_PAGE_VERSIONS: &str = "page_versions";
pub const TABLE_ISSUE_DESCRIPTION_VERSIONS: &str = "issue_description_versions";
pub const TABLE_WEBHOOK_LOGS: &str = "webhook_logs";

/// Every failure the cleanup plane reports.
#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("cleanup database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("cleanup mongo transport error: {0}")]
    MongoTransport(String),
}

/// Render the retention cutoff the way Django renders the `__lte` bound in
/// the fixture SQL: `2026-08-29 06:00:00+00:00`.
pub fn render_cutoff(cutoff: &DateTime<Utc>) -> String {
    cleanup_dto::format_py_datetime(cutoff)
}

/// `timezone.now() - timedelta(days=…)` (`cleanup_task.py:270`).
pub fn cutoff_time(days: i64) -> DateTime<Utc> {
    Utc::now() - chrono::Duration::days(days)
}

// ---------------------------------------------------------------------------
// Queryset SQL: same text Django emits (asserted against the fixture).
// ---------------------------------------------------------------------------

/// `get_api_logs_queryset` (`cleanup_task.py:267-291`): `all_objects` (no
/// `deleted_at` scope) + `created_at__lte` + default `-created_at` ordering.
pub fn api_logs_sql(cutoff: &str) -> String {
    format!(
        "SELECT \"api_activity_logs\".\"id\", \"api_activity_logs\".\"created_at\", \
        \"api_activity_logs\".\"token_identifier\", \"api_activity_logs\".\"path\", \
        \"api_activity_logs\".\"method\", \"api_activity_logs\".\"query_params\", \
        \"api_activity_logs\".\"headers\", \"api_activity_logs\".\"body\", \
        \"api_activity_logs\".\"response_code\", \"api_activity_logs\".\"response_body\", \
        \"api_activity_logs\".\"ip_address\", \"api_activity_logs\".\"user_agent\", \
        \"api_activity_logs\".\"created_by_id\" FROM \"api_activity_logs\" \
        WHERE \"api_activity_logs\".\"created_at\" <= {cutoff} \
        ORDER BY \"api_activity_logs\".\"created_at\" DESC"
    )
}

/// `get_email_logs_queryset` (`cleanup_task.py:294-318`). Predicate is
/// `sent_at__lte`, NOT `created_at__lte`.
pub fn email_logs_sql(cutoff: &str) -> String {
    format!(
        "SELECT \"email_notification_logs\".\"id\", \"email_notification_logs\".\"created_at\", \
        \"email_notification_logs\".\"receiver_id\", \"email_notification_logs\".\"triggered_by_id\", \
        \"email_notification_logs\".\"entity_identifier\", \"email_notification_logs\".\"entity_name\", \
        \"email_notification_logs\".\"data\", \"email_notification_logs\".\"processed_at\", \
        \"email_notification_logs\".\"sent_at\", \"email_notification_logs\".\"entity\", \
        \"email_notification_logs\".\"old_value\", \"email_notification_logs\".\"new_value\", \
        \"email_notification_logs\".\"created_by_id\" FROM \"email_notification_logs\" \
        WHERE \"email_notification_logs\".\"sent_at\" <= {cutoff} \
        ORDER BY \"email_notification_logs\".\"created_at\" DESC"
    )
}

/// `get_page_versions_queryset` (`cleanup_task.py:321-354`): everything but
/// the newest 20 per page (`ROW_NUMBER() OVER (PARTITION BY page_id ORDER
/// BY created_at DESC)`, keep `row_num > 20`).
pub fn page_versions_sql() -> String {
    "SELECT \"page_versions\".\"id\", \"page_versions\".\"created_at\", \"page_versions\".\"page_id\", \
    \"page_versions\".\"workspace_id\", \"page_versions\".\"owned_by_id\", \
    \"page_versions\".\"description_html\", \"page_versions\".\"description_binary\", \
    \"page_versions\".\"description_stripped\", \"page_versions\".\"description_json\", \
    \"page_versions\".\"sub_pages_data\", \"page_versions\".\"created_by_id\", \
    \"page_versions\".\"updated_by_id\", \"page_versions\".\"deleted_at\", \
    \"page_versions\".\"last_saved_at\" FROM \"page_versions\" \
    WHERE \"page_versions\".\"id\" IN (SELECT \"col1\" FROM ( SELECT * FROM ( SELECT U0.\"id\" AS \"col1\", \
    ROW_NUMBER() OVER (PARTITION BY U0.\"page_id\" ORDER BY U0.\"created_at\" DESC) AS \"qual0\", \
    U0.\"created_at\" AS \"qual1\" FROM \"page_versions\" U0 ORDER BY U0.\"created_at\" DESC ) \
    \"qualify\" WHERE \"qual0\" > 20 ) \"qualify_mask\" ORDER BY \"qual1\" DESC) \
    ORDER BY \"page_versions\".\"created_at\" DESC"
        .to_owned()
}

/// `get_issue_description_versions_queryset` (`cleanup_task.py:357-390`):
/// same newest-20 window partitioned by `issue_id`.
pub fn issue_description_versions_sql() -> String {
    "SELECT \"issue_description_versions\".\"id\", \"issue_description_versions\".\"created_at\", \
    \"issue_description_versions\".\"issue_id\", \"issue_description_versions\".\"workspace_id\", \
    \"issue_description_versions\".\"project_id\", \"issue_description_versions\".\"created_by_id\", \
    \"issue_description_versions\".\"updated_by_id\", \"issue_description_versions\".\"owned_by_id\", \
    \"issue_description_versions\".\"last_saved_at\", \"issue_description_versions\".\"description_binary\", \
    \"issue_description_versions\".\"description_html\", \
    \"issue_description_versions\".\"description_stripped\", \
    \"issue_description_versions\".\"description_json\", \
    \"issue_description_versions\".\"deleted_at\" FROM \"issue_description_versions\" \
    WHERE \"issue_description_versions\".\"id\" IN (SELECT \"col1\" FROM ( SELECT * FROM ( \
    SELECT U0.\"id\" AS \"col1\", ROW_NUMBER() OVER (PARTITION BY U0.\"issue_id\" ORDER BY \
    U0.\"created_at\" DESC) AS \"qual0\" FROM \"issue_description_versions\" U0 ) \"qualify\" \
    WHERE \"qual0\" > 20 ) \"qualify_mask\")"
        .to_owned()
}

/// `get_webhook_logs_queryset` (`cleanup_task.py:393-419`): explicit
/// `.order_by("created_at")` ASC.
pub fn webhook_logs_sql(cutoff: &str) -> String {
    format!(
        "SELECT \"webhook_logs\".\"id\", \"webhook_logs\".\"created_at\", \
        \"webhook_logs\".\"workspace_id\", \"webhook_logs\".\"webhook\", \
        \"webhook_logs\".\"event_type\", \"webhook_logs\".\"request_method\", \
        \"webhook_logs\".\"request_headers\", \"webhook_logs\".\"request_body\", \
        \"webhook_logs\".\"response_status\", \"webhook_logs\".\"response_body\", \
        \"webhook_logs\".\"response_headers\", \"webhook_logs\".\"retry_count\" \
        FROM \"webhook_logs\" WHERE \"webhook_logs\".\"created_at\" <= {cutoff} \
        ORDER BY \"webhook_logs\".\"created_at\" ASC"
    )
}

/// `model.all_objects.filter(id__in=…).delete()`: `all_objects` is a plain
/// manager, so this is a real `DELETE` (no `deleted_at` stamp).
pub fn delete_by_ids_sql(table: &str) -> String {
    format!("DELETE FROM \"{table}\" WHERE \"id\" = ANY($1)")
}

// ---------------------------------------------------------------------------
// Row decoding: Django column types to DTO inputs.
// ---------------------------------------------------------------------------

fn opt_text(row: &PgRow, col: &str) -> Result<Option<String>, sqlx::Error> {
    row.try_get::<Option<String>, _>(col)
}

/// Text columns feeding `str()`-wrapped outputs: production values are
/// text; the fixture goldens replay them as opaque text.
fn opt_key(row: &PgRow, col: &str) -> Result<Option<ScalarKey>, sqlx::Error> {
    Ok(opt_text(row, col)?.map(ScalarKey::Text))
}

/// Passthrough columns (no `str()`): text stays text.
fn text_as_value(row: &PgRow, col: &str) -> Result<Option<Value>, sqlx::Error> {
    Ok(opt_text(row, col)?.map(Value::String))
}

fn uuid_key(row: &PgRow, col: &str) -> Result<Option<ScalarKey>, sqlx::Error> {
    Ok(row.try_get::<Option<Uuid>, _>(col)?.map(ScalarKey::Uuid))
}

fn opt_dt(row: &PgRow, col: &str) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    row.try_get::<Option<DateTime<Utc>>, _>(col)
}

fn opt_json(row: &PgRow, col: &str) -> Result<Option<Value>, sqlx::Error> {
    row.try_get::<Option<Value>, _>(col)
}

fn opt_blob(row: &PgRow, col: &str) -> Result<Option<BlobValue>, sqlx::Error> {
    Ok(row
        .try_get::<Option<Vec<u8>>, _>(col)?
        .map(BlobValue::Bytes))
}

fn decode_api_log(row: &PgRow) -> Result<(ApiLogRow, Uuid), sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    Ok((
        ApiLogRow {
            id: ScalarKey::Uuid(id),
            created_at: opt_dt(row, "created_at")?,
            token_identifier: row.try_get("token_identifier")?,
            path: row.try_get("path")?,
            method: row.try_get("method")?,
            query_params: text_as_value(row, "query_params")?,
            headers: text_as_value(row, "headers")?,
            body: text_as_value(row, "body")?,
            response_code: row.try_get("response_code")?,
            response_body: text_as_value(row, "response_body")?,
            ip_address: opt_text(row, "ip_address")?,
            user_agent: opt_text(row, "user_agent")?,
            created_by_id: uuid_key(row, "created_by_id")?,
        },
        id,
    ))
}

fn decode_email_log(row: &PgRow) -> Result<(EmailLogRow, Uuid), sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    Ok((
        EmailLogRow {
            id: ScalarKey::Uuid(id),
            created_at: opt_dt(row, "created_at")?,
            receiver_id: uuid_key(row, "receiver_id")?,
            triggered_by_id: uuid_key(row, "triggered_by_id")?,
            entity_identifier: uuid_key(row, "entity_identifier")?,
            entity_name: row.try_get("entity_name")?,
            data: opt_json(row, "data")?,
            processed_at: opt_dt(row, "processed_at")?,
            sent_at: opt_dt(row, "sent_at")?,
            entity: row.try_get("entity")?,
            old_value: opt_key(row, "old_value")?,
            new_value: opt_key(row, "new_value")?,
            created_by_id: uuid_key(row, "created_by_id")?,
        },
        id,
    ))
}

fn decode_page_version(row: &PgRow) -> Result<(PageVersionRow, Uuid), sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    Ok((
        PageVersionRow {
            id: ScalarKey::Uuid(id),
            created_at: opt_dt(row, "created_at")?,
            page_id: uuid_key(row, "page_id")?,
            workspace_id: uuid_key(row, "workspace_id")?,
            owned_by_id: uuid_key(row, "owned_by_id")?,
            description_html: row.try_get("description_html")?,
            description_binary: opt_blob(row, "description_binary")?,
            description_stripped: opt_text(row, "description_stripped")?,
            description_json: opt_json(row, "description_json")?,
            sub_pages_data: opt_json(row, "sub_pages_data")?,
            created_by_id: uuid_key(row, "created_by_id")?,
            updated_by_id: uuid_key(row, "updated_by_id")?,
            deleted_at: opt_dt(row, "deleted_at")?,
            last_saved_at: opt_dt(row, "last_saved_at")?,
        },
        id,
    ))
}

fn decode_issue_description_version(
    row: &PgRow,
) -> Result<(IssueDescriptionVersionRow, Uuid), sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    Ok((
        IssueDescriptionVersionRow {
            id: ScalarKey::Uuid(id),
            created_at: opt_dt(row, "created_at")?,
            issue_id: uuid_key(row, "issue_id")?,
            workspace_id: uuid_key(row, "workspace_id")?,
            project_id: uuid_key(row, "project_id")?,
            created_by_id: uuid_key(row, "created_by_id")?,
            updated_by_id: uuid_key(row, "updated_by_id")?,
            owned_by_id: uuid_key(row, "owned_by_id")?,
            last_saved_at: opt_dt(row, "last_saved_at")?,
            description_binary: opt_blob(row, "description_binary")?,
            description_html: row.try_get("description_html")?,
            description_stripped: opt_text(row, "description_stripped")?,
            description_json: opt_json(row, "description_json")?,
            deleted_at: opt_dt(row, "deleted_at")?,
        },
        id,
    ))
}

fn decode_webhook_log(row: &PgRow) -> Result<(WebhookLogRow, Uuid), sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    Ok((
        WebhookLogRow {
            id: ScalarKey::Uuid(id),
            created_at: opt_dt(row, "created_at")?,
            workspace_id: uuid_key(row, "workspace_id")?,
            webhook: uuid_key(row, "webhook")?,
            event_type: opt_key(row, "event_type")?,
            request_method: opt_key(row, "request_method")?,
            request_headers: opt_key(row, "request_headers")?,
            request_body: opt_key(row, "request_body")?,
            response_status: opt_key(row, "response_status")?,
            response_body: opt_key(row, "response_body")?,
            response_headers: opt_key(row, "response_headers")?,
            retry_count: row.try_get("retry_count")?,
        },
        id,
    ))
}

// ---------------------------------------------------------------------------
// BSON conversion: ordered JSON map to Mongo document.
// ---------------------------------------------------------------------------

/// Convert a transform output map to a BSON document, preserving key order.
pub fn json_to_bson(value: &Value) -> Bson {
    match value {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i32::try_from(i).is_ok() {
                    Bson::Int32(i as i32)
                } else {
                    Bson::Int64(i)
                }
            } else if let Some(u) = n.as_u64() {
                Bson::Int64(u as i64)
            } else {
                Bson::Double(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => Bson::String(s.clone()),
        Value::Array(items) => Bson::Array(items.iter().map(json_to_bson).collect()),
        Value::Object(map) => {
            let mut doc = Document::new();
            for (k, v) in map {
                doc.insert(k, json_to_bson(v));
            }
            Bson::Document(doc)
        }
    }
}

/// Restore binary payloads after `json_to_bson`: `Vec<u8>` has no JSON
/// form, so bytes travel out-of-band and are written as BSON Binary
/// (subtype 0x00, exactly what pymongo stores for `bytes`).
pub fn apply_binaries(doc: &mut Document, fields: &[(&str, Option<&BlobValue>)]) {
    for (name, blob) in fields {
        if let Some(BlobValue::Bytes(bytes)) = blob {
            doc.insert(
                *name,
                Bson::Binary(Binary {
                    subtype: BinarySubtype::Generic,
                    bytes: bytes.clone(),
                }),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mongo archival: `get_mongo_collection` + the `bulk_write` half of
// `flush_to_mongo_and_delete`.
// ---------------------------------------------------------------------------

/// Distinguishes the two Python outcomes: `BulkWriteError` skips the
/// Postgres delete but continues the loop; any other failure propagates
/// and fails the task (which the worker retries).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveError {
    /// `BulkWriteError`: log, skip this batch's delete, continue.
    BulkWrite(String),
    /// Anything else (connection, timeout, …): propagate.
    Transport(String),
}

/// Lazily-connected Mongo handle. Creation performs no I/O, mirroring
/// `MongoClient(url)` + `get_collection`, which likewise only fail on
/// misconfiguration — mapped to `None` exactly like Python's broad
/// `except …: return None` (`cleanup_task.py:48-51`).
#[derive(Clone, Debug)]
pub struct MongoSink {
    client: mongodb::Client,
    database: String,
}

impl MongoSink {
    /// Resolve from `MONGO_DB_URL` / `MONGO_DB_DATABASE` (the Django
    /// settings of the same names). `None` when either is missing, empty,
    /// or unparsable: "MongoDB not configured". Construction performs no
    /// server I/O, mirroring `MongoClient(url)`.
    pub async fn from_env() -> Option<Self> {
        let url = std::env::var("MONGO_DB_URL").ok()?;
        let database = std::env::var("MONGO_DB_DATABASE").ok()?;
        if url.trim().is_empty() || database.trim().is_empty() {
            tracing::info!("MongoDB not configured");
            return None;
        }
        match mongodb::Client::with_uri_str(&url).await {
            Ok(client) => {
                tracing::info!("MongoDB configured");
                Some(Self { client, database })
            }
            Err(error) => {
                tracing::error!(%error, "Failed to get MongoDB collection");
                None
            }
        }
    }

    /// Ordered multi-insert, mirroring
    /// `bulk_write([InsertOne(doc) for doc in buffer])` (ordered by
    /// default on both sides: the first error stops the batch).
    pub async fn archive(&self, collection: &str, docs: Vec<Document>) -> Result<(), ArchiveError> {
        debug_assert!(!docs.is_empty(), "empty buffer never reaches mongo");
        let coll = self
            .client
            .database(&self.database)
            .collection::<Document>(collection);
        coll.insert_many(docs)
            .ordered(true)
            .await
            .map(|_| ())
            .map_err(|err| match *err.kind {
                mongodb::error::ErrorKind::BulkWrite(_) => ArchiveError::BulkWrite(err.to_string()),
                _ => ArchiveError::Transport(err.to_string()),
            })
    }
}

// ---------------------------------------------------------------------------
// Flush: `flush_to_mongo_and_delete` (`cleanup_task.py:54-89`).
// ---------------------------------------------------------------------------

/// What one flush did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlushOutcome {
    /// Mongo archival (when available) succeeded and the Postgres rows
    /// were deleted; carries the deleted count.
    Deleted { count: u64 },
    /// Mongo archival failed: nothing deleted, loop continues.
    ArchiveFailedSkipped,
}

/// Mongo side of a flush, injectable for tests.
pub trait BatchArchiver {
    fn archive(
        &self,
        collection: &str,
        docs: Vec<Document>,
    ) -> impl Future<Output = Result<(), ArchiveError>> + Send;
}

/// Postgres side of a flush, injectable for tests.
pub trait BatchDeleter {
    fn delete(
        &self,
        table: &str,
        ids: &[Uuid],
    ) -> impl Future<Output = Result<u64, sqlx::Error>> + Send;
}

/// Port of `flush_to_mongo_and_delete`: no-op on an empty buffer; Mongo
/// `bulk_write` first (only when a collection is available AND the run
/// sampled `mongo_available`); `BulkWriteError` logs and returns WITHOUT
/// deleting; otherwise `DELETE … WHERE id = ANY($1)`.
///
/// `mongo_collection` (`Option`) and `mongo_available` (`bool`) are kept as
/// separate parameters exactly as Python threads both through.
pub async fn flush_batch<A: BatchArchiver + Sync, D: BatchDeleter + Sync>(
    archiver: Option<&A>,
    mongo_available: bool,
    collection: &str,
    table: &str,
    docs: Vec<Document>,
    ids: Vec<Uuid>,
    deleter: &D,
) -> Result<FlushOutcome, CleanupError> {
    if docs.is_empty() {
        return Ok(FlushOutcome::Deleted { count: 0 });
    }
    let mut archival_failed = false;
    if let Some(archiver) = archiver {
        if mongo_available {
            match archiver.archive(collection, docs).await {
                Ok(()) => {}
                Err(ArchiveError::BulkWrite(detail)) => {
                    tracing::error!(collection, detail, "MongoDB bulk write error");
                    archival_failed = true;
                }
                Err(ArchiveError::Transport(detail)) => {
                    return Err(CleanupError::MongoTransport(detail));
                }
            }
        }
    }
    if archival_failed {
        tracing::error!(
            collection,
            count = ids.len(),
            "MongoDB archival failed; skipping Postgres delete"
        );
        return Ok(FlushOutcome::ArchiveFailedSkipped);
    }
    let deleted = deleter.delete(table, &ids).await?;
    tracing::info!(collection, deleted, "batch flush completed");
    Ok(FlushOutcome::Deleted { count: deleted })
}

/// Production [`BatchDeleter`]: hard `DELETE` over the primary pool.
pub struct PgDeleter<'a> {
    pool: &'a PgPool,
}

impl<'a> PgDeleter<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

impl BatchDeleter for PgDeleter<'_> {
    async fn delete(&self, table: &str, ids: &[Uuid]) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(&delete_by_ids_sql(table))
            .bind(ids)
            .execute(self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

impl BatchArchiver for MongoSink {
    async fn archive(&self, collection: &str, docs: Vec<Document>) -> Result<(), ArchiveError> {
        self.archive(collection, docs).await
    }
}

// ---------------------------------------------------------------------------
// Batching: `process_cleanup_task` accumulation (`cleanup_task.py:118-153`).
// ---------------------------------------------------------------------------

/// Accumulates `(document, id)` pairs and yields a full batch every
/// `BATCH_SIZE` items, plus the final partial batch. Generic over the item
/// so the boundary semantics are unit-testable without a database.
pub struct BatchAccumulator<T> {
    batch_size: usize,
    pending: Vec<T>,
}

impl<T> BatchAccumulator<T> {
    pub fn new(batch_size: usize) -> Self {
        Self {
            batch_size,
            pending: Vec::new(),
        }
    }

    /// Push one item; returns the full batch when the threshold is hit.
    pub fn push(&mut self, item: T) -> Option<Vec<T>> {
        self.pending.push(item);
        if self.pending.len() >= self.batch_size {
            Some(std::mem::take(&mut self.pending))
        } else {
            None
        }
    }

    /// The final partial batch, if any (`if buffer:` — empty emits nothing).
    pub fn finish(&mut self) -> Option<Vec<T>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

// ---------------------------------------------------------------------------
// Task runners: `process_cleanup_task` per queryset (`cleanup_task.py:92+`).
// ---------------------------------------------------------------------------

/// Which of the five cleanup tasks is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupTask {
    ApiLogs,
    EmailLogs,
    PageVersions,
    IssueDescriptionVersions,
    WebhookLogs,
}

impl CleanupTask {
    pub fn table(&self) -> &'static str {
        match self {
            CleanupTask::ApiLogs => TABLE_API_LOGS,
            CleanupTask::EmailLogs => TABLE_EMAIL_LOGS,
            CleanupTask::PageVersions => TABLE_PAGE_VERSIONS,
            CleanupTask::IssueDescriptionVersions => TABLE_ISSUE_DESCRIPTION_VERSIONS,
            CleanupTask::WebhookLogs => TABLE_WEBHOOK_LOGS,
        }
    }

    pub fn select_sql(&self, cutoff: &str) -> String {
        match self {
            CleanupTask::ApiLogs => api_logs_sql(cutoff),
            CleanupTask::EmailLogs => email_logs_sql(cutoff),
            CleanupTask::PageVersions => page_versions_sql(),
            CleanupTask::IssueDescriptionVersions => issue_description_versions_sql(),
            CleanupTask::WebhookLogs => webhook_logs_sql(cutoff),
        }
    }
}

/// Outcome of one task run, mirroring the `logger.info(…, extra={…})`
/// completion record (`cleanup_task.py:155-163`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskOutcome {
    pub total_processed: u64,
    pub total_batches: u64,
    pub mongo_available: bool,
}

/// Decode one streamed row into its archive document plus its id.
fn decode_row(task: CleanupTask, row: &PgRow) -> Result<(Document, Uuid), sqlx::Error> {
    fn doc_of<T: serde::Serialize>(doc: &T) -> Document {
        json_to_bson(&Value::Object(cleanup_dto::doc_map(doc)))
            .as_document()
            .expect("docs are objects")
            .clone()
    }
    match task {
        CleanupTask::ApiLogs => {
            let (record, id) = decode_api_log(row)?;
            Ok((doc_of(&cleanup_dto::transform_api_log(&record)), id))
        }
        CleanupTask::EmailLogs => {
            let (record, id) = decode_email_log(row)?;
            Ok((doc_of(&cleanup_dto::transform_email_log(&record)), id))
        }
        CleanupTask::PageVersions => {
            let (record, id) = decode_page_version(row)?;
            let mut doc = doc_of(&cleanup_dto::transform_page_version(&record));
            apply_binaries(
                &mut doc,
                &[("description_binary", record.description_binary.as_ref())],
            );
            Ok((doc, id))
        }
        CleanupTask::IssueDescriptionVersions => {
            let (record, id) = decode_issue_description_version(row)?;
            let mut doc = doc_of(&cleanup_dto::transform_issue_description_version(&record));
            apply_binaries(
                &mut doc,
                &[("description_binary", record.description_binary.as_ref())],
            );
            Ok((doc, id))
        }
        CleanupTask::WebhookLogs => {
            let (record, id) = decode_webhook_log(row)?;
            Ok((doc_of(&cleanup_dto::transform_webhook_log(&record)), id))
        }
    }
}

/// Port of `process_cleanup_task`: sample `mongo_available` once, stream
/// the queryset, transform each record, flush every [`BATCH_SIZE`], flush
/// the final partial batch, and report totals.
///
/// `total_processed` counts flushed records even when a batch's delete was
/// skipped after archival failure, exactly like the Python counter.
pub async fn run_cleanup(
    pool: &PgPool,
    mongo: Option<&MongoSink>,
    task: CleanupTask,
    cutoff: &str,
    task_name: &str,
    collection: &str,
) -> Result<TaskOutcome, CleanupError> {
    use futures_util::TryStreamExt;

    tracing::info!(task_name, "starting cleanup task");
    let mongo_available = mongo.is_some();
    if mongo_available {
        tracing::info!("MongoDB collection '{collection}' connected successfully");
    }
    let sql = task.select_sql(cutoff);
    let table = task.table();
    let deleter = PgDeleter::new(pool);

    let mut stream = sqlx::query(&sql).fetch(pool);
    let mut batches = BatchAccumulator::new(BATCH_SIZE);
    let mut total_processed: u64 = 0;
    let mut total_batches: u64 = 0;

    while let Some(row) = stream.try_next().await? {
        let (doc, id) = decode_row(task, &row)?;
        if let Some(items) = batches.push((doc, id)) {
            total_batches += 1;
            let (docs, ids): (Vec<Document>, Vec<Uuid>) = items.into_iter().unzip();
            total_processed += ids.len() as u64;
            flush_batch(
                mongo,
                mongo_available,
                collection,
                table,
                docs,
                ids,
                &deleter,
            )
            .await?;
        }
    }
    if let Some(items) = batches.finish() {
        total_batches += 1;
        let (docs, ids): (Vec<Document>, Vec<Uuid>) = items.into_iter().unzip();
        total_processed += ids.len() as u64;
        flush_batch(
            mongo,
            mongo_available,
            collection,
            table,
            docs,
            ids,
            &deleter,
        )
        .await?;
    }

    tracing::info!(
        task_name,
        total_processed,
        total_batches,
        mongo_available,
        collection,
        "cleanup task completed"
    );
    Ok(TaskOutcome {
        total_processed,
        total_batches,
        mongo_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Same committed evidence as the DTO tests:
    /// `rust-api/fixtures/tasks_cleanup/cleanup.json`.
    static FIXTURE: &str = include_str!("../../../../fixtures/tasks_cleanup/cleanup.json");

    fn fixture_querysets() -> serde_json::Map<String, Value> {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        match fixture["querysets"].clone() {
            Value::Object(map) => map,
            _ => panic!("fixture has querysets"),
        }
    }

    /// Frozen cutoff from the fixture (`cutoff_frozen`).
    const FROZEN_CUTOFF: &str = "2026-08-29 06:00:00+00:00";

    #[test]
    fn api_logs_sql_matches_django() {
        let q = fixture_querysets();
        assert_eq!(
            api_logs_sql(FROZEN_CUTOFF),
            q["api_logs_sql"].as_str().unwrap()
        );
    }

    #[test]
    fn email_logs_sql_matches_django() {
        let q = fixture_querysets();
        assert_eq!(
            email_logs_sql(FROZEN_CUTOFF),
            q["email_logs_sql"].as_str().unwrap()
        );
    }

    #[test]
    fn page_versions_sql_matches_django() {
        let q = fixture_querysets();
        assert_eq!(
            page_versions_sql(),
            q["page_versions_sql"].as_str().unwrap()
        );
    }

    #[test]
    fn issue_description_versions_sql_matches_django() {
        let q = fixture_querysets();
        assert_eq!(
            issue_description_versions_sql(),
            q["issue_description_versions_sql"].as_str().unwrap()
        );
    }

    #[test]
    fn webhook_logs_sql_matches_django() {
        let q = fixture_querysets();
        assert_eq!(
            webhook_logs_sql(FROZEN_CUTOFF),
            q["webhook_logs_sql"].as_str().unwrap()
        );
    }

    #[test]
    fn cutoff_renders_like_django() {
        let cutoff = "2026-08-29T06:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(render_cutoff(&cutoff), FROZEN_CUTOFF);
    }

    #[test]
    fn delete_sql_targets_ids() {
        assert_eq!(
            delete_by_ids_sql("api_activity_logs"),
            "DELETE FROM \"api_activity_logs\" WHERE \"id\" = ANY($1)"
        );
    }

    // -- flush ordering with fake sides ------------------------------------

    struct FakeArchiver {
        calls: Arc<Mutex<Vec<(String, usize)>>>,
        fail: Option<ArchiveError>,
    }

    impl BatchArchiver for FakeArchiver {
        async fn archive(&self, collection: &str, docs: Vec<Document>) -> Result<(), ArchiveError> {
            self.calls
                .lock()
                .unwrap()
                .push((collection.to_owned(), docs.len()));
            match &self.fail {
                Some(ArchiveError::BulkWrite(d)) => Err(ArchiveError::BulkWrite(d.clone())),
                Some(ArchiveError::Transport(d)) => Err(ArchiveError::Transport(d.clone())),
                None => Ok(()),
            }
        }
    }

    struct FakeDeleter {
        calls: Arc<Mutex<Vec<(String, usize)>>>,
    }

    impl BatchDeleter for FakeDeleter {
        async fn delete(&self, table: &str, ids: &[Uuid]) -> Result<u64, sqlx::Error> {
            self.calls
                .lock()
                .unwrap()
                .push((table.to_owned(), ids.len()));
            Ok(ids.len() as u64)
        }
    }

    fn doc(n: u8) -> Document {
        let mut d = Document::new();
        d.insert("n", i32::from(n));
        d
    }

    fn fakes(fail: Option<ArchiveError>) -> (FakeArchiver, FakeDeleter) {
        (
            FakeArchiver {
                calls: Arc::new(Mutex::new(Vec::new())),
                fail,
            },
            FakeDeleter {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        )
    }

    #[tokio::test]
    async fn empty_buffer_is_noop() {
        let (archiver, deleter) = fakes(None);
        let outcome = flush_batch(
            Some(&archiver),
            true,
            "api_activity_logs",
            TABLE_API_LOGS,
            vec![],
            vec![],
            &deleter,
        )
        .await
        .unwrap();
        assert_eq!(outcome, FlushOutcome::Deleted { count: 0 });
        assert!(archiver.calls.lock().unwrap().is_empty());
        assert!(deleter.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn archive_then_delete_ordering() {
        let (archiver, deleter) = fakes(None);
        let ids = vec![Uuid::nil(), Uuid::max()];
        let outcome = flush_batch(
            Some(&archiver),
            true,
            "api_activity_logs",
            TABLE_API_LOGS,
            vec![doc(1), doc(2)],
            ids,
            &deleter,
        )
        .await
        .unwrap();
        assert_eq!(outcome, FlushOutcome::Deleted { count: 2 });
        assert_eq!(
            *archiver.calls.lock().unwrap(),
            vec![("api_activity_logs".to_owned(), 2)]
        );
        assert_eq!(
            *deleter.calls.lock().unwrap(),
            vec![(TABLE_API_LOGS.to_owned(), 2)]
        );
    }

    #[tokio::test]
    async fn bulk_write_failure_skips_delete_and_continues() {
        let (archiver, deleter) = fakes(Some(ArchiveError::BulkWrite("dup key".to_owned())));
        let outcome = flush_batch(
            Some(&archiver),
            true,
            "page_versions",
            TABLE_PAGE_VERSIONS,
            vec![doc(1)],
            vec![Uuid::nil()],
            &deleter,
        )
        .await
        .unwrap();
        assert_eq!(outcome, FlushOutcome::ArchiveFailedSkipped);
        assert_eq!(archiver.calls.lock().unwrap().len(), 1);
        assert!(deleter.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn transport_failure_propagates() {
        let (archiver, deleter) = fakes(Some(ArchiveError::Transport("down".to_owned())));
        let err = flush_batch(
            Some(&archiver),
            true,
            "page_versions",
            TABLE_PAGE_VERSIONS,
            vec![doc(1)],
            vec![Uuid::nil()],
            &deleter,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CleanupError::MongoTransport(_)));
        assert!(deleter.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unconfigured_mongo_deletes_without_archiving() {
        let (archiver, deleter) = fakes(None);
        // No collection (None) — and mongo_available false: PG-only delete.
        let outcome = flush_batch(
            None::<&FakeArchiver>,
            false,
            "webhook_logs",
            TABLE_WEBHOOK_LOGS,
            vec![doc(1)],
            vec![Uuid::nil()],
            &deleter,
        )
        .await
        .unwrap();
        assert_eq!(outcome, FlushOutcome::Deleted { count: 1 });
        assert!(archiver.calls.lock().unwrap().is_empty());
        assert_eq!(deleter.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn accumulator_batches_at_threshold() {
        let mut acc = BatchAccumulator::new(3);
        assert_eq!(acc.push(1), None);
        assert_eq!(acc.push(2), None);
        assert_eq!(acc.push(3), Some(vec![1, 2, 3]));
        assert_eq!(acc.push(4), None);
        assert_eq!(acc.finish(), Some(vec![4]));
        assert_eq!(acc.finish(), None);
    }

    #[test]
    fn accumulator_exact_batch_leaves_no_remainder() {
        let mut acc = BatchAccumulator::new(BATCH_SIZE);
        let mut full = 0;
        for i in 0..BATCH_SIZE {
            if acc.push(i).is_some() {
                full += 1;
            }
        }
        assert_eq!(full, 1);
        assert_eq!(acc.finish(), None);
    }

    #[test]
    fn json_to_bson_preserves_types_and_order() {
        let v: Value = serde_json::from_str(
            r#"{"id":"7","response_code":201,"big":5000000000,"ok":true,"nil":null,"obj":{"a":1}}"#,
        )
        .unwrap();
        let bson = json_to_bson(&v);
        let doc = bson.as_document().unwrap();
        let keys: Vec<&str> = doc.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["id", "response_code", "big", "ok", "nil", "obj"]);
        assert_eq!(doc.get_str("id").unwrap(), "7");
        assert_eq!(doc.get_i32("response_code").unwrap(), 201);
        assert_eq!(doc.get_i64("big").unwrap(), 5_000_000_000);
    }

    #[test]
    fn binaries_become_bson_binary() {
        let mut doc = Document::new();
        doc.insert("description_binary", Bson::Null);
        apply_binaries(
            &mut doc,
            &[(
                "description_binary",
                Some(&BlobValue::Bytes(vec![0x62, 0x69, 0x6e])),
            )],
        );
        match doc.get("description_binary").unwrap() {
            Bson::Binary(b) => {
                assert_eq!(b.subtype, BinarySubtype::Generic);
                assert_eq!(b.bytes, vec![0x62, 0x69, 0x6e]);
            }
            other => panic!("expected Binary, got {other:?}"),
        }
        // Text and absent binaries leave the JSON value untouched.
        let mut doc2 = Document::new();
        doc2.insert("description_binary", Bson::String("Ymlu".to_owned()));
        apply_binaries(
            &mut doc2,
            &[(
                "description_binary",
                Some(&BlobValue::Text("Ymlu".to_owned())),
            )],
        );
        assert_eq!(doc2.get_str("description_binary").unwrap(), "Ymlu");
    }
}
