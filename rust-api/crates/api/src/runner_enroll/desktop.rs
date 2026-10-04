//! Desktop-app machine enrollment (D-13 handlers-E, PIDASHCONV-595).
//!
//! Ports `runner/views/desktop.py:65-194` (`DesktopEnrollEndpoint`
//! post+delete, route `dev-machines/desktop-enroll/` under
//! `/api/v1/runner/`):
//!
//! * POST mints (or rotates, idempotently per user + workspace + host) a
//!   `MachineToken` on a `DESKTOP_BUNDLED` dev machine: the `IsDesktopSession`
//!   gate, required-field 400s, the `_version_is_allowed` dotted-tuple floor
//!   (409), the workspace 404 (same body for missing/forbidden), the
//!   bundled-machine find-or-create, the per-(machine, workspace) token
//!   rotate, and the 201 body.
//! * DELETE revokes the caller's bundled-machine tokens and parks the
//!   bundled runners offline (rows survive); 204, including with an empty
//!   machine selection.
//!
//! # Execution model
//!
//! * Preamble: session via [`crate::license::resolve_actor`] (401 when
//!   anonymous — DRF reports `NotAuthenticated`, and the project's
//!   `auth_exception_handler` forces it back to 401), then the desktop
//!   marker via the [`pidash_auth::permissions::desktop`] kernel (403 with
//!   the exact `IsDesktopSession.message` body otherwise). The contract
//!   suite pins bothShapes byte for byte (`test_daemon_projects.py`).
//! * SQL text comes from the merged queries builders
//!   ([`catalog_reads`](pidash_services::runner_enroll::queries::catalog_reads)
//!   K-series); this module binds the documented `$N` params positionally
//!   and owns the `BEGIN`/`COMMIT` boundaries, plus the two unpinned
//!   statements the queries layer did not cover (workspace-by-slug, the
//!   `is_workspace_member` probe — same semantics as the fixtures' J2
//!   spellings).
//! * Token mints go through the merged services-A
//!   [`mint_machine_token`](pidash_services::runner_enroll::tokens::mint_machine_token);
//!   `now()` values are truncated to microseconds before bind (Django
//!   datetimes are microsecond-exact, while `chrono::Utc::now()` carries
//!   nanos that Postgres would *round* on store).
//! * Request bodies parse through this module's shared envelope
//!   ([`super::read_request_data`]); DRF parses DELETE bodies too
//!   (verified), so DELETE reads `request.data` exactly like POST.
//!
//! # Ported bugs and quirks (translate, don't redesign; also in the PR)
//!
//! * QUIRK-desktop-join-ignores-token-revoked (`desktop.py:115`): the
//!   bundled-machine lookup joins `machine_token` with NO `revoked_at`
//!   filter — even a fully-revoked token row makes the machine reusable.
//! * QUIRK-delete-body-or (`desktop.py:169`): the `or` applies to the RAW
//!   body value *before* stripping, so a whitespace-only body label selects
//!   the body (stripping to "all machines") rather than falling through to
//!   the query param ([`delete_host_label`]).
//! * QUIRK-version-500 (`desktop.py:49-56`): a version chunk whose digit
//!   filter keeps non-decimal numerics (e.g. `²`) raises `ValueError` in
//!   `int()` — an unhandled 500, ported as one ([`version_is_allowed`]).
//!
//! # Documented approximations
//!
//! * Unhandled failures answer the JSON 500 (`SERVER_ERROR_BODY`): Django
//!   renders its HTML error page there, so only the status is
//!   contract-pinned (the `runner_runs` precedent).
//! * CSRF is not replicated: the endpoint keeps DRF's default
//!   `SessionAuthentication` (unlike the `BaseSessionAuthentication`
//!   siblings), so Django 403s session POSTs/DELETEs without a CSRF token
//!   while this port serves them. No merged port replicates `enforce_csrf`;
//!   no fixture or contract test covers the rejection path.
//! * Non-decimal-numeric versions 500; non-ASCII *decimal* digits (e.g.
//!   Arabic-Indic `٢`, which Python compares numerically) also 500 here —
//!   `char::to_digit` is ASCII-only, so their values are unrecoverable in
//!   `std`. Real desktops send ASCII; no fixture covers the gap.
//! * `updated_at`/`created_at`/`last_seen_at`/`revoked_at` binds are this
//!   request's microsecond `now()`s, like Django's view-`now()` + `auto_now`
//!   calls (separate calls where the source makes separate calls).

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::Extension;
use axum::extract::Query;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Router;
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use pidash_services::runner_enroll::queries::catalog_reads;
use pidash_services::runner_enroll::tokens;

