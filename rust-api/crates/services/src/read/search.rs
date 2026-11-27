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
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait,
};
use sea_query::extension::postgres::PgExpr;
use sea_query::{Alias, Condition, Expr, Func, SimpleExpr};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{issue_comments, issues, project_members, projects, states, workspaces};

use super::{DbError, Error};
use crate::app_views_search::fts;

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

/// One search row: the issue payload columns plus the joined refs
/// (`project_identifier`, `workspace_slug`, `state_name`, `state_group`)
/// and the `_rank` / `_headline` annotations, decoded by name. The
/// joined refs are `Option` for the `LEFT JOIN` state; the inner-joined
/// project/workspace refs always decode as `Some`.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct IssueSearchRow {
    pub id: Uuid,
    pub type_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub point: Option<i32>,
    pub name: String,
    pub description_html: String,
    pub description_binary: Option<Vec<u8>>,
    pub priority: String,
    pub complexity_score: i32,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<chrono::NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub estimate_point_id: Option<Uuid>,
    pub assigned_pod_id: Option<Uuid>,
    pub project_identifier: Option<String>,
    pub workspace_slug: Option<String>,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub rank: Option<f32>,
    pub headline: Option<String>,
}

/// `'english'::regconfig`: the FTS config literal, bound as text and
/// cast (the `queries_search` precedent).
fn regconfig() -> SimpleExpr {
    SimpleExpr::from(Expr::val("english")).cast_as(Alias::new("regconfig"))
}

/// `to_tsvector('english'::regconfig, <text>)` as a function
/// expression (`Func::cust`, the `queries_search` precedent: `PgFunc`
/// only omits the config or takes an OID, and the config name is
/// normative here).
fn tsvector(text: SimpleExpr) -> sea_query::FunctionCall {
    Func::cust("to_tsvector").arg(regconfig()).arg(text)
}

/// `websearch_to_tsquery('english'::regconfig, <query>)`.
fn tsquery(query: &str) -> sea_query::FunctionCall {
    Func::cust("websearch_to_tsquery")
        .arg(regconfig())
        .arg(Expr::val(query))
}

/// The issue's search vector: `to_tsvector('english'::regconfig,
/// COALESCE(name, '') || ' ' || COALESCE(description_stripped, ''))`.
fn issue_vector() -> sea_query::FunctionCall {
    let text = SimpleExpr::from(Func::coalesce([
        Expr::col((issues::Entity, issues::Column::Name)).into(),
        SimpleExpr::from(Expr::val("")),
    ]))
    .concatenate(Expr::val(" "))
    .concatenate(Func::coalesce([
        Expr::col((issues::Entity, issues::Column::DescriptionStripped)).into(),
        SimpleExpr::from(Expr::val("")),
    ]));
    tsvector(text)
}

/// The comment vector: `to_tsvector('english'::regconfig,
/// COALESCE(comment_stripped, ''))`.
fn comment_vector() -> sea_query::FunctionCall {
    tsvector(SimpleExpr::from(Func::coalesce([
        Expr::col((
            issue_comments::Entity,
            issue_comments::Column::CommentStripped,
        ))
        .into(),
        SimpleExpr::from(Expr::val("")),
    ])))
}

/// The comment-text arm (`Q(id__in=_matching_comment_issue_ids(...))`,
/// `search/issue.py:102-103,111-112`): the soft-deletion-excluded
/// comment set, selecting `issue_id`.
fn comment_match(query: &str) -> SimpleExpr {
    use sea_query::Query as SeaQuery;
    Expr::col((issues::Entity, issues::Column::Id)).in_subquery(
        SeaQuery::select()
            .column(issue_comments::Column::IssueId)
            .from(issue_comments::Entity)
            .and_where(Expr::col(issue_comments::Column::DeletedAt).is_null())
            .and_where(comment_vector().matches(tsquery(query)))
            .take(),
    )
}

