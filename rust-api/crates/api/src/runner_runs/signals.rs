//! Default-pod signal (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `create_default_pod_for_new_project`
//! (`apps/api/pi_dash/runner/signals.py:32-66`, the `@receiver(post_save,
//! sender=Project)` handler) as an explicit post-commit fn
//! ([`create_default_pod_for_new_project`]): when a project is created,
//! ensure it has a default pod, idempotently. Per the transactions
//! decision, signals become explicit calls — the project-creation owner
//! (D-19/D-25 area) wires this call on its create path; that wiring
//! belongs to that domain's split, not this issue.
//!
//! Body-note fix: the issue text calls this unit
//! `ensure_default_pod_for_project`; the real identifier is
//! `create_default_pod_for_new_project` (split-review correction).

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Auto-created pod description, verbatim (`signals.py:54`).
pub const DEFAULT_POD_DESCRIPTION: &str = "Auto-created default pod. Add tier pods anytime.";

/// The project facts the handler reads (`signals.py:32-58`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub identifier: String,
    /// The `post_save` `created` flag: resaves are a no-op.
    pub created: bool,
    /// `created_by` fallback chain (`signals.py:56-57`): project lead,
    /// else default assignee, else `None` (NULL).
    pub project_lead: Option<Uuid>,
    pub default_assignee: Option<Uuid>,
}

/// Default pod name: `{identifier}_pod_1` (`signals.py:48`).
pub fn default_pod_name(identifier: &str) -> String {
    format!("{identifier}_pod_1")
}

/// `created_by` fallback (`signals.py:56-57`).
pub fn pod_created_by(project: &NewProject) -> Option<Uuid> {
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
/// insert `{identifier}_pod_1`. ANY failure is swallowed and logged —
/// pod creation must never block project creation (`signals.py:59-66`).
pub async fn create_default_pod_for_new_project(pool: &PgPool, project: &NewProject) {
    if !project.created {
        return;
    }
    if let Err(error) = ensure_default_pod(pool, project).await {
        tracing::error!(
            %error,
            project_id = %project.id,
            "runner.signals: failed to auto-create default pod for project",
        );
    }
}

/// The fallible core: exists-guard, then insert.
async fn ensure_default_pod(pool: &PgPool, project: &NewProject) -> Result<(), sqlx::Error> {
    let exists: bool = sqlx::query_scalar(&pod_exists_for_project_sql())
        .bind(project.id)
        .fetch_one(pool)
        .await?;
    if exists {
        return Ok(());
    }
    // `auto_now_add` / `auto_now` sample separately on save: two
    // timestamps, like the source.
    let created_at = Utc::now();
    let updated_at = Utc::now();
    sqlx::query(&insert_default_pod_sql())
        .bind(Uuid::new_v4())
        .bind(project.workspace_id)
        .bind(project.id)
        .bind(default_pod_name(&project.identifier))
        .bind(DEFAULT_POD_DESCRIPTION)
        .bind(pod_created_by(project))
        .bind(true)
        .bind(None::<DateTime<Utc>>)
        .bind(created_at)
        .bind(updated_at)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fx08() -> Value {
        let raw =
            include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
        serde_json::from_str(raw).expect("valid fixture")
    }

    #[test]
    fn name_and_fallback_match_source() {
        assert_eq!(default_pod_name("SIG"), "SIG_pod_1");
        let lead = Uuid::nil();
        let assignee = Uuid::max();
        let project = NewProject {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
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
    fn row_shape_matches_fx08() {
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
