//! The public GitHub integration client (OVL-06, PIDASHCONV-1544).
//!
//! One live [`GithubClient`] transport plus the pool-backed
//! [`PrLinksStore`]: the two API surfaces (`app_issues` relations and
//! `v1_work_items` pr-links) carried byte-identical private twins of
//! both, and the overlay copied them a third time. This module is the
//! single canonical copy; the handlers are thin callers that keep only
//! their sentinel consts (for error mapping) and their call sites.
//!
//! Re-exported through `pidash-sdk::ops`.
//!
//! # The sync seam
//!
//! [`GithubClient`] is sync because Python's `requests` is blocking.
//! Async work (HTTP, Redis, app-config reads) runs through the private
//! [`run_sync`] helper: a fresh current-thread runtime on a spawned
//! thread, so the async executor stays unblocked. The ambient
//! [`TransportContext`] rides a thread-local installed by
//! [`TransportGuard`], because the seam has no parameter for it.
//!
//! # Sentinels
//!
//! [`PoolPrStore`] emits per-surface sentinel bytes (see
//! [`PrLinksSurface`]): each handler maps only its own prefix to
//! 404/400, so unifying on one string would flip the other surface to
//! 500.
//!
//! Fixture ids: the `pr_links` SQL consts and golden files next to
//! [`crate::app_issues::pr_links`]; the transport pins are the
//! `github_client.py` line refs on each method.

use std::collections::HashMap;

use serde_json::Value;
use uuid::Uuid;

use crate::app_issues::pr_links::{
    CreatePrLinkOutcome, InstallationRow, NewPrLink, NewReviewLink, PrLinkRow, PrLinksStore,
    ReviewLinkRow, ReviewMirrorFields, StoreError,
};
use crate::integrations::adapters_github::{ClientAuth, GithubClient, GithubError};
use pidash_db::app_integrations::queries_github::{build_app_jwt_claims, normalize_private_key};

/// Run an async future to completion from the sync `GithubClient` seam:
/// a fresh current-thread runtime on a spawned thread. The seam is sync
/// because Python's `requests` is blocking; this keeps the async
/// executor unblocked without new Cargo features.
fn run_sync<F>(future: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("github transport runtime")
            .block_on(future)
    })
    .join()
    .expect("github transport thread")
}

/// True when the failure is an integrity violation (SQLSTATE class 23).
///
/// Private copy of the api helpers (OVL-04 canonicalizes these; this
/// module duplicates the three lines to avoid conflicting with that
/// concurrently open unit).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")))
}

/// True when the failure is a unique violation (SQLSTATE 23505): the
/// `ignore_conflicts` / create-race arm. (Same OVL-04 note as
/// [`is_integrity_error`].)
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code == "23505"))
}

/// What the GitHub transport needs beyond the request: pool
/// (app-config reads), cache (installation-token cache), instance
/// secret (config decrypt + PAT keyring source).
#[derive(Clone)]
pub struct TransportContext {
    /// Pool for the `get_configuration_value` app-config reads.
    pub pool: sqlx::PgPool,
    /// Installation-token cache; `None` mints through (see
    /// [`installation_token`]).
    pub redis: Option<pidash_db::redis::RedisHandle>,
    /// Instance secret: the config-decrypt + PAT keyring source.
    pub secret: String,
}

