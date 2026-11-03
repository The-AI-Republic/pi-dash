//! `?expand=` nested expansions for the four app:issues reads
//! (PIDASHCONV-1143).
//!
//! Port of `DynamicBaseSerializer` (`app/serializers/base.py:12-196`) as one
//! shared kernel, plus the hand-written `IssueListDetailSerializer`
//! expansion (`app/serializers/issue.py:1165-1216`) used by
//! `issues-detail/`.
//!
//! Two distinct dialects exist:
//!
//! * **DynamicBase** — main retrieve (`IssueDetailSerializer`), archive
//!   retrieve (same class) and the flat `fields=`/`expand=` branch
//!   (`IssueSerializer`). `_filter_fields` appends a declared field for every
//!   requested mapper name the serializer does not already declare (the
//!   mapper there omits `issue_attachment`); `to_representation` then replaces
//!   or appends each key. Declared-but-not-mapped names are clobbered with
//!   `getattr(instance, "<name>_id", None)`. Mapper names that are not real
//!   `Issue` attributes make the *base* render raise (`AttributeError`), which
//!   is the generic 500. The `issue_attachments` leg at the end reads a local
//!   `IssueAttachmentLiteSerializer` that is imported inside the per-name
//!   branch, so a request whose every expand name misses `self.fields` raises
//!   `UnboundLocalError` — also a 500.
//! * **IssueListDetail** — `issues-detail/` appends only `issue_relation` /
//!   `issue_related` arrays from inline dicts; every other name (including
//!   `sub_issues`) is ignored.
//!
//! Unknown expand names are silently ignored on both sides.

use chrono::DateTime;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::serializer::{
    apply_expansion, expand_fallback_id, expansion_kind, needs_attachments, SelItem,
    EXPANSION_FIELDS_FILTER, EXPANSION_FIELDS_REPR,
};

use super::Denial;

/// Mapper names whose nested serializer is not yet ported. `issue_cycle`
/// renders `CycleIssueSerializer`, whose `issue_detail` nest is the
/// unported `IssueStateSerializer`; rather than render a wrong link row, this
/// port leaves it as the pre-existing residual gap (the base shape, no key).
/// Recorded in the PR and in the workpad.
const RESIDUAL_GAP_NAMES: &[&str] = &["issue_cycle"];

/// The `Issue` attributes the expansion mapper can read. Every other mapper
/// name (`user`, `default_assignee`, `project_lead`, `issue`, `actor`,
/// `owned_by`, `members`, `sub_issues`) is not an attribute, so `_filter_fields`
/// declaring it makes `super().to_representation` raise — the generic 500.
const ISSUE_EXPAND_ATTRS: &[&str] = &[
    "workspace",
    "project",
    "state",
    "created_by",
    "parent",
    "assignees",
    "labels",
    "issue_cycle",
    "issue_relation",
    "issue_intake",
    "issue_related",
    "issue_reactions",
    "issue_link",
];

/// `IssueSerializer.Meta.fields` (`issue.py:1071-1103`): 31 names. The flat
/// branch's serializer declares all of them even though seven annotation-backed
/// ones `SkipField` out of the body; `expand in self.fields` still sees them, so
/// a declared-field clobber can add a key the base body omitted.
pub const FLAT_EXPAND_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "type_id",
    "is_epic",
    "assigned_pod_id",
    "agent_executor",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "is_synced",
];

/// `IssueDetailSerializer.Meta.fields` (`issue.py:1250-1260`):
/// [`FLAT_EXPAND_FIELDS`] plus the seven detail-only names.
pub const DETAIL_EXPAND_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "type_id",
    "is_epic",
    "assigned_pod_id",
    "agent_executor",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "is_synced",
    "description_html",
    "is_subscribed",
    "is_intake",
    "agent_ticker",
    "agent_status",
    "relations_summary",
    "has_open_blockers",
];

/// The `Issue` values the declared-field clobber reads through
/// `getattr(instance, "<expand>_id", None)`. Only `<name>_id` attributes that
/// actually exist on the model can surface; every other declared name clobbers
/// to `null`.
pub struct ExpandAnchor {
    pub issue_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub estimate_point_id: Option<Uuid>,
}

