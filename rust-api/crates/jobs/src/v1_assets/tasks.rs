//! api-v1 asset + intake task publishers (D-21, jobs layer, PIDASHCONV-417).
//!
//! Port of the four `.delay()` units named by fixtures `fx-task-asset`
//! (`rust-api/fixtures/v1_assets/fx-task-asset.json`) and `fx-task-intake`
//! (`rust-api/fixtures/v1_assets/fx-task-intake.json`):
//!
//! 1. `get_asset_object_metadata` enqueue — fired only when
//!    `storage_metadata` is falsy from the user-asset patch
//!    (`apps/api/pi_dash/api/views/asset.py:212-219`), the server-asset
//!    patch (`:366-373`) and the generic-asset patch (`:608-617`).
//!    Exact arg shape `asset_id=str(asset_id)` in kwarg form. Task body:
//!    `apps/api/pi_dash/bgtasks/storage_metadata_task.py:14-30`.
//! 2. `issue_activity` created — intake post payload (type
//!    `issue.activity.created`, DjangoJSON-encoded `requested_data`,
//!    actor/issue/project ids, intake id, epoch;
//!    `apps/api/pi_dash/api/views/intake.py:186-216`).
//! 3. `issue_activity` updated — intake patch payload (type
//!    `issue.activity.updated`, `current_instance` = the pre-save
//!    `IssueSerializer` dump; `views/intake.py:394-412`).
//! 4. `intake.activity.created` — intake patch status-change payload
//!    (`notification=False`, `origin=base_host(app)`;
//!    `views/intake.py:414-431`).
//!
//! This module owns the Celery wire surface for those four call sites —
//! the task names, the `(args, kwargs)` binding in Python call order and
//! the falsy-`storage_metadata` guard — plus the [`NewJob`] constructors
//! the D-21 route handlers enqueue transactionally (`enqueue_in`). It
//! never implements task bodies and never touches the [`Registry`]:
//! * the metadata body + handler live in D-09
//!   (`tasks_cleanup::assets::register_metadata`, wired in the worker
//!   binary); the publisher below reuses [`delay_metadata`] verbatim;
//! * the activity dispatcher + handler live in D-08
//!   (`tasks_webhooks::activity_dispatch`); the publishers below reuse
//!   its task name and round-trip through its `bind_issue_activity` in
//!   tests.
//!
//! Both Celery names already have owners, so registering them again here
//! would fork ownership — the D-31 precedent (`services/app_assets/tasks.rs`:
//! record the task name and the publisher kwargs only, never the body).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * Patch `updated` stores the PRE-save issue snapshot as
//!   `current_instance` (serialized before `issue_serializer.save()`,
//!   `views/intake.py:396-412`) and the status-change call stores the
//!   PRE-save intake snapshot (`:416-417`) — both are before-images
//!   despite the key name `current_instance`. Ported as-is (callers pass
//!   the snapshot they hold; this module never re-reads the row).
//! * Post ignores the return of `issue_activity.delay` and any failure
//!   inside the task (the task swallows per its try/except); the 201
//!   response never reflects the activity outcome. Ported as-is
//!   (fire-and-forget enqueue, no result readback).
//! * The space asset site (`space/views/asset.py:148`) passes the id as
//!   one positional arg while the three api-v1 sites pass the
//!   `asset_id=` kwarg; both bind through the same D-09 `task_arg`
//!   (kwarg wins, else positional 0). The api-v1 publishers below always
//!   use the kwarg form.
//!
//! Fixtures: `rust-api/fixtures/v1_assets/fx-task-asset.json`
//! (FX `fx-task-asset`) and `fx-task-intake.json` (FX `fx-task-intake`).
//! The `#[cfg(test)]` suite below replays both: wire names, kwarg order,
//! null-ness, the guard truth table and a bind round-trip through the
//! D-08 binder.
//!
//! [`Registry`]: crate::worker::Registry
//! [`NewJob`]: crate::queue::NewJob

use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use crate::queue::NewJob;
/// Activity `type` strings for the three intake call sites. The
/// `issue.activity.*` pair is shared with the space publishers
/// (`crate::space`); `intake.activity.created` only fires here.
pub use crate::space::{ISSUE_CREATED, ISSUE_UPDATED};
/// The metadata task name and kwarg-form publisher, owned by D-09
/// (`tasks_cleanup::assets::TASK_GET_METADATA` / `delay_metadata`).
/// Re-exported — never redefined — so the two domains pin one string.
pub use crate::tasks_cleanup::assets::{
    delay_metadata, TASK_GET_METADATA as GET_ASSET_OBJECT_METADATA_TASK,
};
/// The activity task name, owned by D-08
/// (`tasks_webhooks::activity_dispatch::ISSUE_ACTIVITY_TASK`).
/// Re-exported so publishers pin the same string the dispatcher binds.
pub use crate::tasks_webhooks::activity_dispatch::ISSUE_ACTIVITY_TASK;

