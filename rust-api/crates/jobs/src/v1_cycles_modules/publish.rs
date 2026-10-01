//! api-v1 cycles + modules task publishers (D-20, jobs layer, PIDASHCONV-310).
//!
//! Port of the eleven `.delay()` call sites in
//! `apps/api/pi_dash/api/views/cycle.py`, `api/views/module.py` and
//! `utils/cycle_transfer_issues.py`, verified against fixture FX-CYCMOD-07
//! (`rust-api/fixtures/v1_cycles_modules/tasks/enqueue.golden.json`):
//!
//! * `model_activity.delay` — cycle create (`cycle.py:338-346`,
//!   `current_instance=None`), cycle update (`:549-557`, `current_instance`
//!   is the pre-save `CycleSerializer` dump), module create
//!   (`module.py:229-237`), module update (`:439-447`). Kwarg order at all
//!   four: `model_name, model_id, requested_data, current_instance,
//!   actor_id, slug, origin`.
//! * `issue_activity.delay` — cycle delete (`cycle.py:594-608`), cycle
//!   issue add/move (`:990-1005`, `notification=True` + `origin`), cycle
//!   issue remove (`:1100-1113`), module delete (`module.py:512-527`,
//!   `current_instance={module_name}` + `origin`), module issue add/move
//!   (`:714-728`, `origin`, no `notification`), module issue remove
//!   (`:879-887`, `current_instance={module_name}`, no `origin`), and the
//!   transfer fan-out (`cycle_transfer_issues.py:462-477`,
//!   `notification=True` + `origin`, `created_cycle_issues: []`).
//!
//! This module owns the Celery wire surface for those sites — the task
//! names, the kwargs in exact Python call order (`preserve_order` keeps
//! insertion order on the wire) — plus the [`NewJob`] constructors the
//! D-20 route handlers enqueue transactionally (`enqueue_in`, so the row
//! commits or rolls back with the request: the post-commit half of the
//! F-04 tx wrapper applied to task fan-out). It never implements task
//! bodies and never touches the [`Registry`]: both bodies + handlers live
//! in D-08 (`tasks_webhooks::activity_dispatch` for `issue_activity`,
//! `tasks_webhooks::webhook_fanout` for `model_activity`), so registering
//! either name again here would fork ownership.
//!
//! Both tasks are plain `@shared_task`s (no bind, no autoretry), so the
//! queue rows carry the Celery defaults: `max_retries = 3`
//! ([`DEFAULT_MAX_RETRIES`]) with the 180s default retry delay
//! ([`DEFAULT_RETRY_DELAY_SECS`]) — exactly what [`NewJob::new`] sets.
//!
//! # Wire types worth stating
//!
//! * `model_activity.requested_data` is the raw `request.data` dict, so it
//!   travels as a JSON *object* (not a string); `actor_id` is the raw
//!   `request.user.id` UUID object, which kombu's JSON serializer renders
//!   as the hyphenated string — the publishers take both in wire form.
//! * `issue_activity` payloads that embed raw request data or Django
//!   serializer output (`cycles_list`/`modules_list` dumps, the
//!   double-encoded `created_*_issues` strings) pass through verbatim as
//!   caller-rendered text, so DjangoJSON shapes survive byte for byte;
//!   rendering them is the caller's job. Payloads fully determined by
//!   scalar inputs (the four delete/remove sites, the transfer fan-out)
//!   render here via [`django_dumps`].
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * Cycle delete carries neither `current_instance` nor `origin` while
//!   module delete carries both (`current_instance={module_name}`).
//! * Module issue-add sends `{"modules_list": str(issues)}` — `str()` of
//!   the re-queried values list — and no `notification` flag, while cycle
//!   issue-add sends the raw list and `notification=True`.
//! * Module issue-remove has `current_instance={module_name}` but no
//!   `origin`; its `issue_id` kwarg is the URL kwarg while
//!   `requested_data` embeds the row's id (provably equal: the row is
//!   fetched by that id, so one parameter feeds both slots).
//! * The add sites double-encode `serializers.serialize("json", ...)`
//!   output inside `json.dumps(...)`; the transfer site instead sends
//!   `created_cycle_issues` as a bare `[]`.
//! * `project_id=str(self.kwargs.get("project_id", None))` would render
//!   `"None"` when unrouted — unreachable (always routed); ported as a
//!   plain pass-through parameter.
//!
//! Fixture: `rust-api/fixtures/v1_cycles_modules/tasks/enqueue.golden.json`
//! (FX-CYCMOD-07). The `#[cfg(test)]` suite below replays it: wire names,
//! kwarg order, null-ness, rendered payloads, and a bind round-trip
//! through the D-08 binders.
//!
//! [`Registry`]: crate::worker::Registry
//! [`NewJob`]: crate::queue::NewJob
//! [`DEFAULT_MAX_RETRIES`]: crate::queue::DEFAULT_MAX_RETRIES
//! [`DEFAULT_RETRY_DELAY_SECS`]: crate::queue::DEFAULT_RETRY_DELAY_SECS
//! [`django_dumps`]: crate::tasks_webhooks::activity_dispatch::django_dumps

