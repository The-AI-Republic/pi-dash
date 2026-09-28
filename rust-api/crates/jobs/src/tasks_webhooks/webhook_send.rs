//! D-08 webhook send path (jobs layer).
//!
//! Port of `save_webhook_log` (`apps/api/pi_dash/bgtasks/webhook_task.py:93-140`),
//! `send_webhook_deactivation_email` (`:189-251`) and `webhook_send_task`
//! (`:253-375`).
//!
//! This module owns the Celery wire surface and every pure decision the
//! tasks make: the two task names, the send task's retry policy, the
//! `.delay()` kwargs constructors, the HTTP-action map, the delivery
//! envelope, the HMAC-SHA256 signature input rendering, the log-document
//! builder (mongo → postgres fallback), the max-retries deactivation
//! cascade and the deactivation-email builders. The impure edges — the
//! webhook-row fetch, the `requests.post` send, the mongo insert, the
//! postgres insert and the SMTP send — stay with the Python workers
//! until the domain gate flips ownership, so (like the D-09 export
//! tasks) no local handler is registered here: [`is_webhook_task`] is
//! the routing predicate and every name routes to `PythonOwned`.
//!
//! The `event_data` / `activity` DjangoJSONEncoder round-trip
//! (`:293-295`) normalises live Python objects (tuples to lists,
//! datetimes/`Decimal`/`UUID` to strings) into plain JSON. At this layer
//! both values already arrive as rendered `serde_json::Value`, where
//! those source types cannot occur, so the round-trip is the identity
//! and [`build_envelope`] takes the values as-is.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`:314-318`): the signature is computed over
//!   `json.dumps(payload)` with *default* separators (`', '`, `': '`),
//!   not over the wire bytes `requests` sends for `json=payload`
//!   (compact separators). [`render_signature_input`] reproduces the
//!   default-separator, `ensure_ascii=True` rendering byte for byte;
//!   the fixture's sample hex pins it.
//! * BUG-2 (`:297-302`): unknown HTTP verbs pass through the action map
//!   unchanged (`GET` stays `GET`). [`map_action`] keeps the fallback.
//! * BUG-3 (`:370`): past max retries the task raises a *bare*
//!   `requests.RequestException()`, discarding the original error; the
//!   autoretry then carries no message. [`AfterFailure::Retry`] marks
//!   that path.
//! * BUG-4 (`:360`): deactivation is a filtered
//!   `QuerySet.update(is_active=False)` — `updated_at` is untouched.
//!   [`deactivation_update`] records the set clause without a timestamp.
//! * BUG-5 (`:134-140`): postgres-fallback failures are swallowed by a
//!   second broad `except`, so a failed log write never fails the
//!   sending task. [`route_log_sink`] keeps the two-sink shape with no
//!   error propagation.
//! * QUIRK-1 (`:107-118`): every log slot is wrapped in `str()` except
//!   `retry_count`, so dicts land as Python `repr`, *not* JSON.
//!   [`build_log_doc`] stringifies via [`crate::celery::py_repr`].
//!
//! Evidence: `rust-api/fixtures/tasks_webhooks/fx-web-01-save-webhook-log.json`
//! (FX-WEB-01) and `rust-api/fixtures/tasks_webhooks/fx-web-03-webhook-send-task.json`
//! (FX-WEB-03).

use serde_json::{Map, Value};

use crate::celery::{py_repr, CeleryTaskMessage};

/// `webhook_send_task` (`:260`): full Celery name.
pub const WEBHOOK_SEND_TASK_NAME: &str = "pi_dash.bgtasks.webhook_task.webhook_send_task";
/// `send_webhook_deactivation_email` (`:190`): full Celery name. A plain
/// `@shared_task` in the source — no bind, no autoretry, no backoff.
pub const DEACTIVATION_EMAIL_TASK_NAME: &str =
    "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email";

/// Both task names owned by this module.
pub const WEBHOOK_TASK_NAMES: [&str; 2] = [WEBHOOK_SEND_TASK_NAME, DEACTIVATION_EMAIL_TASK_NAME];

/// True for the two D-08 webhook task names. The worker forwards them
/// to the Python plane until the domain gate flips ownership.
pub fn is_webhook_task(task: &str) -> bool {
    WEBHOOK_TASK_NAMES.contains(&task)
}

