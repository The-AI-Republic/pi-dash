//! v2 project-side asset queries (D-31, stage 5).
//!
//! Port of the query closure in `apps/api/pi_dash/app/views/asset/v2.py:432-835`:
//! `StaticFileAssetEndpoint` (`:432-465`), `AssetRestoreEndpoint` (`:468-477`),
//! `ProjectAssetEndpoint` (`:480-627`), `ProjectBulkAssetEndpoint` (`:630-688`),
//! `AssetCheckEndpoint` (`:691-697`), `DuplicateAssetEndpoint` (`:700-780`),
//! `WorkspaceAssetDownloadEndpoint` (`:783-807`) and
//! `ProjectAssetDownloadEndpoint` (`:810-835`).
//!
//! Everything here is pure: SQL text with PostgreSQL `$N` binds, the
//! `get_entity_id_field` column maps, storage-key shapes and presigned-call
//! shapes. Execution, S3 calls and queue writes live behind the handler/jobs
//! layers. Tests replay `rust-api/fixtures/app_assets/queries/`
//! `v2_project.golden.json` (PIDASHCONV-306) with no database.
//!
//! SQL semantics are Django's (translate, don't redesign):
//! * Reads through the default manager carry `deleted_at IS NULL`
//!   (`db/mixins.py:56-58`); reads through `all_objects` (restore lookup,
//!   check) carry no manager scope — check adds an explicit
//!   `deleted_at__isnull` filter (`v2.py:696`), restore adds none.
//! * `workspace__slug` filters are a join to `workspaces`
//!   (`workspaces.id = file_assets.workspace_id AND workspaces.slug = $N`).
//! * `QuerySet.update(...)` writes the named columns only (no `updated_at`
//!   bump); `save(update_fields=[...])` writes exactly the named columns
//!   too (Django builds the UPDATE from that list only — no `auto_now`
//!   bump for the rest).
//! * `save_project_cover` (`v2.py:631-634`) re-gets the project per row and
//!   the LAST row wins.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG (`v2.py:554-563`): project `post` with `PROJECT_COVER` resolves
//!   `get_entity_id_field` to `{"project_id": ...}`, colliding with the
//!   explicit `project_id=project_id` kwarg → `TypeError` → generic 500.
//!   [`project_create_columns`] keeps the spread order (explicit kwarg
//!   first, `**spread` second) so the collision reproduces.
//! * BUG-FLAG (`v2.py:609`): project `get` looks up `pk=pk` while siblings
//!   use `id=pk`. Django aliases `pk` to the primary key, so the SQL is
//!   identical; [`PROJECT_GET_SQL`] keeps the same predicate for trace
//!   fidelity.
//! * QUIRK (`v2.py:739`): duplicate `post` reads `entity_id`, not
//!   `entity_identifier` — sending `entity_identifier` leaves all entity
//!   FKs NULL. [`DUPLICATE_ENTITY_ID_REQUEST_KEY`] pins the key name.
//! * QUIRK (`v2.py:761-778`): the duplicate row is created BEFORE
//!   `copy_object`; on storage failure the API 500s while the row stays.
//!   The is_uploaded flip runs unconditionally after the copy returns.
//! * QUIRK (`v2.py:760`): the destination key renders
//!   `original.attributes.get('name')` with no fallback, so a missing name
//!   renders the literal `...-None`. [`duplicate_destination_key`] ports
//!   the `.get` shape with no fallback.

/// `FileAsset` table (merged `db` const agrees on this name).
pub const FILE_ASSET_TABLE: &str = "file_assets";
/// Tenant table for `workspace__slug` joins.
pub const WORKSPACE_TABLE: &str = "workspaces";
/// Projects table for the cover-link write.
pub const PROJECT_TABLE: &str = "projects";

// ---------------------------------------------------------------------------
// Entity-type vocabulary
// ---------------------------------------------------------------------------

/// Response when the project-mint `entity_type` is not a known value
/// (`v2.py:521-526`).
pub const INVALID_ENTITY_TYPE_BODY: &str = r#"{"error":"Invalid entity type.","status":false}"#;

