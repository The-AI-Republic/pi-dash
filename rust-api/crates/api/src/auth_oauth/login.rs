//! Shared `django.contrib.auth.login()` session semantics for the four
//! OAuth providers (PIDASHCONV-1357).
//!
//! Django's `user_login` (`authentication/utils/login.py:15-27`) calls
//! `django.contrib.auth.login()`, which — in the pinned Django 4.2.30
//! (`contrib/auth/__init__.py:107-143`) — does three things every OAuth
//! port must repeat:
//!
//! 1. Session-key rotation: `flush()` when `_auth_user_id` is present for
//!    a *different* user or the stored `_auth_user_hash` mismatches (data
//!    gone, old row deleted); `cycle_key()` when anonymous (data kept,
//!    old row deleted); untouched for the same user with a matching hash.
//! 2. `user_logged_in` → `update_last_login` writes
//!    `users.last_login = now()`.
//! 3. `rotate_token(request)` mints a fresh CSRF secret, which
//!    `CsrfViewMiddleware` sets as the `csrftoken` cookie on the response.
//!
//! [`login_session_keys`] performs all three and returns the `csrftoken`
//! `Set-Cookie` value; callers append it with [`push_cookie`].
//! `device_info` stays per-provider (each callback computes its own
//! domain) and is written after this returns, matching `login.py` order
//! (`login()` first, `device_info` second).

use axum::http::header;
use axum::response::Response;
use serde_json::Value;
use sqlx::PgPool;

use crate::middleware::SessionHandle;

/// `django.contrib.auth.backends.ModelBackend`: the only configured auth
/// backend (`login.py` passes no `backend`, so `login()` resolves the
/// single configured one).
pub const LOGIN_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";

/// `settings.CSRF_COOKIE_NAME` (project default, unchanged).
const CSRF_COOKIE_NAME: &str = "csrftoken";

/// `settings.CSRF_COOKIE_AGE` (one year, the Django default).
const CSRF_COOKIE_MAX_AGE: i64 = 31_449_600;

/// `user.get_session_auth_hash()`: `salted_hmac` over the `users.password`
/// field with the `AbstractBaseUser.get_session_auth_hash` key salt.
/// Canonical impl is [`pidash_compat::http::session_auth_hash`]; this keeps
/// the provider path stable.
pub fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    pidash_compat::http::session_auth_hash(password_field, secret_key)
}

