#![forbid(unsafe_code)]

//! API-token + timezone handlers (D-24, stage 5, PIDASHCONV-624).
//!
//! Ports `ApiTokenEndpoint` (`apps/api/pi_dash/app/views/api.py:20-72`)
//! and `TimezoneEndpoint`
//! (`apps/api/pi_dash/app/views/timezone/base.py:23-215`), plus this
//! domain's half of the route wiring:
//!
//! - `post` (`:21-39`): raw `request.data.get` create (no serializer):
//!   `label` defaults to `uuid4().hex` only when absent, `description` to
//!   `""`, `expired_at` to `None`; `user_type` is `1` iff
//!   `request.user.is_bot`. Model defaults fill the rest
//!   (`pi_dash_api_` + hex token, `is_active` true, `60/min`). Answers
//!   201 with the `APITokenSerializer` shape (token visible).
//! - `get` (`:41-49`): list filters `user` + `is_service=False`
//!   (`-created_at`); detail `.get(user, pk)` with NO service filter.
//!   Both render `APITokenReadSerializer` (no token, computed
//!   `is_active`).
//! - `delete` (`:51-54`): `.get(user, pk, is_service=False)`, instance
//!   soft-delete (stamps `deleted_at`, saves so `updated_at` moves too,
//!   enqueues the `soft_delete_related_objects` sweep), 204 empty.
//! - `patch` (`:56-72`): `.filter(user, pk,
//!   is_service=False).first()`; a miss is a BARE 404 (empty body, no
//!   content type); otherwise partial `APITokenSerializer` validation,
//!   200 with the create shape on success (token visible —
//!   `api.py:68-71` re-serializes with `APITokenSerializer`, the
//!   read-shape shorthand in older notes is wrong for PATCH), 400
//!   `serializer.errors` otherwise.
//! - `TimezoneEndpoint.get` (`timezone/base.py:23-215`): `AllowAny` +
//!   `AuthenticationThrottle` + `cache_page(2h)`. The 120-row table is
//!   transcribed verbatim; per-zone offsets come from the
//!   naive-`now()` + floor-division pipeline, sorted by
//!   `(offset, label)`, offset stripped; `{"timezones": [...]}`.
//!
//! Routes (`app/urls/api.py:10-19`, `app/urls/timezone.py:11`):
//! collection `users/api-tokens/` (GET list, POST create), detail
//! `users/api-tokens/<uuid:pk>/` (GET/PATCH/DELETE),
//! `timezones/` (GET). Owned methods are served here; every other
//! method on those paths proxies to Django (`edge.rs`), including via
//! `MethodRouter::fallback` for methods axum has no arm for.
//! Non-UUID `pk` segments proxy too: Django's `<uuid:pk>` converter
//! 404s before the view (the DEBUG HTML page under `settings.test`).
//!
//! Fixture ids: F-W24-15
//! (`rust-api/fixtures/app_workspace/handlers/routes.golden.json`);
//! consumed F-W24-05 (`ser_account_token`, PIDASHCONV-604), F-W24-12
//! R13-R14 (`queries_user`, PIDASHCONV-612), F-W24-13 (`gates`,
//! PIDASHCONV-613).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * POST has no serializer: explicit `""` labels store empty (the
//!   `uuid4().hex` default applies only when the key is absent); any
//!   non-null JSON value stringifies Python-style (`True` → `"True"`,
//!   `["a"]` → `"['a']"`); labels past 255 chars 500 (`DataError`).
//! * POST echoes the raw `expired_at` input string verbatim (DRF
//!   returns `str` values unrendered), so a naive input renders with
//!   no offset while a later GET renders `Z`.
//! * GET-detail has no `is_service` filter: service tokens read 200.
//! * PATCH on a missing/other-user/service row is a bare 404 with an
//!   EMPTY body and no content type.
//! * `is_service` is PATCH-writable (a token can flip its own service
//!   flag); `deleted_at` / `created_by` / `updated_by` validate too,
//!   so a PATCH can soft-delete its own row or reassign `created_by`
//!   (`updated_by` is stamped over by `BaseModel.save`).
//! * `""` for `created_by`/`updated_by` becomes `None`
//!   (`RelatedField.run_validation` forces empty strings to null).
//! * Form-blank PATCH input skips `label` / `is_service` /
//!   `allowed_rate_limit` (key behaves as absent) but nulls
//!   `deleted_at` / `created_by` / `updated_by` and empties
//!   `description` (`Field.get_value` HTML rules).
//! * Timezone `utc_offset`/`gmt_offset` strings use floor division,
//!   so negative sub-hour zones render a wrong hour (Marquesas
//!   `UTC-10:30`, St Johns `UTC-03:30` in DST); only the strings are
//!   wrong, the `int(%z)` sort key is exact.
//! * The timezone table carries duplicate rows (Caracas x2, Lagos x2,
//!   Karachi x2, Kolkata x4).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Offset as _, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pidash_services::app_workspace::{queries_user, ser_account_token};

use super::gates;
use crate::app_issues::Denial;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_data, JObject, JStr, JVal, JsonFail, JSON_PARSE_PREFIX,
};

/// Collection path (`app/urls/api.py:10-14`).
pub const API_TOKENS_PATH: &str = "/api/users/api-tokens/";
/// Detail path (`app/urls/api.py:15-19`).
pub const API_TOKEN_PATH: &str = "/api/users/api-tokens/{pk}/";
/// Timezone path (`app/urls/timezone.py:11`).
pub const TIMEZONES_PATH: &str = "/api/timezones/";

/// POST + PATCH accept JSON, form and multipart (DRF default parsers);
/// nothing is list-shaped and POST applies no blank rules (raw `.get`).
const TOKEN_POST_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// PATCH blank rules (`Field.get_value` HTML arm, verified live):
/// form-blank `label` / `is_service` / `allowed_rate_limit` behave as
/// absent; `description` keeps `''` (`allow_blank`); `deleted_at` /
/// `created_by` / `updated_by` keep `''` for the validators to null
/// (`allow_null` / relational forcing).
const TOKEN_PATCH_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &["allowed_rate_limit", "is_service", "label"],
};

/// Owned methods serve locally; every other method on these paths
/// proxies to Django (unowned-method `405`s, collection `PATCH` /
/// `DELETE` and detail `POST` post-gate `TypeError` 500s, `OPTIONS`
/// metadata, the timezone CSRF arm), including unknown methods via
/// the `MethodRouter` fallback.
pub fn routes() -> Router<AppState> {
    let proxy = crate::edge::proxy;
    Router::new()
        .route(
            API_TOKENS_PATH,
            axum::routing::get(token_list)
                .post(token_create)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            API_TOKEN_PATH,
            axum::routing::get(token_detail)
                .patch(token_patch)
                .delete(token_delete)
                .post(proxy)
                .put(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            TIMEZONES_PATH,
            axum::routing::get(timezone_list)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("view response")
}

/// 204 / bare-404 shape: empty body with NO content type (verified
/// live — DRF dataless responses carry no `Content-Type` here).
fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("empty response")
}

fn unsupported_media_type(message: String) -> Response {
    let body = serde_json::json!({"detail": message}).to_string();
    json_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, body)
}

/// `request.user` from the Django session: missing session, missing key,
/// or a non-UUID id is anonymous → 401.
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::Unauthorized)
}

/// Authenticated option of [`actor_user_id`] (the timezone gate never
/// 401s — `AllowAny`).
fn actor_user_id_opt(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    actor_user_id(extension).ok()
}

/// The request's `users` row facts: `user_timezone` + `is_bot`. A
/// session whose user row is gone reads anonymous (Django's `get_user`
/// falls back the same way → 401); an unparseable zone 400s with the
/// missing-key body (`TimezoneMixin.initial`: `ZoneInfoNotFoundError`
/// subclasses `KeyError` — verified live, not the generic 500).
async fn actor_facts(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<(Tz, bool, String), Denial> {
    let row: Option<(String, bool)> =
        sqlx::query_as(r#"SELECT u.user_timezone, u.is_bot FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (zone_name, is_bot) = row.ok_or(Denial::Unauthorized)?;
    let zone = zone_name
        .parse::<Tz>()
        .map_err(|_| Denial::BadError("The required key does not exist.".to_owned()))?;
    Ok((zone, is_bot, zone_name))
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `api_tokens` row in `_meta` order for the serializer views.
struct TokenRow {
    id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    label: String,
    description: String,
    is_active: bool,
    last_used: Option<DateTime<Utc>>,
    token: String,
    user_type: i16,
    expired_at: Option<DateTime<Utc>>,
    is_service: bool,
    allowed_rate_limit: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    user_id: uuid::Uuid,
    workspace_id: Option<uuid::Uuid>,
}

fn token_row_from(row: &sqlx::postgres::PgRow) -> Result<TokenRow, Denial> {
    Ok(TokenRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
        label: row.try_get("label").map_err(|_| Denial::ServerError)?,
        description: row
            .try_get("description")
            .map_err(|_| Denial::ServerError)?,
        is_active: row.try_get("is_active").map_err(|_| Denial::ServerError)?,
        last_used: row.try_get("last_used").map_err(|_| Denial::ServerError)?,
        token: row.try_get("token").map_err(|_| Denial::ServerError)?,
        user_type: row.try_get("user_type").map_err(|_| Denial::ServerError)?,
        expired_at: row.try_get("expired_at").map_err(|_| Denial::ServerError)?,
        is_service: row.try_get("is_service").map_err(|_| Denial::ServerError)?,
        allowed_rate_limit: row
            .try_get("allowed_rate_limit")
            .map_err(|_| Denial::ServerError)?,
        created_by: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
        user_id: row.try_get("user_id").map_err(|_| Denial::ServerError)?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// Rendered strings backing one serializer view (lifetimes need owned
/// storage behind the borrowed view).
struct RenderedToken {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    label: String,
    description: String,
    is_active: bool,
    last_used: Option<String>,
    token: String,
    user_type: i32,
    expired_at: Option<String>,
    is_service: bool,
    allowed_rate_limit: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    user: String,
    workspace: Option<String>,
}

fn render_token(row: &TokenRow, zone: &Tz) -> RenderedToken {
    let render = |dt: &DateTime<Utc>| crate::serializer::render_datetime_in(dt, zone);
    RenderedToken {
        id: row.id.to_string(),
        created_at: render(&row.created_at),
        updated_at: render(&row.updated_at),
        deleted_at: row.deleted_at.as_ref().map(render),
        label: row.label.clone(),
        description: row.description.clone(),
        is_active: row.is_active,
        last_used: row.last_used.as_ref().map(render),
        token: row.token.clone(),
        user_type: i32::from(row.user_type),
        expired_at: row.expired_at.as_ref().map(render),
        is_service: row.is_service,
        allowed_rate_limit: row.allowed_rate_limit.clone(),
        created_by: row.created_by.as_ref().map(ToString::to_string),
        updated_by: row.updated_by.as_ref().map(ToString::to_string),
        user: row.user_id.to_string(),
        workspace: row.workspace_id.as_ref().map(ToString::to_string),
    }
}

/// GET list/detail body: `APITokenReadSerializer` (no token, computed
/// `is_active` per row).
fn read_body(rendered: &RenderedToken, is_active: bool) -> String {
    let row = ser_account_token::ApiTokenReadRow {
        id: &rendered.id,
        created_at: &rendered.created_at,
        updated_at: &rendered.updated_at,
        deleted_at: rendered.deleted_at.as_deref(),
        label: &rendered.label,
        description: &rendered.description,
        last_used: rendered.last_used.as_deref(),
        user_type: rendered.user_type,
        expired_at: rendered.expired_at.as_deref(),
        is_service: rendered.is_service,
        allowed_rate_limit: &rendered.allowed_rate_limit,
        created_by: rendered.created_by.as_deref(),
        updated_by: rendered.updated_by.as_deref(),
        user: &rendered.user,
        workspace: rendered.workspace.as_deref(),
    };
    let view = ser_account_token::api_token_read_to_representation(&row, is_active);
    serde_json::to_string(&view).expect("read view serializes")
}

/// POST-create / PATCH-200 body: `APITokenSerializer` (carries the
/// token value; `expired_at` may be the raw POST echo).
fn create_body(rendered: &RenderedToken, expired_echo: Option<&str>) -> String {
    let row = ser_account_token::ApiTokenRow {
        id: &rendered.id,
        created_at: &rendered.created_at,
        updated_at: &rendered.updated_at,
        deleted_at: rendered.deleted_at.as_deref(),
        label: &rendered.label,
        description: &rendered.description,
        is_active: rendered.is_active,
        last_used: rendered.last_used.as_deref(),
        token: &rendered.token,
        user_type: rendered.user_type,
        expired_at: expired_echo.or(rendered.expired_at.as_deref()),
        is_service: rendered.is_service,
        allowed_rate_limit: &rendered.allowed_rate_limit,
        created_by: rendered.created_by.as_deref(),
        updated_by: rendered.updated_by.as_deref(),
        user: &rendered.user,
        workspace: rendered.workspace.as_deref(),
    };
    let view = ser_account_token::api_token_to_representation(&row);
    serde_json::to_string(&view).expect("create view serializes")
}

/// Swap the services builders' symbolic placeholders for positional
/// binds, left to right. `:now` takes a FRESH number per occurrence
/// (`auto_now_add` / `auto_now` / `delete()` are separate `now()`
/// calls); every other name reuses its first number.
fn bind_placeholders(fragment: String) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut numbers: BTreeMap<String, usize> = BTreeMap::new();
    let mut next = 0usize;
    let bytes = fragment.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b':'
            && index + 1 < bytes.len()
            && (bytes[index + 1].is_ascii_alphabetic() || bytes[index + 1] == b'_')
        {
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let name = &fragment[index + 1..end];
            if name == "now" {
                next += 1;
                out.push_str(&format!("${next}"));
            } else {
                let number = *numbers.entry(name.to_owned()).or_insert_with(|| {
                    next += 1;
                    next
                });
                out.push_str(&format!("${number}"));
            }
            index = end;
        } else {
            out.push(byte as char);
            index += 1;
        }
    }
    out
}

/// Parse `request.data` through content negotiation: empty is `{}`,
/// JSON keeps the CPython-envelope path, form/multipart arrives as its
/// text map (plus uploads per key — a key carrying any file reads the
/// LAST file, `QueryDict` last-wins over texts-then-files).
struct NegotiatedData {
    value: JVal,
    files: shared_body::FilesMap,
    is_html: bool,
}

#[allow(clippy::result_large_err)]
fn negotiate_data(
    headers: &HeaderMap,
    body: &[u8],
    spec: &shared_body::BodySpec,
) -> Result<NegotiatedData, Response> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => unsupported_media_type(message),
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message).into_response(),
        shared_body::BodyError::ServerError => Denial::ServerError.into_response(),
    };
    match shared_body::negotiate_body(headers, body, spec).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(NegotiatedData {
            value: JVal::Object(JObject::new()),
            files: BTreeMap::new(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText { text, .. } => parse_request_data(text.as_bytes())
            .map(|value| NegotiatedData {
                value,
                files: BTreeMap::new(),
                is_html: false,
            })
            .map_err(|fail| match fail {
                JsonFail::Message(detail) => {
                    Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")).into_response()
                }
                JsonFail::Recursion => Denial::ServerError.into_response(),
            }),
        shared_body::NegotiatedBody::Form { map, files, .. } => {
            let mut object = JObject::new();
            for (key, item) in map.iter() {
                let jval = match item {
                    serde_json::Value::String(text) => JVal::Str(JStr::from_clean(text.clone())),
                    serde_json::Value::Null => JVal::Null,
                    serde_json::Value::Array(items) => JVal::Array(
                        items
                            .iter()
                            .map(|entry| match entry {
                                serde_json::Value::String(text) => {
                                    JVal::Str(JStr::from_clean(text.clone()))
                                }
                                serde_json::Value::Null => JVal::Null,
                                _ => unreachable!("form lists hold strings and nulls"),
                            })
                            .collect(),
                    ),
                    _ => unreachable!("form maps hold strings, nulls and arrays"),
                };
                object.insert(JStr::from_clean(key.clone()), jval);
            }
            Ok(NegotiatedData {
                value: JVal::Object(object),
                files,
                is_html: true,
            })
        }
    }
}

/// `request.data.get(key)`: JSON reads the member; HTML reads the last
/// file's name when the key carries any upload, else the map member.
/// (DRF merges files into `request.data` with texts-then-files order;
/// `str(UploadedFile)` is its filename.)
fn data_get<'a>(data: &'a NegotiatedData, object: &'a JObject, key: &str) -> Option<DataValue<'a>> {
    if let Some(parts) = data.files.get(key) {
        if let Some(last) = parts.last() {
            return Some(DataValue::File(&last.filename));
        }
    }
    object.get(key).map(DataValue::Json)
}

enum DataValue<'a> {
    Json(&'a JVal),
    File(&'a str),
}

/// Best-effort post-delete sweep
/// (`soft_delete_related_objects.delay("db", "apitoken", pk, None)`,
/// `db/mixins.py:77-79`): without it the response still stands.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, pk: &uuid::Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            serde_json::Value::String("db".to_owned()),
            serde_json::Value::String("apitoken".to_owned()),
            serde_json::Value::String(pk.to_string()),
            serde_json::Value::Null,
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, "soft-delete sweep enqueue failed; response stands");
    }
}