/// Response when the duplicate `entity_type` is missing or unknown
/// (`v2.py:742-746`) — note the message text differs from the mint
/// endpoints (`Invalid entity type or entity id`, no `status` key).
pub const DUPLICATE_INVALID_ENTITY_BODY: &str = r#"{"error":"Invalid entity type or entity id"}"#;

/// `entity_type` values the static endpoint serves (`v2.py:449-456`).
pub const STATIC_ENTITY_ALLOWLIST: &[&str] = &[
    "USER_AVATAR",
    "USER_COVER",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
];

/// Mintable file types for the project `post` (`v2.py:528-535`).
pub const PROJECT_ALLOWED_FILE_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// Request key the duplicate endpoint reads the entity id from
/// (`v2.py:739` — QUIRK, ported as-is: `entity_id`, not
/// `entity_identifier`).
pub const DUPLICATE_ENTITY_ID_REQUEST_KEY: &str = "entity_id";

/// Port of `ProjectAssetEndpoint.get_entity_id_field` (`v2.py:483-510`):
/// the create-kwarg column for an entity type, or `None` for the `{}` fallthrough.
/// 8 branches — the workspace variant's 7 plus `DRAFT_ISSUE_DESCRIPTION`.
/// Do not unify the two maps.
pub fn project_entity_id_field(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "WORKSPACE_LOGO" => Some("workspace_id"),
        "PROJECT_COVER" => Some("project_id"),
        "USER_AVATAR" | "USER_COVER" => Some("user_id"),
        "ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION" => Some("issue_id"),
        "PAGE_DESCRIPTION" => Some("page_id"),
        "COMMENT_DESCRIPTION" => Some("comment_id"),
        "DRAFT_ISSUE_DESCRIPTION" => Some("draft_issue_id"),
        _ => None,
    }
}

/// Port of `DuplicateAssetEndpoint.get_entity_id_field` (`v2.py:703-734`):
/// 7 branches — NO `DRAFT_ISSUE_DESCRIPTION` arm (differs from the project
/// variant; port the difference as-is).
pub fn duplicate_entity_id_field(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "WORKSPACE_LOGO" => Some("workspace_id"),
        "PROJECT_COVER" => Some("project_id"),
        "USER_AVATAR" | "USER_COVER" => Some("user_id"),
        "ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION" => Some("issue_id"),
        "PAGE_DESCRIPTION" => Some("page_id"),
        "COMMENT_DESCRIPTION" => Some("comment_id"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Storage-key shapes
// ---------------------------------------------------------------------------

/// Port of the project-mint asset key (`v2.py:551`):
/// `f"{workspace.id}/{uuid.uuid4().hex}-{name}"`.
pub fn project_asset_key(workspace_id: &str, uuid_hex: &str, name: &str) -> String {
    format!("{workspace_id}/{uuid_hex}-{name}")
}

/// Port of the duplicate destination key (`v2.py:760`):
/// `f"{workspace.id}/{uuid.uuid4().hex}-{original.attributes.get('name')}"`.
/// `original_name` is the `.get('name')` result, so `None` renders as the
/// literal `None` — ported with no fallback.
pub fn duplicate_destination_key(
    workspace_id: &str,
    uuid_hex: &str,
    original_name: Option<&str>,
) -> String {
    match original_name {
        Some(name) => format!("{workspace_id}/{uuid_hex}-{name}"),
        None => format!("{workspace_id}/{uuid_hex}-None"),
    }
}

/// Column order for the project-mint create (`v2.py:554-563`): the explicit
/// `project_id=project_id` kwarg comes FIRST and the
/// `**get_entity_id_field(...)` spread SECOND — so `PROJECT_COVER` (which
/// spreads `{"project_id": ...}`) collides and raises `TypeError` (500).
/// The `Vec` order below is that kwarg order; handlers must apply it
/// verbatim.
pub fn project_create_columns(entity_type: &str) -> Vec<&'static str> {
    let mut cols = vec![
        "attributes",
        "asset",
        "size",
        "workspace_id",
        "created_by_id",
        "entity_type",
        "project_id",
    ];
    if let Some(spread) = project_entity_id_field(entity_type) {
        cols.push(spread);
    }
    cols
}

// ---------------------------------------------------------------------------
// Static endpoint (`v2.py:432-465`)
// ---------------------------------------------------------------------------

/// `.get(id)` — NO workspace scoping (`v2.py:439`); unknown ids 404 via the
/// base-view handler. Default-manager scope only.
pub const STATIC_GET_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1)";

