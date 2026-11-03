//! D-08 activity field trackers (services layer).
//!
//! Port of `apps/api/pi_dash/bgtasks/issue_activities_task.py:41-556`:
//! `extract_ids` (`:41-49`) plus the twelve per-field diff builders
//! `track_name` (`:50-77`), `track_description` (`:78-114`),
//! `track_parent` (`:115-160`), `track_priority` (`:161-188`),
//! `track_state` (`:189-229`), `track_target_date` (`:230-259`),
//! `track_start_date` (`:260-289`), `track_labels` (`:290-356`),
//! `track_assignees` (`:357-432`), `track_estimate_points` (`:433-477`),
//! `track_archive_at` (`:478-526`) and `track_closed_to` (`:527-556`).
//!
//! Every helper is pure over injected snapshots: the Django ORM reads the
//! Python performs inline (`Label.objects.get`, `State.objects.filter`,
//! `Issue.objects.filter`, `EstimatePoint.objects.filter`,
//! `User.objects.get`, the latest-`IssueActivity` query in
//! `track_description`) arrive here as pre-resolved arguments — a snapshot
//! struct for `.filter(...).first()` (missing row = `None`) or a resolver
//! closure for `.get(...)` (missing row = [`TrackError::RowMissing`]).
//! The jobs layer (PIDASHCONV-198, `issue_activity` dispatcher) owns the
//! actual queries, the `bulk_create` writes and the `timezone.now()` touch,
//! under the same call order as the Python.
//!
//! Parameter mapping (same semantics, Rust shapes): `requested_data` and
//! `current_instance` are the JSON-decoded dicts `update_issue_activity`
//! (`:624-625`) builds — `&serde_json::Value` objects here. The five
//! per-row passthroughs (`issue_id`, `project_id`, `workspace_id`,
//! `actor_id`, `epoch`) travel as [`TrackFrame`]; emitted rows are
//! [`IssueActivityDraft`] (plus [`IssueSubscriberDraft`] for the assignee
//! side effect). `actor_id` is the stringified actor throughout: Python
//! compares `actor_id == str(last_activity.actor_id)` (`:93`).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG-ACT-01 (`:469`): `track_estimate_points` builds
//!   `field="estimate_" + new_estimate.estimate.type` unguarded, so clearing
//!   the estimate (`new_estimate is None`) raises `AttributeError` before any
//!   row is appended — the `verb="removed"` branch (`:458`) is dead code.
//!   [`plan_track_estimate_points`] returns
//!   [`TrackError::EstimateRemovedField`] on that path instead of a row.
//! * BUG-ACT-02 (`:499`): the restore row hardcodes `old_value="archive"`
//!   even when the previous value was a manual archive. Kept as-is.
//!
//! # Deliberate deviations (documented, not bugs)
//!
//! * Set iteration order: Python iterates `added_labels` / `dropped_labels`
//!   / assignee sets in hash order (nondeterministic); the port collects
//!   [`extract_ids`] into a `BTreeSet` and emits rows in sorted order.
//!   Every recorded golden replays; tests compare multi-row output as sets.
//! * `bool`/`int`/`float`/`None` elements render via Python `str()`
//!   (`True`/`False`/`None`, floats `1.0`-style); container elements fall
//!   back to compact JSON — unreachable in contract (id lists hold UUID
//!   strings). Out-of-contract float exponents render Rust-style
//!   (`1e300`, not Python's `1e+300`).
//!
//! Fixture: `rust-api/fixtures/tasks_webhooks/fx-act-01-track-helpers.json`
//! (FX-ACT-01). The `#[cfg(test)]` suite below asserts every golden there.

use std::collections::BTreeSet;

use serde_json::Value;

/// One appended `IssueActivity` row: every column the twelve helpers write.
///
/// `old_value` / `new_value` are `None` exactly where the Python passes
/// `None` (a NULL column); `""` is a real empty string (labels/assignees
/// added/dropped rows, cleared dates). Identifier columns are `None` where
/// the Python omits the kwarg.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityDraft {
    pub issue_id: String,
    pub actor_id: String,
    pub verb: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub field: String,
    pub project_id: String,
    pub workspace_id: String,
    pub comment: String,
    pub old_identifier: Option<String>,
    pub new_identifier: Option<String>,
    pub epoch: f64,
}

/// One collected `IssueSubscriber` row (`track_assignees` `:396-405`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueSubscriberDraft {
    pub subscriber_id: String,
    pub issue_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub created_by_id: String,
    pub updated_by_id: String,
}

/// The five per-row passthroughs every helper threads into its rows
/// (`issue_id`, `project_id`, `workspace_id`, `actor_id`, `epoch`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackFrame {
    pub issue_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub actor_id: String,
    pub epoch: f64,
}

/// A resolved parent `Issue` row: `Issue.objects.filter(pk=...).first()`
/// (`:135-136`), rendered `f"{identifier}-{sequence_id}"` (`:144,147`).
#[derive(Debug, Clone, PartialEq)]
pub struct ParentRef {
    pub id: String,
    pub identifier: String,
    pub sequence_id: i32,
}

/// A resolved `State` row (`track_state` `:208-209`, `track_closed_to`
/// `:538`).
#[derive(Debug, Clone, PartialEq)]
pub struct StateRef {
    pub id: String,
    pub name: String,
}

/// A resolved `Label` row (`track_labels` `:313,337`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRef {
    pub id: String,
    pub name: String,
}

/// A resolved `User` row (`track_assignees` `:380,415`).
#[derive(Debug, Clone, PartialEq)]
pub struct UserRef {
    pub id: String,
    pub display_name: String,
}