use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use crate::queue::NewJob;
use crate::tasks_webhooks::activity_dispatch::django_dumps;
/// The activity task name, owned by D-08
/// (`tasks_webhooks::activity_dispatch::ISSUE_ACTIVITY_TASK`).
/// Re-exported so publishers pin the same string the dispatcher binds.
pub use crate::tasks_webhooks::activity_dispatch::ISSUE_ACTIVITY_TASK;
/// The model-activity task name, owned by D-08
/// (`tasks_webhooks::webhook_fanout::MODEL_ACTIVITY_TASK_NAME`).
/// Re-exported — never redefined — so the two domains pin one string.
pub use crate::tasks_webhooks::webhook_fanout::MODEL_ACTIVITY_TASK_NAME;

/// `cycle.activity.created` (issue add/move `:990`, transfer `:462`).
pub const CYCLE_CREATED: &str = "cycle.activity.created";
/// `cycle.activity.deleted` (cycle delete `:594`, issue remove `:1100`).
pub const CYCLE_DELETED: &str = "cycle.activity.deleted";
/// `module.activity.created` (issue add/move `module.py:714`).
pub const MODULE_CREATED: &str = "module.activity.created";
/// `module.activity.deleted` (module delete `:512`, issue remove `:879`).
pub const MODULE_DELETED: &str = "module.activity.deleted";

fn opt_string(value: Option<&str>) -> Value {
    value
        .map(|v| Value::String(v.to_owned()))
        .unwrap_or(Value::Null)
}

/// Lift a constructed message into its queue row: `args=[]` with the same
/// kwargs, so the worker forward path rebuilds the identical Celery v2
/// body whichever plane serves the task. Retry policy is the
/// [`NewJob::new`] default (`max_retries = 3`, 180s delay), matching the
/// plain `@shared_task` on both Python tasks.
fn job_from_message(message: &CeleryTaskMessage) -> NewJob {
    NewJob::new(
        message.task.clone(),
        Value::Array(Vec::new()),
        Value::Object(message.kwargs.clone()),
    )
}

// ---------------------------------------------------------------------------
// `model_activity` (`webhook_task.py:463`)
// ---------------------------------------------------------------------------

/// Shared kwarg builder for the four `model_activity.delay` sites, in the
/// exact Python call order — `model_name, model_id, requested_data,
/// current_instance, actor_id, slug, origin` (`cycle.py:338-346`,
/// `:549-557`; `module.py:229-237`, `:439-447`). `requested_data` is the
/// raw `request.data` object and passes through verbatim; `actor_id` is
/// the wire-form (hyphenated-string) user id — kombu JSON-encodes the
/// UUID object Python passes; `current_instance` is `None` on create and
/// the pre-save serializer dump on update.
fn model_activity_kwargs(
    model_name: &str,
    model_id: &str,
    requested_data: &Map<String, Value>,
    current_instance: Option<&str>,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert(
        "model_name".to_owned(),
        Value::String(model_name.to_owned()),
    );
    kwargs.insert("model_id".to_owned(), Value::String(model_id.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::Object(requested_data.clone()),
    );
    kwargs.insert("current_instance".to_owned(), opt_string(current_instance));
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

/// Cycle create (`cycle.py:338-346`) and module create
/// (`module.py:229-237`): `current_instance=None`, fired after
/// `serializer.save()`; `model_id` is `str(serializer.instance.id)`.
pub fn model_created_message(
    model_name: &str,
    model_id: &str,
    requested_data: &Map<String, Value>,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        MODEL_ACTIVITY_TASK_NAME,
        Vec::new(),
        model_activity_kwargs(
            model_name,
            model_id,
            requested_data,
            None,
            actor_id,
            slug,
            origin,
        ),
    )
}

/// [`model_created_message`] as its queue row.
pub fn model_created_job(
    model_name: &str,
    model_id: &str,
    requested_data: &Map<String, Value>,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> NewJob {
    job_from_message(&model_created_message(
        model_name,
        model_id,
        requested_data,
        actor_id,
        slug,
        origin,
    ))
}

/// Cycle update (`cycle.py:549-557`) and module update
/// (`module.py:439-447`): `current_instance` is the PRE-save
/// `json.dumps(Serializer(row).data, cls=DjangoJSONEncoder)` snapshot
/// (`cycle.py:502`, `module.py:410`) — a before-image despite the key
/// name, ported as-is (the caller passes the snapshot it holds; this
/// module never re-reads the row).
pub fn model_updated_message(
    model_name: &str,
    model_id: &str,
    requested_data: &Map<String, Value>,
    current_instance: &str,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        MODEL_ACTIVITY_TASK_NAME,
        Vec::new(),
        model_activity_kwargs(
            model_name,
            model_id,
            requested_data,
            Some(current_instance),
            actor_id,
            slug,
            origin,
        ),
    )
}

/// [`model_updated_message`] as its queue row.
pub fn model_updated_job(
    model_name: &str,
    model_id: &str,
    requested_data: &Map<String, Value>,
    current_instance: &str,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> NewJob {
    job_from_message(&model_updated_message(
        model_name,
        model_id,
        requested_data,
        current_instance,
        actor_id,
        slug,
        origin,
    ))
}

// ---------------------------------------------------------------------------
// `issue_activity` (`issue_activities_task.py:1503`)
// ---------------------------------------------------------------------------

/// Shared kwarg builder for the seven `issue_activity.delay` sites, in the
/// exact Python call order — `type, requested_data, actor_id, issue_id,
/// project_id, current_instance, epoch`, then `notification` and `origin`
/// where the site passes them. `notification`/`origin` are omitted
/// entirely when `None` (Python's defaults `False`/`None` apply at the
/// worker); `epoch` is `int(timezone.now().timestamp())`, supplied by the
/// caller.
#[allow(clippy::too_many_arguments)]
fn issue_activity_kwargs(
    activity_type: &str,
    requested_data: &str,
    actor_id: &str,
    issue_id: Option<&str>,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
    notification: Option<bool>,
    origin: Option<&str>,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("issue_id".to_owned(), opt_string(issue_id));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_owned()),
    );
    kwargs.insert("current_instance".to_owned(), opt_string(current_instance));
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    if let Some(notification) = notification {
        kwargs.insert("notification".to_owned(), Value::Bool(notification));
    }
    if let Some(origin) = origin {
        kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    }
    kwargs
}

