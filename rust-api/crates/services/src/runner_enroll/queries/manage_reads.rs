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
//!   reuses [`scoped_machine_read_sql`] + the M4 probes (the delete
//!   read is text-identical to `_scoped_machine`'s unlocked read).
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

use pidash_db::app_project::models::project as project_cols;
use pidash_db::runner_enroll::columns::{
    dev_machine as dm_cols, machine_token as mt_cols, pod as pod_cols, runner as r_cols,
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
/// Tuple order is the `status__in` bind order (`$3`-`$10`).
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

/// Python `str.strip()` parity (same set as
/// `super::enroll_reads`' private helper, which this module cannot
/// reuse without touching that file): Python strips `str.isspace()`
/// characters — Unicode `White_Space` plus U+001C-U+001F and U+0085 —
/// while Rust `trim()` strips `White_Space` only.
fn py_strip(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{85}' || ('\u{1c}'..='\u{1f}').contains(&c)
    })
}

// ---------------------------------------------------------------------------
// M1. Control-online subquery
// ---------------------------------------------------------------------------

/// M1 — control-online `Exists` fragment (`runners.py:52-59`), shared
/// by the M3 list and the M5 serialize (both bind it as `$12`/`$13`).
///
/// `Exists(MachineSession where dev_machine = OuterRef(pk), active,
/// last_seen_at >= cutoff)`: Django aliases the table `U0`, renders
/// the `OuterRef("pk")` in parens, binds the `SELECT 1` constant
/// (`one`) and the cutoff (`cutoff`), and appends `LIMIT 1`.
pub fn control_online_fragment(one: usize, cutoff: usize) -> String {
    format!(
        "EXISTS(SELECT ${one} AS \"a\" FROM \"{MACHINE_SESSION_TABLE}\" U0 \
         WHERE (U0.\"dev_machine_id\" = (\"dev_machine\".\"id\") \
         AND U0.\"last_seen_at\" >= ${cutoff} AND U0.\"revoked_at\" IS NULL) \
         LIMIT 1) AS \"control_online\""
    )
}

// ---------------------------------------------------------------------------
// M2/M3. Machine list + annotations
// ---------------------------------------------------------------------------

