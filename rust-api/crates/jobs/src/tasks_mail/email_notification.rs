//! `email_notification_task.py` helpers, part 1 (D-07, PIDASHCONV-212).
//!
//! Port of `apps/api/pi_dash/bgtasks/email_notification_task.py`:
//! `remove_unwanted_characters` (`:27-31`), `acquire_lock` (`:34-38`),
//! `release_lock` (`:40-43`) and `create_payload` (`:87-127`).
//! PIDASHCONV-213 builds the `stack_email_notification` /
//! `send_email_notification` task bodies on these helpers.

use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime};
use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use crate::worker::{Handler, Registry, Verdict};

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

// ============================================================
// Part 2 (PIDASHCONV-213): mention helpers, stack + send tasks.
// ============================================================
//
// Port of the rest of `apps/api/pi_dash/bgtasks/email_notification_task.py`:
// `process_mention` (`:130-140`), `process_html_content` (`:142-150`),
// `stack_email_notification` (`:46-84`) and `send_email_notification`
// (`:152-306`). Built on the part-1 helpers above (`remove_unwanted_characters`,
// `acquire_lock`/`release_lock`, `create_payload`, `RedisLock`).
//
// Dependency note: the workspace has no HTML parser, template engine,
// Redis client or SMTP client (offline environment; none of `scraper`,
// `minijinja`, `lettre`, `redis` are in `rust-api/Cargo.lock`), so I/O
// that needs one lives behind domain-local traits that record the exact
// call shapes Python makes — the same status as part 1's `RedisLock`
// ("the task layer supplies the client"). The pure decision logic, the
// builders and the wire constructors below are complete and unit-pinned
// against `F-STACK`/`F-SEND`/`F-WIRE-MAIL`/`F-COMMON`. Flipping live
// traffic to these handlers is the domain gate's call (PIDASHCONV-218,
// after the PIDASHCONV-21 proxy pass); until then every D-07 name routes
// to `PythonOwned` (see `super::assert_python_owned`).

/// Log line when a send lock is already held
/// (`email_notification_task.py:300`).
pub const DUPLICATE_LOG: &str = "Duplicate email received skipping";
/// Log line on a successful send (`:285`).
pub const SENT_LOG: &str = "Email Sent Successfully";
/// `summary` context value (`:241`).
pub const SUMMARY: &str = "Updates were made to the issue by";
/// `entity_type` context value (`:260`).
pub const ENTITY_TYPE_ISSUE: &str = "issue";
/// Template rendered for the send (`:263`).
pub const ISSUE_UPDATES_TEMPLATE: &str = "emails/notifications/issue-updates.html";

/// What a per-mention user lookup can report. The only production source
/// is `User.objects.get(pk=user_id)` (`:136`); `DoesNotExist` propagates
/// out of `process_mention` (no catch there) into the send task's outer
/// `except (Issue.DoesNotExist, User.DoesNotExist)` (`:303-306`), which
/// releases the lock and returns silently — mirrored in [`run_send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionLookupError {
    /// `User.DoesNotExist` for this id.
    NotFound(String),
}

/// Every way `process_mention` can fail. Both variants propagate to the
/// send task's outer `except`: `UserNotFound` is a `DoesNotExist` (silent
/// release + return), `MissingEntityIdentifier` is a `KeyError`
/// (`mention["entity_identifier"]`, `:135`) and takes the generic
/// `log_exception` + release + return path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MentionError {
    /// `<mention-component>` without an `entity_identifier` attribute
    /// (Python `KeyError`, `:135`).
    #[error("mention-component without entity_identifier")]
    MissingEntityIdentifier,
    /// `User.objects.get(pk)` raised `DoesNotExist` (`:136`).
    #[error("user {0} does not exist")]
    UserNotFound(String),
    /// A `new_value`/`old_value` slot is neither null, a string nor a
    /// string array. Python would iterate a dict's keys (or fail on a
    /// scalar); at the Celery JSON boundary only null/array-of-string
    /// occur, so anything else is rejected here instead of guessed.
    #[error("bad mention content shape")]
    BadShape,
}

/// Escape `&`, `<`, `>` exactly as BeautifulSoup serializes an inserted
/// text node (`mention.replace_with(highlighted_name)` + `str(soup)`,
/// `:138-140`): the `@name` text re-serializes with those three escaped.
fn escape_bs_text(name: &str) -> String {
    name.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One parsed `<mention-component …>` open tag: its byte length and the
/// `entity_identifier` attribute (`None` when absent — the Python
/// `KeyError` case).
struct OpenTag {
    open_len: usize,
    entity_identifier: Option<String>,
}

/// Match `<mention-component` (case-insensitive — `html.parser`
/// lowercases tag names before `find_all`, `:133`) followed by a tag
/// delimiter. Returns `None` for anything else, including unterminated
/// `<…` (copied literally, as the parser would text-ify it).
fn match_open_tag(s: &str) -> Option<OpenTag> {
    if !s.starts_with('<') {
        return None;
    }
    let rest = &s[1..];
    let name_len = "mention-component".len();
    if rest.len() < name_len || !rest[..name_len].eq_ignore_ascii_case("mention-component") {
        return None;
    }
    match rest[name_len..].chars().next() {
        Some(c) if c.is_whitespace() || c == '/' || c == '>' => {}
        _ => return None,
    }
    // Scan to the closing `>` respecting quoted attribute values.
    let bytes = s.as_bytes();
    let mut j = 1 + name_len;
    let mut quote: Option<u8> = None;
    while j < bytes.len() {
        let c = bytes[j];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'>' {
            break;
        }
        j += 1;
    }
    if j >= bytes.len() {
        return None;
    }
    let tag_text = &s[..=j];
    Some(OpenTag {
        open_len: j + 1,
        entity_identifier: find_attr(tag_text, "entity_identifier"),
    })
}

/// Match a `</mention-component …>` close tag (case-insensitive,
/// optional whitespace): its byte length, else `None`.
fn match_close_tag(s: &str) -> Option<usize> {
    if !s.starts_with("</") {
        return None;
    }
    let rest = &s[2..];
    let name_len = "mention-component".len();
    if rest.len() < name_len || !rest[..name_len].eq_ignore_ascii_case("mention-component") {
        return None;
    }
    let mut j = 2 + name_len;
    let bytes = s.as_bytes();
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b'>' {
        Some(j + 1)
    } else {
        None
    }
}

/// Read one attribute value out of an open-tag slice. Attribute names
/// compare lowercased (`html.parser` lowercases them, `:135`); values
/// accept `"…"`, `'…'` or bare spellings, taken raw with no entity
/// decoding (ids are UUIDs). A valueless attribute yields `Some("")`
/// (Python sees `""`, and the `User.objects.get(pk="")` then raises
/// `DoesNotExist` — preserved).
fn find_attr(tag: &str, want: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut k = 1 + "mention-component".len();
    while k < bytes.len() {
        while k < bytes.len() && (bytes[k].is_ascii_whitespace() || bytes[k] == b'/') {
            k += 1;
        }
        if k >= bytes.len() || bytes[k] == b'>' {
            break;
        }
        let start = k;
        while k < bytes.len()
            && !bytes[k].is_ascii_whitespace()
            && bytes[k] != b'='
            && bytes[k] != b'>'
            && bytes[k] != b'/'
        {
            k += 1;
        }
        let name = tag[start..k].to_ascii_lowercase();
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        let mut value = String::new();
        if k < bytes.len() && bytes[k] == b'=' {
            k += 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && (bytes[k] == b'"' || bytes[k] == b'\'') {
                let q = bytes[k];
                k += 1;
                let vs = k;
                while k < bytes.len() && bytes[k] != q {
                    k += 1;
                }
                value = tag[vs..k.min(tag.len())].to_owned();
                k += 1;
            } else {
                let vs = k;
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                    k += 1;
                }
                value = tag[vs..k].to_owned();
            }
        }
        if name == want {
            return Some(value);
        }
    }
    None
}

