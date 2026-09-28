//! Space intake + asset read/write queries: intake issues and file assets.
//!
//! Port of `apps/api/pi_dash/space/views/intake.py:31-280`
//! (`IntakeIssuePublicViewSet`) and `apps/api/pi_dash/space/views/asset.py:26-226`
//! (`EntityAssetEndpoint`, `AssetRestoreEndpoint`, `EntityBulkAssetEndpoint`).
//! Fixture record `rust-api/fixtures/space/queries/intake.{sql,rows.json}` and
//! `queries/assets.{sql,rows.json}` (filed by PIDASHCONV-135; trace lines in
//! `rust-api/fixtures/space/TRACE.md`).
//!
//! Conventions (same as [`super::project_meta`], which owns the shared board
//! closures this module reuses by equality test):
//!
//! * Builders return the SQL text with Postgres `$N` placeholders in first-
//!   appearance order (Django renders `%s`; same binding order). Execution
//!   belongs to the handlers layer (PIDASHCONV-177/178), which binds the
//!   documented `$N` params in order and maps row counts onto the `get()`
//!   contract (`0 -> DoesNotExist`, `>1 -> MultipleObjectsReturned`).
//! * `.get()` single-row reads omit the `ORDER BY ... LIMIT 21` Django's
//!   `get()` adds: the scoped lookups below are unique by construction, so
//!   ordering/limit cannot change the row (or the 0/1 outcome); the caller
//!   still treats `>1` rows as `MultipleObjectsReturned`.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings; rows keep them as `String`. UUID and FK keys render as strings.
//! * Error bodies and status codes belong to the guards layer, not here; the
//!   Python line for each is cited so handlers wire the same mapping.
//! * S3 presigned URLs (`asset.py:62-66,122-124`) and `issue_activity` /
//!   `get_asset_object_metadata` task enqueues are handler/task-layer work;
//!   only their DB-touching halves are pinned here.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * QUIRK-unscoped-board (`asset.py:36,70,137`): get/post/patch look the
//!   board up by anchor with NO `entity_name="project"` scoping — a board of
//!   any entity type satisfies it. Delete/restore/bulk (`:158,:177,:195`)
//!   DO scope on `entity_name="project"`. Both shapes are kept.
//! * QUIRK-dead-get_queryset (`intake.py:37-54`): the mixin queryset looks the
//!   board up by `slug`/`project_id` kwargs the routes never supply
//!   (`urls/intake.py:15-29` give `anchor`/`intake_id`/`pk` only), so both
//!   params are `NULL` and the lookup always misses. `list()` bypasses
//!   `get_queryset`, so the dead path never serves traffic; it is ported as
//!   [`intake_dead_board_get_sql`] for the record.
//! * QUIRK-priority-default (`intake.py:119-126` vs `:149`): creation
//!   validates `priority` against `[low,medium,high,urgent,none]` defaulting
//!   to `"none"`, but the INSERT falls back to `"low"`. Both literals kept.
//! * QUIRK-no-workspace-insert (`intake.py:145-152,165-170`): the Issue and
//!   IntakeIssue creates pass no `workspace_id`; `ProjectBaseModel.save()`
//!   (`db/models/project.py:302-311`) backfills it from the project row at
//!   write time. The builders below emit the explicit kwarg set; the
//!   backfill read-then-write is handler work.
//! * QUIRK-triage-resequence (`db/models/state.py:132-139`): `State.save()`
//!   overwrites `sequence` with `max(project sequences)+15000` whenever the
//!   project already has states, so the auto-created Triage keeps `65000`
//!   only on an empty project. `slug` is likewise filled (`"triage"`).
//!   [`triage_insert_sql`] emits the explicit create kwargs; the reseed is
//!   handler work.
//! * QUIRK-unconditional-comment-id (`asset.py:110-119`): post stuffs
//!   `entity_identifier` into `comment_id` for EVERY entity type, even
//!   non-comment ones. Kept.
//! * QUIRK-bulk-silent-noop (`asset.py:223-226`): bulk reassigns `comment_id`
//!   only when the first asset's type is `COMMENT_DESCRIPTION`; every other
//!   type updates nothing yet still returns 204. Kept.
//! * QUIRK-destroy-keeps-issue (`intake.py:258-280`): destroy deletes the
//!   INTAKE row only; the Issue survives. Kept.
//! * QUIRK-intake-id-from-url (`intake.py:165-170`): the IntakeIssue row takes
//!   `intake_id` from the URL kwarg, NOT from `board.intake`. Kept.
//! * QUIRK-pop-mutates (`intake.py:197`): partial_update `pop("issue")`s from
//!   `request.data` (a missing `issue` key raises `KeyError`, no `.get`
//!   default). Handler work; the 3-key subset is [`PARTIAL_UPDATE_KEYS`].
//! * QUIRK-pod-backfill (`db/models/issue.py:267-285`): `Issue.save()` fills
//!   `assigned_pod` from the project's default pod on creation. Handler work.

