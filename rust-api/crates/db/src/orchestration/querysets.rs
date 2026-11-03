#![forbid(unsafe_code)]

//! Shared issue querysets + membership role facts (D-12 L2, stage 5).
//!
//! Ports `core/querysets.py` whole (`member_project_issues`, `:19-30`;
//! `user_issues_queryset`, `:33-52`) and the role-*fact* SQL of
//! `core/permissions.py` (`:28-58` plus the three `EXISTS` facts behind
//! `check_project_role`, `:73-119`).
//!
//! Decisions stay in the merged F-06 kernel
//! (`pidash_auth::permissions::membership`, which this crate cannot
//! import): [`fetch_workspace_role`] feeds `is_workspace_member` /
//! `is_workspace_admin` / `is_at_least_member` (each takes the fetched
//! role), and the three project-fact fetchers feed
//! `check_project_role`'s `ProjectRoleFacts`.
//!
//! Bind order is fixture order everywhere: `$1` the user id, `$2` the
//! workspace id/slug, then (`check_project_role` facts) `$3` the
//! project id and `$4+` the allowed roles.
//!
//! Fixture: `rust-api/fixtures/orchestration/fx02_reads/` (FX-ORCH-02:
//! `querysets.sql`, `querysets.rows.json`, `role_facts.sql`,
//! `role_facts.golden.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use sqlx::postgres::PgRow;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Issue querysets
// ---------------------------------------------------------------------------

/// `member_project_issues` (`core/querysets.py:19-30`): issues in the
/// workspace whose project the user actively belongs to. `$1` is the
/// user id, `$2` the workspace slug. Joins, predicate order and the
/// `DISTINCT (id, created_at)` projection follow the fixture
/// (`values_list` capture); rows come back newest-first. The
/// `Issue.issue_objects` manager scope (`db/models/issue.py:95-104`)
/// renders `.exclude(state__group='triage')` as
/// `NOT (states.group = 'triage' AND states.group IS NOT NULL)` —
/// kept verbatim. NULL-group subtlety: the inner `AND` is `FALSE`
/// (not `NULL`) for a stateless issue, so stateless issues are
/// *included* — the simpler `NOT (states.group = 'triage')` rewrite
/// would wrongly exclude them. The fixture pins this shape.
pub const MEMBER_PROJECT_ISSUES_SQL: &str =
    "SELECT DISTINCT issues.id, issues.created_at FROM issues \
    LEFT OUTER JOIN states ON (issues.state_id = states.id) \
    INNER JOIN projects ON (issues.project_id = projects.id) \
    INNER JOIN project_members ON (projects.id = project_members.project_id) \
    INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
    WHERE (issues.deleted_at IS NULL \
    AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
    AND NOT (issues.archived_at IS NOT NULL) \
    AND NOT (projects.archived_at IS NOT NULL) \
    AND NOT (issues.is_draft) \
    AND project_members.is_active \
    AND project_members.member_id = $1 \
    AND workspaces.slug = $2) \
    ORDER BY issues.created_at DESC";

/// `user_issues_queryset(..., scope="all")` (`:33-52`, `else`
/// branch): assigned OR created OR subscribed, inside the member
/// scope. The M2M traversals join with no `deleted_at` predicate — a
/// soft-deleted link still matches, as in Django.
pub const USER_ISSUES_ALL_SQL: &str = "SELECT DISTINCT issues.id, issues.created_at FROM issues \
    LEFT OUTER JOIN states ON (issues.state_id = states.id) \
    INNER JOIN projects ON (issues.project_id = projects.id) \
    INNER JOIN project_members ON (projects.id = project_members.project_id) \
    INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
    LEFT OUTER JOIN issue_assignees ON (issues.id = issue_assignees.issue_id) \
    LEFT OUTER JOIN issue_subscribers ON (issues.id = issue_subscribers.issue_id) \
    WHERE (issues.deleted_at IS NULL \
    AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
    AND NOT (issues.archived_at IS NOT NULL) \
    AND NOT (projects.archived_at IS NOT NULL) \
    AND NOT (issues.is_draft) \
    AND project_members.is_active \
    AND project_members.member_id = $1 \
    AND workspaces.slug = $2 \
    AND (issue_assignees.assignee_id = $1 OR issues.created_by_id = $1 OR issue_subscribers.subscriber_id = $1)) \
    ORDER BY issues.created_at DESC";

/// `user_issues_queryset(..., scope="assigned")`: the `assignees__id`
/// branch (inner join, as Django renders it).
pub const USER_ISSUES_ASSIGNED_SQL: &str =
    "SELECT DISTINCT issues.id, issues.created_at FROM issues \
    LEFT OUTER JOIN states ON (issues.state_id = states.id) \
    INNER JOIN projects ON (issues.project_id = projects.id) \
    INNER JOIN project_members ON (projects.id = project_members.project_id) \
    INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
    INNER JOIN issue_assignees ON (issues.id = issue_assignees.issue_id) \
    WHERE (issues.deleted_at IS NULL \
    AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
    AND NOT (issues.archived_at IS NOT NULL) \
    AND NOT (projects.archived_at IS NOT NULL) \
    AND NOT (issues.is_draft) \
    AND project_members.is_active \
    AND project_members.member_id = $1 \
    AND workspaces.slug = $2 \
    AND issue_assignees.assignee_id = $1) \
    ORDER BY issues.created_at DESC";

