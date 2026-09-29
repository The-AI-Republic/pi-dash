#![forbid(unsafe_code)]

//! Intake task publishers: `issue_activity` + `issue_description_version_task`.
//!
//! Ports the five `.delay()` sites in
//! `apps/api/pi_dash/app/views/intake/base.py` (D-32, PIDASHCONV-345):
//!
//! * create, `issue_activity.delay(type="issue.activity.created", ...)`
//!   (`:274-285`): `requested_data` is the full `request.data` dump,
//!   `current_instance` is `None`, `notification` is `True`,
//!   `origin` is `base_host(request, is_app=True)`, `intake` is the new
//!   `IntakeIssue` row id.
//! * create, `issue_description_version_task.delay(...)` (`:287-292`):
//!   `updated_issue` is the same full-request dump, `user_id` is the raw
//!   `request.user.id` (a UUID object — kombu JSON-encodes it, so the wire
//!   value is the string form), `is_creating=True`.
//! * partial_update issue path, `issue_activity.delay(
//!   type="issue.activity.updated", ...)` (`:438-449`): `requested_data`
//!   is the popped `issue` sub-object dump, `current_instance` is the
//!   pre-save `IssueDetailSerializer(issue)` dump. Skipped together with
//!   the description-version emit when the migration-update predicate
//!   holds (`:434-436`, see [`is_migration_description_update`]).
//! * partial_update issue path, `issue_description_version_task.delay(...)`
//!   (`:451-455`): `updated_issue` is the pre-save snapshot (NOT the
//!   request dump), `is_creating` is omitted entirely so the worker
//!   default (`False`) applies.
//! * partial_update intake path, `issue_activity.delay(
//!   type="intake.activity.created", ...)` (`:460-471`): `requested_data`
//!   is the `request.data` dump *after* the `skip_activity` (`:330`) and
//!   `issue` (`:371`) pops, `current_instance` is the pre-save
//!   `IntakeIssueSerializer` dump, `notification` is `False`.
//!
//! Fixture oracle (PIDASHCONV-278):
//! `rust-api/fixtures/app_intake/tasks/intake_create.before_after.json`
//! (`_trace: base.py:221-326`) and
//! `rust-api/fixtures/app_intake/tasks/intake_update.before_after.json`
//! (`_trace: base.py:328-500 + destroy 549-566`). The tests below replay
//! every `enqueued` payload field for field, in call-site kwarg order.
//!
//! Wire contract (Porting guide Jobs plane row): `.delay(**kwargs)`
//! publishes a first-attempt Celery protocol v2 message with `args = []`
//! and kwargs in call-site order. Both tasks are bare `@shared_task`
//! (`bgtasks/issue_activities_task.py:1503`,
//! `bgtasks/issue_description_version_task.py:43`), so the wire names are
//! the dotted module paths in [`ISSUE_ACTIVITY_TASK`] /
//! [`DESCRIPTION_VERSION_TASK`] and there is no queue override
//! (`task_routes` unset in `celery.py`).
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! [`IssueActivityEmit::task_name`] + [`IssueActivityEmit::kwargs`] (and
//! the description-version twin) — and the handlers (PIDASHCONV-385/395,
//! in the `api` crate which already depends on `pidash-jobs`) wrap them
//! with `CeleryTaskMessage::new(task, vec![], kwargs)` plus
//! `queue::enqueue`, exactly like the space intake handlers do
//! (`api/src/space/intake.rs:1260-1269`). The dump *text*
//! (`requested_data`, `current_instance`, `updated_issue`, `origin`) is
//! the caller's job: `json.dumps(..., cls=DjangoJSONEncoder)` in Python,
//! i.e. CPython `", "` / `": "` separators over the serializer output
//! (the `api` crate's `python_dumps` owns that rendering).
//!
//! No worker `Registry` handler is registered for either name: both are
//! Python-owned, so the worker forwards them to RabbitMQ in Celery
//! protocol v2 (Porting guide: the registry starts empty; D-07…D-10 flip
//! one task group at a time, and intake tasks are not among them).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-pop-then-dump (`:330`, `:371`, `:462`): the intake-branch
//!   `requested_data` never contains `skip_activity` or `issue` — both
//!   are popped off `request.data` before the dump. The create-path dump
//!   keeps them (nothing is popped there).
//! * QUIRK-omitted-is-creating (`:451-455`): the update-path
//!   description-version emit carries no `is_creating` kwarg at all; the
//!   worker default `False` applies. The port omits the key rather than
//!   sending `false`.
//! * QUIRK-uuid-user-id (`:290`, `:454`): `user_id` is passed as the raw
//!   `request.user.id` UUID object, not `str(...)` (unlike `actor_id`).
//!   Kombu's JSON encoder renders UUIDs as strings, so the wire value is
//!   the string form either way; the field takes the string.

use serde_json::{Map, Value};

/// Celery wire name for `issue_activity` (bare `@shared_task` default).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Celery wire name for `issue_description_version_task` (bare default;
///
/// also pinned by the jobs crate at
/// `jobs/src/tasks_cleanup/versions.rs:96`).
pub const DESCRIPTION_VERSION_TASK: &str =
    "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task";

/// Activity `type` strings for the three `issue_activity.delay` sites.
pub const ISSUE_CREATED: &str = "issue.activity.created";
pub const ISSUE_UPDATED: &str = "issue.activity.updated";
pub const INTAKE_CREATED: &str = "intake.activity.created";

/// Kwarg order of every `issue_activity.delay` call in this domain
/// (`base.py:274-285`, `:438-449`, `:460-471` — identical order at all
/// three sites: the extra `notification` / `origin` / `intake` kwargs
/// follow `epoch`).
pub const ISSUE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
    "notification",
    "origin",
    "intake",
];

