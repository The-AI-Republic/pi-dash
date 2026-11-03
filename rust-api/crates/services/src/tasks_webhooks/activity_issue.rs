//! D-08 entity activity builders: issue, comment, cycle, module (services layer).
//!
//! Port of `apps/api/pi_dash/bgtasks/issue_activities_task.py:557-927`:
//! `create/update/delete_issue_activity` (`:557-665`,
//! `ISSUE_ACTIVITY_MAPPER` 17 keys `:604-622`), the three comment builders
//! (`:666-754`), `create/delete_cycle_issue_activity` (`:755-860`) and
//! `create/delete_module_issue_activity` (`:861-927`) — one closure on top
//! of the `track_*` helpers from [`super::activity_tracks`] (PIDASHCONV-196).
//!
//! Every builder is pure over injected snapshots: the Django ORM reads the
//! Python performs inline (`Issue.objects.get`, `Cycle.objects.filter`,
//! `Module.objects.filter`, the `Issue` touch `issue.updated_at = now()`)
//! arrive here as pre-resolved arguments — an [`Option`] snapshot for
//! `.filter(...).first()` (missing row = `None`), a resolver closure for
//! `.get(...)` (missing row = [`BuildError::RowMissing`]). The jobs layer
//! (PIDASHCONV-198, `issue_activity` dispatcher) owns the actual queries,
//! the `bulk_create` writes and the `timezone.now()` touches, under the
//! same call order as the Python; per-record touches are reported back as
//! [`CycleOutcome::touched_issue_ids`] (or a single id for the module
//! builders) so the dispatcher touches exactly the rows the Python touches.
//!
//! Parameter mapping (same semantics, Rust shapes): `requested_data` and
//! `current_instance` arrive as the raw JSON *strings* the Celery task
//! receives — each builder runs `json.loads(x) if x is not None else None`
//! itself, so the entry points take `Option<&str>` and parse internally.
//! The five per-row passthroughs travel as [`TrackFrame`] (reused from
//! `activity_tracks`); emitted rows are [`ActivityRow`], which extends the
//! twelve helpers' columns with `issue_comment_id` (comment builders),
//! an optional `field` (`create_issue_activity` omits it) and an optional
//! `issue_id` (cycle records carry their own, possibly absent, id).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG-ACT-03 (`:574`): `create_issue_activity` runs
//!   `requested_data.get("assignee_ids")` on the `None` that `json.loads`
//!   skips, so a `None` requested payload raises `AttributeError`.
//!   [`plan_create_issue`] returns [`BuildError::NullInput`] on that path.
//! * BUG-ACT-04 (`:771`): `create_cycle_issue_activity` defaults a missing
//!   `created_cycle_issues` key to the *list* `[]` and passes it straight
//!   to `json.loads`, raising `TypeError`. Any non-string value (missing
//!   key, `null`, already-decoded list) returns
//!   [`BuildError::NotJsonString`].
//! * BUG-ACT-05 (`:806-817`): the created-cycle loop dereferences
//!   `cycle.name` / `cycle.id` unguarded, so a missing `Cycle` row raises
//!   `AttributeError` — unlike the updated loop just above, which is
//!   `None`-safe through ternaries. [`plan_create_cycle_issue`] returns
//!   [`BuildError::MissingCycle`] on that path instead of a row.
//! * BUG-ACT-06 (`:711`): `update_comment_activity` stores
//!   `new_identifier=current_instance.get("id")`, reusing the *current* id
//!   for the new identifier instead of the requested one. Kept as-is.
//!
//! # Deliberate deviations (documented, not bugs)
//!
//! * `update_issue_activity` iterates `requested_data` in Python insertion
//!   order; `serde_json` here builds `Map` without `preserve_order`
//!   (sorted keys), so multi-key updates emit rows in sorted-key order.
//!   Single-key updates — the contract case — are identical, and tests
//!   compare multi-row output as sets (same convention as FX-ACT-01).
//! * A non-object `requested_data` (a JSON string or list) returns
//!   [`BuildError::NotObject`]: Python would silently skip a string payload
//!   (no mapper key matches one-character keys) or raise `TypeError` on a
//!   list — both collapse into the dispatcher's broad-except swallow, so
//!   the observable outcome is identical.
//! * A non-list `issues` value in `delete_cycle_issue_activity` returns
//!   [`BuildError::NotList`]: Python would iterate a string char by char
//!   and append one garbage row per character. Callers always pass id
//!   lists; the deviation is unreachable in contract.
//!
//! Fixture: `rust-api/fixtures/tasks_webhooks/fx-act-02-activity-builders.json`
//! (FX-ACT-02, issue/comment/cycle/module subset). The `#[cfg(test)]`
//! suite below asserts every golden in that subset.

use serde_json::Value;

use super::activity_tracks::{
    plan_track_archive_at, plan_track_assignees, plan_track_closed_to, plan_track_description,
    plan_track_estimate_points, plan_track_labels, plan_track_name, plan_track_parent,
    plan_track_priority, plan_track_start_date, plan_track_state, plan_track_target_date,
    AssigneeOutcome, DescriptionLatest, DescriptionOutcome, EstimatePointRef, IssueActivityDraft,
    IssueSubscriberDraft, LabelRef, ParentRef, StateRef, TrackError, TrackFrame, UserRef,
};

/// One appended `IssueActivity` row: every column the entity builders write.
///
/// `old_value` / `new_value` are `None` exactly where the Python passes
/// `None` (a NULL column); `""` is a real empty string (cleared cycle /
/// module values, created-cycle `old_value`). `field` is `None` only for
/// the `create_issue_activity` row, which omits the kwarg; `issue_id` is
/// `None` only when a cycle record carries no id; `issue_comment_id` is
/// `None` everywhere except the comment builders.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityRow {
    pub issue_id: Option<String>,
    pub actor_id: String,
    pub verb: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub field: Option<String>,
    pub project_id: String,
    pub workspace_id: String,
    pub comment: String,
    pub old_identifier: Option<String>,
    pub new_identifier: Option<String>,
    pub issue_comment_id: Option<String>,
    pub epoch: f64,
}

