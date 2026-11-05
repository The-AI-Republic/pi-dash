#![forbid(unsafe_code)]

//! Retrieve / sub-issue / relation+link / archive read querysets (D-26 queries-A).
//!
//! Ports the annotated read querysets behind the issue detail, sub-issue,
//! relation, link and archive endpoints. Every builder is SeaORM / sea-query
//! (relations and links since batch 2g, sub-issues and archive retrieve since
//! Batch L2, which moved the old text builders to the `reads_oracle`
//! differential oracle). Each builder returns an expression or a complete
//! statement the handler executes; handlers own auth gates, filter stacks,
//! ordering variants, pagination and response shaping.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/issue/base.py:486-620` — `IssueViewSet.retrieve` (base
//!   `:489-495`, cycle/count/array annotations `:496-558`, reaction/link
//!   prefetches `:559-570`, `is_subscribed` `:571-580`, guest 403 `:593-607`).
//! - `app/views/issue/sub_issue.py:37-201` — `SubIssuesEndpoint.get`
//!   (annotations `:38-131`, ordering `:133-138`, `.values(27)` `:140-171`).
//! - `app/views/issue/relation.py:42-207` — `IssueRelationViewSet.list`
//!   (relation scope `:42-50`, ten id-lists `:52-100`, target queryset
//!   `:102-154`, eight buckets `:174-207`).
//! - `app/views/issue/link.py:32-46` — `IssueLinkViewSet.get_queryset`.
//! - `app/views/issue/archive.py:54-255` — `IssueArchiveViewSet`
//!   (`apply_annotations` `:61-96`, `get_queryset` `:98-104`, list `:108-219`,
//!   retrieve `:222-255`).
//!
//! Fixture oracle: FX-ISS-11
//! (`rust-api/fixtures/app_issues/queries/FX-ISS-11.core.json`). Every builder
//! below was verified against live-compiled Django 4.2.30 ORM SQL
//! (`str(qs.query)` — no database needed); the unit tests pin the verified
//! text so transcription drift fails the build.
//!
//! Reuse vs pilot-2 (deliberate, verified live):
//! - The `NULLIF(COUNT(*), 0)` count idiom and the `cycle_id` / `label_ids`
//!   subquery texts are pilot-2's, reused verbatim — Django renders the same
//!   predicates as a grouped `COUNT(id)` subquery, which returns no row (SQL
//!   `NULL`) exactly when `NULLIF` yields `NULL`. Pin tests assert the shared
//!   texts still match `super::annotation_selects`.
//! - Three pilot-2 guards are deliberately NOT inherited: the joined-row
//!   `project_members.deleted_at` guard on the active-member assignee array,
//!   the joined-row `modules.deleted_at` guard on the module array, and the
//!   `states.deleted_at` guard on joined state rows. Django applies only the
//!   base model's manager to a query — `filter()` joins and `select_related`
//!   never carry the related model's soft-delete guard (live SQL shows none
//!   of the three). The builders here match Django; pilot-2's merged list
//!   paths keep their disclosed forms (out of scope for this issue).
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `relation.py`'s `duplicate` / `relates_to` `|` combinations compile to
//!    a single `SELECT` with OR-ed `WHERE` branches — not a SQL `UNION`
//!    (FX-ISS-11 says UNION; live SQL shows OR). Ported as OR.
//! 2. The relation target queryset's `cycle_id` / count annotations,
//!    `select_related` and prefetches never reach SQL: `.values(*fields)`
//!    selects only the 14 bucket keys, and Django renders joins solely for
//!    referenced relations. The dead chains are documented, not emitted.
//! 3. The relation id-lists `SELECT DISTINCT issue_id, created_at`: the
//!    ordering column rides along because `DISTINCT` + `ORDER BY` requires
//!    it. Ported with the extra column.
//! 4. The relation bucket queries carry no `ORDER BY`: the direct-`ArrayAgg`
//!    `GROUP BY` drops the model's default ordering. Ported without order.
//! 5. Sub-issue `.values(27)` renders concrete columns in call order, then
//!    annotations in `annotate()` order — so `module_ids` (called third)
//!    selects after `label_ids`/`assignee_ids`. [`sub_issues_select`]
//!    keeps Django's render order.
//! 6. Explicit `deleted_at__isnull` guards on sub-issue/archive-cycle and
//!    sub-issue arrays double the manager's guard (`deleted_at IS NULL AND
//!    deleted_at IS NULL`). One guard is ported; the doubling is unobservable.
//! 7. Link visibility ignores soft-deleted memberships: the member join has
//!    no `deleted_at` guard, so a deleted membership still shows links.
//! 8. `NOT issue_types.is_epic` (not `is_epic = FALSE`): three-valued logic
//!    drops `NULL`-flag rows unless `type_id IS NULL`. Ported as `NOT`.
//! 9. The relation `Func(F('id'), function='Count')` counts would return `0`
//!    (scalar aggregate, no `GROUP BY`) where every sibling path yields
//!    `NULL` — unobservable, since the bucket `.values()` never selects them.
//!
//! Out of scope (owned by sibling handler/guard issues): the permission gates
//! (`resolve_gate`, `@allow_permission`), rich + legacy filter stacks
//! (`complex_filter`, `legacy_sql`), ordering variants (`ordering.rs`,
//! `order_key`), pagination envelopes (`render.rs`, groupers), the archive
//! grouped-count path (uses `list_engine::archive_issue_set` plus pilot-2's
//! grouped paginator), serializer shapes and `handle_exception` bodies.
//! Queryset-adjacent constants handlers need (`.values()` key orders, the
//! `IN`-list bucket table, default orders) ARE in scope and marked.

