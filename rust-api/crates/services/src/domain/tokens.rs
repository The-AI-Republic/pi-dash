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
//!   rotate, enroll mint) stay executor-generic on the caller's existing
//!   `sqlx` transaction: each flow joins tables outside this module, and
//!   design D5 runs the whole domain write on one transaction, so moving
//!   one leg onto [`WriteTxn`][crate::domain::WriteTxn] alone would split
//!   it. Their token SQL moves here; the flows convert to `WriteTxn`
//!   whole when their aggregates do (ORM-03/ORM-07).
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
///
/// `created_by` is the `created_by_id` stamp (D6, PIDASHCONV-1609):
/// the endpoint passes the owner (`views/api.py:20-35` stamps the
/// request user, who is always the owner there), while the cloud MCP
/// OAuth exchange passes the session user — or `None` on a cookieless
/// POST — which may differ from the grant user (`oauth.py:357-374`).
pub struct NewApiToken {
    pub label: String,
    pub description: String,
    pub user_id: Uuid,
    pub user_type: i16,
    pub expired_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
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
/// `updated_by_id` NULL). `created_by_id` is the caller's
/// [`NewApiToken::created_by`] stamp (`None` binds `NULL`, like the
/// PATCH input's). `created`/`updated` are two separate `now()`
/// calls, like the endpoint's, so captured micros differ.
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
        .bind(params.created_by)
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
// machine_token flows (rotate, mint, sign-out, delete)
// ---------------------------------------------------------------------------

/// Device rotate revoke (`device.py:113-117`):
/// `MachineToken.objects.select_for_update().filter(workspace=,
/// dev_machine=, revoked_at__isnull=True).update(revoked_at=now)`.
/// Moved verbatim from the device-machine consts: `$1` now, `$2`
/// workspace, `$3` dev machine, live rows only. The shape differs
/// from the enroll twin below only in conjunct order (unguarded
/// parens here vs `Q`-sorted parens there); the two stay separate
/// functions per D6.
const ROTATE_REVOKE_FOR_PAIR_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"workspace_id\" = $2 AND \"machine_token\".\"dev_machine_id\" = $3 AND \"machine_token\".\"revoked_at\" IS NULL";

/// Enroll/desktop rotate revoke, machine branch
/// (`enrollment.py:174-203`, `desktop.py:134-138`): `$1` now, `$2`
/// dev machine, `$3` workspace, live rows only. Moved verbatim (the
/// enroll and desktop builders spelled identical text). NOTE the
/// filter asymmetry vs B1: `user` is NOT in the filter
/// (BUG-rotate-user-omission, ported as-is).
const ROTATE_REVOKE_FOR_MACHINE_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"workspace_id\" = $3)";

/// Enroll rotate revoke, legacy branch (no dev machine): `$1` now,
/// `$2` host label, `$3` user, `$4` workspace, over rows with
/// `dev_machine_id IS NULL`. Moved verbatim.
const ROTATE_REVOKE_LEGACY_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IS NULL AND \"machine_token\".\"host_label\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"user_id\" = $3 AND \"machine_token\".\"workspace_id\" = $4)";

/// Machine revoke-all (`runners.py:198,238`, delete D1): `$1` now,
/// `$2` dev machine, live rows only, no workspace scope. Moved
/// verbatim (the manage builder, the delete const and the teardown
/// callers spelled identical text).
const REVOKE_TOKENS_FOR_MACHINE_SQL: &str =
    "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL)";

/// Delete-cascade `SET_NULL` (`delete.py`): token rows SURVIVE with
/// a NULLed machine link. `$1` dev machine. Moved verbatim.
const CLEAR_TOKEN_MACHINES_SQL: &str =
    "UPDATE \"machine_token\" SET \"dev_machine_id\" = NULL WHERE \"machine_token\".\"dev_machine_id\" IN ($1)";

/// Machine-token mint: the plain 12-column `INSERT` in
/// `mt_cols::COLUMNS` order (`$1..$12`: id, user, dev machine,
/// workspace, host label, hash, fingerprint, label, is_service,
/// created, last_used, revoked). The device const, the enroll
/// builder and the desktop builder spelled identical text; the
/// column list is rendered from the shared const so drift fails the
/// pin test, not silently.
fn machine_token_insert_sql() -> String {
    let cols = pidash_db::runner_enroll::columns::machine_token::COLUMNS
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({cols}) VALUES ({})",
        pidash_db::runner_enroll::columns::machine_token::TABLE,
        (1..=pidash_db::runner_enroll::columns::machine_token::COLUMNS.len())
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Sign-out revoke over bundled machines (`desktop.py:170-178`):
/// `$1` now, `$2..` dev-machine ids in list order, live rows only.
/// Moved verbatim with the builder's contract: empty input is a
/// caller bug (Django raises `EmptyResultSet`), so the driver
/// asserts like the builder did.
fn signout_revoke_tokens_sql(machine_count: usize) -> String {
    assert!(
        machine_count > 0,
        "signout revoke needs at least one machine id (the source returns 204 with no SQL when no machines match)"
    );
    format!(
        "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IN ({}) AND \"machine_token\".\"revoked_at\" IS NULL)",
        (0..machine_count)
            .map(|i| format!("${}", 2 + i))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Revoke every token for one device pair (device rotate shape).
pub async fn revoke_tokens_for_device_pair<'e, E>(
    ex: E,
    workspace_id: Uuid,
    dev_machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ROTATE_REVOKE_FOR_PAIR_SQL)
        .bind(now)
        .bind(workspace_id)
        .bind(dev_machine_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke live tokens for one machine in one workspace (enroll and
/// desktop rotate shape).
pub async fn revoke_tokens_for_machine_in_workspace<'e, E>(
    ex: E,
    dev_machine_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ROTATE_REVOKE_FOR_MACHINE_SQL)
        .bind(now)
        .bind(dev_machine_id)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke live legacy tokens by host label (enroll rotate without a
/// dev machine).
pub async fn revoke_tokens_legacy_host<'e, E>(
    ex: E,
    host_label: &str,
    user_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ROTATE_REVOKE_LEGACY_SQL)
        .bind(now)
        .bind(host_label)
        .bind(user_id)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke live tokens for one dev machine (manage revoke-all and
/// delete D1 shape; no workspace scope).
pub async fn revoke_tokens_for_dev_machine<'e, E>(
    ex: E,
    dev_machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(REVOKE_TOKENS_FOR_MACHINE_SQL)
        .bind(now)
        .bind(dev_machine_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Revoke live tokens for several dev machines (desktop sign-out).
/// The only caller returns 204 before reaching here with an empty
/// list; an empty list here panics, like the moved builder.
pub async fn revoke_tokens_for_dev_machines<'e, E>(
    ex: E,
    dev_machine_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = signout_revoke_tokens_sql(dev_machine_ids.len());
    let mut query = sqlx::query(&sql).bind(now);
    for id in dev_machine_ids {
        query = query.bind(*id);
    }
    query.execute(ex).await.map(|_| ())
}

/// NULL a deleted machine's link on its token rows (delete cascade;
/// the rows survive).
pub async fn clear_token_dev_machines<'e, E>(ex: E, dev_machine_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CLEAR_TOKEN_MACHINES_SQL)
        .bind(dev_machine_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Resolved inputs for [`insert_machine_token`]: the caller mints
/// the secret, hashes it, derives the labels and stamps the clock;
/// the driver only writes the row. `dev_machine_id` is `None` on
/// the legacy enroll path.
pub struct NewMachineToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub host_label: String,
    pub token_hash: String,
    pub token_fingerprint: String,
    pub label: String,
    pub is_service: bool,
    pub created_at: DateTime<Utc>,
}

/// Mint one machine token row (device, enroll and desktop share the
/// 12-column shape; `last_used_at`/`revoked_at` NULL).
pub async fn insert_machine_token<'e, E>(ex: E, params: &NewMachineToken) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(&machine_token_insert_sql())
        .bind(params.id)
        .bind(params.user_id)
        .bind(params.dev_machine_id)
        .bind(params.workspace_id)
        .bind(&params.host_label)
        .bind(&params.token_hash)
        .bind(&params.token_fingerprint)
        .bind(&params.label)
        .bind(params.is_service)
        .bind(params.created_at)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<DateTime<Utc>>)
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

/// Session open (`views/sessions.py:169+184`): `create(runner=,
/// protocol_version=, last_seen_at=now())`. Moved verbatim from the
/// session model consts: `$1` id (caller-supplied fresh `uuid4`),
/// `$2` runner, `$3` protocol version, `$4` `created_at`
/// (`auto_now_add`), `$5` `last_seen_at`, `$6` `revoked_at` (NULL),
/// `$7` `revoked_reason` (`""` default). No `RETURNING`: the PK is
/// supplied, so Django fetches nothing back.
const OPEN_SESSION_SQL: &str =
    "INSERT INTO \"runner_session\" (\"id\", \"runner_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\") VALUES ($1, $2, $3, $4, $5, $6, $7)";

/// Runner-level session fast delete (delete cascade): `$1..` runner
/// ids in fetch order. Moved verbatim from the delete kernel.
fn delete_runner_sessions_sql(runner_count: usize) -> String {
    format!(
        "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ({})",
        (1..=runner_count)
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Open one runner session. The two stamps are separate `now()` calls
/// in Django's evaluation order: the `last_seen_at` kwarg first, then
/// the `auto_now_add` `created_at` — so `last_seen_at` is never after
/// `created_at`, like the source.
pub async fn open_runner_session<'e, E>(
    ex: E,
    id: Uuid,
    runner_id: Uuid,
    protocol_version: i32,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let last_seen_at = Utc::now();
    let created_at = Utc::now();
    sqlx::query(OPEN_SESSION_SQL)
        .bind(id)
        .bind(runner_id)
        .bind(protocol_version)
        .bind(created_at)
        .bind(last_seen_at)
        .bind(None::<DateTime<Utc>>)
        .bind("")
        .execute(ex)
        .await
        .map(|_| ())
}

/// Delete every session for several runners (delete cascade). The
/// drivers never pass an empty list (the collector returns early on
/// no rows); an empty list here is a no-op rather than an invalid
/// `IN ()`, like the sweeps bulk revoke.
pub async fn delete_runner_sessions_for_runners<'e, E>(
    ex: E,
    runner_ids: &[Uuid],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if runner_ids.is_empty() {
        return Ok(());
    }
    let sql = delete_runner_sessions_sql(runner_ids.len());
    let mut query = sqlx::query(&sql);
    for id in runner_ids {
        query = query.bind(*id);
    }
    query.execute(ex).await.map(|_| ())
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

    // Moved S2 pin (the revoke kernel's `s2_where_order_is_q_sorted`
    // is deleted with the const): the WHERE order is Django's
    // Q-sorted kwargs, not the source filter order.
    #[test]
    fn revoke_runner_sessions_where_order_is_q_sorted() {
        assert_eq!(
            REVOKE_RUNNER_SESSIONS_SQL,
            "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $3)"
        );
    }

    // Moved open/delete pins (the session model's `OPEN_INSERT_SQL`
    // test and the delete kernel's fast-delete test are deleted with
    // their SQL).
    #[test]
    fn session_open_and_delete_sql_pins() {
        assert_eq!(
            OPEN_SESSION_SQL,
            "INSERT INTO \"runner_session\" (\"id\", \"runner_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\") VALUES ($1, $2, $3, $4, $5, $6, $7)"
        );
        assert_eq!(
            delete_runner_sessions_sql(1),
            "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ($1)"
        );
        assert_eq!(
            delete_runner_sessions_sql(3),
            "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ($1, $2, $3)"
        );
    }

    /// Live-db half: open a session, touch it, revoke it, then delete
    /// the runner's rows — all inside a rolled-back transaction (the
    /// `runner_session` FK is deferred, so no parents or cleanup are
    /// needed).
    #[tokio::test]
    async fn runner_session_flow_roundtrip() {
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
        let mut tx = pool.begin().await.expect("begin");
        let sid = Uuid::new_v4();
        let runner_id = Uuid::new_v4();
        open_runner_session(&mut *tx, sid, runner_id, 4)
            .await
            .expect("open");
        let row: (i32, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT protocol_version, revoked_at FROM runner_session WHERE id = $1")
                .bind(sid)
                .fetch_one(&mut *tx)
                .await
                .expect("read open row");
        assert_eq!(row.0, 4);
        assert_eq!(row.1, None);
        touch_runner_session(&mut *tx, sid, Utc::now())
            .await
            .expect("touch");
        revoke_runner_session(&mut *tx, sid, Utc::now(), "flow-test")
            .await
            .expect("revoke");
        let reason: (String,) =
            sqlx::query_as("SELECT revoked_reason FROM runner_session WHERE id = $1")
                .bind(sid)
                .fetch_one(&mut *tx)
                .await
                .expect("read revoked row");
        assert_eq!(reason.0, "flow-test");
        delete_runner_sessions_for_runners(&mut *tx, &[runner_id])
            .await
            .expect("delete");
        let left: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runner_session WHERE id = $1")
            .bind(sid)
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        assert_eq!(left.0, 0);
        tx.rollback().await.expect("rollback");
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

    // Moved machine-flow pins (the enroll/desktop/manage/delete/db
    // pin tests are deleted with their builders; these carry the
    // text here).
    #[test]
    fn machine_flow_sql_pins() {
        assert_eq!(
            ROTATE_REVOKE_FOR_PAIR_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"workspace_id\" = $2 AND \"machine_token\".\"dev_machine_id\" = $3 AND \"machine_token\".\"revoked_at\" IS NULL"
        );
        assert_eq!(
            ROTATE_REVOKE_FOR_MACHINE_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"workspace_id\" = $3)"
        );
        assert_eq!(
            ROTATE_REVOKE_LEGACY_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IS NULL AND \"machine_token\".\"host_label\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"user_id\" = $3 AND \"machine_token\".\"workspace_id\" = $4)"
        );
        assert_eq!(
            REVOKE_TOKENS_FOR_MACHINE_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL)"
        );
        assert_eq!(
            CLEAR_TOKEN_MACHINES_SQL,
            "UPDATE \"machine_token\" SET \"dev_machine_id\" = NULL WHERE \"machine_token\".\"dev_machine_id\" IN ($1)"
        );
        assert_eq!(
            machine_token_insert_sql(),
            "INSERT INTO \"machine_token\" (\"id\", \"user_id\", \"dev_machine_id\", \"workspace_id\", \"host_label\", \"token_hash\", \"token_fingerprint\", \"label\", \"is_service\", \"created_at\", \"last_used_at\", \"revoked_at\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        );
        assert_eq!(
            signout_revoke_tokens_sql(1),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IN ($2) AND \"machine_token\".\"revoked_at\" IS NULL)"
        );
        assert_eq!(
            signout_revoke_tokens_sql(2),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IN ($2, $3) AND \"machine_token\".\"revoked_at\" IS NULL)"
        );
        assert_eq!(
            signout_revoke_tokens_sql(3),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" IN ($2, $3, $4) AND \"machine_token\".\"revoked_at\" IS NULL)"
        );
        // BUG-rotate-user-omission, ported as-is: no user_id on the
        // machine branch (moved with the enroll B3 pin).
        assert!(!ROTATE_REVOKE_FOR_MACHINE_SQL.contains("user_id"));
        // Revokes every active row in scope — no LIMIT (fixture
        // rotate_B3_after; moved with the enroll B3 pin).
        assert!(!ROTATE_REVOKE_FOR_MACHINE_SQL.contains("LIMIT"));
        assert!(!ROTATE_REVOKE_LEGACY_SQL.contains("LIMIT"));
    }

    #[test]
    #[should_panic(expected = "at least one machine id")]
    fn signout_revoke_builder_rejects_empty() {
        signout_revoke_tokens_sql(0);
    }

    /// Live-db half: insert two rows (machine + legacy shapes),
    /// revoke each through its driver, clear one machine link, and
    /// read every row back — all inside a rolled-back transaction.
    /// Every FK on `machine_token` is deferred, so the rollback
    /// skips the checks and no cleanup is needed.
    #[tokio::test]
    async fn machine_token_flow_roundtrip() {
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
        let mut tx = pool.begin().await.expect("begin");
        let now = Utc::now();
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let machine_id = Uuid::new_v4();
        let machine_row = Uuid::new_v4();
        let legacy_row = Uuid::new_v4();
        insert_machine_token(
            &mut *tx,
            &NewMachineToken {
                id: machine_row,
                user_id,
                dev_machine_id: Some(machine_id),
                workspace_id,
                host_label: "live-host".to_owned(),
                token_hash: "live-hash".to_owned(),
                token_fingerprint: "live-fp".to_owned(),
                label: "live-label".to_owned(),
                is_service: true,
                created_at: now,
            },
        )
        .await
        .expect("insert machine row");
        insert_machine_token(
            &mut *tx,
            &NewMachineToken {
                id: legacy_row,
                user_id,
                dev_machine_id: None,
                workspace_id,
                host_label: "legacy-host".to_owned(),
                token_hash: "legacy-hash".to_owned(),
                token_fingerprint: "legacy-fp".to_owned(),
                label: "legacy-label".to_owned(),
                is_service: true,
                created_at: now,
            },
        )
        .await
        .expect("insert legacy row");
        revoke_tokens_for_machine_in_workspace(&mut *tx, machine_id, workspace_id, now)
            .await
            .expect("revoke machine row");
        revoke_tokens_legacy_host(&mut *tx, "legacy-host", user_id, workspace_id, now)
            .await
            .expect("revoke legacy row");
        clear_token_dev_machines(&mut *tx, machine_id)
            .await
            .expect("clear machine link");
        let rows: Vec<(Uuid, Option<Uuid>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT id, dev_machine_id, revoked_at FROM machine_token WHERE id IN ($1, $2)",
        )
        .bind(machine_row)
        .bind(legacy_row)
        .fetch_all(&mut *tx)
        .await
        .expect("read back");
        assert_eq!(rows.len(), 2);
        for (id, machine, revoked) in rows {
            assert!(revoked.is_some(), "row {id} revoked");
            if id == machine_row {
                assert_eq!(machine, None, "machine link cleared");
            } else {
                assert_eq!(machine, None, "legacy row never had a link");
            }
        }
        tx.rollback().await.expect("rollback");
    }

    /// Seed one user row for the FK parents the token writes
    /// reference (the workspace-seed bot-user column set, trimmed to a
    /// plain user). `tag` keeps usernames unique: live tests share one
    /// scratch DB and run in parallel.
    async fn seed_user(pool: &sqlx::PgPool, tag: &str) -> Uuid {
        let now = Utc::now();
        let username = format!("domain-tokens-{tag}");
        let email = format!("domain-tokens-{tag}@example.test");
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
             VALUES ('!', NULL, $1, $3, NULL, $4,
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
        .bind(&username)
        .bind(&email)
        .fetch_one(pool)
        .await
        .expect("seed user")
    }

    /// Live-db `created_by` option (D6, PIDASHCONV-1609): the MCP
    /// OAuth exchange stamps the session user — or `NULL` on a
    /// cookieless POST — while the token owner is the grant user
    /// (`oauth.py:357-374`). Both shapes round-trip through
    /// [`issue_token`]; model defaults stay as they are.
    #[tokio::test]
    async fn issue_token_created_by_override() {
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
        let granter = seed_user(&pool, "cb-grant").await;
        let stamper = seed_user(&pool, "cb-stamp").await;
        // Cookieless POST: `created_by_id` stays NULL.
        let anonymous = issue_token(
            &pool,
            &NewApiToken {
                label: "MCP client".to_owned(),
                description: String::new(),
                user_id: granter,
                user_type: 1,
                expired_at: None,
                created_by: None,
            },
        )
        .await
        .expect("issue anonymous");
        assert!(anonymous.token.starts_with("pi_dash_api_"));
        // Session user differs from the grant user.
        let stamped = issue_token(
            &pool,
            &NewApiToken {
                label: "MCP client".to_owned(),
                description: String::new(),
                user_id: granter,
                user_type: 1,
                expired_at: None,
                created_by: Some(stamper),
            },
        )
        .await
        .expect("issue stamped");
        type Row = (Uuid, Option<Uuid>, i16, bool, String);
        for (id, want_created_by) in [(anonymous.id, None), (stamped.id, Some(stamper))] {
            let row: Row = sqlx::query_as(
                r#"SELECT user_id, created_by_id, user_type, is_service, allowed_rate_limit
                   FROM api_tokens WHERE id = $1"#,
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read back");
            assert_eq!(row.0, granter, "owner is the grant user");
            assert_eq!(row.1, want_created_by, "created_by stamp");
            assert_eq!(row.2, 1, "bot user_type");
            assert!(!row.3, "is_service FALSE");
            assert_eq!(row.4, "60/min", "default rate");
        }
        for id in [anonymous.id, stamped.id] {
            sqlx::query("DELETE FROM api_tokens WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .expect("cleanup");
        }
        for id in [granter, stamper] {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .expect("cleanup");
        }
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
        let user_id = seed_user(&pool, "live").await;
        let issued = issue_token(
            &pool,
            &NewApiToken {
                label: "domain-tokens-live".to_owned(),
                description: "live roundtrip".to_owned(),
                user_id,
                user_type: 0,
                expired_at: None,
                created_by: Some(user_id),
            },
        )
        .await
        .expect("issue");
        assert!(issued.token.starts_with("pi_dash_api_"));
        let (created_by,): (Option<Uuid>,) =
            sqlx::query_as("SELECT created_by_id FROM api_tokens WHERE id = $1")
                .bind(issued.id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(created_by, Some(user_id));
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