/// Read app-config values through the instance-configurations
/// registry (`get_configuration_value`), like the GitHub App handler.
async fn app_config_values(
    ctx: &TransportContext,
    keys: &[&str],
) -> Option<HashMap<String, pidash_db::config::ConfigValue>> {
    let store = pidash_db::config::PgConfigStore::new(ctx.pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = crate::license::encryption::Keyring::from_secret(&ctx.secret);
    pidash_db::config::accessor::get_many_in(registry, &store, &keyring, keys)
        .await
        .ok()
}

/// `require_github_app_config()` (`github_app_auth.py:63-74`): app id +
/// slug + private key, each stripped, missing → the config error.
async fn github_app_config(
    ctx: &TransportContext,
) -> Result<(String, String, String), GithubError> {
    let values = app_config_values(
        ctx,
        &["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_PRIVATE_KEY"],
    )
    .await
    .unwrap_or_default();
    let string = |key: &str| match values.get(key) {
        Some(pidash_db::config::ConfigValue::Str(value)) => value.clone(),
        Some(pidash_db::config::ConfigValue::Int(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Float(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Bool(true)) => "True".to_owned(),
        Some(pidash_db::config::ConfigValue::Bool(false)) => "False".to_owned(),
        _ => String::new(),
    };
    let app_id = string("GITHUB_APP_ID");
    let app_slug = string("GITHUB_APP_SLUG");
    let private_key = string("GITHUB_APP_PRIVATE_KEY");
    let config = (
        app_id.trim().to_owned(),
        app_slug.trim().to_owned(),
        normalize_private_key(Some(&private_key)),
    );
    let mut missing: Vec<&str> = Vec::new();
    if config.0.is_empty() {
        missing.push("app_id");
    }
    if config.1.is_empty() {
        missing.push("app_slug");
    }
    if config.2.is_empty() {
        missing.push("private_key");
    }
    if !missing.is_empty() {
        return Err(GithubError::Transport(format!(
            "GitHub App config missing: {}",
            missing.join(", ")
        )));
    }
    Ok(config)
}

/// `build_app_jwt` (`github_app_auth.py:77-89`): RS256 over
/// `iat=now-60, exp=now+540, iss=app_id`.
fn build_app_jwt_token(app_id: &str, private_key: &str) -> Result<String, GithubError> {
    let now = chrono::Utc::now().timestamp();
    let claims = build_app_jwt_claims(now, app_id);
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))?;
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.typ = Some("JWT".to_owned());
    #[derive(serde::Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    jsonwebtoken::encode(
        &header,
        &Claims {
            iat: claims.iat,
            exp: claims.exp,
            iss: claims.iss,
        },
        &key,
    )
    .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))
}