/// The project-identifier arm: `EXISTS (SELECT 1 FROM projects WHERE
/// projects.id = issues.project_id AND identifier ILIKE pattern)`.
fn identifier_exists(like_pattern: &str) -> SimpleExpr {
    use sea_query::Query as SeaQuery;
    Expr::exists(
        SeaQuery::select()
            .expr(Expr::val(1))
            .from(projects::Entity)
            .and_where(
                Expr::col((projects::Entity, projects::Column::Id))
                    .eq(Expr::col((issues::Entity, issues::Column::ProjectId))),
            )
            .and_where(
                Expr::col((projects::Entity, projects::Column::Identifier)).ilike(like_pattern),
            )
            .take(),
    )
}

/// `EXISTS (SELECT 1 FROM "project_members" WHERE
/// "project_members"."project_id" = <project_col> AND
/// "project_members"."member_id" = $user AND "is_active")`: the
/// membership scope (the old `JOIN project_members ... + DISTINCT`
/// collapses to one row per issue; `EXISTS` yields the same rows
/// without the fan-out).
fn member_exists(project_col: (impl EntityTrait, impl ColumnTrait), user_id: &Uuid) -> SimpleExpr {
    use sea_query::Query as SeaQuery;
    Expr::exists(
        SeaQuery::select()
            .expr(Expr::val(1))
            .from(project_members::Entity)
            .and_where(
                Expr::col((project_members::Entity, project_members::Column::ProjectId))
                    .equals(project_col),
            )
            .and_where(
                Expr::col((project_members::Entity, project_members::Column::MemberId))
                    .eq(*user_id),
            )
            .and_where(
                Expr::col((project_members::Entity, project_members::Column::IsActive)).eq(true),
            )
            .take(),
    )
}

/// The `_build_search_filter` OR-chain (`search/issue.py:93-125`), in
/// Django arm order: FTS, name fallback, optional comment arm,
/// sequence ints, project identifier. The `ILIKE` patterns arrive
/// pre-concatenated (`%{escape}%`); the `ESCAPE '\'` clause is
/// postgres's default, so omitting it is equivalent.
fn search_filter(
    mut condition: Condition,
    query: &str,
    like: &str,
    sequence: &[i32],
    include_comments: bool,
) -> Condition {
    condition = condition.add(issue_vector().matches(tsquery(query)));
    condition = condition.add(Expr::col((issues::Entity, issues::Column::Name)).ilike(like));
    if include_comments {
        condition = condition.add(comment_match(query));
    }
    condition = condition.add(
        Expr::col((issues::Entity, issues::Column::SequenceId)).is_in(sequence.iter().copied()),
    );
    condition.add(identifier_exists(like))
}

/// The `_headline` annotation options (`search/issue.py:171-182`):
/// the exact literal `fts::headline_sql` renders (its `''` escapes
/// are the SQL-literal form of these single quotes).
const HEADLINE_OPTIONS: &str =
    "StartSel='<<', StopSel='>>', MaxWords=20, MinWords=10, ShortWord=3, HighlightAll=false";

