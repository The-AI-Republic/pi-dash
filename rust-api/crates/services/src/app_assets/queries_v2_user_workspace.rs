#![forbid(unsafe_code)]

//! v2 user + workspace asset queries (D-31, stage 5).
//!
//! Ports the query units of `UserAssetsV2Endpoint` (`app/views/asset/v2.py:29-198`)
//! and `WorkspaceFileAssetEndpoint` (`v2.py:201-429`) for the services layer:
//!
//! * [`get_entity_id_field`] — the 7-branch mapping (`v2.py:204-234`).
//! * [`asset_delete_lookup_sql`] / [`asset_delete_update_sql`] — the shared
//!   `filter(id).first()` + `save(update_fields=["is_deleted", "deleted_at"])`
//!   closure (`v2.py:32-39`, `:236-245`).
//! * [`user_asset_lookup_sql`] — the caller-scoped `.get(id, user_id)`
//!   (`v2.py:172,192`); [`workspace_asset_lookup_sql`] — the tenant-scoped
//!   `.get(id, workspace__slug)` join (`v2.py:381,401,411`).
//! * [`user_insert_sql`] / [`workspace_insert_sql`] — the presigned-post
//!   create sets (`v2.py:147-154`, `:355-363`).
//! * [`confirm_update_sql`] — `save(update_fields=["is_uploaded", "attributes"])`
//!   (`v2.py:188,397`).
//! * Entity link actions ([`user_save_action`], [`user_delete_action`],
//!   [`workspace_save_action`], [`workspace_delete_action`]) with the FK
//!   columns, cleared values and [`CacheInvalidation`] descriptors — the
//!   `entity_asset_save` / `entity_asset_delete` closures (`v2.py:41-107`,
//!   `:247-312`). Per the Porting guide transactions rule, Django signals
//!   become explicit calls: invalidations are data here, fired post-commit
//!   by the handler.
//! * Post validation ([`validate_user_entity_type`], [`validate_file_type`],
//!   [`parse_size`], [`clamp_size`]) with the exact 400 bodies
//!   (`v2.py:120-141`, `:322-343`).
//! * Asset-key shapes ([`user_asset_key`], [`workspace_asset_key`]) and the
//!   attributes set ([`create_attributes`]).
//! * The metadata publisher predicate ([`should_publish_metadata`]:
//!   `if not asset.storage_metadata`, `v2.py:176-177,:385-386`) and the
//!   is_uploaded-404 body ([`not_uploaded_body`], `v2.py:414-418`).
//!
//! Builders return SQL text with PostgreSQL `$N` binds; execution is
//! handlers-owned (`sqlx::query` at runtime — no `query!` macros, there is
//! no build-time database, same as the merged `queries_views` precedent).
//! The services crate holds no `sea-query` dependency, so statements are
//! assembled as strings over [`pidash_db::app_assets::columns`] consts; the
//! Django fixtures spell binds `%s` and the tests normalize both.
//! The cross-table `workspace__slug` filter renders with Django's `U0`
//! join alias (same alias-map convention as the merged `db` queries
//! precedent).
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * Every read goes through the default manager (`deleted_at IS NULL`,
//!   `db/mixins.py:56-58`); the delete-flip rows leave the manager scope.
//! * `.get()` lookups carry no `LIMIT`; `.filter().first()` lookups carry
//!   `LIMIT 1` and return `None` (no 404) when missing.
//! * `size_limit = min(size, FILE_SIZE_LIMIT)` — the user post spells it
//!   `min(size, ...)` (`:117`), the workspace post `min(..., size)` (`:346`);
//!   identical semantics, one [`clamp_size`].
//! * No membership check on any workspace route (tenant gaps pinned by the
//!   oracle `test_tenant_isolation.py`): the lookups scope by slug only.
//!   Ported as written.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG (`v2.py:290-291`): `entity_asset_delete`'s WORKSPACE_LOGO branch
//!   uses `Workspace.objects.get(...)` (raises when missing) and then
//!   checks `if workspace is None: return` — dead code, the row can never
//!   be `None`. [`WorkspaceLookup::Get`] ports the `.get` shape; the delete
//!   action keeps no `None` arm, exactly like the source.
//! * BUG (`v2.py:45` vs `:63`, `:257` vs `:279`): the avatar/logo clear
//!   writes `""` while the cover clear writes `None`
//!   ([`USER_AVATAR_CLEARED`], [`USER_COVER_CLEARED`],
//!   [`WORKSPACE_LOGO_CLEARED`], [`PROJECT_COVER_CLEARED`]). Ported as-is.
//! * BUG (`v2.py:319`): the workspace post defaults `entity_identifier`
//!   to `False`, so a `WORKSPACE_LOGO` post without identifier spreads
//!   `workspace_id=False` into the create (DB error path), unlike the
//!   project post default `None`. [`WorkspaceEntityId`] ports the `False`.
//! * BUG (`v2.py:113,317`): `size = int(...)` raises `ValueError` on
//!   non-numeric input (generic 500, no 400). [`parse_size`] surfaces
//!   [`SizeParseError`]; mapping it to the 500 body is handlers-owned.
//!
//! Fixture source of truth:
//! `rust-api/fixtures/app_assets/queries/v2_user_workspace.golden.json`
//! (recorded by PIDASHCONV-306). The `#[cfg(test)]` suite matches every
//! builder and helper against that fixture.

use pidash_db::app_assets::columns;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `file_assets` (`db/models/asset.py:67`,
/// `pidash_db::app_assets::columns::TABLE`).
pub const FILE_ASSET_TABLE: &str = columns::TABLE;
/// `users` (`db/models/user.py:136`).
pub const USER_TABLE: &str = "users";
/// `workspaces` (`db/models/workspace.py:181`).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// `projects` (`db/models/project.py:252`).
pub const PROJECT_TABLE: &str = "projects";

/// Django's join alias for the single `workspace__slug` join, kept verbatim
/// (same alias-map convention as the merged `db` queries precedent).
const U0: &str = "U0";

// ---------------------------------------------------------------------------
// Entity-type vocabulary
// ---------------------------------------------------------------------------

/// `USER_AVATAR` (`db/models/asset.py:38`).
pub const ENTITY_USER_AVATAR: &str = "USER_AVATAR";
/// `USER_COVER` (`db/models/asset.py:37`).
pub const ENTITY_USER_COVER: &str = "USER_COVER";
/// `WORKSPACE_LOGO` (`db/models/asset.py:39`).
pub const ENTITY_WORKSPACE_LOGO: &str = "WORKSPACE_LOGO";
/// `PROJECT_COVER` (`db/models/asset.py:40`).
pub const ENTITY_PROJECT_COVER: &str = "PROJECT_COVER";
/// `ISSUE_ATTACHMENT` (`db/models/asset.py:34`).
pub const ENTITY_ISSUE_ATTACHMENT: &str = "ISSUE_ATTACHMENT";
/// `ISSUE_DESCRIPTION` (`db/models/asset.py:35`).
pub const ENTITY_ISSUE_DESCRIPTION: &str = "ISSUE_DESCRIPTION";
/// `PAGE_DESCRIPTION` (`db/models/asset.py:36`).
pub const ENTITY_PAGE_DESCRIPTION: &str = "PAGE_DESCRIPTION";
/// `COMMENT_DESCRIPTION` (`db/models/asset.py:33`).
pub const ENTITY_COMMENT_DESCRIPTION: &str = "COMMENT_DESCRIPTION";

/// Entity types the user post accepts (`v2.py:120`): a hardcoded 2-value
/// list, NOT `EntityTypeContext.values`.
pub const USER_POST_ENTITY_TYPES: &[&str] = &[ENTITY_USER_AVATAR, ENTITY_USER_COVER];

/// MIME types either post accepts (`v2.py:127-133`, `:329-335`).
pub const ALLOWED_FILE_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// `FileAsset.EntityTypeContext.values` membership for the workspace post
/// (`v2.py:322`): the full 10-value set
/// (`pidash_db::app_assets::columns::ENTITY_TYPES`).
pub fn is_known_entity_type(entity_type: &str) -> bool {
    columns::ENTITY_TYPES.contains(&entity_type)
}

/// `get_entity_id_field` (`v2.py:204-234`): the 7-branch mapping from
/// entity type to the FK column the create spreads. `None` is the
/// fallthrough `{}` (`:234`) — ported per-branch verbatim against the
/// PROJECT variant (which adds `draft_issue_id`) and the DUPLICATE variant
/// (which drops DRAFT again); never unify the three.
pub fn get_entity_id_field(entity_type: &str) -> Option<&'static str> {
    if entity_type == ENTITY_WORKSPACE_LOGO {
        // v2.py:206-207.
        Some("workspace_id")
    } else if entity_type == ENTITY_PROJECT_COVER {
        // v2.py:210-211.
        Some("project_id")
    } else if entity_type == ENTITY_USER_AVATAR || entity_type == ENTITY_USER_COVER {
        // v2.py:214-218.
        Some("user_id")
    } else if entity_type == ENTITY_ISSUE_ATTACHMENT || entity_type == ENTITY_ISSUE_DESCRIPTION {
        // v2.py:221-225.
        Some("issue_id")
    } else if entity_type == ENTITY_PAGE_DESCRIPTION {
        // v2.py:228-229.
        Some("page_id")
    } else if entity_type == ENTITY_COMMENT_DESCRIPTION {
        // v2.py:232-233.
        Some("comment_id")
    } else {
        // v2.py:234 (`return {}`).
        None
    }
}

// ---------------------------------------------------------------------------
// Post validation
// ---------------------------------------------------------------------------

/// Which post validation failed; the handler maps each to its exact 400
/// body below (and [`SizeParseError`] to the generic 500).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostValidationError {
    EntityType,
    FileType,
}

