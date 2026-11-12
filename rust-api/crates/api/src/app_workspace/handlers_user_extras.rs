//! D-24 user-extras handlers (stage 5, PIDASHCONV-620).
//!
//! Port of five handler units (`apps/api/pi_dash/app/views/`):
//!
//! * `AccountEndpoint` (`user/base.py:406-421`) — [`list_accounts`],
//!   [`get_account`], [`delete_account`].
//! * `ProfileEndpoint` (`user/base.py:423-462`) — [`get_profile`],
//!   [`patch_profile`], plus the `core.user_settings` validate/merge
//!   semantics (`pi_dash/core/user_settings.py`,
//!   `pi_dash/ee/settings/user_settings.py`) — [`validate_settings_patch`],
//!   [`merge_settings`].
//! * `UserActivityGraphEndpoint` (`workspace/user.py:524-539`) —
//!   [`activity_graph`].
//! * `UserIssueCompletedGraphEndpoint` (`workspace/user.py:541-559`) —
//!   [`completed_graph`].
//! * `UserWorkspaceDashboardEndpoint` (`workspace/base.py:262-348`) —
//!   [`dashboard`].
//!
//! Routes mirror `app/urls/user.py:48-83` (mounted under `/api/`):
//! `users/me/profile/` (U06), `users/me/accounts/` (U07),
//! `users/me/accounts/<uuid:pk>/` (U08),
//! `users/me/workspaces/<slug>/activity-graph/` (U14),
//! `users/me/workspaces/<slug>/issues-completed-graph/` (U15),
//! `users/me/workspaces/<slug>/dashboard/` (U16).
//! Registration is the cutover granularity: owned methods serve from
//! Rust, every other method on these paths proxies to Django (its
//! 401-before-405 ordering, 405 bodies and redirects live there).
//!
//! None of the five units carries `@allow_permission` — only
//! `IsAuthenticated`, so anonymous callers get 401 before anything
//! else runs. The graph/dashboard units do no membership check either:
//! an unknown slug (or a caller with no rows) yields zeros, never 404
//! (contract `test_dashboard_unknown_slug_is_zeros`).
//!
//! SQL text comes from the merged D-24 layers
//! (`pidash_services::app_workspace::{queries_core, queries_profile,
//! queries_user}`); shapes from `ser_account_token`; header bytes from
//! `super::gates`. Placeholders stay symbolic there (`:user`, …), so
//! the handlers renumber them to `$n` via [`number_placeholders`] and
//! bind positionally.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * `?month=` garbage is a 500, not a 400: `completed_at__month=<str>`
//!   goes through integer prep (`int(value)`), which raises `ValueError`
//!   — not `ValidationError` — so `handle_exception` falls through to
//!   the generic branch. Shared with the dashboard Q2.
//! * Dashboard Q5 completed count uses the literal `"completed"`, not the
//!   complement of Q4's `~Q(state__group__in=CLOSED)` — `cancelled`
//!   issues sit in neither count.
//! * Dashboard Q2 `month` defaults to the int `1`: with no `?month=` the
//!   buckets are always January's.
//! * Q6 ignores the year on both sides: ISO week 52 matches week 52 of
//!   any year.
//! * Q8/Q9 compare `DateField`s against `timezone.now()`; Django
//!   truncates the datetime to the UTC date, so time-of-day never
//!   matters.
//! * `UUID(int=True)` accepts JSON booleans for `last_workspace_id`
//!   (`True` → `…0001`), because `bool` subclasses `int`.
//! * `settings` is read-only on the serializer on purpose: whole-field
//!   writes stay rejected; the endpoint validates and merges instead.
//! * Merging `{"settings": {}}` still rewrites the stored bag through
//!   the dict-namespace filter, dropping non-dict namespaces.
//! * Non-dict PATCH bodies mostly 500: `null`/numbers/bools raise
//!   `TypeError` in the `:439` settings-membership check, and
//!   strings/lists that *contain* `"settings"` raise
//!   `AttributeError` on the following `.get` — only unfound
//!   strings/lists reach the serializer's non-dict 400.
//!
//! Deliberate non-ports (no observable difference under the contract
//! suite; noted for follow-up):
//!
//! * `settings.DEBUG` query-count print: dropped, not a response
//!   behavior.
//! * Malformed-body `detail` text uses serde's message where Django
//!   interpolates CPython's (`JSON parse error - …`); the key, the
//!   400 status, and the 415 media-type denial all match.
//! * A lone-surrogate JSON escape fails body parsing here while
//!   CPython parses it and fails field validation instead.
//! * `HEAD`/`TRACE`: axum's router answers where Django 405s (every
//!   ported domain shares this; untested either way).

use axum::body::Bytes;
use axum::extract::{Extension, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, SubsecRound, Utc};
use chrono_tz::Tz;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{
    accounts, issue_activities, issue_assignees, issues, profiles, projects, states, users,
    workspaces,
};
use pidash_db::orm_tx::TxConn;
use pidash_services::app_workspace::queries_core as core_q;
use pidash_services::app_workspace::queries_profile as profile_q;
use pidash_services::app_workspace::queries_user as user_q;
use pidash_services::app_workspace::ser_account_token as ser;
use pidash_services::user_settings::SettingsSchema;
use sea_orm::sea_query::{Alias, BinOper, Condition, Expr, ExprTrait, Func};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait,
};

use crate::middleware::SessionHandle;
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use pidash_compat::py::py_json_string;
pub use pidash_compat::response::{json_response, SERVER_ERROR_BODY};

/// Register the owned D-24 user-extras routes. Unowned methods fall
/// through to Django through the edge fallback (never a Rust 405).
pub fn routes() -> Router<AppState> {
    let proxy = crate::edge::proxy;
    Router::new()
        .route(
            "/api/users/me/profile/",
            get(get_profile)
                .patch(patch_profile)
                .post(proxy)
                .put(proxy)
                .delete(proxy)
                .head(proxy)
                .options(proxy),
        )
        .route(
            "/api/users/me/accounts/",
            get(list_accounts)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .head(proxy)
                .options(proxy),
        )
        .route(
            "/api/users/me/accounts/{pk}/",
            get(get_account)
                .delete(delete_account)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .head(proxy)
                .options(proxy),
        )
        .route(
            "/api/users/me/workspaces/{slug}/activity-graph/",
            get(activity_graph)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .head(proxy)
                .options(proxy),
        )
        .route(
            "/api/users/me/workspaces/{slug}/issues-completed-graph/",
            get(completed_graph)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .head(proxy)
                .options(proxy),
        )
        .route(
            "/api/users/me/workspaces/{slug}/dashboard/",
            get(dashboard)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .head(proxy)
                .options(proxy),
        )
}

// ---------------------------------------------------------------------------
// Error kernel (BaseAPIView.handle_exception + DRF denials)
// ---------------------------------------------------------------------------

/// 401 bytes: DRF `NotAuthenticated` through the default
/// `exception_handler` (`{'detail': exc.Detail}` — lowercase `detail`,
/// `rest_framework/views.py:96`; the project's `auth_exception_handler`
/// delegates). Reuses the merged D-24 guard const; pinned by the
/// live-Django contract `test_me_anon_denied`.
pub const UNAUTHENTICATED_BODY: &str = super::gates::ANON_BODY;
/// 404 bytes: `ObjectDoesNotExist` branch
/// (`app/views/base.py:132-136`).
pub const NOT_FOUND_BODY: &str = profile_q::OBJECT_NOT_FOUND_BODY;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum HandlerError {
    /// 401, DRF `NotAuthenticated` (no session on a guarded route).
    Unauthorized,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 400, serializer `errors` dict or `{"settings": [...]}` (pre-rendered).
    FieldErrors(String),
    /// 400/415, DRF `{"detail": ...}` bodies (parse errors, …) (pre-rendered).
    BadDetail(StatusCode, String),
    /// 500, generic branch (logged, like `log_exception`).
    ServerError,
}

impl HandlerError {
    pub fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            HandlerError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
            }
            HandlerError::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            HandlerError::FieldErrors(body) => (StatusCode::BAD_REQUEST, body.clone()),
            HandlerError::BadDetail(status, body) => (*status, body.clone()),
            HandlerError::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            // `log_exception(e)` (`app/views/base.py:145`).
            tracing::warn!("app_workspace user_extras handler: internal error");
        }
        let (status, body) = self.status_and_body();
        json_response(status, body)
    }
}

// ---------------------------------------------------------------------------
// Session actor (BaseSessionAuthentication without CSRF)
// ---------------------------------------------------------------------------

/// The request's user: `None` is anonymous. Mirrors DRF session auth:
/// a missing session, an unknown user id, or an inactive user all
/// authenticate as nobody (guarded routes then answer 401).
struct Actor {
    id: Uuid,
    timezone: Tz,
    /// `request.user.user_timezone`, bound as `:tzname` for the
    /// `__date` / `Extract*` shifts (activated-zone semantics).
    tz_name: String,
}

async fn request_actor(
    pool: &PgPool,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Option<Actor>, HandlerError> {
    let raw = extension
        .and_then(|Extension(handle)| {
            handle
                .snapshot()
                .get("_auth_user_id")
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .and_then(|raw| raw.parse::<Uuid>().ok());
    let id = match raw {
        Some(id) => id,
        None => return Ok(None),
    };
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(bool, Option<String>)> = users::Entity::find()
        .select_only()
        .columns([users::Column::IsActive, users::Column::UserTimezone])
        .filter(users::Column::Id.eq(id))
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((true, timezone)) => {
            // `TimezoneMixin.initial`: `timezone.activate(
            // ZoneInfo(user_timezone))` — an unknown zone (or a missing
            // one) raises into the 500 branch.
            let name = timezone.as_deref().unwrap_or("UTC");
            let tz: Tz = name.parse().map_err(|_| HandlerError::ServerError)?;
            Ok(Some(Actor {
                id,
                timezone: tz,
                tz_name: name.to_owned(),
            }))
        }
        _ => Ok(None),
    }
}

fn pool_of(state: &AppState) -> Result<&PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

/// Parse a UUID path segment. Django's `<uuid:pk>` converter leaves
/// the route unmatched for garbage, so the request must fall through
/// to Django (whose own 404 lives there) — hence the proxy, not a
/// Rust 404. The parse runs *before* auth: Django answers its
/// routing 404 without authenticating.
fn parse_pk(raw: &str) -> Option<Uuid> {
    raw.parse::<Uuid>().ok()
}

// ---------------------------------------------------------------------------
// Query + placeholder helpers
// ---------------------------------------------------------------------------

/// Decode one `application/x-www-form-urlencoded` value (`+` → space,
/// `%XX` → byte; malformed sequences pass through like Django's
/// forgiving unquote; lossy UTF-8 like Django's `errors="replace"`).
fn urldecode(raw: &str) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi * 16 + lo);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `request.GET.get(key, None)`: last value wins on repeats
/// (Django `QueryDict.get`), urldecoded. `None` when absent.
fn query_last(raw_query: &str, key: &str) -> Option<String> {
    let mut last: Option<&str> = None;
    for pair in raw_query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some((n, v)) => (n, v),
            None => (pair, ""),
        };
        if urldecode(name) == key {
            last = Some(value);
        }
    }
    last.map(urldecode)
}

/// Renumber the services layer's symbolic `:name` placeholders to
/// `$n`, where `n` is the 1-based position of `name` in `order`.
/// Every occurrence renumbers (the same name may bind twice, e.g.
/// `:tzname` in the dashboard Q2 select and filter). Casts (`::INT`)
/// and unknown names pass through untouched.
fn number_placeholders(sql: &str, order: &[&str]) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(colon) = rest.find(':') {
        let (head, tail) = rest.split_at(colon);
        out.push_str(head);
        let after = &tail[1..];
        let name_len: usize = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .map(char::len_utf8)
            .sum();
        let is_placeholder = after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if is_placeholder {
            let name = &after[..name_len];
            match order.iter().position(|want| *want == name) {
                Some(pos) => {
                    out.push('$');
                    out.push_str(&(pos + 1).to_string());
                }
                None => {
                    out.push(':');
                    out.push_str(name);
                }
            }
            rest = &after[name_len..];
        } else {
            // A bare colon (`::` casts, slices): literal, rescan after it.
            out.push(':');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// user_settings (pi_dash/core/user_settings.py + ee seam)
// ---------------------------------------------------------------------------

/// Ceiling on one PATCH's accepted payload, encoded as JSON
/// (`core/user_settings.py:24`).
pub const MAX_SETTINGS_PATCH_BYTES: usize = 4096;

/// The schema [`validate_settings_patch`] checks against is the
/// injectable seam, carried on [`AppState`] (`user_settings_schema`):
/// CE keeps the empty default, so the bag stays empty and every write
/// is rejected; the overlay installs its extended schema instead
/// (`ee/user_settings.py`), and declaring a key is what makes it
/// writable through the profile API. The validate/merge logic below
/// stays a line-for-line port of `core/user_settings.py` either way.
///
/// Read the type declaration off one declared default value: a
/// `null` default accepts any scalar, a number default dispatches on
/// whether it is a float (`type(default)`, `_check_value`).
fn setting_default_kind(default: &serde_json::Value) -> SettingDefault {
    match default {
        serde_json::Value::Null => SettingDefault::Null,
        serde_json::Value::Bool(_) => SettingDefault::Bool,
        serde_json::Value::Number(number) => {
            if number.is_f64() {
                SettingDefault::Float
            } else {
                SettingDefault::Int
            }
        }
        serde_json::Value::String(_) => SettingDefault::Str,
        serde_json::Value::Array(_) => SettingDefault::List,
        serde_json::Value::Object(_) => SettingDefault::Dict,
    }
}

/// A schema default doubling as the type declaration
/// (`_check_value`, `core/user_settings.py:87-112`). Modelled as the
/// JSON value so `type(default)` dispatches exactly like Python.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingDefault {
    Null,
    Bool,
    /// An int default (any magnitude): accepts JSON integer tokens.
    Int,
    /// A float default: accepts JSON ints and floats.
    Float,
    Str,
    List,
    Dict,
}

/// Reject a value whose type does not match the key's declared
/// default (`_check_value`). `bool` is checked before `int` because
/// it subclasses it.
fn check_setting_value(
    namespace: &str,
    key: &str,
    value: &serde_json::Value,
    default: SettingDefault,
) -> Result<(), String> {
    let type_name = match default {
        SettingDefault::Null => "NoneType",
        SettingDefault::Bool => "bool",
        SettingDefault::Int => "int",
        SettingDefault::Float => "float",
        SettingDefault::Str => "str",
        SettingDefault::List => "list",
        SettingDefault::Dict => "dict",
    };
    let ok = match default {
        // Nothing to infer a type from; allow scalars, refuse containers.
        SettingDefault::Null => !matches!(
            value,
            serde_json::Value::Object(_) | serde_json::Value::Array(_)
        ),
        SettingDefault::Bool => matches!(value, serde_json::Value::Bool(_)),
        // `isinstance(value, int) and not isinstance(value, bool)`:
        // an integer JSON token that is not a boolean.
        SettingDefault::Int => !matches!(value, serde_json::Value::Bool(_)) && is_json_int(value),
        // `isinstance(value, (int, float)) and not isinstance(value, bool)`.
        SettingDefault::Float => {
            !matches!(value, serde_json::Value::Bool(_))
                && matches!(value, serde_json::Value::Number(_))
        }
        SettingDefault::Str => matches!(value, serde_json::Value::String(_)),
        SettingDefault::List => matches!(value, serde_json::Value::Array(_)),
        SettingDefault::Dict => matches!(value, serde_json::Value::Object(_)),
    };
    if ok {
        Ok(())
    } else if default == SettingDefault::Null {
        Err(format!("settings.{namespace}.{key} must be a scalar"))
    } else {
        Err(format!(
            "settings.{namespace}.{key} must be of type {type_name}"
        ))
    }
}

/// Whether a JSON value is an integer token (`1`, `-0`, `1_2` never
/// reaches us — JSON has no underscores): no `.`/`e`/`E`, which
/// `arbitrary_precision` preserves verbatim.
fn is_json_int(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(n) => !n
            .to_string()
            .bytes()
            .any(|b| b == b'.' || b == b'e' || b == b'E'),
        _ => false,
    }
}

