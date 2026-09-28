//! D-01 instance console handlers (stage 3).
//!
//! Port of `apps/api/pi_dash/license/api/views/instance.py`:
//!
//! * `InstanceEndpoint.get` (`:36-173`) — [`get_instance`].
//! * `InstanceEndpoint.patch` (`:176-183`) — [`patch_instance`].
//! * `SignUpScreenVisitedEndpoint.post` (`:190-199`) —
//!   [`signup_screen_visited`].
//!
//! Routes mirror `license/urls.py`: `""` (mounted at `api/instances/`) and
//! `admins/sign-up-screen-visited/`. Registration is the cutover
//! granularity: owned methods serve from Rust, every other method on these
//! paths proxies to Django (its 401-before-405 ordering, 405 bodies and
//! redirects live there).
//!
//! Shared base behavior (`TimezoneMixin`, the exception matrix,
//! `Cache-Control`, denials) lives in [`super::handlers_base`]; the
//! instance row, permission gate and config resolver come from the merged
//! D-01 foundation (`pidash_db::license`, `pidash_auth::license`,
//! `pidash_services::license`, `pidash_types::license`).
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-4 (`instance.py:46-48` vs `:128`): GET builds `serializer.data`
//!   plus `is_activated = True`, then discards it (`data = {}`); the
//!   instance branch never carries `is_activated`. Reproduced: the
//!   response `instance` object has no such key.
//! * Partial-update null-skip: DRF drops `None` values when the serializer
//!   is `partial` (`validate_empty_values` raises `SkipField`), so an
//!   explicit `null` on PATCH is silently ignored for every field —
//!   including non-nullable ones. Reproduced in [`validate_partial`].
//! * `read_only_fields` (`instance.py:17`) lists `email`, a model field
//!   that does not exist: unknown input keys are ignored, so `email` (and
//!   any other unknown key) never validates and never writes.
//! * `dispatch` returns the exception object instead of the mapped
//!   response on error paths (`base.py:107-109`); unreachable because the
//!   `handle_exception` override never re-raises — see
//!   [`super::handlers_base`].
//!
//! Deliberate non-ports (no observable difference under the contract
//! suite; noted for follow-up):
//!
//! * `@cache_response(2h)` / `@invalidate_cache`: the contract suite wipes
//!   the database between tests without flushing the cache, so Django
//!   under test demonstrably does not serve cached rows (any 2h cache
//!   would go stale across tests); an in-process Rust cache would
//!   therefore diverge. Reads stay live; `Cache-Control: private,
//!   max-age=12` is still emitted on GET.
//! * `settings.DEBUG` query-count print (`base.py:101-104`): dropped, the
//!   print is not a response behavior.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

// NOTE: `pidash_auth::license` (PIDASHCONV-118) exists on disk but is not
// wired into the auth crate root yet, so handlers use the F-06 kernel
// directly — the same decision function the domain wrapper delegates to.
use pidash_auth::permissions::instance::decide_instance_admin;
use pidash_db::config::accessor::PgConfigStore;
use pidash_db::config::encryption::Keyring;
use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
use pidash_db::config::registry;
use pidash_db::config::value::ConfigValue;
use pidash_db::license::queries::{
    admin_permission_check_sql, fetch_instance_first, ADMIN_PERMISSION_ROLE_GTE,
};
use pidash_types::license::serializers_core::{instance_to_representation, InstanceRow};

use super::handlers_base::{json_response, resolve_request_tz, HandlerError, CACHE_CONTROL_VALUE};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Register the owned D-01 instance routes. Unowned methods fall through
/// to Django through the edge fallback (never a Rust 405).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/",
            get(get_instance)
                .patch(patch_instance)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/instances/admins/sign-up-screen-visited/",
            post(signup_screen_visited)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Session actor (BaseSessionAuthentication without CSRF)
// ---------------------------------------------------------------------------

/// The request's user: `None` is anonymous. Mirrors DRF
/// `SessionAuthentication.authenticate`: a missing session, an unknown
/// user id, or an inactive user all authenticate as nobody (guarded routes
/// then answer 401, `AllowAny` routes proceed).
struct Actor {
    id: Uuid,
    timezone: Tz,
}

async fn request_actor(
    pool: &sqlx::PgPool,
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
    let row: Option<(bool, Option<String>)> =
        sqlx::query_as("SELECT is_active, user_timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((true, timezone)) => {
            let tz = resolve_request_tz(true, timezone.as_deref())?;
            Ok(Some(Actor { id, timezone: tz }))
        }
        _ => Ok(None),
    }
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// GET /api/instances/
// ---------------------------------------------------------------------------

