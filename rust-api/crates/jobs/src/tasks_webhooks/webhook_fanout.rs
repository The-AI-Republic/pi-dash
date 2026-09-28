//! D-08 webhook fan-out (jobs layer).
//!
//! Port of `webhook_activity` (`apps/api/pi_dash/bgtasks/webhook_task.py:377-461`)
//! and `model_activity` (`:463-506`).
//!
//! This module owns the Celery wire surface and every pure decision the
//! two tasks make: the two task names (both plain `@shared_task`, no
//! bind, no autoretry), the positional-or-keyword payload binders, the
//! workspace/flag webhook filter decision, the `.delay()` kwargs
//! constructors in source call order, the deleted-verb `{id}`-only
//! `event_data` rule, the `activity` object builder, the per-webhook
//! send-task fan-out (via [`webhook_send`][crate::tasks_webhooks::webhook_send]
//! kwargs, whose task owns the actual send), the created-vs-diff plan of
//! `model_activity` with Python-`==` value comparison, and the fan-out
//! error classifier.
//!
//! The impure edges — the `Webhook` queryset fetch, the per-webhook
//! `get_model_data` reads and the `.delay()` broker writes — stay with
//! the Python workers until the domain gate flips ownership, so (like
//! the send path in `webhook_send`) no local handler is registered
//! here: [`is_webhook_fanout_task`] is the routing predicate and every
//! name routes to `PythonOwned` on an empty registry. Registering a
//! handler would steal live fan-out traffic while the row fetch, the
//! model-data reads and the downstream send still live in Python.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * QUIRK-1 (`:420-433`): the flag filters are independent `if`s, and
//!   an event with no matching branch (`user`, `intake_issue`, …)
//!   leaves the queryset UNFILTERED — every active webhook in the
//!   workspace fires. [`event_flag_for`] returns `None` for those.
//! * QUIRK-2 (`:435-451`): `event_data` and the actor lookup are
//!   recomputed inside the per-webhook loop, so N webhooks repeat the
//!   same reads N times. [`fan_out_messages`] builds one message per
//!   webhook id from a single rendered pair, keeping the repetition
//!   visible in the call shape.
//! * QUIRK-3 (`:486-488`): `model_activity` dispatches only for keys
//!   *present* in `current_instance`. A requested key absent there is
//!   silently ignored even when its value differs — including keys
//!   that were deleted from the model. [`diff_updates`] keeps the
//!   `in`-guard.
//! * QUIRK-4 (`model_activity` has no `try`): a `json.loads` failure or
//!   a non-object instance propagates and fails the task instead of
//!   being swallowed like the fan-out's broad `except`.
//!   [`parse_current_instance`] and [`diff_updates`] return `Err` on
//!   those paths.
//! * TRAP-1 (`:490`): `current_value != requested_value` is Python
//!   `==`, where `True == 1`, `1 == 1.0` and `[0] == [False]`.
//!   [`py_json_eq`] reproduces that comparison for JSON-decoded
//!   values (pinned against a CPython oracle); a naive
//!   `serde_json` `==` would fire spurious `updated` dispatches.
//!
//! Evidence: `rust-api/fixtures/tasks_webhooks/fx-web-04-webhook-fanout.json`
//! (FX-WEB-04).

use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use crate::tasks_webhooks::webhook_send::{webhook_send_task_kwargs, WEBHOOK_SEND_TASK_NAME};

/// `webhook_activity` (`:376`): full Celery name. A plain
/// `@shared_task` — no bind, no autoretry, no backoff.
pub const WEBHOOK_ACTIVITY_TASK_NAME: &str = "pi_dash.bgtasks.webhook_task.webhook_activity";
/// `model_activity` (`:462`): full Celery name. A plain
/// `@shared_task` — no bind, no autoretry, no backoff.
pub const MODEL_ACTIVITY_TASK_NAME: &str = "pi_dash.bgtasks.webhook_task.model_activity";

/// Both task names owned by this module.
pub const WEBHOOK_FANOUT_TASK_NAMES: [&str; 2] =
    [WEBHOOK_ACTIVITY_TASK_NAME, MODEL_ACTIVITY_TASK_NAME];

/// True for the two D-08 fan-out task names. The worker forwards them
/// to the Python plane until the domain gate flips ownership.
pub fn is_webhook_fanout_task(task: &str) -> bool {
    WEBHOOK_FANOUT_TASK_NAMES.contains(&task)
}

/// The `@shared_task` options decorating both fan-out tasks: plain
/// tasks (`:376`, `:462` carry no options).
pub struct TaskSpec {
    /// `bind=True` (neither task is bound).
    pub bind: bool,
    /// Autoretry on exception (neither task autoretries; the fan-out
    /// swallows everything, the diff propagates without retry policy).
    pub autoretry: bool,
}

/// Task spec shared by both fan-out tasks.
pub const FANOUT_TASK_SPEC: TaskSpec = TaskSpec {
    bind: false,
    autoretry: false,
};

/// Task spec for a task name: `Some` for the two fan-out tasks, `None`
/// otherwise.
pub fn task_spec_for(task: &str) -> Option<&'static TaskSpec> {
    if is_webhook_fanout_task(task) {
        Some(&FANOUT_TASK_SPEC)
    } else {
        None
    }
}

