#![forbid(unsafe_code)]

//! Session handling, mirroring
//! `pi_dash.authentication.middleware.session.SessionMiddleware`.
//!
//! Python references: `apps/api/pi_dash/authentication/middleware/session.py`
//! and `apps/api/pi_dash/db/models/session.py`. The pure request half
//! (cookie-name routing, key screening, `session_data` decoding, expiry
//! predicate) already lives in [`pidash_auth::session`]; this module is the
//! tower half: loading the session into request extensions, and the full
//! `process_response` contract (delete vs. save vs. touch, cookie
//! attributes, `Vary: Cookie`).
//!
//! `process_response` rules, in order:
//!
//! 1. When the request carried the session cookie but the session is now
//!    empty, the cookie is deleted (`Max-Age=0`, epoch expiry) and
//!    `Vary: Cookie` is patched — regardless of response status.
//! 2. Otherwise, when the session was accessed, `Vary: Cookie` is patched.
//! 3. When the session was modified (or `SESSION_SAVE_EVERY_REQUEST` is on)
//!    and is non-empty and the response status is below 500, the session is
//!    saved and a `Set-Cookie` is issued. A browser-close session carries no
//!    `Max-Age`/`expires`; otherwise the admin cookie uses
//!    `ADMIN_SESSION_COOKIE_AGE` on `instances` paths and the ordinary
//!    cookie uses the session expiry age. Attributes are always `Path=/`,
//!    `SameSite=Lax`, the configured domain, and `Secure`/`HttpOnly` only
//!    when truthy — see [`pidash_auth::session::render_set_cookie`].
//! 4. A failed save raises `SessionInterrupted` in Django (a 500); here the
//!    layer answers `500` with an empty body.
//!
//! Request side, the layer mirrors Django `get_user` once for every route:
//! the backend must be `ModelBackend`, the `users` row must exist and be
//! active, and `_auth_user_hash` must match the current password field. A
//! hash mismatch flushes the session (row and cookie); the other failures
//! are anonymous without a flush, exactly like Django. The rejected bare
//! auth keys are hidden from handlers so their existing 401 arms fire.
//! Without a store (`SessionConfig.store == None`) the layer is fully
//! transparent: nothing is loaded, saved, or set. That is the deployed
//! shape until `serve` boots pools — Django behind the proxy owns sessions
//! meanwhile, and a transparent layer cannot disturb its cookies.
//!
//! The store persists the `sessions` table (`session_key` primary key,
//! `session_data` TimestampSigner-signed JSON, `expire_date`, plus the
//! mirrored `user_id` / `device_info` columns from `create_model_instance`:
//! `_auth_user_id` when it is a string, `device_info` when it is a dict).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use tower::{Layer, Service};

use pidash_auth::session::{
    cookie_value, generate_session_key, is_expired, is_plausible_session_key, render_delete_cookie,
    render_set_cookie, SetCookie,
};
use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};

use crate::license::{verify_session_hash, MODEL_BACKEND};

/// What went wrong talking to the session table. A load/save failure maps
/// to Django's 500 (`SessionInterrupted` on save races), never to a
/// session-state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session store failed: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// One `sessions` row as the store sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub session_data: String,
    pub expire_date_unix: i64,
}

/// The `users` columns Django's `ModelBackend.get_user` and session-hash
/// check read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUser {
    pub password: String,
    pub is_active: bool,
}

/// A row to persist: the signed payload plus the mirrored columns and the
/// absolute expiry (`sessions.expire_date`).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    /// `None` means "issue a fresh key", like Django saving a keyless store.
    pub key: Option<String>,
    pub session_data: String,
    pub expire_date_unix: i64,
    /// `_auth_user_id` when it is a string, else `None`
    /// (`create_model_instance`).
    pub user_id: Option<String>,
    /// `device_info` when it is a dict, else `None`.
    pub device_info: Option<serde_json::Value>,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send>>;

/// Session persistence. Object-safe so `build_app` can hold whichever
/// implementation the deployment wires (`PgSessionStore` with pools, memory
/// in tests). Parameters are owned (`String`) so futures are `'static`.
pub trait SessionStore: Clone + Send + Sync + 'static {
    fn load(&self, key: String) -> BoxFuture<Option<StoredSession>>;
    /// Persist `row`; returns the key it is stored under (freshly issued
    /// when `row.key` is `None`, like `_get_new_session_key`). An existing
    /// key whose row is gone errors — Django's `UpdateError` on the
    /// `force_update` miss (`SessionInterrupted` → 500) — on the Postgres
    /// store; the memory store upserts (test double, like Django's
    /// non-db backends, which have no update-miss failure).
    fn save(&self, row: SessionRow) -> BoxFuture<String>;
    /// Load the user behind `_auth_user_id` for Django's `get_user` check.
    /// `None` means the user is missing or the id is not a UUID; Django
    /// treats both as anonymous without flushing. Stores without a users
    /// table keep the default (`None`).
    fn load_user(&self, user_id: String) -> BoxFuture<Option<SessionUser>> {
        let _ = user_id;
        Box::pin(async move { Ok(None) })
    }
    /// Delete one `sessions` row. Used only when Django's hash check calls
    /// `request.session.flush()`; the default keeps stores without rows
    /// usable.
    fn delete(&self, key: String) -> BoxFuture<()> {
        let _ = key;
        Box::pin(async move { Ok(()) })
    }
}

// ---------------------------------------------------------------------------
// In-memory store (tests and examples)
// ---------------------------------------------------------------------------

