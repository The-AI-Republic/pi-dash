//! Composed issue full-text search (`search_issues`).
//!
//! One query: the membership scope plus the OSS FTS closure
//! (`app_views_search::fts` rank/headline/filter) with the status, since,
//! sort and limit composed before the cutoff. Rank, sort and limit must
//! compose in a single statement — the cutoff applies to ranked rows, so
//! the scope and the FTS arms cannot run as separate queries.
//! (Moved from the MCP read tools by PIDASHCONV-1661; Rule 5: queries on
//! OSS tables live here. Argument parsing — status/since/sort strip,
//! match and value checks — stays with the MCP caller as argument
//! mapping; this module takes parsed values.)

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};
use crate::app_views_search::fts;

/// The full-row select list: the issue payload columns plus the
/// joined refs (`project_identifier`, `workspace_slug`, `state_name`,
/// `state_group`), all aliased identically for the row decode.
pub const FULL_ROW_COLUMNS: &str =
    "issues.id, issues.type_id, issues.created_at, issues.updated_at, \
     issues.deleted_at, issues.point, issues.name, issues.description_html, \
     issues.description_binary, issues.priority, issues.complexity_score, \
     issues.start_date, issues.target_date, issues.sequence_id, issues.sort_order, \
     issues.completed_at, issues.archived_at, issues.is_draft, issues.external_source, \
     issues.external_id, issues.git_work_branch, issues.created_via, issues.agent_executor, \
     issues.created_by_id, issues.updated_by_id, issues.project_id, issues.workspace_id, \
     issues.parent_id, issues.state_id, issues.estimate_point_id, issues.assigned_pod_id, \
     project.identifier AS project_identifier, workspaces.slug AS workspace_slug, \
     state.name AS state_name, state.\"group\" AS state_group";

/// The `status` filter (`tools.py:1310`, checked at `:1316-1321`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSearchStatus {
    All,
    Open,
    Closed,
}

/// The `sort` order (`tools.py:1312`, checked at `:1338-1345`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSearchSort {
    Rank,
    Created,
    Updated,
}

/// Parsed `search_issues` inputs: the membership scope, the FTS options,
/// and the cutoff. `query` travels separately (bound three times: rank,
/// headline, filter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSearchParams<'a> {
    /// The workspace slug (`$1`).
    pub workspace_slug: &'a str,
    /// The calling user (`$2`, membership scope).
    pub user_id: Uuid,
    /// Project narrowing (`resolve_project`, gated).
    pub project_id: Option<Uuid>,
    /// The status filter.
    pub status: IssueSearchStatus,
    /// The `since` floor (`issues.updated_at >=`, parsed).
    pub since: Option<DateTime<Utc>>,
    /// The sort order.
    pub sort: IssueSearchSort,
    /// OR-include comment-text matches.
    pub include_comments: bool,
    /// The row cutoff (clamped `1..=max` by the caller).
    pub limit: i64,
}

