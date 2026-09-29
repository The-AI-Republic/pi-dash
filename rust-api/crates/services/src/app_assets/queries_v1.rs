#![forbid(unsafe_code)]

//! Legacy v1 asset reads/writes (D-31, stage 5).
//!
//! Ports `apps/api/pi_dash/app/views/asset/base.py:1-86` — one closure,
//! 3 classes / 7 methods — as declarative query builders: SQL text plus
//! the status/body contract per outcome. The HTTP handlers live in the
//! sibling handlers issue (PIDASHCONV-394); this module owns the data
//! plane only.
//!
//! Method map (trace : Python lines):
//! - `FileAssetEndpoint.get` (`:23-33`): [`workspace_asset_key`] +
//!   [`workspace_filter_sql`]; exists-branch 200 `{"data", "status": true}`
//!   vs miss-branch 200 [`miss_body`] (both 200 — port the quirk exactly).
//! - `FileAssetEndpoint.post` (`:35-42`): [`workspace_lookup_sql`] for the
//!   `Workspace.objects.get(slug=slug)` step, then the serializer insert
//!   (shape owned by PIDASHCONV-319); 201 on success.
//! - `FileAssetEndpoint.delete` (`:44-49`): [`workspace_get_sql`] +
//!   [`set_deleted_sql`] with `true`; 204, empty body.
//! - `FileAssetViewSet.restore` (`:52-58`): same lookup shape as delete,
//!   [`set_deleted_sql`] with `false`; 204, empty body.
//! - `UserAssetsEndpoint.get` (`:64-73`): [`user_filter_sql`]; miss-branch
//!   200 [`miss_body`]; the exists-branch 500s (ported bug, see below).
//! - `UserAssetsEndpoint.post` (`:75-80`): serializer insert, no FK kwargs;
//!   201 on success.
//! - `UserAssetsEndpoint.delete` (`:82-86`): [`user_get_sql`] (raw
//!   `asset_key`, no workspace prefix) + [`set_deleted_sql`] with `true`;
//!   204, empty body.
//!
//! SQL conventions (Porting guide data rules):
//! - Builders emit SQL text with Postgres `$n` placeholders; the caller
//!   splices them into its sqlx/sea-query statement (same split as
//!   `app_issues::ordering`, whose fragments the caller splices). Django
//!   spells placeholders `%s`; `$n` is the driver-level translation, the
//!   predicates are unchanged.
//! - Every read carries the default-manager scope `"deleted_at" IS NULL`
//!   (`SoftDeletionManager`, `db/mixins.py:57-59`; kernel reference
//!   `pidash_db::soft_delete::active_condition`). The v1 delete never sets
//!   `deleted_at` (ported bug below), so delete/restore round-trip through
//!   this scope; a row with `deleted_at` set is invisible here (404).
//! - Filter (multi-row) builders carry the model default ordering
//!   `ORDER BY "created_at" DESC` (`Meta.ordering = ["-created_at"]`,
//!   `db/models/asset.py:68`; Django applies it to `.filter()` with no
//!   explicit `order_by`). Single-row `.get()` builders end in `LIMIT 1`
//!   (fixture `queries/v1.golden.json` records the `.get` lookups this
//!   way).
//! - `SELECT *` is the full column list in
//!   [`columns::COLUMNS`][pidash_db::app_assets::columns::COLUMNS] order
//!   (Django selects every concrete field when no `.values()` is used).
//!
//! Ported bugs (translate, don't redesign — recorded here, fixed nowhere):
//! - `BUG (base.py:47-48)`: delete/restore write ONLY `is_deleted` via
//!   `save(update_fields=["is_deleted"])`; `deleted_at` stays `NULL`, so a
//!   v1-deleted row REMAINS visible to reads (the oracle pins GET-after-
//!   DELETE returning `status: true`). [`set_deleted_sql`] names only
//!   `is_deleted`. (`update_fields` also excludes the `auto_now`
//!   `updated_at`: `_save_table` filters `non_pks_non_generated` down to
//!   the named fields before `pre_save`, so no timestamp is written.)
//! - `BUG (base.py:67)`: `UserAssetsEndpoint.get` serializes the queryset
//!   WITHOUT `many=True`, so any existing user row raises and the
//!   `handle_exception` fallback returns 500
//!   [`unhandled_body`]. [`USER_GET_FOUND_IS_500`] pins this: handlers
//!   must return 500 (never the row) when the user filter matches.
//!
//! Fixture: `rust-api/fixtures/app_assets/queries/v1.golden.json`
//! (PIDASHCONV-306). Unit tests replay it: table/column consts
//! cross-checked against `models/fileasset.columns.json`, WHERE fragments
//! against the recorded `*_sql`, bodies/statuses asserted equal.