/// `intake.activity.created` (`views/intake.py:417-431`).
pub const INTAKE_CREATED: &str = "intake.activity.created";

/// The api-v1 metadata enqueue as a queue job: `args=[]`,
/// `kwargs={"asset_id": ...}` — the kwarg form at `asset.py:214,370,614`
/// (`delay_metadata` builds the identical kwargs for the wire message;
/// converting from it keeps the two shapes in one place).
pub fn metadata_job(asset_id: &str) -> NewJob {
    let message = delay_metadata(asset_id);
    NewJob::new(
        message.task,
        Value::Array(message.args),
        Value::Object(message.kwargs),
    )
}

/// Port of `if not asset.storage_metadata` (`asset.py:214,370,614`).
///
/// True (fetch the metadata) for every Python-falsy JSON value — null,
/// false, `0`/`0.0`, `""`, `[]`, `{}` — exactly the fixture guard
/// ("Falsy = None, {}, '', [], 0. A row with {'a': 1} skips the task").
/// Python ints are unbounded, so a nonzero number of any width is
/// truthy; only an exact zero in any numeric width fetches.
pub fn storage_metadata_missing(storage_metadata: &Value) -> bool {
    match storage_metadata {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => {
            number.as_i64().is_some_and(|v| v == 0)
                || number.as_u64().is_some_and(|v| v == 0)
                || number.as_f64().is_some_and(|v| v == 0.0)
        }
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
    }
}

fn opt_string(value: Option<&str>) -> Value {
    value
        .map(|v| Value::String(v.to_owned()))
        .unwrap_or(Value::Null)
}

/// Identity bundle for an intake activity enqueue: every id Python
/// stringifies at the call site (`str(request.user.id)`, `str(issue_id)`,
/// `str(project_id)`, `str(intake_issue.id)`).
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeActivityIds {
    pub actor_id: String,
    pub issue_id: String,
    pub project_id: String,
    pub intake_id: String,
}

impl IntakeActivityIds {
    pub fn new(actor_id: &str, issue_id: &str, project_id: &str, intake_id: &str) -> Self {
        Self {
            actor_id: actor_id.to_owned(),
            issue_id: issue_id.to_owned(),
            project_id: project_id.to_owned(),
            intake_id: intake_id.to_owned(),
        }
    }
}

/// Shared kwarg builder for the three intake `issue_activity.delay`
/// sites: kwargs in the exact Python call order — `type`,
/// `requested_data`, `actor_id`, `issue_id`, `project_id`,
/// `current_instance`, `epoch`, `intake` (`intake.py:207-216`,
/// `:403-411`, `:417-430`; `preserve_order` keeps insertion order on
/// the wire). `requested_data` / `current_instance` are the rendered
/// `json.dumps(..., cls=DjangoJSONEncoder)` text and pass through
/// verbatim, so DjangoJSON shapes (datetimes, Decimals, UUIDs) survive
/// byte for byte; rendering them is the caller's job.
fn intake_activity_kwargs(
    activity_type: &str,
    requested_data: &str,
    ids: &IntakeActivityIds,
    current_instance: Option<&str>,
    epoch: i64,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(10);
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(ids.actor_id.clone()));
    kwargs.insert("issue_id".to_owned(), Value::String(ids.issue_id.clone()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(ids.project_id.clone()),
    );
    kwargs.insert("current_instance".to_owned(), opt_string(current_instance));
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    kwargs.insert("intake".to_owned(), Value::String(ids.intake_id.clone()));
    kwargs
}

/// Intake post (`views/intake.py:207-216`): `requested_data` is the full
/// body dump (`json.dumps(request.data)`); `current_instance` is `None`;
/// `intake` is `str(intake_issue.id)` of the just-created row. `epoch`
/// is `int(timezone.now().timestamp())`, supplied by the caller.
pub fn intake_issue_created_message(
    requested_data: &str,
    ids: &IntakeActivityIds,
    epoch: i64,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        ISSUE_ACTIVITY_TASK,
        Vec::new(),
        intake_activity_kwargs(ISSUE_CREATED, requested_data, ids, None, epoch),
    )
}