use crate::middleware::SessionHandle;
use crate::runner_runs::json_response;
use crate::runner_runs::pool_of;
use crate::runner_runs::server_error;
use crate::state::AppState;

use super::data_get;
use super::j_truthy;
use super::now_micros;
use super::or_empty_clean;
use super::or_empty_lossy;
use super::py_strip;
use super::read_request_data;
use super::JVal;

/// `dev-machines/desktop-enroll/` under `/api/v1/runner/`
/// (`runner/urls.py:74-78`).
pub const DESKTOP_ENROLL_PATH: &str = "/api/v1/runner/dev-machines/desktop-enroll/";

// ---------------------------------------------------------------------------
// Error bodies (D13-F7 `handlers/endpoints.golden.json`, key order verbatim)
// ---------------------------------------------------------------------------

/// `{"error": "workspace_slug is required"}` — 400 (`desktop.py:88`).
pub const WORKSPACE_SLUG_REQUIRED_BODY: &str = r#"{"error":"workspace_slug is required"}"#;
/// `{"error": "host_label is required"}` — 400 (`desktop.py:90`).
pub const HOST_LABEL_REQUIRED_BODY: &str = r#"{"error":"host_label is required"}"#;
/// `{"error": "workspace_not_found"}` — 404, same body for a missing
/// workspace and a forbidden one (`desktop.py:102-105`).
pub const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"error":"workspace_not_found"}"#;

/// Exact `IsDesktopSession.message` (`managed_runner/permissions.py:20`):
/// dict messages render as-is (DRF `permission_denied` raises with the dict
/// as `detail`, and the exception handler echoes it), so the wire body is
/// the dict itself. Byte-identical to the merged D-06 twin
/// (`crate::assistant::agent_profile::DESKTOP_DENIED_BODY`, asserted in
/// tests); restated here so this module has no cross-domain code edge.
pub const DESKTOP_DENIED_BODY: &str = "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}";

/// `RunnerProvisioning.DESKTOP_BUNDLED` value (`models.py:204-206`).
pub const PROVISIONING_DESKTOP_BUNDLED: &str = "desktop_bundled";
/// `RunnerStatus.OFFLINE` value (the sign-out sweep target).
pub const STATUS_OFFLINE: &str = "offline";
/// `RunnerStatus.REVOKED` value (excluded from the sweep).
pub const STATUS_REVOKED: &str = "revoked";

/// An owned path: the listed methods serve from Rust, every other method
/// falls through to Django (the `app_scheduler` precedent). `HEAD` proxies
/// too: Django has no GET here, so Django's own 405 answers (the
/// `agent_profile` token-path precedent).
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the desktop-enroll path (POST + DELETE owned). Merged under
/// `RouteGroup::Runner` at the F-10 seam; sibling handler issues extend
/// the merge, keeping both sides.
pub fn routes() -> Router<AppState> {
    use axum::routing::post;
    const OWNED: &[&str] = &["GET", "PUT", "PATCH", "OPTIONS", "HEAD"];
    Router::new().route(
        DESKTOP_ENROLL_PATH,
        owned(
            post(post_desktop_enroll).delete(delete_desktop_enroll),
            OWNED,
        ),
    )
}

// ---------------------------------------------------------------------------
// Version floor (`desktop.py:37-62`, D13-F6 `version_is_allowed`)
// ---------------------------------------------------------------------------

/// `ValueError` inside `_version_is_allowed` (`desktop.py:55`): the digit
/// filter kept a non-decimal numeric. Unhandled in the view → 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionError;

/// A dotted-version component as an unbounded integer: leading zeros
/// stripped (empty means zero), compared by length then lexically — Python
/// ints are unbounded, so no fixed width is faithful.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NormInt(String);

impl NormInt {
    fn new(digits: &str) -> Self {
        let trimmed = digits.trim_start_matches('0');
        Self(if trimmed.is_empty() {
            "0".to_owned()
        } else {
            trimmed.to_owned()
        })
    }
}

