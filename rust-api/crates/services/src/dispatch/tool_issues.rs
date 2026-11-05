//! Cloud-agent tool issue reads through SeaORM: the run's bound issue
//! (`tools.py` `_bound_issue`), `pidash_get_project_issue` (`:243-251`) and
//! `pidash_search_project_issues` (`:227-241`).
//!
//! These replace hand-written joins (`BOUND_ISSUE_SQL`,
//! `GET_PROJECT_ISSUE_SQL`, `SEARCH_ISSUES_SQL`) that selected both
//! `issues.name` and `states.name`. sqlx resolves a duplicated output name to
//! the LAST column, so the readers' `try_get("name")` returned the state's
//! name as the issue title, and failed outright for an issue without a state.
//! Here every joined column has its own alias and rows decode by name into
//! [`ToolIssueRow`].

use pidash_db::entity::{issues, projects, states};
use sea_orm::sea_query::{BinOper, Expr, Func};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, JoinType,
    QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait, Select,
};
use uuid::Uuid;

/// One issue as the agent tools render it: the issue's own columns, its
/// state's name and group (NULL without a state) and its project identifier.
/// Decodes by name both through SeaORM (`FromQueryResult`) and on the audit
/// transaction's sqlx path (`sqlx::FromRow`, [`bound_issue_sql`]).
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult, sqlx::FromRow)]
pub struct ToolIssueRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub sequence_id: i32,
    pub name: String,
    pub priority: String,
    pub description_stripped: Option<String>,
    pub workpad: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub project_identifier: String,
}

/// The run's bound issue by id. Like `_bound_issue`, there is no
/// soft-delete filter: a deleted bound issue still resolves.
pub async fn bound_issue(
    db: &DatabaseConnection,
    issue_id: Uuid,
) -> Result<Option<ToolIssueRow>, DbErr> {
    issue_query()
        .filter(issues::Column::Id.eq(issue_id))
        .into_model::<ToolIssueRow>()
        .one(db)
        .await
}

/// The [`bound_issue`] statement as SQL text with one `$1` (the issue id),
/// for the agent tools' audit transaction: that is an `sqlx::Transaction`,
/// which a SeaORM call cannot join, so the caller runs this text on it.
/// SeaORM still builds the statement (once), so its output names are unique.
pub fn bound_issue_sql() -> &'static str {
    static SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        issue_query()
            .filter(issues::Column::Id.eq(Uuid::nil()))
            .build(sea_orm::DbBackend::Postgres)
            .sql
    });
    &SQL
}

/// The search hit: what `_issue_data(issue)` (no detail) reads.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct ToolIssueBrief {
    pub id: Uuid,
    pub sequence_id: i32,
    pub name: String,
    pub priority: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub project_identifier: String,
}

/// `Issue.issue_objects.get(id=..., project_id=...)`: a live issue in the
/// run's project.
pub async fn project_issue(
    db: &DatabaseConnection,
    issue_id: Uuid,
    project_id: Uuid,
) -> Result<Option<ToolIssueRow>, DbErr> {
    issue_query()
        .filter(issues::Column::Id.eq(issue_id))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(issues::Column::DeletedAt.is_null())
        .into_model::<ToolIssueRow>()
        .one(db)
        .await
}

/// `name__icontains=q | description_stripped__icontains=q`, newest first,
/// at most `limit`. `escaped` is the query with LIKE wildcards already
/// escaped (`escape_icontains`), as Django does for `icontains`.
pub async fn search_project_issues(
    db: &DatabaseConnection,
    project_id: Uuid,
    escaped: &str,
    limit: u64,
) -> Result<Vec<ToolIssueBrief>, DbErr> {
    search_query(project_id, escaped, limit)
        .into_model::<ToolIssueBrief>()
        .all(db)
        .await
}

/// The detail select: issue columns, `INNER JOIN projects`, `LEFT JOIN
/// states`, every joined column under its own alias.
fn issue_query() -> Select<issues::Entity> {
    joined(&[
        issues::Column::Id,
        issues::Column::WorkspaceId,
        issues::Column::ProjectId,
        issues::Column::SequenceId,
        issues::Column::Name,
        issues::Column::Priority,
        issues::Column::DescriptionStripped,
        issues::Column::Workpad,
    ])
}

fn joined(columns: &[issues::Column]) -> Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .columns(columns.iter().copied())
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
        .column_as(projects::Column::Identifier, "project_identifier")
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
}