// ---------------------------------------------------------------------------
// Restore endpoint (`v2.py:468-477`)
// ---------------------------------------------------------------------------

/// `all_objects.get(id, workspace__slug)` (`v2.py:473`) — sees soft-deleted
/// rows; no `deleted_at` predicate.
pub const RESTORE_GET_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE \"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2";

/// `is_deleted=False; deleted_at=None; save(...)` (`v2.py:474-476`).
/// Idempotent on live rows.
pub const RESTORE_SAVE_SQL: &str = "UPDATE \"file_assets\" SET \"is_deleted\" = FALSE, \"deleted_at\" = NULL WHERE \"file_assets\".\"id\" = $1";

// ---------------------------------------------------------------------------
// Project endpoint reads (`v2.py:579-627`)
// ---------------------------------------------------------------------------

/// Shared scoped-read predicate for patch/delete/get: default manager plus
/// `workspace__slug` join plus `project_id` (`v2.py:582,598,609`).
/// `$1` is the asset id, `$2` the workspace slug, `$3` the project id.
/// The `get` spelling uses `pk=pk` (BUG-FLAG, identical SQL — kept).
pub const PROJECT_SCOPED_GET_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"file_assets\".\"project_id\" = $3)";

/// Alias kept so the `pk=pk` quirk (`v2.py:609`) is traceable at the call
/// site: same text as [`PROJECT_SCOPED_GET_SQL`] by construction.
pub const PROJECT_GET_SQL: &str = PROJECT_SCOPED_GET_SQL;

/// `save(update_fields=["is_uploaded", "attributes"])` (`v2.py:592`):
/// flips `is_uploaded` and replaces-or-keeps `attributes` — exactly these
/// two columns, no `updated_at` bump (`update_fields` filters the UPDATE).
pub const PROJECT_PATCH_SAVE_SQL: &str = "UPDATE \"file_assets\" SET \"is_uploaded\" = TRUE, \"attributes\" = $2 WHERE \"file_assets\".\"id\" = $1";

/// `is_deleted=True; deleted_at=now; save(...)` (`v2.py:600-603`) — BOTH
/// columns, unlike the v1 delete.
pub const PROJECT_DELETE_SQL: &str = "UPDATE \"file_assets\" SET \"is_deleted\" = TRUE, \"deleted_at\" = CURRENT_TIMESTAMP WHERE \"file_assets\".\"id\" = $1";

/// Metadata publisher guard (`v2.py:586-587`): enqueued only when the row
/// has no `storage_metadata`, with `asset_id=str(pk)` — the URL kwarg.
/// Payload shape lives in `super::tasks::metadata_delay_kwargs`.
pub const PROJECT_PATCH_METADATA_KWARG_SOURCE: &str = "pk";

// ---------------------------------------------------------------------------
// Bulk endpoint (`v2.py:630-688`)
// ---------------------------------------------------------------------------

/// `filter(id__in=asset_ids, workspace__slug=slug)` (`v2.py:645`).
/// `$1` is the id array (`= ANY`), `$2` the workspace slug.
pub const BULK_LOOKUP_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = ANY($1) AND \"workspaces\".\"slug\" = $2)";

/// One bulk dispatch arm: the branch Django takes off `assets.first()`'s
/// `entity_type` (`v2.py:657-686`). A mixed-type id list follows ONLY the
/// first row — single dispatch, ported as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkBranch {
    ProjectCover,
    IssueDescription,
    CommentDescription,
    PageDescription,
    DraftIssueDescription,
    /// No arm matches: 204 with no writes.
    Noop,
}

