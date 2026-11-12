#![forbid(unsafe_code)]

//! Activity / comment / version / meta / identifier read querysets (D-26 queries-B).
//!
//! Ports the annotated read querysets behind the issue history, comment,
//! version, meta and identifier endpoints. Every builder is SeaORM / sea-query
//! (history and comments since batch 2h, meta and identifier since Batch L2,
//! which moved the old text builders to the `reads_oracle` differential
//! oracle). Each builder returns an expression or a complete statement the
//! handler executes; handlers own auth gates,
//! filter stacks, ordering variants, pagination envelopes and response shaping.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/issue/activity.py:30-86` — `IssueActivityEndpoint.get`
//!   (activities `:35-46`, comments `:47-64`, `issue-property` branch
//!   `:66-75`, `issue-comment` branch `:77-79`, default branch `:81-85`).
//! - `app/views/issue/comment.py:43-69` — `IssueCommentViewSet.get_queryset`
//!   (selects `:55-57`, `is_member` `:58-67`, `distinct` `:68`) and
//!   `:186-200` — `CommentReactionViewSet.get_queryset`.
//! - `app/views/issue/version.py:36-144` — `IssueVersionEndpoint.get`
//!   (detail `:38-44`, list `:46-74`) and
//!   `WorkItemDescriptionVersionEndpoint.get` (detail `:107-116`, list
//!   `:118-144`), both paginated through `utils/global_paginator.py:33-87`.
//! - `app/views/issue/base.py:1199-1211` — `IssueMetaEndpoint.get`.
//! - `app/views/issue/base.py:1214-1489` — `IssueDetailIdentifierEndpoint.get`
//!   (project lookup `:1253`, member gate `:1256-1265`, lite `:1267-1310`,
//!   full `:1312-1446`, guest gate `:1460-1474`).
//!
//! Fixture oracle: FX-ISS-12
//! (`rust-api/fixtures/app_issues/queries/FX-ISS-12.engage.json`). Every
//! builder below was verified against live-compiled Django 4.2.30 ORM SQL
//! (`str(qs.query)` — no database needed, plus a replicated `get_count`
//! rendering for the two `paginate` counts); the unit tests pin the
//! verified text so transcription drift fails the build.
//!
//! Reuse (deliberate, verified live — nothing is re-ported):
//! - Column lists, the `cycle_id` / count / array annotation texts,
//!   `is_subscribed`, the reaction/link prefetch statements and the
//!   guest-view gate are `super::queries_core` fragments, referenced, not
//!   copied. Pin tests assert the shared texts still match.
//! - Page math is pilot-2's `super::render::v2_page` (the
//!   `global_paginator.paginate` port). The version reads themselves are
//!   SeaORM selects in `handlers_versions_move` (batch 2c); only their
//!   [`VERSION_PAGE_KEYS`] render order stays here.
//! - The identifier reaction/link prefetches are
//!   `queries_core::{reaction_prefetch_sql, link_prefetch_sql}` verbatim
//!   (same `Prefetch` chains as retrieve); the guest-view gate is
//!   `queries_core::guest_view_exists_sql` verbatim.
//! - The identifier array subqueries carry Django's doubled `deleted_at`
//!   guard (explicit `deleted_at__isnull=True` plus the manager's); one
//!   guard is ported, the doubling is unobservable (queries-A bug 6).
//! - The `NULLIF(COUNT(*), 0)` count idiom reproduces Django's grouped
//!   `COUNT(id)` subquery, which yields no row (SQL `NULL`) when empty
//!   (queries-A CAGG class, shared text).
//!
//! `Issue.objects` vs `issue_objects` (load-bearing, verified live): the
//! identifier lite/full lookups use the plain `Issue.objects` manager, so
//! triage-state, archived and draft rows ARE visible there; only the meta
//! lookup uses the scoped `issue_objects` manager (triage/archived/draft
//! exclusions). [`meta_select`] carries the scope, the identifier builders
//! carry none.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. The history default branch subscripts model instances
//!    (`instance["created_at"]`, `activity.py:81-84`) and raises
//!    `TypeError` → generic 500 whenever `?activity_type` is absent or
//!    unknown. Handler behavior, not SQL: the two querysets below serve
//!    all three branches; the handler owns the 500.
//! 2. Lite `is_synced` is a git-only `Exists` over `git_issue_syncs`
//!    (unscoped — no project/workspace predicate) that the
//!    `_issue_is_actively_synced` predicate honors, so lite rows report
//!    Github-synced issues as unsynced. Ported as-is.
//! 3. Version `paginate`: a malformed cursor raises `ValueError` and a
//!    zero-size cursor raises `ZeroDivisionError`, both uncaught → generic
//!    500. `render::v2_page` already fails both inputs; the handler maps
//!    the failure to the 500.
//! 4. The history/comment member join carries no `deleted_at` guard
//!    (`filter()` joins never apply the related manager), so a deleted
//!    membership still authorizes — and, on the `DISTINCT`-less history
//!    paths, duplicates rows. Ported as-is (queries-A bug 7 class).
//!
//! Deliberate consolidations (same values, fewer round trips):
//! - Meta: Django's `.only("sequence_id", "project__identifier")` selects
//!   three issue columns and lazy-loads `project.identifier` in a second
//!   query (a spanning `.only()` without `select_related` never joins).
//!   [`meta_select`] is one statement selecting both values over the join the
//!   manager scope already requires.
//! - `.get()` lookups port `LIMIT 21` as `LIMIT 1`: every one is over a
//!   unique key (version pk; project `(identifier, workspace)` partial
//!   unique plus `save()` upper-normalization), so `MultipleObjectsReturned`
//!   is unreachable through the app (queries-A `.get()` precedent).
//! - Django's generated table aliases (`T4`/`T7` actors and parents, `U0`
//!   subqueries) are normalized to readable aliases (`users`, `parent`,
//!   short subquery aliases); a semantic no-op.
//!
//! Out of scope (owned by sibling handler/guard/serializer/task issues):
//! the permission gates (`resolve_gate`, `@allow_permission`, the
//! `strict_str_to_int` / `is_lite_request` pure helpers), the comment
//! `filterset_fields` splice (`issue__id`, `workspace__id`), the
//! `paginate` envelopes (keys + `user_timezone_converter`, via `render::V2Page`),
//! the lite/full serializer shapes (the 15-key lite dict, `LITE_KEYS`),
//! the `recent_visited_task` / `issue_activity` enqueues, and the
//! identifier full `assignees` / `labels` / `issue_module__module` M2M
//! prefetches (handler-owned, pilot-2 prefetch pattern — queries-A
//! archive precedent). Queryset-adjacent constants handlers need (the
//! `.values()` key orders) ARE in scope and marked.