impl ExpandAnchor {
    /// The `<name>_id` attribute map `expand_fallback_id` reads.
    fn fallback_instance(&self) -> Map<String, Value> {
        let mut map = Map::new();
        let mut put = |key: &str, value: Option<Uuid>| {
            map.insert(
                key.to_owned(),
                value.map_or(Value::Null, |id| Value::String(id.to_string())),
            );
        };
        put("created_by_id", self.created_by_id);
        put("updated_by_id", self.updated_by_id);
        put("project_id", Some(self.project_id));
        put("workspace_id", Some(self.workspace_id));
        put("state_id", self.state_id);
        put("parent_id", self.parent_id);
        put("estimate_point_id", self.estimate_point_id);
        map
    }
}

/// Split a raw `expand` query value the way `BaseAPIView.expand` does
/// (`base.py:184-186`): comma-split, empties dropped, `None` when empty.
pub fn split_expand(raw: Option<&str>) -> Option<Vec<String>> {
    crate::serializer::parse_list_param(raw)
}

/// Apply the `DynamicBaseSerializer` expansion to an already-rendered base
/// body (`base_fields` = the serializer's declared names, `anchor` = the issue
/// whose related rows are read).
pub async fn apply_dynamic_expand(
    pool: &PgPool,
    anchor: &ExpandAnchor,
    timezone: &Tz,
    base_fields: &[&str],
    expands: &[String],
    body: &mut Map<String, Value>,
) -> Result<(), Denial> {
    let selected: Vec<SelItem> = expands.iter().map(|name| SelItem::field(name)).collect();
    // `_filter_fields`: append a declared field for each mapped name.
    let mut declared: Vec<String> = base_fields.iter().map(|name| (*name).to_owned()).collect();
    apply_expansion(&mut declared, &selected, EXPANSION_FIELDS_FILTER)
        .map_err(|_| Denial::ServerError)?;

    // `super().to_representation` renders every declared field first: a mapped
    // name with no matching attribute raises `AttributeError` → 500 before any
    // expansion runs.
    for name in expands {
        if RESIDUAL_GAP_NAMES.contains(&name.as_str()) {
            continue;
        }
        let added = !base_fields.contains(&name.as_str())
            && expansion_kind(EXPANSION_FIELDS_FILTER, name).is_some();
        if added && !ISSUE_EXPAND_ATTRS.contains(&name.as_str()) {
            return Err(Denial::ServerError);
        }
    }

    // The `issue_attachments` leg names a serializer imported inside the
    // per-name branch; if no expand name entered that branch the name is
    // unbound and the leg raises `UnboundLocalError` → 500.
    let import_bound = expands
        .iter()
        .any(|name| declared.iter().any(|field| field == name));

    let fallback = anchor.fallback_instance();
    for name in expands {
        if RESIDUAL_GAP_NAMES.contains(&name.as_str()) {
            continue;
        }
        if !declared.iter().any(|field| field == name) {
            continue;
        }
        // A mapped attribute renders its nested serializer; a declared name
        // with no mapper entry is clobbered (`getattr(instance, "<name>_id")`).
        let nested = expansion_kind(EXPANSION_FIELDS_REPR, name).is_some()
            && ISSUE_EXPAND_ATTRS.contains(&name.as_str());
        let value = if nested {
            render_nested(pool, anchor, timezone, name).await?
        } else {
            expand_fallback_id(&fallback, name)
        };
        body.insert(name.clone(), value);
    }

    if needs_attachments(&[], &selected) {
        if !import_bound {
            return Err(Denial::ServerError);
        }
        body.insert(
            "issue_attachments".to_owned(),
            render_issue_attachments(pool, anchor, timezone).await?,
        );
    }
    Ok(())
}

