//! D-14 control-message envelopes + wire frames (pure, no I/O).
//!
//! Port of the envelope layer shared by both outboxes:
//!
//! * `_VALID_TYPES` / `_OFFLINE_REJECT` (`outbox.py:38-67`,
//!   `machine_outbox.py:49-65`)
//! * `_serialize` / `_decode_read_result` (`outbox.py:120-136,
//!   :164-189`, reused by the machine outbox per
//!   `machine_outbox.py:23-25,37-40`)
//! * `_ensure_envelope` (`pubsub.py:39-42`)
//! * `_build_assign_msg` (`matcher.py:306-330`)
//! * resume_ack / cancel variants (`session_service.py:473-508`) and the
//!   cancel-first redeliver frame (`session_service.py:431-437`)
//! * `revoke` / `remove_runner` frames (`pubsub.py:110-171`) and the
//!   session-eviction pub/sub body (`outbox.py:544-556`,
//!   `machine_outbox.py:373-385`)
//!
//! Two generated-`mid` call sites (`_serialize`, `_ensure_envelope`) take
//! the fresh id as a parameter instead of minting it: this crate performs
//! no I/O and its `uuid` dependency has no `v4` feature, so callers pass
//! `Uuid::new_v4().to_string()` (the `services`/`api`/`db` crates all
//! enable `v4`). The mid-selection rules themselves are exact.
//!
//! Fixture replayed by the unit tests below:
//! `rust-api/fixtures/runner_sessions/fx-rses-03-envelopes.json`
//! (FX-RSES-03).

use serde_json::{Map, Value};

use super::{render_compact_utf8, render_spaced_ascii};

/// Live per-runner message types (`outbox.py:38-53`). Anything else is
/// rejected by `enqueue_for_runner` with `ValueError`.
pub const VALID_TYPES_RUNNER: &[&str] = &[
    "assign",
    "cancel",
    "chat_cancel",
    "chat_close",
    "chat_decide",
    "chat_user_message",
    "chat_warm",
    "config_push",
    "decide",
    "force_refresh",
    "remove_runner",
    "resume_ack",
    "revoke",
    "welcome",
];

/// Per-runner types that raise `RunnerOfflineError` instead of queueing
/// offline (`outbox.py:57-67`).
pub const OFFLINE_REJECT_RUNNER: &[&str] = &[
    "assign",
    "cancel",
    "chat_cancel",
    "chat_close",
    "chat_decide",
    "chat_user_message",
    "chat_warm",
    "decide",
    "resume_ack",
];

/// Live machine-scoped message types (`machine_outbox.py:49-54`).
pub const VALID_TYPES_MACHINE: &[&str] = &["config_push", "create_runner", "ping", "welcome"];

/// Machine-scoped types that raise `MachineOfflineError` instead of
/// queueing offline (`machine_outbox.py:62-65`).
pub const OFFLINE_REJECT_MACHINE: &[&str] = &["config_push", "create_runner"];

/// Whether `msg_type` is enqueueable on the per-runner outbox.
pub fn is_valid_runner_type(msg_type: &str) -> bool {
    VALID_TYPES_RUNNER.contains(&msg_type)
}

/// Whether `msg_type` must not queue offline on the per-runner outbox.
pub fn is_offline_reject_runner_type(msg_type: &str) -> bool {
    OFFLINE_REJECT_RUNNER.contains(&msg_type)
}

/// Whether `msg_type` is enqueueable on the machine outbox.
pub fn is_valid_machine_type(msg_type: &str) -> bool {
    VALID_TYPES_MACHINE.contains(&msg_type)
}

/// Whether `msg_type` must not queue offline on the machine outbox.
pub fn is_offline_reject_machine_type(msg_type: &str) -> bool {
    OFFLINE_REJECT_MACHINE.contains(&msg_type)
}

/// Python truthiness for a JSON value, as `message.get("mid") or …`
/// sees it.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `str()` for the JSON-native scalars `_serialize` stringifies.
///
/// Containers never reach this position from a real call site (every
/// Python caller passes a string mid or none); they render as compact
/// JSON as a documented stand-in for `repr()`.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                super::render_float_py(f, false)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => render_compact_utf8(value),
    }
}

