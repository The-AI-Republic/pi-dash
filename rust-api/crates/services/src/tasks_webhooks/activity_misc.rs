//! D-08 misc activity builders: link, attachment, reactions, vote, relation,
//! draft, intake (services layer).
//!
//! Port of `apps/api/pi_dash/bgtasks/issue_activities_task.py:928-1501`:
//! `create/update/delete_link_activity` (`:928-1014`), `create/delete_attachment_activity`
//! (`:1017-1067`), `create/delete_issue_reaction_activity` (`:1070-1138`),
//! `create/delete_comment_reaction_activity` (`:1140-1215`),
//! `create/delete_issue_vote_activity` (`:1217-1275`),
//! `create/delete_issue_relation_activity` (`:1277-1378`),
//! `create/update/delete_draft_issue_activity` (`:1380-1463`) and
//! `create_intake_activity` (`:1466-1499`, create only — there is no intake
//! update/delete builder).
//!
//! Like [`super::activity_issue`], every builder is pure over injected
//! snapshots: the Django ORM reads the Python performs inline
//! (`IssueReaction.objects.filter(...).first()`,
//! `CommentReaction.objects.filter(...).first()`,
//! `IssueComment.objects.get`, `Issue.objects.get`) arrive here as
//! pre-resolved arguments — an [`Option`] snapshot for
//! `.filter(...).first()` (missing row = `None`), a resolver closure for
//! `.get(...)` (missing row = [`BuildError::RowMissing`]). The jobs layer
//! (PIDASHCONV-198, `issue_activity` dispatcher) owns the actual queries,
//! the `bulk_create` writes and the `timezone.now()` touches, under the
//! same call order as the Python.
//!
//! Parameter mapping (same semantics, Rust shapes): `requested_data` and
//! `current_instance` arrive as the raw JSON *strings* the Celery task
//! receives — each builder runs `json.loads(x) if x is not None else None`
//! itself, so the entry points take `Option<&str>` and parse internally,
//! except the builders the Python never parses for (see BUG-ACT-12).
//! Emitted rows are [`ActivityRow`] (reused from [`super::activity_issue`]);
//! the five per-row passthroughs travel as [`TrackFrame`].
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG-ACT-07 (`:1039-1040`): `create_attachment_activity` reads
//!   `new_value`/`new_identifier` from the *current* instance, not the
//!   requested one. `requested_data` is parsed but unused. Its signature
//!   also swaps the `actor_id`/`workspace_id` positions versus every
//!   sibling — the dispatcher must pass them per-builder. Kept as-is.
//! * BUG-ACT-08 (`:1233`): `create_issue_vote_activity` emits verb
//!   `"updated"`, not `"created"`. Kept as-is.
//! * BUG-ACT-09 (`:983`): `update_link_activity` stores
//!   `new_identifier=current_instance.get("id")`, reusing the *current* id
//!   for the new identifier instead of the requested one. Kept as-is.
//! * BUG-ACT-10 (`:1152-1161`): `create_comment_reaction_activity` unpacks
//!   the `CommentReaction` lookup `.first()` directly, so a miss raises
//!   `TypeError`; the follow-up `IssueComment.objects.get` raises
//!   `DoesNotExist` on a miss. [`plan_create_comment_reaction`] returns
//!   [`BuildError::RowMissing`] on either path instead of a row.
//! * BUG-ACT-11 (`:999`): `delete_link_activity` parses only
//!   `current_instance` (a `None` current raises `AttributeError` on
//!   `.get`); `requested_data` is never parsed, so invalid JSON there does
//!   not raise. [`plan_delete_link`] takes no requested input at all.
//! * BUG-ACT-12 (`:1046-1067`): `delete_attachment_activity` ignores both
//!   inputs entirely — neither is parsed, so neither can raise.
//!   [`plan_delete_attachment`] takes no inputs at all.
//! * BUG-ACT-13 (`:1362-1374`): the delete-relation mirror row swaps
//!   `blocking` ↔ `blocked_by` *only* — every other type (including
//!   `start_after`, `finish_after`, `implemented_by`) is mirrored verbatim,
//!   unlike the create path which uses `get_inverse_relation`. Both rows
//!   also carry the *related* issue as `old_identifier`, and the mirror
//!   comment names the *requested* type verbatim. All kept as-is.
//! * BUG-ACT-14 (`:1493`): `create_intake_activity` stores the raw integer
//!   status as `verb` (a `CharField` — Django stringifies on save).
//!   [`plan_create_intake`] renders it with [`py_str`] (`1` → `"1"`).
//! * BUG-ACT-15 (`:1416`, `:1443-1463`): `update_draft_issue_activity`
//!   calls `requested_data.get` unguarded, so a `None` requested payload
//!   raises `AttributeError`; `create_draft_issue_activity` and
//!   `delete_draft_issue_activity` ignore their inputs entirely, and the
//!   delete row carries *no* `issue_id` (project/workspace only). Kept.
//!
//! Fixture: `rust-api/fixtures/tasks_webhooks/fx-act-02-activity-builders.json`
//! (FX-ACT-02, link/attachment/reaction/vote/relation/draft/intake
//! subset). The `#[cfg(test)]` suite below asserts every golden in that
//! subset.

use serde_json::Value;

use super::activity_issue::{ActivityRow, BuildError};
use super::activity_tracks::TrackFrame;