use super::list_engine::{
    a, attachment_count, col, cycle_id_subquery, label_ids, link_count, select_one_from,
    sub_issues_count,
};
use super::queries_core::{
    assignee_ids_active, coalesce_zero, is_subscribed, module_ids_unarchived, scoped_issues,
    select_fields, workspace_slug, ISSUE_FIELDS, PROJECT_FIELDS, STATE_FIELDS,
};

use pidash_db::entity::{
    comment_reactions, git_issue_syncs, intake_issues, issue_activities, issue_agent_ticker,
    issue_comments, issue_types, issues, project_members, projects, states, users, workspaces,
};
use sea_orm::sea_query::{Alias, Expr, Func, Order, Query, SelectStatement, SimpleExpr};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, IdenStatic, JoinType, QueryFilter, QueryOrder,
    QuerySelect, RelationDef, RelationTrait, Select,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// History (activity.py:30-86)
// ---------------------------------------------------------------------------
//
// SeaORM selects (batch 2h). The base table's own columns come back
// unprefixed (`Entity::find()` selects them all, decoded through the
// entity `Model`); every joined nest is aliased `<prefix><column>`
// ([`PROJECT_PREFIX`] and friends), so output names are unique by
// construction and the handlers decode by name, never by position. Only
// the nest columns the serializers read are selected.

/// Output-name prefix of the joined project-lite columns.
pub const PROJECT_PREFIX: &str = "p_";
/// Output-name prefix of the joined workspace-lite columns.
pub const WORKSPACE_PREFIX: &str = "w_";
/// Output-name prefix of the joined issue-flat columns.
pub const ISSUE_PREFIX: &str = "i_";
/// Output-name prefix of the joined actor-lite columns.
pub const USER_PREFIX: &str = "u_";

/// `ProjectLiteSerializer` inputs (`project.py:120-133`).
const PROJECT_LITE: [projects::Column; 8] = [
    projects::Column::Id,
    projects::Column::Identifier,
    projects::Column::Name,
    projects::Column::CoverImage,
    projects::Column::CoverImageAssetId,
    projects::Column::LogoProps,
    projects::Column::Description,
    projects::Column::IsDefault,
];

/// `WorkspaceLiteSerializer` inputs (`workspace.py:79-83`).
const WORKSPACE_LITE: [workspaces::Column; 5] = [
    workspaces::Column::Id,
    workspaces::Column::Name,
    workspaces::Column::Slug,
    workspaces::Column::Logo,
    workspaces::Column::LogoAssetId,
];

/// `IssueFlatSerializer` inputs (`issue.py:105-123`).
const ISSUE_FLAT: [issues::Column; 11] = [
    issues::Column::Id,
    issues::Column::Name,
    issues::Column::DescriptionJson,
    issues::Column::DescriptionHtml,
    issues::Column::Priority,
    issues::Column::ComplexityScore,
    issues::Column::StartDate,
    issues::Column::TargetDate,
    issues::Column::SequenceId,
    issues::Column::SortOrder,
    issues::Column::IsDraft,
];

/// `UserLiteSerializer` inputs (`user.py:141-153`).
const USER_LITE: [users::Column; 7] = [
    users::Column::Id,
    users::Column::FirstName,
    users::Column::LastName,
    users::Column::Avatar,
    users::Column::AvatarAssetId,
    users::Column::IsBot,
    users::Column::DisplayName,
];

/// Add `columns` aliased `<prefix><column>`.
fn select_prefixed<E, C, I>(mut select: Select<E>, columns: I, prefix: &str) -> Select<E>
where
    E: EntityTrait,
    C: ColumnTrait,
    I: IntoIterator<Item = C>,
{
    for column in columns {
        select = select.column_as(column, format!("{prefix}{}", column.as_str()));
    }
    select
}

/// The project / workspace / issue nests (`select_related`), prefixed.
fn select_nests<E: EntityTrait>(select: Select<E>) -> Select<E> {
    let select = select_prefixed(select, PROJECT_LITE, PROJECT_PREFIX);
    let select = select_prefixed(select, WORKSPACE_LITE, WORKSPACE_PREFIX);
    select_prefixed(select, ISSUE_FLAT, ISSUE_PREFIX)
}

/// The actor nest (`select_related("actor")`), prefixed.
fn select_actor<E: EntityTrait>(select: Select<E>) -> Select<E> {
    select_prefixed(select, USER_LITE, USER_PREFIX)
}

