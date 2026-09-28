#![forbid(unsafe_code)]

//! Space public-API task publishers (D-02, stage 4).
//!
//! Port of every `.delay()` call in `apps/api/pi_dash/space/views/`:
//!
//! * eleven `issue_activity.delay(...)` sites in `space/views/issue.py`
//!   (`:274-282` comment create, `:308-316` comment partial_update,
//!   `:329-337` comment destroy, `:391-399` issue_reaction create,
//!   `:417-425` issue_reaction destroy, `:476-484` comment_reaction create,
//!   `:503-517` comment_reaction destroy, `:561-569` vote create,
//!   `:581-589` vote destroy) and `space/views/intake.py` (`:155-163`
//!   intake create, `:223-231` intake partial_update);
//! * one `get_asset_object_metadata.delay(...)` site in
//!   `space/views/asset.py:148` (asset patch, guarded by
//!   `if not asset.storage_metadata` at `:147`).
//!
//! Fixture oracle: `rust-api/fixtures/space/tasks/enqueue.golden.json`
//! (trace: `rust-api/fixtures/space/TRACE.md`, Tasks section).
//! Task signatures are read-only (`bgtasks/issue_activities_task.py:1503`
//! and `bgtasks/storage_metadata_task.py:14`); both are bare
//! `@shared_task`, so the wire names are the dotted module paths below
//! and there is no queue override (`task_routes` unset, `CELERY_IMPORTS`
//! relies on autodiscovery).
//!
//! Wire contract (F-09, `crate::celery`): `.delay(**kwargs)` publishes a
//! first-attempt Celery protocol v2 message with `args = []` and the
//! kwargs below; `get_asset_object_metadata.delay(str)` publishes one
//! positional arg with empty kwargs. Every keyword call site passes its
//! kwargs in the same order — `type, requested_data, actor_id, issue_id,
//! project_id, current_instance, epoch` — and [`IssueActivityPublish`]
//! inserts them in exactly that order (`preserve_order`).
//!
//! `requested_data` and `current_instance` are pre-serialized JSON *text*
//! (`json.dumps(..., cls=DjangoJSONEncoder)` in Python, `None` where the
//! site passes `None`). They pass through verbatim: this module never
//! re-encodes them, so DjangoJSONEncoder shapes (datetimes, Decimals,
//! UUIDs) survive byte for byte. Rendering that text is the caller's job
//! (the D-02 handler issues own the serializer goldens).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * `comment_reaction_created` sends `project_id` as the literal string
//!   `"None"` (`views/issue.py:481` reads the absent `project_id` URL
//!   kwarg; the sibling destroy at `:508` correctly uses the board's
//!   project). [`comment_reaction_created`] takes `project_id` as a plain
//!   parameter so the handler reproduces the bug with the same
//!   `str(kwargs.get("project_id", None))` logic; the unit test pins the
//!   `"None"` value.

use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;

/// Celery wire name for `issue_activity` (bare `@shared_task` default).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Celery wire name for `get_asset_object_metadata` (bare default).
pub const GET_ASSET_OBJECT_METADATA_TASK: &str =
    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// Activity `type` strings, one per `issue_activity.delay` call site.
pub const COMMENT_CREATED: &str = "comment.activity.created";
pub const COMMENT_UPDATED: &str = "comment.activity.updated";
pub const COMMENT_DELETED: &str = "comment.activity.deleted";
pub const ISSUE_REACTION_CREATED: &str = "issue_reaction.activity.created";
pub const ISSUE_REACTION_DELETED: &str = "issue_reaction.activity.deleted";
pub const COMMENT_REACTION_CREATED: &str = "comment_reaction.activity.created";
pub const COMMENT_REACTION_DELETED: &str = "comment_reaction.activity.deleted";
pub const ISSUE_VOTE_CREATED: &str = "issue_vote.activity.created";
pub const ISSUE_VOTE_DELETED: &str = "issue_vote.activity.deleted";
pub const ISSUE_CREATED: &str = "issue.activity.created";
pub const ISSUE_UPDATED: &str = "issue.activity.updated";

