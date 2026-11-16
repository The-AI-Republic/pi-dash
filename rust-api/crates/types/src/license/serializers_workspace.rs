//! Workspace serializers for the license / instance-console domain (D-01).
//!
//! Ports `apps/api/pi_dash/license/api/serializers/user.py:9-11`
//! (`UserLiteSerializer`) and
//! `apps/api/pi_dash/license/api/serializers/workspace.py:15-41`
//! (`WorkspaceSerializer`) as plain serde shapes plus the pure
//! [`validate_slug`] rule.
//!
//! Layering notes:
//!
//! * `Workspace`/`User` rows are owned by other domains; this module only
//!   shapes them for JSON output and performs no schema work.
//! * UUIDs and datetimes cross this boundary as pre-rendered strings in the
//!   exact DRF format (UUID lowercase hyphenated; datetimes ISO-8601 with a
//!   `Z` suffix, microseconds omitted when zero and six digits otherwise, per
//!   DRF 3.x `DateTimeField`). The rendering itself is DRF machinery owned by
//!   the foundation/api layers, so this crate takes no `chrono`/`uuid`
//!   dependency.
//! * `validate_slug` takes the `slug__iexact` existence bit as an argument:
//!   this crate performs no I/O, so the caller (which owns the query in
//!   `api/views/workspace.py` / `serializers/workspace.py:26`) supplies it.
//! * Field declaration order is the DRF output order, verified against the
//!   live serializer: declared fields first
//!   (`id`, `owner`, `logo_url`, `total_projects`, `total_members`), then
//!   model `_meta` order.
//! * `total_projects`/`total_members` are `read_only` annotation fields fed
//!   by the GET query (`api/views/workspace.py:41-56`). A missing annotation
//!   is absent from the output (DRF `SkipField`); an explicit `None` renders
//!   `null`. `Option<Option<i64>>` models that triple state: `None` is
//!   skipped, `Some(None)` is `null`, `Some(Some(n))` is `n`.
//!
//! Ported quirks (kept as-is, translate don't redesign):
//!
//! * The restricted-slug check (`value in RESTRICTED_WORKSPACE_SLUGS`) is a
//!   case-sensitive exact match, while the existence check is `iexact`, so
//!   e.g. `"API"` passes the first check but still fails when a
//!   case-insensitive row exists.
//! * The restricted list carries duplicate entries (`config`, `mobile`,
//!   `monitor` appear twice in `pi_dash/utils/constants.py:5-70`); they are
//!   kept verbatim since membership is all that matters.
//! * `validate_slug` does not exclude the instance being updated, so
//!   re-saving an unchanged slug fails the `iexact` check. No update path
//!   uses this serializer today (`api/views/workspace.py` has GET/POST
//!   only).
//! * On create, an exact-duplicate slug is rejected by DRF's model
//!   `UniqueValidator` (default `"workspace with this slug already exists."`
//!   message) before `validate_slug` runs; the `"Slug is already in use"`
//!   branch is reached only for case-variants of an existing slug.
//!
//! Wiring note: the crate root declares `pub mod license;` and
//! `license/mod.rs` re-exports this module (foundation change, tracked
//! separately); this file is new-files-only for this issue.

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

/// `UserLiteSerializer` (`serializers/user.py:9-11`).
///
/// `id` renders via `PrimaryKeyRelatedField` (UUID as string); `email` is
/// nullable (`null` renders present); `first_name`/`last_name` are blankable
/// strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserLite {
    pub id: String,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
}

/// `WorkspaceSerializer` (`serializers/workspace.py:15-41`, `Meta.fields =
/// "__all__"`).
///
/// `owner` nests [`UserLite`] (`read_only`); `logo_url` is the model property
/// (`db/models/workspace.py:145-154`: `logo_asset.asset_url` if set, else
/// `logo`, else `None`) and renders present even when `null`;
/// `total_projects`/`total_members` are annotation-fed (see module docs).
/// Read-only model fields (`id`, `created_by`, `updated_by`, `created_at`,
/// `updated_at`, `owner`, `logo_url`) are output only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub owner: UserLite,
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total_projects: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total_members: Option<Option<i64>>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub name: String,
    pub logo: Option<String>,
    pub slug: String,
    pub organization_size: Option<String>,
    pub timezone: String,
    pub background_color: String,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub logo_asset: Option<String>,
}

