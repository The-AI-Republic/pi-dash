//! OSS token writes: `api_tokens`, `machine_token`, `runner_session`
//! (ORM-08, PIDASHCONV-1520 part B).
//!
//! Every OSS write to the three token tables lives in this module; call
//! sites in `api`, `jobs` and the rest of `services` call these functions
//! instead of executing SQL text. Reads stay where they are (design D5).
//!
//! Two execution shapes, by writer kind:
//!
//! * Single-statement writers (middleware stamps, endpoint lifecycle,
//!   deactivations, shims, single-row session ops) are generic over
//!   `sqlx::Executor`: they run on the caller's pool or on the caller's
//!   existing `sqlx` transaction with no restructuring. Best-effort vs
//!   hard-error is the CALLER's choice (`let _ =` vs `?`), preserved
//!   per call site — Django `save()` calls propagate, the Rust
//!   middlewares deliberately swallow stamp failures, and both
//!   behaviors are pinned by contract suites.
//! * Transactional drivers (revoke, delete cascade, session open,
//!   rotate, enroll mint) move onto [`WriteTxn`][crate::domain::WriteTxn]
//!   in part B2; their SQL moves with them.
//!
//! Clocks are load-bearing and preserved per shape: middleware stamps use
//! the DB `now()`, flows holding a frozen timestamp bind it. Quoting is
//! consolidated to the quoted form (no fixture pins the stamp text).

use chrono::{DateTime, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// api_tokens
// ---------------------------------------------------------------------------

/// Middleware stamp (`api_authentication.py:41-44`):
/// `api_token.last_used = now; save(update_fields=["last_used"])`.
/// By presented token, DB clock. Best-effort at the middleware call
/// sites; the id-stamped twin is [`touch_api_token_last_used_by_id`].
const TOUCH_LAST_USED_SQL: &str =
    "UPDATE \"api_tokens\" SET \"last_used\" = now() WHERE \"token\" = $1";

/// By-id bound-clock twin of [`touch_api_token_last_used_by_token`]
/// (device, CLI and enroll validators resolve the id first and stamp
/// their frozen `now`).
const TOUCH_LAST_USED_BY_ID_SQL: &str =
    "UPDATE \"api_tokens\" SET \"last_used\" = $1 WHERE \"id\" = $2";

/// `BaseModel.save` second statement on token PATCH and soft-delete:
/// `updated_by` is stamped by the request actor after the main write.
const STAMP_UPDATED_BY_SQL: &str = "UPDATE api_tokens SET updated_by_id = $1 WHERE id = $2";

/// Device revoke endpoint (`device.py:496-511`): inline
/// `is_active=False` save by id (NOT the `cli_token` helper).
const DEACTIVATE_BY_ID_SQL: &str =
    "UPDATE api_tokens SET is_active = false, updated_at = $1 WHERE id = $2";

/// Touch `last_used` by presented token (middleware shape).
pub async fn touch_api_token_last_used_by_token<'e, E>(
    ex: E,
    token: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_LAST_USED_SQL)
        .bind(token)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Touch `last_used` by id with the caller's clock.