use serde::{Deserialize, Serialize};

use super::project_meta::BOARD_COLUMNS;

// ---------------------------------------------------------------------------
// Board closures (shared with project_meta; pinned by equality)
// ---------------------------------------------------------------------------

/// Intake board get: `DeployBoard.objects.get(anchor=anchor,
/// entity_name="project")` (`views/intake.py:57`; same call at `:108,:176,`
/// `:237,:259`). Text-identical to the settings get; `$1` = anchor.
pub fn intake_board_get_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1 AND \"deploy_boards\".\"entity_name\" = 'project')",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Asset board-first, unscoped: `DeployBoard.objects.filter(anchor=anchor)`
/// `.first()` (`views/asset.py:36,70,137`). `$1` = anchor.
pub fn asset_board_first_unscoped_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1) ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Asset board-first, project-scoped: `.filter(anchor=anchor,`
/// `entity_name="project").first()` (`views/asset.py:158,177,195`).
/// `$1` = anchor.
pub fn asset_board_first_scoped_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1 AND \"deploy_boards\".\"entity_name\" = 'project') ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// ---------------------------------------------------------------------------
// Intake list (N2) + dead get_queryset (N1)
// ---------------------------------------------------------------------------

/// N1 dead path: `DeployBoard.objects.get(workspace__slug=None,`
/// `project_id=None)` (`views/intake.py:38-41`). The routes never supply
/// `slug`/`project_id`, so Django renders both lookups as `IS NULL` and the
/// row count always maps to `DoesNotExist`. No params.
pub fn intake_dead_board_get_sql() -> String {
    "SELECT \"deploy_boards\".* FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" IS NULL AND \"deploy_boards\".\"project_id\" IS NULL)".to_string()
}

/// N2 bridge annotation: `.annotate(bridge_id=F("issue_intake__id"))`
/// (`views/intake.py:72`).
pub fn intake_list_bridge_select_sql() -> String {
    "\"issue_intake\".\"id\" AS \"bridge_id\"".to_string()
}

/// N2 ordering: `.order_by("issue_intake__snoozed_till",`
/// `"issue_intake__status")` (`views/intake.py:75`).
pub fn intake_list_order_sql() -> String {
    "\"issue_intake\".\"snoozed_till\" ASC, \"issue_intake\".\"status\" ASC".to_string()
}

/// N2 WHERE core: `Issue.objects.filter(issue_intake__intake_id=intake_id,`
/// `workspace_id=..., project_id=...)` (`views/intake.py:66-70`). `$1` =
/// intake id, `$2` = workspace id, `$3` = project id. Bare conjuncts (no
/// outer parens) so the text matches the fixture's WHERE block verbatim; the
/// dynamic `issue_filters(query_params, "GET")` conjunct (`:64,:71`) is
/// caller supplied (see [`intake_list_sql`]).
pub fn intake_list_where_sql() -> String {
    "\"issues\".\"deleted_at\" IS NULL AND \"issue_intake\".\"intake_id\" = $1 AND \"issues\".\"workspace_id\" = $2 AND \"issues\".\"project_id\" = $3".to_string()
}

/// N2 sub-issue count: `Issue.issue_objects.filter(parent=OuterRef("id"))`
/// counted (`views/intake.py:77-81`). The `issue_objects` scope
/// (`db/models/issue.py:95-104`; single-table conjuncts in
/// `db::space::columns::issue_objects_scope`, joins owned here) travels into
/// the subquery.
fn sub_issues_count_sql() -> String {
    "(SELECT COUNT(U0.\"id\") FROM \"issues\" U0 INNER JOIN \"states\" U1 ON (U0.\"state_id\" = U1.\"id\") INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") WHERE (U0.\"deleted_at\" IS NULL AND U0.\"archived_at\" IS NULL AND U0.\"is_draft\" = false AND U1.\"group\" != 'triage' AND U2.\"archived_at\" IS NULL AND U0.\"parent_id\" = (\"issues\".\"id\"))) AS \"sub_issues_count\"".to_string()
}

/// N2 link count: `IssueLink.objects.filter(issue=OuterRef("id"))` counted
/// (`views/intake.py:82-87`; soft-delete scope via `ProjectBaseModel`).
fn link_count_sql() -> String {
    "(SELECT COUNT(U0.\"id\") FROM \"issue_links\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\"))) AS \"link_count\"".to_string()
}