impl PartialOrd for NormInt {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NormInt {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

/// Whether Python's `int()` would accept the filtered digit run: ASCII
/// digits only. A non-ASCII numeric (`²`, `½`, but also `٢`) fails here —
/// `QUIRK-version-500` for the former (Python raises too), a documented
/// over-approximation for non-ASCII decimals (Python compares them).
fn all_ascii_digits(digits: &str) -> bool {
    digits.bytes().all(|b| b.is_ascii_digit())
}

/// `parse` (`desktop.py:49-56`): per-`.`-chunk digit filter + tuple, or
/// `None` when a chunk keeps no digits. `Err(VersionError)` is the source's
/// `ValueError` → 500.
fn parse_version(value: &str) -> Result<Option<Vec<NormInt>>, VersionError> {
    let mut parts = Vec::new();
    // `(value or "").strip().split(".")` — the caller passes `""` for a
    // missing value; `"".split('.')` yields one empty chunk in both
    // languages, which keeps no digits and returns `None`.
    for chunk in py_strip(value).split('.') {
        let mut digits = String::new();
        let mut non_ascii_numeric = false;
        for ch in chunk.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if ch.is_numeric() && !ch.is_alphabetic() {
                // Python `str.isdigit()` (≈ `Nd` + `No`): kept by the
                // filter, then rejected by `int()` unless decimal. The
                // `!is_alphabetic` arm drops letter-numbers (`Ⅷ`):
                // Python filters those too (`isdigit()` is false).
                non_ascii_numeric = true;
            }
            // Anything else is not a digit in either language: skipped.
        }
        if non_ascii_numeric || !digits.is_empty() && !all_ascii_digits(&digits) {
            return Err(VersionError);
        }
        if digits.is_empty() {
            return Ok(None);
        }
        parts.push(NormInt::new(&digits));
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts))
}

/// `_version_is_allowed` (`desktop.py:37-62`): blank floor allows;
/// unparseable floor *or* reported allows (fail-open); otherwise the
/// reported tuple must be `>=` the floor tuple (dotted integers, so
/// `0.10.0` sorts above `0.9.0`). `Err(VersionError)` is the source's `ValueError`
/// → 500 (`QUIRK-version-500`).
pub fn version_is_allowed(floor: &str, reported: &str) -> Result<bool, VersionError> {
    // `(settings.DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER or "").strip()` —
    // the settings layer already applies the `or ""` (empty default).
    if py_strip(floor).is_empty() {
        return Ok(true);
    }
    let wanted = parse_version(floor)?;
    let got = parse_version(reported)?;
    match (wanted, got) {
        (Some(wanted), Some(got)) => Ok(got >= wanted),
        _ => Ok(true),
    }
}

/// The 409 body (`desktop.py:92-100`): `error` before `error_description`,
/// the floor interpolated *raw* from settings (unstripped, `:96`).
#[derive(Serialize)]
struct UpdateRequiredBody<'a> {
    error: &'a str,
    error_description: String,
}

fn update_required_body(floor: &str) -> String {
    serde_json::to_string(&UpdateRequiredBody {
        error: "desktop_update_required",
        error_description: format!("Pi Dash Desktop {floor} or newer is required."),
    })
    .expect("json body")
}

// ---------------------------------------------------------------------------
// Preamble (session → desktop marker; DRF runs auth before permissions)
// ---------------------------------------------------------------------------

/// Session → `(pool, user_id, desktop)`: anonymous answers the DRF
/// `NotAuthenticated` 401, pool failures the JSON 500. The desktop flag is
/// borrowed from the session snapshot before `extension` moves into auth.
async fn desktop_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(sqlx::PgPool, Uuid, bool), Response> {
    let desktop = is_desktop_from_extension(&extension);
    let pool = pool_of(state)?.clone();
    let secret = state.settings().secret_key.clone();
    let actor = crate::license::resolve_actor(&pool, secret.as_bytes(), extension)
        .await
        .map_err(|_| server_error())?;
    let Some(actor) = actor else {
        return Err(json_response(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        ));
    };
    Ok((pool, actor.id, desktop))
}

/// Whether the request's session carries the CE desktop marker, through
/// the [`pidash_auth::permissions::desktop`] kernel. A missing snapshot,
/// a missing key, or a non-string value is not a desktop (the broad
/// `except` in `request_is_desktop` fails closed).
fn is_desktop_from_extension(extension: &Option<Extension<SessionHandle>>) -> bool {
    use pidash_auth::permissions::desktop::is_desktop_session;
    let data = extension.as_ref().map(|handle| handle.snapshot());
    let mut data = data.unwrap_or_else(crate::middleware::RequestSession::empty);
    let value = data
        .get(pidash_auth::permissions::desktop::DESKTOP_SESSION_KEY)
        .and_then(serde_json::Value::as_str);
    // `authenticated` is decided by the actor resolution that follows;
    // pass `true` here so this predicate is purely the marker check, and
    // let the 401 arm above own the anonymous case (DRF checks
    // authentication before permissions, so anon still 401s).
    is_desktop_session(true, value)
}

