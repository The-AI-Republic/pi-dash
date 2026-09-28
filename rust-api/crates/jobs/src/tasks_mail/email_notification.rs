//! `email_notification_task.py` helpers, part 1 (D-07, PIDASHCONV-212).
//!
//! Port of `apps/api/pi_dash/bgtasks/email_notification_task.py`:
//! `remove_unwanted_characters` (`:27-31`), `acquire_lock` (`:34-38`),
//! `release_lock` (`:40-43`) and `create_payload` (`:87-127`).
//! PIDASHCONV-213 builds the `stack_email_notification` /
//! `send_email_notification` task bodies on these helpers.

use chrono::{DateTime, NaiveDateTime};
use serde_json::{Map, Value};

/// Value stored under every mail lock (`acquire_lock` passes the literal
/// `"true"`; `email_notification_task.py:38`).
pub const LOCK_VALUE: &str = "true";

/// Default lock TTL in seconds (`acquire_lock(lock_id, expire_time=300)`;
/// `email_notification_task.py:34`).
pub const DEFAULT_LOCK_EXPIRE_SECS: u64 = 300;

/// The Redis surface the lock helpers need. There is no shared Redis
/// client in the Rust crates yet, so this domain-owned trait records the
/// exact call shapes the Python code makes: SET with NX plus EX, and DEL.
/// The task layer (PIDASHCONV-213) supplies the client. A needed change to
/// a shared seam would be a new issue; this trait keeps that unnecessary.
pub trait RedisLock {
    /// `redis_client.set(key, "true", nx=True, ex=expire_secs)`
    /// (`email_notification_task.py:38`).
    fn set_nx_ex(&self, key: &str, value: &str, ex_secs: u64) -> LockSet;
    /// `redis_client.delete(lock_id)` (`email_notification_task.py:43`).
    fn del(&self, key: &str);
}

/// What redis-py `set(..., nx=True)` returns: `True` when the key was set,
/// `None` when the key already existed (lock contended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockSet {
    Acquired,
    Contended,
}

/// Attempt to acquire a lock with a specified expiration time
/// (`email_notification_task.py:34-38`).
///
/// Returns the exact truthiness Python callers test (`if
/// acquire_lock(...)`): `true` for redis-py's `True`, `false` for its
/// `None`.
pub fn acquire_lock(client: &impl RedisLock, lock_id: &str, expire_secs: u64) -> bool {
    matches!(
        client.set_nx_ex(lock_id, LOCK_VALUE, expire_secs),
        LockSet::Acquired
    )
}

/// Release a lock (`email_notification_task.py:40-43`). Python ignores the
/// `delete` return, so this returns nothing.
pub fn release_lock(client: &impl RedisLock, lock_id: &str) {
    client.del(lock_id);
}

/// Remove only control characters and potentially problematic characters
/// for email subjects (`email_notification_task.py:27-31`):
/// `re.sub(r"[\x00-\x1F\x7F-\x9F]", "", input_text)`.
///
/// `char::is_control` is exactly Unicode general category Cc
/// (U+0000–U+001F, U+007F–U+009F), i.e. the same code-point set the Python
/// character class matches — including TAB/LF (`\x09`–`\x0D`, see the
/// `tab\there` fixture case) — so no `regex` dependency is needed.
pub fn remove_unwanted_characters(input_text: &str) -> String {
    input_text.chars().filter(|c| !c.is_control()).collect()
}

/// Python `str()` applied to a JSON value, as `create_payload` stringifies
/// `old_value`/`new_value` (`str(issue_activity.get("old_value"))`,
/// `email_notification_task.py:94-95`): `None` becomes `"None"` (truthy, so
/// it is kept), booleans render `"True"`/`"False"`, numbers render as
/// written, strings pass through, and containers render with Python
/// `repr` (single-quoted strings, insertion order).
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&Value::String(k.clone())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `repr()` of a JSON value (used for container elements above).
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => py_str(value),
    }
}