/// A resolved `EstimatePoint` row plus its `Estimate.type`
/// (`track_estimate_points` `:444-453,469`). `value` is a `CharField`
/// (`db/models/estimate.py:47`).
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatePointRef {
    pub value: String,
    pub estimate_type: String,
}

/// The latest-`IssueActivity` projection `track_description` reads
/// (`:89-94`): only `field` and the stringified `actor_id` matter.
#[derive(Debug, Clone, PartialEq)]
pub struct DescriptionLatest {
    pub field: String,
    pub actor_id: String,
}

/// `track_description` outcome (`:90-111`): no change, touch the latest row
/// (`created_at = now()`, `save(update_fields=["created_at"])` — the jobs
/// layer performs the write), or append a fresh row.
#[derive(Debug, Clone, PartialEq)]
pub enum DescriptionOutcome {
    Unchanged,
    TouchLatest,
    Append(Box<IssueActivityDraft>),
}

/// `track_assignees` outcome: appended rows plus the collected
/// `IssueSubscriber` rows the jobs layer `bulk_create`s
/// (`batch_size=10, ignore_conflicts=True`) after the added loop and before
/// the dropped loop (`:407-408`).
#[derive(Debug, Clone, PartialEq)]
pub struct AssigneeOutcome {
    pub activities: Vec<IssueActivityDraft>,
    pub subscribers: Vec<IssueSubscriberDraft>,
}

/// Failure modes that mirror the Python raises (the dispatcher's
/// broad-except swallows them; see FX-ACT-03).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TrackError {
    /// A present-but-`None` (or otherwise non-iterable) id-list value:
    /// `{str(x) for x in None}` raises `TypeError` (`:45-46`).
    #[error("extract_ids: value under key '{key}' is not iterable")]
    NotIterable { key: String },
    /// `.get(pk=...)` with no matching row (`Label` `:313,337`, `User`
    /// `:380,415`, `State` `:538`): Django raises `DoesNotExist`.
    #[error("row lookup missed: {table} pk={pk}")]
    RowMissing { table: &'static str, pk: String },
    /// BUG-ACT-01: the `field=` expression raises `AttributeError` when the
    /// new estimate is `None` (`:469`).
    #[error("track_estimate_points: field expression on a cleared estimate")]
    EstimateRemovedField,
}

/// `pi_dash/utils/uuid.py:10` — `uuid.UUID(s).version == 4`,
/// `ValueError` mapping to `False`.
///
/// `Uuid::parse_str` accepts the same four spellings Python's constructor
/// does (hyphenated, 32-hex, `{braced}`, `urn:uuid:`); `version == 4`
/// becomes `get_version() == Some(Version::Random)`, so v1 UUIDs and the nil
/// UUID both read invalid, exactly as in Python.
pub fn is_valid_uuid(value: &str) -> bool {
    match value.parse::<uuid::Uuid>() {
        Ok(id) => id.get_version() == Some(uuid::Version::Random),
        Err(_) => false,
    }
}

/// Python `str()` over a JSON scalar: `None` → `"None"`,
/// `True`/`False`, ints plain, floats `1.0`-style. Container input is
/// out of contract (id lists hold scalars) and renders as compact JSON.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 1e16 {
                    format!("{f:.1}")
                } else {
                    format!("{f:?}")
                }
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `Some(string)` for any present non-`None` value, `None` for a missing key
/// or JSON `null` — the `.get()` projection most helpers compare and store.
fn opt_string(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => Some(python_str(v)),
    }
}

/// Python truthiness over JSON (`or`-chains, the `automation` flag): empty
/// string / `0` / `false` / `null` / missing / empty containers are falsy.
fn python_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(m)) => !m.is_empty(),
    }
}

/// `data.get("x_id") or data.get("x")` (`:125-126,199-200`): first truthy
/// raw id, stringified; falsy spellings (`""`, `0`, `null`, missing) fall
/// through, exactly like Python `or`.
fn first_id(data: &Value, primary: &str, fallback: &str) -> Option<String> {
    [primary, fallback]
        .into_iter()
        .map(|key| data.get(key))
        .find(|v| python_truthy(*v))
        .and_then(opt_string)
}

/// Fill the five passthrough columns shared by every row.
fn draft(
    frame: &TrackFrame,
    field: &str,
    comment: &str,
    old_value: Option<String>,
    new_value: Option<String>,
) -> IssueActivityDraft {
    IssueActivityDraft {
        issue_id: frame.issue_id.clone(),
        actor_id: frame.actor_id.clone(),
        verb: "updated".to_owned(),
        old_value,
        new_value,
        field: field.to_owned(),
        project_id: frame.project_id.clone(),
        workspace_id: frame.workspace_id.clone(),
        comment: comment.to_owned(),
        old_identifier: None,
        new_identifier: None,
        epoch: frame.epoch,
    }
}