/// `Webhook` flag column for an event (`:420-433`): `project`,
/// `issue`, `module` (also `module_issue`), `cycle` (also
/// `cycle_issue`), `issue_comment`. `None` means no branch matched and
/// the queryset stays unfiltered (QUIRK-1) — every active webhook in
/// the workspace fires.
pub fn event_flag_for(event: &str) -> Option<&'static str> {
    match event {
        "project" => Some("project"),
        "issue" => Some("issue"),
        "module" | "module_issue" => Some("module"),
        "cycle" | "cycle_issue" => Some("cycle"),
        "issue_comment" => Some("issue_comment"),
        _ => None,
    }
}

/// The base webhook filter (`:418`):
/// `Webhook.objects.filter(workspace__slug=slug, is_active=True)`,
/// before the per-event flag column. `webhooks` carries
/// `Meta.ordering = ("-created_at",)`, so iteration (and hence
/// enqueue order) is newest-first.
pub struct WebhookFilter<'a> {
    /// `workspace__slug` value.
    pub slug: &'a str,
    /// `is_active=True` (always set in the source).
    pub is_active: bool,
    /// Extra flag column from [`event_flag_for`] (`None` = unfiltered).
    pub flag: Option<&'static str>,
}

/// Build the base filter plus the per-event flag for `event`.
pub fn webhook_filter_for<'a>(slug: &'a str, event: &str) -> WebhookFilter<'a> {
    WebhookFilter {
        slug,
        is_active: true,
        flag: event_flag_for(event),
    }
}

/// `webhooks` table (`Webhook.Meta.db_table`).
pub const WEBHOOKS_TABLE: &str = "webhooks";

/// `event_data` for one fan-out call (`:440`): the deleted verb sends
/// the bare `{"id": event_id}` shape only; every other verb forwards
/// the rendered `get_model_data(event, event_id)` value as-is.
pub fn event_data_for(verb: &str, event_id: &str, live_data: Value) -> Value {
    if verb == "deleted" {
        let mut data = Map::with_capacity(1);
        data.insert("id".to_owned(), Value::String(event_id.to_owned()));
        Value::Object(data)
    } else {
        live_data
    }
}

/// `activity` object key order (`:442-449`): field, new_value,
/// old_value, actor, old_identifier, new_identifier.
pub const ACTIVITY_KEY_ORDER: [&str; 6] = [
    "field",
    "new_value",
    "old_value",
    "actor",
    "old_identifier",
    "new_identifier",
];

/// Build the `activity` payload (`:442-449`) in source key order.
/// `actor` is the rendered `get_model_data(event="user",
/// event_id=actor_id)` value.
pub fn build_activity(
    field: Option<&str>,
    new_value: Value,
    old_value: Value,
    actor: Value,
    old_identifier: Option<&str>,
    new_identifier: Option<&str>,
) -> Map<String, Value> {
    let mut activity = Map::with_capacity(6);
    activity.insert(
        "field".to_owned(),
        field.map_or(Value::Null, |field| Value::String(field.to_owned())),
    );
    activity.insert("new_value".to_owned(), new_value);
    activity.insert("old_value".to_owned(), old_value);
    activity.insert("actor".to_owned(), actor);
    activity.insert(
        "old_identifier".to_owned(),
        old_identifier.map_or(Value::Null, |identifier| {
            Value::String(identifier.to_owned())
        }),
    );
    activity.insert(
        "new_identifier".to_owned(),
        new_identifier.map_or(Value::Null, |identifier| {
            Value::String(identifier.to_owned())
        }),
    );
    activity
}

/// One bound `webhook_activity` call: the eleven `.delay()` params
/// (`:377-388`), in signature order. `model_activity` dispatches
/// exactly this shape (`:467-480`, `:492-503`); the fan-out loop then
/// renders `event_data` and `activity` per webhook row from it.
#[derive(Debug, Clone, PartialEq)]
pub struct WebhookActivityCall {
    /// `event`: project, issue, module, cycle, issue_comment, …
    pub event: String,
    /// `verb`: created, updated, deleted (passed through as `action`).
    pub verb: String,
    /// Changed field (`None` on created).
    pub field: Option<String>,
    /// Previous value (`None` on created).
    pub old_value: Value,
    /// New value (`None` on created).
    pub new_value: Value,
    /// Actor id; rendered to the user object per webhook at fan-out.
    pub actor_id: String,
    /// `workspace__slug` filter value.
    pub slug: String,
    /// `current_site` (`origin`; may be `None`).
    pub current_site: Option<String>,
    /// Id of the event object.
    pub event_id: String,
    /// Previous identifier, if any.
    pub old_identifier: Option<String>,
    /// New identifier, if any.
    pub new_identifier: Option<String>,
}

/// `webhook_activity` kwargs in signature order (`:377-388`): event,
/// verb, field, old_value, new_value, actor_id, slug, current_site,
/// event_id, old_identifier, new_identifier.
pub const WEBHOOK_ACTIVITY_PARAMS_ORDER: [&str; 11] = [
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
    "new_identifier",
];

