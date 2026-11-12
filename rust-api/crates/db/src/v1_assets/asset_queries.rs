//! User + generic asset lookups (PIDASHCONV-409, D-21 stage 5).
//!
//! Ports query units 1-3 of `apps/api/pi_dash/api/views/asset.py` to
//! sea-query builders (drift baseline `01a93e17`):
//!
//! * Unit 1 — `asset_delete` soft-delete helper
//!   (`asset.py:51-58`, server twin `:249-256`): `filter(id).first()`
//!   select plus the `save(update_fields=["is_deleted", "deleted_at"])`
//!   update. The server twin is byte-identical SQL, so one shared
//!   builder covers both; the user-vs-server credential flag lives in
//!   the handler issue.
//! * Unit 2 — `entity_asset_delete` user lookups (`:60-73`, server twin
//!   `:258-271`): `User.objects.get(id=asset.user_id)` plus the
//!   full-row `user.save()` that clears `avatar_asset_id` (`USER_AVATAR`)
//!   or `cover_image_asset_id` (`USER_COVER`); any other entity type is
//!   a no-op that leaves the profile untouched.
//! * Unit 3 — generic asset lookups: `get(id, workspace__slug,
//!   is_deleted=False)` (`:430` get, `:608` patch — one shared builder),
//!   the external-dedupe
//!   `filter(workspace__slug, external_source, external_id,
//!   is_deleted=False).first()` (`:534-540`), and the `is_uploaded`
//!   guard (handler-owned, `fx-h-asset-generic` — noted, not built).
//!
//! Fixture oracle: `fixtures/v1_assets/fx-q-asset.json`. Every shape
//! below was checked against live Django 4.2 `str(queryset.query)`
//! output (test settings); the unit tests pin the fragments so
//! transcription drift fails the build.
//!
//! # Statement contract
//!
//! Each `*_select` / `*_update` function returns a complete sea-query
//! statement with bound parameters (no `Expr::cust` placeholders);
//! execution stays handler-owned via [`ConnectionTrait`] on the
//! `sea_orm_connection` wrapper, or through the typed `fetch_*`
//! helpers below. Reads scope the physical tables with explicit
//! `deleted_at IS NULL` conjuncts (matching Django's SQL exactly)
//! rather than the `<table>_active` views; writes hit the tables.
//!
//! `.get()` ports carry no `ORDER BY` (Django's `get()` clears the
//! default ordering) and no `LIMIT`: Django sets `LIMIT 21`
//! (`MAX_GET_RESULTS`), which the fixture elides and which returns the
//! same row as no limit on these unique predicates (same reasoning as
//! the merged `app_integrations/queries_webhook.rs` precedent).
//! `.first()` ports keep `ORDER BY "…"."created_at" DESC LIMIT 1`
//! (the `FileAsset` `Meta.ordering`, `asset.py:68`).
//!
//! # Ported bugs and corrections (translate, don't redesign)
//!
//! 1. The user patch/delete `get(id, user_id)` (`:210,:237,:366,:394`)
//!    carries **no** `is_deleted` predicate: patch flips `is_uploaded`
//!    on an already-soft-deleted row and delete re-saves one (delete
//!    stays 204 on the wire). Ported as observed.
//! 2. `filter(id).first()` likewise has no `is_deleted=False` filter,
//!    so `asset_delete` re-deletes deleted rows (bumping `deleted_at`)
//!    and returns `None` silently when the id is unknown (callers
//!    ignore the return; the wire stays 204). Ported as observed.
//! 3. `is_deleted=False` renders as `NOT "…"."is_deleted"`, not
//!    `= false` (live Django 4.2 text; `is_deleted` is `NOT NULL`, so
//!    the two coincide). Ported as rendered.
//! 4. CORRECTION against the fixture: `fx-q-asset.json`
//!    `entity_asset_delete_lookups.sql[0]` claims `User.objects.get`
//!    renders `("users"."deleted_at" IS NULL AND …)` via a
//!    "soft-delete manager". Live Django 4.2 renders **no** `deleted_at`
//!    predicate: `User` (`db/models/user.py:56`) extends Django's
//!    `AbstractBaseUser`/`PermissionsMixin` only, and `objects` is
//!    Django's own `UserManager` — there is no soft-delete manager and
//!    no `deleted_at` column on `users`. [`entity_user_select_sql`]
//!    ports the live truth. The fixture line needs a one-line fix in a
//!    follow-up (fixture file owned by PIDASHCONV-375, already merged).
//!
//! Out of scope (sibling D-21 issues): the `is_uploaded` 400 guard and
//! every response body (handlers, PIDASHCONV-419/421/423/426), the
//! `size=int(…)`-before-required-check 500 (recorded in
//! `fx-h-asset-generic`; handler-owned), permission gates
//! (PIDASHCONV-415), and metadata task enqueues (tasks issue).

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, FromQueryResult};
use sea_query::{Alias, Condition, Expr, JoinType, Order, Query};

