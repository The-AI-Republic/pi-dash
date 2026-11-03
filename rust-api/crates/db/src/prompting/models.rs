//! Prompting table models (D-04, stage 4).
//!
//! Ports `apps/api/pi_dash/prompting/models.py:1-140` (`PromptTemplate`
//! `:19-62`, `PromptSectionOverride` `:65-140`), adopting the
//! Django-owned schema column-for-column. Migrations are not ported;
//! Django stays schema owner until switchover. Queries stay in the
//! compose/seed layers; this module records column lists, defaults,
//! constraints, and the `__str__`/property derivations.
//!
//! Column order in each `COLUMNS` const follows Django `_meta` field order
//! (the order recorded in
//! `rust-api/fixtures/prompting/FIX-models.json`), **not** physical
//! `information_schema` ordinal order. Order is cosmetic for query
//! building; membership is the contract.
//!
//! # Application-level defaults
//!
//! Every default below is Django-level; inserts must supply these values
//! explicitly — there is no DB fallback. `name` defaults to
//! [`prompt_template::DEFAULT_NAME`]; `is_active` defaults to `true`;
//! `version` defaults to `1`; `needs_attention` defaults to `false`
//! (override rows only).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `PromptSectionOverride` carries twin partial-unique constraints
//!   (`models.py:116-125`) because Postgres treats NULLs as DISTINCT and
//!   `nulls_distinct=False` needs Django 5.0+ / PG 15+ (repo is on Django
//!   4.2): one over `(workspace, section_key)` for user-NULL rows, one
//!   over `(workspace, user, section_key)` for user-set rows. Both names
//!   are ported verbatim.
//! * The fixture `bugs` array is empty: no model-level bug to port. (The
//!   PUT-create-returns-200 quirk owns to the handler layer,
//!   PIDASHCONV-158.)
//!
//! Wiring note: the crate root declares `pub mod prompting;`
//! (foundation change, tracked separately); these files are
//! new-files-only for this issue.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; mirrors
/// `crate::license::models::OnDelete` without coupling domains).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `prompt_template` table (`models.py:19-54`, `db_table = "prompt_template"`).
pub mod prompt_template {
    use super::{Deserialize, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:46`).
    pub const TABLE: &str = "prompt_template";

    /// Default template name (`models.py:20`, `DEFAULT_NAME = "coding-task"`).
    pub const DEFAULT_NAME: &str = "coding-task";

    /// Columns in Django `_meta` field order. FK columns use the Django
    /// attnames (`workspace_id`, `updated_by_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "workspace_id",
        "name",
        "body",
        "is_active",
        "version",
        "updated_by_id",
        "created_at",
        "updated_at",
    ];

    /// Default `name` (`models.py:31`, `default=DEFAULT_NAME`).
    pub const DEFAULT_NAME_VALUE: &str = DEFAULT_NAME;

    /// Default `is_active` (`models.py:33`).
    pub const DEFAULT_IS_ACTIVE: bool = true;

    /// Default `version` (`models.py:34`).
    pub const DEFAULT_VERSION: i32 = 1;

    /// `workspace` FK (`models.py:23-30`): nullable, `CASCADE`,
    /// `related_name="prompt_templates"`. `NULL` = global default template.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = true;
    pub const WORKSPACE_RELATED_NAME: &str = "prompt_templates";

    /// `updated_by` FK (`models.py:35-41`): nullable, `SET_NULL`,
    /// `related_name="prompt_templates_updated"`.
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const UPDATED_BY_NULLABLE: bool = true;
    pub const UPDATED_BY_RELATED_NAME: &str = "prompt_templates_updated";

    /// `name` length cap (`models.py:31`, `max_length=64`).
    pub const NAME_MAX_LENGTH: usize = 64;

    /// Partial unique constraint (`models.py:47-53`):
    /// one active row per `(workspace, name)`.
    pub const UNIQUE_CONSTRAINT: &str = "prompt_template_one_active_per_ws_name";
    pub const UNIQUE_FIELDS: &[&str] = &["workspace", "name"];

    /// Read index (`models.py:54`): Django auto name on current tree.
    pub const INDEX_NAME: &str = "prompt_temp_workspa_e56e29_idx";
    pub const INDEX_FIELDS: &[&str] = &["workspace", "name", "is_active"];

