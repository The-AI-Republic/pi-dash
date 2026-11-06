#![forbid(unsafe_code)]

//! Machines + runners read/write statement sets (queries-B, PIDASHCONV-583).
//!
//! Port of the five statement sets in `runner/views/runners.py` (+
//! `_scoped_machine` in `runner/views/machine_commands.py:63-73`):
//!
//! 1. Dev-machine list + serialize (`runners.py:73-125, 150-170`):
//!    runner/token machine-id subselects (M2), `runner_count` /
//!    `online_runner_count` / `last_heartbeat_at` annotations (M3/M5).
//! 2. Runner list + visibility predicate (`:289-333`): workspace filter,
//!    `runner_visible_to_user_q` as SQL, pod/project filters, bundled
//!    exclusion + `include_bundled` flag (R1).
//! 3. Runner detail reads + patch + busy-guard (`:336-425`): `_get_runner`
//!    tri-state (R2), rename validation, pod-move lock + workspace check +
//!    `NON_TERMINAL_STATUSES` busy-guard (R3).
//! 4. Machine-command scope reads + presence (`machine_commands.py:63-73`;
//!    `runners.py:49-59, 128-147`): `_scoped_machine`,
//!    `_machine_is_in_workspace_scope` probes (M4),
//!    `_request_workspace_id`, `_control_online_subquery` (M1).
//! 5. Machine revoke/rotate write sets (`:189-246`): machine lock + scope
//!    check, token revoke updates, runner select-for-update, the
//!    rotate-after-revoke 409 guard (M6/M7).
//!
//! Fixture record: `rust-api/fixtures/runner_enroll/queries/`
//! `machines_runners.sql` (M1-M7, R1-R4) + `machines_runners.rows.json`
//! with example rows, `models/columns.json` (D13-F1) for the column
//! lists, `external/wire_pins.json` (D13-F8) for the
//! `NON_TERMINAL_STATUSES` value (filed by PIDASHCONV-578). The fixture
//! SQL is hand-composed and abbreviates several Django renderings
//! (see "Fixture deviations" below); every builder below was instead
//! verified against SQL compiled from the real querysets on the
//! repo-pinned Django 4.2.30 (plus sqlite execution capture for the
//! `save()`/`update()`/`exists()` shapes), and emits that exact text
//! with `%s` replaced by `$N`. The `#[cfg(test)]` suite pins the
//! recorded fixture fragments inside the builder output (the
//! `assert_builder_contains` direction from the D-02 `space/queries/`
//! precedent) AND the full compiled text.
//!
//! Conventions (same as [`super::enroll_reads`]):
//!
//! * Builders return owned SQL text; `$N` params are documented in
//!   first-appearance (binding) order. Handlers bind them positionally.
//! * `WHERE` conjunct order is Django's: manager-scope conjuncts first,
//!   then the filter kwargs in `Q`-sorted (alphabetical) order, then one
//!   chained `.filter()`/`.exclude()` call's conjuncts after another in
//!   call order. `$N` numbering follows that order, NOT the Python call
//!   order (e.g. M4a binds `dev_machine_id=$1` although the source
//!   passes `workspace_id` first).
//! * `save(update_fields=[...])` emits `SET` in Django *model-field*
//!   order, not in `update_fields` list order (`Model.save_base`,
//!   `django/db/models/base.py`). The `*_UPDATE_FIELDS` consts pin the
//!   verbatim Python lists; the builders emit the verified model order.
//! * `.exists()` compiles to `SELECT $1 AS "a" …` — the constant `1`
//!   (`Value(1)`) is the FIRST bind param, the `WHERE` binds follow
//!   (`Query.exists().sql_with_params()` on Django 4.2.30; sqlite
//!   execution capture interpolates params and hides this, so only a
//!   compiled probe shows it).
//! * `select_related` forward-FK joins use the real table names (no
//!   `U0` aliases); the joined tables carry NO manager scope (the pod
//!   join in R1/R2 has no `deleted_at` guard).
//! * Timestamps and UUIDs cross this boundary as bind params (the M1
//!   cutoff and every `now()` are computed by the caller); the SQL only
//!   names the `$N` slots.
//!
//! Out of scope here (owned elsewhere, referenced so handlers can find them):
//!
//! * `is_workspace_member` — kernel membership fact (`pidash_auth`,
//!   consumed by handlers, never re-ported).
//! * `can_view_dev_machine` / `can_view_runner` / `can_manage_runner`
//!   — kernel-blessed in `pidash_auth::permissions::runner`: all three
//!   reduce to "private and owned by the requester"; anonymous matches
//!   nothing. Pure-Python in the source, pure-Rust in the kernel; the
//!   SQL here carries only the `owner_id`/`visibility` conjuncts and
//!   the [`DetailOutcome`] mapping.
//! * `send_runner_revoke` / `close_runner_session` / `Runner.revoke`
//!   — services-C (PIDASHCONV-588) + handlers-C (PIDASHCONV-592); this
//!   unit is the SQL only. The per-runner/per-id loops stay in the
//!   handlers (M6 revoke cascade, M7 session closes).
//! * R4 runner-revoke locked read (`runners.py:476`) — handlers-C names
//!   it in its own unit ("row-locked read", PIDASHCONV-592); it is not
//!   one of this issue's five units.
//! * Machine-delete / runner-delete flows — handlers-D (PIDASHCONV-593)
//!   reads through [`machine_gate`] + the M4 probe functions (SeaORM).
//! * Response bodies (`workspace is required`, `not found`, …) —
//!   handlers-B/C/D own every status + body; the guard→error mapping
//!   is documented on each builder but no body const lives here.
//! * `live_state` — NOT prefetched (N+1 per row on serialize); the R1
//!   SQL carries no live-state join by design.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * QUIRK-subquery-ordering-cleared (M2): standalone, the id probes
//!   select `DISTINCT dev_machine_id` PLUS the `Meta.ordering` columns
//!   with `ORDER BY`; nested as `IN` subqueries Django drops the
//!   ordering columns and the `ORDER BY`. Only the nested form ships.
//! * QUIRK-join-order (R1): the three `select_related` joins reorder
//!   by filter combo — `?project=` runs `pod, projects, dev_machine`;
//!   `?pod=` without `?project=` runs `pod, dev_machine, projects`;
//!   otherwise `dev_machine, pod, projects` (the bundled exclusion
//!   never reorders). Fresh-queryset compiles only: recompiling a
//!   queryset MUTATES its join order, so the ground-truth probe builds
//!   each combo fresh. The select-list order never moves (runner,
//!   dev_machine, pod, projects). Verified deterministic across
//!   `PYTHONHASHSEED` 0/1/42/123/999.
//! * QUIRK-set-ordered-update-fields (R3): the patch save passes
//!   `list(set(updates + ["updated_at"]))` — nondeterministic Python
//!   order — but Django emits `SET` in model order regardless, so all
//!   three variants are pinned exactly.
//! * QUIRK-workspace-or-chain (`_request_workspace_id`): a
//!   whitespace-only `data["workspace"]` is TRUTHY, wins over the
//!   query param, then strips to `""` → 400. Ported as-is
//!   ([`request_workspace_id`]).
//! * QUIRK-rotate-keeps-runners (M7): rotate revokes tokens and closes
//!   sessions but never touches the runners' `status`/`revoked_at`
//!   columns — ported as-is (no runner `UPDATE` here).
//! * QUIRK-conditional-machine-write (M6): the machine `UPDATE` runs
//!   only when `revoked_at` was NULL; an already-revoked machine
//!   still revokes tokens, still cascades runners, still 200s.
//! * QUIRK-m5-fallback: when the serialize re-read finds nothing, the
//!   serializer falls back to the UNANNOTATED instance, so the
//!   annotation keys are omitted (F2 shapes) — handlers own the
//!   fallback, the SQL is just the M5 read.
//!
//! # Fixture deviations (fixture abbreviates; builders are Django-exact)
//!
//! * M1 spells `ms.dev_machine_id = dev_machine.id` with an inline
//!   `now() - interval`; Django binds the `SELECT 1` constant as `$N`,
//!   renders the `OuterRef` in parens, aliases the table `U0`, takes
//!   the cutoff (computed once per request) as a bind, and appends
//!   `LIMIT 1` + `AS "a"`.
//! * M2 spells two round-trips; Django nests both probes as
//!   `IN`-subqueries with no `ORDER BY` (QUIRK above).
//! * M3 spells `GROUP BY` all machine columns and `dm`/`r` aliases;
//!   Django groups by the PK only and uses real table names (`U0`
//!   only inside the subqueries). `FILTER` conjunct order is
//!   `owner_id, visibility, workspace_id` (Q-sorted).
//! * M4 spells `SELECT EXISTS(SELECT 1 …)`; Django's `.exists()`
//!   emits `SELECT $1 AS "a" … LIMIT 1` (the `1` bound, ordering
//!   cleared). Sibling note: queries-A's E4 probe spells the `1`
//!   literal — sqlite capture hides the bind; this module follows the
//!   compiled probe (bound `$1`) per the exact-text convention.
//! * M6-lock / M6-locked-list / M7-ids / scoped-read spell no
//!   `ORDER BY`; Django keeps `Meta.ordering` on every one
//!   (`.first()`/list evaluation never clears it).
//! * R1 spells select order `r, p, pr, dm` with the pod join first and
//!   `provisioning <> 'desktop_bundled'`; Django selects
//!   runner/dev_machine/pod/projects, orders joins per QUIRK-join-order,
//!   and renders `.exclude()` as `NOT ("runner"."provisioning" = $N)`.
//!   `$N` binds run `workspace, owner, visibility` then the filters in
//!   call order (`pod`, bundled, `project`).
//! * R2 spells no `ORDER BY`/`LIMIT`; Django keeps `Meta.ordering`
//!   and `.first()` adds `LIMIT 1`.
//! * R3-podlock spells a bare `WHERE id` + lock; Django leads with the
//!   `PodManager` scope (`deleted_at IS NULL`), keeps `Meta.ordering`
//!   (`is_default DESC, created_at ASC`), then `LIMIT 1 FOR UPDATE`.
//! * R3-busy spells `SELECT EXISTS` with one runner bind; Django
//!   emits `SELECT $1 AS "a"` (constant bound), binds the runner id
//!   TWICE (`$2`/`$3`, one per `OR` leg), and the eight statuses as
//!   `$4`-`$11` in `NON_TERMINAL_STATUSES` tuple order.
//! * R3-write spells `updated_at = now()`; Django binds the
//!   `auto_now` `pre_save` value as `$N` like every other `SET`.
//!
//! Django-idiom → Rust-pattern rows applied: `$N` bind builders +
//! `*_UPDATE_FIELDS` consts (`space/queries/issue_retrieve.rs`,
//! `enroll_reads.rs`); char-boundary-safe semantics where slicing
//! applies (Porting guide semantic traps — no truncation in this unit,
//! only `strip()` parity in [`request_workspace_id`]).