pub async fn touch_api_token_last_used_by_id<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_LAST_USED_BY_ID_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Deactivate by id with the caller's clock (device revoke shape).
pub async fn deactivate_api_token_by_id<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(DEACTIVATE_BY_ID_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Resolved inputs for [`issue_token`]: everything the endpoint
/// validation decides (label fallback, description default, bot
/// `user_type`, `expired_at` parsing). Id, token string and stamps are
/// generated inside, like the model layer.
pub struct NewApiToken {
    pub label: String,
    pub description: String,
    pub user_id: Uuid,
    pub user_type: i16,
    pub expired_at: Option<DateTime<Utc>>,
}

/// What [`issue_token`] generated: the row id, the plaintext token (only
/// visible at creation) and the two stamps.
pub struct IssuedApiToken {
    pub id: Uuid,
    pub token: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Issue a user API key (`views/api.py:20-35`):
/// `APIToken.objects.create(...)` with model defaults (`token` is
/// `generate_token()`, `is_active` TRUE, `is_service` FALSE,
/// `workspace_id` NULL, `allowed_rate_limit` `"60/min"`,
/// `created_by_id` from the request user, `updated_by_id` NULL).
/// `created`/`updated` are two separate `now()` calls, like the
/// endpoint's, so captured micros differ.
pub async fn issue_token<'e, E>(ex: E, params: &NewApiToken) -> Result<IssuedApiToken, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4();
    let token = format!("pi_dash_api_{}", Uuid::new_v4().simple());
    let created_at = Utc::now();
    let updated_at = Utc::now();
    let sql = bind_placeholders(api_token_insert_sql());
    sqlx::query(&sql)
        .bind(created_at)
        .bind(updated_at)
        .bind(params.user_id)
        .bind(id)
        .bind(&params.label)
        .bind(&params.description)
        .bind(&token)
        .bind(params.user_id)
        .bind(params.user_type)
        .bind(params.expired_at)
        .execute(ex)
        .await?;
    Ok(IssuedApiToken {
        id,
        token,
        created_at,
        updated_at,
    })
}

/// Resolved inputs for [`update_token`]: the client-effective writable
/// fields plus the actor for the `updated_by` stamp.
pub struct ApiTokenPatch {
    pub label: String,
    pub description: String,
    pub is_service: bool,
    pub allowed_rate_limit: String,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
}

/// Token PATCH save (`views/api.py:68-71`): the partial serializer's
/// full-row `UPDATE` plus the `updated_by` stamp. Two autocommit
/// statements, in this order, like the endpoint. Pool-scoped: the
/// endpoint never runs these in a transaction.
pub async fn update_token(
    pool: &sqlx::PgPool,
    pk: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
    patch: &ApiTokenPatch,
) -> Result<(), sqlx::Error> {
    let sql = bind_placeholders(api_token_patch_save_sql());
    sqlx::query(&sql)
        .bind(&patch.label)
        .bind(&patch.description)
        .bind(patch.is_service)
        .bind(&patch.allowed_rate_limit)
        .bind(patch.deleted_at)
        .bind(patch.created_by)
        .bind(now)
        .bind(pk)
        .execute(pool)
        .await?;
    sqlx::query(STAMP_UPDATED_BY_SQL)
        .bind(actor)
        .bind(pk)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Token soft-delete (`views/api.py:46-64`): the stamp-and-save (two
/// separate `now()` calls, like the endpoint) plus the `updated_by`
/// stamp. Two autocommit statements, in this order. Pool-scoped like
/// [`update_token`].
pub async fn soft_delete_token(
    pool: &sqlx::PgPool,
    pk: Uuid,
    actor: Uuid,
) -> Result<(), sqlx::Error> {
    let sql = bind_placeholders(api_token_soft_delete_sql());
    sqlx::query(&sql)
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(pk)
        .execute(pool)
        .await?;
    sqlx::query(STAMP_UPDATED_BY_SQL)
        .bind(actor)
        .bind(pk)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Resolved inputs for [`issue_device_token`]: the device mint's labels
/// and ids (the caller formats the label and generates id/token).
/// `workspace_id` is nullable: a device code may carry no workspace.
pub struct NewDeviceToken {
    pub id: Uuid,
    pub token: String,
    pub label: String,
    pub description: String,
    pub user_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub now: DateTime<Utc>,
}

/// Device-code-flow mint (`device.py:363-369`): the fixed-shape INSERT
/// (`is_active` TRUE, `user_type` 0, `is_service` FALSE,
/// `allowed_rate_limit` `"60/min"`). Runs on the caller's executor —
/// the device post-token handler mints inside its transaction.
pub async fn issue_device_token<'e, E>(ex: E, params: &NewDeviceToken) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        r#"INSERT INTO api_tokens
           (id, created_at, updated_at, token, label, description,
            is_active, user_type, user_id, workspace_id,
            is_service, allowed_rate_limit)
           VALUES ($1, $2, $3, $4, $5, $6, true, 0, $7, $8, false, '60/min')"#,
    )
    .bind(params.id)
    .bind(params.now)
    .bind(params.now)
    .bind(&params.token)
    .bind(&params.label)
    .bind(&params.description)
    .bind(params.user_id)
    .bind(params.workspace_id)
    .execute(ex)
    .await
    .map(|_| ())
}

/// Resolved inputs for [`insert_github_shim_token`]: the FK shim row is
/// never used for auth (`is_active` FALSE); the caller generates the
/// id, the label and the non-auth token value. `actor_id` fills both
/// `created_by_id` and `user_id`, like both call sites.
pub struct NewGithubShimToken {
    pub id: Uuid,
    pub label: String,
    pub token: String,
    pub actor_id: Uuid,
    pub workspace_id: Uuid,
    pub now: DateTime<Utc>,
}

/// GitHub integration FK shim (`github.py:114-128` / the workspace
/// webhook ensure): the inactive placeholder row the integration points
/// at. The app and workspace call sites pass their own label/token
/// formats; the column set is the union both used (`updated_by_id` /
/// `deleted_at` are explicit NULLs, identical to omitting them).
pub async fn insert_github_shim_token<'e, E>(
    ex: E,
    params: &NewGithubShimToken,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        r#"INSERT INTO api_tokens
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            label, description, is_active, last_used, token, user_id, user_type,
            workspace_id, expired_at, is_service, allowed_rate_limit)
           VALUES ($1,$2,$2,$3,NULL,NULL,$4,'GitHub integration FK shim — not for auth',false,NULL,$5,$3,1,$6,NULL,false,'60/min')"#,
    )
    .bind(params.id)
    .bind(params.now)
    .bind(params.actor_id)
    .bind(&params.label)
    .bind(&params.token)
    .bind(params.workspace_id)
    .execute(ex)
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Moved builders: endpoint lifecycle SQL (byte-identical; fixtures pin it)
// ---------------------------------------------------------------------------

