#![forbid(unsafe_code)]

//! Celery protocol v2 message format.
//!
//! Mirrors what the Python workers speak (Celery 5.x, JSON serializer,
//! protocol 2 — see `CELERY_TASK_SERIALIZER` in
//! `apps/api/pi_dash/settings/common.py` and the `beat_schedule` in
//! `apps/api/pi_dash/celery.py`).
//!
//! A published task is a JSON body of `[args, kwargs, embed]` sent with
//! headers carrying identity (`id`, `task`, `root_id`, `parent_id`),
//! scheduling (`eta`, `expires`, `timelimit`), delivery bookkeeping
//! (`retries`, `lang`, `argsrepr`, `kwargsrepr`, `origin`) and the
//! protocol-v2 extension keys newer kombu versions add (`group_index`,
//! `ignore_result`, `replaced_task_nesting`, `stamps`, `stamped_headers`).
//! The headers are a superset of what any single kombu version emits:
//! older consumers ignore unknown keys while newer ones require theirs,
//! so the superset keeps both directions working during coexistence.
//!
//! Reference oracle: `app.amqp.create_task_message(...)` from kombu, as
//! exercised by `rust-api/contract-tests/_harness/celery_wire.py`. The
//! nine header keys that harness asserts (`lang`, `task`, `id`, `eta`,
//! `expires`, `retries`, `timelimit`, `root_id`, `parent_id`) are always
//! present here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// AMQP content type for every published body (mirrors the harness
/// `broker.py` pika properties and kombu's JSON serializer output).
pub const CONTENT_TYPE: &str = "application/json";
/// AMQP content encoding for every published body.
pub const CONTENT_ENCODING: &str = "utf-8";

/// A task message ready to publish to the broker.
///
/// Field-for-field this is kombu's `as_task_v2` without the transport:
/// `new` mirrors `.delay()` (fresh id, first attempt), while retry and
/// scheduled dispatch set [`CeleryTaskMessage::eta`] /
/// [`CeleryTaskMessage::retries`] explicitly like `apply_async` does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CeleryTaskMessage {
    pub id: String,
    pub task: String,
    pub args: Vec<Value>,
    pub kwargs: Map<String, Value>,
    pub retries: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,
    #[serde(default)]
    pub timelimit: (Option<u64>, Option<u64>),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl CeleryTaskMessage {
    /// Create a first-attempt message with a fresh id (the `.delay()` path).
    pub fn new(task: impl Into<String>, args: Vec<Value>, kwargs: Map<String, Value>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task: task.into(),
            args,
            kwargs,
            retries: 0,
            eta: None,
            expires: None,
            timelimit: (None, None),
            root_id: None,
            parent_id: None,
            origin: None,
        }
    }

    /// Schedule for no earlier than `eta` (the `countdown`/`eta` path of
    /// `apply_async`: kombu converts `countdown` to an absolute `eta`).
    pub fn with_eta(mut self, eta: DateTime<Utc>) -> Self {
        self.eta = Some(eta);
        self
    }

    /// Mark as attempt number `retries` (kombu resends the same id with a
    /// bumped counter; `root_id` still points at the first attempt).
    pub fn with_retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    /// Effective root id: the chain head, or this message's own id for a
    /// first attempt (kombu sets `root_id = id` when there is no parent).
    pub fn effective_root_id(&self) -> &str {
        self.root_id.as_deref().unwrap_or(&self.id)
    }

    /// AMQP headers Celery workers expect: the full kombu protocol-v2 set.
    pub fn headers(&self) -> Map<String, Value> {
        let mut headers = Map::new();
        headers.insert("lang".to_owned(), Value::String("py".to_owned()));
        headers.insert("task".to_owned(), Value::String(self.task.clone()));
        headers.insert("id".to_owned(), Value::String(self.id.clone()));
        headers.insert(
            "root_id".to_owned(),
            Value::String(self.effective_root_id().to_owned()),
        );
        headers.insert(
            "parent_id".to_owned(),
            self.parent_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        headers.insert("group".to_owned(), Value::Null);
        headers.insert("group_index".to_owned(), Value::Null);
        headers.insert("meth".to_owned(), Value::Null);
        headers.insert("shadow".to_owned(), Value::Null);
        headers.insert(
            "eta".to_owned(),
            self.eta
                .map(|eta| Value::String(format_eta(eta)))
                .unwrap_or(Value::Null),
        );
        headers.insert(
            "expires".to_owned(),
            self.expires
                .map(|expires| Value::String(format_eta(expires)))
                .unwrap_or(Value::Null),
        );
        headers.insert("retries".to_owned(), Value::Number(self.retries.into()));
        headers.insert(
            "timelimit".to_owned(),
            Value::Array(vec![opt_u64(self.timelimit.0), opt_u64(self.timelimit.1)]),
        );
        headers.insert("argsrepr".to_owned(), Value::String(tuple_repr(&self.args)));
        headers.insert(
            "kwargsrepr".to_owned(),
            Value::String(dict_repr(&self.kwargs)),
        );
        headers.insert(
            "origin".to_owned(),
            Value::String(self.origin.clone().unwrap_or_else(default_origin)),
        );
        headers.insert("ignore_result".to_owned(), Value::Bool(false));
        headers.insert("replaced_task_nesting".to_owned(), Value::Number(0.into()));
        headers.insert("stamps".to_owned(), Value::Object(Map::new()));
        headers.insert("stamped_headers".to_owned(), Value::Null);
        headers
    }

    /// Protocol v2 body: `[args, kwargs, embed]`, where the embed carries
    /// the canvas pointers (all null outside a chain/group/chord, exactly
    /// as kombu emits them).
    pub fn body(&self) -> Value {
        let mut embed = Map::new();
        embed.insert("callbacks".to_owned(), Value::Null);
        embed.insert("errbacks".to_owned(), Value::Null);
        embed.insert("chain".to_owned(), Value::Null);
        embed.insert("chord".to_owned(), Value::Null);
        Value::Array(vec![
            Value::Array(self.args.clone()),
            Value::Object(self.kwargs.clone()),
            Value::Object(embed),
        ])
    }

    /// The AMQP message properties (mirrors the pika properties in the
    /// contract harness `broker.py`: persistent JSON, correlated by id).
    pub fn properties(&self) -> AmqpProperties {
        AmqpProperties {
            content_type: CONTENT_TYPE.to_owned(),
            content_encoding: CONTENT_ENCODING.to_owned(),
            correlation_id: self.id.clone(),
            delivery_mode: 2,
        }
    }

    /// The full wire payload: headers plus the serialized body.
    pub fn to_wire(&self) -> (Map<String, Value>, Vec<u8>) {
        let body = serde_json::to_vec(&self.body()).expect("body is JSON");
        (self.headers(), body)
    }
}