/// Test store: a mutex-guarded map. Key issuance loops until unused,
/// mirroring `_get_new_session_key`'s `while True`.
#[derive(Debug, Clone, Default)]
pub struct MemorySessionStore {
    rows: std::sync::Arc<std::sync::Mutex<BTreeMap<String, StoredSession>>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for MemorySessionStore {
    fn load(&self, key: String) -> BoxFuture<Option<StoredSession>> {
        let rows = self.rows.clone();
        Box::pin(async move { Ok(rows.lock().expect("memory store lock").get(&key).cloned()) })
    }

    fn save(&self, row: SessionRow) -> BoxFuture<String> {
        let rows = self.rows.clone();
        Box::pin(async move {
            let mut guard = rows.lock().expect("memory store lock");
            let key = match row.key.clone() {
                Some(key) => key,
                None => loop {
                    let fresh = generate_session_key();
                    if !guard.contains_key(&fresh) {
                        break fresh;
                    }
                },
            };
            guard.insert(
                key.clone(),
                StoredSession {
                    session_data: row.session_data,
                    expire_date_unix: row.expire_date_unix,
                },
            );
            Ok(key)
        })
    }

    fn load_user(&self, user_id: String) -> BoxFuture<Option<SessionUser>> {
        // The memory store models sessions, not the users table. Tests that
        // need missing/inactive users use a scripted store; this keeps the
        // existing session vectors authenticated as an active user with an
        // empty password field.
        let _ = user_id;
        Box::pin(async move {
            Ok(Some(SessionUser {
                password: String::new(),
                is_active: true,
            }))
        })
    }

    fn delete(&self, key: String) -> BoxFuture<()> {
        let rows = self.rows.clone();
        Box::pin(async move {
            rows.lock().expect("memory store lock").remove(&key);
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// Postgres store (the `sessions` table)
// ---------------------------------------------------------------------------

/// Postgres-backed store over the Django-owned `sessions` table. Django
/// stays schema owner; this only reads and writes rows with the exact
/// columns `pi_dash/db/models/session.py` declares.
#[derive(Debug, Clone)]
pub struct PgSessionStore {
    pool: sqlx::PgPool,
}

impl PgSessionStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// The row lookup the Django `SessionStore` issues.
    pub const LOAD_SQL: &'static str =
        "SELECT session_data, EXTRACT(EPOCH FROM expire_date)::BIGINT FROM sessions WHERE session_key = $1";

    /// The `get_user` probes behind [`SessionStore::load_user`].
    pub const USER_SQL: &'static str = "SELECT password, is_active FROM users WHERE id = $1";

    const DELETE_SQL: &'static str = "DELETE FROM sessions WHERE session_key = $1";

    const INSERT_SQL: &'static str =
        "INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info) \
         VALUES ($1, $2, to_timestamp($3::DOUBLE PRECISION), $4, $5) \
         ON CONFLICT (session_key) DO NOTHING";

    const UPDATE_SQL: &'static str =
        "UPDATE sessions SET session_data = $2, expire_date = to_timestamp($3::DOUBLE PRECISION), \
         user_id = $4, device_info = $5 WHERE session_key = $1";
}

impl SessionStore for PgSessionStore {
    fn load(&self, key: String) -> BoxFuture<Option<StoredSession>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let row: Option<(String, i64)> = sqlx::query_as(Self::LOAD_SQL)
                .bind(&key)
                .fetch_optional(&pool)
                .await
                .map_err(|e| StoreError(e.to_string()))?;
            Ok(row.map(|(session_data, expire_date_unix)| StoredSession {
                session_data,
                expire_date_unix,
            }))
        })
    }

    fn save(&self, row: SessionRow) -> BoxFuture<String> {
        let pool = self.pool.clone();
        Box::pin(async move {
            if let Some(key) = row.key.clone() {
                // Django's `save()` with an existing key is `force_update`:
                // the row must be there. A 0-row update means the session
                // vanished mid-request (concurrent flush) — `UpdateError`
                // (`SessionInterrupted` → 500) in Django, never a fresh
                // insert (PIDASHCONV-1304).
                let updated = sqlx::query(Self::UPDATE_SQL)
                    .bind(&key)
                    .bind(&row.session_data)
                    .bind(row.expire_date_unix as f64)
                    .bind(&row.user_id)
                    .bind(&row.device_info)
                    .execute(&pool)
                    .await
                    .map_err(|e| StoreError(e.to_string()))?
                    .rows_affected();
                if updated > 0 {
                    return Ok(key);
                }
                return Err(StoreError(
                    "session row missing for existing key (Django UpdateError)".to_string(),
                ));
            }
            insert_new(&pool, &row).await
        })
    }

    fn load_user(&self, user_id: String) -> BoxFuture<Option<SessionUser>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            // `users.id` is a UUID primary key. A non-UUID session value
            // is anonymous, like a missing row (`ModelBackend.get_user`).
            let Ok(id) = user_id.parse::<uuid::Uuid>() else {
                return Ok(None);
            };
            let row: Option<(String, bool)> = sqlx::query_as(Self::USER_SQL)
                .bind(id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| StoreError(e.to_string()))?;
            Ok(row.map(|(password, is_active)| SessionUser {
                password,
                is_active,
            }))
        })
    }

    fn delete(&self, key: String) -> BoxFuture<()> {
        let pool = self.pool.clone();
        Box::pin(async move {
            sqlx::query(Self::DELETE_SQL)
                .bind(key)
                .execute(&pool)
                .await
                .map_err(|e| StoreError(e.to_string()))?;
            Ok(())
        })
    }
}

async fn insert_new(pool: &sqlx::PgPool, row: &SessionRow) -> Result<String, StoreError> {
    // Fresh keys loop until unused, like `_get_new_session_key`.
    loop {
        let key = generate_session_key();
        let inserted = sqlx::query(PgSessionStore::INSERT_SQL)
            .bind(&key)
            .bind(&row.session_data)
            .bind(row.expire_date_unix as f64)
            .bind(&row.user_id)
            .bind(&row.device_info)
            .execute(pool)
            .await
            .map_err(|e| StoreError(e.to_string()))?
            .rows_affected();
        if inserted > 0 {
            return Ok(key);
        }
    }
}

// ---------------------------------------------------------------------------
// RequestSession: the `request.session` equivalent in extensions
// ---------------------------------------------------------------------------

/// How long the saved session lives. Handlers set this; the default is
/// "the settings age", matching a store that never calls
/// `set_expiry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionExpiry {
    /// `SESSION_COOKIE_AGE` (or the admin age on `instances` paths).
    #[default]
    Default,
    /// `get_expire_at_browser_close()`: no `Max-Age`/`expires` on the
    /// cookie. The row still gets an expiry (Django writes the age-based
    /// `expire_date`; only the cookie omits it).
    BrowserClose,
    /// `set_expiry(seconds)`: explicit age for both row and cookie.
    AgeSecs(i64),
}

/// Shared handle to the live session, stored in request extensions under
/// this type. Handlers lock it, read/write through [`RequestSession`], and
/// drop the guard; the layer holds its own clone and snapshots it after
/// the inner service responds. (Request extensions do not propagate to
/// response extensions, so the layer cannot read the session back off the
/// response — the shared handle is the whole channel.)
#[derive(Debug, Clone, Default)]
pub struct SessionHandle {
    inner: std::sync::Arc<std::sync::Mutex<RequestSession>>,
}

impl SessionHandle {
    pub fn new(session: RequestSession) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(session)),
        }
    }

    /// Lock the live session. Guards are never held across `.await`
    /// (neither the layer nor handlers await while locked), so a blocking
    /// mutex is correct here.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, RequestSession> {
        self.inner.lock().expect("session handle lock")
    }

    /// Clone the session out for the response rules.
    pub fn snapshot(&self) -> RequestSession {
        self.lock().clone()
    }
}

/// The live session. `accessed` flips on any read, `modified` on any
/// write — the two flags `process_response` consults.
#[derive(Debug, Clone, Default)]
pub struct RequestSession {
    /// Presented key that decoded to a live row. `None` for anonymous or
    /// invalid/expired sessions: the next save issues a fresh key.
    pub key: Option<String>,
    data: BTreeMap<String, serde_json::Value>,
    pub modified: bool,
    pub accessed: bool,
    pub expiry: SessionExpiry,
    /// Django returned `AnonymousUser` before the handler ran. Hide the
    /// `_auth_*` keys from readers without disturbing the stored session
    /// (the non-flush anonymous cases keep the row and cookie).
    auth_rejected: bool,
}