/// Validate a client-supplied `settings` payload against the schema
/// (`validate_settings_patch`, `core/user_settings.py:115-155`).
/// Returns the accepted patch; raises the human-readable message on
/// anything unrecognised. The caller supplies the schema — the
/// endpoint reads it off [`AppState`] — so the overlay can declare its
/// own namespaces without touching this port.
fn validate_settings_patch(
    schema: &SettingsSchema,
    patch: &serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let serde_json::Value::Object(namespaces) = patch else {
        return Err("settings must be an object".to_owned());
    };
    let mut accepted = serde_json::Map::with_capacity(namespaces.len());
    for (namespace, values) in namespaces {
        let Some(keys) = schema.get(namespace) else {
            return Err(format!("unknown settings namespace: {namespace}"));
        };
        let serde_json::Value::Object(entries) = values else {
            return Err(format!("settings.{namespace} must be an object"));
        };
        let mut unknown: Vec<&str> = entries
            .keys()
            .filter(|key| !keys.contains_key(key.as_str()))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            unknown.sort_unstable();
            return Err(format!(
                "unknown settings key(s) in {namespace}: {}",
                unknown.join(", ")
            ));
        }
        for (key, value) in entries {
            let default = keys.get(key.as_str()).expect("key is known");
            check_setting_value(namespace, key, value, setting_default_kind(default))?;
        }
        accepted.insert(
            namespace.clone(),
            serde_json::Value::Object(entries.clone()),
        );
    }
    let encoded = py_json_dumps_len(&serde_json::Value::Object(accepted.clone()));
    if encoded > MAX_SETTINGS_PATCH_BYTES {
        return Err(format!(
            "settings payload is too large ({encoded} > {MAX_SETTINGS_PATCH_BYTES} bytes)"
        ));
    }
    Ok(accepted)
}

/// Merge a validated patch into the stored bag, per namespace
/// (`merge_settings`, `core/user_settings.py:158-167`). The `(stored
/// or {})` uses Python truthiness: every falsy non-dict (null,
/// false, `0`, `""`, `[]`) merges as `{}`; a truthy non-dict raises
/// `AttributeError` (`.items()`), which is the 500 branch.
fn merge_settings(
    stored: &serde_json::Value,
    patch: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, HandlerError> {
    let mut merged = serde_json::Map::new();
    match stored {
        serde_json::Value::Object(namespaces) => {
            for (namespace, values) in namespaces {
                if let serde_json::Value::Object(entries) = values {
                    merged.insert(
                        namespace.clone(),
                        serde_json::Value::Object(entries.clone()),
                    );
                }
            }
        }
        // Falsy non-dicts behave as `{}`.
        serde_json::Value::Null => {}
        serde_json::Value::Bool(false) => {}
        serde_json::Value::Number(n) if is_json_zero(n) => {}
        serde_json::Value::String(s) if s.is_empty() => {}
        serde_json::Value::Array(items) if items.is_empty() => {}
        // Truthy non-dicts: `(stored or {})` keeps them and `.items()`
        // raises `AttributeError` → generic 500 branch.
        _ => return Err(HandlerError::ServerError),
    }
    for (namespace, values) in patch {
        let serde_json::Value::Object(entries) = values else {
            // Unreachable: validated patches only carry objects.
            return Err(HandlerError::ServerError);
        };
        merged
            .entry(namespace.clone())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .expect("namespace entries are objects")
            .extend(entries.clone());
    }
    Ok(serde_json::Value::Object(merged))
}

/// Whether a JSON number token is a zero (`0`, `-0`, `0.0`, `0e3`):
/// Python-falsy, so `(stored or {})` drops it.
fn is_json_zero(n: &serde_json::Number) -> bool {
    let token = n.to_string();
    let digits = token.trim_start_matches(['+', '-']);
    let mantissa = digits
        .split(['e', 'E'])
        .next()
        .unwrap_or(digits)
        .replace('.', "");
    !mantissa.is_empty() && mantissa.bytes().all(|b| b == b'0')
}

/// Byte length of CPython `json.dumps(value)` with the defaults
/// (`ensure_ascii=True`, `(',', ': ')` separators, insertion order):
/// what `validate_settings_patch` measures the 4096 cap with.
fn py_json_dumps_len(value: &serde_json::Value) -> usize {
    py_json_dumps(value).len()
}

fn py_json_dumps(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(true) => "true".to_owned(),
        serde_json::Value::Bool(false) => "false".to_owned(),
        serde_json::Value::Number(n) => py_json_number(n),
        serde_json::Value::String(s) => py_json_string(s),
        serde_json::Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&py_json_dumps(item));
            }
            out.push(']');
            out
        }
        serde_json::Value::Object(entries) => {
            let mut out = String::from("{");
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&py_json_string(key));
                out.push_str(": ");
                out.push_str(&py_json_dumps(item));
            }
            out.push('}');
            out
        }
    }
}

/// CPython `json.dumps` number encoding: ints verbatim (`-0` parses
/// to `0`), floats as `repr` (`Infinity`/`-Infinity` past range).
fn py_json_number(n: &serde_json::Number) -> String {
    let token = n.to_string();
    if token.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
        let value: f64 = token.parse().unwrap_or(f64::NAN);
        py_float_str(value)
    } else {
        let (negative, digits) = match token.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, token.as_str()),
        };
        let stripped = digits.trim_start_matches('0');
        if stripped.is_empty() {
            "0".to_owned()
        } else if negative {
            format!("-{stripped}")
        } else {
            stripped.to_owned()
        }
    }
}

/// Python `repr()` of a float: shortest round-trip digits with
/// `e±XX` exponents (`1.0`, `1.5`, `1e+16`, `1e-05`),
/// `Infinity`/`-Infinity` past range.
fn py_float_str(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    let rendered = format!("{n:?}");
    let Some(pos) = rendered.find('e') else {
        return rendered;
    };
    let (mantissa, exp) = rendered.split_at(pos);
    let exp: i32 = exp[1..].parse().unwrap_or(0);
    format!("{mantissa}e{exp:+03}")
}

// ---------------------------------------------------------------------------
// PATCH body + ProfileSerializer validation
// ---------------------------------------------------------------------------

/// Decode the PATCH body like DRF: empty → `{}`; otherwise JSON by
/// content-type (anything else → 415 `UnsupportedMediaType`), parse
/// errors → 400 `ParseError`. Both denial bodies use the lowercase
/// `detail` key (`exception_handler`); only the interpolated message
/// text differs (serde's, not CPython's).
fn patch_body(headers: &HeaderMap, body: &[u8]) -> Result<serde_json::Value, HandlerError> {
    if body.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !media_type.is_empty() && media_type != "application/json" {
        return Err(HandlerError::BadDetail(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            serde_json::json!({
                "detail": format!("Unsupported media type \"{content_type}\" in request.")
            })
            .to_string(),
        ));
    }
    serde_json::from_slice(body).map_err(|e| {
        HandlerError::BadDetail(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"detail": format!("JSON parse error - {e}")}).to_string(),
        )
    })
}

/// Run the PATCH prelude over a decoded body: the `:439` settings
/// membership check (which 500s most non-dict bodies before the
/// serializer runs) and the serializer's non-dict 400. Returns the
/// body object on the dict path.
fn patch_object(
    value: &serde_json::Value,
) -> Result<&serde_json::Map<String, serde_json::Value>, HandlerError> {
    // `if "settings" in request.data`: `None`/numbers/bools raise
    // `TypeError` (500); strings check substring and lists
    // membership, and either raises `AttributeError` on the
    // following `.get("settings")` (500) when "settings" is found —
    // only unfound strings/lists reach the serializer's non-dict
    // 400. (The serializer's own "No data provided" for `None` is
    // unreachable: `None` 500s here first.)
    let settings_present = match value {
        serde_json::Value::Object(object) => object.contains_key("settings"),
        serde_json::Value::String(text) => text.contains("settings"),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| item.as_str() == Some("settings"))
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            return Err(HandlerError::ServerError)
        }
    };
    if settings_present && !value.is_object() {
        return Err(HandlerError::ServerError);
    }
    match value {
        serde_json::Value::Object(object) => Ok(object),
        other => {
            let message = format!(
                "Invalid data. Expected a dictionary, but got {}.",
                datatype_name(other)
            );
            Err(HandlerError::FieldErrors(
                serde_json::json!({"non_field_errors": [message]}).to_string(),
            ))
        }
    }
}

/// JSON value kinds by DRF's `type(data).__name__` for the non-dict
/// error (`serializers.py:340`). Python ints are unbounded, so every
/// integer token — however large — is `"int"`: the token shape (no
/// `.`/`e`/`E`) discriminates, which `arbitrary_precision` preserves
/// verbatim.
fn datatype_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "NoneType",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(n) => {
            if n.to_string()
                .bytes()
                .any(|b| b == b'.' || b == b'e' || b == b'E')
            {
                "float"
            } else {
                "int"
            }
        }
        serde_json::Value::String(_) => "str",
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "dict",
    }
}