fn now_float() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|span| span.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Datetime input parsing (Django `parse_datetime` / `parse_date`)
// ---------------------------------------------------------------------------

/// A parsed datetime input: naive wall time or an absolute UTC instant.
/// (`parse_datetime` returns naive for offset-less input, aware for
/// offset input; callers attach zones per their own field rules.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedInput {
    Naive(NaiveDateTime),
    Aware(DateTime<Utc>),
}

/// Django `parse_datetime` (`utils/dateparse.py:104-129`): CPython
/// 3.12 `datetime.fromisoformat` first, then the `datetime_re`
/// fallback. `None` covers both the no-match (`None`) and the
/// well-formed-but-invalid (`ValueError`) arms — both answer the same
/// 400s on our two call sites.
fn parse_django_datetime(text: &str) -> Option<ParsedInput> {
    if let Some(parsed) = parse_fromisoformat(text) {
        return Some(parsed);
    }
    parse_datetime_regex(text)
}

/// Django `parse_date` (`utils/dateparse.py:67-79`): `date.fromisoformat`
/// first, then the non-padded `date_re` fallback. POST-only (the model
/// field's `to_python` date fallback — `2030-6-1` 201s on POST and 400s
/// on PATCH).
fn parse_django_date(text: &str) -> Option<NaiveDate> {
    if let Some(date) = parse_iso_date(text) {
        return Some(date);
    }
    parse_date_regex(text)
}

/// CPython 3.12 `datetime.fromisoformat` accept set (pinned against the
/// interpreter, PIDASHCONV-624 probe): calendar dates (extended
/// `YYYY-MM-DD`, basic `YYYYMMDD`, zero-padded only), ISO week dates
/// (extended `YYYY-Www-D`, basic `YYYYWwwD`) — never ordinal or
/// non-padded — then end of string (midnight), or exactly ONE
/// separator scalar (any character, `X`/`\t`/`é` alike) plus a time
/// (extended `HH[:MM[:SS]]`, basic `HH[MM[SS]]`, hour-only), an
/// optional `[.,]` fraction (1+ ASCII digits, truncated past 6, always
/// sub-second even after minutes), and an optional zone (`Z` uppercase
/// only, or an offset). Components are ASCII digits only, no padding
/// whitespace anywhere.
fn parse_fromisoformat(text: &str) -> Option<ParsedInput> {
    let bytes = text.as_bytes();
    // Date part at position 0: 10-char extended/week, 8-char basic.
    let (date, rest) = parse_iso_date_prefix(bytes)?;
    if rest.is_empty() {
        return Some(ParsedInput::Naive(date.and_hms_opt(0, 0, 0)?));
    }
    let rest_str = std::str::from_utf8(rest).ok()?;
    let mut chars = rest_str.chars();
    let _separator = chars.next()?;
    let time_text = chars.as_str();
    if time_text.is_empty() {
        return None;
    }
    parse_iso_time(date, time_text)
}

/// The `fromisoformat` date prefix: returns the date plus the
/// unconsumed tail. Strictly padded; Basic and week forms included.
fn parse_iso_date_prefix(bytes: &[u8]) -> Option<(NaiveDate, &[u8])> {
    if bytes.len() >= 10 && bytes[4] == b'-' {
        // Extended `YYYY-MM-DD` or `YYYY-Www-D`.
        let year = digits_to_u32(bytes.get(0..4)?)?;
        if bytes.get(5) == Some(&b'W') {
            let week = digits_to_u32(bytes.get(6..8)?)?;
            if bytes.get(8) != Some(&b'-') {
                return None;
            }
            let weekday = *bytes.get(9)?;
            if !(b'1'..=b'7').contains(&weekday) {
                return None;
            }
            if year == 0 {
                return None;
            }
            let date = NaiveDate::from_isoywd_opt(
                year as i32,
                week,
                chrono::Weekday::try_from(weekday - b'1').ok()?,
            )?;
            return Some((date, bytes.get(10..)?));
        }
        let month = digits_to_u32(bytes.get(5..7)?)?;
        if bytes.get(7) != Some(&b'-') {
            return None;
        }
        let day = digits_to_u32(bytes.get(8..10)?)?;
        if year == 0 {
            return None;
        }
        let date = NaiveDate::from_ymd_opt(year as i32, month, day)?;
        return Some((date, bytes.get(10..)?));
    }
    if bytes.len() >= 8 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        if bytes.get(4) == Some(&b'W') {
            // Basic `YYYYWwwD`.
            let year = digits_to_u32(bytes.get(0..4)?)?;
            let week = digits_to_u32(bytes.get(5..7)?)?;
            let weekday = *bytes.get(7)?;
            if !(b'1'..=b'7').contains(&weekday) {
                return None;
            }
            if year == 0 {
                return None;
            }
            let date = NaiveDate::from_isoywd_opt(
                year as i32,
                week,
                chrono::Weekday::try_from(weekday - b'1').ok()?,
            )?;
            return Some((date, bytes.get(8..)?));
        }
        if bytes.get(0..8)?.iter().all(|b| b.is_ascii_digit()) {
            // Basic `YYYYMMDD`.
            let year = digits_to_u32(bytes.get(0..4)?)?;
            let month = digits_to_u32(bytes.get(4..6)?)?;
            let day = digits_to_u32(bytes.get(6..8)?)?;
            if year == 0 {
                return None;
            }
            let date = NaiveDate::from_ymd_opt(year as i32, month, day)?;
            return Some((date, bytes.get(8..)?));
        }
    }
    None
}

/// `date.fromisoformat` over the WHOLE string (the `parse_date` first
/// arm): same date grammar as [`parse_iso_date_prefix`], nothing after.
fn parse_iso_date(text: &str) -> Option<NaiveDate> {
    let (date, rest) = parse_iso_date_prefix(text.as_bytes())?;
    if rest.is_empty() {
        Some(date)
    } else {
        None
    }
}

/// `fromisoformat` time + optional zone after the separator.
fn parse_iso_time(date: NaiveDate, text: &str) -> Option<ParsedInput> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_digit() || !bytes[1].is_ascii_digit() {
        return None;
    }
    let hour = digits_to_u32(bytes.get(0..2)?)?;
    if hour > 23 {
        return None;
    }
    let mut pos = 2usize;
    let mut minute = 0u32;
    let mut second = 0u32;
    let mut micros = 0u32;
    let extended = bytes.get(pos) == Some(&b':');
    if extended {
        pos += 1;
        minute = two_digits(bytes, &mut pos)?;
        if minute > 59 {
            return None;
        }
        if bytes.get(pos) == Some(&b':') {
            pos += 1;
            second = two_digits(bytes, &mut pos)?;
            if second > 59 {
                return None;
            }
            if matches!(bytes.get(pos), Some(b'.') | Some(b',')) {
                pos = parse_time_fraction(bytes, pos, &mut micros)?;
            }
        } else if matches!(bytes.get(pos), Some(b'.') | Some(b',')) {
            pos = parse_time_fraction(bytes, pos, &mut micros)?;
        }
    } else if pos + 2 <= bytes.len()
        && bytes[pos].is_ascii_digit()
        && bytes[pos + 1].is_ascii_digit()
    {
        // Basic `HHMM[SS]`, or hour-only followed by zone chars.
        // `HHMM` requires the pair NOT to start a longer digit run that
        // the zone parser would need... simplest exact rule: consume
        // greedily like CPython (MM then SS), the zone arm sorts out
        // trailing digits.
        minute = two_digits(bytes, &mut pos)?;
        if minute > 59 {
            return None;
        }
        if pos + 2 <= bytes.len() && bytes[pos].is_ascii_digit() && bytes[pos + 1].is_ascii_digit()
        {
            // Ambiguity guard: `HHMMSS` needs 6 digits, but an hour-only
            // `HH` followed by a Basic offset (`+HHMM`) must not eat the
            // offset. Offsets always start with a sign or `Z`, plain
            // digits here are always time components — consume.
            second = two_digits(bytes, &mut pos)?;
            if second > 59 {
                return None;
            }
        }
        if matches!(bytes.get(pos), Some(b'.') | Some(b',')) {
            pos = parse_time_fraction(bytes, pos, &mut micros)?;
        }
    } else if matches!(bytes.get(pos), Some(b'.') | Some(b',')) {
        // Hour-only with fraction (`12.5` → `:00:00.5`).
        pos = parse_time_fraction(bytes, pos, &mut micros)?;
    }
    let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
    let naive = NaiveDateTime::new(date, time);
    parse_iso_zone(&text[pos..], naive)
}

/// Time-side `[.,]` fraction: digits truncated past 6 (never
/// rounded); EMPTY digits are accepted only before a zone tail
/// (`00.Z` / `00.+05:00` parse, `00.` at end fails).
fn parse_time_fraction(bytes: &[u8], pos: usize, micros: &mut u32) -> Option<usize> {
    let mut end = pos + 1;
    let mut digits = 0usize;
    let mut value = 0u32;
    let mut scale = 100_000u32;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        if digits < 6 {
            value += u32::from(bytes[end] - b'0') * scale;
            scale /= 10;
        }
        digits += 1;
        end += 1;
    }
    if digits == 0 && end == bytes.len() {
        return None;
    }
    *micros = value;
    Some(end)
}

/// `[.,]` + 1+ ASCII digits, truncated past 6 (never rounded) — the
/// offset-side and `datetime_re` fraction (empty always fails there).
fn parse_fraction(bytes: &[u8], pos: usize, micros: &mut u32) -> Option<usize> {
    let mut end = pos + 1;
    let mut digits = 0usize;
    let mut value = 0u32;
    let mut scale = 100_000u32;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        if digits < 6 {
            value += u32::from(bytes[end] - b'0') * scale;
            scale /= 10;
        }
        digits += 1;
        end += 1;
    }
    if digits == 0 {
        return None;
    }
    *micros = value;
    Some(end)
}

