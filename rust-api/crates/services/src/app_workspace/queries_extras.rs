#![forbid(unsafe_code)]

//! Workspace extras query builders: lists, favorites, drafts, quick-links,
//! stickies, prefs, visits (D-24, stage 5).
//!
//! Ports the queryset and write shapes behind the workspace list endpoints,
//! user favorites, draft issues (including draft-to-issue), quick-links,
//! stickies, home/sidebar preferences and recent visits as SQL text,
//! following the D-24 precedent (`queries_membership.rs`, PIDASHCONV-609):
//! each builder returns a fragment (or a representative statement) the
//! caller splices into the statement it executes. The services crate carries
//! no `sea-query`/`sqlx` dependency (foundation crates are read-only for
//! port agents), so placeholders stay symbolic — `:slug`, `:user`, `:pk`,
//! `:ws`, `:now`, `:fid`, `:key`, `:draft`, `:query`, `:entity`,
//! `:entity_type`, `:entity_identifier`, `:estimate_ids`, `:module_ids`,
//! `:pinned`, `:sort` — exactly the notation the fixtures use; handlers
//! bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/workspace/label.py:17-30` — labels list.
//! - `app/views/workspace/state.py:17-41` — states list + in-memory order.
//! - `app/views/workspace/estimate.py:17-32` — estimates two-query list.
//! - `app/views/workspace/cycle.py:19-104` — cycles list + 6 annotations.
//! - `app/views/workspace/module.py:19-111` — modules list + 6 annotations.
//! - `app/views/workspace/favorite.py:20-97` — favorites CRUD + group get.
//! - `app/views/workspace/draft.py:46-312` — drafts queryset/list/create/
//!   patch/retrieve/destroy + draft-to-issue.
//! - `app/views/workspace/quick_link.py:16-65` — owner-scoped CRUD.
//! - `app/views/workspace/sticky.py:16-60` — stickies queryset/list/create +
//!   stock patch/destroy.
//! - `app/views/workspace/home.py:17-79` — home prefs autocreate + patch.
//! - `app/views/workspace/user_preference.py:18-101` — sidebar prefs
//!   autocreate + patch.
//! - `app/views/workspace/recent_visit.py:17-36` — recent-visits list.
//! - `db/models/{label,state,estimate,cycle,module,project}.py` — list-domain
//!   tables, managers, orderings.
//! - `db/models/{favorite,draft,recent_visit,sticky,workspace}.py` —
//!   extras tables, orderings, key enums.
//! - `db/models/asset.py:28-67` — `FileAsset` (`entity_type` contexts).
//! - `db/mixins.py:48-82` — soft-delete manager (`deleted_at IS NULL`) and
//!   soft-by-default `.delete()`.
//! - `app/views/base.py:84-108` — `BaseViewSet.get_queryset` (`model.objects.all()`).
//! - `utils/paginator.py:642-660` — paginate defaults (`default_per_page=1000`).
//!
//! Fixture oracle: F-W24-12, extras part (`fixtures/app_workspace/queries/`
//! `extras_user.sql` R1-R12 + `extras_user.rows.json` cases `labels` …
//! `recent_visits`). The unit tests below pin the builders against those
//! files so transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. R4/R5: `.order_by(self.kwargs.get("order_by", "-created_at"))`
//!    (`module.py:107`, `cycle.py:100`) — URL kwargs never hold `order_by`,
//!    so `?order_by=` is IGNORED and the order is ALWAYS `-created_at`.
//! 2. R5 vs R4: the cycle counts lack `distinct=True`, count
//!    `issue_cycle__issue__state__group` for the five group counts (vs the
//!    link for the total and for every module count), and carry an extra
//!    `issue.deleted_at` filter the module counts lack — the asymmetry is
//!    kept, never unified.
//! 3. R2: `state.order = index / count` (`state.py:34-38`) is an in-memory
//!    mutation that is never saved; the serializer reads the mutated values.
//! 4. R6: the GET branch (`favorite.py:27-28`) reads `(project-null AND NOT
//!    page) OR (project-member)` because `&` binds tighter than `|` —
//!    project-linked page favorites PASS via the right branch. The group
//!    GET (`:88-95`) has NO page exclusion at all.
//! 5. R6: the POST dedupe lookup (`:44-49`) filters workspace + user +
//!    entity type/identifier only — NO project and NO parent filter.
//! 6. R7: the cycle subquery (`draft.py:55-60`) is `[:1]` with NO `ORDER BY`
//!    — the picked cycle is nondeterministic when several links are alive.
//! 7. R7: the assignee `ArrayAgg` guard (`:76`) requires ANY active project
//!    membership of the assignee — there is NO project scoping.
//! 8. R8: the quick-link 404 key differs — `partial_update` answers
//!    `{"detail": ...}` (`:43`) while `retrieve` answers `{"error": ...}`
//!    (`:52`).
//! 9. R9: `retrieve` is NOT overridden on the sticky viewset, so it falls
//!    through to the `ModelViewSet` default — authenticated-only, with NO
//!    `allow_permission` workspace-membership/role gate (row scoping via
//!    `get_queryset` still applies).
//! 10. R10/R11: the autocreate `bulk_create` runs INSIDE the per-key loop
//!     over the GROWING key list with `ignore_conflicts=True`, and the
//!     `values_list("key")` existence check re-queries per key (N+1) —
//!     effective rows are first-insert-wins with shifted sort orders.
//! 11. R11: the sidebar PATCH lookup (`user_preference.py:88`) has NO user
//!     filter (vs the home PATCH, `home.py:69`) — a member can rewrite
//!     ANOTHER user's row.
//! 12. R10: a missing home pref answers 400 `{"Detail": "Preference not
//!     found"}` (`home.py:79`) — 400, not 404, and note the capital `D`.
//! 13. R12: the HARD `entity_name__in=["issue", "page", "project"]` clamp
//!     (`recent_visit.py:33`) applies AFTER the optional `?entity_name=`
//!     filter — a non-listed entity name yields `[]`.
//! 14. R3: the estimates id query (`estimate.py:23-25`) has NO member
//!     scoping — any project in the workspace qualifies.
//!
//! Code truths the fixture under-records (ported from the code; the gaps
//! are listed in the PR, and the tests pin the code shape, not the gap):
//! - The draft create re-read (R7) is a 21-key `.values()`
//!   (`draft.py:127-149`); the fixture says 19 in both files.
//! - The draft destroy (R7) is SOFT (`SoftDeleteModel.delete` defaults to
//!   `soft=True`; `db/models/draft.py` has no override); the `.sql` says
//!   SOFT but `rows.json` says "hard delete()".
//! - The states list (R2) additionally excludes `group = 'triage'` via the
//!   `StateManager` (`db/models/state.py:79-83`) and orders by `sequence`
//!   ASC (`Meta.ordering`); the fixture records neither.
//! - The labels (R1), estimates (R3), favorites (R6), quick-link list (R8)
//!   and home-pref response (R10) carry their models' `Meta.ordering`
//!   (`-created_at`, except estimates `name` ASC); the fixture is silent.
//!
//! Out of scope (owned by sibling issues): the user-account / token /
//! timezone part of F-W24-12 (queries E, PIDASHCONV-612); workspace core +
//! dashboard reads (queries A, PIDASHCONV-608); profile/issues/stats reads
//! (queries C, PIDASHCONV-610); response envelopes and error bodies
//! (handlers G/H/I, PIDASHCONV-621/622/623); serializer shapes
//! (PIDASHCONV-600…604, plus D-25/D-26 for state/estimate/label/draft
//! serializers); model column lists (PIDASHCONV-605/606/607, plus
//! `db::v1_assets::Sticky`); permission gates, throttles and cache sites
//! (PIDASHCONV-613); Celery enqueues (PIDASHCONV-614, incl. the three
//! draft-to-issue `issue_activity` sites); the `issue_filters` legacy
//! filter compiler (dynamic — handlers/D-26 compile it).

use super::queries_membership::{workspace_id_by_slug_sql, workspace_lookup_where};

// ---------------------------------------------------------------------------
// R1 labels list (label.py:22-30)
// ---------------------------------------------------------------------------