/// `installation_token` (`github_app_auth.py:174-193`): cached token or
/// a fresh mint. Without a Redis handle the mint runs through (the cache
/// only saves the POST; a mint failure still fails the connect either
/// way).
async fn installation_token(
    ctx: &TransportContext,
    installation_id: i64,
) -> Result<String, GithubError> {
    let cache_key = format!("github_app_installation_token:{installation_id}");
    if let Some(redis) = ctx.redis.as_ref() {
        match redis.get_string(&cache_key).await {
            Ok(Some(token)) if !token.is_empty() => return Ok(token),
            Ok(_) => {}
            Err(error) => {
                return Err(GithubError::Transport(format!(
                    "installation token cache: {error}"
                )))
            }
        }
    }
    let (app_id, _slug, private_key) = github_app_config(ctx).await?;
    let jwt = build_app_jwt_token(&app_id, &private_key)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let response = client
        .post(format!(
            "https://api.github.com/app/installations/{installation_id}/access_tokens"
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "pi-dash-github-app")
        .send()
        .await
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(GithubError::Transport(format!(
            "installation token mint: HTTP {}: {text}",
            status.as_u16()
        )));
    }
    let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if token.is_empty() {
        return Err(GithubError::Transport(
            "GitHub did not return an installation token".to_owned(),
        ));
    }
    if let Some(redis) = ctx.redis.as_ref() {
        // `ttl = max(60, expires-now-60)`, default 55 minutes
        // (`github_app_auth.py:184-188`).
        let mut ttl: u64 = 55 * 60;
        if let Some(expires) = payload.get("expires_at").and_then(Value::as_str) {
            let normalized = expires.replace('Z', "+00:00");
            if let Ok(when) = chrono::DateTime::parse_from_rfc3339(&normalized) {
                let seconds =
                    (when.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds() - 60;
                ttl = u64::try_from(seconds.max(60)).unwrap_or(60);
            }
        }
        if let Err(error) = redis.set_ex(&cache_key, &token, ttl).await {
            return Err(GithubError::Transport(format!(
                "installation token cache: {error}"
            )));
        }
    }
    Ok(token)
}

/// The production `GithubClient` (`github_client.py:38-220`): a bearer
/// token plus the shared context for installation mints.
pub struct LiveGithubClient {
    token: String,
    ctx: TransportContext,
}

impl LiveGithubClient {
    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Authorization", format!("Bearer {}", self.token)),
            ("Accept", "application/vnd.github+json".to_owned()),
            ("X-GitHub-Api-Version", "2022-11-28".to_owned()),
            ("User-Agent", "pi-dash-github-sync".to_owned()),
        ]
    }

    async fn request_json(
        &self,
        method: reqwest::Method,
        url: &str,
        json: Option<&Value>,
    ) -> Result<(Value, Option<String>), GithubError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let mut request = client.request(method, url);
        for (name, value) in self.headers() {
            request = request.header(name, value);
        }
        if let Some(body) = json {
            let text = serde_json::to_string(body).unwrap_or_default();
            request = request
                .header("Content-Type", "application/json")
                .body(text);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let link = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let text = response.text().await.unwrap_or_default();
        // `_request` (`github_client.py:59-71`): 401/403/404 map by
        // status with the response text; anything else failing raises.
        if status == 401 {
            return Err(GithubError::Auth(text));
        }
        if status == 403 {
            return Err(GithubError::Permission(text));
        }
        if status == 404 {
            return Err(GithubError::NotFound(text));
        }
        if !(200..300).contains(&status) {
            return Err(GithubError::Transport(format!("HTTP {status}: {text}")));
        }
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((payload, link))
    }

    /// `_next_url`: the `rel="next"` link, if any.
    fn next_url(link: Option<&str>) -> Option<String> {
        let link = link?;
        for part in link.split(',') {
            let part = part.trim_start();
            if !part.starts_with('<') {
                continue;
            }
            let Some(end) = part.find('>') else {
                continue;
            };
            let url = &part[1..end];
            let rest = part[end + 1..].trim_start();
            if let Some(rest) = rest.strip_prefix(';') {
                let rest = rest.trim_start();
                if rest.starts_with("rel=\"next\"") {
                    return Some(url.to_owned());
                }
            }
        }
        None
    }

    async fn paginate(&self, mut url: String) -> Result<Vec<Value>, GithubError> {
        let mut items = Vec::new();
        loop {
            let (payload, link) = self.request_json(reqwest::Method::GET, &url, None).await?;
            match payload {
                Value::Array(page) => items.extend(page),
                Value::Null => {}
                // A mapping payload iterates its keys, like Python's
                // `for item in response.json()` (the API never sends
                // one here, but the iteration is the ported behavior).
                Value::Object(map) => {
                    items.extend(map.keys().map(|key| Value::String(key.clone())));
                }
                _ => {}
            }
            match Self::next_url(link.as_deref()) {
                Some(next) => url = next,
                None => return Ok(items),
            }
        }
    }
}

impl GithubClient for LiveGithubClient {
    fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
        // The context rides a thread-local: the sync seam has no
        // parameter for it, and `attach` always runs on the request
        // task that installed it just before calling into the port.
        let ctx = TRANSPORT_CONTEXT.with(|slot| slot.borrow().clone());
        let ctx =
            ctx.ok_or_else(|| GithubError::Transport("github transport context".to_owned()))?;
        match auth {
            ClientAuth::Token(token) => {
                if token.is_empty() {
                    return Err(GithubError::Auth("empty token".to_owned()));
                }
                Ok(Self {
                    token: token.clone(),
                    ctx,
                })
            }
            ClientAuth::Installation(id) => {
                let id = *id;
                let minted = ctx.clone();
                let token = run_sync(async move { installation_token(&minted, id).await })?;
                Ok(Self { token, ctx })
            }
        }
    }

    fn get_authenticated_user(&self) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            this.request_json(reqwest::Method::GET, "https://api.github.com/user", None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            let url = format!(
                "https://api.github.com/user/repos?affiliation=owner%2Ccollaborator%2Corganization_member&per_page=100&sort=updated&page={page}"
            );
            let (payload, link) = this.request_json(reqwest::Method::GET, &url, None).await?;
            let repos = match payload {
                Value::Array(repos) => repos,
                _ => Vec::new(),
            };
            Ok((repos, Self::next_url(link.as_deref()).is_some()))
        })
    }

    fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"state": "open", "per_page": 100, "sort": "updated",
        // "direction": "desc"}` in insertion order (`:136-140`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues?state=open&per_page=100&sort=updated&direction=desc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn list_issue_comments(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"per_page": 100, "sort": "created", "direction": "asc"}`
        // (`:149-153`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues/{number}/comments?per_page=100&sort=created&direction=asc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        number: i64,
        body: &str,
    ) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/issues/{number}/comments");
        let payload = serde_json::json!({"body": body});
        run_sync(async move {
            this.request_json(reqwest::Method::POST, &url, Some(&payload))
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn get_pull_request(&self, owner: &str, name: &str, number: i64) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/pulls/{number}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }
}