/// Resolve the dispatch arm from the first row's `entity_type`.
pub fn bulk_branch(first_entity_type: &str) -> BulkBranch {
    match first_entity_type {
        "PROJECT_COVER" => BulkBranch::ProjectCover,
        "ISSUE_DESCRIPTION" => BulkBranch::IssueDescription,
        "COMMENT_DESCRIPTION" => BulkBranch::CommentDescription,
        "PAGE_DESCRIPTION" => BulkBranch::PageDescription,
        "DRAFT_ISSUE_DESCRIPTION" => BulkBranch::DraftIssueDescription,
        _ => BulkBranch::Noop,
    }
}

/// Per-branch `UPDATE` text. `$1` is the id array, `$2` the entity link id
/// (`entity_id` path arg), `$3` the project id where the branch sets it.
/// `IntegrityError` is swallowed (`pass`) on every branch except
/// `PAGE_DESCRIPTION` and `PROJECT_COVER` (`v2.py:664-667,672-675,683-686`).
pub fn bulk_update_sql(branch: BulkBranch) -> Option<&'static str> {
    match branch {
        BulkBranch::ProjectCover => Some(
            "UPDATE \"file_assets\" SET \"project_id\" = $3 WHERE \"file_assets\".\"id\" = ANY($1)",
        ),
        BulkBranch::IssueDescription => Some(
            "UPDATE \"file_assets\" SET \"issue_id\" = $2, \"project_id\" = $3 WHERE \"file_assets\".\"id\" = ANY($1)",
        ),
        // NOTE: no `project_id` here — differs from the issue branch.
        BulkBranch::CommentDescription => Some(
            "UPDATE \"file_assets\" SET \"comment_id\" = $2 WHERE \"file_assets\".\"id\" = ANY($1)",
        ),
        BulkBranch::PageDescription => Some(
            "UPDATE \"file_assets\" SET \"page_id\" = $2 WHERE \"file_assets\".\"id\" = ANY($1)",
        ),
        BulkBranch::DraftIssueDescription => Some(
            "UPDATE \"file_assets\" SET \"draft_issue_id\" = $2 WHERE \"file_assets\".\"id\" = ANY($1)",
        ),
        BulkBranch::Noop => None,
    }
}

/// Whether the branch swallows `IntegrityError` (`v2.py:664-667,672-675,683-686`).
pub fn bulk_swallows_integrity_error(branch: BulkBranch) -> bool {
    match branch {
        BulkBranch::IssueDescription
        | BulkBranch::CommentDescription
        | BulkBranch::DraftIssueDescription => true,
        BulkBranch::ProjectCover | BulkBranch::PageDescription | BulkBranch::Noop => false,
    }
}

/// `save_project_cover` (`v2.py:631-634`): per row, re-get the project and
/// stamp `cover_image_asset_id`; the LAST row wins. The re-get uses the
/// default manager, so it carries `deleted_at IS NULL`.
pub const SAVE_PROJECT_COVER_GET_SQL: &str =
    "SELECT \"projects\".\"id\" FROM \"projects\" WHERE \"projects\".\"id\" = $1 AND \"projects\".\"deleted_at\" IS NULL";
pub const SAVE_PROJECT_COVER_SQL: &str =
    "UPDATE \"projects\" SET \"cover_image_asset_id\" = $2 WHERE \"projects\".\"id\" = $1";

// ---------------------------------------------------------------------------
// Check endpoint (`v2.py:691-697`)
// ---------------------------------------------------------------------------

/// `all_objects.filter(id, workspace__slug, deleted_at__isnull).exists()`
/// (`v2.py:696`) — sees `is_deleted`-only rows (e.g. v1-deleted rows report
/// `exists: true`).
pub const CHECK_EXISTS_SQL: &str = "SELECT EXISTS(SELECT 1 FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE \"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"file_assets\".\"deleted_at\" IS NULL)";

// ---------------------------------------------------------------------------
// Duplicate endpoint (`v2.py:700-780`)
// ---------------------------------------------------------------------------

/// `filter(id, is_uploaded=True).first()` on the DEFAULT manager
/// (`v2.py:755`) — soft-deleted originals are invisible; `None` → 404
/// `Asset not found`.
pub const DUPLICATE_ORIGINAL_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 AND \"file_assets\".\"is_uploaded\")";