    /// One `prompt_template` row. `workspace_id=None` is the global
    /// default; timestamps are `timestamptz`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PromptTemplate {
        pub id: uuid::Uuid,
        pub workspace_id: Option<uuid::Uuid>,
        pub name: String,
        pub body: String,
        pub is_active: bool,
        pub version: i32,
        pub updated_by_id: Option<uuid::Uuid>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    /// `__str__` (`models.py:56-58`):
    /// `PromptTemplate<global:{name}:v{version}>` when `workspace_id` is
    /// `None`, else `PromptTemplate<ws={workspace_id}:{name}:v{version}>`.
    /// Django renders a `None` UUID as `None`; here the id is already a
    /// string, so the caller passes `None` for the global row.
    pub fn display(workspace_id: Option<&str>, name: &str, version: i32) -> String {
        let scope = match workspace_id {
            Some(id) => format!("ws={id}"),
            None => "global".to_string(),
        };
        format!("PromptTemplate<{scope}:{name}:v{version}>")
    }

    /// `is_global_default` (`models.py:60-62`): `self.workspace_id is None`.
    pub fn is_global_default(workspace_id: Option<&uuid::Uuid>) -> bool {
        workspace_id.is_none()
    }
}

/// `prompt_section_override` table (`models.py:65-132`,
/// `db_table = "prompt_section_override"`).
pub mod prompt_section_override {
    use super::{Deserialize, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:108`).
    pub const TABLE: &str = "prompt_section_override";

    /// Columns in Django `_meta` field order. FK columns use the Django
    /// attnames (`workspace_id`, `user_id`, `updated_by_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "workspace_id",
        "user_id",
        "section_key",
        "body",
        "is_active",
        "version",
        "needs_attention",
        "updated_by_id",
        "created_at",
        "updated_at",
    ];

    /// Default `is_active` (`models.py:92`).
    pub const DEFAULT_IS_ACTIVE: bool = true;

    /// Default `version` (`models.py:93`).
    pub const DEFAULT_VERSION: i32 = 1;

    /// Default `needs_attention` (`models.py:96`): set by the
    /// re-validation command, never auto-deactivated.
    pub const DEFAULT_NEEDS_ATTENTION: bool = false;

    /// `workspace` FK (`models.py:77-81`): required, `CASCADE`,
    /// `related_name="prompt_section_overrides"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "prompt_section_overrides";

    /// `user` FK (`models.py:82-89`): nullable, `CASCADE`,
    /// `related_name="prompt_section_overrides"`. `NULL` = workspace-level
    /// override; set = personal override.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = true;
    pub const USER_RELATED_NAME: &str = "prompt_section_overrides";

    /// `updated_by` FK (`models.py:97-103`): nullable, `SET_NULL`,
    /// `related_name="prompt_section_overrides_updated"`.
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const UPDATED_BY_NULLABLE: bool = true;
    pub const UPDATED_BY_RELATED_NAME: &str = "prompt_section_overrides_updated";

    /// `section_key` length cap (`models.py:90`, `max_length=64`).
    pub const SECTION_KEY_MAX_LENGTH: usize = 64;

    /// Workspace-scope partial unique constraint (`models.py:116-120`):
    /// one active row per `(workspace, section_key)` where `user IS NULL`.
    pub const UNIQUE_WS_CONSTRAINT: &str = "prompt_section_override_one_active_ws";
    pub const UNIQUE_WS_FIELDS: &[&str] = &["workspace", "section_key"];

    /// User-scope partial unique constraint (`models.py:121-125`):
    /// one active row per `(workspace, user, section_key)` where
    /// `user IS NOT NULL`.
    pub const UNIQUE_USER_CONSTRAINT: &str = "prompt_section_override_one_active_user";
    pub const UNIQUE_USER_FIELDS: &[&str] = &["workspace", "user", "section_key"];

    /// Read index (`models.py:127-132`), explicit name.
    pub const INDEX_NAME: &str = "prompt_sec_overrid_ws_usr_idx";
    pub const INDEX_FIELDS: &[&str] = &["workspace", "user", "section_key", "is_active"];