/// AMQP content properties for a published task body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmqpProperties {
    pub content_type: String,
    pub content_encoding: String,
    pub correlation_id: String,
    /// 2 = persistent, so a broker restart does not drop tasks.
    pub delivery_mode: u8,
}

/// Format a timestamp the way kombu renders `eta`/`expires`: ISO-8601 with
/// an explicit `+00:00` offset (never `Z`), fractional seconds only when
/// nonzero (mirrors Python `datetime.isoformat`, which always uses six
/// fraction digits when they are present).
pub fn format_eta(eta: DateTime<Utc>) -> String {
    let base = eta.format("%Y-%m-%dT%H:%M:%S").to_string();
    if eta.timestamp_subsec_nanos() == 0 {
        format!("{base}+00:00")
    } else {
        // Truncated to microseconds: Python datetimes cannot represent
        // finer precision, so this matches what kombu would have sent.
        format!("{base}.{:06}+00:00", eta.timestamp_subsec_micros())
    }
}

/// Default publisher node name in the `origin` header. Kombu fills in
/// `<counter>@<hostname>`; the hostname is deployment-specific, so the
/// value is informational only and consumers never route on it.
fn default_origin() -> String {
    "pidash-rust".to_owned()
}

fn opt_u64(value: Option<u64>) -> Value {
    value
        .map(|v| Value::Number(v.into()))
        .unwrap_or(Value::Null)
}

/// Python `repr` of one JSON value, as kombu's `saferepr` renders the
/// `argsrepr`/`kwargsrepr` headers: `None`/`True`/`False`, single-quoted
/// strings, `[...]` lists and `{'k': v}` dicts.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => py_str(s),
        Value::Array(items) => {
            let inner = items.iter().map(py_repr).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Object(map) => dict_repr(map),
    }
}

