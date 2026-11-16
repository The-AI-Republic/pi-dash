//! Typed per-project settings (`projects.settings`, NEWPIDASH-278).
//!
//! `projects.settings` is one namespaced JSON bag per project
//! (`jsonb NOT NULL DEFAULT '{}'::jsonb`, db `0182`), owned by the Rust
//! system: Django never reads or writes it beyond storing it. This
//! module is the single definition of every key, its type and its
//! default. New project settings then need no schema migration — add
//! the key to [`ProjectSettings`] and its arm to
//! [`check_key_value`] and [`KNOWN_KEYS`].
//!
//! Adding a new setting:
//! 1. Add the field to [`ProjectSettings`] with `#[serde(default)]`
//!    and a `default_*` function for non-`Default` values.
//! 2. Add the key to [`KNOWN_KEYS`] with the role that may change it
//!    ([`SettingsRole`]).
//! 3. Add the `match` arm for the key in [`check_key_value`] (type
//!    check the incoming value).
//! 4. No migration, no entity change: reads fill the default for
//!    missing keys, writes merge the validated key.
//!
//! Keep a setting as its own column instead of a JSON key when it is:
//! - used to filter or join across many projects (for example the
//!   tick scheduler's sweep);
//! - a foreign key that needs database integrity
//!   (`default_state_id`, `estimate_id`, ...);
//! - core access control (`network`).
//!
//! Read/write contract:
//! - Reads tolerate unknown keys (forward compatibility) and carry
//!   them in [`ParsedSettings::extra`]; the atomic merge below never
//!   drops them, because it ORs the validated fragment into the
//!   stored blob instead of replacing it.
//! - Writes reject unknown keys and bad values with a 400 field
//!   error ([`SettingsError`]), and a key the caller may not change
//!   with a 403. Validation always runs before the merge.
//! - Missing keys read as their default (`#[serde(default)]`), so
//!   `{}` means "all defaults".

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, UpdateMany};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_auth::permissions::ROLE_ADMIN;
use pidash_db::entity::projects;
use pidash_db::orm_tx::into_sqlx;

/// The role that may change a settings key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRole {
    /// The project admin, or any workspace admin.
    ProjectAdmin,
    /// A workspace admin only.
    WorkspaceAdmin,
}

impl SettingsRole {
    /// Whether a caller with `project_role` (project membership role)
    /// and `workspace_role` (workspace membership role) may change a
    /// key requiring this role. Non-member roles are [`ROLE_GUEST`]
    /// (or any lower value).
    pub fn allows(self, project_role: i32, workspace_role: i32) -> bool {
        match self {
            SettingsRole::ProjectAdmin => {
                project_role >= ROLE_ADMIN || workspace_role >= ROLE_ADMIN
            }
            SettingsRole::WorkspaceAdmin => workspace_role >= ROLE_ADMIN,
        }
    }
}

/// One registered settings key: its JSON name and the role that may
/// change it.
#[derive(Debug, Clone, Copy)]
pub struct SettingsKey {
    /// The JSON object key in `projects.settings`.
    pub name: &'static str,
    /// Who may change it.
    pub role: SettingsRole,
}

/// Every known settings key, in wire order. Empty until the first
/// consumer adds one (NEWPIDASH-261): reads render `{}`, PATCH
/// rejects any key as unknown.
pub const KNOWN_KEYS: &[SettingsKey] = &[];

/// The typed settings: the single definition of each key, its type
/// and its default. Stored as one JSON object; missing keys read as
/// their `#[serde(default)]` value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {}

/// The validated read of one stored blob: the typed settings plus
/// the unknown keys carried for forward compatibility.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSettings {
    /// The typed settings with defaults filled in.
    pub settings: ProjectSettings,
    /// Keys no registered key claims, preserved across writes.
    pub extra: Map<String, Value>,
}