/// `user_issues_queryset(..., scope="created")`: the `created_by_id`
/// branch.
pub const USER_ISSUES_CREATED_SQL: &str =
    "SELECT DISTINCT issues.id, issues.created_at FROM issues \
    LEFT OUTER JOIN states ON (issues.state_id = states.id) \
    INNER JOIN projects ON (issues.project_id = projects.id) \
    INNER JOIN project_members ON (projects.id = project_members.project_id) \
    INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
    WHERE (issues.deleted_at IS NULL \
    AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
    AND NOT (issues.archived_at IS NOT NULL) \
    AND NOT (projects.archived_at IS NOT NULL) \
    AND NOT (issues.is_draft) \
    AND project_members.is_active \
    AND project_members.member_id = $1 \
    AND workspaces.slug = $2 \
    AND issues.created_by_id = $1) \
    ORDER BY issues.created_at DESC";

/// Pick the `user_issues_queryset` statement for `scope`. Any string
/// other than `assigned`/`created` falls into `all`, exactly as
/// Python's `else` branch does.
pub fn user_issues_sql(scope: &str) -> &'static str {
    match scope {
        "assigned" => USER_ISSUES_ASSIGNED_SQL,
        "created" => USER_ISSUES_CREATED_SQL,
        _ => USER_ISSUES_ALL_SQL,
    }
}

/// Issue ids in member projects, newest-first
/// (`core/querysets.py:19-30`).
pub async fn fetch_member_project_issue_ids<'e, E>(
    ex: E,
    user_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Vec<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(MEMBER_PROJECT_ISSUES_SQL)
        .bind(user_id)
        .bind(workspace_slug)
        .fetch_all(ex)
        .await?;
    rows.iter().map(|row| row.try_get("id")).collect()
}

/// Issue ids the user is involved in, newest-first
/// (`core/querysets.py:33-52`). `scope` is `all` (default),
/// `assigned` or `created`; anything else means `all`.
pub async fn fetch_user_issue_ids<'e, E>(
    ex: E,
    user_id: uuid::Uuid,
    workspace_slug: &str,
    scope: &str,
) -> Result<Vec<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(user_issues_sql(scope))
        .bind(user_id)
        .bind(workspace_slug)
        .fetch_all(ex)
        .await?;
    rows.iter().map(|row| row.try_get("id")).collect()
}

// ---------------------------------------------------------------------------
// Workspace role facts
// ---------------------------------------------------------------------------

/// Role values (`core/permissions.py:23-25`, from `ROLE_CHOICES`).
/// The F-06 kernel owns the decisions; these pin the vocabulary the
/// fact SQL embeds.
pub const ROLE_ADMIN: i32 = 20;
pub const ROLE_MEMBER: i32 = 15;
pub const ROLE_GUEST: i32 = 5;

/// `is_workspace_member` existence check (`:28-34`). `$1` is the user
/// id, `$2` the workspace id.
pub const IS_WORKSPACE_MEMBER_SQL: &str = "SELECT 1 AS a FROM workspace_members \
    WHERE (workspace_members.deleted_at IS NULL AND workspace_members.is_active \
    AND workspace_members.member_id = $1 AND workspace_members.workspace_id = $2) LIMIT 1";

/// `workspace_role` read (`:37-45`): newest active row's role. The
/// `ORDER BY created_at DESC` comes from the model's `Meta.ordering`
/// plus `.first()`.
pub const WORKSPACE_ROLE_SQL: &str = "SELECT workspace_members.role FROM workspace_members \
    WHERE (workspace_members.deleted_at IS NULL AND workspace_members.is_active \
    AND workspace_members.member_id = $1 AND workspace_members.workspace_id = $2) \
    ORDER BY workspace_members.created_at DESC LIMIT 1";

/// `workspace_role_by_slug` read (`:48-58`): same fact, workspace
/// resolved by slug through the join.
pub const WORKSPACE_ROLE_BY_SLUG_SQL: &str =
    "SELECT workspace_members.role FROM workspace_members \
    INNER JOIN workspaces ON (workspace_members.workspace_id = workspaces.id) \
    WHERE (workspace_members.deleted_at IS NULL AND workspace_members.is_active \
    AND workspace_members.member_id = $1 AND workspaces.slug = $2) \
    ORDER BY workspace_members.created_at DESC LIMIT 1";

