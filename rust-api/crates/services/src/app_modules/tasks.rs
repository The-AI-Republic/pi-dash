//! App-modules enqueue closure: `issue_activity` + `model_activity` +
//! `recent_visited_task` publishers (D-28, stage 5).
//!
//! Port of the eight `.delay()` call sites in
//! `apps/api/pi_dash/app/views/module/base.py` and `issue.py`
//! (PIDASHCONV-384):
//!
//! * `ModuleViewSet.create`, `model_activity.delay(...)` (`base.py:339-347`):
//!   `model_id=str(module id)`, `current_instance=None`.
//! * `ModuleViewSet.retrieve`, `recent_visited_task.delay(...)`
//!   (`base.py:641-647`): visit upsert, deferred (see NO-OP note below).
//! * `ModuleViewSet.partial_update`, `model_activity.delay(...)`
//!   (`base.py:708-716`): `current_instance` is the
//!   `json.dumps(ModuleSerializer(current_module).data)` snapshot taken
//!   BEFORE the save (`:668`). Member add/remove rides this emit: members
//!   are replaced inside `serializer.save()` and the new `member_ids`
//!   travel in `requested_data`; there is no separate member task.
//! * `ModuleViewSet.destroy`, `issue_activity.delay(...)` per linked issue
//!   (`base.py:728-741`): `type="module.activity.deleted"`,
//!   `current_instance={"module_name": str(module.name)}`.
//! * `ModuleIssueViewSet.create_module_issues`, `issue_activity.delay(...)`
//!   per issue (`issue.py:232-245`): `type="module.activity.created"`,
//!   `requested_data={"module_id": str(module_id)}`, `current_instance=None`.
//! * `ModuleIssueViewSet.create_issue_modules`, `issue_activity.delay(...)`
//!   per added module (`issue.py:272-285`): same shape, except
//!   `requested_data={"module_id": module}` uses the RAW loop value.
//! * `ModuleIssueViewSet.create_issue_modules`, `issue_activity.delay(...)`
//!   per removed module (`issue.py:294-312`): `type="module.activity.deleted"`
//!   with a None-safe `current_instance` (`module_name` is `None` when the
//!   link row is already gone).
//! * `ModuleIssueViewSet.destroy`, `issue_activity.delay(...)`
//!   (`issue.py:325-335`): `type="module.activity.deleted"` with the
//!   None-UNSAFE `module_issue.first().module.name` lookup.
//!
//! Fixture oracle (PIDASHCONV-294):
//! `rust-api/fixtures/app_modules/tasks/enqueue_payloads.golden.json`
//! (FX-MOD-05; `_trace` →
//! `base.py:339-347,641-647,708-716,728-741` +
//! `issue.py:232-245,272-285,294-313,325-335`). The tests below replay
//! every `payload` / `payload_per_issue` / `payload_added` /
//! `payload_removed` field for field, in call-site kwarg order.
//!
//! Wire contract (Porting guide Jobs plane row): `.delay(**kwargs)`
//! publishes a first-attempt Celery protocol v2 message with `args = []`
//! and kwargs in call-site order. All three tasks are bare `@shared_task`
//! (`issue_activities_task.py:1503`, `webhook_task.py:463`,
//! `recent_visited_task.py:17`), so the wire names are the dotted module
//! paths in [`ISSUE_ACTIVITY_TASK`] / [`MODEL_ACTIVITY_TASK`] /
//! [`RECENT_VISITED_TASK`] and there is no queue override.
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! task name + [`ModelActivityEmit::kwargs`] /
//! [`ModuleIssueActivityEmit::kwargs`] / [`RecentVisitedEmit::kwargs`] —
//! and the handlers (PIDASHCONV-391, in the `api` crate which already
//! depends on `pidash-jobs`) wrap them with
//! `CeleryTaskMessage::new(task, vec![], kwargs)` plus `queue::enqueue`
//! (the D-02 `space::intake` precedent), transactionally post-commit.
//! The dump *text* (`requested_data`, `current_instance`, `origin`) is the
//! caller's job: `json.dumps(..., cls=DjangoJSONEncoder)` in Python, i.e.
//! CPython `", "` / `": "` separators (the `api` crate's `python_dumps`
//! owns that rendering). No worker `Registry` handler is registered for
//! any of these names: all three are Python-owned, so the worker forwards
//! them to RabbitMQ in Celery protocol v2.
//!
//! `recent_visited_task` NO-OP note (pilot-2 pattern,
//! `api/src/app_issues/mod.rs` `record_recent_visit`): in every
//! environment the contract suite runs (memory broker, no worker), Django
//! writes no `user_recent_visits` row from `.delay()`; the call site is
//! kept but performs no inline write (an inline upsert broke the gate's
//! teardown with a `ForeignKeyViolation` Django never produces).
//! [`RecentVisitedEmit`] pins the kwargs the deferred publish carries so
//! the handler-side call site (PIDASHCONV-391) fires the exact message.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-int-actor (`base.py:344,713` vs every `issue_activity` site):
//!   `model_activity` passes `actor_id=request.user.id` as the RAW object
//!   while `issue_activity` wraps `str(request.user.id)`. Kombu JSON
//!   renders both to the same string on the wire; the builders take the
//!   string form (the D-33 Q6 convention).
//! * QUIRK-raw-project (`base.py:734`, `issue.py:238,278` vs `:299,330`):
//!   destroy, link-create and link-add pass `project_id` as the RAW kwarg;
//!   link-remove and link-destroy wrap `str(project_id)`. Wire-identical;
//!   the builders take the string form and the call table names the side.
//! * QUIRK-raw-module-ref (`issue.py:275` vs `:235,296,327`): the link-add
//!   `requested_data` dumps `{"module_id": module}` with the RAW loop
//!   value, NOT `str()`-wrapped. [`module_ref_raw`] vs
//!   [`module_ref_str`] pin the two shapes at the `Value` level.
//! * QUIRK-unsafe-first (`issue.py:331` vs `:302-306`): link-destroy reads
//!   `module_issue.first().module.name` with no guard — an already-gone
//!   link row raises `AttributeError` (recorded 500 risk). Link-remove is
//!   None-safe (`module_name` is `None` when the row is missing). The
//!   types encode this: [`link_destroy_instance`] takes `&str` (caller
//!   guarantees presence), [`link_removed_instance`] takes `Option<&str>`.
//! * Module-destroy `current_instance` (`base.py:735`) uses
//!   `str(module.name)` from the `.get()`-loaded row, so it cannot hit
//!   the QUIRK-unsafe-first path (a missing module raises before any
//!   enqueue).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