/// `InstanceEndpoint.get` (`instance.py:36-173`): `AllowAny`. No instance
/// row → `{"is_activated": false, "is_setup_done": false}` (`:40-44`);
/// otherwise `{"config": ..., "instance": ...}` with the 15-key derivation
/// (`:66-167`) plus `workspaces_exist` (`:169-170`).
async fn get_instance(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<axum::response::Response, HandlerError> {
    let pool = pool_of(&state)?;
    let actor = request_actor(pool, extension).await?;
    let timezone = actor.map(|a| a.timezone).unwrap_or(chrono_tz::UTC);
    let row = fetch_instance_first(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let Some(row) = row else {
        return Ok(with_cache_control(json_response(
            StatusCode::OK,
            r#"{"is_activated":false,"is_setup_done":false}"#.to_owned(),
        )));
    };
    let config = build_config(&state, pool).await?;
    let workspaces_exist = count_workspaces(pool).await? >= 1;
    let instance_body = render_instance(&row, timezone, Some(workspaces_exist));
    let mut body = serde_json::Map::with_capacity(2);
    body.insert(
        "config".to_owned(),
        serde_json::to_value(&config).map_err(|_| HandlerError::ServerError)?,
    );
    body.insert("instance".to_owned(), instance_body);
    Ok(with_cache_control(json_response(
        StatusCode::OK,
        serde_json::Value::Object(body).to_string(),
    )))
}

/// `Workspace.objects.count() >= 1` (`instance.py:170`): the default
/// manager excludes soft-deleted rows.
async fn count_workspaces(pool: &sqlx::PgPool) -> Result<i64, HandlerError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM workspaces WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// GET config derivation (instance.py:50-167)
// ---------------------------------------------------------------------------

/// The 15 `get_configuration_value` items with the view's inline env
/// defaults (`instance.py:66-126`), `os.environ.get` evaluated at call
/// time.
fn config_items() -> Vec<LegacyItem> {
    let env_default = |key: &str, fallback: ConfigValue| {
        let default = match std::env::var(key) {
            Ok(v) => ConfigValue::Str(v),
            Err(_) => fallback,
        };
        LegacyItem::new(key, default)
    };
    let zero = || ConfigValue::Str("0".to_owned());
    let one = || ConfigValue::Str("1".to_owned());
    let empty = || ConfigValue::Str(String::new());
    vec![
        env_default("ENABLE_SIGNUP", zero()),
        env_default("DISABLE_WORKSPACE_CREATION", zero()),
        env_default("IS_GOOGLE_ENABLED", zero()),
        env_default("IS_GITHUB_ENABLED", zero()),
        env_default("GITHUB_APP_NAME", empty()),
        env_default("IS_GITLAB_ENABLED", zero()),
        env_default("IS_GITEA_ENABLED", zero()),
        env_default("EMAIL_HOST", empty()),
        env_default("ENABLE_MAGIC_LINK_LOGIN", one()),
        env_default("ENABLE_EMAIL_PASSWORD", one()),
        env_default("SLACK_CLIENT_ID", ConfigValue::Null),
        env_default("POSTHOG_API_KEY", ConfigValue::Null),
        env_default("POSTHOG_HOST", ConfigValue::Null),
        env_default("UNSPLASH_ACCESS_KEY", empty()),
        env_default("LLM_API_KEY", empty()),
    ]
}

/// `{"config": ...}` in Python insertion order (`instance.py:128-167`).
#[derive(Debug, Clone, PartialEq, Serialize)]
struct ConfigPayload {
    enable_signup: bool,
    is_workspace_creation_disabled: bool,
    is_google_enabled: bool,
    is_github_enabled: bool,
    is_gitlab_enabled: bool,
    is_gitea_enabled: bool,
    is_magic_login_enabled: bool,
    is_email_password_enabled: bool,
    github_app_name: String,
    slack_client_id: Option<String>,
    posthog_api_key: Option<String>,
    posthog_host: Option<String>,
    has_unsplash_configured: bool,
    has_llm_configured: bool,
    file_size_limit: f64,
    is_smtp_configured: bool,
    admin_base_url: Option<String>,
    space_base_url: Option<String>,
    app_base_url: Option<String>,
    instance_changelog_url: String,
    is_self_managed: bool,
}

/// Python `str(value)`: `None` renders `"None"`, booleans `"True"` /
/// `"False"`, numbers plain (`GITHUB_APP_NAME`, `instance.py:140`).
fn py_str(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Null => "None".to_owned(),
        ConfigValue::Str(s) => s.clone(),
        ConfigValue::Int(i) => i.to_string(),
        ConfigValue::Float(f) => value.as_display().unwrap_or_else(|| f.to_string()),
        ConfigValue::Bool(true) => "True".to_owned(),
        ConfigValue::Bool(false) => "False".to_owned(),
    }
}

/// Python truthiness: empty string / `None` / zero are false
/// (`bool(UNSPLASH_ACCESS_KEY)`, `instance.py:150-159`).
fn py_truthy(value: &ConfigValue) -> bool {
    match value {
        ConfigValue::Null => false,
        ConfigValue::Str(s) => !s.is_empty(),
        ConfigValue::Int(i) => *i != 0,
        ConfigValue::Float(f) => *f != 0.0,
        ConfigValue::Bool(b) => *b,
    }
}

/// `None` stays `null`, strings verbatim (`slack_client_id`, `posthog_*`,
/// `instance.py:143-147`).
fn opt_str(value: &ConfigValue) -> Option<String> {
    match value {
        ConfigValue::Null => None,
        ConfigValue::Str(s) => Some(s.clone()),
        other => Some(py_str(other)),
    }
}

/// `float(os.environ.get("FILE_SIZE_LIMIT", 5242880))`
/// (`instance.py:156`): the default is the int `5242880`; a non-numeric
/// value raises (`ValueError` → 500 through the exception matrix).
fn parse_file_size_limit(raw: Option<String>) -> Result<f64, HandlerError> {
    match raw {
        None => Ok(5242880.0),
        Some(text) => ConfigValue::Str(text)
            .to_float()
            .ok_or(HandlerError::ServerError),
    }
}

async fn build_config(
    state: &AppState,
    pool: &sqlx::PgPool,
) -> Result<ConfigPayload, HandlerError> {
    let store = PgConfigStore::new(pool.clone());
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let values = get_configuration_values(registry::global(), &store, &keyring, &config_items())
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let urls = &state.settings().urls;
    Ok(ConfigPayload {
        enable_signup: values[0].is_flag_set(),
        is_workspace_creation_disabled: values[1].is_flag_set(),
        is_google_enabled: values[2].is_flag_set(),
        is_github_enabled: values[3].is_flag_set(),
        is_gitlab_enabled: values[5].is_flag_set(),
        is_gitea_enabled: values[6].is_flag_set(),
        is_magic_login_enabled: values[8].is_flag_set(),
        is_email_password_enabled: values[9].is_flag_set(),
        github_app_name: py_str(&values[4]),
        slack_client_id: opt_str(&values[10]),
        posthog_api_key: opt_str(&values[11]),
        posthog_host: opt_str(&values[12]),
        has_unsplash_configured: py_truthy(&values[13]),
        has_llm_configured: py_truthy(&values[14]),
        file_size_limit: parse_file_size_limit(std::env::var("FILE_SIZE_LIMIT").ok())?,
        is_smtp_configured: py_truthy(&values[7]),
        admin_base_url: urls.admin_base_url.clone(),
        space_base_url: urls.space_base_url.clone(),
        app_base_url: urls.app_base_url.clone(),
        instance_changelog_url: state.settings().instance_changelog_url.clone(),
        // `IS_SELF_MANAGED = True` (`settings/common.py:36`); no override
        // in any settings module, so the port is a literal.
        is_self_managed: true,
    })
}

// ---------------------------------------------------------------------------
// Instance rendering (InstanceSerializer + workspaces_exist)
// ---------------------------------------------------------------------------

/// Render one instance row as `InstanceSerializer` output
/// (`serializers/instance.py:11-17`, `fields = "__all__"`, BUG-1: no
/// `primary_owner_details` key) with datetimes in the request zone, plus
/// `workspaces_exist` appended last when given (GET only, `:169-170`).
fn render_instance(
    row: &pidash_db::license::models::instance::Instance,
    timezone: Tz,
    workspaces_exist: Option<bool>,
) -> serde_json::Value {
    use crate::serializer::render_datetime_in;
    let id = row.id.to_string();
    let created_at = render_datetime_in(&row.created_at, &timezone);
    let updated_at = render_datetime_in(&row.updated_at, &timezone);
    let deleted_at = row
        .deleted_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, &timezone));
    let last_checked_at = render_datetime_in(&row.last_checked_at, &timezone);
    let created_by = row.created_by_id.as_ref().map(Uuid::to_string);
    let updated_by = row.updated_by_id.as_ref().map(Uuid::to_string);
    let view_row = InstanceRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        instance_name: &row.instance_name,
        whitelist_emails: row.whitelist_emails.as_deref(),
        instance_id: &row.instance_id,
        current_version: &row.current_version,
        latest_version: row.latest_version.as_deref(),
        edition: &row.edition,
        domain: &row.domain,
        last_checked_at: &last_checked_at,
        namespace: row.namespace.as_deref(),
        is_telemetry_enabled: row.is_telemetry_enabled,
        is_support_required: row.is_support_required,
        is_setup_done: row.is_setup_done,
        is_signup_screen_visited: row.is_signup_screen_visited,
        is_verified: row.is_verified,
        is_test: row.is_test,
        is_current_version_deprecated: row.is_current_version_deprecated,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = instance_to_representation(&view_row);
    let mut object = serde_json::to_value(&view)
        .expect("instance view serializes")
        .as_object()
        .cloned()
        .expect("instance view is an object");
    if let Some(exists) = workspaces_exist {
        object.insert("workspaces_exist".to_owned(), exists.into());
    }
    serde_json::Value::Object(object)
}