/// Whether an active membership row exists (`:28-34`). `None` (an
/// anonymous caller) is `false` with no query, as the fixture pins.
pub async fn fetch_is_workspace_member<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(false);
    };
    let row: Option<PgRow> = sqlx::query(IS_WORKSPACE_MEMBER_SQL)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// The newest active row's role, or `None` (`:37-45`). The column is
/// `smallint`; the value widens to `i32` for the F-06 kernel.
/// `None` user short-circuits to `None` with no query.
pub async fn fetch_workspace_role<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_id: uuid::Uuid,
) -> Result<Option<i32>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let row: Option<PgRow> = sqlx::query(WORKSPACE_ROLE_SQL)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(ex)
        .await?;
    match row {
        None => Ok(None),
        Some(row) => {
            let role: i16 = row.try_get("role")?;
            Ok(Some(i32::from(role)))
        }
    }
}

/// The newest active row's role for the workspace slug, or `None`
/// (`:48-58`). `None` user short-circuits with no query.
pub async fn fetch_workspace_role_by_slug<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_slug: &str,
) -> Result<Option<i32>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let row: Option<PgRow> = sqlx::query(WORKSPACE_ROLE_BY_SLUG_SQL)
        .bind(user_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    match row {
        None => Ok(None),
        Some(row) => {
            let role: i16 = row.try_get("role")?;
            Ok(Some(i32::from(role)))
        }
    }
}

// ---------------------------------------------------------------------------
// Project role facts (check_project_role EXISTS trio)
// ---------------------------------------------------------------------------

/// First `check_project_role` fact (`:89-95`): an active
/// `ProjectMember` row with a role in `allowed_roles`. `$1` user,
/// `$2` slug, `$3` project, `$4+` the roles. An empty role slice
/// short-circuits to `false` with no query (Django's empty `IN`
/// matches nothing; the Q4 precedent skips the statement too).
pub fn project_has_allowed_role_sql(allowed_roles: usize) -> String {
    let binds: Vec<String> = (0..allowed_roles)
        .map(|index| format!("${}", 4 + index))
        .collect();
    format!(
        "SELECT 1 AS a FROM project_members \
        INNER JOIN workspaces ON (project_members.workspace_id = workspaces.id) \
        WHERE (project_members.deleted_at IS NULL AND project_members.is_active \
        AND project_members.member_id = $1 AND project_members.project_id = $3 \
        AND project_members.role IN ({}) AND workspaces.slug = $2) LIMIT 1",
        binds.join(", ")
    )
}

/// Second fact (`:104-109`): an active `ProjectMember` row, any role.
pub const PROJECT_IS_MEMBER_SQL: &str = "SELECT 1 AS a FROM project_members \
    INNER JOIN workspaces ON (project_members.workspace_id = workspaces.id) \
    WHERE (project_members.deleted_at IS NULL AND project_members.is_active \
    AND project_members.member_id = $1 AND project_members.project_id = $3 \
    AND workspaces.slug = $2) LIMIT 1";

/// Third fact (`:110-115`): an active `WorkspaceMember` row with role
/// exactly `ADMIN` — `role=...`, not `>=`, ported as written.
pub const WORKSPACE_ADMIN_BY_SLUG_SQL: &str = "SELECT 1 AS a FROM workspace_members \
    INNER JOIN workspaces ON (workspace_members.workspace_id = workspaces.id) \
    WHERE (workspace_members.deleted_at IS NULL AND workspace_members.is_active \
    AND workspace_members.member_id = $1 AND workspace_members.role = 20 \
    AND workspaces.slug = $2) LIMIT 1";

/// Whether the user holds an active project row with an allowed role
/// (`:89-95`). `None` user (or no roles) is `false` with no query.
pub async fn fetch_project_has_allowed_role<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    allowed_roles: &[i32],
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(false);
    };
    if allowed_roles.is_empty() {
        return Ok(false);
    }
    let sql = project_has_allowed_role_sql(allowed_roles.len());
    let mut query = sqlx::query(&sql)
        .bind(user_id)
        .bind(workspace_slug)
        .bind(project_id);
    for role in allowed_roles {
        query = query.bind(*role);
    }
    let row: Option<PgRow> = query.fetch_optional(ex).await?;
    Ok(row.is_some())
}

