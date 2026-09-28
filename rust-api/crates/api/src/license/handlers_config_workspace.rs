//! Configuration + workspace handlers (D-01, PIDASHCONV-123).
//!
//! Ports `license/api/views/configuration.py` (all three endpoints) and
//! `license/api/views/workspace.py` (both endpoints):
//!
//! - `GET /api/instances/configurations/` — `InstanceConfigurationEndpoint.get`
//!   (`configuration.py:35-39`): admin gate, every row (`-created_at`),
//!   `InstanceConfigurationSerializer` (decrypt + `source`/`is_managed`).
//! - `PATCH /api/instances/configurations/` — `.patch`
//!   (`configuration.py:43-60`): `filter(key__in=body keys)`; unknown keys
//!   ignored, never created; `""` for `None` else `str(raw).strip()`;
//!   `encrypt_data` when `is_encrypted`; `bulk_update(["value"],
//!   batch_size=100)`; the re-serialized rows answer.
//! - `DELETE /api/instances/configurations/disable-email-feature/` —
//!   `DisableEmailFeatureEndpoint.delete` (`configuration.py:67-86`): one
//!   conditional `UPDATE` over the six keys (`ENABLE_SMTP` becomes `"0"`,
//!   the rest `""`); any failure is 400
//!   `{"error": "Failed to disable email configuration"}`.
//! - `POST /api/instances/email-credentials-check/` —
//!   `EmailCredentialCheckEndpoint.post` (`configuration.py:90-172`):
//!   `receiver_email`-required 400; `get_email_configuration`;
//!   `int(EMAIL_PORT)`; SMTP send with the 9-branch error matrix plus the
//!   generic-`Exception` fallthrough, every message string verbatim.
//! - `GET /api/instances/workspace-slug-check/` —
//!   `InstanceWorkSpaceAvailabilityCheckEndpoint.get`
//!   (`workspace.py:22-32`): admin gate; missing/empty slug is 400;
//!   `slug__iexact` exists **or** restricted-list membership answers
//!   `{status: false}`, otherwise `{status: true}`.
//! - `GET /api/instances/workspaces/` — `InstanceWorkSpaceEndpoint.get`
//!   (`workspace.py:40-69`): admin gate; `Count` annotations
//!   (`total_projects`, `total_members` with `member__is_bot=False`,
//!   `is_active=True`); optional `name__icontains` search; `paginate`
//!   (`default_per_page=10`, `max_per_page=10`, 12-key envelope).
//! - `POST /api/instances/workspaces/` — `.post` (`workspace.py:71-110`):
//!   `name`+`slug`-required 400; 80/48 length 400; serializer validation
//!   (any failure raises a DRF `ValidationError` through
//!   `is_valid(raise_exception=True)`, which the base `handle_exception`
//!   delegates to DRF first, so the response is the raw detail dict — the
//!   trailing per-field-list return is dead code); `save(owner=request.user)`
//!   plus a
//!   `WorkspaceMember(role=20)` row; `IntegrityError` containing
//!   `"already exists"` is 409, anything else falls off the `except` block
//!   (500, BUG-3).
//!
//! Layering (all foundation use is read-only): row SQL + pure transforms in
//! `pidash_db::license::queries`; shapes in `pidash_types::license`;
//! `get_email_configuration` + Fernet in `pidash_services::license`;
//! the admin gate decision in `pidash_auth::license`; cursor/envelope math
//! and datetime rendering in the F-07 kernels (`crate::paginator`,
//! `crate::serializer`). This module owns the HTTP shell, the session gate
//! (via `super`), the SMTP conversation, and the workspace write path.
//!
//! Ported bugs and deliberate warts (also listed in the PR):
//!
//! - BUG-2 (`configuration.py:111`): `int(EMAIL_PORT)` runs outside the
//!   `try`, so garbage raises `ValueError` into the base 500, not the
//!   fallthrough 400. `None` (a `NULL` row with no env fallback) raises
//!   `TypeError` the same way.
//! - BUG-3 (`workspace.py:105-110`): an `IntegrityError` whose message
//!   lacks `"already exists"` (every real Postgres unique violation reads
//!   `duplicate key value ...`) falls off the `except` block, so the view
//!   returns `None` and Django answers 500. The 409 branch is dead in
//!   practice and ported as written.
//! - The trailing `return Response([...serializer.errors...], 400)`
//!   (`workspace.py:100-103`) is dead code: `is_valid(raise_exception=True)`
//!   raises a DRF `ValidationError` on any field error, which the base
//!   `handle_exception` delegates to DRF first, so the response is the raw
//!   detail dict (ported as `detail_response`).
//! - The restricted-slug check is a case-sensitive `in`
//!   (`workspace.py:31`); `"API"` is available, `"api"` is not.
//! - `disable-email` overwrites an encrypted `EMAIL_HOST_PASSWORD` with `""`
//!   in the clear (`is_encrypted` untouched).
//! - POST performs no view-level stripping (unlike PATCH's explicit
//!   `str(raw).strip()`); DRF `CharField` trimming still applies and is
//!   ported per field.
//! - Response caching (`cache_response` 2h on the GETs, `invalidate_cache`
//!   on the writes) has no Rust counterpart: the contract suites cannot
//!   observe it, and an uninvalidated cache would only risk stale reads.
//! - The workspace list performs per-row owner/asset lookups, like Django's
//!   N+1 (no `select_related("owner")` on the list path); the emitted row
//!   SQL matches the fixtures exactly.
//! - SMTP trust anchors are Mozilla roots (`webpki-roots`), while
//!   `smtplib` uses the system store; `AUTH` follows smtplib's preferred
//!   order (`CRAM-MD5`, `PLAIN`, `LOGIN`, with `LOGIN` initial-response).
//! - `str(float)` for PATCH values uses shortest-round-trip rendering,
//!   which matches CPython `repr` except for very large/small exponents.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use serde_json::Value;
use std::collections::HashMap;

use super::{json_response, owned, query_last, require_admin, Actor, Denial, QueryMap};
use crate::state::AppState;

/// Register the five configuration/workspace paths. Sibling license paths
/// stay unmatched and proxy to Django; unowned methods on these paths
/// proxy too (Django's 405-after-auth and metadata responses).
pub fn config_workspace_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/configurations/",
            owned(
                get(get_configurations).patch(patch_configurations),
                &["GET", "PATCH"],
            ),
        )
        .route(
            "/api/instances/configurations/disable-email-feature/",
            owned(delete(disable_email_feature), &["DELETE"]),
        )
        .route(
            "/api/instances/email-credentials-check/",
            owned(post(email_credential_check), &["POST"]),
        )
        .route(
            "/api/instances/workspace-slug-check/",
            owned(get(workspace_slug_check), &["GET"]),
        )
        .route(
            "/api/instances/workspaces/",
            owned(
                get(list_workspaces).post(create_workspace),
                &["GET", "POST"],
            ),
        )
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// InstanceConfigurationEndpoint
// ---------------------------------------------------------------------------

/// `GET configurations/` (`configuration.py:35-39`).
async fn get_configurations(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `InstanceConfiguration.objects.all()` (`Meta.ordering -created_at`).
    let sql = format!(
        "SELECT {} FROM \"instance_configurations\" \
         WHERE \"instance_configurations\".\"deleted_at\" IS NULL \
         ORDER BY \"instance_configurations\".\"created_at\" DESC",
        pidash_db::license::queries::CONFIG_COLUMNS.join(", ")
    );
    let rows: Vec<sqlx::postgres::PgRow> = match sqlx::query(&sql).fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let keyring =
        pidash_services::license::encryption::Keyring::from_secret(&state.settings().secret_key);
    let registry = pidash_db::config::registry::global();
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mapped = match pidash_db::license::queries::map_config_row(row) {
            Ok(mapped) => mapped,
            Err(_) => return Denial::ServerError.into_response(),
        };
        out.push(rendered_config_value(&mapped, &keyring, registry, &actor));
    }
    json_response(&out)
}

/// Owned rendering scratch: datetimes in the request zone (`TimezoneMixin`),
/// UUIDs as strings, `NULL` staying `null`.
struct RenderedConfig {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    key: String,
    value: Option<String>,
    category: String,
    is_encrypted: bool,
    created_by: Option<String>,
    updated_by: Option<String>,
}

impl RenderedConfig {
    fn from_row(
        row: &pidash_db::license::models::instance_configuration::InstanceConfiguration,
        actor: &Actor,
    ) -> Self {
        Self {
            id: row.id.to_string(),
            created_at: crate::serializer::render_datetime_in(&row.created_at, &actor.timezone),
            updated_at: crate::serializer::render_datetime_in(&row.updated_at, &actor.timezone),
            deleted_at: row
                .deleted_at
                .as_ref()
                .map(|dt| crate::serializer::render_datetime_in(dt, &actor.timezone)),
            key: row.key.clone(),
            value: row.value.clone(),
            category: row.category.clone(),
            is_encrypted: row.is_encrypted,
            created_by: row.created_by_id.map(|id| id.to_string()),
            updated_by: row.updated_by_id.map(|id| id.to_string()),
        }
    }
}

/// Serialize one configuration row to an owned `Value` (struct field order
/// is the wire order: `id` first via `PrimaryKeyRelatedField`, model
/// `_meta` order, then appended `source` / `is_managed`).
fn rendered_config_value(
    row: &pidash_db::license::models::instance_configuration::InstanceConfiguration,
    keyring: &pidash_services::license::encryption::Keyring,
    registry: &pidash_db::config::registry::ConfigRegistry,
    actor: &Actor,
) -> Value {
    let rendered = RenderedConfig::from_row(row, actor);
    let decrypt =
        |stored: &str| pidash_services::license::encryption::decrypt_data(keyring, Some(stored));
    let source_of = |key: &str| {
        registry.get(key).map(|entry| match entry.source {
            pidash_db::config::registry::ConfigSource::Env => "env",
            pidash_db::config::registry::ConfigSource::Db => "db",
        })
    };
    let view_row = pidash_types::license::serializers_core::InstanceConfigurationRow {
        id: &rendered.id,
        created_at: &rendered.created_at,
        updated_at: &rendered.updated_at,
        deleted_at: rendered.deleted_at.as_deref(),
        key: &rendered.key,
        value: rendered.value.as_deref(),
        category: &rendered.category,
        is_encrypted: rendered.is_encrypted,
        created_by: rendered.created_by.as_deref(),
        updated_by: rendered.updated_by.as_deref(),
    };
    let view = pidash_types::license::serializers_core::instance_configuration_to_representation(
        &view_row, &decrypt, &source_of,
    );
    serde_json::to_value(&view).expect("config view serializes")
}

// ---------------------------------------------------------------------------
// InstanceConfigurationEndpoint.patch + DisableEmailFeatureEndpoint
// ---------------------------------------------------------------------------