/// Build the search statement. The scope is the membership issue
/// scope line for line (membership join, null-safe triage exclusion,
/// archived / draft guards, `DISTINCT`) over the full-row select list
/// plus the `_rank` / `_headline` annotations; then the optional
/// project narrowing, status, and since predicates, the FTS filter,
/// and the sort + limit.
///
/// Bind order: `$1` slug, `$2` user, `[$3]` project, rank/headline/filter
/// query copies, int tokens, LIKE pattern, `[since]`, limit.
pub fn search_issues_sql(
    narrowed: bool,
    status: IssueSearchStatus,
    with_since: bool,
    sort: IssueSearchSort,
    include_comments: bool,
) -> String {
    let mut next: i32 = 3;
    let mut predicates = String::new();
    if narrowed {
        predicates.push_str(&format!(" AND issues.project_id = ${next}"));
        next += 1;
    }
    match status {
        IssueSearchStatus::All => {}
        // `.exclude(state__group__in=[...])` renders `NOT (... IN ...)`:
        // NULL-group rows evaluate to NULL and drop out, exactly like
        // Django — stateless issues only surface under `all`.
        // (BUG-947-1: `tools.py:1317-1318`; the open listing silently
        // drops stateless issues while `list_issues` keeps them.)
        IssueSearchStatus::Open => {
            predicates.push_str(" AND NOT (state.\"group\" IN ('completed', 'cancelled'))");
        }
        IssueSearchStatus::Closed => {
            predicates.push_str(" AND state.\"group\" IN ('completed', 'cancelled')");
        }
    }
    let rank_param = next;
    let headline_param = next + 1;
    let fts_param = next + 2;
    let seq_param = next + 3;
    let like_param = next + 4;
    next += 5;
    predicates.push_str(&format!(
        " AND {}",
        fts::search_filter_sql(
            include_comments,
            &format!("${fts_param}"),
            &format!("${seq_param}"),
            &format!("${like_param}"),
        )
    ));
    if with_since {
        predicates.push_str(&format!(" AND issues.updated_at >= ${next}"));
        next += 1;
    }
    let order = match sort {
        IssueSearchSort::Rank => "ORDER BY _rank DESC, issues.updated_at DESC",
        IssueSearchSort::Created => "ORDER BY issues.created_at DESC",
        IssueSearchSort::Updated => "ORDER BY issues.updated_at DESC",
    };
    format!(
        "SELECT DISTINCT {FULL_ROW_COLUMNS}, {rank} AS _rank, {headline} AS _headline \
         FROM issues \
         JOIN workspaces ON workspaces.id = issues.workspace_id \
         JOIN projects AS project ON project.id = issues.project_id \
         JOIN project_members AS pm ON pm.project_id = project.id \
         LEFT OUTER JOIN states AS state ON state.id = issues.state_id \
         WHERE issues.deleted_at IS NULL \
           AND workspaces.slug = $1 \
           AND pm.member_id = $2 AND pm.is_active \
           AND (state.\"group\" IS NULL OR NOT (state.\"group\" = 'triage')) \
           AND issues.archived_at IS NULL \
           AND project.archived_at IS NULL \
           AND issues.is_draft = FALSE{predicates} \
         {order} LIMIT ${next}",
        rank = fts::rank_sql("issues", &format!("${rank_param}")),
        headline = fts::headline_sql(&format!("${headline_param}")),
    )
}

/// `search_issues` (`tools.py:1304-1361`): full-text search over the
/// membership scope. Returns the full rows with `_rank` (`ts_rank`,
/// `real`) and `_headline` annotations; the caller shapes snippets,
/// ranks, urls and per-row summaries. Bind order follows
/// [`search_issues_sql`]: slug, user, [project], the raw query three
/// times (rank, headline, filter — Django binds one copy per
/// `SearchQuery` rendering too), the int tokens, the LIKE pattern,
/// [since], limit.
pub async fn search_issues(
    pool: &PgPool,
    query: &str,
    params: &IssueSearchParams<'_>,
) -> Result<Vec<sqlx::postgres::PgRow>, Error> {
    let sql = search_issues_sql(
        params.project_id.is_some(),
        params.status,
        params.since.is_some(),
        params.sort,
        params.include_comments,
    );
    let like = fts::escape_like(query);
    let sequence: Vec<i32> = fts::sequence_tokens(query)
        .into_iter()
        .filter_map(|token| i32::try_from(token).ok())
        .collect();
    let mut bound = sqlx::query(&sql)
        .bind(params.workspace_slug)
        .bind(params.user_id);
    if let Some(project_id) = params.project_id {
        bound = bound.bind(project_id);
    }
    bound = bound
        .bind(query)
        .bind(query)
        .bind(query)
        .bind(sequence.as_slice())
        .bind(like.as_str());
    if let Some(since) = params.since {
        bound = bound.bind(since);
    }
    let rows = bound
        .bind(params.limit)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("issue-search", error))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_sql_composes_scope_filters_and_sort() {
        // Moved with the builder from the MCP read tools: the full arm
        // composes narrowing + open + since + rank + comments; the bare
        // arm drops each.
        let full = search_issues_sql(
            true,
            IssueSearchStatus::Open,
            true,
            IssueSearchSort::Rank,
            true,
        );
        assert!(full.contains("SELECT DISTINCT"), "{full}");
        assert!(full.contains("AS _rank"), "{full}");
        assert!(full.contains("AS _headline"), "{full}");
        assert!(full.contains("issues.project_id = $3"), "{full}");
        assert!(
            full.contains("AND NOT (state.\"group\" IN ('completed', 'cancelled'))"),
            "{full}"
        );
        assert!(full.contains("issues.updated_at >="), "{full}");
        assert!(full.contains("issues.id IN (SELECT issue_id"), "{full}");
        assert!(
            full.contains("ORDER BY _rank DESC, issues.updated_at DESC"),
            "{full}"
        );
        // Placeholder census, narrowed + since: 1 slug, 2 user, 3 project,
        // 4-8 rank/headline/fts/seq/like, 9 since, 10 limit.
        for placeholder in [
            "$1",
            "$2",
            "$3",
            "$4",
            "$5",
            "$6",
            "$7",
            "$8",
            "$9",
            "LIMIT $10",
        ] {
            assert!(full.contains(placeholder), "{full}");
        }
        assert!(!full.contains("$11"), "{full}");

        let bare = search_issues_sql(
            false,
            IssueSearchStatus::All,
            false,
            IssueSearchSort::Updated,
            false,
        );
        assert!(!bare.contains("issues.project_id = "), "{bare}");
        assert!(!bare.contains("IN ('completed', 'cancelled')"), "{bare}");
        assert!(!bare.contains("issues.updated_at >="), "{bare}");
        assert!(!bare.contains("issues.id IN (SELECT issue_id"), "{bare}");
        assert!(bare.contains("ORDER BY issues.updated_at DESC"), "{bare}");
        // 1 slug, 2 user, 3-7 rank/headline/fts/seq/like, 8 limit.
        assert!(bare.contains("LIMIT $8"), "{bare}");

        let created = search_issues_sql(
            false,
            IssueSearchStatus::Closed,
            false,
            IssueSearchSort::Created,
            false,
        );
        assert!(
            created.contains("AND state.\"group\" IN ('completed', 'cancelled')"),
            "{created}"
        );
        assert!(
            created.contains("ORDER BY issues.created_at DESC"),
            "{created}"
        );
    }
}