/// M3 — dev-machine list (`runners.py:73-125`).
///
/// `DevMachine.objects.filter(Q(id__in=M2a) | Q(id__in=M2b), owner,
/// visibility=PRIVATE)` annotated with `runner_count` /
/// `online_runner_count` / `last_heartbeat_at` (all `FILTER`ed on the
/// workspace+owner+private predicate; the online count additionally
/// requires `revoked_at IS NULL` + `status IN ('online','busy')`) and
/// [`control_online_fragment`], ordered `-last_seen_at, -created_at`
/// (NULL `last_seen_at` sorts FIRST under `DESC` — the Postgres
/// default, ported as the bare `ORDER BY`, not spelled out).
///
/// The M2 id probes nest as `IN`-subqueries (QUIRK above): the runner
/// leg filters `workspace, owner, visibility=0, dev_machine NOT NULL`,
/// the token leg `workspace, user, dev_machine NOT NULL`.
///
/// Binds `$1`-`$20`: three annotation triples
/// (`owner, visibility, workspace`; the online triple carries
/// `online=$7, busy=$8`), the `Exists` constant (`$12`) + cutoff
/// (`$13`), the M2a triple (`$14`-`$16`), the M2b pair (`$17`-`$18`),
/// the outer `owner` (`$19`) + `visibility` (`$20`).
///
/// Guards (handlers map these): missing `?workspace=` → 400
/// `workspace is required`; non-member → 403 `forbidden`.
pub fn machine_list_sql() -> String {
    let exists = control_online_fragment(12, 13);
    format!(
        "SELECT {}, \
         COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $1 \
         AND \"runner\".\"visibility\" = $2 AND \"runner\".\"workspace_id\" = $3)) AS \"runner_count\", \
         COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $4 \
         AND \"runner\".\"visibility\" = $5 AND \"runner\".\"workspace_id\" = $6 \
         AND \"runner\".\"revoked_at\" IS NULL AND \"runner\".\"status\" IN ($7, $8))) AS \"online_runner_count\", \
         MAX(\"runner\".\"last_heartbeat_at\") FILTER (WHERE (\"runner\".\"owner_id\" = $9 \
         AND \"runner\".\"visibility\" = $10 AND \"runner\".\"workspace_id\" = $11)) AS \"last_heartbeat_at\", \
         {exists} \
         FROM \"dev_machine\" LEFT OUTER JOIN \"runner\" ON (\"dev_machine\".\"id\" = \"runner\".\"dev_machine_id\") \
         WHERE ((\"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"runner\" U0 \
         WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"owner_id\" = $14 \
         AND U0.\"visibility\" = $15 AND U0.\"workspace_id\" = $16)) \
         OR \"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"machine_token\" U0 \
         WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"user_id\" = $17 \
         AND U0.\"workspace_id\" = $18))) \
         AND \"dev_machine\".\"owner_id\" = $19 AND \"dev_machine\".\"visibility\" = $20) \
         GROUP BY \"dev_machine\".\"id\" \
         ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

// ---------------------------------------------------------------------------
// M4. Scope probes
// ---------------------------------------------------------------------------

/// M4a — machine-scope runner probe (`runners.py:136-141`).
///
/// `Runner.objects.filter(workspace, owner, visibility=PRIVATE,
/// dev_machine=machine).exists()`: `SELECT $1 AS "a" … LIMIT 1` with
/// ordering cleared. `$1` is the constant `1`, then `$2` machine id,
/// `$3` owner id, `$4` visibility (`0`), `$5` workspace id (Q-sorted).
pub fn machine_scope_runner_probe_sql() -> String {
    "SELECT $1 AS \"a\" FROM \"runner\" \
     WHERE (\"runner\".\"dev_machine_id\" = $2 AND \"runner\".\"owner_id\" = $3 \
     AND \"runner\".\"visibility\" = $4 AND \"runner\".\"workspace_id\" = $5) \
     LIMIT 1"
        .to_string()
}

/// M4b — machine-scope token probe (`runners.py:142-146`).
///
/// `MachineToken.objects.filter(workspace, user,
/// dev_machine=machine).exists()`. `$1` is the constant `1`, then
/// `$2` machine id, `$3` user id, `$4` workspace id (Q-sorted).
pub fn machine_scope_token_probe_sql() -> String {
    format!(
        "SELECT $1 AS \"a\" FROM \"{}\" \
         WHERE (\"{}\".\"dev_machine_id\" = $2 \
         AND \"{}\".\"user_id\" = $3 \
         AND \"{}\".\"workspace_id\" = $4) \
         LIMIT 1",
        mt_cols::TABLE,
        mt_cols::TABLE,
        mt_cols::TABLE,
        mt_cols::TABLE,
    )
}

// ---------------------------------------------------------------------------
// M5. Single-machine serialize
// ---------------------------------------------------------------------------

/// M5 — single-machine serialize re-read (`runners.py:150-170`).
///
/// The M3 annotations over `DevMachine.objects.filter(pk)` +
/// `.first()` (keeps `Meta.ordering`, adds `LIMIT 1`). Binds `$1`-`$13`
/// exactly as [`machine_list_sql`]; `$14` is the machine pk.
///
/// When the row vanished, the view serializes the UNANNOTATED
/// instance (QUIRK-m5-fallback) — handlers own that branch.
pub fn machine_serialize_sql() -> String {
    let exists = control_online_fragment(12, 13);
    format!(
        "SELECT {}, \
         COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $1 \
         AND \"runner\".\"visibility\" = $2 AND \"runner\".\"workspace_id\" = $3)) AS \"runner_count\", \
         COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $4 \
         AND \"runner\".\"visibility\" = $5 AND \"runner\".\"workspace_id\" = $6 \
         AND \"runner\".\"revoked_at\" IS NULL AND \"runner\".\"status\" IN ($7, $8))) AS \"online_runner_count\", \
         MAX(\"runner\".\"last_heartbeat_at\") FILTER (WHERE (\"runner\".\"owner_id\" = $9 \
         AND \"runner\".\"visibility\" = $10 AND \"runner\".\"workspace_id\" = $11)) AS \"last_heartbeat_at\", \
         {exists} \
         FROM \"dev_machine\" LEFT OUTER JOIN \"runner\" ON (\"dev_machine\".\"id\" = \"runner\".\"dev_machine_id\") \
         WHERE \"dev_machine\".\"id\" = $14 \
         GROUP BY \"dev_machine\".\"id\" \
         ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC LIMIT 1",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

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
/// `$1`/`$2` = the single `now()` (bound twice — `auto_now` uses the
/// same timestamp), `$3` = machine id.
pub fn machine_revoke_sql() -> String {
    "UPDATE \"dev_machine\" SET \"revoked_at\" = $1, \"updated_at\" = $2 \
     WHERE \"dev_machine\".\"id\" = $3"
        .to_string()
}

/// M6/M7 — machine-token revoke update (`runners.py:198, 238`).
///
/// Both call sites run the identical ORM call —
/// `MachineToken.objects.filter(dev_machine=machine,
/// revoked_at__isnull=True).update(revoked_at=now)` — so one builder
/// serves both. `$1` = `now()`, `$2` = machine id.
pub fn machine_tokens_revoke_sql() -> String {
    format!(
        "UPDATE \"{}\" SET \"revoked_at\" = $1 \
         WHERE (\"{}\".\"dev_machine_id\" = $2 \
         AND \"{}\".\"revoked_at\" IS NULL)",
        mt_cols::TABLE,
        mt_cols::TABLE,
        mt_cols::TABLE,
    )
}

/// M6 — locked active-runner list (`runners.py:199`).
///
/// `list(Runner.objects.select_for_update().filter(dev_machine,
/// revoked_at__isnull=True))`: full rows, `Meta.ordering` kept, NO
/// `LIMIT` (a list, not `.first()`), `FOR UPDATE`. `$1` = machine id.
/// Handlers then emit revoke frames BEFORE the per-runner cascade
/// (services-C) — order pinned there, not here.
pub fn machine_runners_locked_list_sql() -> String {
    format!(
        "SELECT {} FROM \"runner\" \
         WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"revoked_at\" IS NULL) \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
         FOR UPDATE",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
    )
}

/// M7 — active-runner id list (`runners.py:239-241`).
///
/// `Runner.objects.filter(dev_machine,
/// revoked_at__isnull=True).values_list("pk", flat=True)`:
/// bare ids, `Meta.ordering` kept, no `LIMIT`. `$1` = machine id.
/// Per id the handler emits a revoke frame + closes the session;
/// the runner ROWS are untouched (QUIRK-rotate-keeps-runners).
///
/// Guards: revoked machine → 409 `dev_machine_revoked` (checked
/// BEFORE this read, `:231-235`).
pub fn machine_runner_ids_sql() -> String {
    "SELECT \"runner\".\"id\" FROM \"runner\" \
     WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"revoked_at\" IS NULL) \
     ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC"
        .to_string()
}

// ---------------------------------------------------------------------------
// R1. Runner list
// ---------------------------------------------------------------------------

/// R1 — runner list (`runners.py:302-333`).
///
/// `Runner.objects.filter(workspace_id)` +
/// `runner_visible_to_user_q` (`owner_id = $2 AND visibility = $3`;
/// the anonymous `pk__isnull` branch never reaches this SQL — the view
/// is `IsAuthenticated`, and the kernel denies anon by fact) +
/// `select_related("pod__project", "dev_machine")` +
/// `order_by("-updated_at")`, then in call order: `?pod=` equality
/// (`pod`), the bundled exclusion (`exclude_bundled`, rendered as
/// `NOT (provisioning = $N)`), `?project=` equality on the joined pod
/// (`project`, reusing the select_related join — no new `JOIN`).
///
/// Flags: `pod` = `?pod=<uuid>` present (no existence check — an
/// unknown pod yields `[]`); `exclude_bundled` = `!`[`include_bundled`]
/// of the raw query value; `project` = `?project=<uuid>` present.
/// `$1`-`$3` are always `workspace, owner, visibility`; each active
/// filter appends the next `$N` in call order.
///
/// Join order follows QUIRK-join-order (`project` → `pod, projects,
/// dev_machine`; `pod` without `project` → `pod, dev_machine,
/// projects`; otherwise `dev_machine, pod, projects`); the select
/// list never moves (runner, dev_machine, pod, projects).
///
/// Guards: missing `?workspace=` → 400; non-member → 403.
pub fn runner_list_sql(pod: bool, exclude_bundled: bool, project: bool) -> String {
    let joins = if project {
        "INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
         INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
         LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\")"
    } else if pod {
        "INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
         LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
         INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\")"
    } else {
        "LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
         INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
         INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\")"
    };
    let mut where_ = String::from(
        "\"runner\".\"workspace_id\" = $1 AND \"runner\".\"owner_id\" = $2 \
         AND \"runner\".\"visibility\" = $3",
    );
    let mut next = 4;
    if pod {
        where_.push_str(&format!(" AND \"runner\".\"pod_id\" = ${next}"));
        next += 1;
    }
    if exclude_bundled {
        where_.push_str(&format!(" AND NOT (\"runner\".\"provisioning\" = ${next})"));
        next += 1;
    }
    if project {
        where_.push_str(&format!(" AND \"pod\".\"project_id\" = ${next}"));
    }
    format!(
        "SELECT {}, {}, {}, {} FROM \"runner\" {joins} \
         WHERE ({where_}) ORDER BY \"runner\".\"updated_at\" DESC",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

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

/// R2 — runner detail read (`runners.py:342-345`, `_get_runner`).
///
/// `Runner.objects.select_related("pod__project",
/// "dev_machine").filter(pk).first()`: the R1 select/join shape
/// without the project join reorder, single-`WHERE` (no parens),
/// `Meta.ordering` + `LIMIT 1`. `$1` = runner id.
pub fn runner_detail_sql() -> String {
    format!(
        "SELECT {}, {}, {}, {} FROM \"runner\" \
         LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
         INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
         INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
         WHERE \"runner\".\"id\" = $1 \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
        qualified(project_cols::TABLE, project_cols::COLUMNS),
    )
}

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

/// R3 — busy-guard probe (`runners.py:409-412`).
///
/// Runs ONLY for a real move ([`is_real_move`]).
/// `AgentRun.objects.filter(Q(runner=runner) | Q(pinned_runner=runner),
/// status__in=NON_TERMINAL_STATUSES).exists()`: `$1` is the constant
/// `1`, the runner id binds TWICE (`$2`/`$3`, one per `OR` leg), then
/// the eight [`NON_TERMINAL_STATUSES`] as `$4`-`$11` in tuple order.
///
/// True → 409 `runner_busy` (`runner has an in-flight or queued run;
/// wait for it to finish or cancel it first`). Unpinned queued runs
/// in the pod do NOT block (no runner/pinned link to this runner).
pub fn runner_busy_guard_sql() -> String {
    format!(
        "SELECT $1 AS \"a\" FROM \"{AGENT_RUN_TABLE}\" \
         WHERE ((\"{AGENT_RUN_TABLE}\".\"runner_id\" = $2 \
         OR \"{AGENT_RUN_TABLE}\".\"pinned_runner_id\" = $3) \
         AND \"{AGENT_RUN_TABLE}\".\"status\" IN ($4, $5, $6, $7, $8, $9, $10, $11)) \
         LIMIT 1"
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

/// R3 — patch write (`runners.py:422, 424`).
///
/// `runner.save(update_fields=set)` in model order (`pod_id`, `name`,
/// `updated_at`): `with_pod && with_name` → `$1` pod, `$2` name,
/// `$3` now, `$4` id; pod-only → `$1` pod, `$2` now, `$3` id;
/// name-only → `$1` name, `$2` now, `$3` id. The `(false, false)`
/// shape (`updated_at`-only) is what Django would emit for
/// `update_fields=["updated_at"]`; the view never calls it (no save
/// runs when the body carries neither key).
pub fn runner_patch_sql(with_pod: bool, with_name: bool) -> String {
    let mut sets = Vec::with_capacity(3);
    let mut next = 1;
    if with_pod {
        sets.push(format!("\"pod_id\" = ${next}"));
        next += 1;
    }
    if with_name {
        sets.push(format!("\"name\" = ${next}"));
        next += 1;
    }
    sets.push(format!("\"updated_at\" = ${next}"));
    next += 1;
    format!(
        "UPDATE \"runner\" SET {} WHERE \"runner\".\"id\" = ${next}",
        sets.join(", ")
    )
}

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

/// `_scoped_machine` read (`machine_commands.py:63-73`).
///
/// `DevMachine.objects.filter(pk).first()`: full row, `Meta.ordering`,
/// `LIMIT 1` — UNLOCKED (unlike [`machine_locked_read_sql`]). `$1` =
/// machine id. Miss or out-of-scope (M4 probes) → 404 `not found`;
/// `revoked_at` set → 409 `dev_machine_revoked`. The delete endpoint
/// (`runners.py:281`) runs this same read before its service call.
pub fn scoped_machine_read_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 \
         ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC \
         LIMIT 1",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn m1_exists_shape_is_django_exact() {
        assert_eq!(
            control_online_fragment(12, 13),
            "EXISTS(SELECT $12 AS \"a\" FROM \"machine_session\" U0 \
             WHERE (U0.\"dev_machine_id\" = (\"dev_machine\".\"id\") \
             AND U0.\"last_seen_at\" >= $13 AND U0.\"revoked_at\" IS NULL) \
             LIMIT 1) AS \"control_online\"",
        );
        // Numbering is positional: other slots render the same shape.
        assert!(control_online_fragment(1, 2).contains("SELECT $1 AS \"a\""));
        assert!(control_online_fragment(1, 2).contains(">= $2"));
        for fragment in [
            fixture_fragment("EXISTS("),
            fixture_fragment("FROM \"machine_session\""),
            fixture_fragment("AS \"control_online\""),
            fixture_fragment("\"revoked_at\" IS NULL"),
            fixture_fragment("\"last_seen_at\" >="),
        ] {
            assert_builder_contains(&control_online_fragment(12, 13), &fragment);
        }
    }

    // -- M3 --

    #[test]
    fn m3_list_emits_annotations_subqueries_order() {
        let sql = machine_list_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"dev_machine\" LEFT OUTER JOIN \"runner\" \
             ON (\"dev_machine\".\"id\" = \"runner\".\"dev_machine_id\") \
             WHERE ((\"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"runner\" U0 \
             WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"owner_id\" = $14 \
             AND U0.\"visibility\" = $15 AND U0.\"workspace_id\" = $16)) \
             OR \"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"machine_token\" U0 \
             WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"user_id\" = $17 \
             AND U0.\"workspace_id\" = $18))) \
             AND \"dev_machine\".\"owner_id\" = $19 AND \"dev_machine\".\"visibility\" = $20) \
             GROUP BY \"dev_machine\".\"id\" \
             ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC",
        );
        let expected = format!(
            "{}, \
             COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $1 \
             AND \"runner\".\"visibility\" = $2 AND \"runner\".\"workspace_id\" = $3)) AS \"runner_count\", \
             COUNT(DISTINCT \"runner\".\"id\") FILTER (WHERE (\"runner\".\"owner_id\" = $4 \
             AND \"runner\".\"visibility\" = $5 AND \"runner\".\"workspace_id\" = $6 \
             AND \"runner\".\"revoked_at\" IS NULL AND \"runner\".\"status\" IN ($7, $8))) AS \"online_runner_count\", \
             MAX(\"runner\".\"last_heartbeat_at\") FILTER (WHERE (\"runner\".\"owner_id\" = $9 \
             AND \"runner\".\"visibility\" = $10 AND \"runner\".\"workspace_id\" = $11)) AS \"last_heartbeat_at\", \
             {}",
            expected_list("dev_machine", MACHINE_COLS),
            control_online_fragment(12, 13),
        );
        assert_eq!(list, expected);
        // NULL last_seen_at sorts FIRST under bare DESC (rows.json
        // M3_ordering): the port is the bare ORDER BY, no NULLS clause.
        assert!(!sql.contains("NULLS"));
        // Nested probes carry no ORDER BY of their own.
        assert_eq!(sql.matches("ORDER BY").count(), 1);
        for fragment in [
            fixture_fragment("COUNT(DISTINCT"),
            fixture_fragment("FILTER ("),
            fixture_fragment("AS \"runner_count\""),
            fixture_fragment("AS \"online_runner_count\""),
            fixture_fragment("AS \"last_heartbeat_at\""),
            fixture_fragment("\"status\" IN ("),
            fixture_fragment("LEFT OUTER JOIN \"runner\""),
            fixture_fragment("GROUP BY"),
            fixture_fragment("\"dev_machine_id\" FROM \"runner\""),
            fixture_fragment("\"dev_machine_id\" IS NOT NULL"),
            fixture_fragment("\"visibility\" = "),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    // -- M4 --

    #[test]
    fn m4_probes_are_exists_with_cleared_ordering() {
        assert_eq!(
            machine_scope_runner_probe_sql(),
            "SELECT $1 AS \"a\" FROM \"runner\" \
             WHERE (\"runner\".\"dev_machine_id\" = $2 AND \"runner\".\"owner_id\" = $3 \
             AND \"runner\".\"visibility\" = $4 AND \"runner\".\"workspace_id\" = $5) \
             LIMIT 1",
        );
        assert_eq!(
            machine_scope_token_probe_sql(),
            "SELECT $1 AS \"a\" FROM \"machine_token\" \
             WHERE (\"machine_token\".\"dev_machine_id\" = $2 \
             AND \"machine_token\".\"user_id\" = $3 \
             AND \"machine_token\".\"workspace_id\" = $4) \
             LIMIT 1",
        );
        assert!(!machine_scope_runner_probe_sql().contains("ORDER BY"));
        assert!(!machine_scope_token_probe_sql().contains("ORDER BY"));
        for fragment in [
            fixture_fragment("FROM \"runner\""),
            fixture_fragment("\"workspace_id\" = "),
            fixture_fragment("\"owner_id\" = "),
        ] {
            assert_builder_contains(&machine_scope_runner_probe_sql(), &fragment);
        }
        for fragment in [
            fixture_fragment("FROM \"machine_token\""),
            fixture_fragment("\"user_id\" = "),
        ] {
            assert_builder_contains(&machine_scope_token_probe_sql(), &fragment);
        }
    }

    // -- M5 --

    #[test]
    fn m5_serialize_is_m3_annotations_over_pk_first() {
        let sql = machine_serialize_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"dev_machine\" LEFT OUTER JOIN \"runner\" \
             ON (\"dev_machine\".\"id\" = \"runner\".\"dev_machine_id\") \
             WHERE \"dev_machine\".\"id\" = $14 \
             GROUP BY \"dev_machine\".\"id\" \
             ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC LIMIT 1",
        );
        // Same annotations as M3, same $1-$13 numbering.
        let m3_list = assert_suffix(
            &machine_list_sql(),
            "FROM \"dev_machine\" LEFT OUTER JOIN \"runner\" \
             ON (\"dev_machine\".\"id\" = \"runner\".\"dev_machine_id\") \
             WHERE ((\"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"runner\" U0 \
             WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"owner_id\" = $14 \
             AND U0.\"visibility\" = $15 AND U0.\"workspace_id\" = $16)) \
             OR \"dev_machine\".\"id\" IN (SELECT DISTINCT U0.\"dev_machine_id\" FROM \"machine_token\" U0 \
             WHERE (U0.\"dev_machine_id\" IS NOT NULL AND U0.\"user_id\" = $17 \
             AND U0.\"workspace_id\" = $18))) \
             AND \"dev_machine\".\"owner_id\" = $19 AND \"dev_machine\".\"visibility\" = $20) \
             GROUP BY \"dev_machine\".\"id\" \
             ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC",
        );
        assert_eq!(list, m3_list);
        for fragment in [
            fixture_fragment("AS \"runner_count\""),
            fixture_fragment("AS \"control_online\""),
            fixture_fragment("LIMIT 1"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

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

    #[test]
    fn token_revoke_update_serves_revoke_and_rotate() {
        // runners.py:198 and :238 run the identical ORM call.
        assert_eq!(
            machine_tokens_revoke_sql(),
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 \
             WHERE (\"machine_token\".\"dev_machine_id\" = $2 \
             AND \"machine_token\".\"revoked_at\" IS NULL)",
        );
        for fragment in [
            fixture_fragment("UPDATE \"machine_token\" SET \"revoked_at\" = "),
            fixture_fragment("\"dev_machine_id\" = "),
            fixture_fragment("\"revoked_at\" IS NULL"),
        ] {
            assert_builder_contains(&machine_tokens_revoke_sql(), &fragment);
        }
    }

    #[test]
    fn m6_locked_runner_list_has_no_limit() {
        let sql = machine_runners_locked_list_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" \
             WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"revoked_at\" IS NULL) \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
             FOR UPDATE",
        );
        assert_eq!(list, expected_list("runner", RUNNER_COLS));
        assert!(!sql.contains("LIMIT"));
        for fragment in [
            fixture_fragment("FROM \"runner\" WHERE"),
            fixture_fragment("FOR UPDATE"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn m7_runner_ids_are_bare_ids_without_lock() {
        assert_eq!(
            machine_runner_ids_sql(),
            "SELECT \"runner\".\"id\" FROM \"runner\" \
             WHERE (\"runner\".\"dev_machine_id\" = $1 AND \"runner\".\"revoked_at\" IS NULL) \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC",
        );
        // No lock, no LIMIT, and no runner UPDATE anywhere in M7
        // (QUIRK-rotate-keeps-runners).
        assert!(!machine_runner_ids_sql().contains("FOR UPDATE"));
        assert!(!machine_runner_ids_sql().contains("LIMIT"));
        for fragment in [
            fixture_fragment("\"id\" FROM \"runner\""),
            fixture_fragment("\"revoked_at\" IS NULL"),
        ] {
            assert_builder_contains(&machine_runner_ids_sql(), &fragment);
        }
    }

    // -- R1 --

    fn r1_select_list() -> String {
        format!(
            "{}, {}, {}, {}",
            expected_list("runner", RUNNER_COLS),
            expected_list("dev_machine", MACHINE_COLS),
            expected_list("pod", POD_COLS),
            expected_list("projects", project_cols::COLUMNS),
        )
    }

    #[test]
    fn r1_base_select_joins_where_order() {
        let sql = runner_list_sql(false, false, false);
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" \
             LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
             INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
             WHERE (\"runner\".\"workspace_id\" = $1 AND \"runner\".\"owner_id\" = $2 \
             AND \"runner\".\"visibility\" = $3) \
             ORDER BY \"runner\".\"updated_at\" DESC",
        );
        assert_eq!(list, r1_select_list());
        // The pod join carries NO manager scope (select_related never
        // does — `"projects"."deleted_at"` in the select list is just a
        // column, not a guard).
        assert!(!sql.contains("deleted_at\" IS NULL"));
        for fragment in [
            fixture_fragment("FROM \"runner\""),
            fixture_fragment("INNER JOIN \"pod\""),
            fixture_fragment("INNER JOIN \"projects\""),
            fixture_fragment("LEFT OUTER JOIN \"dev_machine\""),
            fixture_fragment("\"workspace_id\" = "),
            fixture_fragment("\"owner_id\" = "),
            fixture_fragment("\"visibility\" = "),
            fixture_fragment("\"updated_at\" DESC"),
        ] {
            assert_builder_contains(&sql, &fragment);
        }
    }

    #[test]
    fn r1_filters_append_in_call_order_with_collapsing_binds() {
        // Pod only: $4.
        let pod = runner_list_sql(true, false, false);
        assert!(pod.contains(
            "AND \"runner\".\"visibility\" = $3 AND \"runner\".\"pod_id\" = $4) \
             ORDER BY \"runner\".\"updated_at\" DESC"
        ));
        // Bundled exclusion only: $4, as NOT (=).
        let excl = runner_list_sql(false, true, false);
        assert!(excl.contains(
            "AND \"runner\".\"visibility\" = $3 AND NOT (\"runner\".\"provisioning\" = $4)) \
             ORDER BY \"runner\".\"updated_at\" DESC"
        ));
        assert!(!excl.contains("<>"));
        // Project only: $4 on the JOINED pod table, joins reorder.
        let proj = runner_list_sql(false, false, true);
        assert!(proj.contains(
            "FROM \"runner\" INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
             LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
             WHERE (\"runner\".\"workspace_id\" = $1 AND \"runner\".\"owner_id\" = $2 \
             AND \"runner\".\"visibility\" = $3 AND \"pod\".\"project_id\" = $4) \
             ORDER BY \"runner\".\"updated_at\" DESC"
        ));
        // Select list never moves, whatever the joins do.
        assert_eq!(
            assert_suffix(&proj, &proj[proj.find("FROM ").unwrap()..]),
            r1_select_list()
        );
        // Full combo: $4/$5/$6 in source call order.
        let full = runner_list_sql(true, true, true);
        assert!(full.contains(
            "AND \"runner\".\"pod_id\" = $4 AND NOT (\"runner\".\"provisioning\" = $5) \
             AND \"pod\".\"project_id\" = $6) ORDER BY \"runner\".\"updated_at\" DESC"
        ));
        assert!(full.contains("INNER JOIN \"pod\" ON"));
        // No new JOIN for the project filter (reuses select_related).
        assert_eq!(full.matches("JOIN").count(), 3);
        for fragment in [
            fixture_fragment("\"pod_id\" = "),
            fixture_fragment("\"provisioning\""),
            fixture_fragment("\"project_id\" = "),
        ] {
            assert_builder_contains(&full, &fragment);
        }
    }

    #[test]
    fn r1_join_order_follows_filter_combo() {
        // QUIRK-join-order truth table (fresh compiles; the bundled
        // exclusion never reorders).
        let dev_first = "FROM \"runner\" LEFT OUTER JOIN \"dev_machine\" \
             ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
             INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\"";
        let pod_dev_proj = "FROM \"runner\" INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
             INNER JOIN \"projects\"";
        let pod_proj_dev =
            "FROM \"runner\" INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
             LEFT OUTER JOIN \"dev_machine\"";
        for excl in [false, true] {
            for (pod, proj, expected) in [
                (false, false, dev_first),
                (true, false, pod_dev_proj),
                (false, true, pod_proj_dev),
                (true, true, pod_proj_dev),
            ] {
                let sql = runner_list_sql(pod, excl, proj);
                assert!(sql.contains(expected), "{sql}");
                // Same three joins, only the order moves.
                assert_eq!(sql.matches("JOIN").count(), 3, "{sql}");
            }
        }
    }

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
    fn r2_detail_is_r1_shape_over_pk_first() {
        let sql = runner_detail_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"runner\" \
             LEFT OUTER JOIN \"dev_machine\" ON (\"runner\".\"dev_machine_id\" = \"dev_machine\".\"id\") \
             INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") \
             INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") \
             WHERE \"runner\".\"id\" = $1 \
             ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1",
        );
        assert_eq!(list, r1_select_list());
        for fragment in [fixture_fragment("\"id\" = $1"), fixture_fragment("LIMIT 1")] {
            assert_builder_contains(&sql, &fragment);
        }
    }

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
    fn r3_busy_guard_binds_runner_twice_then_statuses() {
        assert_eq!(
            runner_busy_guard_sql(),
            "SELECT $1 AS \"a\" FROM \"agent_run\" \
             WHERE ((\"agent_run\".\"runner_id\" = $2 OR \"agent_run\".\"pinned_runner_id\" = $3) \
             AND \"agent_run\".\"status\" IN ($4, $5, $6, $7, $8, $9, $10, $11)) \
             LIMIT 1",
        );
        assert!(!runner_busy_guard_sql().contains("ORDER BY"));
        for fragment in [
            fixture_fragment("FROM \"agent_run\""),
            fixture_fragment("\"runner_id\" = "),
            fixture_fragment("\"pinned_runner_id\" = "),
            fixture_fragment("\"status\" IN ("),
        ] {
            assert_builder_contains(&runner_busy_guard_sql(), &fragment);
        }
        // rows.json pins the guard matrix: pinned+queued blocks, same-pod
        // and terminal runs do not.
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert!(rows["R3_busy_guard"]["pinned_blocked"]
            .as_str()
            .unwrap()
            .contains("pinned_runner"));
        let not_blocked = rows["R3_busy_guard"]["not_blocked"].as_array().unwrap();
        assert!(not_blocked
            .iter()
            .any(|v| v.as_str().unwrap().contains("same-pod")));
    }

    #[test]
    fn real_move_is_pod_inequality() {
        let a = Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
        let b = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
        assert!(is_real_move(&a, &b));
        assert!(!is_real_move(&a, &a));
    }

    #[test]
    fn r3_patch_variants_emit_model_order_set() {
        assert_eq!(RUNNER_PATCH_UPDATABLE, &["name", "pod"]);
        assert_eq!(
            runner_patch_sql(true, true),
            "UPDATE \"runner\" SET \"pod_id\" = $1, \"name\" = $2, \"updated_at\" = $3 \
             WHERE \"runner\".\"id\" = $4",
        );
        assert_eq!(
            runner_patch_sql(true, false),
            "UPDATE \"runner\" SET \"pod_id\" = $1, \"updated_at\" = $2 \
             WHERE \"runner\".\"id\" = $3",
        );
        assert_eq!(
            runner_patch_sql(false, true),
            "UPDATE \"runner\" SET \"name\" = $1, \"updated_at\" = $2 \
             WHERE \"runner\".\"id\" = $3",
        );
        for sql in [
            runner_patch_sql(true, true),
            runner_patch_sql(true, false),
            runner_patch_sql(false, true),
        ] {
            for fragment in [
                fixture_fragment("UPDATE \"runner\" SET "),
                fixture_fragment("\"updated_at\" = "),
            ] {
                assert_builder_contains(&sql, &fragment);
            }
        }
        assert_builder_contains(
            &runner_patch_sql(true, false),
            &fixture_fragment("\"pod_id\" = "),
        );
        assert_builder_contains(
            &runner_patch_sql(false, true),
            &fixture_fragment("\"name\" = "),
        );
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

    #[test]
    fn scoped_machine_read_is_unlocked_first() {
        let sql = scoped_machine_read_sql();
        let list = assert_suffix(
            &sql,
            "FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 \
             ORDER BY \"dev_machine\".\"last_seen_at\" DESC, \"dev_machine\".\"created_at\" DESC \
             LIMIT 1",
        );
        assert_eq!(list, expected_list("dev_machine", MACHINE_COLS));
        // Text-identical to the M6 locked read minus the lock (the
        // fixture records this read as prose — "machine by PK (NO
        // lock)" — so the pin is structural, not a fixture needle).
        assert!(!sql.contains("FOR UPDATE"));
        assert_eq!(
            sql,
            machine_locked_read_sql().replace(" LIMIT 1 FOR UPDATE", " LIMIT 1")
        );
    }

    // -- rows.json shape --

    #[test]
    fn fixture_rows_shape_is_as_documented() {
        let rows: serde_json::Value = serde_json::from_str(FIXTURE_ROWS).unwrap();
        assert_eq!(rows["fixture_id"], "D13-F5");
        // M3_ordering: never-checked-in machines sort FIRST via the bare
        // DESC default — builders must not spell NULLS out.
        assert!(rows["M3_ordering"]
            .as_str()
            .unwrap()
            .contains("NULLS FIRST"));
        assert!(!machine_list_sql().contains("NULLS"));
        assert!(!machine_serialize_sql().contains("NULLS"));
        // The example row carries exactly the three annotation keys.
        for key in ["runner_count", "online_runner_count", "last_heartbeat_at"] {
            assert!(rows["M3_example_row"].get(key).is_some(), "{key}");
            assert!(
                machine_list_sql().contains(&format!("AS \"{key}\"")),
                "{key}"
            );
        }
        assert!(rows["M3_example_row"].get("control_online").is_some());
    }
}