/// N2 attachment count: `FileAsset.objects.filter(issue_id=OuterRef("id"),`
/// `entity_type=ISSUE_ATTACHMENT)` counted (`views/intake.py:88-96`).
fn attachment_count_sql() -> String {
    "(SELECT COUNT(U0.\"id\") FROM \"file_assets\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\") AND U0.\"entity_type\" = 'ISSUE_ATTACHMENT')) AS \"attachment_count\"".to_string()
}

/// N2 list statement (`views/intake.py:65-105`): base filter + bridge id +
/// three count annotations, `select_related`/`prefetch_related` produce no
/// extra top-level SQL, no pagination. `extra_predicate` is the rendered
/// `issue_filters(query_params, "GET")` conjunct (`:64,:71`), appended as
/// `AND (...)`; `None` renders the unfiltered shape.
pub fn intake_list_sql(extra_predicate: Option<&str>) -> String {
    let where_clause = match extra_predicate {
        Some(predicate) => format!("({} AND ({}))", intake_list_where_sql(), predicate),
        None => format!("({})", intake_list_where_sql()),
    };
    format!(
        "SELECT \"issues\".*, {}, {}, {}, {} FROM \"issues\" INNER JOIN \"intake_issues\" \"issue_intake\" ON (\"issues\".\"id\" = \"issue_intake\".\"issue_id\") WHERE {} ORDER BY {}",
        intake_list_bridge_select_sql(),
        sub_issues_count_sql(),
        link_count_sql(),
        attachment_count_sql(),
        where_clause,
        intake_list_order_sql()
    )
}

/// N2 intake prefetch: `Prefetch("issue_intake", IntakeIssue.objects.only(`
/// `"status", "duplicate_to", "snoozed_till", "source"))`
/// (`views/intake.py:97-102`). `.only()` always keeps the PK plus the join
/// key; every other column is deferred. `$1` is the caller's first `$N`
/// placeholder: the caller expands the `IN` list to `$1..$N`.
pub fn intake_issue_prefetch_sql() -> String {
    "SELECT \"intake_issues\".\"id\", \"intake_issues\".\"status\", \"intake_issues\".\"duplicate_to_id\", \"intake_issues\".\"snoozed_till\", \"intake_issues\".\"source\", \"intake_issues\".\"issue_id\" FROM \"intake_issues\" WHERE (\"intake_issues\".\"deleted_at\" IS NULL AND \"intake_issues\".\"issue_id\" IN ($1))".to_string()
}

// ---------------------------------------------------------------------------
// Intake create (N3)
// ---------------------------------------------------------------------------

/// N3 triage lookup: `State.triage_objects.filter(project_id=...,`
/// `workspace_id=...).first()` (`views/intake.py:129-131`; triage scope
/// `db/models/state.py:86-90`). `State.Meta.ordering = ("sequence",)`
/// (`:129`) feeds `.first()`. `$1` = project id, `$2` = workspace id.
pub fn triage_lookup_sql() -> String {
    "SELECT \"states\".* FROM \"states\" WHERE (\"states\".\"deleted_at\" IS NULL AND \"states\".\"group\" = 'triage' AND \"states\".\"project_id\" = $1 AND \"states\".\"workspace_id\" = $2) ORDER BY \"states\".\"sequence\" ASC LIMIT 1".to_string()
}

/// N3 triage auto-create exact defaults (`views/intake.py:134-142`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriageDefaults {
    pub name: String,
    pub group: String,
    pub color: String,
    pub sequence: f64,
    pub default: bool,
}

/// The literal defaults from `intake.py:134-142`: `name="Triage"`,
/// `group=StateGroup.TRIAGE`, `color="#4E5355"`, `sequence=65000`,
/// `default=False` (project/workspace come from the board row).
pub fn triage_defaults() -> TriageDefaults {
    TriageDefaults {
        name: "Triage".to_string(),
        group: "triage".to_string(),
        color: "#4E5355".to_string(),
        sequence: 65000.0,
        default: false,
    }
}

/// N3 triage INSERT column set (`views/intake.py:134-142`). `$1` = id, `$2`
/// = created_at, `$3` = updated_at, `$4` = project id, `$5` = workspace id;
/// name/group/color/sequence/default are the [`triage_defaults`] literals.
pub fn triage_insert_sql() -> String {
    "INSERT INTO \"states\" (\"id\", \"created_at\", \"updated_at\", \"name\", \"group\", \"project_id\", \"workspace_id\", \"color\", \"sequence\", \"default\") VALUES ($1, $2, $3, 'Triage', 'triage', $4, $5, '#4E5355', 65000, false)".to_string()
}