/// Swap the builders' symbolic placeholders for positional binds, left
/// to right. `:now` takes a FRESH number per occurrence
/// (`auto_now_add` / `auto_now` / `delete()` are separate `now()`
/// calls); every other name reuses its first number.
/// Moved verbatim from the token handlers with the builders it serves.
fn bind_placeholders(fragment: String) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut numbers: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut next = 0usize;
    let bytes = fragment.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b':'
            && index + 1 < bytes.len()
            && (bytes[index + 1].is_ascii_alphabetic() || bytes[index + 1] == b'_')
        {
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let name = &fragment[index + 1..end];
            if name == "now" {
                next += 1;
                out.push_str(&format!("${next}"));
            } else {
                let number = *numbers.entry(name.to_owned()).or_insert_with(|| {
                    next += 1;
                    next
                });
                out.push_str(&format!("${number}"));
            }
            index = end;
        } else {
            out.push(byte as char);
            index += 1;
        }
    }
    out
}

/// `api_tokens` concrete columns in `_meta` order (abstract parents
/// first, then `id`, then `api.py:37-51` declared order): the
/// `INSERT` column set for [`api_token_insert_sql`].
/// Moved verbatim with the endpoint builders.
const API_TOKEN_INSERT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "label",
    "description",
    "is_active",
    "last_used",
    "token",
    "user_id",
    "user_type",
    "workspace_id",
    "expired_at",
    "is_service",
    "allowed_rate_limit",
];

/// Token create (`:29-35`):
/// `APIToken.objects.create(label=..., description=...,
/// user=..., user_type=..., expired_at=...)`.
///
/// Django fills: `token` via `generate_token()` (`api.py:20-21`),
/// `created_by_id` from the request user (`:31`, `None` when
/// anonymous), `is_active` TRUE (`api.py:41`), `is_service` FALSE
/// (`api.py:50-51`), `workspace_id` NULL (`api.py:49`,
/// `blank=True`), rate `"60/min"`, and stamps.
/// Moved verbatim with the endpoint executor.
fn api_token_insert_sql() -> String {
    format!(
        "INSERT INTO api_tokens ({}) VALUES (:now, :now, :actor, NULL, NULL, \
         :id, :label, :description, TRUE, NULL, :token, :user, :user_type, \
         NULL, :expired_at, FALSE, '60/min')",
        API_TOKEN_INSERT_COLUMNS.join(", ")
    )
}

/// Token delete (`:46-64`): guarded lookup then `token.delete()`
/// (`BaseModel.delete` stamps `deleted_at`, the concrete `save()`
/// refreshes `updated_at`).
/// Moved verbatim with the endpoint executor.
fn api_token_soft_delete_sql() -> String {
    "UPDATE api_tokens SET deleted_at = :now, updated_at = :now WHERE id = :pk".to_owned()
}

/// Token PATCH (`:68-71`): same guarded lookup, then the partial
/// serializer's full-row `UPDATE` (`SET` every writable attr,
/// `deleted_at` included).
/// Moved verbatim with the endpoint executor.
fn api_token_patch_save_sql() -> String {
    "UPDATE api_tokens SET label = :label, description = :description, \
     is_service = :is_service, allowed_rate_limit = :rate, \
     deleted_at = :deleted_at, created_by_id = :created_by, \
     updated_at = :now WHERE id = :pk"
        .to_owned()
}