/// End offset (relative to `s`, which starts at the open tag) of the
/// span a replacement covers: through the matching close tag
/// (nesting-aware), or `Some(open_len)` for self-closing `<…/>`, or
/// `None` when never closed — in which case only the open tag is
/// replaced (documented divergence: BeautifulSoup would swallow the
/// trailing siblings into the element; the editor always emits balanced
/// tags, so no producer hits this).
fn match_close(s: &str, open_len: usize) -> Option<usize> {
    let open = &s[..open_len];
    let inner = open[..open.len() - 1].trim_end();
    if inner.ends_with('/') {
        return Some(open_len);
    }
    let mut depth = 1;
    let mut k = open_len;
    while k < s.len() {
        if s[k..].starts_with("<!--") {
            k = s[k..].find("-->").map(|j| k + j + 3).unwrap_or(s.len());
            continue;
        }
        if let Some(tag) = match_open_tag(&s[k..]) {
            depth += 1;
            k += tag.open_len;
            continue;
        }
        if let Some(len) = match_close_tag(&s[k..]) {
            depth -= 1;
            k += len;
            if depth == 0 {
                return Some(k);
            }
            continue;
        }
        k += s[k..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

/// Rewrite every `<mention-component>` to `@display_name`
/// (`email_notification_task.py:130-140`).
///
/// `lookup` is one `User.objects.get(pk=user_id)` + `.display_name`
/// (`:136-137`) per mention occurrence — including repeats (no caching:
/// Python runs the SELECT per element, N queries preserved in shape).
/// Tags inside `<!-- … -->` comments are never elements (BeautifulSoup
/// parity) and pass through untouched.
pub fn process_mention(
    html: &str,
    lookup: &dyn Fn(&str) -> Result<String, MentionLookupError>,
) -> Result<String, MentionError> {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        // Tags inside HTML comments are not elements.
        if html[i..].starts_with("<!--") {
            let end = html[i..]
                .find("-->")
                .map(|j| i + j + 3)
                .unwrap_or(html.len());
            out.push_str(&html[i..end]);
            i = end;
            continue;
        }
        if let Some(tag) = match_open_tag(&html[i..]) {
            // Python collects `find_all` first: a missing attribute raises
            // `KeyError` before any SELECT or replacement (`:133-135`).
            let entity_id = tag
                .entity_identifier
                .ok_or(MentionError::MissingEntityIdentifier)?;
            let name =
                lookup(&entity_id).map_err(|_| MentionError::UserNotFound(entity_id.clone()))?;
            let end = match_close(&html[i..], tag.open_len).map(|e| i + e);
            out.push('@');
            out.push_str(&escape_bs_text(&name));
            i = end.unwrap_or(i + tag.open_len);
            continue;
        }
        let ch = html[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8().max(1);
    }
    Ok(out)
}

/// Map `process_mention` over a content list (`:142-150`): `None` maps
/// to `None` (`:144-145`); otherwise each entry is rewritten (`:147-150`).
/// `Value` carries the Celery JSON boundary: `Null`/missing is the
/// Python `None`, an array of strings is the list, anything else is a
/// [`MentionError::BadShape`].
pub fn process_html_content(
    content: Option<&Value>,
    lookup: &dyn Fn(&str) -> Result<String, MentionLookupError>,
) -> Result<Option<Vec<String>>, MentionError> {
    match content {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(html) => out.push(process_mention(html, lookup)?),
                    _ => return Err(MentionError::BadShape),
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(MentionError::BadShape),
    }
}

// ---- stack_email_notification (`:46-84`) ----

/// One `EmailNotificationLog.objects…​.values()` row (`:49`: full-row
/// dicts, `processed_at IS NULL`, `ORDER BY receiver`). Ids are already
/// the `str(…)` forms Python computes at the Celery JSON boundary
/// (`:55`, `:80`); `entity_identifier` is the UUID object → `None` when
/// the column is NULL (Python groups a `None` key too — preserved).
/// `data` is the `data` column verbatim (`None`/null appended as-is,
/// `:66`).
#[derive(Debug, Clone, PartialEq)]
pub struct StackRow {
    pub id: String,
    pub receiver_id: String,
    pub triggered_by_id: String,
    pub entity_identifier: Option<String>,
    pub data: Value,
}

/// One per-issue fan-out: `send_email_notification.delay(issue_id,
/// notification_data, receiver_id, email_notification_ids)` (`:75-81`)
/// with `notification_data = {actor: [data]}` for that issue only.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IssueBatch {
    pub issue_id: Option<String>,
    pub notification_data: Map<String, Value>,
}

/// One receiver's fan-out. BUG (ported verbatim, `:72` vs `:80` —
/// `PORT_BUG_1` in `F-STACK`): `email_notification_ids` is appended for
/// EVERY row of the receiver but fanned out per issue, so each send
/// task receives the receiver's ids across ALL its issues, not the
/// single issue's ids.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReceiverBatch {
    pub receiver_id: String,
    pub batches: Vec<IssueBatch>,
    pub email_notification_ids: Vec<String>,
}

/// Group unprocessed rows exactly like `:55-72`:
/// `receivers = list(set(str(receiver_id)))`, then per receiver
/// `payload.setdefault(entity_identifier, {}).setdefault(str(triggered_by_id),
/// []).append(data)`.
///
/// Ordering note: Python iterates a `set`, so receiver order is
/// nondeterministic; the port uses first-seen row order (deterministic
/// given the `ORDER BY receiver` input). The fanned-out SET is
/// identical — only the enqueue sequence is pinned down. Per-issue
/// order inside a receiver is first-seen row order on both sides (dict
/// insertion order, preserved here by `preserve_order`).
///
/// Returns `(batches, processed_ids)`: `processed_ids` is every row's
/// id in row order (`processed_notifications`, `:68-71`) and feeds the
/// unconditional `processed_at` update (`:84` runs even when empty).
pub fn plan_stack(rows: &[StackRow]) -> (Vec<ReceiverBatch>, Vec<String>) {
    let mut receiver_order: Vec<String> = Vec::new();
    for row in rows {
        if !receiver_order.iter().any(|r| r == &row.receiver_id) {
            receiver_order.push(row.receiver_id.clone());
        }
    }
    let mut batches = Vec::with_capacity(receiver_order.len());
    let mut processed = Vec::with_capacity(rows.len());
    for receiver_id in &receiver_order {
        let mut payload: Map<String, Value> = Map::new();
        let mut issue_order: Vec<Option<String>> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        for row in rows.iter().filter(|r| &r.receiver_id == receiver_id) {
            let issue_key = row.entity_identifier.clone();
            let entry = issue_key.clone().unwrap_or_default();
            if !issue_order.contains(&issue_key) {
                issue_order.push(issue_key);
            }
            let actors = payload
                .entry(entry)
                .or_insert_with(|| Value::Object(Map::new()));
            let list = actors
                .as_object_mut()
                .expect("issue entry is always an object")
                .entry(row.triggered_by_id.clone())
                .or_insert_with(|| Value::Array(Vec::new()));
            list.as_array_mut()
                .expect("actor entry is always an array")
                .push(row.data.clone());
            processed.push(row.id.clone());
            // BUG (:72): receiver-wide, not per-issue.
            ids.push(row.id.clone());
        }
        let batches_for_receiver = issue_order
            .into_iter()
            .map(|issue_key| {
                let entry = issue_key.clone().unwrap_or_default();
                let notification_data = match payload.remove(&entry) {
                    Some(Value::Object(map)) => map,
                    _ => Map::new(),
                };
                IssueBatch {
                    issue_id: issue_key,
                    notification_data,
                }
            })
            .collect();
        batches.push(ReceiverBatch {
            receiver_id: receiver_id.clone(),
            batches: batches_for_receiver,
            email_notification_ids: ids,
        });
    }
    (batches, processed)
}

/// `.delay()` equivalent for one fanned-out send (`:75-81`): kwargs
/// insertion order is `issue_id, notification_data, receiver_id,
/// email_notification_ids` (the `F-WIRE-MAIL` `cpython_kwargsrepr`
/// order); `issue_id` is the `entity_identifier` UUID object, i.e. its
/// string form at the JSON boundary (`None`/null preserved).
pub fn stack_delay_message(
    issue_id: Option<&str>,
    notification_data: Map<String, Value>,
    receiver_id: &str,
    email_notification_ids: &[String],
) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert(
        "issue_id".to_owned(),
        issue_id
            .map(|id| Value::String(id.to_owned()))
            .unwrap_or(Value::Null),
    );
    kwargs.insert(
        "notification_data".to_owned(),
        Value::Object(notification_data),
    );
    kwargs.insert(
        "receiver_id".to_owned(),
        Value::String(receiver_id.to_owned()),
    );
    kwargs.insert(
        "email_notification_ids".to_owned(),
        Value::Array(
            email_notification_ids
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    CeleryTaskMessage::new(super::SEND_EMAIL_NOTIFICATION_TASK, Vec::new(), kwargs)
}

/// Every way the stack task can fail. Python (`:46-84`) has NO
/// `try/except`: any exception propagates to Celery (the retry path —
/// `F-COMMON.exception_handling`: "NONE — exceptions propagate").
/// The handler maps these to [`Verdict::Fail`] (the worker's park path,
/// mirroring the deletion-task precedent).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackError {
    /// A store read/write failed.
    #[error("stack store error: {0}")]
    Store(String),
    /// A fan-out publish failed.
    #[error("stack outbox error: {0}")]
    Outbox(String),
}

/// The stack task's database surface: the `:49` input query plus the
/// `:84` `processed_at` update. The `sqlx` implementation is
/// [`SqlStackStore`]; tests use a recording fake.
pub trait StackStore: Send + Sync {
    /// `EmailNotificationLog.objects.filter(processed_at__isnull=True)
    /// .order_by("receiver").values()` (`:49`).
    fn unprocessed_logs(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<StackRow>, StackError>> + Send;
    /// `…​.filter(pk__in=processed_notifications).update(processed_at=now())`
    /// (`:84`): unconditional — runs even when `processed` is empty.
    fn mark_processed(
        &self,
        processed: &[String],
    ) -> impl std::future::Future<Output = Result<(), StackError>> + Send;
}

/// The stack task's fan-out surface: `send_email_notification.delay(…)`
/// (`:75-81`). The live implementation publishes over AMQP
/// ([`AmqpStackOutbox`]); tests use a recording fake.
pub trait StackOutbox: Send + Sync {
    fn delay_send(
        &self,
        message: CeleryTaskMessage,
    ) -> impl std::future::Future<Output = Result<(), StackError>> + Send;
}

/// `SELECT id, receiver_id, triggered_by_id, entity_identifier, data
/// FROM email_notification_logs WHERE processed_at IS NULL ORDER BY
/// receiver_id` — the `:49` ORM chain as SQL (`email_notification_logs`
/// per `db/models/notification.py:121-148`; `ORDER BY "receiver"` is the
/// `receiver_id` column). Only the consumed columns are selected: the
/// row's other columns are never read downstream, so they are
/// unobservable.
pub const STACK_SELECT_SQL: &str = "SELECT id, receiver_id, triggered_by_id, entity_identifier, data FROM email_notification_logs WHERE processed_at IS NULL ORDER BY receiver_id";

/// `UPDATE email_notification_logs SET processed_at = now() WHERE id =
/// ANY($1)` — the `:84` update as SQL (a no-op for an empty id list,
/// exactly like the ORM call).
pub const STACK_MARK_PROCESSED_SQL: &str =
    "UPDATE email_notification_logs SET processed_at = now() WHERE id = ANY($1)";

/// Live [`StackStore`] over the primary pool. Reads go to the primary:
/// a background task has no request context to opt into the replica,
/// and the rows it marks seconds later must be the rows it read.
pub struct SqlStackStore {
    pool: sqlx::PgPool,
}

impl SqlStackStore {
    /// Borrow the primary pool handle (cloned — `PgPool` is cheap to clone).
    pub fn new(pools: &pidash_db::Pools) -> Self {
        Self {
            pool: pools.primary().clone(),
        }
    }
}

impl StackStore for SqlStackStore {
    async fn unprocessed_logs(&self) -> Result<Vec<StackRow>, StackError> {
        use sqlx::Row;
        let rows = sqlx::query(STACK_SELECT_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StackError::Store(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: uuid::Uuid = row
                .try_get("id")
                .map_err(|e| StackError::Store(e.to_string()))?;
            let receiver_id: uuid::Uuid = row
                .try_get("receiver_id")
                .map_err(|e| StackError::Store(e.to_string()))?;
            let triggered_by_id: uuid::Uuid = row
                .try_get("triggered_by_id")
                .map_err(|e| StackError::Store(e.to_string()))?;
            let entity_identifier: Option<uuid::Uuid> = row
                .try_get("entity_identifier")
                .map_err(|e| StackError::Store(e.to_string()))?;
            let data: Option<Value> = row
                .try_get("data")
                .map_err(|e| StackError::Store(e.to_string()))?;
            out.push(StackRow {
                id: id.to_string(),
                receiver_id: receiver_id.to_string(),
                triggered_by_id: triggered_by_id.to_string(),
                entity_identifier: entity_identifier.map(|u| u.to_string()),
                data: data.unwrap_or(Value::Null),
            });
        }
        Ok(out)
    }

    async fn mark_processed(&self, processed: &[String]) -> Result<(), StackError> {
        let ids: Vec<uuid::Uuid> = processed
            .iter()
            .map(|id| {
                id.parse::<uuid::Uuid>()
                    .map_err(|e| StackError::Store(format!("bad log id {id:?}: {e}")))
            })
            .collect::<Result<_, _>>()?;
        sqlx::query(STACK_MARK_PROCESSED_SQL)
            .bind(ids)
            .execute(&self.pool)
            .await
            .map_err(|e| StackError::Store(e.to_string()))?;
        Ok(())
    }
}

/// Live [`StackOutbox`] over the AMQP publisher: the coexistence path
/// that carries the fanned-out sends to the Python workers until the
/// domain gate flips ownership.
pub struct AmqpStackOutbox<'a> {
    publisher: &'a crate::amqp::Publisher,
}

impl<'a> AmqpStackOutbox<'a> {
    /// Borrow the worker's publisher.
    pub fn new(publisher: &'a crate::amqp::Publisher) -> Self {
        Self { publisher }
    }
}

impl StackOutbox for AmqpStackOutbox<'_> {
    async fn delay_send(&self, message: CeleryTaskMessage) -> Result<(), StackError> {
        self.publisher
            .publish(&message)
            .await
            .map_err(|e| StackError::Outbox(e.to_string()))
    }
}

/// What one stack run did (observability for logs and tests).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StackReport {
    /// Receivers fanned out to.
    pub receivers: usize,
    /// `send_email_notification.delay` calls issued.
    pub fanned: usize,
    /// Rows marked `processed_at` (== input row count).
    pub processed: usize,
}

/// Drive one `stack_email_notification` run (`:46-84`): read, group,
/// fan out per issue, mark processed — the mark runs unconditionally,
/// even when nothing fanned out (`:84`).
pub async fn run_stack(
    store: &impl StackStore,
    outbox: &impl StackOutbox,
) -> Result<StackReport, StackError> {
    let rows = store.unprocessed_logs().await?;
    let (batches, processed) = plan_stack(&rows);
    let mut fanned = 0;
    for receiver in &batches {
        for batch in &receiver.batches {
            outbox
                .delay_send(stack_delay_message(
                    batch.issue_id.as_deref(),
                    batch.notification_data.clone(),
                    &receiver.receiver_id,
                    &receiver.email_notification_ids,
                ))
                .await?;
            fanned += 1;
        }
    }
    store.mark_processed(&processed).await?;
    Ok(StackReport {
        receivers: batches.len(),
        fanned,
        processed: processed.len(),
    })
}

