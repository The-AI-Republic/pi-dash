//! OSS token writes: `api_tokens`, `machine_token`, `runner_session`
//! (ORM-08, PIDASHCONV-1520 part B).
//!
//! Every OSS write to the three token tables lives in this module; call
//! sites in `api`, `jobs` and the rest of `services` call these functions
//! instead of executing SQL text. Reads stay where they are (design D5).
//!
//! Every writer takes a SeaORM [`ConnectionTrait`]: the caller's pool
//! (`pidash_db::pool::sea_orm_connection(&pool)`) or the caller's open
//! sqlx transaction (`pidash_db::orm_tx::TxConn::new(&mut tx)`), so no
//! flow restructures its transaction. Two shapes, by writer kind:
//!
//! * Single-statement writers (middleware stamps, endpoint lifecycle,
//!   deactivations, shims, single-row session ops): best-effort vs
//!   hard-error is the CALLER's choice (`let _ =` vs `?`), preserved
//!   per call site — Django `save()` calls propagate, the Rust
//!   middlewares deliberately swallow stamp failures, and both
//!   behaviors are pinned by contract suites.
//! * Transactional drivers (revoke, delete cascade, session open,
//!   rotate, enroll mint) run on the caller's existing sqlx transaction
//!   through `TxConn`: each flow joins tables outside this module, and
//!   design D5 runs the whole domain write on one transaction, so moving
//!   one leg onto [`WriteTxn`][crate::domain::WriteTxn] alone would split
//!   it. The flows convert to `WriteTxn` whole when their aggregates do
//!   (ORM-03/ORM-07).
//!
//! Errors stay [`sqlx::Error`] (through [`pidash_db::orm_tx::into_sqlx`]),
//! so the callers' SQLSTATE classification is unchanged.
//!
//! Clocks are load-bearing and preserved per shape: middleware stamps use
//! the DB clock (`CURRENT_TIMESTAMP`, Postgres's `now()`), flows holding a
//! frozen timestamp bind it.

use chrono::{DateTime, Utc};
use pidash_db::entity::{api_tokens, machine_token, runner, runner_session};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::sea_query::{Expr, Query};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

/// A bound clock as the entity's `timestamptz` value.
fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

// ---------------------------------------------------------------------------
// api_tokens
// ---------------------------------------------------------------------------

/// Middleware stamp (`api_authentication.py:41-44`):
/// `api_token.last_used = now; save(update_fields=["last_used"])`.
/// By presented token, DB clock. Best-effort at the middleware call
/// sites; the id-stamped twin is [`touch_api_token_last_used_by_id`].
fn touch_last_used_by_token_stmt(token: &str) -> sea_orm::UpdateMany<api_tokens::Entity> {
    api_tokens::Entity::update_many()
        .col_expr(
            api_tokens::Column::LastUsed,
            Expr::current_timestamp().into(),
        )
        .filter(api_tokens::Column::Token.eq(token))
}