fn opt_str(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!("expected string-or-null, got {other}")),
    }
}

fn req_str(value: Option<&Value>, name: &str) -> Result<String, String> {
    match value {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(format!("{name} must be a string")),
    }
}

fn opt_value(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

/// Bind a `webhook_activity` payload. Celery binds positionally
/// first, then by keyword: eleven positional args in signature order
/// or the same eleven keywords (as every view and `model_activity`
/// call it). Anything else is a `TypeError` in Python, i.e. a
/// handler failure here.
pub fn bind_webhook_activity(args: &Value, kwargs: &Value) -> Result<WebhookActivityCall, String> {
    let args = args.as_array().ok_or("args must be an array")?;
    let kwargs = kwargs.as_object().ok_or("kwargs must be an object")?;
    if !args.is_empty() {
        if args.len() != WEBHOOK_ACTIVITY_PARAMS_ORDER.len() || !kwargs.is_empty() {
            return Err(format!(
                "{} takes 11 positional arguments",
                WEBHOOK_ACTIVITY_TASK_NAME
            ));
        }
        let mut iter = args.iter();
        let mut next = || iter.next();
        return Ok(WebhookActivityCall {
            event: req_str(next(), "event")?,
            verb: req_str(next(), "verb")?,
            field: opt_str(next())?,
            old_value: opt_value(next()),
            new_value: opt_value(next()),
            actor_id: req_str(next(), "actor_id")?,
            slug: req_str(next(), "slug")?,
            current_site: opt_str(next())?,
            event_id: req_str(next(), "event_id")?,
            old_identifier: opt_str(next())?,
            new_identifier: opt_str(next())?,
        });
    }
    Ok(WebhookActivityCall {
        event: req_str(kwargs.get("event"), "event")?,
        verb: req_str(kwargs.get("verb"), "verb")?,
        field: opt_str(kwargs.get("field"))?,
        old_value: opt_value(kwargs.get("old_value")),
        new_value: opt_value(kwargs.get("new_value")),
        actor_id: req_str(kwargs.get("actor_id"), "actor_id")?,
        slug: req_str(kwargs.get("slug"), "slug")?,
        current_site: opt_str(kwargs.get("current_site"))?,
        event_id: req_str(kwargs.get("event_id"), "event_id")?,
        old_identifier: opt_str(kwargs.get("old_identifier"))?,
        new_identifier: opt_str(kwargs.get("new_identifier"))?,
    })
}

fn opt_json(value: &Option<String>) -> Value {
    value
        .as_deref()
        .map_or(Value::Null, |text| Value::String(text.to_owned()))
}

/// `webhook_activity.delay(...)` kwargs in signature order
/// (`:467-480`): event, verb, field, old_value, new_value, actor_id,
/// slug, current_site, event_id, old_identifier, new_identifier.
pub fn webhook_activity_kwargs(call: &WebhookActivityCall) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(WEBHOOK_ACTIVITY_PARAMS_ORDER.len());
    kwargs.insert("event".to_owned(), Value::String(call.event.clone()));
    kwargs.insert("verb".to_owned(), Value::String(call.verb.clone()));
    kwargs.insert("field".to_owned(), opt_json(&call.field));
    kwargs.insert("old_value".to_owned(), call.old_value.clone());
    kwargs.insert("new_value".to_owned(), call.new_value.clone());
    kwargs.insert("actor_id".to_owned(), Value::String(call.actor_id.clone()));
    kwargs.insert("slug".to_owned(), Value::String(call.slug.clone()));
    kwargs.insert("current_site".to_owned(), opt_json(&call.current_site));
    kwargs.insert("event_id".to_owned(), Value::String(call.event_id.clone()));
    kwargs.insert("old_identifier".to_owned(), opt_json(&call.old_identifier));
    kwargs.insert("new_identifier".to_owned(), opt_json(&call.new_identifier));
    kwargs
}

/// First-attempt Celery message for `webhook_activity` (as
/// `model_activity` dispatches it, `:467-480` / `:492-503`).
pub fn webhook_activity_message(call: &WebhookActivityCall) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        WEBHOOK_ACTIVITY_TASK_NAME,
        Vec::new(),
        webhook_activity_kwargs(call),
    )
}

/// One bound `model_activity` call (`:463`): model_name, model_id,
/// requested_data, current_instance, actor_id, slug, origin (`None`
/// by default).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelActivityCall {
    /// `model_name` (doubles as the fan-out `event`).
    pub model_name: String,
    /// `model_id` (doubles as the fan-out `event_id`).
    pub model_id: String,
    /// New field values to diff.
    pub requested_data: Map<String, Value>,
    /// Raw `current_instance` JSON text (`None` = the `is None`
    /// created branch, checked before any parsing).
    pub current_instance: Option<String>,
    /// Actor id.
    pub actor_id: String,
    /// Workspace slug.
    pub slug: String,
    /// `origin` (becomes `current_site`; may be `None`).
    pub origin: Option<String>,
}

/// `model_activity` params in signature order (`:463`).
pub const MODEL_ACTIVITY_PARAMS_ORDER: [&str; 7] = [
    "model_name",
    "model_id",
    "requested_data",
    "current_instance",
    "actor_id",
    "slug",
    "origin",
];