use pidash_db::app_assets::columns;
use serde_json::{json, Value};

/// Compose the stored asset key for workspace-scoped endpoints
/// (`str(workspace_id) + "/" + asset_key`, `base.py:24,45,54`).
///
/// User-scoped endpoints (`UserAssetsEndpoint`) use the raw `asset_key`
/// with no composition (`base.py:65,83`) — callers must NOT route user
/// keys through this function.
pub fn workspace_asset_key(workspace_id: &str, asset_key: &str) -> String {
    format!("{workspace_id}/{asset_key}")
}

/// Multi-row read for `FileAssetEndpoint.get` (`base.py:25`):
/// `FileAsset.objects.filter(asset=asset_key)` — default-manager scope,
/// model ordering. Params: `$1` = composed asset key.
pub fn workspace_filter_sql() -> String {
    format!(
        "SELECT * FROM \"{table}\" WHERE \"asset\" = $1 AND \"deleted_at\" IS NULL \
         ORDER BY \"created_at\" DESC",
        table = columns::TABLE,
    )
}

/// Single-row read for `FileAssetEndpoint.delete` (`base.py:46`) and
/// `FileAssetViewSet.restore` (`base.py:55`):
/// `FileAsset.objects.get(asset=asset_key)` — default-manager scope.
/// A missing row raises `DoesNotExist`, mapped to 404
/// [`not_found_body`] by `handle_exception` (`app/views/base.py:234-238`).
/// Params: `$1` = composed asset key.
pub fn workspace_get_sql() -> String {
    format!(
        "SELECT * FROM \"{table}\" WHERE \"asset\" = $1 AND \"deleted_at\" IS NULL LIMIT 1",
        table = columns::TABLE,
    )
}

/// Multi-row read for `UserAssetsEndpoint.get` (`base.py:65`):
/// `FileAsset.objects.filter(asset=asset_key, created_by=request.user)` —
/// raw key, creator scoping, default-manager scope, model ordering.
/// Params: `$1` = raw asset key, `$2` = requesting user id.
pub fn user_filter_sql() -> String {
    format!(
        "SELECT * FROM \"{table}\" WHERE \"asset\" = $1 AND \"created_by_id\" = $2 \
         AND \"deleted_at\" IS NULL ORDER BY \"created_at\" DESC",
        table = columns::TABLE,
    )
}

/// Single-row read for `UserAssetsEndpoint.delete` (`base.py:83`):
/// `FileAsset.objects.get(asset=asset_key, created_by=request.user)`.
/// Creator scoping makes another user's row invisible (404).
/// Params: `$1` = raw asset key, `$2` = requesting user id.
pub fn user_get_sql() -> String {
    format!(
        "SELECT * FROM \"{table}\" WHERE \"asset\" = $1 AND \"created_by_id\" = $2 \
         AND \"deleted_at\" IS NULL LIMIT 1",
        table = columns::TABLE,
    )
}

/// Workspace id lookup for `FileAssetEndpoint.post` (`base.py:39`):
/// `Workspace.objects.get(slug=slug)`. A missing slug raises
/// `DoesNotExist`, mapped to 404 [`not_found_body`]. The resolved id is
/// passed as `serializer.save(workspace_id=...)` (`base.py:40`) — it is an
/// extra kwarg, never request data. Params: `$1` = workspace slug.
pub fn workspace_lookup_sql() -> String {
    "SELECT \"id\" FROM \"workspaces\" WHERE \"slug\" = $1 LIMIT 1".to_owned()
}

/// The `is_deleted` flip shared by delete (`base.py:47-48`), restore
/// (`base.py:56-57`) and user delete (`base.py:84-85`):
/// `save(update_fields=["is_deleted"])` over the instance pk.
/// Params: `$1` = new `is_deleted` value, `$2` = row id.
///
/// BUG PORT (`base.py:47-48`): only `is_deleted` is named — `deleted_at`
/// is untouched, so the row stays visible to the default manager. The
/// statement must NOT mention `deleted_at` (asserted in tests).
pub fn set_deleted_sql() -> String {
    format!(
        "UPDATE \"{table}\" SET \"is_deleted\" = $1 WHERE \"id\" = $2",
        table = columns::TABLE,
    )
}

/// 200 with the found envelope `{"data": [...], "status": true}`
/// (`base.py:26-28`). `data` holds `FileAssetSerializer` rows
/// (PIDASHCONV-319).
pub const FOUND_STATUS: u16 = 200;

/// 200 with the miss body — the miss is signalled ONLY by
/// `{"status": false}` (`base.py:30-33,70-73`). Port the 200-on-miss
/// exactly; never 404 here.
pub const MISS_STATUS: u16 = 200;

