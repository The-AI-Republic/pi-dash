#![forbid(unsafe_code)]

//! Activity / comment / version / meta / identifier read querysets (D-26 queries-B).
//!
//! Ports the annotated read querysets behind the issue history, comment,
//! version, meta and identifier endpoints to SQL text, extending pilot-2's
//! api-crate query pattern (`super::queries_core` fragments, [`Binder`] `$n`
//! binds). Each builder returns a fragment
//! or a complete statement the handler executes; handlers own auth gates,
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
//! exclusions). [`meta_sql`] carries the scope, the identifier builders
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
//!   [`meta_sql`] is one statement selecting both values over the join the
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

use super::queries_core::{
    attachment_count_select, is_subscribed_select, link_count_select, sub_issues_count_select,
    ASSIGNEE_IDS_ACTIVE_SELECT, CYCLE_ID_SELECT, ISSUE_COLUMNS, LABEL_IDS_SELECT,
    MODULE_IDS_SELECT, PROJECT_COLUMNS, STATE_COLUMNS,
};
use super::Binder;

use pidash_db::entity::{
    comment_reactions, intake_issues, issue_activities, issue_comments, issues, project_members,
    projects, users, workspaces,
};
use sea_orm::sea_query::{Alias, Expr, Query, SimpleExpr};
use sea_orm::{
    ColumnTrait, EntityTrait, IdenStatic, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationDef, RelationTrait, Select,
};

// ---------------------------------------------------------------------------
// Shared column lists (Django concrete-field order, live-compiled)
// ---------------------------------------------------------------------------

/// All 14 concrete `workspaces` columns (`select_related("workspace")` order).
pub const WORKSPACE_COLUMNS: &str = "workspaces.created_at, workspaces.updated_at, \
    workspaces.created_by_id, workspaces.updated_by_id, workspaces.deleted_at, workspaces.id, \
    workspaces.name, workspaces.logo, workspaces.logo_asset_id, workspaces.owner_id, \
    workspaces.slug, workspaces.organization_size, workspaces.timezone, \
    workspaces.background_color";

/// All 20 concrete `issue_agent_ticker` columns (`select_related("agent_ticker")`
/// order; matches D-10 `tasks_ticker::models::issue_agent_ticker::COLUMNS`).
pub const TICKER_COLUMNS: &str = "issue_agent_ticker.created_at, issue_agent_ticker.updated_at, \
    issue_agent_ticker.created_by_id, issue_agent_ticker.updated_by_id, \
    issue_agent_ticker.deleted_at, issue_agent_ticker.id, issue_agent_ticker.issue_id, \
    issue_agent_ticker.used, issue_agent_ticker.granted, issue_agent_ticker.waited, \
    issue_agent_ticker.user_disabled, issue_agent_ticker.next_run_at, \
    issue_agent_ticker.last_tick_at, issue_agent_ticker.enabled, \
    issue_agent_ticker.disarm_reason, issue_agent_ticker.pending_entry, \
    issue_agent_ticker.pending_entry_free, issue_agent_ticker.pending_entry_actor_id, \
    issue_agent_ticker.pending_entry_trigger, issue_agent_ticker.resume_parent_run_id";

/// All 34 concrete `issues` columns under the `parent` self-join alias
/// (identifier-full `select_related("parent")`; Django renders `T4`).
pub const PARENT_ISSUE_COLUMNS: &str = "parent.created_at, parent.updated_at, \
    parent.created_by_id, parent.updated_by_id, parent.deleted_at, parent.id, \
    parent.project_id, parent.workspace_id, parent.parent_id, parent.state_id, parent.point, \
    parent.estimate_point_id, parent.name, parent.description_json, parent.description_html, \
    parent.description_stripped, parent.description_binary, parent.priority, \
    parent.complexity_score, parent.start_date, parent.target_date, parent.sequence_id, \
    parent.sort_order, parent.completed_at, parent.archived_at, parent.is_draft, \
    parent.external_source, parent.external_id, parent.type_id, parent.git_work_branch, \
    parent.workpad, parent.created_via, parent.assigned_pod_id, parent.agent_executor";