/// Bind a `model_activity` payload: seven positional args in
/// signature order or the same seven keywords (as the views call
/// it); `origin` defaults to `None` when absent. Anything else is a
/// `TypeError` in Python.
pub fn bind_model_activity(args: &Value, kwargs: &Value) -> Result<ModelActivityCall, String> {
    let args = args.as_array().ok_or("args must be an array")?;
    let kwargs = kwargs.as_object().ok_or("kwargs must be an object")?;
    if !args.is_empty() {
        if args.len() != MODEL_ACTIVITY_PARAMS_ORDER.len() || !kwargs.is_empty() {
            return Err(format!(
                "{} takes 7 positional arguments",
                MODEL_ACTIVITY_TASK_NAME
            ));
        }
        let mut iter = args.iter();
        let mut next = || iter.next();
        return Ok(ModelActivityCall {
            model_name: req_str(next(), "model_name")?,
            model_id: req_str(next(), "model_id")?,
            requested_data: next()
                .and_then(Value::as_object)
                .cloned()
                .ok_or("requested_data must be an object")?,
            current_instance: opt_str(next())?,
            actor_id: req_str(next(), "actor_id")?,
            slug: req_str(next(), "slug")?,
            origin: opt_str(next())?,
        });
    }
    Ok(ModelActivityCall {
        model_name: req_str(kwargs.get("model_name"), "model_name")?,
        model_id: req_str(kwargs.get("model_id"), "model_id")?,
        requested_data: kwargs
            .get("requested_data")
            .and_then(Value::as_object)
            .cloned()
            .ok_or("requested_data must be an object")?,
        current_instance: opt_str(kwargs.get("current_instance"))?,
        actor_id: req_str(kwargs.get("actor_id"), "actor_id")?,
        slug: req_str(kwargs.get("slug"), "slug")?,
        origin: opt_str(kwargs.get("origin"))?,
    })
}

/// The `created` dispatch for a `None` instance (`:466-481`): one
/// `webhook_activity.delay` with verb `created`, null field and
/// values, null identifiers, `current_site` from `origin`.
pub fn created_call(call: &ModelActivityCall) -> WebhookActivityCall {
    WebhookActivityCall {
        event: call.model_name.clone(),
        verb: "created".to_owned(),
        field: None,
        old_value: Value::Null,
        new_value: Value::Null,
        actor_id: call.actor_id.clone(),
        slug: call.slug.clone(),
        current_site: call.origin.clone(),
        event_id: call.model_id.clone(),
        old_identifier: None,
        new_identifier: None,
    }
}

/// The `updated` dispatches for a present instance (`:486-504`): one
/// `webhook_activity.delay` per diff, with verb `updated`, null
/// identifiers, `current_site` from `origin`.
pub fn updated_calls(call: &ModelActivityCall, diffs: &[FieldDiff]) -> Vec<WebhookActivityCall> {
    diffs
        .iter()
        .map(|diff| WebhookActivityCall {
            event: call.model_name.clone(),
            verb: "updated".to_owned(),
            field: Some(diff.field.clone()),
            old_value: diff.old_value.clone(),
            new_value: diff.new_value.clone(),
            actor_id: call.actor_id.clone(),
            slug: call.slug.clone(),
            current_site: call.origin.clone(),
            event_id: call.model_id.clone(),
            old_identifier: None,
            new_identifier: None,
        })
        .collect()
}

/// One `webhook_send_task.delay(...)` per webhook row (`:435-451`),
/// in queryset order. `live_event_data` is the rendered
/// `get_model_data(event, event_id)` value shared by every row;
/// `actor` is the rendered user value shared by every row (QUIRK-2:
/// the source recomputes both inside the loop).
pub fn fan_out_messages(
    call: &WebhookActivityCall,
    webhook_ids: &[&str],
    actor: Value,
    live_event_data: Value,
    current_site: &str,
) -> Vec<CeleryTaskMessage> {
    let event_data = event_data_for(&call.verb, &call.event_id, live_event_data);
    let activity = build_activity(
        call.field.as_deref(),
        call.new_value.clone(),
        call.old_value.clone(),
        actor,
        call.old_identifier.as_deref(),
        call.new_identifier.as_deref(),
    );
    webhook_ids
        .iter()
        .map(|webhook_id| {
            CeleryTaskMessage::new(
                WEBHOOK_SEND_TASK_NAME,
                Vec::new(),
                webhook_send_task_kwargs(
                    webhook_id,
                    &call.slug,
                    &call.event,
                    event_data.clone(),
                    &call.verb,
                    current_site,
                    Value::Object(activity.clone()),
                ),
            )
        })
        .collect()
}

/// What the fan-out's broad `except` does (`:453-460`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutFailure {
    /// `isinstance(e, ObjectDoesNotExist)`: silent return for the
    /// race where the row was deleted mid-flight.
    NotFound,
    /// Any other exception: `print(e)` only when `settings.DEBUG`,
    /// then `log_exception(e)`, then return.
    Other,
}