/// N3 Issue INSERT (`views/intake.py:145-152`). `$1` = id, `$2` =
/// created_at, `$3` = updated_at, `$4` = name, `$5` = description_json, `$6`
/// = description_html, `$7` = priority, `$8` = project id, `$9` = triage
/// state id. Python-side fallbacks (`:146-149`):
/// description_json `{}` / description_html `"<p></p>"` / priority `"low"`
/// (NOT the validated `"none"` — QUIRK-priority-default); a present-but-null
/// key passes `None` through. No `workspace_id` kwarg
/// (QUIRK-no-workspace-insert).
pub fn issue_insert_sql() -> String {
    "INSERT INTO \"issues\" (\"id\", \"created_at\", \"updated_at\", \"name\", \"description_json\", \"description_html\", \"priority\", \"project_id\", \"state_id\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)".to_string()
}

/// N3 IntakeIssue INSERT (`views/intake.py:165-170`). `$1` = id, `$2` =
/// created_at, `$3` = updated_at, `$4` = intake id (**URL kwarg**,
/// QUIRK-intake-id-from-url), `$5` = project id, `$6` = issue id;
/// `source=IN_APP` literal, `status` model default `-2` (pending). No
/// `workspace_id` kwarg (QUIRK-no-workspace-insert).
pub fn intake_issue_insert_sql() -> String {
    "INSERT INTO \"intake_issues\" (\"id\", \"created_at\", \"updated_at\", \"intake_id\", \"project_id\", \"issue_id\", \"source\", \"status\") VALUES ($1, $2, $3, $4, $5, $6, 'IN_APP', -2)".to_string()
}

// ---------------------------------------------------------------------------
// Intake retrieve / partial_update / destroy (N4-N6)
// ---------------------------------------------------------------------------

/// N4/N6 intake-issue get: `IntakeIssue.objects.get(pk=pk,`
/// `workspace_id=..., project_id=..., intake_id=intake_id)`
/// (`views/intake.py:183-188,244-249,266-271`). `$1` = pk, `$2` = workspace
/// id, `$3` = project id, `$4` = intake id.
pub fn intake_issue_scoped_get_sql() -> String {
    "SELECT \"intake_issues\".* FROM \"intake_issues\" WHERE (\"intake_issues\".\"deleted_at\" IS NULL AND \"intake_issues\".\"id\" = $1 AND \"intake_issues\".\"workspace_id\" = $2 AND \"intake_issues\".\"project_id\" = $3 AND \"intake_issues\".\"intake_id\" = $4)".to_string()
}

/// N4/N5 issue get: `Issue.objects.get(pk=intake_issue.issue_id,`
/// `workspace_id=..., project_id=...)` (`views/intake.py:199-203,250-254`).
/// `$1` = issue id, `$2` = workspace id, `$3` = project id.
pub fn issue_scoped_get_sql() -> String {
    "SELECT \"issues\".* FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1 AND \"issues\".\"workspace_id\" = $2 AND \"issues\".\"project_id\" = $3)".to_string()
}

/// N4 update subset: only these three keys survive into the serializer
/// (`views/intake.py:205-209`), each falling back to the stored issue value.
pub const PARTIAL_UPDATE_KEYS: &[&str] = &["name", "description_html", "description_json"];

/// N6 destroy: `intake_issue.delete()` (`views/intake.py:279`) — deletes the
/// intake row only (QUIRK-destroy-keeps-issue). `$1` = intake-issue id.
pub fn intake_issue_delete_sql() -> String {
    "DELETE FROM \"intake_issues\" WHERE \"intake_issues\".\"id\" = $1".to_string()
}

// ---------------------------------------------------------------------------
// Assets (A1-A6)
// ---------------------------------------------------------------------------

/// Allowed entity types for the A1 get (`views/asset.py:48-51`).
pub const ASSET_GET_ENTITY_TYPES: &[&str] = &["ISSUE_DESCRIPTION", "COMMENT_DESCRIPTION"];

/// A1 get WHERE core (`views/asset.py:45-52`). `$1` = workspace id, `$2` =
/// asset id. Bare predicate so the text matches the fixture's WHERE block.
pub fn asset_get_where_sql() -> String {
    "\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"workspace_id\" = $1 AND \"file_assets\".\"id\" = $2 AND \"file_assets\".\"entity_type\" IN ('ISSUE_DESCRIPTION', 'COMMENT_DESCRIPTION')".to_string()
}

/// A1 get: `FileAsset.objects.get(workspace_id=..., pk=pk,`
/// `entity_type__in=[ISSUE_DESCRIPTION, COMMENT_DESCRIPTION])`
/// (`views/asset.py:45-52`). The `is_uploaded` 404 (`:55-59`) and the 302
/// presigned redirect (`:62-66`) are handler work.
pub fn asset_get_sql() -> String {
    format!(
        "SELECT \"file_assets\".* FROM \"file_assets\" WHERE ({})",
        asset_get_where_sql()
    )
}

