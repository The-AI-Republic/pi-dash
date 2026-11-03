//! Project / state row reads.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05 pattern,
//! PIDASHCONV-1661); the handlers call these and map
//! [`Error`](super::Error) onto their denial type. The MCP-semantics
//! variants share the v1 SQL builders and differ only where the MCP
//! queryset itself differs (BUG-944-1 member scope, explicit ordering).

use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// Full payload rows for scope-resolved project ids: the plain
/// `Project.objects.get` shape the v1 PATCH before-image reads
/// (`api::v1_projects::handlers_project`, `SELECT * ... WHERE id = $1
/// AND deleted_at IS NULL`), generalized to `= ANY($1)` so one round
/// trip serves many ids. The ids arrive pre-scoped; the `deleted_at`
/// guard keeps a row deleted in the race window out (it then reports
/// as vanished). Empty input returns empty without querying.
pub async fn fetch_project_rows(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<Vec<sqlx::postgres::PgRow>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows =
        sqlx::query(r#"SELECT * FROM "projects" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#)
            .bind(ids)
            .fetch_all(pool)
            .await
            .map_err(|error| DbError::new("project-rows", error))?;
    Ok(rows)
}

/// `list_states` rows (`pi_dash_cloud/mcp/tools.py:777-789`): live
/// non-triage states of the project in the slug where the caller holds
/// an active project membership, the project unarchived,
/// sequence-ordered, distinct. Shares the v1 list builder
/// (`pidash_db::v1_projects::queries_stateest::state_list_sql_with`)
/// with the `project_members` soft-delete guard off (BUG-944-1: only
/// the base model's manager applies across the MCP traversal, so a
/// soft-deleted membership row still authorizes this read); every
/// other clause and the bind order are the v1 scope. Ordering is in
/// SQL (`.order_by("sequence")`), unlike the v1 handler which sorts
/// the shared builder's unordered rows itself. Rows stay raw: the
/// caller decodes and renders them.
pub async fn fetch_state_columns(
    pool: &PgPool,
    workspace_slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, Error> {
    let sql = format!(
        "{} ORDER BY \"states\".\"sequence\" ASC",
        pidash_db::v1_projects::queries_stateest::state_list_sql_with(false)
    );
    let rows = sqlx::query(&sql)
        .bind(workspace_slug)
        .bind(project_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("state-columns", error))?;
    Ok(rows)
}

#[cfg(test)]
mod live {
    use sqlx::Row;

    use super::*;
    use crate::read::test_support::{pool, seed_world};

    async fn seed_member(pool: &PgPool, world: &crate::read::test_support::World) {
        sqlx::query(
            "INSERT INTO project_members (id, role, project_id, workspace_id, member_id, \
             view_props, default_props, sort_order, preferences, is_active, created_at, updated_at) \
             VALUES ($1, 20, $2, $3, $4, '{}', '{}', 65535, '{}', true, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.project_id)
        .bind(world.workspace_id)
        .bind(world.user_id)
        .execute(pool)
        .await
        .expect("seed member");
    }

    #[tokio::test]
    async fn project_rows_hit_miss_and_empty() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        let rows = fetch_project_rows(&db, &[world.project_id])
            .await
            .expect("hit");
        assert_eq!(rows.len(), 1);
        let name: String = rows[0].try_get("name").expect("name");
        assert_eq!(name, "Seed Project");
        let missing = fetch_project_rows(&db, &[Uuid::new_v4()])
            .await
            .expect("miss");
        assert!(missing.is_empty());
        let empty = fetch_project_rows(&db, &[]).await.expect("empty");
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn state_columns_hit_triage_excluded_member_scope() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        seed_member(&db, &world).await;
        // A triage state the manager scope must hide.
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
             VALUES ($1, now(), now(), 'Triage', '', '#fff', 'triage', $2, $3, 65000, \
             'triage', false, true)",
        )
        .bind(Uuid::new_v4())
        .bind(world.project_id)
        .bind(world.workspace_id)
        .execute(&db)
        .await
        .expect("seed triage");
        let rows = fetch_state_columns(&db, &world.workspace_slug, world.project_id, world.user_id)
            .await
            .expect("hit");
        assert_eq!(rows.len(), 1);
        let name: String = rows[0].try_get("name").expect("name");
        assert_eq!(name, "Seed Backlog");
        // BUG-944-1: a soft-deleted membership row still authorizes the
        // MCP read (no `project_members.deleted_at` guard).
        sqlx::query("UPDATE project_members SET deleted_at = now() WHERE member_id = $1")
            .bind(world.user_id)
            .execute(&db)
            .await
            .expect("soft-delete member");
        let rows = fetch_state_columns(&db, &world.workspace_slug, world.project_id, world.user_id)
            .await
            .expect("hit after member soft-delete");
        assert_eq!(rows.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_columns_sql_is_mcp_scope_sequence_ordered() {
        // The service SQL is the shared v1 list builder with the member
        // deleted guard off (BUG-944-1) plus the MCP `.order_by`.
        let sql = format!(
            "{} ORDER BY \"states\".\"sequence\" ASC",
            pidash_db::v1_projects::queries_stateest::state_list_sql_with(false)
        );
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains(r#""states"."is_triage" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> 'triage'"#), "{sql}");
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        assert!(!sql.contains(r#""project_members"."deleted_at""#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "states"."sequence" ASC"#),
            "{sql}"
        );
    }
}