/// Django's queryset was `.distinct()`, but with the PK selected and both
/// joins to-one no row can repeat, so the search drops `DISTINCT` (and the
/// `created_at` it forced into the select list) and selects only the
/// brief's columns, not the wide description and workpad.
fn search_query(project_id: Uuid, escaped: &str, limit: u64) -> Select<issues::Entity> {
    // `UPPER(col) LIKE UPPER('%' || q || '%')`: Django's icontains on
    // Postgres. The pattern is upper-cased in SQL, like the column, so both
    // sides use the database's case mapping.
    let pattern = format!("%{escaped}%");
    let contains = |column: issues::Column| {
        Expr::expr(Func::upper(Expr::col((issues::Entity, column))))
            .binary(BinOper::Like, Func::upper(Expr::val(pattern.clone())))
    };
    joined(&[
        issues::Column::Id,
        issues::Column::SequenceId,
        issues::Column::Name,
        issues::Column::Priority,
    ])
    .filter(issues::Column::ProjectId.eq(project_id))
    .filter(issues::Column::DeletedAt.is_null())
    .filter(
        Condition::any()
            .add(contains(issues::Column::Name))
            .add(contains(issues::Column::DescriptionStripped)),
    )
    .order_by_desc(issues::Column::CreatedAt)
    .limit(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn names(sql: &str) -> Vec<String> {
        let select = sql
            .strip_prefix("SELECT ")
            .and_then(|rest| rest.split(" FROM ").next())
            .expect("select list");
        select
            .split(", ")
            .map(|column| {
                column
                    .rsplit(" AS ")
                    .next()
                    .and_then(|name| name.rsplit('.').next())
                    .expect("name")
                    .trim_matches('"')
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn every_output_column_has_its_own_name() {
        for sql in [
            issue_query().build(DbBackend::Postgres).to_string(),
            search_query(Uuid::nil(), "x", 5)
                .build(DbBackend::Postgres)
                .to_string(),
        ] {
            let mut seen = names(&sql);
            let total = seen.len();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), total, "duplicate output column: {sql}");
            assert!(seen.contains(&"name".to_owned()), "{sql}");
            assert!(seen.contains(&"state_name".to_owned()), "{sql}");
        }
    }

    #[test]
    fn bound_issue_sql_binds_the_id_as_its_only_parameter() {
        let sql = bound_issue_sql();
        assert!(sql.ends_with(r#"WHERE "issues"."id" = $1"#), "{sql}");
        assert!(!sql.contains("$2"), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    /// The bug this module fixes: the issue's own name comes back as `name`
    /// (the old `try_get("name")` returned the state's), and an issue with
    /// no state reads instead of failing on a NULL state name.
    #[tokio::test]
    async fn live_issue_name_is_not_the_state_name() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let world = crate::read::test_support::seed_world(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);

        let bound = bound_issue(&db, world.issue_id)
            .await
            .expect("read")
            .expect("found");
        assert_eq!(bound.name, "Seed issue");
        assert_eq!(bound.state_name.as_deref(), Some("Seed Backlog"));
        assert_eq!(bound.state_group.as_deref(), Some("backlog"));
        assert_eq!(bound.project_identifier, world.project_identifier);

        let in_project = project_issue(&db, world.issue_id, world.project_id)
            .await
            .expect("read")
            .expect("found");
        assert_eq!(in_project, bound);
        assert!(project_issue(&db, world.issue_id, Uuid::new_v4())
            .await
            .expect("read")
            .is_none());

        let hits = search_project_issues(&db, world.project_id, "seed ISS", 10)
            .await
            .expect("search");
        assert_eq!(
            hits.iter().map(|hit| hit.id).collect::<Vec<_>>(),
            vec![world.issue_id]
        );
        assert_eq!(hits[0].name, "Seed issue");

        sqlx::query("UPDATE issues SET state_id = NULL WHERE id = $1")
            .bind(world.issue_id)
            .execute(&pool)
            .await
            .expect("drop the state");
        let stateless = bound_issue(&db, world.issue_id)
            .await
            .expect("read")
            .expect("found");
        assert_eq!(stateless.name, "Seed issue");
        assert_eq!((stateless.state_name, stateless.state_group), (None, None));
        let decoded: ToolIssueRow = sqlx::query_as(bound_issue_sql())
            .bind(world.issue_id)
            .fetch_one(&pool)
            .await
            .expect("transaction-path statement");
        assert_eq!(decoded.name, "Seed issue");
    }

    #[test]
    fn joins_projects_inner_and_states_left() {
        let sql = issue_query().build(DbBackend::Postgres).to_string();
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "issues"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{sql}"
        );
    }

    #[test]
    fn search_is_icontains_on_name_or_description_newest_first() {
        let sql = search_query(Uuid::nil(), r"50\%", 5)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(!sql.contains("workpad"), "{sql}");
        assert!(
            sql.contains(r#"UPPER("issues"."name") LIKE UPPER('%50\%%')"#)
                || sql.contains(r#"UPPER("issues"."name") LIKE UPPER(E'%50\\%%')"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"UPPER("issues"."description_stripped") LIKE"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "issues"."created_at" DESC LIMIT 5"#),
            "{sql}"
        );
    }
}
