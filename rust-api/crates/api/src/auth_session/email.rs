//! Email-session HTTP wiring (D-16, PIDASHCONV-422).
//!
//! Python: `authentication/views/app/email.py:26-238`
//! (`SignInAuthEndpoint`, `SignUpAuthEndpoint`),
//! `authentication/views/space/email.py:25-191` (space twins),
//! `authentication/views/app/check.py:34-103` + `space/check.py:34-101`
//! (`EmailCheckEndpoint` twins), `views/app/signout.py:16-28` +
//! `views/space/signout.py:17-33` (sign-out twins),
//! `authentication/urls.py:51-57,118-120` (routes). Fixtures:
//! `rust-api/fixtures/auth_session/FX-AUTH-06.providers.json`
//! (email-provider part) + `FX-AUTH-07.handlers_email.json`.
//!
//! The branch decisions live in the pure kernel
//! (`pidash_services::auth_session::email`); this module owns the HTTP
//! shell: form/JSON extraction, the `CsrfViewMiddleware` gate, row
//! reads/writes, session issuance/deletion, cookie emission, and the
//! 302/200/400/429 bytes. Sibling D-16 handler issues (magic, password,
//! CSRF token) extend [`routes`] with their own routers; merges keep
//! both sides. Every other method on the eight paths proxies to Django
//! ([`owned`]), where the framework's own 405s live.
//!
//! Session-middleware interaction: the global `SessionLayer` loads the
//! presented session into extensions before these handlers run. The
//! handlers never touch that handle — every read is direct SQL and
//! every write sets its own headers — so the layer stays transparent
//! (no second save, no duplicate cookies); `Vary: Cookie` is set here,
//! matching what `CsrfViewMiddleware` + the session layer emit in
//! Django on these paths.
//!
//! Ported quirks (kept, listed in the PR; translated, not fixed):
//! * `Q-missing-email` / `Q-slashless-success` / `Q-space-success` /
//!   `Q-anon-signout` / `Q-provider-order`: see the kernel docs.
//! * `login()` refreshes `users.last_login` via the `user_logged_in`
//!   signal (`update_last_login`); replayed as an explicit UPDATE.
//! * `save()` regenerates `users.token` whenever `token_updated_at`
//!   is set (`user.py:172-174`); replayed in `save_user_data`.
//! * `is_active` defaults `True`, so fresh sign-ups never take the
//!   activation-mail branch; an inactive existing user does (published
//!   through the F-09 publisher — broker errors answer 500, like
//!   `.delay()` raising in Django).
//! * `process_workspace_project_invitations` runs on every success:
//!   the member bulk-writes + invite deletes are replayed; the
//!   per-workspace cache invalidation is a documented no-op (Rust
//!   serves no cached members rows, so there is nothing to invalidate
//!   — same reasoning as the D-32 `recent_visited_task` no-op), and
//!   each `track_event.delay` publishes through the F-09 publisher.
//! * The email-check throttle is the process-local DRF loop from the
//!   D-06 governor (shared cache would need a foundation change — a
//!   new issue, not a workaround); single-process deployments observe
//!   exactly Django-with-local-memory-cache behaviour.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use hmac::Hmac;
use sha2::{Digest, Sha256};

use pidash_auth::csrf as csrf_kernel;
use pidash_auth::session as session_kernel;
use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};
use pidash_services::auth_session::email as kernel;
use pidash_services::auth_session::guards as guards_kernel;
use pidash_services::auth_session::shapes as shapes_kernel;
use pidash_services::auth_session::tasks as tasks_kernel;

use crate::assistant::governor::throttle_check;
use crate::assistant::throttles::ThrottleSpec;
use crate::auth_session::render::render_csrf_failure;
use crate::edge;
use crate::license::handlers_auth_forms::client_ip;
use crate::state::AppState;

/// `AuthenticationThrottle`: 30/minute, scope `authentication`
/// (`authentication/rate_limit.py:17-19`).
const AUTHENTICATION_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "authentication",
    requests: 30,
    window_secs: 60,
};

/// `AUTHENTICATION_BACKENDS` is exactly `(ModelBackend,)`; `login()`
/// resolves this backend string (`settings/common.py:125`).
const MODEL_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";

/// Salt for `user.get_session_auth_hash()` (same value as the D-01
/// license wiring).
const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

/// Django 4.2 `PBKDF2PasswordHasher`: 600000 iterations, 22-char
/// alphanumeric salt (128 bits of entropy over 62 chars).
const PBKDF2_ITERATIONS: u32 = 600_000;
const PASSWORD_SALT_LEN: usize = 22;
/// `CSRF_COOKIE_AGE` default: 1 year; `CSRF_COOKIE_NAME` default.
const CSRF_AGE_SECS: i64 = 31_449_600;
const CSRF_COOKIE_NAME: &str = "csrftoken";

/// `owned` mirrors the D-32 intake helper: owned methods serve Rust,
/// every other method proxies so Django's own 405s live there.
pub fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(edge::proxy),
            "PUT" => router.put(edge::proxy),
            "PATCH" => router.patch(edge::proxy),
            "DELETE" => router.delete(edge::proxy),
            "OPTIONS" => router.options(edge::proxy),
            _ => router.get(edge::proxy),
        };
    }
    router
}

