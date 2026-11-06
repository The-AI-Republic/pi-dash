#![forbid(unsafe_code)]

//! Daemon-enrollment read/write statement sets (queries-A, PIDASHCONV-582).
//!
//! Port of the five statement sets in `runner/views/enrollment.py`:
//!
//! 1. Enroll tx (`:284-358`): locked runner by one-time enrollment hash
//!    (E1) + mark-enrolled update (E2).
//! 2. Refresh row-lock (`:394-467`): locked runner read (E3),
//!    dev-machine-revoked probe (E4), rotation update + force-refresh
//!    clear (E5).
//! 3. Create endpoint (`:597-680` + `:706-735`): machine-token binding
//!    checks (pure, documented by ref), workspace slug/inference reads
//!    (E6a-E6b), project read (E6c), pod read-or-default (E6d), the
//!    desktop-bundled recover-or-cap reads (E7 reuse probe + cap count)
//!    and the runner insert.
//! 4. Token bootstrap/rotate (`:130-203`): locked-exists probe (B1) +
//!    savepoint insert (B2, `IntegrityError` → `None`) and revoke-then-insert
//!    rotate (B3).
//! 5. Dev-machine get-or-create (`:57-128`): locked-by-id (D1),
//!    create-with-id (D2), locked-by-owner-host + legacy reselect (D3),
//!    `_touch_dev_machine` updates (D4).
//!
//! Fixture record: `rust-api/fixtures/runner_enroll/queries/` —
//! `enroll_refresh.sql` (E1-E7) + `bootstrap.sql` (B1-B3, D1-D4) with
//! example rows in `enroll_refresh.rows.json` (filed by PIDASHCONV-578).
//! The fixture SQL is hand-composed and abbreviates several Django
//! renderings (see "Fixture deviations" below); every builder below was
//! instead verified against SQL compiled from the real querysets on the
//! repo-pinned Django 4.2.30, and emits that exact text with `%s`
//! replaced by `$N`. The `#[cfg(test)]` suite pins the recorded fixture
//! fragments inside the builder output (the `assert_builder_contains`
//! direction from the D-02 `space/queries/` precedent) AND the full
//! compiled text.
//!
//! Conventions (same as `space/queries/`):
//!
//! * Builders return owned SQL text; `$N` params are documented in
//!   first-appearance (binding) order. Handlers bind them positionally.
//! * `WHERE` conjunct order is Django's: manager-scope conjuncts first,
//!   then the filter kwargs in `Q`-sorted (alphabetical) order. `$N`
//!   numbering follows that order, NOT the Python call order (e.g. E6c
//!   binds `identifier=$1, workspace_id=$2` although the source passes
//!   `workspace_id` first).
//! * `save(update_fields=[...])` emits `SET` in Django *model-field*
//!   order, not in `update_fields` list order (`Model.save_base`,
//!   `django/db/models/base.py`). The `*_UPDATE_FIELDS` consts pin the
//!   verbatim Python lists; the builders emit the verified model order.
//! * `select_related` forward-FK joins use the real table names (no
//!   `U0` aliases); the joined tables carry NO manager scope.
//! * Timestamps and UUIDs cross this boundary as bind params (`now()`
//!   and fresh UUIDs are computed by the caller); the SQL only names
//!   the `$N` slots.
//!
//! Out of scope here (owned elsewhere, referenced so handlers can find them):
//!
//! * `Pod.default_for_project_id` — already ported as
//!   [`pod::DEFAULT_FOR_PROJECT_ID_SQL`](pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL);
//!   it is the second leg of E6d.
//! * `is_workspace_member` — kernel membership fact
//!   (`pidash_auth`, consumed by handlers, never re-ported).
//! * `_next_auto_runner_name`, `_managed_cap_error`'s 409 body,
//!   `_RUNNER_NAME_RE`, `_touch_dev_machine`'s golden helper semantics —
//!   F6 flows / handlers-A (PIDASHCONV-590).
//! * `deactivate_api_token` — already ported
//!   (`db/src/auth_oauth/queries.rs`, reused by handlers-A).
//! * Response bodies (`invalid_or_expired_enrollment_token`, …) —
//!   handlers-A/B/C own every status + body; the guard→error mapping is
//!   documented on each builder but no body const lives here.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-rotate-user-omission (`enrollment.py:181-186`): `_rotate_machine_token`
//!   with a dev machine filters on workspace + machine only — `user` is
//!   dropped — while `_maybe_mint_machine_token` keeps it. Ported as-is
//!   (the domain `revoke_tokens_for_machine_in_workspace` takes no user
//!   param); listed as a suspected bug.
//! * QUIRK-pod-fallthrough (`:671-675`): when `pod_name` is given but
//!   matches nothing, `pod` stays `None` and the code falls through to
//!   the default pod — an unknown `?pod=` is silently ignored, no 404.
//!   Ported as-is (handlers run E6d-explicit, then the default leg).
//! * QUIRK-get-no-limit (E1): Django's `.get()` clears `ORDER BY` and
//!   adds `LIMIT 21` for the multiplicity probe. Following the D-02
//!   `space/queries` precedent, [`enroll_locked_read_sql`] omits the
//!   `LIMIT 21`; handlers map row counts (`0 → DoesNotExist`,
//!   `>1 → MultipleObjectsReturned`).
//! * QUIRK-duplicate-deleted-at (E6d-explicit): the explicit
//!   `deleted_at__isnull=True` filter does NOT collapse with the
//!   `PodManager` scope — Django emits the conjunct twice. Kept.
//! * QUIRK-lock-before-update (B3): the source calls
//!   `select_for_update()` before `.update(...)`; Django drops the lock
//!   on update queries, so the rotate revoke is a plain `UPDATE` (now in
//!   `crate::domain::tokens`).
//! * QUIRK-rotation-value-binds (E5): the rotation copies are computed
//!   Python-side from the locked row (`generation + 1`, previous ←
//!   current), so the `UPDATE` binds plain values — there is no
//!   `generation = generation + 1` self-reference in the SQL.
//! * QUIRK-enroll-host-not-stripped (`:281` vs `:78`): the enroll path
//!   slices `host_label[:255]` WITHOUT stripping, while the get-or-create
//!   and touch paths strip first. Both forms are ported
//!   ([`slice_host_label`] vs [`normalize_host_label`]).
//! * QUIRK-bare-boolean: `BooleanField(exact=True)` renders as a bare
//!   column (`"workspace_members"."is_active"`, `"pod"."is_default"`),
//!   not `= TRUE`.
//!
//! # Fixture deviations (fixture abbreviates; builders are Django-exact)
//!
//! * E6a/E6b/E6c omit the `SoftDeletionManager` scope
//!   (`deleted_at IS NULL`, `db/mixins.py:56-58`) and the `-created_at`
//!   ordering; E6b also omits the scope on the base table only (the
//!   joined `workspaces` row correctly carries none).
//! * `.first()` reads keep `Meta.ordering` + `LIMIT 1`; the fixture
//!   spells some without `ORDER BY`.
//! * E2/E5/D4 list `SET` in `update_fields` order; Django emits model
//!   order (E2 `name` 2nd when present; E5 `previous_…` LAST; D4
//!   `host_label, label` before the timestamps).
//! * E5 spells the rotation as self-referential SQL; Django binds
//!   Python-computed values (QUIRK-rotation-value-binds above).
//! * E4 spells `SELECT EXISTS(SELECT 1 …)`; Django's `.exists()` emits
//!   `SELECT 1 AS "a" … LIMIT 1`.
//! * E7-cap spells `status <> 'revoked'`; `.exclude()` emits
//!   `NOT ("runner"."status" = $5)`.
//! * B2 spells the label as `'machine: ' || substr($4, 1, 96)` SQL;
//!   Django binds the Python-computed string
//!   ([`machine_token_label`]).
//! * E5's force-refresh clear spells `WHERE "runner_id" = $3`; the `$N`
//!   is per-statement here (`$1`).
//!
//! Django-idiom → Rust-pattern rows applied: `$N` bind builders +
//! `*_UPDATE_FIELDS` consts (`space/queries/issue_retrieve.rs`,
//! `app_assets/queries_v2_user_workspace.rs`); char-boundary-safe
//! `[:N]` slicing (Porting guide semantic traps).

