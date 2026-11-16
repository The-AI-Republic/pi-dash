//! `FileAsset` columns + helpers (D-31, stage 5).
//!
//! Translation of `apps/api/pi_dash/db/models/asset.py` (100 LOC) into a
//! read-only reference for query builders. Django stays schema owner until
//! switchover: no migrations, no DDL — the consts below describe the live
//! `file_assets` table; they create nothing.
//!
//! Fixture source of truth: `rust-api/fixtures/app_assets/models/` recorded
//! by PIDASHCONV-306 (traced in `rust-api/fixtures/app_assets/TRACE.md`);
//! the `#[cfg(test)]` suite asserts these consts and helpers equal the
//! fixture column lists, URL shapes and key shapes.
//!
//! # Ported bugs (translated, not fixed)
//!
//! - BUG (`asset.py:46`): the `asset` field carries no `validators=`.
//!   Migration `0075_alter_fileasset_asset.py` lists
//!   `FileExtensionValidator(jpg,jpeg,png)` + `file_size`, but the model
//!   dropped them with no later `AlterField`; runtime enforces neither.
//!   There is deliberately no validators const here.
//! - BUG (`asset.py:25`, limit at `settings/common.py:428`): the
//!   `file_size` message hardcodes "5 MB" while `FILE_SIZE_LIMIT` is
//!   env-overridable (`int(get_config("FILE_SIZE_LIMIT", 5242880))`).
//!   [`FILE_SIZE_MESSAGE`] ports the literal.
//!
//! # Notes for query builders
//!
//! - `entity_type` passes no `choices=` (`asset.py:54`; the
//!   `EntityTypeContext` exists at `:33-43` but values are enforced only in
//!   view code via `in FileAsset.EntityTypeContext.values`). Port as an
//!   unconstrained varchar; never assume a DB constraint.
//! - `__str__` is `str(self.asset)` (`asset.py:76-77`) — the storage key,
//!   not the id. `Display` formatting of rows belongs to the queries
//!   layer, which holds the row.
//! - `get_upload_path`'s workspace branch reads `instance.workspace.id`
//!   (`asset.py:19`) — the related *object*, not `workspace_id`, so with
//!   only `workspace_id` set Django issues one extra `SELECT` on
//!   `workspaces`. The key prefix is the workspace id either way;
//!   [`upload_path_key`] takes the id the caller already holds and builds
//!   the same key shape without the extra read.
//! - `asset_url`'s non-static branches dereference `self.workspace.slug`
//!   (`asset.py:90,98`): with `workspace` unset Python raises
//!   `AttributeError` (a 500), it does not return `None`. [`asset_url`]
//!   therefore takes the already-resolved slug/project/issue ids as
//!   `&str`; resolving them (and the 500-on-missing behavior) belongs to
//!   the queries layer.
//! - `objects` is the inherited `SoftDeletionManager` (`db/mixins.py:66`)
//!   and `all_objects` the plain manager (`:67`); every read built from
//!   these columns must apply
//!   [`crate::soft_delete::active_condition`], except the restore path,
//!   which reads through `all_objects`.

/// Physical table (`Meta.db_table`, `asset.py:67`).
pub const TABLE: &str = "file_assets";

/// Default ordering (`Meta.ordering`, `asset.py:68`).
pub const ORDERING: &[&str] = &["-created_at"];

/// A `Meta.indexes` entry: the index name plus its column list
/// (`asset.py:69-74`).
pub struct Index {
    /// Index name as created by Django.
    pub name: &'static str,
    /// Indexed columns in order.
    pub columns: &'static [&'static str],
}

/// `Meta.indexes` (`asset.py:69-74`) in declaration order.
pub const INDEXES: &[Index] = &[
    Index {
        name: "asset_entity_type_idx",
        columns: &["entity_type"],
    },
    Index {
        name: "asset_entity_identifier_idx",
        columns: &["entity_identifier"],
    },
    Index {
        name: "asset_entity_idx",
        columns: &["entity_type", "entity_identifier"],
    },
    Index {
        name: "asset_asset_idx",
        columns: &["asset"],
    },
];

/// Columns in Django field-definition order: `BaseModel.id`
/// (`db/models/base.py:17-18`), audit cols (`db/mixins.py:16-42,61-64`),
/// then `asset.py:45-62`. FK entries use the Django attnames
/// (`user_id`, `workspace_id`, …).
pub const COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "attributes",
    "asset",
    "user_id",
    "workspace_id",
    "draft_issue_id",
    "project_id",
    "issue_id",
    "comment_id",
    "page_id",
    "entity_type",
    "entity_identifier",
    "is_deleted",
    "is_archived",
    "external_id",
    "external_source",
    "size",
    "is_uploaded",
    "storage_metadata",
];