/// `RESTRICTED_WORKSPACE_SLUGS` (`pi_dash/utils/constants.py:5-70`), verbatim
/// including duplicates and source order.
pub const RESTRICTED_WORKSPACE_SLUGS: &[&str] = &[
    "404",
    "accounts",
    "api",
    "create-workspace",
    "god-mode",
    "installations",
    "invitations",
    "onboarding",
    "profile",
    "spaces",
    "workspace-invitations",
    "password",
    "flags",
    "monitor",
    "monitoring",
    "ingest",
    "pi-dash-pro",
    "pi-dash-ultimate",
    "enterprise",
    "pi-dash-enterprise",
    "disco",
    "silo",
    "chat",
    "calendar",
    "drive",
    "channels",
    "upgrade",
    "billing",
    "sign-in",
    "sign-up",
    "signin",
    "signup",
    "config",
    "live",
    "admin",
    "m",
    "import",
    "importers",
    "integrations",
    "integration",
    "configuration",
    "initiatives",
    "initiative",
    "config",
    "workflow",
    "workflows",
    "epics",
    "epic",
    "story",
    "mobile",
    "dashboard",
    "desktop",
    "onload",
    "real-time",
    "one",
    "pages",
    "mobile",
    "business",
    "pro",
    "settings",
    "monitor",
    "license",
    "licenses",
    "instances",
    "instance",
];

/// Slug rejection reasons (`serializers/workspace.py:21-28`).
///
/// `Display` renders the exact DRF `ValidationError` message, which DRF
/// envelopes as `{"slug": ["<message>"]}` in `serializer.errors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
pub enum SlugError {
    /// Slug is on the restricted list (`workspace.py:23-24`).
    #[error("Slug is not valid")]
    Restricted,
    /// A `slug__iexact` row exists (`workspace.py:26-27`).
    #[error("Slug is already in use")]
    Taken,
}