/// Handler for [`super::STACK_EMAIL_NOTIFICATION_TASK`]: run and settle.
/// Python has no `try/except` here, so a failure is a task-level failure
/// ([`Verdict::Fail`]; the worker parks the row — the deletion-task
/// precedent for task-level failures).
pub fn stack_handler<S: StackStore + 'static, O: StackOutbox + 'static>(
    store: Arc<S>,
    outbox: Arc<O>,
) -> Handler {
    Arc::new(move |_job: crate::queue::JobRow| {
        let store = store.clone();
        let outbox = outbox.clone();
        Box::pin(async move {
            match run_stack(store.as_ref(), outbox.as_ref()).await {
                Ok(_) => Ok(Verdict::Ack),
                Err(error) => Ok(Verdict::Fail {
                    error: error.to_string(),
                }),
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

// ---- send_email_notification (`:152-306`) ----

/// `lock_id = f"send_email_notif_{issue_id}_{receiver_id}_{ids_str}"`
/// with `sorted(email_notification_ids)` joined by `_` (`:153-157`).
/// Sorted lexicographically on the wire-string form (canonical
/// lowercase UUID hex — the same order as UUID comparison).
pub fn send_lock_id(
    issue_id: &str,
    receiver_id: &str,
    email_notification_ids: &[String],
) -> String {
    let mut sorted = email_notification_ids.to_vec();
    sorted.sort();
    format!(
        "send_email_notif_{issue_id}_{receiver_id}_{}",
        sorted.join("_")
    )
}

/// Human click-time for one template row: `datetime.strptime(
/// activity_time, "%Y-%m-%d %H:%M:%S").strftime("%H:%M %p")` (`:227`).
/// Naive both ways — the semantic trap in the Porting guide: NO tz
/// conversion happens, wall time passes through. `%H` stays 24-hour
/// next to AM/PM (`13:00` → `"13:00 PM"` — ported exactly).
/// Only zero-padded producer output is accepted (`create_payload`
/// always emits `%Y-%m-%d %H:%M:%S` via `strftime`).
pub fn format_activity_time(activity_time: &str) -> Result<String, SendError> {
    NaiveDateTime::parse_from_str(activity_time, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.format("%H:%M %p").to_string())
        .map_err(|_| SendError::BadActivityTime(activity_time.to_owned(), activity_time.to_owned()))
}

/// `subject = f"{issue.project.identifier}-{issue.sequence_id}
/// {remove_unwanted_characters(issue.name)}"` (`:243`).
///
/// Name note: the issue text says "{stripped name}" but the Python
/// applies only `remove_unwanted_characters` (control-char strip) —
/// no `.strip()`. The Python is ported (no strip); leading/trailing
/// spaces survive exactly as Django sends them.
pub fn build_subject(project_identifier: &str, sequence_id: i64, issue_name: &str) -> String {
    format!(
        "{project_identifier}-{sequence_id} {}",
        remove_unwanted_characters(issue_name)
    )
}

/// One actor's send-time snapshot: `User.objects.get(pk=actor_id)`
/// (`:191`) projected to the three template fields. `avatar_url` is
/// the `avatar_url` property (`user.py:143-154`): asset URL, else the
/// `avatar` text, else `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorSnapshot {
    pub first_name: String,
    pub last_name: String,
    pub avatar_url: Option<String>,
}

/// The issue plus its `project`/`workspace` joins, projected to the
/// fields the send uses (`:184`, `:236-259`): `issues` → `projects`
/// (`identifier`, `name`) → `workspaces` (`slug`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueSnapshot {
    pub id: String,
    pub name: String,
    pub identifier: String,
    pub sequence_id: i64,
    pub project_id: String,
    pub project_name: String,
    pub workspace_slug: String,
}

/// `f"{base_api}{actor.avatar_url}"` (`:199`, `:209`, `:232`):
/// `None` renders as the four letters `None` (f-string of `None`).
pub fn avatar_src(base_api: &str, avatar_url: Option<&str>) -> String {
    format!("{base_api}{}", avatar_url.unwrap_or("None"))
}

/// One `actor_detail` template dict (`:197-203`, `:207-213`, `:230-234`).
fn actor_detail(base_api: &str, actor: &ActorSnapshot) -> Map<String, Value> {
    let mut detail = Map::new();
    detail.insert(
        "avatar_url".to_owned(),
        Value::String(avatar_src(base_api, actor.avatar_url.as_deref())),
    );
    detail.insert(
        "first_name".to_owned(),
        Value::String(actor.first_name.clone()),
    );
    detail.insert(
        "last_name".to_owned(),
        Value::String(actor.last_name.clone()),
    );
    detail
}

/// Python truthiness for the `comment`/`mention` pops (`:192-193`):
/// `if comment:` / `if mention:` (`:194`, `:203`). `False` (the
/// `.pop(…, False)` default), `None`, `""`, `[]`, `{}` and `0` are
/// falsy; everything else is truthy.
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Plain-text alternative: `generate_plain_text_from_html(html_content)`
/// (`utils/email.py:18-44`, called at `:264`): drop `<style>…</style>`
/// blocks, strip tags, collapse 3+-newline runs to `\n\n`, wrap in
/// `\n\n…\n\n`.
///
/// Tag stripping is quote-aware and skips `<!--…-->` comments.
/// Entities are preserved verbatim (Django parses with
/// `convert_charrefs=False` — proven by probe).
pub fn plain_text_from_html(html: &str) -> String {
    let no_style = strip_style_blocks(html);
    let stripped = strip_tags(&no_style);
    let collapsed = collapse_blank_runs(&stripped);
    format!("\n\n{}\n\n", collapsed.trim())
}

/// Remove `<style …>…</style>` spans, case-insensitive, dot-all
/// (`re.sub(r"<style[^>]*>.*?</style>", "", …, DOTALL | IGNORECASE)`).
fn strip_style_blocks(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = find_insensitive(rest, "<style") {
        let after_open = &rest[start..];
        let open_end = match after_open.find('>') {
            Some(j) => start + j + 1,
            None => break,
        };
        match find_insensitive(&rest[open_end..], "</style>") {
            Some(j) => {
                out.push_str(&rest[..start]);
                rest = &rest[open_end + j + "</style>".len()..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn find_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| {
        haystack.is_char_boundary(i)
            && haystack[i..]
                .get(..needle.len())
                .map(|s| s.eq_ignore_ascii_case(needle))
                .unwrap_or(false)
    })
}

/// Strip every `<…>` tag (quote-aware; comments removed wholesale).
///
/// A `<` only opens a tag when followed by a letter, `/`, `!` or `?`
/// (the HTMLParser rule Django's `strip_tags` inherits) — `a < b` stays
/// literal text. Character references are NOT decoded: Django parses
/// with `convert_charrefs=False`, so `&amp;` survives into the plain
/// text verbatim (proven by probe against Django 4.2.30).
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        if html[i..].starts_with("<!--") {
            i = html[i..]
                .find("-->")
                .map(|j| i + j + 3)
                .unwrap_or(html.len());
            continue;
        }
        if html[i..].starts_with('<') {
            let opens_tag = html[i + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?');
            if opens_tag {
                let bytes = html.as_bytes();
                let mut j = i + 1;
                let mut quote: Option<u8> = None;
                while j < bytes.len() {
                    let c = bytes[j];
                    if let Some(q) = quote {
                        if c == q {
                            quote = None;
                        }
                    } else if c == b'"' || c == b'\'' {
                        quote = Some(c);
                    } else if c == b'>' {
                        break;
                    }
                    j += 1;
                }
                i = (j + 1).min(html.len());
                continue;
            }
        }
        let ch = html[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8().max(1);
    }
    out
}

/// Collapse runs of 3+ newlines (allowing whitespace-only lines
/// between) to `\n\n` (`re.sub(r"\n\s*\n\s*\n+", "\n\n", …)`).
fn collapse_blank_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut newlines = 0usize;
    let mut pending_ws = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            newlines += 1;
            continue;
        }
        if newlines > 0 && ch.is_whitespace() {
            pending_ws.push(ch);
            continue;
        }
        if newlines >= 3 {
            out.push_str("\n\n");
        } else {
            for _ in 0..newlines {
                out.push('\n');
            }
            out.push_str(&pending_ws);
        }
        newlines = 0;
        pending_ws.clear();
        out.push(ch);
    }
    if newlines >= 3 {
        out.push_str("\n\n");
    } else {
        for _ in 0..newlines {
            out.push('\n');
        }
        out.push_str(&pending_ws);
    }
    out
}

/// The 7-tuple from `get_email_configuration()`
/// (`license/utils/instance_value.py:57-74`; `F-COMMON`): host, user,
/// password, port (`int(EMAIL_PORT)`, default int `587`), TLS/SSL
/// flags (`=="1"` string compares), sender.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailConfig {
    pub host: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Raw `EMAIL_PORT` (int `587` default or a DB/env string).
    pub port: Value,
    /// Raw `EMAIL_USE_TLS` — truthy iff exactly `"1"`.
    pub use_tls: Value,
    /// Raw `EMAIL_USE_SSL` — truthy iff exactly `"1"`.
    pub use_ssl: Value,
    pub from_email: String,
}

/// `port=int(EMAIL_PORT)` (`:267`): ints pass through, strings parse
/// (surrounding whitespace tolerated, like `int()`). `None`/bool/garbage
/// raises in Python — `None` here, and the caller takes the
/// `log_exception` + release + return path. Micro-divergence: Python's
/// `int()` also accepts underscores (`"5_87"` → `587`); no real
/// configuration does this, and both sides fail closed on garbage.
pub fn smtp_port(port: &Value) -> Option<i64> {
    match port {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `EMAIL_USE_TLS == "1"` / `EMAIL_USE_SSL == "1"` (`:271-272`):
/// Python string compares — only the exact string `"1"` is truthy
/// (integer `1` and boolean `true` are NOT; `F-COMMON`).
pub fn smtp_flag(value: &Value) -> bool {
    matches!(value, Value::String(s) if s == "1")
}

/// One send's SMTP call shapes (`:266-284`): `get_connection(host,
/// int(port), username, password, use_tls, use_ssl)` then
/// `EmailMultiAlternatives(subject, body=text, from_email, to=[email])`,
/// `attach_alternative(html, "text/html")`, `send()`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmtpEnvelope {
    pub host: Option<String>,
    pub port: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub use_ssl: bool,
    pub from_email: String,
    /// `to=[receiver.email]` — `None` when the column is NULL (a real
    /// client fails the send, mirroring the Python raise inside the
    /// send-block `try`).
    pub to: Option<String>,
    pub subject: String,
    pub text_content: String,
    pub html_content: String,
}

/// Every mid-send failure the orchestration maps to a lock/exit path.
/// The mapping lives in [`run_send`]:
///
/// - [`SendError::UserNotFound`] is a `DoesNotExist` (user or, via
///   [`MailDbError::NotFound`], receiver/issue/actor): release silently.
/// - everything else is a generic `Exception`: `log_exception` +
///   release + return (`:303-306`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// An actor's payload entry is not an object (Python
    /// `AttributeError` on `.pop`, `:192-193`).
    #[error("payload for actor {0} is not an object")]
    BadActorPayload(String),
    /// `changes.pop("activity_time")` raised `KeyError` (`:225`).
    #[error("payload lacks activity_time for actor {0}")]
    MissingActivityTime(String),
    /// `activity_time` is not a string, or `strptime` raised
    /// `ValueError` (`:227`).
    #[error("bad activity_time {1:?} for actor {0}")]
    BadActivityTime(String, String),
    /// `User.objects.get(pk)` raised `DoesNotExist` — from a per-actor
    /// read (`:191`) or a per-mention read inside `process_mention`
    /// (`:136`). Silent release + return.
    #[error("user {0} does not exist")]
    UserNotFound(String),
    /// A non-`DoesNotExist` mention failure: missing
    /// `entity_identifier` (`KeyError`, `:135`) or a bad content shape.
    #[error("mention error: {0}")]
    Mention(#[from] MentionError),
    /// Defensive only: [`run_send`] pre-fetches every actor in the
    /// payload, so the plan always finds its actors. Unreachable unless
    /// the payload is mutated between the two steps.
    #[error("unknown actor {0}")]
    UnknownActor(String),
}

/// A located `<mention-component>`: byte span plus the resolved
/// `entity_identifier` (missing attribute is already a
/// [`MentionError::MissingEntityIdentifier`] here).
struct MentionSpan {
    start: usize,
    end: usize,
    entity_id: String,
}

/// Phase 1 of mention rewriting: locate every element span in order
/// (same traversal as [`process_mention`).
fn extract_mentions(html: &str) -> Result<Vec<MentionSpan>, MentionError> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < html.len() {
        if html[i..].starts_with("<!--") {
            i = html[i..]
                .find("-->")
                .map(|j| i + j + 3)
                .unwrap_or(html.len());
            continue;
        }
        if let Some(tag) = match_open_tag(&html[i..]) {
            let entity_id = tag
                .entity_identifier
                .ok_or(MentionError::MissingEntityIdentifier)?;
            let end = match_close(&html[i..], tag.open_len).map(|e| i + e);
            spans.push(MentionSpan {
                start: i,
                end: end.unwrap_or(i + tag.open_len),
                entity_id,
            });
            i = spans.last().expect("just pushed").end;
            continue;
        }
        i += html[i..]
            .chars()
            .next()
            .map(|c| c.len_utf8().max(1))
            .unwrap_or(1);
    }
    Ok(spans)
}

/// Phase 3 of mention rewriting: splice resolved `@name`s into the
/// spans (names escaped exactly like [`process_mention`]).
fn splice_mentions(html: &str, spans: &[MentionSpan], names: &[String]) -> String {
    debug_assert_eq!(spans.len(), names.len());
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    for (span, name) in spans.iter().zip(names.iter()) {
        out.push_str(&html[cursor..span.start]);
        out.push('@');
        out.push_str(&escape_bs_text(name));
        cursor = span.end;
    }
    out.push_str(&html[cursor..]);
    out
}

/// Async per-mention display-name source. The production implementation
/// calls `User.objects.get(pk)` + `.display_name` FRESH per occurrence
/// (N SELECTs preserved in shape — no dedup, no cache).
pub trait MentionSource: Send + Sync {
    /// One mention's `User.objects.get(pk=user_id).display_name`.
    fn display_name(
        &self,
        user_id: &str,
    ) -> impl std::future::Future<Output = Result<String, MentionLookupError>> + Send;
}

/// Rewrite one `mention` slot's `new_value`/`old_value` through
/// `process_html_content` (`:205-206`): each slot is `None`/null (stays
/// `None`) or a list of HTML strings. The key is always written back —
/// even when it was absent (`mention.get(…)` → `None` → assigned).
async fn rewrite_mention_slots(
    mention: &mut Map<String, Value>,
    mentions: &impl MentionSource,
) -> Result<(), SendError> {
    for slot in ["new_value", "old_value"] {
        let current = mention.get(slot).cloned();
        let rewritten = match current {
            None | Some(Value::Null) => Value::Null,
            Some(Value::Array(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let html = match item {
                        Value::String(html) => html,
                        _ => return Err(SendError::Mention(MentionError::BadShape)),
                    };
                    out.push(Value::String(rewrite_html(&html, mentions).await?));
                }
                Value::Array(out)
            }
            Some(_) => return Err(SendError::Mention(MentionError::BadShape)),
        };
        mention.insert(slot.to_owned(), rewritten);
    }
    Ok(())
}

/// `process_html_content` over one HTML string with an async lookup:
/// extract spans, resolve each display name with a fresh call (no
/// dedup — the N-query shape), splice.
async fn rewrite_html(html: &str, mentions: &impl MentionSource) -> Result<String, SendError> {
    let spans = extract_mentions(html)?;
    let mut names = Vec::with_capacity(spans.len());
    for span in &spans {
        let name = mentions
            .display_name(&span.entity_id)
            .await
            .map_err(|_| SendError::UserNotFound(span.entity_id.clone()))?;
        names.push(name);
    }
    Ok(splice_mentions(html, &spans, &names))
}

/// The per-actor plan: template rows plus comment entries (`:189-239`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SendPlan {
    /// `template_data` rows (`:228-239`): actors with residual changes.
    pub template_data: Vec<Map<String, Value>>,
    /// `comments` entries (`:194-221`): `comment` then `mention` pops.
    pub comments: Vec<Map<String, Value>>,
    /// `actors_involved` (`:193`, counted as a set at `:246`).
    pub actors_involved: Vec<String>,
}

/// Build the per-actor plan from the `create_payload` output (`:189-239`).
/// `data` is mutated exactly like the Python local (comment/mention/
/// activity_time popped per actor). `actors` holds one pre-fetched
/// snapshot per payload key (the `:191` SELECT each — same count, same
/// order); a missing key is [`SendError::UnknownActor`] (defensive:
/// [`run_send`] fetches every key first).
///
/// `total_changes` (`:192`) is deliberately NOT accumulated: it is
/// write-only in Python (never read by the template or context —
/// `PORT_NOTE_template` in `F-SEND`), so carrying it would be dead data.
pub async fn plan_send_actors(
    mut data: Map<String, Value>,
    actors: &std::collections::HashMap<String, ActorSnapshot>,
    issue: &IssueSnapshot,
    base_api: &str,
    mentions: &impl MentionSource,
) -> Result<SendPlan, SendError> {
    let mut plan = SendPlan::default();
    // `Map` iteration is insertion order (`preserve_order`) — the same
    // order as the Python `data.items()` loop.
    let actor_ids: Vec<String> = data.keys().cloned().collect();
    for actor_id in &actor_ids {
        let actor = actors
            .get(actor_id)
            .ok_or_else(|| SendError::UnknownActor(actor_id.clone()))?;
        let changes = data
            .get_mut(actor_id)
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| SendError::BadActorPayload(actor_id.clone()))?;
        let detail = actor_detail(base_api, actor);
        if let Some(comment) = changes.remove("comment") {
            if py_truthy(&comment) {
                let mut entry = Map::new();
                entry.insert("actor_comments".to_owned(), comment);
                entry.insert("actor_detail".to_owned(), Value::Object(detail.clone()));
                plan.comments.push(entry);
            }
        }
        if let Some(mention) = changes.remove("mention") {
            if py_truthy(&mention) {
                let mut mention = match mention {
                    Value::Object(map) => map,
                    _ => return Err(SendError::BadActorPayload(actor_id.clone())),
                };
                rewrite_mention_slots(&mut mention, mentions).await?;
                let mut entry = Map::new();
                entry.insert("actor_comments".to_owned(), Value::Object(mention));
                entry.insert("actor_detail".to_owned(), Value::Object(detail.clone()));
                plan.comments.push(entry);
            }
        }
        let activity_time = changes
            .remove("activity_time")
            .ok_or_else(|| SendError::MissingActivityTime(actor_id.clone()))?;
        let activity_time = match &activity_time {
            Value::String(raw) => format_activity_time(raw)
                .map_err(|_| SendError::BadActivityTime(actor_id.clone(), raw.clone()))?,
            other => {
                return Err(SendError::BadActivityTime(actor_id.clone(), py_str(other)));
            }
        };
        if !changes.is_empty() {
            let mut row = Map::new();
            row.insert("actor_detail".to_owned(), Value::Object(detail));
            row.insert("changes".to_owned(), Value::Object(std::mem::take(changes)));
            let mut issue_details = Map::new();
            issue_details.insert("name".to_owned(), Value::String(issue.name.clone()));
            issue_details.insert(
                "identifier".to_owned(),
                Value::String(format!("{}-{}", issue.identifier, issue.sequence_id)),
            );
            row.insert("issue_details".to_owned(), Value::Object(issue_details));
            row.insert("activity_time".to_owned(), Value::String(activity_time));
            plan.template_data.push(row);
        }
        plan.actors_involved.push(actor_id.clone());
    }
    Ok(plan)
}

/// Assemble the `render_to_string` context (`:244-261`), insertion
/// order as in Python (the `F-SEND` `context_keys` list).
pub fn build_context(
    plan: &SendPlan,
    issue: &IssueSnapshot,
    receiver_email: Option<&str>,
    base_api: &str,
) -> Map<String, Value> {
    let issue_identifier = format!("{}-{}", issue.identifier, issue.sequence_id);
    let issue_url = format!(
        "{}/{}/projects/{}/issues/{}",
        base_api, issue.workspace_slug, issue.project_id, issue.id
    );
    let mut context = Map::new();
    context.insert(
        "data".to_owned(),
        Value::Array(
            plan.template_data
                .iter()
                .cloned()
                .map(Value::Object)
                .collect(),
        ),
    );
    context.insert("summary".to_owned(), Value::String(SUMMARY.to_owned()));
    let mut involved = plan.actors_involved.clone();
    involved.sort();
    involved.dedup();
    context.insert(
        "actors_involved".to_owned(),
        Value::Number(serde_json::Number::from(involved.len())),
    );
    let mut issue_ctx = Map::new();
    issue_ctx.insert(
        "issue_identifier".to_owned(),
        Value::String(issue_identifier),
    );
    issue_ctx.insert("name".to_owned(), Value::String(issue.name.clone()));
    issue_ctx.insert("issue_url".to_owned(), Value::String(issue_url.clone()));
    context.insert("issue".to_owned(), Value::Object(issue_ctx));
    let mut receiver = Map::new();
    receiver.insert(
        "email".to_owned(),
        receiver_email
            .map(|e| Value::String(e.to_owned()))
            .unwrap_or(Value::Null),
    );
    context.insert("receiver".to_owned(), Value::Object(receiver));
    context.insert("issue_url".to_owned(), Value::String(issue_url));
    context.insert(
        "project_url".to_owned(),
        Value::String(format!(
            "{}/{}/projects/{}/issues/",
            base_api, issue.workspace_slug, issue.project_id
        )),
    );
    context.insert(
        "workspace".to_owned(),
        Value::String(issue.workspace_slug.clone()),
    );
    context.insert(
        "project".to_owned(),
        Value::String(issue.project_name.clone()),
    );
    context.insert(
        "user_preference".to_owned(),
        Value::String(format!(
            "{}/{}/settings/account/notifications/",
            base_api, issue.workspace_slug
        )),
    );
    context.insert(
        "comments".to_owned(),
        Value::Array(plan.comments.iter().cloned().map(Value::Object).collect()),
    );
    context.insert(
        "entity_type".to_owned(),
        Value::String(ENTITY_TYPE_ISSUE.to_owned()),
    );
    context
}

/// What the send task's reads can report. `NotFound` is a Django
/// `DoesNotExist` (`User` or `Issue`): release the lock and return
/// silently (`:303-305`). `Db` is anything else: `log_exception` +
/// release + return (`:306-309`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailDbError {
    /// `User.DoesNotExist` / `Issue.DoesNotExist`.
    NotFound(String),
    /// Any other read/write failure.
    Db(String),
}