use super::model::file_asset;
use crate::entity::{file_assets, users};

/// `users` table (no db-layer port owns it yet; literal matches the
/// Django table name, `db/models/user.py:136`).
const USER_TABLE: &str = "users";
/// `workspaces` table (no db-layer port owns it yet; literal matches
/// the Django table name, same as `queries_stateest::WORKSPACE_TABLE`).
const WORKSPACE_TABLE: &str = "workspaces";

/// `users` columns in Django `_meta` field order, captured from live
/// Django 4.2 `str(User.objects.filter(id=…).query)` output:
/// `AbstractBaseUser` (`password`, `last_login`), the pk `id`, then
/// `user.py:56-133` declaration order (`avatar_asset_id` /
/// `cover_image_asset_id` are the `avatar_asset` / `cover_image_asset`
/// FK attnames). There is no `deleted_at` column (see the module docs,
/// correction 4).
pub const USER_COLUMNS: &[&str] = &[
    "password",
    "last_login",
    "id",
    "username",
    "mobile_number",
    "email",
    "display_name",
    "first_name",
    "last_name",
    "avatar",
    "avatar_asset_id",
    "cover_image",
    "cover_image_asset_id",
    "date_joined",
    "created_at",
    "updated_at",
    "last_location",
    "created_location",
    "is_superuser",
    "is_managed",
    "is_password_expired",
    "is_active",
    "is_staff",
    "is_email_verified",
    "is_password_autoset",
    "is_password_reset_required",
    "token",
    "last_active",
    "last_login_time",
    "last_logout_time",
    "last_login_ip",
    "last_logout_ip",
    "last_login_medium",
    "last_login_uagent",
    "token_updated_at",
    "is_bot",
    "bot_type",
    "user_timezone",
    "is_email_valid",
    "masked_at",
];

/// `USER_AVATAR` entity type (`db/models/asset.py:38`,
/// [`file_asset::ENTITY_TYPES`]).
pub const USER_AVATAR: &str = "USER_AVATAR";
/// `USER_COVER` entity type (`db/models/asset.py:39`).
pub const USER_COVER: &str = "USER_COVER";

/// Column cleared by [`entity_clear_target`]: the `users` FK nulled
/// before the full-row save, or no-op for unhandled entity types
/// (`asset.py:60-73` fall-through `return`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityClearTarget {
    /// `user.avatar_asset_id = None` (`USER_AVATAR`).
    AvatarAssetId,
    /// `user.cover_image_asset_id = None` (`USER_COVER`).
    CoverImageAssetId,
    /// Any other entity type (including `None`): profile untouched.
    NoOp,
}