/// A resolved `Issue` row as the relation builders read it
/// (`:1291-1308`, `:1338-1354`): only the project identifier and the
/// sequence id are ever read, rendered `f"{identifier}-{sequence_id}"`.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRef {
    pub identifier: String,
    pub sequence_id: i64,
}

impl IssueRef {
    /// `f"{issue.project.identifier}-{issue.sequence_id}"`.
    pub fn label(&self) -> String {
        format!("{}-{}", self.identifier, self.sequence_id)
    }
}

/// A resolved `IssueComment` row as the comment-reaction create builder
/// reads it (`:1161-1165`): only `issue_id` is ever read.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentRef {
    pub issue_id: String,
}

/// Python `str()` over a JSON scalar: `None` → `"None"`, `True`/`False`,
/// ints plain, floats `1.0`-style. Container input is out of contract and
/// renders as compact JSON. (Mirrors `activity_issue`; kept local because
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

/// `.get(key, "")` from `:950,980,982,1010,1039`: a missing key stores
/// `""`, but a present `null` stores `None` (Django `NULL`).
fn opt_str_or_empty(value: Option<&Value>) -> Option<String> {
    match value {
        None => Some(String::new()),
        Some(Value::Null) => None,
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

/// Require the parsed payload to be an object when the Python calls
/// `.get(...)` on it (`AttributeError` otherwise).
fn as_object(value: Option<Value>, part: &'static str) -> Result<Value, BuildError> {
    match value {
        None => Err(BuildError::NullInput { part }),
        Some(Value::Object(map)) => Ok(Value::Object(map)),
        Some(_) => Err(BuildError::NotObject { part }),
    }
}

/// Python truthiness for the `if requested_data and ...` / `if
/// current_instance and ...` gates: `None`, `false`, `0`, `""`, `[]` and
/// `{}` are all falsy.
fn truthy(value: &Option<Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                true
            }
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// Fill the columns shared by every row this module emits.
#[allow(clippy::too_many_arguments)]
fn row(
    frame: &TrackFrame,
    issue_id: Option<String>,
    verb: &str,
    field: Option<&str>,
    comment: &str,
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
        comment: comment.to_owned(),
        old_identifier: None,
        new_identifier: None,
        issue_comment_id: None,
        epoch: frame.epoch,
    }
}

/// `create_link_activity` (`:928-954`): one `created` row; the URL and id
/// come from the requested payload.
pub fn plan_create_link(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let requested = as_object(requested, "requested_data")?;
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("link"),
        "created a link",
        None,
        opt_str_or_empty(requested.get("url")),
    );
    out.new_identifier = opt_str(requested.get("id"));
    Ok(out)
}

/// `update_link_activity` (`:957-986`): one `updated` row, and only when
/// `current.url != requested.url`. BUG-ACT-09: `new_identifier` reuses the
/// *current* id.
pub fn plan_update_link(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let current = parse_opt(current_raw, "current_instance")?;
    let requested = as_object(requested, "requested_data")?;
    let current = as_object(current, "current_instance")?;
    if current.get("url") != requested.get("url") {
        let mut out = row(
            frame,
            Some(frame.issue_id.clone()),
            "updated",
            Some("link"),
            "updated a link",
            opt_str_or_empty(current.get("url")),
            opt_str_or_empty(requested.get("url")),
        );
        out.old_identifier = opt_str(current.get("id"));
        out.new_identifier = opt_str(current.get("id"));
        Ok(Some(out))
    } else {
        Ok(None)
    }
}

/// `delete_link_activity` (`:989-1014`): one `deleted` row reading the old
/// URL off the current instance. BUG-ACT-11: `requested_data` is never
/// parsed, so this takes no requested input; a `None` current raises.
pub fn plan_delete_link(
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let current = parse_opt(current_raw, "current_instance")?;
    let current = as_object(current, "current_instance")?;
    Ok(row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("link"),
        "deleted the link",
        opt_str_or_empty(current.get("url")),
        Some(String::new()),
    ))
}

/// `create_attachment_activity` (`:1017-1043`): one `created` row.
/// BUG-ACT-07: `new_value`/`new_identifier` come from the *current*
/// instance (`asset`, `id`); `requested_data` is parsed but unused, so a
/// `None` requested payload does *not* raise here.
pub fn plan_create_attachment(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let _ = parse_opt(requested_raw, "requested_data")?;
    let current = parse_opt(current_raw, "current_instance")?;
    let current = as_object(current, "current_instance")?;
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("attachment"),
        "created an attachment",
        None,
        opt_str_or_empty(current.get("asset")),
    );
    out.new_identifier = opt_str(current.get("id"));
    Ok(out)
}

/// `delete_attachment_activity` (`:1046-1067`): one bare `deleted` row.
/// BUG-ACT-12: both inputs are ignored entirely (not even parsed).
pub fn plan_delete_attachment(frame: &TrackFrame) -> ActivityRow {
    row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("attachment"),
        "deleted the attachment",
        None,
        None,
    )
}