/// A2 post INSERT (`views/asset.py:110-119`). `$1` = id, `$2` = created_at,
/// `$3` = updated_at, `$4` = attributes JSON (`{name, type, size}`), `$5` =
/// asset key (`{workspace_id}/{uuid4hex}-{name}`, `:107`), `$6` = size, `$7`
/// = workspace id, `$8` = creator id, `$9` = entity_type, `$10` = project id,
/// `$11` = entity_identifier stored into **`comment_id` unconditionally**
/// (QUIRK-unconditional-comment-id); `is_uploaded` starts false. `size`
/// falls back to `settings.FILE_SIZE_LIMIT` with an unguarded `int()` and no
/// max check (`:78`); `type` defaults to `"image/jpeg"` (`:77`).
pub fn asset_insert_sql() -> String {
    "INSERT INTO \"file_assets\" (\"id\", \"created_at\", \"updated_at\", \"attributes\", \"asset\", \"size\", \"workspace_id\", \"created_by_id\", \"entity_type\", \"project_id\", \"comment_id\", \"is_uploaded\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, false)".to_string()
}

/// A3 patch get: `FileAsset.objects.get(id=pk, workspace=...)`
/// (`views/asset.py:143`) — patch passes NO project scoping. `$1` = asset
/// id, `$2` = workspace id.
pub fn asset_patch_get_sql() -> String {
    "SELECT \"file_assets\".* FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 AND \"file_assets\".\"workspace_id\" = $2)".to_string()
}

/// A4 delete get: `FileAsset.objects.get(id=pk, workspace=...,
/// project_id=...)` (`views/asset.py:163`). `$1` = asset id, `$2` =
/// workspace id, `$3` = project id.
pub fn asset_scoped_get_sql() -> String {
    "SELECT \"file_assets\".* FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1 AND \"file_assets\".\"workspace_id\" = $2 AND \"file_assets\".\"project_id\" = $3)".to_string()
}

/// A3 patch write: `is_uploaded=True` (`:145`), `attributes` replaced
/// (`:151`), `save(update_fields=["attributes", "is_uploaded"])` (`:153`).
/// `$1` = asset id, `$2` = attributes JSON. The `storage_metadata` metadata
/// task (`:147-148`) is task-layer work.
pub fn asset_patch_sql() -> String {
    "UPDATE \"file_assets\" SET \"attributes\" = $2, \"is_uploaded\" = true WHERE \"file_assets\".\"id\" = $1".to_string()
}

/// A3 patch DB effect (fixture `assets.rows.json` → `A3_patch_effect`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetPatchEffect {
    pub attributes: serde_json::Value,
    pub is_uploaded: bool,
}

/// A4 soft-delete: `is_deleted=True, deleted_at=now`
/// (`views/asset.py:165-168`). `$1` = asset id, `$2` = now.
pub fn asset_soft_delete_sql() -> String {
    "UPDATE \"file_assets\" SET \"is_deleted\" = true, \"deleted_at\" = $2 WHERE \"file_assets\".\"id\" = $1".to_string()
}

/// A5 restore read: `FileAsset.all_objects.get(id=pk, workspace=...)`
/// (`views/asset.py:183`) — the unscoped manager sees soft-deleted rows, so
/// there is deliberately NO `deleted_at IS NULL` conjunct. `$1` = asset id,
/// `$2` = workspace id.
pub fn asset_restore_get_sql() -> String {
    "SELECT \"file_assets\".* FROM \"file_assets\" WHERE (\"file_assets\".\"id\" = $1 AND \"file_assets\".\"workspace_id\" = $2)".to_string()
}

/// A5 restore write: `is_deleted=False, deleted_at=None`
/// (`views/asset.py:184-186`). `$1` = asset id.
pub fn asset_restore_sql() -> String {
    "UPDATE \"file_assets\" SET \"is_deleted\" = false, \"deleted_at\" = NULL WHERE \"file_assets\".\"id\" = $1".to_string()
}

/// A6 bulk filter: `FileAsset.objects.filter(id__in=asset_ids,`
/// `workspace=..., project_id=...)` (`views/asset.py:207-211`). `count` ids
/// take `$1..$N`; `$(N+1)` = workspace id, `$(N+2)` = project id.
pub fn bulk_filter_sql(count: usize) -> String {
    let ids = (1..=count)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT \"file_assets\".* FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" IN ({ids}) AND \"file_assets\".\"workspace_id\" = ${} AND \"file_assets\".\"project_id\" = ${})",
        count + 1,
        count + 2
    )
}