/// History member-gate joins and predicates (`activity.py:35-55`),
/// shared by both history querysets: project membership (unguarded —
/// bug 4), the archived-project exclusion, the workspace slug, the
/// optional `?created_at__gt=` bound, and the oldest-first order.
/// `created_after` binds as text cast to `timestamptz` (Postgres parses
/// it, the same rows Django's timestamptz bind returns).
#[allow(clippy::too_many_arguments)]
fn history_gate<E: EntityTrait>(
    select: Select<E>,
    columns: HistoryColumns<E::Column>,
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    created_after: Option<&str>,
) -> Select<E> {
    let select = select
        .join(JoinType::InnerJoin, columns.issue)
        .join(JoinType::InnerJoin, columns.project)
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .join(JoinType::InnerJoin, columns.workspace)
        .join(JoinType::LeftJoin, columns.actor)
        .filter(columns.deleted_at.is_null())
        .filter(columns.issue_id.eq(issue_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug));
    let select = match created_after {
        None => select,
        Some(raw) => select.filter(
            Expr::col((E::default(), columns.created_at))
                .gt(Expr::val(raw.to_owned()).cast_as(Alias::new("timestamptz"))),
        ),
    };
    select.order_by_asc(columns.created_at)
}

/// The base-table edges and columns [`history_gate`] references.
struct HistoryColumns<C> {
    issue: RelationDef,
    project: RelationDef,
    workspace: RelationDef,
    actor: RelationDef,
    issue_id: C,
    deleted_at: C,
    created_at: C,
}

/// History-activities select (`activity.py:35-46`): the property feed
/// minus the `comment`/`vote`/`reaction`/`draft` fields, oldest first.
/// The negated `IN` keeps Django's exact null semantics — `NOT (field IN
/// (...) AND field IS NOT NULL)` INCLUDES null-field rows, where a bare
/// `NOT IN` would drop them. Base columns unprefixed; project, workspace,
/// issue and actor nests prefixed.
pub fn history_activities_select(
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    created_after: Option<&str>,
) -> Select<issue_activities::Entity> {
    use issue_activities::Column as A;
    let select = select_actor(select_nests(issue_activities::Entity::find()));
    let field = || Expr::col((issue_activities::Entity, A::Field));
    history_gate(
        select,
        HistoryColumns {
            issue: issue_activities::Relation::Issues.def(),
            project: issue_activities::Relation::Projects.def(),
            workspace: issue_activities::Relation::Workspaces.def(),
            actor: issue_activities::Relation::Users3.def(),
            issue_id: A::IssueId,
            deleted_at: A::DeletedAt,
            created_at: A::CreatedAt,
        },
        slug,
        issue_id,
        user_id,
        created_after,
    )
    .filter(
        field()
            .is_in(["comment", "vote", "reaction", "draft"])
            .and(field().is_not_null())
            .not(),
    )
}

/// History-comments select (`activity.py:47-64`): same gate and order as
/// [`history_activities_select`], no field exclusion. The
/// `comment_reactions` prefetch is [`history_reactions_prefetch_select`].
pub fn history_comments_select(
    slug: &str,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
    created_after: Option<&str>,
) -> Select<issue_comments::Entity> {
    use issue_comments::Column as C;
    let select = select_actor(select_nests(issue_comments::Entity::find()));
    history_gate(
        select,
        HistoryColumns {
            issue: issue_comments::Relation::Issues.def(),
            project: issue_comments::Relation::Projects.def(),
            workspace: issue_comments::Relation::Workspaces.def(),
            actor: issue_comments::Relation::Users3.def(),
            issue_id: C::IssueId,
            deleted_at: C::DeletedAt,
            created_at: C::CreatedAt,
        },
        slug,
        issue_id,
        user_id,
        created_after,
    )
}

/// History reaction prefetch (`Prefetch("comment_reactions",
/// CommentReaction.objects.select_related("actor"))`, `activity.py:58-63`).
/// The actor FK is non-null, so the users join is `INNER`; only the
/// actor's `display_name` is read (`u_display_name`).
pub fn history_reactions_prefetch_select(
    comment_ids: &[uuid::Uuid],
) -> Select<comment_reactions::Entity> {
    comment_reactions::Entity::find()
        .column_as(
            users::Column::DisplayName,
            format!("{USER_PREFIX}{}", users::Column::DisplayName.as_str()),
        )
        .join(
            JoinType::InnerJoin,
            comment_reactions::Relation::Users3.def(),
        )
        .filter(comment_reactions::Column::DeletedAt.is_null())
        .filter(comment_reactions::Column::CommentId.is_in(comment_ids.iter().copied()))
        .order_by_desc(comment_reactions::Column::CreatedAt)
}

/// `issue-property` intake prefetch (`activity.py:67-73`):
/// `issue__issue_intake` deferred to the three source columns, newest
/// first (`to_attr` is fetch-shape only — no SQL).
pub fn history_intake_prefetch_select(issue_ids: &[uuid::Uuid]) -> Select<intake_issues::Entity> {
    intake_issues::Entity::find()
        .select_only()
        .columns([
            intake_issues::Column::Id,
            intake_issues::Column::Source,
            intake_issues::Column::SourceEmail,
            intake_issues::Column::Extra,
        ])
        .filter(intake_issues::Column::DeletedAt.is_null())
        .filter(intake_issues::Column::IssueId.is_in(issue_ids.iter().copied()))
        .order_by_desc(intake_issues::Column::CreatedAt)
}

// ---------------------------------------------------------------------------
// Comments (comment.py:43-69,186-200)
// ---------------------------------------------------------------------------

/// `is_member` `Exists` (`comment.py:58-67`): an active membership of the
/// caller in this project. Uncorrelated — every predicate is a literal;
/// the `pm` / `pmw` aliases keep it apart from the outer joins.
pub fn comment_is_member_expr(
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> SimpleExpr {
    let pm = || Alias::new("pm");
    let pmw = || Alias::new("pmw");
    Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from_as(project_members::Entity, pm())
            .join_as(
                JoinType::InnerJoin,
                workspaces::Entity,
                pmw(),
                Expr::col((pm(), project_members::Column::WorkspaceId))
                    .equals((pmw(), workspaces::Column::Id)),
            )
            .and_where(Expr::col((pm(), project_members::Column::DeletedAt)).is_null())
            .and_where(Expr::col((pm(), project_members::Column::IsActive)).eq(true))
            .and_where(Expr::col((pm(), project_members::Column::MemberId)).eq(user_id))
            .and_where(Expr::col((pm(), project_members::Column::ProjectId)).eq(project_id))
            .and_where(Expr::col((pmw(), workspaces::Column::Slug)).eq(slug))
            .limit(1)
            .to_owned(),
    )
}