/// The `Issue.objects.get(pk=issue_id)` projection `create_issue_activity`
/// reads (`:565-567`): the row's `created_at` override and the actor
/// override (`issue.created_by_id`, not the caller).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatedIssueRef {
    pub created_at: String,
    pub created_by_id: String,
}

/// A resolved `Cycle` row (`create/delete_cycle_issue_activity`): only
/// `id` and `name` are ever read.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleRef {
    pub id: String,
    pub name: String,
}

/// Failure modes that mirror the Python raises (the dispatcher's
/// broad-except swallows them; see FX-ACT-03).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BuildError {
    /// `.get(...)` / iterating on a `None` payload (`AttributeError` /
    /// `TypeError`): `create_issue_activity` with `None` requested
    /// (BUG-ACT-03), `update_issue_activity` iterating `None`,
    /// `update_comment_activity` reading a `None` side, comment builders
    /// with `None` requested, cycle builders with a `None` current
    /// instance, `delete_module_issue_activity` with either side `None`.
    #[error("builder input '{part}' is null")]
    NullInput { part: &'static str },
    /// `json.loads` on an invalid JSON string.
    #[error("builder input '{part}' is not valid JSON")]
    JsonParse { part: &'static str },
    /// A decoded `requested_data` that is not an object (see the module
    /// docs for why strings/lists collapse here).
    #[error("builder input '{part}' is not an object")]
    NotObject { part: &'static str },
    /// Iterating a present-but-`None` (or otherwise non-list) value:
    /// `updated_cycle_issues` (`:768`), `issues` in the cycle delete
    /// (`:836`).
    #[error("value under key '{key}' is not a list")]
    NotList { key: &'static str },
    /// BUG-ACT-04: `created_cycle_issues` is not a JSON string (missing
    /// key defaults to the list `[]`, `null`, or an already-decoded
    /// value) — `json.loads` would raise `TypeError` (`:771`).
    #[error("double-encoded list under key '{key}' is not a JSON string")]
    NotJsonString { key: &'static str },
    /// `.get(...)` on a missing intermediate dict: a cycle created record
    /// without `fields` (`:804-805`).
    #[error("record is missing '{path}'")]
    MissingField { path: &'static str },
    /// `.get(pk=...)` with no matching row (`Issue.objects.get`, `:565`).
    #[error("row lookup missed: {table} pk={pk}")]
    RowMissing { table: &'static str, pk: String },
    /// BUG-ACT-05: the created-cycle loop dereferences `cycle.name` on a
    /// missing `Cycle` row (`AttributeError`, `:806-817`).
    #[error("cycle row missing for created record pk={pk}")]
    MissingCycle { pk: String },
    /// `create_issue_activity` received a non-empty, non-object current
    /// instance: the Python hands it raw to `track_assignees`, whose
    /// `extract_ids` raises `AttributeError` on a string (`:574-583`).
    #[error("create current instance is not decoded")]
    CurrentNotDecoded,
    /// A `track_*` helper raised (`DoesNotExist` / `TypeError` /
    /// BUG-ACT-01); propagates out of `update_issue_activity` and the
    /// create-time assignee call exactly as in Python.
    #[error(transparent)]
    Track(#[from] TrackError),
}

/// Python `str()` over a JSON scalar: `None` → `"None"`, `True`/`False`,
/// ints plain, floats `1.0`-style. Container input is out of contract and
/// renders as compact JSON. (Mirrors `activity_tracks`; kept local because
/// the helper there is private.)
fn py_str(value: &Value) -> String {
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

/// `Some(string)` for any present non-`None` value, `None` for a missing
/// key or JSON `null` — the `.get()` projection most builders store.
fn opt_str(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => Some(py_str(v)),
    }
}

/// `json.loads(raw) if raw is not None else None` at each builder entry.
fn parse_opt(raw: Option<&str>, part: &'static str) -> Result<Option<Value>, BuildError> {
    match raw {
        None => Ok(None),
        Some(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|_| BuildError::JsonParse { part }),
    }
}

/// Fill the columns shared by every row this module emits.
#[allow(clippy::too_many_arguments)]
fn row(
    frame: &TrackFrame,
    issue_id: Option<String>,
    verb: &str,
    field: Option<&str>,
    comment: String,
    old_value: Option<String>,
    new_value: Option<String>,
) -> ActivityRow {
    ActivityRow {
        issue_id,
        actor_id: frame.actor_id.clone(),
        verb: verb.to_owned(),
        old_value,
        new_value,
        field: field.map(str::to_owned),
        project_id: frame.project_id.clone(),
        workspace_id: frame.workspace_id.clone(),
        comment,
        old_identifier: None,
        new_identifier: None,
        issue_comment_id: None,
        epoch: frame.epoch,
    }
}

/// `create_issue_activity` (`:557-583`): one `created` row whose actor and
/// `created_at` come from the `Issue` row itself (`:565-567`), then the
/// assignee trackers run iff `requested.assignee_ids is not None` (`:574`).
/// `current_instance` travels raw into `track_assignees`: `None` (or `{}`)
/// means no previous assignees, a decoded object diffs normally, anything
/// else raises like the Python's `AttributeError`.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateIssueOutcome {
    pub row: ActivityRow,
    /// `= issue.created_at`, written back with
    /// `save(update_fields=["created_at", "actor_id"])` (`:566-567`); the
    /// jobs layer performs the write.
    pub created_at_override: String,
    pub assignees: Option<AssigneeOutcome>,
}

pub fn plan_create_issue(
    requested_raw: Option<&str>,
    current: Option<&Value>,
    issue: Option<CreatedIssueRef>,
    frame: &TrackFrame,
    resolve_user: &dyn Fn(&str) -> Option<UserRef>,
) -> Result<CreateIssueOutcome, BuildError> {
    let issue = issue.ok_or_else(|| BuildError::RowMissing {
        table: "Issue",
        pk: frame.issue_id.clone(),
    })?;
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        None,
        "created the issue".to_owned(),
        None,
        None,
    );
    out.actor_id = issue.created_by_id.clone();
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let assignees = if requested.get("assignee_ids").is_some_and(|v| !v.is_null()) {
        let empty = Value::Object(Default::default());
        let cur = current.unwrap_or(&empty);
        if !cur.is_object() {
            return Err(BuildError::CurrentNotDecoded);
        }
        Some(plan_track_assignees(&requested, cur, frame, resolve_user)?)
    } else {
        None
    };
    Ok(CreateIssueOutcome {
        row: out,
        created_at_override: issue.created_at,
        assignees,
    })
}

/// The resolver bundle `update_issue_activity` threads into the twelve
/// `track_*` helpers; each closure owns its query scoping (project-scoped
/// state lookup, latest-`IssueActivity` projection, …) exactly as the
/// Python's inline ORM reads do.
pub struct UpdateResolvers<'a> {
    pub parent: &'a dyn Fn(&str) -> Option<ParentRef>,
    pub state: &'a dyn Fn(&str) -> Option<StateRef>,
    pub label: &'a dyn Fn(&str) -> Option<LabelRef>,
    pub user: &'a dyn Fn(&str) -> Option<UserRef>,
    pub estimate: &'a dyn Fn(&str) -> Option<EstimatePointRef>,
    pub description_latest: Option<DescriptionLatest>,
}

/// `update_issue_activity` (`:585-646`): every key of the parsed requested
/// dict dispatches through `ISSUE_ACTIVITY_MAPPER` (17 keys → 12 helpers);
/// unknown keys are skipped silently. A raising helper aborts the loop and
/// propagates to the dispatcher, like the Python.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateOutcome {
    pub rows: Vec<IssueActivityDraft>,
    pub subscribers: Vec<IssueSubscriberDraft>,
    pub description_touched: bool,
}

pub fn plan_update_issue(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
    resolvers: &UpdateResolvers<'_>,
) -> Result<UpdateOutcome, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let current = parse_opt(current_raw, "current_instance")?;
    let keys = requested.as_object().ok_or(BuildError::NotObject {
        part: "requested_data",
    })?;
    let mut out = UpdateOutcome {
        rows: Vec::new(),
        subscribers: Vec::new(),
        description_touched: false,
    };
    for key in keys.keys() {
        // `track_closed_to` never reads `current_instance`, and unknown
        // keys are skipped before any helper runs — both stay silent on a
        // `None` current, exactly as in Python.
        if key.as_str() == "closed_to" {
            if let Some(row) = plan_track_closed_to(&requested, frame, resolvers.state)? {
                out.rows.push(row);
            }
            continue;
        }
        let mapped = matches!(
            key.as_str(),
            "name"
                | "parent_id"
                | "parent"
                | "priority"
                | "state_id"
                | "state"
                | "description_html"
                | "target_date"
                | "start_date"
                | "label_ids"
                | "labels"
                | "assignee_ids"
                | "assignees"
                | "estimate_point"
                | "archived_at"
        );
        if !mapped {
            continue;
        }
        let current = current.as_ref().ok_or(BuildError::NullInput {
            part: "current_instance",
        })?;
        match key.as_str() {
            "name" => {
                if let Some(row) = plan_track_name(&requested, current, frame) {
                    out.rows.push(row);
                }
            }
            "parent_id" | "parent" => {
                if let Some(row) = plan_track_parent(&requested, current, frame, resolvers.parent) {
                    out.rows.push(row);
                }
            }
            "priority" => {
                if let Some(row) = plan_track_priority(&requested, current, frame) {
                    out.rows.push(row);
                }
            }
            "state_id" | "state" => {
                if let Some(row) = plan_track_state(&requested, current, frame, resolvers.state) {
                    out.rows.push(row);
                }
            }
            "description_html" => match plan_track_description(
                &requested,
                current,
                frame,
                resolvers.description_latest.as_ref(),
            ) {
                DescriptionOutcome::Unchanged => {}
                DescriptionOutcome::TouchLatest => out.description_touched = true,
                DescriptionOutcome::Append(row) => out.rows.push(*row),
            },
            "target_date" => {
                if let Some(row) = plan_track_target_date(&requested, current, frame) {
                    out.rows.push(row);
                }
            }
            "start_date" => {
                if let Some(row) = plan_track_start_date(&requested, current, frame) {
                    out.rows.push(row);
                }
            }
            "label_ids" | "labels" => {
                out.rows.extend(plan_track_labels(
                    &requested,
                    current,
                    frame,
                    resolvers.label,
                )?);
            }
            "assignee_ids" | "assignees" => {
                let outcome = plan_track_assignees(&requested, current, frame, resolvers.user)?;
                out.rows.extend(outcome.activities);
                out.subscribers.extend(outcome.subscribers);
            }
            "estimate_point" => {
                if let Some(row) =
                    plan_track_estimate_points(&requested, current, frame, resolvers.estimate)?
                {
                    out.rows.push(row);
                }
            }
            "archived_at" => {
                if let Some(row) = plan_track_archive_at(&requested, current, frame) {
                    out.rows.push(row);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// `delete_issue_activity` (`:648-665`): a single `deleted` row on
/// `field="issue"`; both payloads ignored.
pub fn plan_delete_issue(frame: &TrackFrame) -> ActivityRow {
    row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("issue"),
        "deleted the issue".to_owned(),
        None,
        None,
    )
}

/// `create_comment_activity` (`:667-691`): one `created` row carrying the
/// requested `comment_html` (default `""`) with both identifiers pointing
/// at the requested id; `current_instance` ignored.
pub fn plan_create_comment(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("comment"),
        "created a comment".to_owned(),
        None,
        Some(
            requested
                .get("comment_html")
                .map(py_str)
                .unwrap_or_default(),
        ),
    );
    out.new_identifier = opt_str(requested.get("id"));
    out.issue_comment_id = opt_str(requested.get("id"));
    Ok(out)
}

/// `update_comment_activity` (`:694-723`): one `updated` row when the html
/// differs, none when equal. BUG-ACT-06: `new_identifier` reuses the
/// *current* id; `issue_comment_id` is the current id too.
pub fn plan_update_comment(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let current = parse_opt(current_raw, "current_instance")?.ok_or(BuildError::NullInput {
        part: "current_instance",
    })?;
    if current.get("comment_html") == requested.get("comment_html") {
        return Ok(None);
    }
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "updated",
        Some("comment"),
        "updated a comment".to_owned(),
        Some(current.get("comment_html").map(py_str).unwrap_or_default()),
        Some(
            requested
                .get("comment_html")
                .map(py_str)
                .unwrap_or_default(),
        ),
    );
    out.old_identifier = opt_str(current.get("id"));
    out.new_identifier = opt_str(current.get("id"));
    out.issue_comment_id = opt_str(current.get("id"));
    Ok(Some(out))
}

/// `delete_comment_activity` (`:726-754`): one `deleted` row whose only
/// link is `issue_comment_id = requested.comment_id`; `current_instance`
/// ignored (not even parsed).
pub fn plan_delete_comment(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("comment"),
        "deleted the comment".to_owned(),
        None,
        None,
    );
    out.issue_comment_id = opt_str(requested.get("comment_id"));
    Ok(out)
}

/// Outcome of the cycle builders: appended rows plus the issue ids whose
/// `updated_at` the jobs layer touches (`issue.updated_at = now();
/// save(update_fields=["updated_at"])`, guarded by `if issue` — one entry
/// per record whose row resolved).
#[derive(Debug, Clone, PartialEq)]
pub struct CycleOutcome {
    pub rows: Vec<ActivityRow>,
    pub touched_issue_ids: Vec<String>,
}

/// The updated-cycle comment template (`:790-791`): a literal newline then
/// sixteen spaces before `to` — ported byte for byte.
fn updated_cycle_comment(old_name: &str, new_name: &str) -> String {
    format!("updated cycle from {old_name}\n                to {new_name}")
}

/// `create_cycle_issue_activity` (`:755-819`) — despite the name, both the
/// updated loop (`updated_cycle_issues` is a plain list) and the created
/// loop (`created_cycle_issues` is a double-encoded JSON string,
/// BUG-ACT-04). Updated lookups are `None`-safe; created lookups are
/// unguarded (BUG-ACT-05).
pub fn plan_create_cycle_issue(
    current_raw: Option<&str>,
    frame: &TrackFrame,
    resolve_cycle: &dyn Fn(&str) -> Option<CycleRef>,
    issue_exists: &dyn Fn(&str) -> bool,
) -> Result<CycleOutcome, BuildError> {
    let current = parse_opt(current_raw, "current_instance")?.ok_or(BuildError::NullInput {
        part: "current_instance",
    })?;
    let mut out = CycleOutcome {
        rows: Vec::new(),
        touched_issue_ids: Vec::new(),
    };
    let empty_updated = Value::Array(Vec::new());
    let updated = match current.get("updated_cycle_issues") {
        None => &empty_updated,
        Some(v) => v,
    };
    let updated = updated.as_array().ok_or(BuildError::NotList {
        key: "updated_cycle_issues",
    })?;
    for record in updated {
        let old_id = opt_str(record.get("old_cycle_id"));
        let new_id = opt_str(record.get("new_cycle_id"));
        let issue_id = opt_str(record.get("issue_id"));
        let old = old_id.as_deref().and_then(resolve_cycle);
        let new = new_id.as_deref().and_then(resolve_cycle);
        let old_name = old.as_ref().map(|c| c.name.as_str()).unwrap_or("");
        let new_name = new.as_ref().map(|c| c.name.as_str()).unwrap_or("");
        if let Some(id) = issue_id.clone() {
            if issue_exists(&id) {
                out.touched_issue_ids.push(id.clone());
            }
        }
        let mut activity = row(
            frame,
            issue_id,
            "updated",
            Some("cycles"),
            updated_cycle_comment(old_name, new_name),
            Some(old_name.to_owned()),
            Some(new_name.to_owned()),
        );
        activity.old_identifier = old.map(|c| c.id);
        activity.new_identifier = new.map(|c| c.id);
        out.rows.push(activity);
    }
    let created_raw = match current.get("created_cycle_issues") {
        Some(Value::String(s)) => s.clone(),
        _ => {
            return Err(BuildError::NotJsonString {
                key: "created_cycle_issues",
            })
        }
    };
    let created: Value = serde_json::from_str(&created_raw).map_err(|_| BuildError::JsonParse {
        part: "created_cycle_issues",
    })?;
    let created = created.as_array().ok_or(BuildError::NotList {
        key: "created_cycle_issues",
    })?;
    for record in created {
        let fields = record
            .get("fields")
            .and_then(Value::as_object)
            .ok_or(BuildError::MissingField { path: "fields" })?;
        let cycle_pk = opt_str(fields.get("cycle"));
        let issue_pk = opt_str(fields.get("issue"));
        let cycle = cycle_pk.as_deref().and_then(resolve_cycle).ok_or_else(|| {
            BuildError::MissingCycle {
                pk: cycle_pk.clone().unwrap_or_default(),
            }
        })?;
        if let Some(id) = issue_pk.clone() {
            if issue_exists(&id) {
                out.touched_issue_ids.push(id.clone());
            }
        }
        let mut activity = row(
            frame,
            issue_pk,
            "created",
            Some("cycles"),
            format!("added cycle {}", cycle.name),
            Some(String::new()),
            Some(cycle.name.clone()),
        );
        activity.new_identifier = Some(cycle.id.clone());
        out.rows.push(activity);
    }
    Ok(out)
}

/// `delete_cycle_issue_activity` (`:821-860`): one `deleted` row per id in
/// `requested.issues`; the cycle name falls back to the requested
/// `cycle_name` when the row is gone; a missing `cycle_id` key reads as
/// `""` (while an explicit `null` yields a `None` identifier).
pub fn plan_delete_cycle_issue(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
    resolve_cycle: &dyn Fn(&str) -> Option<CycleRef>,
    issue_exists: &dyn Fn(&str) -> bool,
) -> Result<CycleOutcome, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let cycle_id = match requested.get("cycle_id") {
        None => Some(String::new()),
        Some(Value::Null) => None,
        Some(v) => Some(py_str(v)),
    };
    let cycle = cycle_id.as_deref().and_then(resolve_cycle);
    let cycle_name = match requested.get("cycle_name") {
        None => String::new(),
        Some(v) => py_str(v),
    };
    let name = cycle.as_ref().map(|c| c.name.clone()).unwrap_or(cycle_name);
    let issues = requested
        .get("issues")
        .and_then(Value::as_array)
        .ok_or(BuildError::NotList { key: "issues" })?;
    let mut out = CycleOutcome {
        rows: Vec::new(),
        touched_issue_ids: Vec::new(),
    };
    for issue in issues {
        let id = py_str(issue);
        if issue_exists(&id) {
            out.touched_issue_ids.push(id.clone());
        }
        let mut activity = row(
            frame,
            Some(id),
            "deleted",
            Some("cycles"),
            format!("removed this issue from {name}"),
            Some(name.clone()),
            Some(String::new()),
        );
        activity.old_identifier = cycle_id.clone();
        out.rows.push(activity);
    }
    Ok(out)
}

/// `create_module_issue_activity` (`:862-893`): one `created` row; the
/// module lookup is `None`-safe (`filter(...).first()`), the identifier
/// echoes the raw requested id, and the touched issue is the activity's
/// own `issue_id`.
pub fn plan_create_module_issue(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
    resolve_module_name: &dyn Fn(&str) -> Option<String>,
    issue_exists: &dyn Fn(&str) -> bool,
) -> Result<(ActivityRow, Vec<String>), BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let module_id = opt_str(requested.get("module_id"));
    let name = module_id
        .as_deref()
        .and_then(resolve_module_name)
        .unwrap_or_default();
    let touched = if issue_exists(&frame.issue_id) {
        vec![frame.issue_id.clone()]
    } else {
        Vec::new()
    };
    let mut activity = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("modules"),
        format!("added module {name}"),
        Some(String::new()),
        Some(name),
    );
    activity.new_identifier = module_id;
    Ok((activity, touched))
}