/// `extract_ids` (`:41-49`): the primary key's *presence* wins even when its
/// list is empty; the fallback defaults to `[]`; `None` input (or `{}`)
/// short-circuits to an empty set; a present-but-`None` value raises
/// (`TypeError`), ported as [`TrackError::NotIterable`].
///
/// A present string value iterates its characters, mirroring
/// `{str(x) for x in "ab"}` → `{"a", "b"}`.
pub fn extract_ids(
    data: Option<&Value>,
    primary_key: &str,
    fallback_key: &str,
) -> Result<BTreeSet<String>, TrackError> {
    let data = match data {
        None => return Ok(BTreeSet::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(BTreeSet::new()),
        Some(v) => v,
    };
    let (key, values) = match (data.get(primary_key), data.get(fallback_key)) {
        (Some(v), _) => (primary_key, v),
        (None, Some(v)) => (fallback_key, v),
        (None, None) => return Ok(BTreeSet::new()),
    };
    match values {
        Value::Array(items) => Ok(items.iter().map(python_str).collect()),
        Value::String(s) => Ok(s.chars().map(|c| c.to_string()).collect()),
        _ => Err(TrackError::NotIterable {
            key: key.to_owned(),
        }),
    }
}

/// `track_name` (`:50-77`): one row when `name` differs (including
/// `None` vs a value); no row when equal, `None` vs `None` included.
pub fn plan_track_name(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
) -> Option<IssueActivityDraft> {
    if current.get("name") != requested.get("name") {
        Some(draft(
            frame,
            "name",
            "updated the name to",
            opt_string(current.get("name")),
            opt_string(requested.get("name")),
        ))
    } else {
        None
    }
}

/// `track_description` (`:78-114`): compares `description_html` on both
/// sides (the row field is `"description"`). On a difference the latest
/// `IssueActivity` for the issue (`order_by -created_at`) decides: same
/// field `"description"` and same stringified actor → touch instead of
/// appending; otherwise append.
pub fn plan_track_description(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    latest: Option<&DescriptionLatest>,
) -> DescriptionOutcome {
    if current.get("description_html") == requested.get("description_html") {
        return DescriptionOutcome::Unchanged;
    }
    match latest {
        Some(last) if last.field == "description" && last.actor_id == frame.actor_id => {
            DescriptionOutcome::TouchLatest
        }
        _ => DescriptionOutcome::Append(Box::new(draft(
            frame,
            "description",
            "updated the description to",
            opt_string(current.get("description_html")),
            opt_string(requested.get("description_html")),
        ))),
    }
}

/// `track_parent` (`:115-160`): `parent_id or parent` on both sides, UUID
/// validation *before* the equality check (`None` skips validation, an
/// invalid id returns silently with no row), then
/// `Issue.objects.filter(pk=...).first()` per non-`None` side (`None` id
/// means no query). Display is `f"{identifier}-{sequence_id}"`, `""` with a
/// `None` identifier when there is no parent.
pub fn plan_track_parent(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<ParentRef>,
) -> Option<IssueActivityDraft> {
    let current_id = first_id(current, "parent_id", "parent");
    let requested_id = first_id(requested, "parent_id", "parent");
    if current_id.as_deref().is_some_and(|id| !is_valid_uuid(id)) {
        return None;
    }
    if requested_id.as_deref().is_some_and(|id| !is_valid_uuid(id)) {
        return None;
    }
    if current_id != requested_id {
        let old = current_id.as_deref().and_then(resolve);
        let new = requested_id.as_deref().and_then(resolve);
        let mut row = draft(
            frame,
            "parent",
            "updated the parent issue to",
            Some(
                old.as_ref()
                    .map(|p| format!("{}-{}", p.identifier, p.sequence_id))
                    .unwrap_or_default(),
            ),
            Some(
                new.as_ref()
                    .map(|p| format!("{}-{}", p.identifier, p.sequence_id))
                    .unwrap_or_default(),
            ),
        );
        row.old_identifier = old.map(|p| p.id);
        row.new_identifier = new.map(|p| p.id);
        Some(row)
    } else {
        None
    }
}

/// `track_priority` (`:161-188`): one row when `priority` differs.
pub fn plan_track_priority(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
) -> Option<IssueActivityDraft> {
    if current.get("priority") != requested.get("priority") {
        Some(draft(
            frame,
            "priority",
            "updated the priority to",
            opt_string(current.get("priority")),
            opt_string(requested.get("priority")),
        ))
    } else {
        None
    }
}

/// `track_state` (`:189-229`): `state_id or state` on both sides; an invalid
/// id coerces to `None` (a row still follows when the sides differ —
/// unlike `track_parent`'s silent return). Lookups scope
/// `filter(pk=..., project_id=project_id)`; the resolver closure owns that
/// scoping. Missing rows read as `None` names/identifiers, no raise.
pub fn plan_track_state(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<StateRef>,
) -> Option<IssueActivityDraft> {
    let mut current_id = first_id(current, "state_id", "state");
    let mut requested_id = first_id(requested, "state_id", "state");
    if current_id.as_deref().is_some_and(|id| !is_valid_uuid(id)) {
        current_id = None;
    }
    if requested_id.as_deref().is_some_and(|id| !is_valid_uuid(id)) {
        requested_id = None;
    }
    if current_id != requested_id {
        let old = current_id.as_deref().and_then(resolve);
        let new = requested_id.as_deref().and_then(resolve);
        let mut row = draft(
            frame,
            "state",
            "updated the state to",
            old.as_ref().map(|s| s.name.clone()),
            new.as_ref().map(|s| s.name.clone()),
        );
        row.old_identifier = old.map(|s| s.id);
        row.new_identifier = new.map(|s| s.id);
        Some(row)
    } else {
        None
    }
}

/// `track_target_date` (`:230-259`): one row on difference; `None` renders
/// as `""` on whichever side is `None`. The comment has no trailing space.
pub fn plan_track_target_date(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
) -> Option<IssueActivityDraft> {
    if current.get("target_date") != requested.get("target_date") {
        Some(draft(
            frame,
            "target_date",
            "updated the target date to",
            Some(opt_string(current.get("target_date")).unwrap_or_default()),
            Some(opt_string(requested.get("target_date")).unwrap_or_default()),
        ))
    } else {
        None
    }
}

/// `track_start_date` (`:260-289`): same shape as the target date, except
/// the comment carries a trailing space — `"updated the start date to "` —
/// ported byte for byte.
pub fn plan_track_start_date(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
) -> Option<IssueActivityDraft> {
    if current.get("start_date") != requested.get("start_date") {
        Some(draft(
            frame,
            "start_date",
            "updated the start date to ",
            Some(opt_string(current.get("start_date")).unwrap_or_default()),
            Some(opt_string(requested.get("start_date")).unwrap_or_default()),
        ))
    } else {
        None
    }
}

/// `track_labels` (`:290-356`): `extract_ids(..., "label_ids", "labels")`
/// on both sides, set difference both ways, non-UUID ids skipped silently,
/// `Label.objects.get(pk=...)` per id (missing row raises, ported as
/// [`TrackError::RowMissing`]). Added rows precede dropped rows.
pub fn plan_track_labels(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<LabelRef>,
) -> Result<Vec<IssueActivityDraft>, TrackError> {
    let requested_ids = extract_ids(Some(requested), "label_ids", "labels")?;
    let current_ids = extract_ids(Some(current), "label_ids", "labels")?;
    let mut out = Vec::new();
    for id in requested_ids.difference(&current_ids) {
        if !is_valid_uuid(id) {
            continue;
        }
        let label = resolve(id).ok_or_else(|| TrackError::RowMissing {
            table: "Label",
            pk: id.clone(),
        })?;
        let mut row = draft(
            frame,
            "labels",
            "added label ",
            Some(String::new()),
            Some(label.name.clone()),
        );
        row.new_identifier = Some(label.id.clone());
        out.push(row);
    }
    for id in current_ids.difference(&requested_ids) {
        if !is_valid_uuid(id) {
            continue;
        }
        let label = resolve(id).ok_or_else(|| TrackError::RowMissing {
            table: "Label",
            pk: id.clone(),
        })?;
        let mut row = draft(
            frame,
            "labels",
            "removed label ",
            Some(label.name.clone()),
            Some(String::new()),
        );
        row.old_identifier = Some(label.id.clone());
        out.push(row);
    }
    Ok(out)
}

/// `track_assignees` (`:357-432`): `extract_ids(... "assignee_ids",
/// "assignees")` both sides, set difference both ways (note the source's
/// `dropped_assginees` spelling), non-UUID ids skipped, `User.objects.get`
/// per id (missing row raises, ported as [`TrackError::RowMissing`]).
/// Added rows also collect `IssueSubscriber` rows bulk-created after the
/// added loop and before the dropped loop.
pub fn plan_track_assignees(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<UserRef>,
) -> Result<AssigneeOutcome, TrackError> {
    let requested_ids = extract_ids(Some(requested), "assignee_ids", "assignees")?;
    let current_ids = extract_ids(Some(current), "assignee_ids", "assignees")?;
    let mut activities = Vec::new();
    let mut subscribers = Vec::new();
    for id in requested_ids.difference(&current_ids) {
        if !is_valid_uuid(id) {
            continue;
        }
        let assignee = resolve(id).ok_or_else(|| TrackError::RowMissing {
            table: "User",
            pk: id.clone(),
        })?;
        let mut row = draft(
            frame,
            "assignees",
            "added assignee ",
            Some(String::new()),
            Some(assignee.display_name.clone()),
        );
        row.new_identifier = Some(assignee.id.clone());
        activities.push(row);
        subscribers.push(IssueSubscriberDraft {
            subscriber_id: assignee.id.clone(),
            issue_id: frame.issue_id.clone(),
            workspace_id: frame.workspace_id.clone(),
            project_id: frame.project_id.clone(),
            created_by_id: assignee.id.clone(),
            updated_by_id: assignee.id.clone(),
        });
    }
    for id in current_ids.difference(&requested_ids) {
        if !is_valid_uuid(id) {
            continue;
        }
        let assignee = resolve(id).ok_or_else(|| TrackError::RowMissing {
            table: "User",
            pk: id.clone(),
        })?;
        let mut row = draft(
            frame,
            "assignees",
            "removed assignee ",
            Some(assignee.display_name.clone()),
            Some(String::new()),
        );
        row.old_identifier = Some(assignee.id.clone());
        activities.push(row);
    }
    Ok(AssigneeOutcome {
        activities,
        subscribers,
    })
}

/// `track_estimate_points` (`:433-477`): compares `estimate_point`
/// (singular) on both sides; each side resolves
/// `EstimatePoint.objects.filter(pk=...).first()` (`None` id means no
/// query). Identifiers echo the raw ids; values come from the snapshots.
/// Clearing the estimate hits BUG-ACT-01 ([`TrackError::EstimateRemovedField`]).
///
/// A non-UUID raw id reaches the resolver untouched, mirroring
/// `.filter(pk=...)` (which raises on garbage) — unlike the parent/state
/// helpers this path validates nothing.
pub fn plan_track_estimate_points(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<EstimatePointRef>,
) -> Result<Option<IssueActivityDraft>, TrackError> {
    if current.get("estimate_point") == requested.get("estimate_point") {
        return Ok(None);
    }
    let old = opt_string(current.get("estimate_point"))
        .as_deref()
        .and_then(resolve);
    let new_raw = opt_string(requested.get("estimate_point"));
    let new = new_raw.as_deref().and_then(resolve);
    let new = match new {
        Some(n) => n,
        None => return Err(TrackError::EstimateRemovedField),
    };
    // Identifiers echo the raw ids (`:459-466`), not the snapshot rows.
    let mut row = draft(
        frame,
        &format!("estimate_{}", new.estimate_type),
        "updated the estimate point to ",
        old.map(|e| e.value),
        Some(new.value.clone()),
    );
    row.old_identifier = opt_string(current.get("estimate_point"));
    row.new_identifier = new_raw;
    Ok(Some(row))
}

/// `track_archive_at` (`:478-526`): one row when `archived_at` differs. A
/// `None` (or missing) requested value restores — BUG-ACT-02: `old_value`
/// hardcodes `"archive"`. Otherwise the `automation` flag picks the comment
/// and the new value (`"archive"` vs `"manual_archive"`); `old_value` stays
/// `None`.
pub fn plan_track_archive_at(
    requested: &Value,
    current: &Value,
    frame: &TrackFrame,
) -> Option<IssueActivityDraft> {
    if current.get("archived_at") == requested.get("archived_at") {
        return None;
    }
    // `requested_data.get("archived_at") is None` — a missing key counts.
    if opt_string(requested.get("archived_at")).is_none() {
        return Some(draft(
            frame,
            "archived_at",
            "has restored the issue",
            Some("archive".to_owned()),
            Some("restore".to_owned()),
        ));
    }
    let automated = python_truthy(requested.get("automation"));
    let (comment, new_value) = if automated {
        ("Pi Dash has archived the issue", "archive")
    } else {
        ("Actor has archived the issue", "manual_archive")
    };
    Some(draft(
        frame,
        "archived_at",
        comment,
        None,
        Some(new_value.to_owned()),
    ))
}

/// `track_closed_to` (`:527-556`): only when `closed_to` is present and not
/// `None` — the current side is never compared. `State.objects.get(pk=...,
/// project_id=...)` (missing row raises, ported as [`TrackError::RowMissing`];
/// the resolver closure owns the `project_id` scoping). The comment carries
/// a trailing space — `"Pi Dash updated the state to "` — ported byte for
/// byte; the row field is `"state"`.
pub fn plan_track_closed_to(
    requested: &Value,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Option<StateRef>,
) -> Result<Option<IssueActivityDraft>, TrackError> {
    let closed_to = match opt_string(requested.get("closed_to")) {
        Some(id) => id,
        None => return Ok(None),
    };
    let state = resolve(&closed_to).ok_or_else(|| TrackError::RowMissing {
        table: "State",
        pk: closed_to.clone(),
    })?;
    let mut row = draft(
        frame,
        "state",
        "Pi Dash updated the state to ",
        None,
        Some(state.name.clone()),
    );
    row.new_identifier = Some(state.id.clone());
    Ok(Some(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Version-4 UUIDs (valid per `is_valid_uuid`); the third group starts
    /// with `4`, the fourth with `8` (RFC 4122 variant).
    const LABEL_A: &str = "11111111-1111-4111-8111-111111111111";
    const LABEL_B: &str = "22222222-2222-4222-8222-222222222222";
    const STATE_DONE: &str = "33333333-3333-4333-8333-333333333333";
    const USER_ADA: &str = "44444444-4444-4444-8444-444444444444";
    const PARENT_OLD: &str = "55555555-5555-4555-8555-555555555555";
    const PARENT_NEW: &str = "66666666-6666-4666-8666-666666666666";

    fn frame() -> TrackFrame {
        TrackFrame {
            issue_id: "issue-1".to_owned(),
            project_id: "proj-1".to_owned(),
            workspace_id: "ws-1".to_owned(),
            actor_id: "actor-1".to_owned(),
            epoch: 1750000000.5,
        }
    }

    // FX-ACT-01 · extract_ids.
    #[test]
    fn extract_ids_none_and_missing_keys() {
        assert!(extract_ids(None, "label_ids", "labels").unwrap().is_empty());
        assert!(extract_ids(Some(&json!({})), "label_ids", "labels")
            .unwrap()
            .is_empty());
        assert!(
            extract_ids(Some(&json!({"other": ["x"]})), "label_ids", "labels")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn extract_ids_primary_fallback_and_precedence() {
        // Primary key wins with a set of str(x).
        assert_eq!(
            extract_ids(
                Some(&json!({"label_ids": ["b", "a"]})),
                "label_ids",
                "labels"
            )
            .unwrap(),
            BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        // Fallback key when the primary is absent.
        assert_eq!(
            extract_ids(Some(&json!({"labels": ["x"]})), "label_ids", "labels").unwrap(),
            BTreeSet::from(["x".to_owned()])
        );
        // Primary PRESENCE wins even when its list is empty.
        assert!(extract_ids(
            Some(&json!({"label_ids": [], "labels": ["x"]})),
            "label_ids",
            "labels"
        )
        .unwrap()
        .is_empty());
        // A None VALUE under either key raises (TypeError → NotIterable).
        assert_eq!(
            extract_ids(Some(&json!({"label_ids": null})), "label_ids", "labels"),
            Err(TrackError::NotIterable {
                key: "label_ids".to_owned()
            })
        );
        assert_eq!(
            extract_ids(Some(&json!({"labels": null})), "label_ids", "labels"),
            Err(TrackError::NotIterable {
                key: "labels".to_owned()
            })
        );
    }

    #[test]
    fn uuid_check_is_version_4_only() {
        assert!(is_valid_uuid(LABEL_A));
        assert!(!is_valid_uuid("not-a-uuid"));
        // A v1 UUID parses but is not version 4.
        assert!(!is_valid_uuid("6ba7b810-9dad-11d1-80b4-00c04fd430c8"));
        // The nil UUID is not version 4 either.
        assert!(!is_valid_uuid("00000000-0000-0000-0000-000000000000"));
    }

    // FX-ACT-01 · name.
    #[test]
    fn track_name_changed_and_same() {
        let f = frame();
        let row = plan_track_name(&json!({"name": "New"}), &json!({"name": "Old"}), &f).unwrap();
        assert_eq!(row.field, "name");
        assert_eq!(row.comment, "updated the name to");
        assert_eq!(row.verb, "updated");
        assert_eq!(row.old_value.as_deref(), Some("Old"));
        assert_eq!(row.new_value.as_deref(), Some("New"));
        assert_eq!(row.issue_id, "issue-1");
        assert_eq!(row.actor_id, "actor-1");
        assert_eq!(row.epoch, 1750000000.5);
        // Equal (including both None) → no row.
        assert_eq!(
            plan_track_name(&json!({"name": "X"}), &json!({"name": "X"}), &f),
            None
        );
        assert_eq!(plan_track_name(&json!({}), &json!({}), &f), None);
    }

    // FX-ACT-01 · description.
    #[test]
    fn track_description_append_touch_and_unchanged() {
        let f = frame();
        let requested = json!({"description_html": "<p>new</p>"});
        let current = json!({"description_html": "<p>old</p>"});
        // No latest activity → append.
        match plan_track_description(&requested, &current, &f, None) {
            DescriptionOutcome::Append(row) => {
                assert_eq!(row.field, "description");
                assert_eq!(row.comment, "updated the description to");
                assert_eq!(row.old_value.as_deref(), Some("<p>old</p>"));
                assert_eq!(row.new_value.as_deref(), Some("<p>new</p>"));
            }
            other => panic!("expected Append, got {other:?}"),
        }
        // Latest row is another field → append.
        let other = DescriptionLatest {
            field: "name".to_owned(),
            actor_id: "actor-1".to_owned(),
        };
        assert!(matches!(
            plan_track_description(&requested, &current, &f, Some(&other)),
            DescriptionOutcome::Append(_)
        ));
        // Latest row is this field by the same actor → touch, no new row.
        let same = DescriptionLatest {
            field: "description".to_owned(),
            actor_id: "actor-1".to_owned(),
        };
        assert_eq!(
            plan_track_description(&requested, &current, &f, Some(&same)),
            DescriptionOutcome::TouchLatest
        );
        // Latest row is this field by another actor → append.
        let foreign = DescriptionLatest {
            field: "description".to_owned(),
            actor_id: "someone-else".to_owned(),
        };
        assert!(matches!(
            plan_track_description(&requested, &current, &f, Some(&foreign)),
            DescriptionOutcome::Append(_)
        ));
        // Equal html → no row and no touch.
        assert_eq!(
            plan_track_description(&current, &current, &f, Some(&same)),
            DescriptionOutcome::Unchanged
        );
    }

    // FX-ACT-01 · parent.
    #[test]
    fn track_parent_row_invalid_and_equal() {
        let f = frame();
        let resolve = |id: &str| match id {
            x if x == PARENT_OLD => Some(ParentRef {
                id: PARENT_OLD.to_owned(),
                identifier: "PROJ".to_owned(),
                sequence_id: 7,
            }),
            x if x == PARENT_NEW => Some(ParentRef {
                id: PARENT_NEW.to_owned(),
                identifier: "PROJ".to_owned(),
                sequence_id: 9,
            }),
            _ => None,
        };
        let row = plan_track_parent(
            &json!({"parent_id": PARENT_NEW}),
            &json!({"parent_id": PARENT_OLD}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(row.field, "parent");
        assert_eq!(row.comment, "updated the parent issue to");
        assert_eq!(row.old_value.as_deref(), Some("PROJ-7"));
        assert_eq!(row.new_value.as_deref(), Some("PROJ-9"));
        assert_eq!(row.old_identifier.as_deref(), Some(PARENT_OLD));
        assert_eq!(row.new_identifier.as_deref(), Some(PARENT_NEW));
        // Fallback key spelling works on both sides.
        let row = plan_track_parent(
            &json!({"parent": PARENT_NEW}),
            &json!({"parent": PARENT_OLD}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(row.new_value.as_deref(), Some("PROJ-9"));
        // Removing the parent renders "" with a None identifier.
        let row = plan_track_parent(
            &json!({"parent_id": null}),
            &json!({"parent_id": PARENT_OLD}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(row.new_value.as_deref(), Some(""));
        assert_eq!(row.new_identifier, None);
        // An invalid UUID on either side returns silently with no row.
        assert_eq!(
            plan_track_parent(
                &json!({"parent_id": "nope"}),
                &json!({"parent_id": PARENT_OLD}),
                &f,
                &resolve
            ),
            None
        );
        assert_eq!(
            plan_track_parent(
                &json!({"parent_id": PARENT_NEW}),
                &json!({"parent_id": "nope"}),
                &f,
                &resolve
            ),
            None
        );
        // Equal ids → no row.
        assert_eq!(
            plan_track_parent(
                &json!({"parent_id": PARENT_OLD}),
                &json!({"parent_id": PARENT_OLD}),
                &f,
                &resolve
            ),
            None
        );
    }

    // FX-ACT-01 · priority.
    #[test]
    fn track_priority_changed() {
        let f = frame();
        let row = plan_track_priority(
            &json!({"priority": "high"}),
            &json!({"priority": "low"}),
            &f,
        )
        .unwrap();
        assert_eq!(row.field, "priority");
        assert_eq!(row.comment, "updated the priority to");
        assert_eq!(row.old_value.as_deref(), Some("low"));
        assert_eq!(row.new_value.as_deref(), Some("high"));
        assert_eq!(
            plan_track_priority(&json!({"priority": "low"}), &json!({"priority": "low"}), &f),
            None
        );
    }

    // FX-ACT-01 · state.
    #[test]
    fn track_state_row_and_invalid_coercion() {
        let f = frame();
        let resolve = |id: &str| match id {
            x if x == STATE_DONE => Some(StateRef {
                id: STATE_DONE.to_owned(),
                name: "Done".to_owned(),
            }),
            _ => None,
        };
        let new_id = "77777777-7777-4777-8777-777777777777";
        let row = plan_track_state(
            &json!({"state_id": STATE_DONE}),
            &json!({"state_id": new_id}),
            &f,
            &|id: &str| {
                if id == new_id {
                    Some(StateRef {
                        id: new_id.to_owned(),
                        name: "Todo".to_owned(),
                    })
                } else {
                    resolve(id)
                }
            },
        )
        .unwrap();
        assert_eq!(row.field, "state");
        assert_eq!(row.comment, "updated the state to");
        assert_eq!(row.old_value.as_deref(), Some("Todo"));
        assert_eq!(row.new_value.as_deref(), Some("Done"));
        assert_eq!(row.old_identifier.as_deref(), Some(new_id));
        assert_eq!(row.new_identifier.as_deref(), Some(STATE_DONE));
        // Invalid ids coerce to None (no silent return): garbage → valid id
        // still appends, with a None old side.
        let row = plan_track_state(
            &json!({"state_id": STATE_DONE}),
            &json!({"state_id": "garbage"}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(row.old_value, None);
        assert_eq!(row.old_identifier, None);
        assert_eq!(row.new_value.as_deref(), Some("Done"));
        // Garbage on both sides coerces equal → no row.
        assert_eq!(
            plan_track_state(
                &json!({"state_id": "garbage"}),
                &json!({"state_id": "junk"}),
                &f,
                &resolve
            ),
            None
        );
    }

    // FX-ACT-01 · target_date / start_date.
    #[test]
    fn track_dates_none_renders_empty() {
        let f = frame();
        let row = plan_track_target_date(
            &json!({"target_date": "2026-10-01"}),
            &json!({"target_date": null}),
            &f,
        )
        .unwrap();
        assert_eq!(row.field, "target_date");
        assert_eq!(row.comment, "updated the target date to");
        assert_eq!(row.old_value.as_deref(), Some(""));
        assert_eq!(row.new_value.as_deref(), Some("2026-10-01"));
        let row = plan_track_target_date(
            &json!({"target_date": null}),
            &json!({"target_date": "2026-10-01"}),
            &f,
        )
        .unwrap();
        assert_eq!(row.old_value.as_deref(), Some("2026-10-01"));
        assert_eq!(row.new_value.as_deref(), Some(""));
        // The start-date comment carries a trailing space; target has none.
        let row = plan_track_start_date(
            &json!({"start_date": "2026-10-01"}),
            &json!({"start_date": null}),
            &f,
        )
        .unwrap();
        assert_eq!(row.field, "start_date");
        assert_eq!(row.comment, "updated the start date to ");
        assert_eq!(row.old_value.as_deref(), Some(""));
        assert_eq!(row.new_value.as_deref(), Some("2026-10-01"));
        assert_eq!(
            plan_track_target_date(
                &json!({"target_date": "2026-10-01"}),
                &json!({"target_date": "2026-10-01"}),
                &f
            ),
            None
        );
    }

    // FX-ACT-01 · labels.
    #[test]
    fn track_labels_added_dropped_skipped_and_missing() {
        let f = frame();
        let resolve = |id: &str| match id {
            x if x == LABEL_A => Some(LabelRef {
                id: LABEL_A.to_owned(),
                name: "bug".to_owned(),
            }),
            x if x == LABEL_B => Some(LabelRef {
                id: LABEL_B.to_owned(),
                name: "ui".to_owned(),
            }),
            _ => None,
        };
        let rows = plan_track_labels(
            &json!({"label_ids": [LABEL_A]}),
            &json!({"label_ids": [LABEL_B]}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        let added = rows
            .iter()
            .find(|r| r.new_value.as_deref() == Some("bug"))
            .unwrap();
        assert_eq!(added.field, "labels");
        assert_eq!(added.comment, "added label ");
        assert_eq!(added.old_value.as_deref(), Some(""));
        assert_eq!(added.new_identifier.as_deref(), Some(LABEL_A));
        assert_eq!(added.old_identifier, None);
        let dropped = rows
            .iter()
            .find(|r| r.old_value.as_deref() == Some("ui"))
            .unwrap();
        assert_eq!(dropped.comment, "removed label ");
        assert_eq!(dropped.new_value.as_deref(), Some(""));
        assert_eq!(dropped.old_identifier.as_deref(), Some(LABEL_B));
        assert_eq!(dropped.new_identifier, None);
        // Non-UUID ids are skipped silently.
        let rows = plan_track_labels(
            &json!({"label_ids": ["nope", LABEL_A]}),
            &json!({"label_ids": []}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        // A missing Label row raises (DoesNotExist → RowMissing).
        assert_eq!(
            plan_track_labels(
                &json!({"label_ids": ["88888888-8888-4888-8888-888888888888"]}),
                &json!({"label_ids": []}),
                &f,
                &resolve
            ),
            Err(TrackError::RowMissing {
                table: "Label",
                pk: "88888888-8888-4888-8888-888888888888".to_owned(),
            })
        );
    }

    // FX-ACT-01 · assignees.
    #[test]
    fn track_assignees_added_collects_subscriber() {
        let f = frame();
        let resolve = |id: &str| {
            if id == USER_ADA {
                Some(UserRef {
                    id: USER_ADA.to_owned(),
                    display_name: "Ada".to_owned(),
                })
            } else {
                None
            }
        };
        let out = plan_track_assignees(
            &json!({"assignee_ids": [USER_ADA]}),
            &json!({"assignee_ids": []}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(out.activities.len(), 1);
        let row = &out.activities[0];
        assert_eq!(row.field, "assignees");
        assert_eq!(row.comment, "added assignee ");
        assert_eq!(row.verb, "updated");
        assert_eq!(row.old_value.as_deref(), Some(""));
        assert_eq!(row.new_value.as_deref(), Some("Ada"));
        assert_eq!(row.new_identifier.as_deref(), Some(USER_ADA));
        assert_eq!(row.old_identifier, None);
        assert_eq!(out.subscribers.len(), 1);
        let sub = &out.subscribers[0];
        assert_eq!(sub.subscriber_id, USER_ADA);
        assert_eq!(sub.issue_id, "issue-1");
        assert_eq!(sub.workspace_id, "ws-1");
        assert_eq!(sub.project_id, "proj-1");
        assert_eq!(sub.created_by_id, USER_ADA);
        assert_eq!(sub.updated_by_id, USER_ADA);
        // Dropped assignee: no subscriber side effect.
        let out = plan_track_assignees(
            &json!({"assignee_ids": []}),
            &json!({"assignee_ids": [USER_ADA]}),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(out.activities.len(), 1);
        let row = &out.activities[0];
        assert_eq!(row.comment, "removed assignee ");
        assert_eq!(row.old_value.as_deref(), Some("Ada"));
        assert_eq!(row.new_value.as_deref(), Some(""));
        assert_eq!(row.old_identifier.as_deref(), Some(USER_ADA));
        assert!(out.subscribers.is_empty());
    }

    // FX-ACT-01 · estimate_points.
    #[test]
    fn track_estimate_points_updated_removed_and_same() {
        let f = frame();
        let new_id = "99999999-9999-4999-8999-999999999999";
        let old_id = "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa";
        let resolve = |id: &str| match id {
            x if x == new_id => Some(EstimatePointRef {
                value: "3".to_owned(),
                estimate_type: "points".to_owned(),
            }),
            x if x == old_id => Some(EstimatePointRef {
                value: "1".to_owned(),
                estimate_type: "points".to_owned(),
            }),
            _ => None,
        };
        let row = plan_track_estimate_points(
            &json!({"estimate_point": new_id}),
            &json!({"estimate_point": old_id}),
            &f,
            &resolve,
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.field, "estimate_points");
        assert_eq!(row.comment, "updated the estimate point to ");
        assert_eq!(row.verb, "updated");
        assert_eq!(row.old_identifier.as_deref(), Some(old_id));
        assert_eq!(row.new_identifier.as_deref(), Some(new_id));
        assert_eq!(row.old_value.as_deref(), Some("1"));
        assert_eq!(row.new_value.as_deref(), Some("3"));
        // Clearing the estimate hits BUG-ACT-01: no row, EstimateRemovedField.
        assert_eq!(
            plan_track_estimate_points(
                &json!({"estimate_point": null}),
                &json!({"estimate_point": old_id}),
                &f,
                &resolve
            ),
            Err(TrackError::EstimateRemovedField)
        );
        // Equal ids (including None) → no row.
        assert_eq!(
            plan_track_estimate_points(
                &json!({"estimate_point": old_id}),
                &json!({"estimate_point": old_id}),
                &f,
                &resolve
            ),
            Ok(None)
        );
    }

    // FX-ACT-01 · archive_at.
    #[test]
    fn track_archive_at_manual_automation_and_restore() {
        let f = frame();
        let archived = json!({"archived_at": "2026-09-28"});
        let open = json!({"archived_at": null});
        let row = plan_track_archive_at(&archived, &open, &f).unwrap();
        assert_eq!(row.field, "archived_at");
        assert_eq!(row.comment, "Actor has archived the issue");
        assert_eq!(row.new_value.as_deref(), Some("manual_archive"));
        assert_eq!(row.old_value, None);
        let auto = json!({"archived_at": "2026-09-28", "automation": true});
        let row = plan_track_archive_at(&auto, &open, &f).unwrap();
        assert_eq!(row.comment, "Pi Dash has archived the issue");
        assert_eq!(row.new_value.as_deref(), Some("archive"));
        let row = plan_track_archive_at(&open, &archived, &f).unwrap();
        assert_eq!(row.comment, "has restored the issue");
        assert_eq!(row.old_value.as_deref(), Some("archive"));
        assert_eq!(row.new_value.as_deref(), Some("restore"));
        assert_eq!(plan_track_archive_at(&open, &open, &f), None);
    }

    // FX-ACT-01 · closed_to.
    #[test]
    fn track_closed_to_row_absent_and_missing() {
        let f = frame();
        let resolve = |id: &str| {
            if id == STATE_DONE {
                Some(StateRef {
                    id: STATE_DONE.to_owned(),
                    name: "Done".to_owned(),
                })
            } else {
                None
            }
        };
        let row = plan_track_closed_to(&json!({"closed_to": STATE_DONE}), &f, &resolve)
            .unwrap()
            .unwrap();
        assert_eq!(row.field, "state");
        assert_eq!(row.comment, "Pi Dash updated the state to ");
        assert_eq!(row.old_value, None);
        assert_eq!(row.old_identifier, None);
        assert_eq!(row.new_value.as_deref(), Some("Done"));
        assert_eq!(row.new_identifier.as_deref(), Some(STATE_DONE));
        // Absent or None → no activity, no lookup.
        let calls = std::cell::Cell::new(0);
        let counting = |id: &str| {
            calls.set(calls.get() + 1);
            resolve(id)
        };
        assert_eq!(plan_track_closed_to(&json!({}), &f, &counting), Ok(None));
        assert_eq!(
            plan_track_closed_to(&json!({"closed_to": null}), &f, &counting),
            Ok(None)
        );
        assert_eq!(calls.get(), 0);
        // Missing State row raises (DoesNotExist → RowMissing).
        assert_eq!(
            plan_track_closed_to(
                &json!({"closed_to": "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb"}),
                &f,
                &resolve
            ),
            Err(TrackError::RowMissing {
                table: "State",
                pk: "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb".to_owned(),
            })
        );
    }
}