/// Python `str()` of a JSON value: bare strings, `repr()` otherwise
/// (`ChoiceField` lookup key and `invalid_choice` input rendering).
fn py_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python `repr()` of a JSON value (container elements, dict keys).
fn py_repr(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(n) => py_num_str(n),
        serde_json::Value::String(s) => py_repr_str(s),
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        serde_json::Value::Object(entries) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_str(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `str()` of a JSON number: decimal for ints, `repr` for floats.
fn py_num_str(n: &serde_json::Number) -> String {
    let token = n.to_string();
    if token.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
        let value: f64 = token.parse().unwrap_or(f64::NAN);
        py_float_str(value)
    } else {
        py_json_number(n)
    }
}

/// Python `repr()` of a string: single quotes unless the value holds
/// one (and no double quote), with backslash and control escapes.
fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            // Non-printables escape as `\xXX` / `\uXXXX` / `\UXXXXXXXX`.
            c if c.is_control() || !is_py_printable(c) => {
                let code = c as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Approximation of Python `str.isprintable` for `repr` escaping:
/// controls and whitespace beyond ASCII space escape. Exact except
/// for `Cf`/`Co`/`Cn` (format, private-use, unassigned) characters,
/// which CPython also escapes but stable Rust cannot classify — an
/// error-message-only path where the divergence is invisible.
fn is_py_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    !c.is_control() && !c.is_whitespace()
}

/// One validated column write for the profile full-row save.
#[derive(Debug, Clone, PartialEq)]
enum Assignment {
    Text(&'static str, String),
    NullableText(&'static str, Option<String>),
    Flag(&'static str, bool),
    NullableUuid(&'static str, Option<Uuid>),
    ChoiceInt(&'static str, i16),
    Json(&'static str, serde_json::Value),
}

impl Assignment {
    fn field(&self) -> &'static str {
        match self {
            Assignment::Text(name, _)
            | Assignment::NullableText(name, _)
            | Assignment::Flag(name, _)
            | Assignment::NullableUuid(name, _)
            | Assignment::ChoiceInt(name, _)
            | Assignment::Json(name, _) => name,
        }
    }
}

/// Writable `ProfileSerializer` fields in validation order (model
/// declaration order minus read-only `user`/`settings`/timestamps),
/// each with its DRF field kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    /// `serializers.JSONField`, `allow_null` from the model.
    Json { allow_null: bool },
    /// `serializers.BooleanField` (never nullable on this model).
    Flag,
    /// `serializers.CharField`: `allow_blank`, `allow_null`,
    /// `max_length` from the model.
    Text {
        allow_blank: bool,
        allow_null: bool,
        max_length: Option<usize>,
    },
    /// `serializers.UUIDField`, `allow_null` from the model.
    Uuid { allow_null: bool },
    /// `serializers.ChoiceField` over `&str` keys (text choices).
    TextChoice { allow_blank: bool },
    /// `serializers.ChoiceField` over `0..=6` (week-day choices).
    IntChoice,
}

/// (`field`, `kind`) in `Profile` declaration order
/// (`db/models/user.py:223-269`), read-only fields excluded.
const WRITABLE_FIELDS: &[(&str, FieldKind)] = &[
    ("theme", FieldKind::Json { allow_null: false }),
    ("is_app_rail_docked", FieldKind::Flag),
    ("is_tour_completed", FieldKind::Flag),
    ("onboarding_step", FieldKind::Json { allow_null: false }),
    (
        "use_case",
        FieldKind::Text {
            allow_blank: true,
            allow_null: true,
            max_length: None,
        },
    ),
    (
        "role",
        FieldKind::Text {
            allow_blank: true,
            allow_null: true,
            max_length: Some(300),
        },
    ),
    ("is_onboarded", FieldKind::Flag),
    ("last_workspace_id", FieldKind::Uuid { allow_null: true }),
    (
        "billing_address_country",
        FieldKind::Text {
            allow_blank: false,
            allow_null: false,
            max_length: Some(255),
        },
    ),
    ("billing_address", FieldKind::Json { allow_null: true }),
    ("has_billing_address", FieldKind::Flag),
    (
        "company_name",
        FieldKind::Text {
            allow_blank: true,
            allow_null: false,
            max_length: Some(255),
        },
    ),
    (
        "notification_view_mode",
        FieldKind::TextChoice { allow_blank: false },
    ),
    ("is_smooth_cursor_enabled", FieldKind::Flag),
    ("is_mobile_onboarded", FieldKind::Flag),
    (
        "mobile_onboarding_step",
        FieldKind::Json { allow_null: false },
    ),
    ("mobile_timezone_auto_set", FieldKind::Flag),
    (
        "language",
        FieldKind::Text {
            allow_blank: false,
            allow_null: false,
            max_length: Some(255),
        },
    ),
    ("start_of_the_week", FieldKind::IntChoice),
    ("goals", FieldKind::Json { allow_null: false }),
    (
        "background_color",
        FieldKind::Text {
            allow_blank: false,
            allow_null: false,
            max_length: Some(255),
        },
    ),
    ("is_navigation_tour_completed", FieldKind::Flag),
    ("has_marketing_email_consent", FieldKind::Flag),
    ("is_subscribed_to_changelog", FieldKind::Flag),
    ("product_tour", FieldKind::Json { allow_null: false }),
];

const NOTIFICATION_VIEW_MODES: &[&str] = &["full", "compact"];

/// Validate one supplied PATCH value like DRF `run_validation`:
/// blank pre-check (CharField only), null check, `to_internal_value`,
/// then the field validators (collected, in order). Returns the
/// column write, or the field's error list.
fn validate_field(
    name: &'static str,
    kind: FieldKind,
    value: &serde_json::Value,
) -> Result<Assignment, Vec<String>> {
    // `validate_empty_values`: explicit null on a non-nullable field
    // fails — partial mode skips only ABSENT keys (`SkipField`).
    if value.is_null() {
        let allow_null = matches!(
            kind,
            FieldKind::Json { allow_null: true }
                | FieldKind::Text {
                    allow_null: true,
                    ..
                }
                | FieldKind::Uuid { allow_null: true }
        );
        if !allow_null {
            return Err(vec!["This field may not be null.".to_owned()]);
        }
        return Ok(match kind {
            FieldKind::Json { .. } => Assignment::Json(name, serde_json::Value::Null),
            FieldKind::Text { .. } => Assignment::NullableText(name, None),
            FieldKind::Uuid { .. } => Assignment::NullableUuid(name, None),
            FieldKind::Flag | FieldKind::TextChoice { .. } | FieldKind::IntChoice => {
                unreachable!("non-nullable kinds fail above")
            }
        });
    }
    match kind {
        FieldKind::Json { .. } => {
            // `JSONField.to_internal_value`: `json.dumps` round-check,
            // which parsed JSON always survives; the value passes
            // through untouched.
            Ok(Assignment::Json(name, value.clone()))
        }
        FieldKind::Flag => as_bool(value)
            .map(|flag| Assignment::Flag(name, flag))
            .map_err(|()| vec!["Must be a valid boolean.".to_owned()]),
        FieldKind::Text {
            allow_blank,
            max_length,
            ..
        } => {
            // `CharField.run_validation` blank pre-check (runs before
            // `to_internal_value`): only strings can be blank.
            if let serde_json::Value::String(s) = value {
                if s.trim().is_empty() {
                    if !allow_blank {
                        return Err(vec!["This field may not be blank.".to_owned()]);
                    }
                    return Ok(Assignment::NullableText(name, Some(String::new())));
                }
            }
            let text = as_text(value).map_err(|()| vec!["Not a valid string.".to_owned()])?;
            let mut errors = Vec::new();
            if let Some(max) = max_length {
                // `MaxLengthValidator`: code points, on the trimmed value.
                if text.chars().count() > max {
                    errors.push(format!(
                        "Ensure this field has no more than {max} characters."
                    ));
                }
            }
            if text.contains('\0') {
                errors.push("Null characters are not allowed.".to_owned());
            }
            // `ProhibitSurrogateCharactersValidator` is unreachable:
            // lone surrogates fail JSON parsing before validation.
            if errors.is_empty() {
                Ok(Assignment::NullableText(name, Some(text)))
            } else {
                Err(errors)
            }
        }
        FieldKind::Uuid { .. } => as_uuid(value)
            .map(|id| Assignment::NullableUuid(name, Some(id)))
            .map_err(|()| vec!["Must be a valid UUID.".to_owned()]),
        FieldKind::TextChoice { allow_blank } => {
            if *value == serde_json::Value::String(String::new()) && allow_blank {
                return Ok(Assignment::Text(name, String::new()));
            }
            let key = py_str(value);
            if NOTIFICATION_VIEW_MODES.contains(&key.as_str()) {
                Ok(Assignment::Text(name, key))
            } else {
                Err(vec![format!("\"{key}\" is not a valid choice.")])
            }
        }
        FieldKind::IntChoice => {
            let key = py_str(value);
            match key.parse::<i64>() {
                Ok(choice) if (0..=6).contains(&choice) && key == choice.to_string() => {
                    Ok(Assignment::ChoiceInt(name, choice as i16))
                }
                _ => Err(vec![format!("\"{key}\" is not a valid choice.")]),
            }
        }
    }
}

/// DRF `BooleanField.to_internal_value` (case-insensitive string
/// sets; `1`/`1.0` true, `0`/`0.0` false; unhashables invalid).
fn as_bool(value: &serde_json::Value) -> Result<bool, ()> {
    match value {
        serde_json::Value::Bool(flag) => Ok(*flag),
        serde_json::Value::String(s) => match s.to_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(()),
        },
        serde_json::Value::Number(n) => {
            if let Some(int) = n.as_i64() {
                if int == 1 {
                    return Ok(true);
                }
                if int == 0 {
                    return Ok(false);
                }
                return Err(());
            }
            // Beyond `i64`: only an exact `1`/`0` float spell-out
            // below can still match (`2**64` is invalid either way).
            if n.as_u64().is_some() {
                return Err(());
            }
            if n.as_f64() == Some(1.0) {
                Ok(true)
            } else if n.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

/// DRF `CharField.to_internal_value`: bools and composites invalid;
/// numbers coerce via `str()`; strings (and coerced numbers) strip.
fn as_text(value: &serde_json::Value) -> Result<String, ()> {
    match value {
        serde_json::Value::String(s) => Ok(s.trim().to_owned()),
        serde_json::Value::Number(n) => Ok(py_num_str(n).trim().to_owned()),
        _ => Err(()),
    }
}

/// DRF `UUIDField.to_internal_value`: bools take the `int` path
/// (`True` → `…0001`, a ported quirk); ints must fit `u128`
/// (`-0` parses to `0`); strings go through `UUID(hex=…)`.
fn as_uuid(value: &serde_json::Value) -> Result<Uuid, ()> {
    match value {
        serde_json::Value::Bool(flag) => Ok(Uuid::from_u128(u128::from(*flag as u8))),
        serde_json::Value::Number(n) => {
            let token = n.to_string();
            if token.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
                return Err(());
            }
            let (negative, digits) = match token.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, token.as_str()),
            };
            if digits.bytes().all(|b| b == b'0') {
                return Ok(Uuid::nil());
            }
            if negative {
                return Err(());
            }
            digits.parse::<u128>().map(Uuid::from_u128).map_err(|_| ())
        }
        serde_json::Value::String(raw) => raw.parse::<Uuid>().map_err(|_| ()),
        _ => Err(()),
    }
}

/// Validate the PATCH object like
/// `ProfileSerializer(partial=True).is_valid()`: writable fields in
/// declaration order; read-only (`user`, `settings`, `id`,
/// timestamps) and unknown keys ignored. Errors render in field
/// order.
fn validate_partial(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<Assignment>, Vec<(String, Vec<String>)>> {
    let mut sets = Vec::new();
    let mut errors = Vec::new();
    for (name, kind) in WRITABLE_FIELDS {
        if let Some(value) = object.get(*name) {
            match validate_field(name, *kind, value) {
                Ok(set) => sets.push(set),
                Err(field_errors) => errors.push((name.to_string(), field_errors)),
            }
        }
    }
    if errors.is_empty() {
        Ok(sets)
    } else {
        Err(errors)
    }
}

/// Render `serializer.errors` (`{field: [messages]}` in field order).
fn render_field_errors(errors: &[(String, Vec<String>)]) -> String {
    let mut body = serde_json::Map::with_capacity(errors.len());
    for (field, messages) in errors {
        body.insert(field.clone(), serde_json::json!(messages));
    }
    serde_json::Value::Object(body).to_string()
}

// ---------------------------------------------------------------------------
// Row structs + rendering (Account list/detail, Profile get)
// ---------------------------------------------------------------------------

/// An `accounts` row as read (`db/models/user.py:280-304`).
struct DbAccount {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    provider_account_id: String,
    provider: String,
    access_token: String,
    access_token_expired_at: Option<DateTime<Utc>>,
    refresh_token: Option<String>,
    refresh_token_expired_at: Option<DateTime<Utc>>,
    last_connected_at: DateTime<Utc>,
    id_token: String,
    metadata: serde_json::Value,
    user_id: Uuid,
}

impl DbAccount {
    fn from_model(model: accounts::Model) -> Self {
        Self {
            id: model.id,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
            provider_account_id: model.provider_account_id,
            provider: model.provider,
            access_token: model.access_token,
            access_token_expired_at: model
                .access_token_expired_at
                .map(|value| value.with_timezone(&Utc)),
            refresh_token: model.refresh_token,
            refresh_token_expired_at: model
                .refresh_token_expired_at
                .map(|value| value.with_timezone(&Utc)),
            last_connected_at: model.last_connected_at.with_timezone(&Utc),
            id_token: model.id_token,
            metadata: model.metadata,
            user_id: model.user_id,
        }
    }
}

/// Render one account as `AccountSerializer` output
/// (`serializers/user.py:214-218`), datetimes in the request zone.
fn render_account(row: &DbAccount, timezone: &Tz) -> serde_json::Value {
    let id = row.id.to_string();
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let access_token_expired_at = row
        .access_token_expired_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, timezone));
    let refresh_token_expired_at = row
        .refresh_token_expired_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, timezone));
    let last_connected_at = render_datetime_in(&row.last_connected_at, timezone);
    let user = row.user_id.to_string();
    let view_row = ser::AccountRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        provider_account_id: &row.provider_account_id,
        provider: &row.provider,
        access_token: &row.access_token,
        access_token_expired_at: access_token_expired_at.as_deref(),
        refresh_token: row.refresh_token.as_deref(),
        refresh_token_expired_at: refresh_token_expired_at.as_deref(),
        last_connected_at: &last_connected_at,
        id_token: &row.id_token,
        metadata: &row.metadata,
        user: &user,
    };
    serde_json::to_value(ser::account_to_representation(&view_row))
        .expect("account view serializes")
}

/// A `profiles` row as read (`db/models/user.py:200-277`).
#[derive(Debug, Clone)]
struct DbProfile {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    user_id: Uuid,
    theme: serde_json::Value,
    is_app_rail_docked: bool,
    is_tour_completed: bool,
    onboarding_step: serde_json::Value,
    use_case: Option<String>,
    role: Option<String>,
    is_onboarded: bool,
    last_workspace_id: Option<Uuid>,
    billing_address_country: String,
    billing_address: Option<serde_json::Value>,
    has_billing_address: bool,
    company_name: String,
    notification_view_mode: String,
    is_smooth_cursor_enabled: bool,
    is_mobile_onboarded: bool,
    mobile_onboarding_step: serde_json::Value,
    mobile_timezone_auto_set: bool,
    language: String,
    start_of_the_week: i16,
    goals: serde_json::Value,
    background_color: String,
    is_navigation_tour_completed: bool,
    has_marketing_email_consent: bool,
    is_subscribed_to_changelog: bool,
    product_tour: serde_json::Value,
    settings: serde_json::Value,
}

impl DbProfile {
    fn from_model(model: profiles::Model) -> Self {
        Self {
            id: model.id,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
            user_id: model.user_id,
            theme: model.theme,
            is_app_rail_docked: model.is_app_rail_docked,
            is_tour_completed: model.is_tour_completed,
            onboarding_step: model.onboarding_step,
            use_case: model.use_case,
            role: model.role,
            is_onboarded: model.is_onboarded,
            last_workspace_id: model.last_workspace_id,
            billing_address_country: model.billing_address_country,
            billing_address: model.billing_address,
            has_billing_address: model.has_billing_address,
            company_name: model.company_name,
            notification_view_mode: model.notification_view_mode,
            is_smooth_cursor_enabled: model.is_smooth_cursor_enabled,
            is_mobile_onboarded: model.is_mobile_onboarded,
            mobile_onboarding_step: model.mobile_onboarding_step,
            mobile_timezone_auto_set: model.mobile_timezone_auto_set,
            language: model.language,
            start_of_the_week: model.start_of_the_week,
            goals: model.goals,
            background_color: model.background_color,
            is_navigation_tour_completed: model.is_navigation_tour_completed,
            has_marketing_email_consent: model.has_marketing_email_consent,
            is_subscribed_to_changelog: model.is_subscribed_to_changelog,
            product_tour: model.product_tour,
            settings: model.settings,
        }
    }

    /// Apply validated PATCH writes (`ModelSerializer.update` assigns
    /// each validated attr onto the instance before `save()`).
    fn apply(&mut self, sets: &[Assignment]) {
        for set in sets {
            match set {
                Assignment::Text("notification_view_mode", value) => {
                    self.notification_view_mode = value.clone();
                }
                Assignment::NullableText("use_case", value) => self.use_case = value.clone(),
                Assignment::NullableText("role", value) => self.role = value.clone(),
                Assignment::NullableText("billing_address_country", value) => {
                    if let Some(text) = value {
                        self.billing_address_country = text.clone();
                    }
                }
                Assignment::NullableText("company_name", value) => {
                    if let Some(text) = value {
                        self.company_name = text.clone();
                    }
                }
                Assignment::NullableText("language", value) => {
                    if let Some(text) = value {
                        self.language = text.clone();
                    }
                }
                Assignment::NullableText("background_color", value) => {
                    if let Some(text) = value {
                        self.background_color = text.clone();
                    }
                }
                Assignment::Flag("is_app_rail_docked", value) => self.is_app_rail_docked = *value,
                Assignment::Flag("is_tour_completed", value) => self.is_tour_completed = *value,
                Assignment::Flag("is_onboarded", value) => self.is_onboarded = *value,
                Assignment::Flag("has_billing_address", value) => self.has_billing_address = *value,
                Assignment::Flag("is_smooth_cursor_enabled", value) => {
                    self.is_smooth_cursor_enabled = *value;
                }
                Assignment::Flag("is_mobile_onboarded", value) => self.is_mobile_onboarded = *value,
                Assignment::Flag("mobile_timezone_auto_set", value) => {
                    self.mobile_timezone_auto_set = *value;
                }
                Assignment::Flag("is_navigation_tour_completed", value) => {
                    self.is_navigation_tour_completed = *value;
                }
                Assignment::Flag("has_marketing_email_consent", value) => {
                    self.has_marketing_email_consent = *value;
                }
                Assignment::Flag("is_subscribed_to_changelog", value) => {
                    self.is_subscribed_to_changelog = *value;
                }
                Assignment::NullableUuid("last_workspace_id", value) => {
                    self.last_workspace_id = *value;
                }
                Assignment::ChoiceInt("start_of_the_week", value) => {
                    self.start_of_the_week = *value;
                }
                Assignment::Json("theme", value) => self.theme = value.clone(),
                Assignment::Json("onboarding_step", value) => self.onboarding_step = value.clone(),
                Assignment::Json("billing_address", value) => {
                    if value.is_null() {
                        self.billing_address = None;
                    } else {
                        self.billing_address = Some(value.clone());
                    }
                }
                Assignment::Json("mobile_onboarding_step", value) => {
                    self.mobile_onboarding_step = value.clone();
                }
                Assignment::Json("goals", value) => self.goals = value.clone(),
                Assignment::Json("product_tour", value) => self.product_tour = value.clone(),
                other => panic!("validator emitted no such assignment: {:?}", other.field()),
            }
        }
    }
}