/// `PATCH configurations/` (`configuration.py:43-60`).
async fn patch_configurations(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.keys()`: a non-object body has no `.keys()`
    // (`AttributeError` -> base 500).
    let data: Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return parse_error_response(&err),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return Denial::ServerError.into_response(),
    };
    // `filter(key__in=request.data.keys())`: unknown keys are never
    // fetched and never created. An empty key set short-circuits to `[]`
    // (Django raises `EmptyResultSet` and skips the database).
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    let rows = match pidash_db::license::queries::fetch_config_by_keys(&pool, &keys).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let keyring =
        pidash_services::license::encryption::Keyring::from_secret(&state.settings().secret_key);
    // Per-row transform (`configuration.py:48-54`): `""` when the request
    // value is `None`, else `str(raw).strip()`; encrypted rows store
    // `encrypt_data(value)`.
    let mut pairs: Vec<(uuid::Uuid, String)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let raw = map.get(&row.key);
        let normalized = py_str_value(raw);
        let stored = if row.is_encrypted {
            pidash_services::license::encryption::encrypt_data(&keyring, Some(&normalized))
        } else {
            normalized
        };
        pairs.push((row.id, stored));
    }
    if !pairs.is_empty() {
        // `bulk_update(rows, ["value"], batch_size=100)`: one `UPDATE ...
        // CASE` per batch (`configuration.py:56-57`).
        for chunk in pairs.chunks(pidash_db::license::queries::CONFIG_BULK_UPDATE_BATCH_SIZE) {
            let refs: Vec<(uuid::Uuid, &str)> = chunk
                .iter()
                .map(|(id, value)| (*id, value.as_str()))
                .collect();
            match pidash_db::license::queries::apply_config_batch(&pool, &refs).await {
                Ok(_) => {}
                Err(sqlx::Error::Database(_)) => {
                    return Denial::BadError("The payload is not valid".to_owned()).into_response()
                }
                Err(_) => return Denial::ServerError.into_response(),
            }
        }
    }
    // The response re-serializes the fetched rows with the new values
    // (the queryset instances were mutated in place).
    let registry = pidash_db::config::registry::global();
    let updated: HashMap<&str, &str> = pairs
        .iter()
        .map(|(id, value)| {
            let key = rows
                .iter()
                .find(|row| row.id == *id)
                .map(|row| row.key.as_str())
                .unwrap_or("");
            (key, value.as_str())
        })
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut row = row.clone();
        if let Some(value) = updated.get(row.key.as_str()) {
            row.value = Some((*value).to_owned());
        }
        out.push(rendered_config_value(&row, &keyring, registry, &actor));
    }
    json_response(&out)
}

/// Python `str()` over a JSON body value, then `.strip()`
/// (`configuration.py:49`): `None` becomes `""` (handled by the `None`
/// arm — a missing key and an explicit `null` are identical here);
/// strings pass through; `True`/`False`/`None` render capitalized;
/// numbers render plainly; arrays/objects render as Python `repr`.
pub fn py_str_value(raw: Option<&Value>) -> String {
    match raw {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Number(n)) => py_number_str(n).trim().to_owned(),
        Some(value @ (Value::Array(_) | Value::Object(_))) => py_repr(value).trim().to_owned(),
    }
}

fn py_number_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    // `str(float)` is `repr(float)` for floats.
    crate::paginator::py_float_str(n.as_f64().unwrap_or(f64::NAN))
}

