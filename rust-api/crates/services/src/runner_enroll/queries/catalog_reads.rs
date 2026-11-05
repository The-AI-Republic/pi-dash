#![forbid(unsafe_code)]

//! Pods + projects + desktop read/write statement sets (queries-C, PIDASHCONV-584).
//!
//! Port of the five statement sets in `runner/views/pods.py`,
//! `runner/views/projects.py` and `runner/views/desktop.py`:
//!
//! 1. Pod list/detail/create reads (P1-P3, `pods.py:55-130`):
//!    project-wins vs workspace list ordering (P1), the project read
//!    (P1/P2, also the rename's `pod.project` FK read at `:176`), the
//!    `_get_pod` detail read with its None-vs-False contract (P3), the
//!    per-row unfiltered runner count, and the pod insert (P2).
//! 2. Pod patch/delete + guards + sweep (P4-P5, `pods.py:155-267`):
//!    rename/description/default-promote updates (P4, incl the sibling
//!    demote), the locked delete read plus the three §7.2 guards in
//!    order — non-revoked runners 409, non-terminal runs 409 over the
//!    pinned [`NON_TERMINAL_STATUSES`], default-pod 409 — the
//!    soft-delete stamp and the `Issue.assigned_pod` sweep (P5).
//! 3. Projects serialize statements (J1-J2, `projects.py:31-77,97-124`):
//!    the pod-values query (default-first, name order), the project
//!    query (identifier order), and the workspace-membership scoping
//!    inputs for the three auth modes (incl the missing-`is_active`
//!    quirk, ported as-is).
//! 4. Desktop enroll statements (K1-K2, `desktop.py:107-149`): the
//!    bundled-machine lookup (owner + host + provisioning + unrevoked +
//!    workspace token join, `-created_at`), the create-or-touch, and the
//!    per-(machine, workspace) token revoke + mint insert.
//! 5. Desktop delete statements (K3, `desktop.py:168-194`): the
//!    bundled-machine id select (+ optional host filter), the token
//!    revoke update, and the bundled-runner offline update excluding
//!    revoked rows.
//!
//! Fixture record: `rust-api/fixtures/runner_enroll/queries/` —
//! `pods_projects_desktop.sql` + `pods_projects_desktop.rows.json`
//! (D13-F5, filed by PIDASHCONV-578) and the status-set pin in
//! `external/wire_pins.json` (`non_terminal_statuses`, D13-F8). The
//! fixture SQL is hand-composed and abbreviates several Django
//! renderings (see "Fixture deviations" below); every builder below
//! was instead verified against SQL compiled from the real querysets
//! on the repo-pinned Django 4.2.30, and emits that exact text with
//! `%s` replaced by `$N`. The `#[cfg(test)]` suite pins the recorded
//! fixture fragments inside the builder output (the
//! `assert_builder_contains` direction from the D-02 `space/queries/`
//! precedent) AND the full compiled text, plus the D13-F8 value pin
//! and the rows-shape rules.
//!
//! Conventions (same as `space/queries/` and queries-A `enroll_reads`):
//!
//! * Builders return owned SQL text; `$N` params are documented in
//!   first-appearance (binding) order. Handlers bind them positionally.
//! * `WHERE` conjunct order is Django's: manager-scope conjuncts first,
//!   then the filter kwargs in `Q`-sorted order. `$N` numbering follows
//!   that order, NOT the Python call order (e.g. J2 binds `member_id=$1,
//!   workspace_id=$2` although the source passes `workspace_id` first;
//!   K1 binds `host_label=$1, workspace_id=$2, owner_id=$3,
//!   provisioning=$4`).
//! * On `UPDATE`s the `SET` binds come first (`$1..`), then the `WHERE`
//!   binds. `SET x = NULL` renders literally (no bind); bare booleans
//!   (`"pod"."is_default"`) and `IS NULL` conjuncts take no bind.
//! * `save(update_fields=[...])` emits `SET` in Django *model-field*
//!   order, not in `update_fields` list order (`Model.save_base`,
//!   `django/db/models/base.py`). P4's `list(set(...))` order is
//!   therefore irrelevant (QUIRK-patch-set-order); the builders emit
//!   the verified model order.
//! * `.exists()` emits `SELECT 1 AS "a" … LIMIT 1` (ordering cleared);
//!   `.count()` emits `SELECT COUNT(*) AS "__count" …` (ordering
//!   cleared); `.get()`'s `LIMIT 21` multiplicity probe is omitted per
//!   the queries-A QUIRK-get-no-limit precedent — handlers map row
//!   counts (`0 → miss`, `>1 → MultipleObjectsReturned`).
//! * `Pod.save()`'s workspace-denorm check (`models.py:145-159`) runs on
//!   EVERY save, including `update_fields` saves: unless `pod.project`
//!   is already cached it fires one project read first. That read is a
//!   forward-FK access, so it uses `_base_manager` — the base variant
//!   [`project_by_id_base_sql`], NOT the scoped [`project_by_id_sql`]
//!   (PIDASHCONV-1210). The P2 create passes the project object (cached —
//!   no extra read); the P4 patch and P5 stamp saves do not (handlers run
//!   the base read).
//! * Timestamps, UUIDs and computed strings cross this boundary as bind
//!   params (`now()`, fresh UUIDs, labels and prefixed names are
//!   computed by the caller); the SQL only names the `$N` slots. The
//!   pure helpers at the bottom compute exactly those values.
//! * This module is self-contained within the declared DAG (blocked by
//!   fixtures + models only): it reuses the merged `pidash_db` column
//!   consts and pins its own cross-domain table facts as string consts
//!   (`agent_run`, `issues`); it does NOT `use super::enroll_reads`.
//!   Twin builders/helpers that emit identical text to queries-A (the
//!   dev-machine and machine-token inserts, the touch, the K2 rotate)
//!   are documented as such — the D-15 Pod-const precedent blesses the
//!   duplication over an undeclared compile edge.
//!
//! Out of scope here (owned elsewhere, referenced so handlers can find them):
//!
//! * `is_workspace_member` / `is_workspace_admin` — kernel membership
//!   facts (`pidash_auth`, consumed by handlers, never re-ported).
//!   `_can_manage_pod` (`pods.py:40-42`) is admin-OR-creator over them.
//! * `validate_user_pod_name` — already ported
//!   (`runner_enroll::pod_naming`, PIDASHCONV-585); the bare-suffix
//!   re-prefix below feeds it.
//! * The desktop version floor (`_version_is_allowed`, `:37-62`) and
//!   the workspace-by-slug read (E6a in queries-A) — guards BEFORE the
//!   K1 tx, owned by handlers-E (PIDASHCONV-595).
//! * Branch logic over `request.data` (`bool(is_default)` truthiness,
//!   `or ""` coercions, blank checks) — handlers-owned; the exact
//!   Python semantics are quoted on each builder so handlers port them
//!   faithfully (see QUIRK-patch-bool below).
//! * Response bodies (every `{"error", "code"}` shape + status) —
//!   handlers-B/E own every status + body; the guard→error mapping is
//!   documented on each builder but no body const lives here.
//! * The J1 row-assembly loop (`default_pod_ids` first-win,
//!   `pod_count`, embedding order) — pure handlers-E logic over the
//!   two queries' rows; the rule is quoted on
//!   [`workspace_pods_query`].
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-membership-no-active (J2, `projects.py:109-111,116-120`): both
//!   membership reads use a RAW `WorkspaceMember.objects.filter(...)`
//!   with NO `is_active` filter — every other D-13 membership check
//!   goes through `is_workspace_member` WITH it. Ported as-is; the
//!   `SoftDeletionManager` scope (`deleted_at IS NULL`) DOES apply.
//! * BUG-runner-count-includes-revoked (P1, `serializers.py:71-72` vs
//!   `pods.py:226-228`): `get_runner_count` is an UNFILTERED
//!   `pod.runners.count()` while guard 1 excludes revoked rows — the
//!   `:227` comment claims they match, but they do not (fixture D13-F2
//!   `runner_count_mismatch`). Ported as-is ([`pod_runner_count_sql`]
//!   has no status conjunct).
//! * QUIRK-demote-leaves-no-default (P4, `:198-200`): demoting the
//!   default pod to `False` is allowed and leaves the project with NO
//!   default. No guard. Ported as-is.
//! * QUIRK-create-conflict-500 (P2): the unique violation on
//!   `pod_unique_name_per_project_when_active` is NOT caught — a
//!   double create 500s. Ported as-is (no handler wraps the insert).
//! * QUIRK-desktop-join-ignores-token-revoked (K1, `:115`): the
//!   `machine_tokens__workspace` join does NOT filter token
//!   `revoked_at` — even a fully-revoked token row makes the machine
//!   reusable. Ported as-is (no conjunct on the joined table).
//! * QUIRK-patch-set-order (P4, `:203`): the final save passes
//!   `list(set(updates + ["updated_at"]))` — nondeterministic Python
//!   order — but Django emits `SET` in model-field order, so the SQL
//!   is deterministic. [`pod_patch_update_sql`] takes flags and emits
//!   model order (`name, description, is_default, updated_at`).
//! * QUIRK-patch-bool (P4, `:188`): `wants_default =
//!   bool(request.data.get("is_default"))` — Python truthiness, NOT
//!   JSON-boolean parsing: `bool("false")` is `True`, missing/None/""/0
//!   are `False`. Handlers-owned branch; quoted here so it is ported
//!   faithfully.
//! * QUIRK-bare-boolean: `BooleanField(exact=True)` renders as a bare
//!   column (`"pod"."is_default"`), not `= TRUE` (P4 demote).
//! * QUIRK-in-dedupe (K3): Django dedupes `__in` values (verified:
//!   `[A,A,B]` renders `IN (A, B)`); an empty list raises
//!   `EmptyResultSet` and the `UPDATE` is never sent. Callers pass PK
//!   lists (distinct by definition) and never empty (the source
//!   returns 204 first) — both builders panic on `machine_count == 0`
//!   to make that contract loud.
//!
//! # Fixture deviations (fixture abbreviates; builders are Django-exact)
//!
//! * `.first()` reads keep `Meta.ordering` + `LIMIT 1`; the fixture
//!   spells P1-project/P2-project/P3 without `ORDER BY`.
//! * P5-sweep carries the `SoftDeletionManager` scope
//!   (`"issues"."deleted_at" IS NULL`) — the fixture's `UPDATE`
//!   omits it. Only live issues are swept.
//! * `.exclude(status=…)` renders `NOT ("t"."status" = $N)`, not
//!   `status <> …` (guard 1, K3 offline — same class as the queries-A
//!   E7-cap deviation).
//! * `.exists()` renders `SELECT 1 AS "a" … LIMIT 1`, not
//!   `SELECT EXISTS(…)` (both guards, J2 probe — same class as the
//!   queries-A E4 deviation).
//! * J2 binds `member_id=$1, workspace_id=$2` (`Q`-sorted); the fixture
//!   spells `workspace_id=$1 AND member_id=$2`.
//! * Guard 2 binds 8 status params (`$2..$9` in
//!   `NON_TERMINAL_STATUSES` order); the fixture inlines the literals.
//! * J2-wsids and K3-ids keep `Meta.ordering` (`-created_at`,
//!   `-last_seen_at, -created_at`); the fixture spells "no ORDER BY".
//! * K1's `WHERE` order is Django's join-aware `Q`-sort
//!   (`host_label, machine_token.workspace_id, owner_id,
//!   provisioning, revoked_at`), not the Python kwarg order.
//! * INSERT labels/prefixed names are Python-computed binds, not SQL
//!   expressions (same class as the queries-A B2 deviation).
//!
//! Django-idiom → Rust-pattern rows applied: `$N` bind builders
//! (`space/queries/issue_retrieve.rs`, queries-A `enroll_reads.rs`);
//! char-boundary-safe `[:N]` slicing + Python-`strip()` parity
//! (Porting guide semantic traps); pinned cross-domain table facts
//! instead of cross-domain imports (split-review direction for
//! `agent_run`).