/// Celery wire name for `issue_activity` (bare `@shared_task` default).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Celery wire name for `model_activity` (bare `@shared_task` default;
/// pinned equal to the D-19/D-33 const — same string, this crate cannot
/// import `pidash-jobs` so the pin is by value on both sides).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";

/// Celery wire name for `recent_visited_task` (bare `@shared_task` default).
pub const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";

/// `model_name` for both D-28 `model_activity` sites
/// (`base.py:340,709`).
pub const MODULE_MODEL_NAME: &str = "module";

/// Activity `type` strings for the five `issue_activity` sites.
pub const MODULE_ACTIVITY_CREATED: &str = "module.activity.created";
pub const MODULE_ACTIVITY_DELETED: &str = "module.activity.deleted";

/// `entity_name` for the retrieve visit emit (`base.py:643`).
pub const VISIT_ENTITY_MODULE: &str = "module";

/// Kwarg order of both `model_activity.delay` calls
/// (`base.py:339-347`, `:708-716`).
pub const MODEL_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "model_name",
    "model_id",
    "requested_data",
    "current_instance",
    "actor_id",
    "slug",
    "origin",
];

/// Kwarg order of all five `issue_activity.delay` calls
/// (`base.py:728-741`, `issue.py:232-245,272-285,294-312,325-335` —
/// identical order at every site; `notification` is always `True`).
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
];