/// Whether the failure path prints the exception: only for
/// non-not-found errors while `settings.DEBUG` is on. The return
/// value is always `None` on every path.
pub fn fanout_prints(failure: FanoutFailure, debug: bool) -> bool {
    failure == FanoutFailure::Other && debug
}

/// Python `==` over JSON-decoded values, as `model_activity`'s
/// `current_value != requested_value` comparison (`:490`) sees it:
/// `True == 1`, `1 == 1.0`, `[0] == [False]` and order-insensitive
/// dicts (TRAP-1). A plain `serde_json` `==` is stricter and would
/// fire spurious `updated` dispatches.
pub fn py_json_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(flag), Value::Number(number)) | (Value::Number(number), Value::Bool(flag)) => {
            bool_eq_number(*flag, number)
        }
        (Value::Number(a), Value::Number(b)) => number_eq(a, b),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| py_json_eq(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| py_json_eq(value, other)))
        }
        _ => false,
    }
}

/// Python `True == 1` / `False == 0`, extended to `1.0` / `0.0`:
/// any other bool/number pair is unequal.
fn bool_eq_number(flag: bool, number: &serde_json::Number) -> bool {
    let target: i64 = if flag { 1 } else { 0 };
    number_eq(&serde_json::Number::from(target), number)
}

/// Python number `==`: integers compare exactly; a float equal to an
/// integer value (`1e2`, `1.0`) equals that integer; anything else
/// compares as `f64`. Integers beyond the `f64` exact range that fell
/// back to floats on parse compare approximately — no model field
/// value lives at that scale.
fn number_eq(left: &serde_json::Number, right: &serde_json::Number) -> bool {
    if let (Some(a), Some(b)) = (left.as_i128(), right.as_i128()) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (as_integer_value(left), as_integer_value(right)) {
        return a == b;
    }
    left.as_f64() == right.as_f64()
}

/// Integer value of a JSON number when it is one: true ints, plus
/// integral floats (`1.0`, `1e2`, `-0.0`) that fit in `i128`.
fn as_integer_value(number: &serde_json::Number) -> Option<i128> {
    if let Some(int) = number.as_i128() {
        return Some(int);
    }
    let float = number.as_f64()?;
    if float.is_finite() && float.fract() == 0.0 && float.abs() < 1e36 {
        // `as` saturates out-of-range casts; the magnitude guard keeps
        // the conversion below the `i128` boundary.
        Some(float as i128)
    } else {
        None
    }
}

/// Parse `current_instance` (`:484`):
/// `json.loads(current_instance)`. A parse failure propagates out of
/// the task (QUIRK-4) — no swallow, unlike the fan-out.
pub fn parse_current_instance(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|error| format!("invalid current_instance: {error}"))
}

/// One `updated` dispatch the diff wants.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDiff {
    /// Changed key, in `requested_data` order.
    pub field: String,
    /// Value from `current_instance`.
    pub old_value: Value,
    /// Value from `requested_data`.
    pub new_value: Value,
}

