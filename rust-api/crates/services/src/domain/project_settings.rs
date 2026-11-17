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

use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_db::entity::{issue_assignees, issues, project_members, projects};
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

/// Every known settings key, in wire order.
pub const KNOWN_KEYS: &[SettingsKey] = &[SettingsKey {
    name: "issue_run_policy",
    role: SettingsRole::ProjectAdmin,
}];

/// Who may a human move an issue's state or start/cancel/release an
/// agent run on it (NEWPIDASH-261). Stored as the `issue_run_policy`
/// key string in `projects.settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueRunPolicy {
    /// Only the issue's creator and its current assignees.
    #[default]
    CreatorAndAssignees,
    /// Any active project Member or Admin.
    ProjectMembers,
}

/// The typed settings: the single definition of each key, its type
/// and its default. Stored as one JSON object; missing keys read as
/// their `#[serde(default)]` value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// Who may move an issue's state or run it. Missing reads as
    /// `creator_and_assignees`, so `{}` (every existing project) is
    /// the strict default mode.
    pub issue_run_policy: IssueRunPolicy,
}

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
    match key {
        "issue_run_policy" => match value {
            Value::String(s)
                if matches!(s.as_str(), "creator_and_assignees" | "project_members") =>
            {
                Ok(())
            }
            _ => Err(SettingsError::InvalidValue {
                key: key.to_owned(),
                detail: "must be \"creator_and_assignees\" or \"project_members\"".to_owned(),
            }),
        },
        _ => Err(SettingsError::UnknownKey {
            key: key.to_owned(),
        }),
    }
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

/// The PATCH entry point: authorize every key the caller sends, then
/// validate the fragment. Permissions run first so an actor that may
/// not change a key learns nothing about the key's value shape.
pub fn authorize_and_validate(
    fragment: &Value,
    project_role: i32,
    workspace_role: i32,
) -> Result<Value, SettingsError> {
    let Value::Object(object) = fragment else {
        return Err(SettingsError::NotAnObject);
    };
    check_permissions(object, project_role, workspace_role)?;
    validate_update(fragment)
}

/// The read shape: the validated settings with defaults filled in.
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

/// Machine-readable refusal reason for a human who may not move an
/// issue's state or run it under the project's `issue_run_policy`
/// (NEWPIDASH-261).
pub const NOT_ISSUE_RUNNER: &str = "not_issue_runner";

/// Human message for [`NOT_ISSUE_RUNNER`].
pub const NOT_ISSUE_RUNNER_MESSAGE: &str = "Only the issue's creator and assignees can do this in this project. Ask the creator to assign you, or ask a project admin to change the project's 'Who can run issues' setting.";

/// Pure decision behind [`can_run_issue`]: whether a human with the
/// given facts may move the issue's state or start/cancel/release a
/// run under `policy`. `member_role` is the caller's active project
/// membership role (`None` when not an active member).
///
/// - `creator_and_assignees`: creator or current (live) assignee only.
///   There is no admin bypass (NEWPIDASH-261 open decision: no).
/// - `project_members`: active Member (15) or Admin (20) only; guests
///   (5) and non-members never pass.
pub fn can_run_issue_pure(
    policy: IssueRunPolicy,
    is_creator: bool,
    is_assignee: bool,
    member_role: Option<i32>,
) -> bool {
    match policy {
        IssueRunPolicy::CreatorAndAssignees => is_creator || is_assignee,
        IssueRunPolicy::ProjectMembers => {
            member_role.map(|role| role >= ROLE_MEMBER).unwrap_or(false)
        }
    }
}

/// Eligible runner-owner set for a work-item run under `policy`
/// (NEWPIDASH-261 dispatch side). The base is always the issue's
/// creator (when set) plus its current assignees; `project_members`
/// adds the run's triggerer again, while `creator_and_assignees`
/// does not (a triggerer-only runner never takes the run).
pub fn eligible_owner_ids(
    issue_creator: Option<Uuid>,
    issue_people: &std::collections::HashSet<Uuid>,
    triggerer: Option<Uuid>,
    policy: IssueRunPolicy,
) -> std::collections::HashSet<Uuid> {
    let mut eligible = issue_people.clone();
    if let Some(creator) = issue_creator {
        eligible.insert(creator);
    }
    if policy == IssueRunPolicy::ProjectMembers {
        if let Some(triggerer) = triggerer {
            eligible.insert(triggerer);
        }
    }
    eligible
}