/// A6 bulk write: `assets.update(comment_id=entity_id)`
/// (`views/asset.py:225`) — executes ONLY when the first asset's
/// `entity_type == COMMENT_DESCRIPTION` (QUIRK-bulk-silent-noop, handler
/// gate). Placeholders follow [`bulk_filter_sql`], plus `$(N+3)` = entity id.
pub fn bulk_reassign_sql(count: usize) -> String {
    let ids = (1..=count)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"file_assets\" SET \"comment_id\" = ${} WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" IN ({ids}) AND \"file_assets\".\"workspace_id\" = ${} AND \"file_assets\".\"project_id\" = ${})",
        count + 3,
        count + 1,
        count + 2
    )
}

#[cfg(test)]
mod tests {
    use super::super::project_meta::{board_by_anchor_first_sql, settings_get_sql};
    use super::*;

    const FIXTURE_INTAKE_SQL: &str =
        include_str!("../../../../../fixtures/space/queries/intake.sql");
    const FIXTURE_INTAKE_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/intake.rows.json");
    const FIXTURE_ASSETS_SQL: &str =
        include_str!("../../../../../fixtures/space/queries/assets.sql");
    const FIXTURE_ASSETS_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/assets.rows.json");

    /// Collapse every whitespace run to one space (Django's compiler wraps
    /// lines; the text is what matters).
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Render a builder's `$N` placeholders Django-style (`%(name)s`, as the
    /// fixture records them) for a containment check against the fixture.
    fn with_named_params(sql: &str, names: &[&str]) -> String {
        let mut out = sql.to_string();
        for (i, name) in names.iter().enumerate().rev() {
            out = out.replace(&format!("${}", i + 1), &format!("%({name})s"));
        }
        out
    }

    /// The builder's Django-shaped fragment must appear verbatim in the
    /// fixture (whitespace-insensitive). Use when the fixture records the
    /// full statement shape.
    fn assert_in_fixture(haystack: &str, sql: &str, params: &[&str]) {
        let hay = squashed(haystack);
        let needle = squashed(&with_named_params(sql, params));
        assert!(
            hay.contains(&needle),
            "builder SQL not found in fixture:\n{needle}"
        );
    }

    /// The fixture's concrete fragment (Django `%()s` style) must appear
    /// verbatim in the builder's output. Use when the fixture abbreviates
    /// the statement (bare column names, `...` ellipses) while the builder
    /// emits fully-qualified executable SQL.
    fn assert_builder_contains(builder_sql: &str, params: &[&str], fragment: &str) {
        let hay = squashed(&with_named_params(builder_sql, params));
        let needle = squashed(fragment);
        assert!(
            hay.contains(&needle),
            "fixture fragment not found in builder SQL:\n{needle}\n----\n{hay}"
        );
    }

    #[test]
    fn intake_board_get_matches_settings_get_shape() {
        // Same ORM call (`DeployBoard.objects.get(anchor, entity_name)`);
        // the intake fixture records it as prose (`:57`), so equality with
        // the project_meta builder (fixture-pinned there) is the contract.
        assert_eq!(intake_board_get_sql(), settings_get_sql());
    }

    #[test]
    fn asset_board_closures_match_shared_shapes() {
        assert_eq!(
            asset_board_first_unscoped_sql(),
            board_by_anchor_first_sql()
        );
        let scoped = asset_board_first_scoped_sql();
        assert!(scoped.contains("\"entity_name\" = 'project'"));
        assert!(scoped.ends_with("ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1"));
    }

    #[test]
    fn dead_get_queryset_board_is_always_null() {
        let sql = intake_dead_board_get_sql();
        assert!(sql.contains("\"workspaces\".\"slug\" IS NULL"));
        assert!(sql.contains("\"deploy_boards\".\"project_id\" IS NULL"));
        // No params: the routes never supply slug/project_id.
        assert!(!sql.contains('$'));
    }

    #[test]
    fn intake_list_fragments_match_fixture_n2() {
        assert_in_fixture(FIXTURE_INTAKE_SQL, &intake_list_bridge_select_sql(), &[]);
        assert_in_fixture(
            FIXTURE_INTAKE_SQL,
            &intake_list_where_sql(),
            &["intake_id", "workspace_id", "project_id"],
        );
        assert_in_fixture(FIXTURE_INTAKE_SQL, &intake_list_order_sql(), &[]);
        // The full statement composes the pinned fragments.
        let full = intake_list_sql(None);
        for fragment in [
            intake_list_bridge_select_sql(),
            intake_list_where_sql(),
            intake_list_order_sql(),
            "AS \"sub_issues_count\"".to_string(),
            "AS \"link_count\"".to_string(),
            "AS \"attachment_count\"".to_string(),
            "INNER JOIN \"intake_issues\" \"issue_intake\" ON (\"issues\".\"id\" = \"issue_intake\".\"issue_id\")".to_string(),
        ] {
            assert!(full.contains(&fragment), "{fragment}");
        }
    }

