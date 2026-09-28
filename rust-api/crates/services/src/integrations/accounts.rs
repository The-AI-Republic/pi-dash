//! Git provider accounts (D-05, stage 5).
//!
//! Ports the account half of `apps/api/pi_dash/integrations/git/services.py`:
//! errors `GitIntegrationError` + 4 subclasses (29-46),
//! `normalize_host_url` (49-53), `account_credential` (56-60),
//! `_provider_account_queryset` (63-69), `create_provider_account`
//! (72-116), `select_provider_account` (118-138), `_account_preference`
//! (139-151), `resolve_provider_account_repository` (153-202).
//!
//! Layering: this crate holds no database pool (no new dependency enters
//! the lockfile), so queries cross the [`GitStore`] seam as typed rows
//! from `pidash-db`, and provider calls cross `&dyn GitProviderAdapter`
//! from `pidash-types`. Every SQL statement the seam executes lives here
//! as a `*_SQL` const so tests assert the queryset shape (workspace +
//! provider + normalized host, `status__in (connected, degraded)`,
//! `deleted_at IS NULL`, `ORDER BY created_at ASC`) without a live
//! database. The real pool implementation lands with the task layers
//! (PIDASHCONV-147/148); its `apply_*` methods must run inside one
//! transaction wherever Python holds `transaction.atomic()`.
//!
//! Writes take an explicit [`RequestContext`][pidash_db::RequestContext]
//! (Porting guide: handlers never hold an unscoped handle); the audit
//! columns come from `ctx.audit_actor()` (`None` stays NULL, mirroring
//! `getattr(actor, "id", None)` when the actor has no id).
//!
//! Fixtures replayed alongside: `account_credential.golden.json`,
//! `account_preference.golden.json`, `errors.golden.json`,
//! `normalize_host_url.golden.json` under
//! `rust-api/fixtures/integrations/services/`.
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * `normalize_host_url` checks the scheme case-sensitively: an
//!   uppercase `HTTP://…` host still gets `https://` prepended
//!   (`services.py:51` `startswith(("http://", "https://"))`).
//! * `str.strip()` vs `str::trim` differ on exotic Unicode whitespace;
//!   ASCII behavior (the reachable case) is identical.
//! * `account_credential` preserves insertion order (workspace
//!   `serde_json` unifies the `preserve_order` feature): stored keys
//!   first, then `auth_type`, then `host_url` — exactly the Python
//!   `dict`/`setdefault` order (`services.py:57-59`).
//! * `select_provider_account` with an explicit id scopes by the same
//!   workspace/provider/host/status queryset first: an id from another
//!   host raises `ProviderAccountNotFound`, exactly like
//!   `queryset.filter(id=…).first()` (`services.py:127`).
//! * `resolve_provider_account_repository` catches exactly the three
//!   provider subclasses (`Auth`, `Permission`, `NotFound`); a base
//!   `General` failure propagates without trying the next account,
//!   matching `except (Auth, Permission, NotFound)` (`services.py:187`).
//! * The single-account fast path propagates provider errors without
//!   retry or fallback (`services.py:178-180`).
//! * `create_provider_account` encrypts through the same falsy gate as
//!   Python `encrypt_data`: an empty token stores `""` without touching
//!   Fernet (`encryption.py:22,26-27`).

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::integrations::git_models::git_provider_account::{self, GitProviderAccount};
use pidash_db::RequestContext;
use pidash_types::integrations::{
    GitProviderAdapter, GitProviderCapabilities, GitProviderError, ParsedRepository,
    RemoteRepository, RepositoryPage,
};

/// Object-safe adapter seam for the services closure.
///
/// `GitProviderAdapter` (the 14-method `base.py:39-86` contract) is not
/// `dyn`-compatible (associated consts), and the services layer only
/// needs the seven members `services.py` touches: URL parsing is owned
/// by the repositories module through this same trait, while issue /
/// comment / webhook members stay on the full trait for the task layers
/// (PIDASHCONV-147/148). The blanket impl forwards every method, so the
/// real GitHub/GitLab adapters plug in unchanged; tests implement this
/// trait directly with scripted fakes.
pub trait ProviderAdapter {
    /// Registration key (`GitProviderAdapter::KEY`).
    fn key(&self) -> &'static str;
    /// Human name (`GitProviderAdapter::DISPLAY_NAME`).
    fn display_name(&self) -> &'static str;
    /// `parse_repo_url` (`base.py:44-45`).
    fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository>;
    /// `verify_provider_account` (`base.py:50-51`).
    fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError>;
    /// `credential_capabilities` (`base.py:53-54`).
    fn credential_capabilities(
        &self,
        credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError>;
    /// `list_repositories` (`base.py:56-57`; `page` passed explicitly).
    fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError>;
    /// `get_repository` (`base.py:59-60`).
    fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError>;
}

impl<T: GitProviderAdapter> ProviderAdapter for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn display_name(&self) -> &'static str {
        T::DISPLAY_NAME
    }

    fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
        GitProviderAdapter::parse_repo_url(self, url)
    }

    fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
        GitProviderAdapter::verify_provider_account(self, credential)
    }

    fn credential_capabilities(
        &self,
        credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError> {
        GitProviderAdapter::credential_capabilities(self, credential)
    }

    fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError> {
        GitProviderAdapter::list_repositories(self, credential, page)
    }

    fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError> {
        GitProviderAdapter::get_repository(self, credential, parsed)
    }
}

/// Service failure (`services.py:29-46`).
///
/// The base `GitIntegrationError` (`status_code = 400`) plus its four
/// subclasses. Each variant carries the message Python would put in
/// `Exception.args` (surfaced via `str(exc)`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitIntegrationError {
    /// Base failure (`GitIntegrationError` raised directly) → HTTP 400.
    #[error("{0}")]
    General(String),
    /// `UnsupportedRepositoryURL` → HTTP 400.
    #[error("{0}")]
    UnsupportedRepositoryUrl(String),
    /// `ProviderAccountRequired` → HTTP 409.
    #[error("{0}")]
    ProviderAccountRequired(String),
    /// `ProviderAccountAmbiguous` → HTTP 409.
    #[error("{0}")]
    ProviderAccountAmbiguous(String),
    /// `ProviderAccountNotFound` → HTTP 404.
    #[error("{0}")]
    ProviderAccountNotFound(String),
}

impl GitIntegrationError {
    /// HTTP status for this error (the `status_code` attributes).
    pub fn status_code(&self) -> u16 {
        match self {
            GitIntegrationError::General(_) | GitIntegrationError::UnsupportedRepositoryUrl(_) => {
                400
            }
            GitIntegrationError::ProviderAccountRequired(_)
            | GitIntegrationError::ProviderAccountAmbiguous(_) => 409,
            GitIntegrationError::ProviderAccountNotFound(_) => 404,
        }
    }