/// Identifier-lite `.only()` select list in Django's RENDER order: `.only()`
/// honors model field order, not the call order. `external_source` rides
/// along for the `_issue_is_actively_synced` short-circuit (serializers
/// own the predicate; the queries layer keeps its inputs loaded).
pub const LITE_ISSUE_COLUMNS: &str = "issue.created_at, issue.updated_at, issue.created_by_id, \
    issue.updated_by_id, issue.id, issue.project_id, issue.workspace_id, issue.name, \
    issue.description_html, issue.sequence_id, issue.sort_order, issue.archived_at, \
    issue.is_draft, issue.external_source";

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

/// Full meta statement (`base.py:1201-1211`): the issue's `sequence_id`
/// plus its project's `identifier`, over the `issue_objects` manager
/// scope (triage/archived/draft rows are NOT visible here — the only
/// lookup in this module on the scoped manager).
///
/// Django fires two queries — the `.only("sequence_id",
/// "project__identifier")` main select plus a lazy `project.identifier`
/// load, because a spanning `.only()` without `select_related` never
/// joins — and this builder is their single-statement consolidation over
/// the join the scope already requires. Same two values, one round trip.
/// Binds, in order: issue id, project id, workspace slug.
pub fn meta_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
) -> String {
    let issue = binder.bind_uuid(issue_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT issues.sequence_id, projects.identifier FROM issues \
        LEFT OUTER JOIN states ON issues.state_id = states.id \
        INNER JOIN projects ON issues.project_id = projects.id \
        INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
        WHERE issues.deleted_at IS NULL \
        AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
        AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
        AND issues.is_draft = FALSE \
        AND issues.id = {issue} AND issues.project_id = {project} \
        AND workspaces.slug = {slug_holder} \
        ORDER BY issues.created_at DESC LIMIT 1"
    )
}

// ---------------------------------------------------------------------------
// Identifier (base.py:1214-1489)
// ---------------------------------------------------------------------------

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

/// Project lookup by workspace-scoped identifier (`base.py:1253`):
/// `identifier__iexact` renders `UPPER(identifier::text) = UPPER($n)`.
/// Unique per `(identifier, workspace)` among live rows (partial unique
/// plus `save()` upper-normalization), so `LIMIT 1` matches `.get()`.
/// Binds, in order: project identifier, workspace slug.
pub fn identifier_project_sql(binder: &mut Binder, slug: &str, project_identifier: &str) -> String {
    let identifier = binder.bind_string(project_identifier.to_owned());
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT {PROJECT_COLUMNS} FROM projects \
        INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
        WHERE projects.deleted_at IS NULL AND UPPER(projects.identifier::text) = UPPER({identifier}) \
        AND workspaces.slug = {slug_holder} \
        ORDER BY projects.created_at DESC LIMIT 1"
    )
}

/// Identifier member gate `Exists` (`base.py:1256-1265`): ANY active
/// membership (no role filter — unlike the guest-view gate, which is
/// `queries_core::guest_view_exists_sql`). Django's `.exists()` renders
/// `SELECT 1 ... LIMIT 1` with no `ORDER BY`. Binds, in order: user id,
/// project id, workspace slug.
pub fn identifier_member_exists_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> String {
    let user = binder.bind_uuid(user_id);
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "SELECT 1 AS a FROM project_members \
        INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
        WHERE project_members.deleted_at IS NULL AND project_members.is_active \
        AND project_members.member_id = {user} AND project_members.project_id = {project} \
        AND workspaces.slug = {slug_holder} LIMIT 1"
    )
}

