#![forbid(unsafe_code)]

//! Instance-admin loop job handlers (D-03, PIDASHCONV-161).
//!
//! Ports `apps/api/pi_dash/loop/admin_views.py:108-234`
//! (`LoopJobListCreateEndpoint`, `LoopJobDetailEndpoint`,
//! `LoopJobTargetsEndpoint`; routes `loop/admin_urls.py:13-18`, mounted
//! under `/api/instances/loop/`):
//!
//! - `GET jobs/` — non-deleted jobs, `-created_at` order, full payloads.
//! - `POST jobs/` — [`guards::validate_writes`] (`partial=false`),
//!   `slug_taken` 409, `dtstart` defaults to now, `is_builtin=False`, 201.
//! - `GET jobs/<uuid>/` — 404 `not_found` when missing/deleted, with the
//!   24h `stats` rollup appended.
//! - `PATCH jobs/<uuid>/` — [`guards::validate_writes`] (`partial=true`),
//!   rename-conflict 409, save, full payload.
//! - `DELETE jobs/<uuid>/` — stamp the job, its targets and its user
//!   preferences with one timestamp in a single transaction, 204 empty.
//! - `GET jobs/<uuid>/targets/` — 404 when the job is missing;
//!   `skip_reason`/`workspace`/`status` filters; `page`/`per=50` envelope
//!   `{"page","results"}` with the `_row` shape.
//!
//! Layering: validation and error bodies come from [`guards`], the 24h
//! stats statement and the targets filter/page helpers from
//! `pidash_db::loop::queries`, and the 14-key admin shape from
//! `pidash_services::loop::shape`. This module owns the HTTP shell (auth,
//! the `InstanceAdminPermission` gate, write SQL, row fetching, response
//! rendering).
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * Explicit-string `dtstart` on POST/PATCH commits the row and then 500s:
//!   the raw string stays on the in-memory attribute, so
//!   `job.dtstart.isoformat()` raises `AttributeError` into the generic
//!   500 branch (`admin_views.py:54`). [`RenderFail::StringDtstart`]
//!   carries that path; the write is committed first, exactly as Python.
//! * `min_role` / `enabled` echo the raw cleaned values in POST/PATCH
//!   responses (`15.5` stays `15.5`, `"15"` stays `"15"`, `1` stays `1`)
//!   while the stored column holds the coerced value; a later GET reads
//!   the stored integer/boolean back.
//! * `enabled` / `dtstart` / `tzid` pass validation untouched
//!   (`{"enabled": "x"}` validates clean); the model layer then answers
//!   400 `{"error":"Please provide valid detail"}` for values
//!   `BooleanField`/`DateTimeField` reject, and 400
//!   `{"error":"The payload is not valid"}` for `NULL` into a NOT NULL
//!   column (`IntegrityError` branch of `BaseAPIView.handle_exception`).
//! * BUG-LOOP-1 (non-numeric `min_role` → generic 500) arrives as
//!   [`guards::ValidateFail::ServerError`] and is answered with the
//!   generic 500 body here.
//! * Datetimes render with Python `isoformat()` (`+00:00`, microseconds
//!   only when nonzero) — not the DRF `Z` kernel — because `_job_payload`
//!   and `_row` call `.isoformat()` directly (`:54,57-58,220-221,229`).
//! * A non-UUID `pk` never reaches a Django view (the `<uuid:pk>`
//!   converter 404s), so it is proxied for Django's own 404.
//! * Unknown `skip_reason`/`status` filter values are ignored (an unknown
//!   workspace slug matches nothing); a bad `page` falls back to 1.

use std::collections::HashMap;

use axum::extract::{Extension, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QueryResult, QuerySelect, TryGetable, UpdateMany,
};
use sea_query::{
    Alias, Condition, Expr, Func, InsertStatement, JoinType, Order, Query, ReturningClause,
    SelectStatement,
};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{
    instance_admins, instances, loop_jobs, loop_targets, loop_user_preferences,
};
use pidash_db::orm_tx::into_sqlx;
use pidash_db::r#loop::queries;
use pidash_services::r#loop::shape::{job_payload, AdminJobRow};

use crate::app_issues::{INVALID_DETAIL_BODY, SERVER_ERROR_BODY, UNAUTHENTICATED_BODY};
use crate::edge;
use crate::middleware::SessionHandle;
use crate::permissions::DEFAULT_DENIED_BODY;
use crate::state::AppState;
use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};

use super::guards::{self, ValidateFail};
use pidash_compat::response::json_value as json_response;