    /// One `prompt_section_override` row. `user_id=None` is the
    /// workspace-level row; timestamps are `timestamptz`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PromptSectionOverride {
        pub id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub user_id: Option<uuid::Uuid>,
        pub section_key: String,
        pub body: String,
        pub is_active: bool,
        pub version: i32,
        pub needs_attention: bool,
        pub updated_by_id: Option<uuid::Uuid>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    /// `__str__` (`models.py:134-136`):
    /// `PromptSectionOverride<ws={workspace_id}:{scope}:{section_key}:v{version}>`
    /// where scope is `user={user_id}` or `workspace`. Ids arrive as
    /// strings; Django renders `None` as `None`, matched here by passing
    /// `None`.
    pub fn display(
        workspace_id: Option<&str>,
        user_id: Option<&str>,
        section_key: &str,
        version: i32,
    ) -> String {
        let ws = workspace_id.unwrap_or("None");
        let scope = match user_id {
            Some(id) => format!("user={id}"),
            None => "workspace".to_string(),
        };
        format!("PromptSectionOverride<ws={ws}:{scope}:{section_key}:v{version}>")
    }

    /// `is_workspace_level` (`models.py:138-140`): `self.user_id is None`.
    pub fn is_workspace_level(user_id: Option<&uuid::Uuid>) -> bool {
        user_id.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::prompt_section_override as pso;
    use super::prompt_template as pt;
    use super::OnDelete;

    fn fixture_models() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/prompting/FIX-models.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let root: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        root["data"]["models"].clone()
    }

    fn column_names(model: &serde_json::Value) -> Vec<String> {
        model["columns"]
            .as_array()
            .expect("columns array")
            .iter()
            .map(|c| c["column"].as_str().expect("column attname").to_string())
            .collect()
    }

    /// Parse a fixture `"True"` / `"False"` default into a bool.
    fn fx_bool(raw: &str) -> bool {
        match raw {
            "True" => true,
            "False" => false,
            other => panic!("unexpected bool default {other}"),
        }
    }

    #[test]
    fn template_columns_table_defaults_match_fixture() {
        let m = fixture_models();
        let fx = &m["prompt_template"];
        assert_eq!(fx["db_table"].as_str().unwrap(), pt::TABLE);
        let expected: Vec<String> = pt::COLUMNS.iter().map(|s| s.to_string()).collect();
        assert_eq!(column_names(fx), expected);
        assert_eq!(m["default_name_const"].as_str().unwrap(), pt::DEFAULT_NAME);
        assert_eq!(
            fx["columns"][2]["default"].as_str().unwrap(),
            format!("'{}'", pt::DEFAULT_NAME_VALUE)
        );
        assert_eq!(
            pt::NAME_MAX_LENGTH as u64,
            fx["columns"][2]["max_length"].as_u64().unwrap()
        );
        assert_eq!(
            pt::DEFAULT_IS_ACTIVE,
            fx_bool(fx["columns"][4]["default"].as_str().unwrap())
        );
        assert_eq!(
            pt::DEFAULT_VERSION.to_string(),
            fx["columns"][5]["default"].as_str().unwrap()
        );
    }