/// The intake-post enqueue as a queue job (same payload as
/// [`intake_issue_created_message`]).
pub fn intake_issue_created_job(
    requested_data: &str,
    ids: &IntakeActivityIds,
    epoch: i64,
) -> NewJob {
    let message = intake_issue_created_message(requested_data, ids, epoch);
    NewJob::new(
        message.task,
        Value::Array(message.args),
        Value::Object(message.kwargs),
    )
}

/// Intake patch, issue half (`views/intake.py:394-412`): fires only when
/// `issue_data` is truthy (`bool(issue_data)` — absent/empty `issue` key
/// means no call). `requested_data` is the post-whitelist dump,
/// `current_instance` the PRE-save `IssueSerializer` snapshot (a
/// before-image despite the key name — ported as-is). The save happens
/// after the delay call (`issue_serializer.save()`, `:412`).
pub fn intake_issue_updated_message(
    requested_data: &str,
    ids: &IntakeActivityIds,
    current_instance: &str,
    epoch: i64,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        ISSUE_ACTIVITY_TASK,
        Vec::new(),
        intake_activity_kwargs(
            ISSUE_UPDATED,
            requested_data,
            ids,
            Some(current_instance),
            epoch,
        ),
    )
}

/// The intake-patch issue-half enqueue as a queue job (same payload as
/// [`intake_issue_updated_message`]).
pub fn intake_issue_updated_job(
    requested_data: &str,
    ids: &IntakeActivityIds,
    current_instance: &str,
    epoch: i64,
) -> NewJob {
    let message = intake_issue_updated_message(requested_data, ids, current_instance, epoch);
    NewJob::new(
        message.task,
        Value::Array(message.args),
        Value::Object(message.kwargs),
    )
}

/// Intake patch, status-change half (`views/intake.py:414-431`): fires
/// only when the intake serializer was built (`role > 15`) and saved.
/// `requested_data` is the remaining body dump (`issue` was popped at
/// `:344`); `current_instance` is the PRE-save `IntakeIssueSerializer`
/// snapshot (`:416-417`); `notification` is the literal `False`;
/// `origin` is `base_host(request, is_app=True)` — the app base URL
/// rendered by the caller; `intake` is `str(intake_issue.id)`.
/// `notification`/`origin` come after `epoch` in call order.
pub fn intake_status_changed_message(
    requested_data: &str,
    ids: &IntakeActivityIds,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> CeleryTaskMessage {
    let mut kwargs = intake_activity_kwargs(
        INTAKE_CREATED,
        requested_data,
        ids,
        Some(current_instance),
        epoch,
    );
    // Inserted before `intake` to hold the call-site order
    // (…, epoch, notification, origin, intake).
    let intake = kwargs.remove("intake").unwrap_or(Value::Null);
    kwargs.insert("notification".to_owned(), Value::Bool(false));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs.insert("intake".to_owned(), intake);
    CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, Vec::new(), kwargs)
}