/// Dispatch one real mapper name to its nested serializer.
async fn render_nested(
    pool: &PgPool,
    anchor: &ExpandAnchor,
    timezone: &Tz,
    name: &str,
) -> Result<Value, Denial> {
    match name {
        "workspace" => {
            let row = super::handlers_engage::fetch_workspace_lite(pool, &anchor.workspace_id)
                .await
                .map_err(|_| Denial::ServerError)?;
            super::handlers_engage::workspace_lite_value(&row).map_err(|_| Denial::ServerError)
        }
        "project" => {
            let row = super::handlers_engage::fetch_project_lite(pool, &anchor.project_id)
                .await
                .map_err(|_| Denial::ServerError)?;
            super::handlers_engage::project_lite_value(&row).map_err(|_| Denial::ServerError)
        }
        "state" => match anchor.state_id {
            Some(id) => render_state_lite(pool, id).await,
            None => Ok(Value::Object(Map::new())),
        },
        "created_by" => match anchor.created_by_id {
            Some(id) => render_user_lite(pool, id).await,
            None => Ok(user_lite_initial()),
        },
        "parent" => match anchor.parent_id {
            Some(id) => render_issue_lite(pool, id).await,
            None => Ok(Value::Object(Map::new())),
        },
        "assignees" => render_assignees(pool, anchor.issue_id).await,
        "labels" => render_labels(pool, anchor.issue_id).await,
        "issue_relation" => {
            render_relations(pool, anchor.issue_id, RelationSide::Outgoing, timezone).await
        }
        "issue_related" => {
            render_relations(pool, anchor.issue_id, RelationSide::Incoming, timezone).await
        }
        "issue_intake" => render_intake_lites(pool, anchor.issue_id, timezone).await,
        "issue_reactions" => render_reactions(pool, anchor.issue_id).await,
        "issue_link" => render_links(pool, anchor.issue_id, timezone).await,
        other => {
            debug_assert!(false, "unmapped expansion {other}");
            Ok(Value::Object(Map::new()))
        }
    }
}

/// Render one datetime string in the request user's zone (`render_datetime_in`,
/// as DRF `DateTimeField` does).
fn shift(raw: &Value, timezone: &Tz) -> Value {
    match raw {
        Value::String(text) => match DateTime::parse_from_rfc3339(text) {
            Ok(aware) => Value::String(crate::serializer::render_datetime_in(&aware, timezone)),
            Err(_) => raw.clone(),
        },
        other => other.clone(),
    }
}