/// Python `repr()` over a JSON value: single quotes, `True`/`False`/`None`,
/// `": "`/`", "` separators, nested recursively.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(s) => py_repr_str(s),
        Value::Number(n) => py_number_str(n),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn py_repr_str(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        return format!("\"{}\"", s);
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        match ch {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// DRF `JSONParser` failure: 400 `{"detail": "JSON parse error - ..."}`.
fn parse_error_response(err: &serde_json::Error) -> Response {
    Denial::BadDetail(format!("JSON parse error - {err}")).into_response()
}

/// `DELETE configurations/disable-email-feature/`
/// (`configuration.py:67-86`).
async fn disable_email_feature(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    if let Err(denial) = require_admin(&state, extension).await {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // One conditional `UPDATE`: `ENABLE_SMTP` becomes `"0"`, the other
    // five keys become `""`. Rows for missing keys are not created.
    // Any failure is 400 with the exact message (`:82-86`).
    match pidash_db::license::queries::disable_email_config(&pool).await {
        Ok(_) => Response::builder()
            .status(StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::empty())
            .expect("empty 200 response"),
        Err(_) => {
            Denial::BadError("Failed to disable email configuration".to_owned()).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// EmailCredentialCheckEndpoint
// ---------------------------------------------------------------------------

/// `POST email-credentials-check/` (`configuration.py:89-172`).
async fn email_credential_check(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    // The endpoint declares no `permission_classes`, so it inherits the
    // base default (`InstanceAdminPermission`).
    if let Err(denial) = require_admin(&state, extension).await {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get(...)`: a non-object body has no `.get`
    // (`AttributeError` -> base 500).
    let data: Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return parse_error_response(&err),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return Denial::ServerError.into_response(),
    };
    // `receiver_email = request.data.get("receiver_email", False)`:
    // anything falsy answers 400 (`configuration.py:91-96`).
    let receiver = map.get("receiver_email");
    if !json_truthy(receiver) {
        return Denial::BadError("Receiver email is required".to_owned()).into_response();
    }
    let recipients = json_string_list(receiver);
    // `get_email_configuration()` runs outside the `try`
    // (`configuration.py:98-106`): a store failure escapes to the base
    // 500, exactly like a DB error in Python.
    let keyring =
        pidash_services::license::encryption::Keyring::from_secret(&state.settings().secret_key);
    let registry = pidash_db::config::registry::global();
    let store = pidash_db::config::PgConfigStore::new(pool.clone());
    // NOTE: `services::license::config::get_email_configuration` is not
    // used here: its future holds a bare `&dyn Fn` environment reader
    // across an await, so it is `!Send` and cannot run on the
    // multi-threaded runtime (filed separately). The equivalent composition
    // — `email_items()` (call-time env defaults) over the legacy batch
    // shim — is `Send` with the concrete store and behaves identically.
    let items = pidash_services::license::config::email_items();
    let values =
        match pidash_db::config::get_configuration_values(registry, &store, &keyring, &items).await
        {
            Ok(values) => values,
            Err(_) => return Denial::ServerError.into_response(),
        };
    let mut values = values.into_iter();
    let host = values.next().and_then(config_string);
    let username = values.next().and_then(config_string);
    let password = values.next().and_then(config_string);
    let port_raw = values.next();
    let use_tls = values
        .next()
        .is_some_and(|v| v == pidash_db::config::ConfigValue::Str("1".to_owned()));
    let use_ssl = values
        .next()
        .is_some_and(|v| v == pidash_db::config::ConfigValue::Str("1".to_owned()));
    let from_email = values.next().and_then(config_string);
    // `port=int(EMAIL_PORT)` (`configuration.py:111`, BUG-2): outside the
    // `try`, so `ValueError`/`TypeError` answer the base 500.
    let port = match port_raw {
        Some(value) => match parse_email_port(&value) {
            Some(port) => port,
            None => return Denial::ServerError.into_response(),
        },
        None => return Denial::ServerError.into_response(),
    };
    match smtp::send_test_email(smtp::EmailConfig {
        host: host.as_deref().unwrap_or(""),
        port,
        username: username.as_deref(),
        password: password.as_deref(),
        use_tls,
        use_ssl,
        from_email: from_email.as_deref(),
        recipients: &recipients,
    })
    .await
    {
        Ok(()) => json_response(&serde_json::json!({"message": "Email successfully sent."})),
        Err(err) => Denial::BadError(err.message().to_owned()).into_response(),
    }
}

/// Python truthiness over a JSON body value (`not receiver_email`).
fn json_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// `msg = ...; to=[receiver_email]`: a bare string is one recipient, a
/// list is many (`sendmail` wraps a bare string). Non-string entries are
/// stringified the way the envelope path would carry them.
fn json_string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(json_scalar_string).collect(),
        Some(other) => vec![json_scalar_string(other)],
        None => Vec::new(),
    }
}

fn json_scalar_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_number_str(n),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// A resolved email value: DB rows arrive as strings; the call-time
/// defaults can be `Int` (`EMAIL_PORT` 587), `Bool`, or `Null`.
fn config_string(value: pidash_db::config::ConfigValue) -> Option<String> {
    use pidash_db::config::ConfigValue;
    match value {
        ConfigValue::Str(s) => Some(s),
        ConfigValue::Int(i) => Some(i.to_string()),
        ConfigValue::Float(f) => Some(crate::paginator::py_float_str(f)),
        ConfigValue::Bool(true) => Some("True".to_owned()),
        ConfigValue::Bool(false) => Some("False".to_owned()),
        ConfigValue::Null => None,
    }
}

/// `int(EMAIL_PORT)` (`configuration.py:111`): Python `int()` over the
/// resolved value. Strings strip surrounding whitespace, take an optional
/// sign, allow `_` separators, and must otherwise be ASCII digits;
/// floats truncate toward zero; `True`/`False` are 1/0; anything else
/// (`None`, garbage strings) raises into the base 500 (BUG-2).
/// Returns the raw integer; range is checked by the SMTP layer, where an
/// out-of-range port raises `OverflowError` into the generic fallthrough.
pub fn parse_email_port(value: &pidash_db::config::ConfigValue) -> Option<i64> {
    use pidash_db::config::ConfigValue;
    match value {
        ConfigValue::Int(i) => Some(*i),
        ConfigValue::Bool(b) => Some(i64::from(*b)),
        ConfigValue::Float(f) => {
            if f.is_finite() {
                Some(*f as i64)
            } else {
                None
            }
        }
        ConfigValue::Str(s) => parse_py_int(s),
        ConfigValue::Null => None,
    }
}

/// Python `int(str)`: strips surrounding whitespace, takes an optional
/// `+`/`-`, allows `_` separators between digits, then requires decimal
/// digits. Overflow saturates: Python keeps arbitrary precision and fails
/// later (`OverflowError` at connect, inside the `try`), so a saturated
/// extreme preserves the fallthrough outcome.
pub fn parse_py_int(text: &str) -> Option<i64> {
    let stripped = text.trim_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match stripped.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, stripped.strip_prefix('+').unwrap_or(stripped)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    let mut saturated = false;
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let digit = ch.to_digit(10)? as i64;
        prev_underscore = false;
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(next) => value = next,
            None => saturated = true,
        }
    }
    if prev_underscore {
        return None;
    }
    if saturated {
        return Some(if negative { i64::MIN } else { i64::MAX });
    }
    Some(if negative { -value } else { value })
}

// ---------------------------------------------------------------------------
// SMTP test-send (EmailCredentialCheckEndpoint)
// ---------------------------------------------------------------------------

/// The genuine send behind `email-credentials-check`, speaking SMTP the way
/// `smtplib` + Django's `EmailBackend` do (`configuration.py:108-172`):
/// connect (+implicit TLS), greeting, EHLO/HELO, STARTTLS, AUTH, `MAIL`,
/// `RCPT`, `DATA`, `QUIT`. Every failure maps to the exact Python exception
/// branch and its message string; nothing here fabricates a send.
mod smtp {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    /// Fixed probe content (`configuration.py:118-119`).
    const SUBJECT: &str = "Email Notification from Pi Dash";
    const BODY: &str = "This is a sample email notification sent from Pi Dash application.";
    /// Django's global default (`DEFAULT_FROM_EMAIL`): the project does not
    /// override it, so a `NULL` `EMAIL_FROM` row falls back here
    /// (`EmailMessage.from_email` → `settings.DEFAULT_FROM_EMAIL`).
    const DEFAULT_FROM_EMAIL: &str = "webmaster@localhost";
    /// `local_hostname`: Django sends the cached FQDN; only the bytes on
    /// the wire differ, never any response, so a fixed name is used.
    const LOCAL_HOSTNAME: &str = "localhost";
    /// `smtplib._MAXLINE`: overlong reply lines are a protocol error.
    const MAX_LINE: usize = 8192 + 1;

    pub struct EmailConfig<'a> {
        pub host: &'a str,
        pub port: i64,
        pub username: Option<&'a str>,
        pub password: Option<&'a str>,
        pub use_tls: bool,
        pub use_ssl: bool,
        pub from_email: Option<&'a str>,
        pub recipients: &'a [String],
    }

    /// One Python exception branch of `configuration.py:131-172`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MailError {
        BadHeader,
        Auth,
        Connect,
        SenderRefused,
        ServerDisconnected,
        RecipientsRefused,
        Timeout,
        Network,
        Failed,
    }

    impl MailError {
        /// The exact response string for each branch.
        pub fn message(self) -> &'static str {
            match self {
                MailError::BadHeader => "Invalid email header.",
                MailError::Auth => "Invalid credentials provided",
                MailError::Connect => "Could not connect with the SMTP server.",
                MailError::SenderRefused => "From address is invalid.",
                MailError::ServerDisconnected => "SMTP server disconnected unexpectedly.",
                MailError::RecipientsRefused => "All recipient addresses were refused.",
                MailError::Timeout => "Timeout error while trying to connect to the SMTP server.",
                MailError::Network => {
                    "Network connection error. Please check your internet connection."
                }
                MailError::Failed => "Could not send email. Please check your configuration",
            }
        }
    }

    /// `socket` errors at connect/write scope (`OSError` propagating out of
    /// `smtplib`, not absorbed): refused/reset-class errors are builtin
    /// `ConnectionError` (`Network ...`); timeouts are builtin
    /// `TimeoutError`; DNS and everything else falls through.
    fn map_io(err: std::io::Error) -> MailError {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::TimedOut => MailError::Timeout,
            ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected => MailError::Network,
            ErrorKind::UnexpectedEof => MailError::ServerDisconnected,
            _ => MailError::Failed,
        }
    }

    /// Reply reads (`getreply`): any `OSError` while reading — timeouts
    /// included — is `SMTPServerDisconnected`, and EOF is too. Only writes
    /// propagate raw socket errors.
    fn map_read(err: std::io::Error) -> MailError {
        let _ = err;
        MailError::ServerDisconnected
    }

    // The `BufReader` lives on the connection for its whole lifetime: a
    // fresh buffer per line would swallow already-read reply bytes and
    // stall the conversation on every multiline reply.
    enum Io {
        Plain(tokio::io::BufReader<TcpStream>),
        Tls(Box<tokio::io::BufReader<tokio_rustls::client::TlsStream<TcpStream>>>),
    }

    impl Io {
        async fn read_reply(&mut self) -> Result<(i32, String), MailError> {
            let mut text = String::new();
            let mut code = -1;
            loop {
                let mut line = String::new();
                let n = match self {
                    Io::Plain(buf) => buf.read_line(&mut line).await,
                    Io::Tls(buf) => buf.read_line(&mut line).await,
                }
                .map_err(map_read)?;
                if n == 0 {
                    return Err(MailError::ServerDisconnected);
                }
                if line.len() > MAX_LINE {
                    // `SMTPResponseException(500, "Line too long.")`.
                    return Err(MailError::Failed);
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                // `get`, not indexing: non-ASCII reply bytes would panic on
                // a non-char-boundary and escape the fallthrough as a 500.
                if let Some(prefix) = trimmed.get(..3) {
                    if let Ok(parsed) = prefix.parse::<i32>() {
                        code = parsed;
                    }
                    text.push_str(trimmed.get(4..).unwrap_or(""));
                }
                // Multiline replies continue while the 4th byte is `-`.
                if !(trimmed.len() > 3 && trimmed.as_bytes()[3] == b'-') {
                    break;
                }
                text.push('\n');
            }
            Ok((code, text))
        }

        async fn write_str(&mut self, command: &str) -> Result<(), MailError> {
            let bytes = format!("{command}\r\n");
            match self {
                Io::Plain(buf) => {
                    buf.get_mut().write_all(bytes.as_bytes()).await
                }
                Io::Tls(buf) => buf.get_mut().write_all(bytes.as_bytes()).await,
            }
            .map_err(map_io)?;
            match self {
                Io::Plain(buf) => buf.get_mut().flush().await,
                Io::Tls(buf) => buf.get_mut().flush().await,
            }
            .map_err(map_io)
        }

        async fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), MailError> {
            match self {
                Io::Plain(buf) => buf.get_mut().write_all(bytes).await,
                Io::Tls(buf) => buf.get_mut().write_all(bytes).await,
            }
            .map_err(map_io)?;
            match self {
                Io::Plain(buf) => buf.get_mut().flush().await,
                Io::Tls(buf) => buf.get_mut().flush().await,
            }
            .map_err(map_io)
        }
    }

    /// `smtplib.quoteaddr`: parse the display name off, wrap the bare
    /// addr-spec in `<>`; unparseable input passes through in `<>` unless
    /// it already starts with `<`.
    fn quote_address(raw: &str) -> String {
        let trimmed = raw.trim();
        if let Some(start) = trimmed.rfind('<') {
            if let Some(end) = trimmed[start..].find('>') {
                let inner = trimmed[start + 1..start + end].trim();
                if !inner.is_empty() {
                    return format!("<{inner}>");
                }
            }
        }
        if trimmed.starts_with('<') {
            return trimmed.to_owned();
        }
        format!("<{trimmed}>")
    }

    /// `EmailMessage.message()` header validation: any header value
    /// containing a newline raises `BadHeaderError`. Subject and body are
    /// constants; `from_email` and the recipients come from config/input.
    fn check_headers(from: &str, recipients: &[String]) -> Result<(), MailError> {
        if [SUBJECT, from].iter().any(|h| h.contains(['\r', '\n'])) {
            return Err(MailError::BadHeader);
        }
        if recipients.iter().any(|r| r.contains(['\r', '\n'])) {
            return Err(MailError::BadHeader);
        }
        Ok(())
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut word = 0u32;
            for (i, byte) in chunk.iter().enumerate() {
                word |= (*byte as u32) << (16 - 8 * i);
            }
            let pad = 3 - chunk.len();
            for i in 0..4 - pad {
                out.push(ALPHABET[((word >> (18 - 6 * i)) & 63) as usize] as char);
            }
            for _ in 0..pad {
                out.push('=');
            }
        }
        out
    }

    fn tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Ok(rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth())
    }

    /// TLS handshake errors are `ssl.SSLError` in Python — the generic
    /// fallthrough — except timeouts, which stay `TimeoutError`.
    fn map_handshake(err: std::io::Error) -> MailError {
        if err.kind() == std::io::ErrorKind::TimedOut {
            MailError::Timeout
        } else {
            MailError::Failed
        }
    }

    async fn upgrade_tls(stream: TcpStream, host: &str) -> Result<Io, MailError> {
        use std::str::FromStr;
        let name = if let Ok(addr) = std::net::IpAddr::from_str(host) {
            rustls::pki_types::ServerName::IpAddress(addr.into())
        } else {
            rustls::pki_types::ServerName::try_from(host.to_owned())
                .map_err(|_| MailError::Failed)?
        };
        let config = std::sync::Arc::new(tls_config().map_err(|_| MailError::Failed)?);
        let connector = tokio_rustls::TlsConnector::from(config);
        let tls = connector
            .connect(name, stream)
            .await
            .map_err(map_handshake)?;
        Ok(Io::Tls(Box::new(tokio::io::BufReader::new(tls))))
    }

    /// `EHLO`, falling back to `HELO` (`ehlo_or_helo_if_needed`): both
    /// failing is `SMTPHeloError`, inside the `try`, so the generic
    /// fallthrough. Returns the extension lines on EHLO success.
    async fn ehlo(io: &mut Io) -> Result<Vec<String>, MailError> {
        io.write_str(&format!("EHLO {LOCAL_HOSTNAME}")).await?;
        let (code, text) = io.read_reply().await?;
        if code == 250 {
            return Ok(text.lines().map(str::to_owned).collect());
        }
        io.write_str(&format!("HELO {LOCAL_HOSTNAME}")).await?;
        let (code, _) = io.read_reply().await?;
        if code == 250 {
            return Ok(Vec::new());
        }
        Err(MailError::Failed)
    }

    fn extensions(lines: &[String]) -> (bool, bool, Vec<String>) {
        let mut starttls = false;
        let mut auth: Vec<String> = Vec::new();
        for line in lines {
            let mut parts = line.split_whitespace();
            match parts.next().map(str::to_uppercase).as_deref() {
                Some("STARTTLS") => starttls = true,
                Some("AUTH") => auth.extend(parts.map(|m| m.to_owned())),
                _ => {}
            }
        }
        (starttls, !auth.is_empty(), auth)
    }

    /// One base64 challenge/response round (`auth_cram_md5`): `user + " "
    /// + HMAC-MD5(password, challenge).hexdigest()`.
    fn cram_md5_response(username: &str, password: &str, challenge: &[u8]) -> String {
        use hmac::Mac;
        let mut mac = hmac::Hmac::<md5::Md5>::new_from_slice(password.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(challenge);
        let digest = mac.finalize().into_bytes();
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        base64_encode(format!("{username} {hex}").as_bytes())
    }

    fn base64_decode(text: &str) -> Result<Vec<u8>, MailError> {
        // `base64.decodebytes`: strict alphabet, `=` padding only at the
        // end; anything else is `binascii.Error` — the fallthrough.
        let clean: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        if !clean.len().is_multiple_of(4) {
            return Err(MailError::Failed);
        }
        let mut out = Vec::with_capacity(clean.len() / 4 * 3);
        let bytes = clean.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let mut word = 0u32;
            let mut pad = 0;
            for (j, byte) in bytes[i..i + 4].iter().enumerate() {
                let sextet = match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    b'=' => {
                        pad += 1;
                        0
                    }
                    _ => return Err(MailError::Failed),
                };
                if pad > 0 && *byte != b'=' {
                    return Err(MailError::Failed);
                }
                word |= (sextet as u32) << (18 - 6 * j);
            }
            if pad > 2 || (pad > 0 && i + 4 != bytes.len()) {
                return Err(MailError::Failed);
            }
            out.push((word >> 16) as u8);
            if pad < 2 {
                out.push((word >> 8) as u8);
            }
            if pad < 1 {
                out.push(word as u8);
            }
            i += 4;
        }
        Ok(out)
    }

    /// `auth()`: one mechanism attempt. The initial response goes out with
    /// the `AUTH` command when the mechanism provides one (`CRAM-MD5`
    /// sends a bare command); every 334 challenge is answered until the
    /// server accepts (235/503), refuses (anything else, an
    /// `SMTPAuthenticationError`), or loops past `_MAXCHALLENGE`
    /// (`SMTPException`, the fallthrough).
    async fn auth_attempt<F>(
        io: &mut Io,
        mechanism: &str,
        initial: Option<String>,
        respond: F,
    ) -> Result<(), MailError>
    where
        // Plain `&dyn Fn` would poison the future's `Send` (and axum's
        // `Handler` bound with it); the generic keeps it `Send`.
        F: Fn(&[u8]) -> String + Send,
    {
        const MAX_CHALLENGE: u32 = 5; // `smtplib._MAXCHALLENGE`
        let command = match &initial {
            Some(response) => format!("AUTH {mechanism} {response}"),
            None => format!("AUTH {mechanism}"),
        };
        io.write_str(&command).await?;
        let mut challenges = u32::from(initial.is_some());
        loop {
            let (code, text) = io.read_reply().await?;
            if code == 235 || code == 503 {
                return Ok(());
            }
            if code != 334 {
                // Any refusal, including 535-class, is
                // `SMTPAuthenticationError`.
                return Err(MailError::Auth);
            }
            challenges += 1;
            if challenges > MAX_CHALLENGE {
                return Err(MailError::Failed);
            }
            // `base64.decodebytes(resp)`: only the first line carries the
            // challenge; multiline 334s do not occur on the wire.
            let first = text.lines().next().unwrap_or("");
            let challenge = base64_decode(first)?;
            io.write_str(&respond(&challenge)).await?;
        }
    }

    /// `login(user, password)`: advertised methods tried in smtplib's
    /// preferred order (`CRAM-MD5`, `PLAIN`, `LOGIN`); an
    /// `SMTPAuthenticationError` falls through to the next method and only
    /// the last one propagates — every other error aborts immediately. No
    /// advertised method at all is `SMTPException("No suitable ...")`.
    async fn login(
        io: &mut Io,
        methods: &[String],
        username: &str,
        password: &str,
    ) -> Result<(), MailError> {
        let upper: Vec<String> = methods.iter().map(|m| m.to_uppercase()).collect();
        let plain_initial = base64_encode(format!("\0{username}\0{password}").as_bytes());
        let login_initial = base64_encode(username.as_bytes());
        let login_password = base64_encode(password.as_bytes());
        let mut order: Vec<&str> = Vec::new();
        for method in ["CRAM-MD5", "PLAIN", "LOGIN"] {
            if upper.iter().any(|m| m == method) {
                order.push(method);
            }
        }
        if order.is_empty() {
            return Err(MailError::Failed);
        }
        let mut last = MailError::Failed;
        for method in order {
            let result = match method {
                // `auth_cram_md5(None)` is `None`: no initial response.
                "CRAM-MD5" => {
                    auth_attempt(io, method, None, |challenge| {
                        cram_md5_response(username, password, challenge)
                    })
                    .await
                }
                // `auth_plain` ignores the challenge: the initial response
                // goes out again.
                "PLAIN" => {
                    auth_attempt(io, method, Some(plain_initial.clone()), |_| {
                        plain_initial.clone()
                    })
                    .await
                }
                // `auth_login(None)` answers the username up front; every
                // challenge (the count is already past the username round)
                // answers the password.
                _ => {
                    auth_attempt(io, method, Some(login_initial.clone()), |_| {
                        login_password.clone()
                    })
                    .await
                }
            };
            match result {
                Ok(()) => return Ok(()),
                Err(MailError::Auth) => last = MailError::Auth,
                Err(other) => return Err(other),
            }
        }
        Err(last)
    }

    fn message_bytes(from: &str, recipients: &[String]) -> Vec<u8> {
        // `message.as_bytes(linesep="\r\n")`: headers plus the plain body.
        // Exact bytes are unobservable past the server; the headers carry
        // the validated values.
        let to = recipients.join(", ");
        let now = chrono::Utc::now();
        let date = now.format("%a, %d %b %Y %H:%M:%S +0000").to_string();
        let mut out = format!("From: {from}\r\nTo: {to}\r\nSubject: {SUBJECT}\r\nDate: {date}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{BODY}\r\n");
        // Dot-stuffing (`smtplib.data` quotes leading periods).
        let mut stuffed = String::with_capacity(out.len());
        for line in out.split_inclusive('\n') {
            if line.starts_with('.') {
                stuffed.push('.');
            }
            stuffed.push_str(line);
        }
        out = stuffed;
        out.into_bytes()
    }

    pub async fn send_test_email(cfg: EmailConfig<'_>) -> Result<(), MailError> {
        // `port=int(...)` already ran; a range failure is `OverflowError`
        // at connect, inside the `try` — the generic fallthrough.
        let port: u16 = u16::try_from(cfg.port).map_err(|_| MailError::Failed)?;
        // `SMTP(host, port)` / `SMTP_SSL(host, port)`: refused/reset-class
        // errors are builtin `ConnectionError`, timeouts `TimeoutError`,
        // DNS and the rest fall through.
        let stream = TcpStream::connect((cfg.host, port)).await.map_err(map_io)?;
        let mut io = if cfg.use_ssl {
            // `SMTP_SSL`: the TLS handshake error is `ssl.SSLError`,
            // inside the `try` — the generic fallthrough.
            upgrade_tls(stream, cfg.host).await?
        } else {
            Io::Plain(tokio::io::BufReader::new(stream))
        };
        // Greeting: EOF is `SMTPServerDisconnected`, anything but 220 is
        // `SMTPConnectError`.
        let (code, _) = io.read_reply().await?;
        if code != 220 {
            return Err(MailError::Connect);
        }
        let mut lines = ehlo(&mut io).await?;
        if !cfg.use_ssl && cfg.use_tls {
            // `starttls()`: unadvertised is `SMTPException`; a non-220 is
            // `SMTPException`; the handshake error is `ssl.SSLError` —
            // all inside the `try`.
            let (advertised, _, _) = extensions(&lines);
            if !advertised {
                return Err(MailError::Failed);
            }
            io.write_str("STARTTLS").await?;
            let (code, _) = io.read_reply().await?;
            if code != 220 {
                return Err(MailError::Failed);
            }
            let Io::Plain(buffered) = io else {
                return Err(MailError::Failed);
            };
            // The buffer is drained here (every reply was consumed), so
            // reclaiming the stream loses nothing past STARTTLS.
            io = upgrade_tls(buffered.into_inner(), cfg.host).await?;
            lines = ehlo(&mut io).await?;
        }
        // `login()` when both are set (`if self.username and self.password`).
        let has_creds = cfg.username.is_some_and(|u| !u.is_empty())
            && cfg.password.is_some_and(|p| !p.is_empty());
        if has_creds {
            let (_, has_auth, methods) = extensions(&lines);
            if !has_auth {
                // `SMTPNotSupportedError("SMTP AUTH extension not ...")`.
                return Err(MailError::Failed);
            }
            login(
                &mut io,
                &methods,
                cfg.username.unwrap_or(""),
                cfg.password.unwrap_or(""),
            )
            .await?;
        }
        // `_send`: addresses are sanitized and the message rendered here —
        // after the transport is up — so a bad header answers
        // `BadHeaderError` even though the server was already reached.
        let from = cfg.from_email.unwrap_or(DEFAULT_FROM_EMAIL);
        check_headers(from, cfg.recipients)?;
        let envelope_from = quote_address(from);
        // `sendmail`: `MAIL` refusal (any code, 421 included) is
        // `SMTPSenderRefused`.
        io.write_str(&format!("MAIL FROM:{envelope_from}")).await?;
        let (code, _) = io.read_reply().await?;
        if code != 250 {
            return Err(MailError::SenderRefused);
        }
        // `RCPT`: 250/251 accepted; anything else recorded; 421 aborts the
        // loop immediately; all-refused is `SMTPRecipientsRefused`.
        let mut refused = 0usize;
        for recipient in cfg.recipients {
            io.write_str(&format!("RCPT TO:{}", quote_address(recipient)))
                .await?;
            let (code, _) = io.read_reply().await?;
            if code == 250 || code == 251 {
                continue;
            }
            refused += 1;
            if code == 421 {
                return Err(MailError::RecipientsRefused);
            }
        }
        if refused == cfg.recipients.len() {
            return Err(MailError::RecipientsRefused);
        }
        // `DATA`: non-354 is `SMTPDataError` (fallthrough); a non-250 end
        // is too.
        io.write_str("DATA").await?;
        let (code, _) = io.read_reply().await?;
        if code != 354 {
            return Err(MailError::Failed);
        }
        let content = message_bytes(from, cfg.recipients);
        io.write_bytes(&content).await?;
        io.write_bytes(b"\r\n.\r\n").await?;
        let (code, _) = io.read_reply().await?;
        if code != 250 {
            return Err(MailError::Failed);
        }
        // `close()` after a successful send: `quit()` errors other than
        // disconnect/SSL propagate; disconnect-shaped ones are swallowed.
        io.write_str("QUIT").await?;
        match io.read_reply().await {
            Ok((221, _)) => Ok(()),
            Ok(_) => Err(MailError::Failed),
            Err(MailError::ServerDisconnected) => Ok(()),
            Err(MailError::Failed) => Ok(()),
            Err(other) => Err(other),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn messages_match_python_exactly() {
            assert_eq!(MailError::BadHeader.message(), "Invalid email header.");
            assert_eq!(MailError::Auth.message(), "Invalid credentials provided");
            assert_eq!(
                MailError::Connect.message(),
                "Could not connect with the SMTP server."
            );
            assert_eq!(
                MailError::SenderRefused.message(),
                "From address is invalid."
            );
            assert_eq!(
                MailError::ServerDisconnected.message(),
                "SMTP server disconnected unexpectedly."
            );
            assert_eq!(
                MailError::RecipientsRefused.message(),
                "All recipient addresses were refused."
            );
            assert_eq!(
                MailError::Timeout.message(),
                "Timeout error while trying to connect to the SMTP server."
            );
            assert_eq!(
                MailError::Network.message(),
                "Network connection error. Please check your internet connection."
            );
            assert_eq!(
                MailError::Failed.message(),
                "Could not send email. Please check your configuration"
            );
        }

        #[test]
        fn quote_address_strips_display_names() {
            assert_eq!(
                quote_address("Team Pi Dash <team@airepublic.com>"),
                "<team@airepublic.com>"
            );
            assert_eq!(quote_address("plain@example.com"), "<plain@example.com>");
            assert_eq!(
                quote_address("<bracketed@example.com>"),
                "<bracketed@example.com>"
            );
        }

        /// Multiline replies arrive in one TCP segment: the reader keeps
        /// one buffer for the whole connection, or bytes already read
        /// are lost and the second line stalls forever.
        #[tokio::test]
        async fn multiline_reply_reads_past_first_line() {
            use tokio::io::AsyncWriteExt;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut server, _) = listener.accept().await.unwrap();
                // One write: the whole reply lands in the reader's buffer
                // at once, exactly like a real EHLO answer.
                server
                    .write_all(b"250-stub greets you\r\n250-AUTH LOGIN PLAIN\r\n250 OK\r\n")
                    .await
                    .unwrap();
                // Hold the connection open until the reader is done.
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            });
            let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            let mut io = Io::Plain(tokio::io::BufReader::new(stream));
            let (code, text) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                io.read_reply(),
            )
            .await
            .expect("multiline reply must not stall")
            .unwrap();
            assert_eq!(code, 250);
            assert!(text.contains("AUTH LOGIN PLAIN"), "got: {text:?}");
            assert!(text.contains("OK"), "got: {text:?}");
        }

        /// Scripted fake SMTP server: one connection, a fixed extension
        /// list, credential checking, and a transcript of client lines.
        struct FakeSmtp {
            extensions: Vec<String>,
            expect_user: String,
            expect_password: String,
            /// When true the server answers 535 to `AUTH PLAIN` even for
            /// right credentials (a PLAIN-blind relay that still does
            /// LOGIN, like the scratch stub).
            reject_plain: bool,
            rcpt_code: i32,
        }

        async fn run_fake_smtp(
            fake: FakeSmtp,
        ) -> (std::net::SocketAddr, tokio::task::JoinHandle<Vec<String>>) {
            use tokio::io::AsyncBufReadExt;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let addr = listener.local_addr().unwrap();
            let handle = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (reader, mut writer) = stream.into_split();
                let mut reader = tokio::io::BufReader::new(reader);
                let mut transcript = Vec::new();
                async fn send(
                    writer: &mut tokio::net::tcp::OwnedWriteHalf,
                    transcript: &mut Vec<String>,
                    line: &str,
                ) {
                    use tokio::io::AsyncWriteExt;
                    writer.write_all(line.as_bytes()).await.unwrap();
                    writer.write_all(b"\r\n").await.unwrap();
                    transcript.push(format!("S: {line}"));
                }
                async fn recv(
                    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
                    transcript: &mut Vec<String>,
                ) -> String {
                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let line = line.trim_end_matches(['\r', '\n']).to_owned();
                    transcript.push(format!("C: {line}"));
                    line
                }
                send(&mut writer, &mut transcript, "220 fake ESMTP ready").await;
                let mut authenticated = false;
                loop {
                    let line = recv(&mut reader, &mut transcript).await;
                    let up = line.to_uppercase();
                    if up.starts_with("EHLO") || up.starts_with("HELO") {
                        send(&mut writer, &mut transcript, "250-fake greets you").await;
                        for ext in &fake.extensions {
                            send(&mut writer, &mut transcript, &format!("250-{ext}")).await;
                        }
                        send(&mut writer, &mut transcript, "250 OK").await;
                    } else if up.starts_with("AUTH PLAIN") {
                        let mut ok = false;
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        let initial = parts
                            .get(1)
                            .and_then(|rest| rest.split_whitespace().nth(1))
                            .map(str::to_owned);
                        let response = match initial {
                            Some(init) => init,
                            None => {
                                send(&mut writer, &mut transcript, "334 ").await;
                                let challenge = recv(&mut reader, &mut transcript).await;
                                challenge
                            }
                        };
                        if !fake.reject_plain {
                            if let Ok(decoded) = base64_decode(&response) {
                                let want = format!("\0{}\0{}", fake.expect_user, fake.expect_password);
                                ok = decoded == want.as_bytes();
                            }
                        }
                        authenticated = ok;
                        send(
                            &mut writer,
                            &mut transcript,
                            if ok { "235 authenticated" } else { "535 bad credentials" },
                        )
                        .await;
                    } else if up.starts_with("AUTH LOGIN") {
                        let mut ok = false;
                        let initial = line.split_whitespace().nth(2).map(str::to_owned);
                        let user_b64 = match initial {
                            Some(init) => init,
                            None => {
                                send(&mut writer, &mut transcript, "334 VXNlcm5hbWU6").await;
                                recv(&mut reader, &mut transcript).await
                            }
                        };
                        if base64_decode(&user_b64).unwrap_or_default()
                            == fake.expect_user.as_bytes()
                        {
                            send(&mut writer, &mut transcript, "334 UGFzc3dvcmQ6").await;
                            let pass = recv(&mut reader, &mut transcript).await;
                            if base64_decode(&pass).unwrap_or_default()
                                == fake.expect_password.as_bytes()
                            {
                                ok = true;
                            }
                        } else {
                            send(&mut writer, &mut transcript, "535 bad credentials").await;
                            continue;
                        }
                        authenticated = ok;
                        send(
                            &mut writer,
                            &mut transcript,
                            if ok { "235 authenticated" } else { "535 bad credentials" },
                        )
                        .await;
                    } else if up.starts_with("MAIL FROM") {
                        send(&mut writer, &mut transcript, "250 OK").await;
                    } else if up.starts_with("RCPT TO") {
                        send(
                            &mut writer,
                            &mut transcript,
                            if fake.rcpt_code == 250 {
                                "250 OK".to_owned()
                            } else {
                                format!("{} refused", fake.rcpt_code)
                            }
                            .as_str(),
                        )
                        .await;
                    } else if up == "DATA" {
                        send(&mut writer, &mut transcript, "354 go ahead").await;
                        loop {
                            let data = recv(&mut reader, &mut transcript).await;
                            if data == "." {
                                break;
                            }
                        }
                        send(&mut writer, &mut transcript, "250 queued").await;
                    } else if up == "QUIT" {
                        send(&mut writer, &mut transcript, "221 bye").await;
                        break;
                    } else if up == "RSET" || up == "NOOP" {
                        send(&mut writer, &mut transcript, "250 OK").await;
                    } else {
                        send(&mut writer, &mut transcript, "502 unimplemented").await;
                    }
                }
                let _ = authenticated;
                transcript
            });
            (addr, handle)
        }

        fn test_config<'a>(
            port: i64,
            username: Option<&'a str>,
            password: Option<&'a str>,
            recipients: &'a [String],
        ) -> EmailConfig<'a> {
            EmailConfig {
                host: "127.0.0.1",
                port,
                username,
                password,
                use_tls: false,
                use_ssl: false,
                from_email: Some("noreply@example.com"),
                recipients,
            }
        }

        #[tokio::test]
        async fn conversation_login_success() {
            let to = ["probe@example.com".to_owned()];
            let (addr, server) = run_fake_smtp(FakeSmtp {
                extensions: vec!["AUTH LOGIN".to_owned()],
                expect_user: "mailer".to_owned(),
                expect_password: "s3cret".to_owned(),
                reject_plain: false,
                rcpt_code: 250,
            })
            .await;
            let result = send_test_email(test_config(
                addr.port() as i64,
                Some("mailer"),
                Some("s3cret"),
                &to,
            ))
            .await;
            assert_eq!(result, Ok(()));
            let transcript = server.await.unwrap();
            // `auth_login(None)` answers the username up front.
            assert!(
                transcript.iter().any(|l| l == "C: AUTH LOGIN bWFpbGVy"),
                "transcript: {transcript:?}"
            );
            assert!(
                transcript.iter().any(|l| l == "C: czNjcmV0"),
                "transcript: {transcript:?}"
            );
        }

        #[tokio::test]
        async fn conversation_plain_falls_back_to_login() {
            // The scratch stub shape: PLAIN advertised but unimplemented,
            // LOGIN working. smtplib tries PLAIN first, eats the 535-class
            // refusal, and succeeds over LOGIN.
            let to = ["probe@example.com".to_owned()];
            let (addr, server) = run_fake_smtp(FakeSmtp {
                extensions: vec!["AUTH LOGIN PLAIN".to_owned()],
                expect_user: "mailer".to_owned(),
                expect_password: "s3cret".to_owned(),
                reject_plain: true,
                rcpt_code: 250,
            })
            .await;
            let result = send_test_email(test_config(
                addr.port() as i64,
                Some("mailer"),
                Some("s3cret"),
                &to,
            ))
            .await;
            assert_eq!(result, Ok(()));
            let transcript = server.await.unwrap();
            assert!(
                transcript.iter().any(|l| l.starts_with("C: AUTH PLAIN ")),
                "transcript: {transcript:?}"
            );
            assert!(
                transcript.iter().any(|l| l.starts_with("C: AUTH LOGIN")),
                "transcript: {transcript:?}"
            );
        }

        #[tokio::test]
        async fn conversation_bad_password_is_auth_error() {
            let to = ["probe@example.com".to_owned()];
            let (addr, _server) = run_fake_smtp(FakeSmtp {
                extensions: vec!["AUTH LOGIN PLAIN".to_owned()],
                expect_user: "mailer".to_owned(),
                expect_password: "right".to_owned(),
                reject_plain: false,
                rcpt_code: 250,
            })
            .await;
            let result = send_test_email(test_config(
                addr.port() as i64,
                Some("mailer"),
                Some("wrong"),
                &to,
            ))
            .await;
            assert_eq!(result, Err(MailError::Auth));
        }

        #[tokio::test]
        async fn conversation_refused_recipient() {
            let to = ["nobody@example.com".to_owned()];
            let (addr, _server) = run_fake_smtp(FakeSmtp {
                extensions: vec!["AUTH LOGIN".to_owned()],
                expect_user: "mailer".to_owned(),
                expect_password: "s3cret".to_owned(),
                reject_plain: false,
                rcpt_code: 550,
            })
            .await;
            let result = send_test_email(test_config(
                addr.port() as i64,
                Some("mailer"),
                Some("s3cret"),
                &to,
            ))
            .await;
            assert_eq!(result, Err(MailError::RecipientsRefused));
        }

        #[test]
        fn io_kinds_map_like_builtin_exceptions() {
            use std::io::ErrorKind;
            assert_eq!(map_io(ErrorKind::TimedOut.into()), MailError::Timeout);
            for kind in [
                ErrorKind::ConnectionRefused,
                ErrorKind::ConnectionReset,
                ErrorKind::ConnectionAborted,
                ErrorKind::BrokenPipe,
            ] {
                assert_eq!(map_io(kind.into()), MailError::Network);
            }
            assert_eq!(map_io(ErrorKind::HostUnreachable.into()), MailError::Failed);
            // Reply reads absorb everything into disconnects (`getreply`).
            assert_eq!(
                map_read(ErrorKind::TimedOut.into()),
                MailError::ServerDisconnected
            );
        }
    }
}