    #[test]
    fn intake_list_extra_predicate_appends_as_and() {
        let full = intake_list_sql(Some("\"issues\".\"priority\" = $4"));
        assert!(
            full.contains("AND \"issues\".\"project_id\" = $3 AND (\"issues\".\"priority\" = $4)")
        );
    }

    #[test]
    fn intake_list_row_prefetch_only_contract() {
        // The N2 fixture row proves the `.only(status, duplicate_to,
        // snoozed_till, source)` prefetch: the nested intake object carries
        // exactly the PK plus those four columns.
        let rows: serde_json::Value =
            serde_json::from_str(FIXTURE_INTAKE_ROWS).expect("rows fixture parses");
        let row = &rows["rows"]["N2_list_row"];
        assert!(row.get("bridge_id").is_some());
        let intake = &row["issue_intake"][0];
        let mut keys: Vec<&str> = intake
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["duplicate_to", "id", "snoozed_till", "source", "status"]
        );
        // Prefetch builder selects exactly that column set (+ join key).
        let prefetch = intake_issue_prefetch_sql();
        for col in [
            "\"status\"",
            "\"duplicate_to_id\"",
            "\"snoozed_till\"",
            "\"source\"",
            "\"issue_id\"",
        ] {
            assert!(prefetch.contains(col), "{col}");
        }
    }

    #[test]
    fn triage_lookup_and_defaults_match_fixture_n3() {
        let sql = triage_lookup_sql();
        assert!(sql.contains("\"group\" = 'triage'"));
        assert!(sql.ends_with("ORDER BY \"states\".\"sequence\" ASC LIMIT 1"));
        let defaults = triage_defaults();
        assert_eq!(defaults.name, "Triage");
        assert_eq!(defaults.group, "triage");
        assert_eq!(defaults.color, "#4E5355");
        assert_eq!(defaults.sequence, 65000.0);
        assert!(!defaults.default);
        let insert = triage_insert_sql();
        assert!(insert.contains("'Triage', 'triage'"));
        assert!(insert.contains("'#4E5355', 65000, false"));
    }

    #[test]
    fn create_inserts_match_fixture_n3() {
        // Fixture N3 abbreviates both INSERTs with `...` ellipses, so pin
        // the explicit column sets and the documented literals instead.
        let issue = issue_insert_sql();
        for col in [
            "\"name\"",
            "\"description_json\"",
            "\"description_html\"",
            "\"priority\"",
            "\"project_id\"",
            "\"state_id\"",
        ] {
            assert!(issue.contains(col), "{col}");
        }
        // No workspace_id kwarg on either write.
        for sql in [issue_insert_sql(), intake_issue_insert_sql()] {
            assert!(!sql.contains("workspace_id"), "{sql}");
        }
        // Intake row takes the URL intake id with IN_APP source + pending.
        let intake = intake_issue_insert_sql();
        assert!(intake.contains("\"intake_id\", \"project_id\", \"issue_id\""));
        assert!(intake.contains("'IN_APP', -2"));
    }

    #[test]
    fn scoped_lookups_match_fixture_n4_n6() {
        // Fixture N4/N6 records the lookups as prose; pin table + conjunct
        // order + placeholder order, which is the SQL contract.
        let scoped = intake_issue_scoped_get_sql();
        assert!(scoped.starts_with("SELECT \"intake_issues\".* FROM \"intake_issues\" WHERE ("));
        // Conjuncts in call order with $1..$4 in sequence.
        let mut cursor = 0;
        for fragment in [
            "\"id\" = $1",
            "\"workspace_id\" = $2",
            "\"project_id\" = $3",
            "\"intake_id\" = $4",
        ] {
            let pos = scoped[cursor..].find(fragment).unwrap_or(usize::MAX);
            assert!(pos != usize::MAX, "{fragment}");
            cursor += pos + fragment.len();
        }
        let issue = issue_scoped_get_sql();
        assert!(issue.contains(
            "\"id\" = $1 AND \"issues\".\"workspace_id\" = $2 AND \"issues\".\"project_id\" = $3"
        ));
        assert_eq!(
            PARTIAL_UPDATE_KEYS,
            &["name", "description_html", "description_json"]
        );
        // Destroy removes the intake row only.
        assert_eq!(
            intake_issue_delete_sql(),
            "DELETE FROM \"intake_issues\" WHERE \"intake_issues\".\"id\" = $1"
        );
    }

    #[test]
    fn asset_get_matches_fixture_a1() {
        assert_in_fixture(
            FIXTURE_ASSETS_SQL,
            &asset_get_where_sql(),
            &["workspace_id", "pk"],
        );
        for entity in ASSET_GET_ENTITY_TYPES {
            assert!(asset_get_sql().contains(entity), "{entity}");
        }
    }

    #[test]
    fn asset_insert_matches_fixture_a2() {
        let sql = asset_insert_sql();
        // comment_id is bound unconditionally, whatever the entity type.
        assert!(sql.contains("\"comment_id\", \"is_uploaded\""));
        assert!(sql.ends_with(", $10, $11, false)"));
        assert_builder_contains(
            &sql,
            &[
                "id",
                "created_at",
                "updated_at",
                "attributes_json",
                "asset_key",
                "size",
                "workspace_id",
                "created_by",
                "entity_type",
                "project_id",
                "entity_identifier",
            ],
            "%(entity_type)s, %(project_id)s, %(entity_identifier)s, false",
        );
    }

    #[test]
    fn asset_patch_effect_byte_matches_fixture_a3() {
        let effect = AssetPatchEffect {
            attributes: serde_json::json!({"name": "shot.png", "type": "image/png", "size": 12345}),
            is_uploaded: true,
        };
        let rows: serde_json::Value =
            serde_json::from_str(FIXTURE_ASSETS_ROWS).expect("rows fixture parses");
        assert_eq!(
            serde_json::to_value(&effect).unwrap(),
            rows["rows"]["A3_patch_effect"]
        );
        let sql = asset_patch_sql();
        assert!(sql.contains("SET \"attributes\" = $2, \"is_uploaded\" = true"));
        assert!(sql.ends_with("WHERE \"file_assets\".\"id\" = $1"));
    }

    #[test]
    fn asset_patch_get_has_no_project_scoping() {
        // Fixture A3 records the patch lookup as `get(id=pk, workspace)`:
        // unlike delete (:163), patch passes no project scoping, so the
        // builder must not emit a project conjunct.
        let patch = asset_patch_get_sql();
        assert!(patch.contains("\"file_assets\".\"deleted_at\" IS NULL"));
        assert!(patch.contains("\"file_assets\".\"id\" = $1"));
        assert!(patch.contains("\"file_assets\".\"workspace_id\" = $2"));
        assert!(!patch.contains("project_id"), "{patch}");
        // Delete keeps the project conjunct.
        assert!(asset_scoped_get_sql().contains("\"file_assets\".\"project_id\" = $3"));
    }

    #[test]
    fn asset_soft_delete_and_restore_match_fixture_a4_a5() {
        // Fixture A4/A5 uses bare `"id"` in WHERE; the builders emit the
        // qualified form Django renders — pin the SET fragments verbatim.
        assert_builder_contains(
            &asset_soft_delete_sql(),
            &["pk", "now"],
            "SET \"is_deleted\" = true, \"deleted_at\" = %(now)s",
        );
        assert_builder_contains(&asset_restore_sql(), &["pk"], "SET \"is_deleted\" = false");
        assert!(asset_restore_sql()
            .ends_with("\"deleted_at\" = NULL WHERE \"file_assets\".\"id\" = $1"));
        // Restore reads through all_objects: no soft-delete scope.
        let get = asset_restore_get_sql();
        assert!(!get.contains("deleted_at"), "{get}");
        assert!(asset_scoped_get_sql().contains("\"deleted_at\" IS NULL"));
    }

    #[test]
    fn bulk_filter_and_reassign_match_fixture_a6() {
        let filter = bulk_filter_sql(2);
        assert!(filter.contains("\"id\" IN ($1, $2)"));
        assert!(filter.ends_with(
            "\"file_assets\".\"workspace_id\" = $3 AND \"file_assets\".\"project_id\" = $4)"
        ));
        // Same scoping survives the Django-style rendering against the A6
        // fixture fragments (bare names are substrings of qualified ones).
        for fragment in [
            "\"workspace_id\" = %(workspace_id)s",
            "\"project_id\" = %(project_id)s",
        ] {
            assert_builder_contains(&filter, &["a", "b", "workspace_id", "project_id"], fragment);
        }
    }

    #[test]
    fn bulk_reassign_targets_comment_id_only() {
        let update = bulk_reassign_sql(2);
        assert!(update.starts_with("UPDATE \"file_assets\" SET \"comment_id\" = $5 WHERE"));
        assert!(update.contains("\"id\" IN ($1, $2)"));
    }
}