/// Per-key diff (`:486-504`): for each key of `requested_data` that
/// is also present in `current_instance` (QUIRK-3: absent keys are
/// skipped even when they differ), dispatch `updated` when the
/// values differ under Python `==`. A non-object instance raises the
/// `TypeError` `in` would raise (QUIRK-4). Order follows
/// `requested_data` insertion order.
pub fn diff_updates(
    requested_data: &Map<String, Value>,
    current_instance: &Value,
) -> Result<Vec<FieldDiff>, String> {
    let current = current_instance
        .as_object()
        .ok_or_else(|| "current_instance is not an object".to_owned())?;
    let mut diffs = Vec::new();
    for (key, requested_value) in requested_data {
        if let Some(current_value) = current.get(key) {
            if !py_json_eq(current_value, requested_value) {
                diffs.push(FieldDiff {
                    field: key.clone(),
                    old_value: current_value.clone(),
                    new_value: requested_value.clone(),
                });
            }
        }
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_webhooks/fx-web-04-webhook-fanout.json`.
    static FIXTURE_FANOUT: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-web-04-webhook-fanout.json");

    fn fanout_fixture() -> Value {
        serde_json::from_str(FIXTURE_FANOUT).expect("fan-out fixture parses")
    }

    fn deleted_call() -> WebhookActivityCall {
        WebhookActivityCall {
            event: "issue".to_owned(),
            verb: "deleted".to_owned(),
            field: Some("name".to_owned()),
            old_value: json!("Old"),
            new_value: json!("New"),
            actor_id: "uid-9".to_owned(),
            slug: "ws-slug".to_owned(),
            current_site: Some("https://app.example".to_owned()),
            event_id: "iid-1".to_owned(),
            old_identifier: None,
            new_identifier: None,
        }
    }

    #[test]
    fn task_names_registered_with_plain_specs() {
        assert!(is_webhook_fanout_task(WEBHOOK_ACTIVITY_TASK_NAME));
        assert!(is_webhook_fanout_task(MODEL_ACTIVITY_TASK_NAME));
        assert_eq!(
            WEBHOOK_ACTIVITY_TASK_NAME,
            "pi_dash.bgtasks.webhook_task.webhook_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK_NAME,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(WEBHOOK_FANOUT_TASK_NAMES.len(), 2);
        // The sibling send-path predicate owns the other two names.
        assert!(!is_webhook_fanout_task(WEBHOOK_SEND_TASK_NAME));
        assert!(!is_webhook_fanout_task(
            "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email"
        ));
        // Both are plain `@shared_task`: no bind, no autoretry.
        for task in WEBHOOK_FANOUT_TASK_NAMES {
            let spec = task_spec_for(task).expect("fan-out task spec");
            assert!(!spec.bind, "{task}");
            assert!(!spec.autoretry, "{task}");
        }
        assert!(task_spec_for(WEBHOOK_SEND_TASK_NAME).is_none());
        assert!(task_spec_for("pi_dash.bgtasks.webhook_task.nope").is_none());
    }

    #[test]
    fn fanout_tasks_route_to_python_until_gate() {
        // No local handler is registered (registering one would steal
        // live traffic): an empty registry routes both names over
        // AMQP to the Python workers.
        use crate::worker::{route_for, Registry, Route};
        let registry = Registry::new();
        assert!(!registry.owns(WEBHOOK_ACTIVITY_TASK_NAME));
        assert!(!registry.owns(MODEL_ACTIVITY_TASK_NAME));
        assert_eq!(
            route_for(&registry, WEBHOOK_ACTIVITY_TASK_NAME),
            Route::PythonOwned
        );
        assert_eq!(
            route_for(&registry, MODEL_ACTIVITY_TASK_NAME),
            Route::PythonOwned
        );
    }

    #[test]
    fn event_flag_matrix_matches_fixture() {
        let fixture = fanout_fixture();
        let mapping = &fixture["fan_out"]["event_to_flag"];
        for (event, flag) in [
            ("project", "project"),
            ("issue", "issue"),
            ("module", "module"),
            ("module_issue", "module"),
            ("cycle", "cycle"),
            ("cycle_issue", "cycle"),
            ("issue_comment", "issue_comment"),
        ] {
            assert_eq!(event_flag_for(event), Some(flag), "event {event}");
            assert_eq!(mapping[event], json!(flag), "fixture {event}");
        }
        // QUIRK-1: events with no branch stay unfiltered.
        for event in ["user", "intake_issue", "", "project_issue"] {
            assert_eq!(event_flag_for(event), None, "event {event}");
        }
        let filter = webhook_filter_for("ws-slug", "issue");
        assert_eq!(filter.slug, "ws-slug");
        assert!(filter.is_active);
        assert_eq!(filter.flag, Some("issue"));
        assert_eq!(
            webhook_filter_for("ws-slug", "user").flag,
            None,
            "unknown event is unfiltered"
        );
        assert_eq!(WEBHOOKS_TABLE, "webhooks");
    }

    #[test]
    fn deleted_verb_sends_id_only_event_data() {
        // Fixture `golden_deleted`: verb deleted → `{"id": event_id}`.
        let fixture = fanout_fixture();
        let golden = &fixture["fan_out"]["per_webhook_delay"]["golden_deleted"];
        let call = deleted_call();
        let messages = fan_out_messages(
            &call,
            &["wid-1"],
            json!({"id": "uid-9"}),
            json!({"id": "iid-1", "name": "Must never leak on deleted"}),
            "https://app.example",
        );
        assert_eq!(messages.len(), 1);
        let send = &messages[0];
        assert_eq!(send.task, WEBHOOK_SEND_TASK_NAME);
        assert!(send.args.is_empty());
        assert_eq!(send.kwargs["event_data"], json!({"id": "iid-1"}));
        assert_eq!(send.kwargs["event_data"], golden["event_data"]);
        assert_eq!(send.kwargs["action"], json!("deleted"));
        assert_eq!(send.kwargs["action"], golden["action"]);
        assert_eq!(send.kwargs["event"], json!("issue"));
        assert_eq!(send.kwargs["slug"], json!("ws-slug"));
        assert_eq!(send.kwargs["webhook_id"], json!("wid-1"));
        assert_eq!(send.kwargs["activity"]["field"], json!("name"));
        assert_eq!(send.kwargs["activity"]["old_value"], json!("Old"));
        assert_eq!(send.kwargs["activity"]["new_value"], json!("New"));
    }

    #[test]
    fn live_verb_forwards_model_data_as_is() {
        // Any non-deleted verb passes the rendered get_model_data
        // value through untouched.
        let call = WebhookActivityCall {
            verb: "created".to_owned(),
            ..deleted_call()
        };
        let live = json!({"id": "iid-1", "name": "Live"});
        let messages = fan_out_messages(&call, &["wid-1"], Value::Null, live.clone(), "site");
        assert_eq!(messages[0].kwargs["event_data"], live);
        assert_eq!(messages[0].kwargs["action"], json!("created"));
    }

    #[test]
    fn fan_out_repeats_rendered_pair_per_webhook() {
        // QUIRK-2: N rows → N send messages sharing one rendered
        // event_data/activity pair.
        let call = deleted_call();
        let messages = fan_out_messages(
            &call,
            &["wid-1", "wid-2", "wid-3"],
            json!({"id": "uid-9"}),
            json!({"id": "iid-1"}),
            "https://app.example",
        );
        assert_eq!(messages.len(), 3);
        for (message, webhook_id) in messages.iter().zip(["wid-1", "wid-2", "wid-3"]) {
            assert_eq!(message.task, WEBHOOK_SEND_TASK_NAME);
            assert_eq!(message.kwargs["webhook_id"], json!(webhook_id));
        }
        assert_eq!(
            messages[0].kwargs["event_data"],
            messages[1].kwargs["event_data"]
        );
        assert_eq!(
            messages[0].kwargs["activity"],
            messages[2].kwargs["activity"]
        );
        // Empty set → no messages, never an error.
        assert!(fan_out_messages(&call, &[], Value::Null, Value::Null, "site").is_empty());
    }

    #[test]
    fn activity_object_key_order_matches_source() {
        let activity = build_activity(
            Some("name"),
            json!("New"),
            json!("Old"),
            json!({"id": "uid-9"}),
            None,
            None,
        );
        let order: Vec<&str> = activity.keys().map(String::as_str).collect();
        assert_eq!(order, ACTIVITY_KEY_ORDER);
        assert_eq!(
            order,
            [
                "field",
                "new_value",
                "old_value",
                "actor",
                "old_identifier",
                "new_identifier"
            ]
        );
    }

    #[test]
    fn webhook_activity_bind_accepts_both_forms() {
        let kwargs = json!({
            "event": "project",
            "verb": "deleted",
            "field": Value::Null,
            "old_value": Value::Null,
            "new_value": Value::Null,
            "actor_id": "uid-9",
            "slug": "ws-slug",
            "current_site": "https://app.example",
            "event_id": "pid-1",
            "old_identifier": Value::Null,
            "new_identifier": Value::Null,
        });
        let from_kwargs = bind_webhook_activity(&json!([]), &kwargs).expect("kwargs bind");
        assert_eq!(from_kwargs.event, "project");
        assert_eq!(
            from_kwargs.current_site.as_deref(),
            Some("https://app.example")
        );
        assert_eq!(from_kwargs.field, None);
        // Positional form binds the same shape.
        let args = json!([
            "project",
            "deleted",
            Value::Null,
            Value::Null,
            Value::Null,
            "uid-9",
            "ws-slug",
            "https://app.example",
            "pid-1",
            Value::Null,
            Value::Null,
        ]);
        let from_args = bind_webhook_activity(&args, &json!({})).expect("args bind");
        assert_eq!(from_args, from_kwargs);
        // Message kwargs follow signature order.
        let message = webhook_activity_message(&from_kwargs);
        assert_eq!(message.task, WEBHOOK_ACTIVITY_TASK_NAME);
        assert!(message.args.is_empty());
        let order: Vec<&str> = message.kwargs.keys().map(String::as_str).collect();
        assert_eq!(order, WEBHOOK_ACTIVITY_PARAMS_ORDER);
        // Arity and type errors fail the bind.
        assert!(bind_webhook_activity(&json!(["only"]), &json!({})).is_err());
        assert!(bind_webhook_activity(&json!([]), &json!({"event": "x"})).is_err());
    }

    #[test]
    fn model_activity_created_branch_matches_source() {
        // `current_instance is None` → one created dispatch, checked
        // before any parsing.
        let call = bind_model_activity(
            &json!([]),
            &json!({
                "model_name": "module",
                "model_id": "mid-1",
                "requested_data": {"name": "M"},
                "current_instance": Value::Null,
                "actor_id": "uid-9",
                "slug": "ws-slug",
                "origin": "https://app.example",
            }),
        )
        .expect("created bind");
        assert_eq!(call.current_instance, None);
        let created = created_call(&call);
        assert_eq!(created.event, "module");
        assert_eq!(created.verb, "created");
        assert_eq!(created.field, None);
        assert_eq!(created.old_value, Value::Null);
        assert_eq!(created.new_value, Value::Null);
        assert_eq!(created.event_id, "mid-1");
        assert_eq!(created.old_identifier, None);
        assert_eq!(created.new_identifier, None);
        assert_eq!(created.current_site.as_deref(), Some("https://app.example"));
        let message = webhook_activity_message(&created);
        assert_eq!(message.task, WEBHOOK_ACTIVITY_TASK_NAME);
        assert_eq!(message.kwargs["verb"], json!("created"));
        // `origin` may be absent (defaults None → null current_site).
        let minimal = bind_model_activity(
            &json!([]),
            &json!({
                "model_name": "issue",
                "model_id": "iid-1",
                "requested_data": {},
                "current_instance": Value::Null,
                "actor_id": "uid-9",
                "slug": "ws-slug",
            }),
        )
        .expect("origin defaults");
        assert_eq!(created_call(&minimal).current_site, None);
    }

    #[test]
    fn model_activity_diff_replays_fixture_golden() {
        // Fixture `golden_diff`: `name` fires (Old → New). `priority`
        // is present-but-equal there, so this test uses a *different*
        // requested value to pin the positive path too; `archived_at`
        // is ABSENT from the instance so nothing fires for it even
        // though a value was requested (QUIRK-3, `:486-488`).
        let requested = json!({
            "name": "New",
            "priority": "high",
            "archived_at": "2026-09-01",
        });
        let current: Value =
            serde_json::from_str(r#"{"name": "Old", "priority": "low"}"#).expect("instance");
        let diffs =
            diff_updates(requested.as_object().expect("requested"), &current).expect("diff");
        assert_eq!(
            diffs,
            [
                FieldDiff {
                    field: "name".to_owned(),
                    old_value: json!("Old"),
                    new_value: json!("New"),
                },
                FieldDiff {
                    field: "priority".to_owned(),
                    old_value: json!("low"),
                    new_value: json!("high"),
                },
            ]
        );
        // Identical values for a present key → no dispatch.
        let same = diff_updates(
            json!({"name": "Old"}).as_object().expect("requested"),
            &current,
        )
        .expect("no-diff");
        assert!(same.is_empty());
        // The updated dispatches carry verb + null identifiers.
        let call = ModelActivityCall {
            model_name: "m".to_owned(),
            model_id: "mid-1".to_owned(),
            requested_data: requested.as_object().expect("requested").clone(),
            current_instance: None,
            actor_id: "uid-9".to_owned(),
            slug: "ws-slug".to_owned(),
            origin: None,
        };
        let updated = updated_calls(&call, &diffs);
        assert_eq!(updated.len(), 2);
        assert_eq!(updated[0].verb, "updated");
        assert_eq!(updated[0].field.as_deref(), Some("name"));
        assert_eq!(updated[0].old_value, json!("Old"));
        assert_eq!(updated[0].new_value, json!("New"));
        assert_eq!(updated[1].field.as_deref(), Some("priority"));
        for dispatch in &updated {
            assert_eq!(dispatch.old_identifier, None);
            assert_eq!(dispatch.new_identifier, None);
            assert_eq!(dispatch.current_site, None);
            assert_eq!(dispatch.event, "m");
            assert_eq!(dispatch.event_id, "mid-1");
        }
    }

    #[test]
    fn model_activity_error_paths_propagate() {
        // QUIRK-4: no try/except — bad JSON and non-object instances
        // fail the task instead of being swallowed.
        assert!(parse_current_instance("{not json").is_err());
        assert!(parse_current_instance("{\"name\": \"Old\"}").is_ok());
        let requested = Map::new();
        assert!(
            diff_updates(&requested, &parse_current_instance("null").expect("null")).is_err(),
            "null instance is not iterable, like `in None`"
        );
        assert!(
            diff_updates(&requested, &json!([1, 2])).is_err(),
            "list instance is not a mapping"
        );
        assert!(bind_model_activity(&json!([]), &json!({"model_name": "x"})).is_err());
    }

    #[test]
    fn python_equality_matrix_matches_cpython_oracle() {
        // Every row below is `json.loads` + `==` output from CPython
        // (see /tmp/pyeq_oracle.py of the implementation run).
        for (left, right, equal) in [
            ("1", "1", true),
            ("1", "1.0", true),
            ("1", "true", true),
            ("0", "false", true),
            ("1.5", "1.5", true),
            ("1", "1.5", false),
            ("true", "1.0", true),
            ("false", "0.0", true),
            ("true", "1.5", false),
            ("2", "true", false),
            ("\"a\"", "\"a\"", true),
            ("\"a\"", "\"b\"", false),
            ("null", "null", true),
            ("null", "false", false),
            ("null", "0", false),
            ("\"1\"", "1", false),
            ("[1, 2]", "[1, 2]", true),
            ("[1, 2]", "[2, 1]", false),
            ("[1, true]", "[1, 1]", true),
            ("{\"a\": 1}", "{\"a\": 1.0}", true),
            ("{\"a\": 1}", "{\"a\": 2}", false),
            ("{\"a\": 1}", "{\"b\": 1}", false),
            ("0.1", "0.1", true),
            ("100", "1e2", true),
            ("-0.0", "0", true),
            ("9007199254740993", "9007199254740993", true),
            ("9007199254740993", "9007199254740992", false),
            ("\"\"", "false", false),
            ("[]", "false", false),
            ("{}", "false", false),
            ("[0]", "[false]", true),
        ] {
            let left: Value = serde_json::from_str(left).expect("left");
            let right: Value = serde_json::from_str(right).expect("right");
            assert_eq!(py_json_eq(&left, &right), equal, "{left} vs {right}");
        }
        // Dict order never matters to Python `==`.
        let ordered: Value = serde_json::from_str(r#"{"a": 1, "b": 2}"#).expect("ordered");
        let shuffled: Value = serde_json::from_str(r#"{"b": 2, "a": 1}"#).expect("shuffled");
        assert!(py_json_eq(&ordered, &shuffled));
    }

    #[test]
    fn fanout_failure_classifier_matches_source() {
        // ObjectDoesNotExist → silent; anything else logs (and prints
        // only under DEBUG); the task always returns None.
        assert!(!fanout_prints(FanoutFailure::NotFound, false));
        assert!(!fanout_prints(FanoutFailure::NotFound, true));
        assert!(!fanout_prints(FanoutFailure::Other, false));
        assert!(fanout_prints(FanoutFailure::Other, true));
    }
}