impl From<MentionLookupError> for MailDbError {
    fn from(error: MentionLookupError) -> Self {
        match error {
            MentionLookupError::NotFound(id) => MailDbError::NotFound(id),
        }
    }
}

/// The send task's database surface (`:170-184`, `:287`):
///
/// - `receiver = User.objects.get(pk=receiver_id)` (`:183`)
/// - `issue = Issue.objects.get(pk=issue_id)` (`:184`, with the
///   `project`/`workspace` joins the template reads through)
/// - `actor = User.objects.get(pk=actor_id)` per payload key (`:191`)
/// - one `User.objects.get(pk)` + `.display_name` per mention
///   occurrence via [`MentionSource`] (`:136`)
/// - `get_email_configuration()` (`:173-181`)
/// - `EmailNotificationLog.objects.filter(pk__in=email_notification_ids)
///   .update(sent_at=timezone.now())` (`:287`): `sent_at` covers the
///   receiver-wide id list — the same PORT BUG as the fan-out
///   (`:72-80`).
///
/// The concrete `sqlx` implementation lands with the domain gate
/// (PIDASHCONV-218); until then this trait records the exact read
/// shapes, and the orchestration below is pinned by fakes.
pub trait MailDb: MentionSource {
    /// `User.objects.get(pk=receiver_id)` projected to the only field
    /// the send reads: `receiver.email` (`:251`, `:280`).
    fn receiver_email(
        &self,
        receiver_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<String>, MailDbError>> + Send;
    /// `User.objects.get(pk=actor_id)` (`:191`) projected to the
    /// template fields.
    fn actor(
        &self,
        actor_id: &str,
    ) -> impl std::future::Future<Output = Result<ActorSnapshot, MailDbError>> + Send;
    /// `Issue.objects.get(pk=issue_id)` (`:184`) with joins. `None` is
    /// the null-`entity_identifier` stack row: `Issue.objects.get(pk=None)`
    /// raises `DoesNotExist` (silent release + return — preserved).
    fn issue(
        &self,
        issue_id: Option<&str>,
    ) -> impl std::future::Future<Output = Result<IssueSnapshot, MailDbError>> + Send;
    /// `get_email_configuration()` (`:173-181`).
    fn email_config(
        &self,
    ) -> impl std::future::Future<Output = Result<EmailConfig, MailDbError>> + Send;
    /// `…​.filter(pk__in=email_notification_ids).update(sent_at=now())`
    /// (`:287`): the receiver-wide id list (PORT BUG, same as fan-out).
    fn mark_sent(
        &self,
        email_notification_ids: &[String],
    ) -> impl std::future::Future<Output = Result<(), MailDbError>> + Send;
}

/// The send task's Redis surface: the part-1 lock shapes plus the
/// `base_api` GET. `ri.get(str(issue_id))` (`:164`): redis-py returns
/// bytes or `None`; `.decode()` on a hit. The recorded shape is the
/// decoded string / absent key.
pub trait RedisMail: RedisLock + Send + Sync {
    /// `ri.get(str(issue_id))`, decoded — `None` when absent.
    fn get(&self, key: &str) -> Option<String>;
}

/// Template rendering: `render_to_string(
/// "emails/notifications/issue-updates.html", context)` (`:263`) with
/// `minijinja` strict-undefined semantics (Porting guide): a missing
/// key raises instead of rendering empty — the Django `string_if_invalid`
/// default renders `''` for the never-sent `current_site` /
/// `total_updates` / `total_comments` keys (`PORT_NOTE_template` in
/// `F-SEND`, proven by the rendered golden), which the strict port
/// reproduces by rendering those three as `""`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The template or context failed to render.
    #[error("render failed: {0}")]
    Failed(String),
}