/// `{"error": "Invalid entity type.", "status": false}` + 400
/// (`v2.py:121-124`, `:323-326`).
pub fn invalid_entity_type_body() -> Value {
    json!({"error": "Invalid entity type.", "status": false})
}

/// `{"error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files
/// are allowed.", "status": false}` + 400 (`v2.py:135-140`, `:337-342`).
pub fn invalid_file_type_body() -> Value {
    json!({
        "error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
        "status": false,
    })
}

/// Maps a [`PostValidationError`] to its exact response body.
pub fn post_error_body(err: PostValidationError) -> Value {
    match err {
        PostValidationError::EntityType => invalid_entity_type_body(),
        PostValidationError::FileType => invalid_file_type_body(),
    }
}

/// User-post entity check (`v2.py:120`):
/// `if not entity_type or entity_type not in ["USER_AVATAR", "USER_COVER"]`.
/// `None` models the `request.data.get("entity_type", False)` default.
pub fn validate_user_entity_type(entity_type: Option<&str>) -> Result<&str, PostValidationError> {
    match entity_type {
        Some(t) if USER_POST_ENTITY_TYPES.contains(&t) => Ok(t),
        _ => Err(PostValidationError::EntityType),
    }
}

/// Workspace-post entity check (`v2.py:322`):
/// `if entity_type not in FileAsset.EntityTypeContext.values`.
/// `None` (missing key — unlike the user post there is no `False` default
/// here, only the identifier has one) is invalid.
pub fn validate_workspace_entity_type(
    entity_type: Option<&str>,
) -> Result<&str, PostValidationError> {
    match entity_type {
        Some(t) if is_known_entity_type(t) => Ok(t),
        _ => Err(PostValidationError::EntityType),
    }
}

/// Shared file-type check (`v2.py:134`, `:336`).
pub fn validate_file_type(file_type: &str) -> Result<&str, PostValidationError> {
    if ALLOWED_FILE_TYPES.contains(&file_type) {
        Ok(file_type)
    } else {
        Err(PostValidationError::FileType)
    }
}

/// `int(...)` failed (`v2.py:113,317`): a non-numeric `size` raises
/// `ValueError`, which `handle_exception` maps to the generic 500
/// (`app/views/base.py`). Handlers own that mapping; the query layer only
/// surfaces the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeParseError;

/// `size = int(request.data.get("size", FILE_SIZE_LIMIT))` (`v2.py:113,317`):
/// CPython `int(s, 10)` semantics — surrounding whitespace, a single
/// leading `+`/`-`, and inter-digit underscores are accepted; anything else
/// raises. `None` is the omitted-key default (`FILE_SIZE_LIMIT`).
pub fn parse_size(raw: Option<&str>, default: i64) -> Result<i64, SizeParseError> {
    let text = match raw {
        None => return Ok(default),
        Some(t) => t.trim(),
    };
    if text.is_empty() {
        return Err(SizeParseError);
    }
    let (digits, negative) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (rest, text.starts_with('-')),
        None => (text, false),
    };
    if digits.is_empty() {
        return Err(SizeParseError);
    }
    // CPython allows single underscores strictly *between* digits.
    let mut clean = String::with_capacity(digits.len());
    let mut prev_underscore = false;
    for (i, b) in digits.bytes().enumerate() {
        if b == b'_' {
            if i == 0 || prev_underscore {
                return Err(SizeParseError);
            }
            prev_underscore = true;
            continue;
        }
        if !b.is_ascii_digit() {
            return Err(SizeParseError);
        }
        prev_underscore = false;
        clean.push(b as char);
    }
    if prev_underscore {
        return Err(SizeParseError);
    }
    // Python ints are unbounded while the column is a float: saturate
    // absurd magnitudes to the i64 extremes (Porting guide semantic trap)
    // so `min()` still clamps them to the limit instead of 500ing.
    let magnitude: i128 = clean.parse().map_err(|_| SizeParseError)?;
    let value: i128 = if negative { -magnitude } else { magnitude };
    Ok(i64::try_from(value).unwrap_or(if negative { i64::MIN } else { i64::MAX }))
}