/// `entity_asset_delete` branch rule (`asset.py:60-73`, server twin
/// `:258-271`): `USER_AVATAR` clears the avatar FK, `USER_COVER`
/// clears the cover FK, everything else is a silent no-op. Pure
/// function over the stored `entity_type` string; the SQL side is
/// [`entity_user_select`] + [`fetch_entity_user`] (the save half is the
/// services-owned `entity_user_save_sql`).
pub fn entity_clear_target(entity_type: Option<&str>) -> EntityClearTarget {
    match entity_type {
        Some(USER_AVATAR) => EntityClearTarget::AvatarAssetId,
        Some(USER_COVER) => EntityClearTarget::CoverImageAssetId,
        _ => EntityClearTarget::NoOp,
    }
}

/// Project `columns` of `table` (`SELECT "t"."c", …`, same helper
/// shape as the merged `module_queries` / `queries_stateest`
/// precedent). Projection order follows the crate's `COLUMNS` consts;
/// live Django emits the same column *set* in `_meta` order
/// (`created_at` first, `id` sixth) — no row-semantic difference, and
/// the fixture records `<cols>` placeholders precisely to avoid
/// pinning the order.
fn select_table_columns(sel: &mut sea_query::SelectStatement, table: &str, columns: &[&str]) {
    for col in columns {
        sel.column((Alias::new(table.to_owned()), Alias::new((*col).to_owned())));
    }
}

/// `INNER JOIN "workspaces" ON ("<table>"."workspace_id" =
/// "workspaces"."id")` (every `workspace__slug` traversal).
fn join_workspace(sel: &mut sea_query::SelectStatement, table: &str) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(table.to_owned()), Alias::new("workspace_id")))
                .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
}

/// Soft-delete-manager scope both asset reads share:
/// `"file_assets"."deleted_at" IS NULL`.
fn asset_alive() -> sea_query::SimpleExpr {
    Expr::col((
        Alias::new(file_asset::TABLE.to_owned()),
        Alias::new("deleted_at"),
    ))
    .is_null()
}

/// `asset_delete` select (`asset.py:51`, server twin `:249`):
/// `FileAsset.objects.filter(id=asset_id).first()` — manager scope
/// only, no `is_deleted` filter (ported bug 2), default ordering, one
/// row. `None` (unknown id) is the caller's silent no-op.
///
/// Binds: `$1` asset id.
pub fn soft_delete_select(asset_id: uuid::Uuid) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(asset_id)),
    );
    sel.order_by(
        (
            Alias::new(file_asset::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel.limit(1);
    sel
}

/// `asset_delete` write (`asset.py:55-57`, server twin `:253-255`):
/// `is_deleted=True`, `deleted_at=now`, then
/// `save(update_fields=["is_deleted", "deleted_at"])`. Django sends
/// both columns as parameters in `update_fields` order.
///
/// Binds: `$1` `deleted_at` (`timezone.now()`), `$2` asset id
/// (`is_deleted` is always true on this path, so it binds inline).
pub fn soft_delete_update(
    deleted_at: DateTime<Utc>,
    asset_id: uuid::Uuid,
) -> sea_query::UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(file_asset::TABLE.to_owned()))
        .value(Alias::new("is_deleted"), Expr::value(true))
        .value(Alias::new("deleted_at"), Expr::value(deleted_at))
        .and_where(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(asset_id));
    stmt
}

/// Typed full-row form of [`soft_delete_select`]: `None` on unknown id.
pub async fn fetch_soft_delete(
    db: &DatabaseConnection,
    asset_id: uuid::Uuid,
) -> Result<Option<file_assets::Model>, DbErr> {
    let stmt = soft_delete_select(asset_id);
    match db.query_one(DbBackend::Postgres.build(&stmt)).await? {
        Some(row) => file_assets::Model::from_query_result(&row, "").map(Some),
        None => Ok(None),
    }
}