/// A settings validation or authorization failure. [`SettingsError::status`]
/// is the HTTP status; [`SettingsError::body`] the byte-exact response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// 400: the stored blob (or the update fragment) is not a JSON
    /// object.
    NotAnObject,
    /// 400: the key is not in [`KNOWN_KEYS`].
    UnknownKey { key: String },
    /// 400: the key is known but the value has the wrong type.
    InvalidValue { key: String, detail: String },
    /// 403: the caller may not change this key.
    Forbidden { key: String },
    /// 500: the stored blob is not an object the typed struct accepts
    /// (corrupt write; writes validate first, so this is unreachable
    /// through the API).
    InvalidStored(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::NotAnObject => write!(f, "settings must be a JSON object"),
            SettingsError::UnknownKey { key } => write!(f, "unknown setting: {key}"),
            SettingsError::InvalidValue { key, detail } => {
                write!(f, "invalid value for {key}: {detail}")
            }
            SettingsError::Forbidden { key } => {
                write!(f, "may not change setting {key}")
            }
            SettingsError::InvalidStored(detail) => write!(f, "invalid stored settings: {detail}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl SettingsError {
    /// HTTP status Django-style handlers answer with.
    pub fn status(&self) -> u16 {
        match self {
            SettingsError::Forbidden { .. } => 403,
            SettingsError::InvalidStored(_) => 500,
            _ => 400,
        }
    }

    /// Byte-exact wire body. 400s are the field-error shape
    /// `{"settings": ["..."]}`; 403 matches the project admin bodies
    /// in both handler families (`{"error": "You don't have the
    /// required permissions."}` as in `handlers_project.rs`
    /// `ADMIN_REQUIRED_BODY`).
    pub fn body(&self) -> String {
        match self {
            SettingsError::NotAnObject => field_body("settings must be a JSON object"),
            SettingsError::UnknownKey { key } => field_body(&format!("unknown setting: {key}")),
            SettingsError::InvalidValue { key, detail } => {
                field_body(&format!("invalid value for {key}: {detail}"))
            }
            SettingsError::Forbidden { .. } => {
                r#"{"error":"You don't have the required permissions."}"#.to_owned()
            }
            SettingsError::InvalidStored(_) => {
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            }
        }
    }
}

/// `{"settings": ["<detail>"]}` — the DRF field-error shape for the
/// nested settings key.
fn field_body(detail: &str) -> String {
    let quoted = serde_json::to_string(detail).unwrap_or_else(|_| "\"\"".to_owned());
    format!("{{\"settings\":[{quoted}]}}")
}

/// The role that may change `key`, or `None` when the key is unknown.
pub fn required_role(key: &str) -> Option<SettingsRole> {
    KNOWN_KEYS.iter().find(|k| k.name == key).map(|k| k.role)
}

/// Whether the caller may change every key in `fragment`. A fragment
/// holding a key the registry does not know is [`SettingsError::UnknownKey`]
/// (validation rejects unknown keys, so the permission check answers
/// the same for an unknown key); a known key the caller may not
/// change is [`SettingsError::Forbidden`].
pub fn check_permissions(
    fragment: &Map<String, Value>,
    project_role: i32,
    workspace_role: i32,
) -> Result<(), SettingsError> {
    for key in fragment.keys() {
        match required_role(key) {
            None => return Err(SettingsError::UnknownKey { key: key.clone() }),
            Some(role) if !role.allows(project_role, workspace_role) => {
                return Err(SettingsError::Forbidden { key: key.clone() });
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Type-check one incoming key/value pair. New keys add an arm here:
/// deserialize `value` into the key's type and answer
/// [`SettingsError::InvalidValue`] on mismatch.
fn check_key_value(key: &str, value: &Value) -> Result<(), SettingsError> {
    // No keys yet: every key is unknown. The first consumer
    // (NEWPIDASH-261, `issue_run_policy`) turns this into a `match`
    // with one arm per key, each type-checking its value.
    let _ = value;
    Err(SettingsError::UnknownKey {
        key: key.to_owned(),
    })
}

/// Parse one stored blob (the `projects.settings` column for one
/// project). Unknown keys are tolerated and carried in `extra`;
/// known keys with wrong-typed values surface as
/// [`SettingsError::InvalidStored`].
pub fn parse_stored(stored: &Value) -> Result<ParsedSettings, SettingsError> {
    let Value::Object(object) = stored else {
        return Err(SettingsError::InvalidStored(format!(
            "expected object, got {}",
            stored_type_name(stored)
        )));
    };
    let settings: ProjectSettings = serde_json::from_value(stored.clone())
        .map_err(|e| SettingsError::InvalidStored(e.to_string()))?;
    let mut extra = Map::new();
    for (key, value) in object {
        if required_role(key).is_none() {
            extra.insert(key.clone(), value.clone());
        }
    }
    Ok(ParsedSettings { settings, extra })
}

fn stored_type_name(stored: &Value) -> &'static str {
    match stored {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validate one PATCH `settings` fragment: it must be an object whose
/// every key is known and whose every value type-checks. Returns the
/// validated fragment to merge into the stored blob (the fragment
/// itself — unknown keys are rejected, so nothing extra survives).
pub fn validate_update(fragment: &Value) -> Result<Value, SettingsError> {
    let Value::Object(object) = fragment else {
        return Err(SettingsError::NotAnObject);
    };
    for (key, value) in object {
        check_key_value(key, value)?;
    }
    Ok(Value::Object(object.clone()))
}

/// The read shape: the validated settings with defaults filled in.
/// `{}` until the first key lands.
pub fn render(settings: &ProjectSettings) -> Value {
    serde_json::to_value(settings).unwrap_or(Value::Object(Map::new()))
}

// ---------------------------------------------------------------------------
// Atomic merge write
// ---------------------------------------------------------------------------

/// The atomic partial-merge statement for [`apply_settings_merge`]:
/// `UPDATE "projects" SET "settings" = "settings" || $1::jsonb,
/// "updated_at" = $2, "updated_by" = $3 WHERE "id" = $4 AND
/// "deleted_at" IS NULL`. The `||` keeps any unknown keys (and any
/// known keys the fragment does not touch) in place, so two writers
/// changing different keys concurrently never overwrite each other.
pub(crate) fn settings_merge_stmt(
    merge: &Value,
    now: DateTime<Utc>,
    actor: Uuid,
    id: Uuid,
) -> UpdateMany<projects::Entity> {
    // raw-sql: SeaORM cannot express the jsonb `||` merge itself;
    // `cust_with_values` binds the validated fragment as one `$N`.
    let settings = Expr::cust_with_values("\"settings\" || $1::jsonb", [merge.clone()]);
    projects::Entity::update_many()
        .col_expr(projects::Column::Settings, settings)
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(actor))
        .filter(projects::Column::Id.eq(id))
        .filter(projects::Column::DeletedAt.is_null())
}

/// Merge the validated `merge` fragment into the project's stored
/// settings and stamp the audit pair, in one statement on the
/// caller's connection (pool or transaction).
pub async fn apply_settings_merge(
    db: &impl ConnectionTrait,
    merge: &Value,
    now: DateTime<Utc>,
    actor: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    settings_merge_stmt(merge, now, actor, id)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Parse the stored blob off a live instance read once per request:
/// the project row's `settings` column parsed to [`ParsedSettings`].
pub fn parse_row_settings(settings: &Value) -> Result<ParsedSettings, SettingsError> {
    parse_stored(settings)
}

/// The empty all-defaults bag for rows created without naming the
/// column (migration default `Value::Object`).
pub fn empty_bag() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use sea_orm::QueryTrait;
    use serde_json::json;

    use pidash_auth::permissions::{ROLE_GUEST, ROLE_MEMBER};

    use super::*;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        Value::Object(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn empty_bag_parses_to_defaults() {
        let parsed = parse_stored(&empty_bag()).expect("empty bag parses");
        assert_eq!(parsed.settings, ProjectSettings::default());
        assert!(parsed.extra.is_empty());
        assert_eq!(render(&parsed.settings), json!({}));
    }

    #[test]
    fn unknown_keys_are_tolerated_and_carried_on_read() {
        // Forward compatibility: a writer newer than this build may
        // add keys; this build must read them without loss.
        let stored = obj(&[("future_key", json!({"x": 1})), ("another", json!("y"))]);
        let parsed = parse_stored(&stored).expect("unknown keys tolerated");
        assert_eq!(parsed.settings, ProjectSettings::default());
        assert_eq!(parsed.extra.get("future_key"), Some(&json!({"x": 1})));
        assert_eq!(parsed.extra.get("another"), Some(&json!("y")));
    }

    #[test]
    fn non_object_stored_blob_is_invalid_stored() {
        for stored in [json!(null), json!([]), json!("x"), json!(1), json!(true)] {
            assert!(matches!(
                parse_stored(&stored),
                Err(SettingsError::InvalidStored(_))
            ));
        }
    }

    #[test]
    fn update_with_any_key_is_unknown_until_first_key_lands() {
        let fragment = obj(&[("anything", json!(true))]);
        assert_eq!(
            validate_update(&fragment),
            Err(SettingsError::UnknownKey {
                key: "anything".to_owned()
            })
        );
        // An empty fragment validates and merges as a no-op.
        assert_eq!(validate_update(&empty_bag()), Ok(empty_bag()));
    }

    #[test]
    fn non_object_fragment_is_not_an_object() {
        assert_eq!(validate_update(&json!([])), Err(SettingsError::NotAnObject));
        assert_eq!(
            validate_update(&json!(null)),
            Err(SettingsError::NotAnObject)
        );
        assert_eq!(
            validate_update(&json!("x")),
            Err(SettingsError::NotAnObject)
        );
    }

    #[test]
    fn unknown_key_is_a_400_field_error() {
        let error = SettingsError::UnknownKey {
            key: "nope".to_owned(),
        };
        assert_eq!(error.status(), 400);
        assert_eq!(error.body(), r#"{"settings":["unknown setting: nope"]}"#);
    }

    #[test]
    fn forbidden_is_a_403_with_the_project_admin_body() {
        let error = SettingsError::Forbidden {
            key: "some_key".to_owned(),
        };
        assert_eq!(error.status(), 403);
        assert_eq!(
            error.body(),
            r#"{"error":"You don't have the required permissions."}"#
        );
    }

    #[test]
    fn permission_check_denies_unknown_keys() {
        // With no registered keys, check_permissions and
        // validate_update agree: every key is UnknownKey.
        let fragment = obj(&[("anything", json!(1))]);
        let Value::Object(object) = &fragment else {
            panic!("fragment is an object");
        };
        assert_eq!(
            check_permissions(object, ROLE_ADMIN, ROLE_ADMIN),
            Err(SettingsError::UnknownKey {
                key: "anything".to_owned()
            })
        );
    }

    #[test]
    fn role_allows_encodes_the_admin_hierarchy() {
        // ProjectAdmin: project admin or workspace admin.
        assert!(SettingsRole::ProjectAdmin.allows(ROLE_ADMIN, ROLE_GUEST));
        assert!(SettingsRole::ProjectAdmin.allows(ROLE_GUEST, ROLE_ADMIN));
        assert!(!SettingsRole::ProjectAdmin.allows(ROLE_MEMBER, ROLE_MEMBER));
        assert!(!SettingsRole::ProjectAdmin.allows(ROLE_GUEST, ROLE_GUEST));
        // WorkspaceAdmin: workspace admin only — a project admin who
        // is not a workspace admin does not qualify.
        assert!(SettingsRole::WorkspaceAdmin.allows(ROLE_GUEST, ROLE_ADMIN));
        assert!(!SettingsRole::WorkspaceAdmin.allows(ROLE_ADMIN, ROLE_GUEST));
        assert!(!SettingsRole::WorkspaceAdmin.allows(ROLE_ADMIN, ROLE_MEMBER));
    }

    #[test]
    fn merge_statement_uses_the_jsonb_concat_operator() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).expect("epoch");
        let id = Uuid::nil();
        let actor = Uuid::nil();
        let merge = obj(&[("key", json!("value"))]);
        let stmt = settings_merge_stmt(&merge, now, actor, id);
        let sql = stmt.build(sea_orm::DbBackend::Postgres).to_string();
        // Atomic partial merge: the stored blob ORs with the fragment.
        assert!(sql.contains("\"settings\" ||"), "sql: {sql}");
        assert!(sql.contains("UPDATE \"projects\""), "sql: {sql}");
        assert!(sql.contains("\"updated_at\" ="), "sql: {sql}");
    }
}
