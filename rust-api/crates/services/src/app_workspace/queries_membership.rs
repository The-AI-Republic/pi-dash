#![forbid(unsafe_code)]

//! Workspace membership / invite / join-request query builders (D-24, stage 5).
//!
//! Ports the queryset and write shapes behind workspace members, workspace
//! invites + join, and workspace join-requests as SQL text, following the
//! D-27/D-28/D-30 precedent (`app_cycles/queries.rs`,
//! `app_modules/queries.rs`, `app_pages/queries.rs`): each builder returns a
//! fragment (or a representative statement) the caller splices into the
//! statement it executes. The services crate carries no `sea-query`/`sqlx`
//! dependency (foundation crates are read-only for port agents), so
//! placeholders stay symbolic — `:slug`, `:user`, `:target`, `:email`,
//! `:pk`, `:pks`, `:ws`, `:role`, `:status`, `:now`, `:project_ids` —
//! exactly the notation the fixtures use; handlers bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/workspace/member.py:30-75` — member list/retrieve.
//! - `app/views/workspace/member.py:76-206` — partial_update, destroy, leave.
//! - `app/views/workspace/member.py:208-265` — views post, me, project-members.
//! - `app/views/workspace/invite.py:37-305` — invites, join, my-invitations.
//! - `app/views/workspace/join_request.py:32-255` — user + admin join-requests.
//! - `app/permissions/base.py:13-16` — `ROLE` (`ADMIN = 20`, `MEMBER = 15`,
//!   `GUEST = 5`).
//! - `db/models/workspace.py:306-313` — join-request partial unique.
//! - `db/mixins.py:48-82` — soft-delete manager (`deleted_at IS NULL`) and
//!   soft-by-default `.delete()`.
//!
//! Fixture oracle: F-W24-10 (`fixtures/app_workspace/queries/`
//! `membership.sql` R1-R19 + `membership.rows.json`). The unit tests below
//! pin the builders against those files so transcription drift fails the
//! build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. R1: member list has NO `is_active` filter — inactive rows are listed.
//! 2. R2 vs R3: the same `role > 5` threshold is spelled as a literal
//!    (`:51`) and as `ROLE.GUEST.value` (`:70`) — both spellings kept.
//! 3. R4: the guest-demote cascade (`:89`) has no `is_active` filter — it
//!    rewrites inactive project rows too.
//! 4. R5: the destroy sole-project-admin guard (`:128`) compares
//!    `member_id` (a User FK) to `workspace_member.id` (a WorkspaceMember
//!    PK) — it never fires. R6 (`:182`) uses `request.user.id` and works.
//!    Both shapes are kept, never unified.
//! 5. R5/R6: equal roles CAN remove — the guard is strict `<` (`:116`).
//! 6. R6: the sole-workspace-admin guard is spelled `not count > 1`
//!    (`:167`), i.e. `count <= 1` — the spelling is kept.
//! 7. R8: the involved-project-ids query (`:245-249`) has no slug filter —
//!    it spans all workspaces.
//! 8. R10: the invite JWT payload (`:97`) encodes the whole email dict
//!    (address plus role), not the address.
//! 9. R10/R14: `bulk_create(..., ignore_conflicts=True)` silently skips
//!    duplicate rows while the endpoint still returns success.
//! 10. R12: when the invited user is missing or rejects, the invite is kept
//!     but `responded_at` is already set — the row is stuck as "already
//!     responded" forever.
//! 11. R12 vs R18: the last-workspace pointer is written to the User model
//!     (`:204`, a transient attribute Django drops on save — the `users`
//!     row is still touched for `updated_at`/audit) versus the Profile
//!     model (`join_request.py:219`, persisted) — both paths kept.
//! 12. R12 create passes no `created_by` while R14 bulk-create sets it.
//! 13. R12/R13 join endpoints are `AllowAny` — the invite is readable and
//!     joinable without a session (token only).
//! 14. R15: `select_related("requester")` on a requester-scoped queryset is
//!     a self-join no-op.
//! 15. R16: the unresolved (`workspace IS NULL`) branch escapes the partial
//!     unique — concurrent duplicates are possible and de-duped on read.
//! 16. R19: deny is a single save with NO transaction while approve is
//!     `transaction.atomic`.
//!
//! Out of scope (owned by sibling issues): response envelopes and error
//! bodies (handlers B/C, PIDASHCONV-616/617); serializer shapes
//! (PIDASHCONV-600/601); model column lists (PIDASHCONV-605/607); permission
//! gates, throttles and cache sites (PIDASHCONV-613); Celery enqueues
//! (PIDASHCONV-614); JWT signing (handlers, via foundation `SECRET_KEY`).

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `ROLE.ADMIN.value` (`app/permissions/base.py:14`).
pub const ROLE_ADMIN: i32 = 20;

/// `ROLE.MEMBER.value` (`app/permissions/base.py:15`).
pub const ROLE_MEMBER: i32 = 15;

/// `ROLE.GUEST.value` (`app/permissions/base.py:16`).
pub const ROLE_GUEST: i32 = 5;

/// `ADMIN_ROLE` (`app/views/workspace/join_request.py:29`): the literal `20`
/// the join-request create path resolves admin targets with.
pub const ADMIN_ROLE: i32 = 20;

/// `WorkspaceJoinRequest.Status.PENDING` (`db/models/workspace.py:271-274`).
pub const JOIN_REQUEST_PENDING: &str = "PENDING";

/// `WorkspaceJoinRequest.Status.APPROVED` (`db/models/workspace.py:271-274`).
pub const JOIN_REQUEST_APPROVED: &str = "APPROVED";

/// `WorkspaceJoinRequest.Status.DENIED` (`db/models/workspace.py:271-274`).
pub const JOIN_REQUEST_DENIED: &str = "DENIED";

/// `WorkspaceJoinRequest.role` Django-side default (`db/models/workspace.py:295`):
/// `15` (Member) — note this is NOT the `5` (Guest) default that
/// `WorkspaceMember` and `WorkspaceMemberInvite` carry.
pub const JOIN_REQUEST_DEFAULT_ROLE: i32 = 15;

/// Serializer field subset for member list/retrieve (`member.py:52,54,71,73`):
/// `fields=("id", "member", "role")`, identical on both serializer branches.
pub const MEMBER_LIST_FIELDS: &[&str] = &["id", "member", "role"];

/// Slug subquery shared by every FK-traversal slug filter
/// (`workspace__slug=:slug`, `member.py`/`invite.py`/`join_request.py`): a
/// plain join predicate with NO `deleted_at` guard — cross-FK filters do not
/// apply the related model's manager. Direct `Workspace.objects.get`
/// lookups instead use [`workspace_lookup_where`], which carries the manager
/// scope.
pub fn workspace_id_by_slug_sql() -> String {
    "(SELECT id FROM workspaces WHERE slug = :slug)".to_owned()
}

// ---------------------------------------------------------------------------
// R1 member get_queryset (member.py:37-43)
// ---------------------------------------------------------------------------