/// `deactivate_api_token` (`services/cli_tokens.py:1-19`, full file),
/// moved verbatim from `pidash-db` with its executor and fixture
/// tests: bridge tokens are deactivated for API-key callers on
/// rotation/consume.
pub mod cli_token {
    /// `CLI_DEVICE_API_TOKEN_DESCRIPTION` (`cli_tokens.py:9`).
    pub(crate) const DEVICE_FLOW_DESCRIPTION: &str =
        pidash_db::auth_oauth::models::api_token_device_flow::DEVICE_FLOW_DESCRIPTION;

    /// `APIToken.objects.filter(token=$1, is_active=true).update(
    /// is_active=false, updated_at=$2)` — fixture F7 `emitted_sql` without
    /// the description guard.
    pub(crate) const DEACTIVATE_SQL: &str = "UPDATE \"api_tokens\" SET \"is_active\" = false, \"updated_at\" = $1 WHERE \"api_tokens\".\"token\" = $2 AND \"api_tokens\".\"is_active\" = true";

    /// Same with the CLI-device guard
    /// (`.filter(description=CLI_DEVICE_API_TOKEN_DESCRIPTION)`).
    pub(crate) const DEACTIVATE_CLI_ONLY_SQL: &str = "UPDATE \"api_tokens\" SET \"is_active\" = false, \"updated_at\" = $1 WHERE \"api_tokens\".\"token\" = $2 AND \"api_tokens\".\"is_active\" = true AND \"api_tokens\".\"description\" = $3";

    /// `if not raw_token: return 0` (`cli_tokens.py:14-15`): `None` or `""`
    /// short-circuits with no query.
    pub fn is_noop_token(raw_token: Option<&str>) -> bool {
        raw_token.unwrap_or("").is_empty()
    }

    /// Mark an `APIToken` inactive after its exchange for an `mt_` token
    /// (`cli_tokens.py:12-19`). Returns the updated row count (`0` when the
    /// token is empty/`None`, unmatched, inactive, or — with
    /// `only_cli_device_tokens` — not a device-flow token). The revoke
    /// endpoint does NOT use this helper (inline `is_active=False` save,
    /// `device.py:496-511`).
    pub async fn deactivate_api_token<'e, E>(
        ex: E,
        raw_token: Option<&str>,
        only_cli_device_tokens: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let token = raw_token.unwrap_or("");
        if token.is_empty() {
            return Ok(0);
        }
        let rows = if only_cli_device_tokens {
            sqlx::query(DEACTIVATE_CLI_ONLY_SQL)
                .bind(now)
                .bind(token)
                .bind(DEVICE_FLOW_DESCRIPTION)
                .execute(ex)
                .await?
                .rows_affected()
        } else {
            sqlx::query(DEACTIVATE_SQL)
                .bind(now)
                .bind(token)
                .execute(ex)
                .await?
                .rows_affected()
        };
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// machine_token
// ---------------------------------------------------------------------------

/// Middleware stamp (`api_authentication.py:60-64`):
/// `MachineToken.objects.filter(pk).update(last_used_at=now)`, by
/// presented hash, DB clock. Best-effort at the middleware call sites;
/// the by-id twins are [`touch_machine_token_last_used_by_id`] (bound
/// clock) and [`touch_machine_token_last_used_by_id_now`] (DB clock).
const TOUCH_MACHINE_LAST_USED_SQL: &str =
    "UPDATE \"machine_token\" SET \"last_used_at\" = now() WHERE \"token_hash\" = $1";

/// By-id DB-clock twin of [`touch_machine_token_last_used_by_hash`]
/// (validators that resolve the id first and stamp `now()`).
const TOUCH_MACHINE_LAST_USED_BY_ID_NOW_SQL: &str =
    "UPDATE \"machine_token\" SET \"last_used_at\" = now() WHERE \"id\" = $1";

/// By-id bound-clock twin (device, CLI, enroll and runner validators
/// stamp their frozen `now`).
const TOUCH_MACHINE_LAST_USED_BY_ID_SQL: &str =
    "UPDATE \"machine_token\" SET \"last_used_at\" = $1 WHERE \"id\" = $2";

/// Middleware revoke-then-deny (`api_authentication.py:54-58`):
/// `machine_token.revoke()` stamps `revoked_at` and the request is
/// denied. By presented hash, DB clock.
const REVOKE_MACHINE_BY_HASH_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = now() WHERE \"token_hash\" = $1";

/// By-id DB-clock twin of [`revoke_machine_token_by_hash`].
const REVOKE_MACHINE_BY_ID_NOW_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = now() WHERE \"id\" = $1";

/// By-id bound-clock revoke (moved verbatim from the enroll column
/// consts): validators stamp their frozen `now`.
const REVOKE_MACHINE_BY_ID_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"id\" = $2";

/// Conditional by-id revoke (device rotate path): only a live row
/// flips, so a double rotate keeps the first `revoked_at`.
const REVOKE_MACHINE_BY_ID_IF_ACTIVE_SQL: &str =
    "UPDATE machine_token SET revoked_at = $1 WHERE id = $2 AND revoked_at IS NULL";

/// Touch `last_used_at` by presented hash (middleware shape).
pub async fn touch_machine_token_last_used_by_hash<'e, E>(
    ex: E,
    token_hash: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_MACHINE_LAST_USED_SQL)
        .bind(token_hash)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Touch `last_used_at` by id with DB `now()`.