/// `size_limit = min(size, settings.FILE_SIZE_LIMIT)` (`v2.py:117,346`).
pub fn clamp_size(size: i64, file_size_limit: i64) -> i64 {
    size.min(file_size_limit)
}

/// Default `FILE_SIZE_LIMIT`
/// (`pidash_db::app_assets::columns::FILE_SIZE_LIMIT_DEFAULT`,
/// `settings/common.py:428`).
pub const FILE_SIZE_LIMIT_DEFAULT: i64 = columns::FILE_SIZE_LIMIT_DEFAULT as i64;

/// Omitted `type` defaults to `"image/jpeg"` on both posts
/// (`v2.py:112,316`).
pub const DEFAULT_FILE_TYPE: &str = "image/jpeg";

/// Omitted user-post `entity_type` defaults to `False`
/// (`request.data.get("entity_type", False)`, `v2.py:114`), so a missing
/// key fails the `not entity_type` check exactly like an invalid value.
/// Model [`validate_user_entity_type`] with `None`.
pub const USER_POST_ENTITY_DEFAULT_FALSE: bool = false;

// ---------------------------------------------------------------------------
// Asset keys + create sets
// ---------------------------------------------------------------------------

/// `asset_key = f"{uuid.uuid4().hex}-{name}"` (`v2.py:144`): bare
/// `<hex>-<name>`, no workspace prefix. `uuid_hex` is the caller-supplied
/// `uuid4().hex` (uuid generation is a side effect and lives at the call
/// site); `name` may be `None` when the key omits it (Django renders
/// `None`).
pub fn user_asset_key(uuid_hex: &str, name: Option<&str>) -> String {
    format!("{uuid_hex}-{}", name.unwrap_or("None"))
}

/// `asset_key = f"{workspace.id}/{uuid.uuid4().hex}-{name}"` (`v2.py:352`):
/// `<workspace-id>/<hex>-<name>` — the id, never the slug.
pub fn workspace_asset_key(workspace_id: &str, uuid_hex: &str, name: Option<&str>) -> String {
    format!("{workspace_id}/{uuid_hex}-{}", name.unwrap_or("None"))
}

/// `attributes={"name": name, "type": type, "size": size_limit}`
/// (`v2.py:148`, `:356`): exactly these three keys.
pub fn create_attributes(name: Option<&str>, file_type: &str, size_limit: i64) -> Value {
    json!({"name": name, "type": file_type, "size": size_limit})
}

/// `patch` confirm: `save(update_fields=["is_uploaded", "attributes"])`
/// (`v2.py:188,397`) — `is_deleted`/`deleted_at` untouched.
pub const CONFIRM_UPDATE_FIELDS: &[&str] = &["is_uploaded", "attributes"];

/// `delete` / `asset_delete` flips: `save(update_fields=["is_deleted",
/// "deleted_at"])` (`v2.py:38,197,244,406`).
pub const DELETE_UPDATE_FIELDS: &[&str] = &["is_deleted", "deleted_at"];

// ---------------------------------------------------------------------------
// Read SQL
// ---------------------------------------------------------------------------

/// `asset_delete`: `FileAsset.objects.filter(id=asset_id).first()`
/// (`v2.py:33,237`) — silent `None` return when missing, no 404.
/// `$1` is the asset id.
pub fn asset_delete_lookup_sql() -> String {
    format!(
        "SELECT \"{t}\".* FROM \"{t}\" WHERE \"{t}\".\"id\" = $1 \
         AND \"{t}\".\"deleted_at\" IS NULL LIMIT 1",
        t = FILE_ASSET_TABLE,
    )
}

/// User patch/delete: `FileAsset.objects.get(id=asset_id,
/// user_id=request.user.id)` (`v2.py:172,192`) — scoped to the caller;
/// another user's row raises (handlers map it to
/// `{"error": "The required object does not exist."}` 404, owned by
/// `app/views/base.py`, not re-ported here). `$1` is the asset id, `$2`
/// the caller id.
pub fn user_asset_lookup_sql() -> String {
    format!(
        "SELECT \"{t}\".* FROM \"{t}\" WHERE \"{t}\".\"id\" = $1 \
         AND \"{t}\".\"user_id\" = $2 AND \"{t}\".\"deleted_at\" IS NULL",
        t = FILE_ASSET_TABLE,
    )
}

/// Workspace patch/delete/get: `FileAsset.objects.get(id=asset_id,
/// workspace__slug=slug)` (`v2.py:381,401,411`) — slug-scoped only, no
/// membership predicate (tenant gap, ported as written). `$1` is the asset
/// id, `$2` the workspace slug.
pub fn workspace_asset_lookup_sql() -> String {
    format!(
        "SELECT \"{t}\".* FROM \"{t}\" INNER JOIN \"{w}\" {u} \
         ON (\"{t}\".\"workspace_id\" = {u}.\"id\") \
         WHERE \"{t}\".\"id\" = $1 AND {u}.\"slug\" = $2 \
         AND \"{t}\".\"deleted_at\" IS NULL",
        t = FILE_ASSET_TABLE,
        w = WORKSPACE_TABLE,
        u = U0,
    )
}

// ---------------------------------------------------------------------------
// Write SQL
// ---------------------------------------------------------------------------

/// User post create (`v2.py:147-154`): `attributes`, `asset`, `size`,
/// `user` + `created_by` (both the caller), `entity_type`. `workspace_id`
/// is NULL; the avatar/cover link is NOT set here — only via
/// patch/`entity_asset_save`. `$1..$6` follow the listed column order.
/// `RETURNING "id"` feeds the `str(asset.id)` response.
pub fn user_insert_sql() -> String {
    format!(
        "INSERT INTO \"{t}\" (\"attributes\", \"asset\", \"size\", \"user_id\", \
         \"created_by_id\", \"entity_type\") \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING \"{t}\".\"id\"",
        t = FILE_ASSET_TABLE,
    )
}