/// Render `issue-updates.html` over the [`build_context`] map.
/// The `minijinja` implementation lands with the domain gate; this
/// trait records the exact input (full context map) and output (HTML).
pub trait Renderer: Send + Sync {
    /// `render_to_string(ISSUE_UPDATES_TEMPLATE, context)`.
    fn render_issue_updates(&self, context: &Map<String, Value>) -> Result<String, RenderError>;
}

/// SMTP sending (`:266-284`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MailError {
    /// `get_connection` / `EmailMultiAlternatives` / `send` failed.
    #[error("smtp send failed: {0}")]
    Failed(String),
}

/// `get_connection(…)` + `EmailMultiAlternatives(…).send()` over one
/// [`SmtpEnvelope`]. The `lettre` implementation lands with the domain
/// gate; this trait records the exact call shapes.
pub trait Mailer: Send + Sync {
    /// Connect, build, attach and send one message.
    fn send(&self, envelope: &SmtpEnvelope) -> Result<(), MailError>;
}

/// One `send_email_notification` invocation after kwarg parsing:
/// `send_email_notification.delay(issue_id=…, notification_data=…,
/// receiver_id=…, email_notification_ids=…)` (`:75-81`). `issue_id` is
/// `Option` for the null-`entity_identifier` stack row (Python receives
/// `None` and raises `DoesNotExist` on the issue read — preserved via
/// [`MailDb::issue`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SendCall {
    pub issue_id: Option<String>,
    pub notification_data: Map<String, Value>,
    pub receiver_id: String,
    pub email_notification_ids: Vec<String>,
}

/// Every way a broker call can fail to parse. Python raises `TypeError`
/// at the call boundary (missing/unexpected args); the handler maps
/// these to [`Verdict::Fail`] (the Celery-failure equivalent — the send
/// body itself never raises out, so a parse failure is the only
/// handler-level failure).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// Positional args were passed (`.delay()` is kwargs-only).
    #[error("send_email_notification takes no positional args")]
    UnexpectedArgs,
    /// A required kwarg is absent.
    #[error("missing kwarg {0}")]
    MissingKwarg(&'static str),
    /// A kwarg has the wrong JSON shape.
    #[error("bad kwarg {0}")]
    BadKwarg(&'static str),
}

/// Parse one Celery-body call into a [`SendCall`]. `issue_id` accepts
/// `null` (the null-entity stack row); every other kwarg is required
/// with its wire shape (`F-WIRE-MAIL` send entry).
pub fn parse_send_call(args: &Value, kwargs: &Value) -> Result<SendCall, CallError> {
    match args {
        Value::Array(items) if items.is_empty() => {}
        _ => return Err(CallError::UnexpectedArgs),
    }
    let kwargs = match kwargs {
        Value::Object(map) => map,
        _ => return Err(CallError::BadKwarg("kwargs")),
    };
    let issue_id = match kwargs.get("issue_id") {
        None => return Err(CallError::MissingKwarg("issue_id")),
        Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(CallError::BadKwarg("issue_id")),
    };
    let notification_data = match kwargs.get("notification_data") {
        None => return Err(CallError::MissingKwarg("notification_data")),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(CallError::BadKwarg("notification_data")),
    };
    let receiver_id = match kwargs.get("receiver_id") {
        None => return Err(CallError::MissingKwarg("receiver_id")),
        Some(Value::String(id)) => id.clone(),
        Some(_) => return Err(CallError::BadKwarg("receiver_id")),
    };
    let email_notification_ids = match kwargs.get("email_notification_ids") {
        None => return Err(CallError::MissingKwarg("email_notification_ids")),
        Some(Value::Array(ids)) => ids
            .iter()
            .map(|id| match id {
                Value::String(id) => Ok(id.clone()),
                _ => Err(CallError::BadKwarg("email_notification_ids")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(CallError::BadKwarg("email_notification_ids")),
    };
    Ok(SendCall {
        issue_id,
        notification_data,
        receiver_id,
        email_notification_ids,
    })
}

/// `.delay()` equivalent for the send (`:75-81`): same kwargs the
/// stack fan-out passes, same insertion order.
pub fn send_delay_message(call: &SendCall) -> CeleryTaskMessage {
    stack_delay_message(
        call.issue_id.as_deref(),
        call.notification_data.clone(),
        &call.receiver_id,
        &call.email_notification_ids,
    )
}

/// How one send run exited (`:152-306`). The lock discipline is part of
/// the contract — every variant names whether the lock is held:
///
/// - `lock NOT released` on exactly one path: [`SendOutcome::BaseApiMissing`]
///   (the `:167-168` early return — PORT NOTE lock leak, the key lives
///   until `ex=300` expiry).
/// - no release needed on [`SendOutcome::DuplicateSkipped`] (never held).
/// - released on every other path, including all failure paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// Mail sent, `sent_at` updated (`:287`), lock released (`:290`).
    Sent,
    /// Lock contended: [`DUPLICATE_LOG`], return, no release (`:299-301`).
    DuplicateSkipped,
    /// `base_api` absent: silent return WITHOUT releasing the lock
    /// (`:167-168` — the leak, ported exactly).
    BaseApiMissing,
    /// A `DoesNotExist` anywhere (receiver, issue, actor, per-mention
    /// user): lock released, silent return (`:303-305`).
    ReleasedSilent,
    /// Any other `Exception`: `log_exception`, lock released, return.
    /// Carries the logged error text (`:292-294`, `:306-309`).
    ReleasedWithLog(String),
}

/// Drive one `send_email_notification` run (`:152-306`) over the seam
/// traits. Step order mirrors the Python exactly:
///
/// 1. lock (`:158-162`); contended → [`SendOutcome::DuplicateSkipped`].
/// 2. `base_api` GET (`:164`); absent → [`SendOutcome::BaseApiMissing`]
///    holding the lock (the leak).
/// 3. `create_payload` (`:170`); error → logged release (outer except).
/// 4. `get_email_configuration` (`:173-181`); error → logged release.
/// 5. receiver + issue reads (`:183-184`); `NotFound` → silent release.
/// 6. per-actor fetch + [`plan_send_actors`]; `NotFound`/`UserNotFound`
///    → silent release, anything else → logged release.
/// 7. subject (`:243`), context (`:244-261`), render (`:263`),
///    plain text (`:264`), SMTP send (`:266-284`); error → logged
///    release (the send-block `except`, `:292-294`).
/// 8. `sent_at` update (`:287`, receiver-wide ids — the PORT BUG);
///    error → logged release (outer except). Then release → [`SendOutcome::Sent`].
pub async fn run_send(
    redis: &impl RedisMail,
    db: &impl MailDb,
    renderer: &impl Renderer,
    mailer: &impl Mailer,
    call: SendCall,
) -> SendOutcome {
    // `:153-162`. `str(…)` of each id part: the wire form is already
    // strings; a null issue_id renders `"None"` (Python `str(None)`),
    // matching the f-string on the null-entity row.
    let issue_id_str = call.issue_id.as_deref().unwrap_or("None");
    let lock_id = send_lock_id(
        issue_id_str,
        &call.receiver_id,
        &call.email_notification_ids,
    );
    if !acquire_lock(redis, &lock_id, DEFAULT_LOCK_EXPIRE_SECS) {
        return SendOutcome::DuplicateSkipped;
    }
    // `:164-168`. NOTE — no `release_lock` on this path (the leak).
    let base_api = match redis.get(issue_id_str) {
        Some(base_api) => base_api,
        None => return SendOutcome::BaseApiMissing,
    };
    // `:170`. Raised here (outside the send-block try) → outer except.
    let data = match create_payload(&call.notification_data) {
        Ok(data) => data,
        Err(error) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error.to_string());
        }
    };
    // `:173-181`.
    let config = match db.email_config().await {
        Ok(config) => config,
        Err(MailDbError::NotFound(_)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedSilent;
        }
        Err(MailDbError::Db(error)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error);
        }
    };
    // `:183-184`.
    let receiver_email = match db.receiver_email(&call.receiver_id).await {
        Ok(email) => email,
        Err(MailDbError::NotFound(_)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedSilent;
        }
        Err(MailDbError::Db(error)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error);
        }
    };
    let issue = match db.issue(call.issue_id.as_deref()).await {
        Ok(issue) => issue,
        Err(MailDbError::NotFound(_)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedSilent;
        }
        Err(MailDbError::Db(error)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error);
        }
    };
    // `:189-193`: one `:191` SELECT per payload key, in loop order.
    let mut actors = std::collections::HashMap::new();
    for actor_id in data.keys() {
        match db.actor(actor_id).await {
            Ok(actor) => {
                actors.insert(actor_id.clone(), actor);
            }
            Err(MailDbError::NotFound(_)) => {
                release_lock(redis, &lock_id);
                return SendOutcome::ReleasedSilent;
            }
            Err(MailDbError::Db(error)) => {
                release_lock(redis, &lock_id);
                return SendOutcome::ReleasedWithLog(error);
            }
        }
    }
    // `:189-239` (per-actor pops, mention rewrites, time reformat).
    // `UserNotFound` (per-mention `:136` or per-actor `:191`) is a
    // `DoesNotExist` → silent; the rest → logged.
    let plan = match plan_send_actors(data, &actors, &issue, &base_api, db).await {
        Ok(plan) => plan,
        Err(SendError::UserNotFound(_)) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedSilent;
        }
        Err(error) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error.to_string());
        }
    };
    // `:243-264`.
    let subject = build_subject(&issue.identifier, issue.sequence_id, &issue.name);
    let context = build_context(&plan, &issue, receiver_email.as_deref(), &base_api);
    let html_content = match renderer.render_issue_updates(&context) {
        Ok(html) => html,
        Err(error) => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(error.to_string());
        }
    };
    let text_content = plain_text_from_html(&html_content);
    // `:266-284`. `int(EMAIL_PORT)` raises → outer except (logged).
    let port = match smtp_port(&config.port) {
        Some(port) => port,
        None => {
            release_lock(redis, &lock_id);
            return SendOutcome::ReleasedWithLog(format!("bad EMAIL_PORT: {}", config.port));
        }
    };
    let envelope = SmtpEnvelope {
        host: config.host.clone(),
        port,
        username: config.username.clone(),
        password: config.password.clone(),
        use_tls: smtp_flag(&config.use_tls),
        use_ssl: smtp_flag(&config.use_ssl),
        from_email: config.from_email.clone(),
        to: receiver_email,
        subject,
        text_content,
        html_content,
    };
    // `:265-294`: send-block errors → `log_exception` + release.
    if let Err(error) = mailer.send(&envelope) {
        release_lock(redis, &lock_id);
        return SendOutcome::ReleasedWithLog(error.to_string());
    }
    // `:285-290`: log, `sent_at` over the receiver-wide ids (the PORT
    // BUG — same list the fan-out passed in), release, return.
    if let Err(error) = db.mark_sent(&call.email_notification_ids).await {
        release_lock(redis, &lock_id);
        let message = match error {
            MailDbError::NotFound(detail) => detail,
            MailDbError::Db(message) => message,
        };
        return SendOutcome::ReleasedWithLog(message);
    }
    release_lock(redis, &lock_id);
    SendOutcome::Sent
}

/// Handler for [`super::SEND_EMAIL_NOTIFICATION_TASK`]: parse the
/// Celery-body call, run, settle. The Python body never raises out
/// (the outer `except` swallows everything), so a completed run —
/// including every silent/logged early return — acknowledges. Only a
/// call-boundary `TypeError` (unparseable args/kwargs) fails the task.
pub fn send_handler<
    R: RedisMail + 'static,
    D: MailDb + 'static,
    T: Renderer + 'static,
    M: Mailer + 'static,