use pidash_db::entity::{
    issue_assignees, issue_labels, issue_links, issue_relations, issue_subscribers, issue_types,
    issues, module_issues, modules, project_members, projects, states, users, workspaces,
};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::extension::postgres::PgFunc;
use sea_orm::sea_query::{Alias, Condition, Expr, Func, Order, Query, SelectStatement, SimpleExpr};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, IdenStatic, JoinType, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait, Select,
};
use uuid::Uuid;

use super::list_engine::{
    a, attachment_count, col, cycle_id_subquery, id_array, label_ids, link_count, select_one_from,
    sub, sub_issues_count,
};

// ---------------------------------------------------------------------------
// Shared column lists (Django concrete-field order, live-compiled)
// ---------------------------------------------------------------------------
//
// sea-query since Batch L2: the lists are typed entity columns, selected
// through [`select_fields`] under the statement's table alias. Output names
// are the column names, or `<prefix><column>` where several tables share
// one row, so every `row_to_json` key is unique by construction.

/// All 34 concrete `issues` columns in Django's select order.
pub const ISSUE_FIELDS: [issues::Column; 34] = [
    issues::Column::CreatedAt,
    issues::Column::UpdatedAt,
    issues::Column::CreatedById,
    issues::Column::UpdatedById,
    issues::Column::DeletedAt,
    issues::Column::Id,
    issues::Column::ProjectId,
    issues::Column::WorkspaceId,
    issues::Column::ParentId,
    issues::Column::StateId,
    issues::Column::Point,
    issues::Column::EstimatePointId,
    issues::Column::Name,
    issues::Column::DescriptionJson,
    issues::Column::DescriptionHtml,
    issues::Column::DescriptionStripped,
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
    issues::Column::TypeId,
    issues::Column::GitWorkBranch,
    issues::Column::Workpad,
    issues::Column::CreatedVia,
    issues::Column::AssignedPodId,
    issues::Column::AgentExecutor,
];

/// All 18 concrete `states` columns (`select_related("state")` order).
pub const STATE_FIELDS: [states::Column; 18] = [
    states::Column::CreatedAt,
    states::Column::UpdatedAt,
    states::Column::CreatedById,
    states::Column::UpdatedById,
    states::Column::DeletedAt,
    states::Column::Id,
    states::Column::ProjectId,
    states::Column::WorkspaceId,
    states::Column::Name,
    states::Column::Description,
    states::Column::Color,
    states::Column::Slug,
    states::Column::Sequence,
    states::Column::Group,
    states::Column::IsTriage,
    states::Column::Default,
    states::Column::ExternalSource,
    states::Column::ExternalId,
];

/// All 46 concrete `projects` columns (`Project.objects.get` order).
pub const PROJECT_FIELDS: [projects::Column; 46] = [
    projects::Column::CreatedAt,
    projects::Column::UpdatedAt,
    projects::Column::CreatedById,
    projects::Column::UpdatedById,
    projects::Column::DeletedAt,
    projects::Column::Id,
    projects::Column::Name,
    projects::Column::Description,
    projects::Column::DescriptionText,
    projects::Column::DescriptionHtml,
    projects::Column::Network,
    projects::Column::WorkspaceId,
    projects::Column::Identifier,
    projects::Column::DefaultAssigneeId,
    projects::Column::ProjectLeadId,
    projects::Column::Emoji,
    projects::Column::IconProp,
    projects::Column::ModuleView,
    projects::Column::CycleView,
    projects::Column::IssueViewsView,
    projects::Column::PageView,
    projects::Column::IntakeView,
    projects::Column::IsTimeTrackingEnabled,
    projects::Column::IsIssueTypeEnabled,
    projects::Column::IsDefault,
    projects::Column::GuestViewAllFeatures,
    projects::Column::MembersCanEditStates,
    projects::Column::CoverImage,
    projects::Column::CoverImageAssetId,
    projects::Column::EstimateId,
    projects::Column::ArchiveIn,
    projects::Column::CloseIn,
    projects::Column::LogoProps,
    projects::Column::DefaultStateId,
    projects::Column::ArchivedAt,
    projects::Column::Timezone,
    projects::Column::ExternalSource,
    projects::Column::ExternalId,
    projects::Column::RepoUrl,
    projects::Column::BaseBranch,
    projects::Column::AgentDefaultIntervalSeconds,
    projects::Column::AgentDefaultMaxTicks,
    projects::Column::AgentReviewDefaultIntervalSeconds,
    projects::Column::AgentTestDefaultIntervalSeconds,
    projects::Column::AgentTickingEnabled,
    projects::Column::DefaultAgentExecutor,
];