// ---------------------------------------------------------------------------
// InstanceWorkSpaceAvailabilityCheckEndpoint + InstanceWorkSpaceEndpoint
// ---------------------------------------------------------------------------

/// `GET workspace-slug-check/` (`workspace.py:19-32`).
async fn workspace_slug_check(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    if let Err(denial) = require_admin(&state, extension).await {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `slug = request.GET.get("slug", False)`; missing or empty is 400
    // (`workspace.py:23-29`).
    let slug = query_last(&query, "slug").unwrap_or_default();
    if slug.is_empty() {
        return Denial::BadError("Workspace Slug is required".to_owned()).into_response();
    }
    // `filter(slug__iexact=slug).exists() or slug in
    // RESTRICTED_WORKSPACE_SLUGS`: the membership test is a case-sensitive
    // Python `in` on the list. A store failure escapes to the base 500
    // (no `try` in the view).
    let exists = match slug_exists(&pool, &slug).await {
        Ok(exists) => exists,
        Err(denial) => return denial.into_response(),
    };
    let restricted = pidash_types::license::serializers_workspace::RESTRICTED_WORKSPACE_SLUGS
        .contains(&slug.as_str());
    json_response(&serde_json::json!({"status": !(exists || restricted)}))
}

async fn slug_exists(pool: &sqlx::PgPool, slug: &str) -> Result<bool, Denial> {
    // A `NUL` byte can never match a stored slug (every stored slug
    // passed the regex at creation), and the driver cannot bind one —
    // Django's probes answer `False` here, so short-circuit identically.
    if slug.contains('\0') {
        return Ok(false);
    }
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM "workspaces"
           WHERE "workspaces"."deleted_at" IS NULL
             AND UPPER("workspaces"."slug"::text) = UPPER($1)"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `GET workspaces/` (`workspace.py:40-69`).
async fn list_workspaces(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `paginate(..., default_per_page=10, max_per_page=10)`; per-page and
    // cursor failures are DRF `ParseError` 400s with `detail` bodies.
    let per_page = match crate::paginator::parse_per_page(
        query_last(&query, "per_page").as_deref(),
        pidash_db::license::queries::WORKSPACE_DEFAULT_PER_PAGE,
        pidash_db::license::queries::WORKSPACE_MAX_PER_PAGE,
    ) {
        Ok(n) => n,
        Err(err) => return Denial::BadDetail(err.detail()).into_response(),
    };
    if per_page <= 0 {
        // `max_hits` divides by the limit (`ZeroDivisionError` at 0);
        // negative limits slice with a negative bound (`ValueError`).
        // Both escape the view into the base 500.
        return Denial::ServerError.into_response();
    }
    let default_cursor = format!("{per_page}:0:0");
    let cursor_raw = query_last(&query, "cursor").unwrap_or(default_cursor);
    let cursor = match crate::paginator::Cursor::from_string(&cursor_raw) {
        Ok(cursor) => cursor,
        Err(err) => return Denial::BadDetail(err.detail()).into_response(),
    };
    let page = cursor.offset;
    let offset = page.saturating_mul(per_page);
    if offset < 0 {
        // `BadPaginationError("Pagination offset cannot be negative")`.
        return Denial::BadDetail("Error in parsing".to_owned()).into_response();
    }
    // `search = request.query_params.get("search", None)`; empty means no
    // filter (`if search:`, `workspace.py:59-61`).
    let search = match query_last(&query, "search") {
        Some(term) if !term.is_empty() => Some(term),
        _ => None,
    };
    // `total_count` is the filtered `COUNT(*)` (a second query, exactly
    // like `queryset.count()` in `get_result`).
    let total: i64 = match workspace_total_count(&pool, search.as_deref()).await {
        Ok(total) => total,
        Err(denial) => return denial.into_response(),
    };
    let (rows, has_more) = match pidash_db::license::queries::fetch_workspace_page(
        &pool,
        search.as_deref(),
        page,
        per_page,
    )
    .await
    {
        Ok(page) => page,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let total_pages = match crate::paginator::max_hits(total, per_page) {
        Ok(pages) => pages,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let next = crate::paginator::next_cursor(per_page, page, has_more).to_string();
    let prev = crate::paginator::prev_cursor(per_page, page).to_string();
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        match render_workspace_row(&pool, row, &actor).await {
            Ok(value) => results.push(value),
            Err(denial) => return denial.into_response(),
        }
    }
    let envelope = crate::paginator::PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: total,
        next_cursor: next,
        prev_cursor: prev,
        next_page_results: has_more,
        prev_page_results: page > 0,
        count: results.len(),
        total_pages,
        total_results: total,
        extra_stats: None,
        results,
    };
    json_response(&envelope.to_json_value())
}

async fn workspace_total_count(pool: &sqlx::PgPool, search: Option<&str>) -> Result<i64, Denial> {
    let (sql, param): (String, Option<String>) = match search {
        Some(term) => (
            "SELECT COUNT(*) FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND UPPER(\"workspaces\".\"name\"::text) LIKE UPPER($1))".to_owned(),
            Some(pidash_db::license::queries::icontains_param(term)),
        ),
        None => (
            "SELECT COUNT(*) FROM \"workspaces\" WHERE \"workspaces\".\"deleted_at\" IS NULL".to_owned(),
            None,
        ),
    };
    let mut query = sqlx::query_as::<_, (i64,)>(&sql);
    if let Some(param) = param {
        query = query.bind(param);
    }
    query
        .fetch_one(pool)
        .await
        .map(|row| row.0)
        .map_err(|_| Denial::ServerError)
}

/// One annotated list row through `WorkspaceSerializer` (read path):
/// `owner` nests the row's user, `logo_url` is the model property,
/// `total_projects`/`total_members` come from the annotations.
async fn render_workspace_row(
    pool: &sqlx::PgPool,
    row: &pidash_db::license::queries::WorkspaceListRow,
    actor: &Actor,
) -> Result<Value, Denial> {
    let owner: Option<(uuid::Uuid, Option<String>, String, String)> =
        sqlx::query_as(r#"SELECT id, email, first_name, last_name FROM users WHERE id = $1"#)
            .bind(row.owner_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((owner_id, owner_email, first_name, last_name)) = owner else {
        // The FK is non-nullable; a missing owner row cannot render.
        return Err(Denial::ServerError);
    };
    let logo_url = workspace_logo_url(pool, row).await?;
    let view = pidash_types::license::serializers_workspace::Workspace {
        id: row.id.to_string(),
        owner: pidash_types::license::serializers_workspace::UserLite {
            id: owner_id.to_string(),
            email: owner_email,
            first_name,
            last_name,
        },
        logo_url,
        total_projects: Some(Some(row.total_projects)),
        total_members: Some(Some(row.total_members)),
        created_at: crate::serializer::render_datetime_in(&row.created_at, &actor.timezone),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, &actor.timezone),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, &actor.timezone)),
        name: row.name.clone(),
        logo: row.logo.clone(),
        slug: row.slug.clone(),
        organization_size: row.organization_size.clone(),
        timezone: row.timezone.clone(),
        background_color: row.background_color.clone(),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        logo_asset: row.logo_asset_id.map(|id| id.to_string()),
    };
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `Workspace.logo_url` (`db/models/workspace.py:145-154`):
/// `logo_asset.asset_url` when a live asset row exists, else `logo` when
/// set, else `None`. `asset_url` (`db/models/asset.py:80-98`) renders by
/// `entity_type`; `None` fills render as `"None"`, exactly like the
/// f-strings do.
async fn workspace_logo_url(
    pool: &sqlx::PgPool,
    row: &pidash_db::license::queries::WorkspaceListRow,
) -> Result<Option<String>, Denial> {
    let Some(asset_id) = row.logo_asset_id else {
        return Ok(row.logo.clone().filter(|logo| !logo.is_empty()));
    };
    // `(id, entity_type, workspace_id, project_id, issue_id)`.
    type AssetRow = (
        uuid::Uuid,
        Option<String>,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
    );
    let asset: Option<AssetRow> = sqlx::query_as(
        r#"SELECT id, entity_type, workspace_id, project_id, issue_id
               FROM file_assets WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, entity_type, workspace_id, project_id, issue_id)) = asset else {
        return Ok(row.logo.clone().filter(|logo| !logo.is_empty()));
    };
    let url = match entity_type.as_deref() {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let slug = asset_workspace_slug(pool, workspace_id).await?;
            Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{id}/",
                slug,
                none_str(&project_id.map(|id| id.to_string())),
                none_str(&issue_id.map(|id| id.to_string())),
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let slug = asset_workspace_slug(pool, workspace_id).await?;
            Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/{id}/",
                slug,
                none_str(&project_id.map(|id| id.to_string())),
            ))
        }
        _ => None,
    };
    Ok(url.or_else(|| row.logo.clone().filter(|logo| !logo.is_empty())))
}

