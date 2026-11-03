#![forbid(unsafe_code)]

//! Project pod model (D-15, stage 5).
//!
//! Ports `Pod` (`apps/api/pi_dash/runner/models.py:52-177`):
//! the default manager's soft-delete exclusion (`PodManager`,
//! `:42-49`), `clean`/`save` workspace denorm + autofill
//! (`:127-159`), `default_for_project_id` (`:173-176`),
//! constraints/indexes (`Meta`, `:98-122`). Pod *views* are D-13's;
//! the model lives here per the epic and D-13's split reuses this
//! module.
//!
//! `default_for_project` (`:161-171`) needs no port: it delegates to
//! `default_for_project_id(project.id)` in one line, so callers pass
//! the project id they already hold.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02 `Pod`, `pod_manager_sql`, `pod_clean_save`,
//! `default_for_project_id`).
//!
//! Ported bugs: none found in this unit on read-through. Two
//! sharp edges are ported as-is and noted here: `save` reads
//! `self.project.workspace_id`, so saving a pod whose `project_id`
//! names a missing project raises `Project.DoesNotExist` — the
//! caller must fetch the project row first
//! ([`resolve_workspace_id`] takes the fetched id); and neither
//! `clean` nor `save` touches `workspace_id` when `project_id` is
//! unset (both skip, `models.py:136,151`), so a project-less pod
//! keeps whatever `workspace_id` it carries, including `NULL`.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:99`).
pub const TABLE: &str = "pod";
/// Default ordering (`Meta.ordering`, `models.py:100`).
pub const ORDERING: &[&str] = &["-is_default", "created_at"];

/// Columns in declaration order (`models.py:70-93`), FK entries as
/// the Django attnames.
pub const COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "created_by_id",
    "is_default",
    "deleted_at",
    "created_at",
    "updated_at",
];

/// Project pod cap (`models.py:68`).
pub const MAX_PER_PROJECT: usize = 20;
/// `name` bound (`models.py:81`, `max_length=128`).
pub const NAME_MAX_LENGTH: usize = 128;
/// `description` bound (`models.py:82`, `max_length=512`).
pub const DESCRIPTION_MAX_LENGTH: usize = 512;

/// `description` Django-side default (`models.py:82`, `default=""`).
pub const DEFAULT_DESCRIPTION: &str = "";
/// `is_default` Django-side default (`models.py:90`,
/// `default=False`).
pub const DEFAULT_IS_DEFAULT: bool = false;