/// Select `alias.column` for every column, named `<prefix><column>` (the
/// bare column name when `prefix` is empty).
pub fn select_fields<C>(
    stmt: &mut SelectStatement,
    alias: &'static str,
    columns: &[C],
    prefix: &str,
) where
    C: IdenStatic + Copy + 'static,
{
    for column in columns {
        if prefix.is_empty() {
            stmt.column((a(alias), *column));
        } else {
            stmt.expr_as(
                col(alias, *column),
                Alias::new(format!("{prefix}{}", column.as_str())),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar annotations shared by sub-issues / identifier / archive retrieve
// ---------------------------------------------------------------------------
//
// `cycle_id`, the `NULLIF(COUNT(*), 0)` counts and `label_ids` are the list
// engine's (`list_engine::{cycle_id_subquery, link_count, attachment_count,
// sub_issues_count, label_ids}`). The sub-issue count is the list engine's
// unguarded form: Django never applies the joined state's manager, so a
// soft-deleted triage state still excludes the child. The two arrays below
// differ from the list engine's on purpose (see module docs): no joined-row
// soft-delete guard on the member or module rows.

/// `COALESCE(<count>, 0)`: the sub-issue / identifier count form.
pub fn coalesce_zero(count: SimpleExpr) -> SimpleExpr {
    Func::coalesce([count, Expr::val(0).into()]).into()
}

/// `assignee_ids` with the active-member join (`base.py:534-545`,
/// `sub_issue.py:99-113`): `users` + `project_members` INNER joins with NO
/// soft-delete guard and no project filter, `pm.is_active` only.
pub fn assignee_ids_active() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(id_array(col("ia", issue_assignees::Column::AssigneeId)))
        .from_as(issue_assignees::Entity, a("ia"))
        .join_as(
            JoinType::InnerJoin,
            users::Entity,
            a("u"),
            col("u", users::Column::Id).equals((a("ia"), issue_assignees::Column::AssigneeId)),
        )
        .join_as(
            JoinType::InnerJoin,
            project_members::Entity,
            a("pm"),
            col("pm", project_members::Column::MemberId).equals((a("u"), users::Column::Id)),
        )
        .and_where(
            col("ia", issue_assignees::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("ia", issue_assignees::Column::DeletedAt).is_null())
        .and_where(col("pm", project_members::Column::IsActive).into());
    sub(stmt)
}

/// `module_ids` with the archived-module guard (`base.py:546-558`,
/// `sub_issue.py:114-129`): NO `modules.deleted_at` guard.
pub fn module_ids_unarchived() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(id_array(col("mi", module_issues::Column::ModuleId)))
        .from_as(module_issues::Entity, a("mi"))
        .join_as(
            JoinType::InnerJoin,
            modules::Entity,
            a("m"),
            col("m", modules::Column::Id).equals((a("mi"), module_issues::Column::ModuleId)),
        )
        .and_where(
            col("mi", module_issues::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .and_where(col("mi", module_issues::Column::DeletedAt).is_null())
        .and_where(col("m", modules::Column::ArchivedAt).is_null());
    sub(stmt)
}

/// `is_subscribed` `Exists` (`base.py:571-580`, `archive.py:238-248`).
pub fn is_subscribed(slug: &str, project_id: Uuid, user_id: Uuid) -> SimpleExpr {
    let mut stmt = select_one_from(issue_subscribers::Entity, "s");
    stmt.join_as(
        JoinType::InnerJoin,
        workspaces::Entity,
        a("sw"),
        col("sw", workspaces::Column::Id).equals((a("s"), issue_subscribers::Column::WorkspaceId)),
    )
    .and_where(col("s", issue_subscribers::Column::DeletedAt).is_null())
    .and_where(
        col("s", issue_subscribers::Column::IssueId).equals((a("issue"), issues::Column::Id)),
    )
    .and_where(col("s", issue_subscribers::Column::ProjectId).eq(project_id))
    .and_where(col("s", issue_subscribers::Column::SubscriberId).eq(user_id))
    .and_where(col("sw", workspaces::Column::Slug).eq(slug))
    .limit(1);
    Expr::exists(stmt)
}

/// `NOT (state."group" = 'triage' AND state."group" IS NOT NULL)`: the
/// `IssueManager` triage exclusion as Django renders it.
pub fn triage_excluded(state_alias: &'static str) -> SimpleExpr {
    col(state_alias, states::Column::Group)
        .eq("triage")
        .and(col(state_alias, states::Column::Group).is_not_null())
        .not()
}

/// `FROM issues AS issue LEFT JOIN states AS state JOIN projects AS project`
/// plus the `issue_objects` manager scope: live, non-triage, non-archived
/// (issue and project), non-draft.
pub fn scoped_issues(stmt: &mut SelectStatement) {
    stmt.from_as(issues::Entity, a("issue"))
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            a("state"),
            col("state", states::Column::Id).equals((a("issue"), issues::Column::StateId)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("project"),
            col("project", projects::Column::Id).equals((a("issue"), issues::Column::ProjectId)),
        )
        .and_where(col("issue", issues::Column::DeletedAt).is_null())
        .and_where(triage_excluded("state"))
        .and_where(col("issue", issues::Column::ArchivedAt).is_null())
        .and_where(col("project", projects::Column::ArchivedAt).is_null())
        .and_where(col("issue", issues::Column::IsDraft).eq(false));
}

/// `JOIN workspaces ON workspaces.id = issue.workspace_id` plus the slug
/// filter.
pub fn workspace_slug(stmt: &mut SelectStatement, slug: &str) {
    stmt.join(
        JoinType::InnerJoin,
        workspaces::Entity,
        Expr::col((workspaces::Entity, workspaces::Column::Id))
            .equals((a("issue"), issues::Column::WorkspaceId)),
    )
    .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug));
}

/// Guest view gate (`base.py:593-601`): an active role-5 membership.
/// Django's `.exists()` is `SELECT 1 ... LIMIT 1`; this reads one id.
pub fn guest_view_select(
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Select<project_members::Entity> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(project_members::Column::DeletedAt.is_null())
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::Role.eq(5))
        .filter(workspaces::Column::Slug.eq(slug))
        .limit(1)
}

// ---------------------------------------------------------------------------
// Sub-issues (sub_issue.py:37-201)
// ---------------------------------------------------------------------------

/// The 27 `.values()` keys in RENDER order (`sub_issue.py:141-171`) — the wire
/// key order handlers render. Django's `.values()` dicts follow the SELECT
/// order, not the call order: concrete columns in call order (`type_id`
/// rides with them, after `parent_id`), then annotations in `annotate()`
/// order (see [`sub_issues_select`]). `estimate_point` / `created_by` /
/// `updated_by` select the `*_id` columns.
pub const SUB_ISSUES_KEYS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "type_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "cycle_id",
    "link_count",
    "attachment_count",
    "sub_issues_count",
    "label_ids",
    "assignee_ids",
    "module_ids",
    "state_group",
];

/// Sub-issue `.values(27)` statement (`sub_issue.py:37-171`) without its
/// `ORDER BY`: concrete columns in call order (aliased where the key
/// differs), then the annotations in `annotate()` order (counts coalesced
/// to `0`), then `state_group`; over the `issue_objects` scope, the parent
/// and the workspace slug. Ordering is handler-owned.
pub fn sub_issues_select(slug: &str, parent_id: Uuid) -> SelectStatement {
    let mut stmt = Query::select();
    let issue = |column: issues::Column| col("issue", column);
    stmt.column((a("issue"), issues::Column::Id))
        .column((a("issue"), issues::Column::Name))
        .column((a("issue"), issues::Column::StateId))
        .column((a("issue"), issues::Column::SortOrder))
        .column((a("issue"), issues::Column::CompletedAt))
        .expr_as(issue(issues::Column::EstimatePointId), a("estimate_point"))
        .column((a("issue"), issues::Column::Priority))
        .column((a("issue"), issues::Column::StartDate))
        .column((a("issue"), issues::Column::TargetDate))
        .column((a("issue"), issues::Column::SequenceId))
        .column((a("issue"), issues::Column::ProjectId))
        .column((a("issue"), issues::Column::ParentId))
        .column((a("issue"), issues::Column::TypeId))
        .column((a("issue"), issues::Column::CreatedAt))
        .column((a("issue"), issues::Column::UpdatedAt))
        .expr_as(issue(issues::Column::CreatedById), a("created_by"))
        .expr_as(issue(issues::Column::UpdatedById), a("updated_by"))
        .column((a("issue"), issues::Column::IsDraft))
        .column((a("issue"), issues::Column::ArchivedAt))
        .expr_as(cycle_id_subquery(), a("cycle_id"))
        .expr_as(coalesce_zero(link_count(true)), a("link_count"))
        .expr_as(coalesce_zero(attachment_count(true)), a("attachment_count"))
        .expr_as(
            coalesce_zero(sub_issues_count(true, false)),
            a("sub_issues_count"),
        )
        .expr_as(label_ids(), a("label_ids"))
        .expr_as(assignee_ids_active(), a("assignee_ids"))
        .expr_as(module_ids_unarchived(), a("module_ids"))
        .expr_as(col("state", states::Column::Group), a("state_group"));
    scoped_issues(&mut stmt);
    stmt.and_where(col("issue", issues::Column::ParentId).eq(parent_id));
    workspace_slug(&mut stmt, slug);
    stmt
}

// ---------------------------------------------------------------------------
// Relations (relation.py:42-207)
// ---------------------------------------------------------------------------

/// One relation id-list: which column to return, which stored `relation_type`
/// to filter, and which side of the pair holds the current issue.
/// Django's ten `values_list` calls collapse to this triple; the eight
/// response buckets each name one (single) or two (OR-ed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationIds {
    /// Returned column: `issue_id` or `related_issue_id`.
    pub column: &'static str,
    /// Stored `relation_type` filter (`blocked_by`, `duplicate`, `relates_to`,
    /// `start_before`, `finish_before`).
    pub relation_type: &'static str,
    /// Side holding the current issue: `issue_id` or `related_issue_id`.
    pub side: &'static str,
}