/// `delete_module_issue_activity` (`:896-927`): one `deleted` row reading
/// the name from `current_instance.module_name` (no DB lookup); a `None`
/// name interpolates as `"None"` in the comment via the f-string while the
/// column stays NULL.
pub fn plan_delete_module_issue(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
    issue_exists: &dyn Fn(&str) -> bool,
) -> Result<(ActivityRow, Vec<String>), BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?.ok_or(BuildError::NullInput {
        part: "requested_data",
    })?;
    let current = parse_opt(current_raw, "current_instance")?.ok_or(BuildError::NullInput {
        part: "current_instance",
    })?;
    let module_name = opt_str(current.get("module_name"));
    let touched = if issue_exists(&frame.issue_id) {
        vec![frame.issue_id.clone()]
    } else {
        Vec::new()
    };
    let mut activity = row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("modules"),
        format!(
            "removed this issue from {}",
            module_name.clone().unwrap_or_else(|| "None".to_owned())
        ),
        module_name,
        Some(String::new()),
    );
    activity.old_identifier = opt_str(requested.get("module_id"));
    Ok((activity, touched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Version-4 UUIDs (valid per `is_valid_uuid`).
    const USER_ADA: &str = "44444444-4444-4444-8444-444444444444";
    const CYCLE_OLD: &str = "77777777-7777-4777-8777-777777777777";
    const CYCLE_NEW: &str = "88888888-8888-4888-8888-888888888888";
    const MODULE_M: &str = "99999999-9999-4999-8999-999999999999";

    fn frame() -> TrackFrame {
        TrackFrame {
            issue_id: "issue-1".to_owned(),
            project_id: "proj-1".to_owned(),
            workspace_id: "ws-1".to_owned(),
            actor_id: "actor-1".to_owned(),
            epoch: 1750000000.5,
        }
    }

    fn created_issue() -> CreatedIssueRef {
        CreatedIssueRef {
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            created_by_id: "creator-9".to_owned(),
        }
    }

    // FX-ACT-02 · issue create.
    #[test]
    fn create_issue_row_overrides_actor_and_created_at() {
        let f = frame();
        let no_user = |_: &str| None;
        let outcome = plan_create_issue(
            Some(r#"{"name": "N", "priority": "high"}"#),
            None,
            Some(created_issue()),
            &f,
            &no_user,
        )
        .unwrap();
        assert_eq!(outcome.row.comment, "created the issue");
        assert_eq!(outcome.row.verb, "created");
        // Actor comes from the Issue row, not the caller.
        assert_eq!(outcome.row.actor_id, "creator-9");
        assert_eq!(outcome.created_at_override, "2026-09-01T00:00:00Z");
        assert_eq!(outcome.row.field, None);
        assert_eq!(outcome.row.issue_id.as_deref(), Some("issue-1"));
        // No assignee_ids key → trackers do not run.
        assert_eq!(outcome.assignees, None);
    }

    #[test]
    fn create_issue_runs_assignees_only_on_non_null_key() {
        let f = frame();
        let resolve = |id: &str| {
            (id == USER_ADA).then(|| UserRef {
                id: USER_ADA.to_owned(),
                display_name: "Ada".to_owned(),
            })
        };
        // Present list → added row + subscriber collected.
        let outcome = plan_create_issue(
            Some(&format!(r#"{{"assignee_ids": ["{USER_ADA}"]}}"#)),
            None,
            Some(created_issue()),
            &f,
            &resolve,
        )
        .unwrap();
        let assignees = outcome.assignees.unwrap();
        assert_eq!(assignees.activities.len(), 1);
        assert_eq!(assignees.activities[0].comment, "added assignee ");
        assert_eq!(assignees.subscribers.len(), 1);
        assert_eq!(assignees.subscribers[0].subscriber_id, USER_ADA);
        // Explicit null → trackers skipped (BUG-ACT-03's guard reads
        // `is not None`, not key presence).
        let outcome = plan_create_issue(
            Some(r#"{"assignee_ids": null}"#),
            None,
            Some(created_issue()),
            &f,
            &resolve,
        )
        .unwrap();
        assert_eq!(outcome.assignees, None);
    }

    #[test]
    fn create_issue_null_requested_and_missing_issue() {
        let f = frame();
        let no_user = |_: &str| None;
        // BUG-ACT-03: `.get` on the None payload raises.
        assert_eq!(
            plan_create_issue(None, None, Some(created_issue()), &f, &no_user).unwrap_err(),
            BuildError::NullInput {
                part: "requested_data"
            }
        );
        // `Issue.objects.get` with no row raises DoesNotExist.
        assert_eq!(
            plan_create_issue(Some("{}"), None, None, &f, &no_user).unwrap_err(),
            BuildError::RowMissing {
                table: "Issue",
                pk: "issue-1".to_owned(),
            }
        );
    }

    const STATE_DONE: &str = "33333333-3333-4333-8333-333333333333";

    // FX-ACT-02 · issue update.
    #[test]
    fn update_issue_dispatches_mapper_and_skips_unknown() {
        let f = frame();
        let parent = |_: &str| None;
        let state = |_: &str| None;
        let label = |_: &str| None;
        let user = |_: &str| None;
        let estimate = |_: &str| None;
        let r = UpdateResolvers {
            parent: &parent,
            state: &state,
            label: &label,
            user: &user,
            estimate: &estimate,
            description_latest: None,
        };
        let outcome = plan_update_issue(
            Some(r#"{"name": "New", "bogus_key": 1}"#),
            Some(r#"{"name": "Old"}"#),
            &f,
            &r,
        )
        .unwrap();
        assert_eq!(outcome.rows.len(), 1);
        assert_eq!(outcome.rows[0].field, "name");
        assert_eq!(outcome.rows[0].verb, "updated");
        assert_eq!(outcome.rows[0].old_value.as_deref(), Some("Old"));
        assert_eq!(outcome.rows[0].new_value.as_deref(), Some("New"));
        // Unknown-only requested with a None current stays silent.
        let outcome = plan_update_issue(Some(r#"{"bogus_key": 1}"#), None, &f, &r).unwrap();
        assert!(outcome.rows.is_empty());
        // A mapped key with a None current raises.
        assert_eq!(
            plan_update_issue(Some(r#"{"name": "New"}"#), None, &f, &r).unwrap_err(),
            BuildError::NullInput {
                part: "current_instance"
            }
        );
        // Null requested raises (iterating None).
        assert_eq!(
            plan_update_issue(None, Some("{}"), &f, &r).unwrap_err(),
            BuildError::NullInput {
                part: "requested_data"
            }
        );
    }

    #[test]
    fn update_issue_external_keys_and_description_touch() {
        let f = frame();
        let parent = |_: &str| None;
        let label = |_: &str| None;
        let user = |_: &str| None;
        let estimate = |_: &str| None;
        let state = |id: &str| {
            (id == STATE_DONE).then(|| StateRef {
                id: STATE_DONE.to_owned(),
                name: "Done".to_owned(),
            })
        };
        // External `state` key dispatches to track_state; the description
        // touch path sets the flag instead of appending.
        let r = UpdateResolvers {
            parent: &parent,
            state: &state,
            label: &label,
            user: &user,
            estimate: &estimate,
            description_latest: Some(DescriptionLatest {
                field: "description".to_owned(),
                actor_id: "actor-1".to_owned(),
            }),
        };
        let outcome = plan_update_issue(
            Some(&format!(
                r#"{{"state": "{STATE_DONE}", "description_html": "<p>x</p>"}}"#
            )),
            Some(r#"{"state": null, "description_html": "<p>old</p>"}"#),
            &f,
            &r,
        )
        .unwrap();
        assert!(outcome.description_touched);
        assert_eq!(
            outcome
                .rows
                .iter()
                .map(|row| row.field.as_str())
                .collect::<Vec<_>>(),
            ["state"]
        );
        // `closed_to` never reads the current instance.
        let outcome = plan_update_issue(
            Some(&format!(r#"{{"closed_to": "{STATE_DONE}"}}"#)),
            None,
            &f,
            &r,
        )
        .unwrap();
        assert_eq!(outcome.rows.len(), 1);
        assert_eq!(outcome.rows[0].field, "state");
    }

    // FX-ACT-02 · issue delete.
    #[test]
    fn delete_issue_row() {
        let row = plan_delete_issue(&frame());
        assert_eq!(row.comment, "deleted the issue");
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.field.as_deref(), Some("issue"));
        assert_eq!(row.issue_id.as_deref(), Some("issue-1"));
    }

    // FX-ACT-02 · comment builders.
    #[test]
    fn comment_create_row() {
        let row = plan_create_comment(
            Some(r#"{"id": "c-1", "comment_html": "<p>hi</p>"}"#),
            &frame(),
        )
        .unwrap();
        assert_eq!(row.comment, "created a comment");
        assert_eq!(row.field.as_deref(), Some("comment"));
        assert_eq!(row.new_value.as_deref(), Some("<p>hi</p>"));
        assert_eq!(row.new_identifier.as_deref(), Some("c-1"));
        assert_eq!(row.issue_comment_id.as_deref(), Some("c-1"));
        // Missing html defaults to "".
        let row = plan_create_comment(Some(r#"{"id": "c-2"}"#), &frame()).unwrap();
        assert_eq!(row.new_value.as_deref(), Some(""));
        // Null requested raises.
        assert_eq!(
            plan_create_comment(None, &frame()).unwrap_err(),
            BuildError::NullInput {
                part: "requested_data"
            }
        );
    }

    #[test]
    fn comment_update_row_and_noop() {
        // BUG-ACT-06: new_identifier reuses the CURRENT id.
        let row = plan_update_comment(
            Some(r#"{"id": "c-new", "comment_html": "<p>new</p>"}"#),
            Some(r#"{"id": "c-1", "comment_html": "<p>old</p>"}"#),
            &frame(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.comment, "updated a comment");
        assert_eq!(row.verb, "updated");
        assert_eq!(row.old_value.as_deref(), Some("<p>old</p>"));
        assert_eq!(row.new_value.as_deref(), Some("<p>new</p>"));
        assert_eq!(row.old_identifier.as_deref(), Some("c-1"));
        assert_eq!(row.new_identifier.as_deref(), Some("c-1"));
        assert_eq!(row.issue_comment_id.as_deref(), Some("c-1"));
        // Equal html → no row.
        assert_eq!(
            plan_update_comment(
                Some(r#"{"comment_html": "<p>same</p>"}"#),
                Some(r#"{"comment_html": "<p>same</p>"}"#),
                &frame(),
            )
            .unwrap(),
            None
        );
        // Either side None raises.
        assert_eq!(
            plan_update_comment(Some("{}"), None, &frame()).unwrap_err(),
            BuildError::NullInput {
                part: "current_instance"
            }
        );
    }

    #[test]
    fn comment_delete_row_links_comment_id_only() {
        let row = plan_delete_comment(Some(r#"{"comment_id": "c-9"}"#), &frame()).unwrap();
        assert_eq!(row.comment, "deleted the comment");
        assert_eq!(row.field.as_deref(), Some("comment"));
        assert_eq!(row.issue_comment_id.as_deref(), Some("c-9"));
        assert_eq!(row.old_value, None);
        assert_eq!(row.new_value, None);
    }

    // FX-ACT-02 · cycle builders.
    fn cycle_lookup(id: &str) -> Option<CycleRef> {
        match id {
            CYCLE_OLD => Some(CycleRef {
                id: CYCLE_OLD.to_owned(),
                name: "Sprint 1".to_owned(),
            }),
            CYCLE_NEW => Some(CycleRef {
                id: CYCLE_NEW.to_owned(),
                name: "Sprint 2".to_owned(),
            }),
            _ => None,
        }
    }

    #[test]
    fn cycle_create_updated_loop_byte_exact_comment() {
        let current = json!({
            "updated_cycle_issues": [
                {"old_cycle_id": CYCLE_OLD, "new_cycle_id": CYCLE_NEW, "issue_id": "issue-7"},
                {"old_cycle_id": null, "new_cycle_id": null, "issue_id": "issue-8"},
            ],
            "created_cycle_issues": "[]",
        });
        let outcome =
            plan_create_cycle_issue(Some(&current.to_string()), &frame(), &cycle_lookup, &|id| {
                id == "issue-7" || id == "issue-8"
            })
            .unwrap();
        assert_eq!(outcome.rows.len(), 2);
        let first = &outcome.rows[0];
        assert_eq!(first.verb, "updated");
        assert_eq!(first.field.as_deref(), Some("cycles"));
        // Literal newline + sixteen spaces before `to`, byte for byte.
        assert_eq!(
            first.comment,
            "updated cycle from Sprint 1\n                to Sprint 2"
        );
        assert_eq!(first.old_value.as_deref(), Some("Sprint 1"));
        assert_eq!(first.new_value.as_deref(), Some("Sprint 2"));
        assert_eq!(first.old_identifier.as_deref(), Some(CYCLE_OLD));
        assert_eq!(first.new_identifier.as_deref(), Some(CYCLE_NEW));
        assert_eq!(first.issue_id.as_deref(), Some("issue-7"));
        // Missing cycles degrade to "" with no identifiers.
        let second = &outcome.rows[1];
        assert_eq!(second.comment, "updated cycle from \n                to ");
        assert_eq!(second.old_identifier, None);
        assert_eq!(second.new_identifier, None);
        assert_eq!(
            outcome.touched_issue_ids,
            vec!["issue-7".to_owned(), "issue-8".to_owned()]
        );
    }

    #[test]
    fn cycle_create_created_loop_double_encoded() {
        let inner = json!([
            {"fields": {"cycle": CYCLE_NEW, "issue": "issue-3"}},
        ])
        .to_string();
        let current = json!({
            "updated_cycle_issues": [],
            "created_cycle_issues": inner,
        });
        let outcome =
            plan_create_cycle_issue(Some(&current.to_string()), &frame(), &cycle_lookup, &|id| {
                id == "issue-3"
            })
            .unwrap();
        assert_eq!(outcome.rows.len(), 1);
        let row = &outcome.rows[0];
        assert_eq!(row.verb, "created");
        assert_eq!(row.comment, "added cycle Sprint 2");
        assert_eq!(row.old_value.as_deref(), Some(""));
        assert_eq!(row.new_value.as_deref(), Some("Sprint 2"));
        assert_eq!(row.new_identifier.as_deref(), Some(CYCLE_NEW));
        assert_eq!(row.issue_id.as_deref(), Some("issue-3"));
        assert_eq!(outcome.touched_issue_ids, vec!["issue-3".to_owned()]);
        // BUG-ACT-04: a missing key defaults to the list [] → TypeError.
        let missing = json!({"updated_cycle_issues": []});
        assert_eq!(
            plan_create_cycle_issue(Some(&missing.to_string()), &frame(), &cycle_lookup, &|_| {
                true
            })
            .unwrap_err(),
            BuildError::NotJsonString {
                key: "created_cycle_issues"
            }
        );
        // BUG-ACT-05: unguarded `cycle.name` on a missing row.
        let bad = json!({
            "updated_cycle_issues": [],
            "created_cycle_issues": json!([{"fields": {"cycle": "nope", "issue": "issue-3"}}]).to_string(),
        });
        assert_eq!(
            plan_create_cycle_issue(Some(&bad.to_string()), &frame(), &cycle_lookup, &|_| true)
                .unwrap_err(),
            BuildError::MissingCycle {
                pk: "nope".to_owned()
            }
        );
    }

    #[test]
    fn cycle_delete_rows_per_issue_with_fallback() {
        let outcome = plan_delete_cycle_issue(
            Some(
                &json!({
                    "cycle_id": CYCLE_OLD,
                    "cycle_name": "Stale",
                    "issues": ["issue-1", "issue-2"],
                })
                .to_string(),
            ),
            &frame(),
            &cycle_lookup,
            &|id| id == "issue-1",
        )
        .unwrap();
        assert_eq!(outcome.rows.len(), 2);
        assert_eq!(outcome.rows[0].comment, "removed this issue from Sprint 1");
        assert_eq!(outcome.rows[0].old_value.as_deref(), Some("Sprint 1"));
        assert_eq!(outcome.rows[0].new_value.as_deref(), Some(""));
        assert_eq!(outcome.rows[0].old_identifier.as_deref(), Some(CYCLE_OLD));
        // Only the resolved issue is touched.
        assert_eq!(outcome.touched_issue_ids, vec!["issue-1".to_owned()]);
        // Gone row → requested cycle_name fallback.
        let outcome = plan_delete_cycle_issue(
            Some(
                &json!({"cycle_id": "gone", "cycle_name": "Stale", "issues": ["issue-1"]})
                    .to_string(),
            ),
            &frame(),
            &cycle_lookup,
            &|_| false,
        )
        .unwrap();
        assert_eq!(outcome.rows[0].comment, "removed this issue from Stale");
        assert!(outcome.touched_issue_ids.is_empty());
        // Missing cycle_id key reads as "" (identifier present-but-empty).
        let outcome = plan_delete_cycle_issue(
            Some(&json!({"cycle_name": "Stale", "issues": ["issue-1"]}).to_string()),
            &frame(),
            &cycle_lookup,
            &|_| true,
        )
        .unwrap();
        assert_eq!(outcome.rows[0].old_identifier.as_deref(), Some(""));
        // Explicit null → None identifier.
        let outcome = plan_delete_cycle_issue(
            Some(&json!({"cycle_id": null, "cycle_name": "Stale", "issues": []}).to_string()),
            &frame(),
            &cycle_lookup,
            &|_| true,
        )
        .unwrap();
        assert!(outcome.rows.is_empty());
    }

    // FX-ACT-02 · module builders.
    #[test]
    fn module_create_row() {
        let resolve = |id: &str| (id == MODULE_M).then(|| "Backend".to_owned());
        let (row, touched) = plan_create_module_issue(
            Some(&format!(r#"{{"module_id": "{MODULE_M}"}}"#)),
            &frame(),
            &resolve,
            &|_| true,
        )
        .unwrap();
        assert_eq!(row.comment, "added module Backend");
        assert_eq!(row.field.as_deref(), Some("modules"));
        assert_eq!(row.new_value.as_deref(), Some("Backend"));
        assert_eq!(row.old_value.as_deref(), Some(""));
        assert_eq!(row.new_identifier.as_deref(), Some(MODULE_M));
        assert_eq!(touched, vec!["issue-1".to_owned()]);
        // Missing row → "" name, raw id still echoed.
        let (row, _) = plan_create_module_issue(
            Some(r#"{"module_id": "gone"}"#),
            &frame(),
            &resolve,
            &|_| false,
        )
        .unwrap();
        assert_eq!(row.comment, "added module ");
        assert_eq!(row.new_value.as_deref(), Some(""));
        assert_eq!(row.new_identifier.as_deref(), Some("gone"));
    }

    #[test]
    fn module_delete_row_and_none_name() {
        let (row, touched) = plan_delete_module_issue(
            Some(&format!(r#"{{"module_id": "{MODULE_M}"}}"#)),
            Some(r#"{"module_name": "Backend"}"#),
            &frame(),
            &|_| true,
        )
        .unwrap();
        assert_eq!(row.comment, "removed this issue from Backend");
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.old_value.as_deref(), Some("Backend"));
        assert_eq!(row.new_value.as_deref(), Some(""));
        assert_eq!(row.old_identifier.as_deref(), Some(MODULE_M));
        assert_eq!(touched, vec!["issue-1".to_owned()]);
        // None name: NULL column, f-string interpolates "None".
        let (row, _) = plan_delete_module_issue(
            Some(r#"{}"#),
            Some(r#"{"module_name": null}"#),
            &frame(),
            &|_| false,
        )
        .unwrap();
        assert_eq!(row.comment, "removed this issue from None");
        assert_eq!(row.old_value, None);
        assert_eq!(row.old_identifier, None);
    }
}