/// Build the search statement. The scope is the membership issue
/// scope line for line (membership `EXISTS`, null-safe triage
/// exclusion, archived / draft guards, `DISTINCT`) over the full-row
/// select list plus the `rank` / `headline` annotations; then the
/// optional project narrowing, status, and since predicates, the FTS
/// filter, and the sort + limit.
pub fn search_issues_statement(
    query: &str,
    like: &str,
    sequence: &[i32],
    params: &IssueSearchParams<'_>,
) -> sea_orm::Select<issues::Entity> {
    let filter = search_filter(
        Condition::any(),
        query,
        like,
        sequence,
        params.include_comments,
    );

    let mut select = issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::TypeId,
            issues::Column::CreatedAt,
            issues::Column::UpdatedAt,
            issues::Column::DeletedAt,
            issues::Column::Point,
            issues::Column::Name,
            issues::Column::DescriptionHtml,
            issues::Column::DescriptionBinary,
            issues::Column::Priority,
            issues::Column::ComplexityScore,
            issues::Column::StartDate,
            issues::Column::TargetDate,
            issues::Column::SequenceId,
            issues::Column::SortOrder,
            issues::Column::CompletedAt,
            issues::Column::ArchivedAt,
            issues::Column::IsDraft,
            issues::Column::ExternalSource,
            issues::Column::ExternalId,
            issues::Column::GitWorkBranch,
            issues::Column::CreatedVia,
            issues::Column::AgentExecutor,
            issues::Column::CreatedById,
            issues::Column::UpdatedById,
            issues::Column::ProjectId,
            issues::Column::WorkspaceId,
            issues::Column::ParentId,
            issues::Column::StateId,
            issues::Column::EstimatePointId,
            issues::Column::AssignedPodId,
        ])
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(workspaces::Column::Slug, "workspace_slug")
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
        .column_as(
            SimpleExpr::from(
                Func::cust("ts_rank")
                    .arg(issue_vector())
                    .arg(tsquery(query)),
            ),
            "rank",
        )
        .column_as(
            SimpleExpr::from(
                Func::cust("ts_headline")
                    .arg(regconfig())
                    .arg(Expr::col((
                        issues::Entity,
                        issues::Column::DescriptionStripped,
                    )))
                    .arg(tsquery(query))
                    .arg(Expr::val(HEADLINE_OPTIONS)),
            ),
            "headline",
        )
        .distinct()
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(params.workspace_slug))
        .filter(member_exists(
            (issues::Entity, issues::Column::ProjectId),
            &params.user_id,
        ))
        .filter(
            Expr::col((states::Entity, states::Column::Group))
                .is_null()
                .or(Expr::col((states::Entity, states::Column::Group)).ne("triage")),
        )
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .filter(filter);

    if let Some(project_id) = params.project_id {
        select = select.filter(issues::Column::ProjectId.eq(project_id));
    }
    match params.status {
        IssueSearchStatus::All => {}
        // `.exclude(state__group__in=[...])` renders `NOT (... IN ...)`:
        // NULL-group rows evaluate to NULL and drop out, exactly like
        // Django — stateless issues only surface under `all`.
        IssueSearchStatus::Open => {
            select = select.filter(
                Expr::col((states::Entity, states::Column::Group))
                    .is_in(["completed", "cancelled"])
                    .not(),
            );
        }
        IssueSearchStatus::Closed => {
            select = select.filter(
                Expr::col((states::Entity, states::Column::Group))
                    .is_in(["completed", "cancelled"]),
            );
        }
    }
    if let Some(since) = params.since {
        select = select.filter(issues::Column::UpdatedAt.gte(since));
    }
    select = match params.sort {
        IssueSearchSort::Rank => select
            .order_by(Expr::col(Alias::new("rank")), sea_query::Order::Desc)
            .order_by_desc(issues::Column::UpdatedAt),
        IssueSearchSort::Created => select.order_by_desc(issues::Column::CreatedAt),
        IssueSearchSort::Updated => select.order_by_desc(issues::Column::UpdatedAt),
    };
    select.limit(u64::try_from(params.limit).unwrap_or(0))
}

