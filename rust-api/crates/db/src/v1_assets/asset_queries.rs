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
//! # Builder contract
//!
//! Each `*_sql` function returns a complete statement with symbolic
//! bind parameters (`$1`, `$2`, … via `Expr::cust`, following the
//! merged `v1_cycles_modules/module_queries.rs` precedent). Handlers
//! bind them in the documented order and execute via sqlx; there is no
//! build-time database, so `sqlx::query!` macros cannot apply here —
//! execution stays handler-owned, as in the D-20 query port. Reads
//! scope the physical tables with explicit `deleted_at IS NULL`
//! conjuncts (matching Django's SQL exactly) rather than the
//! `<table>_active` views; writes hit the tables.
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

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query};

use super::model::file_asset;

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
/// [`entity_user_select_sql`] + [`entity_user_save_sql`].
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
pub fn soft_delete_select_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(Expr::cust("$1"))),
    );
    sel.order_by(
        (
            Alias::new(file_asset::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// `asset_delete` write (`asset.py:55-57`, server twin `:253-255`):
/// `is_deleted=True`, `deleted_at=now`, then
/// `save(update_fields=["is_deleted", "deleted_at"])`. Django sends
/// both columns as parameters in `update_fields` order.
///
/// Binds: `$1` is_deleted (always true on this path), `$2`
/// `deleted_at` (`timezone.now()`), `$3` asset id.
pub fn soft_delete_update_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut stmt = Query::update();
    stmt.table(Alias::new(file_asset::TABLE.to_owned()))
        .value(Alias::new("is_deleted"), Expr::cust("$1"))
        .value(Alias::new("deleted_at"), Expr::cust("$2"))
        .and_where(
            Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(Expr::cust("$3")),
        );
    stmt.to_string(PostgresQueryBuilder)
}

/// User patch/delete lookup (`asset.py:210`, `:237`, server twins
/// `:366`, `:394`): `FileAsset.objects.get(id=asset_id,
/// user_id=request.user.id)` — manager scope plus both exact
/// predicates, deliberately **no** `is_deleted` filter (ported bug 1).
/// `DoesNotExist` → 404 is handler-owned (`BaseAPIView`,
/// `views/base.py:154-159`).
///
/// Binds: `$1` asset id, `$2` acting-user id.
pub fn user_scoped_get_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(Expr::cust("$1")))
            .add(
                Expr::col((Alias::new(file_asset::TABLE), Alias::new("user_id")))
                    .eq(Expr::cust("$2")),
            ),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// `entity_asset_delete` user fetch (`asset.py:62,68`, server twins
/// `:260,:266`): `User.objects.get(id=asset.user_id)`. Plain pk
/// lookup — live Django renders no `deleted_at` predicate and no
/// ordering (see module-docs correction 4).
///
/// Binds: `$1` user id (`asset.user_id`).
pub fn entity_user_select_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, USER_TABLE, USER_COLUMNS);
    sel.from(Alias::new(USER_TABLE.to_owned()));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(USER_TABLE), Alias::new("id"))).eq(Expr::cust("$1"))),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// `entity_asset_delete` user write (`asset.py:63-64,69-70`, server
/// twins `:261-262,:267-268`): `user.save()` with no `update_fields`
/// is a **full-row** save — every non-pk column in `_meta` order, pk
/// in `WHERE`. The builder emits exactly that: `SET` over
/// [`USER_COLUMNS`] minus `id` (`$1`–`$39`), `WHERE "users"."id" =
/// $40`. Handlers bind the fetched row with
/// [`EntityClearTarget::AvatarAssetId`] / `CoverImageAssetId` nulled;
/// [`EntityClearTarget::NoOp`] binds the row unchanged (and the
/// handler skips the statement entirely — the view returns without
/// touching the profile).
///
/// `User.save()` also lowercases/strips the email and rotates the
/// token when `token_updated_at` is set (`db/models/user.py:162-170`);
/// that value-shaping is handler-owned, like every other Python-side
/// assignment on this path.
pub fn entity_user_save_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut stmt = Query::update();
    stmt.table(Alias::new(USER_TABLE.to_owned()));
    let mut bind = 1u32;
    for col in USER_COLUMNS {
        if *col == "id" {
            continue;
        }
        stmt.value(
            Alias::new((*col).to_owned()),
            Expr::cust(format!("${bind}")),
        );
        bind += 1;
    }
    stmt.and_where(
        Expr::col((Alias::new(USER_TABLE), Alias::new("id"))).eq(Expr::cust(format!("${bind}"))),
    );
    stmt.to_string(PostgresQueryBuilder)
}

/// Generic get + patch lookup (`asset.py:430`, `:608` — one shared
/// builder): `FileAsset.objects.get(id=asset_id,
/// workspace__slug=slug, is_deleted=False)`. `DoesNotExist` → 404
/// `{"error": "Asset not found"}` and the `is_uploaded` 400 guard are
/// handler-owned (`fx-h-asset-generic`).
///
/// Binds: `$1` asset id, `$2` workspace slug.
pub fn generic_detail_select_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    join_workspace(&mut sel, file_asset::TABLE);
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(Expr::col((Alias::new(file_asset::TABLE), Alias::new("id"))).eq(Expr::cust("$1")))
            .add(Expr::cust(format!(
                "NOT \"{}\".\"is_deleted\"",
                file_asset::TABLE
            )))
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$2"))),
    );
    sel.to_string(PostgresQueryBuilder)
}