/// The desktop-only denial: 403 with the exact `IsDesktopSession`
/// message body.
fn desktop_denied() -> Response {
    json_response(StatusCode::FORBIDDEN, DESKTOP_DENIED_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// Own SQL (statements the queries layer did not cover)
// ---------------------------------------------------------------------------

/// Workspace by slug (`desktop.py:102`):
/// `Workspace.objects.filter(slug=…).first()` — manager scope, `Meta`
/// ordering (`-created_at`), `.first()` → `LIMIT 1`. Slug is unique, so
/// the order never breaks a tie; it is kept for statement fidelity.
///
/// `$1` = slug. Returns `(id, slug)`.
fn workspace_by_slug_sql() -> String {
    "SELECT \"workspaces\".\"id\", \"workspaces\".\"slug\" FROM \"workspaces\" \
     WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1) \
     ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1"
        .to_string()
}

/// `is_workspace_member` (`core/permissions.py:28-34`):
/// `WorkspaceMember.objects.filter(workspace_id, member=user,
/// is_active=True).exists()` — manager scope applies. Spelled after the
/// J2 probe plus the `is_active` conjunct (same semantics as Django's
/// single-filter `AND`).
///
/// `$1` = member (user) id, `$2` = workspace id.
fn workspace_member_sql() -> String {
    "SELECT 1 AS \"a\" FROM \"workspace_members\" \
     WHERE (\"workspace_members\".\"deleted_at\" IS NULL \
     AND \"workspace_members\".\"member_id\" = $1 \
     AND \"workspace_members\".\"workspace_id\" = $2 \
     AND \"workspace_members\".\"is_active\") LIMIT 1"
        .to_string()
}

// ---------------------------------------------------------------------------
// POST (`desktop.py:80-166`)
// ---------------------------------------------------------------------------

/// The 201 body (`desktop.py:157-166`), key order verbatim.
#[derive(Serialize)]
struct EnrollBody<'a> {
    dev_machine_id: String,
    machine_token: &'a str,
    workspace_slug: &'a str,
    managed_runner_enabled: bool,
    graceful_stop_seconds: i64,
}

async fn post_desktop_enroll(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let floor = state.settings().managed_runner.desktop_min_version.clone();
    let managed_enabled = state.settings().managed_runner.enabled;
    let graceful_stop_secs = state.settings().managed_runner.graceful_stop_secs;
    let secret_key = state.settings().secret_key.clone();

    let (pool, user_id, desktop) = match desktop_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(denial) => return denial,
    };
    if !desktop {
        return desktop_denied();
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(failure) => return failure,
    };
    // `:83-85`, in source order (the first bad `.strip()` wins its 500):
    // `(request.data.get(…) or "").strip()[:N]`.
    let workspace_raw = match data_get(&data, "workspace_slug") {
        Ok(raw) => raw,
        Err(failure) => return failure,
    };
    let host_raw = match data_get(&data, "host_label") {
        Ok(raw) => raw,
        Err(failure) => return failure,
    };
    let version_raw = match data_get(&data, "app_version") {
        Ok(raw) => raw,
        Err(failure) => return failure,
    };
    let workspace_slug = match or_empty_lossy(workspace_raw) {
        Ok(slug) => slug,
        Err(failure) => return failure,
    };
    let host_label =
        match or_empty_lossy(host_raw).map(|host| catalog_reads::truncate_chars(&host, 255)) {
            Ok(host) => host,
            Err(failure) => return failure,
        };
    let app_version = match or_empty_lossy(version_raw)
        .map(|version| catalog_reads::truncate_chars(&version, 32))
    {
        Ok(version) => version,
        Err(failure) => return failure,
    };

    if workspace_slug.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            WORKSPACE_SLUG_REQUIRED_BODY.to_owned(),
        );
    }
    if host_label.is_empty() {
        return json_response(StatusCode::BAD_REQUEST, HOST_LABEL_REQUIRED_BODY.to_owned());
    }
    match version_is_allowed(&floor, &app_version) {
        Ok(true) => {}
        Ok(false) => {
            return json_response(StatusCode::CONFLICT, update_required_body(&floor));
        }
        Err(VersionError) => return server_error(),
    }

    // Same 404 for "no such workspace" and "not yours" (`:103`).
    // Surrogate-cleanliness is enforced at the bind, after the pure
    // guards above, so the 409 still wins over the 500.
    let workspace_slug = match or_empty_clean(workspace_raw) {
        Ok(slug) => slug,
        Err(failure) => return failure,
    };
    let workspace: Option<(Uuid, String)> = match sqlx::query_as(&workspace_by_slug_sql())
        .bind(&workspace_slug)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some((workspace_id, slug)) = workspace else {
        return json_response(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND_BODY.to_owned());
    };
    let member: Option<i32> = match sqlx::query_scalar(&workspace_member_sql())
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    if member.is_none() {
        return json_response(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND_BODY.to_owned());
    }

    // K1 + K2 inside one transaction (`:107-149`).
    let host_label =
        match or_empty_clean(host_raw).map(|host| catalog_reads::truncate_chars(&host, 255)) {
            Ok(host) => host,
            Err(failure) => return failure,
        };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // K1 lookup: `$1` host, `$2` workspace, `$3` owner, `$4` provisioning.
    // Only the id (column 0) is read; the rest of the row is never
    // touched on this path.
    let found: Option<Uuid> = match sqlx::query(&catalog_reads::bundled_machine_lookup_sql())
        .bind(&host_label)
        .bind(workspace_id)
        .bind(user_id)
        .bind(PROVISIONING_DESKTOP_BUNDLED)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(row) => match row {
            None => None,
            Some(row) => match row.try_get::<Uuid, _>(0) {
                Ok(id) => Some(id),
                Err(_) => return server_error(),
            },
        },
        Err(_) => return server_error(),
    };
    let dev_machine_id = match found {
        Some(id) => {
            // Touch (`:128-130`): two `now()`s, like the explicit
            // assignment plus `auto_now`.
            if sqlx::query(&catalog_reads::dev_machine_touch_sql())
                .bind(now_micros())
                .bind(now_micros())
                .bind(id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            id
        }
        None => {
            // Insert (`:121-127`): `$1..$10` in column order.
            let id = Uuid::new_v4();
            let label = catalog_reads::dev_machine_label(&host_label);
            if sqlx::query(&catalog_reads::dev_machine_insert_sql())
                .bind(id)
                .bind(user_id)
                .bind(&host_label)
                .bind(label)
                .bind(0i16)
                .bind(PROVISIONING_DESKTOP_BUNDLED)
                .bind(now_micros())
                .bind(Option::<DateTime<Utc>>::None)
                .bind(now_micros())
                .bind(now_micros())
                .execute(&mut *tx)
                .await
                .is_err()
            {
                return server_error();
            }
            id
        }
    };

    // K2 rotate (`:134-138`): `$1` revoked_at, `$2` machine, `$3` workspace.
    if sqlx::query(&catalog_reads::desktop_token_revoke_sql())
        .bind(now_micros())
        .bind(dev_machine_id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    let minted = tokens::mint_machine_token(&secret_key);
    // K2 mint (`:139-149`): `$1..$12` in column order.
    let token_label = catalog_reads::desktop_token_label(&host_label);
    if sqlx::query(&catalog_reads::machine_token_insert_sql())
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(dev_machine_id)
        .bind(workspace_id)
        .bind(&host_label)
        .bind(&minted.hashed)
        .bind(&minted.fingerprint)
        .bind(token_label)
        .bind(true)
        .bind(now_micros())
        .bind(Option::<DateTime<Utc>>::None)
        .bind(Option::<DateTime<Utc>>::None)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if tx.commit().await.is_err() {
        return server_error();
    }

    tracing::info!(
        user_id = %user_id,
        dev_machine = %dev_machine_id,
        workspace = %workspace_id,
        "managed_runner.enrolled"
    );
    let body = serde_json::to_string(&EnrollBody {
        dev_machine_id: dev_machine_id.to_string(),
        machine_token: &minted.raw,
        workspace_slug: &slug,
        managed_runner_enabled: managed_enabled,
        graceful_stop_seconds: graceful_stop_secs,
    })
    .expect("json body");
    json_response(StatusCode::CREATED, body)
}

// ---------------------------------------------------------------------------
// DELETE (`desktop.py:168-194`)
// ---------------------------------------------------------------------------

/// `host_label` for the sign-out select (`desktop.py:169`):
/// `(request.data.get("host_label") or request.query_params.get("host_label")
/// or "").strip()[:255]` — the `or` applies to the RAW body value, so a
/// whitespace-only body label selects the body (stripping to "all machines")
/// rather than falling through to the query
/// (`QUIRK-delete-body-or`). Non-dict bodies 500 on `.get` before the
/// query is ever read.
fn delete_host_label(data: &JVal, params: &crate::license::QueryMap) -> Result<String, Response> {
    let raw = data_get(data, "host_label")?;
    let selected = match raw {
        Some(value) if j_truthy(value) => match value {
            JVal::Str(text) => text.to_lossy_string(),
            JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
                return Err(server_error());
            }
        },
        _ => crate::license::query_last(params, "host_label").unwrap_or_default(),
    };
    Ok(catalog_reads::truncate_chars(py_strip(&selected), 255))
}