/// Whether the user holds an active project row, any role
/// (`:104-109`). `None` user is `false` with no query.
pub async fn fetch_project_is_member<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_slug: &str,
    project_id: uuid::Uuid,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(false);
    };
    let row: Option<PgRow> = sqlx::query(PROJECT_IS_MEMBER_SQL)
        .bind(user_id)
        .bind(workspace_slug)
        .bind(project_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// Whether the user holds an active workspace row with role exactly
/// `ADMIN` (`:110-115`). `None` user is `false` with no query.
pub async fn fetch_workspace_admin_by_slug<'e, E>(
    ex: E,
    user_id: Option<uuid::Uuid>,
    workspace_slug: &str,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(user_id) = user_id else {
        return Ok(false);
    };
    let row: Option<PgRow> = sqlx::query(WORKSPACE_ADMIN_BY_SLUG_SQL)
        .bind(user_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;

    static QUERYSETS_SQL: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/querysets.sql");
    static QUERYSETS_ROWS: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/querysets.rows.json");
    static ROLE_FACTS_SQL: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/role_facts.sql");
    static ROLE_FACTS_GOLDEN: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/role_facts.golden.json");

    /// How the fixture's literals map onto our bind order (`$1`
    /// user, `$2` workspace id/slug, `$3` project, `$4+` roles).
    enum FixtureBinds {
        /// member uuid, workspace uuid → `$1`, `$2`.
        UserWorkspace,
        /// member uuid, slug string → `$1`, `$2`.
        UserSlug,
        /// member uuid, project uuid, slug string, `IN` ints → `$1`,
        /// `$3`, `$2`, `$4+`.
        ProjectTrio,
    }

    /// Fixture SQL reduced to our bind vocabulary: quotes stripped,
    /// literals folded per [`FixtureBinds`], whitespace collapsed.
    /// The `'triage'` literal is kept — both sides spell it the same.
    fn normalize_fixture(sql: &str, binds: FixtureBinds) -> String {
        // Fold `'...'::uuid` occurrences in order.
        let mut folded = String::new();
        let mut rest = sql;
        let mut uuids = 0;
        while let Some(start) = rest.find('\'') {
            let after_quote = &rest[start + 1..];
            let end = after_quote.find('\'').expect("closing quote") + start + 1;
            let cast = rest[end + 1..].starts_with("::uuid");
            if !cast {
                // A plain string: the slug (fold) or 'triage' (keep).
                let literal = &rest[start..=end];
                folded.push_str(&rest[..start]);
                if literal == "'triage'" {
                    folded.push_str(literal);
                } else {
                    folded.push_str("$2");
                }
                rest = &rest[end + 1..];
                continue;
            }
            uuids += 1;
            let bind = match (&binds, uuids) {
                (_, 1) => "$1",
                // Queryset fixtures repeat the same user id in the
                // involvement clause.
                (FixtureBinds::UserSlug, _) => "$1",
                (FixtureBinds::UserWorkspace, _) => "$2",
                (FixtureBinds::ProjectTrio, _) => "$3",
            };
            folded.push_str(&rest[..start]);
            folded.push_str(bind);
            rest = &rest[end + 7..];
        }
        folded.push_str(rest);
        // Fold bare ints inside IN lists (`IN (15, 20)` → `$4+`).
        let mut out = String::new();
        let mut rest = folded.as_str();
        let mut role_bind = 4;
        while let Some(at) = rest.find("IN (") {
            let list_end = rest[at..].find(')').expect("IN close") + at;
            out.push_str(&rest[..at + 4]);
            let mut first = true;
            for _ in rest[at + 4..list_end].split(',') {
                if !first {
                    out.push_str(", ");
                }
                first = false;
                out.push_str(&format!("${role_bind}"));
                role_bind += 1;
            }
            out.push(')');
            rest = &rest[list_end + 1..];
        }
        out.push_str(rest);
        out.replace('"', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn normalize_ours(sql: &str) -> String {
        sql.replace('"', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn queryset_statements_match_fixture_verbatim_modulo_binds() {
        let parsed: Value = serde_json::from_str(QUERYSETS_SQL).expect("fixture parses");
        let executed = parsed["executed_sql"].as_object().expect("executed_sql");
        let cases = [
            ("member_project_issues", MEMBER_PROJECT_ISSUES_SQL),
            ("user_issues_queryset scope=all", USER_ISSUES_ALL_SQL),
            (
                "user_issues_queryset scope=assigned",
                USER_ISSUES_ASSIGNED_SQL,
            ),
            (
                "user_issues_queryset scope=created",
                USER_ISSUES_CREATED_SQL,
            ),
        ];
        assert_eq!(executed.len(), cases.len());
        for (key, statement) in cases {
            let recorded = executed[key][0]["sql"].as_str().expect("recorded sql");
            assert_eq!(
                normalize_ours(statement),
                normalize_fixture(recorded, FixtureBinds::UserSlug),
                "{key}"
            );
        }
        // The `else`-branch fallback: unknown scopes compile to `all`.
        assert_eq!(user_issues_sql("all"), USER_ISSUES_ALL_SQL);
        assert_eq!(user_issues_sql("assigned"), USER_ISSUES_ASSIGNED_SQL);
        assert_eq!(user_issues_sql("created"), USER_ISSUES_CREATED_SQL);
        assert_eq!(user_issues_sql("bogus"), USER_ISSUES_ALL_SQL);
        assert_eq!(user_issues_sql(""), USER_ISSUES_ALL_SQL);
        // Every statement carries the manager scope + DISTINCT.
        for statement in [
            MEMBER_PROJECT_ISSUES_SQL,
            USER_ISSUES_ALL_SQL,
            USER_ISSUES_ASSIGNED_SQL,
            USER_ISSUES_CREATED_SQL,
        ] {
            assert!(statement.starts_with("SELECT DISTINCT issues.id, issues.created_at"));
            for predicate in [
                "issues.deleted_at IS NULL",
                "NOT (states.group = 'triage' AND states.group IS NOT NULL)",
                "NOT (issues.archived_at IS NOT NULL)",
                "NOT (projects.archived_at IS NOT NULL)",
                "NOT (issues.is_draft)",
            ] {
                assert!(statement.contains(predicate), "{predicate}");
            }
            assert!(statement.ends_with("ORDER BY issues.created_at DESC"));
        }
    }

    #[test]
    fn role_fact_statements_match_fixture_verbatim_modulo_binds() {
        let parsed: Value = serde_json::from_str(ROLE_FACTS_SQL).expect("fixture parses");
        let executed = parsed["executed_sql"].as_object().expect("executed_sql");
        for (key, statement, by_slug) in [
            ("is_workspace_member", IS_WORKSPACE_MEMBER_SQL, false),
            ("workspace_role", WORKSPACE_ROLE_SQL, false),
            ("workspace_role_by_slug", WORKSPACE_ROLE_BY_SLUG_SQL, true),
            // `is_workspace_admin` / `is_at_least_member` decisions
            // call `workspace_role`, so they record its statement.
            ("is_workspace_admin", WORKSPACE_ROLE_SQL, false),
            ("is_at_least_member", WORKSPACE_ROLE_SQL, false),
        ] {
            for entry in executed[key].as_array().expect("entries") {
                let recorded = entry["sql"].as_str().expect("recorded sql");
                let binds = if by_slug {
                    FixtureBinds::UserSlug
                } else {
                    FixtureBinds::UserWorkspace
                };
                assert_eq!(
                    normalize_ours(statement),
                    normalize_fixture(recorded, binds),
                    "{key}"
                );
            }
        }
        // The `check_project_role` trio, case by case. The first
        // statement of each non-empty case is the allowed-role
        // `EXISTS` (arity varies); bypass cases add the member +
        // admin `EXISTS` pair.
        let check = parsed["check_project_role_sql"].as_object().expect("trio");
        for (case, entries) in check {
            let entries = entries.as_array().expect("entries");
            if entries.is_empty() {
                continue;
            }
            let first = entries[0]["sql"].as_str().expect("sql");
            let in_list = &first[first.find("IN (").expect("IN") + 4..];
            let in_list = &in_list[..in_list.find(')').expect("close")];
            let roles: Vec<&str> = in_list.split(", ").collect();
            assert_eq!(
                normalize_ours(&project_has_allowed_role_sql(roles.len())),
                normalize_fixture(first, FixtureBinds::ProjectTrio),
                "{case} allowed-role"
            );
            if entries.len() > 1 {
                assert_eq!(
                    normalize_ours(PROJECT_IS_MEMBER_SQL),
                    normalize_fixture(
                        entries[1]["sql"].as_str().expect("sql"),
                        FixtureBinds::ProjectTrio
                    ),
                    "{case} is-member"
                );
            }
            if entries.len() > 2 {
                assert_eq!(
                    normalize_ours(WORKSPACE_ADMIN_BY_SLUG_SQL),
                    normalize_fixture(
                        entries[2]["sql"].as_str().expect("sql"),
                        FixtureBinds::ProjectTrio
                    ),
                    "{case} workspace-admin"
                );
            }
        }
    }

    #[test]
    fn role_vocabulary_matches_fixture() {
        let parsed: Value = serde_json::from_str(ROLE_FACTS_GOLDEN).expect("fixture parses");
        assert_eq!(
            ROLE_ADMIN,
            parsed["ROLE_ADMIN"].as_i64().expect("ROLE_ADMIN") as i32
        );
        assert_eq!(
            ROLE_MEMBER,
            parsed["ROLE_MEMBER"].as_i64().expect("ROLE_MEMBER") as i32
        );
        assert_eq!(
            ROLE_GUEST,
            parsed["ROLE_GUEST"].as_i64().expect("ROLE_GUEST") as i32
        );
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, slug TEXT NOT NULL)",
        "CREATE TEMPORARY TABLE states (id UUID PRIMARY KEY, \"group\" TEXT)",
        "CREATE TEMPORARY TABLE projects (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, archived_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE issues (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, project_id UUID NOT NULL, state_id UUID, created_by_id UUID, archived_at TIMESTAMPTZ, is_draft BOOLEAN NOT NULL, deleted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL)",
        "CREATE TEMPORARY TABLE project_members (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, member_id UUID, role SMALLINT NOT NULL, is_active BOOLEAN NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE workspace_members (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, member_id UUID NOT NULL, role SMALLINT NOT NULL, is_active BOOLEAN NOT NULL, deleted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL)",
        "CREATE TEMPORARY TABLE issue_assignees (id UUID PRIMARY KEY, issue_id UUID NOT NULL, assignee_id UUID NOT NULL)",
        "CREATE TEMPORARY TABLE issue_subscribers (id UUID PRIMARY KEY, issue_id UUID NOT NULL, subscriber_id UUID NOT NULL)",
    ];

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("temp table");
        }
        tx
    }

    fn live_uuid(tag: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("11111111-2222-3333-4444-{tag}")).expect("fixed uuid")
    }

    fn live_time(order: u32) -> chrono::DateTime<chrono::Utc> {
        format!("2026-10-03T11:{order:02}:00+00:00")
            .parse()
            .expect("fixed time")
    }

    /// Seed the `querysets.rows.json` layout: member project FX2 with
    /// the assigned / created / subscribed / uninvolved issues, plus
    /// a non-member project issue that must stay out of every set.
    async fn seed_queryset_layout(conn: &mut sqlx::PgConnection) -> HashMap<String, uuid::Uuid> {
        let ws = live_uuid("000000000001");
        let fx2 = live_uuid("000000000002");
        let fx2b = live_uuid("000000000003");
        let member = live_uuid("000000000010");
        let other = live_uuid("000000000011");
        let state = live_uuid("000000000020");
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'fx2-workspace')")
            .bind(ws)
            .execute(&mut *conn)
            .await
            .expect("workspace");
        sqlx::query("INSERT INTO states (id, \"group\") VALUES ($1, 'unstarted')")
            .bind(state)
            .execute(&mut *conn)
            .await
            .expect("state");
        for project in [fx2, fx2b] {
            sqlx::query(
                "INSERT INTO projects (id, workspace_id, archived_at) VALUES ($1, $2, NULL)",
            )
            .bind(project)
            .bind(ws)
            .execute(&mut *conn)
            .await
            .expect("project");
        }
        sqlx::query("INSERT INTO project_members (id, project_id, workspace_id, member_id, role, is_active, deleted_at) VALUES ($1, $2, $3, $4, 15, true, NULL)")
            .bind(live_uuid("000000000030")).bind(fx2).bind(ws).bind(member)
            .execute(&mut *conn).await.expect("membership");
        // (label, project, created_by, minute)
        let issues = [
            ("q-assigned", fx2, other, 31u32),
            ("q-created", fx2, member, 32),
            ("q-subscribed", fx2, other, 33),
            ("q-uninvolved", fx2, other, 34),
            ("q-nonmember-project", fx2b, other, 35),
        ];
        let mut ids = HashMap::new();
        for (index, (label, project, creator, minute)) in issues.iter().enumerate() {
            let id = live_uuid(&format!("0000000030{:02}", 31 + index));
            sqlx::query("INSERT INTO issues (id, workspace_id, project_id, state_id, created_by_id, archived_at, is_draft, deleted_at, created_at) VALUES ($1, $2, $3, $4, $5, NULL, false, NULL, $6)")
                .bind(id).bind(ws).bind(project).bind(state).bind(creator).bind(live_time(*minute))
                .execute(&mut *conn).await.expect("issue");
            ids.insert(label.to_string(), id);
        }
        sqlx::query("INSERT INTO issue_assignees (id, issue_id, assignee_id) VALUES ($1, $2, $3)")
            .bind(live_uuid("000000000040"))
            .bind(ids["q-assigned"])
            .bind(member)
            .execute(&mut *conn)
            .await
            .expect("assignee");
        sqlx::query("INSERT INTO issue_assignees (id, issue_id, assignee_id) VALUES ($1, $2, $3)")
            .bind(live_uuid("000000000041"))
            .bind(ids["q-nonmember-project"])
            .bind(member)
            .execute(&mut *conn)
            .await
            .expect("assignee");
        sqlx::query(
            "INSERT INTO issue_subscribers (id, issue_id, subscriber_id) VALUES ($1, $2, $3)",
        )
        .bind(live_uuid("000000000042"))
        .bind(ids["q-subscribed"])
        .bind(member)
        .execute(&mut *conn)
        .await
        .expect("subscriber");
        // Manager-scope exclusions (pinned by `issue.py:95-104`,
        // beyond the fixture seeds): triage, archived, draft and
        // soft-deleted rows in the member project stay out.
        let triage = live_uuid("000000000021");
        sqlx::query("INSERT INTO states (id, \"group\") VALUES ($1, 'triage')")
            .bind(triage)
            .execute(&mut *conn)
            .await
            .expect("triage");
        for (index, label) in ["ex-triage", "ex-archived", "ex-draft", "ex-deleted"]
            .iter()
            .enumerate()
        {
            let id = live_uuid(&format!("0000000030{:02}", 41 + index));
            sqlx::query("INSERT INTO issues (id, workspace_id, project_id, state_id, created_by_id, archived_at, is_draft, deleted_at, created_at) VALUES ($1, $2, $3, $4, $5, NULL, false, NULL, $6)")
                .bind(id).bind(ws).bind(fx2).bind(state).bind(other).bind(live_time(40 + index as u32))
                .execute(&mut *conn).await.expect("issue");
            ids.insert(label.to_string(), id);
        }
        let x = |label: &str| ids[label];
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(triage)
            .bind(x("ex-triage"))
            .execute(&mut *conn)
            .await
            .expect("triage");
        sqlx::query("UPDATE issues SET archived_at = now() WHERE id = $1")
            .bind(x("ex-archived"))
            .execute(&mut *conn)
            .await
            .expect("archive");
        sqlx::query("UPDATE issues SET is_draft = true WHERE id = $1")
            .bind(x("ex-draft"))
            .execute(&mut *conn)
            .await
            .expect("draft");
        sqlx::query("UPDATE issues SET deleted_at = now() WHERE id = $1")
            .bind(x("ex-deleted"))
            .execute(&mut *conn)
            .await
            .expect("delete");
        // A stateless issue is *included*: `NULL = 'triage'` is
        // `NULL` but `NULL IS NOT NULL` is `FALSE`, so the inner
        // `AND` is `FALSE` and the `NOT` passes the row — live Django
        // semantics per the fixture SQL.
        let stateless = live_uuid("000000003045");
        sqlx::query("INSERT INTO issues (id, workspace_id, project_id, state_id, created_by_id, archived_at, is_draft, deleted_at, created_at) VALUES ($1, $2, $3, NULL, $4, NULL, false, NULL, $5)")
            .bind(stateless).bind(ws).bind(fx2).bind(other).bind(live_time(45))
            .execute(&mut *conn).await.expect("stateless");
        ids.insert("ex-stateless".to_string(), stateless);
        ids.insert("member".to_string(), member);
        ids
    }

    /// `querysets.rows.json` replay: the four id sets plus the
    /// manager-scope exclusions. Seeds reuse the fixture's issue ids,
    /// so the fetched sets compare directly against the recorded rows.
    #[tokio::test]
    async fn live_queryset_sets_replay_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ids = seed_queryset_layout(&mut tx).await;
        let member = ids["member"];
        let rows: Value = serde_json::from_str(QUERYSETS_ROWS).expect("fixture parses");
        let fixture_set = |key: &str| -> Vec<uuid::Uuid> {
            let mut set: Vec<uuid::Uuid> = rows["results"][key]
                .as_array()
                .expect("result set")
                .iter()
                .map(|entry| entry.as_str().expect("id").parse().expect("uuid"))
                .collect();
            set.sort();
            set
        };
        let mut member_issues = fetch_member_project_issue_ids(&mut *tx, member, "fx2-workspace")
            .await
            .expect("member issues");
        member_issues.sort();
        // The member set is the fixture's plus the extra stateless
        // seed (included per the fixture SQL's NULL semantics).
        let mut expected_member = fixture_set("member_project_issues");
        expected_member.push(ids["ex-stateless"]);
        expected_member.sort();
        assert_eq!(member_issues, expected_member);
        let mut all = fetch_user_issue_ids(&mut *tx, member, "fx2-workspace", "all")
            .await
            .expect("all");
        all.sort();
        assert_eq!(all, fixture_set("user_issues_queryset scope=all"));
        // Unknown scopes fall into `all`.
        let mut bogus = fetch_user_issue_ids(&mut *tx, member, "fx2-workspace", "bogus")
            .await
            .expect("bogus scope");
        bogus.sort();
        assert_eq!(bogus, all);
        let assigned = fetch_user_issue_ids(&mut *tx, member, "fx2-workspace", "assigned")
            .await
            .expect("assigned");
        assert_eq!(assigned, fixture_set("user_issues_queryset scope=assigned"));
        let created = fetch_user_issue_ids(&mut *tx, member, "fx2-workspace", "created")
            .await
            .expect("created");
        assert_eq!(created, fixture_set("user_issues_queryset scope=created"));
        // Newest-first order (no sort): `created_at` descends.
        let ordered = fetch_member_project_issue_ids(&mut *tx, member, "fx2-workspace")
            .await
            .expect("ordered");
        assert_eq!(
            ordered,
            vec![
                ids["ex-stateless"],
                ids["q-uninvolved"],
                ids["q-subscribed"],
                ids["q-created"],
                ids["q-assigned"]
            ]
        );
    }

    /// Seed the `role_facts.golden.json` layout and replay the full
    /// fact matrix plus the nine `check_project_role` outcomes (the
    /// outcomes combine the fetched facts through the kernel rule,
    /// which itself lives in F-06).
    #[tokio::test]
    async fn live_role_facts_replay_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid("000000001001");
        let fx2 = live_uuid("000000002001");
        let admin = live_uuid("000000000101");
        let member = live_uuid("000000000102");
        let guest = live_uuid("000000000103");
        let inactive = live_uuid("000000000104");
        let outsider = live_uuid("000000000105");
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'fx2-workspace')")
            .bind(ws)
            .execute(&mut *tx)
            .await
            .expect("workspace");
        sqlx::query("INSERT INTO projects (id, workspace_id, archived_at) VALUES ($1, $2, NULL)")
            .bind(fx2)
            .bind(ws)
            .execute(&mut *tx)
            .await
            .expect("project");
        // (actor, role, is_active)
        for (index, (actor, role, active)) in [
            (admin, 20i16, true),
            (member, 15, true),
            (guest, 5, true),
            (inactive, 15, false),
        ]
        .iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, deleted_at, created_at) VALUES ($1, $2, $3, $4, $5, NULL, $6)")
                .bind(live_uuid(&format!("0000000011{:02}", 10 + index)))
                .bind(ws).bind(actor).bind(role).bind(active).bind(live_time(10 + index as u32))
                .execute(&mut *tx).await.expect("workspace member");
        }
        // FX2 project rows: member15 role 15, admin20 role 5, guest5
        // role 5 (all active).
        for (index, (actor, role)) in [(member, 15i16), (admin, 5), (guest, 5)].iter().enumerate() {
            sqlx::query("INSERT INTO project_members (id, project_id, workspace_id, member_id, role, is_active, deleted_at) VALUES ($1, $2, $3, $4, $5, true, NULL)")
                .bind(live_uuid(&format!("0000000021{:02}", 10 + index)))
                .bind(fx2).bind(ws).bind(actor).bind(role)
                .execute(&mut *tx).await.expect("project member");
        }

        // Workspace facts per actor.
        for (actor, is_member, role) in [
            (Some(admin), true, Some(20)),
            (Some(member), true, Some(15)),
            (Some(guest), true, Some(5)),
            (Some(inactive), false, None),
            (Some(outsider), false, None),
            (None, false, None),
        ] {
            assert_eq!(
                fetch_is_workspace_member(&mut *tx, actor, ws)
                    .await
                    .expect("member?"),
                is_member,
                "{actor:?}"
            );
            assert_eq!(
                fetch_workspace_role(&mut *tx, actor, ws)
                    .await
                    .expect("role"),
                role,
                "{actor:?}"
            );
            assert_eq!(
                fetch_workspace_role_by_slug(&mut *tx, actor, "fx2-workspace")
                    .await
                    .expect("role by slug"),
                role,
                "{actor:?}"
            );
        }

        // The nine `check_project_role` outcomes, combining the three
        // fetched facts through the kernel rule (`membership.rs`).
        let decide = |allowed: bool, member: bool, admin: bool, bypass: bool| {
            allowed || (bypass && member && admin)
        };
        async fn project_facts(
            conn: &mut sqlx::PgConnection,
            actor: Option<uuid::Uuid>,
            project: uuid::Uuid,
            roles: &[i32],
        ) -> (bool, bool, bool) {
            let allowed =
                fetch_project_has_allowed_role(&mut *conn, actor, "fx2-workspace", project, roles)
                    .await
                    .expect("allowed");
            let member = fetch_project_is_member(&mut *conn, actor, "fx2-workspace", project)
                .await
                .expect("member");
            let admin = fetch_workspace_admin_by_slug(&mut *conn, actor, "fx2-workspace")
                .await
                .expect("admin");
            (allowed, member, admin)
        }
        let (allowed, is_member, is_admin) = project_facts(&mut tx, Some(member), fx2, &[15]).await;
        assert!(decide(allowed, is_member, is_admin, true)); // member15 allowed=[15]
        let (allowed, is_member, is_admin) = project_facts(&mut tx, Some(member), fx2, &[20]).await;
        assert!(!decide(allowed, is_member, is_admin, true)); // member15 allowed=[20]
        assert!(!decide(allowed, is_member, is_admin, false)); // ... no-bypass
        let (allowed, is_member, is_admin) = project_facts(&mut tx, Some(admin), fx2, &[15]).await;
        assert!(decide(allowed, is_member, is_admin, true)); // admin5 allowed=[15] bypass
        assert!(!decide(allowed, is_member, is_admin, false)); // ... no-bypass
        let (allowed, is_member, is_admin) =
            project_facts(&mut tx, Some(guest), fx2, &[15, 20]).await;
        assert!(!decide(allowed, is_member, is_admin, true)); // guest5 bypass denied
        let (allowed, is_member, is_admin) =
            project_facts(&mut tx, Some(outsider), fx2, &[5, 15, 20]).await;
        assert!(!decide(allowed, is_member, is_admin, true)); // outsider denied
        let (allowed, is_member, is_admin) = project_facts(&mut tx, None, fx2, &[15]).await;
        assert!(!allowed && !is_member && !is_admin); // anonymous: no facts
        assert!(!decide(allowed, is_member, is_admin, true));
        // Empty allowed-roles: no query, `false`.
        assert!(
            !fetch_project_has_allowed_role(&mut *tx, Some(member), "fx2-workspace", fx2, &[])
                .await
                .expect("empty")
        );
    }
}