async fn asset_workspace_slug(
    pool: &sqlx::PgPool,
    workspace_id: Option<uuid::Uuid>,
) -> Result<String, Denial> {
    let Some(workspace_id) = workspace_id else {
        return Ok("None".to_owned());
    };
    let row: Option<(String,)> = sqlx::query_as(r#"SELECT slug FROM workspaces WHERE id = $1"#)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0).unwrap_or_else(|| "None".to_owned()))
}

/// Python `str(None)` inside the asset-URL f-strings.
fn none_str(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "None".to_owned())
}

// ---------------------------------------------------------------------------
// InstanceWorkSpaceEndpoint.post
// ---------------------------------------------------------------------------

/// Field-validated workspace input (`WorkspaceSerializer` write path,
/// `workspace.py:73-98`). `is_valid(raise_exception=True)` raises a DRF
/// `ValidationError` on any field error; the app-base `handle_exception`
/// delegates to DRF first (`super().handle_exception(exc)`), so the
/// response is the raw detail dict — every failing field in serializer
/// field order, every message in validator order — which makes the
/// trailing per-field-list return (`workspace.py:100-103`) dead code.
struct ValidatedWorkspace {
    name: String,
    logo: Option<String>,
    logo_asset: Option<uuid::Uuid>,
    slug: String,
    organization_size: Option<String>,
    timezone: String,
    background_color: String,
}