fn opt_str(row: &Map<String, Value>, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn req_str(row: &Map<String, Value>, key: &str) -> Result<String, Denial> {
    opt_str(row, key).ok_or(Denial::ServerError)
}

fn req_i32(row: &Map<String, Value>, key: &str) -> Result<i32, Denial> {
    row.get(key)
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .ok_or(Denial::ServerError)
}

async fn single_row(
    pool: &PgPool,
    sql: &str,
    values: Vec<sea_query::Value>,
) -> Result<Map<String, Value>, Denial> {
    let mut rows = super::fetch_json_rows(pool, sql, values).await?;
    rows.pop().ok_or(Denial::ServerError)
}

fn uuid_value(id: Uuid) -> sea_query::Value {
    sea_query::Value::Uuid(Some(Box::new(id)))
}

async fn render_user_lite(pool: &PgPool, user_id: Uuid) -> Result<Value, Denial> {
    let row = super::handlers_engage::fetch_user_lite(pool, &user_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    super::handlers_engage::user_lite_value(&row).map_err(|_| Denial::ServerError)
}

/// `UserLiteSerializer(None).data`: DRF's unbound `.data` falls back to
/// `get_initial()`, which keeps every writable field in field order.
/// `first_name`, `last_name`, `avatar` and `display_name` are writable
/// CharFields with `initial = ''`; `id`/`is_bot` are read-only and
/// `avatar_url` is a model property (`app/serializers/user.py:141-153`).
fn user_lite_initial() -> Value {
    let mut initial = Map::with_capacity(4);
    initial.insert("first_name".to_owned(), Value::String(String::new()));
    initial.insert("last_name".to_owned(), Value::String(String::new()));
    initial.insert("avatar".to_owned(), Value::String(String::new()));
    initial.insert("display_name".to_owned(), Value::String(String::new()));
    Value::Object(initial)
}

async fn render_state_lite(pool: &PgPool, state_id: Uuid) -> Result<Value, Denial> {
    let row = single_row(
        pool,
        "SELECT id, name, color, \"group\" FROM states WHERE id = $1",
        vec![uuid_value(state_id)],
    )
    .await?;
    use pidash_services::app_issues::{state_lite_to_representation, StateLiteRow};
    let input = StateLiteRow {
        id: &req_str(&row, "id")?,
        name: &req_str(&row, "name")?,
        color: &req_str(&row, "color")?,
        group: &req_str(&row, "group")?,
    };
    serde_json::to_value(state_lite_to_representation(&input)).map_err(|_| Denial::ServerError)
}

async fn render_issue_lite(pool: &PgPool, issue_id: Uuid) -> Result<Value, Denial> {
    let row = single_row(
        pool,
        "SELECT id, sequence_id, project_id FROM issues WHERE id = $1",
        vec![uuid_value(issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{issue_lite_to_representation, IssueLiteRow};
    let input = IssueLiteRow {
        id: &req_str(&row, "id")?,
        sequence_id: req_i32(&row, "sequence_id")?,
        project_id: &req_str(&row, "project_id")?,
    };
    serde_json::to_value(issue_lite_to_representation(&input)).map_err(|_| Denial::ServerError)
}

/// `issue.assignees` — the user lite fleet. Django builds this from the
/// forward `ManyToManyField(through=IssueAssignee)` manager, which filters the
/// *target* model only (`create_forward_many_to_many_manager` sets
/// `self.model = rel.model`); `users` has no `deleted_at`, so the through row
/// is not filtered at all — a removed assignee (soft-deleted link row) still
/// renders.
async fn render_assignees(pool: &PgPool, issue_id: Uuid) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT u.id FROM issue_assignees ia JOIN users u ON u.id = ia.assignee_id \
         WHERE ia.issue_id = $1 \
         ORDER BY u.created_at DESC",
        vec![uuid_value(issue_id)],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = req_str(row, "id")?;
        let user_id = id.parse::<Uuid>().map_err(|_| Denial::ServerError)?;
        out.push(render_user_lite(pool, user_id).await?);
    }
    Ok(Value::Array(out))
}

/// `issue.labels` — app `LabelSerializer` over `issue_labels`. Django's
/// forward `ManyToManyField(through=IssueLabel)` manager filters the *target*
/// model only (`labels.deleted_at`), never the link row, so a removed label
/// (soft-deleted `issue_labels` row) still renders.
async fn render_labels(pool: &PgPool, issue_id: Uuid) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT l.parent_id, l.name, l.color, l.id, l.project_id, l.workspace_id, l.sort_order \
         FROM labels l JOIN issue_labels il ON il.label_id = l.id \
         WHERE il.issue_id = $1 AND l.deleted_at IS NULL \
         ORDER BY l.created_at DESC",
        vec![uuid_value(issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{app_label_to_representation, AppLabelRow};
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let sort_order = row
            .get("sort_order")
            .and_then(Value::as_f64)
            .ok_or(Denial::ServerError)?;
        let parent = opt_str(row, "parent_id");
        let project_id = opt_str(row, "project_id");
        let input = AppLabelRow {
            parent: parent.as_deref(),
            name: &req_str(row, "name")?,
            color: &req_str(row, "color")?,
            id: &req_str(row, "id")?,
            project_id: project_id.as_deref(),
            workspace_id: &req_str(row, "workspace_id")?,
            sort_order,
        };
        out.push(
            serde_json::to_value(app_label_to_representation(&input))
                .map_err(|_| Denial::ServerError)?,
        );
    }
    Ok(Value::Array(out))
}

#[derive(Clone, Copy)]
enum RelationSide {
    /// `issue.issue_relation` — rows whose `issue` is this issue.
    Outgoing,
    /// `issue.issue_related` — rows whose `related_issue` is this issue.
    Incoming,
}

/// `issue_relation` / `issue_related`: relation rows joined to the
/// *counterpart* issue's lite columns. Both are reverse FK managers over
/// `IssueRelation` (`related_name` on each FK), so Django applies
/// `IssueRelation.Meta.ordering` = `("-created_at",)` — hence
/// `ORDER BY issue_relations.created_at DESC`.
async fn render_relations(
    pool: &PgPool,
    issue_id: Uuid,
    side: RelationSide,
    timezone: &Tz,
) -> Result<Value, Denial> {
    // `side_col` is the link column; `other_col` the counterpart issue.
    let (side_col, other_col) = match side {
        RelationSide::Outgoing => ("r.issue_id", "r.related_issue_id"),
        RelationSide::Incoming => ("r.related_issue_id", "r.issue_id"),
    };
    let sql = format!(
        "SELECT o.id, o.project_id, o.sequence_id, o.name, r.relation_type, o.state_id, \
                o.priority, r.created_by_id, r.created_at, r.updated_at, r.updated_by_id \
         FROM issue_relations r JOIN issues o ON o.id = {other_col} \
         WHERE {side_col} = $1 AND r.deleted_at IS NULL \
         ORDER BY r.created_at DESC"
    );
    let rows = super::fetch_json_rows(pool, &sql, vec![uuid_value(issue_id)]).await?;
    use pidash_services::app_issues::{
        issue_relation_expand_to_representation, related_issue_expand_to_representation,
        IssueRelationExpandRow,
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let state_id = opt_str(row, "state_id");
        let created_by = opt_str(row, "created_by_id");
        let updated_by = opt_str(row, "updated_by_id");
        let created_at = shift(row.get("created_at").unwrap_or(&Value::Null), timezone);
        let updated_at = shift(row.get("updated_at").unwrap_or(&Value::Null), timezone);
        let input = IssueRelationExpandRow {
            id: &req_str(row, "id")?,
            project_id: &req_str(row, "project_id")?,
            sequence_id: req_i32(row, "sequence_id")?,
            relation_type: &req_str(row, "relation_type")?,
            name: &req_str(row, "name")?,
            state_id: state_id.as_deref(),
            priority: &req_str(row, "priority")?,
            created_by: created_by.as_deref(),
            created_at: created_at.as_str().unwrap_or_default(),
            updated_at: updated_at.as_str().unwrap_or_default(),
            updated_by: updated_by.as_deref(),
        };
        let value = match side {
            RelationSide::Outgoing => {
                serde_json::to_value(issue_relation_expand_to_representation(&input))
            }
            RelationSide::Incoming => {
                serde_json::to_value(related_issue_expand_to_representation(&input))
            }
        };
        out.push(value.map_err(|_| Denial::ServerError)?);
    }
    Ok(Value::Array(out))
}

/// `issue.issue_intake` — app `IntakeIssueLiteSerializer` rows (`-created_at`).
async fn render_intake_lites(
    pool: &PgPool,
    issue_id: Uuid,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT id, status, duplicate_to_id, snoozed_till, source FROM intake_issues \
         WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        vec![uuid_value(issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{intake_issue_lite_to_representation, IntakeIssueLiteRow};
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let duplicate_to = opt_str(row, "duplicate_to_id");
        let snoozed = row.get("snoozed_till").map(|raw| shift(raw, timezone));
        let snoozed_till = snoozed.as_ref().and_then(Value::as_str);
        let source = opt_str(row, "source");
        let input = IntakeIssueLiteRow {
            id: &req_str(row, "id")?,
            status: req_i32(row, "status")?,
            duplicate_to: duplicate_to.as_deref(),
            snoozed_till,
            source: source.as_deref(),
        };
        out.push(
            serde_json::to_value(intake_issue_lite_to_representation(&input))
                .map_err(|_| Denial::ServerError)?,
        );
    }
    Ok(Value::Array(out))
}

/// `issue.issue_reactions` — `IssueReactionLiteSerializer` rows with the
/// actor's `display_name`.
async fn render_reactions(pool: &PgPool, issue_id: Uuid) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT r.id, r.actor_id, r.issue_id, r.reaction, u.display_name AS display_name \
         FROM issue_reactions r JOIN users u ON u.id = r.actor_id \
         WHERE r.issue_id = $1 AND r.deleted_at IS NULL \
         ORDER BY r.created_at DESC",
        vec![uuid_value(issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{
        issue_reaction_lite_to_representation, IssueReactionLiteRow,
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let input = IssueReactionLiteRow {
            id: &req_str(row, "id")?,
            actor: &req_str(row, "actor_id")?,
            issue: &req_str(row, "issue_id")?,
            reaction: &req_str(row, "reaction")?,
            display_name: &req_str(row, "display_name")?,
        };
        out.push(
            serde_json::to_value(issue_reaction_lite_to_representation(&input))
                .map_err(|_| Denial::ServerError)?,
        );
    }
    Ok(Value::Array(out))
}

/// `issue.issue_link` — `IssueLinkLiteSerializer` rows (`-created_at`).
async fn render_links(pool: &PgPool, issue_id: Uuid, timezone: &Tz) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT l.id, l.issue_id, l.title, l.url, l.metadata, l.created_by_id, l.created_at \
         FROM issue_links l WHERE l.issue_id = $1 AND l.deleted_at IS NULL ORDER BY l.created_at DESC",
        vec![uuid_value(issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{issue_link_lite_to_representation, IssueLinkLiteRow};
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let created_at = shift(row.get("created_at").unwrap_or(&Value::Null), timezone);
        let title = opt_str(row, "title");
        let created_by = opt_str(row, "created_by_id");
        let metadata = row.get("metadata").cloned().unwrap_or(Value::Null);
        let input = IssueLinkLiteRow {
            id: &req_str(row, "id")?,
            issue_id: &req_str(row, "issue_id")?,
            title: title.as_deref(),
            url: &req_str(row, "url")?,
            metadata: &metadata,
            created_by_id: created_by.as_deref(),
            created_at: created_at.as_str().unwrap_or_default(),
        };
        out.push(
            serde_json::to_value(issue_link_lite_to_representation(&input))
                .map_err(|_| Denial::ServerError)?,
        );
    }
    Ok(Value::Array(out))
}

/// `issue_attachments` — `FileAsset.objects.filter(issue_id,
/// entity_type=ISSUE_ATTACHMENT)` (`base.py:177-195`), `-created_at`.
async fn render_issue_attachments(
    pool: &PgPool,
    anchor: &ExpandAnchor,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let rows = super::fetch_json_rows(
        pool,
        "SELECT fa.id, fa.asset, fa.attributes, fa.created_by_id AS created_by, \
                fa.updated_at, fa.updated_by_id AS updated_by, fa.entity_type, \
                fa.project_id, fa.issue_id, w.slug AS workspace_slug \
         FROM file_assets fa JOIN workspaces w ON w.id = fa.workspace_id \
         WHERE fa.issue_id = $1 AND fa.entity_type = 'ISSUE_ATTACHMENT' AND fa.deleted_at IS NULL \
         ORDER BY fa.created_at DESC",
        vec![uuid_value(anchor.issue_id)],
    )
    .await?;
    use pidash_services::app_issues::{
        issue_attachment_lite_to_representation, IssueAttachmentLiteRow,
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = req_str(row, "id")?;
        let project_id = opt_str(row, "project_id").unwrap_or_else(|| "None".to_owned());
        let issue_id = opt_str(row, "issue_id").unwrap_or_else(|| "None".to_owned());
        let workspace_slug = opt_str(row, "workspace_slug").unwrap_or_default();
        let asset_url = pidash_db::app_assets::columns::asset_url(
            opt_str(row, "entity_type").as_deref(),
            &id,
            &workspace_slug,
            &project_id,
            &issue_id,
        );
        let asset = opt_str(row, "asset").filter(|text| !text.is_empty());
        let created_by = opt_str(row, "created_by");
        let updated_by = opt_str(row, "updated_by");
        let updated_at = shift(row.get("updated_at").unwrap_or(&Value::Null), timezone);
        let attributes = row.get("attributes").cloned().unwrap_or(Value::Null);
        let input = IssueAttachmentLiteRow {
            id: &id,
            asset: asset.as_deref(),
            attributes: &attributes,
            created_by: created_by.as_deref(),
            updated_at: updated_at.as_str().unwrap_or_default(),
            updated_by: updated_by.as_deref(),
            asset_url: asset_url.as_deref(),
        };
        out.push(
            serde_json::to_value(issue_attachment_lite_to_representation(&input))
                .map_err(|_| Denial::ServerError)?,
        );
    }
    Ok(Value::Array(out))
}

/// `issues-detail/` expansion (`IssueListDetailSerializer.to_representation`,
/// `issue.py:1165-1216`): only `issue_relation` / `issue_related` are appended,
/// from inline dicts. Datetimes stay raw (UTC, `Z`) — no `DateTimeField` runs.
pub async fn append_detail_list_expand(
    pool: &PgPool,
    issue_id: Uuid,
    expands: &[String],
    body: &mut Map<String, Value>,
) -> Result<(), Denial> {
    if expands.iter().any(|name| name == "issue_relation") {
        body.insert(
            "issue_relation".to_owned(),
            render_detail_list_relations(pool, issue_id, RelationSide::Outgoing).await?,
        );
    }
    if expands.iter().any(|name| name == "issue_related") {
        body.insert(
            "issue_related".to_owned(),
            render_detail_list_relations(pool, issue_id, RelationSide::Incoming).await?,
        );
    }
    Ok(())
}

/// Read a UUID column (or null) out of a `row_to_json` map — the anchor
/// readers for the flat and archive paths.
pub fn row_uuid(row: &Map<String, Value>, key: &str) -> Result<Option<Uuid>, Denial> {
    match row.get(key).and_then(Value::as_str) {
        Some(text) => text.parse().map(Some).map_err(|_| Denial::ServerError),
        None => Ok(None),
    }
}

/// The inline relation dicts of `IssueListDetailSerializer` — key order
/// differs from the `IssueRelationSerializer` (name before relation_type) and
/// `state_id` is never omitted (raw `related_issue.state_id`, may be null).
async fn render_detail_list_relations(
    pool: &PgPool,
    issue_id: Uuid,
    side: RelationSide,
) -> Result<Value, Denial> {
    let (side_col, other_col) = match side {
        RelationSide::Outgoing => ("r.issue_id", "r.related_issue_id"),
        RelationSide::Incoming => ("r.related_issue_id", "r.issue_id"),
    };
    let sql = format!(
        "SELECT o.id, o.project_id, o.sequence_id, o.name, r.relation_type, o.state_id, \
                o.priority, o.created_by_id, o.created_at, o.updated_at, o.updated_by_id \
         FROM issue_relations r JOIN issues o ON o.id = {other_col} \
         WHERE {side_col} = $1 AND r.deleted_at IS NULL \
         ORDER BY r.created_at DESC"
    );
    let rows = super::fetch_json_rows(pool, &sql, vec![uuid_value(issue_id)]).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut map = Map::new();
        for (key, value) in [
            ("id", row.get("id")),
            ("project_id", row.get("project_id")),
            ("sequence_id", row.get("sequence_id")),
            ("name", row.get("name")),
        ] {
            map.insert(key.to_owned(), value.cloned().unwrap_or(Value::Null));
        }
        map.insert(
            "relation_type".to_owned(),
            row.get("relation_type").cloned().unwrap_or(Value::Null),
        );
        for (key, value) in [
            ("state_id", row.get("state_id")),
            ("priority", row.get("priority")),
            ("created_by", row.get("created_by_id")),
        ] {
            map.insert(key.to_owned(), value.cloned().unwrap_or(Value::Null));
        }
        // Raw values: DRF's JSONEncoder renders UTC datetimes with a `Z`.
        for (key, col) in [("created_at", "created_at"), ("updated_at", "updated_at")] {
            map.insert(
                key.to_owned(),
                match row.get(col).and_then(Value::as_str) {
                    Some(text) => match DateTime::parse_from_rfc3339(text) {
                        Ok(aware) => Value::String(crate::serializer::render_datetime(&aware)),
                        Err(_) => Value::Null,
                    },
                    None => Value::Null,
                },
            );
        }
        // Django's inline dict closes with `updated_by` last.
        map.insert(
            "updated_by".to_owned(),
            row.get("updated_by_id").cloned().unwrap_or(Value::Null),
        );
        out.push(Value::Object(map));
    }
    Ok(Value::Array(out))
}