// ---------------------------------------------------------------------------
// PATCH /api/instances/
// ---------------------------------------------------------------------------

/// JSON value kinds by DRF's `type(data).__name__` for the non-dict error
/// (`serializers.py:485`). Python ints are unbounded, so every integer
/// token — however large — is `"int"`: the token shape (no `.`/`e`/`E`)
/// discriminates, which `arbitrary_precision` preserves verbatim.
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

/// One validated column write. The `NullableUuid` display string is the
/// original pk rendering DRF echoes in its `does_not_exist` message
/// (`Invalid pk "<display>" - object does not exist.`): the raw string for
/// string pks, the canonical decimal for integer pks.
#[derive(Debug, Clone, PartialEq)]
enum Assignment {
    Text(&'static str, String),
    NullableText(&'static str, Option<String>),
    Flag(&'static str, bool),
    NullableUuid(&'static str, Option<(Uuid, String)>),
}

/// PATCH validation outcome: assignments or per-field errors
/// (`serializer.errors`). Every pk shape, including malformed UUIDs, maps
/// to a per-field error: DRF catches Django's `ValidationError` inside
/// `PrimaryKeyRelatedField` (`serializers.py:506`).
#[derive(Debug, Clone, PartialEq)]
enum PatchRejection {
    FieldErrors(Vec<(String, Vec<String>)>),
}

const MAX_NAME_LEN: usize = 255;

/// DRF `CharField` input: bools, objects and arrays are `"Not a valid
/// string."`; numbers coerce via `str()`; strings are stripped
/// (`trim_whitespace=True`), so blank-check, length-count and storage all
/// see the trimmed value.
fn as_text(value: &serde_json::Value) -> Result<String, &'static str> {
    match value {
        serde_json::Value::String(s) => Ok(s.trim().to_owned()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err("Not a valid string."),
    }
}

/// Python `str()` of a float, for pk error displays: shortest round-trip
/// digits with `e±XX` exponents (`1.0`, `1.5`, `1e+16`, `1e-05`).
fn py_float_str(n: f64) -> String {
    if n.is_nan() {
        return "nan".to_owned();
    }
    if n.is_infinite() {
        return if n.is_sign_positive() {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
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

/// Python `repr()` of a string: single quotes unless the value holds one
/// (and no double quote), with backslash and control escapes.
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
            c if (c < '\u{20}') || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a JSON value (container elements, dict keys).
fn py_repr_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(n) => py_number_str(n),
        serde_json::Value::String(s) => py_repr_str(s),
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", inner.join(", "))
        }
        serde_json::Value::Object(object) => {
            let inner: Vec<String> = object
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_str(key), py_repr_value(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `str()` of a JSON value: bare strings, `repr()` otherwise.
fn py_str_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => py_repr_value(value),
    }
}

/// Python `str()` of a JSON number: decimal for ints, [`py_float_str`]
/// for floats. Int-vs-float follows the token shape (no `.`/`e`/`E` is an
/// int), which `arbitrary_precision` preserves verbatim.
fn py_number_str(n: &serde_json::Number) -> String {
    let token = n.to_string();
    if token.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
        py_float_str(n.as_f64().unwrap_or(f64::NAN))
    } else {
        token
    }
}

/// Django `UUIDField.get_prep_value` for an integer pk: `uuid.UUID(int=…)`,
/// valid exactly for `[0, 2**128)`. Returns the UUID plus the canonical
/// decimal DRF echoes in both the lookup and the failure messages.
fn uuid_from_int_token(token: &str) -> Option<(Uuid, String)> {
    if let Ok(v) = token.parse::<i128>() {
        if v >= 0 {
            return Some((Uuid::from_u128(v as u128), v.to_string()));
        }
        return None;
    }
    token
        .parse::<u128>()
        .ok()
        .map(|v| (Uuid::from_u128(v), v.to_string()))
}

/// The Django `UUIDField` failure message, curly quotes included
/// (Django emits them raw UTF-8: the renderer runs `ensure_ascii=False`).
fn invalid_uuid_message(display: &str) -> String {
    format!("\u{201c}{display}\u{201d} is not a valid UUID.")
}

/// DRF `BooleanField` input: the strict true/false sets plus `1` / `0`
/// (`1.0 == 1` in Python, so float ones count too).
fn as_bool(value: &serde_json::Value) -> Result<bool, &'static str> {
    match value {
        serde_json::Value::Bool(b) => Ok(*b),
        serde_json::Value::Number(n) => {
            if n.as_f64() == Some(1.0) {
                Ok(true)
            } else if n.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err("Must be a valid boolean.")
            }
        }
        serde_json::Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "1" | "true" | "t" | "y" | "yes" | "on" => Ok(true),
            "0" | "false" | "f" | "n" | "no" | "off" => Ok(false),
            _ => Err("Must be a valid boolean."),
        },
        _ => Err("Must be a valid boolean."),
    }
}

/// Validate one writable text field (`blank` / `max_length` in that order,
/// after the partial null-skip resolved by the caller). `max_length` is
/// `Some` for `CharField`s, `None` for `TextField`s (no length validator).
fn check_text(
    field: &'static str,
    value: &serde_json::Value,
    allow_blank: bool,
    max_length: Option<usize>,
    errors: &mut Vec<(String, Vec<String>)>,
) -> Option<String> {
    match as_text(value) {
        Err(message) => {
            errors.push((field.to_owned(), vec![message.to_owned()]));
            None
        }
        Ok(text) => {
            if text.is_empty() && !allow_blank {
                errors.push((
                    field.to_owned(),
                    vec!["This field may not be blank.".to_owned()],
                ));
                return None;
            }
            // `max_length` counts characters (code points).
            if let Some(limit) = max_length {
                if text.chars().count() > limit {
                    errors.push((
                        field.to_owned(),
                        vec![format!(
                            "Ensure this field has no more than {limit} characters."
                        )],
                    ));
                    return None;
                }
            }
            Some(text)
        }
    }
}

/// `InstanceSerializer(instance, data, partial=True)` validation
/// (`instance.py:179-180`): unknown and read-only keys ignored, explicit
/// `null`s skipped (partial `SkipField`), the rest validated in wire
/// order with every error collected.
fn validate_partial(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<Assignment>, PatchRejection> {
    let mut sets = Vec::new();
    let mut errors: Vec<(String, Vec<String>)> = Vec::new();
    let get = |field: &str| match object.get(field) {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value),
    };

    // `CharField(max_length=255)` vs `TextField` (no length validator).
    const CAPPED: Option<usize> = Some(MAX_NAME_LEN);
    if let Some(value) = get("instance_name") {
        if let Some(text) = check_text("instance_name", value, false, CAPPED, &mut errors) {
            sets.push(Assignment::Text("instance_name", text));
        }
    }
    if let Some(value) = get("whitelist_emails") {
        if let Some(text) = check_text("whitelist_emails", value, true, None, &mut errors) {
            sets.push(Assignment::NullableText("whitelist_emails", Some(text)));
        }
    }
    if let Some(value) = get("instance_id") {
        if let Some(text) = check_text("instance_id", value, false, CAPPED, &mut errors) {
            sets.push(Assignment::Text("instance_id", text));
        }
    }
    if let Some(value) = get("current_version") {
        if let Some(text) = check_text("current_version", value, false, CAPPED, &mut errors) {
            sets.push(Assignment::Text("current_version", text));
        }
    }
    if let Some(value) = get("latest_version") {
        if let Some(text) = check_text("latest_version", value, true, CAPPED, &mut errors) {
            sets.push(Assignment::NullableText("latest_version", Some(text)));
        }
    }
    if let Some(value) = get("edition") {
        if let Some(text) = check_text("edition", value, false, CAPPED, &mut errors) {
            sets.push(Assignment::Text("edition", text));
        }
    }
    if let Some(value) = get("domain") {
        if let Some(text) = check_text("domain", value, true, None, &mut errors) {
            sets.push(Assignment::Text("domain", text));
        }
    }
    if let Some(value) = get("namespace") {
        if let Some(text) = check_text("namespace", value, true, CAPPED, &mut errors) {
            sets.push(Assignment::NullableText("namespace", Some(text)));
        }
    }
    for field in [
        "is_telemetry_enabled",
        "is_support_required",
        "is_signup_screen_visited",
        "is_verified",
        "is_test",
        "is_current_version_deprecated",
    ] {
        if let Some(value) = get(field) {
            match as_bool(value) {
                Ok(flag) => sets.push(Assignment::Flag(field, flag)),
                Err(message) => errors.push((field.to_owned(), vec![message.to_owned()])),
            }
        }
    }
    // `created_by` / `updated_by`: writable `PrimaryKeyRelatedField`s over
    // users. DRF catches every conversion failure per field
    // (`serializers.py:506`), so each shape below is a field error: bools
    // are `incorrect_type`; strings parse as UUIDs; integers coerce through
    // `uuid.UUID(int=…)` (valid for `[0, 2**128)`) and are looked up;
    // floats, lists and dicts fail with the Python `str()` of the value.
    for field in ["created_by", "updated_by"] {
        if let Some(value) = get(field) {
            match value {
                serde_json::Value::Bool(_) => errors.push((
                    field.to_owned(),
                    vec![format!(
                        "Incorrect type. Expected pk value, received {}.",
                        datatype_name(value)
                    )],
                )),
                serde_json::Value::String(raw) => match raw.parse::<Uuid>() {
                    Ok(id) => sets.push(Assignment::NullableUuid(field, Some((id, raw.clone())))),
                    Err(_) => errors.push((field.to_owned(), vec![invalid_uuid_message(raw)])),
                },
                serde_json::Value::Number(n) => {
                    let token = n.to_string();
                    if token.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
                        errors.push((
                            field.to_owned(),
                            vec![invalid_uuid_message(&py_number_str(n))],
                        ));
                    } else {
                        match uuid_from_int_token(&token) {
                            Some((id, display)) => {
                                sets.push(Assignment::NullableUuid(field, Some((id, display))));
                            }
                            None => {
                                errors.push((field.to_owned(), vec![invalid_uuid_message(&token)]))
                            }
                        }
                    }
                }
                other => errors.push((
                    field.to_owned(),
                    vec![invalid_uuid_message(&py_str_value(other))],
                )),
            }
        }
    }

    if errors.is_empty() {
        Ok(sets)
    } else {
        Err(PatchRejection::FieldErrors(errors))
    }
}

/// Render `serializer.errors`: `{"field": ["message", ...]}` in wire
/// order, compact separators (DRF `JSONRenderer`, `COMPACT_JSON`).
fn render_field_errors(errors: &[(String, Vec<String>)]) -> String {
    let mut body = String::from("{");
    for (index, (field, messages)) in errors.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&serde_json::to_string(field).expect("field name"));
        body.push(':');
        body.push_str(&serde_json::to_string(messages).expect("messages"));
    }
    body.push('}');
    body
}