/// Create columns for [`workspace_insert_sql`]: the fixed set plus an
/// optional per-[`get_entity_id_field`] FK column (or nothing on the `{}`
/// fallthrough).
pub fn workspace_insert_columns(entity_fk_col: Option<&'static str>) -> Vec<&'static str> {
    let mut cols = vec![
        "attributes",
        "asset",
        "size",
        "workspace_id",
        "created_by_id",
        "entity_type",
    ];
    if let Some(fk) = entity_fk_col {
        cols.push(fk);
    }
    cols
}

/// Workspace post create (`v2.py:355-363`): the fixed set
/// (`attributes`, `asset`, `size`, `workspace` resolved from the slug,
/// `created_by`, `entity_type`) spread with
/// `**get_entity_id_field(entity_type, entity_identifier)`. Binds `$1..$N`
/// follow [`workspace_insert_columns`] order; `RETURNING "id"` feeds
/// `str(asset.id)`.
pub fn workspace_insert_sql(entity_fk_col: Option<&'static str>) -> String {
    let cols = workspace_insert_columns(entity_fk_col);
    let list = cols
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let binds = (1..=cols.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{t}\" ({list}) VALUES ({binds}) RETURNING \"{t}\".\"id\"",
        t = FILE_ASSET_TABLE,
    )
}

/// Patch confirm (`v2.py:188,397`): `save(update_fields=[...])` over
/// [`CONFIRM_UPDATE_FIELDS`]. `$1` is_uploaded, `$2` attributes, `$3` id.
pub fn confirm_update_sql() -> String {
    format!(
        "UPDATE \"{t}\" SET \"is_uploaded\" = $1, \"attributes\" = $2 WHERE \"{t}\".\"id\" = $3",
        t = FILE_ASSET_TABLE,
    )
}

/// `asset_delete` / `delete` (`v2.py:36-38,:193-197,:242-244,:402-406`):
/// `is_deleted=True, deleted_at=now` saved over [`DELETE_UPDATE_FIELDS`].
/// `$1` is_deleted, `$2` deleted_at, `$3` id.
pub fn delete_update_sql() -> String {
    format!(
        "UPDATE \"{t}\" SET \"is_deleted\" = $1, \"deleted_at\" = $2 WHERE \"{t}\".\"id\" = $3",
        t = FILE_ASSET_TABLE,
    )
}

/// Entity-link flip (`user.save()` / `workspace.save()` / `project.save()`
/// after setting one FK): `UPDATE "<table>" SET "<fk>" = $1 WHERE "id" =
/// $2`. Full-row saves render every column; the link column is the one the
/// golden pins, so the builder carries it explicitly and the handler owns
/// the remaining columns.
pub fn entity_link_update_sql(table: &str, fk_col: &str) -> String {
    format!("UPDATE \"{table}\" SET \"{fk_col}\" = $1 WHERE \"id\" = $2")
}

// ---------------------------------------------------------------------------
// Entity link actions
// ---------------------------------------------------------------------------

/// `User.avatar_asset_id` (`db/models/user.py:69-75`).
pub const USER_AVATAR_FK: &str = "avatar_asset_id";
/// `User.cover_image_asset_id` (`db/models/user.py:78-84`).
pub const USER_COVER_FK: &str = "cover_image_asset_id";
/// `Workspace.logo_asset_id` (`db/models/workspace.py:124-130`).
pub const WORKSPACE_LOGO_FK: &str = "logo_asset_id";
/// `Project.cover_image_asset_id` (`db/models/project.py:108-114`).
pub const PROJECT_COVER_FK: &str = "cover_image_asset_id";

/// `user.avatar = ""` (`v2.py:45`) — empty string, while the cover below
/// uses `None`. Ported as-is.
pub const USER_AVATAR_CLEARED: &str = "";
/// `user.cover_image = None` (`v2.py:63`) — asymmetry with `avatar`
/// ported as-is.
pub const USER_COVER_CLEARED: Option<&str> = None;
/// `workspace.logo = ""` (`v2.py:257`). Ported as-is.
pub const WORKSPACE_LOGO_CLEARED: &str = "";
/// `project.cover_image = ""` (`v2.py:279`). Ported as-is.
pub const PROJECT_COVER_CLEARED: &str = "";

/// What `UserAssetsV2Endpoint.entity_asset_save` does (`v2.py:41-78`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSaveAction {
    /// `USER_AVATAR`: clear `avatar`, reap the previous link, set
    /// `avatar_asset_id`, full save, invalidate (`:43-59`).
    SetAvatar,
    /// `USER_COVER`: clear `cover_image`, reap the previous link, set
    /// `cover_image_asset_id`, full save, invalidate (`:61-77`).
    SetCover,
    /// Any other type: return, nothing happens (`:78`). Unreachable from
    /// `post` (which rejects non-USER_* types); patch-time defence.
    Noop,
}

/// Maps the entity type to its [`UserSaveAction`].
pub fn user_save_action(entity_type: &str) -> UserSaveAction {
    if entity_type == ENTITY_USER_AVATAR {
        UserSaveAction::SetAvatar
    } else if entity_type == ENTITY_USER_COVER {
        UserSaveAction::SetCover
    } else {
        UserSaveAction::Noop
    }
}

/// What `UserAssetsV2Endpoint.entity_asset_delete` does (`v2.py:80-107`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDeleteAction {
    /// `USER_AVATAR`: `avatar_asset_id=None`, save, invalidate (`:82-93`).
    ClearAvatar,
    /// `USER_COVER`: `cover_image_asset_id=None`, save, invalidate
    /// (`:95-106`).
    ClearCover,
    /// Any other type: return (`:107`).
    Noop,
}

/// Maps the entity type to its [`UserDeleteAction`].
pub fn user_delete_action(entity_type: &str) -> UserDeleteAction {
    if entity_type == ENTITY_USER_AVATAR {
        UserDeleteAction::ClearAvatar
    } else if entity_type == ENTITY_USER_COVER {
        UserDeleteAction::ClearCover
    } else {
        UserDeleteAction::Noop
    }
}

/// How the entity row is read: Django `.get()` raises on a miss while
/// `.filter().first()` returns `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceLookup {
    /// `.filter(id=...).first()` — miss returns `None`, caller returns
    /// (`v2.py:250,272`).
    FilterFirst,
    /// `.get(id=...)` — miss raises (handlers map it to 404). The
    /// `if workspace is None: return` after it (`v2.py:290-291`) is dead
    /// code (see the module-level BUG note).
    Get,
}

