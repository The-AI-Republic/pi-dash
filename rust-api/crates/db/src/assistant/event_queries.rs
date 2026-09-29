//! Event/message persistence shapes + history queries (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/runtime/events.py:1-145` (channel,
//! serialize, seq allocation, append, create, envelope, prune, publish) and
//! `apps/api/pi_dash/assistant/runtime/history.py:27-60` (history caps,
//! query, blob decode, dump). Table/column names and the `kind` bound come
//! from the layer below ([`super::models`], PIDASHCONV-247); fixture id
//! F-A6-06 (`rust-api/fixtures/assistant/queries.json`).
//!
//! Shape of the port: pure SQL builders plus the JSON wire shapes. The SQL
//! strings interpolate the caller's id (Django compiler-form, exactly what
//! the fixture records); production executors must bind parameters instead —
//! the `*_sql` functions pin the statement shape, they are not the execution
//! path. Row locking renders `FOR UPDATE SKIP LOCKED` where Python takes a
//! `SELECT ... FOR UPDATE` row lock (`events.py:92-93,116`): under contention
//! a concurrent writer skips the locked thread row instead of blocking, per
//! the Postgres-queue convention of this backend. Executing these statements
//! (sqlx transactions, post-commit publish, Redis PUBLISH) belongs to the
//! tasks/handlers layers that own transactions; those layers also own the
//! `orm_sql`/`updates` fixture entries from `tasks.py`/views, which are
//! recorded for them and deliberately not built here.

use serde_json::Value;

use super::models::{
    assistant_event, assistant_message, assistant_thread, assistant_turn, ThreadKind, TurnStatus,
};