/// Typed form of [`soft_delete_update`]: flips `is_deleted` and stamps
/// `deleted_at` for one asset.
pub async fn soft_delete_save(
    db: &DatabaseConnection,
    deleted_at: DateTime<Utc>,
    asset_id: uuid::Uuid,
) -> Result<(), DbErr> {
    let stmt = soft_delete_update(deleted_at, asset_id);
    db.execute(DbBackend::Postgres.build(&stmt)).await?;
    Ok(())
}

/// User patch/delete lookup (`asset.py:210`, `:237`, server twins
/// `:366`, `:394`): `FileAsset.objects.get(id=asset_id,
/// user_id=request.user.id)` — manager scope plus both exact
/// predicates, deliberately **no** `is_deleted` filter (ported bug 1).
/// `DoesNotExist` → 404 is handler-owned (`BaseAPIView`,
/// `views/base.py:154-159`).
///
/// Binds: `$1` asset id, `$2` acting-user id.
pub fn user_scoped_get(asset_id: uuid::Uuid, user_id: uuid::Uuid) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(asset_id))
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("user_id"))).eq(user_id)),
    );
    sel
}

/// Typed full-row form of [`user_scoped_get`]: `None` on miss.
pub async fn fetch_user_scoped(
    db: &DatabaseConnection,
    asset_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<file_assets::Model>, DbErr> {
    let stmt = user_scoped_get(asset_id, user_id);
    match db.query_one(DbBackend::Postgres.build(&stmt)).await? {
        Some(row) => file_assets::Model::from_query_result(&row, "").map(Some),
        None => Ok(None),
    }
}

/// `entity_asset_delete` user fetch (`asset.py:62,68`, server twins
/// `:260,:266`): `User.objects.get(id=asset.user_id)`. Plain pk
/// lookup — live Django renders no `deleted_at` predicate and no
/// ordering (see module-docs correction 4).
///
/// Binds: `$1` user id (`asset.user_id`).
pub fn entity_user_select(user_id: uuid::Uuid) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, USER_TABLE, USER_COLUMNS);
    sel.from(Alias::new(USER_TABLE.to_owned()));
    sel.cond_where(
        Condition::all().add(Expr::col((Alias::new(USER_TABLE), Alias::new("id"))).eq(user_id)),
    );
    sel
}

/// Typed full-row form of [`entity_user_select`]: `None` on miss.
pub async fn fetch_entity_user(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
) -> Result<Option<users::Model>, DbErr> {
    let stmt = entity_user_select(user_id);
    match db.query_one(DbBackend::Postgres.build(&stmt)).await? {
        Some(row) => users::Model::from_query_result(&row, "").map(Some),
        None => Ok(None),
    }
}

// NOTE: the `entity_user_save_sql` builder lived here; it moved to
// `pidash_services::domain::identity` (PIDASHCONV-1631), whose tests
// carry the pin. `USER_COLUMNS` stays: the select builder and the
// relocated save builder share it.

/// Generic get + patch lookup (`asset.py:430`, `:608` — one shared
/// builder): `FileAsset.objects.get(id=asset_id,
/// workspace__slug=slug, is_deleted=False)`. `DoesNotExist` → 404
/// `{"error": "Asset not found"}` and the `is_uploaded` 400 guard are
/// handler-owned (`fx-h-asset-generic`).
///
/// Binds: `$1` asset id, `$2` workspace slug.
pub fn generic_detail_select(asset_id: uuid::Uuid, slug: &str) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    join_workspace(&mut sel, file_asset::TABLE);
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(asset_id))
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("is_deleted"))).not())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(slug)),
    );
    sel
}

/// Typed full-row form of [`generic_detail_select`]: `None` on miss.
pub async fn fetch_generic_detail(
    db: &DatabaseConnection,
    asset_id: uuid::Uuid,
    slug: &str,
) -> Result<Option<file_assets::Model>, DbErr> {
    let stmt = generic_detail_select(asset_id, slug);
    match db.query_one(DbBackend::Postgres.build(&stmt)).await? {
        Some(row) => file_assets::Model::from_query_result(&row, "").map(Some),
        None => Ok(None),
    }
}