/// What `WorkspaceFileAssetEndpoint.entity_asset_save` does
/// (`v2.py:247-284`): only `WORKSPACE_LOGO` and `PROJECT_COVER` link;
/// `ISSUE`/`PAGE`/`COMMENT`/`DRAFT` rows link NOTHING here (`:283-284`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSaveAction {
    /// `WORKSPACE_LOGO`: [`WorkspaceLookup::FilterFirst`], reap the
    /// previous logo, set `logo=""` + `logo_asset_id`, save, invalidate
    /// (`:249-268`).
    SetLogo,
    /// `PROJECT_COVER`: [`WorkspaceLookup::FilterFirst`], reap the
    /// previous cover, set `cover_image=""` + `cover_image_asset_id`,
    /// save (`:271-282`, no invalidations).
    SetCover,
    /// Any other type: return, nothing (`:283-284`).
    Noop,
}

/// Maps the entity type to its [`WorkspaceSaveAction`].
pub fn workspace_save_action(entity_type: &str) -> WorkspaceSaveAction {
    if entity_type == ENTITY_WORKSPACE_LOGO {
        WorkspaceSaveAction::SetLogo
    } else if entity_type == ENTITY_PROJECT_COVER {
        WorkspaceSaveAction::SetCover
    } else {
        WorkspaceSaveAction::Noop
    }
}

/// What `WorkspaceFileAssetEndpoint.entity_asset_delete` does
/// (`v2.py:286-312`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDeleteAction {
    /// `WORKSPACE_LOGO`: [`WorkspaceLookup::Get`], `logo_asset_id=None`,
    /// save, invalidate (`:288-302`).
    ClearLogo,
    /// `PROJECT_COVER`: [`WorkspaceLookup::FilterFirst`], miss returns,
    /// else `cover_image_asset_id=None`, save (`:304-310`).
    ClearCover,
    /// Any other type: return (`:311-312`).
    Noop,
}

/// Maps the entity type to its [`WorkspaceDeleteAction`].
pub fn workspace_delete_action(entity_type: &str) -> WorkspaceDeleteAction {
    if entity_type == ENTITY_WORKSPACE_LOGO {
        WorkspaceDeleteAction::ClearLogo
    } else if entity_type == ENTITY_PROJECT_COVER {
        WorkspaceDeleteAction::ClearCover
    } else {
        WorkspaceDeleteAction::Noop
    }
}

/// The lookup each workspace entity action reads through.
pub fn workspace_save_lookup(action: WorkspaceSaveAction) -> Option<WorkspaceLookup> {
    match action {
        WorkspaceSaveAction::SetLogo | WorkspaceSaveAction::SetCover => {
            Some(WorkspaceLookup::FilterFirst)
        }
        WorkspaceSaveAction::Noop => None,
    }
}

/// The lookup each workspace delete action reads through: the logo branch
/// uses `.get` (with the dead `None` check), the cover branch
/// `.filter().first()`.
pub fn workspace_delete_lookup(action: WorkspaceDeleteAction) -> Option<WorkspaceLookup> {
    match action {
        WorkspaceDeleteAction::ClearLogo => Some(WorkspaceLookup::Get),
        WorkspaceDeleteAction::ClearCover => Some(WorkspaceLookup::FilterFirst),
        WorkspaceDeleteAction::Noop => None,
    }
}

// ---------------------------------------------------------------------------
// Post-commit invalidations + metadata publisher
// ---------------------------------------------------------------------------

/// One `invalidate_cache_directly(path, url_params=False, user, request)`
/// call (`utils/cache.py`): the path plus the `user` flag. Fired
/// post-commit by the handler (Porting guide transactions rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheInvalidation {
    /// The invalidated path prefix.
    pub path: &'static str,
    /// The `user=` argument.
    pub user: bool,
}

/// User save + delete invalidations (`v2.py:52-58,:70-76,:86-92,:99-104`):
/// `/api/users/me/` and `/api/users/me/settings/`, both `user=True`.
pub const USER_POST_COMMIT: &[CacheInvalidation] = &[
    CacheInvalidation {
        path: "/api/users/me/",
        user: true,
    },
    CacheInvalidation {
        path: "/api/users/me/settings/",
        user: true,
    },
];

/// Workspace-logo save + delete invalidations
/// (`v2.py:260-267,:294-301`): `/api/workspaces/` (`user=False`),
/// `/api/users/me/workspaces/` (`user=True`), `/api/instances/`
/// (`user=False`). The project-cover branches invalidate nothing.
pub const WORKSPACE_LOGO_POST_COMMIT: &[CacheInvalidation] = &[
    CacheInvalidation {
        path: "/api/workspaces/",
        user: false,
    },
    CacheInvalidation {
        path: "/api/users/me/workspaces/",
        user: true,
    },
    CacheInvalidation {
        path: "/api/instances/",
        user: false,
    },
];

/// The metadata task kwarg: `get_asset_object_metadata.delay(asset_id=str(asset_id))`
/// (`v2.py:177,386`) — the asset uuid as a STRING under `asset_id`.
/// The task body is owned by PIDASHCONV-387
/// (`tasks/file_asset.golden.json`); this is publishers-only.
pub const METADATA_TASK_KWARG: &str = "asset_id";

/// `if not asset.storage_metadata:` (`v2.py:176,385`) — falsy fires:
/// `None` (null column), JSON `null`, `""`, `{}`, `[]`, `false` and
/// numeric zero all publish; anything else skips. `None` is the NULL
/// column. (Porting guide semantic trap: falsy `storage_metadata`, `None`
/// and `{}` alike.)
pub fn should_publish_metadata(storage_metadata: Option<&Value>) -> bool {
    match storage_metadata {
        None => true,
        Some(v) => {
            v.is_null()
                || v.as_str().is_some_and(|s| s.is_empty())
                || v.as_object().is_some_and(|m| m.is_empty())
                || v.as_array().is_some_and(|a| a.is_empty())
                || v.as_bool().is_some_and(|b| !b)
                || v.as_f64().is_some_and(|n| n == 0.0)
        }
    }
}

/// `request.data.get("entity_identifier", False)` (`v2.py:319`): the
/// workspace post defaults a MISSING identifier to boolean `False` —
/// unlike the project post default `None` (`v2.py:518`). A
/// `WORKSPACE_LOGO` post without identifier therefore spreads
/// `workspace_id=False` into the create (DB error path). Ported as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntityId<'a> {
    /// Identifier present: spread as the FK value.
    Value(&'a str),
    /// Identifier missing: boolean `False` is spread instead.
    MissingBecomesFalse,
}

