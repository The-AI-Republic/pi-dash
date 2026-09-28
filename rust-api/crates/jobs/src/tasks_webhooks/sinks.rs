//! D-08 logging + event-tracking sinks (jobs layer).
//!
//! Port of the task entry points of
//! `apps/api/pi_dash/bgtasks/logger_task.py:22-100` (`process_logs` and
//! its two sinks) and
//! `apps/api/pi_dash/bgtasks/event_tracking_task.py:24-81`
//! (`posthogConfiguration`, the workspace role lookup half of
//! `preprocess_data_properties`, and `track_event`). The pure halves
//! (`safe_decode_body`, the role decision) live in `pidash-services`
//! (`tasks_webhooks::log_decode`); this module owns the Celery wire
//! surface — task names, `(args, kwargs)` extraction, the
//! mongo-else-postgres routing, the PostHog `/batch/` payload — and the
//! [`Registry`][crate::worker::Registry] wiring.
//!
//! Fixture oracle: `rust-api/fixtures/tasks_webhooks/fx-log-01-process-logs.json`
//! and `rust-api/fixtures/tasks_webhooks/fx-evt-01-track-event.json`.
//! Trace: `rust-api/fixtures/tasks_webhooks/TRACE.md`.
//!
//! Ownership note: [`register_sink_tasks`] only builds the handler table.
//! Flipping the live worker to these handlers (calling it from the
//! binary) is the domain gate's call (PIDASHCONV-203, after the
//! PIDASHCONV-21 proxy pass) — not this layer issue.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * `process_logs` routes on `MongoConnection.is_configured()` and
//!   passes TWO DIFFERENT payloads: the mongo-shaped `mongo_log` doc vs
//!   the postgres-shaped `log_data` row (`logger_task.py:97-100`). The
//!   unconfigured path never touches mongo. [`route_for_logs`] pins it.
//! * The mongo write is `insert_one` in Python; here it goes through the
//!   shared [`MongoSink::archive`][pidash_db::tasks_cleanup::cleanup_queries::MongoSink::archive]
//!   with a single-document batch (the foundation helper only exposes the
//!   batch path). Same row effect: the one document lands in
//!   `api_activity_logs`.
//! * `track_event` is the ONLY task here returning `False` on error —
//!   which Celery still acknowledges (a return value is not a failure).
//!   Both handlers therefore always settle [`Verdict::Ack`]; failures are
//!   traced (`log_exception` mirror) and never requeued. Neither task
//!   carries retry options (bare `@shared_task`; see the contract suite's
//!   `test_retry_options_parity`).
//! * The PostHog SDK (posthog==3.5.0) does its 3 background retries inside
//!   its consumer thread, invisibly to the task result; the port posts
//!   once per event and logs the failure. The task-level contract —
//!   always succeeds — is unchanged.
//! * Role `admin` covers ANY non-owner, including users with no workspace
//!   membership (no membership check in Python). Kept as-is.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use pidash_db::tasks_cleanup::cleanup_queries::{json_to_bson, MongoSink};
use pidash_db::Pools;
use pidash_services::tasks_webhooks::{is_role_event, preprocess_data_properties};

use crate::worker::{Handler, Registry, Verdict};

/// `pi_dash.bgtasks.logger_task.process_logs` (bare `@shared_task`).
pub const PROCESS_LOGS_TASK: &str = "pi_dash.bgtasks.logger_task.process_logs";
/// `pi_dash.bgtasks.event_tracking_task.track_event` (bare `@shared_task`).
pub const TRACK_EVENT_TASK: &str = "pi_dash.bgtasks.event_tracking_task.track_event";

/// Mongo collection for external API activity logs
/// (`logger_task.py:33`; note the sibling `webhook_logs` collection used
/// by `save_webhook_log` — different collection, same connection class).
pub const MONGO_COLLECTION: &str = "api_activity_logs";

/// PostHog `/batch/` path (`posthog/request.py:88`).
pub const POSTHOG_BATCH_PATH: &str = "/batch/";
/// `$lib` marker the SDK stamps on every message (`client.py:369`).
pub const POSTHOG_LIB: &str = "posthog-python";
/// `$lib_version` (`posthog/version.py:1`, posthog==3.5.0).
pub const POSTHOG_LIB_VERSION: &str = "3.5.0";
/// `User-Agent` header (`posthog/request.py:19`).
pub const POSTHOG_USER_AGENT: &str = "posthog-python/3.5.0";
/// Request timeout seconds (`Posthog.__init__`, `timeout=15`).
pub const POSTHOG_TIMEOUT_SECS: u64 = 15;
/// Default host (`US_INGESTION_ENDPOINT`, `posthog/request.py:14`).
pub const POSTHOG_DEFAULT_HOST: &str = "https://us-api.i.posthog.com";

/// `POSTHOG_API_KEY` / `POSTHOG_HOST` env names
/// (`event_tracking_task.py:27-34`).
pub const POSTHOG_API_KEY_VAR: &str = "POSTHOG_API_KEY";
pub const POSTHOG_HOST_VAR: &str = "POSTHOG_HOST";

/// Warning when either PostHog value is missing
/// (`event_tracking_task.py:64`).
pub const NOT_CONFIGURED_WARNING: &str = "Event tracking is not configured";