#[allow(clippy::too_many_arguments)]
fn issue_activity_message(
    activity_type: &str,
    requested_data: &str,
    actor_id: &str,
    issue_id: Option<&str>,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
    notification: Option<bool>,
    origin: Option<&str>,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        ISSUE_ACTIVITY_TASK,
        Vec::new(),
        issue_activity_kwargs(
            activity_type,
            requested_data,
            actor_id,
            issue_id,
            project_id,
            current_instance,
            epoch,
            notification,
            origin,
        ),
    )
}

/// `json.dumps({"<id_key>": id, "<name_key>": name, "issues": [...]})`
/// for the delete payloads (`cycle.py:596-602`, `module.py:514-520`):
/// all-string values, so [`django_dumps`] renders byte-identical output
/// (stdlib separators, ASCII escaping).
fn delete_requested_data(
    id_key: &str,
    id: &str,
    name_key: &str,
    name: &str,
    issues: &[&str],
) -> String {
    let mut fields = Map::with_capacity(3);
    fields.insert(id_key.to_owned(), Value::String(id.to_owned()));
    fields.insert(name_key.to_owned(), Value::String(name.to_owned()));
    fields.insert(
        "issues".to_owned(),
        Value::Array(
            issues
                .iter()
                .map(|issue| Value::String((*issue).to_owned()))
                .collect(),
        ),
    );
    django_dumps(&Value::Object(fields))
}

/// `json.dumps({"<id_key>": id, "issues": [issue]})` for the
/// issue-remove payloads (`cycle.py:1102-1107`, `module.py:881`).
fn remove_requested_data(id_key: &str, id: &str, issue_id: &str) -> String {
    let mut fields = Map::with_capacity(2);
    fields.insert(id_key.to_owned(), Value::String(id.to_owned()));
    fields.insert(
        "issues".to_owned(),
        Value::Array(vec![Value::String(issue_id.to_owned())]),
    );
    django_dumps(&Value::Object(fields))
}

/// `json.dumps({"module_name": name})` (`module.py:524, 885`): the delete
/// and remove sites both snapshot the module name as `current_instance`
/// (cycles send `None` instead — the asymmetry is ported as-is).
fn module_name_instance(name: &str) -> String {
    let mut fields = Map::with_capacity(1);
    fields.insert("module_name".to_owned(), Value::String(name.to_owned()));
    django_dumps(&Value::Object(fields))
}