/// `WorkspaceSerializer.validate_slug` (`serializers/workspace.py:21-28`).
///
/// `slug_taken_iexact` is the result of
/// `Workspace.objects.filter(slug__iexact=value).exists()`, evaluated by the
/// caller. The restricted-list check runs first, exactly as in Python.
pub fn validate_slug(value: &str, slug_taken_iexact: bool) -> Result<&str, SlugError> {
    if RESTRICTED_WORKSPACE_SLUGS.contains(&value) {
        return Err(SlugError::Restricted);
    }
    if slug_taken_iexact {
        return Err(SlugError::Taken);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> UserLite {
        UserLite {
            id: "11111111-1111-1111-1111-111111111111".to_owned(),
            email: Some("admin@acme.test".to_owned()),
            first_name: "Ada".to_owned(),
            last_name: "Lovelace".to_owned(),
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            id: "55555555-5555-5555-5555-555555555555".to_owned(),
            owner: owner(),
            logo_url: Some("https://cdn.test/logo.png".to_owned()),
            total_projects: Some(Some(3)),
            total_members: Some(Some(7)),
            created_at: "2026-01-15T12:30:45Z".to_owned(),
            updated_at: "2026-01-15T12:30:45Z".to_owned(),
            deleted_at: None,
            name: "Acme Works".to_owned(),
            logo: Some("https://cdn.test/logo.png".to_owned()),
            slug: "acme-works".to_owned(),
            organization_size: Some("10-50".to_owned()),
            timezone: "UTC".to_owned(),
            background_color: "#FF0000".to_owned(),
            created_by: None,
            updated_by: None,
            logo_asset: None,
        }
    }

    /// Exact DRF bytes for the fixture row, captured from the live
    /// `WorkspaceSerializer` via DRF's `JSONRenderer` (field order: declared
    /// fields first, then model `_meta` order).
    const DRF_BYTES: &str = r##"{"id":"55555555-5555-5555-5555-555555555555","owner":{"id":"11111111-1111-1111-1111-111111111111","email":"admin@acme.test","first_name":"Ada","last_name":"Lovelace"},"logo_url":"https://cdn.test/logo.png","total_projects":3,"total_members":7,"created_at":"2026-01-15T12:30:45Z","updated_at":"2026-01-15T12:30:45Z","deleted_at":null,"name":"Acme Works","logo":"https://cdn.test/logo.png","slug":"acme-works","organization_size":"10-50","timezone":"UTC","background_color":"#FF0000","created_by":null,"updated_by":null,"logo_asset":null}"##;

    #[test]
    fn workspace_serializes_byte_identical_to_drf() {
        assert_eq!(
            serde_json::to_string(&workspace()).expect("serializes"),
            DRF_BYTES
        );
    }

    #[test]
    fn workspace_replays_golden_output_value() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/license/serializers/workspace.golden.json"
        ))
        .expect("golden parses");
        let output = golden.get("output").expect("golden has output");
        let replayed = serde_json::to_value(workspace()).expect("serializes");
        assert_eq!(&replayed, output);
    }

    #[test]
    fn user_lite_field_order_matches_meta_fields() {
        let rendered = serde_json::to_string(&owner()).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"id":"11111111-1111-1111-1111-111111111111","email":"admin@acme.test","first_name":"Ada","last_name":"Lovelace"}"#
        );
    }

    #[test]
    fn user_lite_email_none_renders_null() {
        let mut lite = owner();
        lite.email = None;
        let value = serde_json::to_value(lite).expect("serializes");
        assert_eq!(value.get("email"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn missing_annotations_are_absent() {
        let mut ws = workspace();
        ws.total_projects = None;
        ws.total_members = None;
        let value = serde_json::to_value(ws).expect("serializes");
        let obj = value.as_object().expect("object");
        assert!(!obj.contains_key("total_projects"));
        assert!(!obj.contains_key("total_members"));
    }

    #[test]
    fn null_annotations_render_null() {
        let mut ws = workspace();
        ws.total_projects = Some(None);
        ws.total_members = Some(None);
        let value = serde_json::to_value(ws).expect("serializes");
        assert_eq!(value.get("total_projects"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("total_members"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn logo_url_none_renders_null_present() {
        let mut ws = workspace();
        ws.logo = None;
        ws.logo_url = None;
        let value = serde_json::to_value(ws).expect("serializes");
        assert_eq!(value.get("logo_url"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("logo"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn restricted_slugs_rejected_with_exact_message() {
        for slug in [
            "404",
            "api",
            "license",
            "instances",
            "create-workspace",
            "config",
        ] {
            assert_eq!(validate_slug(slug, false), Err(SlugError::Restricted));
        }
        assert_eq!(SlugError::Restricted.to_string(), "Slug is not valid");
    }

    #[test]
    fn taken_slug_rejected_with_exact_message() {
        assert_eq!(validate_slug("acme-works", true), Err(SlugError::Taken));
        assert_eq!(SlugError::Taken.to_string(), "Slug is already in use");
    }

    #[test]
    fn fresh_slug_accepted() {
        assert_eq!(validate_slug("acme-works", false), Ok("acme-works"));
    }

    #[test]
    fn restricted_check_is_case_sensitive_like_python() {
        // Python `value in RESTRICTED_WORKSPACE_SLUGS` is an exact match;
        // "API" passes here and only the iexact DB check can reject it.
        assert_eq!(validate_slug("API", false), Ok("API"));
        assert_eq!(validate_slug("API", true), Err(SlugError::Taken));
    }

    #[test]
    fn restricted_list_matches_python_source() {
        assert_eq!(RESTRICTED_WORKSPACE_SLUGS.len(), 65);
        assert!(RESTRICTED_WORKSPACE_SLUGS.contains(&"god-mode"));
        assert!(RESTRICTED_WORKSPACE_SLUGS.contains(&"workspace-invitations"));
        assert!(!RESTRICTED_WORKSPACE_SLUGS.contains(&"acme-works"));
    }
}