    #[test]
    fn template_fk_semantics_match_python() {
        let m = fixture_models();
        let fx = &m["prompt_template"];
        assert_eq!(pt::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            pt::WORKSPACE_NULLABLE,
            fx["columns"][1]["null"].as_bool().unwrap()
        );
        assert_eq!(pt::WORKSPACE_RELATED_NAME, "prompt_templates");
        assert_eq!(pt::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(
            pt::UPDATED_BY_NULLABLE,
            fx["columns"][6]["null"].as_bool().unwrap()
        );
        assert_eq!(pt::UPDATED_BY_RELATED_NAME, "prompt_templates_updated");
    }

    #[test]
    fn template_constraint_and_index_match_fixture() {
        let m = fixture_models();
        let fx = &m["prompt_template"];
        let constraints = fx["constraints"].as_array().expect("constraints");
        assert_eq!(constraints.len(), 1);
        assert_eq!(
            constraints[0]["name"].as_str().unwrap(),
            pt::UNIQUE_CONSTRAINT
        );
        let fields: Vec<&str> = constraints[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(fields, pt::UNIQUE_FIELDS);
        let indexes = fx["indexes"].as_array().expect("indexes");
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0]["name"].as_str().unwrap(), pt::INDEX_NAME);
        let idx_fields: Vec<&str> = indexes[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(idx_fields, pt::INDEX_FIELDS);
    }

    #[test]
    fn template_str_and_global_default_match_fixture() {
        let m = fixture_models();
        assert_eq!(
            m["str_template_global"].as_str().unwrap(),
            pt::display(None, "coding-task", 1)
        );
        assert_eq!(
            pt::display(
                Some("11111111-1111-1111-1111-111111111111"),
                "coding-task",
                2
            ),
            "PromptTemplate<ws=11111111-1111-1111-1111-111111111111:coding-task:v2>"
        );
        assert!(m["is_global_default_none"].as_bool().unwrap());
        assert!(pt::is_global_default(None));
        assert!(!pt::is_global_default(Some(&uuid::Uuid::nil())));
    }

    #[test]
    fn override_columns_table_defaults_match_fixture() {
        let m = fixture_models();
        let fx = &m["prompt_section_override"];
        assert_eq!(fx["db_table"].as_str().unwrap(), pso::TABLE);
        let expected: Vec<String> = pso::COLUMNS.iter().map(|s| s.to_string()).collect();
        assert_eq!(column_names(fx), expected);
        assert_eq!(
            pso::SECTION_KEY_MAX_LENGTH as u64,
            fx["columns"][3]["max_length"].as_u64().unwrap()
        );
        assert_eq!(
            pso::DEFAULT_IS_ACTIVE,
            fx_bool(fx["columns"][5]["default"].as_str().unwrap())
        );
        assert_eq!(
            pso::DEFAULT_VERSION.to_string(),
            fx["columns"][6]["default"].as_str().unwrap()
        );
        assert_eq!(
            pso::DEFAULT_NEEDS_ATTENTION,
            fx_bool(fx["columns"][7]["default"].as_str().unwrap())
        );
    }

    #[test]
    fn override_fk_semantics_match_python() {
        let m = fixture_models();
        let fx = &m["prompt_section_override"];
        assert_eq!(pso::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            pso::WORKSPACE_NULLABLE,
            fx["columns"][1]["null"].as_bool().unwrap()
        );
        assert_eq!(pso::WORKSPACE_RELATED_NAME, "prompt_section_overrides");
        assert_eq!(pso::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            pso::USER_NULLABLE,
            fx["columns"][2]["null"].as_bool().unwrap()
        );
        assert_eq!(pso::USER_RELATED_NAME, "prompt_section_overrides");
        assert_eq!(pso::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(
            pso::UPDATED_BY_NULLABLE,
            fx["columns"][8]["null"].as_bool().unwrap()
        );
        assert_eq!(
            pso::UPDATED_BY_RELATED_NAME,
            "prompt_section_overrides_updated"
        );
    }

    #[test]
    fn override_twin_constraints_and_index_match_fixture() {
        let m = fixture_models();
        let fx = &m["prompt_section_override"];
        let constraints = fx["constraints"].as_array().expect("constraints");
        assert_eq!(constraints.len(), 2);
        assert_eq!(
            constraints[0]["name"].as_str().unwrap(),
            pso::UNIQUE_WS_CONSTRAINT
        );
        let ws_fields: Vec<&str> = constraints[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ws_fields, pso::UNIQUE_WS_FIELDS);
        assert_eq!(
            constraints[1]["name"].as_str().unwrap(),
            pso::UNIQUE_USER_CONSTRAINT
        );
        let user_fields: Vec<&str> = constraints[1]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(user_fields, pso::UNIQUE_USER_FIELDS);
        let indexes = fx["indexes"].as_array().expect("indexes");
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0]["name"].as_str().unwrap(), pso::INDEX_NAME);
        let idx_fields: Vec<&str> = indexes[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(idx_fields, pso::INDEX_FIELDS);
    }

    #[test]
    fn override_str_and_workspace_level_match_fixture() {
        let m = fixture_models();
        assert_eq!(
            m["str_override_ws"].as_str().unwrap(),
            pso::display(None, None, "intro", 1)
        );
        assert_eq!(
            pso::display(
                Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
                Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
                "intro",
                3
            ),
            "PromptSectionOverride<ws=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa:user=bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb:intro:v3>"
        );
        assert!(m["is_workspace_level_none"].as_bool().unwrap());
        assert!(pso::is_workspace_level(None));
        assert!(!pso::is_workspace_level(Some(&uuid::Uuid::nil())));
    }
}