/// `filter(id=project_id, workspace=workspace).exists()` (`v2.py:750`) —
/// only when `project_id` is given. Default-manager scope: a soft-deleted
/// project reads as missing → 404.
pub const DUPLICATE_PROJECT_CHECK_SQL: &str = "SELECT EXISTS(SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = $1 AND \"projects\".\"workspace_id\" = $2 AND \"projects\".\"deleted_at\" IS NULL)";

/// `FileAsset.objects.filter(id=new).update(is_uploaded=True)` (`v2.py:778`)
/// — runs unconditionally after `copy_object` returns, whatever the copy did
/// (copy failures are swallowed inside `copy_object`).
pub const DUPLICATE_MARK_UPLOADED_SQL: &str =
    "UPDATE \"file_assets\" SET \"is_uploaded\" = TRUE WHERE \"file_assets\".\"id\" = $1";

/// Column order for the duplicate create (`v2.py:761-775`): `created_by`
/// passes the `_id` form (`created_by_id=request.user.id`, `:770` — unlike
/// the mint endpoints) and `storage_metadata` is copied verbatim.
pub const DUPLICATE_CREATE_COLUMNS: &[&str] = &[
    "attributes",
    "asset",
    "size",
    "workspace_id",
    "created_by_id",
    "entity_type",
    "project_id",
    "storage_metadata",
];

// ---------------------------------------------------------------------------
// Download endpoints (`v2.py:783-835`)
// ---------------------------------------------------------------------------

/// Workspace download lookup (`v2.py:788-795`): default-manager scope
/// (`deleted_at IS NULL`) plus `is_uploaded` in the LOOKUP (a
/// present-but-unuploaded row answers the same 404).
pub const WORKSPACE_DOWNLOAD_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE (\"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"file_assets\".\"is_uploaded\" AND \"file_assets\".\"deleted_at\" IS NULL)";

/// Project download lookup (`v2.py:815-823`): as above plus `project_id`
/// scoping — a row without this project 404s.
pub const PROJECT_DOWNLOAD_SQL: &str = "SELECT \"file_assets\".\"id\" FROM \"file_assets\" INNER JOIN \"workspaces\" ON \"workspaces\".\"id\" = \"file_assets\".\"workspace_id\" WHERE (\"file_assets\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"file_assets\".\"project_id\" = $3 AND \"file_assets\".\"is_uploaded\" AND \"file_assets\".\"deleted_at\" IS NULL)";

/// Presigned-call shape for the fetch/download redirects: `disposition`
/// and how `filename` resolves. Static uses bare defaults (inline);
/// the rest force `attachment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresignedShape {
    /// `generate_presigned_url(object_name)` (`v2.py:463`) — no
    /// disposition/filename args (inline default).
    StaticInline,
    /// `generate_presigned_url(object_name, disposition="attachment",
    /// filename=attributes.get("name"))` (`v2.py:621-625`) — plain `.get`,
    /// no fallback.
    AttachmentName,
    /// As above but `attributes.get("name", uuidhex)` (`v2.py:800-805,
    /// :828-833`) — fresh uuid hex fallback when `name` is absent.
    AttachmentNameOrUuid,
}