#[cfg(test)]
mod live {
    use chrono::{TimeZone, Utc};
    use sqlx::Row;
    use uuid::Uuid;

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

    fn params<'a>(world: &'a crate::read::test_support::World) -> IssueSearchParams<'a> {
        IssueSearchParams {
            workspace_slug: &world.workspace_slug,
            user_id: world.user_id,
            project_id: None,
            status: IssueSearchStatus::All,
            since: None,
            sort: IssueSearchSort::Updated,
            include_comments: false,
            limit: 10,
        }
    }

    #[tokio::test]
    async fn search_binds_every_arm_and_ranks() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        seed_member(&db, &world).await;
        // Bare arm (8 binds): the seeded issue matches by name.
        let rows = search_issues(&db, "seed", &params(&world))
            .await
            .expect("bare");
        assert_eq!(rows.len(), 1);
        let id: Uuid = rows[0].try_get("id").expect("id");
        assert_eq!(id, world.issue_id);
        // Full arm (10 binds): narrowed + open + since + rank + comments.
        let mut full = params(&world);
        full.project_id = Some(world.project_id);
        full.status = IssueSearchStatus::Open;
        full.since = Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());
        full.sort = IssueSearchSort::Rank;
        full.include_comments = true;
        let rows = search_issues(&db, "seed", &full).await.expect("full");
        assert_eq!(rows.len(), 1);
        let rank: Option<f32> = rows[0].try_get("_rank").expect("rank");
        assert!(rank.is_some());
        // The seed has no description text, so no headline markers are
        // asserted — only that the annotation column decodes.
        let _headline: Option<String> = rows[0].try_get("_headline").expect("headline");
        // Created sort binds.
        let mut created = params(&world);
        created.sort = IssueSearchSort::Created;
        let rows = search_issues(&db, "seed", &created).await.expect("created");
        assert_eq!(rows.len(), 1);
        // Closed excludes the backlog-group seed; a future since excludes
        // everything; narrowing to another project excludes everything.
        let mut closed = params(&world);
        closed.status = IssueSearchStatus::Closed;
        let rows = search_issues(&db, "seed", &closed).await.expect("closed");
        assert!(rows.is_empty());
        let mut future = params(&world);
        future.since = Some(Utc.with_ymd_and_hms(2999, 1, 1, 0, 0, 0).unwrap());
        let rows = search_issues(&db, "seed", &future)
            .await
            .expect("future since");
        assert!(rows.is_empty());
        let mut elsewhere = params(&world);
        elsewhere.project_id = Some(Uuid::new_v4());
        let rows = search_issues(&db, "seed", &elsewhere)
            .await
            .expect("narrowed away");
        assert!(rows.is_empty());
    }
}