/// Render a string exactly like stdlib `json.dumps` (`ensure_ascii=True`):
/// short escapes for `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`; `\u00xx`
/// (lowercase) for other controls and DEL; `\uXXXX` (lowercase, surrogate
/// pairs past the BMP) for everything non-ASCII. This is the escaping inside
/// both `publish_event` and the SSE replay frames, which share
/// `json.dumps(payload, default=str)` (`events.py:62`, `views/events.py:47`).
fn dumps_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{00}'..='\u{1f}' | '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            '\u{80}'..='\u{ffff}' => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            _ if ch as u32 > 0xffff => {
                let shifted = ch as u32 - 0x1_0000;
                out.push_str(&format!(
                    "\\u{:04x}\\u{:04x}",
                    0xd800 + (shifted >> 10),
                    0xdc00 + (shifted & 0x3ff)
                ));
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Render a string exactly like DRF's `JSONRenderer` over this project's
/// settings (`REST_FRAMEWORK` leaves the DRF defaults: `UNICODE_JSON=True`
/// so `ensure_ascii=False`, `COMPACT_JSON=True`): `serde_json`'s quoting
/// already matches (short escapes, raw UTF-8, lowercase `\u00xx`), and the
/// renderer post-pass escapes raw U+2028/U+2029
/// (`renderers.py:108-111`). Applied to the whole rendered body, payload
/// region included, like DRF's `ret.replace`.
fn drf_escape(rendered: &str) -> String {
    rendered
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Redis channel prefix (`events.py:34`).
pub const CHANNEL_PREFIX: &str = "assistant:thread:";
/// SSE replay turns only on this message status (`history.py:40`).
pub const HISTORY_STATUS: TurnStatus = TurnStatus::Completed;
/// Chat-thread history default (`history.py:37`, `ASSISTANT_HISTORY_MAX_TURNS`).
pub const CHAT_HISTORY_MAX_TURNS: i64 = 40;
/// Loop-thread history default (`history.py:35`,
/// `ASSISTANT_LOOP_HISTORY_MAX_TURNS`).
pub const LOOP_HISTORY_MAX_TURNS: i64 = 5;
/// Pruned delta kind (`events.py:145`).
pub const DELTA_KIND: &str = "assistant_delta";

/// SSE channel for a thread (`events.py:37-38`): `assistant:thread:<id>`.
pub fn event_channel(thread_id: &str) -> String {
    format!("{CHANNEL_PREFIX}{thread_id}")
}

/// Inputs to [`serialize_event_json`]. Ids and timestamps cross pre-rendered
/// (the `dtos.rs` convention): UUIDs as hyphenated strings, `created_at` as
/// the Python `datetime.isoformat()` rendering (`+00:00`, micros when set),
/// `payload` as already-serialized JSON (rendered by the owning layer with
/// the same `json.dumps` rules, or read back from the `JSONField` column).
pub struct EventParts<'a> {
    pub id: i64,
    pub thread_id: &'a str,
    pub message_id: Option<&'a str>,
    pub seq: i64,
    pub kind: &'a str,
    pub payload_json: &'a str,
    pub created_at: &'a str,
}

/// Event wire shape (`events.py:41-50`) rendered byte-for-byte like
/// `json.dumps(..., default=str)`: Python key order (`id, thread, message,
/// seq, kind, payload, created_at`) with `(', ', ': ')` separators. This is
/// what `publish_event` puts on the channel (`events.py:62`) and what SSE
/// subscribers receive verbatim (`views/events.py:82-85`); the replay
/// frames render through the same `json.dumps` call (`views/events.py:47`).
pub fn serialize_event_json(parts: &EventParts<'_>) -> String {
    let message = match parts.message_id {
        Some(id) => dumps_string(id),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\": {}, \"thread\": {}, \"message\": {}, \"seq\": {}, \"kind\": {}, \"payload\": {}, \"created_at\": {}}}",
        parts.id,
        dumps_string(parts.thread_id),
        message,
        parts.seq,
        dumps_string(parts.kind),
        parts.payload_json,
        dumps_string(parts.created_at)
    )
}

/// Truncate an event `kind` to the column bound (`events.py:98`,
/// `kind[:64]`). Python slices code points, so this cuts at a char boundary,
/// never mid-UTF-8 (the `[:255]` semantic trap). The bound is the layer-below
/// [`assistant_event::KIND_MAX_LENGTH`].
pub fn truncate_kind(kind: &str) -> &str {
    let bound = assistant_event::KIND_MAX_LENGTH;
    if kind.chars().count() <= bound {
        return kind;
    }
    let end = kind
        .char_indices()
        .nth(bound)
        .map(|(index, _)| index)
        .unwrap_or(kind.len());
    &kind[..end]
}

/// `MAX(seq)+1` over a thread's events, `0` when empty (`events.py:73-75`).
/// Allocated under the thread row lock in [`lock_thread_sql`].
pub fn next_event_seq_sql(thread_id: &str) -> String {
    let table = assistant_event::TABLE;
    format!(
        "SELECT \"{table}\".\"thread_id\", MAX(\"{table}\".\"seq\") AS \"m\" \
         FROM \"{table}\" WHERE \"{table}\".\"thread_id\" = {thread_id} \
         GROUP BY \"{table}\".\"thread_id\""
    )
}

/// `MAX(seq)+1` over a thread's messages, `0` when empty (`events.py:78-80`).
pub fn next_message_seq_sql(thread_id: &str) -> String {
    let table = assistant_message::TABLE;
    format!(
        "SELECT \"{table}\".\"thread_id\", MAX(\"{table}\".\"seq\") AS \"m\" \
         FROM \"{table}\" WHERE \"{table}\".\"thread_id\" = {thread_id} \
         GROUP BY \"{table}\".\"thread_id\""
    )
}

fn quoted_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Thread row lock for `append_event`/`create_message`
/// (`events.py:92-93,116`): full thread row, `FOR UPDATE SKIP LOCKED` where
/// Python blocks on `SELECT ... FOR UPDATE`. A concurrent writer that finds
/// the row locked skips it instead of queueing behind the holder.
pub fn lock_thread_sql(thread_id: &str) -> String {
    let table = assistant_thread::TABLE;
    let columns = quoted_columns(table, assistant_thread::COLUMNS);
    format!("SELECT {columns} FROM \"{table}\" WHERE \"{table}\".\"id\" = {thread_id} FOR UPDATE SKIP LOCKED")
}

/// `append_event` insert shape (`events.py:94-101`): every concrete event
/// column in model field order (the `BigAutoField` pk takes `DEFAULT`),
/// values bound positionally. `kind` must pass through [`truncate_kind`]
/// first; `payload` defaults to `{}` (`payload or {}`, `events.py:100`);
/// `seq` is [`next_event_seq_sql`] under [`lock_thread_sql`]; the row
/// publishes via [`publish_frame`] on transaction commit
/// (`transaction.on_commit`, `events.py:102`).
pub fn insert_event_sql() -> String {
    let table = assistant_event::TABLE;
    let columns = assistant_event::COLUMNS
        .iter()
        .filter(|column| **column != "id")
        .cloned()
        .collect::<Vec<_>>()
        .join("\", \"");
    let placeholders = (1..=assistant_event::COLUMNS.len() - 1)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"{table}\" (\"{columns}\") VALUES ({placeholders})")
}