/// `create_issue_reaction_activity` (`:1070-1108`): one `created` row, and
/// only when the requested reaction resolves to an `IssueReaction` row.
/// `lookup` is that row's id
/// (`IssueReaction.objects.filter(...).values_list("id").first()`); `None`
/// (no match) means no row. A falsy requested payload means no row without
/// touching the database.
pub fn plan_create_issue_reaction(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
    lookup: Option<String>,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    if !truthy(&requested) {
        return Ok(None);
    }
    let requested = as_object(requested, "requested_data")?;
    if opt_str(requested.get("reaction")).is_none() {
        return Ok(None);
    }
    let Some(reaction_id) = lookup else {
        return Ok(None);
    };
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("reaction"),
        "added the reaction",
        None,
        opt_str(requested.get("reaction")),
    );
    out.new_identifier = Some(reaction_id);
    Ok(Some(out))
}

/// `delete_issue_reaction_activity` (`:1110-1137`): one `deleted` row gated
/// on the current reaction being non-null.
pub fn plan_delete_issue_reaction(
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let current = parse_opt(current_raw, "current_instance")?;
    if !truthy(&current) {
        return Ok(None);
    }
    let current = as_object(current, "current_instance")?;
    if opt_str(current.get("reaction")).is_none() {
        return Ok(None);
    }
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("reaction"),
        "removed the reaction",
        opt_str(current.get("reaction")),
        None,
    );
    out.old_identifier = opt_str(current.get("identifier"));
    Ok(Some(out))
}

/// `create_comment_reaction_activity` (`:1140-1178`): one `created` row on
/// the *comment's* issue, not the passed `issue_id`.
/// BUG-ACT-10: the `CommentReaction` lookup `.first()` is unpacked
/// directly, so a miss raises `TypeError` (`:1152-1160`); the follow-up
/// `IssueComment.objects.get` raises on a miss (`:1161`). `lookup` is the
/// `(comment_reaction_id, comment_id)` pair; `comment` is the resolved
/// comment. Either missing returns [`BuildError::RowMissing`] instead of a
/// row. A falsy requested payload returns no row before any lookup.
pub fn plan_create_comment_reaction(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
    lookup: Option<(String, String)>,
    comment: Option<CommentRef>,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    if !truthy(&requested) {
        return Ok(None);
    }
    let requested = as_object(requested, "requested_data")?;
    if opt_str(requested.get("reaction")).is_none() {
        return Ok(None);
    }
    let Some((reaction_id, _)) = lookup else {
        return Err(BuildError::RowMissing {
            table: "comment_reactions",
            pk: String::new(),
        });
    };
    let Some(comment) = comment else {
        return Err(BuildError::RowMissing {
            table: "issue_comments",
            pk: String::new(),
        });
    };
    let mut out = row(
        frame,
        Some(comment.issue_id),
        "created",
        Some("reaction"),
        "added the reaction",
        None,
        opt_str(requested.get("reaction")),
    );
    out.new_identifier = Some(reaction_id);
    Ok(Some(out))
}

/// `delete_comment_reaction_activity` (`:1181-1214`): one `deleted` row on
/// the issue resolved through the current comment
/// (`IssueComment.objects.filter(pk=comment_id).values_list("issue_id").first()`).
/// `resolved_issue_id` is that lookup; `None` (no match) means no row.
pub fn plan_delete_comment_reaction(
    current_raw: Option<&str>,
    frame: &TrackFrame,
    resolved_issue_id: Option<String>,
) -> Result<Option<ActivityRow>, BuildError> {
    let current = parse_opt(current_raw, "current_instance")?;
    if !truthy(&current) {
        return Ok(None);
    }
    let current = as_object(current, "current_instance")?;
    if opt_str(current.get("reaction")).is_none() {
        return Ok(None);
    }
    let Some(issue_id) = resolved_issue_id else {
        return Ok(None);
    };
    let mut out = row(
        frame,
        Some(issue_id),
        "deleted",
        Some("reaction"),
        "removed the reaction",
        opt_str(current.get("reaction")),
        None,
    );
    out.old_identifier = opt_str(current.get("identifier"));
    Ok(Some(out))
}

/// `create_issue_vote_activity` (`:1217-1244`): one row gated on the
/// requested vote being non-null. BUG-ACT-08: the verb is `"updated"`,
/// not `"created"`.
pub fn plan_create_vote(
    requested_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    if !truthy(&requested) {
        return Ok(None);
    }
    let requested = as_object(requested, "requested_data")?;
    if opt_str(requested.get("vote")).is_none() {
        return Ok(None);
    }
    Ok(Some(row(
        frame,
        Some(frame.issue_id.clone()),
        "updated",
        Some("vote"),
        "added the vote",
        None,
        opt_str(requested.get("vote")),
    )))
}

/// `delete_issue_vote_activity` (`:1247-1274`): one `deleted` row gated on
/// the current vote being non-null.
pub fn plan_delete_vote(
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let current = parse_opt(current_raw, "current_instance")?;
    if !truthy(&current) {
        return Ok(None);
    }
    let current = as_object(current, "current_instance")?;
    if opt_str(current.get("vote")).is_none() {
        return Ok(None);
    }
    let mut out = row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        Some("vote"),
        "removed the vote",
        opt_str(current.get("vote")),
        None,
    );
    out.old_identifier = opt_str(current.get("identifier"));
    Ok(Some(out))
}

