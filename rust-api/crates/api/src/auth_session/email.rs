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
//! shell: form/JSON extraction, the `CsrfViewMiddleware` gate (plus
//! the DRF `SessionAuthentication` 403 for email-check), row
//! reads/writes, `login()`/`user_login` session issuance (same-user
//! keys retained, others cycled), sign-out flush plus the session
//! middleware's deletion rule, cookie emission, and the
//! 302/200/400/403/415/429 bytes. Sibling D-16 handler issues
//! (magic, password, CSRF token) extend [`routes`] with their own
//! routers; merges keep both sides. Every other method on the eight
//! paths proxies to Django ([`owned`]), where the framework's own
//! 405s live.
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
//! * `process_workspace_project_invitations` runs on every app
//!   success (the space twins pass no callback): the member
//!   bulk-writes + invite deletes are replayed; the per-workspace
//!   cache invalidation is a documented no-op (Rust serves no cached
//!   members rows, so there is nothing to invalidate — the former D-32
//!   `recent_visited_task` analogy is retired now that task executes in
//!   the Rust worker (PIDASHCONV-1107)), and each
//!   `track_event.delay` publishes its full kwargs (including
//!   `event_properties`) through the F-09 publisher.
//! * The email-check throttle is the process-local DRF loop from the
//!   D-06 governor (shared cache would need a foundation change — a
//!   new issue, not a workaround); single-process deployments observe
//!   exactly Django-with-local-memory-cache behaviour.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

use pidash_auth::csrf as csrf_kernel;
use pidash_auth::session as session_kernel;
use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};
use pidash_services::auth_session::email as kernel;
use pidash_services::auth_session::guards as guards_kernel;
use pidash_services::auth_session::queries as queries_kernel;
use pidash_services::auth_session::shapes as shapes_kernel;
use pidash_services::auth_session::tasks as tasks_kernel;
use pidash_services::domain::identity;
use pidash_services::domain::workspaces::members;

use crate::assistant::governor::throttle_check;
use crate::assistant::throttles::ThrottleSpec;
use crate::auth_session::render::{disallowed_redirect, render_csrf_failure};
use crate::auth_session::throttle_ident;
use crate::edge;
use crate::license::handlers_auth_forms::client_ip;
use crate::middleware::body_limit::read_request_bytes;
use crate::state::AppState;
use pidash_compat::response::{redirect_found_cookies, server_error};

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