/// The eight response buckets (`relation.py:174-207`) with their id-list(s).
/// `duplicate` and `relates_to` carry two lists, OR-ed into one statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationBucket {
    /// Response key and `relation_type` literal.
    pub label: &'static str,
    /// One id-list, or two for the OR-ed buckets.
    pub ids: &'static [RelationIds],
}

pub const RELATION_BUCKETS: &[RelationBucket] = &[
    RelationBucket {
        label: "blocking",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "blocked_by",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "blocked_by",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "blocked_by",
            side: "issue_id",
        }],
    },
    RelationBucket {
        label: "duplicate",
        ids: &[
            RelationIds {
                column: "related_issue_id",
                relation_type: "duplicate",
                side: "issue_id",
            },
            RelationIds {
                column: "issue_id",
                relation_type: "duplicate",
                side: "related_issue_id",
            },
        ],
    },
    RelationBucket {
        label: "relates_to",
        ids: &[
            RelationIds {
                column: "related_issue_id",
                relation_type: "relates_to",
                side: "issue_id",
            },
            RelationIds {
                column: "issue_id",
                relation_type: "relates_to",
                side: "related_issue_id",
            },
        ],
    },
    RelationBucket {
        label: "start_after",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "start_before",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "start_before",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "start_before",
            side: "issue_id",
        }],
    },
    RelationBucket {
        label: "finish_after",
        ids: &[RelationIds {
            column: "issue_id",
            relation_type: "finish_before",
            side: "related_issue_id",
        }],
    },
    RelationBucket {
        label: "finish_before",
        ids: &[RelationIds {
            column: "related_issue_id",
            relation_type: "finish_before",
            side: "issue_id",
        }],
    },
];