/// Every failure these sinks report. Handler-level: every variant is
/// traced and acknowledged — Python swallows them into `log_exception`
/// plus `True`/`False`/`None` returns, all of which Celery acks.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("bad {0} payload: {1}")]
    BadPayload(&'static str, String),
    #[error("postgres sink failed: {0}")]
    Postgres(String),
    #[error("mongo sink failed: {0}")]
    Mongo(String),
    #[error("posthog sink failed: {0}")]
    Posthog(String),
    #[error("workspace lookup failed: {0}")]
    Workspace(String),
}

// ---------------------------------------------------------------------------
// process_logs routing
// ---------------------------------------------------------------------------

/// Which sink `process_logs` writes to (`logger_task.py:97-100`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSink {
    Mongo,
    Postgres,
}

/// `if MongoConnection.is_configured(): log_to_mongo(mongo_log) else:
/// log_to_postgres(log_data)`.
pub fn route_for_logs(mongo_configured: bool) -> LogSink {
    if mongo_configured {
        LogSink::Mongo
    } else {
        LogSink::Postgres
    }
}

// ---------------------------------------------------------------------------
// PostHog configuration
// ---------------------------------------------------------------------------

/// `posthogConfiguration` (`event_tracking_task.py:24-41`): both values
/// truthy → the pair, else `(None, None)`. Python truthiness on strings:
/// only the empty string (and a missing value) fails; whitespace-only
/// counts as configured — kept as-is.
///
/// Resolution order (`get_configuration_value` with per-key defaults of
/// `os.environ.get(...)`): the committed config registry
/// (`pidash-db config::registry`) classifies BOTH keys as `Env`-sourced,
/// so the instance-value (DB) tier never applies and this is an env read.
/// A future reclassification to `Db` would flow through the config
/// accessor; the pair rule below is unchanged either way.
pub fn resolve_posthog_config(
    api_key: Option<String>,
    host: Option<String>,
) -> Option<(String, String)> {
    match (api_key, host) {
        (Some(key), Some(host)) if !key.is_empty() && !host.is_empty() => Some((key, host)),
        _ => None,
    }
}

/// Read the pair from the process environment.
pub fn posthog_configuration_from_env() -> Option<(String, String)> {
    resolve_posthog_config(
        std::env::var(POSTHOG_API_KEY_VAR).ok(),
        std::env::var(POSTHOG_HOST_VAR).ok(),
    )
}

// ---------------------------------------------------------------------------
// PostHog wire
// ---------------------------------------------------------------------------

/// `determine_server_host` (`posthog/request.py:24-36`): the two legacy
/// cloud hosts remap to their ingestion endpoints; anything else passes
/// through unchanged (trailing slash is NOT stripped here — `post()`
/// strips it when joining `path`).
pub fn determine_server_host(host: &str) -> String {
    match host {
        "https://app.posthog.com" | "https://us.posthog.com" => {
            "https://us-api.i.posthog.com".to_owned()
        }
        "https://eu.posthog.com" => "https://eu-api.i.posthog.com".to_owned(),
        other => other.to_owned(),
    }
}

/// `remove_trailing_slash(host) + "/batch/"` (`posthog/request.py:88`).
pub fn capture_url(host: &str) -> String {
    format!(
        "{}{}",
        determine_server_host(host).trim_end_matches('/'),
        POSTHOG_BATCH_PATH
    )
}

/// Render `datetime.utcnow().replace(tzinfo=tzutc()).isoformat()`:
/// `+00:00` offset, `.%f` fraction only when microseconds are nonzero
/// (same conditional shape as the D-09 expiry cutoff).
pub fn iso_now(now: DateTime<Utc>) -> String {
    let micros = now.timestamp_subsec_micros();
    if micros == 0 {
        now.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{:06}+00:00", now.format("%Y-%m-%dT%H:%M:%S"), micros)
    }
}

/// One PostHog `/batch/` POST, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureRequest {
    pub url: String,
    pub user_agent: String,
    pub timeout_secs: u64,
    pub body: Value,
}

/// Build the exact `capture()` message (posthog==3.5.0, `client.py:173-`
/// `228` + `_enqueue`, `request.py:36-89`):
///
/// * `groups={"workspace": slug}` does NOT travel top-level — the SDK
///   writes it into `properties["$groups"]`, mutating the caller's dict
///   (`client.py:200-202`). `properties` here is that same (already
///   role-stamped) object with `$groups`, `$lib`, `$lib_version` and
///   `$geoip_disable` (`disable_geoip` defaults `True`) appended in that
///   order.
/// * `timestamp=None` becomes now-UTC isoformat; `uuid=None` is dropped;
///   `context={}`; `distinct_id=str(user_id)`.
/// * Envelope `{"batch": [msg], "sentAt": now, "api_key": key}`
///   (`request.py:36-53`); `User-Agent: posthog-python/3.5.0`.
pub fn build_capture_request(
    api_key: &str,
    host: &str,
    event_name: &str,
    distinct_id: &str,
    properties: &Map<String, Value>,
    slug: &str,
    now: DateTime<Utc>,
) -> CaptureRequest {
    let mut props = properties.clone();
    let mut groups = Map::new();
    groups.insert("workspace".to_owned(), Value::String(slug.to_owned()));
    props.insert("$groups".to_owned(), Value::Object(groups));
    props.insert("$lib".to_owned(), Value::String(POSTHOG_LIB.to_owned()));
    props.insert(
        "$lib_version".to_owned(),
        Value::String(POSTHOG_LIB_VERSION.to_owned()),
    );
    props.insert("$geoip_disable".to_owned(), Value::Bool(true));

    let mut msg = Map::new();
    msg.insert("properties".to_owned(), Value::Object(props));
    msg.insert("timestamp".to_owned(), Value::String(iso_now(now)));
    msg.insert("context".to_owned(), Value::Object(Map::new()));
    msg.insert(
        "distinct_id".to_owned(),
        Value::String(distinct_id.to_owned()),
    );
    msg.insert("event".to_owned(), Value::String(event_name.to_owned()));

    let mut body = Map::new();
    body.insert("batch".to_owned(), Value::Array(vec![Value::Object(msg)]));
    body.insert("sentAt".to_owned(), Value::String(iso_now(now)));
    body.insert("api_key".to_owned(), Value::String(api_key.to_owned()));

    CaptureRequest {
        url: capture_url(host),
        user_agent: POSTHOG_USER_AGENT.to_owned(),
        timeout_secs: POSTHOG_TIMEOUT_SECS,
        body: Value::Object(body),
    }
}