/// Comment-list select (`comment.py:43-69`): `DISTINCT` (the member join
/// can fan out), model-ordered newest first. Base columns unprefixed,
/// then `is_member`, then the project / workspace / issue nests (no actor
/// join: the actor loads lazily). `filter_queryset`'s optional
/// `filterset_fields` (`issue__id`, `workspace__id`) and `get_object`'s
/// pk narrow the result through `.filter` (handler-owned).
pub fn comment_list_select(
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<issue_comments::Entity> {
    use issue_comments::Column as C;
    let select = issue_comments::Entity::find().distinct().expr_as(
        comment_is_member_expr(slug, project_id, user_id),
        "is_member",
    );
    select_nests(select)
        .join(
            JoinType::InnerJoin,
            issue_comments::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            issue_comments::Relation::Projects.def(),
        )
        .join(JoinType::InnerJoin, issue_comments::Relation::Issues.def())
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .filter(C::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(C::ProjectId.eq(project_id))
        .filter(C::IssueId.eq(issue_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(user_id))
        .order_by_desc(C::CreatedAt)
}

/// Comment-reaction-list select (`comment.py:186-200`): `DISTINCT`,
/// newest first, no `select_related`.
pub fn comment_reaction_list_select(
    slug: &str,
    project_id: uuid::Uuid,
    comment_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<comment_reactions::Entity> {
    use comment_reactions::Column as R;
    comment_reactions::Entity::find()
        .distinct()
        .join(
            JoinType::InnerJoin,
            comment_reactions::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            comment_reactions::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .filter(R::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(R::ProjectId.eq(project_id))
        .filter(R::CommentId.eq(comment_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::MemberId.eq(user_id))
        .order_by_desc(R::CreatedAt)
}

// ---------------------------------------------------------------------------
// Versions (version.py:36-144)
// ---------------------------------------------------------------------------

/// The 10 `.values()` keys in RENDER order (`version.py:48-59`,
/// `:120-131`) — the wire key order handlers render. FK fields keep
/// their field names as keys (`workspace`, not `workspace_id`).
pub const VERSION_PAGE_KEYS: &[&str] = &[
    "id",
    "workspace",
    "project",
    "issue",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

// ---------------------------------------------------------------------------
// Meta (base.py:1199-1211)
// ---------------------------------------------------------------------------

/// One meta row.
#[derive(Debug, FromQueryResult)]
pub struct MetaRow {
    pub sequence_id: i32,
    pub identifier: String,
}

/// Meta statement (`base.py:1201-1211`): the issue's `sequence_id` plus
/// its project's `identifier`, over the `issue_objects` manager scope
/// (triage/archived/draft rows are NOT visible here — the only lookup in
/// this module on the scoped manager).
///
/// Django fires two queries — the `.only("sequence_id",
/// "project__identifier")` main select plus a lazy `project.identifier`
/// load, because a spanning `.only()` without `select_related` never
/// joins — and this builder is their single-statement consolidation over
/// the join the scope already requires. Same two values, one round trip.
pub fn meta_select(slug: &str, project_id: Uuid, issue_id: Uuid) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.column((a("issue"), issues::Column::SequenceId))
        .column((a("project"), projects::Column::Identifier));
    scoped_issues(&mut stmt);
    workspace_slug(&mut stmt, slug);
    stmt.and_where(col("issue", issues::Column::Id).eq(issue_id))
        .and_where(col("issue", issues::Column::ProjectId).eq(project_id))
        .order_by((a("issue"), issues::Column::CreatedAt), Order::Desc)
        .limit(1);
    stmt
}

/// The 15 lite wire keys in `serialize_lite_issue` order
/// (`base.py:1223-1240`) — the key order handlers render. `created_by` /
/// `updated_by` select the `*_id` columns; `is_synced` is the predicate's
/// answer over the `is_synced` annotation (bug 2).
pub const LITE_KEYS: &[&str] = &[
    "id",
    "sequence_id",
    "name",
    "description_html",
    "sort_order",
    "project_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "is_epic",
    "is_intake",
    "is_synced",
    "archived_at",
];

/// All 14 concrete `workspaces` columns (`select_related("workspace")` order).
pub const WORKSPACE_FIELDS: [workspaces::Column; 14] = [
    workspaces::Column::CreatedAt,
    workspaces::Column::UpdatedAt,
    workspaces::Column::CreatedById,
    workspaces::Column::UpdatedById,
    workspaces::Column::DeletedAt,
    workspaces::Column::Id,
    workspaces::Column::Name,
    workspaces::Column::Logo,
    workspaces::Column::LogoAssetId,
    workspaces::Column::OwnerId,
    workspaces::Column::Slug,
    workspaces::Column::OrganizationSize,
    workspaces::Column::Timezone,
    workspaces::Column::BackgroundColor,
];

/// All 20 concrete `issue_agent_ticker` columns (`select_related("agent_ticker")`
/// order; matches D-10 `tasks_ticker::models::issue_agent_ticker::COLUMNS`).
pub const TICKER_FIELDS: [issue_agent_ticker::Column; 20] = [
    issue_agent_ticker::Column::CreatedAt,
    issue_agent_ticker::Column::UpdatedAt,
    issue_agent_ticker::Column::CreatedById,
    issue_agent_ticker::Column::UpdatedById,
    issue_agent_ticker::Column::DeletedAt,
    issue_agent_ticker::Column::Id,
    issue_agent_ticker::Column::IssueId,
    issue_agent_ticker::Column::Used,
    issue_agent_ticker::Column::Granted,
    issue_agent_ticker::Column::Waited,
    issue_agent_ticker::Column::UserDisabled,
    issue_agent_ticker::Column::NextRunAt,
    issue_agent_ticker::Column::LastTickAt,
    issue_agent_ticker::Column::Enabled,
    issue_agent_ticker::Column::DisarmReason,
    issue_agent_ticker::Column::PendingEntry,
    issue_agent_ticker::Column::PendingEntryFree,
    issue_agent_ticker::Column::PendingEntryActorId,
    issue_agent_ticker::Column::PendingEntryTrigger,
    issue_agent_ticker::Column::ResumeParentRunId,
];

/// Identifier-lite `.only()` columns in Django's RENDER order: `.only()`
/// honors model field order, not the call order. `external_source` rides
/// along for the `_issue_is_actively_synced` short-circuit (serializers
/// own the predicate; the queries layer keeps its inputs loaded).
pub const LITE_ISSUE_FIELDS: [issues::Column; 14] = [
    issues::Column::CreatedAt,
    issues::Column::UpdatedAt,
    issues::Column::CreatedById,
    issues::Column::UpdatedById,
    issues::Column::Id,
    issues::Column::ProjectId,
    issues::Column::WorkspaceId,
    issues::Column::Name,
    issues::Column::DescriptionHtml,
    issues::Column::SequenceId,
    issues::Column::SortOrder,
    issues::Column::ArchivedAt,
    issues::Column::IsDraft,
    issues::Column::ExternalSource,
];

/// Identifier-full output-name prefixes, one per joined table.
pub const FULL_ISSUE_PREFIX: &str = "i_";
pub const FULL_PROJECT_PREFIX: &str = "p_";
pub const FULL_WORKSPACE_PREFIX: &str = "w_";
pub const FULL_PARENT_PREFIX: &str = "par_";
pub const FULL_STATE_PREFIX: &str = "s_";
pub const FULL_TICKER_PREFIX: &str = "t_";

/// Project lookup by workspace-scoped identifier (`base.py:1253`):
/// `identifier__iexact` is `UPPER(identifier::text) = UPPER(<value>)`.
/// Unique per `(identifier, workspace)` among live rows (partial unique
/// plus `save()` upper-normalization), so `LIMIT 1` matches `.get()`.
pub fn identifier_project_select(slug: &str, project_identifier: &str) -> SelectStatement {
    let mut stmt = Query::select();
    select_fields(&mut stmt, "projects", &PROJECT_FIELDS, "");
    stmt.from(projects::Entity)
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((projects::Entity, projects::Column::WorkspaceId))
                .equals((workspaces::Entity, workspaces::Column::Id)),
        )
        .and_where(Expr::col((projects::Entity, projects::Column::DeletedAt)).is_null())
        .and_where(
            Expr::expr(Func::upper(
                Expr::col((projects::Entity, projects::Column::Identifier))
                    .cast_as(Alias::new("text")),
            ))
            .eq(Func::upper(Expr::val(project_identifier.to_owned()))),
        )
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .order_by((projects::Entity, projects::Column::CreatedAt), Order::Desc)
        .limit(1);
    stmt
}

/// Identifier member gate (`base.py:1256-1265`): ANY active membership
/// (no role filter — unlike the guest-view gate,
/// `queries_core::guest_view_select`). Reads one id; Django's `.exists()`.
pub fn identifier_member_select(
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
        .filter(workspaces::Column::Slug.eq(slug))
        .limit(1)
}

/// `is_intake` `Exists`, shared by the lite (`base.py:1294-1301`) and
/// full (`base.py:1436-1445`) lookups: a pending (`-2`) or snoozed (`0`)
/// intake row for this issue, correlated to the `issue` alias.
pub fn is_intake_exists(slug: &str, project_id: Uuid) -> SimpleExpr {
    let mut stmt = select_one_from(intake_issues::Entity, "ii");
    stmt.join_as(
        JoinType::InnerJoin,
        workspaces::Entity,
        a("iiw"),
        col("ii", intake_issues::Column::WorkspaceId).equals((a("iiw"), workspaces::Column::Id)),
    )
    .and_where(col("ii", intake_issues::Column::DeletedAt).is_null())
    .and_where(col("ii", intake_issues::Column::IssueId).equals((a("issue"), issues::Column::Id)))
    .and_where(col("ii", intake_issues::Column::ProjectId).eq(project_id))
    .and_where(col("ii", intake_issues::Column::Status).is_in([-2, 0]))
    .and_where(col("iiw", workspaces::Column::Slug).eq(slug))
    .limit(1);
    Expr::exists(stmt)
}

/// Lite `is_synced` `Exists` (`base.py:1303-1309`): bug 2 — git syncs
/// only, unscoped.
fn lite_is_synced() -> SimpleExpr {
    let mut stmt = select_one_from(git_issue_syncs::Entity, "gs");
    stmt.and_where(col("gs", git_issue_syncs::Column::DeletedAt).is_null())
        .and_where(
            col("gs", git_issue_syncs::Column::IssueId).equals((a("issue"), issues::Column::Id)),
        )
        .limit(1);
    Expr::exists(stmt)
}

/// `FROM issues AS issue INNER JOIN workspaces` over plain `Issue.objects`
/// (triage/archived/draft rows visible), this project, slug and sequence
/// id, newest first, one row.
fn identifier_issue_scope(
    stmt: &mut SelectStatement,
    slug: &str,
    project_id: Uuid,
    sequence_id: i32,
) {
    stmt.and_where(col("issue", issues::Column::DeletedAt).is_null())
        .and_where(col("issue", issues::Column::ProjectId).eq(project_id))
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .and_where(col("issue", issues::Column::SequenceId).eq(sequence_id))
        .order_by((a("issue"), issues::Column::CreatedAt), Order::Desc)
        .limit(1);
}

fn join_issue_workspace(stmt: &mut SelectStatement) {
    stmt.join(
        JoinType::InnerJoin,
        workspaces::Entity,
        col("issue", issues::Column::WorkspaceId)
            .equals((workspaces::Entity, workspaces::Column::Id)),
    );
}

/// Lite statement (`base.py:1267-1310`): the deferred 14-column row plus
/// `is_epic` / `is_intake` / `is_synced`.
pub fn identifier_lite_select(slug: &str, project_id: Uuid, sequence_id: i32) -> SelectStatement {
    let mut stmt = Query::select();
    select_fields(&mut stmt, "issue", &LITE_ISSUE_FIELDS, "");
    stmt.expr_as(
        Func::coalesce([
            Expr::col((issue_types::Entity, issue_types::Column::IsEpic)).into(),
            Expr::val(false).into(),
        ]),
        a("is_epic"),
    )
    .expr_as(is_intake_exists(slug, project_id), a("is_intake"))
    .expr_as(lite_is_synced(), a("is_synced"))
    .from_as(issues::Entity, a("issue"));
    join_issue_workspace(&mut stmt);
    stmt.join(
        JoinType::LeftJoin,
        issue_types::Entity,
        col("issue", issues::Column::TypeId).equals((issue_types::Entity, issue_types::Column::Id)),
    );
    identifier_issue_scope(&mut stmt, slug, project_id, sequence_id);
    stmt
}

/// Full identifier statement (`base.py:1312-1446`): all issue columns, the
/// seven annotations in `annotate()` order (counts coalesced to `0`),
/// `is_subscribed` and `is_intake`, then the `select_related("workspace",
/// "project", "state", "parent", "agent_ticker")` columns in Django's
/// rendering order (projects, workspaces, parent, states, ticker), each
/// table's columns under its `FULL_*_PREFIX`. The `assignees` / `labels` /
/// `issue_module__module` M2M prefetches are handler-owned.
pub fn identifier_full_select(
    slug: &str,
    project_id: Uuid,
    sequence_id: i32,
    user_id: Uuid,
) -> SelectStatement {
    let mut stmt = Query::select();
    select_fields(&mut stmt, "issue", &ISSUE_FIELDS, FULL_ISSUE_PREFIX);
    stmt.expr_as(cycle_id_subquery(), a("cycle_id"))
        .expr_as(coalesce_zero(link_count(true)), a("link_count"))
        .expr_as(coalesce_zero(attachment_count(true)), a("attachment_count"))
        .expr_as(
            coalesce_zero(sub_issues_count(true, false)),
            a("sub_issues_count"),
        )
        .expr_as(label_ids(), a("label_ids"))
        .expr_as(assignee_ids_active(), a("assignee_ids"))
        .expr_as(module_ids_unarchived(), a("module_ids"))
        .expr_as(is_subscribed(slug, project_id, user_id), a("is_subscribed"))
        .expr_as(is_intake_exists(slug, project_id), a("is_intake"));
    select_fields(&mut stmt, "projects", &PROJECT_FIELDS, FULL_PROJECT_PREFIX);
    select_fields(
        &mut stmt,
        "workspaces",
        &WORKSPACE_FIELDS,
        FULL_WORKSPACE_PREFIX,
    );
    select_fields(&mut stmt, "parent", &ISSUE_FIELDS, FULL_PARENT_PREFIX);
    select_fields(&mut stmt, "state", &STATE_FIELDS, FULL_STATE_PREFIX);
    select_fields(
        &mut stmt,
        "issue_agent_ticker",
        &TICKER_FIELDS,
        FULL_TICKER_PREFIX,
    );
    stmt.from_as(issues::Entity, a("issue")).join(
        JoinType::InnerJoin,
        projects::Entity,
        col("issue", issues::Column::ProjectId).equals((projects::Entity, projects::Column::Id)),
    );
    join_issue_workspace(&mut stmt);
    stmt.join_as(
        JoinType::LeftJoin,
        issues::Entity,
        a("parent"),
        col("issue", issues::Column::ParentId).equals((a("parent"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        states::Entity,
        a("state"),
        col("issue", issues::Column::StateId).equals((a("state"), states::Column::Id)),
    )
    .join(
        JoinType::LeftJoin,
        issue_agent_ticker::Entity,
        col("issue", issues::Column::Id).equals((
            issue_agent_ticker::Entity,
            issue_agent_ticker::Column::IssueId,
        )),
    );
    identifier_issue_scope(&mut stmt, slug, project_id, sequence_id);
    stmt
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLUG: &str = "ws-slug";
    const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    const ISSUE: uuid::Uuid = uuid::Uuid::from_u128(0x33333333_3333_3333_3333_333333333333);
    const USER: uuid::Uuid = uuid::Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);
    const COMMENT: uuid::Uuid = uuid::Uuid::from_u128(0x44444444_4444_4444_4444_444444444444);

    fn stmt_sql(stmt: &SelectStatement) -> String {
        stmt.to_string(sea_orm::sea_query::PostgresQueryBuilder)
    }

    // -- reuse pins ----------------------------------------------------------

    // -- history -------------------------------------------------------------

    fn built<E: sea_orm::EntityTrait>(select: Select<E>) -> String {
        use sea_orm::QueryTrait;
        select.build(sea_orm::DbBackend::Postgres).to_string()
    }

    /// Every `AS "<name>"` output alias of a built statement, in order.
    fn aliases(sql: &str) -> Vec<String> {
        sql.split(" AS \"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap_or_default().to_owned())
            .collect()
    }

    fn assert_unique(names: &[String]) {
        let mut sorted = names.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "duplicate output names: {names:?}"
        );
    }

    #[test]
    fn history_activities_matches_django_shape() {
        let sql = built(history_activities_select(SLUG, ISSUE, USER, None));
        // Base columns unprefixed, nests prefixed: unique by construction.
        assert!(sql.starts_with(r#"SELECT "issue_activities"."created_at","#));
        let names = aliases(&sql);
        assert_eq!(names.first().map(String::as_str), Some("p_id"));
        assert!(names.iter().any(|n| n == "w_slug"));
        assert!(names.iter().any(|n| n == "i_description_html"));
        assert_eq!(names.last().map(String::as_str), Some("u_display_name"));
        assert_unique(&names);
        // The negated IN keeps Django's null semantics.
        assert!(sql.contains(
            r#"NOT ("issue_activities"."field" IN ('comment', 'vote', 'reaction', 'draft') AND "issue_activities"."field" IS NOT NULL)"#
        ));
        // Joins: issues/projects/members/workspaces INNER, nullable actor LEFT.
        assert!(
            sql.contains(r#"INNER JOIN "issues" ON "issue_activities"."issue_id" = "issues"."id""#)
        );
        assert!(sql.contains(
            r#"INNER JOIN "projects" ON "issue_activities"."project_id" = "projects"."id""#
        ));
        assert!(sql.contains(
            r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#
        ));
        assert!(sql.contains(
            r#"INNER JOIN "workspaces" ON "issue_activities"."workspace_id" = "workspaces"."id""#
        ));
        assert!(
            sql.contains(r#"LEFT JOIN "users" ON "issue_activities"."actor_id" = "users"."id""#)
        );
        // Gate: archived-project exclusion, active membership, slug. The
        // member join carries no deleted_at guard (bug 4).
        assert!(sql.contains(r#""issue_activities"."deleted_at" IS NULL"#));
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#));
        assert!(sql.contains(r#""project_members"."is_active" = TRUE"#));
        assert!(sql.contains(&format!(r#""project_members"."member_id" = '{USER}'"#)));
        assert!(sql.contains(r#""workspaces"."slug" = 'ws-slug'"#));
        assert!(!sql.contains(r#""project_members"."deleted_at""#));
        assert!(sql.ends_with(r#"ORDER BY "issue_activities"."created_at" ASC"#));
        assert!(!sql.contains("DISTINCT"));
    }

    #[test]
    fn history_created_after_casts_to_timestamptz() {
        let sql = built(history_activities_select(
            SLUG,
            ISSUE,
            USER,
            Some("2026-01-02T03:04:05+00:00"),
        ));
        assert!(sql.contains(
            r#""issue_activities"."created_at" > CAST('2026-01-02T03:04:05+00:00' AS timestamptz)"#
        ));
        let sql = built(history_comments_select(SLUG, ISSUE, USER, None));
        assert!(!sql.contains("created_at\" >"));
        assert!(!sql.contains("\"field\""));
        assert!(sql.contains(r#"LEFT JOIN "users" ON "issue_comments"."actor_id" = "users"."id""#));
        assert!(sql.ends_with(r#"ORDER BY "issue_comments"."created_at" ASC"#));
        assert_unique(&aliases(&sql));
    }

    #[test]
    fn history_prefetches_match_django_shapes() {
        let reactions = built(history_reactions_prefetch_select(&[COMMENT]));
        // Non-null actor FK: INNER join; IN-list; model ordering.
        assert!(reactions
            .contains(r#"INNER JOIN "users" ON "comment_reactions"."actor_id" = "users"."id""#));
        assert!(reactions.contains(r#""users"."display_name" AS "u_display_name""#));
        assert!(reactions.contains(&format!(
            r#""comment_reactions"."comment_id" IN ('{COMMENT}')"#
        )));
        assert!(reactions.contains(r#""comment_reactions"."deleted_at" IS NULL"#));
        assert!(reactions.ends_with(r#"ORDER BY "comment_reactions"."created_at" DESC"#));

        let intake = built(history_intake_prefetch_select(&[ISSUE]));
        assert!(intake.starts_with(
            r#"SELECT "intake_issues"."id", "intake_issues"."source", "intake_issues"."source_email", "intake_issues"."extra" FROM "intake_issues""#
        ));
        assert!(intake.contains(&format!(r#""intake_issues"."issue_id" IN ('{ISSUE}')"#)));
        assert!(intake.contains(r#""intake_issues"."deleted_at" IS NULL"#));
        assert!(intake.ends_with(r#"ORDER BY "intake_issues"."created_at" DESC"#));
    }

    // -- comments ------------------------------------------------------------

    #[test]
    fn comment_list_has_distinct_member_annotation_and_related() {
        let sql = built(comment_list_select(SLUG, PROJECT, ISSUE, USER));
        assert!(sql.starts_with("SELECT DISTINCT"));
        // Annotation between base and select_related columns.
        let base_pos = sql.find(r#""issue_comments"."created_at""#).expect("base");
        let member_pos = sql.find(r#"AS "is_member""#).expect("is_member");
        let proj_pos = sql.find(r#"AS "p_id""#).expect("projects");
        assert!(
            base_pos < member_pos && member_pos < proj_pos,
            "comment select order wrong: {sql}"
        );
        assert!(sql.contains(&format!(r#""pm"."member_id" = '{USER}'"#)));
        assert!(sql.contains(&format!(r#""pm"."project_id" = '{PROJECT}'"#)));
        assert!(sql.contains(r#""pmw"."slug" = 'ws-slug'"#));
        assert!(sql.contains(r#""pm"."deleted_at" IS NULL"#));
        // No actor join on the list statement.
        assert!(!sql.contains(r#"JOIN "users""#));
        assert!(!aliases(&sql).iter().any(|name| name.starts_with("u_")));
        assert_unique(&aliases(&sql));
        assert!(sql.ends_with(r#"ORDER BY "issue_comments"."created_at" DESC"#));
    }

    #[test]
    fn comment_reaction_list_is_distinct_newest_first() {
        let sql = built(comment_reaction_list_select(SLUG, PROJECT, COMMENT, USER));
        assert!(sql.starts_with(r#"SELECT DISTINCT "comment_reactions"."created_at""#));
        assert!(sql.contains(&format!(
            r#""comment_reactions"."comment_id" = '{COMMENT}'"#
        )));
        assert!(sql.contains(&format!(r#""project_members"."member_id" = '{USER}'"#)));
        assert!(sql.contains(
            r#"INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id""#
        ));
        assert!(!sql.contains(" AS \""));
        assert!(sql.ends_with(r#"ORDER BY "comment_reactions"."created_at" DESC"#));
    }

    // -- meta ----------------------------------------------------------------

    #[test]
    fn meta_selects_both_values_over_manager_scope() {
        let sql = stmt_sql(&meta_select(SLUG, PROJECT, ISSUE));
        assert!(sql.starts_with(r#"SELECT "issue"."sequence_id", "project"."identifier" FROM"#));
        assert!(sql.contains(r#"NOT ("state"."group" = 'triage' AND "state"."group" IS NOT NULL)"#));
        assert!(sql.contains(r#""issue"."archived_at" IS NULL"#));
        assert!(sql.contains(r#""project"."archived_at" IS NULL"#));
        assert!(sql.contains(r#""issue"."is_draft" = FALSE"#));
        assert!(sql.ends_with(r#"ORDER BY "issue"."created_at" DESC LIMIT 1"#));
    }

    // -- identifier ----------------------------------------------------------

    #[test]
    fn identifier_project_uses_iexact_and_limit_1() {
        let sql = stmt_sql(&identifier_project_select(SLUG, "ENG"));
        assert!(
            sql.contains(r#"UPPER(CAST("projects"."identifier" AS text)) = UPPER('ENG')"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = 'ws-slug'"#));
        assert!(sql.ends_with(r#"ORDER BY "projects"."created_at" DESC LIMIT 1"#));
    }

    #[test]
    fn identifier_member_gate_has_no_role_filter() {
        use sea_orm::QueryTrait;
        let sql = identifier_member_select(SLUG, PROJECT, USER)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(!sql.contains("role"));
        assert!(!sql.contains("ORDER BY"));
        assert!(sql.ends_with("LIMIT 1"));
    }

    #[test]
    fn identifier_lite_selects_deferred_row_plus_three_flags() {
        let sql = stmt_sql(&identifier_lite_select(SLUG, PROJECT, 42));
        let pos = |needle: &str| {
            sql.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {sql}"))
        };
        assert!(pos(r#""issue"."external_source""#) < pos(r#"AS "is_epic""#));
        assert!(pos(r#"AS "is_epic""#) < pos(r#"AS "is_intake""#));
        assert!(pos(r#"AS "is_intake""#) < pos(r#"AS "is_synced""#));
        assert!(!sql.contains(r#""issue"."priority""#));
        assert!(sql.contains(r#"COALESCE("issue_types"."is_epic", FALSE) AS "is_epic""#));
        assert!(
            sql.contains(r#"LEFT JOIN "issue_types" ON "issue"."type_id" = "issue_types"."id""#)
        );
        assert!(sql.contains(r#""ii"."status" IN (-2, 0)"#));
        // Bug 2: git-only, unscoped.
        assert!(sql.contains(r#"FROM "git_issue_syncs" AS "gs""#));
        assert!(!sql.contains("github"));
        let outer = sql.rsplit(r#"AS "is_synced""#).next().expect("outer");
        assert!(
            !outer.contains("triage")
                && !outer.contains("archived_at")
                && !outer.contains("is_draft")
        );
        assert!(sql.ends_with(r#"ORDER BY "issue"."created_at" DESC LIMIT 1"#));
    }

    #[test]
    fn identifier_full_selects_row_annotations_and_related_in_order() {
        let sql = stmt_sql(&identifier_full_select(SLUG, PROJECT, 42, USER));
        let pos = |needle: &str| {
            sql.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {sql}"))
        };
        let order = [
            r#"AS "i_created_at""#,
            r#"AS "i_agent_executor""#,
            r#"AS "cycle_id""#,
            r#"AS "link_count""#,
            r#"AS "attachment_count""#,
            r#"AS "sub_issues_count""#,
            r#"AS "label_ids""#,
            r#"AS "assignee_ids""#,
            r#"AS "module_ids""#,
            r#"AS "is_subscribed""#,
            r#"AS "is_intake""#,
            r#"AS "p_created_at""#,
            r#"AS "w_created_at""#,
            r#"AS "par_created_at""#,
            r#"AS "s_created_at""#,
            r#"AS "t_resume_parent_run_id""#,
        ];
        for pair in order.windows(2) {
            assert!(
                pos(pair[0]) < pos(pair[1]),
                "{} before {}",
                pair[0],
                pair[1]
            );
        }
        assert!(sql.contains(r#"INNER JOIN "projects" ON "issue"."project_id" = "projects"."id""#));
        assert!(sql
            .contains(r#"INNER JOIN "workspaces" ON "issue"."workspace_id" = "workspaces"."id""#));
        assert!(sql
            .contains(r#"LEFT JOIN "issues" AS "parent" ON "issue"."parent_id" = "parent"."id""#));
        assert!(
            sql.contains(r#"LEFT JOIN "states" AS "state" ON "issue"."state_id" = "state"."id""#)
        );
        assert!(sql.contains(
            r#"LEFT JOIN "issue_agent_ticker" ON "issue"."id" = "issue_agent_ticker"."issue_id""#
        ));
        assert!(sql.ends_with(r#"ORDER BY "issue"."created_at" DESC LIMIT 1"#));
    }

    #[test]
    fn lite_keys_follow_serializer_order() {
        assert_eq!(
            LITE_KEYS,
            &[
                "id",
                "sequence_id",
                "name",
                "description_html",
                "sort_order",
                "project_id",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "is_draft",
                "is_epic",
                "is_intake",
                "is_synced",
                "archived_at",
            ]
        );
    }
}