/// Resolve the download filename for [`PresignedShape::AttachmentNameOrUuid`]:
/// `attributes.get("name", uuid.uuid4().hex)` — the fallback is a FRESH hex
/// per call, supplied by the caller.
pub fn download_filename(attributes_name: Option<&str>, fresh_uuid_hex: &str) -> String {
    attributes_name.unwrap_or(fresh_uuid_hex).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/app_assets/queries/v2_project.golden.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_assets/queries/v2_project.golden.json");

    fn golden() -> Value {
        serde_json::from_str(FIXTURE).expect("golden parses")
    }

    #[test]
    fn project_entity_map_has_eight_branches() {
        let g = golden();
        let branches = g["ProjectAssetEndpoint"]["get_entity_id_field"]["branches"]
            .as_str()
            .expect("branches prose");
        assert!(branches.contains("DRAFT_ISSUE_DESCRIPTION"));
        assert_eq!(
            project_entity_id_field("WORKSPACE_LOGO"),
            Some("workspace_id")
        );
        assert_eq!(project_entity_id_field("PROJECT_COVER"), Some("project_id"));
        assert_eq!(project_entity_id_field("USER_AVATAR"), Some("user_id"));
        assert_eq!(project_entity_id_field("USER_COVER"), Some("user_id"));
        assert_eq!(
            project_entity_id_field("ISSUE_ATTACHMENT"),
            Some("issue_id")
        );
        assert_eq!(
            project_entity_id_field("ISSUE_DESCRIPTION"),
            Some("issue_id")
        );
        assert_eq!(project_entity_id_field("PAGE_DESCRIPTION"), Some("page_id"));
        assert_eq!(
            project_entity_id_field("COMMENT_DESCRIPTION"),
            Some("comment_id")
        );
        assert_eq!(
            project_entity_id_field("DRAFT_ISSUE_DESCRIPTION"),
            Some("draft_issue_id")
        );
        assert_eq!(project_entity_id_field("NOPE"), None);
    }

    #[test]
    fn duplicate_entity_map_has_no_draft_branch() {
        let g = golden();
        let branches = g["DuplicateAssetEndpoint"]["get_entity_id_field"]["branches"]
            .as_str()
            .expect("branches prose");
        assert!(branches.contains("No DRAFT branch"));
        assert_eq!(
            duplicate_entity_id_field("WORKSPACE_LOGO"),
            Some("workspace_id")
        );
        assert_eq!(
            duplicate_entity_id_field("PROJECT_COVER"),
            Some("project_id")
        );
        assert_eq!(duplicate_entity_id_field("USER_AVATAR"), Some("user_id"));
        assert_eq!(
            duplicate_entity_id_field("ISSUE_ATTACHMENT"),
            Some("issue_id")
        );
        assert_eq!(
            duplicate_entity_id_field("PAGE_DESCRIPTION"),
            Some("page_id")
        );
        assert_eq!(
            duplicate_entity_id_field("COMMENT_DESCRIPTION"),
            Some("comment_id")
        );
        assert_eq!(duplicate_entity_id_field("DRAFT_ISSUE_DESCRIPTION"), None);
        // The request key quirk is pinned, not unified with the mint path.
        assert_eq!(DUPLICATE_ENTITY_ID_REQUEST_KEY, "entity_id");
        assert!(g["DuplicateAssetEndpoint"]["post"]["duplicate_kwarg_quirk"]
            .as_str()
            .expect("quirk prose")
            .contains("entity_id"));
    }

    #[test]
    fn project_cover_create_collides_on_project_id() {
        let g = golden();
        let bug = g["ProjectAssetEndpoint"]["post"]["project_cover_500"]
            .as_str()
            .expect("bug prose");
        assert!(bug.contains("COLLIDES"));
        // Explicit kwarg first, **spread second: PROJECT_COVER repeats it.
        let cols = project_create_columns("PROJECT_COVER");
        assert_eq!(cols[6], "project_id");
        assert_eq!(cols[7], "project_id");
        // Non-cover types spread a distinct column.
        let cols = project_create_columns("ISSUE_DESCRIPTION");
        assert_eq!(cols[6], "project_id");
        assert_eq!(cols[7], "issue_id");
        // Unknown types spread nothing.
        assert_eq!(project_create_columns("NOPE").len(), 7);
    }

    #[test]
    fn scoped_reads_carry_slug_join_project_and_active_scope() {
        for sql in [PROJECT_SCOPED_GET_SQL, PROJECT_GET_SQL] {
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
            assert!(sql.contains("\"workspaces\".\"slug\" = $2"), "{sql}");
            assert!(sql.contains("\"project_id\" = $3"), "{sql}");
        }
        // get's pk=pk quirk is the identical predicate, kept for trace fidelity.
        assert_eq!(PROJECT_GET_SQL, PROJECT_SCOPED_GET_SQL);
        let g = golden();
        assert!(g["ProjectAssetEndpoint"]["get"]["pk_quirk"]
            .as_str()
            .expect("quirk prose")
            .contains("IDENTICAL"));
        // Patch saves exactly is_uploaded + attributes; delete stamps both columns.
        assert!(PROJECT_PATCH_SAVE_SQL.contains("\"is_uploaded\" = TRUE"));
        assert!(PROJECT_PATCH_SAVE_SQL.contains("\"attributes\" = $2"));
        // save(update_fields=[...]) writes the named columns only: no auto_now bump.
        assert!(!PROJECT_PATCH_SAVE_SQL.contains("updated_at"));
        assert!(PROJECT_DELETE_SQL.contains("\"is_deleted\" = TRUE"));
        assert!(PROJECT_DELETE_SQL.contains("\"deleted_at\" = CURRENT_TIMESTAMP"));
        // Patch metadata publisher passes the URL kwarg pk.
        assert_eq!(PROJECT_PATCH_METADATA_KWARG_SOURCE, "pk");
    }

    #[test]
    fn bulk_dispatch_is_single_first_row_with_per_branch_writes() {
        let g = golden();
        assert!(g["ProjectBulkAssetEndpoint"]["first_asset_governs"]
            .as_str()
            .expect("prose")
            .contains("ONLY the first"));
        assert_eq!(bulk_branch("PROJECT_COVER"), BulkBranch::ProjectCover);
        assert_eq!(
            bulk_branch("ISSUE_DESCRIPTION"),
            BulkBranch::IssueDescription
        );
        assert_eq!(
            bulk_branch("COMMENT_DESCRIPTION"),
            BulkBranch::CommentDescription
        );
        assert_eq!(bulk_branch("PAGE_DESCRIPTION"), BulkBranch::PageDescription);
        assert_eq!(
            bulk_branch("DRAFT_ISSUE_DESCRIPTION"),
            BulkBranch::DraftIssueDescription
        );
        assert_eq!(bulk_branch("USER_AVATAR"), BulkBranch::Noop);
        assert_eq!(bulk_update_sql(BulkBranch::Noop), None);
        // Comment branch sets comment_id but NOT project_id (differs from issue).
        let comment = bulk_update_sql(BulkBranch::CommentDescription).expect("sql");
        assert!(comment.contains("\"comment_id\" = $2"));
        assert!(!comment.contains("project_id"));
        let issue = bulk_update_sql(BulkBranch::IssueDescription).expect("sql");
        assert!(issue.contains("\"issue_id\" = $2"));
        assert!(issue.contains("\"project_id\" = $3"));
        // Swallow matrix: issue/comment/draft yes, page/cover/noop no.
        assert!(bulk_swallows_integrity_error(BulkBranch::IssueDescription));
        assert!(bulk_swallows_integrity_error(
            BulkBranch::CommentDescription
        ));
        assert!(bulk_swallows_integrity_error(
            BulkBranch::DraftIssueDescription
        ));
        assert!(!bulk_swallows_integrity_error(BulkBranch::PageDescription));
        assert!(!bulk_swallows_integrity_error(BulkBranch::ProjectCover));
        // Cover loop re-gets the project per row; last row wins (documented).
        assert!(SAVE_PROJECT_COVER_SQL.contains("\"cover_image_asset_id\" = $2"));
        // The per-row project re-get uses the default manager.
        assert!(SAVE_PROJECT_COVER_GET_SQL.contains("\"deleted_at\" IS NULL"));
        // Lookup scopes ids + slug + active rows.
        assert!(BULK_LOOKUP_SQL.contains("= ANY($1)"));
        assert!(BULK_LOOKUP_SQL.contains("\"workspaces\".\"slug\" = $2"));
        assert!(BULK_LOOKUP_SQL.contains("\"deleted_at\" IS NULL"));
    }

    #[test]
    fn check_and_restore_read_through_all_objects() {
        // Check: explicit deleted_at__isnull exists() over all_objects.
        assert!(CHECK_EXISTS_SQL.contains("SELECT EXISTS"));
        assert!(CHECK_EXISTS_SQL.contains("\"deleted_at\" IS NULL"));
        assert!(CHECK_EXISTS_SQL.contains("\"workspaces\".\"slug\" = $2"));
        // Restore: all_objects get with NO deleted_at predicate, unflip on save.
        assert!(!RESTORE_GET_SQL.contains("deleted_at"));
        assert!(RESTORE_GET_SQL.contains("\"workspaces\".\"slug\" = $2"));
        assert!(RESTORE_SAVE_SQL.contains("\"is_deleted\" = FALSE"));
        assert!(RESTORE_SAVE_SQL.contains("\"deleted_at\" = NULL"));
    }

    #[test]
    fn duplicate_lookup_key_copy_order_and_mark_uploaded() {
        // Original lookup requires is_uploaded on the default manager.
        assert!(DUPLICATE_ORIGINAL_SQL.contains("\"is_uploaded\""));
        assert!(DUPLICATE_ORIGINAL_SQL.contains("\"deleted_at\" IS NULL"));
        // Destination key renders a missing name as literal None (no fallback).
        assert_eq!(
            duplicate_destination_key("ws", "hex", Some("a.png")),
            "ws/hex-a.png"
        );
        assert_eq!(duplicate_destination_key("ws", "hex", None), "ws/hex-None");
        let g = golden();
        assert!(g["DuplicateAssetEndpoint"]["post"]["destination_key_shape"]
            .as_str()
            .expect("prose")
            .contains("no fallback"));
        // Create uses the _id form for created_by (unlike mint endpoints).
        assert!(DUPLICATE_CREATE_COLUMNS.contains(&"created_by_id"));
        assert!(DUPLICATE_CREATE_COLUMNS.contains(&"storage_metadata"));
        // The is_uploaded flip runs unconditionally after the copy.
        assert!(DUPLICATE_MARK_UPLOADED_SQL.contains("\"is_uploaded\" = TRUE"));
        // The project-exists check reads through the default manager.
        assert!(DUPLICATE_PROJECT_CHECK_SQL.contains("\"deleted_at\" IS NULL"));
    }

    #[test]
    fn download_and_static_presigned_shapes() {
        // is_uploaded is part of the download lookups (same 404 either way).
        assert!(WORKSPACE_DOWNLOAD_SQL.contains("\"is_uploaded\""));
        assert!(PROJECT_DOWNLOAD_SQL.contains("\"is_uploaded\""));
        assert!(PROJECT_DOWNLOAD_SQL.contains("\"project_id\" = $3"));
        // Default-manager reads: soft-deleted rows 404 like missing rows.
        assert!(WORKSPACE_DOWNLOAD_SQL.contains("\"deleted_at\" IS NULL"));
        assert!(PROJECT_DOWNLOAD_SQL.contains("\"deleted_at\" IS NULL"));
        // Static lookup has NO workspace scoping (AllowAny, id only).
        assert!(!STATIC_GET_SQL.contains("workspaces"));
        assert!(STATIC_GET_SQL.contains("\"deleted_at\" IS NULL"));
        // Static allowlist is exactly the four golden entity types.
        assert_eq!(
            STATIC_ENTITY_ALLOWLIST,
            &[
                "USER_AVATAR",
                "USER_COVER",
                "WORKSPACE_LOGO",
                "PROJECT_COVER"
            ]
        );
        // Download filename falls back to a fresh uuid hex (unlike fetch .get).
        assert_eq!(download_filename(Some("a.png"), "hex"), "a.png");
        assert_eq!(download_filename(None, "hex"), "hex");
        assert_ne!(
            PresignedShape::StaticInline,
            PresignedShape::AttachmentNameOrUuid
        );
    }

    #[test]
    fn key_shapes_and_validation_consts_match_golden() {
        assert_eq!(project_asset_key("ws", "hex", "n.png"), "ws/hex-n.png");
        assert_eq!(
            PROJECT_ALLOWED_FILE_TYPES,
            &[
                "image/jpeg",
                "image/png",
                "image/webp",
                "image/jpg",
                "image/gif"
            ]
        );
        assert!(INVALID_ENTITY_TYPE_BODY.contains("Invalid entity type."));
        assert!(DUPLICATE_INVALID_ENTITY_BODY.contains("Invalid entity type or entity id"));
        // Duplicate error text differs from the mint text — pinned, not shared.
        assert_ne!(INVALID_ENTITY_TYPE_BODY, DUPLICATE_INVALID_ENTITY_BODY);
    }
}