use pidash_compat::py::py_strip;
use pidash_db::entity::{
    agent_run, dev_machine, machine_session, machine_token, pod, projects, runner,
    runner_live_state, workspace_members, workspaces,
};
use pidash_db::runner_enroll::columns::{dev_machine as dm_cols, pod as pod_cols};
use sea_orm::sea_query::{Expr, Func, Query};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, JoinType,
    QueryFilter, QueryOrder, QuerySelect, RelationDef, RelationTrait, Select,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Pinned cross-domain names (D-14/D-15 own these tables; constant only)
// ---------------------------------------------------------------------------

/// `machine_session` table (`runner/models.py:723-762`, D-14-owned).
/// D-13 pins only the name + the three `WHERE` columns M1 needs
/// (F5 `machines_runners.sql` M1); the owning domain ports the model.
pub const MACHINE_SESSION_TABLE: &str = "machine_session";

/// `agent_run` table (`runner/models.py:871+`, D-15-owned). D-13 pins
/// only the name + the three `WHERE` columns the R3 busy-guard needs
/// (F5 R3).
pub const AGENT_RUN_TABLE: &str = "agent_run";

/// `NON_TERMINAL_STATUSES` value (`runner/services/matcher.py:54-66`,
/// pinned by D13-F8 `wire_pins.json#non_terminal_statuses`; D-12 owns
/// the set, D-13 pins the constant only — no D-14/D-12 code needed).
/// Tuple order is the `status__in` bind order (`$4`-`$11`).
pub const NON_TERMINAL_STATUSES: &[&str] = &[
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
    "paused_awaiting_input",
];

/// Control-presence window (`runners.py:49`,
/// `_CONTROL_PRESENCE_WINDOW = timedelta(seconds=90)`). The cutoff
/// (`now() - 90s`) is computed ONCE per request at annotation time and
/// crossed as the M1 bind — never recomputed per row.
pub const CONTROL_PRESENCE_WINDOW_SECS: i64 = 90;