/// `fromisoformat` zone tail: empty (naive), `Z`, or an offset.
/// Offset components are NOT range-checked (`+00:60` is one hour);
/// only the total is (`timezone()` requires strictly under 24h). A
/// zero integral offset drops the fraction (see below).
fn parse_iso_zone(tail: &str, naive: NaiveDateTime) -> Option<ParsedInput> {
    if tail.is_empty() {
        return Some(ParsedInput::Naive(naive));
    }
    let bytes = tail.as_bytes();
    if tail == "Z" {
        return Some(ParsedInput::Aware(naive.and_utc()));
    }
    let (negative, rest) = match bytes.first() {
        Some(b'+') => (false, &tail[1..]),
        Some(b'-') => (true, &tail[1..]),
        _ => return None,
    };
    let rest_bytes = rest.as_bytes();
    if rest_bytes.len() < 2 || !rest_bytes[0].is_ascii_digit() || !rest_bytes[1].is_ascii_digit() {
        return None;
    }
    let hours = digits_to_u32(rest_bytes.get(0..2)?)? as i64;
    let mut pos = 2usize;
    let mut minutes = 0i64;
    let mut seconds = 0i64;
    let mut micros = 0i64;
    if rest_bytes.get(pos) == Some(&b':') {
        pos += 1;
        minutes = two_digits(rest_bytes, &mut pos)? as i64;
        if rest_bytes.get(pos) == Some(&b':') {
            pos += 1;
            seconds = two_digits(rest_bytes, &mut pos)? as i64;
        }
        if matches!(rest_bytes.get(pos), Some(b'.') | Some(b',')) {
            let mut frac = 0u32;
            pos = parse_fraction(rest_bytes, pos, &mut frac)?;
            micros = i64::from(frac);
        }
    } else {
        if pos + 2 <= rest_bytes.len()
            && rest_bytes[pos].is_ascii_digit()
            && rest_bytes[pos + 1].is_ascii_digit()
        {
            minutes = two_digits(rest_bytes, &mut pos)? as i64;
            if pos + 2 <= rest_bytes.len()
                && rest_bytes[pos].is_ascii_digit()
                && rest_bytes[pos + 1].is_ascii_digit()
            {
                seconds = two_digits(rest_bytes, &mut pos)? as i64;
            }
        }
        if matches!(rest_bytes.get(pos), Some(b'.') | Some(b',')) {
            let mut frac = 0u32;
            pos = parse_fraction(rest_bytes, pos, &mut frac)?;
            micros = i64::from(frac);
        }
    }
    if pos != rest_bytes.len() {
        return None;
    }
    // CPython quirk (verified): a zero integral offset answers exact
    // UTC — the fraction is consumed but ignored (`+00:00:00.5` →
    // `timezone.utc`, while `+01:00:00.5` keeps its half second).
    let integral = hours * 3600 + minutes * 60 + seconds;
    let total_micros = if integral == 0 {
        0
    } else {
        (integral * 1_000_000 + micros) * if negative { -1 } else { 1 }
    };
    if total_micros.abs() >= 86_400_000_000 {
        return None;
    }
    // Sub-day offsets on years 1-9999 never leave chrono's range.
    let utc = naive - chrono::TimeDelta::microseconds(total_micros);
    Some(ParsedInput::Aware(utc.and_utc()))
}

/// Django `datetime_re` fallback (non-padded dates/times, `[T ]`
/// separator only): `YYYY-M-D[T ]H:M[:S[.ffffff]][tz]`, optional
/// whitespace before an optional `Z|±HH[MM|:MM]` zone. No per-component
/// range check either (`get_fixed_timezone` enforces the 24h total).
/// ASCII digits only: Python's `\d` would also match Unicode decimal
/// digits (fullwidth etc.), which this port deliberately rejects (see
/// the PR note) — `fromisoformat` is ASCII-only too.
fn parse_datetime_regex(text: &str) -> Option<ParsedInput> {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let year = take_n(bytes, &mut pos, 4, 4)?;
    expect_byte(bytes, &mut pos, b'-')?;
    let month = take_n(bytes, &mut pos, 1, 2)?;
    expect_byte(bytes, &mut pos, b'-')?;
    let day = take_n(bytes, &mut pos, 1, 2)?;
    if !matches!(bytes.get(pos), Some(b'T') | Some(b' ')) {
        return None;
    }
    pos += 1;
    let hour = take_n(bytes, &mut pos, 1, 2)?;
    expect_byte(bytes, &mut pos, b':')?;
    let minute = take_n(bytes, &mut pos, 1, 2)?;
    let mut second = 0u32;
    let mut micros = 0u32;
    if bytes.get(pos) == Some(&b':') {
        pos += 1;
        second = take_n(bytes, &mut pos, 1, 2)?;
        if matches!(bytes.get(pos), Some(b'.') | Some(b',')) {
            pos = parse_fraction(bytes, pos, &mut micros)?;
        }
    }
    if year == 0 {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(year as i32, month, day)?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let naive = NaiveDateTime::new(
        date,
        NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?,
    );
    let tail = text.get(pos..)?;
    let tail = tail.trim_start_matches(is_python_space);
    if tail.is_empty() {
        return Some(ParsedInput::Naive(naive));
    }
    if tail == "Z" {
        return Some(ParsedInput::Aware(naive.and_utc()));
    }
    let tail_bytes = tail.as_bytes();
    let mut tpos = 0usize;
    let negative = match tail_bytes.first() {
        Some(b'+') => false,
        Some(b'-') => true,
        _ => return None,
    };
    tpos += 1;
    let hours = take_n(tail_bytes, &mut tpos, 2, 2)? as i64;
    let mut minutes = 0i64;
    if tail_bytes.get(tpos) == Some(&b':') {
        tpos += 1;
        minutes = take_n(tail_bytes, &mut tpos, 2, 2)? as i64;
    } else if tpos + 2 == tail_bytes.len()
        && tail_bytes[tpos].is_ascii_digit()
        && tail_bytes[tpos + 1].is_ascii_digit()
    {
        minutes = take_n(tail_bytes, &mut tpos, 2, 2)? as i64;
    }
    if tpos != tail_bytes.len() {
        return None;
    }
    let total_secs = (hours * 3600 + minutes * 60) * if negative { -1 } else { 1 };
    if total_secs.abs() >= 86_400 {
        return None;
    }
    Some(ParsedInput::Aware(
        (naive - chrono::TimeDelta::seconds(total_secs)).and_utc(),
    ))
}

/// Django `date_re` fallback: `YYYY-M-D`, nothing after (ASCII digits
/// only, like above).
fn parse_date_regex(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let year = take_n(bytes, &mut pos, 4, 4)?;
    expect_byte(bytes, &mut pos, b'-')?;
    let month = take_n(bytes, &mut pos, 1, 2)?;
    expect_byte(bytes, &mut pos, b'-')?;
    let day = take_n(bytes, &mut pos, 1, 2)?;
    if pos != bytes.len() {
        return None;
    }
    if year == 0 {
        return None;
    }
    NaiveDate::from_ymd_opt(year as i32, month, day)
}
fn digits_to_u32(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse::<u32>().ok()
}

fn two_digits(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 2 > bytes.len() {
        return None;
    }
    let value = digits_to_u32(bytes.get(*pos..*pos + 2)?)?;
    *pos += 2;
    Some(value)
}

fn take_n(bytes: &[u8], pos: &mut usize, min: usize, max: usize) -> Option<u32> {
    let mut end = *pos;
    while end < bytes.len() && end - *pos < max && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end - *pos < min {
        return None;
    }
    let value = digits_to_u32(bytes.get(*pos..end)?)?;
    *pos = end;
    Some(value)
}

fn expect_byte(bytes: &[u8], pos: &mut usize, expected: u8) -> Option<()> {
    if bytes.get(*pos) == Some(&expected) {
        *pos += 1;
        Some(())
    } else {
        None
    }
}

/// Python `str.strip`/`\s` whitespace: Rust `White_Space` plus the
/// `Cc` controls `\x1c-\x1f` Python also strips.
fn is_python_space(ch: char) -> bool {
    ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch)
}

fn python_strip(text: &str) -> &str {
    text.trim_matches(is_python_space)
}

// ---------------------------------------------------------------------------
// PATCH field validation (partial `APITokenSerializer`)
// ---------------------------------------------------------------------------

/// Writable-field order for the 400 body: the serializer wire order
/// restricted to validating fields (verified live on a 5-field error).
const PATCH_FIELD_ORDER: [&str; 7] = [
    "deleted_at",
    "label",
    "description",
    "is_service",
    "allowed_rate_limit",
    "created_by",
    "updated_by",
];

const MAX_CHAR_LEN: usize = 255;
const DATETIME_FORMAT_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";

/// `CharField.to_internal_value` + blank/max-length validators
/// (`label`, `description`, `allowed_rate_limit`): `bool`/list/dict
/// are "Not a valid string"; `int`/`float` stringify; strings trim
/// (`trim_whitespace`) before the blank/length checks. Dirty (lone
/// surrogate) strings validate like their length but poison the save
/// (`UnicodeEncodeError` → 500); NUL likewise (PG `0x00` → 500).
#[derive(Debug, PartialEq, Eq)]
enum CleanText {
    Clean(String),
    Dirty,
}

fn validate_char(
    value: &JVal,
    allow_blank: bool,
    max_length: Option<usize>,
) -> Result<CleanText, String> {
    let raw = match value {
        JVal::Null => return Err("This field may not be null.".to_owned()),
        JVal::Bool(_) | JVal::Array(_) | JVal::Object(_) => {
            return Err("Not a valid string.".to_owned());
        }
        JVal::Num(number) => number.py_string(),
        JVal::Str(text) => match text.to_clean_string() {
            Some(clean) => clean,
            None => {
                // Length still counts surrogate units (`len()`); the
                // save fails later.
                return length_or_dirty(text.len_chars(), max_length);
            }
        },
    };
    let trimmed = python_strip(&raw);
    if trimmed.is_empty() && !allow_blank {
        return Err("This field may not be blank.".to_owned());
    }
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            return Err(format!(
                "Ensure this field has no more than {max} characters."
            ));
        }
    }
    if raw.contains('\0') {
        return Ok(CleanText::Dirty);
    }
    Ok(CleanText::Clean(trimmed.to_owned()))
}

fn length_or_dirty(len_chars: usize, max_length: Option<usize>) -> Result<CleanText, String> {
    if let Some(max) = max_length {
        if len_chars > max {
            return Err(format!(
                "Ensure this field has no more than {max} characters."
            ));
        }
    }
    Ok(CleanText::Dirty)
}

/// PATCH value of a file input for a char field: the upload object is
/// not a string (`CharField.to_internal_value` rejects it); only a
/// whitespace-only name reaches the blank rule via `str()`.
fn char_file_outcome(allow_blank: bool, filename: &str) -> Result<String, String> {
    if python_strip(filename).is_empty() {
        if allow_blank {
            Ok(String::new())
        } else {
            Err("This field may not be blank.".to_owned())
        }
    } else {
        Err("Not a valid string.".to_owned())
    }
}

/// `BooleanField.to_internal_value` (`is_service`): strings lowercase
/// before set membership (`_lower_if_str`, so `tRUE`/`yEs`/`oFf` are
/// valid — ASCII-lowering is exact since every set member is ASCII),
/// no strip; `1`/`1.0` true, `0`/`-0`/`0.0` false, everything else
/// invalid.
fn validate_bool(value: &JVal) -> Result<bool, String> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        JVal::Null => Err("This field may not be null.".to_owned()),
        JVal::Bool(flag) => Ok(*flag),
        JVal::Num(number) => {
            if number.is_float() {
                let float = number.as_f64();
                if float == 1.0 {
                    Ok(true)
                } else if float == 0.0 {
                    Ok(false)
                } else {
                    Err(INVALID.to_owned())
                }
            } else {
                match number.text() {
                    "1" => Ok(true),
                    "0" | "-0" => Ok(false),
                    _ => Err(INVALID.to_owned()),
                }
            }
        }
        JVal::Str(text) => {
            let lowered = text
                .to_clean_string()
                .map(|clean| clean.to_ascii_lowercase());
            match lowered.as_deref() {
                Some("1" | "t" | "y" | "yes" | "true" | "on") => Ok(true),
                Some("0" | "f" | "n" | "no" | "false" | "off") => Ok(false),
                _ => Err(INVALID.to_owned()),
            }
        }
        JVal::Array(_) | JVal::Object(_) => Err(INVALID.to_owned()),
    }
}

/// The `DateTimeField` wrong-format message (also the file-input
/// outcome — an upload is not a datetime).
fn datetime_invalid_message() -> String {
    format!("Datetime has wrong format. Use one of these formats instead: {DATETIME_FORMAT_HINT}.")
}

/// DRF `DateTimeField.to_internal_value` (`deleted_at`): ISO-8601 only
/// (`parse_datetime` — no date fallback); naive input is made aware in
/// the REQUEST zone (`enforce_timezone`); a DST-gap wall time fails
/// with the `make_aware` message; ambiguous takes the first fold;
/// aware input whose request-zone rendering leaves years 1-9999 fails
/// with the `overflow` message. Empty-string HTML input was nulled by
/// `get_value` before this runs.
fn validate_datetime(
    value: &JVal,
    zone: &Tz,
    zone_name: &str,
    is_html: bool,
) -> Result<Option<DateTime<Utc>>, String> {
    let invalid = datetime_invalid_message;
    match value {
        JVal::Null => Ok(None),
        JVal::Str(text) => {
            let Some(clean) = text.to_clean_string() else {
                return Err(invalid());
            };
            if clean.is_empty() {
                if is_html {
                    return Ok(None);
                }
                return Err(invalid());
            }
            match parse_django_datetime(&clean) {
                None => Err(invalid()),
                Some(ParsedInput::Aware(instant)) => {
                    if !(1..=9999).contains(&instant.with_timezone(zone).year()) {
                        return Err("Datetime value out of range.".to_owned());
                    }
                    Ok(Some(instant))
                }
                Some(ParsedInput::Naive(naive)) => {
                    use chrono::MappedLocalTime;
                    match zone.from_local_datetime(&naive) {
                        MappedLocalTime::Single(local) => Ok(Some(local.with_timezone(&Utc))),
                        MappedLocalTime::Ambiguous(first, _) => Ok(Some(first.with_timezone(&Utc))),
                        MappedLocalTime::None => Err(format!(
                            "Invalid datetime for the timezone \"{zone_name}\"."
                        )),
                    }
                }
            }
        }
        _ => Err(invalid()),
    }
}

/// Nullable-UUID-FK failure: a 400 message, or a lone surrogate
/// inside the echoed input (breaks DRF's render → 500, ahead of every
/// 400 since the whole errors dict fails to encode).
#[derive(Debug, PartialEq, Eq)]
enum FkError {
    Message(String),
    Dirty,
}