/// Cycle delete (`cycle.py:594-608`): fired BEFORE `cycle.delete()`;
/// `issues` are the linked issue ids (`:592`); no `notification`, no
/// `origin`, `current_instance=None`.
pub fn cycle_deleted_message(
    cycle_id: &str,
    cycle_name: &str,
    issues: &[&str],
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> CeleryTaskMessage {
    let requested_data =
        delete_requested_data("cycle_id", cycle_id, "cycle_name", cycle_name, issues);
    issue_activity_message(
        CYCLE_DELETED,
        &requested_data,
        actor_id,
        None,
        project_id,
        None,
        epoch,
        None,
        None,
    )
}

/// [`cycle_deleted_message`] as its queue row.
pub fn cycle_deleted_job(
    cycle_id: &str,
    cycle_name: &str,
    issues: &[&str],
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> NewJob {
    job_from_message(&cycle_deleted_message(
        cycle_id, cycle_name, issues, actor_id, project_id, epoch,
    ))
}

/// Cycle issue add/move (`cycle.py:990-1005`): `requested_data` is the
/// `json.dumps({"cycles_list": issues})` text over the RAW request list
/// and `current_instance` the dump with the Django-serialized
/// `created_cycle_issues` string — both caller-rendered, passing through
/// verbatim. `notification=True`, `origin=base_host(app)`.
pub fn cycle_issue_added_message(
    requested_data: &str,
    actor_id: &str,
    project_id: &str,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> CeleryTaskMessage {
    issue_activity_message(
        CYCLE_CREATED,
        requested_data,
        actor_id,
        None,
        project_id,
        Some(current_instance),
        epoch,
        Some(true),
        Some(origin),
    )
}

/// [`cycle_issue_added_message`] as its queue row.
pub fn cycle_issue_added_job(
    requested_data: &str,
    actor_id: &str,
    project_id: &str,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> NewJob {
    job_from_message(&cycle_issue_added_message(
        requested_data,
        actor_id,
        project_id,
        current_instance,
        epoch,
        origin,
    ))
}

/// Cycle issue remove (`cycle.py:1100-1113`): fired AFTER
/// `cycle_issue.delete()`; the single issue id feeds both the
/// `requested_data` list and the `issue_id` kwarg (Python reassigns the
/// name from the row at `:1098`, a single source). No `notification`,
/// no `origin`, `current_instance=None`.
pub fn cycle_issue_removed_message(
    cycle_id: &str,
    issue_id: &str,
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> CeleryTaskMessage {
    let requested_data = remove_requested_data("cycle_id", cycle_id, issue_id);
    issue_activity_message(
        CYCLE_DELETED,
        &requested_data,
        actor_id,
        Some(issue_id),
        project_id,
        None,
        epoch,
        None,
        None,
    )
}

/// [`cycle_issue_removed_message`] as its queue row.
pub fn cycle_issue_removed_job(
    cycle_id: &str,
    issue_id: &str,
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> NewJob {
    job_from_message(&cycle_issue_removed_message(
        cycle_id, issue_id, actor_id, project_id, epoch,
    ))
}

/// Module delete (`module.py:512-527`): fired BEFORE `module.delete()`;
/// unlike the cycle delete it snapshots `current_instance={module_name}`
/// and passes `origin` — ported as-is.
pub fn module_deleted_message(
    module_id: &str,
    module_name: &str,
    issues: &[&str],
    actor_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> CeleryTaskMessage {
    let requested_data =
        delete_requested_data("module_id", module_id, "module_name", module_name, issues);
    let current_instance = module_name_instance(module_name);
    issue_activity_message(
        MODULE_DELETED,
        &requested_data,
        actor_id,
        None,
        project_id,
        Some(&current_instance),
        epoch,
        None,
        Some(origin),
    )
}

/// [`module_deleted_message`] as its queue row.
pub fn module_deleted_job(
    module_id: &str,
    module_name: &str,
    issues: &[&str],
    actor_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> NewJob {
    job_from_message(&module_deleted_message(
        module_id,
        module_name,
        issues,
        actor_id,
        project_id,
        epoch,
        origin,
    ))
}

/// Module issue add/move (`module.py:714-728`): `requested_data` is the
/// `json.dumps({"modules_list": str(issues)})` text — `str()` of the
/// re-queried values list, caller-rendered — and `current_instance` the
/// dump with the Django-serialized `created_module_issues` string. Has
/// `origin` but NO `notification` flag (unlike the cycle add) —
/// ported as-is.
pub fn module_issue_added_message(
    requested_data: &str,
    actor_id: &str,
    project_id: &str,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> CeleryTaskMessage {
    issue_activity_message(
        MODULE_CREATED,
        requested_data,
        actor_id,
        None,
        project_id,
        Some(current_instance),
        epoch,
        None,
        Some(origin),
    )
}

/// [`module_issue_added_message`] as its queue row.
pub fn module_issue_added_job(
    requested_data: &str,
    actor_id: &str,
    project_id: &str,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> NewJob {
    job_from_message(&module_issue_added_message(
        requested_data,
        actor_id,
        project_id,
        current_instance,
        epoch,
        origin,
    ))
}

/// Module issue remove (`module.py:879-887`): fired AFTER
/// `module_issue.delete()`; `current_instance={module_name}` (empty
/// string when the row's module is `None`, `:877` — the caller passes
/// what it read) and NO `origin`.
pub fn module_issue_removed_message(
    module_id: &str,
    module_name: &str,
    issue_id: &str,
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> CeleryTaskMessage {
    let requested_data = remove_requested_data("module_id", module_id, issue_id);
    let current_instance = module_name_instance(module_name);
    issue_activity_message(
        MODULE_DELETED,
        &requested_data,
        actor_id,
        Some(issue_id),
        project_id,
        Some(&current_instance),
        epoch,
        None,
        None,
    )
}

/// [`module_issue_removed_message`] as its queue row.
pub fn module_issue_removed_job(
    module_id: &str,
    module_name: &str,
    issue_id: &str,
    actor_id: &str,
    project_id: &str,
    epoch: i64,
) -> NewJob {
    job_from_message(&module_issue_removed_message(
        module_id,
        module_name,
        issue_id,
        actor_id,
        project_id,
        epoch,
    ))
}

/// One moved row for the transfer fan-out
/// (`cycle_transfer_issues.py:450-456`).
#[derive(Debug, Clone, PartialEq)]
pub struct CycleMove {
    pub old_cycle_id: String,
    pub new_cycle_id: String,
    pub issue_id: String,
}

impl CycleMove {
    pub fn new(old_cycle_id: &str, new_cycle_id: &str, issue_id: &str) -> Self {
        Self {
            old_cycle_id: old_cycle_id.to_owned(),
            new_cycle_id: new_cycle_id.to_owned(),
            issue_id: issue_id.to_owned(),
        }
    }
}

/// Transfer fan-out (`cycle_transfer_issues.py:462-477`):
/// `requested_data` is always `json.dumps({"cycles_list": []})` and
/// `current_instance` the dump of `{"updated_cycle_issues": [...],
/// "created_cycle_issues": []}` — note the bare empty LIST where the
/// add sites embed a Django-serialized string. `notification=True`,
/// `origin=base_host(app)`; `actor_id=str(user_id)` of the caller.
pub fn cycle_issues_transferred_message(
    actor_id: &str,
    project_id: &str,
    moves: &[CycleMove],
    epoch: i64,
    origin: &str,
) -> CeleryTaskMessage {
    let mut requested = Map::with_capacity(1);
    requested.insert("cycles_list".to_owned(), Value::Array(Vec::new()));
    let mut current = Map::with_capacity(2);
    current.insert(
        "updated_cycle_issues".to_owned(),
        Value::Array(
            moves
                .iter()
                .map(|m| {
                    let mut fields = Map::with_capacity(3);
                    fields.insert(
                        "old_cycle_id".to_owned(),
                        Value::String(m.old_cycle_id.clone()),
                    );
                    fields.insert(
                        "new_cycle_id".to_owned(),
                        Value::String(m.new_cycle_id.clone()),
                    );
                    fields.insert("issue_id".to_owned(), Value::String(m.issue_id.clone()));
                    Value::Object(fields)
                })
                .collect(),
        ),
    );
    current.insert("created_cycle_issues".to_owned(), Value::Array(Vec::new()));
    issue_activity_message(
        CYCLE_CREATED,
        &django_dumps(&Value::Object(requested)),
        actor_id,
        None,
        project_id,
        Some(&django_dumps(&Value::Object(current))),
        epoch,
        Some(true),
        Some(origin),
    )
}

/// [`cycle_issues_transferred_message`] as its queue row.
pub fn cycle_issues_transferred_job(
    actor_id: &str,
    project_id: &str,
    moves: &[CycleMove],
    epoch: i64,
    origin: &str,
) -> NewJob {
    job_from_message(&cycle_issues_transferred_message(
        actor_id, project_id, moves, epoch, origin,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{DEFAULT_MAX_RETRIES, DEFAULT_QUEUE};
    use crate::tasks_webhooks::activity_dispatch::bind_issue_activity;
    use crate::tasks_webhooks::webhook_fanout::bind_model_activity;
    use serde_json::json;

    /// Committed oracle evidence:
    /// `rust-api/fixtures/v1_cycles_modules/tasks/enqueue.golden.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/v1_cycles_modules/tasks/enqueue.golden.json");

    /// Mirror of `contract-tests/_harness/celery_wire.py::assert_wire_message`
    /// (same helper as `v1_assets::tasks::tests::assert_wire`): the nine
    /// protocol-v2 header keys are present, `task`/`lang` match, and the
    /// body carries exactly the published args/kwargs.
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
        assert_eq!(body[0].clone(), args);
        assert_eq!(body[1].as_object().expect("kwargs object"), kwargs);
    }

    fn kwargs_keys(message: &CeleryTaskMessage) -> Vec<String> {
        message.kwargs.keys().cloned().collect()
    }

    /// Bind a published issue message the way the D-08 dispatcher binds it
    /// (task-oracle style: the wire this module publishes must bind cleanly
    /// where the worker runs it).
    fn bind_issue(
        message: &CeleryTaskMessage,
    ) -> crate::tasks_webhooks::activity_dispatch::IssueActivityCall {
        bind_issue_activity(
            &Value::Array(message.args.clone()),
            &Value::Object(message.kwargs.clone()),
        )
        .expect("published issue payload binds")
    }

    /// Bind a published model message the way the D-08 fan-out binds it.
    fn bind_model(
        message: &CeleryTaskMessage,
    ) -> crate::tasks_webhooks::webhook_fanout::ModelActivityCall {
        bind_model_activity(
            &Value::Array(message.args.clone()),
            &Value::Object(message.kwargs.clone()),
        )
        .expect("published model payload binds")
    }

    fn sample_data() -> Map<String, Value> {
        json!({"name": "Sprint 1", "owned_by": "u1"})
            .as_object()
            .expect("object")
            .clone()
    }

    // FX-CYCMOD-07 · task names are the bare `@shared_task` dotted paths.
    #[test]
    fn task_names_match_python() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK_NAME,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        assert_eq!(fx["fixture"], json!("FX-CYCMOD-07"));
        let kinds: Vec<&str> = ["cycle_views", "module_views", "transfer_util"]
            .iter()
            .flat_map(|group| {
                fx["extracted_call_sites"][group]
                    .as_array()
                    .expect("group array")
                    .iter()
                    .map(|site| site["task"].as_str().expect("task kind"))
            })
            .collect();
        assert_eq!(kinds.len(), 11);
        assert!(kinds
            .iter()
            .all(|k| *k == "issue_activity" || *k == "model_activity"));
    }

    // FX-CYCMOD-07 · activity type strings.
    #[test]
    fn activity_types_match_fixture() {
        assert_eq!(CYCLE_CREATED, "cycle.activity.created");
        assert_eq!(CYCLE_DELETED, "cycle.activity.deleted");
        assert_eq!(MODULE_CREATED, "module.activity.created");
        assert_eq!(MODULE_DELETED, "module.activity.deleted");
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let body = serde_json::to_string(&fx).expect("reserialize");
        for needle in [CYCLE_CREATED, CYCLE_DELETED, MODULE_CREATED, MODULE_DELETED] {
            assert!(body.contains(needle), "fixture mentions {needle}");
        }
    }

    // FX-CYCMOD-07 trace · every call site this module ports is listed.
    #[test]
    fn fixture_trace_covers_all_sites() {
        let fx: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let trace = fx["trace"].as_array().expect("trace array");
        let body: String = trace
            .iter()
            .map(|line| line.as_str().expect("trace str").to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "cycle.py:338-346",
            ":549-557 (update)",
            "module.py:229-237",
            ":439-447 (update)",
            "cycle.py:594-608",
            ":987-1002 (issue add/move",
            ":1086-1114 (issue remove",
            "module.py:512-527",
            ":714-728 (issue add/move)",
            ":879-887 (issue remove)",
            "cycle_transfer_issues.py:462-478",
        ] {
            assert!(body.contains(needle), "trace covers {needle}");
        }
    }

    // Model create (cycle.py:338 / module.py:229) · kwarg order + nulls + wire.
    #[test]
    fn model_created_kwargs_order_and_wire() {
        let data = sample_data();
        for model in ["cycle", "module"] {
            let message = model_created_message(model, "m1", &data, "a1", "ws", "https://app");
            assert_eq!(
                kwargs_keys(&message),
                [
                    "model_name",
                    "model_id",
                    "requested_data",
                    "current_instance",
                    "actor_id",
                    "slug",
                    "origin"
                ]
            );
            let kwargs = message.kwargs.clone();
            assert_wire(&message, MODEL_ACTIVITY_TASK_NAME, json!([]), &kwargs);
            assert_eq!(kwargs["requested_data"], Value::Object(data.clone()));
            assert_eq!(kwargs["current_instance"], Value::Null);
            let bound = bind_model(&message);
            assert_eq!(bound.model_name, model);
            assert_eq!(bound.model_id, "m1");
            assert_eq!(bound.requested_data, data);
            assert_eq!(bound.current_instance, None);
            assert_eq!(bound.actor_id, "a1");
            assert_eq!(bound.slug, "ws");
            assert_eq!(bound.origin.as_deref(), Some("https://app"));
            let job = model_created_job(model, "m1", &data, "a1", "ws", "https://app");
            assert_eq!(job.task, MODEL_ACTIVITY_TASK_NAME);
            assert_eq!(job.args, json!([]));
            assert_eq!(job.kwargs, Value::Object(kwargs));
            assert_eq!(job.max_retries, DEFAULT_MAX_RETRIES);
            assert_eq!(job.max_retries, 3);
            assert_eq!(job.queue, DEFAULT_QUEUE);
        }
    }

    // Model update (cycle.py:549 / module.py:439) · current_instance passes through.
    #[test]
    fn model_updated_carries_snapshot() {
        let data = sample_data();
        let snapshot = r#"{"name": "Old"}"#;
        let message =
            model_updated_message("cycle", "m1", &data, snapshot, "a1", "ws", "https://app");
        assert_eq!(
            kwargs_keys(&message),
            [
                "model_name",
                "model_id",
                "requested_data",
                "current_instance",
                "actor_id",
                "slug",
                "origin"
            ]
        );
        assert_eq!(
            message.kwargs["current_instance"],
            Value::String(snapshot.to_owned())
        );
        let bound = bind_model(&message);
        assert_eq!(bound.current_instance.as_deref(), Some(snapshot));
        let job = module_updated_job_helper();
        assert_eq!(job.task, MODEL_ACTIVITY_TASK_NAME);
        assert_eq!(
            job.kwargs["current_instance"],
            Value::String(snapshot.to_owned())
        );
    }

    fn module_updated_job_helper() -> NewJob {
        model_updated_job(
            "module",
            "m9",
            &sample_data(),
            r#"{"name": "Old"}"#,
            "a1",
            "ws",
            "https://app",
        )
    }

    // Cycle delete (cycle.py:594) · rendered payload, base-seven kwargs, no extras.
    #[test]
    fn cycle_deleted_payload_and_keys() {
        let message =
            cycle_deleted_message("c1", "Sprint 1", &["i1", "i2"], "a1", "p1", 1_700_000_000);
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch"
            ]
        );
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], Value::String(CYCLE_DELETED.to_owned()));
        // Python `json.dumps` separators (", " / ": ") — byte-identical.
        assert_eq!(
            kwargs["requested_data"],
            json!("{\"cycle_id\": \"c1\", \"cycle_name\": \"Sprint 1\", \"issues\": [\"i1\", \"i2\"]}")
        );
        assert_eq!(kwargs["issue_id"], Value::Null);
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["epoch"], json!(1_700_000_000));
        assert!(!kwargs.contains_key("notification"));
        assert!(!kwargs.contains_key("origin"));
        let bound = bind_issue(&message);
        assert_eq!(bound.activity_type, CYCLE_DELETED);
        assert!(!bound.notification);
        assert_eq!(bound.origin, None);
        assert!(bound.subscriber);
        let job = cycle_deleted_job("c1", "Sprint 1", &["i1", "i2"], "a1", "p1", 1_700_000_000);
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs, Value::Object(kwargs));
        assert_eq!(job.max_retries, 3);
    }

    // Cycle issue add (cycle.py:990) · pass-through payloads, notification + origin.
    #[test]
    fn cycle_issue_added_passthrough_and_flags() {
        let requested = r#"{"cycles_list": ["i1"]}"#;
        let current = r#"{"updated_cycle_issues": [], "created_cycle_issues": "[{...}]"}"#;
        let message = cycle_issue_added_message(requested, "a1", "p1", current, 42, "https://app");
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
                "origin"
            ]
        );
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], Value::String(CYCLE_CREATED.to_owned()));
        assert_eq!(
            kwargs["requested_data"],
            Value::String(requested.to_owned())
        );
        assert_eq!(
            kwargs["current_instance"],
            Value::String(current.to_owned())
        );
        assert_eq!(kwargs["notification"], Value::Bool(true));
        assert_eq!(kwargs["origin"], Value::String("https://app".to_owned()));
        let bound = bind_issue(&message);
        assert!(bound.notification);
        assert_eq!(bound.origin.as_deref(), Some("https://app"));
        let job = cycle_issue_added_job(requested, "a1", "p1", current, 42, "https://app");
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.kwargs, Value::Object(kwargs));
    }

    // Cycle issue remove (cycle.py:1100) · single-issue payload, base-seven kwargs.
    #[test]
    fn cycle_issue_removed_payload_and_keys() {
        let message = cycle_issue_removed_message("c1", "i9", "a1", "p1", 7);
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch"
            ]
        );
        assert_eq!(
            message.kwargs["requested_data"],
            json!("{\"cycle_id\": \"c1\", \"issues\": [\"i9\"]}")
        );
        assert_eq!(message.kwargs["issue_id"], json!("i9"));
        assert_eq!(message.kwargs["current_instance"], Value::Null);
        assert!(!message.kwargs.contains_key("notification"));
        assert!(!message.kwargs.contains_key("origin"));
        let bound = bind_issue(&message);
        assert_eq!(bound.activity_type, CYCLE_DELETED);
        assert_eq!(bound.issue_id.as_deref(), Some("i9"));
        let job = cycle_issue_removed_job("c1", "i9", "a1", "p1", 7);
        assert_eq!(job.kwargs, Value::Object(message.kwargs.clone()));
    }

    // Module delete (module.py:512) · name snapshot + origin (cycle has neither).
    #[test]
    fn module_deleted_payload_and_keys() {
        let message = module_deleted_message(
            "m1",
            "Auth",
            &["i1"],
            "a1",
            "p1",
            1_700_000_000,
            "https://app",
        );
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "origin"
            ]
        );
        assert_eq!(
            message.kwargs["requested_data"],
            json!("{\"module_id\": \"m1\", \"module_name\": \"Auth\", \"issues\": [\"i1\"]}")
        );
        assert_eq!(
            message.kwargs["current_instance"],
            json!("{\"module_name\": \"Auth\"}")
        );
        assert_eq!(message.kwargs["origin"], json!("https://app"));
        assert!(!message.kwargs.contains_key("notification"));
        let bound = bind_issue(&message);
        assert_eq!(bound.activity_type, MODULE_DELETED);
        assert_eq!(bound.origin.as_deref(), Some("https://app"));
        let job = module_deleted_job(
            "m1",
            "Auth",
            &["i1"],
            "a1",
            "p1",
            1_700_000_000,
            "https://app",
        );
        assert_eq!(job.kwargs, Value::Object(message.kwargs.clone()));
    }

    // Module issue add (module.py:714) · origin but NO notification.
    #[test]
    fn module_issue_added_has_origin_without_notification() {
        let requested = r#"{"modules_list": "[<uuids>]"}"#;
        let current = r#"{"updated_module_issues": [], "created_module_issues": "[{...}]"}"#;
        let message = module_issue_added_message(requested, "a1", "p1", current, 42, "https://app");
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "origin"
            ]
        );
        assert_eq!(
            message.kwargs["type"],
            Value::String(MODULE_CREATED.to_owned())
        );
        assert_eq!(
            message.kwargs["requested_data"],
            Value::String(requested.to_owned())
        );
        assert!(!message.kwargs.contains_key("notification"));
        let bound = bind_issue(&message);
        assert!(!bound.notification);
        let job = module_issue_added_job(requested, "a1", "p1", current, 42, "https://app");
        assert_eq!(job.kwargs, Value::Object(message.kwargs.clone()));
    }

    // Module issue remove (module.py:879) · name snapshot, no origin.
    #[test]
    fn module_issue_removed_payload_and_keys() {
        let message = module_issue_removed_message("m1", "Auth", "i9", "a1", "p1", 7);
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch"
            ]
        );
        assert_eq!(
            message.kwargs["requested_data"],
            json!("{\"module_id\": \"m1\", \"issues\": [\"i9\"]}")
        );
        assert_eq!(message.kwargs["issue_id"], json!("i9"));
        assert_eq!(
            message.kwargs["current_instance"],
            json!("{\"module_name\": \"Auth\"}")
        );
        assert!(!message.kwargs.contains_key("origin"));
        let bound = bind_issue(&message);
        assert_eq!(bound.issue_id.as_deref(), Some("i9"));
        let job = module_issue_removed_job("m1", "Auth", "i9", "a1", "p1", 7);
        assert_eq!(job.kwargs, Value::Object(message.kwargs.clone()));
    }

    // Rendered payloads vs the CPython oracle (`python3 /tmp/verify_dumps.py`,
    // same dict literals): quotes, backslash, non-ASCII and control chars
    // escape exactly like `json.dumps` with `ensure_ascii=True`.
    #[test]
    fn rendered_payloads_match_cpython_escaping() {
        let message = cycle_deleted_message("c1", "A\"B\\Cé\t\x01", &[], "a1", "p1", 1);
        assert_eq!(
            message.kwargs["requested_data"],
            json!("{\"cycle_id\": \"c1\", \"cycle_name\": \"A\\\"B\\\\C\\u00e9\\t\\u0001\", \"issues\": []}")
        );
        let message = module_issue_removed_message("m1", "café \"x\"", "i9", "a1", "p1", 1);
        assert_eq!(
            message.kwargs["current_instance"],
            json!("{\"module_name\": \"caf\\u00e9 \\\"x\\\"\"}")
        );
    }

    // Transfer (cycle_transfer_issues.py:462) · rendered payloads, created=[] list.
    #[test]
    fn transfer_payload_renders_both_dumps() {
        let moves = vec![
            CycleMove::new("c-old", "c-new", "i1"),
            CycleMove::new("c-old", "c-new", "i2"),
        ];
        let message = cycle_issues_transferred_message("a1", "p1", &moves, 99, "https://app");
        assert_eq!(
            kwargs_keys(&message),
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
                "origin"
            ]
        );
        assert_eq!(
            message.kwargs["type"],
            Value::String(CYCLE_CREATED.to_owned())
        );
        assert_eq!(
            message.kwargs["requested_data"],
            json!("{\"cycles_list\": []}")
        );
        assert_eq!(
            message.kwargs["current_instance"],
            json!(
                "{\"updated_cycle_issues\": [{\"old_cycle_id\": \"c-old\", \"new_cycle_id\": \"c-new\", \"issue_id\": \"i1\"}, {\"old_cycle_id\": \"c-old\", \"new_cycle_id\": \"c-new\", \"issue_id\": \"i2\"}], \"created_cycle_issues\": []}"
            )
        );
        assert_eq!(message.kwargs["notification"], Value::Bool(true));
        let bound = bind_issue(&message);
        assert!(bound.notification);
        // Empty transfer still renders the same shapes.
        let empty = cycle_issues_transferred_message("a1", "p1", &[], 99, "https://app");
        assert_eq!(
            empty.kwargs["current_instance"],
            json!("{\"updated_cycle_issues\": [], \"created_cycle_issues\": []}")
        );
        let job = cycle_issues_transferred_job("a1", "p1", &moves, 99, "https://app");
        assert_eq!(job.kwargs, Value::Object(message.kwargs.clone()));
    }
}