/// The `{mid, type, payload}` field shape `XADD` wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedMessage {
    /// `str(message.get("mid") or uuid4())`.
    pub mid: String,
    /// `str(body.get("type") or "")`.
    pub msg_type: String,
    /// `json.dumps(body)` with default separators.
    pub payload: String,
}

/// Encode a control message into the `XADD` field shape
/// (`outbox.py:120-136`).
///
/// `fresh_mid` is the caller's fresh uuid4 string, used exactly when
/// Python would mint one (missing or falsy `mid`). The body keeps the
/// message's key order with `mid` replaced in place (or appended);
/// a missing `type` is NOT added to the body — only surfaced as an
/// empty `msg_type` (verbatim port of the `missing_type` vector).
pub fn serialize(message: &Map<String, Value>, fresh_mid: &str) -> SerializedMessage {
    let mid = match message.get("mid") {
        Some(value) if is_truthy(value) => python_str(value),
        _ => fresh_mid.to_string(),
    };
    let mut body = message.clone();
    body.insert("mid".to_string(), Value::String(mid.clone()));
    let msg_type = match body.get("type") {
        Some(value) if is_truthy(value) => python_str(value),
        _ => String::new(),
    };
    SerializedMessage {
        mid,
        msg_type,
        payload: render_spaced_ascii(&Value::Object(body)),
    }
}

/// One stream's entries from an `XREADGROUP` reply, pre-decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEntry {
    /// Raw stream id (`bytes` in redis-py).
    pub id: Vec<u8>,
    /// Raw field pairs (`{bytes: bytes}` in redis-py).
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
}

/// One stream's section of an `XREADGROUP` reply, pre-decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamRead {
    /// Raw stream name (ignored, like Python's `_`).
    pub name: Vec<u8>,
    /// Raw entries.
    pub entries: Vec<StreamEntry>,
}

/// Decode failure. Python lets `UnicodeDecodeError` propagate out of
/// `_decode_read_result` (the `.decode()` calls sit outside its
/// `try`), and raises `AttributeError` when a non-object JSON payload
/// forces a `body.get` fallback; both surface here as `Err` while a
/// corrupt payload still decodes to `{}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A stream id, field name or field value is not valid UTF-8.
    InvalidUtf8(std::str::Utf8Error),
    /// The payload parses to non-object JSON while `mid`/`type` need
    /// the body fallback.
    BodyNotObject,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::InvalidUtf8(err) => write!(f, "invalid utf-8: {err}"),
            DecodeError::BodyNotObject => {
                write!(f, "payload is not a JSON object and mid/type are missing")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// One decoded poll entry, in wire key order
/// (`stream_id`, `mid`, `type`, `body`).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedMessage {
    pub stream_id: String,
    pub mid: String,
    pub msg_type: String,
    pub body: Value,
}

impl DecodedMessage {
    /// The entry as a wire-ordered JSON object for the poll envelope.
    pub fn to_json_value(&self) -> Value {
        let mut map = Map::with_capacity(4);
        map.insert(
            "stream_id".to_string(),
            Value::String(self.stream_id.clone()),
        );
        map.insert("mid".to_string(), Value::String(self.mid.clone()));
        map.insert("type".to_string(), Value::String(self.msg_type.clone()));
        map.insert("body".to_string(), self.body.clone());
        Value::Object(map)
    }
}

