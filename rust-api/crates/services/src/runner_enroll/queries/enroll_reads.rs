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
//!   ([`rotate_revoke_sql`] takes no user param in the machine branch);
//!   listed as a suspected bug.
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
//!   on update queries, so [`rotate_revoke_sql`] is a plain `UPDATE`.
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

use pidash_db::app_project::models::project as project_cols;
use pidash_db::runner_enroll::columns::{
    dev_machine as dm_cols, machine_token as mt_cols, pod as pod_cols, runner as r_cols,
    runner_force_refresh as ffr_cols,
};

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

/// `$1, $2, …, $n` placeholder list for an `n`-column `INSERT`.
fn placeholders(n: usize) -> String {
    (1..=n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// E1. Enroll tx: locked runner by one-time enrollment hash
// ---------------------------------------------------------------------------

/// E1 — enroll locked read (`enrollment.py:284-290`).
///
/// `Runner.objects.select_for_update().select_related("workspace",
/// "pod__project").get(enrollment_token_hash=token_hash)` inside
/// `transaction.atomic()`. Full `runner` + `workspaces` + `pod` +
/// `projects` rows (`INNER JOIN`: both FKs are non-nullable).
/// `.get()` clears ordering and (per QUIRK-get-no-limit) the `LIMIT 21`
/// multiplicity probe is omitted — handlers map row counts.
///
/// `$1` = enrollment token hash (`tokens.hash_token(presented)`).
///
/// Guards (handlers map these; bodies are handlers-owned): miss → 401
/// `invalid_or_expired_enrollment_token` (rolls back); `revoked_at` set
/// → 409 `runner_revoked`; `enrolled_at` set → 409
/// `enrollment_token_already_used` (`:291-305`).
pub fn enroll_locked_read_sql() -> String {
    format!(
        "SELECT {}, {}, {}, {} FROM \"runner\" \
         INNER JOIN \"workspaces\" ON (\"runner\".\"workspace_id\" = \"workspaces\".\"id\") \
         INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
         INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
         WHERE \"runner\".\"enrollment_token_hash\" = $1 FOR UPDATE",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
        qualified("workspaces", WORKSPACES_SELECT_COLUMNS),
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

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

/// E2 — mark enrolled (`enrollment.py:325-348`), still inside the enroll tx.
///
/// `runner.save(update_fields=...)`: `updated_at` is NOT in the list, so
/// `auto_now` does NOT bump it (fixture `enroll_E2_after`). `SET` order
/// is model-field order: `dev_machine_id`, then `name` 2nd ONLY when
/// `with_name` (the body supplied a non-blank name, `:345-347`), then
/// `host_label`, the four refresh columns, the two enrollment columns,
/// `enrolled_at`.
///
/// Without name — `$1` dev_machine_id (or NULL), `$2` host_label
/// (`request or stored`), `$3` new refresh hash, `$4` new refresh
/// fingerprint, `$5` generation (`1`), `$6` previous hash (`''`), `$7`
/// enrollment hash (`''`), `$8` enrollment fingerprint (`''`), `$9`
/// enrolled_at (`now()`), `$10` runner id. With name, `$2` is the name
/// and every later bind shifts by one (`$11` id).
pub fn enroll_mark_enrolled_sql(with_name: bool) -> String {
    if !with_name {
        let body = [
            "\"dev_machine_id\" = $1",
            "\"host_label\" = $2",
            "\"refresh_token_hash\" = $3",
            "\"refresh_token_fingerprint\" = $4",
            "\"refresh_token_generation\" = $5",
            "\"previous_refresh_token_hash\" = $6",
            "\"enrollment_token_hash\" = $7",
            "\"enrollment_token_fingerprint\" = $8",
            "\"enrolled_at\" = $9",
        ]
        .join(", ");
        return format!("UPDATE \"runner\" SET {body} WHERE \"runner\".\"id\" = $10");
    }
    let body = [
        "\"dev_machine_id\" = $1",
        "\"name\" = $2",
        "\"host_label\" = $3",
        "\"refresh_token_hash\" = $4",
        "\"refresh_token_fingerprint\" = $5",
        "\"refresh_token_generation\" = $6",
        "\"previous_refresh_token_hash\" = $7",
        "\"enrollment_token_hash\" = $8",
        "\"enrollment_token_fingerprint\" = $9",
        "\"enrolled_at\" = $10",
    ]
    .join(", ");
    format!("UPDATE \"runner\" SET {body} WHERE \"runner\".\"id\" = $11")
}

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

/// E6b — inferred workspace (`enrollment.py:639-643`).
///
/// `WorkspaceMember.objects.filter(member=user,
/// is_active=True).select_related("workspace").order_by("created_at")[:2]`:
/// the caller's active memberships, oldest first, probing 2 rows. The
/// base table carries the manager scope; the joined `workspaces` row
/// carries none. `is_active=True` renders bare (QUIRK-bare-boolean).
///
/// `$1` = caller (member) id.
///
/// 0 rows → 400 `no_workspace_membership`; 2 rows → 400
/// `workspace_slug_required`; 1 row → its workspace.
pub fn memberships_for_inference_sql() -> String {
    format!(
        "SELECT {}, {} FROM \"workspace_members\" \
         INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE (\"workspace_members\".\"deleted_at\" IS NULL \
         AND \"workspace_members\".\"is_active\" \
         AND \"workspace_members\".\"member_id\" = $1) \
         ORDER BY \"workspace_members\".\"created_at\" ASC LIMIT 2",
        qualified("workspace_members", WORKSPACE_MEMBERS_SELECT_COLUMNS),
        qualified("workspaces", WORKSPACES_SELECT_COLUMNS),
    )
}

/// E6c — project (`enrollment.py:664-669`).
///
/// `Project.objects.filter(workspace_id=ws, identifier=id).first()`:
/// scope + `-created_at` + `LIMIT 1`. `Q`-sorted: `identifier` binds
/// first although the source passes `workspace_id` first.
///
/// `$1` = project identifier, `$2` = workspace id.
///
/// Miss → 404 `project_not_found`.
pub fn project_by_workspace_identifier_sql() -> String {
    format!(
        "SELECT {} FROM \"projects\" \
         WHERE (\"projects\".\"deleted_at\" IS NULL \
         AND \"projects\".\"identifier\" = $1 AND \"projects\".\"workspace_id\" = $2) \
         ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

/// E6d — pod by explicit name (`enrollment.py:672-673`).
///
/// `Pod.objects.filter(project=project, name=name,
/// deleted_at__isnull=True).first()`: `PodManager` scope + the explicit
/// `deleted_at` conjunct (kept twice — QUIRK-duplicate-deleted-at) +
/// `Meta.ordering` (`-is_default, created_at`) + `LIMIT 1`.
///
/// `$1` = pod name, `$2` = project id.
///
/// Miss does NOT 404: the code falls through to
/// [`pod::DEFAULT_FOR_PROJECT_ID_SQL`](pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL)
/// (QUIRK-pod-fallthrough); only a missing default → 409
/// `project_has_no_default_pod` (`:676-680`).
pub fn pod_by_name_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" \
         WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"deleted_at\" IS NULL \
         AND \"pod\".\"name\" = $1 AND \"pod\".\"project_id\" = $2) \
         ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// E7. Create tx body: desktop reuse probe + cap count + insert
// ---------------------------------------------------------------------------

/// E7 — desktop-bundled reuse probe (`enrollment.py:709-720`).
///
/// `Runner.objects.select_for_update().filter(owner, workspace_id,
/// dev_machine, pod, provisioning=DESKTOP_BUNDLED,
/// revoked_at__isnull=True).first()`, inside the per-attempt tx.
/// `Q`-sorted `WHERE`, `Meta.ordering`, `LIMIT 1`.
///
/// `$1` dev_machine_id, `$2` owner_id, `$3` pod_id, `$4` provisioning
/// (`'desktop_bundled'`), `$5` workspace_id.
///
/// Hit → reuse the row (break the retry loop; nothing minted). Miss →
/// the cap count, then the insert.
pub fn desktop_reuse_probe_sql() -> String {
    format!(
        "SELECT {} FROM \"runner\" \
         WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"owner_id\" = $2 \
         AND \"runner\".\"pod_id\" = $3 AND \"runner\".\"provisioning\" = $4 \
         AND \"runner\".\"revoked_at\" IS NULL AND \"runner\".\"workspace_id\" = $5) \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
    )
}

/// E7 — managed cap count (`_managed_cap_error`, `enrollment.py:237-246`).
///
/// `Runner.objects.filter(owner, pod, workspace_id,
/// provisioning=DESKTOP_BUNDLED).exclude(status=REVOKED).count()`:
/// `.exclude()` renders `NOT (…)` appended after the sorted filters;
/// `.count()` renders `COUNT(*) AS "__count"` with ordering cleared.
///
/// `$1` owner_id, `$2` pod_id, `$3` provisioning (`'desktop_bundled'`),
/// `$4` workspace_id, `$5` status (`'revoked'`).
///
/// Count ≥ `MANAGED_RUNNER_MAX_PER_USER_PROJECT` (default 1) → 409
/// `managed_runner_limit` (rolls the attempt tx back).
pub fn managed_cap_count_sql() -> String {
    "SELECT COUNT(*) AS \"__count\" FROM \"runner\" \
     WHERE (\"runner\".\"owner_id\" = $1 AND \"runner\".\"pod_id\" = $2 \
     AND \"runner\".\"provisioning\" = $3 AND \"runner\".\"workspace_id\" = $4 \
     AND NOT (\"runner\".\"status\" = $5))"
        .to_string()
}

/// E7 — runner insert (`enrollment.py:726-735`), inside the attempt tx.
///
/// `Runner.objects.create(owner, workspace_id, dev_machine, pod, name,
/// host_label, provisioning, enrolled_at=now())`. `Runner.save()`'s
/// single-project auto-resolve is a no-op here (`pod` is always set —
/// `:676-680` 409s otherwise), so this is a plain 30-column `INSERT`
/// with every column bound (`$1..$30` in [`r_cols::COLUMNS`] order):
/// `$1` id (fresh uuid4), `$2` owner, `$3` workspace, `$4` dev machine
/// (or NULL — legacy path), `$5` pod, `$6` name (explicit or
/// `_next_auto_runner_name`), `$7` host label, `$8` provisioning
/// (inherited from the machine, NEVER from the body, `:701-704`), `$9`
/// visibility (`0`), `$10-12` refresh hash/fingerprint (`''`) +
/// generation (`0`), `$13` previous hash (`''`), `$14` signing key
/// version (`1`), `$15-16` enrollment hash/fingerprint (`''` — enrolled
/// at creation, no legacy token), `$17` enrolled_at (`now()`), `$18`
/// capabilities (`[]`), `$19` status (`'offline'`), `$20-22`
/// os/arch/version (`''`), `$23` dev_metadata (`{}`), `$24` protocol
/// version (`1`), `$25-26` heartbeat/free-worktrees (NULL), `$27-28`
/// created/updated (`now()`), `$29-30` revoked_at (NULL)/reason (`''`).
///
/// `IntegrityError` on `runner_unique_name_per_pod` → explicit name:
/// 409 `runner_name_taken`; auto name: retry (max 5) else 409
/// `could_not_allocate_runner_name` (`:750-758`).
pub fn runner_insert_sql() -> String {
    let cols = r_cols::COLUMNS
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({cols}) VALUES ({})",
        r_cols::TABLE,
        placeholders(r_cols::COLUMNS.len()),
    )
}

// ---------------------------------------------------------------------------
// B1/B2/B3. Token bootstrap + rotate
// ---------------------------------------------------------------------------

/// B1 — bootstrap locked-exists probe (`_maybe_mint_machine_token`,
/// `enrollment.py:143-155`), inside the enrollment/creation tx.
///
/// `MachineToken.objects.select_for_update().filter(**filters).first()`
/// with `Q`-sorted `WHERE`, `-created_at` ordering, `LIMIT 1`.
///
/// With a dev machine — `$1` dev_machine_id, `$2` user_id, `$3`
/// workspace_id (plus the unbound `revoked_at IS NULL`). Legacy
/// (host-label token) — `$1` host_label, `$2` user_id, `$3`
/// workspace_id (plus `revoked_at IS NULL AND dev_machine_id IS NULL`).
///
/// Hit → return `None` (no mint; the response omits `machine_token`).
pub fn bootstrap_probe_sql(with_dev_machine: bool) -> String {
    let select = qualified(mt_cols::TABLE, mt_cols::COLUMNS);
    let order = "ORDER BY \"machine_token\".\"created_at\" DESC LIMIT 1 FOR UPDATE";
    if with_dev_machine {
        format!(
            "SELECT {select} FROM \"machine_token\" \
             WHERE (\"machine_token\".\"dev_machine_id\" = $1 \
             AND \"machine_token\".\"revoked_at\" IS NULL \
             AND \"machine_token\".\"user_id\" = $2 \
             AND \"machine_token\".\"workspace_id\" = $3) {order}"
        )
    } else {
        format!(
            "SELECT {select} FROM \"machine_token\" \
             WHERE (\"machine_token\".\"dev_machine_id\" IS NULL \
             AND \"machine_token\".\"host_label\" = $1 \
             AND \"machine_token\".\"revoked_at\" IS NULL \
             AND \"machine_token\".\"user_id\" = $2 \
             AND \"machine_token\".\"workspace_id\" = $3) {order}"
        )
    }
}

/// B2 — bootstrap insert (`enrollment.py:157-171`), in a savepoint.
///
/// `MachineToken.objects.create(user, dev_machine, workspace,
/// host_label, token_hash, token_fingerprint, label, is_service=True)`:
/// plain 12-column `INSERT`, `$1..$12` in [`mt_cols::COLUMNS`] order —
/// `$1` id (fresh uuid4), `$2` user, `$3` dev machine (or NULL), `$4`
/// workspace, `$5` host label, `$6` token hash, `$7` fingerprint, `$8`
/// label ([`machine_token_label`]), `$9` is_service (`TRUE`), `$10`
/// created_at (`now()`), `$11-12` last_used/revoked (NULL).
///
/// Unique violation (the two partial unique indexes) → `ROLLBACK TO
/// SAVEPOINT`, return `None` (`:169-170`). `_rotate_machine_token`
/// reuses this same shape for its insert (`:193-202`).
pub fn machine_token_insert_sql() -> String {
    let cols = mt_cols::COLUMNS
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({cols}) VALUES ({})",
        mt_cols::TABLE,
        placeholders(mt_cols::COLUMNS.len()),
    )
}

/// B3 — rotate revoke (`_rotate_machine_token`, `enrollment.py:174-203`),
/// in the caller tx (NOT savepoint-guarded).
///
/// `MachineToken.objects.select_for_update().filter(**filters).update(revoked_at=now())`.
/// The `select_for_update()` is a no-op on updates
/// (QUIRK-lock-before-update). The `SET` bind comes first, then the
/// `Q`-sorted `WHERE`.
///
/// With a dev machine — `$1` revoked_at (`now()`), `$2` dev_machine_id,
/// `$3` workspace_id. NOTE the filter asymmetry vs B1: `user` is NOT in
/// the filter (BUG-rotate-user-omission, ported as-is). Legacy — `$1`
/// revoked_at, `$2` host_label, `$3` user_id, `$4` workspace_id (plus
/// the unbound `revoked_at IS NULL AND dev_machine_id IS NULL`).
///
/// The follow-up insert is [`machine_token_insert_sql`].
pub fn rotate_revoke_sql(with_dev_machine: bool) -> String {
    if with_dev_machine {
        "UPDATE \"machine_token\" SET \"revoked_at\" = $1 \
         WHERE (\"machine_token\".\"dev_machine_id\" = $2 \
         AND \"machine_token\".\"revoked_at\" IS NULL \
         AND \"machine_token\".\"workspace_id\" = $3)"
            .to_string()
    } else {
        "UPDATE \"machine_token\" SET \"revoked_at\" = $1 \
         WHERE (\"machine_token\".\"dev_machine_id\" IS NULL \
         AND \"machine_token\".\"host_label\" = $2 \
         AND \"machine_token\".\"revoked_at\" IS NULL \
         AND \"machine_token\".\"user_id\" = $3 \
         AND \"machine_token\".\"workspace_id\" = $4)"
            .to_string()
    }
}

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

/// D2 — dev-machine insert (`enrollment.py:86-99,111-118`), in a savepoint.
///
/// `DevMachine.objects.create(id?, owner, host_label, label,
/// last_seen_at=now())`: plain 10-column `INSERT`, `$1..$10` in
/// [`dm_cols::COLUMNS`] order — `$1` id (client id, or a fresh server
/// uuid4 on the D3 path), `$2` owner, `$3` host label (normalized —
/// possibly `""` on the with-id path), `$4` label (host `[:128]`),
/// `$5` visibility (`0`), `$6` provisioning (`'manual'`), `$7`
/// last_seen_at (`now()`), `$8` revoked_at (NULL), `$9-10`
/// created/updated (`now()`).
///
/// With-id path: `IntegrityError` (id race) → re-lock D1;
/// miss/foreign owner → ownership error; else touch + return (`:95-99`).
/// D3 path: `IntegrityError` (legacy owner/host constraint race) →
/// legacy reselect (same SQL as [`dev_machine_by_owner_host_sql`]);
/// return whatever won, even `None` (`:119-127`).
pub fn dev_machine_insert_sql() -> String {
    let cols = dm_cols::COLUMNS
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({cols}) VALUES ({})",
        dm_cols::TABLE,
        placeholders(dm_cols::COLUMNS.len()),
    )
}

/// D3 — dev-machine by (owner, host_label), locked, oldest first
/// (`enrollment.py:101-127`).
///
/// `DevMachine.objects.select_for_update().filter(owner, host_label,
/// revoked_at__isnull=True).order_by("created_at").first()`. Also the
/// legacy-race reselect (`:122-127`), which is textually identical.
///
/// `$1` host_label (normalized — the empty-label + no-id case returns
/// `None` BEFORE any SQL, `:101-102`, leaving `runner.dev_machine`
/// NULL), `$2` owner_id.
///
/// Hit → touch (D4) + return. Miss → D2 insert with a server uuid.
pub fn dev_machine_by_owner_host_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" \
         WHERE (\"dev_machine\".\"host_label\" = $1 \
         AND \"dev_machine\".\"owner_id\" = $2 \
         AND \"dev_machine\".\"revoked_at\" IS NULL) \
         ORDER BY \"dev_machine\".\"created_at\" ASC LIMIT 1 FOR UPDATE",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

/// D4 — the always-written touch fields (`_touch_dev_machine`,
/// `enrollment.py:60`). [`touch_dev_machine_sql`] emits these plus the
/// conditional `host_label`/`label` in Django model-field order.
pub const TOUCH_BASE_FIELDS: &[&str] = &["last_seen_at", "updated_at"];

/// D4 — `_touch_dev_machine` write (`enrollment.py:57-69`).
///
/// `machine.save(update_fields=[…])`: always `last_seen_at=now` +
/// `updated_at=now`, plus `host_label` when [`touch_selection`] says the
/// non-empty request label differs from stored, plus `label` when the
/// non-empty request label meets an empty stored label. `SET` order is
/// model-field order: `host_label`, `label`, `last_seen_at`,
/// `updated_at` — so the `$N` numbering depends on the flags:
///
/// * neither: `$1` last_seen_at, `$2` updated_at, `$3` id.
/// * host only: `$1` host_label, `$2` last_seen_at, `$3` updated_at, `$4` id.
/// * label only: `$1` label, `$2` last_seen_at, `$3` updated_at, `$4` id.
/// * both: `$1` host_label, `$2` label, `$3` last_seen_at, `$4`
///   updated_at, `$5` id.
pub fn touch_dev_machine_sql(update_host_label: bool, update_label: bool) -> String {
    let mut set: Vec<String> = Vec::new();
    let mut next: u32 = 1;
    if update_host_label {
        set.push(format!("\"host_label\" = ${next}"));
        next += 1;
    }
    if update_label {
        set.push(format!("\"label\" = ${next}"));
        next += 1;
    }
    set.push(format!("\"last_seen_at\" = ${next}"));
    next += 1;
    set.push(format!("\"updated_at\" = ${next}"));
    next += 1;
    format!(
        "UPDATE \"dev_machine\" SET {} WHERE \"dev_machine\".\"id\" = ${next}",
        set.join(", "),
    )
}

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
    truncate_chars(raw.trim(), 255)
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
    truncate_chars(raw.trim(), 128)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        "free_worktrees",
        "created_at",
        "updated_at",
        "revoked_at",
        "revoked_reason",
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

    #[test]
    fn e1_locks_runner_with_workspace_pod_project_joins() {
        let sql = enroll_locked_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" INNER JOIN \"workspaces\" ON (\"runner\".\"workspace_id\" = \"workspaces\".\"id\") \
             INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
             WHERE \"runner\".\"enrollment_token_hash\" = $1 FOR UPDATE",
        );
        // Full rows in Django select order: runner, workspaces, pod, projects.
        let expected = format!(
            "{}, {}, {}, {}",
            expected_list("runner", RUNNER_COLS),
            expected_list("workspaces", WORKSPACES_SELECT_COLUMNS),
            expected_list("pod", POD_COLS),
            expected_list("projects", project_cols::COLUMNS),
        );
        assert_eq!(list, expected);
        // .get(): no ordering, no LIMIT (handlers map row counts).
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
        for fragment in [
            fixture_fragment("INNER JOIN \"workspaces\""),
            fixture_fragment("INNER JOIN \"pod\""),
            fixture_fragment("INNER JOIN \"projects\""),
            fixture_fragment("\"enrollment_token_hash\" = $1"),
            fixture_fragment("FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- E2 --

    #[test]
    fn e2_emits_model_order_set_without_updated_at() {
        assert_eq!(
            ENROLL_UPDATE_FIELDS,
            &[
                "dev_machine",
                "host_label",
                "enrolled_at",
                "enrollment_token_hash",
                "enrollment_token_fingerprint",
                "refresh_token_hash",
                "refresh_token_fingerprint",
                "refresh_token_generation",
                "previous_refresh_token_hash",
            ]
        );
        assert_eq!(
            enroll_mark_enrolled_sql(false),
            "UPDATE \"runner\" SET \"dev_machine_id\" = $1, \"host_label\" = $2, \
             \"refresh_token_hash\" = $3, \"refresh_token_fingerprint\" = $4, \
             \"refresh_token_generation\" = $5, \"previous_refresh_token_hash\" = $6, \
             \"enrollment_token_hash\" = $7, \"enrollment_token_fingerprint\" = $8, \
             \"enrolled_at\" = $9 WHERE \"runner\".\"id\" = $10",
        );
        // updated_at NOT bumped (fixture enroll_E2_after).
        assert!(!enroll_mark_enrolled_sql(false).contains("updated_at"));
        assert!(!enroll_mark_enrolled_sql(true).contains("updated_at"));
        for fragment in [
            fixture_fragment("\"dev_machine_id\" = $1"),
            fixture_fragment("\"host_label\" = $2"),
            fixture_fragment("\"refresh_token_hash\" = "),
            fixture_fragment("\"enrolled_at\" = "),
        ] {
            assert_builder_contains(&enroll_mark_enrolled_sql(false), &fragment);
        }
    }

    #[test]
    fn e2_name_slot_is_second_when_body_named() {
        assert_eq!(
            enroll_mark_enrolled_sql(true),
            "UPDATE \"runner\" SET \"dev_machine_id\" = $1, \"name\" = $2, \"host_label\" = $3, \
             \"refresh_token_hash\" = $4, \"refresh_token_fingerprint\" = $5, \
             \"refresh_token_generation\" = $6, \"previous_refresh_token_hash\" = $7, \
             \"enrollment_token_hash\" = $8, \"enrollment_token_fingerprint\" = $9, \
             \"enrolled_at\" = $10 WHERE \"runner\".\"id\" = $11",
        );
        // The SET covers exactly the update_fields (+ name), whatever the order.
        for col in ENROLL_UPDATE_FIELDS.iter().copied().chain(["name"]) {
            let attr = if col == "dev_machine" {
                "dev_machine_id"
            } else {
                col
            };
            assert!(
                enroll_mark_enrolled_sql(true).contains(&format!("\"{attr}\" = ")),
                "{col}"
            );
        }
    }

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

    #[test]
    fn e6b_probes_two_membership_rows_oldest_first() {
        let sql = memberships_for_inference_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"workspace_members\" INNER JOIN \"workspaces\" ON \
             (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") \
             WHERE (\"workspace_members\".\"deleted_at\" IS NULL \
             AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = $1) \
             ORDER BY \"workspace_members\".\"created_at\" ASC LIMIT 2",
        );
        assert_eq!(
            list,
            format!(
                "{}, {}",
                expected_list("workspace_members", WORKSPACE_MEMBERS_SELECT_COLUMNS),
                expected_list("workspaces", WORKSPACES_SELECT_COLUMNS),
            )
        );
        for fragment in [
            fixture_fragment("FROM \"workspace_members\""),
            fixture_fragment("\"member_id\" = $1"),
            fixture_fragment("is_active"),
            fixture_fragment("LIMIT 2"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn e6c_binds_identifier_before_workspace() {
        let sql = project_by_workspace_identifier_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL \
             AND \"projects\".\"identifier\" = $1 AND \"projects\".\"workspace_id\" = $2) \
             ORDER BY \"projects\".\"created_at\" DESC LIMIT 1",
        );
        assert_eq!(list, expected_list("projects", project_cols::COLUMNS));
        for fragment in [
            fixture_fragment("FROM \"projects\""),
            fixture_fragment("\"identifier\" = "),
            fixture_fragment("\"workspace_id\" = "),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn e6d_explicit_keeps_duplicate_deleted_at() {
        let sql = pod_by_name_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"deleted_at\" IS NULL \
             AND \"pod\".\"name\" = $1 AND \"pod\".\"project_id\" = $2) \
             ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1",
        );
        assert_eq!(list, expected_list("pod", POD_COLS));
        for fragment in [
            fixture_fragment("\"deleted_at\" IS NULL"),
            fixture_fragment("\"name\" = "),
            fixture_fragment("\"project_id\" = "),
            fixture_fragment("is_default"),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
        // Second leg is the merged default-pod read (db-owned, PIDASHCONV-581).
        let default = pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL;
        assert!(default.contains("\"pod\".\"project_id\" = $1"), "{default}");
        assert!(default.contains("\"pod\".\"is_default\""), "{default}");
        assert!(default.contains("LIMIT 1"), "{default}");
    }

    // -- E7 --

    #[test]
    fn e7_reuse_probe_locks_sorted_filters() {
        let sql = desktop_reuse_probe_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"owner_id\" = $2 \
             AND \"runner\".\"pod_id\" = $3 AND \"runner\".\"provisioning\" = $4 \
             AND \"runner\".\"revoked_at\" IS NULL AND \"runner\".\"workspace_id\" = $5) \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("runner", RUNNER_COLS));
        for fragment in [
            fixture_fragment("\"owner_id\" = "),
            fixture_fragment("\"provisioning\" = "),
            fixture_fragment("\"revoked_at\" IS NULL"),
            fixture_fragment("FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn e7_cap_count_excludes_revoked() {
        assert_eq!(
            managed_cap_count_sql(),
            "SELECT COUNT(*) AS \"__count\" FROM \"runner\" \
             WHERE (\"runner\".\"owner_id\" = $1 AND \"runner\".\"pod_id\" = $2 \
             AND \"runner\".\"provisioning\" = $3 AND \"runner\".\"workspace_id\" = $4 \
             AND NOT (\"runner\".\"status\" = $5))",
        );
        for fragment in [
            fixture_fragment("COUNT(*)"),
            fixture_fragment("\"owner_id\" = "),
            fixture_fragment("status"),
        ] {
            assert_builder_contains(&managed_cap_count_sql(), &fragment);
        }
    }

    #[test]
    fn e7_insert_binds_all_thirty_columns() {
        let sql = runner_insert_sql();
        let cols = RUNNER_COLS
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            sql,
            format!(
                "INSERT INTO \"runner\" ({cols}) VALUES ({})",
                (1..=30)
                    .map(|i| format!("${i}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        );
        for fragment in [
            fixture_fragment("INSERT INTO \"runner\""),
            fixture_fragment("\"enrolled_at\""),
            fixture_fragment("VALUES"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- B1/B2/B3 --

    #[test]
    fn b1_probe_sorts_filters_with_limit_for_update() {
        let machine = bootstrap_probe_sql(true);
        let list = assert_suffix(
            &machine,
            "FROM \"machine_token\" WHERE (\"machine_token\".\"dev_machine_id\" = $1 \
             AND \"machine_token\".\"revoked_at\" IS NULL AND \"machine_token\".\"user_id\" = $2 \
             AND \"machine_token\".\"workspace_id\" = $3) \
             ORDER BY \"machine_token\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("machine_token", TOKEN_COLS));
        let legacy = bootstrap_probe_sql(false);
        let legacy_list = assert_suffix(
            &legacy,
            "FROM \"machine_token\" WHERE (\"machine_token\".\"dev_machine_id\" IS NULL \
             AND \"machine_token\".\"host_label\" = $1 AND \"machine_token\".\"revoked_at\" IS NULL \
             AND \"machine_token\".\"user_id\" = $2 AND \"machine_token\".\"workspace_id\" = $3) \
             ORDER BY \"machine_token\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(legacy_list, expected_list("machine_token", TOKEN_COLS));
        for fragment in [
            fixture_fragment("\"user_id\" = "),
            fixture_fragment("\"dev_machine_id\" = "),
            fixture_fragment("\"host_label\" = "),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&format!("{machine} {legacy}"), &fragment);
        }
    }

    #[test]
    fn b2_insert_binds_all_twelve_columns() {
        let sql = machine_token_insert_sql();
        let cols = TOKEN_COLS
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            sql,
            format!(
                "INSERT INTO \"machine_token\" ({cols}) VALUES ({})",
                (1..=12)
                    .map(|i| format!("${i}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        );
        for fragment in [
            fixture_fragment("INSERT INTO \"machine_token\""),
            fixture_fragment("\"token_hash\""),
            fixture_fragment("\"is_service\""),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn b3_revoke_omits_user_on_machine_branch() {
        // Suspected bug, ported as-is: no user_id on the machine branch.
        assert_eq!(
            rotate_revoke_sql(true),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 \
             WHERE (\"machine_token\".\"dev_machine_id\" = $2 \
             AND \"machine_token\".\"revoked_at\" IS NULL \
             AND \"machine_token\".\"workspace_id\" = $3)",
        );
        assert!(!rotate_revoke_sql(true).contains("user_id"));
        assert_eq!(
            rotate_revoke_sql(false),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 \
             WHERE (\"machine_token\".\"dev_machine_id\" IS NULL \
             AND \"machine_token\".\"host_label\" = $2 \
             AND \"machine_token\".\"revoked_at\" IS NULL \
             AND \"machine_token\".\"user_id\" = $3 AND \"machine_token\".\"workspace_id\" = $4)",
        );
        // Revokes every active row in scope — no LIMIT (fixture rotate_B3_after).
        assert!(!rotate_revoke_sql(true).contains("LIMIT"));
        assert!(!rotate_revoke_sql(false).contains("LIMIT"));
        for fragment in [
            fixture_fragment("\"revoked_at\" = "),
            fixture_fragment("\"workspace_id\" = "),
        ] {
            assert_builder_contains(
                &format!("{} {}", rotate_revoke_sql(true), rotate_revoke_sql(false)),
                &fragment,
            );
        }
    }

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

    #[test]
    fn d2_insert_binds_all_ten_columns() {
        let sql = dev_machine_insert_sql();
        let cols = MACHINE_COLS
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            sql,
            format!(
                "INSERT INTO \"dev_machine\" ({cols}) VALUES ({})",
                (1..=10)
                    .map(|i| format!("${i}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        );
        for fragment in [
            fixture_fragment("INSERT INTO \"dev_machine\""),
            fixture_fragment("\"owner_id\""),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn d3_selects_by_owner_host_oldest_first() {
        let sql = dev_machine_by_owner_host_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"dev_machine\" WHERE (\"dev_machine\".\"host_label\" = $1 \
             AND \"dev_machine\".\"owner_id\" = $2 AND \"dev_machine\".\"revoked_at\" IS NULL) \
             ORDER BY \"dev_machine\".\"created_at\" ASC LIMIT 1 FOR UPDATE",
        );
        assert_eq!(list, expected_list("dev_machine", MACHINE_COLS));
        for fragment in [
            fixture_fragment("\"host_label\" = "),
            fixture_fragment("\"owner_id\" = "),
            fixture_fragment("\"created_at\" ASC"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn d4_variants_number_binds_in_model_order() {
        assert_eq!(TOUCH_BASE_FIELDS, &["last_seen_at", "updated_at"]);
        assert_eq!(
            touch_dev_machine_sql(false, false),
            "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1, \"updated_at\" = $2 \
             WHERE \"dev_machine\".\"id\" = $3",
        );
        assert_eq!(
            touch_dev_machine_sql(true, false),
            "UPDATE \"dev_machine\" SET \"host_label\" = $1, \"last_seen_at\" = $2, \
             \"updated_at\" = $3 WHERE \"dev_machine\".\"id\" = $4",
        );
        assert_eq!(
            touch_dev_machine_sql(false, true),
            "UPDATE \"dev_machine\" SET \"label\" = $1, \"last_seen_at\" = $2, \
             \"updated_at\" = $3 WHERE \"dev_machine\".\"id\" = $4",
        );
        assert_eq!(
            touch_dev_machine_sql(true, true),
            "UPDATE \"dev_machine\" SET \"host_label\" = $1, \"label\" = $2, \
             \"last_seen_at\" = $3, \"updated_at\" = $4 WHERE \"dev_machine\".\"id\" = $5",
        );
        for fragment in [
            fixture_fragment("\"last_seen_at\" = "),
            fixture_fragment("\"updated_at\" = "),
        ] {
            assert_builder_contains(&touch_dev_machine_sql(true, true), &fragment);
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

    #[test]
    fn fixture_rows_shape_is_as_documented() {
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert_eq!(rows["fixture_id"], "D13-F5");
        // E2_after pins updated_at UNCHANGED — builders must not name it.
        assert!(rows["enroll_E2_after"]["updated_at"]
            .as_str()
            .unwrap()
            .contains("UNCHANGED"));
        assert!(!enroll_mark_enrolled_sql(true).contains("updated_at"));
        // E1_hit guards read revoked_at/enrolled_at off the locked row.
        assert!(rows["enroll_E1_hit"]["runner"].get("revoked_at").is_some());
        assert!(rows["enroll_E1_hit"]["runner"].get("enrolled_at").is_some());
        assert!(RUNNER_COLS.contains(&"revoked_at"));
        assert!(RUNNER_COLS.contains(&"enrolled_at"));
        // B3_after pins set-wide revoke (no LIMIT) + the new-row shape.
        assert!(rows["rotate_B3_after"]["old_rows"]
            .as_str()
            .unwrap()
            .contains("regardless of count"));
        assert_eq!(rows["rotate_B3_after"]["new_row"]["is_service"], true);
        assert_eq!(
            machine_token_label("macbook-pro"),
            rows["rotate_B3_after"]["new_row"]["label"]
                .as_str()
                .unwrap(),
        );
    }
}