async fn delete_desktop_enroll(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id, desktop) = match desktop_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(denial) => return denial,
    };
    if !desktop {
        return desktop_denied();
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(failure) => return failure,
    };
    let host_label = match delete_host_label(&data, &params) {
        Ok(host) => host,
        Err(failure) => return failure,
    };
    // The empty selection short-circuits to 204 with no transaction
    // (`:179-180`).
    let machine_ids: Vec<Uuid> = if host_label.is_empty() {
        match sqlx::query_scalar(&catalog_reads::bundled_machine_ids_sql(false))
            .bind(user_id)
            .bind(PROVISIONING_DESKTOP_BUNDLED)
            .fetch_all(&pool)
            .await
        {
            Ok(ids) => ids,
            Err(_) => return server_error(),
        }
    } else {
        // A surrogate-carrying body label 500s at the bind (Django's
        // `UnicodeEncodeError`); the query spelling is always clean.
        let body_raw = match data_get(&data, "host_label") {
            Ok(raw) => raw,
            Err(failure) => return failure,
        };
        if matches!(body_raw, Some(value) if j_truthy(value)) {
            match or_empty_clean(body_raw) {
                Ok(_) => {}
                Err(failure) => return failure,
            }
        }
        match sqlx::query_scalar(&catalog_reads::bundled_machine_ids_sql(true))
            .bind(user_id)
            .bind(PROVISIONING_DESKTOP_BUNDLED)
            .bind(&host_label)
            .fetch_all(&pool)
            .await
        {
            Ok(ids) => ids,
            Err(_) => return server_error(),
        }
    };
    if machine_ids.is_empty() {
        return json_response(StatusCode::NO_CONTENT, String::new());
    }

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // `$1` revoked_at, `$2..` machine ids in list order.
    let revoke_sql = catalog_reads::signout_revoke_tokens_sql(machine_ids.len());
    let mut revoke = sqlx::query(&revoke_sql).bind(now_micros());
    for id in &machine_ids {
        revoke = revoke.bind(id);
    }
    if revoke.execute(&mut *tx).await.is_err() {
        return server_error();
    }
    // `$1` status, `$2..` ids, `$N+2` provisioning, `$N+3` revoked.
    let sweep_sql = catalog_reads::signout_offline_runners_sql(machine_ids.len());
    let mut sweep = sqlx::query(&sweep_sql).bind(STATUS_OFFLINE);
    for id in &machine_ids {
        sweep = sweep.bind(id);
    }
    sweep = sweep
        .bind(PROVISIONING_DESKTOP_BUNDLED)
        .bind(STATUS_REVOKED);
    if sweep.execute(&mut *tx).await.is_err() {
        return server_error();
    }
    if tx.commit().await.is_err() {
        return server_error();
    }

    tracing::info!(
        user_id = %user_id,
        machines = machine_ids.len(),
        "managed_runner.removed"
    );
    json_response(StatusCode::NO_CONTENT, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_FLOWS: &str =
        include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
    const FIXTURE_ENDPOINTS: &str =
        include_str!("../../../../fixtures/runner_enroll/handlers/endpoints.golden.json");

    /// D13-F6 `version_is_allowed`: every golden case passes verbatim
    /// (the `reported` strings carry their `(…)` annotations inline —
    /// they parse the same way Python parses them).
    #[test]
    fn version_goldens_match_f6() {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_FLOWS).expect("flows fixture parses");
        let cases = fixture["version_is_allowed"]["cases"]
            .as_array()
            .expect("cases array");
        assert_eq!(cases.len(), 13, "F6 pins 13 version cases");
        for case in cases {
            let floor = case["floor"].as_str().expect("floor str");
            let reported = case["reported"].as_str().expect("reported str");
            let allowed = case["allowed"].as_bool().expect("allowed bool");
            assert_eq!(
                version_is_allowed(floor, reported),
                Ok(allowed),
                "floor={floor:?} reported={reported:?}"
            );
        }
    }

    /// `_version_is_allowed` edges past the goldens (verified against the
    /// real `parse` semantics): empty chunks fail open, huge ints compare
    /// numerically, non-decimal numerics 500.
    #[test]
    fn version_edges() {
        // Empty/whitespace reports fail open.
        assert_eq!(version_is_allowed("1.2.0", ""), Ok(true));
        assert_eq!(version_is_allowed("1.2.0", "   "), Ok(true));
        // Empty chunks fail open (`1..2`, `.1`, `1.`).
        assert_eq!(version_is_allowed("1.2.0", "1..2"), Ok(true));
        assert_eq!(version_is_allowed("0.0.1", ".1"), Ok(true));
        assert_eq!(version_is_allowed("0.0.1", "1."), Ok(true));
        // Unbounded components compare numerically, not lexically.
        assert_eq!(
            version_is_allowed("9.9.9", "99999999999999999999999.0.0"),
            Ok(true)
        );
        assert_eq!(
            version_is_allowed("99999999999999999999999.0.0", "9.9.9"),
            Ok(false)
        );
        // Leading zeros are insignificant.
        assert_eq!(version_is_allowed("1.2.0", "01.02.00"), Ok(true));
        // `²` is `isdigit()` but not `int()` → `ValueError` → 500.
        assert_eq!(version_is_allowed("1.2.0", "².0"), Err(VersionError));
        assert_eq!(version_is_allowed("².0", "1.2.0"), Err(VersionError));
        // Letter-numbers (`Ⅷ`) are filtered like Python filters them.
        assert_eq!(version_is_allowed("1.2.0", "Ⅷ.0.0"), Ok(true));
        // Exact equality allows.
        assert_eq!(version_is_allowed("1.2.0", "1.2.0"), Ok(true));
        // Shorter floor tuple still compares (`(1,2,0) >= (1,2)`).
        assert_eq!(version_is_allowed("1.2", "1.2.0"), Ok(true));
        assert_eq!(version_is_allowed("1.2.1", "1.2"), Ok(false));
        // Floor whitespace is stripped for the gate but the 409
        // interpolates it raw (see below).
        assert_eq!(version_is_allowed("  1.2.0  ", "1.2.0"), Ok(true));
    }

    /// The 403 denial body is byte-identical to the merged D-06 twin and
    /// to the lowercase-`detail` source dict (the key case is asserted
    /// here precisely because it is easy to misread).
    #[test]
    fn denied_body_matches_source_and_precedent() {
        assert_eq!(
            DESKTOP_DENIED_BODY,
            crate::assistant::agent_profile::DESKTOP_DENIED_BODY
        );
        let parsed: serde_json::Value =
            serde_json::from_str(DESKTOP_DENIED_BODY).expect("denial parses");
        assert_eq!(parsed["error"], "desktop_session_required");
        assert_eq!(
            parsed["detail"],
            "This endpoint is available to the Pi Dash desktop app."
        );
        assert!(parsed.get("Detail").is_none(), "lowercase key only");
        assert_eq!(
            DESKTOP_DENIED_BODY,
            "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}"
        );
    }

    /// D13-F7 desktop errors: the 400s and the 404 match the golden
    /// bodies; the 409 matches with the floor interpolated.
    #[test]
    fn f7_desktop_errors_match_consts() {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_ENDPOINTS).expect("endpoints fixture parses");
        let endpoint = &fixture["daemon"]["POST_desktop_enroll"];
        assert_eq!(
            endpoint["route"],
            "POST /api/v1/runner/dev-machines/desktop-enroll/"
        );
        let errors = endpoint["errors"].as_array().expect("errors array");
        assert_eq!(errors.len(), 4);
        assert_eq!(errors[0]["status"], 400);
        assert_eq!(
            serde_json::to_string(&errors[0]["body"]).expect("json"),
            WORKSPACE_SLUG_REQUIRED_BODY
        );
        assert_eq!(errors[1]["status"], 400);
        assert_eq!(
            serde_json::to_string(&errors[1]["body"]).expect("json"),
            HOST_LABEL_REQUIRED_BODY
        );
        assert_eq!(errors[2]["status"], 409);
        assert_eq!(errors[2]["body"]["error"], "desktop_update_required");
        // The golden spells the floor as `<floor>`; the port
        // interpolates the raw settings value in that position.
        let rendered: serde_json::Value =
            serde_json::from_str(&update_required_body("0.5.0")).expect("409 parses");
        assert_eq!(rendered["error"], errors[2]["body"]["error"]);
        assert_eq!(
            rendered["error_description"],
            "Pi Dash Desktop 0.5.0 or newer is required."
        );
        assert_eq!(errors[3]["status"], 404);
        assert_eq!(
            serde_json::to_string(&errors[3]["body"]).expect("json"),
            WORKSPACE_NOT_FOUND_BODY
        );
        let delete = &fixture["daemon"]["DELETE_desktop_enroll"];
        assert_eq!(
            delete["route"],
            "DELETE /api/v1/runner/dev-machines/desktop-enroll/"
        );
        assert!(delete["ok"].as_str().expect("ok str").starts_with("204"));
    }

    /// The 409 interpolates the floor RAW (unstripped, `:96`) and stays
    /// valid compact JSON with `error` first.
    #[test]
    fn update_required_interpolates_raw_floor() {
        let body = update_required_body("  0.5.0\"quoted\"  ");
        assert_eq!(
            body,
            "{\"error\":\"desktop_update_required\",\"error_description\":\"Pi Dash Desktop   0.5.0\\\"quoted\\\"   or newer is required.\"}"
        );
    }

    /// The 201 body keeps the source key order.
    #[test]
    fn enroll_body_key_order() {
        let body = serde_json::to_string(&EnrollBody {
            dev_machine_id: "00000000-0000-0000-0000-000000000000".to_owned(),
            machine_token: "mt_raw",
            workspace_slug: "ws",
            managed_runner_enabled: true,
            graceful_stop_seconds: 30,
        })
        .expect("json");
        assert_eq!(
            body,
            "{\"dev_machine_id\":\"00000000-0000-0000-0000-000000000000\",\"machine_token\":\"mt_raw\",\"workspace_slug\":\"ws\",\"managed_runner_enabled\":true,\"graceful_stop_seconds\":30}"
        );
    }

    /// The two own-SQL statements keep the J2 spelling: manager scope,
    /// binds, ordering, limit.
    #[test]
    fn own_sql_shapes() {
        let workspace = workspace_by_slug_sql();
        assert!(workspace.contains("FROM \"workspaces\""), "{workspace}");
        assert!(workspace.contains("\"deleted_at\" IS NULL"), "{workspace}");
        assert!(workspace.contains("\"slug\" = $1"), "{workspace}");
        assert!(
            workspace.contains("ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1"),
            "{workspace}"
        );
        let member = workspace_member_sql();
        assert!(member.contains("FROM \"workspace_members\""), "{member}");
        assert!(member.contains("\"deleted_at\" IS NULL"), "{member}");
        assert!(member.contains("\"member_id\" = $1"), "{member}");
        assert!(member.contains("\"workspace_id\" = $2"), "{member}");
        assert!(member.contains("\"is_active\""), "{member}");
        assert!(member.contains("LIMIT 1"), "{member}");
    }

    fn query_map(pairs: &[(&str, &str)]) -> crate::license::QueryMap {
        use crate::license::OneOrMany;
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OneOrMany::One((*value).to_owned())))
            .collect()
    }

    fn parse_body(json: &str) -> JVal {
        crate::v1_cycles_modules::json_cpython::parse_request_bytes(json.as_bytes())
            .expect("test json parses")
    }

    /// `QUIRK-delete-body-or`: the raw body value wins over the query
    /// param even when it strips to empty; only a *falsy* body value
    /// falls through.
    #[test]
    fn delete_host_label_body_or_query() {
        let params = query_map(&[("host_label", "from-query")]);
        // Body label wins.
        assert_eq!(
            delete_host_label(&parse_body(r#"{"host_label": "from-body"}"#), &params)
                .expect("body wins"),
            "from-body"
        );
        // Whitespace-only body strips to "" — and does NOT fall through
        // to the query (the `or` ran before the strip).
        assert_eq!(
            delete_host_label(&parse_body(r#"{"host_label": "   "}"#), &params)
                .expect("body selected"),
            ""
        );
        // Falsy body values fall through to the query.
        assert_eq!(
            delete_host_label(&parse_body(r#"{"host_label": ""}"#), &params).expect("query"),
            "from-query"
        );
        assert_eq!(
            delete_host_label(&parse_body(r#"{}"#), &params).expect("query"),
            "from-query"
        );
        assert_eq!(
            delete_host_label(&parse_body(r#"{"host_label": null}"#), &params).expect("query"),
            "from-query"
        );
        // Neither → "" (all machines).
        assert_eq!(
            delete_host_label(&parse_body(r#"{}"#), &query_map(&[])).expect("empty"),
            ""
        );
        // Truncation applies after the strip.
        let long = "h".repeat(300);
        assert_eq!(
            delete_host_label(
                &parse_body(&format!(r#"{{"host_label": "  {long}  "}}"#)),
                &query_map(&[])
            )
            .expect("truncated")
            .len(),
            255
        );
    }

    /// Non-dict DELETE bodies 500 on `.get` before the query is read.
    #[test]
    fn delete_host_label_non_dict_500s() {
        let params = query_map(&[("host_label", "from-query")]);
        for raw in ["[1]", "\"s\"", "1", "true", "null"] {
            let response = delete_host_label(&parse_body(raw), &params).expect_err("500s");
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        // Truthy non-string labels 500 on `.strip()`.
        let response =
            delete_host_label(&parse_body(r#"{"host_label": 123}"#), &params).expect_err("500s");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