/// `search_issues` (`tools.py:1304-1361`): full-text search over the
/// membership scope. Returns the full rows with `rank` (`ts_rank`,
/// `real`) and `headline` annotations; the caller shapes snippets,
/// ranks, urls and per-row summaries.
pub async fn search_issues(
    pool: &PgPool,
    query: &str,
    params: &IssueSearchParams<'_>,
) -> Result<Vec<IssueSearchRow>, Error> {
    let like = fts::escape_like(query);
    let sequence: Vec<i32> = fts::sequence_tokens(query)
        .into_iter()
        .filter_map(|token| i32::try_from(token).ok())
        .collect();
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = search_issues_statement(query, &like, &sequence, params)
        .into_model::<IssueSearchRow>()
        .all(&db)
        .await
        .map_err(|error| DbError::from_orm("issue-search", error))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use sea_orm::{DbBackend, QueryTrait};

    use super::*;

    /// The full arm: narrowed + open + since + rank + comments; the
    /// bare arm drops each.
    #[test]
    fn search_statement_composes_scope_filters_and_sort() {
        let full = search_issues_statement(
            "seed",
            "%seed%",
            &[1, 2],
            &IssueSearchParams {
                workspace_slug: "acme",
                user_id: Uuid::nil(),
                project_id: Some(Uuid::nil()),
                status: IssueSearchStatus::Open,
                since: Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
                sort: IssueSearchSort::Rank,
                include_comments: true,
                limit: 10,
            },
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(full.contains("SELECT DISTINCT"), "{full}");
        assert!(full.contains(r#"AS "rank""#), "{full}");
        assert!(full.contains(r#"AS "headline""#), "{full}");
        assert!(full.contains("ts_rank"), "{full}");
        assert!(full.contains("ts_headline"), "{full}");
        assert!(full.contains("websearch_to_tsquery"), "{full}");
        assert!(full.contains("@@"), "{full}");
        assert!(full.contains(r#""issues"."project_id" = "#), "{full}");
        assert!(
            full.contains(r#"NOT "states"."group" IN ('completed', 'cancelled')"#),
            "{full}"
        );
        assert!(full.contains(r#""issues"."updated_at" >= "#), "{full}");
        assert!(
            full.contains(r#"ORDER BY "rank" DESC, "issues"."updated_at" DESC"#),
            "{full}"
        );
        assert!(full.contains("LIMIT 10"), "{full}");
        assert!(
            full.contains(r#"JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id""#),
            "{full}"
        );
        assert!(
            full.contains(r#"JOIN "projects" ON "issues"."project_id" = "projects"."id""#),
            "{full}"
        );
        assert!(
            full.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{full}"
        );
        assert!(
            full.contains(r#"EXISTS(SELECT 1 FROM "project_members""#),
            "{full}"
        );
        assert!(full.contains(r#""project_members"."member_id""#), "{full}");
        assert!(
            full.contains(r#""project_members"."is_active" = TRUE"#),
            "{full}"
        );
        assert!(full.contains(r#""states"."group" IS NULL"#), "{full}");
        assert!(full.contains(r#""states"."group" <> 'triage'"#), "{full}");
        assert!(full.contains("ILIKE"), "{full}");
        assert!(full.contains(r#""issues"."sequence_id" IN ("#), "{full}");
        // The comment arm nests the comment subquery.
        assert!(full.contains("issue_comments"), "{full}");

        let bare = search_issues_statement(
            "seed",
            "%seed%",
            &[],
            &IssueSearchParams {
                workspace_slug: "acme",
                user_id: Uuid::nil(),
                project_id: None,
                status: IssueSearchStatus::All,
                since: None,
                sort: IssueSearchSort::Updated,
                include_comments: false,
                limit: 8,
            },
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(!bare.contains(r#"AND "issues"."project_id" = "#), "{bare}");
        assert!(!bare.contains("IN ('completed', 'cancelled')"), "{bare}");
        assert!(!bare.contains(r#""issues"."updated_at" >= "#), "{bare}");
        assert!(
            bare.contains(r#"ORDER BY "issues"."updated_at" DESC"#),
            "{bare}"
        );
        // Empty sequence list: `IN ()` would be a syntax error; the
        // builder renders the always-false guard instead.
        assert!(bare.contains("1 = 2"), "{bare}");
        assert!(bare.contains("LIMIT 8"), "{bare}");

        let created = search_issues_statement(
            "seed",
            "%seed%",
            &[],
            &IssueSearchParams {
                workspace_slug: "acme",
                user_id: Uuid::nil(),
                project_id: None,
                status: IssueSearchStatus::Closed,
                since: None,
                sort: IssueSearchSort::Created,
                include_comments: false,
                limit: 5,
            },
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(
            created.contains(r#""states"."group" IN ('completed', 'cancelled')"#),
            "{created}"
        );
        assert!(
            created.contains(r#"ORDER BY "issues"."created_at" DESC"#),
            "{created}"
        );
    }
}

#[cfg(test)]
mod live {
    use chrono::{TimeZone, Utc};
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
        // Bare arm: the seeded issue matches by name.
        let rows = search_issues(&db, "seed", &params(&world))
            .await
            .expect("bare");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, world.issue_id);
        // Full arm: narrowed + open + since + rank + comments.
        let mut full = params(&world);
        full.project_id = Some(world.project_id);
        full.status = IssueSearchStatus::Open;
        full.since = Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());
        full.sort = IssueSearchSort::Rank;
        full.include_comments = true;
        let rows = search_issues(&db, "seed", &full).await.expect("full");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].rank.is_some());
        // The seed has no description text, so no headline markers are
        // asserted — only that the annotation column decodes.
        let _headline: Option<String> = rows[0].headline.clone();
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