use pidash_compat::http::{
    csrf_failure_json, csrf_failure_response, csrf_token_source, form_field, session_auth_hash,
    TokenSource,
};

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
/// fields come straight off the headers. `user_agent` stays `None`
/// when the header is absent (`META.get` returns `None`, stored as
/// NULL) and `Some("")` when it is present but empty.
struct Provenance {
    user_agent: Option<String>,
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
            .map(str::to_owned),
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

/// `request.POST` as ordered pairs: multipart fields when the body is
/// framed multipart, else the urlencoded parse for form posts
/// (PIDASHCONV-1349; same shape as `magic.rs::form_pairs` from
/// PIDASHCONV-1079). Any other content type leaves `POST` empty in
/// Django, so the pairs are empty there.
fn form_pairs(content_type: Option<&str>, body: &[u8]) -> Vec<(String, String)> {
    let content_type = content_type.unwrap_or("");
    if let Some(pairs) = crate::auth_session::multipart::parse_fields(content_type, body) {
        return pairs;
    }
    let mime = content_type.split(';').next().map(str::trim).unwrap_or("");
    let is_form = mime.eq_ignore_ascii_case("application/x-www-form-urlencoded")
        || mime.eq_ignore_ascii_case("multipart/form-data");
    if !is_form {
        return Vec::new();
    }
    serde_urlencoded::from_bytes(body).unwrap_or_default()
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

/// `_check_token` + `_does_token_match` over one token: the format
/// gate first (`has incorrect length` / `has invalid characters`),
/// then the unmask-and-compare, rendered exactly like
/// `_bad_token_message`.
fn csrf_check_token(secret: &str, token: &str, source: TokenSource) -> Result<(), String> {
    if let Err(error) = csrf_kernel::check_token_format(token) {
        return Err(format!(
            "CSRF token from {} {}.",
            source.describe(),
            error.reason()
        ));
    }
    if csrf_kernel::tokens_match(token, secret) {
        Ok(())
    } else {
        Err(format!("CSRF token from {} incorrect.", source.describe()))
    }
}

/// Mirror `CsrfViewMiddleware.process_view` for these endpoints: the
/// `csrftoken` cookie must be present and the request token must
/// check out. A 64-char cookie unmasks first (`_get_secret`); a
/// malformed cookie secret can never match (Django would mint a fresh
/// secret and then fail the compare the same way); the failure page
/// never renders the reason either way.
fn csrf_gate(prov: &Provenance, pairs: &[(String, String)]) -> Result<(), CsrfDenial> {
    let Some(cookie) = prov.csrf_cookie.as_deref() else {
        return Err(CsrfDenial::NoCookie);
    };
    let secret = if cookie.len() == csrf_kernel::CSRF_TOKEN_LENGTH {
        csrf_kernel::unmask_token(cookie).map_err(|_| CsrfDenial::BadToken)?
    } else {
        cookie.to_owned()
    };
    let Some((token, source)) = csrf_token_source(pairs, prov.header_token.as_deref()) else {
        return Err(CsrfDenial::BadToken);
    };
    csrf_check_token(&secret, &token, source).map_err(|_| CsrfDenial::BadToken)
}

// ---------------------------------------------------------------------------
// Redirect / JSON responses
// ---------------------------------------------------------------------------

/// `HttpResponseRedirect`: 302 + `Location`, `Vary: Cookie`, and any
/// `Set-Cookie` values the branch issued. Django percent-encodes the URL
/// first (`iri_to_uri`, `django/http/response.py:620`). Past
/// `MAX_URL_REDIRECT_LENGTH` characters the final `Location` answers
/// 400 (`DisallowedRedirect`).
fn redirect_response(location: String, set_cookies: Vec<String>) -> Response {
    let too_long = location.chars().count() > shapes_kernel::MAX_URL_REDIRECT_LENGTH;
    let encoded = pidash_compat::http::iri_to_uri(&location);
    let Some(location) = crate::response_headers::header_value(&encoded) else {
        return server_error();
    };
    if too_long {
        return disallowed_redirect();
    }
    redirect_found_cookies(location, set_cookies)
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

/// DRF `UnsupportedMediaType` (`exceptions.py:217`): the request's
/// content type echoed verbatim. `Vary: Cookie`, same as above.
/// Shared with the magic-generate handler (PIDASHCONV-1351), whose
/// DRF views parse `request.data` under the same default parsers.
pub(crate) fn unsupported_media_type_response(content_type: &str) -> Response {
    (
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::VARY, "Cookie"),
        ],
        format!(
            "{{\"detail\":\"Unsupported media type \\\"{}\\\" in request.\"}}",
            content_type.replace('\\', "\\\\").replace('"', "\\\"")
        ),
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
/// mismatch *and* on unparsable hashes. The setter side effect
/// (re-hash outdated hashes) lives in `provider_outcome`, not here.
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
// User writes (`adapter/base.py:220-360`, via `domain::identity`)
// ---------------------------------------------------------------------------
// NOTE: user create, the `create_user_notification` post_save insert and
// the signup profile row moved to `domain::identity::create_user`
// (PIDASHCONV-1525), which runs the entity insert (the `after_save` hook
// fires) plus the service-level profile row. The signup arm below calls
// it directly.

/// `save_user_data` (`base.py:220-234`): the login stamps plus the
/// `is_active` flip in one `UPDATE`. The activation mail is published by
/// the caller BEFORE this write (`base.py:230` precedes `:232-233`), so a
/// broker failure 500s with the user still inactive and the mail is
/// re-sent on the next login (QUIRK-activation-order).
async fn save_user_data(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_agent: Option<String>,
    ip: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let token = fresh_user_token();
    identity::save_user_data(
        pool,
        user_id,
        &identity::LoginStamps {
            medium: Some("email"),
            login_ip: ip.as_deref(),
            login_uagent: user_agent.as_deref(),
            avatar: None,
            avatar_asset_id: None,
            display_name: None,
            is_staff: None,
            // The email stamps never bump `updated_at` (preserved verbatim;
            // see `domain::identity`).
            touch_updated_at: false,
            rotated_token: &token,
            now,
        },
    )
    .await
}

/// Fresh `User.save()` token: `uuid4().hex + uuid4().hex`
/// (`db/models/user.py:174`).
pub(crate) fn fresh_user_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// NOTE: the change/set/reset-password write (the password plus the
// `User.save()` column effects) moved to
// `domain::identity::write_password_with_save_effects` (PIDASHCONV-1628),
// and the `user_logged_in` `last_login` stamp to
// `domain::identity::stamp_last_login`; the change/set/reset callers live
// in `password.rs`.

/// `get_random_color` (`utils/color.py:9-14`): `"#"` + six hexdigits.
/// The uuid draw is lowercase-only (see `domain::identity`); no test can
/// pin a random color.
fn profile_background_color() -> String {
    let color_suffix: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(6)
        .collect();
    format!("#{color_suffix}")
}

/// `get_redirection_path` (`redirection_path.py:8-46`): onboarding
/// first, then the active last workspace, then the earliest active
/// membership, then invites, else create-workspace.
async fn redirection_path(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    user_email: &str,
) -> Result<String, sqlx::Error> {
    let path = identity::get_or_create_profile(
        pool,
        user_id,
        chrono::Utc::now(),
        &profile_background_color(),
    )
    .await?;
    let (is_onboarded, last_workspace_id) = (path.is_onboarded, path.last_workspace_id);
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

/// Accepted workspace invites newest-first (`WorkspaceMemberInvite.Meta`
/// `ordering = ("-created_at",)`): `(workspace_id, role, slug)`.
async fn accepted_workspace_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<(uuid::Uuid, i16, String)>, sqlx::Error> {
    // `role` is `smallint`: decoding into `i32` fails sqlx's strict
    // type check, so the invite path 500s with zero rows (found while
    // replaying the bulk join).
    let rows: Vec<(uuid::Uuid, i16, String)> = sqlx::query_as(
        r#"SELECT i."workspace_id", i."role", w."slug"
           FROM "workspace_member_invites" i
           JOIN "workspaces" w ON w."id" = i."workspace_id"
           WHERE i."email" = $1 AND i."accepted" = true
           ORDER BY i."created_at" DESC"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Accepted project invites newest-first (`ProjectMemberInvite.Meta`
/// `ordering = ("-created_at",)`): `(workspace_id, role, created_by)`.
async fn accepted_project_invites(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Vec<(uuid::Uuid, i16, Option<uuid::Uuid>)>, sqlx::Error> {
    // `role` is `smallint` (see above): `i16`, not `i32`.
    let rows: Vec<(uuid::Uuid, i16, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT "workspace_id", "role", "created_by_id"
           FROM "project_member_invites" WHERE "email" = $1 AND "accepted" = true
           ORDER BY "created_at" DESC"#,
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Map a project-invite role onto a member role (`... in [5, 15]`
/// else 15). Same mapping as the OAuth twin (`map_invite_role`).
fn invite_member_role(role: i16) -> i16 {
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

/// `track_event.delay(user_id=..., event_name=...,
/// slug=..., event_properties=...)` for one accepted workspace
/// invite: kwargs form like every sibling publisher (and like
/// `.delay` itself) — the worker's `parse_track_event_call`
/// requires the `event_properties` object.
fn track_event_message(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    role: i16,
    joined_at: &str,
) -> pidash_jobs::TaskMessage {
    let call = queries_kernel::workspace_join_track_event_call(
        user_id,
        workspace_id,
        slug,
        i32::from(role),
        joined_at,
    );
    pidash_jobs::TaskMessage::new(
        pidash_jobs::tasks_webhooks::sinks::TRACK_EVENT_TASK,
        vec![],
        call.as_object().cloned().unwrap_or_default(),
    )
}

/// Enqueue one task message onto the Postgres-backed queue (`rust_job_queue`).
/// Enqueue errors propagate (500), like `.delay()` raising in Django (the
/// `auth_session::magic` precedent).
async fn publish_message(
    pool: &sqlx::PgPool,
    message: &pidash_jobs::TaskMessage,
) -> Result<(), sqlx::Error> {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    pidash_jobs::queue::enqueue(pool, &job).await.map(|_| ())
}

/// `WorkspaceMember.objects.bulk_create(..., ignore_conflicts=True)`
/// over ALL accepted project invites (`workspace_project_join.py:62-73`):
/// one multi-row `INSERT` with `ON CONFLICT DO NOTHING`, committed before
/// the project arm's ported `IntegrityError` below. Same
/// `QueryBuilder.push_values` shape as the OAuth twin
/// (`auth_oauth/oauth_google.rs:insert_workspace_members`); empty input
/// emits no statement (Django's `bulk_create([])` is a no-op).
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn insert_project_arm_workspace_members(
    pool: &sqlx::PgPool,
    rows: &[(uuid::Uuid, i16, Option<uuid::Uuid>)],
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), Response> {
    if rows.is_empty() {
        return Ok(());
    }
    let default_props = default_props_json();
    let issue_props = issue_props_json();
    let bulk_rows: Vec<members::BulkMember> = rows
        .iter()
        .map(|(workspace_id, role, created_by)| members::BulkMember {
            workspace_id: *workspace_id,
            user_id,
            role: invite_member_role(*role),
            created_by: *created_by,
        })
        .collect();
    members::bulk_add(pool, &bulk_rows, &default_props, &issue_props, now)
        .await
        .map_err(|_| server_error())?;
    Ok(())
}

/// `ProjectMember.objects.bulk_create(..., ignore_conflicts=True)` as
/// written (`workspace_project_join.py:76-87`): the `ProjectMember`
/// constructor never sets `project`, so `project_id` is missing although
/// the column is NOT NULL and any accepted project invite raises
/// `IntegrityError` even with `ON CONFLICT DO NOTHING` — the ported 500,
/// with the workspace rows above already committed. Same
/// `QueryBuilder.push_values` shape as the OAuth twin
/// (`auth_oauth/oauth_google.rs:insert_project_members`).
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn insert_project_arm_project_members(
    pool: &sqlx::PgPool,
    rows: &[(uuid::Uuid, i16, Option<uuid::Uuid>)],
    user_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), Response> {
    if rows.is_empty() {
        return Ok(());
    }
    let default_props = default_props_json();
    let preferences = default_preferences_json();
    let bulk: Vec<pidash_services::domain::projects::members::BulkRow> = rows
        .iter()
        .map(|(workspace_id, role, created_by)| {
            pidash_services::domain::projects::members::BulkRow {
                workspace_id: *workspace_id,
                // PB-1008: the project arm omits `project_id`; the
                // explicit `NULL` raises the same 23502 as the omitted
                // column (identical message, identical effect).
                project_id: None,
                user_id,
                role: pidash_services::domain::projects::members::RoleBind::Int(i64::from(
                    invite_member_role(*role),
                )),
                created_by: *created_by,
                created_at: now,
                updated_at: now,
            }
        })
        .collect();
    pidash_services::domain::projects::members::bulk_add(
        pool,
        &bulk,
        &default_props,
        &preferences,
        false,
    )
    .await
    .map_err(|_| server_error())?;
    Ok(())
}

/// `process_workspace_project_invitations`
/// (`workspace_project_join.py:22-91`): the workspace arm bulk-writes
/// members (per-row `INSERT ... ON CONFLICT DO NOTHING`, same committed
/// rows as the bulk) and publishes one `track_event.delay` per invite;
/// the project arm replays Django's two bulk statements (all
/// `workspace_members` rows first, then the `project_members` bulk that
/// raises the ported `IntegrityError`); then the invite deletes. Cache
/// invalidation is a documented no-op (see the module docs). Each
/// statement is its own autocommit write — no `transaction.atomic`
/// anywhere — so a later failure keeps the earlier rows, exactly like
/// Django.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
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
        members::add(
            pool,
            &members::NewMember {
                workspace_id: *workspace_id,
                user_id: Some(user_id),
                role: *role,
                company_role: None,
                created_by: None,
                view_props: &default_props,
                default_props: &default_props,
                issue_props: &issue_props,
                now,
                ignore_conflicts: true,
                db_now: false,
            },
        )
        .await
        .map_err(|_| server_error())?;
    }
    for (workspace_id, role, slug) in &workspace_invites {
        // `joined_at` is stamped per invite
        // (`workspace_project_join.py:45-56`).
        let message = track_event_message(
            &user_id.to_string(),
            &workspace_id.to_string(),
            slug,
            *role,
            &chrono::Utc::now().to_rfc3339(),
        );
        publish_message(pool, &message)
            .await
            .map_err(|_| server_error())?;
    }
    let project_invites = accepted_project_invites(pool, user_email)
        .await
        .map_err(|_| server_error())?;
    // Two bulk statements, not one loop: Django commits every project-arm
    // `workspace_members` row before the `project_members` bulk below
    // raises the ported `IntegrityError`.
    insert_project_arm_workspace_members(pool, &project_invites, user_id, now).await?;
    insert_project_arm_project_members(pool, &project_invites, user_id, now).await?;
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

/// The auth keys `login()` writes into the session payload.
fn auth_payload_entries(
    user_id: uuid::Uuid,
    password_field: &str,
    device_info: &serde_json::Value,
    secret_key: &[u8],
) -> Vec<(String, serde_json::Value)> {
    vec![
        (
            "_auth_user_id".to_owned(),
            serde_json::Value::String(user_id.to_string()),
        ),
        (
            "_auth_user_backend".to_owned(),
            serde_json::Value::String(MODEL_BACKEND.to_owned()),
        ),
        (
            "_auth_user_hash".to_owned(),
            serde_json::Value::String(session_auth_hash(password_field, secret_key)),
        ),
        ("device_info".to_owned(), device_info.clone()),
    ]
}

/// `login()` + `user_login`, keeping Django's key discipline
/// (`django/contrib/auth/__init__.py:106-118`): a presented session
/// that already authenticates this same user (id + backend + hash
/// all check out) keeps its key — the auth entries are merged over
/// the surviving payload ("data set during the anonymous session is
/// retained") and the row is updated in place. Anything else
/// (anonymous session, another user, stale hash) cycles the key:
/// the presented row is deleted and a fresh row is inserted.
/// `expire_date = now + SESSION_COOKIE_AGE` on both arms. Returns
/// the two `Set-Cookie` values (session + rotated CSRF secret, which
/// `login()` rotates on every login via `rotate_token`).
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
    let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
    let entries = auth_payload_entries(user_id, password_field, &device_info, secret_key);
    let user_id_str = user_id.to_string();
    let expire_unix = now_unix + age_secs;
    if let Some(old) = presented_key.filter(|key| session_kernel::is_plausible_session_key(key)) {
        let row: Option<(String, i64)> = sqlx::query_as(
            r#"SELECT "session_data", EXTRACT(EPOCH FROM "expire_date")::BIGINT FROM "sessions" WHERE "session_key" = $1"#,
        )
        .bind(old)
        .fetch_optional(pool)
        .await?;
        if let Some((session_data, expire_date_unix)) = row {
            if !session_kernel::is_expired(expire_date_unix, now_unix) {
                let unwrapped: Option<serde_json::Map<String, serde_json::Value>> =
                    signer.unsign_object(&session_data).ok();
                if let Some(mut payload) = unwrapped {
                    let same_user = payload.get("_auth_user_id").and_then(|v| v.as_str())
                        == Some(user_id_str.as_str())
                        && payload.get("_auth_user_backend").and_then(|v| v.as_str())
                            == Some(MODEL_BACKEND)
                        && payload.get("_auth_user_hash").and_then(|v| v.as_str())
                            == Some(session_auth_hash(password_field, secret_key).as_str());
                    if same_user {
                        for (key, value) in &entries {
                            payload.insert(key.clone(), value.clone());
                        }
                        let session_data = signer
                            .sign_object(&payload, now_unix as u64)
                            .map_err(|_| sqlx::Error::RowNotFound)?;
                        sqlx::query(
                            r#"UPDATE "sessions" SET "session_data" = $2,
                               "expire_date" = to_timestamp($3::DOUBLE PRECISION),
                               "user_id" = $4, "device_info" = $5
                               WHERE "session_key" = $1"#,
                        )
                        .bind(old)
                        .bind(&session_data)
                        .bind(expire_unix as f64)
                        .bind(user_id.to_string())
                        .bind(sqlx::types::Json(device_info.clone()))
                        .execute(pool)
                        .await?;
                        return Ok(vec![
                            session_set_cookie(old, ctx, cookie_name, age_secs, now_unix),
                            csrf_rotate_cookie(&csrf_kernel::new_secret(), ctx),
                        ]);
                    }
                }
            }
            sqlx::query(r#"DELETE FROM "sessions" WHERE "session_key" = $1"#)
                .bind(old)
                .execute(pool)
                .await?;
        }
    }
    let mut payload = serde_json::Map::new();
    for (key, value) in &entries {
        payload.insert(key.clone(), value.clone());
    }
    let session_data = signer
        .sign_object(&payload, now_unix as u64)
        .map_err(|_| sqlx::Error::RowNotFound)?;
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
/// branch (`base.py:229-230`), enqueued onto the Postgres-backed queue.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn publish_activation(
    pool: &sqlx::PgPool,
    current_site: &str,
    user_id: uuid::Uuid,
) -> Result<(), Response> {
    let message = pidash_jobs::TaskMessage::new(
        tasks_kernel::USER_ACTIVATION_EMAIL_TASK,
        vec![
            serde_json::Value::String(current_site.to_owned()),
            serde_json::Value::String(user_id.to_string()),
        ],
        serde_json::Map::new(),
    );
    publish_message(pool, &message)
        .await
        .map_err(|_| server_error())
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
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn provider_outcome(
    pool: &sqlx::PgPool,
    form: kernel::EmailForm,
    normalized: &str,
    raw_password: &str,
    existing: Option<&mut UserRow>,
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
            // Django's `check_password` setter (`base_user.py:111-123`): a
            // correct password on an outdated hash re-encodes with the
            // default hasher at once (`save(update_fields=["password"])`
            // writes only that column — no `updated_at` bump). This is the
            // same row the session tail reads, so the session hash covers
            // the new field exactly as `login()` does after the setter.
            if let Some(new_hash) =
                pidash_auth::password::rehash_if_outdated(raw_password, &user.password)
            {
                identity::rehash_password(pool, user.id, &new_hash)
                    .await
                    .map_err(|_| server_error())?;
                user.password = new_hash;
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
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
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
    let mut existing = user_by_email(pool, &normalized)
        .await
        .map_err(|_| server_error())?;
    let user_exists = existing.is_some();
    let provider = if (req.form == kernel::EmailForm::SignIn) != user_exists {
        // The view's own existence pre-check fails: the provider is
        // never constructed (`Q-provider-order`).
        kernel::ProviderOutcome::Ok
    } else {
        provider_outcome(pool, req.form, &normalized, raw_password, existing.as_mut()).await?
    };
    // Cloned after the provider ran: a sign-in re-hash above already
    // rewrote `password` in place for the session tail.
    let user_row = existing.clone();
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

/// Run the authenticated tail shared by sign-in and sign-up, in Django
/// order: the activation publish precedes the `save_user_data` write
/// (`base.py:230` before `:232-233`), the invitation callback runs inside
/// `authenticate()` before the view's `user_login()`, and the `last_login`
/// stamp plus session issuance come last (`login.py:15`). `current_site`
/// is the surface base for `device_info.domain` (`user_login` passes its
/// own flags); `activation_site` is the flag-less `base_host(request)` the
/// activation mail builds its profile link from (`base.py:230`).
#[allow(clippy::too_many_arguments)]
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn run_authenticated(
    pool: &sqlx::PgPool,
    state: &AppState,
    req: &EmailRequest,
    user_id: uuid::Uuid,
    password_field: &str,
    user_email: &str,
    was_active: bool,
    current_site: &str,
    activation_site: &str,
) -> Result<Response, Response> {
    let now = chrono::Utc::now();
    // `META.get("HTTP_USER_AGENT")`: `None` when absent (NULL), the
    // raw value — even empty — when present.
    let user_agent = req.prov.user_agent.clone();
    // The activation mail precedes the `is_active` save (`base.py:230`
    // before `:232-233`): a broker failure 500s with the user still
    // inactive, and the mail is re-sent on the next login.
    if !was_active {
        publish_activation(pool, activation_site, user_id).await?;
    }
    save_user_data(pool, user_id, user_agent, req.prov.ip.clone(), now)
        .await
        .map_err(|_| server_error())?;
    // Only the app endpoints pass `callback=post_user_auth_workflow`
    // (`app/email.py:86,208`); the space twins construct the provider
    // without a callback (`space/email.py:80,156`) so the workflow
    // never runs there.
    if !req.space {
        post_user_auth_workflow(pool, user_id, user_email, now).await?;
    }
    // The `user_logged_in` signal fires inside `login()`, which runs in
    // `user_login` AFTER `provider.authenticate()` returns
    // (`app/email.py:109-111`) — so the stamp lands after the invitation
    // workflow, never on its 500 path.
    identity::stamp_last_login(pool, user_id, now)
        .await
        .map_err(|_| server_error())?;
    let device_info = serde_json::json!({
        "user_agent": req.prov.user_agent.as_deref().unwrap_or(""),
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
        return csrf_failure_response(render_csrf_failure(&req.hosts.root_base));
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
    // `save_user_data` mails with flag-less `base_host(request)`
    // (`base.py:230`); everything else on this path uses the surface
    // base.
    let activation_site = req.hosts.root_base.clone();
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
                &activation_site,
            )
            .await
        }
        kernel::EmailForm::SignUp => {
            let now = chrono::Utc::now();
            let Some(orm) = state.orm() else {
                return server_error();
            };
            // `User(...)` + first `save()` (`base.py:296-328`) with the
            // `post_save` preferences row (`user.py:307`) and the adapter
            // `Profile.objects.create(user=user)` (`base.py:342`), all inside
            // `domain::identity::create_user`. The profile lands for every
            // new user on both surfaces (PIDASHCONV-879).
            let created = identity::create_user(
                orm,
                &identity::NewUser {
                    id: None,
                    email: &normalized,
                    username: uuid::Uuid::new_v4().simple().to_string(),
                    password_hash: encode_password(&raw_password),
                    first_name: "",
                    last_name: "",
                    display_name: display_name_for(&normalized),
                    avatar: "",
                    is_email_verified: false,
                    is_password_autoset: false,
                    // Django's field default (`user.py:105`): the insert
                    // stores `last_active = now` (PIDASHCONV-1560).
                    last_active: Some(now),
                    login_ip: Some(""),
                    login_uagent: Some(""),
                    is_bot: false,
                    now,
                    profile: Some(identity::NewProfile {
                        company_name: "",
                        background_color: profile_background_color(),
                    }),
                },
            )
            .await;
            let (user_id, password_field) = match created {
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
                &activation_site,
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
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
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
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
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
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
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
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
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

/// `request.data` for one email-check call (`request.py:325-355`):
/// the `email` member when the media type yields one, `None` for the
/// empty-body `{}` and every other falsy shape, or the content type
/// back when nothing parses it (DRF 415).
fn check_request_email(headers: &HeaderMap, body: &[u8]) -> Result<Option<String>, String> {
    if body.is_empty() {
        return Ok(None);
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime == "application/json" {
        let parsed: Result<serde_json::Value, _> = serde_json::from_slice(body);
        // A JSON body that is not an object has no `email` member
        // (`request.data.get("email", False)` → `False`).
        return Ok(check_email_input(
            parsed.as_ref().ok().and_then(|v| v.get("email")),
        ));
    }
    if mime == "application/x-www-form-urlencoded" {
        // Single-field read through the shared canonical
        // (PIDASHCONV-1626): `multipart::parse_fields` returns `None`
        // for this media type, so `form_pairs` here is exactly the
        // canonical's gate + parse + last-wins. Missing or empty stays
        // missing (`request.data.get("email", False)` is falsy either
        // way).
        return Ok(form_field(body, headers, "email").filter(|value| !value.is_empty()));
    }
    if mime == "multipart/form-data" {
        // Multipart reads through `form_pairs` (the DRF default
        // parsers include `MultiPartParser`; PIDASHCONV-1349):
        // missing or empty stays missing (`request.data.get("email",
        // False)` is falsy either way).
        return Ok(
            match kernel::form_field(&form_pairs(Some(content_type), body), "email") {
                kernel::FormField::Missing => None,
                kernel::FormField::Present("") => None,
                kernel::FormField::Present(value) => Some(value.to_owned()),
            },
        );
    }
    Err(content_type.to_owned())
}

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
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let Some(pool) = state.pools().map(|pools| pools.primary()) else {
        return server_error();
    };
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
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
    // `SessionAuthentication.authenticate`: anonymous callers return
    // `None` before the CSRF check; authenticated callers without a
    // valid token get the 403 (`authentication.py:124-148`).
    if authenticated {
        let csrf_cookie = session_kernel::cookie_value(cookie_header, CSRF_COOKIE_NAME);
        let header_token = parts
            .headers
            .get("x-csrftoken")
            .and_then(|v| v.to_str().ok());
        let content_type = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());
        let pairs = form_pairs(content_type, &body);
        let Some(secret) = csrf_cookie.as_deref() else {
            return csrf_failure_json("CSRF cookie not set.");
        };
        let Some((token, source)) = csrf_token_source(&pairs, header_token) else {
            return csrf_failure_json("CSRF token missing.");
        };
        if let Err(reason) = csrf_check_token(secret, &token, source) {
            return csrf_failure_json(&reason);
        }
    }
    if !authenticated {
        // DRF `get_ident`, not the first-entry `client_ip` stamp:
        // the whole X-Forwarded-For chain is the bucket.
        let ident = throttle_ident(&parts.headers, peer);
        if !check_email_throttle(state, &ident).await {
            return throttle_429();
        }
    }
    if !instance_is_setup(pool).await.unwrap_or(false) {
        let decision = kernel::EmailCheckDecision::NotConfigured;
        return json_400(&decision.error_pairs());
    }
    let email_raw = match check_request_email(&parts.headers, &body) {
        Ok(email_raw) => email_raw,
        Err(media_type) => return unsupported_media_type_response(&media_type),
    };
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

/// Whether the presented session reads empty (`Session.is_empty()`
/// over `SessionStore.load()`): a missing or expired row, an
/// undecodable payload, or an empty dict all read empty — and an
/// implausible key can never match a row, so it reads empty too.
async fn presented_session_is_empty(
    pool: &sqlx::PgPool,
    presented_key: Option<&str>,
    secret_key: &[u8],
    now_unix: i64,
) -> bool {
    let Some(key) = presented_key.filter(|key| session_kernel::is_plausible_session_key(key))
    else {
        return true;
    };
    let row: Option<(String, i64)> = sqlx::query_as(
        r#"SELECT "session_data", EXTRACT(EPOCH FROM "expire_date")::BIGINT FROM "sessions" WHERE "session_key" = $1"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some((session_data, expire_unix)) = row else {
        return true;
    };
    if session_kernel::is_expired(expire_unix, now_unix) {
        return true;
    }
    let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
    let data: Result<serde_json::Value, _> = signer.unsign_object(&session_data);
    !matches!(data, Ok(serde_json::Value::Object(map)) if !map.is_empty())
}

/// Delete-cookie for the session cookie, exactly as
/// `SessionMiddleware` emits it when the post-view session reads
/// empty (never `Secure`/`HttpOnly`; `Domain` only when configured).
fn session_delete_cookie(ctx: &CookieCtx, cookie_name: &str) -> String {
    session_kernel::render_delete_cookie(cookie_name, "/", "Lax", ctx.domain.as_deref())
}

/// Serve one sign-out endpoint: stamp `last_logout_ip/time`, flush,
/// redirect. `logout(request)` flushes (deletes the row) and the
/// session middleware then answers with a deletion cookie because
/// the post-view session reads empty — while every failure inside
/// the `try` (anonymous callers included) skips the writes and only
/// redirects (`Q-anon-signout`), keeping the middleware's own rule:
/// a presented-but-empty session still answers the deletion, a
/// missing cookie or a live anonymous session answers bare.
async fn serve_signout(state: &AppState, req: axum::extract::Request, space: bool) -> Response {
    let (parts, body) = req.into_parts();
    let body = match read_request_bytes(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let hosts = host_ctx(state);
    let cookie_name = state.settings().session.cookie_name.clone();
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let prov = provenance(&parts.headers, peer, &cookie_name);
    if csrf_gate(&prov, &form_pairs(prov.content_type.as_deref(), &body)).is_err() {
        return csrf_failure_response(render_csrf_failure(&hosts.root_base));
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
    // The `try` body: `request.user` must be authenticated (valid
    // session + hash + active user — `User.objects.get(pk=None)`
    // raises for anonymous callers); then the stamp and the flush.
    // Any failure redirects bare, unless the middleware rule fires
    // (a presented-but-empty session still answers the deletion).
    let ctx = cookie_ctx(state);
    let delete_cookie = session_delete_cookie(&ctx, &cookie_name);
    let with_delete = || redirect_response(location.clone(), vec![delete_cookie.clone()]);
    let bare = || redirect_response(location.clone(), Vec::new());
    let secret = state.settings().secret_key.as_bytes();
    let Some(pool) = state.pools().map(|pools| pools.primary()) else {
        return bare();
    };
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(user_id) =
        resolve_session_user(pool, prov.session_cookie.as_deref(), secret, now_unix).await
    else {
        if prov.session_cookie.is_some()
            && presented_session_is_empty(pool, prov.session_cookie.as_deref(), secret, now_unix)
                .await
        {
            return with_delete();
        }
        return bare();
    };
    let now = chrono::Utc::now();
    let stamped = identity::stamp_logout(pool, user_id, prov.ip.as_deref(), now).await;
    if stamped.is_err() {
        return bare();
    }
    // `Session.flush()`: drop the old row; the middleware then
    // answers the deletion because the post-view session reads empty.
    if let Some(key) = prov.session_cookie.as_deref() {
        let _ = sqlx::query(r#"DELETE FROM "sessions" WHERE "session_key" = $1"#)
            .bind(key)
            .execute(pool)
            .await;
    }
    with_delete()
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

    #[test]
    fn overlong_redirect_location_answers_400() {
        let long = format!("http://app.test/?{}", "a".repeat(17_000));
        assert_eq!(
            redirect_response(long, Vec::new()).status(),
            StatusCode::BAD_REQUEST
        );
        let ok = redirect_response("http://app.test/?next_path=/x".to_owned(), Vec::new());
        assert_eq!(ok.status(), StatusCode::FOUND);
    }

    fn prov_with(secret: Option<&str>, token: Option<&str>) -> Provenance {
        Provenance {
            user_agent: Some("UA/1.0".to_owned()),
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
    fn csrf_gate_unmasks_a_64_char_cookie() {
        // `_get_secret`: a masked cookie unmasks before the compare, so the
        // cookie value itself and any other masking of the secret pass.
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        let cookie = csrf_kernel::mask_secret(secret).expect("masks");
        let other = csrf_kernel::mask_secret(secret).expect("masks");
        let pairs: Vec<(String, String)> = vec![];
        assert_eq!(
            csrf_gate(&prov_with(Some(&cookie), Some(&cookie)), &pairs),
            Ok(())
        );
        assert_eq!(
            csrf_gate(&prov_with(Some(&cookie), Some(&other)), &pairs),
            Ok(())
        );
        // A token for a different secret still fails.
        let wrong = csrf_kernel::mask_secret(&csrf_kernel::new_secret()).expect("masks");
        assert_eq!(
            csrf_gate(&prov_with(Some(&cookie), Some(&wrong)), &pairs),
            Err(CsrfDenial::BadToken)
        );
        // A 64-char cookie with invalid characters fails closed.
        assert_eq!(
            csrf_gate(&prov_with(Some(&"!".repeat(64)), Some(&other)), &pairs),
            Err(CsrfDenial::BadToken)
        );
    }

    #[test]
    fn csrf_token_source_prefers_the_form_field() {
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        let masked = csrf_kernel::mask_secret(secret).expect("masks");
        let bogus = "0".repeat(64);
        // Form field wins over the header, even when the form value
        // is wrong and the header is right (`request.POST.get`
        // first, header fallback second).
        let form = vec![("csrfmiddlewaretoken".to_owned(), bogus.clone())];
        assert_eq!(
            csrf_token_source(&form, Some(&masked)),
            Some((bogus.clone(), TokenSource::Post))
        );
        assert_eq!(
            csrf_gate(&prov_with(Some(secret), Some(&masked)), &form),
            Err(CsrfDenial::BadToken)
        );
        // An empty form value falls back to the header.
        let empty = vec![("csrfmiddlewaretoken".to_owned(), String::new())];
        assert_eq!(
            csrf_token_source(&empty, Some(&masked)),
            Some((masked.clone(), TokenSource::Header))
        );
        assert_eq!(
            csrf_gate(&prov_with(Some(secret), Some(&masked)), &empty),
            Ok(())
        );
        // No token anywhere is still missing.
        let pairs: Vec<(String, String)> = vec![];
        assert_eq!(csrf_token_source(&pairs, None), None);
    }

    #[test]
    fn csrf_check_token_renders_django_reasons() {
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        let masked = csrf_kernel::mask_secret(secret).expect("masks");
        assert!(csrf_check_token(secret, &masked, TokenSource::Post).is_ok());
        assert_eq!(
            csrf_check_token(secret, &"x".repeat(64), TokenSource::Post),
            Err("CSRF token from POST incorrect.".to_owned())
        );
        assert_eq!(
            csrf_check_token(secret, &"x".repeat(64), TokenSource::Header),
            Err("CSRF token from the 'X-Csrftoken' HTTP header incorrect.".to_owned())
        );
        assert_eq!(
            csrf_check_token(secret, "short", TokenSource::Header),
            Err("CSRF token from the 'X-Csrftoken' HTTP header has incorrect length.".to_owned())
        );
        assert_eq!(
            csrf_check_token(secret, &"!".repeat(64), TokenSource::Post),
            Err("CSRF token from POST has invalid characters.".to_owned())
        );
    }

    fn headers_with_content_type(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, value.parse().expect("content type"));
        headers
    }

    #[test]
    fn check_request_email_follows_the_media_type() {
        let json = headers_with_content_type("application/json");
        let urlencoded = headers_with_content_type("application/x-www-form-urlencoded");
        let multipart = headers_with_content_type("multipart/form-data; boundary=abc");
        let text = headers_with_content_type("text/plain");
        // JSON object member.
        assert_eq!(
            check_request_email(&json, br#"{"email":"h@x.com"}"#),
            Ok(Some("h@x.com".to_owned()))
        );
        // Empty body is `{}` whatever the media type.
        assert_eq!(check_request_email(&json, b""), Ok(None));
        assert_eq!(check_request_email(&text, b""), Ok(None));
        // Urlencoded reads the last field; empty stays missing.
        assert_eq!(
            check_request_email(&urlencoded, b"email=a%40x.com&email=b%40x.com"),
            Ok(Some("b@x.com".to_owned()))
        );
        assert_eq!(check_request_email(&urlencoded, b"email="), Ok(None));
        // Multipart reads the last field; missing or empty stays missing.
        let framed = b"--abc\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\nh@x.com\r\n--abc--\r\n";
        assert_eq!(
            check_request_email(&multipart, framed),
            Ok(Some("h@x.com".to_owned()))
        );
        let framed_empty =
            b"--abc\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\n\r\n--abc--\r\n";
        assert_eq!(check_request_email(&multipart, framed_empty), Ok(None));
        // Anything else is the content type back (DRF 415).
        assert_eq!(
            check_request_email(&text, b"h@x.com"),
            Err("text/plain".to_owned())
        );
        assert_eq!(
            check_request_email(&HeaderMap::new(), b"h@x.com"),
            Err(String::new())
        );
    }

    #[test]
    fn track_event_message_carries_kwargs_with_properties() {
        let message = track_event_message("uid", "wid", "ws", 5, "2026-09-30T00:00:00+00:00");
        assert!(message.args.is_empty());
        let props = message
            .kwargs
            .get("event_properties")
            .and_then(|v| v.as_object())
            .expect("event_properties object");
        assert_eq!(
            message.kwargs.get("event_name").and_then(|v| v.as_str()),
            Some("user_joined_workspace")
        );
        assert_eq!(props.get("role").and_then(|v| v.as_i64()), Some(5));
        assert_eq!(
            props.get("workspace_id").and_then(|v| v.as_str()),
            Some("wid")
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
        // Framed multipart reads fields in order (last wins downstream).
        let framed = b"--abc\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\na@x.com\r\n--abc\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\nb@x.com\r\n--abc--\r\n";
        let pairs = form_pairs(Some("multipart/form-data; boundary=abc"), framed);
        assert_eq!(
            kernel::form_field(&pairs, "email"),
            kernel::FormField::Present("b@x.com")
        );
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