/// The `@shared_task` retry options decorating `webhook_send_task`
/// (`:253-259`): bound task, autoretry on `RequestException`, 600s
/// backoff with jitter, 5 max retries.
pub struct RetrySpec {
    /// `bind=True`.
    pub bind: bool,
    /// `autoretry_for=(requests.RequestException,)`.
    pub autoretry_for: &'static str,
    /// `retry_backoff=600` seconds.
    pub backoff_secs: u64,
    /// `max_retries=5`.
    pub max_retries: u32,
    /// `retry_jitter=True`.
    pub jitter: bool,
}

/// Retry spec of the send task (`:253-259`).
pub const WEBHOOK_SEND_RETRY: RetrySpec = RetrySpec {
    bind: true,
    autoretry_for: "requests.RequestException",
    backoff_secs: 600,
    max_retries: 5,
    jitter: true,
};

/// Retry spec for a task name: `Some` for the send task, `None` for the
/// plain deactivation-email task (`:189` carries no options).
pub fn retry_spec_for(task: &str) -> Option<&'static RetrySpec> {
    if task == WEBHOOK_SEND_TASK_NAME {
        Some(&WEBHOOK_SEND_RETRY)
    } else {
        None
    }
}

/// Map the calling HTTP verb to the webhook action (`:297-302`):
/// POST→create, PATCH|PUT→update, DELETE→delete, anything else passes
/// through unchanged (BUG-2).
pub fn map_action(action: &str) -> &str {
    match action {
        "POST" => "create",
        "PATCH" | "PUT" => "update",
        "DELETE" => "delete",
        other => other,
    }
}

/// Envelope key order (`:304-311`): event, action, webhook_id,
/// workspace_id, data, activity.
pub const ENVELOPE_KEY_ORDER: [&str; 6] = [
    "event",
    "action",
    "webhook_id",
    "workspace_id",
    "data",
    "activity",
];

/// Build the delivery envelope (`:304-311`) in source key order.
/// `None` data/activity render as JSON null, as in the fixture golden.
pub fn build_envelope(
    event: &str,
    action: &str,
    webhook_id: &str,
    workspace_id: &str,
    data: Option<Value>,
    activity: Option<Value>,
) -> Map<String, Value> {
    let mut envelope = Map::with_capacity(6);
    envelope.insert("event".to_owned(), Value::String(event.to_owned()));
    envelope.insert("action".to_owned(), Value::String(action.to_owned()));
    envelope.insert(
        "webhook_id".to_owned(),
        Value::String(webhook_id.to_owned()),
    );
    envelope.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    envelope.insert("data".to_owned(), data.unwrap_or(Value::Null));
    envelope.insert("activity".to_owned(), activity.unwrap_or(Value::Null));
    envelope
}

/// Render a JSON value the way `json.dumps(payload)` does with default
/// settings (`:317`): `', '` / `': '` separators, `ensure_ascii=True`
/// (BUG-1). Object key order is the map's insertion order — build the
/// envelope with [`build_envelope`] so the HMAC input matches Python's
/// `dict` order.
pub fn render_signature_input(value: &Value) -> String {
    let mut out = String::new();
    render_value(value, &mut out);
    out
}

fn render_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => render_number(number, out),
        Value::String(text) => render_py_json_str(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_py_json_str(key, out);
                out.push_str(": ");
                render_value(item, out);
            }
            out.push('}');
        }
    }
}

fn render_number(number: &serde_json::Number, out: &mut String) {
    if let Some(unsigned) = number.as_u64() {
        out.push_str(&unsigned.to_string());
    } else if let Some(signed) = number.as_i64() {
        out.push_str(&signed.to_string());
    } else if let Some(float) = number.as_f64() {
        out.push_str(&py_float_repr(float));
    } else {
        out.push_str(&number.to_string());
    }
}

/// Python `repr(float)` for finite values: shortest round-trip like
/// `ryu`, but the exponent always carries a sign with at least two
/// digits (`1e+16`, `1e-05`). `serde_json::Value` cannot hold
/// NaN/infinity, so those tokens are unreachable here.
fn py_float_repr(float: f64) -> String {
    let text = serde_json::Number::from_f64(float)
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_owned());
    if let Some(exponent_at) = text.find(['e', 'E']) {
        let (mantissa, exponent) = text.split_at(exponent_at);
        let shift: i32 = exponent[1..].parse().unwrap_or(0);
        return format!("{mantissa}e{shift:+03}");
    }
    // Python switches to exponent form once the decimal exponent drops
    // below -4 (`1e-05`), while `serde_json` still prints that decade
    // fixed (`0.00001`). A fixed fraction with four or more leading
    // zeros is exactly that decade, so rewrite it to the exponent form.
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    if let Some(fraction) = digits.strip_prefix("0.") {
        let zeros = fraction.bytes().take_while(|byte| *byte == b'0').count();
        if zeros >= 4 {
            let sig = fraction[zeros..].trim_end_matches('0');
            if sig.is_empty() {
                return text;
            }
            let head = &sig[..1];
            let tail = &sig[1..];
            let shift = zeros + 1;
            if tail.is_empty() {
                return format!("{sign}{head}e-{shift:02}");
            }
            return format!("{sign}{head}.{tail}e-{shift:02}");
        }
    }
    text
}