/// `InstanceEndpoint.patch` (`instance.py:176-183`): `PATCH` requires
/// `InstanceAdminPermission` (`get_permissions`, `:29-32`); partial
/// serializer save; `400` on errors.
async fn patch_instance(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, HandlerError> {
    let pool = pool_of(&state)?;
    // Authentication runs before permission (DRF `initial`), and the body
    // is only parsed inside the handler (`request.data`): no valid
    // session answers 401 even for a malformed body, never 403 or 400.
    let actor = request_actor(pool, extension)
        .await?
        .ok_or(HandlerError::Unauthorized)?;
    if !admin_allowed(pool, &actor.id).await? {
        return Err(HandlerError::Forbidden);
    }
    let value = patch_body(&headers, &body)?;
    let object = match &value {
        serde_json::Value::Object(object) => object.clone(),
        other => {
            return Err(HandlerError::BadDetail(
                serde_json::json!({
                    "non_field_errors": [
                        format!(
                            "Invalid data. Expected a dictionary, but got {}.",
                            datatype_name(other)
                        )
                    ]
                })
                .to_string(),
            ));
        }
    };
    let sets = validate_partial(&object).map_err(|rejection| {
        let PatchRejection::FieldErrors(errors) = rejection;
        HandlerError::FieldErrors(render_field_errors(&errors))
    })?;
    let row = fetch_instance_first(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    // `instance_id` unique check (DRF `UniqueValidator`), excluding the
    // row being patched.
    if let Some(Assignment::Text("instance_id", candidate)) = sets
        .iter()
        .find(|set| matches!(set, Assignment::Text("instance_id", _)))
    {
        reject_duplicate_instance_id(pool, candidate, row.as_ref().map(|row| row.id)).await?;
    }
    // `created_by` / `updated_by` existence (DRF `does_not_exist`).
    check_user_assignments(pool, &sets).await?;
    match row {
        Some(current) => {
            apply_patch(pool, &current.id, &sets, &actor.id).await?;
        }
        // No row: `ModelSerializer.save()` with `instance=None` creates.
        // Partial mode still skips missing fields, so Django column
        // defaults fill the gaps and the database enforces the rest.
        None => {
            insert_instance(pool, &sets, &actor.id).await?;
        }
    }
    let updated = fetch_instance_first(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?
        .ok_or(HandlerError::ServerError)?;
    Ok(json_response(
        StatusCode::OK,
        render_instance(&updated, actor.timezone, None).to_string(),
    ))
}

/// Decode the PATCH body like DRF: empty → `{}`; otherwise JSON by
/// content-type (anything else → 400 with the same body; DRF answers 415
/// here, a documented divergence), parse errors → 400 `detail`.
fn patch_body(headers: &HeaderMap, body: &[u8]) -> Result<serde_json::Value, HandlerError> {
    if body.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
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
            serde_json::json!({
                "detail": format!("Unsupported media type \"{content_type}\" in request.")
            })
            .to_string(),
        ));
    }
    serde_json::from_slice(body).map_err(|e| {
        HandlerError::BadDetail(
            serde_json::json!({"detail": format!("JSON parse error - {e}")}).to_string(),
        )
    })
}