/// Python `repr` of a dict: `{'key': value}` with insertion-ordered keys.
pub fn dict_repr(map: &Map<String, Value>) -> String {
    let inner = map
        .iter()
        .map(|(k, v)| format!("{}: {}", py_str(k), py_repr(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

/// Python `repr` of the positional args tuple: `()`, `(one,)` or `(a, b)`.
/// Kombu always passes a tuple (`.delay(*args)`), so the empty case is
/// `()` even though the body carries a JSON list.
fn tuple_repr(args: &[Value]) -> String {
    match args {
        [] => "()".to_owned(),
        [single] => format!("({},)", py_repr(single)),
        _ => {
            let inner = args.iter().map(py_repr).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
    }
}

/// Python `repr` of a string: single-quoted with backslash escapes.
fn py_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn message() -> CeleryTaskMessage {
        CeleryTaskMessage {
            id: "task-id-1".to_owned(),
            task: "pi_dash.bgtasks.agent_ticker.scan_due_tickers".to_owned(),
            args: vec![json!(1)],
            kwargs: Map::new(),
            retries: 0,
            eta: None,
            expires: None,
            timelimit: (None, None),
            root_id: None,
            parent_id: None,
            origin: None,
        }
    }

    #[test]
    fn headers_carry_oracle_required_keys() {
        // The exact set `contract-tests/_harness/celery_wire.py` asserts.
        let headers = message().headers();
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
            assert!(headers.contains_key(key), "missing header {key}");
        }
        assert_eq!(headers["lang"], "py");
        assert_eq!(
            headers["task"],
            "pi_dash.bgtasks.agent_ticker.scan_due_tickers"
        );
        assert_eq!(headers["id"], "task-id-1");
        assert_eq!(headers["root_id"], "task-id-1");
        assert!(headers["parent_id"].is_null());
        assert!(headers["eta"].is_null());
        assert!(headers["expires"].is_null());
        assert_eq!(headers["retries"], 0);
        assert_eq!(
            headers["timelimit"],
            Value::Array(vec![Value::Null, Value::Null])
        );
    }

    #[test]
    fn headers_match_kombu_reference_capture() {
        // Values transcribed from `app.amqp.create_task_message` (kombu
        // 5.6.3) for an empty-args first attempt.
        let msg = CeleryTaskMessage {
            args: vec![],
            ..message()
        };
        let headers = msg.headers();
        assert_eq!(headers["argsrepr"], "()");
        assert_eq!(headers["kwargsrepr"], "{}");
        assert_eq!(headers["group"], Value::Null);
        assert_eq!(headers["ignore_result"], false);
        assert_eq!(headers["replaced_task_nesting"], 0);
        assert_eq!(headers["stamps"], json!({}));
    }

    #[test]
    fn countdown_eta_renders_with_offset() {
        let eta = Utc.with_ymd_and_hms(2026, 9, 26, 19, 55, 30).unwrap()
            + chrono::Duration::microseconds(173352);
        let headers = message().with_eta(eta).headers();
        assert_eq!(headers["eta"], "2026-09-26T19:55:30.173352+00:00");
    }

    #[test]
    fn eta_keeps_six_fraction_digits_like_cpython() {
        // Python `isoformat` renders exact-millisecond times with six
        // digits (`.173000`); chrono's `AutoSi` would trim to `.173`.
        let eta = Utc.with_ymd_and_hms(2026, 9, 26, 19, 55, 30).unwrap()
            + chrono::Duration::milliseconds(173);
        assert_eq!(format_eta(eta), "2026-09-26T19:55:30.173000+00:00");
        let whole = Utc.with_ymd_and_hms(2026, 9, 26, 19, 55, 30).unwrap();
        assert_eq!(format_eta(whole), "2026-09-26T19:55:30+00:00");
    }

    fn embed_triple() -> Value {
        let mut embed = Map::new();
        embed.insert("callbacks".to_owned(), Value::Null);
        embed.insert("errbacks".to_owned(), Value::Null);
        embed.insert("chain".to_owned(), Value::Null);
        embed.insert("chord".to_owned(), Value::Null);
        Value::Array(vec![json!([1]), json!({}), Value::Object(embed)])
    }

    #[test]
    fn body_is_args_kwargs_embed_quadruple() {
        assert_eq!(message().body(), embed_triple());
    }

    #[test]
    fn wire_body_round_trips() {
        let (headers, bytes) = message().to_wire();
        assert_eq!(headers["id"], "task-id-1");
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body, embed_triple());
    }

    #[test]
    fn properties_are_persistent_json() {
        let props = message().properties();
        assert_eq!(props.content_type, "application/json");
        assert_eq!(props.content_encoding, "utf-8");
        assert_eq!(props.correlation_id, "task-id-1");
        assert_eq!(props.delivery_mode, 2);
    }

    #[test]
    fn retry_keeps_id_and_bumps_counter() {
        let retry = message().with_retries(2);
        assert_eq!(retry.id, "task-id-1");
        assert_eq!(retry.headers()["retries"], 2);
        assert_eq!(retry.effective_root_id(), "task-id-1");
    }

    #[test]
    fn new_assigns_unique_ids() {
        let first = CeleryTaskMessage::new("t", vec![], Map::new());
        let second = CeleryTaskMessage::new("t", vec![], Map::new());
        assert_ne!(first.id, second.id);
        assert_eq!(first.retries, 0);
    }

    #[test]
    fn kwargsrepr_keeps_python_insertion_order() {
        // Verified against kombu: `{'b': 2, 'a': 1}` renders in insertion
        // order, not sorted. Requires serde_json `preserve_order`.
        let mut kwargs = Map::new();
        kwargs.insert("b".to_owned(), json!(2));
        kwargs.insert("a".to_owned(), json!(1));
        assert_eq!(dict_repr(&kwargs), "{'b': 2, 'a': 1}");
    }

    #[test]
    fn python_repr_matches_cpython_for_json_values() {
        assert_eq!(py_repr(&Value::Null), "None");
        assert_eq!(py_repr(&json!(true)), "True");
        assert_eq!(py_repr(&json!(1)), "1");
        assert_eq!(py_repr(&json!("a'b")), "'a\\'b'");
        assert_eq!(py_repr(&json!("a\nb")), "'a\\nb'");
        assert_eq!(py_repr(&json!([1, "x"])), "[1, 'x']");
        assert_eq!(py_repr(&json!({"a": 2})), "{'a': 2}");
        assert_eq!(tuple_repr(&[]), "()");
        assert_eq!(tuple_repr(&[json!(1)]), "(1,)");
        assert_eq!(tuple_repr(&[json!(1), json!("x")]), "(1, 'x')");
    }
}