use pidash_compat::py::py_strip;
use pidash_db::app_project::models::project as project_cols;
use pidash_db::entity::{
    dev_machine, machine_token, pod, projects, runner, workspace_members, workspaces,
};
use pidash_db::runner_enroll::columns::{pod as pod_cols, runner as r_cols};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationDef, Select, Set,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Cross-domain table pins (own SQL; D-14/D-15 own these tables' ports)
// ---------------------------------------------------------------------------

/// `agent_run` physical table (`runner/models.py`, `db_table`, D-15 owned).
/// Pinned here because guard 2's `EXISTS` needs it; the split review
/// directs D-13 to pin the SQL it needs via F5/F8, not import it.
pub const AGENT_RUN_TABLE: &str = "agent_run";
/// `AgentRun.pod` FK attname (`related_name="agent_runs"`, `:898-902`).
pub const AGENT_RUN_POD_FK: &str = "pod_id";
/// `AgentRun.status` column.
pub const AGENT_RUN_STATUS_COL: &str = "status";

/// `issues` physical table (`db/models/issue.py:253`, D-26 owned).
/// Pinned here because the P5 sweep updates it.
pub const ISSUES_TABLE: &str = "issues";
/// `Issue.assigned_pod` FK attname (`:206-212`, nullable, PROTECT).
pub const ISSUES_ASSIGNED_POD_FK: &str = "assigned_pod_id";

/// Pinned `matcher.NON_TERMINAL_STATUSES`
/// (`runner/services/matcher.py:54-66`, D13-F8 `non_terminal_statuses`).
/// Tuple order, which is also the guard-2 `$2..$9` bind order:
/// queued, assigned, waiting_for_worktree (retired PDASHOSS01-137 but
/// KEPT — historical rows must keep gating), running,
/// cancel_requested, awaiting_approval, awaiting_reauth,
/// paused_awaiting_input.
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

// ---------------------------------------------------------------------------
// Render helpers (private; same shape as queries-A, owned by this module)
// ---------------------------------------------------------------------------

/// Qualify every column (`"table"."col"`) and join with `", "`, exactly
/// how Django renders a full-row select list.
fn qualified(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|col| format!("\"{table}\".\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `$1, $2, …, $n` placeholder list for an `n`-column `INSERT`.
fn placeholders(n: usize) -> String {
    (1..=n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `$start, …, $start+count-1` placeholder list for an `__in` filter.
fn in_placeholders(start: u32, count: usize) -> String {
    (0..count)
        .map(|i| format!("${}", start + i as u32))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// P1/P2/P4. Project read by id
// ---------------------------------------------------------------------------

/// P1/P2/P4 — project read (`pods.py:63,99` + the rename's
/// `pod.project` FK read at `:176`).
///
/// `Project.objects.filter(pk=project_id).first()`: `SoftDeletionManager`
/// scope + `Meta.ordering` (`-created_at`) + `LIMIT 1`. The `:63` and
/// `:99` call sites are textually identical; the `:176` forward-FK
/// access is a `.get()` on the same row (its `LIMIT 21` probe is
/// omitted per QUIRK-get-no-limit — behaviorally identical for a PK).
/// `Pod.save()`'s denorm check fires this same shape on the P4/P5
/// saves unless `pod.project` is cached (see module docs).
///
/// `$1` = project id.
///
/// Guards (handlers map these; bodies are handlers-owned): miss → 404
/// `project not found` (P1/P2); non-member of `project.workspace_id`
/// (kernel fact) → 403 (P1); non-admin → 403 `workspace admin required`
/// (P2 — note create requires ADMIN while rename/delete are
/// admin-OR-creator).
pub fn project_by_id_sql() -> String {
    format!(
        "SELECT {} FROM \"projects\" \
         WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1) \
         ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

/// Forward-FK `pod.project` read (the serializer's `project.identifier`
/// / the `Pod.save()` denorm check).
///
/// A forward FK is resolved through `Project._base_manager` — a plain
/// `models.Manager` — because `Project` sets no `base_manager_name`, so
/// `SoftDeletionManager` does NOT scope it and a tombstoned project still
/// renders (PIDASHCONV-1210; same base-manager semantics PB-0617 records
/// for the scheduler `pod_name`). Same shape as [`project_by_id_sql`] with
/// the `deleted_at` conjunct dropped (the `ORDER BY` is unobservable on a
/// PK lookup, kept for parity). `$1` = project id.
pub fn project_by_id_base_sql() -> String {
    format!(
        "SELECT {} FROM \"projects\" \
         WHERE \"projects\".\"id\" = $1 \
         ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// P1. Pod lists
// ---------------------------------------------------------------------------

/// P1 — project-mode pod list (`pods.py:70-72`).
///
/// `Pod.objects.filter(project=project).order_by("-is_default",
/// "created_at")`: `PodManager` scope + explicit ordering (== the
/// `Meta.ordering`), no limit. `?project=` wins over `?workspace=`
/// (`:56-58`); every row is serialized with its (N+1, unfiltered)
/// [`pod_runner_count_sql`] and its `project.identifier` (also N+1 —
/// not `select_related`'d; port shape, not count).
///
/// `$1` = project id.
pub fn pods_by_project_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"project_id\" = $1) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

/// P1 — workspace-mode pod list (`pods.py:83-85`).
///
/// Same shape as [`pods_by_project_sql`] over `workspace_id`
/// (back-compat path for dashboards aggregating a workspace's pods).
/// Missing `?workspace=` (and no `?project=`) → 400 `project or
/// workspace is required`; non-member → 403 (handlers-owned).
///
/// `$1` = workspace id.
pub fn pods_by_workspace_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"workspace_id\" = $1) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

/// P1 — per-pod runner count (`PodSerializer.get_runner_count`,
/// `serializers.py:71-72`: `pod.runners.count()`).
///
/// UNFILTERED — revoked runners are counted
/// (BUG-runner-count-includes-revoked, ported as-is). One query per
/// serialized pod (N+1); handlers loop it over the P1/P3 rows.
///
/// `$1` = pod id.
pub fn pod_runner_count_sql() -> String {
    format!(
        "SELECT COUNT(*) AS \"__count\" FROM \"{}\" WHERE \"{}\".\"pod_id\" = $1",
        r_cols::TABLE,
        r_cols::TABLE,
    )
}

// ---------------------------------------------------------------------------
// P2. Pod insert
// ---------------------------------------------------------------------------

/// P2 — pod insert (`pods.py:118-127`).
///
/// `Pod.objects.create(workspace_id, project, name, description,
/// created_by, is_default=False)`: plain 10-column `INSERT`,
/// `$1..$10` in [`pod_cols::COLUMNS`] order — `$1` id (fresh uuid4),
/// `$2` workspace_id (`= project.workspace_id`, so `save()`'s denorm
/// check is a no-op and the assigned project object keeps it cached —
/// no extra read), `$3` project_id, `$4` name (stripped, bare-suffix
/// re-prefixed, validator-passed), `$5` description (request value or
/// `''` — Python `or`, so `None` AND `""` both store `''`), `$6`
/// created_by_id, `$7` is_default (`FALSE` — user pods are never
/// default), `$8` deleted_at (`NULL`), `$9-10` created/updated (`now()`).
///
/// Unique violation on `pod_unique_name_per_project_when_active` is
/// NOT caught → 500 (QUIRK-create-conflict-500). Response: 201
/// serialized (handlers-owned).
pub fn pod_insert_sql() -> String {
    let cols = pod_cols::COLUMNS
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({cols}) VALUES ({})",
        pod_cols::TABLE,
        placeholders(pod_cols::COLUMNS.len()),
    )
}

// ---------------------------------------------------------------------------
// P3. Pod detail read
// ---------------------------------------------------------------------------

/// P3 — `_get_pod` detail read (`pods.py:139-145`).
///
/// `Pod.objects.filter(pk=pod_id).first()`: `PodManager` scope (miss
/// covers tombstones too) + `Meta.ordering` + `LIMIT 1`.
///
/// `$1` = pod id.
///
/// None-vs-False contract (handlers map it): miss → `None` → 404 `not
/// found`; non-member of `pod.workspace_id` (kernel fact) → `False` →
/// 403 `forbidden`; else the pod. Shared by GET/PATCH/DELETE.
pub fn pod_by_id_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// P4. Pod patch
// ---------------------------------------------------------------------------

/// P4 — default-promote sibling demote (`pods.py:192-197`), inside a tx.
///
/// `Pod.objects.filter(project_id, is_default=True).exclude(pk).update(is_default=False)`:
/// the `SET` bind comes first, then the `Q`-sorted `WHERE` — manager
/// scope, the BARE `"pod"."is_default"` conjunct
/// (QUIRK-bare-boolean), `project_id`, and the `.exclude()` as
/// `NOT (…)`.
///
/// `$1` = `FALSE`, `$2` = project id, `$3` = the promoted pod's id.
///
/// Runs only on the promote branch (`wants_default && !pod.is_default`;
/// `wants_default` is Python `bool()` — QUIRK-patch-bool). Project-
/// scoped (was workspace-scoped — the comment at `:191` says so).
pub fn pod_demote_siblings_sql() -> String {
    "UPDATE \"pod\" SET \"is_default\" = $1 \
     WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"is_default\" \
     AND \"pod\".\"project_id\" = $2 AND NOT (\"pod\".\"id\" = $3))"
        .to_string()
}

/// P4 — patch final write (`pods.py:202-203`).
///
/// `pod.save(update_fields=list(set(updates + ["updated_at"])))`, sent
/// ONLY when `updates` is non-empty. `SET` order is Django
/// model-field order regardless of the `set()` order
/// (QUIRK-patch-set-order): `name`, `description`, `is_default`,
/// `updated_at` — so the `$N` numbering depends on the flags (the
/// trailing `$N` is always the pod id):
///
/// * name only: `$1` name, `$2` updated_at, `$3` id.
/// * description only: `$1` description, `$2` updated_at, `$3` id.
/// * is_default only: `$1` is_default, `$2` updated_at, `$3` id.
/// * name + description: `$1` name, `$2` description, `$3` updated_at, `$4` id.
/// * name + is_default: `$1` name, `$2` is_default, `$3` updated_at, `$4` id.
/// * description + is_default: `$1` description, `$2` is_default, `$3` updated_at, `$4` id.
/// * all three: `$1` name, `$2` description, `$3` is_default, `$4` updated_at, `$5` id.
///
/// `save()` also runs the workspace-denorm check first (see module
/// docs): handlers run [`project_by_id_base_sql`] (a forward-FK read)
/// unless `pod.project` is cached. Rename inputs: blank (after strip) →
/// 400 `name cannot be empty`; bare suffix re-prefixed with
/// `pod.project.identifier`
/// (needs the project row); validator failure → 400 (handlers-owned).
/// Description input: `value or ""`. Demote branch (`!wants_default
/// && pod.is_default`) sets `FALSE`, leaving NO default
/// (QUIRK-demote-leaves-no-default); unchanged `is_default` is a no-op.
/// Response: 200 serialized (handlers-owned).
///
/// # Panics
///
/// Panics when all three flags are `false`: the source sends no `UPDATE`
/// in that case, so there is no faithful SQL to emit.
pub fn pod_patch_update_sql(
    update_name: bool,
    update_description: bool,
    update_is_default: bool,
) -> String {
    assert!(
        update_name || update_description || update_is_default,
        "pod_patch_update_sql: at least one field flag must be set \
         (the source sends no UPDATE when `updates` is empty)"
    );
    let mut set: Vec<String> = Vec::new();
    let mut next: u32 = 1;
    if update_name {
        set.push(format!("\"name\" = ${next}"));
        next += 1;
    }
    if update_description {
        set.push(format!("\"description\" = ${next}"));
        next += 1;
    }
    if update_is_default {
        set.push(format!("\"is_default\" = ${next}"));
        next += 1;
    }
    set.push(format!("\"updated_at\" = ${next}"));
    next += 1;
    format!(
        "UPDATE \"pod\" SET {} WHERE \"pod\".\"id\" = ${next}",
        set.join(", "),
    )
}

// ---------------------------------------------------------------------------
// P5. Pod delete: locked read + guards + stamp + sweep
// ---------------------------------------------------------------------------

/// P5 — locked pod read (`pods.py:218-225`), inside the delete tx.
///
/// [`pod_by_id_sql`] + `FOR UPDATE`: the re-read after the manage gate
/// closes the TOCTOU window (`:215-217`). Raced-delete miss → 404 `not
/// found` (handlers-owned).
///
/// `$1` = pod id.
pub fn pod_locked_read_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1 FOR UPDATE",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

/// P5 — guard 1: non-revoked runners (`pods.py:226-235`).
///
/// `locked.runners.exclude(status=REVOKED).exists()`: the reverse-FK
/// manager adds `pod_id = $1`; `.exclude()` renders `NOT (…)`; the
/// `Runner` default manager carries no scope. True → 409
/// `pod has runners; move or revoke them first` / `pod_has_runners`
/// (handlers-owned). Revoked runners keep their FK but do not block.
///
/// `$1` = pod id, `$2` = status (`'revoked'`).
pub fn pod_runners_exist_sql() -> String {
    format!(
        "SELECT 1 AS \"a\" FROM \"{t}\" WHERE (\"{t}\".\"pod_id\" = $1 AND NOT (\"{t}\".\"status\" = $2)) LIMIT 1",
        t = r_cols::TABLE,
    )
}

/// P5 — guard 2: non-terminal runs (`pods.py:236-243`).
///
/// `locked.agent_runs.filter(status__in=NON_TERMINAL_STATUSES).exists()`:
/// the reverse-FK manager adds `pod_id = $1`; the 8 pinned statuses
/// bind as `$2..$9` in [`NON_TERMINAL_STATUSES`] order. The `agent_run`
/// table carries no manager scope. True → 409
/// `pod has non-terminal runs; cancel or wait` / `pod_has_active_runs`
/// (handlers-owned).
///
/// `$1` = pod id, `$2..$9` = the pinned status values in order.
pub fn pod_active_runs_exist_sql() -> String {
    format!(
        "SELECT 1 AS \"a\" FROM \"{AGENT_RUN_TABLE}\" \
         WHERE (\"{AGENT_RUN_TABLE}\".\"{AGENT_RUN_POD_FK}\" = $1 \
         AND \"{AGENT_RUN_TABLE}\".\"{AGENT_RUN_STATUS_COL}\" IN ({})) LIMIT 1",
        in_placeholders(2, NON_TERMINAL_STATUSES.len()),
    )
}

/// P5 — soft-delete stamp (`pods.py:260-262`), inside the delete tx.
///
/// `locked.save(update_fields=["deleted_at", "is_default",
/// "updated_at"])`: model-field order is `is_default`, `deleted_at`,
/// `updated_at` (NOT the list order). `save()` also runs the
/// workspace-denorm check first (see module docs): handlers run
/// [`project_by_id_base_sql`] unless `pod.project` is cached. Guard 3
/// (`locked.is_default` → 409 `default_pod_undeletable`) is a pure
/// column read off the locked row — no SQL of its own. Guard order is
/// runners → runs → default (first hit wins).
///
/// `$1` = `FALSE`, `$2` = deleted_at (`now()`), `$3` = updated_at
/// (`now()`), `$4` = pod id.
pub fn pod_soft_delete_sql() -> String {
    "UPDATE \"pod\" SET \"is_default\" = $1, \"deleted_at\" = $2, \"updated_at\" = $3 \
     WHERE \"pod\".\"id\" = $4"
        .to_string()
}

/// P5 — `Issue.assigned_pod` sweep (`pods.py:263-266`), inside the tx.
///
/// `Issue.objects.filter(assigned_pod=locked).update(assigned_pod=None)`:
/// `SET … = NULL` renders literally (no bind). `Issue.objects` is the
/// inherited `SoftDeletionManager`, so tombstoned issues are NOT swept
/// (fixture deviation — the fixture omits the scope).
///
/// `$1` = pod id. `COMMIT`; response 204 empty (handlers-owned).
pub fn issue_assigned_pod_clear_sql() -> String {
    format!(
        "UPDATE \"{ISSUES_TABLE}\" SET \"{ISSUES_ASSIGNED_POD_FK}\" = NULL \
         WHERE (\"{ISSUES_TABLE}\".\"deleted_at\" IS NULL \
         AND \"{ISSUES_TABLE}\".\"{ISSUES_ASSIGNED_POD_FK}\" = $1)"
    )
}

// ---------------------------------------------------------------------------
// J1. Projects serialize
// ---------------------------------------------------------------------------

/// J1 — workspace pod-values query (`projects.py:46-50`).
///
/// `Pod.objects.filter(workspace_id).values("project_id", "is_default",
/// "id", "name").order_by("-is_default", "name")`: the select list is
/// in `.values()` order (NOT table order); ordering is default-first,
/// then name. No limit.
///
/// `$1` = workspace id.
///
/// Handlers-E assembly rule (quoted so it is ported faithfully):
/// rows group by `project_id` preserving row order; each entry is
/// `{id: str, name, is_default: bool}`; `default_pod_id` is the FIRST
/// row with `is_default` per project (`if row["is_default"] and pid
/// not in default_pod_ids` — a second default row does NOT overwrite);
/// `pod_count` is the group length. Projects without pods get
/// `default_pod_id: None`, `pod_count: 0`, `pods: []`.
pub fn workspace_pods_query(workspace_id: Uuid) -> Select<pod::Entity> {
    pod::Entity::find()
        .select_only()
        .columns([
            pod::Column::ProjectId,
            pod::Column::IsDefault,
            pod::Column::Id,
            pod::Column::Name,
        ])
        .filter(pod::Column::DeletedAt.is_null())
        .filter(pod::Column::WorkspaceId.eq(workspace_id))
        .order_by_desc(pod::Column::IsDefault)
        .order_by_asc(pod::Column::Name)
}

/// [`workspace_pods_query`] rows: `(project_id, is_default, id, name)`.
pub async fn workspace_pods(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
) -> Result<Vec<(Uuid, bool, Uuid, String)>, DbErr> {
    workspace_pods_query(workspace_id)
        .into_tuple()
        .all(db)
        .await
}

/// J1 — workspace project query (`projects.py:74-76`).
///
/// `Project.objects.filter(workspace_id).order_by("identifier")`:
/// `SoftDeletionManager` scope, identifier order, no limit; only the five
/// columns the serializer reads are selected. Each row becomes `{id: str,
/// identifier, name, description, is_default, default_pod_id, pod_count,
/// pods}` (handlers-owned).
pub fn workspace_projects_query(workspace_id: Uuid) -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::Name,
            projects::Column::Description,
            projects::Column::Identifier,
            projects::Column::IsDefault,
        ])
        .filter(projects::Column::DeletedAt.is_null())
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .order_by_asc(projects::Column::Identifier)
}

/// The project columns `_serialize_projects` reads.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub identifier: String,
    pub is_default: bool,
}

pub async fn workspace_projects(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
) -> Result<Vec<ProjectSummary>, DbErr> {
    workspace_projects_query(workspace_id)
        .into_model::<ProjectSummary>()
        .all(db)
        .await
}

// ---------------------------------------------------------------------------
// J2. Projects auth-mode scoping inputs
// ---------------------------------------------------------------------------

/// J2 — `?workspace=` membership probe (`projects.py:109-111`).
///
/// `WorkspaceMember.objects.filter(workspace_id=ws,
/// member=user).exists()`: RAW filter — NO `is_active` conjunct
/// (BUG-membership-no-active, ported as-is) — but the
/// `SoftDeletionManager` scope DOES apply.
///
/// False → 403 `forbidden` (handlers-owned). Anonymous (either
/// mode 2/3 path) → 401 `authentication required` before any SQL.
/// Mode 1 (runner access-token, `request.auth_runner` set) skips all
/// of J2 and serializes `runner.workspace_id` directly.
pub fn workspace_membership_query(
    member_id: Uuid,
    workspace_id: Uuid,
) -> Select<workspace_members::Entity> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Id)
        .filter(workspace_members::Column::DeletedAt.is_null())
        .filter(workspace_members::Column::MemberId.eq(member_id))
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .limit(1)
}

pub async fn is_workspace_member(
    db: &impl ConnectionTrait,
    member_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = workspace_membership_query(member_id, workspace_id)
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// J2 — caller's membership workspace ids (`projects.py:116-120`).
///
/// `WorkspaceMember.objects.filter(member=user).values_list("workspace_id",
/// flat=True)`: same raw filter (no `is_active` —
/// BUG-membership-no-active), manager scope applies, and
/// `Meta.ordering` (`-created_at`) is KEPT (fixture deviation — the
/// fixture spells "no ORDER BY"). No limit. Handlers serialize EVERY
/// listed workspace and concatenate.
pub fn member_workspace_ids_query(member_id: Uuid) -> Select<workspace_members::Entity> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::WorkspaceId)
        .filter(workspace_members::Column::DeletedAt.is_null())
        .filter(workspace_members::Column::MemberId.eq(member_id))
        .order_by_desc(workspace_members::Column::CreatedAt)
}

pub async fn member_workspace_ids(
    db: &impl ConnectionTrait,
    member_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    member_workspace_ids_query(member_id)
        .into_tuple()
        .all(db)
        .await
}

// ---------------------------------------------------------------------------
// K1. Desktop enroll: bundled-machine lookup + create-or-touch
// ---------------------------------------------------------------------------

/// SeaORM form of [`bundled_machine_lookup_sql`], run on the enroll
/// transaction (`TxConn`): the newest unrevoked bundled machine of this
/// owner + host with a token in the workspace, row-locked (`FOR UPDATE`
/// over the join, as Django's `select_for_update()` locks every joined
/// row). Only the id is read.
pub async fn bundled_machine_for_update(
    db: &impl ConnectionTrait,
    host_label: &str,
    workspace_id: Uuid,
    owner_id: Uuid,
    provisioning: &str,
) -> Result<Option<Uuid>, DbErr> {
    bundled_machine_lookup_query(host_label, workspace_id, owner_id, provisioning)
        .into_tuple()
        .one(db)
        .await
}

pub fn bundled_machine_lookup_query(
    host_label: &str,
    workspace_id: Uuid,
    owner_id: Uuid,
    provisioning: &str,
) -> Select<dev_machine::Entity> {
    // The entities omit dev_machine -> machine_token (see entity/mod.rs);
    // the join is spelled here: `dev_machine.id = machine_token.dev_machine_id`.
    let tokens: RelationDef = dev_machine::Entity::belongs_to(machine_token::Entity)
        .from(dev_machine::Column::Id)
        .to(machine_token::Column::DevMachineId)
        .into();
    dev_machine::Entity::find()
        .select_only()
        .column(dev_machine::Column::Id)
        .join(JoinType::InnerJoin, tokens)
        .filter(dev_machine::Column::HostLabel.eq(host_label))
        .filter(machine_token::Column::WorkspaceId.eq(workspace_id))
        .filter(dev_machine::Column::OwnerId.eq(owner_id))
        .filter(dev_machine::Column::Provisioning.eq(provisioning))
        .filter(dev_machine::Column::RevokedAt.is_null())
        .order_by_desc(dev_machine::Column::CreatedAt)
        .limit(1)
        .lock_exclusive()
}

/// `DevMachine.objects.create(owner, host_label, label, provisioning,
/// last_seen_at)` for the desktop enroll (`desktop.py:121-127`):
/// visibility `0`, no revocation, every timestamp `now`.
pub async fn insert_dev_machine(
    db: &impl ConnectionTrait,
    id: Uuid,
    owner_id: Uuid,
    host_label: &str,
    provisioning: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    dev_machine::Entity::insert(dev_machine::ActiveModel {
        id: Set(id),
        owner_id: Set(owner_id),
        host_label: Set(host_label.to_owned()),
        label: Set(dev_machine_label(host_label)),
        visibility: Set(0),
        provisioning: Set(provisioning.to_owned()),
        last_seen_at: Set(Some(now.into())),
        revoked_at: Set(None),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    })
    .exec_without_returning(db)
    .await
    .map(drop)
}

/// `dev_machine.save(update_fields=["last_seen_at", "updated_at"])`
/// (`desktop.py:128-130`).
pub async fn touch_dev_machine(
    db: &impl ConnectionTrait,
    id: Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    dev_machine::Entity::update_many()
        .col_expr(dev_machine::Column::LastSeenAt, Expr::value(now))
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now))
        .filter(dev_machine::Column::Id.eq(id))
        .exec(db)
        .await
        .map(drop)
}