/// The intake-patch status-change enqueue as a queue job (same payload
/// as [`intake_status_changed_message`]).
pub fn intake_status_changed_job(
    requested_data: &str,
    ids: &IntakeActivityIds,
    current_instance: &str,
    epoch: i64,
    origin: &str,
) -> NewJob {
    let message =
        intake_status_changed_message(requested_data, ids, current_instance, epoch, origin);
    NewJob::new(
        message.task,
        Value::Array(message.args),
        Value::Object(message.kwargs),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks_webhooks::activity_dispatch::bind_issue_activity;
    use serde_json::json;

    /// Committed oracle evidence:
    /// `rust-api/fixtures/v1_assets/fx-task-asset.json`.
    static ASSET_FIXTURE: &str = include_str!("../../../../fixtures/v1_assets/fx-task-asset.json");
    /// Committed oracle evidence:
    /// `rust-api/fixtures/v1_assets/fx-task-intake.json`.
    static INTAKE_FIXTURE: &str =
        include_str!("../../../../fixtures/v1_assets/fx-task-intake.json");

    /// Mirror of `contract-tests/_harness/celery_wire.py::assert_wire_message`
    /// (same helper as `space::tests::assert_wire`): the nine protocol-v2
    /// header keys are present, `task`/`lang` match, and the body carries
    /// exactly the published args/kwargs.
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

    fn ids() -> IntakeActivityIds {
        IntakeActivityIds::new("u1", "i9", "p1", "k1")
    }

    fn kwargs_keys(message: &CeleryTaskMessage) -> Vec<String> {
        message.body()[1]
            .as_object()
            .expect("kwargs object")
            .keys()
            .cloned()
            .collect()
    }

    /// Bind a published message the way the D-08 dispatcher binds it
    /// (task-oracle style: the wire this module publishes must bind
    /// cleanly where the worker runs it).
    fn bind_message(
        message: &CeleryTaskMessage,
    ) -> crate::tasks_webhooks::activity_dispatch::IssueActivityCall {
        bind_issue_activity(
            &Value::Array(message.args.clone()),
            &Value::Object(message.kwargs.clone()),
        )
        .expect("published intake payload binds")
    }

    // fx-task-asset · task name is the bare `@shared_task` dotted path.
    #[test]
    fn metadata_task_name_matches_fixture() {
        let fx: Value = serde_json::from_str(ASSET_FIXTURE).expect("asset fixture parses");
        assert_eq!(
            GET_ASSET_OBJECT_METADATA_TASK,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(
            fx["celery_wire"]["task"].as_str().expect("wire task"),
            GET_ASSET_OBJECT_METADATA_TASK
        );
        assert_eq!(fx["celery_wire"]["args"], json!([]));
    }

    // fx-task-asset · kwarg form `asset_id=str(asset_id)` at all 3 sites.
    #[test]
    fn metadata_wire_is_kwarg_form() {
        let message = delay_metadata("a1");
        let kwargs = message.kwargs.clone();
        assert_wire(&message, GET_ASSET_OBJECT_METADATA_TASK, json!([]), &kwargs);
        assert_eq!(
            kwargs,
            json!({"asset_id": "a1"}).as_object().expect("obj").clone()
        );
        let job = metadata_job("a1");
        assert_eq!(job.task, GET_ASSET_OBJECT_METADATA_TASK);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs, json!({"asset_id": "a1"}));
    }

    // fx-task-asset · guard: falsy fetches, `{'a': 1}` skips.
    #[test]
    fn metadata_guard_truth_table() {
        for falsy in [
            json!(null),
            json!({}),
            json!(""),
            json!([]),
            json!(0),
            json!(0.0),
            json!(false),
        ] {
            assert!(storage_metadata_missing(&falsy), "must fetch for {falsy}");
        }
        for truthy in [
            json!({"a": 1}),
            json!("x"),
            json!([0]),
            json!([null]),
            json!(1),
            json!(-3),
            json!(0.5),
            json!(true),
            json!(10_000_000_000_000_000_000_u64),
        ] {
            assert!(!storage_metadata_missing(&truthy), "must skip for {truthy}");
        }
    }

    // fx-task-asset · all three call sites share the one wire shape.
    #[test]
    fn metadata_call_sites_share_wire_shape() {
        let fx: Value = serde_json::from_str(ASSET_FIXTURE).expect("asset fixture parses");
        let sites = fx["call_sites"].as_array().expect("call_sites");
        assert_eq!(sites.len(), 3);
        for site in sites {
            assert_eq!(
                site["wire"]["task"].as_str().expect("site task"),
                GET_ASSET_OBJECT_METADATA_TASK
            );
            assert!(site["wire"]["kwargs"]["asset_id"].is_string());
        }
    }

    // fx-task-intake · signature: 7 positional params + 4 defaults.
    #[test]
    fn activity_signature_matches_fixture() {
        let fx: Value = serde_json::from_str(INTAKE_FIXTURE).expect("intake fixture parses");
        let params: Vec<String> = fx["task_signature"]["params"]
            .as_array()
            .expect("params")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        assert_eq!(
            params,
            [
                "type",
                "requested_data",
                "current_instance",
                "issue_id",
                "actor_id",
                "project_id",
                "epoch"
            ]
        );
        assert_eq!(
            fx["task_signature"]["defaults"],
            json!({"intake": null, "notification": false, "origin": null, "subscriber": true})
        );
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
    }

    const CALL_ORDER: [&str; 8] = [
        "type",
        "requested_data",
        "actor_id",
        "issue_id",
        "project_id",
        "current_instance",
        "epoch",
        "intake",
    ];

    // fx-task-intake · post create: full-body dump, current_instance null.
    #[test]
    fn intake_created_wire_matches_fixture() {
        let fx: Value = serde_json::from_str(INTAKE_FIXTURE).expect("intake fixture parses");
        let call = &fx["calls"]["post_issue_activity_created"]["wire"];
        assert_eq!(call["type"], json!("issue.activity.created"));
        assert!(call["current_instance"].is_null());
        let message = intake_issue_created_message(r#"{"issue":{"name":"n"}}"#, &ids(), 1727);
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue.activity.created"));
        assert_eq!(kwargs["requested_data"], json!(r#"{"issue":{"name":"n"}}"#));
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["epoch"], json!(1727));
        assert_eq!(kwargs["intake"], json!("k1"));
        assert_eq!(kwargs_keys(&message), CALL_ORDER);
        let job = intake_issue_created_job(r#"{"issue":{"name":"n"}}"#, &ids(), 1727);
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs["intake"], json!("k1"));
        let bound = bind_message(&message);
        assert_eq!(bound.activity_type, "issue.activity.created");
        assert!(!bound.notification);
        assert!(bound.origin.is_none());
    }

    // fx-task-intake · patch updated: post-whitelist dump + pre-save
    // IssueSerializer snapshot as current_instance (before-image bug).
    #[test]
    fn intake_updated_wire_matches_fixture() {
        let fx: Value = serde_json::from_str(INTAKE_FIXTURE).expect("intake fixture parses");
        let call = &fx["calls"]["patch_issue_activity_updated"]["wire"];
        assert_eq!(call["type"], json!("issue.activity.updated"));
        assert!(call["current_instance"].is_string());
        let message =
            intake_issue_updated_message(r#"{"name":"n"}"#, &ids(), r#"{"name":"o"}"#, 1727);
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!("issue.activity.updated"));
        assert_eq!(kwargs["current_instance"], json!(r#"{"name":"o"}"#));
        assert_eq!(kwargs_keys(&message), CALL_ORDER);
        let job = intake_issue_updated_job(r#"{"name":"n"}"#, &ids(), r#"{"name":"o"}"#, 1727);
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs["type"], json!("issue.activity.updated"));
        let bound = bind_message(&message);
        assert_eq!(bound.activity_type, "issue.activity.updated");
        assert_eq!(bound.current_instance.as_deref(), Some(r#"{"name":"o"}"#));
    }

    // fx-task-intake · patch status change: notification False,
    // origin app URL, intake last (…, epoch, notification, origin, intake).
    #[test]
    fn intake_status_changed_wire_matches_fixture() {
        let fx: Value = serde_json::from_str(INTAKE_FIXTURE).expect("intake fixture parses");
        let call = &fx["calls"]["patch_intake_activity_created"]["wire"];
        assert_eq!(call["type"], json!("intake.activity.created"));
        assert_eq!(call["notification"], json!(false));
        assert!(call["origin"].is_string());
        let message = intake_status_changed_message(
            r#"{"status":1}"#,
            &ids(),
            r#"{"status":0}"#,
            1727,
            "https://app.example.test",
        );
        let kwargs = message.kwargs.clone();
        assert_wire(&message, ISSUE_ACTIVITY_TASK, json!([]), &kwargs);
        assert_eq!(kwargs["type"], json!(INTAKE_CREATED));
        assert_eq!(kwargs["notification"], json!(false));
        assert_eq!(kwargs["origin"], json!("https://app.example.test"));
        assert_eq!(kwargs["intake"], json!("k1"));
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
                "origin",
                "intake",
            ]
        );
        let job = intake_status_changed_job(
            r#"{"status":1}"#,
            &ids(),
            r#"{"status":0}"#,
            1727,
            "https://app.example.test",
        );
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.kwargs["notification"], json!(false));
        assert_eq!(job.kwargs["origin"], json!("https://app.example.test"));
        let bound = bind_message(&message);
        assert_eq!(bound.activity_type, "intake.activity.created");
        assert!(!bound.notification);
        assert_eq!(bound.origin.as_deref(), Some("https://app.example.test"));
    }

    // The D-08 binder drops the dead `intake` parameter: every api-v1
    // payload binds, and `intake` never reaches the call struct.
    #[test]
    fn published_payloads_bind_without_intake() {
        let ids = IntakeActivityIds::new("u", "i", "p", "k");
        for message in [
            intake_issue_created_message("{}", &ids, 1),
            intake_issue_updated_message("{}", &ids, "{}", 1),
            intake_status_changed_message("{}", &ids, "{}", 1, "o"),
        ] {
            assert!(bind_message(&message).subscriber);
        }
    }
}