/// R1 scope (`:41`): `workspace__slug=:slug` over the soft-delete manager.
/// Ported bug 1: there is NO `is_active` filter — inactive rows ARE listed.
pub fn member_scope_where() -> String {
    format!(
        "workspace_members.workspace_id = {} AND workspace_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R1 search (`:34` + `SearchFilter`): `?search=` across
/// `member__display_name` / `member__first_name` as `icontains` (OR within
/// one term, AND across space-separated terms). Handlers bind `:search` as
/// `%<term>%` per term.
pub fn member_search_where() -> String {
    "(users.display_name ILIKE :search OR users.first_name ILIKE :search)".to_owned()
}

/// R1 list order: `WorkspaceMember.Meta.ordering = ("-created_at",)`
/// (`db/models/workspace.py:228`).
pub const MEMBER_LIST_ORDER_SQL: &str = "workspace_members.created_at DESC";

/// Full representative R1 SELECT: scope + `select_related("member",
/// "member__avatar_asset")` (`:42`) as joins (fetch hints — same rows with
/// or without them; the avatar join is `LEFT` because
/// `User.avatar_asset` is nullable, `db/models/user.py:69-75`) + default
/// ordering. `filter_queryset()` search ([`member_search_where`]) is ANDed
/// by the handler when `?search=` is present.
pub fn member_list_sql() -> String {
    format!(
        "SELECT workspace_members.*, users.*, file_assets.* FROM workspace_members JOIN users ON users.id = workspace_members.member_id LEFT JOIN file_assets ON file_assets.id = users.avatar_asset_id WHERE {} ORDER BY {}",
        member_scope_where(),
        MEMBER_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R2 list / R3 retrieve requester + role branch (member.py:45-74)
// ---------------------------------------------------------------------------

/// R2/R3 requester lookup (`:47`, `:59`, also `:60` in `invite.py`,
/// `:106-108` in `destroy`, `:162` in `leave` and `:210` in views post):
/// `.get(member=:user, workspace__slug=:slug, is_active=True)`. A bare
/// `.get` — `DoesNotExist` renders 404 `{"error": "The required object
/// does not exist."}` via `BaseViewSet`/`BaseAPIView.handle_exception`
/// (`app/views/base.py:110-150, 205-244`); handlers sequence that.
pub fn member_requester_where() -> String {
    format!(
        "workspace_members.member_id = :user AND workspace_members.workspace_id = {} AND workspace_members.is_active = TRUE AND workspace_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R3 retrieve target (`:63`): R1 scope plus `.get(pk=:pk)`.
/// `DoesNotExist` renders 404 `{"error": "Workspace member not found"}`
/// (`:64-68`, handler body).
pub fn member_retrieve_target_where() -> String {
    format!("{} AND workspace_members.id = :pk", member_scope_where())
}

/// R2 admin-vs-plain branch (`:51`): the literal spelling `role > 5`.
pub fn admin_branch_literal_sql() -> &'static str {
    "role > 5"
}

/// R3 admin-vs-plain branch (`:70`): the enum spelling
/// `role > ROLE.GUEST.value`. Same threshold as [`admin_branch_literal_sql`]
/// (ported bug 2) — both spellings are kept.
pub fn admin_branch_enum_sql() -> &'static str {
    "role > ROLE.GUEST.value"
}

/// Shared predicate behind both branch spellings: `role > 5` selects the
/// admin serializer shape, else the plain member shape (both over
/// [`MEMBER_LIST_FIELDS`]).
pub fn is_admin_shape(role: i32) -> bool {
    role > ROLE_GUEST
}

// ---------------------------------------------------------------------------
// R4 partial_update guest-demote cascade (member.py:76-96)
// ---------------------------------------------------------------------------

/// R4/R5 write target (`:78-80` + `:101-103`, same predicate set in a
/// different source arg order): `.get(pk=:pk, workspace__slug=:slug,
/// member__is_bot=False, is_active=True)`. Requires the `JOIN users` for
/// the bot guard. The self-update/self-remove guards compare
/// `request.user.id == target.member_id` (`:81`, 400) and
/// `str(target.id) == str(requester.id)` (`:110`, 400) respectively
/// (handler sequencing).
pub fn member_write_target_where() -> String {
    format!(
        "workspace_members.id = :pk AND workspace_members.workspace_id = {} AND users.is_bot = FALSE AND workspace_members.is_active = TRUE AND workspace_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R4 demote trigger role (`:88`): `"role" in data and int(data["role"]) ==
/// 5` — the literal `5`, verbatim.
pub const GUEST_DEMOTE_ROLE: i32 = 5;

/// R4 guest-demote cascade (`:89`): `UPDATE project_members SET role=5
/// WHERE workspace__slug=:slug AND member_id=:target`. Ported bug 3: NO
/// `is_active` filter — inactive project rows are rewritten too. The
/// `deleted_at` guard comes from the default manager underneath.
pub fn guest_demote_cascade_sql() -> String {
    format!(
        "UPDATE project_members SET role = 5 WHERE project_members.workspace_id = {} AND project_members.member_id = :target AND project_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

// ---------------------------------------------------------------------------
// R5 destroy / R6 leave guards + deactivations (member.py:98-205)
// ---------------------------------------------------------------------------

/// R5/R6 sole-project-admin guard (`:122-135` + `:176-189`):
/// `EXISTS(projects annotated total_members=1 AND member_with_role=1)` over
/// the slug. `member_param` selects the compared id: pass `":wm_pk"` for
/// R5 (ported bug 4 — `workspace_member.id`, the WorkspaceMember PK
/// compared against the User FK, so the guard NEVER fires) and `":user"`
/// for R6 (`request.user.id`, correct). Both shapes are kept, never
/// unified. As observed, the counted `project_members` rows carry neither
/// an `is_active` nor a `deleted_at` filter; the `projects` base carries
/// the manager scope.
pub fn sole_project_admin_exists_sql(member_param: &str) -> String {
    format!(
        "SELECT EXISTS(SELECT 1 FROM projects p LEFT JOIN project_members pm ON pm.project_id = p.id WHERE p.workspace_id = {} AND p.deleted_at IS NULL GROUP BY p.id HAVING COUNT(pm.id) = 1 AND COUNT(CASE WHEN pm.member_id = {} AND pm.role = 20 THEN 1 END) = 1)",
        workspace_id_by_slug_sql(),
        member_param
    )
}

/// R5 higher-role guard (`:116`): `requester.role < target.role` blocks.
/// Strict less — equal roles CAN remove (ported bug 5).
pub fn can_remove(requester_role: i32, target_role: i32) -> bool {
    requester_role >= target_role
}

/// R5/R6 project deactivation (`:144-146` + `:198-200`): `UPDATE
/// project_members SET is_active=FALSE, updated_at=:now WHERE
/// workspace__slug=:slug AND member_id=:target AND is_active=TRUE`.
pub fn project_deactivate_sql() -> String {
    format!(
        "UPDATE project_members SET is_active = FALSE, updated_at = :now WHERE project_members.workspace_id = {} AND project_members.member_id = :target AND project_members.is_active = TRUE AND project_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R5/R6 member deactivation (`:148-149` + `:203-204`):
/// `workspace_member.is_active = False; save()` renders an `UPDATE` on the
/// row (`save()` additionally stamps `updated_by` from the request user via
/// `BaseModel.save`, `db/models/base.py:23-44` — handler sequencing).
/// Both paths answer 204.
pub fn member_deactivate_sql() -> String {
    "UPDATE workspace_members SET is_active = FALSE, updated_at = :now WHERE id = :pk".to_owned()
}

/// R6 sole-workspace-admin count (`:167`): active admins in the workspace.
pub fn sole_workspace_admin_count_sql() -> String {
    format!(
        "SELECT COUNT(*) FROM workspace_members WHERE workspace_members.workspace_id = {} AND workspace_members.role = 20 AND workspace_members.is_active = TRUE AND workspace_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R6 sole-workspace-admin guard (`:165-168`): fires when the leaver is an
/// admin and `count <= 1` (Python `not count > 1`, ported bug 6; simplified
/// from the negated spelling — behavior-identical for integers).
pub fn sole_workspace_admin_blocks(requester_role: i32, admin_count: i64) -> bool {
    requester_role == ROLE_ADMIN && admin_count <= 1
}

// ---------------------------------------------------------------------------
// R7 me draft count (member.py:217-234)
// ---------------------------------------------------------------------------

/// R7 `draft_issue_count` annotation (`:221-230`): `Coalesce(Subquery(
/// DraftIssue filter created_by=:user, workspace_id=OuterRef(workspace_id)
/// GROUP BY workspace_id, Count(id)), 0)`. The inner `GROUP BY` means an
/// empty match returns NO row (not a zero row) — that is why the
/// `Coalesce(..., 0)` guard exists. `Count("id")` renders `COUNT(d.id)`.
pub fn draft_count_annotation_sql() -> String {
    "COALESCE((SELECT COUNT(d.id) FROM draft_issues d WHERE d.created_by_id = :user AND d.workspace_id = workspace_members.workspace_id AND d.deleted_at IS NULL GROUP BY d.workspace_id), 0) AS draft_issue_count".to_owned()
}

/// R7 lookup (`:229-231`): `.filter(member=:user, workspace__slug=:slug,
/// is_active=True).first()` with NO explicit `order_by` — `.first()` falls
/// back to `Meta.ordering` (`-created_at`).
pub fn me_lookup_where() -> String {
    format!(
        "workspace_members.member_id = :user AND workspace_members.workspace_id = {} AND workspace_members.is_active = TRUE AND workspace_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// Full representative R7 SELECT: lookup + [`draft_count_annotation_sql`].
pub fn me_sql() -> String {
    format!(
        "SELECT workspace_members.*, {} FROM workspace_members WHERE {} ORDER BY {} LIMIT 1",
        draft_count_annotation_sql(),
        me_lookup_where(),
        MEMBER_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// Views-post view_props save (member.py:208-214)
// ---------------------------------------------------------------------------

/// Views-post save (`:210-212`): `view_props = ...; save()` → full-row
/// `UPDATE` stamping `updated_at` (`BaseModel.save` also stamps
/// `updated_by` from the request user via crum, `db/models/base.py:23-44`
/// — handler sequencing). The lookup is the [`member_requester_where`]
/// predicate set (`:210`); the path answers 204.
pub fn member_view_props_sql() -> String {
    "UPDATE workspace_members SET view_props = :props, updated_at = :now WHERE id = :pk".to_owned()
}

// ---------------------------------------------------------------------------
// R8 project-members dict (member.py:243-265)
// ---------------------------------------------------------------------------

/// R8 involved-project ids (`:245-249`): `DISTINCT project_id WHERE
/// member=:user AND is_active`. Ported bug 7: NO slug filter — the ids span
/// ALL workspaces; the slug applies only in [`project_members_scope_where`].
pub fn involved_project_ids_sql() -> String {
    "SELECT DISTINCT pm.project_id FROM project_members pm WHERE pm.member_id = :user AND pm.is_active = TRUE AND pm.deleted_at IS NULL".to_owned()
}

/// R8 members scope (`:252-254`): `WHERE workspace__slug=:slug AND
/// project_id IN (:project_ids) AND is_active`, with
/// `select_related("project", "member", "workspace")` as fetch joins.
/// Handlers then group rows into a dict keyed by `str(project_id)`, popping
/// the `"project"` key out of each row (`:257-264`).
pub fn project_members_scope_where() -> String {
    format!(
        "project_members.workspace_id = {} AND project_members.project_id IN (:project_ids) AND project_members.is_active = TRUE AND project_members.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

// ---------------------------------------------------------------------------
// R9 invite list get_queryset (invite.py:45-51)
// ---------------------------------------------------------------------------

/// R9 scope (`:49`): `workspace__slug=:slug` over the soft-delete manager.
/// List/retrieve are the inherited `ModelViewSet` actions over this scope.
pub fn invite_scope_where() -> String {
    format!(
        "workspace_member_invites.workspace_id = {} AND workspace_member_invites.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R9 list order: `WorkspaceMemberInvite.Meta.ordering = ("-created_at",)`
/// (`db/models/workspace.py:256`).
pub const INVITE_LIST_ORDER_SQL: &str = "workspace_member_invites.created_at DESC";

/// Full representative R9 SELECT: scope +
/// `select_related("workspace", "workspace__owner", "created_by")` (`:50`)
/// as joins (the owner join is inner — `Workspace.owner` is non-nullable,
/// `db/models/workspace.py:131-135`; the `created_by` join is `LEFT` —
/// audit FKs are nullable, `db/mixins.py:29-35`) + default ordering.
pub fn invite_list_sql() -> String {
    format!(
        "SELECT workspace_member_invites.*, workspaces.*, owners.*, creators.* FROM workspace_member_invites JOIN workspaces ON workspaces.id = workspace_member_invites.workspace_id JOIN users owners ON owners.id = workspaces.owner_id LEFT JOIN users creators ON creators.id = workspace_member_invites.created_by_id WHERE {} ORDER BY {}",
        invite_scope_where(),
        INVITE_LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// R10 invite create (invite.py:53-142)
// ---------------------------------------------------------------------------

/// R10 role cap (`:63`): `int(email.get("role", 5)) > requesting_user.role`
/// blocks with 400 — equal roles are OK. `requesting_user` is the
/// [`member_requester_where`] lookup (bare `.get`, 404 for non-members).
pub fn invite_role_cap_blocks(invited_role: i32, requester_role: i32) -> bool {
    invited_role > requester_role
}

/// R10 workspace lookup (`:70`): `Workspace.objects.get(slug=:slug)` — a
/// direct manager get, so the `deleted_at` scope applies (contrast the
/// join-shaped [`workspace_id_by_slug_sql`]).
pub fn workspace_lookup_where() -> String {
    "workspaces.slug = :slug AND workspaces.deleted_at IS NULL".to_owned()
}

/// R10 already-member check (`:73-79`): `filter(workspace_id=:ws,
/// member__email__in=[...], is_active=True)` plus `if queryset:` (`:79`).
/// Ported as observed: truthiness fetches ALL matching rows (with
/// `select_related("member", "member__avatar_asset")` for the 400
/// `workspace_users` body) — it is NOT an `EXISTS`. Requires the `JOIN
/// users` for the email match.
pub fn invite_already_member_where() -> String {
    "workspace_members.workspace_id = :ws AND users.email IN (:emails) AND workspace_members.is_active = TRUE AND workspace_members.deleted_at IS NULL".to_owned()
}

/// R10 bulk batch (`:113-115`): `bulk_create(..., batch_size=10,
/// ignore_conflicts=True)`. R14's bulk-create passes no `batch_size`
/// (Django default) — the difference is kept.
pub const INVITE_BULK_BATCH_SIZE: i32 = 10;

/// R10 bulk insert (`:113-115`): `INSERT ... ON CONFLICT DO NOTHING`.
/// Ported bug 9: duplicate `(email, workspace)` rows are SILENTLY skipped
/// (partial unique `workspace_member_invite_unique_email_workspace_...`,
/// `db/models/workspace.py:245-250`) while the endpoint still answers
/// success. Per-email `validate_email` (`:91`), `strip().lower()` (`:94`)
/// and role default `5` (`:101`) are handler sequencing.
pub fn invite_bulk_insert_sql() -> String {
    "INSERT INTO workspace_member_invites (id, created_at, updated_at, created_by_id, email, workspace_id, token, role) VALUES (:id, :now, :now, :user, :email, :ws, :token, :role) ON CONFLICT DO NOTHING".to_owned()
}

/// R10 token claims (`:96-100`): `jwt.encode({"email": ..., "timestamp":
/// ...}, SECRET_KEY, HS256)`. Ported bug 8: the `"email"` claim holds the
/// WHOLE email dict (address plus role), not the address. Signing lives
/// with handlers (PIDASHCONV-617); the claim set is pinned here.
pub const INVITE_TOKEN_CLAIMS: &[&str] = &["email", "timestamp"];

// ---------------------------------------------------------------------------
// R11 invite destroy (invite.py:144-147)
// ---------------------------------------------------------------------------

/// R11/R12/R13 invite lookup (`:145`, `:164`, `:239`):
/// `.get(pk=:pk, workspace__slug=:slug)`. No responded/accepted check
/// anywhere on this path. R12/R13 render `DoesNotExist` as 404 `{"error":
/// "The required object does not exist."}` (bare `.get` through the
/// base-view handler); R11 inherits the same.
pub fn invite_lookup_where() -> String {
    format!(
        "workspace_member_invites.id = :pk AND workspace_member_invites.workspace_id = {} AND workspace_member_invites.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R11/R12 instance invite delete (`:146`, `:220`):
/// `SoftDeleteModel.delete(soft=True)` stamps `deleted_at` and runs a full
/// `save()` (so `updated_at` is stamped too), plus enqueues
/// `soft_delete_related_objects` (`db/mixins.py:72-82`; the enqueue is owned
/// by the jobs plane). R14's queryset `.delete()` is a different shape —
/// see [`my_invites_bulk_soft_delete_sql`].
pub fn invite_soft_delete_sql() -> String {
    "UPDATE workspace_member_invites SET deleted_at = :now, updated_at = :now WHERE id = :pk"
        .to_owned()
}

// ---------------------------------------------------------------------------
// R12 join post (invite.py:163-236, AllowAny :151)
// ---------------------------------------------------------------------------

/// R12 token guard (`:166-173`): `token = data.get("token", "")`; `if not
/// token or invite.token != token:` answers 403. An empty (or missing)
/// token is denied even before comparison (ported bug 13's companion:
/// without a session the token is the ONLY credential).
pub fn join_token_denied(provided: &str, expected: &str) -> bool {
    provided.is_empty() || provided != expected
}

/// R12 respond-save (`:177-179`): `accepted = ...; responded_at = now;
/// save()` → full-row `UPDATE` stamping `updated_at`. Runs only when
/// `responded_at is None` (`:176`); ported bug 10 is the stuck row this
/// write leaves behind when the user is missing or rejects.
pub fn invite_respond_sql() -> String {
    "UPDATE workspace_member_invites SET accepted = :accepted, responded_at = :now, updated_at = :now WHERE id = :pk".to_owned()
}

/// R12 user lookup (`:183`): `User.objects.filter(email=invite.email)
/// .first()` — exact (case-sensitive) match, `Meta.ordering`
/// (`-created_at`) via `.first()`. `users` carries no `deleted_at`
/// (`User` is not a `BaseModel`).
pub fn join_user_lookup_sql() -> String {
    "SELECT * FROM users WHERE users.email = :email ORDER BY users.created_at DESC LIMIT 1"
        .to_owned()
}

/// R12/R18 membership probe (`:188-190` + `join_request.py:204`):
/// `.filter(workspace=:ws, member=:user).first()`. Deliberately NO
/// `is_active` filter — the probe must find deactivated rows to reactivate
/// them. `.first()` falls back to `Meta.ordering` (`-created_at`).
pub fn member_reactivate_lookup_where() -> String {
    "workspace_members.workspace_id = :ws AND workspace_members.member_id = :user AND workspace_members.deleted_at IS NULL".to_owned()
}

/// R12 reactivate (`:192-194`): `is_active=True, role=invite.role` + save
/// (`:role` binds the invite's role; R18 binds the request's role via the
/// same statement shape).
pub fn member_reactivate_sql() -> String {
    "UPDATE workspace_members SET is_active = TRUE, role = :role, updated_at = :now WHERE id = :pk"
        .to_owned()
}

/// R12 create (`:197-201`): `WorkspaceMember(workspace, member, role)` —
/// ported bug 12: NO explicit `created_by` (contrast [`my_invite_bulk_insert_sql`]
/// and [`approve_member_insert_sql`], which set it; `bulk_create` skips
/// `save()`, so only the explicit value lands there, while this path relies
/// on `BaseModel.save` crum backfill — `None` for anonymous joins).
pub fn member_join_insert_sql() -> String {
    "INSERT INTO workspace_members (id, created_at, updated_at, workspace_id, member_id, role) VALUES (:id, :now, :now, :ws, :user, :role)".to_owned()
}

/// R12 last-workspace write (`:204-205`): `user.last_workspace_id =
/// workspace.id; user.save()`. Ported bug 11: `User` HAS no such column
/// (it lives on `Profile`, `db/models/user.py:236` — the U16 read of it
/// 500s) — Django drops the transient attribute on save, so NO pointer is
/// persisted; the emitted `users` UPDATE only stamps `updated_at`/audit.
/// R18's Profile update ([`profile_last_workspace_sql`]) is the path that
/// persists. Both are kept.
pub fn user_touch_sql() -> String {
    "UPDATE users SET updated_at = :now WHERE id = :user".to_owned()
}

// ---------------------------------------------------------------------------
// R13 join get (invite.py:238-241)
// ---------------------------------------------------------------------------

/// R13 detail (`:239`): the [`invite_lookup_where`] row rendered
/// unauthenticated (`AllowAny`, ported bug 13) — no new SQL beyond the R11
/// lookup; the serializer shape is owned by PIDASHCONV-601.
pub fn join_detail_where() -> String {
    invite_lookup_where()
}

// ---------------------------------------------------------------------------
// R14 my-invitations (invite.py:244-305)
// ---------------------------------------------------------------------------

/// R14 scope (`:250`, `:258`): `email=request.user.email`. Stale when the
/// user changed their email after the invite was sent (the invite keeps the
/// old address) — ported as observed. R14's `get_queryset` adds
/// `select_related("workspace")` as a fetch join.
pub fn my_invites_where() -> String {
    "workspace_member_invites.email = :email AND workspace_member_invites.deleted_at IS NULL"
        .to_owned()
}

/// R14 accept order (`:259`): explicit `.order_by("-created_at")`.
pub const MY_INVITES_ORDER_SQL: &str = "workspace_member_invites.created_at DESC";

/// R14 accept lookup (`:257-259`): `filter(pk__in=:pks, email=:email)
/// .order_by("-created_at")`. Another user's pks are silently ignored (the
/// email conjunct filters them out — no error).
pub fn my_invites_accept_lookup_sql() -> String {
    format!(
        "SELECT * FROM workspace_member_invites WHERE workspace_member_invites.id IN (:pks) AND {} ORDER BY {}",
        my_invites_where(),
        MY_INVITES_ORDER_SQL,
    )
}

/// R14 bulk soft-delete (`:303`): queryset `.delete()` →
/// `SoftDeletionQuerySet.delete` (`db/mixins.py:48-53`) → `.update(
/// deleted_at=now)` over the accept-lookup predicate — `SET deleted_at`
/// ONLY (`.update()` writes only the named columns, no `updated_at`),
/// multi-row. Contrast the instance delete ([`invite_soft_delete_sql`]).
pub fn my_invites_bulk_soft_delete_sql() -> String {
    format!(
        "UPDATE workspace_member_invites SET deleted_at = :now WHERE workspace_member_invites.id IN (:pks) AND {}",
        my_invites_where()
    )
}

/// R14 per-invite reactivate (`:270-272`): `UPDATE workspace_members SET
/// is_active=TRUE, role=:role WHERE workspace_id=:ws AND member=:user` —
/// no `is_active` filter (active rows are rewritten in place) and no
/// `updated_at` (`QuerySet.update()` writes only the named columns — same
/// rule as R18's [`profile_last_workspace_sql`]).
pub fn my_invite_reactivate_sql() -> String {
    "UPDATE workspace_members SET is_active = TRUE, role = :role WHERE workspace_members.workspace_id = :ws AND workspace_members.member_id = :user AND workspace_members.deleted_at IS NULL".to_owned()
}

/// R14 bulk-create (`:289-300`): `bulk_create(..., ignore_conflicts=True)`
/// with `created_by=request.user` explicit and NO `batch_size` (contrast
/// R10's [`INVITE_BULK_BATCH_SIZE`]). The update-then-create double-write
/// covers reactivate (alive row updated above) plus new (inserted here);
/// rows updated above conflict-skip here (ported bug 9). Invite rows are
/// then soft-deleted ([`my_invites_bulk_soft_delete_sql`]) and the path
/// answers 204.
pub fn my_invite_bulk_insert_sql() -> String {
    "INSERT INTO workspace_members (id, created_at, updated_at, created_by_id, workspace_id, member_id, role) VALUES (:id, :now, :now, :user, :ws, :user, :role) ON CONFLICT DO NOTHING".to_owned()
}

// ---------------------------------------------------------------------------
// R15 user join-request list (join_request.py:43-46)
// ---------------------------------------------------------------------------

/// R15/R17 list order: `WorkspaceJoinRequest.Meta.ordering =
/// ("-created_at",)` (`db/models/workspace.py:317`).
pub const JOIN_REQUEST_LIST_ORDER_SQL: &str = "workspace_join_requests.created_at DESC";

/// R15 scope (`:45`): `requester=:user`. Ported bug 14:
/// `select_related("requester")` joins the requester row back onto a
/// requester-scoped queryset — a self-join no-op, kept as observed.
/// List is the inherited `ModelViewSet` action (paginated) over
/// [`JOIN_REQUEST_LIST_ORDER_SQL`].
pub fn own_join_requests_where() -> String {
    "workspace_join_requests.requester_id = :user AND workspace_join_requests.deleted_at IS NULL"
        .to_owned()
}

// ---------------------------------------------------------------------------
// R16 join-request create (join_request.py:48-153)
// ---------------------------------------------------------------------------

/// R16 admin targets (`:70-75`): active-`ADMIN_ROLE` (`20`) memberships of
/// the typed email UNION workspaces it owns. `admin_email` is
/// `strip().lower()`ed (`:49`); both email matches are exact. Python
/// `set.update` dedupes the union; already-member ids are subtracted next
/// ([`join_request_already_member_sql`]).
pub fn join_request_admin_targets_sql() -> String {
    "SELECT wm.workspace_id FROM workspace_members wm JOIN users u ON u.id = wm.member_id WHERE u.email = :email AND wm.role = 20 AND wm.is_active = TRUE AND wm.deleted_at IS NULL UNION SELECT w.id FROM workspaces w WHERE w.owner_id IN (SELECT u2.id FROM users u2 WHERE u2.email = :email) AND w.deleted_at IS NULL".to_owned()
}

/// R16 already-member subtraction (`:78-83`): `filter(member=:user,
/// workspace_id__in=:targets, is_active=True)`.
pub fn join_request_already_member_sql() -> String {
    "SELECT wm.workspace_id FROM workspace_members wm WHERE wm.member_id = :user AND wm.workspace_id IN (:targets) AND wm.is_active = TRUE AND wm.deleted_at IS NULL".to_owned()
}

/// R16 all-already short-circuit (`:90-100`): earliest-created slug —
/// `ORDER BY created_at ASC` (ascending, against the `-created_at` default)
/// `.first()`. Safe to name — the requester is already a member. Answers
/// 200 with `{"message", "workspace_slug"}` (handler body).
pub fn join_request_shortcircuit_slug_sql() -> String {
    "SELECT w.slug FROM workspaces w WHERE w.id IN (:already) AND w.deleted_at IS NULL ORDER BY w.created_at ASC LIMIT 1".to_owned()
}

/// R16 pending guard (`:105-111`): per-workspace `exists()` skip —
/// `requester=:user AND workspace_id=:ws AND status=PENDING`.
pub fn join_request_pending_exists_sql() -> String {
    "SELECT EXISTS(SELECT 1 FROM workspace_join_requests jr WHERE jr.requester_id = :user AND jr.workspace_id = :ws AND jr.status = 'PENDING' AND jr.deleted_at IS NULL)".to_owned()
}

/// R16 create (`:115-125`): `atomic create(requester, workspace_id,
/// admin_email, message, created_by)` swallowing `IntegrityError` — the
/// partial unique (`requester`, `workspace` WHERE `deleted_at NULL AND
/// status PENDING`, `db/models/workspace.py:306-313`) turns a concurrent
/// loser idempotent. `role`/`status` ride their Django defaults
/// (`15`/`PENDING`, [`JOIN_REQUEST_DEFAULT_ROLE`]).
pub fn join_request_create_sql() -> String {
    "INSERT INTO workspace_join_requests (id, created_at, updated_at, created_by_id, requester_id, workspace_id, admin_email, message) VALUES (:id, :now, :now, :user, :user, :ws, :email, :message) ON CONFLICT DO NOTHING".to_owned()
}

/// R16 unresolved guard (`:137-142`): `requester=:user AND
/// workspace IS NULL AND admin_email=:email AND status=PENDING`.
/// Ported bug 15: the partial unique does NOT cover `NULL` workspaces
/// (Postgres treats NULLs as distinct), so this `exists()` is the ONLY
/// guard — concurrent duplicates are possible and de-duped on read.
pub fn join_request_unresolved_pending_exists_sql() -> String {
    "SELECT EXISTS(SELECT 1 FROM workspace_join_requests jr WHERE jr.requester_id = :user AND jr.workspace_id IS NULL AND jr.admin_email = :email AND jr.status = 'PENDING' AND jr.deleted_at IS NULL)".to_owned()
}

/// R16 unresolved create (`:144-150`): same columns with `workspace_id
/// NULL`. The endpoint ALWAYS answers neutral 201 `{"message": "Request
/// sent"}` (`:152-153`, anti-enumeration) whether the email resolved or not.
pub fn join_request_unresolved_create_sql() -> String {
    "INSERT INTO workspace_join_requests (id, created_at, updated_at, created_by_id, requester_id, workspace_id, admin_email, message) VALUES (:id, :now, :now, :user, :user, NULL, :email, :message)".to_owned()
}

// ---------------------------------------------------------------------------
// R17 admin join-request list (join_request.py:168-177)
// ---------------------------------------------------------------------------

/// R17 scope (`:172-176`): `workspace__slug=:slug AND status=PENDING` with
/// `select_related("workspace", "requester")` fetch joins, ordered by
/// [`JOIN_REQUEST_LIST_ORDER_SQL`]. Non-pending rows are invisible here,
/// so approve/deny fetch via direct `get_object_or_404`
/// ([`join_request_fetch_where`]) — ported as observed.
pub fn admin_join_requests_where() -> String {
    format!(
        "workspace_join_requests.workspace_id = {} AND workspace_join_requests.status = 'PENDING' AND workspace_join_requests.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

// ---------------------------------------------------------------------------
// R18 approve / R19 deny (join_request.py:187-255)
// ---------------------------------------------------------------------------

/// R18/R19 fetch (`:188`, `:242`): `get_object_or_404(pk=:pk,
/// workspace__slug=:slug)` — a direct get with NO status filter (contrast
/// [`admin_join_requests_where`]) that 404s on missing (contrast R12's
/// bare `.get`, whose `DoesNotExist` renders the base-view 404 `{"error":
/// "The required object does not exist."}`).
pub fn join_request_fetch_where() -> String {
    format!(
        "workspace_join_requests.id = :pk AND workspace_join_requests.workspace_id = {} AND workspace_join_requests.deleted_at IS NULL",
        workspace_id_by_slug_sql()
    )
}

/// R18/R19 responded guard (`:190`, `:244`): non-`PENDING` answers 400
/// `"This request has already been responded to"` (handler body).
pub fn join_request_pending_blocks(status: &str) -> bool {
    status != JOIN_REQUEST_PENDING
}

/// R18 member create (`:210-215`): `WorkspaceMember(workspace, requester,
/// role=request.role, created_by=request.user)` — explicit `created_by`
/// (contrast R12's [`member_join_insert_sql`]). The reactivate arm reuses
/// [`member_reactivate_lookup_where`] + [`member_reactivate_sql`] with
/// `:role` bound to the request's role (default [`JOIN_REQUEST_DEFAULT_ROLE`]).
pub fn approve_member_insert_sql() -> String {
    "INSERT INTO workspace_members (id, created_at, updated_at, created_by_id, workspace_id, member_id, role) VALUES (:id, :now, :now, :user, :ws, :requester, :role)".to_owned()
}

/// R18 profile pointer (`:219`): `Profile.objects.filter(user=:requester)
/// .update(last_workspace_id=:ws)`. Ported bug 11 (second half): this
/// `profiles` UPDATE is the path that PERSISTS (contrast R12's
/// [`user_touch_sql`] no-op). Zero matching profiles updates nothing —
/// no error.
pub fn profile_last_workspace_sql() -> String {
    "UPDATE profiles SET last_workspace_id = :ws WHERE user_id = :requester".to_owned()
}

/// R18/R19 status transition (`:221-224` + `:250-253`): `status=:status,
/// responded_at=:now, responded_by=:user` (`:status` binds `APPROVED` /
/// `DENIED`). R18 runs inside `transaction.atomic` (`:202`, ported bug 16's
/// first half); R19 is this single save with NO transaction.
pub fn join_request_status_sql() -> String {
    "UPDATE workspace_join_requests SET status = :status, responded_at = :now, responded_by_id = :user, updated_at = :now WHERE id = :pk".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/membership.sql"
    );
    const FIXTURE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/membership.rows.json"
    );

    fn rows_fixture() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_ROWS).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn sql_fixture() -> String {
        std::fs::read_to_string(FIXTURE_SQL).expect("fixture exists")
    }

    #[test]
    fn every_soft_delete_read_carries_deleted_at_scope() {
        // Issue rule: soft-delete scoping on every read. users/profiles
        // carry no deleted_at (not BaseModels); every other read does.
        for sql in [
            member_scope_where(),
            member_requester_where(),
            member_retrieve_target_where(),
            member_write_target_where(),
            guest_demote_cascade_sql(),
            sole_project_admin_exists_sql(":wm_pk"),
            project_deactivate_sql(),
            sole_workspace_admin_count_sql(),
            me_lookup_where(),
            draft_count_annotation_sql(),
            involved_project_ids_sql(),
            project_members_scope_where(),
            invite_scope_where(),
            workspace_lookup_where(),
            invite_already_member_where(),
            invite_lookup_where(),
            member_reactivate_lookup_where(),
            my_invites_where(),
            my_invites_accept_lookup_sql(),
            my_invites_bulk_soft_delete_sql(),
            my_invite_reactivate_sql(),
            own_join_requests_where(),
            join_request_admin_targets_sql(),
            join_request_already_member_sql(),
            join_request_shortcircuit_slug_sql(),
            join_request_pending_exists_sql(),
            join_request_unresolved_pending_exists_sql(),
            admin_join_requests_where(),
            join_request_fetch_where(),
        ] {
            assert!(
                sql.contains("deleted_at IS NULL"),
                "missing soft-delete scope: {sql}"
            );
        }
        // INSERTs are writes, not reads: their dedupe scope is the partial
        // unique (which excludes deleted rows itself), not a WHERE guard —
        // so invite_bulk_insert_sql / my_invite_bulk_insert_sql stay out of
        // the list above.
        // users/profiles have no deleted_at column — assert absence, not scope.
        assert!(!join_user_lookup_sql().contains("deleted_at"));
        assert!(!profile_last_workspace_sql().contains("deleted_at"));
        assert!(!user_touch_sql().contains("deleted_at"));
    }

    #[test]
    fn r1_scope_lists_inactive_rows() {
        // Ported bug 1: NO is_active filter.
        let scope = member_scope_where();
        assert!(scope.contains("slug = :slug"));
        assert!(scope.contains("workspace_members.deleted_at IS NULL"));
        assert!(!scope.contains("is_active"));
        // Search: icontains OR across display_name / first_name.
        let search = member_search_where();
        assert!(search.contains("users.display_name ILIKE :search"));
        assert!(search.contains("users.first_name ILIKE :search"));
        // Representative SELECT: member join + LEFT avatar join + ordering.
        let sql = member_list_sql();
        assert!(sql.contains("JOIN users ON users.id = workspace_members.member_id"));
        assert!(sql.contains("LEFT JOIN file_assets ON file_assets.id = users.avatar_asset_id"));
        assert!(sql.ends_with(&format!("ORDER BY {MEMBER_LIST_ORDER_SQL}")));
        assert_eq!(MEMBER_LIST_ORDER_SQL, "workspace_members.created_at DESC");
    }

    #[test]
    fn r2_r3_requester_and_role_branch() {
        // Source order (:47): member, slug, is_active.
        let requester = member_requester_where();
        let member = requester.find("member_id = :user").expect("member");
        let slug = requester.find("slug = :slug").expect("slug");
        let active = requester.find("is_active = TRUE").expect("active");
        assert!(member < slug && slug < active);
        let target = member_retrieve_target_where();
        assert!(target.contains(&member_scope_where()));
        assert!(target.contains("workspace_members.id = :pk"));
        // Ported bug 2: same threshold, two spellings.
        assert_eq!(admin_branch_literal_sql(), "role > 5");
        assert_eq!(admin_branch_enum_sql(), "role > ROLE.GUEST.value");
        assert!(!is_admin_shape(ROLE_GUEST));
        assert!(is_admin_shape(ROLE_GUEST + 1));
        assert!(is_admin_shape(ROLE_ADMIN));
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(ROLE_MEMBER, 15);
        assert_eq!(ROLE_GUEST, 5);
        assert_eq!(MEMBER_LIST_FIELDS, &["id", "member", "role"]);
    }

    #[test]
    fn r4_demote_cascade_rewrites_inactive_rows() {
        let target = member_write_target_where();
        for needle in [
            "workspace_members.id = :pk",
            "slug = :slug",
            "users.is_bot = FALSE",
            "workspace_members.is_active = TRUE",
        ] {
            assert!(target.contains(needle), "missing {needle}");
        }
        // Ported bug 3: NO is_active filter on the cascade.
        assert_eq!(GUEST_DEMOTE_ROLE, 5);
        let cascade = guest_demote_cascade_sql();
        assert!(cascade.contains("UPDATE project_members SET role = 5"));
        assert!(cascade.contains("project_members.member_id = :target"));
        assert!(!cascade.contains("is_active"));
    }

    #[test]
    fn r5_dead_guard_vs_r6_live_guard() {
        // Ported bug 4: R5 binds the WorkspaceMember PK (never matches);
        // R6 binds the user id (works). Same statement otherwise.
        let dead = sole_project_admin_exists_sql(":wm_pk");
        let live = sole_project_admin_exists_sql(":user");
        assert!(dead.contains("pm.member_id = :wm_pk"));
        assert!(live.contains("pm.member_id = :user"));
        for sql in [&dead, &live] {
            assert!(sql.starts_with("SELECT EXISTS("));
            assert!(sql.contains("HAVING COUNT(pm.id) = 1"));
            assert!(sql.contains("pm.role = 20 THEN 1 END) = 1"));
            assert!(sql.contains("GROUP BY p.id"));
        }
        // Ported bug 5: strict less — equal roles CAN remove.
        assert!(can_remove(20, 20));
        assert!(can_remove(15, 5));
        assert!(!can_remove(5, 15));
        // Deactivations answer 204.
        let projects = project_deactivate_sql();
        assert!(projects.contains("SET is_active = FALSE, updated_at = :now"));
        assert!(projects.contains("project_members.is_active = TRUE"));
        let member = member_deactivate_sql();
        assert!(member.contains("UPDATE workspace_members SET is_active = FALSE"));
        assert!(member.contains("WHERE id = :pk"));
    }

    #[test]
    fn r6_leave_guard_keeps_not_count_spelling() {
        let count = sole_workspace_admin_count_sql();
        assert!(count.contains("SELECT COUNT(*)"));
        assert!(count.contains("workspace_members.role = 20"));
        assert!(count.contains("workspace_members.is_active = TRUE"));
        // Ported bug 6: `role == 20 and not count > 1`.
        assert!(sole_workspace_admin_blocks(ROLE_ADMIN, 1));
        assert!(sole_workspace_admin_blocks(ROLE_ADMIN, 0));
        assert!(!sole_workspace_admin_blocks(ROLE_ADMIN, 2));
        assert!(!sole_workspace_admin_blocks(ROLE_MEMBER, 1));
    }

    #[test]
    fn r7_me_coalesce_covers_group_by_emptiness() {
        let annotation = draft_count_annotation_sql();
        assert!(annotation.starts_with("COALESCE((SELECT COUNT(d.id)"));
        assert!(annotation.contains("d.created_by_id = :user"));
        assert!(annotation.contains("d.workspace_id = workspace_members.workspace_id"));
        assert!(annotation.contains("GROUP BY d.workspace_id"));
        assert!(annotation.ends_with(", 0) AS draft_issue_count"));
        let sql = me_sql();
        assert!(sql.contains(&annotation));
        assert!(sql.ends_with("ORDER BY workspace_members.created_at DESC LIMIT 1"));
    }

    #[test]
    fn r8_project_ids_span_all_workspaces() {
        // Ported bug 7: Q1 has NO slug filter.
        let q1 = involved_project_ids_sql();
        assert!(q1.contains("SELECT DISTINCT pm.project_id"));
        assert!(!q1.contains("slug"));
        let q2 = project_members_scope_where();
        assert!(q2.contains("slug = :slug"));
        assert!(q2.contains("project_members.project_id IN (:project_ids)"));
    }

    #[test]
    fn r9_invite_scope_and_joins() {
        let scope = invite_scope_where();
        assert!(scope.contains("slug = :slug"));
        let sql = invite_list_sql();
        assert!(sql.contains("JOIN users owners ON owners.id = workspaces.owner_id"));
        assert!(sql.contains(
            "LEFT JOIN users creators ON creators.id = workspace_member_invites.created_by_id"
        ));
        assert!(sql.ends_with(&format!("ORDER BY {INVITE_LIST_ORDER_SQL}")));
    }

    #[test]
    fn r10_invite_create_caps_and_skips() {
        // Equal roles OK.
        assert!(!invite_role_cap_blocks(5, 5));
        assert!(!invite_role_cap_blocks(15, 20));
        assert!(invite_role_cap_blocks(20, 15));
        // Direct workspace get carries the manager scope.
        assert!(workspace_lookup_where().contains("workspaces.deleted_at IS NULL"));
        // Full-fetch truthiness, NOT exists().
        let already = invite_already_member_where();
        assert!(already.contains("users.email IN (:emails)"));
        assert!(!already.contains("EXISTS"));
        // Bulk: batch 10 + silent skip.
        assert_eq!(INVITE_BULK_BATCH_SIZE, 10);
        assert!(invite_bulk_insert_sql().ends_with("ON CONFLICT DO NOTHING"));
        // Ported bug 8: the email claim is the whole dict.
        assert_eq!(INVITE_TOKEN_CLAIMS, &["email", "timestamp"]);
    }

    #[test]
    fn r11_invite_lookup_and_soft_delete() {
        let lookup = invite_lookup_where();
        assert!(lookup.contains("workspace_member_invites.id = :pk"));
        assert!(lookup.contains("slug = :slug"));
        // Soft, not hard: deleted_at stamp. Instance .delete() runs a full
        // save, so updated_at is stamped too (contrast the R14 bulk shape).
        let delete = invite_soft_delete_sql();
        assert!(delete.contains("SET deleted_at = :now, updated_at = :now"));
        assert!(delete.contains("WHERE id = :pk"));
        assert!(!delete.to_uppercase().starts_with("DELETE FROM"));
    }

    #[test]
    fn r12_join_reactivate_or_create() {
        // Token: empty denied before comparison.
        assert!(join_token_denied("", "tok"));
        assert!(join_token_denied("bad", "tok"));
        assert!(!join_token_denied("tok", "tok"));
        // Respond-save: accepted + responded_at + full-save updated_at.
        let respond = invite_respond_sql();
        assert!(
            respond.contains("SET accepted = :accepted, responded_at = :now, updated_at = :now")
        );
        assert!(respond.contains("WHERE id = :pk"));
        // User lookup: exact email, default order, no deleted_at.
        let user = join_user_lookup_sql();
        assert!(user.contains("users.email = :email"));
        assert!(user.ends_with("ORDER BY users.created_at DESC LIMIT 1"));
        // Probe finds deactivated rows (no is_active).
        let probe = member_reactivate_lookup_where();
        assert!(probe.contains("workspace_members.workspace_id = :ws"));
        assert!(!probe.contains("is_active"));
        assert!(member_reactivate_sql().contains("SET is_active = TRUE, role = :role"));
        // Ported bug 12 (first half): no created_by on the join insert.
        assert!(!member_join_insert_sql().contains("created_by"));
        // Ported bug 11 (first half): users UPDATE touches audit only.
        assert_eq!(
            user_touch_sql(),
            "UPDATE users SET updated_at = :now WHERE id = :user"
        );
        // R13 reuses the R11 lookup.
        assert_eq!(join_detail_where(), invite_lookup_where());
    }

    #[test]
    fn r14_my_invitations_double_write() {
        assert!(my_invites_where().contains("workspace_member_invites.email = :email"));
        assert_eq!(
            MY_INVITES_ORDER_SQL,
            "workspace_member_invites.created_at DESC"
        );
        let lookup = my_invites_accept_lookup_sql();
        assert!(lookup.contains("workspace_member_invites.id IN (:pks)"));
        assert!(lookup.contains("workspace_member_invites.email = :email"));
        assert!(lookup.ends_with(&format!("ORDER BY {MY_INVITES_ORDER_SQL}")));
        // .update() writes only named columns: no updated_at (like R18).
        let reactivate = my_invite_reactivate_sql();
        assert!(reactivate.contains("SET is_active = TRUE, role = :role"));
        assert!(!reactivate.contains("updated_at"));
        // Queryset .delete() = SET deleted_at only, over the accept predicate.
        let bulk_delete = my_invites_bulk_soft_delete_sql();
        assert!(bulk_delete.contains("SET deleted_at = :now WHERE"));
        assert!(!bulk_delete.contains("updated_at"));
        assert!(bulk_delete.contains("workspace_member_invites.id IN (:pks)"));
        assert!(bulk_delete.contains(&my_invites_where()));
        // Ported bug 12 (second half): bulk sets created_by; update ran first.
        let bulk = my_invite_bulk_insert_sql();
        assert!(bulk.contains("created_by_id"));
        assert!(bulk.ends_with("ON CONFLICT DO NOTHING"));
    }

    #[test]
    fn views_post_view_props_save() {
        // Full save: view_props + updated_at on the member row.
        let save = member_view_props_sql();
        assert!(
            save.contains("UPDATE workspace_members SET view_props = :props, updated_at = :now")
        );
        assert!(save.contains("WHERE id = :pk"));
    }

    #[test]
    fn r15_own_requests_scope() {
        let scope = own_join_requests_where();
        assert!(scope.contains("workspace_join_requests.requester_id = :user"));
        assert_eq!(
            JOIN_REQUEST_LIST_ORDER_SQL,
            "workspace_join_requests.created_at DESC"
        );
    }

    #[test]
    fn r16_targets_union_and_idempotency() {
        let targets = join_request_admin_targets_sql();
        assert!(targets.contains("wm.role = 20"));
        assert!(targets.contains("UNION"));
        assert!(
            targets.contains("w.owner_id IN (SELECT u2.id FROM users u2 WHERE u2.email = :email)")
        );
        assert_eq!(ADMIN_ROLE, 20);
        assert!(join_request_already_member_sql().contains("workspace_id IN (:targets)"));
        // Earliest-created short-circuit: ASC against the default DESC.
        let slug = join_request_shortcircuit_slug_sql();
        assert!(slug.contains("ORDER BY w.created_at ASC LIMIT 1"));
        // Pending guard + idempotent create.
        let pending = join_request_pending_exists_sql();
        assert!(pending.contains("jr.status = 'PENDING'"));
        assert!(pending.contains("jr.workspace_id = :ws"));
        let create = join_request_create_sql();
        assert!(create.contains("admin_email"));
        assert!(create.ends_with("ON CONFLICT DO NOTHING"));
        assert_eq!(JOIN_REQUEST_DEFAULT_ROLE, 15);
        // Ported bug 15: unresolved branch escapes the partial unique.
        let unresolved_guard = join_request_unresolved_pending_exists_sql();
        assert!(unresolved_guard.contains("jr.workspace_id IS NULL"));
        assert!(unresolved_guard.contains("jr.admin_email = :email"));
        assert!(join_request_unresolved_create_sql().contains("NULL, :email"));
    }

    #[test]
    fn r17_admin_scope_hides_non_pending() {
        let scope = admin_join_requests_where();
        assert!(scope.contains("slug = :slug"));
        assert!(scope.contains("workspace_join_requests.status = 'PENDING'"));
        assert_eq!(
            JOIN_REQUEST_LIST_ORDER_SQL,
            "workspace_join_requests.created_at DESC"
        );
    }

    #[test]
    fn r18_approve_txn_vs_r19_deny_single_save() {
        // Direct fetch: NO status filter (contrast R17).
        let fetch = join_request_fetch_where();
        assert!(fetch.contains("workspace_join_requests.id = :pk"));
        assert!(fetch.contains("slug = :slug"));
        assert!(!fetch.contains("PENDING"));
        assert!(join_request_pending_blocks(JOIN_REQUEST_APPROVED));
        assert!(join_request_pending_blocks(JOIN_REQUEST_DENIED));
        assert!(!join_request_pending_blocks(JOIN_REQUEST_PENDING));
        assert_eq!(JOIN_REQUEST_PENDING, "PENDING");
        assert_eq!(JOIN_REQUEST_APPROVED, "APPROVED");
        assert_eq!(JOIN_REQUEST_DENIED, "DENIED");
        // Approve create sets created_by (contrast R12).
        assert!(approve_member_insert_sql().contains("created_by_id"));
        // Ported bug 11 (second half): the Profile UPDATE persists.
        assert_eq!(
            profile_last_workspace_sql(),
            "UPDATE profiles SET last_workspace_id = :ws WHERE user_id = :requester"
        );
        // Shared status transition; atomicity differs (ported bug 16).
        let status = join_request_status_sql();
        assert!(status.contains("status = :status"));
        assert!(status.contains("responded_at = :now"));
        assert!(status.contains("responded_by_id = :user"));
    }

    #[test]
    fn fixture_sql_names_every_unit() {
        // The R1-R19 markers in membership.sql are the port's checklist.
        let sql = sql_fixture();
        for marker in [
            "R1 member get_queryset",
            "R2 list",
            "R3 retrieve",
            "R4 partial_update",
            "R5 destroy",
            "R6 leave",
            "R7 me",
            "R8 project-members",
            "R9 invite list",
            "R10 invite create",
            "R11 invite destroy",
            "R12 join post",
            "R13 join get",
            "R14 my-invitations",
            "R15 join-request user get_queryset",
            "R16 join-request create",
            "R17 admin get_queryset",
            "R18 approve txn",
            "R19 deny",
        ] {
            assert!(sql.contains(marker), "fixture missing {marker}");
        }
    }

    #[test]
    fn fixture_rows_match_builders() {
        let fixture = rows_fixture();
        assert_eq!(
            fixture["source"],
            "app/views/workspace/member.py:30-265;invite.py:37-305;join_request.py:32-255"
        );
        let rows = fixture["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 7);
        // R2 admin-shape row vs guest member-shape branch.
        let admin = rows.iter().find(|r| r["role"] == 15).expect("admin row");
        assert!(admin["note"].as_str().unwrap_or("").contains("role>5"));
        assert!(is_admin_shape(admin["role"].as_i64().unwrap() as i32));
        assert!(!is_admin_shape(ROLE_GUEST));
        // R7 Coalesce count row.
        let me = rows
            .iter()
            .find(|r| r.get("draft_issue_count").is_some())
            .expect("me row");
        assert_eq!(me["draft_issue_count"], 2);
        assert!(me["note"].as_str().unwrap_or("").contains("Coalesce"));
        // R9 invite row.
        let invite = rows
            .iter()
            .find(|r| r.get("email").is_some())
            .expect("invite row");
        assert_eq!(invite["workspace__slug"], "acme");
        // R12 accepted join.
        let joined = rows
            .iter()
            .find(|r| r.get("accepted") == Some(&serde_json::Value::Bool(true)))
            .expect("joined row");
        assert!(joined["note"]
            .as_str()
            .unwrap_or("")
            .contains("invite deleted"));
        // R16 resolved vs unresolved.
        let resolved = rows
            .iter()
            .find(|r| r.get("requester").is_some())
            .expect("resolved row");
        assert_eq!(resolved["status"], "PENDING");
        let unresolved = rows
            .iter()
            .find(|r| r.get("workspace").is_some_and(|w| w.is_null()))
            .expect("unresolved row");
        assert_eq!(unresolved["status"], "PENDING");
        // R18 approved.
        let approved = rows
            .iter()
            .find(|r| r["status"] == "APPROVED")
            .expect("approved row");
        assert!(approved["note"]
            .as_str()
            .unwrap_or("")
            .contains("Profile.last_workspace"));
        // All 15 ported bugs recorded.
        let bugs = fixture["bugs"].as_array().expect("bugs array");
        assert_eq!(bugs.len(), 15);
        for needle in [
            "NO is_active filter",
            "ROLE.GUEST.value",
            "member_id to WorkspaceMember PK",
            "strict <",
            "no slug filter",
            "whole email dict",
            "ignore_conflicts",
            "already responded",
            "User (:204) vs Profile (:219)",
            "no created_by",
            "AllowAny",
            "self-join",
            "partial unique",
            "no transaction",
        ] {
            assert!(
                bugs.iter()
                    .any(|b| b.as_str().unwrap_or("").contains(needle)),
                "bug note missing: {needle}"
            );
        }
        // All 5 read exclusions recorded with source refs.
        let excluded = fixture["rows_excluded"].as_array().expect("excluded array");
        assert_eq!(excluded.len(), 5);
        for needle in [":250", ":174", ":105", ":78-100", ":79"] {
            assert!(
                excluded
                    .iter()
                    .any(|r| r["why"].as_str().unwrap_or("").contains(needle)),
                "exclusion missing: {needle}"
            );
        }
    }
}
