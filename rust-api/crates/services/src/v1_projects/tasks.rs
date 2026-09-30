//! D-19 project task publishers: `model_activity` x2 + `webhook_activity` x1 (stage 5).
//!
//! Port of the three post-commit `.delay()` call sites in
//! `apps/api/pi_dash/api/views/project.py` — create `:258-267`, update
//! `:442-450`, delete `:495-507` — and the task defs they target in
//! `apps/api/pi_dash/bgtasks/webhook_task.py` (`model_activity` `:463-504`,
//! `webhook_activity` `:377-390`).
//!
//! Everything here is pure: Celery task names and the kwargs each `.delay()`
//! publisher enqueues, in call order. Bodies stay where they are ported —
//! the fan-out/send bodies in D-08 (`pidash-jobs` `tasks_webhooks::`
//! `webhook_fanout` / `webhook_send`; the `model_activity` /
//! `webhook_activity` names are pinned equal to D-33's
//! `app_integrations::tasks` consts below). This crate cannot depend on
//! `pidash-jobs`, so the handler side (PIDASHCONV-369) builds the
//! `CeleryTaskMessage` / `NewJob` from these triples (the D-02
//! `space::intake` precedent: `CeleryTaskMessage::new(task, vec![],
//! kwargs)`, then `queue::NewJob::new(...)`, then
//! `queue::enqueue_in(tx, ...)` inside the request transaction — the
//! transactional-enqueue half of the Porting guide jobs-plane row).
//! Handlers call the wrappers post-commit; they
//! never inline DB writes that bypass the queue (these three call sites
//! write no audit/history rows themselves — fixture `webhook_delivery`).
//!
//! # Publisher call sites
//!
//! | Call site | Python | Enqueued kwargs |
//! | --- | --- | --- |
//! | project create | `views/project.py:259-267` `model_activity.delay(...)` | [`model_activity_kwargs`] with `model_name="project"`, `model_id=str(project.id)`, `current_instance=None` |
//! | project update | `views/project.py:442-450` `model_activity.delay(...)` | [`model_activity_kwargs`] with `current_instance` = the `json.dumps(ProjectSerializer(project).data)` before-image (`:412`) |
//! | project delete | `views/project.py:495-507` `webhook_activity.delay(...)` | [`delete_kwargs`] with verb `"deleted"`, field/old/new `None`, identifiers `None` |
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * `model_activity` has no `deleted` branch — deletions enter through a
//!   direct `webhook_activity.delay(verb="deleted")`. The created path
//!   (`current_instance is None`) fans out to exactly one `created`
//!   dispatch; the updated path emits one `updated` dispatch per changed
//!   key. Both fan-out shapes are owned by D-33
//!   (`app_integrations::tasks::{plan_created_dispatch,
//!   plan_updated_dispatches}`) and re-exported here, never re-derived —
//!   including BUG-8 (`webhook_task.py:486-488`): only keys present in
//!   *both* `requested_data` and the snapshot emit; brand-new keys are
//!   silently ignored and deleted keys never emit.
//! * `requested_data` is the raw `request.data` (unvalidated input object);
//!   the update `current_instance` is the full `ProjectSerializer` read
//!   shape *with annotations*, rendered by `json.dumps(..., cls=
//!   DjangoJSONEncoder)` — it crosses `.delay` as a JSON *string*, not an
//!   object. [`update_kwargs`] keeps that string verbatim; the per-key
//!   diff parses it first ([`plan_update_dispatches_from_snapshot`]).
//! * `actor_id` (`request.user.id`) and the delete `event_id`
//!   (`project.id`, read *after* `project.delete()` — the row is
//!   soft-deleted, the in-memory pk survives) cross `.delay` as `UUID`
//!   objects, not `str` (only the create/update `model_id` is wrapped in
//!   `str()` at the call site). Celery's JSON serializer renders every
//!   `UUID` as `str` on the wire, so the builders take the string form
//!   (the D-33 Q6 convention).
//! * The origin kwarg is named `origin` on `model_activity` but
//!   `current_site` on `webhook_activity`, both `base_host(request,
//!   is_app=True)` (`utils/host.py:17-25`: `WEB_URL or APP_BASE_URL`).
//!   Ported as-is.
//! * Both tasks are plain `@shared_task` (no bind, no autoretry —
//!   `webhook_task.py:377` and `:463` carry bare decorators; only
//!   `webhook_send_task` binds/retries).
//!
//! Fixture replayed by the unit tests beside this file:
//! `rust-api/fixtures/v1_projects/tasks/project_activity.before_after.json`
//! (FX-TASKS).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