// ---------------------------------------------------------------------------
// Payload parsing
// ---------------------------------------------------------------------------

/// Python `str()` over the JSON-scalar shapes that can arrive on the
/// Celery wire (`str(user_id)`, `str(owner_id)`): strings pass through,
/// numbers render plainly, booleans use Python capitalisation. Complex
/// values cannot occur from the real call sites (the middleware and the
/// workspace views only pass scalars/objects at known keys); they fall
/// back to compact JSON rather than failing the task.
pub fn python_str(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

/// `process_logs(log_data, mongo_log)` (`logger_task.py:91-100`).
/// Wire: `process_logs.delay(log_data=log_data, mongo_log=mongo_log)`
/// (`middleware/logger.py:149`) — kwargs; positionals accepted in the
/// same order.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessLogsCall {
    pub log_data: Map<String, Value>,
    pub mongo_log: Map<String, Value>,
}

pub fn parse_process_logs_call(args: &Value, kwargs: &Value) -> Result<ProcessLogsCall, SinkError> {
    let positional: &[Value] = args.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let log_data = positional
        .first()
        .and_then(|value| value.as_object().cloned())
        .or_else(|| {
            kwargs
                .get("log_data")
                .and_then(|value| value.as_object().cloned())
        })
        .ok_or_else(|| {
            SinkError::BadPayload(PROCESS_LOGS_TASK, "missing object \"log_data\"".to_owned())
        })?;
    let mongo_log = positional
        .get(1)
        .and_then(|value| value.as_object().cloned())
        .or_else(|| {
            kwargs
                .get("mongo_log")
                .and_then(|value| value.as_object().cloned())
        })
        .ok_or_else(|| {
            SinkError::BadPayload(PROCESS_LOGS_TASK, "missing object \"mongo_log\"".to_owned())
        })?;
    Ok(ProcessLogsCall {
        log_data,
        mongo_log,
    })
}

/// `track_event(user_id, event_name, slug, event_properties)`
/// (`event_tracking_task.py:61-81`). Wire: keyword delay calls
/// (`app/views/workspace/invite.py:128-140`, …) — kwargs; positionals
/// accepted in signature order.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEventCall {
    pub user_id: String,
    pub event_name: String,
    pub slug: String,
    pub event_properties: Map<String, Value>,
}

pub fn parse_track_event_call(args: &Value, kwargs: &Value) -> Result<TrackEventCall, SinkError> {
    let positional: &[Value] = args.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let get = |index: usize, key: &str| positional.get(index).or_else(|| kwargs.get(key));
    let user_id = get(0, "user_id")
        .and_then(python_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| SinkError::BadPayload(TRACK_EVENT_TASK, "missing user_id".to_owned()))?;
    let event_name = get(1, "event_name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|text| !text.is_empty())
        .ok_or_else(|| SinkError::BadPayload(TRACK_EVENT_TASK, "missing event_name".to_owned()))?;
    let slug = get(2, "slug")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|text| !text.is_empty())
        .ok_or_else(|| SinkError::BadPayload(TRACK_EVENT_TASK, "missing slug".to_owned()))?;
    let event_properties = get(3, "event_properties")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| {
            SinkError::BadPayload(
                TRACK_EVENT_TASK,
                "missing object \"event_properties\"".to_owned(),
            )
        })?;
    Ok(TrackEventCall {
        user_id,
        event_name,
        slug,
        event_properties,
    })
}

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// `Workspace.objects.get(slug=slug)` (`event_tracking_task.py:50`):
/// the default manager is `SoftDeletionManager`, so live rows only.
/// A miss raises `DoesNotExist` (→ warning + role `unknown`); any other
/// failure propagates to `track_event`'s broad `except`.
pub const WORKSPACE_OWNER_SQL: &str =
    "SELECT owner_id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL";

/// `APIActivityLog.objects.create(**log_data)` (`logger_task.py:85`):
/// `id`/`created_at`/`updated_at` are Python-side defaults (uuid4/now —
/// supplied here, mirroring the queue.rs `new_v4().to_string()`
/// precedent); audit columns stay NULL (no current user inside a worker,
/// `BaseModel.save` with anonymous user); `deleted_at` NULL.
pub const API_ACTIVITY_LOG_INSERT: &str = "INSERT INTO api_activity_logs \
    (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
    token_identifier, path, method, query_params, headers, body, \
    response_code, response_body, ip_address, user_agent) \
    VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12::inet, $13)";