use pidash_compat::py::py_strip;
use pidash_db::entity::{
    dev_machine, machine_token, pod, projects, runner, runner_force_refresh, users,
    workspace_members, workspaces,
};
use pidash_db::runner_enroll::columns::{
    dev_machine as dm_cols, runner as r_cols, runner_force_refresh as ffr_cols,
};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, JoinType, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationDef, RelationTrait, Select, Set,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Cross-domain SELECT lists (Django `_meta` field order)
// ---------------------------------------------------------------------------

/// `workspaces` full-row column order as Django selects it
/// (`db/models/workspace.py:119-139` over `BaseModel` + `AuditModel`).
/// Pinned from SQL compiled on Django 4.2.30: audit fields first,
/// then `id`, then the concrete fields.
///
/// NOTE: this deliberately does NOT reuse
/// `crate::app_workspace::models_workspace::workspace::COLUMNS`, whose
/// `id`-first order is fixture order, not `_meta` (select) order.
pub const WORKSPACES_SELECT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "name",
    "logo",
    "logo_asset_id",
    "owner_id",
    "slug",
    "organization_size",
    "timezone",
    "background_color",
];

/// `workspace_members` full-row column order as Django selects it
/// (`db/models/workspace.py:198-213`). Same `id`-sixth note as
/// [`WORKSPACES_SELECT_COLUMNS`].
pub const WORKSPACE_MEMBERS_SELECT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "workspace_id",
    "member_id",
    "role",
    "company_role",
    "view_props",
    "default_props",
    "issue_props",
    "is_active",
    "getting_started_checklist",
    "tips",
    "explored_features",
];

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
// E1. Enroll tx: locked runner by one-time enrollment hash
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// E2. Enroll tx: mark enrolled + rotate to refresh generation 1
// ---------------------------------------------------------------------------

/// E2 — the verbatim `update_fields` list (`enrollment.py:325-335`),
/// before the conditional `"name"` append at `:345-347`.
/// [`enroll_mark_enrolled_sql`] emits these columns in Django model-field
/// order (see module docs), NOT in this order.
pub const ENROLL_UPDATE_FIELDS: &[&str] = &[
    "dev_machine",
    "host_label",
    "enrolled_at",
    "enrollment_token_hash",
    "enrollment_token_fingerprint",
    "refresh_token_hash",
    "refresh_token_fingerprint",
    "refresh_token_generation",
    "previous_refresh_token_hash",
];

// ---------------------------------------------------------------------------
// E3. Refresh row-lock: locked runner + workspace
// ---------------------------------------------------------------------------