/// `model_activity` (`webhook_task.py:463`): full Celery name, plain
/// `@shared_task`. Body in D-08; pinned equal to the D-33 const (same
/// string — this crate cannot import `pidash-jobs`, so the pin is by
/// value on both sides).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";

/// `webhook_activity` (`webhook_task.py:377`): full Celery name, plain
/// `@shared_task`. Body in D-08; pinned equal to the D-33 const.
pub const WEBHOOK_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.webhook_activity";

/// `model_name` for all three D-19 call sites (`views/project.py:260,443`
/// and the delete fan-out `event="project"` at `:496`).
pub const PROJECT_MODEL_NAME: &str = "project";

/// The created/updated fan-out plan owned by D-33
/// (`app_integrations::tasks`, port of `webhook_task.py:466-504`).
/// Re-exported — never forked — so the two domains pin the same dispatch
/// logic, including the BUG-8 `in`-guard.
pub use crate::app_integrations::tasks::{
    plan_created_dispatch, plan_updated_dispatches, ActivityDispatch,
};

/// Re-exported `webhook_activity.delay(...)` kwargs builder owned by D-33
/// (port of `webhook_task.py:466-504` call order: `event`, `verb`,
/// `field`, `old_value`, `new_value`, `actor_id`, `slug`, `current_site`,
/// `event_id`, `old_identifier`, `new_identifier`). The delete call site
/// below builds on it.
pub use crate::app_integrations::tasks::webhook_activity_kwargs;