/// Python `json.dumps` string escaping with `ensure_ascii=True`:
/// `"`, `\` and the short escapes, other controls as lowercase
/// `\u00xx`, every code point outside `0x20..=0x7E` as lowercase
/// `\uXXXX` with surrogate pairs above `0xFFFF`. `/` stays bare.
fn render_py_json_str(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if ('\u{20}'..='\u{7e}').contains(&c) => out.push(c),
            c => {
                let code = c as u32;
                if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let scalar = code - 0x10000;
                    let high = 0xD800 + (scalar >> 10);
                    let low = 0xDC00 + (scalar & 0x3FF);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// HMAC-SHA256 hex over the rendered signature input
/// (`hmac.new(secret.encode(), json.dumps(payload).encode(),
/// hashlib.sha256).hexdigest()`, `:315-320`).
pub fn sign_payload_hex(secret: &str, payload_json: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(payload_json.as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

/// Outgoing header names (`:285-290`, `:321`).
pub const HEADER_CONTENT_TYPE: &str = "Content-Type";
/// Outgoing header names (`:285-290`, `:321`).
pub const HEADER_USER_AGENT: &str = "User-Agent";
/// Outgoing header names (`:285-290`, `:321`).
pub const HEADER_DELIVERY: &str = "X-Pi Dash-Delivery";
/// Outgoing header names (`:285-290`, `:321`).
pub const HEADER_EVENT: &str = "X-Pi Dash-Event";
/// Signature header, sent only when `webhook.secret_key` is truthy (`:314`).
pub const HEADER_SIGNATURE: &str = "X-Pi Dash-Signature";

/// Fixed header values (`:285-290`).
pub const CONTENT_TYPE_JSON: &str = "application/json";
/// Fixed header values (`:285-290`).
pub const USER_AGENT: &str = "Autopilot";

/// Build the delivery headers in source order (`:285-290`): the delivery
/// id is fresh per attempt (`str(uuid.uuid4())`, `:288`), generated by
/// the caller.
pub fn base_headers(event: &str, delivery_id: &str) -> Map<String, Value> {
    let mut headers = Map::with_capacity(4);
    headers.insert(
        HEADER_CONTENT_TYPE.to_owned(),
        Value::String(CONTENT_TYPE_JSON.to_owned()),
    );
    headers.insert(
        HEADER_USER_AGENT.to_owned(),
        Value::String(USER_AGENT.to_owned()),
    );
    headers.insert(
        HEADER_DELIVERY.to_owned(),
        Value::String(delivery_id.to_owned()),
    );
    headers.insert(HEADER_EVENT.to_owned(), Value::String(event.to_owned()));
    headers
}

/// `requests.post(webhook.url, headers=headers, json=payload,
/// timeout=30)` (`:329`): connect/read timeout in seconds.
pub const WEBHOOK_POST_TIMEOUT_SECS: u64 = 30;

/// `save_webhook_log` field order (`:107-118`).
pub const LOG_FIELD_ORDER: [&str; 10] = [
    "workspace_id",
    "webhook",
    "event_type",
    "request_method",
    "request_headers",
    "request_body",
    "response_status",
    "response_headers",
    "response_body",
    "retry_count",
];

/// Mongo collection for the first log sink (`:105`).
pub const MONGO_LOG_COLLECTION: &str = "webhook_logs";
/// Postgres table for the fallback sink (`WebhookLog.Meta.db_table`).
pub const POSTGRES_LOG_TABLE: &str = "webhook_logs";

/// `WebhookLog` own columns in model order (fixture `postgres_columns.own`).
pub const WEBHOOK_LOG_COLUMNS: [&str; 10] = [
    "workspace_id",
    "webhook",
    "event_type",
    "request_method",
    "request_headers",
    "request_body",
    "response_status",
    "response_headers",
    "response_body",
    "retry_count",
];

/// Build the webhook log document (`log_data`, `:107-118`) in source
/// field order. QUIRK-1: every slot is `str()`-wrapped except
/// `retry_count`, so dicts render as Python `repr` (single quotes),
/// not JSON.
#[allow(clippy::too_many_arguments)]
pub fn build_log_doc(
    workspace_id: &str,
    webhook_id: &str,
    event_type: &str,
    request_method: &str,
    request_headers: &Value,
    request_body: &Value,
    response_status: impl std::fmt::Display,
    response_headers: &str,
    response_body: &str,
    retry_count: u32,
) -> Map<String, Value> {
    let mut doc = Map::with_capacity(10);
    doc.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    doc.insert("webhook".to_owned(), Value::String(webhook_id.to_owned()));
    doc.insert(
        "event_type".to_owned(),
        Value::String(event_type.to_owned()),
    );
    doc.insert(
        "request_method".to_owned(),
        Value::String(request_method.to_owned()),
    );
    doc.insert(
        "request_headers".to_owned(),
        Value::String(py_repr(request_headers)),
    );
    doc.insert(
        "request_body".to_owned(),
        Value::String(py_repr(request_body)),
    );
    doc.insert(
        "response_status".to_owned(),
        Value::String(response_status.to_string()),
    );
    doc.insert(
        "response_headers".to_owned(),
        Value::String(response_headers.to_owned()),
    );
    doc.insert(
        "response_body".to_owned(),
        Value::String(response_body.to_owned()),
    );
    doc.insert("retry_count".to_owned(), Value::from(retry_count));
    doc
}

/// Which sink a log write lands in (`:120-140`): mongo when the
/// collection exists *and* the insert succeeds, otherwise the postgres
/// fallback. Fallback failures are swallowed too (BUG-5), so this
/// returns a sink, never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSink {
    /// `mongo_collection.insert_one(log_data)` (`:124`).
    Mongo,
    /// `WebhookLog.objects.create(**log_data)` (`:136`).
    Postgres,
}

/// Route a log write to its sink (`:120-140`).
pub fn route_log_sink(mongo_collection_available: bool, mongo_write_ok: bool) -> LogSink {
    if mongo_collection_available && mongo_write_ok {
        LogSink::Mongo
    } else {
        LogSink::Postgres
    }
}

/// What the `except requests.RequestException` block does (`:344-370`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterFailure {
    /// Below max retries: log the failure and raise a bare
    /// `RequestException` so Celery autoretries (BUG-3: the original
    /// error is discarded).
    Retry,
    /// `self.request.retries >= self.max_retries`: deactivate the
    /// webhook and queue the deactivation email, then return.
    Deactivate,
}

/// Decide the failure path from the attempt's retry count (`:359`).
/// The comparison is `>=`, so an already-over-budget count still
/// deactivates instead of retrying forever.
pub fn after_failure(retries: u32) -> AfterFailure {
    if retries >= WEBHOOK_SEND_RETRY.max_retries {
        AfterFailure::Deactivate
    } else {
        AfterFailure::Retry
    }
}

/// The deactivation write (`:360`):
/// `Webhook.objects.filter(pk=webhook.id).update(is_active=False)` —
/// a filtered update with no `save()`, so `updated_at` is untouched
/// (BUG-4). Returns the `(set clause, filter)` pair.
pub fn deactivation_update() -> (&'static str, &'static str) {
    ("is_active=False", "pk=webhook.id")
}

/// `send_webhook_deactivation_email.delay(webhook_id, receiver_id,
/// reason, current_site)` kwargs (`:363-368`), in call order.
/// `receiver_id` is `webhook.created_by_id` and `reason` is `str(e)`.
pub fn deactivation_email_kwargs(
    webhook_id: &str,
    created_by_id: &str,
    reason: &str,
    current_site: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(4);
    kwargs.insert(
        "webhook_id".to_owned(),
        Value::String(webhook_id.to_owned()),
    );
    kwargs.insert(
        "receiver_id".to_owned(),
        Value::String(created_by_id.to_owned()),
    );
    kwargs.insert("reason".to_owned(), Value::String(reason.to_owned()));
    kwargs.insert(
        "current_site".to_owned(),
        Value::String(current_site.to_owned()),
    );
    kwargs
}

/// `webhook_send_task.delay(...)` kwargs in call order (`:436-451`).
#[allow(clippy::too_many_arguments)]
pub fn webhook_send_task_kwargs(
    webhook_id: &str,
    slug: &str,
    event: &str,
    event_data: Value,
    action: &str,
    current_site: &str,
    activity: Value,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert(
        "webhook_id".to_owned(),
        Value::String(webhook_id.to_owned()),
    );
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("event".to_owned(), Value::String(event.to_owned()));
    kwargs.insert("event_data".to_owned(), event_data);
    kwargs.insert("action".to_owned(), Value::String(action.to_owned()));
    kwargs.insert(
        "current_site".to_owned(),
        Value::String(current_site.to_owned()),
    );
    kwargs.insert("activity".to_owned(), activity);
    kwargs
}

/// First-attempt Celery message for the send task (`.delay(**kwargs)`).
pub fn webhook_send_task_message(
    webhook_id: &str,
    slug: &str,
    event: &str,
    event_data: Value,
    action: &str,
    current_site: &str,
    activity: Value,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        WEBHOOK_SEND_TASK_NAME,
        Vec::new(),
        webhook_send_task_kwargs(
            webhook_id,
            slug,
            event,
            event_data,
            action,
            current_site,
            activity,
        ),
    )
}

/// First-attempt Celery message for the deactivation email (`.delay(**kwargs)`).
pub fn deactivation_email_message(
    webhook_id: &str,
    created_by_id: &str,
    reason: &str,
    current_site: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        DEACTIVATION_EMAIL_TASK_NAME,
        Vec::new(),
        deactivation_email_kwargs(webhook_id, created_by_id, reason, current_site),
    )
}