pub async fn touch_machine_token_last_used_by_id_now<'e, E>(
    ex: E,
    id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_MACHINE_LAST_USED_BY_ID_NOW_SQL)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Touch `last_used_at` by id with the caller's clock.
pub async fn touch_machine_token_last_used_by_id<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_MACHINE_LAST_USED_BY_ID_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke-then-deny by presented hash (middleware shape).
pub async fn revoke_machine_token_by_hash<'e, E>(ex: E, token_hash: &str) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_MACHINE_BY_HASH_SQL)
        .bind(token_hash)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke by id with DB `now()`.
pub async fn revoke_machine_token_by_id_now<'e, E>(ex: E, id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_MACHINE_BY_ID_NOW_SQL)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke by id with the caller's clock.
pub async fn revoke_machine_token_by_id<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_MACHINE_BY_ID_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke by id with the caller's clock, only when still live.
pub async fn revoke_machine_token_by_id_if_active<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_MACHINE_BY_ID_IF_ACTIVE_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// runner_session
// ---------------------------------------------------------------------------

/// Single-session revoke (`sessions.py:307-309` /
/// `pubsub.py:96-104`): unconditional stamp by id, moved verbatim
/// from the session model consts.
const REVOKE_SESSION_SQL: &str =
    "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"runner_session\".\"id\" = $3";

/// Session heartbeat (`sessions.py`, `last_seen_at` stamp on every
/// poll), moved verbatim from the session model consts.
const TOUCH_SESSION_SQL: &str =
    "UPDATE \"runner_session\" SET \"last_seen_at\" = $1 WHERE \"runner_session\".\"id\" = $2";