/// One serializer error dict: `(field, messages)` in field order.
type FieldErrors = Vec<(String, Vec<String>)>;

fn field_error(errors: &mut FieldErrors, field: &str, message: &str) {
    if let Some(entry) = errors.iter_mut().find(|(name, _)| name == field) {
        entry.1.push(message.to_owned());
    } else {
        errors.push((field.to_owned(), vec![message.to_owned()]));
    }
}

fn detail_response(errors: &FieldErrors) -> Response {
    let mut map = serde_json::Map::with_capacity(errors.len());
    for (field, messages) in errors {
        map.insert(
            field.clone(),
            Value::Array(messages.iter().map(|m| Value::String(m.clone())).collect()),
        );
    }
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&map).expect("detail dict serializes"),
        ))
        .expect("400 detail response")
}

/// `POST workspaces/` (`workspace.py:71-110`).
async fn create_workspace(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get(...)`: a non-object body has no `.get`
    // (`AttributeError` -> base 500).
    let data: Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return parse_error_response(&err),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return Denial::ServerError.into_response(),
    };
    // `if not name or not slug` (`workspace.py:78-82`).
    let name_raw = map.get("name");
    let slug_raw = map.get("slug");
    if !json_truthy(name_raw) || !json_truthy(slug_raw) {
        return Denial::BadError("Both name and slug are required".to_owned()).into_response();
    }
    // `len(name) > 80 or len(slug) > 48` (`workspace.py:84-88`).
    // `len()` over a non-sized value (`int`, `bool`, `float`) raises
    // `TypeError` into the base 500; lists/dicts take `len()` normally.
    let name_len = match json_len(name_raw) {
        Some(len) => len,
        None => return Denial::ServerError.into_response(),
    };
    let slug_len = match json_len(slug_raw) {
        Some(len) => len,
        None => return Denial::ServerError.into_response(),
    };
    if name_len > 80 || slug_len > 48 {
        return Denial::BadError("The maximum length for name is 80 and for slug is 48".to_owned())
            .into_response();
    }
    // Serializer validation (`is_valid(raise_exception=True)`): a DRF
    // `ValidationError` answers the raw detail dict.
    let validated = match validate_workspace_input(&pool, map).await {
        Ok(validated) => validated,
        Err(ValidationFailure::Detail(errors)) => return detail_response(&errors),
        Err(ValidationFailure::Store) => return Denial::ServerError.into_response(),
    };
    // `serializer.save(owner=request.user)`: `created_by` is stamped from
    // the request user (`BaseModel.save` via crum); `updated_by` stays
    // `None` on insert.
    let workspace_id = uuid::Uuid::new_v4();
    let created_at = chrono::Utc::now();
    let updated_at = chrono::Utc::now();
    let insert: Result<Option<(uuid::Uuid,)>, sqlx::Error> = sqlx::query_as(
        r#"INSERT INTO workspaces
           (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            id, name, logo, logo_asset_id, owner_id, slug,
            organization_size, timezone, background_color)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
           RETURNING id"#,
    )
    .bind(created_at)
    .bind(updated_at)
    .bind(actor.id)
    .bind(None::<uuid::Uuid>)
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .bind(workspace_id)
    .bind(&validated.name)
    .bind(&validated.logo)
    .bind(validated.logo_asset)
    .bind(actor.id)
    .bind(&validated.slug)
    .bind(&validated.organization_size)
    .bind(&validated.timezone)
    .bind(&validated.background_color)
    .fetch_optional(&pool)
    .await;
    match insert {
        Ok(_) => {}
        Err(err) => {
            // `except IntegrityError: if "already exists" in str(e)` —
            // real Postgres violations never contain it (BUG-3), so this
            // is 409 only in the specified case, 500 otherwise.
            let message = database_error_message(&err);
            if message.contains("already exists") {
                return Denial::Conflict(
                    "slug".to_owned(),
                    "The workspace with the slug already exists".to_owned(),
                )
                .into_response();
            }
            return Denial::ServerError.into_response();
        }
    }
    // `WorkspaceMember.objects.create(workspace_id, member=user, role=20,
    // company_role=...)`: model `JSONField` defaults apply Python-side.
    let company_role = match company_role_value(map.get("company_role")) {
        Some(value) => value,
        None => return Denial::ServerError.into_response(),
    };
    let member_now = chrono::Utc::now();
    let member_result = sqlx::query(
        r#"INSERT INTO workspace_members
           (created_at, updated_at, id, role, member_id, workspace_id,
            view_props, default_props, issue_props, is_active,
            explored_features, getting_started_checklist, tips,
            company_role, created_by_id, updated_by_id, deleted_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                   $14, $15, $16, $17)"#,
    )
    .bind(member_now)
    .bind(member_now)
    .bind(uuid::Uuid::new_v4())
    .bind(20i16)
    .bind(actor.id)
    .bind(workspace_id)
    .bind(sqlx::types::Json(default_view_props()))
    .bind(sqlx::types::Json(default_view_props()))
    .bind(sqlx::types::Json(default_issue_props()))
    .bind(true)
    .bind(sqlx::types::Json(serde_json::json!({})))
    .bind(sqlx::types::Json(serde_json::json!({})))
    .bind(sqlx::types::Json(serde_json::json!({})))
    .bind(company_role)
    .bind(actor.id)
    .bind(None::<uuid::Uuid>)
    .bind(None::<chrono::DateTime<chrono::Utc>>)
    .execute(&pool)
    .await;
    if let Err(err) = member_result {
        // Same `except IntegrityError` as the workspace row: the
        // 409 needs `"already exists"` in the message, anything else
        // falls off the block into the base 500.
        if database_error_message(&err).contains("already exists") {
            return Denial::Conflict(
                "slug".to_owned(),
                "The workspace with the slug already exists".to_owned(),
            )
            .into_response();
        }
        return Denial::ServerError.into_response();
    }
    // `Response(serializer.data, 201)`: annotations are absent on a fresh
    // row, so `total_projects`/`total_members` are skipped, exactly like
    // DRF's `SkipField`.
    let view = pidash_types::license::serializers_workspace::Workspace {
        id: workspace_id.to_string(),
        owner: pidash_types::license::serializers_workspace::UserLite {
            id: actor.id.to_string(),
            email: actor.email.clone(),
            first_name: actor.first_name.clone(),
            last_name: actor.last_name.clone(),
        },
        logo_url: validated.logo.clone().filter(|logo| !logo.is_empty()),
        total_projects: None,
        total_members: None,
        created_at: crate::serializer::render_datetime_in(&created_at, &actor.timezone),
        updated_at: crate::serializer::render_datetime_in(&updated_at, &actor.timezone),
        deleted_at: None,
        name: validated.name,
        logo: validated.logo,
        slug: validated.slug,
        organization_size: validated.organization_size,
        timezone: validated.timezone,
        background_color: validated.background_color,
        created_by: Some(actor.id.to_string()),
        updated_by: None,
        logo_asset: validated.logo_asset.map(|id| id.to_string()),
    };
    let body = serde_json::to_string(&view).expect("workspace view serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("201 workspace response")
}

/// Python `len()` over a JSON body value (`workspace.py:84`): strings
/// count code points, arrays/objects count items, everything else raises
/// `TypeError`.
fn json_len(value: Option<&Value>) -> Option<usize> {
    match value {
        Some(Value::String(s)) => Some(s.chars().count()),
        Some(Value::Array(items)) => Some(items.len()),
        Some(Value::Object(map)) => Some(map.len()),
        _ => None,
    }
}

fn database_error_message(err: &sqlx::Error) -> String {
    match err {
        sqlx::Error::Database(db) => db.message().to_owned(),
        other => other.to_string(),
    }
}

/// `request.data.get("company_role", "")`: missing keys store `""`,
/// strings store verbatim (no serializer strips a non-field), `None`
/// stores `NULL`; numerics/bools take the Postgres assignment cast
/// (`5` -> `"5"`, `True` -> `"true"`); composites fail to adapt
/// (`DataError` -> base 500).
fn company_role_value(raw: Option<&Value>) -> Option<Option<String>> {
    match raw {
        None => Some(Some(String::new())),
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some(Some(s.clone())),
        Some(Value::Bool(true)) => Some(Some("true".to_owned())),
        Some(Value::Bool(false)) => Some(Some("false".to_owned())),
        Some(Value::Number(n)) => Some(Some(py_number_str(n))),
        Some(Value::Array(_) | Value::Object(_)) => None,
    }
}

/// `get_default_props()` (`db/models/workspace.py:22-60`): the
/// `view_props`/`default_props` content stamped Python-side on create.
fn default_view_props() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null,
            "state": null,
            "state_group": null,
            "assignees": null,
            "created_by": null,
            "labels": null,
            "start_date": null,
            "target_date": null,
            "subscriber": null
        },
        "display_filters": {
            "group_by": null,
            "order_by": "-created_at",
            "type": null,
            "sub_issue": true,
            "show_empty_groups": true,
            "layout": "list",
            "calendar_date_range": ""
        },
        "display_properties": {
            "assignee": true,
            "attachment_count": true,
            "created_on": true,
            "due_date": true,
            "estimate": true,
            "key": true,
            "labels": true,
            "link": true,
            "priority": true,
            "start_date": true,
            "state": true,
            "sub_issue_count": true,
            "updated_on": true
        }
    })
}