/// Register the three owned admin paths. Cutover granularity is the
/// route + method (pilot `owned()` pattern): sibling methods keep
/// proxying so Django's own 405s stay byte-for-byte.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/loop/jobs/",
            get(list_jobs)
                .post(create_job)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/loop/jobs/{pk}/",
            get(detail_job)
                .patch(patch_job)
                .delete(delete_job)
                .post(edge::proxy)
                .put(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            "/api/instances/loop/jobs/{pk}/targets/",
            get(job_targets)
                .post(edge::proxy)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (session missing, dangling, or the app
    /// cookie presented on an `instances` path).
    Unauthorized,
    /// 403, DRF-default permission denial (`InstanceAdminPermission` has
    /// no `message`).
    DefaultForbidden,
    /// 404 `{"error":"not_found"}` (unknown or soft-deleted job).
    NotFound,
    /// A [`guards`] rejection: 400/409 with the exact `{"error": …}` body.
    Guard(StatusCode, Value),
    /// 400, `IntegrityError` branch (`{"error":"The payload is not valid"}`).
    InvalidPayload,
    /// 400, `ValidationError` branch
    /// (`{"error":"Please provide valid detail"}`).
    InvalidDetail,
    /// 400, DRF `ParseError` (`{"detail": …}`, malformed JSON body).
    BadDetail(String),
    /// 500, generic branch (BUG-LOOP-1, the string-`dtstart` render crash,
    /// unexpected DB errors).
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::DefaultForbidden => (StatusCode::FORBIDDEN, DEFAULT_DENIED_BODY.to_owned()),
            Denial::NotFound => (
                StatusCode::NOT_FOUND,
                guards::LOOP_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::Guard(status, body) => (*status, body.to_string()),
            Denial::InvalidPayload => (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()),
            Denial::InvalidDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

/// `BaseAPIView.handle_exception`'s `IntegrityError` branch.
const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

impl From<ValidateFail> for Denial {
    fn from(fail: ValidateFail) -> Self {
        match fail {
            ValidateFail::Reject(reject) => Denial::Guard(reject.status, reject.body),
            ValidateFail::ServerError => Denial::ServerError,
        }
    }
}

// ---------------------------------------------------------------------------
// auth (InstanceAdminPermission; same session semantics as loop/user.rs)
// ---------------------------------------------------------------------------

/// Authenticated actor or 401, with full DRF session semantics. Delegates to
/// [`crate::license::resolve_actor`] (the shared license plumbing), which
/// mirrors `django.contrib.auth.get_user` + DRF `SessionAuthentication`:
/// the session must carry a UUID `_auth_user_id` for the `ModelBackend`
/// with a matching `_auth_user_hash`, and the user row must exist and be
/// active. Anything else is anonymous (401), not 403 — and on `instances`
/// paths the middleware reads the `admin-session-id` cookie, so an
/// app-cookie caller is anonymous here (pinned by
/// `test_app_cookie_is_anonymous_on_admin_routes`).
async fn authed_user(
    state: &AppState,
    pool: &PgPool,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Uuid, Denial> {
    // `resolve_actor` only ever errs with the license-layer 500, so the
    // mapping is exact (mirrors `loop/user.rs`'s `actor`).
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor.id),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// `Instance.objects.first()`: `Meta.ordering = ("-created_at",)` over the
/// soft-deletion manager.
async fn instance_first(pool: &PgPool) -> Result<Option<Uuid>, Denial> {
    let db = pidash_db::sea_orm_connection(pool);
    let row: Option<instances::Model> = instances::Entity::find()
        .filter(instances::Column::DeletedAt.is_null())
        .order_by_desc(instances::Column::CreatedAt)
        .limit(1)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.id))
}

/// `InstanceAdminPermission.has_permission`
/// (`license/api/permissions/instance.py:12-18`): anonymous is denied by
/// the caller (401); otherwise the instance's first row plus an
/// `InstanceAdmin` with `role__gte=15` for `(instance, user)` must exist,
/// else the DRF-default 403.
async fn require_instance_admin(pool: &PgPool, user_id: &Uuid) -> Result<Uuid, Denial> {
    let Some(instance_id) = instance_first(pool).await? else {
        return Err(Denial::DefaultForbidden);
    };
    let db = pidash_db::sea_orm_connection(pool);
    let found = instance_admins::Entity::find()
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .filter(instance_admins::Column::UserId.eq(*user_id))
        .filter(instance_admins::Column::Role.gte(15))
        .filter(instance_admins::Column::DeletedAt.is_null())
        .exists(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if !found {
        return Err(Denial::DefaultForbidden);
    }
    Ok(instance_id)
}

fn pools(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// request parsing (DRF shapes)
// ---------------------------------------------------------------------------

/// Map a body-layer failure onto this module's denials.
fn map_body_error(error: BodyError) -> Denial {
    match error {
        BodyError::UnsupportedMediaType(message) => Denial::Guard(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            json!({ "detail": message }),
        ),
        BodyError::ParseDetail(message) => Denial::BadDetail(message),
        BodyError::ServerError => Denial::ServerError,
    }
}

/// Read a POST/PATCH body through DRF's `Request._parse` pipeline
/// (`negotiate_body`): Content-Length 0 is `{}` without parsing, a missing
/// or unsupported Content-Type is the 415 `UnsupportedMediaType` detail,
/// form/multipart input arrives as its text map, and JSON is decoded for
/// the existing `JSONParser`-shaped error path. Unlike the license console
/// (where a non-object body 500s), the loop admin views run
/// `request.data if isinstance(request.data, dict) else {}`, so any
/// well-formed non-object JSON counts as `{}`.
fn parse_json_body(headers: &HeaderMap, body: &[u8]) -> Result<Value, Denial> {
    match negotiate_body(headers, body, &super::LOOP_BODY_SPEC).map_err(map_body_error)? {
        NegotiatedBody::Empty => Ok(Value::Object(Map::new())),
        NegotiatedBody::JsonText { text, .. } => match serde_json::from_str::<Value>(&text) {
            Ok(data) => Ok(data),
            Err(_) if text.trim().is_empty() => {
                let len = text.len();
                Err(Denial::BadDetail(format!(
                    "JSON parse error - Expecting value: line 1 column {} (char {})",
                    len + 1,
                    len
                )))
            }
            Err(_) => Err(Denial::BadDetail("JSON parse error.".to_owned())),
        },
        NegotiatedBody::Form { map, .. } => Ok(Value::Object(map)),
    }
}

/// Outcome of the `BooleanField` write coercion (`to_python`): accepted
/// values store the bool, `None` stores `NULL` (the NOT NULL column then
/// raises the `IntegrityError` branch), anything else is the
/// `ValidationError` branch. Membership follows CPython `in` semantics:
/// `1`/`1.0` equal `True`, `0`/`0.0` equal `False`.
enum BoolPrep {
    Value(bool),
    Null,
}

impl BoolPrep {
    #[cfg(test)]
    fn into_bool(self) -> bool {
        match self {
            BoolPrep::Value(b) => b,
            BoolPrep::Null => panic!("expected bool, got null"),
        }
    }
}

fn bool_prep(value: &Value) -> Result<BoolPrep, Denial> {
    match value {
        Value::Null => Ok(BoolPrep::Null),
        Value::Bool(b) => Ok(BoolPrep::Value(*b)),
        Value::Number(n) => {
            if n.as_i64() == Some(1) || n.as_f64() == Some(1.0) {
                Ok(BoolPrep::Value(true))
            } else if n.as_i64() == Some(0) || n.as_f64() == Some(0.0) {
                Ok(BoolPrep::Value(false))
            } else {
                Err(Denial::InvalidDetail)
            }
        }
        Value::String(s) => match s.as_str() {
            "t" | "True" | "1" => Ok(BoolPrep::Value(true)),
            "f" | "False" | "0" => Ok(BoolPrep::Value(false)),
            _ => Err(Denial::InvalidDetail),
        },
        Value::Array(_) | Value::Object(_) => Err(Denial::InvalidDetail),
    }
}

/// Parse a `dtstart` string the way `DateTimeField.get_prep_value` does:
/// RFC 3339 with any offset, or Django's `parse_datetime` shape
/// (`YYYY-MM-DD[ T]HH:MM[:ss[.ffffff]][tz]`); naive values are UTC.
/// Anything else is the `ValidationError` branch.
fn parse_dtstart_text(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    let (date_part, time_part) = raw.split_once(['T', ' ']).unwrap_or((raw, ""));
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    if time_part.is_empty() {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }
    // Split an optional trailing zone (`Z` or `±HH:?MM?`); the wall time
    // itself parses as naive, then shifts by the zone offset.
    let (wall, offset_mins) = split_zone_suffix(time_part)?;
    let mut wall_parts = wall.split('.');
    let hms = wall_parts.next().unwrap_or("");
    let frac = wall_parts.next().unwrap_or("");
    if wall_parts.next().is_some() {
        return None;
    }
    let mut hms_parts = hms.split(':');
    let (hour, min, sec) = match (hms_parts.next(), hms_parts.next(), hms_parts.next()) {
        (Some(h), Some(m), Some(s)) => (h, m, s),
        (Some(h), Some(m), None) => (h, m, "00"),
        _ => return None,
    };
    if hms_parts.next().is_some() {
        return None;
    }
    let nanos = match frac.len() {
        0 => 0,
        1..=9 => {
            let mut padded = frac.to_owned();
            while padded.len() < 9 {
                padded.push('0');
            }
            padded.parse::<u32>().ok()?
        }
        _ => return None,
    };
    let naive = NaiveDateTime::new(
        date,
        chrono::NaiveTime::from_hms_nano_opt(
            hour.parse().ok()?,
            min.parse().ok()?,
            sec.parse().ok()?,
            nanos,
        )?,
    );
    naive
        .and_utc()
        .checked_sub_signed(chrono::Duration::minutes(offset_mins))
}

/// Split a trailing `Z` / `±HH:MM` / `±HHMM` / `±HH` zone suffix off a
/// time part, returning the wall time and the offset in minutes.
fn split_zone_suffix(time_part: &str) -> Option<(&str, i64)> {
    if let Some(wall) = time_part.strip_suffix(['Z', 'z']) {
        return Some((wall, 0));
    }
    let idx = time_part.rfind(['+', '-'])?;
    let (wall, zone) = time_part.split_at(idx);
    if wall.is_empty() {
        return None;
    }
    let (sign, digits) = match zone.split_at(1) {
        ("+", rest) => (1i64, rest),
        ("-", rest) => (-1i64, rest),
        _ => return None,
    };
    let digits: String = digits.chars().filter(|c| *c != ':').collect();
    let offset_mins = match digits.len() {
        2 => digits.parse::<i64>().ok()? * 60,
        4 => digits[..2].parse::<i64>().ok()? * 60 + digits[2..].parse::<i64>().ok()?,
        _ => return None,
    };
    Some((wall, sign * offset_mins))
}

/// Python `datetime.isoformat()` for a UTC instant: `+00:00` suffix (never
/// `Z`), microseconds only when nonzero (`admin_views.py:54,57-58`).
fn iso(dt: &DateTime<Utc>) -> String {
    let format = if dt.timestamp_subsec_micros() == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    dt.to_rfc3339_opts(format, false)
}

fn iso_or_null(dt: Option<&DateTime<Utc>>) -> Value {
    match dt {
        Some(dt) => Value::String(iso(dt)),
        None => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// rows
// ---------------------------------------------------------------------------

/// One `loop_jobs` row projected for the admin payload.
#[derive(Debug, Clone)]
struct JobRow {
    id: Uuid,
    slug: String,
    name: String,
    public_name: String,
    public_description: String,
    prompt: String,
    min_role: i16,
    enabled: bool,
    is_builtin: bool,
    dtstart: DateTime<Utc>,
    rrule: String,
    tzid: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn map_job_row(model: loop_jobs::Model) -> JobRow {
    JobRow {
        id: model.id,
        slug: model.slug,
        name: model.name,
        public_name: model.public_name,
        public_description: model.public_description,
        prompt: model.prompt,
        min_role: model.min_role,
        enabled: model.enabled,
        is_builtin: model.is_builtin,
        dtstart: model.dtstart.with_timezone(&Utc),
        rrule: model.rrule,
        tzid: model.tzid,
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
    }
}

/// Decode one `JobRow`-projected row off a SeaORM `QueryResult` (column
/// names, order-independent).
fn map_job_row_query(row: &QueryResult) -> Result<JobRow, sqlx::Error> {
    fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T, sqlx::Error> {
        row.try_get::<T>("", column).map_err(into_sqlx)
    }
    Ok(JobRow {
        id: get(row, "id")?,
        slug: get(row, "slug")?,
        name: get(row, "name")?,
        public_name: get(row, "public_name")?,
        public_description: get(row, "public_description")?,
        prompt: get(row, "prompt")?,
        min_role: get(row, "min_role")?,
        enabled: get(row, "enabled")?,
        is_builtin: get(row, "is_builtin")?,
        dtstart: get(row, "dtstart")?,
        rrule: get(row, "rrule")?,
        tzid: get(row, "tzid")?,
        created_at: get(row, "created_at")?,
        updated_at: get(row, "updated_at")?,
    })
}

/// The `POST` job insert: explicit column list, `gen_random_uuid()`
/// id, `is_builtin` FALSE, `updated_by_id`/`deleted_at` NULL, and the
/// 14-column `RETURNING` the payload projects. Explicit nulls stay
/// `NULL` binds (the 23502 `IntegrityError` 400), never defaults.
#[allow(clippy::too_many_arguments)]
fn job_insert(
    slug: String,
    name: Option<String>,
    public_name: Option<String>,
    public_description: Option<String>,
    prompt: Option<String>,
    min_role: i16,
    enabled: Option<bool>,
    dtstart: Option<DateTime<Utc>>,
    rrule: Option<String>,
    tzid: Option<String>,
    now: DateTime<Utc>,
    actor: Uuid,
) -> InsertStatement {
    use loop_jobs::Column as C;
    Query::insert()
        .into_table(loop_jobs::Entity)
        .columns([
            C::Id,
            C::Slug,
            C::Name,
            C::PublicName,
            C::PublicDescription,
            C::Prompt,
            C::MinRole,
            C::Enabled,
            C::IsBuiltin,
            C::Dtstart,
            C::Rrule,
            C::Tzid,
            C::CreatedAt,
            C::UpdatedAt,
            C::CreatedById,
            C::UpdatedById,
            C::DeletedAt,
        ])
        .values_panic([
            Func::cust(Alias::new("gen_random_uuid")).into(),
            slug.into(),
            name.into(),
            public_name.into(),
            public_description.into(),
            prompt.into(),
            min_role.into(),
            enabled.into(),
            false.into(),
            dtstart.into(),
            rrule.into(),
            tzid.into(),
            now.into(),
            now.into(),
            actor.into(),
            Expr::value(Option::<Uuid>::None),
            Expr::value(Option::<DateTime<Utc>>::None),
        ])
        .returning(ReturningClause::Exprs(vec![
            Expr::col(C::Id).into(),
            Expr::col(C::Slug).into(),
            Expr::col(C::Name).into(),
            Expr::col(C::PublicName).into(),
            Expr::col(C::PublicDescription).into(),
            Expr::col(C::Prompt).into(),
            Expr::col(C::MinRole).into(),
            Expr::col(C::Enabled).into(),
            Expr::col(C::IsBuiltin).into(),
            Expr::col(C::Dtstart).into(),
            Expr::col(C::Rrule).into(),
            Expr::col(C::Tzid).into(),
            Expr::col(C::CreatedAt).into(),
            Expr::col(C::UpdatedAt).into(),
        ]))
        .to_owned()
}

/// Run one built `INSERT ... RETURNING` and decode its single row.
async fn insert_job_row(
    db: &impl sea_orm::ConnectionTrait,
    stmt: &InsertStatement,
) -> Result<JobRow, sqlx::Error> {
    let row = db
        .query_one(DbBackend::Postgres.build(stmt))
        .await
        .map_err(into_sqlx)?
        .ok_or(sqlx::Error::RowNotFound)?;
    map_job_row_query(&row)
}

/// The `PATCH` job update: `updated_at`/`updated_by_id` always, plus
/// every column present in `cleaned` (explicit nulls included, same
/// `patch_columns` order as the hand-written bind list), filtered to
/// the live row. `RETURNING` comes from `exec_with_returning`.
fn patch_job_update(
    write: &mut PreparedWrite,
    columns: &[&'static str],
    now: DateTime<Utc>,
    actor: Uuid,
    job_id: Uuid,
) -> UpdateMany<loop_jobs::Entity> {
    let mut update = loop_jobs::Entity::update_many();
    update = update.col_expr(loop_jobs::Column::UpdatedAt, Expr::value(now));
    update = update.col_expr(loop_jobs::Column::UpdatedById, Expr::value(Some(actor)));
    for col in columns {
        update = match *col {
            "slug" => update.col_expr(loop_jobs::Column::Slug, Expr::value(write.slug.take())),
            "name" => update.col_expr(loop_jobs::Column::Name, Expr::value(write.name.take())),
            "public_name" => update.col_expr(
                loop_jobs::Column::PublicName,
                Expr::value(write.public_name.take()),
            ),
            "public_description" => update.col_expr(
                loop_jobs::Column::PublicDescription,
                Expr::value(write.public_description.take()),
            ),
            "prompt" => {
                update.col_expr(loop_jobs::Column::Prompt, Expr::value(write.prompt.take()))
            }
            "min_role" => update.col_expr(
                loop_jobs::Column::MinRole,
                Expr::value(write.min_role.take()),
            ),
            "enabled" => update.col_expr(
                loop_jobs::Column::Enabled,
                Expr::value(write.enabled.take()),
            ),
            "dtstart" => update.col_expr(
                loop_jobs::Column::Dtstart,
                Expr::value(write.dtstart.take()),
            ),
            "rrule" => update.col_expr(loop_jobs::Column::Rrule, Expr::value(write.rrule.take())),
            "tzid" => update.col_expr(loop_jobs::Column::Tzid, Expr::value(write.tzid.take())),
            _ => unreachable!("patch_columns only yields writable columns"),
        };
    }
    update
        .filter(loop_jobs::Column::Id.eq(job_id))
        .filter(loop_jobs::Column::DeletedAt.is_null())
}

/// The targets list (`admin_views.py:183-210`): the `select_related`
/// join over workspace/user/last-run, the `t.deleted_at IS NULL` +
/// job filter, the three optional equality filters in
/// `skip_reason, workspace_slug, run_status` order, `-updated_at`
/// order, and the page window. Columns are aliased because four joined
/// tables share names and row mapping is by name.
fn targets_list_select(
    job_id: Uuid,
    filter: &queries::TargetsFilter,
    page: i64,
) -> SelectStatement {
    let t = Alias::new("t");
    let w = Alias::new("w");
    let u = Alias::new("u");
    let turn = Alias::new("turn");
    let mut cond = Condition::all()
        .add(Expr::col((t.clone(), Alias::new("deleted_at"))).is_null())
        .add(Expr::col((t.clone(), Alias::new("job_id"))).eq(job_id));
    // Bind order follows the filter order above (`$1 = job_id`, then
    // `skip_reason`, `workspace_slug`, `run_status`).
    if let Some(reason) = &filter.skip_reason {
        cond = cond.add(Expr::col((t.clone(), Alias::new("last_skip_reason"))).eq(reason.as_str()));
    }
    if let Some(slug) = &filter.workspace_slug {
        cond = cond.add(Expr::col((w.clone(), Alias::new("slug"))).eq(slug.as_str()));
    }
    if let Some(status) = &filter.run_status {
        cond = cond.add(Expr::col((turn.clone(), Alias::new("status"))).eq(status.as_str()));
    }
    let (limit, offset) = queries::targets_page_window(page);
    let mut sel = Query::select();
    sel.expr_as(Expr::col((t.clone(), Alias::new("id"))), Alias::new("id"))
        .expr_as(
            Expr::col((t.clone(), Alias::new("next_run_at"))),
            Alias::new("next_run_at"),
        )
        .expr_as(
            Expr::col((t.clone(), Alias::new("last_skipped_at"))),
            Alias::new("last_skipped_at"),
        )
        .expr_as(
            Expr::col((t.clone(), Alias::new("last_skip_reason"))),
            Alias::new("last_skip_reason"),
        )
        .expr_as(
            Expr::col((w.clone(), Alias::new("slug"))),
            Alias::new("workspace_slug"),
        )
        .expr_as(
            Expr::col((u.clone(), Alias::new("email"))),
            Alias::new("user_email"),
        )
        .expr_as(
            Expr::col((turn.clone(), Alias::new("status"))),
            Alias::new("run_status"),
        )
        .expr_as(
            Expr::col((turn.clone(), Alias::new("error_code"))),
            Alias::new("run_error_code"),
        )
        .expr_as(
            Expr::col((turn.clone(), Alias::new("model_used"))),
            Alias::new("run_model_used"),
        )
        .expr_as(
            Expr::col((turn.clone(), Alias::new("usage"))),
            Alias::new("run_usage"),
        )
        .expr_as(
            Expr::col((turn.clone(), Alias::new("completed_at"))),
            Alias::new("run_completed_at"),
        )
        .from_as(Alias::new("loop_targets"), t.clone())
        .join_as(
            JoinType::InnerJoin,
            Alias::new("workspaces"),
            w.clone(),
            Expr::col((t.clone(), Alias::new("workspace_id")))
                .equals((w.clone(), Alias::new("id"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("users"),
            u.clone(),
            Expr::col((t.clone(), Alias::new("user_id"))).equals((u.clone(), Alias::new("id"))),
        )
        .join_as(
            JoinType::LeftJoin,
            Alias::new(queries::ASSISTANT_TURN_TABLE),
            Alias::new("turn"),
            Expr::col((t.clone(), Alias::new("last_run_id"))).equals((turn, Alias::new("id"))),
        )
        .cond_where(cond)
        .order_by((t, Alias::new("updated_at")), Order::Desc)
        .limit(limit as u64);
    if offset > 0 {
        sel.offset(offset as u64);
    }
    sel
}

/// Decode one targets-list row by output name (order-independent).
fn map_target_row(row: &QueryResult) -> Result<TargetRow, sqlx::Error> {
    fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T, sqlx::Error> {
        row.try_get::<T>("", column).map_err(into_sqlx)
    }
    Ok(TargetRow {
        id: get(row, "id")?,
        next_run_at: get(row, "next_run_at")?,
        last_skipped_at: get(row, "last_skipped_at")?,
        last_skip_reason: get(row, "last_skip_reason")?,
        workspace_slug: get(row, "workspace_slug")?,
        user_email: get(row, "user_email")?,
        run_status: get(row, "run_status")?,
        run_error_code: get(row, "run_error_code")?,
        run_model_used: get(row, "run_model_used")?,
        run_usage: get(row, "run_usage")?,
        run_completed_at: get(row, "run_completed_at")?,
    })
}

async fn fetch_job(pool: &PgPool, job_id: &Uuid) -> Result<Option<JobRow>, Denial> {
    let db = pidash_db::sea_orm_connection(pool);
    loop_jobs::Entity::find_by_id(*job_id)
        .filter(loop_jobs::Column::DeletedAt.is_null())
        .one(&db)
        .await
        .map(|model| model.map(map_job_row))
        .map_err(|_| Denial::ServerError)
}

/// One targets-list entry with its joined columns (explicit aliases: the
/// underlying select joins four tables, so bare names would collide).
#[derive(Debug, Clone)]
struct TargetRow {
    id: Uuid,
    next_run_at: Option<DateTime<Utc>>,
    last_skipped_at: Option<DateTime<Utc>>,
    last_skip_reason: String,
    workspace_slug: String,
    user_email: String,
    run_status: Option<String>,
    run_error_code: Option<String>,
    run_model_used: Option<String>,
    run_usage: Option<Value>,
    run_completed_at: Option<DateTime<Utc>>,
}

/// Render one `_row` (`admin_views.py:212-234`): the cursor plus the
/// nested `last_run` block (or null), with `total_tokens` read out of the
/// run's `usage` JSON. Key order follows the Python dict.
fn render_target_row(row: &TargetRow) -> Value {
    let last_run = row.run_status.as_ref().map(|status| {
        let total_tokens = row
            .run_usage
            .as_ref()
            .and_then(|usage| usage.get("total_tokens"))
            .cloned()
            .unwrap_or(Value::Null);
        json!({
            "status": status,
            "error_code": row.run_error_code.clone().unwrap_or_default(),
            "model_used": row.run_model_used.clone().unwrap_or_default(),
            "total_tokens": total_tokens,
            "completed_at": iso_or_null(row.run_completed_at.as_ref()),
        })
    });
    json!({
        "id": row.id.to_string(),
        "workspace_slug": row.workspace_slug,
        "user_email": row.user_email,
        "next_run_at": iso_or_null(row.next_run_at.as_ref()),
        "last_skipped_at": iso_or_null(row.last_skipped_at.as_ref()),
        "last_skip_reason": row.last_skip_reason,
        "last_run": last_run,
    })
}

// ---------------------------------------------------------------------------
// response rendering (stored row + raw echo of the write)
// ---------------------------------------------------------------------------

/// The explicit-string-`dtstart` write: the row commits, then rendering
/// the in-memory attribute (`str.isoformat()`) raises into the generic
/// 500 (`admin_views.py:54`). Returned after the write commits.
#[derive(Debug)]
struct StringDtstart;

impl From<StringDtstart> for Denial {
    fn from(_: StringDtstart) -> Self {
        Denial::ServerError
    }
}

/// Render the full 14-key admin payload for a stored row, echoing the raw
/// cleaned write values the way the in-memory Django instance does after
/// `create`/`save` (`min_role`/`enabled`/texts echo verbatim; a raw
/// string `dtstart` is the 500 path above).
fn render_job(row: &JobRow, echo: Option<&Map<String, Value>>) -> Result<Value, StringDtstart> {
    let owned_id = row.id.to_string();
    let owned_dtstart = iso(&row.dtstart);
    let owned_created_at = iso(&row.created_at);
    let owned_updated_at = iso(&row.updated_at);
    let shape_row = AdminJobRow {
        id: &owned_id,
        slug: &row.slug,
        name: &row.name,
        public_name: &row.public_name,
        public_description: &row.public_description,
        prompt: &row.prompt,
        min_role: i32::from(row.min_role),
        enabled: row.enabled,
        is_builtin: row.is_builtin,
        dtstart: Some(owned_dtstart.as_str()),
        rrule: &row.rrule,
        tzid: &row.tzid,
        created_at: Some(owned_created_at.as_str()),
        updated_at: Some(owned_updated_at.as_str()),
    };
    let mut body = job_payload(&shape_row);
    if let Some(cleaned) = echo {
        for (key, value) in cleaned {
            if key == "dtstart" && value.is_string() {
                // The stored column holds the parsed instant, but the
                // in-memory attribute is the raw string: rendering it
                // raises after the write commits.
                return Err(StringDtstart);
            }
            body[key] = value.clone();
        }
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// write preparation (ORM coercions)
// ---------------------------------------------------------------------------

/// Prepared column values for an INSERT/UPDATE, plus whether the raw
/// `dtstart` was a string (the commit-then-500 path).
struct PreparedWrite {
    slug: Option<String>,
    name: Option<String>,
    public_name: Option<String>,
    public_description: Option<String>,
    prompt: Option<String>,
    min_role: Option<i16>,
    enabled: Option<bool>,
    dtstart: Option<DateTime<Utc>>,
    rrule: Option<String>,
    tzid: Option<String>,
    dtstart_was_string: bool,
}

/// Coerce the cleaned write map to column values the way
/// `setattr(job, …)` + `save()` does: texts stringify, `min_role` is
/// `int(value)`, `enabled` follows `BooleanField.to_python`, `dtstart`
/// parses or 400s. `None` (JSON null) binds `NULL`, which the NOT NULL
/// columns turn into the `IntegrityError` 400.
fn prepare_write(cleaned: &Map<String, Value>) -> Result<PreparedWrite, Denial> {
    let text = |key: &str| -> Result<Option<Option<String>>, Denial> {
        match cleaned.get(key) {
            None => Ok(None),
            Some(value) => Ok(Some(guards::python_str(value))),
        }
    };
    let min_role = match cleaned.get("min_role") {
        None => None,
        Some(value) => {
            let stored = guards::coerce_min_role_int(value).ok_or(Denial::ServerError)?;
            Some(i16::try_from(stored).map_err(|_| Denial::ServerError)?)
        }
    };
    let enabled = match cleaned.get("enabled") {
        None => None,
        Some(value) => match bool_prep(value)? {
            BoolPrep::Value(b) => Some(Some(b)),
            BoolPrep::Null => Some(None),
        },
    };
    let mut dtstart: Option<Option<DateTime<Utc>>> = None;
    let mut dtstart_was_string = false;
    if let Some(value) = cleaned.get("dtstart") {
        match value {
            Value::Null => dtstart = Some(None),
            Value::String(s) => {
                dtstart = Some(Some(parse_dtstart_text(s).ok_or(Denial::InvalidDetail)?));
                dtstart_was_string = true;
            }
            _ => return Err(Denial::InvalidDetail),
        }
    }
    Ok(PreparedWrite {
        slug: text("slug")?.flatten(),
        name: text("name")?.flatten(),
        public_name: text("public_name")?.flatten(),
        public_description: text("public_description")?.flatten(),
        prompt: text("prompt")?.flatten(),
        min_role,
        enabled: enabled.flatten(),
        dtstart: dtstart.flatten(),
        rrule: text("rrule")?.flatten(),
        tzid: text("tzid")?.flatten(),
        dtstart_was_string,
    })
}

/// Columns a PATCH writes, in bind order: every key present in `cleaned`,
/// including explicit nulls. Python `setattr(job, key, None)` + `save()`
/// writes NULL (the NOT NULL columns answer the `IntegrityError` 400), so
/// an explicit null must bind NULL — skipping the column would wrongly
/// answer 200 unchanged (the Porting guide's `None`-vs-absent trap).
fn patch_columns(cleaned: &Map<String, Value>) -> Vec<&'static str> {
    const ALL: [&str; 10] = [
        "slug",
        "name",
        "public_name",
        "public_description",
        "prompt",
        "min_role",
        "enabled",
        "dtstart",
        "rrule",
        "tzid",
    ];
    ALL.into_iter()
        .filter(|col| cleaned.contains_key(*col))
        .collect()
}

/// Create-time value for an optional-with-default column: the model default
/// applies only when the key is absent. An explicit null binds NULL (the
/// `IntegrityError` 400), never the default.
fn or_default_when_absent<T>(
    cleaned: &Map<String, Value>,
    key: &str,
    prepared: Option<T>,
    default: T,
) -> Option<T> {
    if cleaned.contains_key(key) {
        prepared
    } else {
        Some(default)
    }
}

/// Map a write error the way `BaseAPIView.handle_exception` does:
/// unique/CHECK/NOT NULL violations are `IntegrityError` → 400
/// invalid-payload; everything else (truncation `DataError`, …) is the
/// generic 500.
fn map_write_error(err: sqlx::Error) -> Denial {
    match err {
        sqlx::Error::Database(db)
            if matches!(db.code().as_deref(), Some("23505" | "23514" | "23502")) =>
        {
            Denial::InvalidPayload
        }
        _ => Denial::ServerError,
    }
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET /api/instances/loop/jobs/` (`admin_views.py:111-113`): non-deleted
/// jobs, `-created_at` order, full payloads.
async fn list_jobs(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    let db = pidash_db::sea_orm_connection(pool);
    let rows: Vec<JobRow> = loop_jobs::Entity::find()
        .filter(loop_jobs::Column::DeletedAt.is_null())
        .order_by_desc(loop_jobs::Column::CreatedAt)
        .all(&db)
        .await
        .map(|models| models.into_iter().map(map_job_row).collect())
        .map_err(|_| Denial::ServerError)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(render_job(row, None).map_err(Denial::from)?);
    }
    Ok(json_response(StatusCode::OK, Value::Array(items)))
}

/// `POST /api/instances/loop/jobs/` (`admin_views.py:115-124`): validate
/// (`partial=False`), `slug_taken` 409, default `dtstart=now`,
/// `is_builtin=False`, 201.
async fn create_job(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    let data = parse_json_body(&headers, &body)?;
    let cleaned = guards::validate_writes(&data, false).map_err(Denial::from)?;

    let slug = cleaned
        .get("slug")
        .and_then(guards::python_str)
        .ok_or(Denial::ServerError)?;
    let db = pidash_db::sea_orm_connection(pool);
    let slug_exists = loop_jobs::Entity::find()
        .filter(loop_jobs::Column::Slug.eq(slug.clone()))
        .filter(loop_jobs::Column::DeletedAt.is_null())
        .exists(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if guards::slug_taken_on_create(slug_exists) {
        return Err(Denial::Guard(
            axum::http::StatusCode::CONFLICT,
            serde_json::from_str(guards::SLUG_TAKEN_BODY).expect("const parses"),
        ));
    }

    let write = prepare_write(&cleaned)?;
    let now = Utc::now();
    let row = insert_job_row(
        &db,
        &job_insert(
            write.slug.unwrap_or(slug),
            write.name,
            write.public_name,
            or_default_when_absent(
                &cleaned,
                "public_description",
                write.public_description,
                String::new(),
            ),
            write.prompt,
            write.min_role.unwrap_or(15),
            or_default_when_absent(&cleaned, "enabled", write.enabled, true),
            or_default_when_absent(&cleaned, "dtstart", write.dtstart, now),
            write.rrule,
            or_default_when_absent(&cleaned, "tzid", write.tzid, "UTC".to_owned()),
            now,
            actor,
        ),
    )
    .await
    .map_err(map_write_error)?;

    if write.dtstart_was_string {
        return Err(StringDtstart.into());
    }
    let body = render_job(&row, Some(&cleaned)).map_err(Denial::from)?;
    Ok(json_response(StatusCode::CREATED, body))
}

/// `GET /api/instances/loop/jobs/<uuid>/` (`admin_views.py:130-150`):
/// 404 `not_found` when missing/deleted, else the full payload with the
/// 24h `stats` rollup appended.
async fn detail_job(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    axum::extract::Path(pk): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, Denial> {
    // Django routes this segment through `<uuid:pk>`
    // (`loop/admin_urls.py:16-17`), whose converter regex is
    // lowercase-hyphenated only and compiled case-sensitively. Every other
    // spelling resolver-404s before auth or the view. `Uuid::parse_str`
    // accepts uppercase, simple, braced and URN spellings, so gate on the raw
    // segment first and proxy the rest — parseable-but-non-canonical and
    // garbage alike — to Django for its own resolver 404
    // (PIDASHCONV-1095 / -1132 / -1225 precedent).
    let job_id = match pk.parse::<Uuid>() {
        Ok(job_id) if is_canonical_uuid(&pk) => job_id,
        _ => return proxy_pass(state, method, uri, headers, &[]).await,
    };
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    let Some(row) = fetch_job(pool, &job_id).await? else {
        return Err(Denial::NotFound);
    };
    let db = pidash_db::sea_orm_connection(pool);
    let stats = queries::fetch_job_stats(&db, job_id, Utc::now() - chrono::Duration::hours(24))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut body = render_job(&row, None).map_err(Denial::from)?;
    body["stats"] = json!({
        "target_count": stats.target_count,
        "completed": stats.completed,
        "failed": stats.failed,
        "skipped": stats.skipped,
    });
    Ok(json_response(StatusCode::OK, body))
}

/// `PATCH /api/instances/loop/jobs/<uuid>/` (`admin_views.py:152-166`):
/// 404 when missing/deleted, validate (`partial=True`), rename-conflict
/// 409, save, full payload.
async fn patch_job(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    axum::extract::Path(pk): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    // Same resolver gate as GET detail: non-canonical spellings never
    // reach the view, so proxy before auth or any write.
    let job_id = match pk.parse::<Uuid>() {
        Ok(job_id) if is_canonical_uuid(&pk) => job_id,
        _ => return proxy_pass(state, method, uri, headers, &body).await,
    };
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    let Some(current) = fetch_job(pool, &job_id).await? else {
        return Err(Denial::NotFound);
    };
    let data = parse_json_body(&headers, &body)?;
    let cleaned = guards::validate_writes(&data, true).map_err(Denial::from)?;

    let db = pidash_db::sea_orm_connection(pool);
    if let Some(raw_slug) = cleaned.get("slug").and_then(guards::python_str) {
        let clash = loop_jobs::Entity::find()
            .filter(loop_jobs::Column::Slug.eq(raw_slug.clone()))
            .filter(loop_jobs::Column::DeletedAt.is_null())
            .filter(loop_jobs::Column::Id.ne(job_id))
            .exists(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        if guards::slug_taken_on_patch(Some(raw_slug.as_str()), &current.slug, clash) {
            return Err(Denial::Guard(
                axum::http::StatusCode::CONFLICT,
                serde_json::from_str(guards::SLUG_TAKEN_BODY).expect("const parses"),
            ));
        }
    }

    let mut write = prepare_write(&cleaned)?;
    let now = Utc::now();
    // `setattr` + `save()`: only cleaned columns plus the `auto_now`
    // `updated_at` (and the audit `updated_by`) move. Columns come from
    // one helper so the SET list and the binds cannot drift apart; an
    // explicit null binds NULL (never skips).
    let columns = patch_columns(&cleaned);
    let updated = patch_job_update(&mut write, &columns, now, actor, job_id)
        .exec_with_returning(&db)
        .await
        .map_err(|err| map_write_error(into_sqlx(err)))?;
    let row = updated
        .into_iter()
        .next()
        .map(map_job_row)
        .ok_or(Denial::NotFound)?;

    if write.dtstart_was_string {
        return Err(StringDtstart.into());
    }
    let body = render_job(&row, Some(&cleaned)).map_err(Denial::from)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `DELETE /api/instances/loop/jobs/<uuid>/` (`admin_views.py:168-181`):
/// 404 when missing/deleted, else stamp the job, its targets and its
/// user preferences with one timestamp in a single transaction, 204
/// empty. Since NEWFRONT-957 Python does the cascade synchronously in
/// the view instead of `SoftDeleteModel.delete()`'s celery enqueue, so
/// nothing is published here: the worker is never involved.
async fn delete_job(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    axum::extract::Path(pk): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, Denial> {
    // Django routes this segment through `<uuid:pk>`
    // (`loop/admin_urls.py:16-17`), whose converter regex is
    // lowercase-hyphenated only and compiled case-sensitively. Every other
    // spelling resolver-404s before auth or the view. `Uuid::parse_str`
    // accepts uppercase, simple, braced and URN spellings, so gate on the raw
    // segment first and proxy the rest — parseable-but-non-canonical and
    // garbage alike — to Django for its own resolver 404
    // (PIDASHCONV-1095 / -1132 / -1225 precedent).
    let job_id = match pk.parse::<Uuid>() {
        Ok(job_id) if is_canonical_uuid(&pk) => job_id,
        _ => return proxy_pass(state, method, uri, headers, &[]).await,
    };
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    // `now = timezone.now()` once, then the job `save()` (which moves
    // `updated_at` via `auto_now` and `updated_by` via CRUM) plus the
    // two `.update(deleted_at=now)` sweeps. `QuerySet.update` writes
    // only `deleted_at` — no `auto_now`, no CRUM — and runs through
    // the `SoftDeletionManager`, so already-deleted rows keep their
    // old stamp (`db/mixins.py:57-58`).
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let marked = loop_jobs::Entity::update_many()
            .col_expr(loop_jobs::Column::DeletedAt, Expr::value(now))
            .col_expr(loop_jobs::Column::UpdatedAt, Expr::value(now))
            .col_expr(loop_jobs::Column::UpdatedById, Expr::value(Some(actor)))
            .filter(loop_jobs::Column::Id.eq(job_id))
            .filter(loop_jobs::Column::DeletedAt.is_null())
            .exec(&conn)
            .await
            .map_err(|_| Denial::ServerError)?;
        if marked.rows_affected == 0 {
            return Err(Denial::NotFound);
        }
        loop_targets::Entity::update_many()
            .col_expr(loop_targets::Column::DeletedAt, Expr::value(now))
            .filter(loop_targets::Column::JobId.eq(job_id))
            .filter(loop_targets::Column::DeletedAt.is_null())
            .exec(&conn)
            .await
            .map_err(|_| Denial::ServerError)?;
        loop_user_preferences::Entity::update_many()
            .col_expr(loop_user_preferences::Column::DeletedAt, Expr::value(now))
            .filter(loop_user_preferences::Column::JobId.eq(job_id))
            .filter(loop_user_preferences::Column::DeletedAt.is_null())
            .exec(&conn)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("empty 204 response"))
}

/// `GET /api/instances/loop/jobs/<uuid>/targets/`
/// (`admin_views.py:179-210`): 404 when the job is missing; the
/// `skip_reason`/`workspace`/`status` filters, `page`/`per=50` slice,
/// `-updated_at` order, and the `{"page","results"}` envelope with the
/// `_row` shape.
async fn job_targets(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    axum::extract::Path(pk): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Response, Denial> {
    // Django routes this segment through `<uuid:pk>`
    // (`loop/admin_urls.py:16-17`), whose converter regex is
    // lowercase-hyphenated only and compiled case-sensitively. Every other
    // spelling resolver-404s before auth or the view. `Uuid::parse_str`
    // accepts uppercase, simple, braced and URN spellings, so gate on the raw
    // segment first and proxy the rest — parseable-but-non-canonical and
    // garbage alike — to Django for its own resolver 404
    // (PIDASHCONV-1095 / -1132 / -1225 precedent).
    let job_id = match pk.parse::<Uuid>() {
        Ok(job_id) if is_canonical_uuid(&pk) => job_id,
        _ => return proxy_pass(state, method, uri, headers, &[]).await,
    };
    let pool = pools(&state)?;
    let actor = authed_user(&state, pool, extension).await?;
    require_instance_admin(pool, &actor).await?;

    if fetch_job(pool, &job_id).await?.is_none() {
        return Err(Denial::NotFound);
    }
    let filter = queries::TargetsFilter::from_params(
        params.get("skip_reason").map(String::as_str),
        params.get("workspace").map(String::as_str),
        params.get("status").map(String::as_str),
    );
    let page = queries::clamp_page(params.get("page").map(String::as_str));

    // Same join/filter/order/slice semantics as
    // `queries::targets_list_sql` (which the queries layer pins against
    // the fixture SQL); columns are aliased here because four joined
    // tables share names (`id`, …) and row mapping is by name.
    let db = pidash_db::sea_orm_connection(pool);
    let rows: Vec<TargetRow> = db
        .query_all(DbBackend::Postgres.build(&targets_list_select(job_id, &filter, page)))
        .await
        .map_err(|_| Denial::ServerError)?
        .iter()
        .map(map_target_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Denial::ServerError)?;

    let body = json!({
        "page": page,
        "results": rows.iter().map(render_target_row).collect::<Vec<_>>(),
    });
    Ok(json_response(StatusCode::OK, body))
}

/// Whether a path segment is spelled the way Django's `UUIDConverter`
/// routes it: lowercase hex, hyphenated, exactly 36 characters
/// (`django/urls/converters.py`; the `v1_cli_auth` PIDASHCONV-1095 and
/// `license` PIDASHCONV-1132 precedents).
fn is_canonical_uuid(raw: &str) -> bool {
    if raw.len() != 36 {
        return false;
    }
    raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    })
}

/// A non-UUID `pk` never reaches a Django view (the `<uuid:pk>`
/// converter 404s), so hand the request back to Django for its own 404.
async fn proxy_pass(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: &[u8],
) -> Result<Response, Denial> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    let req = builder
        .body(axum::body::Body::from(body.to_vec()))
        .map_err(|_| Denial::ServerError)?;
    Ok(edge::proxy(State(state), req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/handlers")
    }

    fn fixture(name: &str) -> Value {
        let path = fixtures_dir().join(name);
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn crud_case(key: &str) -> (i64, Value) {
        let crud = fixture("admin_crud.golden.json");
        let case = crud.get(key).expect("crud case");
        (
            case.get("status").and_then(Value::as_i64).expect("status"),
            case.get("body").expect("body").clone(),
        )
    }

    #[test]
    fn error_bodies_replay_crud_golden() {
        // Every error shape the admin CRUD golden records, byte for byte.
        for (key, status) in [
            ("admin_create_409", 409),
            ("admin_create_400_slug", 400),
            ("admin_create_400_rrule", 400),
            ("admin_detail_404", 404),
            ("admin_patch_404", 404),
            ("admin_patch_409", 409),
            ("admin_delete_404_again", 404),
        ] {
            let (got_status, body) = crud_case(key);
            assert_eq!(got_status, status, "{key} status");
            let (denial_status, denial_body) = match status {
                404 => Denial::NotFound.status_and_body(),
                409 => Denial::Guard(
                    StatusCode::CONFLICT,
                    serde_json::from_str(guards::SLUG_TAKEN_BODY).expect("const parses"),
                )
                .status_and_body(),
                _ => {
                    let known: Value = serde_json::from_str(match key {
                        "admin_create_400_slug" => guards::INVALID_SLUG_BODY,
                        _ => guards::RRULE_TOO_FREQUENT_BODY,
                    })
                    .expect("const parses");
                    assert_eq!(&body, &known, "{key} body");
                    continue;
                }
            };
            assert_eq!(denial_status.as_u16(), status as u16, "{key} denial status");
            let parsed: Value = serde_json::from_str(&denial_body).expect("denial parses");
            assert_eq!(&parsed, &body, "{key} body");
        }
    }

    #[test]
    fn missing_fields_body_is_sorted_like_golden() {
        // `admin_create_400_missing`: the `detail` list is sorted
        // (`admin_views.py:98-104`). Exercised through the real create
        // path, not the body constructor directly.
        let (_, body) = crud_case("admin_create_400_missing");
        let err = guards::validate_writes(&json!({"slug": "mf"}), false).expect_err("missing");
        let produced = match err {
            ValidateFail::Reject(reject) => {
                assert_eq!(reject.status, StatusCode::BAD_REQUEST);
                reject.body
            }
            ValidateFail::ServerError => panic!("expected missing_fields, got 500"),
        };
        assert_eq!(&produced, &body);
        // Wire order follows the Python dict (`error` before `detail`);
        // the golden file stores keys sorted, so order is asserted
        // separately.
        let order: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order, vec!["error", "detail"]);
    }

    #[test]
    fn targets_404_replays_golden() {
        let targets = fixture("admin_targets.golden.json");
        let case = targets.get("targets_404").expect("targets_404");
        assert_eq!(case.get("status").and_then(Value::as_i64), Some(404));
        let (status, body) = Denial::NotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let parsed: Value = serde_json::from_str(&body).expect("denial parses");
        assert_eq!(&parsed, case.get("body").expect("body"));
    }

    #[test]
    fn bool_prep_matches_django_to_python() {
        // Probed against Django 6.0.5 `BooleanField.get_prep_value`.
        let yes = |v: Value| bool_prep(&v).expect("accepts").into_bool();
        assert!(yes(json!(true)));
        assert!(yes(json!(1)));
        assert!(yes(json!(1.0)));
        assert!(yes(json!("t")) && yes(json!("True")) && yes(json!("1")));
        assert!(!yes(json!(false)));
        assert!(!yes(json!(0)));
        assert!(!yes(json!(0.0)));
        assert!(!yes(json!("f")) && !yes(json!("False")) && !yes(json!("0")));
        assert!(matches!(bool_prep(&json!(null)), Ok(BoolPrep::Null)));
        for bad in [
            json!("yes"),
            json!(""),
            json!(1.5),
            json!(2),
            json!([]),
            json!({}),
        ] {
            assert!(
                matches!(bool_prep(&bad), Err(Denial::InvalidDetail)),
                "{bad}"
            );
        }
    }

    #[test]
    fn dtstart_parser_matches_django_shapes() {
        let utc = |s: &str| iso(&parse_dtstart_text(s).expect("parses"));
        assert_eq!(
            utc("2026-09-28T04:28:10.235211+00:00"),
            "2026-09-28T04:28:10.235211+00:00"
        );
        assert_eq!(
            utc("2026-09-28 04:28:10.235211+00:00"),
            "2026-09-28T04:28:10.235211+00:00"
        );
        assert_eq!(utc("2026-09-28T04:28:10Z"), "2026-09-28T04:28:10+00:00");
        assert_eq!(utc("2026-09-28"), "2026-09-28T00:00:00+00:00");
        assert_eq!(
            utc("2026-09-28T06:28:10+02:00"),
            "2026-09-28T04:28:10+00:00"
        );
        for bad in [
            "garbage",
            "2026-13-01",
            "04:28:10",
            "",
            "2026-09-28T25:00:00",
        ] {
            assert!(parse_dtstart_text(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn iso_renders_python_isoformat_not_drf() {
        // `_job_payload` calls `.isoformat()`: `+00:00`, microseconds only
        // when nonzero — never the DRF `Z` rewrite.
        let with_micros = DateTime::parse_from_rfc3339("2026-09-28T04:28:10.235211+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(iso(&with_micros), "2026-09-28T04:28:10.235211+00:00");
        let millis = DateTime::parse_from_rfc3339("2026-11-01T03:00:00.500+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(iso(&millis), "2026-11-01T03:00:00.500000+00:00");
        let whole = DateTime::parse_from_rfc3339("2026-09-29T03:21:09+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(iso(&whole), "2026-09-29T03:21:09+00:00");
    }

    #[test]
    fn target_row_render_replays_golden_shapes() {
        // The completed-run row of `targets_200`, rebuilt from its own
        // values: rendering must return the exact recorded bytes shape.
        let targets = fixture("admin_targets.golden.json");
        let results = targets
            .get("targets_200")
            .and_then(|v| v.get("body"))
            .and_then(|v| v.get("results"))
            .and_then(Value::as_array)
            .expect("results");
        let expected = results
            .iter()
            .find(|r| r.get("user_email").and_then(Value::as_str) == Some("fxloop-admin@e.com"))
            .expect("admin row");
        let dt = |s: &str| {
            DateTime::parse_from_rfc3339(s)
                .expect("fixture datetime parses")
                .with_timezone(&Utc)
        };
        let row = TargetRow {
            id: expected
                .get("id")
                .and_then(Value::as_str)
                .expect("id")
                .parse()
                .expect("uuid"),
            next_run_at: Some(dt("2026-09-28T05:28:09.657296+00:00")),
            last_skipped_at: None,
            last_skip_reason: String::new(),
            workspace_slug: "fxloop-ws".to_owned(),
            user_email: "fxloop-admin@e.com".to_owned(),
            run_status: Some("completed".to_owned()),
            run_error_code: Some(String::new()),
            run_model_used: Some("gpt-test".to_owned()),
            run_usage: Some(json!({"total_tokens": 1234})),
            run_completed_at: Some(dt("2026-09-28T03:28:09.657296+00:00")),
        };
        let produced = render_target_row(&row);
        assert_eq!(&produced, expected);
        // Key order follows the Python dict, not the golden file's sorted
        // storage order.
        let order: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            order,
            vec![
                "id",
                "workspace_slug",
                "user_email",
                "next_run_at",
                "last_skipped_at",
                "last_skip_reason",
                "last_run",
            ]
        );
        // The skipped row renders a null `last_run` with its reason kept.
        let skipped = results
            .iter()
            .find(|r| r.get("user_email").and_then(Value::as_str) == Some("fxloop-guest@e.com"))
            .expect("guest row");
        let skipped_row = TargetRow {
            id: skipped
                .get("id")
                .and_then(Value::as_str)
                .expect("id")
                .parse()
                .expect("uuid"),
            next_run_at: Some(dt("2026-09-28T05:28:09.657296+00:00")),
            last_skipped_at: Some(dt("2026-09-28T04:23:09.657296+00:00")),
            last_skip_reason: "min_role".to_owned(),
            workspace_slug: "fxloop-ws".to_owned(),
            user_email: "fxloop-guest@e.com".to_owned(),
            run_status: None,
            run_error_code: None,
            run_model_used: None,
            run_usage: None,
            run_completed_at: None,
        };
        assert_eq!(&render_target_row(&skipped_row), skipped);
    }

    #[test]
    fn patch_columns_include_explicit_nulls_in_order() {
        // The `None`-vs-absent trap: an explicit null is a write (binds
        // NULL), so the PATCH column list keys on presence, not on the
        // prepared value.
        let cleaned = guards::validate_writes(
            &json!({"name": null, "enabled": null, "dtstart": null, "tzid": "UTC"}),
            true,
        )
        .expect("nulls validate clean");
        assert_eq!(
            patch_columns(&cleaned),
            vec!["name", "enabled", "dtstart", "tzid"]
        );
        let write = prepare_write(&cleaned).expect("prepares");
        assert!(write.name.is_none());
        assert!(write.enabled.is_none());
        assert!(write.dtstart.is_none());
        assert!(!write.dtstart_was_string);
        assert_eq!(write.tzid.as_deref(), Some("UTC"));
        let empty = guards::validate_writes(&json!({}), true).expect("empty patch validates");
        assert!(patch_columns(&empty).is_empty());
    }

    #[test]
    fn create_defaults_apply_only_when_absent() {
        // Absent keys take the model defaults; explicit nulls stay null so
        // the NOT NULL columns answer the `IntegrityError` 400.
        let absent = guards::validate_writes(&json!({}), true).expect("empty patch validates");
        assert_eq!(
            or_default_when_absent(&absent, "enabled", None, true),
            Some(true)
        );
        assert_eq!(
            or_default_when_absent(&absent, "public_description", None, String::new()),
            Some(String::new())
        );
        let nulled =
            guards::validate_writes(&json!({"enabled": null, "public_description": null}), true)
                .expect("nulls validate clean");
        let prepared = prepare_write(&nulled).expect("prepares");
        assert_eq!(
            or_default_when_absent(&nulled, "enabled", prepared.enabled, true),
            None
        );
        assert_eq!(
            or_default_when_absent(
                &nulled,
                "public_description",
                prepared.public_description,
                String::new()
            ),
            None
        );
    }

    #[test]
    fn job_render_replays_list_row_and_stats_shape() {
        // `admin_list.body[0]` round-trips through the renderer untouched
        // (no echo), and the detail rollup appends `stats` last.
        let (status, body) = crud_case("admin_list");
        assert_eq!(status, 200);
        let first = body.as_array().expect("array").first().expect("row");
        let req = |key: &str| {
            first
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("golden lacks string key {key}"))
        };
        let dt = |key: &str| {
            DateTime::parse_from_rfc3339(req(key))
                .expect("fixture datetime parses")
                .with_timezone(&Utc)
        };
        let row = JobRow {
            id: req("id").parse().expect("uuid"),
            slug: req("slug").to_owned(),
            name: req("name").to_owned(),
            public_name: req("public_name").to_owned(),
            public_description: req("public_description").to_owned(),
            prompt: req("prompt").to_owned(),
            min_role: first.get("min_role").and_then(Value::as_i64).expect("role") as i16,
            enabled: first
                .get("enabled")
                .and_then(Value::as_bool)
                .expect("enabled"),
            is_builtin: first
                .get("is_builtin")
                .and_then(Value::as_bool)
                .expect("builtin"),
            dtstart: dt("dtstart"),
            rrule: req("rrule").to_owned(),
            tzid: req("tzid").to_owned(),
            created_at: dt("created_at"),
            updated_at: dt("updated_at"),
        };
        assert_eq!(&render_job(&row, None).expect("renders"), first);
        // Echo path: a raw float `min_role` is echoed verbatim while the
        // stored column holds the truncated int (contract `min_role`
        // echo cases).
        let mut echo = Map::new();
        echo.insert("min_role".to_owned(), json!(15.5));
        let echoed = render_job(&row, Some(&echo)).expect("renders");
        assert_eq!(echoed.get("min_role"), Some(&json!(15.5)));
        // The detail shape is the same payload with `stats` appended last.
        let mut detail = render_job(&row, None).expect("renders");
        detail["stats"] = json!({"target_count": 5, "completed": 1, "failed": 1, "skipped": 1});
        let order: Vec<&str> = detail
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order.last(), Some(&"stats"));
        assert_eq!(order.len(), 15);
    }

    // SeaORM statement shapes (`orm-migration.md` §3.2): the create
    // insert names every column, generates the id server-side, marks
    // `is_builtin` false, leaves `updated_by_id`/`deleted_at` NULL,
    // and returns the 14 projected columns; the patch update names
    // the cleaned columns plus `updated_at`/`updated_by_id` and
    // filters the live row; the targets select keeps the four-table
    // join, the job filter, and the page window.
    #[test]
    fn job_insert_names_every_column_with_projected_returning() {
        let now =
            chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0).expect("epoch valid");
        let sql = DbBackend::Postgres
            .build(&job_insert(
                "fxloop-job".to_owned(),
                Some("Job".to_owned()),
                Some("Public".to_owned()),
                Some("Desc".to_owned()),
                Some("Prompt".to_owned()),
                15,
                Some(true),
                Some(now),
                Some("FREQ=HOURLY".to_owned()),
                Some("UTC".to_owned()),
                now,
                Uuid::nil(),
            ))
            .to_string();
        assert!(
            sql.contains(r#"INSERT INTO "loop_jobs" ("id", "slug", "name", "public_name", "public_description", "prompt", "min_role", "enabled", "is_builtin", "dtstart", "rrule", "tzid", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at")"#),
            "columns: {sql}"
        );
        assert!(sql.contains("gen_random_uuid()"), "server id: {sql}");
        assert!(sql.contains("FALSE"), "is_builtin: {sql}");
        assert!(
            sql.contains(r#"RETURNING "id", "slug", "name", "public_name", "public_description", "prompt", "min_role", "enabled", "is_builtin", "dtstart", "rrule", "tzid", "created_at", "updated_at""#),
            "returning: {sql}"
        );
    }

    #[test]
    fn patch_update_names_cleaned_columns_on_live_row() {
        use sea_orm::QueryTrait;
        let now =
            chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0).expect("epoch valid");
        let cleaned = guards::validate_writes(&json!({"name": "Renamed", "enabled": null}), true)
            .expect("patch validates");
        let mut write = prepare_write(&cleaned).expect("prepares");
        let sql = patch_job_update(
            &mut write,
            &patch_columns(&cleaned),
            now,
            Uuid::nil(),
            Uuid::nil(),
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(sql.contains(r#"UPDATE "loop_jobs" SET"#), "update: {sql}");
        assert!(sql.contains(r#""updated_at" = "#), "updated_at: {sql}");
        assert!(
            sql.contains(r#""updated_by_id" = "#),
            "updated_by_id: {sql}"
        );
        assert!(sql.contains(r#""name" = 'Renamed'"#), "name: {sql}");
        // An explicit null stays a NULL bind, never a skipped column.
        assert!(sql.contains(r#""enabled" = NULL"#), "null bind: {sql}");
        // Absent columns do not move.
        assert!(!sql.contains(r#""prompt" = "#), "prompt untouched: {sql}");
        assert!(
            sql.contains(r#""id" = '00000000-0000-0000-0000-000000000000'"#),
            "filter: {sql}"
        );
        assert!(
            sql.contains(r#""deleted_at" IS NULL"#),
            "live filter: {sql}"
        );
    }

    #[test]
    fn targets_select_keeps_join_filters_and_window() {
        let job = Uuid::nil();
        let plain = targets_list_select(job, &queries::TargetsFilter::default(), 1);
        let sql = plain.to_string(sea_query::PostgresQueryBuilder);
        assert!(
            sql.contains(r#"FROM "loop_targets" AS "t" INNER JOIN "workspaces" AS "w" ON "t"."workspace_id" = "w"."id" INNER JOIN "users" AS "u" ON "t"."user_id" = "u"."id" LEFT JOIN "assistant_turn" AS "turn" ON "t"."last_run_id" = "turn"."id""#),
            "join: {sql}"
        );
        assert!(
            sql.contains(r#""t"."deleted_at" IS NULL AND "t"."job_id" = '00000000-0000-0000-0000-000000000000'"#),
            "live job filter, one deleted_at: {sql}"
        );
        assert!(sql.contains(r#"AS "workspace_slug""#), "alias: {sql}");
        assert!(sql.contains(r#"AS "run_completed_at""#), "alias: {sql}");
        assert!(
            sql.contains(r#"ORDER BY "t"."updated_at" DESC LIMIT 50"#),
            "order+limit: {sql}"
        );
        // Page 1 omits OFFSET; page 2 offsets by one window.
        assert!(!sql.contains("OFFSET"), "no offset page 1: {sql}");
        let page_two = targets_list_select(job, &queries::TargetsFilter::default(), 2);
        let sql = page_two.to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.ends_with("LIMIT 50 OFFSET 50"), "window: {sql}");

        // Filters bind in skip_reason, workspace, status order.
        let filtered = targets_list_select(
            job,
            &queries::TargetsFilter::from_params(
                Some("min_role"),
                Some("fxloop-ws"),
                Some("completed"),
            ),
            1,
        );
        let sql = filtered.to_string(sea_query::PostgresQueryBuilder);
        let skip = sql
            .find(r#""t"."last_skip_reason" = 'min_role'"#)
            .expect("skip");
        let ws = sql.find(r#""w"."slug" = 'fxloop-ws'"#).expect("workspace");
        let status = sql
            .find(r#""turn"."status" = 'completed'"#)
            .expect("status");
        assert!(skip < ws && ws < status, "filter order: {sql}");
    }

    #[test]
    fn canonical_uuid_accepts_lowercase_hyphenated_only() {
        assert!(is_canonical_uuid("12345678-1234-1234-1234-1234567890ab"));
        let canonical = "12345678-1234-1234-1234-1234567890ab";
        for raw in [
            canonical.to_uppercase(),
            canonical.replace('-', ""),
            format!("{{{canonical}}}"),
            format!("urn:uuid:{canonical}"),
            "not-a-uuid".to_owned(),
            format!("{canonical}0"),
        ] {
            assert!(!is_canonical_uuid(&raw), "{raw}");
        }
    }
}