/// Kwarg order of `recent_visited_task.delay` (`base.py:641-647`).
pub const RECENT_VISITED_KWARG_ORDER: &[&str] = &[
    "slug",
    "entity_name",
    "entity_identifier",
    "user_id",
    "project_id",
];

/// One `model_activity.delay(...)` enqueue from the module create/update
/// call sites (`base.py:339-347`, `:708-716`).
///
/// Kwargs in call order: `model_name`, `model_id`, `requested_data`,
/// `current_instance`, `actor_id`, `slug`, `origin`. `requested_data` is
/// the raw `request.data` object; `current_instance` is `None` on create
/// and the `json.dumps(ModuleSerializer(current_module).data)` snapshot
/// string on update. Values pass through verbatim (QUIRK-int-actor: the
/// wire form of `actor_id` is the string form either way).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelActivityEmit {
    pub model_name: String,
    pub model_id: String,
    pub requested_data: Value,
    pub current_instance: Option<String>,
    pub actor_id: String,
    pub slug: String,
    pub origin: String,
}

impl ModelActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        MODEL_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(MODEL_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert(
            "model_name".to_owned(),
            Value::String(self.model_name.clone()),
        );
        kwargs.insert("model_id".to_owned(), Value::String(self.model_id.clone()));
        kwargs.insert("requested_data".to_owned(), self.requested_data.clone());
        kwargs.insert(
            "current_instance".to_owned(),
            self.current_instance
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert("origin".to_owned(), Value::String(self.origin.clone()));
        kwargs
    }
}

/// Module-create publisher (`base.py:339-347`): `model_name="module"`,
/// `model_id=str(module id)`, `current_instance=None`.
pub fn module_create_activity(
    model_id: &str,
    requested_data: Value,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> ModelActivityEmit {
    ModelActivityEmit {
        model_name: MODULE_MODEL_NAME.to_owned(),
        model_id: model_id.to_owned(),
        requested_data,
        current_instance: None,
        actor_id: actor_id.to_owned(),
        slug: slug.to_owned(),
        origin: origin.to_owned(),
    }
}

/// Module-update publisher (`base.py:708-716`): `current_instance` is the
/// before-save snapshot string (`base.py:668`).
#[allow(clippy::too_many_arguments)]
pub fn module_update_activity(
    model_id: &str,
    requested_data: Value,
    snapshot_json: &str,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> ModelActivityEmit {
    ModelActivityEmit {
        model_name: MODULE_MODEL_NAME.to_owned(),
        model_id: model_id.to_owned(),
        requested_data,
        current_instance: Some(snapshot_json.to_owned()),
        actor_id: actor_id.to_owned(),
        slug: slug.to_owned(),
        origin: origin.to_owned(),
    }
}

/// One `issue_activity.delay(...)` enqueue from the five module
/// `module.activity.*` call sites.
///
/// Kwargs in call order: `type`, `requested_data`, `actor_id`, `issue_id`,
/// `project_id`, `current_instance`, `epoch`, `notification`, `origin`.
/// `requested_data` / `current_instance` are the pre-rendered
/// `json.dumps` texts; `epoch` is `int(timezone.now().timestamp())`
/// supplied by the caller; `notification` is always `true` in this
/// domain. QUIRK-raw-project: `project_id` crosses as the string form
/// whether the site passes it raw or `str()`-wrapped.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleIssueActivityEmit {
    pub activity_type: String,
    pub requested_data: String,
    pub actor_id: String,
    pub issue_id: String,
    pub project_id: String,
    pub current_instance: Option<String>,
    pub epoch: i64,
    pub notification: bool,
    pub origin: String,
}

impl ModuleIssueActivityEmit {
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
        kwargs
    }
}