/// Kwarg order of `issue_description_version_task.delay`
/// (`base.py:287-292` create, `:451-455` update without `is_creating`).
pub const DESCRIPTION_VERSION_KWARG_ORDER: &[&str] =
    &["updated_issue", "issue_id", "user_id", "is_creating"];

/// One `issue_activity.delay(...)` call: keyword args exactly as the view
/// passes them. `requested_data` / `current_instance` / `origin` are the
/// pre-rendered `json.dumps` text (`None` for `current_instance` only on
/// the create path); `epoch` is `int(timezone.now().timestamp())`,
/// supplied by the caller; `intake` is `str(intake_issue.id)` — the
/// `IntakeIssue` bridge row, not the `Intake` row.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityEmit {
    pub activity_type: String,
    pub requested_data: String,
    pub actor_id: String,
    pub issue_id: String,
    pub project_id: String,
    pub current_instance: Option<String>,
    pub epoch: i64,
    pub notification: bool,
    pub origin: String,
    pub intake_issue_id: String,
}

impl IssueActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        ISSUE_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(ISSUE_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert("type".to_owned(), Value::String(self.activity_type.clone()));
        kwargs.insert(
            "requested_data".to_owned(),
            Value::String(self.requested_data.clone()),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("issue_id".to_owned(), Value::String(self.issue_id.clone()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            self.current_instance
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("epoch".to_owned(), Value::Number(self.epoch.into()));
        kwargs.insert("notification".to_owned(), Value::Bool(self.notification));
        kwargs.insert("origin".to_owned(), Value::String(self.origin.clone()));
        kwargs.insert(
            "intake".to_owned(),
            Value::String(self.intake_issue_id.clone()),
        );
        kwargs
    }
}

/// Create path, `issue.activity.created` (`base.py:274-285`):
/// `current_instance=None`, `notification=True`.
pub fn intake_create_activity(
    requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    epoch: i64,
    origin: String,
    intake_issue_id: String,
) -> IssueActivityEmit {
    IssueActivityEmit {
        activity_type: ISSUE_CREATED.to_owned(),
        requested_data,
        actor_id,
        issue_id,
        project_id,
        current_instance: None,
        epoch,
        notification: true,
        origin,
        intake_issue_id,
    }
}

/// Partial_update issue path, `issue.activity.updated` (`base.py:438-449`):
/// `requested_data` is the popped `issue` sub-object dump,
/// `current_instance` the pre-save `IssueDetailSerializer` dump,
/// `notification=True`. The caller skips this (and the description-version
/// emit) when [`is_migration_description_update`] holds.
#[allow(clippy::too_many_arguments)]
pub fn intake_update_issue_activity(
    issue_requested_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    issue_current_instance: String,
    epoch: i64,
    origin: String,
    intake_issue_id: String,
) -> IssueActivityEmit {
    IssueActivityEmit {
        activity_type: ISSUE_UPDATED.to_owned(),
        requested_data: issue_requested_data,
        actor_id,
        issue_id,
        project_id,
        current_instance: Some(issue_current_instance),
        epoch,
        notification: true,
        origin,
        intake_issue_id,
    }
}

/// Partial_update intake path, `intake.activity.created` (`base.py:460-471`):
/// `requested_data` is the post-pop `request.data` dump, `current_instance`
/// the pre-save `IntakeIssueSerializer` dump, `notification=False`.
#[allow(clippy::too_many_arguments)]
pub fn intake_update_intake_activity(
    request_data: String,
    actor_id: String,
    issue_id: String,
    project_id: String,
    intake_current_instance: String,
    epoch: i64,
    origin: String,
    intake_issue_id: String,
) -> IssueActivityEmit {
    IssueActivityEmit {
        activity_type: INTAKE_CREATED.to_owned(),
        requested_data: request_data,
        actor_id,
        issue_id,
        project_id,
        current_instance: Some(intake_current_instance),
        epoch,
        notification: false,
        origin,
        intake_issue_id,
    }
}

/// One `issue_description_version_task.delay(...)` call
/// (`base.py:287-292`, `:451-455`). `updated_issue` is pre-rendered dump
/// text (full request JSON on create, pre-save snapshot on update);
/// `user_id` is the string form of `request.user.id` (QUIRK-uuid-user-id);
/// `is_creating` is `Some(true)` on create and `None` on update — the
/// update path omits the kwarg so the worker default `False` applies
/// (QUIRK-omitted-is-creating).
#[derive(Debug, Clone, PartialEq)]
pub struct DescriptionVersionEmit {
    pub updated_issue: String,
    pub issue_id: String,
    pub user_id: String,
    pub is_creating: Option<bool>,
}

impl DescriptionVersionEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        DESCRIPTION_VERSION_TASK
    }

    /// `.delay()` kwargs in call-site order (`is_creating` last, and only
    /// when present). `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(DESCRIPTION_VERSION_KWARG_ORDER.len());
        kwargs.insert(
            "updated_issue".to_owned(),
            Value::String(self.updated_issue.clone()),
        );
        kwargs.insert("issue_id".to_owned(), Value::String(self.issue_id.clone()));
        kwargs.insert("user_id".to_owned(), Value::String(self.user_id.clone()));
        if let Some(is_creating) = self.is_creating {
            kwargs.insert("is_creating".to_owned(), Value::Bool(is_creating));
        }
        kwargs
    }
}