/// The eight owned email-session routes (cutover granularity: sibling
/// paths keep proxying through the fallback).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/auth/sign-in/",
            owned(
                axum::routing::post(signin_app),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/sign-up/",
            owned(
                axum::routing::post(signup_app),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/spaces/sign-in/",
            owned(
                axum::routing::post(signin_space),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/spaces/sign-up/",
            owned(
                axum::routing::post(signup_space),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/email-check/",
            owned(
                axum::routing::post(email_check_app),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/spaces/email-check/",
            owned(
                axum::routing::post(email_check_space),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/sign-out/",
            owned(
                axum::routing::post(signout_app),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/auth/spaces/sign-out/",
            owned(
                axum::routing::post(signout_space),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
}

// ---------------------------------------------------------------------------
// Request provenance (headers the views read)
// ---------------------------------------------------------------------------

/// What the handlers need from the HTTP envelope. `peer_ip` is the
/// socket peer (`REMOTE_ADDR` when no `X-Forwarded-For`); all other
/// fields come straight off the headers.
struct Provenance {
    user_agent: String,
    ip: Option<String>,
    csrf_cookie: Option<String>,
    header_token: Option<String>,
    session_cookie: Option<String>,
    content_type: Option<String>,
}

fn provenance(headers: &HeaderMap, peer_ip: Option<String>, cookie_name: &str) -> Provenance {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    Provenance {
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned(),
        ip: client_ip(forwarded, peer_ip.as_deref()),
        csrf_cookie: session_kernel::cookie_value(cookie_header, CSRF_COOKIE_NAME),
        header_token: headers
            .get("x-csrftoken")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        session_cookie: session_kernel::cookie_value(cookie_header, cookie_name),
        content_type: headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    }
}

/// `request.POST` as last-wins pairs, only for form content types
/// (any other content type leaves `POST` empty in Django).
fn form_pairs(content_type: Option<&str>, body: &[u8]) -> Vec<(String, String)> {
    let is_form = content_type.is_some_and(|ct| {
        ct.split(';').next().is_some_and(|mime| {
            mime.trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
    });
    if !is_form {
        return Vec::new();
    }
    serde_urlencoded::from_bytes::<Vec<(String, String)>>(body).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// CSRF gate (`CsrfViewMiddleware` for the plain form Views)
// ---------------------------------------------------------------------------

/// Why the CSRF gate rejected (Django `REASON_*`; the failure page
/// never renders the reason, so these are log labels only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsrfDenial {
    NoCookie,
    BadToken,
}

/// Mirror `CsrfViewMiddleware.process_view` for these endpoints: the
/// `csrftoken` cookie must be present and the request token (the
/// `X-CSRFToken` header, else the `csrfmiddlewaretoken` form field)
/// must unmask to it.
fn csrf_gate(prov: &Provenance, pairs: &[(String, String)]) -> Result<(), CsrfDenial> {
    let Some(secret) = prov.csrf_cookie.as_deref() else {
        return Err(CsrfDenial::NoCookie);
    };
    let token = prov.header_token.as_deref().map(str::to_owned).or_else(|| {
        pairs
            .iter()
            .rev()
            .find(|(k, _)| k == "csrfmiddlewaretoken")
            .map(|(_, v)| v.clone())
    });
    match token {
        Some(token) if csrf_kernel::tokens_match(&token, secret) => Ok(()),
        _ => Err(CsrfDenial::BadToken),
    }
}

/// The `csrf_failure` page: 200 + `text/html`, `root_url` =
/// `base_host(request)` with no surface flags.
fn csrf_failure_response(root_url: &str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::VARY, "Cookie"),
        ],
        render_csrf_failure(root_url),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Redirect / JSON responses
// ---------------------------------------------------------------------------

/// `HttpResponseRedirect`: 302 + `Location`, `Vary: Cookie`, and any
/// `Set-Cookie` values the branch issued.
fn redirect_response(location: String, set_cookies: Vec<String>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::VARY, "Cookie");
    for cookie in set_cookies {
        builder = builder.header(header::SET_COOKIE, cookie);
    }
    builder.body(Body::empty()).expect("static redirect")
}

/// DRF 200 JSON (`Json` renders compact `application/json`).
/// `Vary: Cookie`: the throttle's `request.user` touch marks the
/// session accessed on every email-check call, authenticated or not.
fn json_ok(body: serde_json::Value) -> Response {
    ([(header::VARY, "Cookie")], axum::Json(body)).into_response()
}

/// 400 over an ordered error-dict pair list (manual string: this
/// preserves `get_error_dict` order). `Vary: Cookie`, same as above.
fn json_400(pairs: &[(String, serde_json::Value)]) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::VARY, "Cookie"),
        ],
        shapes_kernel::error_dict_json(pairs),
    )
        .into_response()
}

/// Live 429 body: the `auth_exception_handler` rewrite (5900 dict).
/// `Vary: Cookie`, same as above.
fn throttle_429() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::VARY, "Cookie"),
        ],
        guards_kernel::throttle_denied_json(),
    )
        .into_response()
}

fn server_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"Something went wrong please try again later"}"#,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Host + config context (`host.py`, `instance_value.py`)
// ---------------------------------------------------------------------------

/// The URL bases + allowed hosts one request resolves, mirroring
/// `base_host` / `get_allowed_hosts` over the serve settings.
struct HostCtx {
    app_base: String,
    space_base: String,
    root_base: String,
    allowed_hosts: Vec<String>,
}

fn host_ctx(state: &AppState) -> HostCtx {
    let settings = state.settings();
    let urls = &settings.urls;
    let web_url = urls.web_url.clone();
    let app_base_url = urls.app_base_url.clone();
    let host_settings = shapes_kernel::HostSettings {
        web_url: web_url.as_deref(),
        app_base_url: app_base_url.as_deref(),
        admin_base_url: urls.admin_base_url.as_deref(),
        space_base_url: urls.space_base_url.as_deref(),
        admin_base_path: Some(urls.admin_base_path.as_str()),
        space_base_path: Some(urls.space_base_path.as_str()),
    };
    let allowed_hosts: Vec<String> = shapes_kernel::allowed_hosts_for(
        web_url.as_deref(),
        app_base_url.as_deref(),
        urls.admin_base_url.as_deref(),
        urls.space_base_url.as_deref(),
    )
    .into_iter()
    .map(str::to_owned)
    .collect();
    HostCtx {
        app_base: shapes_kernel::base_host(&host_settings, false, false, true),
        space_base: shapes_kernel::base_host(&host_settings, false, true, false),
        root_base: shapes_kernel::base_host(&host_settings, false, false, false),
        allowed_hosts,
    }
}

fn allowed_hosts_ref(hosts: &[String]) -> Vec<&str> {
    hosts.iter().map(String::as_str).collect()
}

/// `get_configuration_value` for one key
/// (`license/utils/instance_value.py:25-52`): the registry sources
/// these keys from `db`, so the newest non-deleted
/// `instance_configurations` row wins; absent that, the caller
/// default (`os.environ.get(key, default)` — env first, then the
/// literal); encrypted rows are a 500 like Django's decrypt failure.
async fn config_value(
    pool: &sqlx::PgPool,
    key: &str,
    literal_default: Option<String>,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String, bool)> = sqlx::query_as(
        r#"SELECT "value", "is_encrypted" FROM "instance_configurations"
           WHERE "deleted_at" IS NULL AND "key" = $1
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((_, true)) => Err(sqlx::Error::RowNotFound),
        Some((value, false)) => Ok(Some(value)),
        None => Ok(std::env::var(key).ok().or(literal_default)),
    }
}

// ---------------------------------------------------------------------------
// Row snapshots
// ---------------------------------------------------------------------------

/// `Instance.objects.first()` + the `is_setup_done` gate.
async fn instance_is_setup(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    let row: Option<(bool,)> =
        sqlx::query_as(r#"SELECT "is_setup_done" FROM "instances" ORDER BY "id" ASC LIMIT 1"#)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some_and(|(done,)| done))
}

/// The `users` columns these endpoints read.
#[derive(Debug, Clone)]
struct UserRow {
    id: uuid::Uuid,
    password: String,
    is_active: bool,
    is_password_autoset: bool,
}

async fn user_by_email(pool: &sqlx::PgPool, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<(uuid::Uuid, String, bool, bool)> = sqlx::query_as(
        r#"SELECT "id", "password", "is_active", "is_password_autoset"
           FROM "users" WHERE "email" = $1 LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(id, password, is_active, is_password_autoset)| UserRow {
            id,
            password,
            is_active,
            is_password_autoset,
        }),
    )
}

async fn user_by_pk(pool: &sqlx::PgPool, id: uuid::Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<(uuid::Uuid, String, bool, bool)> = sqlx::query_as(
        r#"SELECT "id", "password", "is_active", "is_password_autoset"
           FROM "users" WHERE "id" = $1 LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(id, password, is_active, is_password_autoset)| UserRow {
            id,
            password,
            is_active,
            is_password_autoset,
        }),
    )
}

// ---------------------------------------------------------------------------
// Password + session-auth primitives (F-05, read-only)
// ---------------------------------------------------------------------------

/// `user.check_password(candidate)` (`hashers.py`): `False` on
/// mismatch *and* on unparsable hashes.
fn check_password(candidate: &str, encoded: &str) -> bool {
    pidash_auth::password::verify_password(candidate, encoded).unwrap_or(false)
}