/// Shared constructor for the five `module.activity.*` emits; each
/// call-site wrapper below fixes `activity_type` and documents which
/// `project_id` side (QUIRK-raw-project) it is on.
#[allow(clippy::too_many_arguments)]
fn module_issue_activity(
    activity_type: &str,
    requested_data: String,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    current_instance: Option<String>,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    ModuleIssueActivityEmit {
        activity_type: activity_type.to_owned(),
        requested_data,
        actor_id: actor_id.to_owned(),
        issue_id: issue_id.to_owned(),
        project_id: project_id.to_owned(),
        current_instance,
        epoch,
        notification: true,
        origin: origin.to_owned(),
    }
}

/// Module-destroy publisher, one per linked issue (`base.py:728-741`):
/// `type="module.activity.deleted"`,
/// `requested_data={"module_id": str(pk)}`,
/// `current_instance={"module_name": str(module.name)}`,
/// `project_id` RAW (QUIRK-raw-project).
pub fn module_destroy_activity(
    module_id: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    module_name_json: &str,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    module_issue_activity(
        MODULE_ACTIVITY_DELETED,
        module_id_ref_json(module_id),
        actor_id,
        issue_id,
        project_id,
        Some(module_name_json.to_owned()),
        epoch,
        origin,
    )
}

/// Link-create publisher, one per issue (`issue.py:232-245`):
/// `type="module.activity.created"`,
/// `requested_data={"module_id": str(module_id)}`,
/// `current_instance=None`, `project_id` RAW (QUIRK-raw-project).
pub fn link_create_activity(
    module_id: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    module_issue_activity(
        MODULE_ACTIVITY_CREATED,
        module_id_ref_json(module_id),
        actor_id,
        issue_id,
        project_id,
        None,
        epoch,
        origin,
    )
}

/// Link-add publisher, one per added module (`issue.py:272-285`):
/// `type="module.activity.created"`, `current_instance=None`,
/// `project_id` RAW (QUIRK-raw-project) — but `requested_data` dumps the
/// RAW loop value (QUIRK-raw-module-ref), so the caller renders it with
/// [`module_ref_raw_json`] instead of [`module_id_ref_json`].
pub fn link_add_activity(
    requested_data: String,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    module_issue_activity(
        MODULE_ACTIVITY_CREATED,
        requested_data,
        actor_id,
        issue_id,
        project_id,
        None,
        epoch,
        origin,
    )
}

/// Link-remove publisher, one per removed module (`issue.py:294-312`):
/// `type="module.activity.deleted"`,
/// `requested_data={"module_id": str(module_id)}`,
/// `project_id` `str()`-wrapped (QUIRK-raw-project), `current_instance`
/// None-safe (QUIRK-unsafe-first: `module_name` is `None` when the link
/// row is missing).
pub fn link_remove_activity(
    module_id: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    module_name: Option<&str>,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    module_issue_activity(
        MODULE_ACTIVITY_DELETED,
        module_id_ref_json(module_id),
        actor_id,
        issue_id,
        project_id,
        Some(link_removed_instance(module_name)),
        epoch,
        origin,
    )
}

/// Link-destroy publisher (`issue.py:325-335`):
/// `type="module.activity.deleted"`,
/// `requested_data={"module_id": str(module_id)}`,
/// `project_id` `str()`-wrapped (QUIRK-raw-project), `current_instance`
/// None-UNSAFE (QUIRK-unsafe-first — the caller guarantees the row is
/// present; a gone row is the recorded 500).
pub fn link_destroy_activity(
    module_id: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    module_name: &str,
    epoch: i64,
    origin: &str,
) -> ModuleIssueActivityEmit {
    module_issue_activity(
        MODULE_ACTIVITY_DELETED,
        module_id_ref_json(module_id),
        actor_id,
        issue_id,
        project_id,
        Some(link_destroy_instance(module_name)),
        epoch,
        origin,
    )
}

/// `{"module_id": str(...)}` ref dump for every site EXCEPT link-add
/// (`base.py:731`, `issue.py:235,296,327`).
pub fn module_id_ref_json(module_id: &str) -> String {
    module_ref_value_json(&Value::String(module_id.to_owned()))
}