/// 201 with `serializer.data` (workspace post `base.py:41`,
/// user post `base.py:79`).
pub const POST_STATUS: u16 = 201;

/// 204, empty body (workspace delete `base.py:49`, restore `base.py:58`,
/// user delete `base.py:86`).
pub const DELETE_STATUS: u16 = 204;
/// 204, empty body (restore `base.py:58`).
pub const RESTORE_STATUS: u16 = 204;

/// 404 `{"error": "The required object does not exist."}` — unknown asset
/// key on delete/restore/user-delete and unknown workspace slug on post,
/// via `ObjectDoesNotExist -> handle_exception`
/// (`app/views/base.py:234-238`).
pub const NOT_FOUND_STATUS: u16 = 404;

/// Ported bug (`base.py:67`): the user-get exists-branch ALWAYS answers
/// 500 because the queryset is serialized without `many=True`. Handlers
/// must return [`unhandled_body`] with this status when the user filter
/// matches any row — never the row.
pub const USER_GET_FOUND_STATUS: u16 = 500;

/// Miss body shared by both GET endpoints (`base.py:30-33,70-73`).
pub fn miss_body() -> Value {
    json!({"error": "Asset key does not exist", "status": false})
}

/// Unknown-key / unknown-workspace body (`app/views/base.py:234-238`).
pub fn not_found_body() -> Value {
    json!({"error": "The required object does not exist."})
}