/// `make_password(password)`: PBKDF2, fresh 22-alphanumeric salt, the
/// pinned Django 4.2 iteration count.
fn encode_password(password: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut salt = String::with_capacity(PASSWORD_SALT_LEN);
    while salt.len() < PASSWORD_SALT_LEN {
        for byte in uuid::Uuid::new_v4().as_bytes() {
            if salt.len() == PASSWORD_SALT_LEN {
                break;
            }
            salt.push(ALPHABET[*byte as usize % ALPHABET.len()] as char);
        }
    }
    pidash_auth::password::hash_password(password, &salt, PBKDF2_ITERATIONS)
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()` (same construction as the D-01 wiring).
fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    use hmac::Mac as _;
    mac.update(password_field.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `zxcvbn(password)["score"]` (same estimator, 0-4 scale; an
/// unscorable input is 0, like the empty-password score).
fn password_score(password: &str) -> u8 {
    zxcvbn::zxcvbn(password, &[]).score() as u8
}

/// `User.get_display_name(email)` for the create path
/// (`user.py:190-197`): the local part of a two-part address.
fn display_name_for(email: &str) -> String {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() == 2 {
        parts[0].to_owned()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// User writes (`adapter/base.py:220-360`)
// ---------------------------------------------------------------------------

/// `User(...)` + first `save()` for a sign-up
/// (`base.py:296-328`): identity + names + the `set_password` hash;
/// every other column takes its model default (`save()` then fills
/// `display_name` from the email local part and leaves `token` empty
/// — `token_updated_at` is still null).
async fn create_user(
    pool: &sqlx::PgPool,
    email: &str,
    password: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let username = uuid::Uuid::new_v4().simple().to_string();
    sqlx::query(
        r#"INSERT INTO "users"
           ("id", "password", "username", "email", "first_name", "last_name",
            "avatar", "date_joined", "created_at", "updated_at", "last_location",
            "created_location", "is_superuser", "is_managed", "is_password_expired",
            "is_active", "is_staff", "is_email_verified", "is_password_autoset",
            "is_password_reset_required", "token", "last_active", "last_login_time",
            "last_logout_time", "last_login_ip", "last_logout_ip", "last_login_medium",
            "last_login_uagent", "token_updated_at", "is_bot", "bot_type",
            "user_timezone", "is_email_valid", "masked_at", "display_name",
            "mobile_number", "cover_image")
           VALUES ($1,$2,$3,$4,'','', '', $5, $5, $5, '', '',
            false,false,false, true,false,false,false,false, '', NULL, NULL,
            NULL, '', '', 'email', '', NULL, false, NULL,
            'UTC', false, NULL, $6,
            NULL, NULL)"#,
    )
    .bind(id)
    .bind(encode_password(password))
    .bind(&username)
    .bind(email)
    .bind(now)
    .bind(display_name_for(email))
    .execute(pool)
    .await?;
    Ok(id)
}

/// `save_user_data` (`base.py:220-234`): the login stamps plus the
/// activation branch. Returns whether the activation mail must be
/// published (`not user.is_active` before the write).
async fn save_user_data(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_agent: Option<String>,
    ip: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
    was_active: bool,
) -> Result<bool, sqlx::Error> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sqlx::query(
        r#"UPDATE "users" SET "last_login_medium" = 'email', "last_active" = $2,
           "last_login_time" = $2, "last_login_ip" = $3, "last_login_uagent" = $4,
           "token_updated_at" = $2, "token" = $5, "is_active" = true
           WHERE "id" = $1"#,
    )
    .bind(user_id)
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(!was_active)
}

/// The `user_logged_in` signal (`update_last_login`): `last_login =
/// now` on every login.
async fn stamp_last_login(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE "users" SET "last_login" = $2 WHERE "id" = $1"#)
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// `Profile.objects.get_or_create(user=user)` for
/// `get_redirection_path` (`redirection_path.py:11`): the insert
/// carries every model default.
async fn get_or_create_profile(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(bool, Option<uuid::Uuid>), sqlx::Error> {
    let row: Option<(bool, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT "is_onboarded", "last_workspace_id" FROM "profiles" WHERE "user_id" = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some(found) = row {
        return Ok(found);
    }
    let color_suffix: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(6)
        .collect();
    sqlx::query(
        r#"INSERT INTO "profiles"
           ("id", "user_id", "created_at", "updated_at", "theme", "is_app_rail_docked",
            "is_tour_completed", "onboarding_step", "use_case", "role", "is_onboarded",
            "last_workspace_id", "billing_address_country", "billing_address",
            "has_billing_address", "company_name", "notification_view_mode",
            "is_smooth_cursor_enabled", "is_mobile_onboarded", "mobile_onboarding_step",
            "mobile_timezone_auto_set", "language", "start_of_the_week", "goals",
            "background_color", "is_navigation_tour_completed",
            "has_marketing_email_consent", "is_subscribed_to_changelog",
            "product_tour", "settings")
           VALUES ($1,$2,$3,$3, '{}', true,
            false, '{"profile_complete": false, "workspace_create": false, "workspace_invite": false, "workspace_join": false}'::jsonb, NULL, NULL, false,
            NULL, 'INDIA', NULL,
            false, '', 'full',
            false, false, '{"profile_complete": false, "workspace_create": false, "workspace_join": false}'::jsonb,
            false, 'en', 0, '{}',
            $4, false,
            false, false,
            '{"work_items": false, "cycles": false, "modules": false, "intake": false, "pages": false}'::jsonb, '{}') "#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(user_id)
    .bind(now)
    .bind(format!("#{color_suffix}"))
    .execute(pool)
    .await?;
    Ok((false, None))
}

/// `get_redirection_path` (`redirection_path.py:8-46`): onboarding
/// first, then the active last workspace, then the earliest active
/// membership, then invites, else create-workspace.
async fn redirection_path(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_email: &str,
) -> Result<String, sqlx::Error> {
    let (is_onboarded, last_workspace_id) =
        get_or_create_profile(pool, user_id, chrono::Utc::now()).await?;
    if !is_onboarded {
        return Ok("onboarding".to_owned());
    }
    if let Some(last_id) = last_workspace_id {
        let slug: Option<(String,)> = sqlx::query_as(
            r#"SELECT w."slug" FROM "workspaces" w
               JOIN "workspace_members" m ON m."workspace_id" = w."id"
               WHERE w."id" = $1 AND m."member_id" = $2 AND m."is_active" = true
                 AND w."deleted_at" IS NULL LIMIT 1"#,
        )
        .bind(last_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
        if let Some((slug,)) = slug {
            return Ok(slug);
        }
    }
    let fallback: Option<(String,)> = sqlx::query_as(
        r#"SELECT w."slug" FROM "workspaces" w
           JOIN "workspace_members" m ON m."workspace_id" = w."id"
           WHERE m."member_id" = $1 AND m."is_active" = true AND w."deleted_at" IS NULL
           ORDER BY w."created_at" ASC LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some((slug,)) = fallback {
        return Ok(slug);
    }
    let invites: (i64,) =
        sqlx::query_as(r#"SELECT COUNT(*) FROM "workspace_member_invites" WHERE "email" = $1"#)
            .bind(user_email)
            .fetch_one(pool)
            .await?;
    if invites.0 > 0 {
        return Ok("invitations".to_owned());
    }
    Ok("create-workspace".to_owned())
}

// ---------------------------------------------------------------------------
// `post_user_auth_workflow` (`workspace_project_join.py`)
// ---------------------------------------------------------------------------

/// One accepted workspace invite: `(workspace_id, role)`.
async fn accepted_workspace_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<(uuid::Uuid, i32, String)>, sqlx::Error> {
    let rows: Vec<(uuid::Uuid, i32, String)> = sqlx::query_as(
        r#"SELECT i."workspace_id", i."role", w."slug"
           FROM "workspace_member_invites" i
           JOIN "workspaces" w ON w."id" = i."workspace_id"
           WHERE i."email" = $1 AND i."accepted" = true"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One accepted project invite: `(workspace_id, role, created_by)`.
async fn accepted_project_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<(uuid::Uuid, i32, Option<uuid::Uuid>)>, sqlx::Error> {
    let rows: Vec<(uuid::Uuid, i32, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT "workspace_id", "role", "created_by_id"
           FROM "project_member_invites" WHERE "email" = $1 AND "accepted" = true"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Map a project-invite role onto a member role (`... in [5, 15]`
/// else 15).
fn invite_member_role(role: i32) -> i32 {
    if role == 5 || role == 15 {
        role
    } else {
        15
    }
}

/// `view_props`/`default_props` default (`workspace.py:22-58`).
fn default_props_json() -> serde_json::Value {
    serde_json::json!({
        "filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null},
        "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""},
        "display_properties": {"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true},
    })
}

/// `issue_props` default (`workspace.py:110-111`).
fn issue_props_json() -> serde_json::Value {
    serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
}

/// `preferences` default (`project.py:get_default_preferences`).
fn default_preferences_json() -> serde_json::Value {
    serde_json::json!({"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}})
}

/// Publish one Celery message through the F-09 AMQP publisher.
/// Broker errors propagate (500), like `.delay()` raising in Django
/// (same pattern as the D-02 asset-metadata publish).
async fn publish_message(message: &pidash_jobs::CeleryTaskMessage) -> Result<(), ()> {
    let config = pidash_jobs::AmqpConfig::from_env().map_err(|_| ())?;
    let publisher = pidash_jobs::Publisher::connect(&config)
        .await
        .map_err(|_| ())?;
    let result = publisher.publish(message).await;
    let _ = publisher.close().await;
    result.map_err(|_| ())
}

/// `process_workspace_project_invitations`
/// (`workspace_project_join.py:22-91`): member bulk-writes with
/// `ignore_conflicts` (every concrete column, like `bulk_create`;
/// `bulk_create` skips `save()`, so `created_by` stays null and the
/// project arm keeps `project_id` null — the ported IntegrityError),
/// one `track_event.delay` per workspace invite, then the invite
/// deletes. Cache invalidation is a documented no-op (see the module
/// docs). Each statement is its own autocommit write — no
/// `transaction.atomic` anywhere — so a later failure keeps the
/// earlier rows, exactly like Django.
async fn post_user_auth_workflow(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_email: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), Response> {
    let default_props = default_props_json();
    let issue_props = issue_props_json();
    let workspace_invites = accepted_workspace_invites(pool, user_email)
        .await
        .map_err(|_| server_error())?;
    for (workspace_id, role, _slug) in &workspace_invites {
        sqlx::query(
            r#"INSERT INTO "workspace_members"
               ("created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                "id", "workspace_id", "member_id", "role", "company_role",
                "view_props", "default_props", "issue_props", "is_active",
                "getting_started_checklist", "tips", "explored_features")
               VALUES ($1,$1,NULL,NULL,NULL, $2,$3,$4,$5,NULL, $6,$6,$7,true, '{}','{}','{}')
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(uuid::Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(role)
        .bind(sqlx::types::Json(default_props.clone()))
        .bind(sqlx::types::Json(issue_props.clone()))
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
    }
    for (workspace_id, role, slug) in &workspace_invites {
        let message = pidash_jobs::CeleryTaskMessage::new(
            pidash_jobs::tasks_webhooks::sinks::TRACK_EVENT_TASK,
            vec![
                serde_json::Value::String(user_id.to_string()),
                serde_json::Value::String("user_joined_workspace".to_owned()),
                serde_json::Value::String(slug.clone()),
                serde_json::json!({
                    "user_id": user_id.to_string(),
                    "workspace_id": workspace_id.to_string(),
                    "workspace_slug": slug,
                    "role": role,
                    "joined_at": now.to_rfc3339(),
                }),
            ],
            serde_json::Map::new(),
        );
        publish_message(&message)
            .await
            .map_err(|_| server_error())?;
    }
    let project_invites = accepted_project_invites(pool, user_email)
        .await
        .map_err(|_| server_error())?;
    let preferences = default_preferences_json();
    for (workspace_id, role, created_by) in &project_invites {
        let member_role = invite_member_role(*role);
        sqlx::query(
            r#"INSERT INTO "workspace_members"
               ("created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                "id", "workspace_id", "member_id", "role", "company_role",
                "view_props", "default_props", "issue_props", "is_active",
                "getting_started_checklist", "tips", "explored_features")
               VALUES ($1,$1,$2,NULL,NULL, $3,$4,$5,$6,NULL, $7,$7,$8,true, '{}','{}','{}')
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(created_by)
        .bind(uuid::Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(member_role)
        .bind(sqlx::types::Json(default_props.clone()))
        .bind(sqlx::types::Json(issue_props.clone()))
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
        // Ported bug (`workspace_project_join.py:76-87`): the
        // `ProjectMember` constructor never sets `project`, so the
        // row carries `project_id` null and Postgres raises
        // `IntegrityError` despite `ignore_conflicts` — the endpoint
        // 500s with the workspace rows already committed.
        sqlx::query(
            r#"INSERT INTO "project_members"
               ("created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
                "id", "project_id", "workspace_id", "member_id", "comment", "role",
                "view_props", "default_props", "preferences", "sort_order", "is_active")
               VALUES ($1,$1,$2,NULL,NULL, $3,NULL,$4,$5,NULL,$6, $7,$7,$8,65535,true)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(created_by)
        .bind(uuid::Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(member_role)
        .bind(sqlx::types::Json(default_props.clone()))
        .bind(sqlx::types::Json(preferences.clone()))
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
    }
    sqlx::query(
        r#"DELETE FROM "workspace_member_invites" WHERE "email" = $1 AND "accepted" = true"#,
    )
    .bind(user_email)
    .execute(pool)
    .await
    .map_err(|_| server_error())?;
    sqlx::query(r#"DELETE FROM "project_member_invites" WHERE "email" = $1 AND "accepted" = true"#)
        .bind(user_email)
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Session issuance (`django.contrib.auth.login` + `user_login`)
// ---------------------------------------------------------------------------

/// Cookie attributes for this deployment (domain/secure from
/// settings; path `/`, `SameSite=Lax`, session cookie HttpOnly —
/// the CSRF cookie is HttpOnly too per project settings).
struct CookieCtx {
    domain: Option<String>,
    secure: bool,
}

fn cookie_ctx(state: &AppState) -> CookieCtx {
    let session = &state.settings().session;
    CookieCtx {
        domain: session.cookie_domain.clone(),
        secure: session.cookie_secure,
    }
}

fn session_set_cookie(
    key: &str,
    ctx: &CookieCtx,
    cookie_name: &str,
    age_secs: i64,
    now_unix: i64,
) -> String {
    session_kernel::render_set_cookie(&session_kernel::SetCookie {
        name: cookie_name.to_owned(),
        value: key.to_owned(),
        // Plain `login()` never calls `set_expiry`, and
        // `SESSION_EXPIRE_AT_BROWSER_CLOSE` defaults false, so the
        // cookie carries the age (`get_expiry_age()`), unlike the
        // browser-close sessions the middleware docs describe.
        expires: Some(session_kernel::http_date(now_unix + age_secs)),
        max_age: Some(age_secs),
        domain: ctx.domain.clone(),
        path: "/".to_owned(),
        secure: ctx.secure,
        httponly: true,
        samesite: "Lax".to_owned(),
    })
}

/// `rotate_token`: fresh 32-char CSRF secret, set with the cookie age
/// (`login()` rotates the CSRF secret on every login).
fn csrf_rotate_cookie(secret: &str, ctx: &CookieCtx) -> String {
    session_kernel::render_set_cookie(&session_kernel::SetCookie {
        name: CSRF_COOKIE_NAME.to_owned(),
        value: secret.to_owned(),
        expires: Some(session_kernel::http_date(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
                + CSRF_AGE_SECS,
        )),
        max_age: Some(CSRF_AGE_SECS),
        domain: ctx.domain.clone(),
        path: "/".to_owned(),
        secure: ctx.secure,
        httponly: true,
        samesite: "Lax".to_owned(),
    })
}

/// `login()` + `user_login`: cycle the session key (delete the
/// presented row when any), insert the new row
/// (`_auth_user_id`/`_auth_user_backend`/`_auth_user_hash` +
/// `device_info`, mirrored `user_id`/`device_info` columns,
/// `expire_date = now + SESSION_COOKIE_AGE`), and return the two
/// `Set-Cookie` values (session + rotated CSRF secret).
#[allow(clippy::too_many_arguments)]
async fn issue_session(
    pool: &sqlx::PgPool,
    presented_key: Option<&str>,
    user_id: uuid::Uuid,
    password_field: &str,
    device_info: serde_json::Value,
    secret_key: &[u8],
    cookie_name: &str,
    ctx: &CookieCtx,
    age_secs: i64,
    now_unix: i64,
) -> Result<Vec<String>, sqlx::Error> {
    if let Some(old) = presented_key {
        if session_kernel::is_plausible_session_key(old) {
            sqlx::query(r#"DELETE FROM "sessions" WHERE "session_key" = $1"#)
                .bind(old)
                .execute(pool)
                .await?;
        }
    }
    let mut payload = serde_json::Map::new();
    payload.insert(
        "_auth_user_id".to_owned(),
        serde_json::Value::String(user_id.to_string()),
    );
    payload.insert(
        "_auth_user_backend".to_owned(),
        serde_json::Value::String(MODEL_BACKEND.to_owned()),
    );
    payload.insert(
        "_auth_user_hash".to_owned(),
        serde_json::Value::String(session_auth_hash(password_field, secret_key)),
    );
    payload.insert("device_info".to_owned(), device_info.clone());
    let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
    let session_data = signer
        .sign_object(&payload, now_unix as u64)
        .map_err(|_| sqlx::Error::RowNotFound)?;
    let expire_unix = now_unix + age_secs;
    let key = loop {
        let key = session_kernel::generate_session_key();
        let inserted = sqlx::query(
            r#"INSERT INTO "sessions" ("session_key", "session_data", "expire_date", "user_id", "device_info")
               VALUES ($1, $2, to_timestamp($3::DOUBLE PRECISION), $4, $5)
               ON CONFLICT ("session_key") DO NOTHING"#,
        )
        .bind(&key)
        .bind(&session_data)
        .bind(expire_unix as f64)
        .bind(user_id.to_string())
        .bind(sqlx::types::Json(device_info.clone()))
        .execute(pool)
        .await?;
        if inserted.rows_affected() > 0 {
            break key;
        }
    };
    Ok(vec![
        session_set_cookie(&key, ctx, cookie_name, age_secs, now_unix),
        csrf_rotate_cookie(&csrf_kernel::new_secret(), ctx),
    ])
}

/// `user_activation_email.delay(base_host, user.id)` on the inactive
/// branch (`base.py:229-230`), through the F-09 publisher.
async fn publish_activation(current_site: &str, user_id: uuid::Uuid) -> Result<(), Response> {
    let message = pidash_jobs::CeleryTaskMessage::new(
        tasks_kernel::USER_ACTIVATION_EMAIL_TASK,
        vec![
            serde_json::Value::String(current_site.to_owned()),
            serde_json::Value::String(user_id.to_string()),
        ],
        serde_json::Map::new(),
    );
    publish_message(&message).await.map_err(|_| server_error())
}

// ---------------------------------------------------------------------------
// Sign-in / sign-up core (`views/app|space/email.py`)
// ---------------------------------------------------------------------------

/// Shared per-request state for the four email form endpoints.
struct EmailRequest {
    pairs: Vec<(String, String)>,
    prov: Provenance,
    hosts: HostCtx,
    next_path: Option<String>,
    base: String,
    space: bool,
    form: kernel::EmailForm,
}

fn email_request(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
    peer_ip: Option<String>,
    form: kernel::EmailForm,
    space: bool,
) -> EmailRequest {
    let hosts = host_ctx(state);
    let cookie_name = state.settings().session.cookie_name.clone();
    let prov = provenance(headers, peer_ip, &cookie_name);
    let pairs = form_pairs(prov.content_type.as_deref(), body);
    let next_path = pairs
        .iter()
        .rev()
        .find(|(k, _)| k == "next_path")
        .map(|(_, v)| v.clone());
    let base = if space {
        hosts.space_base.clone()
    } else {
        hosts.app_base.clone()
    };
    EmailRequest {
        pairs,
        prov,
        hosts,
        next_path,
        base,
        space,
        form,
    }
}

/// Resolve the `EmailProvider` outcome against the database: the
/// `ENABLE_EMAIL_PASSWORD == "0"` init gate first, then the
/// credential checks (`email.py:26-96`, `base.py:90-120`). Called
/// only after the view's own pre-checks pass, preserving
/// `Q-provider-order`.
async fn provider_outcome(
    pool: &sqlx::PgPool,
    form: kernel::EmailForm,
    normalized: &str,
    raw_password: &str,
    existing: Option<&UserRow>,
) -> Result<kernel::ProviderOutcome, Response> {
    let enable_email_password = config_value(pool, "ENABLE_EMAIL_PASSWORD", None)
        .await
        .map_err(|_| server_error())?;
    if enable_email_password.as_deref() == Some("0") {
        return Ok(kernel::ProviderOutcome::Disabled);
    }
    match form {
        kernel::EmailForm::SignIn => {
            let user = existing.expect("sign-in pre-check passed");
            if !check_password(raw_password, &user.password) {
                return Ok(kernel::ProviderOutcome::BadPassword {
                    email: normalized.to_owned(),
                });
            }
            Ok(kernel::ProviderOutcome::Ok)
        }
        kernel::EmailForm::SignUp => {
            let enable_signup = config_value(pool, "ENABLE_SIGNUP", Some("1".to_owned()))
                .await
                .map_err(|_| server_error())?;
            if enable_signup.as_deref() == Some("0") {
                let invited: Option<(uuid::Uuid,)> = sqlx::query_as(
                    r#"SELECT "id" FROM "workspace_member_invites" WHERE "email" = $1 LIMIT 1"#,
                )
                .bind(normalized)
                .fetch_optional(pool)
                .await
                .map_err(|_| server_error())?;
                if invited.is_none() {
                    return Ok(kernel::ProviderOutcome::SignupDisabled {
                        email: normalized.to_owned(),
                    });
                }
            }
            if shapes_kernel::is_password_too_weak(password_score(raw_password)) {
                return Ok(kernel::ProviderOutcome::WeakPassword {
                    email: normalized.to_owned(),
                });
            }
            Ok(kernel::ProviderOutcome::Ok)
        }
    }
}

/// Evaluate one attempt: the instance gate + form reads here, the
/// branch order in the kernel.
async fn evaluate_attempt(
    pool: &sqlx::PgPool,
    req: &EmailRequest,
) -> Result<(kernel::EmailDecision, Option<UserRow>, String, String), Response> {
    let instance_ok = instance_is_setup(pool).await.map_err(|_| server_error())?;
    let email_field = kernel::form_field(&req.pairs, "email");
    let password_field = kernel::form_field(&req.pairs, "password");
    if !instance_ok {
        return Ok((
            kernel::EmailDecision::NotConfigured,
            None,
            String::new(),
            String::new(),
        ));
    }
    if kernel::field_is_missing(email_field) || kernel::field_is_missing(password_field) {
        let (code, message) = match req.form {
            kernel::EmailForm::SignIn => (
                kernel::REQUIRED_EMAIL_PASSWORD_SIGN_IN,
                "REQUIRED_EMAIL_PASSWORD_SIGN_IN",
            ),
            kernel::EmailForm::SignUp => (
                kernel::REQUIRED_EMAIL_PASSWORD_SIGN_UP,
                "REQUIRED_EMAIL_PASSWORD_SIGN_UP",
            ),
        };
        return Ok((
            kernel::EmailDecision::Required {
                code,
                message,
                email: kernel::field_payload_str(email_field),
            },
            None,
            String::new(),
            String::new(),
        ));
    }
    let raw_email = match email_field {
        kernel::FormField::Present(value) => value,
        kernel::FormField::Missing => "",
    };
    let raw_password = match password_field {
        kernel::FormField::Present(value) => value,
        kernel::FormField::Missing => "",
    };
    let normalized = kernel::normalize_email(raw_email);
    let existing = user_by_email(pool, &normalized)
        .await
        .map_err(|_| server_error())?;
    let user_exists = existing.is_some();
    let user_row = existing.clone();
    let provider = if (req.form == kernel::EmailForm::SignIn) != user_exists {
        // The view's own existence pre-check fails: the provider is
        // never constructed (`Q-provider-order`).
        kernel::ProviderOutcome::Ok
    } else {
        provider_outcome(pool, req.form, &normalized, raw_password, existing.as_ref()).await?
    };
    let attempt = kernel::EmailAttempt {
        form: req.form,
        instance_ok: true,
        email: email_field,
        password: password_field,
        normalized_email: Some(normalized.as_str()),
        user_exists,
        provider,
        next_path: req.next_path.as_deref(),
    };
    let decision = kernel::evaluate_email_attempt(&attempt);
    Ok((decision, user_row, normalized, raw_password.to_owned()))
}

/// Run the authenticated tail shared by sign-in and sign-up:
/// `save_user_data` (+ activation branch), `last_login` stamp,
/// `post_user_auth_workflow`, `user_login` session issuance, and the
/// success redirect.
#[allow(clippy::too_many_arguments)]
async fn run_authenticated(
    pool: &sqlx::PgPool,
    state: &AppState,
    req: &EmailRequest,
    user_id: uuid::Uuid,
    password_field: &str,
    user_email: &str,
    was_active: bool,
    current_site: &str,
) -> Result<Response, Response> {
    let now = chrono::Utc::now();
    let user_agent = if req.prov.user_agent.is_empty() {
        None
    } else {
        Some(req.prov.user_agent.clone())
    };
    let need_activation = save_user_data(
        pool,
        user_id,
        user_agent,
        req.prov.ip.clone(),
        now,
        was_active,
    )
    .await
    .map_err(|_| server_error())?;
    stamp_last_login(pool, user_id, now)
        .await
        .map_err(|_| server_error())?;
    if need_activation {
        publish_activation(current_site, user_id).await?;
    }
    // Only the app endpoints pass `callback=post_user_auth_workflow`
    // (`app/email.py:86,208`); the space twins construct the provider
    // without a callback (`space/email.py:80,156`) so the workflow
    // never runs there.
    if !req.space {
        post_user_auth_workflow(pool, user_id, user_email, now).await?;
    }
    let device_info = serde_json::json!({
        "user_agent": req.prov.user_agent,
        "ip_address": req.prov.ip.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
        "domain": current_site,
    });
    let cookies = issue_session(
        pool,
        req.prov.session_cookie.as_deref(),
        user_id,
        password_field,
        device_info,
        state.settings().secret_key.as_bytes(),
        &state.settings().session.cookie_name,
        &cookie_ctx(state),
        state.settings().session.cookie_age_secs,
        now.timestamp(),
    )
    .await
    .map_err(|_| server_error())?;
    let hosts = allowed_hosts_ref(&req.hosts.allowed_hosts);
    let location = if req.space {
        kernel::space_success_location(&req.base, req.next_path.as_deref(), &hosts)
    } else {
        let path = redirection_path(pool, user_id, user_email)
            .await
            .map_err(|_| server_error())?;
        kernel::app_success_location(&req.base, req.next_path.as_deref(), &path, &hosts)
    };
    Ok(redirect_response(location, cookies))
}

/// Serve one email form endpoint: CSRF gate, kernel decision, then
/// either the error redirect or the authenticated tail.
async fn serve_email_form(state: &AppState, req: &EmailRequest, current_site: &str) -> Response {
    if csrf_gate(&req.prov, &req.pairs).is_err() {
        return csrf_failure_response(&req.hosts.root_base);
    }
    let Some(pool) = state.pools().map(|pools| pools.primary()) else {
        return server_error();
    };
    let (decision, user_row, normalized, raw_password) = match evaluate_attempt(pool, req).await {
        Ok(done) => done,
        Err(response) => return response,
    };
    if decision != kernel::EmailDecision::Authenticated {
        let hosts = allowed_hosts_ref(&req.hosts.allowed_hosts);
        let location = kernel::email_decision_location(
            &decision,
            req.form,
            req.space,
            &req.base,
            req.next_path.as_deref(),
            "",
            &hosts,
        );
        return redirect_response(location, Vec::new());
    }
    let authenticated = match req.form {
        kernel::EmailForm::SignIn => {
            let user = user_row.expect("sign-in authenticated has a row");
            run_authenticated(
                pool,
                state,
                req,
                user.id,
                &user.password,
                &normalized,
                user.is_active,
                current_site,
            )
            .await
        }
        kernel::EmailForm::SignUp => {
            let now = chrono::Utc::now();
            let (user_id, password_field) =
                match create_user(pool, &normalized, &raw_password, now).await {
                    Ok(id) => {
                        let row = match user_by_email(pool, &normalized).await {
                            Ok(row) => row,
                            Err(_) => return server_error(),
                        };
                        match row {
                            Some(row) => (id, row.password),
                            None => return server_error(),
                        }
                    }
                    Err(_) => return server_error(),
                };
            run_authenticated(
                pool,
                state,
                req,
                user_id,
                &password_field,
                &normalized,
                true,
                current_site,
            )
            .await
        }
    };
    match authenticated {
        Ok(response) => response,
        Err(response) => response,
    }
}

async fn signin_app(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let email_req = email_request(
        &state,
        &parts.headers,
        &body,
        peer,
        kernel::EmailForm::SignIn,
        false,
    );
    let current_site = email_req.base.clone();
    serve_email_form(&state, &email_req, &current_site).await
}

async fn signup_app(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let email_req = email_request(
        &state,
        &parts.headers,
        &body,
        peer,
        kernel::EmailForm::SignUp,
        false,
    );
    let current_site = email_req.base.clone();
    serve_email_form(&state, &email_req, &current_site).await
}

async fn signin_space(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let email_req = email_request(
        &state,
        &parts.headers,
        &body,
        peer,
        kernel::EmailForm::SignIn,
        true,
    );
    let current_site = email_req.base.clone();
    serve_email_form(&state, &email_req, &current_site).await
}

async fn signup_space(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let email_req = email_request(
        &state,
        &parts.headers,
        &body,
        peer,
        kernel::EmailForm::SignUp,
        true,
    );
    let current_site = email_req.base.clone();
    serve_email_form(&state, &email_req, &current_site).await
}

// ---------------------------------------------------------------------------
// Email-check (`views/app|space/check.py`)
// ---------------------------------------------------------------------------

/// Coerce a JSON `email` member the way the view does:
/// `request.data.get("email", False)` is falsy for missing, `False`,
/// `""`, `0`, `null`, `[]`, `{}`; anything truthy becomes
/// `str(value)`.
fn check_email_input(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(false) => None,
        serde_json::Value::Bool(true) => Some("True".to_owned()),
        serde_json::Value::Number(n) => {
            if n.as_i64() == Some(0) || n.as_f64() == Some(0.0) {
                None
            } else {
                Some(n.to_string())
            }
        }
        serde_json::Value::String(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
    }
}

/// Resolve `request.user` from the session cookie
/// (`django.contrib.auth.get_user` + DRF `SessionAuthentication`):
/// the row must exist and be unexpired, the payload must decode and
/// carry a UUID `_auth_user_id` for the `ModelBackend` with a
/// matching `_auth_user_hash`, and the user row must exist and be
/// active. Anything else is anonymous (same predicate as the D-01
/// `resolve_actor`).
async fn resolve_session_user(
    pool: &sqlx::PgPool,
    session_cookie: Option<&str>,
    secret_key: &[u8],
    now_unix: i64,
) -> Option<uuid::Uuid> {
    let key = session_cookie?;
    if !session_kernel::is_plausible_session_key(key) {
        return None;
    }
    let row: Option<(String, i64)> = sqlx::query_as(
        r#"SELECT "session_data", EXTRACT(EPOCH FROM "expire_date")::BIGINT FROM "sessions" WHERE "session_key" = $1"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let (session_data, expire_unix) = row?;
    if session_kernel::is_expired(expire_unix, now_unix) {
        return None;
    }
    let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
    let data: serde_json::Value = signer.unsign_object(&session_data).ok()?;
    let user_id = data
        .get("_auth_user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if data.get("_auth_user_backend").and_then(|v| v.as_str()) != Some(MODEL_BACKEND) {
        return None;
    }
    let session_hash = data
        .get("_auth_user_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let user_id: uuid::Uuid = user_id.parse().ok()?;
    let user = user_by_pk(pool, user_id).await.ok().flatten()?;
    if !user.is_active || session_hash.is_empty() {
        return None;
    }
    if session_auth_hash(&user.password, secret_key) != session_hash {
        return None;
    }
    Some(user_id)
}

/// One `AuthenticationThrottle` pass over the shared redis history
/// (`rate_limit.py:17-19` + `throttling.py:109-132` via the guards
/// kernel): trim entries older than the window, allow iff fewer than
/// 30 remain, recording `now` on allow with a 60s TTL (DRF caches
/// with `timeout=duration`). Redis errors fall back to the
/// process-local governor.
async fn check_email_throttle(state: &AppState, ident: &str) -> bool {
    let key = guards_kernel::throttle_cache_key(AUTHENTICATION_THROTTLE.scope, ident);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if let Some(redis) = state.redis() {
        let history: Vec<f64> = redis
            .get_string(&key)
            .await
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let decision = guards_kernel::allow_request(
            &history,
            AUTHENTICATION_THROTTLE.requests,
            AUTHENTICATION_THROTTLE.window_secs,
            now,
        );
        if decision.allowed {
            let raw = serde_json::to_string(&decision.history).unwrap_or_default();
            if redis
                .set_ex(&key, &raw, AUTHENTICATION_THROTTLE.window_secs)
                .await
                .is_ok()
            {
                return true;
            }
        } else {
            return false;
        }
    }
    throttle_check(AUTHENTICATION_THROTTLE, ident)
}

async fn serve_email_check(state: &AppState, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let Some(pool) = state.pools().map(|pools| pools.primary()) else {
        return server_error();
    };
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let forwarded = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let ip = client_ip(forwarded, peer.as_deref()).unwrap_or_default();
    // Throttle before the handler (`APIView.initial`: auth, then
    // permissions, then throttles); authenticated callers bypass the
    // anon scope. Histories live in the shared redis db under the DRF
    // key (`throttle_<scope>_<ident>`, JSON floats — Django stores a
    // pickle under its own prefixed key, so the two never collide;
    // the suite's flushdb resets both), with the process-local
    // governor as fallback when redis is unavailable.
    let cookie_name = state.settings().session.cookie_name.clone();
    let cookie_header = parts
        .headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let session_cookie = session_kernel::cookie_value(cookie_header, &cookie_name);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let authenticated = resolve_session_user(
        pool,
        session_cookie.as_deref(),
        state.settings().secret_key.as_bytes(),
        now_unix,
    )
    .await
    .is_some();
    if !authenticated {
        let ident = if ip.is_empty() {
            "127.0.0.1".to_owned()
        } else {
            ip.clone()
        };
        if !check_email_throttle(state, &ident).await {
            return throttle_429();
        }
    }
    if !instance_is_setup(pool).await.unwrap_or(false) {
        let decision = kernel::EmailCheckDecision::NotConfigured;
        return json_400(&decision.error_pairs());
    }
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&body);
    let email_value = parsed.as_ref().ok().and_then(|v| v.get("email"));
    // A JSON body that is not an object has no `email` member
    // (`request.data.get("email", False)` → `False` → REQUIRED).
    let email_raw = check_email_input(email_value);
    let (email_host, magic_login) = match (
        config_value(pool, "EMAIL_HOST", Some(String::new())).await,
        config_value(pool, "ENABLE_MAGIC_LINK_LOGIN", Some("1".to_owned())).await,
    ) {
        (Ok(host), Ok(magic)) => (host, magic),
        _ => return server_error(),
    };
    // `User.objects.filter(email=email).first()`: the autoset bit of
    // the existing row (if any) drives the MAGIC_CODE branch.
    let existing_autoset = match email_raw.as_deref() {
        Some(raw) => {
            let normalized = kernel::normalize_email(raw);
            match user_by_email(pool, &normalized).await {
                Ok(row) => row.map(|user| user.is_password_autoset),
                Err(_) => return server_error(),
            }
        }
        None => None,
    };
    let decision = kernel::evaluate_email_check(&kernel::EmailCheck {
        instance_ok: true,
        email_raw: email_raw.as_deref(),
        existing_autoset,
        smtp_configured: email_host.as_deref().is_some_and(|v| !v.is_empty()),
        magic_enabled: magic_login.as_deref() == Some("1"),
    });
    match decision {
        kernel::EmailCheckDecision::Checked(_) => json_ok(decision.body_json()),
        _ => json_400(&decision.error_pairs()),
    }
}

async fn email_check_app(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    serve_email_check(&state, req).await
}

async fn email_check_space(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    serve_email_check(&state, req).await
}

// ---------------------------------------------------------------------------
// Sign-out (`views/app|space/signout.py`)
// ---------------------------------------------------------------------------

/// Delete-cookie for the session cookie (`response.delete_cookie`
/// on the emptied session).
fn session_delete_cookie(ctx: &CookieCtx, cookie_name: &str) -> String {
    session_kernel::render_delete_cookie(cookie_name, "/", "Lax", ctx.domain.as_deref())
}

/// Serve one sign-out endpoint: stamp `last_logout_ip/time`, flush
/// the session row, clear the cookie, redirect. Every failure inside
/// the `try` — anonymous callers included — redirects to the same
/// target (`Q-anon-signout`).
async fn serve_signout(state: &AppState, req: axum::extract::Request, space: bool) -> Response {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .unwrap_or_default();
    let hosts = host_ctx(state);
    let cookie_name = state.settings().session.cookie_name.clone();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let prov = provenance(&parts.headers, peer, &cookie_name);
    if csrf_gate(&prov, &form_pairs(prov.content_type.as_deref(), &body)).is_err() {
        return csrf_failure_response(&hosts.root_base);
    }
    let hosts_ref = allowed_hosts_ref(&hosts.allowed_hosts);
    let next_path = form_pairs(prov.content_type.as_deref(), &body)
        .iter()
        .rev()
        .find(|(k, _)| k == "next_path")
        .map(|(_, v)| v.clone());
    let location = if space {
        kernel::signout_space_location(&hosts.space_base, next_path.as_deref(), &hosts_ref)
    } else {
        kernel::signout_app_location(&hosts.app_base)
    };
    let ctx = cookie_ctx(state);
    let delete_cookie = session_delete_cookie(&ctx, &cookie_name);
    // The `try` body: `request.user` must be authenticated (valid
    // session + hash + active user — `User.objects.get(pk=None)`
    // raises for anonymous callers); then the stamp and the flush.
    // Any failure skips the writes and only redirects.
    if let Some(pool) = state.pools().map(|pools| pools.primary()) {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let user_id = resolve_session_user(
            pool,
            prov.session_cookie.as_deref(),
            state.settings().secret_key.as_bytes(),
            now_unix,
        )
        .await;
        if let Some(user_id) = user_id {
            let now = chrono::Utc::now();
            let stamped = sqlx::query(
                r#"UPDATE "users" SET "last_logout_ip" = $2, "last_logout_time" = $3, "updated_at" = $3 WHERE "id" = $1"#,
            )
            .bind(user_id)
            .bind(prov.ip.clone())
            .bind(now)
            .execute(pool)
            .await;
            if stamped.is_ok() {
                if let Some(key) = prov.session_cookie.as_deref() {
                    let _ = sqlx::query(r#"DELETE FROM "sessions" WHERE "session_key" = $1"#)
                        .bind(key)
                        .execute(pool)
                        .await;
                }
            }
        }
    }
    redirect_response(location, vec![delete_cookie])
}

async fn signout_app(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    serve_signout(&state, req, false).await
}

async fn signout_space(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    serve_signout(&state, req, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prov_with(secret: Option<&str>, token: Option<&str>) -> Provenance {
        Provenance {
            user_agent: "UA/1.0".to_owned(),
            ip: Some("2.2.2.2".to_owned()),
            csrf_cookie: secret.map(str::to_owned),
            header_token: token.map(str::to_owned),
            session_cookie: None,
            content_type: Some("application/x-www-form-urlencoded".to_owned()),
        }
    }

    #[test]
    fn csrf_gate_needs_cookie_and_matching_token() {
        let pairs: Vec<(String, String)> = vec![];
        // No cookie at all.
        assert_eq!(
            csrf_gate(&prov_with(None, Some("whatever")), &pairs),
            Err(CsrfDenial::NoCookie)
        );
        // Cookie but no token anywhere.
        assert_eq!(
            csrf_gate(&prov_with(Some("secret"), None), &pairs),
            Err(CsrfDenial::BadToken)
        );
        // Cookie plus a token that does not unmask to it.
        assert_eq!(
            csrf_gate(&prov_with(Some("secret"), Some("bogus")), &pairs),
            Err(CsrfDenial::BadToken)
        );
        // Masked token over the secret passes.
        let masked = csrf_kernel::mask_secret("abcdefghijklmnopqrstuvwxyz012345").expect("masks");
        assert_eq!(
            csrf_gate(
                &prov_with(Some("abcdefghijklmnopqrstuvwxyz012345"), Some(&masked)),
                &pairs
            ),
            Ok(())
        );
        // The form fallback field works when the header is absent.
        let form = vec![("csrfmiddlewaretoken".to_owned(), masked.clone())];
        assert_eq!(
            csrf_gate(
                &prov_with(Some("abcdefghijklmnopqrstuvwxyz012345"), None),
                &form
            ),
            Ok(())
        );
    }

    #[test]
    fn form_pairs_last_wins_and_ignores_other_content_types() {
        let pairs = form_pairs(
            Some("application/x-www-form-urlencoded"),
            b"email=a%40x.com&email=b%40x.com&password=pw",
        );
        assert_eq!(
            kernel::form_field(&pairs, "email"),
            kernel::FormField::Present("b@x.com")
        );
        assert!(form_pairs(Some("application/json"), b"email=a%40x.com").is_empty());
        assert!(form_pairs(None, b"email=a%40x.com").is_empty());
    }

    #[test]
    fn check_email_input_mirrors_python_truthiness() {
        use serde_json::json;
        assert_eq!(check_email_input(None), None);
        assert_eq!(check_email_input(Some(&json!(null))), None);
        assert_eq!(check_email_input(Some(&json!(false))), None);
        assert_eq!(check_email_input(Some(&json!(""))), None);
        assert_eq!(check_email_input(Some(&json!(0))), None);
        assert_eq!(check_email_input(Some(&json!([]))), None);
        assert_eq!(check_email_input(Some(&json!({}))), None);
        assert_eq!(
            check_email_input(Some(&json!(true))),
            Some("True".to_owned())
        );
        assert_eq!(
            check_email_input(Some(&json!("h@x.com"))),
            Some("h@x.com".to_owned())
        );
        assert_eq!(check_email_input(Some(&json!(123))), Some("123".to_owned()));
    }

    #[test]
    fn password_scores_match_zxcvbn_bands() {
        assert!(shapes_kernel::is_password_too_weak(password_score("123")));
        assert!(!shapes_kernel::is_password_too_weak(password_score(
            "Fresh-Strong-Pass-9!x"
        )));
    }

    #[test]
    fn display_name_takes_the_local_part() {
        assert_eq!(display_name_for("h@x.com"), "h");
        assert_eq!(display_name_for("no-at-sign"), "");
    }

    #[tokio::test]
    async fn routes_own_the_eight_email_paths() {
        use tower::ServiceExt;
        // A registered path routes (the pool-less state answers 500
        // from inside the handler); an unknown path falls through to
        // axum's 404. Every owned method is POST.
        for path in [
            "/auth/sign-in/",
            "/auth/sign-up/",
            "/auth/spaces/sign-in/",
            "/auth/spaces/sign-up/",
            "/auth/email-check/",
            "/auth/spaces/email-check/",
            "/auth/sign-out/",
            "/auth/spaces/sign-out/",
        ] {
            let app = routes().with_state(AppState::new("email-test"));
            let response = app
                .oneshot(
                    axum::http::Request::post(path)
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("serve");
            assert_ne!(
                response.status(),
                axum::http::StatusCode::NOT_FOUND,
                "{path} registered"
            );
        }
    }
}