    /// The `str(exc)` message.
    pub fn message(&self) -> &str {
        match self {
            GitIntegrationError::General(msg)
            | GitIntegrationError::UnsupportedRepositoryUrl(msg)
            | GitIntegrationError::ProviderAccountRequired(msg)
            | GitIntegrationError::ProviderAccountAmbiguous(msg)
            | GitIntegrationError::ProviderAccountNotFound(msg) => msg,
        }
    }
}

/// Storage failure for the [`GitStore`] seam.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// A required row (workspace, project) is missing; the payload names
    /// it (`"workspace"`, `"project"`), mirroring `get_object_or_404`.
    #[error("not found: {0}")]
    NotFound(&'static str),
    /// Any database failure.
    #[error("database error: {0}")]
    Db(String),
}

/// Combined failure for the store-touching service functions.
///
/// Provider errors propagate unchanged (Python lets adapter exceptions
/// bubble out of `create`/`resolve`/`bind`).
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// A `GitIntegrationError` subclass was raised.
    #[error("{0}")]
    Integration(#[from] GitIntegrationError),
    /// The provider rejected the call.
    #[error("{0}")]
    Provider(#[from] GitProviderError),
    /// The store call failed.
    #[error("{0}")]
    Store(#[from] StoreError),
    /// `get_adapter` found no adapter for the parsed provider
    /// (`registry.py:21` `KeyError("Unsupported Git provider: …")`).
    #[error("{0}")]
    UnknownProvider(pidash_types::integrations::UnknownProvider),
}

/// Python truthiness over a JSON value.
///
/// `None`/missing, `null`, `false`, `0`, `""`, `[]`, `{}` are falsy;
/// everything else (including `true`, nonzero numbers, nonempty
/// strings/containers) is truthy. Mirrors the `or`-chains and `if`
/// guards throughout `services.py` (`external_id or …`, `if count …`,
/// `remote.default_branch and not project.base_branch`, …).
pub fn json_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

/// First truthy string in Python `or`-chain order.
///
/// Empty strings count as missing, exactly like `a or b or c`.
pub fn first_present<'a>(candidates: &[&'a str]) -> &'a str {
    candidates
        .iter()
        .find(|candidate| !candidate.is_empty())
        .copied()
        .unwrap_or("")
}

/// `normalize_host_url` (`services.py:49-53`).
///
/// Strips whitespace, strips trailing slashes, prepends `https://` when
/// no `http(s)://` scheme is present (case-sensitive, as in Python),
/// then strips trailing slashes again. `None` has no Rust spelling for
/// `&str`; callers pass `""` (mirroring `(host_url or "")`).
pub fn normalize_host_url(host_url: &str) -> String {
    let stripped = host_url.trim().trim_end_matches('/');
    if stripped.is_empty() {
        return String::new();
    }
    let with_scheme = if stripped.starts_with("http://") || stripped.starts_with("https://") {
        stripped.to_owned()
    } else {
        format!("https://{stripped}")
    };
    with_scheme.trim_end_matches('/').to_owned()
}

/// `account_credential` (`services.py:56-60`).
///
/// Copies `credential_config` (`dict(None or {})` makes a null config
/// safe) and fills `auth_type` then `host_url` with `setdefault`
/// semantics: stored keys are never overwritten.
pub fn account_credential(credential_config: &Value, auth_type: &str, host_url: &str) -> Value {
    let mut config = credential_config.as_object().cloned().unwrap_or_default();
    config
        .entry("auth_type")
        .or_insert_with(|| Value::String(auth_type.to_owned()));
    config
        .entry("host_url")
        .or_insert_with(|| Value::String(host_url.to_owned()));
    Value::Object(config)
}

/// Shared scope predicate for the provider-account queryset
/// (`services.py:63-69`): workspace + provider + normalized host,
/// `status__in [connected, degraded]`, live rows only.
pub const PROVIDER_ACCOUNT_SCOPE_SQL: &str = "workspace_id = $1 AND provider = $2 AND host_url = $3 AND status IN ('connected', 'degraded') AND deleted_at IS NULL";

/// `_provider_account_queryset` list shape (`services.py:63-69`).
///
/// Parameters: `$1` workspace id, `$2` provider key, `$3` normalized
/// host URL. `ORDER BY created_at ASC` serves
/// `resolve_provider_account_repository` (`.order_by("created_at")`,
/// `services.py:169-175`); the single-row `select` path narrows with
/// [`PROVIDER_ACCOUNT_GET_SQL`].
pub const PROVIDER_ACCOUNT_LIST_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, provider, host_url, auth_type, external_account_id, external_account_login, display_name, capabilities, credential_config, workspace_integration_id, status, verified_at, last_check_error, metadata FROM git_provider_accounts WHERE workspace_id = $1 AND provider = $2 AND host_url = $3 AND status IN ('connected', 'degraded') AND deleted_at IS NULL ORDER BY created_at ASC";

/// Scoped single-account fetch (`services.py:127`
/// `queryset.filter(id=…).first()`).
///
/// Parameters: `$1` workspace id, `$2` provider key, `$3` normalized
/// host, `$4` account id. Django's `first()` on an unordered queryset
/// orders by pk; the id predicate already yields at most one row.
pub const PROVIDER_ACCOUNT_GET_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, provider, host_url, auth_type, external_account_id, external_account_login, display_name, capabilities, credential_config, workspace_integration_id, status, verified_at, last_check_error, metadata FROM git_provider_accounts WHERE workspace_id = $1 AND provider = $2 AND host_url = $3 AND id = $4 AND status IN ('connected', 'degraded') AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `INSERT` for `create_provider_account` (`services.py:95-114`).
///
/// Parameters in column order: `$1` id, `$2` created_at, `$3`
/// updated_at, `$4` created_by id (nullable), `$5` updated_by id
/// (nullable), `$6` workspace id, `$7` provider, `$8` host_url,
/// `$9` auth_type, `$10` external_account_id, `$11`
/// external_account_login, `$12` display_name, `$13` capabilities
/// (jsonb), `$14` credential_config (jsonb), `$15` status, `$16`
/// verified_at, `$17` metadata (jsonb). `last_check_error` keeps its
/// application default (`""`); `deleted_at` stays NULL.
pub const PROVIDER_ACCOUNT_INSERT_SQL: &str = "INSERT INTO git_provider_accounts (id, created_at, updated_at, created_by_id, updated_by_id, workspace_id, provider, host_url, auth_type, external_account_id, external_account_login, display_name, capabilities, credential_config, status, verified_at, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