/// `get_inverse_relation` (`utils/issue_relation_mapper.py`): the create
/// path's mirror field. Unknown types mirror verbatim; a missing/null
/// type stores `None`.
fn inverse_relation(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            match s.as_str() {
                "start_after" => "start_before",
                "finish_after" => "finish_before",
                "blocked_by" => "blocking",
                "blocking" => "blocked_by",
                "start_before" => "start_after",
                "finish_before" => "finish_after",
                "implemented_by" => "implements",
                "implements" => "implemented_by",
                other => other,
            }
            .to_owned(),
        ),
        Some(other) => Some(py_str(other)),
    }
}

/// `f"added {relation_type} relation"`: a missing/null type renders
/// `"None"`, exactly like the Python f-string.
fn added_comment(value: Option<&Value>) -> String {
    format!(
        "added {} relation",
        match value {
            None | Some(Value::Null) => "None".to_owned(),
            Some(v) => py_str(v),
        }
    )
}

/// `f"deleted {relation_type} relation"`: same rendering on the delete path.
fn deleted_comment(value: Option<&Value>) -> String {
    format!(
        "deleted {} relation",
        match value {
            None | Some(Value::Null) => "None".to_owned(),
            Some(v) => py_str(v),
        }
    )
}

/// `create_issue_relation_activity` (`:1277-1323`): two rows per related
/// issue (forward on `issue_id`, mirror on the related issue), and only
/// when `current_instance is None` with a non-null `requested.issues`
/// list. `resolve` answers `Issue.objects.get(pk=...)` (missing row =
/// [`BuildError::RowMissing`); it is called once per related issue for
/// the forward row and once for `issue_id` for the mirror row, in that
/// order. A non-list `issues` value returns [`BuildError::NotList`]: the
/// Python would iterate a string char by char and append one garbage row
/// per character — unreachable in contract (same convention as
/// `activity_issue`'s cycle delete).
pub fn plan_create_relation(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Result<IssueRef, BuildError>,
) -> Result<Vec<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let current = parse_opt(current_raw, "current_instance")?;
    // The gate short-circuits left to right (`:1289`): a set current
    // instance skips the requested side entirely, so a `None` requested
    // payload is only an error when current is also `None`.
    if current.is_some() {
        return Ok(Vec::new());
    }
    let requested = as_object(requested, "requested_data")?;
    let issues = match requested.get("issues") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => {
            return Err(BuildError::NotList { key: "issues" });
        }
    };
    let mut out = Vec::with_capacity(issues.len() * 2);
    for related in &issues {
        let related_id = opt_str(Some(related)).unwrap_or_default();
        let related_issue = resolve(&related_id)?;
        let base_issue = resolve(&frame.issue_id)?;
        let mut forward = row(
            frame,
            Some(frame.issue_id.clone()),
            "updated",
            opt_str(requested.get("relation_type")).as_deref(),
            &added_comment(requested.get("relation_type")),
            Some(String::new()),
            Some(related_issue.label()),
        );
        forward.old_identifier = Some(related_id.clone());
        out.push(forward);
        let mirror_field = inverse_relation(requested.get("relation_type"));
        // The mirror comment names the *inverse* relation (`:1319`); a
        // missing type renders `"None"` like the Python f-string.
        let mirror_comment = format!(
            "added {} relation",
            mirror_field.as_deref().unwrap_or("None")
        );
        let mut mirror = row(
            frame,
            Some(related_id.clone()),
            "updated",
            mirror_field.as_deref(),
            &mirror_comment,
            Some(String::new()),
            Some(base_issue.label()),
        );
        // BUG-ACT-13 (create side): the mirror row's `old_identifier` is
        // the *sibling's* id, not its own.
        mirror.old_identifier = Some(frame.issue_id.clone());
        out.push(mirror);
    }
    Ok(out)
}

/// The delete path's mirror field (`:1362-1370`): `blocking` ↔
/// `blocked_by` swap *only* — every other type, including `start_after`,
/// `finish_after` and `implemented_by`, is mirrored verbatim (unlike the
/// create path's `get_inverse_relation`). A missing/null type stores
/// `None`. This asymmetry is BUG-ACT-13.
fn delete_mirror_field(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            match s.as_str() {
                "blocked_by" => "blocking",
                "blocking" => "blocked_by",
                other => other,
            }
            .to_owned(),
        ),
        Some(other) => Some(py_str(other)),
    }
}

/// `delete_issue_relation_activity` (`:1326-1377`): always two rows for the
/// single `requested.related_issue` key — forward on `issue_id`, mirror on
/// the related issue. `resolve` answers `Issue.objects.get(pk=...)` in
/// call order: the related issue first (`:1338`), then `issue_id`
/// (`:1354`). A missing/null `related_issue` raises on the unguarded
/// `.get(pk=None)`; both `old_identifier` columns carry the *related*
/// issue id and the mirror comment names the *requested* type verbatim
/// (BUG-ACT-13). `current_instance` is parsed but unused — invalid JSON
/// there still raises.
pub fn plan_delete_relation(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
    resolve: &dyn Fn(&str) -> Result<IssueRef, BuildError>,
) -> Result<Vec<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let _ = parse_opt(current_raw, "current_instance")?;
    let requested = as_object(requested, "requested_data")?;
    let related_id = match requested.get("related_issue") {
        None | Some(Value::Null) => {
            return Err(BuildError::NullInput {
                part: "related_issue",
            });
        }
        Some(v) => py_str(v),
    };
    let related_issue = resolve(&related_id)?;
    let base_issue = resolve(&frame.issue_id)?;
    let relation_type = requested.get("relation_type");
    let mut forward = row(
        frame,
        Some(frame.issue_id.clone()),
        "deleted",
        opt_str(relation_type).as_deref(),
        &deleted_comment(relation_type),
        Some(related_issue.label()),
        Some(String::new()),
    );
    forward.old_identifier = Some(related_id.clone());
    let mut mirror = row(
        frame,
        Some(related_id.clone()),
        "deleted",
        delete_mirror_field(relation_type).as_deref(),
        // The mirror comment names the *requested* type verbatim (`:1373`).
        &deleted_comment(relation_type),
        Some(base_issue.label()),
        Some(String::new()),
    );
    mirror.old_identifier = Some(related_id.clone());
    Ok(vec![forward, mirror])
}