/// Decode an `XREADGROUP` reply into the entry dicts the poll returns
/// (`outbox.py:164-189`).
///
/// `None` (and an empty reply) decodes to no entries. Field decoding is
/// last-wins like Python's `decoded` dict; a corrupt payload decodes to
/// `{}`; missing `mid`/`type` fields fall back to the parsed body.
pub fn decode_read_result(
    result: Option<&[StreamRead]>,
) -> Result<Vec<DecodedMessage>, DecodeError> {
    let mut out = Vec::new();
    let Some(streams) = result else {
        return Ok(out);
    };
    for stream in streams {
        for entry in &stream.entries {
            let stream_id = std::str::from_utf8(&entry.id).map_err(DecodeError::InvalidUtf8)?;
            let mut mid: Option<&str> = None;
            let mut msg_type: Option<&str> = None;
            let mut payload: Option<&str> = None;
            for (key, value) in &entry.fields {
                let key = std::str::from_utf8(key).map_err(DecodeError::InvalidUtf8)?;
                let value = std::str::from_utf8(value).map_err(DecodeError::InvalidUtf8)?;
                match key {
                    "mid" => mid = Some(value),
                    "type" => msg_type = Some(value),
                    "payload" => payload = Some(value),
                    _ => {}
                }
            }
            let body: Value =
                serde_json::from_str(payload.filter(|p| !p.is_empty()).unwrap_or("{}"))
                    .unwrap_or_else(|_| Value::Object(Map::new()));
            let fallback = |key: &str| -> Result<String, DecodeError> {
                match &body {
                    Value::Object(obj) => match obj.get(key) {
                        Some(value) if is_truthy(value) => Ok(python_str(value)),
                        _ => Ok(String::new()),
                    },
                    _ => Err(DecodeError::BodyNotObject),
                }
            };
            let mid = match mid.filter(|m| !m.is_empty()) {
                Some(m) => m.to_string(),
                None => fallback("mid")?,
            };
            let msg_type = match msg_type.filter(|t| !t.is_empty()) {
                Some(t) => t.to_string(),
                None => fallback("type")?,
            };
            out.push(DecodedMessage {
                stream_id: stream_id.to_string(),
                mid,
                msg_type,
                body,
            });
        }
    }
    Ok(out)
}

/// Add a `mid` when the message lacks the key (`pubsub.py:39-42`).
///
/// `setdefault` semantics: only a missing key is filled (a present but
/// falsy `mid` is kept, unlike `_serialize`'s `or`). `fresh_mid` is
/// the caller's fresh uuid4 string.
pub fn ensure_envelope(message: &Map<String, Value>, fresh_mid: &str) -> Map<String, Value> {
    let mut body = message.clone();
    body.entry("mid".to_string())
        .or_insert_with(|| Value::String(fresh_mid.to_string()));
    body
}

/// Compose the `assign` envelope sent to a runner daemon
/// (`matcher.py:306-330`).
///
/// Key order is `v, type, run_id, work_item_id, prompt, repo_url,
/// repo_ref, git_work_branch, expected_codex_model,
/// approval_policy_overrides, deadline`. `run_config` lookups default
/// to `null` exactly like `dict.get` (note `expected_codex_model`
/// reads the `model` key).
pub fn build_assign_msg(
    run_id: &str,
    work_item_id: Option<&str>,
    prompt: &str,
    run_config: &Map<String, Value>,
) -> Map<String, Value> {
    let get = |key: &str| run_config.get(key).cloned().unwrap_or(Value::Null);
    let mut msg = Map::with_capacity(11);
    msg.insert("v".to_string(), Value::from(1));
    msg.insert("type".to_string(), Value::String("assign".to_string()));
    msg.insert("run_id".to_string(), Value::String(run_id.to_string()));
    msg.insert(
        "work_item_id".to_string(),
        work_item_id.map_or(Value::Null, |id| Value::String(id.to_string())),
    );
    msg.insert("prompt".to_string(), Value::String(prompt.to_string()));
    msg.insert("repo_url".to_string(), get("repo_url"));
    msg.insert("repo_ref".to_string(), get("repo_ref"));
    msg.insert("git_work_branch".to_string(), get("git_work_branch"));
    msg.insert("expected_codex_model".to_string(), get("model"));
    msg.insert(
        "approval_policy_overrides".to_string(),
        get("approval_policy_overrides"),
    );
    msg.insert("deadline".to_string(), Value::Null);
    msg
}

/// `cancel` reason when the in-flight run does not exist
/// (`session_service.py:487`).
pub const REASON_UNKNOWN_RUN_ON_RECONNECT: &str = "unknown_run_on_reconnect";

/// `cancel` reason when the run is `cancel_requested`
/// (`session_service.py:493`, `:436`).
pub const REASON_CANCELLATION_PENDING_ON_RECONNECT: &str = "cancellation_pending_on_reconnect";

/// A `cancel` frame (`session_service.py:484-494`): `type, run_id,
/// reason` in order.
pub fn cancel_frame(run_id: &str, reason: &str) -> Map<String, Value> {
    let mut msg = Map::with_capacity(3);
    msg.insert("type".to_string(), Value::String("cancel".to_string()));
    msg.insert("run_id".to_string(), Value::String(run_id.to_string()));
    msg.insert("reason".to_string(), Value::String(reason.to_string()));
    msg
}