/// Every way `create_payload` can fail. Each variant is a Python raise on
/// the same path: the send task's outer `except` releases the lock and
/// returns (`email_notification_task.py:303-306`), which PIDASHCONV-213
/// mirrors when it wires this helper into the task body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    /// A change row has no `.get` (not an object) or an `issue_activity`
    /// entry is truthy but not an object: Python raises `AttributeError`
    /// (`email_notification_task.py:91-93`).
    #[error("change row or issue_activity is not an object (actor {0})")]
    BadChange(String),
    /// `activity_time` is missing, not a string, or not ISO-8601: Python
    /// raises `AttributeError` (`.rstrip` on `None`) or `ValueError`
    /// (`fromisoformat`) (`email_notification_task.py:121-125`).
    #[error("bad activity_time for actor {0}: {1}")]
    BadActivityTime(String, String),
    /// The actor has no payload entry when `activity_time` is written
    /// (every change so far had empty old+new): Python raises `KeyError`
    /// on `data[actor_id]["activity_time"]`
    /// (`email_notification_task.py:121`).
    #[error("actor {0} has no payload entry for activity_time")]
    MissingActor(String),
}

/// Field-dict key coercion. Python uses the raw `field` value as a dict
/// key; at the Celery wire boundary JSON coerces keys (`None` → `"null"`,
/// `True` → `"true"`), so non-string fields render with their JSON key
/// spelling here. Strings pass through untouched (the only fixture shape).
fn field_key(field: Option<&Value>) -> String {
    match field {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => Value::Null.to_string(),
    }
}