/// SeaORM form of [`bundled_machine_ids_sql`]: the owner's unrevoked
/// bundled machine ids, optionally for one host, in `Meta.ordering`.
pub fn bundled_machine_ids_query(
    owner_id: Uuid,
    provisioning: &str,
    host_label: Option<&str>,
) -> Select<dev_machine::Entity> {
    let query = dev_machine::Entity::find()
        .select_only()
        .column(dev_machine::Column::Id)
        .filter(dev_machine::Column::OwnerId.eq(owner_id))
        .filter(dev_machine::Column::Provisioning.eq(provisioning))
        .filter(dev_machine::Column::RevokedAt.is_null());
    match host_label {
        Some(host) => query.filter(dev_machine::Column::HostLabel.eq(host)),
        None => query,
    }
    .order_by_desc(dev_machine::Column::LastSeenAt)
    .order_by_desc(dev_machine::Column::CreatedAt)
}

pub async fn bundled_machine_ids(
    db: &impl ConnectionTrait,
    owner_id: Uuid,
    provisioning: &str,
    host_label: Option<&str>,
) -> Result<Vec<Uuid>, DbErr> {
    bundled_machine_ids_query(owner_id, provisioning, host_label)
        .into_tuple()
        .all(db)
        .await
}

/// SeaORM form of [`signout_offline_runners_sql`]: non-revoked bundled
/// runners on these machines go offline (the rows survive).
pub async fn signout_offline_runners(
    db: &impl ConnectionTrait,
    machine_ids: &[Uuid],
    provisioning: &str,
    offline: &str,
    revoked: &str,
) -> Result<(), DbErr> {
    runner::Entity::update_many()
        .col_expr(runner::Column::Status, Expr::value(offline))
        .filter(runner::Column::DevMachineId.is_in(machine_ids.iter().copied()))
        .filter(runner::Column::Provisioning.eq(provisioning))
        .filter(runner::Column::Status.ne(revoked))
        .exec(db)
        .await
        .map(drop)
}