/// `workspace` FK: `CASCADE` (`models.py:71-75`).
pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `project` FK: `CASCADE` (`models.py:76-80`).
pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `created_by` FK: `SET_NULL`, nullable (`models.py:83-89`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Unique pod name per project while active (`models.py:105-109`).
pub const UNIQUE_NAME_PER_PROJECT: &str = "pod_unique_name_per_project_when_active";
/// Fields of [`UNIQUE_NAME_PER_PROJECT`], Django field names.
pub const UNIQUE_NAME_PER_PROJECT_FIELDS: &[&str] = &["project", "name"];
/// Condition of [`UNIQUE_NAME_PER_PROJECT`] as SQL
/// (`models.py:107`, `Q(deleted_at__isnull=True)`).
pub const UNIQUE_NAME_PER_PROJECT_CONDITION_SQL: &str = "\"pod\".\"deleted_at\" IS NULL";

/// Exactly one active default pod per project (`models.py:113-117`).
pub const ONE_DEFAULT_PER_PROJECT: &str = "pod_one_default_per_project_when_active";
/// Fields of [`ONE_DEFAULT_PER_PROJECT`], Django field names.
pub const ONE_DEFAULT_PER_PROJECT_FIELDS: &[&str] = &["project"];
/// Condition of [`ONE_DEFAULT_PER_PROJECT`] as SQL
/// (`models.py:115`,
/// `Q(is_default=True) & Q(deleted_at__isnull=True)`).
pub const ONE_DEFAULT_PER_PROJECT_CONDITION_SQL: &str =
    "\"pod\".\"is_default\" AND \"pod\".\"deleted_at\" IS NULL";

/// Project + default lookup index (`models.py:120`).
pub const PROJECT_IS_DEFAULT_INDEX: &str = "pod_project_is_def_idx";
/// Fields of [`PROJECT_IS_DEFAULT_INDEX`], Django field names.
pub const PROJECT_IS_DEFAULT_INDEX_FIELDS: &[&str] = &["project", "is_default"];
/// Workspace + default lookup index (`models.py:121`).
pub const WORKSPACE_IS_DEFAULT_INDEX: &str = "pod_workspc_is_def_idx";
/// Fields of [`WORKSPACE_IS_DEFAULT_INDEX`], Django field names.
pub const WORKSPACE_IS_DEFAULT_INDEX_FIELDS: &[&str] = &["workspace", "is_default"];

/// `Pod.objects.all()`: the default manager excludes soft-deleted
/// pods (`PodManager.get_queryset`, `models.py:48-49`) and carries
/// `Meta.ordering`. Byte-identical to the recorded Django SQL
/// (fixture `pod_manager_sql.objects_all`).
pub const OBJECTS_ALL_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE \"pod\".\"deleted_at\" IS NULL ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC";

/// `Pod.all_objects.all()`: the plain manager includes tombstones
/// (`models.py:96`), same ordering. Byte-identical to the recorded
/// Django SQL (fixture `pod_manager_sql.all_objects_all`).
pub const ALL_OBJECTS_ALL_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC";

/// `Pod.default_for_project_id` (`models.py:174-176`):
/// `objects.filter(project_id=…, is_default=True).first()` — the
/// soft-delete scope applies, so a tombstoned default is invisible
/// here while `all_objects` still sees it. Django's `%s` renders as
/// `$1`; `.first()` executes with `LIMIT 1` (`fetch_optional`
/// below), which the recorded `str(qs.query)` omits.
pub const DEFAULT_FOR_PROJECT_ID_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"is_default\" AND \"pod\".\"project_id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1";

/// `pod.workspace must match pod.project.workspace`
/// (`models.py:143,158`): the `ValidationError({"workspace": …})`
/// both `clean` and `save` raise when the denormalised `workspace`
/// disagrees with `project.workspace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("pod.workspace must match pod.project.workspace")]
pub struct PodWorkspaceError;

impl PodWorkspaceError {
    /// The `ValidationError` dict key (`models.py:142,157`).
    pub const FIELD: &str = "workspace";
}

/// Shared body of `Pod.clean` (`models.py:127-143`) and `Pod.save`
/// (`models.py:145-159`): `save` duplicates the logic because
/// Django's `save()` never invokes `clean()`, and no Pod call site
/// runs `full_clean` (`models.py:148-150`).
///
/// * `project_workspace_id` is `None` (pod carries no `project_id`):
///   both skip — the workspace passes through untouched.
/// * `workspace_id` is `None`: autofill from the project.
/// * otherwise the two must be equal, else [`PodWorkspaceError`].
///
/// The caller fetches the project row first: Python reads
/// `self.project.workspace_id`, which raises `Project.DoesNotExist`
/// when `project_id` names a missing project.
pub fn resolve_workspace_id(
    workspace_id: Option<uuid::Uuid>,
    project_workspace_id: Option<uuid::Uuid>,
) -> Result<Option<uuid::Uuid>, PodWorkspaceError> {
    match (workspace_id, project_workspace_id) {
        (_, None) => Ok(workspace_id),
        (None, Some(project_workspace_id)) => Ok(Some(project_workspace_id)),
        (Some(workspace_id), Some(project_workspace_id))
            if workspace_id == project_workspace_id =>
        {
            Ok(Some(workspace_id))
        }
        (Some(_), Some(_)) => Err(PodWorkspaceError),
    }
}

/// One project pod row, declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pod {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub created_by_id: Option<uuid::Uuid>,
    pub is_default: bool,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Map a `default_for_project_id` row into [`Pod`], one `try_get`
/// per column in [`COLUMNS`] order (manual mapping follows the
/// `v1_cli_auth` precedent; there is no `FromRow` derive in this
/// crate).
pub fn pod_from_row(row: &sqlx::postgres::PgRow) -> Result<Pod, sqlx::Error> {
    use sqlx::Row;
    Ok(Pod {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        project_id: row.try_get("project_id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        created_by_id: row.try_get("created_by_id")?,
        is_default: row.try_get("is_default")?,
        deleted_at: row.try_get("deleted_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// `Pod.default_for_project_id` (`models.py:174-176`): the active
/// default pod for a project, or `None` (transient pre-signal
/// window, or the default was soft-deleted without a promoted
/// replacement). Callers pick the pool and pass an executor in;
/// every executor is generic over `sqlx::Executor`.
pub async fn default_for_project_id<'e, E>(
    ex: E,
    project_id: uuid::Uuid,
) -> Result<Option<Pod>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(DEFAULT_FOR_PROJECT_ID_SQL)
        .bind(project_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| pod_from_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Default-pod creation (`post_save(Project)`, PIDASHCONV-1297)
// ---------------------------------------------------------------------------

/// Auto-created pod description, verbatim (`signals.py:54`). This is
/// the receiver's fixed text, not the model field default
/// ([`DEFAULT_DESCRIPTION`]).
pub const DEFAULT_POD_DESCRIPTION: &str = "Auto-created default pod. Add tier pods anytime.";

/// The project facts the handler reads (`signals.py:32-58`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub identifier: String,
    /// The `post_save` `created` flag: resaves are a no-op.
    pub created: bool,
    /// `created_by` fallback chain (`signals.py:56-57`): project lead,
    /// else default assignee, else `None` (NULL).
    pub project_lead: Option<uuid::Uuid>,
    pub default_assignee: Option<uuid::Uuid>,
}

/// Default pod name: `{identifier}_pod_1` (`signals.py:48`).
pub fn default_pod_name(identifier: &str) -> String {
    format!("{identifier}_pod_1")
}

/// `created_by` fallback (`signals.py:56-57`).
pub fn pod_created_by(project: &NewProject) -> Option<uuid::Uuid> {
    project.project_lead.or(project.default_assignee)
}

/// Exists-guard (`Pod.objects.filter(project=...).exists()`,
/// `signals.py:45`): the default manager excludes soft-deleted pods.
/// Param: `$1` the project id.
pub fn pod_exists_for_project_sql() -> String {
    r#"SELECT EXISTS(SELECT 1 FROM "pod" WHERE ("pod"."project_id" = $1 AND "pod"."deleted_at" IS NULL))"#.to_owned()
}

/// Default-pod `INSERT` (`Pod.objects.create(...)`, `signals.py:50-58`):
/// every column is bound (the live tables carry no `column_default`).
/// Params: `$1` id, `$2` workspace, `$3` project, `$4` name,
/// `$5` description, `$6` created_by (nullable), `$7` is_default,
/// `$8` deleted_at (NULL), `$9` created_at, `$10` updated_at.
pub fn insert_default_pod_sql() -> String {
    r#"INSERT INTO "pod" ("id", "workspace_id", "project_id", "name", "description", "created_by_id", "is_default", "deleted_at", "created_at", "updated_at") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#.to_owned()
}

/// Ensure a fresh project has a default pod (`signals.py:32-66`):
/// no-op unless `created`, no-op when any active pod exists, else
/// insert `{identifier}_pod_1`. Returns the new pod id, or `None`
/// when the guard hit or the insert was swallowed.
///
/// The `try` placement is Django's: the exists-guard sits outside
/// the `try` (`signals.py:45`), so its storage failure propagates;
/// only the `create` is swallowed and logged (`signals.py:59-66`) —
/// pod creation must never block project creation.
pub async fn ensure_default_pod(
    pool: &sqlx::PgPool,
    project: &NewProject,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    if !project.created {
        return Ok(None);
    }
    let exists: bool = sqlx::query_scalar(&pod_exists_for_project_sql())
        .bind(project.id)
        .fetch_one(pool)
        .await?;
    if exists {
        return Ok(None);
    }
    // `auto_now_add` / `auto_now` sample separately on save: two
    // timestamps, like the source.
    let created_at = chrono::Utc::now();
    let updated_at = chrono::Utc::now();
    let pod_id = uuid::Uuid::new_v4();
    if let Err(error) = sqlx::query(&insert_default_pod_sql())
        .bind(pod_id)
        .bind(project.workspace_id)
        .bind(project.id)
        .bind(default_pod_name(&project.identifier))
        .bind(DEFAULT_POD_DESCRIPTION)
        .bind(pod_created_by(project))
        .bind(true)
        .bind(None::<chrono::DateTime<chrono::Utc>>)
        .bind(created_at)
        .bind(updated_at)
        .execute(pool)
        .await
    {
        tracing::error!(
            %error,
            project_id = %project.id,
            "runner.signals: failed to auto-create default pod for project",
        );
        return Ok(None);
    }
    Ok(Some(pod_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn pod_fixture() -> serde_json::Value {
        ts::model(&ts::fx02(), "Pod").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&pod_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = pod_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let max = MAX_PER_PROJECT;
        assert_eq!(max, 20, "MAX_PER_PROJECT (models.py:68)");
    }

    #[test]
    fn constraints_match_fixture() {
        let m = pod_fixture();
        let name = ts::constraint(&m, UNIQUE_NAME_PER_PROJECT);
        assert_eq!(name["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(
            name["fields"],
            serde_json::json!(UNIQUE_NAME_PER_PROJECT_FIELDS)
        );
        assert_eq!(
            name["condition"].as_str(),
            Some("(AND: ('deleted_at__isnull', True))")
        );
        assert_eq!(
            UNIQUE_NAME_PER_PROJECT_CONDITION_SQL,
            "\"pod\".\"deleted_at\" IS NULL"
        );
        let one = ts::constraint(&m, ONE_DEFAULT_PER_PROJECT);
        assert_eq!(one["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(
            one["fields"],
            serde_json::json!(ONE_DEFAULT_PER_PROJECT_FIELDS)
        );
        assert_eq!(
            one["condition"].as_str(),
            Some("(AND: ('is_default', True), ('deleted_at__isnull', True))")
        );
        assert_eq!(
            ONE_DEFAULT_PER_PROJECT_CONDITION_SQL,
            "\"pod\".\"is_default\" AND \"pod\".\"deleted_at\" IS NULL"
        );
    }

    #[test]
    fn indexes_match_fixture() {
        let m = pod_fixture();
        let project = ts::index(&m, PROJECT_IS_DEFAULT_INDEX);
        assert_eq!(
            project["fields"],
            serde_json::json!(PROJECT_IS_DEFAULT_INDEX_FIELDS)
        );
        let workspace = ts::index(&m, WORKSPACE_IS_DEFAULT_INDEX);
        assert_eq!(
            workspace["fields"],
            serde_json::json!(WORKSPACE_IS_DEFAULT_INDEX_FIELDS)
        );
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = pod_fixture();
        let id = ts::field(&m, "id");
        assert_eq!(id["type"].as_str(), Some("UUIDField"));
        assert_eq!(id["db_type"].as_str(), Some("uuid"));
        assert_eq!(id["default"].as_str(), Some("callable:<uuid4>"));
        assert_eq!(id["unique"].as_bool(), Some(true));
        for (field, to, on_delete) in [
            ("workspace", "workspaces", "CASCADE"),
            ("project", "projects", "CASCADE"),
        ] {
            let f = ts::field(&m, field);
            assert_eq!(f["type"].as_str(), Some("ForeignKey"));
            assert_eq!(f["db_type"].as_str(), Some("uuid"));
            assert_eq!(f["null"].as_bool(), Some(false));
            assert_eq!(f["rel"]["to"].as_str(), Some(to));
            assert_eq!(f["rel"]["on_delete"].as_str(), Some(on_delete));
        }
        assert_eq!(WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(CREATED_BY_ON_DELETE, OnDelete::SetNull);
        let created_by = ts::field(&m, "created_by");
        assert_eq!(created_by["null"].as_bool(), Some(true));
        assert_eq!(created_by["rel"]["on_delete"].as_str(), Some("SET_NULL"));
        let name = ts::field(&m, "name");
        assert_eq!(name["db_type"].as_str(), Some("varchar(128)"));
        assert_eq!(name["max_length"].as_u64(), Some(128));
        assert_eq!(NAME_MAX_LENGTH, 128);
        let description = ts::field(&m, "description");
        assert_eq!(description["db_type"].as_str(), Some("varchar(512)"));
        assert_eq!(description["max_length"].as_u64(), Some(512));
        assert_eq!(description["default"].as_str(), Some(DEFAULT_DESCRIPTION));
        assert_eq!(DESCRIPTION_MAX_LENGTH, 512);
        let is_default = ts::field(&m, "is_default");
        assert_eq!(is_default["type"].as_str(), Some("BooleanField"));
        assert_eq!(is_default["default"].as_bool(), Some(DEFAULT_IS_DEFAULT));
        let deleted_at = ts::field(&m, "deleted_at");
        assert_eq!(deleted_at["null"].as_bool(), Some(true));
        assert_eq!(deleted_at["db_index"].as_bool(), Some(true));
        let created_at = ts::field(&m, "created_at");
        assert_eq!(created_at["auto_now_add"].as_bool(), Some(true));
        let updated_at = ts::field(&m, "updated_at");
        assert_eq!(updated_at["auto_now"].as_bool(), Some(true));
    }

    #[test]
    fn manager_sql_is_byte_identical_to_django() {
        let v = ts::fx02();
        let objects = OBJECTS_ALL_SQL;
        assert_eq!(
            objects,
            v["pod_manager_sql"]["objects_all"].as_str().expect("sql")
        );
        let all_objects = ALL_OBJECTS_ALL_SQL;
        assert_eq!(
            all_objects,
            v["pod_manager_sql"]["all_objects_all"]
                .as_str()
                .expect("sql")
        );
        assert!(
            objects.contains("WHERE \"pod\".\"deleted_at\" IS NULL"),
            "default manager excludes tombstones"
        );
        assert!(
            !all_objects.contains("WHERE"),
            "all_objects carries no filter"
        );
    }

    #[test]
    fn default_lookup_sql_shape_matches_django() {
        let v = ts::fx02();
        let django = v["default_for_project_id"]["sql"].as_str().expect("sql");
        let sql = DEFAULT_FOR_PROJECT_ID_SQL;
        // Same projection, same order: every fixture column quoted.
        let mut cursor = 0;
        for col in COLUMNS {
            let quoted = format!("\"pod\".\"{col}\"");
            let pos = sql[cursor..]
                .find(quoted.as_str())
                .unwrap_or_else(|| panic!("projection carries {quoted}"));
            cursor += pos + quoted.len();
            assert!(django.contains(&quoted), "Django projects {quoted}");
        }
        // Same filter terms in the same order, `%s` param as `$1`.
        assert!(django.contains("\"pod\".\"deleted_at\" IS NULL"));
        assert!(django.contains("\"pod\".\"is_default\""));
        assert!(django.contains("\"pod\".\"project_id\" = "));
        for term in [
            "\"pod\".\"deleted_at\" IS NULL",
            "\"pod\".\"is_default\"",
            "\"pod\".\"project_id\" = $1",
        ] {
            let pos = sql[cursor..]
                .find(term)
                .unwrap_or_else(|| panic!("WHERE carries {term}"));
            cursor += pos + term.len();
        }
        assert!(sql.contains("ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC"));
        assert!(sql.ends_with("LIMIT 1"), ".first() executes LIMIT 1");
        // Recorded live semantics the lookup implements: the default
        // is returned while active; after soft-delete `objects`
        // excludes it while `all_objects` still sees it.
        let live = &v["default_for_project_id"]["live"];
        assert_eq!(live["returned"]["is_default"].as_bool(), Some(true));
        assert_eq!(live["objects_excludes"].as_bool(), Some(true));
        assert_eq!(live["all_objects_still_sees"].as_bool(), Some(true));
    }

    #[test]
    fn clean_and_save_share_autofill_and_mismatch() {
        let v = ts::fx02();
        let clean_save = &v["pod_clean_save"];
        let project_ws: uuid::Uuid = clean_save["project_workspace_id"]
            .as_str()
            .expect("uuid")
            .parse()
            .expect("parses");
        // clean() autofills the project workspace when omitted.
        assert_eq!(
            clean_save["clean_autofill_workspace_id"].as_str(),
            clean_save["project_workspace_id"].as_str()
        );
        assert_eq!(
            resolve_workspace_id(None, Some(project_ws)),
            Ok(Some(project_ws))
        );
        // save() autofills identically.
        assert_eq!(
            clean_save["save_autofill"]["workspace_id"].as_str(),
            clean_save["save_autofill"]["project_workspace_id"].as_str()
        );
        // A matching workspace passes through.
        assert_eq!(
            resolve_workspace_id(Some(project_ws), Some(project_ws)),
            Ok(Some(project_ws))
        );
        // A mismatch raises the exact ValidationError both paths raise.
        let other = uuid::Uuid::nil();
        assert_ne!(other, project_ws);
        let err = resolve_workspace_id(Some(other), Some(project_ws)).expect_err("mismatch");
        assert_eq!(PodWorkspaceError::FIELD, "workspace");
        let message = err.to_string();
        assert_eq!(
            message,
            clean_save["clean_mismatch"]["dict"]["workspace"][0]
                .as_str()
                .expect("message")
        );
        assert_eq!(
            message,
            clean_save["save_mismatch"]["messages"][0]
                .as_str()
                .expect("message")
        );
        // No project: both clean (:136) and save (:151) skip.
        assert_eq!(resolve_workspace_id(Some(other), None), Ok(Some(other)));
        assert_eq!(resolve_workspace_id(None, None), Ok(None));
    }

    fn fx08() -> serde_json::Value {
        let raw =
            include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
        serde_json::from_str(raw).expect("valid fixture")
    }

    #[test]
    fn default_pod_name_and_fallback_match_source() {
        assert_eq!(default_pod_name("SIG"), "SIG_pod_1");
        let lead = uuid::Uuid::nil();
        let assignee = uuid::Uuid::max();
        let project = NewProject {
            id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            identifier: "SIG".to_owned(),
            created: true,
            project_lead: Some(lead),
            default_assignee: Some(assignee),
        };
        assert_eq!(pod_created_by(&project), Some(lead));
        let project = NewProject {
            project_lead: None,
            ..project
        };
        assert_eq!(pod_created_by(&project), Some(assignee));
        let project = NewProject {
            default_assignee: None,
            ..project
        };
        assert_eq!(pod_created_by(&project), None);
    }

    #[test]
    fn default_pod_row_shape_matches_fx08() {
        let fx = fx08();
        let row = &fx["signals"]["auto"]["row"];
        assert_eq!(row["name"], "SIG_pod_1");
        assert_eq!(default_pod_name("SIG"), row["name"].as_str().expect("name"));
        assert_eq!(row["is_default"], true);
        assert_eq!(row["description"], DEFAULT_POD_DESCRIPTION);
        assert_eq!(
            DEFAULT_POD_DESCRIPTION,
            "Auto-created default pod. Add tier pods anytime."
        );
        assert!(row["created_by"].is_null());
        assert!(pod_exists_for_project_sql().contains(r#""pod"."deleted_at" IS NULL"#));
        // The insert binds every column (no live `column_default`).
        assert!(insert_default_pod_sql().contains("$10"));
    }
}