/// The 14 bucket `.values()` keys in RENDER order (`relation.py:157-172`):
/// concrete columns in call order, the two direct-`ArrayAgg` annotations in
/// `annotate()` order, then the `relation_type` literal. `.values()` dicts —
/// and the wire JSON — follow this order, not the call order.
pub const RELATION_ROW_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "priority",
    "sequence_id",
    "project_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "label_ids",
    "assignee_ids",
    "relation_type",
];

/// `issue_relations` column behind a [`RelationIds`] side or returned
/// column (`issue_id` / `related_issue_id`).
fn relation_column(name: &str) -> issue_relations::Column {
    match name {
        "issue_id" => issue_relations::Column::IssueId,
        "related_issue_id" => issue_relations::Column::RelatedIssueId,
        other => unreachable!("relation bucket column {other}"),
    }
}

/// One relation id-list (`relation.py:52-100`) as an `IN` subquery:
/// `SELECT DISTINCT <column>` over the `(issue OR related)` scope with the
/// workspace slug (an aliased `workspaces` join), side and type filters.
/// Subqueries carry no ordering, so the list's `-created_at` drops.
fn relation_ids_subquery(slug: &str, issue_id: Uuid, ids: RelationIds) -> SelectStatement {
    let rw = Alias::new("rw");
    Query::select()
        .distinct()
        .column((issue_relations::Entity, relation_column(ids.column)))
        .from(issue_relations::Entity)
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            rw.clone(),
            Expr::col((rw.clone(), workspaces::Column::Id)).equals((
                issue_relations::Entity,
                issue_relations::Column::WorkspaceId,
            )),
        )
        .and_where(
            Expr::col((issue_relations::Entity, issue_relations::Column::DeletedAt)).is_null(),
        )
        .cond_where(
            Condition::any()
                .add(
                    Expr::col((issue_relations::Entity, issue_relations::Column::IssueId))
                        .eq(issue_id),
                )
                .add(
                    Expr::col((
                        issue_relations::Entity,
                        issue_relations::Column::RelatedIssueId,
                    ))
                    .eq(issue_id),
                ),
        )
        .and_where(Expr::col((rw, workspaces::Column::Slug)).eq(slug))
        .and_where(Expr::col((issue_relations::Entity, relation_column(ids.side))).eq(issue_id))
        .and_where(
            Expr::col((
                issue_relations::Entity,
                issue_relations::Column::RelationType,
            ))
            .eq(ids.relation_type),
        )
        .to_owned()
}

/// `(SELECT ARRAY_AGG(DISTINCT <column>) FROM <query>)`, correlated on the
/// outer `issues.id`.
fn distinct_array(mut query: SelectStatement, column: SimpleExpr) -> SimpleExpr {
    query.expr(PgFunc::array_agg_distinct(column));
    SimpleExpr::SubQuery(None, Box::new(query.into_sub_query_statement()))
}

/// One relation bucket row (`relation.py:157-172`): the eleven concrete
/// columns plus the two id-list annotations. The `relation_type` literal
/// is the bucket label, rendered by the caller. An empty aggregate is SQL
/// `NULL` (`None`), Django's `[]` default.
#[derive(Debug, FromQueryResult)]
pub struct RelationBucketRow {
    pub id: Uuid,
    pub name: String,
    pub state_id: Option<Uuid>,
    pub sort_order: f64,
    pub priority: String,
    pub sequence_id: i32,
    pub project_id: Uuid,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub label_ids: Option<Vec<Uuid>>,
    pub assignee_ids: Option<Vec<Uuid>>,
}