/// `cancel` reason for a terminated run: `run_already_<status>`
/// (`session_service.py:499`).
pub fn terminal_cancel_reason(status: &str) -> String {
    format!("run_already_{status}")
}

/// A `resume_ack` frame (`session_service.py:502-508`): `type, run_id,
/// last_seq, status, thread_id` in order.
pub fn resume_ack_frame(
    run_id: &str,
    last_seq: Option<i64>,
    status: &str,
    thread_id: &str,
) -> Map<String, Value> {
    let mut msg = Map::with_capacity(5);
    msg.insert("type".to_string(), Value::String("resume_ack".to_string()));
    msg.insert("run_id".to_string(), Value::String(run_id.to_string()));
    msg.insert(
        "last_seq".to_string(),
        last_seq.map_or(Value::Null, Value::from),
    );
    msg.insert("status".to_string(), Value::String(status.to_string()));
    msg.insert(
        "thread_id".to_string(),
        Value::String(thread_id.to_string()),
    );
    msg
}

/// The cancel-first redeliver frame (`session_service.py:432-437`):
/// `v, type, run_id, reason` in order.
pub fn redeliver_cancel_frame(run_id: &str) -> Map<String, Value> {
    let mut msg = Map::with_capacity(4);
    msg.insert("v".to_string(), Value::from(1));
    msg.insert("type".to_string(), Value::String("cancel".to_string()));
    msg.insert("run_id".to_string(), Value::String(run_id.to_string()));
    msg.insert(
        "reason".to_string(),
        Value::String(REASON_CANCELLATION_PENDING_ON_RECONNECT.to_string()),
    );
    msg
}

/// Default `revoke` reason (`pubsub.py:111`).
pub const REVOKE_DEFAULT_REASON: &str = "runner revoked";

/// A `revoke` frame (`pubsub.py:118-121`): `type, reason` in order.
pub fn revoke_frame(reason: &str) -> Map<String, Value> {
    let mut msg = Map::with_capacity(2);
    msg.insert("type".to_string(), Value::String("revoke".to_string()));
    msg.insert("reason".to_string(), Value::String(reason.to_string()));
    msg
}

/// Default `remove_runner` reason (`pubsub.py:131`).
pub const REMOVE_RUNNER_DEFAULT_REASON: &str = "deleted by user";

/// A `remove_runner` frame (`pubsub.py:155-161`): `type, runner_id,
/// reason` in order.
pub fn remove_runner_frame(runner_id: &str, reason: &str) -> Map<String, Value> {
    let mut msg = Map::with_capacity(3);
    msg.insert(
        "type".to_string(),
        Value::String("remove_runner".to_string()),
    );
    msg.insert(
        "runner_id".to_string(),
        Value::String(runner_id.to_string()),
    );
    msg.insert("reason".to_string(), Value::String(reason.to_string()));
    msg
}

/// The session-eviction pub/sub body (`outbox.py:553-555`,
/// `machine_outbox.py:384`): `json.dumps({"old_sid": …, "new_sid":
/// …})` with default separators.
pub fn eviction_body(old_sid: Option<&str>, new_sid: &str) -> String {
    let mut body = Map::with_capacity(2);
    body.insert(
        "old_sid".to_string(),
        old_sid.map_or(Value::Null, |sid| Value::String(sid.to_string())),
    );
    body.insert("new_sid".to_string(), Value::String(new_sid.to_string()));
    render_spaced_ascii(&Value::Object(body))
}

/// Reject code for the retired WS control plane (`consumers.py:29-42`,
/// skip stub per Dead Python Code §4 — the constant is kept so D-14
/// preserves the close-code-1008 behavior old runners rely on).
pub const CLOSE_CODE_PROTOCOL_UNSUPPORTED: u16 = 1008;