/// Create path (`base.py:287-292`): full-request dump + `is_creating=True`.
pub fn intake_create_description_version(
    request_data: String,
    issue_id: String,
    user_id: String,
) -> DescriptionVersionEmit {
    DescriptionVersionEmit {
        updated_issue: request_data,
        issue_id,
        user_id,
        is_creating: Some(true),
    }
}

/// Partial_update issue path (`base.py:451-455`): pre-save snapshot, no
/// `is_creating` kwarg.
pub fn intake_update_description_version(
    issue_current_instance: String,
    issue_id: String,
    user_id: String,
) -> DescriptionVersionEmit {
    DescriptionVersionEmit {
        updated_issue: issue_current_instance,
        issue_id,
        user_id,
        is_creating: None,
    }
}

/// Migration-update silent path (`base.py:434`):
/// `is_migration_description_update = skip_activity and is_description_update`,
/// where `skip_activity = request.data.pop("skip_activity", False)` and
/// `is_description_update = request.data.get("description_html") is not None`
/// (`:330-331`). When true, the issue-branch emits (`:436-455`) are skipped
/// entirely — neither the `issue.activity.updated` nor the
/// description-version task fires. The intake branch (`:457-471`) is
/// unaffected.
pub fn is_migration_description_update(skip_activity: bool, is_description_update: bool) -> bool {
    skip_activity && is_description_update
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_order(kwargs: &Map<String, Value>) -> Vec<&str> {
        kwargs.keys().map(String::as_str).collect()
    }

    #[test]
    fn wire_names_are_bare_shared_task_paths() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            DESCRIPTION_VERSION_TASK,
            "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task"
        );
    }

    /// Fixture `tasks/intake_create.before_after.json`, `enqueued[0]`
    /// (`base.py:274-285`): field-for-field kwargs plus call-site order.
    #[test]
    fn create_activity_matches_fixture() {
        let emit = intake_create_activity(
            "<request.data as JSON>".to_owned(),
            "<user-uuid>".to_owned(),
            "<new-issue-uuid>".to_owned(),
            "<project-uuid>".to_owned(),
            1_700_000_000,
            "<base_host app url>".to_owned(),
            "<new-intake-issue-uuid>".to_owned(),
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "type": "issue.activity.created",
                "requested_data": "<request.data as JSON>",
                "actor_id": "<user-uuid>",
                "issue_id": "<new-issue-uuid>",
                "project_id": "<project-uuid>",
                "current_instance": null,
                "epoch": 1_700_000_000,
                "notification": true,
                "origin": "<base_host app url>",
                "intake": "<new-intake-issue-uuid>",
            })
        );
    }

    /// Fixture `tasks/intake_create.before_after.json`, `enqueued[1]`
    /// (`base.py:287-292`).
    #[test]
    fn create_description_version_matches_fixture() {
        let emit = intake_create_description_version(
            "<request.data as JSON>".to_owned(),
            "<new-issue-uuid>".to_owned(),
            "<user-id>".to_owned(),
        );
        assert_eq!(emit.task_name(), DESCRIPTION_VERSION_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), DESCRIPTION_VERSION_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "updated_issue": "<request.data as JSON>",
                "issue_id": "<new-issue-uuid>",
                "user_id": "<user-id>",
                "is_creating": true,
            })
        );
    }

    /// Fixture `tasks/intake_update.before_after.json`, `enqueued[0]`
    /// (`base.py:438-449`): pre-save snapshot as `current_instance`,
    /// `notification` still true.
    #[test]
    fn update_issue_activity_matches_fixture() {
        let emit = intake_update_issue_activity(
            "<issue_data JSON>".to_owned(),
            "<user-uuid>".to_owned(),
            "<issue-uuid>".to_owned(),
            "<project-uuid>".to_owned(),
            "<IssueDetailSerializer(issue) JSON before save>".to_owned(),
            1_700_000_000,
            "<base_host app url>".to_owned(),
            "<intake-issue-uuid>".to_owned(),
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "type": "issue.activity.updated",
                "requested_data": "<issue_data JSON>",
                "actor_id": "<user-uuid>",
                "issue_id": "<issue-uuid>",
                "project_id": "<project-uuid>",
                "current_instance": "<IssueDetailSerializer(issue) JSON before save>",
                "epoch": 1_700_000_000,
                "notification": true,
                "origin": "<base_host app url>",
                "intake": "<intake-issue-uuid>",
            })
        );
    }

    /// Fixture `tasks/intake_update.before_after.json`, `enqueued[1]`
    /// (`base.py:451-455`): `updated_issue` is the pre-save snapshot and
    /// `is_creating` is absent (QUIRK-omitted-is-creating).
    #[test]
    fn update_description_version_omits_is_creating() {
        let emit = intake_update_description_version(
            "<pre-save issue JSON>".to_owned(),
            "<pk>".to_owned(),
            "<user-id>".to_owned(),
        );
        assert_eq!(emit.task_name(), DESCRIPTION_VERSION_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(
            key_order(&kwargs),
            &["updated_issue", "issue_id", "user_id"]
        );
        assert!(!kwargs.contains_key("is_creating"));
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "updated_issue": "<pre-save issue JSON>",
                "issue_id": "<pk>",
                "user_id": "<user-id>",
            })
        );
    }

    /// Fixture `tasks/intake_update.before_after.json`, `enqueued[2]`
    /// (`base.py:460-471`): `type` is `intake.activity.created`,
    /// `notification` is false, `current_instance` the pre-save intake dump.
    #[test]
    fn update_intake_activity_matches_fixture() {
        let emit = intake_update_intake_activity(
            "<request.data JSON>".to_owned(),
            "<user-uuid>".to_owned(),
            "<pk>".to_owned(),
            "<project-uuid>".to_owned(),
            "<IntakeIssueSerializer JSON before save>".to_owned(),
            1_700_000_000,
            "<base_host app url>".to_owned(),
            "<intake-issue-uuid>".to_owned(),
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "type": "intake.activity.created",
                "requested_data": "<request.data JSON>",
                "actor_id": "<user-uuid>",
                "issue_id": "<pk>",
                "project_id": "<project-uuid>",
                "current_instance": "<IntakeIssueSerializer JSON before save>",
                "epoch": 1_700_000_000,
                "notification": false,
                "origin": "<base_host app url>",
                "intake": "<intake-issue-uuid>",
            })
        );
    }

    /// `base.py:434`: the silent path needs both flags together.
    #[test]
    fn migration_update_predicate_needs_both_flags() {
        assert!(is_migration_description_update(true, true));
        assert!(!is_migration_description_update(true, false));
        assert!(!is_migration_description_update(false, true));
        assert!(!is_migration_description_update(false, false));
    }
}