/// `create_message` insert shape (`events.py:117-125`): same conventions as
/// [`insert_event_sql`]; `status` defaults to `completed`
/// (`MessageStatus.COMPLETED`, `events.py:113`).
pub fn insert_message_sql() -> String {
    let table = assistant_message::TABLE;
    let columns = assistant_message::COLUMNS
        .iter()
        .filter(|column| **column != "id")
        .cloned()
        .collect::<Vec<_>>()
        .join("\", \"");
    let placeholders = (1..=assistant_message::COLUMNS.len() - 1)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"{table}\" (\"{columns}\") VALUES ({placeholders})")
}

/// Inputs to [`message_envelope_json`]; same pre-rendered convention as
/// [`EventParts`], except `payload_json` arrives rendered with the *DRF*
/// rules (compact separators) rather than `json.dumps` (spaced): the whole
/// envelope renders through DRF `Response`, so a spaced payload would leak
/// foreign bytes into it. Payloads read back from the `JSONField` column
/// for envelopes must be compacted by the owning layer first.
pub struct MessageParts<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub display_content: &'a str,
    pub status: &'a str,
    pub seq: i64,
    pub turn_id: Option<&'a str>,
    pub payload_json: &'a str,
    pub created_at: &'a str,
    pub completed_at: Option<&'a str>,
}

/// Frontend chat-kit shape (`events.py:128-140`): `kind -> role`,
/// `display_content -> content`, exact Python key order (`id, role, content,
/// status, seq, turn_id, payload, created_at, completed_at`). The envelope
/// only ever renders through DRF `Response` (`views/messages.py:61,108`,
/// `tasks.py:125,151`), so this renders like DRF's `JSONRenderer` over this
/// project's settings: compact `(',', ':')` separators, raw UTF-8.
pub fn message_envelope_json(parts: &MessageParts<'_>) -> String {
    let id = serde_json::to_string(parts.id).expect("string serializes");
    let role = serde_json::to_string(parts.kind).expect("string serializes");
    let content = serde_json::to_string(parts.display_content).expect("string serializes");
    let status = serde_json::to_string(parts.status).expect("string serializes");
    let turn_id = match parts.turn_id {
        Some(id) => serde_json::to_string(id).expect("string serializes"),
        None => "null".to_owned(),
    };
    let created_at = serde_json::to_string(parts.created_at).expect("string serializes");
    let completed_at = match parts.completed_at {
        Some(stamp) => serde_json::to_string(stamp).expect("string serializes"),
        None => "null".to_owned(),
    };
    drf_escape(&format!(
        "{{\"id\":{},\"role\":{},\"content\":{},\"status\":{},\"seq\":{},\"turn_id\":{},\"payload\":{},\"created_at\":{},\"completed_at\":{}}}",
        id,
        role,
        content,
        status,
        parts.seq,
        turn_id,
        parts.payload_json,
        created_at,
        completed_at
    ))
}

/// Delete a finished turn's delta events (`events.py:143-145`); completed
/// content lives in the message rows.
pub fn prune_turn_deltas_sql(turn_id: &str) -> String {
    let table = assistant_event::TABLE;
    format!(
        "DELETE FROM \"{table}\" WHERE (\"{table}\".\"turn_id\" = {turn_id} \
         AND \"{table}\".\"kind\" = '{DELTA_KIND}')"
    )
}

/// `PUBLISH <channel> <serialize_event JSON>` frame (`events.py:53-70`).
/// The executor publishes this and swallows every Redis failure (log and
/// return, never raise): `redis_instance()` raising, a `None` client, and a
/// failed `publish` all end the call silently.
pub fn publish_frame(thread_id: &str, event_json: &str) -> (String, String) {
    (event_channel(thread_id), event_json.to_owned())
}

/// History window (`history.py:34-37`): loop threads replay at most
/// `loop_max` turns, everything else at most `chat_max`, lower-bounded at 1.
pub fn history_limit(kind: ThreadKind, chat_max: i64, loop_max: i64) -> i64 {
    match kind {
        ThreadKind::Loop => loop_max.max(1),
        ThreadKind::Chat => chat_max.max(1),
    }
}

/// Prior-turn query (`history.py:39-43`): newest `limit` completed turns with
/// non-null `model_messages`, ordered newest-first with the `-id` tie-break
/// for same-microsecond turns. Callers take this window then reverse it to
/// chronological (`blobs.reverse()`, `history.py:44`).
pub fn load_history_sql(thread_id: &str, limit: i64) -> String {
    let table = assistant_turn::TABLE;
    let status = HISTORY_STATUS.as_str();
    format!(
        "SELECT \"{table}\".\"model_messages\" FROM \"{table}\" \
         WHERE (\"{table}\".\"thread_id\" = {thread_id} \
         AND \"{table}\".\"model_messages\" IS NOT NULL \
         AND \"{table}\".\"status\" = {status}) \
         ORDER BY \"{table}\".\"created_at\" DESC, \"{table}\".\"id\" DESC LIMIT {limit}"
    )
}