/// Qualify every column (`"table"."col"`) and join with `", "`, exactly
/// how Django renders a full-row select list.
fn qualified(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|col| format!("\"{table}\".\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// M1. Control-online subquery
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// M2/M3. Machine list + annotations
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// M4. Scope probes
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// M5. Single-machine serialize
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// M6/M7. Machine revoke/rotate writes
// ---------------------------------------------------------------------------

/// M6 — locked machine read (`runners.py:190, 228`, shared by the
/// revoke and rotate endpoints).
///
/// `DevMachine.objects.select_for_update().filter(pk).first()` inside
/// `transaction.atomic()`: full row, `Meta.ordering`, `LIMIT 1 FOR
/// UPDATE`. `$1` = machine id.
///
/// Miss or out-of-scope (M4 probes + kernel `can_view_dev_machine`)
/// → 404 `not found` (tx rolls back).
pub fn machine_locked_read_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 \
         ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

/// M6 — the verbatim machine-revoke `update_fields` list
/// (`runners.py:197`). [`machine_revoke_sql`] emits these in Django
/// model-field order (which matches this list order here).
pub const MACHINE_REVOKE_UPDATE_FIELDS: &[&str] = &["revoked_at", "updated_at"];

/// M6 — machine revoke write (`runners.py:194-197`).
///
/// Runs ONLY when `revoked_at` was NULL (QUIRK-conditional-machine-write).
/// `machine.save(update_fields=["revoked_at", "updated_at"])`:
/// `$1` = the view's `now()`, `$2` = the `auto_now` `pre_save` value
/// (a fresh `timezone.now()` at save time, microseconds after `$1`),
/// `$3` = machine id.
pub fn machine_revoke_sql() -> String {
    "UPDATE \"dev_machine\" SET \"revoked_at\" = $1, \"updated_at\" = $2 \
     WHERE \"dev_machine\".\"id\" = $3"
        .to_string()
}

// NOTE: the M6/M7 machine-token revoke builder lived here; the statement
// moved to `crate::domain::tokens::revoke_tokens_for_dev_machine`
// (PIDASHCONV-1520), whose test carries the pin.

// ---------------------------------------------------------------------------
// R1. Runner list
// ---------------------------------------------------------------------------

/// R1 — the `include_bundled` flag (`runners.py:326`).
///
/// `request.query_params.get("include_bundled") in ("1", "true",
/// "yes")`: exact, case-sensitive string match — `"True"`/`"TRUE"`
/// do NOT match (F5 rows). `None` (absent) excludes bundled runners.
pub fn include_bundled(raw: Option<&str>) -> bool {
    matches!(raw, Some("1") | Some("true") | Some("yes"))
}

// ---------------------------------------------------------------------------
// R2. Runner detail read + _get_runner tri-state
// ---------------------------------------------------------------------------

/// `_get_runner` outcome (`runners.py:342-352`): `None` (missing row
/// OR row the caller may not view — existence is not leaked) → 404
/// `not found`; `False` (caller is no member of the runner's
/// workspace) → 403 `forbidden`; the row → proceed. PATCH adds the
/// kernel `can_manage_runner` (owner) check → 403.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailOutcome {
    /// Row present, member, viewable — serialize / patch it.
    Found,
    /// Missing row, or present but `can_view_runner` is false → 404.
    Missing,
    /// Present row in a workspace the caller is no member of → 403.
    Forbidden,
}

/// `_get_runner` decision order (`:346-352`): missing → [`DetailOutcome::Missing`];
/// present but `!is_member(runner.workspace_id)` → [`DetailOutcome::Forbidden`];
/// present but `!can_view` → [`DetailOutcome::Missing`]; else [`DetailOutcome::Found`].
pub fn detail_outcome(found: bool, is_member: bool, can_view: bool) -> DetailOutcome {
    if !found {
        return DetailOutcome::Missing;
    }
    if !is_member {
        return DetailOutcome::Forbidden;
    }
    if !can_view {
        return DetailOutcome::Missing;
    }
    DetailOutcome::Found
}

// ---------------------------------------------------------------------------
// R3. Pod-move lock + busy-guard + patch write
// ---------------------------------------------------------------------------

/// R3 — pod-move lock read (`runners.py:386`), inside the patch tx
/// (which opens ONLY when `"pod"` is in the body).
///
/// `Pod.objects.select_for_update().filter(pk).first()`: the
/// `PodManager` scope FIRST (`deleted_at IS NULL` — a soft-deleted
/// pod reads as missing), then the pk, `Meta.ordering`
/// (`is_default DESC, created_at ASC`), `LIMIT 1 FOR UPDATE`.
/// `$1` = the body's pod id.
///
/// Guards: miss → 400 `pod does not exist or has been deleted`;
/// `pod.workspace_id != runner.workspace_id` → 400 `pod is in a
/// different workspace`.
pub fn pod_locked_read_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC \
         LIMIT 1 FOR UPDATE",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

/// R3 — the real-move check (`runners.py:409`): only re-sending a
/// DIFFERENT pod id runs the busy-guard; re-sending the current pod
/// is a no-op (still saved, still 200).
pub fn is_real_move(new_pod_id: &Uuid, current_pod_id: &Uuid) -> bool {
    new_pod_id != current_pod_id
}

/// R3 — the patch body keys that become `UPDATE` columns
/// (`runners.py:371-381, 384-421`), in source order: `"name"` (rename,
/// via [`normalize_runner_name`]) and `"pod"` (move). `"updated_at"`
/// is always appended (`list(set(updates + ["updated_at"]))` —
/// QUIRK-set-ordered-update-fields); [`runner_patch_sql`] emits all
/// three variants in Django model order.
pub const RUNNER_PATCH_UPDATABLE: &[&str] = &["name", "pod"];

/// R3 — rename normalization (`runners.py:373`):
/// `(request.data.get("name") or "").strip()`. Empty result → 400
/// `name cannot be empty` (handlers own the body); else the name
/// rides the patch `UPDATE` (inside the pod tx when both keys are
/// present, standalone otherwise).
pub fn normalize_runner_name(raw: Option<&str>) -> String {
    py_strip(raw.unwrap_or("")).to_string()
}

// ---------------------------------------------------------------------------
// Unit 4. Scope helpers + _scoped_machine
// ---------------------------------------------------------------------------

/// `_request_workspace_id` (`runners.py:128-129`, shared by the
/// machine revoke/rotate/delete endpoints and the machine-command
/// endpoints): `(data.get("workspace") or
/// query_params.get("workspace") or "").strip()` — body first, then
/// query, Python `or` falsy semantics (QUIRK-workspace-or-chain),
/// then `strip()`. Empty result → 400 `workspace is required`.
pub fn request_workspace_id(data_workspace: Option<&str>, query_workspace: Option<&str>) -> String {
    let picked = data_workspace
        .filter(|s| !s.is_empty())
        .or_else(|| query_workspace.filter(|s| !s.is_empty()))
        .unwrap_or("");
    py_strip(picked).to_string()
}

// ---------------------------------------------------------------------------
// SeaORM gate reads (the delete / machine-command gates)
// ---------------------------------------------------------------------------

/// The runner facts the delete gate reads.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct RunnerGateRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub visibility: i16,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The [`runner_detail_sql`] read reduced to the gate columns: the
/// `select_related("pod__project")` INNER JOINs stay (they drop a runner
/// whose pod or project row is gone), as do `Meta.ordering` and `LIMIT 1`.
pub fn runner_gate_query(runner_id: Uuid) -> Select<runner::Entity> {
    // The entities omit runner -> pod (see entity/mod.rs).
    let to_pod: RelationDef = runner::Entity::belongs_to(pod::Entity)
        .from(runner::Column::PodId)
        .to(pod::Column::Id)
        .into();
    runner::Entity::find()
        .select_only()
        .columns([
            runner::Column::Id,
            runner::Column::OwnerId,
            runner::Column::WorkspaceId,
            runner::Column::Visibility,
            runner::Column::RevokedAt,
        ])
        .join(JoinType::InnerJoin, to_pod)
        .join(JoinType::InnerJoin, pod::Relation::Projects.def())
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
}

pub async fn runner_gate(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<Option<RunnerGateRow>, DbErr> {
    runner_gate_query(runner_id)
        .into_model::<RunnerGateRow>()
        .one(db)
        .await
}

/// The machine facts the scope gates read.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct MachineGateRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub visibility: i16,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `_scoped_machine`'s read (`machine_commands.py:63-73`,
/// `DevMachine.objects.filter(pk).first()`) reduced to the gate columns:
/// unlocked, `Meta.ordering`, `LIMIT 1`.
pub async fn machine_gate(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<MachineGateRow>, DbErr> {
    dev_machine::Entity::find()
        .select_only()
        .columns([
            dev_machine::Column::Id,
            dev_machine::Column::OwnerId,
            dev_machine::Column::Visibility,
            dev_machine::Column::RevokedAt,
        ])
        .filter(dev_machine::Column::Id.eq(machine_id))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
        .into_model::<MachineGateRow>()
        .one(db)
        .await
}

/// M4a ([`machine_scope_runner_probe_sql`]): a private runner of this
/// owner on the machine in the workspace exists.
pub async fn machine_scope_runner_exists(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    owner_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .filter(runner::Column::OwnerId.eq(owner_id))
        .filter(runner::Column::Visibility.eq(0i16))
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// M4b ([`machine_scope_token_probe_sql`]): a token of this user for the
/// machine in the workspace exists (revoked tokens count).
pub async fn machine_scope_token_exists(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = machine_token::Entity::find()
        .select_only()
        .column(machine_token::Column::Id)
        .filter(machine_token::Column::DevMachineId.eq(machine_id))
        .filter(machine_token::Column::UserId.eq(user_id))
        .filter(machine_token::Column::WorkspaceId.eq(workspace_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// `workspace_role` (`core/permissions.py:37-45`): the caller's active,
/// live membership role (`-created_at`, `.first()`).
pub async fn workspace_role(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i16>, DbErr> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Role)
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .order_by_desc(workspace_members::Column::CreatedAt)
        .into_tuple()
        .one(db)
        .await
}

/// `Workspace.objects.filter(pk=ws).first()` (`machine_commands.py:105`):
/// the live workspace's slug.
pub async fn workspace_slug(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
) -> Result<Option<String>, DbErr> {
    workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Slug)
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Id.eq(workspace_id))
        .order_by_desc(workspaces::Column::CreatedAt)
        .into_tuple()
        .one(db)
        .await
}

/// `Project.objects.filter(workspace_id=ws, identifier=id).exists()`
/// (`machine_commands.py:108`), manager scope applied.
pub async fn project_identifier_exists(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    identifier: &str,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .filter(projects::Column::DeletedAt.is_null())
        .filter(projects::Column::Identifier.eq(identifier))
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// SeaORM machine / runner reads (M1-M3, R1-R3)
// ---------------------------------------------------------------------------

/// One M3 list row: the serialized machine columns plus the four
/// annotations (#2910 engine state).
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct MachineListRow {
    pub id: Uuid,
    pub host_label: String,
    pub label: String,
    pub visibility: i16,
    pub provisioning: String,
    pub engine_paused_by_owner: bool,
    pub engine_auto_pause_reason: String,
    pub engine_team_runs: bool,
    pub engine_sign_in_state: String,
    pub engine_credential_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub engine_state: serde_json::Value,
    pub engine_state_reported_at: Option<chrono::DateTime<chrono::Utc>>,
    pub merged_into_id: Option<Uuid>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub runner_count: i64,
    pub online_runner_count: i64,
    pub last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    pub control_online: bool,
}

/// M3 (`runners.py:73-125`, [`machine_list_sql`]): the owner's private
/// machines with a private runner or a token in the workspace, annotated
/// over the joined runners, newest-seen first. Django's
/// `COUNT(...) FILTER (WHERE p)` is written `COUNT(CASE WHEN p THEN x END)`
/// (aggregates skip NULL, so the values are identical); M1
/// `control_online` is an `EXISTS` over the live sessions seen since
/// `cutoff`.
pub fn machine_list_query(
    owner_id: Uuid,
    workspace_id: Uuid,
    visibility: i16,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Select<dev_machine::Entity> {
    let runner_machines = Query::select()
        .distinct()
        .column(runner::Column::DevMachineId)
        .from(runner::Entity)
        .and_where(runner::Column::DevMachineId.is_not_null())
        .and_where(runner::Column::OwnerId.eq(owner_id))
        .and_where(runner::Column::Visibility.eq(visibility))
        .and_where(runner::Column::WorkspaceId.eq(workspace_id))
        .to_owned();
    let token_machines = Query::select()
        .distinct()
        .column(machine_token::Column::DevMachineId)
        .from(machine_token::Entity)
        .and_where(machine_token::Column::DevMachineId.is_not_null())
        .and_where(machine_token::Column::UserId.eq(owner_id))
        .and_where(machine_token::Column::WorkspaceId.eq(workspace_id))
        .to_owned();
    annotated_machines(owner_id, workspace_id, visibility, cutoff)
        .filter(
            Condition::any()
                .add(dev_machine::Column::Id.in_subquery(runner_machines))
                .add(dev_machine::Column::Id.in_subquery(token_machines)),
        )
        .filter(dev_machine::Column::OwnerId.eq(owner_id))
        .filter(dev_machine::Column::Visibility.eq(visibility))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
}

/// M5 (`runners.py:150-170`, `machine_serialize_sql`): the M3
/// annotations over one machine by pk, `Meta.ordering`, first.
pub async fn machine_serialize(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    owner_id: Uuid,
    workspace_id: Uuid,
    visibility: i16,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Result<Option<MachineListRow>, DbErr> {
    annotated_machines(owner_id, workspace_id, visibility, cutoff)
        .filter(dev_machine::Column::Id.eq(machine_id))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
        .into_model::<MachineListRow>()
        .one(db)
        .await
}

/// The M3/M5 select: machine columns, the runner annotations (over the
/// owner's private runners in the workspace) and M1 `control_online`,
/// grouped by machine.
fn annotated_machines(
    owner_id: Uuid,
    workspace_id: Uuid,
    visibility: i16,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Select<dev_machine::Entity> {
    let in_scope = || {
        Condition::all()
            .add(runner::Column::OwnerId.eq(owner_id))
            .add(runner::Column::Visibility.eq(visibility))
            .add(runner::Column::WorkspaceId.eq(workspace_id))
    };
    let runner_col = |column: runner::Column| Expr::col((runner::Entity, column));
    let online = in_scope()
        .add(runner::Column::RevokedAt.is_null())
        .add(runner::Column::Status.is_in(["online", "busy"]));
    let control_online = Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from(machine_session::Entity)
            .and_where(
                Expr::col((
                    machine_session::Entity,
                    machine_session::Column::DevMachineId,
                ))
                .equals((dev_machine::Entity, dev_machine::Column::Id)),
            )
            .and_where(machine_session::Column::LastSeenAt.gte(cutoff))
            .and_where(machine_session::Column::RevokedAt.is_null())
            .limit(1)
            .to_owned(),
    );
    let runners: RelationDef = dev_machine::Entity::belongs_to(runner::Entity)
        .from(dev_machine::Column::Id)
        .to(runner::Column::DevMachineId)
        .into();
    dev_machine::Entity::find()
        .select_only()
        .columns([
            dev_machine::Column::Id,
            dev_machine::Column::HostLabel,
            dev_machine::Column::Label,
            dev_machine::Column::Visibility,
            dev_machine::Column::Provisioning,
            dev_machine::Column::EnginePausedByOwner,
            dev_machine::Column::EngineAutoPauseReason,
            dev_machine::Column::EngineTeamRuns,
            dev_machine::Column::EngineSignInState,
            dev_machine::Column::EngineCredentialExpiresAt,
            dev_machine::Column::EngineState,
            dev_machine::Column::EngineStateReportedAt,
            dev_machine::Column::MergedIntoId,
            dev_machine::Column::LastSeenAt,
            dev_machine::Column::RevokedAt,
            dev_machine::Column::CreatedAt,
            dev_machine::Column::UpdatedAt,
        ])
        .expr_as(
            Func::count_distinct(Expr::case(in_scope(), runner_col(runner::Column::Id))),
            "runner_count",
        )
        .expr_as(
            Func::count_distinct(Expr::case(online, runner_col(runner::Column::Id))),
            "online_runner_count",
        )
        .expr_as(
            Func::max(Expr::case(
                in_scope(),
                runner_col(runner::Column::LastHeartbeatAt),
            )),
            "last_heartbeat_at",
        )
        .expr_as(control_online, "control_online")
        .join(JoinType::LeftJoin, runners)
        .group_by(dev_machine::Column::Id)
}

pub async fn machine_list(
    db: &impl ConnectionTrait,
    owner_id: Uuid,
    workspace_id: Uuid,
    visibility: i16,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<MachineListRow>, DbErr> {
    machine_list_query(owner_id, workspace_id, visibility, cutoff)
        .into_model::<MachineListRow>()
        .all(db)
        .await
}

/// One R1/R2 row: the runner's serialized columns, its dev-machine mini
/// (NULL when the `LEFT JOIN` misses: legacy enrollments with no
/// machine), its pod and the pod's project identifier (#2910 engine
/// dispatch + mini engine trio).
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct RunnerJoinRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub name: String,
    pub host_label: String,
    pub provisioning: String,
    pub pod_dispatch: String,
    pub visibility: i16,
    pub enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub capabilities: serde_json::Value,
    pub status: String,
    pub os: String,
    pub arch: String,
    pub runner_version: String,
    pub dev_metadata: serde_json::Value,
    pub protocol_version: i32,
    pub last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_reason: String,
    pub paused_at: Option<chrono::DateTime<chrono::Utc>>,
    pub paused_by_id: Option<Uuid>,
    pub dm_id: Option<Uuid>,
    pub dm_host_label: Option<String>,
    pub dm_label: Option<String>,
    pub dm_engine_paused_by_owner: Option<bool>,
    pub dm_engine_auto_pause_reason: Option<String>,
    pub dm_engine_team_runs: Option<bool>,
    pub dm_engine_sign_in_state: Option<String>,
    pub dm_engine_credential_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pod_workspace_id: Uuid,
    pub pod_project_id: Uuid,
    pub pod_name: String,
    pub pod_description: String,
    pub pod_created_by_id: Option<Uuid>,
    pub pod_is_default: bool,
    pub pod_created_at: chrono::DateTime<chrono::Utc>,
    pub pod_updated_at: chrono::DateTime<chrono::Utc>,
    pub project_identifier: String,
}

/// The R1/R2 select (`select_related("pod__project", "dev_machine")`):
/// runner, LEFT JOIN dev_machine, INNER JOIN pod and project.
fn runner_join_query() -> Select<runner::Entity> {
    let to_machine: RelationDef = runner::Entity::belongs_to(dev_machine::Entity)
        .from(runner::Column::DevMachineId)
        .to(dev_machine::Column::Id)
        .into();
    let to_pod: RelationDef = runner::Entity::belongs_to(pod::Entity)
        .from(runner::Column::PodId)
        .to(pod::Column::Id)
        .into();
    runner::Entity::find()
        .select_only()
        .columns([
            runner::Column::Id,
            runner::Column::OwnerId,
            runner::Column::WorkspaceId,
            runner::Column::DevMachineId,
            runner::Column::PodId,
            runner::Column::Name,
            runner::Column::HostLabel,
            runner::Column::Provisioning,
            runner::Column::PodDispatch,
            runner::Column::Visibility,
            runner::Column::EnrolledAt,
            runner::Column::Capabilities,
            runner::Column::Status,
            runner::Column::Os,
            runner::Column::Arch,
            runner::Column::RunnerVersion,
            runner::Column::DevMetadata,
            runner::Column::ProtocolVersion,
            runner::Column::LastHeartbeatAt,
            runner::Column::CreatedAt,
            runner::Column::UpdatedAt,
            runner::Column::RevokedAt,
            runner::Column::RevokedReason,
            runner::Column::PausedAt,
            runner::Column::PausedById,
        ])
        .column_as(dev_machine::Column::Id, "dm_id")
        .column_as(dev_machine::Column::HostLabel, "dm_host_label")
        .column_as(dev_machine::Column::Label, "dm_label")
        .column_as(dev_machine::Column::EnginePausedByOwner, "dm_engine_paused_by_owner")
        .column_as(dev_machine::Column::EngineAutoPauseReason, "dm_engine_auto_pause_reason")
        .column_as(dev_machine::Column::EngineTeamRuns, "dm_engine_team_runs")
        .column_as(dev_machine::Column::EngineSignInState, "dm_engine_sign_in_state")
        .column_as(dev_machine::Column::EngineCredentialExpiresAt, "dm_engine_credential_expires_at")
        .column_as(pod::Column::WorkspaceId, "pod_workspace_id")
        .column_as(pod::Column::ProjectId, "pod_project_id")
        .column_as(pod::Column::Name, "pod_name")
        .column_as(pod::Column::Description, "pod_description")
        .column_as(pod::Column::CreatedById, "pod_created_by_id")
        .column_as(pod::Column::IsDefault, "pod_is_default")
        .column_as(pod::Column::CreatedAt, "pod_created_at")
        .column_as(pod::Column::UpdatedAt, "pod_updated_at")
        .column_as(projects::Column::Identifier, "project_identifier")
        .join(JoinType::LeftJoin, to_machine)
        .join(JoinType::InnerJoin, to_pod)
        .join(JoinType::InnerJoin, pod::Relation::Projects.def())
}

/// R1 (`runners.py:289-333`, [`runner_list_sql`]): the caller's private
/// runners in the workspace, `?pod=` / bundled exclusion / `?project=`
/// in call order, `-updated_at`.
pub fn runner_list_query(
    workspace_id: Uuid,
    owner_id: Uuid,
    visibility: i16,
    pod_id: Option<Uuid>,
    exclude_provisioning: Option<&str>,
    project_id: Option<Uuid>,
) -> Select<runner::Entity> {
    let mut query = runner_join_query()
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .filter(runner::Column::OwnerId.eq(owner_id))
        .filter(runner::Column::Visibility.eq(visibility));
    if let Some(pod_id) = pod_id {
        query = query.filter(runner::Column::PodId.eq(pod_id));
    }
    if let Some(provisioning) = exclude_provisioning {
        query = query.filter(runner::Column::Provisioning.ne(provisioning));
    }
    if let Some(project_id) = project_id {
        query = query.filter(pod::Column::ProjectId.eq(project_id));
    }
    query.order_by_desc(runner::Column::UpdatedAt)
}

pub async fn runner_list(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    owner_id: Uuid,
    visibility: i16,
    pod_id: Option<Uuid>,
    exclude_provisioning: Option<&str>,
    project_id: Option<Uuid>,
) -> Result<Vec<RunnerJoinRow>, DbErr> {
    runner_list_query(
        workspace_id,
        owner_id,
        visibility,
        pod_id,
        exclude_provisioning,
        project_id,
    )
    .into_model::<RunnerJoinRow>()
    .all(db)
    .await
}

/// R2 (`runners.py:342-345`, [`runner_detail_sql`]): one runner by id,
/// `Meta.ordering`, first.
pub async fn runner_detail(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<Option<RunnerJoinRow>, DbErr> {
    runner_join_query()
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .into_model::<RunnerJoinRow>()
        .one(db)
        .await
}

/// R3 busy guard (`runners.py:409-412`, [`runner_busy_guard_sql`]): a
/// non-terminal run on, or pinned to, the runner.
pub async fn runner_is_busy(db: &impl ConnectionTrait, runner_id: Uuid) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::Id)
        .filter(
            Condition::any()
                .add(agent_run::Column::RunnerId.eq(runner_id))
                .add(agent_run::Column::PinnedRunnerId.eq(runner_id)),
        )
        .filter(agent_run::Column::Status.is_in(NON_TERMINAL_STATUSES.iter().copied()))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// R3 write (`runners.py:422, 424`, [`runner_patch_sql`]):
/// `runner.save(update_fields=...)` for the changed pod / name plus
/// `updated_at`.
pub async fn patch_runner(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
    pod_id: Option<Uuid>,
    name: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    let mut update = runner::Entity::update_many();
    if let Some(pod_id) = pod_id {
        update = update.col_expr(runner::Column::PodId, Expr::value(pod_id));
    }
    if let Some(name) = name {
        update = update.col_expr(runner::Column::Name, Expr::value(name));
    }
    update
        .col_expr(runner::Column::UpdatedAt, Expr::value(now))
        .filter(runner::Column::Id.eq(runner_id))
        .exec(db)
        .await
        .map(drop)
}

/// The reverse one-to-one `runner.live_state` (`models.py:1504-1558`).
pub async fn runner_live_state(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<Option<runner_live_state::Model>, DbErr> {
    runner_live_state::Entity::find_by_id(runner_id)
        .one(db)
        .await
}

/// The M6 locked machine row the revoke / rotate views read.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct LockedMachineRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub host_label: String,
    pub label: String,
    pub visibility: i16,
    pub provisioning: String,
    pub engine_paused_by_owner: bool,
    pub engine_auto_pause_reason: String,
    pub engine_team_runs: bool,
    pub engine_sign_in_state: String,
    pub engine_credential_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub engine_state: serde_json::Value,
    pub engine_state_reported_at: Option<chrono::DateTime<chrono::Utc>>,
    pub merged_into_id: Option<Uuid>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// M6 ([`machine_locked_read_sql`]): the machine by id, `Meta.ordering`,
/// first, `FOR UPDATE`.
pub async fn lock_machine(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<LockedMachineRow>, DbErr> {
    dev_machine::Entity::find()
        .select_only()
        .columns([
            dev_machine::Column::Id,
            dev_machine::Column::OwnerId,
            dev_machine::Column::HostLabel,
            dev_machine::Column::Label,
            dev_machine::Column::Visibility,
            dev_machine::Column::Provisioning,
            dev_machine::Column::EnginePausedByOwner,
            dev_machine::Column::EngineAutoPauseReason,
            dev_machine::Column::EngineTeamRuns,
            dev_machine::Column::EngineSignInState,
            dev_machine::Column::EngineCredentialExpiresAt,
            dev_machine::Column::EngineState,
            dev_machine::Column::EngineStateReportedAt,
            dev_machine::Column::MergedIntoId,
            dev_machine::Column::LastSeenAt,
            dev_machine::Column::RevokedAt,
            dev_machine::Column::CreatedAt,
            dev_machine::Column::UpdatedAt,
        ])
        .filter(dev_machine::Column::Id.eq(machine_id))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
        .lock_exclusive()
        .into_model::<LockedMachineRow>()
        .one(db)
        .await
}

/// `DevMachine.revoke()` (`runners.py:186-190`): stamp `revoked_at` and
/// `updated_at`.
pub async fn revoke_machine(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    revoked_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    dev_machine::Entity::update_many()
        .col_expr(dev_machine::Column::RevokedAt, Expr::value(revoked_at))
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(updated_at))
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await
        .map(drop)
}

fn live_machine_runners(machine_id: Uuid) -> Select<runner::Entity> {
    runner::Entity::find()
        .select_only()
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .filter(runner::Column::RevokedAt.is_null())
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
}

/// The machine revoke's locked runner list (`runners.py:192-205`,
/// [`machine_runners_locked_list_sql`]): `(id, revoked_at)` of the
/// machine's unrevoked runners, `FOR UPDATE`.
#[allow(clippy::type_complexity)]
pub async fn lock_live_machine_runners(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>, DbErr> {
    live_machine_runners(machine_id)
        .columns([runner::Column::Id, runner::Column::RevokedAt])
        .lock_exclusive()
        .into_tuple()
        .all(db)
        .await
}

/// The machine rotate's runner ids (`runners.py:229-231`,
/// [`machine_runner_ids_sql`]): unlocked.
pub async fn live_machine_runner_ids(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    live_machine_runners(machine_id)
        .column(runner::Column::Id)
        .into_tuple()
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::app_project::models::project as project_cols;
    use pidash_db::runner_enroll::columns::{machine_token as mt_cols, runner as r_cols};

    const FIXTURE_SQL: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/machines_runners.sql");
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/machines_runners.rows.json");
    const FIXTURE_WIRE: &str =
        include_str!("../../../../../fixtures/runner_enroll/external/wire_pins.json");

    /// Collapse every whitespace run to one space.
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Assert `needle` (squashed) is recorded in the D13-F5 SQL
    /// fixture, and return it. Every `assert_builder_contains` below
    /// goes through here so a stale needle fails loudly at the fixture
    /// end, not silently at the builder end.
    fn fixture_fragment(needle: &str) -> String {
        let needle = squashed(needle);
        assert!(
            squashed(FIXTURE_SQL).contains(&needle),
            "fragment missing from D13-F5 fixtures:\n{needle}"
        );
        needle
    }

    /// The D-02 direction: the fixture's recorded fragment must appear
    /// verbatim (modulo whitespace) in the builder's Django-exact output.
    fn assert_builder_contains(builder_sql: &str, fragment: &str) {
        let hay = squashed(builder_sql);
        assert!(
            hay.contains(fragment),
            "fixture fragment not found in builder SQL:\n{fragment}\n----\n{hay}"
        );
    }

    /// Split `SELECT <list> <suffix>`; assert the suffix is exactly the
    /// Django-compiled text and return the select list for a list-order pin.
    /// Splits on the MAIN `FROM` (the M1 `EXISTS` subquery embeds its own
    /// `FROM "machine_session"`, so a naive first-`" FROM "` split lands
    /// inside the select list).
    fn assert_suffix(builder_sql: &str, suffix: &str) -> String {
        let body = builder_sql
            .strip_prefix("SELECT ")
            .expect("builder must emit SELECT <list> FROM …");
        // The main FROM is the first `FROM` whose table is unaliased:
        // every subquery FROM is followed by an aliased table (`U0`),
        // and the main FROM precedes the WHERE-embedded subqueries.
        let mut split = None;
        for (i, _) in body.match_indices(" FROM ") {
            let after = &body[i + 6..];
            if after.starts_with("\"dev_machine\" LEFT")
                || after.starts_with("\"dev_machine\" WHERE")
                || after.starts_with("\"runner\"")
                || after.starts_with("\"pod\"")
            {
                split = Some(i);
                break;
            }
        }
        let i = split.expect("main FROM not found");
        let (list, rest) = (&body[..i], &body[i + 1..]);
        assert_eq!(rest, suffix, "FROM/WHERE/ORDER/LIMIT suffix");
        list.to_string()
    }

    // -- Oracle column order (transcribed from SQL compiled on Django 4.2.30) --

    const MACHINE_COLS: &[&str] = &[
        "id",
        "owner_id",
        "host_label",
        "label",
        "visibility",
        "provisioning",
        "engine_paused_by_owner",
        "engine_auto_pause_reason",
        "engine_team_runs",
        "engine_sign_in_state",
        "engine_credential_expires_at",
        "engine_state",
        "engine_state_reported_at",
        "merged_into_id",
        "merge_result",
        "last_seen_at",
        "revoked_at",
        "created_at",
        "updated_at",
    ];
    const RUNNER_COLS: &[&str] = &[
        "id",
        "owner_id",
        "workspace_id",
        "dev_machine_id",
        "pod_id",
        "name",
        "host_label",
        "provisioning",
        "pod_dispatch",
        "visibility",
        "refresh_token_hash",
        "refresh_token_fingerprint",
        "refresh_token_generation",
        "previous_refresh_token_hash",
        "access_token_signing_key_version",
        "enrollment_token_hash",
        "enrollment_token_fingerprint",
        "enrolled_at",
        "capabilities",
        "status",
        "os",
        "arch",
        "runner_version",
        "dev_metadata",
        "protocol_version",
        "last_heartbeat_at",
        "created_at",
        "updated_at",
        "revoked_at",
        "revoked_reason",
        "paused_at",
        "paused_by_id",
    ];
    const POD_COLS: &[&str] = &[
        "id",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "created_by_id",
        "is_default",
        "deleted_at",
        "created_at",
        "updated_at",
    ];

    fn expected_list(table: &str, cols: &[&str]) -> String {
        cols.iter()
            .map(|c| format!("\"{table}\".\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn reused_column_consts_match_compiled_order() {
        // The builders reuse db-crate consts; pin those consts to the
        // independently compiled Django column order so a drift fails here.
        assert_eq!(dm_cols::COLUMNS, MACHINE_COLS);
        assert_eq!(dm_cols::TABLE, "dev_machine");
        assert_eq!(r_cols::COLUMNS, RUNNER_COLS);
        assert_eq!(r_cols::TABLE, "runner");
        assert_eq!(pod_cols::COLUMNS, POD_COLS);
        assert_eq!(pod_cols::TABLE, "pod");
        assert_eq!(mt_cols::TABLE, "machine_token");
        assert_eq!(MACHINE_SESSION_TABLE, "machine_session");
        assert_eq!(AGENT_RUN_TABLE, "agent_run");
        // Cross-domain projects list (46 cols, id sixth) comes from the
        // merged app_project port — spot-pin head, id slot and tail.
        assert_eq!(
            &project_cols::COLUMNS[..6],
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id"
            ]
        );
        assert_eq!(project_cols::COLUMNS.len(), 46);
        assert_eq!(project_cols::COLUMNS[12], "identifier");
        assert_eq!(project_cols::COLUMNS[45], "default_agent_executor");
        assert_eq!(project_cols::TABLE, "projects");
    }

    #[test]
    fn pinned_scalars_match_fixtures() {
        assert_eq!(CONTROL_PRESENCE_WINDOW_SECS, 90);
        assert_eq!(
            NON_TERMINAL_STATUSES,
            &[
                "queued",
                "assigned",
                "waiting_for_worktree",
                "running",
                "cancel_requested",
                "awaiting_approval",
                "awaiting_reauth",
                "paused_awaiting_input",
            ]
        );
        // F8 annotates waiting_for_worktree in prose; the value is the prefix.
        let wire: serde_json::Value = serde_json::from_str(FIXTURE_WIRE).unwrap();
        let pinned = wire["non_terminal_statuses"]["value"].as_array().unwrap();
        assert_eq!(pinned.len(), NON_TERMINAL_STATUSES.len());
        for (entry, value) in pinned.iter().zip(NON_TERMINAL_STATUSES.iter()) {
            assert!(
                entry.as_str().unwrap().starts_with(value),
                "{entry:?} vs {value}"
            );
        }
        assert_eq!(
            wire["non_terminal_statuses"]["source"],
            "runner/services/matcher.py:54-66"
        );
        // The retired-but-kept member is what makes the set 8, not 7.
        assert!(!NON_TERMINAL_STATUSES.contains(&"completed"));
        assert!(!NON_TERMINAL_STATUSES.contains(&"failed"));
        assert!(!NON_TERMINAL_STATUSES.contains(&"cancelled"));
        assert!(!NON_TERMINAL_STATUSES.contains(&"refused"));
        assert!(!NON_TERMINAL_STATUSES.contains(&"blocked"));
    }

    // -- M1 --

    // -- M3 --

    // -- M4 --

    // -- M5 --

    // -- M6/M7 --

    #[test]
    fn m6_lock_read_orders_limits_for_update() {
        let sql = machine_locked_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 \
             ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC \
             LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("dev_machine", MACHINE_COLS));
        for fragment in [
            fixture_fragment("FROM \"dev_machine\""),
            fixture_fragment("\"id\" = $1"),
            fixture_fragment("LIMIT 1 FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn m6_revoke_update_binds_now_twice() {
        assert_eq!(MACHINE_REVOKE_UPDATE_FIELDS, &["revoked_at", "updated_at"]);
        assert_eq!(
            machine_revoke_sql(),
            "UPDATE \"dev_machine\" SET \"revoked_at\" = $1, \"updated_at\" = $2 \
             WHERE \"dev_machine\".\"id\" = $3",
        );
        for fragment in [
            fixture_fragment("UPDATE \"dev_machine\" SET \"revoked_at\" = "),
            fixture_fragment("\"updated_at\" = "),
        ] {
            assert_builder_contains(&machine_revoke_sql(), &fragment);
        }
    }

    // (The token-revoke pin moved with the statement to
    // `crate::domain::tokens`; runners.py:198 and :238 still run the
    // identical call through `revoke_tokens_for_dev_machine`.)

    // -- R1 --

    #[test]
    fn include_bundled_matches_exact_strings_only() {
        assert!(include_bundled(Some("1")));
        assert!(include_bundled(Some("true")));
        assert!(include_bundled(Some("yes")));
        assert!(!include_bundled(None));
        assert!(!include_bundled(Some("")));
        assert!(!include_bundled(Some("True")));
        assert!(!include_bundled(Some("TRUE")));
        assert!(!include_bundled(Some("0")));
        assert!(!include_bundled(Some("no")));
        // rows.json pins the case-sensitivity explicitly.
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert!(rows["R1_filters"]["bundled_default"]
            .as_str()
            .unwrap()
            .contains("'True'/'TRUE' do NOT match"));
        assert!(rows["R1_filters"]["pod"]
            .as_str()
            .unwrap()
            .contains("no existence check"));
    }

    // -- R2 --

    #[test]
    fn detail_outcome_follows_get_runner_order() {
        // Missing row, whatever the flags say.
        assert_eq!(detail_outcome(false, true, true), DetailOutcome::Missing);
        assert_eq!(detail_outcome(false, false, false), DetailOutcome::Missing);
        // Present but no workspace membership -> 403, even if viewable.
        assert_eq!(detail_outcome(true, false, true), DetailOutcome::Forbidden);
        assert_eq!(detail_outcome(true, false, false), DetailOutcome::Forbidden);
        // Present member the kernel cannot view -> 404 (no leak).
        assert_eq!(detail_outcome(true, true, false), DetailOutcome::Missing);
        // Present member owner -> proceed.
        assert_eq!(detail_outcome(true, true, true), DetailOutcome::Found);
    }

    // -- R3 --

    #[test]
    fn r3_pod_lock_leads_with_manager_scope() {
        let sql = pod_locked_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"pod\" \
             WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
             ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC \
             LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("pod", POD_COLS));
        for fragment in [
            fixture_fragment("FROM \"pod\""),
            fixture_fragment("LIMIT 1 FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn real_move_is_pod_inequality() {
        let a = Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
        let b = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
        assert!(is_real_move(&a, &b));
        assert!(!is_real_move(&a, &a));
    }

    #[test]
    fn normalize_runner_name_strips_python_whitespace() {
        assert_eq!(normalize_runner_name(None), "");
        assert_eq!(normalize_runner_name(Some("")), "");
        assert_eq!(normalize_runner_name(Some("  worker-1  ")), "worker-1");
        assert_eq!(normalize_runner_name(Some("   ")), "");
        assert_eq!(normalize_runner_name(Some("\t\nx\r")), "x");
        // Interior whitespace survives.
        assert_eq!(normalize_runner_name(Some("a b")), "a b");
        // Python-strip-only characters (Rust trim() would keep U+001C).
        assert_eq!(normalize_runner_name(Some("\u{a0}")), "");
        assert_eq!(
            normalize_runner_name(Some("\u{1c}\u{85}x\u{1f} \u{85}")),
            "x"
        );
    }

    // -- Unit 4 helpers --

    #[test]
    fn request_workspace_id_follows_or_chain_then_strip() {
        assert_eq!(request_workspace_id(None, None), "");
        assert_eq!(request_workspace_id(Some("w1"), Some("w2")), "w1");
        assert_eq!(request_workspace_id(Some(""), Some("w2")), "w2");
        assert_eq!(request_workspace_id(None, Some("w2")), "w2");
        assert_eq!(request_workspace_id(Some("w1"), None), "w1");
        assert_eq!(request_workspace_id(Some("  w1  "), None), "w1");
        // QUIRK-workspace-or-chain: whitespace-only body wins (truthy),
        // then strips to "" -> 400, IGNORING the query param.
        assert_eq!(request_workspace_id(Some("  "), Some("w2")), "");
        assert_eq!(request_workspace_id(Some(""), Some("")), "");
    }

    // -- rows.json shape --

    /// The SeaORM gate reads against constraint-free temp copies of their
    /// tables on one sqlx transaction (`TxConn`): the runner gate keeps
    /// the pod/project INNER JOINs, the probes match only in scope.
    #[tokio::test]
    async fn live_gate_reads() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "runner",
            "pod",
            "projects",
            "dev_machine",
            "machine_token",
            "workspace_members",
            "workspaces",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        let (user, ws, project, pod_id, orphan_pod, machine, runner_id, orphan) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO projects (id, identifier, workspace_id) VALUES ('{project}', 'PRJ', '{ws}')"),
            format!("INSERT INTO pod (id, project_id) VALUES ('{pod_id}', '{project}')"),
            format!(
                "INSERT INTO runner (id, owner_id, workspace_id, pod_id, dev_machine_id, visibility, created_at) \
                 VALUES ('{runner_id}', '{user}', '{ws}', '{pod_id}', '{machine}', 0, now()), \
                 ('{orphan}', '{user}', '{ws}', '{orphan_pod}', NULL, 0, now())"
            ),
            format!("INSERT INTO dev_machine (id, owner_id, visibility, revoked_at, created_at) VALUES ('{machine}', '{user}', 0, now(), now())"),
            format!("INSERT INTO machine_token (id, dev_machine_id, user_id, workspace_id) VALUES (gen_random_uuid(), '{machine}', '{user}', '{ws}')"),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, created_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{user}', 20, true, now()), \
                 (gen_random_uuid(), '{ws}', '{user}', 5, true, now() - interval '1 day')"
            ),
            format!("INSERT INTO workspaces (id, slug, created_at) VALUES ('{ws}', 'ws-gate', now())"),
        ] {
            sqlx::query(&sql).execute(&mut *tx).await.expect("seed");
        }
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let gate = runner_gate(&conn, runner_id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(
            (gate.owner_id, gate.workspace_id, gate.visibility),
            (user, ws, 0)
        );
        // Pod row missing: the INNER JOIN drops the runner.
        assert_eq!(runner_gate(&conn, orphan).await.expect("read"), None);
        let machine_row = machine_gate(&conn, machine)
            .await
            .expect("read")
            .expect("row");
        assert!(machine_row.revoked_at.is_some());
        assert!(machine_scope_runner_exists(&conn, machine, user, ws)
            .await
            .expect("probe"));
        assert!(
            !machine_scope_runner_exists(&conn, machine, Uuid::new_v4(), ws)
                .await
                .expect("probe")
        );
        assert!(machine_scope_token_exists(&conn, machine, user, ws)
            .await
            .expect("probe"));
        assert!(
            !machine_scope_token_exists(&conn, machine, user, Uuid::new_v4())
                .await
                .expect("probe")
        );
        // Newest membership wins (`-created_at`).
        assert_eq!(
            workspace_role(&conn, ws, user).await.expect("role"),
            Some(20)
        );
        assert_eq!(
            workspace_slug(&conn, ws).await.expect("slug").as_deref(),
            Some("ws-gate")
        );
        assert!(project_identifier_exists(&conn, ws, "PRJ")
            .await
            .expect("probe"));
        assert!(!project_identifier_exists(&conn, ws, "NOPE")
            .await
            .expect("probe"));
    }

    /// M3 annotations, R1/R2 joins, the busy guard, the patch and the live
    /// state against constraint-free temp tables on one transaction.
    #[tokio::test]
    async fn live_machine_and_runner_reads() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "dev_machine",
            "runner",
            "machine_token",
            "machine_session",
            "pod",
            "projects",
            "agent_run",
            "runner_live_state",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        // `GROUP BY dev_machine.id` relies on the primary key (functional
        // dependency), as on the real table.
        sqlx::query("ALTER TABLE dev_machine ADD PRIMARY KEY (id)")
            .execute(&mut *tx)
            .await
            .expect("pk");
        let (me, ws, other_ws) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (m1, m2, m3, pod_id, project) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (r1, r2, r3) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let runner_row = |id: Uuid,
                          machine: Option<Uuid>,
                          workspace: Uuid,
                          status: &str,
                          beat: &str| {
            let machine = machine.map_or("NULL".to_owned(), |m| format!("'{m}'"));
            format!(
                "INSERT INTO runner (id, owner_id, workspace_id, dev_machine_id, pod_id, name, host_label, \
                 provisioning, visibility, capabilities, status, os, arch, runner_version, dev_metadata, \
                 protocol_version, last_heartbeat_at, created_at, updated_at, revoked_reason) VALUES \
                 ('{id}', '{me}', '{workspace}', {machine}, '{pod_id}', 'r-{status}', 'h', 'manual', 0, \
                 '[]', '{status}', '', '', '', '{{}}', 1, {beat}, now(), now() - interval '{n} min', '')",
                n = if status == "online" { 1 } else { 5 },
            )
        };
        for sql in [
            format!(
                "INSERT INTO dev_machine (id, owner_id, host_label, label, visibility, provisioning, last_seen_at, created_at, updated_at) VALUES \
                 ('{m1}', '{me}', 'one', 'One', 0, 'manual', now(), now(), now()), \
                 ('{m2}', '{me}', 'two', 'Two', 0, 'manual', NULL, now(), now()), \
                 ('{m3}', '{me}', 'three', 'Three', 0, 'manual', now(), now(), now())"
            ),
            runner_row(r1, Some(m1), ws, "online", "now()"),
            runner_row(r2, Some(m1), ws, "offline", "now() - interval '1 hour'"),
            // A runner in another workspace: joined, but outside every annotation.
            runner_row(r3, Some(m1), other_ws, "online", "now() + interval '1 hour'"),
            format!("INSERT INTO machine_token (id, dev_machine_id, user_id, workspace_id) VALUES (gen_random_uuid(), '{m2}', '{me}', '{ws}')"),
            format!("INSERT INTO machine_session (id, dev_machine_id, last_seen_at, revoked_at) VALUES (gen_random_uuid(), '{m1}', now(), NULL)"),
            format!(
                "INSERT INTO pod (id, workspace_id, project_id, name, description, is_default, created_at, updated_at) \
                 VALUES ('{pod_id}', '{ws}', '{project}', 'main', '', true, now(), now())"
            ),
            format!("INSERT INTO projects (id, identifier) VALUES ('{project}', 'MGR')"),
            format!(
                "INSERT INTO runner_live_state (runner_id, usage, updated_at, turn_count) \
                 VALUES ('{r1}', '{{}}', now(), 3)"
            ),
        ] {
            sqlx::query(&sql).execute(&mut *tx).await.expect("seed");
        }
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(CONTROL_PRESENCE_WINDOW_SECS);
        let machines = machine_list(&conn, me, ws, 0, cutoff).await.expect("M3");
        // m3 has no runner and no token in the workspace; m2 (NULL
        // last_seen_at) sorts first under DESC.
        assert_eq!(
            machines.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![m2, m1]
        );
        let one = &machines[1];
        assert_eq!((one.runner_count, one.online_runner_count), (2, 1));
        assert!(one.control_online);
        let beat = one.last_heartbeat_at.expect("beat");
        assert!(
            beat <= chrono::Utc::now() + chrono::Duration::minutes(1),
            "{beat}"
        );
        let two = &machines[0];
        assert_eq!(
            (
                two.runner_count,
                two.online_runner_count,
                two.control_online
            ),
            (0, 0, false)
        );
        assert_eq!(two.last_heartbeat_at, None);

        let listed = runner_list(
            &conn,
            ws,
            me,
            0,
            None,
            Some("desktop_bundled"),
            Some(project),
        )
        .await
        .expect("R1");
        // -updated_at: r1 (1 min ago) before r2 (5 min ago).
        assert_eq!(
            listed.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![r1, r2]
        );
        assert_eq!(
            (
                listed[0].dm_label.as_deref(),
                listed[0].pod_name.as_str(),
                listed[0].project_identifier.as_str()
            ),
            (Some("One"), "main", "MGR")
        );
        assert!(runner_list(&conn, ws, me, 0, None, Some("manual"), None)
            .await
            .expect("R1")
            .is_empty());
        let detail = runner_detail(&conn, r2).await.expect("R2").expect("row");
        assert_eq!(
            (detail.name.as_str(), detail.pod_is_default),
            ("r-offline", true)
        );

        assert!(!runner_is_busy(&conn, r1).await.expect("busy"));
        conn.execute_unprepared(&format!(
            "INSERT INTO agent_run (id, pinned_runner_id, status) VALUES (gen_random_uuid(), '{r1}', '{}')",
            NON_TERMINAL_STATUSES[0]
        ))
        .await
        .expect("seed");
        assert!(runner_is_busy(&conn, r1).await.expect("busy"));
        patch_runner(&conn, r2, None, Some("renamed"), chrono::Utc::now())
            .await
            .expect("patch");
        assert_eq!(
            runner_detail(&conn, r2)
                .await
                .expect("R2")
                .expect("row")
                .name,
            "renamed"
        );
        let live = runner_live_state(&conn, r1)
            .await
            .expect("live")
            .expect("row");
        assert_eq!(live.turn_count, Some(3));
        assert!(runner_live_state(&conn, r2).await.expect("live").is_none());
    }

    /// The teardown statements: M6 lock, revoke stamp, the live-runner
    /// lists, M5 against M3 on the same data, and the E3-E5 refresh reads
    /// and writes.
    #[tokio::test]
    async fn live_teardown_statements() {
        use crate::runner_enroll::queries::enroll_reads;
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "dev_machine",
            "runner",
            "machine_token",
            "machine_session",
            "workspaces",
            "runner_force_refresh",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        sqlx::query("ALTER TABLE dev_machine ADD PRIMARY KEY (id)")
            .execute(&mut *tx)
            .await
            .expect("pk");
        let (me, ws, machine, live, gone) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!(
                "INSERT INTO dev_machine (id, owner_id, host_label, label, visibility, provisioning, created_at, updated_at) \
                 VALUES ('{machine}', '{me}', 'h', 'H', 0, 'manual', now(), now())"
            ),
            format!(
                "INSERT INTO runner (id, owner_id, workspace_id, dev_machine_id, visibility, status, \
                 refresh_token_hash, refresh_token_generation, previous_refresh_token_hash, created_at, revoked_at) VALUES \
                 ('{live}', '{me}', '{ws}', '{machine}', 0, 'online', 'cur', 4, 'prev', now(), NULL), \
                 ('{gone}', '{me}', '{ws}', '{machine}', 0, 'revoked', '', 0, '', now(), now())"
            ),
            format!("INSERT INTO workspaces (id) VALUES ('{ws}')"),
            format!("INSERT INTO runner_force_refresh (runner_id, min_rtg) VALUES ('{live}', 9)"),
        ] {
            sqlx::query(&sql).execute(&mut *tx).await.expect("seed");
        }
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let locked = lock_machine(&conn, machine)
            .await
            .expect("M6")
            .expect("row");
        assert_eq!((locked.owner_id, locked.revoked_at), (me, None));
        assert_eq!(
            lock_live_machine_runners(&conn, machine)
                .await
                .expect("runners"),
            vec![(live, None)]
        );
        assert_eq!(
            live_machine_runner_ids(&conn, machine).await.expect("ids"),
            vec![live]
        );
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(CONTROL_PRESENCE_WINDOW_SECS);
        let listed = machine_list(&conn, me, ws, 0, cutoff).await.expect("M3");
        let single = machine_serialize(&conn, machine, me, ws, 0, cutoff)
            .await
            .expect("M5")
            .expect("row");
        assert_eq!(listed, vec![single.clone()]);
        assert_eq!((single.runner_count, single.online_runner_count), (2, 1));
        let now = chrono::Utc::now();
        revoke_machine(&conn, machine, now, now)
            .await
            .expect("revoke");
        assert!(lock_machine(&conn, machine)
            .await
            .expect("M6")
            .expect("row")
            .revoked_at
            .is_some());
        assert!(enroll_reads::dev_machine_is_revoked(&conn, machine)
            .await
            .expect("E4"));

        let refresh = enroll_reads::refresh_locked_runner(&conn, live)
            .await
            .expect("E3")
            .expect("row");
        assert_eq!(
            (
                refresh.refresh_token_hash.as_str(),
                refresh.refresh_token_generation,
                refresh.previous_refresh_token_hash.as_str()
            ),
            ("cur", 4, "prev")
        );
        // No workspace row: the E3 INNER JOIN drops the runner.
        let orphan = Uuid::new_v4();
        conn.execute_unprepared(&format!(
            "INSERT INTO runner (id, owner_id, workspace_id, created_at) VALUES ('{orphan}', '{me}', '{}', now())",
            Uuid::new_v4()
        ))
        .await
        .expect("seed");
        assert_eq!(
            enroll_reads::refresh_locked_runner(&conn, orphan)
                .await
                .expect("E3"),
            None
        );
        assert!(enroll_reads::refresh_locked_runner(&conn, gone)
            .await
            .expect("E3")
            .is_some());
        enroll_reads::rotate_refresh_token(&conn, live, "new", "fp", 5, "cur")
            .await
            .expect("E5");
        let rotated = enroll_reads::refresh_locked_runner(&conn, live)
            .await
            .expect("E3")
            .expect("row");
        assert_eq!(
            (
                rotated.refresh_token_hash.as_str(),
                rotated.refresh_token_generation,
                rotated.previous_refresh_token_hash.as_str()
            ),
            ("new", 5, "cur")
        );
        enroll_reads::clear_force_refresh(&conn, live)
            .await
            .expect("clear");
        let left: Option<i32> = conn
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Postgres,
                "SELECT 1 FROM runner_force_refresh",
            ))
            .await
            .expect("probe")
            .map(|_| 1);
        assert_eq!(left, None);
    }
}