/// Email subject (`:215`).
pub const DEACTIVATION_SUBJECT: &str = "Webhook Deactivated";
/// Notification template (`:224`).
pub const DEACTIVATION_TEMPLATE: &str = "emails/notifications/webhook-deactivate.html";

/// Email body message (`:216`):
/// `f"Webhook {webhook.url} has been deactivated due to failed requests."`.
pub fn deactivation_message(webhook_url: &str) -> String {
    format!("Webhook {webhook_url} has been deactivated due to failed requests.")
}

/// Webhook settings URL in the email context (`:222`):
/// `f"{current_site}/{workspace.slug}/settings/webhooks/{webhook.id}"`.
pub fn deactivation_webhook_url(
    current_site: &str,
    workspace_slug: &str,
    webhook_id: &str,
) -> String {
    format!("{current_site}/{workspace_slug}/settings/webhooks/{webhook_id}")
}

/// SMTP connection flags (`:234-235`): `use_tls=(EMAIL_USE_TLS == "1")`
/// — a plain string comparison, so only the exact string `"1"` enables
/// TLS/SSL.
pub fn email_tls_flags(email_use_tls: &str, email_use_ssl: &str) -> (bool, bool) {
    (email_use_tls == "1", email_use_ssl == "1")
}

/// SMTP port (`:231`): `int(EMAIL_PORT)`. Python's `int()` strips
/// surrounding whitespace; anything else raises inside the task's broad
/// `except` and the send is swallowed.
pub fn parse_email_port(port: &str) -> Result<i64, std::num::ParseIntError> {
    port.trim().parse::<i64>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_webhooks/fx-web-01-save-webhook-log.json`.
    static FIXTURE_LOG: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-web-01-save-webhook-log.json");
    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_webhooks/fx-web-03-webhook-send-task.json`.
    static FIXTURE_SEND: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-web-03-webhook-send-task.json");

    fn log_fixture() -> Value {
        serde_json::from_str(FIXTURE_LOG).expect("log fixture parses")
    }

    fn send_fixture() -> Value {
        serde_json::from_str(FIXTURE_SEND).expect("send fixture parses")
    }

    fn golden_headers() -> Value {
        json!({
            "Content-Type": "application/json",
            "User-Agent": "Autopilot",
            "X-Pi Dash-Delivery": "d-1",
            "X-Pi Dash-Event": "issue"
        })
    }

    fn golden_payload() -> Value {
        // Built in source key order (event, action, webhook_id,
        // workspace_id, data, activity): the parsed fixture object keeps
        // file order, which differs, so the repr input must be rebuilt.
        Value::Object(build_envelope(
            "issue",
            "create",
            "wid-1",
            "ws-1",
            Some(json!({"id": "iid-1"})),
            None,
        ))
    }

    #[test]
    fn log_doc_replays_mongo_golden() {
        let golden = &log_fixture()["activity"]["mongo_doc_golden"];
        let doc = build_log_doc(
            "ws-1",
            "wid-1",
            "issue",
            "create",
            &golden_headers(),
            &golden_payload(),
            200,
            "{}",
            "ok",
            0,
        );
        assert_eq!(Value::Object(doc), *golden);
    }

    #[test]
    fn log_doc_slots_are_repr_not_json() {
        // QUIRK-1: dicts land as Python repr (single quotes), None as
        // `None` — never JSON.
        let doc = build_log_doc(
            "ws-1",
            "wid-1",
            "issue",
            "create",
            &golden_headers(),
            &golden_payload(),
            200,
            "{}",
            "ok",
            0,
        );
        let body = doc["request_body"].as_str().expect("body is text");
        assert!(body.contains("'event': 'issue'"), "repr quotes: {body}");
        assert!(
            body.ends_with("'data': {'id': 'iid-1'}, 'activity': None}"),
            "None repr: {body}"
        );
        assert!(!body.contains("\"event\""), "never JSON: {body}");
    }

    #[test]
    fn log_doc_field_order_and_column_specs() {
        let doc = build_log_doc(
            "ws-1",
            "wid-1",
            "issue",
            "create",
            &golden_headers(),
            &golden_payload(),
            200,
            "{}",
            "ok",
            0,
        );
        let order: Vec<&str> = doc.keys().map(String::as_str).collect();
        let parsed_log = log_fixture();
        let fixture_order: Vec<&str> = parsed_log["activity"]["field_order"]
            .as_array()
            .expect("field order")
            .iter()
            .map(|field| field.as_str().expect("field name"))
            .collect();
        assert_eq!(order, LOG_FIELD_ORDER);
        assert_eq!(order, fixture_order);
        // retry_count stays raw; every other slot is text.
        assert_eq!(doc["retry_count"], json!(0));
        for field in LOG_FIELD_ORDER
            .iter()
            .filter(|field| **field != "retry_count")
        {
            assert!(doc[*field].is_string(), "{field} is str()-wrapped");
        }
        // Column list matches the WebhookLog model columns.
        let parsed_cols = log_fixture();
        let own: Vec<&str> = parsed_cols["postgres_columns"]["own"]
            .as_array()
            .expect("own columns")
            .iter()
            .map(|column| {
                column
                    .as_str()
                    .expect("column spec")
                    .split(" (")
                    .next()
                    .expect("column name")
            })
            .collect();
        assert_eq!(own, WEBHOOK_LOG_COLUMNS);
        assert_eq!(
            log_fixture()["activity"]["mongo_collection"],
            json!(MONGO_LOG_COLLECTION)
        );
    }

    #[test]
    fn log_sink_routing_prefers_mongo_falls_back_to_postgres() {
        assert_eq!(route_log_sink(true, true), LogSink::Mongo);
        assert_eq!(route_log_sink(false, true), LogSink::Postgres);
        assert_eq!(route_log_sink(true, false), LogSink::Postgres);
        assert_eq!(route_log_sink(false, false), LogSink::Postgres);
        assert_eq!(
            log_fixture()["branches"]
                .as_array()
                .map(|branches| branches.len()),
            Some(2)
        );
    }

    #[test]
    fn failure_log_row_uses_int_status_and_empty_headers() {
        // `result_rows.failure_log`: status 500 as INT (str-wrapped to
        // "500" by QUIRK-1), empty headers, str(exception) body, live
        // retry count.
        let doc = build_log_doc(
            "ws-1",
            "wid-1",
            "issue",
            "create",
            &golden_headers(),
            &golden_payload(),
            500,
            "",
            "connection refused",
            3,
        );
        assert_eq!(doc["response_status"], json!("500"));
        assert_eq!(doc["response_headers"], json!(""));
        assert_eq!(doc["response_body"], json!("connection refused"));
        assert_eq!(doc["retry_count"], json!(3));
    }

    #[test]
    fn action_map_with_passthrough() {
        let fixture = send_fixture();
        assert_eq!(map_action("POST"), "create");
        assert_eq!(map_action("PATCH"), "update");
        assert_eq!(map_action("PUT"), "update");
        assert_eq!(map_action("DELETE"), "delete");
        assert_eq!(map_action("GET"), "GET");
        for (verb, mapped) in [
            ("POST", "create"),
            ("PATCH", "update"),
            ("PUT", "update"),
            ("DELETE", "delete"),
            ("GET", "GET"),
        ] {
            assert_eq!(fixture["action_map"][verb], json!(mapped), "verb {verb}");
        }
    }

    #[test]
    fn envelope_replays_golden_in_key_order() {
        let fixture = send_fixture();
        let envelope = build_envelope(
            "issue",
            "create",
            "wid-1",
            "ws-1",
            Some(json!({"id": "iid-1"})),
            None,
        );
        assert_eq!(
            Value::Object(envelope.clone()),
            fixture["envelope"]["golden"]
        );
        let order: Vec<&str> = envelope.keys().map(String::as_str).collect();
        let fixture_order: Vec<&str> = fixture["envelope"]["key_order"]
            .as_array()
            .expect("key order")
            .iter()
            .map(|key| key.as_str().expect("key"))
            .collect();
        assert_eq!(order, ENVELOPE_KEY_ORDER);
        assert_eq!(order, fixture_order);
    }

    #[test]
    fn signature_input_and_sample_hex_are_byte_identical() {
        // BUG-1 pin: default-separator dumps, then HMAC-SHA256.
        let fixture = send_fixture();
        let envelope = build_envelope(
            "issue",
            "create",
            "wid-1",
            "ws-1",
            Some(json!({"id": "iid-1"})),
            None,
        );
        let wire = render_signature_input(&Value::Object(envelope));
        assert_eq!(wire, fixture["hmac"]["json_wire"].as_str().expect("wire"));
        assert_eq!(
            sign_payload_hex("s3cret", &wire),
            fixture["hmac"]["sample_sha256_hex"].as_str().expect("hex")
        );
    }

    #[test]
    fn signature_absent_without_secret_key() {
        // `:314`: the header is set only when `webhook.secret_key` is truthy.
        let mut headers = base_headers("issue", "d-1");
        assert!(!headers.contains_key(HEADER_SIGNATURE));
        headers.insert(
            HEADER_SIGNATURE.to_owned(),
            Value::String(sign_payload_hex("s3cret", "{}")),
        );
        assert!(headers.contains_key(HEADER_SIGNATURE));
        let base = base_headers("issue", "d-1");
        let order: Vec<&str> = base.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                HEADER_CONTENT_TYPE,
                HEADER_USER_AGENT,
                HEADER_DELIVERY,
                HEADER_EVENT
            ]
        );
    }

    #[test]
    fn task_options_match_source_decorators() {
        let fixture = send_fixture();
        assert_eq!(
            WEBHOOK_SEND_RETRY.autoretry_for,
            "requests.RequestException"
        );
        assert_eq!(WEBHOOK_SEND_RETRY.backoff_secs, 600);
        assert_eq!(WEBHOOK_SEND_RETRY.max_retries, 5);
        assert_eq!(
            WEBHOOK_SEND_RETRY.bind,
            fixture["task_options"]["bind"].as_bool().unwrap()
        );
        assert_eq!(
            WEBHOOK_SEND_RETRY.jitter,
            fixture["task_options"]["retry_jitter"].as_bool().unwrap()
        );
        assert_eq!(fixture["task_options"]["max_retries"], json!(5));
        assert_eq!(fixture["task_options"]["retry_backoff"], json!(600));
        assert!(fixture["task_options"]["retry_jitter"].as_bool().unwrap());
        assert!(fixture["task_options"]["bind"].as_bool().unwrap());
    }

    #[test]
    fn retry_boundary_deactivates_at_max() {
        for retries in 0..5 {
            assert_eq!(
                after_failure(retries),
                AfterFailure::Retry,
                "retries={retries}"
            );
        }
        assert_eq!(after_failure(5), AfterFailure::Deactivate);
        assert_eq!(after_failure(6), AfterFailure::Deactivate);
        let (set_clause, filter) = deactivation_update();
        assert_eq!(set_clause, "is_active=False");
        assert_eq!(filter, "pk=webhook.id");
        assert!(send_fixture()["retry"]["max_reached"]
            .as_str()
            .expect("max_reached")
            .contains("update(is_active=False)"));
    }

    #[test]
    fn deactivation_email_builders_match_fixture() {
        let parsed = send_fixture();
        let fixture = &parsed["deactivation_email_task"];
        assert_eq!(DEACTIVATION_SUBJECT, "Webhook Deactivated");
        assert_eq!(fixture["subject"], json!(DEACTIVATION_SUBJECT));
        assert_eq!(
            deactivation_message("https://hooks.example/w1"),
            "Webhook https://hooks.example/w1 has been deactivated due to failed requests."
        );
        assert_eq!(
            deactivation_webhook_url("https://app.example", "ws-slug", "wid-1"),
            "https://app.example/ws-slug/settings/webhooks/wid-1"
        );
        assert_eq!(
            DEACTIVATION_TEMPLATE,
            "emails/notifications/webhook-deactivate.html"
        );
        assert_eq!(email_tls_flags("1", "0"), (true, false));
        assert_eq!(email_tls_flags("0", "0"), (false, false));
        assert_eq!(parse_email_port("587").unwrap(), 587);
        assert!(parse_email_port("not-a-port").is_err());
        assert_eq!(parse_email_port(" 587 ").unwrap(), 587);
        assert!(fixture["connection"]
            .as_str()
            .unwrap()
            .contains("use_tls=(EMAIL_USE_TLS == '1')"));
    }

    #[test]
    fn delay_messages_carry_kwargs_in_call_order() {
        let send = webhook_send_task_message(
            "wid-1",
            "ws",
            "issue",
            json!({"id": "iid-1"}),
            "create",
            "https://app.example",
            Value::Null,
        );
        assert_eq!(send.task, WEBHOOK_SEND_TASK_NAME);
        assert!(send.args.is_empty());
        let order: Vec<&str> = send.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "webhook_id",
                "slug",
                "event",
                "event_data",
                "action",
                "current_site",
                "activity"
            ]
        );

        let email = deactivation_email_message("wid-1", "uid-9", "boom", "https://app.example");
        assert_eq!(email.task, DEACTIVATION_EMAIL_TASK_NAME);
        assert!(email.args.is_empty());
        let order: Vec<&str> = email.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            ["webhook_id", "receiver_id", "reason", "current_site"]
        );
        assert_eq!(email.kwargs["receiver_id"], json!("uid-9"));
    }

    #[test]
    fn task_names_registered_with_send_retry_only() {
        assert!(is_webhook_task(WEBHOOK_SEND_TASK_NAME));
        assert!(is_webhook_task(DEACTIVATION_EMAIL_TASK_NAME));
        assert!(!is_webhook_task(
            "pi_dash.bgtasks.webhook_task.webhook_activity"
        ));
        assert_eq!(WEBHOOK_TASK_NAMES.len(), 2);
        let retry = retry_spec_for(WEBHOOK_SEND_TASK_NAME).expect("send task retry");
        assert_eq!(
            (retry.max_retries, retry.backoff_secs, retry.jitter),
            (5, 600, true)
        );
        // The email task is a plain @shared_task in the source: no retry spec.
        assert!(retry_spec_for(DEACTIVATION_EMAIL_TASK_NAME).is_none());
        assert_eq!(WEBHOOK_POST_TIMEOUT_SECS, 30);
        assert_eq!(
            send_fixture()["request"].as_str().unwrap(),
            "requests.post(webhook.url, headers=headers, json=payload, timeout=30)"
        );
    }

    #[test]
    fn float_wire_matches_python_repr() {
        assert_eq!(render_signature_input(&json!(1.0)), "1.0");
        assert_eq!(render_signature_input(&json!(1.5)), "1.5");
        assert_eq!(render_signature_input(&json!(1e16)), "1e+16");
        assert_eq!(render_signature_input(&json!(42)), "42");
    }

    #[test]
    fn float_e05_decade_matches_python_repr() {
        // Python switches to exponent form below 1e-4 (`1e-05`) while
        // `serde_json` prints that decade fixed (`0.00001`); every
        // expectation below is `json.dumps` output from CPython.
        for (value, wire) in [
            (1e-5, "1e-05"),
            (1.5e-5, "1.5e-05"),
            (9.999e-5, "9.999e-05"),
            (1.00001e-5, "1.00001e-05"),
            (-1e-5, "-1e-05"),
            (1e-4, "0.0001"),
            (1.5e-4, "0.00015"),
            (1e-6, "1e-06"),
        ] {
            assert_eq!(render_signature_input(&json!(value)), wire, "{value}");
        }
    }

    #[test]
    fn signature_input_escapes_like_ensure_ascii() {
        assert_eq!(render_signature_input(&json!("a\"b")), "\"a\\\"b\"");
        assert_eq!(
            render_signature_input(&json!("caf\u{e9}")),
            "\"caf\\u00e9\""
        );
        assert_eq!(render_signature_input(&json!("a/b")), "\"a/b\"");
    }
}