std::thread_local! {
    /// The ambient transport context for the sync `GithubClient` seam.
    static TRANSPORT_CONTEXT: std::cell::RefCell<Option<TransportContext>> =
        const { std::cell::RefCell::new(None) };
}

/// Install the ambient transport context around one sync-seam call.
/// The guard restores the previous value on drop.
pub struct TransportGuard {
    previous: Option<TransportContext>,
}

impl TransportGuard {
    /// Install `ctx` as the ambient context; the guard restores
    /// the previous value on drop.
    pub fn install(ctx: TransportContext) -> Self {
        let previous = TRANSPORT_CONTEXT.with(|slot| slot.borrow_mut().replace(ctx));
        Self { previous }
    }
}

impl Drop for TransportGuard {
    fn drop(&mut self) {
        TRANSPORT_CONTEXT.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

/// Map a sqlx failure into the `pr_links` store error.
fn pr_store_error(context: &str, error: sqlx::Error) -> StoreError {
    let _ = context;
    StoreError(format!("{context}: {error}"))
}

/// Which API surface a [`PoolPrStore`] speaks for.
///
/// The two surfaces chose different sentinel prefixes for the same two
/// store failures (`relations: …` on the app surface,
/// `pr-links: …` on the v1 surface); each handler maps only its own
/// prefix to 404/400, so the store must emit the caller's bytes
/// exactly. There is no default: every construction site names its
/// surface explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrLinksSurface {
    /// `app_issues/handlers_relations.rs` (`relations: …` sentinels).
    Relations,
    /// `v1_work_items/handlers_pr_links.rs` (`pr-links: …` sentinels).
    PrLinks,
}

impl PrLinksSurface {
    /// The sentinel prefix for this surface (`relations` / `pr-links`).
    fn tag(self) -> &'static str {
        match self {
            PrLinksSurface::Relations => "relations",
            PrLinksSurface::PrLinks => "pr-links",
        }
    }

    /// The deleted-project lookup miss: the handler maps it to the 404
    /// `DoesNotExist` body (Python's `self.project` fetch on `save()`).
    fn workspace_miss(self) -> String {
        format!("{}: project workspace not found", self.tag())
    }

    /// The mid-attach integrity-failure marker prefix (Python's uncaught
    /// `IntegrityError` → the payload 400).
    fn payload_invalid(self) -> String {
        format!("{}: payload not valid", self.tag())
    }
}

/// Map a mid-attach write failure: integrity violations carry the
/// payload marker (Python's `IntegrityError` 400), anything else the
/// generic store error (500).
fn pr_write_error(surface: PrLinksSurface, context: &str, error: sqlx::Error) -> StoreError {
    if is_integrity_error(&error) {
        StoreError(format!("{}: {context}: {error}", surface.payload_invalid()))
    } else {
        pr_store_error(context, error)
    }
}

/// The live `PrLinksStore`: pool + request actor, shared by both API
/// surfaces. Audit columns follow
/// CRUM (`BaseModel.save`): creates stamp `created_by`, updates stamp
/// `updated_by`, deletes stamp `updated_by`. The project workspace
/// resolves at write time (mirroring `save()`'s lazy project fetch),
/// deleted projects miss → the 404 sentinel.
pub struct PoolPrStore {
    pool: sqlx::PgPool,
    surface: PrLinksSurface,
}

impl PoolPrStore {
    /// Build for `surface`: the store emits that surface's sentinel
    /// bytes (see [`PrLinksSurface`]).
    pub fn new(pool: sqlx::PgPool, surface: PrLinksSurface) -> Self {
        Self { pool, surface }
    }