/// `get_issue_props()` (`db/models/workspace.py:110-111`).
fn default_issue_props() -> Value {
    serde_json::json!({
        "subscribed": true,
        "assigned": true,
        "created": true,
        "all_issues": true
    })
}

/// `"#" + random.choices(string.hexdigits, k=6)`
/// (`utils/color.py:9-13`): mixed-case hex, randomness unobservable past
/// the response. A splitmix64 stream seeded from the clock; no new
/// dependencies for six hex digits.
fn random_background_color() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15);
    let mut state =
        nanos.wrapping_add((std::process::id() as u64).wrapping_mul(0x9e3779b97f4a7c15));
    const HEXDIGITS: &[u8; 22] = b"0123456789abcdefABCDEF";
    let mut hex = String::with_capacity(6);
    for _ in 0..6 {
        state = splitmix64(state);
        let digit = HEXDIGITS[(state >> 33) as usize % HEXDIGITS.len()];
        hex.push(digit as char);
    }
    format!("#{hex}")
}

fn splitmix64(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// `TIMEZONE_CHOICES` (`db/models/workspace.py:139`, `pytz.common_timezones`): DRF rejects anything outside this list with `"<v>" is not a valid choice.`.
pub const WORKSPACE_TIMEZONES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

/// Field validation for `POST workspaces/`. Read-only keys and unknown
/// keys are ignored (read-only fields never enter `_writable_fields`);
/// `CharField` values trim (`trim_whitespace`) and reject `\0`
/// (`ProhibitNullCharactersValidator`); failures collapse to the
/// app-base `ValidationError` body (the trailing per-field return is dead
/// code). Store failures (the `iexact` probe) are the base 500.
/// Field validation for `POST workspaces/`, in serializer field order
/// (`name`, `logo`, `logo_asset`, `slug`, `organization_size`, `timezone`,
/// `background_color`). Read-only and unknown input keys are ignored.
/// Failures accumulate per field in validator order and answer the raw
/// detail dict; a store failure escapes to the base 500.
/// Validation failure: field detail (400) or a store failure (500).
enum ValidationFailure {
    Detail(FieldErrors),
    Store,
}

impl From<Denial> for ValidationFailure {
    fn from(_: Denial) -> Self {
        ValidationFailure::Store
    }
}

async fn validate_workspace_input(
    pool: &sqlx::PgPool,
    map: &serde_json::Map<String, Value>,
) -> Result<ValidatedWorkspace, ValidationFailure> {
    let mut errors: FieldErrors = Vec::new();
    // `name`: length pre-checked by the view; the field rejects
    // non-strings (`CharField.invalid`) and blanks/controls.
    let name = match map.get("name") {
        Some(Value::String(name)) => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                field_error(&mut errors, "name", "This field may not be blank.");
                None
            } else if trimmed.contains('\0') {
                field_error(&mut errors, "name", "Null characters are not allowed.");
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
        _ => {
            field_error(&mut errors, "name", "Not a valid string.");
            None
        }
    };
    // `logo`: blankable/nullable text; numerics coerce via `str()`.
    let logo = match map.get("logo") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            field_error(&mut errors, "logo", "Not a valid string.");
            None
        }
        Some(Value::Number(n)) => coerced_text(n, "logo", &mut errors),
        Some(Value::String(s)) => trimmed_text(s, "logo", &mut errors, true),
    };
    // `logo_asset`: writable `PrimaryKeyRelatedField` over live rows.
    let logo_asset = match map.get("logo_asset") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(_)) => {
            field_error(
                &mut errors,
                "logo_asset",
                "Incorrect type. Expected pk value, received bool.",
            );
            None
        }
        Some(Value::Number(n)) => asset_by_number(pool, n, &mut errors).await?,
        Some(Value::String(raw)) => asset_by_pk(pool, raw, &mut errors).await?,
        Some(Value::Array(_) | Value::Object(_)) => {
            let raw = py_repr(map.get("logo_asset").expect("matched"));
            asset_by_pk(pool, &raw, &mut errors).await?
        }
    };
    // `slug`: model validators (`slug_validator`, then the `unique=True`
    // `UniqueValidator` carrying the model's message), then DRF's
    // (`MaxLength` preempted by the view, null characters, regex), then
    // the serializer `validate_slug` (`iexact`).
    let slug = match map.get("slug") {
        Some(Value::String(slug)) => {
            if slug.trim().is_empty() {
                field_error(&mut errors, "slug", "This field may not be blank.");
                None
            } else {
                let trimmed = slug.trim().to_owned();
                let mut valid = true;
                if pidash_types::license::serializers_workspace::RESTRICTED_WORKSPACE_SLUGS
                    .contains(&trimmed.as_str())
                {
                    field_error(&mut errors, "slug", "Slug is not valid");
                    valid = false;
                }
                if unique_slug_taken(pool, &trimmed).await? {
                    field_error(
                        &mut errors,
                        "slug",
                        "Workspace with this slug already exists.",
                    );
                    valid = false;
                }
                if trimmed.contains('\0') {
                    field_error(&mut errors, "slug", "Null characters are not allowed.");
                    valid = false;
                }
                if !slug_syntax_ok(&trimmed) {
                    field_error(
                        &mut errors,
                        "slug",
                        "Enter a valid \"slug\" consisting of letters, numbers, underscores or hyphens.",
                    );
                    valid = false;
                }
                if valid {
                    if slug_exists(pool, &trimmed).await? {
                        field_error(&mut errors, "slug", "Slug is already in use");
                        None
                    } else {
                        Some(trimmed)
                    }
                } else {
                    None
                }
            }
        }
        _ => {
            // `SlugField.invalid` carries the regex message, not
            // `CharField`'s: non-strings fail typing as invalid slugs.
            field_error(
                &mut errors,
                "slug",
                "Enter a valid \"slug\" consisting of letters, numbers, underscores or hyphens.",
            );
            None
        }
    };
    // `organization_size`: blankable/nullable, 20 chars.
    let organization_size = match map.get("organization_size") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            field_error(&mut errors, "organization_size", "Not a valid string.");
            None
        }
        Some(Value::Number(n)) => coerced_text(n, "organization_size", &mut errors),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.contains('\0') {
                field_error(
                    &mut errors,
                    "organization_size",
                    "Null characters are not allowed.",
                );
                None
            } else if trimmed.chars().count() > 20 {
                field_error(
                    &mut errors,
                    "organization_size",
                    "Ensure this field has no more than 20 characters.",
                );
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
    };
    // `timezone`: a `ChoiceField` — no trimming, no blank gate, no
    // null-character check; anything outside `TIMEZONE_CHOICES` (compared
    // as `str(value)`) fails, `None` fails null.
    let timezone = match map.get("timezone") {
        None => Some("UTC".to_owned()),
        Some(Value::Null) => {
            field_error(&mut errors, "timezone", "This field may not be null.");
            None
        }
        Some(value) => {
            let text = json_scalar_string(value);
            if !WORKSPACE_TIMEZONES.contains(&text.as_str()) {
                field_error(
                    &mut errors,
                    "timezone",
                    &format!("\"{text}\" is not a valid choice."),
                );
                None
            } else {
                Some(text)
            }
        }
    };
    // `background_color`: default `get_random_color()` when absent.
    let background_color = match map.get("background_color") {
        None => Some(random_background_color()),
        Some(Value::Null) => {
            field_error(
                &mut errors,
                "background_color",
                "This field may not be null.",
            );
            None
        }
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => {
            field_error(&mut errors, "background_color", "Not a valid string.");
            None
        }
        Some(Value::Number(n)) => coerced_text(n, "background_color", &mut errors),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                field_error(
                    &mut errors,
                    "background_color",
                    "This field may not be blank.",
                );
                None
            } else if trimmed.contains('\0') {
                field_error(
                    &mut errors,
                    "background_color",
                    "Null characters are not allowed.",
                );
                None
            } else if trimmed.chars().count() > 255 {
                field_error(
                    &mut errors,
                    "background_color",
                    "Ensure this field has no more than 255 characters.",
                );
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
    };
    if !errors.is_empty() {
        return Err(ValidationFailure::Detail(errors));
    }
    Ok(ValidatedWorkspace {
        name: name.expect("valid"),
        logo,
        logo_asset,
        slug: slug.expect("valid"),
        organization_size,
        timezone: timezone.expect("valid"),
        background_color: background_color.expect("valid"),
    })
}