/// Nullable text column: missing/`null` → NULL. Real callers only pass
/// strings here (the middleware builds them); scalars stringify like
/// `str()`, complex values compact-JSON (see [`python_str`]).
fn opt_text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(|value| match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Number(_) | Value::Bool(_) => python_str(value),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    })
}

fn req_text(
    map: &Map<String, Value>,
    task: &'static str,
    key: &'static str,
) -> Result<String, SinkError> {
    map.get(key)
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|text| !text.is_empty())
        .ok_or_else(|| SinkError::BadPayload(task, format!("missing {key:?}")))
}

/// Look up the workspace owner's id (`workspace.owner_id`).
/// `Ok(None)` is the `DoesNotExist` path; `Err` propagates to the
/// task-level `except`. The caller compares `str(owner_id)` against
/// `str(user_id)`, exactly like Python.
pub async fn lookup_workspace_owner(
    pool: &sqlx::PgPool,
    slug: &str,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(WORKSPACE_OWNER_SQL)
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(owner_id,)| owner_id))
}

/// One `APIActivityLog.objects.create(**log_data)` row
/// (`logger_task.py:85`): the 10 payload columns. `id`/`created_at`/
/// `updated_at` are stamped at INSERT (Python-side defaults); audit
/// columns stay NULL (no current user inside a worker).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityLogRow {
    pub token_identifier: String,
    pub path: String,
    pub method: String,
    pub query_params: Option<String>,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub response_code: i32,
    pub response_body: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Validate the `log_data` dict into a row. A missing required key (or a
/// non-integer `response_code`) mirrors the ORM raising into
/// `log_to_postgres`'s `except` → `log_exception(e)` + `False`.
pub fn parse_activity_log_row(log_data: &Map<String, Value>) -> Result<ActivityLogRow, SinkError> {
    const TASK: &str = PROCESS_LOGS_TASK;
    Ok(ActivityLogRow {
        token_identifier: req_text(log_data, TASK, "token_identifier")?,
        path: req_text(log_data, TASK, "path")?,
        method: req_text(log_data, TASK, "method")?,
        query_params: opt_text(log_data, "query_params"),
        headers: opt_text(log_data, "headers"),
        body: opt_text(log_data, "body"),
        response_code: log_data
            .get("response_code")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok())
            .ok_or_else(|| SinkError::BadPayload(TASK, "missing \"response_code\"".to_owned()))?,
        response_body: opt_text(log_data, "response_body"),
        ip_address: opt_text(log_data, "ip_address"),
        user_agent: opt_text(log_data, "user_agent"),
    })
}

/// `log_to_postgres` (`logger_task.py:79-89`).
pub async fn insert_api_activity_log(
    pool: &sqlx::PgPool,
    log_data: &Map<String, Value>,
) -> Result<(), SinkError> {
    let row = parse_activity_log_row(log_data)?;
    let now = Utc::now();
    sqlx::query(API_ACTIVITY_LOG_INSERT)
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(now)
        .bind(row.token_identifier)
        .bind(row.path)
        .bind(row.method)
        .bind(row.query_params)
        .bind(row.headers)
        .bind(row.body)
        .bind(row.response_code)
        .bind(row.response_body)
        .bind(row.ip_address)
        .bind(row.user_agent)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|error| SinkError::Postgres(error.to_string()))
}

/// `log_to_mongo` document (`logger_task.py:62-77`): the `mongo_log`
/// dict verbatim — it already carries `created_at`/`updated_at`/
/// `created_by`/`updated_by` from the middleware (`logger.py:141-147`).
/// Conversion goes through the shared [`json_to_bson`] helper, whose
/// int32-if-it-fits rule matches what pymongo stores for Python ints;
/// ISO timestamp strings stay strings (kombu already serialised the
/// datetimes on the wire).
pub fn mongo_log_to_document(mongo_log: &Map<String, Value>) -> Result<bson::Document, SinkError> {
    match json_to_bson(&Value::Object(mongo_log.clone())) {
        bson::Bson::Document(doc) => Ok(doc),
        _ => Err(SinkError::BadPayload(
            PROCESS_LOGS_TASK,
            "mongo_log is not a document".to_owned(),
        )),
    }
}