/// One `model_activity.delay(...)` enqueue from the project create/update
/// call sites (`views/project.py:259-267`, `:442-450`).
///
/// Kwargs in call order: `model_name`, `model_id`, `requested_data`,
/// `current_instance`, `actor_id`, `slug`, `origin`. `requested_data` is
/// the raw `request.data` object; `current_instance` is `None` on create
/// and the `json.dumps` snapshot string on update. Values pass through
/// verbatim (the worker normalizes via `DjangoJSONEncoder`, not the
/// publisher).
#[allow(clippy::too_many_arguments)]
pub fn model_activity_kwargs(
    model_name: &str,
    model_id: &str,
    requested_data: Value,
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
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

/// The project-create publisher (`views/project.py:259-267`):
/// `model_name="project"`, `model_id=str(project.id)`,
/// `current_instance=None`.
pub fn create_kwargs(
    model_id: &str,
    requested_data: Value,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    model_activity_kwargs(
        PROJECT_MODEL_NAME,
        model_id,
        requested_data,
        None,
        actor_id,
        slug,
        origin,
    )
}

/// The project-update publisher (`views/project.py:442-450`):
/// `current_instance` is the before-image JSON string
/// (`views/project.py:412`).
pub fn update_kwargs(
    model_id: &str,
    requested_data: Value,
    snapshot_json: &str,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    model_activity_kwargs(
        PROJECT_MODEL_NAME,
        model_id,
        requested_data,
        Some(snapshot_json),
        actor_id,
        slug,
        origin,
    )
}

/// The project-delete publisher (`views/project.py:495-507`): a direct
/// `webhook_activity.delay(...)` — the only call site that skips
/// `model_activity`. `event="project"`, verb `"deleted"`, field/old/new
/// `None`, both identifiers `None`, `event_id` the (string form of the)
/// soft-deleted row's pk.
pub fn delete_kwargs(
    actor_id: &str,
    slug: &str,
    current_site: &str,
    event_id: &str,
) -> Map<String, Value> {
    webhook_activity_kwargs(
        PROJECT_MODEL_NAME,
        "deleted",
        None,
        Value::Null,
        Value::Null,
        actor_id,
        slug,
        current_site,
        event_id,
        None,
        None,
    )
}

/// The update-path diff (`webhook_task.py:482-504`) starting from the
/// snapshot *string* the update call site enqueues: parse the
/// `json.dumps(ProjectSerializer(project).data)` before-image (must be a
/// JSON object — `json.loads` raises in Python on anything else) and run
/// the shared [`plan_updated_dispatches`] over `(requested_data,
/// snapshot)`, keeping the BUG-8 `in`-guard.
pub fn plan_update_dispatches_from_snapshot(
    requested_data: &Map<String, Value>,
    snapshot_json: &str,
) -> Result<Vec<ActivityDispatch>, String> {
    let snapshot: Value =
        serde_json::from_str(snapshot_json).map_err(|e| format!("current_instance: {e}"))?;
    let Some(snapshot_map) = snapshot.as_object() else {
        return Err("current_instance: snapshot is not a JSON object".to_owned());
    };
    Ok(plan_updated_dispatches(requested_data, snapshot_map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_integrations::tasks as shared;
    use serde_json::json;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/v1_projects/tasks/project_activity.before_after.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/v1_projects/tasks/project_activity.before_after.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn fixture_names_the_python_sources() {
        let f = fixture();
        assert_eq!(f["fixture"], json!("FX-TASKS"));
        let sites = &f["call_sites"];
        for (site, source) in [
            ("project_create", "api/views/project.py:258-267"),
            ("project_update", "api/views/project.py:442-450"),
            ("project_delete", "api/views/project.py:486-507"),
        ] {
            let recorded = sites[site]["source"].as_str().expect("source line");
            assert!(
                recorded.contains(source),
                "{site} trace names {source}: {recorded}"
            );
        }
        // Task bodies + origin helper are traced too (exact fragments
        // from the fixture: def lines :463-464 / :377-390 in the task
        // slots, fan-out ranges :466-480 / :482-508 in the worker_fanout
        // slots, origin in utils/host.py:17-25).
        let body = serde_json::to_string(&f).expect("serializes");
        for fragment in [
            "bgtasks/webhook_task.py:253-260,377-390",
            "bgtasks/webhook_task.py:463-464",
            "bgtasks/webhook_task.py:466-480",
            "bgtasks/webhook_task.py:482-508",
            "utils/host.py:17-25",
        ] {
            assert!(body.contains(fragment), "fixture names {fragment}");
        }
    }

    #[test]
    fn task_names_match_the_python_celery_names() {
        // Plain `@shared_task` names are the module path; these literals
        // must stay byte-identical to the D-08/D-33 owners (this crate
        // cannot import pidash-jobs, so the pin is by value — and by
        // equality with the D-33 consts, so the two domains never fork).
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            WEBHOOK_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.webhook_activity"
        );
        assert_eq!(MODEL_ACTIVITY_TASK, shared::MODEL_ACTIVITY_TASK);
        assert_eq!(WEBHOOK_ACTIVITY_TASK, shared::WEBHOOK_ACTIVITY_TASK);
    }

    #[test]
    fn create_kwargs_follow_the_delay_call_order() {
        // views/project.py:259-267: model_name, model_id, requested_data,
        // current_instance=None, actor_id, slug, origin=base_host(is_app).
        let kwargs = create_kwargs(
            "proj-1",
            json!({"name": "Pinned"}),
            "actor-1",
            "ws-1",
            "https://app.example",
        );
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
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
        assert_eq!(kwargs["model_name"], json!("project"));
        assert_eq!(kwargs["model_id"], json!("proj-1"));
        assert_eq!(kwargs["requested_data"], json!({"name": "Pinned"}));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["origin"], json!("https://app.example"));
    }

    #[test]
    fn update_kwargs_carry_the_snapshot_string_verbatim() {
        // views/project.py:412 + :442-450: current_instance is the
        // json.dumps(ProjectSerializer(project).data) before-image — a
        // JSON string on the wire, never re-parsed by the publisher.
        let snapshot = r#"{"name":"Old","description":null}"#;
        let kwargs = update_kwargs(
            "proj-1",
            json!({"name": "New"}),
            snapshot,
            "actor-1",
            "ws-1",
            "https://app.example",
        );
        assert_eq!(kwargs["current_instance"], json!(snapshot));
        assert_eq!(kwargs["requested_data"], json!({"name": "New"}));
    }

    #[test]
    fn delete_kwargs_are_a_direct_deleted_webhook_activity() {
        // views/project.py:495-507: skips model_activity; verb "deleted",
        // field/old/new None, identifiers None, and the kwarg is named
        // current_site (not origin).
        let kwargs = delete_kwargs("actor-1", "ws-1", "https://app.example", "proj-1");
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "event",
                "verb",
                "field",
                "old_value",
                "new_value",
                "actor_id",
                "slug",
                "current_site",
                "event_id",
                "old_identifier",
                "new_identifier"
            ]
        );
        assert_eq!(kwargs["event"], json!("project"));
        assert_eq!(kwargs["verb"], json!("deleted"));
        assert_eq!(kwargs["field"], Value::Null);
        assert_eq!(kwargs["old_value"], Value::Null);
        assert_eq!(kwargs["new_value"], Value::Null);
        assert_eq!(kwargs["current_site"], json!("https://app.example"));
        assert!(kwargs.get("origin").is_none());
        assert_eq!(kwargs["event_id"], json!("proj-1"));
        assert_eq!(kwargs["old_identifier"], Value::Null);
        assert_eq!(kwargs["new_identifier"], Value::Null);
    }

    #[test]
    fn update_plan_diffs_snapshot_with_the_in_guard() {
        // webhook_task.py:486-504 via BUG-8: only keys present in BOTH the
        // requested data and the snapshot emit; brand-new keys are
        // silently ignored, equal keys never emit.
        let requested: Map<String, Value> = Map::from_iter([
            ("name".to_owned(), json!("New")),
            ("same".to_owned(), json!(1)),
            ("brand_new".to_owned(), json!("x")),
        ]);
        let snapshot = r#"{"name":"Old","same":1}"#;
        let plans =
            plan_update_dispatches_from_snapshot(&requested, snapshot).expect("snapshot parses");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].verb, "updated");
        assert_eq!(plans[0].field.as_deref(), Some("name"));
        assert_eq!(plans[0].old_value, json!("Old"));
        assert_eq!(plans[0].new_value, json!("New"));
    }

    #[test]
    fn update_plan_rejects_a_non_object_snapshot() {
        let requested: Map<String, Value> = Map::new();
        assert!(plan_update_dispatches_from_snapshot(&requested, "[1,2]").is_err());
        assert!(plan_update_dispatches_from_snapshot(&requested, "not json").is_err());
    }

    #[test]
    fn db_deltas_are_domain_rows_only() {
        // FX-TASKS before/after: these three call sites write no
        // audit/history rows themselves — the observable delta is the
        // domain rows plus the Celery queue messages.
        let f = fixture();
        let delivery = f["webhook_delivery"].as_str().expect("delivery note");
        assert!(
            delivery.contains("no audit/history rows are written"),
            "delivery note pins the no-audit delta"
        );
        for site in ["project_create", "project_update", "project_delete"] {
            let payload_task = f["call_sites"][site]["payload"]["task"]
                .as_str()
                .expect("payload task");
            assert!(
                payload_task.contains("pi_dash.bgtasks.webhook_task."),
                "{site} payload names its task: {payload_task}"
            );
        }
        assert_eq!(
            f["call_sites"]["project_delete"]["payload"]["task"],
            json!("pi_dash.bgtasks.webhook_task.webhook_activity (direct .delay, no model_activity wrapper)")
        );
    }
}