/// Django field defaults in [`COLUMNS`] order (`None` = no default).
/// `attributes`/`storage_metadata` default to `dict` (`:45,:62`), the
/// booleans to `false` (`:56,:57,:61`), `size` to `0` (`:60`); `id` is a
/// `uuid4` primary key (`base.py:18`).
pub const DEFAULTS: &[Option<&str>] = &[
    None,          // id (uuid4 pk)
    None,          // created_at (auto_now_add)
    None,          // updated_at (auto_now)
    None,          // created_by_id (SET_NULL)
    None,          // updated_by_id (SET_NULL)
    None,          // deleted_at
    Some("dict"),  // attributes (JSONField)
    None,          // asset (FileField, max_length=800)
    None,          // user_id (CASCADE)
    None,          // workspace_id (CASCADE)
    None,          // draft_issue_id (CASCADE)
    None,          // project_id (CASCADE)
    None,          // issue_id (CASCADE)
    None,          // comment_id (CASCADE)
    None,          // page_id (CASCADE)
    None,          // entity_type (varchar(255) null blank, no choices=)
    None,          // entity_identifier (varchar(255) null blank)
    Some("false"), // is_deleted
    Some("false"), // is_archived
    None,          // external_id (varchar(255) null blank)
    None,          // external_source (varchar(255) null blank)
    Some("0"),     // size (FloatField)
    Some("false"), // is_uploaded
    Some("dict"),  // storage_metadata (JSONField null blank)
];

/// All `EntityTypeContext` values (`asset.py:33-43`) in source definition
/// order.
pub const ENTITY_TYPES: &[&str] = &[
    "ISSUE_ATTACHMENT",
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "USER_COVER",
    "USER_AVATAR",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
    "DRAFT_ISSUE_ATTACHMENT",
    "DRAFT_ISSUE_DESCRIPTION",
];

/// Default `FILE_SIZE_LIMIT` (`settings/common.py:428`):
/// `int(get_config("FILE_SIZE_LIMIT", 5242880))`.
pub const FILE_SIZE_LIMIT_DEFAULT: u64 = 5_242_880;

/// `file_size` rejection message (`asset.py:25`). Ports the literal,
/// including the hardcoded "5 MB" (see the module-level BUG note).
pub const FILE_SIZE_MESSAGE: &str = "File too large. Size should not exceed 5 MB.";

/// Builds the storage key for `FileAsset.asset`
/// (`get_upload_path`, `asset.py:17-20`).
///
/// `workspace_id` mirrors the `instance.workspace_id is not None` check
/// (`:18`): `Some` prefixes the workspace id (`:19`), `None` mints the
/// user key with no user id (`user-{hex}-{filename}`, `:20`).
/// `uuid_hex` is the caller-supplied `uuid4().hex` (32 lowercase hex);
/// uuid generation is a side effect and lives at the call site.
pub fn upload_path_key(workspace_id: Option<&str>, uuid_hex: &str, filename: &str) -> String {
    match workspace_id {
        Some(id) => format!("{id}/{uuid_hex}-{filename}"),
        None => format!("user-{uuid_hex}-{filename}"),
    }
}

/// Mirrors `file_size` (`asset.py:23-25`): errors when
/// `value.size > limit` (strictly greater — equal passes).
pub fn validate_file_size(size: u64, limit: u64) -> Result<(), &'static str> {
    if size > limit {
        Err(FILE_SIZE_MESSAGE)
    } else {
        Ok(())
    }
}

/// Entity types served from the static endpoint (`asset_url` first branch,
/// `asset.py:81-87`).
pub fn is_static_asset_type(entity_type: &str) -> bool {
    matches!(
        entity_type,
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER"
    )
}