/// The one shared human gate (NEWPIDASH-261): whether `actor_id` may
/// move `issue`'s state or start/cancel/release an agent run on it
/// under `project`'s `issue_run_policy`. Callers answer a refusal
/// with 403 [`NOT_ISSUE_RUNNER`] + [`NOT_ISSUE_RUNNER_MESSAGE`];
/// system actions (ticks, continuations, review/test entry runs,
/// project-move handoffs, agent self-actions inside a run) never
/// call this check.
pub async fn can_run_issue(
    db: &impl ConnectionTrait,
    actor_id: Uuid,
    issue: &issues::Model,
    project: &projects::Model,
) -> Result<bool, sea_orm::DbErr> {
    let parsed = parse_row_settings(&project.settings)
        .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    let policy = parsed.settings.issue_run_policy;
    let is_creator = issue.created_by_id == Some(actor_id);
    let is_assignee = issue_assignees::Entity::find()
        .filter(issue_assignees::Column::IssueId.eq(issue.id))
        .filter(issue_assignees::Column::AssigneeId.eq(actor_id))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some();
    let member_role = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project.id))
        .filter(project_members::Column::MemberId.eq(actor_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .map(|row| i32::from(row.role));
    Ok(can_run_issue_pure(
        policy,
        is_creator,
        is_assignee,
        member_role,
    ))
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
        assert_eq!(
            parsed.settings.issue_run_policy,
            IssueRunPolicy::CreatorAndAssignees
        );
        assert!(parsed.extra.is_empty());
        assert_eq!(
            render(&parsed.settings),
            json!({"issue_run_policy": "creator_and_assignees"})
        );
    }

    #[test]
    fn issue_run_policy_reads_stored_value() {
        let stored = obj(&[("issue_run_policy", json!("project_members"))]);
        let parsed = parse_stored(&stored).expect("stored value parses");
        assert_eq!(
            parsed.settings.issue_run_policy,
            IssueRunPolicy::ProjectMembers
        );
        assert!(parsed.extra.is_empty());
        assert_eq!(
            render(&parsed.settings),
            json!({"issue_run_policy": "project_members"})
        );
    }

    #[test]
    fn issue_run_policy_accepts_both_values() {
        for value in ["creator_and_assignees", "project_members"] {
            let fragment = obj(&[("issue_run_policy", json!(value))]);
            assert_eq!(
                validate_update(&fragment),
                Ok(fragment.clone()),
                "value {value}"
            );
        }
    }

    #[test]
    fn issue_run_policy_rejects_bad_values_as_invalid_value() {
        // Non-string JSON types.
        for bad in [
            json!(null),
            json!(true),
            json!(1),
            json!([]),
            json!({}),
            json!("creator"),
            json!(""),
            json!("PROJECT_MEMBERS"),
            json!("creator_and_assignees "),
        ] {
            let fragment = obj(&[("issue_run_policy", bad.clone())]);
            let error = validate_update(&fragment).expect_err("bad value rejected");
            assert_eq!(
                error,
                SettingsError::InvalidValue {
                    key: "issue_run_policy".to_owned(),
                    detail: "must be \"creator_and_assignees\" or \"project_members\"".to_owned(),
                },
                "bad {bad}"
            );
            assert_eq!(error.status(), 400, "bad {bad}");
        }
    }

    #[test]
    fn issue_run_policy_stored_wrong_type_is_invalid_stored() {
        let stored = obj(&[("issue_run_policy", json!(1))]);
        assert!(matches!(
            parse_stored(&stored),
            Err(SettingsError::InvalidStored(_))
        ));
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
    fn known_and_unknown_keys_coexist_on_read() {
        let stored = obj(&[
            ("issue_run_policy", json!("project_members")),
            ("future_key", json!(7)),
        ]);
        let parsed = parse_stored(&stored).expect("mixed blob parses");
        assert_eq!(
            parsed.settings.issue_run_policy,
            IssueRunPolicy::ProjectMembers
        );
        assert_eq!(parsed.extra.get("future_key"), Some(&json!(7)));
        assert!(!parsed.extra.contains_key("issue_run_policy"));
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
    fn update_with_unregistered_key_is_unknown() {
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
    fn issue_run_policy_permission_is_project_admin() {
        use pidash_auth::permissions::ROLE_ADMIN as ADMIN;
        // Project admin and workspace admin may change it; members
        // and guests may not (403 Forbidden).
        let fragment = obj(&[("issue_run_policy", json!("project_members"))]);
        let Value::Object(object) = &fragment else {
            panic!("fragment is an object");
        };
        assert!(check_permissions(object, ADMIN, ROLE_GUEST).is_ok());
        assert!(check_permissions(object, ROLE_GUEST, ADMIN).is_ok());
        assert_eq!(
            check_permissions(object, ROLE_MEMBER, ROLE_MEMBER),
            Err(SettingsError::Forbidden {
                key: "issue_run_policy".to_owned()
            })
        );
        assert_eq!(
            check_permissions(object, ROLE_GUEST, ROLE_GUEST),
            Err(SettingsError::Forbidden {
                key: "issue_run_policy".to_owned()
            })
        );
        // authorize_and_validate runs permissions first: a member
        // gets 403 even with a well-formed value.
        assert_eq!(
            authorize_and_validate(&fragment, ROLE_MEMBER, ROLE_MEMBER),
            Err(SettingsError::Forbidden {
                key: "issue_run_policy".to_owned()
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
    fn can_run_issue_pure_covers_both_modes() {
        use pidash_auth::permissions::{
            ROLE_ADMIN as ADMIN, ROLE_GUEST as GUEST, ROLE_MEMBER as MEMBER,
        };
        // Default mode: creator or live assignee only; project
        // membership (even Admin) without creator/assignee fails —
        // there is no admin bypass.
        assert!(can_run_issue_pure(
            IssueRunPolicy::CreatorAndAssignees,
            true,
            false,
            None
        ));
        assert!(can_run_issue_pure(
            IssueRunPolicy::CreatorAndAssignees,
            false,
            true,
            None
        ));
        assert!(!can_run_issue_pure(
            IssueRunPolicy::CreatorAndAssignees,
            false,
            false,
            Some(ADMIN)
        ));
        assert!(!can_run_issue_pure(
            IssueRunPolicy::CreatorAndAssignees,
            false,
            false,
            Some(MEMBER)
        ));
        // project_members: Member/Admin pass, guest and non-member fail
        // regardless of creator/assignee facts.
        assert!(can_run_issue_pure(
            IssueRunPolicy::ProjectMembers,
            false,
            false,
            Some(MEMBER)
        ));
        assert!(can_run_issue_pure(
            IssueRunPolicy::ProjectMembers,
            false,
            false,
            Some(ADMIN)
        ));
        assert!(!can_run_issue_pure(
            IssueRunPolicy::ProjectMembers,
            false,
            false,
            Some(GUEST)
        ));
        assert!(!can_run_issue_pure(
            IssueRunPolicy::ProjectMembers,
            false,
            false,
            None
        ));
    }

    #[test]
    fn eligible_owner_ids_follows_the_policy() {
        use std::collections::HashSet;
        let creator = Uuid::from_u128(1);
        let assignee = Uuid::from_u128(2);
        let triggerer = Uuid::from_u128(3);
        let people: HashSet<Uuid> = [assignee].into_iter().collect();
        // Default: triggerer excluded — a triggerer-only runner never
        // takes the run.
        let default_set = eligible_owner_ids(
            Some(creator),
            &people,
            Some(triggerer),
            IssueRunPolicy::CreatorAndAssignees,
        );
        assert!(default_set.contains(&creator));
        assert!(default_set.contains(&assignee));
        assert!(!default_set.contains(&triggerer));
        // project_members: triggerer is admitted again.
        let members_set = eligible_owner_ids(
            Some(creator),
            &people,
            Some(triggerer),
            IssueRunPolicy::ProjectMembers,
        );
        assert!(members_set.contains(&creator));
        assert!(members_set.contains(&assignee));
        assert!(members_set.contains(&triggerer));
        // No triggerer (system/automatic run): both modes agree on the
        // creator+assignee base.
        let system_set =
            eligible_owner_ids(Some(creator), &people, None, IssueRunPolicy::ProjectMembers);
        assert_eq!(system_set, default_set);
        // No creator and no people (orphan row): empty set.
        let empty: HashSet<Uuid> = HashSet::new();
        assert!(
            eligible_owner_ids(None, &empty, None, IssueRunPolicy::CreatorAndAssignees).is_empty()
        );
    }

    #[test]
    fn not_issue_runner_reason_and_message_are_stable() {
        assert_eq!(NOT_ISSUE_RUNNER, "not_issue_runner");
        assert_eq!(
            NOT_ISSUE_RUNNER_MESSAGE,
            "Only the issue's creator and assignees can do this in this project. Ask the creator to assign you, or ask a project admin to change the project's 'Who can run issues' setting."
        );
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