/// One `issue_activity.delay(...)` call: keyword args exactly as the view
/// passes them. `requested_data` / `current_instance` are the rendered
/// `json.dumps` text (`None` where the site passes `None`); `issue_id` is
/// `None` on the comment_reaction sites; `epoch` is
/// `int(timezone.now().timestamp())`, supplied by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityPublish {
    pub activity_type: String,
    pub requested_data: Option<String>,
    pub actor_id: String,
    pub issue_id: Option<String>,
    pub project_id: String,
    pub current_instance: Option<String>,
    pub epoch: i64,
}

impl IssueActivityPublish {
    /// Build the `.delay()` message: first attempt, `args = []`, kwargs in
    /// the call-site order `type, requested_data, actor_id, issue_id,
    /// project_id, current_instance, epoch`.
    pub fn message(&self) -> CeleryTaskMessage {
        let mut kwargs = Map::new();
        kwargs.insert("type".to_owned(), Value::String(self.activity_type.clone()));
        kwargs.insert("requested_data".to_owned(), opt_str(&self.requested_data));
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("issue_id".to_owned(), opt_str(&self.issue_id));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            opt_str(&self.current_instance),
        );
        kwargs.insert("epoch".to_owned(), Value::Number(self.epoch.into()));
        CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, Vec::new(), kwargs)
    }
}

fn opt_str(value: &Option<String>) -> Value {
    value.clone().map(Value::String).unwrap_or(Value::Null)
}

/// Comment create (`views/issue.py:274-282`): `requested_data` is the
/// validated serializer output dump; `current_instance` is `None`.
/// The view then runs the [`MEMBER_EXISTS_SQL`] guard and the
/// get-or-create pair below for non-members (`:284-293`).
pub fn comment_created(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: COMMENT_CREATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: None,
        epoch,
    }
}

/// Comment partial_update (`views/issue.py:308-316`): `requested_data` is
/// the raw `request.data` dump; `current_instance` is the serializer
/// snapshot of the comment row.
pub fn comment_updated(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: COMMENT_UPDATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Comment destroy (`views/issue.py:329-337`): `requested_data` is the
/// `{"comment_id": str(pk)}` dump; the delay fires *before*
/// `comment.delete()` (`:338`), so `current_instance` is the pre-delete
/// snapshot.
pub fn comment_deleted(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: COMMENT_DELETED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Issue_reaction create (`views/issue.py:391-399`): `requested_data` is
/// the `request.data` dump; `issue_id` is `str(kwargs issue_id)`.
/// Non-members get the [`MEMBER_EXISTS_SQL`] guard + get-or-create
/// (`:385-390`, before the delay).
pub fn issue_reaction_created(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_REACTION_CREATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: None,
        epoch,
    }
}

/// Issue_reaction destroy (`views/issue.py:417-425`): `requested_data` is
/// `None`; `current_instance` is the `{"reaction", "identifier"}` dump
/// (no issue/comment id in this shape).
pub fn issue_reaction_deleted(
    actor_id: String,
    issue_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_REACTION_DELETED.to_owned(),
        requested_data: None,
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Comment_reaction create (`views/issue.py:476-484`): `issue_id` is
/// `None`, and BUG-PORT `project_id` is the literal string `"None"`
/// (`str(kwargs.get("project_id", None))` with no such URL kwarg on the
/// comment-reaction routes). Non-members get the guard + get-or-create
/// (`:469-474`, before the delay).
pub fn comment_reaction_created(
    requested_data: String,
    actor_id: String,
    project_id: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: COMMENT_REACTION_CREATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: None,
        project_id,
        current_instance: None,
        epoch,
    }
}

/// Comment_reaction destroy (`views/issue.py:503-517`): `issue_id` is
/// `None`, `requested_data` is `None`; `current_instance` is the
/// `{"reaction", "identifier", "comment_id"}` dump (note the extra
/// `comment_id` versus the issue_reaction shape); `project_id` is the
/// board's project here — correct, unlike the create site above.
pub fn comment_reaction_deleted(
    actor_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: COMMENT_REACTION_DELETED.to_owned(),
        requested_data: None,
        actor_id,
        issue_id: None,
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Vote create (`views/issue.py:561-569`): `requested_data` is the
/// `request.data` dump. The row itself is
/// `IssueVote.objects.get_or_create(actor, project, issue)` (`:541-545`)
/// with `vote = request.data.get("vote", 1)` (`:557`), always 201;
/// non-members get the guard + get-or-create (`:546-555`).
pub fn issue_vote_created(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_VOTE_CREATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: None,
        epoch,
    }
}

/// Vote destroy (`views/issue.py:581-589`): `requested_data` is `None`;
/// `current_instance` is the `{"vote", "identifier"}` dump.
pub fn issue_vote_deleted(
    actor_id: String,
    issue_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_VOTE_DELETED.to_owned(),
        requested_data: None,
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Intake create (`views/intake.py:155-163`): `requested_data` is the full
/// intake payload dump; `issue_id` is the newly created row; no
/// membership side effect on this path.
pub fn intake_created(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_CREATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: None,
        epoch,
    }
}

/// Intake partial_update (`views/intake.py:223-231`): `requested_data` is
/// the popped 3-key `{name, description_html, description_json}` subset
/// dump (`:197,205-209,221`); `current_instance` is the pre-save
/// `IssueSerializer` snapshot (`:219,229`). Fires only when the partial
/// serializer validates (`:216`), before `save()` (`:232`).
pub fn intake_updated(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    current_instance: String,
    epoch: i64,
) -> IssueActivityPublish {
    IssueActivityPublish {
        activity_type: ISSUE_UPDATED.to_owned(),
        requested_data: Some(requested_data),
        actor_id,
        issue_id: Some(issue_id),
        project_id,
        current_instance: Some(current_instance),
        epoch,
    }
}

/// Asset patch (`views/asset.py:148`): `get_asset_object_metadata.delay`
/// with one positional arg, `str(asset.id)`. Fires only when
/// `not asset.storage_metadata` (`:147`), after `is_uploaded = True` is
/// set but before `save(update_fields=["attributes", "is_uploaded"])`
/// (`:145-153`). No kwargs.
pub fn asset_metadata_message(asset_id: &str) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        GET_ASSET_OBJECT_METADATA_TASK,
        vec![Value::String(asset_id.to_owned())],
        Map::new(),
    )
}