/// Storage seam for the services closure.
///
/// Methods mirror the Django ORM calls in `services.py`, one per query
/// shape; the SQL text for each lives in the adjacent `*_SQL` consts,
/// which the pool implementation must execute verbatim. Repositories and
/// bindings methods serve `repositories.rs` (same closure, one seam).
/// `apply_bind` must run inside a single transaction (Python holds
/// `transaction.atomic()` over the binding section, `services.py:324`).
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam.
#[allow(async_fn_in_trait)]
pub trait GitStore {
    /// `_provider_account_queryset` list (`PROVIDER_ACCOUNT_LIST_SQL`).
    /// `host_url` arrives normalized (`normalize_host_url`).
    async fn list_provider_accounts(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
    ) -> Result<Vec<GitProviderAccount>, StoreError>;

    /// Scoped single fetch (`PROVIDER_ACCOUNT_GET_SQL`).
    async fn get_provider_account(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
        account_id: Uuid,
    ) -> Result<Option<GitProviderAccount>, StoreError>;

    /// `GitProviderAccount.objects.create` (`PROVIDER_ACCOUNT_INSERT_SQL`).
    async fn insert_provider_account(
        &self,
        row: NewProviderAccount,
    ) -> Result<GitProviderAccount, StoreError>;

    /// `upsert_repository` lookup by external id
    /// (`REPOSITORY_FIND_BY_EXTERNAL_SQL`).
    async fn find_repository_by_external(
        &self,
        provider: &str,
        host_url: &str,
        external_id: &str,
    ) -> Result<
        Option<pidash_db::integrations::git_models::git_repository::GitRepository>,
        StoreError,
    >;

    /// `upsert_repository` lookup by full name
    /// (`REPOSITORY_FIND_BY_FULL_NAME_SQL`).
    async fn find_repository_by_full_name(
        &self,
        provider: &str,
        host_url: &str,
        full_name: &str,
    ) -> Result<
        Option<pidash_db::integrations::git_models::git_repository::GitRepository>,
        StoreError,
    >;

    /// `update_or_create` update branch (`REPOSITORY_UPDATE_SQL`).
    async fn update_repository(
        &self,
        id: Uuid,
        defaults: super::repositories::RepositoryDefaults,
        now: DateTime<Utc>,
    ) -> Result<pidash_db::integrations::git_models::git_repository::GitRepository, StoreError>;

    /// `update_or_create` create branch (`REPOSITORY_INSERT_SQL`).
    async fn insert_repository(
        &self,
        row: super::repositories::NewRepository,
    ) -> Result<pidash_db::integrations::git_models::git_repository::GitRepository, StoreError>;

    /// `get_object_or_404(Workspace, slug=…)` id lookup.
    async fn find_workspace_id(&self, slug: &str) -> Result<Option<Uuid>, StoreError>;

    /// `get_object_or_404(Project, pk=…, workspace=…)` row lookup.
    async fn find_project(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<super::repositories::ProjectRef>, StoreError>;

    /// The binding section of `bind_repository` as one transaction:
    /// delete the existing project binding (hard), delete the project's
    /// `GithubRepositorySync` rows (hard), insert the new binding, apply
    /// the project `repo_url`/`base_branch` updates. `BINDING_*_SQL`,
    /// `GITHUB_SYNC_DELETE_SQL`, `PROJECT_REPO_UPDATE_SQL`.
    async fn apply_bind(
        &self,
        plan: super::repositories::BindPlan,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        StoreError,
    >;

    /// `get_binding` with `select_related` (`BINDING_GET_SQL`).
    async fn get_binding(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<super::repositories::BindingView>, StoreError>;

    /// `set_binding_sync_enabled` binding update
    /// (`BINDING_SET_SYNC_SQL`).
    async fn set_binding_sync(
        &self,
        binding_id: Uuid,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        StoreError,
    >;

    /// `set_binding_sync_enabled` Github mirror
    /// (`GITHUB_SYNC_SET_ENABLED_SQL`); returns rows updated.
    async fn set_github_syncs_enabled(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
        enabled: bool,
    ) -> Result<u64, StoreError>;

    /// Hard-delete one binding by id (`BINDING_DELETE_ONE_SQL`).
    async fn delete_binding(&self, binding_id: Uuid) -> Result<(), StoreError>;

    /// Hard-delete a project's `GithubRepositorySync` rows
    /// (`GITHUB_SYNC_DELETE_SQL`); returns rows deleted.
    async fn delete_github_syncs_for_project(&self, project_id: Uuid) -> Result<u64, StoreError>;
}

/// Insert plan for `create_provider_account` (`services.py:95-114`).
///
/// Built by [`build_new_account`]; executed via
/// [`GitStore::insert_provider_account`] (`PROVIDER_ACCOUNT_INSERT_SQL`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewProviderAccount {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub provider: String,
    pub host_url: String,
    pub auth_type: String,
    pub external_account_id: String,
    pub external_account_login: String,
    pub display_name: String,
    pub capabilities: Value,
    pub credential_config: Value,
    pub status: String,
    pub verified_at: DateTime<Utc>,
    pub metadata: Value,
}

/// `create_provider_account` caller input (`services.py:72-80`).
pub struct CreateAccountRequest<'a> {
    pub workspace_id: Uuid,
    pub provider: &'a str,
    pub host_url: &'a str,
    pub auth_type: &'a str,
    pub token: &'a str,
}

/// Identity strings derived from `verify_provider_account`
/// (`services.py:92-94`).
///
/// `external_id` is the first present of `id`/`username`/`login`
/// (stringified: numbers render as-is, like `str(…)`); `login` is the
/// first present of `login`/`username`/`name`; `display_name` falls back
/// to `"{Display} account"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedIdentity {
    pub external_id: String,
    pub login: String,
    pub display_name: String,
}

/// One `identity.get(key)` under Python `or` semantics
/// (`services.py:92-94`).
///
/// Missing/`null`/falsy values (`""`, `0`, `false`) read as absent, so
/// the chain falls through exactly like `a or b or ""`; truthy values
/// render like `str(…)`. Arrays/objects read as absent — unreachable in
/// `verify_provider_account` payloads, where these keys are scalars.
fn identity_text(identity: &Value, key: &str) -> String {
    match identity.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(number)) => {
            let is_zero = number.as_i64().is_some_and(|int| int == 0)
                || number.as_u64().is_some_and(|int| int == 0)
                || number.as_f64().is_some_and(|float| float == 0.0);
            if is_zero {
                String::new()
            } else {
                number.to_string()
            }
        }
        Some(_) => String::new(),
    }
}