/// Revoke one session by id.
pub async fn revoke_runner_session<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_SESSION_SQL)
        .bind(now)
        .bind(reason)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke active sessions for one runner (enroll revoke S2,
/// `:618-620`). `$1` = now, `$2` = stored reason, `$3` = runner. The
/// `WHERE` order (`revoked_at IS NULL` first) is Django's `Q`-sorted
/// kwargs, not the source filter order. Moved verbatim from the
/// revoke kernel const.
const REVOKE_RUNNER_SESSIONS_SQL: &str =
    "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $3)";

/// Revoke every live session for one runner.
pub async fn revoke_runner_sessions_for_runner<'e, E>(
    ex: E,
    runner_id: Uuid,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_RUNNER_SESSIONS_SQL)
        .bind(now)
        .bind(reason)
        .bind(runner_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Stamp a session's heartbeat.
pub async fn touch_runner_session<'e, E>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(TOUCH_SESSION_SQL)
        .bind(now)
        .bind(id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Idle-sweep bulk revoke (moved verbatim with the sweeps builder):
/// one `UPDATE ... WHERE id IN (...)` over the swept ids. Empty input
/// returns `Ok(0)` with no query, like the sweeps driver's early
/// return (the builder would emit an invalid `IN ()`).
pub async fn revoke_runner_sessions_by_ids<'e, E>(
    ex: E,
    ids: &[Uuid],
    now: DateTime<Utc>,
    reason: &str,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if ids.is_empty() {
        return Ok(0);
    }
    let sql = revoke_sessions_update(ids.len());
    let mut query = sqlx::query(&sql).bind(now).bind(reason);
    for sid in ids {
        query = query.bind(*sid);
    }
    query.execute(ex).await.map(|done| done.rows_affected())
}

/// Bulk session revoke for `session_count` ids.
/// Moved verbatim with the sweeps executor.
fn revoke_sessions_update(session_count: usize) -> String {
    let placeholders = (0..session_count)
        .map(|i| format!("${}", i + 3))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
         \"runner_session\".\"id\" IN ({placeholders})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/auth_oauth")
                .join(name),
        )
        .unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name} is valid JSON: {e}"))
    }

    // Moved from the token handlers: translator vectors over the moved
    // endpoint builders.
    #[test]
    fn bind_placeholders_vectors() {
        assert_eq!(
            bind_placeholders(api_token_insert_sql()),
            "INSERT INTO api_tokens (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, label, description, is_active, last_used, token, user_id, user_type, workspace_id, expired_at, is_service, allowed_rate_limit) VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, TRUE, NULL, $7, $8, $9, NULL, $10, FALSE, '60/min')"
        );
        assert_eq!(
            bind_placeholders(api_token_soft_delete_sql()),
            "UPDATE api_tokens SET deleted_at = $1, updated_at = $2 WHERE id = $3"
        );
        assert_eq!(
            bind_placeholders(api_token_patch_save_sql()),
            "UPDATE api_tokens SET label = $1, description = $2, is_service = $3, allowed_rate_limit = $4, deleted_at = $5, created_by_id = $6, updated_at = $7 WHERE id = $8"
        );
    }

    // Moved write half of the endpoint unit test: builder shapes for
    // the token lifecycle (the read-builder half stays with the
    // queries module).
    #[test]
    fn api_token_write_builders() {
        assert_eq!(API_TOKEN_INSERT_COLUMNS.len(), 17);
        let insert = api_token_insert_sql();
        assert!(insert.contains("INSERT INTO api_tokens (created_at, updated_at"));
        assert!(insert.contains(":token, :user, :user_type, NULL, :expired_at, FALSE, '60/min'"));
        assert_eq!(
            api_token_soft_delete_sql(),
            "UPDATE api_tokens SET deleted_at = :now, updated_at = :now WHERE id = :pk"
        );
        let save = api_token_patch_save_sql();
        assert!(save.contains("is_service = :is_service"));
        assert!(save.contains("deleted_at = :deleted_at"));
        assert!(save.contains("created_by_id = :created_by"));
        assert!(!save.contains("expired_at"));
        assert!(!save.contains("user_type"));
    }

    // Moved F7 tests: deactivation SQL against the fixture.
    #[test]
    fn f7_sql_and_const_match_fixture() {
        let f7 = fixture("F7_deactivate_token.before_after.json");
        assert_eq!(
            cli_token::DEVICE_FLOW_DESCRIPTION,
            "Issued by pidash auth login (device-code flow)."
        );
        assert_eq!(
            f7["function"]["const"],
            "CLI_DEVICE_API_TOKEN_DESCRIPTION='Issued by pidash auth login (device-code flow).'"
        );
        let emitted = f7["emitted_sql"].as_str().expect("emitted sql");
        // (Django fragment, Rust rendering with quoted identifiers).
        for (frag, rust) in [
            ("SET is_active=false", "\"is_active\" = false"),
            ("updated_at=%s", "\"updated_at\" = $1"),
            ("WHERE token=%s", "\"token\" = $2"),
            ("is_active=true", "\"is_active\" = true"),
        ] {
            assert!(emitted.contains(frag), "emitted sql has {frag}");
            assert!(
                cli_token::DEACTIVATE_SQL.contains(rust)
                    || cli_token::DEACTIVATE_CLI_ONLY_SQL.contains(rust),
                "rust sql has {frag} as {rust}"
            );
        }
        assert!(
            emitted.contains("description='Issued by pidash auth login (device-code flow).'"),
            "description guard recorded"
        );
        assert!(
            cli_token::DEACTIVATE_CLI_ONLY_SQL.contains("\"description\" = $3"),
            "cli-only sql guards description"
        );
        assert!(
            !cli_token::DEACTIVATE_SQL.contains("description"),
            "base sql has no description guard"
        );
    }

    #[test]
    fn f7_empty_token_is_noop() {
        assert!(cli_token::is_noop_token(None));
        assert!(cli_token::is_noop_token(Some("")));
        assert!(!cli_token::is_noop_token(Some("pi_dash_api_x")));
    }

    // Moved revoke half of the sweeps byte-exact test (the select
    // half stays with the sweeps module).
    #[test]
    fn revoke_sessions_update_byte_exact() {
        assert_eq!(
            revoke_sessions_update(1),
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
             \"runner_session\".\"id\" IN ($3)"
        );
        assert_eq!(
            revoke_sessions_update(3),
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \
             \"runner_session\".\"id\" IN ($3, $4, $5)"
        );
    }

    // Moved enroll revoke-text pin (the only production use of the
    // enroll revoke const was this assertion).
    #[test]
    fn machine_revoke_by_id_sql_is_quoted_qualified() {
        assert_eq!(
            REVOKE_MACHINE_BY_ID_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"id\" = $2"
        );
    }

    /// Seed one user row for the FK parents the token writes
    /// reference (the workspace-seed bot-user column set, trimmed to a
    /// plain user).
    async fn seed_user(pool: &sqlx::PgPool) -> Uuid {
        let now = Utc::now();
        sqlx::query_scalar(
            "INSERT INTO users (password, last_login, id, username, mobile_number, email,
               display_name, first_name, last_name, avatar, avatar_asset_id, cover_image,
               cover_image_asset_id, date_joined, created_at, updated_at, last_location,
               created_location, is_superuser, is_managed, is_password_expired, is_active,
               is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
               token, user_timezone,
               last_active, last_login_time, last_logout_time, last_login_ip, last_logout_ip,
               last_login_medium, last_login_uagent, token_updated_at, is_bot, bot_type,
               is_email_valid, masked_at)
             VALUES ('!', NULL, $1, 'domain-tokens-live', NULL, 'domain-tokens-live@example.test',
               'Live', 'Live', '', '', NULL, NULL,
               NULL, $2, $2, $2, '',
               '', false, false, false, true,
               false, false, true, false, '', 'UTC',
               $2, NULL, NULL, '', '',
               'email', '', NULL, false, '',
               false, NULL)
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .fetch_one(pool)
        .await
        .expect("seed user")
    }

    /// Live-db half: the endpoint lifecycle round-trips through the
    /// domain functions (issue → touch → patch → deactivate →
    /// soft-delete). Like every other live-db test in the workspace it
    /// skips when `DATABASE_URL` is unset and runs for real otherwise.
    #[tokio::test]
    async fn api_token_lifecycle_roundtrip() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let user_id = seed_user(&pool).await;
        let issued = issue_token(
            &pool,
            &NewApiToken {
                label: "domain-tokens-live".to_owned(),
                description: "live roundtrip".to_owned(),
                user_id,
                user_type: 0,
                expired_at: None,
            },
        )
        .await
        .expect("issue");
        assert!(issued.token.starts_with("pi_dash_api_"));
        touch_api_token_last_used_by_token(&pool, &issued.token)
            .await
            .expect("touch by token");
        let (last_used,): (Option<DateTime<Utc>>,) =
            sqlx::query_as("SELECT last_used FROM api_tokens WHERE id = $1")
                .bind(issued.id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(last_used.is_some());
        update_token(
            &pool,
            issued.id,
            user_id,
            Utc::now(),
            &ApiTokenPatch {
                label: "renamed".to_owned(),
                description: "live roundtrip".to_owned(),
                is_service: false,
                allowed_rate_limit: "60/min".to_owned(),
                deleted_at: None,
                created_by: Some(user_id),
            },
        )
        .await
        .expect("patch");
        deactivate_api_token_by_id(&pool, issued.id, Utc::now())
            .await
            .expect("deactivate");
        let (is_active,): (bool,) =
            sqlx::query_as("SELECT is_active FROM api_tokens WHERE id = $1")
                .bind(issued.id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!is_active);
        soft_delete_token(&pool, issued.id, user_id)
            .await
            .expect("soft delete");
        let (deleted_at,): (Option<DateTime<Utc>>,) =
            sqlx::query_as("SELECT deleted_at FROM api_tokens WHERE id = $1")
                .bind(issued.id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(deleted_at.is_some());
        sqlx::query("DELETE FROM api_tokens WHERE id = $1")
            .bind(issued.id)
            .execute(&pool)
            .await
            .expect("cleanup");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