/// Maps the raw identifier to its [`WorkspaceEntityId`] spread value.
pub fn workspace_entity_id(entity_identifier: Option<&str>) -> WorkspaceEntityId<'_> {
    match entity_identifier {
        Some(v) => WorkspaceEntityId::Value(v),
        None => WorkspaceEntityId::MissingBecomesFalse,
    }
}

// ---------------------------------------------------------------------------
// Get response
// ---------------------------------------------------------------------------

/// `{"error": "The requested asset could not be found."}` + 404
/// (`v2.py:415-418`): NO `status` key — differs from the post validation
/// bodies and the v1 miss bodies. Ported exactly.
pub fn not_uploaded_body() -> Value {
    json!({"error": "The requested asset could not be found."})
}

/// HTTP status for [`not_uploaded_body`].
pub const NOT_UPLOADED_STATUS: u16 = 404;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn golden() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_assets/queries/v2_user_workspace.golden.json");
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read v2_user_workspace.golden.json: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    fn bind(sql: &str) -> String {
        // The Django fixtures spell binds `%s`; builders emit `$N`.
        sql.replace("$1", "%s")
            .replace("$2", "%s")
            .replace("$3", "%s")
            .replace("$4", "%s")
            .replace("$5", "%s")
            .replace("$6", "%s")
            .replace("$7", "%s")
    }

    #[test]
    fn entity_literals_are_known_types() {
        for t in [
            ENTITY_USER_AVATAR,
            ENTITY_USER_COVER,
            ENTITY_WORKSPACE_LOGO,
            ENTITY_PROJECT_COVER,
            ENTITY_ISSUE_ATTACHMENT,
            ENTITY_ISSUE_DESCRIPTION,
            ENTITY_PAGE_DESCRIPTION,
            ENTITY_COMMENT_DESCRIPTION,
        ] {
            assert!(is_known_entity_type(t), "{t} must be a known entity type");
        }
    }

    #[test]
    fn entity_id_field_matches_golden_branches() {
        let g = golden();
        let branches = g["WorkspaceFileAssetEndpoint"]["get_entity_id_field"]["branches"]
            .as_array()
            .expect("branches array");
        assert_eq!(branches.len(), 7, "golden pins 7 branches");
        // Every golden branch maps to the same FK column here.
        let cases: &[(&str, Option<&str>)] = &[
            ("WORKSPACE_LOGO", Some("workspace_id")),
            ("PROJECT_COVER", Some("project_id")),
            ("USER_AVATAR", Some("user_id")),
            ("USER_COVER", Some("user_id")),
            ("ISSUE_ATTACHMENT", Some("issue_id")),
            ("ISSUE_DESCRIPTION", Some("issue_id")),
            ("PAGE_DESCRIPTION", Some("page_id")),
            ("COMMENT_DESCRIPTION", Some("comment_id")),
        ];
        for (entity, want) in cases {
            assert_eq!(get_entity_id_field(entity), *want, "{entity}");
        }
        // Fallthrough `{}` (v2.py:234), incl. the DRAFT types this variant
        // deliberately omits (divergence note in the golden).
        for entity in [
            "NOPE",
            "DRAFT_ISSUE_ATTACHMENT",
            "DRAFT_ISSUE_DESCRIPTION",
            "",
        ] {
            assert_eq!(get_entity_id_field(entity), None, "{entity}");
        }
        // The golden branch count covers the same entity set.
        let golden_text = serde_json::to_string(branches).expect("branches serialize");
        for entity in [
            "WORKSPACE_LOGO",
            "ISSUE_ATTACHMENT",
            "PAGE_DESCRIPTION",
            "COMMENT_DESCRIPTION",
        ] {
            assert!(golden_text.contains(entity), "golden names {entity}");
        }
    }

    #[test]
    fn error_bodies_match_golden() {
        let g = golden();
        assert_eq!(
            post_error_body(PostValidationError::EntityType),
            g["UserAssetsV2Endpoint"]["post"]["entity_type_check"]["body"],
        );
        assert_eq!(
            post_error_body(PostValidationError::FileType),
            g["UserAssetsV2Endpoint"]["post"]["file_type_check"]["body"],
        );
        assert_eq!(
            post_error_body(PostValidationError::EntityType),
            g["WorkspaceFileAssetEndpoint"]["post"]["entity_type_check"]["body"],
        );
        assert_eq!(
            post_error_body(PostValidationError::FileType),
            g["WorkspaceFileAssetEndpoint"]["post"]["file_type_check"]["body"],
        );
        // Workspace file-type body carries the same literal (golden holds
        // only status + trace there, so pin the literal directly).
        assert_eq!(
            invalid_file_type_body(),
            json!({
                "error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
                "status": false,
            }),
        );
    }

    #[test]
    fn allowed_file_types_match_golden() {
        let g = golden();
        let golden_types = g["UserAssetsV2Endpoint"]["post"]["allowed_file_types"]
            .as_array()
            .expect("allowed_file_types array");
        let got: Vec<&str> = ALLOWED_FILE_TYPES.to_vec();
        let want: Vec<&str> = golden_types
            .iter()
            .map(|v| v.as_str().expect("string"))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn user_entity_validation() {
        assert_eq!(
            validate_user_entity_type(Some("USER_AVATAR")),
            Ok("USER_AVATAR")
        );
        assert_eq!(
            validate_user_entity_type(Some("USER_COVER")),
            Ok("USER_COVER")
        );
        // `not entity_type`: missing, empty, and non-USER_* all 400.
        for bad in [None, Some(""), Some("WORKSPACE_LOGO"), Some("NOPE")] {
            assert_eq!(
                validate_user_entity_type(bad),
                Err(PostValidationError::EntityType),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn workspace_entity_validation_uses_full_value_set() {
        // Full 10-value set accepted here (unlike the user 2-list).
        for t in columns::ENTITY_TYPES {
            assert_eq!(validate_workspace_entity_type(Some(t)), Ok(*t), "{t}");
        }
        for bad in [None, Some(""), Some("NOPE")] {
            assert_eq!(
                validate_workspace_entity_type(bad),
                Err(PostValidationError::EntityType),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn file_type_validation() {
        for t in ALLOWED_FILE_TYPES {
            assert_eq!(validate_file_type(t), Ok(*t));
        }
        assert_eq!(
            validate_file_type("application/pdf"),
            Err(PostValidationError::FileType)
        );
        assert_eq!(validate_file_type(""), Err(PostValidationError::FileType));
    }

    #[test]
    fn size_parse_and_clamp() {
        let limit = 5_242_880_i64;
        assert_eq!(parse_size(None, limit), Ok(limit));
        assert_eq!(parse_size(Some("512"), limit), Ok(512));
        assert_eq!(parse_size(Some(" 1024 "), limit), Ok(1024));
        assert_eq!(parse_size(Some("+7"), limit), Ok(7));
        assert_eq!(parse_size(Some("-5"), limit), Ok(-5));
        assert_eq!(parse_size(Some("1_0"), limit), Ok(10));
        // `int()` 500 edges.
        for bad in ["", "  ", "abc", "5.5", "1__0", "_1", "1_", "+-", "12a"] {
            assert_eq!(parse_size(Some(bad), limit), Err(SizeParseError), "{bad}");
        }
        // Unbounded Python ints saturate instead of erroring, so `min()`
        // still clamps them to the limit.
        let huge = parse_size(Some("99999999999999999999999"), limit).expect("saturates");
        assert_eq!(clamp_size(huge, limit), limit);
        // `min()` semantics either operand order.
        assert_eq!(clamp_size(512, limit), 512);
        assert_eq!(clamp_size(10_i64.pow(9), limit), limit);
        assert_eq!(clamp_size(limit, limit), limit);
        assert_eq!(FILE_SIZE_LIMIT_DEFAULT, limit);
        // Post input defaults (v2.py:112-114,:316-317).
        assert_eq!(DEFAULT_FILE_TYPE, "image/jpeg");
        assert!(!USER_POST_ENTITY_DEFAULT_FALSE);
        assert_eq!(
            validate_user_entity_type(None),
            Err(PostValidationError::EntityType),
            "missing entity_type 400s like an invalid one"
        );
    }

    #[test]
    fn asset_key_shapes_match_golden() {
        let g = golden();
        let hex = "d".repeat(32);
        let user_key = user_asset_key(&hex, Some("avatar.png"));
        assert_eq!(user_key, format!("{hex}-avatar.png"));
        assert!(!user_key.contains('/'), "bare key, no workspace prefix");
        assert!(user_key.ends_with("-avatar.png"));
        assert!(g["UserAssetsV2Endpoint"]["post"]["asset_key_shape"]
            .as_str()
            .unwrap()
            .contains("hex"));
        let ws_key = workspace_asset_key("ws-id-1", &hex, Some("logo.png"));
        assert_eq!(ws_key, format!("ws-id-1/{hex}-logo.png"));
        assert!(ws_key.starts_with("ws-id-1/"));
        assert!(ws_key.ends_with("-logo.png"));
        assert!(!ws_key.contains("slug"), "id, never the slug");
    }

    #[test]
    fn create_attributes_set() {
        assert_eq!(
            create_attributes(Some("logo.png"), "image/png", 1024),
            json!({"name": "logo.png", "type": "image/png", "size": 1024}),
        );
    }

    #[test]
    fn update_fields_match_golden() {
        assert_eq!(
            CONFIRM_UPDATE_FIELDS.to_vec(),
            vec!["is_uploaded", "attributes"]
        );
        assert_eq!(
            DELETE_UPDATE_FIELDS.to_vec(),
            vec!["is_deleted", "deleted_at"]
        );
        let g = golden();
        assert!(g["UserAssetsV2Endpoint"]["patch"]["save"]
            .as_str()
            .unwrap()
            .contains("is_uploaded"));
        assert!(g["WorkspaceFileAssetEndpoint"]["patch"]["save"]
            .as_str()
            .unwrap()
            .contains("attributes"));
    }

    #[test]
    fn read_sql_shapes() {
        // asset_delete: filter().first() — guarded, LIMIT 1, silent None.
        let sql = bind(&asset_delete_lookup_sql());
        assert!(sql.contains("\"file_assets\".\"id\" = %s"), "{sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
        assert!(!sql.contains("user_id"), "{sql}");
        // User lookup: caller-scoped .get — no LIMIT, user guard.
        let sql = bind(&user_asset_lookup_sql());
        assert!(sql.contains("\"user_id\" = %s"), "{sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
        // Workspace lookup: slug join via U0, no membership predicate.
        let sql = bind(&workspace_asset_lookup_sql());
        assert!(sql.contains("INNER JOIN \"workspaces\" U0"), "{sql}");
        assert!(sql.contains("U0.\"slug\" = %s"), "{sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(!sql.contains("member"), "{sql}");
        // Golden prose agrees on the scoping columns.
        let g = golden();
        assert!(g["UserAssetsV2Endpoint"]["delete"]["lookup_sql"]
            .as_str()
            .unwrap()
            .contains("user_id"));
        assert!(g["WorkspaceFileAssetEndpoint"]["delete"]["lookup_sql"]
            .as_str()
            .unwrap()
            .contains("workspace__slug"));
    }

    #[test]
    fn write_sql_shapes() {
        // User insert: fixed 6 columns, caller twice (user + created_by),
        // no workspace FK, RETURNING id.
        let sql = user_insert_sql();
        for col in [
            "attributes",
            "asset",
            "size",
            "user_id",
            "created_by_id",
            "entity_type",
        ] {
            assert!(sql.contains(&format!("\"{col}\"")), "{sql}");
        }
        assert!(!sql.contains("workspace_id"), "{sql}");
        assert!(sql.contains("RETURNING"), "{sql}");
        // Workspace insert: fixed 6 + spread FK, or nothing on fallthrough.
        assert_eq!(
            workspace_insert_columns(Some("workspace_id")).len(),
            7,
            "spread adds the FK column"
        );
        assert_eq!(
            workspace_insert_columns(None).len(),
            6,
            "fallthrough spreads nothing"
        );
        let sql = workspace_insert_sql(Some("workspace_id"));
        assert!(sql.contains("\"workspace_id\""), "{sql}");
        assert!(sql.contains("$7"), "{sql}");
        let sql = workspace_insert_sql(None);
        assert!(!sql.contains("$7"), "{sql}");
        // Confirm + delete updates carry the golden update_fields.
        let sql = confirm_update_sql();
        assert!(sql.contains("\"is_uploaded\" = $1"), "{sql}");
        assert!(sql.contains("\"attributes\" = $2"), "{sql}");
        let sql = delete_update_sql();
        assert!(sql.contains("\"is_deleted\" = $1"), "{sql}");
        assert!(sql.contains("\"deleted_at\" = $2"), "{sql}");
        // Entity-link flip renders the golden FK columns.
        assert_eq!(
            entity_link_update_sql(USER_TABLE, USER_AVATAR_FK),
            "UPDATE \"users\" SET \"avatar_asset_id\" = $1 WHERE \"id\" = $2",
        );
        assert_eq!(
            entity_link_update_sql(WORKSPACE_TABLE, WORKSPACE_LOGO_FK),
            "UPDATE \"workspaces\" SET \"logo_asset_id\" = $1 WHERE \"id\" = $2",
        );
        assert_eq!(
            entity_link_update_sql(PROJECT_TABLE, PROJECT_COVER_FK),
            "UPDATE \"projects\" SET \"cover_image_asset_id\" = $1 WHERE \"id\" = $2",
        );
    }

    #[test]
    fn entity_actions_match_golden() {
        assert_eq!(user_save_action("USER_AVATAR"), UserSaveAction::SetAvatar);
        assert_eq!(user_save_action("USER_COVER"), UserSaveAction::SetCover);
        assert_eq!(user_save_action("WORKSPACE_LOGO"), UserSaveAction::Noop);
        assert_eq!(
            user_delete_action("USER_AVATAR"),
            UserDeleteAction::ClearAvatar
        );
        assert_eq!(
            user_delete_action("USER_COVER"),
            UserDeleteAction::ClearCover
        );
        assert_eq!(
            user_delete_action("ISSUE_ATTACHMENT"),
            UserDeleteAction::Noop
        );
        assert_eq!(
            workspace_save_action("WORKSPACE_LOGO"),
            WorkspaceSaveAction::SetLogo
        );
        assert_eq!(
            workspace_save_action("PROJECT_COVER"),
            WorkspaceSaveAction::SetCover
        );
        // ISSUE/PAGE/COMMENT/DRAFT link NOTHING on patch (v2.py:283-284).
        for t in [
            "ISSUE_ATTACHMENT",
            "PAGE_DESCRIPTION",
            "COMMENT_DESCRIPTION",
        ] {
            assert_eq!(workspace_save_action(t), WorkspaceSaveAction::Noop, "{t}");
            assert_eq!(
                workspace_delete_action(t),
                WorkspaceDeleteAction::Noop,
                "{t}"
            );
        }
        // Save reads through filter().first(); the logo delete through
        // .get (dead None check ported — no None arm exists).
        assert_eq!(
            workspace_save_lookup(WorkspaceSaveAction::SetLogo),
            Some(WorkspaceLookup::FilterFirst)
        );
        assert_eq!(
            workspace_save_lookup(WorkspaceSaveAction::SetCover),
            Some(WorkspaceLookup::FilterFirst)
        );
        assert_eq!(
            workspace_delete_lookup(WorkspaceDeleteAction::ClearLogo),
            Some(WorkspaceLookup::Get)
        );
        assert_eq!(
            workspace_delete_lookup(WorkspaceDeleteAction::ClearCover),
            Some(WorkspaceLookup::FilterFirst)
        );
        // Clear-value asymmetry ported as-is.
        assert_eq!(USER_AVATAR_CLEARED, "");
        assert_eq!(USER_COVER_CLEARED, None);
        assert_eq!(WORKSPACE_LOGO_CLEARED, "");
        assert_eq!(PROJECT_COVER_CLEARED, "");
        assert_eq!(USER_AVATAR_FK, "avatar_asset_id");
        assert_eq!(USER_COVER_FK, "cover_image_asset_id");
        assert_eq!(WORKSPACE_LOGO_FK, "logo_asset_id");
        assert_eq!(PROJECT_COVER_FK, "cover_image_asset_id");
    }

    #[test]
    fn post_commit_invalidations_match_golden() {
        let g = golden();
        let text = serde_json::to_string(&g).expect("golden serializes");
        for inv in USER_POST_COMMIT {
            assert!(text.contains(inv.path), "golden names {}", inv.path);
            assert!(inv.user, "user paths invalidate per-user");
        }
        assert_eq!(USER_POST_COMMIT.len(), 2);
        let want = [
            ("/api/workspaces/", false),
            ("/api/users/me/workspaces/", true),
            ("/api/instances/", false),
        ];
        assert_eq!(WORKSPACE_LOGO_POST_COMMIT.len(), want.len());
        for (inv, (path, user)) in WORKSPACE_LOGO_POST_COMMIT.iter().zip(want) {
            assert_eq!(inv.path, path);
            assert_eq!(inv.user, user);
            assert!(text.contains(path), "golden names {path}");
        }
    }

    #[test]
    fn metadata_publisher_predicate() {
        // Falsy fires (None and {} alike), truthy skips.
        assert!(should_publish_metadata(None));
        for falsy in [
            json!(null),
            json!(""),
            json!({}),
            json!([]),
            json!(false),
            json!(0),
        ] {
            assert!(should_publish_metadata(Some(&falsy)), "{falsy}");
        }
        for truthy in [
            json!({"sha": "x"}),
            json!("x"),
            json!([1]),
            json!(true),
            json!(3),
        ] {
            assert!(!should_publish_metadata(Some(&truthy)), "{truthy}");
        }
        assert_eq!(METADATA_TASK_KWARG, "asset_id");
        let g = golden();
        assert!(g["UserAssetsV2Endpoint"]["patch"]["metadata_publisher"]
            .as_str()
            .unwrap()
            .contains("asset_id"));
    }

    #[test]
    fn workspace_entity_id_default_is_false() {
        assert_eq!(
            workspace_entity_id(None),
            WorkspaceEntityId::MissingBecomesFalse,
            "missing identifier spreads False (v2.py:319)"
        );
        assert_eq!(
            workspace_entity_id(Some("id-1")),
            WorkspaceEntityId::Value("id-1")
        );
        let g = golden();
        assert!(g["WorkspaceFileAssetEndpoint"]["post"]["defaults"]
            .as_str()
            .unwrap()
            .contains("FALSE"));
    }

    #[test]
    fn not_uploaded_body_matches_golden() {
        let g = golden();
        assert_eq!(
            not_uploaded_body(),
            g["WorkspaceFileAssetEndpoint"]["get"]["not_uploaded_body"],
        );
        // No `status` key — ported exactly.
        assert!(not_uploaded_body().get("status").is_none());
        assert_eq!(
            NOT_UPLOADED_STATUS,
            g["WorkspaceFileAssetEndpoint"]["get"]["not_uploaded_status"]
                .as_u64()
                .expect("status") as u16,
        );
    }
}