/// Derive the stored identity strings (`services.py:92-94`).
pub fn derive_identity(display_name_fallback: &str, identity: &Value) -> DerivedIdentity {
    let external_id = first_present(&[
        &identity_text(identity, "id"),
        &identity_text(identity, "username"),
        &identity_text(identity, "login"),
    ])
    .to_owned();
    let login = first_present(&[
        &identity_text(identity, "login"),
        &identity_text(identity, "username"),
        &identity_text(identity, "name"),
    ])
    .to_owned();
    let display_name = if login.is_empty() {
        format!("{display_name_fallback} account")
    } else {
        login.clone()
    };
    DerivedIdentity {
        external_id,
        login,
        display_name,
    }
}

/// Capabilities dict for storage (`as_dict`, `dtos.py:55-62`).
///
/// Exactly the five keys in Python order; rendered here as a JSON
/// object (stored `jsonb`, order-insensitive).
pub fn capabilities_value(capabilities: &GitProviderCapabilities) -> Value {
    serde_json::json!({
        "read_repositories": capabilities.read_repositories,
        "read_issues": capabilities.read_issues,
        "write_comments": capabilities.write_comments,
        "manage_webhooks": capabilities.manage_webhooks,
        "clone": capabilities.clone,
    })
}

/// Build the insert row for `create_provider_account`
/// (`services.py:88-114`) without touching the store.
///
/// `identity` and `capabilities` are the adapter results
/// (`verify_provider_account`, `credential_capabilities().as_dict()`).
/// `actor_id` comes from the request context (`getattr(actor, "id",
/// None)`); unparseable ids stay NULL. An empty token stores `""`
/// without calling `encrypt_token` (the `encrypt_data` falsy gate).
pub fn build_new_account(
    request: &CreateAccountRequest<'_>,
    adapter_display_name: &str,
    identity: &Value,
    capabilities: &GitProviderCapabilities,
    encrypt_token: &dyn Fn(&str) -> String,
    actor_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> NewProviderAccount {
    let normalized_host = normalize_host_url(request.host_url);
    let derived = derive_identity(adapter_display_name, identity);
    let encrypted_token = if request.token.is_empty() {
        String::new()
    } else {
        encrypt_token(request.token)
    };
    NewProviderAccount {
        id: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        created_by_id: actor_id,
        updated_by_id: actor_id,
        workspace_id: request.workspace_id,
        provider: request.provider.to_owned(),
        host_url: normalized_host.clone(),
        auth_type: request.auth_type.to_owned(),
        external_account_id: derived.external_id,
        external_account_login: derived.login,
        display_name: derived.display_name,
        capabilities: capabilities_value(capabilities),
        credential_config: serde_json::json!({
            "auth_type": request.auth_type,
            "host_url": normalized_host,
            "token": encrypted_token,
        }),
        status: git_provider_account::STATUS_CONNECTED.to_owned(),
        verified_at: now,
        metadata: serde_json::json!({ "identity": identity }),
    }
}

/// Actor id from the request context, parsed to UUID.
///
/// Django user ids are UUIDs; a non-UUID actor (system writes) stays
/// NULL, mirroring `getattr(actor, "id", None)` → NULL column.
pub fn ctx_actor_id(ctx: &RequestContext) -> Option<Uuid> {
    ctx.audit_actor().and_then(|raw| raw.parse::<Uuid>().ok())
}

/// `create_provider_account` (`services.py:72-116`): verify, then
/// capabilities, then encrypt, then create — in that order.
pub async fn create_provider_account<S: GitStore>(
    store: &S,
    ctx: &RequestContext,
    adapter: &dyn ProviderAdapter,
    encrypt_token: &dyn Fn(&str) -> String,
    request: CreateAccountRequest<'_>,
    now: DateTime<Utc>,
) -> Result<GitProviderAccount, ServiceError> {
    let normalized_host = normalize_host_url(request.host_url);
    let credential = serde_json::json!({
        "auth_type": request.auth_type,
        "host_url": normalized_host,
        "token": request.token,
    });
    let identity = adapter.verify_provider_account(&credential)?;
    let capabilities = adapter.credential_capabilities(&credential)?;
    let row = build_new_account(
        &request,
        adapter.display_name(),
        &identity,
        &capabilities,
        encrypt_token,
        ctx_actor_id(ctx),
        now,
    );
    Ok(store.insert_provider_account(row).await?)
}

/// `select_provider_account` over an already-scoped account list
/// (`services.py:118-136`).
///
/// The list is the `_provider_account_queryset` result (workspace +
/// provider + normalized host + connected/degraded, live rows).
pub fn select_from_accounts(
    accounts: &[GitProviderAccount],
    provider_account_id: Option<&Uuid>,
) -> Result<GitProviderAccount, GitIntegrationError> {
    if let Some(id) = provider_account_id {
        return accounts
            .iter()
            .find(|account| &account.id == id)
            .cloned()
            .ok_or_else(|| {
                GitIntegrationError::ProviderAccountNotFound(
                    "Provider account not found for this repository host".to_owned(),
                )
            });
    }
    match accounts.len() {
        0 => Err(GitIntegrationError::ProviderAccountRequired(
            "Connect a provider account before binding this repository".to_owned(),
        )),
        1 => Ok(accounts[0].clone()),
        _ => Err(GitIntegrationError::ProviderAccountAmbiguous(
            "Multiple provider accounts can access this host; choose one".to_owned(),
        )),
    }
}

/// `select_provider_account` (`services.py:118-136`): scoped fetch,
/// then [`select_from_accounts`].
pub async fn select_provider_account<S: GitStore>(
    store: &S,
    workspace_id: Uuid,
    provider: &str,
    host_url: &str,
    provider_account_id: Option<&Uuid>,
) -> Result<GitProviderAccount, ServiceError> {
    let normalized_host = normalize_host_url(host_url);
    if let Some(id) = provider_account_id {
        return store
            .get_provider_account(workspace_id, provider, &normalized_host, *id)
            .await?
            .map(Ok)
            .unwrap_or_else(|| {
                Err(GitIntegrationError::ProviderAccountNotFound(
                    "Provider account not found for this repository host".to_owned(),
                )
                .into())
            });
    }
    let accounts = store
        .list_provider_accounts(workspace_id, provider, &normalized_host)
        .await?;
    Ok(select_from_accounts(&accounts, None)?)
}

/// `_account_preference` (`services.py:139-150`).
///
/// Rank 0-4 per the golden (`pat`+write … anything else); the second
/// element is the pre-rendered `str(account.created_at)` tiebreak, which
/// the caller supplies (Django renders `"YYYY-MM-DD HH:MM:SS+HH:MM"`).
pub fn account_preference(
    auth_type: &str,
    write_comments: bool,
    created_at_display: &str,
) -> (u8, String) {
    let rank = if auth_type == "pat" && write_comments {
        0
    } else if write_comments {
        1
    } else if auth_type == "pat" {
        2
    } else if auth_type == "github_app" {
        3
    } else {
        4
    };
    (rank, created_at_display.to_owned())
}

/// `write_comments` capability under Python truthiness
/// (`services.py:142,144`; `capabilities or {}` makes null safe).
pub fn account_write_comments(capabilities: &Value) -> bool {
    json_truthy(capabilities.get("write_comments"))
}

/// `resolve_provider_account_repository` (`services.py:153-200`).
///
/// With an explicit account id, delegates to [`select_provider_account`]
/// and propagates provider errors. Otherwise probes every scoped
/// account (oldest first — the store's `ORDER BY created_at ASC`),
/// collecting `(account, remote)` matches and swallowing exactly
/// `Auth`/`Permission`/`NotFound`; no match re-raises the last provider
/// error (or `ProviderAccountRequired` when nothing was ever raised);
/// the best rank wins, ties stay ambiguous unless exactly one account
/// holds it.
pub async fn resolve_provider_account_repository<S: GitStore>(
    store: &S,
    adapter: &dyn ProviderAdapter,
    workspace_id: Uuid,
    parsed: &ParsedRepository,
    provider_account_id: Option<&Uuid>,
) -> Result<(GitProviderAccount, RemoteRepository), ServiceError> {
    if let Some(id) = provider_account_id {
        let account = select_provider_account(
            store,
            workspace_id,
            &parsed.provider,
            &parsed.host_url,
            Some(id),
        )
        .await?;
        let remote = adapter.get_repository(
            &account_credential(
                &account.credential_config,
                &account.auth_type,
                &account.host_url,
            ),
            parsed,
        )?;
        return Ok((account, remote));
    }

    let accounts = store
        .list_provider_accounts(
            workspace_id,
            &parsed.provider,
            &normalize_host_url(&parsed.host_url),
        )
        .await?;
    if accounts.is_empty() {
        return Err(GitIntegrationError::ProviderAccountRequired(
            "Connect a provider account before binding this repository".to_owned(),
        )
        .into());
    }
    if accounts.len() == 1 {
        let account = &accounts[0];
        let remote = adapter.get_repository(
            &account_credential(
                &account.credential_config,
                &account.auth_type,
                &account.host_url,
            ),
            parsed,
        )?;
        return Ok((account.clone(), remote));
    }

    let mut matches: Vec<(GitProviderAccount, RemoteRepository)> = Vec::new();
    let mut last_provider_error: Option<GitProviderError> = None;
    for account in &accounts {
        let credential = account_credential(
            &account.credential_config,
            &account.auth_type,
            &account.host_url,
        );
        match adapter.get_repository(&credential, parsed) {
            Ok(remote) => matches.push((account.clone(), remote)),
            Err(error) if error.is_auth() || error.is_permission() || error.is_not_found() => {
                last_provider_error = Some(error);
            }
            Err(error) => return Err(error.into()),
        }
    }
    if matches.is_empty() {
        if let Some(error) = last_provider_error {
            return Err(error.into());
        }
        return Err(GitIntegrationError::ProviderAccountRequired(
            "Connect a provider account before binding this repository".to_owned(),
        )
        .into());
    }

    let best_rank = matches
        .iter()
        .map(|(account, _)| {
            account_preference(
                &account.auth_type,
                account_write_comments(&account.capabilities),
                "",
            )
            .0
        })
        .min()
        .expect("matches is nonempty");
    let mut best = matches.into_iter().filter(|(account, _)| {
        account_preference(
            &account.auth_type,
            account_write_comments(&account.capabilities),
            "",
        )
        .0 == best_rank
    });
    let first = best.next().expect("best is nonempty");
    if best.next().is_none() {
        return Ok(first);
    }
    Err(GitIntegrationError::ProviderAccountAmbiguous(
        "Multiple provider accounts can access this repository; choose one".to_owned(),
    )
    .into())
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use pidash_types::integrations::{GitProviderCapabilities, RepositoryPage};
    use std::sync::Mutex;

    fn golden(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/services/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    pub(crate) fn golden_file(name: &str) -> Value {
        golden(name)
    }

    /// Probe adapter mirroring `_RepositoryProbeAdapter` in
    /// `test_git_services.py`: only `visible_auth_types` resolve; the
    /// rest raise `NotFound`. Records every `get_repository` auth type
    /// in call order.
    pub struct ProbeAdapter {
        pub visible_auth_types: Vec<String>,
        pub calls: Mutex<Vec<String>>,
    }

    impl ProbeAdapter {
        pub fn new(visible: &[&str]) -> Self {
            Self {
                visible_auth_types: visible.iter().map(|text| text.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn probe_remote(
            &self,
            credential: &Value,
            parsed: &ParsedRepository,
        ) -> Result<RemoteRepository, GitProviderError> {
            let auth_type = credential
                .get("auth_type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            self.calls.lock().unwrap().push(auth_type.clone());
            if !self
                .visible_auth_types
                .iter()
                .any(|visible| visible == &auth_type)
            {
                return Err(GitProviderError::NotFound("repository not visible".into()));
            }
            Ok(RemoteRepository {
                provider: parsed.provider.clone(),
                external_id: "123".into(),
                namespace: parsed.namespace.clone(),
                name: parsed.name.clone(),
                full_name: parsed.full_name.clone(),
                web_url: format!("{}/{}", parsed.host_url, parsed.full_name),
                clone_url_http: format!("{}/{}.git", parsed.host_url, parsed.full_name),
                clone_url_ssh: String::new(),
                default_branch: "main".into(),
                is_private: true,
                metadata: Value::Object(Default::default()),
            })
        }
    }

    impl ProviderAdapter for ProbeAdapter {
        fn key(&self) -> &'static str {
            "github"
        }

        fn display_name(&self) -> &'static str {
            "GitHub"
        }

        fn parse_repo_url(&self, _url: &str) -> Option<ParsedRepository> {
            None
        }

        fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
            Ok(credential.clone())
        }

        fn credential_capabilities(
            &self,
            _credential: &Value,
        ) -> Result<GitProviderCapabilities, GitProviderError> {
            Ok(GitProviderCapabilities {
                read_repositories: true,
                read_issues: true,
                write_comments: true,
                manage_webhooks: false,
                clone: false,
            })
        }

        fn list_repositories(
            &self,
            _credential: &Value,
            page: i64,
        ) -> Result<RepositoryPage, GitProviderError> {
            Ok(RepositoryPage {
                repositories: Vec::new(),
                page,
                has_next_page: false,
            })
        }

        fn get_repository(
            &self,
            credential: &Value,
            parsed: &ParsedRepository,
        ) -> Result<RemoteRepository, GitProviderError> {
            self.probe_remote(credential, parsed)
        }
    }

    /// In-memory [`GitStore`]: scoped filtering mirrors
    /// `PROVIDER_ACCOUNT_LIST_SQL` (workspace + provider + host +
    /// connected/degraded + live), ordered by `created_at`.
    pub struct FakeStore {
        pub accounts: Mutex<Vec<GitProviderAccount>>,
    }

    impl FakeStore {
        pub fn new(accounts: Vec<GitProviderAccount>) -> Self {
            Self {
                accounts: Mutex::new(accounts),
            }
        }

        fn scoped(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
        ) -> Vec<GitProviderAccount> {
            let mut scoped: Vec<GitProviderAccount> = self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| {
                    account.workspace_id == workspace_id
                        && account.provider == provider
                        && account.host_url == host_url
                        && (account.status == git_provider_account::STATUS_CONNECTED
                            || account.status == git_provider_account::STATUS_DEGRADED)
                        && account.deleted_at.is_none()
                })
                .cloned()
                .collect();
            scoped.sort_by(|left, right| left.created_at.cmp(&right.created_at));
            scoped
        }
    }

    impl GitStore for FakeStore {
        async fn list_provider_accounts(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
        ) -> Result<Vec<GitProviderAccount>, StoreError> {
            Ok(self.scoped(workspace_id, provider, host_url))
        }

        async fn get_provider_account(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
            account_id: Uuid,
        ) -> Result<Option<GitProviderAccount>, StoreError> {
            Ok(self
                .scoped(workspace_id, provider, host_url)
                .into_iter()
                .find(|account| account.id == account_id))
        }

        async fn insert_provider_account(
            &self,
            row: NewProviderAccount,
        ) -> Result<GitProviderAccount, StoreError> {
            let account = GitProviderAccount {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                deleted_at: None,
                workspace_id: row.workspace_id,
                provider: row.provider,
                host_url: row.host_url,
                auth_type: row.auth_type,
                external_account_id: row.external_account_id,
                external_account_login: row.external_account_login,
                display_name: row.display_name,
                capabilities: row.capabilities,
                credential_config: row.credential_config,
                workspace_integration_id: None,
                status: row.status,
                verified_at: Some(row.verified_at),
                last_check_error: String::new(),
                metadata: row.metadata,
            };
            self.accounts.lock().unwrap().push(account.clone());
            Ok(account)
        }

        async fn find_repository_by_external(
            &self,
            _provider: &str,
            _host_url: &str,
            _external_id: &str,
        ) -> Result<
            Option<pidash_db::integrations::git_models::git_repository::GitRepository>,
            StoreError,
        > {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn find_repository_by_full_name(
            &self,
            _provider: &str,
            _host_url: &str,
            _full_name: &str,
        ) -> Result<
            Option<pidash_db::integrations::git_models::git_repository::GitRepository>,
            StoreError,
        > {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn update_repository(
            &self,
            _id: Uuid,
            _defaults: super::super::repositories::RepositoryDefaults,
            _now: DateTime<Utc>,
        ) -> Result<pidash_db::integrations::git_models::git_repository::GitRepository, StoreError>
        {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn insert_repository(
            &self,
            _row: super::super::repositories::NewRepository,
        ) -> Result<pidash_db::integrations::git_models::git_repository::GitRepository, StoreError>
        {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn find_workspace_id(&self, _slug: &str) -> Result<Option<Uuid>, StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn find_project(
            &self,
            _workspace_id: Uuid,
            _project_id: Uuid,
        ) -> Result<Option<super::super::repositories::ProjectRef>, StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn apply_bind(
            &self,
            _plan: super::super::repositories::BindPlan,
        ) -> Result<
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
            StoreError,
        > {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn get_binding(
            &self,
            _project_id: Uuid,
            _workspace_slug: &str,
        ) -> Result<Option<super::super::repositories::BindingView>, StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn set_binding_sync(
            &self,
            _binding_id: Uuid,
            _enabled: bool,
            _now: DateTime<Utc>,
        ) -> Result<
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
            StoreError,
        > {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn set_github_syncs_enabled(
            &self,
            _project_id: Uuid,
            _workspace_slug: &str,
            _enabled: bool,
        ) -> Result<u64, StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn delete_binding(&self, _binding_id: Uuid) -> Result<(), StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }

        async fn delete_github_syncs_for_project(
            &self,
            _project_id: Uuid,
        ) -> Result<u64, StoreError> {
            unimplemented!("repositories tests provide the full fake")
        }
    }

    pub fn workspace_id() -> Uuid {
        Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap()
    }

    pub fn stamp() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap()
    }

    /// `test_git_services.py::_account` equivalent: connected account
    /// with caller-chosen auth type / external id / write_comments.
    pub fn probe_account(
        workspace_id: Uuid,
        auth_type: &str,
        external_id: &str,
        write_comments: bool,
        created_at: DateTime<Utc>,
    ) -> GitProviderAccount {
        GitProviderAccount {
            id: Uuid::new_v4(),
            created_at,
            updated_at: created_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id,
            provider: "github".into(),
            host_url: "https://github.com".into(),
            auth_type: auth_type.into(),
            external_account_id: external_id.into(),
            external_account_login: external_id.into(),
            display_name: external_id.into(),
            capabilities: serde_json::json!({
                "read_repositories": true,
                "read_issues": true,
                "write_comments": write_comments,
                "manage_webhooks": auth_type == "github_app",
                "clone": false,
            }),
            credential_config: serde_json::json!({
                "auth_type": auth_type,
                "host_url": "https://github.com",
                "token": auth_type,
            }),
            workspace_integration_id: None,
            status: git_provider_account::STATUS_CONNECTED.into(),
            verified_at: Some(created_at),
            last_check_error: String::new(),
            metadata: serde_json::json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use pidash_types::integrations::GitProviderCapabilities;

    fn capabilities(write_comments: bool) -> GitProviderCapabilities {
        GitProviderCapabilities {
            read_repositories: true,
            read_issues: true,
            write_comments,
            manage_webhooks: false,
            clone: false,
        }
    }

    #[test]
    fn error_status_codes_replay_golden() {
        let gold = golden_file("errors.golden.json");
        assert_eq!(
            gold["GitIntegrationError"],
            serde_json::json!(GitIntegrationError::General(String::new()).status_code())
        );
        assert_eq!(
            gold["UnsupportedRepositoryURL"],
            serde_json::json!(
                GitIntegrationError::UnsupportedRepositoryUrl(String::new()).status_code()
            )
        );
        assert_eq!(
            gold["ProviderAccountRequired"],
            serde_json::json!(
                GitIntegrationError::ProviderAccountRequired(String::new()).status_code()
            )
        );
        assert_eq!(
            gold["ProviderAccountAmbiguous"],
            serde_json::json!(
                GitIntegrationError::ProviderAccountAmbiguous(String::new()).status_code()
            )
        );
        assert_eq!(
            gold["ProviderAccountNotFound"],
            serde_json::json!(
                GitIntegrationError::ProviderAccountNotFound(String::new()).status_code()
            )
        );
        assert_eq!(
            GitIntegrationError::ProviderAccountRequired("x".into()).message(),
            "x"
        );
        assert_eq!(
            GitIntegrationError::UnsupportedRepositoryUrl(
                "A supported GitHub or GitLab repository URL is required".into()
            )
            .to_string(),
            "A supported GitHub or GitLab repository URL is required"
        );
    }

    #[test]
    fn normalize_host_url_replays_golden() {
        let gold = golden_file("normalize_host_url.golden.json");
        let cases = gold["cases"].as_object().expect("cases is a map");
        let inputs = [
            ("bare_hostname", "gitlab.example.com"),
            ("empty_string", ""),
            ("http_scheme_kept", "http://gitlab.example.com"),
            ("none", ""),
            ("only_slashes", "///"),
            ("trailing_slashes_stripped", "https://gitlab.example.com///"),
            ("whitespace_and_slashes", "  https://gitlab.example.com//  "),
        ];
        for (key, input) in inputs {
            assert_eq!(
                normalize_host_url(input),
                cases[key].as_str().expect("golden output is a string"),
                "case {key}"
            );
        }
        // Ported case-sensitivity bug: an uppercase scheme still gets a
        // prefix (Python `startswith` is case-sensitive).
        assert_eq!(
            normalize_host_url("HTTP://gitlab.example.com"),
            "https://HTTP://gitlab.example.com"
        );
    }

    #[test]
    fn account_credential_replays_golden() {
        let gold = golden_file("account_credential.golden.json");
        for (key, case) in gold["cases"].as_object().expect("cases is a map") {
            let input = &case["input"];
            let rendered = account_credential(
                &input["credential_config"],
                input["auth_type"].as_str().unwrap(),
                input["host_url"].as_str().unwrap(),
            );
            // Value-equality: key order renders alphabetically in Rust
            // (documented approximation); semantics are exact.
            assert_eq!(rendered, case["output"], "case {key}");
        }
        // setdefault never overwrites stored keys.
        let stored = serde_json::json!({"auth_type": "oauth", "token": "t"});
        let rendered = account_credential(&stored, "pat", "https://github.com");
        assert_eq!(rendered["auth_type"], serde_json::json!("oauth"));
        assert_eq!(
            rendered["host_url"],
            serde_json::json!("https://github.com")
        );
        // Insertion order is Python's dict order byte for byte: stored
        // keys first, then auth_type, then host_url (services.py:57-59).
        let stored = serde_json::json!({"token": "t", "custom": 1});
        let rendered = account_credential(&stored, "pat", "https://github.com");
        assert_eq!(
            serde_json::to_string(&rendered).unwrap(),
            r#"{"token":"t","custom":1,"auth_type":"pat","host_url":"https://github.com"}"#
        );
    }

    #[test]
    fn queryset_sql_asserts_scope_shape() {
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains("FROM git_provider_accounts"));
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains(PROVIDER_ACCOUNT_SCOPE_SQL));
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains("status IN ('connected', 'degraded')"));
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains("deleted_at IS NULL"));
        assert!(PROVIDER_ACCOUNT_LIST_SQL.contains("ORDER BY created_at ASC"));
        assert!(PROVIDER_ACCOUNT_GET_SQL.contains("AND id = $4"));
        assert!(PROVIDER_ACCOUNT_GET_SQL.contains("LIMIT 1"));
        assert!(PROVIDER_ACCOUNT_INSERT_SQL.starts_with("INSERT INTO git_provider_accounts"));
    }

    #[test]
    fn account_preference_replays_golden_ranks() {
        let gold = golden_file("account_preference.golden.json");
        let ranks = &gold["ranks"];
        let cases = [
            ("pat_with_write", "pat", true, 0),
            ("pat_without_write", "pat", false, 2),
            ("other_auth_with_write", "oauth", true, 1),
            ("other_auth_without_write", "oauth", false, 4),
            ("github_app_without_write", "github_app", false, 3),
            ("null_capabilities", "oauth", false, 4),
        ];
        for (key, auth_type, write_comments, rank) in cases {
            assert_eq!(
                account_preference(auth_type, write_comments, "2024-05-01 12:00:00+00:00").0,
                rank,
                "case {key}"
            );
            assert_eq!(ranks[key], serde_json::json!(rank), "golden rank {key}");
        }
        // The tiebreak element is opaque to ranking (min over element 0).
        assert_eq!(account_preference("pat", true, "a"), (0, "a".to_owned()));
        // Null capabilities read as no-write (services.py:140 `or {}`).
        assert!(!account_write_comments(&Value::Null));
        assert!(account_write_comments(
            &serde_json::json!({"write_comments": true})
        ));
    }

    #[test]
    fn derive_identity_follows_or_chains() {
        let identity = serde_json::json!({"id": 42, "login": "octo", "name": "O"});
        let derived = derive_identity("GitHub", &identity);
        assert_eq!(derived.external_id, "42");
        assert_eq!(derived.login, "octo");
        assert_eq!(derived.display_name, "octo");

        let empty = serde_json::json!({});
        let derived = derive_identity("GitHub", &empty);
        assert_eq!(derived.external_id, "");
        assert_eq!(derived.display_name, "GitHub account");

        // Falsy scalars fall through the `or`-chain like in Python.
        let falsy = serde_json::json!({"id": 0, "username": false, "login": ""});
        let derived = derive_identity("GitHub", &falsy);
        assert_eq!(derived.external_id, "");
        assert_eq!(derived.login, "");
    }

    #[tokio::test]
    async fn create_builds_verify_then_capabilities_then_encrypt_then_create() {
        let workspace = workspace_id();
        let store = FakeStore::new(Vec::new());
        let adapter = ProbeAdapter::new(&["pat"]);
        let now = stamp();
        let request = CreateAccountRequest {
            workspace_id: workspace,
            provider: "github",
            host_url: "github.com/",
            auth_type: "pat",
            token: "secret",
        };
        let account = create_provider_account(
            &store,
            &ctx_for(None),
            &adapter,
            &|token| format!("enc({token})"),
            request,
            now,
        )
        .await
        .expect("creates");
        // Normalized host stored everywhere (services.py:82,95-109).
        assert_eq!(account.host_url, "https://github.com");
        assert_eq!(account.status, "connected");
        assert_eq!(account.verified_at, Some(now));
        assert_eq!(
            account.credential_config["token"],
            serde_json::json!("enc(secret)")
        );
        assert_eq!(
            account.credential_config["host_url"],
            serde_json::json!("https://github.com")
        );
        // Probe verify echoes the credential, so identity fields are empty
        // and display falls back to "{Display} account".
        assert_eq!(account.display_name, "GitHub account");
        assert_eq!(
            account.capabilities["write_comments"],
            serde_json::json!(true)
        );

        // Empty token short-circuits encryption (encrypt_data falsy gate).
        let request = CreateAccountRequest {
            workspace_id: workspace,
            provider: "github",
            host_url: "https://github.com",
            auth_type: "pat",
            token: "",
        };
        let account = create_provider_account(
            &store,
            &ctx_for(None),
            &adapter,
            &|_| panic!("must not encrypt an empty token"),
            request,
            now,
        )
        .await
        .expect("creates");
        assert_eq!(account.credential_config["token"], serde_json::json!(""));
    }

    #[tokio::test]
    async fn select_enforces_scoped_required_ambiguous_not_found() {
        let workspace = workspace_id();
        let first = probe_account(workspace, "pat", "a", true, stamp());
        let store = FakeStore::new(vec![first.clone()]);

        // Zero → Required.
        let empty = FakeStore::new(Vec::new());
        let error =
            select_provider_account(&empty, workspace, "github", "https://github.com", None)
                .await
                .expect_err("required");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::ProviderAccountRequired(_))
        ));

        // One → the account (host normalization applies to lookup).
        let found = select_provider_account(&store, workspace, "github", "github.com", None)
            .await
            .expect("selects");
        assert_eq!(found.id, first.id);

        // Explicit id outside the scope → NotFound.
        let error = select_provider_account(
            &store,
            workspace,
            "github",
            "https://github.com",
            Some(&Uuid::new_v4()),
        )
        .await
        .expect_err("not found");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::ProviderAccountNotFound(_))
        ));

        // Two → Ambiguous.
        let second = probe_account(workspace, "pat", "b", true, stamp());
        let store = FakeStore::new(vec![first, second]);
        let error =
            select_provider_account(&store, workspace, "github", "https://github.com", None)
                .await
                .expect_err("ambiguous");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::ProviderAccountAmbiguous(_))
        ));
    }

    #[tokio::test]
    async fn resolve_prefers_pat_with_write_comments() {
        // Mirrors test_bind_repository_prefers_pat_with_write_comments:
        // PAT+write ranks 0, app without write ranks 3; both visible.
        let workspace = workspace_id();
        let pat = probe_account(workspace, "pat", "pat", true, stamp());
        let app = probe_account(workspace, "github_app", "installation", false, stamp());
        let store = FakeStore::new(vec![pat.clone(), app]);
        let adapter = ProbeAdapter::new(&["pat", "github_app"]);
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        };
        let (account, remote) =
            resolve_provider_account_repository(&store, &adapter, workspace, &parsed, None)
                .await
                .expect("resolves");
        assert_eq!(account.id, pat.id);
        assert_eq!(remote.full_name, "acme/web");
        assert_eq!(adapter.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn resolve_uses_app_when_only_app_can_access() {
        let workspace = workspace_id();
        let pat = probe_account(workspace, "pat", "pat", true, stamp());
        let app = probe_account(workspace, "github_app", "installation", false, stamp());
        let store = FakeStore::new(vec![pat, app.clone()]);
        let adapter = ProbeAdapter::new(&["github_app"]);
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        };
        let (account, _) =
            resolve_provider_account_repository(&store, &adapter, workspace, &parsed, None)
                .await
                .expect("resolves");
        assert_eq!(account.id, app.id);
        assert_eq!(
            adapter.calls.lock().unwrap().as_slice(),
            &["pat".to_owned(), "github_app".to_owned()]
        );
    }

    #[tokio::test]
    async fn resolve_stays_ambiguous_on_tied_best_rank() {
        let workspace = workspace_id();
        let first = probe_account(workspace, "pat", "pat-1", true, stamp());
        let second = probe_account(workspace, "pat", "pat-2", true, stamp());
        let store = FakeStore::new(vec![first, second]);
        let adapter = ProbeAdapter::new(&["pat"]);
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        };
        let error = resolve_provider_account_repository(&store, &adapter, workspace, &parsed, None)
            .await
            .expect_err("ambiguous");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::ProviderAccountAmbiguous(_))
        ));
    }

    #[tokio::test]
    async fn resolve_honors_explicit_provider_account_id() {
        let workspace = workspace_id();
        let pat = probe_account(workspace, "pat", "pat", true, stamp());
        let app = probe_account(workspace, "github_app", "installation", false, stamp());
        let store = FakeStore::new(vec![pat, app.clone()]);
        let adapter = ProbeAdapter::new(&["pat", "github_app"]);
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        };
        let (account, _) = resolve_provider_account_repository(
            &store,
            &adapter,
            workspace,
            &parsed,
            Some(&app.id),
        )
        .await
        .expect("resolves");
        assert_eq!(account.id, app.id);
        assert_eq!(
            adapter.calls.lock().unwrap().as_slice(),
            &["github_app".to_owned()]
        );
    }

    #[tokio::test]
    async fn resolve_reraises_last_provider_error_when_nothing_visible() {
        let workspace = workspace_id();
        let pat = probe_account(workspace, "pat", "pat", true, stamp());
        let store = FakeStore::new(vec![pat]);
        // Single account, invisible: the fast path propagates the
        // provider error (services.py:178-180, no fallback).
        let adapter = ProbeAdapter::new(&[]);
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        };
        let error = resolve_provider_account_repository(&store, &adapter, workspace, &parsed, None)
            .await
            .expect_err("provider error");
        assert!(matches!(error, ServiceError::Provider(_)));

        // Two invisible accounts: the last provider error re-raises.
        let second = probe_account(workspace, "pat", "pat-2", true, stamp());
        let first = store.accounts.lock().unwrap()[0].clone();
        let store = FakeStore::new(vec![first, second]);
        let error = resolve_provider_account_repository(&store, &adapter, workspace, &parsed, None)
            .await
            .expect_err("provider error");
        assert!(matches!(error, ServiceError::Provider(_)));
    }

    fn ctx_for(actor: Option<Uuid>) -> RequestContext {
        use pidash_types::{UserId, WorkspaceId};
        let workspace = WorkspaceId::from("ws".to_owned());
        match actor {
            Some(id) => RequestContext::new(workspace, Some(UserId::from(id.to_string()))),
            None => RequestContext::new(workspace, None),
        }
    }
}