/// R1 scope (`:23-28`): `workspace__slug=:slug` + project-member active +
/// project unarchived, over the soft-delete manager. Cross-FK filters carry
/// NO related-manager scope, so only `labels` gets a `deleted_at` guard.
/// The member FK is nullable; `= :user` excludes NULLs naturally.
pub fn label_scope_where() -> String {
    format!(
        "labels.workspace_id = {} AND pm.member_id = :user AND pm.is_active = TRUE AND projects.archived_at IS NULL AND labels.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R1 list order: `Label.Meta.ordering = ("-created_at",)`
/// (`db/models/label.py:43`) — the view has no explicit `order_by`.
pub const LABEL_LIST_ORDER_SQL: &str = "labels.created_at DESC";

/// Full representative R1 SELECT: scope + joins (`Label.project` is
/// non-nullable, so both joins are inner) + default ordering.
pub fn label_list_sql() -> String {
    format!(
        "SELECT labels.* FROM labels JOIN projects ON projects.id = labels.project_id JOIN project_members pm ON pm.project_id = projects.id WHERE {} ORDER BY {}",
        label_scope_where(),
        LABEL_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R2 states list (state.py:21-41)
// ---------------------------------------------------------------------------

/// R2 scope (`:22-28`): the R1 predicate set plus `is_triage=False`, plus
/// the `StateManager` exclusion of `group = 'triage'`
/// (`db/models/state.py:79-83`, `.exclude()` renders `NOT (...)`) — note
/// the manager reads the `group` column while the view reads the separate
/// `is_triage` column; both apply.
pub fn state_scope_where() -> String {
    format!(
        "states.workspace_id = {} AND pm.member_id = :user AND pm.is_active = TRUE AND projects.archived_at IS NULL AND states.is_triage = FALSE AND NOT (states.group = 'triage') AND states.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R2 list order: `State.Meta.ordering = ("sequence",)`
/// (`db/models/state.py:128`) — ascending. Row order feeds the per-group
/// regroup below (dict insertion follows row order).
pub const STATE_LIST_ORDER_SQL: &str = "states.sequence ASC";

/// Full representative R2 SELECT: scope + inner joins + default ordering.
pub fn state_list_sql() -> String {
    format!(
        "SELECT states.* FROM states JOIN projects ON projects.id = states.project_id JOIN project_members pm ON pm.project_id = projects.id WHERE {} ORDER BY {}",
        state_scope_where(),
        STATE_LIST_ORDER_SQL,
    )
}

/// R2 in-memory regroup (`:30-38`, ported bug 3): per `group`, the
/// 1-based `index` over the group's row count — `state.order =
/// index / count` — mutated on the instances, never saved. `count` is
/// always >= 1 (a group exists only via its rows).
pub fn state_group_order(index_1based: usize, count: usize) -> f64 {
    index_1based as f64 / count as f64
}

// ---------------------------------------------------------------------------
// R3 estimates list (estimate.py:22-33)
// ---------------------------------------------------------------------------

/// R3 Q1 (`:23-25`): `values_list("estimate_id", flat=True)` over projects
/// in the workspace with a non-null estimate. Ported bug 14: NO member
/// scoping — any project in the workspace qualifies. The `estimate__isnull`
/// check renders on the local `estimate_id` column (no join).
pub fn estimate_ids_q1_sql() -> String {
    format!(
        "SELECT projects.estimate_id FROM projects WHERE projects.workspace_id = {} AND projects.estimate_id IS NOT NULL AND projects.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R3 Q2 scope (`:27`): `pk__in=<Q1> AND workspace__slug=:slug` over the
/// soft-delete manager. Handlers bind `:estimate_ids` from Q1 (or splice
/// the subquery).
pub fn estimate_scope_where() -> String {
    format!(
        "estimates.id IN (:estimate_ids) AND estimates.workspace_id = {} AND estimates.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R3 Q2 order: `Estimate.Meta.ordering = ("name",)`
/// (`db/models/estimate.py:39`) — ascending.
pub const ESTIMATE_LIST_ORDER_SQL: &str = "estimates.name ASC";

/// Full representative R3 Q2 SELECT: scope + `select_related("workspace",
/// "project")` (`:29`) as inner joins (both FKs non-nullable) + default
/// ordering.
pub fn estimate_list_sql() -> String {
    format!(
        "SELECT estimates.*, workspaces.*, projects.* FROM estimates JOIN workspaces ON workspaces.id = estimates.workspace_id JOIN projects ON projects.id = estimates.project_id WHERE {} ORDER BY {}",
        estimate_scope_where(),
        ESTIMATE_LIST_ORDER_SQL,
    )
}

/// R3 points prefetch (`:28`): `prefetch_related("points")` renders one
/// query over `EstimatePoint.estimate` (`related_name="points"`,
/// `db/models/estimate.py:44`) with the through default ordering
/// `("value",)` (`:56`).
pub fn estimate_points_prefetch_sql() -> String {
    "SELECT estimate_points.* FROM estimate_points WHERE estimate_points.estimate_id IN (:estimate_ids) AND estimate_points.deleted_at IS NULL ORDER BY estimate_points.value ASC".to_owned()
}

// ---------------------------------------------------------------------------
// R4 modules list (module.py:22-111)
// ---------------------------------------------------------------------------

/// R4/R5 list order (ported bug 1): the view spells
/// `.order_by(self.kwargs.get("order_by", "-created_at"))` (`module.py:107`,
/// `cycle.py:100`) but URL kwargs never hold `order_by`, so `?order_by=`
/// is IGNORED and the order is ALWAYS `-created_at`.
pub const MODULE_LIST_ORDER_SQL: &str = "modules.created_at DESC";

/// R4 scope (`:24-29`): `workspace__slug=:slug` + `archived_at` null over
/// the soft-delete manager. NO member/project scoping — the only filters
/// are workspace + unarchived.
pub fn module_scope_where() -> String {
    format!(
        "modules.workspace_id = {} AND modules.archived_at IS NULL AND modules.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// Annotation alias stems (`:37-106`): the total plus the five state
/// groups, in source order.
pub const MODULE_COUNT_GROUPS: &[&str] =
    &["completed", "cancelled", "started", "unstarted", "backlog"];

/// R4 count annotation (`:36-106`): `Count("issue_module", filter=...,
/// distinct=True)`. Every count — total and grouped — counts the link
/// (`module_issues.id`) with `DISTINCT`, guarded by issue-unarchived +
/// not-a-draft + link-alive. Pass `None` for the total, `Some(group)` for
/// a group count. NOTE (ported bug 2, module half): there is NO
/// `issues.deleted_at` filter here — the cycle counts have one.
pub fn module_count_annotation_sql(group: Option<&str>) -> String {
    let (alias, group_predicate) = match group {
        None => ("total_issues".to_owned(), String::new()),
        Some(g) => (format!("{g}_issues"), format!(" AND s.group = '{g}'")),
    };
    format!(
        "COUNT(DISTINCT mi.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND mi.deleted_at IS NULL{group_predicate}) AS {alias}"
    )
}

/// Full representative R4 SELECT: scope + `select_related("project",
/// "workspace", "lead")` (`:25-27`) as joins (`lead` is nullable,
/// `db/models/module.py:86`, so that join is `LEFT`) + the six
/// annotations (joining `module_issues mi`, `issues i`, `states s`) +
/// always-`-created_at` order. NOTE: NO `.distinct()` — contrast the
/// cycle list.
pub fn module_list_sql() -> String {
    let mut annotations = vec![module_count_annotation_sql(None)];
    annotations.extend(
        MODULE_COUNT_GROUPS
            .iter()
            .map(|g| module_count_annotation_sql(Some(g))),
    );
    format!(
        "SELECT modules.*, {} FROM modules JOIN projects ON projects.id = modules.project_id JOIN workspaces ON workspaces.id = modules.workspace_id LEFT JOIN users lead ON lead.id = modules.lead_id LEFT JOIN module_issues mi ON mi.module_id = modules.id LEFT JOIN issues i ON i.id = mi.issue_id LEFT JOIN states s ON s.id = i.state_id WHERE {} GROUP BY modules.id ORDER BY {}",
        annotations.join(", "),
        module_scope_where(),
        MODULE_LIST_ORDER_SQL,
    )
}

/// R4 members prefetch (`:28`): `prefetch_related("members")` over the
/// `ModuleMember` through table (`db/models/module.py:86-95`) with the
/// through manager's soft-delete scope; member rows render in `User`
/// default order (`-created_at`, `db/models/user.py:137`).
pub fn module_members_prefetch_sql() -> String {
    "SELECT users.*, module_members.* FROM module_members JOIN users ON users.id = module_members.member_id WHERE module_members.module_id IN (:module_ids) AND module_members.deleted_at IS NULL ORDER BY users.created_at DESC".to_owned()
}

/// R4 link prefetch (`:30-35`): `Prefetch("link_module",
/// queryset=ModuleLink.objects.select_related("module", "created_by"))` —
/// the module join is inner (`ModuleLink.module` non-nullable,
/// `db/models/module.py:177`); the `created_by` join is `LEFT` (audit FKs
/// are nullable, `db/mixins.py:29-35`).
pub fn module_links_prefetch_sql() -> String {
    "SELECT module_links.*, modules.*, creators.* FROM module_links JOIN modules ON modules.id = module_links.module_id LEFT JOIN users creators ON creators.id = module_links.created_by_id WHERE module_links.module_id IN (:module_ids) AND module_links.deleted_at IS NULL".to_owned()
}

// ---------------------------------------------------------------------------
// R5 cycles list (cycle.py:22-104)
// ---------------------------------------------------------------------------

/// R5 list order: the same order_by-kwargs bug as R4 (ported bug 1) —
/// always `-created_at` (`cycle.py:100`).
pub const CYCLE_LIST_ORDER_SQL: &str = "cycles.created_at DESC";

/// R5 scope (`:24-28`): same no-scoping shape as R4 — workspace + unarchived
/// + soft-delete manager.
pub fn cycle_scope_where() -> String {
    format!(
        "cycles.workspace_id = {} AND cycles.archived_at IS NULL AND cycles.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R5 count annotation (`:29-99`, ported bug 2, cycle half): `Count(...)`
/// WITHOUT `distinct`, counting the LINK (`issue_cycle`) for the total
/// (`:31`) but the STATE GROUP (`issue_cycle__issue__state__group`,
/// `:42/:54/:66/:78/:90`) for the five grouped counts, each guarded by
/// issue-unarchived + not-a-draft + issue-alive + link-alive. The filter
/// predicates keep source order (group first, `:43-48`). Pass `None` for
/// the total, `Some(group)` for a group count.
pub fn cycle_count_annotation_sql(group: Option<&str>) -> String {
    match group {
        None => "COUNT(ic.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ic.deleted_at IS NULL AND i.deleted_at IS NULL) AS total_issues".to_owned(),
        Some(g) => format!(
            "COUNT(s.group) FILTER (WHERE s.group = '{g}' AND i.archived_at IS NULL AND i.is_draft = FALSE AND i.deleted_at IS NULL AND ic.deleted_at IS NULL) AS {g}_issues"
        ),
    }
}

/// Full representative R5 SELECT: scope + `select_related("project",
/// "workspace", "owned_by")` (`:25-27`) as joins (all three FKs
/// non-nullable — `owned_by` has no `null=True`,
/// `db/models/cycle.py:65-69` — so every join is inner) + the six
/// annotations + always-`-created_at` order + `.distinct()` (`:101`) —
/// contrast the module list, which has no distinct.
pub fn cycle_list_sql() -> String {
    let mut annotations = vec![cycle_count_annotation_sql(None)];
    annotations.extend(
        MODULE_COUNT_GROUPS
            .iter()
            .map(|g| cycle_count_annotation_sql(Some(g))),
    );
    format!(
        "SELECT DISTINCT cycles.*, {} FROM cycles JOIN projects ON projects.id = cycles.project_id JOIN workspaces ON workspaces.id = cycles.workspace_id JOIN users owner ON owner.id = cycles.owned_by_id LEFT JOIN cycle_issues ic ON ic.cycle_id = cycles.id LEFT JOIN issues i ON i.id = ic.issue_id LEFT JOIN states s ON s.id = i.state_id WHERE {} GROUP BY cycles.id ORDER BY {}",
        annotations.join(", "),
        cycle_scope_where(),
        CYCLE_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R6 favorites (favorite.py:20-97)
// ---------------------------------------------------------------------------

/// Shared project-membership probe behind both favorite GET branches: the
/// project is set and the requester holds an ACTIVE membership in it.
/// Rendered as `EXISTS` (Django renders the `Q` disjunction as `LEFT
/// JOIN`s; the shapes return the same rows because `(project, member)` is
/// unique among alive rows — `db/models/project.py:367-373` — so the join
/// cannot fan out).
pub fn favorite_member_probe_sql() -> String {
    "user_favorites.project_id IS NOT NULL AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id = user_favorites.project_id AND pm.member_id = :user AND pm.is_active = TRUE)".to_owned()
}

/// R6 top-level scope (`:26`): user + workspace slug + parent-null over the
/// soft-delete manager.
pub fn favorite_scope_where() -> String {
    format!(
        "user_favorites.user_id = :user AND user_favorites.workspace_id = {} AND user_favorites.parent_id IS NULL AND user_favorites.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R6 top-level branch (`:27-32`, ported bug 4): `(project-null AND NOT
/// page) OR (project-member)`. `&` binds tighter than `|` in the source,
/// so project-linked page favorites PASS via the right branch — only
/// project-less page favorites are excluded. `entity_type` is non-nullable
/// (`db/models/favorite.py:20`), so `<> 'page'` matches the source `~Q`.
pub fn favorite_branch_where() -> String {
    format!(
        "((user_favorites.project_id IS NULL AND user_favorites.entity_type <> 'page') OR ({}))",
        favorite_member_probe_sql()
    )
}

/// R6 list order: `UserFavorite.Meta.ordering = ("-created_at",)`
/// (`db/models/favorite.py:47`).
pub const FAVORITE_LIST_ORDER_SQL: &str = "user_favorites.created_at DESC";

/// Full representative R6 GET SELECT: scope + branch + default ordering.
pub fn favorite_list_sql() -> String {
    format!(
        "SELECT user_favorites.* FROM user_favorites WHERE {} AND {} ORDER BY {}",
        favorite_scope_where(),
        favorite_branch_where(),
        FAVORITE_LIST_ORDER_SQL,
    )
}

/// R6 POST dedupe lookup (`:44-49`, ported bug 5): workspace + user +
/// entity type/identifier, `.first()` over default `-created_at` order.
/// Runs only when `entity_identifier` is present (`:43`). NOTE: NO project
/// filter and NO parent filter — a folder child with the same entity keys
/// dedupes against a top-level row. A hit answers 200 with the existing
/// row (`:52-54`); else the serializer creates
/// (`:57-64`, serializer-owned INSERT) and an `IntegrityError` on the
/// `(entity_type, entity_identifier, user)` partial unique
/// (`db/models/favorite.py:34-40`) answers 400 `{"error": "Favorite
/// already exists"}` (`:66-67`, handler body).
pub fn favorite_dedupe_lookup_sql() -> String {
    "SELECT user_favorites.* FROM user_favorites WHERE user_favorites.workspace_id = :ws AND user_favorites.user_id = :user AND user_favorites.entity_type = :entity_type AND user_favorites.entity_identifier = :entity_identifier AND user_favorites.deleted_at IS NULL ORDER BY user_favorites.created_at DESC LIMIT 1".to_owned()
}

/// R6 PATCH/DELETE lookup (`:71`, `:80`): `.get(user, slug, pk)` — a bare
/// get, so a miss raises through the base-view 404 handler
/// (`app/views/base.py:110-150`).
pub fn favorite_lookup_where() -> String {
    format!(
        "user_favorites.user_id = :user AND user_favorites.workspace_id = {} AND user_favorites.id = :pk AND user_favorites.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R6 DELETE (`:81`): `delete(soft=False)` — a HARD delete. Both paths
/// answer 204.
pub fn favorite_hard_delete_sql() -> String {
    "DELETE FROM user_favorites WHERE id = :pk".to_owned()
}

/// R6 group scope (`:88`): user + workspace slug + `parent_id=:fid` over
/// the soft-delete manager.
pub fn favorite_group_scope_where() -> String {
    format!(
        "user_favorites.user_id = :user AND user_favorites.workspace_id = {} AND user_favorites.parent_id = :fid AND user_favorites.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R6 group branch (`:89-94`, ported bug 4, second half): `project-null OR
/// project-member` — NOTE there is NO page exclusion here, unlike the
/// top-level branch.
pub fn favorite_group_branch_where() -> String {
    format!(
        "(user_favorites.project_id IS NULL OR ({}))",
        favorite_member_probe_sql()
    )
}

/// Full representative R6 group GET SELECT: scope + branch + default ordering.
pub fn favorite_group_list_sql() -> String {
    format!(
        "SELECT user_favorites.* FROM user_favorites WHERE {} AND {} ORDER BY {}",
        favorite_group_scope_where(),
        favorite_group_branch_where(),
        FAVORITE_LIST_ORDER_SQL,
    )
}

/// R6 POST workspace lookup (`:40`): `Workspace.objects.get(slug=:slug)` —
/// a direct manager get, so the `deleted_at` scope applies.
pub fn favorite_workspace_lookup_where() -> String {
    workspace_lookup_where()
}

// ---------------------------------------------------------------------------
// R7 drafts (draft.py:46-312)
// ---------------------------------------------------------------------------

/// R7 base scope (`draft.py:51`): `workspace__slug=:slug` over the
/// soft-delete manager. Every draft read below starts here.
pub fn draft_scope_where() -> String {
    format!(
        "draft_issues.workspace_id = {} AND draft_issues.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// `select_related` set (`:52`): `workspace`, `project`, `state`, `parent`.
/// `workspace` is non-nullable (inner join); `project` is nullable on
/// `WorkspaceBaseModel`, and `state`/`parent` are nullable on `DraftIssue`
/// (`db/models/draft.py:17-35`) — those three joins are `LEFT`.
pub const DRAFT_SELECT_RELATED: &[&str] = &["workspace", "project", "state", "parent"];

/// `prefetch_related` set (`:53`): `assignees`, `labels`,
/// `draft_issue_module__module` — three follow-up queries (handlers run
/// them; the shapes are the M2M/through selects with manager scopes).
pub const DRAFT_PREFETCH_RELATED: &[&str] = &["assignees", "labels", "draft_issue_module__module"];

/// R7 cycle subquery (`:55-60`, ported bug 6): `Subquery(
/// DraftIssueCycle...values("cycle_id")[:1])` — alive links for this draft,
/// `LIMIT 1` with NO `ORDER BY`: the picked cycle is nondeterministic when
/// several links are alive. Correlated on the outer `draft_issues.id`.
pub fn draft_cycle_subquery_sql() -> String {
    "(SELECT dic.cycle_id FROM draft_issue_cycles dic WHERE dic.draft_issue_id = draft_issues.id AND dic.deleted_at IS NULL LIMIT 1) AS cycle_id".to_owned()
}

/// R7 label-ids annotation (`:62-69`): `Coalesce(ArrayAgg("labels__id",
/// distinct=True, filter=label-set + link-alive), [])` — `DISTINCT` label
/// ids over alive `draft_issue_labels` links, `COALESCE`d to `[]`.
pub fn draft_label_ids_annotation_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT labels.id) FILTER (WHERE labels.id IS NOT NULL AND draft_issue_labels.deleted_at IS NULL), '{}') AS label_ids".to_owned()
}

/// R7 assignee-ids annotation (`:70-81`, ported bug 7):
/// `Coalesce(ArrayAgg("assignees__id", distinct=True,
/// filter=assignee-set + member-active + link-alive), [])`. NOTE the
/// `assignees__member_project__is_active=True` leg joins `project_members`
/// on the assignee with NO project scoping — ANY active membership
/// qualifies the assignee.
pub fn draft_assignee_ids_annotation_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT users.id) FILTER (WHERE users.id IS NOT NULL AND pm.is_active = TRUE AND draft_issue_assignees.deleted_at IS NULL), '{}') AS assignee_ids".to_owned()
}

/// R7 module-ids annotation (`:82-93`): `Coalesce(ArrayAgg(
/// "draft_issue_module__module_id", distinct=True, filter=module-set +
/// module-unarchived + link-alive), [])`.
pub fn draft_module_ids_annotation_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT draft_issue_modules.module_id) FILTER (WHERE draft_issue_modules.module_id IS NOT NULL AND modules.archived_at IS NULL AND draft_issue_modules.deleted_at IS NULL), '{}') AS module_ids".to_owned()
}

/// R7 list order (`:101`): explicit `.order_by("-created_at")`.
pub const DRAFT_LIST_ORDER_SQL: &str = "draft_issues.created_at DESC";

/// R7 list page size (`:105-109`): `paginate(...)` with the default
/// `default_per_page=1000` (`utils/paginator.py:660`).
pub const DRAFT_LIST_PER_PAGE: i32 = 1000;

/// Full representative R7 list SELECT (`:99-109`): base scope +
/// `created_by=:user` + annotations + `.distinct()` (`:95`) + explicit
/// order. The `issue_filters(request.query_params, "GET")` predicates
/// (`:100`, `:103`) are dynamic — handlers compile them (D-26 owns the
/// compiler); the gzip wrapper (`:97`) is handler sequencing.
pub fn draft_list_sql() -> String {
    format!(
        "SELECT DISTINCT draft_issues.*, {}, {}, {}, {} FROM draft_issues WHERE {} AND draft_issues.created_by_id = :user GROUP BY draft_issues.id ORDER BY {}",
        draft_cycle_subquery_sql(),
        draft_label_ids_annotation_sql(),
        draft_assignee_ids_annotation_sql(),
        draft_module_ids_annotation_sql(),
        draft_scope_where(),
        DRAFT_LIST_ORDER_SQL,
    )
}

/// R7 create re-read keys (`:127-149`): the 21-key `.values()` in source
/// order — 17 columns plus the 4 annotations (`cycle_id`, `module_ids`,
/// `label_ids`, `assignee_ids`). NOTE the source spellings `estimate_point`
/// (not `estimate_point_id`) and `created_by`/`updated_by` (not `*_id`) —
/// Django resolves them to the PK values anyway; the key names are kept
/// verbatim. (Code truth: 21 keys — the fixture says 19.)
pub const DRAFT_CREATE_READ_KEYS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "project_id",
    "parent_id",
    "cycle_id",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "type_id",
    "description_html",
];

/// R7 create re-read (`:124-151`): the get-queryset shape filtered to the
/// just-saved pk, `.values(21 keys).first()` — `LIMIT 1` over default
/// `-created_at` order — answering 201. The create itself
/// (`DraftIssueCreateSerializer...save()`, `:115-123`) is serializer-owned
/// (D-26); only the re-read is recorded here.
pub fn draft_create_reread_sql() -> String {
    format!(
        "SELECT DISTINCT {} FROM draft_issues WHERE {} AND draft_issues.id = :pk GROUP BY draft_issues.id ORDER BY {} LIMIT 1",
        DRAFT_CREATE_READ_KEYS.join(", "),
        draft_scope_where(),
        DRAFT_LIST_ORDER_SQL,
    )
}

/// R7 patch/retrieve own-or-404 lookup (`:163`, `:188`): base scope + pk +
/// `created_by=:user`, `.first()`. A miss answers 404 — `{"error": "Issue
/// not found"}` for patch (`:166`), `{"error": "The required object does
/// not exist."}` for retrieve (`:192`) (handler bodies).
pub fn draft_own_lookup_where() -> String {
    format!(
        "{} AND draft_issues.id = :pk AND draft_issues.created_by_id = :user",
        draft_scope_where()
    )
}

/// R7 patch serializer context (`:174-177`): `project_id` defaults to the
/// issue's own project (`:168`), `cycle_id` defaults to the `"not_provided"`
/// sentinel (`:176`) — the serializer distinguishes "absent" from explicit
/// null. Patch answers 204 on save (`:183`).
pub const DRAFT_PATCH_CYCLE_ID_DEFAULT: &str = "not_provided";

/// R7 destroy lookup (`:201`): `DraftIssue.objects.get(workspace__slug,
/// pk)` — a direct get with NO `created_by` filter (ownership is enforced
/// by the creator guard, guards territory).
pub fn draft_destroy_lookup_where() -> String {
    format!(
        "draft_issues.workspace_id = {} AND draft_issues.id = :pk AND draft_issues.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R7 destroy write (`:202`) and draft-to-issue final delete (`:308`):
/// bare `.delete()` — SOFT (`SoftDeleteModel.delete` defaults to
/// `soft=True`; `db/models/draft.py` has no override): stamps `deleted_at`
/// via a full `save()` (so `updated_at` too) and enqueues
/// `soft_delete_related_objects` (jobs plane). Both paths answer 204/201
/// respectively. (Code truth: SOFT — the `.sql` fixture agrees; the
/// `rows.json` "hard delete()" wording does not.)
pub fn draft_soft_delete_sql() -> String {
    "UPDATE draft_issues SET deleted_at = :now, updated_at = :now WHERE id = :pk".to_owned()
}

/// R7 draft-to-issue fetch (`:207`): the get-queryset shape filtered to
/// `pk=:draft`. No project on the draft answers 400 `{"error": "Project is
/// required to create an issue."}` (`:209-213`, handler body).
pub fn draft_to_issue_fetch_where() -> String {
    format!("{} AND draft_issues.id = :draft", draft_scope_where())
}

/// R7 draft-to-issue cycle link (`:240-248`): `CycleIssue.objects.create(...)`
/// iff `request.data.cycle_id` is present — a single-row `INSERT` with the
/// DRAFT creator's audit ids (`created_by_id`/`updated_by_id` from the
/// draft, NOT the requester). `CycleIssue` is a `ProjectBaseModel`, so
/// `project_id`/`workspace_id` are columns.
pub fn cycle_issue_insert_sql() -> String {
    "INSERT INTO cycle_issues (id, created_at, updated_at, created_by_id, updated_by_id, cycle_id, issue_id, project_id, workspace_id) VALUES (:id, :now, :now, :creator, :updater, :cycle, :issue, :project, :ws)".to_owned()
}

/// R7 draft-to-issue module bulk batch (`:281`): `bulk_create(...,
/// batch_size=10)` — with NO `ignore_conflicts` (contrast the pref
/// autocreates).
pub const MODULE_ISSUE_BULK_BATCH_SIZE: i32 = 10;

/// R7 draft-to-issue module links (`:267-282`): one `ModuleIssue` row per
/// `request.data.module_ids` entry (draft workspace/project/creator ids),
/// multi-row `INSERT`, no conflict handling.
pub fn module_issue_bulk_insert_sql() -> String {
    "INSERT INTO module_issues (id, created_at, updated_at, created_by_id, updated_by_id, module_id, issue_id, project_id, workspace_id) VALUES (:id, :now, :now, :creator, :updater, :module, :issue, :project, :ws)".to_owned()
}

/// `FileAsset.EntityTypeContext.ISSUE_DESCRIPTION`
/// (`db/models/asset.py:35`) — the re-point target below.
pub const FILE_ASSET_ISSUE_DESCRIPTION: &str = "ISSUE_DESCRIPTION";

/// R7 draft-to-issue file re-point (`:299-305`):
/// `FileAsset.objects.filter(draft_issue_id=:draft).update(issue_id,
/// entity_type=ISSUE_DESCRIPTION, draft_issue_id=None)` — `QuerySet.update`
/// writes ONLY the named columns (no `updated_at`), over the manager's
/// `deleted_at` scope.
pub fn file_asset_repoint_sql() -> String {
    "UPDATE file_assets SET issue_id = :issue, entity_type = 'ISSUE_DESCRIPTION', draft_issue_id = NULL WHERE file_assets.draft_issue_id = :draft AND file_assets.deleted_at IS NULL".to_owned()
}

// ---------------------------------------------------------------------------
// R8 quick-links (quick_link.py:16-65)
// ---------------------------------------------------------------------------

/// R8 row scope (`:35`, `:48`, `:56`): pk + workspace slug + `owner=:user`
/// over the soft-delete manager — every read/write targets one owner row.
pub fn quick_link_owner_where() -> String {
    format!(
        "workspace_user_links.id = :pk AND workspace_user_links.workspace_id = {} AND workspace_user_links.owner_id = :user AND workspace_user_links.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R8 PATCH lookup (`:35`): `filter(...).first()` — `LIMIT 1` over default
/// `-created_at` order (`WorkspaceUserLink.Meta.ordering`,
/// `db/models/workspace.py:430`). A miss answers 404 `{"detail": "Quick
/// link not found."}` (`:43`, handler body — note the `"detail"` key).
pub fn quick_link_patch_lookup_sql() -> String {
    format!(
        "SELECT workspace_user_links.* FROM workspace_user_links WHERE {} ORDER BY workspace_user_links.created_at DESC LIMIT 1",
        quick_link_owner_where()
    )
}

/// R8 retrieve lookup (`:48`, ported bug 8): the same predicate set as the
/// PATCH lookup, but via `.get()` inside try/except — and the 404 body uses
/// the `"error"` key (`:52`), INCONSISTENT with the PATCH `"detail"` key.
/// Both shapes are kept.
pub fn quick_link_retrieve_where() -> String {
    quick_link_owner_where()
}

/// R8 destroy (`:56-57`): `.get(...)` + bare `.delete()` — SOFT (no
/// override on `WorkspaceUserLink`): stamps `deleted_at` via a full `save()`
/// (so `updated_at` too). Answers 204.
pub fn quick_link_soft_delete_sql() -> String {
    "UPDATE workspace_user_links SET deleted_at = :now, updated_at = :now WHERE id = :pk".to_owned()
}

/// R8 list scope (`:62`): `filter(workspace__slug, owner)` over the
/// soft-delete manager.
pub fn quick_link_list_where() -> String {
    format!(
        "workspace_user_links.workspace_id = {} AND workspace_user_links.owner_id = :user AND workspace_user_links.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R8 list order: `WorkspaceUserLink.Meta.ordering = ("-created_at",)` —
/// the view has no explicit `order_by`.
pub const QUICK_LINK_LIST_ORDER_SQL: &str = "workspace_user_links.created_at DESC";

/// Full representative R8 list SELECT: scope + default ordering. Create
/// (`:24-31`) looks the workspace up via [`workspace_lookup_where`] and
/// saves through `WorkspaceUserLinkSerializer` (SER-C, PIDASHCONV-602) —
/// serializer-owned, no builder here.
pub fn quick_link_list_sql() -> String {
    format!(
        "SELECT workspace_user_links.* FROM workspace_user_links WHERE {} ORDER BY {}",
        quick_link_list_where(),
        QUICK_LINK_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R9 stickies (sticky.py:16-60)
// ---------------------------------------------------------------------------

/// R9 base scope (`sticky.py:25-26`): `workspace__slug=:slug` +
/// `owner_id=:user` over the soft-delete manager (via
/// `BaseViewSet.get_queryset`, `model.objects.all()`).
pub fn sticky_scope_where() -> String {
    format!(
        "stickies.workspace_id = {} AND stickies.owner_id = :user AND stickies.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R9 list order (`:43`): explicit `.order_by("-sort_order")`.
pub const STICKY_LIST_ORDER_SQL: &str = "stickies.sort_order DESC";

/// R9 list page size (`:47-52`): `paginate(..., default_per_page=20)`.
pub const STICKY_LIST_PER_PAGE: i32 = 20;

/// Full representative R9 list SELECT (`:21-29` + `:41-52`): scope +
/// `select_related("workspace", "owner")` as inner joins (both FKs
/// non-nullable, `db/models/sticky.py:33-34`) + `.distinct()` (`:28`) +
/// explicit order. `filter_queryset` (`:22`) is a NO-OP here — the viewset
/// defines no `filterset_fields`/`search_fields`, so neither backend
/// filter applies. Handlers AND [`sticky_query_where`] when `?query=` is
/// truthy (empty adds no filter, `:42`, `:44`).
///
/// NOTE (ported bug 9): `retrieve` is NOT overridden, so it falls through
/// to the `ModelViewSet` default — authenticated-only, with NO
/// `allow_permission` workspace-membership/role gate (row scoping via this
/// scope still applies through `get_object`). `partial_update`/`destroy`
/// (`:54-60`) run the stock actions over this scope behind creator-only
/// gates (guards territory).
pub fn sticky_list_sql() -> String {
    format!(
        "SELECT DISTINCT stickies.*, workspaces.*, owners.* FROM stickies JOIN workspaces ON workspaces.id = stickies.workspace_id JOIN users owners ON owners.id = stickies.owner_id WHERE {} ORDER BY {}",
        sticky_scope_where(),
        STICKY_LIST_ORDER_SQL,
    )
}

/// R9 `?query=` filter (`:44-45`): `description_stripped__icontains=query`
/// renders `ILIKE` (Postgres case-insensitive `LIKE`, per the Semantic
/// traps section). Handlers bind `:query` as `%<escaped>%` where the term
/// is escaped with [`escape_icontains`]. The filter applies only when
/// `query` is truthy (`:42`, `:44` — `request.query_params.get("query",
/// False)`).
pub fn sticky_query_where() -> String {
    "stickies.description_stripped ILIKE :query".to_owned()
}

/// `icontains` LIKE-escaping: Django escapes the pattern characters `%`
/// and `_` (plus the backslash itself) with a backslash before wrapping
/// the term in `%...%`.
pub fn escape_icontains(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// R10 home prefs (home.py:17-79)
// ---------------------------------------------------------------------------

/// R10 widget keys (`home.py:31-35`): `HomeWidgetKeys.choices` in
/// definition order (`db/models/workspace.py:439-444`), minus the two
/// excluded keys below — i.e. `[quick_links, recents, my_stickies]`.
pub const HOME_PREF_KEYS: &[&str] = &["quick_links", "recents", "my_stickies"];

/// R10 excluded keys (`:34`): never autocreated, never returned.
pub const HOME_PREF_EXCLUDED_KEYS: &[&str] = &["quick_tutorial", "new_at_pi_dash"];

/// R10 bulk batch (`:55`): `bulk_create(..., batch_size=10,
/// ignore_conflicts=True)`.
pub const HOME_PREF_BULK_BATCH_SIZE: i32 = 10;

/// One effective autocreate row: the key plus the `sort_order` that won the
/// first-insert-wins race (an `i32` — the loop only ever computes integral
/// values, stored into the `FloatField` column).
pub type HomePrefRow<'a> = (&'a str, i32);

/// R10 autocreate loop (`:37-58`, ported bug 10): simulates the loop
/// exactly, given the keys already present for (user, workspace).
/// Per missing key (in [`HOME_PREF_KEYS`] order): append to the GROWING
/// list, compute `sort_order = 1000 - counter`, `bulk_create` the WHOLE
/// growing list at that `sort_order` with `ignore_conflicts` (earlier keys
/// conflict-skip, keeping their first insert), bump the counter. Returns
/// the newly inserted `(key, sort_order)` pairs in key order.
///
/// The per-key `values_list("key")` re-query (`:40`, N+1) has no behavioral
/// effect — each key is either pre-existing or inserted exactly once — so
/// the simulation reads `existing` once. Empty-table trace: `[(quick_links,
/// 999), (recents, 998), (my_stickies, 997)]`.
pub fn home_autocreate_plan<'a>(existing: &[&'a str]) -> Vec<HomePrefRow<'a>> {
    let mut growing: Vec<&'a str> = Vec::new();
    let mut inserted: Vec<HomePrefRow<'a>> = Vec::new();
    let mut counter = 1;
    for key in HOME_PREF_KEYS {
        if existing.contains(key) || growing.contains(key) {
            continue;
        }
        growing.push(key);
        let sort_order = 1000 - counter;
        for candidate in &growing {
            if !inserted.iter().any(|(k, _)| k == candidate) {
                inserted.push((candidate, sort_order));
            }
        }
        counter += 1;
    }
    inserted.sort_by_key(|(k, _)| HOME_PREF_KEYS.iter().position(|p| p == k));
    inserted
}

/// R10 bulk insert (`:45-57`): `INSERT ... ON CONFLICT DO NOTHING` over the
/// alive `(workspace, user, key)` partial unique
/// (`db/models/workspace.py:461-467`). `is_enabled`/`config` ride their
/// Django defaults (`true`/`{}`).
pub fn home_pref_insert_sql() -> String {
    "INSERT INTO workspace_home_preferences (id, created_at, updated_at, key, user_id, workspace_id, sort_order) VALUES (:id, :now, :now, :key, :user, :ws, :sort) ON CONFLICT DO NOTHING".to_owned()
}

/// R10 response keys (`:63`): `.values("key", "is_enabled", "config",
/// "sort_order")`.
pub const HOME_PREF_RESPONSE_KEYS: &[&str] = &["key", "is_enabled", "config", "sort_order"];

/// R10 response order: NO explicit `order_by` (`:60-64`) — the model's
/// `Meta.ordering = ("-created_at",)` applies, so the newest row comes
/// first regardless of `sort_order`.
pub const HOME_PREF_RESPONSE_ORDER_SQL: &str = "workspace_home_preferences.created_at DESC";

/// Full representative R10 GET response SELECT (`:60-64`): user + workspace
/// + soft-delete scope, response keys, default ordering.
pub fn home_pref_response_sql() -> String {
    format!(
        "SELECT {} FROM workspace_home_preferences WHERE workspace_home_preferences.user_id = :user AND workspace_home_preferences.workspace_id = :ws AND workspace_home_preferences.deleted_at IS NULL ORDER BY {}",
        HOME_PREF_RESPONSE_KEYS.join(", "),
        HOME_PREF_RESPONSE_ORDER_SQL,
    )
}

/// R10 PATCH lookup (`:69`): `filter(key, slug, user).first()` — `LIMIT 1`
/// over default `-created_at` order. NOTE the user filter (contrast the
/// sidebar PATCH). A miss answers 400 `{"Detail": "Preference not found"}`
/// (`:79`, ported bug 12 — 400, not 404, capital `D`); a hit saves through the
/// serializer (full save, `updated_at` stamped).
pub fn home_pref_patch_lookup_sql() -> String {
    format!(
        "SELECT workspace_home_preferences.* FROM workspace_home_preferences WHERE workspace_home_preferences.key = :key AND workspace_home_preferences.workspace_id = {} AND workspace_home_preferences.user_id = :user AND workspace_home_preferences.deleted_at IS NULL ORDER BY {} LIMIT 1",
        workspace_id_by_slug_sql(),
        HOME_PREF_RESPONSE_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R11 sidebar prefs (user_preference.py:18-101)
// ---------------------------------------------------------------------------

/// R11 pref keys (`user_preference.py:33`): ALL seven
/// `UserPreferenceKeys.choices` in definition order
/// (`db/models/workspace.py:482-489`).
pub const SIDEBAR_PREF_KEYS: &[&str] = &[
    "views",
    "active_cycles",
    "analytics",
    "drafts",
    "your_work",
    "archives",
    "stickies",
];

/// R11 pinned keys (`:46-55`): `is_pinned=True` iff the key is one of
/// `DRAFTS`/`YOUR_WORK`/`STICKIES`.
pub const SIDEBAR_PINNED_KEYS: &[&str] = &["drafts", "your_work", "stickies"];

/// R11 bulk batch (`:59`): `bulk_create(..., batch_size=10,
/// ignore_conflicts=True)`.
pub const SIDEBAR_PREF_BULK_BATCH_SIZE: i32 = 10;

/// One effective sidebar autocreate row: `(key, is_pinned, sort_order)`.
pub type SidebarPrefRow<'a> = (&'a str, bool, i32);

/// Whether an autocreated sidebar row is pinned (`:46-55`).
pub fn sidebar_pref_pinned(key: &str) -> bool {
    SIDEBAR_PINNED_KEYS.contains(&key)
}

/// R11 autocreate loop (`:35-61`, ported bug 10, sidebar shape): simulates
/// the loop exactly, given the keys already present. Per missing key:
/// append to the GROWING list, `bulk_create` the whole growing list with
/// `sort_order = 65535 + i * 10000` (`:45`, `i` = index in the GROWING
/// list) and per-key `is_pinned`, `ignore_conflicts` (earlier keys keep
/// their first insert). NOTE the index basis: with pre-existing rows the
/// growing-list indexes shift, so later keys land on LOWER sort orders
/// than the empty-table trace. Empty-table trace: `views@65535`,
/// `active_cycles@75535`, `analytics@85535`, `drafts@95535` (pinned),
/// `your_work@105535` (pinned), `archives@115535`, `stickies@125535`
/// (pinned).
pub fn sidebar_autocreate_plan<'a>(existing: &[&'a str]) -> Vec<SidebarPrefRow<'a>> {
    let mut growing: Vec<&'a str> = Vec::new();
    let mut inserted: Vec<SidebarPrefRow<'a>> = Vec::new();
    for key in SIDEBAR_PREF_KEYS {
        if existing.contains(key) || growing.contains(key) {
            continue;
        }
        growing.push(key);
        for (i, candidate) in growing.iter().enumerate() {
            if !inserted.iter().any(|(k, _, _)| k == candidate) {
                #[allow(clippy::cast_possible_truncation)]
                let sort_order = 65535 + (i as i32) * 10000;
                inserted.push((candidate, sidebar_pref_pinned(candidate), sort_order));
            }
        }
    }
    inserted.sort_by_key(|(k, _, _)| SIDEBAR_PREF_KEYS.iter().position(|p| p == k));
    inserted
}

/// R11 bulk insert (`:39-61`): `INSERT ... ON CONFLICT DO NOTHING` over the
/// alive `(workspace, user, key)` partial unique
/// (`db/models/workspace.py:505-511`).
pub fn sidebar_pref_insert_sql() -> String {
    "INSERT INTO workspace_user_preferences (id, created_at, updated_at, key, user_id, workspace_id, sort_order, is_pinned) VALUES (:id, :now, :now, :key, :user, :ws, :sort, :pinned) ON CONFLICT DO NOTHING".to_owned()
}

/// R11 response keys (`:66`): `.values("key", "is_pinned", "sort_order")`,
/// ordered by `sort_order` ASC (`:65`) and shaped by the handler into
/// `{key: {is_pinned, sort_order}}` (`:69-79`).
pub const SIDEBAR_PREF_RESPONSE_KEYS: &[&str] = &["key", "is_pinned", "sort_order"];

/// Full representative R11 GET response SELECT (`:63-67`).
pub fn sidebar_pref_response_sql() -> String {
    format!(
        "SELECT {} FROM workspace_user_preferences WHERE workspace_user_preferences.user_id = :user AND workspace_user_preferences.workspace_id = :ws AND workspace_user_preferences.deleted_at IS NULL ORDER BY workspace_user_preferences.sort_order ASC",
        SIDEBAR_PREF_RESPONSE_KEYS.join(", "),
    )
}

/// R11 PATCH lookup (`:88`, ported bug 11): `filter(key, slug).first()` —
/// `LIMIT 1` over default `-created_at` order, with NO user filter: a
/// member can match (and rewrite, below) ANOTHER user's row. Per request
/// row the handler pops `key` (`:84`), skips a missing key (`:85-86`) or a
/// no-match (`:90-91`), and sets `is_pinned`/`sort_order` when present
/// (`:93-97`).
pub fn sidebar_pref_patch_lookup_sql() -> String {
    format!(
        "SELECT workspace_user_preferences.* FROM workspace_user_preferences WHERE workspace_user_preferences.key = :key AND workspace_user_preferences.workspace_id = {} AND workspace_user_preferences.deleted_at IS NULL ORDER BY workspace_user_preferences.created_at DESC LIMIT 1",
        workspace_id_by_slug_sql()
    )
}

/// R11 PATCH write (`:99`): `save(update_fields=["is_pinned",
/// "sort_order"])` writes ONLY those two columns — `updated_at` is NOT
/// stamped (Django `_save_table` restricts the `UPDATE` to the named
/// fields) — even when only one (or neither) was in the request. The path
/// always answers 200 `{"message": "Successfully updated"}` (`:101`).
pub fn sidebar_pref_patch_sql() -> String {
    "UPDATE workspace_user_preferences SET is_pinned = :pinned, sort_order = :sort WHERE id = :pk"
        .to_owned()
}

// ---------------------------------------------------------------------------
// R12 recent visits (recent_visit.py:24-36)
// ---------------------------------------------------------------------------

/// R12 entity allowlist (`:33`): the HARD `entity_name__in` clamp.
pub const VISIT_ENTITY_ALLOWLIST: &[&str] = &["issue", "page", "project"];

/// R12 slice (`:35`): `[:20]` over `Meta.ordering = ("-created_at",)`
/// (`db/models/recent_visit.py:35`).
pub const RECENT_VISITS_CAP: i32 = 20;

/// R12 scope: `filter(workspace__slug, user)` (`:26`) + the optional
/// `?entity_name=` narrowing (`:28-31`) + the HARD allowlist clamp (`:33`,
/// ported bug 13). The narrowing applies only when the param is TRUTHY
/// (`if entity_name:`, `:30` — an empty `?entity_name=` adds no filter);
/// the clamp applies regardless — a non-listed `?entity_name=` yields `[]`.
pub fn recent_visits_where(entity: Option<&str>) -> String {
    let mut where_clause = format!(
        "user_recent_visits.workspace_id = {} AND user_recent_visits.user_id = :user AND user_recent_visits.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    );
    if entity.is_some_and(|e| !e.is_empty()) {
        where_clause.push_str(" AND user_recent_visits.entity_name = :entity");
    }
    where_clause.push_str(" AND user_recent_visits.entity_name IN ('issue', 'page', 'project')");
    where_clause
}

/// Full representative R12 list SELECT (`:26-36`): scope + default ordering
/// + `[:20]` cap.
pub fn recent_visits_sql(entity: Option<&str>) -> String {
    format!(
        "SELECT user_recent_visits.* FROM user_recent_visits WHERE {} ORDER BY user_recent_visits.created_at DESC LIMIT 20",
        recent_visits_where(entity)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/extras_user.sql"
    );
    const FIXTURE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/extras_user.rows.json"
    );

    fn rows_fixture() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_ROWS).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn sql_fixture() -> String {
        std::fs::read_to_string(FIXTURE_SQL).expect("fixture exists")
    }

    fn case<'a>(fixture: &'a serde_json::Value, q: &str) -> &'a serde_json::Value {
        fixture["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|c| c["q"] == q)
            .unwrap_or_else(|| panic!("case {q}"))
    }

    #[test]
    fn every_soft_delete_read_carries_deleted_at_scope() {
        // Issue rule: soft-delete scoping on every read. Cross-FK joins
        // carry no related-manager scope (only the base table does).
        for sql in [
            label_scope_where(),
            label_list_sql(),
            state_scope_where(),
            state_list_sql(),
            estimate_ids_q1_sql(),
            estimate_scope_where(),
            estimate_list_sql(),
            estimate_points_prefetch_sql(),
            module_scope_where(),
            module_list_sql(),
            module_members_prefetch_sql(),
            module_links_prefetch_sql(),
            cycle_scope_where(),
            cycle_list_sql(),
            favorite_scope_where(),
            favorite_list_sql(),
            favorite_dedupe_lookup_sql(),
            favorite_lookup_where(),
            favorite_group_scope_where(),
            favorite_group_list_sql(),
            favorite_workspace_lookup_where(),
            draft_scope_where(),
            draft_cycle_subquery_sql(),
            draft_label_ids_annotation_sql(),
            draft_assignee_ids_annotation_sql(),
            draft_module_ids_annotation_sql(),
            draft_list_sql(),
            draft_create_reread_sql(),
            draft_own_lookup_where(),
            draft_destroy_lookup_where(),
            draft_to_issue_fetch_where(),
            file_asset_repoint_sql(),
            quick_link_owner_where(),
            quick_link_patch_lookup_sql(),
            quick_link_retrieve_where(),
            quick_link_list_where(),
            quick_link_list_sql(),
            sticky_scope_where(),
            sticky_list_sql(),
            home_pref_response_sql(),
            home_pref_patch_lookup_sql(),
            sidebar_pref_response_sql(),
            sidebar_pref_patch_lookup_sql(),
            recent_visits_where(None),
            recent_visits_where(Some("issue")),
            recent_visits_sql(None),
        ] {
            assert!(
                sql.contains("deleted_at IS NULL"),
                "missing soft-delete scope: {sql}"
            );
        }
        // Hard writes are not reads: the favorite hard DELETE and the
        // INSERTs stay out of the sweep. users-joined probes reference
        // users (no deleted_at column) alongside scoped tables. Fragments
        // composed onto scoped statements (favorite branches, sticky
        // ?query=) carry no scope of their own — composed forms are swept
        // above.
        assert!(favorite_hard_delete_sql().starts_with("DELETE FROM"));
        assert!(!file_asset_repoint_sql().contains("updated_at"));
        assert!(favorite_list_sql().contains(&favorite_branch_where()));
        assert!(favorite_group_list_sql().contains(&favorite_group_branch_where()));
    }

    #[test]
    fn r1_labels_member_and_unarchived() {
        let scope = label_scope_where();
        for needle in [
            "labels.workspace_id = ",
            "slug = :slug",
            "pm.member_id = :user",
            "pm.is_active = TRUE",
            "projects.archived_at IS NULL",
            "labels.deleted_at IS NULL",
        ] {
            assert!(scope.contains(needle), "missing {needle}");
        }
        // No related-manager scope on the joined tables.
        assert!(!scope.contains("projects.deleted_at"));
        assert!(!scope.contains("pm.deleted_at"));
        let sql = label_list_sql();
        assert!(sql.contains("JOIN projects ON projects.id = labels.project_id"));
        assert!(sql.contains("JOIN project_members pm ON pm.project_id = projects.id"));
        assert!(sql.ends_with(&format!("ORDER BY {LABEL_LIST_ORDER_SQL}")));
        assert_eq!(LABEL_LIST_ORDER_SQL, "labels.created_at DESC");
    }

    #[test]
    fn r2_states_triage_and_in_memory_order() {
        let scope = state_scope_where();
        for needle in [
            "pm.member_id = :user",
            "projects.archived_at IS NULL",
            "states.is_triage = FALSE",
            "NOT (states.group = 'triage')",
        ] {
            assert!(scope.contains(needle), "missing {needle}");
        }
        assert_eq!(STATE_LIST_ORDER_SQL, "states.sequence ASC");
        assert!(state_list_sql().ends_with("ORDER BY states.sequence ASC"));
        // Ported bug 3: index/count, 1-based, float division.
        assert_eq!(state_group_order(1, 4), 0.25);
        assert_eq!(state_group_order(2, 4), 0.5);
        assert_eq!(state_group_order(4, 4), 1.0);
        assert_eq!(state_group_order(1, 1), 1.0);
    }

    #[test]
    fn r3_estimates_two_queries_no_member_scope() {
        // Ported bug 14: Q1 has NO member filter.
        let q1 = estimate_ids_q1_sql();
        assert!(q1.contains("SELECT projects.estimate_id FROM projects"));
        assert!(q1.contains("projects.estimate_id IS NOT NULL"));
        assert!(!q1.contains("member"));
        let scope = estimate_scope_where();
        assert!(scope.contains("estimates.id IN (:estimate_ids)"));
        assert!(scope.contains("slug = :slug"));
        assert_eq!(ESTIMATE_LIST_ORDER_SQL, "estimates.name ASC");
        let sql = estimate_list_sql();
        assert!(sql.contains("JOIN workspaces ON workspaces.id = estimates.workspace_id"));
        assert!(sql.contains("JOIN projects ON projects.id = estimates.project_id"));
        assert!(sql.ends_with("ORDER BY estimates.name ASC"));
        let prefetch = estimate_points_prefetch_sql();
        assert!(prefetch.contains("estimate_points.estimate_id IN (:estimate_ids)"));
        assert!(prefetch.ends_with("ORDER BY estimate_points.value ASC"));
    }

    #[test]
    fn r4_modules_counts_order_bug_and_prefetches() {
        // Ported bug 1: always -created_at.
        assert_eq!(MODULE_LIST_ORDER_SQL, "modules.created_at DESC");
        let scope = module_scope_where();
        assert!(scope.contains("modules.archived_at IS NULL"));
        assert!(!scope.contains("member"));
        // Six annotations, all DISTINCT on the link, no issue.deleted filter.
        assert_eq!(
            MODULE_COUNT_GROUPS,
            &["completed", "cancelled", "started", "unstarted", "backlog"]
        );
        let total = module_count_annotation_sql(None);
        assert!(total.contains("COUNT(DISTINCT mi.id)"));
        assert!(total.ends_with("AS total_issues"));
        // NOTE: the negative match needs the leading space — "mi.deleted_at"
        // (the link guard, present) otherwise contains "i.deleted_at".
        assert!(!total.contains(" i.deleted_at"));
        for group in MODULE_COUNT_GROUPS {
            let annotation = module_count_annotation_sql(Some(group));
            assert!(annotation.contains("COUNT(DISTINCT mi.id)"), "{group}");
            assert!(
                annotation.contains(&format!("s.group = '{group}'")),
                "{group}"
            );
            assert!(
                annotation.ends_with(&format!("AS {group}_issues")),
                "{group}"
            );
            assert!(!annotation.contains(" i.deleted_at"), "{group}");
        }
        let sql = module_list_sql();
        assert!(sql.contains("LEFT JOIN users lead ON lead.id = modules.lead_id"));
        assert!(sql.ends_with("ORDER BY modules.created_at DESC"));
        assert!(!sql.starts_with("SELECT DISTINCT"));
        let members = module_members_prefetch_sql();
        assert!(members.contains("module_members.module_id IN (:module_ids)"));
        assert!(members.ends_with("ORDER BY users.created_at DESC"));
        let links = module_links_prefetch_sql();
        assert!(links.contains("module_links.module_id IN (:module_ids)"));
        assert!(
            links.contains("LEFT JOIN users creators ON creators.id = module_links.created_by_id")
        );
    }

    #[test]
    fn r5_cycles_counts_distinct_and_order_bug() {
        assert_eq!(CYCLE_LIST_ORDER_SQL, "cycles.created_at DESC");
        // Ported bug 2, cycle half: NO distinct; total counts the link,
        // group counts count the state group; issue.deleted filter present.
        let total = cycle_count_annotation_sql(None);
        assert!(total.contains("COUNT(ic.id)"));
        assert!(!total.contains("DISTINCT"));
        assert!(total.contains("i.deleted_at IS NULL"));
        assert!(total.contains("ic.deleted_at IS NULL"));
        for group in MODULE_COUNT_GROUPS {
            let annotation = cycle_count_annotation_sql(Some(group));
            assert!(annotation.contains("COUNT(s.group)"), "{group}");
            assert!(!annotation.contains("DISTINCT"), "{group}");
            // Source predicate order: group, archived, draft, issue-deleted, link-deleted.
            let group_pos = annotation
                .find(&format!("s.group = '{group}'"))
                .expect("group");
            let archived = annotation.find("i.archived_at IS NULL").expect("archived");
            let draft = annotation.find("i.is_draft = FALSE").expect("draft");
            let issue_deleted = annotation
                .find("i.deleted_at IS NULL")
                .expect("issue deleted");
            let link_deleted = annotation
                .find("ic.deleted_at IS NULL")
                .expect("link deleted");
            assert!(
                group_pos < archived
                    && archived < draft
                    && draft < issue_deleted
                    && issue_deleted < link_deleted
            );
        }
        let sql = cycle_list_sql();
        assert!(sql.starts_with("SELECT DISTINCT cycles.*"));
        assert!(sql.contains("JOIN users owner ON owner.id = cycles.owned_by_id"));
        assert!(!sql.contains("LEFT JOIN users owner"));
        assert!(sql.ends_with("ORDER BY cycles.created_at DESC"));
    }

    #[test]
    fn r6_favorites_branches_dedupe_and_hard_delete() {
        // Ported bug 4: (project-null AND NOT page) OR (project-member).
        let branch = favorite_branch_where();
        assert!(branch.contains(
            "user_favorites.project_id IS NULL AND user_favorites.entity_type <> 'page'"
        ));
        assert!(branch.contains(&favorite_member_probe_sql()));
        let probe = favorite_member_probe_sql();
        assert!(probe.contains("pm.member_id = :user"));
        assert!(probe.contains("pm.is_active = TRUE"));
        // Group branch: NO page exclusion.
        let group_branch = favorite_group_branch_where();
        assert!(group_branch.contains("user_favorites.project_id IS NULL OR"));
        assert!(!group_branch.contains("page"));
        assert!(group_branch.contains(&probe));
        // Ported bug 5: dedupe has no project/parent filter.
        let dedupe = favorite_dedupe_lookup_sql();
        for needle in [
            "user_favorites.workspace_id = :ws",
            "user_favorites.user_id = :user",
            "user_favorites.entity_type = :entity_type",
            "user_favorites.entity_identifier = :entity_identifier",
        ] {
            assert!(dedupe.contains(needle), "missing {needle}");
        }
        assert!(!dedupe.contains("project_id"));
        assert!(!dedupe.contains("parent_id"));
        assert!(dedupe.ends_with("ORDER BY user_favorites.created_at DESC LIMIT 1"));
        // Patch/delete lookup + HARD delete.
        let lookup = favorite_lookup_where();
        assert!(lookup.contains("user_favorites.id = :pk"));
        assert_eq!(
            favorite_hard_delete_sql(),
            "DELETE FROM user_favorites WHERE id = :pk"
        );
        assert!(favorite_workspace_lookup_where().contains("workspaces.slug = :slug"));
        assert_eq!(FAVORITE_LIST_ORDER_SQL, "user_favorites.created_at DESC");
    }

    #[test]
    fn r7_drafts_annotations_reread_and_writes() {
        // Cycle subquery: LIMIT 1, NO order by (ported bug 6).
        let cycle = draft_cycle_subquery_sql();
        assert!(cycle.contains("draft_issue_cycles dic"));
        assert!(cycle.contains("dic.draft_issue_id = draft_issues.id"));
        assert!(cycle.contains("LIMIT 1"));
        assert!(!cycle.contains("ORDER BY"));
        // Three Coalesce ArrayAggs with guards.
        let labels = draft_label_ids_annotation_sql();
        assert!(labels.starts_with("COALESCE(ARRAY_AGG(DISTINCT labels.id)"));
        assert!(labels.contains("labels.id IS NOT NULL"));
        assert!(labels.contains("draft_issue_labels.deleted_at IS NULL"));
        assert!(labels.ends_with(", '{}') AS label_ids"));
        let assignees = draft_assignee_ids_annotation_sql();
        assert!(assignees.contains("ARRAY_AGG(DISTINCT users.id)"));
        assert!(assignees.contains("pm.is_active = TRUE"));
        assert!(assignees.contains("draft_issue_assignees.deleted_at IS NULL"));
        // Ported bug 7: no project scoping on the member leg.
        assert!(!assignees.contains("project_id"));
        let modules = draft_module_ids_annotation_sql();
        assert!(modules.contains("ARRAY_AGG(DISTINCT draft_issue_modules.module_id)"));
        assert!(modules.contains("modules.archived_at IS NULL"));
        assert!(modules.contains("draft_issue_modules.deleted_at IS NULL"));
        assert_eq!(
            DRAFT_SELECT_RELATED,
            &["workspace", "project", "state", "parent"]
        );
        assert_eq!(
            DRAFT_PREFETCH_RELATED,
            &["assignees", "labels", "draft_issue_module__module"]
        );
        // List: own + distinct + explicit order + paginate 1000.
        assert_eq!(DRAFT_LIST_ORDER_SQL, "draft_issues.created_at DESC");
        assert_eq!(DRAFT_LIST_PER_PAGE, 1000);
        let list = draft_list_sql();
        assert!(list.starts_with("SELECT DISTINCT draft_issues.*"));
        assert!(list.contains("draft_issues.created_by_id = :user"));
        // Create re-read: 21 keys, source order, verbatim spellings.
        assert_eq!(DRAFT_CREATE_READ_KEYS.len(), 21);
        assert_eq!(DRAFT_CREATE_READ_KEYS[11], "cycle_id");
        assert_eq!(DRAFT_CREATE_READ_KEYS[12], "module_ids");
        assert_eq!(DRAFT_CREATE_READ_KEYS[13], "label_ids");
        assert_eq!(DRAFT_CREATE_READ_KEYS[14], "assignee_ids");
        assert_eq!(DRAFT_CREATE_READ_KEYS[5], "estimate_point");
        assert_eq!(DRAFT_CREATE_READ_KEYS[17], "created_by");
        assert_eq!(DRAFT_CREATE_READ_KEYS[19], "type_id");
        assert_eq!(DRAFT_CREATE_READ_KEYS[20], "description_html");
        let reread = draft_create_reread_sql();
        assert!(reread.contains("draft_issues.id = :pk"));
        assert!(reread.ends_with("ORDER BY draft_issues.created_at DESC LIMIT 1"));
        // Own lookup vs destroy lookup (no created_by on destroy).
        let own = draft_own_lookup_where();
        assert!(own.contains("draft_issues.created_by_id = :user"));
        let destroy_lookup = draft_destroy_lookup_where();
        assert!(!destroy_lookup.contains("created_by"));
        assert_eq!(DRAFT_PATCH_CYCLE_ID_DEFAULT, "not_provided");
        // Destroy is SOFT (code truth; rows.json "hard" wording is stale).
        let soft = draft_soft_delete_sql();
        assert!(soft.contains("SET deleted_at = :now, updated_at = :now"));
        assert!(!soft.to_uppercase().starts_with("DELETE FROM"));
        // Draft-to-issue writes.
        assert!(draft_to_issue_fetch_where().contains("draft_issues.id = :draft"));
        let cycle_insert = cycle_issue_insert_sql();
        assert!(cycle_insert.contains("INSERT INTO cycle_issues"));
        assert!(cycle_insert.contains(":creator"));
        assert_eq!(MODULE_ISSUE_BULK_BATCH_SIZE, 10);
        let module_insert = module_issue_bulk_insert_sql();
        assert!(module_insert.contains("INSERT INTO module_issues"));
        assert!(!module_insert.contains("ON CONFLICT"));
        assert_eq!(FILE_ASSET_ISSUE_DESCRIPTION, "ISSUE_DESCRIPTION");
        let repoint = file_asset_repoint_sql();
        assert!(repoint.contains("entity_type = 'ISSUE_DESCRIPTION'"));
        assert!(repoint.contains("draft_issue_id = NULL"));
        assert!(repoint.contains("draft_issue_id = :draft"));
    }

    #[test]
    fn r8_quick_links_owner_scoped() {
        let owner = quick_link_owner_where();
        for needle in [
            "workspace_user_links.id = :pk",
            "slug = :slug",
            "workspace_user_links.owner_id = :user",
        ] {
            assert!(owner.contains(needle), "missing {needle}");
        }
        let patch = quick_link_patch_lookup_sql();
        assert!(patch.contains(&owner));
        assert!(patch.ends_with("ORDER BY workspace_user_links.created_at DESC LIMIT 1"));
        // Ported bug 8: same predicate, different 404 key (handler bodies).
        assert_eq!(quick_link_retrieve_where(), owner);
        let delete = quick_link_soft_delete_sql();
        assert!(delete.contains("SET deleted_at = :now, updated_at = :now"));
        assert_eq!(
            QUICK_LINK_LIST_ORDER_SQL,
            "workspace_user_links.created_at DESC"
        );
        let list = quick_link_list_sql();
        assert!(list.contains("workspace_user_links.owner_id = :user"));
        assert!(list.ends_with(&format!("ORDER BY {QUICK_LINK_LIST_ORDER_SQL}")));
    }

    #[test]
    fn r9_stickies_scope_order_and_icontains() {
        let scope = sticky_scope_where();
        assert!(scope.contains("slug = :slug"));
        assert!(scope.contains("stickies.owner_id = :user"));
        assert_eq!(STICKY_LIST_ORDER_SQL, "stickies.sort_order DESC");
        assert_eq!(STICKY_LIST_PER_PAGE, 20);
        let sql = sticky_list_sql();
        assert!(sql.starts_with("SELECT DISTINCT stickies.*"));
        assert!(sql.contains("JOIN users owners ON owners.id = stickies.owner_id"));
        assert!(sql.ends_with("ORDER BY stickies.sort_order DESC"));
        // icontains -> ILIKE with LIKE-escaping.
        assert_eq!(
            sticky_query_where(),
            "stickies.description_stripped ILIKE :query"
        );
        assert_eq!(escape_icontains("plain"), "plain");
        assert_eq!(escape_icontains("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_icontains("%_%"), "\\%\\_\\%");
    }

    #[test]
    fn r10_home_autocreate_loop_quirk() {
        assert_eq!(HOME_PREF_KEYS, &["quick_links", "recents", "my_stickies"]);
        assert_eq!(
            HOME_PREF_EXCLUDED_KEYS,
            &["quick_tutorial", "new_at_pi_dash"]
        );
        assert_eq!(HOME_PREF_BULK_BATCH_SIZE, 10);
        // Empty table: first-insert-wins 999/998/997.
        assert_eq!(
            home_autocreate_plan(&[]),
            vec![("quick_links", 999), ("recents", 998), ("my_stickies", 997)]
        );
        // Partial: pre-existing keys skipped, counter only bumps per insert.
        assert_eq!(
            home_autocreate_plan(&["recents"]),
            vec![("quick_links", 999), ("my_stickies", 998)]
        );
        assert_eq!(
            home_autocreate_plan(&["quick_links", "recents", "my_stickies"]),
            vec![]
        );
        assert_eq!(
            home_autocreate_plan(&["quick_links"]),
            vec![("recents", 999), ("my_stickies", 998)]
        );
        let insert = home_pref_insert_sql();
        assert!(insert.contains("INSERT INTO workspace_home_preferences"));
        assert!(insert.ends_with("ON CONFLICT DO NOTHING"));
        assert_eq!(
            HOME_PREF_RESPONSE_KEYS,
            &["key", "is_enabled", "config", "sort_order"]
        );
        assert_eq!(
            HOME_PREF_RESPONSE_ORDER_SQL,
            "workspace_home_preferences.created_at DESC"
        );
        let response = home_pref_response_sql();
        assert!(response.contains("SELECT key, is_enabled, config, sort_order"));
        // PATCH lookup keeps the user filter (contrast sidebar).
        let patch = home_pref_patch_lookup_sql();
        assert!(patch.contains("workspace_home_preferences.key = :key"));
        assert!(patch.contains("workspace_home_preferences.user_id = :user"));
        assert!(patch.ends_with("ORDER BY workspace_home_preferences.created_at DESC LIMIT 1"));
    }

    #[test]
    fn r11_sidebar_autocreate_and_cross_user_patch() {
        assert_eq!(
            SIDEBAR_PREF_KEYS,
            &[
                "views",
                "active_cycles",
                "analytics",
                "drafts",
                "your_work",
                "archives",
                "stickies"
            ]
        );
        assert_eq!(SIDEBAR_PINNED_KEYS, &["drafts", "your_work", "stickies"]);
        assert!(sidebar_pref_pinned("drafts"));
        assert!(sidebar_pref_pinned("your_work"));
        assert!(sidebar_pref_pinned("stickies"));
        assert!(!sidebar_pref_pinned("views"));
        assert_eq!(SIDEBAR_PREF_BULK_BATCH_SIZE, 10);
        // Empty table: 65535 + i*10000 over the growing list.
        assert_eq!(
            sidebar_autocreate_plan(&[]),
            vec![
                ("views", false, 65535),
                ("active_cycles", false, 75535),
                ("analytics", false, 85535),
                ("drafts", true, 95535),
                ("your_work", true, 105535),
                ("archives", false, 115535),
                ("stickies", true, 125535),
            ]
        );
        // Partial: growing-list indexes shift later keys DOWN.
        assert_eq!(
            sidebar_autocreate_plan(&["views", "active_cycles"]),
            vec![
                ("analytics", false, 65535),
                ("drafts", true, 75535),
                ("your_work", true, 85535),
                ("archives", false, 95535),
                ("stickies", true, 105535),
            ]
        );
        assert_eq!(
            SIDEBAR_PREF_RESPONSE_KEYS,
            &["key", "is_pinned", "sort_order"]
        );
        let response = sidebar_pref_response_sql();
        assert!(response.ends_with("ORDER BY workspace_user_preferences.sort_order ASC"));
        // Ported bug 11: NO user filter on the PATCH lookup.
        let lookup = sidebar_pref_patch_lookup_sql();
        assert!(lookup.contains("workspace_user_preferences.key = :key"));
        assert!(lookup.contains("slug = :slug"));
        assert!(!lookup.contains("user_id"));
        // update_fields writes both columns, no updated_at.
        let patch = sidebar_pref_patch_sql();
        assert!(patch.contains("SET is_pinned = :pinned, sort_order = :sort"));
        assert!(!patch.contains("updated_at"));
    }

    #[test]
    fn r12_visits_clamp_and_cap() {
        assert_eq!(VISIT_ENTITY_ALLOWLIST, &["issue", "page", "project"]);
        assert_eq!(RECENT_VISITS_CAP, 20);
        // The hard clamp applies with and without ?entity_name=.
        for sql in [
            recent_visits_where(None),
            recent_visits_where(Some("issue")),
        ] {
            assert!(sql.contains("user_recent_visits.entity_name IN ('issue', 'page', 'project')"));
        }
        assert!(!recent_visits_where(None).contains(":entity"));
        // `if entity_name:` is a truthiness check — an empty param adds no filter.
        assert_eq!(recent_visits_where(Some("")), recent_visits_where(None));
        assert!(
            recent_visits_where(Some("cycle")).contains("user_recent_visits.entity_name = :entity")
        );
        let sql = recent_visits_sql(Some("page"));
        assert!(sql.ends_with("ORDER BY user_recent_visits.created_at DESC LIMIT 20"));
    }

    #[test]
    fn fixture_sql_names_every_unit() {
        // The R1-R12 markers in extras_user.sql are the port's checklist
        // (R13-R16 belong to sibling issues 612/608/610).
        let sql = sql_fixture();
        for marker in [
            "R1 labels",
            "R2 states",
            "R3 estimates",
            "R4 modules",
            "R5 cycles",
            "R6 favorites",
            "R7 drafts",
            "R8 quick-links",
            "R9 stickies",
            "R10 home prefs",
            "R11 sidebar prefs",
            "R12 recent visits",
        ] {
            assert!(sql.contains(marker), "fixture missing {marker}");
        }
    }

    #[test]
    fn fixture_rows_match_builders() {
        let fixture = rows_fixture();
        assert_eq!(fixture["cases"].as_array().expect("cases").len(), 13);
        // Labels: member-gated, unarchived projects.
        let labels = case(&fixture, "labels");
        assert_eq!(labels["source"], "views/workspace/label.py:23-28");
        assert_eq!(labels["row"]["member_of_project"], "included");
        assert_eq!(labels["row"]["archived_project"], "excluded (:27)");
        // States: triage excluded, order = index/count.
        let states = case(&fixture, "states");
        assert_eq!(states["source"], "views/workspace/state.py:22-40");
        assert_eq!(states["row"]["is_triage_excluded"], true);
        assert_eq!(states["row"]["order"], 0.5);
        assert_eq!(
            state_group_order(2, 4),
            states["row"]["order"].as_f64().unwrap()
        );
        // Estimates: no member scoping, points prefetch.
        let estimates = case(&fixture, "estimates");
        assert_eq!(estimates["source"], "views/workspace/estimate.py:23-30");
        assert!(estimates["row"]["member_scoping"]
            .as_str()
            .unwrap_or("")
            .contains("NONE"));
        assert!(!estimate_ids_q1_sql().contains("member"));
        // Modules: six counts, always -created_at.
        let modules = case(&fixture, "modules");
        assert_eq!(modules["source"], "views/workspace/module.py:36-108");
        assert_eq!(modules["row"]["total_issues"], 3);
        assert_eq!(modules["row"]["completed_issues"], 1);
        assert!(modules["row"]["order"]
            .as_str()
            .unwrap_or("")
            .contains("-created_at"));
        // Cycles: distinct + order.
        let cycles = case(&fixture, "cycles");
        assert_eq!(cycles["source"], "views/workspace/cycle.py:29-101");
        assert_eq!(cycles["row"]["distinct"], true);
        assert!(cycle_list_sql().starts_with("SELECT DISTINCT"));
        // Favorites: branches + writes.
        let favorites = case(&fixture, "favorites");
        assert_eq!(favorites["source"], "views/workspace/favorite.py:26-97");
        assert!(favorites["row"]["top_get"]
            .as_str()
            .unwrap_or("")
            .contains("member-project"));
        assert!(favorites["row"]["group_get"]
            .as_str()
            .unwrap_or("")
            .contains("no page exclusion"));
        assert!(!favorite_group_branch_where().contains("page"));
        assert_eq!(
            favorites["writes"]["delete"],
            "get+delete(soft=False)->204 (:80-82)"
        );
        assert!(favorite_hard_delete_sql().starts_with("DELETE FROM"));
        // Drafts: annotations + writes. Two rows.json wordings disagree
        // with the code and are NOT pinned as truth here: "19 .values
        // cols" (code has 21, draft.py:127-149) and "hard delete()"
        // (code is SOFT — no delete() override in db/models/draft.py).
        let drafts = case(&fixture, "drafts");
        assert_eq!(drafts["source"], "views/workspace/draft.py:49-203");
        assert_eq!(drafts["row"]["cycle_id"], "subquery [:1] (:55-60)");
        assert!(!draft_cycle_subquery_sql().contains("ORDER BY"));
        assert_eq!(DRAFT_CREATE_READ_KEYS.len(), 21);
        assert!(drafts["row"]["create_refetch"]
            .as_str()
            .unwrap_or("")
            .contains(":124-153"));
        assert!(draft_soft_delete_sql().contains("SET deleted_at = :now"));
        assert!(drafts["writes"]["draft_to_issue"]
            .as_str()
            .unwrap_or("")
            .contains("bulk batch10"));
        assert_eq!(MODULE_ISSUE_BULK_BATCH_SIZE, 10);
        // Home prefs: autocreate trace.
        let home = case(&fixture, "home_prefs");
        assert_eq!(home["source"], "views/workspace/home.py:27-79");
        assert_eq!(
            home["row"]["autocreated"],
            "[quick_links:999, recents:998, my_stickies:997] (first-insert-wins)"
        );
        let plan = home_autocreate_plan(&[]);
        assert_eq!(
            format!(
                "[{}] (first-insert-wins)",
                plan.iter()
                    .map(|(k, s)| format!("{k}:{s}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            home["row"]["autocreated"]
        );
        // Sidebar prefs: 7 keys, pinned set.
        let sidebar = case(&fixture, "sidebar_prefs");
        assert_eq!(
            sidebar["source"],
            "views/workspace/user_preference.py:29-101"
        );
        assert!(sidebar["row"]["autocreated"]
            .as_str()
            .unwrap_or("")
            .contains("pinned=[drafts,your_work,stickies]"));
        assert_eq!(sidebar_autocreate_plan(&[]).len(), 7);
        // Recent visits: clamp + cap.
        let visits = case(&fixture, "recent_visits");
        assert_eq!(visits["source"], "views/workspace/recent_visit.py:26-36");
        assert!(visits["row"]["entity_filter"]
            .as_str()
            .unwrap_or("")
            .contains("HARD"));
        assert_eq!(RECENT_VISITS_CAP, 20);
        // All 14 recorded bugs present (this issue's 8 among them).
        let bugs = fixture["bugs"].as_array().expect("bugs array");
        assert_eq!(bugs.len(), 14);
        for needle in [
            "order_by",
            "distinct=True",
            "precedence",
            "in memory",
            "bulk_create inside the per-key loop",
            "lacks user filter",
            "404 key differs",
            "ModelViewSet default",
        ] {
            assert!(
                bugs.iter()
                    .any(|b| b.as_str().unwrap_or("").contains(needle)),
                "bug note missing: {needle}"
            );
        }
    }
}