/// Ticket-invalid WS close code, same source.
pub const CLOSE_CODE_TICKET_INVALID: u16 = 4404;

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-03-envelopes.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn as_str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect()
    }

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn type_sets_replay_fixture() {
        let fx = fixture();
        for (field, consts) in [
            ("valid_types_runner", VALID_TYPES_RUNNER),
            ("offline_reject_runner", OFFLINE_REJECT_RUNNER),
            ("valid_types_machine", VALID_TYPES_MACHINE),
            ("offline_reject_machine", OFFLINE_REJECT_MACHINE),
        ] {
            let expected = as_str_list(fx.get(field).expect("field"));
            let actual: Vec<String> = consts.iter().map(|s| s.to_string()).collect();
            assert_eq!(actual, expected, "field {field}");
        }
        assert!(is_valid_runner_type("assign") && !is_valid_runner_type("ping"));
        assert!(
            is_offline_reject_runner_type("decide") && !is_offline_reject_runner_type("revoke")
        );
        assert!(is_valid_machine_type("ping") && !is_valid_machine_type("assign"));
        assert!(
            is_offline_reject_machine_type("create_runner")
                && !is_offline_reject_machine_type("ping")
        );
    }

    /// `serialize` vectors: the fixture records outputs; the input is the
    /// parsed payload itself (`mid` present in place makes `serialize`
    /// idempotent here).
    #[test]
    fn serialize_replays_fixture() {
        let fx = fixture();
        let vectors = fx.get("serialize").expect("serialize");
        for label in ["explicit_mid", "missing_type"] {
            let vector = vectors.get(label).expect("vector");
            let payload = vector
                .get("payload")
                .and_then(Value::as_str)
                .expect("payload");
            let message = object(serde_json::from_str(payload).expect("payload parses"));
            let out = serialize(&message, "unused-fresh");
            assert_eq!(
                Some(out.mid.as_str()),
                vector.get("mid").and_then(Value::as_str),
                "{label}"
            );
            assert_eq!(
                Some(out.msg_type.as_str()),
                vector.get("type").and_then(Value::as_str),
                "{label}"
            );
            assert_eq!(out.payload, payload, "{label}");
        }
        // `missing_type` pins the verbatim rule: no `type` key is added.
        assert_eq!(
            vectors["missing_type"]["payload"],
            Value::String("{\"mid\": \"m2\"}".to_string())
        );
    }

    #[test]
    fn serialize_default_mid_mechanism() {
        // The fixture records `<uuid4>` placeholders, so replay the
        // mechanism with a caller-supplied fresh id.
        let message = object(serde_json::json!({"type": "cancel"}));
        let fresh = "11111111-2222-4333-8444-555555555555";
        let out = serialize(&message, fresh);
        assert_eq!(out.mid, fresh);
        assert_eq!(out.msg_type, "cancel");
        assert_eq!(
            out.payload,
            "{\"type\": \"cancel\", \"mid\": \"11111111-2222-4333-8444-555555555555\"}"
        );
        for mid in [Value::Null, Value::String(String::new())] {
            let mut message = message.clone();
            message.insert("mid".to_string(), mid);
            assert_eq!(serialize(&message, fresh).mid, fresh);
        }
        let parsed = uuid::Uuid::parse_str(&out.mid).expect("uuid");
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
    }

    /// `revoke` / `remove_runner` payloads, replayed byte-exact: inputs
    /// are extracted from each recorded payload and rebuilt through the
    /// frame builders + `serialize`.
    #[test]
    fn revoke_and_remove_runner_payloads_replay_fixture() {
        let fx = fixture();
        let buffered = fx["pubsub_frames"]["offline_buffer_payloads"]
            .as_array()
            .expect("buffered");
        assert_eq!(buffered.len(), 4);
        for entry in buffered {
            let payload = entry
                .get("payload")
                .and_then(Value::as_str)
                .expect("payload");
            let body: Value = serde_json::from_str(payload).expect("payload parses");
            let mid = body.get("mid").and_then(Value::as_str).expect("mid");
            let frame = match body.get("type").and_then(Value::as_str) {
                Some("revoke") => revoke_frame(body["reason"].as_str().expect("reason")),
                Some("remove_runner") => remove_runner_frame(
                    body["runner_id"].as_str().expect("runner_id"),
                    body["reason"].as_str().expect("reason"),
                ),
                other => panic!("unexpected frame {other:?}"),
            };
            let out = serialize(&frame, mid);
            assert_eq!(out.mid, mid);
            assert_eq!(out.msg_type, entry["type"].as_str().expect("type"));
            assert_eq!(out.payload, payload);
        }
        assert_eq!(REVOKE_DEFAULT_REASON, "runner revoked");
        assert_eq!(REMOVE_RUNNER_DEFAULT_REASON, "deleted by user");
    }

    fn stream_reply(entries: Vec<(&str, Vec<(&str, &str)>)>) -> Vec<StreamRead> {
        vec![StreamRead {
            name: b"runner_stream:x".to_vec(),
            entries: entries
                .into_iter()
                .map(|(id, fields)| StreamEntry {
                    id: id.as_bytes().to_vec(),
                    fields: fields
                        .into_iter()
                        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                        .collect(),
                })
                .collect(),
        }]
    }

    /// `decode_read_result` vectors: inputs are reconstructed as the
    /// fields `_serialize` would have written (fixture bodies keep the
    /// original payload key order through `json.dump`); every golden
    /// output comes from the fixture.
    #[test]
    fn decode_replays_fixture() {
        let fx = fixture();
        let vectors = fx.get("decode_read_result").expect("vectors");
        assert_eq!(decode_read_result(None).expect("none"), vec![]);
        assert_eq!(vectors.get("none"), Some(&Value::Array(vec![])));
        assert_eq!(decode_read_result(Some(&[])).expect("empty"), vec![]);
        assert_eq!(vectors.get("empty"), Some(&Value::Array(vec![])));

        // `bytes_reply`: payload bytes rebuilt from each recorded body.
        let expected = vectors
            .get("bytes_reply")
            .and_then(Value::as_array)
            .expect("array");
        let rendered: Vec<String> = expected
            .iter()
            .map(|entry| render_spaced_ascii(&entry["body"]))
            .collect();
        let entries: Vec<(&str, Vec<(&str, &str)>)> = expected
            .iter()
            .zip(rendered.iter())
            .map(|(entry, payload)| {
                (
                    entry["stream_id"].as_str().expect("stream_id"),
                    vec![
                        ("mid", entry["mid"].as_str().expect("mid")),
                        ("type", entry["type"].as_str().expect("type")),
                        ("payload", payload.as_str()),
                    ],
                )
            })
            .collect();
        let reply = stream_reply(entries);
        let out = decode_read_result(Some(&reply)).expect("bytes reply");
        assert_eq!(out.len(), expected.len());
        for (decoded, entry) in out.iter().zip(expected) {
            assert_eq!(
                Some(decoded.stream_id.as_str()),
                entry["stream_id"].as_str()
            );
            assert_eq!(Some(decoded.mid.as_str()), entry["mid"].as_str());
            assert_eq!(Some(decoded.msg_type.as_str()), entry["type"].as_str());
            assert_eq!(decoded.body, entry["body"]);
            // Wire order of the re-emitted entry.
            let value = decoded.to_json_value();
            let keys: Vec<&str> = value
                .as_object()
                .expect("object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["stream_id", "mid", "type", "body"]);
        }

        // `corrupt_payload`: any unparseable payload → `{}` body.
        let expected = &vectors["corrupt_payload"][0];
        let reply = stream_reply(vec![(
            expected["stream_id"].as_str().expect("stream_id"),
            vec![
                ("mid", expected["mid"].as_str().expect("mid")),
                ("type", expected["type"].as_str().expect("type")),
                ("payload", "{not json"),
            ],
        )]);
        let out = decode_read_result(Some(&reply)).expect("corrupt");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, expected["body"]);
        assert_eq!(Some(out[0].mid.as_str()), expected["mid"].as_str());
        assert_eq!(Some(out[0].msg_type.as_str()), expected["type"].as_str());

        // `missing_fields_fallback_to_body`: payload-only fields.
        let expected = &vectors["missing_fields_fallback_to_body"][0];
        let payload = render_spaced_ascii(&expected["body"]);
        let reply = stream_reply(vec![(
            expected["stream_id"].as_str().expect("stream_id"),
            vec![("payload", payload.as_str())],
        )]);
        let out = decode_read_result(Some(&reply)).expect("fallback");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, expected["body"]);
        assert_eq!(Some(out[0].mid.as_str()), expected["mid"].as_str());
        assert_eq!(Some(out[0].msg_type.as_str()), expected["type"].as_str());

        // `empty_payload_string`: empty payload → `{}` → empty mid/type.
        let expected = &vectors["empty_payload_string"][0];
        let reply = stream_reply(vec![(
            expected["stream_id"].as_str().expect("stream_id"),
            vec![("payload", "")],
        )]);
        let out = decode_read_result(Some(&reply)).expect("empty payload");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, expected["body"]);
        assert_eq!(Some(out[0].mid.as_str()), expected["mid"].as_str());
        assert_eq!(Some(out[0].msg_type.as_str()), expected["type"].as_str());
    }

    #[test]
    fn decode_errors_mirror_python_raises() {
        // Invalid UTF-8 propagates (UnicodeDecodeError in Python).
        let reply = vec![StreamRead {
            name: b"s".to_vec(),
            entries: vec![StreamEntry {
                id: vec![0xff, 0xfe],
                fields: vec![],
            }],
        }];
        assert!(matches!(
            decode_read_result(Some(&reply)),
            Err(DecodeError::InvalidUtf8(_))
        ));
        // Non-object JSON payload with missing fields (AttributeError).
        let reply = stream_reply(vec![("4-0", vec![("payload", "5")])]);
        assert_eq!(
            decode_read_result(Some(&reply)),
            Err(DecodeError::BodyNotObject)
        );
        // …but present fields never touch the body fallback.
        let reply = stream_reply(vec![(
            "4-0",
            vec![("mid", "m"), ("type", "t"), ("payload", "5")],
        )]);
        let out = decode_read_result(Some(&reply)).expect("fields win");
        assert_eq!(out[0].body, serde_json::json!(5));
    }

    #[test]
    fn ensure_envelope_replays_fixture() {
        let fx = fixture();
        let vectors = fx.get("ensure_envelope").expect("vectors");
        // `adds_mid` records a `<uuid4>` placeholder: replay the
        // mechanism (fresh id used, appended after existing keys).
        let message = object(serde_json::json!({"type": "cancel"}));
        let fresh = "22222222-3333-4444-8555-666666666666";
        let out = ensure_envelope(&message, fresh);
        assert_eq!(out.get("mid").and_then(Value::as_str), Some(fresh));
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(keys, ["type", "mid"]);
        assert_eq!(
            vectors["adds_mid"]["type"],
            Value::String("cancel".to_string())
        );
        // `keeps_existing_mid` replays exactly.
        let kept = vectors.get("keeps_existing_mid").expect("kept");
        let message = object(kept.clone());
        let out = ensure_envelope(&message, fresh);
        assert_eq!(Value::Object(out), *kept);
    }

    /// `_build_assign_msg`: `run_config` is reconstructed from the
    /// recorded message (every config value round-trips through
    /// `dict.get`); message bytes + key order come from the fixture.
    #[test]
    fn build_assign_msg_replays_fixture() {
        let fx = fixture();
        let full = fx.get("build_assign_msg_full").expect("full");
        let msg = full.get("msg").expect("msg");
        let get = |key: &str| msg.get(key).expect("key");
        let mut run_config = Map::new();
        for (config_key, msg_key) in [
            ("repo_url", "repo_url"),
            ("repo_ref", "repo_ref"),
            ("git_work_branch", "git_work_branch"),
            ("model", "expected_codex_model"),
            ("approval_policy_overrides", "approval_policy_overrides"),
        ] {
            let value = get(msg_key);
            if !value.is_null() {
                run_config.insert(config_key.to_string(), value.clone());
            }
        }
        let rebuilt = build_assign_msg(
            get("run_id").as_str().expect("run_id"),
            get("work_item_id").as_str(),
            get("prompt").as_str().expect("prompt"),
            &run_config,
        );
        assert_eq!(Value::Object(rebuilt.clone()), *msg);
        let keys: Vec<String> = rebuilt.keys().cloned().collect();
        let expected_keys = as_str_list(full.get("key_order").expect("key_order"));
        assert_eq!(keys, expected_keys);

        let bare = fx.get("build_assign_msg_bare").expect("bare");
        let msg = bare.get("msg").expect("msg");
        let rebuilt = build_assign_msg(
            msg["run_id"].as_str().expect("run_id"),
            msg["work_item_id"].as_str(),
            msg["prompt"].as_str().expect("prompt"),
            &Map::new(),
        );
        assert_eq!(Value::Object(rebuilt), *msg);
    }

    #[test]
    fn resume_ack_and_cancel_variants_replay_fixture() {
        let fx = fixture();
        let shapes = fx.get("resume_ack_shapes").expect("shapes");
        let unknown = shapes.get("unknown_run").expect("unknown");
        assert_eq!(
            unknown["reason"],
            Value::String(REASON_UNKNOWN_RUN_ON_RECONNECT.to_string())
        );
        assert_eq!(
            Value::Object(cancel_frame(
                unknown["run_id"].as_str().expect("run_id"),
                unknown["reason"].as_str().expect("reason")
            )),
            *unknown
        );
        let requested = shapes.get("cancel_requested").expect("requested");
        assert_eq!(
            requested["reason"],
            Value::String(REASON_CANCELLATION_PENDING_ON_RECONNECT.to_string())
        );
        assert_eq!(
            Value::Object(cancel_frame(
                requested["run_id"].as_str().expect("run_id"),
                requested["reason"].as_str().expect("reason")
            )),
            *requested
        );
        let terminal = shapes.get("terminal").expect("terminal");
        let reason = terminal["reason"].as_str().expect("reason");
        let status = reason.strip_prefix("run_already_").expect("prefix");
        assert_eq!(terminal_cancel_reason(status), reason);
        assert_eq!(terminal_cancel_reason("completed"), "run_already_completed");
        assert_eq!(
            Value::Object(cancel_frame(
                terminal["run_id"].as_str().expect("run_id"),
                &terminal_cancel_reason(status)
            )),
            *terminal
        );
        for label in ["live", "live_no_events"] {
            let shape = shapes.get(label).expect("shape");
            let rebuilt = resume_ack_frame(
                shape["run_id"].as_str().expect("run_id"),
                shape["last_seq"].as_i64(),
                shape["status"].as_str().expect("status"),
                shape["thread_id"].as_str().expect("thread_id"),
            );
            assert_eq!(Value::Object(rebuilt), *shape, "{label}");
        }
        let cancel_first = fx["redeliver_shapes"]["cancel_first"].clone();
        assert_eq!(
            Value::Object(redeliver_cancel_frame(
                cancel_first["run_id"].as_str().expect("run_id")
            )),
            cancel_first
        );
    }

    /// Eviction bodies, replayed byte-exact from the recorded `publish`
    /// args (and the `close_runner_session` publish with a real sid).
    #[test]
    fn eviction_bodies_replay_fixture() {
        let fx = fixture();
        let publishes = fx["eviction_publish"]["redis"]
            .as_array()
            .expect("publishes");
        assert_eq!(publishes.len(), 2);
        for publish in publishes {
            replay_publish_body(publish);
        }
        let close = fx["close_runner_session"]["redis"]
            .as_array()
            .expect("close redis");
        replay_publish_body(&close[1]);
    }

    fn replay_publish_body(publish: &Value) {
        assert_eq!(publish["cmd"], Value::String("publish".to_string()));
        let args = publish["args"].as_array().expect("args");
        let raw = args[1].as_str().expect("body arg");
        // Args are Python-repr-quoted: strip the outer single quotes.
        let body = raw
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .expect("quoted");
        let parsed: Value = serde_json::from_str(body).expect("body parses");
        let old = parsed["old_sid"].as_str();
        let new = parsed["new_sid"].as_str().expect("new_sid");
        assert_eq!(eviction_body(old, new), body);
    }

    #[test]
    fn ws_close_codes_replay_fixture() {
        let fx = fixture();
        let codes = fx.get("ws_close_1008").expect("codes");
        assert_eq!(
            codes
                .get("CLOSE_CODE_PROTOCOL_UNSUPPORTED")
                .and_then(Value::as_u64),
            Some(u64::from(CLOSE_CODE_PROTOCOL_UNSUPPORTED))
        );
        assert_eq!(
            codes
                .get("CLOSE_CODE_TICKET_INVALID")
                .and_then(Value::as_u64),
            Some(u64::from(CLOSE_CODE_TICKET_INVALID))
        );
    }
}