/// Render one profile as `ProfileSerializer` output
/// (`serializers/user.py:201-211`), datetimes in the request zone.
fn render_profile(row: &DbProfile, timezone: &Tz) -> serde_json::Value {
    let id = row.id.to_string();
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let last_workspace_id = row.last_workspace_id.as_ref().map(Uuid::to_string);
    let user = row.user_id.to_string();
    let view_row = ser::ProfileRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        theme: &row.theme,
        is_app_rail_docked: row.is_app_rail_docked,
        is_tour_completed: row.is_tour_completed,
        onboarding_step: &row.onboarding_step,
        use_case: row.use_case.as_deref(),
        role: row.role.as_deref(),
        is_onboarded: row.is_onboarded,
        last_workspace_id: last_workspace_id.as_deref(),
        billing_address_country: &row.billing_address_country,
        billing_address: row.billing_address.as_ref(),
        has_billing_address: row.has_billing_address,
        company_name: &row.company_name,
        notification_view_mode: &row.notification_view_mode,
        is_smooth_cursor_enabled: row.is_smooth_cursor_enabled,
        is_mobile_onboarded: row.is_mobile_onboarded,
        mobile_onboarding_step: &row.mobile_onboarding_step,
        mobile_timezone_auto_set: row.mobile_timezone_auto_set,
        language: &row.language,
        start_of_the_week: i32::from(row.start_of_the_week),
        goals: &row.goals,
        background_color: &row.background_color,
        is_navigation_tour_completed: row.is_navigation_tour_completed,
        has_marketing_email_consent: row.has_marketing_email_consent,
        is_subscribed_to_changelog: row.is_subscribed_to_changelog,
        product_tour: &row.product_tour,
        settings: &row.settings,
        user: &user,
    };
    serde_json::to_value(ser::profile_to_representation(&view_row))
        .expect("profile view serializes")
}

// ---------------------------------------------------------------------------
// AccountEndpoint (user/base.py:406-421)
// ---------------------------------------------------------------------------