/// `CharField.to_internal_value` numerics: `str(data)` then trimmed;
/// controls rejected after.
fn coerced_text(n: &serde_json::Number, field: &str, errors: &mut FieldErrors) -> Option<String> {
    trimmed_text(&py_number_str(n), field, errors, false)
}

/// Trimmed text with the null-character gate. `allow_blank` decides
/// whether `""` stores or fails.
fn trimmed_text(
    s: &str,
    field: &str,
    errors: &mut FieldErrors,
    allow_blank: bool,
) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() && !allow_blank {
        field_error(errors, field, "This field may not be blank.");
        return None;
    }
    if trimmed.contains('\0') {
        field_error(errors, field, "Null characters are not allowed.");
        return None;
    }
    Some(trimmed.to_owned())
}

/// The `unique=True` `UniqueValidator` (`field_mapping.py`): exact match
/// over the default (live-rows) manager, carrying the model's message.
async fn unique_slug_taken(pool: &sqlx::PgPool, slug: &str) -> Result<bool, Denial> {
    // Same `NUL` short-circuit as `slug_exists`.
    if slug.contains('\0') {
        return Ok(false);
    }
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM "workspaces"
           WHERE "workspaces"."deleted_at" IS NULL AND "workspaces"."slug" = $1"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `PrimaryKeyRelatedField` over a numeric: `UUID(int=i)` is well-formed
/// for `i >= 0` (existence probe) and fails with the invalid message
/// otherwise; floats fail the same way carrying `str(value)`.
async fn asset_by_number(
    pool: &sqlx::PgPool,
    n: &serde_json::Number,
    errors: &mut FieldErrors,
) -> Result<Option<uuid::Uuid>, Denial> {
    if let Some(i) = n.as_i64() {
        if i >= 0 {
            return asset_by_id(
                pool,
                uuid::Uuid::from_u128(i as u128),
                &i.to_string(),
                errors,
            )
            .await;
        }
        // `UUID(int=i)` rejects negatives with the invalid message.
        return asset_by_pk(pool, &i.to_string(), errors).await;
    }
    if let Some(u) = n.as_u64() {
        return asset_by_id(
            pool,
            uuid::Uuid::from_u128(u128::from(u)),
            &u.to_string(),
            errors,
        )
        .await;
    }
    // Floats fail `UUID()` with the invalid message carrying `str(value)`.
    let raw = py_number_str(n);
    asset_by_pk(pool, &raw, errors).await
}

async fn asset_by_pk(
    pool: &sqlx::PgPool,
    raw: &str,
    errors: &mut FieldErrors,
) -> Result<Option<uuid::Uuid>, Denial> {
    match raw.parse::<uuid::Uuid>() {
        Ok(id) => asset_by_id(pool, id, raw, errors).await,
        Err(_) => {
            field_error(
                errors,
                "logo_asset",
                &format!("\u{201c}{raw}\u{201d} is not a valid UUID."),
            );
            Ok(None)
        }
    }
}

async fn asset_by_id(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    raw: &str,
    errors: &mut FieldErrors,
) -> Result<Option<uuid::Uuid>, Denial> {
    let exists: Option<(i32,)> =
        sqlx::query_as(r#"SELECT 1 FROM file_assets WHERE id = $1 AND deleted_at IS NULL"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    if exists.is_none() {
        field_error(
            errors,
            "logo_asset",
            &format!("Invalid pk \"{raw}\" - object does not exist."),
        );
        return Ok(None);
    }
    Ok(Some(id))
}

/// `SlugField` regex `^[-a-zA-Z0-9_]+$` (`fields.py:806`): Python `$`
/// also matches just before one trailing newline, so exactly one trailing
/// `\n` is tolerated and anything else non-alphanumeric fails.
pub fn slug_syntax_ok(slug: &str) -> bool {
    let body = slug.strip_suffix('\n').unwrap_or(slug);
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod handler_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn py_str_coercion_matches_python() {
        assert_eq!(py_str_value(None), "");
        assert_eq!(py_str_value(Some(&json!(null))), "");
        assert_eq!(py_str_value(Some(&json!("  padded  "))), "padded");
        assert_eq!(py_str_value(Some(&json!(true))), "True");
        assert_eq!(py_str_value(Some(&json!(false))), "False");
        assert_eq!(py_str_value(Some(&json!(587))), "587");
        assert_eq!(py_str_value(Some(&json!(-4))), "-4");
        assert_eq!(py_str_value(Some(&json!(1.5))), "1.5");
        assert_eq!(
            py_str_value(Some(&json!([1, true, null]))),
            "[1, True, None]"
        );
        assert_eq!(py_str_value(Some(&json!({"a": 1}))), "{'a': 1}");
    }

    #[test]
    fn py_repr_quoting_matches_python() {
        assert_eq!(py_repr(&json!("it's")), "\"it's\"");
        assert_eq!(py_repr(&json!("plain")), "'plain'");
        assert_eq!(py_repr(&json!("a\nb")), "'a\\nb'");
        assert_eq!(
            py_repr(&json!({"k": "v", "n": null})),
            "{'k': 'v', 'n': None}"
        );
    }

    #[test]
    fn int_parsing_matches_python() {
        assert_eq!(parse_py_int("587"), Some(587));
        assert_eq!(parse_py_int("  587  "), Some(587));
        assert_eq!(parse_py_int("+587"), Some(587));
        assert_eq!(parse_py_int("-0"), Some(0));
        assert_eq!(parse_py_int("5_8_7"), Some(587));
        assert_eq!(parse_py_int(""), None);
        assert_eq!(parse_py_int("587.0"), None);
        assert_eq!(parse_py_int("abc"), None);
        assert_eq!(parse_py_int("_587"), None);
        assert_eq!(parse_py_int("587_"), None);
        assert_eq!(parse_py_int("5__87"), None);
        assert_eq!(parse_py_int("0x10"), None);
        // Arbitrary precision saturates into the fallthrough, like
        // `OverflowError` at connect in Python.
        assert_eq!(parse_py_int("99999999999999999999999"), Some(i64::MAX));
        assert_eq!(parse_py_int("-99999999999999999999999"), Some(i64::MIN));
    }

    #[test]
    fn port_resolution_matches_python() {
        use pidash_db::config::ConfigValue;
        assert_eq!(parse_email_port(&ConfigValue::Int(587)), Some(587));
        assert_eq!(
            parse_email_port(&ConfigValue::Str("587".to_owned())),
            Some(587)
        );
        assert_eq!(parse_email_port(&ConfigValue::Bool(true)), Some(1));
        assert_eq!(parse_email_port(&ConfigValue::Float(587.9)), Some(587));
        assert_eq!(parse_email_port(&ConfigValue::Null), None);
        assert_eq!(
            parse_email_port(&ConfigValue::Str("garbage".to_owned())),
            None
        );
    }

    #[test]
    fn truthiness_matches_python() {
        assert!(!json_truthy(None));
        assert!(!json_truthy(Some(&json!(null))));
        assert!(!json_truthy(Some(&json!(false))));
        assert!(!json_truthy(Some(&json!(0))));
        assert!(!json_truthy(Some(&json!(""))));
        assert!(!json_truthy(Some(&json!([]))));
        assert!(!json_truthy(Some(&json!({}))));
        assert!(json_truthy(Some(&json!("x"))));
        assert!(json_truthy(Some(&json!(1))));
        assert!(json_truthy(Some(&json!([0]))));
    }

    #[test]
    fn slug_syntax_matches_drf_regex() {
        assert!(slug_syntax_ok("acme-works_2"));
        assert!(!slug_syntax_ok("has space"));
        assert!(!slug_syntax_ok("bang!"));
        assert!(!slug_syntax_ok(""));
        // Python `$` tolerates exactly one trailing newline.
        assert!(slug_syntax_ok("acme\n"));
        assert!(!slug_syntax_ok("acme\n\n"));
        assert!(!slug_syntax_ok("ac\nme"));
    }

    #[test]
    fn timezone_allowlist_matches_pytz() {
        assert_eq!(WORKSPACE_TIMEZONES.len(), 433);
        assert!(WORKSPACE_TIMEZONES.contains(&"UTC"));
        assert!(WORKSPACE_TIMEZONES.contains(&"America/New_York"));
        assert!(!WORKSPACE_TIMEZONES.contains(&"Mars/Olympus"));
    }

    #[test]
    fn json_len_matches_python_len() {
        assert_eq!(json_len(Some(&json!("abc"))), Some(3));
        assert_eq!(json_len(Some(&json!("é"))), Some(1));
        assert_eq!(json_len(Some(&json!([1, 2]))), Some(2));
        assert_eq!(json_len(Some(&json!({"a": 1}))), Some(1));
        assert_eq!(json_len(Some(&json!(5))), None);
        assert_eq!(json_len(Some(&json!(true))), None);
        assert_eq!(json_len(None), None);
    }

    #[test]
    fn company_role_coercion_matches_casts() {
        assert_eq!(company_role_value(None), Some(Some(String::new())));
        assert_eq!(company_role_value(Some(&json!(null))), Some(None));
        assert_eq!(
            company_role_value(Some(&json!("Ops"))),
            Some(Some("Ops".to_owned()))
        );
        assert_eq!(
            company_role_value(Some(&json!(true))),
            Some(Some("true".to_owned()))
        );
        assert_eq!(
            company_role_value(Some(&json!(5))),
            Some(Some("5".to_owned()))
        );
        assert_eq!(company_role_value(Some(&json!([1]))), None);
    }

    #[test]
    fn routes_cover_all_five_paths() {
        // Registration is the cutover granularity: every owned path must
        // resolve, and nothing else may be claimed.
        let _ = config_workspace_routes();
    }
}