impl RequestSession {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Mirrors `SessionStore(session_key)` + decode: a live row becomes the
    /// full session dict, anything else an empty session with no key.
    pub fn load(
        key: String,
        stored: Option<StoredSession>,
        secret_key: &[u8],
        now_unix: i64,
    ) -> Self {
        let Some(stored) = stored else {
            return Self::empty();
        };
        if is_expired(stored.expire_date_unix, now_unix) {
            return Self::empty();
        }
        let signer = Signer::new(secret_key, SESSION_SIGNING_SALT);
        // The FULL decoded dict is kept: sessions carry arbitrary app keys
        // beyond the `_auth_*` trio, and a re-save must not drop them.
        // Corrupt rows decode to an empty session (Django logs and
        // continues anonymous); a non-dict payload is equally unusable.
        let data: BTreeMap<String, serde_json::Value> =
            match signer.unsign_object::<serde_json::Value>(&stored.session_data) {
                Ok(serde_json::Value::Object(map)) => map.into_iter().collect(),
                _ => return Self::empty(),
            };
        Self {
            key: Some(key),
            data,
            modified: false,
            accessed: false,
            expiry: SessionExpiry::Default,
            auth_rejected: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Read one value. Marks `accessed`, like every `SessionBase` getter.
    pub fn get(&mut self, key: &str) -> Option<&serde_json::Value> {
        self.accessed = true;
        if self.auth_rejected && is_auth_key(key) {
            return None;
        }
        self.data.get(key)
    }

    /// Write one value. Marks `modified` (and `accessed`), like every
    /// `SessionBase` setter.
    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.accessed = true;
        self.modified = true;
        // A login handler writes the auth trio after the pre-request
        // `get_user` check ran as anonymous; those fresh values are real.
        if is_auth_key(&key) {
            self.auth_rejected = false;
        }
        self.data.insert(key, value);
    }

    /// Remove one value. Marks `modified` only when something was there —
    /// `pop` on a missing key leaves the session untouched.
    pub fn remove(&mut self, key: &str) {
        if self.data.remove(key).is_some() {
            self.accessed = true;
            self.modified = true;
        }
    }

    /// Empty the session, like `flush()` + `clear()`: data gone, key
    /// dropped (the next save issues a fresh one), `modified` set so the
    /// delete-cookie branch can tell "was emptied" from "was always empty".
    pub fn clear(&mut self) {
        self.data.clear();
        self.key = None;
        self.accessed = true;
        self.modified = true;
        self.auth_rejected = false;
    }

    /// Django `get_user` returned anonymous without flushing: hide the
    /// bare session auth keys from handlers while leaving the row/cookie
    /// untouched.
    fn reject_auth(&mut self) {
        self.accessed = true;
        self.auth_rejected = true;
    }

    fn user_id(&self) -> Option<String> {
        if self.auth_rejected {
            return None;
        }
        self.data
            .get("_auth_user_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    fn backend(&self) -> Option<String> {
        self.data
            .get("_auth_user_backend")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    fn session_hash(&self) -> Option<String> {
        self.data
            .get("_auth_user_hash")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    fn device_info(&self) -> Option<serde_json::Value> {
        self.data
            .get("device_info")
            .filter(|v| v.is_object())
            .cloned()
    }

    /// The full session dict as JSON. The save path serializes this;
    /// the desktop gate reads its marker from a snapshot's copy (marking
    /// nothing — the snapshot is already detached from the live session).
    /// `auth_rejected` hides the trio from readers only: Django's
    /// non-flush anonymous cases never drop keys, so a later save
    /// persists them.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }
}

fn is_auth_key(key: &str) -> bool {
    matches!(
        key,
        "_auth_user_id" | "_auth_user_backend" | "_auth_user_hash"
    )
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

/// Session wiring. `store == None` means fully transparent (no pools yet);
/// see the module docs.
#[derive(Debug, Clone)]
pub struct SessionConfig<S> {
    pub store: Option<S>,
    pub secret_key: Vec<u8>,
    /// `SESSION_COOKIE_NAME` (default `session-id`).
    pub cookie_name: String,
    /// `ADMIN_SESSION_COOKIE_NAME` (`admin-session-id`).
    pub admin_cookie_name: String,
    /// `COOKIE_DOMAIN`, if set.
    pub cookie_domain: Option<String>,
    /// `SESSION_COOKIE_SECURE` (from `secure_origins`).
    pub cookie_secure: bool,
    /// `SESSION_COOKIE_AGE` (default 604800).
    pub cookie_age_secs: i64,
    /// `ADMIN_SESSION_COOKIE_AGE` (default 3600).
    pub admin_cookie_age_secs: i64,
    /// `SESSION_SAVE_EVERY_REQUEST`.
    pub save_every_request: bool,
}

impl<S> SessionConfig<S> {
    /// Build from the F-03 `Settings` overlay. The admin cookie name has no
    /// setting (it is hardcoded in `common.py`), so it comes from
    /// [`pidash_auth::session::ADMIN_SESSION_COOKIE_NAME`].
    pub fn from_settings(settings: &pidash_db::config::Settings, store: Option<S>) -> Self {
        Self {
            store,
            secret_key: settings.secret_key.clone().into_bytes(),
            cookie_name: settings.session.cookie_name.clone(),
            admin_cookie_name: pidash_auth::session::ADMIN_SESSION_COOKIE_NAME.to_string(),
            cookie_domain: settings.session.cookie_domain.clone(),
            cookie_secure: settings.session.cookie_secure,
            cookie_age_secs: settings.session.cookie_age_secs,
            admin_cookie_age_secs: settings.session.admin_cookie_age_secs,
            save_every_request: settings.session.save_every_request,
        }
    }

    /// Which cookie this path uses: paths containing `"instances"` read the
    /// admin cookie, the rest the ordinary one — the substring test is
    /// verbatim Python (`in request.path`), shared with
    /// [`pidash_auth::session::cookie_name_for_path`].
    pub fn cookie_name_for(&self, path: &str) -> &str {
        if path.contains("instances") {
            &self.admin_cookie_name
        } else {
            &self.cookie_name
        }
    }

    /// `(max_age, expires, row_expire_unix)` for a save, mirroring the
    /// `process_response` branch: browser-close sessions carry neither
    /// cookie attribute, but the row still gets the default age
    /// (`get_expiry_date` falls back to the age when no explicit expiry is
    /// set); the admin cookie uses its own age for the cookie only — the
    /// row always uses the session age unless `set_expiry` overrode it.
    pub fn cookie_lifetime(
        &self,
        cookie_name: &str,
        expiry: SessionExpiry,
        now_unix: i64,
    ) -> (Option<i64>, Option<String>, i64) {
        match expiry {
            SessionExpiry::BrowserClose => (None, None, now_unix + self.cookie_age_secs),
            SessionExpiry::AgeSecs(secs) => (
                Some(secs),
                Some(pidash_auth::session::http_date(now_unix + secs)),
                now_unix + secs,
            ),
            SessionExpiry::Default => {
                let age = if cookie_name == self.admin_cookie_name.as_str() {
                    self.admin_cookie_age_secs
                } else {
                    self.cookie_age_secs
                };
                (
                    Some(age),
                    Some(pidash_auth::session::http_date(now_unix + age)),
                    now_unix + self.cookie_age_secs,
                )
            }
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct SessionLayer<S> {
    config: SessionConfig<S>,
}

impl<S> SessionLayer<S> {
    pub fn new(config: SessionConfig<S>) -> Self {
        Self { config }
    }
}

impl<S, Store> Layer<S> for SessionLayer<Store>
where
    Store: Clone,
{
    type Service = SessionService<S, Store>;

    fn layer(&self, inner: S) -> Self::Service {
        SessionService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionService<S, Store> {
    inner: S,
    config: SessionConfig<Store>,
}

impl<S, Store> Service<Request<Body>> for SessionService<S, Store>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    Store: SessionStore,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let config = self.config.clone();
        let Some(store) = config.store.clone() else {
            // No pools yet: fully transparent, Django owns sessions.
            let mut inner = self.inner.clone();
            std::mem::swap(&mut self.inner, &mut inner);
            return Box::pin(async move { inner.call(req).await });
        };
        let path = req.uri().path().to_string();
        let cookie_name = config.cookie_name_for(&path).to_string();
        let cookie_header = req
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let presented = cookie_value(&cookie_header, &cookie_name);
        let cookie_present = presented.is_some();
        let secret_key = config.secret_key.clone();
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let mut session = match presented.filter(|k| is_plausible_session_key(k)) {
                Some(key) => {
                    let stored = match store.load(key.clone()).await {
                        Ok(stored) => stored,
                        Err(_) => return Ok(server_error()),
                    };
                    RequestSession::load(key, stored, &secret_key, now_unix())
                }
                None => RequestSession::empty(),
            };
            // Django `get_user`: every request resolves request.user before
            // routing. Do the same once here so handlers cannot trust a bare
            // `_auth_user_id`. A failed store probe is a 500 like any other
            // session-table failure.
            if let Some(user_id) = session.user_id() {
                if session.backend().as_deref() != Some(MODEL_BACKEND) {
                    // `get_user` skips the backend entirely when its path is
                    // not configured; the session row and cookie survive.
                    session.reject_auth();
                } else {
                    let user = match store.load_user(user_id).await {
                        Ok(user) => user,
                        Err(_) => return Ok(server_error()),
                    };
                    match user {
                        // `ModelBackend.get_user` returns None for a missing
                        // or inactive user; Django does not flush that case.
                        None => session.reject_auth(),
                        Some(user) if !user.is_active => session.reject_auth(),
                        Some(user) => {
                            let session_hash = session.session_hash().unwrap_or_default();
                            if !verify_session_hash(&session_hash, &user.password, &secret_key) {
                                // The one branch Django flushes: a loaded,
                                // active user whose session hash no longer
                                // matches the password field.
                                let key = session.key.clone();
                                session.clear();
                                if let Some(key) = key {
                                    if store.delete(key).await.is_err() {
                                        return Ok(server_error());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let handle = SessionHandle::new(session);
            req.extensions_mut().insert(handle.clone());
            let mut response = inner.call(req).await?;
            // Transparent when the responder already owns the session
            // decision: Django behind the proxy (marked
            // `ProxiedUpstream`), or an owned handler that set the session
            // cookie itself (email-style direct-SQL issuance,
            // `auth_session/email.rs`). Applying the pre-request
            // snapshot's rules on top would append a second, conflicting
            // `Set-Cookie` (PIDASHCONV-1304).
            if response
                .extensions()
                .get::<crate::edge::ProxiedUpstream>()
                .is_some()
                || response_has_session_cookie(&response, &cookie_name)
            {
                return Ok(response);
            }
            let session = handle.snapshot();
            if apply_response_rules(
                &config,
                &store,
                &cookie_name,
                cookie_present,
                session,
                &mut response,
            )
            .await
            .is_err()
            {
                return Ok(server_error());
            }
            Ok(response)
        })
    }
}

/// Empty-500 for store failures (`SessionInterrupted` → 500 in Django).
/// The shape stays minimal on purpose — the F-07 error renderer owns API
/// error bodies, and Django's own 500 page is deployment-specific.
fn server_error() -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::empty())
        .expect("static 500 response")
}

async fn apply_response_rules<Store: SessionStore>(
    config: &SessionConfig<Store>,
    store: &Store,
    cookie_name: &str,
    cookie_present: bool,
    session: RequestSession,
    response: &mut Response<Body>,
) -> Result<(), StoreError> {
    // Rule 1: cookie present but session empty → delete, any status.
    // The configured domain rides along, like `delete_cookie` receiving
    // `SESSION_COOKIE_DOMAIN`; without it a domain-scoped cookie survives.
    if cookie_present && session.is_empty() {
        push_set_cookie(
            response,
            &render_delete_cookie(cookie_name, "/", "Lax", config.cookie_domain.as_deref()),
        );
        crate::middleware::append_vary(response.headers_mut(), "Cookie");
        return Ok(());
    }
    // Rule 2: touched sessions vary on Cookie.
    if session.accessed {
        crate::middleware::append_vary(response.headers_mut(), "Cookie");
    }
    // Rule 3: save + Set-Cookie when modified (or always-save) and
    // non-empty and the response is not a 5xx.
    if !(session.modified || config.save_every_request)
        || session.is_empty()
        || response.status().as_u16() >= 500
    {
        return Ok(());
    }
    let now = now_unix();
    let (max_age, expires, expire_date_unix) =
        config.cookie_lifetime(cookie_name, session.expiry, now);
    // Keys serialize sorted (BTreeMap), not in Django insertion order.
    // Cross-implementation reads still hold: signatures cover the payload
    // string verbatim and JSON parsing is order-insensitive both ways.
    let signer = Signer::new(&config.secret_key, SESSION_SIGNING_SALT);
    let session_data = signer
        .sign_object(&session.to_json(), now.max(0) as u64)
        .map_err(|e| StoreError(e.to_string()))?;
    let row = SessionRow {
        key: session.key.clone(),
        session_data,
        expire_date_unix,
        user_id: session.user_id(),
        device_info: session.device_info(),
    };
    let key = store.save(row).await?;
    push_set_cookie(
        response,
        &render_set_cookie(&SetCookie {
            name: cookie_name.to_string(),
            value: key,
            expires,
            max_age,
            domain: config.cookie_domain.clone(),
            path: "/".to_string(),
            // `secure=... or None`: only present when truthy. HttpOnly is
            // unconditionally true in settings.
            secure: config.cookie_secure,
            httponly: true,
            samesite: "Lax".to_string(),
        }),
    );
    Ok(())
}

/// Does `response` already carry a `Set-Cookie` for the session cookie?
/// Owned session-managing handlers set it themselves, as does Django on
/// proxied answers; either way the session decision is made downstream.
fn response_has_session_cookie(response: &Response<Body>, cookie_name: &str) -> bool {
    let prefix = format!("{cookie_name}=");
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.starts_with(&prefix))
}

/// Append a `Set-Cookie` without disturbing existing ones (Django's
/// response can carry several; `insert` would clobber).
fn push_set_cookie(response: &mut Response<Body>, value: &str) {
    if let Ok(value) = header::HeaderValue::from_str(value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use pidash_auth::session::{ADMIN_SESSION_COOKIE_NAME, SESSION_COOKIE_NAME};
    use tower::{Layer, ServiceExt};

    const SECRET: &[u8] = b"f08-test-secret-key";

    fn config(store: Option<MemorySessionStore>) -> SessionConfig<MemorySessionStore> {
        config_for(store)
    }

    fn config_for<S>(store: Option<S>) -> SessionConfig<S> {
        SessionConfig {
            store,
            secret_key: SECRET.to_vec(),
            cookie_name: SESSION_COOKIE_NAME.to_string(),
            admin_cookie_name: ADMIN_SESSION_COOKIE_NAME.to_string(),
            cookie_domain: None,
            cookie_secure: false,
            cookie_age_secs: 604800,
            admin_cookie_age_secs: 3600,
            save_every_request: false,
        }
    }

    fn auth_hash() -> String {
        crate::license::make_session_auth_hash("", SECRET)
    }

    /// Seed one live row carrying the full Django auth trio, through any store.
    async fn seed_session_with<S: SessionStore>(
        store: &S,
        key: &str,
        user_id: &str,
        backend: &str,
        session_hash: &str,
    ) {
        use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};

        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let session_data = signer
            .sign_object(
                &serde_json::json!({
                    "_auth_user_id": user_id,
                    "_auth_user_backend": backend,
                    "_auth_user_hash": session_hash,
                }),
                4_000_000_000,
            )
            .expect("sign");
        store
            .save(SessionRow {
                key: Some(key.to_string()),
                session_data,
                expire_date_unix: 4_100_000_000,
                user_id: Some(user_id.to_string()),
                device_info: None,
            })
            .await
            .expect("seed");
    }

    async fn seed_session<S: SessionStore>(store: &S, key: &str, user_id: &str) {
        seed_session_with(store, key, user_id, MODEL_BACKEND, &auth_hash()).await;
    }

    /// Memory sessions plus a scripted `users` probe: `missing` ids report
    /// no row, `fail_probe` errors the probe (a 500 like any store failure).
    #[derive(Debug, Clone)]
    struct ProbeStore {
        inner: MemorySessionStore,
        missing: Option<String>,
        fail_probe: bool,
        inactive: bool,
        deleted: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl ProbeStore {
        fn with_users() -> Self {
            Self {
                inner: MemorySessionStore::new(),
                missing: None,
                fail_probe: false,
                inactive: false,
                deleted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }

        fn deleted_keys(&self) -> Vec<String> {
            self.deleted.lock().expect("deleted lock").clone()
        }
    }

    impl SessionStore for ProbeStore {
        fn load(&self, key: String) -> BoxFuture<Option<StoredSession>> {
            self.inner.load(key)
        }

        fn save(&self, row: SessionRow) -> BoxFuture<String> {
            self.inner.save(row)
        }

        fn load_user(&self, user_id: String) -> BoxFuture<Option<SessionUser>> {
            if self.fail_probe {
                return Box::pin(async move { Err(StoreError("probe down".to_string())) });
            }
            let missing = self.missing.clone();
            let inactive = self.inactive;
            Box::pin(async move {
                if missing.as_ref() == Some(&user_id) {
                    return Ok(None);
                }
                Ok(Some(SessionUser {
                    password: String::new(),
                    is_active: !inactive,
                }))
            })
        }

        fn delete(&self, key: String) -> BoxFuture<()> {
            let deleted = self.deleted.clone();
            Box::pin(async move {
                deleted.lock().expect("deleted lock").push(key);
                Ok(())
            })
        }
    }

    fn set_cookies(response: &Response<Body>) -> Vec<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_owned))
            .collect()
    }

    fn has_vary_cookie(response: &Response<Body>) -> bool {
        response
            .headers()
            .get_all(header::VARY)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(',').map(str::trim).map(str::to_owned))
            .any(|v| v.eq_ignore_ascii_case("Cookie"))
    }

    /// Session key out of a `name=key; ...` Set-Cookie value.
    fn cookie_key(set_cookie: &str, name: &str) -> String {
        let pair = set_cookie.split(';').next().unwrap_or("");
        let (key, value) = pair.split_once('=').unwrap_or(("", ""));
        assert_eq!(key, name);
        value.to_string()
    }

    /// Concrete inner handlers (see cors tests for why `Router`): named
    /// async fns, so every behavior below stays a concrete type.
    async fn plain_ok() -> &'static str {
        "ok"
    }

    fn mutate(req: &Request<Body>, user: &str) {
        let handle = req
            .extensions()
            .get::<SessionHandle>()
            .expect("session handle in extensions");
        let mut session = handle.lock();
        session.set(
            "_auth_user_id".to_string(),
            serde_json::Value::String(user.to_string()),
        );
        session.set(
            "_auth_user_backend".to_string(),
            serde_json::Value::String(MODEL_BACKEND.to_string()),
        );
        session.set(
            "_auth_user_hash".to_string(),
            serde_json::Value::String(auth_hash()),
        );
    }

    async fn set_user(req: Request<Body>) -> Response<Body> {
        mutate(&req, "user-1");
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn set_admin(req: Request<Body>) -> Response<Body> {
        mutate(&req, "admin-1");
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn set_user_500(req: Request<Body>) -> Response<Body> {
        mutate(&req, "user-1");
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .expect("inner")
    }

    async fn set_user_browser_close(req: Request<Body>) -> Response<Body> {
        let mut session = req
            .extensions()
            .get::<SessionHandle>()
            .expect("session")
            .lock();
        session.expiry = SessionExpiry::BrowserClose;
        session.set(
            "_auth_user_id".to_string(),
            serde_json::Value::String("user-1".to_string()),
        );
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn read_only(req: Request<Body>) -> Response<Body> {
        req.extensions()
            .get::<SessionHandle>()
            .expect("session")
            .lock()
            .get("_auth_user_id");
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn echo_user(req: Request<Body>) -> Response<Body> {
        let user = req
            .extensions()
            .get::<SessionHandle>()
            .and_then(|h| h.lock().get("_auth_user_id").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(user))
            .expect("inner")
    }

    /// Every handler's `actor_user_id` 401 arm in miniature: no
    /// `_auth_user_id` in the session means anonymous.
    async fn auth_gate(req: Request<Body>) -> Response<Body> {
        let authed = req
            .extensions()
            .get::<SessionHandle>()
            .and_then(|h| h.lock().get("_auth_user_id").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .is_some();
        if authed {
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("ok"))
                .expect("inner")
        } else {
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"detail":"Authentication credentials were not provided."}"#,
                ))
                .expect("inner")
        }
    }

    #[tokio::test]
    async fn transparent_without_store() {
        // No pools yet: even a presented cookie passes through untouched.
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(plain_ok));
        let response = SessionLayer::new(config(None))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=present-but-unreadable")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(set_cookies(&response).is_empty());
        assert!(!has_vary_cookie(&response));
    }

    #[tokio::test]
    async fn anonymous_without_cookie_changes_nothing() {
        let store = MemorySessionStore::new();
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(plain_ok));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::get("/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(set_cookies(&response).is_empty());
        assert!(!has_vary_cookie(&response));
    }

    #[tokio::test]
    async fn presented_but_empty_session_deletes_cookie() {
        let store = MemorySessionStore::new();
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(plain_ok));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=unknownkey")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // Rule 1: exact Django delete_cookie bytes, plus Vary: Cookie.
        assert_eq!(
            set_cookies(&response),
            vec!["session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax".to_string()]
        );
        assert!(has_vary_cookie(&response));
    }

    #[tokio::test]
    async fn delete_cookie_carries_configured_domain() {
        let store = MemorySessionStore::new();
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(plain_ok));
        let mut cfg = config(Some(store));
        cfg.cookie_domain = Some("example.com".to_string());
        let response = SessionLayer::new(cfg)
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=unknownkey")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // Like `delete_cookie(..., domain=SESSION_COOKIE_DOMAIN)`.
        assert_eq!(
            set_cookies(&response),
            vec!["session-id=\"\"; Domain=example.com; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax".to_string()]
        );
        assert!(has_vary_cookie(&response));
    }

    #[tokio::test]
    async fn modified_session_is_saved_and_round_trips() {
        let store = MemorySessionStore::new();
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(set_user));
        let response = SessionLayer::new(config(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        let set_cookie = &cookies[0];
        assert!(set_cookie.starts_with("session-id="), "{set_cookie}");
        assert!(set_cookie.contains("; Max-Age=604800"), "{set_cookie}");
        assert!(set_cookie.contains("; Path=/"), "{set_cookie}");
        assert!(set_cookie.contains("; SameSite=Lax"), "{set_cookie}");
        assert!(set_cookie.contains("; HttpOnly"), "{set_cookie}");
        assert!(!set_cookie.contains("Secure"), "{set_cookie}");
        assert!(set_cookie.contains("; expires="), "{set_cookie}");
        assert!(has_vary_cookie(&response));
        let key = cookie_key(set_cookie, "session-id");
        assert_eq!(key.len(), 128);

        // Round trip: the issued key loads the saved user on the next request.
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(echo_user));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, format!("session-id={key}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(&body[..], b"user-1");
    }

    #[tokio::test]
    async fn instances_paths_use_the_admin_cookie() {
        let store = MemorySessionStore::new();
        let inner =
            axum::Router::new().route("/api/instances/license/", axum::routing::any(set_admin));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::post("/api/instances/license/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        assert!(
            cookies[0].starts_with("admin-session-id="),
            "{}",
            cookies[0]
        );
        assert!(cookies[0].contains("; Max-Age=3600"), "{}", cookies[0]);
    }

    #[tokio::test]
    async fn no_save_on_server_error() {
        let store = MemorySessionStore::new();
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(set_user_500));
        // Rule 3's status gate: a 5xx never saves, even when modified.
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(set_cookies(&response).is_empty());
    }

    #[tokio::test]
    async fn browser_close_session_omits_cookie_expiry() {
        let store = MemorySessionStore::new();
        let inner =
            axum::Router::new().route("/api/x/", axum::routing::any(set_user_browser_close));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        assert!(!cookies[0].contains("Max-Age"), "{}", cookies[0]);
        assert!(!cookies[0].contains("expires"), "{}", cookies[0]);
    }

    #[tokio::test]
    async fn save_every_request_persists_unmodified_sessions() {
        use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};

        let store = MemorySessionStore::new();
        // Seed a live row: signed payload, expiry far in the future.
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let session_data = signer
            .sign_object(
                &serde_json::json!({"_auth_user_id": "user-2"}),
                4_000_000_000,
            )
            .expect("sign");
        store
            .save(SessionRow {
                key: Some("seededkey123".to_string()),
                session_data,
                expire_date_unix: 4_100_000_000,
                user_id: Some("user-2".to_string()),
                device_info: None,
            })
            .await
            .expect("seed");
        // Untouched session + flag on: Django re-saves and re-issues the cookie.
        let mut full = config(Some(store));
        full.save_every_request = true;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(plain_ok));
        let response = SessionLayer::new(full)
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=seededkey123")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        // Same key re-issued (Django saves under the existing key).
        assert_eq!(cookie_key(&cookies[0], "session-id"), "seededkey123");
    }

    #[tokio::test]
    async fn touched_but_unmodified_session_varies_without_saving() {
        use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};

        let store = MemorySessionStore::new();
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let session_data = signer
            .sign_object(
                &serde_json::json!({"_auth_user_id": "user-3"}),
                4_000_000_000,
            )
            .expect("sign");
        store
            .save(SessionRow {
                key: Some("touchedkey1".to_string()),
                session_data,
                expire_date_unix: 4_100_000_000,
                user_id: Some("user-3".to_string()),
                device_info: None,
            })
            .await
            .expect("seed");
        // Rule 2: a read marks accessed → Vary: Cookie, but nothing saves.
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(read_only));
        let response = SessionLayer::new(config(Some(store)))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=touchedkey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert!(has_vary_cookie(&response));
        assert!(set_cookies(&response).is_empty());
    }

    #[test]
    fn cookie_lifetimes_mirror_process_response() {
        let full = config(None);
        // Ordinary cookie: the session age; the row matches the cookie.
        let (max_age, expires, row_expire) =
            full.cookie_lifetime("session-id", SessionExpiry::Default, 1_000_000);
        assert_eq!(max_age, Some(604800));
        assert_eq!(expires.as_deref(), Some("Mon, 19 Jan 1970 13:46:40 GMT"));
        assert_eq!(row_expire, 1_000_000 + 604800);
        // Admin cookie on instances paths: its own cookie age, but the
        // row still uses the session age.
        let (max_age, _, row_expire) =
            full.cookie_lifetime("admin-session-id", SessionExpiry::Default, 1_000_000);
        assert_eq!(max_age, Some(3600));
        assert_eq!(row_expire, 1_000_000 + 604800);
        // Explicit override wins over both, cookie and row alike.
        let (max_age, _, row_expire) =
            full.cookie_lifetime("session-id", SessionExpiry::AgeSecs(99), 1_000_000);
        assert_eq!(max_age, Some(99));
        assert_eq!(row_expire, 1_000_000 + 99);
        // Browser-close: neither cookie attribute, default row expiry.
        let (max_age, expires, row_expire) =
            full.cookie_lifetime("session-id", SessionExpiry::BrowserClose, 1_000_000);
        assert_eq!(max_age, None);
        assert_eq!(expires, None);
        assert_eq!(row_expire, 1_000_000 + 604800);
        // Routing itself: the substring test, verbatim.
        assert_eq!(
            full.cookie_name_for("/api/instances/x/"),
            "admin-session-id"
        );
        assert_eq!(full.cookie_name_for("/api/auth/sign-in/"), "session-id");
    }

    #[test]
    fn load_keeps_arbitrary_session_keys() {
        use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};

        // A Django-issued row with app keys beyond the `_auth_*` trio.
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let session_data = signer
            .sign_object(
                &serde_json::json!({"_auth_user_id": "u1", "onboarding_step": 3, "oauth_state": "xyz"}),
                4_000_000_000,
            )
            .expect("sign");
        let session = RequestSession::load(
            "somekey".to_string(),
            Some(StoredSession {
                session_data,
                expire_date_unix: 4_100_000_000,
            }),
            SECRET,
            3_000_000_000,
        );
        assert!(!session.is_empty());
        assert_eq!(
            session.data.get("onboarding_step"),
            Some(&serde_json::Value::from(3))
        );
        assert_eq!(
            session.data.get("oauth_state").and_then(|v| v.as_str()),
            Some("xyz")
        );
    }