/// `AccountEndpoint.get` without `pk` (`:413-415`): the caller's
/// accounts, `-created_at` first.
async fn list_accounts(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(&state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = accounts::Entity::find()
        .filter(accounts::Column::UserId.eq(actor.id))
        .order_by_desc(accounts::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(render_account(&DbAccount::from_model(row), &actor.timezone));
    }
    Ok(json_response(
        StatusCode::OK,
        serde_json::Value::Array(items).to_string(),
    ))
}

/// `AccountEndpoint.get` with `pk` (`:407-411`): user-scoped lookup;
/// a miss (another user's row included) is 404.
async fn get_account(
    State(state): State<AppState>,
    Path(pk_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    match get_account_inner(&state, pk, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn get_account_inner(
    state: &AppState,
    pk: Uuid,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = accounts::Entity::find()
        .filter(accounts::Column::Id.eq(pk))
        .filter(accounts::Column::UserId.eq(actor.id))
        .one(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let Some(row) = row else {
        return Err(HandlerError::NotFound);
    };
    Ok(json_response(
        StatusCode::OK,
        render_account(&DbAccount::from_model(row), &actor.timezone).to_string(),
    ))
}

/// `AccountEndpoint.delete` (`:417-420`): scoped lookup (404 like
/// detail), then a HARD `DELETE` — `Account` has no soft-delete
/// mixin — 204 with an empty body.
async fn delete_account(
    State(state): State<AppState>,
    Path(pk_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    match delete_account_inner(&state, pk, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn delete_account_inner(
    state: &AppState,
    pk: Uuid,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = accounts::Entity::find()
        .filter(accounts::Column::Id.eq(pk))
        .filter(accounts::Column::UserId.eq(actor.id))
        .one(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    if row.is_none() {
        return Err(HandlerError::NotFound);
    }
    let delete = number_placeholders(&user_q::account_hard_delete_sql(), &["pk"]);
    // raw-sql: domain-covered table write; the domain builder owns the statement.
    sqlx::query(&delete)
        .bind(pk)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// ProfileEndpoint (user/base.py:423-462)
// ---------------------------------------------------------------------------

/// `ProfileEndpoint.get` (`:423-429`): the caller's profile with
/// `Cache-Control: private, max-age=12` + `Vary: Cookie`; a missing
/// row is 404.
async fn get_profile(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(&state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = profiles::Entity::find()
        .filter(profiles::Column::UserId.eq(actor.id))
        .one(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let Some(row) = row else {
        return Err(HandlerError::NotFound);
    };
    let body = render_profile(&DbProfile::from_model(row), &actor.timezone).to_string();
    let headers = super::gates::headers_for("GET", "users/me/profile/");
    let mut response = json_response(StatusCode::OK, body);
    if let Some(cache_control) = headers.cache_control {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            cache_control.parse().expect("static cache-control"),
        );
    }
    if let Some(vary) = headers.vary {
        response
            .headers_mut()
            .insert(axum::http::header::VARY, vary.parse().expect("static vary"));
    }
    Ok(response)
}

/// `ProfileEndpoint.patch` (`:431-462`): the `settings` patch
/// validates FIRST (outside the lock — a bad namespace beats even a
/// missing row); then one `transaction.atomic()` runs the
/// `select_for_update` read, the `partial=True` serializer
/// validation, the per-namespace settings merge *before* the
/// full-row `serializer.save()`.
async fn patch_profile(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HandlerError> {
    let pool = pool_of(&state)?;
    // Authentication runs before permission (DRF `initial`), and the
    // body is only parsed inside the handler (`request.data`): no
    // valid session answers 401 even for a malformed body.
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let value = patch_body(&headers, &body)?;
    let object = patch_object(&value)?;
    let settings_patch = if let Some(settings) = object.get("settings") {
        Some(
            validate_settings_patch(state.user_settings_schema(), settings).map_err(|message| {
                HandlerError::FieldErrors(serde_json::json!({"settings": [message]}).to_string())
            })?,
        )
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(|_| HandlerError::ServerError)?;
    let row = {
        let conn = TxConn::new(&mut tx);
        profiles::Entity::find()
            .filter(profiles::Column::UserId.eq(actor.id))
            .lock_exclusive()
            .one(&conn)
            .await
            .map_err(|_| HandlerError::ServerError)?
    };
    let Some(row) = row else {
        tx.rollback().await.map_err(|_| HandlerError::ServerError)?;
        return Err(HandlerError::NotFound);
    };
    let mut profile = DbProfile::from_model(row);
    let sets = match validate_partial(object) {
        Ok(sets) => sets,
        Err(errors) => {
            tx.rollback().await.map_err(|_| HandlerError::ServerError)?;
            return Err(HandlerError::FieldErrors(render_field_errors(&errors)));
        }
    };
    profile.apply(&sets);
    if let Some(patch) = &settings_patch {
        profile.settings = merge_settings(&profile.settings, patch)?;
    }
    profile.updated_at = patch_now();
    let save = number_placeholders(&user_q::profile_full_save_sql(), &profile_save_order());
    // raw-sql: domain-covered table write; the domain builder owns the statement.
    bind_profile_save(sqlx::query(&save), &profile)
        .execute(&mut *tx)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    tx.commit().await.map_err(|_| HandlerError::ServerError)?;
    Ok(json_response(
        StatusCode::OK,
        render_profile(&profile, &actor.timezone).to_string(),
    ))
}

/// `auto_now` for the profile save: Django's clock resolves
/// microseconds, so the value bound (and echoed) here does too — a
/// raw `Utc::now()` carries sub-microsecond digits that render as 9
/// fraction digits where DRF renders 6.
fn patch_now() -> DateTime<Utc> {
    Utc::now().round_subsecs(6)
}

/// Placeholder order for [`user_q::profile_full_save_sql`]: the save
/// columns, then the row pk.
fn profile_save_order() -> Vec<&'static str> {
    let mut order: Vec<&'static str> = user_q::PROFILE_SAVE_COLUMNS.to_vec();
    order.push("pk");
    order
}

/// Bind the full-row profile save in [`user_q::PROFILE_SAVE_COLUMNS`]
/// order, then the pk.
fn bind_profile_save<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    profile: &'q DbProfile,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    let query = query
        .bind(profile.created_at)
        .bind(profile.updated_at)
        .bind(profile.user_id)
        .bind(&profile.theme)
        .bind(profile.is_app_rail_docked)
        .bind(profile.is_tour_completed)
        .bind(&profile.onboarding_step);
    let query = match &profile.use_case {
        Some(text) => query.bind(text.as_str()),
        None => query.bind(Option::<&str>::None),
    };
    let query = match &profile.role {
        Some(text) => query.bind(text.as_str()),
        None => query.bind(Option::<&str>::None),
    };
    let query = query
        .bind(profile.is_onboarded)
        .bind(profile.last_workspace_id)
        .bind(profile.billing_address_country.as_str());
    let query = match &profile.billing_address {
        Some(value) => query.bind(value),
        None => query.bind(Option::<&serde_json::Value>::None),
    };
    query
        .bind(profile.has_billing_address)
        .bind(profile.company_name.as_str())
        .bind(profile.notification_view_mode.as_str())
        .bind(profile.is_smooth_cursor_enabled)
        .bind(profile.is_mobile_onboarded)
        .bind(&profile.mobile_onboarding_step)
        .bind(profile.mobile_timezone_auto_set)
        .bind(profile.language.as_str())
        .bind(profile.start_of_the_week)
        .bind(&profile.goals)
        .bind(profile.background_color.as_str())
        .bind(profile.is_navigation_tour_completed)
        .bind(profile.has_marketing_email_consent)
        .bind(profile.is_subscribed_to_changelog)
        .bind(&profile.product_tour)
        .bind(&profile.settings)
        .bind(profile.id)
}

// ---------------------------------------------------------------------------
// Graphs (workspace/user.py:524-559)
// ---------------------------------------------------------------------------

/// Server-local today (`date.today()` is zone-naive local, not UTC):
/// the anchor for the graph window cutoffs.
fn local_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// `EXTRACT(part FROM expr)` as `date_part('part', expr)` (same values;
/// `date_part` returns `double precision`, `EXTRACT` `numeric` — after
/// the integer casts below, identical).
fn date_part(
    part: &str,
    expr: impl Into<sea_orm::sea_query::SimpleExpr>,
) -> sea_orm::sea_query::SimpleExpr {
    Func::cust("date_part")
        .arg(Expr::val(part))
        .arg(expr.into())
        .into()
}

fn zoned(
    column: impl Into<sea_orm::sea_query::SimpleExpr>,
    tz_name: &str,
) -> sea_orm::sea_query::SimpleExpr {
    column.binary(
        BinOper::Custom("AT TIME ZONE"),
        Expr::val(tz_name.to_owned()),
    )
}

#[derive(Debug, FromQueryResult)]
struct ActivityGraphRow {
    created_date: NaiveDate,
    activity_count: i64,
}

#[derive(Debug, FromQueryResult)]
struct CompletedGraphRow {
    week: i32,
    completed_count: i64,
}

#[derive(Debug, FromQueryResult)]
struct WeekInMonthRow {
    week_in_month: i32,
    completed_count: i64,
}

#[derive(Debug, FromQueryResult)]
struct StateDistributionRow {
    state_group: Option<String>,
    state_count: i64,
}

#[derive(Debug, FromQueryResult)]
struct IssueValuesRow {
    id: Uuid,
    name: String,
    workspace_slug: String,
    project_id: Uuid,
    value_date: NaiveDate,
}

/// The `issue_objects` scope + tenant predicate shared by the dashboard
/// issue counts and lists (`queries_core::issue_manager_where_sql` +
/// `issue_tenant_where_sql`). The `states` LEFT JOIN keeps NULL-state
/// rows through the `IS NOT NULL` guard.
fn dashboard_tenant(user_id: Uuid, slug: &str) -> Condition {
    Condition::all()
        .add(issues::Column::DeletedAt.is_null())
        .add(
            Condition::all()
                .add(states::Column::Group.eq("triage"))
                .add(states::Column::Group.is_not_null())
                .not(),
        )
        .add(issues::Column::ArchivedAt.is_null())
        .add(projects::Column::ArchivedAt.is_null())
        .add(issues::Column::IsDraft.eq(false))
        .add(issue_assignees::Column::AssigneeId.eq(user_id))
        .add(workspaces::Column::Slug.eq(slug))
}

fn dashboard_issues_select() -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::InnerJoin, issues::Relation::IssueAssignees.def())
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
}

fn activity_graph_select(
    user_id: Uuid,
    slug: &str,
    cutoff: NaiveDate,
    tz_name: &str,
) -> sea_orm::Select<issue_activities::Entity> {
    let created = Expr::col((
        issue_activities::Entity,
        issue_activities::Column::CreatedAt,
    ));
    let created_date = created.clone().cast_as(Alias::new("date"));
    issue_activities::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_activities::Relation::Workspaces.def(),
        )
        .select_only()
        .expr_as(created_date.clone(), "created_date")
        .expr_as(Func::count(created_date.clone()), "activity_count")
        .filter(issue_activities::Column::ActorId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_activities::Column::DeletedAt.is_null())
        .filter(
            zoned(created, tz_name)
                .cast_as(Alias::new("date"))
                .gte(Expr::val(cutoff)),
        )
        .group_by(created_date)
        .order_by_asc(Expr::col(Alias::new("created_date")))
}

fn completed_graph_select(
    user_id: Uuid,
    slug: &str,
    month: i64,
    tz_name: &str,
) -> sea_orm::Select<issues::Entity> {
    let completed = Expr::col((issues::Entity, issues::Column::CompletedAt));
    let completed_zoned = zoned(completed.clone(), tz_name);
    let week_part = date_part("week", completed_zoned.clone());
    let month_part = date_part("month", completed_zoned);
    let week_expr = week_part.clone().cast_as(Alias::new("integer")).modulo(4);
    issues::Entity::find()
        .join(JoinType::InnerJoin, issues::Relation::IssueAssignees.def())
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .select_only()
        .expr_as(week_expr.clone(), "week")
        .expr_as(Func::count(week_part), "completed_count")
        .filter(issue_assignees::Column::AssigneeId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issues::Column::CompletedAt.is_not_null())
        .filter(month_part.eq(Expr::val(month)))
        .filter(issues::Column::DeletedAt.is_null())
        .filter(
            Condition::all()
                .add(
                    Condition::all()
                        .add(states::Column::Group.eq("triage"))
                        .add(states::Column::Group.is_not_null())
                        .not(),
                )
                .add(issues::Column::ArchivedAt.is_null())
                .add(projects::Column::ArchivedAt.is_null())
                .add(issues::Column::IsDraft.eq(false)),
        )
        .group_by(Expr::col(Alias::new("week")))
        .order_by_asc(Expr::col(Alias::new("week")))
}

fn dashboard_q1_select(
    user_id: Uuid,
    slug: &str,
    from_date: NaiveDate,
    tz_name: &str,
) -> sea_orm::Select<issue_activities::Entity> {
    let created = Expr::col((
        issue_activities::Entity,
        issue_activities::Column::CreatedAt,
    ));
    let created_date = created.clone().cast_as(Alias::new("date"));
    issue_activities::Entity::find()
        .join(
            JoinType::InnerJoin,
            issue_activities::Relation::Workspaces.def(),
        )
        .select_only()
        .expr_as(created_date.clone(), "created_date")
        .expr_as(Func::count(created_date.clone()), "activity_count")
        .filter(issue_activities::Column::ActorId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_activities::Column::DeletedAt.is_null())
        .filter(
            zoned(created, tz_name)
                .cast_as(Alias::new("date"))
                .gte(Expr::val(from_date)),
        )
        .group_by(created_date)
        .order_by_asc(Expr::col(Alias::new("created_date")))
}

fn dashboard_q2_select(
    user_id: Uuid,
    slug: &str,
    month: i64,
    tz_name: &str,
) -> sea_orm::Select<issues::Entity> {
    let completed = Expr::col((issues::Entity, issues::Column::CompletedAt));
    let completed_zoned = zoned(completed.clone(), tz_name);
    let day_part = date_part("day", completed_zoned);
    let week_in_month = day_part.sub(1).div(7).add(1).cast_as(Alias::new("integer"));
    let month_part = date_part("month", zoned(completed, tz_name));
    dashboard_issues_select()
        .select_only()
        .expr_as(week_in_month.clone(), "week_in_month")
        .expr_as(
            Func::count(Expr::col((issues::Entity, issues::Column::Id))),
            "completed_count",
        )
        .filter(dashboard_tenant(user_id, slug))
        .filter(issues::Column::CompletedAt.is_not_null())
        .filter(month_part.eq(Expr::val(month)))
        .group_by(Expr::col(Alias::new("week_in_month")))
        .order_by_asc(Expr::col(Alias::new("week_in_month")))
}

fn dashboard_state_select(user_id: Uuid, slug: &str) -> sea_orm::Select<issues::Entity> {
    dashboard_issues_select()
        .select_only()
        .column_as(states::Column::Group, "state_group")
        .expr_as(
            Func::count(Expr::col((states::Entity, states::Column::Group))),
            "state_count",
        )
        .filter(dashboard_tenant(user_id, slug))
        .group_by(Expr::col((states::Entity, states::Column::Group)))
        .order_by_asc(Expr::col((states::Entity, states::Column::Group)))
}

fn dashboard_values_select(
    user_id: Uuid,
    slug: &str,
    today: NaiveDate,
    overdue: bool,
) -> sea_orm::Select<issues::Entity> {
    let mut select = dashboard_issues_select();
    select = select
        .select_only()
        .column(issues::Column::Id)
        .column(issues::Column::Name)
        .column_as(workspaces::Column::Slug, "workspace_slug")
        .column(issues::Column::ProjectId)
        .column_as(
            if overdue {
                issues::Column::TargetDate
            } else {
                issues::Column::StartDate
            },
            "value_date",
        )
        .filter(dashboard_tenant(user_id, slug))
        .filter(open_groups_condition())
        .filter(issues::Column::CompletedAt.is_null());
    if overdue {
        select = select
            .filter(Expr::col((issues::Entity, issues::Column::TargetDate)).lt(Expr::val(today)));
    } else {
        select = select
            .filter(Expr::col((issues::Entity, issues::Column::StartDate)).gte(Expr::val(today)));
    }
    select.order_by_desc(issues::Column::CreatedAt)
}

/// `UserActivityGraphEndpoint.get` (`:524-539`): per-day activity
/// counts for the caller over the trailing 6 months.
async fn activity_graph(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    match activity_graph_inner(&state, &slug, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn activity_graph_inner(
    state: &AppState,
    slug: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let cutoff = profile_q::months_ago(local_today(), profile_q::ACTIVITY_GRAPH_MONTHS);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = activity_graph_select(actor.id, slug, cutoff, &actor.tz_name)
        .into_model::<ActivityGraphRow>()
        .all(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(serde_json::json!({
            "created_date": row.created_date.to_string(),
            "activity_count": row.activity_count,
        }));
    }
    Ok(json_response(
        StatusCode::OK,
        serde_json::Value::Array(items).to_string(),
    ))
}

/// `UserIssueCompletedGraphEndpoint.get` (`:541-559`): per-week-bucket
/// (`week % 4`) completed counts for the caller in `?month=`
/// (default 1; garbage is a 500, not a 400).
async fn completed_graph(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let raw_query = req.uri().query().unwrap_or("").to_owned();
    match completed_graph_inner(&state, &slug, &raw_query, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn completed_graph_inner(
    state: &AppState,
    slug: &str,
    raw_query: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let month = profile_q::parse_month_param(query_last(raw_query, "month").as_deref())
        .map_err(|_| HandlerError::ServerError)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = completed_graph_select(actor.id, slug, month, &actor.tz_name)
        .into_model::<CompletedGraphRow>()
        .all(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(serde_json::json!({
            "week": row.week,
            "completed_count": row.completed_count,
        }));
    }
    Ok(json_response(
        StatusCode::OK,
        serde_json::Value::Array(items).to_string(),
    ))
}

// ---------------------------------------------------------------------------
// UserWorkspaceDashboardEndpoint (workspace/base.py:262-348)
// ---------------------------------------------------------------------------

fn open_groups_condition() -> Condition {
    Condition::all()
        .add(states::Column::Group.is_in(["completed", "cancelled"]))
        .add(states::Column::Group.is_not_null())
        .not()
}

async fn dashboard_q2_rows(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    slug: &str,
    month: i64,
    tz_name: &str,
) -> Result<Vec<WeekInMonthRow>, HandlerError> {
    dashboard_q2_select(user_id, slug, month, tz_name)
        .into_model::<WeekInMonthRow>()
        .all(db)
        .await
        .map_err(|_| HandlerError::ServerError)
}

async fn dashboard_q1_rows(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    slug: &str,
    from_date: NaiveDate,
    tz_name: &str,
) -> Result<Vec<ActivityGraphRow>, HandlerError> {
    dashboard_q1_select(user_id, slug, from_date, tz_name)
        .into_model::<ActivityGraphRow>()
        .all(db)
        .await
        .map_err(|_| HandlerError::ServerError)
}

async fn dashboard_state_rows(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    slug: &str,
) -> Result<Vec<StateDistributionRow>, HandlerError> {
    dashboard_state_select(user_id, slug)
        .into_model::<StateDistributionRow>()
        .all(db)
        .await
        .map_err(|_| HandlerError::ServerError)
}

async fn dashboard_values_rows(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    slug: &str,
    today: NaiveDate,
    overdue: bool,
) -> Result<Vec<IssueValuesRow>, HandlerError> {
    dashboard_values_select(user_id, slug, today, overdue)
        .into_model::<IssueValuesRow>()
        .all(db)
        .await
        .map_err(|_| HandlerError::ServerError)
}

/// `UserWorkspaceDashboardEndpoint.get` (`:262-348`): the 9-key
/// envelope. No membership check: unknown slugs and foreign callers
/// get zeros, never 404.
async fn dashboard(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let raw_query = req.uri().query().unwrap_or("").to_owned();
    match dashboard_inner(&state, &slug, &raw_query, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn dashboard_inner(
    state: &AppState,
    slug: &str,
    raw_query: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    let month = profile_q::parse_month_param(query_last(raw_query, "month").as_deref())
        .map_err(|_| HandlerError::ServerError)?;
    // Q8/Q9 `:today` and the Q6 ISO week bind Python-side UTC dates
    // (`timezone.now().date()`); the Q1 window binds server-local
    // `date.today() - 3 months`.
    let today_utc = Utc::now().date_naive();
    let iso_week = today_utc.iso_week().week() as i32;
    let from_date = profile_q::months_ago(local_today(), 3);
    let db = pidash_db::pool::sea_orm_connection(pool);

    // Q1: activity series.
    let q1 = dashboard_q1_rows(&db, actor.id, slug, from_date, &actor.tz_name).await?;
    let issue_activities: Vec<serde_json::Value> = q1
        .iter()
        .map(|row| {
            serde_json::json!({
                "created_date": row.created_date.to_string(),
                "activity_count": row.activity_count,
            })
        })
        .collect();

    // Q2: completed-by-week buckets.
    let q2 = dashboard_q2_rows(&db, actor.id, slug, month, &actor.tz_name).await?;
    let completed_issues: Vec<serde_json::Value> = q2
        .iter()
        .map(|row| {
            serde_json::json!({
                "week_in_month": row.week_in_month,
                "completed_count": row.completed_count,
            })
        })
        .collect();

    // Q3-Q6: counts.
    let assigned = dashboard_issues_select()
        .filter(dashboard_tenant(actor.id, slug))
        .count(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let pending = dashboard_issues_select()
        .filter(dashboard_tenant(actor.id, slug))
        .filter(open_groups_condition())
        .count(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let completed = dashboard_issues_select()
        .filter(dashboard_tenant(actor.id, slug))
        .filter(states::Column::Group.eq("completed"))
        .count(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let due_week = dashboard_issues_select()
        .filter(dashboard_tenant(actor.id, slug))
        .filter(
            date_part(
                "week",
                Expr::col((issues::Entity, issues::Column::TargetDate)),
            )
            .eq(Expr::val(iso_week)),
        )
        .count(&db)
        .await
        .map_err(|_| HandlerError::ServerError)?;

    // Q7: state distribution (`COUNT("states"."group")` is 0, not
    // null, for the null-group bucket — the SQL does it as written).
    let q7 = dashboard_state_rows(&db, actor.id, slug).await?;
    let state_distribution: Vec<serde_json::Value> = q7
        .iter()
        .map(|row| {
            serde_json::json!({
                "state_group": row.state_group,
                "state_count": row.state_count,
            })
        })
        .collect();

    // Q8: overdue `.values()` in source order.
    let q8 = dashboard_values_rows(&db, actor.id, slug, today_utc, true).await?;
    let overdue_issues: Vec<serde_json::Value> = q8
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.id.to_string(),
                "name": row.name,
                "workspace__slug": row.workspace_slug,
                "project_id": row.project_id.to_string(),
                "target_date": row.value_date.to_string(),
            })
        })
        .collect();

    // Q9: upcoming `.values()` in source order.
    let q9 = dashboard_values_rows(&db, actor.id, slug, today_utc, false).await?;
    let upcoming_issues: Vec<serde_json::Value> = q9
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.id.to_string(),
                "name": row.name,
                "workspace__slug": row.workspace_slug,
                "project_id": row.project_id.to_string(),
                "start_date": row.value_date.to_string(),
            })
        })
        .collect();

    // Envelope keys in source order (`:336-346`).
    let mut envelope = serde_json::Map::with_capacity(9);
    envelope.insert(
        "issue_activities".to_owned(),
        serde_json::Value::Array(issue_activities),
    );
    envelope.insert(
        "completed_issues".to_owned(),
        serde_json::Value::Array(completed_issues),
    );
    envelope.insert("assigned_issues_count".to_owned(), assigned.into());
    envelope.insert("pending_issues_count".to_owned(), pending.into());
    envelope.insert("completed_issues_count".to_owned(), completed.into());
    envelope.insert("issues_due_week_count".to_owned(), due_week.into());
    envelope.insert(
        "state_distribution".to_owned(),
        serde_json::Value::Array(state_distribution),
    );
    envelope.insert(
        "overdue_issues".to_owned(),
        serde_json::Value::Array(overdue_issues),
    );
    envelope.insert(
        "upcoming_issues".to_owned(),
        serde_json::Value::Array(upcoming_issues),
    );
    debug_assert_eq!(
        envelope.keys().map(String::as_str).collect::<Vec<_>>(),
        core_q::DASHBOARD_RESPONSE_KEYS.to_vec(),
    );
    Ok(json_response(
        StatusCode::OK,
        serde_json::Value::Object(envelope).to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use pidash_services::user_settings::{
        base_settings_schema, extend_settings_schema, SettingsSchema,
    };

    fn fixture(name: &str) -> serde_json::Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_workspace")
            .join(name);
        let text = std::fs::read_to_string(&path).expect("fixture exists");
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn json(raw: &str) -> serde_json::Value {
        serde_json::from_str(raw).expect("test json parses")
    }

    // ------------------------------------------------------------------
    // Error kernel
    // ------------------------------------------------------------------

    #[test]
    fn error_bodies_are_exact() {
        // Lowercase `detail`: DRF `exception_handler` renders
        // `{'detail': exc.detail}`; pinned live by
        // `test_me_anon_denied` on these very routes.
        assert_eq!(UNAUTHENTICATED_BODY, crate::app_workspace::gates::ANON_BODY);
        assert_eq!(
            HandlerError::Unauthorized.status_and_body(),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            HandlerError::NotFound.status_and_body(),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"The required object does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            HandlerError::ServerError.status_and_body(),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
        assert_eq!(
            HandlerError::FieldErrors(r#"{"a":["b"]}"#.to_owned())
                .status_and_body()
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn month_garbage_maps_to_500() {
        // `int(value)` raises `ValueError`, not `ValidationError`
        // (graphs + dashboard Q2 share this).
        assert_eq!(profile_q::MONTH_INVALID_STATUS, 500);
        assert_eq!(profile_q::SERVER_ERROR_BODY, SERVER_ERROR_BODY);
        assert!(profile_q::parse_month_param(Some("abc")).is_err());
        assert_eq!(profile_q::parse_month_param(None).unwrap(), 1);
    }

    // ------------------------------------------------------------------
    // Query + placeholder helpers
    // ------------------------------------------------------------------

    #[test]
    fn query_last_matches_querydict_get() {
        assert_eq!(query_last("", "month"), None);
        assert_eq!(query_last("a=1", "month"), None);
        assert_eq!(query_last("month=2", "month").as_deref(), Some("2"));
        // Last value wins on repeats.
        assert_eq!(query_last("month=2&month=3", "month").as_deref(), Some("3"));
        // Present-but-empty is `Some("")`, not absent.
        assert_eq!(query_last("month=", "month").as_deref(), Some(""));
        assert_eq!(query_last("month", "month").as_deref(), Some(""));
        // URL decoding like Django's forgiving unquote.
        assert_eq!(query_last("month=%2B1", "month").as_deref(), Some("+1"));
        assert_eq!(query_last("a+b=c+d", "a b").as_deref(), Some("c d"));
        // Malformed escapes pass through; bad UTF-8 becomes U+FFFD.
        assert_eq!(query_last("month=%zz", "month").as_deref(), Some("%zz"));
        assert_eq!(
            query_last("month=%FF", "month").as_deref(),
            Some("\u{fffd}")
        );
    }

    #[test]
    fn placeholders_renumber_in_order() {
        assert_eq!(
            number_placeholders("a = :user AND b = :slug", &["user", "slug"]),
            "a = $1 AND b = $2"
        );
        // Repeats share the number.
        assert_eq!(
            number_placeholders(":tzname x :user y :tzname", &["tzname", "user"]),
            "$1 x $2 y $1"
        );
        // Casts pass through.
        assert_eq!(
            number_placeholders(")::INTEGER x :user", &["user"]),
            ")::INTEGER x $1"
        );
        // Unknown names pass through.
        assert_eq!(number_placeholders(":user :nope", &["user"]), "$1 :nope");
        // Prefix traps: `:user_id` is one name, not `:user` + `_id`.
        assert_eq!(
            number_placeholders(":user_id = :user", &["user", "user_id"]),
            "$2 = $1"
        );
    }

    /// No symbolic placeholder may survive into an executed
    /// statement (casts excluded).
    fn assert_fully_numbered(sql: &str) {
        let mut rest = sql;
        while let Some(colon) = rest.find(':') {
            let after = &rest[colon + 1..];
            // Postgres casts (`::DATE`, `::INTEGER`) are not placeholders.
            if let Some(stripped) = after.strip_prefix(':') {
                rest = stripped;
                continue;
            }
            let first = after.chars().next().unwrap_or(' ');
            assert!(
                !first.is_ascii_alphabetic() && first != '_',
                "unnumbered placeholder in: {sql}"
            );
            rest = after;
        }
    }

    #[test]
    fn dashboard_statements_pin() {
        use sea_orm::{DbBackend, QueryTrait};
        let uid = Uuid::nil();
        let from_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 2, 6).unwrap();
        let q1 = dashboard_q1_select(uid, "acme", from_date, "UTC")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(q1.contains("FROM \"issue_activities\""), "{q1}");
        assert!(q1.contains("INNER JOIN \"workspaces\""), "{q1}");
        assert!(q1.contains("\"created_date\""), "{q1}");
        assert!(q1.contains("\"activity_count\""), "{q1}");
        assert!(q1.contains("AT TIME ZONE"), "{q1}");
        assert!(q1.contains("GROUP BY"), "{q1}");

        let q2 = dashboard_q2_select(uid, "acme", 1, "UTC")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(q2.contains("\"week_in_month\""), "{q2}");
        assert!(q2.contains("LEFT JOIN \"states\""), "{q2}");
        assert!(q2.contains("INNER JOIN \"projects\""), "{q2}");
        assert!(q2.contains("INNER JOIN \"issue_assignees\""), "{q2}");
        assert!(q2.contains("INNER JOIN \"workspaces\""), "{q2}");
        assert!(q2.contains("date_part"), "{q2}");
        assert!(q2.contains("GROUP BY"), "{q2}");

        let q7 = dashboard_state_select(uid, "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(q7.contains("\"state_count\""), "{q7}");
        assert!(q7.contains("GROUP BY"), "{q7}");

        let q8 = dashboard_values_select(uid, "acme", today, true)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(q8.contains("\"workspace_slug\""), "{q8}");
        assert!(q8.contains("DESC"), "{q8}");

        let q9 = dashboard_values_select(uid, "acme", today, false)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(q9.contains("\"value_date\""), "{q9}");

        // Counts share the tenant scope and carry no ORDER BY.
        let assigned = dashboard_issues_select()
            .filter(dashboard_tenant(uid, "acme"))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            assigned.contains("\"issues\".\"deleted_at\" IS NULL"),
            "{assigned}"
        );
        assert!(!assigned.contains("ORDER BY"), "{assigned}");
    }

    #[test]
    fn account_and_profile_statements_pin() {
        use sea_orm::{DbBackend, QueryTrait};
        let uid = Uuid::nil();
        let list = accounts::Entity::find()
            .filter(accounts::Column::UserId.eq(uid))
            .order_by_desc(accounts::Column::CreatedAt)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(list.contains("ORDER BY"), "{list}");

        let detail = accounts::Entity::find()
            .filter(accounts::Column::Id.eq(uid))
            .filter(accounts::Column::UserId.eq(uid))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(detail.contains("FROM \"accounts\""), "{detail}");

        let profile = profiles::Entity::find()
            .filter(profiles::Column::UserId.eq(uid))
            .lock_exclusive()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(profile.contains("FOR UPDATE"), "{profile}");

        assert_fully_numbered(&number_placeholders(
            &user_q::account_hard_delete_sql(),
            &["pk"],
        ));
        let save = number_placeholders(&user_q::profile_full_save_sql(), &profile_save_order());
        assert_fully_numbered(&save);
        // 29 columns + pk.
        assert_eq!(profile_save_order().len(), 30);
        assert_eq!(profile_save_order()[29], "pk");
        assert!(save.contains("$30"));
        assert!(!save.contains("$31"));
    }

    #[test]
    fn pk_parse_matches_uuid_converter() {
        let id = Uuid::nil();
        assert_eq!(parse_pk(&id.to_string()), Some(id));
        assert_eq!(parse_pk("not-a-uuid"), None);
        assert_eq!(parse_pk(""), None);
    }

    // ------------------------------------------------------------------
    // PATCH body + datatype names
    // ------------------------------------------------------------------

    fn headers(content_type: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_type {
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                value.parse().expect("test content type"),
            );
        }
        headers
    }

    #[test]
    fn patch_body_decode_matches_drf() {
        // Empty → `{}`.
        assert_eq!(patch_body(&headers(None), b"").unwrap(), json("{}"));
        // Missing content-type still parses as JSON.
        assert_eq!(
            patch_body(&headers(None), b"{\"a\":1}").unwrap(),
            json("{\"a\":1}")
        );
        // Suffix parameters are ignored.
        assert_eq!(
            patch_body(&headers(Some("application/json; charset=utf-8")), b"[]").unwrap(),
            json("[]")
        );
        // Anything else → 415, lowercase `detail`.
        let error = patch_body(&headers(Some("text/plain")), b"{}").unwrap_err();
        let (status, body) = error.status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
        // Parse errors → 400 `detail`.
        let error = patch_body(&headers(Some("application/json")), b"{nope").unwrap_err();
        let (status, body) = error.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with(r#"{"detail":"JSON parse error - "#),
            "{body}"
        );
    }

    #[test]
    fn datatype_names_match_python() {
        assert_eq!(datatype_name(&json("null")), "NoneType");
        assert_eq!(datatype_name(&json("true")), "bool");
        assert_eq!(datatype_name(&json("1")), "int");
        // Unbounded ints stay `int`, however large.
        assert_eq!(datatype_name(&json("10000000000000000000000000")), "int");
        assert_eq!(datatype_name(&json("1.0")), "float");
        assert_eq!(datatype_name(&json("1e3")), "float");
        assert_eq!(datatype_name(&json("\"x\"")), "str");
        assert_eq!(datatype_name(&json("[1]")), "list");
        assert_eq!(datatype_name(&json("{}")), "dict");
    }

    #[test]
    fn patch_prelude_matches_live_probe_matrix() {
        // `None`/numbers/bools 500 in the `:439` membership check
        // (`TypeError`), before the serializer runs.
        for raw in ["null", "5", "1.5", "true"] {
            assert!(
                matches!(patch_object(&json(raw)), Err(HandlerError::ServerError)),
                "body {raw}"
            );
        }
        // Strings/lists containing "settings" 500 on the following
        // `.get` (`AttributeError`); the rest 400 non-dict.
        for raw in [
            "\"settings\"",
            "\"xsettingsy\"",
            "[\"settings\"]",
            "[1,\"settings\"]",
        ] {
            assert!(
                matches!(patch_object(&json(raw)), Err(HandlerError::ServerError)),
                "body {raw}"
            );
        }
        for (raw, name) in [("\"x\"", "str"), ("[1]", "list"), ("[\"x\"]", "list")] {
            let error = patch_object(&json(raw)).unwrap_err();
            let (status, body) = error.status_and_body();
            assert_eq!(status, StatusCode::BAD_REQUEST, "body {raw}");
            assert_eq!(
                body,
                format!("{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {name}.\"]}}"),
                "body {raw}"
            );
        }
        // Dicts pass through (settings presence decided by key).
        assert!(patch_object(&json("{}")).is_ok());
        assert!(patch_object(&json("{\"settings\":{}}")).is_ok());
        // Datatype names still cover every JSON kind.
        for (raw, name) in [
            ("null", "NoneType"),
            ("true", "bool"),
            ("1", "int"),
            ("10000000000000000000000000", "int"),
            ("1.5", "float"),
            ("\"s\"", "str"),
            ("[1]", "list"),
        ] {
            assert_eq!(datatype_name(&json(raw)), name, "value {raw}");
        }
    }

    // ------------------------------------------------------------------
    // user_settings port
    // ------------------------------------------------------------------

    #[test]
    fn settings_rejects_non_objects() {
        let schema = base_settings_schema();
        assert_eq!(
            validate_settings_patch(&schema, &json("[1]")).unwrap_err(),
            "settings must be an object"
        );
        assert_eq!(
            validate_settings_patch(&schema, &json("null")).unwrap_err(),
            "settings must be an object"
        );
    }

    #[test]
    fn settings_rejects_every_namespace_in_ce() {
        let schema = base_settings_schema();
        // Exact bytes pinned by the live contract
        // `test_profile_patch_unknown_settings_namespace_rejected`.
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"nope\":{\"k\":1}}")).unwrap_err(),
            "unknown settings namespace: nope"
        );
        // The namespace check runs before the shape check.
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"nope\":1}")).unwrap_err(),
            "unknown settings namespace: nope"
        );
    }

    #[test]
    fn settings_accepts_empty_patch() {
        let schema = base_settings_schema();
        assert_eq!(
            validate_settings_patch(&schema, &json("{}")).unwrap(),
            serde_json::Map::new()
        );
    }

    /// The cloud declaration (`ee/settings/user_settings.py`): the base
    /// schema extended with `openhub.apps_enabled` (default `false`).
    fn openhub_schema() -> SettingsSchema {
        extend_settings_schema(
            &base_settings_schema(),
            &BTreeMap::from([(
                "openhub".to_string(),
                BTreeMap::from([("apps_enabled".to_string(), json("false"))]),
            )]),
        )
        .expect("openhub extends the empty base")
    }

    #[test]
    fn settings_ce_schema_rejects_openhub_write() {
        // CE declares nothing, so the overlay key is refused — the
        // pre-seam behavior, now through the injected empty schema.
        assert_eq!(
            validate_settings_patch(
                &base_settings_schema(),
                &json("{\"openhub\":{\"apps_enabled\":true}}")
            )
            .unwrap_err(),
            "unknown settings namespace: openhub"
        );
    }

    #[test]
    fn settings_extended_schema_accepts_declared_keys() {
        let schema = openhub_schema();
        let accepted =
            validate_settings_patch(&schema, &json("{\"openhub\":{\"apps_enabled\":true}}"))
                .unwrap();
        assert_eq!(
            serde_json::Value::Object(accepted),
            json("{\"openhub\":{\"apps_enabled\":true}}")
        );
        // An empty namespace patch stays accepted (no keys to dispute).
        let accepted = validate_settings_patch(&schema, &json("{\"openhub\":{}}")).unwrap();
        assert_eq!(
            serde_json::Value::Object(accepted),
            json("{\"openhub\":{}}")
        );
    }

    #[test]
    fn settings_extended_schema_still_rejects_unknown() {
        let schema = openhub_schema();
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"nope\":{\"k\":1}}")).unwrap_err(),
            "unknown settings namespace: nope"
        );
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"openhub\":{\"nope\":true}}")).unwrap_err(),
            "unknown settings key(s) in openhub: nope"
        );
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"openhub\":true}")).unwrap_err(),
            "settings.openhub must be an object"
        );
        assert_eq!(
            validate_settings_patch(&schema, &json("{\"openhub\":{\"apps_enabled\":\"yes\"}}"))
                .unwrap_err(),
            "settings.openhub.apps_enabled must be of type bool"
        );
    }

    #[test]
    fn settings_declared_defaults_drive_type_checks() {
        let schema: SettingsSchema = BTreeMap::from([(
            "ns".to_string(),
            BTreeMap::from([
                ("count".to_string(), json("0")),
                ("ratio".to_string(), json("1.5")),
                ("name".to_string(), json("\"\"")),
                ("tags".to_string(), json("[]")),
                ("meta".to_string(), json("{}")),
                ("anything".to_string(), json("null")),
            ]),
        )]);
        // Matching values pass, including ints under float keys and
        // scalars under null-defaulted keys.
        for raw in [
            "{\"ns\":{\"count\":3}}",
            "{\"ns\":{\"ratio\":2}}",
            "{\"ns\":{\"ratio\":2.5}}",
            "{\"ns\":{\"name\":\"x\"}}",
            "{\"ns\":{\"tags\":[1]}}",
            "{\"ns\":{\"meta\":{\"a\":1}}}",
            "{\"ns\":{\"anything\":1}}",
            "{\"ns\":{\"anything\":\"x\"}}",
        ] {
            assert!(
                validate_settings_patch(&schema, &json(raw)).is_ok(),
                "patch {raw}"
            );
        }
        // Mismatches fail naming the declared type.
        for (raw, expected) in [
            (
                "{\"ns\":{\"count\":true}}",
                "settings.ns.count must be of type int",
            ),
            (
                "{\"ns\":{\"count\":1.5}}",
                "settings.ns.count must be of type int",
            ),
            (
                "{\"ns\":{\"ratio\":true}}",
                "settings.ns.ratio must be of type float",
            ),
            (
                "{\"ns\":{\"name\":1}}",
                "settings.ns.name must be of type str",
            ),
            (
                "{\"ns\":{\"tags\":{}}}",
                "settings.ns.tags must be of type list",
            ),
            (
                "{\"ns\":{\"meta\":[]}}",
                "settings.ns.meta must be of type dict",
            ),
            (
                "{\"ns\":{\"anything\":[]}}",
                "settings.ns.anything must be a scalar",
            ),
            (
                "{\"ns\":{\"anything\":{}}}",
                "settings.ns.anything must be a scalar",
            ),
        ] {
            assert_eq!(
                validate_settings_patch(&schema, &json(raw)).unwrap_err(),
                expected,
                "patch {raw}"
            );
        }
    }

    #[test]
    fn setting_value_types_dispatch_like_python() {
        let value = json("{\"a\":1}");
        // Null default: scalars pass, containers fail.
        assert!(check_setting_value("n", "k", &json("1"), SettingDefault::Null).is_ok());
        assert!(check_setting_value("n", "k", &json("null"), SettingDefault::Null).is_ok());
        assert_eq!(
            check_setting_value("n", "k", &value, SettingDefault::Null).unwrap_err(),
            "settings.n.k must be a scalar"
        );
        // Bool before int: `True` satisfies bool keys only.
        assert!(check_setting_value("n", "k", &json("true"), SettingDefault::Bool).is_ok());
        assert!(check_setting_value("n", "k", &json("1"), SettingDefault::Bool).is_err());
        assert!(check_setting_value("n", "k", &json("1"), SettingDefault::Int).is_ok());
        assert!(check_setting_value("n", "k", &json("true"), SettingDefault::Int).is_err());
        assert!(check_setting_value("n", "k", &json("1.0"), SettingDefault::Int).is_err());
        assert!(check_setting_value("n", "k", &json("1"), SettingDefault::Float).is_ok());
        assert!(check_setting_value("n", "k", &json("1.5"), SettingDefault::Float).is_ok());
        assert!(check_setting_value("n", "k", &json("true"), SettingDefault::Float).is_err());
        assert_eq!(
            check_setting_value("n", "k", &json("\"x\""), SettingDefault::Int).unwrap_err(),
            "settings.n.k must be of type int"
        );
        assert!(check_setting_value("n", "k", &json("\"x\""), SettingDefault::Str).is_ok());
        assert!(check_setting_value("n", "k", &json("[1]"), SettingDefault::List).is_ok());
        assert!(check_setting_value("n", "k", &value, SettingDefault::Dict).is_ok());
    }

    #[test]
    fn settings_merge_is_per_namespace() {
        let stored = json("{\"a\":{\"x\":1},\"b\":{\"y\":2},\"flat\":1}");
        let patch: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str("{\"a\":{\"z\":3}}").unwrap();
        // Non-dict namespaces drop even on merge; patch extends per key.
        assert_eq!(
            merge_settings(&stored, &patch).unwrap(),
            json("{\"a\":{\"x\":1,\"z\":3},\"b\":{\"y\":2}}")
        );
        // Merging `{}` still rewrites through the dict filter (ported quirk).
        let empty: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        assert_eq!(
            merge_settings(&stored, &empty).unwrap(),
            json("{\"a\":{\"x\":1},\"b\":{\"y\":2}}")
        );
    }

    #[test]
    fn settings_merge_falsy_stored_behaves_as_empty() {
        let empty: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for raw in ["null", "false", "0", "-0", "0.0", "\"\"", "[]", "{}"] {
            assert_eq!(
                merge_settings(&json(raw), &empty).unwrap(),
                json("{}"),
                "stored {raw}"
            );
        }
    }

    #[test]
    fn settings_merge_truthy_non_dict_is_500() {
        let empty: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for raw in ["true", "5", "0.5", "\"x\"", "[1]"] {
            assert!(
                matches!(
                    merge_settings(&json(raw), &empty),
                    Err(HandlerError::ServerError)
                ),
                "stored {raw}"
            );
        }
    }

    #[test]
    fn settings_size_cap_counts_cpython_bytes() {
        // Oracle: CPython `len(json.dumps(v).encode())`.
        for (raw, expected) in [
            ("{}", 2),
            ("{\"a\":1}", 8),
            ("{\"ns\":{\"k\":\"é\"}}", 23),
            ("{\"z\":\"a😀b\"}", 23),
            ("{\"d\":1e16}", 12),
            ("{\"i\":10000000000000000000000000000000000000000}", 48),
        ] {
            assert_eq!(py_json_dumps_len(&json(raw)), expected, "value {raw}");
        }
    }

    #[test]
    fn float_repr_matches_python() {
        // Oracle: CPython `repr(float)`.
        for (input, expected) in [
            (1e16, "1e+16"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (100000.0, "100000.0"),
            (1e-5, "1e-05"),
            (123456789.0, "123456789.0"),
            (2.5e-7, "2.5e-07"),
            (-0.0, "-0.0"),
        ] {
            assert_eq!(py_float_str(input), expected, "input {input}");
        }
        assert_eq!(py_float_str(f64::INFINITY), "Infinity");
        assert_eq!(py_float_str(f64::NEG_INFINITY), "-Infinity");
    }

    // ------------------------------------------------------------------
    // ProfileSerializer validation
    // ------------------------------------------------------------------

    fn validate_one(name: &'static str, raw: &str) -> Result<Assignment, Vec<String>> {
        let (_, kind) = WRITABLE_FIELDS
            .iter()
            .find(|(field, _)| *field == name)
            .expect("writable field");
        validate_field(name, *kind, &json(raw))
    }

    #[test]
    fn bool_fields_coerce_like_drf() {
        for raw in [
            "true", "\"TRUE\"", "\"t\"", "\"Yes\"", "\"on\"", "\"1\"", "1", "1.0",
        ] {
            assert_eq!(
                validate_one("is_onboarded", raw).unwrap(),
                Assignment::Flag("is_onboarded", true),
                "input {raw}"
            );
        }
        for raw in [
            "false",
            "\"FALSE\"",
            "\"f\"",
            "\"No\"",
            "\"off\"",
            "\"0\"",
            "0",
            "0.0",
        ] {
            assert_eq!(
                validate_one("is_onboarded", raw).unwrap(),
                Assignment::Flag("is_onboarded", false),
                "input {raw}"
            );
        }
        for raw in ["null", "\"2\"", "\" 1\"", "2", "1.5", "[1]", "{\"a\":1}"] {
            let errors = validate_one("is_onboarded", raw).unwrap_err();
            assert_eq!(errors.len(), 1, "input {raw}");
            assert!(
                errors[0] == "Must be a valid boolean."
                    || errors[0] == "This field may not be null.",
                "input {raw}: {errors:?}"
            );
        }
        // Explicit null on a non-nullable field fails (partial skips
        // only absent keys).
        assert_eq!(
            validate_one("is_onboarded", "null").unwrap_err(),
            vec!["This field may not be null.".to_owned()]
        );
    }

    #[test]
    fn text_fields_trim_coerce_and_count_code_points() {
        // Numbers coerce; strings strip.
        assert_eq!(
            validate_one("company_name", "1.5").unwrap(),
            Assignment::NullableText("company_name", Some("1.5".to_owned()))
        );
        assert_eq!(
            validate_one("company_name", "\"  Acme  \"").unwrap(),
            Assignment::NullableText("company_name", Some("Acme".to_owned()))
        );
        // Bools and composites are invalid.
        for raw in ["true", "[\"x\"]", "{\"a\":1}"] {
            assert_eq!(
                validate_one("company_name", raw).unwrap_err(),
                vec!["Not a valid string.".to_owned()],
                "input {raw}"
            );
        }
        // Blank rules follow `allow_blank`.
        assert_eq!(
            validate_one("company_name", "\"   \"").unwrap(),
            Assignment::NullableText("company_name", Some(String::new()))
        );
        assert_eq!(
            validate_one("language", "\"   \"").unwrap_err(),
            vec!["This field may not be blank.".to_owned()]
        );
        // Null follows `allow_null`.
        assert_eq!(
            validate_one("use_case", "null").unwrap(),
            Assignment::NullableText("use_case", None)
        );
        assert_eq!(
            validate_one("language", "null").unwrap_err(),
            vec!["This field may not be null.".to_owned()]
        );
        // `max_length` counts code points on the trimmed value (255
        // CJK code points are within the limit).
        let cjk = "水".repeat(255);
        assert!(validate_one("language", &format!("\"{cjk}\"")).is_ok());
        let too_long = "x".repeat(256);
        assert_eq!(
            validate_one("language", &format!("\"{too_long}\"")).unwrap_err(),
            vec!["Ensure this field has no more than 255 characters.".to_owned()]
        );
        let role_long = "x".repeat(301);
        assert_eq!(
            validate_one("role", &format!("\"{role_long}\"")).unwrap_err(),
            vec!["Ensure this field has no more than 300 characters.".to_owned()]
        );
        // Null characters are rejected after length (validators collect).
        let nul_and_long = format!("\"{}\\u0000\"", "x".repeat(300));
        assert_eq!(
            validate_one("language", &nul_and_long).unwrap_err(),
            vec![
                "Ensure this field has no more than 255 characters.".to_owned(),
                "Null characters are not allowed.".to_owned(),
            ]
        );
    }

    #[test]
    fn uuid_field_matches_drf_edge_for_edge() {
        let verbose = "12345678-1234-5678-1234-567812345678";
        assert_eq!(
            validate_one("last_workspace_id", &format!("\"{verbose}\"")).unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(verbose.parse().unwrap()))
        );
        // `UUID(hex=…)` spellings.
        assert!(validate_one("last_workspace_id", "\"12345678123456781234567812345678\"").is_ok());
        assert!(validate_one(
            "last_workspace_id",
            "\"{12345678-1234-5678-1234-567812345678}\""
        )
        .is_ok());
        assert!(validate_one(
            "last_workspace_id",
            "\"urn:uuid:12345678-1234-5678-1234-567812345678\""
        )
        .is_ok());
        // Bools take the `int` path (`True` → `…0001`): ported quirk.
        assert_eq!(
            validate_one("last_workspace_id", "true").unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(Uuid::from_u128(1)))
        );
        assert_eq!(
            validate_one("last_workspace_id", "false").unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(Uuid::nil()))
        );
        // Ints must fit `u128`; `-0` parses to `0`.
        assert_eq!(
            validate_one("last_workspace_id", "1").unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(Uuid::from_u128(1)))
        );
        assert_eq!(
            validate_one("last_workspace_id", "\"-0\"").unwrap_err(),
            vec!["Must be a valid UUID.".to_owned()]
        );
        assert_eq!(
            validate_one("last_workspace_id", "-0").unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(Uuid::nil()))
        );
        assert_eq!(
            validate_one(
                "last_workspace_id",
                "340282366920938463463374607431768211455"
            )
            .unwrap(),
            Assignment::NullableUuid("last_workspace_id", Some(Uuid::from_u128(u128::MAX)))
        );
        for raw in [
            "-1",
            "340282366920938463463374607431768211456",
            "1.0",
            "\"nope\"",
            "\"\"",
            "[1]",
            "{\"a\":1}",
        ] {
            assert_eq!(
                validate_one("last_workspace_id", raw).unwrap_err(),
                vec!["Must be a valid UUID.".to_owned()],
                "input {raw}"
            );
        }
        // Nullable on the model.
        assert_eq!(
            validate_one("last_workspace_id", "null").unwrap(),
            Assignment::NullableUuid("last_workspace_id", None)
        );
    }

    #[test]
    fn choice_fields_match_drf() {
        assert_eq!(
            validate_one("notification_view_mode", "\"full\"").unwrap(),
            Assignment::Text("notification_view_mode", "full".to_owned())
        );
        assert_eq!(
            validate_one("notification_view_mode", "\"FULL\"").unwrap_err(),
            vec!["\"FULL\" is not a valid choice.".to_owned()]
        );
        // `str(data)` lookups: numbers and bools stringify, then miss.
        assert_eq!(
            validate_one("notification_view_mode", "1").unwrap_err(),
            vec!["\"1\" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            validate_one("notification_view_mode", "true").unwrap_err(),
            vec!["\"True\" is not a valid choice.".to_owned()]
        );
        assert_eq!(
            validate_one("notification_view_mode", "{\"a\":1}").unwrap_err(),
            vec!["\"{'a': 1}\" is not a valid choice.".to_owned()]
        );
        // Integer choices accept ints and their string spellings only.
        assert_eq!(
            validate_one("start_of_the_week", "3").unwrap(),
            Assignment::ChoiceInt("start_of_the_week", 3)
        );
        assert_eq!(
            validate_one("start_of_the_week", "\"6\"").unwrap(),
            Assignment::ChoiceInt("start_of_the_week", 6)
        );
        for raw in ["\"01\"", "\" 1\"", "\"1.0\"", "1.0", "7", "-1", "\"mon\""] {
            assert_eq!(
                validate_one("start_of_the_week", raw).unwrap_err(),
                vec![format!(
                    "\"{}\" is not a valid choice.",
                    raw.trim_matches('"')
                )],
                "input {raw}"
            );
        }
        assert_eq!(
            validate_one("start_of_the_week", "null").unwrap_err(),
            vec!["This field may not be null.".to_owned()]
        );
        assert_eq!(
            validate_one("start_of_the_week", "true").unwrap_err(),
            vec!["\"True\" is not a valid choice.".to_owned()]
        );
    }

    #[test]
    fn json_fields_pass_through_with_null_rules() {
        assert_eq!(
            validate_one("theme", "{\"mode\":\"dark\"}").unwrap(),
            Assignment::Json("theme", json("{\"mode\":\"dark\"}"))
        );
        assert_eq!(
            validate_one("theme", "[1,2]").unwrap(),
            Assignment::Json("theme", json("[1,2]"))
        );
        assert_eq!(
            validate_one("theme", "null").unwrap_err(),
            vec!["This field may not be null.".to_owned()]
        );
        assert_eq!(
            validate_one("billing_address", "null").unwrap(),
            Assignment::Json("billing_address", serde_json::Value::Null)
        );
    }

    #[test]
    fn partial_validation_ignores_readonly_and_unknown() {
        let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            "{\"user\":\"00000000-0000-0000-0000-000000000000\",\"settings\":{},\"id\":\"x\",\"nope\":1}",
        )
        .unwrap();
        assert_eq!(validate_partial(&object).unwrap(), vec![]);
        assert_eq!(validate_partial(&serde_json::Map::new()).unwrap(), vec![]);
    }

    #[test]
    fn partial_errors_render_in_field_order() {
        let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            "{\"product_tour\":null,\"theme\":null,\"is_onboarded\":\"maybe\"}",
        )
        .unwrap();
        let errors = validate_partial(&object).unwrap_err();
        // Declaration order (theme before is_onboarded before
        // product_tour), not input order.
        assert_eq!(
            errors
                .iter()
                .map(|(field, _)| field.as_str())
                .collect::<Vec<_>>(),
            vec!["theme", "is_onboarded", "product_tour"]
        );
        assert_eq!(
            render_field_errors(&errors),
            "{\"theme\":[\"This field may not be null.\"],\"is_onboarded\":[\"Must be a valid boolean.\"],\"product_tour\":[\"This field may not be null.\"]}"
        );
    }

    #[test]
    fn profile_apply_lands_every_assignment_kind() {
        let mut profile = sample_profile();
        let ws = Uuid::from_u128(42);
        profile.apply(&[
            Assignment::Text("notification_view_mode", "compact".to_owned()),
            Assignment::NullableText("use_case", None),
            Assignment::NullableText("role", Some("Dev".to_owned())),
            Assignment::NullableText("language", Some("fr".to_owned())),
            Assignment::Flag("is_onboarded", true),
            Assignment::NullableUuid("last_workspace_id", Some(ws)),
            Assignment::ChoiceInt("start_of_the_week", 1),
            Assignment::Json("theme", json("{\"mode\":\"dark\"}")),
            Assignment::Json("billing_address", serde_json::Value::Null),
        ]);
        assert_eq!(profile.notification_view_mode, "compact");
        assert_eq!(profile.use_case, None);
        assert_eq!(profile.role.as_deref(), Some("Dev"));
        assert_eq!(profile.language, "fr");
        assert!(profile.is_onboarded);
        assert_eq!(profile.last_workspace_id, Some(ws));
        assert_eq!(profile.start_of_the_week, 1);
        assert_eq!(profile.theme, json("{\"mode\":\"dark\"}"));
        assert_eq!(profile.billing_address, None);
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    fn sample_account() -> DbAccount {
        DbAccount {
            id: Uuid::from_u128(1),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 123_456_000).unwrap(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap(),
            provider_account_id: "acc-1".to_owned(),
            provider: "google".to_owned(),
            access_token: "tok".to_owned(),
            access_token_expired_at: None,
            refresh_token: None,
            refresh_token_expired_at: Some(
                chrono::DateTime::from_timestamp(1_700_000_200, 0).unwrap(),
            ),
            last_connected_at: chrono::DateTime::from_timestamp(1_700_000_050, 0).unwrap(),
            id_token: "idt".to_owned(),
            metadata: json("{\"a\":1}"),
            user_id: Uuid::from_u128(2),
        }
    }

    fn sample_profile() -> DbProfile {
        DbProfile {
            id: Uuid::from_u128(3),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap(),
            user_id: Uuid::from_u128(2),
            theme: json("{}"),
            is_app_rail_docked: true,
            is_tour_completed: false,
            onboarding_step: json("{\"profile_complete\":false}"),
            use_case: None,
            role: Some("Dev".to_owned()),
            is_onboarded: false,
            last_workspace_id: None,
            billing_address_country: "INDIA".to_owned(),
            billing_address: None,
            has_billing_address: false,
            company_name: String::new(),
            notification_view_mode: "full".to_owned(),
            is_smooth_cursor_enabled: false,
            is_mobile_onboarded: false,
            mobile_onboarding_step: json("{}"),
            mobile_timezone_auto_set: false,
            language: "en".to_owned(),
            start_of_the_week: 0,
            goals: json("{}"),
            background_color: "#fff".to_owned(),
            is_navigation_tour_completed: false,
            has_marketing_email_consent: false,
            is_subscribed_to_changelog: false,
            product_tour: json("{}"),
            settings: json("{}"),
        }
    }

    #[test]
    fn account_renders_wire_order_with_zoned_datetimes() {
        let rendered = render_account(&sample_account(), &chrono_tz::UTC);
        let object = rendered.as_object().expect("account is an object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            ser::ACCOUNT_WIRE_FIELDS.to_vec(),
        );
        assert_eq!(object["provider"], json("\"google\""));
        assert_eq!(object["refresh_token"], serde_json::Value::Null);
        // Microseconds print; `+00:00` becomes `Z`.
        assert_eq!(
            object["created_at"],
            json("\"2023-11-14T22:13:20.123456Z\"")
        );
        assert_eq!(object["updated_at"], json("\"2023-11-14T22:15:00Z\""));
        let eastern: Tz = "America/New_York".parse().unwrap();
        let shifted = render_account(&sample_account(), &eastern);
        assert_eq!(
            shifted["created_at"],
            json("\"2023-11-14T17:13:20.123456-05:00\"")
        );
    }

    #[test]
    fn profile_renders_wire_order_with_nulls() {
        let rendered = render_profile(&sample_profile(), &chrono_tz::UTC);
        let object = rendered.as_object().expect("profile is an object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            ser::PROFILE_WIRE_FIELDS.to_vec(),
        );
        assert_eq!(object["use_case"], serde_json::Value::Null);
        assert_eq!(object["role"], json("\"Dev\""));
        assert_eq!(object["last_workspace_id"], serde_json::Value::Null);
        assert_eq!(object["billing_address"], serde_json::Value::Null);
        assert_eq!(object["start_of_the_week"], json("0"));
        assert_eq!(
            object["user"],
            json(format!("\"{}\"", Uuid::from_u128(2)).as_str())
        );
    }

    #[test]
    fn patch_now_resolves_microseconds_like_django() {
        // `auto_now` binds (and echoes) microsecond clock values: a raw
        // `Utc::now()` would render 9 fraction digits where DRF renders 6.
        for _ in 0..100 {
            let now = patch_now();
            assert_eq!(now.timestamp_subsec_nanos() % 1000, 0, "{now:?}");
            let rendered = render_datetime_in(&now, &chrono_tz::UTC);
            let fraction = rendered
                .split(['T'])
                .nth(1)
                .expect("time part")
                .split(['Z', '+', '-'])
                .next()
                .expect("fraction part");
            match fraction.split_once('.') {
                None => {}
                Some((_, digits)) => assert_eq!(digits.len(), 6, "{rendered}"),
            }
        }
    }

    #[test]
    fn profile_get_carries_cache_headers() {
        let headers = crate::app_workspace::gates::headers_for("GET", "users/me/profile/");
        assert_eq!(headers.cache_control, Some("private, max-age=12"));
        assert_eq!(headers.vary, Some("Cookie"));
        assert!(!headers.gzip);
    }

    // ------------------------------------------------------------------
    // Fixture replay (F-W24-15 routes + consumed shapes)
    // ------------------------------------------------------------------

    #[test]
    fn routes_table_names_all_six_paths() {
        let golden = fixture("handlers/routes.golden.json");
        let routes = golden["routes"].as_array().expect("routes is a list");
        let text = serde_json::to_string(routes).expect("routes serialize");
        for route in [
            "U06 GET+PATCH users/me/profile/",
            "U07 GET users/me/accounts/",
            "U08 users/me/accounts/<pk>/",
            "U14 GET users/me/workspaces/<slug>/activity-graph/",
            "U15 GET users/me/workspaces/<slug>/issues-completed-graph/",
            "U16 GET users/me/workspaces/<slug>/dashboard/",
        ] {
            assert!(text.contains(route), "missing route row: {route}");
        }
    }

    #[test]
    fn serializer_wire_orders_match_fixture_shapes() {
        // The consumed serializer ports (F-W24-04/05) drive the
        // renders above; the row structs must cover every wire key.
        assert_eq!(ser::PROFILE_WIRE_FIELDS.len(), 30);
        assert_eq!(ser::ACCOUNT_WIRE_FIELDS.len(), 13);
        assert!(ser::PROFILE_WIRE_FIELDS.contains(&"settings"));
        assert!(ser::PROFILE_WIRE_FIELDS.contains(&"user"));
        assert_eq!(ser::PROFILE_READ_ONLY_FIELDS, ["user", "settings"]);
        assert_eq!(ser::ACCOUNT_READ_ONLY_FIELDS, ["user"]);
    }

    #[test]
    fn dashboard_envelope_order_is_source_order() {
        assert_eq!(
            core_q::DASHBOARD_RESPONSE_KEYS,
            [
                "issue_activities",
                "completed_issues",
                "assigned_issues_count",
                "pending_issues_count",
                "completed_issues_count",
                "issues_due_week_count",
                "state_distribution",
                "overdue_issues",
                "upcoming_issues",
            ]
        );
        assert_eq!(
            core_q::OVERDUE_VALUES_FIELDS,
            ["id", "name", "workspace__slug", "project_id", "target_date"]
        );
        assert_eq!(
            core_q::UPCOMING_VALUES_FIELDS,
            ["id", "name", "workspace__slug", "project_id", "start_date"]
        );
    }

    // ------------------------------------------------------------------
    // Live reads (scratch DB): every new SeaORM statement executes.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn live_user_extras_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "users",
            "accounts",
            "profiles",
            "workspaces",
            "projects",
            "states",
            "issues",
            "issue_assignees",
            "issue_activities",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (uid, ws, project, state, issue, completed_issue, account) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO users (id, is_active, user_timezone) VALUES ('{uid}', true, 'UTC')"),
            format!(
                "INSERT INTO accounts (created_at, updated_at, id, provider_account_id, provider, \
                 access_token, access_token_expired_at, refresh_token, refresh_token_expired_at, \
                 last_connected_at, metadata, user_id, id_token) VALUES (now(), now(), '{account}', \
                 'pa', 'github', 'tok', NULL, NULL, NULL, now(), '{{}}'::jsonb, '{uid}', 'idt')"
            ),
            format!(
                "INSERT INTO profiles (created_at, updated_at, id, theme, is_tour_completed, \
                 onboarding_step, is_onboarded, billing_address_country, has_billing_address, \
                 company_name, user_id, is_mobile_onboarded, mobile_onboarding_step, \
                 mobile_timezone_auto_set, language, is_smooth_cursor_enabled, start_of_the_week, \
                 is_app_rail_docked, background_color, goals, has_marketing_email_consent, \
                 is_navigation_tour_completed, is_subscribed_to_changelog, notification_view_mode, \
                 product_tour, settings) VALUES (now(), now(), gen_random_uuid(), '{{}}'::jsonb, false, \
                 '{{}}'::jsonb, false, '', false, '', '{uid}', false, '{{}}'::jsonb, false, 'en', \
                 false, 0, false, '#fff', '{{}}'::jsonb, false, false, false, 'full', '{{}}'::jsonb, \
                 '{{}}'::jsonb)"
            ),
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!("INSERT INTO projects (id, workspace_id, deleted_at) VALUES ('{project}', '{ws}', NULL)"),
            format!(
                "INSERT INTO states (id, workspace_id, project_id, \"group\") \
                 VALUES ('{state}', '{ws}', '{project}', 'started')"
            ),
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, state_id, name, deleted_at, \
                 target_date, start_date, completed_at, is_draft) VALUES ('{issue}', '{ws}', \
                 '{project}', '{state}', 'Open issue', NULL, '2026-02-01'::date, \
                 '2026-03-01'::date, NULL, false)"
            ),
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, state_id, name, deleted_at, \
                 target_date, start_date, completed_at, is_draft) VALUES ('{completed_issue}', \
                 '{ws}', '{project}', '{state}', 'Done issue', NULL, NULL, NULL, \
                 '2026-02-06T12:00:00Z'::timestamptz, false)"
            ),
            format!(
                "INSERT INTO issue_assignees (id, assignee_id, issue_id, project_id, workspace_id, \
                 deleted_at) VALUES (gen_random_uuid(), '{uid}', '{completed_issue}', '{project}', \
                 '{ws}', NULL), (gen_random_uuid(), '{uid}', '{issue}', '{project}', '{ws}', NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);

        // Actor read.
        let actor: Option<(bool, Option<String>)> = users::Entity::find()
            .select_only()
            .columns([users::Column::IsActive, users::Column::UserTimezone])
            .filter(users::Column::Id.eq(uid))
            .into_tuple()
            .one(&db)
            .await
            .expect("actor");
        assert_eq!(actor, Some((true, Some("UTC".to_owned()))));

        // Account list / detail.
        let list = accounts::Entity::find()
            .filter(accounts::Column::UserId.eq(uid))
            .order_by_desc(accounts::Column::CreatedAt)
            .all(&db)
            .await
            .expect("accounts");
        assert_eq!(list.len(), 1);
        assert_eq!(DbAccount::from_model(list[0].clone()).id, account);
        let detail = accounts::Entity::find()
            .filter(accounts::Column::Id.eq(account))
            .filter(accounts::Column::UserId.eq(uid))
            .one(&db)
            .await
            .expect("detail")
            .expect("row");
        assert_eq!(detail.provider, "github");

        // Profile locked read over a transaction.
        let mut tx = pool.begin().await.expect("begin");
        let locked = {
            let conn = TxConn::new(&mut tx);
            profiles::Entity::find()
                .filter(profiles::Column::UserId.eq(uid))
                .lock_exclusive()
                .one(&conn)
                .await
                .expect("locked")
        };
        assert!(locked.is_some());
        tx.rollback().await.expect("rollback");

        // Graphs + dashboard.
        let activity = activity_graph_select(
            uid,
            "acme",
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            "UTC",
        )
        .into_model::<ActivityGraphRow>()
        .all(&db)
        .await
        .expect("activity");
        assert!(activity.is_empty());

        let completed = completed_graph_select(uid, "acme", 2, "UTC")
            .into_model::<CompletedGraphRow>()
            .all(&db)
            .await
            .expect("completed");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].completed_count, 1);

        let assigned = dashboard_issues_select()
            .filter(dashboard_tenant(uid, "acme"))
            .count(&db)
            .await
            .expect("count");
        assert_eq!(assigned, 2);

        let state_rows = dashboard_state_select(uid, "acme")
            .into_model::<StateDistributionRow>()
            .all(&db)
            .await
            .expect("state");
        assert_eq!(state_rows.len(), 1);
        assert_eq!(state_rows[0].state_group.as_deref(), Some("started"));

        let overdue = dashboard_values_select(
            uid,
            "acme",
            NaiveDate::from_ymd_opt(2026, 2, 6).unwrap(),
            true,
        )
        .into_model::<IssueValuesRow>()
        .all(&db)
        .await
        .expect("overdue");
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].workspace_slug, "acme");

        let upcoming = dashboard_values_select(
            uid,
            "acme",
            NaiveDate::from_ymd_opt(2026, 2, 6).unwrap(),
            false,
        )
        .into_model::<IssueValuesRow>()
        .all(&db)
        .await
        .expect("upcoming");
        assert_eq!(upcoming.len(), 1);
    }

    #[test]
    fn writable_fields_cover_every_non_readonly_wire_key() {
        let mut writable: Vec<&str> = WRITABLE_FIELDS.iter().map(|(name, _)| *name).collect();
        writable.sort_unstable();
        let mut expected: Vec<&str> = ser::PROFILE_WIRE_FIELDS
            .iter()
            .filter(|key| !["id", "created_at", "updated_at", "user", "settings"].contains(key))
            .copied()
            .collect();
        expected.sort_unstable();
        assert_eq!(writable, expected);
    }
}