/// Nullable-UUID-FK validation (`created_by`, `updated_by`):
/// `PrimaryKeyRelatedField` over `User`, `""` forced to `None`
/// (`RelatedField.run_validation`), `bool` rejected before the
/// queryset, `int` via `UUID(int=...)`, everything else via
/// `UUID(hex=...)` whose `ValueError`/`AttributeError` becomes the
/// curly-quote message (surfaced per-field by DRF's
/// `DjangoValidationError` catch). Returns the raw echo for the
/// `does_not_exist` message plus the parsed UUID.
/// The curly-quote UUID message (also the file-input outcome —
/// `%s` renders the upload as its filename).
fn fk_curly_message(rendered: &str) -> String {
    format!("\u{201c}{rendered}\u{201d} is not a valid UUID.")
}

fn validate_fk(value: &JVal) -> Result<Option<(String, uuid::Uuid)>, FkError> {
    let curly = |rendered: &str| FkError::Message(fk_curly_message(rendered));
    match value {
        JVal::Null => Ok(None),
        JVal::Bool(_) => Err(FkError::Message(
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        )),
        JVal::Num(number) => {
            if number.is_float() {
                return Err(curly(&number.py_string()));
            }
            // `-0` parses to int `0` (`UUID(int=0)` probes; the echo is
            // the int, `"0"`).
            if number.text() == "-0" {
                return Ok(Some(("0".to_owned(), uuid::Uuid::nil())));
            }
            match number.to_u128() {
                Some(int) => Ok(Some((number.text().to_owned(), uuid::Uuid::from_u128(int)))),
                None => Err(curly(number.text())),
            }
        }
        JVal::Str(text) => {
            let Some(clean) = text.to_clean_string() else {
                return Err(FkError::Dirty);
            };
            if clean.is_empty() {
                return Ok(None);
            }
            match parse_python_uuid(&clean) {
                Some(id) => Ok(Some((clean, id))),
                None => Err(curly(&clean)),
            }
        }
        JVal::Array(_) | JVal::Object(_) => match py_repr(value) {
            Some(rendered) => Err(curly(&rendered)),
            None => Err(FkError::Dirty),
        },
    }
}

/// CPython `uuid.UUID(hex=...)`: 32 hex digits, hyphenated
/// `8-4-4-4-12`, `{braces}`, `urn:uuid:` — uppercase accepted, no
/// whitespace stripping.
fn parse_python_uuid(text: &str) -> Option<uuid::Uuid> {
    let hex = text.strip_prefix("urn:uuid:").unwrap_or(text);
    let hex = hex
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(hex);
    if hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return uuid::Uuid::parse_str(hex).ok();
    }
    if hex.len() == 36 {
        let bytes = hex.as_bytes();
        if bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[18] == b'-'
            && bytes[23] == b'-'
            && hex
                .bytes()
                .enumerate()
                .all(|(index, b)| matches!(index, 8 | 13 | 18 | 23) || b.is_ascii_hexdigit())
        {
            return uuid::Uuid::parse_str(hex).ok();
        }
    }
    None
}

/// Assemble the PATCH 400 body in serializer wire order.
fn field_errors_body(errors: &[(String, String)]) -> String {
    let mut map = serde_json::Map::new();
    for field in PATCH_FIELD_ORDER {
        if let Some((_, message)) = errors.iter().find(|(name, _)| name == field) {
            map.insert(
                field.to_owned(),
                serde_json::Value::Array(vec![serde_json::Value::String(message.clone())]),
            );
        }
    }
    serde_json::Value::Object(map).to_string()
}

// ---------------------------------------------------------------------------
// POST coercion (raw `request.data.get`, `CharField.get_prep_value`)
// ---------------------------------------------------------------------------

/// Python `str()` over a JSON value (`label`, `description`): strings
/// verbatim, numbers via `py_string`, `True`/`False`, containers via
/// `repr()`. `None` marks a lone surrogate anywhere (encode fails →
/// 500); callers check NUL separately.
fn py_str(value: &JVal) -> Option<String> {
    match value {
        JVal::Null => None,
        JVal::Bool(true) => Some("True".to_owned()),
        JVal::Bool(false) => Some("False".to_owned()),
        JVal::Num(number) => Some(number.py_string()),
        JVal::Str(text) => text.to_clean_string(),
        JVal::Array(_) | JVal::Object(_) => py_repr(value),
    }
}

/// Python `repr()` over a JSON value (container `str()`): single
/// quotes unless the text holds `'` without `"`, short escapes for
/// `\t\n\r\\`, `\xhh`/`\uhhhh`/`\Uhhhhhhhh` for other non-printables,
/// printable non-ASCII literal. `None` on any lone surrogate.
fn py_repr(value: &JVal) -> Option<String> {
    match value {
        JVal::Null => Some("None".to_owned()),
        JVal::Bool(true) => Some("True".to_owned()),
        JVal::Bool(false) => Some("False".to_owned()),
        JVal::Num(number) => Some(number.py_string()),
        JVal::Str(text) => {
            let clean = text.to_clean_string()?;
            let has_single = clean.contains('\'');
            let has_double = clean.contains('"');
            let quote = if has_single && !has_double { '"' } else { '\'' };
            let mut out = String::new();
            out.push(quote);
            for ch in clean.chars() {
                if ch == quote {
                    out.push('\\');
                    out.push(ch);
                } else if ch == '\\' {
                    out.push_str("\\\\");
                } else if ch == '\t' {
                    out.push_str("\\t");
                } else if ch == '\n' {
                    out.push_str("\\n");
                } else if ch == '\r' {
                    out.push_str("\\r");
                } else if is_py_printable(ch) {
                    out.push(ch);
                } else {
                    let code = ch as u32;
                    if code < 0x100 {
                        out.push_str(&format!("\\x{code:02x}"));
                    } else if code < 0x10000 {
                        out.push_str(&format!("\\u{code:04x}"));
                    } else {
                        out.push_str(&format!("\\U{code:08x}"));
                    }
                }
            }
            out.push(quote);
            Some(out)
        }
        JVal::Array(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(py_repr(item)?);
            }
            Some(format!("[{}]", parts.join(", ")))
        }
        JVal::Object(object) => {
            let mut parts = Vec::new();
            for (key, item) in object.iter() {
                parts.push(format!(
                    "{}: {}",
                    py_repr(&JVal::Str(key.clone()))?,
                    py_repr(item)?
                ));
            }
            Some(format!("{{{}}}", parts.join(", ")))
        }
    }
}

/// Python `str.isprintable` approximation: ASCII graphic + space are
/// printable; `Cc`/`Cf`/`Cs`/`Co`/`Cn`/`Zl`/`Zp` and `Mc`/`Me`/`Mn`
/// marks are not. Above U+9F only the separator/controls that matter
/// in practice are excluded (`\u2028`/`\u2029`, directional marks);
/// other printable non-ASCII stays literal like CPython.
fn is_py_printable(ch: char) -> bool {
    if ch == ' ' || ch.is_ascii_graphic() {
        return true;
    }
    if ch.is_ascii() {
        return false;
    }
    if ('\u{80}'..='\u{9f}').contains(&ch) {
        return false;
    }
    if matches!(
        ch,
        '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}' | '\u{feff}'
    ) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Token handlers (`app/views/api.py:20-72`)
// ---------------------------------------------------------------------------

/// The gate row for one token route+method: every row here is
/// `IsAuthenticated`-only, so anonymous 401s and any login runs.
#[allow(clippy::result_large_err)]
fn authed_gate(
    method: &str,
    path: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Response> {
    let gate = &gates::gate_for(method, path).expect("token gate row").gate;
    if !matches!(gate, gates::Gate::Authenticated) {
        return Err(Denial::ServerError.into_response());
    }
    actor_user_id(extension).map_err(|denial| denial.into_response())
}

/// Django `<uuid:pk>` converter strictness (`converters.py:25-29`):
/// lowercase-hex hyphenated only — uppercase, unhyphenated, braced
/// and `urn:` forms never reach the view (resolver 404), so they
/// proxy like garbage.
fn parse_detail_pk(raw: &str) -> Option<uuid::Uuid> {
    let bytes = raw.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
    {
        return None;
    }
    let hex = |b: &u8| matches!(b, b'0'..=b'9' | b'a'..=b'f');
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, b)| matches!(index, 8 | 13 | 18 | 23) || hex(b))
    {
        return None;
    }
    raw.parse::<uuid::Uuid>().ok()
}

/// Forward a detail request Django's `<uuid:pk>` converter would 404
/// (non-UUID segment): the 404 HTML is byte-exact only from Django.
async fn proxy_detail_request(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body))
        .expect("proxy request");
    *request.headers_mut() = headers;
    crate::edge::proxy(State(state), request).await
}

/// `get` list (`api.py:42-45`): `user` + `is_service=False`,
/// `-created_at`, read shapes with computed `is_active`.
async fn token_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let user_id = match authed_gate("GET", "users/api-tokens/", extension) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (zone, _, _) = match actor_facts(&pool, &user_id).await {
        Ok(facts) => facts,
        Err(denial) => return denial.into_response(),
    };
    let sql = bind_placeholders(queries_user::api_token_list_sql());
    let rows = match sqlx::query(&sql).bind(user_id).fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        let token = match token_row_from(row) {
            Ok(token) => token,
            Err(denial) => return denial.into_response(),
        };
        let rendered = render_token(&token, &zone);
        let active = ser_account_token::api_token_is_active(token.expired_at, Utc::now());
        bodies.push(read_body(&rendered, active));
    }
    json_response(StatusCode::OK, format!("[{}]", bodies.join(",")))
}

/// `get` detail (`api.py:46-49`): `.get(user, pk)` — no service
/// filter — read shape; a miss 404s through `handle_exception`.
async fn token_detail(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path(pk_raw): Path<String>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    let Some(pk) = parse_detail_pk(&pk_raw) else {
        return proxy_detail_request(state, method, uri, headers, body).await;
    };
    let user_id = match authed_gate("GET", "users/api-tokens/<pk>/", extension) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (zone, _, _) = match actor_facts(&pool, &user_id).await {
        Ok(facts) => facts,
        Err(denial) => return denial.into_response(),
    };
    let sql = bind_placeholders(queries_user::api_token_detail_sql());
    let row = match sqlx::query(&sql)
        .bind(user_id)
        .bind(pk)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let row = match row {
        Some(row) => row,
        None => return Denial::NotFound.into_response(),
    };
    let token = match token_row_from(&row) {
        Ok(token) => token,
        Err(denial) => return denial.into_response(),
    };
    let rendered = render_token(&token, &zone);
    let active = ser_account_token::api_token_is_active(token.expired_at, Utc::now());
    json_response(StatusCode::OK, read_body(&rendered, active))
}