/// POST one capture request (timeout [`POSTHOG_TIMEOUT_SECS`]).
pub async fn post_capture(
    client: &reqwest::Client,
    request: &CaptureRequest,
) -> Result<(), SinkError> {
    let body = serde_json::to_string(&request.body)
        .map_err(|error| SinkError::Posthog(error.to_string()))?;
    let response = client
        .post(&request.url)
        .header("Content-Type", "application/json")
        .header("User-Agent", request.user_agent.clone())
        .timeout(std::time::Duration::from_secs(request.timeout_secs))
        .body(body)
        .send()
        .await
        .map_err(|error| SinkError::Posthog(error.to_string()))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(SinkError::Posthog(format!(
            "posthog /batch/ -> {}",
            response.status()
        )))
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handler for [`PROCESS_LOGS_TASK`]: route on the mongo handle —
/// `Some` mirrors `is_configured() == True` — and write the matching
/// payload. Every outcome acknowledges: Python returns `None` on both
/// paths and each sink swallows its own errors into `False`.
pub fn process_logs_handler(pools: Pools, mongo: Option<MongoSink>) -> Handler {
    std::sync::Arc::new(move |job: crate::queue::JobRow| {
        let pools = pools.clone();
        let mongo = mongo.clone();
        Box::pin(async move {
            let outcome = match parse_process_logs_call(&job.args, &job.kwargs) {
                Err(error) => {
                    tracing::error!(task = PROCESS_LOGS_TASK, %error, "bad process_logs payload");
                    return Ok(Verdict::Ack);
                }
                Ok(call) => run_process_logs(&pools, mongo.as_ref(), &call).await,
            };
            if let Err(error) = outcome {
                tracing::error!(task = PROCESS_LOGS_TASK, %error, "process_logs sink failed");
            }
            Ok(Verdict::Ack)
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

async fn run_process_logs(
    pools: &Pools,
    mongo: Option<&MongoSink>,
    call: &ProcessLogsCall,
) -> Result<(), SinkError> {
    match (route_for_logs(mongo.is_some()), mongo) {
        (LogSink::Mongo, Some(sink)) => {
            let document = mongo_log_to_document(&call.mongo_log)?;
            sink.archive(MONGO_COLLECTION, vec![document])
                .await
                // `ArchiveError` carries no `Display` (foundation type —
                // not modified); its `Debug` form is the error text.
                .map_err(|error| SinkError::Mongo(format!("{error:?}")))
        }
        // Unreachable: the route derives from the same handle. Kept as
        // an explicit arm (never `expect`) so a future refactor fails
        // into the postgres path's error log, not a panic. Python's
        // `log_to_mongo` maps this race to error log + `False`.
        (LogSink::Mongo, None) => {
            tracing::error!("MongoDB not configured");
            Err(SinkError::Mongo("MongoDB not configured".to_owned()))
        }
        (LogSink::Postgres, _) => insert_api_activity_log(pools.primary(), &call.log_data).await,
    }
}

/// Handler for [`TRACK_EVENT_TASK`]: config gate, role lookup, single
/// `/batch/` POST. Always acknowledges (see module docs).
pub fn track_event_handler(pools: Pools, http: reqwest::Client) -> Handler {
    std::sync::Arc::new(move |job: crate::queue::JobRow| {
        let pools = pools.clone();
        let http = http.clone();
        Box::pin(async move {
            let outcome = match parse_track_event_call(&job.args, &job.kwargs) {
                Err(error) => {
                    tracing::error!(task = TRACK_EVENT_TASK, %error, "bad track_event payload");
                    return Ok(Verdict::Ack);
                }
                Ok(call) => run_track_event(&pools, &http, &call).await,
            };
            if let Err(error) = outcome {
                tracing::error!(task = TRACK_EVENT_TASK, %error, "track_event failed");
            }
            Ok(Verdict::Ack)
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

async fn run_track_event(
    pools: &Pools,
    http: &reqwest::Client,
    call: &TrackEventCall,
) -> Result<(), SinkError> {
    // `posthogConfiguration()` + the unconfigured early return
    // (`event_tracking_task.py:62-65`): BEFORE any workspace lookup.
    let Some((api_key, host)) = posthog_configuration_from_env() else {
        tracing::warn!(NOT_CONFIGURED_WARNING);
        return Ok(());
    };
    // `preprocess_data_properties` for the pair events only; other
    // events skip the workspace lookup entirely (`:46` guard).
    let mut properties = call.event_properties.clone();
    if is_role_event(&call.event_name) {
        let owner_match = match lookup_workspace_owner(pools.primary(), &call.slug).await {
            Ok(None) => {
                tracing::warn!(
                    "Workspace {} does not exist while sending event {} for user {}",
                    call.slug,
                    call.event_name,
                    call.user_id
                );
                None
            }
            Ok(Some(owner_id)) => Some(owner_id.to_string() == call.user_id),
            Err(error) => return Err(SinkError::Workspace(error.to_string())),
        };
        preprocess_data_properties(&call.event_name, owner_match, &mut properties);
    }
    let request = build_capture_request(
        &api_key,
        &host,
        &call.event_name,
        &call.user_id,
        &properties,
        &call.slug,
        Utc::now(),
    );
    post_capture(http, &request).await
}

/// Register both D-08 sink tasks. The binary wiring (calling this from
/// `main.rs`) is the domain gate's call — see module docs.
pub fn register_sink_tasks(
    registry: &mut Registry,
    pools: Pools,
    mongo: Option<MongoSink>,
    http: reqwest::Client,
) {
    registry.register(
        PROCESS_LOGS_TASK,
        process_logs_handler(pools.clone(), mongo),
    );
    registry.register(TRACK_EVENT_TASK, track_event_handler(pools, http));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{route_for, Route};

    /// Same committed evidence as the services layer:
    /// `rust-api/fixtures/tasks_webhooks/fx-log-01-process-logs.json`.
    static LOG_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-log-01-process-logs.json");
    /// `rust-api/fixtures/tasks_webhooks/fx-evt-01-track-event.json`.
    static EVT_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-evt-01-track-event.json");

    fn log_fixture() -> Value {
        serde_json::from_str(LOG_FIXTURE).expect("log fixture parses")
    }

    fn evt_fixture() -> Value {
        serde_json::from_str(EVT_FIXTURE).expect("event fixture parses")
    }

    fn object(pairs: &[(&str, Value)]) -> Value {
        Value::Object(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn str_value(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn sample_log_data() -> Map<String, Value> {
        // `middleware/logger.py:123-134` shape.
        let mut map = Map::new();
        for key in [
            "token_identifier",
            "path",
            "method",
            "query_params",
            "headers",
            "body",
            "response_body",
            "ip_address",
            "user_agent",
        ] {
            map.insert(key.to_owned(), str_value("x"));
        }
        map.insert("path".to_owned(), str_value("/api/issues/"));
        map.insert("method".to_owned(), str_value("GET"));
        map.insert("response_code".to_owned(), Value::Number(200.into()));
        map
    }

    #[test]
    fn task_names_match_python_registration() {
        assert_eq!(
            PROCESS_LOGS_TASK,
            "pi_dash.bgtasks.logger_task.process_logs"
        );
        assert_eq!(
            TRACK_EVENT_TASK,
            "pi_dash.bgtasks.event_tracking_task.track_event"
        );
        assert_ne!(PROCESS_LOGS_TASK, TRACK_EVENT_TASK);
    }

    #[test]
    fn registered_names_route_local() {
        fn ack() -> Handler {
            std::sync::Arc::new(|_: crate::queue::JobRow| {
                Box::pin(async { Ok(Verdict::Ack) })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
            })
        }

        let mut registry = Registry::new();
        // Mirrors what register_sink_tasks does with the real handlers
        // (which need a live pool + HTTP client): both names route local.
        registry.register(PROCESS_LOGS_TASK, ack());
        registry.register(TRACK_EVENT_TASK, ack());
        assert_eq!(route_for(&registry, PROCESS_LOGS_TASK), Route::Local);
        assert_eq!(route_for(&registry, TRACK_EVENT_TASK), Route::Local);
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.logger_task.nope"),
            Route::PythonOwned
        );
    }

    #[test]
    fn routing_matches_fixture() {
        let fixture = log_fixture();
        let routing = fixture["process_logs"]["routing"]
            .as_str()
            .expect("routing text");
        assert!(routing.contains("log_to_mongo(mongo_log)"), "{routing}");
        assert!(routing.contains("log_to_postgres(log_data)"), "{routing}");
        assert_eq!(route_for_logs(true), LogSink::Mongo);
        assert_eq!(route_for_logs(false), LogSink::Postgres);
        // The two payloads differ (mongo-shaped doc vs postgres row).
        assert!(routing.contains("TWO DIFFERENT payloads"), "{routing}");
        assert_eq!(
            fixture["process_logs"]["signature"].as_str(),
            Some("process_logs(log_data, mongo_log)")
        );
        assert_eq!(
            fixture["process_logs"]["task_options"].as_str(),
            Some("shared_task, NOT bound")
        );
        assert!(fixture["process_logs"]["no_return"]
            .as_str()
            .unwrap()
            .contains("returns None"));
    }

    #[test]
    fn mongo_names_match_fixture() {
        let fixture = log_fixture();
        assert_eq!(MONGO_COLLECTION, "api_activity_logs");
        let mongo = &fixture["mongo"];
        assert!(mongo["collection"]
            .as_str()
            .unwrap()
            .contains("api_activity_logs"));
        assert!(mongo["collection"]
            .as_str()
            .unwrap()
            .contains("webhook_logs"));
        let get = &fixture["mongo"]["get_mongo_collection"];
        assert_eq!(get.as_array().unwrap().len(), 3);
    }

    #[test]
    fn parse_process_logs_kwargs_and_positional() {
        let log_data = sample_log_data();
        let mut mongo_log = log_data.clone();
        mongo_log.insert(
            "created_at".to_owned(),
            str_value("2026-09-28T00:00:00+00:00"),
        );
        // Real wire: kwargs (`middleware/logger.py:149`).
        let kwargs = object(&[
            ("log_data", Value::Object(log_data.clone())),
            ("mongo_log", Value::Object(mongo_log.clone())),
        ]);
        let call = parse_process_logs_call(&Value::Array(Vec::new()), &kwargs).expect("kwargs");
        assert_eq!(call.log_data, log_data);
        assert_eq!(call.mongo_log, mongo_log);
        // Positional fallback in signature order.
        let args = Value::Array(vec![
            Value::Object(log_data.clone()),
            Value::Object(mongo_log.clone()),
        ]);
        let call = parse_process_logs_call(&args, &Value::Object(Map::new())).expect("positional");
        assert_eq!(call.log_data, log_data);
        assert_eq!(call.mongo_log, mongo_log);
        // Missing either half fails the parse (task-level: logged + Ack).
        assert!(
            parse_process_logs_call(&Value::Array(Vec::new()), &kwargs_only_log(&log_data))
                .is_err()
        );
    }

    fn kwargs_only_log(log_data: &Map<String, Value>) -> Value {
        object(&[("log_data", Value::Object(log_data.clone()))])
    }

    #[test]
    fn parse_track_event_kwargs_and_positional() {
        let fixture = evt_fixture();
        let signature = fixture["track_event"]["signature"]
            .as_array()
            .expect("signature");
        let names: Vec<&str> = signature
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["user_id", "event_name", "slug", "event_properties"]
        );

        let mut props = Map::new();
        props.insert("invitee_email".to_owned(), str_value("a@x.io"));
        let kwargs = object(&[
            ("user_id", str_value("11111111-1111-4111-8111-111111111111")),
            ("event_name", str_value("user_invited_to_workspace")),
            ("slug", str_value("acme")),
            ("event_properties", Value::Object(props.clone())),
        ]);
        let call = parse_track_event_call(&Value::Array(Vec::new()), &kwargs).expect("kwargs");
        assert_eq!(call.user_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(call.event_name, "user_invited_to_workspace");
        assert_eq!(call.slug, "acme");
        assert_eq!(call.event_properties, props);

        let args = Value::Array(vec![
            str_value("11111111-1111-4111-8111-111111111111"),
            str_value("user_invited_to_workspace"),
            str_value("acme"),
            Value::Object(props),
        ]);
        let call = parse_track_event_call(&args, &Value::Object(Map::new())).expect("positional");
        assert_eq!(call.slug, "acme");

        assert!(
            parse_track_event_call(&Value::Array(Vec::new()), &Value::Object(Map::new())).is_err()
        );
        // `str(user_id)`: numbers stringify.
        let num_kwargs = object(&[
            ("user_id", Value::Number(5.into())),
            ("event_name", str_value("workspace_deleted")),
            ("slug", str_value("acme")),
            ("event_properties", Value::Object(Map::new())),
        ]);
        let call =
            parse_track_event_call(&Value::Array(Vec::new()), &num_kwargs).expect("numeric id");
        assert_eq!(call.user_id, "5");
    }

    #[test]
    fn python_str_scalars() {
        assert_eq!(python_str(&str_value("x")), Some("x".to_owned()));
        assert_eq!(python_str(&Value::Number(5.into())), Some("5".to_owned()));
        assert_eq!(python_str(&Value::Bool(true)), Some("True".to_owned()));
        assert_eq!(python_str(&Value::Bool(false)), Some("False".to_owned()));
        assert_eq!(python_str(&Value::Null), None);
    }

    #[test]
    fn posthog_config_gate_matches_fixture() {
        let fixture = evt_fixture();
        let rule = fixture["posthogConfiguration"]["rule"]
            .as_str()
            .expect("rule");
        assert!(rule.contains("both truthy"), "{rule}");
        assert_eq!(
            resolve_posthog_config(Some("key".into()), Some("https://ph.io".into())),
            Some(("key".to_owned(), "https://ph.io".to_owned()))
        );
        assert_eq!(
            resolve_posthog_config(None, Some("https://ph.io".into())),
            None
        );
        assert_eq!(resolve_posthog_config(Some("key".into()), None), None);
        assert_eq!(
            resolve_posthog_config(Some(String::new()), Some("https://ph.io".into())),
            None
        );
        assert_eq!(
            resolve_posthog_config(Some("key".into()), Some(String::new())),
            None
        );
        // Python truthiness: whitespace-only is truthy — kept as-is.
        assert!(resolve_posthog_config(Some(" ".into()), Some("https://ph.io".into())).is_some());
        let gate = fixture["track_event"]["gate"].as_str().expect("gate");
        assert!(gate.contains("Event tracking is not configured"), "{gate}");
        assert!(gate.contains("BEFORE any workspace lookup"), "{gate}");
    }

    #[test]
    fn server_host_mapping() {
        assert_eq!(
            determine_server_host("https://app.posthog.com"),
            "https://us-api.i.posthog.com"
        );
        assert_eq!(
            determine_server_host("https://us.posthog.com"),
            "https://us-api.i.posthog.com"
        );
        assert_eq!(
            determine_server_host("https://eu.posthog.com"),
            "https://eu-api.i.posthog.com"
        );
        assert_eq!(
            determine_server_host("https://ph.example.com"),
            "https://ph.example.com"
        );
        assert_eq!(
            capture_url("https://ph.example.com/"),
            "https://ph.example.com/batch/"
        );
        assert_eq!(
            capture_url("https://ph.example.com"),
            "https://ph.example.com/batch/"
        );
        assert_eq!(
            capture_url("https://app.posthog.com"),
            "https://us-api.i.posthog.com/batch/"
        );
    }

    #[test]
    fn iso_now_conditional_micros() {
        use chrono::{TimeZone, Timelike};
        let whole = Utc.with_ymd_and_hms(2026, 9, 28, 7, 0, 0).unwrap();
        assert_eq!(iso_now(whole), "2026-09-28T07:00:00+00:00");
        let frac = whole.with_nanosecond(123_456_000).unwrap();
        assert_eq!(iso_now(frac), "2026-09-28T07:00:00.123456+00:00");
    }

    #[test]
    fn capture_payload_matches_sdk() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 7, 0, 0).unwrap();
        let mut props = Map::new();
        props.insert("role".to_owned(), str_value("admin"));
        props.insert("invitee_email".to_owned(), str_value("a@x.io"));
        let request = build_capture_request(
            "phc_test",
            "https://ph.example.com/",
            "user_invited_to_workspace",
            "11111111-1111-4111-8111-111111111111",
            &props,
            "acme",
            now,
        );
        assert_eq!(request.url, "https://ph.example.com/batch/");
        assert_eq!(request.user_agent, POSTHOG_USER_AGENT);
        assert_eq!(request.timeout_secs, POSTHOG_TIMEOUT_SECS);

        // Envelope key order: batch, sentAt, api_key (`request.py:36-53`).
        let body = request.body.as_object().expect("body object");
        let keys: Vec<&str> = body.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["batch", "sentAt", "api_key"]);
        assert_eq!(body["api_key"], str_value("phc_test"));
        assert_eq!(body["sentAt"], str_value("2026-09-28T07:00:00+00:00"));

        // Message key order: properties, timestamp, context, distinct_id,
        // event (`client.py:188-195`); NO uuid key (None is popped).
        let batch = body["batch"].as_array().expect("batch");
        assert_eq!(batch.len(), 1);
        let msg = batch[0].as_object().expect("msg");
        let msg_keys: Vec<&str> = msg.keys().map(String::as_str).collect();
        assert_eq!(
            msg_keys,
            vec!["properties", "timestamp", "context", "distinct_id", "event"]
        );
        assert_eq!(msg["event"], str_value("user_invited_to_workspace"));
        assert_eq!(
            msg["distinct_id"],
            str_value("11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(msg["timestamp"], str_value("2026-09-28T07:00:00+00:00"));
        assert_eq!(msg["context"], Value::Object(Map::new()));

        // Properties: caller props in order, then $groups, $lib,
        // $lib_version, $geoip_disable (capture + _enqueue order).
        let out = msg["properties"].as_object().expect("properties");
        let prop_keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(
            prop_keys,
            vec![
                "role",
                "invitee_email",
                "$groups",
                "$lib",
                "$lib_version",
                "$geoip_disable"
            ]
        );
        assert_eq!(out["$groups"], object(&[("workspace", str_value("acme"))]));
        assert_eq!(out["$lib"], str_value("posthog-python"));
        assert_eq!(out["$lib_version"], str_value("3.5.0"));
        assert_eq!(out["$geoip_disable"], Value::Bool(true));

        // Fixture capture shape: per-event construct, groups carry the
        // SLUG string (not the workspace id) — the fixture cell annotates
        // the value with that rule, so pin the rule text, not the cell.
        let fixture = evt_fixture();
        let capture = &fixture["capture"];
        assert!(
            capture["call"].as_str().unwrap().contains("PER EVENT"),
            "{}",
            capture["call"]
        );
        let groups_note = capture["args"]["groups"]["workspace"]
            .as_str()
            .expect("groups note");
        assert!(groups_note.contains("SLUG"), "{groups_note}");
        assert!(
            groups_note.contains("not the workspace id"),
            "{groups_note}"
        );
        assert_eq!(
            capture["args"]["distinct_id"].as_str(),
            Some("str(user_id)")
        );
    }

    #[test]
    fn activity_log_row_parses_full_map() {
        let fixture = log_fixture();
        let own = fixture["postgres_columns"]["own"]
            .as_array()
            .expect("own columns");
        assert_eq!(own.len(), 10);

        let row = parse_activity_log_row(&sample_log_data()).expect("full map");
        assert_eq!(row.token_identifier, "x");
        assert_eq!(row.path, "/api/issues/");
        assert_eq!(row.method, "GET");
        assert_eq!(row.response_code, 200);
        assert_eq!(row.query_params, Some("x".to_owned()));

        let mut missing = sample_log_data();
        missing.remove("token_identifier");
        assert!(parse_activity_log_row(&missing).is_err());
        let mut bad_code = sample_log_data();
        bad_code.insert("response_code".to_owned(), str_value("200"));
        assert!(parse_activity_log_row(&bad_code).is_err());
        let mut nulls = Map::new();
        nulls.insert("token_identifier".to_owned(), str_value("t"));
        nulls.insert("path".to_owned(), str_value("/"));
        nulls.insert("method".to_owned(), str_value("GET"));
        nulls.insert("response_code".to_owned(), Value::Number(500.into()));
        let row = parse_activity_log_row(&nulls).expect("sparse map");
        assert_eq!(row.query_params, None);
        assert_eq!(row.ip_address, None);
    }

    #[test]
    fn sql_shapes_match_models() {
        assert!(API_ACTIVITY_LOG_INSERT.starts_with("INSERT INTO api_activity_logs"));
        for column in [
            "token_identifier",
            "path",
            "method",
            "query_params",
            "headers",
            "body",
            "response_code",
            "response_body",
            "ip_address",
            "user_agent",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
        ] {
            assert!(API_ACTIVITY_LOG_INSERT.contains(column), "missing {column}");
        }
        assert!(API_ACTIVITY_LOG_INSERT.contains("$12::inet"), "inet cast");
        assert!(WORKSPACE_OWNER_SQL.contains("FROM workspaces"));
        assert!(WORKSPACE_OWNER_SQL.contains("slug = $1"));
        assert!(
            WORKSPACE_OWNER_SQL.contains("deleted_at IS NULL"),
            "soft-delete manager guard"
        );
    }

    #[test]
    fn mongo_doc_conversion() {
        let mut map = Map::new();
        map.insert("path".to_owned(), str_value("/api/issues/"));
        map.insert("response_code".to_owned(), Value::Number(200.into()));
        map.insert("body".to_owned(), Value::Null);
        let doc = mongo_log_to_document(&map).expect("document");
        assert_eq!(doc.get_str("path").expect("path"), "/api/issues/");
        assert_eq!(doc.get_i32("response_code").expect("code"), 200);
        assert_eq!(doc.get("body"), Some(&bson::Bson::Null));
    }
}