/// Touch `last_used` by presented token (middleware shape).
pub async fn touch_api_token_last_used_by_token(
    db: &impl ConnectionTrait,
    token: &str,
) -> Result<(), sqlx::Error> {
    touch_last_used_by_token_stmt(token)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Touch `last_used` by id with the caller's clock (device, CLI and
/// enroll validators resolve the id first and stamp their frozen `now`).
pub async fn touch_api_token_last_used_by_id(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::update_many()
        .col_expr(api_tokens::Column::LastUsed, Expr::value(now))
        .filter(api_tokens::Column::Id.eq(id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Deactivate by id with the caller's clock (device revoke shape,
/// `device.py:496-511`: inline `is_active=False` save by id, NOT the
/// `cli_token` helper).
pub async fn deactivate_api_token_by_id(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::update_many()
        .col_expr(api_tokens::Column::IsActive, Expr::value(false))
        .col_expr(api_tokens::Column::UpdatedAt, Expr::value(now))
        .filter(api_tokens::Column::Id.eq(id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `BaseModel.save` second statement on token PATCH and soft-delete:
/// `updated_by` is stamped by the request actor after the main write.
async fn stamp_updated_by(
    db: &impl ConnectionTrait,
    pk: Uuid,
    actor: Uuid,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::update_many()
        .col_expr(api_tokens::Column::UpdatedById, Expr::value(actor))
        .filter(api_tokens::Column::Id.eq(pk))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
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

/// The `api_tokens` row for [`issue_token`]: `APIToken.objects.create(...)`
/// with model defaults (`token` is `generate_token()`, `is_active` TRUE,
/// `is_service` FALSE, `workspace_id` NULL, `allowed_rate_limit`
/// `"60/min"`, `updated_by_id` / `deleted_at` / `last_used` NULL).
fn issue_token_row(
    params: &NewApiToken,
    id: Uuid,
    token: &str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> api_tokens::ActiveModel {
    api_tokens::ActiveModel {
        created_at: Set(at(created_at)),
        updated_at: Set(at(updated_at)),
        created_by_id: Set(params.created_by),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        id: Set(id),
        label: Set(params.label.clone()),
        description: Set(params.description.clone()),
        is_active: Set(true),
        last_used: Set(None),
        token: Set(token.to_owned()),
        user_id: Set(params.user_id),
        user_type: Set(params.user_type),
        workspace_id: Set(None),
        expired_at: Set(params.expired_at.map(at)),
        is_service: Set(false),
        allowed_rate_limit: Set("60/min".to_owned()),
    }
}

/// Issue a user API key (`views/api.py:20-35`). `created_by_id` is the
/// caller's [`NewApiToken::created_by`] stamp (`None` binds `NULL`, like
/// the PATCH input's). `created`/`updated` are two separate `now()`
/// calls, like the endpoint's, so captured micros differ.
pub async fn issue_token(
    db: &impl ConnectionTrait,
    params: &NewApiToken,
) -> Result<IssuedApiToken, sqlx::Error> {
    let id = Uuid::new_v4();
    let token = format!("pi_dash_api_{}", Uuid::new_v4().simple());
    let created_at = Utc::now();
    let updated_at = Utc::now();
    api_tokens::Entity::insert(issue_token_row(params, id, &token, created_at, updated_at))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
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

/// Token PATCH (`views/api.py:68-71`): the partial serializer's full-row
/// `UPDATE` (`SET` every writable attr, `deleted_at` included).
fn update_token_stmt(
    pk: Uuid,
    now: DateTime<Utc>,
    patch: &ApiTokenPatch,
) -> sea_orm::UpdateMany<api_tokens::Entity> {
    api_tokens::Entity::update_many()
        .col_expr(api_tokens::Column::Label, Expr::value(patch.label.clone()))
        .col_expr(
            api_tokens::Column::Description,
            Expr::value(patch.description.clone()),
        )
        .col_expr(api_tokens::Column::IsService, Expr::value(patch.is_service))
        .col_expr(
            api_tokens::Column::AllowedRateLimit,
            Expr::value(patch.allowed_rate_limit.clone()),
        )
        .col_expr(api_tokens::Column::DeletedAt, Expr::value(patch.deleted_at))
        .col_expr(
            api_tokens::Column::CreatedById,
            Expr::value(patch.created_by),
        )
        .col_expr(api_tokens::Column::UpdatedAt, Expr::value(now))
        .filter(api_tokens::Column::Id.eq(pk))
}

/// Token PATCH save (`views/api.py:68-71`): the full-row `UPDATE` plus
/// the `updated_by` stamp. Two autocommit statements, in this order,
/// like the endpoint (the endpoint never runs these in a transaction).
pub async fn update_token(
    db: &impl ConnectionTrait,
    pk: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
    patch: &ApiTokenPatch,
) -> Result<(), sqlx::Error> {
    update_token_stmt(pk, now, patch)
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    stamp_updated_by(db, pk, actor).await
}

/// Token soft-delete (`views/api.py:46-64`): guarded lookup then
/// `token.delete()` (`BaseModel.delete` stamps `deleted_at`, the
/// concrete `save()` refreshes `updated_at`: two separate `now()` calls,
/// like the endpoint) plus the `updated_by` stamp. Two autocommit
/// statements, in this order, like [`update_token`].
pub async fn soft_delete_token(
    db: &impl ConnectionTrait,
    pk: Uuid,
    actor: Uuid,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::update_many()
        .col_expr(api_tokens::Column::DeletedAt, Expr::value(Utc::now()))
        .col_expr(api_tokens::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(api_tokens::Column::Id.eq(pk))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    stamp_updated_by(db, pk, actor).await
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
/// `allowed_rate_limit` `"60/min"`; the audit, `deleted_at`,
/// `last_used` and `expired_at` columns NULL). Runs on the caller's
/// connection — the device post-token handler mints inside its
/// transaction.
pub async fn issue_device_token(
    db: &impl ConnectionTrait,
    params: &NewDeviceToken,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::insert(api_tokens::ActiveModel {
        id: Set(params.id),
        created_at: Set(at(params.now)),
        updated_at: Set(at(params.now)),
        token: Set(params.token.clone()),
        label: Set(params.label.clone()),
        description: Set(params.description.clone()),
        is_active: Set(true),
        user_type: Set(0),
        user_id: Set(params.user_id),
        workspace_id: Set(params.workspace_id),
        is_service: Set(false),
        allowed_rate_limit: Set("60/min".to_owned()),
        created_by_id: Set(None),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        last_used: Set(None),
        expired_at: Set(None),
    })
    .exec_without_returning(db)
    .await
    .map(|_| ())
    .map_err(into_sqlx)
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

/// The shim token's fixed description.
const GITHUB_SHIM_DESCRIPTION: &str = "GitHub integration FK shim — not for auth";

/// GitHub integration FK shim (`github.py:114-128` / the workspace
/// webhook ensure): the inactive placeholder row the integration points
/// at. The app and workspace call sites pass their own label/token
/// formats; `user_type` 1 (bot), `updated_by_id` / `deleted_at` /
/// `last_used` / `expired_at` NULL.
pub async fn insert_github_shim_token(
    db: &impl ConnectionTrait,
    params: &NewGithubShimToken,
) -> Result<(), sqlx::Error> {
    api_tokens::Entity::insert(api_tokens::ActiveModel {
        id: Set(params.id),
        created_at: Set(at(params.now)),
        updated_at: Set(at(params.now)),
        created_by_id: Set(Some(params.actor_id)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        label: Set(params.label.clone()),
        description: Set(GITHUB_SHIM_DESCRIPTION.to_owned()),
        is_active: Set(false),
        last_used: Set(None),
        token: Set(params.token.clone()),
        user_id: Set(params.actor_id),
        user_type: Set(1),
        workspace_id: Set(Some(params.workspace_id)),
        expired_at: Set(None),
        is_service: Set(false),
        allowed_rate_limit: Set("60/min".to_owned()),
    })
    .exec_without_returning(db)
    .await
    .map(|_| ())
    .map_err(into_sqlx)
}

/// `deactivate_api_token` (`services/cli_tokens.py:1-19`, full file),
/// moved verbatim from `pidash-db` with its executor and fixture
/// tests: bridge tokens are deactivated for API-key callers on
/// rotation/consume.
pub mod cli_token {
    use super::*;

    /// `CLI_DEVICE_API_TOKEN_DESCRIPTION` (`cli_tokens.py:9`).
    pub(crate) const DEVICE_FLOW_DESCRIPTION: &str =
        pidash_db::auth_oauth::models::api_token_device_flow::DEVICE_FLOW_DESCRIPTION;

    /// `if not raw_token: return 0` (`cli_tokens.py:14-15`): `None` or `""`
    /// short-circuits with no query.
    pub fn is_noop_token(raw_token: Option<&str>) -> bool {
        raw_token.unwrap_or("").is_empty()
    }

    /// `APIToken.objects.filter(token=, is_active=True)[.filter(
    /// description=CLI_DEVICE_API_TOKEN_DESCRIPTION)].update(
    /// is_active=False, updated_at=now)` — fixture F7.
    pub(crate) fn deactivate_stmt(
        token: &str,
        only_cli_device_tokens: bool,
        now: DateTime<Utc>,
    ) -> sea_orm::UpdateMany<api_tokens::Entity> {
        let update = api_tokens::Entity::update_many()
            .col_expr(api_tokens::Column::IsActive, Expr::value(false))
            .col_expr(api_tokens::Column::UpdatedAt, Expr::value(now))
            .filter(api_tokens::Column::Token.eq(token))
            .filter(api_tokens::Column::IsActive.eq(true));
        if only_cli_device_tokens {
            update.filter(api_tokens::Column::Description.eq(DEVICE_FLOW_DESCRIPTION))
        } else {
            update
        }
    }

    /// Mark an `APIToken` inactive after its exchange for an `mt_` token
    /// (`cli_tokens.py:12-19`). Returns the updated row count (`0` when the
    /// token is empty/`None`, unmatched, inactive, or — with
    /// `only_cli_device_tokens` — not a device-flow token). The revoke
    /// endpoint does NOT use this helper (inline `is_active=False` save,
    /// `device.py:496-511`).
    pub async fn deactivate_api_token(
        db: &impl ConnectionTrait,
        raw_token: Option<&str>,
        only_cli_device_tokens: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, sqlx::Error> {
        let token = raw_token.unwrap_or("");
        if token.is_empty() {
            return Ok(0);
        }
        deactivate_stmt(token, only_cli_device_tokens, now)
            .exec(db)
            .await
            .map(|done| done.rows_affected)
            .map_err(into_sqlx)
    }
}

// ---------------------------------------------------------------------------
// machine_token
// ---------------------------------------------------------------------------

/// One `machine_token` stamp: `SET <column> = <value> WHERE <filter>`.
async fn stamp_machine_token(
    db: &impl ConnectionTrait,
    column: machine_token::Column,
    value: sea_orm::sea_query::SimpleExpr,
    filter: sea_orm::Condition,
) -> Result<(), sqlx::Error> {
    machine_token::Entity::update_many()
        .col_expr(column, value)
        .filter(filter)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// One `machine_token` touch: `SET last_used_at = <used>, expires_at =
/// <expires> WHERE <filter>` (the 60-day idle-expiry slide).
fn touch_machine_token_stmt(
    used: sea_orm::sea_query::SimpleExpr,
    expires: sea_orm::sea_query::SimpleExpr,
    filter: sea_orm::Condition,
) -> sea_orm::UpdateMany<machine_token::Entity> {
    machine_token::Entity::update_many()
        .col_expr(machine_token::Column::LastUsedAt, used)
        .col_expr(machine_token::Column::ExpiresAt, expires)
        .filter(filter)
}

async fn touch_machine_token(
    db: &impl ConnectionTrait,
    used: sea_orm::sea_query::SimpleExpr,
    expires: sea_orm::sea_query::SimpleExpr,
    filter: sea_orm::Condition,
) -> Result<(), sqlx::Error> {
    touch_machine_token_stmt(used, expires, filter)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `revoked_at = <now>` over `filter` (every revoke shape below).
fn revoke_machine_stmt(
    now: sea_orm::sea_query::SimpleExpr,
    filter: sea_orm::Condition,
) -> sea_orm::UpdateMany<machine_token::Entity> {
    machine_token::Entity::update_many()
        .col_expr(machine_token::Column::RevokedAt, now)
        .filter(filter)
}

async fn revoke_machine(
    db: &impl ConnectionTrait,
    now: sea_orm::sea_query::SimpleExpr,
    filter: sea_orm::Condition,
) -> Result<(), sqlx::Error> {
    revoke_machine_stmt(now, filter)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

fn by_hash(token_hash: &str) -> sea_orm::Condition {
    sea_orm::Condition::all().add(machine_token::Column::TokenHash.eq(token_hash))
}

fn by_id(id: Uuid) -> sea_orm::Condition {
    sea_orm::Condition::all().add(machine_token::Column::Id.eq(id))
}

/// Middleware stamp (`api_authentication.py:60-64` plus the 60-day idle
/// expiry slide): `last_used_at = now, expires_at = now + 60d`, by
/// presented hash, DB clock. Best-effort at the middleware call sites;
/// the by-id twins are [`touch_machine_token_last_used_by_id`] (bound
/// clock) and [`touch_machine_token_last_used_by_id_now`] (DB clock).
pub async fn touch_machine_token_last_used_by_hash(
    db: &impl ConnectionTrait,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    touch_machine_token(
        db,
        Expr::current_timestamp().into(),
        Expr::value(Utc::now() + chrono::Duration::days(60)),
        by_hash(token_hash),
    )
    .await
}

/// Touch `last_used_at`/`expires_at` by id with the DB clock (validators
/// that resolve the id first and stamp `now()`).
pub async fn touch_machine_token_last_used_by_id_now(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    touch_machine_token(
        db,
        Expr::current_timestamp().into(),
        Expr::value(Utc::now() + chrono::Duration::days(60)),
        by_id(id),
    )
    .await
}

/// Touch `last_used_at`/`expires_at` by id with the caller's clock
/// (device, CLI, enroll and runner validators stamp their frozen `now`).
pub async fn touch_machine_token_last_used_by_id(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    touch_machine_token(
        db,
        Expr::value(now),
        Expr::value(now + chrono::Duration::days(60)),
        by_id(id),
    )
    .await
}

/// Middleware revoke-then-deny (`api_authentication.py:54-58`):
/// `machine_token.revoke()` stamps `revoked_at` and the request is
/// denied. By presented hash, DB clock.
pub async fn revoke_machine_token_by_hash(
    db: &impl ConnectionTrait,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    revoke_machine(db, Expr::current_timestamp().into(), by_hash(token_hash)).await
}

/// Revoke by id with the DB clock.
pub async fn revoke_machine_token_by_id_now(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    revoke_machine(db, Expr::current_timestamp().into(), by_id(id)).await
}

/// Revoke by id with the caller's clock (validators stamp their frozen
/// `now`).
pub async fn revoke_machine_token_by_id(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(db, Expr::value(now), by_id(id)).await
}

/// Revoke by id with the caller's clock, only when still live (device
/// rotate path): a double rotate keeps the first `revoked_at`.
pub async fn revoke_machine_token_by_id_if_active(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(
        db,
        Expr::value(now),
        by_id(id).add(machine_token::Column::RevokedAt.is_null()),
    )
    .await
}

/// Workspace machine-token revoke filter (`ee-overlay`
/// `service.py:462-467`, `token_qs.update(revoked_at=now)`): live rows in
/// the workspace, optionally only one user's.
fn workspace_tokens_filter(workspace_id: Uuid, user_id: Option<Uuid>) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(machine_token::Column::WorkspaceId.eq(workspace_id))
        .add(machine_token::Column::RevokedAt.is_null())
        .add_option(user_id.map(|user| machine_token::Column::UserId.eq(user)))
}

/// Revoke live machine tokens by workspace, optionally scoped to one
/// user (`_revoke_runtime_access`, `ee-overlay`
/// `service.py:457-477`). Runs on the caller's connection — the overlay
/// calls this on its event transaction; `now` is the caller's shared
/// stamp (tokens and the session sweep stamp the same instant).
/// Single-column stamp, no `updated_at` (the table has none).
pub async fn revoke_machine_tokens_by_workspace(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(
        db,
        Expr::value(now),
        workspace_tokens_filter(workspace_id, user_id),
    )
    .await
}

// ---------------------------------------------------------------------------
// machine_token flows (rotate, mint, sign-out, delete)
// ---------------------------------------------------------------------------

/// Device rotate revoke (`device.py:113-117`):
/// `MachineToken.objects.select_for_update().filter(workspace=,
/// dev_machine=, revoked_at__isnull=True).update(revoked_at=now)`.
fn device_pair_filter(workspace_id: Uuid, dev_machine_id: Uuid) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(machine_token::Column::WorkspaceId.eq(workspace_id))
        .add(machine_token::Column::DevMachineId.eq(dev_machine_id))
        .add(machine_token::Column::RevokedAt.is_null())
}

/// Enroll/desktop rotate revoke, machine branch
/// (`enrollment.py:174-203`, `desktop.py:134-138`): live rows for the
/// machine in the workspace. NOTE the filter asymmetry vs the device
/// pair: `user` is NOT in the filter (BUG-rotate-user-omission, ported
/// as-is), and there is no `LIMIT` (every live row in scope).
fn machine_in_workspace_filter(dev_machine_id: Uuid, workspace_id: Uuid) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(machine_token::Column::DevMachineId.eq(dev_machine_id))
        .add(machine_token::Column::RevokedAt.is_null())
        .add(machine_token::Column::WorkspaceId.eq(workspace_id))
}

/// Enroll rotate revoke, legacy branch (no dev machine): live rows by
/// host label, user and workspace with `dev_machine_id IS NULL`.
fn legacy_host_filter(host_label: &str, user_id: Uuid, workspace_id: Uuid) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(machine_token::Column::DevMachineId.is_null())
        .add(machine_token::Column::HostLabel.eq(host_label))
        .add(machine_token::Column::RevokedAt.is_null())
        .add(machine_token::Column::UserId.eq(user_id))
        .add(machine_token::Column::WorkspaceId.eq(workspace_id))
}

/// Machine revoke-all (`runners.py:198,238`, delete D1) and desktop
/// sign-out (`desktop.py:170-178`): live rows for the machines, no
/// workspace scope.
fn dev_machines_filter(dev_machine_ids: &[Uuid]) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(machine_token::Column::DevMachineId.is_in(dev_machine_ids.iter().copied()))
        .add(machine_token::Column::RevokedAt.is_null())
}

/// Revoke every token for one device pair (device rotate shape).
pub async fn revoke_tokens_for_device_pair(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    dev_machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(
        db,
        Expr::value(now),
        device_pair_filter(workspace_id, dev_machine_id),
    )
    .await
}

/// Revoke live tokens for one machine in one workspace (enroll and
/// desktop rotate shape).
pub async fn revoke_tokens_for_machine_in_workspace(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(
        db,
        Expr::value(now),
        machine_in_workspace_filter(dev_machine_id, workspace_id),
    )
    .await
}

/// Revoke live legacy tokens by host label (enroll rotate without a
/// dev machine).
pub async fn revoke_tokens_legacy_host(
    db: &impl ConnectionTrait,
    host_label: &str,
    user_id: Uuid,
    workspace_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(
        db,
        Expr::value(now),
        legacy_host_filter(host_label, user_id, workspace_id),
    )
    .await
}

/// Revoke live tokens for one dev machine (manage revoke-all and
/// delete D1 shape; no workspace scope).
pub async fn revoke_tokens_for_dev_machine(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    revoke_machine(db, Expr::value(now), dev_machines_filter(&[dev_machine_id])).await
}

/// Revoke live tokens for several dev machines (desktop sign-out).
/// The only caller returns 204 before reaching here with an empty
/// list; an empty list here panics (Django raises `EmptyResultSet`:
/// the source returns 204 with no SQL when no machines match).
pub async fn revoke_tokens_for_dev_machines(
    db: &impl ConnectionTrait,
    dev_machine_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    assert!(
        !dev_machine_ids.is_empty(),
        "signout revoke needs at least one machine id (the source returns 204 with no SQL when no machines match)"
    );
    revoke_machine(db, Expr::value(now), dev_machines_filter(dev_machine_ids)).await
}

/// NULL a deleted machine's link on its token rows (delete cascade
/// `SET_NULL`, `delete.py`: the token rows SURVIVE).
pub async fn clear_token_dev_machines(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
) -> Result<(), sqlx::Error> {
    stamp_machine_token(
        db,
        machine_token::Column::DevMachineId,
        Expr::value(None::<Uuid>),
        sea_orm::Condition::all().add(machine_token::Column::DevMachineId.eq(dev_machine_id)),
    )
    .await
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

/// The minted `machine_token` row: every column, `last_used_at` and
/// `revoked_at` NULL.
fn machine_token_row(params: &NewMachineToken) -> machine_token::ActiveModel {
    machine_token::ActiveModel {
        id: Set(params.id),
        user_id: Set(params.user_id),
        dev_machine_id: Set(params.dev_machine_id),
        workspace_id: Set(params.workspace_id),
        host_label: Set(params.host_label.clone()),
        token_hash: Set(params.token_hash.clone()),
        token_fingerprint: Set(params.token_fingerprint.clone()),
        label: Set(params.label.clone()),
        is_service: Set(params.is_service),
        created_at: Set(at(params.created_at)),
        last_used_at: Set(None),
        revoked_at: Set(None),
        expires_at: Set(Some(at(params.created_at + chrono::Duration::days(60)))),
    }
}

/// Mint one machine token row (device, enroll and desktop share the
/// shape).
pub async fn insert_machine_token(
    db: &impl ConnectionTrait,
    params: &NewMachineToken,
) -> Result<(), sqlx::Error> {
    machine_token::Entity::insert(machine_token_row(params))
        .exec_without_returning(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// runner_session
// ---------------------------------------------------------------------------

/// `revoked_at = now, revoked_reason = reason` over `filter` (every
/// session revoke shape).
fn revoke_sessions_stmt(
    now: DateTime<Utc>,
    reason: &str,
    filter: sea_orm::Condition,
) -> sea_orm::UpdateMany<runner_session::Entity> {
    runner_session::Entity::update_many()
        .col_expr(runner_session::Column::RevokedAt, Expr::value(now))
        .col_expr(runner_session::Column::RevokedReason, Expr::value(reason))
        .filter(filter)
}

async fn revoke_sessions(
    db: &impl ConnectionTrait,
    now: DateTime<Utc>,
    reason: &str,
    filter: sea_orm::Condition,
) -> Result<u64, sqlx::Error> {
    revoke_sessions_stmt(now, reason, filter)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// Revoke one session by id (`sessions.py:307-309` / `pubsub.py:96-104`):
/// an unconditional stamp.
pub async fn revoke_runner_session(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    revoke_sessions(
        db,
        now,
        reason,
        sea_orm::Condition::all().add(runner_session::Column::Id.eq(id)),
    )
    .await
    .map(|_| ())
}

/// Revoke every live session for one runner (enroll revoke S2,
/// `:618-620`).
pub async fn revoke_runner_sessions_for_runner(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    revoke_sessions(
        db,
        now,
        reason,
        sea_orm::Condition::all()
            .add(runner_session::Column::RevokedAt.is_null())
            .add(runner_session::Column::RunnerId.eq(runner_id)),
    )
    .await
    .map(|_| ())
}

/// Workspace session sweep (`ee-overlay` `service.py:470-475`): Django's
/// spanning `runner__workspace` / `runner__owner` update filter as the
/// equivalent `runner_id IN (SELECT id FROM runner WHERE ...)`, live rows
/// only, optionally one owner's runners.
fn workspace_sessions_filter(workspace_id: Uuid, owner_id: Option<Uuid>) -> sea_orm::Condition {
    let runners = Query::select()
        .column(runner::Column::Id)
        .from(runner::Entity)
        .and_where(runner::Column::WorkspaceId.eq(workspace_id))
        .and_where_option(owner_id.map(|owner| runner::Column::OwnerId.eq(owner)))
        .to_owned();
    sea_orm::Condition::all()
        .add(runner_session::Column::RevokedAt.is_null())
        .add(runner_session::Column::RunnerId.in_subquery(runners))
}

/// Revoke live runner sessions by workspace, optionally scoped to one
/// owner (`_revoke_runtime_access`, `ee-overlay`
/// `service.py:457-477`). Runs on the caller's connection with the
/// caller's shared `now`; the overlay passes its `membership_revoked`
/// reason.
pub async fn revoke_runner_sessions_by_workspace(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    owner_id: Option<Uuid>,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    revoke_sessions(
        db,
        now,
        reason,
        workspace_sessions_filter(workspace_id, owner_id),
    )
    .await
    .map(|_| ())
}

/// Stamp a session's heartbeat (`sessions.py`, `last_seen_at` on every
/// poll).
pub async fn touch_runner_session(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    runner_session::Entity::update_many()
        .col_expr(runner_session::Column::LastSeenAt, Expr::value(now))
        .filter(runner_session::Column::Id.eq(id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Idle-sweep bulk revoke: one `UPDATE ... WHERE id IN (...)` over the
/// swept ids. Empty input returns `Ok(0)` with no query, like the sweeps
/// driver's early return.
pub async fn revoke_runner_sessions_by_ids(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
    now: DateTime<Utc>,
    reason: &str,
) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    revoke_sessions(
        db,
        now,
        reason,
        sea_orm::Condition::all().add(runner_session::Column::Id.is_in(ids.iter().copied())),
    )
    .await
}

/// Session open (`views/sessions.py:169+184`): `create(runner=,
/// protocol_version=, last_seen_at=now())`: caller-supplied fresh
/// `uuid4`, `created_at` (`auto_now_add`), `revoked_at` NULL,
/// `revoked_reason` `""` (the default). No `RETURNING`: the PK is
/// supplied, so Django fetches nothing back.
fn open_session_row(
    id: Uuid,
    runner_id: Uuid,
    protocol_version: i32,
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
) -> runner_session::ActiveModel {
    runner_session::ActiveModel {
        id: Set(id),
        runner_id: Set(runner_id),
        protocol_version: Set(protocol_version),
        created_at: Set(at(created_at)),
        last_seen_at: Set(Some(at(last_seen_at))),
        revoked_at: Set(None),
        revoked_reason: Set(String::new()),
    }
}

/// Open one runner session. The two stamps are separate `now()` calls
/// in Django's evaluation order: the `last_seen_at` kwarg first, then
/// the `auto_now_add` `created_at` — so `last_seen_at` is never after
/// `created_at`, like the source.
pub async fn open_runner_session(
    db: &impl ConnectionTrait,
    id: Uuid,
    runner_id: Uuid,
    protocol_version: i32,
) -> Result<(), sqlx::Error> {
    let last_seen_at = Utc::now();
    let created_at = Utc::now();
    runner_session::Entity::insert(open_session_row(
        id,
        runner_id,
        protocol_version,
        created_at,
        last_seen_at,
    ))
    .exec_without_returning(db)
    .await
    .map(|_| ())
    .map_err(into_sqlx)
}

/// Runner-level session fast delete (delete cascade) over the runner
/// ids.
fn delete_runner_sessions_stmt(runner_ids: &[Uuid]) -> sea_orm::DeleteMany<runner_session::Entity> {
    runner_session::Entity::delete_many()
        .filter(runner_session::Column::RunnerId.is_in(runner_ids.iter().copied()))
}

/// Delete every session for several runners (delete cascade). The
/// drivers never pass an empty list (the collector returns early on
/// no rows); an empty list here is a no-op rather than an `IN ()`, like
/// the sweeps bulk revoke.
pub async fn delete_runner_sessions_for_runners(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    if runner_ids.is_empty() {
        return Ok(());
    }
    delete_runner_sessions_stmt(runner_ids)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
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

    /// The rendered statement (values inlined), for shape pins.
    fn sql<Q: sea_orm::QueryTrait>(stmt: &Q) -> String {
        stmt.build(sea_orm::DbBackend::Postgres).to_string()
    }

    fn sample_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-05T10:00:00Z")
            .expect("rfc3339")
            .with_timezone(&Utc)
    }

    // The token lifecycle statements: the create row carries Django's
    // model defaults, the PATCH sets every writable attr (no
    // `expired_at` / `user_type`), stamps go by id.
    #[test]
    fn api_token_write_statements() {
        let params = NewApiToken {
            label: "L".to_owned(),
            description: "D".to_owned(),
            user_id: Uuid::nil(),
            user_type: 1,
            expired_at: None,
            created_by: None,
        };
        let now = sample_now();
        let insert = sql(&api_tokens::Entity::insert(issue_token_row(
            &params,
            Uuid::nil(),
            "pi_dash_api_x",
            now,
            now,
        )));
        assert!(
            insert.starts_with("INSERT INTO \"api_tokens\" ("),
            "{insert}"
        );
        for column in [
            "\"created_at\"",
            "\"updated_at\"",
            "\"created_by_id\"",
            "\"updated_by_id\"",
            "\"deleted_at\"",
            "\"is_active\"",
            "\"last_used\"",
            "\"workspace_id\"",
            "\"is_service\"",
            "\"allowed_rate_limit\"",
        ] {
            assert!(insert.contains(column), "insert sets {column}: {insert}");
        }
        assert!(insert.contains("'60/min'"), "{insert}");
        assert!(
            insert.contains("TRUE") && insert.contains("FALSE"),
            "{insert}"
        );
        let patch = ApiTokenPatch {
            label: "L".to_owned(),
            description: "D".to_owned(),
            is_service: true,
            allowed_rate_limit: "10/min".to_owned(),
            deleted_at: None,
            created_by: None,
        };
        let save = sql(&update_token_stmt(Uuid::nil(), now, &patch));
        for set in [
            "\"label\" = 'L'",
            "\"description\" = 'D'",
            "\"is_service\" = TRUE",
            "\"allowed_rate_limit\" = '10/min'",
            "\"deleted_at\" = NULL",
            "\"created_by_id\" = NULL",
            "\"updated_at\" = '2026-11-05 10:00:00.000000 +00:00'",
        ] {
            assert!(save.contains(set), "patch sets {set}: {save}");
        }
        assert!(!save.contains("expired_at"), "{save}");
        assert!(!save.contains("user_type"), "{save}");
        assert!(
            save.ends_with("WHERE \"api_tokens\".\"id\" = '00000000-0000-0000-0000-000000000000'"),
            "{save}"
        );
        let touch = sql(&touch_last_used_by_token_stmt("tok"));
        assert_eq!(
            touch,
            "UPDATE \"api_tokens\" SET \"last_used\" = CURRENT_TIMESTAMP WHERE \"api_tokens\".\"token\" = 'tok'"
        );
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
        let now = sample_now();
        let base = sql(&cli_token::deactivate_stmt("tok", false, now));
        let cli_only = sql(&cli_token::deactivate_stmt("tok", true, now));
        // (Django fragment, SeaORM rendering with quoted identifiers).
        for (frag, rust) in [
            ("SET is_active=false", "SET \"is_active\" = FALSE"),
            (
                "updated_at=%s",
                "\"updated_at\" = '2026-11-05 10:00:00.000000 +00:00'",
            ),
            ("WHERE token=%s", "\"api_tokens\".\"token\" = 'tok'"),
            ("is_active=true", "\"api_tokens\".\"is_active\" = TRUE"),
        ] {
            assert!(emitted.contains(frag), "emitted sql has {frag}");
            assert!(base.contains(rust), "base sql has {frag} as {rust}: {base}");
            assert!(
                cli_only.contains(rust),
                "cli sql has {frag} as {rust}: {cli_only}"
            );
        }
        assert!(
            emitted.contains("description='Issued by pidash auth login (device-code flow).'"),
            "description guard recorded"
        );
        assert!(
            cli_only.contains(
                "\"api_tokens\".\"description\" = 'Issued by pidash auth login (device-code flow).'"
            ),
            "cli-only sql guards description: {cli_only}"
        );
        assert!(
            !base.contains("description"),
            "base sql has no description guard"
        );
    }

    #[test]
    fn f7_empty_token_is_noop() {
        assert!(cli_token::is_noop_token(None));
        assert!(cli_token::is_noop_token(Some("")));
        assert!(!cli_token::is_noop_token(Some("pi_dash_api_x")));
    }

    // Session statements: the S2 revoke filters live rows of the
    // runner, open writes every column, the fast delete goes by runner.
    #[test]
    fn runner_session_statements() {
        let now = sample_now();
        let runner = Uuid::nil();
        let s2 = sql(&revoke_sessions_stmt(
            now,
            "r",
            sea_orm::Condition::all()
                .add(runner_session::Column::RevokedAt.is_null())
                .add(runner_session::Column::RunnerId.eq(runner)),
        ));
        assert_eq!(
            s2,
            "UPDATE \"runner_session\" SET \"revoked_at\" = '2026-11-05 10:00:00.000000 +00:00', \"revoked_reason\" = 'r' WHERE \"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = '00000000-0000-0000-0000-000000000000'"
        );
        let open = sql(&runner_session::Entity::insert(open_session_row(
            Uuid::nil(),
            runner,
            4,
            now,
            now,
        )));
        assert!(
            open.starts_with("INSERT INTO \"runner_session\" ("),
            "{open}"
        );
        for column in [
            "\"id\"",
            "\"runner_id\"",
            "\"protocol_version\"",
            "\"created_at\"",
            "\"last_seen_at\"",
            "\"revoked_at\"",
            "\"revoked_reason\"",
        ] {
            assert!(open.contains(column), "open sets {column}: {open}");
        }
        let delete = sql(&delete_runner_sessions_stmt(&[runner, runner]));
        assert_eq!(
            delete,
            "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ('00000000-0000-0000-0000-000000000000', '00000000-0000-0000-0000-000000000000')"
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
        open_runner_session(&pidash_db::orm_tx::TxConn::new(&mut tx), sid, runner_id, 4)
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
        touch_runner_session(&pidash_db::orm_tx::TxConn::new(&mut tx), sid, Utc::now())
            .await
            .expect("touch");
        revoke_runner_session(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            sid,
            Utc::now(),
            "flow-test",
        )
        .await
        .expect("revoke");
        let reason: (String,) =
            sqlx::query_as("SELECT revoked_reason FROM runner_session WHERE id = $1")
                .bind(sid)
                .fetch_one(&mut *tx)
                .await
                .expect("read revoked row");
        assert_eq!(reason.0, "flow-test");
        delete_runner_sessions_for_runners(&pidash_db::orm_tx::TxConn::new(&mut tx), &[runner_id])
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

    // Machine-flow filters: the device pair and the enroll machine
    // branch differ only in conjunct order; the machine branch has no
    // user filter (BUG-rotate-user-omission, ported as-is) and no
    // `LIMIT`; the legacy branch is `dev_machine_id IS NULL` by host.
    #[test]
    fn machine_flow_statements() {
        let now = sample_now();
        let id = Uuid::nil();
        let render = |filter| sql(&revoke_machine_stmt(Expr::value(now), filter));
        let prefix =
            "UPDATE \"machine_token\" SET \"revoked_at\" = '2026-11-05 10:00:00.000000 +00:00' WHERE ";
        let nil = "'00000000-0000-0000-0000-000000000000'";
        assert_eq!(
            render(device_pair_filter(id, id)),
            format!("{prefix}\"machine_token\".\"workspace_id\" = {nil} AND \"machine_token\".\"dev_machine_id\" = {nil} AND \"machine_token\".\"revoked_at\" IS NULL")
        );
        let machine = render(machine_in_workspace_filter(id, id));
        assert_eq!(
            machine,
            format!("{prefix}\"machine_token\".\"dev_machine_id\" = {nil} AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"workspace_id\" = {nil}")
        );
        assert!(!machine.contains("user_id"));
        assert!(!machine.contains("LIMIT"));
        let legacy = render(legacy_host_filter("h", id, id));
        assert_eq!(
            legacy,
            format!("{prefix}\"machine_token\".\"dev_machine_id\" IS NULL AND \"machine_token\".\"host_label\" = 'h' AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"user_id\" = {nil} AND \"machine_token\".\"workspace_id\" = {nil}")
        );
        assert!(!legacy.contains("LIMIT"));
        assert_eq!(
            render(dev_machines_filter(&[id, id])),
            format!("{prefix}\"machine_token\".\"dev_machine_id\" IN ({nil}, {nil}) AND \"machine_token\".\"revoked_at\" IS NULL")
        );
        assert_eq!(
            sql(&revoke_machine_stmt(Expr::current_timestamp().into(), by_hash("h"))),
            "UPDATE \"machine_token\" SET \"revoked_at\" = CURRENT_TIMESTAMP WHERE \"machine_token\".\"token_hash\" = 'h'"
        );
        let insert = sql(&machine_token::Entity::insert(machine_token_row(
            &NewMachineToken {
                id,
                user_id: id,
                dev_machine_id: None,
                workspace_id: id,
                host_label: "h".to_owned(),
                token_hash: "x".to_owned(),
                token_fingerprint: "f".to_owned(),
                label: "l".to_owned(),
                is_service: true,
                created_at: now,
            },
        )));
        assert_eq!(
            insert,
            format!("INSERT INTO \"machine_token\" (\"id\", \"host_label\", \"token_hash\", \"token_fingerprint\", \"label\", \"is_service\", \"created_at\", \"last_used_at\", \"revoked_at\", \"expires_at\", \"user_id\", \"workspace_id\", \"dev_machine_id\") VALUES ({nil}, 'h', 'x', 'f', 'l', TRUE, '2026-11-05 10:00:00.000000 +00:00', NULL, NULL, '2027-01-04 10:00:00.000000 +00:00', {nil}, {nil}, NULL)")
        );
        // The touch slides the 60-day idle expiry with every use.
        let touch = sql(&touch_machine_token_stmt(
            Expr::value(now),
            Expr::value(now + chrono::Duration::days(60)),
            by_id(id),
        ));
        assert_eq!(
            touch,
            format!("UPDATE \"machine_token\" SET \"last_used_at\" = '2026-11-05 10:00:00.000000 +00:00', \"expires_at\" = '2027-01-04 10:00:00.000000 +00:00' WHERE \"machine_token\".\"id\" = {nil}")
        );
    }

    #[tokio::test]
    #[should_panic(expected = "at least one machine id")]
    async fn signout_revoke_rejects_empty() {
        let db = pidash_db::pool::sea_orm_connection(
            &sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/unused")
                .expect("lazy pool"),
        );
        let _ = revoke_tokens_for_dev_machines(&db, &[], sample_now()).await;
    }

    // D6 gap PIDASHCONV-1618: the workspace revoke filters are the
    // overlay `revoke_machine_tokens` / `revoke_workspace_sessions`
    // semantics (Django's spanning `runner__workspace` filter as an
    // `IN (SELECT ...)`), pinned so drift fails loudly.
    #[test]
    fn workspace_revoke_statements() {
        let now = sample_now();
        let id = Uuid::nil();
        let nil = "'00000000-0000-0000-0000-000000000000'";
        let tokens = |user| {
            sql(&revoke_machine_stmt(
                Expr::value(now),
                workspace_tokens_filter(id, user),
            ))
        };
        assert!(tokens(Some(id)).ends_with(&format!(
            "WHERE \"machine_token\".\"workspace_id\" = {nil} AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"user_id\" = {nil}"
        )));
        assert!(tokens(None).ends_with(&format!(
            "WHERE \"machine_token\".\"workspace_id\" = {nil} AND \"machine_token\".\"revoked_at\" IS NULL"
        )));
        let sessions = |owner| {
            sql(&revoke_sessions_stmt(
                now,
                "r",
                workspace_sessions_filter(id, owner),
            ))
        };
        assert_eq!(
            sessions(Some(id)),
            format!("UPDATE \"runner_session\" SET \"revoked_at\" = '2026-11-05 10:00:00.000000 +00:00', \"revoked_reason\" = 'r' WHERE \"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" IN (SELECT \"id\" FROM \"runner\" WHERE \"runner\".\"workspace_id\" = {nil} AND \"runner\".\"owner_id\" = {nil})")
        );
        assert!(sessions(None).ends_with(&format!(
            "IN (SELECT \"id\" FROM \"runner\" WHERE \"runner\".\"workspace_id\" = {nil})"
        )));
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
            &pidash_db::orm_tx::TxConn::new(&mut tx),
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
            &pidash_db::orm_tx::TxConn::new(&mut tx),
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
        revoke_tokens_for_machine_in_workspace(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            machine_id,
            workspace_id,
            now,
        )
        .await
        .expect("revoke machine row");
        revoke_tokens_legacy_host(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            "legacy-host",
            user_id,
            workspace_id,
            now,
        )
        .await
        .expect("revoke legacy row");
        clear_token_dev_machines(&pidash_db::orm_tx::TxConn::new(&mut tx), machine_id)
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

    /// One user row for the workspace-revoke graph (the fx6 compact
    /// user column set).
    async fn seed_wsrev_user(
        ex: &mut sqlx::PgConnection,
        at: DateTime<Utc>,
        name: &str,
        email: &str,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, username, email, password, first_name, last_name, \
             display_name, avatar, created_location, last_location, last_login_ip, \
             last_login_medium, last_login_uagent, last_logout_ip, user_timezone, token, \
             is_active, is_bot, is_email_valid, is_email_verified, is_managed, \
             is_password_autoset, is_password_expired, is_password_reset_required, \
             is_staff, is_superuser, created_at, updated_at, date_joined) \
             VALUES ($1, $2, $3, '!', '', '', '', '', '', '', '', 'email', '', '', 'UTC', \
             $4, true, false, false, false, false, false, false, false, false, false, \
             $5, $5, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(email)
        .bind(Uuid::new_v4().to_string())
        .bind(at)
        .execute(&mut *ex)
        .await
        .expect("seed user");
        id
    }

    /// One workspace row for the workspace-revoke graph (the fx6
    /// workspace column set).
    async fn seed_wsrev_workspace(
        ex: &mut sqlx::PgConnection,
        at: DateTime<Utc>,
        name: &str,
        slug: &str,
        owner: Uuid,
    ) -> Uuid {
        let ws = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, background_color, \
             owner_id, slug, timezone) \
             VALUES ($1, $2, $2, $3, '#f6c8dB', $4, $5, 'UTC')",
        )
        .bind(ws)
        .bind(at)
        .bind(name)
        .bind(owner)
        .bind(slug)
        .execute(&mut *ex)
        .await
        .expect("seed workspace");
        ws
    }

    /// One runner row for the workspace-revoke graph (the runner seed
    /// column set).
    async fn seed_wsrev_runner(
        ex: &mut sqlx::PgConnection,
        at: DateTime<Utc>,
        name: &str,
        ws: Uuid,
        owner: Uuid,
        pod: Uuid,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runner (id, created_at, updated_at, owner_id, workspace_id, pod_id, \
             name, host_label, provisioning, visibility, refresh_token_hash, \
             refresh_token_fingerprint, refresh_token_generation, \
             previous_refresh_token_hash, access_token_signing_key_version, \
             enrollment_token_hash, enrollment_token_fingerprint, capabilities, status, \
             os, arch, runner_version, dev_metadata, protocol_version, revoked_reason) \
             VALUES ($1, $2, $2, $3, $4, $5, $6, '', 'desktop_bundled', 0, '', '', 0, '', \
             1, '', '', '[]', $7, '', '', '', '{}', 1, '')",
        )
        .bind(id)
        .bind(at)
        .bind(owner)
        .bind(ws)
        .bind(pod)
        .bind(name)
        .bind("offline")
        .execute(&mut *ex)
        .await
        .expect("seed runner");
        id
    }

    /// Parent rows for the workspace-revoke roundtrip: two users, two
    /// workspaces, one project + pod, and three runners
    /// (`ra`: ws1/u1, `rb`: ws1/u2, `rc`: ws2/u1, all on the one pod —
    /// the sweep reads the runner's own workspace/owner). The
    /// `runner` FKs are enforced, so random ids would not do. Names
    /// carry the tag for parallel safety.
    async fn seed_workspace_revoke_graph(
        ex: &mut sqlx::PgConnection,
        tag: &str,
    ) -> (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let at = Utc::now();
        let u1 = seed_wsrev_user(
            &mut *ex,
            at,
            &format!("wsrev-{tag}-a"),
            &format!("wsrev-{tag}-a@example.test"),
        )
        .await;
        let u2 = seed_wsrev_user(
            &mut *ex,
            at,
            &format!("wsrev-{tag}-b"),
            &format!("wsrev-{tag}-b@example.test"),
        )
        .await;
        let ws1 = seed_wsrev_workspace(
            &mut *ex,
            at,
            &format!("WSREV ws {tag}-1"),
            &format!("wsrev-ws-{tag}-1"),
            u1,
        )
        .await;
        let ws2 = seed_wsrev_workspace(
            &mut *ex,
            at,
            &format!("WSREV ws {tag}-2"),
            &format!("wsrev-ws-{tag}-2"),
            u1,
        )
        .await;
        let project = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, created_at, updated_at, name, identifier, description, \
             network, module_view, cycle_view, issue_views_view, page_view, intake_view, \
             is_time_tracking_enabled, is_issue_type_enabled, is_default, \
             guest_view_all_features, members_can_edit_states, archive_in, close_in, \
             logo_props, timezone, repo_url, base_branch, agent_default_interval_seconds, \
             agent_default_max_ticks, agent_review_default_interval_seconds, \
             agent_test_default_interval_seconds, agent_ticking_enabled, \
             default_agent_executor, workspace_id) \
             VALUES ($1, $2, $2, $3, $4, '', 2, false, false, false, true, false, false, \
             false, false, false, true, 0, 0, '{}', 'UTC', 'https://example.com/wsrev.git', \
             'main', 10800, 10, 10800, 10800, true, 'local_runner', $5)",
        )
        .bind(project)
        .bind(at)
        .bind(format!("WSREV-{tag}"))
        .bind(format!("WR{tag}"))
        .bind(ws1)
        .execute(&mut *ex)
        .await
        .expect("seed project");
        let pod = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
             project_id, workspace_id) \
             VALUES ($1, $2, $2, '', true, $3, $4, $5)",
        )
        .bind(pod)
        .bind(at)
        .bind(format!("wsrev-pod-{tag}"))
        .bind(project)
        .bind(ws1)
        .execute(&mut *ex)
        .await
        .expect("seed pod");
        let ra =
            seed_wsrev_runner(&mut *ex, at, &format!("wsrev-runner-{tag}-a"), ws1, u1, pod).await;
        let rb =
            seed_wsrev_runner(&mut *ex, at, &format!("wsrev-runner-{tag}-b"), ws1, u2, pod).await;
        let rc =
            seed_wsrev_runner(&mut *ex, at, &format!("wsrev-runner-{tag}-c"), ws2, u1, pod).await;
        (u1, u2, ws1, ws2, ra, rb, rc)
    }

    /// One machine-token row for the workspace-revoke roundtrip.
    async fn seed_wsrev_token(
        ex: &mut sqlx::PgConnection,
        tag: &str,
        suffix: &str,
        ws: Uuid,
        user: Uuid,
        created_at: DateTime<Utc>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        insert_machine_token(
            &pidash_db::orm_tx::TxConn::new(&mut *ex),
            &NewMachineToken {
                id,
                user_id: user,
                dev_machine_id: None,
                workspace_id: ws,
                host_label: format!("wsrev-{tag}-{suffix}"),
                token_hash: format!("wsrev-hash-{tag}-{suffix}"),
                token_fingerprint: format!("wsrev-fp-{suffix}"),
                label: format!("wsrev-{suffix}"),
                is_service: true,
                created_at,
            },
        )
        .await
        .expect("seed token");
        id
    }

    /// One live session row for the workspace-revoke roundtrip.
    async fn seed_wsrev_session(ex: &mut sqlx::PgConnection, runner: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        open_runner_session(&pidash_db::orm_tx::TxConn::new(&mut *ex), id, runner, 4)
            .await
            .expect("seed session");
        id
    }

    /// Live-db half: the workspace revoke scoping. The user/owner arms
    /// stamp only their rows, the whole-workspace arms sweep the rest,
    /// pre-revoked rows keep their stamps, and the other workspace is
    /// untouched. Fixed whole-second stamps so the read-backs compare
    /// exactly — all inside a rolled-back transaction.
    #[tokio::test]
    async fn workspace_revoke_roundtrip() {
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
        let uniq = Uuid::new_v4().simple().to_string();
        let tag = &uniq[..8];
        let (u1, u2, ws1, ws2, ra, rb, rc) = seed_workspace_revoke_graph(&mut tx, tag).await;
        let old = DateTime::from_timestamp(1_700_000_000, 0).expect("fixed stamp");
        let now1 = DateTime::from_timestamp(1_780_000_001, 0).expect("fixed stamp");
        let now2 = DateTime::from_timestamp(1_780_000_002, 0).expect("fixed stamp");
        let t1 = seed_wsrev_token(&mut tx, tag, "t1", ws1, u1, old).await;
        let t2 = seed_wsrev_token(&mut tx, tag, "t2", ws1, u2, old).await;
        let t3 = seed_wsrev_token(&mut tx, tag, "t3", ws2, u1, old).await;
        let t4 = seed_wsrev_token(&mut tx, tag, "t4", ws1, u1, old).await;
        revoke_machine_token_by_id(&pidash_db::orm_tx::TxConn::new(&mut tx), t4, old)
            .await
            .expect("pre-revoke token");
        // The pre-revoked session goes first: only one live session
        // per runner (`runner_session_one_active_per_runner`).
        let sa2 = seed_wsrev_session(&mut tx, ra).await;
        revoke_runner_session(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            sa2,
            old,
            "old-reason",
        )
        .await
        .expect("pre-revoke session");
        let sa = seed_wsrev_session(&mut tx, ra).await;
        let sb = seed_wsrev_session(&mut tx, rb).await;
        let sc = seed_wsrev_session(&mut tx, rc).await;

        revoke_machine_tokens_by_workspace(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            ws1,
            Some(u1),
            now1,
        )
        .await
        .expect("revoke tokens scoped");
        revoke_runner_sessions_by_workspace(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            ws1,
            Some(u1),
            now1,
            "membership_revoked",
        )
        .await
        .expect("revoke sessions scoped");
        let tokens: Vec<(Uuid, Option<DateTime<Utc>>)> =
            sqlx::query_as("SELECT id, revoked_at FROM machine_token WHERE id IN ($1, $2, $3, $4)")
                .bind(t1)
                .bind(t2)
                .bind(t3)
                .bind(t4)
                .fetch_all(&mut *tx)
                .await
                .expect("read tokens");
        let revoked = |id: Uuid| {
            tokens
                .iter()
                .find(|(row, _)| *row == id)
                .unwrap_or_else(|| panic!("token row {id} present"))
                .1
        };
        assert_eq!(revoked(t1), Some(now1), "scoped arm stamps its row");
        assert_eq!(revoked(t2), None, "scoped arm skips other user");
        assert_eq!(revoked(t3), None, "scoped arm skips other workspace");
        assert_eq!(revoked(t4), Some(old), "scoped arm keeps prior stamp");
        let sessions: Vec<(Uuid, Option<DateTime<Utc>>, String)> = sqlx::query_as(
            "SELECT id, revoked_at, revoked_reason FROM runner_session WHERE id IN ($1, $2, $3, $4)",
        )
        .bind(sa)
        .bind(sb)
        .bind(sc)
        .bind(sa2)
        .fetch_all(&mut *tx)
        .await
        .expect("read sessions");
        let swept = |id: Uuid| {
            sessions
                .iter()
                .find(|(row, _, _)| *row == id)
                .unwrap_or_else(|| panic!("session row {id} present"))
                .clone()
        };
        assert_eq!(
            (swept(sa).1, swept(sa).2.as_str()),
            (Some(now1), "membership_revoked"),
            "owner arm stamps its runner's session"
        );
        assert_eq!(swept(sb).1, None, "owner arm skips other owner's session");
        assert_eq!(swept(sc).1, None, "owner arm skips other workspace");
        assert_eq!(
            (swept(sa2).1, swept(sa2).2.as_str()),
            (Some(old), "old-reason"),
            "owner arm keeps prior stamp and reason"
        );

        revoke_machine_tokens_by_workspace(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            ws1,
            None,
            now2,
        )
        .await
        .expect("revoke tokens sweep");
        revoke_runner_sessions_by_workspace(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            ws1,
            None,
            now2,
            "membership_revoked",
        )
        .await
        .expect("revoke sessions sweep");
        let tokens: Vec<(Uuid, Option<DateTime<Utc>>)> =
            sqlx::query_as("SELECT id, revoked_at FROM machine_token WHERE id IN ($1, $2, $3, $4)")
                .bind(t1)
                .bind(t2)
                .bind(t3)
                .bind(t4)
                .fetch_all(&mut *tx)
                .await
                .expect("read tokens");
        let revoked = |id: Uuid| {
            tokens
                .iter()
                .find(|(row, _)| *row == id)
                .unwrap_or_else(|| panic!("token row {id} present"))
                .1
        };
        assert_eq!(revoked(t1), Some(now1), "sweep keeps first stamp");
        assert_eq!(revoked(t2), Some(now2), "sweep stamps remaining row");
        assert_eq!(revoked(t3), None, "sweep skips other workspace");
        assert_eq!(revoked(t4), Some(old), "sweep keeps prior stamp");
        let sessions: Vec<(Uuid, Option<DateTime<Utc>>, String)> = sqlx::query_as(
            "SELECT id, revoked_at, revoked_reason FROM runner_session WHERE id IN ($1, $2, $3, $4)",
        )
        .bind(sa)
        .bind(sb)
        .bind(sc)
        .bind(sa2)
        .fetch_all(&mut *tx)
        .await
        .expect("read sessions");
        let swept = |id: Uuid| {
            sessions
                .iter()
                .find(|(row, _, _)| *row == id)
                .unwrap_or_else(|| panic!("session row {id} present"))
                .clone()
        };
        assert_eq!(
            (swept(sa).1, swept(sa).2.as_str()),
            (Some(now1), "membership_revoked"),
            "sweep keeps first stamp"
        );
        assert_eq!(
            (swept(sb).1, swept(sb).2.as_str()),
            (Some(now2), "membership_revoked"),
            "sweep stamps remaining session"
        );
        assert_eq!(swept(sc).1, None, "sweep skips other workspace");
        assert_eq!(
            (swept(sa2).1, swept(sa2).2.as_str()),
            (Some(old), "old-reason"),
            "sweep keeps prior stamp and reason"
        );
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
            &pidash_db::pool::sea_orm_connection(&pool),
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
            &pidash_db::pool::sea_orm_connection(&pool),
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
            &pidash_db::pool::sea_orm_connection(&pool),
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
        touch_api_token_last_used_by_token(
            &pidash_db::pool::sea_orm_connection(&pool),
            &issued.token,
        )
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
            &pidash_db::pool::sea_orm_connection(&pool),
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
        deactivate_api_token_by_id(
            &pidash_db::pool::sea_orm_connection(&pool),
            issued.id,
            Utc::now(),
        )
        .await
        .expect("deactivate");
        let (is_active,): (bool,) =
            sqlx::query_as("SELECT is_active FROM api_tokens WHERE id = $1")
                .bind(issued.id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!is_active);
        soft_delete_token(
            &pidash_db::pool::sea_orm_connection(&pool),
            issued.id,
            user_id,
        )
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