    #[test]
    fn pg_load_sql_matches_django_lookup() {
        // The exact lookup the Django SessionStore issues: row by primary
        // key, payload plus expiry. Pinned so schema drift shows up here.
        assert_eq!(
            PgSessionStore::LOAD_SQL,
            "SELECT session_data, EXTRACT(EPOCH FROM expire_date)::BIGINT FROM sessions WHERE session_key = $1"
        );
    }

    #[test]
    fn pg_user_sql_loads_password_and_active() {
        assert_eq!(
            PgSessionStore::USER_SQL,
            "SELECT password, is_active FROM users WHERE id = $1"
        );
    }

    #[tokio::test]
    async fn memory_store_models_an_active_user() {
        assert!(MemorySessionStore::new()
            .load_user("any-id".to_string())
            .await
            .expect("memory load")
            .is_some());
    }

    #[tokio::test]
    async fn missing_user_session_is_anonymous_without_flush() {
        let user_id = "11111111-1111-4111-8111-111111111111";
        let store = ProbeStore {
            inner: MemorySessionStore::new(),
            missing: Some(user_id.to_string()),
            fail_probe: false,
            inactive: false,
            deleted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        seed_session(&store, "deleteduserkey1", user_id).await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=deleteduserkey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        // Django `get_user` parity: anonymous 401 without a flush, so the
        // row and cookie survive.
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookies(&response).is_empty());
        assert!(has_vary_cookie(&response));
        assert!(store.deleted_keys().is_empty());
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(
            &body[..],
            b"{\"detail\":\"Authentication credentials were not provided.\"}"
        );
    }