/// External-dedupe lookup (`asset.py:534-540`): only reached when
/// `external_id` **and** `external_source` are both truthy (Python
/// `and` — empty strings skip the check; handler-owned).
/// `filter(workspace__slug, external_source, external_id,
/// is_deleted=False).first()`; a hit → 409 with the existing row's id
/// and `asset_url`, a miss (`None`) → proceed to create.
///
/// Binds: `$1` external id, `$2` external source, `$3` workspace slug.
pub fn external_dedupe_select_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    select_table_columns(&mut sel, file_asset::TABLE, file_asset::COLUMNS);
    sel.from(Alias::new(file_asset::TABLE.to_owned()));
    join_workspace(&mut sel, file_asset::TABLE);
    sel.cond_where(
        Condition::all()
            .add(asset_alive())
            .add(
                Expr::col((Alias::new(file_asset::TABLE), Alias::new("external_id")))
                    .eq(Expr::cust("$1")),
            )
            .add(
                Expr::col((Alias::new(file_asset::TABLE), Alias::new("external_source")))
                    .eq(Expr::cust("$2")),
            )
            .add(Expr::cust(format!(
                "NOT \"{}\".\"is_deleted\"",
                file_asset::TABLE
            )))
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$3"))),
    );
    sel.order_by(
        (
            Alias::new(file_asset::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel.limit(1);
    sel.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset_cols_fragment() -> String {
        file_asset::COLUMNS
            .iter()
            .map(|c| format!("\"{}\".\"{}\"", file_asset::TABLE, c))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn soft_delete_select_matches_fixture_shape() {
        let sql = soft_delete_select_sql();
        // Projection + manager scope (fx-q-asset soft_delete_helper).
        assert!(sql.contains(&asset_cols_fragment()), "{sql}");
        assert!(
            sql.contains("FROM \"file_assets\" WHERE \"file_assets\".\"deleted_at\" IS NULL"),
            "{sql}"
        );
        assert!(sql.contains("\"file_assets\".\"id\" = ($1)"), "{sql}");
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
        let sql = soft_delete_update_sql();
        assert!(
            sql.contains("UPDATE \"file_assets\" SET \"is_deleted\" = $1, \"deleted_at\" = $2"),
            "{sql}"
        );
        assert!(sql.contains("WHERE \"file_assets\".\"id\" = ($3)"), "{sql}");
    }

    #[test]
    fn user_scoped_get_has_no_soft_delete_filter() {
        let sql = user_scoped_get_sql();
        assert!(sql.contains("\"file_assets\".\"id\" = ($1)"), "{sql}");
        assert!(sql.contains("\"file_assets\".\"user_id\" = ($2)"), "{sql}");
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
        let sql = entity_user_select_sql();
        // Live Django renders no deleted_at predicate here (module-docs
        // correction 4) and no ordering/limit on the get() path.
        assert!(
            sql.contains("FROM \"users\" WHERE \"users\".\"id\" = ($1)"),
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
    fn entity_user_save_is_full_row_with_pk_last() {
        let sql = entity_user_save_sql();
        assert!(sql.starts_with("UPDATE \"users\" SET "), "{sql}");
        // Every non-pk column bound in USER_COLUMNS order ($1-$39),
        // pk last ($40) — Django save() shape.
        assert!(sql.contains("\"password\" = $1"), "{sql}");
        assert!(sql.contains("\"masked_at\" = $39"), "{sql}");
        assert!(sql.contains("WHERE \"users\".\"id\" = ($40)"), "{sql}");
        assert!(!sql.contains("\"id\" = $"), "{sql}");
    }

    #[test]
    fn generic_detail_select_joins_workspace_with_not_deleted() {
        let sql = generic_detail_select_sql();
        assert!(
            sql.contains(
                "INNER JOIN \"workspaces\" ON \"file_assets\".\"workspace_id\" = \"workspaces\".\"id\""
            ),
            "{sql}"
        );
        assert!(sql.contains("\"file_assets\".\"id\" = ($1)"), "{sql}");
        // Live Django renders is_deleted=False as NOT (ported bug 3).
        assert!(sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = ($2)"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn external_dedupe_select_limits_to_newest_hit() {
        let sql = external_dedupe_select_sql();
        assert!(
            sql.contains("\"file_assets\".\"external_id\" = ($1)"),
            "{sql}"
        );
        assert!(
            sql.contains("\"file_assets\".\"external_source\" = ($2)"),
            "{sql}"
        );
        assert!(sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = ($3)"), "{sql}");
        assert!(
            sql.contains("ORDER BY \"file_assets\".\"created_at\" DESC LIMIT 1"),
            "{sql}"
        );
    }
}