/// `django.contrib.auth.login()` (`auth/__init__.py:107-143`) for an OAuth
/// callback: rotate-or-flush the session key (deleting the abandoned row
/// either way, like `flush()`/`cycle_key()`), write the `_auth_*` trio,
/// stamp `users.last_login`, and render the rotated `csrftoken`
/// `Set-Cookie` (returned; the caller appends it to the success redirect).
///
/// Every lock below is statement-scoped: the guard never crosses an
/// `.await`, per the session layer's contract ("neither the layer nor
/// handlers await while locked").
pub async fn login_session_keys(
    handle: &SessionHandle,
    pool: &PgPool,
    user_id: uuid::Uuid,
    password_field: &str,
    secret_key: &[u8],
    settings: &pidash_db::config::Settings,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, sqlx::Error> {
    let user_id_str = user_id.to_string();
    let session_hash = session_auth_hash(password_field, secret_key);
    let (current_user, current_hash, old_key): (Option<String>, Option<String>, Option<String>) = {
        let mut session = handle.lock();
        (
            session
                .get("_auth_user_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            session
                .get("_auth_user_hash")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            session.key.clone(),
        )
    };
    // `if SESSION_KEY in request.session: if different user or stale
    // hash: flush(); else: cycle_key()`. A same-user session with a
    // matching hash is left alone — Django does not cycle it.
    let flush = match current_user.as_deref() {
        Some(current) => {
            current != user_id_str
                || (!session_hash.is_empty()
                    && current_hash.as_deref() != Some(session_hash.as_str()))
        }
        None => false,
    };
    if flush {
        handle.lock().clear();
        if let Some(old) = old_key {
            sqlx::query("DELETE FROM \"sessions\" WHERE \"session_key\" = $1")
                .bind(&old)
                .execute(pool)
                .await?;
        }
    } else if current_user.is_none() {
        // Anonymous `cycle_key()`: fresh key on save, data kept, the
        // pre-login row deleted.
        handle.lock().key = None;
        if let Some(old) = old_key {
            sqlx::query("DELETE FROM \"sessions\" WHERE \"session_key\" = $1")
                .bind(&old)
                .execute(pool)
                .await?;
        }
    }
    {
        let mut session = handle.lock();
        session.set("_auth_user_id".to_owned(), Value::String(user_id_str));
        session.set(
            "_auth_user_backend".to_owned(),
            Value::String(LOGIN_BACKEND.to_owned()),
        );
        session.set("_auth_user_hash".to_owned(), Value::String(session_hash));
    }
    // `user_logged_in` → `update_last_login` (`auth/models.py`): the
    // signal save, inside `login()` before the session persists.
    sqlx::query("UPDATE \"users\" SET \"last_login\" = $1 WHERE \"id\" = $2")
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;
    // `rotate_token` half (QUIRK-csrf-rotate): a fresh 32-char secret as
    // the cookie value — `_add_new_csrf_cookie` stores the raw secret
    // (the `auth_session/email.rs` and `auth_session/magic.rs` login
    // ports do the same). Attributes follow
    // Django's cookie writer (secure/domain from settings, httponly per
    // the project override, `/` + `Lax` defaults).
    let secret = pidash_auth::csrf::new_secret();
    Ok(pidash_auth::session::render_set_cookie(
        &pidash_auth::session::SetCookie {
            name: CSRF_COOKIE_NAME.to_owned(),
            value: secret,
            expires: Some(pidash_auth::session::http_date(
                now.timestamp() + CSRF_COOKIE_MAX_AGE,
            )),
            max_age: Some(CSRF_COOKIE_MAX_AGE),
            domain: settings.session.cookie_domain.clone(),
            path: "/".to_owned(),
            secure: settings.session.cookie_secure,
            httponly: true,
            samesite: "Lax".to_owned(),
        },
    ))
}

/// Push one `Set-Cookie` without disturbing the layer's own (Django's
/// login response carries both the session and the rotated CSRF cookie).
pub fn push_cookie(response: &mut Response, value: String) {
    if let Ok(value) = axum::http::HeaderValue::from_str(&value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

#[cfg(test)]
mod tests {
    use super::session_auth_hash;

    // `session_auth_hash` goldens: `salted_hmac` vectors from the
    // contract harness (`_harness/http.py::session_auth_hash`), which
    // mirrors Django's `get_session_auth_hash`.
    #[test]
    fn session_auth_hash_matches_django_vectors() {
        assert_eq!(
            session_auth_hash(
                "pbkdf2_sha256$600000$abc$def",
                b"contract-1357-fixed-secret"
            ),
            "b0e4e43bc88423e12b4733b8049b3a0c9e01c854e932de289f7ee18809c85e8d"
        );
        assert_eq!(
            session_auth_hash("pw", b"secret"),
            "78c7ecf82272227e62a5aa5f01cc8e2e3b4ce3b13352ddc6521ec0c4fa3e7f81"
        );
        // Relocated oracle from the removed gitlab `make_session_auth_hash`
        // (PIDASHCONV-1357): Django `salted_hmac(...get_session_auth_hash,
        // "pbkdf2_sha256$1200000$abc$def", secret="topsecret",
        // algorithm="sha256")` (the default `algorithm="sha1"` is a
        // trap — `get_session_auth_hash` passes sha256 explicitly).
        assert_eq!(
            session_auth_hash("pbkdf2_sha256$1200000$abc$def", b"topsecret"),
            "9c9a3cd895ca244f0c1fc8a4d24cd83d65287bb3353f39d69ab926a0d240b079"
        );
    }

    // -- live kernel tests (env-gated, PIDASHCONV-1357) ------------------
    // Same convention as the github `github_kernel_*` tests: unset
    // DATABASE_URL (plain `cargo test`) skips these. They pin the three
    // rotation branches, the `last_login` write, and the CSRF cookie shape
    // of `login_session_keys` — the same-user and stale-hash branches no
    // HTTP test drives, and the github half of the shared helper whose
    // HTTP success path is sealed on both backends (hardcoded
    // `https://github.com` URLs; PIDASHCONV-1083).

    async fn kernel_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-kernel test: DATABASE_URL is not set");
                None
            }
        }
    }

    fn kernel_settings() -> pidash_db::config::Settings {
        let mut vars = std::collections::HashMap::new();
        vars.insert("APP_BASE_URL".to_owned(), "http://app.test".to_owned());
        pidash_db::config::Settings::from_map_with(
            &vars,
            pidash_db::config::Profile::Common,
            &pidash_db::config::NoOverlay,
        )
        .expect("kernel settings")
    }

    /// Minimal `users` row (the harness `make_user` column set); returns
    /// the id. The password field is an opaque string here — the helper
    /// only HMACs it.
    async fn kernel_user(pool: &sqlx::PgPool, email: &str, password_field: &str) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        let username = format!("u-{}", &id.simple().to_string()[..8]);
        let token = format!("k1357-{}", id.simple());
        sqlx::query(
            "INSERT INTO users (password, id, username, email, first_name, last_name, \
             avatar, date_joined, created_at, updated_at, last_location, created_location, \
             is_superuser, is_managed, is_password_expired, is_active, is_staff, \
             is_email_verified, is_password_autoset, token, user_timezone, last_login_ip, \
             last_logout_ip, last_login_medium, last_login_uagent, is_bot, display_name, \
             is_email_valid, is_password_reset_required) \
             VALUES ($1,$2,$3,$4,'Kernel','User','',now(),now(),now(),'','', \
             false,false,false,true,false,true,false,$5,'UTC','','', \
             'email','',false,'Kernel User',true,false)",
        )
        .bind(password_field)
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(token)
        .execute(pool)
        .await
        .expect("insert kernel user");
        id
    }

    async fn kernel_session_row(pool: &sqlx::PgPool, key: &str) {
        sqlx::query(
            "INSERT INTO sessions (session_key, session_data, expire_date) \
             VALUES ($1, 'e30:0:kernel', now() + make_interval(days => 7))",
        )
        .bind(key)
        .execute(pool)
        .await
        .expect("insert kernel session row");
    }

    async fn session_row_exists(pool: &sqlx::PgPool, key: &str) -> bool {
        sqlx::query_as::<_, (i32,)>("SELECT 1 FROM sessions WHERE session_key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await
            .expect("probe session row")
            .is_some()
    }

    async fn last_login(
        pool: &sqlx::PgPool,
        id: uuid::Uuid,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        sqlx::query_as::<_, (Option<chrono::DateTime<chrono::Utc>>,)>(
            "SELECT last_login FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read last_login")
        .0
    }

    async fn kernel_cleanup(pool: &sqlx::PgPool, emails: &[&str], keys: &[&str]) {
        for email in emails {
            sqlx::query("DELETE FROM users WHERE email = $1")
                .bind(email)
                .execute(pool)
                .await
                .expect("delete kernel user");
        }
        for key in keys {
            sqlx::query("DELETE FROM sessions WHERE session_key = $1")
                .bind(key)
                .execute(pool)
                .await
                .expect("delete kernel session");
        }
    }

    fn kernel_handle(
        key: Option<String>,
        pairs: &[(&str, &str)],
    ) -> crate::middleware::SessionHandle {
        let mut session = crate::middleware::RequestSession::empty();
        session.key = key;
        for (k, v) in pairs {
            session.set((*k).to_owned(), serde_json::Value::String((*v).to_owned()));
        }
        crate::middleware::SessionHandle::new(session)
    }

    /// The `csrftoken` cookie shape Django 4.2 emits on `rotate_token`:
    /// raw 32-char secret, Morsel attribute order, no `Domain`/`Secure`
    /// on plain HTTP.
    fn assert_csrf_cookie_shape(cookie: &str) {
        let (head, attrs) = cookie.split_once(';').expect("cookie has attributes");
        let value = head.strip_prefix("csrftoken=").expect("csrftoken name");
        assert_eq!(value.len(), 32, "raw secret, not masked: {value:?}");
        assert!(
            value.bytes().all(|b| b.is_ascii_alphanumeric()),
            "alphabet-only: {value:?}"
        );
        assert!(
            attrs.contains("; HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax"),
            "django attributes: {cookie:?}"
        );
        assert!(!cookie.contains("Domain="), "no domain: {cookie:?}");
        assert!(!cookie.contains("Secure"), "no secure on http: {cookie:?}");
    }

    const KERNEL_SECRET: &[u8] = b"kernel-secret";

    #[tokio::test]
    async fn login_kernel_anonymous_cycles_key_keeps_data() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let email = format!("k1357-anon-{tag}@example.test");
        let password_field = "pbkdf2_sha256$600000$salt$hash";
        let user_id = kernel_user(&pool, &email, password_field).await;
        assert_eq!(last_login(&pool, user_id).await, None);
        let old_key = format!("k1357old{tag}");
        kernel_session_row(&pool, &old_key).await;
        let handle = kernel_handle(
            Some(old_key.clone()),
            &[("state", "st"), ("host", "http://x.test")],
        );
        let cookie = super::login_session_keys(
            &handle,
            &pool,
            user_id,
            password_field,
            KERNEL_SECRET,
            &kernel_settings(),
            chrono::Utc::now(),
        )
        .await
        .expect("login succeeds");
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.key, None, "cycle drops the key");
        let data = snapshot.to_json();
        assert_eq!(data["state"], serde_json::Value::String("st".to_owned()));
        assert_eq!(
            data["host"],
            serde_json::Value::String("http://x.test".to_owned())
        );
        assert_eq!(
            data["_auth_user_id"],
            serde_json::Value::String(user_id.to_string())
        );
        assert_eq!(
            data["_auth_user_backend"],
            serde_json::Value::String(super::LOGIN_BACKEND.to_owned())
        );
        assert_eq!(
            data["_auth_user_hash"],
            serde_json::Value::String(super::session_auth_hash(password_field, KERNEL_SECRET))
        );
        assert!(
            !session_row_exists(&pool, &old_key).await,
            "cycle deletes the pre-login row"
        );
        assert!(
            last_login(&pool, user_id).await.is_some(),
            "last_login stamped"
        );
        assert_csrf_cookie_shape(&cookie);
        kernel_cleanup(&pool, &[&email], &[&old_key]).await;
    }

    #[tokio::test]
    async fn login_kernel_different_user_flushes() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let email_a = format!("k1357-a-{tag}@example.test");
        let email_b = format!("k1357-b-{tag}@example.test");
        let id_a = kernel_user(&pool, &email_a, "pw-a").await;
        let id_b = kernel_user(&pool, &email_b, "pw-b").await;
        let old_key = format!("k1357old{tag}");
        kernel_session_row(&pool, &old_key).await;
        let hash_a = super::session_auth_hash("pw-a", KERNEL_SECRET);
        let handle = kernel_handle(
            Some(old_key.clone()),
            &[
                ("_auth_user_id", &id_a.to_string()),
                ("_auth_user_hash", &hash_a),
                ("probe_marker", "A123"),
            ],
        );
        let cookie = super::login_session_keys(
            &handle,
            &pool,
            id_b,
            "pw-b",
            KERNEL_SECRET,
            &kernel_settings(),
            chrono::Utc::now(),
        )
        .await
        .expect("login succeeds");
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.key, None, "flush drops the key");
        let data = snapshot.to_json();
        assert!(data.get("probe_marker").is_none(), "A's keys are gone");
        assert_eq!(
            data["_auth_user_id"],
            serde_json::Value::String(id_b.to_string())
        );
        assert!(
            !session_row_exists(&pool, &old_key).await,
            "flush deletes A's row"
        );
        assert!(last_login(&pool, id_b).await.is_some(), "B stamped");
        assert_eq!(last_login(&pool, id_a).await, None, "A untouched");
        assert_csrf_cookie_shape(&cookie);
        kernel_cleanup(&pool, &[&email_a, &email_b], &[&old_key]).await;
    }

    #[tokio::test]
    async fn login_kernel_same_user_matching_hash_keeps_key() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let email = format!("k1357-same-{tag}@example.test");
        let password_field = "pw-same";
        let user_id = kernel_user(&pool, &email, password_field).await;
        let key = format!("k1357keep{tag}");
        kernel_session_row(&pool, &key).await;
        let hash = super::session_auth_hash(password_field, KERNEL_SECRET);
        let handle = kernel_handle(
            Some(key.clone()),
            &[
                ("_auth_user_id", &user_id.to_string()),
                ("_auth_user_hash", &hash),
                ("kept", "1"),
            ],
        );
        let cookie = super::login_session_keys(
            &handle,
            &pool,
            user_id,
            password_field,
            KERNEL_SECRET,
            &kernel_settings(),
            chrono::Utc::now(),
        )
        .await
        .expect("login succeeds");
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.key, Some(key.clone()), "same user keeps the key");
        let data = snapshot.to_json();
        assert_eq!(data["kept"], serde_json::Value::String("1".to_owned()));
        assert!(
            session_row_exists(&pool, &key).await,
            "no cycle means no row delete"
        );
        assert!(
            last_login(&pool, user_id).await.is_some(),
            "last_login still stamped"
        );
        assert_csrf_cookie_shape(&cookie);
        kernel_cleanup(&pool, &[&email], &[&key]).await;
    }

    #[tokio::test]
    async fn login_kernel_stale_hash_flushes() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let email = format!("k1357-stale-{tag}@example.test");
        let user_id = kernel_user(&pool, &email, "pw-new").await;
        let key = format!("k1357stale{tag}");
        kernel_session_row(&pool, &key).await;
        let handle = kernel_handle(
            Some(key.clone()),
            &[
                ("_auth_user_id", &user_id.to_string()),
                ("_auth_user_hash", "stale-hash"),
                ("probe_marker", "old"),
            ],
        );
        let cookie = super::login_session_keys(
            &handle,
            &pool,
            user_id,
            "pw-new",
            KERNEL_SECRET,
            &kernel_settings(),
            chrono::Utc::now(),
        )
        .await
        .expect("login succeeds");
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.key, None, "stale hash flushes");
        let data = snapshot.to_json();
        assert!(data.get("probe_marker").is_none(), "old data is gone");
        assert!(
            !session_row_exists(&pool, &key).await,
            "flush deletes the row"
        );
        assert!(
            last_login(&pool, user_id).await.is_some(),
            "last_login stamped"
        );
        assert_csrf_cookie_shape(&cookie);
        kernel_cleanup(&pool, &[&email], &[&key]).await;
    }
}