    async fn workspace_of(&self, project_id: &uuid::Uuid) -> Result<uuid::Uuid, StoreError> {
        // The public read service (OVL-05); deleted projects miss →
        // the 404 sentinel (Python's `self.project` traversal on
        // `save()`).
        crate::read::workspace_of(&self.pool, project_id)
            .await
            .map_err(|error| match error {
                crate::read::Error::NotFound => StoreError(self.surface.workspace_miss()),
                crate::read::Error::Db(err) => {
                    pr_store_error("project workspace", err.into_source())
                }
                // Unreachable: the lookup never fails mid-row.
                crate::read::Error::Failed(site) => {
                    StoreError(format!("project workspace: {site}"))
                }
            })
    }
}

/// Map a PR-link row: the v1 detach path reuses the store's mapper
/// for its pre-delete read.
pub fn pr_link_row_from(row: &sqlx::postgres::PgRow) -> Result<PrLinkRow, StoreError> {
    use sqlx::Row as _;
    Ok(PrLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_owner: row
            .try_get("repo_owner")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_name: row
            .try_get("repo_name")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_number: row
            .try_get::<i32, _>("pr_number")
            .map(i64::from)
            .map_err(|error| pr_store_error("pr link map", error))?,
        url: row
            .try_get("url")
            .map_err(|error| pr_store_error("pr link map", error))?,
        title: row
            .try_get("title")
            .map_err(|error| pr_store_error("pr link map", error))?,
        state: row
            .try_get("state")
            .map_err(|error| pr_store_error("pr link map", error))?,
        merged: row
            .try_get("merged")
            .map_err(|error| pr_store_error("pr link map", error))?,
        draft: row
            .try_get("draft")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_updated_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("pr_updated_at")
            .map(|stamp| {
                stamp.map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            })
            .map_err(|error| pr_store_error("pr link map", error))?,
    })
}

fn review_link_row_from(row: &sqlx::postgres::PgRow) -> Result<ReviewLinkRow, StoreError> {
    use sqlx::Row as _;
    Ok(ReviewLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("review link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("review link map", error))?,
        metadata: row
            .try_get("metadata")
            .map_err(|error| pr_store_error("review link map", error))?,
    })
}