/// `Workspace.objects.filter(slug).first()` for the desktop enroll
/// (`desktop.py:100-103`): live workspace, `Meta.ordering` `-created_at`.
pub async fn workspace_by_slug(
    db: &impl ConnectionTrait,
    slug: &str,
) -> Result<Option<(Uuid, String)>, DbErr> {
    workspaces::Entity::find()
        .select_only()
        .columns([workspaces::Column::Id, workspaces::Column::Slug])
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .order_by_desc(workspaces::Column::CreatedAt)
        .into_tuple()
        .one(db)
        .await
}

/// `is_workspace_member` (`core/permissions.py:28-34`): like
/// [`is_workspace_member`] plus `is_active=True`.
pub async fn is_active_workspace_member(
    db: &impl ConnectionTrait,
    member_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = workspace_membership_query(member_id, workspace_id)
        .filter(workspace_members::Column::IsActive.eq(true))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// K2. Desktop enroll: token rotate + mint
// ---------------------------------------------------------------------------

// NOTE: the K2 rotate-revoke and mint-insert builders lived here; the
// statements moved to `crate::domain::tokens`
// (`revoke_tokens_for_machine_in_workspace`, `insert_machine_token`;
// PIDASHCONV-1520), whose tests carry the pins. Shape notes for the
// record: `select_for_update()` is a no-op on updates, so the revoke
// is a plain `UPDATE` with the `SET` bind first; one live token per
// (machine, workspace) — rotate, don't append.

// ---------------------------------------------------------------------------
// K3. Desktop delete (sign-out)
// ---------------------------------------------------------------------------

// NOTE: the K3 sign-out revoke builder lived here; the statement moved
// to `crate::domain::tokens::revoke_tokens_for_dev_machines`
// (PIDASHCONV-1520), whose tests carry the pins (including the
// empty-list panic).

// ---------------------------------------------------------------------------
// Bind helpers (pure; the SQL above names the slots, these compute values)
// ---------------------------------------------------------------------------

/// Python `s[:n]` over code points. Never panics on a UTF-8 boundary
/// (Porting guide semantic traps: byte slicing would).
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// Desktop host label (`desktop.py:84,169`):
/// `(value or "").strip()[:255]`. The caller passes `""` for a missing
/// value (the `or ""`); blank-after-strip → 400 is handlers-owned.
/// Twin of the queries-A D-path helper (identical semantics).
pub fn normalize_host_label(raw: &str) -> String {
    truncate_chars(py_strip(raw), 255)
}

/// Pod name strip (`pods.py:92,166`): `(value or "").strip()` — with
/// NO `[:N]` truncation (port as-is; `validate_user_pod_name`
/// enforces the length). The caller passes `""` for a missing value.
pub fn strip_pod_name(raw: &str) -> String {
    py_strip(raw).to_string()
}

/// Bare-suffix convenience (`pods.py:113-114,177-178`): when the
/// stripped name lacks the `{identifier}_` prefix, re-prefix it
/// server-side before validation. Takes the already-stripped name.
pub fn prefixed_pod_name(stripped_name: &str, project_identifier: &str) -> String {
    let prefix = format!("{project_identifier}_");
    if stripped_name.starts_with(&prefix) {
        stripped_name.to_string()
    } else {
        format!("{prefix}{stripped_name}")
    }
}

/// K1 machine label (`desktop.py:124`): `host_label[:128]` (of the
/// already-normalized label). Twin of the queries-A D2 helper.
pub fn dev_machine_label(normalized_host_label: &str) -> String {
    truncate_chars(normalized_host_label, 128)
}

/// K2 mint label (`desktop.py:147`): `f"desktop: {host_label[:88]}"`.
/// The slice applies to the host label BEFORE the prefix is added.
pub fn desktop_token_label(host_label: &str) -> String {
    format!("desktop: {}", truncate_chars(host_label, 88))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::runner_enroll::columns::dev_machine as dm_cols;
    use pidash_db::runner_enroll::columns::{enums, machine_token as mt_cols};

    const FIXTURE_SQL: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/pods_projects_desktop.sql");
    const FIXTURE_ROWS: &str = include_str!(
        "../../../../../fixtures/runner_enroll/queries/pods_projects_desktop.rows.json"
    );
    const FIXTURE_WIRE: &str =
        include_str!("../../../../../fixtures/runner_enroll/external/wire_pins.json");

    /// Collapse every whitespace run to one space.
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Assert `needle` (squashed) is recorded in the D13-F5 SQL fixture,
    /// and return it. Every `assert_builder_contains` below goes through
    /// here so a stale needle fails loudly at the fixture end, not
    /// silently at the builder end.
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
    fn assert_suffix(builder_sql: &str, suffix: &str) -> String {
        let (list, rest) = builder_sql
            .strip_prefix("SELECT ")
            .and_then(|s| s.find(" FROM ").map(|i| (&s[..i], &s[i + 1..])))
            .expect("builder must emit SELECT <list> FROM …");
        assert_eq!(rest, suffix, "FROM/WHERE/ORDER/LIMIT suffix");
        list.to_string()
    }

    fn expected_list(table: &str, cols: &[&str]) -> String {
        cols.iter()
            .map(|c| format!("\"{table}\".\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    // -- Oracle column order (transcribed from SQL compiled on Django 4.2.30) --

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
    const MACHINE_COLS: &[&str] = &[
        "id",
        "owner_id",
        "host_label",
        "label",
        "visibility",
        "provisioning",
        "last_seen_at",
        "revoked_at",
        "created_at",
        "updated_at",
    ];
    const TOKEN_COLS: &[&str] = &[
        "id",
        "user_id",
        "dev_machine_id",
        "workspace_id",
        "host_label",
        "token_hash",
        "token_fingerprint",
        "label",
        "is_service",
        "created_at",
        "last_used_at",
        "revoked_at",
    ];

    #[test]
    fn reused_column_consts_match_compiled_order() {
        // The builders reuse db-crate consts; pin those consts to the
        // independently compiled Django column order so a drift fails here.
        assert_eq!(pod_cols::COLUMNS, POD_COLS);
        assert_eq!(pod_cols::TABLE, "pod");
        assert_eq!(dm_cols::COLUMNS, MACHINE_COLS);
        assert_eq!(dm_cols::TABLE, "dev_machine");
        assert_eq!(mt_cols::COLUMNS, TOKEN_COLS);
        assert_eq!(mt_cols::TABLE, "machine_token");
        assert_eq!(r_cols::TABLE, "runner");
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
        // Own cross-domain table pins.
        assert_eq!(AGENT_RUN_TABLE, "agent_run");
        assert_eq!(AGENT_RUN_POD_FK, "pod_id");
        assert_eq!(AGENT_RUN_STATUS_COL, "status");
        assert_eq!(ISSUES_TABLE, "issues");
        assert_eq!(ISSUES_ASSIGNED_POD_FK, "assigned_pod_id");
        // Enum values the builders' $N slots bind.
        assert_eq!(enums::RUNNER_STATUS_REVOKED, "revoked");
        assert_eq!(enums::RUNNER_STATUS_OFFLINE, "offline");
        assert_eq!(
            enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED,
            "desktop_bundled"
        );
    }

    // -- P1/P2/P4 project read --

    #[test]
    fn project_read_scopes_orders_limits() {
        let sql = project_by_id_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1) \
             ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        );
        assert_eq!(list, expected_list("projects", project_cols::COLUMNS));
        for fragment in [
            fixture_fragment("FROM \"projects\""),
            fixture_fragment("\"id\" = $1"),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn project_read_base_has_no_soft_delete_scope() {
        // Forward-FK `pod.project` read: `_base_manager` (no scope), so a
        // tombstoned project still renders (PIDASHCONV-1210). Otherwise the
        // shape matches the scoped read.
        let sql = project_by_id_base_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"projects\" WHERE \"projects\".\"id\" = $1 \
             ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        );
        assert_eq!(list, expected_list("projects", project_cols::COLUMNS));
        assert!(
            !sql.contains("\"deleted_at\" IS NULL"),
            "base read must not filter tombstones:\n{sql}"
        );
    }

    // -- P1 lists + count --

    #[test]
    fn pod_lists_order_default_first_then_created() {
        for (sql, fk) in [
            (pods_by_project_sql(), "project_id"),
            (pods_by_workspace_sql(), "workspace_id"),
        ] {
            let list = assert_suffix(
                &sql,
                &format!(
                    "FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"{fk}\" = $1) \
                     ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC"
                ),
            );
            assert_eq!(list, expected_list("pod", POD_COLS));
        }
        for (sql, fk) in [
            (pods_by_project_sql(), "\"project_id\" = $1"),
            (pods_by_workspace_sql(), "\"workspace_id\" = $1"),
        ] {
            for fragment in [
                fixture_fragment(fk),
                fixture_fragment("\"is_default\" DESC"),
                fixture_fragment("\"created_at\" ASC"),
            ] {
                assert_builder_contains(&sql, &fragment);
            }
        }
    }

    #[test]
    fn runner_count_is_unfiltered() {
        let sql = pod_runner_count_sql();
        assert_eq!(
            sql,
            "SELECT COUNT(*) AS \"__count\" FROM \"runner\" WHERE \"runner\".\"pod_id\" = $1"
        );
        for fragment in [
            fixture_fragment("COUNT(*)"),
            fixture_fragment("FROM \"runner\""),
            fixture_fragment("\"pod_id\" = "),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
        // BUG-runner-count-includes-revoked: no status conjunct, by design.
        assert!(!sql.contains("status"));
        assert!(!sql.contains("revoked"));
    }

    // -- P2 insert --

    #[test]
    fn pod_insert_binds_all_columns_in_order() {
        let sql = pod_insert_sql();
        assert_eq!(
            sql,
            "INSERT INTO \"pod\" (\"id\", \"workspace_id\", \"project_id\", \"name\", \
             \"description\", \"created_by_id\", \"is_default\", \"deleted_at\", \
             \"created_at\", \"updated_at\") \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        );
        for fragment in [
            fixture_fragment("INSERT INTO \"pod\""),
            fixture_fragment(
                "\"id\", \"workspace_id\", \"project_id\", \"name\", \"description\",",
            ),
            fixture_fragment("VALUES ($1, $2, $3, $4, $5, $6"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- P3 --

    #[test]
    fn pod_detail_scopes_orders_limits() {
        let sql = pod_by_id_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
             ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1",
        );
        assert_eq!(list, expected_list("pod", POD_COLS));
        for fragment in [
            fixture_fragment("FROM \"pod\""),
            fixture_fragment("\"id\" = $1"),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- P4 --

    #[test]
    fn demote_siblings_renders_bare_boolean_and_not() {
        let sql = pod_demote_siblings_sql();
        assert_eq!(
            squashed(&sql),
            "UPDATE \"pod\" SET \"is_default\" = $1 WHERE (\"pod\".\"deleted_at\" IS NULL \
             AND \"pod\".\"is_default\" AND \"pod\".\"project_id\" = $2 \
             AND NOT (\"pod\".\"id\" = $3))"
        );
        for fragment in [
            fixture_fragment("UPDATE \"pod\" SET \"is_default\" = "),
            // Fixture numbers the WHERE binds from $1; Django numbers the
            // SET bind first, so the builder binds project_id as $2.
            fixture_fragment("\"project_id\" = "),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn patch_update_emits_model_order_for_every_subset() {
        // Model order (name, description, is_default, updated_at) for all
        // seven non-empty subsets; the trailing bind is always the pod id.
        let cases = [
            (
                (true, false, false),
                "UPDATE \"pod\" SET \"name\" = $1, \"updated_at\" = $2 WHERE \"pod\".\"id\" = $3",
            ),
            (
                (false, true, false),
                "UPDATE \"pod\" SET \"description\" = $1, \"updated_at\" = $2 WHERE \"pod\".\"id\" = $3",
            ),
            (
                (false, false, true),
                "UPDATE \"pod\" SET \"is_default\" = $1, \"updated_at\" = $2 WHERE \"pod\".\"id\" = $3",
            ),
            (
                (true, true, false),
                "UPDATE \"pod\" SET \"name\" = $1, \"description\" = $2, \"updated_at\" = $3 \
                 WHERE \"pod\".\"id\" = $4",
            ),
            (
                (true, false, true),
                "UPDATE \"pod\" SET \"name\" = $1, \"is_default\" = $2, \"updated_at\" = $3 \
                 WHERE \"pod\".\"id\" = $4",
            ),
            (
                (false, true, true),
                "UPDATE \"pod\" SET \"description\" = $1, \"is_default\" = $2, \"updated_at\" = $3 \
                 WHERE \"pod\".\"id\" = $4",
            ),
            (
                (true, true, true),
                "UPDATE \"pod\" SET \"name\" = $1, \"description\" = $2, \"is_default\" = $3, \
                 \"updated_at\" = $4 WHERE \"pod\".\"id\" = $5",
            ),
        ];
        for ((name, desc, default), expected) in cases {
            assert_eq!(
                squashed(&pod_patch_update_sql(name, desc, default)),
                squashed(expected),
                "flags ({name}, {desc}, {default})"
            );
        }
        for fragment in [
            fixture_fragment("UPDATE \"pod\" SET"),
            fixture_fragment("\"updated_at\" = "),
        ] {
            assert_builder_contains(&pod_patch_update_sql(true, true, true), &fragment);
        }
    }

    #[test]
    #[should_panic(expected = "at least one field flag must be set")]
    fn patch_update_panics_when_updates_empty() {
        let _ = pod_patch_update_sql(false, false, false);
    }

    // -- P5 --

    #[test]
    fn locked_read_is_detail_plus_for_update() {
        let sql = pod_locked_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) \
             ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("pod", POD_COLS));
        assert_builder_contains(&sql, &fixture_fragment("LIMIT 1 FOR UPDATE"));
    }

    #[test]
    fn guard_runners_excludes_revoked_with_not() {
        let sql = pod_runners_exist_sql();
        assert_eq!(
            sql,
            "SELECT 1 AS \"a\" FROM \"runner\" \
             WHERE (\"runner\".\"pod_id\" = $1 AND NOT (\"runner\".\"status\" = $2)) LIMIT 1"
        );
        for fragment in [
            fixture_fragment("FROM \"runner\""),
            fixture_fragment("\"pod_id\" = $1"),
            fixture_fragment("\"status\""),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn guard_runs_binds_eight_pinned_statuses() {
        let sql = pod_active_runs_exist_sql();
        assert_eq!(
            sql,
            "SELECT 1 AS \"a\" FROM \"agent_run\" \
             WHERE (\"agent_run\".\"pod_id\" = $1 \
             AND \"agent_run\".\"status\" IN ($2, $3, $4, $5, $6, $7, $8, $9)) LIMIT 1"
        );
        for fragment in [
            fixture_fragment("FROM \"agent_run\""),
            fixture_fragment("\"pod_id\" = $1"),
            fixture_fragment("\"status\" IN"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn status_pin_matches_fixture_f8() {
        // D13-F8 `non_terminal_statuses.value` (wire_pins.json) is the
        // pinned matcher tuple; the retired entry carries an annotation
        // the const must strip.
        let wire: serde_json::Value = serde_json::from_str(FIXTURE_WIRE).unwrap();
        assert_eq!(wire["fixture_id"], "D13-F8");
        let pinned: Vec<&str> = wire["non_terminal_statuses"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().split(" (").next().unwrap())
            .collect();
        assert_eq!(pinned, NON_TERMINAL_STATUSES);
        assert_eq!(NON_TERMINAL_STATUSES.len(), 8);
        // And the guard binds exactly those eight, in that order.
        let sql = pod_active_runs_exist_sql();
        for (i, status) in NON_TERMINAL_STATUSES.iter().enumerate() {
            let _ = status;
            assert!(
                sql.contains(&format!("${}", i + 2)),
                "missing bind for status #{i}:\n{sql}"
            );
        }
    }

    #[test]
    fn soft_delete_stamp_orders_model_fields() {
        let sql = pod_soft_delete_sql();
        assert_eq!(
            squashed(&sql),
            "UPDATE \"pod\" SET \"is_default\" = $1, \"deleted_at\" = $2, \"updated_at\" = $3 \
             WHERE \"pod\".\"id\" = $4"
        );
        for fragment in [
            fixture_fragment("UPDATE \"pod\" SET"),
            fixture_fragment("\"deleted_at\" = "),
            fixture_fragment("\"is_default\" = "),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn issue_sweep_scopes_live_rows_and_nulls_fk() {
        let sql = issue_assigned_pod_clear_sql();
        assert_eq!(
            squashed(&sql),
            "UPDATE \"issues\" SET \"assigned_pod_id\" = NULL \
             WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"assigned_pod_id\" = $1)"
        );
        for fragment in [
            fixture_fragment("UPDATE \"issues\" SET \"assigned_pod_id\" = NULL"),
            fixture_fragment("\"assigned_pod_id\" = $1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- J1 / J2 (SeaORM) --

    fn built(query: impl sea_orm::QueryTrait) -> String {
        query.build(sea_orm::DbBackend::Postgres).to_string()
    }

    #[test]
    fn workspace_pods_keep_values_order_default_first() {
        let sql = built(workspace_pods_query(Uuid::nil()));
        // `.values()` order, NOT table order: the tuple decodes by it.
        assert!(
            sql.starts_with(
                r#"SELECT "pod"."project_id", "pod"."is_default", "pod"."id", "pod"."name" FROM "pod""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""pod"."deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "pod"."is_default" DESC, "pod"."name" ASC"#),
            "{sql}"
        );
    }

    #[test]
    fn workspace_projects_order_by_identifier() {
        let sql = built(workspace_projects_query(Uuid::nil()));
        assert!(sql.contains(r#""projects"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""projects"."workspace_id" = '"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "projects"."identifier" ASC"#),
            "{sql}"
        );
    }

    #[test]
    fn membership_probe_has_no_active_filter() {
        let sql = built(workspace_membership_query(Uuid::nil(), Uuid::nil()));
        assert!(
            sql.contains(r#""workspace_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""workspace_members"."member_id" = "#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""workspace_members"."workspace_id" = "#),
            "{sql}"
        );
        assert!(sql.ends_with("LIMIT 1"), "{sql}");
        // BUG-membership-no-active: raw filter, no is_active conjunct.
        assert!(!sql.contains("is_active"), "{sql}");
    }

    #[test]
    fn member_workspace_ids_keep_default_ordering() {
        let sql = built(member_workspace_ids_query(Uuid::nil()));
        assert!(
            sql.starts_with(
                r#"SELECT "workspace_members"."workspace_id" FROM "workspace_members""#
            ),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "workspace_members"."created_at" DESC"#),
            "{sql}"
        );
        assert!(!sql.contains("is_active"), "{sql}");
    }

    /// The four J1/J2 reads against a seeded world: soft-deleted rows are
    /// out, membership ignores `is_active`, pods come default-first.
    #[tokio::test]
    async fn live_projects_serialize_reads() {
        use sea_orm::{ActiveModelTrait, Set};
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let world = crate::read::test_support::seed_world(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let now = chrono::Utc::now();
        let empty = || serde_json::json!({});
        workspace_members::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            role: Set(20),
            member_id: Set(world.user_id),
            workspace_id: Set(world.workspace_id),
            view_props: Set(empty()),
            default_props: Set(empty()),
            issue_props: Set(empty()),
            is_active: Set(false),
            explored_features: Set(empty()),
            getting_started_checklist: Set(empty()),
            tips: Set(empty()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("member");
        let pod = |name: &str, is_default: bool, deleted: bool| pod::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(name.to_owned()),
            description: Set(String::new()),
            is_default: Set(is_default),
            deleted_at: Set(deleted.then(|| now.into())),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            workspace_id: Set(world.workspace_id),
            project_id: Set(world.project_id),
            ..Default::default()
        };
        let a = pod("a-pod", false, false).insert(&db).await.expect("pod a");
        let z = pod("z-pod", true, false).insert(&db).await.expect("pod z");
        pod("gone", false, true)
            .insert(&db)
            .await
            .expect("deleted pod");

        // Inactive membership still counts (BUG-membership-no-active).
        assert!(is_workspace_member(&db, world.user_id, world.workspace_id)
            .await
            .expect("probe"));
        assert!(
            !is_workspace_member(&db, Uuid::new_v4(), world.workspace_id)
                .await
                .expect("probe")
        );
        assert_eq!(
            member_workspace_ids(&db, world.user_id).await.expect("ids"),
            vec![world.workspace_id]
        );
        assert_eq!(
            workspace_pods(&db, world.workspace_id).await.expect("pods"),
            vec![
                (world.project_id, true, z.id, "z-pod".to_owned()),
                (world.project_id, false, a.id, "a-pod".to_owned()),
            ]
        );
        let projects = workspace_projects(&db, world.workspace_id)
            .await
            .expect("projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, world.project_id);
        assert_eq!(projects[0].identifier, world.project_identifier);
    }

    // -- K1 --

    #[test]
    fn bundled_lookup_joins_token_and_locks() {
        let sql = built(bundled_machine_lookup_query(
            "host",
            Uuid::nil(),
            Uuid::nil(),
            "desktop_bundled",
        ));
        assert!(
            sql.starts_with(r#"SELECT "dev_machine"."id" FROM "dev_machine""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"INNER JOIN "machine_token" ON "dev_machine"."id" = "machine_token"."dev_machine_id""#
            ),
            "{sql}"
        );
        for fragment in [
            r#""dev_machine"."host_label" = 'host'"#,
            r#""machine_token"."workspace_id" = "#,
            r#""dev_machine"."owner_id" = "#,
            r#""dev_machine"."provisioning" = 'desktop_bundled'"#,
            r#""dev_machine"."revoked_at" IS NULL"#,
        ] {
            assert!(sql.contains(fragment), "{fragment}: {sql}");
        }
        assert!(
            sql.ends_with(r#"ORDER BY "dev_machine"."created_at" DESC LIMIT 1 FOR UPDATE"#),
            "{sql}"
        );
        // QUIRK-desktop-join-ignores-token-revoked: the joined table
        // carries no revoked_at conjunct.
        assert!(!sql.contains(r#""machine_token"."revoked_at""#), "{sql}");
    }

    #[test]
    fn bundled_ids_keep_ordering_with_host_last() {
        let plain = built(bundled_machine_ids_query(
            Uuid::nil(),
            "desktop_bundled",
            None,
        ));
        assert!(!plain.contains("host_label"), "{plain}");
        let hosted = built(bundled_machine_ids_query(
            Uuid::nil(),
            "desktop_bundled",
            Some("h"),
        ));
        assert!(
            hosted.contains(
                r#""dev_machine"."revoked_at" IS NULL AND "dev_machine"."host_label" = 'h'"#
            ),
            "{hosted}"
        );
        for sql in [plain, hosted] {
            assert!(
                sql.ends_with(
                    r#"ORDER BY "dev_machine"."last_seen_at" DESC, "dev_machine"."created_at" DESC"#
                ),
                "{sql}"
            );
        }
    }

    /// The desktop enroll and sign-out writes on one sqlx transaction
    /// through `TxConn`: insert, locked lookup through the token join,
    /// touch, id list, and the offline sweep that spares revoked runners.
    #[tokio::test]
    async fn live_desktop_machine_flow() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let world = crate::read::test_support::seed_world(&pool).await;
        let mut tx = pool.begin().await.expect("begin");
        let machine = Uuid::new_v4();
        let then = chrono::Utc::now() - chrono::Duration::hours(1);
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            insert_dev_machine(
                &conn,
                machine,
                world.user_id,
                "host-1",
                "desktop_bundled",
                then,
            )
            .await
            .expect("insert");
            // No token in the workspace yet: the join finds nothing.
            assert_eq!(
                bundled_machine_for_update(
                    &conn,
                    "host-1",
                    world.workspace_id,
                    world.user_id,
                    "desktop_bundled"
                )
                .await
                .expect("lookup"),
                None
            );
        }
        sqlx::query(
            "INSERT INTO machine_token (id, user_id, dev_machine_id, workspace_id, host_label, \
             token_hash, token_fingerprint, label, is_service, created_at, last_used_at, revoked_at) \
             VALUES ($1, $2, $3, $4, 'host-1', $5, '', '', true, now(), NULL, now())",
        )
        .bind(Uuid::new_v4())
        .bind(world.user_id)
        .bind(machine)
        .bind(world.workspace_id)
        .bind(Uuid::new_v4().to_string())
        .execute(&mut *tx)
        .await
        .expect("revoked token");
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            // QUIRK-desktop-join-ignores-token-revoked: a revoked token still joins.
            assert_eq!(
                bundled_machine_for_update(
                    &conn,
                    "host-1",
                    world.workspace_id,
                    world.user_id,
                    "desktop_bundled"
                )
                .await
                .expect("lookup"),
                Some(machine)
            );
            let now = chrono::Utc::now();
            touch_dev_machine(&conn, machine, now).await.expect("touch");
            let touched = dev_machine::Entity::find_by_id(machine)
                .one(&conn)
                .await
                .expect("read")
                .expect("row");
            assert!(touched.last_seen_at.expect("seen") > then);
            assert_eq!(
                bundled_machine_ids(&conn, world.user_id, "desktop_bundled", Some("host-1"))
                    .await
                    .expect("ids"),
                vec![machine]
            );
            assert!(
                bundled_machine_ids(&conn, world.user_id, "desktop_bundled", Some("other"))
                    .await
                    .expect("ids")
                    .is_empty()
            );
        }
        tx.rollback().await.expect("rollback");
    }

    #[test]
    fn host_label_strips_and_truncates() {
        assert_eq!(normalize_host_label("  mbp  "), "mbp");
        assert_eq!(normalize_host_label(""), "");
        assert_eq!(normalize_host_label("   "), "");
        // Python-strip-only whitespace also normalizes away.
        assert_eq!(normalize_host_label("\u{85} \u{1c}"), "");
        // 300-char label truncates to the stored 255 width.
        assert_eq!(normalize_host_label(&"z".repeat(300)).len(), 255);
        // Never panics on a UTF-8 boundary.
        assert_eq!(truncate_chars("ééé", 2), "éé");
    }

    #[test]
    fn pod_name_strips_without_truncating() {
        assert_eq!(strip_pod_name("  WEB_beefy  "), "WEB_beefy");
        assert_eq!(strip_pod_name(""), "");
        // NO [:N] slice — a 200-char name survives for the validator.
        assert_eq!(strip_pod_name(&"a".repeat(200)).len(), 200);
        assert_eq!(prefixed_pod_name("beefy", "WEB"), "WEB_beefy");
        assert_eq!(prefixed_pod_name("WEB_beefy", "WEB"), "WEB_beefy");
        // Prefix match is on the full `{identifier}_`, not a substring.
        assert_eq!(prefixed_pod_name("WEB2_x", "WEB"), "WEB_WEB2_x");
    }

    #[test]
    fn labels_slice_before_prefixing() {
        assert_eq!(dev_machine_label("mbp"), "mbp");
        assert_eq!(dev_machine_label(&"z".repeat(200)).len(), 128);
        assert_eq!(desktop_token_label("mbp"), "desktop: mbp");
        // [:88] applies to the host label BEFORE the prefix is added.
        assert_eq!(
            desktop_token_label(&"h".repeat(100)),
            format!("desktop: {}", "h".repeat(88))
        );
    }

    // -- Rows fixture --

    #[test]
    fn fixture_rows_shape_is_as_documented() {
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert_eq!(rows["fixture_id"], "D13-F5");
        // J1 ordering rule the two J1 builders deliver.
        assert_eq!(
            rows["J1_ordering"],
            "projects by identifier ASC; pods default-first then name ASC"
        );
        // J1_example: first-default-wins + pod_count == len(pods).
        let first = &rows["J1_example"][0];
        assert_eq!(first["identifier"], "API");
        assert_eq!(first["pod_count"], 2);
        assert_eq!(first["pods"].as_array().unwrap().len(), 2);
        assert_eq!(first["pods"][0]["is_default"], true);
        assert_eq!(first["default_pod_id"], first["pods"][0]["id"]);
        let second = &rows["J1_example"][1];
        assert_eq!(second["pod_count"], 0);
        assert!(second["default_pod_id"].is_null());
        // Guard precedence the P5 builders are ordered for.
        assert!(rows["P5_guard_precedence"]
            .as_str()
            .unwrap()
            .contains("runners-guard beats runs-guard beats default-guard"));
        // Sweep shape.
        assert!(rows["P5_sweep"]
            .as_str()
            .unwrap()
            .contains("assigned_pod_id"));
        assert!(issue_assigned_pod_clear_sql().contains("assigned_pod_id"));
        // Runner-count N+1 note: unfiltered (includes revoked).
        assert!(rows["P1_runner_count_n1"]
            .as_str()
            .unwrap()
            .contains("includes revoked"));
        assert!(!pod_runner_count_sql().contains("revoked"));
    }
}