/// Membership guard shared by the comment / issue_reaction /
/// comment_reaction / vote create paths (`views/issue.py:284-288`,
/// `:381-386`, `:469-473`, `:551-555`):
/// `ProjectMember.objects.filter(project_id, member, is_active=True).exists()`.
/// Table `project_members` (`db/models/project.py` Meta); the default
/// manager is `SoftDeletionManager` (`db/mixins.py:56-58`), so soft-deleted
/// rows are excluded via `deleted_at IS NULL`.
/// `$1` = project id, `$2` = member (user) id.
pub const MEMBER_EXISTS_SQL: &str = "SELECT 1 AS a FROM project_members WHERE project_id = $1 AND member_id = $2 AND is_active = TRUE AND deleted_at IS NULL LIMIT 1";

/// `ProjectPublicMember.objects.get_or_create(project_id, member)` — the
/// GET half (`views/issue.py:289-291` and siblings). Table
/// `project_public_members` (`db/models/project.py:442-461`); the default
/// manager is `SoftDeletionManager` (`db/mixins.py:56-58`), so soft-deleted
/// rows are excluded via `deleted_at IS NULL` (consistent with the partial
/// unique index on live rows). `$1` = project id, `$2` = member id.
pub const PUBLIC_MEMBER_GET_SQL: &str =
    "SELECT id FROM project_public_members WHERE project_id = $1 AND member_id = $2 AND deleted_at IS NULL LIMIT 1";