/// One bucket statement (target queryset `relation.py:102-154`): the
/// `issue_objects` manager plus the workspace slug, `id IN` one or two
/// id-lists (Django's `|` of two conjunctive scopes, factored). The
/// label/assignee `ArrayAgg(distinct, filter=...)` annotations are
/// correlated subqueries with the same filters (live label links; live
/// assignee links whose user has an active project membership in any
/// project, as the unguarded `users` / `project_members` joins decide),
/// so the outer row needs no `GROUP BY`. No `ORDER BY` (bug 4). The
/// target queryset's `cycle_id` / count annotations never reach
/// `.values(*fields)` and are not emitted.
pub fn relation_bucket_select(
    slug: &str,
    issue_id: Uuid,
    bucket: RelationBucket,
) -> sea_orm::Select<issues::Entity> {
    let outer_id = || Expr::col((issues::Entity, issues::Column::Id));
    let label_ids = distinct_array(
        Query::select()
            .from(issue_labels::Entity)
            .and_where(
                Expr::col((issue_labels::Entity, issue_labels::Column::IssueId)).eq(outer_id()),
            )
            .and_where(
                Expr::col((issue_labels::Entity, issue_labels::Column::LabelId)).is_not_null(),
            )
            .and_where(Expr::col((issue_labels::Entity, issue_labels::Column::DeletedAt)).is_null())
            .to_owned(),
        Expr::col((issue_labels::Entity, issue_labels::Column::LabelId)).into(),
    );
    let assignee_ids = distinct_array(
        Query::select()
            .from(issue_assignees::Entity)
            .inner_join(
                users::Entity,
                Expr::col((users::Entity, users::Column::Id))
                    .equals((issue_assignees::Entity, issue_assignees::Column::AssigneeId)),
            )
            .inner_join(
                project_members::Entity,
                Expr::col((project_members::Entity, project_members::Column::MemberId))
                    .equals((users::Entity, users::Column::Id)),
            )
            .and_where(
                Expr::col((issue_assignees::Entity, issue_assignees::Column::IssueId))
                    .eq(outer_id()),
            )
            .and_where(
                Expr::col((issue_assignees::Entity, issue_assignees::Column::AssigneeId))
                    .is_not_null(),
            )
            .and_where(
                Expr::col((project_members::Entity, project_members::Column::IsActive)).eq(true),
            )
            .and_where(
                Expr::col((issue_assignees::Entity, issue_assignees::Column::DeletedAt)).is_null(),
            )
            .to_owned(),
        Expr::col((issue_assignees::Entity, issue_assignees::Column::AssigneeId)).into(),
    );
    let membership = bucket.ids.iter().fold(Condition::any(), |any, ids| {
        any.add(outer_id().in_subquery(relation_ids_subquery(slug, issue_id, *ids)))
    });
    issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::Name,
            issues::Column::StateId,
            issues::Column::SortOrder,
            issues::Column::Priority,
            issues::Column::SequenceId,
            issues::Column::ProjectId,
            issues::Column::CreatedAt,
            issues::Column::UpdatedAt,
            issues::Column::CreatedById,
            issues::Column::UpdatedById,
        ])
        .expr_as(label_ids, "label_ids")
        .expr_as(assignee_ids, "assignee_ids")
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(
            Condition::all()
                .add(states::Column::Group.eq("triage"))
                .add(states::Column::Group.is_not_null())
                .not(),
        )
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(membership)
}

// ---------------------------------------------------------------------------
// Links (link.py:32-46)
// ---------------------------------------------------------------------------

/// The link-list queryset (`link.py:26-35`): all link columns,
/// `DISTINCT` (the member join can fan out), newest first. The member
/// join is unguarded (bug 7): any active membership row of the actor in
/// the project, soft-deleted or not.
pub fn link_list_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
) -> sea_orm::Select<issue_links::Entity> {
    issue_links::Entity::find()
        .distinct()
        .join(JoinType::InnerJoin, issue_links::Relation::Workspaces.def())
        .join(JoinType::InnerJoin, issue_links::Relation::Projects.def())
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Projects.def().rev(),
        )
        .filter(issue_links::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_links::Column::ProjectId.eq(project_id))
        .filter(issue_links::Column::IssueId.eq(issue_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(user_id))
        .order_by_desc(issue_links::Column::CreatedAt)
}

// ---------------------------------------------------------------------------
// Archive (archive.py:54-255)
// ---------------------------------------------------------------------------
//
// The archive list / count / group statements are the list engine's
// (`list_engine::archive_issue_set`); only the retrieve lives here.