>(
    redis: Arc<R>,
    db: Arc<D>,
    renderer: Arc<T>,
    mailer: Arc<M>,
) -> Handler {
    Arc::new(move |job: crate::queue::JobRow| {
        let redis = redis.clone();
        let db = db.clone();
        let renderer = renderer.clone();
        let mailer = mailer.clone();
        Box::pin(async move {
            let call = match parse_send_call(&job.args, &job.kwargs) {
                Ok(call) => call,
                Err(error) => {
                    return Ok(Verdict::Fail {
                        error: error.to_string(),
                    });
                }
            };
            let outcome = run_send(
                redis.as_ref(),
                db.as_ref(),
                renderer.as_ref(),
                mailer.as_ref(),
                call,
            )
            .await;
            match &outcome {
                SendOutcome::Sent => {
                    tracing::info!(task = super::SEND_EMAIL_NOTIFICATION_TASK, "{SENT_LOG}")
                }
                SendOutcome::DuplicateSkipped => {
                    tracing::info!(
                        task = super::SEND_EMAIL_NOTIFICATION_TASK,
                        "{DUPLICATE_LOG}"
                    )
                }
                SendOutcome::ReleasedWithLog(error) => {
                    tracing::error!(task = super::SEND_EMAIL_NOTIFICATION_TASK, "{error}")
                }
                SendOutcome::BaseApiMissing | SendOutcome::ReleasedSilent => {}
            }
            Ok(Verdict::Ack)
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
    })
}

/// Register both email-notification handlers
/// ([`super::STACK_EMAIL_NOTIFICATION_TASK`],
/// [`super::SEND_EMAIL_NOTIFICATION_TASK`]).
///
/// Building the table only (the `register_deletion_tasks` precedent):
/// the worker binary calls this when the domain gate flips ownership
/// (PIDASHCONV-218, after the PIDASHCONV-21 proxy pass). Until then
/// both names stay `PythonOwned` (see `super::assert_python_owned`).
pub fn register_email_notification_tasks(registry: &mut Registry, stack: Handler, send: Handler) {
    registry.register(super::STACK_EMAIL_NOTIFICATION_TASK, stack);
    registry.register(super::SEND_EMAIL_NOTIFICATION_TASK, send);
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

#[cfg(test)]
mod part2_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn lookup_of(
        map: &HashMap<String, String>,
    ) -> impl Fn(&str) -> Result<String, MentionLookupError> + '_ {
        move |id| {
            map.get(id)
                .cloned()
                .ok_or_else(|| MentionLookupError::NotFound(id.to_owned()))
        }
    }

    fn names(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn mention_rewrites_single_component() {
        let map = names(&[("u1", "Ada L")]);
        let html = "<p>hi <mention-component entity_identifier=\"u1\">Ada</mention-component></p>";
        assert_eq!(
            process_mention(html, &lookup_of(&map)).expect("rewrite"),
            "<p>hi @Ada L</p>"
        );
    }

    #[test]
    fn mention_rewrites_each_occurrence_with_fresh_lookup() {
        // N-query shape: the SELECT runs per element, repeats included.
        let map = names(&[("u1", "Ada"), ("u2", "Bo")]);
        let calls = AtomicUsize::new(0);
        let lookup = |id: &str| {
            calls.fetch_add(1, Ordering::SeqCst);
            map.get(id)
                .cloned()
                .ok_or_else(|| MentionLookupError::NotFound(id.to_owned()))
        };
        let html = "<mention-component entity_identifier=\"u1\">a</mention-component> and \
            <mention-component entity_identifier='u2'>b</mention-component> and \
            <mention-component entity_identifier=\"u1\">a</mention-component>";
        assert_eq!(
            process_mention(html, &lookup).expect("rewrite"),
            "@Ada and @Bo and @Ada"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn mention_passthrough_without_components() {
        let map = names(&[]);
        let html = "<p class=\"x\">looks <b>good</b> &amp; done</p>";
        assert_eq!(
            process_mention(html, &lookup_of(&map)).expect("passthrough"),
            html
        );
        assert_eq!(process_mention("", &lookup_of(&map)).expect("empty"), "");
    }

    #[test]
    fn mention_missing_attribute_errors_before_lookup() {
        let map = names(&[("u1", "Ada")]);
        let calls = AtomicUsize::new(0);
        let lookup = |id: &str| {
            calls.fetch_add(1, Ordering::SeqCst);
            map.get(id)
                .cloned()
                .ok_or_else(|| MentionLookupError::NotFound(id.to_owned()))
        };
        assert_eq!(
            process_mention("<mention-component>oops</mention-component>", &lookup),
            Err(MentionError::MissingEntityIdentifier)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mention_unknown_user_propagates() {
        let map = names(&[]);
        assert_eq!(
            process_mention(
                "<mention-component entity_identifier=\"ghost\">x</mention-component>",
                &lookup_of(&map)
            ),
            Err(MentionError::UserNotFound("ghost".to_owned()))
        );
    }

    #[test]
    fn mention_escapes_name_like_bs_serialization() {
        let map = names(&[("u1", "A&B<C>")]);
        assert_eq!(
            process_mention(
                "<mention-component entity_identifier=\"u1\">x</mention-component>",
                &lookup_of(&map)
            )
            .expect("rewrite"),
            "@A&amp;B&lt;C&gt;"
        );
    }

    #[test]
    fn mention_matches_case_insensitive_and_self_closing() {
        let map = names(&[("u1", "Ada")]);
        assert_eq!(
            process_mention(
                "<MENTION-COMPONENT ENTITY_IDENTIFIER=\"u1\">x</MENTION-COMPONENT>",
                &lookup_of(&map)
            )
            .expect("upper"),
            "@Ada"
        );
        assert_eq!(
            process_mention(
                "<mention-component entity_identifier=\"u1\"/>",
                &lookup_of(&map)
            )
            .expect("self-closing"),
            "@Ada"
        );
        assert_eq!(
            process_mention(
                "<mention-component entity_identifier=u1>x</mention-component>",
                &lookup_of(&map)
            )
            .expect("bare attr"),
            "@Ada"
        );
    }

    #[test]
    fn mention_inside_comments_is_not_an_element() {
        let map = names(&[("u1", "Ada")]);
        let html =
            "<!-- <mention-component entity_identifier=\"u1\">x</mention-component> --><p>ok</p>";
        assert_eq!(
            process_mention(html, &lookup_of(&map)).expect("comments skipped"),
            html
        );
    }

    #[test]
    fn html_content_none_maps_to_none() {
        let map = names(&[]);
        assert_eq!(
            process_html_content(None, &lookup_of(&map)).expect("none"),
            None
        );
        assert_eq!(
            process_html_content(Some(&Value::Null), &lookup_of(&map)).expect("null"),
            None
        );
    }

    #[test]
    fn html_content_maps_each_entry() {
        let map = names(&[("u1", "Ada")]);
        let content = serde_json::json!([
            "<p>a</p>",
            "<mention-component entity_identifier=\"u1\">x</mention-component>"
        ]);
        assert_eq!(
            process_html_content(Some(&content), &lookup_of(&map)).expect("mapped"),
            Some(vec!["<p>a</p>".to_owned(), "@Ada".to_owned()])
        );
    }

    #[test]
    fn html_content_rejects_bad_shapes() {
        let map = names(&[]);
        assert_eq!(
            process_html_content(Some(&serde_json::json!({"a": 1})), &lookup_of(&map)),
            Err(MentionError::BadShape)
        );
        assert_eq!(
            process_html_content(Some(&serde_json::json!([42])), &lookup_of(&map)),
            Err(MentionError::BadShape)
        );
    }

    fn stack_row(id: &str, receiver: &str, actor: &str, issue: Option<&str>) -> StackRow {
        StackRow {
            id: id.to_owned(),
            receiver_id: receiver.to_owned(),
            triggered_by_id: actor.to_owned(),
            entity_identifier: issue.map(str::to_owned),
            data: serde_json::json!({"n": id}),
        }
    }

    #[test]
    fn stack_groups_per_receiver_and_issue_with_port_bug() {
        // Receiver A: two issues (i1 x2 rows, i2 x1); receiver B: one issue.
        let rows = vec![
            stack_row("l1", "recv-a", "act-1", Some("iss-1")),
            stack_row("l2", "recv-a", "act-2", Some("iss-1")),
            stack_row("l3", "recv-a", "act-1", Some("iss-2")),
            stack_row("l4", "recv-b", "act-9", Some("iss-9")),
        ];
        let (batches, processed) = plan_stack(&rows);
        assert_eq!(processed, vec!["l1", "l2", "l3", "l4"]);
        assert_eq!(batches.len(), 2);
        let a = &batches[0];
        assert_eq!(a.receiver_id, "recv-a");
        assert_eq!(a.batches.len(), 2);
        assert_eq!(a.batches[0].issue_id.as_deref(), Some("iss-1"));
        assert_eq!(
            a.batches[0].notification_data["act-1"],
            serde_json::json!([{"n": "l1"}])
        );
        assert_eq!(a.batches[1].issue_id.as_deref(), Some("iss-2"));
        // PORT BUG: every issue's send carries receiver A's ids across
        // BOTH issues — not the single issue's ids.
        assert_eq!(a.email_notification_ids, vec!["l1", "l2", "l3"]);
        let b = &batches[1];
        assert_eq!(b.receiver_id, "recv-b");
        assert_eq!(b.email_notification_ids, vec!["l4"]);
    }

    #[test]
    fn stack_null_entity_groups_none_key() {
        let rows = vec![stack_row("l1", "recv-a", "act-1", None)];
        let (batches, processed) = plan_stack(&rows);
        assert_eq!(processed, vec!["l1"]);
        assert_eq!(batches[0].batches[0].issue_id, None);
    }

    #[test]
    fn stack_delay_message_kwargs_match_wire_order() {
        // `F-WIRE-MAIL` send entry `cpython_kwargsrepr` order:
        // issue_id, notification_data, receiver_id, email_notification_ids.
        let mut data = Map::new();
        data.insert("act-1".to_owned(), Value::Array(vec![]));
        let message = stack_delay_message(
            Some("11111111-1111-1111-1111-111111111111"),
            data,
            "22222222-2222-2222-2222-222222222222",
            &["l2".to_owned(), "l1".to_owned()],
        );
        assert_eq!(
            message.task,
            "pi_dash.bgtasks.email_notification_task.send_email_notification"
        );
        assert!(message.args.is_empty());
        let keys: Vec<&str> = message.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "issue_id",
                "notification_data",
                "receiver_id",
                "email_notification_ids"
            ]
        );
        assert_eq!(
            message.kwargs["issue_id"],
            serde_json::json!("11111111-1111-1111-1111-111111111111")
        );
        // Null entity_identifier rides as JSON null (Python `None`).
        let null_message = stack_delay_message(None, Map::new(), "r", &[]);
        assert_eq!(null_message.kwargs["issue_id"], Value::Null);
    }

    struct FakeStackStore {
        rows: Vec<StackRow>,
        marked: Mutex<Vec<Vec<String>>>,
    }

    impl StackStore for FakeStackStore {
        async fn unprocessed_logs(&self) -> Result<Vec<StackRow>, StackError> {
            Ok(self.rows.clone())
        }

        async fn mark_processed(&self, processed: &[String]) -> Result<(), StackError> {
            self.marked.lock().expect("lock").push(processed.to_vec());
            Ok(())
        }
    }

    struct FakeStackOutbox {
        sent: Mutex<Vec<CeleryTaskMessage>>,
    }

    impl StackOutbox for FakeStackOutbox {
        async fn delay_send(&self, message: CeleryTaskMessage) -> Result<(), StackError> {
            self.sent.lock().expect("lock").push(message);
            Ok(())
        }
    }

    #[tokio::test]
    async fn stack_run_fans_out_and_marks_processed() {
        let store = FakeStackStore {
            rows: vec![
                stack_row("l1", "recv-a", "act-1", Some("iss-1")),
                stack_row("l2", "recv-a", "act-1", Some("iss-2")),
            ],
            marked: Mutex::new(Vec::new()),
        };
        let outbox = FakeStackOutbox {
            sent: Mutex::new(Vec::new()),
        };
        let report = run_stack(&store, &outbox).await.expect("run");
        assert_eq!(
            report,
            StackReport {
                receivers: 1,
                fanned: 2,
                processed: 2
            }
        );
        let sent = outbox.sent.lock().expect("lock");
        assert_eq!(sent.len(), 2);
        // Both sends carry the receiver-wide id list (the PORT BUG).
        for message in sent.iter() {
            assert_eq!(
                message.kwargs["email_notification_ids"],
                serde_json::json!(["l1", "l2"])
            );
        }
        assert_eq!(sent[0].kwargs["issue_id"], serde_json::json!("iss-1"));
        assert_eq!(sent[1].kwargs["issue_id"], serde_json::json!("iss-2"));
        // The `:84` update runs with every row id.
        assert_eq!(
            *store.marked.lock().expect("lock"),
            vec![vec!["l1".to_owned(), "l2".to_owned()]]
        );
    }

    #[tokio::test]
    async fn stack_run_empty_still_marks_processed() {
        // `:84` is unconditional — a no-op update, still issued.
        let store = FakeStackStore {
            rows: Vec::new(),
            marked: Mutex::new(Vec::new()),
        };
        let outbox = FakeStackOutbox {
            sent: Mutex::new(Vec::new()),
        };
        let report = run_stack(&store, &outbox).await.expect("run");
        assert_eq!(report.fanned, 0);
        assert!(outbox.sent.lock().expect("lock").is_empty());
        assert_eq!(
            *store.marked.lock().expect("lock"),
            vec![Vec::<String>::new()]
        );
    }

    #[test]
    fn send_lock_id_sorts_and_joins() {
        assert_eq!(
            send_lock_id(
                "iss",
                "recv",
                &["b".to_owned(), "a".to_owned(), "c".to_owned()]
            ),
            "send_email_notif_iss_recv_a_b_c"
        );
    }

    #[test]
    fn activity_time_reformats_naive() {
        assert_eq!(
            format_activity_time("2026-09-01 10:00:00").expect("morning"),
            "10:00 AM"
        );
        assert_eq!(
            format_activity_time("2026-09-01 00:05:00").expect("midnight"),
            "00:05 AM"
        );
        // `%H` stays 24-hour next to AM/PM — the Python oddity, ported.
        assert_eq!(
            format_activity_time("2026-09-01 13:00:00").expect("afternoon"),
            "13:00 PM"
        );
        assert!(format_activity_time("not-a-time").is_err());
        assert!(format_activity_time("2026-09-01T10:00:00").is_err());
    }

    #[test]
    fn subject_strips_control_characters_only() {
        assert_eq!(build_subject("PROJ", 12, "Fix login"), "PROJ-12 Fix login");
        assert_eq!(build_subject("PROJ", 3, "a\x07b\x1bc"), "PROJ-3 abc");
        // No `.strip()`: spaces survive exactly as Django sends them.
        assert_eq!(build_subject("PROJ", 3, "  padded  "), "PROJ-3   padded  ");
    }

    #[test]
    fn avatar_src_renders_none_like_fstring() {
        assert_eq!(avatar_src("http://b", Some("/a.png")), "http://b/a.png");
        assert_eq!(avatar_src("http://b", None), "http://bNone");
    }

    #[test]
    fn truthiness_matches_python() {
        assert!(!py_truthy(&Value::Null));
        assert!(!py_truthy(&serde_json::json!(false)));
        assert!(py_truthy(&serde_json::json!(true)));
        assert!(!py_truthy(&serde_json::json!("")));
        assert!(py_truthy(&serde_json::json!("x")));
        assert!(!py_truthy(&serde_json::json!([])));
        assert!(!py_truthy(&serde_json::json!({})));
        assert!(!py_truthy(&serde_json::json!(0)));
        assert!(py_truthy(&serde_json::json!(2)));
    }

    #[test]
    fn plain_text_drops_style_and_tags() {
        let html = "<html><head><STYLE>p { color: red; }</STYLE></head><body><p>Hello <b>World</b></p><!-- c --><p>Bye</p></body></html>";
        assert_eq!(plain_text_from_html(html), "\n\nHello WorldBye\n\n");
    }

    #[test]
    fn plain_text_preserves_entities_like_strip_tags() {
        // Django parses with `convert_charrefs=False`: entities survive
        // verbatim (proven by probe against Django 4.2.30).
        assert_eq!(
            plain_text_from_html("<p>A &amp; B &#33;</p>"),
            "\n\nA &amp; B &#33;\n\n"
        );
        assert_eq!(plain_text_from_html("<p>&foo;</p>"), "\n\n&foo;\n\n");
        // A `<` that opens no tag stays literal (the HTMLParser rule).
        assert_eq!(plain_text_from_html("<p>a < b</p>"), "\n\na < b\n\n");
    }

    #[test]
    fn smtp_mapping_matches_common_fixture() {
        // `F-COMMON.email_configuration`: `int(EMAIL_PORT)`,
        // `== "1"` string compares.
        assert_eq!(smtp_port(&serde_json::json!(587)), Some(587));
        assert_eq!(smtp_port(&serde_json::json!("587")), Some(587));
        assert_eq!(smtp_port(&serde_json::json!(" 587 ")), Some(587));
        assert_eq!(smtp_port(&Value::Null), None);
        assert_eq!(smtp_port(&serde_json::json!(true)), None);
        assert!(smtp_flag(&serde_json::json!("1")));
        assert!(!smtp_flag(&serde_json::json!("0")));
        assert!(!smtp_flag(&serde_json::json!(1)));
        assert!(!smtp_flag(&serde_json::json!(true)));
    }

    struct MapMentions {
        names: HashMap<String, String>,
        calls: AtomicUsize,
    }

    impl MentionSource for MapMentions {
        async fn display_name(&self, user_id: &str) -> Result<String, MentionLookupError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.names
                .get(user_id)
                .cloned()
                .ok_or_else(|| MentionLookupError::NotFound(user_id.to_owned()))
        }
    }

    fn actor() -> ActorSnapshot {
        ActorSnapshot {
            first_name: "Ada".to_owned(),
            last_name: "L".to_owned(),
            avatar_url: Some("/a.png".to_owned()),
        }
    }

    fn issue() -> IssueSnapshot {
        IssueSnapshot {
            id: "iss-1".to_owned(),
            name: "Fix login".to_owned(),
            identifier: "PROJ".to_owned(),
            sequence_id: 12,
            project_id: "proj-1".to_owned(),
            project_name: "Rocket".to_owned(),
            workspace_slug: "acme".to_owned(),
        }
    }

    #[tokio::test]
    async fn plan_splits_comments_mentions_and_rows() {
        let mut data = Map::new();
        data.insert(
            "act-1".to_owned(),
            serde_json::json!({
                "comment": {"old_value": [], "new_value": ["looks good"]},
                "state": {"old_value": ["Todo"], "new_value": ["Done"]},
                "activity_time": "2026-09-01 10:00:00"
            }),
        );
        data.insert(
            "act-2".to_owned(),
            serde_json::json!({
                "mention": {
                    "old_value": ["<p>ping <mention-component entity_identifier=\"u9\">X</mention-component></p>"],
                    "new_value": []
                },
                "activity_time": "2026-09-01 11:00:00"
            }),
        );
        let mut actors = HashMap::new();
        actors.insert("act-1".to_owned(), actor());
        actors.insert("act-2".to_owned(), actor());
        let mentions = MapMentions {
            names: names(&[("u9", "Zed")]),
            calls: AtomicUsize::new(0),
        };
        let plan = plan_send_actors(data, &actors, &issue(), "http://localhost", &mentions)
            .await
            .expect("plan");
        // Comment pop first, then the mention pop.
        assert_eq!(plan.comments.len(), 2);
        assert_eq!(
            plan.comments[0]["actor_comments"],
            serde_json::json!({"old_value": [], "new_value": ["looks good"]})
        );
        assert_eq!(
            plan.comments[0]["actor_detail"]["avatar_url"],
            serde_json::json!("http://localhost/a.png")
        );
        // Mention slots rewritten; one fresh lookup for the occurrence.
        assert_eq!(
            plan.comments[1]["actor_comments"]["old_value"],
            serde_json::json!(["<p>ping @Zed</p>"])
        );
        assert_eq!(mentions.calls.load(Ordering::SeqCst), 1);
        // Only act-1 has residual changes (act-2's mention+time popped clean).
        assert_eq!(plan.template_data.len(), 1);
        let row = &plan.template_data[0];
        assert_eq!(
            row["issue_details"]["identifier"],
            serde_json::json!("PROJ-12")
        );
        assert_eq!(row["activity_time"], serde_json::json!("10:00 AM"));
        assert_eq!(
            row["changes"],
            serde_json::json!({"state": {"old_value": ["Todo"], "new_value": ["Done"]}})
        );
        assert_eq!(plan.actors_involved, vec!["act-1", "act-2"]);
    }

    #[tokio::test]
    async fn plan_rejects_missing_activity_time() {
        let mut data = Map::new();
        data.insert("act-1".to_owned(), serde_json::json!({"state": {}}));
        let mut actors = HashMap::new();
        actors.insert("act-1".to_owned(), actor());
        let mentions = MapMentions {
            names: HashMap::new(),
            calls: AtomicUsize::new(0),
        };
        assert_eq!(
            plan_send_actors(data, &actors, &issue(), "http://b", &mentions).await,
            Err(SendError::MissingActivityTime("act-1".to_owned()))
        );
    }

    #[test]
    fn context_keys_match_send_fixture_order() {
        // `F-SEND.send_email_notification` `context_keys` order.
        let plan = SendPlan {
            actors_involved: vec!["a".to_owned(), "a".to_owned(), "b".to_owned()],
            ..SendPlan::default()
        };
        let context = build_context(&plan, &issue(), Some("r@example.com"), "http://localhost");
        let keys: Vec<&str> = context.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "data",
                "summary",
                "actors_involved",
                "issue",
                "receiver",
                "issue_url",
                "project_url",
                "workspace",
                "project",
                "user_preference",
                "comments",
                "entity_type"
            ]
        );
        assert_eq!(context["actors_involved"], serde_json::json!(2));
        assert_eq!(
            context["issue_url"],
            serde_json::json!("http://localhost/acme/projects/proj-1/issues/iss-1")
        );
        assert_eq!(
            context["project_url"],
            serde_json::json!("http://localhost/acme/projects/proj-1/issues/")
        );
        assert_eq!(
            context["user_preference"],
            serde_json::json!("http://localhost/acme/settings/account/notifications/")
        );
        assert_eq!(context["entity_type"], serde_json::json!("issue"));
    }

    #[test]
    fn parse_send_call_accepts_wire_shape() {
        let args = Value::Array(vec![]);
        let kwargs = serde_json::json!({
            "issue_id": "11111111-1111-1111-1111-111111111111",
            "notification_data": {},
            "receiver_id": "22222222-2222-2222-2222-222222222222",
            "email_notification_ids": []
        });
        let call = parse_send_call(&args, &kwargs).expect("parse");
        assert_eq!(
            call.issue_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        // Null entity row parses to `None` (the `DoesNotExist` path).
        let null_kwargs = serde_json::json!({
            "issue_id": null,
            "notification_data": {},
            "receiver_id": "r",
            "email_notification_ids": []
        });
        assert_eq!(
            parse_send_call(&args, &null_kwargs)
                .expect("null issue")
                .issue_id,
            None
        );
        assert_eq!(
            parse_send_call(&serde_json::json!(["x"]), &kwargs),
            Err(CallError::UnexpectedArgs)
        );
        assert_eq!(
            parse_send_call(&args, &serde_json::json!({})),
            Err(CallError::MissingKwarg("issue_id"))
        );
    }

    struct FakeMailRedis {
        sets: Mutex<Vec<(String, String, u64)>>,
        dels: Mutex<Vec<String>>,
        acquire: LockSet,
        values: HashMap<String, String>,
    }

    impl RedisLock for FakeMailRedis {
        fn set_nx_ex(&self, key: &str, value: &str, ex_secs: u64) -> LockSet {
            self.sets
                .lock()
                .expect("lock")
                .push((key.to_owned(), value.to_owned(), ex_secs));
            self.acquire
        }

        fn del(&self, key: &str) {
            self.dels.lock().expect("lock").push(key.to_owned());
        }
    }

    impl RedisMail for FakeMailRedis {
        fn get(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
    }

    struct FakeMailDb {
        receivers: HashMap<String, Option<String>>,
        actors: HashMap<String, ActorSnapshot>,
        issues: HashMap<String, IssueSnapshot>,
        missing_issue: bool,
        missing_receiver: bool,
        missing_actor: Option<String>,
        config: EmailConfig,
        mark_sent: Mutex<Vec<Vec<String>>>,
        actor_calls: Mutex<Vec<String>>,
        mentions: MapMentions,
    }

    impl MentionSource for FakeMailDb {
        async fn display_name(&self, user_id: &str) -> Result<String, MentionLookupError> {
            self.mentions.display_name(user_id).await
        }
    }

    impl MailDb for FakeMailDb {
        async fn receiver_email(&self, receiver_id: &str) -> Result<Option<String>, MailDbError> {
            if self.missing_receiver {
                return Err(MailDbError::NotFound(receiver_id.to_owned()));
            }
            Ok(self.receivers.get(receiver_id).cloned().flatten())
        }

        async fn actor(&self, actor_id: &str) -> Result<ActorSnapshot, MailDbError> {
            self.actor_calls
                .lock()
                .expect("lock")
                .push(actor_id.to_owned());
            if self.missing_actor.as_deref() == Some(actor_id) {
                return Err(MailDbError::NotFound(actor_id.to_owned()));
            }
            self.actors
                .get(actor_id)
                .cloned()
                .ok_or_else(|| MailDbError::NotFound(actor_id.to_owned()))
        }

        async fn issue(&self, issue_id: Option<&str>) -> Result<IssueSnapshot, MailDbError> {
            match issue_id {
                None => Err(MailDbError::NotFound("None".to_owned())),
                Some(id) if self.missing_issue => Err(MailDbError::NotFound(id.to_owned())),
                Some(id) => self
                    .issues
                    .get(id)
                    .cloned()
                    .ok_or_else(|| MailDbError::NotFound(id.to_owned())),
            }
        }

        async fn email_config(&self) -> Result<EmailConfig, MailDbError> {
            Ok(self.config.clone())
        }

        async fn mark_sent(&self, ids: &[String]) -> Result<(), MailDbError> {
            self.mark_sent.lock().expect("lock").push(ids.to_vec());
            Ok(())
        }
    }

    struct FakeRenderer {
        html: String,
        contexts: Mutex<Vec<Map<String, Value>>>,
    }

    impl Renderer for FakeRenderer {
        fn render_issue_updates(
            &self,
            context: &Map<String, Value>,
        ) -> Result<String, RenderError> {
            self.contexts.lock().expect("lock").push(context.clone());
            Ok(self.html.clone())
        }
    }

    struct FakeMailer {
        envelopes: Mutex<Vec<SmtpEnvelope>>,
        fail: bool,
    }

    impl Mailer for FakeMailer {
        fn send(&self, envelope: &SmtpEnvelope) -> Result<(), MailError> {
            self.envelopes.lock().expect("lock").push(envelope.clone());
            if self.fail || envelope.to.is_none() {
                return Err(MailError::Failed("smtp down".to_owned()));
            }
            Ok(())
        }
    }

    fn send_fixtures() -> (FakeMailRedis, FakeMailDb, FakeRenderer, FakeMailer) {
        let mut actors = HashMap::new();
        actors.insert("act-1".to_owned(), actor());
        let mut issues = HashMap::new();
        issues.insert("iss-1".to_owned(), issue());
        let mut receivers = HashMap::new();
        receivers.insert("recv-1".to_owned(), Some("recv@example.com".to_owned()));
        (
            FakeMailRedis {
                sets: Mutex::new(Vec::new()),
                dels: Mutex::new(Vec::new()),
                acquire: LockSet::Acquired,
                values: HashMap::from([("iss-1".to_owned(), "http://localhost".to_owned())]),
            },
            FakeMailDb {
                receivers,
                actors,
                issues,
                missing_issue: false,
                missing_receiver: false,
                missing_actor: None,
                config: EmailConfig {
                    host: Some("smtp.example.com".to_owned()),
                    username: Some("user".to_owned()),
                    password: Some("pass".to_owned()),
                    port: serde_json::json!(587),
                    use_tls: serde_json::json!("1"),
                    use_ssl: serde_json::json!("0"),
                    from_email: "Team Pi Dash <team@airepublic.com>".to_owned(),
                },
                mark_sent: Mutex::new(Vec::new()),
                actor_calls: Mutex::new(Vec::new()),
                mentions: MapMentions {
                    names: HashMap::new(),
                    calls: AtomicUsize::new(0),
                },
            },
            FakeRenderer {
                html: "<p>updates</p>".to_owned(),
                contexts: Mutex::new(Vec::new()),
            },
            FakeMailer {
                envelopes: Mutex::new(Vec::new()),
                fail: false,
            },
        )
    }

    fn send_call() -> SendCall {
        let mut notification_data = Map::new();
        notification_data.insert(
            "act-1".to_owned(),
            serde_json::json!([{"issue_activity": {
                "field": "state",
                "old_value": "Todo",
                "new_value": "Done",
                "activity_time": "2026-09-01T10:00:00Z"
            }}]),
        );
        SendCall {
            issue_id: Some("iss-1".to_owned()),
            notification_data,
            receiver_id: "recv-1".to_owned(),
            // Receiver-wide ids (the PORT BUG): two issues' logs.
            email_notification_ids: vec!["l2".to_owned(), "l1".to_owned()],
        }
    }

    #[tokio::test]
    async fn send_success_marks_receiver_wide_ids() {
        let (redis, db, renderer, mailer) = send_fixtures();
        let outcome = run_send(&redis, &db, &renderer, &mailer, send_call()).await;
        assert_eq!(outcome, SendOutcome::Sent);
        // SMTP envelope mirrors `:266-284`.
        let envelopes = mailer.envelopes.lock().expect("lock");
        assert_eq!(envelopes.len(), 1);
        let envelope = &envelopes[0];
        assert_eq!(envelope.subject, "PROJ-12 Fix login");
        assert_eq!(envelope.to.as_deref(), Some("recv@example.com"));
        assert_eq!(envelope.from_email, "Team Pi Dash <team@airepublic.com>");
        assert_eq!(envelope.port, 587);
        assert!(envelope.use_tls);
        assert!(!envelope.use_ssl);
        assert_eq!(envelope.text_content, "\n\nupdates\n\n");
        // `sent_at` covers the receiver-wide list (the PORT BUG) —
        // and the template saw the full context.
        assert_eq!(
            *db.mark_sent.lock().expect("lock"),
            vec![vec!["l2".to_owned(), "l1".to_owned()]]
        );
        assert_eq!(renderer.contexts.lock().expect("lock").len(), 1);
        // Lock acquired with the part-1 shape, released on success.
        let lock_id = "send_email_notif_iss-1_recv-1_l1_l2";
        assert_eq!(
            *redis.sets.lock().expect("lock"),
            vec![(lock_id.to_owned(), "true".to_owned(), 300)]
        );
        assert_eq!(*redis.dels.lock().expect("lock"), vec![lock_id.to_owned()]);
    }

    #[tokio::test]
    async fn send_contended_lock_skips_without_release() {
        let (mut redis, db, renderer, mailer) = send_fixtures();
        redis.acquire = LockSet::Contended;
        let outcome = run_send(&redis, &db, &renderer, &mailer, send_call()).await;
        assert_eq!(outcome, SendOutcome::DuplicateSkipped);
        assert!(redis.dels.lock().expect("lock").is_empty());
        assert!(mailer.envelopes.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn send_missing_base_api_returns_holding_lock() {
        // PORT NOTE lock leak (`:167-168`): silent return, NO release.
        let (redis, db, renderer, mailer) = send_fixtures();
        let mut call = send_call();
        call.issue_id = Some("unknown-issue".to_owned());
        let outcome = run_send(&redis, &db, &renderer, &mailer, call).await;
        assert_eq!(outcome, SendOutcome::BaseApiMissing);
        assert!(redis.dels.lock().expect("lock").is_empty());
        assert!(!redis.sets.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn send_missing_receiver_releases_silently() {
        let (redis, mut db, renderer, mailer) = send_fixtures();
        db.missing_receiver = true;
        let outcome = run_send(&redis, &db, &renderer, &mailer, send_call()).await;
        assert_eq!(outcome, SendOutcome::ReleasedSilent);
        assert_eq!(redis.dels.lock().expect("lock").len(), 1);
        assert!(mailer.envelopes.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn send_bad_payload_logs_and_releases() {
        // Both values empty → `create_payload` has no actor entry →
        // `MissingActor` → outer except → logged release.
        let (redis, db, renderer, mailer) = send_fixtures();
        let mut notification_data = Map::new();
        notification_data.insert(
            "act-1".to_owned(),
            serde_json::json!([{"issue_activity": {
                "field": "state",
                "old_value": "",
                "new_value": "",
                "activity_time": "2026-09-01T10:00:00Z"
            }}]),
        );
        let mut call = send_call();
        call.notification_data = notification_data;
        let outcome = run_send(&redis, &db, &renderer, &mailer, call).await;
        assert!(matches!(outcome, SendOutcome::ReleasedWithLog(_)));
        assert_eq!(redis.dels.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn send_missing_mention_user_releases_silently() {
        // Per-mention `User.DoesNotExist` (`:136`) → outer
        // `DoesNotExist` branch → silent release.
        let (redis, db, renderer, mailer) = send_fixtures();
        let mut notification_data = Map::new();
        notification_data.insert(
            "act-1".to_owned(),
            serde_json::json!([{"issue_activity": {
                "field": "mention",
                "old_value": "<mention-component entity_identifier=\"ghost\">x</mention-component>",
                "new_value": "",
                "activity_time": "2026-09-01T10:00:00Z"
            }}]),
        );
        let mut call = send_call();
        call.notification_data = notification_data;
        let outcome = run_send(&redis, &db, &renderer, &mailer, call).await;
        assert_eq!(outcome, SendOutcome::ReleasedSilent);
        assert_eq!(redis.dels.lock().expect("lock").len(), 1);
        assert!(mailer.envelopes.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn send_smtp_failure_logs_and_releases() {
        let (redis, db, renderer, _) = send_fixtures();
        let mailer = FakeMailer {
            envelopes: Mutex::new(Vec::new()),
            fail: true,
        };
        let outcome = run_send(&redis, &db, &renderer, &mailer, send_call()).await;
        assert!(matches!(outcome, SendOutcome::ReleasedWithLog(_)));
        assert_eq!(redis.dels.lock().expect("lock").len(), 1);
        // `sent_at` never advances when the send fails.
        assert!(db.mark_sent.lock().expect("lock").is_empty());
    }

    #[test]
    fn send_wire_message_matches_stack_kwargs() {
        let call = send_call();
        let message = send_delay_message(&call);
        assert_eq!(
            message.task,
            "pi_dash.bgtasks.email_notification_task.send_email_notification"
        );
        let keys: Vec<&str> = message.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "issue_id",
                "notification_data",
                "receiver_id",
                "email_notification_ids"
            ]
        );
    }

    #[test]
    fn template_name_matches_python_render_path() {
        // `render_to_string("emails/notifications/issue-updates.html", …)`
        // (`:263`; `F-SEND.send_email_notification` "template_rows").
        assert_eq!(
            ISSUE_UPDATES_TEMPLATE,
            "emails/notifications/issue-updates.html"
        );
    }

    fn job_row(args: Value, kwargs: Value) -> crate::queue::JobRow {
        let now = chrono::Utc::now();
        crate::queue::JobRow {
            id: 1,
            celery_id: "cid".to_owned(),
            task: super::super::SEND_EMAIL_NOTIFICATION_TASK.to_owned(),
            args,
            kwargs,
            queue: "celery".to_owned(),
            status: "running".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: now,
            claimed_at: None,
            claimed_by: None,
            created_at: now,
            last_error: None,
        }
    }

    fn send_job_kwargs() -> (Value, Value) {
        let message = send_delay_message(&send_call());
        (Value::Array(message.args), Value::Object(message.kwargs))
    }

    #[tokio::test]
    async fn send_handler_acks_completed_send() {
        // The Python body never raises out (outer `except` swallows
        // everything), so a completed run acknowledges.
        let (redis, db, renderer, mailer) = send_fixtures();
        let handler = send_handler(
            Arc::new(redis),
            Arc::new(db),
            Arc::new(renderer),
            Arc::new(mailer),
        );
        let (args, kwargs) = send_job_kwargs();
        let verdict = handler(job_row(args, kwargs))
            .await
            .expect("handler never raises");
        assert_eq!(verdict, Verdict::Ack);
    }

    #[tokio::test]
    async fn send_handler_fails_unparseable_call() {
        // A call-boundary `TypeError` is the only handler-level failure
        // (the Celery-failure equivalent).
        let (redis, db, renderer, mailer) = send_fixtures();
        let handler = send_handler(
            Arc::new(redis),
            Arc::new(db),
            Arc::new(renderer),
            Arc::new(mailer),
        );
        let verdict = handler(job_row(
            serde_json::json!(["unexpected-positional"]),
            serde_json::json!({}),
        ))
        .await
        .expect("handler returns a verdict");
        assert!(matches!(verdict, Verdict::Fail { .. }));
    }
}