/// Unhandled-exception body (`app/views/base.py:249-254`): the user-get
/// serializer bug (`base.py:67`) and the real-file-upload storage failure
/// on both posts land here with a 500. Storage internals belong to the
/// handlers/storage layer; this module records the path, not the cause.
pub fn unhandled_body() -> Value {
    json!({"error": "Something went wrong please try again later"})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/app_assets/queries/v1.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn models_columns_fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_assets/models/fileasset.columns.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("columns golden exists"))
            .expect("columns golden parses")
    }

    #[test]
    fn table_matches_models_layer_and_fixture() {
        assert_eq!(columns::TABLE, "file_assets");
        let fixture = models_columns_fixture();
        assert_eq!(
            fixture
                .get("db_table")
                .expect("db_table")
                .as_str()
                .expect("str"),
            columns::TABLE,
        );
        for sql in [
            workspace_filter_sql(),
            workspace_get_sql(),
            user_filter_sql(),
            user_get_sql(),
            set_deleted_sql(),
        ] {
            assert!(sql.contains("\"file_assets\""), "table in {sql}");
        }
    }

    #[test]
    fn workspace_key_composition_matches_python() {
        assert_eq!(
            workspace_asset_key("550e8400-e29b-41d4-a716-446655440000", "f.txt"),
            "550e8400-e29b-41d4-a716-446655440000/f.txt",
        );
        // str(workspace_id): numeric ids stringify without padding.
        assert_eq!(workspace_asset_key("7", "a/b.png"), "7/a/b.png");
    }

    #[test]
    fn workspace_filter_sql_matches_golden_where() {
        let golden = golden();
        let recorded = golden["endpoint_FileAssetEndpoint"]["get"]["query_sql"]
            .as_str()
            .expect("query_sql");
        // Golden records Django `%s` placeholders; builders emit `$n`.
        assert!(recorded.contains("asset = %s"), "golden pins asset filter");
        assert!(recorded.contains("deleted_at IS NULL"), "golden pins scope");
        let sql = workspace_filter_sql();
        assert_eq!(
            sql,
            "SELECT * FROM \"file_assets\" WHERE \"asset\" = $1 AND \"deleted_at\" IS NULL \
             ORDER BY \"created_at\" DESC",
        );
    }

    #[test]
    fn workspace_get_sql_is_scoped_single_row() {
        let sql = workspace_get_sql();
        assert!(sql.contains("\"asset\" = $1"), "asset predicate");
        assert!(
            sql.contains("\"deleted_at\" IS NULL"),
            "default-manager scope"
        );
        assert!(sql.ends_with("LIMIT 1"), "single-row get");
    }

    #[test]
    fn user_sqls_scope_by_creator_and_raw_key() {
        for sql in [user_filter_sql(), user_get_sql()] {
            assert!(sql.contains("\"asset\" = $1"), "raw asset key first");
            assert!(sql.contains("\"created_by_id\" = $2"), "creator scoping");
            assert!(
                sql.contains("\"deleted_at\" IS NULL"),
                "default-manager scope"
            );
        }
        assert!(
            user_filter_sql().contains("ORDER BY \"created_at\" DESC"),
            "model ordering"
        );
        assert!(user_get_sql().ends_with("LIMIT 1"), "single-row get");
        let golden = golden();
        let recorded = golden["endpoint_UserAssetsEndpoint"]["get"]["query_sql"]
            .as_str()
            .expect("query_sql");
        assert!(
            recorded.contains("created_by_id = %s"),
            "golden pins creator scope"
        );
    }

    #[test]
    fn workspace_lookup_targets_slug() {
        assert_eq!(
            workspace_lookup_sql(),
            "SELECT \"id\" FROM \"workspaces\" WHERE \"slug\" = $1 LIMIT 1",
        );
    }

    #[test]
    fn set_deleted_ports_is_deleted_only_bug() {
        assert_eq!(
            set_deleted_sql(),
            "UPDATE \"file_assets\" SET \"is_deleted\" = $1 WHERE \"id\" = $2",
        );
        // BUG (base.py:47-48): the flip names ONLY is_deleted — deleted_at
        // stays NULL so the row remains visible to reads. Port exactly.
        assert!(
            !set_deleted_sql().contains("deleted_at"),
            "no deleted_at write"
        );
        assert!(
            !set_deleted_sql().contains("updated_at"),
            "update_fields excludes auto_now"
        );
        let golden = golden();
        assert!(
            golden["endpoint_FileAssetEndpoint"]["delete"]["v1_delete_quirk"]
                .as_str()
                .expect("quirk recorded")
                .contains("sets ONLY is_deleted")
        );
    }

    #[test]
    fn miss_and_found_contract_match_golden() {
        let golden = golden();
        assert_eq!(
            MISS_STATUS,
            golden["endpoint_FileAssetEndpoint"]["get"]["empty_status"]
        );
        assert_eq!(
            miss_body(),
            golden["endpoint_FileAssetEndpoint"]["get"]["empty_body"]
        );
        assert_eq!(
            miss_body(),
            golden["endpoint_UserAssetsEndpoint"]["get"]["empty_body"]
        );
        assert_eq!(
            FOUND_STATUS,
            golden["endpoint_FileAssetEndpoint"]["get"]["found_status"]
        );
        let found_keys: std::collections::BTreeSet<&str> = golden["endpoint_FileAssetEndpoint"]
            ["get"]["found_body"]
            .as_object()
            .expect("found body")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            found_keys,
            std::collections::BTreeSet::from(["data", "status"])
        );
        assert_eq!(FOUND_STATUS, 200);
        assert_eq!(MISS_STATUS, 200, "both branches return 200");
    }

    #[test]
    fn write_outcomes_match_golden() {
        let golden = golden();
        assert_eq!(POST_STATUS, 201);
        assert_eq!(DELETE_STATUS, 204);
        assert_eq!(RESTORE_STATUS, 204);
        assert_eq!(
            DELETE_STATUS,
            golden["endpoint_FileAssetEndpoint"]["delete"]["response"]["status"],
        );
        assert_eq!(
            RESTORE_STATUS,
            golden["endpoint_FileAssetViewSet_restore"]["response"]["status"],
        );
        assert_eq!(NOT_FOUND_STATUS, 404);
        assert_eq!(
            not_found_body(),
            golden["endpoint_FileAssetEndpoint"]["delete"]["errors"]["unknown_key"]["body"],
        );
    }

    #[test]
    fn user_get_found_is_500_bug_match_golden() {
        // BUG (base.py:67): missing many=True — any existing user row 500s.
        let golden = golden();
        assert_eq!(USER_GET_FOUND_STATUS, 500);
        assert_eq!(
            USER_GET_FOUND_STATUS,
            golden["endpoint_UserAssetsEndpoint"]["get"]["found_status"],
        );
        assert_eq!(
            unhandled_body(),
            golden["error_envelopes"]["unhandled"]["body"]
        );
        assert!(
            golden["endpoint_UserAssetsEndpoint"]["get"]["serializer_bug_500"]
                .as_str()
                .expect("bug recorded")
                .contains("many=True")
        );
    }

    #[test]
    fn golden_covers_all_seven_methods() {
        let golden = golden();
        for pointer in [
            "/endpoint_FileAssetEndpoint/get",
            "/endpoint_FileAssetEndpoint/post",
            "/endpoint_FileAssetEndpoint/delete",
            "/endpoint_FileAssetViewSet_restore",
            "/endpoint_UserAssetsEndpoint/get",
            "/endpoint_UserAssetsEndpoint/post",
            "/endpoint_UserAssetsEndpoint/delete",
        ] {
            assert!(golden.pointer(pointer).is_some(), "golden covers {pointer}");
        }
        assert!(golden["_trace"]
            .as_str()
            .expect("trace")
            .contains("base.py:1-86"));
    }
}