/// `InstanceAdminPermission.has_permission` (`permissions/instance.py`):
/// anonymous denies without a DB hit; otherwise the first instance must
/// have an admin row for the user with `role >= 15`.
async fn admin_allowed(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<bool, HandlerError> {
    let row = fetch_instance_first(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let Some(instance) = row else {
        // No first instance: `filter(instance=None, ...)` matches nothing.
        return Ok(decide_instance_admin(true, false, false));
    };
    // The check SQL selects the full row (fixture
    // `queries/admin_crud.sql` §1); only presence matters (`.exists()`).
    let hit = sqlx::query(&admin_permission_check_sql())
        .bind(instance.id)
        .bind(ADMIN_PERMISSION_ROLE_GTE)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(decide_instance_admin(true, true, hit.is_some()))
}

/// DRF `UniqueValidator` on `instance_id`: another live row with the same
/// value → `{"instance_id": ["This field must be unique."]}`.
async fn reject_duplicate_instance_id(
    pool: &sqlx::PgPool,
    candidate: &str,
    exclude: Option<Uuid>,
) -> Result<(), HandlerError> {
    let (count,): (i64,) = if let Some(id) = exclude {
        sqlx::query_as(
            "SELECT COUNT(*) FROM instances WHERE instance_id = $1 AND deleted_at IS NULL AND id != $2",
        )
        .bind(candidate)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?
    } else {
        sqlx::query_as(
            "SELECT COUNT(*) FROM instances WHERE instance_id = $1 AND deleted_at IS NULL",
        )
        .bind(candidate)
        .fetch_one(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?
    };
    if count > 0 {
        return Err(HandlerError::FieldErrors(
            r#"{"instance_id":["This field must be unique."]}"#.to_owned(),
        ));
    }
    Ok(())
}

/// DRF `PrimaryKeyRelatedField(queryset=User.objects.all())`:
/// `{"created_by": ["Invalid pk \"...\" - object does not exist."]}`.
async fn check_user_assignments(
    pool: &sqlx::PgPool,
    sets: &[Assignment],
) -> Result<(), HandlerError> {
    let mut errors = Vec::new();
    for set in sets {
        if let Assignment::NullableUuid(field, Some((id, display))) = set {
            let hit: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|_| HandlerError::ServerError)?;
            if hit.is_none() {
                errors.push((
                    (*field).to_owned(),
                    vec![format!("Invalid pk \"{display}\" - object does not exist.")],
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(HandlerError::FieldErrors(render_field_errors(&errors)))
    }
}

/// `serializer.save()` on the update path: validated columns plus
/// `updated_at = now` (`auto_now`) and `updated_by = request.user`
/// (`BaseModel.save`, `db/models/base.py:26-40`).
async fn apply_patch(
    pool: &sqlx::PgPool,
    id: &Uuid,
    sets: &[Assignment],
    user_id: &Uuid,
) -> Result<(), HandlerError> {
    let now = chrono::Utc::now();
    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE instances SET ");
    let mut first = true;
    for set in sets {
        if !first {
            builder.push(", ");
        }
        first = false;
        push_assignment(&mut builder, set);
    }
    if !first {
        builder.push(", ");
    }
    builder.push("updated_at = ");
    builder.push_bind(now);
    builder.push(", updated_by_id = ");
    builder.push_bind(*user_id);
    builder.push(" WHERE id = ");
    builder.push_bind(*id);
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(())
}

/// One `SET column = value` fragment for an [`Assignment`].
fn push_assignment(builder: &mut QueryBuilder<Postgres>, set: &Assignment) {
    match set {
        Assignment::Text(column, text) | Assignment::NullableText(column, Some(text)) => {
            builder.push(format!("{column} = "));
            builder.push_bind(text.clone());
        }
        Assignment::NullableText(column, None) => {
            builder.push(format!("{column} = NULL"));
        }
        Assignment::Flag(column, flag) => {
            builder.push(format!("{column} = "));
            builder.push_bind(*flag);
        }
        Assignment::NullableUuid(column, Some((target, _))) => {
            builder.push(format!("{column}_id = "));
            builder.push_bind(*target);
        }
        Assignment::NullableUuid(column, None) => {
            builder.push(format!("{column}_id = NULL"));
        }
    }
}

fn assignment_column(set: &Assignment) -> &'static str {
    match set {
        Assignment::Text(column, _)
        | Assignment::NullableText(column, _)
        | Assignment::Flag(column, _)
        | Assignment::NullableUuid(column, _) => column,
    }
}

/// `serializer.save()` with `instance=None`: create. Missing columns fall
/// back to the Django field defaults (`edition`, `domain`, untouched
/// flags); required columns left out fail at the database — the same
/// `IntegrityError` → 500 path Python takes through its dispatch.
async fn insert_instance(
    pool: &sqlx::PgPool,
    sets: &[Assignment],
    user_id: &Uuid,
) -> Result<(), HandlerError> {
    let now = chrono::Utc::now();
    let mut columns: Vec<&'static str> = Vec::new();
    for set in sets {
        let column = assignment_column(set);
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    // Django field defaults for untouched columns (`blank`/default
    // fields); required columns absent here fail at the database.
    // (`edition`, `domain`, and every boolean flag carry model defaults;
    // nullable text falls back to NULL at the database.)
    const DEFAULT_FLAGS: &[(&str, bool)] = &[
        ("is_telemetry_enabled", true),
        ("is_support_required", true),
        ("is_setup_done", false),
        ("is_signup_screen_visited", false),
        ("is_verified", false),
        ("is_test", false),
        ("is_current_version_deprecated", false),
    ];
    for column in ["edition", "domain"] {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    for (column, _) in DEFAULT_FLAGS {
        if !columns.contains(column) {
            columns.push(column);
        }
    }
    // Django assigns the UUID client-side pre-insert; without the `v4`
    // feature on the `uuid` crate the equivalent unobservable choice is
    // `gen_random_uuid()` (same v4 shape, no wire difference).
    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO instances (id, created_at, updated_at, created_by_id, updated_by_id",
    );
    for column in &columns {
        builder.push(", ");
        builder.push(*column);
        if sets
            .iter()
            .find(|set| assignment_column(set) == *column)
            .is_some_and(|set| matches!(set, Assignment::NullableUuid(_, _)))
        {
            builder.push("_id");
        }
    }
    builder.push(") VALUES (gen_random_uuid(), ");
    builder.push_bind(now);
    builder.push(", ");
    builder.push_bind(now);
    builder.push(", ");
    // Creating: `created_by = user`, `updated_by = None`
    // (`db/models/base.py:26-40`).
    builder.push_bind(*user_id);
    builder.push(", NULL");
    for column in &columns {
        builder.push(", ");
        match sets.iter().find(|set| assignment_column(set) == *column) {
            Some(Assignment::Text(_, text)) | Some(Assignment::NullableText(_, Some(text))) => {
                builder.push_bind(text.clone());
            }
            Some(Assignment::NullableText(_, None)) => {
                builder.push("NULL");
            }
            Some(Assignment::Flag(_, flag)) => {
                builder.push_bind(*flag);
            }
            Some(Assignment::NullableUuid(_, Some((target, _)))) => {
                builder.push_bind(*target);
            }
            Some(Assignment::NullableUuid(_, None)) => {
                builder.push("NULL");
            }
            None if *column == "edition" => {
                builder.push_bind("PI_DASH_COMMUNITY".to_owned());
            }
            None if *column == "domain" => {
                builder.push_bind(String::new());
            }
            None => {
                let flag = DEFAULT_FLAGS
                    .iter()
                    .find(|(name, _)| name == column)
                    .map(|(_, value)| *value)
                    .unwrap_or(false);
                builder.push_bind(flag);
            }
        }
    }
    builder.push(")");
    builder
        .build()
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// POST /api/instances/admins/sign-up-screen-visited/
// ---------------------------------------------------------------------------

/// `SignUpScreenVisitedEndpoint.post` (`instance.py:190-199`): `AllowAny`;
/// no instance → 400 `"Instance is not configured"`; otherwise stamp the
/// flag and answer 204.
async fn signup_screen_visited(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<axum::response::Response, HandlerError> {
    let pool = pool_of(&state)?;
    // `TimezoneMixin.initial` runs before the handler body: resolve the
    // actor first so a bad stored zone 500s before the instance check,
    // exactly like Django.
    let actor = request_actor(pool, extension).await?;
    let row = fetch_instance_first(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let Some(instance) = row else {
        return Err(HandlerError::BadError(
            r#"{"error":"Instance is not configured"}"#.to_owned(),
        ));
    };
    // `BaseModel.save` stamps `updated_by` with the current user when there
    // is one (`db/models/base.py:26-40`); anonymous posts leave it NULL.
    let now = chrono::Utc::now();
    if let Some(actor) = actor {
        sqlx::query(
            "UPDATE instances SET is_signup_screen_visited = TRUE, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(actor.id)
        .bind(instance.id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    } else {
        sqlx::query(
            "UPDATE instances SET is_signup_screen_visited = TRUE, updated_at = $1, updated_by_id = NULL WHERE id = $2",
        )
        .bind(now)
        .bind(instance.id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    }
    Ok(axum::response::Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("204 response"))
}

/// Attach `Cache-Control: private, max-age=12` to a response (the
/// `@cache_control` decorator on GET, `instance.py:35`).
fn with_cache_control(response: axum::response::Response) -> axum::response::Response {
    let (mut parts, body) = response.into_parts();
    parts.headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static(CACHE_CONTROL_VALUE),
    );
    axum::response::Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::license::queries::INSTANCE_FIRST_SQL;

    fn obj(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().cloned().expect("object")
    }

    #[test]
    fn patch_rejects_overlong_name_like_drf() {
        let body = obj(serde_json::json!({"instance_name": "n".repeat(300)}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"instance_name":["Ensure this field has no more than 255 characters."]}"#
        );
    }

    #[test]
    fn patch_ignores_read_only_and_unknown_keys() {
        // `is_setup_done` (read-only), `email` (unknown), `id` (read-only):
        // all ignored, nothing else to write, no errors.
        let body = obj(serde_json::json!({
            "instance_name": "Kept",
            "is_setup_done": true,
            "email": "x@y.zz",
            "id": "00000000-0000-0000-0000-000000000000",
            "last_checked_at": "2026-01-01T00:00:00Z",
            "primary_owner_details": {},
        }));
        let sets = validate_partial(&body).expect("valid");
        assert_eq!(
            sets,
            vec![Assignment::Text("instance_name", "Kept".to_owned())]
        );
    }

    #[test]
    fn patch_skips_explicit_nulls_like_partial_serializers() {
        let body = obj(serde_json::json!({
            "instance_name": null,
            "whitelist_emails": null,
            "is_verified": null,
            "created_by": null,
        }));
        assert_eq!(validate_partial(&body).expect("nulls skipped"), vec![]);
    }

    #[test]
    fn patch_blank_and_boolean_edges_match_drf() {
        let body = obj(serde_json::json!({"instance_name": ""}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"instance_name":["This field may not be blank."]}"#
        );

        let body = obj(serde_json::json!({"domain": ""}));
        assert!(validate_partial(&body)
            .expect("blank ok")
            .contains(&Assignment::Text("domain", String::new())));

        for (raw, want) in [
            (serde_json::json!(true), true),
            (serde_json::json!("True"), true),
            (serde_json::json!("1"), true),
            (serde_json::json!(1), true),
            (serde_json::json!(1.0), true),
            (serde_json::json!("off"), false),
            (serde_json::json!(0), false),
        ] {
            let body = obj(serde_json::json!({"is_test": raw}));
            assert_eq!(
                validate_partial(&body).expect("bool"),
                vec![Assignment::Flag("is_test", want)],
                "input {raw}"
            );
        }
        let body = obj(serde_json::json!({"is_test": "maybe"}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"is_test":["Must be a valid boolean."]}"#
        );
        let body = obj(serde_json::json!({"instance_name": true}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"instance_name":["Not a valid string."]}"#
        );
    }

    #[test]
    fn patch_text_fields_have_no_length_limit() {
        // `domain` is a `TextField`: DRF attaches no `max_length`
        // validator, so 300 chars validate where `instance_name` 400s.
        let body = obj(serde_json::json!({"domain": "x".repeat(300)}));
        assert_eq!(
            validate_partial(&body).expect("text field uncapped"),
            vec![Assignment::Text("domain", "x".repeat(300))]
        );
    }

    #[test]
    fn patch_numbers_coerce_to_text_like_drf() {
        let body = obj(serde_json::json!({"instance_name": 123}));
        assert_eq!(
            validate_partial(&body).expect("number coerces"),
            vec![Assignment::Text("instance_name", "123".to_owned())]
        );
    }

    #[test]
    fn patch_max_length_counts_characters() {
        // 255 CJK code points (765 UTF-8 bytes) are within the limit.
        let body = obj(serde_json::json!({"instance_name": "あ".repeat(255)}));
        assert!(validate_partial(&body).is_ok());
        let body = obj(serde_json::json!({"instance_name": "あ".repeat(256)}));
        assert!(validate_partial(&body).is_err());
    }

    #[test]
    fn patch_pk_errors_match_drf_per_field() {
        // DRF catches every pk conversion failure per field
        // (`serializers.py:506`): only bools are `incorrect_type`; every
        // other malformed shape is a `“… is not a valid UUID.”` field
        // error. Bodies parsed from raw JSON so the token path (including
        // `arbitrary_precision` echo) matches the wire.
        let uuid_msg = |display: &str| format!("\u{201c}{display}\u{201d} is not a valid UUID.");
        let field_error = |field: &str, message: &str| {
            format!(
                "{{\"{field}\":[{msg}]}}",
                msg = serde_json::to_string(message).expect("message")
            )
        };
        // Malformed string: UUID message with the raw value (curly quotes
        // U+201C/U+201D, raw UTF-8 on the wire like Django's
        // `ensure_ascii=False` renderer).
        let body: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(r#"{"created_by": "not-a-uuid"}"#).expect("json");
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        let rendered = render_field_errors(&errors);
        assert_eq!(rendered, field_error("created_by", &uuid_msg("not-a-uuid")));
        assert!(
            rendered
                .as_bytes()
                .windows(3)
                .any(|w| w == [0xe2, 0x80, 0x9c]),
            "curly quotes are raw UTF-8, not \\u escapes: {rendered:?}"
        );
        // Ints in `[0, 2**128)` coerce via `uuid.UUID(int=…)` and are
        // looked up: validation passes with the canonical decimal display.
        for (raw, display) in [
            ("42", "42"),
            ("0", "0"),
            (
                "340282366920938463463374607431768211455",
                "340282366920938463463374607431768211455",
            ),
        ] {
            let body: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&format!(r#"{{"created_by": {raw}}}"#)).expect("json");
            let sets = validate_partial(&body).expect("int pk validates");
            assert_eq!(
                sets,
                vec![Assignment::NullableUuid(
                    "created_by",
                    Some((
                        Uuid::from_u128(display.parse::<u128>().expect("u128")),
                        display.to_owned()
                    )),
                )],
                "input {raw}"
            );
        }
        // Out-of-range ints fail with the decimal display.
        for raw in ["340282366920938463463374607431768211456", "-5"] {
            let body: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&format!(r#"{{"created_by": {raw}}}"#)).expect("json");
            let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
                panic!("expected field errors for {raw}");
            };
            assert_eq!(
                render_field_errors(&errors),
                field_error("created_by", &uuid_msg(raw)),
                "input {raw}"
            );
        }
        // Floats fail with the Python `str()` of the value.
        for (raw, display) in [("1.5", "1.5"), ("1.0", "1.0"), ("1e3", "1000.0")] {
            let body: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&format!(r#"{{"created_by": {raw}}}"#)).expect("json");
            let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
                panic!("expected field errors for {raw}");
            };
            assert_eq!(
                render_field_errors(&errors),
                field_error("created_by", &uuid_msg(display)),
                "input {raw}"
            );
        }
        // Lists and dicts fail with the Python `str()` (single quotes).
        for (raw, display) in [(r#"["a"]"#, "['a']"), (r#"{"a": 1}"#, "{'a': 1}")] {
            let body: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&format!(r#"{{"created_by": {raw}}}"#)).expect("json");
            let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
                panic!("expected field errors for {raw}");
            };
            assert_eq!(
                render_field_errors(&errors),
                field_error("created_by", &uuid_msg(display)),
                "input {raw}"
            );
        }
        // Bools stay `incorrect_type`; `updated_by` mirrors `created_by`.
        let body = obj(serde_json::json!({"created_by": true}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"created_by":["Incorrect type. Expected pk value, received bool."]}"#
        );
        let body = obj(serde_json::json!({"updated_by": "not-a-uuid"}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            field_error("updated_by", &uuid_msg("not-a-uuid"))
        );
    }

    #[test]
    fn patch_namespace_and_latest_version_cap_at_255() {
        // Both are `CharField(max_length=255)` (`models/instance.py:28,33`);
        // the `TextField`s (`whitelist_emails`, `domain`) stay uncapped.
        for field in ["namespace", "latest_version"] {
            let body = obj(serde_json::json!({field: "x".repeat(300)}));
            let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
                panic!("expected field errors for {field}");
            };
            assert_eq!(
                render_field_errors(&errors),
                format!("{{\"{field}\":[\"Ensure this field has no more than 255 characters.\"]}}"),
                "field {field}"
            );
            let body = obj(serde_json::json!({field: "x".repeat(255)}));
            assert!(validate_partial(&body).is_ok(), "field {field}");
        }
    }

    #[test]
    fn patch_trims_text_like_drf() {
        // DRF `trim_whitespace`: whitespace-only fails blank-guarded
        // fields, surrounding runs are stripped before store.
        let body = obj(serde_json::json!({"instance_name": "   "}));
        let Err(PatchRejection::FieldErrors(errors)) = validate_partial(&body) else {
            panic!("expected field errors");
        };
        assert_eq!(
            render_field_errors(&errors),
            r#"{"instance_name":["This field may not be blank."]}"#
        );
        let body = obj(serde_json::json!({"instance_name": "  x  "}));
        assert_eq!(
            validate_partial(&body).expect("trimmed"),
            vec![Assignment::Text("instance_name", "x".to_owned())]
        );
        let body = obj(serde_json::json!({"domain": "   "}));
        assert_eq!(
            validate_partial(&body).expect("blank ok"),
            vec![Assignment::Text("domain", String::new())]
        );
    }

    #[test]
    fn non_dict_bodies_report_python_type_names() {
        // `type(data).__name__` (`serializers.py:485`): Python ints are
        // unbounded, so every integer token — however large — is "int".
        // Bodies parsed from raw JSON so the token path (including
        // `arbitrary_precision` echo) matches the wire.
        for (raw, want) in [
            ("42", "int"),
            ("-5", "int"),
            ("9223372036854775807", "int"),
            ("9223372036854775808", "int"),
            ("18446744073709551616", "int"),
            ("340282366920938463463374607431768211455", "int"),
            ("1.5", "float"),
            ("1.0", "float"),
            ("1e3", "float"),
            ("1E5", "float"),
            ("true", "bool"),
            ("\"x\"", "str"),
            ("[1]", "list"),
            ("{\"a\": 1}", "dict"),
            ("null", "NoneType"),
        ] {
            let value: serde_json::Value = serde_json::from_str(raw).expect("json");
            assert_eq!(datatype_name(&value), want, "input {raw}");
        }
        // The full non-field-errors body for a bare big-int PATCH body.
        let value: serde_json::Value =
            serde_json::from_str("340282366920938463463374607431768211455").expect("json");
        let body = serde_json::json!({
            "non_field_errors": [
                format!(
                    "Invalid data. Expected a dictionary, but got {}.",
                    datatype_name(&value)
                )
            ]
        })
        .to_string();
        assert_eq!(
            body,
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got int."]}"#
        );
    }

    #[test]
    fn config_derivation_matches_fixture_shapes() {
        // `get_present.config_derivation` in
        // fixtures/license/handlers/instance.golden.json: =="1" flags,
        // str()/bool() wrappers, verbatim nulls.
        let one = ConfigValue::Str("1".to_owned());
        let zero = ConfigValue::Str("0".to_owned());
        assert!(one.is_flag_set());
        assert!(!zero.is_flag_set());
        assert_eq!(py_str(&ConfigValue::Null), "None");
        assert_eq!(py_str(&ConfigValue::Bool(true)), "True");
        assert_eq!(opt_str(&ConfigValue::Null), None);
        assert_eq!(
            opt_str(&ConfigValue::Str("https://posthog.example".to_owned())),
            Some("https://posthog.example".to_owned())
        );
        assert!(py_truthy(&ConfigValue::Str("key".to_owned())));
        assert!(!py_truthy(&ConfigValue::Str(String::new())));
        // Semantic trap: Python `bool("0")` is True (non-empty string);
        // the `=="1"` flags use `is_flag_set`, never truthiness.
        assert!(py_truthy(&zero));
        assert!(!py_truthy(&ConfigValue::Null));
        assert_eq!(parse_file_size_limit(None).unwrap(), 5242880.0);
        assert_eq!(
            parse_file_size_limit(Some("100".to_owned())).unwrap(),
            100.0
        );
        assert!(parse_file_size_limit(Some("lots".to_owned())).is_err());
    }

    #[test]
    fn config_payload_key_order_matches_contract() {
        // CONFIG_KEYS in contract-tests/license/test_instance.py.
        let payload = ConfigPayload {
            enable_signup: true,
            is_workspace_creation_disabled: false,
            is_google_enabled: false,
            is_github_enabled: false,
            is_gitlab_enabled: false,
            is_gitea_enabled: false,
            is_magic_login_enabled: true,
            is_email_password_enabled: true,
            github_app_name: String::new(),
            slack_client_id: None,
            posthog_api_key: None,
            posthog_host: None,
            has_unsplash_configured: false,
            has_llm_configured: false,
            file_size_limit: 5242880.0,
            is_smtp_configured: false,
            admin_base_url: None,
            space_base_url: None,
            app_base_url: None,
            instance_changelog_url: "https://example.test/changelog".to_owned(),
            is_self_managed: true,
        };
        let text = serde_json::to_string(&payload).expect("config serializes");
        assert!(text.contains("\"file_size_limit\":5242880.0"));
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "enable_signup",
                "is_workspace_creation_disabled",
                "is_google_enabled",
                "is_github_enabled",
                "is_gitlab_enabled",
                "is_gitea_enabled",
                "is_magic_login_enabled",
                "is_email_password_enabled",
                "github_app_name",
                "slack_client_id",
                "posthog_api_key",
                "posthog_host",
                "has_unsplash_configured",
                "has_llm_configured",
                "file_size_limit",
                "is_smtp_configured",
                "admin_base_url",
                "space_base_url",
                "app_base_url",
                "instance_changelog_url",
                "is_self_managed",
            ]
        );
    }

    #[test]
    fn instance_query_text_is_the_django_shape() {
        // The exact text Django's compiler emits, shared with the queries
        // layer fixture (queries/instance_first.sql).
        assert!(INSTANCE_FIRST_SQL.contains("FROM \"instances\""));
        assert!(INSTANCE_FIRST_SQL.contains("ORDER BY \"instances\".\"created_at\" DESC LIMIT 1"));
    }
}