/// E3 — refresh locked read (`enrollment.py:403-404`).
///
/// `Runner.objects.select_for_update().select_related("workspace").filter(id=runner_id).first()`
/// inside `transaction.atomic()`. `.first()` keeps `Meta.ordering`
/// (`-last_heartbeat_at, -created_at`) and adds `LIMIT 1`.
///
/// `$1` = runner id (URL pk).
///
/// Guards: miss → 401 `invalid_refresh_token` (same code as a bad hash —
/// existence is not leaked, `:405-409`); `revoked_at` set → 401
/// `runner_revoked` (`:410-414`).
pub fn refresh_locked_read_sql() -> String {
    format!(
        "SELECT {}, {} FROM \"runner\" \
         INNER JOIN \"workspaces\" ON (\"runner\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE \"runner\".\"id\" = $1 \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
        qualified("workspaces", WORKSPACES_SELECT_COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// E4. Refresh: dev-machine revoked probe
// ---------------------------------------------------------------------------

/// E4 — dev-machine revoked probe (`enrollment.py:415-425`).
///
/// NOT via the E3 join: a separate
/// `DevMachine.objects.filter(pk=dev_machine_id,
/// revoked_at__isnull=False).exists()`, run only when
/// `runner.dev_machine_id is not None`. Django's `.exists()` emits
/// `SELECT 1 AS "a" … LIMIT 1` with ordering cleared.
///
/// `$1` = the runner's `dev_machine_id`.
///
/// True → 401 `dev_machine_revoked` (`:421-425`).
pub fn dev_machine_revoked_probe_sql() -> String {
    "SELECT 1 AS \"a\" FROM \"dev_machine\" \
     WHERE (\"dev_machine\".\"id\" = $1 AND \"dev_machine\".\"revoked_at\" IS NOT NULL) \
     LIMIT 1"
        .to_string()
}

// ---------------------------------------------------------------------------
// E5. Refresh: rotate + force-refresh clear
// ---------------------------------------------------------------------------

/// E5 — the verbatim rotation `update_fields` list
/// (`enrollment.py:453-459`). [`refresh_rotate_sql`] emits these in
/// Django model-field order (`previous_…` LAST), not in this order.
pub const REFRESH_ROTATE_UPDATE_FIELDS: &[&str] = &[
    "previous_refresh_token_hash",
    "refresh_token_hash",
    "refresh_token_fingerprint",
    "refresh_token_generation",
];

/// E5 — refresh rotation (`enrollment.py:448-460`), still inside the tx.
///
/// `runner.save(update_fields=...)`: `updated_at` unchanged (not in the
/// list). Per QUIRK-rotation-value-binds, every `SET` is a plain bind of
/// a Python-computed value from the locked row — `$1` new refresh hash,
/// `$2` new refresh fingerprint, `$3` new generation (old + 1), `$4`
/// previous hash (the OLD current hash), `$5` runner id.
///
/// The previous-hash replay branch instead calls
/// `runner.revoke('refresh_token_replayed')` + 401 (services-C), and the
/// non-member branch `runner.revoke('membership_revoked')` + 401
/// (`:429-446`) — neither runs this statement.
pub fn refresh_rotate_sql() -> String {
    "UPDATE \"runner\" SET \"refresh_token_hash\" = $1, \
     \"refresh_token_fingerprint\" = $2, \"refresh_token_generation\" = $3, \
     \"previous_refresh_token_hash\" = $4 WHERE \"runner\".\"id\" = $5"
        .to_string()
}

/// E5 — force-refresh clear (`enrollment.py:467`), still inside the tx.
///
/// `RunnerForceRefresh.objects.filter(runner=runner).delete()`: a plain
/// single-table delete (nothing cascades off it).
///
/// `$1` = runner id.
pub fn force_refresh_clear_sql() -> String {
    format!(
        "DELETE FROM \"{}\" WHERE \"{}\".\"runner_id\" = $1",
        ffr_cols::TABLE,
        ffr_cols::TABLE,
    )
}

// ---------------------------------------------------------------------------
// E6. Create-endpoint reads
// ---------------------------------------------------------------------------

/// E6a — explicit workspace slug (`enrollment.py:629-637`).
///
/// `Workspace.objects.filter(slug=slug).first()`: default-manager scope
/// (`deleted_at IS NULL`) + `Meta.ordering` (`-created_at`) + `LIMIT 1`.
///
/// `$1` = workspace slug.
///
/// Miss — or caller not a member (same 404, no leak; the membership
/// check is a kernel fact) → 404 `workspace_not_found`.
pub fn workspace_by_slug_sql() -> String {
    format!(
        "SELECT {} FROM \"workspaces\" \
         WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1) \
         ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1",
        qualified("workspaces", WORKSPACES_SELECT_COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// E7. Create tx body: desktop reuse probe + cap count + insert
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// B1/B2/B3. Token bootstrap + rotate
// ---------------------------------------------------------------------------

// NOTE: the B2 bootstrap insert and the B3 rotate revoke builders lived
// here; the statements moved to `crate::domain::tokens`
// (`insert_machine_token`, `revoke_tokens_for_machine_in_workspace`,
// `revoke_tokens_legacy_host`; PIDASHCONV-1520), whose tests carry the
// pins. The unique-violation → savepoint-rollback contract stays with
// the enroll handler.

// ---------------------------------------------------------------------------
// D1/D2/D3/D4. Dev-machine get-or-create
// ---------------------------------------------------------------------------

/// D1 — dev-machine by id, locked (`_get_or_create_dev_machine`,
/// `enrollment.py:80-85`).
///
/// `DevMachine.objects.select_for_update().filter(pk=id).first()`:
/// `Meta.ordering` (`-last_seen_at, -created_at`) + `LIMIT 1`. Also the
/// D2 race-retry re-lock (`:96`).
///
/// `$1` = dev-machine id.
///
/// Hit + owner mismatch → `DevMachineOwnershipError` → 404
/// `dev_machine_not_found`; hit + owner match → touch (D4) + return;
/// miss → D2 create.
pub fn dev_machine_by_id_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 \
         ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

/// D4 — the always-written touch fields (`_touch_dev_machine`,
/// `enrollment.py:60`). [`touch_dev_machine_sql`] emits these plus the
/// conditional `host_label`/`label` in Django model-field order.
pub const TOUCH_BASE_FIELDS: &[&str] = &["last_seen_at", "updated_at"];

// ---------------------------------------------------------------------------
// Bind helpers (pure; the SQL above names the slots, these compute values)
// ---------------------------------------------------------------------------

/// Python `s[:n]` over code points (`enrollment.py` slices `host_label`,
/// `name` and `label` this way). Never panics on a UTF-8 boundary
/// (Porting guide semantic traps: byte slicing would).
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// D-path host label (`_get_or_create_dev_machine:78`,
/// `_touch_dev_machine:58`): `(host_label or "").strip()[:255]`.
/// The caller passes `""` for a missing label (the `or ""`).
pub fn normalize_host_label(raw: &str) -> String {
    truncate_chars(py_strip(raw), 255)
}

/// Enroll-path request label (`RunnerEnrollEndpoint.post:281`):
/// `(data.get("host_label") or "")[:255]` — sliced WITHOUT stripping
/// (QUIRK-enroll-host-not-stripped).
pub fn slice_host_label(raw: &str) -> String {
    truncate_chars(raw, 255)
}

/// Create/enroll body name (`:282`, `:580`):
/// `(data.get("name") or "").strip()[:128]`. Empty result → the E2
/// `name` slot stays out (`if body_name:`, `:345`).
pub fn normalize_body_name(raw: &str) -> String {
    truncate_chars(py_strip(raw), 128)
}

/// B2/B3-rot label (`:166`, `:200`): `f"machine: {host_label[:96]}"`.
/// The slice applies to the host label BEFORE the prefix is added.
pub fn machine_token_label(host_label: &str) -> String {
    format!("machine: {}", truncate_chars(host_label, 96))
}

/// D2 label (`:92`, `:116`): `host_label[:128]` (the already-normalized
/// D-path label, possibly `""`).
pub fn dev_machine_label(normalized_host_label: &str) -> String {
    truncate_chars(normalized_host_label, 128)
}

/// E2 host label (`:337`): `host_label or runner.host_label` — a blank
/// request label keeps the stored value (Python `or`, not a NULL).
pub fn enroll_host_label<'a>(request_label: &'a str, stored: &'a str) -> &'a str {
    if request_label.is_empty() {
        stored
    } else {
        request_label
    }
}

/// Which conditional columns D4 writes (see [`touch_dev_machine_sql`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchSelection {
    /// Request label non-empty and differs from the stored host label
    /// (`:62-64`).
    pub update_host_label: bool,
    /// Request label non-empty and the stored label is empty (`:65-67`).
    pub update_label: bool,
}

/// D4 field selection (`_touch_dev_machine:57-69`).
///
/// `request_label_raw` is the RAW request label — normalization
/// (`strip()[:255]`) happens inside, exactly like the source. Stored
/// values are never NULL (`host_label`/`label` have no `null=True`).
pub fn touch_selection(
    request_label_raw: &str,
    stored_host_label: &str,
    stored_label: &str,
) -> TouchSelection {
    let host_label = normalize_host_label(request_label_raw);
    TouchSelection {
        update_host_label: !host_label.is_empty() && stored_host_label != host_label,
        update_label: !host_label.is_empty() && stored_label.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// SeaORM reads and writes (the enroll / create endpoints)
// ---------------------------------------------------------------------------

// The entities omit runner's relations (see entity/mod.rs); the joins
// the enroll reads need are spelled here.
fn runner_to_workspace() -> RelationDef {
    runner::Entity::belongs_to(workspaces::Entity)
        .from(runner::Column::WorkspaceId)
        .to(workspaces::Column::Id)
        .into()
}

fn runner_to_pod() -> RelationDef {
    runner::Entity::belongs_to(pod::Entity)
        .from(runner::Column::PodId)
        .to(pod::Column::Id)
        .into()
}

/// The [`enroll_locked_read_sql`] row the enroll endpoint reads.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct EnrollRunnerRow {
    pub runner_id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub pod_id: Uuid,
    pub name: String,
    pub host_label: String,
    pub enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub workspace_slug: String,
    pub pod_name: String,
    pub project_identifier: String,
}

/// E1 (`enrollment.py:284-290`): runners by one-time enrollment hash,
/// INNER JOINed to workspace, pod and project, row-locked. Every match is
/// returned so the caller can 500 on two (`MultipleObjectsReturned`).
pub fn enroll_locked_query(token_hash: &str) -> Select<runner::Entity> {
    runner::Entity::find()
        .select_only()
        .column_as(runner::Column::Id, "runner_id")
        .columns([
            runner::Column::OwnerId,
            runner::Column::WorkspaceId,
            runner::Column::PodId,
            runner::Column::Name,
            runner::Column::HostLabel,
            runner::Column::EnrolledAt,
            runner::Column::RevokedAt,
        ])
        .column_as(workspaces::Column::Slug, "workspace_slug")
        .column_as(pod::Column::Name, "pod_name")
        .column_as(projects::Column::Identifier, "project_identifier")
        .join(JoinType::InnerJoin, runner_to_workspace())
        .join(JoinType::InnerJoin, runner_to_pod())
        .join(JoinType::InnerJoin, pod::Relation::Projects.def())
        .filter(runner::Column::EnrollmentTokenHash.eq(token_hash))
        .lock_exclusive()
}

pub async fn enroll_locked_runners(
    db: &impl ConnectionTrait,
    token_hash: &str,
) -> Result<Vec<EnrollRunnerRow>, DbErr> {
    enroll_locked_query(token_hash)
        .into_model::<EnrollRunnerRow>()
        .all(db)
        .await
}

/// E2 (`enrollment.py:325-348`, [`ENROLL_UPDATE_FIELDS`]): mark enrolled
/// and rotate to refresh generation 1; `name` only when the body set one.
/// `updated_at` is not in the field list.
#[allow(clippy::too_many_arguments)]
pub async fn mark_runner_enrolled(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
    dev_machine_id: Option<Uuid>,
    name: Option<&str>,
    host_label: &str,
    refresh_hash: &str,
    refresh_fingerprint: &str,
    enrolled_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    let mut update = runner::Entity::update_many()
        .col_expr(runner::Column::DevMachineId, Expr::value(dev_machine_id));
    if let Some(name) = name {
        update = update.col_expr(runner::Column::Name, Expr::value(name));
    }
    update
        .col_expr(runner::Column::HostLabel, Expr::value(host_label))
        .col_expr(runner::Column::RefreshTokenHash, Expr::value(refresh_hash))
        .col_expr(
            runner::Column::RefreshTokenFingerprint,
            Expr::value(refresh_fingerprint),
        )
        .col_expr(runner::Column::RefreshTokenGeneration, Expr::value(1i32))
        .col_expr(runner::Column::PreviousRefreshTokenHash, Expr::value(""))
        .col_expr(runner::Column::EnrollmentTokenHash, Expr::value(""))
        .col_expr(runner::Column::EnrollmentTokenFingerprint, Expr::value(""))
        .col_expr(runner::Column::EnrolledAt, Expr::value(enrolled_at))
        .filter(runner::Column::Id.eq(runner_id))
        .exec(db)
        .await
        .map(drop)
}

/// A locked dev-machine row: the touch inputs plus provisioning.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct LockedDevMachineRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub host_label: String,
    pub label: String,
    pub provisioning: String,
}

fn locked_dev_machine() -> Select<dev_machine::Entity> {
    dev_machine::Entity::find()
        .select_only()
        .columns([
            dev_machine::Column::Id,
            dev_machine::Column::OwnerId,
            dev_machine::Column::HostLabel,
            dev_machine::Column::Label,
            dev_machine::Column::Provisioning,
        ])
        .lock_exclusive()
}

/// D1 ([`dev_machine_by_id_sql`]): by id, `Meta.ordering`, `LIMIT 1`,
/// `FOR UPDATE`.
pub async fn lock_dev_machine_by_id(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<Option<LockedDevMachineRow>, DbErr> {
    locked_dev_machine()
        .filter(dev_machine::Column::Id.eq(id))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
        .into_model::<LockedDevMachineRow>()
        .one(db)
        .await
}

/// D3 ([`dev_machine_by_owner_host_sql`]): the owner's oldest unrevoked
/// machine with this host label, `FOR UPDATE`.
pub async fn lock_dev_machine_by_owner_host(
    db: &impl ConnectionTrait,
    host_label: &str,
    owner_id: Uuid,
) -> Result<Option<LockedDevMachineRow>, DbErr> {
    locked_dev_machine()
        .filter(dev_machine::Column::HostLabel.eq(host_label))
        .filter(dev_machine::Column::OwnerId.eq(owner_id))
        .filter(dev_machine::Column::RevokedAt.is_null())
        .order_by_asc(dev_machine::Column::CreatedAt)
        .into_model::<LockedDevMachineRow>()
        .one(db)
        .await
}

/// `_touch_dev_machine` (`enrollment.py:57-69`, [`touch_selection`]):
/// `host_label` / `label` only when selected, then the two timestamps.
pub async fn touch_dev_machine(
    db: &impl ConnectionTrait,
    id: Uuid,
    host_label: Option<&str>,
    label: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    let mut update = dev_machine::Entity::update_many();
    if let Some(host_label) = host_label {
        update = update.col_expr(dev_machine::Column::HostLabel, Expr::value(host_label));
    }
    if let Some(label) = label {
        update = update.col_expr(dev_machine::Column::Label, Expr::value(label));
    }
    update
        .col_expr(dev_machine::Column::LastSeenAt, Expr::value(now))
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now))
        .filter(dev_machine::Column::Id.eq(id))
        .exec(db)
        .await
        .map(drop)
}

/// `_maybe_mint_machine_token`'s locked probe ([`bootstrap_probe_sql`]):
/// an unrevoked token for this user + workspace on the machine (or, with
/// no machine, on the host label with no machine), newest first,
/// `FOR UPDATE`.
pub async fn bootstrap_token_exists(
    db: &impl ConnectionTrait,
    dev_machine_id: Option<Uuid>,
    host_label: &str,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, DbErr> {
    let query = machine_token::Entity::find()
        .select_only()
        .column(machine_token::Column::Id);
    let query = match dev_machine_id {
        Some(machine) => query.filter(machine_token::Column::DevMachineId.eq(machine)),
        None => query
            .filter(machine_token::Column::DevMachineId.is_null())
            .filter(machine_token::Column::HostLabel.eq(host_label)),
    };
    let hit: Option<Uuid> = query
        .filter(machine_token::Column::RevokedAt.is_null())
        .filter(machine_token::Column::UserId.eq(user_id))
        .filter(machine_token::Column::WorkspaceId.eq(workspace_id))
        .order_by_desc(machine_token::Column::CreatedAt)
        .lock_exclusive()
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// E6a ([`workspace_by_slug_sql`]): a live workspace by slug,
/// `-created_at`, first.
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

/// A live workspace's slug by id (the stale-ticket check).
pub async fn live_workspace_slug(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
) -> Result<Option<String>, DbErr> {
    workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Slug)
        .filter(workspaces::Column::Id.eq(workspace_id))
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// E6b ([`memberships_for_inference_sql`]): the caller's active, live
/// memberships' workspaces `(id, slug)`, oldest membership first, at
/// most two (the caller only needs "exactly one").
pub async fn memberships_for_inference(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<Vec<(Uuid, String)>, DbErr> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .column(workspaces::Column::Slug)
        .join(
            JoinType::InnerJoin,
            workspace_members::Relation::Workspaces.def(),
        )
        .filter(workspace_members::Column::DeletedAt.is_null())
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .order_by_asc(workspace_members::Column::CreatedAt)
        .limit(2)
        .into_tuple()
        .all(db)
        .await
}

/// `is_workspace_member` (`core/permissions.py:28-34`): an active, live
/// membership exists.
pub async fn is_active_member(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Id)
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// E6c ([`project_by_workspace_identifier_sql`]): a live project
/// `(id, identifier)`, `-created_at`, first.
pub async fn project_by_workspace_identifier(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    identifier: &str,
) -> Result<Option<(Uuid, String)>, DbErr> {
    projects::Entity::find()
        .select_only()
        .columns([projects::Column::Id, projects::Column::Identifier])
        .filter(projects::Column::DeletedAt.is_null())
        .filter(projects::Column::Identifier.eq(identifier))
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .order_by_desc(projects::Column::CreatedAt)
        .into_tuple()
        .one(db)
        .await
}

/// A pod hit `(id, name, project_id)`.
pub type PodHitRow = (Uuid, String, Uuid);

fn live_pods(project_id: Uuid) -> Select<pod::Entity> {
    pod::Entity::find()
        .select_only()
        .columns([pod::Column::Id, pod::Column::Name, pod::Column::ProjectId])
        .filter(pod::Column::DeletedAt.is_null())
        .filter(pod::Column::ProjectId.eq(project_id))
        .order_by_desc(pod::Column::IsDefault)
        .order_by_asc(pod::Column::CreatedAt)
}

/// E6d-explicit ([`pod_by_name_sql`]): a live pod of the project by name.
pub async fn pod_by_name(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    name: &str,
) -> Result<Option<PodHitRow>, DbErr> {
    live_pods(project_id)
        .filter(pod::Column::Name.eq(name))
        .into_tuple()
        .one(db)
        .await
}

/// E6d-default (`Pod.default_for_project_id`, `models.py:173-176`).
pub async fn default_pod_for_project(
    db: &impl ConnectionTrait,
    project_id: Uuid,
) -> Result<Option<PodHitRow>, DbErr> {
    live_pods(project_id)
        .filter(pod::Column::IsDefault.eq(true))
        .into_tuple()
        .one(db)
        .await
}

/// `_next_auto_runner_name` (`enrollment.py:790-796`): every runner in
/// the pod, revoked included.
pub async fn count_pod_runners(db: &impl ConnectionTrait, pod_id: Uuid) -> Result<u64, DbErr> {
    runner::Entity::find()
        .filter(runner::Column::PodId.eq(pod_id))
        .count(db)
        .await
}

/// A reused desktop runner `(id, name, refresh_token_generation)`.
pub type ReuseHitRow = (Uuid, String, i32);

/// E7 reuse probe ([`desktop_reuse_probe_sql`]): the live bundled runner
/// for this machine / owner / pod / workspace, `Meta.ordering`, locked.
pub async fn desktop_reuse_probe(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
    owner_id: Uuid,
    pod_id: Uuid,
    provisioning: &str,
    workspace_id: Uuid,
) -> Result<Option<ReuseHitRow>, DbErr> {
    runner::Entity::find()
        .select_only()
        .columns([
            runner::Column::Id,
            runner::Column::Name,
            runner::Column::RefreshTokenGeneration,
        ])
        .filter(runner::Column::DevMachineId.eq(dev_machine_id))
        .filter(runner::Column::OwnerId.eq(owner_id))
        .filter(runner::Column::PodId.eq(pod_id))
        .filter(runner::Column::Provisioning.eq(provisioning))
        .filter(runner::Column::RevokedAt.is_null())
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .lock_exclusive()
        .into_tuple()
        .one(db)
        .await
}

/// E7 cap count (`_managed_cap_error`, `enrollment.py:237-246`): the
/// owner's non-revoked runners of this provisioning in the pod.
pub async fn managed_cap_count(
    db: &impl ConnectionTrait,
    owner_id: Uuid,
    pod_id: Uuid,
    provisioning: &str,
    workspace_id: Uuid,
    revoked: &str,
) -> Result<u64, DbErr> {
    runner::Entity::find()
        .filter(runner::Column::OwnerId.eq(owner_id))
        .filter(runner::Column::PodId.eq(pod_id))
        .filter(runner::Column::Provisioning.eq(provisioning))
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .filter(runner::Column::Status.ne(revoked))
        .count(db)
        .await
}

/// The E7 runner insert's caller-chosen values; every other column takes
/// the `Runner` field default, as the old 31-column `INSERT` bound them.
pub struct NewRunner<'a> {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub name: &'a str,
    pub host_label: &'a str,
    pub provisioning: &'a str,
    pub status: &'a str,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// E7 (`enrollment.py:726-735`): `Runner.objects.create(...)` with the
/// model defaults (visibility 0, generation 0, key version 1, empty
/// hashes, `[]` capabilities, `{}` metadata, protocol 1) and
/// `enrolled_at = created_at = updated_at = now`.
pub async fn insert_runner(db: &impl ConnectionTrait, new: &NewRunner<'_>) -> Result<(), DbErr> {
    runner::Entity::insert(runner::ActiveModel {
        id: Set(new.id),
        owner_id: Set(new.owner_id),
        workspace_id: Set(new.workspace_id),
        dev_machine_id: Set(new.dev_machine_id),
        pod_id: Set(new.pod_id),
        name: Set(new.name.to_owned()),
        host_label: Set(new.host_label.to_owned()),
        provisioning: Set(new.provisioning.to_owned()),
        pod_dispatch: Set(
            if new.provisioning == "desktop_bundled" {
                "pinned_only".to_owned()
            } else {
                "pod_member".to_owned()
            },
        ),
        visibility: Set(0),
        refresh_token_hash: Set(String::new()),
        refresh_token_fingerprint: Set(String::new()),
        refresh_token_generation: Set(0),
        previous_refresh_token_hash: Set(String::new()),
        access_token_signing_key_version: Set(1),
        enrollment_token_hash: Set(String::new()),
        enrollment_token_fingerprint: Set(String::new()),
        enrolled_at: Set(Some(new.now.into())),
        capabilities: Set(serde_json::json!([])),
        status: Set(new.status.to_owned()),
        os: Set(String::new()),
        arch: Set(String::new()),
        runner_version: Set(String::new()),
        dev_metadata: Set(serde_json::json!({})),
        protocol_version: Set(1),
        last_heartbeat_at: Set(None),
        created_at: Set(new.now.into()),
        updated_at: Set(new.now.into()),
        revoked_at: Set(None),
        revoked_reason: Set(String::new()),
        paused_at: Set(None),
        paused_by_id: Set(None),
    })
    .exec_without_returning(db)
    .await
    .map(drop)
}

/// The session-auth user row `(id, password, is_active)`.
pub async fn session_user(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<Option<(Uuid, String, bool)>, DbErr> {
    users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::Password,
            users::Column::IsActive,
        ])
        .filter(users::Column::Id.eq(user_id))
        .into_tuple()
        .one(db)
        .await
}

/// The user row exists (the stale-ticket check).
pub async fn user_exists(db: &impl ConnectionTrait, user_id: Uuid) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .filter(users::Column::Id.eq(user_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// SeaORM refresh statements (E3-E5)
// ---------------------------------------------------------------------------

/// The E3 runner facts the refresh endpoint reads.
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
pub struct RefreshRunnerRow {
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub refresh_token_hash: String,
    pub refresh_token_generation: i32,
    pub previous_refresh_token_hash: String,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// E3 (`enrollment.py:403-404`): the runner by id, INNER JOINed to its
/// workspace (`select_related`), `Meta.ordering`, first, `FOR UPDATE`.
pub async fn refresh_locked_runner(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<Option<RefreshRunnerRow>, DbErr> {
    runner::Entity::find()
        .select_only()
        .columns([
            runner::Column::OwnerId,
            runner::Column::WorkspaceId,
            runner::Column::DevMachineId,
            runner::Column::RefreshTokenHash,
            runner::Column::RefreshTokenGeneration,
            runner::Column::PreviousRefreshTokenHash,
            runner::Column::RevokedAt,
        ])
        .join(JoinType::InnerJoin, runner_to_workspace())
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .lock_exclusive()
        .into_model::<RefreshRunnerRow>()
        .one(db)
        .await
}

/// E4 (`enrollment.py:415-425`): the linked dev machine is revoked.
pub async fn dev_machine_is_revoked(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = dev_machine::Entity::find()
        .select_only()
        .column(dev_machine::Column::Id)
        .filter(dev_machine::Column::Id.eq(dev_machine_id))
        .filter(dev_machine::Column::RevokedAt.is_not_null())
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// E5 (`enrollment.py:448-460`): rotate to the new refresh token; the
/// previous hash is the OLD current hash.
pub async fn rotate_refresh_token(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
    new_hash: &str,
    new_fingerprint: &str,
    new_generation: i32,
    previous_hash: &str,
) -> Result<(), DbErr> {
    runner::Entity::update_many()
        .col_expr(runner::Column::RefreshTokenHash, Expr::value(new_hash))
        .col_expr(
            runner::Column::RefreshTokenFingerprint,
            Expr::value(new_fingerprint),
        )
        .col_expr(
            runner::Column::RefreshTokenGeneration,
            Expr::value(new_generation),
        )
        .col_expr(
            runner::Column::PreviousRefreshTokenHash,
            Expr::value(previous_hash),
        )
        .filter(runner::Column::Id.eq(runner_id))
        .exec(db)
        .await
        .map(drop)
}

/// `RunnerForceRefresh.objects.filter(runner=runner).delete()`
/// (`enrollment.py:467`).
pub async fn clear_force_refresh(db: &impl ConnectionTrait, runner_id: Uuid) -> Result<(), DbErr> {
    runner_force_refresh::Entity::delete_many()
        .filter(runner_force_refresh::Column::RunnerId.eq(runner_id))
        .exec(db)
        .await
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::app_project::models::project as project_cols;
    use pidash_db::runner_enroll::columns::{machine_token as mt_cols, pod as pod_cols};

    const FIXTURE_ENROLL: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/enroll_refresh.sql");
    const FIXTURE_BOOTSTRAP: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/bootstrap.sql");
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/runner_enroll/queries/enroll_refresh.rows.json");

    /// Collapse every whitespace run to one space.
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Assert `needle` (squashed) is recorded in one of the D13-F5 SQL
    /// fixtures, and return it. Every `assert_builder_contains` below
    /// goes through here so a stale needle fails loudly at the fixture
    /// end, not silently at the builder end.
    fn fixture_fragment(needle: &str) -> String {
        let needle = squashed(needle);
        assert!(
            squashed(FIXTURE_ENROLL).contains(&needle)
                || squashed(FIXTURE_BOOTSTRAP).contains(&needle),
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

    // -- Oracle column order (transcribed from SQL compiled on Django 4.2.30) --

    const RUNNER_COLS: &[&str] = &[
        "id",
        "owner_id",
        "workspace_id",
        "dev_machine_id",
        "pod_id",
        "name",
        "host_label",
        "provisioning",
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
        assert_eq!(r_cols::COLUMNS, RUNNER_COLS);
        assert_eq!(r_cols::TABLE, "runner");
        assert_eq!(pod_cols::COLUMNS, POD_COLS);
        assert_eq!(pod_cols::TABLE, "pod");
        assert_eq!(dm_cols::COLUMNS, MACHINE_COLS);
        assert_eq!(dm_cols::TABLE, "dev_machine");
        assert_eq!(mt_cols::COLUMNS, TOKEN_COLS);
        assert_eq!(mt_cols::TABLE, "machine_token");
        assert_eq!(ffr_cols::TABLE, "runner_force_refresh");
        assert_eq!(WORKSPACES_SELECT_COLUMNS.len(), 14);
        assert_eq!(WORKSPACE_MEMBERS_SELECT_COLUMNS.len(), 17);
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

    // -- E1 --

    // -- E2 --

    // -- E3/E4/E5 --

    #[test]
    fn e3_keeps_ordering_with_limit_for_update() {
        let sql = refresh_locked_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" INNER JOIN \"workspaces\" ON (\"runner\".\"workspace_id\" = \"workspaces\".\"id\") \
             WHERE \"runner\".\"id\" = $1 ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \
             \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(
            list,
            format!(
                "{}, {}",
                expected_list("runner", RUNNER_COLS),
                expected_list("workspaces", WORKSPACES_SELECT_COLUMNS),
            )
        );
        for fragment in [
            fixture_fragment("INNER JOIN \"workspaces\""),
            fixture_fragment("\"id\" = $1"),
            fixture_fragment("FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn e4_is_exists_select_one() {
        assert_eq!(
            dev_machine_revoked_probe_sql(),
            "SELECT 1 AS \"a\" FROM \"dev_machine\" \
             WHERE (\"dev_machine\".\"id\" = $1 AND \"dev_machine\".\"revoked_at\" IS NOT NULL) LIMIT 1",
        );
        for fragment in [
            fixture_fragment("SELECT 1"),
            fixture_fragment("FROM \"dev_machine\""),
            fixture_fragment("\"revoked_at\" IS NOT NULL"),
        ] {
            assert_builder_contains(&dev_machine_revoked_probe_sql(), &fragment);
        }
    }

    #[test]
    fn e5_rotation_binds_values_and_clears_force_refresh() {
        assert_eq!(
            REFRESH_ROTATE_UPDATE_FIELDS,
            &[
                "previous_refresh_token_hash",
                "refresh_token_hash",
                "refresh_token_fingerprint",
                "refresh_token_generation",
            ]
        );
        // Model order (previous LAST), plain value binds — no self-reference.
        assert_eq!(
            refresh_rotate_sql(),
            "UPDATE \"runner\" SET \"refresh_token_hash\" = $1, \
             \"refresh_token_fingerprint\" = $2, \"refresh_token_generation\" = $3, \
             \"previous_refresh_token_hash\" = $4 WHERE \"runner\".\"id\" = $5",
        );
        assert!(!refresh_rotate_sql().contains("updated_at"));
        assert_eq!(
            force_refresh_clear_sql(),
            "DELETE FROM \"runner_force_refresh\" WHERE \"runner_force_refresh\".\"runner_id\" = $1",
        );
        for fragment in [
            fixture_fragment("\"refresh_token_hash\" = $1"),
            fixture_fragment("\"refresh_token_fingerprint\" = $2"),
            fixture_fragment("\"refresh_token_generation\" = "),
            fixture_fragment("DELETE FROM \"runner_force_refresh\""),
            fixture_fragment("\"runner_id\" = "),
        ] {
            assert_builder_contains(
                &(refresh_rotate_sql() + " " + &force_refresh_clear_sql()),
                &fragment,
            );
        }
    }

    // -- E6 --

    #[test]
    fn e6a_scopes_soft_delete_with_desc_order() {
        let sql = workspace_by_slug_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \
             \"workspaces\".\"slug\" = $1) ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1",
        );
        assert_eq!(list, expected_list("workspaces", WORKSPACES_SELECT_COLUMNS));
        for fragment in [
            fixture_fragment("FROM \"workspaces\""),
            fixture_fragment("\"slug\" = $1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- E7 --

    // -- B1/B2/B3 --

    // (The B2/B3 SQL pins moved with the statements to
    // `crate::domain::tokens`; the no-`LIMIT` contract rides the same
    // pin test there.)

    // -- D1/D2/D3/D4 --

    #[test]
    fn d1_selects_by_id_with_limit_for_update() {
        let sql = dev_machine_by_id_sql();
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
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- Bind helpers --

    #[test]
    fn truncate_chars_counts_code_points_not_bytes() {
        assert_eq!(truncate_chars("abcdef", 3), "abc");
        assert_eq!(truncate_chars("ab", 96), "ab");
        // 4-byte chars: byte slicing at 255 would panic or split one.
        let emoji = "🚀".repeat(300);
        assert_eq!(truncate_chars(&emoji, 255).chars().count(), 255);
        assert_eq!(truncate_chars("héllo-wörld", 5), "héllo");
    }

    #[test]
    fn host_label_normalization_matches_both_paths() {
        // D-path strips then slices.
        assert_eq!(normalize_host_label("  mbp  "), "mbp");
        assert_eq!(normalize_host_label(""), "");
        assert_eq!(normalize_host_label("   "), "");
        assert_eq!(normalize_host_label(&"x".repeat(300)).len(), 255);
        // Enroll path slices without stripping.
        assert_eq!(slice_host_label("  mbp  "), "  mbp  ");
        assert_eq!(slice_host_label(&"x".repeat(300)).len(), 255);
        // Body name strips then slices to 128.
        assert_eq!(normalize_body_name("  r1  "), "r1");
        assert_eq!(normalize_body_name("   "), "");
        assert_eq!(normalize_body_name(&"y".repeat(200)).len(), 128);
        // Python strip() also strips U+001C-U+001F and U+0085, which Rust
        // trim() leaves behind (probe-verified against CPython).
        assert_eq!(normalize_host_label("\u{1c}mbp\u{1f}"), "mbp");
        assert_eq!(normalize_host_label(" \u{85}mbp\u{85} "), "mbp");
        assert_eq!(normalize_body_name("\u{85}r1\u{1c}"), "r1");
        assert_eq!(normalize_host_label(" \u{1c} "), "");
    }

    #[test]
    fn token_and_machine_labels_slice_before_prefix() {
        assert_eq!(machine_token_label("mbp"), "machine: mbp");
        let long = "h".repeat(100);
        assert_eq!(
            machine_token_label(&long),
            format!("machine: {}", "h".repeat(96))
        );
        assert_eq!(dev_machine_label("mbp"), "mbp");
        assert_eq!(dev_machine_label(""), "");
        assert_eq!(dev_machine_label(&"h".repeat(200)).len(), 128);
    }

    #[test]
    fn enroll_host_label_falls_back_on_blank() {
        assert_eq!(enroll_host_label("new", "old"), "new");
        assert_eq!(enroll_host_label("", "old"), "old");
        // Whitespace-only is NOT blank to Python `or` (only "" is falsy).
        assert_eq!(enroll_host_label("  ", "old"), "  ");
    }

    #[test]
    fn touch_selection_matches_fixture_matrix() {
        // Fixture enroll_refresh.rows.json touch_D4, case by case.
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert!(rows.get("touch_D4").is_some());
        // stored 'old' + request 'new' -> stored 'new' (host updates).
        assert_eq!(
            touch_selection("new", "old", "old"),
            TouchSelection {
                update_host_label: true,
                update_label: false
            },
        );
        // label '' + host_label 'mbp' -> label='mbp' (label backfills).
        assert_eq!(
            touch_selection("mbp", "mbp", ""),
            TouchSelection {
                update_host_label: false,
                update_label: true
            },
        );
        // label 'Work laptop' kept (no clobber).
        assert_eq!(
            touch_selection("mbp", "other", "Work laptop"),
            TouchSelection {
                update_host_label: true,
                update_label: false
            },
        );
        // Blank request (after strip) writes timestamps only.
        assert_eq!(
            touch_selection("   ", "old", ""),
            TouchSelection {
                update_host_label: false,
                update_label: false
            },
        );
        // Python-strip-only whitespace also normalizes to blank.
        assert_eq!(
            touch_selection("\u{85} \u{1c}", "old", ""),
            TouchSelection {
                update_host_label: false,
                update_label: false
            },
        );
        assert_eq!(
            touch_selection("", "old", ""),
            TouchSelection {
                update_host_label: false,
                update_label: false
            },
        );
        // 300-char request truncates to the stored 255 width.
        assert_eq!(normalize_host_label(&"z".repeat(300)).len(), 255);
        // Same label, empty stored label: label still backfills.
        assert_eq!(
            touch_selection("same", "same", ""),
            TouchSelection {
                update_host_label: false,
                update_label: true
            },
        );
    }

    /// The SeaORM enroll statements against constraint-free temp copies of
    /// their tables on one sqlx transaction (`TxConn`).
    #[tokio::test]
    async fn live_enroll_statements() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "runner",
            "pod",
            "projects",
            "workspaces",
            "dev_machine",
            "machine_token",
            "workspace_members",
            "users",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        let (user, ws, project, pod_id, machine) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO workspaces (id, slug, created_at) VALUES ('{ws}', 'ws-e', now())"),
            format!("INSERT INTO projects (id, identifier, workspace_id, created_at) VALUES ('{project}', 'ENR', '{ws}', now())"),
            format!(
                "INSERT INTO pod (id, name, project_id, is_default, created_at) VALUES \
                 ('{pod_id}', 'main', '{project}', true, now()), \
                 (gen_random_uuid(), 'gone', '{project}', false, now())"
            ),
            "UPDATE pod SET deleted_at = now() WHERE name = 'gone'".to_owned(),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, is_active, created_at) \
                 VALUES (gen_random_uuid(), '{ws}', '{user}', true, now())"
            ),
            format!("INSERT INTO users (id, password, is_active) VALUES ('{user}', 'pw', true)"),
            format!(
                "INSERT INTO dev_machine (id, owner_id, host_label, label, provisioning, visibility, \
                 created_at, updated_at) VALUES ('{machine}', '{user}', 'box', 'box', 'manual', 0, \
                 now(), now())"
            ),
        ] {
            sqlx::query(&sql).execute(&mut *tx).await.expect("seed");
        }
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let now = chrono::Utc::now();
        let runner_id = Uuid::new_v4();
        insert_runner(
            &conn,
            &NewRunner {
                id: runner_id,
                owner_id: user,
                workspace_id: ws,
                dev_machine_id: Some(machine),
                pod_id,
                name: "r1",
                host_label: "box",
                provisioning: "desktop_bundled",
                status: "offline",
                now,
            },
        )
        .await
        .expect("insert runner");
        runner::Entity::update_many()
            .col_expr(
                runner::Column::EnrollmentTokenHash,
                Expr::value("enroll-hash"),
            )
            .filter(runner::Column::Id.eq(runner_id))
            .exec(&conn)
            .await
            .expect("set hash");

        let rows = enroll_locked_runners(&conn, "enroll-hash")
            .await
            .expect("E1");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            (
                row.runner_id,
                row.workspace_slug.as_str(),
                row.pod_name.as_str(),
                row.project_identifier.as_str()
            ),
            (runner_id, "ws-e", "main", "ENR")
        );
        mark_runner_enrolled(
            &conn,
            runner_id,
            Some(machine),
            Some("renamed"),
            "box",
            "rh",
            "rf",
            now,
        )
        .await
        .expect("E2");
        let marked = runner::Entity::find_by_id(runner_id)
            .one(&conn)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(
            (
                marked.name.as_str(),
                marked.refresh_token_generation,
                marked.enrollment_token_hash.as_str()
            ),
            ("renamed", 1, "")
        );
        assert!(enroll_locked_runners(&conn, "enroll-hash")
            .await
            .expect("E1")
            .is_empty());

        let locked = lock_dev_machine_by_id(&conn, machine)
            .await
            .expect("D1")
            .expect("row");
        assert_eq!(
            (locked.owner_id, locked.provisioning.as_str()),
            (user, "manual")
        );
        assert_eq!(
            lock_dev_machine_by_owner_host(&conn, "box", user)
                .await
                .expect("D3")
                .map(|m| m.id),
            Some(machine)
        );
        touch_dev_machine(&conn, machine, None, Some("new-label"), now)
            .await
            .expect("touch");
        let touched = dev_machine::Entity::find_by_id(machine)
            .one(&conn)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(
            (touched.host_label.as_str(), touched.label.as_str()),
            ("box", "new-label")
        );

        assert!(
            !bootstrap_token_exists(&conn, Some(machine), "box", user, ws)
                .await
                .expect("probe")
        );
        assert_eq!(
            workspace_by_slug(&conn, "ws-e").await.expect("E6a"),
            Some((ws, "ws-e".to_owned()))
        );
        assert_eq!(
            memberships_for_inference(&conn, user).await.expect("E6b"),
            vec![(ws, "ws-e".to_owned())]
        );
        assert!(is_active_member(&conn, ws, user).await.expect("member"));
        assert_eq!(
            project_by_workspace_identifier(&conn, ws, "ENR")
                .await
                .expect("E6c"),
            Some((project, "ENR".to_owned()))
        );
        assert_eq!(
            pod_by_name(&conn, project, "main")
                .await
                .expect("E6d")
                .map(|p| p.0),
            Some(pod_id)
        );
        assert_eq!(
            pod_by_name(&conn, project, "gone").await.expect("E6d"),
            None
        );
        assert_eq!(
            default_pod_for_project(&conn, project)
                .await
                .expect("default")
                .map(|p| p.0),
            Some(pod_id)
        );
        assert_eq!(count_pod_runners(&conn, pod_id).await.expect("count"), 1);
        assert_eq!(
            desktop_reuse_probe(&conn, machine, user, pod_id, "desktop_bundled", ws)
                .await
                .expect("reuse")
                .map(|hit| (hit.0, hit.2)),
            Some((runner_id, 1))
        );
        assert_eq!(
            managed_cap_count(&conn, user, pod_id, "desktop_bundled", ws, "revoked")
                .await
                .expect("cap"),
            1
        );
        assert_eq!(
            managed_cap_count(&conn, user, pod_id, "desktop_bundled", ws, "offline")
                .await
                .expect("cap"),
            0
        );
        assert_eq!(
            session_user(&conn, user).await.expect("user"),
            Some((user, "pw".to_owned(), true))
        );
        assert!(user_exists(&conn, user).await.expect("user"));
        assert_eq!(
            live_workspace_slug(&conn, ws)
                .await
                .expect("slug")
                .as_deref(),
            Some("ws-e")
        );
    }
}