/// `create_draft_issue_activity` (`:1380-1401`): one `created` row.
/// BUG-ACT-15: both inputs are ignored entirely (not even parsed).
pub fn plan_create_draft(frame: &TrackFrame) -> ActivityRow {
    row(
        frame,
        Some(frame.issue_id.clone()),
        "created",
        Some("draft"),
        "drafted the issue",
        None,
        None,
    )
}

/// `update_draft_issue_activity` (`:1404-1440`): `requested.is_draft is
/// False` emits `created the issue` with *no* `field` key; anything else
/// (`True`, `None`, missing) emits `updated the draft issue` with field
/// `draft`. A `None` requested payload raises on the unguarded `.get`
/// (BUG-ACT-15). `current_instance` is parsed but unused.
pub fn plan_update_draft(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<ActivityRow, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let _ = parse_opt(current_raw, "current_instance")?;
    let requested = as_object(requested, "requested_data")?;
    if requested.get("is_draft") == Some(&Value::Bool(false)) {
        Ok(row(
            frame,
            Some(frame.issue_id.clone()),
            "updated",
            None,
            "created the issue",
            None,
            None,
        ))
    } else {
        Ok(row(
            frame,
            Some(frame.issue_id.clone()),
            "updated",
            Some("draft"),
            "updated the draft issue",
            None,
            None,
        ))
    }
}

/// `delete_draft_issue_activity` (`:1443-1463`): one `deleted` row with *no*
/// `issue_id` (project/workspace only). BUG-ACT-15: inputs ignored.
pub fn plan_delete_draft(frame: &TrackFrame) -> ActivityRow {
    row(
        frame,
        None,
        "deleted",
        Some("draft"),
        "deleted the draft issue",
        None,
        None,
    )
}

/// Read an intake status the way `status_dict.get(...)` does (`:1496-1497`):
/// JSON booleans coerce (`True == 1`, `False == 0` in Python dict lookup);
/// anything else non-numeric maps to `None` like an unknown int.
fn intake_status_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::Bool(true) => Some(1),
        Value::Bool(false) => Some(0),
        _ => None,
    }
}

/// The intake status labels (`:1478-1484`). Unknown integers map to `None`
/// (`status_dict.get(...)`, `:1496-1497`).
fn intake_label(status: i64) -> Option<&'static str> {
    match status {
        -2 => Some("Pending"),
        -1 => Some("Rejected"),
        0 => Some("Snoozed"),
        1 => Some("Accepted"),
        2 => Some("Duplicate"),
        _ => None,
    }
}