/// QUIRK-raw-module-ref (`issue.py:275`): the link-add
/// `requested_data` dumps `{"module_id": module}` with the RAW loop
/// value — a non-string JSON value passes through unwrapped, so
/// `{"module_id": 42}` stays `{"module_id": 42}` instead of becoming
/// `{"module_id": "42"}`.
pub fn module_ref_raw_json(module: &Value) -> String {
    module_ref_value_json(module)
}

/// Shared `{"module_id": ...}` renderer behind [`module_id_ref_json`]
/// (str-wrapped side) and [`module_ref_raw_json`] (raw side).
/// Separator rendering is the caller's job (`python_dumps` in the `api`
/// crate); compact form here is only for shape, never shipped.
fn module_ref_value_json(module: &Value) -> String {
    format!("{{\"module_id\": {module}}}")
}

/// `{"module_name": ...}` dump for the link-remove path
/// (`issue.py:300-308`): None-safe — a missing link row renders
/// `{"module_name": null}`.
pub fn link_removed_instance(module_name: Option<&str>) -> String {
    match module_name {
        Some(name) => format!("{{\"module_name\": {}}}", Value::String(name.to_owned())),
        None => "{\"module_name\": null}".to_owned(),
    }
}

/// `{"module_name": ...}` dump for the link-destroy path
/// (`issue.py:331`): None-UNSAFE by construction — takes `&str`, so a
/// gone row cannot be rendered and the caller must reproduce the Python
/// `AttributeError` (recorded 500) instead of inventing a null.
pub fn link_destroy_instance(module_name: &str) -> String {
    format!(
        "{{\"module_name\": {}}}",
        Value::String(module_name.to_owned())
    )
}

/// One `recent_visited_task.delay(...)` enqueue from the retrieve call
/// site (`base.py:641-647`).
///
/// Kwargs in call order: `slug`, `entity_name`, `entity_identifier`,
/// `user_id`, `project_id`. `entity_name` is always `"module"`;
/// `entity_identifier` is the raw `pk` kwarg (string form on the wire).
/// Deferred publish only (NO-OP note above): handlers keep the call site
/// but perform no inline write.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentVisitedEmit {
    pub slug: String,
    pub entity_name: String,
    pub entity_identifier: String,
    pub user_id: String,
    pub project_id: String,
}

impl RecentVisitedEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        RECENT_VISITED_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(RECENT_VISITED_KWARG_ORDER.len());
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert(
            "entity_name".to_owned(),
            Value::String(self.entity_name.clone()),
        );
        kwargs.insert(
            "entity_identifier".to_owned(),
            Value::String(self.entity_identifier.clone()),
        );
        kwargs.insert("user_id".to_owned(), Value::String(self.user_id.clone()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs
    }
}