/// Fold stored `model_messages` blobs into one history list
/// (`history.py:45-52`): falsy blobs are skipped, and a blob that fails
/// validation is logged and skipped (`except Exception: continue`), so a
/// storage-format change never crashes a turn. Here the blobs are already
/// JSON — validation is "a JSON array of messages", anything else is
/// skipped; per-message schema checks belong to the LLM runtime layer.
pub fn decode_history_blobs(blobs: &[Value]) -> Vec<Value> {
    let mut messages = Vec::new();
    for blob in blobs {
        if let Value::Array(items) = blob {
            messages.extend(items.iter().cloned());
        }
    }
    messages
}

/// Serialize new turn messages for storage (`history.py:56-60`,
/// `ModelMessagesTypeAdapter.dump_python(..., mode='json')`): values already
/// in JSON form pass through unchanged.
pub fn dump_new_messages_json(new_messages: Vec<Value>) -> Vec<Value> {
    new_messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/queries.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn event_channel_matches_fixture() {
        let events = &fixture()["events"];
        assert_eq!(
            event_channel("tid-1"),
            events["event_channel"]["vector"].as_str().expect("vector")
        );
        assert!(event_channel("tid-1")
            .starts_with(events["event_channel"]["prefix"].as_str().expect("prefix")));
    }

    #[test]
    fn serialize_event_keys_and_vector_match_fixture() {
        let events = &fixture()["events"]["serialize_event"];
        let keys: Vec<&str> = events["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| key.as_str().expect("key"))
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "thread",
                "message",
                "seq",
                "kind",
                "payload",
                "created_at"
            ]
        );
        let vector = &events["vector"];
        // Payload crosses pre-rendered, exactly as `json.dumps` renders the
        // fixture payload (`{"t": 1}`, spaced separators).
        let payload_json = "{\"t\": 1}";
        let parts = EventParts {
            id: vector["id"].as_i64().expect("id"),
            thread_id: vector["thread"].as_str().expect("thread"),
            message_id: vector["message"].as_str(),
            seq: vector["seq"].as_i64().expect("seq"),
            kind: vector["kind"].as_str().expect("kind"),
            payload_json,
            created_at: vector["created_at"].as_str().expect("created_at"),
        };
        // Byte-for-byte `json.dumps(serialize_event(...), default=str)`
        // (verified live): `(', ', ': ')` separators, ASCII-escaped.
        assert_eq!(
            serialize_event_json(&parts),
            "{\"id\": 7, \"thread\": \"0c46aebe-0915-458b-ad1c-3a1d7b8de050\", \
             \"message\": null, \"seq\": 1, \"kind\": \"turn_started\", \
             \"payload\": {\"t\": 1}, \"created_at\": \"2026-09-29T03:39:16.065834+00:00\"}"
        );
    }

    #[test]
    fn dumps_string_matches_json_dumps() {
        // (input, `json.dumps` output), probed live.
        const STRINGS: &[(&str, &str)] = &[
            ("hi", "\"hi\""),
            ("a\"q\\b", "\"a\\\"q\\\\b\""),
            ("line\nbreak", "\"line\\nbreak\""),
            ("tab\there", "\"tab\\there\""),
            ("cr\rr", "\"cr\\rr\""),
            ("back\u{08}form\u{0c}f", "\"back\\bform\\ff\""),
            ("ctrl\u{01}\u{7f}", "\"ctrl\\u0001\\u007f\""),
            ("\u{e9}\u{2603}", "\"\\u00e9\\u2603\""),
            ("emoji\u{1f600}", "\"emoji\\ud83d\\ude00\""),
            ("sep\u{2028}end", "\"sep\\u2028end\""),
            ("sep\u{2029}end", "\"sep\\u2029end\""),
            ("quote\"and\u{27}", "\"quote\\\"and'\""),
            ("mix \u{e9}\n\0", "\"mix \\u00e9\\n\\u0000\""),
        ];
        for (input, expected) in STRINGS {
            assert_eq!(&dumps_string(input), expected, "{input:?}");
        }
    }

    #[test]
    fn kind_truncation_counts_chars_not_bytes() {
        assert_eq!(truncate_kind(&"a".repeat(64)), "a".repeat(64));
        assert_eq!(truncate_kind(&"a".repeat(65)), "a".repeat(64));
        // 70 snowmen: Python kind[:64] keeps 64 code points (192 bytes).
        let wide = "☃".repeat(70);
        let cut = truncate_kind(&wide);
        assert_eq!(cut.chars().count(), 64);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(cut, "☃".repeat(64));
    }

    #[test]
    fn message_envelope_keys_and_vector_match_fixture() {
        let events = &fixture()["events"]["message_envelope"];
        let keys: Vec<&str> = events["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| key.as_str().expect("key"))
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "role",
                "content",
                "status",
                "seq",
                "turn_id",
                "payload",
                "created_at",
                "completed_at"
            ]
        );
        let vector = &events["vector"];
        // Payload crosses pre-rendered with the DRF rules (compact), so the
        // assembled envelope matches `Response(envelope).render().content`.
        let parts = MessageParts {
            id: vector["id"].as_str().expect("id"),
            kind: "user",
            display_content: vector["content"].as_str().expect("content"),
            status: vector["status"].as_str().expect("status"),
            seq: vector["seq"].as_i64().expect("seq"),
            turn_id: vector["turn_id"].as_str(),
            payload_json: "{\"a\":1}",
            created_at: vector["created_at"].as_str().expect("created_at"),
            completed_at: vector["completed_at"].as_str(),
        };
        assert_eq!(
            message_envelope_json(&parts),
            "{\"id\":\"869b748c-b9e0-40e7-84c3-5e2d9b9c47a2\",\"role\":\"user\",\
             \"content\":\"hi\",\"status\":\"completed\",\"seq\":3,\"turn_id\":null,\
             \"payload\":{\"a\":1},\"created_at\":\"2026-09-29T03:39:16.065775+00:00\",\
             \"completed_at\":null}"
        );
    }

    #[test]
    fn envelope_keeps_utf8_and_escapes_line_separators() {
        // DRF over `UNICODE_JSON=True`: raw UTF-8, plus the renderer's
        // U+2028/U+2029 post-pass (`renderers.py:108-111`).
        let parts = MessageParts {
            id: "m",
            kind: "assistant",
            display_content: "\u{e9}\u{2603}\u{2028}",
            status: "completed",
            seq: 1,
            turn_id: None,
            payload_json: "{}",
            created_at: "t",
            completed_at: None,
        };
        let rendered = message_envelope_json(&parts);
        assert!(
            rendered.contains("\"content\":\"\u{e9}\u{2603}\\u2028\""),
            "{rendered}"
        );
    }

    #[test]
    fn prune_and_publish_shapes() {
        let delete = prune_turn_deltas_sql("turn-1");
        assert!(delete.starts_with("DELETE FROM \"assistant_event\""));
        assert!(delete.contains("\"kind\" = 'assistant_delta'"));
        assert!(delete.contains("turn-1"));
        let (channel, payload) = publish_frame("tid-1", "{}");
        assert_eq!(channel, "assistant:thread:tid-1");
        assert_eq!(payload, "{}");
    }

    #[test]
    fn history_caps_and_sql_match_fixture() {
        let history = &fixture()["history"];
        assert_eq!(CHAT_HISTORY_MAX_TURNS, 40);
        assert_eq!(LOOP_HISTORY_MAX_TURNS, 5);
        assert_eq!(history_limit(ThreadKind::Chat, 40, 5), 40);
        assert_eq!(history_limit(ThreadKind::Loop, 40, 5), 5);
        assert_eq!(history_limit(ThreadKind::Chat, 0, 0), 1);
        assert_eq!(history_limit(ThreadKind::Loop, 40, -3), 1);
        assert_eq!(
            load_history_sql("33ade466-0600-43ed-b19a-9616ba72e19b", 40),
            history["load_history"]["sql"].as_str().expect("sql")
        );
    }

    #[test]
    fn history_blobs_decode_and_dump() {
        let blobs = vec![
            json!([{"role": "user", "content": "hi"}]),
            json!(null),
            json!({"not": "a list"}),
            json!([{"role": "assistant", "content": "yo"}]),
        ];
        assert_eq!(
            decode_history_blobs(&blobs),
            vec![
                json!({"role": "user", "content": "hi"}),
                json!({"role": "assistant", "content": "yo"}),
            ]
        );
        let messages = vec![json!({"role": "user"})];
        assert_eq!(dump_new_messages_json(messages.clone()), messages);
    }
}
