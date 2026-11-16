//! File-asset URL kernel + `FileAssetSerializer` read shape (D-21 models,
//! PIDASHCONV-404).
//!
//! Ports `apps/api/pi_dash/db/models/asset.py:79-100` (the `asset_url`
//! property) and `apps/api/pi_dash/api/serializers/asset.py:93-123`
//! (`FileAssetSerializer`: `fields = "__all__"`, the exact read-only
//! list, `asset_url` read-only char).
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-model-fileasset.json`
//! (`fx-model-fileasset`): the `#[cfg(test)]` suite replays the
//! `asset_url` goldens and the read-shape list.
//!
//! The branching mirrors `pidash-db` `app_assets::asset_url` exactly
//! (same table, app-plane port, D-31): the four static logo/cover types
//! render `/api/assets/v2/static/<id>/`; `ISSUE_ATTACHMENT` renders the
//! workspace/project/issue URL; the four description types render the
//! workspace/project URL; anything else — including `None` and
//! `DRAFT_ISSUE_ATTACHMENT` — renders `None`. The slug/ids arrive
//! already resolved (`&str`): resolving them, and the missing-workspace
//! `AttributeError` (a 500 on the wire, `asset.py:90,98`), belongs to
//! the queries layer (PIDASHCONV-409).
//!
//! Ported bugs (translate, don't redesign; listed for the PR):
//!
//! * `DRAFT_ISSUE_ATTACHMENT` has no branch (`asset.py:79-100`
//!   enumerates 9 of the 10 `EntityTypeContext` values) → `None`.
//! * The serializer's `asset_url = CharField(read_only=True)` renders
//!   this property; `None` renders JSON `null`.

/// Entity types served from the static endpoint (`asset_url` first
/// branch, `db/models/asset.py:81-87`).
pub fn is_static_asset_type(entity_type: &str) -> bool {
    matches!(
        entity_type,
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER"
    )
}

/// Entity types rendering the description URL (`asset.py:92-98`).
pub fn is_description_asset_type(entity_type: &str) -> bool {
    matches!(
        entity_type,
        "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION"
    )
}

/// Mirrors the `asset_url` property (`db/models/asset.py:79-100`).
///
/// `entity_type` is `None` for null/unset rows and falls through to
/// `None` (`:100`), as does any unrecognized value — including
/// `DRAFT_ISSUE_ATTACHMENT`. `workspace_slug`, `project_id` and
/// `issue_id` are the already-resolved values the f-strings interpolate
/// (`:87,:90,:98`); see the module-level note on the missing-workspace
/// 500.
pub fn asset_url(
    entity_type: Option<&str>,
    id: &str,
    workspace_slug: &str,
    project_id: &str,
    issue_id: &str,
) -> Option<String> {
    match entity_type {
        Some(t) if is_static_asset_type(t) => Some(format!("/api/assets/v2/static/{id}/")),
        Some("ISSUE_ATTACHMENT") => Some(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/issues/{issue_id}/attachments/{id}/"
        )),
        Some(t) if is_description_asset_type(t) => Some(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/{id}/"
        )),
        _ => None,
    }
}

/// `FileAssetSerializer.Meta.fields` (`serializers/asset.py:97`):
/// every model field plus the `asset_url` property.
pub const FIELDS: &str = "__all__";

/// `FileAssetSerializer.Meta.read_only_fields`
/// (`serializers/asset.py:98-115`) in declaration order.
pub const READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "workspace",
    "project",
    "issue",
    "comment",
    "page",
    "draft_issue",
    "user",
    "is_deleted",
    "deleted_at",
    "storage_metadata",
    "asset_url",
];

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE: &str = include_str!("../../../../fixtures/v1_assets/fx-model-fileasset.json");

    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    #[test]
    fn asset_url_goldens() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let goldens = fixture["asset_url"]["goldens"]
            .as_array()
            .expect("asset_url goldens");
        assert!(!goldens.is_empty());
        // The goldens cover all 10 entity types plus null: the 9
        // branched values plus the unbranched DRAFT_ISSUE_ATTACHMENT.
        let mut seen: Vec<String> = Vec::new();
        for (i, golden) in goldens.iter().enumerate() {
            let entity_type = golden["entity_type"].as_str();
            if let Some(t) = entity_type {
                seen.push(t.to_owned());
            }
            let input = &golden["in"];
            let id = input["id"].as_str().unwrap_or("");
            let workspace_slug = input["workspace_slug"].as_str().unwrap_or("");
            let project_id = input["project_id"].as_str().unwrap_or("");
            let issue_id = input["issue_id"].as_str().unwrap_or("");
            let expected = golden["out"].as_str().map(str::to_owned);
            assert_eq!(
                asset_url(entity_type, id, workspace_slug, project_id, issue_id),
                expected,
                "asset_url golden {i}"
            );
        }
        seen.sort();
        assert_eq!(
            seen,
            [
                "COMMENT_DESCRIPTION",
                "DRAFT_ISSUE_ATTACHMENT",
                "DRAFT_ISSUE_DESCRIPTION",
                "ISSUE_ATTACHMENT",
                "ISSUE_DESCRIPTION",
                "PAGE_DESCRIPTION",
                "PROJECT_COVER",
                "USER_AVATAR",
                "USER_COVER",
                "WORKSPACE_LOGO",
            ]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn read_shape_matches_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let shape = &fixture["read_shape"];
        assert_eq!(shape["fields"].as_str().unwrap(), FIELDS);
        let expected: Vec<String> = shape["read_only_fields"]
            .as_array()
            .expect("read_only_fields must be an array")
            .iter()
            .map(|v| v.as_str().expect("field name").to_owned())
            .collect();
        assert_eq!(owned(READ_ONLY_FIELDS), expected);
        assert!(READ_ONLY_FIELDS.contains(&"asset_url"));
    }

    #[test]
    fn branch_predicates_partition_entity_types() {
        // Every static/description grouping the kernel relies on.
        for t in [
            "WORKSPACE_LOGO",
            "USER_AVATAR",
            "USER_COVER",
            "PROJECT_COVER",
        ] {
            assert!(is_static_asset_type(t), "{t}");
            assert!(!is_description_asset_type(t), "{t}");
        }
        for t in [
            "ISSUE_DESCRIPTION",
            "COMMENT_DESCRIPTION",
            "PAGE_DESCRIPTION",
            "DRAFT_ISSUE_DESCRIPTION",
        ] {
            assert!(!is_static_asset_type(t), "{t}");
            assert!(is_description_asset_type(t), "{t}");
        }
        for t in ["ISSUE_ATTACHMENT", "DRAFT_ISSUE_ATTACHMENT", "NOPE"] {
            assert!(!is_static_asset_type(t), "{t}");
            assert!(!is_description_asset_type(t), "{t}");
        }
        // The fall-through renders null.
        assert_eq!(
            asset_url(Some("DRAFT_ISSUE_ATTACHMENT"), "id", "w", "p", "i"),
            None
        );
        assert_eq!(asset_url(None, "id", "w", "p", "i"), None);
        assert_eq!(asset_url(Some("NOPE"), "id", "w", "p", "i"), None);
    }
}