/// Retrieve publisher (`base.py:641-647`): `entity_name="module"`,
/// `entity_identifier` the raw `pk`, `user_id` the raw
/// `request.user.id` (QUIRK-int-actor — string form on the wire).
pub fn retrieve_visit_emit(
    slug: &str,
    entity_identifier: &str,
    user_id: &str,
    project_id: &str,
) -> RecentVisitedEmit {
    RecentVisitedEmit {
        slug: slug.to_owned(),
        entity_name: VISIT_ENTITY_MODULE.to_owned(),
        entity_identifier: entity_identifier.to_owned(),
        user_id: user_id.to_owned(),
        project_id: project_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/app_modules/tasks/enqueue_payloads.golden.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_modules/tasks/enqueue_payloads.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn key_order(kwargs: &Map<String, Value>) -> Vec<&str> {
        kwargs.keys().map(String::as_str).collect()
    }

    #[test]
    fn fixture_names_the_python_sources() {
        let f = fixture();
        let trace = f["trace"].as_str().expect("trace line");
        for fragment in [
            "app/views/module/base.py:339-347,641-647,708-716,728-741",
            "app/views/module/issue.py:232-245,272-285,294-313,325-335",
        ] {
            assert!(trace.contains(fragment), "trace names {fragment}");
        }
        // Seven enqueue sites transcribed, no more.
        assert_eq!(
            f["enqueues"].as_array().expect("enqueues array").len(),
            7,
            "fixture covers exactly the seven enqueue events"
        );
    }

    #[test]
    fn task_names_match_the_python_celery_names() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            RECENT_VISITED_TASK,
            "pi_dash.bgtasks.recent_visited_task.recent_visited_task"
        );
        // Same strings the D-19/D-33 owners pin (by value — this crate
        // cannot import pidash-jobs, so the literals must stay identical).
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            crate::v1_projects::tasks::MODEL_ACTIVITY_TASK
        );
    }

    #[test]
    fn model_create_matches_fixture() {
        // Fixture `enqueues[0]` (`base.py:339-347`): model_id str-wrapped,
        // current_instance null, actor raw id, origin base_host app url.
        let emit = module_create_activity(
            "module-1",
            json!({"name": "M"}),
            "actor-1",
            "ws-1",
            "https://app.example",
        );
        assert_eq!(emit.task_name(), MODEL_ACTIVITY_TASK);
        assert_eq!(emit.model_name, MODULE_MODEL_NAME);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            json!({
                "model_name": "module",
                "model_id": "module-1",
                "requested_data": {"name": "M"},
                "current_instance": null,
                "actor_id": "actor-1",
                "slug": "ws-1",
                "origin": "https://app.example",
            })
        );
    }

    #[test]
    fn model_update_carries_the_before_save_snapshot_verbatim() {
        // Fixture `enqueues[1]` (`base.py:668` + `:708-716`): the
        // current_instance snapshot is taken BEFORE serializer.save() and
        // crosses .delay as a JSON string, never re-parsed.
        let snapshot = r#"{"name":"Old","member_ids":[]}"#;
        let emit = module_update_activity(
            "module-1",
            json!({"name": "New"}),
            snapshot,
            "actor-1",
            "ws-1",
            "https://app.example",
        );
        assert_eq!(emit.task_name(), MODEL_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["current_instance"], json!(snapshot));
        assert_eq!(kwargs["requested_data"], json!({"name": "New"}));
        // Member add/remove rides this emit: the new member_ids travel in
        // requested_data — no separate member task exists.
        let member_emit = module_update_activity(
            "module-1",
            json!({"member_ids": ["u-1", "u-2"]}),
            snapshot,
            "actor-1",
            "ws-1",
            "https://app.example",
        );
        assert_eq!(
            member_emit.kwargs()["requested_data"],
            json!({"member_ids": ["u-1", "u-2"]})
        );
    }

    #[test]
    fn module_destroy_matches_fixture_per_issue() {
        // Fixture `enqueues[2]` (`base.py:728-741`): one emit per linked
        // issue, type deleted, requested_data str(pk), current the
        // module_name dump, project_id RAW (QUIRK-raw-project).
        let emit = module_destroy_activity(
            "module-1",
            "actor-1",
            "issue-9",
            "project-1",
            r#"{"module_name": "M"}"#,
            1_700_000_000,
            "https://app.example",
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            json!({
                "type": "module.activity.deleted",
                "requested_data": "{\"module_id\": \"module-1\"}",
                "actor_id": "actor-1",
                "issue_id": "issue-9",
                "project_id": "project-1",
                "current_instance": "{\"module_name\": \"M\"}",
                "epoch": 1_700_000_000,
                "notification": true,
                "origin": "https://app.example",
            })
        );
    }

    #[test]
    fn retrieve_visit_matches_fixture_and_stays_a_deferred_publish() {
        // Fixture `enqueues[3]` (`base.py:641-647`): the kwargs the
        // deferred publish carries. The builder pins them; handlers keep
        // the call site with no inline write (pilot-2 NO-OP pattern).
        let emit = retrieve_visit_emit("ws-1", "module-1", "actor-1", "project-1");
        assert_eq!(emit.task_name(), RECENT_VISITED_TASK);
        assert_eq!(emit.entity_name, VISIT_ENTITY_MODULE);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), RECENT_VISITED_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            json!({
                "slug": "ws-1",
                "entity_name": "module",
                "entity_identifier": "module-1",
                "user_id": "actor-1",
                "project_id": "project-1",
            })
        );
    }

    #[test]
    fn link_create_matches_fixture_per_issue() {
        // Fixture `enqueues[4]` (`issue.py:232-245`): one emit per issue,
        // type created, requested_data str(module_id), current null.
        let emit = link_create_activity(
            "module-1",
            "actor-1",
            "issue-9",
            "project-1",
            1_700_000_000,
            "https://app.example",
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["type"], json!("module.activity.created"));
        assert_eq!(
            kwargs["requested_data"],
            json!("{\"module_id\": \"module-1\"}")
        );
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["notification"], json!(true));
    }

    #[test]
    fn link_add_keeps_the_raw_loop_value() {
        // Fixture `enqueues[5]` (`issue.py:272-285` + QUIRK-raw-module-ref):
        // the added path dumps the RAW loop value — a non-string JSON
        // value passes through unwrapped.
        assert_eq!(
            module_ref_raw_json(&json!("module-1")),
            "{\"module_id\": \"module-1\"}"
        );
        assert_eq!(
            module_ref_raw_json(&json!(42)),
            "{\"module_id\": 42}",
            "raw loop value is NOT str-wrapped (issue.py:275)"
        );
        assert_eq!(
            module_id_ref_json("42"),
            "{\"module_id\": \"42\"}",
            "every other site str-wraps (issue.py:235,296,327)"
        );
        let emit = link_add_activity(
            module_ref_raw_json(&json!("module-1")),
            "actor-1",
            "issue-9",
            "project-1",
            1_700_000_000,
            "https://app.example",
        );
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["type"], json!("module.activity.created"));
        assert_eq!(kwargs["current_instance"], Value::Null);
    }

    #[test]
    fn link_remove_is_none_safe_while_destroy_is_not() {
        // Fixture `enqueues[5]` removed half (`issue.py:294-312`): a
        // missing link row renders module_name null.
        assert_eq!(link_removed_instance(Some("M")), "{\"module_name\": \"M\"}");
        assert_eq!(
            link_removed_instance(None),
            "{\"module_name\": null}",
            "gone row is None-safe on the remove path (issue.py:302-306)"
        );
        // Fixture `enqueues[6]` (`issue.py:325-335`): the destroy path has
        // no such guard — the type forces the caller to supply the name
        // (a gone row is the recorded 500, never an invented null).
        assert_eq!(link_destroy_instance("M"), "{\"module_name\": \"M\"}");
        let removed = link_remove_activity(
            "module-1",
            "actor-1",
            "issue-9",
            "project-1",
            None,
            1_700_000_000,
            "https://app.example",
        );
        let destroyed = link_destroy_activity(
            "module-1",
            "actor-1",
            "issue-9",
            "project-1",
            "M",
            1_700_000_000,
            "https://app.example",
        );
        for emit in [&removed, &destroyed] {
            assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
            assert_eq!(key_order(&emit.kwargs()), ISSUE_ACTIVITY_KWARG_ORDER);
            assert_eq!(emit.kwargs()["type"], json!("module.activity.deleted"));
        }
        assert_eq!(
            removed.kwargs()["current_instance"],
            json!("{\"module_name\": null}")
        );
        assert_eq!(
            destroyed.kwargs()["current_instance"],
            json!("{\"module_name\": \"M\"}")
        );
    }

    #[test]
    fn activity_types_and_entity_names_are_pinned() {
        assert_eq!(MODULE_ACTIVITY_CREATED, "module.activity.created");
        assert_eq!(MODULE_ACTIVITY_DELETED, "module.activity.deleted");
        assert_eq!(VISIT_ENTITY_MODULE, "module");
        assert_eq!(MODULE_MODEL_NAME, "module");
    }
}