/// Archive retrieve (`archive.py:222-255`): all issue columns plus
/// `is_subscribed`, NO count/array annotations, over the archive scope
/// (non-epic type, archived, this project and workspace; `Issue.objects`,
/// so triage states and drafts stay visible) and the pk.
pub fn archive_retrieve_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
) -> SelectStatement {
    let mut stmt = Query::select();
    select_fields(&mut stmt, "issue", &ISSUE_FIELDS, "");
    stmt.expr_as(is_subscribed(slug, project_id, user_id), a("is_subscribed"))
        .from_as(issues::Entity, a("issue"))
        .join(
            JoinType::LeftJoin,
            issue_types::Entity,
            Expr::col((a("issue"), issues::Column::TypeId))
                .equals((issue_types::Entity, issue_types::Column::Id)),
        );
    workspace_slug(&mut stmt, slug);
    stmt.and_where(col("issue", issues::Column::DeletedAt).is_null())
        .and_where(
            col("issue", issues::Column::TypeId)
                .is_null()
                .or(Expr::col((issue_types::Entity, issue_types::Column::IsEpic)).not()),
        )
        .and_where(col("issue", issues::Column::ArchivedAt).is_not_null())
        .and_where(col("issue", issues::Column::ProjectId).eq(project_id))
        .and_where(col("issue", issues::Column::Id).eq(issue_id))
        .order_by((a("issue"), issues::Column::CreatedAt), Order::Desc)
        .limit(1);
    stmt
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLUG: &str = "ws-slug";
    const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    const ISSUE: uuid::Uuid = uuid::Uuid::from_u128(0x33333333_3333_3333_3333_333333333333);
    const USER: uuid::Uuid = uuid::Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);

    fn built(stmt: &SelectStatement) -> String {
        stmt.to_string(sea_orm::sea_query::PostgresQueryBuilder)
    }

    // -- annotations ---------------------------------------------------------

    #[test]
    fn faithful_arrays_drop_the_list_engine_guards() {
        // Live Django SQL carries no soft-delete guard on the joined member,
        // module, or state rows; these builders match Django, not the list
        // engine's shared forms.
        let mut stmt = Query::select();
        stmt.expr_as(assignee_ids_active(), a("assignee_ids"))
            .expr_as(module_ids_unarchived(), a("module_ids"))
            .expr_as(coalesce_zero(sub_issues_count(true, false)), a("n"));
        let sql = built(&stmt);
        assert!(!sql.contains(r#""pm"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""m"."deleted_at""#), "{sql}");
        assert!(!sql.contains(r#""cs"."deleted_at""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "users" AS "u" ON "u"."id" = "ia"."assignee_id""#));
        assert!(
            sql.contains(r#"INNER JOIN "project_members" AS "pm" ON "pm"."member_id" = "u"."id""#)
        );
        assert!(sql.contains(r#"AND "pm"."is_active""#), "{sql}");
        assert!(sql.contains(r#""m"."archived_at" IS NULL"#));
        assert!(
            sql.contains("COALESCE((SELECT NULLIF(COUNT(*), 0)"),
            "{sql}"
        );
    }

    // -- sub-issues ----------------------------------------------------------

    #[test]
    fn sub_issues_select_renders_values_order_over_manager_scope() {
        let sql = built(&sub_issues_select(SLUG, ISSUE));
        let pos = |needle: &str| {
            sql.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {sql}"))
        };
        assert!(
            sql.starts_with(r#"SELECT "issue"."id", "issue"."name""#),
            "{sql}"
        );
        assert!(pos(r#""issue"."parent_id""#) < pos(r#""issue"."type_id""#));
        assert!(pos(r#""issue"."type_id""#) < pos(r#""issue"."created_at""#));
        assert!(pos(r#"AS "cycle_id""#) < pos(r#"AS "link_count""#));
        assert!(pos(r#"AS "sub_issues_count""#) < pos(r#"AS "label_ids""#));
        assert!(pos(r#"AS "label_ids""#) < pos(r#"AS "assignee_ids""#));
        assert!(pos(r#"AS "assignee_ids""#) < pos(r#"AS "module_ids""#));
        assert!(pos(r#"AS "module_ids""#) < pos(r#"AS "state_group""#));
        assert_eq!(
            sql.matches("COALESCE((SELECT NULLIF(COUNT(*), 0)").count(),
            3
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" AS "state" ON "state"."id" = "issue"."state_id""#)
        );
        assert!(sql.contains(r#"INNER JOIN "projects" AS "project""#));
        assert!(sql.contains(r#"NOT ("state"."group" = 'triage' AND "state"."group" IS NOT NULL)"#));
        assert!(sql.contains(r#""project"."archived_at" IS NULL"#));
        assert!(sql.contains(r#""issue"."is_draft" = FALSE"#));
        assert!(sql.contains(&format!(r#""issue"."parent_id" = '{ISSUE}'"#)));
        assert!(sql.contains(r#""workspaces"."slug" = 'ws-slug'"#));
        assert!(!sql.contains("ORDER BY"));
    }

    #[test]
    fn sub_issues_keys_cover_all_27() {
        assert_eq!(SUB_ISSUES_KEYS.len(), 27);
        for key in [
            "estimate_point",
            "created_by",
            "updated_by",
            "state_group",
            "type_id",
        ] {
            assert!(SUB_ISSUES_KEYS.contains(&key), "missing {key}");
        }
        // Wire order is RENDER order, not call order: Django's `.values()`
        // dicts follow the SELECT list (concrete columns first, then
        // annotations in `annotate()` order).
        let pos = |key: &str| SUB_ISSUES_KEYS.iter().position(|k| *k == key).unwrap();
        assert!(pos("parent_id") < pos("type_id"));
        assert!(pos("type_id") < pos("created_at"));
        assert!(pos("archived_at") < pos("cycle_id"));
        assert!(pos("cycle_id") < pos("link_count"));
        assert!(pos("link_count") < pos("attachment_count"));
        assert!(pos("attachment_count") < pos("sub_issues_count"));
        assert!(pos("sub_issues_count") < pos("label_ids"));
        assert!(pos("label_ids") < pos("assignee_ids"));
        assert!(pos("assignee_ids") < pos("module_ids"));
        assert!(pos("module_ids") < pos("state_group"));
    }

    // -- relations -----------------------------------------------------------

    #[test]
    fn relation_buckets_cover_all_eight_labels() {
        let labels: Vec<_> = RELATION_BUCKETS.iter().map(|bucket| bucket.label).collect();
        assert_eq!(
            labels,
            [
                "blocking",
                "blocked_by",
                "duplicate",
                "relates_to",
                "start_after",
                "start_before",
                "finish_after",
                "finish_before"
            ]
        );
        // Ten id-lists in total: six singles plus two OR-ed pairs.
        let total: usize = RELATION_BUCKETS.iter().map(|bucket| bucket.ids.len()).sum();
        assert_eq!(total, 10);
        assert_eq!(RELATION_ROW_FIELDS.len(), 14);
        // RELATION_ROW_FIELDS is RENDER (wire) order: concrete columns, then
        // the label/assignee annotations, then the relation_type literal.
        let pos = |key: &str| RELATION_ROW_FIELDS.iter().position(|k| *k == key).unwrap();
        assert!(pos("project_id") < pos("created_at"));
        assert!(pos("updated_by") < pos("label_ids"));
        assert!(pos("label_ids") < pos("assignee_ids"));
        assert!(pos("assignee_ids") < pos("relation_type"));
    }

    #[test]
    fn relation_bucket_select_pins_scope_membership_and_aggregates() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = relation_bucket_select(SLUG, ISSUE, RELATION_BUCKETS[2]) // duplicate
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#"LEFT JOIN "states""#));
        assert!(sql.contains(r#"INNER JOIN "projects""#));
        assert!(
            sql.contains(r#"NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)"#)
        );
        assert!(sql.contains(r#""issues"."is_draft" = FALSE"#));
        assert!(sql.contains(r#"ARRAY_AGG(DISTINCT "issue_labels"."label_id")"#));
        assert!(sql.contains(r#"ARRAY_AGG(DISTINCT "issue_assignees"."assignee_id")"#));
        assert!(sql.contains(r#""project_members"."is_active" = TRUE"#));
        assert!(sql
            .contains(r#""issues"."id" IN (SELECT DISTINCT "issue_relations"."related_issue_id""#));
        assert!(
            sql.contains(r#" OR "issues"."id" IN (SELECT DISTINCT "issue_relations"."issue_id""#)
        );
        assert_eq!(sql.matches(r#""rw"."slug" = 'ws-slug'"#).count(), 2);
        assert_eq!(sql.matches(r#""workspaces"."slug" = 'ws-slug'"#).count(), 1);
        assert!(!sql.contains("ORDER BY"));
        assert!(!sql.contains("GROUP BY"));
        assert!(sql.contains(r#"AS "label_ids""#));
        assert!(sql.contains(r#"AS "assignee_ids""#));
    }

    // -- links ---------------------------------------------------------------

    #[test]
    fn link_list_joins_member_without_deleted_guard() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = link_list_select(SLUG, PROJECT, ISSUE, USER)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.starts_with("SELECT DISTINCT"));
        assert!(sql
            .contains(r#"INNER JOIN "projects" ON "issue_links"."project_id" = "projects"."id""#));
        assert!(sql.contains(
            r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#
        ));
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#));
        assert!(sql.contains(r#""project_members"."is_active" = TRUE"#));
        assert!(!sql.contains(r#""project_members"."deleted_at""#));
        assert!(sql.ends_with(r#"ORDER BY "issue_links"."created_at" DESC"#));
    }

    // -- archive -------------------------------------------------------------

    #[test]
    fn archive_retrieve_has_subscribed_but_no_counts() {
        let sql = built(&archive_retrieve_select(SLUG, PROJECT, ISSUE, USER));
        assert!(sql.starts_with(r#"SELECT "issue"."created_at", "issue"."updated_at""#));
        assert!(sql.contains(r#"AS "is_subscribed""#));
        assert!(!sql.contains("link_count") && !sql.contains("label_ids"));
        assert!(
            sql.contains(r#"LEFT JOIN "issue_types" ON "issue"."type_id" = "issue_types"."id""#)
        );
        assert!(
            sql.contains(r#"("issue"."type_id" IS NULL OR (NOT "issue_types"."is_epic"))"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issue"."archived_at" IS NOT NULL"#));
        let scope = sql.rsplit(" WHERE ").next().expect("where");
        assert!(
            !scope.contains("triage") && !scope.contains("is_draft"),
            "{scope}"
        );
        assert!(sql.contains(&format!(r#""s"."subscriber_id" = '{USER}'"#)));
        assert!(sql.contains(r#""sw"."slug" = 'ws-slug'"#));
        assert!(sql.ends_with(r#"ORDER BY "issue"."created_at" DESC LIMIT 1"#));
    }

    #[test]
    fn guest_gate_is_role_5_active_member() {
        use sea_orm::QueryTrait;
        let sql = guest_view_select(SLUG, PROJECT, USER)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""project_members"."role" = 5"#));
        assert!(sql.contains(r#""project_members"."is_active" = TRUE"#));
        assert!(sql.contains(r#""project_members"."deleted_at" IS NULL"#));
        assert!(sql.contains(r#""workspaces"."slug" = 'ws-slug'"#));
        assert!(!sql.contains("ORDER BY"));
        assert!(sql.ends_with("LIMIT 1"));
    }
}