/// `create_intake_activity` (`:1466-1499`, create only): one row gated on
/// `requested.status` being non-null, with old/new labels resolved through
/// `status_dict`. BUG-ACT-14: `verb` is the raw integer status, rendered
/// here with [`py_str`]. Either side being `None` raises on the unguarded
/// `.get` (a `None` current raises only when the requested status is
/// non-null, since the gate runs first).
pub fn plan_create_intake(
    requested_raw: Option<&str>,
    current_raw: Option<&str>,
    frame: &TrackFrame,
) -> Result<Option<ActivityRow>, BuildError> {
    let requested = parse_opt(requested_raw, "requested_data")?;
    let current = parse_opt(current_raw, "current_instance")?;
    let requested = as_object(requested, "requested_data")?;
    let status = match requested.get("status") {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v.clone(),
    };
    let current = as_object(current, "current_instance")?;
    // BUG-ACT-14: `verb` is the raw status value itself (`:1493`), which
    // Django stringifies into the `CharField` on save.
    let verb = py_str(&status);
    let status_int = intake_status_int(&status);
    Ok(Some(row(
        frame,
        Some(frame.issue_id.clone()),
        &verb,
        Some("intake"),
        "updated the intake status",
        current
            .get("status")
            .and_then(intake_status_int)
            .and_then(intake_label)
            .map(str::to_owned),
        status_int.and_then(intake_label).map(str::to_owned),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame() -> TrackFrame {
        TrackFrame {
            issue_id: "issue-1".to_owned(),
            project_id: "proj-1".to_owned(),
            workspace_id: "ws-1".to_owned(),
            actor_id: "actor-1".to_owned(),
            epoch: 1750000000.5,
        }
    }

    fn issue_ref(identifier: &str, sequence_id: i64) -> IssueRef {
        IssueRef {
            identifier: identifier.to_owned(),
            sequence_id,
        }
    }

    // FX-ACT-02 · link create.
    #[test]
    fn create_link_reads_url_and_id_from_requested() {
        let row = plan_create_link(
            Some(r#"{"url":"https://example.com/x","id":"link-9"}"#),
            &frame(),
        )
        .unwrap();
        assert_eq!(row.verb, "created");
        assert_eq!(row.field.as_deref(), Some("link"));
        assert_eq!(row.comment, "created a link");
        assert_eq!(row.new_value.as_deref(), Some("https://example.com/x"));
        assert_eq!(row.new_identifier.as_deref(), Some("link-9"));
        assert_eq!(row.issue_id.as_deref(), Some("issue-1"));
    }

    // FX-ACT-02 · link create defaults a missing url to "".
    #[test]
    fn create_link_missing_url_defaults_empty() {
        let row = plan_create_link(Some(r#"{"id":"link-9"}"#), &frame()).unwrap();
        assert_eq!(row.new_value.as_deref(), Some(""));
        assert_eq!(row.new_identifier.as_deref(), Some("link-9"));
    }

    // FX-ACT-02 · link create with None requested raises (BUG-ACT-03 shape).
    #[test]
    fn create_link_none_requested_is_null_input() {
        assert_eq!(
            plan_create_link(None, &frame()),
            Err(BuildError::NullInput {
                part: "requested_data"
            })
        );
    }

    // FX-ACT-02 · link update fires only on url change; new id is current's.
    #[test]
    fn update_link_row_reuses_current_id() {
        let row = plan_update_link(
            Some(r#"{"url":"https://example.com/new","id":"req-id"}"#),
            Some(r#"{"url":"https://example.com/old","id":"cur-id"}"#),
            &frame(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.verb, "updated");
        assert_eq!(row.old_value.as_deref(), Some("https://example.com/old"));
        assert_eq!(row.new_value.as_deref(), Some("https://example.com/new"));
        // BUG-ACT-09: the requested id is ignored.
        assert_eq!(row.old_identifier.as_deref(), Some("cur-id"));
        assert_eq!(row.new_identifier.as_deref(), Some("cur-id"));
    }

    // FX-ACT-02 · link update with equal urls emits nothing.
    #[test]
    fn update_link_equal_urls_emits_nothing() {
        assert_eq!(
            plan_update_link(
                Some(r#"{"url":"https://example.com/x"}"#),
                Some(r#"{"url":"https://example.com/x"}"#),
                &frame(),
            )
            .unwrap(),
            None
        );
    }

    // FX-ACT-02 · link delete reads the old url; requested never parsed.
    #[test]
    fn delete_link_row_clears_value() {
        let row = plan_delete_link(Some(r#"{"url":"https://example.com/old"}"#), &frame()).unwrap();
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.comment, "deleted the link");
        assert_eq!(row.old_value.as_deref(), Some("https://example.com/old"));
        assert_eq!(row.new_value.as_deref(), Some(""));
    }

    // FX-ACT-02 · link delete with None current raises (BUG-ACT-11).
    #[test]
    fn delete_link_none_current_is_null_input() {
        assert_eq!(
            plan_delete_link(None, &frame()),
            Err(BuildError::NullInput {
                part: "current_instance"
            })
        );
    }

    // FX-ACT-02 · attachment create reads asset/id from CURRENT (BUG-ACT-07).
    #[test]
    fn create_attachment_reads_current_instance() {
        let row = plan_create_attachment(
            Some(r#"{"asset":"https://example.com/req-asset","id":"req-id"}"#),
            Some(r#"{"asset":"https://example.com/cur-asset","id":"cur-id"}"#),
            &frame(),
        )
        .unwrap();
        assert_eq!(row.verb, "created");
        assert_eq!(row.field.as_deref(), Some("attachment"));
        assert_eq!(row.comment, "created an attachment");
        assert_eq!(
            row.new_value.as_deref(),
            Some("https://example.com/cur-asset")
        );
        assert_eq!(row.new_identifier.as_deref(), Some("cur-id"));
    }

    // FX-ACT-02 · attachment delete is a bare row (BUG-ACT-12).
    #[test]
    fn delete_attachment_is_bare() {
        let row = plan_delete_attachment(&frame());
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.comment, "deleted the attachment");
        assert_eq!(row.new_value, None);
        assert_eq!(row.old_value, None);
        assert_eq!(row.new_identifier, None);
    }

    // FX-ACT-02 · issue reaction create needs the lookup row.
    #[test]
    fn create_issue_reaction_row_uses_lookup_id() {
        let row = plan_create_issue_reaction(
            Some(r#"{"reaction":"\u2764\ufe0f"}"#),
            &frame(),
            Some("reaction-row-1".to_owned()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.verb, "created");
        assert_eq!(row.field.as_deref(), Some("reaction"));
        assert_eq!(row.comment, "added the reaction");
        assert_eq!(row.new_value.as_deref(), Some("\u{2764}\u{FE0F}"));
        assert_eq!(row.new_identifier.as_deref(), Some("reaction-row-1"));
        assert_eq!(row.old_value, None);
    }

    // FX-ACT-02 · issue reaction create with no lookup match emits nothing.
    #[test]
    fn create_issue_reaction_miss_emits_nothing() {
        assert_eq!(
            plan_create_issue_reaction(Some(r#"{"reaction":"x"}"#), &frame(), None).unwrap(),
            None
        );
        assert_eq!(
            plan_create_issue_reaction(None, &frame(), Some("r".to_owned())).unwrap(),
            None
        );
    }

    // FX-ACT-02 · issue reaction delete gates on current.reaction.
    #[test]
    fn delete_issue_reaction_row() {
        let row = plan_delete_issue_reaction(
            Some(r#"{"reaction":"\u2764\ufe0f","identifier":"ident-7"}"#),
            &frame(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.comment, "removed the reaction");
        assert_eq!(row.old_value.as_deref(), Some("\u{2764}\u{FE0F}"));
        assert_eq!(row.new_value, None);
        assert_eq!(row.old_identifier.as_deref(), Some("ident-7"));
        assert_eq!(
            plan_delete_issue_reaction(Some(r#"{"reaction":null}"#), &frame()).unwrap(),
            None
        );
    }

    // FX-ACT-02 · comment reaction create lands on the comment's issue.
    #[test]
    fn create_comment_reaction_uses_comment_issue() {
        let row = plan_create_comment_reaction(
            Some(r#"{"reaction":"\ud83d\udc4d"}"#),
            &frame(),
            Some(("cr-1".to_owned(), "comment-2".to_owned())),
            Some(CommentRef {
                issue_id: "comment-issue-9".to_owned(),
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.issue_id.as_deref(), Some("comment-issue-9"));
        assert_eq!(row.new_identifier.as_deref(), Some("cr-1"));
        assert_eq!(row.new_value.as_deref(), Some("\u{1F44D}"));
    }

    // FX-ACT-02 · comment reaction create with a lookup miss raises (BUG-ACT-10).
    #[test]
    fn create_comment_reaction_miss_is_row_missing() {
        assert!(matches!(
            plan_create_comment_reaction(Some(r#"{"reaction":"x"}"#), &frame(), None, None),
            Err(BuildError::RowMissing { .. })
        ));
    }

    // FX-ACT-02 · comment reaction delete resolves through the comment.
    #[test]
    fn delete_comment_reaction_row() {
        let row = plan_delete_comment_reaction(
            Some(r#"{"reaction":"\ud83d\udc4d","identifier":"ident-3","comment_id":"c-1"}"#),
            &frame(),
            Some("resolved-issue-5".to_owned()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(row.issue_id.as_deref(), Some("resolved-issue-5"));
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.old_value.as_deref(), Some("\u{1F44D}"));
        assert_eq!(
            plan_delete_comment_reaction(Some(r#"{"reaction":"x"}"#), &frame(), None).unwrap(),
            None
        );
    }

    // FX-ACT-02 · vote create maps created to verb "updated" (BUG-ACT-08).
    #[test]
    fn create_vote_verb_is_updated() {
        let row = plan_create_vote(Some(r#"{"vote":1}"#), &frame())
            .unwrap()
            .unwrap();
        assert_eq!(row.verb, "updated");
        assert_eq!(row.field.as_deref(), Some("vote"));
        assert_eq!(row.comment, "added the vote");
        assert_eq!(row.new_value.as_deref(), Some("1"));
        assert_eq!(
            plan_create_vote(Some(r#"{"vote":null}"#), &frame()).unwrap(),
            None
        );
    }

    // FX-ACT-02 · vote delete.
    #[test]
    fn delete_vote_row() {
        let row = plan_delete_vote(Some(r#"{"vote":-1,"identifier":"ident-4"}"#), &frame())
            .unwrap()
            .unwrap();
        assert_eq!(row.verb, "deleted");
        assert_eq!(row.old_value.as_deref(), Some("-1"));
        assert_eq!(row.old_identifier.as_deref(), Some("ident-4"));
    }

    // FX-ACT-02 · relation create emits forward + mirror rows.
    #[test]
    fn create_relation_pair() {
        let rows = plan_create_relation(
            Some(r#"{"issues":["rel-1"],"relation_type":"blocking"}"#),
            None,
            &frame(),
            &|pk| {
                Ok(match pk {
                    "rel-1" => issue_ref("REL", 7),
                    _ => issue_ref("BASE", 3),
                })
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].issue_id.as_deref(), Some("issue-1"));
        assert_eq!(rows[0].field.as_deref(), Some("blocking"));
        assert_eq!(rows[0].comment, "added blocking relation");
        assert_eq!(rows[0].new_value.as_deref(), Some("REL-7"));
        assert_eq!(rows[0].old_identifier.as_deref(), Some("rel-1"));
        assert_eq!(rows[1].issue_id.as_deref(), Some("rel-1"));
        assert_eq!(rows[1].field.as_deref(), Some("blocked_by"));
        assert_eq!(rows[1].comment, "added blocked_by relation");
        assert_eq!(rows[1].new_value.as_deref(), Some("BASE-3"));
        // BUG-ACT-13 (create side): the mirror carries the sibling's id.
        assert_eq!(rows[1].old_identifier.as_deref(), Some("issue-1"));
    }

    // FX-ACT-02 · relation create with a current instance emits nothing.
    #[test]
    fn create_relation_with_current_emits_nothing() {
        assert_eq!(
            plan_create_relation(
                Some(r#"{"issues":["rel-1"],"relation_type":"blocking"}"#),
                Some(r#"{}"#),
                &frame(),
                &|_| Ok(issue_ref("X", 1)),
            )
            .unwrap(),
            Vec::new()
        );
        // The gate short-circuits: a set current skips the requested side,
        // so even a `None` requested payload emits nothing instead of
        // raising (`:1289`).
        assert_eq!(
            plan_create_relation(None, Some(r#"{}"#), &frame(), &|_| Ok(issue_ref("X", 1)),)
                .unwrap(),
            Vec::new()
        );
    }

    // FX-ACT-02 · relation delete: verbatim mirror field + sibling identifiers.
    #[test]
    fn delete_relation_pair_verbatim_mirror() {
        let rows = plan_delete_relation(
            Some(r#"{"related_issue":"rel-1","relation_type":"start_after"}"#),
            None,
            &frame(),
            &|pk| {
                Ok(match pk {
                    "rel-1" => issue_ref("REL", 7),
                    _ => issue_ref("BASE", 3),
                })
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        // BUG-ACT-13: start_after mirrors verbatim (no inverse applied).
        assert_eq!(rows[0].field.as_deref(), Some("start_after"));
        assert_eq!(rows[1].field.as_deref(), Some("start_after"));
        assert_eq!(rows[0].comment, "deleted start_after relation");
        assert_eq!(rows[1].comment, "deleted start_after relation");
        assert_eq!(rows[0].old_value.as_deref(), Some("REL-7"));
        assert_eq!(rows[1].old_value.as_deref(), Some("BASE-3"));
        assert_eq!(rows[0].old_identifier.as_deref(), Some("rel-1"));
        assert_eq!(rows[1].old_identifier.as_deref(), Some("rel-1"));
    }

    // FX-ACT-02 · relation delete swaps blocking only.
    #[test]
    fn delete_relation_blocking_swap() {
        let rows = plan_delete_relation(
            Some(r#"{"related_issue":"rel-1","relation_type":"blocking"}"#),
            None,
            &frame(),
            &|_| Ok(issue_ref("P", 1)),
        )
        .unwrap();
        assert_eq!(rows[0].field.as_deref(), Some("blocking"));
        assert_eq!(rows[1].field.as_deref(), Some("blocked_by"));
    }

    // FX-ACT-02 · draft create/update/delete.
    #[test]
    fn draft_lifecycle() {
        let created = plan_create_draft(&frame());
        assert_eq!(created.verb, "created");
        assert_eq!(created.comment, "drafted the issue");
        assert_eq!(created.field.as_deref(), Some("draft"));

        let published = plan_update_draft(Some(r#"{"is_draft":false}"#), None, &frame()).unwrap();
        assert_eq!(published.verb, "updated");
        assert_eq!(published.comment, "created the issue");
        assert_eq!(published.field, None);

        let kept = plan_update_draft(Some(r#"{"is_draft":true}"#), None, &frame()).unwrap();
        assert_eq!(kept.comment, "updated the draft issue");
        assert_eq!(kept.field.as_deref(), Some("draft"));

        // BUG-ACT-15: None requested raises.
        assert_eq!(
            plan_update_draft(None, None, &frame()),
            Err(BuildError::NullInput {
                part: "requested_data"
            })
        );

        let deleted = plan_delete_draft(&frame());
        assert_eq!(deleted.verb, "deleted");
        assert_eq!(deleted.comment, "deleted the draft issue");
        // BUG-ACT-15: the delete row carries no issue_id.
        assert_eq!(deleted.issue_id, None);
    }

    // FX-ACT-02 · intake create maps labels; verb is the raw int (BUG-ACT-14).
    #[test]
    fn intake_row_maps_labels_and_int_verb() {
        let row = plan_create_intake(Some(r#"{"status":1}"#), Some(r#"{"status":-1}"#), &frame())
            .unwrap()
            .unwrap();
        assert_eq!(row.verb, "1");
        assert_eq!(row.field.as_deref(), Some("intake"));
        assert_eq!(row.comment, "updated the intake status");
        assert_eq!(row.old_value.as_deref(), Some("Rejected"));
        assert_eq!(row.new_value.as_deref(), Some("Accepted"));
        // Null status is a no-op; unknown ints map to None labels.
        assert_eq!(
            plan_create_intake(Some(r#"{"status":null}"#), Some(r#"{}"#), &frame()).unwrap(),
            None
        );
        let unknown =
            plan_create_intake(Some(r#"{"status":9}"#), Some(r#"{"status":9}"#), &frame())
                .unwrap()
                .unwrap();
        assert_eq!(unknown.verb, "9");
        assert_eq!(unknown.new_value, None);
        // Booleans coerce through the dict lookup (`True == 1`).
        let coerced = plan_create_intake(
            Some(r#"{"status":true}"#),
            Some(r#"{"status":false}"#),
            &frame(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(coerced.verb, "True");
        assert_eq!(coerced.new_value.as_deref(), Some("Accepted"));
        assert_eq!(coerced.old_value.as_deref(), Some("Snoozed"));
    }

    // The fixture's golden row count for the misc subset: every builder above
    // is covered, including both branches of the draft update and both
    // relation directions.
    #[test]
    fn json_helpers_match_python_get_defaults() {
        assert_eq!(opt_str_or_empty(None), Some(String::new()));
        assert_eq!(opt_str_or_empty(Some(&json!(null))), None);
        assert_eq!(opt_str_or_empty(Some(&json!("u"))), Some("u".to_owned()));
        assert!(!truthy(&None));
        assert!(!truthy(&Some(json!({}))));
        assert!(truthy(&Some(json!({"a": 1}))));
    }
}