/// `is_intake` `Exists`, shared by the lite (`base.py:1294-1301`) and
/// full (`base.py:1436-1445`) lookups: a pending (`-2`) or snoozed (`0`)
/// intake row for this issue. Correlated to the `issue` alias both
/// statements use. Binds, in order: project id, workspace slug.
pub fn is_intake_exists_select(binder: &mut Binder, slug: &str, project_id: uuid::Uuid) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    format!(
        "EXISTS(SELECT 1 AS a FROM intake_issues ii \
        INNER JOIN workspaces iiw ON ii.workspace_id = iiw.id \
        WHERE ii.deleted_at IS NULL AND ii.issue_id = issue.id AND ii.project_id = {project} \
        AND ii.status IN (-2, 0) AND iiw.slug = {slug_holder} LIMIT 1) AS is_intake"
    )
}

/// Lite `is_synced` `Exists` (`base.py:1303-1309`): bug 2 — git syncs
/// only, unscoped. No binds, so a plain const.
pub const LITE_IS_SYNCED_SELECT: &str = "EXISTS(SELECT 1 AS a FROM git_issue_syncs gs \
    WHERE gs.deleted_at IS NULL AND gs.issue_id = issue.id LIMIT 1) AS is_synced";

/// Lite `is_epic` (`base.py:1288-1293`): the nullable type flag coalesced
/// to `FALSE`, over the `issue_types` left join. No binds.
pub const LITE_IS_EPIC_SELECT: &str = "COALESCE(issue_types.is_epic, FALSE) AS is_epic";

/// Bind a 32-bit integer (issue `sequence_id`) for a `$n` placeholder.
fn bind_sequence_id(binder: &mut Binder, sequence_id: i32) -> String {
    binder.bind(sea_query::Value::Int(Some(sequence_id)))
}