impl PrLinksStore for PoolPrStore {
    async fn installation_for_account(
        &self,
        workspace_slug: &str,
        owner: &str,
    ) -> Result<Option<InstallationRow>, StoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(crate::app_issues::pr_links::INSTALLATION_LOOKUP_SQL)
                .bind(workspace_slug)
                .bind(owner)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("installation lookup", error))?;
        row.map(|row| {
            use sqlx::Row as _;
            let id: Uuid = row
                .try_get("id")
                .map_err(|error| pr_store_error("installation map", error))?;
            let installation_id: i64 = row
                .try_get("installation_id")
                .map_err(|error| pr_store_error("installation map", error))?;
            Ok(InstallationRow {
                id,
                installation_id,
            })
        })
        .transpose()
    }

    async fn issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, StoreError> {
        let row: Option<(i32,)> = sqlx::query_as(crate::app_issues::pr_links::ISSUE_EXISTS_SQL)
            .bind(issue_id)
            .bind(project_id)
            .bind(workspace_slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| pr_store_error("issue exists", error))?;
        Ok(row.is_some())
    }

    async fn pr_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Option<PrLinkRow>, StoreError> {
        let number_i32 = i32::try_from(number).map_err(|_| {
            StoreError(format!(
                "pr_number {number} out of range for integer column"
            ))
        })?;
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(crate::app_issues::pr_links::PR_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number_i32)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("pr link lookup", error))?;
        row.map(|row| pr_link_row_from(&row)).transpose()
    }

    async fn review_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: &str,
    ) -> Result<Option<ReviewLinkRow>, StoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(crate::app_issues::pr_links::REVIEW_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("review link lookup", error))?;
        row.map(|row| review_link_row_from(&row)).transpose()
    }

    async fn create_pr_link(
        &self,
        new: &NewPrLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<CreatePrLinkOutcome, StoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        let pr_number = i32::try_from(new.pr_number).map_err(|_| {
            StoreError(format!(
                "pr_number {} out of range for integer column",
                new.pr_number
            ))
        })?;
        let outcome = sqlx::query(crate::app_issues::pr_links::PR_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.repo_owner)
            .bind(&new.repo_name)
            .bind(pr_number)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.pr_updated_at.as_deref().and_then(parse_snapshot_dt))
            .execute(&self.pool)
            .await;
        match outcome {
            Ok(_) => Ok(CreatePrLinkOutcome::Created(Box::new(PrLinkRow {
                id: new.id,
                project_id: new.project_id,
                issue_id: new.issue_id,
                repo_owner: new.repo_owner.clone(),
                repo_name: new.repo_name.clone(),
                pr_number: new.pr_number,
                url: new.url.clone(),
                title: new.title.clone(),
                state: new.state.clone(),
                merged: new.merged,
                draft: new.draft,
                pr_updated_at: new.pr_updated_at.clone(),
            }))),
            Err(error) if is_unique_violation(&error) => Ok(CreatePrLinkOutcome::Conflict),
            Err(error) if is_integrity_error(&error) => Err(StoreError(format!(
                "{}: pr link insert: {error}",
                self.surface.payload_invalid()
            ))),
            Err(error) => Err(pr_store_error("pr link insert", error)),
        }
    }

    async fn create_review_link(
        &self,
        new: &NewReviewLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<ReviewLinkRow, StoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        sqlx::query(crate::app_issues::pr_links::REVIEW_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.remote_updated_at.as_deref().and_then(parse_snapshot_dt))
            .bind(&new.metadata)
            .bind(&new.namespace)
            .bind(&new.repo_name)
            .bind(&new.external_iid)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error(self.surface, "review link insert", error))?;
        Ok(ReviewLinkRow {
            id: new.id,
            issue_id: new.issue_id,
            metadata: new.metadata.clone(),
        })
    }

    async fn update_review_link(
        &self,
        id: Uuid,
        fields: &ReviewMirrorFields,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError> {
        let workspace_id = self.workspace_of(&fields.project_id).await?;
        sqlx::query(crate::app_issues::pr_links::REVIEW_LINK_UPDATE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(fields.project_id)
            .bind(workspace_id)
            .bind(fields.issue_id)
            .bind(&fields.url)
            .bind(&fields.title)
            .bind(&fields.state)
            .bind(fields.merged)
            .bind(fields.draft)
            .bind(
                fields
                    .remote_updated_at
                    .as_deref()
                    .and_then(parse_snapshot_dt),
            )
            .bind(&fields.metadata)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error(self.surface, "review link update", error))?;
        Ok(())
    }

    async fn delete_pr_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError> {
        sqlx::query(crate::app_issues::pr_links::PR_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("pr link delete", error))?;
        Ok(())
    }

    async fn delete_review_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError> {
        sqlx::query(crate::app_issues::pr_links::REVIEW_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("review link delete", error))?;
        Ok(())
    }
}

/// Parse a snapshot ISO datetime (`GithubAdapter.parse_dt` format) for a
/// timestamptz bind.
fn parse_snapshot_dt(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|aware| aware.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- github pagination ----------------------------------------------------

    #[test]
    fn next_url_parses_link_headers() {
        assert_eq!(
            LiveGithubClient::next_url(Some(
                r#"<https://api.github.com/x?page=2>; rel="next", <https://api.github.com/x?page=5>; rel="last""#
            ))
            .as_deref(),
            Some("https://api.github.com/x?page=2")
        );
        assert_eq!(
            LiveGithubClient::next_url(Some(r#"<https://x>; rel="last""#)),
            None
        );
        assert_eq!(LiveGithubClient::next_url(None), None);
    }

    #[test]
    fn surfaces_emit_their_own_sentinel_bytes() {
        assert_eq!(
            PrLinksSurface::Relations.workspace_miss(),
            "relations: project workspace not found"
        );
        assert_eq!(
            PrLinksSurface::PrLinks.workspace_miss(),
            "pr-links: project workspace not found"
        );
        assert_eq!(
            PrLinksSurface::Relations.payload_invalid(),
            "relations: payload not valid"
        );
        assert_eq!(
            PrLinksSurface::PrLinks.payload_invalid(),
            "pr-links: payload not valid"
        );
    }
}