/// External-dedupe lookup (`asset.py:534-540`): only reached when
/// `external_id` **and** `external_source` are both truthy (Python
/// `and` — empty strings skip the check; handler-owned).
/// `filter(workspace__slug, external_source, external_id,
/// is_deleted=False).first()`; a hit → 409 with the existing row's id
/// and `asset_url`, a miss (`None`) → proceed to create.
///
/// Binds: `$1` external id, `$2` external source, `$3` workspace slug.
pub fn external_dedupe_select(
    external_id: &str,
    external_source: &str,
    slug: &str,
) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    join_workspace(&mut sel, file_asset::TABLE);
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(
                Expr::col((Alias::new(file_asset::TABLE), Alias::new("external_id")))
                    .eq(external_id),
            )
            .add(
                Expr::col((Alias::new(file_asset::TABLE), Alias::new("external_source")))
                    .eq(external_source),
            )
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("is_deleted"))).not())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(slug)),
    );
    sel.order_by(
        (
            Alias::new(file_asset::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel.limit(1);
    sel
}

/// Typed full-row form of [`external_dedupe_select`]: `None` on miss.
pub async fn fetch_external_dedupe(
    db: &DatabaseConnection,
    external_id: &str,
    external_source: &str,
    slug: &str,
) -> Result<Option<file_assets::Model>, DbErr> {
    let stmt = external_dedupe_select(external_id, external_source, slug);
    match db.query_one(DbBackend::Postgres.build(&stmt)).await? {
        Some(row) => file_assets::Model::from_query_result(&row, "").map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn render(sel: &sea_query::SelectStatement) -> String {
        sel.to_string(PostgresQueryBuilder)
    }

    fn render_update(stmt: &sea_query::UpdateStatement) -> String {
        stmt.to_string(PostgresQueryBuilder)
    }

    fn asset_id() -> uuid::Uuid {
        uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
    }

    fn user_id() -> uuid::Uuid {
        uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()
    }

    fn asset_cols_fragment() -> String {
        file_asset::COLUMNS
            .iter()
            .map(|c| format!("\"{}\".\"{}\"", file_asset::TABLE, c))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn soft_delete_select_matches_fixture_shape() {
        let sql = render(&soft_delete_select(asset_id()));
        // Projection + manager scope (fx-q-asset soft_delete_helper).
        assert!(sql.contains(&asset_cols_fragment()), "{sql}");
        assert!(
            sql.contains("FROM \"file_assets\" WHERE \"file_assets\".\"deleted_at\" IS NULL"),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"id\" = '22222222-2222-2222-2222-222222222222'"),
            "{sql}"
        );
        // No is_deleted predicate (ported bug 2; the projection still
        // selects the column), default ordering, one row.
        assert!(!sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(!sql.contains("\"is_deleted\" ="), "{sql}");
        assert!(
            sql.contains("ORDER BY \"file_assets\".\"created_at\" DESC LIMIT 1"),
            "{sql}"
        );
    }

    #[test]
    fn soft_delete_update_sets_two_fields_by_id() {
        let sql = render_update(&soft_delete_update(
            chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
            asset_id(),
        ));
        assert!(
            sql.contains("UPDATE \"file_assets\" SET \"is_deleted\" = TRUE, \"deleted_at\" ="),
            "{sql}"
        );
        assert!(
            sql.contains("WHERE \"file_assets\".\"id\" = '22222222-2222-2222-2222-222222222222'"),
            "{sql}"
        );
    }

    #[test]
    fn user_scoped_get_has_no_soft_delete_filter() {
        let sql = render(&user_scoped_get(asset_id(), user_id()));
        assert!(
            sql.contains("\"file_assets\".\"id\" = '22222222-2222-2222-2222-222222222222'"),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"user_id\" = '33333333-3333-3333-3333-333333333333'"),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"deleted_at\" IS NULL"),
            "{sql}"
        );
        // Ported bug 1: no is_deleted predicate (projection still
        // selects the column); get() clears ordering.
        assert!(!sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(!sql.contains("\"is_deleted\" ="), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn entity_user_select_is_plain_pk_lookup() {
        let sql = render(&entity_user_select(user_id()));
        // Live Django renders no deleted_at predicate here (module-docs
        // correction 4) and no ordering/limit on the get() path.
        assert!(
            sql.contains(
                "FROM \"users\" WHERE \"users\".\"id\" = '33333333-3333-3333-3333-333333333333'"
            ),
            "{sql}"
        );
        assert!(!sql.contains("deleted_at"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        // Spot-check the captured _meta column order (password, pk,
        // avatar/cover FK attnames).
        for col in [
            "\"users\".\"password\"",
            "\"users\".\"avatar_asset_id\"",
            "\"users\".\"cover_image_asset_id\"",
            "\"users\".\"masked_at\"",
        ] {
            assert!(sql.contains(col), "{sql}");
        }
        assert_eq!(USER_COLUMNS.len(), 40, "users column count drift");
    }

    #[test]
    fn entity_clear_target_branches_like_python() {
        assert_eq!(
            entity_clear_target(Some("USER_AVATAR")),
            EntityClearTarget::AvatarAssetId
        );
        assert_eq!(
            entity_clear_target(Some("USER_COVER")),
            EntityClearTarget::CoverImageAssetId
        );
        // Anything else — including None and the seven other entity
        // types — leaves the profile untouched.
        for other in [
            None,
            Some("ISSUE_ATTACHMENT"),
            Some("WORKSPACE_LOGO"),
            Some(""),
        ] {
            assert_eq!(entity_clear_target(other), EntityClearTarget::NoOp);
        }
    }

    #[test]
    fn generic_detail_select_joins_workspace_with_not_deleted() {
        let sql = render(&generic_detail_select(asset_id(), "acme"));
        assert!(
            sql.contains(
                "INNER JOIN \"workspaces\" ON \"file_assets\".\"workspace_id\" = \"workspaces\".\"id\""
            ),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"id\" = '22222222-2222-2222-2222-222222222222'"),
            "{sql}"
        );
        // Live Django renders is_deleted=False as NOT (ported bug 3).
        assert!(sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = 'acme'"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn external_dedupe_select_limits_to_newest_hit() {
        let sql = render(&external_dedupe_select("ext-1", "s3", "acme"));
        assert!(
            sql.contains("\"file_assets\".\"external_id\" = 'ext-1'"),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"external_source\" = 's3'"),
            "{sql}"
        );
        assert!(sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = 'acme'"), "{sql}");
        assert!(
            sql.contains("ORDER BY \"file_assets\".\"created_at\" DESC LIMIT 1"),
            "{sql}"
        );
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.file_assets')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        // Session-local copies without keys or constraints, so the test
        // needs no seeded parents; they shadow the real tables on this
        // single session. No ON COMMIT DROP: there is no explicit
        // transaction, so the tables would drop at the first commit.
        for table in ["file_assets", "workspaces", "users"] {
            let sql =
                format!("CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA");
            sqlx::query(&sql).execute(&pool).await.expect("temp table");
        }
        Some(pool)
    }

    /// The `file_assets` + `workspaces` lookups against the shadow
    /// tables: hits, slug/id misses and the dedupe miss. Skips without
    /// `DATABASE_URL`.
    #[tokio::test]
    async fn live_file_asset_lookups() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = crate::pool::sea_orm_connection(&pool);
        let ws_id = uuid::Uuid::new_v4();
        let asset_id = uuid::Uuid::new_v4();
        let owner_id = uuid::Uuid::new_v4();
        let now = Utc::now();
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'acme')")
            .bind(ws_id)
            .execute(&pool)
            .await
            .expect("seed workspace");
        sqlx::query(
            "INSERT INTO file_assets (id, created_at, updated_at, attributes, asset,
                 user_id, workspace_id, is_deleted, is_archived, external_id,
                 external_source, size, is_uploaded)
             VALUES ($1, $2, $2, '{}', 'k', $3, $4, false, false, 'e1', 's3', 1.5, true)",
        )
        .bind(asset_id)
        .bind(now)
        .bind(owner_id)
        .bind(ws_id)
        .execute(&pool)
        .await
        .expect("seed asset");
        let detail = fetch_generic_detail(&db, asset_id, "acme")
            .await
            .expect("detail")
            .expect("hit");
        assert_eq!(detail.id, asset_id);
        assert_eq!(detail.asset, "k");
        assert!(detail.is_uploaded);
        assert_eq!(
            fetch_generic_detail(&db, asset_id, "other")
                .await
                .expect("detail"),
            None
        );
        assert_eq!(
            fetch_generic_detail(&db, uuid::Uuid::new_v4(), "acme")
                .await
                .expect("detail"),
            None
        );
        let dedupe = fetch_external_dedupe(&db, "e1", "s3", "acme")
            .await
            .expect("dedupe")
            .expect("hit");
        assert_eq!(dedupe.id, asset_id);
        assert_eq!(
            fetch_external_dedupe(&db, "nope", "s3", "acme")
                .await
                .expect("dedupe"),
            None
        );
        let scoped = fetch_user_scoped(&db, asset_id, owner_id)
            .await
            .expect("scoped")
            .expect("hit");
        assert_eq!(scoped.user_id, Some(owner_id));
        assert_eq!(
            fetch_user_scoped(&db, asset_id, uuid::Uuid::new_v4())
                .await
                .expect("scoped"),
            None
        );
        let soft = fetch_soft_delete(&db, asset_id)
            .await
            .expect("soft")
            .expect("hit");
        assert_eq!(soft.id, asset_id);
        soft_delete_save(&db, now, asset_id).await.expect("save");
        // The manager scope now excludes the row.
        assert_eq!(fetch_soft_delete(&db, asset_id).await.expect("soft"), None);
        let flagged: bool = sqlx::query_scalar("SELECT is_deleted FROM file_assets WHERE id = $1")
            .bind(asset_id)
            .fetch_one(&pool)
            .await
            .expect("flag");
        assert!(flagged);
    }

    /// The `users` by-pk lookup against the shadow table: hit + miss.
    /// Skips without `DATABASE_URL`.
    #[tokio::test]
    async fn live_fetch_entity_user() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = crate::pool::sea_orm_connection(&pool);
        let id = uuid::Uuid::new_v4();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO users (password, id, username, first_name, last_name,
                 avatar, date_joined, created_at, updated_at, last_location,
                 created_location, is_superuser, is_managed, is_password_expired,
                 is_active, is_staff, is_email_verified, is_password_autoset,
                 token, user_timezone, last_login_ip, last_logout_ip,
                 last_login_medium, last_login_uagent, is_bot, display_name,
                 is_email_valid, is_password_reset_required)
             VALUES ('x', $1, 'u', 'f', 'l', '', $2, $2, $2, '', '', false, false,
                     false, true, false, false, false, 't', 'UTC', '', '', '', '',
                     false, 'd', true, false)",
        )
        .bind(id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed user");
        let hit = fetch_entity_user(&db, id)
            .await
            .expect("lookup")
            .expect("hit");
        assert_eq!(hit.id, id);
        assert_eq!(hit.username, "u");
        assert_eq!(hit.display_name, "d");
        assert!(hit.is_active);
        assert_eq!(
            fetch_entity_user(&db, uuid::Uuid::new_v4())
                .await
                .expect("lookup"),
            None
        );
    }
}