/// Full lite statement (`base.py:1267-1310`): the deferred 14-column row
/// plus `is_epic` / `is_intake` / `is_synced`, over plain
/// `Issue.objects` (triage/archived/draft rows visible). Binds, in order:
/// project id, workspace slug, sequence id (outer `WHERE`), then the
/// `is_intake` binds (project id, slug).
pub fn identifier_lite_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    sequence_id: i32,
) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let sequence = bind_sequence_id(binder, sequence_id);
    let is_intake = is_intake_exists_select(binder, slug, project_id);
    format!(
        "SELECT {LITE_ISSUE_COLUMNS}, {LITE_IS_EPIC_SELECT}, {is_intake}, {LITE_IS_SYNCED_SELECT} \
        FROM issues AS issue \
        INNER JOIN workspaces ON issue.workspace_id = workspaces.id \
        LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id \
        WHERE issue.deleted_at IS NULL AND issue.project_id = {project} \
        AND workspaces.slug = {slug_holder} AND issue.sequence_id = {sequence} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

/// Full identifier statement (`base.py:1312-1446`): all issue columns, the
/// seven annotations in `annotate()` order (counts coalesced to `0`,
/// arrays over the doubled-guard single-port form), `is_subscribed` and
/// `is_intake`, then the `select_related("workspace", "project",
/// "state", "parent", "agent_ticker")` columns in Django's field-order
/// rendering (projects, workspaces, parent, states, ticker) — Django's
/// exact select order. The `assignees` / `labels` / `issue_module__module`
/// M2M prefetches are separate queries, handler-owned; the
/// reaction/link prefetches reuse
/// `queries_core::{reaction_prefetch_sql, link_prefetch_sql}`. Binds, in
/// order: project id, workspace slug, sequence id (outer `WHERE`), then
/// the `is_subscribed` binds (project id, subscriber id, slug), then the
/// `is_intake` binds (project id, slug).
pub fn identifier_full_sql(
    binder: &mut Binder,
    slug: &str,
    project_id: uuid::Uuid,
    sequence_id: i32,
    user_id: uuid::Uuid,
) -> String {
    let project = binder.bind_uuid(project_id);
    let slug_holder = binder.bind_string(slug.to_owned());
    let sequence = bind_sequence_id(binder, sequence_id);
    let subscribed = is_subscribed_select(binder, slug, project_id, user_id);
    let is_intake = is_intake_exists_select(binder, slug, project_id);
    let link = link_count_select(true);
    let attachment = attachment_count_select(true);
    let sub = sub_issues_count_select(true);
    format!(
        "SELECT {ISSUE_COLUMNS}, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, \
        {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, {MODULE_IDS_SELECT}, \
        {subscribed}, {is_intake}, \
        {PROJECT_COLUMNS}, {WORKSPACE_COLUMNS}, {PARENT_ISSUE_COLUMNS}, {STATE_COLUMNS}, \
        {TICKER_COLUMNS} \
        FROM issues AS issue \
        INNER JOIN projects ON issue.project_id = projects.id \
        INNER JOIN workspaces ON issue.workspace_id = workspaces.id \
        LEFT OUTER JOIN issues AS parent ON issue.parent_id = parent.id \
        LEFT OUTER JOIN states AS state ON issue.state_id = state.id \
        LEFT OUTER JOIN issue_agent_ticker ON issue.id = issue_agent_ticker.issue_id \
        WHERE issue.deleted_at IS NULL AND issue.project_id = {project} \
        AND workspaces.slug = {slug_holder} AND issue.sequence_id = {sequence} \
        ORDER BY issue.created_at DESC LIMIT 1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLUG: &str = "ws-slug";
    const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    const ISSUE: uuid::Uuid = uuid::Uuid::from_u128(0x33333333_3333_3333_3333_333333333333);
    const USER: uuid::Uuid = uuid::Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);
    const COMMENT: uuid::Uuid = uuid::Uuid::from_u128(0x44444444_4444_4444_4444_444444444444);

    /// Collapse runs of whitespace so multi-line builders compare cleanly.
    fn flat(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // -- reuse pins ----------------------------------------------------------

    #[test]
    fn shared_fragments_are_queries_core_text() {
        // Referenced, not copied: the full statement embeds the shared
        // fragments byte-identically, so a fix in queries_core lands here
        // with no second edit.
        let mut binder = Binder::new();
        let sql = identifier_full_sql(&mut binder, SLUG, PROJECT, 42, USER);
        for fragment in [
            CYCLE_ID_SELECT,
            &link_count_select(true),
            &attachment_count_select(true),
            &sub_issues_count_select(true),
            LABEL_IDS_SELECT,
            ASSIGNEE_IDS_ACTIVE_SELECT,
            MODULE_IDS_SELECT,
            ISSUE_COLUMNS,
            STATE_COLUMNS,
            PROJECT_COLUMNS,
        ] {
            assert!(
                flat(&sql).contains(&flat(fragment)),
                "full statement drifted from shared fragment: {fragment}"
            );
        }
    }

    #[test]
    fn shared_prefetches_and_guest_gate_are_reused() {
        // The identifier reaction/link prefetches and the guest-view gate
        // are queries_core builders — assert the reuse compiles to the
        // same text the retrieve path uses.
        let mut binder = Binder::new();
        let reactions = super::super::queries_core::reaction_prefetch_sql(&mut binder, &[ISSUE]);
        assert!(reactions.contains("issue_reactions.issue_id IN ($1)"));
        let mut binder = Binder::new();
        let links = super::super::queries_core::link_prefetch_sql(&mut binder, &[ISSUE]);
        assert!(links.contains("issue_links.issue_id IN ($1)"));
        let mut binder = Binder::new();
        let guest =
            super::super::queries_core::guest_view_exists_sql(&mut binder, SLUG, PROJECT, USER);
        assert!(guest.contains("project_members.role = 5"));
        assert_eq!(binder.values().len(), 3);
    }

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
        let mut binder = Binder::new();
        let sql = meta_sql(&mut binder, SLUG, PROJECT, ISSUE);
        // Single-statement consolidation of Django's two queries.
        assert!(flat(&sql).starts_with("SELECT issues.sequence_id, projects.identifier FROM"));
        // issue_objects scope: triage exclusion, both archived guards, drafts.
        assert!(sql.contains("NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL)"));
        assert!(sql.contains("issues.archived_at IS NULL"));
        assert!(sql.contains("projects.archived_at IS NULL"));
        assert!(sql.contains("issues.is_draft = FALSE"));
        assert!(sql.ends_with("ORDER BY issues.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 3);
    }

    // -- identifier ----------------------------------------------------------

    #[test]
    fn identifier_project_uses_iexact_and_limit_1() {
        let mut binder = Binder::new();
        let sql = identifier_project_sql(&mut binder, SLUG, "ENG");
        assert!(sql.contains("UPPER(projects.identifier::text) = UPPER($1)"));
        assert!(sql.contains("workspaces.slug = $2"));
        assert!(sql.ends_with("ORDER BY projects.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 2);
    }

    #[test]
    fn identifier_member_gate_has_no_role_filter() {
        let mut binder = Binder::new();
        let sql = identifier_member_exists_sql(&mut binder, SLUG, PROJECT, USER);
        assert!(sql.starts_with("SELECT 1 AS a"));
        assert!(!sql.contains("role"));
        assert!(!sql.contains("ORDER BY"));
        assert!(sql.ends_with("LIMIT 1"));
        assert_eq!(binder.values().len(), 3);
    }

    #[test]
    fn identifier_lite_selects_deferred_row_plus_three_flags() {
        let mut binder = Binder::new();
        let sql = identifier_lite_sql(&mut binder, SLUG, PROJECT, 42);
        // Select order: deferred columns, then annotations in annotate() order.
        let base_pos = sql.find("issue.created_at").expect("base");
        let epic_pos = sql.find("AS is_epic").expect("epic");
        let intake_pos = sql.find("AS is_intake").expect("intake");
        let synced_pos = sql.find("AS is_synced").expect("synced");
        assert!(
            base_pos < epic_pos && epic_pos < intake_pos && intake_pos < synced_pos,
            "lite select order wrong: {sql}"
        );
        // Deferred shape: 14 columns incl. external_source (predicate input).
        assert!(sql.contains("issue.external_source, COALESCE"));
        assert!(!sql.contains("issue.priority"));
        // is_epic over the type left join; is_intake pending/snoozed only.
        assert!(sql.contains("COALESCE(issue_types.is_epic, FALSE) AS is_epic"));
        assert!(sql.contains("LEFT OUTER JOIN issue_types ON issue.type_id = issue_types.id"));
        assert!(sql.contains("ii.status IN (-2, 0)"));
        // Bug 2: git-only, unscoped — no Github table, no project predicate.
        assert!(sql.contains("FROM git_issue_syncs gs"));
        assert!(!flat(&sql).contains("github"));
        let synced_from = sql.find("FROM git_issue_syncs").expect("sync from");
        let synced_end = sql.find("AS is_synced").expect("sync end");
        assert!(!sql[synced_from..synced_end].contains("project_id"));
        // Plain manager: no triage/archived/draft guards on the outer query.
        let outer = sql.split("FROM issues AS issue").nth(1).expect("outer");
        assert!(!outer.contains("triage"));
        assert!(!outer.contains("archived_at"));
        assert!(!outer.contains("is_draft"));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 5);
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

    /// Dump every builder plus the column consts for the `/tmp/replay-649`
    /// fixture replay (live Django 4.2.30 SQL vs these builders). Prints
    /// `@@ name @@` / SQL / `@@ binds: N @@` per statement (queries-A
    /// precedent); the replay script, not this test, compares.
    #[test]
    fn dump_statements_for_fixture_replay() {
        let mut statements: Vec<(&str, String, usize)> = Vec::new();
        macro_rules! dump {
            ($name:expr, $call:expr) => {{
                let mut binder = Binder::new();
                let sql = $call(&mut binder);
                statements.push(($name, sql, binder.values().len()));
            }};
        }
        dump!("meta", |binder: &mut Binder| {
            meta_sql(binder, SLUG, PROJECT, ISSUE)
        });
        dump!("identifier_project", |binder: &mut Binder| {
            identifier_project_sql(binder, SLUG, "ENG")
        });
        dump!("identifier_member_exists", |binder: &mut Binder| {
            identifier_member_exists_sql(binder, SLUG, PROJECT, USER)
        });
        dump!("identifier_lite", |binder: &mut Binder| {
            identifier_lite_sql(binder, SLUG, PROJECT, 42)
        });
        dump!("identifier_full", |binder: &mut Binder| {
            identifier_full_sql(binder, SLUG, PROJECT, 42, USER)
        });
        for (name, sql, binds) in &statements {
            println!("@@ {name} @@\n{sql}\n@@ binds: {binds} @@");
        }
        for (name, cols) in [
            ("cols_workspace", WORKSPACE_COLUMNS),
            ("cols_ticker", TICKER_COLUMNS),
            ("cols_parent_issue", PARENT_ISSUE_COLUMNS),
            ("cols_lite_issue", LITE_ISSUE_COLUMNS),
        ] {
            println!("@@ {name} @@\n{cols}");
        }
    }

    #[test]
    fn identifier_full_selects_row_annotations_and_related_in_order() {
        let mut binder = Binder::new();
        let sql = identifier_full_sql(&mut binder, SLUG, PROJECT, 42, USER);
        let issue_pos = sql.find("issue.created_at").expect("issue");
        let cycle_pos = sql.find("AS cycle_id").expect("cycle");
        let link_pos = sql.find("AS link_count").expect("link");
        let attach_pos = sql.find("AS attachment_count").expect("attach");
        let sub_pos = sql.find("AS sub_issues_count").expect("sub");
        let label_pos = sql.find("AS label_ids").expect("labels");
        let assignee_pos = sql.find("AS assignee_ids").expect("assignees");
        let module_pos = sql.find("AS module_ids").expect("modules");
        let subscribed_pos = sql.find("AS is_subscribed").expect("subscribed");
        let intake_pos = sql.find("AS is_intake").expect("intake");
        let proj_pos = sql.find("projects.created_at").expect("projects");
        let ws_pos = sql.find("workspaces.created_at").expect("workspaces");
        let parent_pos = sql.find("parent.created_at").expect("parent");
        let state_pos = sql.find("state.created_at").expect("state");
        let ticker_pos = sql.find("issue_agent_ticker.created_at").expect("ticker");
        assert!(
            issue_pos < cycle_pos
                && cycle_pos < link_pos
                && link_pos < attach_pos
                && attach_pos < sub_pos
                && sub_pos < label_pos
                && label_pos < assignee_pos
                && assignee_pos < module_pos
                && module_pos < subscribed_pos
                && subscribed_pos < intake_pos
                && intake_pos < proj_pos
                && proj_pos < ws_pos
                && ws_pos < parent_pos
                && parent_pos < state_pos
                && state_pos < ticker_pos,
            "full select order wrong: {sql}"
        );
        // Counts coalesce to 0 on this path (unlike retrieve's NULL).
        assert!(sql.contains("COALESCE((SELECT NULLIF(COUNT(*), 0)"));
        // Joins: projects/workspaces INNER, parent/state/ticker LEFT.
        assert!(sql.contains("INNER JOIN projects ON issue.project_id = projects.id"));
        assert!(sql.contains("INNER JOIN workspaces ON issue.workspace_id = workspaces.id"));
        assert!(sql.contains("LEFT OUTER JOIN issues AS parent ON issue.parent_id = parent.id"));
        assert!(sql.contains("LEFT OUTER JOIN states AS state ON issue.state_id = state.id"));
        assert!(sql.contains(
            "LEFT OUTER JOIN issue_agent_ticker ON issue.id = issue_agent_ticker.issue_id"
        ));
        assert!(sql.ends_with("ORDER BY issue.created_at DESC LIMIT 1"));
        assert_eq!(binder.values().len(), 8);
    }
}