/// Parse `activity_time` the way
/// `datetime.fromisoformat(issue_activity.get("activity_time").rstrip("Z"))`
/// does (`email_notification_task.py:122`), then render `"%Y-%m-%d
/// %H:%M:%S"`. `rstrip("Z")` strips every trailing `Z`; `fromisoformat`
/// keeps any `+HH:MM` offset and `strftime` drops it without converting,
/// so offsets parse but never shift the wall time — mirrored here.
fn parse_activity_time(raw: &str) -> Option<String> {
    let stripped = raw.trim_end_matches('Z');
    if let Ok(offset) = DateTime::parse_from_rfc3339(stripped) {
        return Some(offset.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(stripped, format) {
            return Some(naive.format("%Y-%m-%d %H:%M:%S").to_string());
        }
    }
    None
}

/// Append `value` to the `slot` (`"old_value"`/`"new_value"`) list of
/// `data[actor_id][field]`, skipping empties and de-duplicating.
///
/// Mirrors the conditional-append idiom
/// (`email_notification_task.py:99-114`): the `if value not in
/// [...].get(slot, [])` guard is evaluated first (creating the nested
/// dicts as a side effect), and the append runs only for unseen values —
/// so the second identical `Todo → Done` change in the fixture adds
/// nothing.
fn push_unique(
    data: &mut Map<String, Value>,
    actor_id: &str,
    field: &str,
    slot: &str,
    value: String,
) {
    if value.is_empty() {
        return;
    }
    let actor = data
        .entry(actor_id.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let fields = actor
        .as_object_mut()
        .expect("actor entry is always an object")
        .entry(field.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let list = fields
        .as_object_mut()
        .expect("field entry is always an object")
        .entry(slot.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    let list = list.as_array_mut().expect("slot entry is always an array");
    if !list
        .iter()
        .any(|existing| existing == &Value::String(value.clone()))
    {
        list.push(Value::String(value));
    }
}

/// Build the send payload from grouped notification data
/// (`email_notification_task.py:87-127`):
/// `{actor_id: [{issue_activity: {field, old_value, new_value,
/// activity_time}}]}` → `{actor_id: {field: {old_value: [], new_value:
/// []}, activity_time}}`.
///
/// Rows whose `issue_activity` is missing or `None` are skipped; `old`/`new`
/// values are `str()`-stringified first (`None` → `"None"`), falsy `""`
/// values are skipped, duplicates are de-duplicated.
///
/// BUG (ported verbatim, `email_notification_task.py:116`): the guard
/// `if not data.get("actor_id", {}).get("activity_time", False)` tests the
/// literal key `"actor_id"` — never the loop variable — so it is always
/// true and `activity_time` is overwritten on every change: last change
/// wins. The port is therefore an unconditional overwrite per change.
pub fn create_payload(
    notification_data: &Map<String, Value>,
) -> Result<Map<String, Value>, PayloadError> {
    let mut data: Map<String, Value> = Map::new();
    for (actor_id, changes) in notification_data {
        let changes = match changes.as_array() {
            Some(changes) => changes,
            None => return Err(PayloadError::BadChange(actor_id.clone())),
        };
        for change in changes {
            let change = match change.as_object() {
                Some(change) => change,
                None => return Err(PayloadError::BadChange(actor_id.clone())),
            };
            let issue_activity = match change.get("issue_activity") {
                None | Some(Value::Null) => continue,
                Some(Value::Object(activity)) => activity,
                Some(_) => return Err(PayloadError::BadChange(actor_id.clone())),
            };
            let field = field_key(issue_activity.get("field"));
            let old_value = py_str(issue_activity.get("old_value").unwrap_or(&Value::Null));
            push_unique(&mut data, actor_id, &field, "old_value", old_value);
            let new_value = py_str(issue_activity.get("new_value").unwrap_or(&Value::Null));
            push_unique(&mut data, actor_id, &field, "new_value", new_value);

            // BUG (:116): literal-"actor_id" guard, always true → last wins.
            let raw_time = match issue_activity.get("activity_time") {
                Some(Value::String(raw)) => raw.clone(),
                Some(other) => {
                    return Err(PayloadError::BadActivityTime(
                        actor_id.clone(),
                        py_str(other),
                    ));
                }
                None => {
                    return Err(PayloadError::BadActivityTime(
                        actor_id.clone(),
                        "missing".to_owned(),
                    ));
                }
            };
            let formatted = parse_activity_time(&raw_time)
                .ok_or_else(|| PayloadError::BadActivityTime(actor_id.clone(), raw_time.clone()))?;
            match data.get_mut(actor_id) {
                Some(Value::Object(actor)) => {
                    actor.insert("activity_time".to_owned(), Value::String(formatted));
                }
                _ => return Err(PayloadError::MissingActor(actor_id.clone())),
            }
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Recorded-call fake for the [`RedisLock`] seam.
    struct FakeRedis {
        sets: RefCell<Vec<(String, String, u64)>>,
        dels: RefCell<Vec<String>>,
        acquire_result: LockSet,
    }

    impl FakeRedis {
        fn acquiring() -> Self {
            Self {
                sets: RefCell::new(Vec::new()),
                dels: RefCell::new(Vec::new()),
                acquire_result: LockSet::Acquired,
            }
        }

        fn contended() -> Self {
            Self {
                sets: RefCell::new(Vec::new()),
                dels: RefCell::new(Vec::new()),
                acquire_result: LockSet::Contended,
            }
        }
    }

    impl RedisLock for FakeRedis {
        fn set_nx_ex(&self, key: &str, value: &str, ex_secs: u64) -> LockSet {
            self.sets
                .borrow_mut()
                .push((key.to_owned(), value.to_owned(), ex_secs));
            self.acquire_result
        }

        fn del(&self, key: &str) {
            self.dels.borrow_mut().push(key.to_owned());
        }
    }

    #[test]
    fn remove_unwanted_characters_matches_fixture() {
        // `rust-api/fixtures/tasks_mail/send/F-SEND.remove_unwanted_characters.golden.json`
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_mail/send/F-SEND.remove_unwanted_characters.golden.json"
        ))
        .expect("fixture must parse");
        assert_eq!(fixture["_fixture"], "F-SEND");
        let cases = fixture["cases"].as_array().expect("cases array");
        assert!(!cases.is_empty());
        for case in cases {
            let input = case["in"].as_str().expect("string input");
            let expected = case["out"].as_str().expect("string output");
            assert_eq!(
                remove_unwanted_characters(input),
                expected,
                "input {input:?}"
            );
        }
    }

    #[test]
    fn create_payload_matches_fixture() {
        // `rust-api/fixtures/tasks_mail/send/F-SEND.create_payload.golden.json`
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_mail/send/F-SEND.create_payload.golden.json"
        ))
        .expect("fixture must parse");
        assert_eq!(fixture["_fixture"], "F-SEND");
        let cases = fixture["cases"].as_array().expect("cases array");
        assert_eq!(cases.len(), 2);
        for (index, case) in cases.iter().enumerate() {
            let input = case["in"].as_object().expect("object input").clone();
            let expected = case["out"].as_object().expect("object output").clone();
            let actual = create_payload(&input).expect("fixture input must succeed");
            assert_eq!(actual, expected, "case {index}");
        }
    }

    #[test]
    fn create_payload_last_change_wins_activity_time() {
        // PORT BUG (:116, `PORT_BUG_2` in the fixture): the literal-key
        // guard is always true, so the second change's timestamp overwrites
        // the first — `11:00` then `12:00` must yield `12:00`.
        let input: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": {"field": "state", "old_value": "a", "new_value": "b",
                                    "activity_time": "2026-09-01T11:00:00Z"}},
                {"issue_activity": {"field": "state", "old_value": "c", "new_value": "d",
                                    "activity_time": "2026-09-01T12:00:00Z"}}
            ]
        }))
        .expect("test input");
        let actual = create_payload(&input).expect("must succeed");
        assert_eq!(actual["actor-1"]["activity_time"], "2026-09-01 12:00:00");
        assert_eq!(
            actual["actor-1"]["state"]["old_value"],
            serde_json::json!(["a", "c"])
        );
    }

    #[test]
    fn create_payload_skips_null_activity_and_empty_values() {
        let input: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": null},
                {"no_activity_key": true},
                {"issue_activity": {"field": "priority", "old_value": "",
                                    "new_value": "high", "activity_time": "2026-09-01T11:00:00Z"}}
            ]
        }))
        .expect("test input");
        let actual = create_payload(&input).expect("must succeed");
        assert_eq!(
            actual["actor-1"],
            serde_json::json!({
                "priority": {"new_value": ["high"]},
                "activity_time": "2026-09-01 11:00:00"
            })
        );
    }

    #[test]
    fn create_payload_str_none_and_offset_time() {
        // `str(None)` → `"None"` (truthy, kept); `+HH:MM` offsets parse
        // without shifting the wall time.
        let input: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": {"field": "comment", "old_value": null,
                                    "new_value": "hi", "activity_time": "2026-09-01T12:30:00+00:00"}}
            ]
        }))
        .expect("test input");
        let actual = create_payload(&input).expect("must succeed");
        assert_eq!(
            actual["actor-1"]["comment"]["old_value"],
            serde_json::json!(["None"])
        );
        assert_eq!(actual["actor-1"]["activity_time"], "2026-09-01 12:30:00");
    }

    #[test]
    fn create_payload_errors_mirror_python_raises() {
        // Both values empty → `data[actor_id]` never created → KeyError.
        let both_empty: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": {"field": "state", "old_value": "",
                                    "new_value": "", "activity_time": "2026-09-01T10:00:00Z"}}
            ]
        }))
        .expect("test input");
        assert_eq!(
            create_payload(&both_empty),
            Err(PayloadError::MissingActor("actor-1".to_owned()))
        );
        // Missing activity_time → AttributeError on `.rstrip`.
        let no_time: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": {"field": "state", "old_value": "a", "new_value": "b"}}
            ]
        }))
        .expect("test input");
        assert!(matches!(
            create_payload(&no_time),
            Err(PayloadError::BadActivityTime(_, _))
        ));
        // Garbage timestamp → ValueError from `fromisoformat`.
        let bad_time: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "actor-1": [
                {"issue_activity": {"field": "state", "old_value": "a", "new_value": "b",
                                    "activity_time": "not-a-time"}}
            ]
        }))
        .expect("test input");
        assert!(matches!(
            create_payload(&bad_time),
            Err(PayloadError::BadActivityTime(_, _))
        ));
    }

    #[test]
    fn lock_shapes_match_fixture() {
        // `F-SEND.send_email_notification.json` lock block:
        // `redis.set(lock_id, "true", nx=True, ex=expire_time=300)` /
        // `redis.delete(lock_id)`.
        let redis = FakeRedis::acquiring();
        assert!(acquire_lock(
            &redis,
            "send_email_notif_i_r_1",
            DEFAULT_LOCK_EXPIRE_SECS
        ));
        assert_eq!(
            redis.sets.borrow().as_slice(),
            [(
                "send_email_notif_i_r_1".to_owned(),
                LOCK_VALUE.to_owned(),
                300
            )]
        );
        release_lock(&redis, "send_email_notif_i_r_1");
        assert_eq!(
            redis.dels.borrow().as_slice(),
            ["send_email_notif_i_r_1".to_owned()]
        );

        // Contended lock (redis-py returns None): falsy, and the send task
        // logs "Duplicate email received skipping" without releasing.
        let redis = FakeRedis::contended();
        assert!(!acquire_lock(&redis, "send_email_notif_i_r_1", 300));
        assert!(redis.dels.borrow().is_empty());
    }
}