/// Mirrors the `asset_url` property (`asset.py:79-100`).
///
/// `entity_type` is `None` for null/unset rows and falls through to `None`
/// (`:100`), as does any unrecognized value — including
/// `DRAFT_ISSUE_ATTACHMENT`. `workspace_slug`, `project_id` and `issue_id`
/// are the already-resolved values the f-strings interpolate (`:87,:90,:98`);
/// see the module-level note on the missing-workspace 500.
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
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => Some(format!(
            "/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/{id}/"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_assets/models")
            .join(name);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    fn strings(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("must be a string array")
            .iter()
            .map(|v| v.as_str().expect("must be a string array").to_owned())
            .collect()
    }

    /// `fields[].column` in order (the object-shaped column entries).
    fn column_names(columns: &serde_json::Value) -> Vec<String> {
        columns
            .as_array()
            .expect("fixture columns must be an array")
            .iter()
            .map(|entry| {
                entry
                    .get("column")
                    .and_then(serde_json::Value::as_str)
                    .expect("column object must carry a column")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn fileasset_columns_match_fixture() {
        let fixture = fixture("fileasset.columns.json");
        assert_eq!(fixture["db_table"].as_str().unwrap(), TABLE);
        assert_eq!(strings(&fixture["ordering"]), ORDERING);
        let expected: Vec<String> = owned(COLUMNS);
        assert_eq!(column_names(&fixture["fields"]), expected);
        assert_eq!(DEFAULTS.len(), COLUMNS.len(), "one default slot per column");
        let index_names: Vec<String> = INDEXES.iter().map(|i| i.name.to_string()).collect();
        let fixture_names: Vec<String> = fixture["indexes"]
            .as_array()
            .expect("indexes must be an array")
            .iter()
            .map(|i| {
                i["name"]
                    .as_str()
                    .expect("index must carry a name")
                    .to_owned()
            })
            .collect();
        assert_eq!(index_names, fixture_names);
        for (index, entry) in INDEXES.iter().zip(fixture["indexes"].as_array().unwrap()) {
            assert_eq!(
                owned(index.columns),
                strings(&entry["fields"]),
                "{}",
                index.name
            );
        }
        // Defaults spot-checks against the fixture field entries.
        let fields = fixture["fields"].as_array().unwrap();
        let default_of = |column: &str| {
            fields
                .iter()
                .find(|f| f["column"] == column)
                .unwrap_or_else(|| panic!("fixture must list {column}"))
                .get("default")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        assert!(default_of("attributes").is_object());
        assert_eq!(default_of("is_deleted"), false);
        assert_eq!(default_of("is_archived"), false);
        assert_eq!(default_of("size"), 0);
        assert!(default_of("storage_metadata").is_object());
        assert_eq!(default_of("entity_type"), serde_json::Value::Null);
        // EntityTypeContext: same value set as the fixture enum.
        let mut fixture_values: Vec<String> = fixture["enums"]["EntityTypeContext"]
            .as_object()
            .expect("enum must be an object")
            .iter()
            .filter(|(k, _)| *k != "trace" && *k != "type")
            .map(|(_, v)| v.as_str().expect("enum value must be a string").to_owned())
            .collect();
        fixture_values.sort();
        let mut const_values = owned(ENTITY_TYPES);
        const_values.sort();
        assert_eq!(const_values, fixture_values);
        assert_eq!(ENTITY_TYPES.len(), 10, "EntityTypeContext holds 10 values");
    }

    #[test]
    fn asset_url_matches_golden() {
        let golden = fixture("asset_url.golden.json");
        let id = "3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f";
        let project_id = "11111111-2222-3333-4444-555555555555";
        let issue_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        for case in golden["cases"].as_array().expect("cases must be an array") {
            let entity_type = case["entity_type"].as_str();
            let shape = case.get("shape");
            let rendered = asset_url(entity_type, id, "acme", project_id, issue_id);
            match shape {
                Some(serde_json::Value::String(shape)) => {
                    let expected = shape
                        .replace("<id>", id)
                        .replace("<workspace.slug>", "acme")
                        .replace("<project_id>", project_id)
                        .replace("<issue_id>", issue_id);
                    assert_eq!(rendered.as_deref(), Some(expected.as_str()));
                }
                _ => assert_eq!(rendered, None),
            }
        }
        // Worked examples from the golden render byte for byte.
        let examples = golden["examples"].as_array().expect("examples must exist");
        assert_eq!(
            asset_url(Some("USER_AVATAR"), id, "acme", project_id, issue_id).as_deref(),
            Some(examples[0]["asset_url"].as_str().unwrap())
        );
        assert_eq!(
            asset_url(Some("ISSUE_ATTACHMENT"), id, "acme", project_id, issue_id).as_deref(),
            Some(examples[1]["asset_url"].as_str().unwrap())
        );
        assert_eq!(
            asset_url(
                Some("DRAFT_ISSUE_ATTACHMENT"),
                id,
                "acme",
                project_id,
                issue_id
            ),
            None
        );
        assert_eq!(examples[2]["asset_url"], serde_json::Value::Null);
        assert_eq!(asset_url(None, id, "acme", project_id, issue_id), None);
    }

    #[test]
    fn upload_path_and_file_size_match_golden() {
        let golden = fixture("upload_path.golden.json");
        let hex = "9f8a7b6c5d4e3f2a1b8c7d6e5f3a2b1c";
        assert_eq!(
            upload_path_key(Some("ws-1"), hex, "logo.png"),
            format!("ws-1/{hex}-logo.png")
        );
        assert_eq!(
            upload_path_key(None, hex, "avatar.png"),
            format!("user-{hex}-avatar.png")
        );
        // The user-branch key carries no user id (golden user_branch_note).
        assert!(upload_path_key(None, hex, "avatar.png").starts_with("user-"));
        let validator = &golden["file_size_validator"];
        // The fixture message field is descriptive prose; it must carry the
        // exact literal this port reproduces (including the hardcoded 5 MB).
        assert!(
            validator["message"]
                .as_str()
                .expect("validator must carry a message")
                .contains(FILE_SIZE_MESSAGE),
            "golden must document the literal message"
        );
        assert_eq!(FILE_SIZE_LIMIT_DEFAULT, 5_242_880);
        assert_eq!(
            validate_file_size(FILE_SIZE_LIMIT_DEFAULT + 1, FILE_SIZE_LIMIT_DEFAULT),
            Err(FILE_SIZE_MESSAGE)
        );
        // Python uses strict `>`: exactly at the limit passes.
        assert_eq!(
            validate_file_size(FILE_SIZE_LIMIT_DEFAULT, FILE_SIZE_LIMIT_DEFAULT),
            Ok(())
        );
        assert_eq!(validate_file_size(0, FILE_SIZE_LIMIT_DEFAULT), Ok(()));
    }
}