    #[tokio::test]
    async fn existing_user_session_passes_through() {
        let user_id = "22222222-2222-4222-8222-222222222222";
        let store = ProbeStore::with_users();
        seed_session(&store, "liveuserkey1", user_id).await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store)))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=liveuserkey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(set_cookies(&response).is_empty());
    }

    #[tokio::test]
    async fn stale_session_hash_is_flushed() {
        let user_id = "44444444-4444-4444-8444-444444444444";
        let store = ProbeStore::with_users();
        seed_session_with(&store, "stalehashkey1", user_id, MODEL_BACKEND, "bogus").await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=stalehashkey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            set_cookies(&response),
            vec!["session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax".to_string()]
        );
        assert_eq!(store.deleted_keys(), vec!["stalehashkey1".to_string()]);
    }

    #[tokio::test]
    async fn non_model_backend_is_anonymous_without_flush() {
        let user_id = "55555555-5555-4555-8555-555555555555";
        let store = ProbeStore::with_users();
        seed_session_with(
            &store,
            "otherbackend1",
            user_id,
            "tests.OtherBackend",
            &auth_hash(),
        )
        .await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=otherbackend1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookies(&response).is_empty());
        assert!(store.deleted_keys().is_empty());
    }

    #[tokio::test]
    async fn inactive_user_is_anonymous_without_flush() {
        let user_id = "66666666-6666-4666-8666-666666666666";
        let store = ProbeStore {
            inactive: true,
            ..ProbeStore::with_users()
        };
        seed_session(&store, "inactivekey1", user_id).await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=inactivekey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookies(&response).is_empty());
        assert!(store.deleted_keys().is_empty());
    }

    #[tokio::test]
    async fn rejected_session_write_keeps_auth_keys() {
        // Django's non-flush anonymous cases leave the session dict alone:
        // a later save persists the auth trio, it does not drop it.
        async fn write_note(req: Request<Body>) -> Response<Body> {
            req.extensions()
                .get::<SessionHandle>()
                .expect("session handle in extensions")
                .lock()
                .set(
                    "note".to_string(),
                    serde_json::Value::String("x".to_string()),
                );
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::empty())
                .expect("inner")
        }
        let user_id = "77777777-7777-4777-8777-777777777777";
        let store = ProbeStore::with_users();
        seed_session_with(
            &store,
            "rejectedwrite1",
            user_id,
            "tests.OtherBackend",
            &auth_hash(),
        )
        .await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(write_note));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=rejectedwrite1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.deleted_keys().is_empty());
        let stored = store
            .inner
            .load("rejectedwrite1".to_string())
            .await
            .expect("load")
            .expect("row kept");
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let saved: serde_json::Value = signer.unsign_object(&stored.session_data).expect("unsign");
        assert_eq!(
            saved.get("_auth_user_id").and_then(|v| v.as_str()),
            Some(user_id)
        );
        assert_eq!(
            saved.get("_auth_user_backend").and_then(|v| v.as_str()),
            Some("tests.OtherBackend")
        );
        assert!(saved
            .get("_auth_user_hash")
            .and_then(|v| v.as_str())
            .is_some());
        assert_eq!(saved.get("note").and_then(|v| v.as_str()), Some("x"));
    }

    #[tokio::test]
    async fn user_probe_failure_is_a_500() {
        let user_id = "33333333-3333-4333-8333-333333333333";
        let store = ProbeStore {
            inner: MemorySessionStore::new(),
            missing: None,
            fail_probe: true,
            inactive: false,
            deleted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        seed_session(&store, "probedownkey1", user_id).await;
        let inner = axum::Router::new().route("/api/x/", axum::routing::any(auth_gate));
        let response = SessionLayer::new(config_for(Some(store)))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/api/x/")
                    .header(header::COOKIE, "session-id=probedownkey1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Save-counting store: proves the layer never persists for
    /// responses whose session decision is already made downstream.
    #[derive(Debug, Clone)]
    struct CountingStore {
        inner: MemorySessionStore,
        saves: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CountingStore {
        fn new() -> Self {
            Self {
                inner: MemorySessionStore::new(),
                saves: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }

        fn saves(&self) -> usize {
            self.saves.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl SessionStore for CountingStore {
        fn load(&self, key: String) -> BoxFuture<Option<StoredSession>> {
            self.inner.load(key)
        }

        fn save(&self, row: SessionRow) -> BoxFuture<String> {
            self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.save(row)
        }
    }

    fn always_save(store: CountingStore) -> SessionConfig<CountingStore> {
        let mut full = config_for(Some(store));
        full.save_every_request = true;
        full
    }

    /// Django's sign-out bytes in miniature: 302 + the flushed-session
    /// delete cookie + `Vary: Cookie`, marked as proxied upstream.
    async fn proxied_signout(_req: Request<Body>) -> Response<Body> {
        let mut response = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::VARY, "Cookie")
            .header(
                header::SET_COOKIE,
                "session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax",
            )
            .body(Body::empty())
            .expect("inner");
        response
            .extensions_mut()
            .insert(crate::edge::ProxiedUpstream);
        response
    }

    /// Django's sign-in bytes in miniature: 302 + the fresh session
    /// cookie, marked as proxied upstream.
    async fn proxied_signin(_req: Request<Body>) -> Response<Body> {
        let mut response = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::VARY, "Cookie")
            .header(
                header::SET_COOKIE,
                "session-id=newkey-from-django; expires=Thu, 29 Oct 2026 05:00:00 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax",
            )
            .body(Body::empty())
            .expect("inner");
        response
            .extensions_mut()
            .insert(crate::edge::ProxiedUpstream);
        response
    }

    /// An owned email-style sign-in in miniature: the handler issued
    /// the session itself (direct SQL) and set the cookie + `Vary`
    /// (`auth_session/email.rs` `redirect_response`); unmarked.
    async fn owned_signin(_req: Request<Body>) -> Response<Body> {
        Response::builder()
            .status(StatusCode::FOUND)
            .header(header::VARY, "Cookie")
            .header(
                header::SET_COOKIE,
                "session-id=newkey-from-handler; expires=Thu, 29 Oct 2026 05:00:00 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax",
            )
            .body(Body::empty())
            .expect("inner")
    }

    #[tokio::test]
    async fn proxied_signout_saves_nothing_and_keeps_djangos_cookie() {
        // Case B (PIDASHCONV-1304): save-every-request + a proxied
        // sign-out. The pre-request snapshot is a live session, so rule 3
        // would re-save (resurrecting the flushed row) and append a fresh
        // cookie after Django's delete. The marked response must pass
        // through untouched instead: no save, Django's bytes as-is.
        let store = CountingStore::new();
        seed_session(&store, "livekey-b123", "user-b").await;
        let seeded = store.saves();
        let inner =
            axum::Router::new().route("/auth/sign-out/", axum::routing::any(proxied_signout));
        let response = SessionLayer::new(always_save(store.clone()))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/auth/sign-out/")
                    .header(header::COOKIE, "session-id=livekey-b123")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::FOUND);
        // No save past the seed: the flushed row stays gone.
        assert_eq!(store.saves(), seeded);
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].contains("Max-Age=0"), "{}", cookies[0]);
        assert!(has_vary_cookie(&response));
    }

    #[tokio::test]
    async fn proxied_signin_keeps_djangos_cookie() {
        // Case A, proxied (PIDASHCONV-1304): a stale cookie (no row) +
        // Django's sign-in answer. Rule 1 would append a delete after
        // Django's new cookie; the marked response passes through.
        let store = CountingStore::new();
        let inner = axum::Router::new().route("/auth/sign-in/", axum::routing::any(proxied_signin));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/auth/sign-in/")
                    .header(header::COOKIE, "session-id=stale-no-such-row")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(store.saves(), 0);
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookie_key(&cookies[0], "session-id"), "newkey-from-django");
    }

    #[tokio::test]
    async fn owned_handler_cookie_disables_layer_rules() {
        // Case A, owned (PIDASHCONV-1304): email-style handlers issue the
        // session themselves and set the cookie (`auth_session/email.rs`
        // documents the layer as transparent there). A stale incoming
        // cookie must not make rule 1 append a delete after the
        // handler's cookie.
        let store = CountingStore::new();
        let inner = axum::Router::new().route("/auth/sign-in/", axum::routing::any(owned_signin));
        let response = SessionLayer::new(config_for(Some(store.clone())))
            .layer(inner)
            .oneshot(
                Request::builder()
                    .uri("/auth/sign-in/")
                    .header(header::COOKIE, "session-id=stale-no-such-row")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(store.saves(), 0);
        let cookies = set_cookies(&response);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookie_key(&cookies[0], "session-id"), "newkey-from-handler");
    }

    #[tokio::test]
    async fn pg_save_missing_key_fails_like_django_update_error() {
        // `PgSessionStore::save` with an existing key whose row vanished
        // is Django's `force_update` miss: `UpdateError`
        // (`SessionInterrupted` → 500), never a fresh insert
        // (PIDASHCONV-1304). Live-db, gated like the handoff tests.
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::PgPool::connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let store = PgSessionStore::new(pool.clone());
        let row = |key: Option<String>| SessionRow {
            key,
            session_data: "signed-data".to_string(),
            expire_date_unix: 4_100_000_000,
            user_id: None,
            device_info: None,
        };
        // Missing row under an existing key: error, and nothing inserted.
        let missing = format!("pidash1304-missing-{}", now_unix());
        let before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_key = $1")
                .bind(&missing)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(before, 0);
        assert!(store.save(row(Some(missing.clone()))).await.is_err());
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_key = $1")
            .bind(&missing)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(after, 0);
        // Positive control: keyless save inserts, keyed save updates.
        let key = store.save(row(None)).await.expect("insert");
        let again = store.save(row(Some(key.clone()))).await.expect("update");
        assert_eq!(again, key);
        sqlx::query("DELETE FROM sessions WHERE session_key = $1")
            .bind(&key)
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