/// The CREATE half of the same `get_or_create`: `id` is `uuid4`
/// (`BaseModel`, `db/models/base.py:18`), timestamps are automatic,
/// `workspace_id` is forced from `project.workspace`
/// (`ProjectBaseModel.save`, `db/models/project.py:309-311`), and
/// `created_by/updated_by` follow the request user (NULL when anonymous).
/// `$1` = new row id, `$2` = project id, `$3` = workspace id,
/// `$4` = member id.
pub const PUBLIC_MEMBER_INSERT_SQL: &str = "INSERT INTO project_public_members (id, created_at, updated_at, project_id, workspace_id, member_id) VALUES ($1, now(), now(), $2, $3, $4)";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mirror of `contract-tests/_harness/celery_wire.py::assert_wire_message`
    /// plus the F-09 superset rule: the nine protocol-v2 header keys are
    /// present, `task`/`lang` match, and the body carries exactly the
    /// published args/kwargs.
    fn assert_wire(
        message: &CeleryTaskMessage,
        task: &str,
        args: Value,
        kwargs: &Map<String, Value>,
    ) {
        let headers = message.headers();
        for key in [
            "lang",
            "task",
            "id",
            "eta",
            "expires",
            "retries",
            "timelimit",
            "root_id",
            "parent_id",
        ] {
            assert!(headers.contains_key(key), "wire headers missing {key}");
        }
        assert_eq!(headers["task"], Value::String(task.to_owned()));
        assert_eq!(headers["lang"], Value::String("py".to_owned()));
        assert_eq!(message.task, task);
        assert_eq!(message.retries, 0);
        assert_eq!(message.effective_root_id(), message.id);
        let body = message.body();
        let body_args = body[0].clone();
        let body_kwargs = body[1].as_object().expect("kwargs object").clone();
        assert_eq!(body_args, args);
        assert_eq!(&body_kwargs, kwargs);
    }

    fn kwargs_keys(message: &CeleryTaskMessage) -> Vec<String> {
        message.body()[1]
            .as_object()
            .expect("kwargs object")
            .keys()
            .cloned()
            .collect()
    }

    const KWARG_ORDER: [&str; 7] = [
        "type",
        "requested_data",
        "actor_id",
        "issue_id",
        "project_id",
        "current_instance",
        "epoch",
    ];

    #[test]
    fn task_names_match_shared_task_defaults() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            GET_ASSET_OBJECT_METADATA_TASK,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
    }

    #[test]
    fn comment_created_wire_matches_call_site() {
        let message = comment_created(
            "{\"id\": \"c1\"}".to_owned(),
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("comment.activity.created"));
        assert_eq!(kwargs["requested_data"], json!("{\"id\": \"c1\"}"));
        assert_eq!(kwargs["actor_id"], json!("u1"));
        assert_eq!(kwargs["issue_id"], json!("i1"));
        assert_eq!(kwargs["project_id"], json!("p1"));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["epoch"], json!(1727));
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
        let headers = message.headers();
        assert!(headers["kwargsrepr"]
            .as_str()
            .expect("kwargsrepr")
            .contains("'type': 'comment.activity.created'"));
    }

    #[test]
    fn comment_updated_carries_raw_request_and_snapshot() {
        let message = comment_updated(
            "{\"comment_html\": \"<p>new</p>\"}".to_owned(),
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            "{\"comment_html\": \"<p>old</p>\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("comment.activity.updated"));
        assert_eq!(
            kwargs["requested_data"],
            json!("{\"comment_html\": \"<p>new</p>\"}")
        );
        assert_eq!(
            kwargs["current_instance"],
            json!("{\"comment_html\": \"<p>old</p>\"}")
        );
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn comment_deleted_requested_data_is_comment_id_dump() {
        let message = comment_deleted(
            "{\"comment_id\": \"c9\"}".to_owned(),
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            "{\"id\": \"c9\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("comment.activity.deleted"));
        assert_eq!(kwargs["requested_data"], json!("{\"comment_id\": \"c9\"}"));
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn issue_reaction_created_wire_matches_call_site() {
        let message = issue_reaction_created(
            "{\"reaction\": \"heart\"}".to_owned(),
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue_reaction.activity.created"));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn issue_reaction_deleted_requested_data_is_null() {
        let message = issue_reaction_deleted(
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            "{\"reaction\": \"heart\", \"identifier\": \"r1\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue_reaction.activity.deleted"));
        assert_eq!(kwargs["requested_data"], Value::Null);
        assert_eq!(
            kwargs["current_instance"],
            json!("{\"reaction\": \"heart\", \"identifier\": \"r1\"}")
        );
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn comment_reaction_created_ports_project_id_none_bug() {
        // views/issue.py:481 — the create site stringifies the absent
        // `project_id` URL kwarg, so the wire carries "None".
        let message = comment_reaction_created(
            "{\"reaction\": \"heart\"}".to_owned(),
            "u1".to_owned(),
            "None".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("comment_reaction.activity.created"));
        assert_eq!(kwargs["issue_id"], Value::Null);
        assert_eq!(kwargs["project_id"], json!("None"));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn comment_reaction_deleted_shape_has_comment_id() {
        let message = comment_reaction_deleted(
            "u1".to_owned(),
            "p1".to_owned(),
            "{\"reaction\": \"heart\", \"identifier\": \"r2\", \"comment_id\": \"c1\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("comment_reaction.activity.deleted"));
        assert_eq!(kwargs["requested_data"], Value::Null);
        assert_eq!(kwargs["issue_id"], Value::Null);
        assert_eq!(kwargs["project_id"], json!("p1"));
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn issue_vote_created_wire_matches_call_site() {
        let message = issue_vote_created(
            "{\"vote\": 1}".to_owned(),
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue_vote.activity.created"));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn issue_vote_deleted_current_instance_is_vote_dump() {
        let message = issue_vote_deleted(
            "u1".to_owned(),
            "i1".to_owned(),
            "p1".to_owned(),
            "{\"vote\": \"1\", \"identifier\": \"v1\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue_vote.activity.deleted"));
        assert_eq!(kwargs["requested_data"], Value::Null);
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn intake_created_uses_new_row_issue_id() {
        let message = intake_created(
            "{\"issue\": {\"name\": \"n\"}}".to_owned(),
            "u1".to_owned(),
            "i9".to_owned(),
            "p1".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue.activity.created"));
        assert_eq!(kwargs["issue_id"], json!("i9"));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn intake_updated_carries_subset_and_serializer_snapshot() {
        let message = intake_updated(
            "{\"name\": \"n\"}".to_owned(),
            "u1".to_owned(),
            "i9".to_owned(),
            "p1".to_owned(),
            "{\"name\": \"o\"}".to_owned(),
            1727,
        )
        .message();
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue.activity.updated"));
        assert_eq!(kwargs_keys(&message), KWARG_ORDER);
    }

    #[test]
    fn asset_metadata_message_is_single_positional_arg() {
        let message = asset_metadata_message("a1");
        let kwargs = message.kwargs.clone();
        assert!(kwargs.is_empty());
        assert_wire(
            &message,
            GET_ASSET_OBJECT_METADATA_TASK,
            json!(["a1"]),
            &kwargs,
        );
        let headers = message.headers();
        assert!(headers["argsrepr"]
            .as_str()
            .expect("argsrepr")
            .contains("'a1'"));
    }

    fn parse_postgres(sql: &str) {
        sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
            .expect("space membership SQL must parse as Postgres");
    }

    #[test]
    fn membership_sql_parses_and_names_exact_tables() {
        parse_postgres(MEMBER_EXISTS_SQL);
        parse_postgres(PUBLIC_MEMBER_GET_SQL);
        parse_postgres(PUBLIC_MEMBER_INSERT_SQL);
        assert!(MEMBER_EXISTS_SQL.contains("project_members"));
        assert!(MEMBER_EXISTS_SQL.contains("is_active"));
        // Django's default manager filters soft-deleted rows on both reads.
        assert!(MEMBER_EXISTS_SQL.contains("deleted_at IS NULL"));
        assert!(PUBLIC_MEMBER_GET_SQL.contains("deleted_at IS NULL"));
        assert!(PUBLIC_MEMBER_GET_SQL.contains("project_public_members"));
        assert!(PUBLIC_MEMBER_INSERT_SQL.contains("project_public_members"));
        assert!(PUBLIC_MEMBER_INSERT_SQL.contains("workspace_id"));
    }
}