/// `delete` (`api.py:51-54`): `.get(user, pk, is_service=False)`,
/// instance soft-delete + sweep enqueue, 204 with no content type.
async fn token_delete(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path(pk_raw): Path<String>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    let Some(pk) = parse_detail_pk(&pk_raw) else {
        return proxy_detail_request(state, method, uri, headers, body).await;
    };
    let user_id = match authed_gate("DELETE", "users/api-tokens/<pk>/", extension) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = actor_facts(&pool, &user_id).await {
        return denial.into_response();
    }
    let lookup = bind_placeholders(queries_user::api_token_delete_lookup_sql());
    let found = match sqlx::query(&lookup)
        .bind(user_id)
        .bind(pk)
        .fetch_optional(&pool)
        .await
    {
        Ok(found) => found,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if found.is_none() {
        return Denial::NotFound.into_response();
    }
    let write = bind_placeholders(queries_user::api_token_soft_delete_sql());
    if sqlx::query(&write)
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(pk)
        .execute(&pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    // `delete()` saves the instance, so `BaseModel.save` stamps
    // `updated_by` too (second statement — the services text is owned;
    // same handler sequencing as the PATCH save).
    if sqlx::query("UPDATE api_tokens SET updated_by_id = $1 WHERE id = $2")
        .bind(user_id)
        .bind(pk)
        .execute(&pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    enqueue_soft_delete(&pool, &pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// `post` (`api.py:21-39`): raw `.get` create — `label` /
/// `description` / `expired_at` only; read-only, unknown and file
/// keys ignored; non-object bodies 500 on `.get`.
async fn token_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let user_id = match authed_gate("POST", "users/api-tokens/", extension) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (zone, is_bot, _) = match actor_facts(&pool, &user_id).await {
        Ok(facts) => facts,
        Err(denial) => return denial.into_response(),
    };
    let data = match negotiate_data(&headers, &body, &TOKEN_POST_SPEC) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let JVal::Object(object) = &data.value else {
        return Denial::ServerError.into_response();
    };
    // Failure order mirrors `create()` → `save()`: `expired_at`
    // `to_python` (pre-save) runs before any SQL, and the SQL errors
    // land encoding/length 500s (parse/coercion) ahead of null-constraint
    // 400s. Resolution marks nulls/dirt without answering yet.
    let mut label_null = false;
    let mut label_dirty = false;
    let label = match data_get(&data, object, "label") {
        None => uuid::Uuid::new_v4().simple().to_string(),
        Some(DataValue::File(name)) => (*name).to_owned(),
        Some(DataValue::Json(JVal::Null)) => {
            label_null = true;
            String::new()
        }
        Some(DataValue::Json(other)) => match py_str(other) {
            Some(text) => text,
            None => {
                label_dirty = true;
                String::new()
            }
        },
    };
    let mut description_null = false;
    let mut description_dirty = false;
    let description = match data_get(&data, object, "description") {
        None => String::new(),
        Some(DataValue::File(name)) => (*name).to_owned(),
        Some(DataValue::Json(JVal::Null)) => {
            description_null = true;
            String::new()
        }
        Some(DataValue::Json(other)) => match py_str(other) {
            Some(text) => text,
            None => {
                description_dirty = true;
                String::new()
            }
        },
    };
    // `expired_at`: absent/null → `None`; strings validate through the
    // model field (invalid → 400) and echo verbatim; an upload hits
    // `to_python` as the file object (`parse_datetime` raises
    // `TypeError`, uncaught) → 500, like every other non-string.
    let mut expired_echo: Option<String> = None;
    let expired_at: Option<DateTime<Utc>> = match data_get(&data, object, "expired_at") {
        None | Some(DataValue::Json(JVal::Null)) => None,
        Some(DataValue::File(_)) => return Denial::ServerError.into_response(),
        Some(DataValue::Json(JVal::Str(text))) => {
            let Some(clean) = text.to_clean_string() else {
                return Denial::BadError("Please provide valid detail".to_owned()).into_response();
            };
            match model_expired_at(&clean) {
                Ok(instant) => {
                    expired_echo = Some(clean);
                    instant
                }
                Err(response) => return response,
            }
        }
        Some(DataValue::Json(_)) => return Denial::ServerError.into_response(),
    };
    if label_dirty
        || description_dirty
        || label.contains('\0')
        || description.contains('\0')
        || label.chars().count() > MAX_CHAR_LEN
    {
        return Denial::ServerError.into_response();
    }
    if label_null || description_null {
        return Denial::BadError("The payload is not valid".to_owned()).into_response();
    }
    let id = uuid::Uuid::new_v4();
    let token = format!("pi_dash_api_{}", uuid::Uuid::new_v4().simple());
    let created = Utc::now();
    let updated = Utc::now();
    let user_type: i16 = if is_bot { 1 } else { 0 };
    let sql = bind_placeholders(queries_user::api_token_insert_sql());
    if let Err(error) = sqlx::query(&sql)
        .bind(created)
        .bind(updated)
        .bind(user_id)
        .bind(id)
        .bind(&label)
        .bind(&description)
        .bind(&token)
        .bind(user_id)
        .bind(user_type)
        .bind(expired_at)
        .execute(&pool)
        .await
    {
        if is_unique_violation(&error) {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
        return Denial::ServerError.into_response();
    }
    let row = TokenRow {
        id,
        created_at: created,
        updated_at: updated,
        deleted_at: None,
        label,
        description,
        is_active: true,
        last_used: None,
        token,
        user_type,
        expired_at,
        is_service: false,
        allowed_rate_limit: "60/min".to_owned(),
        created_by: Some(user_id),
        updated_by: None,
        user_id,
        workspace_id: None,
    };
    let rendered = render_token(&row, &zone);
    json_response(
        StatusCode::CREATED,
        create_body(&rendered, expired_echo.as_deref()),
    )
}

/// POST `expired_at` through the model field (`to_python` + naive→UTC
/// `get_prep_value`): `parse_datetime`, then the `parse_date` fallback
/// (midnight); unparseable → 400 `valid detail`.
#[allow(clippy::result_large_err)]
fn model_expired_at(text: &str) -> Result<Option<DateTime<Utc>>, Response> {
    let invalid = || Denial::BadError("Please provide valid detail".to_owned()).into_response();
    match parse_django_datetime(text) {
        Some(ParsedInput::Aware(instant)) => Ok(Some(instant)),
        Some(ParsedInput::Naive(naive)) => Ok(Some(naive.and_utc())),
        None => match parse_django_date(text) {
            Some(date) => Ok(date.and_hms_opt(0, 0, 0).map(|midnight| midnight.and_utc())),
            None => Err(invalid()),
        },
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.as_ref() == "23505")
}

/// `patch` (`api.py:56-72`): scoped `.first()` (a miss is the bare
/// 404), partial validation (400 `serializer.errors` in wire order),
/// `serializer.save()` (200 create shape with the token).
async fn token_patch(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path(pk_raw): Path<String>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    let Some(pk) = parse_detail_pk(&pk_raw) else {
        return proxy_detail_request(state, method, uri, headers, body).await;
    };
    let user_id = match authed_gate("PATCH", "users/api-tokens/<pk>/", extension) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (zone, _, zone_name) = match actor_facts(&pool, &user_id).await {
        Ok(facts) => facts,
        Err(denial) => return denial.into_response(),
    };
    let lookup = bind_placeholders(queries_user::api_token_patch_lookup_sql());
    let row = match sqlx::query(&lookup)
        .bind(user_id)
        .bind(pk)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let row = match row {
        Some(row) => row,
        None => return empty_response(StatusCode::NOT_FOUND),
    };
    let mut token = match token_row_from(&row) {
        Ok(token) => token,
        Err(denial) => return denial.into_response(),
    };
    let data = match negotiate_data(&headers, &body, &TOKEN_PATCH_SPEC) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // Serializer input stage: objects pass, `null` is "No data
    // provided", every other JSON type names itself.
    let object = match &data.value {
        JVal::Object(object) => object,
        JVal::Null => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"non_field_errors":["No data provided"]}"#.to_owned(),
            );
        }
        other => {
            let kind = match other {
                JVal::Array(_) => "list",
                JVal::Str(_) => "str",
                JVal::Num(number) => {
                    if number.is_float() {
                        "float"
                    } else {
                        "int"
                    }
                }
                JVal::Bool(_) => "bool",
                JVal::Null | JVal::Object(_) => unreachable!("handled above"),
            };
            return json_response(
                StatusCode::BAD_REQUEST,
                format!(
                    "{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {kind}.\"]}}"
                ),
            );
        }
    };
    // A surrogate inside either FK echo breaks the errors render →
    // 500 ahead of every 400.
    for key in ["created_by", "updated_by"] {
        if let Some(DataValue::Json(value)) = data_get(&data, object, key) {
            let dirty = match value {
                JVal::Str(text) => text.has_surrogate(),
                JVal::Array(_) | JVal::Object(_) => py_repr(value).is_none(),
                _ => false,
            };
            if dirty {
                return Denial::ServerError.into_response();
            }
        }
    }
    // Validate every present field (DRF collects all errors).
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut dirty_save = false;
    let mut label = token.label.clone();
    let mut description = token.description.clone();
    let mut is_service = token.is_service;
    let mut allowed_rate_limit = token.allowed_rate_limit.clone();
    let mut deleted_at = token.deleted_at;
    let mut created_by = token.created_by;
    // File uploads validate as the upload OBJECT (unlike POST's
    // `str()` coercion): every field type rejects it with its own
    // `invalid` message — only char blank rules peek at the name.
    if let Some(input) = data_get(&data, object, "deleted_at") {
        match input {
            DataValue::File(_) => {
                errors.push(("deleted_at".to_owned(), datetime_invalid_message()));
            }
            DataValue::Json(value) => {
                match validate_datetime(value, &zone, &zone_name, data.is_html) {
                    Ok(instant) => deleted_at = instant,
                    Err(message) => errors.push(("deleted_at".to_owned(), message)),
                }
            }
        }
    }
    if let Some(input) = data_get(&data, object, "label") {
        match input {
            DataValue::File(name) => match char_file_outcome(false, name) {
                Ok(_) => unreachable!("label file never stores"),
                Err(message) => errors.push(("label".to_owned(), message)),
            },
            DataValue::Json(value) => match validate_char(value, false, Some(MAX_CHAR_LEN)) {
                Ok(CleanText::Clean(text)) => label = text,
                Ok(CleanText::Dirty) => dirty_save = true,
                Err(message) => errors.push(("label".to_owned(), message)),
            },
        }
    }
    if let Some(input) = data_get(&data, object, "description") {
        match input {
            DataValue::File(name) => match char_file_outcome(true, name) {
                Ok(text) => description = text,
                Err(message) => errors.push(("description".to_owned(), message)),
            },
            DataValue::Json(value) => match validate_char(value, true, None) {
                Ok(CleanText::Clean(text)) => description = text,
                Ok(CleanText::Dirty) => dirty_save = true,
                Err(message) => errors.push(("description".to_owned(), message)),
            },
        }
    }
    if let Some(input) = data_get(&data, object, "is_service") {
        match input {
            DataValue::File(_) => {
                errors.push((
                    "is_service".to_owned(),
                    "Must be a valid boolean.".to_owned(),
                ));
            }
            DataValue::Json(value) => match validate_bool(value) {
                Ok(flag) => is_service = flag,
                Err(message) => errors.push(("is_service".to_owned(), message)),
            },
        }
    }
    if let Some(input) = data_get(&data, object, "allowed_rate_limit") {
        match input {
            DataValue::File(name) => match char_file_outcome(false, name) {
                Ok(_) => unreachable!("rate file never stores"),
                Err(message) => errors.push(("allowed_rate_limit".to_owned(), message)),
            },
            DataValue::Json(value) => match validate_char(value, false, Some(MAX_CHAR_LEN)) {
                Ok(CleanText::Clean(text)) => allowed_rate_limit = text,
                Ok(CleanText::Dirty) => dirty_save = true,
                Err(message) => errors.push(("allowed_rate_limit".to_owned(), message)),
            },
        }
    }
    // The FK existence probes (`queryset.get`): a miss formats the RAW
    // input (braces/case/`urn:` preserved, ints as digits). Format-valid
    // FKs probe even when other fields already errored (DRF collects
    // every field's error before answering).
    let mut created_by_echo: Option<(String, uuid::Uuid)> = None;
    let mut updated_by_echo: Option<(String, uuid::Uuid)> = None;
    if let Some(input) = data_get(&data, object, "created_by") {
        match input {
            // The queryset lookup renders the upload as its name.
            DataValue::File(name) => {
                errors.push(("created_by".to_owned(), fk_curly_message(name)));
            }
            DataValue::Json(value) => match validate_fk(value) {
                Ok(parsed) => {
                    created_by = parsed.as_ref().map(|(_, id)| *id);
                    created_by_echo = parsed;
                }
                Err(FkError::Message(message)) => {
                    errors.push(("created_by".to_owned(), message));
                }
                Err(FkError::Dirty) => return Denial::ServerError.into_response(),
            },
        }
    }
    if let Some(input) = data_get(&data, object, "updated_by") {
        match input {
            DataValue::File(name) => {
                errors.push(("updated_by".to_owned(), fk_curly_message(name)));
            }
            DataValue::Json(value) => match validate_fk(value) {
                Ok(parsed) => updated_by_echo = parsed,
                Err(FkError::Message(message)) => {
                    errors.push(("updated_by".to_owned(), message));
                }
                Err(FkError::Dirty) => return Denial::ServerError.into_response(),
            },
        }
    }
    for (field, echo) in [
        ("created_by", &created_by_echo),
        ("updated_by", &updated_by_echo),
    ] {
        if let Some((raw, id)) = echo {
            match user_exists(&pool, id).await {
                Ok(true) => {}
                Ok(false) => errors.push((
                    field.to_owned(),
                    format!("Invalid pk \"{raw}\" - object does not exist."),
                )),
                Err(denial) => return denial.into_response(),
            }
        }
    }
    if !errors.is_empty() {
        return json_response(StatusCode::BAD_REQUEST, field_errors_body(&errors));
    }
    if dirty_save {
        return Denial::ServerError.into_response();
    }
    // `serializer.save()`: the builder's writable SET plus the
    // `BaseModel.save` stamps (`updated_at` always moves — even on a
    // no-op PATCH — and `updated_by` is the actor whatever the input
    // carried). The stamp rides a second statement: the services text
    // is owned, and the queries layer documents `updated_by` as
    // handler sequencing.
    let now = Utc::now();
    let save = bind_placeholders(queries_user::api_token_patch_save_sql());
    if sqlx::query(&save)
        .bind(&label)
        .bind(&description)
        .bind(is_service)
        .bind(&allowed_rate_limit)
        .bind(deleted_at)
        .bind(created_by)
        .bind(now)
        .bind(pk)
        .execute(&pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    if sqlx::query("UPDATE api_tokens SET updated_by_id = $1 WHERE id = $2")
        .bind(user_id)
        .bind(pk)
        .execute(&pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    token.label = label;
    token.description = description;
    token.is_service = is_service;
    token.allowed_rate_limit = allowed_rate_limit;
    token.deleted_at = deleted_at;
    token.created_by = created_by;
    token.updated_at = now;
    token.updated_by = Some(user_id);
    let rendered = render_token(&token, &zone);
    json_response(StatusCode::OK, create_body(&rendered, None))
}

/// `User.objects.get(pk=...)` existence half (the queryset uses the
/// default manager — no soft-delete filter).
async fn user_exists(pool: &sqlx::PgPool, id: &uuid::Uuid) -> Result<bool, Denial> {
    sqlx::query("SELECT 1 FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map(|row| row.is_some())
        .map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Timezone endpoint (`app/views/timezone/base.py:23-215`)
// ---------------------------------------------------------------------------

/// One `{"utc_offset","gmt_offset","value","label"}` row (the pipeline's
/// `offset` key is stripped before responding).
#[derive(Serialize)]
struct TimezoneEntry<'a> {
    utc_offset: String,
    gmt_offset: String,
    value: &'a str,
    label: &'a str,
}

#[derive(Serialize)]
struct TimezoneList<'a> {
    timezones: Vec<TimezoneEntry<'a>>,
}

/// `get` (`timezone/base.py:23-215`): `AllowAny` (never 401s, authed
/// callers bypass the throttle), `AuthenticationThrottle` for
/// anonymous, `cache_page(2h)` headers. The body is computed live per
/// request (the license read-only precedent) rather than cached.
/// Non-GET methods proxy (the `APIView` CSRF arm answers authed POST,
/// `405`s answer the rest).
async fn timezone_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    peer: axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let gate = &gates::gate_for("GET", "timezones/")
        .expect("timezone gate row")
        .gate;
    if !matches!(gate, gates::Gate::AllowAny) {
        return Denial::ServerError.into_response();
    }
    let authenticated = actor_user_id_opt(extension).is_some();
    if let Err(wait) = timezone_throttle(&state, &headers, Some(peer.0), authenticated).await {
        let mut response =
            json_response(StatusCode::TOO_MANY_REQUESTS, gates::throttle_denied_json());
        if let Some(secs) = wait {
            if secs > 0.0 {
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    axum::http::HeaderValue::from(secs.ceil() as u64),
                );
            }
        }
        return response;
    }
    let now = Utc::now();
    let entries = timezone_entries(now);
    let body = serde_json::to_string(&TimezoneList { timezones: entries })
        .expect("timezone list serializes");
    let mut response = json_response(StatusCode::OK, body);
    let expires = (now + chrono::TimeDelta::seconds(gates::CACHE_PAGE_TIMEOUT_SECS as i64))
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("max-age=7200"),
    );
    headers.insert(
        header::EXPIRES,
        axum::http::HeaderValue::from_str(&expires).expect("http date"),
    );
    response
}

/// `AuthenticationThrottle` (`authentication/rate_limit.py:38-64`):
/// anonymous-only, 30/minute by client IP over the shared redis
/// history. `Ok` admits; `Err` carries the DRF `Throttled.wait` for
/// `Retry-After` (`auth_exception_handler` rewrites the 429 body to
/// the 5900 shape but keeps DRF's header). No redis, no peer address,
/// or any redis failure admits (Django's fail-open cache behavior).
async fn timezone_throttle(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    authenticated: bool,
) -> Result<(), Option<f64>> {
    if authenticated {
        return Ok(());
    }
    let Some(redis) = state.redis() else {
        return Ok(());
    };
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let remote = peer.map(|peer| peer.ip().to_string());
    let Some(ident) = crate::license::handlers_auth_forms::client_ip(forwarded, remote.as_deref())
    else {
        return Ok(());
    };
    let spec = gates::AUTHENTICATION_THROTTLE;
    let key = pidash_services::auth_session::throttle_cache_key(spec.scope, &ident);
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let now = now_float();
    let decision = pidash_services::auth_session::allow_request(
        &history,
        spec.requests,
        spec.window_secs,
        now,
    );
    if !decision.allowed {
        return Err(pidash_services::auth_session::throttle_wait(
            &decision.history,
            spec.requests,
            spec.window_secs,
            now,
        ));
    }
    let raw = serde_json::to_string(&decision.history).unwrap_or_else(|_| "[]".to_owned());
    if redis.set_ex(&key, &raw, spec.window_secs).await.is_err() {
        return Ok(());
    }
    Ok(())
}

/// The `base.py:32-177` pipeline at one instant: per-zone `%z` offsets
/// from the naive-`now()`, floor-division display strings, sorted by
/// (`int(%z)`, `label`), offset stripped. Python's naive `now()` plus
/// per-zone `astimezone` is exactly `Utc::now` in each zone (the
/// server zone cancels out); `chrono-tz` 0.10.4 agrees with pytz
/// 2024.1 on all 113 zone ids at the pinned instant (unit test below).
fn timezone_entries(now: DateTime<Utc>) -> Vec<TimezoneEntry<'static>> {
    let mut rows: Vec<(i32, TimezoneEntry<'static>)> = Vec::with_capacity(TIMEZONE_TABLE.len());
    for (label, tzid) in TIMEZONE_TABLE {
        let Ok(zone) = tzid.parse::<Tz>() else {
            continue;
        };
        let offset_secs = now.with_timezone(&zone).offset().fix().local_minus_utc();
        let (utc_offset, gmt_offset) = floor_offset_strings(offset_secs);
        rows.push((
            zulu_int(offset_secs),
            TimezoneEntry {
                utc_offset,
                gmt_offset,
                value: tzid,
                label,
            },
        ));
    }
    rows.sort_by(|left, right| (left.0, left.1.label).cmp(&(right.0, right.1.label)));
    rows.into_iter().map(|(_, entry)| entry).collect()
}

/// `int(now.astimezone(tz).strftime("%z"))`: `±HHMM` as an integer
/// (`-0930` → `-930`).
fn zulu_int(offset_secs: i32) -> i32 {
    let sign = if offset_secs < 0 { -1 } else { 1 };
    let abs = offset_secs.abs();
    sign * ((abs / 3600) * 100 + (abs % 3600) / 60)
}

/// The `hours_offset`/`minutes_offset` floor-division strings
/// (`base.py:192-208`): `total // 3600` floors, so negative sub-hour
/// zones show the wrong hour; `abs(total % 3600) // 60` the minutes.
fn floor_offset_strings(offset_secs: i32) -> (String, String) {
    let hours = offset_secs.div_euclid(3600);
    let minutes = offset_secs.rem_euclid(3600) / 60;
    let sign = if hours >= 0 { '+' } else { '-' };
    let rendered = format!("{sign}{:02}:{:02}", hours.abs(), minutes);
    (format!("UTC{rendered}"), format!("GMT{rendered}"))
}

/// `(label, tz)` pairs transcribed verbatim from
/// `app/views/timezone/base.py:33-177` (duplicates kept: Caracas x2,
/// Lagos x2, Karachi x2, Kolkata x4).
const TIMEZONE_TABLE: [(&str, &str); 120] = [
    ("Midway Island", "Pacific/Midway"),
    ("American Samoa", "Pacific/Pago_Pago"),
    ("Hawaii", "Pacific/Honolulu"),
    ("Aleutian Islands", "America/Adak"),
    ("Marquesas Islands", "Pacific/Marquesas"),
    ("Alaska", "America/Anchorage"),
    ("Gambier Islands", "Pacific/Gambier"),
    ("Pacific Time (US and Canada)", "America/Los_Angeles"),
    ("Baja California", "America/Tijuana"),
    ("Mountain Time (US and Canada)", "America/Denver"),
    ("Arizona", "America/Phoenix"),
    ("Chihuahua, Mazatlan", "America/Chihuahua"),
    ("Central Time (US and Canada)", "America/Chicago"),
    ("Saskatchewan", "America/Regina"),
    ("Guadalajara, Mexico City, Monterrey", "America/Mexico_City"),
    ("Tegucigalpa, Honduras", "America/Tegucigalpa"),
    ("Costa Rica", "America/Costa_Rica"),
    ("Eastern Time (US and Canada)", "America/New_York"),
    ("Lima", "America/Lima"),
    ("Bogota", "America/Bogota"),
    ("Quito", "America/Guayaquil"),
    ("Chetumal", "America/Cancun"),
    ("Caracas (Old Venezuela Time)", "America/Caracas"),
    ("Atlantic Time (Canada)", "America/Halifax"),
    ("Caracas", "America/Caracas"),
    ("Santiago", "America/Santiago"),
    ("La Paz", "America/La_Paz"),
    ("Manaus", "America/Manaus"),
    ("Georgetown", "America/Guyana"),
    ("Bermuda", "Atlantic/Bermuda"),
    ("Newfoundland Time (Canada)", "America/St_Johns"),
    ("Buenos Aires", "America/Argentina/Buenos_Aires"),
    ("Brasilia", "America/Sao_Paulo"),
    ("Greenland", "America/Godthab"),
    ("Montevideo", "America/Montevideo"),
    ("Falkland Islands", "Atlantic/Stanley"),
    (
        "South Georgia and the South Sandwich Islands",
        "Atlantic/South_Georgia",
    ),
    ("Azores", "Atlantic/Azores"),
    ("Cape Verde Islands", "Atlantic/Cape_Verde"),
    ("Dublin", "Europe/Dublin"),
    ("Reykjavik", "Atlantic/Reykjavik"),
    ("Lisbon", "Europe/Lisbon"),
    ("Monrovia", "Africa/Monrovia"),
    ("Casablanca", "Africa/Casablanca"),
    (
        "Central European Time (Berlin, Rome, Paris)",
        "Europe/Paris",
    ),
    ("West Central Africa", "Africa/Lagos"),
    ("Algiers", "Africa/Algiers"),
    ("Lagos", "Africa/Lagos"),
    ("Tunis", "Africa/Tunis"),
    (
        "Eastern European Time (Cairo, Helsinki, Kyiv)",
        "Europe/Kyiv",
    ),
    ("Athens", "Europe/Athens"),
    ("Jerusalem", "Asia/Jerusalem"),
    ("Johannesburg", "Africa/Johannesburg"),
    ("Harare, Pretoria", "Africa/Harare"),
    ("Moscow Time", "Europe/Moscow"),
    ("Baghdad", "Asia/Baghdad"),
    ("Nairobi", "Africa/Nairobi"),
    ("Kuwait, Riyadh", "Asia/Riyadh"),
    ("Tehran", "Asia/Tehran"),
    ("Abu Dhabi", "Asia/Dubai"),
    ("Baku", "Asia/Baku"),
    ("Yerevan", "Asia/Yerevan"),
    ("Astrakhan", "Europe/Astrakhan"),
    ("Tbilisi", "Asia/Tbilisi"),
    ("Mauritius", "Indian/Mauritius"),
    ("Kabul", "Asia/Kabul"),
    ("Islamabad", "Asia/Karachi"),
    ("Karachi", "Asia/Karachi"),
    ("Tashkent", "Asia/Tashkent"),
    ("Yekaterinburg", "Asia/Yekaterinburg"),
    ("Maldives", "Indian/Maldives"),
    ("Chagos", "Indian/Chagos"),
    ("Chennai", "Asia/Kolkata"),
    ("Kolkata", "Asia/Kolkata"),
    ("Mumbai", "Asia/Kolkata"),
    ("New Delhi", "Asia/Kolkata"),
    ("Sri Jayawardenepura", "Asia/Colombo"),
    ("Kathmandu", "Asia/Kathmandu"),
    ("Dhaka", "Asia/Dhaka"),
    ("Almaty", "Asia/Almaty"),
    ("Bishkek", "Asia/Bishkek"),
    ("Thimphu", "Asia/Thimphu"),
    ("Yangon (Rangoon)", "Asia/Yangon"),
    ("Cocos Islands", "Indian/Cocos"),
    ("Bangkok", "Asia/Bangkok"),
    ("Hanoi", "Asia/Ho_Chi_Minh"),
    ("Jakarta", "Asia/Jakarta"),
    ("Novosibirsk", "Asia/Novosibirsk"),
    ("Krasnoyarsk", "Asia/Krasnoyarsk"),
    ("Beijing", "Asia/Shanghai"),
    ("Singapore", "Asia/Singapore"),
    ("Perth", "Australia/Perth"),
    ("Hong Kong", "Asia/Hong_Kong"),
    ("Ulaanbaatar", "Asia/Ulaanbaatar"),
    ("Palau", "Pacific/Palau"),
    ("Eucla", "Australia/Eucla"),
    ("Tokyo", "Asia/Tokyo"),
    ("Seoul", "Asia/Seoul"),
    ("Yakutsk", "Asia/Yakutsk"),
    ("Adelaide", "Australia/Adelaide"),
    ("Darwin", "Australia/Darwin"),
    ("Sydney", "Australia/Sydney"),
    ("Brisbane", "Australia/Brisbane"),
    ("Guam", "Pacific/Guam"),
    ("Vladivostok", "Asia/Vladivostok"),
    ("Tahiti", "Pacific/Tahiti"),
    ("Lord Howe Island", "Australia/Lord_Howe"),
    ("Solomon Islands", "Pacific/Guadalcanal"),
    ("Magadan", "Asia/Magadan"),
    ("Norfolk Island", "Pacific/Norfolk"),
    ("Bougainville Island", "Pacific/Bougainville"),
    ("Chokurdakh", "Asia/Srednekolymsk"),
    ("Auckland", "Pacific/Auckland"),
    ("Wellington", "Pacific/Auckland"),
    ("Fiji Islands", "Pacific/Fiji"),
    ("Anadyr", "Asia/Anadyr"),
    ("Chatham Islands", "Pacific/Chatham"),
    ("Nuku'alofa", "Pacific/Tongatapu"),
    ("Samoa", "Pacific/Apia"),
    ("Kiritimati Island", "Pacific/Kiritimati"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::json_cpython::JNum;

    fn jstr(text: &str) -> JVal {
        JVal::Str(JStr::from_clean(text.to_owned()))
    }

    fn jint(text: &str) -> JVal {
        JVal::Num(JNum::int(text.to_owned()))
    }

    fn jfloat(text: &str) -> JVal {
        JVal::Num(JNum::float(text.to_owned()))
    }

    // Generated from CPython 3.12 + Django 4.2 dateparse (PIDASHCONV-624 probe).
    // (input, parse_datetime instant or None, parse_date or None)
    const DATETIME_VECTORS: &[(&str, Option<&str>, Option<&str>)] = &[
        (
            "2030-01-01T00:00:00Z",
            Some("2030-01-01T00:00:00+00:00"),
            None,
        ),
        ("2030-06-01T12:00:00", Some("2030-06-01T12:00:00"), None),
        (
            "2030-06-01 12:00:00+00:00",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+05:30",
            Some("2030-06-01T06:30:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+0000",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01",
            Some("2030-06-01T00:00:00"),
            Some("2030-06-01"),
        ),
        (
            "2030-06-01T12:00:00.123456Z",
            Some("2030-06-01T12:00:00.123456+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00.123456789",
            Some("2030-06-01T12:00:00.123456"),
            None,
        ),
        (
            "2030-06-01T12:00:00,5",
            Some("2030-06-01T12:00:00.500000"),
            None,
        ),
        ("garbage", None, None),
        ("", None, None),
        ("2030-13-01", None, None),
        ("2030-02-30", None, None),
        ("2030-06-01T25:00:00", None, None),
        (
            "2030-06-01t12:00:00Z",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        ("2030-06-01T12:00:00z", None, None),
        ("20300601", Some("2030-06-01T00:00:00"), Some("2030-06-01")),
        ("20300601T120000", Some("2030-06-01T12:00:00"), None),
        ("2030-06-01T12", Some("2030-06-01T12:00:00"), None),
        ("2030-06-01T12:00", Some("2030-06-01T12:00:00"), None),
        ("2030-06-01 12:00", Some("2030-06-01T12:00:00"), None),
        ("12:00:00", None, None),
        (
            "2030-06-01T12:00:00+00:00:00",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        ("2030-06-01T12:00:00+24:00", None, None),
        (" 2030-06-01T12:00:00Z ", None, None),
        (
            "2030-06-01T12:00:00+05",
            Some("2030-06-01T07:00:00+00:00"),
            None,
        ),
        (
            "2030-W23-2",
            Some("2030-06-04T00:00:00"),
            Some("2030-06-04"),
        ),
        ("2030-153", None, None),
        ("2030-06-01T12:00:60Z", None, None),
        (
            "0001-01-01T00:00:00Z",
            Some("0001-01-01T00:00:00+00:00"),
            None,
        ),
        (
            "9999-12-31T23:59:59Z",
            Some("9999-12-31T23:59:59+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+05:30:15",
            Some("2030-06-01T06:29:45+00:00"),
            None,
        ),
        (
            "2030-06-01X12:00:00Z",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        ("2030-6-1T2:00:00Z", Some("2030-06-01T02:00:00+00:00"), None),
        ("30-06-01T12:00:00Z", None, None),
        ("2030/06/01", None, None),
        ("2030-6-1", None, Some("2030-06-01")),
        ("2030-6-1T2:00+99:99", None, None),
        (
            "2030-06-01T12:00:00+0530",
            Some("2030-06-01T06:30:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+053015",
            Some("2030-06-01T06:29:45+00:00"),
            None,
        ),
        ("2030-06-01T12:00:00+05:3015", None, None),
        ("2030-06-01T12:00:00+0530:15", None, None),
        (
            "2030-06-01T12:00:00+05:30:15,5",
            Some("2030-06-01T06:29:44.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+23:59:59.999999",
            Some("2030-05-31T12:00:00.000001+00:00"),
            None,
        ),
        ("2030-06-01T12Z", Some("2030-06-01T12:00:00+00:00"), None),
        (
            "2030-06-01T12+05:00",
            Some("2030-06-01T07:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+0530,5",
            Some("2030-06-01T06:29:59.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+05:30,5",
            Some("2030-06-01T06:29:59.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+05,5",
            Some("2030-06-01T06:59:59.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+00:60",
            Some("2030-06-01T11:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+00:00:60",
            Some("2030-06-01T11:59:00+00:00"),
            None,
        ),
        ("2030-06-01T12:00:00+99", None, None),
        ("2030-06-01T120060+05:00", None, None),
        (
            "2030-06-01T12:00,5+05:00",
            Some("2030-06-01T07:00:00.500000+00:00"),
            None,
        ),
        ("2030-W23-2T120000", Some("2030-06-04T12:00:00"), None),
        ("20300601T12", Some("2030-06-01T12:00:00"), None),
        ("2030-06-01T1200", Some("2030-06-01T12:00:00"), None),
        (
            "2030-06-01T120000.5",
            Some("2030-06-01T12:00:00.500000"),
            None,
        ),
        (
            "2029-W01-1",
            Some("2029-01-01T00:00:00"),
            Some("2029-01-01"),
        ),
        ("2030-W54-1", None, None),
        (
            "2024-02-29",
            Some("2024-02-29T00:00:00"),
            Some("2024-02-29"),
        ),
        (
            "2030-06-01T12:00:00+00:00:00.000001",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00.+05:00",
            Some("2030-06-01T07:00:00+00:00"),
            None,
        ),
        ("2030-06-01T", None, None),
        ("2030-06-01T12:00:00+", None, None),
        ("2030-06-01	12:00:00", Some("2030-06-01T12:00:00"), None),
        (
            "2030-06-01é12:00:00Z",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        // Deliberate: Django accepts Unicode decimal digits here (`\d`);
        // this port rejects them (PR note).
        ("２０３０-０６-０１", None, None),
        ("2030-6-1 2:3", Some("2030-06-01T02:03:00"), None),
        (
            "2030-6-1T2:3:4.567890123Z",
            Some("2030-06-01T02:03:04.567890+00:00"),
            None,
        ),
        (
            "2030-6-1T2:00 +05:00",
            Some("2030-05-31T21:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00.000000Z",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "1970-01-01T00:00:00Z",
            Some("1970-01-01T00:00:00+00:00"),
            None,
        ),
        (
            "1969-12-31T23:59:59Z",
            Some("1969-12-31T23:59:59+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00-00:00",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+00:99",
            Some("2030-06-01T10:21:00+00:00"),
            None,
        ),
        (
            "2030-6-1T2:00+00:99",
            Some("2030-06-01T00:21:00+00:00"),
            None,
        ),
        (
            "2030-06-01T00:00:00.000001Z",
            Some("2030-06-01T00:00:00.000001+00:00"),
            None,
        ),
        (
            "2030-06-01T00:00:00.0000001Z",
            Some("2030-06-01T00:00:00+00:00"),
            None,
        ),
        ("0000-06-01", None, None),
        ("0000-W01-1", None, None),
        ("00000101", None, None),
        ("0000-06-01T12:00:00Z", None, None),
        // Year 0, full battery (PIDASHCONV-768; same shapes as 762):
        // every path already guards `year == 0` (year is 4 digits, so
        // that is the whole 1..=9999 clamp) — these pins prove it.
        ("0000-06-01T12:00:00", None, None),
        ("0000-06-01 12:00:00", None, None),
        ("0000-06-01T12:00:00+00:00", None, None),
        ("0000-W01", None, None),
        ("0000W011", None, None),
        ("0000W01", None, None),
        ("0000-W01-1T12:00:00+00:00", None, None),
        ("0000-6-1T12:00:00", None, None),
        ("0000-6-1 12:00:00+05:00", None, None),
        (
            "2030-06-01T12:00:00+01:00:00.5",
            Some("2030-06-01T10:59:59.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00-00:00:00.5",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00+00:01:00.5",
            Some("2030-06-01T11:58:59.500000+00:00"),
            None,
        ),
        ("2030-06-01T12.5", Some("2030-06-01T12:00:00.500000"), None),
        (
            "2030-06-01T12.5Z",
            Some("2030-06-01T12:00:00.500000+00:00"),
            None,
        ),
        ("2030-06-01T12.", None, None),
        ("2030-06-01T12:00:00.", None, None),
        (
            "2030-06-01T12:00:00.Z",
            Some("2030-06-01T12:00:00+00:00"),
            None,
        ),
        (
            "2030-06-01T1200.5",
            Some("2030-06-01T12:00:00.500000"),
            None,
        ),
        (
            "2030-06-01T12:00.5",
            Some("2030-06-01T12:00:00.500000"),
            None,
        ),
        ("2030-06-01T12:00.", None, None),
        ("2030-06-01T12:00:00+05.", None, None),
        ("2030-06-01T12:00:00+05.Z", None, None),
        (
            "2030-06-01T120000.5+0530",
            Some("2030-06-01T06:30:00.500000+00:00"),
            None,
        ),
        (
            "2030-06-01T12:00:00.1234567Z",
            Some("2030-06-01T12:00:00.123456+00:00"),
            None,
        ),
    ];

    #[test]
    fn datetime_vectors_match_django() {
        use chrono::Timelike;
        for (input, expected_dt, expected_date) in DATETIME_VECTORS {
            let parsed = parse_django_datetime(input);
            // Python `isoformat` drops zero microseconds and aware
            // vectors were normalized to UTC (`+00:00`).
            let rendered = parsed.map(|value| match value {
                ParsedInput::Naive(naive) => {
                    if naive.nanosecond() == 0 {
                        naive.format("%Y-%m-%dT%H:%M:%S").to_string()
                    } else {
                        naive.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
                    }
                }
                ParsedInput::Aware(instant) => {
                    if instant.nanosecond() == 0 {
                        instant.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
                    } else {
                        instant.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
                    }
                }
            });
            assert_eq!(
                rendered.as_deref(),
                *expected_dt,
                "parse_datetime({input:?})"
            );
            let date = parse_django_date(input);
            let date_rendered = date.map(|day| day.format("%Y-%m-%d").to_string());
            assert_eq!(
                date_rendered.as_deref(),
                *expected_date,
                "parse_date({input:?})"
            );
        }
    }

    // pytz 2024.1 %z at 2026-10-03T15:13:00Z for all 113 unique zone ids.
    const FIXED_OFFSETS: &[(&str, &str)] = &[
        ("Africa/Algiers", "+0100"),
        ("Africa/Casablanca", "+0100"),
        ("Africa/Harare", "+0200"),
        ("Africa/Johannesburg", "+0200"),
        ("Africa/Lagos", "+0100"),
        ("Africa/Monrovia", "+0000"),
        ("Africa/Nairobi", "+0300"),
        ("Africa/Tunis", "+0100"),
        ("America/Adak", "-0900"),
        ("America/Anchorage", "-0800"),
        ("America/Argentina/Buenos_Aires", "-0300"),
        ("America/Bogota", "-0500"),
        ("America/Cancun", "-0500"),
        ("America/Caracas", "-0400"),
        ("America/Chicago", "-0500"),
        ("America/Chihuahua", "-0600"),
        ("America/Costa_Rica", "-0600"),
        ("America/Denver", "-0600"),
        ("America/Godthab", "-0100"),
        ("America/Guayaquil", "-0500"),
        ("America/Guyana", "-0400"),
        ("America/Halifax", "-0300"),
        ("America/La_Paz", "-0400"),
        ("America/Lima", "-0500"),
        ("America/Los_Angeles", "-0700"),
        ("America/Manaus", "-0400"),
        ("America/Mexico_City", "-0600"),
        ("America/Montevideo", "-0300"),
        ("America/New_York", "-0400"),
        ("America/Phoenix", "-0700"),
        ("America/Regina", "-0600"),
        ("America/Santiago", "-0300"),
        ("America/Sao_Paulo", "-0300"),
        ("America/St_Johns", "-0230"),
        ("America/Tegucigalpa", "-0600"),
        ("America/Tijuana", "-0700"),
        ("Asia/Almaty", "+0500"),
        ("Asia/Anadyr", "+1200"),
        ("Asia/Baghdad", "+0300"),
        ("Asia/Baku", "+0400"),
        ("Asia/Bangkok", "+0700"),
        ("Asia/Bishkek", "+0600"),
        ("Asia/Colombo", "+0530"),
        ("Asia/Dhaka", "+0600"),
        ("Asia/Dubai", "+0400"),
        ("Asia/Ho_Chi_Minh", "+0700"),
        ("Asia/Hong_Kong", "+0800"),
        ("Asia/Jakarta", "+0700"),
        ("Asia/Jerusalem", "+0300"),
        ("Asia/Kabul", "+0430"),
        ("Asia/Karachi", "+0500"),
        ("Asia/Kathmandu", "+0545"),
        ("Asia/Kolkata", "+0530"),
        ("Asia/Krasnoyarsk", "+0700"),
        ("Asia/Magadan", "+1100"),
        ("Asia/Novosibirsk", "+0700"),
        ("Asia/Riyadh", "+0300"),
        ("Asia/Seoul", "+0900"),
        ("Asia/Shanghai", "+0800"),
        ("Asia/Singapore", "+0800"),
        ("Asia/Srednekolymsk", "+1100"),
        ("Asia/Tashkent", "+0500"),
        ("Asia/Tbilisi", "+0400"),
        ("Asia/Tehran", "+0330"),
        ("Asia/Thimphu", "+0600"),
        ("Asia/Tokyo", "+0900"),
        ("Asia/Ulaanbaatar", "+0800"),
        ("Asia/Vladivostok", "+1000"),
        ("Asia/Yakutsk", "+0900"),
        ("Asia/Yangon", "+0630"),
        ("Asia/Yekaterinburg", "+0500"),
        ("Asia/Yerevan", "+0400"),
        ("Atlantic/Azores", "+0000"),
        ("Atlantic/Bermuda", "-0300"),
        ("Atlantic/Cape_Verde", "-0100"),
        ("Atlantic/Reykjavik", "+0000"),
        ("Atlantic/South_Georgia", "-0200"),
        ("Atlantic/Stanley", "-0300"),
        ("Australia/Adelaide", "+0930"),
        ("Australia/Brisbane", "+1000"),
        ("Australia/Darwin", "+0930"),
        ("Australia/Eucla", "+0845"),
        ("Australia/Lord_Howe", "+1030"),
        ("Australia/Perth", "+0800"),
        ("Australia/Sydney", "+1000"),
        ("Europe/Astrakhan", "+0400"),
        ("Europe/Athens", "+0300"),
        ("Europe/Dublin", "+0100"),
        ("Europe/Kyiv", "+0300"),
        ("Europe/Lisbon", "+0100"),
        ("Europe/Moscow", "+0300"),
        ("Europe/Paris", "+0200"),
        ("Indian/Chagos", "+0600"),
        ("Indian/Cocos", "+0630"),
        ("Indian/Maldives", "+0500"),
        ("Indian/Mauritius", "+0400"),
        ("Pacific/Apia", "+1300"),
        ("Pacific/Auckland", "+1300"),
        ("Pacific/Bougainville", "+1100"),
        ("Pacific/Chatham", "+1345"),
        ("Pacific/Fiji", "+1200"),
        ("Pacific/Gambier", "-0900"),
        ("Pacific/Guadalcanal", "+1100"),
        ("Pacific/Guam", "+1000"),
        ("Pacific/Honolulu", "-1000"),
        ("Pacific/Kiritimati", "+1400"),
        ("Pacific/Marquesas", "-0930"),
        ("Pacific/Midway", "-1100"),
        ("Pacific/Norfolk", "+1200"),
        ("Pacific/Pago_Pago", "-1100"),
        ("Pacific/Palau", "+0900"),
        ("Pacific/Tahiti", "-1000"),
        ("Pacific/Tongatapu", "+1300"),
    ];

    #[test]
    fn timezone_fixed_offsets_match_pytz() {
        let fixed = Utc.with_ymd_and_hms(2026, 10, 3, 15, 13, 0).unwrap();
        assert_eq!(TIMEZONE_TABLE.len(), 120);
        assert_eq!(FIXED_OFFSETS.len(), 113);
        for (tzid, expected) in FIXED_OFFSETS {
            let zone: Tz = tzid.parse().expect(tzid);
            let offset = fixed.with_timezone(&zone).offset().fix().local_minus_utc();
            let sign = if offset < 0 { '-' } else { '+' };
            let abs = offset.abs();
            let rendered = format!("{sign}{:02}{:02}", abs / 3600, (abs % 3600) / 60);
            assert_eq!(&rendered, expected, "{tzid}");
        }
    }

    #[test]
    fn timezone_table_all_parse_and_sort() {
        for (_, tzid) in TIMEZONE_TABLE {
            tzid.parse::<Tz>().expect(tzid);
        }
        let entries = timezone_entries(Utc::now());
        assert_eq!(entries.len(), 120);
        let keys: Vec<(i32, &str)> = entries
            .iter()
            .map(|entry| {
                let zone: Tz = entry.value.parse().unwrap();
                let offset = Utc::now()
                    .with_timezone(&zone)
                    .offset()
                    .fix()
                    .local_minus_utc();
                (zulu_int(offset), entry.label)
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "entries sorted by (offset, label)");
        // First row is a UTC-11 zone (sort head pins the key order).
        assert_eq!(entries[0].utc_offset, "UTC-11:00");
        // Offset key stripped: entries carry no raw offset field.
        let body = serde_json::to_string(&TimezoneList { timezones: entries }).expect("serializes");
        assert!(!body.contains("\"offset\""));
    }

    #[test]
    fn floor_offset_strings_vectors() {
        // (total seconds, UTC string) — floor-division bugs included.
        for (total, expected) in [
            (0, "UTC+00:00"),
            (3600, "UTC+01:00"),
            (-3600, "UTC-01:00"),
            (-39600, "UTC-11:00"),
            (-34200, "UTC-10:30"),
            (-9000, "UTC-03:30"),
            (19800, "UTC+05:30"),
            (20700, "UTC+05:45"),
            (31500, "UTC+08:45"),
            (49500, "UTC+13:45"),
            (50400, "UTC+14:00"),
            (-1, "UTC-01:59"),
        ] {
            assert_eq!(floor_offset_strings(total).0, expected, "{total}");
        }
        assert_eq!(zulu_int(-34200), -930);
        assert_eq!(zulu_int(20700), 545);
        assert_eq!(zulu_int(0), 0);
    }

    #[test]
    fn bool_vectors() {
        let truthy: Vec<JVal> = vec![
            JVal::Bool(true),
            jint("1"),
            jfloat("1.0"),
            jstr("1"),
            jstr("t"),
            jstr("T"),
            jstr("y"),
            jstr("Y"),
            jstr("yes"),
            jstr("Yes"),
            jstr("YES"),
            jstr("yEs"),
            jstr("true"),
            jstr("True"),
            jstr("TRUE"),
            jstr("tRUE"),
            jstr("TrUe"),
            jstr("on"),
            jstr("On"),
            jstr("ON"),
            jstr("oN"),
        ];
        for value in truthy {
            assert_eq!(validate_bool(&value), Ok(true), "{value:?}");
        }
        let falsy: Vec<JVal> = vec![
            JVal::Bool(false),
            jint("0"),
            jint("-0"),
            jfloat("0.0"),
            jstr("0"),
            jstr("f"),
            jstr("F"),
            jstr("n"),
            jstr("N"),
            jstr("no"),
            jstr("No"),
            jstr("NO"),
            jstr("nO"),
            jstr("false"),
            jstr("False"),
            jstr("FALSE"),
            jstr("fAlSe"),
            jstr("off"),
            jstr("Off"),
            jstr("OFF"),
            jstr("oFf"),
        ];
        for value in falsy {
            assert_eq!(validate_bool(&value), Ok(false), "{value:?}");
        }
        for value in [
            jint("2"),
            jint("-1"),
            jfloat("1.5"),
            jstr(""),
            jstr("junk"),
            jstr(" 1"),
            jstr("TRUEE"),
            JVal::Array(vec![]),
            JVal::Object(JObject::new()),
        ] {
            assert_eq!(
                validate_bool(&value),
                Err("Must be a valid boolean.".to_owned()),
                "{value:?}"
            );
        }
        assert_eq!(
            validate_bool(&JVal::Null),
            Err("This field may not be null.".to_owned())
        );
    }

    #[test]
    fn char_vectors() {
        // Trim before blank/length checks.
        assert!(matches!(
            validate_char(&jstr("  padded  "), false, Some(255)),
            Ok(CleanText::Clean(text)) if text == "padded"
        ));
        assert_eq!(
            validate_char(&jstr("   "), false, Some(255)),
            Err("This field may not be blank.".to_owned())
        );
        assert!(matches!(
            validate_char(&jstr("   "), true, None),
            Ok(CleanText::Clean(text)) if text.is_empty()
        ));
        assert_eq!(
            validate_char(&jstr(""), false, Some(255)),
            Err("This field may not be blank.".to_owned())
        );
        assert!(matches!(
            validate_char(&jstr(""), true, None),
            Ok(CleanText::Clean(_))
        ));
        assert_eq!(
            validate_char(&jstr(&"y".repeat(256)), false, Some(255)),
            Err("Ensure this field has no more than 255 characters.".to_owned())
        );
        assert!(validate_char(&jstr(&"y".repeat(255)), false, Some(255)).is_ok());
        // Description has no length cap.
        assert!(validate_char(&jstr(&"y".repeat(100_000)), true, None).is_ok());
        // Coercions and rejections.
        assert!(matches!(
            validate_char(&jint("123"), false, Some(255)),
            Ok(CleanText::Clean(text)) if text == "123"
        ));
        assert!(matches!(
            validate_char(&jfloat("1.5"), false, Some(255)),
            Ok(CleanText::Clean(text)) if text == "1.5"
        ));
        for value in [
            JVal::Bool(true),
            JVal::Array(vec![]),
            JVal::Object(JObject::new()),
        ] {
            assert_eq!(
                validate_char(&value, false, Some(255)),
                Err("Not a valid string.".to_owned()),
                "{value:?}"
            );
        }
        assert_eq!(
            validate_char(&JVal::Null, false, Some(255)),
            Err("This field may not be null.".to_owned())
        );
        // NUL poisons the save, not validation.
        assert!(matches!(
            validate_char(&jstr("a\0b"), false, Some(255)),
            Ok(CleanText::Dirty)
        ));
    }

    #[test]
    fn fk_vectors() {
        let id = "8a206f51-243d-4442-9503-ac289d116d0d";
        // `""` and null become `None`.
        assert_eq!(validate_fk(&jstr("")).unwrap(), None);
        assert_eq!(validate_fk(&JVal::Null).unwrap(), None);
        // Verbatim forms parse.
        for raw in [
            id,
            &id.to_uppercase(),
            &format!("{{{id}}}"),
            &format!("urn:uuid:{id}"),
            &id.replace('-', ""),
        ] {
            let parsed = validate_fk(&jstr(raw)).unwrap().expect(raw);
            assert_eq!(parsed.0, raw);
            assert_eq!(parsed.1.to_string(), id);
        }
        for raw in ["garbage", "12345", " 8a206f51-243d-4442-9503-ac289d116d0d "] {
            assert_eq!(
                validate_fk(&jstr(raw)).unwrap_err(),
                FkError::Message(format!("\u{201c}{raw}\u{201d} is not a valid UUID.")),
                "{raw:?}"
            );
        }
        // `int` goes through the queryset (`UUID(int=...)`).
        let (echo, parsed) = validate_fk(&jint("5")).unwrap().expect("int");
        assert_eq!(echo, "5");
        assert_eq!(parsed, uuid::Uuid::from_u128(5));
        // `-0` is int `0` (probes; the echo is the int, `"0"`).
        let (echo, parsed) = validate_fk(&jint("-0")).unwrap().expect("neg zero");
        assert_eq!(echo, "0");
        assert_eq!(parsed, uuid::Uuid::nil());
        assert!(matches!(
            validate_fk(&jint("-5")).unwrap_err(),
            FkError::Message(_)
        ));
        assert!(matches!(
            validate_fk(&jfloat("1.5")).unwrap_err(),
            FkError::Message(message) if message.contains("1.5")
        ));
        assert!(matches!(
            validate_fk(&JVal::Bool(true)).unwrap_err(),
            FkError::Message(message) if message.contains("received bool")
        ));
    }

    #[test]
    fn py_repr_vectors() {
        let mut object = JObject::new();
        object.insert(JStr::from_clean("a".to_owned()), jint("1"));
        for (value, expected) in [
            (jint("7"), "7"),
            (jfloat("1.5"), "1.5"),
            (jfloat("100.0"), "100.0"),
            (JVal::Bool(true), "True"),
            (JVal::Bool(false), "False"),
            (JVal::Array(vec![jstr("a")]), "['a']"),
            (JVal::Array(vec![jstr("a"), jint("1")]), "['a', 1]"),
            (JVal::Object(object), "{'a': 1}"),
            (jstr("plain"), "'plain'"),
            (jstr("has 'single"), "\"has 'single\""),
            (jstr("both ' and \""), "'both \\' and \"'"),
            (jstr("tab\there"), "'tab\\there'"),
            (jstr("line\nbreak"), "'line\\nbreak'"),
            (jstr("back\\slash"), "'back\\\\slash'"),
            (jstr("nul\0byte"), "'nul\\x00byte'"),
            (jstr("héllo中🎉"), "'héllo中🎉'"),
        ] {
            assert_eq!(py_repr(&value).as_deref(), Some(expected), "{value:?}");
        }
        for (value, expected) in [
            (jint("7"), "7"),
            (jfloat("100.0"), "100.0"),
            (JVal::Bool(true), "True"),
            (jstr("both ' and \""), "both ' and \""),
            (jstr("nul\0byte"), "nul\0byte"),
        ] {
            assert_eq!(py_str(&value).as_deref(), Some(expected), "{value:?}");
        }
        assert_eq!(py_str(&JVal::Null), None);
    }

    #[test]
    fn bind_placeholders_vectors() {
        assert_eq!(
            bind_placeholders(queries_user::api_token_insert_sql()),
            "INSERT INTO api_tokens (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, label, description, is_active, last_used, token, user_id, user_type, workspace_id, expired_at, is_service, allowed_rate_limit) VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, TRUE, NULL, $7, $8, $9, NULL, $10, FALSE, '60/min')"
        );
        assert_eq!(
            bind_placeholders(queries_user::api_token_soft_delete_sql()),
            "UPDATE api_tokens SET deleted_at = $1, updated_at = $2 WHERE id = $3"
        );
        assert_eq!(
            bind_placeholders(queries_user::api_token_patch_save_sql()),
            "UPDATE api_tokens SET label = $1, description = $2, is_service = $3, allowed_rate_limit = $4, deleted_at = $5, created_by_id = $6, updated_at = $7 WHERE id = $8"
        );
    }

    #[test]
    fn field_errors_wire_order() {
        let errors = vec![
            ("created_by".to_owned(), "c".to_owned()),
            ("label".to_owned(), "l".to_owned()),
            ("deleted_at".to_owned(), "d".to_owned()),
            ("is_service".to_owned(), "s".to_owned()),
            ("description".to_owned(), "e".to_owned()),
        ];
        assert_eq!(
            field_errors_body(&errors),
            r#"{"deleted_at":["d"],"label":["l"],"description":["e"],"is_service":["s"],"created_by":["c"]}"#
        );
    }

    #[test]
    fn python_uuid_vectors() {
        let id = "8a206f51-243d-4442-9503-ac289d116d0d";
        assert!(parse_python_uuid(id).is_some());
        assert!(parse_python_uuid(&id.to_uppercase()).is_some());
        assert!(parse_python_uuid(&format!("{{{id}}}")).is_some());
        assert!(parse_python_uuid(&format!("urn:uuid:{id}")).is_some());
        assert!(parse_python_uuid(&id.replace('-', "")).is_some());
        for raw in [
            "",
            "garbage",
            "12345",
            " 8a206f51-243d-4442-9503-ac289d116d0d",
            "URN:UUID:8a206f51-243d-4442-9503-ac289d116d0d",
            "{8a206f51-243d-4442-9503-ac289d116d0d",
            "8a206f51-243d-4442-9503-ac289d116d0dg",
        ] {
            assert_eq!(parse_python_uuid(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn detail_pk_matches_uuid_converter() {
        let id = "8a206f51-243d-4442-9503-ac289d116d0d";
        assert_eq!(
            parse_detail_pk(id).as_ref().map(ToString::to_string),
            Some(id.to_owned())
        );
        // Every other `Uuid::parse` form misses the converter (resolver
        // 404) and must proxy.
        for raw in [
            "8A206F51-243D-4442-9503-AC289D116D0D",
            "8a206f51243d44429503ac289d116d0d",
            "{8a206f51-243d-4442-9503-ac289d116d0d}",
            "urn:uuid:8a206f51-243d-4442-9503-ac289d116d0d",
            "8a206f51_243d_4442_9503_ac289d116d0d",
            "not-a-uuid",
            "",
        ] {
            assert_eq!(parse_detail_pk(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn patch_file_outcome_vectors() {
        // Uploads validate as the object, never the filename.
        assert_eq!(
            char_file_outcome(false, "hello.txt"),
            Err("Not a valid string.".to_owned())
        );
        assert_eq!(
            char_file_outcome(false, "  "),
            Err("This field may not be blank.".to_owned())
        );
        assert_eq!(
            char_file_outcome(true, "d.txt"),
            Err("Not a valid string.".to_owned())
        );
        assert_eq!(char_file_outcome(true, "  "), Ok(String::new()));
        assert_eq!(
            fk_curly_message("nope.txt"),
            "\u{201c}nope.txt\u{201d} is not a valid UUID."
        );
        assert!(datetime_invalid_message().starts_with("Datetime has wrong format."));
    }

    #[test]
    fn datetime_overflow_vectors() {
        let kiritimati: Tz = "Pacific/Kiritimati".parse().unwrap();
        let utc: Tz = "UTC".parse().unwrap();
        // Renders past year 9999 in +14 → `overflow`.
        assert_eq!(
            validate_datetime(
                &jstr("9999-12-31T23:30:00Z"),
                &kiritimati,
                "Pacific/Kiritimati",
                false
            ),
            Err("Datetime value out of range.".to_owned())
        );
        // The same instant is fine in UTC.
        assert!(validate_datetime(&jstr("9999-12-31T23:30:00Z"), &utc, "UTC", false).is_ok());
        // Naive input never overflows (zone attach is infallible).
        assert!(validate_datetime(
            &jstr("9999-12-31T23:30:00"),
            &kiritimati,
            "Pacific/Kiritimati",
            false
        )
        .is_ok());
    }
}
